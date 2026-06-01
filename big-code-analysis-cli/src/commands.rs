//! Top-level command dispatch for the `bca` CLI.
//!
//! Owns the public `run()` entry point (called by `bca`'s `main` and
//! by `xtask` for man-page rendering), the per-command helpers
//! (`run_command_*`), the `check` subcommand pipeline (`run_check` +
//! its stage helpers), and the per-file footer rendering used by
//! `bca check`'s stderr output.
//!
//! All other CLI plumbing — argument types, the parallel walker, the
//! legacy-hint scaffolding — lives in `lib.rs` / sibling submodules.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches};

use big_code_analysis::{Count, PreprocResults, SuppressionPolicy};
use big_code_analysis::{fix_includes, write_file};

use crate::baseline::{self, Coverage};
use crate::baseline_diff::{BaselineDiff, SectionFilter};
use crate::check_format::{self, violation_to_offender};
use crate::diff;
use crate::exemptions::{BaselineRow, BaselineSection, ExemptionsReport, FileMarkers, MarkerRow};
use crate::format_util::MetricScalar;
use crate::formats::{CBOR_STDOUT_ERROR, MetricsDispatch, MetricsFormat, ReportFormat};
use crate::html_report::generate_html_report;
use crate::manifest::{self, Manifest};
use crate::markdown_report::{FunctionSummary, generate_report};
use crate::metric_catalog::write_metrics;
use crate::thresholds::{
    ParsedThresholds, SoftLimit, ThresholdSet, Violation, render_violation_line, scale_threshold,
};
use crate::{
    Action, CheckArgs, Cli, Command, Config, DiffBaselineArgs, ExemptionsArgs, GlobalOpts,
    InitArgs, ListMetricsArgs, NodesArgs, OutputFormat, PreprocArgs, PrintConfigFormat, ReportArgs,
    StripCommentsArgs, StructuredArgs, Tier, die, die_io, legacy_hint, load_baseline,
    load_preproc_data, load_threshold_config, read_exclude_patterns_from, run_walk, write_atomic,
    write_stdout_or_die,
};

fn run_check(
    globals: GlobalOpts,
    mut args: CheckArgs,
    manifest: Option<&Manifest>,
    preproc: Option<Arc<PreprocResults>>,
) {
    // Merge the check-only manifest keys (baseline / headroom) under the
    // CLI flags, and take the `[thresholds]` table (hard + soft layers)
    // as the base for the resolver. `--config` merges on top of it;
    // `--threshold` overrides win last (see
    // `validate_and_build_thresholds`).
    let base_thresholds = match manifest {
        Some(m) => {
            m.merge_check(&mut args);
            m.thresholds()
        }
        None => ParsedThresholds::default(),
    };
    let ResolvedThresholds {
        set,
        hard_limits,
        provenance,
    } = validate_and_build_thresholds(&args, base_thresholds);
    // `--print-effective-config` is a read-only debug aid: print the
    // resolved configuration and exit 0 before the walk. clap already
    // rejects pairing with `--write-baseline` (conflicts_with), so by
    // the time we get here the flag is unambiguous.
    if let Some(format) = args.print_effective_config {
        print_effective_config(&globals, &args, &set, manifest, format);
        return;
    }
    let scope = resolve_diff_scope(&args);
    // Clone globals for the remediation builder: `run_check_walk`
    // consumes `globals` by value (it passes through to `run_walk`
    // which spawns worker threads with ownership), but
    // `format_remediation_block` needs the resolved `--paths` /
    // `--exclude` set to compose a copy-paste-safe refresh command.
    let globals_for_remediation = globals.clone();
    let (violations, files_dispatched) = run_check_walk(globals, &args, preproc, set);

    if files_dispatched.load(Ordering::Relaxed) == 0 {
        // No files survived `--paths` expansion + `--include`/`--exclude`
        // filtering. Treat this as a tool error (exit 1), not a clean
        // pass (exit 0): a typo in `--paths` would otherwise silently
        // green-light CI.
        die("bca check: no input files matched; check --paths, --include, --exclude");
    }

    // Drop offenders from `[check.exclude]` files (#378) before *any*
    // downstream consumer sees them — so `--write-baseline` never
    // records the structural exemptions and the gate never fails on
    // them. Applied after the empty-input guard above: exempt files are
    // still walked and counted, only their violations are dropped.
    let violations = apply_check_exclude(violations, &args);

    if let Some(path) = args.write_baseline.as_deref() {
        write_check_baseline(violations, path, provenance);
        return;
    }

    let pairs = filter_by_baseline(
        violations,
        args.baseline.as_deref(),
        args.baseline_line_tolerance
            .unwrap_or(baseline::DEFAULT_LINE_TOLERANCE),
        args.baseline_fuzzy_match,
        provenance,
    );
    let pairs = apply_changed_only(pairs, scope.as_ref(), args.changed_only);
    let any_violations = !pairs.is_empty();
    // Categorise the kept violations for the exit-code contract (#385)
    // before `emit_check_results` consumes `pairs`.
    let outcome = classify_check_outcome(&pairs, args.tier, &hard_limits);
    // Build the remediation block ONLY when we have something to
    // remediate. Empty pairs (clean run) get no trailing block —
    // there is no baseline to refresh and no artifact worth pointing
    // at.
    let remediation = if any_violations {
        format_remediation_block(&globals_for_remediation, &args)
    } else {
        None
    };
    emit_check_results(pairs, &args, scope.as_ref(), remediation.as_deref());

    // `--no-fail` always forces exit 0; otherwise map the outcome to the
    // process exit code (tiered when `--strict-exit-codes` is set, the
    // stable 0/1/2 contract otherwise). A clean run returns `None` and
    // the process exits 0 implicitly.
    if !args.no_fail
        && let Some(code) = outcome.exit_code(args.strict_exit_codes)
    {
        process::exit(code);
    }
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
/// / `--tier` per #375, the tiered exit-code style per #385) are already
/// folded into the serialized view; future layers (baseline state per
/// #381) will extend `EffectiveConfig` additively. This printer is the
/// single place that needs to learn about them.
fn print_effective_config(
    globals: &GlobalOpts,
    args: &CheckArgs,
    set: &ThresholdSet,
    manifest: Option<&Manifest>,
    format: PrintConfigFormat,
) {
    let effective = EffectiveConfig::from_resolved(globals, args, set, manifest);
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
struct EffectiveConfig {
    thresholds: BTreeMap<String, f64>,
    check: EffectiveCheck,
}

#[derive(serde::Serialize)]
struct EffectiveCheck {
    paths: Vec<String>,
    include: Vec<String>,
    exclude: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exclude_from: Option<String>,
    /// Resolved `[check.exclude]` globs (#378): files analysed and
    /// reported but exempt from the gate. Empty when unset.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    check_exclude: Vec<String>,
    /// Source file for additional `check_exclude` globs
    /// (`--check-exclude-from` / `[check] exclude_from`), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    check_exclude_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    paths_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<String>,
    /// Path to the auto-discovered `bca.toml` whose keys were merged
    /// under the CLI flags, if any. Provenance for the resolved view:
    /// signals that values not traceable to a flag came from here.
    #[serde(skip_serializing_if = "Option::is_none")]
    manifest: Option<String>,
    no_fail: bool,
    no_suppress: bool,
    no_ignore: bool,
    no_skip_generated: bool,
    exclude_tests: bool,
    changed_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
    /// The `--headroom` ratio applied to the config-derived limits, if
    /// any. Recorded for provenance: the `[thresholds]` table above
    /// already shows the post-scaling values, so this is the one signal
    /// that distinguishes "limit 14.25 because config said 15 × 0.95"
    /// from "limit 14.25 because config literally said 14.25".
    #[serde(skip_serializing_if = "Option::is_none")]
    headroom: Option<f64>,
    /// Which tier the `thresholds` table above was resolved for
    /// (`"hard"` or `"soft"`, issue #375). The limits shown already
    /// reflect any `[thresholds.soft]` merge or `--headroom` scaling, so
    /// this field is the one signal that records *which* tier produced
    /// them.
    tier: &'static str,
    /// Which exit-code contract is in force (#385): `"default"` (the
    /// stable 0/1/2 codes) or `"tiered"` (the 2-5 severity split,
    /// enabled by `--strict-exit-codes` or `[check] exit_codes`).
    exit_codes: &'static str,
    /// The `--baseline-line-tolerance` override, if set (issue #377).
    /// Absent means the built-in default applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline_line_tolerance: Option<usize>,
    /// Whether the body-hash fuzzy fallback (`--baseline-fuzzy-match`)
    /// is active. Only meaningful alongside `baseline`.
    baseline_fuzzy_match: bool,
}

impl EffectiveConfig {
    /// Project the resolved `ThresholdSet` + the original CLI args into
    /// a serializable view. Paths are rendered with [`Path::display`]
    /// because the printed config is informational; `--config` only
    /// reads the `[thresholds]` table back, where keys/values are pure
    /// ASCII metric names + numbers and round-trip exactly.
    fn from_resolved(
        globals: &GlobalOpts,
        args: &CheckArgs,
        set: &ThresholdSet,
        manifest: Option<&Manifest>,
    ) -> Self {
        let thresholds: BTreeMap<String, f64> = set
            .iter()
            .map(|(name, limit)| (name.to_owned(), limit))
            .collect();
        let check = EffectiveCheck {
            paths: globals
                .paths
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            include: globals.include.clone(),
            exclude: globals.exclude.clone(),
            exclude_from: globals
                .exclude_from
                .as_ref()
                .map(|p| p.display().to_string()),
            check_exclude: args.check_exclude.clone(),
            check_exclude_from: args
                .check_exclude_from
                .as_ref()
                .map(|p| p.display().to_string()),
            paths_from: globals.paths_from.as_ref().map(|p| p.display().to_string()),
            baseline: args.baseline.as_ref().map(|p| p.display().to_string()),
            config: args.config.as_ref().map(|p| p.display().to_string()),
            manifest: manifest.map(|m| m.path().display().to_string()),
            no_fail: args.no_fail,
            no_suppress: args.no_suppress,
            no_ignore: globals.no_ignore,
            no_skip_generated: globals.no_skip_generated,
            exclude_tests: globals.exclude_tests,
            changed_only: args.changed_only,
            since: args.since.clone(),
            headroom: args.headroom,
            tier: args.tier.as_str(),
            exit_codes: if args.strict_exit_codes {
                "tiered"
            } else {
                "default"
            },
            baseline_line_tolerance: args.baseline_line_tolerance,
            baseline_fuzzy_match: args.baseline_fuzzy_match,
        };
        Self { thresholds, check }
    }
}

/// Default soft-tier scale applied when `--tier=soft` is requested with
/// neither a `[thresholds.soft]` table nor an explicit `--headroom`. A
/// concrete default keeps `--tier=soft` from being a silent no-op (the
/// "config error" the issue #375 resolution order warns against) — it
/// always produces a band tighter than the hard gate.
const DEFAULT_SOFT_HEADROOM: f64 = 0.95;

/// Resolved threshold layers handed back to [`run_check`].
///
/// `set` is the gate the walker compares against (the requested tier's
/// limits). `hard_limits` is the hard-tier limit per metric — equal to
/// `set` at the hard tier, but the *un-scaled* ceilings at the soft
/// tier, so [`classify_check_outcome`] can tell a soft-band
/// encroachment apart from a true hard breach (#385).
struct ResolvedThresholds {
    set: Arc<ThresholdSet>,
    hard_limits: BTreeMap<String, f64>,
    /// Tier/headroom the gate resolved to (issue #486). Stamped into the
    /// baseline on `--write-baseline` and compared against a loaded
    /// baseline's recorded provenance to warn on a stricter-than-baseline
    /// desync.
    provenance: baseline::Provenance,
}

/// Reduce the resolved tier + soft-table presence + headroom to the
/// [`baseline::Provenance`] stamped on a write and compared on a read
/// (issue #486). Mirrors the tier-resolution branches in
/// [`resolve_tier`]: hard → no scaling; soft with a `[thresholds.soft]`
/// table → per-metric limits (no single ratio); soft without a table →
/// scaled by `--headroom` (defaulting to [`DEFAULT_SOFT_HEADROOM`]).
fn resolve_provenance(
    tier: Tier,
    soft_table_present: bool,
    headroom: Option<f64>,
) -> baseline::Provenance {
    match tier {
        Tier::Hard => baseline::Provenance::hard(),
        Tier::Soft if soft_table_present => baseline::Provenance::soft_table(),
        Tier::Soft => {
            baseline::Provenance::soft_headroom(headroom.unwrap_or(DEFAULT_SOFT_HEADROOM))
        }
    }
}

/// Validate `--output` / `--output-format` pairing, then resolve the
/// effective threshold set per the documented resolution order
/// (#373/#374/#375/#380): the manifest `[thresholds]` base, the
/// `--config` file merged on top (keys win on collision), the tier
/// resolution (hard verbatim, or soft via `[thresholds.soft]` /
/// `--headroom`), and finally the absolute `--threshold` CLI overrides.
/// Dies if no thresholds were configured. Also returns the un-scaled
/// hard-tier limits per metric (#385) so the caller can tell a soft-band
/// encroachment apart from a true hard breach. The set is wrapped in
/// `Arc` so it can be cloned into each walker worker's `Config`.
fn validate_and_build_thresholds(
    args: &CheckArgs,
    base_thresholds: ParsedThresholds,
) -> ResolvedThresholds {
    // Validate --output / --output-format pairing before the walk so
    // a misconfigured invocation fails fast instead of after a full
    // parse. `--output` without `--output-format` is silently ignored
    // — only the human stderr stream is emitted, which is the
    // default contract — to keep the simplest invocation
    // (`bca check --threshold ... --no-fail > /dev/null`) frictionless.
    if let Some(fmt) = args.output_format
        && let Some(ref out) = args.output
        && out.exists()
        && out.is_dir()
    {
        die(format_args!(
            "--output must be a file path for `check --output-format {}`",
            fmt.name()
        ));
    }

    // Layer 1: the manifest `[thresholds]` table (empty when no
    // `bca.toml` was discovered). Layer 2: `--config` merges on top,
    // its keys winning on collision, preserving every existing recipe.
    // Both the hard and soft layers merge the same way.
    let ParsedThresholds { mut hard, mut soft } = base_thresholds;
    if let Some(config) = args.config.as_deref() {
        let cfg = load_threshold_config(config);
        hard.extend(cfg.hard);
        soft.extend(cfg.soft);
    }

    // A `--headroom` value is validated regardless of tier so a typo
    // (`--headroom 2`) is always a usage error, even when the tier
    // ultimately ignores the scalar.
    if let Some(ratio) = args.headroom
        && !crate::thresholds::is_valid_scale_ratio(ratio)
    {
        die(format_args!("--headroom must be in (0, 1]; got {ratio}"));
    }

    // Capture whether a soft table is configured before `resolve_tier`
    // borrows `soft`, so provenance resolution (#486) matches the same
    // branch the tier resolver takes.
    let soft_table_present = !soft.is_empty();

    // Layer 3: tier resolution. Produces the per-metric limits the gate
    // compares against. Clone `hard` so the un-scaled hard-tier limits
    // survive for #385 hard-breach detection below.
    let mut merged = resolve_tier(args.tier, hard.clone(), &soft, args.headroom);

    // Layer 4: `--threshold` CLI flags override the resolved limit for
    // the same metric name. They are absolute — applied *after* any
    // scaling — because a user who typed an exact limit means it, not a
    // fraction of it. The same value also defines the hard-tier ceiling
    // for that metric (#385): an explicit `--threshold` is the user's
    // declared limit, replacing whatever the hard table held.
    for (name, limit) in &args.thresholds {
        merged.insert(name.clone(), *limit);
        hard.insert(name.clone(), *limit);
    }
    let set = ThresholdSet::build(&merged).unwrap_or_else(|e| die(e));
    if set.is_empty() {
        die(
            "no thresholds configured; pass --threshold, --config, or a bca.toml [thresholds] table",
        );
    }
    ResolvedThresholds {
        set: Arc::new(set),
        hard_limits: hard,
        provenance: resolve_provenance(args.tier, soft_table_present, args.headroom),
    }
}

/// Resolve the per-metric limits for the requested tier (#375).
///
/// - `Hard`: the `[thresholds]` table verbatim. `[thresholds.soft]` is
///   ignored entirely; `--headroom` (a soft-tier dial) draws a note.
/// - `Soft` with a `[thresholds.soft]` table: merge the soft overrides
///   on top of the hard limits (metrics absent from the soft table keep
///   their hard limit — no soft band). `--headroom` is ignored with a
///   warning, because explicit per-metric limits encode intent more
///   precisely than a scalar multiplier.
/// - `Soft` without a soft table: scale every hard limit by `--headroom`
///   (defaulting to [`DEFAULT_SOFT_HEADROOM`] when unset).
fn resolve_tier(
    tier: Tier,
    hard: BTreeMap<String, f64>,
    soft: &BTreeMap<String, SoftLimit>,
    headroom: Option<f64>,
) -> BTreeMap<String, f64> {
    match tier {
        Tier::Hard => {
            if headroom.is_some() {
                eprintln!(
                    "note: --headroom applies only to the soft tier; pass --tier=soft \
                     to enable it. Ignored at the hard tier."
                );
            }
            hard
        }
        Tier::Soft if !soft.is_empty() => {
            if headroom.is_some() {
                eprintln!(
                    "warning: --headroom is ignored because a [thresholds.soft] table \
                     is configured; per-metric soft limits take precedence."
                );
            }
            // Start from the hard limits so metrics without a soft
            // override inherit their hard limit (no soft band), then
            // apply each soft override on top.
            let mut out = hard;
            for (name, soft_limit) in soft {
                let resolved = soft_limit
                    .resolve(name, out.get(name).copied())
                    .unwrap_or_else(|e| die(e));
                out.insert(name.clone(), resolved);
            }
            out
        }
        Tier::Soft => {
            // No soft table: scale the hard limits by the headroom ratio,
            // defaulting so `--tier=soft` is never a silent no-op.
            let ratio = headroom.unwrap_or(DEFAULT_SOFT_HEADROOM);
            if hard.is_empty() {
                eprintln!(
                    "note: --tier=soft has no effect without configured thresholds \
                     (bca.toml [thresholds] or --config); --threshold limits are \
                     absolute and are not scaled"
                );
            }
            let mut out = hard;
            for limit in out.values_mut() {
                *limit = scale_threshold(*limit, ratio);
            }
            out
        }
    }
}

/// Run the parallel walker with a check-flavoured `Config`, collect
/// every emitted `Violation`, and sort them by `(path, start_line,
/// metric)` so CI diff tooling sees identical output across runs over
/// the same tree. Returns the sorted vector plus the
/// `files_dispatched` counter so the caller can detect the "no inputs
/// matched" case.
fn run_check_walk(
    globals: GlobalOpts,
    args: &CheckArgs,
    preproc: Option<Arc<PreprocResults>>,
    set: Arc<ThresholdSet>,
) -> (Vec<Violation>, Arc<AtomicUsize>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let files_dispatched = Arc::new(AtomicUsize::new(0));
    let cfg = Config {
        threshold_set: Some(set),
        check_tx: Some(Mutex::new(tx)),
        files_dispatched: Some(Arc::clone(&files_dispatched)),
        suppression_policy: SuppressionPolicy::from_no_suppress(args.no_suppress),
        // Compute body hashes during the walk only when fuzzy matching
        // is requested — whether for a `--baseline` read or to populate
        // `body_hash` in a `--write-baseline` write.
        fuzzy_baseline: args.baseline_fuzzy_match,
        ..Config::new(Action::Check, &globals, preproc)
    };
    run_walk(globals, cfg);

    // Workers have all joined by the time `run_walk` returns, so the
    // sender side is dropped and `rx.into_iter()` terminates cleanly.
    let mut violations: Vec<Violation> = rx.into_iter().collect();
    // Stable, deterministic stderr output: by path, then start line, then
    // metric name. Different runs over the same tree produce identical
    // output, which CI diff tooling relies on.
    violations.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.start_line.cmp(&b.start_line))
            .then(a.metric.cmp(b.metric))
    });

    (violations, files_dispatched)
}

/// Serialize and write the collected violations as a baseline TOML
/// file. Used by the `--write-baseline` early-exit branch. The
/// baseline-file directory becomes the *anchor* — every entry's path
/// is keyed relative to it, so a subsequent `--baseline` invocation
/// from any `--paths` form (`.`, `src/`, `$PWD`) still matches.
fn write_check_baseline(violations: Vec<Violation>, path: &Path, provenance: baseline::Provenance) {
    let anchor = baseline::anchor_for(path);
    let file = baseline::from_violations(violations, &anchor, provenance);
    let entry_count = file.entries.len();
    let text =
        baseline::render(&file).unwrap_or_else(|e| die(format_args!("serialize baseline: {e}")));
    write_atomic(path, text.as_bytes()).unwrap_or_else(|e| die_io("write baseline", path, e));
    eprintln!(
        "bca: wrote {entry_count} baseline entries to {}",
        path.display()
    );
}

fn apply_check_exclude(violations: Vec<Violation>, args: &CheckArgs) -> Vec<Violation> {
    // Fast path: nothing configured (the common case) skips the
    // glob-set build and the file read entirely.
    if args.check_exclude.is_empty() && args.check_exclude_from.is_none() {
        return violations;
    }
    let globset = crate::build_exclude_globset(
        args.check_exclude.clone(),
        args.check_exclude_from.as_deref(),
        "--check-exclude-from",
    );
    let before = violations.len();
    let kept: Vec<Violation> = violations
        .into_iter()
        .filter(|v| !globset.is_match(&v.path))
        .collect();
    let skipped = before - kept.len();
    if skipped > 0 {
        eprintln!("bca: skipped {skipped} violations via [check.exclude]");
    }
    kept
}

/// Classify each violation against the optional `--baseline` file.
/// The kept list carries `(Violation, Option<Coverage>)` so the
/// stderr renderer can attach a `[new]` / `[regr +N%]` tag. Without
/// `--baseline`, `Option<Coverage>` is `None` and the renderer emits
/// the exact pre-tag line format byte-identically.
/// Compose the stderr warning (issue #486) when the current run is
/// stricter than the baseline was written against, or `None` when the
/// comparison is safe (see [`baseline::check_provenance`] for the
/// directional rule). Split out from [`filter_by_baseline`] so a test
/// can pin the exact message and the silent cases without a baseline
/// file on disk.
fn provenance_warning(
    current: baseline::Provenance,
    baseline: Option<baseline::Provenance>,
) -> Option<String> {
    match baseline::check_provenance(current, baseline) {
        baseline::ProvenanceCheck::Ok => None,
        baseline::ProvenanceCheck::StricterThanBaseline {
            current: cur,
            baseline: base,
        } => Some(format!(
            "warning: this check's effective limits (strictness {cur}) are \
             stricter than the baseline was written against (strictness \
             {base}); the baseline may under-cover and the gate can fire on \
             untouched files. Refresh it at the matching tier, e.g. \
             `bca check --tier soft --headroom {cur} --write-baseline \
             <file>` (or `--write-baseline <file>` for the hard tier)."
        )),
    }
}

fn filter_by_baseline(
    violations: Vec<Violation>,
    baseline_path: Option<&Path>,
    tolerance: usize,
    fuzzy: bool,
    provenance: baseline::Provenance,
) -> Vec<(Violation, Option<Coverage>)> {
    let Some(path) = baseline_path else {
        return violations.into_iter().map(|v| (v, None)).collect();
    };
    let baseline = load_baseline(path, tolerance, fuzzy);
    // Issue #486: warn when this run's effective limits are stricter than
    // the baseline was written against (the baseline may under-cover and
    // the gate can fire on untouched files). Silent in the safe
    // directions (hard reading soft, equal, absent provenance).
    if let Some(msg) = provenance_warning(provenance, baseline.provenance()) {
        eprintln!("{msg}");
    }
    let before = violations.len();
    let kept: Vec<_> = violations
        .into_iter()
        .filter_map(|v| match baseline.classify(&v) {
            Coverage::Covered { .. } => None,
            c => Some((v, Some(c))),
        })
        .collect();
    let filtered = before - kept.len();
    if filtered > 0 {
        eprintln!("bca: filtered {filtered} violations via baseline");
    }
    kept
}

/// Resolve the diff scope for `--since` / `--changed-only` /
/// auto-detected env vars. Behaviour:
///
/// - No flag, no env signal → `None`. The footer prints today's
///   single-section listing; `--changed-only` is rejected at the
///   top of the helper because it requires a scope.
/// - Resolved scope (`ResolveOutcome::Ok`) → `Some(scope)`, surfaced
///   in the footer banner and used to bucket touched-in-range rows.
/// - Resolver hit an error (`ResolveOutcome::Failed`) → fatal when
///   `--changed-only` is passed (otherwise the gate would silently
///   suppress nothing), warning-only otherwise (the developer still
///   sees the offender list, just without the touched-in-range
///   partition).
fn resolve_diff_scope(args: &CheckArgs) -> Option<diff::DiffScope> {
    let outcome = diff::resolve_scope(args.since.as_deref());
    match outcome {
        diff::ResolveOutcome::Ok(scope) => Some(scope),
        diff::ResolveOutcome::Disabled => {
            if args.changed_only {
                die("--changed-only requires --since <ref> or one of \
                     BCA_DIFF_BASE / GITHUB_BASE_REF / GITHUB_EVENT_BEFORE \
                     in the environment");
            }
            None
        }
        diff::ResolveOutcome::Failed { reason, source } => {
            if args.changed_only {
                die(format_args!(
                    "--changed-only: failed to resolve diff base via {}: {reason}",
                    source.label(),
                ));
            }
            eprintln!(
                "bca: --since/auto-detect via {} failed ({reason}); proceeding without diff scope",
                source.label(),
            );
            None
        }
    }
}

fn apply_changed_only(
    pairs: Vec<(Violation, Option<Coverage>)>,
    scope: Option<&diff::DiffScope>,
    changed_only: bool,
) -> Vec<(Violation, Option<Coverage>)> {
    let outcome = apply_changed_only_inner(pairs, scope, changed_only);
    if let Some(diag) = outcome.diagnostic {
        eprintln!("{diag}");
    }
    outcome.kept
}

/// Result of [`apply_changed_only_inner`]: the filtered pairs plus
/// an optional diagnostic string for the caller to surface. Extracted
/// from the outer `apply_changed_only` so tests can pin the
/// diagnostic shape (the "silent regression" guard the audit-tests
/// pass would otherwise miss).
struct ChangedOnlyOutcome {
    kept: Vec<(Violation, Option<Coverage>)>,
    diagnostic: Option<String>,
}

fn apply_changed_only_inner(
    pairs: Vec<(Violation, Option<Coverage>)>,
    scope: Option<&diff::DiffScope>,
    changed_only: bool,
) -> ChangedOnlyOutcome {
    if !changed_only {
        return ChangedOnlyOutcome {
            kept: pairs,
            diagnostic: None,
        };
    }
    let Some(scope) = scope else {
        // `resolve_diff_scope` fatal-errors when `--changed-only` is
        // set without a resolvable scope, so this branch is
        // unreachable from the production `run_check` pipeline. It
        // exists for tests and to defend against a future refactor
        // that bypasses that gate — degrade to a no-op rather than
        // silently emit the empty set (which would green-light the
        // gate on a misconfigured CI), but log so the operator
        // notices.
        return ChangedOnlyOutcome {
            kept: pairs,
            diagnostic: Some(
                "bca: --changed-only requested but no diff scope is available; \
                 skipping filter (would-be programmer error — \
                 resolve_diff_scope should have fatal-errored)"
                    .to_string(),
            ),
        };
    };
    if scope.changed.is_empty() {
        // A resolved-but-empty scope (e.g. running `--since main` from
        // a branch that has been merged/squashed into main locally, or
        // a force-pushed branch where the diff base now equals HEAD)
        // would otherwise silently drop every violation and exit 0,
        // which is exactly the "silent green-light" failure mode #359
        // was meant to prevent. Surface it explicitly so CI logs make
        // it obvious the gate ran but had nothing to check. Branch on
        // `pairs.is_empty()` so the wording matches reality: "dropping
        // 0 violations" would suggest the gate suppressed something
        // it did not, confusing a developer reading a clean PR log.
        let diag = if pairs.is_empty() {
            format!(
                "bca: --changed-only: diff scope is empty (no files touched between {} and HEAD); \
                 no violations to check and no files in diff scope",
                scope.base,
            )
        } else {
            format!(
                "bca: --changed-only: diff scope is empty (no files touched between {} and HEAD); \
                 dropping {} violations and exiting clean",
                scope.base,
                pairs.len()
            )
        };
        return ChangedOnlyOutcome {
            kept: Vec::new(),
            diagnostic: Some(diag),
        };
    }
    // Memoize `scope.contains` (which canonicalizes internally) by
    // raw `v.path`. Real-world inputs cluster heavily per file
    // (a 50-violation run typically touches 5-10 files), so this
    // turns O(violations) realpath(2) calls into O(unique raw
    // paths). Precondition: the walker must emit violation paths in
    // a canonical-form-consistent style across one check run (it
    // does — paths are always rooted at the same `--paths` seed
    // and don't mix `./X` with `X`). If a future change introduces
    // alias paths in a single run, two violations of the same file
    // would each pay a separate canonicalize call — the cache would
    // still be correct, just not optimal.
    let mut in_scope: HashMap<PathBuf, bool> = HashMap::new();
    let before = pairs.len();
    let kept: Vec<_> = pairs
        .into_iter()
        .filter(|(v, _)| {
            *in_scope
                .entry(v.path.clone())
                .or_insert_with(|| scope.contains(&v.path))
        })
        .collect();
    let dropped = before - kept.len();
    let diagnostic = (dropped > 0).then(|| {
        format!("bca: --changed-only dropped {dropped} violations from files outside diff scope")
    });
    ChangedOnlyOutcome { kept, diagnostic }
}

fn emit_check_results(
    pairs: Vec<(Violation, Option<Coverage>)>,
    args: &CheckArgs,
    scope: Option<&diff::DiffScope>,
    remediation: Option<&str>,
) {
    // BrokenPipe on stderr (e.g. when piped to `head`) is the only
    // realistic write failure here; swallow it rather than die so the
    // exit-code contract is honored.
    let mut stderr = std::io::stderr().lock();
    for (v, tag) in &pairs {
        let _ = writeln!(stderr, "{}", render_violation_line(v, tag.as_ref()));
    }
    if !args.no_summary && !pairs.is_empty() {
        let _ = write_summary_footer(&mut stderr, &pairs, scope);
    }
    if github_annotations_enabled(args) && !pairs.is_empty() {
        // Emit annotations *after* the human stream + summary footer
        // so a reader tailing the CI log sees the contiguous
        // human-readable block first. The GHA log viewer scrapes
        // `::error…` lines wherever they appear and renders them as
        // inline annotations on the file-diff view regardless of
        // position.
        let _ = check_format::write_github_annotations(
            &mut stderr,
            pairs.iter().map(|(v, _)| v),
            check_format::DEFAULT_GITHUB_ANNOTATION_CAP,
        );
    }
    // The remediation block is the final thing on stderr — a reader
    // skimming a CI log sees it as the natural "what now?" answer
    // immediately after the failure evidence. Skipped when the
    // caller passed `None` (clean run, or `--no-remediation`).
    if let Some(block) = remediation {
        let _ = write!(stderr, "{block}");
    }
    drop(stderr);

    // Append the markdown digest to `$GITHUB_STEP_SUMMARY` (or the
    // user-supplied `--summary-file`). Writes are bracketed by the
    // bca-step-summary markers so a retried GHA step replaces
    // (instead of stacks) the previous block. Failures here are
    // logged but never affect the exit-code contract — the
    // step-summary panel is informational.
    if let Some(path) = step_summary_path(args)
        && let Err(e) = check_format::write_step_summary(&path, &pairs, remediation)
    {
        eprintln!(
            "bca: failed to append step summary to {}: {e}",
            path.display()
        );
    }

    // Emit the aggregated CI/IDE document if requested. Empty input
    // produces a well-formed but offender-free document, which CI
    // consumers can ingest unchanged on clean runs. The exit-code
    // contract is unaffected by this branch.
    if let Some(fmt) = args.output_format {
        let offenders: Vec<_> = pairs
            .into_iter()
            .map(|(v, _)| violation_to_offender(v))
            .collect();
        fmt.dump(&offenders, args.output.as_deref())
            .unwrap_or_else(|e| die(format_args!("failed to write {}: {e}", fmt.name())));
    }
}

/// Severity category of a `bca check` run, used to derive the process
/// exit code (#385). The variants are *not* the exit codes — the
/// mapping depends on `--strict-exit-codes` (see [`Self::exit_code`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckOutcome {
    /// No violations survived filtering.
    Clean,
    /// Violations exist, but none is a baseline regression and none
    /// breaches the hard limit under `--tier=soft`. Also the bucket for
    /// every violation when no `--baseline` was supplied (nothing is
    /// baselined, so nothing can have "regressed").
    NewOnly,
    /// Every kept violation matched a baseline entry that worsened.
    RegressionOnly,
    /// A mix of new offenders and baseline regressions.
    Mixed,
    /// At least one `--tier=soft` violation also exceeds the hard
    /// limit — escalated above the new/regression split because a true
    /// breach is more urgent than soft-band encroachment.
    HardBreach,
}

impl CheckOutcome {
    /// Map the outcome to a process exit code. In the default contract
    /// (`strict == false`) every non-clean run collapses to exit `2`,
    /// preserving the stable 0/1/2 behaviour every existing integration
    /// relies on. In tiered mode (`--strict-exit-codes`) each category
    /// gets its own code (2-5). Returns `None` for a clean run, where
    /// the caller exits 0 implicitly by returning.
    fn exit_code(self, strict: bool) -> Option<i32> {
        let tiered = match self {
            Self::Clean => return None,
            Self::NewOnly => 2,
            Self::RegressionOnly => 3,
            Self::Mixed => 4,
            Self::HardBreach => 5,
        };
        // The default contract collapses every violation category to
        // exit 2; only `--strict-exit-codes` surfaces the 3/4/5 split.
        Some(if strict { tiered } else { 2 })
    }
}

/// Categorise the kept violations for the exit-code contract (#385).
///
/// `hard_limits` holds the resolved hard-tier limit per metric. It is
/// consulted only at the soft tier, where a violation whose value also
/// exceeds the hard limit escalates to [`CheckOutcome::HardBreach`]. At
/// the hard tier every violation already exceeds the hard limit, so the
/// escalation is suppressed (it would otherwise swallow the new/regr
/// split) and only baseline coverage drives the result.
fn classify_check_outcome(
    pairs: &[(Violation, Option<Coverage>)],
    tier: Tier,
    hard_limits: &BTreeMap<String, f64>,
) -> CheckOutcome {
    if pairs.is_empty() {
        return CheckOutcome::Clean;
    }
    let mut has_new = false;
    let mut has_regression = false;
    let mut has_hard_breach = false;
    for (v, coverage) in pairs {
        // A NaN value (degenerate Halstead on a trivial function) yields
        // `NaN > hard == false`, so it never escalates to a hard breach;
        // it falls to the new/regr split below, mirroring how
        // `Baseline::classify` treats a NaN as `Regressed` rather than a
        // magnitude. A NaN has no meaningful distance from the ceiling.
        if tier == Tier::Soft
            && let Some(&hard) = hard_limits.get(v.metric)
            && v.value > hard
        {
            has_hard_breach = true;
        }
        match coverage {
            Some(Coverage::Regressed { .. }) => has_regression = true,
            // `Coverage::New`, or `None` when no `--baseline` was given.
            // `Coverage::Covered` never reaches here — `filter_by_baseline`
            // drops it before the kept set is built.
            _ => has_new = true,
        }
    }
    if has_hard_breach {
        CheckOutcome::HardBreach
    } else if has_new && has_regression {
        CheckOutcome::Mixed
    } else if has_regression {
        CheckOutcome::RegressionOnly
    } else {
        CheckOutcome::NewOnly
    }
}

/// Decide whether GitHub Actions `::error` annotations should be
/// emitted. The explicit `--github-annotations` flag wins; otherwise
/// fall back to auto-detection via `$GITHUB_ACTIONS == "true"`, the
/// signal GHA sets inside every workflow step. Mirrors the
/// auto-detect ladder in the diff resolver so the two CI-presentation
/// behaviours stay in lockstep.
fn github_annotations_enabled(args: &CheckArgs) -> bool {
    args.github_annotations
        || std::env::var(check_format::GITHUB_ACTIONS_ENV).as_deref() == Ok("true")
}

/// Resolve the path to append the step-summary digest to, in
/// precedence: explicit `--summary-file <path>` wins; otherwise
/// `$GITHUB_STEP_SUMMARY` (auto-detected in GHA workflows); otherwise
/// `None` and the digest is not emitted.
fn step_summary_path(args: &CheckArgs) -> Option<PathBuf> {
    if let Some(p) = &args.summary_file {
        return Some(p.clone());
    }
    std::env::var_os(check_format::GITHUB_STEP_SUMMARY_ENV).map(PathBuf::from)
}

fn format_remediation_block(globals: &GlobalOpts, args: &CheckArgs) -> Option<String> {
    use std::fmt::Write as _;
    if args.no_remediation {
        return None;
    }
    let mut out = String::from("\n--- next steps ---\n");
    let _ = writeln!(out, "* Detailed reports: {}", artifact_link());
    let _ = writeln!(
        out,
        "* To refresh baseline: {}",
        refresh_baseline_command(globals, args)
    );
    // The refresh command mirrors path filters (`--paths`,
    // `--exclude`, `--exclude-from`, `--config`, `--baseline`) but
    // intentionally omits selectors that don't affect baseline
    // composition (`--num-jobs`) and ones that would bloat the
    // common-case command (`--include`, `--language-type`,
    // `--paths-from`, `--exclude-tests`). Surface the omission so a
    // user with a non-trivial scope re-adds them rather than
    // assuming the printed command is complete.
    out.push_str(
        "  (mirrors path filters only — re-add any --include / --language-type / --exclude-tests / --paths-from flags as needed)\n",
    );
    out.push_str(
        "* Adoption guide: https://dekobon.github.io/big-code-analysis/recipes/baselines.html\n",
    );
    Some(out)
}

fn artifact_link() -> String {
    artifact_link_for(
        std::env::var(check_format::GITHUB_REPOSITORY_ENV).ok(),
        std::env::var(check_format::GITHUB_RUN_ID_ENV).ok(),
    )
}

/// Pure inner: render the artifact bullet given explicit env values
/// (rather than reading them from the process environment). Extracted
/// so tests can pin both the SOME and NONE branches without
/// depending on whether the test process happens to have GHA env
/// vars set. Empty strings are treated as absent — GitHub Actions
/// does set these vars but the spec doesn't promise non-empty values
/// on every event type.
fn artifact_link_for(repo: Option<String>, run_id: Option<String>) -> String {
    let repo = repo.filter(|s| !s.is_empty());
    let run_id = run_id.filter(|s| !s.is_empty());
    match (repo, run_id) {
        (Some(repo), Some(run_id)) => {
            format!("bca-reports artifact at https://github.com/{repo}/actions/runs/{run_id}")
        }
        _ => "bca-reports artifact (uploaded to this run)".to_string(),
    }
}

fn refresh_baseline_command(globals: &GlobalOpts, args: &CheckArgs) -> String {
    let mut cmd = String::from("bca");
    let paths: Vec<&Path> = if globals.paths.is_empty() {
        // Default-when-absent — mirror the walker's `expand_seed_paths`
        // fallback so the printed command behaves identically to a
        // pathless invocation.
        vec![Path::new(".")]
    } else {
        globals.paths.iter().map(PathBuf::as_path).collect()
    };
    for p in &paths {
        cmd.push_str(" --paths ");
        cmd.push_str(&shell_quote_path(p));
    }
    for ex in &globals.exclude {
        cmd.push_str(" --exclude ");
        cmd.push_str(&shell_quote(ex));
    }
    if let Some(p) = &globals.exclude_from {
        cmd.push_str(" --exclude-from ");
        cmd.push_str(&shell_quote_path(p));
    }
    cmd.push_str(" check");
    if let Some(p) = &args.config {
        cmd.push_str(" --config ");
        cmd.push_str(&shell_quote_path(p));
    }
    // `--baseline` and `--write-baseline` conflict in clap, so we
    // prefer the user's baseline path if they ran with it (the
    // refresh writes back to the same file). Fall back to the
    // documented default `.bca-baseline.toml`.
    cmd.push_str(" --write-baseline ");
    match args.baseline.as_deref() {
        Some(p) => cmd.push_str(&shell_quote_path(p)),
        None => cmd.push_str(&shell_quote(".bca-baseline.toml")),
    }
    cmd
}

fn shell_quote_path(p: &Path) -> String {
    // The printed command is an *identifier* in the user's shell —
    // running it must reach the same file `bca` walked. Non-UTF-8
    // paths cannot be expressed as a shell argument verbatim, so
    // surface them as a clearly-broken placeholder rather than emit
    // a `to_string_lossy` form that silently points at the wrong
    // file (AGENTS.md: identifier paths use `to_str()` with explicit
    // non-UTF-8 handling, not `path.display()`).
    //
    // The placeholder contains `<`, `>` and spaces, which force
    // `shell_quote`'s slow path → single-quoted literal. Combining
    // the to_str + quote here (instead of leaving callers to chain
    // them) makes the discipline structural: a future caller can't
    // accidentally `eprintln!("{}", path_for_shell(p))` and emit an
    // unquoted `<non-UTF-8 path: …>` that bash would parse as input
    // redirection.
    let raw = p.to_str().map_or_else(
        || format!("<non-UTF-8 path: {}>", p.display()),
        str::to_string,
    );
    shell_quote(&raw)
}

/// Shell-quote `s` for inclusion in the remediation block's
/// copy-paste command. Uses single-quoting for simplicity: every
/// character is literal inside `'...'` except `'` itself, which we
/// escape via `'\''`. ASCII-safe and POSIX-compatible.
///
/// **POSIX-only**: This quoting is correct for bash / zsh / dash /
/// sh, which is what GitHub Actions runs every step in. It is NOT
/// safe for `cmd.exe` or `PowerShell` — a Windows user copy-pasting
/// the refresh command from a Windows CI log would need to
/// re-escape. The remediation block is a GHA/POSIX-CI feature by
/// design; Windows-host CI is out of scope.
fn shell_quote(s: &str) -> String {
    // Fast path: identifiers / paths without metacharacters need no
    // quoting at all. Keeping them unquoted makes the copy-paste
    // command read naturally for the common case.
    if !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | ',' | '@')
        })
    {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

struct FooterRow<'a> {
    count: usize,
    worst: &'a Violation,
    display: String,
    path: &'a Path,
}

fn compute_footer_rows(pairs: &[(Violation, Option<Coverage>)]) -> Vec<FooterRow<'_>> {
    Violation::group_pairs_by_path(pairs)
        .into_iter()
        .map(|(count, worst, display, path)| FooterRow {
            count,
            worst,
            display,
            path,
        })
        .collect()
}

/// Emit each row in `rows`, propagating the first I/O error. Used
/// by both the legacy single-section path and the per-bucket
/// partitioned path so the row format stays in lockstep.
fn emit_footer_rows(w: &mut impl Write, rows: &[FooterRow<'_>]) -> std::io::Result<()> {
    for row in rows {
        write_footer_row(w, row.count, row.worst, &row.display)?;
    }
    Ok(())
}

/// Emit the "Files in this range:" header followed by the touched
/// rows. When the diff scope had no offenders in it, emit an
/// explicit "(none — …)" line so the reader gets a positive "your
/// change is clean" signal instead of having to compare both halves
/// of the footer to confirm absence.
fn write_in_range_section(
    w: &mut impl Write,
    scope: &diff::DiffScope,
    in_range: &[FooterRow<'_>],
) -> std::io::Result<()> {
    writeln!(
        w,
        "Files in this range (diff base: {} via {}):",
        scope.base,
        scope.source.label()
    )?;
    if in_range.is_empty() {
        writeln!(w, "  (none — no offenders in files touched by this diff)")?;
    } else {
        emit_footer_rows(w, in_range)?;
    }
    Ok(())
}

/// Emit the "Other offenders:" header followed by the legacy
/// offender list (files not touched by the diff scope). Returns a
/// clean `Ok(())` when `other` is empty so the caller need not gate
/// the call — the section's heading would be misleading without
/// rows below it.
fn write_other_section(w: &mut impl Write, other: &[FooterRow<'_>]) -> std::io::Result<()> {
    if other.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    writeln!(w, "Other offenders:")?;
    emit_footer_rows(w, other)
}

fn write_summary_footer(
    w: &mut impl Write,
    pairs: &[(Violation, Option<Coverage>)],
    scope: Option<&diff::DiffScope>,
) -> std::io::Result<()> {
    // The caller (`emit_check_results`) gates on `!pairs.is_empty()`,
    // so `compute_footer_rows` should always return at least one
    // row. Assert in debug builds so a future refactor that
    // surfaces the footer on clean runs (e.g. for positive-
    // confirmation symmetry with the step-summary "✓ No threshold
    // violations" message) doesn't silently emit a dangling
    // `Files in this range:` banner with no body.
    let rows = compute_footer_rows(pairs);
    debug_assert!(
        !rows.is_empty(),
        "write_summary_footer called with no rows; \
         caller must gate on !pairs.is_empty()"
    );
    writeln!(w)?;
    writeln!(w, "--- summary ---")?;
    let Some(s) = scope else {
        // Without a scope, today's single-section footer is
        // byte-identical to the pre-#359 output. This is the
        // load-bearing back-compat path for CI tooling that grep-
        // anchors on the legacy footer shape.
        return emit_footer_rows(w, &rows);
    };
    // With a scope, partition rows into "touched in this range" vs
    // legacy offenders. `DiffScope::contains` canonicalises once per
    // row group (already deduplicated by `compute_footer_rows`), so
    // the partitioning is at worst O(unique files) realpath(2) calls.
    let (in_range, other): (Vec<_>, Vec<_>) =
        rows.into_iter().partition(|row| s.contains(row.path));
    write_in_range_section(w, s, &in_range)?;
    write_other_section(w, &other)
}

/// Render a single per-file footer row. Shared between the in-range
/// and other-offenders sections so the formatting stays in lockstep.
fn write_footer_row(
    w: &mut impl Write,
    count: usize,
    worst: &Violation,
    display: &str,
) -> std::io::Result<()> {
    let noun = if count == 1 {
        "violation"
    } else {
        "violations"
    };
    writeln!(
        w,
        "{display}: {count} {noun} (worst: {} = {} vs limit {} at L{})",
        worst.metric,
        MetricScalar(worst.value),
        MetricScalar(worst.limit),
        worst.start_line,
    )
}

/// Parse `std::env::args_os()` and execute the selected `bca`
/// subcommand. Intended to be called from the `bca` binary's `main`,
/// which is a one-liner over this function.
///
/// # Termination contract
///
/// This function **may terminate the calling process** rather than
/// return. It is not a re-entrant library entry point:
///
/// - clap argument-parsing failures bubble up through
///   [`clap::Error::exit`] (exit 0 on `--help` / `--version`, exit 2
///   on usage errors).
/// - User-input errors (invalid threshold spec, unreadable preproc
///   data, malformed `bca.toml`, missing `--output` parent directory,
///   walk errors, mutually exclusive output-format combinations,
///   broken-pipe writes, etc.) call `process::exit(1)` via internal
///   `die` / `die_io` helpers.
/// - The `check` subcommand calls `process::exit(2)` when any
///   threshold is exceeded, reserving exit 1 for tool errors so CI can
///   distinguish "metric regression" from "tool crashed".
///
/// Hosts that call [`run`] will be torn down on any of those paths
/// without unwinding. If you need to drive the same functionality from
/// inside another process, use the [`big_code_analysis`] library crate
/// directly instead of going through this entry point.
pub fn run() {
    let (mut cli, num_jobs_from_cli) = parse_cli_with_legacy_hint();

    // Auto-discover a `bca.toml` manifest (unless `--no-config`) and
    // merge its global keys *under* the parsed CLI flags. Check-only
    // keys (baseline / headroom / thresholds) are merged later, inside
    // `run_check`, where the resolved `CheckArgs` lives.
    //
    // `bca init` is deliberately excluded: it *scaffolds* configuration,
    // so consuming an existing manifest would merge repo-level `paths`
    // into init's baseline-generation walk and pin the wrong tree.
    //
    // `bca diff-baseline` and `bca diff` are excluded for the same
    // reason from the other direction: they walk no source and read no
    // global config, so manifest discovery would be pure overhead.
    let manifest = if cli.globals.no_config
        || matches!(
            cli.command,
            Command::Init(_) | Command::DiffBaseline(_) | Command::Diff(_)
        ) {
        None
    } else {
        manifest::discover_and_load()
    };
    if let Some(m) = &manifest {
        m.merge_globals(&mut cli.globals, num_jobs_from_cli);
    }

    let preproc = cli
        .globals
        .preproc_data
        .as_ref()
        .map(|p| load_preproc_data(p));

    match cli.command {
        Command::ListMetrics(args) => run_command_list_metrics(args),
        Command::Dump => run_command_dump(cli.globals, preproc),
        Command::Functions => run_command_functions(cli.globals, preproc),
        Command::Metrics(args) => run_command_metrics(cli.globals, args, preproc),
        Command::Ops(args) => run_command_ops(cli.globals, args, preproc),
        Command::Report(args) => run_command_report(cli.globals, args, preproc),
        Command::Find(args) => run_command_find(cli.globals, args, preproc),
        Command::Count(args) => run_command_count(cli.globals, args, preproc),
        Command::StripComments(args) => run_command_strip_comments(cli.globals, args, preproc),
        Command::Check(args) => run_check(cli.globals, *args, manifest.as_ref(), preproc),
        Command::Preproc(args) => run_command_preproc(cli.globals, args),
        Command::Init(args) => run_command_init(cli.globals, args, preproc),
        Command::DiffBaseline(args) => run_command_diff_baseline(args),
        Command::Diff(args) => run_command_diff(cli.globals, args),
        Command::Exemptions(args) => {
            run_command_exemptions(cli.globals, args, manifest.as_ref(), preproc);
        }
    }
}

/// Parse the CLI from `std::env::args_os`, emitting a legacy-CLI
/// migration hint to stderr when the failure looks like it came from
/// the pre-restructure flag shape (`-d` instead of `dump`, `-O
/// markdown` instead of `report markdown`, etc.). Exits the process
/// on parse failure via `clap::Error::exit`.
///
/// Returns the parsed [`Cli`] plus whether `--num-jobs` was set on the
/// command line. `num_jobs` is the one manifest-backed global with a
/// non-`None`/non-empty default, so its CLI-vs-default state cannot be
/// inferred from the parsed value alone — the manifest merge needs the
/// `ArgMatches` value source to know whether to override it.
fn parse_cli_with_legacy_hint() -> (Cli, bool) {
    let matches = match Cli::command().try_get_matches() {
        Ok(matches) => matches,
        Err(err) => {
            if matches!(
                err.kind(),
                clap::error::ErrorKind::UnknownArgument
                    | clap::error::ErrorKind::InvalidSubcommand
                    | clap::error::ErrorKind::InvalidValue
                    | clap::error::ErrorKind::MissingSubcommand
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) && let Some(hint) = legacy_hint(std::env::args_os())
            {
                eprintln!("{hint}");
            }
            err.exit();
        }
    };
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|err| err.exit());
    (cli, num_jobs_set_on_cli(&matches))
}

/// Whether `--num-jobs` was supplied on the command line (vs. left at
/// its `auto` default). `num_jobs` is a `global = true` arg, so when it
/// is passed after the subcommand its value source surfaces in the
/// subcommand's matches, not the root's — walk the chain.
fn num_jobs_set_on_cli(matches: &ArgMatches) -> bool {
    if matches.value_source("num_jobs") == Some(ValueSource::CommandLine) {
        return true;
    }
    match matches.subcommand() {
        Some((_, sub)) => num_jobs_set_on_cli(sub),
        None => false,
    }
}

fn run_command_list_metrics(args: ListMetricsArgs) {
    let mut buf = Vec::new();
    write_metrics(&mut buf, args.mode).expect("writing to Vec<u8> is infallible");
    write_stdout_or_die(&buf);
}

fn run_command_dump(globals: GlobalOpts, preproc: Option<Arc<PreprocResults>>) {
    let cfg = Config::new(Action::Dump, &globals, preproc);
    run_walk(globals, cfg);
}

fn run_command_functions(globals: GlobalOpts, preproc: Option<Arc<PreprocResults>>) {
    let cfg = Config::new(Action::Functions, &globals, preproc);
    run_walk(globals, cfg);
}

/// Shared `--output must be a directory` guard for the `metrics` and
/// `ops` commands. Skips when no `--output-format` is set (then
/// `--output` is silently ignored) or when no `--output` is passed.
/// `command` names the subcommand for the error message.
fn require_output_is_dir(have_format: bool, output: Option<&Path>, command: &str) {
    if have_format
        && let Some(out) = output
        && out.exists()
        && !out.is_dir()
    {
        die(format_args!("--output must be a directory for `{command}`"));
    }
}

fn run_command_metrics(
    globals: GlobalOpts,
    args: StructuredArgs,
    preproc: Option<Arc<PreprocResults>>,
) {
    if matches!(args.output_format, Some(MetricsFormat::Cbor)) && args.output.is_none() {
        die(CBOR_STDOUT_ERROR);
    }
    require_output_is_dir(
        args.output_format.is_some(),
        args.output.as_deref(),
        "metrics",
    );
    let action = Action::Metrics {
        format: args.output_format,
        pretty: args.pretty,
    };
    let cfg = Config {
        output: args.output,
        ..Config::new(action, &globals, preproc)
    };
    run_walk(globals, cfg);
}

fn run_command_ops(
    globals: GlobalOpts,
    args: StructuredArgs,
    preproc: Option<Arc<PreprocResults>>,
) {
    if matches!(args.output_format, Some(MetricsFormat::Cbor)) && args.output.is_none() {
        die(CBOR_STDOUT_ERROR);
    }
    if let Some(MetricsDispatch::Csv) = args.output_format.map(MetricsFormat::dispatch) {
        die(
            "CSV is not supported by `ops` because its column schema is metric-shaped; use `bca metrics --output-format <fmt>`",
        );
    }
    require_output_is_dir(args.output_format.is_some(), args.output.as_deref(), "ops");
    let action = Action::Ops {
        format: args.output_format,
        pretty: args.pretty,
    };
    let cfg = Config {
        output: args.output,
        ..Config::new(action, &globals, preproc)
    };
    run_walk(globals, cfg);
}

fn run_command_report(globals: GlobalOpts, args: ReportArgs, preproc: Option<Arc<PreprocResults>>) {
    if let Some(ref output) = args.output {
        if output.exists() && output.is_dir() {
            die("--output must be a file path for `report`");
        }
        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            die(format_args!(
                "parent directory of --output does not exist: {}",
                parent.display()
            ));
        }
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = Config {
        markdown_tx: Some(Mutex::new(tx)),
        strip_prefix: args.strip_prefix,
        ..Config::new(Action::Report, &globals, preproc)
    };
    run_walk(globals, cfg);

    // ConcurrentRunner::run() consumed Config (and thus the Sender).
    // All worker threads have joined, so `rx.into_iter()` terminates.
    let summaries: Vec<FunctionSummary> = rx.into_iter().collect();
    let report = match args.format {
        ReportFormat::Markdown => generate_report(&summaries, args.top as usize),
        ReportFormat::Html => generate_html_report(&summaries, args.top as usize),
    };
    if let Some(ref output_path) = args.output {
        std::fs::write(output_path, &report)
            .unwrap_or_else(|e| die_io("write report to", output_path, e));
    } else {
        write_stdout_or_die(report.as_bytes());
    }
}

fn run_command_find(globals: GlobalOpts, args: NodesArgs, preproc: Option<Arc<PreprocResults>>) {
    let cfg = Config::new(Action::Find(args.nodes.into()), &globals, preproc);
    run_walk(globals, cfg);
}

fn run_command_count(globals: GlobalOpts, args: NodesArgs, preproc: Option<Arc<PreprocResults>>) {
    let count_lock = Arc::new(Mutex::new(Count::default()));
    let cfg = Config {
        count_lock: Some(count_lock.clone()),
        ..Config::new(Action::Count(args.nodes.into()), &globals, preproc)
    };
    run_walk(globals, cfg);

    let count = Arc::try_unwrap(count_lock)
        .expect("all worker threads have joined; Arc refcount is 1")
        .into_inner()
        .expect("mutex not poisoned");
    println!("{count}");
}

fn run_command_strip_comments(
    globals: GlobalOpts,
    args: StripCommentsArgs,
    preproc: Option<Arc<PreprocResults>>,
) {
    let action = Action::StripComments {
        in_place: args.in_place,
    };
    let cfg = Config::new(action, &globals, preproc);
    run_walk(globals, cfg);
}

fn run_command_preproc(globals: GlobalOpts, args: PreprocArgs) {
    let preproc_lock = Arc::new(Mutex::new(PreprocResults::default()));
    let output = args.output;
    let cfg = Config {
        preproc_lock: Some(preproc_lock.clone()),
        // PreprocProduce builds its own preproc results; any inbound
        // `--preproc-data` from globals is intentionally ignored for
        // this command (the original code passed `None` here too).
        ..Config::new(Action::PreprocProduce, &globals, None)
    };
    let all_files = run_walk(globals, cfg);

    let mut data = Arc::try_unwrap(preproc_lock)
        .expect("all worker threads have joined; Arc refcount is 1")
        .into_inner()
        .expect("mutex not poisoned");
    fix_includes(&mut data.files, &all_files);

    let serialized = serde_json::to_string(&data)
        .unwrap_or_else(|e| die(format_args!("failed to serialize preproc data: {e}")));
    if let Some(output_path) = output {
        write_file(&output_path, serialized.as_bytes())
            .unwrap_or_else(|e| die_io("write preproc output to", &output_path, e));
    } else {
        println!("{serialized}");
    }
}

/// Canonical contents of a freshly-scaffolded `bca.toml` manifest.
/// This is the consolidated config `bca` auto-discovers when climbing
/// from the working directory to the repo root (#374, #483): a bare
/// `bca check` (no flags) reproduces the gate once this file exists.
/// The `[thresholds]` values mirror the project's own root `bca.toml`,
/// so `bca init` hands adopters the same limits the project gates
/// itself with. The two `[thresholds]` tables are kept in lock-step by
/// the `init_template_thresholds_match_repo_root` drift test: retuning
/// the root gate fails that test until this template is updated to
/// match, and vice versa.
///
/// Only the surrounding prose differs by design — the root file
/// carries the repo-specific rationale for each limit, while this
/// template keeps the generic editing-rules header. Adopters typically
/// run `--write-baseline` right after `init`, which pins today's
/// offenders so subsequent runs fail only on regressions, then tighten
/// limits over time.
const INIT_MANIFEST_TEMPLATE: &str = "\
# bca.toml — project manifest, auto-discovered by `bca` when climbing
# from the working directory to the repo root. This is the single
# source of truth for the metric gate: with this file present, a bare
# `bca check` (no flags) reproduces the full gate, zero-config.

# Scan the whole project, honouring the ignore file below.
paths = [\".\"]
exclude_from = \".bcaignore\"

# Pair with `.bca-baseline.toml` so existing offenders are absorbed;
# only regressions and new offenders fail the gate. See the Baselines
# recipe in the book for the bootstrap/refresh/retire flow.
baseline = \".bca-baseline.toml\"

# Rust-only policy: treat `?` (the `try_expression` node) as linear
# error propagation rather than a branch, so it does not contribute to
# cyclomatic complexity. Equivalent to `--no-cyclomatic-try` on every
# invocation (#409). Inert for non-Rust grammars (no other grammar
# emits the node). Uncomment to opt in if your project is Rust.
# cyclomatic_count_try = false

# bca per-function threshold configuration.
#
# Editing rules:
#   * Each key is a stable metric name (or dotted sub-metric name) from
#     `bca list-metrics` / `bca check --help`. Available names:
#         cognitive, cyclomatic, cyclomatic.modified,
#         halstead.volume, halstead.difficulty, halstead.effort,
#         halstead.time, halstead.bugs,
#         loc.sloc, loc.ploc, loc.lloc, loc.cloc, loc.blank,
#         nom, tokens, nexits, nargs,
#         mi.original, mi.sei, mi.visual_studio,
#         abc, wmc, npm, npa
#   * Quote keys containing a dot (TOML requires it).
#   * Values are per-function limits. A function whose metric exceeds the
#     limit becomes an offender; the baseline file decides whether it
#     fails CI.
#   * Adding a metric is a tightening — regenerate `.bca-baseline.toml`
#     in the same change so day-one CI does not flip red on offenders
#     that were previously invisible to the gate.
#
# Metrics intentionally NOT gated (yet):
#   halstead.{volume,difficulty,time,bugs}, cyclomatic.modified, mi.*
# They are still computed and visible in `bca report markdown|html`.
[thresholds]
cognitive = 25
cyclomatic = 15
\"halstead.effort\" = 50000
\"loc.sloc\" = 800
nom = 30
nargs = 7
nexits = 5
abc = 50
wmc = 60
";

/// Canonical contents of a freshly-scaffolded `.bcaignore`. The
/// patterns are commented out so the file is a no-op until the
/// adopter opts in — uncommenting per project. They mirror the
/// patterns the self-scan workflow walks and the book's adoption
/// recipe recommends.
const INIT_BCAIGNORE_TEMPLATE: &str = "\
# Shared exclude list for `bca --paths . --exclude-from .bcaignore ...`.
# Patterns are `.gitignore`-style; blank lines and lines whose first
# non-whitespace character is `#` are skipped.
#
# Uncomment any patterns that match generated / vendored / test code
# you do not want included in the metric gate. The defaults below are
# the common cases; tailor as needed.

# ./target/**
# ./dist/**
# ./build/**
# ./node_modules/**
# ./**/*.generated.*
# ./**/tests/**
# ./**/*_tests.rs
";

/// Empty-but-valid baseline file written when the user passes
/// `--no-baseline`. Matches the shape `baseline::render` would emit
/// for an empty `BaselineFile`, including the `version` key the
/// loader requires.
/// Render the empty-baseline placeholder written by `bca init
/// --no-baseline`. Built dynamically so the `version` line always
/// tracks [`baseline::BASELINE_VERSION`] rather than drifting on a
/// schema bump.
fn init_empty_baseline_template() -> String {
    format!(
        "\
# bca baseline file. Generated by `bca init --no-baseline`.
# Populate via:
#   bca check --write-baseline .bca-baseline.toml
# A function whose metric value worsens vs. its baselined entry still
# fails; new offenders also still fail. Refresh with `--write-baseline`
# when entries become stale.
version = {}
",
        baseline::BASELINE_VERSION
    )
}

fn run_command_init(globals: GlobalOpts, args: InitArgs, preproc: Option<Arc<PreprocResults>>) {
    let target = args.dir.clone().unwrap_or_else(|| PathBuf::from("."));
    if !target.exists() {
        die(format_args!(
            "bca init: target directory does not exist: {}",
            target.display()
        ));
    }
    if !target.is_dir() {
        die(format_args!(
            "bca init: target path is not a directory: {}",
            target.display()
        ));
    }

    let manifest_path = target.join("bca.toml");
    let bcaignore_path = target.join(".bcaignore");
    let baseline_path = target.join(".bca-baseline.toml");

    // Refuse to clobber existing files unless --force. List every
    // blocker in one shot so the user can decide whether to delete /
    // back up before retrying, rather than fixing one and re-running
    // to discover the next.
    if !args.force {
        let list = [&manifest_path, &bcaignore_path, &baseline_path]
            .into_iter()
            .filter(|p| p.exists())
            .map(|p| format!("  {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n");
        if !list.is_empty() {
            die(format_args!(
                "bca init: refusing to overwrite existing files (pass --force to clobber):\n{list}"
            ));
        }
    }

    write_atomic(&manifest_path, INIT_MANIFEST_TEMPLATE.as_bytes())
        .unwrap_or_else(|e| die_io("write", &manifest_path, e));
    eprintln!("bca init: wrote {}", manifest_path.display());

    write_atomic(&bcaignore_path, INIT_BCAIGNORE_TEMPLATE.as_bytes())
        .unwrap_or_else(|e| die_io("write", &bcaignore_path, e));
    eprintln!("bca init: wrote {}", bcaignore_path.display());

    if args.no_baseline {
        write_atomic(&baseline_path, init_empty_baseline_template().as_bytes())
            .unwrap_or_else(|e| die_io("write", &baseline_path, e));
        eprintln!(
            "bca init: wrote empty {} (populate via `bca check --write-baseline {}`)",
            baseline_path.display(),
            baseline_path.display(),
        );
    } else {
        // Reuse the same code path `bca check --write-baseline` uses
        // so the produced baseline is byte-identical to one a manual
        // bootstrap would write. The thresholds we just wrote are
        // loaded from the scaffolded `bca.toml` to keep this consistent
        // with what the user will use day-to-day.
        let check_args = CheckArgs {
            thresholds: Vec::new(),
            config: Some(manifest_path.clone()),
            no_fail: false,
            no_suppress: false,
            output_format: None,
            output: None,
            baseline: None,
            write_baseline: Some(baseline_path.clone()),
            no_summary: true,
            since: None,
            changed_only: false,
            github_annotations: false,
            summary_file: None,
            no_remediation: true,
            print_effective_config: None,
            headroom: None,
            tier: Tier::Hard,
            strict_exit_codes: false,
            baseline_line_tolerance: None,
            baseline_fuzzy_match: false,
            check_exclude: Vec::new(),
            check_exclude_from: None,
        };
        // `run_check` early-exits after `write_check_baseline` runs,
        // so it returns normally on success here.
        let mut walk_globals = globals;
        if walk_globals.paths.is_empty() {
            walk_globals.paths.push(target.clone());
        }
        // `init` writes its baseline from the manifest it just
        // scaffolded, so it deliberately bypasses manifest discovery
        // (passing `None`) — the freshly-written `bca.toml` is the
        // source of truth here, supplied directly via `--config`.
        run_check(walk_globals, check_args, None, preproc);
        eprintln!("bca init: wrote {}", baseline_path.display());
    }

    eprintln!(
        "bca init: done. Next steps:\n  \
         1. Review {} and tighten/loosen thresholds for your codebase.\n  \
         2. Uncomment relevant patterns in {}.\n  \
         3. Run `bca check` to verify the gate (the manifest is auto-discovered).",
        manifest_path.display(),
        bcaignore_path.display(),
    );
}

/// Diff two baseline files and print the structured result (issue #382).
///
/// Both files are loaded through [`load_baseline`] — the same reader
/// `bca check` uses — so a supported legacy version is migrated on read
/// and an unsupported version dies with a clear message (exit 1) rather
/// than silently no-matching every entry. The matcher's `tolerance` and
/// `fuzzy` parameters do not influence the flattened entry set, so the
/// defaults are passed: the diff keys on `(path, qualified, metric)`
/// regardless.
///
/// Always exits 0 on success; the diff is informational, not a gate.
fn run_command_diff_baseline(args: DiffBaselineArgs) {
    let old = load_baseline(&args.old, baseline::DEFAULT_LINE_TOLERANCE, false);
    let new = load_baseline(&args.new, baseline::DEFAULT_LINE_TOLERANCE, false);
    let diff = BaselineDiff::compute(&old.diff_entries(), &new.diff_entries());
    let filter = SectionFilter::from_flags([
        args.added_only,
        args.removed_only,
        args.worsened_only,
        args.improved_only,
    ]);
    let rendered = match args.format {
        OutputFormat::Tty => diff.render_tty(filter),
        OutputFormat::Markdown => diff.render_markdown(filter),
        // Serialization of a fixed-shape struct of owned scalars cannot
        // fail in practice; surface any future error as a tool error
        // rather than panicking.
        OutputFormat::Json => diff
            .render_json()
            .unwrap_or_else(|e| die(format_args!("failed to serialize diff to JSON: {e}"))),
    };
    write_stdout_or_die(rendered.as_bytes());
}

fn run_command_diff(globals: GlobalOpts, args: crate::DiffArgs) {
    let diff = if let Some(since_ref) = args.since.as_deref() {
        // `--since` takes at most one positional (the after-side tree),
        // which clap binds to `old` first. A second positional (`new`)
        // is ambiguous in this mode, so reject it with a clear message
        // rather than silently ignoring it.
        if args.new.is_some() {
            die(
                "bca diff --since takes at most one positional (the after-side tree); \
                 omit it to diff against the working tree",
            );
        }
        compute_since_diff(&globals, &args, since_ref)
    } else {
        // File/dir mode: both positionals are required captured metric
        // sets, so reaching here with `--since` absent means `old`/`new`
        // came from the positionals; enforce both are present.
        let Some(old) = args.old.as_deref() else {
            die("bca diff: provide two metric-output paths (<old> <new>) or use --since <ref>");
        };
        let Some(new) = args.new.as_deref() else {
            die("bca diff: missing <new> metric-output path (or use --since <ref>)");
        };
        crate::metric_diff::MetricDiff::compute(old, new, args.min_change, &args.metric)
    }
    .unwrap_or_else(|e| die(format_args!("{e}")));
    let rendered = match args.format {
        OutputFormat::Tty => diff.render_tty(),
        OutputFormat::Markdown => diff.render_markdown(),
        // Serialization of a fixed-shape struct of owned scalars cannot
        // fail in practice; surface any future error as a tool error
        // rather than panicking.
        OutputFormat::Json => diff
            .render_json()
            .unwrap_or_else(|e| die(format_args!("failed to serialize diff to JSON: {e}"))),
    };
    write_stdout_or_die(rendered.as_bytes());
}

/// Compute the `bca diff --since <ref>` diff: materialize the tree at
/// `since_ref` into an auto-cleaning tempdir, run the metric walk
/// against it (the before side) and against the after side (the
/// optional positional source tree or the current working tree), then
/// reuse `metric_diff`'s bucketing. Both sides honor the same
/// `--paths`/`--include`/`--exclude` selection.
///
/// Ref/checkout failures are hard errors (exit 1) per #492, surfaced via
/// `die` in the caller; this returns `DiffError` for the file-walk /
/// JSON-load failures `metric_diff` already models.
fn compute_since_diff(
    globals: &GlobalOpts,
    args: &crate::DiffArgs,
    since_ref: &str,
) -> Result<crate::metric_diff::MetricDiff, crate::metric_diff::DiffError> {
    // Hard-error early on an unresolvable ref / non-git checkout, before
    // creating any temp state, so nothing needs cleaning up on this path.
    diff::validate_since_ref(since_ref).unwrap_or_else(|reason| die(reason));

    // TempDir auto-removes on drop — including every `?` below — so the
    // "no leftover temp trees, even on error" acceptance holds without
    // manual teardown.
    let before_tree = tempfile::TempDir::new().map_err(io_to_diff_error)?;
    diff::materialize_tree(since_ref, before_tree.path()).unwrap_or_else(|reason| die(reason));

    let before_json = tempfile::TempDir::new().map_err(io_to_diff_error)?;
    let before = crate::walk_metric_set(
        before_tree.path(),
        side_globals(globals),
        before_json.path(),
    )?;

    // After side: the optional positional source tree, else the working
    // tree. In `--since` mode the single trailing positional is bound to
    // `args.old` by clap; the caller has already rejected a second
    // positional (`args.new`). `walk_metric_set` anchors the CWD at this
    // root, so the before/after keys (root-relative) line up.
    let after_root = match args.old.as_deref() {
        Some(new_tree) => new_tree.to_path_buf(),
        None => std::env::current_dir().map_err(io_to_diff_error)?,
    };
    let after_json = tempfile::TempDir::new().map_err(io_to_diff_error)?;
    let after = crate::walk_metric_set(&after_root, side_globals(globals), after_json.path())?;

    Ok(crate::metric_diff::MetricDiff::from_sets(
        &before,
        &after,
        args.min_change,
        &args.metric,
    ))
}

/// Build the per-side [`GlobalOpts`] for a `--since` metric walk: clone
/// the user's globals (carrying `--include`/`--exclude` and the rest)
/// but pin `paths` to `.` so the walk is anchored at the side's tree
/// root (`walk_metric_set` sets the CWD there). When the user passed
/// explicit `--paths`, honor them as-is — they are interpreted relative
/// to each tree root, keeping both sides on the same file set.
fn side_globals(globals: &GlobalOpts) -> GlobalOpts {
    let mut side = globals.clone();
    if side.paths.is_empty() {
        side.paths = vec![PathBuf::from(".")];
    }
    // `paths_from` is a captured file outside the tree; consuming it on
    // both sides would double-read it relative to the wrong roots, so
    // drop it — `--since` selection is via `--paths`/globs only.
    side.paths_from = None;
    side
}

/// Adapt a `std::io::Error` raised while creating temp state for the
/// `--since` walk into the `DiffError` the caller already renders.
fn io_to_diff_error(source: std::io::Error) -> crate::metric_diff::DiffError {
    crate::metric_diff::DiffError::Read {
        path: PathBuf::from("<temp>"),
        source,
    }
}

/// Default baseline file audited by `bca exemptions` when neither
/// `--baseline` nor `bca.toml`'s `[check] baseline` is set. Matches the
/// filename `bca init` scaffolds and `bca check --write-baseline`
/// defaults to.
const DEFAULT_BASELINE_FILE: &str = ".bca-baseline.toml";

/// Audit everything the `bca check` gate skips in one report (issue
/// #386): in-source suppression markers, `[check.exclude]` globs, and
/// `.bca-baseline.toml` entries.
///
/// Read-only and always exits 0 on success — the report is a review
/// surface, not a gate. Each section is opt-out via the mutually
/// exclusive `--only-*` flags (none set = all three). The baseline
/// (`bca.toml` top-level `baseline`) and exclude (`[check] exclude`)
/// inputs default to the same sources `bca check` reads, so the audit
/// reflects what the gate would skip.
fn run_command_exemptions(
    globals: GlobalOpts,
    mut args: ExemptionsArgs,
    manifest: Option<&Manifest>,
    preproc: Option<Arc<PreprocResults>>,
) {
    // Merge `bca.toml` `[check]` defaults (baseline path, exclude globs)
    // under the CLI flags — CLI wins, mirroring `bca check`.
    if let Some(m) = manifest {
        m.merge_exemptions(&mut args);
    }

    // Validate `--output` before the (slower) walk so a bad path fails
    // fast, mirroring `run_command_report`.
    if let Some(ref output) = args.output {
        if output.exists() && output.is_dir() {
            die("--output must be a file path for `exemptions`");
        }
        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            die(format_args!(
                "parent directory of --output does not exist: {}",
                parent.display()
            ));
        }
    }

    // No `--only-*` flag selects every section; one selects just that
    // one (clap enforces mutual exclusivity).
    let only_any = args.only_markers || args.only_excludes || args.only_baseline;
    let want_markers = !only_any || args.only_markers;
    let want_excludes = !only_any || args.only_excludes;
    let want_baseline = !only_any || args.only_baseline;

    // Resolve the config-driven sections before the walk: it consumes
    // `globals`, and a missing exclude-from file or unparseable baseline
    // should error ahead of the slower tree traversal.
    let excludes = want_excludes.then(|| resolve_exclude_globs(&args));
    let baseline = want_baseline.then(|| resolve_baseline_section(&args));
    let markers = want_markers.then(|| collect_marker_rows(globals, preproc));

    let report = ExemptionsReport {
        markers,
        excludes,
        baseline,
    };
    let rendered = report
        .render(args.format, &args.strip_prefix)
        .unwrap_or_else(|e| die(format_args!("failed to serialize exemptions to JSON: {e}")));
    if let Some(ref output_path) = args.output {
        std::fs::write(output_path, rendered.as_bytes())
            .unwrap_or_else(|e| die_io("write exemptions report to", output_path, e));
    } else {
        write_stdout_or_die(rendered.as_bytes());
    }
}

/// Run the suppression-marker walk and return the flattened rows sorted
/// by `(path, line)` for deterministic output. Files arrive in
/// worker-completion order, so the sort cannot be skipped even though
/// each file's markers are already line-sorted by the collector.
fn collect_marker_rows(
    globals: GlobalOpts,
    preproc: Option<Arc<PreprocResults>>,
) -> Vec<MarkerRow> {
    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = Config {
        exemptions_tx: Some(Mutex::new(tx)),
        ..Config::new(Action::Exemptions, &globals, preproc)
    };
    run_walk(globals, cfg);
    // ConcurrentRunner::run() consumed Config (and thus the Sender).
    // All worker threads have joined, so `rx.into_iter()` terminates.
    let mut rows: Vec<MarkerRow> = rx
        .into_iter()
        .flat_map(|FileMarkers { path, markers }| {
            markers.into_iter().map(move |marker| MarkerRow {
                path: path.clone(),
                marker,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.marker.line.cmp(&b.marker.line))
    });
    rows
}

/// Resolve the `[check.exclude]` glob list for display: the
/// CLI/manifest `check_exclude` values unioned with the lines of
/// `--check-exclude-from`, in that order.
fn resolve_exclude_globs(args: &ExemptionsArgs) -> Vec<String> {
    let mut globs = args.check_exclude.clone();
    if let Some(from) = args.check_exclude_from.as_deref() {
        match read_exclude_patterns_from(from, "--check-exclude-from") {
            Ok(patterns) => globs.extend(patterns),
            Err(e) => die(e),
        }
    }
    globs
}

/// Resolve and load the baseline section. An explicit/manifest
/// `--baseline` path is loaded through the same reader `bca check` uses
/// (dying on a missing or unparseable file). With no path configured,
/// the default `.bca-baseline.toml` is audited when present and reported
/// as empty otherwise — so a zero-config invocation never errors.
fn resolve_baseline_section(args: &ExemptionsArgs) -> BaselineSection {
    let path = if let Some(p) = args.baseline.as_deref() {
        p.to_path_buf()
    } else {
        let default = PathBuf::from(DEFAULT_BASELINE_FILE);
        if !default.exists() {
            // Zero-config: no baseline present, report an empty section
            // rather than erroring.
            return BaselineSection {
                path: DEFAULT_BASELINE_FILE.to_owned(),
                entries: Vec::new(),
            };
        }
        default
    };
    let loaded = load_baseline(&path, baseline::DEFAULT_LINE_TOLERANCE, false);
    let mut entries: Vec<BaselineRow> = loaded
        .diff_entries()
        .into_iter()
        .map(BaselineRow::from)
        .collect();
    entries.sort_by(|a, b| {
        (a.path.as_str(), a.start_line, a.metric.as_str()).cmp(&(
            b.path.as_str(),
            b.start_line,
            b.metric.as_str(),
        ))
    });
    BaselineSection {
        path: path.display().to_string(),
        entries,
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
