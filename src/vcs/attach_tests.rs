// Exact-equality on f64 is intentional: every compared hotspot is an
// exactly-representable product of small integers (60.0, 6.0, 15.0), as
// in `hotspot_tests.rs`.
#![allow(clippy::float_cmp)]

use std::path::PathBuf;

use super::*;
use crate::vcs::HistoryIndex;
use crate::{LANG, MetricSet, MetricsOptions, Source, analyze};

/// `outer` own 2 (one `if`), `inner` own 3 (two `if`s), so `outer`'s
/// subtree sum is 5 and the file's is 1 + 2 + 3 = 6. Hand-derived; the
/// setup asserts in `descendants_score_their_own_value…` hold the
/// analysis to it.
#[cfg(feature = "python")]
const NESTED: &[u8] = b"def outer(x):\n    if x:\n        pass\n    def inner(y):\n        if y:\n            pass\n        if y > 1:\n            pass\n";

#[cfg(feature = "python")]
fn nested_space(options: MetricsOptions) -> FuncSpace {
    analyze(Source::new(LANG::Python, NESTED), options).expect("the fixture parses")
}

#[cfg(any(feature = "python", feature = "rust"))]
fn stats_with_churn(churn_recent: u64) -> Stats {
    Stats {
        churn_recent,
        ..Stats::default()
    }
}

#[cfg(any(feature = "python", feature = "rust"))]
fn hotspot_of(space: &FuncSpace) -> Option<f64> {
    space.metrics.vcs.as_ref().and_then(|vcs| vcs.hotspot_score)
}

/// Every descendant scores from its own cyclomatic and `SubtreeSum`
/// reads the subtree sum (#1624). `outer`'s sum differs from its own
/// figure, so a descendant path scoring by sum moves its score.
#[cfg(feature = "python")]
#[test]
fn descendants_score_their_own_value_and_subtree_sum_reads_sum() {
    let mut root = nested_space(MetricsOptions::default());
    let outer = &root.spaces[0];
    assert_eq!(
        (
            outer.metrics.cyclomatic.cyclomatic(),
            outer.metrics.cyclomatic.cyclomatic_sum()
        ),
        (2, 5)
    );
    assert_eq!(root.metrics.cyclomatic.cyclomatic_sum(), 6);
    assert_eq!(file_complexity(&root.metrics), Some(6.0));

    attach_stats(
        &mut root,
        stats_with_churn(10),
        HotspotComplexity::SubtreeSum,
    );
    let mut stats = [stats_with_churn(3), stats_with_churn(5)].into_iter();
    assign_child_stats(&mut root, &mut stats);
    assert!(stats.next().is_none(), "one stat per descendant");

    assert_eq!(hotspot_of(&root), Some(60.0), "root: sum 6 x churn 10");
    let outer = &root.spaces[0];
    assert_eq!(
        hotspot_of(outer),
        Some(6.0),
        "outer: own 2 x churn 3, not sum 5"
    );
    assert_eq!(
        hotspot_of(&outer.spaces[0]),
        Some(15.0),
        "inner: own 3 x churn 5"
    );
}

/// Regression test for issue #709. A nested function inflates the
/// *outer* function's `cyclomatic_sum()` (subtree rollup) but not its own
/// `cyclomatic()`. The per-function walk must score each function by its
/// own complexity, otherwise the nested branches are re-counted at every
/// enclosing level. The isolated `attach_stats` probes show the two
/// figures yield different scores; driving `assign_child_stats` itself
/// then pins which one the walk picks, since a regression to
/// `SubtreeSum` there is invisible to probes that pass the figure
/// themselves.
#[cfg(feature = "rust")]
#[test]
fn per_function_hotspot_uses_own_not_subtree_cyclomatic() {
    const CHURN: u64 = 12;

    let code = br"
fn outer(x: i32) -> i32 {
    fn inner(y: i32) -> i32 {
        if y > 0 { 1 } else if y < 0 { -1 } else { 0 }
    }
    if x > 10 { inner(x) } else { 0 }
}
";
    let mut space = analyze(Source::new(LANG::Rust, code), MetricsOptions::default())
        .expect("the fixture parses");
    let outer = &mut space.spaces[0];
    let own = outer.metrics.cyclomatic.cyclomatic();
    let subtree = outer.metrics.cyclomatic.cyclomatic_sum();
    assert!(
        subtree > own,
        "the nested function must make subtree ({subtree}) exceed own ({own})"
    );

    attach_stats(outer, stats_with_churn(CHURN), HotspotComplexity::Own);
    let own_score = hotspot_of(outer);
    attach_stats(
        outer,
        stats_with_churn(CHURN),
        HotspotComplexity::SubtreeSum,
    );
    let sum_score = hotspot_of(outer);
    assert!(
        own_score.is_some_and(|score| score > 0.0),
        "own-cyclomatic path yields a positive hotspot score"
    );
    assert_ne!(
        own_score, sum_score,
        "own vs subtree complexity must produce distinct hotspot scores"
    );

    let mut stats = std::iter::once(stats_with_churn(CHURN));
    assign_child_stats(&mut space, &mut stats);
    let attached = hotspot_of(&space.spaces[0]);
    assert_eq!(
        attached, own_score,
        "assign_child_stats must score the outer function by its own cyclomatic"
    );
    assert_ne!(
        attached, sum_score,
        "assign_child_stats must not score by the subtree cyclomatic_sum"
    );
}

/// The issue's fixture: a nested function so that the file-level and the
/// per-function paths each attach a block, analysed with only `nargs`
/// selected so no space computed cyclomatic.
#[cfg(feature = "python")]
fn nargs_only_space() -> FuncSpace {
    let code = br"
def outer(x):
    if x:
        pass
    def inner(y):
        if y:
            return 1
        return 2
    return inner
";
    analyze(
        Source::new(LANG::Python, code),
        MetricsOptions::default().with_only(&[Metric::Nargs]),
    )
    .expect("the fixture parses")
}

/// Regression test for issue #1742. With `--metrics nargs` the file
/// space's `cyclomatic_sum()` reads 0 and every nested space's own
/// `cyclomatic()` reads its default 1, so an unconditional score
/// multiplies churn by a figure nobody computed (0.0 / 8.0 / 4.0 in the
/// issue). Both paths must leave `hotspot_score` `None`, which the wire
/// form then elides. Every stat fed in already carries a score, so the
/// two paths are shown to *reset* the field rather than merely leave an
/// unset one alone — a `HistoryIndex::new` caller may seed it.
#[cfg(feature = "python")]
#[test]
fn hotspot_is_absent_when_cyclomatic_was_not_computed() {
    const SEEDED: Option<f64> = Some(9.0);

    let mut space = nargs_only_space();
    assert!(
        !space.metrics.selected.contains(Metric::Cyclomatic),
        "the fixture must not have cyclomatic selected"
    );
    let inner = space.spaces[0]
        .spaces
        .first()
        .expect("nested inner function");
    assert_eq!(
        inner.metrics.cyclomatic.cyclomatic(),
        1,
        "an uncomputed per-space cyclomatic reads its default 1 — the \
         figure the unconditional score multiplied by"
    );
    assert_eq!(
        space.metrics.cyclomatic.cyclomatic_sum(),
        0,
        "an uncomputed file-level cyclomatic sum reads 0 — the other \
         figure the unconditional score multiplied by"
    );
    assert_eq!(
        file_complexity(&space.metrics),
        None,
        "the report join must not be handed the uncomputed sum either"
    );

    // `attach_file_history` keys the index by the canonical repo-relative
    // path, so a real file under a canonical workdir is needed: a raw
    // `tempdir()` path is not canonical where the temp root is a symlink
    // (macOS `/var/folders`, `/tmp` -> `/private/tmp`).
    let workdir = tempfile::tempdir().expect("tempdir");
    let workdir_root = workdir.path().canonicalize().expect("canonicalize workdir");
    let file = workdir.path().join("nested.py");
    std::fs::write(&file, b"").expect("write nested.py");
    let files = std::iter::once((
        PathBuf::from("nested.py"),
        Stats {
            churn_recent: 8,
            hotspot_score: SEEDED,
            ..Default::default()
        },
    ))
    .collect();
    let index = HistoryIndex::new(files, Some(workdir_root), false);
    assert!(
        attach_file_history(&mut space, &file, &index),
        "the tracked file attaches"
    );
    let file_vcs = space
        .metrics
        .vcs
        .as_ref()
        .expect("file space received a vcs block");
    assert_eq!(
        file_vcs.churn_recent, 8,
        "the file block is the index entry"
    );
    assert_eq!(
        file_vcs.hotspot_score, None,
        "attach_file_history must not score from an uncomputed \
         cyclomatic_sum, nor keep the seeded score"
    );

    // `outer` then `inner`, in the pre-order `collect_child_spans` walks.
    let mut stats = [8, 4].into_iter().map(|churn_recent| Stats {
        churn_recent,
        hotspot_score: SEEDED,
        ..Default::default()
    });
    assign_child_stats(&mut space, &mut stats);
    assert!(stats.next().is_none(), "both nested spaces consumed a stat");
    let outer_vcs = space.spaces[0]
        .metrics
        .vcs
        .as_ref()
        .expect("outer received a vcs block");
    let inner_vcs = space.spaces[0].spaces[0]
        .metrics
        .vcs
        .as_ref()
        .expect("inner received a vcs block");
    assert_eq!((outer_vcs.churn_recent, inner_vcs.churn_recent), (8, 4));
    assert_eq!(
        (outer_vcs.hotspot_score, inner_vcs.hotspot_score),
        (None, None),
        "assign_child_stats must not score from an uncomputed own \
         cyclomatic, nor keep the seeded scores"
    );
}

/// An untracked path — one the index does not carry — leaves the space
/// untouched and reports so.
#[cfg(feature = "python")]
#[test]
fn attach_file_history_reports_an_untracked_file() {
    let workdir = tempfile::tempdir().expect("tempdir");
    let workdir_root = workdir.path().canonicalize().expect("canonicalize workdir");
    let file = workdir.path().join("untracked.py");
    std::fs::write(&file, b"").expect("write untracked.py");
    let index = HistoryIndex::new(std::collections::HashMap::new(), Some(workdir_root), false);
    let mut space = nested_space(MetricsOptions::default());
    assert!(!attach_file_history(&mut space, &file, &index));
    assert!(space.metrics.vcs.is_none(), "an untracked file stays bare");
}

/// A path that cannot be canonicalized — here one whose parent directory
/// is gone, the shape of a tracked file deleted from the working tree —
/// is looked up as spelled (`attach_file_history`'s `unwrap_or_else`
/// fallback), so an index that carries that spelling still attaches.
/// Pins the fallback arm: a version that treated the canonicalize
/// failure as "not tracked" (`let Ok(canonical) = … else { return false }`)
/// returns `false` here and leaves the space bare; `.expect` panics.
/// The second lookup, at a spelling the index does not carry, shows the
/// `true` is the index entry's and not an unconditional attach.
#[cfg(feature = "python")]
#[test]
fn attach_file_history_looks_an_uncanonicalizable_path_up_as_spelled() {
    let workdir = tempfile::tempdir().expect("tempdir");
    let workdir_root = workdir.path().canonicalize().expect("canonicalize workdir");
    // Spelled from the canonical root, so the as-spelled lookup strips
    // cleanly; nothing under `gone/` exists on disk.
    let file = workdir_root.join("gone").join("deleted.py");
    assert!(
        file.canonicalize().is_err(),
        "the fixture path must not canonicalize"
    );
    let files = std::iter::once((PathBuf::from("gone/deleted.py"), stats_with_churn(7))).collect();
    let index = HistoryIndex::new(files, Some(workdir_root.clone()), false);

    let mut space = nested_space(MetricsOptions::default());
    assert!(
        attach_file_history(&mut space, &file, &index),
        "the index carries the spelled path, so the fallback lookup attaches"
    );
    let vcs = space
        .metrics
        .vcs
        .as_ref()
        .expect("the file block is attached");
    assert_eq!(vcs.churn_recent, 7, "the block is the index entry");
    assert_eq!(
        vcs.hotspot_score,
        Some(hotspot::hotspot_score(6.0, 7)),
        "scored from the subtree sum, as a canonicalizable path is"
    );

    let mut other = nested_space(MetricsOptions::default());
    assert!(
        !attach_file_history(
            &mut other,
            &workdir_root.join("gone").join("other.py"),
            &index
        ),
        "an uncanonicalizable path the index does not carry still reports untracked"
    );
    assert!(other.metrics.vcs.is_none());
}

/// A `Stats` reaching the library through the public `HistoryIndex::new`
/// may already carry a score; the gate must assign `None`, not skip the
/// assignment and let the stale value leak into the output. The space's
/// `selected` set is toggled directly, so this covers the gate for both
/// figures without a second fixture.
#[cfg(feature = "python")]
#[test]
fn attach_stats_clears_a_seeded_score_when_unselected_and_scores_when_selected() {
    let mut space = nested_space(MetricsOptions::default());
    assert_eq!(space.metrics.cyclomatic.cyclomatic_sum(), 6);
    let seeded = || Stats {
        churn_recent: 8,
        hotspot_score: Some(9.0),
        ..Default::default()
    };

    space.metrics.selected = MetricSet::empty().with(Metric::Nargs);
    attach_stats(&mut space, seeded(), HotspotComplexity::SubtreeSum);
    assert_eq!(
        space.metrics.vcs.as_ref().map(|vcs| vcs.churn_recent),
        Some(8),
        "the block is attached"
    );
    assert_eq!(
        hotspot_of(&space),
        None,
        "cyclomatic unselected clears the score"
    );
    assert_eq!(file_complexity(&space.metrics), None);

    space.metrics.selected = MetricSet::empty().with(Metric::Cyclomatic);
    attach_stats(&mut space, seeded(), HotspotComplexity::SubtreeSum);
    assert_eq!(
        hotspot_of(&space),
        Some(hotspot::hotspot_score(6.0, 8)),
        "cyclomatic selected scores complexity × recent churn"
    );
    assert_eq!(file_complexity(&space.metrics), Some(6.0));
}

/// A space with no descendants is a no-op that never blames. The proof
/// is the path: it does not exist in the repository, so the same
/// session returns the blame's error for a space that *has* descendants
/// — `Ok(())` on the leafless one can only be the early return.
#[cfg(feature = "python")]
#[test]
fn a_space_with_no_descendants_returns_ok_without_blaming() {
    use crate::vcs::{Options, PerFunctionBlame};

    let dir = tempfile::tempdir().expect("temp dir");
    let repo = gix::init(dir.path()).expect("init repository");
    // `open` resolves `HEAD`, so the repository needs one commit; the
    // empty tree is enough, since no file is meant to be found.
    let who = gix::actor::SignatureRef {
        name: "Ada".into(),
        email: "ada@example.com".into(),
        time: "1700000000 +0000",
    };
    repo.commit_as(
        who,
        who,
        "HEAD",
        "seed",
        repo.empty_tree().id,
        std::iter::empty::<gix::ObjectId>(),
    )
    .expect("seed commit");
    let engine = PerFunctionBlame::open(dir.path(), Options::default()).expect("open blame engine");
    let mut session = std::sync::Arc::new(engine).session();
    let missing = dir.path().join("missing.py");

    let mut nested = nested_space(MetricsOptions::default());
    attach_per_function_history(&mut nested, &missing, &mut session)
        .expect_err("a path the repository does not carry fails to blame");

    let mut leafless = analyze(
        Source::new(LANG::Python, b"x = 1\n"),
        MetricsOptions::default(),
    )
    .expect("the fixture parses");
    assert!(leafless.spaces.is_empty(), "the fixture has no descendants");
    attach_per_function_history(&mut leafless, &missing, &mut session)
        .expect("no descendants: nothing to blame, so the missing path is never looked up");
    assert!(leafless.metrics.vcs.is_none(), "the root is left untouched");
}

/// Both injector walks run before serialization bounds the space depth
/// at `MAX_SPACE_SERIALIZE_DEPTH`, so nesting far past it must reach the
/// serializer's reportable error rather than abort on the stack (#1770).
/// The chain is built directly, as `src/wire.rs`'s `space_chain` does,
/// because analysing this depth in a debug build is slow.
///
/// A regression fails this test as a stack-overflow abort of the test
/// binary, not a panic `join` could report: an overflow never unwinds.
/// The root is dropped on the same small-stack thread, so `FuncSpace`'s
/// iterative `Drop` (#1056) is exercised alongside.
#[cfg(feature = "python")]
#[test]
fn injector_walks_survive_nesting_past_the_serialize_bound() {
    const DEPTH: usize = 10_000;
    /// Far below a production thread's stack, so a walk that recurses
    /// per level fails here rather than riding on the test runner's.
    const TIGHT_STACK: usize = 128 * 1024;

    let file = analyze(
        Source::new(LANG::Python, b"def f():\n    pass\n"),
        MetricsOptions::default().with_only(&[Metric::Loc]),
    )
    .expect("the fixture parses");
    let leaf = file.spaces[0].clone();
    let mut root = file;
    let mut cursor = &mut root.spaces[0];
    for _ in 1..DEPTH {
        cursor.spaces.push(leaf.clone());
        cursor = &mut cursor.spaces[0];
    }

    let (spans, attached) = std::thread::Builder::new()
        .stack_size(TIGHT_STACK)
        .spawn(move || {
            let mut spans = Vec::new();
            collect_child_spans(&root, &mut spans);
            let mut stats = std::iter::repeat_with(Stats::default).take(spans.len());
            assign_child_stats(&mut root, &mut stats);
            let mut attached = 0;
            let mut cursor = &root;
            while let Some(child) = cursor.spaces.first() {
                attached += usize::from(child.metrics.vcs.is_some());
                cursor = child;
            }
            (spans.len(), attached)
        })
        .expect("spawn bounded-stack thread")
        .join()
        .expect("the walk thread must not panic");
    assert_eq!((spans, attached), (DEPTH, DEPTH));
}

/// The two walks agree on one pre-order: a shallow sibling before *and*
/// after a deep subtree, so a post-order walk, a breadth-first one, or
/// one that visits siblings in the wrong order hands the deep spaces
/// each other's stats, and the trailing sibling makes the walk resume an
/// ancestor's iterator after unwinding several levels — the one path the
/// explicit stack has that recursion did not. Every stat carries a
/// distinct `churn_recent`, so each space is checked for the one meant
/// for its position, not merely for having a block.
#[cfg(feature = "python")]
#[test]
fn child_stats_follow_the_span_pre_order() {
    // root → [a (leaf), b → [c → [d]], e (leaf)]
    let code = b"\
def a():
    pass

def b():
    def c():
        def d():
            pass

def e():
    pass
";
    let mut root = analyze(
        Source::new(LANG::Python, code),
        MetricsOptions::default().with_only(&[Metric::Loc]),
    )
    .expect("the fixture parses");
    let name_of = |space: &FuncSpace| space.name.clone().unwrap_or_default();
    assert_eq!(
        root.spaces.iter().map(name_of).collect::<Vec<_>>(),
        ["a", "b", "e"],
        "the fixture's top-level spaces"
    );

    let mut spans = Vec::new();
    collect_child_spans(&root, &mut spans);
    assert_eq!(
        spans,
        [
            LineSpan::new(1, 2),
            LineSpan::new(4, 7),
            LineSpan::new(5, 7),
            LineSpan::new(6, 7),
            LineSpan::new(9, 10),
        ],
        "spans are collected in pre-order: a, b, c, d, e"
    );

    // Churn 1 for `a`, 2 for `b`, 3 for `c`, 4 for `d`, 5 for `e`.
    let mut stats = (1..=5).map(stats_with_churn);
    assign_child_stats(&mut root, &mut stats);
    assert!(stats.next().is_none(), "every span's stat was consumed");

    let churn_of = |space: &FuncSpace| space.metrics.vcs.as_ref().map(|v| v.churn_recent);
    let outer_b = &root.spaces[1];
    let inner_c = &outer_b.spaces[0];
    let pre_order = [
        &root.spaces[0],
        outer_b,
        inner_c,
        &inner_c.spaces[0],
        &root.spaces[2],
    ];
    assert_eq!(
        pre_order.map(name_of),
        ["a", "b", "c", "d", "e"],
        "the fixture nests c under b and d under c, then e follows b"
    );
    assert_eq!(
        pre_order.map(churn_of),
        [Some(1), Some(2), Some(3), Some(4), Some(5)],
        "each space receives the stat collected for its pre-order position"
    );
}
