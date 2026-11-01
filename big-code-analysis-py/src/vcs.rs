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

use big_code_analysis::vcs::{
    self, AuthorHashKey, CacheConfig, Options, build_history_index_cached, build_trend, hotspot,
    parse_timestamp, parse_window, score_commit, score_diff, workdir_root,
};
use big_code_analysis::wire::{self, VcsReport, VcsReportFile};
use big_code_analysis::{FuncSpace, Metric};

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
/// file in that repo — and this attaches the matching block, with a
/// hotspot score from the subtree cyclomatic sum. A file with no index
/// entry (untracked / binary) is left untouched.
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
    let canonical = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());
    if let Some(stat) = index.get_for_path(&canonical) {
        attach_vcs_to_space(space, stat.clone(), HotspotComplexity::SubtreeSum);
    }
}

/// Which cyclomatic figure a space's `hotspot_score` is scored from. The
/// choice follows the space's position in the tree, as the CLI's
/// `vcs_command::inject` / `assign_child_stats` make it: the file (root)
/// space uses the subtree rollup so the file-level score matches `bca
/// vcs`, and every descendant — container or function — uses its own
/// figure so nested complexity is not re-counted at each enclosing level
/// (#709, #1624).
#[derive(Clone, Copy, Debug)]
enum HotspotComplexity {
    /// `cyclomatic.sum`: the space plus everything nested in it.
    SubtreeSum,
    /// `cyclomatic.value`: the space's own complexity.
    Own,
}

impl HotspotComplexity {
    fn of(self, space: &FuncSpace) -> u64 {
        match self {
            Self::SubtreeSum => space.metrics.cyclomatic.cyclomatic_sum(),
            Self::Own => space.metrics.cyclomatic.cyclomatic(),
        }
    }
}

/// Fill `stat.hotspot_score` from the space's `complexity` figure and set
/// it as the space's `metrics.vcs`. Shared by the file-level
/// [`inject_vcs_with_index`] and the per-function [`assign_child_stats`]
/// so the two paths compute the hotspot identically and cannot drift
/// (mirrors the CLI's `set_hotspot_score`).
///
/// One deliberate difference from the CLI: a space whose cyclomatic was
/// not computed (excluded via `metrics=`) keeps `hotspot_score` unset
/// rather than scoring from the zeroed figure. `selected` is the set the
/// serializer consults to elide a metric, so this is exactly "the
/// serialized metrics carry no `cyclomatic`".
fn attach_vcs_to_space(space: &mut FuncSpace, mut stat: vcs::Stats, complexity: HotspotComplexity) {
    if space.metrics.selected.contains(Metric::Cyclomatic) {
        // Cyclomatic counts stay far below 2^53, so the cast is exact.
        #[allow(clippy::cast_precision_loss)]
        let figure = complexity.of(space) as f64;
        stat.hotspot_score = Some(hotspot::hotspot_score(figure, stat.churn_recent));
    }
    space.metrics.vcs = Some(stat);
}

/// Attach per-function `vcs` blocks to every nested function / method /
/// class space of a file using a pre-built blame session (issue #329 /
/// #578).
///
/// Backs both `analyze(..., vcs_per_function=True)` (single file) and the
/// batch `vcs_per_function=True` path (#670): the caller opens the blame
/// engine off-GIL (#620) — one per containing repository for the batch
/// path, reused across every file in that repo — and this blames the file
/// **once** and attaches one block per descendant space (with a hotspot
/// score from that space's own cyclomatic, not its subtree sum).
///
/// The file-level (root) space is left untouched here — it carries the
/// whole-file block that [`inject_vcs_with_index`] attaches when
/// `vcs=True` is also set. A file with no nested spaces, or an unblameable
/// file (untracked, outside the work tree, deleted at the target ref), is
/// left unchanged, mirroring the CLI's graceful degradation (`bca metrics
/// --vcs-per-function` never aborts the walk on one bad file).
pub(crate) fn inject_vcs_per_function_with_blame(
    space: &mut FuncSpace,
    file_path: &Path,
    blame: &mut vcs::BlameSession,
) {
    // Pre-order over descendants (the root is the file space). The same
    // traversal collects the spans and, after the single blame, replays in
    // lockstep to attach each returned `Stats`, so spans and stats line up
    // one-to-one (mirrors the CLI's `collect_child_spans` /
    // `assign_child_stats`).
    let mut spans = Vec::new();
    collect_child_spans(space, &mut spans);
    if spans.is_empty() {
        return;
    }

    // Blame the file exactly once and reuse the result across every span —
    // re-blaming per function would be an O(n) perf bug. A blame failure
    // leaves the per-function blocks unset (the file still emits its AST
    // metrics), so one unblameable file is silent rather than fatal —
    // matching the CLI's per-file skip.
    let Ok(stats) = blame.per_function(file_path, &spans) else {
        return;
    };

    // `per_function` returns exactly one `Stats` per span, and
    // `assign_child_stats` replays the identical pre-order, so the iterator
    // must drain fully; a length mismatch or leftover means the two
    // traversals drifted out of lockstep (mirrors the CLI's debug_asserts in
    // `vcs_command::inject_per_function`).
    debug_assert_eq!(stats.len(), spans.len());
    let mut stats = stats.into_iter();
    assign_child_stats(space, &mut stats);
    debug_assert!(
        stats.next().is_none(),
        "per-function VCS stats outnumbered the spaces they attach to"
    );
}

/// Collect the 1-based inclusive line span of every descendant space, in
/// pre-order. Saturates a span line past `u32::MAX` (no real source file
/// reaches that line count) rather than wrapping. Mirrors the CLI's
/// `vcs_command::collect_child_spans`.
///
/// Walks with an explicit stack of child iterators rather than recursing:
/// this runs before serialization, which is where the space depth is
/// bounded (`MAX_SPACE_SERIALIZE_DEPTH`), so a recursive walk would
/// overflow the thread stack — an abort, not a catchable error — on
/// nesting the serializer would have refused with a `ValueError`.
fn collect_child_spans(space: &FuncSpace, out: &mut Vec<vcs::LineSpan>) {
    let mut pending = vec![space.spaces.iter()];
    while let Some(children) = pending.last_mut() {
        let Some(child) = children.next() else {
            pending.pop();
            continue;
        };
        let start = u32::try_from(child.start_line).unwrap_or(u32::MAX);
        let end = u32::try_from(child.end_line).unwrap_or(u32::MAX);
        out.push(vcs::LineSpan::new(start, end));
        pending.push(child.spaces.iter());
    }
}

/// Replay the [`collect_child_spans`] pre-order, attaching one blame
/// `vcs::Stats` to each descendant space and filling its per-function
/// `hotspot_score` from that space's own cyclomatic. Mirrors the CLI's
/// `vcs_command::assign_child_stats`, and walks iteratively for the
/// reason [`collect_child_spans`] gives.
fn assign_child_stats(space: &mut FuncSpace, stats: &mut impl Iterator<Item = vcs::Stats>) {
    let mut pending = vec![space.spaces.iter_mut()];
    while let Some(children) = pending.last_mut() {
        let Some(child) = children.next() else {
            pending.pop();
            continue;
        };
        if let Some(stat) = stats.next() {
            attach_vcs_to_space(child, stat, HotspotComplexity::Own);
        }
        pending.push(child.spaces.iter_mut());
    }
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
    use big_code_analysis::{LANG, MetricsOptions, Source, analyze};

    /// `outer` own 2 (one `if`), `inner` own 3 (two `if`s), so `outer`'s
    /// subtree sum is 5 and the file's is 1 + 2 + 3 = 6. Hand-derived;
    /// the setup asserts below hold the analysis to it.
    const NESTED: &[u8] = b"def outer(x):\n    if x:\n        pass\n    def inner(y):\n        if y:\n            pass\n        if y > 1:\n            pass\n";

    fn nested_space(options: MetricsOptions) -> FuncSpace {
        analyze(Source::new(LANG::Python, NESTED), options).expect("the fixture parses")
    }

    fn stats_with_churn(churn_recent: u64) -> vcs::Stats {
        vcs::Stats {
            churn_recent,
            ..vcs::Stats::default()
        }
    }

    fn hotspot_of(space: &FuncSpace) -> Option<f64> {
        space.metrics.vcs.as_ref().and_then(|vcs| vcs.hotspot_score)
    }

    /// #1624: every descendant scores from its own cyclomatic and
    /// `SubtreeSum` reads the subtree sum, matching the CLI's
    /// `assign_child_stats` / `inject`. `outer`'s sum differs from its own
    /// figure, so a descendant path scoring by sum moves its score. The
    /// root is attached here directly, because `inject_vcs_with_index`
    /// needs a history index; that call site's choice of `SubtreeSum` is
    /// pinned by `test_analyze_vcs_per_function_hotspot_matches_cli` in
    /// `tests/test_vcs.py`.
    #[test]
    fn descendants_score_their_own_value_and_subtree_sum_reads_sum() {
        let mut root = nested_space(MetricsOptions::default());
        let outer = &root.spaces[0];
        assert_eq!(
            (
                outer.metrics.cyclomatic.cyclomatic(),
                outer.metrics.cyclomatic.cyclomatic_sum()
            ),
            (2, 5)
        );
        assert_eq!(root.metrics.cyclomatic.cyclomatic_sum(), 6);

        attach_vcs_to_space(
            &mut root,
            stats_with_churn(10),
            HotspotComplexity::SubtreeSum,
        );
        let mut stats = [stats_with_churn(3), stats_with_churn(5)].into_iter();
        assign_child_stats(&mut root, &mut stats);
        assert!(stats.next().is_none(), "one stat per descendant");

        assert_eq!(hotspot_of(&root), Some(60.0), "root: sum 6 x churn 10");
        let outer = &root.spaces[0];
        assert_eq!(
            hotspot_of(outer),
            Some(6.0),
            "outer: own 2 x churn 3, not sum 5"
        );
        assert_eq!(
            hotspot_of(&outer.spaces[0]),
            Some(15.0),
            "inner: own 3 x churn 5"
        );
    }

    /// A space whose cyclomatic was not computed (excluded via `metrics=`)
    /// still gets its block, with no hotspot (#1742 tracks the CLI, which
    /// scores the zeroed figure instead).
    #[test]
    fn space_without_cyclomatic_gets_a_block_without_a_hotspot() {
        let mut space = nested_space(MetricsOptions::default().with_only(&[Metric::Loc]));
        assert!(!space.metrics.selected.contains(Metric::Cyclomatic));
        attach_vcs_to_space(&mut space, stats_with_churn(7), HotspotComplexity::Own);
        let vcs = space.metrics.vcs.as_ref().expect("the block is attached");
        assert_eq!(vcs.churn_recent, 7);
        assert_eq!(hotspot_of(&space), None);
    }

    /// The spans the single blame is asked for are every descendant's, in
    /// the pre-order `assign_child_stats` replays.
    #[test]
    fn child_spans_are_every_descendant_in_pre_order() {
        let space = nested_space(MetricsOptions::default());
        let mut spans = Vec::new();
        collect_child_spans(&space, &mut spans);
        assert_eq!(spans, [vcs::LineSpan::new(1, 8), vcs::LineSpan::new(4, 8)]);
    }

    /// Both injector walks run before serialization bounds the space depth
    /// at `MAX_SPACE_SERIALIZE_DEPTH`, so nesting far past it must reach
    /// the serializer's catchable error rather than abort on the stack
    /// (#1768). The chain is built directly, as `src/wire.rs`'s
    /// `space_chain` does, because analysing this depth in a debug build
    /// is slow.
    #[test]
    fn injector_walks_survive_nesting_past_the_serialize_bound() {
        const DEPTH: usize = 10_000;
        /// Far below a production thread's stack, so a walk that recurses
        /// per level fails here rather than riding on the test runner's.
        const TIGHT_STACK: usize = 128 * 1024;

        let file = analyze(
            Source::new(LANG::Python, b"def f():\n    pass\n"),
            MetricsOptions::default().with_only(&[Metric::Loc]),
        )
        .expect("the fixture parses");
        let leaf = file.spaces[0].clone();
        let mut root = file;
        let mut cursor = &mut root.spaces[0];
        for _ in 1..DEPTH {
            cursor.spaces.push(leaf.clone());
            cursor = &mut cursor.spaces[0];
        }

        let (spans, attached) = std::thread::Builder::new()
            .stack_size(TIGHT_STACK)
            .spawn(move || {
                let mut spans = Vec::new();
                collect_child_spans(&root, &mut spans);
                let mut stats = std::iter::repeat_with(vcs::Stats::default).take(spans.len());
                assign_child_stats(&mut root, &mut stats);
                let mut attached = 0;
                let mut cursor = &root;
                while let Some(child) = cursor.spaces.first() {
                    attached += usize::from(child.metrics.vcs.is_some());
                    cursor = child;
                }
                (spans.len(), attached)
            })
            .expect("spawn bounded-stack thread")
            .join()
            .expect("the injector walks must not overflow the stack");
        assert_eq!((spans, attached), (DEPTH, DEPTH));
    }
}
