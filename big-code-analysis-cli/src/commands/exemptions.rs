//! The `bca exemptions` audit subcommand.

use super::*;

/// Default baseline file audited by `bca exemptions` when neither
/// `--baseline` nor `bca.toml`'s `[check] baseline` is set. Matches the
/// filename `bca init` scaffolds and `bca check --write-baseline`
/// defaults to.
pub(crate) const DEFAULT_BASELINE_FILE: &str = ".bca-baseline.toml";

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
pub(crate) fn run_command_exemptions(
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
        validate_output_path(output, "exemptions");
    }

    // No `--*-only` flag selects every section; one selects just that
    // one (clap enforces mutual exclusivity).
    let only_any = args.markers_only || args.excludes_only || args.baseline_only;
    let want_markers = !only_any || args.markers_only;
    let want_excludes = !only_any || args.excludes_only;
    let want_baseline = !only_any || args.baseline_only;

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
    write_output_or_stdout(
        args.output.as_deref(),
        "write exemptions report to",
        rendered.as_bytes(),
    );
}

/// Run the suppression-marker walk and return the flattened rows sorted
/// by `(path, line)` for deterministic output. Files arrive in
/// worker-completion order, so the sort cannot be skipped even though
/// each file's markers are already line-sorted by the collector.
pub(crate) fn collect_marker_rows(
    globals: GlobalOpts,
    preproc: Option<Arc<PreprocResults>>,
) -> Vec<MarkerRow> {
    let (tx, rx) = crossbeam::channel::unbounded();
    let cfg = Config {
        exemptions_tx: Some(tx),
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
pub(crate) fn resolve_exclude_globs(args: &ExemptionsArgs) -> Vec<String> {
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
pub(crate) fn resolve_baseline_section(args: &ExemptionsArgs) -> BaselineSection {
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
    // Order on the full identity (see [`BaselineIdentity`]).
    // `diff_entries` walks a `HashMap`, so the input order is arbitrary
    // and every displayed row needs a total order to be reproducible.
    entries.sort_by(baseline::cmp_identity);
    BaselineSection {
        path: path.display().to_string(),
        entries,
    }
}
