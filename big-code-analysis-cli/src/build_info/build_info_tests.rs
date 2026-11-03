use std::path::Path;

use super::digest::{DigestError, INPUT_ROOTS, digest, input_files};
use super::elf::{build_id, hex_lower};

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

/// A minimal little-endian ELF64 image: header, one program header of
/// `kind`, and a note segment built from `notes`.
fn elf64(kind: u32, notes: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 0x40];
    out[..4].copy_from_slice(b"\x7fELF");
    out[4] = 2;
    out[5] = 1;
    out[0x20..0x28].copy_from_slice(&0x40u64.to_le_bytes());
    out[0x36..0x38].copy_from_slice(&0x38u16.to_le_bytes());
    out[0x38..0x3a].copy_from_slice(&1u16.to_le_bytes());
    let seg_at = 0x40u64 + 0x38;
    let mut ph = vec![0u8; 0x38];
    ph[..4].copy_from_slice(&kind.to_le_bytes());
    ph[0x08..0x10].copy_from_slice(&seg_at.to_le_bytes());
    ph[0x20..0x28].copy_from_slice(&(notes.len() as u64).to_le_bytes());
    out.extend(ph);
    out.extend_from_slice(notes);
    out
}

fn note(name: &[u8], kind: u32, desc: &[u8]) -> Vec<u8> {
    let len = |s: &[u8]| u32::try_from(s.len()).expect("fixture notes are tiny");
    let mut out = Vec::new();
    out.extend(len(name).to_le_bytes());
    out.extend(len(desc).to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(name);
    out.resize(out.len().next_multiple_of(4), 0);
    out.extend(desc);
    out.resize(out.len().next_multiple_of(4), 0);
    out
}

#[test]
fn build_id_is_read_past_an_unrelated_note_and_hex_rendered() {
    let id = [0xde, 0xad, 0xbe, 0xef, 0x01];
    let mut notes = note(b"GNU\0", 1, b"\0\0\0\0\x03\0\0\0\x02\0\0\0\0\0\0\0");
    notes.extend(note(b"GNU\0", 3, &id));
    let image = elf64(4, &notes);
    assert_eq!(build_id(&image), Some(&id[..]));
    assert_eq!(
        build_id(&image).map(hex_lower).as_deref(),
        Some("deadbeef01")
    );
}

#[test]
fn build_id_is_none_for_every_shape_the_reader_does_not_support() {
    let id = [1u8, 2, 3];
    let good = elf64(4, &note(b"GNU\0", 3, &id));
    assert_eq!(
        build_id(&good),
        Some(&id[..]),
        "the fixture itself must parse"
    );

    let mut wrong_magic = good.clone();
    wrong_magic[0] = 0;
    assert_eq!(build_id(&wrong_magic), None, "wrong magic");

    let mut elf32 = good.clone();
    elf32[4] = 1;
    assert_eq!(build_id(&elf32), None, "ELF32");

    let mut big_endian = good.clone();
    big_endian[5] = 2;
    assert_eq!(build_id(&big_endian), None, "big-endian");

    assert_eq!(build_id(&good[..0x30]), None, "truncated header");
    assert_eq!(build_id(&good[..0x50]), None, "truncated program header");
    assert_eq!(
        build_id(&elf64(1, &note(b"GNU\0", 3, &id))),
        None,
        "not a PT_NOTE segment"
    );
    assert_eq!(
        build_id(&elf64(4, &note(b"XYZ\0", 3, &id))),
        None,
        "not the GNU note"
    );
    assert_eq!(
        build_id(&elf64(4, &note(b"GNU\0", 7, &id))),
        None,
        "not NT_GNU_BUILD_ID"
    );

    // Every prefix of a valid image must read as absent, not panic and
    // not misparse. This walks each `?` in the header and program-header
    // readers; it never reaches the note reader, because the shortened
    // image keeps the program header's declared segment size and the
    // segment slice fails first.
    for cut in 0..good.len() {
        assert_eq!(build_id(&good[..cut]), None, "truncated at byte {cut}");
    }
    // The note reader's own short reads need a segment whose *declared*
    // size is honest: 4 bytes reach `descsz`'s read, 8 reach `kind`'s,
    // 12 reach the name's. These execute each read's `None` arm; they
    // cannot tell a failed read from an infallible one, because the
    // next read fails the same way and no id can come out of a note
    // that short — the outcome, not the mechanism, is what is pinned.
    for short in [&[0u8; 4][..], &[0u8; 8], &[0u8; 12]] {
        assert_eq!(
            build_id(&elf64(4, short)),
            None,
            "{}-byte note segment",
            short.len()
        );
    }

    let mut overflow = good.clone();
    overflow[0x40 + 0x20..0x40 + 0x28].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(build_id(&overflow), None, "segment size past the end");
    let mut far_offset = good.clone();
    far_offset[0x40 + 0x08..0x40 + 0x10].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(build_id(&far_offset), None, "segment offset past the end");

    let mut more_headers_than_exist = good.clone();
    more_headers_than_exist[0x38..0x3a].copy_from_slice(&7u16.to_le_bytes());
    assert_eq!(
        build_id(&more_headers_than_exist),
        Some(&id[..]),
        "a phnum past the table still finds the note in the headers present"
    );

    let mut huge_phoff = good.clone();
    huge_phoff[0x20..0x28].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(
        build_id(&huge_phoff),
        None,
        "program-header offset past the end"
    );

    // A name length that overruns the segment: the note walk fails
    // closed. (A clamped slice would fail closed too — the name would
    // not compare equal — so this pins the outcome, not the mechanism;
    // the bounded-read spelling is the module's safety argument.)
    let mut lying_namesz = good;
    let note_at = 0x40 + 0x38;
    lying_namesz[note_at..note_at + 4].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
    assert_eq!(build_id(&lying_namesz), None, "namesz past the segment");

    // A descriptor length that overruns: same, on the other field.
    let mut lying_descsz = elf64(4, &note(b"GNU\0", 3, &id));
    lying_descsz[note_at + 4..note_at + 8].copy_from_slice(&0xffff_fff0u32.to_le_bytes());
    assert_eq!(build_id(&lying_descsz), None, "descsz past the segment");
}

/// A capability probe, not a contract: the linker on this host is what
/// decides whether the test binary carries a note at all. When it does,
/// the reader must find it and render 40 hex digits; when it does not,
/// `unavailable` is the documented answer.
#[cfg(target_os = "linux")]
#[test]
fn running_executable_yields_hex_or_nothing() {
    match super::elf::current_build_id() {
        Some(id) => assert!(
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "{id}"
        ),
        None => eprintln!("no GNU build-id note in the test binary on this host"),
    }
}
