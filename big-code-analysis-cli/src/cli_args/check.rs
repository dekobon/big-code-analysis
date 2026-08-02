//! Clap argument groups for the gating / baseline / diff / exemptions
//! subcommands: `check`, `init`, `diff-baseline`, `diff`, and
//! `exemptions`.

use super::*;

#[derive(Args, Debug)]
pub(crate) struct CheckArgs {
    #[clap(flatten)]
    pub(crate) positional: PositionalPaths,
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    #[clap(flatten)]
    pub(crate) preproc: PreprocConsumeArgs,
    /// Threshold expressed as `<metric>=<limit>`. Repeatable. Metric
    /// names match `bca list-metrics`; sub-metrics use a dotted form
    /// (e.g. `loc.lloc`, `halstead.volume`). The bare `bca diff --metric`
    /// spelling of a `loc` sub-metric is accepted as an alias (`sloc` ==
    /// `loc.sloc`); a bare family head with no single scalar (`halstead`,
    /// `mi`) is rejected with a "did you mean" hint. CLI flags override
    /// values from `--config`. Limits must be finite and non-negative;
    /// `0` is allowed and means "no value permitted".
    #[clap(long = "threshold", value_parser = parse_cli_threshold)]
    pub(crate) thresholds: Vec<(String, f64)>,
    /// Preview what a candidate `<metric>=<limit>` would cost, at both
    /// tiers, instead of gating. Reports the hard-tier offender count,
    /// the resolved soft limit and its offender count, and how many of
    /// each already match a `--baseline` entry — so the decision-grade
    /// figure (new baseline entries the change would add) is on the
    /// screen. Repeatable, one candidate per metric; every other metric
    /// is left out of the walk.
    ///
    /// This exists because `--threshold` cannot answer the question:
    /// its limits are applied last and absolutely, never scaled, so a
    /// candidate trialled that way has no soft tier at all and reads as
    /// free when it is not. The soft band is derived from the candidate
    /// itself, at the `--tier=soft=RATIO` ratio when one is given and
    /// 0.95 otherwise.
    ///
    /// Honours `exclude_tests`, `[check] exclude`, in-source suppression
    /// markers and the baseline exactly as the run it predicts, and
    /// writes nothing: no gate runs, so it always exits 0 on success
    /// (1 on a tool error, such as a candidate naming a metric this
    /// build does not gate). Conflicts with `--write-baseline`,
    /// `--print-effective-config`, `--report-format`, `--output`, and an
    /// explicit `--summary-file <path>`, each of which would produce a
    /// second, different artifact.
    // The candidate-limit preview landed in issue #1169; see the
    // "Choosing thresholds" and "Baselines" recipes in the book.
    //
    // `--summary-file` is rejected by `reject_summary_file_path` rather
    // than by `conflicts_with_all`: clap conflicts on the flag's
    // *presence*, and the keyword forms `auto` / `never` must keep
    // working — `auto` is what a GHA workflow leaves implicit, and the
    // preview simply produces no step summary the way any other
    // non-gating run does.
    #[clap(
        long = "explain-threshold",
        value_name = "METRIC=LIMIT",
        value_parser = parse_cli_threshold,
        conflicts_with_all = ["write_baseline", "print_effective_config", "output_format", "output"],
    )]
    pub(crate) explain_thresholds: Vec<(String, f64)>,
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
    pub(crate) config: Option<PathBuf>,
    /// Report offenders as usual but exit 0 even when thresholds are
    /// exceeded. Useful while adopting baselines without flipping CI red.
    /// Default: exit 2 when any threshold is exceeded.
    #[clap(long = "no-fail")]
    pub(crate) no_fail: bool,
    /// Ignore in-source suppression markers (`bca: suppress`,
    /// `#lizard forgives`, etc.). Every threshold violation is
    /// reported regardless of comment-based silencers. CI auditors
    /// pass this to see the raw, un-silenced offender list.
    #[clap(long = "no-suppress")]
    pub(crate) no_suppress: bool,
    /// Surface suppressed debt in the offender document instead of
    /// dropping it. Offenders silenced by an in-source `bca: suppress`
    /// marker or covered by the baseline are still kept out of the gate
    /// (exit code and offender rows are unaffected), but are emitted into
    /// the `--format sarif` document carrying a SARIF
    /// `suppressions` entry — GitHub Code Scanning renders them as
    /// suppressed (closed) alerts so the debt stays visible. Only the
    /// SARIF format represents suppression; other formats ignore the
    /// flag. Mutually exclusive with `--no-suppress` (which un-silences
    /// markers) and `--write-baseline`.
    #[clap(long = "report-suppressed", conflicts_with_all = ["no_suppress", "write_baseline"])]
    pub(crate) report_suppressed: bool,
    /// CI/IDE *report dialect* for offender records (Checkstyle 4.3 XML,
    /// SARIF 2.1.0 JSON, GitLab Code Climate JSON, clang/GCC warning
    /// lines, MSVC warning lines). Named `--report-format` to separate
    /// "which CI report dialect" from the data-serialization `--format`
    /// the structured subcommands use. When omitted *and*
    /// `--output` is also omitted, only the human-readable offender rows
    /// are emitted; the exit-code contract is unaffected. Note that
    /// passing this flag without `--output` gives the document stdout,
    /// so the human rows fall back to stderr for that combination —
    /// add `--output <file>` to keep both. When omitted but
    /// `--output` is given, the dialect is inferred from the output
    /// extension (`.sarif` → sarif, `.xml` → checkstyle); an extension
    /// with no unique dialect is a usage error. The old `--format` / `-O`
    /// / `--output-format` spellings stay hidden aliases for one release
    /// cycle and are slated for removal in the next major.
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
    pub(crate) output_format: Option<AggregatedFormat>,
    /// File path for the aggregated offender document. Stdout if omitted.
    /// When `--report-format` is also omitted, the dialect is inferred
    /// from this path's extension (`.sarif` → sarif, `.xml` → checkstyle);
    /// a path with no dialect-bearing extension is rejected rather than
    /// silently ignored. Parent directories are created on demand.
    #[clap(long, short, value_parser)]
    pub(crate) output: Option<PathBuf>,
    /// Filter known offenders listed in this TOML baseline. A baselined
    /// function whose metric value has not worsened is suppressed; a
    /// worsened value (or any new offender) still fails. See the
    /// "Baselines" recipe in the book for the full adoption flow.
    #[clap(long = "baseline", value_parser, conflicts_with = "write_baseline")]
    pub(crate) baseline: Option<PathBuf>,
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
    pub(crate) write_baseline: Option<Option<PathBuf>>,
    /// Skip the trailing per-file rollup footer. The footer groups
    /// violations by file and cites the single worst-ratio metric per
    /// file. It is written to stderr, so a plain `bca check | ...`
    /// pipeline never sees it; pass this when a tool reads the *merged*
    /// streams and would be confused by the trailing summary block.
    /// Default: footer enabled.
    #[clap(long = "no-summary")]
    pub(crate) no_summary: bool,
    /// Git ref to diff `HEAD` against. The set of files reported by
    /// `git diff --name-only <ref>...HEAD` is surfaced first in the
    /// summary footer under "Files in this range:", so a reader
    /// scanning a CI log sees their own contributions before the
    /// legacy offender list. Defaults to auto-detection from
    /// `BCA_DIFF_BASE`, `GITHUB_BASE_REF` (PR runs), or
    /// `GITHUB_EVENT_BEFORE` (push runs), in that precedence.
    #[clap(long = "since")]
    pub(crate) since: Option<String>,
    /// Drop violations from files outside the `--since`/auto-detected
    /// touched set entirely (terser CI output for PR gates). Requires
    /// a resolvable diff base, either via `--since` or one of the
    /// auto-detected env vars; failing to resolve is fatal so a
    /// misconfigured CI does not silently turn the gate into a no-op.
    #[clap(long = "changed-only")]
    pub(crate) changed_only: bool,
    /// Emit GitHub Actions `::error file=…,line=…,title=…::msg`
    /// workflow commands per violation so the GHA UI renders them as
    /// inline annotations on the file-diff view. Written to stderr,
    /// additive to the human-readable offender rows — annotations ride
    /// on top, they don't replace them. Tri-state `<auto|always|never>`
    /// mirroring `--color`: `auto` (default) emits annotations when
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
    pub(crate) github_annotations: CiDetect,
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
    pub(crate) summary_file: Option<SummaryFile>,
    /// Suppress the trailing "--- next steps ---" remediation block
    /// that names the artifact, prints a copy-paste-safe
    /// `--write-baseline` refresh invocation, and links to the
    /// Baselines recipe. By default the block is emitted on
    /// failure (and in `$GITHUB_STEP_SUMMARY` when present) so a
    /// first-time reader of a failing CI log can see what to do
    /// next without leaving the page.
    #[clap(long = "no-remediation")]
    pub(crate) no_remediation: bool,
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
    pub(crate) print_effective_config: Option<PrintConfigFormat>,
    /// Which threshold tier to gate against. Accepts
    /// `hard`, `soft`, or `soft=<RATIO>`:
    ///
    /// - `hard` (default) — flag a function only when a metric is at or
    ///   over its `[thresholds]` limit.
    /// - `soft` — early-warning tier: tighten every limit by `RATIO`
    ///   (default 0.95) so a function is flagged before the hard gate
    ///   trips. With a `[thresholds.soft]` table present, the
    ///   per-metric soft limits take precedence over the blanket ratio
    ///   (metrics absent from it inherit their hard limit).
    /// - `soft=0.90` — soft tier tightening every limit by 0.90;
    ///   `soft=1.0` disables the blanket scale (a soft tier driven only
    ///   by an explicit `[thresholds.soft]` table).
    ///
    /// `RATIO` scales the band, not the number: a ceiling comes down
    /// (`cognitive = 15` warns at 13.5), while a lower-is-worse `mi.*`
    /// floor goes up (`mi.original = 20` warns at 22.2223).
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
    pub(crate) tier: TierSpec,
    /// Deprecated alias for `--tier=soft=<RATIO>`. Retained
    /// for one release cycle; pass `--tier=soft=<RATIO>` instead. When
    /// `--tier` is left at its `hard` default, `--headroom <R>` is
    /// promoted to `--tier=soft=<R>` with a deprecation warning; passing
    /// both `--headroom` and an explicit `--tier=soft=<R>` is a conflict.
    // The `--tier` gate and its `--headroom` alias landed in issue #688.
    #[clap(long = "headroom", value_name = "RATIO", hide = true)]
    pub(crate) headroom: Option<f64>,
    /// Exit-code style: `default` keeps the stable
    /// 0/1/2 contract; `tiered` splits exit `2` by severity so CI can
    /// branch without parsing the `[new]` / `[regr +N%]` row tags:
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
    pub(crate) exit_codes: Option<ExitCodes>,
    /// Deprecated alias for `--exit-codes=tiered`. Retained
    /// for one release cycle; pass `--exit-codes=tiered` instead.
    // The `--exit-codes` flag and the tiered split trace to issues #385/#666.
    #[clap(long = "strict-exit-codes", hide = true, conflicts_with = "exit_codes")]
    pub(crate) strict_exit_codes: bool,
    /// Tolerance, in lines, for matching a `--baseline` entry whose
    /// qualified symbol is ambiguous (two methods with the same name on
    /// different `impl` blocks, overloads, collided anonymous spaces).
    /// Unambiguous symbols match regardless of line drift; this only
    /// disambiguates a tie. Defaults to 50. Mirrored by the
    /// `baseline_line_tolerance` key in `bca.toml`.
    #[clap(long = "baseline-line-tolerance", value_name = "LINES")]
    pub(crate) baseline_line_tolerance: Option<usize>,
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
    pub(crate) baseline_fuzzy_match: Option<bool>,
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
    ///
    /// A relative glob given here is resolved against the directory you
    /// ran `bca` from; one written in `bca.toml` is resolved against the
    /// directory holding that `bca.toml`. Put an exemption in the
    /// manifest when it should hold whichever directory the caller
    /// stands in — an editor or hook invoking `bca check <file>` per
    /// edit does not control its own working directory.
    #[clap(long = "check-exclude", value_name = "GLOB")]
    pub(crate) check_exclude: Vec<String>,
    /// Read newline-separated `--check-exclude` globs from a file (one
    /// per line, `.gitignore`-style: blank lines and `#`-comments are
    /// skipped). Use `-` for stdin; to pass a file literally named `-`,
    /// use `./-`. Unioned with any `--check-exclude` values. Convention
    /// is a `.bcacheckignore` at the repo root, mirroring `.bcaignore`
    /// for the walker. Mirrored by the `[check] exclude_from` key in
    /// `bca.toml`.
    #[clap(long = "check-exclude-from", value_parser)]
    pub(crate) check_exclude_from: Option<PathBuf>,
    // Not a flag: filled by the `bca.toml` merge. The manifest's
    // `[check] exclude` / `exclude_from` patterns live here rather than
    // in the two fields above so they keep the manifest directory as
    // their anchor (#1164) — a glob written for the project root must
    // not change meaning with the caller's working directory.
    #[clap(skip)]
    pub(crate) manifest_check_exclude: Option<crate::walk_seed::ManifestExcludes>,
}

impl CheckArgs {
    /// Assemble the runtime [`GlobalOpts`] for the check walk. `check`
    /// emits no colorized text dump, so the output group is defaulted.
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

    /// Resolve the effective [`TierSpec`], folding the deprecated
    /// `--headroom <R>` alias into `--tier=soft=<R>` (issue #688). When
    /// `--tier` is left at its `hard` default and `--headroom` is given,
    /// the headroom value promotes the gate to the soft tier with a
    /// one-cycle deprecation warning. Passing both `--headroom` and an
    /// explicit `--tier=soft=<R>` is a usage error (clap can't express
    /// the conflict because `--tier` always has a default, so it is
    /// rejected here).
    pub(crate) fn resolved_tier(&self) -> TierSpec {
        let Some(ratio) = self.headroom else {
            // No alias: the parsed (or manifest-folded) `--tier` wins.
            return self.tier;
        };
        warn_deprecated_flag("--headroom <R>", "--tier=soft=<R>");
        // Range-validate the alias ratio here (exit 1, tool error) — clap
        // does not parse `--headroom` through `TierSpec`, so the `(0, 1]`
        // check the canonical form gets at parse time must be replicated.
        if !crate::threshold_soft::is_valid_scale_ratio(ratio) {
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
    pub(crate) fn resolved_exit_codes(&self) -> Option<ExitCodes> {
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
pub(crate) struct InitArgs {
    // `init` scaffolds into `--dir` (default `.`) and walks it to
    // generate the baseline, so it consumes the walk / tuning / preproc
    // groups (#597) but names its target via `--dir`, not a positional
    // `[PATHS]`.
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    #[clap(flatten)]
    pub(crate) preproc: PreprocConsumeArgs,
    /// Directory to scaffold into. Defaults to the current working
    /// directory. The directory must already exist; `init` will not
    /// create the project root itself.
    #[clap(long, value_parser)]
    pub(crate) dir: Option<PathBuf>,
    /// Overwrite any of the canonical files that already exist.
    /// Default: refuse to clobber, listing which files block.
    #[clap(long)]
    pub(crate) force: bool,
    /// Skip the baseline-generation pass. The written
    /// `.bca-baseline.toml` is then an empty placeholder; the user
    /// can populate it later with
    /// `bca check --write-baseline .bca-baseline.toml`.
    /// Default: walk the target directory and pin today's offenders.
    #[clap(long = "no-baseline")]
    pub(crate) no_baseline: bool,
}

impl InitArgs {
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
pub(crate) struct DiffBaselineArgs {
    /// Old (base) baseline file — the "before" side of the diff.
    #[clap(value_parser)]
    pub(crate) old: PathBuf,
    /// New (updated) baseline file — the "after" side of the diff.
    #[clap(value_parser)]
    pub(crate) new: PathBuf,
    /// Output style: `text` (default), `markdown`, or `json`.
    #[clap(long, short = 'O', value_enum, default_value_t = OutputFormat::Text)]
    pub(crate) format: OutputFormat,
    /// Render only the "Added" section (combinable with the other
    /// `--*-only` flags). Ignored by `--format json`.
    #[clap(long = "added-only")]
    pub(crate) added_only: bool,
    /// Render only the "Removed" section. Ignored by `--format json`.
    #[clap(long = "removed-only")]
    pub(crate) removed_only: bool,
    /// Render only the "Worsened" section. Ignored by `--format json`.
    #[clap(long = "worsened-only")]
    pub(crate) worsened_only: bool,
    /// Render only the "Improved" section. Ignored by `--format json`.
    #[clap(long = "improved-only")]
    pub(crate) improved_only: bool,
    /// Output file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    pub(crate) output: Option<PathBuf>,
    /// Path prefix to strip from displayed file paths in the TTY and
    /// Markdown per-file tables. No-op for `--format json`, whose paths
    /// are a stable machine identity.
    #[clap(long, default_value = "")]
    pub(crate) strip_prefix: String,
    /// Exit with the metric-gate code (`2`) when the diff — after the
    /// active `--*-only` section filtering — is non-empty; exit `0` when
    /// it is empty. Opt-in (`git diff --exit-code`-style) for grammar-bump
    /// CI that wants a boolean "anything changed" without parsing the
    /// output. Default (flag absent) always exits `0` on success. A tool
    /// error still exits `1` regardless.
    #[clap(long = "exit-code")]
    pub(crate) exit_code: bool,
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
pub(crate) struct DiffArgs {
    // `diff --since` walks both trees, so it consumes the walk-selection
    // and tuning flag groups (#597). Its positional slots are already
    // spent on the `<old> <new>` metric-output sets (the `--since`-mode
    // `<old>` doubles as a relative path scope), so it takes no
    // positional `[PATHS]` (#651) — selection is via `--paths` / globs.
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
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
    pub(crate) old: Option<PathBuf>,
    /// In file/dir mode: the new (after) metric output (a per-file JSON
    /// file or a directory of them). Not used in `--since` mode, where
    /// the after side is always the working tree and the single
    /// positional is a path scope — a second positional with `--since`
    /// is rejected at runtime.
    #[clap(value_parser)]
    pub(crate) new: Option<PathBuf>,
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
    pub(crate) since: Option<String>,
    /// Output style: `text` (default), `markdown`, or `json`.
    #[clap(long, short = 'O', value_enum, default_value_t = OutputFormat::Text)]
    pub(crate) format: OutputFormat,
    /// Minimum absolute change for a per-file metric delta to be
    /// reported. `0` (the default) reports any change of any size;
    /// raise it to suppress small deltas and surface only the larger
    /// metric movements (e.g. on a grammar bump).
    #[clap(long = "min-change", default_value_t = 0.0)]
    pub(crate) min_change: f64,
    /// Restrict the diff to one or more metrics (repeatable). Names are
    /// those printed by `bca list-metrics` (e.g. `cyclomatic`, `sloc`).
    /// The dotted `bca check --threshold` spelling is accepted as an
    /// alias (`loc.sloc` == `sloc`, `halstead.volume` == `halstead`).
    /// When omitted, every metric is reported.
    #[clap(long = "metric")]
    pub(crate) metric: Vec<String>,
    /// Output file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    pub(crate) output: Option<PathBuf>,
    /// Path prefix to strip from displayed file paths in the TTY and
    /// Markdown per-file tables. No-op for `--format json`, whose paths
    /// are a stable machine identity.
    #[clap(long, default_value = "")]
    pub(crate) strip_prefix: String,
    /// Exit with the metric-gate code (`2`) when the diff is non-empty in
    /// any section the active `--metric` / `--min-change` filtering keeps;
    /// exit `0` when it is empty. Opt-in (`git diff --exit-code`-style) for
    /// grammar-bump CI that wants a boolean "anything changed". Default
    /// (flag absent) always exits `0` on success. A tool error still exits
    /// `1` regardless.
    #[clap(long = "exit-code")]
    pub(crate) exit_code: bool,
}

impl DiffArgs {
    /// Assemble the runtime [`GlobalOpts`] for the `--since` walk. `diff`
    /// consumes no preprocessor data and renders no colorized dump, so
    /// those groups default. Its `<old>` positional path scope is folded
    /// in by `side_globals`, not here.
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
pub(crate) struct ExemptionsArgs {
    #[clap(flatten)]
    pub(crate) positional: PositionalPaths,
    #[clap(flatten)]
    pub(crate) selection: WalkSelectionArgs,
    #[clap(flatten)]
    pub(crate) tuning: WalkTuningArgs,
    #[clap(flatten)]
    pub(crate) preproc: PreprocConsumeArgs,
    /// Output style: `text` (default), `markdown`, or `json`. JSON nests
    /// all three sections under a single `suppressions` envelope.
    #[clap(long, short = 'O', value_enum, default_value_t = OutputFormat::Text)]
    pub(crate) format: OutputFormat,
    /// Output file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    pub(crate) output: Option<PathBuf>,
    /// Path prefix to strip from displayed file paths.
    #[clap(long, default_value = "")]
    pub(crate) strip_prefix: String,
    /// Report only the in-source markers section. The old
    /// `--only-markers` spelling stays a hidden alias for one cycle.
    #[clap(long = "markers-only", alias = "only-markers", conflicts_with_all = ["excludes_only", "baseline_only"])]
    pub(crate) markers_only: bool,
    /// Report only the `[check.exclude]` globs section. The old
    /// `--only-excludes` spelling stays a hidden alias for one cycle.
    #[clap(long = "excludes-only", alias = "only-excludes", conflicts_with_all = ["markers_only", "baseline_only"])]
    pub(crate) excludes_only: bool,
    /// Report only the `.bca-baseline.toml` entries section. The old
    /// `--only-baseline` spelling stays a hidden alias for one cycle.
    #[clap(long = "baseline-only", alias = "only-baseline", conflicts_with_all = ["markers_only", "excludes_only"])]
    pub(crate) baseline_only: bool,
    /// Baseline file to audit. Defaults to `bca.toml`'s `[check] baseline`
    /// key, then `.bca-baseline.toml` in the working directory when
    /// present. A path given here must exist.
    #[clap(long = "baseline", value_parser)]
    pub(crate) baseline: Option<PathBuf>,
    /// Glob exempting files from the check gate, mirroring
    /// `bca check --check-exclude`. Repeatable. CLI values are
    /// *added to* (merged with), not a replacement for, the `bca.toml`
    /// `[check] exclude` list, so a CLI `--check-exclude` never silently
    /// re-gates a path the project config deliberately exempted.
    #[clap(long = "check-exclude", value_name = "GLOB")]
    pub(crate) check_exclude: Vec<String>,
    /// Read newline-separated `--check-exclude` globs from a file
    /// (`.gitignore`-style), mirroring `bca check --check-exclude-from`.
    /// Use `-` for stdin. Unioned with any `--check-exclude` values.
    #[clap(long = "check-exclude-from", value_parser)]
    pub(crate) check_exclude_from: Option<PathBuf>,
}

impl ExemptionsArgs {
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
