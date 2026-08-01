//! End-to-end per-function change-history tests (issue #329) against
//! real, deterministic git repositories.
//!
//! These drive [`big_code_analysis::vcs::PerFunctionBlame`] directly with
//! hand-specified [`LineSpan`]s rather than going through the AST walk,
//! so the assertions pin the blame → bucket → aggregate pipeline itself
//! without coupling to any one language's exact `FuncSpace` line spans.
//! Every commit carries a fixed identity and UNIX timestamp and the
//! engine pins `as_of` to [`FIXED_NOW`], so counts are exact. Gated
//! behind the `vcs-git` backend feature.
#![cfg(feature = "vcs-git")]
// Exact-equality on f64 is intentional: the compared ownership ratios
// are exact integer fractions (1.0, 0.5) representable in binary f64.
#![allow(clippy::float_cmp)]

use big_code_analysis::vcs::{LineSpan, Options, PerFunctionBlame, Stats};

mod common;
use common::vcs_fixture::{DAY, FIXED_NOW, Repo};

/// Options pinned to the fixture clock; tweak fields per test.
fn opts() -> Options {
    let mut options = Options::default();
    options.as_of = Some(FIXED_NOW);
    options
}

/// Blame `rel` in `repo` and return one `Stats` per span, in order.
fn per_function(repo: &Repo, options: Options, rel: &str, spans: &[LineSpan]) -> Vec<Stats> {
    let engine = PerFunctionBlame::open(repo.path(), options).expect("open blame engine");
    engine
        .per_function(&repo.path().join(rel), spans)
        .expect("blame file")
}

/// A two-function fixture: `first` on lines 1-3, `second` on lines 4-6.
const TWO_FUNCS: &str = "fn first() {\n    let x = 1;\n}\nfn second() {\n    let y = 2;\n}\n";

#[test]
fn attributes_each_commit_to_its_enclosing_function() {
    let repo = Repo::init();
    // Both functions created 200 days ago (inside the 365-day long
    // window, outside the 90-day recent window).
    repo.write("src/work.rs", TWO_FUNCS);
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 200 * DAY, "create");
    // Five days ago, Grace edits only line 5 — inside `second`.
    repo.write(
        "src/work.rs",
        "fn first() {\n    let x = 1;\n}\nfn second() {\n    let y = 22;\n}\n",
    );
    repo.commit("Grace", "grace@example.com", FIXED_NOW - 5 * DAY, "tweak");

    let stats = per_function(
        &repo,
        opts(),
        "src/work.rs",
        &[LineSpan::new(1, 3), LineSpan::new(4, 6)],
    );
    let (first, second) = (&stats[0], &stats[1]);

    // `first` is untouched since creation: one author, one (non-recent)
    // commit, all three lines surviving from it.
    assert_eq!(first.commits_long, 1, "first: only the create commit");
    assert_eq!(first.commits_recent, 0, "first: create is 200d old");
    assert_eq!(first.authors_long, 1);
    assert_eq!(first.churn_long, 3, "first: 3 surviving lines");
    assert_eq!(first.churn_recent, 0);
    assert_eq!(first.age_days, 200);
    assert_eq!(first.last_modified_days, 200);
    assert_eq!(first.ownership_top_share, 1.0);

    // `second` was touched by both commits: Grace's edit is recent and
    // owns exactly one surviving line; Ada's two lines remain.
    assert_eq!(second.commits_long, 2, "second: create + tweak");
    assert_eq!(second.commits_recent, 1, "second: only the tweak is recent");
    assert_eq!(second.authors_long, 2, "second: Ada and Grace");
    assert_eq!(second.authors_recent, 1, "second: only Grace recently");
    assert_eq!(second.churn_long, 3, "second: 3 surviving lines total");
    assert_eq!(second.churn_recent, 1, "second: Grace's single line");
    assert_eq!(second.last_modified_days, 5);
    assert_eq!(second.age_days, 200);
    assert_eq!(
        second.ownership_top_share, 0.5,
        "second: one commit each → tied ownership"
    );
}

#[test]
fn lines_older_than_the_long_window_drop_to_zero() {
    let repo = Repo::init();
    // `first` created 800 days ago — well outside the 365-day window.
    repo.write("src/work.rs", TWO_FUNCS);
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 800 * DAY, "ancient");
    // `second` rewritten 10 days ago.
    repo.write(
        "src/work.rs",
        "fn first() {\n    let x = 1;\n}\nfn second() {\n    let y = 99;\n}\n",
    );
    repo.commit("Grace", "grace@example.com", FIXED_NOW - 10 * DAY, "fresh");

    let stats = per_function(
        &repo,
        opts(),
        "src/work.rs",
        &[LineSpan::new(1, 3), LineSpan::new(4, 6)],
    );
    let (first, second) = (&stats[0], &stats[1]);

    // `first`'s only commit predates the window: it contributes no
    // commits, authors, or churn, and its age/last-modified saturate at
    // the long window (365 days) — the documented "stable function" case.
    assert_eq!(first.commits_long, 0, "ancient commit is out of window");
    assert_eq!(first.churn_long, 0, "no in-window surviving lines");
    assert_eq!(first.authors_long, 0);
    assert_eq!(first.age_days, 365, "saturates at the long window");
    assert_eq!(first.last_modified_days, 365);

    // `second` keeps line 5 (Grace, in window) but lines 4 and 6 trace to
    // the ancient commit and are dropped from the windowed counts.
    assert_eq!(second.commits_long, 1, "only Grace's commit is in window");
    assert_eq!(second.churn_long, 1, "only line 5 survives in window");
    assert_eq!(second.last_modified_days, 10);
}

#[test]
fn bot_only_commit_excluded_by_default() {
    let blame = |exclude_bots: bool| {
        let repo = Repo::init();
        repo.write("src/work.rs", TWO_FUNCS);
        repo.commit("Ada", "ada@example.com", FIXED_NOW - 30 * DAY, "human");
        // A bot rewrites line 5 inside `second`.
        repo.write(
            "src/work.rs",
            "fn first() {\n    let x = 1;\n}\nfn second() {\n    let y = 7;\n}\n",
        );
        repo.commit(
            "dependabot[bot]",
            "49699333+dependabot[bot]@users.noreply.github.com",
            FIXED_NOW - 5 * DAY,
            "bump",
        );
        let mut options = opts();
        options.exclude_bots = exclude_bots;
        let stats = per_function(&repo, options, "src/work.rs", &[LineSpan::new(4, 6)]);
        (stats[0].commits_long, stats[0].authors_long)
    };

    // Default: the bot's line is dropped, leaving only Ada's two lines.
    assert_eq!(blame(true), (1, 1), "bot commit excluded");
    // Opt-in: both the human and the bot count.
    assert_eq!(blame(false), (2, 2), "bot commit included");
}

#[test]
fn follows_a_file_rename_across_history() {
    // `first` (lines 1-3) is created 40 days ago under `old.rs`, then the
    // file is renamed to `new.rs` and only line 5 (inside `second`) is
    // edited 5 days ago. Blaming span 1-3 on `new.rs` must trace those
    // lines back through the rename to the 40-day-old create.
    //
    // The distinguishing signal is `last_modified_days`, NOT the commit
    // count: without rename-following, blame sees `new.rs` as wholly
    // introduced by the 5-day-old move commit, so it *also* reports
    // commits_long == 1 / churn_long == 3 — only the age differs (40 vs 5).
    let blame_first = |follow_renames: bool| {
        let repo = Repo::init();
        repo.write("old.rs", TWO_FUNCS);
        repo.commit("Ada", "ada@example.com", FIXED_NOW - 40 * DAY, "create old");
        repo.git(&["mv", "old.rs", "new.rs"]);
        repo.write(
            "new.rs",
            "fn first() {\n    let x = 1;\n}\nfn second() {\n    let y = 3;\n}\n",
        );
        repo.commit(
            "Grace",
            "grace@example.com",
            FIXED_NOW - 5 * DAY,
            "move + edit",
        );
        let mut options = opts();
        options.follow_renames = follow_renames;
        let mut stats = per_function(&repo, options, "new.rs", &[LineSpan::new(1, 3)]);
        stats.remove(0)
    };

    // Following renames (the default): `first`'s lines attribute to Ada's
    // 40-day-old create under the former path `old.rs`.
    let followed = blame_first(true);
    assert_eq!(followed.commits_long, 1);
    assert_eq!(followed.churn_long, 3);
    assert_eq!(
        followed.last_modified_days, 40,
        "create is seen through the rename"
    );

    // Not following: blame stops at the rename commit, so the lines
    // attribute to Grace's 5-day-old move instead — proving the assertion
    // above actually exercises rename-following rather than passing either
    // way.
    let stopped = blame_first(false);
    assert_eq!(
        stopped.last_modified_days, 5,
        "without follow, attribution stops at the move commit"
    );
}

#[test]
fn untracked_file_blame_errors() {
    let repo = Repo::init();
    repo.write("tracked.rs", TWO_FUNCS);
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 5 * DAY, "init");
    // A file that exists on disk but was never committed has nothing to
    // blame; the caller treats the error as "leave per-function unset".
    std::fs::write(repo.path().join("untracked.rs"), TWO_FUNCS).expect("write");

    let engine = PerFunctionBlame::open(repo.path(), opts()).expect("open");
    let result = engine.per_function(&repo.path().join("untracked.rs"), &[LineSpan::new(1, 3)]);
    // Specifically the blame-failure variant, not an earlier open/resolve
    // error — the caller distinguishes "skip per-function" from a fatal one.
    assert!(
        matches!(result, Err(big_code_analysis::vcs::Error::Blame(_))),
        "blaming an untracked file should return Error::Blame, got {result:?}"
    );
}

#[test]
fn perf_10k_lines_50_commits_under_8s() {
    // Issue #329's acceptance criterion was literally "per-function VCS on
    // a 10 000-line, 200-commit file completes well under 30 s". #1125
    // rescaled the fixture to 50 commits: the 10 000 lines / 2000 spans
    // stay, because span count is the dimension per-function bucketing
    // scales in, and the per-commit budget is preserved (30 s / 200 and
    // 8 s / 50 are both ~0.15 s per commit). What the shrink does trade
    // away is sensitivity to a *superlinear*-in-history regression: a
    // quadratic one now has to be ~4x worse before it trips.
    //
    // We build a 2000-function file (5 lines each), make 50 commits each
    // editing a spread-out line, then time the single whole-file blame +
    // bucketing across all 2000 spans. Only the blame call is timed — the
    // fixture's `git` invocations are setup, not the measured work.
    //
    // Fixture size and budget were re-derived together, and must stay that
    // way: leaving 30 s over a quarter-size fixture would quadruple the
    // regression the assertion tolerates while covering less. Measured on
    // one 16-core host under concurrent load, debug build, median of 5
    // runs (min in brackets):
    //
    //   2000 funcs / 200 commits (pre-#1125): blame 0.53 s [0.49], 407
    //     `git` spawns. Budget 30 s ⇒ 57x headroom.
    //   2000 funcs /  50 commits (current):   blame 0.14 s [0.13], 107
    //     `git` spawns. Budget  8 s ⇒ 57x headroom.
    //
    // That headroom is what carries over, not the 30 s literal, and it is
    // deliberately loose: at 57x this is a smoke alarm for a catastrophic
    // regression, not a benchmark. A 10x slowdown passes. It is sized so a
    // shared CI runner under load cannot flake it.
    use std::fmt::Write as _;
    const FUNCS: u32 = 2_000;
    const LINES_PER_FUNC: u32 = 5;
    const COMMITS: usize = 50;
    // Stride between successive edited functions, so the edited set is
    // exactly the stride's multiples — which is what the work-product
    // assertions below are derived from. That identity needs every commit
    // to land on a distinct function, hence the bound.
    const EDIT_STRIDE: usize = 7;
    const _: () = assert!(
        COMMITS * EDIT_STRIDE < FUNCS as usize,
        "edited functions must stay distinct and in range"
    );

    // Unique identifiers per line so the fixture resembles real source
    // rather than 10_000 near-identical lines. (Pathologically repetitive
    // content trips a gix-blame robustness bug — see the module-level
    // limitations note in `src/vcs/git/blame.rs`; real code does not.)
    let render = |values: &[u64]| {
        let mut body = String::with_capacity(values.len() * 32 * LINES_PER_FUNC as usize);
        for (f, value) in values.iter().enumerate() {
            // `write!` to the buffer rather than `push_str(&format!(..))`
            // to avoid a transient allocation per line over 10k lines.
            let _ = write!(
                body,
                "fn func_{f}() {{\n    let value_{f}: u64 = {value};\n    record_{f}(value_{f});\n    finalize_{f}();\n}}\n"
            );
        }
        body
    };

    let mut values: Vec<u64> = (0..FUNCS).map(u64::from).collect();
    let repo = Repo::init();
    repo.write("big.rs", &render(&values));
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 300 * DAY, "seed");
    for commit in 1..=COMMITS {
        // Each commit edits exactly one function's value line, spreading
        // localized edits across the file over history (as real commits do).
        values[commit * EDIT_STRIDE] += 1;
        repo.write("big.rs", &render(&values));
        // Quarter-day spacing — the only sub-day commit spacing in
        // `tests/`, so this is also the one fixture that puts several
        // commits inside a single `age_days` bucket.
        let age = i64::try_from(COMMITS - commit).expect("commit age fits i64");
        let secs = FIXED_NOW - age * DAY / 4;
        repo.commit("Ada", "ada@example.com", secs, &format!("edit {commit}"));
    }

    // One span per function: lines `5*f + 1 ..= 5*f + 5` (1-based). All
    // arithmetic stays in `u32`, so no narrowing cast is needed.
    let spans: Vec<LineSpan> = (0..FUNCS)
        .map(|f| {
            let start = f * LINES_PER_FUNC + 1;
            LineSpan::new(start, start + LINES_PER_FUNC - 1)
        })
        .collect();

    let engine = PerFunctionBlame::open(repo.path(), opts()).expect("open");
    let path = repo.path().join("big.rs");
    let start = std::time::Instant::now();
    let stats = match engine.per_function(&path, &spans) {
        Ok(stats) => stats,
        // This deep-history, large-file fixture is exactly the stress case
        // that can provoke the non-deterministic gix-odb pack-refresh race
        // (issue #579) — see the limitations note in src/vcs/git/blame.rs.
        // `per_function` already retries the transient miss; on the rare run
        // where every retry still misses, production skips the file's
        // per-function blocks rather than aborting, so this perf test mirrors
        // that and skips rather than flaking. Only the three documented
        // transient wordings are tolerated; any other blame error still fails.
        //
        // The race surfaces in three `Display` forms, one per retried gix
        // lookup (see `is_transient_blame_miss` / `is_transient_object_miss`
        // in src/vcs/git/blame.rs):
        //   - "iterator over a tree"  — `gix::blame::Error::FindExistingIter`
        //   - "blob or commit"        — `gix::blame::Error::FindExistingObject`
        //   - "could not be found"    — the post-blame commit `NotFound` lookup
        Err(big_code_analysis::vcs::Error::Blame(reason))
            if reason.contains("iterator over a tree")
                || reason.contains("blob or commit")
                || reason.contains("could not be found") =>
        {
            eprintln!("skipping perf assertion: transient gix-odb race (#579): {reason}");
            return;
        }
        Err(other) => panic!("blame: {other:?}"),
    };
    let elapsed = start.elapsed();

    assert_eq!(
        stats.len(),
        FUNCS as usize,
        "one stats record per function span"
    );
    // A timing assertion that does not check the work product is worthless:
    // a regression that returns early with empty or all-zero stats would
    // pass the budget trivially. Both windows are known exactly by
    // construction, so assert the exact history rather than "non-zero" —
    // `>= 1` would also admit a regression that credited every function
    // with the whole file's history.
    let edited: Vec<usize> = (1..=COMMITS).map(|c| c * EDIT_STRIDE).collect();
    // The seed commit is 300 days old, inside the default 12mo long
    // window, so every function keeps it; an edited function additionally
    // keeps the commit that rewrote its value line.
    let expect_long = |index: usize| if edited.contains(&index) { 2 } else { 1 };
    let wrong: Vec<(usize, u32)> = stats
        .iter()
        .enumerate()
        .filter(|(index, s)| s.commits_long != expect_long(*index))
        .map(|(index, s)| (index, s.commits_long))
        .collect();
    assert!(
        wrong.is_empty(),
        "{} function(s) have an unexpected commits_long; first few \
         (index, value): {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(5)]
    );
    // Every edit commit lands inside the default 90d recent window, so the
    // recent bucket names exactly the edited functions. Pinning the set
    // (not its size) catches attribution that lands on the wrong span.
    let with_recent: Vec<usize> = stats
        .iter()
        .enumerate()
        .filter(|(_, s)| s.commits_recent > 0)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        with_recent, edited,
        "recent commits should land on exactly the edited functions"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(8),
        "per-function blame took {elapsed:?}, over the 8s budget"
    );
}

#[test]
fn empty_span_list_yields_no_stats() {
    let repo = Repo::init();
    repo.write("src/work.rs", TWO_FUNCS);
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 5 * DAY, "init");

    let stats = per_function(&repo, opts(), "src/work.rs", &[]);
    assert!(stats.is_empty(), "no spans → no per-function stats");
}

/// A [`BlameSession`] reused across files must answer exactly what a
/// fresh one-shot engine call answers for each of them (issue #1117).
///
/// The session carries a commit-metadata memo across files, so the risk
/// the hoist introduces is cross-file contamination: a commit resolved
/// while blaming `a.rs` is reused when `b.rs` blames the same commit, and
/// a memo keyed or filtered wrongly would silently mis-attribute. The
/// fixture is built so that matters — every file shares the `shared`
/// commit, and the files differ in which *other* commits touch them, so
/// a memo that leaked per-file state would show up as a wrong
/// `commits_long` / `authors_long` rather than as an equal-but-wrong
/// value on both sides.
///
/// The session also blames one file *twice*, out of order relative to
/// the reference pass, so a memo that mutated on read (or an outcome
/// cached against the wrong path) is caught too.
#[test]
fn session_matches_one_shot_across_files_and_repeat_blames() {
    let repo = Repo::init();
    // One commit touching every file: the memo entry each later blame
    // reuses.
    repo.write("src/a.rs", TWO_FUNCS);
    repo.write("src/b.rs", TWO_FUNCS);
    repo.write("src/c.rs", TWO_FUNCS);
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 200 * DAY, "shared");
    // Then per-file edits by different authors, so the three files do
    // not share a single answer that any implementation would produce.
    repo.write(
        "src/a.rs",
        "fn first() {\n    let x = 11;\n}\nfn second() {\n    let y = 2;\n}\n",
    );
    repo.commit("Grace", "grace@example.com", FIXED_NOW - 5 * DAY, "edit a");
    repo.write(
        "src/b.rs",
        "fn first() {\n    let x = 1;\n}\nfn second() {\n    let y = 22;\n}\n",
    );
    repo.commit("Alan", "alan@example.com", FIXED_NOW - 50 * DAY, "fix b");

    let spans = [LineSpan::new(1, 3), LineSpan::new(4, 6)];
    let files = ["src/a.rs", "src/b.rs", "src/c.rs"];

    // Reference: one engine per call, exactly as the pre-session code ran.
    let expected: Vec<Vec<Stats>> = files
        .iter()
        .map(|rel| per_function(&repo, opts(), rel, &spans))
        .collect();
    // The fixture is only meaningful if the three files really differ.
    assert_ne!(
        expected[0], expected[1],
        "fixture is degenerate: a.rs and b.rs blame identically, so a \
         contaminated memo could not be distinguished from a clean one"
    );

    let engine =
        std::sync::Arc::new(PerFunctionBlame::open(repo.path(), opts()).expect("open engine"));
    let mut session = engine.session();
    // Reverse order, with `src/a.rs` blamed again at the end against a
    // memo warmed by every other file.
    for (rel, want) in files.iter().zip(&expected).rev() {
        let got = session
            .per_function(&repo.path().join(rel), &spans)
            .expect("session blame");
        assert_eq!(&got, want, "session diverged from one-shot for {rel}");
    }
    let again = session
        .per_function(&repo.path().join("src/a.rs"), &spans)
        .expect("session re-blame");
    assert_eq!(
        again, expected[0],
        "re-blaming a file through a warm session changed its answer"
    );

    // A failed blame must leave the session usable: the memo is filled
    // only on the `Ok` branch, so a mid-blame error writes nothing and
    // the next file must still answer correctly.
    let missing = repo.path().join("src/never-committed.rs");
    std::fs::write(&missing, TWO_FUNCS).expect("write untracked file");
    assert!(
        session.per_function(&missing, &spans).is_err(),
        "an untracked file must fail rather than blame"
    );
    assert_eq!(
        session
            .per_function(&repo.path().join("src/b.rs"), &spans)
            .expect("blame after a failure"),
        expected[1],
        "a failed blame poisoned the session"
    );
}

/// The session's commit memo must accumulate **across** files, not just
/// within one blame (issue #1117).
///
/// This is the perf invariant the whole change exists for, and it is
/// invisible to every value assertion: a memo cleared at the top of each
/// blame returns identical `Stats`. `commits_resolved` can see it, but
/// only against a fixture where the running union is larger than any one
/// file's commit set — otherwise a per-call clear reports the same
/// numbers a working memo does. So `a.rs` and `b.rs` are given
/// **disjoint** commits: after blaming both, a memo holds 3 and a clear
/// holds `b.rs`'s 1. Verified by inserting `meta.clear()` in
/// `blame_spans` — this test is the only failure.
#[test]
fn session_memoises_commits_across_files() {
    let repo = Repo::init();
    // a.rs: created, then edited — two commits, neither touching b.rs.
    repo.write("src/a.rs", TWO_FUNCS);
    repo.commit("Ada", "ada@example.com", FIXED_NOW - 200 * DAY, "create a");
    // b.rs: created by a commit that touches nothing else.
    repo.write("src/b.rs", TWO_FUNCS);
    repo.commit(
        "Grace",
        "grace@example.com",
        FIXED_NOW - 100 * DAY,
        "create b",
    );
    repo.write(
        "src/a.rs",
        "fn first() {\n    let x = 1;\n}\nfn second() {\n    let y = 22;\n}\n",
    );
    repo.commit("Alan", "alan@example.com", FIXED_NOW - 5 * DAY, "edit a");

    let spans = [LineSpan::new(1, 3), LineSpan::new(4, 6)];
    let engine =
        std::sync::Arc::new(PerFunctionBlame::open(repo.path(), opts()).expect("open engine"));
    let mut session = engine.session();
    assert_eq!(
        session.commits_resolved(),
        0,
        "a fresh session memoises nothing"
    );

    session
        .per_function(&repo.path().join("src/a.rs"), &spans)
        .expect("blame a");
    assert_eq!(
        session.commits_resolved(),
        2,
        "a.rs survives from its create and its edit"
    );

    session
        .per_function(&repo.path().join("src/b.rs"), &spans)
        .expect("blame b");
    assert_eq!(
        session.commits_resolved(),
        3,
        "b.rs's one commit must be added to a.rs's two, not replace them"
    );

    session
        .per_function(&repo.path().join("src/a.rs"), &spans)
        .expect("re-blame a");
    assert_eq!(
        session.commits_resolved(),
        3,
        "re-blaming a file must resolve nothing new"
    );
}
