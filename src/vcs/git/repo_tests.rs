use std::collections::HashSet;
use std::path::PathBuf;

use super::*;
use crate::vcs::git::history::{WalkContext, collect_events};
use crate::vcs::git::tests::{append_config, commit_tree_as, tree_of};
use crate::vcs::git::{current_unix_seconds, jit};
use crate::vcs::options::Options;

const DAY: i64 = 86_400;
const ADA: (&str, &str) = ("Ada", "ada@example.com");

/// Two files moved *and edited* in one commit, so only the similarity
/// pass can pair them: an unedited move matches by blob id before any
/// limit applies, and a fixture of those would read the same at every
/// limit. Four permutations, which `renameLimit = 1` (one squared)
/// refuses and the default of a thousand runs. The default is pinned
/// in the repository's own config: the jit and blame consumers open
/// through `gix::discover`, which reads the host's global config, and a
/// `renameLimit` there would otherwise decide the positive controls.
/// Returns the repository directory and the tip.
fn edited_move_fixture() -> (tempfile::TempDir, gix::ObjectId) {
    const A: &[u8] = b"fn a() {}\nfn a2() {}\nfn a3() {}\nfn a4() {}\n";
    const B: &[u8] = b"fn b() {}\nfn b2() {}\nfn b3() {}\nfn b4() {}\n";
    const A_EDITED: &[u8] = b"fn a() {}\nfn a2() {}\nfn a3() {}\nfn a4() {}\nfn a5() {}\n";
    const B_EDITED: &[u8] = b"fn b() {}\nfn b2() {}\nfn b3() {}\nfn b4() {}\nfn b5() {}\n";
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    let now = current_unix_seconds();
    let root = commit_tree_as(
        &scratch,
        ADA,
        now - 2 * DAY,
        tree_of(&scratch, &[("a.rs", A), ("b.rs", B)]),
        &[],
    );
    let tip = commit_tree_as(
        &scratch,
        ADA,
        now - DAY,
        tree_of(&scratch, &[("x.rs", A_EDITED), ("y.rs", B_EDITED)]),
        &[root],
    );
    append_config(
        dir.path(),
        &format!(
            "[diff]\n\trenameLimit = {}\n",
            gix::diff::Rewrites::default().limit
        ),
    );
    (dir, tip)
}

/// The rename edges the walk records at `tip`, under the repository's
/// own configuration.
fn walk_renames(dir: &std::path::Path, tip: gix::ObjectId) -> Vec<(PathBuf, PathBuf)> {
    let repo = gix::open_opts(dir, gix::open::Options::isolated()).expect("open");
    let options = Options::default();
    assert!(
        options.follow_renames,
        "the fixture is about rename tracking"
    );
    let walk = WalkContext::open(&repo, &options, current_unix_seconds()).expect("open context");
    let (events, _) = collect_events(&walk, tip, &HashSet::new()).expect("walk");
    let mut renames = events[0].renames.clone();
    renames.sort();
    renames
}

const EXPECTED_RENAMES: [(&str, &str); 2] = [("a.rs", "x.rs"), ("b.rs", "y.rs")];

fn expected_renames() -> Vec<(PathBuf, PathBuf)> {
    EXPECTED_RENAMES
        .iter()
        .map(|(from, to)| (PathBuf::from(from), PathBuf::from(to)))
        .collect()
}

/// The positive control every limited row below depends on: at gix's
/// default limit the edited moves are paired by similarity, so a row
/// that then sees no edges under a lower limit is reading the limit and
/// not a fixture that never qualified. The fixture spells the default
/// out; that an *absent* key resolves to it is
/// [`an_absent_rename_limit_is_gix_s_default`]'s.
#[test]
fn the_edited_moves_are_paired_at_the_default_limit() {
    let (dir, tip) = edited_move_fixture();
    assert_eq!(
        rewrites(
            &gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open"),
            true
        ),
        Some(gix::diff::Rewrites::default()),
        "the pinned limit is gix's default: the same `Rewrites`, exactly"
    );
    assert_eq!(walk_renames(dir.path(), tip), expected_renames());
}

/// With no `diff.renameLimit` anywhere the open can see, the read is
/// gix's own defaults, exactly. Isolated, so the host's global config
/// cannot supply one.
#[test]
fn an_absent_rename_limit_is_gix_s_default() {
    let dir = tempfile::tempdir().expect("temp dir");
    gix::init(dir.path()).expect("init repository");
    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");
    assert_eq!(rewrites(&repo, true), Some(gix::diff::Rewrites::default()));
}

/// `diff.renameLimit` is read from the repository and bounds the
/// similarity pass the way git bounds it: at `1`, four permutations
/// exceed one squared, the pass does not run, and the edited moves
/// record as deletions and additions.
#[test]
fn the_history_walk_honours_the_repository_rename_limit() {
    let (dir, tip) = edited_move_fixture();
    append_config(dir.path(), "[diff]\n\trenameLimit = 1\n");
    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");
    assert_eq!(
        rewrites(&repo, true).map(|rewrites| rewrites.limit),
        Some(1),
        "the configured limit must reach the tracker"
    );
    assert_eq!(
        walk_renames(dir.path(), tip),
        [],
        "four permutations exceed a limit of one squared, so no similarity pass runs"
    );
}

/// The limit is squared, as git squares it and as gix 0.90 compares it
/// (0.89 compared the raw product): at `2`, four permutations do not
/// exceed two squared and the pass runs, where the unsquared compare
/// would have refused them. The one row that tells the two apart on
/// this fixture.
#[test]
fn the_rename_limit_is_squared_as_git_squares_it() {
    let (dir, tip) = edited_move_fixture();
    append_config(dir.path(), "[diff]\n\trenameLimit = 2\n");
    assert_eq!(
        walk_renames(dir.path(), tip),
        expected_renames(),
        "four permutations are within two squared, so the pass runs"
    );
}

/// A trusted `0` is unlimited, not disabled: through a consumer, the
/// pass runs and the edited moves are paired. The only row where the
/// two readings of `0` differ.
#[test]
fn a_zero_rename_limit_still_pairs_through_the_walk() {
    let (dir, tip) = edited_move_fixture();
    append_config(dir.path(), "[diff]\n\trenameLimit = 0\n");
    assert_eq!(walk_renames(dir.path(), tip), expected_renames());
}

/// `vcs commit` scores one commit through the same read: the touched
/// files carry a parent-side path for a rename and none for an
/// addition, so the limit decides whether `x.rs` is a move of `a.rs` or
/// a new file.
#[test]
fn the_commit_score_honours_the_repository_rename_limit() {
    let (dir, tip) = edited_move_fixture();
    let spec = tip.to_string();
    let paired = jit::score_commit(dir.path(), &spec, &Options::default()).expect("score");
    assert_eq!(
        paired.features.history.new_files, 0,
        "at the default limit both edited moves are renames, so nothing is new"
    );
    append_config(dir.path(), "[diff]\n\trenameLimit = 1\n");
    let unpaired = jit::score_commit(dir.path(), &spec, &Options::default()).expect("score");
    assert_eq!(
        unpaired.features.history.new_files, 2,
        "at a limit of one the similarity pass does not run, so both are additions"
    );
}

/// Per-function blame follows the same limit. Blame credits each
/// surviving line to the commit that last touched it, so lines 1-4 of
/// `x.rs` — unchanged across the move — blame to the root commit when
/// the rename is followed and to the tip when it is not. One commit
/// either way; which one shows in the recent window, which here is
/// set to admit the tip (a day old) and not the root (two days).
#[test]
fn per_function_blame_honours_the_repository_rename_limit() {
    use crate::vcs::git::blame::{LineSpan, PerFunctionBlame};
    let (dir, _tip) = edited_move_fixture();
    let git_dir = dir.path();
    // Blame needs a working tree holding the file; the fixture committed
    // in-process, so write the tip's content out.
    std::fs::write(
        git_dir.join("x.rs"),
        b"fn a() {}\nfn a2() {}\nfn a3() {}\nfn a4() {}\nfn a5() {}\n",
    )
    .expect("write x.rs");
    let span = [LineSpan::new(1, 4)];
    let options = Options {
        recent_window_secs: DAY + DAY / 2,
        ..Options::default()
    };
    let blame = |options: Options| {
        let stats = PerFunctionBlame::open(git_dir, options)
            .expect("open blame engine")
            .per_function(&git_dir.join("x.rs"), &span)
            .expect("blame");
        (stats[0].commits_long, stats[0].commits_recent)
    };
    assert_eq!(
        blame(options.clone()),
        (1, 0),
        "followed through the rename, lines 1-4 are the root commit's: inside \
         the long window, outside the recent one"
    );
    append_config(git_dir, "[diff]\n\trenameLimit = 1\n");
    assert_eq!(
        blame(options),
        (1, 1),
        "without the pass `x.rs` is new at the tip, whose commit is recent"
    );
}

/// The values gix and git document, and the one bca decides. `0` is
/// unlimited in both, so it reaches the tracker as `0`; a value that is
/// not an unsigned integer falls back to gix's default rather than
/// refusing the repository, since the limit is a performance knob.
#[test]
fn zero_is_unlimited_and_a_malformed_limit_is_the_default() {
    for (value, expected) in [
        ("0", 0),
        ("250", 250),
        ("-1", gix::diff::Rewrites::default().limit),
        ("lots", gix::diff::Rewrites::default().limit),
    ] {
        let dir = tempfile::tempdir().expect("temp dir");
        gix::init(dir.path()).expect("init repository");
        append_config(dir.path(), &format!("[diff]\n\trenameLimit = {value}\n"));
        let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");
        assert_eq!(
            rewrites(&repo, true).map(|rewrites| rewrites.limit),
            Some(expected),
            "renameLimit = {value}"
        );
    }
}

/// With renames off the result is `None` whatever the configuration
/// says: the switch is the option's, not the configuration's.
#[test]
fn renames_off_resolves_to_none_whatever_the_limit() {
    let dir = tempfile::tempdir().expect("temp dir");
    gix::init(dir.path()).expect("init repository");
    append_config(dir.path(), "[diff]\n\trenameLimit = 1\n");
    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");
    assert_eq!(rewrites(&repo, false), None);
}

/// The read goes through the trusted-section filter gix installs under
/// reduced trust, so a repository-local `renameLimit` from a repository
/// owned by someone else is ignored and gix's default holds. The fixture
/// is opened with `Trust::Reduced` explicitly — a repository the test
/// itself created is fully trusted — and the row first pins that the
/// local section really is reduced under that open, so the filter is
/// seen to reject something rather than pass over a trusted value.
/// Isolated, so the host's `safe.directory` cannot promote the section
/// and no global `[diff]` section is loaded at all: the `Local` find
/// below is a guard for a future un-isolated open, not what excludes
/// the host's config today.
#[test]
fn a_reduced_trust_repository_s_rename_limit_is_ignored() {
    let dir = tempfile::tempdir().expect("temp dir");
    gix::init(dir.path()).expect("init repository");
    append_config(dir.path(), "[diff]\n\trenameLimit = 0\n");
    let repo = gix::open_opts(
        dir.path(),
        gix::open::Options::isolated().with(gix::sec::Trust::Reduced),
    )
    .expect("open");
    let snapshot = repo.config_snapshot();
    let local = snapshot
        .plumbing()
        .sections_by_name("diff")
        .expect("the diff section")
        .find(|section| section.meta().source == gix::config::Source::Local)
        .expect("the repository-local diff section");
    assert_eq!(
        local.meta().trust,
        gix::sec::Trust::Reduced,
        "the local section must carry the reduced trust the open assigned"
    );
    assert!(
        !gix::config::section::is_trusted(local.meta()),
        "and the filter must reject it"
    );
    assert_eq!(
        snapshot
            .plumbing()
            .integer(gix::config::tree::Diff::RENAME_LIMIT)
            .expect("an integer"),
        Some(0),
        "unfiltered, the untrusted value is visible — the read gix's own \
         `diff_renames` would make"
    );
    assert_eq!(
        rewrites(&repo, true).map(|rewrites| rewrites.limit),
        Some(gix::diff::Rewrites::default().limit),
        "filtered, it is not: an untrusted `0` cannot uncap the similarity pass"
    );
}
