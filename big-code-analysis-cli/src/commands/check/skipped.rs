//! The gate's skipped-input reporting (#1055): the default not-checked
//! stderr summary over the generated-skip tally and the walk's
//! ignore-rule measurement (see `IgnoredEntries` in the walk layer for
//! how the ignored set is derived).

use super::*;

use crate::format_util::counted;

/// Say, by default, what the gate declined to look at (#1055): each
/// ignore-dropped entry under `--report-skipped` (the generated listing
/// is printed during dispatch), then the one-line count summary. A run
/// that skipped nothing stays silent, so clean local runs are
/// unchanged. Deliberately loud-not-strict: the counts change no gate
/// behaviour and no exit code — `--strict` is the profile that does.
pub(crate) fn report_unchecked_files(walk: &CheckWalk, report_skipped: bool) {
    if report_skipped {
        for path in &walk.ignored.files {
            note(format_args!("skipped (ignored): {}", path.display()));
        }
        for path in &walk.ignored.dirs {
            note(format_args!(
                "skipped (ignored directory): {}",
                path.display()
            ));
        }
    }
    if let Some(summary) = unchecked_summary(walk.generated_skipped, &walk.ignored, report_skipped)
    {
        // The severity-free `bca:` family the gate's other informational
        // lines use (`bca: skipped N violations via [check.exclude]`) —
        // a `note:` prefix after the namespace would be the #609 double
        // prefix.
        eprintln!("bca: {summary}");
    }
}

/// The one-line not-checked summary, or `None` when nothing was
/// skipped. Zero-count categories are omitted. Pruned directories get
/// their own clause because their contents are unknown by design (the
/// walk never enters them, so they cannot be given a file count) — and
/// that clause appears only under `--report-skipped` (`listed`):
/// essentially every real checkout has an ignored build tree on disk
/// (`target/`, `node_modules/`), so a default summary that counted
/// pruned directories would fire on every run and bury the signal the
/// file counts carry. The `--report-skipped` hint is dropped when the
/// flag is already on and the per-entry lines have just been printed.
pub(crate) fn unchecked_summary(
    generated: usize,
    ignored: &crate::IgnoredEntries,
    listed: bool,
) -> Option<String> {
    let ignored_files = ignored.files.len();
    let dirs = if listed { ignored.dirs.len() } else { 0 };
    let file_total = generated + ignored_files;
    if file_total + dirs == 0 {
        return None;
    }
    let mut clauses = Vec::new();
    if file_total > 0 {
        let mut breakdown = Vec::new();
        if generated > 0 {
            breakdown.push(format!("{generated} generated"));
        }
        if ignored_files > 0 {
            breakdown.push(format!("{ignored_files} ignored"));
        }
        clauses.push(format!(
            "{} not checked ({})",
            counted(file_total, "file", "files"),
            breakdown.join(", ")
        ));
    }
    if dirs > 0 {
        clauses.push(format!(
            "{} not walked",
            counted(dirs, "ignored directory", "ignored directories")
        ));
    }
    let hint = if listed {
        ""
    } else {
        " — pass --report-skipped to list them"
    };
    Some(format!("{}{hint}", clauses.join("; ")))
}
