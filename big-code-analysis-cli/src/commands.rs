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

use clap::Parser;

use big_code_analysis::{Count, PreprocResults, SuppressionPolicy};
use big_code_analysis::{fix_includes, write_file};

use crate::baseline::{self, Coverage};
use crate::check_format::violation_to_offender;
use crate::diff;
use crate::format_util::MetricScalar;
use crate::formats::{CBOR_STDOUT_ERROR, MetricsDispatch, MetricsFormat, ReportFormat};
use crate::html_report::generate_html_report;
use crate::markdown_report::{FunctionSummary, generate_report};
use crate::metric_catalog::write_metrics;
use crate::thresholds::{ThresholdSet, Violation, render_violation_line};
use crate::{
    Action, CheckArgs, Cli, Command, Config, GlobalOpts, ListMetricsArgs, NodesArgs, PreprocArgs,
    ReportArgs, StripCommentsArgs, StructuredArgs, die, die_io, legacy_hint, load_baseline,
    load_preproc_data, load_threshold_config, run_walk, write_atomic, write_stdout_or_die,
};

/// Drive the `check` subcommand as a six-stage pipeline:
/// validate args + build thresholds → resolve diff scope (if any) →
/// walk + sort → maybe write a baseline → filter against a loaded
/// baseline → emit results and exit. Each stage is its own helper
/// so the control flow reads top-down without nested decisions about
/// which arm fires when.
fn run_check(globals: GlobalOpts, args: CheckArgs, preproc: Option<Arc<PreprocResults>>) {
    let set = validate_and_build_thresholds(&args);
    let scope = resolve_diff_scope(&args);
    let (violations, files_dispatched) = run_check_walk(globals, &args, preproc, set);

    if files_dispatched.load(Ordering::Relaxed) == 0 {
        // No files survived `--paths` expansion + `--include`/`--exclude`
        // filtering. Treat this as a tool error (exit 1), not a clean
        // pass (exit 0): a typo in `--paths` would otherwise silently
        // green-light CI.
        die("bca check: no input files matched; check --paths, --include, --exclude");
    }

    if let Some(path) = args.write_baseline.as_deref() {
        write_check_baseline(violations, path);
        return;
    }

    let pairs = filter_by_baseline(violations, args.baseline.as_deref());
    let pairs = apply_changed_only(pairs, scope.as_ref(), args.changed_only);
    let any_violations = emit_check_results(pairs, &args, scope.as_ref());

    if any_violations && !args.no_fail {
        process::exit(2);
    }
}

/// Validate `--output` / `--output-format` pairing, merge the
/// `--config` and `--threshold` flag inputs, and build the
/// `ThresholdSet`. Dies if no thresholds were configured. Returns
/// the set wrapped in `Arc` so it can be cloned into each walker
/// worker's `Config`.
fn validate_and_build_thresholds(args: &CheckArgs) -> Arc<ThresholdSet> {
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

    let mut merged: BTreeMap<String, f64> = args
        .config
        .as_deref()
        .map(load_threshold_config)
        .unwrap_or_default();
    // CLI flags override config values for the same metric name.
    for (name, limit) in &args.thresholds {
        merged.insert(name.clone(), *limit);
    }
    let set = ThresholdSet::build(&merged).unwrap_or_else(|e| die(e));
    if set.is_empty() {
        die("no thresholds configured; pass --threshold or --config");
    }
    Arc::new(set)
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
/// file. Used by the `--write-baseline` early-exit branch.
fn write_check_baseline(violations: Vec<Violation>, path: &Path) {
    let file = baseline::from_violations(violations);
    let entry_count = file.entries.len();
    let text =
        baseline::render(&file).unwrap_or_else(|e| die(format_args!("serialize baseline: {e}")));
    write_atomic(path, text.as_bytes()).unwrap_or_else(|e| die_io("write baseline", path, e));
    eprintln!(
        "bca: wrote {entry_count} baseline entries to {}",
        path.display()
    );
}

/// Classify each violation against the optional `--baseline` file.
/// The kept list carries `(Violation, Option<Coverage>)` so the
/// stderr renderer can attach a `[new]` / `[regr +N%]` tag. Without
/// `--baseline`, `Option<Coverage>` is `None` and the renderer emits
/// the exact pre-tag line format byte-identically.
fn filter_by_baseline(
    violations: Vec<Violation>,
    baseline_path: Option<&Path>,
) -> Vec<(Violation, Option<Coverage>)> {
    let Some(path) = baseline_path else {
        return violations.into_iter().map(|v| (v, None)).collect();
    };
    let baseline = load_baseline(path);
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
    if !changed_only {
        return pairs;
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
        eprintln!(
            "bca: --changed-only requested but no diff scope is available; \
             skipping filter (would-be programmer error — \
             resolve_diff_scope should have fatal-errored)"
        );
        return pairs;
    };
    if scope.changed.is_empty() {
        // A resolved-but-empty scope (e.g. running `--since main` from
        // a branch that has been merged/squashed into main locally, or
        // a force-pushed branch where the diff base now equals HEAD)
        // would otherwise silently drop every violation and exit 0,
        // which is exactly the "silent green-light" failure mode #359
        // was meant to prevent. Surface it explicitly so CI logs make
        // it obvious the gate ran but had nothing to check.
        eprintln!(
            "bca: --changed-only: diff scope is empty (no files touched between {} and HEAD); \
             dropping {} violations and exiting clean",
            scope.base,
            pairs.len()
        );
        return Vec::new();
    }
    // Canonicalize once per unique raw path rather than once per
    // violation. Real-world inputs cluster heavily per file
    // (a 50-violation run typically touches 5-10 files), so memoizing
    // the `Path::canonicalize` syscall here turns O(violations)
    // realpath(2) calls into O(unique files).
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
    if dropped > 0 {
        eprintln!("bca: --changed-only dropped {dropped} violations from files outside diff scope");
    }
    kept
}

/// Render the (optionally tagged) violations to stderr and, if
/// `--output-format` is set, also emit the aggregated CI/IDE
/// document. `scope` is threaded into the summary footer so the
/// per-file rollup can split "touched in this range" rows from the
/// legacy offender list. Returns `true` iff any violations were
/// emitted, so the caller can decide the exit code without
/// re-checking the pairs.
fn emit_check_results(
    pairs: Vec<(Violation, Option<Coverage>)>,
    args: &CheckArgs,
    scope: Option<&diff::DiffScope>,
) -> bool {
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
    drop(stderr);

    // Emit the aggregated CI/IDE document if requested. Empty input
    // produces a well-formed but offender-free document, which CI
    // consumers can ingest unchanged on clean runs. The exit-code
    // contract is unaffected by this branch.
    let any_violations = !pairs.is_empty();
    if let Some(fmt) = args.output_format {
        let offenders: Vec<_> = pairs
            .into_iter()
            .map(|(v, _)| violation_to_offender(v))
            .collect();
        fmt.dump(&offenders, args.output.as_deref())
            .unwrap_or_else(|e| die(format_args!("failed to write {}: {e}", fmt.name())));
    }
    any_violations
}

/// One row in the per-file rollup footer. The display string is
/// cached at construction time so the sort comparator does not
/// recompute `path.display().to_string()` on every call (O(n²) on
/// the path length otherwise).
struct FooterRow<'a> {
    count: usize,
    worst: &'a Violation,
    display: String,
    path: &'a PathBuf,
}

/// Group `pairs` by raw `PathBuf` (preserving non-UTF-8 byte
/// identity), pick the worst-ratio violation per file, and sort the
/// resulting rows by violation count descending then path ascending.
fn compute_footer_rows<'a>(pairs: &'a [(Violation, Option<Coverage>)]) -> Vec<FooterRow<'a>> {
    // Group by raw PathBuf rather than `path.display()` to preserve
    // non-UTF-8 byte identity. Two paths that differ only in invalid
    // UTF-8 (`b"foo\xff.rs"` vs `b"foo\xfe.rs"`) would collapse to
    // the same lossy display string but stay distinct here.
    // `path.display()` is still used to *render* the footer row so
    // it matches the per-violation stderr line format above.
    let mut by_path: BTreeMap<&PathBuf, Vec<&Violation>> = BTreeMap::new();
    for (v, _) in pairs {
        by_path.entry(&v.path).or_default().push(v);
    }
    let mut rows: Vec<FooterRow<'a>> = by_path
        .iter()
        .filter_map(|(path, vs)| {
            let worst = pick_worst(vs)?;
            Some(FooterRow {
                count: vs.len(),
                worst,
                display: path.display().to_string(),
                path,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.display.cmp(&b.display))
    });
    rows
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
    let rows = compute_footer_rows(pairs);
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

/// Pick the worst violation in a slice by `value / limit` ratio. Ties
/// break by larger absolute value, then by metric name ascending.
/// Limits of `0.0` saturate to `f64::INFINITY` so a "no value
/// permitted" threshold dominates ratio comparison without triggering
/// NaN from `value / 0.0`.
///
/// Returns `None` only if the slice is empty. In `write_summary_footer`
/// the slice can't be empty in practice — `BTreeMap::entry(...).or_default()
/// .push(v)` guarantees at least one element per key — but expressing
/// "non-empty slice" in the type system isn't worth the ceremony, so
/// the caller propagates the `None` via `?` instead.
fn pick_worst<'a>(vs: &[&'a Violation]) -> Option<&'a Violation> {
    vs.iter().copied().max_by(|a, b| {
        ratio(a)
            .total_cmp(&ratio(b))
            .then_with(|| a.value.total_cmp(&b.value))
            .then_with(|| b.metric.cmp(a.metric))
    })
}

fn ratio(v: &Violation) -> f64 {
    if v.limit > 0.0 {
        v.value / v.limit
    } else {
        f64::INFINITY
    }
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
///   data, missing `--output` parent directory, walk errors, mutually
///   exclusive output-format combinations, broken-pipe writes, etc.)
///   call `process::exit(1)` via internal `die` / `die_io` helpers.
/// - The `check` subcommand calls `process::exit(2)` when any
///   threshold is exceeded, reserving exit 1 for tool errors so CI can
///   distinguish "metric regression" from "tool crashed".
///
/// Hosts that call [`run`] will be torn down on any of those paths
/// without unwinding. If you need to drive the same functionality from
/// inside another process, use the [`big_code_analysis`] library crate
/// directly instead of going through this entry point.
pub fn run() {
    let cli = parse_cli_with_legacy_hint();

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
        Command::Check(args) => run_check(cli.globals, args, preproc),
        Command::Preproc(args) => run_command_preproc(cli.globals, args),
    }
}

/// Parse the CLI from `std::env::args_os`, emitting a legacy-CLI
/// migration hint to stderr when the failure looks like it came from
/// the pre-restructure flag shape (`-d` instead of `dump`, `-O
/// markdown` instead of `report markdown`, etc.). Exits the process
/// on parse failure via `clap::Error::exit`.
fn parse_cli_with_legacy_hint() -> Cli {
    match Cli::try_parse() {
        Ok(cli) => cli,
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

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
