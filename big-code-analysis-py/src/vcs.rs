// bca: suppress-file(halstead, nargs, nexits)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// option-builder + report/inject `?` error maps and the many-field
// report assembly), not per-function logic complexity
// (cognitive/cyclomatic stay enforced).

//! Bridge for the change-history (VCS) metrics surface exposed to
//! Python: the standalone [`vcs_report_json`] (backing `vcs_metrics()`)
//! and [`inject_vcs`] (backing the `analyze(..., vcs=True)` opt-in).
//!
//! Both produce / mutate JSON strings, reusing the same
//! `conversion::json_string_to_py` boundary as the AST entry points, so
//! the Python side sees ordinary dicts.

use std::path::Path;

use pyo3::PyErr;
use pyo3::exceptions::PyValueError;
use serde::Serialize;
use serde_json::Value;

use big_code_analysis::vcs::{
    self, CacheConfig, Options, build_history_index_cached, build_trend, hotspot, parse_timestamp,
    parse_window, score_commit, score_diff,
};
use big_code_analysis::wire;

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

/// One ranked file: repo-relative path plus the flat VCS block.
#[derive(Serialize)]
struct FileEntry {
    path: String,
    #[serde(flatten)]
    vcs: wire::Vcs,
}

/// The serialized report shape (matches the CLI / web report).
#[derive(Serialize)]
struct Report {
    long_window_days: u32,
    recent_window_days: u32,
    risk_score_version: u32,
    vcs_schema_version: u32,
    truncated_shallow_clone: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    vcs_aggregate: Option<vcs::VcsAggregate>,
    files: Vec<FileEntry>,
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
    // `vcs_metrics()` always returns the directory/repo bus factor.
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

    let mut files: Vec<FileEntry> = index
        .iter()
        .filter_map(|(rel, stat)| {
            rel.to_str().map(|path| FileEntry {
                path: path.to_owned(),
                vcs: wire::Vcs::from(stat),
            })
        })
        .collect();
    vcs::rank_by_risk(&mut files, params.top.unwrap_or(0), |e| {
        (e.path.as_str(), e.vcs.risk_score)
    });

    let report = Report {
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

/// Score a single commit (issue #331), or — when `diff` is supplied — an
/// arbitrary unified diff (issue #580), returning the JIT report as a JSON
/// string for [`crate::conversion::json_string_to_py`].
///
/// In diff mode only the size and diffusion groups are computable, so the
/// returned report's `source` is `"diff"` and its `partial_risk_score` is
/// **not comparable** to a commit score. `repo_path` / `commit` / the window
/// knobs are ignored in diff mode.
///
/// # Errors
///
/// `ValueError` for a bad option, a non-repository `repo_path`, an
/// unresolvable commit, or a malformed diff.
pub(crate) fn vcs_jit_json(
    repo_path: Option<&Path>,
    commit: &str,
    diff: Option<&str>,
    params: &JitParams,
) -> Result<String, PyErr> {
    if let Some(diff) = diff {
        let report = score_diff(diff).map_err(vcs_error_to_py)?;
        return serde_json::to_string(&report)
            .map_err(|e| PyValueError::new_err(format!("serializing jit diff report: {e}")));
    }
    let root = repo_path.ok_or_else(|| {
        PyValueError::new_err("vcs_jit requires repo_path when no diff is supplied")
    })?;
    let options = params.options()?;
    let report = score_commit(root, commit, &options).map_err(vcs_error_to_py)?;
    serde_json::to_string(&report)
        .map_err(|e| PyValueError::new_err(format!("serializing jit report: {e}")))
}

/// The directory to discover the repository from for `file_path`: its
/// parent, or the current directory for a bare filename. `Path::parent()`
/// returns `Some("")` (an empty path) — not `None` — for `"foo.rs"`, so an
/// `unwrap_or(".")` alone would discover from an empty path and silently
/// find no repository; map that empty parent to `.`.
fn repo_root_for(file_path: &Path) -> &Path {
    match file_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Inject a `vcs` block into a single file's metrics JSON for
/// `analyze(..., vcs=True)`. Builds a one-shot index for the file's
/// repository, attaches the matching block (and a hotspot score from
/// the cyclomatic sum already present in the JSON), and returns the
/// rewritten JSON. A file with no index entry (untracked / binary) or
/// outside any repository is returned unchanged.
pub(crate) fn inject_vcs(funcspace_json: String, file_path: &Path) -> Result<String, PyErr> {
    let root = repo_root_for(file_path);
    // Discovery failures (not a repo) are non-fatal here: `analyze` still
    // returns the AST metrics, just without a `vcs` block.
    let Ok(index) = build_history_index_cached(root, &Options::default(), &CacheConfig::default())
    else {
        return Ok(funcspace_json);
    };
    let canonical = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_path_buf());
    let Some(stat) = index.get_for_path(&canonical) else {
        return Ok(funcspace_json);
    };

    let mut doc: Value = serde_json::from_str(&funcspace_json)
        .map_err(|e| PyValueError::new_err(format!("parsing metrics JSON: {e}")))?;
    let mut wire_vcs = wire::Vcs::from(stat);
    attach_vcs_to_space(&mut doc, &mut wire_vcs)?;
    serde_json::to_string(&doc)
        .map_err(|e| PyValueError::new_err(format!("reserializing metrics JSON: {e}")))
}

/// Fill `wire_vcs.hotspot_score` from the space's own cyclomatic sum and
/// insert the serialized block under the space's `metrics.vcs`. Shared by
/// the file-level [`inject_vcs`] and the per-function [`attach_vcs_block`]
/// so the two paths compute the hotspot and serialize the block
/// identically and cannot drift (mirrors the CLI's `set_hotspot_score`).
///
/// The hotspot needs the cyclomatic sum already in the serialized metrics
/// (present when cyclomatic was computed); a space without it keeps
/// `hotspot_score` unset, exactly like the CLI when the sum is zero.
fn attach_vcs_to_space(space: &mut Value, wire_vcs: &mut wire::Vcs) -> Result<(), PyErr> {
    if let Some(sum) = space
        .get("metrics")
        .and_then(|m| m.get("cyclomatic"))
        .and_then(|c| c.get("sum"))
        .and_then(Value::as_f64)
    {
        wire_vcs.hotspot_score = Some(hotspot::hotspot_score(sum, wire_vcs.churn_recent));
    }
    let vcs_value = serde_json::to_value(&*wire_vcs)
        .map_err(|e| PyValueError::new_err(format!("serializing vcs block: {e}")))?;
    if let Some(metrics) = space.get_mut("metrics").and_then(Value::as_object_mut) {
        metrics.insert("vcs".to_owned(), vcs_value);
    }
    Ok(())
}

/// Inject a per-function `vcs` block into every nested function / method /
/// class space of a file's metrics JSON for
/// `analyze(..., vcs_per_function=True)` (issue #329 / #578). Opens a
/// `git blame` engine for the file's repository, blames the file **once**,
/// and attaches one block per descendant space (with a hotspot score from
/// that space's own cyclomatic sum), returning the rewritten JSON.
///
/// The file-level (root) space is left untouched here — it carries the
/// whole-file block that [`inject_vcs`] attaches when `vcs=True` is also
/// set. A file outside any repository, an unblameable file (untracked,
/// outside the work tree, deleted at the target ref), or a file with no
/// nested spaces is returned unchanged, mirroring the CLI's graceful
/// degradation (`bca metrics --vcs-per-function` never aborts the walk on
/// one bad file).
pub(crate) fn inject_vcs_per_function(
    funcspace_json: String,
    file_path: &Path,
) -> Result<String, PyErr> {
    let root = repo_root_for(file_path);
    // Discovery failures (not a repo) are non-fatal: `analyze` still
    // returns the AST metrics, just without per-function `vcs` blocks.
    let Ok(blame) = vcs::PerFunctionBlame::open(root, Options::default()) else {
        return Ok(funcspace_json);
    };

    let mut doc: Value = serde_json::from_str(&funcspace_json)
        .map_err(|e| PyValueError::new_err(format!("parsing metrics JSON: {e}")))?;

    // Pre-order over descendants (the root is the file space). The same
    // traversal collects the spans and, after the single blame, replays in
    // lockstep to attach each returned `Stats`, so spans and stats line up
    // one-to-one (mirrors the CLI's `collect_child_spans` /
    // `assign_child_stats`).
    let mut spans = Vec::new();
    collect_child_spans(&doc, &mut spans);
    if spans.is_empty() {
        return Ok(funcspace_json);
    }

    // Blame the file exactly once and reuse the result across every span —
    // re-blaming per function would be an O(n) perf bug. A blame failure
    // leaves the per-function blocks unset (the file still emits its AST
    // metrics), so one unblameable file is silent rather than fatal —
    // matching the CLI's per-file skip.
    let Ok(stats) = blame.per_function(file_path, &spans) else {
        return Ok(funcspace_json);
    };

    // `per_function` returns exactly one `Stats` per span, and
    // `assign_child_stats` replays the identical pre-order, so the iterator
    // must drain fully; a length mismatch or leftover means the two
    // traversals drifted out of lockstep (mirrors the CLI's debug_asserts in
    // `vcs_command::inject_per_function`).
    debug_assert_eq!(stats.len(), spans.len());
    let mut stats = stats.into_iter();
    assign_child_stats(&mut doc, &mut stats)?;
    debug_assert!(
        stats.next().is_none(),
        "per-function VCS stats outnumbered the spaces they attach to"
    );
    serde_json::to_string(&doc)
        .map_err(|e| PyValueError::new_err(format!("reserializing metrics JSON: {e}")))
}

/// Collect the 1-based inclusive line span of every descendant space, in
/// pre-order, reading `start_line` / `end_line` from the JSON tree.
/// Saturates a span line past `u32::MAX` (no real source file reaches that
/// line count) rather than wrapping. Mirrors the CLI's
/// `vcs_command::collect_child_spans`, which walks `FuncSpace` structs;
/// here the same shape is read from the serialized JSON.
fn collect_child_spans(node: &Value, out: &mut Vec<vcs::LineSpan>) {
    let Some(children) = node.get("spaces").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        let start = json_line(child, "start_line");
        let end = json_line(child, "end_line");
        out.push(vcs::LineSpan::new(start, end));
        collect_child_spans(child, out);
    }
}

/// Read a 1-based line number field from a space's JSON, saturating to
/// `u32::MAX` (the serializer emits these as JSON integers).
fn json_line(node: &Value, field: &str) -> u32 {
    node.get(field)
        .and_then(Value::as_u64)
        .map_or(u32::MAX, |line| u32::try_from(line).unwrap_or(u32::MAX))
}

/// Replay the [`collect_child_spans`] pre-order, attaching one blame
/// `vcs::Stats` to each descendant space's `metrics.vcs` and filling its
/// per-function `hotspot_score` from that space's own cyclomatic sum.
/// Mirrors the CLI's `vcs_command::assign_child_stats`.
fn assign_child_stats(
    node: &mut Value,
    stats: &mut impl Iterator<Item = vcs::Stats>,
) -> Result<(), PyErr> {
    let Some(children) = node.get_mut("spaces").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for child in children {
        if let Some(stat) = stats.next() {
            attach_vcs_block(child, &stat)?;
        }
        assign_child_stats(child, stats)?;
    }
    Ok(())
}

/// Serialize one `Stats` (with a hotspot score derived from the space's own
/// cyclomatic sum) into the space's `metrics.vcs`. Mirrors
/// [`inject_vcs`]'s file-level attachment so the per-function block shape is
/// byte-identical to the CLI's.
fn attach_vcs_block(space: &mut Value, stat: &vcs::Stats) -> Result<(), PyErr> {
    let mut wire_vcs = wire::Vcs::from(stat);
    attach_vcs_to_space(space, &mut wire_vcs)
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
