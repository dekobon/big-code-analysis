//! Unit tests for the argv-scan deprecation detector (#646). These cover
//! the spelling-detection logic in isolation; the integration suite
//! (`tests/deprecated_aliases.rs`) asserts the end-to-end stderr text and
//! that canonical spellings stay silent.

use super::{is_flag_spelling, subcommand_used, top_subcommand};

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
fn subcommand_used_quiet_for_canonical_commit() {
    let tokens = vec![
        String::from("vcs"),
        String::from("commit"),
        String::from("HEAD"),
    ];
    assert!(!subcommand_used(&tokens, "jit"));
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
