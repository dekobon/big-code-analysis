use std::path::Path;

use super::digest::{DigestError, INPUT_ROOTS, digest, input_files};

/// The fixture `utils/self-scan-bca-test.py` builds byte-for-byte, and
/// the hex both implementations must produce from it. One file per rule
/// the walker has: a nested input, a NUL byte in content, an empty
/// file, a `.snap` that must be ignored, a top-level `target/` that
/// must be skipped, and a nested `build/` that must *not* be.
const SHARED_FIXTURE_DIGEST: &str =
    "2d6dfcc20156bfc6bcd2bc38ab2cdff68cc864b1a4b6951b891cffc40508a17e";

fn shared_fixture(dir: &Path) {
    let write = |rel: &str, bytes: &[u8]| {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("fixture paths have parents")).expect("mkdir");
        std::fs::write(p, bytes).expect("write");
    };
    write("Cargo.toml", b"[workspace]\n");
    write("Cargo.lock", b"version = 4\n");
    write("src/lib.rs", b"fn a() {}\n");
    write("src/nested/b.rs", b"fn b() {\0}\n");
    write("src/empty.rs", b"");
    write("src/snapshots/x.snap", b"ignored\n");
    write("src/build/generated.rs", b"// nested build dir is hashed\n");
    write("big-code-analysis-ast/Cargo.toml", b"[package]\n");
    write("big-code-analysis-cli/Cargo.toml", b"[package]\n");
    write("big-code-analysis-cli/build.rs", b"fn main() {}\n");
    write(
        "big-code-analysis-cli/tests/smoke.rs",
        b"// integration test, not compiled in\n",
    );
    write("big-code-analysis-cli/pyproject.toml", b"[tool]\n");
    write(
        "src/tests/mod.rs",
        b"// a module named tests under src/ is source\n",
    );
    write("src/.rs", b"// a pure dotfile has no extension\n");
    write("tree-sitter-ccomment/src/parser.c", b"int p;\n");
    write("tree-sitter-ccomment/target/junk.rs", b"skipped\n");
    write(
        "tree-sitter-mozcpp/src/tree_sitter/parser.h",
        b"#define X\n",
    );
    write("tree-sitter-mozjs/src/scanner.c", b"int s;\n");
    write("tree-sitter-preproc/src/node-types.json", b"[]\n");
    write("tree-sitter-tcl/src/grammar.json", b"{}\n");
    write("tree-sitter-tcl/examples/x.c", b"int e;\n");
}

#[test]
fn shared_fixture_hashes_to_the_pinned_digest() {
    let dir = tempfile::tempdir().expect("tempdir");
    shared_fixture(dir.path());
    assert_eq!(
        input_files(dir.path()).expect("walk"),
        [
            "Cargo.lock",
            "Cargo.toml",
            "big-code-analysis-ast/Cargo.toml",
            "big-code-analysis-cli/Cargo.toml",
            "big-code-analysis-cli/build.rs",
            "big-code-analysis-cli/pyproject.toml",
            "src/build/generated.rs",
            "src/empty.rs",
            "src/lib.rs",
            "src/nested/b.rs",
            "src/tests/mod.rs",
            "tree-sitter-ccomment/src/parser.c",
            "tree-sitter-mozcpp/src/tree_sitter/parser.h",
            "tree-sitter-mozjs/src/scanner.c",
        ]
    );
    assert_eq!(digest(dir.path()).expect("digest"), SHARED_FIXTURE_DIGEST);
}

#[test]
fn digest_is_independent_of_creation_order_and_moves_on_every_input_kind() {
    let a = tempfile::tempdir().expect("tempdir");
    shared_fixture(a.path());
    let base = digest(a.path()).expect("digest");

    // Same bytes, written in a different order.
    let b = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(b.path().join("src/nested")).expect("mkdir");
    std::fs::write(b.path().join("src/nested/b.rs"), b"fn b() {\0}\n").expect("write");
    shared_fixture(b.path());
    assert_eq!(digest(b.path()).expect("digest"), base);

    let perturb = |rel: &str, bytes: &[u8]| {
        let d = tempfile::tempdir().expect("tempdir");
        shared_fixture(d.path());
        let p = d.path().join(rel);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(p, bytes).expect("write");
        digest(d.path()).expect("digest")
    };
    assert_ne!(
        perturb("src/lib.rs", b"fn a() { }\n"),
        base,
        "content change"
    );
    assert_ne!(perturb("src/added.rs", b""), base, "an added empty file");
    assert_ne!(
        perturb("src/lib2.rs", b"fn a() {}\n"),
        base,
        "same bytes, new path"
    );
    assert_eq!(
        perturb("src/snapshots/y.snap", b"more"),
        base,
        "a snapshot is not an input"
    );
    assert_eq!(
        perturb("tree-sitter-tcl/target/out.rs", b"x"),
        base,
        "a grammar's top-level target/ is not an input"
    );
    assert_eq!(
        perturb("big-code-analysis-cli/tests/other.rs", b"x"),
        base,
        "a crate's integration tests are not compiled into bca"
    );
    assert_eq!(
        perturb("tree-sitter-tcl/src/node-types.json", b"[1]"),
        base,
        "grammar metadata is not compiled; the generated enum under src/ is"
    );
    assert_ne!(
        perturb("src/tests/other.rs", b"x"),
        base,
        "src/tests/ is a module of the root crate, not a test target"
    );
}

#[test]
fn walk_refuses_a_partial_tree_rather_than_hashing_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    shared_fixture(dir.path());
    std::fs::remove_dir_all(dir.path().join("tree-sitter-tcl")).expect("rm");
    assert_eq!(
        digest(dir.path()),
        Err(DigestError::MissingRoot("tree-sitter-tcl".into()))
    );
}

/// Only an absent root is the registry layout; a root whose lookup
/// fails any other way is a defect in a real checkout and must say so,
/// or `build.rs` reports `unavailable` without the warning it owes.
/// Permissions do not reach `symlink_metadata` on the entry itself —
/// they gate reading the parent, which is the workspace here — so the
/// failure that can is a symlink loop: `ELOOP`, not `NotFound`.
#[cfg(unix)]
#[test]
fn a_root_whose_lookup_fails_is_an_io_error_not_a_missing_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    shared_fixture(dir.path());
    let root = dir.path().join("tree-sitter-tcl");
    std::fs::remove_dir_all(&root).expect("rm");
    std::os::unix::fs::symlink("tree-sitter-tcl", &root).expect("self-referential symlink");
    // `symlink_metadata` reads the link itself, so a loop is only hit
    // one level down; make the parent the loop instead.
    std::fs::remove_file(&root).expect("rm link");
    let hidden = dir.path().join("loop");
    std::os::unix::fs::symlink("loop", &hidden).expect("symlink loop");
    let result = input_files_under_root(dir.path(), "loop/tree-sitter-tcl");
    assert!(
        matches!(result, Err(DigestError::Io(_, ref msg)) if !msg.is_empty()),
        "{result:?}"
    );
}

/// `input_files`' root lookup, pointed at an arbitrary relative root
/// so a test can hand it a path whose `symlink_metadata` fails with
/// something other than `NotFound`.
#[cfg(unix)]
fn input_files_under_root(workspace: &Path, root: &str) -> Result<(), DigestError> {
    super::digest::lookup_root(workspace, root).map(|_| ())
}

/// Every `DigestError` renders its path and its kind; `build.rs` prints
/// the rendering as a `cargo:warning`, so each arm is a line a reader
/// has to be able to act on.
#[test]
fn every_digest_error_names_its_path_and_its_kind() {
    let cases = [
        (
            DigestError::MissingRoot("src".into()),
            "input root is absent: src",
        ),
        (
            DigestError::Symlink("src/link.rs".into()),
            "symlink under an input root: src/link.rs",
        ),
        (
            DigestError::NonUtf8Path("src/x".into()),
            "non-UTF-8 path under an input root: src/x",
        ),
        (
            DigestError::Io("src/lib.rs".into(), "boom".into()),
            "src/lib.rs: boom",
        ),
    ];
    for (error, rendered) in cases {
        assert_eq!(error.to_string(), rendered);
    }
}

/// A root that is itself a symlink is refused before anything beneath
/// it is read — the compiler would follow it to bytes the digest never
/// saw.
#[cfg(unix)]
#[test]
fn walk_refuses_a_symlinked_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    shared_fixture(dir.path());
    std::fs::remove_dir_all(dir.path().join("big-code-analysis-ast")).expect("rm");
    std::os::unix::fs::symlink(
        "big-code-analysis-cli",
        dir.path().join("big-code-analysis-ast"),
    )
    .expect("symlink");
    assert_eq!(
        digest(dir.path()),
        Err(DigestError::Symlink("big-code-analysis-ast".into()))
    );
}

/// A name the shared encoding cannot spell is refused rather than
/// hashed under a lossy spelling the Python side would not reproduce.
/// Linux only: APFS refuses to create such a name (`EILSEQ`), and
/// Windows file names are UTF-16, where the analogous case is an
/// unpaired surrogate this fixture cannot write portably.
#[cfg(target_os = "linux")]
#[test]
fn walk_refuses_a_non_utf8_path() {
    use std::os::unix::ffi::OsStrExt as _;
    let dir = tempfile::tempdir().expect("tempdir");
    shared_fixture(dir.path());
    let name = std::ffi::OsStr::from_bytes(b"bad\xff.rs");
    std::fs::write(dir.path().join("src").join(name), b"fn x() {}\n").expect("write");
    assert!(
        matches!(digest(dir.path()), Err(DigestError::NonUtf8Path(_))),
        "{:?}",
        digest(dir.path())
    );
}

/// `input_files` lists the file, then the read fails: the error carries
/// the file's path, not the directory's, so the warning names what to
/// fix. Skipped for root, who reads anything.
#[cfg(unix)]
#[test]
fn an_unreadable_file_is_an_io_error_naming_the_file() {
    if effective_root() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    shared_fixture(dir.path());
    let _unreadable = Unreadable::new(dir.path().join("src/lib.rs"), 0o644);
    let result = digest(dir.path());
    assert!(
        matches!(result, Err(DigestError::Io(ref p, _)) if p.ends_with("src/lib.rs")),
        "{result:?}"
    );
}

/// A directory the walk cannot list is an `Io` error naming that
/// directory, from `read_dir` rather than from a later read.
#[cfg(unix)]
#[test]
fn an_unlistable_directory_is_an_io_error_naming_the_directory() {
    if effective_root() {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    shared_fixture(dir.path());
    let _unlistable = Unreadable::new(dir.path().join("src/nested"), 0o755);
    let result = digest(dir.path());
    assert!(
        matches!(result, Err(DigestError::Io(ref p, _)) if p.ends_with("src/nested")),
        "{result:?}"
    );
}

/// A path held at mode `0o000` for the guard's lifetime and restored on
/// drop — on an assertion failure too, which matters for a directory:
/// `TempDir::drop` ignores the `remove_dir_all` error a `0o000`
/// subdirectory produces and leaves the tree behind. Declared before
/// the `TempDir` in each test so it drops first.
#[cfg(unix)]
struct Unreadable {
    path: std::path::PathBuf,
    restore: u32,
}

#[cfg(unix)]
impl Unreadable {
    fn new(path: std::path::PathBuf, restore: u32) -> Self {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        Self { path, restore }
    }
}

#[cfg(unix)]
impl Drop for Unreadable {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(self.restore));
    }
}

/// Whether this process reads past permission bits. The *effective*
/// uid, from `id -u`, which every Unix has — `/proc` does not exist on
/// macOS, and the `Uid:` line there is the real uid anyway. An answer
/// that cannot be read is not evidence of an unprivileged run, so it
/// counts as root and skips.
#[cfg(unix)]
fn effective_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .is_none_or(|uid| uid.trim() == "0")
}

#[cfg(unix)]
#[test]
fn walk_refuses_a_symlinked_input() {
    let dir = tempfile::tempdir().expect("tempdir");
    shared_fixture(dir.path());
    std::os::unix::fs::symlink("lib.rs", dir.path().join("src/link.rs")).expect("symlink");
    assert_eq!(
        digest(dir.path()),
        Err(DigestError::Symlink("src/link.rs".into()))
    );
}

#[test]
fn input_roots_name_what_the_build_script_and_the_resolver_agree_on() {
    // The lockfile is in — the resolver bypasses cargo's fingerprint, so
    // a dependency bump must move the digest — and every runtime input
    // is out; the resolver's copy of this list is checked against the
    // same fixture, so this pins the Rust side.
    assert!(INPUT_ROOTS.contains(&"Cargo.lock"));
    assert!(!INPUT_ROOTS.contains(&"bca.toml"));
    assert!(INPUT_ROOTS.contains(&"big-code-analysis-cli"));
}

/// The baked digest equals a fresh walk of the checkout this test runs
/// in. Cargo reruns `build.rs` whenever an input moves, so the two can
/// disagree only if the `rerun-if-changed` set it emits is incomplete —
/// which is the defect this pins.
#[test]
fn baked_digest_matches_a_fresh_walk_of_the_checkout() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the CLI crate sits one below the workspace root");
    match digest(workspace) {
        Ok(live) => assert_eq!(super::BUILD_INPUTS, live),
        Err(DigestError::MissingRoot(_)) => assert_eq!(super::BUILD_INPUTS, "unavailable"),
        Err(e) => panic!("walk failed: {e}"),
    }
}

/// The shared fixture pins the two walkers to the rules it exercises;
/// this pins them to the rules the live tree exercises — a dotfile, an
/// extension neither fixture thought of, whatever lands next. A
/// disagreement is silent in production (the resolver just stops
/// taking the fast path), so it has to be loud here. Same shape as
/// `python_reader_agrees_with_the_compiled_tables` in the ast crate:
/// a missing `python3` is a failure, not a skip.
#[cfg(not(windows))]
#[test]
fn python_digest_agrees_with_the_rust_digest_over_this_checkout() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the CLI crate sits one below the workspace root");
    let rust = match digest(workspace) {
        Ok(digest) => digest,
        Err(DigestError::MissingRoot(_)) => return,
        Err(e) => panic!("walk failed: {e}"),
    };
    let out = std::process::Command::new("python3")
        .args(["-B", "utils/self_scan_digest.py"])
        .arg(workspace)
        .current_dir(workspace)
        .output()
        .expect("python3 is on PATH: the self-scan resolver needs it, so this test does too");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let python = String::from_utf8(out.stdout).expect("digest is UTF-8");
    assert_eq!(python.trim(), rust);
}

/// The sentinel `build.rs` writes for a field it could not determine
/// is a word in the `--version` text and nothing anywhere else: the
/// accessors map it to `None` so SARIF omits the property and the
/// footer its suffix. A checkout build never produces it, so the
/// mapping is pinned on the function rather than on the baked consts.
#[test]
fn the_unavailable_sentinel_reads_as_none_and_anything_else_as_itself() {
    assert_eq!(super::known("unavailable"), None);
    assert_eq!(super::known("deadbeef"), Some("deadbeef"));
    assert_eq!(super::known(""), Some(""));
    // Whatever this build baked, the accessors agree with the consts.
    assert_eq!(super::build_inputs(), super::known(super::BUILD_INPUTS));
    assert_eq!(super::build_git(), super::known(super::BUILD_GIT));
}

#[test]
fn long_version_starts_with_the_bare_version_and_carries_every_field() {
    let text = super::long_version();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some(env!("CARGO_PKG_VERSION")));
    let rest: Vec<_> = lines.collect();
    assert_eq!(rest.len(), 4, "{text}");
    assert!(rest[0].starts_with("build inputs: "), "{text}");
    assert!(rest[1].starts_with("git: "), "{text}");
    // The state is read from this crate's own `cfg`, so the test binary
    // knows the right answer; `DEBUG`, the obvious build-script source,
    // reports debug *info* and is `true` for the release profile here.
    // A hardcoded `off` passed every other assertion in this module.
    let expected = if cfg!(debug_assertions) { "on" } else { "off" };
    assert!(
        rest[2].starts_with("built: ")
            && rest[2].ends_with(&format!("debug-assertions {expected})")),
        "{text}"
    );
    assert!(rest[3].starts_with("build id: "), "{text}");
    // The exact baked value, not a shape: the render could print any
    // 64-hex string and a shape check would never know.
    assert_eq!(rest[0], format!("build inputs: {}", super::BUILD_INPUTS));
}

#[test]
fn the_derive_carries_no_long_version_so_the_man_page_cannot_absorb_one() {
    use clap::CommandFactory as _;
    assert!(crate::Cli::command().get_long_version().is_none());
}
