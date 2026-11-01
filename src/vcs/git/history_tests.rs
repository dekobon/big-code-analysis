use std::collections::HashSet;

use super::*;
use crate::vcs::git::{current_unix_seconds, repo};

const DAY: i64 = 86_400;

/// Folds Grace's identity into Ada's.
const FOLD_GRACE_INTO_ADA: &[u8] = b"Ada <ada@example.com> Grace <grace@example.com>\n";
/// The inverse mapping: folds Ada's identity into Grace's.
const FOLD_ADA_INTO_GRACE: &[u8] = b"Grace <grace@example.com> Ada <ada@example.com>\n";

/// Commit a one-file tree holding `contents` as `name <email>` at `time`.
fn commit_file(
    repo: &gix::Repository,
    (name, email): (&str, &str),
    contents: &[u8],
    parent: Option<gix::ObjectId>,
    time: i64,
) -> gix::ObjectId {
    let blob = repo.write_blob(contents).expect("write blob").detach();
    let tree = gix::objs::Tree {
        entries: vec![gix::objs::tree::Entry {
            mode: gix::objs::tree::EntryKind::Blob.into(),
            filename: "a.rs".into(),
            oid: blob,
        }],
    };
    let tree = repo.write_object(&tree).expect("write tree").detach();
    let time = format!("{time} +0000");
    let who = gix::actor::SignatureRef {
        name: name.into(),
        email: email.into(),
        time: time.as_str(),
    };
    repo.commit_as(who, who, "HEAD", "c", tree, parent)
        .expect("write commit")
        .detach()
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
        now,
    };
    assert_ne!(
        walk.mailmap_digest(),
        WalkContext::open(&repo, &options, now).mailmap_digest(),
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
