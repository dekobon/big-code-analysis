// bca: suppress-file(halstead, nargs, exit)
// File-level halstead/nargs/exit are many-fn aggregation artifacts (the
// window parser's unit branches + the Options builder), not
// per-function logic complexity (cognitive/cyclomatic stay enforced).

//! Configuration for a change-history walk.
//!
//! [`Options`] is a plain data struct: the CLI / web / Python layers
//! fill it in from user input, and [`build_history_index`](crate::vcs::build_history_index)
//! consumes it. Time windows are stored already resolved to seconds so
//! the generic core never re-parses user duration strings; the
//! [`parse_window`] helper is exposed for those front ends to perform
//! that resolution (and to surface a typed [`Error`] on bad input).

use std::path::Path;

use super::error::Error;

/// Seconds in a day.
pub(crate) const SECONDS_PER_DAY: i64 = 86_400;
/// Seconds in a week.
const SECONDS_PER_WEEK: i64 = 7 * SECONDS_PER_DAY;
/// Seconds in an average Gregorian month (30.436875 days). Months and
/// years are inherently approximate; the average keeps `12mo` and `1y`
/// numerically identical (both 365.2425 days → 365 days), matching the
/// `long_window_days: 365` shown in the issue's output sample.
const SECONDS_PER_MONTH: i64 = 2_629_746;
/// Seconds in an average Gregorian year (365.2425 days).
const SECONDS_PER_YEAR: i64 = 31_556_952;

/// Default long window (`12mo` ≈ 365 days).
pub const DEFAULT_LONG_WINDOW: &str = "12mo";
/// Default recent window (`90d`).
pub const DEFAULT_RECENT_WINDOW: &str = "90d";

/// Default bus-factor coverage (abandonment) threshold (`0.5`, per
/// Avelino). Re-exported from the bus-factor module so the front ends
/// share one source of truth for the default and the validation bound.
pub const DEFAULT_BUS_FACTOR_THRESHOLD: f64 = super::bus_factor::DEFAULT_COVERAGE_THRESHOLD;

/// Default bot-author exclusion pattern (case-insensitive, matched as a
/// substring against both the canonical author name and email). The
/// `[bot]` suffixes are regex-escaped. Mirrors the well-known automation
/// identities called out in issue #328.
pub const DEFAULT_BOT_PATTERN: &str = r"dependabot\[bot\]|renovate\[bot\]|github-actions\[bot\]|pre-commit-ci\[bot\]|mergify\[bot\]|pyup-bot";

/// Which composite risk-score formula to apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RiskFormula {
    /// Log-scaled weighted sum with categorical bumps (the `v1`
    /// formula documented in [`score`](crate::vcs::score)).
    #[default]
    Weighted,
    /// Each signal re-ranked to its percentile within the analyzed
    /// set, then averaged. The literature recommends relative triggers
    /// over hard thresholds for cross-project robustness.
    Percentile,
}

impl std::str::FromStr for RiskFormula {
    type Err = Error;

    /// Parse the user-facing formula name. The single source of truth
    /// shared by the web (`POST /vcs`) and Python (`vcs_metrics`) front
    /// ends; the CLI uses a clap `ValueEnum` instead.
    fn from_str(s: &str) -> Result<Self, Error> {
        match s {
            "weighted" => Ok(Self::Weighted),
            "percentile" => Ok(Self::Percentile),
            other => Err(Error::InvalidFormula(other.to_owned())),
        }
    }
}

/// Which tracked files the change-history walk ranks (issue #576).
///
/// Applied as an **additional** extension-only filter on top of the
/// `--paths` / `--include` / `--exclude` globs (AND semantics): a file
/// must pass both to be ranked. The check never reads blob content, so a
/// language detected only by an in-file modeline (Emacs / Vim) — never by
/// its extension — is treated as out-of-scope under [`Metrics`](Self::Metrics);
/// this is the one documented divergence from the content-aware `bca
/// metrics` walk.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FileTypeScope {
    /// Only files bca computes metrics for — the same set `bca metrics`
    /// would analyze, resolved by extension via
    /// [`get_language_for_file`](crate::get_language_for_file). The
    /// default: it keeps the change-history ranking aligned with the AST
    /// hotspot tables (which only cover files-with-metrics) and keeps
    /// high-churn non-source files (`CHANGELOG.md`, `Cargo.lock`, CI
    /// config) out of the risk ranking. Extension-less files
    /// (`Makefile`, `Dockerfile`, `LICENSE`) and unknown extensions are
    /// excluded.
    #[default]
    Metrics,
    /// Every tracked, non-binary, non-symlink text file — the behaviour
    /// before the `metrics` default was introduced.
    All,
    /// A user-supplied allow-list of file extensions, normalised to
    /// lowercase with any leading dot stripped (`rs`, `py`, `toml`). A
    /// file is in scope iff its lowercased extension is in the list.
    Custom(Vec<String>),
}

impl FileTypeScope {
    /// Whether `path` is in scope, judged by extension only (no blob
    /// content is read, so this stays a cheap pre-filter on the file
    /// enumeration).
    #[must_use]
    pub fn includes(&self, path: &Path) -> bool {
        match self {
            Self::All => true,
            // Route through the same extension predicate the metrics walk
            // resolves a language with, so the `metrics` scope stays in
            // lockstep with the analyzable-file set as languages are
            // added or removed.
            Self::Metrics => crate::get_language_for_file(path).is_some(),
            Self::Custom(extensions) => path
                .extension()
                .and_then(|ext| ext.to_str())
                // The stored extensions are already lowercased, so an
                // ASCII case-insensitive compare avoids allocating a
                // lowercased copy of every file's extension in this
                // per-file path.
                .is_some_and(|ext| {
                    extensions
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(ext))
                }),
        }
    }

    /// Parse a custom comma-separated extension list, normalising each
    /// entry (trim, strip a leading dot, lowercase) and dropping blanks.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidFileTypeScope`] when the list normalises
    /// to nothing (empty, or only blanks / bare dots) — a scope that
    /// would silently rank no files.
    fn from_extensions(list: &str) -> Result<Self, Error> {
        let mut extensions: Vec<String> = Vec::new();
        for raw in list.split(',') {
            let normalized = raw.trim().trim_start_matches('.').to_lowercase();
            if normalized.is_empty() {
                continue;
            }
            if !extensions.contains(&normalized) {
                extensions.push(normalized);
            }
        }
        if extensions.is_empty() {
            return Err(Error::InvalidFileTypeScope(format!(
                "{list:?} lists no usable file extensions"
            )));
        }
        Ok(Self::Custom(extensions))
    }
}

impl std::str::FromStr for FileTypeScope {
    type Err = Error;

    /// Parse the user-facing scope: the keywords `metrics` / `all`, or
    /// any other value as a comma-separated custom extension list. The
    /// single source of truth shared by the CLI (`--file-types`), the
    /// `bca.toml` `[vcs] file_types` key, the web front end, and Python.
    fn from_str(s: &str) -> Result<Self, Error> {
        match s.trim() {
            "" => Err(Error::InvalidFileTypeScope("the value is empty".to_owned())),
            "metrics" => Ok(Self::Metrics),
            "all" => Ok(Self::All),
            list => Self::from_extensions(list),
        }
    }
}

/// Configuration for a single change-history walk.
// The booleans are independent on/off CLI toggles (`--full-history`,
// `--include-merges`, …); packing them into a flags newtype would hide
// each one's meaning at construction sites for no real gain.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug)]
// Sealed against external struct-literal construction so future additive
// fields stay non-breaking: downstream crates start from `Options::default()`
// and assign the `pub` fields they care about (see STABILITY.md).
#[non_exhaustive]
pub struct Options {
    /// Long observation window, in seconds (default ≈ 365 days).
    pub long_window_secs: i64,
    /// Recent observation window, in seconds (default 90 days).
    pub recent_window_secs: i64,
    /// Revision to start the walk from (default `HEAD`).
    pub reference: String,
    /// Walk the full commit DAG rather than first-parent only.
    pub full_history: bool,
    /// Include merge commits (default: skip them).
    pub include_merges: bool,
    /// Follow file renames across history (default: on).
    pub follow_renames: bool,
    /// Exclude bot author identities (default: on).
    pub exclude_bots: bool,
    /// Regex matched against author name/email to detect bots.
    pub bot_pattern: String,
    /// Reference "now" as a Unix timestamp for reproducible snapshots
    /// (`--as-of`). `None` means wall-clock time at walk start.
    pub as_of: Option<i64>,
    /// Which composite score to compute.
    pub risk_formula: RiskFormula,
    /// Emit SHA-256-hashed canonical author identities (default: off —
    /// author identities never leave the process otherwise).
    pub emit_author_details: bool,
    /// Emit stats for files deleted at the target ref (default: off).
    pub include_deleted: bool,
    /// Compute the directory- / repo-level bus-factor aggregate from the
    /// walk (issue #332). Default off: it retains per-file authorship
    /// beyond the per-file [`Stats`](crate::vcs::Stats), which the
    /// repeated JIT-prior and per-file-injection walks neither need nor
    /// should pay for.
    pub compute_bus_factor: bool,
    /// Coverage (abandonment) threshold for the bus factor, in `(0, 1)`
    /// — the fraction of files that must be orphaned for the greedy
    /// removal to stop (default [`DEFAULT_BUS_FACTOR_THRESHOLD`], `0.5`
    /// per Avelino). Ignored unless `compute_bus_factor` is set.
    pub bus_factor_threshold: f64,
    /// Which tracked files to rank (issue #576). Defaults to
    /// [`FileTypeScope::Metrics`] — only files bca has metrics for — so
    /// high-churn non-source files do not dominate the risk ranking and
    /// the change-history view aligns with the AST hotspot tables.
    pub file_types: FileTypeScope,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            // The default-window constants are valid by construction;
            // `expect` documents the invariant (AGENTS.md permits it
            // for provably-unreachable cases). A unit test pins it.
            long_window_secs: parse_window(DEFAULT_LONG_WINDOW)
                .expect("DEFAULT_LONG_WINDOW parses"),
            recent_window_secs: parse_window(DEFAULT_RECENT_WINDOW)
                .expect("DEFAULT_RECENT_WINDOW parses"),
            reference: "HEAD".to_owned(),
            full_history: false,
            include_merges: false,
            follow_renames: true,
            exclude_bots: true,
            bot_pattern: DEFAULT_BOT_PATTERN.to_owned(),
            as_of: None,
            risk_formula: RiskFormula::Weighted,
            emit_author_details: false,
            include_deleted: false,
            compute_bus_factor: false,
            bus_factor_threshold: DEFAULT_BUS_FACTOR_THRESHOLD,
            file_types: FileTypeScope::Metrics,
        }
    }
}

impl Options {
    /// Long window expressed in whole days (for the serialized
    /// `long_window_days` field and the `new_file`/age cap).
    #[must_use]
    pub fn long_window_days(&self) -> u32 {
        secs_to_days(self.long_window_secs)
    }

    /// Recent window expressed in whole days.
    #[must_use]
    pub fn recent_window_days(&self) -> u32 {
        secs_to_days(self.recent_window_secs)
    }
}

/// Round a second count to the nearest whole day, saturating into
/// `u32`. Window lengths never approach `u32::MAX` days in practice,
/// but the saturation keeps the conversion total and lint-clean.
fn secs_to_days(secs: i64) -> u32 {
    let days = (secs + SECONDS_PER_DAY / 2) / SECONDS_PER_DAY;
    u32::try_from(days.max(0)).unwrap_or(u32::MAX)
}

/// Validate a bus-factor coverage threshold, accepting only a finite
/// value in the open interval `(0, 1)`.
///
/// A `0` would make the first key-developer removal "exceed" the
/// abandonment fraction (bus factor always 1) and a `1` could never be
/// exceeded (bus factor = every author), so both extremes are user errors
/// rather than values to silently clamp. The single source of truth
/// shared by every front end.
///
/// # Errors
///
/// Returns [`Error::InvalidBusFactorThreshold`] when `threshold` is
/// non-finite or outside `(0, 1)`.
pub fn validate_bus_factor_threshold(threshold: f64) -> Result<f64, Error> {
    if threshold.is_finite() && threshold > 0.0 && threshold < 1.0 {
        Ok(threshold)
    } else {
        Err(Error::InvalidBusFactorThreshold(format!(
            "{threshold} is not in the open interval (0, 1)"
        )))
    }
}

/// Parse a human time-window string into seconds.
///
/// Accepts a suffix form — `<number><unit>` with unit `d` (days),
/// `w` (weeks), `mo` (months), or `y` (years) — or an ISO 8601 duration
/// (`P12M`, `P90D`, `P2Y`, `P8W`). Months and years use the average
/// Gregorian length, so `12mo`, `1y`, and `P1Y` all resolve to
/// 365 days.
///
/// # Errors
///
/// Returns [`Error::InvalidWindow`] when the input is empty, carries an
/// unrecognised unit, or has a non-numeric magnitude.
pub fn parse_window(spec: &str) -> Result<i64, Error> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidWindow("window is empty".to_owned()));
    }
    if let Some(rest) = trimmed.strip_prefix(['P', 'p']) {
        return parse_iso8601(rest, spec);
    }
    // Suffix form: split the trailing alphabetic unit from the leading
    // numeric magnitude.
    let split = trimmed
        .find(|c: char| c.is_ascii_alphabetic())
        .ok_or_else(|| Error::InvalidWindow(format!("{spec:?} has no unit")))?;
    let (number, unit) = trimmed.split_at(split);
    let magnitude: i64 = number
        .trim()
        .parse()
        .map_err(|_| Error::InvalidWindow(format!("{number:?} is not a whole number")))?;
    let factor = unit_factor(unit)
        .ok_or_else(|| Error::InvalidWindow(format!("unknown unit {unit:?} in {spec:?}")))?;
    checked_window(magnitude, factor, spec)
}

/// Seconds-per-unit for the suffix form. `mo` is months (the bare `m`
/// is intentionally rejected as ambiguous between minutes and months).
fn unit_factor(unit: &str) -> Option<i64> {
    match unit {
        "d" => Some(SECONDS_PER_DAY),
        "w" => Some(SECONDS_PER_WEEK),
        "mo" => Some(SECONDS_PER_MONTH),
        "y" => Some(SECONDS_PER_YEAR),
        _ => None,
    }
}

/// Parse the post-`P` body of an ISO 8601 duration. Only the date
/// portion is meaningful for a history window; a `T` time section is
/// rejected rather than silently ignored.
fn parse_iso8601(body: &str, original: &str) -> Result<i64, Error> {
    if body.is_empty() {
        return Err(Error::InvalidWindow(format!("{original:?} has no fields")));
    }
    let mut total: i64 = 0;
    let mut digits = String::new();
    for ch in body.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if digits.is_empty() {
            return Err(Error::InvalidWindow(format!(
                "{original:?} field {ch:?} has no magnitude"
            )));
        }
        let magnitude: i64 = digits
            .parse()
            .map_err(|_| Error::InvalidWindow(format!("{digits:?} is not a whole number")))?;
        digits.clear();
        // Date designators only: Y, M (months — date context), W, D.
        let factor = match ch {
            'Y' => SECONDS_PER_YEAR,
            'M' => SECONDS_PER_MONTH,
            'W' => SECONDS_PER_WEEK,
            'D' => SECONDS_PER_DAY,
            _ => {
                return Err(Error::InvalidWindow(format!(
                    "unsupported ISO 8601 designator {ch:?} in {original:?}"
                )));
            }
        };
        total = total
            .checked_add(
                magnitude
                    .checked_mul(factor)
                    .ok_or_else(|| Error::InvalidWindow(format!("{original:?} overflows")))?,
            )
            .ok_or_else(|| Error::InvalidWindow(format!("{original:?} overflows")))?;
    }
    if !digits.is_empty() {
        return Err(Error::InvalidWindow(format!(
            "{original:?} ends with a magnitude {digits:?} lacking a designator"
        )));
    }
    reject_non_positive(total, original)
}

/// Multiply a magnitude by its unit factor, rejecting negatives and
/// overflow.
fn checked_window(magnitude: i64, factor: i64, spec: &str) -> Result<i64, Error> {
    if magnitude < 0 {
        return Err(Error::InvalidWindow(format!("{spec:?} is negative")));
    }
    let product = magnitude
        .checked_mul(factor)
        .ok_or_else(|| Error::InvalidWindow(format!("{spec:?} overflows")))?;
    reject_non_positive(product, spec)
}

/// A zero-length window degenerates the walk (its boundary collapses
/// onto `now`, admitting no history), so reject it rather than silently
/// producing an empty result.
fn reject_non_positive(seconds: i64, spec: &str) -> Result<i64, Error> {
    if seconds <= 0 {
        return Err(Error::InvalidWindow(format!(
            "{spec:?} is not a positive duration"
        )));
    }
    Ok(seconds)
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
