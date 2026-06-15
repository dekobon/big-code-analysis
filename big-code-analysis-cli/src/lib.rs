// bca: suppress-file(halstead, loc, nargs, nexits, nom)
// CLI library glue (seed-path expansion, legacy-flag hints, walk wiring); the
// offenders are static-table volume and many-fn aggregation artifacts, not
// per-function logic complexity (cognitive/cyclomatic stay enforced).

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
mod baseline_diff;
mod check_flags;
mod check_format;
mod commands;
mod deprecations;
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
mod provenance;
mod threshold_suggestion;
mod thresholds;
mod vcs_command;
mod vcs_jit;
mod vcs_report;
mod vcs_trend;
mod walk_seed;

pub use commands::run;
use dispatch::act_on_file;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Display;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use clap::{Args, Parser, Subcommand, ValueEnum};
use globset::{Glob, GlobSet, GlobSetBuilder};

use baseline::Baseline;
use check_flags::{CiDetect, ExitCodes, SummaryFile, Tier, TierSpec};
use check_format::AggregatedFormat;
use formats::{JitFormat, MetricsFormat, ReportFormat, TrendFormat, VcsFormat};
use markdown_report::FunctionSummary;
use metric_catalog::ListMetricsMode;
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

/// One aggregated per-file result for the single-file `--output <FILE>`
/// mode on `metrics` / `ops` (#669). Workers stream these through
/// `Config::aggregate_tx`; the command runner collects them after the
/// walk and serializes the whole set as one document. Boxed so the
/// channel item stays small (a `FuncSpace` tree is large).
pub(crate) enum AggregateItem {
    /// A `metrics` file-level space plus its emitted path (the path is
    /// needed for the CSV aggregate, whose rows are keyed by file).
    Metrics(Box<FuncSpace>, PathBuf),
    /// An `ops` operator/operand tree.
    Ops(Box<Ops>),
}

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

/// Print an `error:`-prefixed diagnostic and exit with [`EXIT_TOOL_ERROR`].
///
/// The bare lowercase `error:` prefix matches clap's own usage-error
/// formatter (#609) so every stderr line — clap's and ours — reads in the
/// one rustc/cargo/git diagnostic family. Routing all fatal tool errors
/// through this single helper keeps the prefix structurally enforced
/// rather than re-spelled at each call site.
pub(crate) fn die(msg: impl Display) -> ! {
    eprintln!("error: {msg}");
    process::exit(EXIT_TOOL_ERROR);
}

/// Print a `warning:`-prefixed diagnostic to stderr (non-fatal). The
/// counterpart to [`die`] / [`note`]: one helper per severity so the
/// lowercase `warning:` prefix is enforced structurally (#609).
pub(crate) fn warn(msg: impl Display) {
    eprintln!("warning: {msg}");
}

/// Print a `note:`-prefixed diagnostic to stderr — supplementary context
/// attached to a warning or to surprising-but-valid input. The lowest of
/// the three diagnostic severities (#609).
pub(crate) fn note(msg: impl Display) {
    eprintln!("note: {msg}");
}

/// Emit a one-line stderr deprecation notice when a deprecated flag (or
/// subcommand) spelling is used in place of its replacement (issues
/// #688/#666/#646; the one-cycle alias horizon). The shared emission
/// point for the CLI flag/subcommand deprecations: the resolution-time
/// folds (`--headroom`, `--strict-exit-codes`) and the argv-scan alias
/// detector in [`crate::deprecations`] both route through here, so all
/// flag-deprecation chatter reads alike under the one `warning:` prefix
/// (via [`warn`]). The manifest's deprecated-key notices
/// ([`crate::manifest`]) are emitted at their own site but share it.
pub(crate) fn warn_deprecated_flag(old: &str, new: &str) {
    warn(format_args!(
        "`{old}` is deprecated; use `{new}` instead (removed in the next major release)"
    ));
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

/// Reject an `--output` path that names an existing directory or whose
/// parent directory is missing, mirroring the fast pre-walk validation
/// `report` / `exemptions` do. `label` names the subcommand for the
/// error message.
fn validate_output_path(output: &Path, label: &str) {
    if output.exists() && output.is_dir() {
        die(format_args!("--output must be a file path for `{label}`"));
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

/// Emit `bytes` to the `--output` file when given, else stdout. The
/// universal rule across every emitting subcommand: stdout if `--output`
/// is omitted. `verb` is the action phrase for an I/O error (e.g.
/// `"write diff to"`).
fn write_output_or_stdout(output: Option<&Path>, verb: &str, bytes: &[u8]) {
    match output {
        Some(path) => std::fs::write(path, bytes).unwrap_or_else(|e| die_io(verb, path, e)),
        None => write_stdout_or_die(bytes),
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
    after_help = "Exit codes:\n  0  success\n  1  tool error (bad flag/threshold/glob spec, unreadable input, parse failure)\n  2  metric gate: `check` thresholds exceeded / `vcs commit --fail-above` breached / `diff --exit-code` non-empty (default contract)\n  3-5  `check --strict-exit-codes` only: tiered violation severity\n\nExit code 1 is always a tool error, never a metric signal — usage errors\n(unknown flag, bad subcommand, malformed `--threshold` value) exit 1, not 2.\nCodes 2-5 are gate signals emitted only by `check`, `vcs commit --fail-above`,\nand `diff` / `diff-baseline` when run with the opt-in `--exit-code` flag.\nEvery other subcommand exits 0 on success and 1 on error.\n\nMigrating from the flag-style CLI? See the migration guide:\n  https://dekobon.github.io/big-code-analysis/migration.html"
)]
pub struct Cli {
    #[clap(flatten)]
    universal: UniversalArgs,
    #[command(subcommand)]
    command: Command,
}

/// Truly universal flags — meaningful for every subcommand and kept
/// `global = true` so they parse in either position. Per the 2.0
/// flag-scoping work (#597), every walk-, tuning-, preprocessor-, and
/// output-specific flag instead lives in a `#[command(flatten)]` group
/// ([`WalkSelectionArgs`], [`WalkTuningArgs`], [`PreprocArgs`],
/// [`OutputArgs`]) attached only to the subcommands that consume it, so
/// passing an inert flag to a subcommand that never read it is now a
/// hard clap usage error (exit 1) instead of a silent no-op.
#[derive(Args, Debug, Default, Clone)]
struct UniversalArgs {
    /// Print warnings (skipped files, unrecognized languages). `--warning`
    /// (singular) is kept as a hidden alias for one release cycle and is
    /// slated for removal in the next major.
    // The singular `--warning` alias dates to issue #604.
    #[clap(long = "warnings", short = 'w', global = true, alias = "warning")]
    warning: bool,
    /// Log a "skipped (generated): <path>" line to stderr for each file
    /// auto-skipped by the generated-code detector. Useful for auditing
    /// which files were excluded.
    #[clap(long, global = true)]
    report_skipped: bool,
}

/// Input-selection flags (#597). Flattened into every subcommand that
/// walks a source tree (`metrics`, `ops`, `dump`, `find`, `count`,
/// `functions`, `strip-comments`, `preproc`, `report`, `check`,
/// `exemptions`, `vcs` ranking, `init`, `diff`). Subcommands that walk
/// nothing (`list-metrics`, `diff-baseline`) and the commit-scoring
/// `vcs commit` / `vcs trend` paths omit it, so an inert `--paths` /
/// `--exclude` there is a usage error.
#[derive(Args, Debug, Default, Clone)]
struct WalkSelectionArgs {
    /// Input files or directories to analyze. Unioned with any
    /// positional `[PATHS]`. Defaults to the current directory
    /// (`.`) when omitted and no manifest `paths` is set; an
    /// explicitly-given path that does not exist is an error (exit 1).
    #[clap(long, short, value_parser, help_heading = "Input selection")]
    paths: Vec<PathBuf>,
    /// Glob to include files. Repeat the flag to add multiple globs
    /// (`-I '*.rs' -I '*.toml'`); each occurrence takes exactly one
    /// value, so a positional argument that follows is never swallowed.
    /// A leading `./` is optional: `dir/**` and `./dir/**` are equivalent.
    #[clap(long, short = 'I', num_args(1), action = clap::ArgAction::Append, help_heading = "Input selection")]
    include: Vec<String>,
    /// Glob to exclude files. Repeat the flag to add multiple globs
    /// (`-X '*.tmp' -X '*.bak'`); each occurrence takes exactly one
    /// value, so a positional argument that follows is never swallowed.
    /// A leading `./` is optional: `dir/**` and `./dir/**` are equivalent.
    /// CLI values are *merged with* (unioned, not a replacement for) any
    /// `bca.toml` `exclude` list and any `--exclude-from` patterns, so a
    /// CLI `--exclude` never silently un-excludes a directory the
    /// project config deliberately skipped. Pass `--no-config` to ignore
    /// the manifest entirely.
    #[clap(long, short = 'X', num_args(1), action = clap::ArgAction::Append, help_heading = "Input selection")]
    exclude: Vec<String>,
    /// Force a language instead of inferring from extension. Accepts a
    /// canonical language name (`rust`, `python`, `cpp`, …) or a file
    /// extension (`rs`, `py`, …). An unrecognized value is a hard error.
    #[clap(
        long,
        short = 'l',
        alias = "language-type",
        help_heading = "Input selection"
    )]
    language: Option<String>,
    /// Disable auto-skip of files marked as generated (e.g. `@generated`,
    /// `DO NOT EDIT`, `GENERATED CODE` near the top). By default the CLI
    /// skips such files so generated bindings do not skew metrics.
    #[clap(long, help_heading = "Input selection")]
    no_skip_generated: bool,
    /// Read newline-separated input paths from a file. Use `-` to read
    /// from stdin. Combined as a union with any `--paths` values; globs
    /// still apply. Blank lines are skipped; `#` is treated as a path
    /// character (not a comment). To pass a file literally named `-`,
    /// use `./-`.
    #[clap(long = "paths-from", value_parser, help_heading = "Input selection")]
    paths_from: Option<PathBuf>,
    /// Read additional `--exclude` glob patterns from a file (one per
    /// line, `.gitignore`-style). Blank lines and lines whose first
    /// non-whitespace character is `#` are skipped. Use `-` to read
    /// from stdin; to pass a file literally named `-`, use `./-`.
    /// Patterns are unioned with any `--exclude` values into a single
    /// deny-set; order does not matter. Convention is a `.bcaignore`
    /// at the repo root, mirroring `.gitignore` / `.dockerignore`.
    #[clap(long = "exclude-from", value_parser, help_heading = "Input selection")]
    exclude_from: Option<PathBuf>,
    /// Disable `.gitignore` / `.ignore` / global gitignore awareness
    /// when expanding input directories. Explicit file paths are always
    /// honored regardless of this flag.
    #[clap(long = "no-ignore", help_heading = "Input selection")]
    no_ignore: bool,
    /// Skip auto-discovery of a `bca.toml` manifest. By default `bca`
    /// climbs from the working directory to the repo root looking for
    /// `bca.toml` and merges its keys *under* any explicit CLI flags.
    /// Pass this for raw, fully-explicit invocations that must not pick
    /// up repo-level config (e.g. a reproducible CI one-liner). When no
    /// manifest is discovered this flag is a no-op.
    #[clap(long = "no-config", help_heading = "Input selection")]
    no_config: bool,
}

/// Walker-tuning / analysis-option flags (#597). Flattened alongside
/// [`WalkSelectionArgs`] into the walking subcommands. `--jobs` controls
/// concurrency; `--exclude-tests` / `--no-cyclomatic-try` shape metric
/// computation, so they ride with the commands that compute metrics.
#[derive(Args, Debug, Default, Clone)]
struct WalkTuningArgs {
    /// Number of jobs.
    ///
    /// Defaults to the effective CPU count as reported by the OS
    /// (cgroup-quota- and cpuset-aware on Linux). Pass an explicit
    /// integer or `auto` to override. `--jobs 1` forces serial mode for
    /// debugging. `--num-jobs` is kept as a hidden alias for one release
    /// cycle and is slated for removal in the next major.
    // The `--num-jobs` alias dates to issue #604.
    #[clap(
        long = "jobs",
        short = 'j',
        alias = "num-jobs",
        default_value = "auto",
        value_name = "N|auto",
        help_heading = "Walker tuning"
    )]
    num_jobs: NumJobs,
    /// Exclude inline test code from metric computation. Currently
    /// applies to Rust only (skips `#[test]`, `#[cfg(test)]`,
    /// `#[tokio::test]`, `#[rstest]`, `#![cfg(test)]` items and
    /// their subtrees). Default is off — every node is counted, so
    /// numbers stay byte-for-byte stable. Languages without a
    /// test-subtree skip rule ignore this flag.
    // The "off by default" guarantee preserves the pre-#182 numbers;
    // the skip hook is `Checker::should_skip_subtree`.
    #[clap(long = "exclude-tests", help_heading = "Walker tuning")]
    exclude_tests: bool,
    /// Whether Rust's `?` operator (the `try_expression` node)
    /// contributes to cyclomatic complexity (standard and modified).
    /// Defaults to `true` — `?` counts +1, matching upstream
    /// rust-code-analysis and every published metric value. Pass
    /// `--cyclomatic-count-try=false` to treat `?` as linear error
    /// propagation — useful when cyclomatic is used as a maintainability
    /// gate that should not penalize fallible-but-linear code. Rust-only:
    /// no other language emits the node, so the flag is inert elsewhere.
    /// Mirrors the `cyclomatic_count_try` manifest key; the CLI value
    /// overrides the manifest in either direction.
    #[clap(
        long = "cyclomatic-count-try",
        value_name = "BOOL",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        action = clap::ArgAction::Set,
        help_heading = "Walker tuning"
    )]
    cyclomatic_count_try: Option<bool>,
    /// Deprecated alias for `--cyclomatic-count-try=false`. Retained for
    /// one release cycle; pass `--cyclomatic-count-try false` instead.
    /// Conflicts with the value-taking form.
    // The `--cyclomatic-count-try` flag pair was introduced in issue #666.
    #[clap(
        long = "no-cyclomatic-try",
        hide = true,
        conflicts_with = "cyclomatic_count_try",
        help_heading = "Walker tuning"
    )]
    no_cyclomatic_try: bool,
}

impl WalkTuningArgs {
    /// Resolve the effective "`?` counts toward cyclomatic" decision,
    /// folding the deprecated `--no-cyclomatic-try` alias into the
    /// positive `--cyclomatic-count-try <bool>` (issue #666). Returns
    /// `None` when neither was set on the CLI, so the manifest
    /// `cyclomatic_count_try` key can fill in; the CLI value otherwise
    /// overrides the manifest in either direction. `clap`'s
    /// `conflicts_with` already rejects passing both.
    fn resolved_count_cyclomatic_try(&self) -> Option<bool> {
        if self.no_cyclomatic_try {
            warn_deprecated_flag("--no-cyclomatic-try", "--cyclomatic-count-try=false");
            return Some(false);
        }
        self.cyclomatic_count_try
    }
}

/// Preprocessor flag (#597). Flattened only into the C/C++-consuming
/// walking subcommands; `vcs`, `preproc` (which produces rather than
/// consumes), and the non-walking subcommands omit it.
#[derive(Args, Debug, Default, Clone)]
struct PreprocConsumeArgs {
    /// Existing preprocessor-data JSON to consume during C/C++ analysis.
    /// Use `bca preproc` to produce one.
    #[clap(long, value_parser, help_heading = "Preprocessor")]
    preproc_data: Option<PathBuf>,
}

/// Output flag (#597). `--color` only affects the human-readable `text`
/// dumps, so it is flattened only into the subcommands that render one
/// (`metrics`, `ops`, `dump`, `find`, `functions`).
#[derive(Args, Debug, Default, Clone)]
struct OutputArgs {
    /// When to colorize the human-readable `text` dumps (`metrics` /
    /// `ops` default tree, `dump`, `find`, `functions`): `auto`
    /// (default — color only when stdout is a terminal and `NO_COLOR`
    /// is unset), `always` (force escapes even when piped/redirected),
    /// or `never` (plain text). Structured formats (`json` / `yaml` /
    /// `toml` / `cbor` / `csv`) and file output are never colorized.
    /// Honors the `NO_COLOR` convention (<https://no-color.org>) unless
    /// `--color always` overrides it.
    #[clap(long = "color", value_enum, default_value_t = ColorWhen::Auto, value_name = "WHEN", help_heading = "Output")]
    color: ColorWhen,
}

/// Runtime carrier assembled from the per-subcommand flag groups
/// ([`WalkSelectionArgs`], [`WalkTuningArgs`], [`PreprocConsumeArgs`],
/// [`OutputArgs`]) plus the [`UniversalArgs`] flags. The command runners
/// and the walk plumbing (`run_walk`, `resolve_walk_files`, the manifest
/// merge) all operate on this single shape, so splitting the clap surface
/// into help-grouped, per-subcommand groups (#597) left their signatures
/// unchanged. Built by the `WalkArgs::to_globals` accessors on each
/// subcommand's Args struct.
#[derive(Debug, Default, Clone)]
struct GlobalOpts {
    paths: Vec<PathBuf>,
    include: Vec<String>,
    exclude: Vec<String>,
    num_jobs: NumJobs,
    language: Option<String>,
    warning: bool,
    no_skip_generated: bool,
    report_skipped: bool,
    preproc_data: Option<PathBuf>,
    paths_from: Option<PathBuf>,
    exclude_from: Option<PathBuf>,
    no_ignore: bool,
    exclude_tests: bool,
    /// Whether Rust's `?` counts toward cyclomatic complexity, in the
    /// positive sense (issue #666). `None` means the CLI set neither
    /// `--cyclomatic-count-try` nor the deprecated `--no-cyclomatic-try`,
    /// so the manifest `cyclomatic_count_try` key (or the built-in
    /// `true` default) decides.
    count_cyclomatic_try: Option<bool>,
    no_config: bool,
    color: ColorWhen,
}

/// Trailing positional `[PATHS]...` shared by the walking subcommands
/// that can take a bare positional path (`bca metrics src/`) — every
/// walker except `diff` (whose positional slots are already spent on the
/// `<old> <new>` metric-output sets). Unioned with `--paths`/`-p` (#651).
#[derive(Args, Debug, Default, Clone)]
struct PositionalPaths {
    /// Input files or directories to analyze, given positionally
    /// (`bca metrics src/ tests/`). Unioned with any `--paths`/`-p`
    /// values.
    // The clap arg id (`positional_paths`, #651) is distinct from the
    // `--paths` flag's id so both can coexist on one command — clap
    // requires unique arg ids; the two are merged by `assemble_globals`.
    #[clap(value_name = "PATHS", value_parser, help_heading = "Input selection")]
    positional_paths: Vec<PathBuf>,
}

/// Assemble a runtime [`GlobalOpts`] from a subcommand's flag groups.
/// `tuning`, `preproc`, `output`, and the `language` source vary by
/// subcommand (a command that does not flatten a group passes the group
/// default), so the builder takes each piece explicitly. `positional`
/// carries the trailing `[PATHS]` (#651), unioned positional-first with
/// the group's `--paths` values.
fn assemble_globals(
    selection: &WalkSelectionArgs,
    positional: &PositionalPaths,
    tuning: &WalkTuningArgs,
    preproc: &PreprocConsumeArgs,
    output: &OutputArgs,
    universal: &UniversalArgs,
) -> GlobalOpts {
    let mut paths = positional.positional_paths.clone();
    paths.extend(selection.paths.iter().cloned());
    GlobalOpts {
        paths,
        include: selection.include.clone(),
        exclude: selection.exclude.clone(),
        num_jobs: tuning.num_jobs,
        language: selection.language.clone(),
        warning: universal.warning,
        no_skip_generated: selection.no_skip_generated,
        report_skipped: universal.report_skipped,
        preproc_data: preproc.preproc_data.clone(),
        paths_from: selection.paths_from.clone(),
        exclude_from: selection.exclude_from.clone(),
        no_ignore: selection.no_ignore,
        exclude_tests: tuning.exclude_tests,
        count_cyclomatic_try: tuning.resolved_count_cyclomatic_try(),
        no_config: selection.no_config,
        color: output.color,
    }
}

/// `--color` flag values: when to emit ANSI color escapes in the
/// human-readable `text` dumps. Resolved into a library
/// [`big_code_analysis::ColorMode`] by [`ColorWhen::resolve`], which
/// folds in the `NO_COLOR` convention and stdout tty detection.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[clap(rename_all = "lower")]
enum ColorWhen {
    /// Color when stdout is a terminal and `NO_COLOR` is unset.
    #[default]
    Auto,
    /// Always emit color, even when piped or redirected.
    Always,
    /// Never emit color.
    Never,
}

impl ColorWhen {
    /// Resolve the user's `--color` choice into the library color mode,
    /// applying the precedence chain explicit flag > `NO_COLOR` > tty
    /// detection.
    ///
    /// - `always` always colorizes — it intentionally overrides
    ///   `NO_COLOR`, so a user who asks for color in a `NO_COLOR`
    ///   environment still gets it (the explicit flag is the strongest
    ///   signal).
    /// - `never` never colorizes.
    /// - `auto` (the default) colorizes only when stdout is a terminal
    ///   *and* `NO_COLOR` is unset. A redirected or piped stdout, or any
    ///   non-empty `NO_COLOR`, resolves to plain text.
    ///
    /// The `is_terminal` argument is injected (rather than read here)
    /// so the precedence logic is unit-testable without an attached tty.
    fn resolve_with(self, stdout_is_terminal: bool) -> big_code_analysis::ColorMode {
        use big_code_analysis::ColorMode;
        match self {
            ColorWhen::Always => ColorMode::Always,
            ColorWhen::Never => ColorMode::Never,
            ColorWhen::Auto => {
                // The `NO_COLOR` convention: any value (including empty)
                // disables color (https://no-color.org/). We treat a set
                // variable as a disable signal regardless of its content,
                // matching cargo / ripgrep.
                let no_color = std::env::var_os("NO_COLOR").is_some();
                Self::resolve_auto(stdout_is_terminal, no_color)
            }
        }
    }

    /// The pure `auto`-mode precedence: colorize only when stdout is a
    /// terminal *and* `NO_COLOR` is unset. Both signals are injected so
    /// each can be exercised independently in unit tests — `resolve_with`
    /// reads the env, but the suppression rule lives here where neither
    /// a real tty nor (`unsafe`) env mutation is needed to test it (#895).
    fn resolve_auto(stdout_is_terminal: bool, no_color: bool) -> big_code_analysis::ColorMode {
        use big_code_analysis::ColorMode;
        if stdout_is_terminal && !no_color {
            ColorMode::Auto
        } else {
            ColorMode::Never
        }
    }

    /// Resolve against the real process stdout's terminal status.
    fn resolve(self) -> big_code_analysis::ColorMode {
        use std::io::IsTerminal;
        self.resolve_with(std::io::stdout().is_terminal())
    }
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compute per-file metrics and emit them in a structured format.
    Metrics(MetricsArgs),
    /// Extract per-file operands and operators.
    Ops(StructuredArgs),
    /// Rank files by change-history (VCS) risk: churn, commit and author
    /// counts, ownership dilution, and bug- / security-fix history over a
    /// git working tree. Errors clearly outside a repo.
    // Change-history ranking landed in issue #328.
    Vcs(Box<VcsArgs>),
    /// Generate an aggregated report across the analyzed source.
    Report(ReportArgs),
    /// Dump the AST to stdout. Each file's tree is prefixed with a
    /// `== <path> ==` banner so a multi-file dump is attributable.
    /// Requires an explicit path — unlike the other walking
    /// subcommands, bare `bca dump` errors instead of dumping the whole
    /// current directory (a whole-tree AST dump has no plausible use).
    Dump(DumpArgs),
    /// Find nodes of one or more types.
    Find(FindArgs),
    /// Count nodes of one or more types.
    Count(CountArgs),
    /// List functions/methods and their spans.
    Functions(FunctionsArgs),
    /// Remove comments from source files.
    StripComments(StripCommentsArgs),
    /// Generate preprocessor-data JSON for C/C++ analysis.
    Preproc(PreprocArgs),
    /// List the metrics this tool can compute and exit.
    ListMetrics(ListMetricsArgs),
    /// Check per-function metrics against thresholds. Exits 2 when any
    /// threshold is exceeded; reserve exit 1 for tool errors so CI can
    /// distinguish "metric regression" from "tool crashed".
    /// `--strict-exit-codes` opts into tiered codes (2-5) that split the
    /// violation case by severity.
    // Boxed because `CheckArgs` is by far the largest variant payload
    // (its many gate-tuning flags dwarf the other subcommands' args);
    // boxing keeps `Command` small and silences `large_enum_variant`.
    Check(Box<CheckArgs>),
    /// Scaffold the canonical adoption files (`bca.toml` manifest,
    /// `.bcaignore`, `.bca-baseline.toml`) in the current directory.
    /// Replaces the six-step copy-paste flow from the book's adoption
    /// recipe. Refuses to overwrite existing files without `--force`.
    Init(InitArgs),
    /// Diff two `.bca-baseline.toml` files and report what was added,
    /// removed, worsened, or improved. Replaces the in-the-head TOML
    /// diff parsing the book's PR-review recipe used to walk through.
    /// Exits 0 on success by default — the diff is informational, not a
    /// gate. With `--exit-code`, exits 2 when the filtered diff is
    /// non-empty.
    DiffBaseline(DiffBaselineArgs),
    /// Compare two metric-output runs and report, per metric, which
    /// files changed (old to new), plus files added/removed between the
    /// two sets. Each side is a per-file JSON file or a directory tree of
    /// them (the form `bca metrics -O json --output-dir DIR` writes).
    /// Replaces the grammar-bump glue chain — the external
    /// `json-minimal-tests` binary plus `split-minimal-tests.py` — with
    /// one native command. Exits 0 on success by default; the diff is
    /// informational, not a gate. With `--exit-code`, exits 2 when the
    /// filtered diff is non-empty.
    Diff(DiffArgs),
    /// Audit everything the `bca check` gate skips in one view:
    /// in-source suppression markers (`bca: suppress`,
    /// `#lizard forgives`, …), `[check.exclude]` globs, and
    /// `.bca-baseline.toml` entries. Read-only; always exits 0 on
    /// success.
    Exemptions(ExemptionsArgs),
}

/// Shared shape for `metrics` and `ops`: same format set, same output
/// semantics (directory of per-file emissions; stdout if omitted).
#[derive(Args, Debug)]
struct StructuredArgs {
    #[clap(flatten)]
    positional: PositionalPaths,
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    #[clap(flatten)]
    preproc: PreprocConsumeArgs,
    #[clap(flatten)]
    out: OutputArgs,
    /// Output format. When omitted, the default `text` format prints a
    /// human-readable colored tree to stdout (`metrics` shows the metric
    /// tree, `ops` the operator/operand tree); pass `--format text`
    /// to request that default explicitly (e.g. to override a `bca.toml`
    /// that set a structured format). `json` / `yaml` / `toml` / `cbor` /
    /// `csv` emit structured per-file data. `--output-format` is accepted
    /// as a deprecated alias; it is hidden from help and slated for
    /// removal in 2.0.
    // The `--output-format` alias is the pre-rename spelling from issue #513.
    #[clap(
        long = "format",
        short = 'O',
        alias = "output-format",
        value_name = "FORMAT",
        value_enum
    )]
    output_format: Option<MetricsFormat>,
    /// Output file. Writes one aggregate document (a top-level array of
    /// the per-file results; TOML wraps it under a `files` key) for the
    /// whole run. Stdout if omitted (CBOR requires this flag). Use
    /// `--output-dir` for the per-file directory tree; passing both is an
    /// error.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Output directory. Writes one document per input file, named by the
    /// input path plus the format extension. Mutually exclusive with
    /// `--output` (which writes a single aggregate file).
    #[clap(long = "output-dir", value_parser)]
    output_dir: Option<PathBuf>,
    /// Pretty-print JSON / TOML output.
    #[clap(long)]
    pretty: bool,
}

impl StructuredArgs {
    /// Assemble the runtime [`GlobalOpts`] from this command's flattened
    /// walk / tuning / preproc / output groups plus the universal flags.
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &self.positional,
            &self.tuning,
            &self.preproc,
            &self.out,
            universal,
        )
    }

    /// Collapse an explicit `--format text` to `None`, the historical
    /// no-`--format` default (issue #604). `text` is a surface alias for
    /// the human-readable tree, so every downstream guard and the
    /// dispatch see the same shape whether the flag was `text` or absent,
    /// keeping the two output paths byte-identical.
    fn normalize_text_format(&mut self) {
        if matches!(self.output_format, Some(MetricsFormat::Text)) {
            self.output_format = None;
        }
    }
}

/// Risk-score formula selection for `bca vcs` (issue #328).
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
enum RiskFormulaArg {
    /// Log-scaled weighted sum with categorical bumps (default).
    Weighted,
    /// Per-signal percentile rank within the analyzed set, averaged.
    Percentile,
}

impl VcsArgs {
    /// Assemble the runtime [`GlobalOpts`] for the `vcs` ranking walk
    /// from its flattened selection / tuning groups plus the universal
    /// flags. `vcs` consumes no preprocessor data and renders no
    /// colorized text dump, so those groups stay at their defaults.
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &PositionalPaths::default(),
            &self.tuning,
            &PreprocConsumeArgs::default(),
            &OutputArgs::default(),
            universal,
        )
    }
}

impl From<RiskFormulaArg> for big_code_analysis::vcs::RiskFormula {
    fn from(arg: RiskFormulaArg) -> Self {
        match arg {
            RiskFormulaArg::Weighted => Self::Weighted,
            RiskFormulaArg::Percentile => Self::Percentile,
        }
    }
}

/// Flags for `bca vcs` — change-history (VCS) metrics over a git
/// working tree (issue #328). Path / include / exclude / exclude-tests /
/// no-ignore are inherited from the global options.
#[derive(Args, Debug)]
struct VcsArgs {
    // Walk-selection / tuning groups (#597) are flattened non-`global`,
    // so `bca vcs --paths src` ranks that subtree but `bca vcs commit
    // --paths …` / `bca vcs trend --exclude …` are hard usage errors:
    // those subcommands score a commit / sample a time series, not a
    // walked tree. `vcs` takes no positional `[PATHS]` (#651) because its
    // positional slot is reserved for the subcommand token.
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    /// Optional `vcs` subcommand. With none, `bca vcs` ranks files by
    /// change-history risk (the default). `commit` instead scores a single
    /// commit for just-in-time (JIT) defect-induction risk; it reuses the
    /// window / bot / merge / rename / as-of flags below (which are
    /// accepted in either position — `bca vcs --long-window 6mo commit` or
    /// `bca vcs commit --long-window 6mo`). `commit` names its commit
    /// positionally, so passing `--ref` with it is a usage error. The old
    /// `jit` spelling is a hidden alias for one release cycle.
    // commit scoring: #331; either-position flags: #598; `jit`->`commit`
    // rename: #603.
    #[command(subcommand)]
    command: Option<VcsSubcommand>,
    /// Output format. When omitted, the human-readable ranked `text`
    /// table is printed; pass `--format text` to request it explicitly.
    /// `markdown` / `html` render a sortable report page like
    /// `bca report`; `json` / `yaml` / `toml` / `cbor` / `csv` emit
    /// structured data. `--output-format` is accepted as a deprecated
    /// alias.
    // `text` unification: #659; `--output-format` alias: #513.
    #[clap(long = "format", short = 'O', alias = "output-format", value_enum)]
    format: Option<VcsFormat>,
    /// Output file. A change-history report is a single whole-repo
    /// document, so this names one file. Stdout if omitted (CBOR requires
    /// this flag — it is binary).
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Pretty-print JSON / TOML output.
    #[clap(long)]
    pretty: bool,
    /// Long observation window (`12mo`, `2y`, `52w`, `365d`, or ISO 8601
    /// `P1Y`). Accepted in the parent or subcommand position.
    #[clap(long, default_value = "12mo", global = true)]
    long_window: String,
    /// Recent observation window. Accepted in the parent or subcommand
    /// position.
    #[clap(long, default_value = "90d", global = true)]
    recent_window: String,
    /// Show only the top N files by risk score (`0` = all).
    #[clap(long, default_value_t = 50)]
    top: usize,
    /// Which tracked files to rank: `metrics` (only files
    /// bca has metrics for — the default), `all` (every tracked text
    /// file), or a comma-separated extension allow-list (`rs,py,toml`).
    /// Applied on top of `--paths`/`--include`/`--exclude` (AND
    /// semantics). When omitted, the `bca.toml` `[vcs] file_types` key is
    /// used if present, else `metrics`. Setting it here replaces the
    /// manifest value.
    #[clap(long, value_name = "SCOPE")]
    file_types: Option<String>,
    /// Revision to analyze (defaults to `HEAD`). Accepted in the parent
    /// or `trend` subcommand position, but rejected under `commit`, which
    /// names its commit positionally.
    //
    // Either-position acceptance and the `commit` positional conflict
    // landed in issue #598.
    // Stored as an `Option` (rather than a `default_value = "HEAD"`
    // `String`) so an explicit `--ref` is distinguishable from the
    // default — the `jit` conflict check keys off `Some`, and
    // `vcs_command::build_options` applies the `HEAD` default at the
    // single point of use.
    #[clap(long = "ref", global = true)]
    reference: Option<String>,
    /// Walk the full commit DAG rather than first-parent only.
    #[clap(long, global = true)]
    full_history: bool,
    /// Include merge commits (skipped by default).
    #[clap(long, global = true)]
    include_merges: bool,
    /// Do not follow file renames across history.
    #[clap(long, global = true)]
    no_follow_renames: bool,
    /// Do not exclude bot author identities.
    #[clap(long, global = true)]
    no_exclude_bots: bool,
    /// Override the bot-author exclusion regex.
    #[clap(long, global = true)]
    bot_pattern: Option<String>,
    /// Reference "now" for reproducible runs (RFC 3339, `@unix`, or any
    /// git date spelling). Defaults to wall-clock time.
    #[clap(long, global = true)]
    as_of: Option<String>,
    /// Composite risk-score formula.
    #[clap(long, value_enum, default_value_t = RiskFormulaArg::Weighted, global = true)]
    risk_formula: RiskFormulaArg,
    /// Emit SHA-256-hashed canonical author identities.
    #[clap(long)]
    emit_author_details: bool,
    /// Emit stats for files deleted at the target ref.
    #[clap(long)]
    include_deleted: bool,
    /// Bus-factor coverage (abandonment) threshold — the fraction of a
    /// directory's files that must be orphaned for the truck-factor
    /// greedy removal to stop. Must be in `(0, 1)`; default
    /// `0.5` per Avelino. Ignored by `bca vcs commit`.
    #[clap(long, default_value_t = big_code_analysis::vcs::options::DEFAULT_BUS_FACTOR_THRESHOLD)]
    bus_factor_threshold: f64,
    /// Disable the persistent change-history cache for the file ranking:
    /// always walk fresh, and neither read nor write the cache. The cache
    /// otherwise reuses prior work on an unchanged tree and walks only new
    /// commits when `HEAD` has advanced.
    #[clap(long)]
    no_cache: bool,
    /// Remove this repository's cached history before ranking, forcing a
    /// full rebuild. Combine with `--no-cache` to wipe without re-priming.
    #[clap(long)]
    clear_cache: bool,
    /// Directory for the persistent history cache. Defaults to
    /// `$XDG_CACHE_HOME/big-code-analysis/vcs` (or the platform
    /// equivalent: `%LOCALAPPDATA%` on Windows, `~/.cache` otherwise).
    #[clap(long, value_parser)]
    cache_dir: Option<PathBuf>,
}

/// Subcommands of `bca vcs`: `commit` (issue #331) and `trend` (issue
/// #333); the bare `bca vcs` ranking path is the `None` case.
#[derive(Subcommand, Debug)]
enum VcsSubcommand {
    /// Score a single commit for defect-induction risk — the
    /// just-in-time (JIT) defect-prediction unit a CI gate reviews at
    /// check-in. Emits a JSON breakdown of size / diffusion /
    /// history / experience / purpose features, their contributions, and
    /// an ordinal composite score. Window / `--ref` / bot / merge /
    /// rename behaviour comes from the parent `vcs` flags. The old `jit`
    /// spelling stays a hidden alias for one release cycle.
    // commit scoring: #331; `jit`->`commit` rename: #603.
    #[command(name = "commit", alias = "jit")]
    Commit(JitArgs),
    /// Sample the change-history metrics at several points in time and
    /// emit a per-file time series, surfacing whether code is improving or
    /// degrading over the project's life. Each point re-anchors at the
    /// mainline tip of that moment, so it is a faithful historical
    /// snapshot. Window / `--ref` / bot / merge / rename / as-of
    /// (the most-recent anchor) and `--top` (files kept) come from the
    /// parent `vcs` flags.
    // Historical trend sampling landed in issue #333.
    Trend(TrendArgs),
}

/// Flags for `bca vcs commit` (issue #331). The history-window, bot, merge,
/// rename, and as-of options come from the parent [`VcsArgs`] (`--ref`
/// does not apply — the commit is named positionally); these are the
/// commit-only additions.
#[derive(Args, Debug)]
struct JitArgs {
    /// Commit / revision to score (any git revision spelling: a SHA, a
    /// tag, `HEAD`, `main~3`, …). Scored against its first parent. Mutually
    /// exclusive with `--diff`.
    #[clap(value_name = "COMMIT", default_value = "HEAD", conflicts_with = "diff")]
    commit: String,
    /// Score a `git diff` instead of a commit. Reads the diff
    /// from the given file, or from stdin when the value is `-`. The input
    /// must be a git-style unified diff with `diff --git` file headers (as
    /// produced by `git diff` / `git format-patch`); plain `diff -u` output
    /// without those headers and combined / merge diffs (`git diff --cc`,
    /// `@@@` headers) are not supported. A bare diff has no author / parent /
    /// history, so ONLY the size and diffusion groups are scored; the result
    /// is a deliberately PARTIAL report (history / experience / purpose are
    /// marked unavailable, not zero) and its score is NOT comparable to a
    /// commit score. Mutually exclusive with the positional commit spec.
    #[clap(long, value_name = "FILE")]
    diff: Option<PathBuf>,
    /// Output format (`json` default, plus `yaml` / `toml` / `cbor`).
    #[clap(long = "format", short = 'O', value_enum, default_value_t = JitFormat::default())]
    format: JitFormat,
    /// Output file. Stdout if omitted (CBOR requires this flag — it is
    /// binary).
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Pretty-print JSON / TOML output.
    #[clap(long)]
    pretty: bool,
    /// Fail the gate (exit code 2, the `check` "metric gate" convention)
    /// when the composite score is at or above this threshold. For use as
    /// a CI gate. The score is ordinal, so calibrate the threshold against
    /// the repository's own commit-score distribution. The old
    /// `--fail-over` spelling stays a hidden alias for one release cycle.
    // The `--fail-over`->`--fail-above` rename landed in issue #603.
    #[clap(
        long = "fail-above",
        alias = "fail-over",
        value_name = "SCORE",
        value_parser = parse_fail_above
    )]
    fail_above: Option<f64>,
}

/// Flags for `bca vcs trend` (issue #333). The history-window, bot, merge,
/// rename, `--ref`, and `--as-of` (the most-recent point's anchor) options
/// come from the parent [`VcsArgs`], as does `--top` (how many files to
/// keep, ranked by most-recent risk); these are the trend-only additions.
#[derive(Args, Debug)]
struct TrendArgs {
    /// Number of evenly-spaced sample points across `--span`, inclusive of
    /// both endpoints (the oldest is `as-of − span`, the newest is
    /// `as-of`). Minimum 2, maximum 120 — the cap bounds the per-point
    /// history walks on deep histories.
    // The 120 cap is `big_code_analysis::vcs::trend::MAX_TREND_POINTS`;
    // keep this prose in sync if that constant changes (validated in
    // `validate_points`).
    #[clap(long, default_value_t = 12)]
    points: usize,
    /// Total look-back window the points span (`12mo`, `2y`, `52w`, `365d`,
    /// or ISO 8601 `P1Y`). With the default 12 points this yields roughly
    /// monthly snapshots over the past year.
    #[clap(long, default_value = "12mo")]
    span: String,
    /// Show only the top N files in each improving / regressing delta
    /// summary (`0` = all).
    #[clap(long, default_value_t = 10)]
    top_deltas: usize,
    /// Output format (`json` default, plus `yaml` / `cbor`). TOML is
    /// excluded — absent points serialize as `null`, which TOML cannot
    /// represent.
    #[clap(long = "format", short = 'O', value_enum, default_value_t = TrendFormat::default())]
    format: TrendFormat,
    /// Output file. Stdout if omitted (CBOR requires this flag — it is
    /// binary).
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Pretty-print JSON output.
    #[clap(long)]
    pretty: bool,
}

/// Flags for `bca metrics`: the shared structured-output set plus an
/// opt-in to attach change-history (VCS) metrics to each file.
#[derive(Args, Debug)]
struct MetricsArgs {
    #[clap(flatten)]
    structured: StructuredArgs,
    /// Restrict computation to this comma-separated and/or repeated set
    /// of metrics (`--metrics cyclomatic,cognitive --metrics loc`).
    /// Names are the canonical ids `bca list-metrics` prints — the same
    /// vocabulary `bca check --threshold` and `bca diff --metric` use;
    /// dotted (`cyclomatic.modified`) and bare `loc` sub-metric (`sloc`)
    /// spellings are accepted too. An unknown name errors (exit 1) with a
    /// "did you mean" hint. Derived metrics auto-pull their dependencies
    /// (selecting `mi` also computes `loc` / `cyclomatic` / `halstead`).
    /// When omitted, every metric is computed.
    #[clap(long = "metrics", value_delimiter = ',', action = clap::ArgAction::Append, value_name = "NAME")]
    metrics: Vec<String>,
    /// Also compute change-history (VCS) metrics and attach a `vcs`
    /// block — plus a `hotspot_score` (cyclomatic × recent churn) — to
    /// each file's metrics. Uses default windows (12mo / 90d, weighted
    /// formula); for window / formula tuning use `bca vcs`. Outside a
    /// git working tree this opt-in is skipped with a warning (the AST
    /// metrics still emit, without the `vcs` block).
    #[clap(long)]
    vcs: bool,
    /// Additionally attach a `vcs` block to every nested function /
    /// method / class space, via `git blame` of each file's surviving
    /// lines. Implies `--vcs`. The per-function numbers are
    /// a current-blame snapshot — `churn` is surviving-line count, not
    /// the file-level added+deleted churn — so they rank functions
    /// within a file but are not directly comparable to the file block.
    /// Costs one blame per file; skipped (with a warning) outside a git
    /// working tree.
    #[clap(long = "vcs-per-function")]
    vcs_per_function: bool,
}

#[derive(Args, Debug)]
struct ReportArgs {
    // `report` keeps its deprecated positional FORMAT (below) working
    // for one more cycle, so it takes `--paths` for input selection
    // rather than a positional `[PATHS]` (#651): a trailing path Vec
    // would be ambiguous against the scalar FORMAT positional.
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    #[clap(flatten)]
    preproc: PreprocConsumeArgs,
    /// Report format (`markdown` or `html`). Defaults to `markdown`
    /// when neither this flag nor the deprecated positional form is
    /// given.
    #[clap(long = "format", short = 'O', value_enum)]
    format: Option<ReportFormat>,
    /// Deprecated positional form of the report format, kept working
    /// for one release cycle. Hidden from help; use `--format`/`-O`
    /// instead. The flag wins when both are given. To be removed in 2.0.
    // The `--format`/`-O` flag superseded the positional form in issue #513.
    #[clap(value_enum, hide = true, value_name = "FORMAT")]
    format_positional: Option<ReportFormat>,
    /// Output file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Maximum number of entries per hotspot table (`0` = all).
    #[clap(long, default_value_t = 20)]
    top: usize,
    /// Path prefix to strip from displayed file paths.
    #[clap(long, default_value = "")]
    strip_prefix: String,
    /// Include functions silenced by in-source suppression markers
    /// (`bca: suppress`, `bca: suppress-file`, `#lizard forgives`) in the
    /// hotspot tables. By default the report honors these markers and
    /// omits a function from a metric's hotspot table when that metric is
    /// suppressed for it — matching `bca check` and the SARIF emitter.
    /// Pass this for the raw audit view that lists every offender.
    /// Value-taking: a bare `--no-suppress` means `true`;
    /// `--no-suppress=false` forces the marker-honoring default even when
    /// the `[report] no_suppress` key in `bca.toml` enabled it. The CLI
    /// value overrides the manifest in either direction.
    #[clap(
        long = "no-suppress",
        value_name = "BOOL",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        action = clap::ArgAction::Set
    )]
    no_suppress: Option<bool>,
    /// Append a "Change-history risk" section ranking files by VCS risk
    /// (churn, authorship, fix history) using default windows, mirroring
    /// `bca metrics --vcs`. Ignored (with a warning) outside a git
    /// working tree. See `bca vcs` for the standalone, tunable report.
    #[clap(long)]
    vcs: bool,
}

impl ReportArgs {
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &PositionalPaths::default(),
            &self.tuning,
            &self.preproc,
            &OutputArgs::default(),
            universal,
        )
    }

    /// Resolve the effective report format. The `--format`/`-O` flag
    /// wins over the deprecated positional form; with neither present
    /// the default is Markdown (issue #513).
    fn resolved_format(&self) -> ReportFormat {
        self.format
            .or(self.format_positional)
            .unwrap_or(ReportFormat::Markdown)
    }

    /// The user's seed paths (the `--paths`/`-p` values, after any manifest
    /// merge), for the provenance footer (issue #680). `report` has no
    /// positional `[PATHS]` group (`to_globals` passes a default), so the
    /// selection's `paths` are the whole seed set; an empty list means the
    /// implicit current-directory default.
    fn seed_paths(&self) -> &[PathBuf] {
        &self.selection.paths
    }
}

/// Node-type selection for `find` / `count` (#651). The node kinds moved
/// off a `<NODES>...` positional onto a repeatable `-t`/`--type` flag so
/// the positional slot is free for input `[PATHS]`, matching every other
/// walking subcommand. At least one `-t` is required (`bca find` with no
/// `-t` is a usage error).
#[derive(Args, Debug)]
struct NodeTypesArgs {
    /// Node-type name to match. Repeat the flag to match several
    /// (`-t function_item -t struct_item`). Required: pass at least one.
    #[clap(
        long = "type",
        short = 't',
        required = true,
        action = clap::ArgAction::Append,
        value_name = "NODE_TYPE"
    )]
    types: Vec<String>,
}

/// Line-range bounds for the `dump` and `find` subcommands. Scoped
/// to those two commands rather than `global` (issue #518): every other
/// subcommand silently ignored the range, and the cryptic `--ls`/`--le`
/// names cluttered all of their help. The descriptive `--line-start` /
/// `--line-end` are canonical; `--ls` / `--le` survive as hidden
/// deprecated aliases for one release cycle (mirrors the #513
/// `--output-format` deprecation) and are slated for removal in 2.0.
#[derive(Args, Debug, Default)]
struct LineRange {
    /// First line of the range to analyze (1-based, inclusive).
    #[clap(long = "line-start", alias = "ls")]
    line_start: Option<usize>,
    /// Last line of the range to analyze (1-based, inclusive).
    #[clap(long = "line-end", alias = "le")]
    line_end: Option<usize>,
}

/// Arguments for the `dump` subcommand: the [`LineRange`] bounds plus the
/// walk / tuning / preproc / output groups and positional `[PATHS]`.
#[derive(Args, Debug)]
struct DumpArgs {
    #[clap(flatten)]
    line: LineRange,
    #[clap(flatten)]
    positional: PositionalPaths,
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    #[clap(flatten)]
    preproc: PreprocConsumeArgs,
    #[clap(flatten)]
    out: OutputArgs,
}

/// Arguments for the `find` subcommand: the `-t`/`--type` node filters,
/// the [`LineRange`] bounds, and the walk / tuning / preproc / output
/// groups plus positional `[PATHS]`.
#[derive(Args, Debug)]
struct FindArgs {
    #[clap(flatten)]
    nodes: NodeTypesArgs,
    #[clap(flatten)]
    line: LineRange,
    #[clap(flatten)]
    positional: PositionalPaths,
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    #[clap(flatten)]
    preproc: PreprocConsumeArgs,
    #[clap(flatten)]
    out: OutputArgs,
}

/// Arguments for the `count` subcommand: the `-t`/`--type` node filters
/// and the walk / tuning / preproc groups plus positional `[PATHS]`.
/// `count` prints a single tally, so it carries no `--color` output
/// group.
#[derive(Args, Debug)]
struct CountArgs {
    #[clap(flatten)]
    nodes: NodeTypesArgs,
    #[clap(flatten)]
    positional: PositionalPaths,
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    #[clap(flatten)]
    preproc: PreprocConsumeArgs,
}

/// Arguments for the `functions` subcommand (#597): no command-specific
/// flags, just the shared walk / tuning / preproc / output groups and
/// positional `[PATHS]`.
#[derive(Args, Debug)]
struct FunctionsArgs {
    #[clap(flatten)]
    positional: PositionalPaths,
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    #[clap(flatten)]
    preproc: PreprocConsumeArgs,
    #[clap(flatten)]
    out: OutputArgs,
}

impl DumpArgs {
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &self.positional,
            &self.tuning,
            &self.preproc,
            &self.out,
            universal,
        )
    }
}

impl FindArgs {
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &self.positional,
            &self.tuning,
            &self.preproc,
            &self.out,
            universal,
        )
    }
}

impl CountArgs {
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &self.positional,
            &self.tuning,
            &self.preproc,
            &OutputArgs::default(),
            universal,
        )
    }
}

impl FunctionsArgs {
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &self.positional,
            &self.tuning,
            &self.preproc,
            &self.out,
            universal,
        )
    }
}

#[derive(Args, Debug)]
struct StripCommentsArgs {
    #[clap(flatten)]
    positional: PositionalPaths,
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    #[clap(flatten)]
    preproc: PreprocConsumeArgs,
    /// Rewrite each input file in place instead of writing to stdout.
    /// Use this for multi-file rewrites; it is mutually exclusive with
    /// `--output`.
    #[clap(long)]
    in_place: bool,
    /// Write the stripped output to this file instead of stdout.
    /// Requires a single input file — a multi-file run is rejected (use
    /// `--in-place` for that). Mutually exclusive with `--in-place`. Omit
    /// it (and `--in-place`) to stream the result to stdout.
    #[clap(
        long = "output",
        short = 'o',
        value_parser,
        conflicts_with = "in_place"
    )]
    output: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct PreprocArgs {
    #[clap(flatten)]
    positional: PositionalPaths,
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    /// Output JSON file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
}

impl PreprocArgs {
    /// Assemble the runtime [`GlobalOpts`] for the producing walk.
    /// `preproc` produces preprocessor data rather than consuming it, so
    /// the preproc-consume group is omitted (defaulted).
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &self.positional,
            &self.tuning,
            &PreprocConsumeArgs::default(),
            &OutputArgs::default(),
            universal,
        )
    }
}

impl StripCommentsArgs {
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &self.positional,
            &self.tuning,
            &self.preproc,
            &OutputArgs::default(),
            universal,
        )
    }
}

impl MetricsArgs {
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        self.structured.to_globals(universal)
    }
}

#[derive(Args, Debug)]
struct CheckArgs {
    #[clap(flatten)]
    positional: PositionalPaths,
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    #[clap(flatten)]
    preproc: PreprocConsumeArgs,
    /// Threshold expressed as `<metric>=<limit>`. Repeatable. Metric
    /// names match `bca list-metrics`; sub-metrics use a dotted form
    /// (e.g. `loc.lloc`, `halstead.volume`). The bare `bca diff --metric`
    /// spelling of a `loc` sub-metric is accepted as an alias (`sloc` ==
    /// `loc.sloc`); a bare family head with no single scalar (`halstead`,
    /// `mi`) is rejected with a "did you mean" hint. CLI flags override
    /// values from `--config`. Limits must be finite and non-negative;
    /// `0` is allowed and means "no value permitted".
    #[clap(long = "threshold", value_parser = parse_cli_threshold)]
    thresholds: Vec<(String, f64)>,
    /// Path to a TOML config with a `[thresholds]` table; CLI
    /// `--threshold` flags override values read from it.
    // The indented example lives in `long_help`, not the `///` doc
    // comment: an indented block in a doc comment is compiled by rustdoc
    // as a Rust doctest and the TOML `[thresholds]` then fails to parse
    // (#608). `long_help` feeds clap's `--help` verbatim while the
    // rustdoc-visible doc comment stays plain prose. A ```toml fence is
    // not an option — clap renders the fence markers literally into help.
    #[clap(
        long,
        value_parser,
        long_help = "\
Path to a TOML config with a `[thresholds]` table. Example:

    [thresholds]
    cyclomatic = 15
    \"loc.lloc\" = 200

CLI `--threshold` flags override values read from this file."
    )]
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
    /// Surface suppressed debt in the offender document instead of
    /// dropping it. Offenders silenced by an in-source `bca: suppress`
    /// marker or covered by the baseline are still kept out of the gate
    /// (exit code and human stream are unaffected), but are emitted into
    /// the `--format sarif` document carrying a SARIF
    /// `suppressions` entry — GitHub Code Scanning renders them as
    /// suppressed (closed) alerts so the debt stays visible. Only the
    /// SARIF format represents suppression; other formats ignore the
    /// flag. Mutually exclusive with `--no-suppress` (which un-silences
    /// markers) and `--write-baseline`.
    #[clap(long = "report-suppressed", conflicts_with_all = ["no_suppress", "write_baseline"])]
    report_suppressed: bool,
    /// CI/IDE *report dialect* for offender records (Checkstyle 4.3 XML,
    /// SARIF 2.1.0 JSON, GitLab Code Climate JSON, clang/GCC warning
    /// lines, MSVC warning lines). Named `--report-format` to separate
    /// "which CI report dialect" from the data-serialization `--format`
    /// the structured subcommands use. When omitted *and*
    /// `--output` is also omitted, only the human-readable stderr stream
    /// is emitted; the exit-code contract is unaffected. When omitted but
    /// `--output` is given, the dialect is inferred from the output
    /// extension (`.sarif` → sarif, `.xml` → checkstyle); an extension
    /// with no unique dialect is a usage error. The old `--format` / `-O`
    /// / `--output-format` spellings stay hidden aliases for one release
    /// cycle and are slated for removal in 2.0.
    // `--report-format` split from the data `--format` in issue #659; the
    // `--format`/`-O`/`--output-format` aliases trace to issues #513/#659.
    #[clap(
        long = "report-format",
        alias = "format",
        alias = "output-format",
        short_alias = 'O',
        value_name = "FORMAT",
        value_enum
    )]
    output_format: Option<AggregatedFormat>,
    /// File path for the aggregated offender document. Stdout if omitted.
    /// When `--report-format` is also omitted, the dialect is inferred
    /// from this path's extension (`.sarif` → sarif, `.xml` → checkstyle);
    /// a path with no dialect-bearing extension is rejected rather than
    /// silently ignored. Parent directories are created on demand.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Filter known offenders listed in this TOML baseline. A baselined
    /// function whose metric value has not worsened is suppressed; a
    /// worsened value (or any new offender) still fails. See the
    /// "Baselines" recipe in the book for the full adoption flow.
    #[clap(long = "baseline", value_parser, conflicts_with = "write_baseline")]
    baseline: Option<PathBuf>,
    /// Walk the tree and write the current offender set to a baseline
    /// file instead of failing. The resulting file pins today's metric
    /// values as the baseline; subsequent `--baseline <path>` runs
    /// ratchet down from there. Takes an optional path: `--write-baseline
    /// <path>` writes there; a bare `--write-baseline` (no value) writes
    /// to the `[check] baseline` key from the auto-discovered `bca.toml`
    /// manifest (errors if no manifest baseline is set). Conflicts with
    /// `--baseline`, `--report-format`, `--output`, `--since`, and
    /// `--changed-only` — diff-scope filtering would write a *partial*
    /// baseline that the next non-`--changed-only` run would treat as a
    /// complete snapshot, silently masking every offender outside the
    /// diff scope.
    #[clap(
        long = "write-baseline",
        value_parser,
        num_args = 0..=1,
        value_name = "PATH",
        conflicts_with_all = ["baseline", "output_format", "output", "since", "changed_only"],
    )]
    // The three states are meaningful and distinct: `None` (flag absent),
    // `Some(None)` (bare `--write-baseline`, resolve from the manifest
    // `baseline` key), and `Some(Some(path))` (explicit path). This is the
    // canonical clap idiom for an optional-value flag, so the
    // `option_option` lint does not apply.
    #[allow(clippy::option_option)]
    write_baseline: Option<Option<PathBuf>>,
    /// Skip the trailing per-file rollup footer. The footer groups
    /// violations by file and cites the single worst-ratio metric per
    /// file. Pass this when downstream tooling grep-pipes the stderr
    /// stream and would be confused by the trailing summary block.
    /// Default: footer enabled.
    #[clap(long = "no-summary")]
    no_summary: bool,
    /// Git ref to diff `HEAD` against. The set of files reported by
    /// `git diff --name-only <ref>...HEAD` is surfaced first in the
    /// summary footer under "Files in this range:", so a reader
    /// scanning a CI log sees their own contributions before the
    /// legacy offender list. Defaults to auto-detection from
    /// `BCA_DIFF_BASE`, `GITHUB_BASE_REF` (PR runs), or
    /// `GITHUB_EVENT_BEFORE` (push runs), in that precedence.
    #[clap(long = "since")]
    since: Option<String>,
    /// Drop violations from files outside the `--since`/auto-detected
    /// touched set entirely (terser CI output for PR gates). Requires
    /// a resolvable diff base, either via `--since` or one of the
    /// auto-detected env vars; failing to resolve is fatal so a
    /// misconfigured CI does not silently turn the gate into a no-op.
    #[clap(long = "changed-only")]
    changed_only: bool,
    /// Emit GitHub Actions `::error file=…,line=…,title=…::msg`
    /// workflow commands per violation so the GHA UI renders them as
    /// inline annotations on the file-diff view. Additive to the
    /// human-readable stderr stream — annotations ride on top, they
    /// don't replace it. Tri-state `<auto|always|never>` mirroring
    /// `--color`: `auto` (default) emits annotations when
    /// `$GITHUB_ACTIONS == "true"`; `always` forces them on; `never`
    /// suppresses them even inside a GHA step (so a workflow that runs
    /// `bca check` twice can annotate from only one run). A bare
    /// `--github-annotations` means `always`. Capped at 10 per metric
    /// (GitHub Actions surfaces at most 10 errors per step in the UI);
    /// overflow rolls up to one `::error::N more <metric> violations not
    /// shown` line per affected metric so the count is still visible.
    #[clap(
        long = "github-annotations",
        value_name = "auto|always|never",
        value_enum,
        default_value_t = CiDetect::Auto,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "always"
    )]
    github_annotations: CiDetect,
    /// Where to append a markdown digest of the violations — per-file
    /// rollup table, per-metric breakdown, and top-N offenders by
    /// `value / limit` ratio. Mirrors the format `bca report markdown`
    /// produces so a reader skimming the GHA step-summary panel sees a
    /// familiar table layout. Accepts a file path, or the keywords
    /// `auto` / `never`: `auto` (the default when the flag
    /// is omitted) appends to `$GITHUB_STEP_SUMMARY` when that env var is
    /// set; `never` suppresses the digest even inside a GHA step; a path
    /// appends there unconditionally. The block is bracketed by
    /// HTML-comment markers so a retried step replaces (not stacks) the
    /// previous digest.
    #[clap(long = "summary-file", value_name = "PATH|auto|never")]
    summary_file: Option<SummaryFile>,
    /// Suppress the trailing "--- next steps ---" remediation block
    /// that names the artifact, prints a copy-paste-safe
    /// `--write-baseline` refresh invocation, and links to the
    /// Baselines recipe. By default the block is emitted on
    /// failure (and in `$GITHUB_STEP_SUMMARY` when present) so a
    /// first-time reader of a failing CI log can see what to do
    /// next without leaving the page.
    #[clap(long = "no-remediation")]
    no_remediation: bool,
    /// Print the resolved threshold/check configuration (after
    /// merging `--config` TOML + `--threshold` CLI overrides) to
    /// stdout, then exit 0 without walking the codebase. Default
    /// format is TOML; pass `=json` for JSON. Mutually exclusive
    /// with `--write-baseline` — this flag is a read-only debug
    /// aid, not a side-effecting operation. Output is
    /// round-trippable: piping it back through `--config` produces
    /// the same effective view.
    #[clap(
        long = "print-effective-config",
        value_enum,
        num_args = 0..=1,
        default_missing_value = "toml",
        value_name = "FORMAT",
        conflicts_with = "write_baseline",
    )]
    print_effective_config: Option<PrintConfigFormat>,
    /// Which threshold tier to gate against. Accepts
    /// `hard`, `soft`, or `soft=<RATIO>`:
    ///
    /// - `hard` (default) — flag a function only when a metric is at or
    ///   over its `[thresholds]` limit.
    /// - `soft` — early-warning tier: flag a function when a metric
    ///   reaches `RATIO` (default 0.95) of any limit, i.e. before the
    ///   hard gate trips. With a `[thresholds.soft]` table present, the
    ///   per-metric soft limits take precedence over the blanket ratio
    ///   (metrics absent from it inherit their hard limit).
    /// - `soft=0.90` — soft tier scaling every limit by 0.90; `soft=1.0`
    ///   disables the blanket scale (a soft tier driven only by an
    ///   explicit `[thresholds.soft]` table).
    ///
    /// Resolution order: `[thresholds]` (manifest + `--config`) →
    /// `[thresholds.soft]` or the soft ratio → absolute
    /// `--threshold name=value` overrides (applied last, never scaled).
    /// Both tiers ratchet through the same `--baseline`. `RATIO` must
    /// lie in `(0, 1]`; an out-of-range value is a usage error.
    #[clap(
        long = "tier",
        value_name = "hard|soft|soft=RATIO",
        default_value = "hard",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "soft"
    )]
    tier: TierSpec,
    /// Deprecated alias for `--tier=soft=<RATIO>`. Retained
    /// for one release cycle; pass `--tier=soft=<RATIO>` instead. When
    /// `--tier` is left at its `hard` default, `--headroom <R>` is
    /// promoted to `--tier=soft=<R>` with a deprecation warning; passing
    /// both `--headroom` and an explicit `--tier=soft=<R>` is a conflict.
    // The `--tier` gate and its `--headroom` alias landed in issue #688.
    #[clap(long = "headroom", value_name = "RATIO", hide = true)]
    headroom: Option<f64>,
    /// Exit-code style: `default` keeps the stable
    /// 0/1/2 contract; `tiered` splits exit `2` by severity so CI can
    /// branch without parsing the `[new]` / `[regr +N%]` stderr tags:
    ///
    /// - `0` — clean.
    /// - `1` — tool error (bad config, unknown metric, unreadable path).
    /// - `2` — new offenders only (no baseline entry matched).
    /// - `3` — regressions only (a baselined offender worsened).
    /// - `4` — both new offenders and regressions.
    /// - `5` — at least one `--tier=soft` violation also breaches the
    ///   hard limit (more urgent than soft-band encroachment). Only
    ///   emitted at the soft tier; at the hard tier every violation is a
    ///   hard breach by definition, so the 2/3/4 split is used instead.
    ///
    /// Every fail-state stays non-zero, so existing `exit != 0 → fail`
    /// tooling is unaffected; only consumers that test `$? -eq 2`
    /// explicitly need to widen to 2-5. `--no-fail` still forces exit
    /// `0`. Mirrors the `[check] exit_codes` key in `bca.toml`; the CLI
    /// value overrides the manifest in either direction.
    #[clap(
        long = "exit-codes",
        value_name = "default|tiered",
        value_enum,
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "tiered"
    )]
    exit_codes: Option<ExitCodes>,
    /// Deprecated alias for `--exit-codes=tiered`. Retained
    /// for one release cycle; pass `--exit-codes=tiered` instead.
    // The `--exit-codes` flag and the tiered split trace to issues #385/#666.
    #[clap(long = "strict-exit-codes", hide = true, conflicts_with = "exit_codes")]
    strict_exit_codes: bool,
    /// Tolerance, in lines, for matching a `--baseline` entry whose
    /// qualified symbol is ambiguous (two methods with the same name on
    /// different `impl` blocks, overloads, collided anonymous spaces).
    /// Unambiguous symbols match regardless of line drift; this only
    /// disambiguates a tie. Defaults to 50. Mirrored by the
    /// `baseline_line_tolerance` key in `bca.toml`.
    #[clap(long = "baseline-line-tolerance", value_name = "LINES")]
    baseline_line_tolerance: Option<usize>,
    /// Enable rename-tolerant baseline matching: when a `--baseline`
    /// entry's qualified symbol no longer matches but the function body
    /// is unchanged (a rename that kept the shape), match on a
    /// normalised body hash instead. Off by default. The hash is also
    /// written into the baseline by `--write-baseline` when this flag is
    /// set, so populate it once with a fuzzy write to enable fuzzy reads.
    /// Value-taking: a bare `--baseline-fuzzy-match` means
    /// `true`; `--baseline-fuzzy-match=false` forces it off even when the
    /// `baseline_fuzzy_match` key in `bca.toml` set it. The CLI value
    /// overrides the manifest in either direction.
    #[clap(
        long = "baseline-fuzzy-match",
        value_name = "BOOL",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "true",
        action = clap::ArgAction::Set
    )]
    baseline_fuzzy_match: Option<bool>,
    /// Glob for files to analyse and report but exempt from the
    /// threshold gate. Repeatable. Matching files are still walked,
    /// parsed, metric'd, and shown by `bca report`; `bca check` simply
    /// drops their violations before emitting offenders and before
    /// `--write-baseline` records anything — so structural exemptions
    /// (test fixtures, generated code, macro-dispatch modules) stay out
    /// of `.bca-baseline.toml`. Precedence: in-source `bca: suppress`
    /// markers win first, then these globs, then the baseline. Unioned
    /// with `--check-exclude-from` and with the `bca.toml` `[check]
    /// exclude` list: CLI values are
    /// *added to* (merged with), not a replacement for, the manifest's
    /// exemptions, so a CLI `--check-exclude` never silently re-gates a
    /// path the project config deliberately exempted. Pass `--no-config`
    /// to ignore the manifest entirely. Globs match the path as walked,
    /// exactly like `--exclude`.
    #[clap(long = "check-exclude", value_name = "GLOB")]
    check_exclude: Vec<String>,
    /// Read newline-separated `--check-exclude` globs from a file (one
    /// per line, `.gitignore`-style: blank lines and `#`-comments are
    /// skipped). Use `-` for stdin; to pass a file literally named `-`,
    /// use `./-`. Unioned with any `--check-exclude` values. Convention
    /// is a `.bcacheckignore` at the repo root, mirroring `.bcaignore`
    /// for the walker. Mirrored by the `[check] exclude_from` key in
    /// `bca.toml`.
    #[clap(long = "check-exclude-from", value_parser)]
    check_exclude_from: Option<PathBuf>,
}

impl CheckArgs {
    /// Assemble the runtime [`GlobalOpts`] for the check walk. `check`
    /// emits no colorized text dump, so the output group is defaulted.
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &self.positional,
            &self.tuning,
            &self.preproc,
            &OutputArgs::default(),
            universal,
        )
    }

    /// Resolve the effective [`TierSpec`], folding the deprecated
    /// `--headroom <R>` alias into `--tier=soft=<R>` (issue #688). When
    /// `--tier` is left at its `hard` default and `--headroom` is given,
    /// the headroom value promotes the gate to the soft tier with a
    /// one-cycle deprecation warning. Passing both `--headroom` and an
    /// explicit `--tier=soft=<R>` is a usage error (clap can't express
    /// the conflict because `--tier` always has a default, so it is
    /// rejected here).
    fn resolved_tier(&self) -> TierSpec {
        let Some(ratio) = self.headroom else {
            // No alias: the parsed (or manifest-folded) `--tier` wins.
            return self.tier;
        };
        warn_deprecated_flag("--headroom <R>", "--tier=soft=<R>");
        // Range-validate the alias ratio here (exit 1, tool error) — clap
        // does not parse `--headroom` through `TierSpec`, so the `(0, 1]`
        // check the canonical form gets at parse time must be replicated.
        if !crate::thresholds::is_valid_scale_ratio(ratio) {
            die(format_args!("--headroom must be in (0, 1]; got {ratio}"));
        }
        match self.tier {
            // `--headroom` on its own, or alongside a bare `--tier=soft`,
            // resolves to `soft=<ratio>` — headroom IS the soft ratio.
            TierSpec::Hard | TierSpec::Soft(None) => TierSpec::Soft(Some(ratio)),
            // An explicit `--tier=soft=<R>` AND `--headroom <R>` give two
            // ratios for the same dial: ambiguous, so reject it.
            TierSpec::Soft(Some(_)) => {
                die("--headroom is the deprecated alias for `--tier=soft=<R>`; \
                 pass one or the other, not both")
            }
        }
    }

    /// Resolve the effective [`ExitCodes`] style after folding the
    /// deprecated `--strict-exit-codes` alias into `--exit-codes=tiered`
    /// (issue #666). `clap`'s `conflicts_with` already rejects passing
    /// both, so at most one is set. Returns `None` when neither was
    /// given on the CLI, so the manifest `[check] exit_codes` value can
    /// fill in (the CLI value otherwise overrides the manifest in either
    /// direction).
    fn resolved_exit_codes(&self) -> Option<ExitCodes> {
        if self.strict_exit_codes {
            warn_deprecated_flag("--strict-exit-codes", "--exit-codes=tiered");
            return Some(ExitCodes::Tiered);
        }
        self.exit_codes
    }
}

/// Arguments for the `init` subcommand. Scaffolds the consolidated
/// `bca.toml` manifest (`paths`, `exclude_from`, `baseline`, and a
/// `[thresholds]` table) plus the `.bcaignore` and `.bca-baseline.toml`
/// files the manifest references, in the target directory. With the
/// manifest in place, a bare `bca check` auto-discovers it and runs the
/// gate zero-config. Interactive prompts and `--emit
/// make/just/pre-commit/github-actions` skeletons are deliberately
/// scoped out of the initial cut — they are tracked against #379 for a
/// follow-up.
#[derive(Args, Debug, Default)]
struct InitArgs {
    // `init` scaffolds into `--dir` (default `.`) and walks it to
    // generate the baseline, so it consumes the walk / tuning / preproc
    // groups (#597) but names its target via `--dir`, not a positional
    // `[PATHS]`.
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    #[clap(flatten)]
    preproc: PreprocConsumeArgs,
    /// Directory to scaffold into. Defaults to the current working
    /// directory. The directory must already exist; `init` will not
    /// create the project root itself.
    #[clap(long, value_parser)]
    dir: Option<PathBuf>,
    /// Overwrite any of the canonical files that already exist.
    /// Default: refuse to clobber, listing which files block.
    #[clap(long)]
    force: bool,
    /// Skip the baseline-generation pass. The written
    /// `.bca-baseline.toml` is then an empty placeholder; the user
    /// can populate it later with
    /// `bca check --write-baseline .bca-baseline.toml`.
    /// Default: walk the target directory and pin today's offenders.
    #[clap(long = "no-baseline")]
    no_baseline: bool,
}

impl InitArgs {
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &PositionalPaths::default(),
            &self.tuning,
            &self.preproc,
            &OutputArgs::default(),
            universal,
        )
    }
}

/// Shared `text`/`markdown`/`json` output style for the read-only
/// reporting commands (`bca diff`, `bca diff-baseline`, `bca
/// exemptions`).
///
/// `Text` (default) is the human, column-aligned form. `Markdown` wraps
/// each section in tables / fenced blocks so the output drops cleanly
/// into a sticky PR comment. `Json` emits the complete structured form
/// for tooling — and deliberately ignores any `--*-only` filters, since
/// a machine consumer reads the field it wants from a stable schema.
///
/// The human value is `text` to match the one human-format vocabulary
/// used across `metrics` / `ops` / `vcs` (#659). The former `tty`
/// spelling stays a hidden alias for one release cycle.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
enum OutputFormat {
    #[default]
    #[value(alias = "tty")]
    Text,
    Markdown,
    Json,
}

/// Arguments for the `diff-baseline` subcommand (issue #382). Takes two
/// baseline files and reports the structured difference between them.
///
/// Both files are read through the same loader `bca check` uses, so any
/// supported legacy version (v2/v3) is accepted and migrated on read; an
/// unsupported version is a hard error rather than a silent no-match.
///
/// The `--*-only` flags are combinable section filters for the TTY and
/// Markdown forms; `--format json` always emits every bucket.
///
/// Entries pair on the path key as stored — each baseline's paths are
/// canonicalised relative to that file's own directory (its anchor), so
/// the diff is apples-to-apples only when both files share an anchor.
/// The documented refresh flow keeps them in the same directory
/// (`cp .bca-baseline.toml .bca-baseline.old.toml`); diffing two
/// baselines that sit at different depths relative to the source tree
/// can show a moved function as a remove + add.
#[derive(Args, Debug)]
struct DiffBaselineArgs {
    /// Old (base) baseline file — the "before" side of the diff.
    #[clap(value_parser)]
    old: PathBuf,
    /// New (updated) baseline file — the "after" side of the diff.
    #[clap(value_parser)]
    new: PathBuf,
    /// Output style: `text` (default), `markdown`, or `json`.
    #[clap(long, short = 'O', value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    /// Render only the "Added" section (combinable with the other
    /// `--*-only` flags). Ignored by `--format json`.
    #[clap(long = "added-only")]
    added_only: bool,
    /// Render only the "Removed" section. Ignored by `--format json`.
    #[clap(long = "removed-only")]
    removed_only: bool,
    /// Render only the "Worsened" section. Ignored by `--format json`.
    #[clap(long = "worsened-only")]
    worsened_only: bool,
    /// Render only the "Improved" section. Ignored by `--format json`.
    #[clap(long = "improved-only")]
    improved_only: bool,
    /// Output file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Path prefix to strip from displayed file paths in the TTY and
    /// Markdown per-file tables. No-op for `--format json`, whose paths
    /// are a stable machine identity.
    #[clap(long, default_value = "")]
    strip_prefix: String,
    /// Exit with the metric-gate code (`2`) when the diff — after the
    /// active `--*-only` section filtering — is non-empty; exit `0` when
    /// it is empty. Opt-in (`git diff --exit-code`-style) for grammar-bump
    /// CI that wants a boolean "anything changed" without parsing the
    /// output. Default (flag absent) always exits `0` on success. A tool
    /// error still exits `1` regardless.
    #[clap(long = "exit-code")]
    exit_code: bool,
}

/// Arguments for the `diff` subcommand (issue #487, `--since` from
/// #492). Reports per-metric, per-file deltas plus added/removed files.
///
/// Two input modes:
/// - File/dir mode: two positional metric-output sets (`<old> <new>`),
///   each a per-file JSON file or a directory tree of them.
/// - `--since <ref> [<new>]`: analyze the tree at `<ref>` for the
///   before side; the after side is the optional `<new>` source tree or
///   the current working tree.
///
/// `--format json` always emits every bucket; `--min-change` and
/// `--metric` shape which deltas are reported. Exits 0 on success by
/// default; the diff is informational, not a gate. With `--exit-code`,
/// exits 2 when the filtered diff is non-empty.
#[derive(Args, Debug)]
struct DiffArgs {
    // `diff --since` walks both trees, so it consumes the walk-selection
    // and tuning flag groups (#597). Its positional slots are already
    // spent on the `<old> <new>` metric-output sets (the `--since`-mode
    // `<old>` doubles as a relative path scope), so it takes no
    // positional `[PATHS]` (#651) — selection is via `--paths` / globs.
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    /// In file/dir mode: the old (base) metric output — the "before"
    /// side (a per-file JSON file or a directory of per-file JSON).
    /// In `--since` mode: an optional *relative path scope* (a
    /// subdirectory or file), equivalent to `--paths`, applied to both
    /// sides — so `bca diff --since HEAD src` diffs only the `src`
    /// subtree. It is a scope, never an alternate root: both sides stay
    /// rooted at the repo top, so the keys always line up. Must be
    /// relative (an absolute path is rejected); omit it to diff the
    /// whole tree.
    #[clap(value_parser)]
    old: Option<PathBuf>,
    /// In file/dir mode: the new (after) metric output (a per-file JSON
    /// file or a directory of them). Not used in `--since` mode, where
    /// the after side is always the working tree and the single
    /// positional is a path scope — a second positional with `--since`
    /// is rejected at runtime.
    #[clap(value_parser)]
    new: Option<PathBuf>,
    /// Analyze the tree at this git ref for the "before" side instead of
    /// reading a captured metric set. Hard-errors (exit 1) if the
    /// process is not in a git checkout, `git` is missing, or the ref
    /// does not resolve. Honors the same `--paths` / `--include` /
    /// `--exclude` selection as the rest of the CLI so both sides
    /// analyze the same file set.
    ///
    /// In this mode the before side comes from `<ref>` and the after
    /// side is the working tree, so at most one positional is accepted
    /// and it is a relative path *scope* (equivalent to `--paths`)
    /// applied to both sides; omit it to diff the whole tree. Passing
    /// two positionals with `--since` is rejected at runtime.
    #[clap(long)]
    since: Option<String>,
    /// Output style: `text` (default), `markdown`, or `json`.
    #[clap(long, short = 'O', value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    /// Minimum absolute change for a per-file metric delta to be
    /// reported. `0` (the default) reports any change of any size;
    /// raise it to suppress small deltas and surface only the larger
    /// metric movements (e.g. on a grammar bump).
    #[clap(long = "min-change", default_value_t = 0.0)]
    min_change: f64,
    /// Restrict the diff to one or more metrics (repeatable). Names are
    /// those printed by `bca list-metrics` (e.g. `cyclomatic`, `sloc`).
    /// The dotted `bca check --threshold` spelling is accepted as an
    /// alias (`loc.sloc` == `sloc`, `halstead.volume` == `halstead`).
    /// When omitted, every metric is reported.
    #[clap(long = "metric")]
    metric: Vec<String>,
    /// Output file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Path prefix to strip from displayed file paths in the TTY and
    /// Markdown per-file tables. No-op for `--format json`, whose paths
    /// are a stable machine identity.
    #[clap(long, default_value = "")]
    strip_prefix: String,
    /// Exit with the metric-gate code (`2`) when the diff is non-empty in
    /// any section the active `--metric` / `--min-change` filtering keeps;
    /// exit `0` when it is empty. Opt-in (`git diff --exit-code`-style) for
    /// grammar-bump CI that wants a boolean "anything changed". Default
    /// (flag absent) always exits `0` on success. A tool error still exits
    /// `1` regardless.
    #[clap(long = "exit-code")]
    exit_code: bool,
}

impl DiffArgs {
    /// Assemble the runtime [`GlobalOpts`] for the `--since` walk. `diff`
    /// consumes no preprocessor data and renders no colorized dump, so
    /// those groups default. Its `<old>` positional path scope is folded
    /// in by `side_globals`, not here.
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &PositionalPaths::default(),
            &self.tuning,
            &PreprocConsumeArgs::default(),
            &OutputArgs::default(),
            universal,
        )
    }
}

/// Arguments for the `exemptions` subcommand (issue #386). Audits the
/// three gate-skipping tiers — in-source markers, `[check.exclude]`
/// globs, and `.bca-baseline.toml` entries — in one report.
///
/// The `--*-only` flags are mutually exclusive section selectors for
/// PR-bot specialisation; omitting them reports all three. The old
/// `--only-*` spellings remain as hidden aliases for one cycle. The
/// baseline and `[check.exclude]` inputs default to the same sources
/// `bca check` reads (`bca.toml` `[check]` table), so the audit
/// reflects exactly what the gate would skip.
#[derive(Args, Debug)]
struct ExemptionsArgs {
    #[clap(flatten)]
    positional: PositionalPaths,
    #[clap(flatten)]
    selection: WalkSelectionArgs,
    #[clap(flatten)]
    tuning: WalkTuningArgs,
    #[clap(flatten)]
    preproc: PreprocConsumeArgs,
    /// Output style: `text` (default), `markdown`, or `json`. JSON nests
    /// all three sections under a single `suppressions` envelope.
    #[clap(long, short = 'O', value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    /// Output file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Path prefix to strip from displayed file paths.
    #[clap(long, default_value = "")]
    strip_prefix: String,
    /// Report only the in-source markers section. The old
    /// `--only-markers` spelling stays a hidden alias for one cycle.
    #[clap(long = "markers-only", alias = "only-markers", conflicts_with_all = ["excludes_only", "baseline_only"])]
    markers_only: bool,
    /// Report only the `[check.exclude]` globs section. The old
    /// `--only-excludes` spelling stays a hidden alias for one cycle.
    #[clap(long = "excludes-only", alias = "only-excludes", conflicts_with_all = ["markers_only", "baseline_only"])]
    excludes_only: bool,
    /// Report only the `.bca-baseline.toml` entries section. The old
    /// `--only-baseline` spelling stays a hidden alias for one cycle.
    #[clap(long = "baseline-only", alias = "only-baseline", conflicts_with_all = ["markers_only", "excludes_only"])]
    baseline_only: bool,
    /// Baseline file to audit. Defaults to `bca.toml`'s `[check] baseline`
    /// key, then `.bca-baseline.toml` in the working directory when
    /// present. A path given here must exist.
    #[clap(long = "baseline", value_parser)]
    baseline: Option<PathBuf>,
    /// Glob exempting files from the check gate, mirroring
    /// `bca check --check-exclude`. Repeatable. CLI values are
    /// *added to* (merged with), not a replacement for, the `bca.toml`
    /// `[check] exclude` list, so a CLI `--check-exclude` never silently
    /// re-gates a path the project config deliberately exempted.
    #[clap(long = "check-exclude", value_name = "GLOB")]
    check_exclude: Vec<String>,
    /// Read newline-separated `--check-exclude` globs from a file
    /// (`.gitignore`-style), mirroring `bca check --check-exclude-from`.
    /// Use `-` for stdin. Unioned with any `--check-exclude` values.
    #[clap(long = "check-exclude-from", value_parser)]
    check_exclude_from: Option<PathBuf>,
}

impl ExemptionsArgs {
    fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        assemble_globals(
            &self.selection,
            &self.positional,
            &self.tuning,
            &self.preproc,
            &OutputArgs::default(),
            universal,
        )
    }
}

/// Serialization format for `--print-effective-config`. TOML is the
/// default because the same shape is accepted by `--config`, so the
/// output is directly round-trippable; JSON is offered for tooling
/// pipelines that prefer structured data over TOML.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum PrintConfigFormat {
    Toml,
    Json,
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
        /// Single-file output sink. `None` streams to stdout; `Some`
        /// writes the stripped source to the given path. Mutually
        /// exclusive with `in_place` (enforced by clap).
        output: Option<PathBuf>,
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
    /// Walks source and streams in-source suppression markers (with
    /// their enclosing-function context) to a channel for the
    /// `bca exemptions` audit.
    Exemptions,
}

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
    /// Subset of metrics to compute, from `bca metrics --metrics`
    /// (issue #691). `None` (the default) computes every metric;
    /// `Some(set)` restricts via [`MetricsOptions::with_only`], which
    /// auto-resolves each metric's dependencies. Set only by
    /// `run_command_metrics`; every other flow leaves it `None`.
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

fn mk_globset(elems: Vec<String>) -> Result<GlobSet, String> {
    if elems.is_empty() {
        return Ok(GlobSet::empty());
    }

    let mut globset = GlobSetBuilder::new();
    for e in &elems {
        // Normalise the optional leading `./` so `dir/**` and `./dir/**`
        // compile to the same glob; the match-path side is stripped
        // symmetrically in `WalkFilters::passes` / the `[check.exclude]`
        // filter (#726). The emptiness skip runs *after* the strip so a
        // bare `./` (empty once normalised) is skipped like an empty
        // pattern instead of compiling an empty glob.
        let pattern = walk_seed::strip_dot_slash(e);
        if pattern.is_empty() {
            continue;
        }
        globset
            .add(Glob::new(pattern).map_err(|err| format!("invalid glob pattern {e:?}: {err}"))?);
    }
    globset
        .build()
        .map_err(|err| format!("failed to build glob set: {err}"))
}

/// Build an exclude [`GlobSet`] from inline `patterns` unioned with any
/// read from the `from` file (`.gitignore`-style, `-` for stdin). Dies
/// (exit 1) on a file-read or glob-compile error. Shared by the walker's
/// `--exclude` / `--exclude-from` deny-set and `bca check`'s
/// `--check-exclude` / `--check-exclude-from` gate-exemption set (#378)
/// so the two surfaces union and compile globs identically. `flag` is
/// the originating option name (`--exclude-from` or
/// `--check-exclude-from`), used only to attribute a file-read error to
/// the surface the user actually invoked.
fn build_exclude_globset(mut patterns: Vec<String>, from: Option<&Path>, flag: &str) -> GlobSet {
    if let Some(src) = from {
        patterns.extend(read_exclude_patterns_from(src, flag).unwrap_or_else(|e| die(e)));
    }
    mk_globset(patterns).unwrap_or_else(|e| die(e))
}

/// Group a resolved file list by basename into the
/// `HashMap<basename, Vec<PathBuf>>` shape that
/// [`big_code_analysis::fix_includes`] consumes to resolve cross-file
/// `#include` directives. Computed from the same file list the workers
/// analyzed, so the grouping always matches the analyzed set — this is
/// what `bca preproc` lost when #489 made the library's directory-walk
/// callback dead (see #495).
fn group_files_by_basename(paths: Vec<PathBuf>) -> HashMap<String, Vec<PathBuf>> {
    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for path in paths {
        // Skip non-UTF-8 basenames: the preproc include-resolution map
        // (`guess_file`) keys on the UTF-8 file name, so a lossy key
        // could never be matched by an `#include` directive anyway.
        let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let key = fname.to_string();
        all_files.entry(key).or_default().push(path);
    }
    all_files
}

/// Resolve the optional `--language` value to a forced [`LANG`].
///
/// Accepts either a canonical language name (`rust`, `python`, `cpp`,
/// …, via [`LANG`]'s `FromStr`) or a file extension (`rs`, `py`, …, via
/// [`get_from_ext`]). The name spelling is tried first so the obvious
/// `-l rust` works; extensions remain accepted for backward
/// compatibility. An unrecognized value is a hard error (`die`, exit 1)
/// listing the valid language names — previously such a value silently
/// disabled analysis with exit 0 (issue #595).
fn resolve_language(typ: Option<&str>, action: &Action) -> Option<LANG> {
    // Force `Preproc` for the producer so `act_on_file`'s "skip
    // unrecognized" guard never fires — every walked file must reach the
    // dispatch where the producer runs its own Cpp check.
    if matches!(action, Action::PreprocProduce) {
        return Some(LANG::Preproc);
    }
    let value = typ?;
    // Try the canonical name first (`rust`), then fall back to treating
    // the value as a file extension (`rs`). Both are user-reasonable
    // spellings of the same intent.
    value
        .parse::<LANG>()
        .ok()
        .or_else(|| get_from_ext(value))
        .or_else(|| {
            die(format_args!(
                "unknown --language value '{value}'; {}",
                valid_languages()
            ))
        })
}

/// Build the "valid values" suffix for the `--language` error, listing
/// every canonical language name. Mirrors the unknown-metric error
/// style used by `--threshold`.
fn valid_languages() -> String {
    let mut names: Vec<&'static str> = LANG::into_enum_iter().map(|lang| lang.name()).collect();
    names.sort_unstable();
    format!("valid languages are: {}", names.join(", "))
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
pub(crate) fn read_paths_from(src: &Path) -> Result<Vec<PathBuf>, String> {
    // Read raw bytes and split on `\n` rather than going through
    // `BufRead::lines` (which decodes UTF-8 and errors on the first
    // invalid byte). A `--paths-from` list may name files whose paths are
    // not valid UTF-8 — exactly the non-UTF-8 paths the rest of the crate
    // tolerates (the baseline encoder, `handle_path`, the walker all
    // preserve raw bytes). Failing the whole list because one entry has a
    // non-UTF-8 byte is the inconsistency #704 flags. Each line is turned
    // into a `PathBuf` from its raw bytes on Unix (lossless); on other
    // platforms there is no stable byte→OsStr view, so UTF-8 decoding is
    // unavoidable there.
    let label = format!("--paths-from {}", src.display());
    let bytes = read_paths_from_bytes(src).map_err(|e| format!("{label}: {e}"))?;
    Ok(split_path_lines(&bytes))
}

/// Read the entire `--paths-from` source (`-` for stdin, else a file) as
/// raw bytes. Kept separate from the line-splitting so the splitter can
/// be unit-tested on synthetic byte input.
fn read_paths_from_bytes(src: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    if src.as_os_str() == "-" {
        std::io::stdin().lock().read_to_end(&mut buf)?;
    } else {
        std::fs::File::open(src)?.read_to_end(&mut buf)?;
    }
    Ok(buf)
}

/// The three bytes of a UTF-8 byte-order mark (U+FEFF encoded as
/// `EF BB BF`). Stripped from the front of each `--paths-from` line so an
/// editor that saved the list as UTF-8-with-BOM does not turn the first
/// path into a literal `\u{feff}/path` that matches no real file (the
/// per-line BOM strip the previous `collect_lines` reader applied).
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Split a `--paths-from` byte buffer into one `PathBuf` per non-blank
/// line, preserving non-UTF-8 bytes verbatim on Unix. Newline-delimited;
/// a leading UTF-8 BOM, a trailing `\r` (CRLF input), and ASCII
/// surrounding whitespace are trimmed, but interior bytes are kept
/// untouched so a path containing a non-UTF-8 byte survives. `#` is a
/// path character, not a comment (mirrors the prior `path_pattern_filter`
/// policy).
fn split_path_lines(bytes: &[u8]) -> Vec<PathBuf> {
    bytes
        .split(|&b| b == b'\n')
        .filter_map(|line| {
            let trimmed = trim_ws_and_bom(strip_trailing_cr(line));
            (!trimmed.is_empty()).then(|| os_path_from_bytes(trimmed))
        })
        .collect()
}

/// Drop a single trailing `\r` so a CRLF-terminated `--paths-from` line
/// does not carry the carriage return into the path.
fn strip_trailing_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Trim leading and trailing ASCII whitespace bytes and UTF-8 BOMs from
/// both ends, interleaved (so `<BOM>  /path`, `  <BOM>/path`, and
/// `/path<BOM>` all reduce to `/path`). Interior and non-ASCII bytes are
/// preserved, so a non-UTF-8 path survives the trim. Mirrors the
/// whitespace-and-BOM char class the previous `collect_lines` reader
/// trimmed, but at the byte level so it does not require valid UTF-8.
fn trim_ws_and_bom(mut s: &[u8]) -> &[u8] {
    loop {
        let start = s;
        if let Some(rest) = s.strip_prefix(&UTF8_BOM[..]) {
            s = rest;
        }
        s = s.trim_ascii_start();
        if let Some(rest) = s.strip_suffix(&UTF8_BOM[..]) {
            s = rest;
        }
        s = s.trim_ascii_end();
        // Fixed point: another pass removed nothing.
        if s.len() == start.len() {
            return s;
        }
    }
}

/// Build a `PathBuf` from raw path bytes. On Unix this is lossless (paths
/// are arbitrary byte sequences); on other platforms there is no stable
/// byte→`OsStr` view, so the bytes are decoded as UTF-8 lossily — the
/// non-UTF-8 tolerance is a Unix property, matching where the rest of the
/// crate's byte-preserving path handling applies.
#[cfg(unix)]
fn os_path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn os_path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// Read newline-separated `--exclude` glob patterns from `src` (a
/// path on disk or `-` for stdin). Blank lines and lines whose first
/// non-whitespace character is `#` (`.gitignore`-style comments) are
/// skipped; surrounding whitespace and any UTF-8 BOM on retained
/// lines are trimmed. Returns `Err(message)` on I/O failure with
/// the path / failing line; the CLI caller translates this into a
/// `die` exit. `flag` names the originating option (`--exclude-from`
/// vs `--check-exclude-from`) so the error points at the surface the
/// user actually used.
fn read_exclude_patterns_from(src: &Path, flag: &str) -> Result<Vec<String>, String> {
    read_lines_from(src, flag, exclude_pattern_filter)
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

/// Borrowed include/exclude glob pair plus the walk-root-anchored match
/// predicate they drive. Grouping the two globsets keeps
/// `expand_seed_paths`'s signature narrow and co-locates the match
/// convention (empty globset = no-op) with the patterns it applies to.
struct WalkFilters<'a> {
    include: &'a GlobSet,
    exclude: &'a GlobSet,
}

impl WalkFilters<'_> {
    /// Does `match_path` pass the include/exclude filters? An empty
    /// globset is a no-op (no `--include` means "all"; no `--exclude`
    /// means "none"). `match_path` is the form anchored to the file's
    /// walk root (#489), so `./`-anchored patterns match regardless of
    /// how the seed was spelled.
    fn passes(&self, match_path: &Path) -> bool {
        // Strip a leading `./` so bare-relative patterns (`dir/**`) match
        // the `./`-anchored walk-root form just like `./dir/**` does (#726).
        let match_path = walk_seed::strip_cur_dir(match_path);
        (self.include.is_empty() || self.include.is_match(match_path))
            && (self.exclude.is_empty() || !self.exclude.is_match(match_path))
    }

    /// Does `match_path` satisfy only the include allow-list (empty =
    /// "all")? Used for explicitly-named file seeds, which bypass the
    /// exclude deny-set: a project's ignore rules shape *directory-walk*
    /// scope and must not silently drop a file the user named on the
    /// command line (#726). The include allow-list still narrows which
    /// named files are analyzed.
    fn includes(&self, match_path: &Path) -> bool {
        let match_path = walk_seed::strip_cur_dir(match_path);
        self.include.is_empty() || self.include.is_match(match_path)
    }
}

/// Resolved file set plus the subset of seeds that were *explicitly
/// named files* (not products of a directory expansion).
///
/// `explicit_files` backs the #663 rule: an explicitly-named file whose
/// language is unrecognized must warn on stderr and, when it is the sole
/// reason the run produced nothing, exit 1 — mirroring the #596
/// nonexistent-explicit-path error. A file discovered by walking a
/// directory seed is *not* in this set, so a tree full of READMEs and
/// configs stays silently skipped (gated behind `-w`).
struct ResolvedFiles {
    files: Vec<PathBuf>,
    explicit_files: std::collections::HashSet<PathBuf>,
}

/// What kind of on-disk object a `--paths` seed resolves to, used to
/// route a seed into the single-file or directory-walk branch of
/// [`expand_seed_paths`]. A symlink is classified by its *target* (the
/// one deliberate follow — the user named the link as a seed), so the
/// returned kind is always `File` or `Dir`; anything else (a FIFO,
/// socket, …) is treated as a directory walk root, which discovers no
/// regular files and is harmless.
#[derive(Clone, Copy)]
enum SeedKind {
    File,
    Dir,
}

impl SeedKind {
    fn is_file(self) -> bool {
        matches!(self, Self::File)
    }
}

/// Classify `seed` without letting a *missing* path masquerade as a
/// real one. `symlink_metadata` first establishes the seed exists at all
/// (it stats the link itself, so a dangling symlink correctly errors
/// here — symmetric with the walk's `follow_links(false)`, #704); a live
/// symlink is then resolved through `metadata` exactly once to classify
/// its target. Returns `Err` only when the seed (or a symlink's target)
/// does not exist, which the caller turns into the #596 "path does not
/// exist" hard error.
fn seed_kind(seed: &Path) -> std::io::Result<SeedKind> {
    let link_meta = seed.symlink_metadata()?;
    let meta = if link_meta.file_type().is_symlink() {
        // Explicitly-named symlink seed: resolve its target once. A
        // dangling target propagates the `Err` (treated as nonexistent).
        seed.metadata()?
    } else {
        link_meta
    };
    Ok(if meta.is_file() {
        SeedKind::File
    } else {
        SeedKind::Dir
    })
}

fn expand_seed_paths(
    mut paths: Vec<PathBuf>,
    paths_from: Option<PathBuf>,
    no_ignore: bool,
    filters: &WalkFilters<'_>,
) -> ResolvedFiles {
    if let Some(src) = paths_from {
        paths.extend(read_paths_from(&src).unwrap_or_else(|e| die(e)));
    }
    // Default the walk root to the current directory when the user
    // supplied no seeds — neither via `--paths`/`--paths-from` nor via
    // a manifest `paths` key (the manifest merge already populated
    // `paths` before we reach here, so an empty `paths` here means
    // "nothing was configured anywhere"). This mirrors `bca vcs`
    // (`vcs_command::run`) so a bare `bca metrics` ranks the tree the
    // user is standing in instead of analyzing nothing (#596). The
    // injected `.` always exists, so it never trips the
    // nonexistent-seed guard below.
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    let mut out: Vec<PathBuf> = Vec::new();
    // Track which emitted paths we have already pushed so overlapping
    // seeds (`--paths src --paths src/lib.rs`, or two seeds whose trees
    // intersect) contribute each file exactly once. Without this a file
    // reachable from two seeds was analyzed and counted twice (#704).
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut explicit_files: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for seed in paths.into_iter().map(walk_seed::reanchor_seed) {
        // Classify the seed *without* following a final symlink so the
        // seed-level existence/kind check is symmetric with the walk's
        // `follow_links(false)` (#704): the previous `exists()` /
        // `is_file()` / `is_dir()` trio each followed symlinks, so a
        // dangling symlink seed passed the existence guard yet produced
        // no files (TOCTOU / asymmetry). `seed_kind` inspects the link
        // itself via `symlink_metadata`, then resolves a live symlink
        // once — the user *explicitly* named it as a seed and expects it
        // honored, so that single follow is the documented exception.
        let Ok(kind) = seed_kind(&seed) else {
            // An explicitly-supplied seed that does not exist (or whose
            // symlink dangles) is a tool error (exit 1), not a skipped
            // warning (#596): a typo in `--paths` / `--paths-from` /
            // manifest `paths` must fail loudly rather than silently
            // analyze nothing. Only the auto-injected `.` default reaches
            // the walk without being explicitly supplied, and it always
            // exists.
            die(format_args!("path does not exist: {}", seed.display()));
        };
        if kind.is_file() {
            // A single explicit file seed keeps the form the caller
            // spelled (its emitted `name` must match the single-file
            // `bca.analyze()` API). It bypasses the exclude deny-set: a
            // file named directly on the command line is a direct request
            // that a project's directory-walk ignore rules (`.bcaignore`,
            // `--exclude`, manifest `exclude`) must not silently drop
            // (#726), matching the ripgrep/fd convention that an explicit
            // path overrides ignore rules. An `--include` allow-list still
            // narrows which named files are analyzed, matched against the
            // seed's CWD-relative form so `--include 'src/**'` treats
            // `--paths "$PWD/src/f.rs"` and `--paths src/f.rs` alike.
            let include_form = walk_seed::file_seed_match_path(&seed);
            if filters.includes(&include_form) && seen.insert(seed.clone()) {
                // Record the explicit seed (in its emitted form) so the
                // per-file dispatch can distinguish it from a
                // directory-expansion product: an explicitly-named file
                // with an unrecognized language must warn + may exit 1
                // (#663), whereas a walked one stays silently skipped.
                explicit_files.insert(seed.clone());
                out.push(seed);
            }
            continue;
        }
        walk_directory_seed(&seed, no_ignore, filters, &mut out, &mut seen);
    }
    // A walk that resolved zero files is almost always a mistake — an
    // over-narrow `--include`, an `--exclude` that swept everything, or
    // a directory with no supported sources. Surface it on stderr so a
    // bare `bca metrics` in an empty tree is not silently a no-op (#596).
    // Non-gate commands still exit 0; `check` layers its own hard error
    // (`no input files matched`) on top for CI safety.
    if out.is_empty() {
        warn("0 files matched");
    }
    ResolvedFiles {
        files: out,
        explicit_files,
    }
}

/// Walk the directory `seed`, pushing every supported file that passes the
/// include/exclude `filters` into `out` exactly once (deduped through `seen`).
/// Factored out of [`expand_seed_paths`] so its per-seed loop reads as
/// "handle a file seed, else expand a directory seed" rather than inlining the
/// whole `ignore::WalkBuilder` setup and per-entry handling.
///
/// A per-entry walk error (an unreadable subdirectory, a broken symlink, a
/// racing unlink) skips that entry with a warning rather than aborting the
/// run (#704): a single EACCES directory deep in a large tree previously took
/// down every file the walk had yet to reach. This mirrors the per-file
/// tolerance the worker pool already applies to unparseable files.
fn walk_directory_seed(
    seed: &Path,
    no_ignore: bool,
    filters: &WalkFilters<'_>,
    out: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    use ignore::WalkBuilder;
    let mut wb = WalkBuilder::new(seed);
    wb.hidden(true)
        .follow_links(false)
        .require_git(false)
        .git_ignore(!no_ignore)
        .git_exclude(!no_ignore)
        .git_global(!no_ignore)
        .ignore(!no_ignore)
        .parents(!no_ignore);
    for entry in wb.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                eprintln!(
                    "bca: warning: skipping walk entry in {}: {e}",
                    seed.display()
                );
                continue;
            }
        };
        if entry.file_type().is_some_and(|t| t.is_file()) {
            let path = entry.into_path();
            // Anchor the glob match to the walk root rather than the emitted
            // (possibly absolute) path, so `./`-anchored excludes match
            // regardless of how the seed resolved — including a manifest root
            // above the CWD (#489).
            if filters.passes(&walk_seed::match_path_for(seed, &path)) && seen.insert(path.clone())
            {
                out.push(path);
            }
        }
    }
}

/// Resolve the seeds into the terminal, walk-root-anchored file list
/// plus the worker count. The include/exclude filtering is applied here,
/// anchored to each file's walk root (#489), so the resolved list is the
/// final file set: the library runner processes it as-is, with no second
/// walk and no second, emitted-path-form-sensitive glob match (the dead
/// library globsets and re-walk were removed in #495). This anchored
/// walk is the single filtering seam.
fn resolve_walk_files(globals: GlobalOpts) -> (ResolvedFiles, usize) {
    let include = mk_globset(globals.include).unwrap_or_else(|e| die(e));
    let exclude = build_exclude_globset(
        globals.exclude,
        globals.exclude_from.as_deref(),
        "--exclude-from",
    );
    let num_jobs = globals.num_jobs.resolve();
    let filters = WalkFilters {
        include: &include,
        exclude: &exclude,
    };
    let resolved = expand_seed_paths(
        globals.paths,
        globals.paths_from,
        globals.no_ignore,
        &filters,
    );
    (resolved, num_jobs)
}

/// Resolve the seeds and process the file set concurrently. The common
/// walk entry point; callers that also need the resolved file list (only
/// `bca preproc`, for `#include` grouping) use [`run_walk_collecting`].
fn run_walk(globals: GlobalOpts, mut cfg: Config) {
    let (resolved, num_jobs) = resolve_walk_files(globals);
    cfg.explicit_seeds = Arc::new(resolved.explicit_files);
    run_walk_resolved(resolved.files, num_jobs, cfg);
}

/// Process an already-resolved terminal file list concurrently. Lets a
/// caller inspect the resolved set (e.g. `strip-comments --output`, which
/// rejects a multi-file match) before dispatching the workers, without
/// re-running the seed expansion.
fn run_walk_resolved(paths: Vec<PathBuf>, num_jobs: usize, cfg: Config) {
    ConcurrentRunner::new(num_jobs, act_on_file)
        .run(cfg, FilesData { paths })
        .unwrap_or_else(|e| die(format_args!("{e:?}")));
}

/// Like [`run_walk`], but returns the resolved terminal file list.
/// `bca preproc` needs it to group files by basename for cross-file
/// `#include` resolution after the analysis (#495); it is the only
/// caller that consumes the list, so only this variant clones it.
fn run_walk_collecting(globals: GlobalOpts, mut cfg: Config) -> Vec<PathBuf> {
    let (resolved, num_jobs) = resolve_walk_files(globals);
    cfg.explicit_seeds = Arc::new(resolved.explicit_files);
    let paths = resolved.files;
    ConcurrentRunner::new(num_jobs, act_on_file)
        .run(
            cfg,
            FilesData {
                paths: paths.clone(),
            },
        )
        .unwrap_or_else(|e| die(format_args!("{e:?}")));
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
pub(crate) fn walk_metric_set(
    root: &Path,
    globals: GlobalOpts,
    json_out_dir: &Path,
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
    // `bca diff` runs `run_walk` to completion synchronously here — the
    // worker threads spawn and join inside this call — so the scoped
    // CWD swap cannot race another command's walk.
    let restore = with_cwd(root)?;
    run_walk(globals, cfg);
    drop(restore);

    // The JSON writer emitted one document per source file under
    // `json_out_dir`, keyed by the root-relative source path — the same
    // shape `bca diff`'s directory inputs use, so reuse its loader.
    metric_diff::load_dir_set(json_out_dir)
}

/// RAII guard that restores the process working directory to its prior
/// value when dropped. Returned by [`with_cwd`].
struct CwdGuard {
    previous: PathBuf,
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        // Best-effort restore: if the prior directory is gone the
        // process is already in a degraded state, and `bca diff` is
        // about to return anyway. Swallowing the error keeps `Drop`
        // panic-free (a panic here would mask the real error on the
        // unwinding path).
        let _ = std::env::set_current_dir(&self.previous);
    }
}

/// Switch the process working directory to `dir`, returning a guard that
/// restores the previous directory on drop (including every `?`/error
/// path in the caller). Surfaces the current-dir read and the switch as
/// [`metric_diff::DiffError::Read`] so the `bca diff --since` caller can
/// render a single error kind.
fn with_cwd(dir: &Path) -> Result<CwdGuard, metric_diff::DiffError> {
    let previous = std::env::current_dir().map_err(|source| metric_diff::DiffError::Read {
        path: PathBuf::from("."),
        source,
    })?;
    std::env::set_current_dir(dir).map_err(|source| metric_diff::DiffError::Read {
        path: dir.to_path_buf(),
        source,
    })?;
    Ok(CwdGuard { previous })
}

/// Read `path` and decode it as UTF-8, dying (exit 1) on an I/O or
/// non-UTF-8 error. `label` names the file kind in the diagnostic —
/// e.g. `"threshold config"`, `"baseline"`, `"bca.toml"` — producing
/// `failed to read <label> …` / `failed to decode UTF-8 from <label> …`.
/// Centralizes the read+decode half shared by every config/baseline
/// loader; the caller parses the returned text itself, since the parse
/// step differs per format (TOML schema vs. anchored baseline).
fn read_utf8_file(path: &Path, label: &str) -> String {
    let bytes = read_file(path).unwrap_or_else(|e| die_io(&format!("read {label}"), path, e));
    String::from_utf8(bytes)
        .unwrap_or_else(|e| die_io(&format!("decode UTF-8 from {label}"), path, e))
}

/// Load a `[thresholds]` table from `path`, returning its hard scalar
/// limits and optional `[thresholds.soft]` overrides. On any I/O,
/// parse, or schema error the process dies with exit code 1, keeping
/// exit 2 reserved for the "thresholds exceeded" case.
fn load_threshold_config(path: &Path) -> ParsedThresholds {
    let text = read_utf8_file(path, "threshold config");
    let cfg: ThresholdConfig =
        toml::from_str(&text).unwrap_or_else(|e| die_io("parse threshold config", path, e));
    split_thresholds_table(&cfg.thresholds).unwrap_or_else(|e| die(e))
}

/// Load a baseline file. Same error contract as `load_threshold_config`:
/// any I/O, UTF-8, or schema error dies with exit code 1. The anchor
/// is derived from `path` itself, so the baseline keys are interpreted
/// against the file's own directory. `tolerance` and `fuzzy` configure
/// the qualified-symbol matcher (issue #377).
fn load_baseline(path: &Path, tolerance: usize, fuzzy: bool) -> Baseline {
    let text = read_utf8_file(path, "baseline");
    let anchor = baseline::anchor_for(path);
    Baseline::from_str(&text, &anchor, tolerance, fuzzy)
        .unwrap_or_else(|e| die_io("parse baseline", path, e))
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
