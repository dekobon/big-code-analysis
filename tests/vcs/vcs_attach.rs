//! End-to-end tests for [`big_code_analysis::vcs::attach`] (issue #1743)
//! against real git repositories: the path lookup the file-level attach
//! performs, and the blame the per-function attach runs. The hotspot
//! rule itself is pinned by the module's unit tests; these pin what it
//! does with a repository.
// Every fixture is Python; on a build without that grammar every import
// is unused, as in the sibling test crate roots.
#![cfg_attr(not(feature = "python"), allow(unused_imports))]

use big_code_analysis::vcs::attach::attach_per_function_history;
use big_code_analysis::vcs::{Options, PerFunctionBlame};
use big_code_analysis::{FuncSpace, LANG, MetricsOptions, Source, analyze};

use crate::common;
use common::vcs_fixture::{DAY, FIXED_NOW, Repo};

/// `outer` own 2 (one `if`), `inner` own 3 (two `if`s): one nested
/// function so the per-function attach has two descendants to fill.
#[cfg(feature = "python")]
const NESTED: &str = "def outer(x):\n    if x:\n        pass\n    def inner(y):\n        if y:\n            pass\n        if y > 1:\n            pass\n";

#[cfg(feature = "python")]
fn nested_space() -> FuncSpace {
    analyze(
        Source::new(LANG::Python, NESTED.as_bytes()),
        MetricsOptions::default(),
    )
    .expect("the fixture parses")
}

/// `HistoryIndex::get_for_path` strips the index's *canonical* work tree
/// lexically, so `attach_file_history` canonicalizes every path before
/// the lookup, unconditionally. This pins what that buys: a tracked file
/// attaches through a symlinked directory and a `..`-laden relative
/// spelling, an untracked one stays bare — and, the row a lexical-first
/// lookup cannot pass, a tracked path that is *itself* a symlink to
/// another tracked file attaches the target's history, since the
/// target's bytes are what was analysed (#1764). `a.py` has one commit
/// and `b.py` two; once `a.py` is a link to `b.py`, the block at `a.py`'s
/// path must read 2, not the 1 its own key carries.
#[cfg(all(unix, feature = "python"))]
#[test]
fn file_level_block_follows_the_path_the_bytes_came_from() {
    // Imported here rather than at the top: this is the only unix-gated
    // test, and a file-level import would be unused on Windows.
    use std::path::PathBuf;

    use big_code_analysis::vcs::attach::attach_file_history;
    use big_code_analysis::vcs::build_history_index;

    let repo = Repo::init();
    repo.write("work.py", NESTED);
    repo.write("a.py", NESTED);
    repo.write("b.py", NESTED);
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 2 * DAY, "add all");
    repo.write("b.py", &format!("{NESTED}# touched\n"));
    repo.commit("Ada", "ada@example.com", FIXED_NOW - DAY, "touch b");
    // `tempdir` itself may sit under a symlink (macOS `/var` →
    // `/private/var`), so the paths below start from the canonical root
    // and the links are the test's own, known-non-canonical ones.
    let root = repo.path().canonicalize().expect("canonical repo root");
    let link = root.join("link");
    std::os::unix::fs::symlink(&root, &link).expect("symlink the repo dir");
    std::fs::remove_file(root.join("a.py")).expect("remove a.py");
    std::os::unix::fs::symlink(root.join("b.py"), root.join("a.py")).expect("symlink a.py to b.py");
    let mut options = Options::default();
    options.as_of = Some(FIXED_NOW);
    let index = build_history_index(&root, &options).expect("the scratch repository indexes");

    let cwd = std::env::current_dir().expect("cwd");
    let ascent: PathBuf = cwd.components().skip(1).map(|_| "..").collect();
    let relative = ascent.join(root.strip_prefix("/").expect("canonical paths are rooted"));
    assert!(relative.is_relative());

    let attaches = |path: PathBuf| {
        let mut space = nested_space();
        let attached = attach_file_history(&mut space, &path, &index);
        let commits = space.metrics.vcs.as_ref().map(|vcs| vcs.commits_long);
        assert_eq!(
            attached,
            commits.is_some(),
            "the return value reports whether a block was attached"
        );
        commits
    };
    assert_eq!(attaches(root.join("work.py")), Some(1), "canonical path");
    assert_eq!(
        attaches(link.join("work.py")),
        Some(1),
        "through the symlink"
    );
    assert_eq!(
        attaches(relative.join("work.py")),
        Some(1),
        "relative, `..`-laden"
    );
    assert_eq!(attaches(root.join("b.py")), Some(2), "the link's target");
    assert_eq!(
        attaches(root.join("a.py")),
        Some(2),
        "a tracked path that is a symlink to another tracked file \
         carries the target's history"
    );
    repo.write("untracked.py", NESTED);
    assert_eq!(
        attaches(root.join("untracked.py")),
        None,
        "untracked stays bare"
    );
    assert_eq!(
        attaches(link.join("untracked.py")),
        None,
        "untracked via the link"
    );
}

/// One blame fills every descendant and leaves the root alone; an
/// unblameable path returns the error with no descendant touched, so
/// the caller can report or ignore it.
#[cfg(feature = "python")]
#[test]
fn per_function_attach_fills_descendants_and_returns_the_blame_error() {
    let repo = Repo::init();
    repo.write("work.py", NESTED);
    repo.commit("Ada", "ada@example.com", FIXED_NOW - DAY, "add");
    let mut options = Options::default();
    options.as_of = Some(FIXED_NOW);
    let engine = PerFunctionBlame::open(repo.path(), options).expect("open blame engine");
    let mut session = std::sync::Arc::new(engine).session();

    let mut space = nested_space();
    attach_per_function_history(&mut space, &repo.path().join("work.py"), &mut session)
        .expect("a tracked file blames");
    assert!(space.metrics.vcs.is_none(), "the root is the file block's");
    let outer = &space.spaces[0];
    let inner = &outer.spaces[0];
    let block = |space: &FuncSpace| {
        space
            .metrics
            .vcs
            .as_ref()
            .map(|vcs| (vcs.commits_long, vcs.hotspot_score))
    };
    // One commit wrote every line; `outer` spans all 8 lines and `inner`
    // its 5, so churn is the surviving line count and the score is own
    // cyclomatic × churn: 2 × 8 and 3 × 5.
    assert_eq!(
        block(outer),
        Some((1, Some(16.0))),
        "outer: own 2 x 8 lines"
    );
    assert_eq!(
        block(inner),
        Some((1, Some(15.0))),
        "inner: own 3 x 5 lines"
    );

    let mut untracked = nested_space();
    repo.write("untracked.py", NESTED);
    let err = attach_per_function_history(
        &mut untracked,
        &repo.path().join("untracked.py"),
        &mut session,
    )
    .expect_err("an untracked file does not blame");
    assert!(
        !err.to_string().is_empty(),
        "the error is the backend's, for the caller to report"
    );
    assert!(
        untracked.spaces[0].metrics.vcs.is_none(),
        "no descendant was touched on the error path"
    );
}
