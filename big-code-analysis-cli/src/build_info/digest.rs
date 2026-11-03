//! The build-inputs digest: one SHA-256 over every file compiled into
//! `bca`, keyed on content alone.
//!
//! Included twice by `#[path]` — from `build.rs`, which bakes the
//! digest into the binary as `BCA_BUILD_INPUTS`, and from the crate
//! under `#[cfg(test)]`, which recomputes it to pin the two in
//! agreement — so it may use only `std` and `sha2`, the one dependency
//! both compilation graphs carry. `utils/self-scan-bca.py` reproduces
//! the same algorithm; the shared fixture in `digest_tests.rs` and
//! `self-scan-bca-test.py` is what holds the three implementations to
//! one answer.
//!
//! The algorithm, stated once so a reimplementation has nothing to
//! guess at:
//!
//! 1. Walk each root under the workspace. A root that is a file
//!    contributes itself; a directory contributes every regular file
//!    beneath it whose extension is in [`INPUT_EXTENSIONS`], except
//!    under a top-level artifact directory named in
//!    [`SKIPPED_TOP_LEVEL`]. Symlinks are refused rather than skipped:
//!    the compiler follows them, so a digest that ignored one would
//!    certify bytes it never saw.
//! 2. Encode each path relative to the workspace as bytes with `/`
//!    separators. Non-UTF-8 names are refused for the same reason.
//! 3. Sort those byte strings.
//! 4. For each file, in that order, feed the hasher
//!    `path ‖ 0x00 ‖ len(content) as u64 little-endian ‖ content`.
//! 5. The digest is the lowercase hex of the SHA-256 state.
//!
//! Content only — never an mtime, mode or owner — so two checkouts
//! with the same bytes agree regardless of when or where they were
//! written, and a `cargo install` from a checkout matches that
//! checkout until something compiled into it changes. CRLF
//! normalisation is deliberately absent: a checkout materialised with
//! other line endings holds other bytes, and the safe answer for a
//! binary built from it is a mismatch.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The workspace-relative roots whose contents reach the compiler.
///
/// Whole crate directories rather than their `src/`, so each crate's
/// `Cargo.toml` and build script are in; the root `Cargo.toml` for the
/// workspace pins and profiles; `Cargo.lock` because the resolver
/// compares digests *instead of* letting cargo fingerprint, and the
/// manifests carry ranged dependencies (`ignore`, `globset`, `regex`)
/// whose resolved version decides which files a walk sees. Runtime
/// inputs (`bca.toml`, `.bcaignore`, the baseline) are not here: a
/// prebuilt binary reads the live copy of each.
pub(crate) const INPUT_ROOTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "src",
    "big-code-analysis-ast",
    "big-code-analysis-cli",
    "tree-sitter-ccomment",
    "tree-sitter-mozcpp",
    "tree-sitter-mozjs",
    "tree-sitter-preproc",
    "tree-sitter-tcl",
];

/// Extensions that are compile-time inputs. `src/` carries over a
/// thousand tracked `.snap` files, and an `insta` accept would
/// otherwise read as a code change; `.snap.new` is untracked but not
/// ignored, so a forced corpus run would too. A grammar's
/// `node-types.json` is `include_str!`ed into a constant nothing in
/// `bca` reaches — the `enums/` generator reads it, and the generated
/// enum under `src/` is what the metrics compile against — so `.json`
/// is out as well: its bytes cannot move a metric.
pub(crate) const INPUT_EXTENSIONS: &[&str] = &["rs", "c", "cc", "h", "toml"];

/// Directories under a crate root that hold nothing the library or
/// the `bca` binary compiles: cargo's integration-test, benchmark and
/// example targets, and the build output a vendored grammar
/// accumulates beside its sources. Matched only as a *direct* child of
/// a root, never as a path component anywhere, so a tracked
/// `src/build/generated.rs` is hashed. Inline `#[cfg(test)]` modules
/// stay in — telling one apart needs the attribute, and they live
/// under `src/` beside the code they test.
pub(crate) const SKIPPED_TOP_LEVEL: &[&str] = &[
    "tests",
    "benches",
    "examples",
    "target",
    "build",
    "node_modules",
];

/// Why a digest could not be taken. Every variant is a reason to report
/// the inputs as unavailable rather than to certify a partial tree.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DigestError {
    /// A root in [`INPUT_ROOTS`] does not exist under the workspace —
    /// the crate is being built from a registry tarball, not a checkout.
    MissingRoot(PathBuf),
    /// A symlink under a root. See the module docs for why it is
    /// refused rather than followed or skipped.
    Symlink(PathBuf),
    /// A path that is not UTF-8, which the shared encoding cannot spell.
    NonUtf8Path(PathBuf),
    /// Any I/O failure during the walk or the read.
    Io(PathBuf, String),
}

impl std::fmt::Display for DigestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRoot(p) => write!(f, "input root is absent: {}", p.display()),
            Self::Symlink(p) => write!(f, "symlink under an input root: {}", p.display()),
            Self::NonUtf8Path(p) => {
                write!(f, "non-UTF-8 path under an input root: {}", p.display())
            }
            Self::Io(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

/// The sorted, workspace-relative, `/`-separated paths of every input
/// file under `workspace`. Exposed separately from [`digest`] so a test
/// can check *which* files are hashed without re-deriving the hash.
pub(crate) fn input_files(workspace: &Path) -> Result<Vec<String>, DigestError> {
    let mut files = Vec::new();
    for root in INPUT_ROOTS {
        let (path, meta) = lookup_root(workspace, root)?;
        if meta.is_file() {
            files.push((*root).to_owned());
        } else {
            // `src/` is the workspace root crate's source directory, not a
            // crate root: a `src/tests/` there would be a module.
            let crate_root = root != &"src";
            collect_dir(workspace, &path, crate_root, &mut files)?;
        }
    }
    files.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    Ok(files)
}

/// Resolve one root under `workspace`. Absent is the registry layout
/// and reads as [`DigestError::MissingRoot`]; any other lookup failure
/// — a symlink loop, an unsearchable parent — is a defect in a real
/// checkout and keeps its own message, so `build.rs` can warn about it
/// rather than file it under "not a workspace".
pub(crate) fn lookup_root(
    workspace: &Path,
    root: &str,
) -> Result<(PathBuf, std::fs::Metadata), DigestError> {
    let path = workspace.join(root);
    let meta = std::fs::symlink_metadata(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            DigestError::MissingRoot(PathBuf::from(root))
        } else {
            DigestError::Io(path.clone(), e.to_string())
        }
    })?;
    if meta.is_symlink() {
        return Err(DigestError::Symlink(PathBuf::from(root)));
    }
    Ok((path, meta))
}

/// What the walk does with one directory entry.
enum Entry {
    Descend(PathBuf),
    Input(String),
    Ignore,
}

fn collect_dir(
    workspace: &Path,
    dir: &Path,
    crate_root: bool,
    out: &mut Vec<String>,
) -> Result<(), DigestError> {
    let io = |e: std::io::Error| DigestError::Io(dir.to_path_buf(), e.to_string());
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(io)?
        .collect::<Result<_, _>>()
        .map_err(io)?;
    // `input_files` re-sorts the whole list by bytes; this sort only makes
    // the first error a deterministic one when two entries are bad.
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        match classify(workspace, &entry, crate_root)? {
            Entry::Descend(path) => collect_dir(workspace, &path, false, out)?,
            Entry::Input(rel) => out.push(rel),
            Entry::Ignore => {}
        }
    }
    Ok(())
}

/// `DirEntry::file_type` is the type `read_dir` already learned from
/// the directory itself — `d_type`, no second `stat` — and it never
/// follows a symlink, which is exactly the question asked here.
fn classify(
    workspace: &Path,
    entry: &std::fs::DirEntry,
    crate_root: bool,
) -> Result<Entry, DigestError> {
    let path = entry.path();
    let kind = entry
        .file_type()
        .map_err(|e| DigestError::Io(path.clone(), e.to_string()))?;
    if kind.is_symlink() {
        return Err(DigestError::Symlink(PathBuf::from(relative_posix(
            workspace, &path,
        )?)));
    }
    let skipped_artifact_dir =
        crate_root && SKIPPED_TOP_LEVEL.iter().any(|s| entry.file_name() == *s);
    Ok(if kind.is_dir() && !skipped_artifact_dir {
        Entry::Descend(path)
    } else if kind.is_file() && has_input_extension(&path) {
        Entry::Input(relative_posix(workspace, &path)?)
    } else {
        Entry::Ignore
    })
}

fn has_input_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| INPUT_EXTENSIONS.contains(&e))
}

fn relative_posix(workspace: &Path, path: &Path) -> Result<String, DigestError> {
    let rel = path.strip_prefix(workspace).unwrap_or(path);
    let mut out = String::new();
    for component in rel.components() {
        let piece = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| DigestError::NonUtf8Path(rel.to_path_buf()))?;
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(piece);
    }
    Ok(out)
}

/// The build-inputs digest of `workspace` as 64 lowercase hex digits.
pub(crate) fn digest(workspace: &Path) -> Result<String, DigestError> {
    digest_of(workspace, &input_files(workspace)?)
}

/// [`digest`] over an already-walked file list, for a caller that has
/// just registered that list with cargo and need not walk it twice.
pub(crate) fn digest_of(workspace: &Path, files: &[String]) -> Result<String, DigestError> {
    let mut hasher = Sha256::new();
    for rel in files {
        let path = workspace.join(rel);
        let content =
            std::fs::read(&path).map_err(|e| DigestError::Io(path.clone(), e.to_string()))?;
        hasher.update(rel.as_bytes());
        hasher.update([0u8]);
        hasher.update((content.len() as u64).to_le_bytes());
        hasher.update(&content);
    }
    Ok(hex_lower(&hasher.finalize()))
}

/// Lowercase hex, two digits per byte. The same shape as the Code
/// Climate writer's `hex_lower_bytes` and `build_id::hex_lower`, repeated
/// because this file is `#[path]`-included by `build.rs`, which can
/// import nothing from the crate — and the crate includes this file
/// only under `cfg(test)`, so `build_id/mod.rs` cannot borrow it back.
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0x0f)] as char);
    }
    out
}
