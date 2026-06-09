// bca: suppress-file(halstead, nargs, exit)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// option-builder + handler `?` error maps), not per-function logic
// complexity (cognitive/cyclomatic stay enforced).

//! `POST /vcs` — change-history (VCS) metrics over a server-side git
//! working tree (issue #328).
//!
//! Unlike the source-in-body endpoints, this one analyses a repository
//! already present on the server's filesystem (`repo_path`): VCS
//! metrics derive from commit history, which has no in-request
//! representation. The handler runs one history walk and returns the
//! files ranked by composite risk score.
//!
//! # Security
//!
//! `repo_path` is a **server-side filesystem path**, so this endpoint
//! lets the caller make the server walk any git repository it can read
//! and returns that repo's relative file paths, churn, and author
//! signals. This is materially different from every other endpoint
//! (which only ever sees code in the request body). Operators must not
//! expose `/vcs` to untrusted clients without an authorization layer;
//! the default `127.0.0.1` bind keeps it local. The walk runs under the
//! same parse-timeout / blocking-pool guard as the other endpoints.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use big_code_analysis::vcs::{
    self, CacheConfig, JitDiffReport, JitReport, Options, build_history_index_cached, build_trend,
    parse_timestamp, parse_window, score_commit, score_diff,
};
use big_code_analysis::wire;

/// Request body for `POST /vcs`.
#[derive(Debug, Deserialize)]
pub struct WebVcsPayload {
    /// Request identifier echoed back in the response.
    pub id: String,
    /// Server-side path to (a directory inside) the git working tree.
    pub repo_path: String,
    /// Long window (default `12mo`).
    pub long_window: Option<String>,
    /// Recent window (default `90d`).
    pub recent_window: Option<String>,
    /// Show only the top N files by risk (`0` / absent = all).
    pub top: Option<usize>,
    /// Revision to analyse (default `HEAD`).
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Composite formula: `weighted` (default) or `percentile`.
    pub risk_formula: Option<String>,
    /// File-type scope (issue #576): `metrics` (default — only files bca
    /// has metrics for), `all` (every tracked text file), or a
    /// comma-separated extension allow-list (`rs,py`).
    pub file_types: Option<String>,
    /// Walk the full DAG rather than first-parent only.
    pub full_history: Option<bool>,
    /// Include merge commits.
    pub include_merges: Option<bool>,
    /// Follow renames (default true).
    pub follow_renames: Option<bool>,
    /// Exclude bot identities (default true).
    pub exclude_bots: Option<bool>,
    /// Override the bot-author exclusion regex (matched against the
    /// author name/email). Parity with the CLI `--bot-pattern` and the
    /// Python `vcs_metrics(bot_pattern=…)`.
    pub bot_pattern: Option<String>,
    /// Reference "now" (RFC 3339 / `@unix` / git date) for snapshots.
    pub as_of: Option<String>,
    /// Emit SHA-256-hashed author identities.
    pub emit_author_details: Option<bool>,
    /// Include files deleted at the target ref.
    pub include_deleted: Option<bool>,
    /// Bus-factor coverage (abandonment) threshold in `(0, 1)` (issue
    /// #332); default `0.5` per Avelino.
    pub bus_factor_threshold: Option<f64>,
    /// Disable the persistent change-history cache for this request
    /// (issue #334). Default `false` — the cache reuses prior work on an
    /// unchanged tree and walks only new commits when `HEAD` advances.
    pub no_cache: Option<bool>,
    /// Override the server-side cache directory. Defaults to the platform
    /// cache location (`$XDG_CACHE_HOME/big-code-analysis/vcs`, etc.).
    pub cache_dir: Option<String>,
}

/// One ranked file: repo-relative path plus the flat VCS block.
#[derive(Debug, Serialize)]
pub struct WebVcsFileEntry {
    /// Repository-relative path.
    pub path: String,
    /// The file's change-history metrics.
    #[serde(flatten)]
    pub vcs: wire::Vcs,
}

/// Response body for `POST /vcs`.
#[derive(Debug, Serialize)]
pub struct WebVcsResponse {
    /// Echoed request identifier.
    pub id: String,
    /// Long window length, in days.
    pub long_window_days: u32,
    /// Recent window length, in days.
    pub recent_window_days: u32,
    /// Whether the repository is a shallow clone (truncated history).
    pub truncated_shallow_clone: bool,
    /// Directory- / repo-level bus factor (issue #332).
    pub vcs_aggregate: Option<vcs::VcsAggregate>,
    /// Files ranked by descending risk score.
    pub files: Vec<WebVcsFileEntry>,
}

/// Translate a payload into backend [`Options`], surfacing a typed
/// [`vcs::Error`] on a bad window / timestamp / formula.
fn options_from(payload: &WebVcsPayload) -> Result<Options, vcs::Error> {
    let mut options = Options::default();
    if let Some(spec) = &payload.long_window {
        options.long_window_secs = parse_window(spec)?;
    }
    if let Some(spec) = &payload.recent_window {
        options.recent_window_secs = parse_window(spec)?;
    }
    if let Some(reference) = &payload.reference {
        options.reference.clone_from(reference);
    }
    if let Some(formula) = &payload.risk_formula {
        options.risk_formula = formula.parse()?;
    }
    if let Some(scope) = &payload.file_types {
        options.file_types = scope.parse()?;
    }
    if let Some(raw) = &payload.as_of {
        options.as_of = Some(parse_timestamp(raw)?);
    }
    options.full_history = payload.full_history.unwrap_or(options.full_history);
    options.include_merges = payload.include_merges.unwrap_or(options.include_merges);
    options.follow_renames = payload.follow_renames.unwrap_or(options.follow_renames);
    options.exclude_bots = payload.exclude_bots.unwrap_or(options.exclude_bots);
    if let Some(pattern) = &payload.bot_pattern {
        options.bot_pattern.clone_from(pattern);
    }
    options.emit_author_details = payload
        .emit_author_details
        .unwrap_or(options.emit_author_details);
    options.include_deleted = payload.include_deleted.unwrap_or(options.include_deleted);
    // The endpoint always surfaces the aggregate; validate the threshold
    // up front so a bad value is a clean 4xx, not a clamped surprise.
    options.compute_bus_factor = true;
    if let Some(threshold) = payload.bus_factor_threshold {
        options.bus_factor_threshold = vcs::options::validate_bus_factor_threshold(threshold)?;
    }
    Ok(options)
}

/// Run the history walk for `payload` and rank the result.
///
/// # Errors
///
/// Returns a [`vcs::Error`] for a bad option, a non-repository
/// `repo_path`, or a history-walk failure; the handler maps it to the
/// appropriate HTTP status.
pub fn compute_vcs(payload: WebVcsPayload) -> Result<WebVcsResponse, vcs::Error> {
    let options = options_from(&payload)?;
    let mut config = CacheConfig::default();
    config.enabled = !payload.no_cache.unwrap_or(false);
    config.dir = payload.cache_dir.as_ref().map(PathBuf::from);
    let index = build_history_index_cached(&PathBuf::from(&payload.repo_path), &options, &config)?;

    let mut files: Vec<WebVcsFileEntry> = index
        .iter()
        .filter_map(|(rel, stat)| {
            rel.to_str().map(|path| WebVcsFileEntry {
                path: path.to_owned(),
                vcs: wire::Vcs::from(stat),
            })
        })
        .collect();
    vcs::rank_by_risk(&mut files, payload.top.unwrap_or(0), |e| {
        (e.path.as_str(), e.vcs.risk_score)
    });

    Ok(WebVcsResponse {
        id: payload.id,
        long_window_days: options.long_window_days(),
        recent_window_days: options.recent_window_days(),
        truncated_shallow_clone: index.truncated_shallow_clone(),
        vcs_aggregate: index.vcs_aggregate(),
        files,
    })
}

/// Request body for `POST /vcs/jit` (issues #331 / #580). Scores a single
/// commit on a server-side repository, or — when `diff` is supplied — an
/// arbitrary unified diff carried in the request body (no repository
/// needed). The two are mutually exclusive.
#[derive(Debug, Deserialize)]
pub struct WebVcsJitPayload {
    /// Request identifier echoed back in the response.
    pub id: String,
    /// Server-side path to (a directory inside) the git working tree.
    /// Required for commit scoring; ignored (and may be omitted) in
    /// diff-only mode.
    pub repo_path: Option<String>,
    /// Commit / revision to score (default `HEAD`). Mutually exclusive with
    /// `diff`.
    pub commit: Option<String>,
    /// An arbitrary unified diff to score instead of a commit (issue #580).
    /// Only the size and diffusion groups are computable, so the response
    /// is a partial report whose score is **not comparable** to a commit
    /// score. Mutually exclusive with `repo_path` / `commit`.
    pub diff: Option<String>,
    /// Long window (default `12mo`). Commit mode only.
    pub long_window: Option<String>,
    /// Recent window (default `90d`). Commit mode only.
    pub recent_window: Option<String>,
    /// Walk the full DAG rather than first-parent only. Commit mode only.
    pub full_history: Option<bool>,
    /// Include merge commits in the experience walk. Commit mode only.
    pub include_merges: Option<bool>,
    /// Follow renames (default true). Commit mode only.
    pub follow_renames: Option<bool>,
    /// Reference "now" (RFC 3339 / `@unix` / git date). Commit mode only.
    pub as_of: Option<String>,
}

/// Response body for `POST /vcs/jit`. The echoed `id` plus the flattened
/// report — either a commit [`JitReport`] or a partial diff
/// [`JitDiffReport`]; the report's own `source` field (`"commit"` /
/// `"diff"`) tells them apart. Untagged so the report fields sit at the top
/// level without an extra wrapper key.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum WebVcsJitReport {
    /// A full commit score.
    Commit(JitReport),
    /// A partial diff score (issue #580).
    Diff(JitDiffReport),
}

/// Response body for `POST /vcs/jit`: the echoed id plus the flattened JIT
/// report.
#[derive(Debug, Serialize)]
pub struct WebVcsJitResponse {
    /// Echoed request identifier.
    pub id: String,
    /// The JIT report (commit or diff mode).
    #[serde(flatten)]
    pub report: WebVcsJitReport,
}

/// Translate the JIT payload's commit-mode knobs into backend [`Options`].
/// The full ranking knob set does not apply here (a single commit is
/// scored, not a file ranking), so only the windows / history / rename /
/// as-of options are honored.
fn jit_options_from(payload: &WebVcsJitPayload) -> Result<Options, vcs::Error> {
    let mut options = Options::default();
    if let Some(spec) = &payload.long_window {
        options.long_window_secs = parse_window(spec)?;
    }
    if let Some(spec) = &payload.recent_window {
        options.recent_window_secs = parse_window(spec)?;
    }
    if let Some(raw) = &payload.as_of {
        options.as_of = Some(parse_timestamp(raw)?);
    }
    options.full_history = payload.full_history.unwrap_or(options.full_history);
    options.include_merges = payload.include_merges.unwrap_or(options.include_merges);
    options.follow_renames = payload.follow_renames.unwrap_or(options.follow_renames);
    Ok(options)
}

/// Score a commit or a diff for `payload` and return the JIT report.
///
/// # Errors
///
/// Returns a [`vcs::Error`] for a bad option, a missing `repo_path` in
/// commit mode, a non-repository `repo_path`, an unresolvable commit, a
/// malformed diff, or a history-walk failure; the handler maps it to the
/// appropriate HTTP status.
pub fn compute_vcs_jit(payload: WebVcsJitPayload) -> Result<WebVcsJitResponse, vcs::Error> {
    let report = if let Some(diff) = &payload.diff {
        WebVcsJitReport::Diff(score_diff(diff)?)
    } else {
        let options = jit_options_from(&payload)?;
        // A commit score needs a repository; a missing `repo_path` is a
        // client mistake, surfaced as the same not-a-repository 400 as a
        // path that exists but is not a working tree.
        let repo_path = payload
            .repo_path
            .as_ref()
            .ok_or_else(|| vcs::Error::NotARepository(PathBuf::from("")))?;
        let spec = payload.commit.as_deref().unwrap_or("HEAD");
        WebVcsJitReport::Commit(score_commit(&PathBuf::from(repo_path), spec, &options)?)
    };
    Ok(WebVcsJitResponse {
        id: payload.id,
        report,
    })
}

/// Request body for `POST /vcs/trend` (issue #333). The base fields are
/// the same as [`WebVcsPayload`] — `top` selects how many files the series
/// keeps, `as_of` anchors the most-recent point — plus the trend-only
/// `points` / `span` / `top_deltas`.
#[derive(Debug, Deserialize)]
pub struct WebVcsTrendPayload {
    /// All the shared `/vcs` knobs (`id`, `repo_path`, windows, `ref`, …).
    #[serde(flatten)]
    pub base: WebVcsPayload,
    /// Number of evenly-spaced sample points (>= 2).
    pub points: usize,
    /// Total look-back span the points cover (default `12mo`).
    pub span: Option<String>,
    /// Top N files per improving / regressing delta list (`0` / absent =
    /// all).
    pub top_deltas: Option<usize>,
}

/// Response body for `POST /vcs/trend`: the echoed id plus the flattened
/// [`wire::VcsTrend`] time series.
#[derive(Debug, Serialize)]
pub struct WebVcsTrendResponse {
    /// Echoed request identifier.
    pub id: String,
    /// The historical metric trend.
    #[serde(flatten)]
    pub trend: wire::VcsTrend,
}

/// Sample the change-history metrics across time for `payload` and return
/// the time series.
///
/// # Errors
///
/// Returns a [`vcs::Error`] for a bad option, an out-of-range point count,
/// a non-repository `repo_path`, or a history-walk failure; the handler
/// maps it to the appropriate HTTP status.
pub fn compute_vcs_trend(payload: WebVcsTrendPayload) -> Result<WebVcsTrendResponse, vcs::Error> {
    let options = options_from(&payload.base)?;
    let span_secs = parse_window(
        payload
            .span
            .as_deref()
            .unwrap_or(vcs::options::DEFAULT_LONG_WINDOW),
    )?;
    let trend = build_trend(
        &PathBuf::from(&payload.base.repo_path),
        &options,
        payload.points,
        span_secs,
    )?;
    let wire_trend = wire::VcsTrend::from_trend(
        &trend,
        payload.base.top.unwrap_or(0),
        payload.top_deltas.unwrap_or(0),
    );
    Ok(WebVcsTrendResponse {
        id: payload.base.id,
        trend: wire_trend,
    })
}
