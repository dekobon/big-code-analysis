//! Unit tests for the integration-corpus guard in `tests/common`.
//!
//! Nineteen tests in this crate analyse a real source file from the
//! `DeepSpeech` corpus, which is a git submodule and therefore absent
//! from a fresh clone or `git worktree`. Before #1171 each of them
//! failed with `bca`'s generic `error: path does not exist: …`, which
//! reads as a bug in whatever the author was changing.
//!
//! The guard is tested against synthetic trees rather than the real
//! corpus: the corpus is present in every tree where this suite runs,
//! so a test that waited for its absence would never execute (see
//! `.claude/rules/testing.md`).

use std::fs;

use crate::common::corpus_checkout_hint;

/// Build a synthetic workspace root whose `tests/repositories/Corpus`
/// directory holds `entries`, and return it with its `TempDir` guard.
fn workspace_with(entries: &[&str]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("create tempdir");
    let corpus = dir.path().join("tests/repositories/Corpus");
    fs::create_dir_all(&corpus).expect("create corpus dir");
    for entry in entries {
        fs::write(corpus.join(entry), "content\n").expect("write corpus entry");
    }
    let root = dir.path().to_path_buf();
    (dir, root)
}

#[test]
fn present_fixture_produces_no_hint() {
    let (_guard, root) = workspace_with(&["stats.py"]);
    assert_eq!(corpus_checkout_hint(&root, "Corpus", "stats.py"), None);
}

#[test]
fn empty_corpus_reads_as_not_checked_out() {
    let (_guard, root) = workspace_with(&[]);
    let hint = corpus_checkout_hint(&root, "Corpus", "stats.py")
        .expect("an empty corpus directory must produce a hint");
    assert!(
        hint.contains("integration corpus not checked out"),
        "hint must name the cause, got: {hint}"
    );
    assert!(
        hint.contains("make worktree-setup"),
        "hint must name the remedy, got: {hint}"
    );
    assert!(
        hint.contains("--force"),
        "hint must say the by-hand recovery needs --force, got: {hint}"
    );
}

#[test]
fn a_corpus_holding_only_dot_git_still_reads_as_not_checked_out() {
    // The interrupted-checkout shape: git writes the submodule's `.git`
    // file before any content, so its presence alone is not evidence
    // that the corpus is usable.
    let (_guard, root) = workspace_with(&[".git"]);
    let hint = corpus_checkout_hint(&root, "Corpus", "stats.py")
        .expect("a corpus holding only .git must produce a hint");
    assert!(
        hint.contains("integration corpus not checked out"),
        "a lone .git must not read as partial content, got: {hint}"
    );
}

#[test]
fn a_populated_corpus_missing_the_fixture_reads_as_partial() {
    // The other #1171 tell: "a corpus directory containing only
    // snapshots/". Some content arrived, the wanted file did not.
    let (_guard, root) = workspace_with(&[".git", "README.rst"]);
    let hint = corpus_checkout_hint(&root, "Corpus", "stats.py")
        .expect("a corpus missing the fixture must produce a hint");
    assert!(
        hint.contains("integration corpus partially checked out"),
        "partial content must be distinguished from an absent corpus, got: {hint}"
    );
    assert!(
        hint.contains("--force"),
        "the partial case is the one --force exists for, got: {hint}"
    );
}

#[test]
fn an_absent_corpus_directory_reads_as_not_checked_out() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let hint = corpus_checkout_hint(dir.path(), "Corpus", "stats.py")
        .expect("a missing corpus directory must produce a hint");
    assert!(
        hint.contains("integration corpus not checked out"),
        "hint must name the cause, got: {hint}"
    );
}
