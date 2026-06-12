// bca: suppress-file(halstead, nargs, nexits)
// bca.toml manifest load/merge; the offenders are many-fn / impl-aggregate
// artifacts. (`merge_check`'s cyclomatic — flat field-by-field config merge —
// is suppressed per-function below; cognitive stays enforced.)

//! `bca.toml` manifest discovery and merge (issue #374).
//!
//! Consolidates the flags every local-gate recipe used to thread
//! through each invocation (`--paths`, `--exclude-from`, `--jobs`,
//! `--config`, `--baseline`, `--tier=soft=<ratio>`) into one
//! discoverable file at the repo root.
//!
//! # Resolution order
//!
//! Per the documented order shared across #373/#374/#375/#380:
//!
//! 1. Manifest `[thresholds]` is the base layer.
//! 2. `--config <file>` merges on top (config keys win on collision).
//! 3. `--tier=soft=<ratio>` scales the merged config-derived limits.
//! 4. Repeated `--threshold name=value` CLI flags apply last, absolutely.
//!
//! For list-valued options the merge splits by *list meaning* (#539):
//! positive scope keys (`paths`, `include`) are REPLACED by any explicit
//! CLI value, while negative filter keys (`exclude`, `[check] exclude`)
//! UNION CLI values with the manifest list (so a CLI exclude never
//! silently un-skips a directory the project deliberately excluded).
//! Scalar / path options (`exclude_from`, `jobs`, `[check]
//! baseline`, `[check] headroom`) fill from the manifest only when the
//! CLI left them unset.
//! `--no-config` bypasses discovery entirely, leaving CLI values alone.
//!
//! Relative paths in the manifest are resolved against the manifest's
//! own directory (Cargo-style), so a `bca.toml` discovered above the
//! current working directory still points at the right files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;

use crate::thresholds::{ParsedThresholds, split_thresholds_table};
use crate::{
    CheckArgs, ExemptionsArgs, GlobalOpts, NumJobs, ReportArgs, VcsArgs, die, die_io,
    read_utf8_file, warn,
};

/// Filename discovered by convention at (or above) the working directory.
const MANIFEST_FILE: &str = "bca.toml";

/// Top-level manifest keys understood today. Any other top-level key
/// triggers a one-line "ignored" warning, so unreleased options can be
/// pre-adopted without breaking older `bca` builds. The
/// `[thresholds.soft]` sub-table (#375) is *not* a top-level key — it
/// lives under the known `thresholds` key and is split out by
/// [`split_thresholds_table`]. The `[check]` table (#378/#385/#599)
/// carries gate-only options (`exclude`, `exclude_from`, `exit_codes`,
/// `baseline`, `baseline_line_tolerance`, `baseline_fuzzy_match`,
/// `headroom`) and is consumed as the typed [`RawCheck`]. The bare
/// top-level `baseline*` / `headroom` keys remain on this allowlist as
/// deprecated aliases (#599): they are honored for one release cycle
/// with a [`warn_deprecated_top_level_check_keys`] notice, so listing
/// them here keeps that the *only* warning they draw (not the
/// misleading "unrecognized key" notice).
const KNOWN_KEYS: &[&str] = &[
    "paths",
    "exclude_from",
    "jobs",
    // Deprecated one-cycle alias for `jobs` (issue #666). Listed here so
    // it draws only the rename-deprecation notice, not the misleading
    // "unrecognized key" warning.
    "num_jobs",
    "include",
    "exclude",
    "baseline",
    "baseline_line_tolerance",
    "baseline_fuzzy_match",
    "cyclomatic_count_try",
    "headroom",
    "thresholds",
    "check",
    "report",
    "vcs",
];

/// A parsed `bca.toml` plus the directory it was found in.
pub(crate) struct Manifest {
    /// Directory containing the manifest; relative manifest paths
    /// resolve against it.
    dir: PathBuf,
    /// Full path to the manifest file (provenance for
    /// `--print-effective-config`).
    path: PathBuf,
    raw: RawManifest,
}

/// Typed view of the keys we consume. Unknown keys are ignored by serde
/// here (no `deny_unknown_fields`); they are surfaced separately by
/// [`Manifest::warn_unknown_keys`] via a second `toml::Table` parse.
#[derive(Debug, Default, Deserialize)]
struct RawManifest {
    paths: Option<Vec<PathBuf>>,
    exclude_from: Option<PathBuf>,
    /// Job count, matching the `--jobs` flag (issue #666). Accepted as
    /// either a string (`"auto"`) or an integer (`4`); the conversion to
    /// [`NumJobs`] happens in [`Manifest::num_jobs`]. The deprecated
    /// `num_jobs` spelling (issue #604/#666) is accepted as a one-cycle
    /// alias and warned about in [`warn_deprecated_renamed_keys`].
    #[serde(rename = "jobs", alias = "num_jobs")]
    jobs: Option<toml::Value>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    /// Deprecated top-level spelling of `[check] baseline` (#599). Read
    /// only as a fallback when the `[check]` value is absent, with a
    /// one-time deprecation warning; removed in the next major bump.
    baseline: Option<PathBuf>,
    /// Deprecated top-level spelling of `[check] baseline_line_tolerance`
    /// (#599). See [`RawManifest::baseline`].
    baseline_line_tolerance: Option<usize>,
    /// Deprecated top-level spelling of `[check] baseline_fuzzy_match`
    /// (#599). See [`RawManifest::baseline`].
    baseline_fuzzy_match: Option<bool>,
    /// When `false`, Rust's `?` operator does not contribute to
    /// cyclomatic complexity (#409). Defaults to counting (the key
    /// absent is equivalent to `true`). Mirrors the value-taking
    /// `--cyclomatic-count-try <bool>` flag (#666); the CLI value
    /// overrides this key in either direction.
    cyclomatic_count_try: Option<bool>,
    /// Deprecated top-level spelling of `[check] headroom` (#599). See
    /// [`RawManifest::baseline`].
    headroom: Option<f64>,
    /// Scalar values are hard limits; the nested `soft` sub-table
    /// (`[thresholds.soft]`, #375) carries the soft-tier overrides.
    /// [`split_thresholds_table`] separates the two layers.
    #[serde(default)]
    thresholds: BTreeMap<String, toml::Value>,
    /// The `[check]` table (#378): gate-only options that affect which
    /// offenders `bca check` emits, without changing what is walked /
    /// reported.
    #[serde(default)]
    check: RawCheck,
    /// The `[report]` table (#501): options for the aggregated
    /// `bca report markdown|html` hotspot tables.
    #[serde(default)]
    report: RawReport,
    /// The `[vcs]` table (#576): change-history ranking options. The CLI
    /// flag wins when both are present.
    #[serde(default)]
    vcs: RawVcs,
}

/// Typed view of the `[vcs]` table (#576). Mirrors the `bca vcs` CLI
/// flags; the CLI value replaces the manifest value when both are
/// present.
#[derive(Debug, Default, Deserialize)]
struct RawVcs {
    /// File-type scope for the change-history ranking: `metrics`, `all`,
    /// or a comma-separated extension allow-list. Parsed (and validated)
    /// by [`big_code_analysis::vcs::FileTypeScope`] at merge time;
    /// mirrors `--file-types`, which replaces this value.
    file_types: Option<String>,
}

/// Typed view of the `[report]` table (#501). Mirrors the `bca report`
/// CLI flags; the CLI value wins when both are present.
#[derive(Debug, Default, Deserialize)]
struct RawReport {
    /// When `true`, the aggregated report includes functions silenced by
    /// in-source suppression markers (the raw audit view). Mirrors the
    /// value-taking `--no-suppress <bool>` flag (#683); the CLI value
    /// overrides this key in either direction. Absent / `false` honors
    /// markers — the default that matches `bca check` and the SARIF
    /// emitter.
    no_suppress: Option<bool>,
}

/// Typed view of the `[check]` table (#378). Each key mirrors a CLI flag
/// or its manifest spelling; the CLI value overrides the manifest in
/// either direction.
#[derive(Debug, Default, Deserialize)]
struct RawCheck {
    /// Glob patterns whose matching files are exempt from the threshold
    /// gate (analysed and reported, but their violations are dropped).
    exclude: Option<Vec<String>>,
    /// Path to a `.gitignore`-style file of additional exclude globs.
    exclude_from: Option<PathBuf>,
    /// Exit-code style (#385/#666): `"default"` keeps the stable 0/1/2
    /// contract; `"tiered"` opts into the 2-5 severity split. Mirrors the
    /// value-taking `--exit-codes <default|tiered>` flag; the CLI value
    /// overrides this key in either direction.
    exit_codes: Option<String>,
    /// Baseline file `bca check` reads (and a bare `--write-baseline`
    /// writes). Mirrors `--baseline`; the CLI value wins. Canonical
    /// location since #599 (the top-level spelling is deprecated).
    baseline: Option<PathBuf>,
    /// Per-function line tolerance for fuzzy baseline matching (#599).
    /// Mirrors `--baseline-line-tolerance`.
    baseline_line_tolerance: Option<usize>,
    /// When `true`, baseline entries match on metric shape rather than
    /// exact line span (#599). Mirrors the value-taking
    /// `--baseline-fuzzy-match <bool>` flag (#683); the CLI value
    /// overrides this key in either direction.
    baseline_fuzzy_match: Option<bool>,
    /// Soft-tier scale ratio in `(0, 1]` (#599). Mirrors `--headroom`;
    /// the CLI value wins. Validated by [`Manifest::headroom`].
    headroom: Option<f64>,
}

/// Append `extra` onto `dst`, skipping any value already present so the
/// merged list is a duplicate-free union with CLI entries kept first
/// (#539). Exclude lists are tiny (a handful of globs), so the linear
/// membership check is clearer than a `HashSet` and cheaper in practice.
fn extend_dedup(dst: &mut Vec<String>, extra: impl IntoIterator<Item = String>) {
    for item in extra {
        if !dst.contains(&item) {
            dst.push(item);
        }
    }
}

/// Discover and load `bca.toml`. Returns `None` when no manifest exists
/// above the working directory. Dies (exit 1) on a read / UTF-8 / parse
/// error of a manifest that *does* exist — a malformed config must not
/// be silently ignored.
pub(crate) fn discover_and_load() -> Option<Manifest> {
    let path = discover()?;
    let dir = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

    let text = read_utf8_file(&path, "bca.toml");
    let raw: RawManifest =
        toml::from_str(&text).unwrap_or_else(|e| die_io("parse bca.toml", &path, e));

    // The typed parse above silently drops unknown keys; a second parse
    // into a generic table lets us enumerate and warn about them.
    warn_unknown_keys(&text);
    warn_deprecated_top_level_check_keys(&raw);
    warn_deprecated_renamed_keys(&text);
    Some(Manifest { dir, path, raw })
}

/// Manifest keys renamed in the 2.0 flag-alignment sweep (issue #666)
/// that are still honored under their old spelling for one release
/// cycle. The serde `alias` on each field accepts the legacy spelling
/// silently; this warning is what makes the deprecation visible. Keyed
/// off the raw `[top-level]` text so a `num_jobs` written in the file
/// draws the rename notice (not the misleading "unrecognized key"
/// warning, since the alias is on [`KNOWN_KEYS`]).
fn warn_deprecated_renamed_keys(text: &str) {
    // (legacy spelling, canonical spelling).
    const RENAMED: &[(&str, &str)] = &[("num_jobs", "jobs")];
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        return;
    };
    for (old, new) in RENAMED {
        if table.contains_key(*old) {
            warn(format_args!(
                "bca.toml: key `{old}` is deprecated and has been renamed \
                 to `{new}`; the old spelling will be removed in the next major release"
            ));
        }
    }
}

/// Keys that moved under `[check]` in #599 but are still honored at the
/// top level for one release cycle. Each tuple is (legacy top-level
/// key, the [`RawManifest`] field's presence) — used only for the
/// deprecation warning; the merge accessors read the values directly.
fn warn_deprecated_top_level_check_keys(raw: &RawManifest) {
    let legacy = [
        ("baseline", raw.baseline.is_some()),
        (
            "baseline_line_tolerance",
            raw.baseline_line_tolerance.is_some(),
        ),
        ("baseline_fuzzy_match", raw.baseline_fuzzy_match.is_some()),
        ("headroom", raw.headroom.is_some()),
    ];
    for (key, present) in legacy {
        if present {
            warn(format_args!(
                "bca.toml: top-level `{key}` is deprecated and has \
                 moved under `[check]`; the top-level spelling will be removed \
                 in the next major release"
            ));
        }
    }
}

/// Emit one stderr warning per unrecognized top-level key. Parses the
/// raw text a second time into a generic table because the typed
/// [`RawManifest`] silently drops anything it does not name.
fn warn_unknown_keys(text: &str) {
    for key in unknown_top_level_keys(text) {
        warn(format_args!(
            "bca.toml: ignoring unrecognized key `{key}` \
             (unknown option, or a feature not yet released)"
        ));
    }
}

/// Top-level keys present in `text` but absent from [`KNOWN_KEYS`].
///
/// Split out from [`warn_unknown_keys`] so the allowlist can be tested
/// directly: every field [`RawManifest`] consumes must be listed in
/// [`KNOWN_KEYS`], or it is silently honored while bca prints a
/// misleading "ignoring unrecognized key" warning (the #409
/// `cyclomatic_count_try` regression).
fn unknown_top_level_keys(text: &str) -> Vec<String> {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        // The typed parse already succeeded, so this cannot fail in
        // practice; if it somehow does, report no unknown keys.
        return Vec::new();
    };
    table
        .keys()
        .filter(|key| !KNOWN_KEYS.contains(&key.as_str()))
        .cloned()
        .collect()
}

/// Climb from the working directory to the repo root looking for
/// `bca.toml`. Stops at the first directory containing `.git` (the
/// manifest lives at or below the repo root by convention) or at the
/// filesystem root.
fn discover() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(MANIFEST_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
        // The dir holding `.git` is the repo root: check it (done above)
        // then stop, rather than escaping into a parent checkout.
        if dir.join(".git").exists() || !dir.pop() {
            return None;
        }
    }
}

impl Manifest {
    /// Full path to the discovered manifest (for provenance output).
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Resolve a manifest-relative path against the manifest directory.
    /// Absolute paths are returned unchanged.
    fn resolve(&self, p: &Path) -> PathBuf {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.dir.join(p)
        }
    }

    /// Merge global options into `g`. A non-empty `Vec` or a `Some`
    /// value means the user set the flag on the command line (clap
    /// cannot produce either from an unset arg), so the CLI wins.
    /// `num_jobs` is the lone scalar-with-default, so its CLI-vs-default
    /// state is passed in explicitly from the parsed `ArgMatches`.
    pub(crate) fn merge_globals(&self, g: &mut GlobalOpts, num_jobs_from_cli: bool) {
        // Positive scope keys (`paths`, `include`) are REPLACED by any
        // explicit CLI value: `bca check one.rs` with manifest
        // `paths = ["src"]` checks just `one.rs`. The empty-CLI fallback
        // applies the manifest only when the user passed nothing.
        if g.paths.is_empty()
            && let Some(paths) = &self.raw.paths
        {
            g.paths = paths.iter().map(|p| self.resolve(p)).collect();
        }
        if g.include.is_empty()
            && let Some(include) = &self.raw.include
        {
            g.include.clone_from(include);
        }
        // Negative filter keys (`exclude`) UNION CLI values with the
        // manifest list (#539): a CLI `--exclude` must never silently
        // un-exclude a directory the project config deliberately skipped
        // (e.g. `vendor/`). Mirrors ruff/ESLint `extend-exclude`. Dedup
        // preserves order, CLI patterns first. `--no-config` short-
        // circuits this by skipping the merge entirely (manifest is None),
        // so an explicit opt-out still yields CLI-only excludes.
        if let Some(exclude) = &self.raw.exclude {
            extend_dedup(&mut g.exclude, exclude.iter().cloned());
        }
        if g.exclude_from.is_none()
            && let Some(exclude_from) = &self.raw.exclude_from
        {
            g.exclude_from = Some(self.resolve(exclude_from));
        }
        if !num_jobs_from_cli && let Some(num_jobs) = self.num_jobs() {
            g.num_jobs = num_jobs;
        }
        // `--cyclomatic-count-try <bool>` is a full override (issue #666):
        // an explicit CLI value wins in either direction; the manifest
        // `cyclomatic_count_try` key fills in only when the CLI left it
        // unset. The positive sense is carried end-to-end, so the
        // downstream default (`?` counts) applies when both are absent.
        g.count_cyclomatic_try = g.count_cyclomatic_try.or(self.raw.cyclomatic_count_try);
    }

    /// Merge check-only options (`baseline`, `baseline_line_tolerance`,
    /// `baseline_fuzzy_match`, the soft-tier `headroom` ratio,
    /// `exit_codes`) into `args`. CLI values win.
    pub(crate) fn merge_check(&self, args: &mut CheckArgs) {
        // bca: suppress(cyclomatic)
        // Flat field-by-field config merge (`if args.x.is_none() { … }` per
        // key) — cyclomatic is guard count, not nested branching.
        // A manifest baseline must not be applied when the user is
        // *writing* one — `--baseline` and `--write-baseline` are
        // mutually exclusive, and clap's check ran before this merge.
        if args.baseline.is_none()
            && args.write_baseline.is_none()
            && let Some(baseline) = self.baseline()
        {
            args.baseline = Some(self.resolve(baseline));
        }
        // A bare `--write-baseline` (flag present, no path) writes to the
        // manifest's `baseline` — the same file `bca check` reads — so the
        // path lives in exactly one place (#496). Resolve it here, since
        // the read-baseline merge above is intentionally skipped on a
        // write run. With no manifest `baseline`, it stays `Some(None)`
        // and `run_check` reports the missing-path error.
        if matches!(args.write_baseline, Some(None))
            && let Some(baseline) = self.baseline()
        {
            args.write_baseline = Some(Some(self.resolve(baseline)));
        }
        if args.baseline_line_tolerance.is_none() {
            args.baseline_line_tolerance = self.baseline_line_tolerance();
        }
        // `--baseline-fuzzy-match <bool>` is a full override (#683): an
        // explicit CLI value wins in either direction; the manifest
        // `baseline_fuzzy_match` key fills in only when the CLI left it
        // unset. The downstream default (off) applies when both absent.
        args.baseline_fuzzy_match = args.baseline_fuzzy_match.or(self.baseline_fuzzy_match());
        // The `[check] headroom` key is the manifest spelling of the soft
        // tier's scale ratio (issue #688). `self.headroom()` validates the
        // range unconditionally — a malformed `headroom` must fail fast
        // (exit 1) even at the hard tier, where it is otherwise ignored.
        // Fold the validated ratio directly into the tier — `--tier=soft`
        // (a bare soft tier with no pinned ratio) inherits the manifest
        // ratio — rather than into the CLI-only `--headroom` alias field,
        // so it never trips that alias's deprecation-warning / both-set
        // conflict path. An explicit CLI `--tier=soft=<R>` or `--headroom
        // <R>` wins (the manifest fills only the bare-`soft` gap).
        if let Some(ratio) = self.headroom()
            && matches!(args.tier, crate::TierSpec::Soft(None))
            && args.headroom.is_none()
        {
            args.tier = crate::TierSpec::Soft(Some(ratio));
        }
        // `[check] exclude` / `exclude_from` (#378, #539). As a negative
        // filter key, `check_exclude` UNIONs CLI values with the manifest
        // list rather than letting the CLI replace it — a CLI
        // `--check-exclude` cannot silently re-gate a path the project
        // config deliberately exempted. The exclude-from path resolves
        // against the manifest directory like every other manifest path.
        if let Some(exclude) = &self.raw.check.exclude {
            extend_dedup(&mut args.check_exclude, exclude.iter().cloned());
        }
        if args.check_exclude_from.is_none()
            && let Some(exclude_from) = &self.raw.check.exclude_from
        {
            args.check_exclude_from = Some(self.resolve(exclude_from));
        }
        // `[check] exit_codes` (#385/#666). The value-taking
        // `--exit-codes <default|tiered>` flag is a full override: an
        // explicit CLI value wins in either direction, so the manifest
        // fills `args.exit_codes` only when the CLI left it unset. An
        // unrecognised value is a hard error rather than a silent default
        // — a typo (`exit_codes = "teired"`) must not quietly fall back
        // to the legacy contract.
        if args.exit_codes.is_none() {
            args.exit_codes = match self.raw.check.exit_codes.as_deref() {
                None => None,
                Some("default") => Some(crate::ExitCodes::Default),
                Some("tiered") => Some(crate::ExitCodes::Tiered),
                Some(other) => die(format_args!(
                    "bca.toml: [check] exit_codes must be \"default\" or \"tiered\"; got {other:?}"
                )),
            };
        }
    }

    /// Merge the gate-skipping defaults `bca exemptions` audits
    /// (`baseline`, `[check] exclude` / `exclude_from`) into `args`,
    /// mirroring [`Self::merge_check`] so the audit reflects exactly what
    /// `bca check` would skip. Positive keys (`baseline`) fill only when
    /// unset; the negative filter `check_exclude` UNIONs CLI values with
    /// the manifest list (#539). Threshold / headroom / exit-code keys are
    /// irrelevant to a read-only listing and are deliberately not merged
    /// here.
    pub(crate) fn merge_exemptions(&self, args: &mut ExemptionsArgs) {
        if args.baseline.is_none()
            && let Some(baseline) = self.baseline()
        {
            args.baseline = Some(self.resolve(baseline));
        }
        if let Some(exclude) = &self.raw.check.exclude {
            extend_dedup(&mut args.check_exclude, exclude.iter().cloned());
        }
        if args.check_exclude_from.is_none()
            && let Some(exclude_from) = &self.raw.check.exclude_from
        {
            args.check_exclude_from = Some(self.resolve(exclude_from));
        }
    }

    /// Merge `[report]` options into `args` (#501/#683). The value-taking
    /// `--no-suppress <bool>` flag is a full override: an explicit CLI
    /// value wins in either direction, so the manifest `no_suppress` key
    /// fills `args.no_suppress` only when the CLI left it unset. The
    /// downstream default (honor markers) applies when both are absent.
    pub(crate) fn merge_report(&self, args: &mut ReportArgs) {
        args.no_suppress = args.no_suppress.or(self.raw.report.no_suppress);
    }

    /// Merge `[vcs]` options into `args` (#576). `file_types` is a
    /// positive scope key, so it fills only when the CLI left
    /// `--file-types` unset — an explicit CLI flag replaces the manifest
    /// value (it never unions). Parsing / validation is deferred to
    /// [`build_options`](crate::vcs_command::build_options); the raw
    /// string is threaded through unchanged so both sources hit the same
    /// [`FileTypeScope`](big_code_analysis::vcs::FileTypeScope) parser and
    /// surface one diagnostic.
    pub(crate) fn merge_vcs(&self, args: &mut VcsArgs) {
        if args.file_types.is_none()
            && let Some(file_types) = &self.raw.vcs.file_types
        {
            args.file_types = Some(file_types.clone());
        }
    }

    /// The validated `headroom` ratio. Dies (exit 1) with a
    /// `bca.toml`-attributed message on an out-of-range value, so a bad
    /// manifest fails fast and clearly rather than borrowing the
    /// `--headroom` flag's wording from the downstream resolver. (The
    /// half-open `(0, 1]` interval matches `--headroom`; NaN, which
    /// fails both comparisons, is rejected too.)
    fn headroom(&self) -> Option<f64> {
        let ratio = self.raw.check.headroom.or(self.raw.headroom)?;
        if !crate::thresholds::is_valid_scale_ratio(ratio) {
            die(format_args!(
                "bca.toml: headroom must be in (0, 1]; got {ratio}"
            ));
        }
        Some(ratio)
    }

    /// The baseline path, preferring the canonical `[check] baseline`
    /// over the deprecated top-level spelling (#599). Returned
    /// unresolved; callers apply [`Self::resolve`].
    fn baseline(&self) -> Option<&Path> {
        self.raw
            .check
            .baseline
            .as_deref()
            .or(self.raw.baseline.as_deref())
    }

    /// The baseline line tolerance, preferring `[check]` over the
    /// deprecated top-level spelling (#599).
    fn baseline_line_tolerance(&self) -> Option<usize> {
        self.raw
            .check
            .baseline_line_tolerance
            .or(self.raw.baseline_line_tolerance)
    }

    /// The fuzzy-match flag, preferring `[check]` over the deprecated
    /// top-level spelling (#599).
    fn baseline_fuzzy_match(&self) -> Option<bool> {
        self.raw
            .check
            .baseline_fuzzy_match
            .or(self.raw.baseline_fuzzy_match)
    }

    /// The `[thresholds]` table split into its hard scalar limits and
    /// the optional `[thresholds.soft]` overrides (#375). Dies (exit 1)
    /// on a malformed table — a bad limit must fail fast, not silently
    /// vanish from the gate. Shares [`split_thresholds_table`] with the
    /// `--config` path so both surfaces parse identically.
    pub(crate) fn thresholds(&self) -> ParsedThresholds {
        split_thresholds_table(&self.raw.thresholds)
            .unwrap_or_else(|e| die(format_args!("bca.toml: {e}")))
    }

    /// Convert the `num_jobs` value (string `"auto"` or an integer) to
    /// [`NumJobs`]. Dies (exit 1) on an out-of-range or wrong-typed
    /// value, reusing [`NumJobs::from_str`]'s diagnostics.
    fn num_jobs(&self) -> Option<NumJobs> {
        let value = self.raw.jobs.as_ref()?;
        // Normalise every arm to `Result<_, String>` for the `die`
        // formatter: `NumJobs::from_str` now returns the typed
        // `ParseNumJobsError`, rendered via `Display` here.
        let parsed = match value {
            toml::Value::String(s) => NumJobs::from_str(s).map_err(|e| e.to_string()),
            // Route the integer through `from_str` (the small `to_string`
            // is once-per-run) so the `>= 1` validation and its error
            // message live in exactly one place; a negative integer
            // surfaces the same "positive integer or auto" diagnostic.
            toml::Value::Integer(i) => NumJobs::from_str(&i.to_string()).map_err(|e| e.to_string()),
            other => Err(format!(
                "expected a positive integer or \"auto\", got {}",
                other.type_str()
            )),
        };
        Some(parsed.unwrap_or_else(|e| die(format_args!("bca.toml: num_jobs: {e}"))))
    }
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
