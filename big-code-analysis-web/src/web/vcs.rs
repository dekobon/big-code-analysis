// bca: suppress-file(halstead, nargs, nexits)
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
//! (which only ever sees code in the request body).
//!
//! The optional `cache_dir` field is a second caller-supplied
//! server-side path, and it grants a *write* capability: when the
//! persistent change-history cache is enabled (the default), the server
//! creates directories and writes JSON cache files under that location
//! (`<cache_dir>/<repo>/<head_sha>.json`). A caller that controls
//! `cache_dir` can therefore direct the server to create directories
//! and write cache files at any path the server process can write to —
//! a strictly larger filesystem reach than the `repo_path` read. Only
//! `/vcs` accepts `cache_dir`; `/vcs/trend` does not use the persistent
//! cache and rejects both cache knobs (issue #961).
//!
//! Together these make the endpoint's filesystem reach an arbitrary
//! read of any readable git repository **and** an arbitrary write of
//! cache files under any writable path. Operators must not expose `/vcs`
//! to untrusted clients without an authorization layer; the default
//! `127.0.0.1` bind keeps it local. The walk runs under the same
//! parse-timeout / blocking-pool guard as the other endpoints.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use big_code_analysis::vcs::{
    self, AuthorHashKey, CacheConfig, JitDiffReport, JitReport, Options,
    build_history_index_cached, build_trend, parse_timestamp, parse_window, score_commit,
    score_diff,
};
use big_code_analysis::wire;

/// Default file-ranking cap when `top` is omitted (#636).
///
/// Matches the CLI's `--top` default (`bca vcs`, 50) so the same logical
/// invocation returns the same-sized ranking regardless of surface. An
/// explicit `top: 0` still means "all files" (#602); the web no longer
/// defaults to the unbounded "all" that risked a serializer self-DoS on a
/// monorepo.
const DEFAULT_TOP: usize = 50;

/// Default per-delta-list cap when `top_deltas` is omitted (#636).
///
/// Matches the CLI's `--top-deltas` default (10). `top_deltas: 0` means
/// "all" per #602.
const DEFAULT_TOP_DELTAS: usize = 10;

/// Default sample-point count when `points` is omitted (#636).
///
/// Matches the CLI's `--points` default (12), which over a 12-month span
/// yields roughly monthly snapshots. Replaces the former hard-required
/// field, so an omitted `points` succeeds instead of 400ing.
const DEFAULT_TREND_POINTS: usize = 12;

/// serde `default` provider for [`WebVcsTrendPayload::points`] (#636).
fn default_trend_points() -> usize {
    DEFAULT_TREND_POINTS
}

/// Request body for `POST /vcs`.
///
/// Unknown fields are rejected with a `400` naming the offending key
/// (#633): on a payload this wide, a typo silently selecting defaults the
/// client did not ask for is worse than a hard error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebVcsPayload {
    /// Request identifier echoed back in the response. Optional on the
    /// wire (#645): an omitted `id` defaults to the empty string, the
    /// "no correlation id" sentinel echoed back unchanged.
    #[serde(default)]
    pub id: String,
    /// Server-side path to (a directory inside) the git working tree.
    pub repo_path: String,
    /// Long window (default `12mo`).
    pub long_window: Option<String>,
    /// Recent window (default `90d`).
    pub recent_window: Option<String>,
    /// Show only the top N files by risk. Absent defaults to 50 (the CLI
    /// `--top` default — #636); an explicit `0` returns all files (#602).
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
    /// Secret key that hardens `emit_author_details` into a keyed
    /// HMAC-SHA256 (issue #956). Requires `emit_author_details`; an empty
    /// key or one without it is a `400`.
    ///
    /// SECURITY: holds the raw secret, and this struct derives `Debug`, so
    /// never whole-struct debug-log a payload (`{payload:?}`) — that would
    /// leak the key into server logs. It is moved into the redacting
    /// `AuthorHashKey` newtype in `options_from`.
    pub author_hash_key: Option<String>,
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
    ///
    /// This is a caller-supplied write path: see the module-level
    /// `# Security` note for the arbitrary-write capability it grants.
    pub cache_dir: Option<String>,
}

/// One ranked file: repo-relative path plus the VCS block, nested under a
/// `vcs` key like every other metric group (issue #684).
#[derive(Debug, Serialize)]
pub struct WebVcsFileEntry {
    /// Repository-relative path.
    pub path: String,
    /// The file's change-history metrics.
    pub vcs: wire::Vcs,
}

/// Response body for `POST /vcs`.
#[derive(Debug, Serialize)]
pub struct WebVcsResponse {
    /// Echoed request identifier.
    pub id: String,
    /// Output-shape version (carried once per response, not per file —
    /// issue #635).
    pub vcs_schema_version: u32,
    /// Composite-formula version (carried once per response — issue #635).
    pub risk_score_version: u32,
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
    if let Some(key) = &payload.author_hash_key {
        // The key only hardens emitted digests, so it is meaningless —
        // and a likely client mistake — without `emit_author_details`.
        if !options.emit_author_details {
            return Err(vcs::Error::InvalidAuthorHashKey(
                "author_hash_key requires emit_author_details".to_owned(),
            ));
        }
        options.author_hash_key = Some(AuthorHashKey::new(key.clone().into_bytes())?);
    }
    options.include_deleted = payload.include_deleted.unwrap_or(options.include_deleted);
    // The endpoint always surfaces the aggregate; validate the threshold
    // up front so a bad value is a clean 4xx, not a clamped surprise.
    options.compute_bus_factor = true;
    if let Some(threshold) = payload.bus_factor_threshold {
        options.bus_factor_threshold = vcs::options::validate_bus_factor_threshold(threshold)?;
    }
    Ok(options)
}

/// Reject a `repo_path` that does not exist on the server filesystem as a
/// client mistake (issue #653).
///
/// A nonexistent path is the most common client error on these endpoints
/// (a typo). `gix::discover` maps a missing directory to an
/// "inaccessible directory" failure that the library classifies as the
/// environment-level [`vcs::Error::OpenRepository`] (a `500`), the same
/// bucket as a corrupt repo or permission denial — so without this check a
/// typo'd path is reported as a server failure while a path that exists but
/// is not a repository correctly answers `400`. Mapping the missing-path
/// case to [`vcs::Error::NotARepository`] makes both bad-path shapes a
/// consistent client-input `400` (`error_kind` `vcs_not_a_repository`),
/// while a path that *does* exist still flows to the backend so a genuine
/// permission / corruption failure keeps its `500`.
///
/// `Path::exists` follows symlinks, so a symlink to a real directory passes
/// and a symlink to a missing target is rejected — the behaviour a client
/// expects for either spelling.
///
/// # Errors
///
/// Returns [`vcs::Error::NotARepository`] naming `repo_path` when it does
/// not resolve to an existing filesystem entry.
fn repo_path_must_exist(repo_path: &str) -> Result<(), vcs::Error> {
    let path = Path::new(repo_path);
    if path.exists() {
        Ok(())
    } else {
        Err(vcs::Error::NotARepository(path.to_path_buf()))
    }
}

/// Run the history walk for `payload` and rank the result.
///
/// # Errors
///
/// Returns a [`vcs::Error`] for a bad option, a non-repository
/// `repo_path`, or a history-walk failure; the handler maps it to the
/// appropriate HTTP status.
pub fn compute_vcs(payload: WebVcsPayload) -> Result<WebVcsResponse, vcs::Error> {
    repo_path_must_exist(&payload.repo_path)?;
    let options = options_from(&payload)?;
    let mut config = CacheConfig::default();
    config.enabled = !payload.no_cache.unwrap_or(false);
    config.dir = payload.cache_dir.as_ref().map(PathBuf::from);
    let index = build_history_index_cached(&PathBuf::from(&payload.repo_path), &options, &config)?;

    let mut files: Vec<WebVcsFileEntry> = index
        .iter()
        .filter_map(|(rel, stat)| {
            let Some(path) = rel.to_str() else {
                // A repo-relative path that is not valid UTF-8 cannot be a
                // JSON string key, so it is dropped from the ranking. Per
                // the no-`to_string_lossy`-as-identifier discipline we never
                // emit a lossy path; instead the drop is signalled in the
                // server log (issue #707) so an operator can see the ranking
                // is incomplete rather than the omission being wholly silent.
                // `display()` is a log-only rendering, never an identifier.
                tracing::warn!(
                    path = %rel.display(),
                    "dropping non-UTF-8 repo-relative path from the VCS ranking"
                );
                return None;
            };
            Some(WebVcsFileEntry {
                path: path.to_owned(),
                vcs: wire::Vcs::from(stat),
            })
        })
        .collect();
    // Absent `top` defaults to the CLI's bounded 50, not the unbounded
    // "all"; an explicit `top: 0` still returns all files (#636 / #602).
    vcs::rank_by_risk(&mut files, payload.top.unwrap_or(DEFAULT_TOP), |e| {
        (e.path.as_str(), e.vcs.risk_score)
    });

    Ok(WebVcsResponse {
        id: payload.id,
        vcs_schema_version: vcs::stats::VCS_SCHEMA_VERSION,
        risk_score_version: vcs::score::RISK_SCORE_VERSION,
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
/// needed). The two modes are mutually exclusive and the combination is
/// **rejected** with a `400` (issue #632): supplying `diff` together with
/// any commit-mode field (`repo_path`, `commit`, or a window / history /
/// rename / `as_of` knob) would silently score the diff and ignore the
/// other fields, answering a different question than the one asked. See
/// [`WebVcsJitPayload::validate`].
///
/// Unknown fields are rejected with a `400` naming the offending key
/// (#633).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebVcsJitPayload {
    /// Request identifier echoed back in the response. Optional on the
    /// wire (#645): an omitted `id` defaults to the empty string, the
    /// "no correlation id" sentinel echoed back unchanged.
    #[serde(default)]
    pub id: String,
    /// Server-side path to (a directory inside) the git working tree.
    /// Required for commit scoring; must be omitted in diff mode.
    pub repo_path: Option<String>,
    /// Commit / revision to score (default `HEAD`). Mutually exclusive with
    /// `diff`.
    pub commit: Option<String>,
    /// An arbitrary unified diff to score instead of a commit (issue #580).
    /// Only the size and diffusion groups are computable, so the response
    /// is a partial report whose score is **not comparable** to a commit
    /// score. Mutually exclusive with every commit-mode field.
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

/// The JSON keys of every commit-mode-only field on [`WebVcsJitPayload`]
/// (issue #632 / #647).
///
/// Single source of truth for the diff-vs-commit conflict check: both
/// [`WebVcsJitPayload::commit_mode_field_presence`] (which maps each name
/// to its `Option::is_some`) and the conflict message
/// [`VCS_JIT_MODE_CONFLICT`] are built from this list, and a drift-guard
/// test (`commit_mode_fields_list_is_exhaustive`) fails if a new
/// commit-mode field is added to the struct without being listed here. A
/// silently-unlisted field would let a `diff` + that-field payload bypass
/// the conflict check and score the diff while dropping the field (#647
/// observation #4). The order matches struct declaration order so the
/// message reads top-to-bottom.
const COMMIT_MODE_FIELDS: &[&str] = &[
    "repo_path",
    "commit",
    "long_window",
    "recent_window",
    "full_history",
    "include_merges",
    "follow_renames",
    "as_of",
];

/// Error body when a `/vcs/jit` payload combines `diff` with any
/// commit-mode field. The two modes score different, **not comparable**
/// things (`src/vcs/jit.rs`), so rather than silently honor `diff` and
/// drop the rest, the combination is a client mistake (`400`, issue #632).
/// The message names the conflicting fields (from `COMMIT_MODE_FIELDS`,
/// #647) so the 400 is actionable.
pub const VCS_JIT_MODE_CONFLICT: &str = "Invalid `/vcs/jit` request: `diff` is mutually exclusive with the commit-mode fields (`repo_path`, `commit`, `long_window`, `recent_window`, `full_history`, `include_merges`, `follow_renames`, `as_of`); supply either a `diff` or commit-mode fields, not both";

impl WebVcsJitPayload {
    /// Reject a payload that mixes the two mutually exclusive scoring modes.
    ///
    /// Diff mode (`diff` present) and commit mode (every other scoring
    /// field) answer different, non-comparable questions. Honoring `diff`
    /// while silently ignoring the commit-mode fields would score a
    /// different thing than the client believes they asked for, which is
    /// dangerous on a risk-gating endpoint (issue #632). When `diff` is
    /// combined with any commit-mode field, this returns
    /// [`VCS_JIT_MODE_CONFLICT`] for the handler to surface as a `400`.
    ///
    /// # Errors
    ///
    /// Returns [`VCS_JIT_MODE_CONFLICT`] when `diff` is present together
    /// with at least one commit-mode field.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.diff.is_some() && self.has_commit_mode_field() {
            return Err(VCS_JIT_MODE_CONFLICT);
        }
        Ok(())
    }

    /// Whether the payload carries any commit-mode-only field. Used only to
    /// detect the `diff` + commit-mode conflict; an all-empty commit-mode
    /// payload (no `diff` either) stays valid and defaults to `HEAD`.
    fn has_commit_mode_field(&self) -> bool {
        self.commit_mode_field_presence()
            .iter()
            .any(|(_, present)| *present)
    }

    /// Pairs each [`COMMIT_MODE_FIELDS`] key with whether this payload
    /// carries it. Co-located with the field list so adding a struct field
    /// without a matching presence entry is caught by the drift-guard test
    /// rather than silently bypassing the conflict check (#647). The pair
    /// names also let the test prove every listed field is actually probed.
    fn commit_mode_field_presence(&self) -> [(&'static str, bool); COMMIT_MODE_FIELDS.len()] {
        [
            ("repo_path", self.repo_path.is_some()),
            ("commit", self.commit.is_some()),
            ("long_window", self.long_window.is_some()),
            ("recent_window", self.recent_window.is_some()),
            ("full_history", self.full_history.is_some()),
            ("include_merges", self.include_merges.is_some()),
            ("follow_renames", self.follow_renames.is_some()),
            ("as_of", self.as_of.is_some()),
        ]
    }
}

/// Response body for `POST /vcs/jit`. The echoed `id` plus the flattened
/// report — either a commit [`JitReport`] or a partial diff
/// [`JitDiffReport`]; the report's own `source` field (`"commit"` /
/// `"diff"`) tells them apart, present on both variants. Untagged so the
/// report fields sit at the top level without an extra wrapper key.
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

/// Score an arbitrary unified `diff`, rejecting non-diff garbage as a
/// client mistake (issue #652).
///
/// The library [`score_diff`] only rejects input that *looks* like a diff
/// but is structurally broken (a bad `@@` hunk header, a `+`/`-` body line
/// outside any hunk, a plain `diff -u` / combined diff with no
/// `diff --git` header). Wholly-non-diff content — the wrong field, an
/// accidentally-mangled string — parses to zero touched files and would
/// otherwise score a confident `partial_risk_score` of `0.0`. On a
/// risk-*gating* endpoint that "zero risk" answer is the most dangerous
/// failure mode: a CI step feeding garbage would be told the change is
/// safe. The book already documents the opposite contract ("a malformed
/// diff … is a `400`").
///
/// An empty or whitespace-only `diff` legitimately means "no changes" and
/// still scores a valid `0.0`: a CI step that computed an empty diff gets
/// the zero-risk answer it expects. The rejection fires only when the input
/// carries non-whitespace content yet yields no touched file
/// ([`JitSize::files_touched`] `== 0`) — i.e. it is not a diff at all. A
/// valid diff with only binary or rename-only stanzas still flushes a
/// touched file, so it passes.
///
/// # Errors
///
/// Propagates [`vcs::Error::InvalidDiff`] from [`score_diff`] for malformed
/// diff-shaped input, and returns it for non-empty, non-whitespace input
/// that parses to zero touched files.
fn score_diff_validated(diff: &str) -> Result<JitDiffReport, vcs::Error> {
    let report = score_diff(diff)?;
    if !diff.trim().is_empty() && report.size.files_touched == 0 {
        return Err(vcs::Error::InvalidDiff(
            "the request body is not a unified diff: no `diff --git` file \
             headers were found (supply `git diff` / `git format-patch` \
             output, or omit `diff` for an empty change)"
                .to_owned(),
        ));
    }
    Ok(report)
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
        WebVcsJitReport::Diff(score_diff_validated(diff)?)
    } else {
        let options = jit_options_from(&payload)?;
        // A commit score needs a repository; a missing `repo_path` is a
        // client mistake, surfaced as the same not-a-repository 400 as a
        // path that exists but is not a working tree.
        let repo_path = payload
            .repo_path
            .as_ref()
            .ok_or_else(|| vcs::Error::NotARepository(PathBuf::from("")))?;
        repo_path_must_exist(repo_path)?;
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
///
/// The shared `/vcs` fields are **inlined** rather than `#[serde(flatten)]`d
/// (#633): `deny_unknown_fields` is incompatible with `flatten` (serde
/// cannot tell which struct owns a given key), so the only way to reject an
/// unknown key on this endpoint is a single flat struct. The JSON shape is
/// unchanged — every field still sits at the top level. The inlined block
/// mirrors [`WebVcsPayload`] field-for-field **except the cache knobs**
/// (`no_cache` / `cache_dir`): trend does not use the persistent cache, so
/// those fields are deliberately omitted and a client that sends them gets
/// a `400` under `deny_unknown_fields` rather than the silent no-op the
/// endpoint used to accept (issue #961). Each sampled point re-anchors at a
/// distinct historical tip with its own `as_of`, which the cache
/// fingerprints separately and never evicts — honoring the knobs would only
/// grow the cache with per-point entries that never hit. Keep the two
/// structs in lockstep when either gains any *other* knob.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebVcsTrendPayload {
    // --- shared `/vcs` knobs, inlined from `WebVcsPayload` (#633) ---
    /// Request identifier echoed back in the response. Optional on the
    /// wire (#645): an omitted `id` defaults to the empty string, the
    /// "no correlation id" sentinel echoed back unchanged.
    #[serde(default)]
    pub id: String,
    /// Server-side path to (a directory inside) the git working tree.
    pub repo_path: String,
    /// Long window (default `12mo`).
    pub long_window: Option<String>,
    /// Recent window (default `90d`).
    pub recent_window: Option<String>,
    /// Show only the top N files by risk (default 50; `0` = all — #636).
    pub top: Option<usize>,
    /// Revision to analyse (default `HEAD`).
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// Composite formula: `weighted` (default) or `percentile`.
    pub risk_formula: Option<String>,
    /// File-type scope (issue #576).
    pub file_types: Option<String>,
    /// Walk the full DAG rather than first-parent only.
    pub full_history: Option<bool>,
    /// Include merge commits.
    pub include_merges: Option<bool>,
    /// Follow renames (default true).
    pub follow_renames: Option<bool>,
    /// Exclude bot identities (default true).
    pub exclude_bots: Option<bool>,
    /// Override the bot-author exclusion regex.
    pub bot_pattern: Option<String>,
    /// Reference "now" (RFC 3339 / `@unix` / git date) for snapshots.
    pub as_of: Option<String>,
    /// Emit SHA-256-hashed author identities.
    pub emit_author_details: Option<bool>,
    /// Secret key that hardens `emit_author_details` into a keyed
    /// HMAC-SHA256 (issue #956). Requires `emit_author_details`. SECURITY:
    /// raw secret in a `Debug`-deriving struct — never whole-struct
    /// debug-log a payload; see [`WebVcsPayload::author_hash_key`].
    pub author_hash_key: Option<String>,
    /// Include files deleted at the target ref.
    pub include_deleted: Option<bool>,
    /// Bus-factor coverage (abandonment) threshold in `(0, 1)` (issue #332).
    pub bus_factor_threshold: Option<f64>,
    // NOTE: `no_cache` / `cache_dir` are intentionally absent here — trend
    // does not use the persistent cache (issue #961). See the struct doc.
    // --- trend-only knobs ---
    /// Number of evenly-spaced sample points (>= 2; default 12 — #636).
    #[serde(default = "default_trend_points")]
    pub points: usize,
    /// Total look-back span the points cover (default `12mo`).
    pub span: Option<String>,
    /// Top N files per improving / regressing delta list (default 10;
    /// `0` = all — #636).
    pub top_deltas: Option<usize>,
}

impl WebVcsTrendPayload {
    /// Reassemble the shared `/vcs` knobs into a [`WebVcsPayload`] so the
    /// trend handler reuses `options_from` without duplicating the
    /// payload→[`Options`] mapping. Consumes the inlined fields.
    fn into_base(self) -> (WebVcsPayload, TrendKnobs) {
        let base = WebVcsPayload {
            id: self.id,
            repo_path: self.repo_path,
            long_window: self.long_window,
            recent_window: self.recent_window,
            top: self.top,
            reference: self.reference,
            risk_formula: self.risk_formula,
            file_types: self.file_types,
            full_history: self.full_history,
            include_merges: self.include_merges,
            follow_renames: self.follow_renames,
            exclude_bots: self.exclude_bots,
            bot_pattern: self.bot_pattern,
            as_of: self.as_of,
            emit_author_details: self.emit_author_details,
            author_hash_key: self.author_hash_key,
            include_deleted: self.include_deleted,
            bus_factor_threshold: self.bus_factor_threshold,
            // Trend never builds a `CacheConfig`, and these fields are not
            // even present on the trend payload (issue #961): the base only
            // exists to reuse `options_from`, which ignores them. Pin them
            // off so a future reader cannot mistake the base for a caching
            // path.
            no_cache: None,
            cache_dir: None,
        };
        let knobs = TrendKnobs {
            points: self.points,
            span: self.span,
            top_deltas: self.top_deltas,
        };
        (base, knobs)
    }
}

/// The trend-only knobs split out of [`WebVcsTrendPayload`] by
/// [`WebVcsTrendPayload::into_base`].
struct TrendKnobs {
    points: usize,
    span: Option<String>,
    top_deltas: Option<usize>,
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
    let (base, knobs) = payload.into_base();
    repo_path_must_exist(&base.repo_path)?;
    let options = options_from(&base)?;
    let span_secs = parse_window(
        knobs
            .span
            .as_deref()
            .unwrap_or(vcs::options::DEFAULT_LONG_WINDOW),
    )?;
    let trend = build_trend(
        &PathBuf::from(&base.repo_path),
        &options,
        knobs.points,
        span_secs,
    )?;
    // Both default to the CLI's bounded counts (`top` 50, `top_deltas`
    // 10), with `0` meaning "all" per #602 (#636).
    let wire_trend = wire::VcsTrend::from_trend(
        &trend,
        base.top.unwrap_or(DEFAULT_TOP),
        knobs.top_deltas.unwrap_or(DEFAULT_TOP_DELTAS),
    );
    Ok(WebVcsTrendResponse {
        id: base.id,
        trend: wire_trend,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // #652: non-diff garbage must be rejected, while a valid diff and an
    // empty diff both pass. Exercises `score_diff_validated` directly so the
    // boundary is pinned at the unit level, independent of the HTTP layer.
    #[test]
    // An empty diff scores an *exact* `0.0` (no float arithmetic feeds it —
    // zero touched files means a zero contribution sum), so the equality is
    // bit-exact rather than magnitude-brittle.
    #[allow(clippy::float_cmp)]
    fn score_diff_validated_rejects_garbage_but_keeps_empty_and_valid() {
        // Wholly-non-diff content → InvalidDiff (was a silent 0.0 score).
        let err = score_diff_validated("not a diff").expect_err("garbage must be rejected");
        assert!(
            matches!(err, vcs::Error::InvalidDiff(_)),
            "garbage must map to InvalidDiff, got: {err:?}"
        );
        // Empty / whitespace-only → valid zero-feature report (no changes).
        for empty in ["", "   \n\t\n"] {
            let report = score_diff_validated(empty)
                .unwrap_or_else(|e| panic!("an empty diff must stay valid, got: {e:?}"));
            assert_eq!(report.size.files_touched, 0);
            assert_eq!(report.partial_risk_score, 0.0);
        }
        // A real diff with one touched file → valid report.
        let diff =
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,2 @@\n keep\n+add\n";
        let report = score_diff_validated(diff).expect("a real diff must score");
        assert_eq!(report.size.files_touched, 1);
    }

    // #653: a nonexistent path is a client mistake mapped to NotARepository
    // (a client-input 400), while an existing path passes the guard (the
    // backend then decides repo-vs-non-repo). `Path::exists` follows
    // symlinks, so this is spelling-agnostic.
    #[test]
    fn repo_path_must_exist_maps_missing_path_to_not_a_repository() {
        let err = repo_path_must_exist("/tmp/bca-definitely-missing-652653")
            .expect_err("a nonexistent path must be rejected");
        assert!(
            matches!(err, vcs::Error::NotARepository(_)),
            "a missing path must map to NotARepository, got: {err:?}"
        );
        assert!(
            err.is_client_input(),
            "a missing path is a client-input error (400), not a backend 500"
        );
        // An existing directory passes the existence guard (the temp dir is
        // not a repo, but that distinction is the backend's to make).
        let dir = tempfile::tempdir().expect("tempdir");
        repo_path_must_exist(dir.path().to_str().expect("utf-8 path"))
            .expect("an existing path must pass the existence guard");
    }

    // #956: an `author_hash_key` without `emit_author_details` is a client
    // mistake (the key would do nothing), surfaced as a `400`; an empty key
    // is likewise rejected; and a key with the flag builds keyed options.
    #[test]
    fn author_hash_key_requires_emit_and_rejects_empty() {
        let without_emit: WebVcsPayload =
            serde_json::from_str(r#"{"id":"x","repo_path":"/x","author_hash_key":"k"}"#)
                .expect("payload deserializes");
        let err = options_from(&without_emit).expect_err("key without emit must be rejected");
        assert!(
            matches!(err, vcs::Error::InvalidAuthorHashKey(_)),
            "key without emit must map to InvalidAuthorHashKey, got: {err:?}"
        );
        assert!(err.is_client_input(), "must map to a 400, not a 500");

        let empty_key: WebVcsPayload = serde_json::from_str(
            r#"{"id":"x","repo_path":"/x","emit_author_details":true,"author_hash_key":""}"#,
        )
        .expect("payload deserializes");
        assert!(
            matches!(
                options_from(&empty_key),
                Err(vcs::Error::InvalidAuthorHashKey(_))
            ),
            "an empty key must be rejected"
        );

        let valid: WebVcsPayload = serde_json::from_str(
            r#"{"id":"x","repo_path":"/x","emit_author_details":true,"author_hash_key":"k"}"#,
        )
        .expect("payload deserializes");
        let options = options_from(&valid).expect("a key with the flag builds options");
        assert!(
            options.author_hash_key.is_some(),
            "the key must reach the backend options"
        );
    }

    // #636: the web defaults must equal the CLI's bounded defaults so the
    // same logical invocation returns the same-sized result on either
    // surface (`bca vcs --top` 50, `--top-deltas` 10, `--points` 12).
    #[test]
    fn web_vcs_defaults_match_cli() {
        assert_eq!(DEFAULT_TOP, 50, "web `top` default must match the CLI");
        assert_eq!(
            DEFAULT_TOP_DELTAS, 10,
            "web `top_deltas` default must match the CLI"
        );
        assert_eq!(
            DEFAULT_TREND_POINTS, 12,
            "web `points` default must match the CLI"
        );
    }

    // #636: an omitted `points` deserializes to the default 12 rather than
    // failing the previously-required field. The shared `/vcs` knobs stay
    // flat (no nested `base` object) after un-flattening for #633.
    #[test]
    fn trend_payload_points_defaults_when_absent() {
        let payload: WebVcsTrendPayload =
            serde_json::from_str(r#"{"id":"t","repo_path":"/x","span":"300d"}"#)
                .expect("a trend payload without `points` must deserialize");
        assert_eq!(payload.points, DEFAULT_TREND_POINTS);
        // `top` / `top_deltas` stay `None` here; the handler applies the
        // 50 / 10 defaults at the call site (an explicit `0` means "all").
        assert_eq!(payload.top, None);
        assert_eq!(payload.top_deltas, None);
    }

    // #633: an unknown key on the un-flattened trend payload is rejected
    // (serde names the offender), proving the flatten removal kept
    // `deny_unknown_fields` working across the inlined `/vcs` knobs.
    #[test]
    fn trend_payload_rejects_unknown_field() {
        let err = serde_json::from_str::<WebVcsTrendPayload>(
            r#"{"id":"t","repo_path":"/x","points":3,"top_dletas":5}"#,
        )
        .expect_err("an unknown key must be rejected");
        assert!(
            err.to_string().contains("top_dletas"),
            "the error must name the offending key, got: {err}"
        );
    }

    // #961 drift guard: the trend endpoint does not use the persistent
    // cache, so the shared `/vcs` cache knobs are absent from its payload —
    // a client that sends them gets a `400` under `deny_unknown_fields`
    // (the serde error names the offender) rather than the silent no-op the
    // endpoint used to accept. Re-adding either field by copy-paste from
    // `WebVcsPayload` without wiring it into a real `CacheConfig` would flip
    // this back to silent acceptance and fail here.
    #[test]
    fn trend_payload_rejects_cache_knobs() {
        for (field, value) in [("no_cache", "true"), ("cache_dir", "\"/tmp/x\"")] {
            let body = format!(r#"{{"id":"t","repo_path":"/x","{field}":{value}}}"#);
            let err = serde_json::from_str::<WebVcsTrendPayload>(&body)
                .expect_err("trend must reject a cache knob it cannot honor");
            assert!(
                err.to_string().contains(field),
                "the error must name the rejected `{field}`, got: {err}"
            );
        }
    }

    // #647: the presence list, the named-field list, and the conflict
    // message must stay in lockstep — they are the three faces of the same
    // single source of truth. If they drift, a `diff` + commit-mode payload
    // could bypass the conflict check and silently score the diff while
    // dropping the field.
    #[test]
    fn commit_mode_presence_names_match_field_list() {
        // The presence pairs (built from struct fields) must name exactly
        // `COMMIT_MODE_FIELDS`, in the same order. An array entry whose name
        // typoes away from the list, or a list entry with no presence pair,
        // fails here.
        let payload = WebVcsJitPayload {
            id: String::new(),
            repo_path: None,
            commit: None,
            diff: None,
            long_window: None,
            recent_window: None,
            full_history: None,
            include_merges: None,
            follow_renames: None,
            as_of: None,
        };
        let presence_names: Vec<&str> = payload
            .commit_mode_field_presence()
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(
            presence_names, COMMIT_MODE_FIELDS,
            "commit_mode_field_presence() must name exactly COMMIT_MODE_FIELDS"
        );
    }

    #[test]
    fn conflict_message_names_every_commit_mode_field() {
        for field in COMMIT_MODE_FIELDS {
            assert!(
                VCS_JIT_MODE_CONFLICT.contains(field),
                "the conflict message must name `{field}` so the 400 is actionable"
            );
        }
    }

    // #647 drift guard: every commit-mode field, supplied on its own
    // alongside `diff`, must individually trip the conflict. Deserializing
    // a real JSON payload per field proves (a) each `COMMIT_MODE_FIELDS`
    // entry is a genuine struct key (a stale name 400s as an unknown field
    // under `deny_unknown_fields`, failing the parse below), and (b) each
    // is wired into `has_commit_mode_field` via its presence pair. A new
    // commit-mode field added to the struct but not to the list would parse
    // here yet not trip the conflict, but it is also unreachable by this
    // loop — so the companion exhaustiveness test below counts the fields.
    #[test]
    fn each_commit_mode_field_conflicts_with_diff() {
        for field in COMMIT_MODE_FIELDS {
            // Booleans take a bare `true`; everything else a JSON string.
            let value = match *field {
                "full_history" | "include_merges" | "follow_renames" => "true".to_owned(),
                _ => "\"x\"".to_owned(),
            };
            let body = format!(r#"{{"id":"t","diff":"--- a\n+++ b\n","{field}":{value}}}"#);
            let payload: WebVcsJitPayload = serde_json::from_str(&body)
                .unwrap_or_else(|e| panic!("`{field}` must be a real payload key: {e}"));
            assert_eq!(
                payload.validate(),
                Err(VCS_JIT_MODE_CONFLICT),
                "`diff` + `{field}` must be rejected as a mode conflict"
            );
        }
    }

    // #647 exhaustiveness: the field list must cover *every* commit-mode
    // option on the struct. A populated payload — every commit-mode field
    // `Some`, `diff` absent — must report exactly `COMMIT_MODE_FIELDS.len()`
    // present fields. Adding a struct field to `commit_mode_field_presence`
    // without listing it in `COMMIT_MODE_FIELDS` (or vice versa) breaks the
    // array length and fails to compile; adding one to neither leaves it
    // unprobed and is caught by code review against this count.
    #[test]
    fn commit_mode_fields_list_is_exhaustive() {
        let all_set = WebVcsJitPayload {
            id: String::new(),
            repo_path: Some("/repo".to_owned()),
            commit: Some("HEAD".to_owned()),
            diff: None,
            long_window: Some("12mo".to_owned()),
            recent_window: Some("90d".to_owned()),
            full_history: Some(true),
            include_merges: Some(true),
            follow_renames: Some(true),
            as_of: Some("2024-01-01".to_owned()),
        };
        let present = all_set
            .commit_mode_field_presence()
            .iter()
            .filter(|(_, p)| *p)
            .count();
        assert_eq!(
            present,
            COMMIT_MODE_FIELDS.len(),
            "every commit-mode field must be probed by the presence list"
        );
        assert!(
            all_set.has_commit_mode_field(),
            "a fully-populated commit-mode payload must report a commit-mode field"
        );
    }
}
