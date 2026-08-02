//! The soft threshold tier: its limit forms, their parsing, and the
//! ratio scaling shared with `--headroom` (issue #375).
//!
//! `bca check --tier=soft` gates against an early-warning band that
//! fires *before* the hard `[thresholds]` limits. A `[thresholds.soft]`
//! entry is either an absolute number or a `"<ratio>x"` string scaling
//! the metric's hard limit, and the scale form cannot be resolved until
//! the manifest and `--config` layers have merged — which is why
//! [`SoftLimit`] is a parsed-but-unresolved value rather than an `f64`.
//!
//! Sits alongside [`crate::threshold_lang`], the per-language tier;
//! [`crate::thresholds`] owns the metric registry and the evaluation
//! engine both feed.

/// Reserved key inside `[thresholds]` that introduces the soft-tier
/// sub-table (`[thresholds.soft]`). Every other key in the table is a
/// hard-limit metric name. No metric is named `soft`, so the reservation
/// never collides with a real threshold.
pub(crate) const SOFT_SUBTABLE_KEY: &str = "soft";

/// One soft-tier limit, before resolution against the hard tier.
///
/// `[thresholds.soft]` values are either a plain number (an absolute soft
/// limit) or a `"<ratio>x"` string (scale the metric's hard limit by
/// `ratio`). The scale form is resolved lazily because it needs the
/// merged hard limit, which is only known after the manifest and
/// `--config` layers combine — see [`SoftLimit::resolve`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SoftLimit {
    /// An explicit soft limit, used as-is.
    Absolute(f64),
    /// A factor in `(0, 1]` applied to the metric's hard limit.
    Scale(f64),
}

impl SoftLimit {
    /// Resolve to a concrete limit. `Absolute` ignores `hard`; `Scale`
    /// tightens the metric's hard limit by its factor, erroring when no
    /// hard limit exists for the metric to scale (a scale factor
    /// relative to nothing is meaningless).
    ///
    /// `name` must already be canonical (#1165), because the scaling
    /// direction is looked up from it — see [`scale_threshold`].
    pub(crate) fn resolve(self, name: &str, hard: Option<f64>) -> Result<f64, String> {
        match self {
            Self::Absolute(value) => Ok(value),
            Self::Scale(factor) => {
                let base = hard.ok_or_else(|| {
                    format!(
                        "[thresholds.soft] {name:?} uses scale-relative syntax but no \
                         hard [thresholds] limit exists for {name:?} to scale; give it an \
                         absolute soft limit or add a hard limit first"
                    )
                })?;
                Ok(scale_threshold(
                    base,
                    factor,
                    crate::thresholds::metric_is_lower_is_worse(name),
                ))
            }
        }
    }
}

/// Significant figures retained when scaling a threshold by a ratio
/// (`--headroom` or a `[thresholds.soft]` `"<ratio>x"` factor). Trims
/// float-multiplication artifacts (e.g. `7 * 0.95 == 6.6499999999999995`)
/// to a readable `6.65` while preserving full precision for the largest
/// thresholds seen in practice (`halstead.effort`, on the order of
/// `50000`). At 6 figures the rounding error is far below any metric's
/// granularity, so the offender set is identical to the un-rounded
/// product. This matches the `{:.6g}` rounding the now-removed
/// `bca-self-scan-headroom.py` helper used (#373), so soft-gate offender
/// lines render byte-for-byte the same whether the band came from
/// `--headroom` or a per-metric scale factor.
const HEADROOM_SIG_FIGS: i32 = 6;

/// Whether `ratio` is a valid soft-tier scaling factor: the half-open
/// interval `(0, 1]`. `1.0` is the no-op identity (parity with the hard
/// gate); a factor `> 1` would make the soft tier *looser* than the hard
/// gate, which is never the early-warning intent; `0`, negatives, and
/// `NaN` (which fails both comparisons) are usage errors. Shared by the
/// `--headroom` scalar (CLI and `bca.toml`) and the `[thresholds.soft]`
/// `"<ratio>x"` form so the accepted range is defined in exactly one
/// place; callers compose their own context-specific error message.
pub(crate) fn is_valid_scale_ratio(ratio: f64) -> bool {
    0.0 < ratio && ratio <= 1.0
}

/// Tighten a threshold `limit` by `ratio`, rounding to
/// [`HEADROOM_SIG_FIGS`] significant figures. `ratio` is assumed already
/// validated (see [`is_valid_scale_ratio`]) to lie in `(0, 1]`. Shared
/// by the `--headroom` scalar path and the `[thresholds.soft]`
/// scale-relative form so both round identically.
///
/// `ratio` scales the *band*, not the number: it always makes the soft
/// tier stricter than the hard gate, and which arithmetic does that
/// depends on the metric's direction (#1166). A higher-is-worse limit is
/// a ceiling, so tightening it means lowering it — multiply. A
/// lower-is-worse `mi.*` limit is a *floor*, so tightening it means
/// **raising** it — divide. Multiplying a floor lowers it, which put the
/// early-warning band *below* the hard gate and made `--tier=soft` a
/// silent no-op for the whole `mi.*` family.
///
/// The rounding is likewise direction-aware, and asymmetric on purpose.
/// A ceiling rounds to nearest: `limit * ratio` has an exact decimal
/// value that the float product misses by an ulp (`7 * 0.95` is
/// `6.6499999999999995`), so nearest-rounding *recovers* the true
/// product, and the resulting output parity is a contract (#373). A
/// floor's `limit / ratio` generally has no exact decimal at all
/// (`20 / 0.9` repeats), so the last figure is a real choice — and it
/// goes **up**, because a floor rounded down is a band that fires
/// marginally late, the same defect in miniature — but only when there
/// is a real remainder to round, see [`FLOOR_GRID_SNAP_ULPS`]. Rounding
/// up also keeps the resolved floor at or above the exact quotient, so
/// it can never land under the hard floor it is derived from and trip
/// [`ThresholdSet::build_tiered`](crate::thresholds::ThresholdSet::build_tiered)'s
/// soft-looser-than-hard guard.
pub(crate) fn scale_threshold(limit: f64, ratio: f64, lower_is_worse: bool) -> f64 {
    let scaled = if lower_is_worse {
        limit / ratio
    } else {
        limit * ratio
    };
    // `log10(0)` is `-inf`; short-circuit the degenerate inputs so the
    // magnitude maths below only sees finite, non-zero values. A zero
    // `ratio` is rejected upstream, but were one to arrive the division
    // yields an infinity that lands here rather than panicking.
    if scaled == 0.0 || !scaled.is_finite() {
        return scaled;
    }
    // `log10` of a finite, non-zero f64 lies in roughly [-323, 308], so
    // its floor always fits an i32 — the truncating cast cannot lose
    // information here.
    #[allow(clippy::cast_possible_truncation)]
    let magnitude = scaled.abs().log10().floor() as i32;
    let decimals = (HEADROOM_SIG_FIGS - 1) - magnitude;
    let factor = 10f64.powi(decimals);
    // For an absurdly tiny limit the sig-fig `factor` overflows to
    // infinity, and `scaled * factor / factor` would be NaN. No real
    // metric threshold is subnormal, but guard it so the function is
    // total: such a value is already far below any rounding granularity,
    // so return it unrounded rather than poisoning the threshold set
    // with NaN.
    if !factor.is_finite() {
        return scaled;
    }
    let ticks = scaled * factor;
    if lower_is_worse {
        ceil_off_grid(ticks) / factor
    } else {
        ticks.round() / factor
    }
}

/// How far from an exact sig-fig grid position a scaled floor may sit
/// and still count as being *on* it, in ULPs of the value itself.
///
/// `limit / ratio` with `ratio == 1.0` reproduces `limit`, but the
/// double for a limit like `8.3` is a hair above the decimal it prints
/// as, so `8.3 * 1e5` is `830000.0000000001` — one ULP over the grid.
/// A bare `ceil` promotes that to the next whole tick and resolves the
/// soft floor to `8.30001`, which makes the documented `ratio == 1.0`
/// identity false and gates the soft tier above the hard one. Four ULPs
/// clears that single-ULP case with room to spare and is orders of
/// magnitude below the smallest genuine remainder a real ratio leaves
/// (`20 / 0.9` is `0.22` of a tick short).
const FLOOR_GRID_SNAP_ULPS: f64 = 4.0;

/// `ticks.ceil()`, except that a value within [`FLOOR_GRID_SNAP_ULPS`]
/// of a grid position is already on it and rounds to it instead.
fn ceil_off_grid(ticks: f64) -> f64 {
    let nearest = ticks.round();
    if (ticks - nearest).abs() <= FLOOR_GRID_SNAP_ULPS * f64::EPSILON * ticks.abs() {
        nearest
    } else {
        ticks.ceil()
    }
}

/// Parse one `[thresholds.soft]` value: a number (absolute) or a
/// `"<ratio>x"` scale string.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn parse_soft_value(name: &str, value: &toml::Value) -> Result<SoftLimit, String> {
    match value {
        toml::Value::Integer(i) => Ok(SoftLimit::Absolute(*i as f64)),
        toml::Value::Float(f) => Ok(SoftLimit::Absolute(*f)),
        toml::Value::String(s) => parse_scale_str(name, s),
        other => Err(format!(
            "[thresholds.soft] {name:?}: expected a number or a \"<ratio>x\" scale \
             string (e.g. \"0.95x\"), got {}",
            other.type_str()
        )),
    }
}

/// Parse a `"<ratio>x"` scale string (case-insensitive `x` suffix). The
/// factor must lie in `(0, 1]`, matching `--headroom`: a soft tier looser
/// than the hard tier is never the intent (the soft tier is an
/// early-warning band that fires *before* the hard gate).
fn parse_scale_str(name: &str, s: &str) -> Result<SoftLimit, String> {
    let trimmed = s.trim();
    let factor_str = trimmed
        .strip_suffix('x')
        .or_else(|| trimmed.strip_suffix('X'))
        .ok_or_else(|| {
            format!(
                "[thresholds.soft] {name:?}: scale string {s:?} must end in `x` (e.g. \"0.95x\")"
            )
        })?;
    let factor: f64 = factor_str
        .trim()
        .parse()
        .map_err(|e| format!("[thresholds.soft] {name:?}: invalid scale factor in {s:?}: {e}"))?;
    if !is_valid_scale_ratio(factor) {
        return Err(format!(
            "[thresholds.soft] {name:?}: scale factor must be in (0, 1]; got {factor}"
        ));
    }
    Ok(SoftLimit::Scale(factor))
}
