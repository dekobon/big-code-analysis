//! Compiler-warning line writers for [`OffenderRecord`] batches.
//!
//! Editor- and CI-annotator-friendly inline warnings: one offender per
//! line, in the conventional Clang/GCC and MSVC formats that quickfix
//! parsers (VS Code, IntelliJ, Vim) and CI annotators (GitHub Actions
//! `::warning::`, GitLab, Jenkins warnings-ng) recognize out of the
//! box.
//!
//! Clang/GCC ([`write_clang_warning`]):
//!
//! ```text
//! path/to/file.rs:42:5: warning: cyclomatic 17 exceeds limit 15 [big-code-analysis-cyclomatic]
//! ```
//!
//! MSVC ([`write_msvc_warning`]):
//!
//! ```text
//! path\to\file.rs(42,5): warning : cyclomatic 17 exceeds limit 15
//! ```
//!
//! Both writers emit one line per offender. An empty offender slice
//! produces empty output (zero bytes), not a blank line. Offenders
//! whose path is not valid UTF-8 are skipped with a warning to stderr.

#![allow(clippy::doc_markdown)]

use std::io::{self, Write};

use crate::output::offenders::{OffenderRecord, TOOL_ID, warn_non_utf8_path};

/// Default column when an offender has no [`OffenderRecord::start_col`].
/// Both formats require a column; `1` is the conventional placeholder
/// (matching how Clang itself reports diagnostics whose column is
/// unknown).
const DEFAULT_COL: u32 = 1;

/// Write Clang/GCC-style warning lines for `offenders` to `writer`.
///
/// Format: `{path}:{line}:{col}: {severity}: {message} [{rule}]`,
/// terminated by `\n`. Non-UTF-8 paths are skipped with a stderr
/// warning. An empty `offenders` slice writes nothing.
///
/// # Errors
///
/// Returns any [`io::Error`] produced by `writer` while emitting a
/// warning line. Stops at the first error; partially-written output
/// may have reached the writer before the failure.
pub fn write_clang_warning<W: Write>(
    offenders: &[OffenderRecord],
    mut writer: W,
) -> io::Result<()> {
    for record in offenders {
        let Some(path) = warn_non_utf8_path("clang-warning", &record.path) else {
            continue;
        };
        let line = record.start_line.max(1);
        let col = record.start_col.unwrap_or(DEFAULT_COL).max(1);
        writeln!(
            writer,
            "{path}:{line}:{col}: {severity}: {message} [{prefix}-{metric}]",
            severity = record.severity.as_str(),
            message = record.default_message(),
            prefix = TOOL_ID,
            metric = record.metric,
        )?;
    }
    Ok(())
}

/// Write MSVC-style warning lines for `offenders` to `writer`.
///
/// Format: `{path}({line},{col}): {severity} : {message}`, terminated
/// by `\n`. Note the space before the colon after `severity` — that is
/// the MSVC convention. On Windows the path uses `\` separators
/// (matching cl.exe output); on other platforms it is emitted as-is.
/// Non-UTF-8 paths are skipped with a stderr warning. An empty
/// `offenders` slice writes nothing.
///
/// # Errors
///
/// Returns any [`io::Error`] produced by `writer` while emitting a
/// warning line. Stops at the first error; partially-written output
/// may have reached the writer before the failure.
pub fn write_msvc_warning<W: Write>(offenders: &[OffenderRecord], mut writer: W) -> io::Result<()> {
    for record in offenders {
        let Some(raw_path) = warn_non_utf8_path("msvc-warning", &record.path) else {
            continue;
        };
        let path = msvc_path(raw_path);
        let line = record.start_line.max(1);
        let col = record.start_col.unwrap_or(DEFAULT_COL).max(1);
        writeln!(
            writer,
            "{path}({line},{col}): {severity} : {message}",
            severity = record.severity.as_str(),
            message = record.default_message(),
        )?;
    }
    Ok(())
}

/// On Windows, normalize forward slashes to backslashes so the output
/// matches the path style cl.exe emits (and quickfix parsers in
/// Windows IDEs expect). Elsewhere the input is returned untouched —
/// CI logs running on Linux/macOS keep `/` separators, which the
/// quickfix parsers also accept.
#[cfg(windows)]
fn msvc_path(raw: &str) -> std::borrow::Cow<'_, str> {
    if raw.contains('/') {
        std::borrow::Cow::Owned(raw.replace('/', "\\"))
    } else {
        std::borrow::Cow::Borrowed(raw)
    }
}

#[cfg(not(windows))]
fn msvc_path(raw: &str) -> &str {
    raw
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
    use crate::output::offenders::Severity;
    use std::path::PathBuf;

    fn rec(path: &str, metric: &str, value: f64, limit: f64) -> OffenderRecord {
        OffenderRecord {
            path: PathBuf::from(path),
            function: Some("f".into()),
            start_line: 42,
            end_line: 50,
            start_col: Some(5),
            metric: metric.into(),
            value,
            limit,
            severity: Severity::Warning,
        }
    }

    fn render_clang(offenders: &[OffenderRecord]) -> String {
        let mut buf = Vec::new();
        write_clang_warning(offenders, &mut buf).expect("writing to Vec is infallible");
        String::from_utf8(buf).expect("output is UTF-8")
    }

    fn render_msvc(offenders: &[OffenderRecord]) -> String {
        let mut buf = Vec::new();
        write_msvc_warning(offenders, &mut buf).expect("writing to Vec is infallible");
        String::from_utf8(buf).expect("output is UTF-8")
    }

    #[test]
    fn clang_empty_writes_nothing() {
        assert_eq!(render_clang(&[]), "");
    }

    #[test]
    fn msvc_empty_writes_nothing() {
        assert_eq!(render_msvc(&[]), "");
    }

    #[test]
    fn clang_single_offender() {
        let out = render_clang(&[rec("src/foo.rs", "cyclomatic", 17.0, 15.0)]);
        assert_eq!(
            out,
            "src/foo.rs:42:5: warning: cyclomatic 17 exceeds limit 15 [big-code-analysis-cyclomatic]\n"
        );
    }

    #[test]
    fn msvc_single_offender() {
        let out = render_msvc(&[rec("src/foo.rs", "cyclomatic", 17.0, 15.0)]);
        // On non-Windows, separators are preserved as-is.
        #[cfg(not(windows))]
        assert_eq!(
            out,
            "src/foo.rs(42,5): warning : cyclomatic 17 exceeds limit 15\n"
        );
        #[cfg(windows)]
        assert_eq!(
            out,
            "src\\foo.rs(42,5): warning : cyclomatic 17 exceeds limit 15\n"
        );
    }

    #[test]
    fn clang_missing_column_defaults_to_one() {
        let mut r = rec("a.rs", "cognitive", 30.0, 15.0);
        r.start_col = None;
        let out = render_clang(&[r]);
        assert!(out.starts_with("a.rs:42:1: warning: "), "{out}");
    }

    #[test]
    fn msvc_missing_column_defaults_to_one() {
        let mut r = rec("a.rs", "cognitive", 30.0, 15.0);
        r.start_col = None;
        let out = render_msvc(&[r]);
        assert!(out.starts_with("a.rs(42,1): warning : "), "{out}");
    }

    #[test]
    fn clang_error_severity_renders_error_token() {
        let mut r = rec("a.rs", "cyclomatic", 99.0, 15.0);
        r.severity = Severity::Error;
        let out = render_clang(&[r]);
        assert!(out.contains(": error: "), "{out}");
    }

    #[test]
    fn msvc_error_severity_renders_error_token() {
        let mut r = rec("a.rs", "cyclomatic", 99.0, 15.0);
        r.severity = Severity::Error;
        let out = render_msvc(&[r]);
        // Note the space before the colon: "error :" not "error:".
        assert!(out.contains("): error : "), "{out}");
    }

    #[test]
    fn clang_integer_value_has_no_decimal_point() {
        let out = render_clang(&[rec("a.rs", "cyclomatic", 17.0, 15.0)]);
        assert!(out.contains("cyclomatic 17 exceeds limit 15"), "{out}");
        assert!(!out.contains("17.0"), "{out}");
        assert!(!out.contains("15.0"), "{out}");
    }

    #[test]
    fn clang_fractional_value_renders_decimals() {
        let out = render_clang(&[rec("a.rs", "halstead.volume", 12.5, 10.0)]);
        assert!(
            out.contains("halstead.volume 12.5 exceeds limit 10"),
            "{out}"
        );
    }

    #[test]
    fn clang_zero_start_line_clamps_to_one() {
        let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        r.start_line = 0;
        let out = render_clang(&[r]);
        assert!(out.starts_with("a.rs:1:5: "), "{out}");
    }

    #[test]
    fn msvc_zero_start_line_clamps_to_one() {
        let mut r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        r.start_line = 0;
        let out = render_msvc(&[r]);
        assert!(out.starts_with("a.rs(1,5): "), "{out}");
    }

    #[test]
    fn clang_multi_offender_one_line_each() {
        let offenders = vec![
            rec("src/alpha.rs", "cyclomatic", 17.0, 15.0),
            rec("src/alpha.rs", "loc.lloc", 250.0, 100.0),
            rec("src/zeta.rs", "cognitive", 30.0, 15.0),
        ];
        let out = render_clang(&offenders);
        assert_eq!(out.lines().count(), 3);
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn clang_function_name_does_not_appear_in_line() {
        // The Clang format has no field for a function name; the
        // writer must not silently smuggle it in.
        let r = rec("a.rs", "cyclomatic", 17.0, 15.0);
        // function is Some("f")
        let out = render_clang(&[r]);
        // The bare token "f" could appear inside other words; assert
        // the structural shape instead.
        assert_eq!(
            out,
            "a.rs:42:5: warning: cyclomatic 17 exceeds limit 15 [big-code-analysis-cyclomatic]\n"
        );
    }

    #[test]
    fn clang_empty_snapshot() {
        insta::assert_snapshot!("clang_warning_empty", render_clang(&[]));
    }

    #[test]
    fn clang_multi_snapshot() {
        let mut err = rec("src/zeta.rs", "cognitive", 30.0, 15.0);
        err.severity = Severity::Error;
        err.start_col = None;
        err.function = None;
        let offenders = vec![
            rec("src/alpha.rs", "cyclomatic", 17.0, 15.0),
            rec("src/alpha.rs", "loc.lloc", 250.0, 100.0),
            err,
        ];
        insta::assert_snapshot!("clang_warning_multi", render_clang(&offenders));
    }

    #[test]
    fn msvc_empty_snapshot() {
        insta::assert_snapshot!("msvc_warning_empty", render_msvc(&[]));
    }

    // The committed snapshot pins forward-slash separators. On Windows
    // `render_msvc` renders backslashes (verified by
    // `msvc_path_uses_backslashes_on_windows`), so gate the multi-offender
    // snapshot to non-Windows.
    #[cfg(not(windows))]
    #[test]
    fn msvc_multi_snapshot() {
        let mut err = rec("src/zeta.rs", "cognitive", 30.0, 15.0);
        err.severity = Severity::Error;
        err.start_col = None;
        err.function = None;
        let offenders = vec![
            rec("src/alpha.rs", "cyclomatic", 17.0, 15.0),
            rec("src/alpha.rs", "loc.lloc", 250.0, 100.0),
            err,
        ];
        insta::assert_snapshot!("msvc_warning_multi", render_msvc(&offenders));
    }

    #[cfg(windows)]
    #[test]
    fn msvc_path_uses_backslashes_on_windows() {
        let out = render_msvc(&[rec("src/foo/bar.rs", "cyclomatic", 17.0, 15.0)]);
        assert!(out.starts_with("src\\foo\\bar.rs("), "{out}");
    }

    #[cfg(not(windows))]
    #[test]
    fn msvc_path_keeps_forward_slashes_off_windows() {
        let out = render_msvc(&[rec("src/foo/bar.rs", "cyclomatic", 17.0, 15.0)]);
        assert!(out.starts_with("src/foo/bar.rs("), "{out}");
    }
}
