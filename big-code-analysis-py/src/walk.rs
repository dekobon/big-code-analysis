// bca: suppress-file(halstead, nargs)
// File-level halstead/nargs are aggregation artifacts: the glob-set
// builders and the many `WalkBuilder` option-chain arguments across a
// few small functions, not per-function logic complexity
// (cognitive/cyclomatic stay enforced).

//! Directory walker backing `analyze_paths` (issue #658).
//!
//! Reuses the gitignore-aware [`ignore`] crate (the same crate the `bca`
//! CLI walker is built on) plus [`globset`] include/exclude filtering,
//! rather than reimplementing the walk in Python. The CLI's own walker is
//! binary-private, so this is a focused re-expression of the same
//! behaviour against the underlying crates: `.gitignore` / `.ignore` /
//! parent-ignore awareness, root-relative include/exclude globs, and a
//! per-seed walk where each seed may be a file or a directory.
//!
//! Language inference and the generated-file filter are *not* applied
//! here — they run per file inside [`crate::analysis::analyze_path`]
//! (which `analyze_paths` calls on every discovered path), exactly as the
//! single-file `analyze` does. A discovered file whose language cannot be
//! inferred therefore surfaces as an `AnalysisFailure` element, matching
//! the batch never-raise contract.

use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use pyo3::PyResult;
use pyo3::exceptions::PyValueError;

/// Compiled include / exclude glob filters applied to walked files.
///
/// Both are matched against each file's path *relative to the walk seed*
/// (mirroring the CLI's walk-root-relative anchoring), so `include=["*.rs"]`
/// matches `src/lib.rs` by its basename glob without the caller spelling out
/// the directory prefix. An empty include set means "no include filter"
/// (every file passes); an empty exclude set excludes nothing.
pub(crate) struct WalkFilters {
    include: GlobSet,
    exclude: GlobSet,
}

impl WalkFilters {
    /// Compile the include / exclude pattern lists into glob sets,
    /// surfacing a `ValueError` on a malformed pattern (naming the bad
    /// glob, so a typo fails fast rather than silently matching nothing).
    pub(crate) fn compile(include: Vec<String>, exclude: Vec<String>) -> PyResult<Self> {
        Ok(Self {
            include: build_globset(include, "include")?,
            exclude: build_globset(exclude, "exclude")?,
        })
    }

    /// Whether a file at `rel` (seed-relative) passes the filters: it must
    /// match the include set (when non-empty) and must not match the
    /// exclude set.
    fn accepts(&self, rel: &Path) -> bool {
        if !self.include.is_empty() && !self.include.is_match(rel) {
            return false;
        }
        !self.exclude.is_match(rel)
    }
}

/// Compile one pattern list into a [`GlobSet`]. An empty list yields the
/// empty set (matches nothing, treated as "no filter" by the caller).
fn build_globset(patterns: Vec<String>, flag: &str) -> PyResult<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(&pattern)
            .map_err(|e| PyValueError::new_err(format!("invalid {flag} glob {pattern:?}: {e}")))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|e| PyValueError::new_err(format!("building {flag} glob set: {e}")))
}

/// Walk every seed in `paths`, returning the discovered files in a stable
/// order.
///
/// Each seed may be a file (emitted directly, still subject to the glob
/// filters relative to its own basename) or a directory (walked with
/// gitignore awareness when `respect_gitignore` is true). Files are
/// de-duplicated across seeds while preserving first-seen order, so
/// overlapping seeds do not double-analyse a file.
pub(crate) fn walk_paths(
    paths: &[PathBuf],
    filters: &WalkFilters,
    respect_gitignore: bool,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for seed in paths {
        walk_seed(seed, filters, respect_gitignore, &mut out, &mut seen);
    }
    out
}

/// Walk a single seed, appending its discovered files to `out`.
fn walk_seed(
    root: &Path,
    filters: &WalkFilters,
    respect_gitignore: bool,
    out: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    // `WalkBuilder` rooted at the seed: a file seed yields just that file,
    // a directory seed yields its tree. Gitignore handling mirrors the CLI
    // walker's default-on posture; `respect_gitignore=false` is the
    // `analyze_paths(..., respect_gitignore=False)` opt-out.
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(respect_gitignore)
        // Hidden (dot-prefixed) entries are skipped unconditionally, matching
        // the CLI walker's `.hidden(true)` — `standard_filters(false)` would
        // otherwise un-hide them, so `respect_gitignore=false` would walk
        // `.git/` / `.venv/` that the CLI never sees. Set after
        // `standard_filters` so it wins regardless of the gitignore toggle.
        .hidden(true)
        // `require_git(false)` honours a `.gitignore` even when the seed is
        // not inside an initialised git repository — matching the CLI walker
        // (`big-code-analysis-cli/src/lib.rs`) so the two surfaces filter
        // identically.
        .require_git(false)
        .git_ignore(respect_gitignore)
        .git_exclude(respect_gitignore)
        .git_global(respect_gitignore)
        .ignore(respect_gitignore)
        .parents(respect_gitignore)
        // Symlinks are not followed: the CLI walker leaves the default
        // (off) too, so a self-referential link can't loop the walk.
        .follow_links(false);
    for entry in builder.build() {
        let Ok(entry) = entry else {
            // A per-entry I/O error (permission, race) is skipped rather
            // than aborting the whole walk — the same graceful posture the
            // CLI walker takes for an unreadable subtree.
            continue;
        };
        // Directories (including the seed itself when it is a dir) are not
        // analysis targets; only files are.
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        // Match the glob filters against the seed-relative path so a
        // basename glob (`*.rs`) matches without a directory prefix; fall
        // back to the full path when the strip fails (a file seed strips to
        // the empty path, so match on its file name instead).
        let rel = path.strip_prefix(root).unwrap_or(path);
        let rel = if rel.as_os_str().is_empty() {
            Path::new(path.file_name().unwrap_or(path.as_os_str()))
        } else {
            rel
        };
        if !filters.accepts(rel) {
            continue;
        }
        let owned = path.to_path_buf();
        if seen.insert(owned.clone()) {
            out.push(owned);
        }
    }
}
