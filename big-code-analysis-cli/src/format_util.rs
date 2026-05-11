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
