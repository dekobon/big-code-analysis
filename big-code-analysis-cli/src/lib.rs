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
mod path_io;
mod provenance;
mod qualified_name;
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
use thresholds::{
    ParsedThresholds, ThresholdConfig, ThresholdSet, Violation, parse_cli_threshold,
    parse_fail_above, split_thresholds_table,
};

use big_code_analysis::LANG;
use big_code_analysis::{
    ConcurrentRunner, CountCollector, FilesData, MetricsOptions, NumJobs, PreprocResults,
    SuppressionPolicy,
};
use big_code_analysis::{FuncSpace, Ops, get_from_ext, read_file};

/// `expect` message used at every `action::<_>` call site inside the
/// extracted `dispatch` module. Kept in `lib.rs` so any module that
/// terminates with `expect(FEATURES_PINNED)` can import the same
/// string and the invariant lives in one place.
///
/// The CLI pins `big-code-analysis` with `features = ["all-languages"]`,
/// so a `LANG` value that reached this point must be enabled at compile
/// time. Any future caller that loosens the feature pin must change
/// this invariant explicitly.
pub(crate) const FEATURES_PINNED: &str =
    "CLI pins big-code-analysis features = [\"all-languages\"]";

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
    /// per-file inline. Wrapped in `Mutex` because `mpsc::Sender` is
    /// `Send` but not `Sync`.
    aggregate_tx: Option<Mutex<std::sync::mpsc::Sender<AggregateItem>>>,
    language: Option<LANG>,
    line_start: Option<usize>,
    line_end: Option<usize>,
    preproc_lock: Option<Arc<Mutex<PreprocResults>>>,
    preproc: Option<Arc<PreprocResults>>,
    count_lock: Option<CountCollector>,
    /// Sender for streaming `FunctionSummary` records when running `report`.
    /// Wrapped in `Mutex` because `mpsc::Sender` is `Send` but not `Sync`.
    markdown_tx: Option<Mutex<std::sync::mpsc::Sender<FunctionSummary>>>,
    /// Sender for streaming each file's `(absolute path, cyclomatic sum)`
    /// when running `report --vcs`, so the change-history section can join
    /// the same hotspot score (`complexity × recent churn`) that
    /// `bca metrics --vcs` attaches per file (issue #615). The absolute
    /// path is canonicalized against the index work-tree downstream — the
    /// identical match `vcs_command::inject` uses — so the join is correct
    /// regardless of `--strip-prefix` or the walk-root spelling. `None`
    /// for every flow other than `report --vcs`. Wrapped in `Mutex` for the
    /// same reason as `markdown_tx`.
    report_hotspot_tx: Option<Mutex<std::sync::mpsc::Sender<(PathBuf, f64)>>>,
    /// Path prefix stripped from file paths in the markdown report.
    strip_prefix: String,
    /// Pre-resolved thresholds for `Action::Check`. `None` for every
    /// other action.
    threshold_set: Option<Arc<ThresholdSet>>,
    /// Sender for streaming [`Violation`] records when running `check`.
    /// Wrapped in `Mutex` for the same reason as `markdown_tx`.
    check_tx: Option<Mutex<std::sync::mpsc::Sender<Violation>>>,
    /// Sender for streaming per-file suppression-marker batches when
    /// running `exemptions`. Wrapped in `Mutex` for the same reason as
    /// `markdown_tx`.
    exemptions_tx: Option<Mutex<std::sync::mpsc::Sender<exemptions::FileMarkers>>>,
    /// Counts how many files survived expansion and glob filtering and
    /// were actually dispatched to `act_on_file`. `Action::Check` reads
    /// this after the walk to distinguish "all clean" (counter > 0,
    /// no violations) from "no files matched" (counter == 0), so a
    /// typo in `--paths` does not silently pass CI.
    files_dispatched: Option<Arc<AtomicUsize>>,
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
            threshold_set: None,
            check_tx: None,
            exemptions_tx: None,
            files_dispatched: None,
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
fn run_walk_resolved_tallying(paths: Vec<PathBuf>, num_jobs: usize, cfg: Config) -> WalkFailures {
    let read_failures = Arc::clone(&cfg.read_failures);
    let write_failures = Arc::clone(&cfg.write_failures);
    ConcurrentRunner::new(num_jobs, act_on_file)
        .run(cfg, FilesData { paths })
        .unwrap_or_else(|e| die(format_args!("{e:?}")));
    WalkFailures {
        read: read_failures.load(Ordering::Relaxed),
        write: write_failures.load(Ordering::Relaxed),
    }
}

/// How many files a walk failed on, split by which end gave way. Both
/// are fatal; counted apart so the summary names the actionable cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WalkFailures {
    read: usize,
    write: usize,
}

/// Resolve the seeds and process the file set concurrently, returning
/// the read-failure tally. The seed-expanding counterpart to
/// [`run_walk_resolved_tallying`].
fn run_walk_tallying(globals: GlobalOpts, mut cfg: Config) -> WalkFailures {
    let (resolved, num_jobs) = resolve_walk_files(globals);
    cfg.explicit_seeds = Arc::new(resolved.explicit_files);
    run_walk_resolved_tallying(resolved.files, num_jobs, cfg)
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

/// The write-side counterpart to [`read_failure_summary`].
pub(crate) fn write_failure_summary(count: usize) -> String {
    let noun = if count == 1 { "file" } else { "files" };
    format!(
        "{count} output {noun} could not be written (see the errors above); \
         refusing to report success for an incomplete result"
    )
}

/// Exit 1 when the walk could not read every input file, or could not
/// write every output document.
///
/// A command that analysed less than its input has no complete result to
/// report, and the omission is invisible in the output: a missing file
/// silently shrinks a metrics document, a report, a node count, or a
/// `diff` side. `EXIT_TOOL_ERROR` is documented to cover unreadable
/// input, so every walking subcommand fails the same way `check` does
/// (#1060, #1098) rather than only when the run produced nothing.
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
    // Reads first: when a file could not be read there is no output to
    // write either, so naming the read is naming the cause.
    if failures.read > 0 {
        die(read_failure_summary(failures.read));
    }
    if failures.write > 0 {
        die(write_failure_summary(failures.write));
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
fn run_walk_resolved(paths: Vec<PathBuf>, num_jobs: usize, cfg: Config) {
    enforce_complete_walk(run_walk_resolved_tallying(paths, num_jobs, cfg));
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
    enforce_complete_walk(run_walk_resolved_tallying(paths.clone(), num_jobs, cfg));
    paths
}

/// Analyze the source tree rooted at `root` with `globals` (whose
/// `paths` the caller has set to seeds *relative to* `root`, and whose
/// `include`/`exclude` carry the user's selection), writing per-file
/// JSON into `json_out_dir`, and return the resulting in-memory
/// [`MetricSet`] keyed by each file's path relative to `root`.
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
/// relative to it, so the JSON writer emits each per-file document under
/// `json_out_dir` named by the root-relative source path
/// (`src/foo.rs.json`). The key is then just that path relative to
/// `json_out_dir` — byte-identical across the two sides whenever the
/// same logical file exists in both trees.
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
    json_out_dir: &Path,
    side: DiffSide,
) -> Result<metric_diff::MetricSet, metric_diff::DiffError> {
    let action = Action::Metrics {
        format: Some(MetricsFormat::Json),
        pretty: false,
    };
    let cfg = Config {
        // `bca diff --since` writes a per-file JSON tree it later reloads;
        // that is the directory-tree mode, which now lives on `output_dir`
        // (#669) rather than `output` (a single aggregate file).
        output_dir: Some(json_out_dir.to_path_buf()),
        ..Config::new(action, &globals, None)
    };

    // Walk with the process CWD anchored at `root` and the seeds
    // expressed relative to it (the caller passes `.`/`<subdir>`),
    // so the JSON writer emits files under `json_out_dir` named by the
    // root-relative source path (`src/foo.rs.json`) with no absolute
    // prefix to strip. This is what makes the "before" side (a
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
    if failures.read > 0 {
        return Err(metric_diff::DiffError::UnreadableInputs {
            side,
            count: failures.read,
        });
    }
    if failures.write > 0 {
        return Err(metric_diff::DiffError::UnwritableOutputs {
            side,
            count: failures.write,
        });
    }

    // The JSON writer emitted one document per source file under
    // `json_out_dir`, keyed by the root-relative source path — the same
    // shape `bca diff`'s directory inputs use, so reuse its loader.
    metric_diff::load_dir_set(json_out_dir)
}

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
