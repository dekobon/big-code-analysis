//! Change-history file-type scoping (issue #576): `bca vcs` ranks only
//! files-with-metrics by default, reproduces the whole-tree ranking under
//! `--file-types all`, and honours a custom extension allow-list.
//!
//! Each walk is exercised against a real, deterministic git repository
//! holding both source and non-source files, with `as_of` pinned to the
//! fixture clock. Gated behind the `vcs-git` backend feature.
#![cfg(feature = "vcs-git")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use big_code_analysis::get_language_for_file;
use big_code_analysis::vcs::{self, FileTypeScope, Options, build_history_index};

mod common;
use common::vcs_fixture::{DAY, FIXED_NOW, Repo};

/// Every tracked file the fixture repository carries: a mix of source
/// (`.rs`, `.py`), docs (`.md`), config / lockfiles (`.toml`, `.lock`),
/// and an extension-less file.
const TRACKED_FILES: &[&str] = &[
    "src/lib.rs",
    "app/main.py",
    "README.md",
    "Cargo.toml",
    "Cargo.lock",
    "Makefile",
];

/// Build a repo containing one commit that adds every [`TRACKED_FILES`]
/// entry, so a single history walk sees them all.
fn fixture() -> Repo {
    let repo = Repo::init();
    for rel in TRACKED_FILES {
        repo.write(rel, "one\ntwo\nthree\n");
    }
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 10 * DAY, "initial");
    repo
}

/// Options pinned to the fixture clock with the given file-type scope.
fn opts(file_types: FileTypeScope) -> Options {
    Options {
        as_of: Some(FIXED_NOW),
        file_types,
        ..Options::default()
    }
}

/// The repo-relative paths the index ranked, as a sorted set for stable
/// comparison.
fn ranked_paths(index: &vcs::HistoryIndex) -> BTreeSet<String> {
    index
        .iter()
        // The fixture paths are ASCII, so `to_str` never fails; the
        // `expect` documents that invariant rather than mangling a path.
        .map(|(rel, _)| rel.to_str().expect("ascii path").to_owned())
        .collect()
}

#[test]
fn metrics_scope_is_the_default_and_ranks_only_files_with_metrics() {
    let repo = fixture();
    // The bare default must already be `metrics` (no explicit scope set).
    let index = build_history_index(repo.path(), &opts(FileTypeScope::Metrics)).expect("walk");
    assert_eq!(
        ranked_paths(&index),
        BTreeSet::from(["src/lib.rs".to_owned(), "app/main.py".to_owned()]),
        "only source files bca has metrics for are ranked by default"
    );
    // Default-constructed options agree with the explicit Metrics scope.
    assert_eq!(Options::default().file_types, FileTypeScope::Metrics);
}

#[test]
fn all_scope_reproduces_the_whole_tree_ranking() {
    let repo = fixture();
    let index = build_history_index(repo.path(), &opts(FileTypeScope::All)).expect("walk");
    let expected: BTreeSet<String> = TRACKED_FILES.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(
        ranked_paths(&index),
        expected,
        "`all` ranks every tracked, non-binary, non-symlink file"
    );
}

#[test]
fn custom_scope_ranks_only_listed_extensions() {
    let repo = fixture();
    // A custom list is a literal extension filter: it can include a
    // non-metrics extension (`toml`) and excludes everything else.
    let scope = "rs,toml".parse::<FileTypeScope>().expect("custom scope");
    let index = build_history_index(repo.path(), &opts(scope)).expect("walk");
    assert_eq!(
        ranked_paths(&index),
        BTreeSet::from(["src/lib.rs".to_owned(), "Cargo.toml".to_owned()]),
        "only the `rs` and `toml` files are ranked"
    );
}

#[test]
fn metrics_scope_equals_the_files_bca_metrics_would_analyze() {
    // Acceptance criterion: the `metrics` scope must equal the file set
    // `bca metrics` analyzes for the same selection — tested directly by
    // routing each tracked file through the same `get_language_for_file`
    // predicate the metrics walk resolves a language with, rather than
    // assuming the two agree.
    let repo = fixture();
    let index = build_history_index(repo.path(), &opts(FileTypeScope::Metrics)).expect("walk");

    let expected: BTreeSet<String> = TRACKED_FILES
        .iter()
        .filter(|rel| get_language_for_file(&PathBuf::from(rel)).is_some())
        .map(|rel| (*rel).to_owned())
        .collect();

    assert_eq!(
        ranked_paths(&index),
        expected,
        "the metrics scope tracks exactly the analyzable-file predicate"
    );
    // Sanity: the predicate is non-trivial here (some files in, some out).
    assert!(
        !expected.is_empty(),
        "fixture must contain analyzable files"
    );
    assert!(
        expected.len() < TRACKED_FILES.len(),
        "fixture must contain non-analyzable files too"
    );
}

#[test]
fn extension_less_file_is_excluded_under_metrics() {
    // `Makefile` carries no extension, so the extension-only `metrics`
    // predicate excludes it — identical to what `bca metrics` does (it
    // never resolves a language from the bare name here).
    let repo = fixture();
    let index = build_history_index(repo.path(), &opts(FileTypeScope::Metrics)).expect("walk");
    assert!(
        index.get(Path::new("Makefile")).is_none(),
        "an extension-less file is out of the metrics scope"
    );
}
