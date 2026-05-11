// Metric counts (token, function, branch, argument, etc.) are stored as
// `usize` and crossed with `f64` averages, ratios, and Halstead scores
// across the cyclomatic / MI / Halstead computations. The `usize as f64`
// and `f64 as usize` casts are intentional and snapshot-anchored — every
// site is bounded by the count it came from. Allowing the lints at the
// module level keeps the metric arithmetic legible.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

//! Numeric formatting adapters shared by every output format.
//!
//! Two `Display` newtypes wrap an `f64` metric value:
//!
//! - [`CellMetric`] — for CSV cells (any structured-tabular cell).
//!   Non-finite values render as the empty string so downstream tools
//!   read them as "not applicable" rather than as `0` or `NaN`. Finite
//!   values use the integer fast-path (no trailing `.0`) for safe-
//!   integer values, and the standard f64 `Display` for everything
//!   else (full round-trippable precision).
//!
//! - [`MessageMetric`] — for human-readable warning text (Checkstyle
//!   `<error message="...">`, SARIF `result.message.text`, Clang/MSVC
//!   warning lines). Non-finite values render via the standard f64
//!   `Display` (`"NaN"` / `"inf"` / `"-inf"`) so the warning still
//!   reads sensibly. Integer-valued finites use the same fast-path as
//!   `CellMetric`. Non-integer finites are rounded to six decimal
//!   places with trailing zeros trimmed — full f64 precision is noise
//!   inside a one-line warning.
//!
//! Both adapters implement `Display`, so callers that build strings
//! via `write!` / `format!` pay no per-number heap allocation. The
//! adapters share the same integer fast-path bound
//! ([`F64_SAFE_INT_BOUND`] = 2^53), the largest f64 that round-trips
//! through `as i64` without precision loss.

use std::fmt;

/// 2^53 — the largest f64 with no precision loss for an integer
/// round-trip via `as i64`. Beyond this bound we keep the floating-
/// point representation rather than risk silently mangling the value.
pub(crate) const F64_SAFE_INT_BOUND: f64 = 9_007_199_254_740_992.0;

/// `Display` adapter for a metric cell in a structured tabular format
/// (CSV). See module docs for semantics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CellMetric(pub f64);

impl fmt::Display for CellMetric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.0.is_finite() {
            // Empty cell: NaN / ±inf are not "values" in a metric row.
            return Ok(());
        }
        if is_safe_integer(self.0) {
            write!(f, "{}", self.0 as i64)
        } else {
            fmt::Display::fmt(&self.0, f)
        }
    }
}

/// `Display` adapter for a metric value embedded in human-readable
/// warning text. See module docs for semantics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MessageMetric(pub f64);

impl fmt::Display for MessageMetric {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.0.is_finite() {
            // f64::Display renders "NaN" / "inf" / "-inf" verbatim,
            // which is exactly what we want inside a one-line warning.
            return fmt::Display::fmt(&self.0, f);
        }
        if is_safe_integer(self.0) {
            return write!(f, "{}", self.0 as i64);
        }
        // Round to 6 decimals, trim trailing zeros (and any orphaned
        // decimal point). One String alloc per call when we hit this
        // branch — acceptable: a warning message renders once per
        // offender, not per metric column.
        let formatted = format!("{:.6}", self.0);
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        f.write_str(trimmed)
    }
}

#[inline]
fn is_safe_integer(v: f64) -> bool {
    // Caller has already guaranteed `v.is_finite()`.
    debug_assert!(v.is_finite());
    v.fract() == 0.0 && v.abs() < F64_SAFE_INT_BOUND
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

    fn cell(v: f64) -> String {
        CellMetric(v).to_string()
    }

    fn msg(v: f64) -> String {
        MessageMetric(v).to_string()
    }

    #[test]
    fn cell_renders_safe_integer_without_trailing_decimal() {
        assert_eq!(cell(17.0), "17");
        assert_eq!(cell(0.0), "0");
        assert_eq!(cell(-1.0), "-1");
    }

    #[test]
    fn cell_renders_non_integer_with_full_precision() {
        // Default Display gives the shortest round-tripping form for
        // exact-decimal values.
        assert_eq!(cell(12.5), "12.5");
        assert_eq!(cell(0.5), "0.5");
        // A value that truly needs many digits keeps them — verify
        // shape (a decimal point and a long mantissa) without baking
        // in the exact least-significant digit, which depends on the
        // closest f64 representation of the source literal.
        let many = cell(0.123_456_789_012_345_67);
        assert!(many.starts_with("0.12345678901234"));
        assert!(many.len() > 16);
    }

    #[test]
    fn cell_renders_non_finite_as_empty() {
        assert_eq!(cell(f64::NAN), "");
        assert_eq!(cell(f64::INFINITY), "");
        assert_eq!(cell(f64::NEG_INFINITY), "");
    }

    #[test]
    fn cell_at_safe_int_bound_falls_back_to_default_display() {
        // The bound is *exclusive*, so 2^53 itself fails the
        // `abs() < F64_SAFE_INT_BOUND` check and takes the default
        // Display path. f64 can still represent 2^53 exactly, so
        // Display prints the integer literal without a decimal —
        // exactly what we want.
        assert_eq!(cell(F64_SAFE_INT_BOUND), "9007199254740992");
    }

    #[test]
    fn cell_above_safe_int_bound_does_not_saturate_via_as_i64() {
        // Regression guard: if the safe-int bound is widened (or the
        // `< F64_SAFE_INT_BOUND` check is removed entirely), values
        // larger than `i64::MAX` would take the integer fast-path and
        // saturate `as i64` to 9_223_372_036_854_775_807, silently
        // mangling huge metric values. f64::MAX is the canonical input
        // that distinguishes the two code paths: the saturating cast
        // would produce a 19-digit literal of `i64::MAX`, while the
        // default Display produces a ~309-digit decimal expansion of
        // the actual value. The two are never confusable.
        let s = cell(f64::MAX);
        assert!(
            !s.contains("9223372036854775807"),
            "saturating `as i64` cast leaked into output: {s}"
        );
        assert!(
            s.starts_with("179769313486231"),
            "expected the leading digits of f64::MAX, got: {s}"
        );
    }

    #[test]
    fn message_renders_safe_integer_without_trailing_decimal() {
        assert_eq!(msg(17.0), "17");
        assert_eq!(msg(15.0), "15");
        assert_eq!(msg(-1.0), "-1");
    }

    #[test]
    fn message_renders_non_integer_rounded_to_six_decimals() {
        assert_eq!(msg(12.5), "12.5");
        // 0.123456789 rounds at 6 decimals to 0.123457; trailing
        // zeros (none here) and any orphan decimal are trimmed.
        assert_eq!(msg(0.123_456_789), "0.123457");
        // Trailing zeros after the rounded fraction get stripped.
        assert_eq!(msg(1.500_000_5), "1.500001");
        assert_eq!(msg(1.250_000), "1.25");
    }

    #[test]
    fn message_renders_non_finite_via_default_display() {
        assert_eq!(msg(f64::NAN), "NaN");
        assert_eq!(msg(f64::INFINITY), "inf");
        assert_eq!(msg(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn message_writes_into_formatter_without_intermediate_alloc_on_integer_path() {
        use std::fmt::Write as _;
        // Integer-valued finite goes through the no-alloc write! path.
        // Non-integer-finite still allocates internally; we verify the
        // observable Display output, not the alloc count.
        let mut buf = String::new();
        write!(
            buf,
            "limit {} value {}",
            MessageMetric(15.0),
            MessageMetric(17.0)
        )
        .unwrap();
        assert_eq!(buf, "limit 15 value 17");
    }
}
