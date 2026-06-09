// bca: suppress-file(halstead, nargs, exit)
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
    self, Options, build_history_index, build_trend, hotspot, parse_timestamp, parse_window,
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
    pub full_history: bool,
    pub include_merges: bool,
    pub follow_renames: bool,
    pub exclude_bots: bool,
    pub bot_pattern: Option<String>,
    pub as_of: Option<String>,
    pub emit_author_details: bool,
    pub include_deleted: bool,
    pub bus_factor_threshold: Option<f64>,
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
    let mut options = Options {
        full_history: params.full_history,
        include_merges: params.include_merges,
        follow_renames: params.follow_renames,
        exclude_bots: params.exclude_bots,
        emit_author_details: params.emit_author_details,
        include_deleted: params.include_deleted,
        ..Options::default()
    };
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
    let index = build_history_index(repo_path, &options).map_err(vcs_error_to_py)?;

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

/// Inject a `vcs` block into a single file's metrics JSON for
/// `analyze(..., vcs=True)`. Builds a one-shot index for the file's
/// repository, attaches the matching block (and a hotspot score from
/// the cyclomatic sum already present in the JSON), and returns the
/// rewritten JSON. A file with no index entry (untracked / binary) or
/// outside any repository is returned unchanged.
pub(crate) fn inject_vcs(funcspace_json: String, file_path: &Path) -> Result<String, PyErr> {
    let root = file_path.parent().unwrap_or(Path::new("."));
    // Discovery failures (not a repo) are non-fatal here: `analyze` still
    // returns the AST metrics, just without a `vcs` block.
    let Ok(index) = build_history_index(root, &Options::default()) else {
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
    // Hotspot needs the file-level cyclomatic sum, which is already in
    // the serialized metrics when cyclomatic was computed.
    if let Some(sum) = doc
        .get("metrics")
        .and_then(|m| m.get("cyclomatic"))
        .and_then(|c| c.get("sum"))
        .and_then(Value::as_f64)
    {
        wire_vcs.hotspot_score = Some(hotspot::hotspot_score(sum, wire_vcs.churn_recent));
    }
    let vcs_value = serde_json::to_value(&wire_vcs)
        .map_err(|e| PyValueError::new_err(format!("serializing vcs block: {e}")))?;
    if let Some(metrics) = doc.get_mut("metrics").and_then(Value::as_object_mut) {
        metrics.insert("vcs".to_owned(), vcs_value);
    }
    serde_json::to_string(&doc)
        .map_err(|e| PyValueError::new_err(format!("reserializing metrics JSON: {e}")))
}

/// Map a [`vcs::Error`] to a Python `ValueError` carrying its message.
// Taken by value so it composes directly with `Result::map_err`
// (`.map_err(vcs_error_to_py)`); `to_string` only borrows it.
#[allow(clippy::needless_pass_by_value)]
fn vcs_error_to_py(error: vcs::Error) -> PyErr {
    PyValueError::new_err(error.to_string())
}
