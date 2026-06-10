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
fn perf_10k_lines_200_commits_under_30s() {
    // Acceptance criterion (issue #329): per-function VCS on a 10k-line,
    // 200-commit file completes well under 30 s. We build a 2000-function
    // file (5 lines each = 10_000 lines), make 200 commits each editing a
    // spread-out line, then time the single whole-file blame + bucketing
    // across all 2000 function spans. Only the blame call is timed — the
    // fixture's 200 `git` invocations are setup, not the measured work.
    use std::fmt::Write as _;
    const FUNCS: u32 = 2_000;
    const LINES_PER_FUNC: u32 = 5;
    const COMMITS: usize = 200;

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
        let target = (commit * 7) % values.len();
        values[target] += 1;
        repo.write("big.rs", &render(&values));
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
        // that and skips rather than flaking. Only the two documented
        // transient strings are tolerated; any other blame error still fails.
        Err(big_code_analysis::vcs::Error::Blame(reason))
            if reason.contains("iterator over a tree") || reason.contains("could not be found") =>
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
    // Guard against the perf test "passing" while blame silently produced
    // all-zero stats: the edited functions must show in-window commits.
    assert!(
        stats.iter().any(|s| s.commits_long > 0),
        "blame attributed no commits to any function — bucketing is a no-op"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "per-function blame took {elapsed:?}, over the 30s budget"
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
