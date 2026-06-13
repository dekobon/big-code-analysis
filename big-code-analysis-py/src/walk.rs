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
//! parent-ignore awareness, root-relative include/exclude globs (a
//! leading `./` on a pattern is optional — `dir/**` ≡ `./dir/**`, #726),
//! and a per-seed walk where each seed may be a file or a directory.
//! An explicitly named *file* seed bypasses the exclude deny-set
//! (mirroring the CLI, #726): naming a file is a direct request that
//! ignore-style rules must not silently drop; the include allow-list
//! still narrows it, matched on its basename (the root-relative form of
//! a file that is its own walk root — the CLI instead matches an
//! explicit file's include against its CWD-relative spelling, a
//! deliberate difference since these walks are not CWD-scoped).
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
/// the directory prefix. A pattern's leading `./` is optional — it is
/// stripped at compile time so `dir/**` and `./dir/**` are equivalent,
/// matching the CLI (#726). An empty include set means "no include filter"
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
        self.includes(rel) && !self.exclude.is_match(rel)
    }

    /// Whether `rel` satisfies only the include allow-list (empty =
    /// "all"). Used for explicitly named file seeds, which bypass the
    /// exclude deny-set (#726) — see the module docs.
    fn includes(&self, rel: &Path) -> bool {
        self.include.is_empty() || self.include.is_match(rel)
    }
}

/// Compile one pattern list into a [`GlobSet`]. An empty list yields the
/// empty set (matches nothing, treated as "no filter" by the caller).
fn build_globset(patterns: Vec<String>, flag: &str) -> PyResult<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        // Strip a single optional leading `./` so `dir/**` and `./dir/**`
        // compile to the identical glob (#726) — matched paths here are
        // already bare seed-relative, so only the pattern side needs the
        // strip. Keep in sync with the CLI's `walk_seed::strip_dot_slash`
        // (binary-private, hence this small duplicate): a doubled-slash
        // spelling (`.//x`) is left untouched so a malformed relative
        // pattern cannot silently become an absolute-anchored one, and a
        // pattern empty after the strip is skipped.
        let normalized = pattern
            .strip_prefix("./")
            .filter(|rest| !rest.starts_with('/'))
            .unwrap_or(&pattern);
        if normalized.is_empty() {
            continue;
        }
        let glob = Glob::new(normalized)
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
/// Each seed may be a file (emitted directly; the include allow-list is
/// matched on its basename, the exclude deny-set does not apply — an
/// explicitly named file is a direct request, #726) or a directory
/// (walked with gitignore awareness when `respect_gitignore` is true).
/// Files are de-duplicated across seeds while preserving first-seen
/// order, so overlapping seeds do not double-analyse a file.
pub(crate) fn walk_paths(
    paths: &[PathBuf],
    filters: &WalkFilters,
    respect_gitignore: bool,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for seed in paths {
        if seed.is_file() {
            let name = seed.file_name().map_or(seed.as_path(), Path::new);
            if filters.includes(name) && seen.insert(seed.clone()) {
                out.push(seed.clone());
            }
            continue;
        }
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
    // `WalkBuilder` rooted at the seed: a directory seed yields its tree
    // (file seeds are emitted directly by `walk_paths` and never reach
    // here; a nonexistent seed yields one Err entry, skipped below).
    // Gitignore handling mirrors the CLI walker's default-on posture;
    // `respect_gitignore=false` is the
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
        // back to the full path when the strip fails (defensive — a
        // directory walk only yields files strictly under `root`; file
        // seeds are emitted by `walk_paths` and never reach this loop).
        let rel = path.strip_prefix(root).unwrap_or(path);
        if !filters.accepts(rel) {
            continue;
        }
        let owned = path.to_path_buf();
        if seen.insert(owned.clone()) {
            out.push(owned);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WalkFilters, walk_paths};
    use std::path::{Path, PathBuf};

    /// Fixture tree: `src/keep.rs`, `vendor/drop.rs`.
    fn make_tree(dir: &Path) {
        for (sub, name) in [("src", "keep.rs"), ("vendor", "drop.rs")] {
            let d = dir.join(sub);
            std::fs::create_dir_all(&d).expect("create fixture dir");
            std::fs::write(d.join(name), "fn f() {}\n").expect("write fixture");
        }
    }

    fn filters(include: &[&str], exclude: &[&str]) -> WalkFilters {
        WalkFilters::compile(
            include.iter().map(|s| (*s).to_owned()).collect(),
            exclude.iter().map(|s| (*s).to_owned()).collect(),
        )
        .expect("valid globs")
    }

    fn walked(root: &Path, f: &WalkFilters) -> Vec<PathBuf> {
        let mut found = walk_paths(&[root.to_path_buf()], f, true);
        found.sort();
        found
    }

    #[test]
    fn bare_relative_and_dot_prefixed_excludes_are_equivalent() {
        // #726 parity: `vendor/**` and `./vendor/**` must drop the same
        // files. Pre-fix the `./`-prefixed spelling silently matched
        // nothing here (the mirror image of the CLI bug: this walker
        // matches bare seed-relative paths).
        let dir = tempfile::tempdir().expect("tempdir");
        make_tree(dir.path());

        let bare = walked(dir.path(), &filters(&[], &["vendor/**"]));
        let dotted = walked(dir.path(), &filters(&[], &["./vendor/**"]));
        assert_eq!(
            bare,
            vec![dir.path().join("src/keep.rs")],
            "bare `vendor/**` must drop the vendored file"
        );
        assert_eq!(
            bare, dotted,
            "`vendor/**` and `./vendor/**` must drop the same files (#726)"
        );
    }

    #[test]
    fn bare_relative_and_dot_prefixed_includes_are_equivalent() {
        let dir = tempfile::tempdir().expect("tempdir");
        make_tree(dir.path());

        let bare = walked(dir.path(), &filters(&["src/**"], &[]));
        let dotted = walked(dir.path(), &filters(&["./src/**"], &[]));
        assert_eq!(
            bare,
            vec![dir.path().join("src/keep.rs")],
            "bare `src/**` include must keep only src/"
        );
        assert_eq!(
            bare, dotted,
            "`src/**` and `./src/**` must keep the same files (#726)"
        );
    }

    #[test]
    fn explicit_file_seed_bypasses_exclude_but_honors_include() {
        // #726 CLI parity: an explicitly named file seed is analyzed even
        // when an exclude glob matches it; the include allow-list still
        // narrows it, matched on its basename.
        let dir = tempfile::tempdir().expect("tempdir");
        make_tree(dir.path());
        let vendored = dir.path().join("vendor/drop.rs");

        let kept = walk_paths(
            std::slice::from_ref(&vendored),
            &filters(&[], &["*.rs"]),
            true,
        );
        assert_eq!(
            kept,
            vec![vendored.clone()],
            "an explicitly named file must bypass the exclude deny-set (#726)"
        );

        let narrowed = walk_paths(
            std::slice::from_ref(&vendored),
            &filters(&["*.py"], &[]),
            true,
        );
        assert!(
            narrowed.is_empty(),
            "the include allow-list must still narrow an explicit file seed"
        );
        let included = walk_paths(
            std::slice::from_ref(&vendored),
            &filters(&["*.rs"], &[]),
            true,
        );
        assert_eq!(
            included,
            vec![vendored],
            "a basename include must accept a matching explicit file seed"
        );
    }
}
