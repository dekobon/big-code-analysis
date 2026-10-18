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

use big_code_analysis::metric_catalog::{Direction, LimitError, Scale, Threshold};

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
    /// arithmetic is looked up from it — see [`scale_named_limit`].
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
                scale_named_limit(name, base, factor)
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

/// Whether `ratio` is a valid soft-tier scaling factor: the library's
/// [`Threshold::is_soft_ratio`], the rule `soft_limit` applies, so the
/// `--headroom` scalar (CLI and `bca.toml`) and the `[thresholds.soft]`
/// `"<ratio>x"` form accept exactly what the scaling accepts. Callers
/// compose their own context-specific error message.
pub(crate) fn is_valid_scale_ratio(ratio: f64) -> bool {
    Threshold::is_soft_ratio(ratio)
}

/// Tighten `threshold` by `ratio` with [`Threshold::soft_limit`], then
/// round to [`HEADROOM_SIG_FIGS`] significant figures. Shared by the
/// `--headroom` scalar path and the `[thresholds.soft]` scale-relative
/// form so both round identically.
///
/// `ratio` scales the *band*, not the number: it always makes the soft
/// tier stricter than the hard gate, and which arithmetic does that
/// depends on the metric (#1166, #1621). A count limit is a ceiling, so
/// tightening it means lowering it — multiply. A lower-is-worse `mi.*`
/// limit is a *floor* on an interval scale, so tightening it means
/// **raising** it by `(1 - ratio)` of the metric's unit: dividing a
/// floor of `-50` would *loosen* it.
///
/// The rounding is direction-aware, and asymmetric on purpose. A
/// ceiling rounds to nearest: `limit * ratio` has an exact decimal value
/// that the float product misses by an ulp (`7 * 0.95` is
/// `6.6499999999999995`), so nearest-rounding *recovers* the true
/// product, and the resulting output parity is a contract (#373). A
/// floor's last figure goes **up**, because a floor rounded down is a
/// band that fires marginally late — but only when there is a real
/// remainder to round, see [`FLOOR_GRID_SNAP_ULPS`].
///
/// A soft limit that did not move — every limit at `ratio == 1`, and a
/// floor the domain clamp handed back unchanged — is returned as is,
/// before any rounding: that is the `ratio == 1` identity STABILITY.md
/// promises. A six-figure grid cannot hold a limit with more figures, so
/// rounding it would gate the soft tier tighter than the hard one on a
/// parity run (a ceiling of `1234564` read as `1234560`, a floor of
/// `-14.77423532` as `-14.7742`).
///
/// Whatever the rounding does to a limit that did move, the result is
/// then held on the hard limit's own side of it: a floor never below it,
/// a ceiling never above it. The floor snap can take a floor far smaller
/// than its unit down a tick, and a crossing would trip
/// [`ThresholdSet::build_tiered`](crate::thresholds::ThresholdSet::build_tiered)'s
/// soft-looser-than-hard guard on a configuration that is valid.
///
/// # Errors
///
/// [`LimitError::BadRatio`] when `ratio` lies outside `(0, 1]`; every
/// caller has already checked it with [`is_valid_scale_ratio`].
pub(crate) fn scale_threshold(threshold: &Threshold, ratio: f64) -> Result<f64, LimitError> {
    let scaled = threshold.soft_limit(ratio)?.limit();
    let hard = threshold.limit();
    // Bitwise: `soft_limit` returns the hard limit itself when it does
    // not move it, and `Threshold::check` already folded `-0.0` into `0.0`.
    if scaled.to_bits() == hard.to_bits() {
        return Ok(hard);
    }
    Ok(match threshold.info().direction {
        // The additive step cancels against a floor of the other sign
        // (`-1 + 0.855`), so the residue the sum carries is sized by the
        // operands, not by the smaller result: snap on their scale.
        // A ratio-scale floor's soft limit is a product, carrying no
        // additive residue, so its unit term is zero.
        Direction::LowerIsWorse => {
            let unit = match threshold.info().scale {
                Scale::Interval { unit, .. } => unit,
                _ => 0.0,
            };
            let operand = hard.abs().max(unit);
            round_sig_figs(scaled, Rounding::UpOffGrid { operand }).max(hard)
        }
        Direction::HigherIsWorse => round_sig_figs(scaled, Rounding::Nearest).min(hard),
    })
}

/// How [`round_sig_figs`] treats the last significant figure.
#[derive(Clone, Copy)]
pub(crate) enum Rounding {
    /// To the nearest grid tick: a ceiling, or a display value.
    Nearest,
    /// Up to the next tick unless already on the grid: a soft floor, so
    /// the band never fires late (see [`scale_threshold`]). `operand` is
    /// the magnitude the floor was computed from, which bounds the
    /// float residue a grid position may carry.
    UpOffGrid {
        /// The largest magnitude among the arithmetic's inputs.
        operand: f64,
    },
}

/// [`scale_threshold`] for the limit `limit` of the canonical metric
/// `name`. A name or limit the catalog rejects is returned unscaled:
/// [`ThresholdSet::build_tiered`](crate::thresholds::ThresholdSet::build_tiered)
/// validates every entry again and owns that error, did-you-mean list
/// included.
pub(crate) fn scale_named_limit(name: &str, limit: f64, ratio: f64) -> Result<f64, String> {
    match Threshold::check(name, limit) {
        Ok(checked) => scale_threshold(&checked.threshold, ratio).map_err(|e| e.to_string()),
        Err(_) => Ok(limit),
    }
}

/// Round `value` to [`HEADROOM_SIG_FIGS`] significant figures, by
/// `rounding`. See [`scale_threshold`] for why a floor rounds up.
pub(crate) fn round_sig_figs(value: f64, rounding: Rounding) -> f64 {
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
    let decimals = (HEADROOM_SIG_FIGS - 1) - magnitude;
    let factor = 10f64.powi(decimals);
    // For an absurdly tiny value the sig-fig `factor` overflows to
    // infinity, and `value * factor / factor` would be NaN. No real
    // metric threshold is subnormal, but guard it so the function is
    // total: such a value is already far below any rounding granularity,
    // so return it unrounded rather than poisoning the threshold set
    // with NaN.
    if !factor.is_finite() {
        return value;
    }
    let ticks = value * factor;
    match rounding {
        // A step that cancels the floor exactly (`-0.855 + 0.855`) leaves
        // a residue the size of the operands' ulp, not of the result; on
        // the residue's own grid it would survive as `4.4e-16`. Within the
        // operand tolerance of zero, the floor *is* zero.
        Rounding::UpOffGrid { operand }
            if value.abs() <= FLOOR_GRID_SNAP_ULPS * f64::EPSILON * operand =>
        {
            0.0
        }
        Rounding::UpOffGrid { operand } => {
            ceil_off_grid(ticks, operand.max(value.abs()) * factor) / factor
        }
        Rounding::Nearest => ticks.round() / factor,
    }
}

/// How far from an exact sig-fig grid position a scaled floor may sit
/// and still count as being *on* it, in ULPs of the arithmetic's
/// largest operand.
///
/// `ratio == 1.0` reproduces `limit`, but the double for a limit like
/// `8.3` is a hair above the decimal it prints as, so `8.3 * 1e5` is
/// `830000.0000000001` — one ULP over the grid. A bare `ceil` promotes
/// that to the next whole tick and resolves the soft floor to
/// `8.30001`, which makes the documented `ratio == 1.0` identity false
/// and gates the soft tier above the hard one. The additive interval
/// arithmetic leaves the same residue (`-50 + 0.855` is
/// `-49.144999999999996`), and when the step cancels against a floor of
/// the other sign (`-1 + 0.855`) the residue is a far larger share of
/// the small result than of the operands it came from — which is why
/// the tolerance is measured on the operands. Four ULPs clears those
/// cases with room to spare and is below any remainder a realistic limit
/// carries; a limit far smaller than its unit can carry a genuine
/// remainder inside it, which the clamp in [`scale_threshold`] keeps
/// from lowering the floor.
const FLOOR_GRID_SNAP_ULPS: f64 = 4.0;

/// `ticks.ceil()`, except that a value within [`FLOOR_GRID_SNAP_ULPS`]
/// of a grid position — measured against `scale`, the operand magnitude
/// in tick units — is already on it and rounds to it instead.
fn ceil_off_grid(ticks: f64, scale: f64) -> f64 {
    let nearest = ticks.round();
    if (ticks - nearest).abs() <= FLOOR_GRID_SNAP_ULPS * f64::EPSILON * scale {
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
