//! The complete clap surface for `bca`: the top-level [`Cli`] parser, the
//! [`Command`] subcommand enum, every per-subcommand `*Args` group and the
//! shared flag groups they flatten, the value enums (`ColorWhen`,
//! `OutputFormat`, `RiskFormulaArg`, `PrintConfigFormat`), and the
//! dispatch [`Action`] the command runners build from the parsed args.
//!
//! The per-subcommand `*Args` groups live in area submodules
//! ([`analyze`], [`check`], [`vcs`], [`preproc`]) and are re-exported
//! here so every name stays reachable as before; this module retains the
//! top-level parser, the shared flag groups they flatten, and the
//! cross-cutting value enums.
//!
//! Field visibility is `pub(crate)` so the sibling command-runner modules
//! (`commands`, `dispatch`, `vcs_command`) can read the parsed flags; the
//! published [`Cli`] type itself stays `pub` for the `xtask` man-page
//! renderer.

use super::*;

mod analyze;
mod check;
mod preproc;
mod vcs;

pub(crate) use analyze::*;
pub(crate) use check::*;
pub(crate) use preproc::*;
pub(crate) use vcs::*;

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
    pub(crate) universal: UniversalArgs,
    #[command(subcommand)]
    pub(crate) command: Command,
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
pub(crate) struct UniversalArgs {
    /// Print warnings (skipped files, unrecognized languages). `--warning`
    /// (singular) is kept as a hidden alias for one release cycle and is
    /// slated for removal in the next major.
    // The singular `--warning` alias dates to issue #604.
    #[clap(long = "warnings", short = 'w', global = true, alias = "warning")]
    pub(crate) warning: bool,
    /// Log a "skipped (generated): <path>" line to stderr for each file
    /// auto-skipped by the generated-code detector. Useful for auditing
    /// which files were excluded.
    #[clap(long, global = true)]
    pub(crate) report_skipped: bool,
}

/// Input-selection flags (#597). Flattened into every subcommand that
/// walks a source tree (`metrics`, `ops`, `dump`, `find`, `count`,
/// `functions`, `strip-comments`, `preproc`, `report`, `check`,
/// `exemptions`, `vcs` ranking, `init`, `diff`). Subcommands that walk
/// nothing (`list-metrics`, `diff-baseline`) and the commit-scoring
/// `vcs commit` / `vcs trend` paths omit it, so an inert `--paths` /
/// `--exclude` there is a usage error.
#[derive(Args, Debug, Default, Clone)]
pub(crate) struct WalkSelectionArgs {
    /// Input files or directories to analyze. Unioned with any
    /// positional `[PATHS]`. Defaults to the current directory
    /// (`.`) when omitted and no manifest `paths` is set; an
    /// explicitly-given path that does not exist is an error (exit 1).
    #[clap(long, short, value_parser, help_heading = "Input selection")]
    pub(crate) paths: Vec<PathBuf>,
    /// Glob to include files. Repeat the flag to add multiple globs
    /// (`-I '*.rs' -I '*.toml'`); each occurrence takes exactly one
    /// value, so a positional argument that follows is never swallowed.
    /// A leading `./` is optional: `dir/**` and `./dir/**` are equivalent.
    #[clap(long, short = 'I', num_args(1), action = clap::ArgAction::Append, help_heading = "Input selection")]
    pub(crate) include: Vec<String>,
    /// Glob to exclude files. Repeat the flag to add multiple globs
    /// (`-X '*.tmp' -X '*.bak'`); each occurrence takes exactly one
    /// value, so a positional argument that follows is never swallowed.
    /// A leading `./` is optional: `dir/**` and `./dir/**` are equivalent.
    /// CLI values are *merged with* (unioned, not a replacement for) any
    /// `bca.toml` `exclude` list and any `--exclude-from` patterns, so a
    /// CLI `--exclude` never silently un-excludes a directory the
    /// project config deliberately skipped. Pass `--no-config` to ignore
    /// the manifest entirely.
    ///
    /// Shapes directory-walk scope only: a file named directly on the
    /// command line overrides every exclude glob and is analyzed anyway
    /// (the ripgrep/fd convention), with a warning on stderr naming the
    /// glob it overrode. `-I` / `--include` is not overridden. To exempt
    /// something from `bca check`'s threshold gate whichever way it is
    /// named, use `--check-exclude` / `[check] exclude` instead.
    // The per-file agent hooks in the book's agent-feedback recipe are
    // the caller this bit exists for; see #1146.
    #[clap(long, short = 'X', num_args(1), action = clap::ArgAction::Append, help_heading = "Input selection")]
    pub(crate) exclude: Vec<String>,
    /// Force a language instead of inferring from extension. Accepts a
    /// canonical language name (`rust`, `python`, `cpp`, …) or a file
    /// extension (`rs`, `py`, …). An unrecognized value is a hard error.
    #[clap(
        long,
        short = 'l',
        alias = "language-type",
        help_heading = "Input selection"
    )]
    pub(crate) language: Option<String>,
    /// Disable auto-skip of files marked as generated (e.g. `@generated`,
    /// `DO NOT EDIT`, `GENERATED CODE` near the top). By default the CLI
    /// skips such files so generated bindings do not skew metrics.
    #[clap(long, help_heading = "Input selection")]
    pub(crate) no_skip_generated: bool,
    /// Read newline-separated input paths from a file. Use `-` to read
    /// from stdin. Combined as a union with any `--paths` values.
    /// `--include` globs still apply; `--exclude` globs do not reach an
    /// entry that names a file directly, since an explicitly-named path
    /// overrides the deny-set. Blank lines are skipped; `#` is treated
    /// as a path character (not a comment). To pass a file literally
    /// named `-`, use `./-`.
    #[clap(long = "paths-from", value_parser, help_heading = "Input selection")]
    pub(crate) paths_from: Option<PathBuf>,
    /// Read additional `--exclude` glob patterns from a file (one per
    /// line, `.gitignore`-style). Blank lines and lines whose first
    /// non-whitespace character is `#` are skipped. Use `-` to read
    /// from stdin; to pass a file literally named `-`, use `./-`.
    /// Patterns are unioned with any `--exclude` values into a single
    /// deny-set; order does not matter. Convention is a `.bcaignore`
    /// at the repo root, mirroring `.gitignore` / `.dockerignore`.
    ///
    /// Carries the same scope as `--exclude`: these patterns shape the
    /// directory walk, and a file named directly on the command line
    /// overrides them (with a warning naming the glob). Put gate-only
    /// exemptions in `--check-exclude-from` / `[check] exclude`.
    #[clap(long = "exclude-from", value_parser, help_heading = "Input selection")]
    pub(crate) exclude_from: Option<PathBuf>,
    /// Disable `.gitignore` / `.ignore` / global gitignore awareness
    /// when expanding input directories. Explicit file paths are always
    /// honored regardless of this flag.
    #[clap(long = "no-ignore", help_heading = "Input selection")]
    pub(crate) no_ignore: bool,
    /// Skip auto-discovery of a `bca.toml` manifest. By default `bca`
    /// climbs from the working directory to the repo root looking for
    /// `bca.toml` and merges its keys *under* any explicit CLI flags.
    /// Pass this for raw, fully-explicit invocations that must not pick
    /// up repo-level config (e.g. a reproducible CI one-liner). When no
    /// manifest is discovered this flag is a no-op.
    #[clap(long = "no-config", help_heading = "Input selection")]
    pub(crate) no_config: bool,
}

/// Walker-tuning / analysis-option flags (#597). Flattened alongside
/// [`WalkSelectionArgs`] into the walking subcommands. `--jobs` controls
/// concurrency; `--exclude-tests` / `--no-cyclomatic-try` shape metric
/// computation, so they ride with the commands that compute metrics.
#[derive(Args, Debug, Default, Clone)]
pub(crate) struct WalkTuningArgs {
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
    pub(crate) num_jobs: NumJobs,
    /// Exclude inline test code from metric computation. Currently
    /// applies to Rust only (skips `#[test]`, `#[cfg(test)]`,
    /// `#[tokio::test]`, `#[rstest]`, `#![cfg(test)]` items and
    /// their subtrees). Default is off — every node is counted, so
    /// numbers stay byte-for-byte stable. Languages without a
    /// test-subtree skip rule ignore this flag.
    // The "off by default" guarantee preserves the pre-#182 numbers;
    // the skip hook is `Checker::should_skip_subtree`.
    #[clap(long = "exclude-tests", help_heading = "Walker tuning")]
    pub(crate) exclude_tests: bool,
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
    pub(crate) cyclomatic_count_try: Option<bool>,
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
    pub(crate) no_cyclomatic_try: bool,
}

impl WalkTuningArgs {
    /// Resolve the effective "`?` counts toward cyclomatic" decision,
    /// folding the deprecated `--no-cyclomatic-try` alias into the
    /// positive `--cyclomatic-count-try <bool>` (issue #666). Returns
    /// `None` when neither was set on the CLI, so the manifest
    /// `cyclomatic_count_try` key can fill in; the CLI value otherwise
    /// overrides the manifest in either direction. `clap`'s
    /// `conflicts_with` already rejects passing both.
    pub(crate) fn resolved_count_cyclomatic_try(&self) -> Option<bool> {
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
pub(crate) struct PreprocConsumeArgs {
    /// Existing preprocessor-data JSON to consume during C/C++ analysis.
    /// Use `bca preproc` to produce one.
    #[clap(long, value_parser, help_heading = "Preprocessor")]
    pub(crate) preproc_data: Option<PathBuf>,
}

/// Output flag (#597). `--color` only affects the human-readable `text`
/// dumps, so it is flattened only into the subcommands that render one
/// (`metrics`, `ops`, `dump`, `find`, `functions`).
#[derive(Args, Debug, Default, Clone)]
pub(crate) struct OutputArgs {
    /// When to colorize the human-readable `text` dumps (`metrics` /
    /// `ops` default tree, `dump`, `find`, `functions`): `auto`
    /// (default — color only when stdout is a terminal and `NO_COLOR`
    /// is unset), `always` (force escapes even when piped/redirected),
    /// or `never` (plain text). Structured formats (`json` / `yaml` /
    /// `toml` / `cbor` / `csv`) and file output are never colorized.
    /// Honors the `NO_COLOR` convention (<https://no-color.org>) unless
    /// `--color always` overrides it.
    #[clap(long = "color", value_enum, default_value_t = ColorWhen::Auto, value_name = "WHEN", help_heading = "Output")]
    pub(crate) color: ColorWhen,
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
pub(crate) struct GlobalOpts {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) include: Vec<String>,
    pub(crate) exclude: Vec<String>,
    pub(crate) num_jobs: NumJobs,
    pub(crate) language: Option<String>,
    pub(crate) warning: bool,
    pub(crate) no_skip_generated: bool,
    pub(crate) report_skipped: bool,
    pub(crate) preproc_data: Option<PathBuf>,
    pub(crate) paths_from: Option<PathBuf>,
    pub(crate) exclude_from: Option<PathBuf>,
    pub(crate) no_ignore: bool,
    pub(crate) exclude_tests: bool,
    /// Whether Rust's `?` counts toward cyclomatic complexity, in the
    /// positive sense (issue #666). `None` means the CLI set neither
    /// `--cyclomatic-count-try` nor the deprecated `--no-cyclomatic-try`,
    /// so the manifest `cyclomatic_count_try` key (or the built-in
    /// `true` default) decides.
    pub(crate) count_cyclomatic_try: Option<bool>,
    pub(crate) no_config: bool,
    pub(crate) color: ColorWhen,
}

/// Trailing positional `[PATHS]...` shared by the walking subcommands
/// that can take a bare positional path (`bca metrics src/`) — every
/// walker except `diff` (whose positional slots are already spent on the
/// `<old> <new>` metric-output sets). Unioned with `--paths`/`-p` (#651).
#[derive(Args, Debug, Default, Clone)]
pub(crate) struct PositionalPaths {
    /// Input files or directories to analyze, given positionally
    /// (`bca metrics src/ tests/`). Unioned with any `--paths`/`-p`
    /// values.
    // The clap arg id (`positional_paths`, #651) is distinct from the
    // `--paths` flag's id so both can coexist on one command — clap
    // requires unique arg ids; the two are merged by `assemble_globals`.
    #[clap(value_name = "PATHS", value_parser, help_heading = "Input selection")]
    pub(crate) positional_paths: Vec<PathBuf>,
}

/// `--color` flag values: when to emit ANSI color escapes in the
/// human-readable `text` dumps. Resolved into a library
/// [`big_code_analysis::ColorMode`] by [`ColorWhen::resolve`], which
/// folds in the `NO_COLOR` convention and stdout tty detection.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[clap(rename_all = "lower")]
pub(crate) enum ColorWhen {
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
    pub(crate) fn resolve_with(self, stdout_is_terminal: bool) -> big_code_analysis::ColorMode {
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
    pub(crate) fn resolve_auto(
        stdout_is_terminal: bool,
        no_color: bool,
    ) -> big_code_analysis::ColorMode {
        use big_code_analysis::ColorMode;
        if stdout_is_terminal && !no_color {
            ColorMode::Auto
        } else {
            ColorMode::Never
        }
    }

    /// Resolve against the real process stdout's terminal status.
    pub(crate) fn resolve(self) -> big_code_analysis::ColorMode {
        use std::io::IsTerminal;
        self.resolve_with(std::io::stdout().is_terminal())
    }
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
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
    ///
    /// Streams: the offender rows go to stdout, so `bca check | wc -l`
    /// and `bca check 2>/dev/null` see them. The summary footer,
    /// remediation block, GitHub Actions annotations, and every
    /// `bca:` / `warning:` / `error:` diagnostic go to stderr. The one
    /// exception is `--report-format` without `--output`: the
    /// aggregated document takes stdout there and the human rows fall
    /// back to stderr, so a SARIF payload stays parseable.
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
pub(crate) enum OutputFormat {
    #[default]
    #[value(alias = "tty")]
    Text,
    Markdown,
    Json,
}

/// Serialization format for `--print-effective-config`. TOML is the
/// default because the same shape is accepted by `--config`, so the
/// output is directly round-trippable; JSON is offered for tooling
/// pipelines that prefer structured data over TOML.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrintConfigFormat {
    Toml,
    Json,
}

/// What `act_on_file` should do per file. Drives the inner dispatch and
/// replaces the prior cluster of mutually-exclusive bool flags.
#[derive(Debug)]
pub(crate) enum Action {
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
