//! Unit tests for the argv-scan deprecation detector (#646). These cover
//! the spelling-detection logic in isolation; the integration suite
//! (`tests/cli_ux/deprecated_aliases.rs`) asserts the end-to-end stderr text and
//! that canonical spellings stay silent.

use super::{DEPRECATED_FLAG_ALIASES, is_flag_spelling, subcommand_used, top_subcommand};

/// The integration suite (`tests/cli_ux/deprecated_aliases.rs`) asserts the
/// deprecation warning for every flag-alias row by name. This guard pins
/// the table's size so adding a row without a matching warning test is a
/// visible failure here (#832) — the silent-breakage #646 was created to
/// prevent. Bump this count only alongside a new per-alias warning test.
#[test]
fn deprecated_flag_alias_table_size_is_pinned() {
    assert_eq!(
        DEPRECATED_FLAG_ALIASES.len(),
        9,
        "DEPRECATED_FLAG_ALIASES changed size; add/remove the matching \
         per-alias warning test in tests/cli_ux/deprecated_aliases.rs and update \
         this count",
    );
}

#[test]
fn flag_spelling_matches_bare_and_equals_forms() {
    assert!(is_flag_spelling("--num-jobs", "--num-jobs"));
    assert!(is_flag_spelling("--num-jobs=4", "--num-jobs"));
}

#[test]
fn flag_spelling_rejects_unrelated_longer_flag() {
    // A bare `starts_with` test would mis-match `--num-jobs-extra`; the
    // `=` boundary (or exact equality) is what prevents that.
    assert!(!is_flag_spelling("--num-jobs-extra", "--num-jobs"));
    assert!(!is_flag_spelling("--jobs", "--num-jobs"));
}

#[test]
fn flag_spelling_rejects_value_token() {
    // The token following `--language` is a value, not a flag spelling.
    assert!(!is_flag_spelling("language-type", "--language-type"));
}

#[test]
fn subcommand_used_detects_jit_after_vcs() {
    let tokens = vec![
        String::from("vcs"),
        String::from("jit"),
        String::from("HEAD"),
    ];
    assert!(subcommand_used(&tokens, "jit"));
}

#[test]
fn subcommand_used_ignores_jit_as_value() {
    // `jit` as a positional value of some other flag, not in subcommand
    // position, must not trip the subcommand detector.
    let tokens = vec![
        String::from("metrics"),
        String::from("--paths"),
        String::from("jit"),
    ];
    assert!(!subcommand_used(&tokens, "jit"));
}

#[test]
fn subcommand_used_detects_jit_after_global_flag() {
    // A boolean `global = true` flag (`-w`) before the subcommand must not
    // suppress detection of the deprecated `jit` spelling (#834).
    let tokens = vec![
        String::from("vcs"),
        String::from("-w"),
        String::from("jit"),
        String::from("HEAD"),
    ];
    assert!(subcommand_used(&tokens, "jit"));
}

#[test]
fn subcommand_used_detects_jit_after_value_taking_flag() {
    // A value-taking global (`--long-window 6mo`) before the subcommand
    // must skip its value (`6mo`) and still find `jit` (#834).
    let tokens = vec![
        String::from("vcs"),
        String::from("--long-window"),
        String::from("6mo"),
        String::from("jit"),
        String::from("HEAD"),
    ];
    assert!(subcommand_used(&tokens, "jit"));
}

#[test]
fn subcommand_used_ignores_jit_not_directly_after_vcs() {
    // `jit` present and `vcs` present, but `jit` is a positional argument
    // of the canonical `commit` subcommand, not in subcommand position.
    // The adjacency guard must reject it (#835); a contains-style scan
    // would wrongly flag it.
    let tokens = vec![
        String::from("vcs"),
        String::from("commit"),
        String::from("jit"),
    ];
    assert!(!subcommand_used(&tokens, "jit"));
}

#[test]
fn subcommand_used_quiet_for_canonical_commit() {
    let tokens = vec![
        String::from("vcs"),
        String::from("commit"),
        String::from("HEAD"),
    ];
    assert!(!subcommand_used(&tokens, "jit"));
}

#[test]
fn subcommand_used_ignores_vcs_jit_after_double_dash() {
    // Paths named `vcs` and `jit` after a `--` end-of-options marker are
    // positional values, not a subcommand pair. Production truncates at
    // `--` before calling `subcommand_used`; mirror that here and assert
    // no detection (#836). The raw (untruncated) slice would falsely
    // detect `jit`.
    let tokens = [
        String::from("metrics"),
        String::from("--paths"),
        String::from("--"),
        String::from("vcs"),
        String::from("jit"),
    ];
    let end = tokens.iter().position(|t| t == "--").expect("has --");
    assert!(!subcommand_used(&tokens[..end], "jit"));
    // Sanity: without the truncation the spurious detection would fire,
    // proving the truncation is what protects the post-`--` values.
    assert!(subcommand_used(&tokens, "jit"));
}

#[test]
fn top_subcommand_is_first_non_flag_token() {
    let tokens = vec![
        String::from("check"),
        String::from("--output-format"),
        String::from("json"),
    ];
    assert_eq!(top_subcommand(&tokens), Some("check"));
}

#[test]
fn top_subcommand_ignores_check_as_a_value() {
    // `metrics --output-format json --paths check`: the `check` here is a
    // path value, not the subcommand, so the `--output-format` hint must
    // resolve to `metrics`/`--format`, not `check`/`--report-format`.
    let tokens = vec![
        String::from("metrics"),
        String::from("--output-format"),
        String::from("json"),
        String::from("--paths"),
        String::from("check"),
    ];
    assert_eq!(top_subcommand(&tokens), Some("metrics"));
}
