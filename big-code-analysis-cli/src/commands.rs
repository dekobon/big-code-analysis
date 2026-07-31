//! Top-level command dispatch for the `bca` CLI.
//!
//! Owns the public `run()` entry point (called by `bca`'s `main` and
//! by `xtask` for man-page rendering) and the dispatch scaffolding it
//! needs: manifest discovery, preproc loading, and the clap-parse /
//! legacy-hint glue.
//!
//! The per-command handlers live in area submodules — `analyze`
//! (metrics / ops), `scan` (find / count / dump / functions /
//! list-metrics), `report`, `preproc`, `init`, `diff_cmd`,
//! `exemptions`, and `check` (whose multi-stage pipeline is further
//! split under `check/`). Each is re-exported here so `run()` reaches
//! every handler by its bare name. Argument types, the parallel
//! walker, and the diagnostic helpers live in `lib.rs` / sibling
//! crate modules.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches};

use big_code_analysis::{CountCollector, PreprocResults, SuppressionPolicy};
use big_code_analysis::{fix_includes, write_file};

use crate::baseline::{self, Coverage};
use crate::baseline_diff::{BaselineDiff, SectionFilter};
use crate::check_format::{self, violation_to_offender};
use crate::diff;
use crate::exemptions::{BaselineRow, BaselineSection, ExemptionsReport, FileMarkers, MarkerRow};
use crate::format_util::MetricScalar;
use crate::formats::{
    CBOR_STDOUT_ERROR, GenericFormat, MetricsDispatch, MetricsFormat, ReportFormat,
};
use crate::html_report::generate_html_report_with_vcs;
use crate::manifest::{self, Manifest};
use crate::markdown_report::advisory::AdvisoryThresholds;
use crate::markdown_report::{FunctionSummary, generate_report_with_vcs};
use crate::metric_catalog::write_metrics;
use crate::metric_diff::DiffSide;
use crate::thresholds::{
    ParsedThresholds, SoftLimit, ThresholdSet, Violation, breaches_limit, render_violation_line,
    scale_threshold,
};
use big_code_analysis::{FuncSpace, Ops};

use crate::{
    Action, AggregateItem, CheckArgs, Cli, Command, Config, CountArgs, DiffBaselineArgs,
    ExemptionsArgs, FindArgs, GlobalOpts, InitArgs, LineRange, ListMetricsArgs, MetricsArgs,
    OutputFormat, PreprocArgs, PrintConfigFormat, ReportArgs, StripCommentsArgs, StructuredArgs,
    SummaryFile, Tier, TierSpec, die, die_io, group_files_by_basename, legacy_hint, load_baseline,
    load_preproc_data, load_threshold_config, note, read_exclude_patterns_from, resolve_walk_files,
    run_walk, run_walk_collecting, run_walk_resolved, validate_output_path, warn, write_atomic,
    write_output_or_stdout, write_stdout_or_die,
};

mod analyze;
mod check;
mod diff_cmd;
mod exemptions;
mod init;
mod preproc;
mod report;
mod scan;

pub(crate) use {
    analyze::*, check::*, diff_cmd::*, exemptions::*, init::*, preproc::*, report::*, scan::*,
};

/// Parse `std::env::args_os()` and execute the selected `bca`
/// subcommand. Intended to be called from the `bca` binary's `main`,
/// which is a one-liner over this function.
///
/// # Termination contract
///
/// This function **may terminate the calling process** rather than
/// return. It is not a re-entrant library entry point:
///
/// - clap argument-parsing failures exit 0 on `--help` / `--version`
///   and exit 1 on usage errors (unknown flag, bad subcommand,
///   `value_parser` rejection). The exit-1 mapping (#594) keeps clap's
///   usage errors out of the 2-5 metric-gate band.
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
    // bca: suppress(cyclomatic, abc)
    // Flat top-level subcommand dispatch (one arm per `Command` variant) —
    // cyclomatic is arm count, not nested branching; cognitive stays enforced.
    let (cli, num_jobs_from_cli) = parse_cli_with_legacy_hint();
    let Cli { universal, command } = cli;

    // Each walking subcommand carries its own flattened walk / tuning /
    // preproc / output flag groups (#597); `to_globals` assembles them
    // (plus the universal flags) into the runtime `GlobalOpts` the walk
    // plumbing consumes. The `bca.toml` manifest is then merged *under*
    // those CLI flags by `with_manifest`. `init` (scaffolds config),
    // `diff` (no global config), `diff-baseline`, and `list-metrics`
    // (walk nothing) skip manifest discovery — see their arms.
    match command {
        Command::ListMetrics(args) => run_command_list_metrics(args),
        Command::DiffBaseline(args) => run_command_diff_baseline(args),
        Command::Diff(args) => {
            let globals = args.to_globals(&universal);
            run_command_diff(globals, args);
        }
        Command::Init(args) => {
            let globals = args.to_globals(&universal);
            let preproc = load_preproc(&globals);
            run_command_init(globals, args, preproc);
        }
        Command::Dump(args) => {
            let globals = with_manifest(args.to_globals(&universal), num_jobs_from_cli).0;
            let preproc = load_preproc(&globals);
            run_command_dump(globals, args.line, preproc);
        }
        Command::Functions(args) => {
            let globals = with_manifest(args.to_globals(&universal), num_jobs_from_cli).0;
            let preproc = load_preproc(&globals);
            run_command_functions(globals, preproc);
        }
        Command::Metrics(args) => {
            let globals = with_manifest(args.to_globals(&universal), num_jobs_from_cli).0;
            let preproc = load_preproc(&globals);
            run_command_metrics(globals, args, preproc);
        }
        Command::Ops(args) => {
            let globals = with_manifest(args.to_globals(&universal), num_jobs_from_cli).0;
            let preproc = load_preproc(&globals);
            run_command_ops(globals, args, preproc);
        }
        Command::Vcs(mut args) => {
            let (globals, manifest) = with_manifest(args.to_globals(&universal), num_jobs_from_cli);
            if let Some(m) = &manifest {
                m.merge_vcs(&mut args);
            }
            crate::vcs_command::run(globals, *args);
        }
        Command::Report(args) => {
            let (globals, manifest) = with_manifest(args.to_globals(&universal), num_jobs_from_cli);
            let preproc = load_preproc(&globals);
            run_command_report(globals, args, manifest.as_ref(), preproc);
        }
        Command::Find(args) => {
            let globals = with_manifest(args.to_globals(&universal), num_jobs_from_cli).0;
            let preproc = load_preproc(&globals);
            run_command_find(globals, args, preproc);
        }
        Command::Count(args) => {
            let globals = with_manifest(args.to_globals(&universal), num_jobs_from_cli).0;
            let preproc = load_preproc(&globals);
            run_command_count(globals, args, preproc);
        }
        Command::StripComments(args) => {
            let globals = with_manifest(args.to_globals(&universal), num_jobs_from_cli).0;
            let preproc = load_preproc(&globals);
            run_command_strip_comments(globals, args, preproc);
        }
        Command::Check(args) => {
            let (globals, manifest) = with_manifest(args.to_globals(&universal), num_jobs_from_cli);
            let preproc = load_preproc(&globals);
            run_check(globals, *args, manifest.as_ref(), preproc);
        }
        Command::Preproc(args) => {
            let globals = with_manifest(args.to_globals(&universal), num_jobs_from_cli).0;
            run_command_preproc(globals, args);
        }
        Command::Exemptions(args) => {
            let (globals, manifest) = with_manifest(args.to_globals(&universal), num_jobs_from_cli);
            let preproc = load_preproc(&globals);
            run_command_exemptions(globals, args, manifest.as_ref(), preproc);
        }
    }
}

/// Auto-discover a `bca.toml` manifest (unless `--no-config`) and merge
/// its global keys *under* the parsed CLI flags, returning the merged
/// [`GlobalOpts`] and the discovered manifest (so callers that also need
/// the check-/vcs-/report-only keys can merge those at their own layer).
///
/// `num_jobs_from_cli` tells the merge whether `--jobs` was set on the
/// command line (vs. left at its `auto` default) so a manifest value
/// only overrides the default, never an explicit flag.
fn with_manifest(
    mut globals: GlobalOpts,
    num_jobs_from_cli: bool,
) -> (GlobalOpts, Option<Manifest>) {
    let manifest = if globals.no_config {
        None
    } else {
        manifest::discover_and_load()
    };
    if let Some(m) = &manifest {
        m.merge_globals(&mut globals, num_jobs_from_cli);
    }
    (globals, manifest)
}

/// Load the consumer-side preprocessor data named by `--preproc-data`,
/// if any. A subcommand whose flag group omits the preproc-consume field
/// always has `preproc_data == None`, so this returns `None` there
/// without a special case.
fn load_preproc(globals: &GlobalOpts) -> Option<Arc<PreprocResults>> {
    globals.preproc_data.as_ref().map(|p| load_preproc_data(p))
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
            exit_clap_error(&err);
        }
    };
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|err| exit_clap_error(&err));
    // clap accepted the input, so any deprecated *spelling* was honored
    // silently — emit the one-cycle migration warning now (#646). The scan
    // runs on the raw argv because clap normalizes each `alias =` to its
    // canonical id before `matches` is built.
    crate::deprecations::warn_deprecated_aliases(std::env::args_os());
    (cli, num_jobs_set_on_cli(&matches))
}

/// Print a clap argument-parsing error and terminate the process,
/// mapping argv/usage/value-parse failures to exit code **1** instead
/// of clap's built-in exit 2.
///
/// Exit code 2 is reserved by the workspace exit-code contract (#561,
/// #594) for the `check` / `vcs commit --fail-above` metric gates. clap's
/// default `Error::exit` collides with that band on every usage error
/// (unknown flag, bad subcommand, `value_parser` rejection), so CI
/// scripts branching on `$? -eq 2` would misread a typo'd flag as a
/// threshold failure. This helper preserves clap's rendered output —
/// colored, with usage and suggestions — and only overrides the code.
///
/// `--help` / `--version` (and any other "display" outcome) are not
/// errors: clap routes them to stdout via `use_stderr() == false`, and
/// they must keep exiting 0. Those paths delegate to clap's own
/// `Error::exit`.
fn exit_clap_error(err: &clap::Error) -> ! {
    if err.use_stderr() {
        let _ = err.print();
        // Argv/usage failures share `die`'s tool-error code so the
        // contract reads "1 = tool error".
        process::exit(crate::EXIT_TOOL_ERROR);
    }
    // Help / version: stdout, exit 0 — clap's default is already correct.
    err.exit();
}

/// Whether `--num-jobs` (now `--jobs`) was supplied on the command line
/// (vs. left at its `auto` default). Since #597 `num_jobs` is scoped to
/// the walking subcommands rather than `global = true`, so its value
/// source surfaces in the subcommand's matches, not the root's — walk the
/// chain. The arg id is absent at any level that does not define it
/// (e.g. the root, or a `vcs commit`/`vcs trend` leaf that takes no walk
/// flags), and `value_source` panics on an unknown id, so only query a
/// level whose `ids()` actually carry `num_jobs`.
fn num_jobs_set_on_cli(matches: &ArgMatches) -> bool {
    let defined_here = matches.ids().any(|id| id.as_str() == "num_jobs");
    if defined_here && matches.value_source("num_jobs") == Some(ValueSource::CommandLine) {
        return true;
    }
    match matches.subcommand() {
        Some((_, sub)) => num_jobs_set_on_cli(sub),
        None => false,
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
