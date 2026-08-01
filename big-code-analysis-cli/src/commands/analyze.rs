//! The structured-output subcommands: `metrics` and `ops`.

use super::*;

/// Resolved structured-output destination for `metrics` / `ops` (#669):
/// stdout, a single aggregate file (`--output <FILE>`), or a per-file
/// directory tree (`--output-dir <DIR>`).
pub(crate) enum StructuredOutput {
    Stdout,
    AggregateFile(PathBuf),
    Dir(PathBuf),
}

/// Validate the `--output` / `--output-dir` / `--format` combination for
/// `metrics` / `ops` and resolve the destination (#669).
///
/// `--output <FILE>` now means a single aggregate file everywhere;
/// `--output-dir <DIR>` carries the per-file-tree mode `--output` used to
/// imply. The guards enforced here:
///
/// - `--output` and `--output-dir` together → error (one destination
///   only).
/// - Either destination without a structured `--format` → error (#661):
///   the default `text` format streams a human-readable tree to stdout
///   and writes no files, so a destination under it would silently no-op.
/// - CBOR with no destination → error (binary cannot go to stdout).
/// - `--output` must be a writable file path (parent exists, not an
///   existing directory); `--output-dir` must not name an existing
///   non-directory.
///
/// `command` names the subcommand for the messages.
pub(crate) fn resolve_structured_output(
    have_format: bool,
    output: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    command: &str,
) -> StructuredOutput {
    if output.is_some() && output_dir.is_some() {
        die(format_args!(
            "`{command}`: --output (single file) and --output-dir (per-file \
             directory) are mutually exclusive; pass only one"
        ));
    }
    // #661: a destination without a structured format silently no-ops
    // under the default text stream. Error loudly so CI never consumes a
    // stale/missing artifact.
    if !have_format && (output.is_some() || output_dir.is_some()) {
        die(format_args!(
            "`{command} --output`/`--output-dir` needs a structured format: \
             the default text format streams to stdout and writes no files. \
             Pass --format json|yaml|toml|cbor|csv."
        ));
    }
    if let Some(out) = output {
        validate_output_path(&out, command);
        return StructuredOutput::AggregateFile(out);
    }
    if let Some(dir) = output_dir {
        if dir.exists() && !dir.is_dir() {
            die(format_args!(
                "--output-dir must be a directory for `{command}`"
            ));
        }
        return StructuredOutput::Dir(dir);
    }
    StructuredOutput::Stdout
}

/// Resolve the `--metrics` selection (#691) into the library `Metric`
/// families to compute, or `None` (compute all metrics) when the flag is
/// absent. Every requested name is validated against the catalog via the
/// shared #662 did-you-mean validator first, so an unrecognized name is a
/// hard error (exit 1) rather than a silently dropped selection.
pub(crate) fn resolve_selected_metrics(names: &[String]) -> Option<Vec<big_code_analysis::Metric>> {
    if names.is_empty() {
        return None;
    }
    crate::metric_alias::validate_diff_metrics(names).unwrap_or_else(|e| die(e));
    let mut selected: Vec<big_code_analysis::Metric> = names
        .iter()
        .filter_map(|name| crate::metric_alias::metric_for_name(name))
        .collect();
    selected.sort_unstable();
    selected.dedup();
    Some(selected)
}

pub(crate) fn run_command_metrics(
    globals: GlobalOpts,
    args: MetricsArgs,
    preproc: Option<Arc<PreprocResults>>,
) {
    let mut structured = args.structured;
    let selected_metrics = resolve_selected_metrics(&args.metrics);
    // `--format text` (issue #604) is a surface alias for the historical
    // no-`--format` default — the human-readable tree. Collapse it to
    // `None` here so every downstream guard and the dispatch see the
    // exact same shape as an omitted flag; the two paths are then
    // byte-identical by construction.
    structured.normalize_text_format();
    let have_format = structured.output_format.is_some();
    if matches!(structured.output_format, Some(MetricsFormat::Cbor))
        && structured.output.is_none()
        && structured.output_dir.is_none()
    {
        die(CBOR_STDOUT_ERROR);
    }
    let output_target = resolve_structured_output(
        have_format,
        structured.output,
        structured.output_dir,
        "metrics",
    );
    // Build the change-history index once, before the per-file walk, so
    // the dispatch can attach a `vcs` block to each file (issue #328).
    // `--vcs` is additive: outside a repo `default_index` warns and
    // returns `None`, so the AST metrics still emit (vcs block omitted).
    //
    // Only the structured-output path renders the `vcs` block; the
    // human-readable dump (no `--format`) cannot show it. So skip the
    // (expensive) history walk entirely when no format is selected,
    // warning that `--vcs` had no effect rather than doing the walk and
    // silently discarding it.
    // `--vcs-per-function` (issue #329) implies `--vcs`: the file-level
    // block is still attached, plus a blame-derived block on every nested
    // function space. The blame engine is built once here and shared
    // read-only across workers.
    let want_vcs = args.vcs || args.vcs_per_function;
    let (vcs_index, vcs_blame) = match (want_vcs, structured.output_format.is_some()) {
        (true, true) => {
            let index = crate::vcs_command::default_index(&globals);
            let blame = if args.vcs_per_function {
                crate::vcs_command::default_blame(&globals)
            } else {
                None
            };
            (index, blame)
        }
        (true, false) => {
            warn(
                "--vcs / --vcs-per-function has no effect without --format: the \
                 human-readable metrics view does not render the vcs block",
            );
            (None, None)
        }
        (false, _) => (None, None),
    };
    let pretty = structured.pretty;
    let fmt = structured.output_format;
    let action = Action::Metrics {
        format: fmt,
        pretty,
    };
    // #663 counters: track analyzable output and explicitly-named files
    // skipped for unrecognized language, so a run that produced nothing
    // because every named file was unrecognized exits 1.
    let output_produced = Arc::new(AtomicUsize::new(0));
    let explicit_unrecognized = Arc::new(AtomicUsize::new(0));
    // Single-file aggregate mode (#669) streams each space through a
    // channel, collected and written once after the walk.
    let (aggregate_tx, aggregate_rx) = match output_target {
        StructuredOutput::AggregateFile(_) => {
            let (tx, rx) = crossbeam::channel::unbounded();
            (Some(tx), Some(rx))
        }
        _ => (None, None),
    };
    let cfg = Config {
        output_dir: match &output_target {
            StructuredOutput::Dir(dir) => Some(dir.clone()),
            _ => None,
        },
        aggregate_tx,
        vcs_index,
        vcs_blame,
        selected_metrics,
        output_produced: Some(Arc::clone(&output_produced)),
        explicit_unrecognized: Some(Arc::clone(&explicit_unrecognized)),
        ..Config::new(action, &globals, preproc)
    };
    run_walk(globals, cfg);
    if let (StructuredOutput::AggregateFile(out), Some(rx)) = (&output_target, aggregate_rx) {
        write_aggregate(fmt, rx.into_iter().collect::<Vec<_>>(), out, pretty);
    }
    enforce_explicit_unrecognized(&output_produced, &explicit_unrecognized);
}

/// Exit 1 when a `metrics` / `ops` run produced no analyzable output
/// *and* skipped at least one explicitly-named file for an unrecognized
/// language (#663) — the parallel to #596's nonexistent-explicit-path
/// error. A mixed run that analyzed at least one file still exits 0 (the
/// per-file warning already fired on stderr).
pub(crate) fn enforce_explicit_unrecognized(
    output_produced: &AtomicUsize,
    explicit_unrecognized: &AtomicUsize,
) {
    if output_produced.load(Ordering::Relaxed) == 0
        && explicit_unrecognized.load(Ordering::Relaxed) > 0
    {
        die("no output produced: every explicitly-named file had an \
             unrecognized language; pass --language to force a parser");
    }
}

/// Write the streamed per-file results as ONE aggregate document to `out`
/// (#669). Reuses the same `--format` the per-file directory mode uses:
/// CSV concatenates each space's rows; the generic formats emit a
/// top-level array (TOML under a `files` key). `fmt` is `Some` here — an
/// aggregate file without a structured format is rejected upstream by
/// `resolve_structured_output`.
pub(crate) fn write_aggregate(
    fmt: Option<MetricsFormat>,
    items: Vec<AggregateItem>,
    out: &Path,
    pretty: bool,
) {
    let Some(fmt) = fmt else {
        return;
    };
    // `metrics` streams `FuncSpace`; `ops` streams `Ops`. The two never
    // mix in one run (separate subcommands), so the first item's variant
    // determines the element type for the whole aggregate.
    let is_ops = matches!(items.first(), Some(AggregateItem::Ops(_)));
    let result = if is_ops {
        let ops: Vec<Ops> = items
            .into_iter()
            .filter_map(|item| match item {
                AggregateItem::Ops(o) => Some(*o),
                AggregateItem::Metrics(..) => None,
            })
            .collect();
        match fmt.dispatch() {
            MetricsDispatch::Generic(g) => crate::formats::dump_aggregate(g, &ops, out, pretty),
            // CSV is rejected upstream for `ops` (its column schema is
            // metric-shaped), so this arm is unreachable; fall back to the
            // generic JSON array to stay exhaustive without a banned
            // `panic!`.
            MetricsDispatch::Csv => {
                crate::formats::dump_aggregate(GenericFormat::Json, &ops, out, pretty)
            }
        }
    } else {
        let spaces_with_path: Vec<(FuncSpace, PathBuf)> = items
            .into_iter()
            .filter_map(|item| match item {
                AggregateItem::Metrics(space, path) => Some((*space, path)),
                AggregateItem::Ops(_) => None,
            })
            .collect();
        match fmt.dispatch() {
            MetricsDispatch::Csv => crate::formats::dump_csv_aggregate(&spaces_with_path, out),
            MetricsDispatch::Generic(g) => {
                let spaces: Vec<FuncSpace> = spaces_with_path.into_iter().map(|(s, _)| s).collect();
                crate::formats::dump_aggregate(g, &spaces, out, pretty)
            }
        }
    };
    if let Err(e) = result {
        die(format_args!(
            "failed to write aggregate output {}: {e}",
            out.display()
        ));
    }
}

pub(crate) fn run_command_ops(
    globals: GlobalOpts,
    args: StructuredArgs,
    preproc: Option<Arc<PreprocResults>>,
) {
    let mut args = args;
    // `--format text` collapses to the default human-readable tree, exactly
    // as an omitted `--format` does (issue #604).
    args.normalize_text_format();
    let have_format = args.output_format.is_some();
    if matches!(args.output_format, Some(MetricsFormat::Cbor))
        && args.output.is_none()
        && args.output_dir.is_none()
    {
        die(CBOR_STDOUT_ERROR);
    }
    if let Some(MetricsDispatch::Csv) = args.output_format.map(MetricsFormat::dispatch) {
        die(
            "CSV is not supported by `ops` because its column schema is metric-shaped; use `bca metrics --output-format <fmt>`",
        );
    }
    let output_target = resolve_structured_output(have_format, args.output, args.output_dir, "ops");
    let pretty = args.pretty;
    let fmt = args.output_format;
    let action = Action::Ops {
        format: fmt,
        pretty,
    };
    let output_produced = Arc::new(AtomicUsize::new(0));
    let explicit_unrecognized = Arc::new(AtomicUsize::new(0));
    let (aggregate_tx, aggregate_rx) = match output_target {
        StructuredOutput::AggregateFile(_) => {
            let (tx, rx) = crossbeam::channel::unbounded();
            (Some(tx), Some(rx))
        }
        _ => (None, None),
    };
    let cfg = Config {
        output_dir: match &output_target {
            StructuredOutput::Dir(dir) => Some(dir.clone()),
            _ => None,
        },
        aggregate_tx,
        output_produced: Some(Arc::clone(&output_produced)),
        explicit_unrecognized: Some(Arc::clone(&explicit_unrecognized)),
        ..Config::new(action, &globals, preproc)
    };
    run_walk(globals, cfg);
    if let (StructuredOutput::AggregateFile(out), Some(rx)) = (&output_target, aggregate_rx) {
        write_aggregate(fmt, rx.into_iter().collect::<Vec<_>>(), out, pretty);
    }
    enforce_explicit_unrecognized(&output_produced, &explicit_unrecognized);
}
