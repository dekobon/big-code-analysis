//! Clap argument groups for the source-rewriting / reporting
//! subcommands: `preproc` (preprocessor-data production),
//! `strip-comments`, and `report`.

use super::*;

#[derive(Args, Debug)]
pub(crate) struct ReportArgs {
    // `report` keeps its deprecated positional FORMAT (below) working
    // for one more cycle, so it takes `--paths` for input selection
    // rather than a positional `[PATHS]` (#651): a trailing path Vec
    // would be ambiguous against the scalar FORMAT positional.
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    #[clap(flatten)]
    pub(crate) preproc: PreprocConsumeArgs,
    /// Report format (`markdown` or `html`). Defaults to `markdown`
    /// when neither this flag nor the deprecated positional form is
    /// given.
    #[clap(long = "format", short = 'O', value_enum)]
    pub(crate) format: Option<ReportFormat>,
    /// Deprecated positional form of the report format, kept working
    /// for one release cycle. Hidden from help; use `--format`/`-O`
    /// instead. The flag wins when both are given. To be removed in
    /// the next major.
    // The `--format`/`-O` flag superseded the positional form in issue #513.
    #[clap(value_enum, hide = true, value_name = "FORMAT")]
    pub(crate) format_positional: Option<ReportFormat>,
    /// Output file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    pub(crate) output: Option<PathBuf>,
    /// Maximum number of entries per hotspot table (`0` = all).
    #[clap(long, default_value_t = 20)]
    pub(crate) top: usize,
    /// Path prefix to strip from displayed file paths.
    #[clap(long, default_value = "")]
    pub(crate) strip_prefix: String,
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
    pub(crate) no_suppress: Option<bool>,
    /// Append a "Change-history risk" section ranking files by VCS risk
    /// (churn, authorship, fix history) using default windows, mirroring
    /// `bca metrics --vcs`. Ignored (with a warning) outside a git
    /// working tree. See `bca vcs` for the standalone, tunable report.
    #[clap(long)]
    pub(crate) vcs: bool,
}

impl ReportArgs {
    pub(crate) fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
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
    pub(crate) fn resolved_format(&self) -> ReportFormat {
        self.format
            .or(self.format_positional)
            .unwrap_or(ReportFormat::Markdown)
    }

    /// The user's seed paths (the `--paths`/`-p` values, after any manifest
    /// merge), for the provenance footer (issue #680). `report` has no
    /// positional `[PATHS]` group (`to_globals` passes a default), so the
    /// selection's `paths` are the whole seed set; an empty list means the
    /// implicit current-directory default.
    pub(crate) fn seed_paths(&self) -> &[PathBuf] {
        &self.selection.paths
    }
}

#[derive(Args, Debug)]
pub(crate) struct StripCommentsArgs {
    #[clap(flatten)]
    pub(crate) positional: PositionalPaths,
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    #[clap(flatten)]
    pub(crate) preproc: PreprocConsumeArgs,
    /// Rewrite each input file in place instead of writing to stdout.
    /// Use this for multi-file rewrites; it is mutually exclusive with
    /// `--output`.
    #[clap(long)]
    pub(crate) in_place: bool,
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
    pub(crate) output: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub(crate) struct PreprocArgs {
    #[clap(flatten)]
    pub(crate) positional: PositionalPaths,
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    /// Output JSON file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    pub(crate) output: Option<PathBuf>,
}

impl PreprocArgs {
    /// Assemble the runtime [`GlobalOpts`] for the producing walk.
    /// `preproc` produces preprocessor data rather than consuming it, so
    /// the preproc-consume group is omitted (defaulted).
    pub(crate) fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
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
