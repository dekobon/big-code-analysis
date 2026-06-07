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

use big_code_analysis::vcs::{self, Options, build_history_index, parse_timestamp, parse_window};
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
    /// Walk the full DAG rather than first-parent only.
    pub full_history: Option<bool>,
    /// Include merge commits.
    pub include_merges: Option<bool>,
    /// Follow renames (default true).
    pub follow_renames: Option<bool>,
    /// Exclude bot identities (default true).
    pub exclude_bots: Option<bool>,
    /// Reference "now" (RFC 3339 / `@unix` / git date) for snapshots.
    pub as_of: Option<String>,
    /// Emit SHA-256-hashed author identities.
    pub emit_author_details: Option<bool>,
    /// Include files deleted at the target ref.
    pub include_deleted: Option<bool>,
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
    if let Some(raw) = &payload.as_of {
        options.as_of = Some(parse_timestamp(raw)?);
    }
    options.full_history = payload.full_history.unwrap_or(options.full_history);
    options.include_merges = payload.include_merges.unwrap_or(options.include_merges);
    options.follow_renames = payload.follow_renames.unwrap_or(options.follow_renames);
    options.exclude_bots = payload.exclude_bots.unwrap_or(options.exclude_bots);
    options.emit_author_details = payload
        .emit_author_details
        .unwrap_or(options.emit_author_details);
    options.include_deleted = payload.include_deleted.unwrap_or(options.include_deleted);
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
    let index = build_history_index(&PathBuf::from(&payload.repo_path), &options)?;

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
        files,
    })
}
