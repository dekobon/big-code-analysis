//! `bca.toml` manifest discovery and merge (issue #374).
//!
//! Consolidates the flags every local-gate recipe used to thread
//! through each invocation (`--paths`, `--exclude-from`, `--num-jobs`,
//! `--config`, `--baseline`, `--headroom`) into one discoverable file
//! at the repo root.
//!
//! # Resolution order
//!
//! Per the documented order shared across #373/#374/#375/#380:
//!
//! 1. Manifest `[thresholds]` is the base layer.
//! 2. `--config <file>` merges on top (config keys win on collision).
//! 3. `--headroom` scales the merged config-derived limits.
//! 4. Repeated `--threshold name=value` CLI flags apply last, absolutely.
//!
//! For the global options (`paths`, `exclude_from`, `num_jobs`,
//! `include`, `exclude`) and the check-only options (`baseline`,
//! `headroom`), an explicit CLI value always wins over the manifest.
//!
//! Relative paths in the manifest are resolved against the manifest's
//! own directory (Cargo-style), so a `bca.toml` discovered above the
//! current working directory still points at the right files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;

use crate::thresholds::{ParsedThresholds, split_thresholds_table};
use crate::{CheckArgs, ExemptionsArgs, GlobalOpts, NumJobs, die, die_io, read_utf8_file};

/// Filename discovered by convention at (or above) the working directory.
const MANIFEST_FILE: &str = "bca.toml";

/// Top-level manifest keys understood today. Any other top-level key
/// triggers a one-line "ignored" warning, so unreleased options can be
/// pre-adopted without breaking older `bca` builds. The
/// `[thresholds.soft]` sub-table (#375) is *not* a top-level key — it
/// lives under the known `thresholds` key and is split out by
/// [`split_thresholds_table`]. The `[check]` table (#378/#385) carries
/// gate-only options (`exclude`, `exclude_from`, `exit_codes`) and is
/// consumed as the typed [`RawCheck`].
const KNOWN_KEYS: &[&str] = &[
    "paths",
    "exclude_from",
    "num_jobs",
    "include",
    "exclude",
    "baseline",
    "baseline_line_tolerance",
    "baseline_fuzzy_match",
    "headroom",
    "thresholds",
    "check",
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
    /// Accepted as either a string (`"auto"`) or an integer (`4`); the
    /// conversion to [`NumJobs`] happens in [`Manifest::num_jobs`].
    num_jobs: Option<toml::Value>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    baseline: Option<PathBuf>,
    baseline_line_tolerance: Option<usize>,
    baseline_fuzzy_match: Option<bool>,
    /// When `false`, Rust's `?` operator does not contribute to
    /// cyclomatic complexity (#409). Defaults to counting (the key
    /// absent is equivalent to `true`). The `--no-cyclomatic-try` CLI
    /// flag ORs on top: it can force opt-out but cannot force counting
    /// back on, mirroring `--strict-exit-codes`.
    cyclomatic_count_try: Option<bool>,
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
}

/// Typed view of the `[check]` table (#378). Both keys mirror a CLI
/// flag (`--check-exclude`, `--check-exclude-from`); the CLI value wins
/// when both are present.
#[derive(Debug, Default, Deserialize)]
struct RawCheck {
    /// Glob patterns whose matching files are exempt from the threshold
    /// gate (analysed and reported, but their violations are dropped).
    exclude: Option<Vec<String>>,
    /// Path to a `.gitignore`-style file of additional exclude globs.
    exclude_from: Option<PathBuf>,
    /// Exit-code style (#385): `"default"` keeps the stable 0/1/2
    /// contract; `"tiered"` opts into the 2-5 severity split. Mirrors
    /// `--strict-exit-codes`, which ORs on top (the CLI flag can only
    /// enable, never disable).
    exit_codes: Option<String>,
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
    Some(Manifest { dir, path, raw })
}

/// Emit one stderr warning per unrecognized top-level key. Parses the
/// raw text a second time into a generic table because the typed
/// [`RawManifest`] silently drops anything it does not name.
fn warn_unknown_keys(text: &str) {
    let Ok(table) = toml::from_str::<toml::Table>(text) else {
        // The typed parse already succeeded, so this cannot fail in
        // practice; if it somehow does, skip the advisory warnings.
        return;
    };
    for key in table.keys() {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            eprintln!(
                "warning: bca.toml: ignoring unrecognized key `{key}` \
                 (unknown option, or a feature not yet released)"
            );
        }
    }
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
        if g.exclude.is_empty()
            && let Some(exclude) = &self.raw.exclude
        {
            g.exclude.clone_from(exclude);
        }
        if g.exclude_from.is_none()
            && let Some(exclude_from) = &self.raw.exclude_from
        {
            g.exclude_from = Some(self.resolve(exclude_from));
        }
        if !num_jobs_from_cli && let Some(num_jobs) = self.num_jobs() {
            g.num_jobs = num_jobs;
        }
        // `--no-cyclomatic-try` ORs on top: a CLI opt-out cannot be
        // undone by the manifest, but the manifest can opt out when the
        // flag is absent (#409).
        if !g.no_cyclomatic_try && self.raw.cyclomatic_count_try == Some(false) {
            g.no_cyclomatic_try = true;
        }
    }

    /// Merge check-only options (`baseline`, `baseline_line_tolerance`,
    /// `baseline_fuzzy_match`, `headroom`) into `args`. CLI values win.
    pub(crate) fn merge_check(&self, args: &mut CheckArgs) {
        // A manifest baseline must not be applied when the user is
        // *writing* one — `--baseline` and `--write-baseline` are
        // mutually exclusive, and clap's check ran before this merge.
        if args.baseline.is_none()
            && args.write_baseline.is_none()
            && let Some(baseline) = &self.raw.baseline
        {
            args.baseline = Some(self.resolve(baseline));
        }
        if args.baseline_line_tolerance.is_none() {
            args.baseline_line_tolerance = self.raw.baseline_line_tolerance;
        }
        // A bare `--baseline-fuzzy-match` flag (clap `bool`) cannot
        // represent "unset", so the manifest only *enables* fuzzy
        // matching; it can never override an explicit CLI opt-out
        // (there is no opt-out flag). OR the two sources together.
        if self.raw.baseline_fuzzy_match == Some(true) {
            args.baseline_fuzzy_match = true;
        }
        if args.headroom.is_none() {
            args.headroom = self.headroom();
        }
        // `[check] exclude` / `exclude_from` (#378). CLI wins: apply the
        // manifest list only when the CLI provided nothing. The
        // exclude-from path resolves against the manifest directory like
        // every other manifest path.
        if args.check_exclude.is_empty()
            && let Some(exclude) = &self.raw.check.exclude
        {
            args.check_exclude.clone_from(exclude);
        }
        if args.check_exclude_from.is_none()
            && let Some(exclude_from) = &self.raw.check.exclude_from
        {
            args.check_exclude_from = Some(self.resolve(exclude_from));
        }
        // `[check] exit_codes` (#385). A bare `--strict-exit-codes` flag
        // cannot represent "off", so the manifest can only *enable* the
        // tiered mode; it never overrides an explicit CLI opt-in. OR the
        // two sources, mirroring `baseline_fuzzy_match`. An unrecognised
        // value is a hard error rather than a silent default — a typo
        // (`exit_codes = "teired"`) must not quietly fall back to the
        // legacy contract.
        match self.raw.check.exit_codes.as_deref() {
            None | Some("default") => {}
            Some("tiered") => args.strict_exit_codes = true,
            Some(other) => die(format_args!(
                "bca.toml: [check] exit_codes must be \"default\" or \"tiered\"; got {other:?}"
            )),
        }
    }

    /// Merge the gate-skipping defaults `bca exemptions` audits
    /// (`baseline`, `[check] exclude` / `exclude_from`) into `args`. CLI
    /// values win, mirroring [`Self::merge_check`]'s CLI-replaces-manifest
    /// semantics so the audit reflects exactly what `bca check` would
    /// skip. Threshold / headroom / exit-code keys are irrelevant to a
    /// read-only listing and are deliberately not merged here.
    pub(crate) fn merge_exemptions(&self, args: &mut ExemptionsArgs) {
        if args.baseline.is_none()
            && let Some(baseline) = &self.raw.baseline
        {
            args.baseline = Some(self.resolve(baseline));
        }
        if args.check_exclude.is_empty()
            && let Some(exclude) = &self.raw.check.exclude
        {
            args.check_exclude.clone_from(exclude);
        }
        if args.check_exclude_from.is_none()
            && let Some(exclude_from) = &self.raw.check.exclude_from
        {
            args.check_exclude_from = Some(self.resolve(exclude_from));
        }
    }

    /// The validated `headroom` ratio. Dies (exit 1) with a
    /// `bca.toml`-attributed message on an out-of-range value, so a bad
    /// manifest fails fast and clearly rather than borrowing the
    /// `--headroom` flag's wording from the downstream resolver. (The
    /// half-open `(0, 1]` interval matches `--headroom`; NaN, which
    /// fails both comparisons, is rejected too.)
    fn headroom(&self) -> Option<f64> {
        let ratio = self.raw.headroom?;
        if !crate::thresholds::is_valid_scale_ratio(ratio) {
            die(format_args!(
                "bca.toml: headroom must be in (0, 1]; got {ratio}"
            ));
        }
        Some(ratio)
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
        let value = self.raw.num_jobs.as_ref()?;
        let parsed = match value {
            toml::Value::String(s) => NumJobs::from_str(s),
            // Route the integer through `from_str` (the small `to_string`
            // is once-per-run) so the `>= 1` validation and its error
            // message live in exactly one place; a negative integer
            // surfaces the same "positive integer or auto" diagnostic.
            toml::Value::Integer(i) => NumJobs::from_str(&i.to_string()),
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
