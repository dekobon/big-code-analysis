//! The `bca check` threshold-gate pipeline and its stage helpers.

use super::*;

mod effective_config;
mod explain;
mod footer;
mod outcome;
mod remediation;
mod skipped;
mod thresholds;

pub(crate) use {
    effective_config::*, explain::*, footer::*, outcome::*, remediation::*, skipped::*,
    thresholds::*,
};

#[cfg(test)]
#[path = "check_tests.rs"]
mod tests;

pub(crate) fn run_check(
    mut globals: GlobalOpts,
    mut args: CheckArgs,
    manifest: Option<&Manifest>,
    preproc: Option<Arc<PreprocResults>>,
) {
    // bca: suppress(abc)
    // Linear check-pipeline orchestration: each stage (threshold resolve,
    // walk, baseline filter, classify, emit) is already its own helper; the
    // ABC count is the call/assignment density of wiring them together, not
    // branching logic.
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
    // The `--strict` profile (or `[check] strict`, folded into
    // `args.strict` by the manifest merge above) is applied here —
    // before the `--explain-threshold` preview,
    // `--print-effective-config`, and the walk — so every downstream
    // consumer sees the flipped defaults.
    args.apply_strict(&mut globals);
    // Resolve the deprecated `--headroom` / `--strict-exit-codes` aliases
    // once (issues #688/#666). The manifest merge above has already
    // folded `[check] exit_codes` into `args.exit_codes` when the CLI
    // left it unset, so `resolved_exit_codes` reflects both sources with
    // the CLI value winning in either direction.
    let tier = args.resolved_tier();
    let tiered_exit_codes = args.resolved_exit_codes() == Some(crate::ExitCodes::Tiered);
    let layers = merge_threshold_layers(&args, base_thresholds);
    // `--explain-threshold` is a candidate-limit preview, not a gate
    // (#1169): it splices its own limits into the same merged layers,
    // walks once, and reports both tiers' cost. Branching here — after
    // the manifest merge, before the gate's own resolution — is what
    // makes the preview honour `exclude_tests`, `[check] exclude` and the
    // baseline exactly as the run it predicts.
    if !args.explain_thresholds.is_empty() {
        run_explain_thresholds(globals, &args, &layers, tier, preproc);
        return;
    }
    let ResolvedThresholds {
        thresholds,
        provenance,
    } = validate_and_build_thresholds(&mut args, layers, tier);
    // `--print-effective-config` is a read-only debug aid: print the
    // resolved configuration and exit 0 before the walk. clap already
    // rejects pairing with `--write-baseline` (conflicts_with), so by
    // the time we get here the flag is unambiguous.
    if let Some(format) = args.print_effective_config {
        print_effective_config(
            &globals,
            &args,
            &thresholds,
            manifest,
            format,
            tier,
            tiered_exit_codes,
        );
        return;
    }
    let CollectedViolations {
        violations,
        scope,
        // Kept for the remediation builder: `run_check_walk` consumes
        // `globals` by value (the walk it drives spawns worker threads
        // with ownership), but `format_remediation_block` needs the
        // resolved `--paths` / `--exclude` set to compose a
        // copy-paste-safe refresh command.
        globals: globals_for_remediation,
    } = collect_check_violations(globals, &args, preproc, thresholds);

    // `--write-baseline <path>` writes there; a bare `--write-baseline`
    // is resolved to the manifest `baseline` by `merge_check` (#496), so
    // a remaining `Some(None)` means no path was given and no manifest
    // `baseline` exists — a hard error rather than a silent no-write.
    if let Some(target) = args.write_baseline.as_ref() {
        let path = target.clone().unwrap_or_else(|| {
            die(
                "--write-baseline needs a path: pass one (`--write-baseline <file>`) \
                 or set a `baseline` key in bca.toml",
            )
        });
        write_check_baseline(violations, &path, provenance);
        return;
    }

    let pairs = classify_check_violations(
        violations,
        &args,
        scope.as_ref(),
        provenance,
        args.report_suppressed,
    );
    // Split the report-only suppressed debt — in-source markers
    // (`v.suppressed`) plus baseline-covered offenders
    // (`Coverage::Covered`), present only under `--report-suppressed` — from
    // the active offenders. Suppressed debt is surfaced in the code-scan
    // document but never reaches the gate: exit code, stderr stream, and
    // remediation are all driven by `active` alone. The default path leaves
    // `suppressed` empty, so behaviour is byte-for-byte unchanged.
    let (suppressed, active): (Vec<_>, Vec<_>) = pairs.into_iter().partition(|(v, coverage)| {
        v.suppressed || matches!(coverage, Some(Coverage::Covered { .. }))
    });
    let any_violations = !active.is_empty();
    // Categorise the active violations for the exit-code contract (#385)
    // before `emit_check_results` consumes them.
    let outcome = classify_check_outcome(&active, tier.tier());
    // Build the remediation block ONLY when we have something to
    // remediate. Empty active set (clean run) gets no trailing block —
    // there is no baseline to refresh and no artifact worth pointing
    // at. Suppressed debt is informational and never remediated here.
    let remediation = if any_violations {
        format_remediation_block(&globals_for_remediation, &args, tier)
    } else {
        None
    };
    emit_check_results(
        active,
        suppressed,
        &args,
        scope.as_ref(),
        remediation.as_deref(),
    );

    // `--no-fail` always forces exit 0; otherwise map the outcome to the
    // process exit code (tiered when `--strict-exit-codes` is set, the
    // stable 0/1/2 contract otherwise). A clean run returns `None` and
    // the process exits 0 implicitly.
    if !args.no_fail
        && let Some(code) = outcome.exit_code(tiered_exit_codes)
    {
        process::exit(code);
    }
}

/// What the walk half of the pipeline yields: the offenders that
/// survived `[check.exclude]`, the diff scope both callers need
/// afterwards, and the `GlobalOpts` clone `run_check_walk` did not
/// consume.
struct CollectedViolations {
    violations: Vec<Violation>,
    scope: Option<diff::DiffScope>,
    globals: GlobalOpts,
}

/// The gate's pre-baseline stages, in the order [`run_check`] applies
/// them: diff scope, walk, empty-input guard, `[check.exclude]`.
///
/// Shared with the `--explain-threshold` preview (#1169) so that the
/// preview cannot describe a different run from the one it predicts. A
/// stage added here reaches both; a stage added to one call site would
/// compile in the other and silently diverge, and no test would catch
/// it because each path would still pass its own assertions.
///
/// The `[check.exclude]` drop (#378) happens before *any* downstream
/// consumer sees the offenders — so `--write-baseline` never records
/// the structural exemptions and the gate never fails on them. It runs
/// after the empty-input guard: exempt files are still walked and
/// counted, only their violations are dropped.
fn collect_check_violations(
    globals: GlobalOpts,
    args: &CheckArgs,
    preproc: Option<Arc<PreprocResults>>,
    thresholds: Arc<LanguageThresholds>,
) -> CollectedViolations {
    let scope = resolve_diff_scope(args);
    // Cloned *before* the walk materializes `--paths-from` into
    // `globals.paths`, so the remediation footer keeps printing the
    // spelling the caller typed (`--paths-from -`) rather than the
    // expanded seed list (#1306).
    let globals_kept = globals.clone();
    let walk = run_check_walk(globals, args, preproc, thresholds);
    // Before the empty-input guard, so a gate whose only input was
    // ignore-dropped still says what happened above the exit-1 error.
    report_unchecked_files(&walk, globals_kept.report_skipped);
    enforce_usable_input(&walk);
    let violations = apply_check_exclude(walk.violations, args, &walk.seeds);
    CollectedViolations {
        violations,
        scope,
        globals: globals_kept,
    }
}

/// The gate's post-exclude stages, in the order [`run_check`] applies
/// them: baseline classification, then `--changed-only` scoping. The
/// companion to [`collect_check_violations`], and shared with the
/// preview for the same reason.
///
/// It also owns the three `CheckArgs` defaults that feed
/// [`filter_by_baseline`], which is what stops the preview and the gate
/// resolving the same `--baseline` differently.
///
/// `--changed-only` filters ALL offenders before the caller splits
/// them, so the suppressed set surfaced in the report respects the
/// touched-file scope exactly as the active set does — otherwise
/// `--changed-only --report-suppressed` would leak suppressed debt from
/// files outside the diff. With `--report-suppressed` off this is the
/// original pre-feature ordering (filter, then everything is active).
fn classify_check_violations(
    violations: Vec<Violation>,
    args: &CheckArgs,
    scope: Option<&diff::DiffScope>,
    provenance: baseline::Provenance,
    keep_covered: bool,
) -> Vec<(Violation, Option<Coverage>)> {
    let pairs = filter_by_baseline(
        violations,
        args.baseline.as_deref(),
        args.baseline_line_tolerance
            .unwrap_or(baseline::DEFAULT_LINE_TOLERANCE),
        args.baseline_fuzzy_match.unwrap_or(false),
        provenance,
        keep_covered,
    );
    apply_changed_only(pairs, scope, args.changed_only)
}

/// What the check walk produced: the sorted violations plus the
/// post-walk tally `run_check` consults before it trusts the gate
/// verdict.
pub(crate) struct CheckWalk {
    violations: Vec<Violation>,
    /// The seed list the walk anchored against: `--paths` with any
    /// `--paths-from` entries already materialized in.
    ///
    /// Carried out of the walk rather than re-derived, because
    /// `--paths-from -` reads stdin and stdin can only be read once
    /// (#1306): a second read yields an empty list, and every violation
    /// then anchors against `--paths` alone — silently defeating a
    /// `[check.exclude]` glob.
    seeds: Vec<PathBuf>,
    /// Files whose contents were read and handed to the pre-dispatch
    /// filters. Zero means nothing survived `--paths` expansion plus
    /// `--include` / `--exclude` filtering.
    files_dispatched: usize,
    /// Files the generated-code detector dropped before parsing.
    /// Counted (and reported by default) because a `@generated` marker
    /// in the branch under test otherwise removes a file from the gate
    /// with nothing on stderr (#1055's bypass A).
    generated_skipped: usize,
    /// What VCS ignore rules dropped from the walk, measured at the
    /// walk's prune points because the walker itself never yields
    /// ignored entries; a `.gitignore` committed in the branch under
    /// test otherwise shrinks the checked set silently (#1055's
    /// bypass B).
    ignored: crate::IgnoredEntries,
}

/// Fail the run when the walk saw no input files at all. A tool error
/// (exit 1) rather than a gate result (exit 2), because a gate that
/// analysed nothing has no verdict to report; it fires before the gate
/// is evaluated, so it is not suppressed by `--no-fail` (which
/// suppresses threshold failures, not broken input) and does not let
/// `--write-baseline` record an empty run. Mirrors
/// `enforce_explicit_unrecognized` on the analyze side.
///
/// The companion guard — any file that could not be *read* (#1060) —
/// now lives in the shared walk layer (`enforce_readable_input`), so it
/// fires for every subcommand and still runs first, naming the real
/// cause instead of blaming the path filters (#1098).
///
/// Like its companion, the message names no subcommand: `bca init`
/// scaffolds its baseline through `run_check`
/// (`commands::init::scaffold_baseline`), so naming one here
/// misattributes the failure to a command the user never ran. `die`
/// already prefixes `error:`.
fn enforce_usable_input(walk: &CheckWalk) {
    if walk.files_dispatched == 0 {
        // No files survived `--paths` expansion + `--include`/`--exclude`
        // filtering. Treat this as a tool error (exit 1), not a clean
        // pass (exit 0): a typo in `--paths` would otherwise silently
        // green-light CI.
        die("no input files matched; check --paths, --include, --exclude");
    }
}

/// Run the parallel walker with a check-flavoured `Config`, collect
/// every emitted `Violation`, and sort them by `(path, start_line,
/// metric)` so CI diff tooling sees identical output across runs over
/// the same tree.
///
/// Also the one place `--paths-from` is resolved for the gate: the
/// materialized seed list leaves in [`CheckWalk::seeds`] so no later
/// stage has to read that source a second time (#1306).
fn run_check_walk(
    mut globals: GlobalOpts,
    args: &CheckArgs,
    preproc: Option<Arc<PreprocResults>>,
    thresholds: Arc<LanguageThresholds>,
) -> CheckWalk {
    // Materialize `--paths-from` into the seed list *once*, here, and
    // hand the result to both consumers: the walk below and
    // `apply_check_exclude`'s re-anchoring. `expand_seed_paths` would
    // otherwise do this read itself, so nothing extra is read — the
    // list is merely retained (#1306).
    if let Some(src) = globals.paths_from.take() {
        globals
            .paths
            .extend(crate::read_paths_from(&src).unwrap_or_else(|e| die(e)));
    }
    // One clone of the seed list, on a path that already clones the
    // whole `GlobalOpts` for the remediation footer. The allocation the
    // old `--paths-from` re-read avoided was never this one: it was the
    // glob-set build and the `--check-exclude-from` read, and
    // `CheckExcludes::resolve` still short-circuits before both on the
    // common no-exclude run.
    let seeds = globals.paths.clone();
    let (tx, rx) = crossbeam::channel::unbounded();
    let files_dispatched = Arc::new(AtomicUsize::new(0));
    let generated_skipped = Arc::new(AtomicUsize::new(0));
    // Compute only the metric families the resolved thresholds read
    // (#1113). A gate on one metric used to pay for the whole suite —
    // Halstead being the most expensive per node — and throw the rest
    // away. `validate_and_build_thresholds` rejects an empty set before
    // the walk, so this is never an empty selection, which `with_only`
    // would read as "compute nothing". The selection is the union across
    // languages (#1141): a metric only a `[thresholds.lang.<slug>]` table
    // gates would otherwise stay at its zero default.
    let selected_metrics = Some(thresholds.selected_metrics());
    let mut cfg = Config {
        thresholds: Some(thresholds),
        selected_metrics,
        check_tx: Some(tx),
        files_dispatched: Some(Arc::clone(&files_dispatched)),
        generated_skipped: Some(Arc::clone(&generated_skipped)),
        suppression_policy: SuppressionPolicy::from_no_suppress(args.no_suppress),
        report_suppressed: args.report_suppressed,
        // Compute body hashes during the walk only when fuzzy matching
        // is requested — whether for a `--baseline` read or to populate
        // `body_hash` in a `--write-baseline` write.
        fuzzy_baseline: args.baseline_fuzzy_match.unwrap_or(false),
        ..Config::new(Action::Check, &globals, preproc)
    };
    // Expand the seeds here rather than through `run_walk`, so the gate
    // can ask for the ignore-rule measurement the summary reports — no
    // other command pays for it. `run_walk_resolved` then carries the
    // same exit-1 incomplete-walk contract as `run_walk`.
    let (resolved, num_jobs) = crate::resolve_walk_files_with_ignored(globals);
    let ignored = resolved.ignored;
    cfg.explicit_seeds = Arc::new(resolved.explicit_files);
    crate::run_walk_resolved(resolved.files, num_jobs, cfg, resolved.walk_errors);

    // Workers have all joined by the time the walk returns, so the
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

    CheckWalk {
        violations,
        seeds,
        files_dispatched: files_dispatched.load(Ordering::Relaxed),
        generated_skipped: generated_skipped.load(Ordering::Relaxed),
        ignored,
    }
}

/// Serialize and write the collected violations as a baseline TOML
/// file. Used by the `--write-baseline` early-exit branch. The
/// baseline-file directory becomes the *anchor* — every entry's path
/// is keyed relative to it, so a subsequent `--baseline` invocation
/// from any `--paths` form (`.`, `src/`, `$PWD`) still matches.
pub(crate) fn write_check_baseline(
    violations: Vec<Violation>,
    path: &Path,
    provenance: baseline::Provenance,
) {
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

/// The gate-exemption deny-sets, one per glob origin, mirroring
/// [`crate::walk::WalkFilters`]'s split of the same two surfaces.
///
/// Two sets rather than one merged list because the two origins anchor
/// differently (#1164): a `--check-exclude` glob was typed in the
/// caller's shell, so the working directory is its root, while a
/// `[check] exclude` glob was written against the project the
/// `bca.toml` sits at the root of. One list can carry only one anchor,
/// and merging them silently gave the manifest half the caller's.
///
/// The matching rule itself lives in
/// [`crate::walk_seed::AnchoredExcludes`], which the walker's filters
/// also use. It did not always: this type anchored the manifest set
/// always and `WalkFilters::passes` never did, on the reasoning that a
/// directory walk's root and the manifest's coincide — true only for
/// the canonical `paths = ["."]`, and the source of #1189. The two now
/// share one implementation and cannot drift again.
struct CheckExcludes<'a> {
    cli: crate::ExcludeGlobs,
    manifest: crate::ExcludeGlobs,
    /// Directory holding the `bca.toml` that supplied `manifest`;
    /// `None` when no manifest configured any exemption.
    manifest_dir: Option<&'a Path>,
    /// The working directory, resolved once here rather than per
    /// violation. A relative violation path is completed against it to
    /// decide whether the file lies under the manifest, and `exempts`
    /// runs for every violation — so reading it there cost a syscall
    /// per offender on exactly the runs #1164 exists to serve.
    ///
    /// Owned rather than borrowed because `AnchoredExcludes` borrows it,
    /// and a self-referential struct is not worth the alternative here;
    /// `exempts` rebuilds the borrowed view per call, which is three
    /// pointer copies.
    cwd: std::path::PathBuf,
    /// `manifest_dir` resolved through symlinks, once per run — see
    /// [`crate::walk_seed::ManifestAnchor`]'s `canonical_root`. `exempts`
    /// runs per violation, so this must not be recomputed there.
    canonical_manifest_dir: Option<std::path::PathBuf>,
}

impl<'a> CheckExcludes<'a> {
    /// Compile the two deny-sets from the resolved gate config, or
    /// `None` when nothing is configured — the common case, which then
    /// skips the glob-set build and the `--check-exclude-from` /
    /// `[check] exclude_from` file reads entirely.
    fn resolve(args: &'a CheckArgs) -> Option<Self> {
        let manifest = args
            .manifest_check_exclude
            .as_ref()
            .filter(|m| !m.is_empty());
        if args.check_exclude.is_empty() && args.check_exclude_from.is_none() && manifest.is_none()
        {
            return None;
        }
        Some(Self {
            cli: crate::build_exclude_globset(
                args.check_exclude.clone(),
                args.check_exclude_from.as_deref(),
                "--check-exclude-from",
            ),
            manifest: crate::build_exclude_globset(
                manifest.map(|m| m.globs.clone()).unwrap_or_default(),
                manifest.and_then(|m| m.globs_from.as_deref()),
                "bca.toml [check] exclude_from",
            ),
            manifest_dir: manifest.map(|m| m.dir.as_path()),
            canonical_manifest_dir: manifest.and_then(|m| m.dir.canonicalize().ok()),
            cwd: std::env::current_dir().unwrap_or_default(),
        })
    }

    /// Whether a violation at `path` — `walk_form` being its
    /// walk-root-anchored spelling — is exempt from the threshold gate.
    ///
    /// Delegates to the shared rule, so the gate and the walker cannot
    /// disagree about which files a manifest glob describes.
    fn exempts(&self, path: &Path, walk_form: crate::walk_seed::CwdForm<'_>) -> bool {
        crate::walk_seed::AnchoredExcludes::new(
            &self.cli,
            &self.manifest,
            self.manifest_dir,
            &self.cwd,
            self.canonical_manifest_dir.as_deref(),
        )
        .excludes(path, walk_form)
    }
}

/// Drop every violation a `[check.exclude]` / `--check-exclude` glob
/// exempts, matching each against the walk-root-anchored form of its
/// path.
///
/// `seeds` must be the full seed list the walk anchored against —
/// `--paths` *with* any `--paths-from` entries materialized in, which
/// is what [`CheckWalk::seeds`] carries.
pub(crate) fn apply_check_exclude(
    violations: Vec<Violation>,
    args: &CheckArgs,
    seeds: &[PathBuf],
) -> Vec<Violation> {
    // Nothing configured is the common case, and it skips the glob-set
    // build and the `--check-exclude-from` read entirely.
    let Some(excludes) = CheckExcludes::resolve(args) else {
        return violations;
    };
    // Anchor each violation's emitted path to the walk-root `./`-form
    // before matching, mirroring the global `--exclude`/`--include`
    // anchoring (#489), so a `./`-anchored `[check.exclude]` pattern
    // exempts the same files regardless of how the seed resolved
    // (absolute, `$PWD`, or a manifest root above the CWD).
    //
    // The seeds must be the *full* set the walk anchored against —
    // `--paths` AND any `--paths-from` entries — reanchored exactly as
    // [`expand_seed_paths`] did. Threading only `--paths` left a
    // violation from a `--paths-from`-sourced (e.g. absolute) seed
    // matched unanchored, so a `[check.exclude]` glob silently failed to
    // exempt it (#497). The list now arrives materialized from the walk
    // (`CheckWalk::seeds`) rather than being rebuilt by a second
    // `--paths-from` read, which returned nothing for `--paths-from -`
    // because the walk had already consumed stdin (#1306). It is the
    // walk's *input* list, so it still needs the same `reanchor_seed`
    // pass `expand_seed_paths` applies.
    let seeds: Vec<PathBuf> = seeds
        .iter()
        .cloned()
        .map(crate::walk_seed::reanchor_seed)
        .collect();
    let before = violations.len();
    let kept: Vec<Violation> = violations
        .into_iter()
        .filter(|v| {
            // Anchor to the `./`-relative walk-root form, then strip the
            // leading `./` so bare-relative `[check.exclude]` patterns match
            // it just like `./`-prefixed ones (#726).
            let anchored = crate::walk_seed::anchor_against_seeds(&seeds, &v.path);
            !excludes.exempts(
                &v.path,
                crate::walk_seed::CwdForm(crate::walk_seed::strip_cur_dir(&anchored)),
            )
        })
        .collect();
    let skipped = before - kept.len();
    if skipped > 0 {
        eprintln!("bca: skipped {skipped} violations via [check.exclude]");
    }
    kept
}

/// Compose the stderr warning (issue #486) when the current run is
/// stricter than the baseline was written against, or `None` when the
/// comparison is safe (see [`baseline::check_provenance`] for the
/// directional rule). Split out from [`filter_by_baseline`] so a test
/// can pin the exact message and the silent cases without a baseline
/// file on disk.
pub(crate) fn provenance_warning(
    current: baseline::Provenance,
    baseline: Option<baseline::Provenance>,
) -> Option<String> {
    match baseline::check_provenance(current, baseline) {
        baseline::ProvenanceCheck::Ok => None,
        baseline::ProvenanceCheck::StricterThanBaseline {
            current: cur,
            baseline: base,
        } => Some(format!(
            "this check's effective limits (strictness {cur}) are \
             stricter than the baseline was written against (strictness \
             {base}); the baseline may under-cover and the gate can fire on \
             untouched files. Refresh it at the matching tier, e.g. \
             `bca check --tier=soft={cur} --write-baseline \
             <file>` (or `--write-baseline <file>` for the hard tier)."
        )),
    }
}

/// Classify each violation against the optional `--baseline` file.
/// The kept list carries `(Violation, Option<Coverage>)` so the
/// stderr renderer can attach a `[new]` / `[regr +N%]` tag. Without
/// `--baseline`, `Option<Coverage>` is `None` and the renderer emits
/// the exact pre-tag line format byte-identically.
pub(crate) fn filter_by_baseline(
    violations: Vec<Violation>,
    baseline_path: Option<&Path>,
    tolerance: usize,
    fuzzy: bool,
    provenance: baseline::Provenance,
    keep_covered: bool,
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
        warn(msg);
    }
    let before = violations.len();
    let kept: Vec<_> = violations
        .into_iter()
        .filter_map(|v| match baseline.classify(&v) {
            // `--report-suppressed` keeps baseline-covered offenders (tagged
            // `Covered`) so they can be surfaced as `external` suppressions
            // in the document; the split in `run_check` keeps them out of the
            // gate. The default path still drops them entirely.
            Coverage::Covered { .. } if !keep_covered => None,
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
pub(crate) fn resolve_diff_scope(args: &CheckArgs) -> Option<diff::DiffScope> {
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

pub(crate) fn apply_changed_only(
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
pub(crate) struct ChangedOnlyOutcome {
    pub(crate) kept: Vec<(Violation, Option<Coverage>)>,
    pub(crate) diagnostic: Option<String>,
}

pub(crate) fn apply_changed_only_inner(
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

/// Render one `path:lines: name: metric = N (limit M)` row per pair, in
/// the order the caller sorted them, then flush.
///
/// Split out from [`emit_check_results`] so the same loop serves both
/// destinations the stream contract there admits. The flush is part of
/// the helper for the same reason [`write_parts_flushed`] carries one:
/// a buffered writer that reports every `write_all` as `Ok` can still
/// fail at flush time, and on the stdout path that error decides between
/// `die` and a silent truncated report.
fn write_violation_rows_flushed(
    out: &mut impl Write,
    pairs: &[(Violation, Option<Coverage>)],
) -> std::io::Result<()> {
    pairs
        .iter()
        .try_for_each(|(v, tag)| writeln!(out, "{}", render_violation_line(v, tag.as_ref())))?;
    out.flush()
}

pub(crate) fn emit_check_results(
    pairs: Vec<(Violation, Option<Coverage>)>,
    suppressed: Vec<(Violation, Option<Coverage>)>,
    args: &CheckArgs,
    scope: Option<&diff::DiffScope>,
    remediation: Option<&str>,
) {
    // Stream contract (#1167) — do not move a line across it without
    // reading the issue first.
    //
    // **stdout** carries the offender rows, and nothing else. They are
    // this command's product, so the obvious ways to work with a list —
    // `| wc -l`, `| head`, `| rg -c`, `2>/dev/null` — must reach them.
    // They used to go to stderr, where all four silently reported an
    // empty offender set: a *plausible* "this tree is clean" rather than
    // an error.
    //
    // **stderr** carries everything that is commentary about the run:
    // the per-file summary footer, the GitHub Actions annotations, the
    // remediation block, and the `bca: …` / `warning:` / `error:`
    // diagnostics the upstream stages emit (`skipped N violations via
    // [check.exclude]`, `filtered N violations via baseline`, …).
    //
    // The one exception: `--report-format` without `--output` puts the
    // aggregated SARIF / Checkstyle / Code Climate document on stdout,
    // so the rows stay on stderr for that combination instead of
    // corrupting a machine-readable payload. `--output <file>` moves the
    // document off stdout and the rows go back to it.
    let document_owns_stdout = args.output_format.is_some() && args.output.is_none();

    if !document_owns_stdout {
        // Written and flushed before the first stderr write below, so a
        // terminal (or a `2>&1`-merged CI log) still shows the rows
        // above the footer that summarizes them.
        //
        // `BrokenPipe` is exempt — `bca check | head` is routine and
        // must still exit on the gate verdict, matching every other
        // subcommand's stdout policy (`write_stdout_or_die`, #1132).
        // Any other write failure is a real tool error: reporting a gate
        // verdict whose evidence never reached the consumer is the
        // silent-success shape this whole contract exists to avoid.
        let mut stdout = BufWriter::new(std::io::stdout().lock());
        let written = write_violation_rows_flushed(&mut stdout, &pairs);
        die_unless_broken_pipe(written, "writing check offenders");
    }

    // BrokenPipe on stderr (e.g. when piped to `head`) is the only
    // realistic write failure here; swallow it rather than die so the
    // exit-code contract is honored.
    //
    // `stderr` is unbuffered — one `write(2)` per line — so the lock is
    // wrapped in a `BufWriter`. What makes that safe is the
    // explicit `drop` at the end of this function, and it is about
    // *ordering*, not about `process::exit`: `BufWriter::drop` does
    // flush (it only discards the error), and this buffer is dropped
    // here, long before `run_check` ever reaches `process::exit`.
    // Deleting that drop lets the `eprintln!` diagnostic and the stdout
    // document that follow overtake the summary footer — which is what
    // `check_stderr_block_is_flushed_before_later_stderr_writes` catches.
    //
    // The one thing buffering does cost: a panic between the writes
    // below and that drop loses the entire report, because
    // `BufWriter::drop` skips the flush when `self.panicked`.
    let mut stderr = BufWriter::new(std::io::stderr().lock());
    if document_owns_stdout {
        let _ = write_violation_rows_flushed(&mut stderr, &pairs);
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
    // Drop before anything else writes: the step-summary diagnostic
    // below goes to `stderr` through `eprintln!` and the aggregated
    // document to stdout, and both must land *after* the violation
    // report a reader is scanning for. `BufWriter::drop` flushes.
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

    emit_aggregated_document(pairs, suppressed, args);
}

/// Emit the aggregated CI/IDE document (`--report-format`, or a dialect
/// inferred from `--output`) — the machine-readable counterpart to the
/// human rows [`emit_check_results`] writes, and the other half of that
/// function's stream contract: it lands in `--output <file>` when given
/// and on stdout otherwise.
///
/// A no-op when neither flag is in effect. Empty input still produces a
/// well-formed but offender-free document, which CI consumers can ingest
/// unchanged on clean runs, and a successful write never perturbs the
/// exit-code contract.
fn emit_aggregated_document(
    pairs: Vec<(Violation, Option<Coverage>)>,
    suppressed: Vec<(Violation, Option<Coverage>)>,
    args: &CheckArgs,
) {
    let Some(fmt) = args.output_format else {
        return;
    };
    let offenders: Vec<_> = pairs
        .into_iter()
        .map(|(v, _)| violation_to_offender(v))
        .collect();
    // Only the SARIF format can represent suppression, so route active +
    // suppressed offenders through the suppression-aware writer there. For
    // every other format (and the default no-suppressed case) fall back to
    // the plain dump so output is byte-for-byte unchanged.
    let written = if !suppressed.is_empty() && matches!(fmt, check_format::AggregatedFormat::Sarif)
    {
        check_format::dump_sarif_with_suppressed(&offenders, suppressed, args.output.as_deref())
    } else {
        fmt.dump(&offenders, args.output.as_deref())
    };
    written.unwrap_or_else(|e| die(format_args!("failed to write {}: {e}", fmt.name())));
}

/// Decide whether GitHub Actions `::error` annotations should be
/// emitted (issue #683). The tri-state `--github-annotations
/// <auto|always|never>` flag resolves like `--color`: `always` forces
/// them on, `never` suppresses them even inside a workflow step, and
/// `auto` (the default) falls back to `$GITHUB_ACTIONS == "true"`, the
/// signal GHA sets inside every step.
pub(crate) fn github_annotations_enabled(args: &CheckArgs) -> bool {
    let in_gha = std::env::var(check_format::GITHUB_ACTIONS_ENV).as_deref() == Ok("true");
    args.github_annotations.enabled_with(in_gha)
}

/// Resolve the path to append the step-summary digest to (issue #683).
/// `--summary-file <path>` appends there unconditionally; `--summary-file
/// never` suppresses the digest even inside a GHA step; `auto` (the
/// default when the flag is omitted) defers to `$GITHUB_STEP_SUMMARY`.
/// Returns `None` when no path is in effect and the digest is skipped.
pub(crate) fn step_summary_path(args: &CheckArgs) -> Option<PathBuf> {
    match &args.summary_file {
        Some(SummaryFile::Path(p)) => Some(p.clone()),
        Some(SummaryFile::Never) => None,
        // `auto` (explicit) and the unset default both detect the env var.
        Some(SummaryFile::Auto) | None => {
            std::env::var_os(check_format::GITHUB_STEP_SUMMARY_ENV).map(PathBuf::from)
        }
    }
}
