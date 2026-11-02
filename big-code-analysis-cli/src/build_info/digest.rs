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
/// workspace pins and profiles. `Cargo.lock` is not listed — cargo
/// already treats it as a fingerprint input, and a lockfile that moves
/// without any source moving changes no metric. Runtime inputs
/// (`bca.toml`, `.bcaignore`, the baseline) are not here either: a
/// prebuilt binary reads the live copy of each.
pub(crate) const INPUT_ROOTS: &[&str] = &[
    "Cargo.toml",
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
/// ignored, so a forced corpus run would too.
pub(crate) const INPUT_EXTENSIONS: &[&str] = &["rs", "c", "cc", "h", "toml", "json"];

/// Build output a vendored grammar crate accumulates beside its
/// sources. Matched only as a *direct* child of a `tree-sitter-*` root,
/// never as a path component anywhere, so a tracked
/// `src/build/generated.rs` under any other root is hashed.
pub(crate) const SKIPPED_TOP_LEVEL: &[&str] = &["target", "build", "node_modules"];

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
        let path = workspace.join(root);
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|_| DigestError::MissingRoot(PathBuf::from(root)))?;
        if meta.is_symlink() {
            return Err(DigestError::Symlink(PathBuf::from(root)));
        }
        if meta.is_file() {
            files.push((*root).to_owned());
        } else {
            let grammar = root.starts_with("tree-sitter-");
            collect_dir(workspace, &path, grammar, &mut files)?;
        }
    }
    files.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    Ok(files)
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
    grammar_top: bool,
    out: &mut Vec<String>,
) -> Result<(), DigestError> {
    let io = |e: std::io::Error| DigestError::Io(dir.to_path_buf(), e.to_string());
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(io)?
        .collect::<Result<_, _>>()
        .map_err(io)?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        match classify(workspace, &entry.path(), grammar_top)? {
            Entry::Descend(path) => collect_dir(workspace, &path, false, out)?,
            Entry::Input(rel) => out.push(rel),
            Entry::Ignore => {}
        }
    }
    Ok(())
}

fn classify(workspace: &Path, path: &Path, grammar_top: bool) -> Result<Entry, DigestError> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| DigestError::Io(path.to_path_buf(), e.to_string()))?;
    let rel = relative_posix(workspace, path)?;
    if meta.is_symlink() {
        return Err(DigestError::Symlink(PathBuf::from(rel)));
    }
    let skipped_artifact_dir = grammar_top
        && path
            .file_name()
            .is_some_and(|n| SKIPPED_TOP_LEVEL.iter().any(|s| n == *s));
    Ok(if meta.is_dir() && !skipped_artifact_dir {
        Entry::Descend(path.to_path_buf())
    } else if meta.is_file() && has_input_extension(path) {
        Entry::Input(rel)
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
    let mut hasher = Sha256::new();
    for rel in input_files(workspace)? {
        let path = workspace.join(&rel);
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
/// Climate writer's `hex_lower_bytes`, repeated here because this file
/// may import nothing from either crate.
pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0x0f)] as char);
    }
    out
}
