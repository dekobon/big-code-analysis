use super::*;

#[test]
fn walk_err_wraps_into_walk_variant() {
    let err = walk_err("object missing");
    assert!(matches!(err, Error::Walk(msg) if msg == "object missing"));
}

#[test]
fn diff_err_wraps_into_diff_variant() {
    let err = diff_err("blob decode failed");
    assert!(matches!(err, Error::Diff(msg) if msg == "blob decode failed"));
}

#[test]
fn at_prefixed_epoch_is_parsed_by_the_fast_path() {
    // gix's date parser does not accept the bare `@<unix>` spelling, so a
    // correct parse here proves our fast-path ran — not the gix fallback.
    // This is what makes the negative test below load-bearing: if the
    // fast-path were deleted, this assertion would fail.
    assert_eq!(
        parse_timestamp("@1577836800").expect("epoch"),
        1_577_836_800
    );
}

#[test]
fn at_prefixed_non_numeric_timestamp_is_rejected() {
    // Pin the fast-path's *own* error message. A bare wildcard match
    // would also pass via the gix fallback (which rejects `@notanumber`
    // as InvalidTimestamp too), so it would not guard the fast-path.
    assert!(matches!(
        parse_timestamp("@notanumber"),
        Err(Error::InvalidTimestamp(msg)) if msg.contains("not a Unix timestamp")
    ));
}

#[test]
fn current_unix_seconds_is_a_recent_wall_clock() {
    // 2020-01-01T00:00:00Z. Any sane build host's clock is well past
    // this, exercising the happy `duration_since(UNIX_EPOCH)` path.
    const JAN_1_2020: i64 = 1_577_836_800;
    assert!(current_unix_seconds() > JAN_1_2020);
}

/// A one-file tree naming `blob`, which the ODB need not contain. The
/// file is `.rs` so the default `metrics` file-type scope keeps it: an
/// out-of-scope file is skipped before its blob is read, and a `.txt`
/// here let the enumeration test pass on the walk's failure instead.
fn tree_with(repo: &gix::Repository, blob: gix::ObjectId) -> gix::ObjectId {
    let tree = gix::objs::Tree {
        entries: vec![gix::objs::tree::Entry {
            mode: gix::objs::tree::EntryKind::Blob.into(),
            filename: "f.rs".into(),
            oid: blob,
        }],
    };
    repo.write_object(&tree).expect("write tree").detach()
}

/// Commit `tree` onto `HEAD` a day before now, inside every default window.
fn commit_tree(repo: &gix::Repository, tree: gix::ObjectId, parents: &[gix::ObjectId]) {
    let time = format!("{} +0000", current_unix_seconds() - 86_400);
    let who = gix::actor::SignatureRef {
        name: "Ada".into(),
        email: "ada@example.com".into(),
        time: time.as_str(),
    };
    repo.commit_as(who, who, "HEAD", "c", tree, parents.iter().copied())
        .expect("write commit");
}

/// An id no fixture writes, so a tree naming it dangles.
fn dangling_blob() -> gix::ObjectId {
    gix::ObjectId::from_hex(b"3333333333333333333333333333333333333333").expect("valid hex")
}

// The three tree-diff visitors each read a blob through `change.diff`.
// A tree naming an unwritten blob is the one input that makes that read
// fail on demand, and each visitor must surface it as `Error::Diff`
// rather than skip the file.

#[test]
fn a_missing_blob_at_the_target_fails_enumeration_with_a_diff_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = gix::init(dir.path()).expect("init repository");
    commit_tree(&repo, tree_with(&repo, dangling_blob()), &[]);

    let err = build(dir.path(), &Options::default()).expect_err("the target blob is missing");
    assert!(matches!(err, Error::Diff(_)), "got {err:?}");
}

#[test]
fn a_missing_blob_in_history_fails_the_walk_with_a_diff_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = gix::init(dir.path()).expect("init repository");
    let tree = tree_with(&repo, dangling_blob());
    commit_tree(&repo, tree, &[]);
    let first = repo.head_id().expect("HEAD").detach();
    // The target tree is whole, so enumeration succeeds and the failure
    // can only come from the walk diffing it against the broken parent.
    let real = repo.write_blob(b"x\n").expect("write blob").detach();
    commit_tree(&repo, tree_with(&repo, real), &[first]);

    let err = build(dir.path(), &Options::default()).expect_err("the parent blob is missing");
    assert!(matches!(err, Error::Diff(_)), "got {err:?}");
}

#[test]
fn a_missing_blob_in_the_scored_commit_is_a_diff_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = gix::init(dir.path()).expect("init repository");
    commit_tree(&repo, tree_with(&repo, dangling_blob()), &[]);

    let err = score_commit(dir.path(), "HEAD", &Options::default())
        .expect_err("the scored commit's blob is missing");
    assert!(matches!(err, Error::Diff(_)), "got {err:?}");
}
