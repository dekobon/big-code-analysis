//! `bca check --print-effective-config`: the resolved-config view and its serializer.

use super::super::*;
use super::*;
use crate::walk_seed::ManifestExcludes;

/// The exclude list to *report*: the caller's globs unioned with the
/// manifest's. Matching keeps the two apart so each keeps its own
/// anchor (#1164); the resolved set a reader asks
/// `--print-effective-config` for is still the union.
fn union_globs(cli: &[String], manifest: Option<&ManifestExcludes>) -> Vec<String> {
    manifest.map_or_else(|| cli.to_vec(), |m| m.union_globs(cli))
}

/// The exclude-from file in effect: the caller's when given, else the
/// manifest's (already resolved against the manifest directory).
fn display_globs_from(cli: Option<&Path>, manifest: Option<&ManifestExcludes>) -> Option<String> {
    cli.or_else(|| manifest.and_then(|m| m.globs_from.as_deref()))
        .map(|p| p.display().to_string())
}

/// Serialize the resolved threshold/check configuration to stdout.
/// Used by `--print-effective-config` to surface the post-merge view
/// of every layer (`--config` TOML + repeated `--threshold` CLI
/// overrides) without running the check.
///
/// The output shape is intentionally a strict superset of what
/// `--config` consumes: piping the TOML form back through `--config`
/// reproduces the same `ThresholdSet`. JSON is offered for tooling
/// pipelines (CI dashboards, IDE plugins) that prefer structured data
/// over TOML — the same field names; same shape.
///
/// The resolved layers (headroom scaling per #373, `[thresholds.soft]`
/// / `--tier` per #375, the tiered exit-code style per #385, the
/// per-language tables per #1141) are already folded into the serialized
/// view; future layers (baseline state per #381) will extend
/// `EffectiveConfig` additively. This printer is the single place that
/// needs to learn about them.
pub(crate) fn print_effective_config(
    globals: &GlobalOpts,
    args: &CheckArgs,
    thresholds: &LanguageThresholds,
    manifest: Option<&Manifest>,
    format: PrintConfigFormat,
    tier: TierSpec,
    tiered_exit_codes: bool,
) {
    let effective = EffectiveConfig::from_resolved(
        globals,
        args,
        thresholds,
        manifest,
        tier,
        tiered_exit_codes,
    );
    let serialized = match format {
        PrintConfigFormat::Toml => toml::to_string_pretty(&effective)
            .unwrap_or_else(|e| die(format_args!("serialize effective config to TOML: {e}"))),
        PrintConfigFormat::Json => serde_json::to_string_pretty(&effective)
            .unwrap_or_else(|e| die(format_args!("serialize effective config to JSON: {e}"))),
    };
    write_stdout_or_die(serialized.as_bytes());
    // TOML's `to_string_pretty` already ends with a newline; JSON's
    // `to_string_pretty` does not. Normalize so consumers piping into
    // `--config` or `jq` see a clean trailing newline either way.
    if !serialized.ends_with('\n') {
        write_stdout_or_die(b"\n");
    }
}

/// Resolved view of `bca check` configuration after layer merge.
///
/// Mirrors the [`ThresholdConfig`][crate::thresholds::ThresholdConfig]
/// schema for the `[thresholds]` table so the TOML form is directly
/// consumable via `--config`. The `[check]` table reports the
/// filtering/scoping inputs (paths, include/exclude globs, suppression
/// policy, etc.) that affect which functions are even considered for
/// threshold comparison; those fields are informational and ignored
/// by `--config`.
#[derive(serde::Serialize)]
pub(crate) struct EffectiveConfig {
    pub(crate) thresholds: EffectiveThresholds,
    pub(crate) check: EffectiveCheck,
}

/// The resolved `[thresholds]` view: the global limits, plus one
/// *fully resolved* sub-table per language carrying a
/// `[thresholds.lang.<slug>]` override (#1141).
///
/// Each language table lists every limit that will apply to that
/// language, inherited entries included — not a diff against the global
/// table. Someone auditing a gate wants the number that fires, not a
/// delta to compute; and the printed form stays directly consumable by
/// `--config`, which reads the same nesting.
pub(crate) struct EffectiveThresholds {
    pub(crate) global: BTreeMap<String, f64>,
    pub(crate) lang: BTreeMap<&'static str, BTreeMap<String, f64>>,
}

impl serde::Serialize for EffectiveThresholds {
    /// Hand-written because this one TOML table holds two value shapes —
    /// scalar limits and a nested table of tables — which a single
    /// `BTreeMap` cannot express without an enum wrapper around every
    /// limit. Serializing from the two typed fields keeps the shape
    /// explicit and puts the language tables last in both TOML and JSON.
    ///
    /// The `toml` serializer additionally reorders values ahead of
    /// tables on its own, so the emitted order here is for readability
    /// and for the JSON form; do not read it as the thing that keeps the
    /// TOML valid.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;

        let has_lang = !self.lang.is_empty();
        let mut map = serializer.serialize_map(Some(self.global.len() + usize::from(has_lang)))?;
        for (name, limit) in &self.global {
            map.serialize_entry(name, limit)?;
        }
        if has_lang {
            map.serialize_entry(crate::threshold_lang::LANG_SUBTABLE_KEY, &self.lang)?;
        }
        map.end()
    }
}

#[derive(serde::Serialize)]
pub(crate) struct EffectiveCheck {
    pub(crate) paths: Vec<String>,
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) exclude_from: Option<String>,
    /// Resolved `[check.exclude]` globs (#378): files analysed and
    /// reported but exempt from the gate. Empty when unset.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) check_exclude: Vec<String>,
    /// Source file for additional `check_exclude` globs
    /// (`--check-exclude-from` / `[check] exclude_from`), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) check_exclude_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) paths_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) baseline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) config: Option<String>,
    /// Path to the auto-discovered `bca.toml` whose keys were merged
    /// under the CLI flags, if any. Provenance for the resolved view:
    /// signals that values not traceable to a flag came from here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) manifest: Option<String>,
    pub(crate) no_fail: bool,
    pub(crate) no_suppress: bool,
    /// Whether `--report-suppressed` is active: suppressed/baselined debt
    /// is kept in the SARIF offender document (as `suppressions` entries)
    /// instead of dropped. Surfaced here because it changes the emitted
    /// offender set — a gate-relevant input the resolved view previously
    /// omitted (#704).
    pub(crate) report_suppressed: bool,
    pub(crate) no_ignore: bool,
    pub(crate) no_skip_generated: bool,
    pub(crate) exclude_tests: bool,
    pub(crate) changed_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) since: Option<String>,
    /// The soft-tier scale ratio applied to the config-derived limits,
    /// if any (the `RATIO` in `--tier=soft=RATIO`, issue #688). Recorded
    /// for provenance: the `[thresholds]` table above already shows the
    /// post-scaling values, so this is the one signal that distinguishes
    /// "limit 14.25 because config said 15 × 0.95" from "limit 14.25
    /// because config literally said 14.25".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) headroom: Option<f64>,
    /// Which tier the `thresholds` table above was resolved for
    /// (`"hard"` or `"soft"`, issue #375). The limits shown already
    /// reflect any `[thresholds.soft]` merge or soft-ratio scaling, so
    /// this field is the one signal that records *which* tier produced
    /// them.
    pub(crate) tier: &'static str,
    /// Which exit-code contract is in force (#385/#666): `"default"`
    /// (the stable 0/1/2 codes) or `"tiered"` (the 2-5 severity split,
    /// enabled by `--exit-codes=tiered` or `[check] exit_codes`).
    pub(crate) exit_codes: &'static str,
    /// The `--baseline-line-tolerance` override, if set (issue #377).
    /// Absent means the built-in default applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) baseline_line_tolerance: Option<usize>,
    /// Whether the body-hash fuzzy fallback (`--baseline-fuzzy-match`)
    /// is active. Only meaningful alongside `baseline`.
    pub(crate) baseline_fuzzy_match: bool,
}

/// One resolved set's `(metric, limit)` pairs, in the shape a
/// serialized `[thresholds]` table takes. Shared by the global table and
/// every per-language one so the two render identically.
fn resolved_limits(set: &ThresholdSet) -> BTreeMap<String, f64> {
    set.iter()
        .map(|(name, limit)| (name.to_owned(), limit))
        .collect()
}

impl EffectiveConfig {
    /// Project the resolved `ThresholdSet` + the original CLI args into
    /// a serializable view. Paths are rendered with [`Path::display`]
    /// because the printed config is informational; `--config` only
    /// reads the `[thresholds]` table back, where keys/values are pure
    /// ASCII metric names + numbers and round-trip exactly.
    pub(crate) fn from_resolved(
        globals: &GlobalOpts,
        args: &CheckArgs,
        resolved: &LanguageThresholds,
        manifest: Option<&Manifest>,
        tier: TierSpec,
        tiered_exit_codes: bool,
    ) -> Self {
        let thresholds = EffectiveThresholds {
            global: resolved_limits(resolved.global()),
            lang: resolved
                .languages()
                .map(|(slug, set)| (slug, resolved_limits(set)))
                .collect(),
        };
        let check = EffectiveCheck {
            paths: globals
                .paths
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            include: globals.include.clone(),
            // Manifest globs are matched from their own set so they keep
            // the manifest directory as their anchor (#1164), but what a
            // reader wants reported is the resolved set both halves add
            // up to.
            exclude: union_globs(&globals.exclude, globals.manifest_excludes.as_ref()),
            exclude_from: display_globs_from(
                globals.exclude_from.as_deref(),
                globals.manifest_excludes.as_ref(),
            ),
            check_exclude: union_globs(&args.check_exclude, args.manifest_check_exclude.as_ref()),
            check_exclude_from: display_globs_from(
                args.check_exclude_from.as_deref(),
                args.manifest_check_exclude.as_ref(),
            ),
            paths_from: globals.paths_from.as_ref().map(|p| p.display().to_string()),
            baseline: args.baseline.as_ref().map(|p| p.display().to_string()),
            config: args.config.as_ref().map(|p| p.display().to_string()),
            manifest: manifest.map(|m| m.path().display().to_string()),
            no_fail: args.no_fail,
            no_suppress: args.no_suppress,
            report_suppressed: args.report_suppressed,
            no_ignore: globals.no_ignore,
            no_skip_generated: globals.no_skip_generated,
            exclude_tests: globals.exclude_tests,
            changed_only: args.changed_only,
            since: args.since.clone(),
            headroom: tier.ratio(),
            tier: tier.tier().as_str(),
            exit_codes: if tiered_exit_codes {
                "tiered"
            } else {
                "default"
            },
            baseline_line_tolerance: args.baseline_line_tolerance,
            baseline_fuzzy_match: args.baseline_fuzzy_match.unwrap_or(false),
        };
        Self { thresholds, check }
    }
}
