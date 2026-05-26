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
    // `run` panics on a handful of provably-unreachable invariants
    // (mutex poisoning where every worker thread has joined, channel
    // sends after run_walk returns). Each one is documented at the
    // call site with an `expect` reason — surfacing them in a `# Panics`
    // section on the entry point adds noise without adding signal.
    clippy::missing_panics_doc
)]
mod baseline;
mod check_format;
mod dispatch;
mod format_util;
mod formats;
mod html_report;
mod markdown_report;
mod metric_catalog;
mod thresholds;

use dispatch::act_on_file;

use std::collections::{BTreeMap, HashMap, hash_map};
use std::ffi::OsString;
use std::fmt::Display;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::available_parallelism;

use clap::{Args, Parser, Subcommand, ValueEnum};
use globset::{Glob, GlobSet, GlobSetBuilder};

use baseline::{Baseline, Coverage};
use check_format::{AggregatedFormat, violation_to_offender};
use format_util::MetricScalar;
use formats::{CBOR_STDOUT_ERROR, MetricsDispatch, MetricsFormat, ReportFormat};
use html_report::generate_html_report;
use markdown_report::{FunctionSummary, generate_report};
use metric_catalog::{ListMetricsMode, write_metrics};
use thresholds::{
    ThresholdConfig, ThresholdSet, Violation, parse_cli_threshold, render_violation_line,
};

use big_code_analysis::LANG;
use big_code_analysis::{
    ConcurrentRunner, Count, FilesData, MetricsOptions, PreprocResults, SuppressionPolicy,
};
use big_code_analysis::{fix_includes, get_from_ext, read_file, write_file};

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

fn die(msg: impl Display) -> ! {
    eprintln!("Error: {msg}");
    process::exit(1);
}

/// Die with `failed to <verb> <path>: <err>`. Centralizes the most common
/// I/O error shape: open/read/parse/write of a user-supplied path that
/// failed with an error implementing `Display`.
fn die_io(verb: &str, path: &Path, err: impl Display) -> ! {
    die(format_args!("failed to {verb} {}: {err}", path.display()))
}

/// Write `bytes` to stdout, tolerating `BrokenPipe` (the typical case when
/// the consumer is `head`, `less`, etc.) and `die`ing on anything else.
fn write_stdout_or_die(bytes: &[u8]) {
    if let Err(e) = std::io::stdout().lock().write_all(bytes)
        && e.kind() != ErrorKind::BrokenPipe
    {
        die(e);
    }
}

/// Analyze source code.
//
// Single-line doc-comment kept in sync with the `about = "..."` attribute
// below — clap promotes a doc-comment to `long_about`, which clap-mangen
// renders into the manpage DESCRIPTION. The embedder contract for this
// crate (which is why `Cli` is `pub` at all) lives in the crate-level
// `//!` docs above, not here.
#[derive(Parser, Debug)]
#[clap(
    name = "bca",
    version,
    author,
    about = "Analyze source code.",
    subcommand_required = true,
    arg_required_else_help = true,
    after_help = "Migrating from the flag-style CLI? See the migration guide:\n  big-code-analysis-book/src/migration.md"
)]
pub struct Cli {
    #[clap(flatten)]
    globals: GlobalOpts,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Debug, Default)]
struct GlobalOpts {
    /// Input files or directories to analyze.
    #[clap(long, short, value_parser, global = true)]
    paths: Vec<PathBuf>,
    /// Glob to include files.
    #[clap(long, short = 'I', num_args(0..), global = true)]
    include: Vec<String>,
    /// Glob to exclude files.
    #[clap(long, short = 'X', num_args(0..), global = true)]
    exclude: Vec<String>,
    /// Number of jobs.
    #[clap(long, short = 'j', global = true)]
    num_jobs: Option<usize>,
    /// Force a language type instead of inferring from extension.
    #[clap(long, short = 'l', global = true)]
    language_type: Option<String>,
    /// Line start (used by `dump` and `find`).
    #[clap(long = "ls", global = true)]
    line_start: Option<usize>,
    /// Line end (used by `dump` and `find`).
    #[clap(long = "le", global = true)]
    line_end: Option<usize>,
    /// Print warnings (skipped files, unrecognized languages).
    #[clap(long, short, global = true)]
    warning: bool,
    /// Disable auto-skip of files marked as generated (e.g. `@generated`,
    /// `DO NOT EDIT`, `GENERATED CODE` near the top). By default the CLI
    /// skips such files so generated bindings do not skew metrics.
    #[clap(long, global = true)]
    no_skip_generated: bool,
    /// Log a "skipped (generated): <path>" line to stderr for each file
    /// auto-skipped by the generated-code detector. Useful for auditing
    /// which files were excluded.
    #[clap(long, global = true)]
    report_skipped: bool,
    /// Existing preprocessor-data JSON to consume during C/C++ analysis.
    /// Use `bca preproc` to produce one.
    #[clap(long, value_parser, global = true)]
    preproc_data: Option<PathBuf>,
    /// Read newline-separated input paths from a file. Use `-` to read
    /// from stdin. Combined as a union with any `--paths` values; globs
    /// still apply. Blank lines are skipped; `#` is treated as a path
    /// character (not a comment). To pass a file literally named `-`,
    /// use `./-`.
    #[clap(long = "paths-from", value_parser, global = true)]
    paths_from: Option<PathBuf>,
    /// Read additional `--exclude` glob patterns from a file (one per
    /// line, `.gitignore`-style). Blank lines and lines whose first
    /// non-whitespace character is `#` are skipped. Use `-` to read
    /// from stdin; to pass a file literally named `-`, use `./-`.
    /// Patterns are unioned with any `--exclude` values into a single
    /// deny-set; order does not matter. Convention is a `.bcaignore`
    /// at the repo root, mirroring `.gitignore` / `.dockerignore`.
    #[clap(long = "exclude-from", value_parser, global = true)]
    exclude_from: Option<PathBuf>,
    /// Disable `.gitignore` / `.ignore` / global gitignore awareness
    /// when expanding directory seeds. Explicit file paths are always
    /// honored regardless of this flag.
    #[clap(long = "no-ignore", global = true)]
    no_ignore: bool,
    /// Exclude inline test code from metric computation. Currently
    /// applies to Rust only (skips `#[test]`, `#[cfg(test)]`,
    /// `#[tokio::test]`, `#[rstest]`, `#![cfg(test)]` items and
    /// their subtrees). Default is off — every node is counted, so
    /// numbers match the pre-#182 behaviour byte-for-byte. Languages
    /// without a `Checker::should_skip_subtree` override ignore this
    /// flag.
    #[clap(long = "exclude-tests", global = true)]
    exclude_tests: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compute per-file metrics and emit them in a structured format.
    Metrics(StructuredArgs),
    /// Extract per-file operands and operators.
    Ops(StructuredArgs),
    /// Generate an aggregated report across the analyzed source.
    Report(ReportArgs),
    /// Dump the AST to stdout.
    Dump,
    /// Find nodes of one or more types.
    Find(NodesArgs),
    /// Count nodes of one or more types.
    Count(NodesArgs),
    /// List functions/methods and their spans.
    Functions,
    /// Remove comments from source files.
    StripComments(StripCommentsArgs),
    /// Generate preprocessor-data JSON for C/C++ analysis.
    Preproc(PreprocArgs),
    /// List the metrics this tool can compute and exit.
    ListMetrics(ListMetricsArgs),
    /// Check per-function metrics against thresholds. Exits 2 when any
    /// threshold is exceeded; reserve exit 1 for tool errors so CI can
    /// distinguish "metric regression" from "tool crashed".
    Check(CheckArgs),
}

/// Shared shape for `metrics` and `ops`: same format set, same output
/// semantics (directory of per-file emissions; stdout if omitted).
#[derive(Args, Debug)]
struct StructuredArgs {
    /// Output format.
    #[clap(long, short = 'O', value_enum)]
    output_format: Option<MetricsFormat>,
    /// Output directory. Filenames mirror input paths plus the format
    /// extension. Stdout if omitted (CBOR requires this flag).
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Pretty-print JSON / TOML output.
    #[clap(long)]
    pretty: bool,
}

#[derive(Args, Debug)]
struct ReportArgs {
    /// Report format.
    #[clap(value_enum)]
    format: ReportFormat,
    /// Output file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Maximum number of entries per hotspot table.
    #[clap(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..))]
    top: u32,
    /// Path prefix to strip from displayed file paths.
    #[clap(long, default_value = "")]
    strip_prefix: String,
}

#[derive(Args, Debug)]
struct NodesArgs {
    /// Node-type names. Pass one or more, space-separated.
    #[clap(required = true, num_args = 1..)]
    nodes: Vec<String>,
}

#[derive(Args, Debug)]
struct StripCommentsArgs {
    /// Rewrite each input file in place instead of writing to stdout.
    #[clap(long)]
    in_place: bool,
}

#[derive(Args, Debug)]
struct PreprocArgs {
    /// Output JSON file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct CheckArgs {
    /// Threshold expressed as `<metric>=<limit>`. Repeatable. Metric
    /// names match `bca list-metrics`; sub-metrics use a dotted form
    /// (e.g. `loc.lloc`, `halstead.volume`). CLI flags override values
    /// from `--config`. Limits must be finite and non-negative; `0` is
    /// allowed and means "no value permitted".
    #[clap(long = "threshold", value_parser = parse_cli_threshold)]
    thresholds: Vec<(String, f64)>,
    /// Path to a TOML config with a `[thresholds]` table:
    ///
    /// ```toml
    /// [thresholds]
    /// cyclomatic = 15
    /// "loc.lloc" = 200
    /// ```
    #[clap(long, value_parser)]
    config: Option<PathBuf>,
    /// Print offenders to stderr but exit 0 even when thresholds are
    /// exceeded. Useful while adopting baselines without flipping CI red.
    /// Default: exit 2 when any threshold is exceeded.
    #[clap(long = "no-fail")]
    no_fail: bool,
    /// Ignore in-source suppression markers (`bca: suppress`,
    /// `#lizard forgives`, etc.). Every threshold violation is
    /// reported regardless of comment-based silencers. CI auditors
    /// pass this to see the raw, un-silenced offender list.
    #[clap(long = "no-suppress")]
    no_suppress: bool,
    /// CI/IDE document format for offender records (Checkstyle 4.3 XML,
    /// SARIF 2.1.0 JSON, clang/GCC warning lines, MSVC warning lines).
    /// When omitted, only the human-readable stderr stream is emitted;
    /// the exit-code contract is unaffected.
    #[clap(long = "output-format", short = 'O', value_enum)]
    output_format: Option<AggregatedFormat>,
    /// File path for the aggregated offender document. Stdout if omitted.
    /// Only meaningful together with `--output-format`. Parent
    /// directories are created on demand.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Filter known offenders listed in this TOML baseline. A baselined
    /// function whose metric value has not worsened is suppressed; a
    /// worsened value (or any new offender) still fails. See the
    /// "Baselines" recipe in the book for the full adoption flow.
    #[clap(long = "baseline", value_parser, conflicts_with = "write_baseline")]
    baseline: Option<PathBuf>,
    /// Walk the tree and write the current offender set to this path
    /// instead of failing. The resulting file pins today's metric
    /// values as the baseline; subsequent `--baseline <path>` runs
    /// ratchet down from there. Conflicts with `--baseline`,
    /// `--output-format`, and `--output` — the baseline file is the
    /// output.
    #[clap(
        long = "write-baseline",
        value_parser,
        conflicts_with_all = ["baseline", "output_format", "output"],
    )]
    write_baseline: Option<PathBuf>,
    /// Skip the trailing per-file rollup footer. The footer groups
    /// violations by file and cites the single worst-ratio metric per
    /// file. Pass this when downstream tooling grep-pipes the stderr
    /// stream and would be confused by the trailing summary block.
    /// Default: footer enabled.
    #[clap(long = "no-summary")]
    no_summary: bool,
}

#[derive(Args, Debug)]
struct ListMetricsArgs {
    /// What to print: `names` (one per line) or `descriptions`
    /// (name + one-line summary).
    #[clap(value_enum, default_value_t = ListMetricsMode::Names)]
    mode: ListMetricsMode,
}

/// What `act_on_file` should do per file. Drives the inner dispatch and
/// replaces the prior cluster of mutually-exclusive bool flags.
#[derive(Debug)]
enum Action {
    Dump,
    Metrics {
        format: Option<MetricsFormat>,
        pretty: bool,
    },
    Ops {
        format: Option<MetricsFormat>,
        pretty: bool,
    },
    StripComments {
        in_place: bool,
    },
    Functions,
    Find(Arc<[String]>),
    Count(Arc<[String]>),
    /// Same walk as `Metrics`, but taps each space tree to stream
    /// `FunctionSummary` records for the post-walk aggregator.
    Report,
    /// Walks source to accumulate preprocessor data (no per-file output).
    PreprocProduce,
    /// Walks source and streams threshold violations to a channel.
    Check,
}

#[derive(Debug)]
struct Config {
    action: Action,
    output: Option<PathBuf>,
    language: Option<LANG>,
    line_start: Option<usize>,
    line_end: Option<usize>,
    preproc_lock: Option<Arc<Mutex<PreprocResults>>>,
    preproc: Option<Arc<PreprocResults>>,
    count_lock: Option<Arc<Mutex<Count>>>,
    /// Sender for streaming `FunctionSummary` records when running `report`.
    /// Wrapped in `Mutex` because `mpsc::Sender` is `Send` but not `Sync`.
    markdown_tx: Option<Mutex<std::sync::mpsc::Sender<FunctionSummary>>>,
    /// Path prefix stripped from file paths in the markdown report.
    strip_prefix: String,
    /// Pre-resolved thresholds for `Action::Check`. `None` for every
    /// other action.
    threshold_set: Option<Arc<ThresholdSet>>,
    /// Sender for streaming [`Violation`] records when running `check`.
    /// Wrapped in `Mutex` for the same reason as `markdown_tx`.
    check_tx: Option<Mutex<std::sync::mpsc::Sender<Violation>>>,
    /// Counts how many files survived expansion and glob filtering and
    /// were actually dispatched to `act_on_file`. `Action::Check` reads
    /// this after the walk to distinguish "all clean" (counter > 0,
    /// no violations) from "no files matched" (counter == 0), so a
    /// typo in `--paths` does not silently pass CI.
    files_dispatched: Option<Arc<AtomicUsize>>,
    /// Whether to honor or ignore in-source suppression markers when
    /// emitting threshold violations. Only meaningful for
    /// `Action::Check`; the field is defaulted to `Honor` for every
    /// other action so the new code path is invisible to existing
    /// flows. Flipped to `Ignore` by `--no-suppress`.
    suppression_policy: SuppressionPolicy,
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
}

impl Config {
    /// Build a `Config` for `action`, populating the fields every command
    /// shares from `globals`. Per-command extras (`output`, `count_lock`,
    /// `markdown_tx`, `strip_prefix`) are set on the returned value at the
    /// call site.
    fn new(action: Action, globals: &GlobalOpts, preproc: Option<Arc<PreprocResults>>) -> Self {
        let language = resolve_language(globals.language_type.as_deref(), &action);
        Self {
            action,
            output: None,
            language,
            line_start: globals.line_start,
            line_end: globals.line_end,
            preproc_lock: None,
            preproc,
            count_lock: None,
            markdown_tx: None,
            strip_prefix: String::new(),
            threshold_set: None,
            check_tx: None,
            files_dispatched: None,
            suppression_policy: SuppressionPolicy::Honor,
            warning: globals.warning,
            skip_generated: !globals.no_skip_generated,
            report_skipped: globals.report_skipped,
            exclude_tests: globals.exclude_tests,
        }
    }

    /// Project this `Config` onto the library's `MetricsOptions`
    /// surface. Centralising the projection here means new metric
    /// options land in one place instead of being duplicated across
    /// every `act_on_file` arm that drives a metric computation.
    #[inline]
    fn metrics_options(&self) -> MetricsOptions {
        MetricsOptions::default().with_exclude_tests(self.exclude_tests)
    }
}

fn mk_globset(elems: Vec<String>) -> Result<GlobSet, String> {
    if elems.is_empty() {
        return Ok(GlobSet::empty());
    }

    let mut globset = GlobSetBuilder::new();
    for e in &elems {
        if e.is_empty() {
            continue;
        }
        globset.add(Glob::new(e).map_err(|err| format!("invalid glob pattern {e:?}: {err}"))?);
    }
    globset
        .build()
        .map_err(|err| format!("failed to build glob set: {err}"))
}


fn process_dir_path(all_files: &mut HashMap<String, Vec<PathBuf>>, path: &Path, cfg: &Config) {
    if !matches!(cfg.action, Action::PreprocProduce) {
        return;
    }
    let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let file_name = fname.to_string();
    match all_files.entry(file_name) {
        hash_map::Entry::Occupied(l) => {
            l.into_mut().push(path.to_path_buf());
        }
        hash_map::Entry::Vacant(p) => {
            p.insert(vec![path.to_path_buf()]);
        }
    }
}

fn resolve_language(typ: Option<&str>, action: &Action) -> Option<LANG> {
    // Force `Preproc` for the producer so `act_on_file`'s "skip
    // unrecognized" guard never fires — every walked file must reach the
    // dispatch where the producer runs its own Cpp check.
    if matches!(action, Action::PreprocProduce) {
        return Some(LANG::Preproc);
    }
    match typ.unwrap_or("") {
        "" => None,
        "ccomment" => Some(LANG::Ccomment),
        "preproc" => Some(LANG::Preproc),
        other => get_from_ext(other),
    }
}

fn resolve_num_jobs(requested: Option<usize>) -> usize {
    requested.map_or_else(
        || {
            std::cmp::max(
                2,
                available_parallelism()
                    .unwrap_or_else(|e| {
                        die(format_args!("could not get available parallelism: {e}"))
                    })
                    .get(),
            ) - 1
        },
        |num_jobs| std::cmp::max(2, num_jobs) - 1,
    )
}

/// Load existing preproc JSON for the consumer side. The producer side
/// (`bca preproc`) builds its own `Mutex<PreprocResults>` directly.
fn load_preproc_data(path: &Path) -> Arc<PreprocResults> {
    let data = read_file(path).unwrap_or_else(|e| die_io("read preproc data", path, e));
    let parsed = serde_json::from_slice::<PreprocResults>(&data)
        .unwrap_or_else(|e| die_io("parse preproc JSON from", path, e));
    Arc::new(parsed)
}

/// Read newline-separated paths from `src` (a path on disk or `-`
/// for stdin). Skips blank/whitespace-only lines; `#` is treated as a
/// path character, not a comment. Returns `Err(message)` on I/O
/// failure with the failing line number; the CLI caller translates
/// this into a `die` exit.
fn read_paths_from(src: &Path) -> Result<Vec<PathBuf>, String> {
    read_lines_from(src, "--paths-from", path_pattern_filter)
}

/// Retention policy for `--paths-from` lines: keep the trimmed
/// non-blank text as a literal path. `#` is a path character, not
/// a comment — paired with [`exclude_pattern_filter`] (the inverse
/// policy) by the unit tests so the two `read_*_from` wrappers
/// cannot accidentally swap predicates.
fn path_pattern_filter(trimmed: &str) -> Option<PathBuf> {
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// Read newline-separated `--exclude` glob patterns from `src` (a
/// path on disk or `-` for stdin). Blank lines and lines whose first
/// non-whitespace character is `#` (`.gitignore`-style comments) are
/// skipped; surrounding whitespace and any UTF-8 BOM on retained
/// lines are trimmed. Returns `Err(message)` on I/O failure with
/// the path / failing line; the CLI caller translates this into a
/// `die` exit.
fn read_exclude_patterns_from(src: &Path) -> Result<Vec<String>, String> {
    read_lines_from(src, "--exclude-from", exclude_pattern_filter)
}

/// Retention policy for `--exclude-from` lines: keep the trimmed
/// non-blank, non-`#`-prefixed text as an exclude pattern; otherwise
/// skip. Named so the unit tests can exercise the exact policy the
/// production reader applies instead of mirroring it.
fn exclude_pattern_filter(trimmed: &str) -> Option<String> {
    (!trimmed.is_empty() && !trimmed.starts_with('#')).then(|| trimmed.to_owned())
}

/// Open `src` (a path on disk or `-` for stdin), buffer it, and
/// hand each trimmed non-comment line to `map`. Items the closure
/// returns `Some` for are collected; `None` skips the line. `flag`
/// is the user-facing CLI flag name (e.g. `--paths-from`), included
/// in error messages so users can tell which input failed.
///
/// Returns `Err(message)` on file-open failure or per-line I/O
/// failure rather than calling `die` itself, so unit tests and
/// future non-CLI callers can recover. The CLI wrappers above
/// translate the `Err` into a `die` exit at their layer.
fn read_lines_from<T>(
    src: &Path,
    flag: &str,
    map: impl Fn(&str) -> Option<T>,
) -> Result<Vec<T>, String> {
    if src.as_os_str() == "-" {
        let label = format!("{flag} -");
        collect_lines(std::io::stdin().lock(), &label, map)
    } else {
        let label = format!("{flag} {}", src.display());
        let f = std::fs::File::open(src).map_err(|e| format!("{label}: {e}"))?;
        collect_lines(std::io::BufReader::new(f), &label, map)
    }
}

/// Drain `reader` line-by-line, trimming surrounding whitespace and
/// any UTF-8 BOMs (leading or trailing), then feeding each result
/// to `map`. Returns `Err(message)` on the first I/O failure, with
/// `label` and the failing line number embedded so the caller can
/// surface which input failed without further context.
///
/// BOM stripping is per-line rather than first-line-only: most
/// lines won't carry a BOM, and `\u{feff}` is not whitespace per
/// `char::is_whitespace`, so a BOM-prefixed pattern (e.g. an editor
/// that saved `.bcaignore` as UTF-8-with-BOM) would otherwise
/// become a literal glob starting with U+FEFF that matches no real
/// path — silently disabling the first exclude. Trimming treats
/// whitespace and BOM as a single character class to handle
/// `\u{feff}  pattern` and `pattern\u{feff}` correctly with one
/// pass — the previous order-sensitive `trim().trim_start_matches`
/// chain corrupted those edge cases.
fn collect_lines<R, T>(
    reader: R,
    label: &str,
    map: impl Fn(&str) -> Option<T>,
) -> Result<Vec<T>, String>
where
    R: std::io::BufRead,
{
    reader
        .lines()
        .enumerate()
        .filter_map(|(i, r)| match r {
            Ok(line) => {
                map(line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')).map(Ok)
            }
            Err(e) => Some(Err(format!("{label}: read error on line {}: {e}", i + 1))),
        })
        .collect()
}

/// Expand seed paths for the walk: union `--paths` with
/// `--paths-from`, then for each seed:
///   - file → keep as-is (explicit override of any ignore rules);
///   - directory → expand via `ignore::WalkBuilder`, gitignore-aware
///     unless `no_ignore` is set.
///
/// Returns a flat `Vec<PathBuf>` of files. Include/exclude globs are
/// applied later by `explore()`, matching today's semantics.
fn expand_seed_paths(
    paths: Vec<PathBuf>,
    paths_from: Option<PathBuf>,
    no_ignore: bool,
) -> Vec<PathBuf> {
    use ignore::WalkBuilder;
    let mut seeds = paths;
    if let Some(src) = paths_from {
        seeds.extend(read_paths_from(&src).unwrap_or_else(|e| die(e)));
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for seed in seeds {
        if !seed.exists() {
            // Match today's `explore()` behavior: warn, do not die.
            eprintln!("Warning: File doesn't exist: {}", seed.display());
            continue;
        }
        if seed.is_file() {
            out.push(seed);
            continue;
        }
        let mut wb = WalkBuilder::new(&seed);
        wb.hidden(true)
            .follow_links(false)
            .require_git(false)
            .git_ignore(!no_ignore)
            .git_exclude(!no_ignore)
            .git_global(!no_ignore)
            .ignore(!no_ignore)
            .parents(!no_ignore);
        for entry in wb.build() {
            let entry = entry
                .unwrap_or_else(|e| die(format_args!("walk error in {}: {e}", seed.display())));
            if entry.file_type().is_some_and(|t| t.is_file()) {
                out.push(entry.into_path());
            }
        }
    }
    out
}

fn run_walk(globals: GlobalOpts, cfg: Config) -> HashMap<String, Vec<PathBuf>> {
    let include = mk_globset(globals.include).unwrap_or_else(|e| die(e));
    let mut exclude_patterns = globals.exclude;
    if let Some(src) = globals.exclude_from {
        exclude_patterns.extend(read_exclude_patterns_from(&src).unwrap_or_else(|e| die(e)));
    }
    let exclude = mk_globset(exclude_patterns).unwrap_or_else(|e| die(e));
    let num_jobs = resolve_num_jobs(globals.num_jobs);
    let paths = expand_seed_paths(globals.paths, globals.paths_from, globals.no_ignore);
    let files_data = FilesData {
        include,
        exclude,
        paths,
    };
    ConcurrentRunner::new(num_jobs, act_on_file)
        .set_proc_dir_paths(process_dir_path)
        .run(cfg, files_data)
        .unwrap_or_else(|e| die(format_args!("{e:?}")))
}

/// Load a `[thresholds]` table from `path`, returning the parsed map.
/// On any I/O or parse error the process dies with exit code 1, keeping
/// exit 2 reserved for the "thresholds exceeded" case.
fn load_threshold_config(path: &Path) -> BTreeMap<String, f64> {
    let bytes = read_file(path).unwrap_or_else(|e| die_io("read threshold config", path, e));
    let text = std::str::from_utf8(&bytes)
        .unwrap_or_else(|e| die_io("decode UTF-8 from threshold config", path, e));
    let cfg: ThresholdConfig =
        toml::from_str(text).unwrap_or_else(|e| die_io("parse threshold config", path, e));
    cfg.thresholds
}

/// Load a baseline file. Same error contract as `load_threshold_config`:
/// any I/O, UTF-8, or schema error dies with exit code 1.
fn load_baseline(path: &Path) -> Baseline {
    let bytes = read_file(path).unwrap_or_else(|e| die_io("read baseline", path, e));
    let text = std::str::from_utf8(&bytes)
        .unwrap_or_else(|e| die_io("decode UTF-8 from baseline", path, e));
    Baseline::from_str(text).unwrap_or_else(|e| die_io("parse baseline", path, e))
}

/// Write `bytes` to `path` atomically: create the parent directory if
/// needed, write to `<path>.bca-tmp`, then rename. Survives a `kill -9`
/// mid-write — the consumer sees either the previous file or the
/// fully-written new file, never a half-written one.
///
/// The suffix is *appended* to the full path rather than replacing the
/// extension, so a user-supplied path like `foo.tmp` does not collide
/// with the temporary file. On rename failure (e.g. cross-filesystem
/// `EXDEV`, permission denied) the temporary file is removed best-effort
/// before propagating the original error.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".bca-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        // Cleanup is best-effort; if the rename failed the user already
        // has an error to report, and a leftover .bca-tmp removal that
        // fails would only obscure it.
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Drive the `check` subcommand as a five-stage pipeline:
/// validate args + build thresholds → walk + sort → maybe write a
/// baseline → filter against a loaded baseline → emit results and
/// exit. Each stage is its own helper so the control flow reads
/// top-down without nested decisions about which arm fires when.
fn run_check(globals: GlobalOpts, args: CheckArgs, preproc: Option<Arc<PreprocResults>>) {
    let set = validate_and_build_thresholds(&args);
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
    let any_violations = emit_check_results(pairs, &args);

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
    let text = baseline::render(&file)
        .unwrap_or_else(|e| die(format_args!("serialize baseline: {e}")));
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

/// Render the (optionally tagged) violations to stderr and, if
/// `--output-format` is set, also emit the aggregated CI/IDE
/// document. Returns `true` iff any violations were emitted, so the
/// caller can decide the exit code without re-checking the pairs.
fn emit_check_results(pairs: Vec<(Violation, Option<Coverage>)>, args: &CheckArgs) -> bool {
    // BrokenPipe on stderr (e.g. when piped to `head`) is the only
    // realistic write failure here; swallow it rather than die so the
    // exit-code contract is honored.
    let mut stderr = std::io::stderr().lock();
    for (v, tag) in &pairs {
        let _ = writeln!(stderr, "{}", render_violation_line(v, tag.as_ref()));
    }
    if !args.no_summary && !pairs.is_empty() {
        let _ = write_summary_footer(&mut stderr, &pairs);
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

/// Emit the per-file rollup footer to `stderr` after the per-violation
/// lines. Groups violations by path, cites the single worst-ratio
/// metric per file (`value / limit`), and sorts rows by violation count
/// descending then path ascending.
///
/// The caller is responsible for skipping the call when `pairs` is
/// empty or `--no-summary` is set; this function unconditionally writes
/// the banner + at least one row when invoked.
///
/// `BrokenPipe` and similar transient write failures are propagated to
/// the caller, which swallows them — the exit-code contract above is
/// the source of truth for whether the gate passed.
fn write_summary_footer(
    w: &mut impl Write,
    pairs: &[(Violation, Option<Coverage>)],
) -> std::io::Result<()> {
    // Group by raw PathBuf rather than `path.display()` to preserve
    // non-UTF-8 byte identity. Two paths that differ only in invalid
    // UTF-8 (`b"foo\xff.rs"` vs `b"foo\xfe.rs"`) would collapse to the
    // same lossy display string but stay distinct here. `path.display()`
    // is still used to *render* the rendered footer row so it matches
    // the per-violation stderr line format above.
    let mut by_path: BTreeMap<&PathBuf, Vec<&Violation>> = BTreeMap::new();
    for (v, _) in pairs {
        by_path.entry(&v.path).or_default().push(v);
    }

    // Compute (count, worst-violation, display) per file. The display
    // string is cached here so the sort below doesn't recompute
    // `path.display().to_string()` on every comparator call (O(n²) on
    // the path length otherwise).
    let mut rows: Vec<(usize, &Violation, String)> = by_path
        .iter()
        .filter_map(|(path, vs)| {
            let worst = pick_worst(vs)?;
            Some((vs.len(), worst, path.display().to_string()))
        })
        .collect();
    // Sort: violation count desc, then cached display string asc for
    // stable equal-count ordering.
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));

    writeln!(w)?;
    writeln!(w, "--- summary ---")?;
    for (count, worst, display) in rows {
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
        )?;
    }
    Ok(())
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

fn run_command_metrics(
    globals: GlobalOpts,
    args: StructuredArgs,
    preproc: Option<Arc<PreprocResults>>,
) {
    if matches!(args.output_format, Some(MetricsFormat::Cbor)) && args.output.is_none() {
        die(CBOR_STDOUT_ERROR);
    }
    if args.output_format.is_some()
        && let Some(ref out) = args.output
        && out.exists()
        && !out.is_dir()
    {
        die("--output must be a directory for `metrics`");
    }
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
    if args.output_format.is_some()
        && let Some(ref out) = args.output
        && out.exists()
        && !out.is_dir()
    {
        die("--output must be a directory for `ops`");
    }
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

fn run_command_report(
    globals: GlobalOpts,
    args: ReportArgs,
    preproc: Option<Arc<PreprocResults>>,
) {
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

/// Names of every subcommand on the new CLI. Kept in sync with the
/// `Command` enum by `tests::subcommands_match_command_enum`, which
/// fails if the two ever drift.
const SUBCOMMANDS: &[&str] = &[
    "metrics",
    "ops",
    "report",
    "dump",
    "find",
    "count",
    "functions",
    "strip-comments",
    "preproc",
    "list-metrics",
    "check",
];

/// Decode the value of `-O <v>` / `--output-format <v>` /
/// `--output-format=<v>` / `-O<v>` from a flat argv slice. Returns
/// the first match (callers pre-filter the slice to the legacy
/// invocation's tokens, so a single occurrence is the realistic
/// case).
fn parse_output_format_value(args: &[String]) -> Option<&str> {
    args.iter().enumerate().find_map(|(i, a)| {
        let s = a.as_str();
        if s == "-O" || s == "--output-format" {
            args.get(i + 1).map(String::as_str)
        } else if let Some(rest) = s.strip_prefix("--output-format=") {
            Some(rest)
        } else {
            s.strip_prefix("-O").filter(|r| !r.is_empty())
        }
    })
}

/// Scan `args` for `-O <offender>` / `--output-format <offender>` /
/// `--output-format=<offender>` against the four moved formats (any
/// variant of [`AggregatedFormat`]) and build a migration hint
/// pointing at `bca check`. Returns `None` when no offender format
/// is found, so the caller can fall through to clap's own error.
fn offender_format_migration_hint(args: &[String]) -> Option<String> {
    let fmt =
        parse_output_format_value(args).filter(|f| AggregatedFormat::from_str(f, true).is_ok())?;
    Some(format!(
        "note: -O {fmt} moved to `bca check` in #235; offender formats are no longer accepted on `bca metrics` / `bca ops`.\n  bca metrics -O {fmt} ...  ->  bca check --threshold <metric>=<limit> --output-format {fmt} [--output FILE]\n  Run `bca check --help` for the threshold and output-format flags.\n"
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
    // <offender>` — the four offender formats moved to `bca check`
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
        ("--find", "bca find <NODE> [<NODE>...]"),
        ("-f", "bca find <NODE> [<NODE>...]"),
        ("--count", "bca count <NODE> [<NODE>...]"),
        ("-C", "bca count <NODE> [<NODE>...]"),
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
