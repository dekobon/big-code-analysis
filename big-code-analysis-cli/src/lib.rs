//! Library surface for the `bca` CLI.
//!
//! Exists so the workspace `xtask` crate can render man pages from the
//! same `clap::Command` tree that `bca` parses at runtime — the binary
//! `main` is a one-liner that delegates to [`run`].
//!
//! # Embedder contract
//!
//! This crate is published to crates.io to support man-page generation
//! and to keep the binary's `main` trivial; it is **not** a re-entrant
//! library API. [`run`] and the internal helpers it calls
//! (`die` / `die_io`, `run_check`, etc.) terminate the calling process
//! via [`std::process::exit`] on user-input errors (bad threshold
//! specs, missing paths, parser failures, broken pipes, and so on)
//! and on the `check` subcommand's "thresholds exceeded" exit-2 path.
//! Hosting [`run`] inside another process will tear that process down
//! without unwinding. If you need a re-entrant entry point, drive the
//! [`big_code_analysis`] library crate directly.

#![allow(
    clippy::too_many_lines,
    clippy::struct_excessive_bools,
    clippy::similar_names,
    clippy::needless_pass_by_value,
    // The CLI is assembled from split modules that each `use super::*`
    // and a crate-root re-export hub of `pub(crate) use <module>::*`
    // globs — the same module-split idiom as `src/metrics/abc`. Allow
    // the wildcard idiom crate-wide rather than repeating a per-file
    // `#![allow]` in every extracted module.
    clippy::wildcard_imports,
    // `run` panics on a handful of provably-unreachable invariants
    // (mutex poisoning where every worker thread has joined, channel
    // sends after run_walk returns). Each one is documented at the
    // call site with an `expect` reason — surfacing them in a `# Panics`
    // section on the entry point adds noise without adding signal.
    clippy::missing_panics_doc
)]
// Production-only `unwrap()` ban. See `[workspace.lints.clippy]` in the
// root `Cargo.toml` for why this is a per-root attribute and not a
// Cargo lint (#1227).
#![cfg_attr(not(test), warn(clippy::unwrap_used))]
mod baseline;
mod baseline_diff;
mod check_flags;
mod check_format;
mod cli_args;
mod commands;
mod default_thresholds;
mod deprecations;
mod diag;
mod diff;
mod dispatch;
mod exemptions;
mod format_util;
mod formats;
mod html_report;
mod manifest;
mod markdown_report;
mod metric_alias;
mod metric_catalog;
mod metric_diff;
mod ordered_stdout;
mod path_io;
mod provenance;
mod qualified_name;
mod threshold_lang;
mod threshold_soft;
mod threshold_suggestion;
mod thresholds;
mod vcs_command;
mod vcs_jit;
mod vcs_report;
mod vcs_trend;
mod walk;
mod walk_seed;

pub use cli_args::Cli;
pub(crate) use cli_args::*;
pub use commands::run;
pub(crate) use diag::*;
use dispatch::act_on_file;
pub(crate) use path_io::*;
pub(crate) use walk::*;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Display;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use clap::{Args, Parser, Subcommand, ValueEnum};
use globset::{Glob, GlobSet, GlobSetBuilder};

use baseline::Baseline;
use check_flags::{CiDetect, ExitCodes, SummaryFile, Tier, TierSpec};
use check_format::AggregatedFormat;
use formats::{JitFormat, MetricsFormat, ReportFormat, TrendFormat, VcsFormat};
use markdown_report::FunctionSummary;
use metric_catalog::ListMetricsMode;
use metric_diff::DiffSide;
use threshold_lang::LanguageThresholds;
use thresholds::{
    ParsedThresholds, ThresholdConfig, Violation, parse_cli_threshold, parse_fail_above,
    split_thresholds_table,
};

use big_code_analysis::LANG;
use big_code_analysis::{
    ConcurrentRunner, CountCollector, FilesData, MetricsOptions, NumJobs, PreprocResults,
    SuppressionPolicy,
};
use big_code_analysis::{FuncSpace, Ops, get_from_ext, get_language_for_file, read_file};

/// Process exit code for tool errors — bad flags/values, unreadable
/// input, I/O failures. Distinct from [`EXIT_GATE_BREACH`] so CI can
/// tell a broken invocation from a failed metric gate (#594); the full
/// contract lives in the book's commands chapter.
pub(crate) const EXIT_TOOL_ERROR: i32 = 1;

/// Process exit code for a metric-gate breach: `check` threshold
/// violations (the stable contract; the tiered 3-5 variants under
/// `--exit-codes=tiered` are derived in the check outcome) and
/// `vcs commit --fail-above`.
pub(crate) const EXIT_GATE_BREACH: i32 = 2;

#[derive(Debug)]
struct Config {
    action: Action,
    /// Per-file output directory (`--output-dir <DIR>`) for `metrics` /
    /// `ops` (#669) — the historical `--output` directory-tree semantics,
    /// where each input file's document is written under this directory
    /// named by its path plus the format extension. Mutually exclusive
    /// with `output`. `None` for every other flow.
    output_dir: Option<PathBuf>,
    /// Sender for streaming each per-file result when `metrics` / `ops`
    /// run in single-file aggregate mode (`--output <FILE>`, #669). The
    /// command runner collects the records after the walk and writes one
    /// document. `None` for the directory / stdout paths, which write
    /// per-file inline.
    ///
    /// Held bare by the shared `Config`, with no `Mutex`: the sender is
    /// `Sync`, so every worker sends through the same one. The wrapper
    /// each `*_tx` used to carry dated from a Rust in which
    /// `std::sync::mpsc::Sender` was `Send`-only; it has been `Sync`
    /// since 1.72, so by the time #1119 removed the `Mutex` it was
    /// buying nothing but a lock taken once per file across the whole
    /// pool and a poisoned-lock branch nothing could reach.
    ///
    /// `crossbeam` rather than `std::sync::mpsc` only because the
    /// library's `ConcurrentRunner` already routes its job queue through
    /// it, so the workspace carries one channel implementation; `mpsc`
    /// would serve here equally well. The same applies to every `*_tx`
    /// below.
    aggregate_tx: Option<crossbeam::channel::Sender<AggregateItem>>,
    /// Reorder buffer for the streaming stdout modes of `metrics` /
    /// `ops` (#1303) — the destination that cannot sort after the fact
    /// the way `--output <FILE>` does (#1244), because it has already
    /// written each document by then.
    ///
    /// Installed by [`run_walk_tallying`] for the walks
    /// [`streams_documents_to_stdout`](Self::streams_documents_to_stdout)
    /// identifies, never by a command runner: the two subcommands that
    /// stream then cannot drift apart on it, and a third cannot forget
    /// it. `None` for every other flow, which leaves emission the
    /// unordered write-as-you-go it has always been.
    ordered_stdout: Option<Arc<ordered_stdout::OrderedStdout>>,
    language: Option<LANG>,
    line_start: Option<usize>,
    line_end: Option<usize>,
    preproc_lock: Option<Arc<Mutex<PreprocResults>>>,
    preproc: Option<Arc<PreprocResults>>,
    count_lock: Option<CountCollector>,
    /// Sender for streaming `FunctionSummary` records when running `report`.
    markdown_tx: Option<crossbeam::channel::Sender<FunctionSummary>>,
    /// Sender for streaming each file's `(absolute path, cyclomatic sum)`
    /// when running `report --vcs`, so the change-history section can join
    /// the same hotspot score (`complexity × recent churn`) that
    /// `bca metrics --vcs` attaches per file (issue #615). The absolute
    /// path is canonicalized against the index work-tree downstream — the
    /// identical match `vcs_command::inject` uses — so the join is correct
    /// regardless of `--strip-prefix` or the walk-root spelling. `None`
    /// for every flow other than `report --vcs`.
    report_hotspot_tx: Option<crossbeam::channel::Sender<(PathBuf, f64)>>,
    /// Path prefix stripped from file paths in the markdown report.
    strip_prefix: String,
    /// Pre-resolved thresholds for `Action::Check`: the global set plus
    /// one fully resolved set per language carrying a
    /// `[thresholds.lang.<slug>]` override (#1141). `None` for every
    /// other action.
    thresholds: Option<Arc<LanguageThresholds>>,
    /// Sender for streaming [`Violation`] records when running `check`.
    check_tx: Option<crossbeam::channel::Sender<Violation>>,
    /// Sender for streaming per-file suppression-marker batches when
    /// running `exemptions`.
    exemptions_tx: Option<crossbeam::channel::Sender<exemptions::FileMarkers>>,
    /// Counts how many files survived expansion and glob filtering and
    /// were actually dispatched to `act_on_file`. `Action::Check` reads
    /// this after the walk to distinguish "all clean" (counter > 0,
    /// no violations) from "no files matched" (counter == 0), so a
    /// typo in `--paths` does not silently pass CI.
    files_dispatched: Option<Arc<AtomicUsize>>,
    /// Counts files the generated-code detector skipped before parsing.
    /// `Action::Check` reads this after the walk to say, by default, how
    /// much of the input the gate declined to look at — a `@generated`
    /// marker in a pull request otherwise removes a file from the gate
    /// with nothing on stderr. `None` for flows that do not report it.
    generated_skipped: Option<Arc<AtomicUsize>>,
    /// Counts input files whose contents could not be read at all —
    /// permission denied, a broken symlink, a path that vanished
    /// mid-walk. A failed read deliberately leaves `files_dispatched`
    /// untouched, so this counter is the only record that the runner
    /// saw the file.
    ///
    /// Unconditional rather than `Option`, because every walk enforces
    /// the same rule: [`run_walk_resolved_tallying`] installs it and the
    /// post-walk guard exits 1 rather than let a command report a result
    /// derived from a partially analysed input set — a gate verdict
    /// (#1060), a metrics document, or a `diff --since` comparison whose
    /// missing files read as removed rather than as an I/O failure
    /// (#1098).
    read_failures: Arc<AtomicUsize>,
    /// Counts input files whose *output* could not be written — a
    /// read-only `--output-dir`, a full disk. The mirror image of
    /// `read_failures`, enforced by the same post-walk guard: the
    /// per-file error already reached stderr, but until it was tallied
    /// `bca dump` onto a filling filesystem left a truncated document
    /// and still exited 0. `BrokenPipe` is excluded, matching the
    /// swallow-it policy the walk's error printer applies to a closed
    /// `| head`.
    write_failures: Arc<AtomicUsize>,
    /// Seeds the user named *explicitly as files* on the command line
    /// (or via `--paths-from` / a manifest `paths` key), in the emitted
    /// path form. A file in this set whose language is unrecognized is a
    /// user error — it warns on stderr unconditionally (not gated behind
    /// `-w`) and bumps `explicit_unrecognized` (#663). A file discovered
    /// by a directory walk is absent here and stays silently skipped.
    /// Empty for every flow until `run_walk` populates it from
    /// [`expand_seed_paths`].
    explicit_seeds: Arc<std::collections::HashSet<PathBuf>>,
    /// Counts explicit-seed files skipped because their language is
    /// unrecognized (#663). Read after the walk together with
    /// `output_produced`: a run that produced nothing *and* skipped at
    /// least one explicitly-named file exits 1, mirroring the #596
    /// nonexistent-explicit-path error. `None` for flows that do not
    /// enforce the rule.
    explicit_unrecognized: Option<Arc<AtomicUsize>>,
    /// Counts files that resolved to a recognized language and were
    /// handed to dispatch (#663). Distinct from `files_dispatched`, which
    /// also counts empty / unrecognized / generated skips. A zero value
    /// after the walk means the run produced no analyzable output.
    output_produced: Option<Arc<AtomicUsize>>,
    /// Whether to honor or ignore in-source suppression markers when
    /// emitting threshold violations. Only meaningful for
    /// `Action::Check`; the field is defaulted to `Honor` for every
    /// other action so the new code path is invisible to existing
    /// flows. Flipped to `Ignore` by `--no-suppress`.
    suppression_policy: SuppressionPolicy,
    /// When true (set by `bca check --report-suppressed`), marker-suppressed
    /// offenders are kept and tagged rather than dropped, so the code-scan
    /// document can surface them as suppressed alerts. They still never reach
    /// the gate or exit code. Defaults off for every action.
    report_suppressed: bool,
    warning: bool,
    /// When true, files whose head matches a generated-code marker are
    /// skipped before parsing. Defaults on; flipped off by
    /// `--no-skip-generated`.
    skip_generated: bool,
    /// When true, log a stderr line for each file auto-skipped by the
    /// generated-code detector. Also enabled by `warning` (which logs
    /// every skip reason); `report_skipped` is the dedicated flag for
    /// users who want the generated-skip audit without the rest of the
    /// warning stream.
    report_skipped: bool,
    /// When true, [`get_function_spaces_with_options`] is used in
    /// place of [`get_function_spaces`] and [`MetricsOptions::exclude_tests`]
    /// is set, so language modules that override
    /// `Checker::should_skip_subtree` (currently only Rust) prune
    /// their test subtrees before metric computation. See
    /// `GlobalOpts::exclude_tests` for the user-facing description.
    exclude_tests: bool,
    /// When true, Rust's `?` operator does NOT contribute to cyclomatic
    /// complexity. Projected onto
    /// [`MetricsOptions::with_count_cyclomatic_try`] (negated). Defaults
    /// off, so `?` counts and numbers match the published default
    /// (#409). Set by `--cyclomatic-count-try=false` (or the deprecated
    /// `--no-cyclomatic-try` alias) or the `cyclomatic_count_try`
    /// manifest key.
    no_cyclomatic_try: bool,
    /// When true (`--baseline-fuzzy-match`), the check walk stamps each
    /// emitted [`Violation`] with a normalised body hash so the baseline
    /// can match a renamed-but-unchanged function. Off by default; the
    /// hashing cost (only for offending functions) is paid only when the
    /// flag is set. Meaningful only for `Action::Check`.
    fuzzy_baseline: bool,
    /// Pre-built change-history index, shared read-only across workers,
    /// set by `bca metrics --vcs`. When present, the per-file metrics
    /// dispatch attaches the matching `vcs` block (plus a hotspot score
    /// derived from the file's cyclomatic sum) to each file-level space.
    /// `None` for every other flow. Issue #328.
    vcs_index: Option<Arc<big_code_analysis::vcs::HistoryIndex>>,
    /// Per-function blame engine, shared read-only across workers, set by
    /// `bca metrics --vcs-per-function`. When present, the per-file
    /// dispatch blames each file and attaches a `vcs` block to every
    /// nested function space (in addition to the file-level block from
    /// `vcs_index`). `None` for every other flow. Issue #329.
    vcs_blame: Option<Arc<big_code_analysis::vcs::PerFunctionBlame>>,
    /// Resolved color policy for the human-readable `text` dumps
    /// (`dump`, `find`, `functions`, and the default `metrics` / `ops`
    /// trees). Resolved once from `--color` + `NO_COLOR` + stdout tty
    /// detection at command entry (issue #605) and threaded into the
    /// library `*_with_color` dump entry points, so piped output is
    /// escape-free by default. Inert for structured / file output.
    color: big_code_analysis::ColorMode,
    /// Subset of metrics to compute. `None` (the default) computes
    /// every metric; `Some(set)` restricts via
    /// [`MetricsOptions::with_only`], which auto-resolves each metric's
    /// dependencies.
    ///
    /// Two flows set it. `run_command_metrics` passes the user's
    /// `bca metrics --metrics` selection (issue #691). `run_check_walk`
    /// derives it from the resolved `ThresholdSet` (issue #1113), since
    /// a gate reads only the families it thresholds. Every other flow
    /// leaves it `None`.
    selected_metrics: Option<Vec<big_code_analysis::Metric>>,
}

impl Config {
    /// Build a `Config` for `action`, populating the fields every command
    /// shares from `globals`. Per-command extras (`output`, `count_lock`,
    /// `markdown_tx`, `strip_prefix`, and the `dump`/`find`
    /// `line_start`/`line_end` bounds) are set on the returned value at
    /// the call site.
    fn new(action: Action, globals: &GlobalOpts, preproc: Option<Arc<PreprocResults>>) -> Self {
        let language = resolve_language(globals.language.as_deref(), &action);
        Self {
            action,
            output_dir: None,
            aggregate_tx: None,
            ordered_stdout: None,
            language,
            // Set by the `dump`/`find` call sites from their `LineRange`
            // args; every other action leaves the range unbounded.
            line_start: None,
            line_end: None,
            preproc_lock: None,
            preproc,
            count_lock: None,
            markdown_tx: None,
            report_hotspot_tx: None,
            strip_prefix: String::new(),
            thresholds: None,
            check_tx: None,
            exemptions_tx: None,
            files_dispatched: None,
            generated_skipped: None,
            read_failures: Arc::new(AtomicUsize::new(0)),
            write_failures: Arc::new(AtomicUsize::new(0)),
            explicit_seeds: Arc::new(std::collections::HashSet::new()),
            explicit_unrecognized: None,
            output_produced: None,
            suppression_policy: SuppressionPolicy::Honor,
            report_suppressed: false,
            warning: globals.warning,
            skip_generated: !globals.no_skip_generated,
            report_skipped: globals.report_skipped,
            exclude_tests: globals.exclude_tests,
            // `GlobalOpts` carries the positive sense (`?` counts) with
            // `None` = "use the default"; the library option is the
            // negated form, so default-true maps to `no_cyclomatic_try =
            // false` and published numbers stay byte-identical (#666).
            no_cyclomatic_try: !globals.count_cyclomatic_try.unwrap_or(true),
            // Defaults off; `run_check_walk` flips it on for the check
            // action when `--baseline-fuzzy-match` is set.
            fuzzy_baseline: false,
            // Set by `run_command_metrics` only when `--vcs` is passed.
            vcs_index: None,
            // Set by `run_command_metrics` only when `--vcs-per-function`
            // is passed.
            vcs_blame: None,
            // Resolve the color policy once at construction: explicit
            // `--color` > `NO_COLOR` > stdout tty detection. Threaded
            // into the library `*_with_color` dump entry points (#605).
            color: globals.color.resolve(),
            // Set by `run_command_metrics` only when `--metrics` is
            // passed; every other flow computes the full set.
            selected_metrics: None,
        }
    }

    /// Project this `Config` onto the library's `MetricsOptions`
    /// surface. Centralising the projection here means new metric
    /// options land in one place instead of being duplicated across
    /// every `act_on_file` arm that drives a metric computation.
    #[inline]
    fn metrics_options(&self) -> MetricsOptions {
        let mut opts = MetricsOptions::default()
            .with_exclude_tests(self.exclude_tests)
            .with_count_cyclomatic_try(!self.no_cyclomatic_try);
        // `--metrics` (issue #691) restricts the computed set to the
        // requested metrics plus their dependencies (resolved by
        // `with_only`). Absent → every metric, matching prior behavior.
        if let Some(selected) = &self.selected_metrics {
            opts = opts.with_only(selected);
        }
        opts
    }

    /// Whether this walk streams one structured document per file to
    /// stdout — the only destination whose document order a parallel
    /// walk can scramble (#1303).
    ///
    /// The other three settle it elsewhere: `--output <FILE>` sorts the
    /// whole set after the walk (#1244), `--output-dir <DIR>` gives each
    /// document its own file, and the human-readable tree (no
    /// `--format`) is a rendering for someone reading along rather than
    /// a document anyone diffs.
    ///
    /// Derived from the destination fields rather than set by each
    /// command runner, so `metrics` and `ops` cannot drift apart on it
    /// and a future streaming subcommand cannot forget it.
    fn streams_documents_to_stdout(&self) -> bool {
        self.output_dir.is_none()
            && self.aggregate_tx.is_none()
            && matches!(
                self.action,
                Action::Metrics {
                    format: Some(_),
                    ..
                } | Action::Ops {
                    format: Some(_),
                    ..
                }
            )
    }
}

/// The three bytes of a UTF-8 byte-order mark (U+FEFF encoded as
/// `EF BB BF`). Stripped from the front of each `--paths-from` line so an
/// editor that saved the list as UTF-8-with-BOM does not turn the first
/// path into a literal `\u{feff}/path` that matches no real file (the
/// per-line BOM strip the previous `collect_lines` reader applied).
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Process an already-resolved terminal file list concurrently and
/// return how many of those files could not be read at all.
///
/// The single seam every walk passes through, so the read-failure tally
/// is collected once here instead of per command runner (#1098). Callers
/// that want the standard exit-1 contract go through the `run_walk*`
/// wrappers; `bca diff --since` uses this directly so it can unwind its
/// temp trees before reporting.
///
/// `walk_errors` comes in from the caller's own seed expansion (#1131):
/// the file list is already resolved by the time it arrives here, so
/// this seam cannot observe a traversal failure and must be told of one.
/// Threading it through rather than defaulting it here is what forces a
/// caller that resolved its own list to account for the tally.
fn run_walk_resolved_tallying(
    paths: Vec<PathBuf>,
    num_jobs: usize,
    cfg: Config,
    walk_errors: WalkErrors,
) -> WalkFailures {
    let read_failures = Arc::clone(&cfg.read_failures);
    let write_failures = Arc::clone(&cfg.write_failures);
    let ordered_stdout = cfg.ordered_stdout.clone();
    let outcome = ConcurrentRunner::new(num_jobs, act_on_file)
        // `expand_seed_paths` classified every entry from the walker's
        // `dirent`, so the runner's own `is_file()` check is a second
        // `stat` for a question already answered (#1114). A path that
        // has since vanished still surfaces through `read_failures`,
        // which is where the walk reports unreadable inputs anyway.
        .without_path_verification()
        .run(cfg, FilesData { paths });
    // Flush *before* the runner's own failure is reported: a panicked
    // worker is exactly the case that leaves a slot unreleased, and
    // dying first would drop the documents queued behind it (#1303).
    flush_ordered_stdout(ordered_stdout.as_deref(), &write_failures);
    outcome.unwrap_or_else(|e| die(format_args!("{e:?}")));
    WalkFailures {
        walk: walk_errors,
        read: read_failures.load(Ordering::Relaxed),
        write: write_failures.load(Ordering::Relaxed),
    }
}

/// Write anything the streaming-stdout reorder buffer is still holding
/// now that every worker has joined (#1303).
///
/// Empty on any ordinary run — every dispatched file releases its slot,
/// document or not — so this only fires for a slot a panicked worker
/// never released, and emits the documents queued behind it late rather
/// than dropping them. The failure is folded into the walk's own
/// write tally so it exits 1 through the same guard a per-file write
/// failure does, `BrokenPipe` excepted for the usual `| head` reason.
fn flush_ordered_stdout(
    ordered: Option<&ordered_stdout::OrderedStdout>,
    write_failures: &AtomicUsize,
) {
    if let Some(ordered) = ordered
        && let Err(err) = ordered.flush_remaining()
        && err.kind() != ErrorKind::BrokenPipe
    {
        warn(format_args!("failed to write buffered output: {err}"));
        write_failures.fetch_add(1, Ordering::Relaxed);
    }
}

/// How many inputs a walk failed on, split by which end gave way. All
/// three are fatal; counted apart so the summary names the actionable
/// cause.
///
/// `walk` is the traversal-side loss (#1131) and is upstream of the
/// other two: an entry the walker could not read never reaches a
/// worker, so it can neither fail to be read nor fail to be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WalkFailures {
    walk: WalkErrors,
    read: usize,
    write: usize,
}

/// Which end of a walk gave way, and how many inputs it cost.
///
/// Exists so the priority order between the three is written once.
/// Both consumers — the exit-code guard and `bca diff --since` — must
/// report the *same* end for the same walk, or the two paths describe
/// one failure differently; they used to re-spell the ladder each, and
/// #1131 would have made that a three-way duplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkFailure {
    Walk(usize),
    Read(usize),
    Write(usize),
}

impl WalkFailures {
    /// The most upstream failure, or `None` when the walk was complete.
    ///
    /// Traversal first: an entry the walker could not read never became
    /// a file to read or a document to write, so naming it names the
    /// cause. Reads before writes for the same reason — a file that
    /// could not be read has no output to write either.
    fn first(self) -> Option<WalkFailure> {
        if self.walk.count() > 0 {
            Some(WalkFailure::Walk(self.walk.count()))
        } else if self.read > 0 {
            Some(WalkFailure::Read(self.read))
        } else if self.write > 0 {
            Some(WalkFailure::Write(self.write))
        } else {
            None
        }
    }
}

impl WalkFailure {
    /// The one-line stderr summary for this failure.
    fn summary(self) -> String {
        match self {
            Self::Walk(count) => walk_failure_summary(count),
            Self::Read(count) => read_failure_summary(count),
            Self::Write(count) => write_failure_summary(count),
        }
    }

    /// The same failure as a `bca diff --since` error, tagged with the
    /// tree it came from. `DiffError` renders each variant through the
    /// matching summary above, so the two surfaces cannot drift.
    fn into_diff_error(self, side: DiffSide) -> metric_diff::DiffError {
        match self {
            Self::Walk(count) => metric_diff::DiffError::UnwalkableInputs { side, count },
            Self::Read(count) => metric_diff::DiffError::UnreadableInputs { side, count },
            Self::Write(count) => metric_diff::DiffError::UnwritableOutputs { side, count },
        }
    }
}

/// Resolve the seeds and process the file set concurrently, returning
/// the failure tallies. The seed-expanding counterpart to
/// [`run_walk_resolved_tallying`], and the only one of the two that can
/// observe a traversal error — `run_walk_resolved_tallying` is handed a
/// file list somebody else expanded.
fn run_walk_tallying(globals: GlobalOpts, mut cfg: Config) -> WalkFailures {
    let (resolved, num_jobs) = resolve_walk_files(globals);
    cfg.explicit_seeds = Arc::new(resolved.explicit_files);
    // Install the reorder buffer for the destination that needs one,
    // recording the resolved list as the emission order before the
    // runner hands out its first path — each worker resolves its own
    // slot from that map (#1303).
    if cfg.streams_documents_to_stdout() {
        cfg.ordered_stdout = ordered_stdout::OrderedStdout::new(&resolved.files).map(Arc::new);
    }
    run_walk_resolved_tallying(resolved.files, num_jobs, cfg, resolved.walk_errors)
}

/// Summarize a walk that could not read every input file. Shared by the
/// post-walk guard below and by `bca diff --since`, which surfaces the
/// same wording through [`metric_diff::DiffError`] (#1098).
pub(crate) fn read_failure_summary(count: usize) -> String {
    let noun = if count == 1 { "file" } else { "files" };
    format!(
        "{count} input {noun} could not be read (see the errors above); \
         refusing to trust a partially analysed input set"
    )
}

/// Summarize a walk whose *traversal* could not read every entry —
/// typically a directory the process cannot list (#1131). Shaped like
/// [`read_failure_summary`], but a separate wording because the loss is
/// a different one: a whole subtree never entered the resolved file set,
/// so the count is of unreadable entries, not of files.
pub(crate) fn walk_failure_summary(count: usize) -> String {
    let noun = if count == 1 { "entry" } else { "entries" };
    format!(
        "{count} directory {noun} could not be read (see the warnings above); \
         refusing to trust a partially walked input set"
    )
}

/// The write-side counterpart to [`read_failure_summary`].
pub(crate) fn write_failure_summary(count: usize) -> String {
    let noun = if count == 1 { "file" } else { "files" };
    format!(
        "{count} output {noun} could not be written (see the errors above); \
         refusing to report success for an incomplete result"
    )
}

/// Exit 1 when the walk could not read every entry it traversed, could
/// not read every input file, or could not write every output document.
///
/// A command that analysed less than its input has no complete result to
/// report, and the omission is invisible in the output: a missing file
/// silently shrinks a metrics document, a report, a node count, or a
/// `diff` side. `EXIT_TOOL_ERROR` is documented to cover unreadable
/// input, so every walking subcommand fails the same way `check` does
/// (#1060, #1098) rather than only when the run produced nothing.
///
/// The traversal side is the same argument one directory level up
/// (#1131). A directory the process cannot list drops its whole subtree
/// before any file is selected, so the per-file tally stays zero and the
/// run reported success — `bca check` most damagingly, since a gate
/// reporting clean on a tree it could not read is indistinguishable from
/// a gate that passed.
///
/// The write side is the same argument backwards: an unwritable
/// `--output-dir` printed one error per file and still exited 0, which
/// a CI script reads as a clean run over a missing output tree.
///
/// The consumer has already printed one `error processing <path>: …`
/// line per failure, so this is a summary, not the first notice. It runs
/// before any aggregate document is written, so no artifact that looks
/// complete reaches disk.
///
/// Deliberately unprefixed by a subcommand name: `bca init` scaffolds
/// its baseline through `run_check`, so naming a subcommand here would
/// misattribute the failure to a command the user never ran.
fn enforce_complete_walk(failures: WalkFailures) {
    if let Some(failure) = failures.first() {
        die(failure.summary());
    }
}

/// Resolve the seeds and process the file set concurrently. The common
/// walk entry point; callers that also need the resolved file list (only
/// `bca preproc`, for `#include` grouping) use [`run_walk_collecting`].
///
/// Exits 1 if any input file could not be read or any output document
/// could not be written, so anything assembled *after* this returns is
/// suppressed — while output already streamed *during* the walk is kept.
fn run_walk(globals: GlobalOpts, cfg: Config) {
    enforce_complete_walk(run_walk_tallying(globals, cfg));
}

/// Process an already-resolved terminal file list concurrently. Lets a
/// caller inspect the resolved set (e.g. `strip-comments --output`, which
/// rejects a multi-file match) before dispatching the workers, without
/// re-running the seed expansion.
///
/// Exits 1 on an unreadable input or unwritable output, as [`run_walk`].
///
/// `walk_errors` is the caller's own [`resolve_walk_files`] tally: this
/// entry point does not expand seeds, so it cannot observe a traversal
/// failure and must be handed one (#1131). Taking it as a parameter
/// rather than defaulting it is what stops a future caller from
/// resolving its own file list and silently dropping the count.
fn run_walk_resolved(paths: Vec<PathBuf>, num_jobs: usize, cfg: Config, walk_errors: WalkErrors) {
    enforce_complete_walk(run_walk_resolved_tallying(
        paths,
        num_jobs,
        cfg,
        walk_errors,
    ));
}

/// Like [`run_walk`], but returns the resolved terminal file list.
/// `bca preproc` needs it to group files by basename for cross-file
/// `#include` resolution after the analysis (#495); it is the only
/// caller that consumes the list, so only this variant clones it.
///
/// Exits 1 like [`run_walk`], so the list is only handed back for a
/// complete walk.
fn run_walk_collecting(globals: GlobalOpts, mut cfg: Config) -> Vec<PathBuf> {
    let (resolved, num_jobs) = resolve_walk_files(globals);
    cfg.explicit_seeds = Arc::new(resolved.explicit_files);
    let paths = resolved.files;
    enforce_complete_walk(run_walk_resolved_tallying(
        paths.clone(),
        num_jobs,
        cfg,
        resolved.walk_errors,
    ));
    paths
}

/// Analyze the source tree rooted at `root` with `globals` (whose
/// `paths` the caller has set to seeds *relative to* `root`, and whose
/// `include`/`exclude` carry the user's selection), returning the
/// resulting in-memory [`MetricSet`] keyed by each file's path relative
/// to `root`.
///
/// This is the metric-extraction half of `bca diff --since`: the
/// "before" side runs it against a tempdir holding the tree at a git
/// ref, the "after" side against the working tree (or an explicit
/// directory). Keying relative to `root` is what lets the two sides
/// pair on the same logical layout even though their absolute roots
/// differ (a `/tmp/…` extraction dir vs the repo).
///
/// The walk runs with the process CWD anchored at `root` (via the
/// drop-restoring [`with_cwd`] guard) and the caller's seeds expressed
/// relative to it, so each analyzed file's emitted name *is* its
/// root-relative path (`src/foo.rs`) with no absolute prefix to strip.
/// That name is the key — read off the serialized document, never
/// reconstructed from an output path — so it is byte-identical across
/// the two sides whenever the same logical file exists in both trees.
/// [`metric_diff::set_from_spaces`] and [`metric_diff::load_dir_set`]
/// share one keying helper precisely so the in-memory side here and the
/// on-disk side of `bca diff <old> <new>` cannot diverge on it.
///
/// `side` names this tree in the error a partially-readable walk raises
/// ("before" / "after"). Unreadable input is an error rather than a gap
/// here for a reason specific to diffing: a file missing from one side's
/// set is indistinguishable from a file the commit added or removed, so
/// tolerating the read failure yields a *wrong* comparison, not merely
/// an incomplete one (#1098). It is returned rather than exited on so
/// the caller's temp trees still unwind.
pub(crate) fn walk_metric_set(
    root: &Path,
    globals: GlobalOpts,
    side: DiffSide,
) -> Result<metric_diff::MetricSet, metric_diff::DiffError> {
    let action = Action::Metrics {
        format: Some(MetricsFormat::Json),
        pretty: false,
    };
    // Stream each file's space back over the aggregate channel and build
    // the set in memory (#1116). This used to run with
    // `output_dir: Some(temp)`, writing one JSON document per source
    // file and then re-walking, re-reading and re-parsing the whole
    // temp tree — while the `FuncSpace` trees were already in the
    // workers' hands. The format stays `Json` because the set's values
    // are `serde_json::Value`s built from the same `Serialize` impl.
    let (tx, rx) =
        crossbeam::channel::bounded(globals.num_jobs.resolve() * AGGREGATE_BACKLOG_PER_JOB);
    let cfg = Config {
        aggregate_tx: Some(tx),
        ..Config::new(action, &globals, None)
    };
    // Collect concurrently with the walk rather than draining afterwards.
    // Each tree is reduced to its metrics `Value` and dropped as it
    // arrives, so the peak holds the channel backlog instead of one
    // `FuncSpace` per file — and the conversion overlaps the walk instead
    // of running after it.
    let collector = std::thread::spawn(move || metric_diff::set_from_spaces(rx));

    // Walk with the process CWD anchored at `root` and the seeds
    // expressed relative to it (the caller passes `.`/`<subdir>`), so
    // each space's emitted name is the root-relative source path
    // (`src/foo.rs`) with no absolute prefix to strip. This is what
    // makes the "before" side (a
    // /tmp/… extraction) and the "after" side (the working tree or an
    // explicit dir) pair on the same keys despite different absolute
    // roots, without depending on `reanchor_seed`'s under-CWD rewrite.
    //
    // `bca diff` runs the walk to completion synchronously here — the
    // worker threads spawn and join inside this call — so the scoped
    // CWD swap cannot race another command's walk.
    let restore = with_cwd(root)?;
    let failures = run_walk_tallying(globals, cfg);
    drop(restore);

    // `run_walk_tallying` took `cfg` by value and has joined every
    // worker, so the sender is dropped and the collector's receiver sees
    // disconnect. Joined before the failure guards below so the thread is
    // always reaped, even on the error paths.
    let collected = collector.join();

    // Any incomplete walk is an error here rather than a gap: a file
    // missing from one side's set is indistinguishable from one the
    // commit added or removed, so tolerating the loss yields a *wrong*
    // comparison (#1098) — and an unlistable directory loses a whole
    // subtree at once (#1131).
    //
    // The write arm is expected to be unreachable now that nothing is
    // written per file, and is kept because `write_failures` is generic
    // walk machinery a future dispatch change could start incrementing.
    if let Some(failure) = failures.first() {
        return Err(failure.into_diff_error(side));
    }

    collected.map_err(|_| metric_diff::DiffError::CollectorPanicked { side })?
}

/// Un-collected `FuncSpace` trees allowed between the worker pool and
/// the `--since` collector, as a multiple of the pool width.
///
/// The bound exists to stop *unbounded* growth, not to tune the peak.
/// Draining after the walk rather than during it let the channel hold
/// one tree per file, which took `diff --since` over a 12,732-file tree
/// to 849 MB resident against 453 MB for the temp-JSON route it
/// replaced. Any finite bound fixes that; the value chosen barely moves
/// the result, because what remains at peak is the accumulated
/// `MetricSet`, not the backlog.
///
/// Measured on that tree at 16 workers, median of three:
///
/// | backlog | wall (min) | peak RSS |
/// |---------|-----------|----------|
/// | unbounded | 7.62 s | 849 MB |
/// | 16x (this value) | 5.13 s | 591 MB |
/// | 4x | 6.55 s | 600 MB |
///
/// So a tighter bound costs wall time and saves nothing: it starts
/// throttling the pool while the peak stays put. Scaled per job rather
/// than fixed so a wider pool cannot be throttled by a constant sized
/// on a 16-core box.
const AGGREGATE_BACKLOG_PER_JOB: usize = 16;

const SUBCOMMANDS: &[&str] = &[
    "metrics",
    "ops",
    "vcs",
    "report",
    "dump",
    "find",
    "count",
    "functions",
    "strip-comments",
    "preproc",
    "list-metrics",
    "check",
    "init",
    "diff-baseline",
    "diff",
    "exemptions",
];

/// Decode the value of the output-format flag from a flat argv slice,
/// in any of its spellings: the canonical `--format <v>` /
/// `--format=<v>` / `-O <v>` / `-O<v>` (issue #513) and the deprecated
/// `--output-format <v>` / `--output-format=<v>` alias. Returns the
/// first match (callers pre-filter the slice to the legacy
/// invocation's tokens, so a single occurrence is the realistic
/// case).
fn parse_output_format_value(args: &[String]) -> Option<&str> {
    args.iter().enumerate().find_map(|(i, a)| {
        let s = a.as_str();
        if s == "-O" || s == "--output-format" || s == "--format" {
            args.get(i + 1).map(String::as_str)
        } else if let Some(rest) = s
            .strip_prefix("--output-format=")
            .or_else(|| s.strip_prefix("--format="))
        {
            Some(rest)
        } else {
            s.strip_prefix("-O").filter(|r| !r.is_empty())
        }
    })
}

/// Scan `args` for an output-format flag (any spelling — see
/// [`parse_output_format_value`]) carrying one of the moved offender
/// formats (any variant of [`AggregatedFormat`]) and build a migration
/// hint pointing at `bca check`. Returns `None` when no offender format
/// is found, so the caller can fall through to clap's own error.
fn offender_format_migration_hint(args: &[String]) -> Option<String> {
    let fmt =
        parse_output_format_value(args).filter(|f| AggregatedFormat::from_str(f, true).is_ok())?;
    Some(format!(
        "note: -O {fmt} moved to `bca check` in #235; offender formats are no longer accepted on `bca metrics` / `bca ops`.\n  bca metrics -O {fmt} ...  ->  bca check --threshold <metric>=<limit> --format {fmt} [--output FILE]\n  Run `bca check --help` for the threshold and format flags.\n"
    ))
}

/// If `argv` looks like an invocation of the pre-restructure CLI, return a
/// hint pointing the user at the new equivalent. Called only when clap
/// rejects the input, so the goal is to make the failure actionable.
///
/// The hint is best-effort and conservative: it triggers only on tokens
/// that are unambiguously legacy (action flags removed in the rewrite, or
/// `-O markdown` whose value no longer exists on `metrics`).
fn legacy_hint(argv: impl IntoIterator<Item = OsString>) -> Option<String> {
    let args: Vec<String> = argv
        .into_iter()
        .skip(1) // program name
        .filter_map(|s| s.into_string().ok())
        .collect();
    if args.is_empty() {
        return None;
    }

    // If the user invoked a known new-CLI subcommand, they're not on
    // the legacy CLI; stay quiet so we don't second-guess legitimate
    // args that happen to look like old flags (e.g. `find --dump`
    // where the user intended `--dump` as a positional node-type
    // value). The one exception is `bca metrics|ops --output-format
    // <offender>` — the offender formats moved to `bca check`
    // (issue #235) and the user still needs a one-line pointer at
    // the new home.
    if let Some(sub) = args.iter().find(|a| SUBCOMMANDS.contains(&a.as_str())) {
        if matches!(sub.as_str(), "metrics" | "ops")
            && let Some(hint) = offender_format_migration_hint(&args)
        {
            return Some(hint);
        }
        return None;
    }

    // Action flags removed by the rewrite. Each one is unambiguously legacy.
    let action_map: &[(&str, &str)] = &[
        ("--metrics", "bca metrics"),
        ("-m", "bca metrics"),
        ("--ops", "bca ops"),
        ("--dump", "bca dump"),
        ("-d", "bca dump"),
        ("--comments", "bca strip-comments [--in-place]"),
        ("--function", "bca functions"),
        ("-F", "bca functions"),
        ("--find", "bca find -t <NODE> [-t <NODE>...] [PATHS]..."),
        ("-f", "bca find -t <NODE> [-t <NODE>...] [PATHS]..."),
        ("--count", "bca count -t <NODE> [-t <NODE>...] [PATHS]..."),
        ("-C", "bca count -t <NODE> [-t <NODE>...] [PATHS]..."),
        ("--list-metrics", "bca list-metrics [names|descriptions]"),
        (
            "--preproc",
            "bca preproc -o OUT.json  (or --preproc-data on consumers)",
        ),
    ];

    let mut lines: Vec<String> = Vec::new();
    let mut saw_legacy_action = false;

    for arg in &args {
        let head = arg.split('=').next().unwrap_or(arg);
        if let Some((_, replacement)) = action_map.iter().find(|(old, _)| *old == head) {
            saw_legacy_action = true;
            lines.push(format!("  {head}  ->  {replacement}"));
        }
    }

    // -O markdown / --output-format markdown is the canonical legacy form
    // for the aggregated report. `markdown` is no longer a valid metrics
    // format value, so seeing it here is unambiguous.
    let format_value = parse_output_format_value(&args);
    if format_value == Some("markdown") {
        saw_legacy_action = true;
        lines.push(String::from(
            "  -O markdown  ->  bca report markdown|html [--top N] [--strip-prefix P]",
        ));
    } else if let Some(fmt) = format_value
        && saw_legacy_action
    {
        // Only suggest a metrics-format mapping when we already confirmed
        // this is a legacy invocation; otherwise `-O json` survives in the
        // new CLI and we shouldn't second-guess it.
        lines.push(format!("  -O {fmt}  ->  bca metrics -O {fmt}"));
    }

    if !saw_legacy_action {
        return None;
    }

    let mut hint = String::from(
        "note: the CLI was restructured into subcommands. See migration.md for the full mapping.\n",
    );
    for line in &lines {
        hint.push_str(line);
        hint.push('\n');
    }
    hint.push_str("  Run `bca --help` for the new command list.\n");
    Some(hint)
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
#[path = "lib_tests.rs"]
mod tests;
