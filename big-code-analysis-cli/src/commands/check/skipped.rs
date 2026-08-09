//! The gate's skipped-input accounting (#1055): the ignore-dropped
//! file measurement and the default not-checked stderr summary.

use super::super::*;
use super::*;

/// Measure which files a VCS ignore file dropped from the gate's walk.
///
/// The `ignore` crate's walker prunes ignored entries internally and
/// never yields them, so the set has to be derived: resolve the same
/// seeds once more with ignore handling off and take the difference
/// against the files the gate walk kept. The unfiltered set is a
/// superset by construction — same seeds, same include/exclude/hidden
/// filtering, and a gitignore negation (`!pattern`) can only re-include
/// — so the difference is exactly the ignore-dropped files. Sorted so
/// the `--report-skipped` listing is deterministic.
///
/// This is a metadata-only second traversal (no file is read), paid
/// only by `bca check`, and skipped outright under `--no-ignore` /
/// `--strict`, where the answer is empty by definition. The
/// measurement resolve's own `walk_errors` tally is deliberately
/// dropped: only the gate walk's tallies decide the exit code, and a
/// traversal error there already fails the run.
pub(crate) fn vcs_ignored_files(
    mut globals: GlobalOpts,
    resolved: &crate::ResolvedFiles,
) -> Vec<PathBuf> {
    if globals.no_ignore {
        return Vec::new();
    }
    globals.no_ignore = true;
    let (unfiltered, _) = resolve_walk_files(globals);
    let checked: std::collections::HashSet<&PathBuf> = resolved.files.iter().collect();
    let mut ignored: Vec<PathBuf> = unfiltered
        .files
        .into_iter()
        .filter(|path| !checked.contains(path))
        .collect();
    ignored.sort_unstable();
    ignored
}

/// Say, by default, what the gate declined to look at (#1055): each
/// ignore-dropped file under `--report-skipped` (the generated listing
/// is printed during dispatch), then the one-line count summary. A run
/// that skipped nothing stays silent, so clean local runs are
/// unchanged. Deliberately loud-not-strict: the counts change no gate
/// behaviour and no exit code — `--strict` is the profile that does.
pub(crate) fn report_unchecked_files(walk: &CheckWalk, report_skipped: bool) {
    if report_skipped {
        for path in &walk.ignored {
            note(format_args!("skipped (ignored): {}", path.display()));
        }
    }
    if let Some(summary) =
        unchecked_summary(walk.generated_skipped, walk.ignored.len(), report_skipped)
    {
        // The severity-free `bca:` family the gate's other informational
        // lines use (`bca: skipped N violations via [check.exclude]`) —
        // a `note:` prefix after the namespace would be the #609 double
        // prefix.
        eprintln!("bca: {summary}");
    }
}

/// The one-line not-checked summary, or `None` when nothing was
/// skipped. Zero-count categories are omitted; the `--report-skipped`
/// hint is dropped when the flag is already on (`listed`) and the
/// per-file lines have therefore just been printed.
pub(crate) fn unchecked_summary(generated: usize, ignored: usize, listed: bool) -> Option<String> {
    let total = generated + ignored;
    if total == 0 {
        return None;
    }
    let mut breakdown = Vec::new();
    if generated > 0 {
        breakdown.push(format!("{generated} generated"));
    }
    if ignored > 0 {
        breakdown.push(format!("{ignored} ignored"));
    }
    let noun = if total == 1 { "file" } else { "files" };
    let hint = if listed {
        ""
    } else {
        " — pass --report-skipped to list them"
    };
    Some(format!(
        "{total} {noun} not checked ({}){hint}",
        breakdown.join(", ")
    ))
}
