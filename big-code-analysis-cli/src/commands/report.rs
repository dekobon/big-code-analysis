//! The `bca report` markdown/HTML aggregated-report subcommand.

use super::*;

pub(crate) fn run_command_report(
    globals: GlobalOpts,
    mut args: ReportArgs,
    manifest: Option<&Manifest>,
    preproc: Option<Arc<PreprocResults>>,
) {
    if let Some(m) = manifest {
        m.merge_report(&mut args);
    }
    let policy = SuppressionPolicy::from_no_suppress(args.no_suppress.unwrap_or(false));
    if let Some(ref output) = args.output {
        validate_output_path(output, "report");
    }
    let format = args.resolved_format();
    // Capture the provenance seed-path string before the walk consumes
    // `args.strip_prefix` into the walker `Config` (issue #680).
    let prov_paths = report_seed_paths_display(&args);
    // Build the change-history report (default windows, top = the same
    // per-table cap) before the AST walk consumes `globals`. `--vcs` is
    // additive: outside a git tree `build_default_report` warns and
    // returns `None`, so the report still renders without the section.
    let mut vcs = args
        .vcs
        .then(|| crate::vcs_command::build_default_report(&globals, args.top))
        .flatten();
    let (tx, rx) = crossbeam::channel::unbounded();
    // When the change-history section is present, also collect each file's
    // cyclomatic sum so its hotspot score can be joined after the walk
    // (issue #615). Only wired for `report --vcs`; a plain `report` leaves
    // the sender `None` and the hotspot column omitted downstream.
    let (hotspot_rx, report_hotspot_tx) = if vcs.is_some() {
        let (htx, hrx) = crossbeam::channel::unbounded();
        (Some(hrx), Some(htx))
    } else {
        (None, None)
    };
    let cfg = Config {
        markdown_tx: Some(tx),
        report_hotspot_tx,
        strip_prefix: args.strip_prefix,
        ..Config::new(Action::Report, &globals, preproc)
    };
    run_walk(globals, cfg);

    // ConcurrentRunner::run() consumed Config (and thus the Senders).
    // All worker threads have joined, so `rx.into_iter()` terminates.
    let summaries: Vec<FunctionSummary> = rx.into_iter().collect();
    // Join the per-file hotspot scores onto the change-history rows before
    // rendering, so `report --vcs` fills the Hotspot column from the AST
    // metrics computed in this same run (issue #615).
    if let (Some(default), Some(hotspot_rx)) = (vcs.as_mut(), hotspot_rx) {
        let cyclomatic_sums: Vec<(PathBuf, f64)> = hotspot_rx.into_iter().collect();
        default.join_hotspot_scores(&cyclomatic_sums);
    }
    let vcs = vcs.map(|d| d.report);
    let top = args.top;

    // Provenance footer facts (issue #680): version from the package, date
    // (SOURCE_DATE_EPOCH-overridable) from `provenance`, the user's seed paths
    // (what they asked to scan, not every walked file), the resolved `--top`,
    // and whether suppression markers were honored.
    let date = crate::provenance::resolved_date();
    let prov = crate::provenance::Provenance {
        version: env!("CARGO_PKG_VERSION"),
        date: &date,
        paths: &prov_paths,
        top,
        policy,
    };

    // Source the report's advisory cutoffs (Actionable Summary, CC note,
    // Many-Parameters filter) from the manifest `[thresholds]` table when one
    // is present, so the report's advice matches the configured `bca check`
    // gate; fall back to the built-in defaults otherwise (issue #630). This
    // reads the manifest hard-threshold scalars for *presentation* only — it
    // does not touch the offender-gating path.
    let advisory = match manifest {
        Some(m) => AdvisoryThresholds::from_manifest_hard(&m.thresholds().hard),
        None => AdvisoryThresholds::DEFAULT,
    };

    // `generate_*_with_vcs` already accept the change-history section as an
    // `Option`, so dispatch only on the output format and pass the optional
    // report straight through rather than enumerating the four format×vcs
    // combinations.
    let vcs = vcs.as_ref();
    let report = match format {
        ReportFormat::Markdown => {
            generate_report_with_vcs(&summaries, top, policy, &advisory, vcs, Some(&prov))
        }
        ReportFormat::Html => {
            generate_html_report_with_vcs(&summaries, top, policy, &advisory, vcs, Some(&prov))
        }
    };
    write_output_or_stdout(args.output.as_deref(), "write report to", report.as_bytes());
}

/// The compact seed-path string the provenance footer reports: the `--paths`
/// the user passed (after manifest merge), joined by `, `, or `.` when none
/// were given (the implicit current-directory default). Non-UTF-8 path
/// components fall back to their lossy form for display only — the footer is
/// human-facing prose, never a map key (cf. the path-as-identifier rule).
pub(crate) fn report_seed_paths_display(args: &ReportArgs) -> String {
    let paths = args.seed_paths();
    if paths.is_empty() {
        ".".to_owned()
    } else {
        paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
