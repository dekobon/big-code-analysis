// bca: suppress-file(halstead, nargs, nexits)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// gix open/diff plumbing with many `?` error maps), not per-function
// logic complexity (cognitive/cyclomatic stay enforced).

//! Repository discovery and target-tree file enumeration.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use super::diff_err;
use crate::vcs::error::Error;
use crate::vcs::options::FileTypeScope;

/// A discovered repository plus the facts the walk needs up front.
pub(crate) struct OpenRepo {
    /// The opened repository handle.
    pub repo: gix::Repository,
    /// Working-tree root, canonicalised; `None` for bare repos.
    pub workdir: Option<PathBuf>,
    /// Whether the repository is a shallow clone (history truncated).
    pub shallow: bool,
}

/// Discover and open the repository containing `root`.
///
/// # Errors
///
/// Returns [`Error::NotARepository`] when no repository encloses
/// `root`, or [`Error::OpenRepository`] for any other open failure
/// (corrupt repo, permission denied, …).
pub(crate) fn open(root: &Path) -> Result<OpenRepo, Error> {
    let repo = gix::discover(root).map_err(|e| map_discover_error(root, &e))?;
    let shallow = repo.is_shallow();
    // Canonicalise the work dir so absolute-path lookups (used by
    // `bca metrics --metrics vcs`) strip a matching prefix even when
    // the caller passed a symlinked or relative root.
    let workdir = repo
        .workdir()
        .map(|w| w.canonicalize().unwrap_or_else(|_| w.to_path_buf()));
    Ok(OpenRepo {
        repo,
        workdir,
        shallow,
    })
}

/// Discover the working-tree root of the repository containing `path`.
///
/// Returns the canonicalised work-tree directory (the same value
/// [`OpenRepo::workdir`] carries), or `None` when `path` is not inside a
/// repository or the repository is bare (no work tree). Backs the public
/// [`crate::vcs::workdir_root`] helper.
///
/// `gix::discover` walks upward from a *directory*; a `path` that names a
/// file (or does not exist) is discovered from its parent directory so the
/// helper accepts either a file or a directory.
pub(crate) fn workdir_root(path: &Path) -> Option<PathBuf> {
    let directory = if path.is_dir() {
        path
    } else {
        // `Path::parent()` of a bare filename is `Some("")`; map that empty
        // parent to the current directory so discovery starts somewhere
        // real (mirroring `crate::vcs::repo_root_for` in the front ends).
        match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        }
    };
    open(directory).ok().and_then(|open| open.workdir)
}

/// Resolve a revision spelling to the commit it names, peeling through
/// any tag. Shared by the file-level walk ([`super::build`]) and the
/// single-commit JIT path ([`super::jit`]) so both map a resolution
/// failure to the same [`Error::ResolveRef`].
///
/// # Errors
///
/// Returns [`Error::ResolveRef`] when `reference` does not resolve to a
/// commit (unknown ref, unborn `HEAD`, …).
pub(crate) fn resolve_commit<'repo>(
    repo: &'repo gix::Repository,
    reference: &str,
) -> Result<gix::Commit<'repo>, Error> {
    let resolve_err = |e: &dyn std::fmt::Display| Error::ResolveRef {
        reference: reference.to_owned(),
        reason: e.to_string(),
    };
    let tip = repo
        .rev_parse_single(reference.as_bytes())
        .map_err(|e| resolve_err(&e))?;
    tip.object()
        .map_err(|e| resolve_err(&e))?
        .peel_to_commit()
        .map_err(|e| resolve_err(&e))
}

/// Map a discovery failure to a typed error, distinguishing "no
/// repository here" (a clean user-facing condition) from genuine open
/// failures.
fn map_discover_error(root: &Path, error: &gix::discover::Error) -> Error {
    use gix::discover::upwards::Error as Upwards;
    if let gix::discover::Error::Discover(
        Upwards::NoGitRepository { .. }
        | Upwards::NoGitRepositoryWithinCeiling { .. }
        | Upwards::NoGitRepositoryWithinFs { .. }
        | Upwards::NoTrustedGitRepository { .. },
    ) = error
    {
        return Error::NotARepository(root.to_path_buf());
    }
    Error::OpenRepository(error.to_string())
}

/// Enumerate the in-scope tracked text files at `target_tree`, mapping
/// each repository-relative path to its line count (SLOC).
///
/// Binary blobs (line counts unavailable) and symlinks are skipped, as
/// the issue specifies. `scope` is an additional extension-only filter
/// (issue #576): an out-of-scope file is dropped *before* its (costlier)
/// line-count diff runs, so the history walk never seeds an accumulator
/// for it. Implemented as a diff of the empty tree against the target
/// tree: every in-scope file then appears as an `Addition` whose "added"
/// line count is the file's total line count.
///
/// # Errors
///
/// Returns [`Error::Diff`] if the diff machinery fails, or if a path is
/// not valid UTF-8 on a platform that requires it.
pub(crate) fn enumerate_target_files(
    repo: &gix::Repository,
    target_tree: &gix::Tree<'_>,
    scope: &FileTypeScope,
) -> Result<HashMap<PathBuf, u64>, Error> {
    let empty = repo.empty_tree();
    let mut cache = repo.diff_resource_cache_for_tree_diff().map_err(diff_err)?;
    let mut files = HashMap::new();

    empty
        .changes()
        .map_err(diff_err)?
        .options(|opts| {
            opts.track_path();
            // No rewrite tracking needed: every entry is a fresh
            // addition relative to the empty tree.
            opts.track_rewrites(None);
        })
        .for_each_to_obtain_tree(target_tree, |change| -> Result<ControlFlow<()>, Error> {
            let entry_mode = change.entry_mode();
            if entry_mode.is_blob() && !entry_mode.is_link() {
                let path = bstr_to_path(change.location())?;
                // Extension-only scope check first — it reads no blob, so
                // an out-of-scope file costs nothing beyond the path decode.
                if !scope.includes(&path) {
                    return Ok(ControlFlow::Continue(()));
                }
                if let Some(stats) = change
                    .diff(&mut cache)
                    .map_err(diff_err)?
                    .line_counts()
                    .map_err(diff_err)?
                {
                    // `after` is the destination line count — i.e. the
                    // file's SLOC, since the source (empty tree) is 0.
                    files.insert(path, stats.after as u64);
                }
            }
            Ok(ControlFlow::Continue(()))
        })
        .map_err(diff_err)?;

    Ok(files)
}

/// Convert a git path (raw bytes) to a `PathBuf`, erroring rather than
/// lossily mangling a non-UTF-8 path that is used as a map key
/// (per the path rules in AGENTS.md).
pub(crate) fn bstr_to_path(location: &bstr::BStr) -> Result<PathBuf, Error> {
    gix::path::try_from_bstr(location)
        .map(std::borrow::Cow::into_owned)
        .map_err(|_| Error::Diff(format!("path {location:?} is not valid UTF-8")))
}
