//! The failure arms of [`WalkContext::open`] — the index load, the
//! attribute-file capture and the root `.gitattributes` blob lookup —
//! each pinned against gix's own platform build refusing the same input.
//! Kept apart from `history_tests.rs`, which holds the digest and parity
//! rows, so the two files can be edited independently.

use super::*;
use crate::vcs::git::current_unix_seconds;

/// Open `dir` isolated from the host, as the digest tests do, and open a
/// walk context on it.
fn open_context(dir: &std::path::Path) -> Result<u64, Error> {
    let repo = gix::open_opts(dir, gix::open::Options::isolated()).expect("open");
    let options = Options::default();
    WalkContext::open(&repo, &options, current_unix_seconds())
        .map(|walk| walk.diff_attributes_digest())
}

/// `AttributeSources::read` fails on an attribute file that exists but
/// cannot be read — the `Some(Err(err)) if !gix_treats_as_missing(&err)`
/// arm — exactly where gix's own platform build fails on the same
/// input. The capture is the walk's only reader of these files, so a
/// read it folded into "absent" would let the walk resolve under no
/// rules where `git` refuses to run, and stamp the entry with a digest
/// over an absence that every later run matches. `PermissionDenied` is
/// also the one kind `gix_treats_as_missing` folds into "absent" under
/// `cfg!(windows)`, so this row pins that Unix keeps it an error.
///
/// Verified by revert: widening the arm to `Some(Err(_)) | None => None`
/// opens the context, and the `assert_ne!` on the digest shows the file
/// is the input in question. Unix-only: it relies on POSIX mode bits,
/// and is skipped where the process reads a mode-000 file anyway (root).
#[cfg(unix)]
#[test]
fn an_unreadable_attributes_file_fails_the_open_as_it_fails_gix() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("temp dir");
    gix::init(dir.path()).expect("init repository");
    let user_home = tempfile::tempdir().expect("temp dir");
    let user_file = user_home.path().join("attributes");
    std::fs::write(&user_file, b"*.rs diff=user\n").expect("write user attributes");
    let config_path = user_file
        .to_str()
        .expect("temp path is valid UTF-8")
        .replace('\\', "/");
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join(".git").join("config"))
        .expect("open config")
        .write_all(format!("[core]\n\tattributesFile = {config_path}\n").as_bytes())
        .expect("append config");
    let readable = open_context(dir.path()).expect("a readable user file opens");

    std::fs::set_permissions(&user_file, std::fs::Permissions::from_mode(0o000))
        .expect("chmod user attributes");
    // A `return` here would be a passing test with no assertion run
    // (`.claude/rules/testing.md`, "an empty test is a passing test"), so
    // a privileged runner fails loudly instead of reading as covered.
    assert!(
        std::fs::read(&user_file).is_err(),
        "this test needs an unprivileged runner: the process read a mode-000 file"
    );

    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");
    assert!(
        repo.diff_resource_cache_for_tree_diff().is_err(),
        "gix's own platform build refuses the unreadable file"
    );
    let err = open_context(dir.path()).expect_err("the capture must refuse it too");
    assert!(matches!(err, Error::Diff(_)), "got {err:?}");
    assert!(
        err.to_string().contains("ermission denied"),
        "the error names the read failure, not another input: {err}"
    );

    std::fs::set_permissions(&user_file, std::fs::Permissions::from_mode(0o644))
        .expect("chmod user attributes back");
    let unbound = tempfile::tempdir().expect("temp dir");
    gix::init(unbound.path()).expect("init repository");
    assert_ne!(
        readable,
        open_context(unbound.path()).expect("a repository with no user file opens"),
        "the user file is read when readable, so the refusal above is about a file \
         the walk would otherwise resolve under"
    );
}

/// `take_root_attributes` reads the root `.gitattributes` blob the index
/// names (`repo.find_blob(id)?`), and an index naming a blob the object
/// database lacks fails the open with `Error::Diff` — the failure gix's
/// first directory push raises on the same index — rather than opening
/// a context that resolves as if the file were absent and stamps the
/// entry with the missing id. The control index names a blob that was
/// written, so the failure is the missing object and not the fixture.
///
/// Verified by revert: `find_blob(id).ok()` with a `None` root opens
/// the context on the dangling index.
#[test]
fn a_root_gitattributes_blob_the_object_database_lacks_fails_the_open() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = gix::init(dir.path()).expect("init repository");
    let source = repo
        .write_blob(b"fn a() {}\n")
        .expect("write blob")
        .detach();
    let stage = |attributes: gix::ObjectId| {
        let mut entries = vec![
            gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: ".gitattributes".into(),
                oid: attributes,
            },
            gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: "a.rs".into(),
                oid: source,
            },
        ];
        entries.sort();
        let tree = repo
            .write_object(&gix::objs::Tree { entries })
            .expect("write tree")
            .detach();
        repo.index_from_tree(&tree)
            .expect("index from tree")
            .write(gix::index::write::Options::default())
            .expect("write index");
    };

    let written = repo
        .write_blob(b"*.rs diff=rs\n")
        .expect("write blob")
        .detach();
    stage(written);
    open_context(dir.path()).expect("an index naming a written blob opens");

    let dangling =
        gix::ObjectId::from_hex(b"3333333333333333333333333333333333333333").expect("valid hex");
    assert!(
        repo.try_find_object(dangling).expect("lookup").is_none(),
        "the fixture id must not be in the object database"
    );
    stage(dangling);
    let err = open_context(dir.path()).expect_err("the root blob is missing");
    assert!(matches!(err, Error::Diff(_)), "got {err:?}");
}

/// A `core.attributesFile` gix cannot interpolate — `~user/…` for a user
/// the host has no passwd entry for (Unix), or any `~user/` form where
/// user interpolation is unsupported (Windows) — fails the open as
/// `Error::Diff`, the `?` on `trusted_path` in `AttributeSources::read`,
/// as gix's own platform build fails on the same key. The value is
/// read however the repository was opened: interpolation is not under
/// the open's leniency, so a context that folded the failure into "no
/// user file" would resolve under rules gix refuses to resolve under.
///
/// Verified by revert: `trusted_path(…).ok().flatten()` opens the
/// context.
#[test]
fn an_uninterpolatable_core_attributes_file_fails_the_open_as_it_fails_gix() {
    use std::io::Write;

    let dir = tempfile::tempdir().expect("temp dir");
    gix::init(dir.path()).expect("init repository");
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join(".git").join("config"))
        .expect("open config")
        .write_all(b"[core]\n\tattributesFile = ~bca-no-such-user-1760/attributes\n")
        .expect("append config");
    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");
    assert!(
        repo.diff_resource_cache_for_tree_diff().is_err(),
        "gix's own platform build refuses the key"
    );

    let err = open_context(dir.path()).expect_err("the capture must refuse it too");
    assert!(matches!(err, Error::Diff(_)), "got {err:?}");
}

/// A `.git/index` gix cannot decode fails the open as `Error::Diff` —
/// the `?` on `index_or_load_from_head_or_empty` — rather than opening
/// a context on an empty or `HEAD`-built index: the index is where the
/// platform's stack reads every `.gitattributes` blob from, so a
/// fallback would resolve a staged binding as unset and stamp the entry
/// with a digest no later run with the index repaired matches. The file
/// is overwritten after a successful open on the same repository, so
/// the failure is the corruption and not the fixture.
///
/// Verified by revert: `.unwrap_or_else(|_| …from_state(State::new(…)))`
/// opens the context.
#[test]
fn an_undecodable_index_fails_the_open_with_a_diff_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let repo = gix::init(dir.path()).expect("init repository");
    let blob = repo
        .write_blob(b"fn a() {}\n")
        .expect("write blob")
        .detach();
    let tree = gix::objs::Tree {
        entries: vec![gix::objs::tree::Entry {
            mode: gix::objs::tree::EntryKind::Blob.into(),
            filename: "a.rs".into(),
            oid: blob,
        }],
    };
    let tree = repo.write_object(&tree).expect("write tree").detach();
    let index_path = dir.path().join(".git").join("index");
    repo.index_from_tree(&tree)
        .expect("index from tree")
        .write(gix::index::write::Options::default())
        .expect("write index");
    open_context(dir.path()).expect("a decodable index opens");

    std::fs::write(&index_path, b"not an index").expect("corrupt the index");
    let err = open_context(dir.path()).expect_err("the index cannot be decoded");
    assert!(matches!(err, Error::Diff(_)), "got {err:?}");
}
