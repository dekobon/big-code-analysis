//! Aggregated violation-document output formats for `bca check`.
//!
//! `bca check` walks the source tree, collects [`Violation`] records,
//! and — when `--output-format` is set — emits them as a single
//! aggregated document. The formats here are the CI/IDE
//! integrations the threshold engine is meant to feed:
//!
//! - [`AggregatedFormat::Checkstyle`] — Checkstyle 4.3 XML (Jenkins,
//!   `SonarQube`, GitLab, most "warnings plugin" CI integrations).
//! - [`AggregatedFormat::Sarif`] — SARIF 2.1.0 JSON (GitHub Code
//!   Scanning, modern IDEs/security tooling).
//! - [`AggregatedFormat::ClangWarning`] — clang/GCC warning lines
//!   (editor quickfix parsers, GitHub Actions problem matchers).
//! - [`AggregatedFormat::CodeClimate`] — GitLab Code Climate JSON
//!   (GitLab MR Code Quality widget).
//! - [`AggregatedFormat::MsvcWarning`] — MSVC `cl.exe` diagnostic
//!   lines (Visual Studio, VS Code, Windows CI runners).
//!
//! Each writer accepts an `&[OffenderRecord]` slice. Empty input
//! produces a well-formed but offender-free document, so a clean
//! `bca check` run (or `--no-fail` run on a clean tree) still emits
//! valid output that consumers can ingest unchanged.

use std::fs::{File, create_dir_all};
use std::io::Write;
use std::path::Path;

use clap::ValueEnum;

use big_code_analysis::{
    OffenderRecord, write_checkstyle, write_clang_warning, write_code_climate, write_msvc_warning,
    write_sarif,
};

use crate::thresholds::Violation;

/// Aggregated CI/IDE output formats accepted by `bca check
/// --output-format <fmt>`. Each variant maps to a single writer that
/// emits one document covering every offender from the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub(crate) enum AggregatedFormat {
    Checkstyle,
    #[value(name = "clang-warning")]
    ClangWarning,
    #[value(name = "code-climate")]
    CodeClimate,
    #[value(name = "msvc-warning")]
    MsvcWarning,
    Sarif,
}

impl AggregatedFormat {
    /// Human-readable name used in error messages when the writer
    /// fails.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Checkstyle => "checkstyle",
            Self::Sarif => "sarif",
            Self::ClangWarning => "clang-warning",
            Self::CodeClimate => "code-climate",
            Self::MsvcWarning => "msvc-warning",
        }
    }

    /// Emit a well-formed (and stable) document for the given
    /// offender records to `output_path` (or stdout if `None`).
    pub(crate) fn dump(
        self,
        offenders: &[OffenderRecord],
        output_path: Option<&Path>,
    ) -> std::io::Result<()> {
        match self {
            Self::Checkstyle => {
                write_to_path_or_stdout(output_path, |w| write_checkstyle(offenders, w))
            }
            Self::Sarif => write_to_path_or_stdout(output_path, |w| write_sarif(offenders, w)),
            Self::ClangWarning => {
                write_to_path_or_stdout(output_path, |w| write_clang_warning(offenders, w))
            }
            Self::CodeClimate => {
                write_to_path_or_stdout(output_path, |w| write_code_climate(offenders, w))
            }
            Self::MsvcWarning => {
                write_to_path_or_stdout(output_path, |w| write_msvc_warning(offenders, w))
            }
        }
    }
}

pub(crate) fn violation_to_offender(v: Violation) -> OffenderRecord {
    let Violation {
        path,
        function,
        start_line,
        end_line,
        metric,
        value,
        limit,
    } = v;
    OffenderRecord {
        path,
        function: (!function.is_empty()).then_some(function),
        start_line: u32::try_from(start_line).unwrap_or(u32::MAX),
        end_line: u32::try_from(end_line).unwrap_or(u32::MAX),
        start_col: None,
        metric: metric.to_string(),
        value,
        limit,
        severity: big_code_analysis::Severity::default(),
    }
}

/// Run `write` against either `path` (creating any missing parent
/// directories) or stdout. Shared scaffolding for the aggregated
/// writers; the writer signature is generic over `W: Write`, and
/// `&mut dyn Write` satisfies that bound.
fn write_to_path_or_stdout<F>(output_path: Option<&Path>, write: F) -> std::io::Result<()>
where
    F: FnOnce(&mut dyn Write) -> std::io::Result<()>,
{
    if let Some(path) = output_path {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            create_dir_all(parent)?;
        }
        let mut file = File::create(path)?;
        write(&mut file)
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        write(&mut handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn violation(function: &str) -> Violation {
        Violation {
            path: std::path::PathBuf::from("fixture.rs"),
            start_line: 1,
            end_line: 2,
            function: function.to_string(),
            metric: "cyclomatic",
            value: 5.0,
            limit: 1.0,
        }
    }

    #[test]
    fn violation_to_offender_collapses_empty_function_to_none() {
        // The offender writers document `function: None` as the
        // "file-level violation" semantics; an empty `Violation`
        // function name must round-trip to `None`, not `Some("")`,
        // so SARIF / Checkstyle consumers see a clean omission
        // rather than a stray empty-string `<function>` element.
        let offender = violation_to_offender(violation(""));
        assert_eq!(offender.function, None);
    }

    #[test]
    fn violation_to_offender_preserves_non_empty_function() {
        let offender = violation_to_offender(violation("compute"));
        assert_eq!(offender.function.as_deref(), Some("compute"));
    }

    /// `OffenderRecord::path` is `PathBuf` precisely so non-UTF-8
    /// path bytes survive the dump boundary. Pre-#240 the
    /// `Violation::path: String` field had already collapsed them
    /// through `to_string_lossy` upstream, so the conversion appeared
    /// lossless but the bytes had already been lost. This regression
    /// test pins the round-trip end to end.
    #[cfg(unix)]
    #[test]
    fn violation_to_offender_preserves_non_utf8_path_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        use std::path::PathBuf;

        let raw_bytes: &[u8] = b"weird-\xff\xfe.rs";
        let path = PathBuf::from(OsString::from_vec(raw_bytes.to_vec()));
        let v = Violation {
            path: path.clone(),
            start_line: 1,
            end_line: 2,
            function: "f".to_string(),
            metric: "cyclomatic",
            value: 5.0,
            limit: 1.0,
        };
        let offender = violation_to_offender(v);
        assert_eq!(offender.path, path);
        assert_eq!(offender.path.as_os_str().as_encoded_bytes(), raw_bytes);
    }
}
