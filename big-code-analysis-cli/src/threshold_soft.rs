//! The soft threshold tier: its limit forms, their parsing, and the
//! lookup that resolves a `"<ratio>x"` factor through the catalog —
//! shared with `--headroom` (issue #375).
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

use big_code_analysis::metric_catalog::Threshold;

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

/// Whether `ratio` is a valid soft-tier scaling factor: the library's
/// [`Threshold::is_soft_ratio`], the rule `soft_limit` applies, so the
/// `--headroom` scalar (CLI and `bca.toml`) and the `[thresholds.soft]`
/// `"<ratio>x"` form accept exactly what the scaling accepts. Callers
/// compose their own context-specific error message.
pub(crate) fn is_valid_scale_ratio(ratio: f64) -> bool {
    Threshold::is_soft_ratio(ratio)
}

/// The soft-tier limit for the hard limit `limit` of the canonical
/// metric `name`: [`Threshold::soft_limit_rounded`], which owns the
/// direction-aware arithmetic, the six-figure rounding and the hard-side
/// clamp (#1621, #1648). Shared by the `--headroom` scalar path and the
/// `[thresholds.soft]` scale-relative form so both resolve identically.
///
/// A name or limit the catalog rejects is returned unscaled:
/// [`ThresholdSet::build_tiered`](crate::thresholds::ThresholdSet::build_tiered)
/// validates every entry again and owns that error, did-you-mean list
/// included.
///
/// # Errors
///
/// The rendered
/// [`LimitError::BadRatio`](big_code_analysis::metric_catalog::LimitError::BadRatio)
/// when `ratio` lies outside `(0, 1]`; every caller has already checked
/// it with [`is_valid_scale_ratio`].
pub(crate) fn scale_named_limit(name: &str, limit: f64, ratio: f64) -> Result<f64, String> {
    match Threshold::check(name, limit) {
        Ok(checked) => checked
            .threshold
            .soft_limit_rounded(ratio)
            .map(|soft| soft.limit())
            .map_err(|e| e.to_string()),
        Err(_) => Ok(limit),
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
