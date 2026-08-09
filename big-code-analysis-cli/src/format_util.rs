//! Shared formatting helpers for metric scalars across the CLI.
//!
//! Metric values are stored as `f64` even when conceptually integer
//! (cyclomatic, cognitive, loc.*). The display rule is the same in
//! every CLI surface: integer-valued results print as integers
//! (`12`, not `12.0`), fractional values keep enough precision to
//! round-trip. Centralizing the rule prevents quietly truncating
//! Halstead volumes/efforts via stray `format!("{:.0}", x)` sites.

use std::fmt;

/// A metric scalar formatted with the shared CLI display rule:
/// integer-valued values print without a decimal, fractional values
/// keep full `f64::to_string` precision. NaN / infinity print as
/// the standard Rust `Display` form (`NaN`, `inf`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct MetricScalar(pub f64);

impl fmt::Display for MetricScalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = self.0;
        if v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15 {
            write!(f, "{v:.0}")
        } else {
            write!(f, "{v}")
        }
    }
}

/// Render a non-negative `f64` metric as a rounded integer with comma
/// thousands separators (`8844.757…` -> `"8,845"`), for report-table columns
/// where fifteen significant digits of a heuristic convey nothing and wreck
/// column scanability (issue #668). Effort / Volume render through this in the
/// hotspot `SPECS`, matching the neighbouring SLOC / Tokens columns; full f64
/// precision stays in JSON / CSV for machine consumers.
///
/// Non-finite input (NaN / infinity) falls back to the standard `Display`
/// form rather than a nonsensical separator-formatted integer; a (practically
/// unreachable) value past `usize::MAX` saturates rather than wrapping.
pub(crate) fn thousands_round(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    let rounded = v.round();
    if rounded < 0.0 {
        // Metric magnitudes are non-negative; guard the cast defensively.
        return rounded.to_string();
    }
    // `as usize` saturates at `usize::MAX` for an out-of-range positive, which
    // keeps a sane (if clamped) render instead of a wrapped wrong one.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let n = rounded as usize;
    crate::markdown_report::thousands(n)
}

/// Strip `prefix` from the front of `path` for display, using
/// `str::strip_prefix` semantics. A no-op when `prefix` is empty or
/// does not match, so callers can pass an empty prefix unconditionally
/// (matching the `--strip-prefix` default on `report` / `exemptions` /
/// `diff` / `diff-baseline`).
pub(crate) fn strip_path_prefix<'a>(path: &'a str, prefix: &str) -> &'a str {
    if prefix.is_empty() {
        path
    } else {
        path.strip_prefix(prefix).unwrap_or(path)
    }
}

/// `3 files`, `1 ignored directory`: a count with the noun that agrees
/// with it. The crate spells this rule in several report renderers;
/// new sites should call this one.
pub(crate) fn counted(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use super::*;

    #[test]
    fn integer_valued_prints_without_decimal() {
        assert_eq!(MetricScalar(12.0).to_string(), "12");
        assert_eq!(MetricScalar(0.0).to_string(), "0");
        assert_eq!(MetricScalar(-7.0).to_string(), "-7");
    }

    #[test]
    fn fractional_keeps_precision() {
        assert_eq!(MetricScalar(12.5).to_string(), "12.5");
        // Halstead-style fractional values must NOT round to an
        // integer — that's the bug this helper exists to prevent.
        assert!(MetricScalar(12.7).to_string().starts_with("12.7"));
    }

    #[test]
    fn thousands_round_rounds_and_separates() {
        // Issue #668: Halstead Effort/Volume render as rounded integers with
        // separators, not 15-significant-digit floats.
        assert_eq!(thousands_round(8_844.757_014_412_85), "8,845");
        assert_eq!(thousands_round(613.115_377_122_328_5), "613");
        assert_eq!(thousands_round(1_481.142_857_142_857_3), "1,481");
        assert_eq!(thousands_round(144.0), "144");
        assert_eq!(thousands_round(0.0), "0");
        // Half rounds to even-away per f64::round (round-half-away-from-zero).
        assert_eq!(thousands_round(2.5), "3");
    }

    #[test]
    fn thousands_round_non_finite_falls_back() {
        assert!(thousands_round(f64::NAN).contains("NaN"));
        assert!(thousands_round(f64::INFINITY).contains("inf"));
    }

    #[test]
    fn nan_does_not_panic() {
        let s = MetricScalar(f64::NAN).to_string();
        assert!(s.contains("NaN") || s.contains("nan"));
    }

    #[test]
    fn infinity_does_not_panic() {
        let s = MetricScalar(f64::INFINITY).to_string();
        assert!(s.contains("inf"));
    }
}
