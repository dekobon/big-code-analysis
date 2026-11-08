//! Bridge for the change-history (VCS) metrics surface exposed to
//! Python under the `big_code_analysis.vcs` facade (issue #612).
//!
//! The standalone reports back the `vcs.rank` / `trend` / `commit` /
//! `score_diff` entry points — [`vcs_report_json`] backs `vcs.rank()`,
//! [`vcs_trend_json`] backs `vcs.trend()`, [`vcs_commit_json`] backs
//! `vcs.commit()`, and [`vcs_score_diff_json`] backs `vcs.score_diff()`.
//! [`inject_vcs_with_index`] and [`inject_vcs_per_function_with_blame`]
//! back the `analyze(..., vcs=True)` / `vcs_per_function=True` opt-ins,
//! with the batch helpers ([`build_index_for`], [`open_blame_for`],
//! [`workdir_root_for`]) reusing one history index across a tree walk.
//!
//! The reports produce JSON strings and the injectors mutate the
//! `FuncSpace` before it is serialized; both reach Python through the same
//! `conversion::json_string_to_py` boundary as the AST entry points, so
//! the Python side sees ordinary dicts.

use std::path::{Path, PathBuf};

use pyo3::PyErr;
use pyo3::exceptions::PyValueError;

use big_code_analysis::FuncSpace;
use big_code_analysis::vcs::{
    self, AuthorHashKey, CacheConfig, Options, attach, build_history_index_cached, build_trend,
    parse_timestamp, parse_window, score_commit, score_diff, workdir_root,
};
use big_code_analysis::wire::{self, VcsReport, VcsReportFile};

/// Knobs accepted from Python, all optional (Python defaults map here).
// The booleans mirror independent `bca vcs` CLI toggles; a flags
// newtype would obscure each at the call site for no gain.
#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
pub(crate) struct VcsParams {
    pub long_window: Option<String>,
    pub recent_window: Option<String>,
    pub top: Option<usize>,
    pub reference: Option<String>,
    pub risk_formula: Option<String>,
    /// File-type scope: `"metrics"` (default), `"all"`, or a
    /// comma-separated extension allow-list (`"rs,py"`). `None` keeps the
    /// `metrics` default (issue #576).
    pub file_types: Option<String>,
    pub full_history: bool,
    pub include_merges: bool,
    pub follow_renames: bool,
    pub exclude_bots: bool,
    pub bot_pattern: Option<String>,
    pub as_of: Option<String>,
    pub emit_author_details: bool,
    /// Secret key that hardens `emit_author_details` into a keyed
    /// HMAC-SHA256 (issue #956). `None` emits the bare SHA-256 pseudonym.
    /// Requires `emit_author_details`.
    ///
    /// SECURITY: holds the raw secret. If a `Debug` derive is ever added to
    /// this struct, do not whole-struct debug-log it — it is moved into the
    /// redacting `AuthorHashKey` newtype in `options_from`.
    pub author_hash_key: Option<String>,
    pub include_deleted: bool,
    pub bus_factor_threshold: Option<f64>,
    /// Disable the persistent change-history cache for this call (issue
    /// #334). Default `false` (cache enabled).
    pub no_cache: bool,
    /// Override the cache directory; `None` uses the platform default.
    pub cache_dir: Option<std::path::PathBuf>,
}

impl VcsParams {
    /// The persistent-cache configuration these params imply.
    fn cache_config(&self) -> CacheConfig {
        let mut config = CacheConfig::default();
        config.enabled = !self.no_cache;
        config.dir.clone_from(&self.cache_dir);
        config
    }
}

/// Build [`Options`] from Python params, surfacing a `ValueError` on a
/// bad window / timestamp / formula.
fn options_from(params: &VcsParams) -> Result<Options, PyErr> {
    let mut options = Options::default();
    options.full_history = params.full_history;
    options.include_merges = params.include_merges;
    options.follow_renames = params.follow_renames;
    options.exclude_bots = params.exclude_bots;
    options.emit_author_details = params.emit_author_details;
    if let Some(key) = &params.author_hash_key {
        // The key only hardens emitted digests, so reject it without
        // `emit_author_details` rather than silently doing nothing.
        if !params.emit_author_details {
            return Err(vcs_error_to_py(vcs::Error::InvalidAuthorHashKey(
                "author_hash_key requires emit_author_details".to_owned(),
            )));
        }
        options.author_hash_key =
            Some(AuthorHashKey::new(key.clone().into_bytes()).map_err(vcs_error_to_py)?);
    }
    options.include_deleted = params.include_deleted;
    if let Some(spec) = &params.long_window {
        options.long_window_secs = parse_window(spec).map_err(vcs_error_to_py)?;
    }
    if let Some(spec) = &params.recent_window {
        options.recent_window_secs = parse_window(spec).map_err(vcs_error_to_py)?;
    }
    if let Some(reference) = &params.reference {
        options.reference.clone_from(reference);
    }
    if let Some(pattern) = &params.bot_pattern {
        options.bot_pattern.clone_from(pattern);
    }
    if let Some(raw) = &params.as_of {
        options.as_of = Some(parse_timestamp(raw).map_err(vcs_error_to_py)?);
    }
    if let Some(formula) = &params.risk_formula {
        options.risk_formula = formula.parse().map_err(vcs_error_to_py)?;
    }
    if let Some(scope) = &params.file_types {
        options.file_types = scope.parse().map_err(vcs_error_to_py)?;
    }
    // `vcs.rank()` always returns the directory/repo bus factor.
    options.compute_bus_factor = true;
    if let Some(threshold) = params.bus_factor_threshold {
        options.bus_factor_threshold =
            vcs::options::validate_bus_factor_threshold(threshold).map_err(vcs_error_to_py)?;
    }
    Ok(options)
}

/// Walk `repo_path`'s history and return the ranked report as a JSON
/// string for [`crate::conversion::json_string_to_py`].
///
/// # Errors
///
/// `ValueError` for a bad option or a non-repository path; the walk
/// itself surfaces its failure the same way.
pub(crate) fn vcs_report_json(repo_path: &Path, params: &VcsParams) -> Result<String, PyErr> {
    let options = options_from(params)?;
    let index = build_history_index_cached(repo_path, &options, &params.cache_config())
        .map_err(vcs_error_to_py)?;

    let mut files: Vec<VcsReportFile> = index
        .iter()
        .filter_map(|(rel, stat)| {
            rel.to_str().map(|path| VcsReportFile {
                path: path.to_owned(),
                vcs: wire::Vcs::from(stat),
            })
        })
        .collect();
    vcs::rank_by_risk(&mut files, params.top.unwrap_or(0), |e| {
        (e.path.as_str(), e.vcs.risk_score)
    });

    let report = VcsReport {
        long_window_days: options.long_window_days(),
        recent_window_days: options.recent_window_days(),
        risk_score_version: vcs::score::RISK_SCORE_VERSION,
        vcs_schema_version: vcs::stats::VCS_SCHEMA_VERSION,
        truncated_shallow_clone: index.truncated_shallow_clone(),
        vcs_aggregate: index.vcs_aggregate(),
        files,
    };
    serde_json::to_string(&report)
        .map_err(|e| PyValueError::new_err(format!("serializing vcs report: {e}")))
}

/// Walk `repo_path`'s history at several points in time and return the
/// historical metric trend (issue #333) as a JSON string for
/// [`crate::conversion::json_string_to_py`].
///
/// `params` supplies the shared window / bot / merge / rename / ref / as-of
/// knobs (its `top` caps how many files the series keeps); `points` and
/// `span` define the sampling grid, and `top_deltas` trims each delta
/// list.
///
/// # Errors
///
/// `ValueError` for a bad option, an out-of-range point count, or a
/// non-repository path; the walk itself surfaces its failure the same way.
pub(crate) fn vcs_trend_json(
    repo_path: &Path,
    params: &VcsParams,
    points: usize,
    span: Option<&str>,
    top_deltas: Option<usize>,
) -> Result<String, PyErr> {
    let options = options_from(params)?;
    let span_secs =
        parse_window(span.unwrap_or(vcs::options::DEFAULT_LONG_WINDOW)).map_err(vcs_error_to_py)?;
    let trend = build_trend(repo_path, &options, points, span_secs).map_err(vcs_error_to_py)?;
    let wire_trend =
        wire::VcsTrend::from_trend(&trend, params.top.unwrap_or(0), top_deltas.unwrap_or(0));
    serde_json::to_string(&wire_trend)
        .map_err(|e| PyValueError::new_err(format!("serializing vcs trend: {e}")))
}

/// The commit-mode knobs accepted by [`vcs_jit_json`]; all optional. The
/// full ranking-knob set does not apply when scoring a single commit, so
/// only the windows / history / rename / as-of options are honored.
#[derive(Default)]
pub(crate) struct JitParams {
    pub long_window: Option<String>,
    pub recent_window: Option<String>,
    pub full_history: bool,
    pub include_merges: bool,
    pub follow_renames: bool,
    pub as_of: Option<String>,
}

impl JitParams {
    /// Build [`Options`] for a commit score, surfacing a `ValueError` on a
    /// bad window / timestamp.
    fn options(&self) -> Result<Options, PyErr> {
        let mut options = Options::default();
        options.full_history = self.full_history;
        options.include_merges = self.include_merges;
        options.follow_renames = self.follow_renames;
        if let Some(spec) = &self.long_window {
            options.long_window_secs = parse_window(spec).map_err(vcs_error_to_py)?;
        }
        if let Some(spec) = &self.recent_window {
            options.recent_window_secs = parse_window(spec).map_err(vcs_error_to_py)?;
        }
        if let Some(raw) = &self.as_of {
            options.as_of = Some(parse_timestamp(raw).map_err(vcs_error_to_py)?);
        }
        Ok(options)
    }
}

/// Score a single commit for just-in-time risk (issue #331), returning the
/// commit JIT report as a JSON string for
/// [`crate::conversion::json_string_to_py`]. Backs `vcs.commit()` (#612 /
/// #667).
///
/// `source` is `"commit"`; the report carries all five feature groups
/// (size / diffusion / history / experience / purpose) and a `risk_score`
/// comparable across commits.
///
/// # Errors
///
/// `ValueError` for a bad option, a non-repository `repo_path`, or an
/// unresolvable commit.
pub(crate) fn vcs_commit_json(
    repo_path: &Path,
    commit: &str,
    params: &JitParams,
) -> Result<String, PyErr> {
    let options = params.options()?;
    let report = score_commit(repo_path, commit, &options).map_err(vcs_error_to_py)?;
    serde_json::to_string(&report)
        .map_err(|e| PyValueError::new_err(format!("serializing jit report: {e}")))
}

/// Score an arbitrary unified diff for partial just-in-time risk (issue
/// #580), returning the diff JIT report as a JSON string for
/// [`crate::conversion::json_string_to_py`]. Backs `vcs.score_diff()`
/// (#612 / #667).
///
/// A bare diff has no author / parent / history, so only the size and
/// diffusion groups are computable: `source` is `"diff"` and
/// `partial_risk_score` is **not comparable** to a commit `risk_score`.
///
/// # Errors
///
/// `ValueError` for a malformed diff.
pub(crate) fn vcs_score_diff_json(diff: &str) -> Result<String, PyErr> {
    let report = score_diff(diff).map_err(vcs_error_to_py)?;
    serde_json::to_string(&report)
        .map_err(|e| PyValueError::new_err(format!("serializing jit diff report: {e}")))
}

/// The directory to discover the repository from for `file_path`: its
/// parent, or the current directory for a bare filename. `Path::parent()`
/// returns `Some("")` (an empty path) — not `None` — for `"foo.rs"`, so an
/// `unwrap_or(".")` alone would discover from an empty path and silently
/// find no repository; map that empty parent to `.`.
pub(crate) fn repo_root_for(file_path: &Path) -> &Path {
    match file_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Resolve the repository work-tree root that contains `file_path`, for
/// the batch VCS cache key (#670). Files in different subdirectories of one
/// checkout resolve to the **same** root, so the batch cache builds one
/// index / blame engine per repository rather than one per directory.
/// Returns `None` when `file_path` is not inside a repository, so the cache
/// falls back to the file's parent directory and degrades gracefully.
pub(crate) fn workdir_root_for(file_path: &Path) -> Option<PathBuf> {
    workdir_root(file_path)
}

/// Build a default-options [`HistoryIndex`] for the repository containing
/// `root`, for the batch `vcs=True` path (#670). Returns `None` (rather
/// than an error) when `root` is not inside a repository, so a batch over
/// a mix of in-repo and out-of-repo files degrades gracefully per file.
pub(crate) fn build_index_for(root: &Path) -> Option<vcs::HistoryIndex> {
    build_history_index_cached(root, &Options::default(), &CacheConfig::default()).ok()
}

/// Open a default-options [`PerFunctionBlame`] engine for the repository
/// containing `root` and return a blame session on it, for the
/// `vcs_per_function=True` paths (#670). Returns `None` when `root` is not
/// inside a repository.
///
/// A session rather than the bare engine because the batch path blames
/// every file in one repository through it: the repository handle, its
/// object cache, the `.mailmap`, and the resolved-commit memo are then
/// built once per repository instead of once per file (#1117). The
/// single-file path opens (and drops) one session for its one blame,
/// which costs the same as the bare engine did.
pub(crate) fn open_blame_for(root: &Path) -> Option<vcs::BlameSession> {
    vcs::PerFunctionBlame::open(root, Options::default())
        .ok()
        .map(|engine| std::sync::Arc::new(engine).session())
}

/// Attach a file-level `vcs` block to `space` from a pre-built
/// [`vcs::HistoryIndex`].
///
/// Backs both `analyze(..., vcs=True)` (single file) and the batch
/// `vcs=True` path (#670): the caller builds the index off-GIL (#620) —
/// one per containing repository for the batch path, reused across every
/// file in that repo — and this attaches the matching block through the
/// library's [`attach::attach_file_history`], which owns the hotspot rule
/// (the subtree cyclomatic sum, or no score when cyclomatic was not
/// computed; #1743) and the path canonicalization (#1764). A file with no
/// index entry (untracked / binary) is left untouched.
///
/// The block goes on the [`FuncSpace`] before it is serialized, as the
/// CLI's `vcs_command::inject` does, not into its JSON afterwards. A
/// `serde_json::Value` round trip sorts every object's keys (the
/// workspace leaves `preserve_order` off), so the earlier JSON injector
/// broke the byte-for-byte parity with `bca metrics -O json` that
/// [`crate::conversion`] documents whenever a block was attached, and
/// paid a parse and a re-serialization per injector besides.
pub(crate) fn inject_vcs_with_index(
    space: &mut FuncSpace,
    file_path: &Path,
    index: &vcs::HistoryIndex,
) {
    attach::attach_file_history(space, file_path, index);
}

/// Attach per-function `vcs` blocks to every nested function / method /
/// class space of a file using a pre-built blame session (issue #329 /
/// #578).
///
/// Backs both `analyze(..., vcs_per_function=True)` (single file) and the
/// batch `vcs_per_function=True` path (#670): the caller opens the blame
/// engine off-GIL (#620) — one per containing repository for the batch
/// path, reused across every file in that repo — and the library's
/// [`attach::attach_per_function_history`] blames the file **once** and
/// attaches one block per descendant space, scored from that space's own
/// cyclomatic (#709, #1743).
///
/// The file-level (root) space is left untouched here — it carries the
/// whole-file block that [`inject_vcs_with_index`] attaches when
/// `vcs=True` is also set. A file with no nested spaces, or an unblameable
/// file (untracked, outside the work tree, deleted at the target ref), is
/// left unchanged: the blame error is dropped here, so one bad file is
/// silent rather than fatal, mirroring the CLI's per-file skip (which
/// warns instead — each front end decides).
pub(crate) fn inject_vcs_per_function_with_blame(
    space: &mut FuncSpace,
    file_path: &Path,
    blame: &mut vcs::BlameSession,
) {
    let _ = attach::attach_per_function_history(space, file_path, blame);
}

/// Map a [`vcs::Error`] to the matching Python exception (#624).
///
/// Every variant maps to a subclass of `VcsError`, which subclasses
/// `ValueError`, so a pre-existing `except ValueError` (or
/// `except VcsError`) handler still catches all of them. The three
/// named subclasses (`NotARepositoryError`, `InvalidRevisionError`,
/// `InvalidDiffError`) carve out the triggers a caller most plausibly
/// branches on; the remaining client-input option failures collapse to
/// the `VcsError` base, and environment / backend failures map to
/// `VcsEnvironmentError`. The client-input vs environment split comes
/// straight from [`vcs::Error::is_client_input`] (#641) — the same
/// classification the web crate maps to `400` vs `500` — so this
/// mapping cannot drift from it: a future variant inherits its
/// upstream classification automatically and only needs an arm here
/// if it deserves its own named subclass.
// Taken by value so it composes directly with `Result::map_err`
// (`.map_err(vcs_error_to_py)`); `to_string` only borrows it.
#[allow(clippy::needless_pass_by_value)]
fn vcs_error_to_py(error: vcs::Error) -> PyErr {
    let message = error.to_string();
    if !error.is_client_input() {
        return crate::VcsEnvironmentError::new_err(message);
    }
    match error {
        vcs::Error::NotARepository(_) => crate::NotARepositoryError::new_err(message),
        vcs::Error::ResolveRef { .. } => crate::InvalidRevisionError::new_err(message),
        vcs::Error::InvalidDiff(_) => crate::InvalidDiffError::new_err(message),
        // Every other client-input failure shares the `VcsError` base —
        // a caller that wants to distinguish them reads the message,
        // exactly as before this change.
        _ => crate::VcsError::new_err(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_code_analysis::vcs::hotspot;
    use big_code_analysis::{LANG, Metric, MetricsOptions, Source, analyze};

    /// `outer` own 2 (one `if`), `inner` own 3 (two `if`s), so `outer`'s
    /// subtree sum is 5 and the file's is 1 + 2 + 3 = 6. The rule that
    /// scores a hotspot from those figures is pinned in the library's
    /// `vcs::attach` tests (#1743); the two tests here pin only that the
    /// bindings' wrappers reach it.
    const NESTED: &[u8] = b"def outer(x):\n    if x:\n        pass\n    def inner(y):\n        if y:\n            pass\n        if y > 1:\n            pass\n";

    fn nested_space(options: MetricsOptions) -> FuncSpace {
        analyze(Source::new(LANG::Python, NESTED), options).expect("the fixture parses")
    }

    fn hotspot_of(space: &FuncSpace) -> Option<f64> {
        space.metrics.vcs.as_ref().and_then(|vcs| vcs.hotspot_score)
    }

    /// Run `git <args>` in `dir`, isolated from the host. Every `GIT_*`
    /// variable the test process inherited goes first: git exports
    /// `GIT_DIR` and `GIT_INDEX_FILE` to its hooks, and `GIT_DIR` to
    /// `git rebase --exec` in a linked worktree, and inherited they
    /// outrank `current_dir`, so the fixture's `init`, `add` and
    /// `commit` wrote into the checkout running the test (#1846). Then
    /// the host's configuration is kept away: no global file (which
    /// also covers `$XDG_CONFIG_HOME/git/config`), no system file, and
    /// the two per-user files that are not configuration —
    /// `core.excludesFile` and `core.attributesFile` default to
    /// `$XDG_CONFIG_HOME/git/…` and are read whatever the first two say
    /// — pinned to `/dev/null` (#1771). Mirrors the CLI's
    /// `isolated_git_env` and the root crate's
    /// `tests/common/vcs_fixture.rs`, neither of which a unit test here
    /// can reach.
    fn git(dir: &Path, args: &[&str]) {
        let status = fixture_git()
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_COUNT", "2")
            .env("GIT_CONFIG_KEY_0", "core.excludesFile")
            .env("GIT_CONFIG_VALUE_0", "/dev/null")
            .env("GIT_CONFIG_KEY_1", "core.attributesFile")
            .env("GIT_CONFIG_VALUE_1", "/dev/null")
            .status()
            .expect("spawn git");
        assert!(status.success(), "git {args:?} failed");
    }

    /// A `git` with every `GIT_*` variable the test process inherited
    /// removed; see [`git`].
    fn fixture_git() -> std::process::Command {
        let mut cmd = std::process::Command::new("git");
        for (key, _) in std::env::vars_os() {
            if is_git_var(&key) {
                cmd.env_remove(key);
            }
        }
        cmd
    }

    /// Whether `key` names a `GIT_*` variable. Byte-wise on the encoded
    /// name, so a non-UTF-8 name is still read; ASCII-case-insensitive
    /// on Windows, where environment names are.
    fn is_git_var(key: &std::ffi::OsStr) -> bool {
        let bytes = key.as_encoded_bytes();
        let Some(prefix) = bytes.get(..4) else {
            return false;
        };
        if cfg!(windows) {
            prefix.eq_ignore_ascii_case(b"GIT_")
        } else {
            prefix == b"GIT_"
        }
    }

    /// A throwaway repository holding `work.py` (the [`NESTED`] fixture)
    /// in one commit, at its canonical path: `tempdir` itself may sit
    /// under a symlink (macOS `/var` → `/private/var`), and the index
    /// keys on the canonical work tree.
    fn one_commit_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().canonicalize().expect("canonical tempdir");
        std::fs::write(repo.join("work.py"), NESTED).expect("write work.py");
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        git(&repo, &["add", "work.py"]);
        git(&repo, &["commit", "-q", "-m", "add work.py"]);
        (dir, repo)
    }

    /// `inject_vcs_with_index` drives `attach::attach_file_history`: the
    /// tracked file's block arrives scored from the file's subtree sum
    /// (6 × churn), and an untracked file stays bare.
    #[test]
    fn file_level_wrapper_attaches_through_the_library() {
        let (_dir, repo) = one_commit_repo();
        // The persistent cache is off so a unit test writes nothing under
        // the user's cache directory; the index is otherwise the one
        // `build_index_for` builds.
        let mut no_cache = CacheConfig::default();
        no_cache.enabled = false;
        let index = build_history_index_cached(&repo, &Options::default(), &no_cache)
            .expect("the scratch repository indexes");

        let mut space = nested_space(MetricsOptions::default());
        assert_eq!(space.metrics.cyclomatic.cyclomatic_sum(), 6);
        inject_vcs_with_index(&mut space, &repo.join("work.py"), &index);
        let vcs = space
            .metrics
            .vcs
            .as_ref()
            .expect("the tracked file attaches");
        assert_eq!(vcs.commits_long, 1);
        assert_eq!(
            hotspot_of(&space),
            Some(hotspot::hotspot_score(6.0, vcs.churn_recent)),
            "scored from the file's subtree sum"
        );

        let mut untracked = nested_space(MetricsOptions::default());
        std::fs::write(repo.join("untracked.py"), NESTED).expect("write untracked.py");
        inject_vcs_with_index(&mut untracked, &repo.join("untracked.py"), &index);
        assert!(untracked.metrics.vcs.is_none(), "untracked stays bare");
    }

    /// `inject_vcs_per_function_with_blame` drives
    /// `attach::attach_per_function_history`: both descendants receive a
    /// block scored from their *own* cyclomatic (2 and 3, not `outer`'s
    /// subtree 5), the root is left to the file-level wrapper, and a
    /// space whose cyclomatic was not computed gets a block with no
    /// score. An unblameable file is skipped silently.
    #[test]
    fn per_function_wrapper_attaches_through_the_library() {
        let (_dir, repo) = one_commit_repo();
        let file = repo.join("work.py");
        let mut blame = open_blame_for(&repo).expect("the scratch repository opens");

        let mut space = nested_space(MetricsOptions::default());
        inject_vcs_per_function_with_blame(&mut space, &file, &mut blame);
        assert!(space.metrics.vcs.is_none(), "the root is the file block's");
        let outer = &space.spaces[0];
        let inner = &outer.spaces[0];
        let churn = |space: &FuncSpace| space.metrics.vcs.as_ref().map(|v| v.churn_recent);
        assert_eq!(
            (hotspot_of(outer), hotspot_of(inner)),
            (
                churn(outer).map(|c| hotspot::hotspot_score(2.0, c)),
                churn(inner).map(|c| hotspot::hotspot_score(3.0, c)),
            ),
            "each descendant is scored from its own cyclomatic"
        );
        assert!(
            churn(outer).is_some() && churn(inner).is_some(),
            "both descendants received a block"
        );

        let mut unscored = nested_space(MetricsOptions::default().with_only(&[Metric::Loc]));
        inject_vcs_per_function_with_blame(&mut unscored, &file, &mut blame);
        let outer = &unscored.spaces[0];
        assert!(outer.metrics.vcs.is_some(), "the block is attached");
        assert_eq!(hotspot_of(outer), None, "no cyclomatic, no score");

        let mut untracked = nested_space(MetricsOptions::default());
        std::fs::write(repo.join("untracked.py"), NESTED).expect("write untracked.py");
        inject_vcs_per_function_with_blame(&mut untracked, &repo.join("untracked.py"), &mut blame);
        assert!(
            untracked.spaces[0].metrics.vcs.is_none(),
            "an unblameable file is skipped silently"
        );
    }

    /// The fixture alone, read back through its own `git`: one commit on
    /// `main`, `work.py` tracked. The target the child-process tests
    /// below re-run.
    #[test]
    fn one_commit_repo_builds_its_repo() {
        let (_dir, repo) = one_commit_repo();
        let read = |args: &[&str]| {
            let out = fixture_git()
                .args(args)
                .current_dir(&repo)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .expect("spawn git");
            assert!(out.status.success(), "git {args:?} failed");
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        assert_eq!(read(&["rev-list", "--count", "HEAD"]), "1");
        assert_eq!(read(&["ls-files"]), "work.py");
        assert_eq!(read(&["log", "-1", "--format=%an %D"]), "Ada HEAD -> main");
    }

    /// The `git` that builds and reads the victim repository: its own
    /// removal loop over the inherited `GIT_*` variables, so a defect in
    /// [`fixture_git`]'s loop cannot reach it (the two share only the
    /// `is_git_var` prefix test), and the host's configuration kept away,
    /// the per-user ignore and attributes files included, since those are
    /// read whatever the global and system variables say and a host
    /// `XDG_CONFIG_HOME/git/ignore` of `*` would fail the victim's `git
    /// add` before the child under test ever ran.
    #[cfg(unix)]
    fn clean_git(dir: &Path, args: &[&str]) -> String {
        let mut cmd = std::process::Command::new("git");
        for (key, _) in std::env::vars_os() {
            if is_git_var(&key) {
                cmd.env_remove(key);
            }
        }
        let out = cmd
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@x",
                "-c",
                "core.excludesFile=/dev/null",
                "-c",
                "core.attributesFile=/dev/null",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Re-runs [`one_commit_repo_builds_its_repo`] in a child test binary
    /// with `vars` exported, and asserts that it passed. A test cannot
    /// set its own process's environment without `unsafe`, and the
    /// fixture reads it through `Command`, so the variables are set on a
    /// child instead. The name is module-qualified: libtest's `--exact`
    /// matches the full path.
    #[cfg(unix)]
    fn rerun_fixture_test_with(vars: &[(&str, PathBuf)]) {
        let child = format!(
            "{}::one_commit_repo_builds_its_repo",
            module_path!()
                .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
                .expect("the test module is nested under the crate root")
        );
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", &child, "--format", "pretty"])
            .envs(vars.iter().map(|(k, v)| (k, v)))
            .output()
            .expect("spawn the child");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("test result: ok. 1 passed"),
            "{child} must pass with {vars:?} inherited; stdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A hook, and `git rebase --exec` in a linked worktree, export
    /// `GIT_DIR` and `GIT_INDEX_FILE`, and both outrank `current_dir`
    /// (#1846). The fixture test is re-run in a child with that pair
    /// naming another repository: the child must pass, and that
    /// repository must come through untouched. Verified by revert:
    /// without the scrub in `fixture_git` the child's `init`, `add` and
    /// `commit` land in the victim — its HEAD moves and its index lists
    /// `work.py`.
    #[cfg(unix)]
    #[test]
    fn fixture_git_ignores_an_inherited_repository() {
        let victim = tempfile::tempdir().expect("tempdir");
        std::fs::write(victim.path().join("keep.py"), "def keep():\n    pass\n").expect("write");
        clean_git(victim.path(), &["init", "-q", "-b", "main"]);
        clean_git(victim.path(), &["add", "keep.py"]);
        clean_git(victim.path(), &["commit", "-q", "-m", "victim"]);
        let state = || {
            (
                clean_git(victim.path(), &["rev-parse", "HEAD"]),
                clean_git(victim.path(), &["ls-files", "-s"]),
            )
        };
        let before = state();
        assert!(before.1.contains("keep.py"), "the victim tracks keep.py");
        let git_dir = victim.path().join(".git");
        rerun_fixture_test_with(&[
            ("GIT_DIR", git_dir.clone()),
            ("GIT_INDEX_FILE", git_dir.join("index")),
        ]);
        assert_eq!(state(), before, "the inherited repository was written to");
    }

    /// `GIT_OBJECT_DIRECTORY`, which a `receive-pack` quarantine hook
    /// exports, replaces the object store git reads; once the quarantine
    /// it names is gone, discovery itself fails with `not a git
    /// repository`. The fixture test is re-run in a child with it naming
    /// a directory whose parent does not exist either: `git init`
    /// creates one missing level of an object directory, so a path one
    /// level down would let the unscrubbed fixture succeed. Kept apart
    /// from the repository case above: a fixture that failed to start
    /// would leave that case's victim untouched for the wrong reason.
    /// Verified by revert: without the scrub the child's `git init`
    /// fails.
    #[cfg(unix)]
    #[test]
    fn fixture_git_ignores_an_inherited_object_directory() {
        let gone = tempfile::tempdir().expect("tempdir");
        rerun_fixture_test_with(&[(
            "GIT_OBJECT_DIRECTORY",
            gone.path().join("gone").join("quarantine"),
        )]);
        // Pins the path, not only the child's success: a git that created
        // both levels, or a scrub that dropped the variable only after
        // `init`, would pass the rerun and leave the directory behind.
        assert!(
            !gone.path().join("gone").exists(),
            "the fixture's git used the inherited object directory"
        );
    }

    /// `is_git_var` reads the name's first four bytes, so a short name, a
    /// name that merely starts `GIT` (`GITHUB_TOKEN`) and an empty one are
    /// not git's; a lowercase spelling is on Windows alone, where
    /// environment names are case-insensitive. Host-independent, so the
    /// Windows branch runs there while the child reruns stay unix-only.
    #[test]
    fn is_git_var_reads_the_prefix_as_the_host_does() {
        use std::ffi::OsStr;
        assert!(is_git_var(OsStr::new("GIT_DIR")));
        assert!(is_git_var(OsStr::new("GIT_")));
        assert!(!is_git_var(OsStr::new("GITHUB_TOKEN")));
        assert!(!is_git_var(OsStr::new("GIT")));
        assert!(!is_git_var(OsStr::new("")));
        assert_eq!(is_git_var(OsStr::new("git_dir")), cfg!(windows));
    }
}
