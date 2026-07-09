//! Clap argument groups for the metrics / inspection subcommands:
//! `metrics`, `ops`, `dump`, `find`, `count`, `functions`, and
//! `list-metrics`.

use super::*;

/// Shared shape for `metrics` and `ops`: same format set, same output
/// semantics (directory of per-file emissions; stdout if omitted).
#[derive(Args, Debug)]
pub(crate) struct StructuredArgs {
    #[clap(flatten)]
    pub(crate) positional: PositionalPaths,
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    #[clap(flatten)]
    pub(crate) preproc: PreprocConsumeArgs,
    #[clap(flatten)]
    pub(crate) out: OutputArgs,
    /// Output format. When omitted, the default `text` format prints a
    /// human-readable colored tree to stdout (`metrics` shows the metric
    /// tree, `ops` the operator/operand tree); pass `--format text`
    /// to request that default explicitly (e.g. to override a `bca.toml`
    /// that set a structured format). `json` / `yaml` / `toml` / `cbor` /
    /// `csv` emit structured per-file data. `--output-format` is accepted
    /// as a deprecated alias; it is hidden from help and slated for
    /// removal in the next major.
    // The `--output-format` alias is the pre-rename spelling from issue #513.
    #[clap(
        long = "format",
        short = 'O',
        alias = "output-format",
        value_name = "FORMAT",
        value_enum
    )]
    pub(crate) output_format: Option<MetricsFormat>,
    /// Output file. Writes one aggregate document (a top-level array of
    /// the per-file results; TOML wraps it under a `files` key) for the
    /// whole run. Stdout if omitted (CBOR requires this flag). Use
    /// `--output-dir` for the per-file directory tree; passing both is an
    /// error.
    #[clap(long, short, value_parser)]
    pub(crate) output: Option<PathBuf>,
    /// Output directory. Writes one document per input file, named by the
    /// input path plus the format extension. Mutually exclusive with
    /// `--output` (which writes a single aggregate file).
    #[clap(long = "output-dir", value_parser)]
    pub(crate) output_dir: Option<PathBuf>,
    /// Pretty-print JSON / TOML output.
    #[clap(long)]
    pub(crate) pretty: bool,
}

impl StructuredArgs {
    /// Assemble the runtime [`GlobalOpts`] from this command's flattened
    /// walk / tuning / preproc / output groups plus the universal flags.
    pub(crate) fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
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
    pub(crate) fn normalize_text_format(&mut self) {
        if matches!(self.output_format, Some(MetricsFormat::Text)) {
            self.output_format = None;
        }
    }
}

/// Flags for `bca metrics`: the shared structured-output set plus an
/// opt-in to attach change-history (VCS) metrics to each file.
#[derive(Args, Debug)]
pub(crate) struct MetricsArgs {
    #[clap(flatten)]
    pub(crate) structured: StructuredArgs,
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
    pub(crate) metrics: Vec<String>,
    /// Also compute change-history (VCS) metrics and attach a `vcs`
    /// block — plus a `hotspot_score` (cyclomatic × recent churn) — to
    /// each file's metrics. Uses default windows (12mo / 90d, weighted
    /// formula); for window / formula tuning use `bca vcs`. Outside a
    /// git working tree this opt-in is skipped with a warning (the AST
    /// metrics still emit, without the `vcs` block).
    #[clap(long)]
    pub(crate) vcs: bool,
    /// Additionally attach a `vcs` block to every nested function /
    /// method / class space, via `git blame` of each file's surviving
    /// lines. Implies `--vcs`. The per-function numbers are
    /// a current-blame snapshot — `churn` is surviving-line count, not
    /// the file-level added+deleted churn — so they rank functions
    /// within a file but are not directly comparable to the file block.
    /// Costs one blame per file; skipped (with a warning) outside a git
    /// working tree.
    #[clap(long = "vcs-per-function")]
    pub(crate) vcs_per_function: bool,
}

impl MetricsArgs {
    pub(crate) fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
        self.structured.to_globals(universal)
    }
}

/// Node-type selection for `find` / `count` (#651). The node kinds moved
/// off a `<NODES>...` positional onto a repeatable `-t`/`--type` flag so
/// the positional slot is free for input `[PATHS]`, matching every other
/// walking subcommand. At least one `-t` is required (`bca find` with no
/// `-t` is a usage error).
#[derive(Args, Debug)]
pub(crate) struct NodeTypesArgs {
    /// Node-type name to match. Repeat the flag to match several
    /// (`-t function_item -t struct_item`). Required: pass at least one.
    #[clap(
        long = "type",
        short = 't',
        required = true,
        action = clap::ArgAction::Append,
        value_name = "NODE_TYPE"
    )]
    pub(crate) types: Vec<String>,
}

/// Line-range bounds for the `dump` and `find` subcommands. Scoped
/// to those two commands rather than `global` (issue #518): every other
/// subcommand silently ignored the range, and the cryptic `--ls`/`--le`
/// names cluttered all of their help. The descriptive `--line-start` /
/// `--line-end` are canonical; `--ls` / `--le` survive as hidden
/// deprecated aliases for one release cycle (mirrors the #513
/// `--output-format` deprecation) and are slated for removal in the
/// next major.
#[derive(Args, Debug, Default)]
pub(crate) struct LineRange {
    /// First line of the range to analyze (1-based, inclusive).
    #[clap(long = "line-start", alias = "ls")]
    pub(crate) line_start: Option<usize>,
    /// Last line of the range to analyze (1-based, inclusive).
    #[clap(long = "line-end", alias = "le")]
    pub(crate) line_end: Option<usize>,
}

/// Arguments for the `dump` subcommand: the [`LineRange`] bounds plus the
/// walk / tuning / preproc / output groups and positional `[PATHS]`.
#[derive(Args, Debug)]
pub(crate) struct DumpArgs {
    #[clap(flatten)]
    pub(crate) line: LineRange,
    #[clap(flatten)]
    pub(crate) positional: PositionalPaths,
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    #[clap(flatten)]
    pub(crate) preproc: PreprocConsumeArgs,
    #[clap(flatten)]
    pub(crate) out: OutputArgs,
}

/// Arguments for the `find` subcommand: the `-t`/`--type` node filters,
/// the [`LineRange`] bounds, and the walk / tuning / preproc / output
/// groups plus positional `[PATHS]`.
#[derive(Args, Debug)]
pub(crate) struct FindArgs {
    #[clap(flatten)]
    pub(crate) nodes: NodeTypesArgs,
    #[clap(flatten)]
    pub(crate) line: LineRange,
    #[clap(flatten)]
    pub(crate) positional: PositionalPaths,
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    #[clap(flatten)]
    pub(crate) preproc: PreprocConsumeArgs,
    #[clap(flatten)]
    pub(crate) out: OutputArgs,
}

/// Arguments for the `count` subcommand: the `-t`/`--type` node filters
/// and the walk / tuning / preproc groups plus positional `[PATHS]`.
/// `count` prints a single tally, so it carries no `--color` output
/// group.
#[derive(Args, Debug)]
pub(crate) struct CountArgs {
    #[clap(flatten)]
    pub(crate) nodes: NodeTypesArgs,
    #[clap(flatten)]
    pub(crate) positional: PositionalPaths,
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    #[clap(flatten)]
    pub(crate) preproc: PreprocConsumeArgs,
}

/// Arguments for the `functions` subcommand (#597): no command-specific
/// flags, just the shared walk / tuning / preproc / output groups and
/// positional `[PATHS]`.
#[derive(Args, Debug)]
pub(crate) struct FunctionsArgs {
    #[clap(flatten)]
    pub(crate) positional: PositionalPaths,
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    #[clap(flatten)]
    pub(crate) preproc: PreprocConsumeArgs,
    #[clap(flatten)]
    pub(crate) out: OutputArgs,
}

impl DumpArgs {
    pub(crate) fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
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
    pub(crate) fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
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
    pub(crate) fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
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
    pub(crate) fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
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
pub(crate) struct ListMetricsArgs {
    /// What to print: `names` (one per line) or `descriptions`
    /// (name + one-line summary).
    #[clap(value_enum, default_value_t = ListMetricsMode::Names)]
    pub(crate) mode: ListMetricsMode,
}
