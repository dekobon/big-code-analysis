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

/// Classify `seed` without letting a *missing* path masquerade as a real
/// one. `symlink_metadata` first establishes the seed exists at all (it
/// stats the link itself, so a dangling symlink correctly errors here —
/// symmetric with the walk's `follow_links(false)`); a live symlink is then
/// resolved through `metadata` exactly once to classify its target. Returns
/// `Ok(true)` for a file, `Ok(false)` for a directory, and `Err` only when
/// the seed (or a live symlink's target) does not exist — which the caller
/// surfaces as a nonexistent-seed failure (#858).
///
/// This mirrors the CLI's `seed_kind` (`big-code-analysis-cli/src/lib.rs`),
/// which is binary-private; the duplication keeps the two walk surfaces in
/// parity on the #596 "a typo in a path seed fails loudly, not silently"
/// contract.
fn seed_kind(seed: &Path) -> std::io::Result<bool> {
    let link_meta = seed.symlink_metadata()?;
    let meta = if link_meta.file_type().is_symlink() {
        // Explicitly-named symlink seed: resolve its target once. A
        // dangling target propagates the `Err` (treated as nonexistent).
        seed.metadata()?
    } else {
        link_meta
    };
    Ok(meta.is_file())
}

/// Outcome of [`walk_paths`]: every discovered file plus the seeds that did
/// not exist (or whose symlink dangled).
///
/// A nonexistent seed is surfaced rather than silently dropped (#858): the
/// CLI hard-errors on a missing `--paths` seed (#596), and the Python
/// re-expression keeps parity by handing the bad seeds back to
/// [`crate::batch::analyze_paths`], which folds each into an
/// `AnalysisFailure` element — making a caller's typo visible without
/// breaking the never-raise posture of the batch result vector.
pub(crate) struct WalkOutcome {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) missing_seeds: Vec<PathBuf>,
}

/// Walk every seed in `paths`, returning the discovered files in a stable
/// order plus any seeds that did not exist.
///
/// Each seed may be a file (emitted directly; the include allow-list is
/// matched on its basename, the exclude deny-set does not apply — an
/// explicitly named file is a direct request, #726) or a directory
/// (walked with gitignore awareness when `respect_gitignore` is true).
/// Files are de-duplicated across seeds while preserving first-seen
/// order, so overlapping seeds do not double-analyse a file. A seed that
/// does not exist (or whose symlink dangles) is collected into
/// [`WalkOutcome::missing_seeds`] rather than silently skipped (#858);
/// existence/kind is classified via [`seed_kind`] (`symlink_metadata`), not
/// `Path::is_file`, so a dangling symlink is detected instead of being
/// misrouted into the directory walk where it yields nothing.
pub(crate) fn walk_paths(
    paths: &[PathBuf],
    filters: &WalkFilters,
    respect_gitignore: bool,
) -> WalkOutcome {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut missing: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for seed in paths {
        let Ok(is_file) = seed_kind(seed) else {
            // A nonexistent / dangling-symlink seed is a caller error, not
            // an empty directory — surface it (#858 / #596 parity).
            // De-duplicate against `seen` like the file path does, so a
            // repeated bad seed yields a single AnalysisFailure, not one
            // per occurrence (a path is never both a valid file and a
            // missing seed, so sharing `seen` is safe).
            if seen.insert(seed.clone()) {
                missing.push(seed.clone());
            }
            continue;
        };
        if is_file {
            let name = seed.file_name().map_or(seed.as_path(), Path::new);
            if filters.includes(name) && seen.insert(seed.clone()) {
                out.push(seed.clone());
            }
            continue;
        }
        walk_seed(seed, filters, respect_gitignore, &mut out, &mut seen);
    }
    WalkOutcome {
        files: out,
        missing_seeds: missing,
    }
}

/// Walk a single seed, appending its discovered files to `out`.
fn walk_seed(
    root: &Path,
    filters: &WalkFilters,
    respect_gitignore: bool,
    out: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    // `WalkBuilder` rooted at the seed: only existing directory seeds reach
    // here. File seeds are emitted directly by `walk_paths`, and a
    // nonexistent / dangling-symlink seed is classified by `seed_kind`
    // up-front and routed to `WalkOutcome::missing_seeds` (#858), so it
    // never enters this directory walk.
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
        let outcome = walk_paths(&[root.to_path_buf()], f, true);
        assert!(
            outcome.missing_seeds.is_empty(),
            "an existing directory seed must not be reported missing",
        );
        let mut found = outcome.files;
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
            kept.files,
            vec![vendored.clone()],
            "an explicitly named file must bypass the exclude deny-set (#726)"
        );

        let narrowed = walk_paths(
            std::slice::from_ref(&vendored),
            &filters(&["*.py"], &[]),
            true,
        );
        assert!(
            narrowed.files.is_empty(),
            "the include allow-list must still narrow an explicit file seed"
        );
        let included = walk_paths(
            std::slice::from_ref(&vendored),
            &filters(&["*.rs"], &[]),
            true,
        );
        assert_eq!(
            included.files,
            vec![vendored],
            "a basename include must accept a matching explicit file seed"
        );
    }

    #[test]
    fn nonexistent_seed_is_reported_missing_not_silently_dropped() {
        // #858 / #596 parity: a nonexistent seed must not vanish — the
        // pre-fix `is_file()` classification routed it into the directory
        // walk, where `WalkBuilder` yielded one skipped `Err` and the seed
        // produced no file *and* no missing-seed entry, indistinguishable
        // from an empty directory.
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist.rs");
        let outcome = walk_paths(std::slice::from_ref(&missing), &filters(&[], &[]), true);
        assert!(
            outcome.files.is_empty(),
            "a nonexistent seed discovers no files"
        );
        assert_eq!(
            outcome.missing_seeds,
            vec![missing],
            "a nonexistent seed must be reported, not silently dropped (#858)"
        );
    }

    #[test]
    fn valid_and_missing_seeds_are_partitioned() {
        // A mix of one valid directory seed and one nonexistent seed: the
        // valid seed's files are still discovered, and the bad seed is
        // surfaced (not silently dropped).
        let dir = tempfile::tempdir().expect("tempdir");
        make_tree(dir.path());
        let missing = dir.path().join("typo");
        let outcome = walk_paths(
            &[dir.path().to_path_buf(), missing.clone()],
            &filters(&[], &[]),
            true,
        );
        let mut files = outcome.files;
        files.sort();
        assert_eq!(
            files,
            vec![
                dir.path().join("src/keep.rs"),
                dir.path().join("vendor/drop.rs"),
            ],
            "the valid seed's files are still discovered alongside a bad seed"
        );
        assert_eq!(
            outcome.missing_seeds,
            vec![missing],
            "the nonexistent seed is surfaced even when another seed is valid"
        );
    }

    #[test]
    fn repeated_missing_seed_is_reported_once() {
        // A nonexistent seed listed twice must yield a single failure, the
        // same way `files` de-duplicates a repeated valid seed — otherwise a
        // duplicated bad path produces duplicate AnalysisFailure records.
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("typo.rs");
        let outcome = walk_paths(
            &[missing.clone(), missing.clone()],
            &filters(&[], &[]),
            true,
        );
        assert_eq!(
            outcome.missing_seeds,
            vec![missing],
            "a repeated missing seed must be reported once, not per occurrence"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_seed_is_reported_missing() {
        // A dangling symlink seed must be detected via `symlink_metadata`,
        // not `is_file()` (which follows the link and returns false,
        // misrouting it into the directory walk that yields nothing). This
        // is the asymmetry #596/#704 fixed on the CLI side.
        let dir = tempfile::tempdir().expect("tempdir");
        let link = dir.path().join("dangling.rs");
        std::os::unix::fs::symlink(dir.path().join("nowhere.rs"), &link)
            .expect("create dangling symlink");
        let outcome = walk_paths(std::slice::from_ref(&link), &filters(&[], &[]), true);
        assert!(
            outcome.files.is_empty(),
            "a dangling symlink yields no file"
        );
        assert_eq!(
            outcome.missing_seeds,
            vec![link],
            "a dangling-symlink seed must be reported missing, not dropped"
        );
    }
}
