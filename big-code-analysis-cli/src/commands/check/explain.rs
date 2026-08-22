//! `bca check --explain-threshold <metric>=<limit>`: what a candidate
//! limit would cost, at **both** tiers, without editing anything (#1169).
//!
//! Tightening a limit onto a cluster of existing values is free at the
//! hard tier and expensive at the soft one, and the hard-tier reading is
//! the one a person naturally takes. `bca check --threshold nargs=6` over
//! this repository reported zero offenders — every one was already
//! baselined — while the same limit written into `bca.toml` puts 73
//! functions permanently inside the `0.95` soft band, because they sit at
//! exactly 6 and the band starts at 5.7. They cannot clear it; they *are*
//! the limit.
//!
//! `--threshold` cannot be made to show this. Its values are applied last
//! and absolutely, never scaled, so the one-command way to trial a
//! candidate limit is by contract the one way that has no soft tier to
//! report. Hence a separate surface.
//!
//! # Why the counts match a real run
//!
//! Everything downstream of the walk is the gate's own code, in the
//! gate's own order, because both run it through the same two helpers:
//! `collect_check_violations` (walk, empty-input guard,
//! [`apply_check_exclude`]) and `classify_check_violations`
//! ([`filter_by_baseline`], [`apply_changed_only`]). A stage added to
//! the gate therefore reaches the preview too, rather than compiling
//! here while quietly predicting a different run. In-source suppression
//! markers are honoured inside the walk exactly as they always are. The
//! only deliberate difference is that baseline-covered offenders are
//! *kept* (tagged `Covered`) instead of dropped, because the split
//! between "already baselined" and "new" is the number a reviewer
//! weighs — 134 soft offenders of which 61 are baselined means 73 new
//! entries.
//!
//! That split is safe to read off a single walk because
//! [`Baseline::classify`](crate::baseline::Baseline::classify) is a
//! function of the offender's path, symbol, metric, and *value* — never
//! of the limit it breached. One walk at the soft limit therefore yields
//! coverage tags that are correct for both tiers.

use super::*;

/// Minimum number of soft-band offenders before a shared value is called
/// a cluster. Below ten, "they all sit at N" describes a handful rather
/// than a population, and the offender counts printed above already say
/// everything a reviewer needs.
const CLUSTER_MIN_FUNCTIONS: usize = 10;

/// Minimum share of the soft band a single value must account for to be
/// called a cluster. A simple majority is the point at which converging
/// the limit onto that value moves most of the band at once — which is
/// the cost this report exists to surface.
const CLUSTER_MIN_SHARE: f64 = 0.5;

/// Run the candidate-limit preview. This *replaces* the gate: nothing
/// here consults or produces an exit code, so the invocation exits 0
/// unless a tool error kills it first.
pub(crate) fn run_explain_thresholds(
    globals: GlobalOpts,
    args: &CheckArgs,
    layers: &ParsedThresholds,
    tier: TierSpec,
    preproc: Option<Arc<PreprocResults>>,
) {
    reject_summary_file_path(args);
    let candidates = candidate_limits(args);
    // The soft band is derived from the candidate, so the tier the user
    // asked to *gate* at does not apply — the report covers both tiers
    // regardless. A `--tier=soft=R` still pins the ratio; the `hard`
    // default leaves it at `DEFAULT_SOFT_HEADROOM`, which is what
    // `make self-scan-headroom` and every other proportional soft gate
    // use.
    let ratio = tier.ratio();
    let (resolved, global_merge_mode) = build_candidate_gate(layers, &candidates, ratio);
    let resolved = Arc::new(resolved);

    let CollectedViolations {
        violations, scope, ..
    } = collect_check_violations(globals, args, preproc, Arc::clone(&resolved));
    let pairs = classify_check_violations(
        violations,
        args,
        scope.as_ref(),
        // The provenance the *user's* tier would stamp, not the soft tier
        // this walk ran at: the walk is soft only to collect a superset,
        // and warning that the baseline may under-cover would report an
        // artifact of that implementation choice as a fact about the run
        // being previewed.
        resolve_provenance(tier, !layers.soft.is_empty()),
        // Keep baseline-covered offenders instead of dropping them: the
        // split between "already baselined" and "new" is the number the
        // report exists to show.
        true,
    );

    let context = CandidateGate {
        layers,
        resolved: &resolved,
        ratio,
        global_merge_mode,
    };
    let report: Vec<CandidateOutcome> = candidates
        .iter()
        .map(|(metric, candidate)| context.explain(metric, *candidate, &pairs))
        .collect();
    write_report(&report);
}

/// The candidate configuration one preview run shares across every metric
/// it explains: the merged manifest layers the candidates were spliced
/// into, the gate resolved from them, and the proportional soft ratio in
/// effect. Grouped because all three answer one question — "what would
/// this limit resolve to, and where" — and every per-metric lookup below
/// needs the same three.
struct CandidateGate<'a> {
    layers: &'a ParsedThresholds,
    resolved: &'a LanguageThresholds,
    ratio: Option<f64>,
    /// Whether the predicted run's global table resolves its soft tier
    /// in merge mode. Decided by [`build_candidate_gate`] with the same
    /// per-table filter the resolver applies — manifest-table emptiness
    /// is not it, because a scale-relative entry whose only hard base
    /// lives in a `[thresholds.lang.*]` override drops out of the
    /// global table and leaves the predicted run in ratio mode.
    global_merge_mode: bool,
}

impl CandidateGate<'_> {
    /// Tally one candidate limit over the offenders the shared walk
    /// produced.
    fn explain(
        &self,
        metric: &str,
        candidate: f64,
        pairs: &[(Violation, Option<Coverage>)],
    ) -> CandidateOutcome {
        // Resolving under the caller's spelling is also the proof that it
        // matches `Violation::metric`: the gate yields each entry's
        // registry name, so a hit means the two strings are the same one
        // and the filter below cannot silently select nothing.
        let global_soft = resolved_limit(self.resolved.global(), metric).unwrap_or_else(|| {
            die(format_args!(
                "--explain-threshold {metric}: candidate limit did not resolve to a gated metric"
            ))
        });
        let mut outcome = CandidateOutcome {
            metric: metric.to_owned(),
            candidate,
            hard: TierTally::new(candidate),
            soft: TierTally::new(global_soft),
            soft_derivation: self.soft_derivation(metric),
            band_values: Vec::new(),
            language_overrides: self.language_overrides(metric, candidate, global_soft),
        };
        // `v.suppressed` is only ever set under `--report-suppressed`,
        // which keeps marker-silenced offenders in the stream for the
        // SARIF document. `run_check` partitions them away from the gate;
        // dropping them here is the same partition, so the preview counts
        // what the gate would count under the user's own flags rather
        // than what one report format happens to surface.
        for (v, coverage) in pairs
            .iter()
            .filter(|(v, _)| v.metric == outcome.metric && !v.suppressed)
        {
            // Every record here already breaches its language's soft
            // limit — that is what the walk gated on. The hard tier is the
            // subset that also breaches that language's candidate ceiling,
            // which the walk stamped on the violation.
            outcome.soft.record(coverage.as_ref());
            let hard_breach = v
                .hard_limit
                .is_some_and(|ceiling| breaches_limit(v.value, ceiling, v.lower_is_worse));
            if hard_breach {
                outcome.hard.record(coverage.as_ref());
            } else {
                outcome.band_values.push(v.value);
            }
        }
        outcome
    }

    /// Where this metric's soft limit came from. A `[thresholds.soft]`
    /// entry overrides the proportional ratio, so the two are exclusive
    /// — and a soft table naming *some other* explained metric suppresses
    /// the ratio for this one without giving it a band of its own.
    fn soft_derivation(&self, metric: &str) -> SoftDerivation {
        if self.layers.soft.contains_key(metric) {
            SoftDerivation::Table
        } else if self.global_merge_mode {
            SoftDerivation::Inherited
        } else {
            SoftDerivation::Ratio(self.ratio.unwrap_or(DEFAULT_SOFT_HEADROOM))
        }
    }

    /// Languages gating this metric at something other than the candidate.
    // Both comparands are threshold *limits*, not measurements: `hard` is
    // a config value verbatim and `soft` is `scale_threshold` applied to
    // one, so two limits derived the same way from the same base are
    // bit-identical and an exact comparison is the contract. Mirrors the
    // module-level allow in `crate::threshold_lang`.
    #[allow(clippy::float_cmp)]
    fn language_overrides(
        &self,
        metric: &str,
        candidate: f64,
        global_soft: f64,
    ) -> Vec<LanguageOverride> {
        self.resolved
            .languages()
            .filter_map(|(slug, set)| {
                let soft = resolved_limit(set, metric)?;
                let hard = self
                    .layers
                    .lang
                    .get(slug)
                    .and_then(|overrides| overrides.get(metric))
                    .copied()
                    .unwrap_or(candidate);
                // A language table that overrides only *other* metrics
                // resolves this one exactly as the global set does; the
                // candidate applies there in full, so it earns no line.
                (hard != candidate || soft != global_soft).then_some(LanguageOverride {
                    slug,
                    hard,
                    soft,
                })
            })
            .collect()
    }
}

/// One tier's share of the answer.
struct TierTally {
    limit: f64,
    total: usize,
    /// Offenders that matched a `--baseline` entry, whether or not the
    /// value worsened against it. The complement (`total - baselined`) is
    /// the number of *new* entries a baseline refresh at this limit would
    /// add, which is the figure a reviewer weighs.
    baselined: usize,
}

impl TierTally {
    fn new(limit: f64) -> Self {
        Self {
            limit,
            total: 0,
            baselined: 0,
        }
    }

    fn record(&mut self, coverage: Option<&Coverage>) {
        self.total += 1;
        if matches!(
            coverage,
            Some(Coverage::Covered { .. } | Coverage::Regressed { .. })
        ) {
            self.baselined += 1;
        }
    }

    fn new_entries(&self) -> usize {
        self.total - self.baselined
    }
}

/// The whole answer for one candidate limit.
struct CandidateOutcome {
    metric: String,
    candidate: f64,
    hard: TierTally,
    soft: TierTally,
    /// How the soft limit was derived, for the report line.
    soft_derivation: SoftDerivation,
    /// Values of the offenders that breach the soft band but *not* the
    /// candidate ceiling — the population the candidate would newly place
    /// in the early-warning band. Cluster detection runs over these.
    band_values: Vec<f64>,
    /// Languages whose `[thresholds.lang.<slug>]` table keeps its own
    /// limits for this metric. Their files are counted against those
    /// numbers, not the candidate.
    language_overrides: Vec<LanguageOverride>,
}

impl CandidateOutcome {
    /// The single value most of the soft band sits on, when there is one.
    fn cluster(&self) -> Option<(f64, usize)> {
        let band = self.band_values.len();
        if band < CLUSTER_MIN_FUNCTIONS {
            return None;
        }
        let mut sorted = self.band_values.clone();
        sorted.sort_by(f64::total_cmp);
        let (value, run) = sorted
            .chunk_by(|a, b| a.total_cmp(b).is_eq())
            .map(|run| (run[0], run.len()))
            .max_by_key(|(_, len)| *len)?;
        #[allow(clippy::cast_precision_loss)]
        let share = run as f64 / band as f64;
        (share >= CLUSTER_MIN_SHARE).then_some((value, run))
    }
}

/// How a candidate's soft limit was derived, for the report line.
enum SoftDerivation {
    /// A `[thresholds.soft]` entry names this metric, so the soft limit
    /// is that table's value and no ratio was applied to it.
    Table,
    /// A `[thresholds.soft]` table is in force but names only *other*
    /// explained metrics. `resolve_tier` merges such a table onto the
    /// hard limits rather than scaling them, so this metric keeps its
    /// hard limit verbatim and has no soft band at all — reporting a
    /// ratio here would name an arithmetic step that never ran.
    Inherited,
    /// No soft table applies, so the soft limit is the candidate scaled
    /// by the proportional ratio in effect.
    Ratio(f64),
}

impl std::fmt::Display for SoftDerivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Table => f.write_str("[thresholds.soft]"),
            Self::Inherited => f.write_str(
                "no soft band; [thresholds.soft] names other metrics, so the hard limit stands",
            ),
            Self::Ratio(ratio) => write!(f, "{}x", MetricScalar(*ratio)),
        }
    }
}

/// One language gating this metric at something other than the
/// candidate. Named fields rather than a positional `(slug, hard,
/// soft)`: the two limits are same-typed, and transposing them in a
/// tuple compiles and prints a plausible-but-wrong note line.
struct LanguageOverride {
    slug: &'static str,
    hard: f64,
    soft: f64,
}

/// Reject an explicit `--summary-file <path>` alongside the preview.
///
/// `--explain-threshold` returns before `emit_check_results`, so the
/// markdown digest is never appended — the user names a destination and
/// finds it untouched, which is the silent no-op the structurally
/// identical `--output` is already a hard usage error for.
///
/// Not expressible in the flag's `conflicts_with_all`, which fires on
/// the *presence* of `--summary-file` and would take the keyword forms
/// with it. `auto` — the default, and what a GHA workflow leaves
/// implicit — must keep working: it names no destination of its own, so
/// producing no step summary is the same thing every other non-gating
/// run does. Only an explicit path is a promise this command breaks.
fn reject_summary_file_path(args: &CheckArgs) {
    if let Some(SummaryFile::Path(path)) = &args.summary_file {
        die(format_args!(
            "--explain-threshold cannot be used with --summary-file {}: the \
             preview replaces the gate, so there are no results to digest \
             into that file",
            path.display()
        ));
    }
}

/// Resolve `--explain-threshold` into canonical `metric -> limit` pairs,
/// rejecting the two ways the request can contradict itself.
fn candidate_limits(args: &CheckArgs) -> BTreeMap<String, f64> {
    let mut candidates: BTreeMap<String, f64> = BTreeMap::new();
    for (name, limit) in canonical_cli_thresholds("--explain-threshold", &args.explain_thresholds) {
        // Two candidate limits for one metric cannot both be previewed in
        // one walk, and silently keeping the last would answer a question
        // the user did not ask.
        if let Some(previous) = candidates.insert(name.clone(), limit) {
            die(format_args!(
                "--explain-threshold {name}={} conflicts with --explain-threshold \
                 {name}={}: preview one candidate limit per metric per run",
                MetricScalar(limit),
                MetricScalar(previous),
            ));
        }
    }
    // A `--threshold` override is absolute and never scaled, so it has no
    // soft tier — the exact gap this flag exists to close. Letting one sit
    // alongside a candidate for the same metric would make the reported
    // soft limit depend on which layer won, so reject the pairing.
    for (name, _) in canonical_cli_thresholds("--threshold", &args.thresholds) {
        if candidates.contains_key(&name) {
            die(format_args!(
                "--threshold {name}=… and --explain-threshold {name}=… name the same \
                 metric; a --threshold limit is absolute and has no soft tier to \
                 preview, so pass only --explain-threshold"
            ));
        }
    }
    candidates
}

/// The limit `set` resolved for `metric`, or `None` when it gates no such
/// metric.
fn resolved_limit(set: &ThresholdSet, metric: &str) -> Option<f64> {
    set.iter()
        .find_map(|(name, limit)| (name == metric).then_some(limit))
}

/// Render the preview to stdout. The report is this invocation's product
/// — per the `emit_check_results` stream contract that means stdout, so
/// `| rg`, `| head`, and `2>/dev/null` all reach it.
fn write_report(report: &[CandidateOutcome]) {
    // Said once, on stderr: the preview never gates, so a bare exit 0 must
    // not read as "this tree is clean at the candidate limit".
    eprintln!("bca: --explain-threshold is a preview; no gate ran and the exit code is always 0");
    let mut stdout = BufWriter::new(std::io::stdout().lock());
    let written = report
        .iter()
        .try_for_each(|outcome| write_outcome(&mut stdout, outcome))
        .and_then(|()| stdout.flush());
    die_unless_broken_pipe(written, "writing threshold preview");
}

fn write_outcome(out: &mut impl Write, outcome: &CandidateOutcome) -> std::io::Result<()> {
    let CandidateOutcome {
        metric,
        candidate,
        hard,
        soft,
        soft_derivation,
        ..
    } = outcome;
    writeln!(
        out,
        "{metric}: candidate limit {}",
        MetricScalar(*candidate)
    )?;
    writeln!(out, "  hard tier {}", tier_line(hard, None))?;
    writeln!(
        out,
        "  soft tier {}",
        tier_line(soft, Some(soft_derivation))
    )?;
    for over in &outcome.language_overrides {
        writeln!(
            out,
            "  note: [thresholds.lang.{}] keeps {metric} at {} (soft {}); its \
             files are counted against that, not the candidate",
            over.slug,
            MetricScalar(over.hard),
            MetricScalar(over.soft),
        )?;
    }
    match outcome.cluster() {
        Some((value, count)) => write_cluster(out, outcome, value, count),
        None => Ok(()),
    }
}

/// One tier's row. `derivation` names where a limit that was *derived*
/// came from; the hard tier passes `None`, because the candidate is the
/// number the user typed.
fn tier_line(tally: &TierTally, derivation: Option<&SoftDerivation>) -> String {
    let derivation = derivation.map_or_else(String::new, |d| format!(", {d}"));
    format!(
        "(limit {}{derivation}): {} offenders, {} already baselined, {} new",
        MetricScalar(tally.limit),
        tally.total,
        tally.baselined,
        tally.new_entries(),
    )
}

/// The finding the whole command exists for: a candidate limit that lands
/// on top of a population puts that population in the soft band by
/// construction, because the soft tier measures distance to the limit.
fn write_cluster(
    out: &mut impl Write,
    outcome: &CandidateOutcome,
    value: f64,
    count: usize,
) -> std::io::Result<()> {
    let band = outcome.band_values.len();
    let converged = if value.total_cmp(&outcome.candidate).is_eq() {
        " — the candidate limit itself"
    } else {
        ""
    };
    writeln!(
        out,
        "  cluster: {count} of {band} soft-band offenders sit at exactly {}{converged}. \
         The soft tier measures distance to the limit, so a limit of {} places them \
         inside the {} band by construction and none of them can clear it without \
         real work.",
        MetricScalar(value),
        MetricScalar(outcome.candidate),
        MetricScalar(outcome.soft.limit),
    )
}
