//! Clap argument groups for the `vcs` subcommand family: change-history
//! file ranking (`bca vcs`), single-commit scoring (`vcs commit`), and
//! historical trend sampling (`vcs trend`).

use super::*;

/// Risk-score formula selection for `bca vcs` (issue #328).
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub(crate) enum RiskFormulaArg {
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
    pub(crate) fn to_globals(&self, universal: &UniversalArgs) -> GlobalOpts {
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
pub(crate) struct VcsArgs {
    // Walk-selection / tuning groups (#597) are flattened non-`global`,
    // so `bca vcs --paths src` ranks that subtree but `bca vcs commit
    // --paths …` / `bca vcs trend --exclude …` are hard usage errors:
    // those subcommands score a commit / sample a time series, not a
    // walked tree. `vcs` takes no positional `[PATHS]` (#651) because its
    // positional slot is reserved for the subcommand token.
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
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
    pub(crate) command: Option<VcsSubcommand>,
    /// Output format. When omitted, the human-readable ranked `text`
    /// table is printed; pass `--format text` to request it explicitly.
    /// `markdown` / `html` render a sortable report page like
    /// `bca report`; `json` / `yaml` / `toml` / `cbor` / `csv` emit
    /// structured data. `--output-format` is accepted as a deprecated
    /// alias.
    // `text` unification: #659; `--output-format` alias: #513.
    #[clap(long = "format", short = 'O', alias = "output-format", value_enum)]
    pub(crate) format: Option<VcsFormat>,
    /// Output file. A change-history report is a single whole-repo
    /// document, so this names one file. Stdout if omitted (CBOR requires
    /// this flag — it is binary).
    #[clap(long, short, value_parser)]
    pub(crate) output: Option<PathBuf>,
    /// Pretty-print JSON / TOML output.
    #[clap(long)]
    pub(crate) pretty: bool,
    /// Long observation window (`12mo`, `2y`, `52w`, `365d`, or ISO 8601
    /// `P1Y`). Accepted in the parent or subcommand position.
    #[clap(long, default_value = "12mo", global = true)]
    pub(crate) long_window: String,
    /// Recent observation window. Accepted in the parent or subcommand
    /// position.
    #[clap(long, default_value = "90d", global = true)]
    pub(crate) recent_window: String,
    /// Show only the top N files by risk score (`0` = all).
    #[clap(long, default_value_t = 50)]
    pub(crate) top: usize,
    /// Which tracked files to rank: `metrics` (only files
    /// bca has metrics for — the default), `all` (every tracked text
    /// file), or a comma-separated extension allow-list (`rs,py,toml`).
    /// Applied on top of `--paths`/`--include`/`--exclude` (AND
    /// semantics). When omitted, the `bca.toml` `[vcs] file_types` key is
    /// used if present, else `metrics`. Setting it here replaces the
    /// manifest value.
    #[clap(long, value_name = "SCOPE")]
    pub(crate) file_types: Option<String>,
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
    pub(crate) reference: Option<String>,
    /// Walk the full commit DAG rather than first-parent only.
    #[clap(long, global = true)]
    pub(crate) full_history: bool,
    /// Include merge commits (skipped by default).
    #[clap(long, global = true)]
    pub(crate) include_merges: bool,
    /// Do not follow file renames across history.
    #[clap(long, global = true)]
    pub(crate) no_follow_renames: bool,
    /// Do not exclude bot author identities.
    #[clap(long, global = true)]
    pub(crate) no_exclude_bots: bool,
    /// Override the bot-author exclusion regex.
    #[clap(long, global = true)]
    pub(crate) bot_pattern: Option<String>,
    /// Reference "now" for reproducible runs (RFC 3339, `@unix`, or any
    /// git date spelling). Defaults to wall-clock time.
    #[clap(long, global = true)]
    pub(crate) as_of: Option<String>,
    /// Composite risk-score formula.
    #[clap(long, value_enum, default_value_t = RiskFormulaArg::Weighted, global = true)]
    pub(crate) risk_formula: RiskFormulaArg,
    /// Emit SHA-256-hashed canonical author identities.
    #[clap(long)]
    pub(crate) emit_author_details: bool,
    /// Secret key that hardens `--emit-author-details` into a keyed
    /// HMAC-SHA256: an attacker can no longer recover the emitted digests
    /// by hashing a candidate set of emails or with a precomputed
    /// email→hash table. Without it the digests are a bare SHA-256
    /// pseudonym. Requires `--emit-author-details`; the same key yields the
    /// same digests across runs and a cache replay.
    ///
    /// Prefer the `BCA_AUTHOR_HASH_KEY` environment variable: a key on the
    /// command line is visible to other users via the process list (`ps`)
    /// and lands in shell history. The flag takes precedence when both are
    /// set.
    //
    // Hardening tracked in issue #956 (follow-up to #811); the rationale
    // lives here in a `//` maintainer comment so clap never renders the
    // issue number into `--help` (the help-text issue-reference gate).
    //
    // SECURITY: this holds the raw secret. `VcsArgs` derives `Debug`, so
    // never whole-struct debug-log it (`{args:?}`) — that would leak the
    // key. It is moved into the redacting `AuthorHashKey` newtype as early
    // as `vcs_command::resolve_author_hash_key`. (On the CLI the key is also
    // argv-visible, which is why the env-var form is recommended.)
    #[clap(long, value_name = "KEY")]
    pub(crate) author_hash_key: Option<String>,
    /// Emit stats for files deleted at the target ref.
    #[clap(long)]
    pub(crate) include_deleted: bool,
    /// Bus-factor coverage (abandonment) threshold — the fraction of a
    /// directory's files that must be orphaned for the truck-factor
    /// greedy removal to stop. Must be in `(0, 1)`; default
    /// `0.5` per Avelino. Ignored by `bca vcs commit`.
    #[clap(long, default_value_t = big_code_analysis::vcs::options::DEFAULT_BUS_FACTOR_THRESHOLD)]
    pub(crate) bus_factor_threshold: f64,
    /// Disable the persistent change-history cache for the file ranking:
    /// always walk fresh, and neither read nor write the cache. The cache
    /// otherwise reuses prior work on an unchanged tree and walks only new
    /// commits when `HEAD` has advanced.
    #[clap(long)]
    pub(crate) no_cache: bool,
    /// Remove this repository's cached history before ranking, forcing a
    /// full rebuild. Combine with `--no-cache` to wipe without re-priming.
    #[clap(long)]
    pub(crate) clear_cache: bool,
    /// Directory for the persistent history cache. Defaults to
    /// `$XDG_CACHE_HOME/big-code-analysis/vcs` (or the platform
    /// equivalent: `%LOCALAPPDATA%` on Windows, `~/.cache` otherwise).
    #[clap(long, value_parser)]
    pub(crate) cache_dir: Option<PathBuf>,
}

/// Subcommands of `bca vcs`: `commit` (issue #331) and `trend` (issue
/// #333); the bare `bca vcs` ranking path is the `None` case.
#[derive(Subcommand, Debug)]
pub(crate) enum VcsSubcommand {
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
pub(crate) struct JitArgs {
    /// Commit / revision to score (any git revision spelling: a SHA, a
    /// tag, `HEAD`, `main~3`, …). Scored against its first parent. Mutually
    /// exclusive with `--diff`.
    #[clap(value_name = "COMMIT", default_value = "HEAD", conflicts_with = "diff")]
    pub(crate) commit: String,
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
    pub(crate) diff: Option<PathBuf>,
    /// Output format (`json` default, plus `yaml` / `toml` / `cbor`).
    #[clap(long = "format", short = 'O', value_enum, default_value_t = JitFormat::default())]
    pub(crate) format: JitFormat,
    /// Output file. Stdout if omitted (CBOR requires this flag — it is
    /// binary).
    #[clap(long, short, value_parser)]
    pub(crate) output: Option<PathBuf>,
    /// Pretty-print JSON / TOML output.
    #[clap(long)]
    pub(crate) pretty: bool,
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
    pub(crate) fail_above: Option<f64>,
}

/// Flags for `bca vcs trend` (issue #333). The history-window, bot, merge,
/// rename, `--ref`, and `--as-of` (the most-recent point's anchor) options
/// come from the parent [`VcsArgs`], as does `--top` (how many files to
/// keep, ranked by most-recent risk); these are the trend-only additions.
#[derive(Args, Debug)]
pub(crate) struct TrendArgs {
    /// Number of evenly-spaced sample points across `--span`, inclusive of
    /// both endpoints (the oldest is `as-of − span`, the newest is
    /// `as-of`). Minimum 2, maximum 120 — the cap bounds the per-point
    /// history walks on deep histories.
    // The 120 cap is `big_code_analysis::vcs::trend::MAX_TREND_POINTS`;
    // keep this prose in sync if that constant changes (validated in
    // `validate_points`).
    #[clap(long, default_value_t = 12)]
    pub(crate) points: usize,
    /// Total look-back window the points span (`12mo`, `2y`, `52w`, `365d`,
    /// or ISO 8601 `P1Y`). With the default 12 points this yields roughly
    /// monthly snapshots over the past year.
    #[clap(long, default_value = "12mo")]
    pub(crate) span: String,
    /// Show only the top N files in each improving / regressing delta
    /// summary (`0` = all).
    #[clap(long, default_value_t = 10)]
    pub(crate) top_deltas: usize,
    /// Output format (`json` default, plus `yaml` / `cbor`). TOML is
    /// excluded — absent points serialize as `null`, which TOML cannot
    /// represent.
    #[clap(long = "format", short = 'O', value_enum, default_value_t = TrendFormat::default())]
    pub(crate) format: TrendFormat,
    /// Output file. Stdout if omitted (CBOR requires this flag — it is
    /// binary).
    #[clap(long, short, value_parser)]
    pub(crate) output: Option<PathBuf>,
    /// Pretty-print JSON output.
    #[clap(long)]
    pub(crate) pretty: bool,
}
