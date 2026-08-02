//! `bca check` threshold-layer resolution (manifest, --config, tier, --threshold).

use std::collections::BTreeSet;

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
/// `thresholds` is the gate the walker compares against: the global set
/// plus one fully resolved set per `[thresholds.lang.<slug>]` language
/// (#1141). Each resolved threshold carries both the requested tier's
/// limit and that language's *un-scaled* hard ceiling, so
/// [`classify_check_outcome`] can tell a soft-band encroachment apart
/// from a true hard breach (#385) without re-deriving which table
/// applied.
pub(crate) struct ResolvedThresholds {
    pub(crate) thresholds: Arc<LanguageThresholds>,
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

/// Merge the two file-sourced threshold layers into one set of
/// unresolved tables: the manifest `[thresholds]` table (empty when no
/// `bca.toml` was discovered) with `--config` layered on top, its keys
/// winning on collision so every existing recipe is preserved. The hard,
/// soft, and per-language layers all merge the same way — per-language
/// nested one level deeper, so a `--config` override of one metric for
/// one language leaves that language's other limits alone.
///
/// Split out of [`validate_and_build_thresholds`] so the
/// `--explain-threshold` preview (`super::explain`) starts from exactly
/// the same merged tables the gate does rather than re-deriving them: a
/// preview whose configuration differs from the run it predicts is worse
/// than none.
pub(crate) fn merge_threshold_layers(
    args: &CheckArgs,
    base_thresholds: ParsedThresholds,
) -> ParsedThresholds {
    let ParsedThresholds {
        mut hard,
        mut soft,
        mut lang,
    } = base_thresholds;
    if let Some(config) = args.config.as_deref() {
        let cfg = load_threshold_config(config);
        hard.extend(cfg.hard);
        soft.extend(cfg.soft);
        for (slug, overrides) in cfg.lang {
            lang.entry(slug).or_default().extend(overrides);
        }
    }
    ParsedThresholds { hard, soft, lang }
}

/// Validate `--output` / `--output-format` pairing, then resolve the
/// effective threshold sets per the documented resolution order
/// (#373/#374/#375/#380): the `merged` manifest + `--config` base from
/// [`merge_threshold_layers`], the per-language
/// `[thresholds.lang.<slug>]` overrides layered per metric (#1141), the
/// tier resolution (hard verbatim, or soft via `[thresholds.soft]` /
/// `--headroom`), and finally the absolute `--threshold` CLI overrides.
/// Dies if no thresholds were configured. The result is wrapped in `Arc`
/// so it can be cloned into each walker worker's `Config`.
pub(crate) fn validate_and_build_thresholds(
    args: &mut CheckArgs,
    merged: ParsedThresholds,
    tier: TierSpec,
) -> ResolvedThresholds {
    // Resolve the `--output` / `--format` pairing before the walk so a
    // misconfigured invocation fails fast instead of after a full parse.
    resolve_check_output_format(args);

    let ParsedThresholds { hard, soft, lang } = merged;

    // The soft ratio (the `RATIO` in `--tier=soft=RATIO`) was already
    // validated to `(0, 1]` by `TierSpec::from_str` at parse time, so a
    // typo is a clap usage error before we ever reach here.

    // Capture whether a soft table is configured before the resolver
    // borrows `soft`, so provenance resolution (#486) matches the same
    // branch the tier resolver takes.
    let soft_table_present = !soft.is_empty();

    let cli = canonical_cli_thresholds(&args.thresholds);
    let layers = SharedLayers::new(&cli, tier, &soft, &lang, &hard);
    let thresholds = layers.resolve_all(&hard, &lang);
    if thresholds.is_empty() {
        die(
            "no thresholds configured; pass --threshold, --config, or a bca.toml [thresholds] table",
        );
    }
    // Legal, but worth saying out loud: with no global `[thresholds]`,
    // every language outside the listed set is walked and gated against
    // nothing, and reports a clean exit 0 (#1141).
    if let Some(gated) = thresholds.languages_gated_without_a_global_table() {
        note(format!(
            "no global [thresholds] table: only {} {} gated; every other language \
             passes unconditionally",
            gated.join(", "),
            if gated.len() == 1 { "is" } else { "are" },
        ));
    }
    ResolvedThresholds {
        thresholds: Arc::new(thresholds),
        provenance: resolve_provenance(tier, soft_table_present),
    }
}

/// Canonicalise the `--threshold` metric names so the CLI layer merges
/// with the manifest, `--config`, and `[thresholds.lang.<slug>]` layers
/// by *metric* rather than by spelling (#1165) — `--threshold ploc=100`
/// must override a manifest `"loc.ploc"`, not gate it a second time.
///
/// Deliberately **not** done inside `parse_cli_threshold`, which is the
/// clap `value_parser` for `--threshold`. That parser owns the
/// `metric=limit` *syntax*; resolving a name against the metric registry
/// is a semantic check, and the sibling semantic check —
/// `--threshold not_a_metric=1`, rejected by
/// [`ThresholdSet::build_tiered`] with the did-you-mean list — already
/// lives here. Split across the two layers, two adjacent typo classes
/// would report through different surfaces: an ambiguous family head
/// wrapped as clap's `invalid value '…' for '--threshold <THRESHOLDS>'`,
/// an unknown metric as the plain `error:` form.
///
/// Note this is *not* an exit-code argument. `exit_clap_error`
/// (#561/#594) already remaps clap's usage exit 2 to `EXIT_TOOL_ERROR`
/// precisely so `bca check`'s exit 2 stays reserved for "thresholds
/// exceeded", so either placement exits 1.
///
/// Repeating one metric stays last-wins, as it already is for two
/// occurrences of the same spelling; only the enclosing tables reject a
/// metric named twice.
pub(crate) fn canonical_cli_thresholds(raw: &[(String, f64)]) -> Vec<(String, f64)> {
    raw.iter()
        .map(|(name, limit)| {
            let canonical = crate::metric_alias::normalize_for_check(name)
                .unwrap_or_else(|e| die(format_args!("--threshold: {e}")));
            (canonical.into_owned(), *limit)
        })
        .collect()
}

/// The per-metric override tables, keyed by canonical language slug.
type LanguageOverrides = BTreeMap<&'static str, BTreeMap<String, f64>>;

/// Resolve the gate a `--explain-threshold` preview walks with (#1169).
///
/// `candidates` replaces the global `[thresholds]` hard limits for
/// exactly the metrics being explained, and every other metric is dropped
/// — the preview reports only what it was asked about, and narrowing the
/// set narrows the metric families the walk computes with it (#1113).
///
/// Resolved at the **soft** tier on purpose. The soft band is the more
/// permissive of the two (it fires *before* the hard gate), so one walk
/// against it collects a superset already containing every hard-tier
/// offender, and each emitted [`Violation`] carries both numbers:
/// `limit` is that language's resolved soft limit and `hard_limit` its
/// candidate ceiling. Counting the hard tier is then a `breaches_limit`
/// filter over the records in hand rather than a second parse of the
/// tree.
///
/// Per-language `[thresholds.lang.<slug>]` overrides of an explained
/// metric are kept as they are: a candidate *global* limit does not
/// change what a language that overrode the metric gates at, and each
/// language's soft band is derived from its own limit by
/// [`SharedLayers::resolve_one`]. That is what keeps the counts exact on
/// the trees #1141 exists for.
pub(crate) fn build_candidate_gate(
    layers: &ParsedThresholds,
    candidates: &BTreeMap<String, f64>,
    ratio: Option<f64>,
) -> LanguageThresholds {
    let soft = retain_explained(&layers.soft, candidates);
    let lang: LanguageOverrides = layers
        .lang
        .iter()
        .map(|(slug, overrides)| (*slug, retain_explained(overrides, candidates)))
        .filter(|(_, kept)| !kept.is_empty())
        .collect();
    // No `--threshold` layer: an absolute CLI override is never scaled,
    // so letting one through would hand the preview a metric with no soft
    // tier to report. `run_explain_thresholds` rejects the overlap up
    // front instead.
    let shared = SharedLayers::new(&[], TierSpec::Soft(ratio), &soft, &lang, candidates);
    shared.resolve_all(candidates, &lang)
}

/// Drop every entry of one threshold layer whose metric is not being
/// explained. Shared by the `[thresholds.soft]` table and each
/// `[thresholds.lang.<slug>]` table so both are narrowed by the same
/// rule.
fn retain_explained<V: Copy>(
    table: &BTreeMap<String, V>,
    candidates: &BTreeMap<String, f64>,
) -> BTreeMap<String, V> {
    table
        .iter()
        .filter(|(name, _)| candidates.contains_key(name.as_str()))
        .map(|(name, value)| (name.clone(), *value))
        .collect()
}

/// The threshold layers every table in one run shares, separated from
/// the per-table hard limits they are applied to.
///
/// Grouping them is not just parameter hygiene: it is what makes "the
/// soft tier is derived per language" checkable at a glance. The soft
/// overrides and the CLI layer are the *same* for every table; only
/// `hard` differs, and that is precisely why each language's soft band
/// comes out scaled from its own limit.
struct SharedLayers<'a> {
    /// Absolute `--threshold` overrides, applied last to every table.
    cli: &'a [(String, f64)],
    tier: TierSpec,
    /// The global `[thresholds.soft]` overrides, resolved afresh against
    /// whichever hard table is being built.
    soft: &'a BTreeMap<String, SoftLimit>,
    /// Metrics that *some* table gives a hard limit. A scale-relative
    /// soft entry needs a base to scale, and since #1141 that base may
    /// live only in a `[thresholds.lang.*]` table — so a table without
    /// one skips the entry rather than failing the whole run.
    hard_somewhere: BTreeSet<&'a str>,
}

impl<'a> SharedLayers<'a> {
    fn new(
        cli: &'a [(String, f64)],
        tier: TierSpec,
        soft: &'a BTreeMap<String, SoftLimit>,
        lang: &'a LanguageOverrides,
        hard: &'a BTreeMap<String, f64>,
    ) -> Self {
        let mut hard_somewhere: BTreeSet<&str> = hard.keys().map(String::as_str).collect();
        for overrides in lang.values() {
            hard_somewhere.extend(overrides.keys().map(String::as_str));
        }
        Self {
            cli,
            tier,
            soft,
            hard_somewhere,
        }
    }

    /// Resolve the global table plus one fully resolved table per
    /// language carrying an override (#1141).
    fn resolve_all(
        &self,
        hard: &BTreeMap<String, f64>,
        lang: &LanguageOverrides,
    ) -> LanguageThresholds {
        let per_language = lang
            .iter()
            .map(|(slug, overrides)| {
                // Per-metric override with inheritance, not wholesale
                // replacement: start from the global hard table so a
                // language that raises `cognitive` still gates everything
                // else at the project limit.
                let mut lang_hard = hard.clone();
                lang_hard.extend(overrides.iter().map(|(k, v)| (k.clone(), *v)));
                let context = format!("[thresholds.lang.{slug}]");
                (*slug, self.resolve_one(lang_hard, Some(&context)))
            })
            .collect();
        LanguageThresholds::new(self.resolve_one(hard.clone(), None), per_language)
    }

    /// Resolve one hard table — the global one, or a language's —
    /// through the tier and the `--threshold` CLI layer.
    ///
    /// The soft tier is **derived from `hard`**, which is already this
    /// table's resolved hard limits — so a language that loosens a limit
    /// gets a soft band scaled from *its* limit, never from the
    /// project-wide one. That is what keeps the soft band from sitting
    /// above the ceiling its offenders are escalated against: were it
    /// derived globally, every function between the project limit and
    /// the language's own would report a hard breach (exit 5) while
    /// sitting inside the limit configured for it. There is deliberately
    /// no `[thresholds.lang.<slug>.soft]` syntax; the derivation needs
    /// none.
    ///
    /// `--threshold` is applied last and absolutely, to the resolved
    /// limit *and* the hard ceiling, for every table: a limit the user
    /// typed on the command line means exactly that number, and a
    /// per-language table must not quietly outrank it.
    ///
    /// `context` names the offending table in a build error, and is
    /// `None` for the global set so its long-standing message is
    /// unprefixed.
    fn resolve_one(&self, mut hard: BTreeMap<String, f64>, context: Option<&str>) -> ThresholdSet {
        // Drop a scale-relative soft entry whose metric is gated by some
        // *other* table: this one has no limit for it to scale, and
        // nothing to gate either. Where no table supplies a base, the
        // entry is left in so `resolve_tier` reports the original "no
        // hard limit exists" error rather than silently dropping a
        // threshold (#1141).
        let soft: BTreeMap<String, SoftLimit> = self
            .soft
            .iter()
            .filter(|(name, limit)| {
                !matches!(limit, SoftLimit::Scale(_))
                    || hard.contains_key(name.as_str())
                    || !self.hard_somewhere.contains(name.as_str())
            })
            .map(|(name, limit)| (name.clone(), *limit))
            .collect();
        let mut merged = resolve_tier(self.tier, hard.clone(), &soft);
        for (name, limit) in self.cli {
            merged.insert(name.clone(), *limit);
            hard.insert(name.clone(), *limit);
        }
        ThresholdSet::build_tiered(&merged, &hard).unwrap_or_else(|e| match context {
            Some(table) => die(format_args!("{table}: {e}")),
            None => die(e),
        })
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
    for (name, limit) in &mut out {
        // Keys arrive canonical (#1165), which is what makes this
        // direction lookup correct by construction rather than by luck:
        // scaling a lower-is-worse floor the higher-is-worse way inverts
        // the whole tier (#1166).
        *limit = scale_threshold(*limit, ratio, metric_is_lower_is_worse(name));
    }
    out
}
