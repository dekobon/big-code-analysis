//! A validated metric limit and the value arithmetic it implies
//! (issue #1621).
//!
//! Every question a threshold surface asks of a value — does it breach,
//! by how much, how does it rank against another breach, where does the
//! soft tier sit, how far has it moved from its baseline record — has
//! an answer that depends on the metric's [`Direction`] *and* its
//! [`Scale`]. [`Threshold`] answers all of them in one place so the
//! front-ends stop re-deriving them with ratio arithmetic that is
//! meaningless on the interval-scale `mi.*` family.

use std::cmp::Ordering;
use std::fmt;

use super::{Direction, METRICS, MetricInfo, Scale, lookup};

/// A metric limit that has been checked against the metric's
/// [`Scale`]: the metric id is known, the limit is finite, and a
/// ratio-scale limit is non-negative.
///
/// Built with [`Threshold::check`]. Fields are private so every
/// `Threshold` in existence has passed that validation; read them back
/// through [`id`](Self::id), [`info`](Self::info) and
/// [`limit`](Self::limit).
///
/// Equality compares the metric id and the limit (`f64` `==`).
#[derive(Debug, Clone, Copy)]
pub struct Threshold {
    info: &'static MetricInfo,
    limit: f64,
}

impl PartialEq for Threshold {
    fn eq(&self, other: &Self) -> bool {
        self.info.id == other.info.id && self.limit == other.limit
    }
}

/// A successfully validated [`Threshold`], together with any advisory
/// the limit raised.
///
/// A limit that is legal but cannot do what its author presumably
/// meant — outside the metric's domain, or sitting on the domain edge
/// where it can never fire — is accepted with a [`LimitWarning`] rather
/// than rejected, because the 2.x CLI already accepts such limits and
/// tightening the grammar is reserved for a major release.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Checked {
    /// The validated threshold.
    pub threshold: Threshold,
    /// An advisory to surface to the user, if the limit raised one.
    pub warning: Option<LimitWarning>,
}

/// Why a limit was rejected by [`Threshold::check`] or a soft ratio by
/// [`Threshold::soft_limit`].
///
/// The [`Display`](fmt::Display) rendering is a bare message with no
/// severity prefix; the presenting layer adds one.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum LimitError {
    /// The metric id is not in [`METRICS`].
    #[non_exhaustive]
    UnknownMetric {
        /// The id as given.
        id: String,
    },
    /// The limit is NaN or infinite, which would silently disable a
    /// strict comparison gate.
    #[non_exhaustive]
    NotFinite {
        /// The metric id.
        id: &'static str,
        /// The rejected limit.
        limit: f64,
    },
    /// A negative limit on a [`Scale::Ratio`] metric, whose values are
    /// counts and cannot be negative.
    #[non_exhaustive]
    Negative {
        /// The metric id.
        id: &'static str,
        /// The rejected limit.
        limit: f64,
    },
    /// A soft-tier ratio outside `(0, 1]`, or not finite.
    #[non_exhaustive]
    BadRatio {
        /// The rejected ratio.
        ratio: f64,
    },
}

impl fmt::Display for LimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMetric { id } => write!(f, "unknown metric {id:?}"),
            Self::NotFinite { id, limit } if !matches!(super::scale(id), Some(Scale::Ratio)) => {
                write!(f, "limit for {id:?} must be a finite number; got {limit}")
            }
            // The wording the CLI's threshold parser used before #1621, kept
            // so its tests and users' log greps survive the migration.
            Self::NotFinite { id, limit } | Self::Negative { id, limit } => write!(
                f,
                "limit for {id:?} must be a finite non-negative number; got {limit}"
            ),
            Self::BadRatio { ratio } => write!(f, "soft ratio must be in (0, 1]; got {ratio}"),
        }
    }
}

impl std::error::Error for LimitError {}

/// An advisory raised by a limit [`Threshold::check`] accepted.
///
/// The [`Display`](fmt::Display) rendering is a bare message with no
/// severity prefix; the presenting layer adds one.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum LimitWarning {
    /// The limit lies outside the metric's [`Scale::Interval`] domain,
    /// so the gate either fires on every value or on none.
    #[non_exhaustive]
    OutOfDomain {
        /// The metric id.
        id: &'static str,
        /// The configured limit.
        limit: f64,
        /// The domain's lower bound.
        min: f64,
        /// The domain's upper bound.
        max: f64,
    },
    /// The limit sits on the domain edge at the metric's bad end — a
    /// floor at the minimum or a ceiling at the maximum — so no value
    /// can ever be strictly past it.
    #[non_exhaustive]
    CannotFire {
        /// The metric id.
        id: &'static str,
        /// The configured limit.
        limit: f64,
    },
}

/// Whether metric `id`'s limit is a floor (lower is worse) rather than a
/// ceiling.
fn is_floor(id: &str) -> bool {
    lookup(id).is_some_and(|m| m.direction == Direction::LowerIsWorse)
}

impl fmt::Display for LimitWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::OutOfDomain {
                id,
                limit,
                min,
                max,
            } => {
                let fires_on_all = if is_floor(id) {
                    limit > max
                } else {
                    limit < min
                };
                let effect = if fires_on_all {
                    "it fires on every value"
                } else {
                    "it can never fire"
                };
                write!(
                    f,
                    "limit {id}={limit} is outside the metric's range [{min}, {max}]; {effect}"
                )
            }
            Self::CannotFire { id, limit } => {
                let edge = if is_floor(id) { "floor" } else { "ceiling" };
                write!(
                    f,
                    "limit {id}={limit} cannot fire: {limit} is the metric's {edge}"
                )?;
                if let Some(alt) = unbounded_sibling(id) {
                    write!(f, "; gate {alt} instead")?;
                }
                Ok(())
            }
        }
    }
}

/// A metric in the same family whose domain is unbounded on the side a
/// `CannotFire` limit sits on — the metric to gate instead
/// (`mi.original` for `mi.visual_studio`). The unbounded-side test is
/// not observable in today's catalog, where `mi.original` is also the
/// family's first row; it keeps a future bounded row from being named.
fn unbounded_sibling(id: &str) -> Option<&'static str> {
    let info = lookup(id)?;
    METRICS
        .iter()
        .find(|m| {
            m.family == info.family
                && m.direction == info.direction
                && match (m.scale, m.direction) {
                    (Scale::Interval { min, .. }, Direction::LowerIsWorse) => min.is_infinite(),
                    (Scale::Interval { max, .. }, Direction::HigherIsWorse) => max.is_infinite(),
                    _ => false,
                }
        })
        .map(|m| m.id)
}

/// How a value has moved relative to its baseline record, oriented so
/// a positive number means *worse*. Returned by [`Threshold::change`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Change {
    /// A [`Scale::Ratio`] metric's change as a percentage of the record.
    Percent(f64),
    /// A [`Scale::Interval`] metric's change in the metric's own points;
    /// a percentage of an arbitrary zero means nothing.
    Points(f64),
    /// A [`Scale::Ratio`] metric whose record was zero: any growth is an
    /// unbounded percentage.
    FromZero,
    /// The current value is NaN.
    NaN,
}

impl Threshold {
    /// Validate `limit` for the metric `id`.
    ///
    /// # Errors
    ///
    /// - [`LimitError::UnknownMetric`] when `id` is not in [`METRICS`];
    /// - [`LimitError::NotFinite`] for a NaN or infinite limit;
    /// - [`LimitError::Negative`] for a negative limit on a
    ///   [`Scale::Ratio`] metric.
    ///
    /// A [`Scale::Interval`] limit outside the domain, or on the domain
    /// edge at the metric's bad end, is accepted with a
    /// [`LimitWarning`] in [`Checked::warning`].
    pub fn check(id: &str, limit: f64) -> Result<Checked, LimitError> {
        let info = lookup(id).ok_or_else(|| LimitError::UnknownMetric { id: id.to_owned() })?;
        if !limit.is_finite() {
            return Err(LimitError::NotFinite { id: info.id, limit });
        }
        // `-0.0` passes every comparison as `0.0` but renders as `-0` and
        // divides to `-inf`; store the zero a user meant.
        let limit = if limit == 0.0 { 0.0 } else { limit };
        if !matches!(info.scale, Scale::Interval { .. }) && limit < 0.0 {
            return Err(LimitError::Negative { id: info.id, limit });
        }
        let threshold = Self { info, limit };
        Ok(Checked {
            warning: threshold.warning(),
            threshold,
        })
    }

    /// The metric id this threshold gates.
    #[must_use]
    pub fn id(&self) -> &'static str {
        self.info.id
    }

    /// The catalog entry of the gated metric.
    #[must_use]
    pub fn info(&self) -> &'static MetricInfo {
        self.info
    }

    /// The configured limit.
    #[must_use]
    pub fn limit(&self) -> f64 {
        self.limit
    }

    /// The advisory this limit raises, if any: an interval-scale limit
    /// outside its domain, or on the domain edge at the metric's bad end
    /// where a strict comparison cannot fire. The same advisory
    /// [`check`](Self::check) returns in [`Checked::warning`], asked of a
    /// threshold already in hand — a soft-tier limit
    /// [`soft_limit`](Self::soft_limit) derived, say.
    #[must_use]
    pub fn warning(&self) -> Option<LimitWarning> {
        match self.info.scale {
            Scale::Interval { min, max, .. } => domain_warning(self.info, self.limit, min, max),
            _ => None,
        }
    }

    /// Whether `v` breaches the limit: strictly greater for a
    /// higher-is-worse metric, strictly less for a lower-is-worse one.
    /// A value equal to the limit does not breach, and NaN never does.
    #[must_use]
    pub fn breaches(&self, v: f64) -> bool {
        match self.info.direction {
            Direction::HigherIsWorse => v > self.limit,
            Direction::LowerIsWorse => v < self.limit,
        }
    }

    /// The signed distance of `v` past the limit in the bad direction:
    /// positive for a breach, zero on the limit, negative inside it.
    #[must_use]
    pub fn excess(&self, v: f64) -> f64 {
        match self.info.direction {
            Direction::HigherIsWorse => v - self.limit,
            Direction::LowerIsWorse => self.limit - v,
        }
    }

    /// How badly `v` breaches the limit, as a multiple where `1.0` is
    /// "on the limit". Comparable across metrics, so a ranking of
    /// offenders can sort on it directly.
    ///
    /// - [`Scale::Ratio`] ceiling with a positive limit: exactly
    ///   `v / limit` (bitwise; the ratio users already read as
    ///   "2× over"). A ratio-scale floor, which the catalog has none of
    ///   today, inverts it to `limit / v`.
    /// - [`Scale::Ratio`] with a zero limit (or a floor breached by a
    ///   zero value): `f64::INFINITY`. Every such breach saturates;
    ///   tie-break on
    ///   [`excess`](Self::excess), as [`cmp_severity`](Self::cmp_severity)
    ///   does.
    /// - [`Scale::Interval`]: `1 + excess / unit`, one step per unit of
    ///   badness past the limit. Finite for any finite `v`, whatever its
    ///   sign or the limit's.
    ///
    /// Defined for a **breaching** value. Callers rank only breaches;
    /// for a value that does not breach the result is unspecified.
    #[must_use]
    pub fn severity(&self, v: f64) -> f64 {
        match self.info.scale {
            Scale::Interval { unit, .. } => 1.0 + self.excess(v) / unit,
            _ => match self.info.direction {
                Direction::HigherIsWorse if self.limit > 0.0 => v / self.limit,
                Direction::LowerIsWorse if v > 0.0 => self.limit / v,
                _ => f64::INFINITY,
            },
        }
    }

    /// Order two breaching values of this metric: [`Ordering::Greater`]
    /// means `a` is the worse breach. Compares
    /// [`severity`](Self::severity) and breaks a tie (including two
    /// saturated zero-limit severities) on [`excess`](Self::excess).
    #[must_use]
    pub fn cmp_severity(&self, a: f64, b: f64) -> Ordering {
        self.cmp_breach(a, self, b)
    }

    /// Order the breach `a` of this threshold against the breach `b` of
    /// `other`, which may gate a different metric or limit:
    /// [`Ordering::Greater`] means `a` is the worse breach. The same key
    /// as [`cmp_severity`](Self::cmp_severity) — severity, then excess —
    /// so a ranking across metrics and a ranking within one agree.
    #[must_use]
    pub fn cmp_breach(&self, a: f64, other: &Self, b: f64) -> Ordering {
        self.severity(a)
            .total_cmp(&other.severity(b))
            .then_with(|| self.excess(a).total_cmp(&other.excess(b)))
    }

    /// Whether `ratio` is a soft-tier band [`soft_limit`](Self::soft_limit)
    /// accepts: finite and in `(0, 1]`. `1.0` is the identity; a ratio
    /// above it would make the soft tier looser than the hard gate, and
    /// `0`, a negative or NaN is a usage error.
    #[must_use]
    pub fn is_soft_ratio(ratio: f64) -> bool {
        // NaN fails both comparisons, and an infinity one of them.
        0.0 < ratio && ratio <= 1.0
    }

    /// The stricter soft-tier threshold for a band `ratio` in `(0, 1]`.
    ///
    /// - [`Scale::Ratio`]: `limit · ratio` for a ceiling,
    ///   `limit / ratio` for a floor.
    /// - [`Scale::Interval`]: the limit moved `(1 - ratio) · unit` into
    ///   the healthy side — up for a floor, down for a ceiling — and
    ///   clamped at the domain's healthy edge (`max` for a floor, `min`
    ///   for a ceiling), though never past the hard limit itself, so a
    ///   soft tier is never looser than its hard tier even for an
    ///   out-of-domain limit. Multiplying a floor of `-50` would
    ///   *loosen* it, which is why an interval metric cannot share the
    ///   ratio arithmetic.
    ///
    /// `ratio == 1` returns the hard threshold unchanged. No rounding is
    /// applied; presentation rounding is the caller's concern.
    ///
    /// # Errors
    ///
    /// [`LimitError::BadRatio`] when `ratio` is not finite or lies
    /// outside `(0, 1]`.
    pub fn soft_limit(&self, ratio: f64) -> Result<Self, LimitError> {
        if !Self::is_soft_ratio(ratio) {
            return Err(LimitError::BadRatio { ratio });
        }
        // `unit - ratio * unit` rounds once where `(1 - ratio) * unit`
        // rounds twice: at 0.95 the latter gives 0.8550000000000009, a
        // residue the CLI's sig-fig grid cannot absorb near zero, so a
        // floor of 0 printed as 0.855001.
        let step = |unit: f64| unit - ratio * unit;
        let limit = match (self.info.scale, self.info.direction) {
            (Scale::Interval { max, unit, .. }, Direction::LowerIsWorse) => {
                (self.limit + step(unit)).min(max).max(self.limit)
            }
            (Scale::Interval { min, unit, .. }, Direction::HigherIsWorse) => {
                (self.limit - step(unit)).max(min).min(self.limit)
            }
            (_, Direction::HigherIsWorse) => self.limit * ratio,
            // A tiny ratio can overflow the quotient; keep it finite.
            (_, Direction::LowerIsWorse) => (self.limit / ratio).min(f64::MAX),
        };
        Ok(Self {
            info: self.info,
            limit,
        })
    }

    /// How `v` has moved from its baseline `recorded` value, positive
    /// when worse. A [`Scale::Ratio`] metric reports a percentage of the
    /// record (or [`Change::FromZero`] for a zero record); a
    /// [`Scale::Interval`] metric reports points, since a percentage of
    /// an arbitrary zero is meaningless. A NaN `v` is [`Change::NaN`].
    #[must_use]
    pub fn change(&self, recorded: f64, v: f64) -> Change {
        if v.is_nan() {
            return Change::NaN;
        }
        let worse_by = match self.info.direction {
            Direction::HigherIsWorse => v - recorded,
            Direction::LowerIsWorse => recorded - v,
        };
        match self.info.scale {
            Scale::Interval { .. } => Change::Points(worse_by),
            _ if recorded == 0.0 => Change::FromZero,
            _ => Change::Percent(worse_by / recorded.abs() * 100.0),
        }
    }

    /// A non-negative magnitude of the move from `recorded` to `v`. The
    /// ratio form lies in `[0, 1]`; the interval form counts units and is
    /// unbounded, so a ranking across the two scales favours a large
    /// `mi.*` move.
    ///
    /// - [`Scale::Ratio`]: `|recorded - v| / max(|recorded|, |v|)`, in
    ///   `[0, 1]`, and `0` when both are zero.
    /// - [`Scale::Interval`]: `|recorded - v| / unit`, so a hundredth of
    ///   a point near an arbitrary zero stays a hundredth of a step.
    #[must_use]
    pub fn drift(&self, recorded: f64, v: f64) -> f64 {
        let delta = (recorded - v).abs();
        match self.info.scale {
            Scale::Interval { unit, .. } => delta / unit,
            _ if delta == 0.0 => 0.0,
            _ => delta / recorded.abs().max(v.abs()),
        }
    }
}

/// The advisory for an interval-scale limit: outside the domain, or on
/// the edge at the bad end where a strict comparison cannot fire.
fn domain_warning(
    info: &'static MetricInfo,
    limit: f64,
    min: f64,
    max: f64,
) -> Option<LimitWarning> {
    if limit < min || limit > max {
        return Some(LimitWarning::OutOfDomain {
            id: info.id,
            limit,
            min,
            max,
        });
    }
    let on_bad_edge = match info.direction {
        Direction::LowerIsWorse => limit <= min,
        Direction::HigherIsWorse => limit >= max,
    };
    on_bad_edge.then_some(LimitWarning::CannotFire { id: info.id, limit })
}

#[cfg(test)]
#[path = "threshold_tests.rs"]
mod tests;
