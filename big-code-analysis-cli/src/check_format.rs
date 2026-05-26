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

use std::collections::BTreeMap;
use std::fs::{File, create_dir_all};
use std::io::Write;
use std::path::Path;

use clap::ValueEnum;

use big_code_analysis::{
    OffenderRecord, write_checkstyle, write_clang_warning, write_code_climate, write_msvc_warning,
    write_sarif,
};

use crate::baseline::Coverage;
use crate::format_util::MetricScalar;
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

/// Default annotation cap per metric. GitHub Actions surfaces at most
/// 10 errors / 10 warnings / 10 notices per step in the UI, so a
/// 400-violation run would exhaust the quota and silently drop
/// everything past the first ten. Capping at this value per metric
/// keeps each metric visible while leaving headroom under the
/// step-level cap (most failing runs trip one or two distinct
/// metrics, not the full N-metric list).
pub(crate) const DEFAULT_GITHUB_ANNOTATION_CAP: usize = 10;

/// Env var GitHub Actions sets to `"true"` inside every workflow
/// step. Used to auto-enable annotation emission when
/// `--github-annotations` is not passed explicitly.
pub(crate) const GITHUB_ACTIONS_ENV: &str = "GITHUB_ACTIONS";

/// `<owner>/<repo>` slug GitHub Actions sets on every workflow run.
/// Combined with [`GITHUB_RUN_ID_ENV`] to build the artifact URL
/// surfaced in the remediation footer.
pub(crate) const GITHUB_REPOSITORY_ENV: &str = "GITHUB_REPOSITORY";

/// Numeric workflow-run identifier GitHub Actions sets on every
/// workflow run. Combined with [`GITHUB_REPOSITORY_ENV`] to build
/// the artifact URL surfaced in the remediation footer.
pub(crate) const GITHUB_RUN_ID_ENV: &str = "GITHUB_RUN_ID";

pub(crate) fn write_github_annotations<'a, W, I>(
    w: &mut W,
    violations: I,
    cap: usize,
) -> std::io::Result<()>
where
    W: Write,
    I: IntoIterator<Item = &'a Violation>,
{
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut overflow: BTreeMap<&'static str, usize> = BTreeMap::new();
    for v in violations {
        let n = counts.entry(v.metric).or_insert(0);
        *n += 1;
        if *n <= cap {
            // Non-UTF-8 paths cannot be expressed as identifiers in
            // GitHub Actions annotation properties (`file=...`), since
            // the GHA UI uses the byte sequence to locate the source
            // file on disk and the workflow-command protocol carries
            // text only. Skip the annotation rather than emit a lossy
            // path that would point at the wrong file; the per-
            // violation human stderr line still names the file via
            // `path.display()`, and the project's AGENTS.md rule
            // ("never `to_string_lossy` on identifier paths") is
            // satisfied.
            let Some(path_str) = v.path.to_str() else {
                continue;
            };
            writeln!(
                w,
                "::error file={path},line={start},endLine={end},title={title}::{msg}",
                path = escape_gha(path_str, GhaSlot::Property),
                start = v.start_line,
                end = v.end_line,
                title = escape_gha(v.metric, GhaSlot::Property),
                msg = escape_gha(&v.summary_tail(), GhaSlot::Message),
            )?;
        } else {
            *overflow.entry(v.metric).or_insert(0) += 1;
        }
    }
    for (metric, n) in overflow {
        writeln!(
            w,
            "::error::{n} more {metric} violations not shown — see full log"
        )?;
    }
    Ok(())
}

/// Percent-encode characters GitHub Actions reserves inside a
/// workflow-command property value (`key=value` slot) or a message
/// body (after `::`). The two slots reserve overlapping but distinct
/// sets — see [`GhaSlot`]. Reference:
/// <https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions#example-setting-an-error-message>.
fn escape_gha(s: &str, slot: GhaSlot) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '\r' => out.push_str("%0D"),
            '\n' => out.push_str("%0A"),
            ':' if matches!(slot, GhaSlot::Property) => out.push_str("%3A"),
            ',' if matches!(slot, GhaSlot::Property) => out.push_str("%2C"),
            _ => out.push(ch),
        }
    }
    out
}

/// Which workflow-command slot a value will land in. Properties (the
/// `key=value` pairs between the command name and `::`) reserve `:`
/// and `,` in addition to the universal `%`/`\r`/`\n`; message bodies
/// (after `::`) allow `:` and `,` literally because GHA's parser only
/// honours the *first* `::` after the command name as the separator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GhaSlot {
    Property,
    Message,
}

/// Env var GitHub Actions sets to the path of the step-summary
/// markdown file. Appending to it surfaces a rendered digest in the
/// step's summary panel — much more discoverable than scrolling
/// the raw job log.
pub(crate) const GITHUB_STEP_SUMMARY_ENV: &str = "GITHUB_STEP_SUMMARY";

/// Number of top-by-ratio offenders surfaced in the step-summary
/// "Top offenders" table. Paired with [`DEFAULT_GITHUB_ANNOTATION_CAP`]
/// so the same N most-egregious functions surface in both the inline
/// annotations and the rollup; tuning either changes both. The `const`
/// references the cap directly rather than declaring an independent
/// `10` so drift cannot occur silently.
pub(crate) const STEP_SUMMARY_TOP_OFFENDERS: usize = DEFAULT_GITHUB_ANNOTATION_CAP;

/// Sentinel marker pair that bounds the `bca`-written block inside
/// the step-summary file. GitHub Actions appends, never truncates,
/// `$GITHUB_STEP_SUMMARY` on every write — so a naive append on a
/// retried step would stack two copies of the rollup. By bracketing
/// our block with these markers and replacing on subsequent writes,
/// retries produce a single up-to-date digest regardless of how many
/// times the step ran. The markers are HTML comments and never
/// visible in the rendered output.
pub(crate) const STEP_SUMMARY_BEGIN_MARKER: &str = "<!-- bca-step-summary-begin -->";
pub(crate) const STEP_SUMMARY_END_MARKER: &str = "<!-- bca-step-summary-end -->";

/// Append (or replace, on retry) a markdown digest of the violations
/// to `path`. The block contains the per-file rollup, per-metric
/// breakdown, top-N offenders by ratio, and the trailing
/// remediation block (when provided). Empty input still writes a
/// "✓ no violations" block so the developer sees positive
/// confirmation in the step summary even on clean runs.
pub(crate) fn write_step_summary(
    path: &Path,
    pairs: &[(Violation, Option<Coverage>)],
    remediation: Option<&str>,
) -> std::io::Result<()> {
    let new_block = compose_step_summary_block(pairs, remediation);
    // First-write (file does not yet exist) is the common case in
    // GHA workflows; treat NotFound as an empty starting state.
    // Other I/O errors (permission denied, mid-FS error) must bubble
    // — silently swallowing them would let the caller emit a
    // misleading "failed to append" diagnostic that hides the actual
    // permission / FS problem.
    let existing = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let replaced = replace_step_summary_block(&existing, &new_block);
    std::fs::write(path, replaced)
}

/// Compose the bracketed markdown block. Always idempotent for the
/// same `(pairs, remediation)` input (no timestamps, no random IDs),
/// so two consecutive runs over the same offenders produce
/// byte-identical output. This is the load-bearing invariant for
/// `replace_step_summary_block`'s retry semantics.
fn compose_step_summary_block(
    pairs: &[(Violation, Option<Coverage>)],
    remediation: Option<&str>,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str(STEP_SUMMARY_BEGIN_MARKER);
    out.push('\n');
    out.push_str("## `bca check`: threshold violations\n\n");
    if pairs.is_empty() {
        out.push_str("✓ No threshold violations.\n");
    } else {
        let _ = writeln!(out, "**Total violations:** {}\n", pairs.len());
        write_per_file_rollup(&mut out, pairs);
        out.push('\n');
        write_metric_breakdown(&mut out, pairs);
        out.push('\n');
        write_top_offenders(&mut out, pairs, STEP_SUMMARY_TOP_OFFENDERS);
    }
    // Embed the remediation block (when present) inside the marker
    // pair so retries replace it along with the rest of the digest.
    // Use a 4-backtick fence so a path / repo name / refresh-command
    // value containing an embedded triple-backtick (legal on Linux,
    // exotic but possible) does not break out of the code block.
    // The 4-fence is a documented GFM idiom for exactly this case.
    if let Some(block) = remediation {
        out.push_str("\n````text\n");
        out.push_str(block.trim_start_matches('\n'));
        if !block.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("````\n");
    }
    out.push_str(STEP_SUMMARY_END_MARKER);
    out.push('\n');
    out
}

/// Replace any existing `bca`-marker block in `existing` with
/// `new_block`. When the markers are absent (first write, or a
/// non-`bca`-managed file), append `new_block` to the end. Idempotent
/// across retries: bca runs N+1 times → file contains exactly one
/// up-to-date block, not N+1 stacked copies. Handles the orphan-BEGIN
/// case (prior run killed mid-write leaving a BEGIN marker without a
/// matching END) by splicing from BEGIN to EOF rather than appending,
/// which would accumulate orphan markers across retries.
fn replace_step_summary_block(existing: &str, new_block: &str) -> String {
    let Some(begin) = existing.find(STEP_SUMMARY_BEGIN_MARKER) else {
        // No BEGIN marker → first write or a non-`bca`-managed file.
        // Append, ensuring a newline separates our block from any
        // existing content.
        let mut out = existing.to_string();
        if !existing.is_empty() && !existing.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(new_block);
        return out;
    };
    // BEGIN is present. Locate the matching END *after* it. If
    // END is missing (prior bca run killed mid-write, OR a
    // non-bca consumer corrupted the marker pair) we still need
    // to converge to a single block; splice from BEGIN to EOF
    // rather than fall through to the append branch, which would
    // accumulate orphan BEGINs across retries.
    let end_abs = match existing[begin..].find(STEP_SUMMARY_END_MARKER) {
        Some(end) => begin + end + STEP_SUMMARY_END_MARKER.len(),
        None => existing.len(),
    };
    // Drop the trailing newline (if any) immediately after the
    // end marker so we don't accumulate blank lines on repeated
    // replacements.
    let tail_start = if existing.as_bytes().get(end_abs) == Some(&b'\n') {
        end_abs + 1
    } else {
        end_abs
    };
    let mut out = String::with_capacity(existing.len() + new_block.len());
    out.push_str(&existing[..begin]);
    out.push_str(new_block);
    out.push_str(&existing[tail_start..]);
    out
}

/// "Per-file rollup" GFM table: file, violation count, worst metric
/// (by ratio), worst value, worst limit. Sorted by count desc, then
/// path asc.
fn write_per_file_rollup(out: &mut String, pairs: &[(Violation, Option<Coverage>)]) {
    use std::fmt::Write as _;
    out.push_str("### Per-file rollup\n\n");
    out.push_str("| File | Violations | Worst metric | Value | Limit |\n");
    out.push_str("|------|-----------:|--------------|------:|------:|\n");
    // Reuse the shared grouping/pick-worst/sort helper so the
    // stderr footer and the markdown rollup never disagree about
    // which violation is "worst" for a given file or how rows are
    // ordered.
    for (count, worst, display, _path) in Violation::group_pairs_by_path(pairs) {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            escape_gfm_cell(&display),
            count,
            worst.metric,
            MetricScalar(worst.value),
            MetricScalar(worst.limit),
        );
    }
}

/// "Metric breakdown" GFM table: count of violations per metric,
/// sorted by count desc then metric name asc.
fn write_metric_breakdown(out: &mut String, pairs: &[(Violation, Option<Coverage>)]) {
    use std::fmt::Write as _;
    out.push_str("### Metric breakdown\n\n");
    out.push_str("| Metric | Violations |\n");
    out.push_str("|--------|-----------:|\n");
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for (v, _) in pairs {
        *counts.entry(v.metric).or_insert(0) += 1;
    }
    let mut rows: Vec<(&&'static str, &usize)> = counts.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (metric, n) in rows {
        let _ = writeln!(out, "| {metric} | {n} |");
    }
}

/// "Top offenders" GFM table: up to `top_n` violations ranked by
/// `value / limit` ratio (saturating to infinity for zero-limit
/// thresholds, mirroring `pick_worst`).
fn write_top_offenders(out: &mut String, pairs: &[(Violation, Option<Coverage>)], top_n: usize) {
    use std::fmt::Write as _;
    let _ = writeln!(out, "### Top {top_n} offenders (by value / limit)\n");
    out.push_str("| File | Line | Function | Metric | Value | Limit |\n");
    out.push_str("|------|-----:|----------|--------|------:|------:|\n");
    let mut sorted: Vec<&Violation> = pairs.iter().map(|(v, _)| v).collect();
    // Primary key: ratio desc (worst first). Secondary keys are
    // deterministic tiebreaks — without them, two violations with
    // the same ratio (common: same metric and limit, integer values)
    // would surface in iterator-order, which depends on upstream
    // collection types and would let the digest reorder between
    // refactors.
    sorted.sort_by(|a, b| {
        b.ratio()
            .total_cmp(&a.ratio())
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.start_line.cmp(&b.start_line))
            .then_with(|| a.metric.cmp(b.metric))
    });
    for v in sorted.iter().take(top_n) {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} |",
            escape_gfm_cell(&v.path.display().to_string()),
            v.start_line,
            escape_gfm_cell(&v.function),
            v.metric,
            MetricScalar(v.value),
            MetricScalar(v.limit),
        );
    }
}

/// Escape `\`, `|`, and newlines in a GFM table cell. `|` is the
/// column separator; `\` must be escaped first so that escaping `|`
/// to `\|` doesn't accidentally interact with a literal backslash
/// already present in the cell (a path `a\|b` would otherwise
/// produce `a\\|b`, which GFM renders as `a\` followed by a column
/// break rather than the literal `a\|b`). Newlines and carriage
/// returns are collapsed to a single space — multi-line cells
/// would break the table layout entirely.
fn escape_gfm_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            // Escape `\` FIRST so the subsequent `|` → `\|` rule
            // doesn't conflict with a literal backslash already in
            // the cell.
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
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

    // --- GitHub Actions annotation tests ---

    fn violation_for(path: &str, function: &str, metric: &'static str) -> Violation {
        Violation {
            path: std::path::PathBuf::from(path),
            start_line: 10,
            end_line: 42,
            function: function.to_string(),
            metric,
            value: 17.0,
            limit: 5.0,
        }
    }

    #[test]
    fn write_github_annotations_emits_one_line_per_violation() {
        let vs = [
            violation_for("src/a.rs", "foo", "cyclomatic"),
            violation_for("src/b.rs", "bar", "cognitive"),
        ];
        let mut buf = Vec::new();
        write_github_annotations(&mut buf, vs.iter(), 10).expect("write");
        let out = String::from_utf8(buf).expect("utf8");
        let expected = "::error file=src/a.rs,line=10,endLine=42,title=cyclomatic::foo: cyclomatic = 17 (limit 5)\n\
            ::error file=src/b.rs,line=10,endLine=42,title=cognitive::bar: cognitive = 17 (limit 5)\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn write_github_annotations_caps_per_metric_with_overflow_rollup() {
        // 12 cyclomatic violations + 1 cognitive. With cap = the
        // documented default we expect 10 cyclomatic annotations, 1
        // cognitive annotation, and a single overflow line
        // accounting for the 2 extras (12 − 10).
        let mut vs: Vec<Violation> = (0..12)
            .map(|i| violation_for(&format!("c{i}.rs"), "f", "cyclomatic"))
            .collect();
        vs.push(violation_for("g.rs", "g", "cognitive"));
        let mut buf = Vec::new();
        write_github_annotations(&mut buf, vs.iter(), DEFAULT_GITHUB_ANNOTATION_CAP)
            .expect("write");
        let out = String::from_utf8(buf).expect("utf8");
        // Anchor on `title=cyclomatic::f:` (function name boundary)
        // so a regression that mis-spelled the title as
        // `cyclomaticxyz::` would not still satisfy the prefix
        // count. See lesson #6.
        let cyclomatic_lines = out.matches("title=cyclomatic::f:").count();
        assert_eq!(cyclomatic_lines, 10, "expected 10 capped cyclomatic lines");
        let cognitive_lines = out.matches("title=cognitive::g:").count();
        assert_eq!(cognitive_lines, 1, "uncapped cognitive line missing");
        assert!(
            out.contains("::error::2 more cyclomatic violations not shown — see full log"),
            "missing overflow line for cyclomatic; got:\n{out}"
        );
    }

    #[test]
    fn write_github_annotations_percent_escapes_property_metacharacters() {
        // GitHub Actions reserves `:`, `,`, `%`, `\r`, `\n` inside
        // property values (key=value). A Windows-style absolute path
        // (`C:\src`) triggers the `:` encode path; a `%` in the
        // function name triggers the message-side `%` encode (which
        // would silently lose precision if the wrong helper were
        // wired into the message slot). The message body keeps `:`
        // literal (GHA's message-side contract).
        let v = Violation {
            path: std::path::PathBuf::from("C:\\src/100%/file.rs"),
            start_line: 1,
            end_line: 1,
            function: "f%encoded".to_string(),
            metric: "cyclomatic",
            value: 17.0,
            limit: 5.0,
        };
        let mut buf = Vec::new();
        write_github_annotations(&mut buf, std::iter::once(&v), DEFAULT_GITHUB_ANNOTATION_CAP)
            .expect("write");
        let out = String::from_utf8(buf).expect("utf8");
        // `:` percent-encoded to %3A in the property, `%` to %25.
        assert!(
            out.contains("file=C%3A\\src/100%25/file.rs,"),
            "expected property-encoded path, got:\n{out}"
        );
        // Message body: `%` encoded to `%25`, `:` kept literal. This
        // pins both the message-side encode and the message-side
        // literal-`:` invariant in one assertion.
        assert!(
            out.contains("::f%25encoded: cyclomatic = 17 (limit 5)"),
            "expected message-body %-encode AND literal `:`, got:\n{out}"
        );
    }

    #[test]
    fn escape_gha_property_encodes_all_reserved_characters() {
        assert_eq!(
            escape_gha("a%b:c,d\re\nf", GhaSlot::Property),
            "a%25b%3Ac%2Cd%0De%0Af"
        );
    }

    #[test]
    fn escape_gha_message_keeps_colon_and_comma_literal() {
        // Per the GHA contract, message bodies allow `:` and `,`
        // literally — only `%`, `\r`, `\n` must be encoded.
        assert_eq!(
            escape_gha("a:b,c%d\re\nf", GhaSlot::Message),
            "a:b,c%25d%0De%0Af"
        );
    }

    #[test]
    fn write_github_annotations_emits_nothing_for_empty_input() {
        let mut buf = Vec::new();
        write_github_annotations(&mut buf, std::iter::empty(), DEFAULT_GITHUB_ANNOTATION_CAP)
            .expect("write");
        assert!(buf.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn write_github_annotations_skips_non_utf8_paths() {
        // Annotation paths are identifiers in GitHub's UI (the renderer
        // looks the file up on disk by the byte sequence), so we
        // forbid `to_string_lossy` here and skip non-UTF-8 entries
        // rather than emit a corrupted file= value. The per-violation
        // human stderr line still names the file via `path.display()`.
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        use std::path::PathBuf;

        let path = PathBuf::from(OsString::from_vec(b"weird-\xff.rs".to_vec()));
        let v = Violation {
            path,
            start_line: 1,
            end_line: 1,
            function: "f".to_string(),
            metric: "cyclomatic",
            value: 17.0,
            limit: 5.0,
        };
        let mut buf = Vec::new();
        write_github_annotations(&mut buf, std::iter::once(&v), DEFAULT_GITHUB_ANNOTATION_CAP)
            .expect("write");
        assert!(
            buf.is_empty(),
            "expected non-UTF-8 path to be skipped, got: {}",
            String::from_utf8_lossy(&buf)
        );
    }

    // --- $GITHUB_STEP_SUMMARY emitter tests ---

    fn coverage_pair(v: Violation) -> (Violation, Option<Coverage>) {
        (v, None)
    }

    #[test]
    fn compose_step_summary_block_is_idempotent_for_same_input() {
        // The block is content-only — no timestamps, no random IDs —
        // so two runs over the same violation set produce
        // byte-identical output. This is the load-bearing invariant
        // for `replace_step_summary_block`'s retry semantics: a
        // retried GHA step writes exactly the same block, so the
        // step-summary panel does not flicker between retries.
        let pairs = vec![
            coverage_pair(violation_for("src/a.rs", "f", "cyclomatic")),
            coverage_pair(violation_for("src/b.rs", "g", "cognitive")),
        ];
        let a = compose_step_summary_block(&pairs, None);
        let b = compose_step_summary_block(&pairs, None);
        assert_eq!(a, b);
    }

    #[test]
    fn compose_step_summary_block_contains_required_sections() {
        let pairs = vec![
            coverage_pair(violation_for("src/a.rs", "f", "cyclomatic")),
            coverage_pair(violation_for("src/b.rs", "g", "cognitive")),
        ];
        let out = compose_step_summary_block(&pairs, None);
        assert!(out.starts_with(STEP_SUMMARY_BEGIN_MARKER));
        assert!(out.trim_end().ends_with(STEP_SUMMARY_END_MARKER));
        assert!(out.contains("## `bca check`: threshold violations"));
        assert!(out.contains("**Total violations:** 2"));
        assert!(out.contains("### Per-file rollup"));
        assert!(out.contains("### Metric breakdown"));
        assert!(out.contains("### Top 10 offenders"));
        // Per-file rollup row pinned via the cell shape (`src/a.rs`
        // with cyclomatic worst metric) so a regression that dropped
        // a column or transposed cells would fail.
        assert!(
            out.contains("| src/a.rs | 1 | cyclomatic |"),
            "missing per-file row, got:\n{out}"
        );
    }

    #[test]
    fn compose_step_summary_block_empty_input_writes_clean_message() {
        let out = compose_step_summary_block(&[], None);
        assert!(out.starts_with(STEP_SUMMARY_BEGIN_MARKER));
        assert!(out.contains("✓ No threshold violations."));
        // Don't emit empty rollup / breakdown sections — the
        // checkmark is the entire body.
        assert!(!out.contains("### Per-file rollup"));
    }

    #[test]
    fn replace_step_summary_block_first_write_appends_to_empty_file() {
        let new_block = "<!-- bca-step-summary-begin -->\nbody\n<!-- bca-step-summary-end -->\n";
        let out = replace_step_summary_block("", new_block);
        assert_eq!(out, new_block);
    }

    #[test]
    fn replace_step_summary_block_first_write_preserves_existing_content() {
        // Another tool may have written to GITHUB_STEP_SUMMARY first
        // (e.g. an upstream cargo step). Don't truncate — append.
        let existing = "## Upstream summary\nfoo\n";
        let new_block = "<!-- bca-step-summary-begin -->\nbody\n<!-- bca-step-summary-end -->\n";
        let out = replace_step_summary_block(existing, new_block);
        assert!(out.starts_with("## Upstream summary"));
        assert!(out.contains(new_block));
    }

    #[test]
    fn replace_step_summary_block_replaces_existing_marker_block() {
        // Retried GHA step: bca writes once, then bca writes again
        // (because the step was retried). The second write replaces
        // the first, leaving exactly ONE block in the file regardless
        // of retry count.
        let first = "<!-- bca-step-summary-begin -->\nfirst body\n<!-- bca-step-summary-end -->\n";
        let second =
            "<!-- bca-step-summary-begin -->\nsecond body\n<!-- bca-step-summary-end -->\n";
        let out = replace_step_summary_block(first, second);
        assert_eq!(out, second);
        // Re-applying the same `second` block is a no-op (also load-
        // bearing for retries: three retries should converge to
        // exactly one block).
        let out2 = replace_step_summary_block(&out, second);
        assert_eq!(out2, second);
    }

    #[test]
    fn replace_step_summary_block_preserves_content_outside_markers() {
        let existing = "## Other tool\nleading\n\
            <!-- bca-step-summary-begin -->\nold\n<!-- bca-step-summary-end -->\n\
            ## More content\ntrailing\n";
        let new_block = "<!-- bca-step-summary-begin -->\nnew\n<!-- bca-step-summary-end -->\n";
        let out = replace_step_summary_block(existing, new_block);
        assert!(out.starts_with("## Other tool\nleading\n"));
        assert!(out.ends_with("## More content\ntrailing\n"));
        assert!(out.contains("new\n"));
        assert!(!out.contains("old\n"));
    }

    #[test]
    fn replace_step_summary_block_converges_in_one_retry_after_orphan_begin() {
        // Regression: a prior bca run killed mid-write (SIGTERM, OOM,
        // runner timeout) can leave the file with a BEGIN marker
        // followed by partial content but NO END marker. Previously
        // the function fell through to the append branch in that
        // case, leaving the orphan BEGIN + partial body alongside
        // the freshly-appended block — two BEGINs, one END, with
        // stale content sandwiched between. The "exactly one block
        // regardless of retry count" invariant was only restored on
        // the *second* retry (find(BEGIN) returned the orphan, then
        // find(END) succeeded). Splicing from BEGIN to EOF in the
        // missing-END case makes a single retry converge.
        let orphan = "prefix\n<!-- bca-step-summary-begin -->\npartial body\n";
        let new_block = "<!-- bca-step-summary-begin -->\nnew\n<!-- bca-step-summary-end -->\n";
        let out = replace_step_summary_block(orphan, new_block);
        // Exactly one BEGIN, exactly one END, no stale `partial body`.
        assert_eq!(
            out.matches(STEP_SUMMARY_BEGIN_MARKER).count(),
            1,
            "expected exactly one BEGIN after retry, got:\n{out}"
        );
        assert_eq!(
            out.matches(STEP_SUMMARY_END_MARKER).count(),
            1,
            "expected exactly one END after retry, got:\n{out}"
        );
        assert!(
            !out.contains("partial body"),
            "stale `partial body` from killed run must be discarded, got:\n{out}"
        );
        assert!(
            out.starts_with("prefix\n"),
            "content outside the bca block must survive, got:\n{out}"
        );
    }

    #[test]
    fn write_step_summary_creates_and_then_replaces_on_retry() {
        // End-to-end: write to a real tempfile, then write twice
        // more (N=3 retries total), read back and confirm exactly
        // one bca block remains regardless of retry count. Three
        // retries exercise the fixed-point property the issue
        // describes; N=2 alone would only show that one retry works.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("step-summary.md");
        let pairs1 = vec![coverage_pair(violation_for("src/a.rs", "f", "cyclomatic"))];
        write_step_summary(&path, &pairs1, None).expect("write 1");
        let after_first = std::fs::read_to_string(&path).expect("read 1");
        assert_eq!(
            after_first.matches(STEP_SUMMARY_BEGIN_MARKER).count(),
            1,
            "expected exactly one begin marker after first write"
        );

        let pairs2 = vec![coverage_pair(violation_for("src/b.rs", "g", "cognitive"))];
        write_step_summary(&path, &pairs2, None).expect("write 2");
        let after_second = std::fs::read_to_string(&path).expect("read 2");
        assert_eq!(
            after_second.matches(STEP_SUMMARY_BEGIN_MARKER).count(),
            1,
            "retried write must replace, not stack (N=2)"
        );
        assert!(after_second.contains("src/b.rs"));
        assert!(!after_second.contains("src/a.rs"));

        // Third write with the same pairs as the second — fixed
        // point: file byte-content must be identical to the
        // after-second state. This proves the marker-replace logic
        // converges, not just that it works once.
        write_step_summary(&path, &pairs2, None).expect("write 3");
        let after_third = std::fs::read_to_string(&path).expect("read 3");
        assert_eq!(
            after_third, after_second,
            "third write with same input must be byte-identical (fixed point)"
        );
    }

    #[test]
    fn write_step_summary_bubbles_non_notfound_io_errors() {
        // Permission denied (and other non-NotFound I/O errors) must
        // bubble — silently swallowing them would let the caller emit
        // a misleading "failed to append" diagnostic that hides the
        // actual permission / FS problem.
        //
        // We simulate by giving `write_step_summary` a path that
        // points *through* a regular file as if it were a directory
        // (e.g. `/etc/hostname/foo`). The read succeeds with
        // NotFound (parent is a file, not a dir), so this only tests
        // the write-side error propagation. That's enough to confirm
        // the function returns `Err` rather than silently succeeding.
        let dir = tempfile::tempdir().expect("tempdir");
        let stub = dir.path().join("not-a-dir");
        std::fs::write(&stub, "stub").expect("write stub");
        let path = stub.join("child");
        let pairs = vec![coverage_pair(violation_for("src/a.rs", "f", "cyclomatic"))];
        assert!(
            write_step_summary(&path, &pairs, None).is_err(),
            "expected non-NotFound I/O error to bubble"
        );
    }

    #[test]
    fn write_step_summary_with_remediation_embeds_fenced_block_and_replaces_on_retry() {
        // End-to-end coverage of the wave-4 integration path: the
        // remediation block must land inside the marker pair (so
        // retries replace it along with the rest of the digest),
        // inside a 4-backtick fenced code block (so embedded
        // triple-backticks in paths / refresh commands cannot
        // escape), and the whole file must remain idempotent across
        // a retry that supplies the same `Some(remediation)` value.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("step-summary.md");
        let pairs = vec![coverage_pair(violation_for("src/a.rs", "f", "cyclomatic"))];
        let remediation = "\n--- next steps ---\n* Detailed reports: bca-reports artifact\n* To refresh baseline: bca --paths . check --write-baseline .bca-baseline.toml\n";
        write_step_summary(&path, &pairs, Some(remediation)).expect("write 1");
        let after_first = std::fs::read_to_string(&path).expect("read 1");
        // 4-backtick fence opens AND closes inside the marker pair.
        // Whole-file `contains` would let a regression that emitted
        // the closer *after* the end marker still pass — bound the
        // checks to the bca-managed block instead.
        let begin = after_first
            .find(STEP_SUMMARY_BEGIN_MARKER)
            .expect("begin marker");
        let end = after_first
            .find(STEP_SUMMARY_END_MARKER)
            .expect("end marker");
        assert!(begin < end, "markers must be ordered");
        let block = &after_first[begin..end];
        assert!(
            block.contains("````text\n"),
            "missing fenced opener inside marker block, got:\n{block}"
        );
        assert!(
            block.contains("\n````\n"),
            "missing fenced closer inside marker block (regression that emitted the closer outside the markers would slip past a whole-file `contains`), got:\n{block}"
        );
        assert!(
            block.contains("--- next steps ---"),
            "missing remediation banner inside marker block, got:\n{block}"
        );

        // Retry with the same remediation: file must be byte-
        // identical (fixed point).
        write_step_summary(&path, &pairs, Some(remediation)).expect("write 2");
        let after_second = std::fs::read_to_string(&path).expect("read 2");
        assert_eq!(
            after_first, after_second,
            "second write with same input must be byte-identical (fixed point)"
        );
        // Exactly one begin marker survives.
        assert_eq!(after_second.matches(STEP_SUMMARY_BEGIN_MARKER).count(), 1);
    }

    #[test]
    fn escape_gfm_cell_escapes_pipe_and_collapses_newlines() {
        // Path containing `|` (legal on Unix) would otherwise split
        // the GFM cell into two columns and corrupt the table layout.
        assert_eq!(escape_gfm_cell("a|b"), "a\\|b");
        // Newlines inside a cell would also break the row; collapse
        // to a single space so the table stays well-formed.
        assert_eq!(escape_gfm_cell("a\nb\rc"), "a b c");
    }

    #[test]
    fn escape_gfm_cell_escapes_backslash_before_pipe() {
        // A literal `\` in a path must round-trip as `\` in the
        // rendered cell. If we only escape `|`, then the input
        // `a\|b` (literal backslash + pipe) becomes `a\\|b` — which
        // GFM renders as `a\` followed by a column break, splitting
        // the cell. Escaping `\` to `\\` first makes the same input
        // become `a\\\|b`, which renders as the literal `a\|b`.
        assert_eq!(escape_gfm_cell("a\\b"), "a\\\\b");
        assert_eq!(escape_gfm_cell("a\\|b"), "a\\\\\\|b");
    }
}
