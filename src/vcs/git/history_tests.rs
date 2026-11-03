use std::collections::HashSet;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

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
    let index = repo.index_or_load_from_head_or_empty().expect("index");
    let sources = AttributeSources::read(
        &repo,
        &mut gix::path::env::var,
        gix::open::permissions::Attributes::all(),
    )
    .expect("attribute files");
    let mappings = id_mappings(&index, false);
    let diff_attributes = diff_attributes_digest(&mappings, &sources, false);
    let diff = tree_diff_platform(&repo, sources, false, mappings).expect("diff platform");
    let walk = WalkContext {
        repo: &repo,
        options: &options,
        mailmap: captured,
        diff,
        diff_attributes,
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

/// A tree of `files` as `(name, contents)`, every one a regular blob.
fn tree_of(repo: &gix::Repository, files: &[(&str, &[u8])]) -> gix::ObjectId {
    let mut entries: Vec<gix::objs::tree::Entry> = files
        .iter()
        .map(|(name, contents)| gix::objs::tree::Entry {
            mode: gix::objs::tree::EntryKind::Blob.into(),
            filename: (*name).into(),
            oid: repo.write_blob(contents).expect("write blob").detach(),
        })
        .collect();
    entries.sort();
    repo.write_object(&gix::objs::Tree { entries })
        .expect("write tree")
        .detach()
}

/// Regression for the per-commit clears in `collect_events` (#1761).
///
/// Without worktree roots gix keys its resource cache by blob id alone
/// (`CacheKey::use_id`), not by path, so a conversion cached for a blob
/// at one path is served for the same blob at any other — including a
/// path whose `diff` attribute binding differs. The walk fills two
/// platforms per commit and clears both before the next, so a stale
/// conversion can only cross commits if a clear is dropped; this fixture
/// makes each drop observable on its own.
///
/// The tip adds `x.bin`, bound `-diff` by the committed `.gitattributes`
/// (the platform reads it from the index gix builds from `HEAD`'s tree),
/// and deletes `old.rs`, so rename tracking runs a similarity check that
/// caches `x.bin`'s blob as *binary* in the rewrite platform; the line
/// counts cache the same verdict in the other. The parent then renames
/// `b.rs` to `a.rs` with that same blob as the new content. Walked
/// newest-first with both clears, `a.rs` is text there: the rename is
/// found and its churn recorded. Verified by revert, one clear at a
/// time: without the rewrite clear the similarity check reads the stale
/// binary verdict and the rename is lost (`renames` empty, `b.rs` a
/// deletion); without the line-count clear the rename survives but
/// `a.rs` is skipped as binary and drops out of `touched`.
#[test]
fn the_per_commit_clears_keep_a_blob_s_attribute_binding_from_crossing_commits() {
    const ATTRIBUTES: &[u8] = b"*.bin -diff\n";
    const BEFORE: &[u8] = b"fn a() {}\nfn b() {}\nfn c() {}\n";
    const AFTER: &[u8] = b"fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\n";
    const OTHER: &[u8] = b"fn old() {}\n";
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    let now = current_unix_seconds();
    let ada = ("Ada", "ada@example.com");
    let root = commit_tree_as(
        &scratch,
        ada,
        now - 3 * DAY,
        tree_of(
            &scratch,
            &[
                (".gitattributes", ATTRIBUTES),
                ("b.rs", BEFORE),
                ("old.rs", OTHER),
            ],
        ),
        &[],
    );
    let renamed = commit_tree_as(
        &scratch,
        ada,
        now - 2 * DAY,
        tree_of(
            &scratch,
            &[
                (".gitattributes", ATTRIBUTES),
                ("a.rs", AFTER),
                ("old.rs", OTHER),
            ],
        ),
        &[root],
    );
    let tip = commit_tree_as(
        &scratch,
        ada,
        now - DAY,
        tree_of(
            &scratch,
            &[
                (".gitattributes", ATTRIBUTES),
                ("a.rs", AFTER),
                ("x.bin", AFTER),
            ],
        ),
        &[renamed],
    );

    // Isolated from the host's global attributes file, which could bind
    // `*.rs` or `*.bin` itself.
    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");
    let options = Options::default();
    assert!(
        options.follow_renames,
        "the fixture is about rename tracking"
    );
    let walk = WalkContext::open(&repo, &options, now).expect("open context");
    let (events, _) = collect_events(&walk, tip, &HashSet::new()).expect("walk");
    assert_eq!(events.len(), 3, "every commit is inside the long window");

    let at_tip = &events[0];
    assert_eq!(at_tip.oid, tip.to_string());
    assert_eq!(
        at_tip.touched,
        [(PathBuf::from("old.rs"), 1)],
        "`x.bin` is bound `-diff` and so records no churn; only the deletion does"
    );

    let at_rename = &events[1];
    assert_eq!(at_rename.oid, renamed.to_string());
    assert_eq!(
        at_rename.renames,
        [(PathBuf::from("b.rs"), PathBuf::from("a.rs"))],
        "the similarity check must read `a.rs`'s blob as text, not the binary \
         verdict the tip cached for the same blob at `x.bin`"
    );
    assert_eq!(
        at_rename.touched,
        [(PathBuf::from("a.rs"), 1)],
        "one inserted line: the line counts must read the blob as text too"
    );
}

/// The platform `WalkContext::open` builds, from the inputs it reads for
/// itself, over a repository opened isolated from the host: the index
/// (bare or not, loaded once) and the attribute files, each read once
/// with `env` under the isolated attribute permissions — the ones the
/// `Options::isolated()` open gix's own build is compared against
/// applies.
fn hand_built_platform(
    repo: &gix::Repository,
    env: &mut dyn FnMut(&str) -> Option<OsString>,
) -> gix::diff::blob::Platform {
    let index = repo.index_or_load_from_head_or_empty().expect("index");
    let ignore_case = repo.filesystem_options().expect("fs options").ignore_case;
    let sources = AttributeSources::read(repo, env, gix::open::permissions::Attributes::isolated())
        .expect("attribute files");
    tree_diff_platform(repo, sources, ignore_case, id_mappings(&index, ignore_case))
        .expect("hand-built platform")
}

/// `WalkContext::open` builds its platform from an index it loads once
/// (#1761 keeps the index for #1760), where gix's
/// `diff_resource_cache_for_tree_diff` loads one of its own. The two must
/// diff under the same configuration: same mode, and the same algorithm,
/// drivers and threshold the cache digest projects. A sanity check on an
/// equivalence rather than a discriminating test — swapping one
/// construction for the other passes it by definition — so the config
/// sets every field the digest reads away from its default, which is
/// what makes a construction that read none of them fail here. Only
/// the configuration half is compared: the attribute half — which
/// files the stack loads under the open's permissions, and how it
/// resolves through them — is
/// `the_hand_built_stack_resolves_diff_as_gix_s_does` and
/// `the_hand_built_stack_skips_the_files_an_isolated_open_withholds`.
#[test]
fn the_hand_built_platform_matches_gix_s() {
    let dir = tempfile::tempdir().expect("temp dir");
    gix::init(dir.path()).expect("init repository");
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.path().join(".git").join("config"))
        .expect("open config")
        .write_all(
            b"[diff]\n\talgorithm = histogram\n\
              [diff \"rs\"]\n\talgorithm = minimal\n\tbinary = false\n\ttextconv = cat\n\
              [core]\n\tbigFileThreshold = 1024\n",
        )
        .expect("append config");
    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");

    let by_hand = hand_built_platform(&repo, &mut gix::path::env::var);
    let by_gix = repo
        .diff_resource_cache_for_tree_diff()
        .expect("gix platform");

    assert_eq!(by_hand.filter_mode, gix::diff::blob::pipeline::Mode::ToGit);
    assert_eq!(by_hand.filter_mode, by_gix.filter_mode);
    assert_eq!(
        diff_config_digest(&by_hand),
        diff_config_digest(&by_gix),
        "the hand-built platform must carry gix's algorithm, drivers and threshold"
    );
    let unconfigured = tempfile::tempdir().expect("temp dir");
    gix::init(unconfigured.path()).expect("init repository");
    let default_repo =
        gix::open_opts(unconfigured.path(), gix::open::Options::isolated()).expect("open");
    assert_ne!(
        diff_config_digest(&by_hand),
        diff_config_digest(
            &default_repo
                .diff_resource_cache_for_tree_diff()
                .expect("default platform")
        ),
        "the config written above must move the digest, or the equality \
         holds for a platform that read no configuration at all"
    );
}

/// Resolve the `diff` attribute of each of `paths` through `platform`'s
/// attribute stack, as the blob pipeline does, rendered as git's
/// `check-attr` spells it: `set`, `unset`, `unspecified` or the value.
fn diff_states(
    repo: &gix::Repository,
    platform: &mut gix::diff::blob::Platform,
    paths: &[&str],
) -> Vec<String> {
    let mut outcome = platform.attr_stack.selected_attribute_matches(["diff"]);
    paths
        .iter()
        .map(|path| {
            let entry = platform
                .attr_stack
                .at_entry(*path, None, repo)
                .expect("push path");
            entry.matching_attributes(&mut outcome);
            let state = outcome
                .iter_selected()
                .next()
                .expect("selected `diff`")
                .assignment
                .state;
            match state {
                gix::attrs::StateRef::Set => "set".to_owned(),
                gix::attrs::StateRef::Unset => "unset".to_owned(),
                gix::attrs::StateRef::Unspecified => "unspecified".to_owned(),
                gix::attrs::StateRef::Value(value) => value.as_bstr().to_string(),
            }
        })
        .collect()
}

/// Regression for the single-read design of #1760. gix seeds its global
/// pattern group from the files it reads itself and appends
/// `info/attributes` to the per-directory group on the first push, after
/// the root `.gitattributes`; the hand-built state has only captured
/// bytes to work from, so it places both the root blob and
/// `info/attributes` among the globals, in that order, and withholds the
/// root from the stack's mappings. This fixture binds `diff` from every
/// source — `core.attributesFile`, a root `.gitattributes`, a nested
/// one, `info/attributes` — with every pair of sources overlapping on
/// some path, so a construction that placed either list anywhere else
/// resolves at least one path differently from gix: with
/// `info/attributes` below the root, `a.rs` and `c.txt` come back
/// `root`; with the root left to the stack, the same two.
///
/// The expected states are pinned against `git check-attr --cached`
/// (2.34.1) on the same fixture rather than only against gix, so the
/// parity is with the oracle the walk means to follow. One row is pinned
/// to gix where it departs from git: `git check-attr` has
/// `info/attributes` override every in-tree file, nested ones included,
/// where gix consults a nested `.gitattributes` first, so `sub/b.rs` and
/// `sub/d.txt` are `info` under git and `nested` under gix. The walk has
/// always resolved through gix's stack, so gix's answer is the contract
/// here, and the row says so.
#[test]
fn the_hand_built_stack_resolves_diff_as_gix_s_does() {
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    let sub = tree_of(
        &scratch,
        &[
            (".gitattributes", b"*.rs diff=nested\n*.txt diff=nested\n"),
            ("b.rs", b"fn b() {}\n"),
            ("d.txt", b"d\n"),
        ],
    );
    let mut entries: Vec<gix::objs::tree::Entry> = [
        (".gitattributes", &b"*.rs diff=root\n*.bin binary\n"[..]),
        ("a.rs", b"fn a() {}\n"),
        ("c.txt", b"c\n"),
        ("e.py", b"e\n"),
        ("f.md", b"f\n"),
        ("g.bin", b"g\n"),
    ]
    .into_iter()
    .map(|(name, contents)| gix::objs::tree::Entry {
        mode: gix::objs::tree::EntryKind::Blob.into(),
        filename: name.into(),
        oid: scratch.write_blob(contents).expect("write blob").detach(),
    })
    .collect();
    entries.push(gix::objs::tree::Entry {
        mode: gix::objs::tree::EntryKind::Tree.into(),
        filename: "sub".into(),
        oid: sub,
    });
    entries.sort();
    let root = scratch
        .write_object(&gix::objs::Tree { entries })
        .expect("write tree")
        .detach();
    stage_tree(&scratch, root);

    let info_dir = dir.path().join(".git").join("info");
    std::fs::create_dir_all(&info_dir).expect("mkdir info");
    std::fs::write(
        info_dir.join("attributes"),
        b"*.rs diff=info\n*.txt diff=info\n",
    )
    .expect("write info/attributes");
    let user_home = tempfile::tempdir().expect("temp dir");
    let user_file = user_home.path().join("attributes");
    std::fs::write(&user_file, b"*.txt diff=user\n*.py diff=user\n").expect("write user file");
    append_config(
        dir.path(),
        &format!("[core]\n\tattributesFile = {}\n", config_path(&user_file)),
    );
    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");

    let paths = [
        "a.rs",
        "sub/b.rs",
        "c.txt",
        "sub/d.txt",
        "e.py",
        "f.md",
        "g.bin",
    ];
    // `git check-attr --cached diff <paths>` on this fixture, with the
    // same `core.attributesFile`, answers `info` for the first four, and
    // the stack's answer for `sub/b.rs` and `sub/d.txt` is `nested` for
    // the reason above. The user file is the fallback only `e.py`
    // reaches; `g.bin` is `unset` through the built-in `binary` macro.
    let expected = [
        "info",
        "nested",
        "info",
        "nested",
        "user",
        "unspecified",
        "unset",
    ];
    let mut by_hand = hand_built_platform(&repo, &mut gix::path::env::var);
    let mut by_gix = repo
        .diff_resource_cache_for_tree_diff()
        .expect("gix platform");
    assert_eq!(
        diff_states(&repo, &mut by_gix, &paths),
        expected,
        "gix's own stack must resolve as pinned, or the parity below is with \
         the wrong oracle"
    );
    assert_eq!(
        diff_states(&repo, &mut by_hand, &paths),
        expected,
        "the hand-built stack must resolve every path as gix's does"
    );
}

/// Regression for the first review finding on the single-read design
/// of #1760. gix's `assemble_attribute_globals` loads the system file
/// only under `permissions.attributes.system` and the XDG default only
/// under `.git`, both off for an `Options::isolated()` open; the first
/// shape of the capture read both unconditionally, so on a host whose
/// `~/.config/git/attributes` bound `*.rs` the hand-built stack
/// resolved `a.rs` where gix's own build left it unspecified. The
/// resolver answers `XDG_CONFIG_HOME` with a directory holding such a
/// file, and the two stacks must agree that nothing binds `a.rs`; the
/// second row shows the file is reachable, so the agreement is not
/// vacuous. Verified by revert: a capture that ignores `permissions.git`
/// fails the first row. The `.system` gate has no row — the system
/// location is the fixed `/etc/gitattributes` on Unix, which a test
/// cannot plant — so it is covered by inspection against gix only.
#[test]
fn the_hand_built_stack_skips_the_files_an_isolated_open_withholds() {
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    stage_tree(&scratch, tree_of(&scratch, &[("a.rs", b"fn a() {}\n")]));
    let xdg = tempfile::tempdir().expect("temp dir");
    let git_dir = xdg.path().join("git");
    std::fs::create_dir_all(&git_dir).expect("mkdir xdg git dir");
    std::fs::write(git_dir.join("attributes"), b"*.rs diff=user\n").expect("write xdg attributes");
    let xdg_home: OsString = xdg.path().into();
    let mut env = |name: &str| (name == "XDG_CONFIG_HOME").then(|| xdg_home.clone());
    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");

    let mut by_hand = hand_built_platform(&repo, &mut env);
    let mut by_gix = repo
        .diff_resource_cache_for_tree_diff()
        .expect("gix platform");
    assert_eq!(
        diff_states(&repo, &mut by_gix, &["a.rs"]),
        ["unspecified"],
        "gix's isolated build must not load the XDG file, or the parity below \
         is with the wrong oracle"
    );
    assert_eq!(
        diff_states(&repo, &mut by_hand, &["a.rs"]),
        diff_states(&repo, &mut by_gix, &["a.rs"]),
        "the hand-built stack must withhold the XDG file an isolated open withholds"
    );

    let index = repo.index_or_load_from_head_or_empty().expect("index");
    let sources =
        AttributeSources::read(&repo, &mut env, gix::open::permissions::Attributes::all())
            .expect("attribute files");
    let mut permitted = tree_diff_platform(&repo, sources, false, id_mappings(&index, false))
        .expect("hand-built platform");
    assert_eq!(
        diff_states(&repo, &mut permitted, &["a.rs"]),
        ["user"],
        "under `Attributes::all()` the same resolver reaches the file, so the \
         equality above is the permission at work, not an unreachable path"
    );
}

/// `path` as a git config value: forward slashes only, since a
/// backslash in a value is an escape and gix's parser rejects an
/// unknown one — the Windows leg failed to open every repository whose
/// `core.attributesFile` was written as a native temp path (#1771 has
/// the CLI fixtures' copy of this).
fn config_path(path: &Path) -> String {
    path.to_str()
        .expect("temp path is valid UTF-8")
        .replace('\\', "/")
}

/// An explicit case-sensitive baseline for the case-rule fixtures. The
/// `ignorecase` `gix::init` writes is the host filesystem's probed
/// answer — `true` on APFS, `false` on btrfs/ext4 — and the last
/// `core.ignoreCase` in the file wins, so a fixture appends this before
/// its baseline digest and then [`CASE_FOLDING`] to flip it.
const CASE_SENSITIVE: &str = "[core]\n\tignoreCase = false\n";
/// The folded case rule a fixture flips to from [`CASE_SENSITIVE`].
const CASE_FOLDING: &str = "[core]\n\tignoreCase = true\n";

/// Append `section` to the repository's own config file.
fn append_config(dir: &Path, section: &str) {
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join(".git").join("config"))
        .expect("open config")
        .write_all(section.as_bytes())
        .expect("append config");
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
    append_config(dir, section);
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

/// Write `tree` as the repository's `.git/index`, the way `git add`
/// stages it, so the next context reads its `.gitattributes` blobs from
/// the staged index rather than from one built off `HEAD`.
fn stage_tree(repo: &gix::Repository, tree: gix::ObjectId) {
    repo.index_from_tree(&tree)
        .expect("index from tree")
        .write(gix::index::write::Options::default())
        .expect("write index");
}

/// Open `dir` isolated from the host, as `digest_after_config` does, and
/// digest the `diff` attribute bindings a fresh context resolves.
fn attributes_digest(dir: &Path) -> u64 {
    let repo = gix::open_opts(dir, gix::open::Options::isolated()).expect("open");
    let options = Options::default();
    WalkContext::open(&repo, &options, current_unix_seconds())
        .expect("open context")
        .diff_attributes_digest()
}

/// Regression for #1760: each attribute input the platform reads moves
/// the digest, and what it does not read leaves it alone — an unstaged
/// working-tree `.gitattributes`, which `Source::IdMapping` never opens,
/// and an unrelated tracked file, whose arrival in the index is what
/// every ordinary commit looks like to the incremental splice. Each step
/// changes one input and is compared with its predecessor, so each
/// comparison isolates one of the digest's three terms. Verified by
/// revert, one term at a time: without the id-mapping term the staged
/// step fails, and without its blob-id half the re-staged step; without
/// the `info/attributes` bytes the edit and the reorder steps fail;
/// without the user-file bytes the `core.attributesFile` step fails;
/// without the case term the `core.ignoreCase` step fails.
///
/// The reorder row is the one the previous, per-winner shape of the
/// digest could not pass: `*.rs diff=rs` and `a.* diff` both win
/// somewhere, so swapping them leaves the set of winning rules as it
/// was while flipping which one binds `a.rs`.
///
/// `core.attributesFile` is read under `Options::isolated()` because gix
/// consults the configured path before it asks whether the XDG default
/// may be read; the XDG and system defaults are what isolation turns
/// off, so no row here depends on them.
///
/// The case rule starts from an explicit `core.ignoreCase = false`:
/// `gix::init` probes the filesystem and writes the result into the new
/// repository's config, so on a case-insensitive host (APFS) the
/// baseline is already folded and the `true` step would be a no-op.
/// The last `core.ignoreCase` wins, so the flip is `false → true` on
/// every host.
#[test]
#[allow(clippy::too_many_lines)]
fn the_diff_attributes_digest_tracks_the_index_and_the_attribute_files() {
    const SOURCE: &[u8] = b"fn a() {}\n";
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    append_config(dir.path(), CASE_SENSITIVE);
    let unbound = tree_of(&scratch, &[("a.rs", SOURCE)]);
    commit_tree_as(
        &scratch,
        ("Ada", "ada@example.com"),
        current_unix_seconds() - DAY,
        unbound,
        &[],
    );
    stage_tree(&scratch, unbound);

    let base = attributes_digest(dir.path());
    assert_eq!(
        base,
        attributes_digest(dir.path()),
        "the same index and attribute files digest the same across two opens, \
         or no entry could ever be a hit"
    );

    std::fs::write(dir.path().join(".gitattributes"), b"*.rs diff=rs\n")
        .expect("write worktree .gitattributes");
    assert_eq!(
        base,
        attributes_digest(dir.path()),
        "an unstaged `.gitattributes` is never read by the walk's platform, so \
         digesting it would cold-walk on an edit the walk cannot observe"
    );

    stage_tree(
        &scratch,
        tree_of(
            &scratch,
            &[(".gitattributes", b"*.py diff=py\n"), ("a.rs", SOURCE)],
        ),
    );
    let staged = attributes_digest(dir.path());
    assert_ne!(
        base, staged,
        "a staged `.gitattributes` moves the digest even when its pattern binds \
         no path the index holds: the paths it governs may be deleted ones"
    );

    stage_tree(
        &scratch,
        tree_of(
            &scratch,
            &[(".gitattributes", b"*.py -diff\n"), ("a.rs", SOURCE)],
        ),
    );
    let restaged = attributes_digest(dir.path());
    assert_ne!(
        staged, restaged,
        "the same `.gitattributes` path staged with other content is a new blob \
         id and nothing else: the mapping's id half must be hashed, not only \
         its path"
    );
    stage_tree(
        &scratch,
        tree_of(
            &scratch,
            &[(".gitattributes", b"*.py diff=py\n"), ("a.rs", SOURCE)],
        ),
    );
    assert_eq!(
        staged,
        attributes_digest(dir.path()),
        "restoring the blob restores the digest"
    );

    let info_dir = dir.path().join(".git").join("info");
    std::fs::create_dir_all(&info_dir).expect("mkdir info");
    let info_file = info_dir.join("attributes");
    std::fs::write(&info_file, b"*.rs diff=rs\na.* diff\n").expect("write info/attributes");
    let info = attributes_digest(dir.path());
    assert_ne!(
        staged, info,
        "`info/attributes` appearing binds `a.rs`, and nothing in the index changed"
    );

    std::fs::write(&info_file, b"a.* diff\n*.rs diff=rs\n").expect("reorder info/attributes");
    let reordered = attributes_digest(dir.path());
    assert_ne!(
        info, reordered,
        "the same two rules in the other order bind `a.rs` the other way: the \
         last match wins, so order is an input the rule set alone cannot see"
    );

    let user_home = tempfile::tempdir().expect("temp dir");
    let user_file = user_home.path().join("attributes");
    std::fs::write(&user_file, b"*.rs diff=user\n").expect("write user attributes");
    append_config(
        dir.path(),
        &format!("[core]\n\tattributesFile = {}\n", config_path(&user_file)),
    );
    let user = attributes_digest(dir.path());
    assert_ne!(
        reordered, user,
        "`core.attributesFile` names a file the stack loads below `info/attributes`"
    );

    append_config(dir.path(), CASE_FOLDING);
    let folded = attributes_digest(dir.path());
    assert_ne!(
        user, folded,
        "`core.ignoreCase` lets `*.rs` bind `A.rs` with no file changed"
    );

    // An unrelated file arrives beside the same `.gitattributes` blob:
    // the id mappings, the files and the case rule are all as before.
    // This is the shape of every commit the splice extends an entry
    // across; a term that hashed the index's paths would cold-walk each.
    stage_tree(
        &scratch,
        tree_of(
            &scratch,
            &[
                (".gitattributes", b"*.py diff=py\n"),
                ("a.rs", SOURCE),
                ("notes.txt", b"plain\n"),
            ],
        ),
    );
    assert_eq!(
        folded,
        attributes_digest(dir.path()),
        "a tracked file that is not a `.gitattributes` is not an input: the \
         bindings are unchanged and the entry must stay a splice base"
    );
}

/// Regression for the second review finding on #1760's single read.
/// `core.ignoreCase` is read for the digest's case term and for the
/// stack's glob case, and the first shape of that read was a strict
/// parse: a repository whose value does not parse as a boolean opened
/// (gix's open is lenient, `git` only warns) and then failed in
/// `WalkContext::open` — refusing a walk `main` performed. The value
/// is read as gix's own stack reads it, unset, so the context opens and
/// the digest is the one a repository whose `core.ignoreCase` is
/// `false` produces; the stack's case rule is the same read, so the
/// last row pins that a glob does not fold under the unparseable value
/// either. Verified by revert: a read through `try_boolean` that
/// propagates the parse error fails the first assertion.
///
/// The baseline is an explicit `false`, not the absence of the key:
/// `gix::init` writes the filesystem's probed case rule into the new
/// repository's config, so on APFS the key is already `true` and the
/// first step would compare a folded digest with itself. The last
/// `core.ignoreCase` wins, so the explicit `false` is the baseline on
/// every host.
#[test]
fn an_unparseable_core_ignore_case_reads_as_unset() {
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    append_config(dir.path(), CASE_SENSITIVE);
    stage_tree(
        &scratch,
        tree_of(
            &scratch,
            &[
                (".gitattributes", b"*.rs diff=rs\n"),
                ("a.RS", b"fn a() {}\n"),
            ],
        ),
    );
    let unset = attributes_digest(dir.path());
    append_config(dir.path(), CASE_FOLDING);
    let folded = attributes_digest(dir.path());
    assert_ne!(unset, folded, "the case term is live");

    append_config(dir.path(), "[core]\n\tignoreCase = yesplease\n");
    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");
    let options = Options::default();
    let walk = WalkContext::open(&repo, &options, current_unix_seconds())
        .expect("a `core.ignoreCase` gix opened leniently must not fail the context");
    assert_eq!(
        walk.diff_attributes_digest(),
        unset,
        "an unparseable `core.ignoreCase` is unset to gix's stack, so it must \
         digest as unset, not as the `true` the earlier line set"
    );
    assert_eq!(
        diff_states(&repo, &mut walk.diff_platform().clone(), &["a.RS"]),
        ["unspecified"],
        "the stack matches case-sensitively under the same read: `*.rs` must \
         not bind `a.RS`"
    );
}

/// Digest the attribute bindings of `dir` with the XDG default resolved
/// through `env` rather than the process environment — the resolver
/// `AttributeSources::read` takes so a test can stand in for
/// `gix::path::env::var`.
fn attributes_digest_under(dir: &Path, env: &mut dyn FnMut(&str) -> Option<OsString>) -> u64 {
    let repo = gix::open_opts(dir, gix::open::Options::isolated()).expect("open");
    let index = repo.index_or_load_from_head_or_empty().expect("index");
    let sources = AttributeSources::read(&repo, env, gix::open::permissions::Attributes::all())
        .expect("attribute files");
    diff_attributes_digest(&id_mappings(&index, false), &sources, false)
}

/// Regression for the second #1760 review finding. With
/// `core.attributesFile` unset gix resolves the user file as the XDG
/// default — `$XDG_CONFIG_HOME/git/attributes`, else
/// `$HOME/.config/git/attributes` — through `gix_path::env::var`, whose
/// `HOME` falls back to the platform home directory when the variable is
/// unset. The first shape of the digest resolved through
/// `std::env::var_os`, so with `HOME` unset it hashed the file gix had
/// loaded as absent, and every edit to it was a hit. The resolver is
/// now a parameter, production passes gix's, and this row injects one
/// that answers `XDG_CONFIG_HOME` with a scratch directory: a file
/// appearing there must move the digest, and its edit again. Verified
/// by revert: a `read` that ignores `env` and resolves through
/// `std::env::var_os` fails the first row on a host whose real XDG
/// attributes file is absent, since nothing it reads changes.
#[test]
fn the_user_attributes_file_is_resolved_through_the_injected_environment() {
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    let tree = tree_of(&scratch, &[("a.rs", b"fn a() {}\n")]);
    stage_tree(&scratch, tree);
    let xdg = tempfile::tempdir().expect("temp dir");
    let git_dir = xdg.path().join("git");
    std::fs::create_dir_all(&git_dir).expect("mkdir xdg git dir");
    let xdg_home: OsString = xdg.path().into();
    let mut env = |name: &str| (name == "XDG_CONFIG_HOME").then(|| xdg_home.clone());

    let absent = attributes_digest_under(dir.path(), &mut env);
    std::fs::write(git_dir.join("attributes"), b"*.rs diff=user\n").expect("write xdg attributes");
    let present = attributes_digest_under(dir.path(), &mut env);
    assert_ne!(
        absent, present,
        "the XDG attributes file must be found through the injected resolver"
    );
    std::fs::write(git_dir.join("attributes"), b"*.rs -diff\n").expect("edit xdg attributes");
    assert_ne!(
        present,
        attributes_digest_under(dir.path(), &mut env),
        "an edit to the XDG attributes file must move the digest"
    );
    let mut without_xdg = |_: &str| None;
    assert_eq!(
        absent,
        attributes_digest_under(dir.path(), &mut without_xdg),
        "a resolver that locates no user file digests as the absent one did: \
         the file above was reached through `XDG_CONFIG_HOME`, not by another \
         route"
    );
}

/// gix reads a `core.attributesFile` that names a directory as absent
/// (`IsADirectory` on Unix; `PermissionDenied` is the Windows spelling
/// gix-glob adds, under `cfg!(windows)` here as there), so the capture
/// must too: the walk resolves without it, the digest hashes it as
/// absent, and an open that failed instead would refuse a repository
/// `git` and gix both open. The second step shows the row is not
/// vacuous — a file at the same key moves the digest the directory did
/// not.
#[test]
fn an_attributes_file_that_names_a_directory_reads_as_absent() {
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    let tree = tree_of(&scratch, &[("a.rs", b"fn a() {}\n")]);
    stage_tree(&scratch, tree);
    let base = attributes_digest(dir.path());

    let user_home = tempfile::tempdir().expect("temp dir");
    let user_dir = user_home.path().join("attributes");
    std::fs::create_dir_all(&user_dir).expect("mkdir attributes directory");
    append_config(
        dir.path(),
        &format!("[core]\n\tattributesFile = {}\n", config_path(&user_dir)),
    );
    assert_eq!(
        base,
        attributes_digest(dir.path()),
        "a `core.attributesFile` naming a directory is absent to gix and must \
         be absent to the capture"
    );

    std::fs::remove_dir(&user_dir).expect("remove attributes directory");
    std::fs::write(&user_dir, b"*.rs diff=user\n").expect("write user attributes");
    assert_ne!(
        base,
        attributes_digest(dir.path()),
        "a file at the same path is read, so the equality above was not vacuous"
    );
}

/// The digest hashes no path: `info/attributes` is reached through
/// `common_dir`, whose spelling follows the path the repository was
/// opened by, so a digest over it would differ between `-p .` and
/// `-p "$PWD"`, or between a checkout and a symlink to it, and every
/// alternation would cost a cold walk. Verified by revert: hashing the
/// `info/attributes` path beside its bytes fails this row.
#[test]
#[cfg(unix)]
fn the_diff_attributes_digest_is_the_same_through_a_symlinked_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let checkout = dir.path().join("checkout");
    let scratch = gix::init(&checkout).expect("init repository");
    let tree = tree_of(&scratch, &[("a.rs", b"fn a() {}\n")]);
    commit_tree_as(
        &scratch,
        ("Ada", "ada@example.com"),
        current_unix_seconds() - DAY,
        tree,
        &[],
    );
    stage_tree(&scratch, tree);
    let without_info = attributes_digest(&checkout);
    let info_dir = checkout.join(".git").join("info");
    std::fs::create_dir_all(&info_dir).expect("mkdir info");
    std::fs::write(info_dir.join("attributes"), b"*.rs diff=rs\n").expect("write info/attributes");

    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&checkout, &link).expect("symlink the checkout");

    let direct = attributes_digest(&checkout);
    assert_ne!(
        without_info, direct,
        "`info/attributes` must be read, or the equality below holds for a \
         digest that never saw the file whose path is in question"
    );
    assert_eq!(
        direct,
        attributes_digest(&link),
        "the repository opened through a symlink must digest the same: the \
         path `info/attributes` was read at is not an input"
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
