use std::ffi::OsString;
use std::path::Path;

use super::*;
use crate::vcs::git::tests::{append_config, stage_tree, tree_of};

/// The capture selects the user file gix 0.90's own build selects —
/// the one [`gix::Repository::attribute_global_paths`] returns — on the
/// spelling where asking gix and asking `Snapshot::trusted_path` can
/// give different answers: an `:(optional)` relative
/// `core.attributesFile`. gix interpolates the path, anchors it to the
/// working directory captured at open, and only then runs the
/// existence check `optional` adds; `trusted_path` runs that check
/// unanchored, against whatever the process's directory is at *read*
/// time. The two disagree only once the process has left the
/// directory the repository was opened from, which the lib tests
/// cannot do (a `chdir` needs a process-wide lock they do not share)
/// and which no production path does either — `bca diff`'s `with_cwd`
/// is the one `set_current_dir` in the tree and opens no repository.
/// So the row pins what can be pinned in-process: the capture's
/// answer *is* gix's anchored one — absolute, under
/// [`gix::Repository::current_dir`], read from there — and the user
/// slot was obtained by asking gix. Verified by revert of the
/// selector alone: reading the slot through `trusted_path` with no
/// anchoring leaves it relative and fails the absolute row. The
/// anchoring-order defect itself, which the pre-fix capture had and
/// this fixture cannot reach, is
/// [`the_capture_anchors_an_optional_user_file_to_the_opening_directory`]'s,
/// from a child process that can move away.
/// The `info/attributes` row pins the anchoring gix applies to its
/// own path; the system row pins only what the permissions withhold —
/// the on side is `the_hand_built_capture_selects_the_system_file_gix_selects`.
#[test]
fn the_hand_built_capture_selects_the_user_file_gix_selects() {
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    stage_tree(&scratch, tree_of(&scratch, &[("a.rs", b"fn a() {}\n")]));
    // Relative to the opening directory, which is the test process's:
    // a fresh name there, removed on drop.
    let file = tempfile::Builder::new()
        .prefix("bca-optional-attributes-")
        .tempfile_in(std::env::current_dir().expect("cwd"))
        .expect("plant the optional file");
    std::fs::write(file.path(), b"*.rs diff=user\n").expect("write user file");
    let name = file
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("tempfile names are ASCII")
        .to_owned();
    append_config(
        dir.path(),
        &format!("[core]\n\tattributesFile = :(optional){name}\n"),
    );
    let repo = gix::open_opts(dir.path(), gix::open::Options::isolated()).expect("open");
    let by_gix = repo.attribute_global_paths().expect("gix's selected paths");
    let expected = repo.current_dir().join(&name);
    assert_eq!(
        by_gix,
        std::slice::from_ref(&expected),
        "gix must select the optional file by the opening directory, or the \
         parity below is with the wrong oracle"
    );

    let by_hand = AttributeSources::read(
        &repo,
        &mut gix::path::env::var,
        gix::open::permissions::Attributes::isolated(),
    )
    .expect("attribute files");
    let [system, user, info] = by_hand.files;
    assert_eq!(
        user.path.as_deref(),
        Some(expected.as_path()),
        "the capture must select the optional file gix selects, anchored to \
         the opening directory"
    );
    assert!(
        user.path.as_deref().is_some_and(Path::is_absolute),
        "the user path must be absolute: a relative one is checked against \
         wherever the process is at read time"
    );
    assert_eq!(
        user.bytes.as_deref(),
        Some(&b"*.rs diff=user\n"[..]),
        "and read from there"
    );
    assert_eq!(
        system.path, None,
        "an isolated open withholds the system file"
    );
    assert_eq!(
        info.path,
        Some(
            repo.current_dir()
                .join(repo.common_dir())
                .join("info")
                .join("attributes")
        ),
        "`info/attributes` is anchored the way gix anchors its own"
    );
}

/// The on side of the `permissions.system` gate, which the row above
/// can only show off: a repository opened with `system: true` under
/// otherwise isolated permissions makes gix list the system location
/// — `/etc/gitattributes` on Unix, where `system_prefix()` is the
/// fixed `/` — whether or not the file exists, and the capture read
/// under the open's own permissions, as `WalkContext::open` reads it,
/// must put that same path in its system slot and leave the user slot
/// for what gix lists after it. The capture's own slot is resolved
/// through an `env` that answers nothing, so a `GIT_ATTR_NOSYSTEM` on
/// the host cannot withhold it — gix ignores the variable under the
/// isolated environment permissions, and the row must too; that the
/// user slot stays empty under that same variable is the second
/// thing the row pins, since the set-aside reads it as gix does.
/// Unix only: on Windows the prefix comes from the git installation,
/// which a CI host need not have.
#[test]
#[cfg(unix)]
fn the_hand_built_capture_selects_the_system_file_gix_selects() {
    let dir = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(dir.path()).expect("init repository");
    stage_tree(&scratch, tree_of(&scratch, &[("a.rs", b"fn a() {}\n")]));
    let mut permissions = gix::open::Permissions::isolated();
    permissions.attributes.system = true;
    let repo = gix::open_opts(
        dir.path(),
        gix::open::Options::isolated().permissions(permissions),
    )
    .expect("open");

    let by_gix = repo.attribute_global_paths().expect("gix's selected paths");
    let by_hand = AttributeSources::read(
        &repo,
        &mut |_| None,
        repo.open_options().permissions.attributes,
    )
    .expect("attribute files");
    let [system, user, _info] = by_hand.files;
    let system = system
        .path
        .expect("`system: true` selects the system location");
    assert_eq!(
        system,
        Path::new("/etc/gitattributes"),
        "the system slot is gix's fixed Unix location, which anchoring leaves as is"
    );
    assert_eq!(
        by_gix,
        std::slice::from_ref(&system),
        "gix lists the system file and, with no `core.attributesFile` and no \
         XDG permission, nothing after it"
    );
    assert_eq!(
        user.path, None,
        "the capture must not mistake gix's system entry for the user file"
    );

    // A user file configured *at* the system location is listed twice
    // by gix and is still the user file: the set-aside is positional,
    // not an equality over the whole list.
    append_config(
        dir.path(),
        "[core]\n\tattributesFile = /etc/gitattributes\n",
    );
    let repo = gix::open_opts(
        dir.path(),
        gix::open::Options::isolated().permissions(permissions),
    )
    .expect("reopen");
    assert_eq!(
        repo.attribute_global_paths().expect("gix's selected paths"),
        [system.clone(), system.clone()],
        "gix lists the system file and then the identical configured user file"
    );
    let by_hand = AttributeSources::read(
        &repo,
        &mut |_| None,
        repo.open_options().permissions.attributes,
    )
    .expect("attribute files");
    assert_eq!(
        by_hand.files[1].path.as_deref(),
        Some(system.as_path()),
        "the configured user file survives being the system path"
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

/// The environment variable the parent of
/// [`the_capture_anchors_an_optional_user_file_to_the_opening_directory`]
/// sets so its child half runs; the child bails without it, and is
/// `#[ignore]`d so no harness runs it on its own.
const CHILD_MARKER: &str = "BCA_ATTRIBUTES_ANCHOR_CHILD";
/// The repository directory the child opens, and the directory it moves
/// to before reading.
const CHILD_REPO: &str = "BCA_ATTRIBUTES_ANCHOR_REPO";
const CHILD_AWAY: &str = "BCA_ATTRIBUTES_ANCHOR_AWAY";
/// What the child prints once every assertion has held.
const CHILD_SENTINEL: &str =
    "attributes-anchor-child: optional user file read from the opening directory";

/// The anchoring *order* the in-process rows cannot reach: an
/// `:(optional)` relative `core.attributesFile` whose file exists under
/// the directory the repository was opened from and not under the
/// directory the process has moved to by the time the capture reads.
/// gix anchors the path to the opening directory before the existence
/// check `optional` adds; `Snapshot::trusted_path` runs that check
/// against wherever the process is, unanchored. The capture must give
/// gix's answer (#1813).
///
/// A `chdir` is process-wide, so the test runs its reading half in a
/// child process: this test binary again, started *in* the repository
/// directory (gix captures the process directory at open, not the
/// repository's), filtered to the `#[ignore]`d child below, which
/// opens the repository, moves away, reads, and prints
/// [`CHILD_SENTINEL`]. The parent asserts the exit status, the
/// sentinel, and libtest's own `1 passed`, so a misspelled child name
/// reads as `0 passed` rather than as a vacuous pass. Verified by
/// revert: a capture that reads the slot through `trusted_path` checks
/// `away/<name>`, finds nothing, falls back to `None`, and the child's
/// first assertion fails.
#[test]
fn the_capture_anchors_an_optional_user_file_to_the_opening_directory() {
    let repo_dir = tempfile::tempdir().expect("temp dir");
    let away = tempfile::tempdir().expect("temp dir");
    let scratch = gix::init(repo_dir.path()).expect("init repository");
    stage_tree(&scratch, tree_of(&scratch, &[("a.rs", b"fn a() {}\n")]));
    std::fs::write(repo_dir.path().join("attributes"), b"*.rs diff=user\n")
        .expect("write user file");
    append_config(
        repo_dir.path(),
        "[core]\n\tattributesFile = :(optional)attributes\n",
    );

    // `module_path!()` carries the crate name; libtest's names do not.
    let child = format!(
        "{}::the_capture_reads_the_optional_user_file_after_moving_away",
        module_path!()
            .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
            .expect("the test module is nested under the crate root")
    );
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            &child,
            "--include-ignored",
            "--no-capture",
            "--format",
            "pretty",
        ])
        .current_dir(repo_dir.path())
        .env(CHILD_MARKER, "1")
        .env(CHILD_REPO, repo_dir.path())
        .env(CHILD_AWAY, away.path())
        .output()
        .expect("spawn the child half");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the child half must pass; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains(CHILD_SENTINEL),
        "the child must have reached its sentinel; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("test result: ok. 1 passed"),
        "exactly one test must have run in the child, or the filter missed it; stdout:\n{stdout}"
    );
}

/// The child half of
/// [`the_capture_anchors_an_optional_user_file_to_the_opening_directory`];
/// see there. Ignored so only the parent's filtered spawn runs it.
#[test]
#[ignore = "child half of the subprocess anchoring test; run by its parent"]
fn the_capture_reads_the_optional_user_file_after_moving_away() {
    assert!(
        std::env::var_os(CHILD_MARKER).is_some(),
        "this is the child half of \
         `the_capture_anchors_an_optional_user_file_to_the_opening_directory`; \
         run that test instead of including ignored tests here"
    );
    let repo_dir = std::env::var_os(CHILD_REPO).expect("parent sets the repository directory");
    let away = std::env::var_os(CHILD_AWAY).expect("parent sets the away directory");
    // By identity, not spelling: `current_dir` resolves a symlinked
    // `TMPDIR` where the parent's `tempdir` path keeps the link.
    assert_eq!(
        std::env::current_dir()
            .expect("cwd")
            .canonicalize()
            .expect("canonical cwd"),
        Path::new(&repo_dir)
            .canonicalize()
            .expect("canonical repository directory"),
        "the parent must start the child in the repository directory"
    );
    let repo = gix::open_opts(&repo_dir, gix::open::Options::isolated()).expect("open");
    std::env::set_current_dir(&away).expect("move away from the opening directory");

    let expected = repo.current_dir().join("attributes");
    let by_hand = AttributeSources::read(
        &repo,
        &mut gix::path::env::var,
        gix::open::permissions::Attributes::isolated(),
    )
    .expect("attribute files");
    let user = &by_hand.files[1];
    assert_eq!(
        user.path.as_deref(),
        Some(expected.as_path()),
        "the optional user file must be the one under the opening directory, \
         not dropped for being absent under the process's directory"
    );
    assert_eq!(
        user.bytes.as_deref(),
        Some(&b"*.rs diff=user\n"[..]),
        "and its bytes must have been read from there"
    );
    assert_eq!(
        repo.attribute_global_paths().expect("gix's selected paths"),
        std::slice::from_ref(&expected),
        "gix itself selects the same file from the same place"
    );
    println!("{CHILD_SENTINEL}");
}
