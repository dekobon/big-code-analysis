//! `bca check` threshold-layer resolution (manifest, --config, tier, --threshold).

use super::super::*;
use super::*;

/// Default soft-tier scale applied when `--tier=soft` is requested with
/// neither a `[thresholds.soft]` table nor an explicit `--headroom`. A
/// concrete default keeps `--tier=soft` from being a silent no-op (the
/// "config error" the issue #375 resolution order warns against) — it
/// always produces a band tighter than the hard gate.
pub(crate) const DEFAULT_SOFT_HEADROOM: f64 = 0.95;

/// Resolved threshold layers handed back to [`run_check`].
///
/// `set` is the gate the walker compares against (the requested tier's
/// limits). `hard_limits` is the hard-tier limit per metric — equal to
/// `set` at the hard tier, but the *un-scaled* ceilings at the soft
/// tier, so [`classify_check_outcome`] can tell a soft-band
/// encroachment apart from a true hard breach (#385).
pub(crate) struct ResolvedThresholds {
    pub(crate) set: Arc<ThresholdSet>,
    pub(crate) hard_limits: BTreeMap<String, f64>,
    /// Tier/headroom the gate resolved to (issue #486). Stamped into the
    /// baseline on `--write-baseline` and compared against a loaded
    /// baseline's recorded provenance to warn on a stricter-than-baseline
    /// desync.
    pub(crate) provenance: baseline::Provenance,
}

/// Reduce the resolved tier + soft-table presence to the
/// [`baseline::Provenance`] stamped on a write and compared on a read
/// (issue #486). Mirrors the tier-resolution branches in
/// [`resolve_tier`]: hard → no scaling; soft with a `[thresholds.soft]`
/// table → per-metric limits (no single ratio); soft without a table →
/// scaled by the spec's ratio (defaulting to [`DEFAULT_SOFT_HEADROOM`]).
pub(crate) fn resolve_provenance(tier: TierSpec, soft_table_present: bool) -> baseline::Provenance {
    match tier {
        TierSpec::Hard => baseline::Provenance::hard(),
        TierSpec::Soft(_) if soft_table_present => baseline::Provenance::soft_table(),
        TierSpec::Soft(r) => {
            baseline::Provenance::soft_headroom(r.unwrap_or(DEFAULT_SOFT_HEADROOM))
        }
    }
}

/// Resolve the effective `--format` for `check` and eagerly validate the
/// `--output` path (#600).
///
/// When `--output` is given without `--format`, the format is inferred
/// from the file extension ([`AggregatedFormat::infer_from_extension`]):
/// `.sarif` → SARIF, `.xml` → Checkstyle. An extension with no unique
/// writer (e.g. `.json`, shared by SARIF and Code Climate) or no
/// extension at all is a usage error — the previous silent no-op (an
/// explicit `--output` that wrote nothing on exit 0) was the worst CLI
/// failure mode. When a format is in effect, an `--output` that names an
/// existing directory is rejected before the walk rather than failing
/// mid-write.
///
/// Invoking `bca check` with neither flag is left alone: the plain
/// human stderr stream is the default contract and stays frictionless.
///
/// The `--output` path is only checked for being a directory; a missing
/// parent is *not* rejected here, because the aggregated writer creates
/// parent directories on demand (`write_to_path_or_stdout`), and that
/// on-demand creation is a long-standing part of `check`'s contract.
pub(crate) fn resolve_check_output_format(args: &mut CheckArgs) {
    let Some(ref out) = args.output else {
        // No `--output`: nothing to validate or infer. A bare `--format`
        // still writes its document to stdout via the existing path.
        return;
    };

    // The effective format: the explicit `--format`, or one inferred from
    // the extension. An un-inferable extension is a usage error.
    let fmt = args.output_format.unwrap_or_else(|| {
        check_format::AggregatedFormat::infer_from_extension(out).unwrap_or_else(|| {
            die(format_args!(
                "--output {} has no format-bearing extension; pass --format \
                 (checkstyle|clang-warning|code-climate|msvc-warning|sarif) \
                 or use a recognized extension (.sarif, .xml)",
                out.display()
            ))
        })
    });

    // A format is now in effect (explicit or inferred), so the path is a
    // real sink: reject a directory up front rather than failing mid-write.
    if out.exists() && out.is_dir() {
        die(format_args!(
            "--output must be a file path for `check --format {}`",
            fmt.name()
        ));
    }

    args.output_format = Some(fmt);
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
pub(crate) fn validate_and_build_thresholds(
    args: &mut CheckArgs,
    base_thresholds: ParsedThresholds,
    tier: TierSpec,
) -> ResolvedThresholds {
    // Resolve the `--output` / `--format` pairing before the walk so a
    // misconfigured invocation fails fast instead of after a full parse.
    resolve_check_output_format(args);

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

    // The soft ratio (the `RATIO` in `--tier=soft=RATIO`) was already
    // validated to `(0, 1]` by `TierSpec::from_str` at parse time, so a
    // typo is a clap usage error before we ever reach here.

    // Capture whether a soft table is configured before `resolve_tier`
    // borrows `soft`, so provenance resolution (#486) matches the same
    // branch the tier resolver takes.
    let soft_table_present = !soft.is_empty();

    // Layer 3: tier resolution. Produces the per-metric limits the gate
    // compares against. Clone `hard` so the un-scaled hard-tier limits
    // survive for #385 hard-breach detection below.
    let mut merged = resolve_tier(tier, hard.clone(), &soft);

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
        provenance: resolve_provenance(tier, soft_table_present),
    }
}

/// Resolve the per-metric limits for the requested tier (#375/#688).
///
/// The soft ratio rides on the [`TierSpec`] itself — there is no longer
/// a separate `--headroom` knob to reconcile — so the precedence model
/// collapses to:
///
/// - `Hard`: the `[thresholds]` table verbatim. `[thresholds.soft]` is
///   ignored entirely.
/// - `Soft` with a `[thresholds.soft]` table: merge the per-metric soft
///   overrides on top of the hard limits (metrics absent from the soft
///   table keep their hard limit — no soft band). The blanket ratio, if
///   one was pinned with `soft=RATIO`, is not applied: explicit
///   per-metric limits encode intent more precisely than a multiplier.
/// - `Soft` without a soft table: scale every hard limit by the spec's
///   ratio (defaulting to [`DEFAULT_SOFT_HEADROOM`] for a bare `soft`).
pub(crate) fn resolve_tier(
    tier: TierSpec,
    hard: BTreeMap<String, f64>,
    soft: &BTreeMap<String, SoftLimit>,
) -> BTreeMap<String, f64> {
    let ratio = match tier {
        TierSpec::Hard => return hard,
        TierSpec::Soft(_) if !soft.is_empty() => {
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
            return out;
        }
        // No soft table: scale the hard limits by the spec's ratio,
        // defaulting so a bare `--tier=soft` is never a silent no-op.
        TierSpec::Soft(r) => r.unwrap_or(DEFAULT_SOFT_HEADROOM),
    };
    if hard.is_empty() {
        note(
            "--tier=soft has no effect without configured thresholds \
             (bca.toml [thresholds] or --config); --threshold limits are \
             absolute and are not scaled",
        );
    }
    let mut out = hard;
    for limit in out.values_mut() {
        *limit = scale_threshold(*limit, ratio);
    }
    out
}
