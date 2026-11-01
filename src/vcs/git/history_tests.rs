use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use super::*;
use crate::vcs::git::tests::{commit_tree_as, tree_with};
use crate::vcs::git::{current_unix_seconds, repo};

const DAY: i64 = 86_400;

/// Folds Grace's identity into Ada's.
const FOLD_GRACE_INTO_ADA: &[u8] = b"Ada <ada@example.com> Grace <grace@example.com>\n";
/// The inverse mapping: folds Ada's identity into Grace's.
const FOLD_ADA_INTO_GRACE: &[u8] = b"Grace <grace@example.com> Ada <ada@example.com>\n";

/// Commit a one-file tree holding `contents` as `who` at `time`.
fn commit_file(
    repo: &gix::Repository,
    who: (&str, &str),
    contents: &[u8],
    parent: Option<gix::ObjectId>,
    time: i64,
) -> gix::ObjectId {
    let blob = repo.write_blob(contents).expect("write blob").detach();
    commit_tree_as(repo, who, time, tree_with(repo, blob), parent.as_slice())
}

/// Regression for #1409. The walk resolves identities through the snapshot
/// its context holds, never through a read of its own. The other half of
/// the pairing needs no test: `mailmap_digest` is a method on the context
/// and the snapshot is private to it, so a stamp can only come from a
/// context's snapshot; that `build_cached` opens one context and hands
/// the same one to the walk is visible in its body (one `WalkContext::open`
/// call) rather than enforced by the type. The on-disk `.mailmap` is written
/// *after* the snapshot is captured and maps the identities the other way,
/// so each of the three possible resolutions records a different digest:
/// the captured snapshot gives Ada's, a re-read inside the walk gives
/// Grace's, and no mailmap at all leaves the two distinct.
#[test]
fn the_walk_resolves_identities_through_the_captured_snapshot() {
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    let now = current_unix_seconds();
    let first = commit_file(
        &scratch,
        ("Ada", "ada@example.com"),
        b"fn a() {}\n",
        None,
        now - 2 * DAY,
    );
    let tip = commit_file(
        &scratch,
        ("Grace", "grace@example.com"),
        b"fn a() { work(); }\n",
        Some(first),
        now - DAY,
    );

    let captured = gix::mailmap::Snapshot::from_bytes(FOLD_GRACE_INTO_ADA);
    std::fs::write(dir.path().join(".mailmap"), FOLD_ADA_INTO_GRACE).expect("write .mailmap");

    // Opened the way production opens it, so `open_mailmap` reads the
    // work-tree file written above: the view a second read would see.
    let repo = repo::open(dir.path()).expect("open").repo;
    let options = Options::default();
    let walk = WalkContext {
        repo: &repo,
        options: &options,
        mailmap: captured,
        diff: repo
            .diff_resource_cache_for_tree_diff()
            .expect("diff platform"),
        shallow: None,
        now,
    };
    assert_ne!(
        walk.mailmap_digest(),
        WalkContext::open(&repo, &options, now)
            .expect("open")
            .mailmap_digest(),
        "the repository's own view must differ from the captured snapshot, \
         or a re-read inside the walk is unobservable"
    );
    let (events, stopped) = collect_events(&walk, tip, &HashSet::new()).expect("walk");
    assert_eq!(stopped, None, "no splice point with an empty stop set");

    assert_eq!(events.len(), 2, "both commits are inside the long window");
    let ada = AuthorId::new(b"Ada", b"ada@example.com").hashed();
    for event in &events {
        assert_eq!(
            event.authors,
            [ada.as_str()],
            "every commit must resolve to Ada through the captured snapshot; \
             Grace's digest means the walk re-read the on-disk mailmap"
        );
    }
}

/// Append `section` to the repository's own config file and re-open, so
/// the next context is built from the changed configuration the way a
/// `git config` edit between two `bca vcs` runs would be.
///
/// The open is isolated from the host's global and system config, which
/// production's `repo::open` merges in under the repository's own: a
/// `[diff "rs"]` or `core.bigFileThreshold` in a contributor's
/// `~/.gitconfig` would already count in every digest and turn the step
/// that adds it locally into a no-op — and a global `textconv` cannot be
/// unset by a local key. The repository-local file is still read.
fn digest_after_config(dir: &Path, section: &str) -> u64 {
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join(".git").join("config"))
        .expect("open config")
        .write_all(section.as_bytes())
        .expect("append config");
    let repo = gix::open_opts(dir, gix::open::Options::isolated()).expect("open");
    let options = Options::default();
    WalkContext::open(&repo, &options, current_unix_seconds())
        .expect("open context")
        .diff_config_digest()
}

/// Regression for #1425: every diff setting the platform reads moves the
/// digest, and what it does not read — a driver's `command`, and a
/// section that configures no behaviour at all — leaves it alone. Each
/// step appends one more setting, so each digest is compared with its
/// predecessor and the comparison isolates that setting. The driver is
/// made an input by its `textconv` *before* its other fields are tested,
/// because the step that first makes a section count moves the digest by
/// the driver's name alone and would mask a dropped field term (seen by
/// perturbation); `binary` is then stepped under that textconv, where gix
/// today reads `true` as `false` but the digest hashes it as written, and
/// again on a second driver without one. The base step sets
/// `diff.algorithm` so the first comparison steps between two values the
/// fixture wrote rather than from gix's default; the host's own config
/// never enters, because `digest_after_config` opens the repository
/// isolated from it.
///
/// There is no diff twin of
/// `the_walk_resolves_identities_through_the_captured_snapshot`: a
/// platform the walk opened for itself would be configured from the same
/// in-memory config the context's was (gix resolves config once at open),
/// so that pairing is unobservable and the clone in `collect_events` is
/// bookkeeping rather than a guard.
#[test]
fn the_diff_config_digest_tracks_what_the_platform_reads() {
    let dir = tempfile::tempdir().expect("temp dir");
    gix::init(dir.path()).expect("init repository");

    let base = digest_after_config(dir.path(), "[diff]\n\talgorithm = myers\n");
    let algorithm = digest_after_config(dir.path(), "[diff]\n\talgorithm = histogram\n");
    assert_ne!(base, algorithm, "`diff.algorithm` selects the edit script");

    let inert = digest_after_config(dir.path(), "[diff \"rs\"]\n\tcommand = /bin/true\n");
    assert_eq!(
        algorithm, inert,
        "a section that sets no algorithm, binary or textconv behaves as no \
         driver would, and an external diff command never runs on this path"
    );

    let driver = digest_after_config(dir.path(), "[diff \"rs\"]\n\ttextconv = cat\n");
    assert_ne!(
        inert, driver,
        "a textconv makes the section a driver the digest reads, by name, and \
         with `binary` unset turns off the NUL sniff"
    );

    let driver_algorithm =
        digest_after_config(dir.path(), "[diff \"rs\"]\n\talgorithm = minimal\n");
    assert_ne!(
        driver, driver_algorithm,
        "a driver's algorithm overrides the platform's for the paths bound to it"
    );

    let driver_text = digest_after_config(dir.path(), "[diff \"rs\"]\n\tbinary = false\n");
    assert_ne!(
        driver_algorithm, driver_text,
        "`binary = false` under a textconv still moves: the big-file threshold \
         no longer applies to the driver's paths"
    );

    let driver_binary = digest_after_config(dir.path(), "[diff \"rs\"]\n\tbinary = true\n");
    assert_ne!(
        driver_text, driver_binary,
        "`binary = true` under a textconv is hashed as written: gix reads it as \
         text today, but that fold is gix's to change, and the raw field can \
         only cost a cold walk, never serve a stale hit"
    );

    // A second driver, without a textconv. It is created with an
    // algorithm first, so each `binary` step compares against a driver
    // the digest already counts.
    let second = digest_after_config(dir.path(), "[diff \"py\"]\n\talgorithm = myers\n");
    assert_ne!(
        driver_binary, second,
        "a second configured driver counts by name"
    );
    let second_text = digest_after_config(dir.path(), "[diff \"py\"]\n\tbinary = false\n");
    assert_ne!(
        second, second_text,
        "`binary = false` turns off the big-file threshold and the NUL sniff"
    );
    let second_binary = digest_after_config(dir.path(), "[diff \"py\"]\n\tbinary = true\n");
    assert_ne!(
        second_text, second_binary,
        "`binary = true` decides that the driver's paths are never diffed"
    );

    let threshold = digest_after_config(dir.path(), "[core]\n\tbigFileThreshold = 1024\n");
    assert_ne!(
        second_binary, threshold,
        "`core.bigFileThreshold` decides whether a blob is diffed at all"
    );
}

/// Open `dir` isolated from the host, as `digest_after_config` does, and
/// digest the shallow boundary a fresh context reads.
fn shallow_digest(dir: &Path) -> u64 {
    let repo = gix::open_opts(dir, gix::open::Options::isolated()).expect("open");
    let options = Options::default();
    WalkContext::open(&repo, &options, current_unix_seconds())
        .expect("open context")
        .shallow_boundary_digest()
}

/// The digest is over the *set* of boundary commits: it moves when the
/// boundary appears, moves again when a deepen replaces it, and is the
/// same for the same commits written in another order. The file is
/// written by hand because what it lists is the whole input — the ids
/// need not name objects the repository holds.
#[test]
fn the_shallow_boundary_digest_tracks_the_boundary_set() {
    const FIRST: &str = "1111111111111111111111111111111111111111\n";
    const SECOND: &str = "2222222222222222222222222222222222222222\n";
    let dir = tempfile::tempdir().expect("temp dir");
    gix::init(dir.path()).expect("init repository");
    let shallow_file = dir.path().join(".git").join("shallow");

    let full = shallow_digest(dir.path());
    std::fs::write(&shallow_file, FIRST).expect("write shallow");
    let one = shallow_digest(dir.path());
    assert_ne!(full, one, "a boundary appearing moves the digest");

    std::fs::write(&shallow_file, SECOND).expect("rewrite shallow");
    let deepened = shallow_digest(dir.path());
    assert_ne!(
        one, deepened,
        "a deepen replaces the boundary commits and must move the digest, \
         though the clone is as shallow as before"
    );

    std::fs::write(&shallow_file, format!("{FIRST}{SECOND}")).expect("rewrite shallow");
    let two = shallow_digest(dir.path());
    std::fs::write(&shallow_file, format!("{SECOND}{FIRST}")).expect("rewrite shallow");
    // gix-shallow sorts the list on read, so this row holds with or
    // without `shallow_boundary_digest`'s own sort: removing that sort
    // fails nothing. The sort stays as the digest's own guarantee, not
    // a property this row covers.
    assert_eq!(
        two,
        shallow_digest(dir.path()),
        "the same boundary written in another order is the same set"
    );
    assert_ne!(deepened, two, "a wider boundary is a different set");

    std::fs::remove_file(&shallow_file).expect("remove shallow");
    assert_eq!(
        full,
        shallow_digest(dir.path()),
        "`--unshallow` returns to the full-history digest"
    );
}
