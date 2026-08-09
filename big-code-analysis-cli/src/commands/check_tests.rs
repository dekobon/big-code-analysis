// Sibling-file unit tests for the `commands::check` module. Wired via
// `#[path = "check_tests.rs"] mod tests;` so the production `check.rs`
// stays under the `bca check` per-file metric caps. Matched by the
// `./**/*_tests.rs` rule in `.bcaignore`.

use super::*;

/// Build an [`crate::IgnoredEntries`] with `files` ignored files and
/// `dirs` pruned directories — the summary only reads the lengths, but
/// the paths are distinct so an accidental dedup could not mask a
/// count.
fn ignored(files: usize, dirs: usize) -> crate::IgnoredEntries {
    crate::IgnoredEntries {
        files: (0..files)
            .map(|i| PathBuf::from(format!("f{i}.rs")))
            .collect(),
        dirs: (0..dirs).map(|i| PathBuf::from(format!("d{i}"))).collect(),
    }
}

/// A run that skipped nothing must stay silent — the summary exists to
/// surface the #1055 bypasses, not to add noise to clean local runs.
#[test]
fn unchecked_summary_is_none_when_nothing_was_skipped() {
    assert_eq!(unchecked_summary(0, &ignored(0, 0), false), None);
    assert_eq!(unchecked_summary(0, &ignored(0, 0), true), None);
}

/// Singular noun, one category, and the `--report-skipped` hint for
/// the default (unlisted) run.
#[test]
fn unchecked_summary_names_a_single_generated_file() {
    assert_eq!(
        unchecked_summary(1, &ignored(0, 0), false).as_deref(),
        Some("1 file not checked (1 generated) — pass --report-skipped to list them")
    );
}

/// A zero-count category is omitted rather than printed as `0 …`.
#[test]
fn unchecked_summary_omits_the_zero_category() {
    assert_eq!(
        unchecked_summary(0, &ignored(3, 0), false).as_deref(),
        Some("3 files not checked (3 ignored) — pass --report-skipped to list them")
    );
}

/// Both file categories present: total first, then the per-cause
/// breakdown in generated-then-ignored order (the issue's documented
/// shape).
#[test]
fn unchecked_summary_combines_both_categories() {
    assert_eq!(
        unchecked_summary(2, &ignored(1, 0), false).as_deref(),
        Some("3 files not checked (2 generated, 1 ignored) — pass --report-skipped to list them")
    );
}

/// With `--report-skipped` already on, the hint would point at a flag
/// the user just passed; the per-entry lines were printed instead.
#[test]
fn unchecked_summary_drops_the_hint_when_already_listing() {
    assert_eq!(
        unchecked_summary(2, &ignored(1, 0), true).as_deref(),
        Some("3 files not checked (2 generated, 1 ignored)")
    );
}

/// A pruned directory gets its own clause — its contents are unknown
/// by design, so it cannot be folded into the file count. Singular
/// form pinned.
#[test]
fn unchecked_summary_reports_pruned_directories_apart() {
    assert_eq!(
        unchecked_summary(0, &ignored(0, 1), false).as_deref(),
        Some("1 ignored directory not walked — pass --report-skipped to list them")
    );
}

/// Files and pruned directories combine into two clauses, files first.
#[test]
fn unchecked_summary_joins_file_and_directory_clauses() {
    assert_eq!(
        unchecked_summary(1, &ignored(0, 2), false).as_deref(),
        Some(
            "1 file not checked (1 generated); 2 ignored directories not walked \
             — pass --report-skipped to list them"
        )
    );
}
