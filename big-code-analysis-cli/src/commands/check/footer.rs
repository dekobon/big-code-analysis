//! `bca check` per-file stderr summary footer rendering.

use super::super::*;
use super::*;

struct FooterRow<'a> {
    count: usize,
    worst: &'a Violation,
    display: String,
    path: &'a Path,
}

fn compute_footer_rows(pairs: &[(Violation, Option<Coverage>)]) -> Vec<FooterRow<'_>> {
    Violation::group_pairs_by_path(pairs)
        .into_iter()
        .map(|(count, worst, display, path)| FooterRow {
            count,
            worst,
            display,
            path,
        })
        .collect()
}

/// Emit each row in `rows`, propagating the first I/O error. Used
/// by both the legacy single-section path and the per-bucket
/// partitioned path so the row format stays in lockstep.
fn emit_footer_rows(w: &mut impl Write, rows: &[FooterRow<'_>]) -> std::io::Result<()> {
    for row in rows {
        write_footer_row(w, row.count, row.worst, &row.display)?;
    }
    Ok(())
}

/// Emit the "Files in this range:" header followed by the touched
/// rows. When the diff scope had no offenders in it, emit an
/// explicit "(none — …)" line so the reader gets a positive "your
/// change is clean" signal instead of having to compare both halves
/// of the footer to confirm absence.
fn write_in_range_section(
    w: &mut impl Write,
    scope: &diff::DiffScope,
    in_range: &[FooterRow<'_>],
) -> std::io::Result<()> {
    writeln!(
        w,
        "Files in this range (diff base: {} via {}):",
        scope.base,
        scope.source.label()
    )?;
    if in_range.is_empty() {
        writeln!(w, "  (none — no offenders in files touched by this diff)")?;
    } else {
        emit_footer_rows(w, in_range)?;
    }
    Ok(())
}

/// Emit the "Other offenders:" header followed by the legacy
/// offender list (files not touched by the diff scope). Returns a
/// clean `Ok(())` when `other` is empty so the caller need not gate
/// the call — the section's heading would be misleading without
/// rows below it.
fn write_other_section(w: &mut impl Write, other: &[FooterRow<'_>]) -> std::io::Result<()> {
    if other.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(w, "Other offenders:")?;
    emit_footer_rows(w, other)
}

pub(crate) fn write_summary_footer(
    w: &mut impl Write,
    pairs: &[(Violation, Option<Coverage>)],
    scope: Option<&diff::DiffScope>,
) -> std::io::Result<()> {
    // The caller (`emit_check_results`) gates on `!pairs.is_empty()`,
    // so `compute_footer_rows` should always return at least one
    // row. Assert in debug builds so a future refactor that
    // surfaces the footer on clean runs (e.g. for positive-
    // confirmation symmetry with the step-summary "✓ No threshold
    // violations" message) doesn't silently emit a dangling
    // `Files in this range:` banner with no body.
    let rows = compute_footer_rows(pairs);
    debug_assert!(
        !rows.is_empty(),
        "write_summary_footer called with no rows; \
         caller must gate on !pairs.is_empty()"
    );
    writeln!(w)?;
    writeln!(w, "--- summary ---")?;
    let Some(s) = scope else {
        // Without a scope, today's single-section footer is
        // byte-identical to the pre-#359 output. This is the
        // load-bearing back-compat path for CI tooling that grep-
        // anchors on the legacy footer shape.
        return emit_footer_rows(w, &rows);
    };
    // With a scope, partition rows into "touched in this range" vs
    // legacy offenders. `DiffScope::contains` canonicalises once per
    // row group (already deduplicated by `compute_footer_rows`), so
    // the partitioning is at worst O(unique files) realpath(2) calls.
    let (in_range, other): (Vec<_>, Vec<_>) =
        rows.into_iter().partition(|row| s.contains(row.path));
    write_in_range_section(w, s, &in_range)?;
    write_other_section(w, &other)
}

/// Render a single per-file footer row. Shared between the in-range
/// and other-offenders sections so the formatting stays in lockstep.
fn write_footer_row(
    w: &mut impl Write,
    count: usize,
    worst: &Violation,
    display: &str,
) -> std::io::Result<()> {
    let noun = if count == 1 {
        "violation"
    } else {
        "violations"
    };
    writeln!(
        w,
        "{display}: {count} {noun} (worst: {} = {} vs limit {} at L{})",
        worst.metric,
        MetricScalar(worst.value),
        MetricScalar(worst.limit),
        worst.start_line,
    )
}
