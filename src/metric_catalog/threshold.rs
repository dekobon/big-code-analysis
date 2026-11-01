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
                    (Scale::Ratio, _) => false,
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
            Scale::Ratio => None,
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
            Scale::Ratio => match self.info.direction {
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
    /// applied: this is the exact arithmetic, and
    /// [`soft_limit_rounded`](Self::soft_limit_rounded) is the limit a
    /// gate reads.
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
        // rounds twice: at 0.95 the latter gives 0.8550000000000009,
        // eight ulps off the decimal, which the result-scaled snap of
        // the #1621 review printed as 0.855001. The one-rounding form
        // keeps the exact limit within four ulps of the decimal, which
        // its ulp test pins.
        let step = |unit: f64| unit - ratio * unit;
        let limit = match (self.info.scale, self.info.direction) {
            (Scale::Interval { max, unit, .. }, Direction::LowerIsWorse) => {
                (self.limit + step(unit)).min(max).max(self.limit)
            }
            (Scale::Interval { min, unit, .. }, Direction::HigherIsWorse) => {
                (self.limit - step(unit)).max(min).min(self.limit)
            }
            (Scale::Ratio, Direction::HigherIsWorse) => self.limit * ratio,
            // A tiny ratio can overflow the quotient; keep it finite.
            (Scale::Ratio, Direction::LowerIsWorse) => (self.limit / ratio).min(f64::MAX),
        };
        Ok(Self {
            info: self.info,
            limit,
        })
    }

    /// The soft-tier threshold [`soft_limit`](Self::soft_limit) derives
    /// for a band `ratio` in `(0, 1]`, resolved onto the six-figure grid
    /// a gate reads it from and held on the hard limit's own side of it.
    ///
    /// Either arithmetic leaves a float residue — `7 · 0.95` is
    /// `6.6499999999999995`, `-50 + 0.855` is `-49.144999999999996` —
    /// that would print on every offender line and gate a hair off the
    /// decimal the configuration meant. The rounding that removes it
    /// moves the last figure toward the band's **strict** side — up for
    /// a floor, down for a ceiling — because a soft limit rounded the
    /// other way is a band that fires late: `loc.sloc = 13801` at `0.95`
    /// is `13110.95`, which the grid cannot hold, and read to nearest as
    /// `13111` the strict comparison let a file of 13111 lines through.
    /// It gates at `13110.9` instead, reporting exactly what the exact
    /// band reports. The move happens only when there is a real
    /// remainder to round: a limit within a few ulps of a grid position,
    /// measured against the magnitude of the arithmetic's operands, is
    /// already on it (`GRID_SNAP_ULPS`), which is what recovers the
    /// decimal product the float arithmetic missed (`6.65`, not
    /// `6.64999`); and a step that cancels the limit exactly
    /// (`-0.855 + 0.855`) is zero, not the `4.4e-16` the sum carries.
    ///
    /// A soft limit that did not move — every limit at `ratio == 1`, and
    /// a floor the domain clamp handed back unchanged — is returned as
    /// is, bitwise, before any rounding. A six-figure grid cannot hold a
    /// limit with more figures, so rounding it would gate the soft tier
    /// tighter than the hard one on a parity run (a ceiling of `1234564`
    /// read as `1234560`, a floor of `-14.77423532` as `-14.7742`).
    ///
    /// Whatever the rounding does to a limit that did move, the result
    /// is then held on the hard limit's side: a floor never below it, a
    /// ceiling never above it. The snap can take a floor far smaller
    /// than its unit down a tick, or a ceiling a few ulps under a hard
    /// limit with more figures than the grid holds up onto the tick
    /// above it, and without the clamp a valid configuration would
    /// resolve a soft tier looser than its hard one — the invariant
    /// `soft_limit` promises. The domain clamp runs in `soft_limit`,
    /// before the rounding, and nothing re-clamps after: the rounded
    /// limit stays inside the domain because every interval edge in the
    /// catalog is a grid point.
    ///
    /// # Errors
    ///
    /// [`LimitError::BadRatio`] when `ratio` is not finite or lies
    /// outside `(0, 1]`.
    pub fn soft_limit_rounded(&self, ratio: f64) -> Result<Self, LimitError> {
        let soft = self.soft_limit(ratio)?;
        // Bitwise: `soft_limit` returns the hard limit itself when it
        // does not move it, and `check` already folded `-0.0` into `0.0`.
        if soft.limit.to_bits() == self.limit.to_bits() {
            return Ok(soft);
        }
        // The additive step cancels against a limit of the other sign
        // (`-1 + 0.855`), so the residue the sum carries is sized by the
        // operands, not by the smaller result: snap on their scale. A
        // ratio-scale limit's soft limit is a product or quotient, whose
        // residue is a few ulps of the result itself, which
        // `round_sig_figs` already measures against; sizing it by the
        // hard limit instead would let a tiny ratio's genuine remainder
        // pass for residue (`799999999 · 1e-8` snapped up to `8`, late).
        let operand = match self.info.scale {
            Scale::Interval { unit, .. } => self.limit.abs().max(unit),
            Scale::Ratio => 0.0,
        };
        let direction = self.info.direction;
        let rounding = Rounding::Strict { direction, operand };
        let rounded = round_sig_figs(soft.limit, rounding);
        let limit = match direction {
            Direction::LowerIsWorse => rounded.max(self.limit),
            Direction::HigherIsWorse => rounded.min(self.limit),
        };
        Ok(Self {
            info: self.info,
            limit,
        })
    }

    /// `value` rounded to nearest on the six-figure grid
    /// [`soft_limit_rounded`](Self::soft_limit_rounded) resolves a moved
    /// limit to, so a quantity derived from one — the step that moved it,
    /// the distance it moved — reads on the same grid instead of carrying
    /// the residue its own arithmetic leaves (`1 - 0.95` is
    /// `0.050000000000000044`). Zero and the non-finite values are
    /// returned as they are.
    #[must_use]
    pub fn round_to_soft_grid(value: f64) -> f64 {
        round_sig_figs(value, Rounding::Nearest)
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
            Scale::Ratio if recorded == 0.0 => Change::FromZero,
            Scale::Ratio => Change::Percent(worse_by / recorded.abs() * 100.0),
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
            Scale::Ratio if delta == 0.0 => 0.0,
            Scale::Ratio => delta / recorded.abs().max(v.abs()),
        }
    }
}

/// Significant figures a soft limit is resolved to. Trims the float
/// residue of the band arithmetic (`7 · 0.95` is `6.6499999999999995`)
/// to a readable `6.65` while preserving full precision for the largest
/// limits seen in practice (`halstead.effort`, on the order of
/// `50000`). Six figures hold every product of a count limit of up to
/// four digits and a two-figure ratio exactly. A longer product does not
/// fit, and moves to the grid tick on its strict side — under one part
/// in a hundred thousand away — so the soft tier can fire up to a tick
/// early and never late: `loc.sloc = 13801` at `0.95` is `13110.95` and
/// gates at `13110.9`, the same offenders, since no count lies between;
/// `1234567` at `0.95` is `1172838.65` and gates at `1172830`, which
/// also reports the counts between. The grid matches the `{:.6g}`
/// rounding of the CLI's retired `bca-self-scan-headroom.py` helper
/// (#373), so a soft-gate offender line renders byte-for-byte the same
/// whether the band came from a blanket ratio or a per-metric scale
/// factor.
const SOFT_LIMIT_SIG_FIGS: i32 = 6;

/// How far from an exact sig-fig grid position a scaled limit may sit
/// and still count as being *on* it, in ulps of the arithmetic's largest
/// operand.
///
/// The double for a limit like `8.3` is a hair above the decimal it
/// prints as, so `8.3 · 1e5` is `830000.0000000001` — one ulp over the
/// grid — and a bare `ceil` would promote it to the next whole tick,
/// as a bare `floor` would demote `7 · 0.95`, which the product leaves
/// an ulp *under* `6.65`. The additive interval arithmetic leaves the
/// same residue (`-50 + 0.855` is `-49.144999999999996`), and when the
/// step cancels against a floor of the other sign (`-1 + 0.855`) the
/// residue is a far larger share of the small result than of the
/// operands it came from — which is why the tolerance is measured on
/// the operands. Four ulps clears those cases with room to spare and is
/// below any remainder a realistic limit carries; a limit far smaller
/// than its unit can carry a genuine remainder inside it, which the
/// clamp in [`Threshold::soft_limit_rounded`] keeps from crossing the
/// hard limit.
const GRID_SNAP_ULPS: f64 = 4.0;

/// How [`round_sig_figs`] treats the last significant figure.
#[derive(Clone, Copy)]
enum Rounding {
    /// To the nearest grid tick: a display value.
    Nearest,
    /// To the next tick on the strict side of a soft limit for a metric
    /// of `direction` — up for a floor, down for a ceiling — unless
    /// already on the grid, so the band never fires late. `operand` is
    /// the largest magnitude among the arithmetic's inputs, which bounds
    /// the float residue a grid position may carry.
    Strict { direction: Direction, operand: f64 },
}

/// Round `value` to [`SOFT_LIMIT_SIG_FIGS`] significant figures, by
/// `rounding`. See [`Threshold::soft_limit_rounded`] for why a soft
/// limit rounds toward its strict side.
fn round_sig_figs(value: f64, rounding: Rounding) -> f64 {
    // `log10(0)` is `-inf`; short-circuit the degenerate inputs so the
    // magnitude maths below only sees finite, non-zero values.
    if value == 0.0 || !value.is_finite() {
        return value;
    }
    // `log10` of a finite, non-zero f64 lies in roughly [-323, 308], so
    // its floor always fits an i32 — the truncating cast cannot lose
    // information here.
    #[allow(clippy::cast_possible_truncation)]
    let magnitude = value.abs().log10().floor() as i32;
    let decimals = (SOFT_LIMIT_SIG_FIGS - 1) - magnitude;
    let factor = 10f64.powi(decimals);
    let ticks = value * factor;
    let rounded = match rounding {
        // A step that cancels the limit exactly (`-0.855 + 0.855`) leaves
        // a residue the size of the operands' ulp, not of the result; on
        // the residue's own grid it would survive as `4.4e-16`. Within the
        // operand tolerance of zero, the limit *is* zero.
        Rounding::Strict { operand, .. }
            if value.abs() <= GRID_SNAP_ULPS * f64::EPSILON * operand =>
        {
            0.0
        }
        Rounding::Strict { direction, operand } => {
            strict_off_grid(ticks, direction, operand.max(value.abs()) * factor) / factor
        }
        Rounding::Nearest => ticks.round() / factor,
    };
    // Two inputs leave the grid. A subnormal `value` overflows `factor`
    // to infinity, so `ticks` and the quotient are infinite or NaN; it is
    // already far below any rounding granularity. And a value within
    // half a tick of `f64::MAX` rounds past it to infinity, where
    // `soft_limit` caps a ratio-scale floor's quotient at `f64::MAX`
    // precisely to keep it finite. Hand either back unrounded rather than
    // poison a threshold set with a non-finite limit.
    if rounded.is_finite() { rounded } else { value }
}

/// `ticks` rounded to the tick on the strict side of a soft limit for a
/// metric of `direction` — `ceil` for a floor, `floor` for a ceiling —
/// except that a value within [`GRID_SNAP_ULPS`] of a grid position,
/// measured against `scale`, the operand magnitude in tick units, is
/// already on it and rounds to it instead.
fn strict_off_grid(ticks: f64, direction: Direction, scale: f64) -> f64 {
    let nearest = ticks.round();
    if (ticks - nearest).abs() <= GRID_SNAP_ULPS * f64::EPSILON * scale {
        return nearest;
    }
    match direction {
        Direction::LowerIsWorse => ticks.ceil(),
        Direction::HigherIsWorse => ticks.floor(),
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
