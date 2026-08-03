//! Threshold engine for `bca check`.
//!
//! Maps stable metric names (the same set surfaced by `bca list-metrics`,
//! plus dotted names for sub-metrics that don't reduce to a single scalar
//! such as `halstead.volume` or `loc.lloc`) to scalar extractors that read
//! per-function values out of [`big_code_analysis::CodeMetrics`].
//!
//! `ThresholdSet::evaluate_with_policy` walks a [`FuncSpace`] tree and yields one
//! [`Violation`] per `(function, metric)` pair whose value exceeds its
//! configured limit.

// Threshold extractors return `f64`; integral `u64` metric accessors are
// widened with `as f64` (#530), bounded by the count they came from.
#![allow(clippy::doc_markdown, clippy::cast_precision_loss)]

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use big_code_analysis::metric_catalog::MetricScope;
use big_code_analysis::{
    CodeMetrics, FuncSpace, Metric, SpaceKind, SuppressionPolicy, SuppressionScope,
    threshold_metric_for_name,
};
use serde::Deserialize;

use crate::baseline::Coverage;
use crate::format_util::MetricScalar;
use crate::qualified_name::qualified_symbol;
use crate::threshold_soft::{SOFT_SUBTABLE_KEY, SoftLimit, parse_soft_value};

/// The space kind each metric's threshold gates (issue #969) — owned by
/// the library catalog so the CLI gate and the Python `to_sarif` binding
/// cannot drift on which kinds a metric measures, exactly as they share
/// the lower-is-worse direction.
pub(crate) fn metric_scope(name: &str) -> MetricScope {
    // Every name reaching here has resolved to a catalog entry (pinned by
    // `extractor_ids_match_library_catalog`), so the lookup is infallible;
    // an unknown id defaults to the narrowest scope rather than ever
    // gating a file/container aggregate.
    big_code_analysis::metric_catalog::scope(name).unwrap_or(MetricScope::Function)
}

/// Static registry entry: stable threshold name -> scalar extractor.
#[derive(Debug)]
struct MetricExtractor {
    name: &'static str,
    /// Read the scalar value from a function's metrics. `f64` matches the
    /// library's accessor return type; integer-shaped metrics (cyclomatic,
    /// loc.*, nargs, ...) round-trip exactly through `f64` for the ranges
    /// that occur in practice.
    extract: fn(&CodeMetrics) -> f64,
    /// The library metric family `extract` reads from, so a check walk
    /// can compute only the families its thresholds actually gate
    /// (#1113).
    ///
    /// Declared per entry rather than derived from `name`, because the
    /// two vocabularies disagree: [`threshold_metric_for_name`] maps
    /// `tokens` to `None` (a suppression marker may never silence it),
    /// yet `tokens` *is* a configurable threshold. Narrowing the walk by
    /// that mapping would leave `m.tokens` at its zero default and
    /// silently disarm the gate. Naming the family here makes this
    /// registry the single source of truth and forces every future
    /// extractor to declare one.
    metric: Metric,
}

/// Source of truth for accepted threshold names. Order matters only for
/// `--help`-style listings; lookup is by name.
///
/// **Accessor convention (issue #709).** Each metric is thresholded
/// against the accessor that matches how that metric is conventionally
/// reported, and the registry deliberately mixes two shapes:
///
/// - *Per-space own* value — `cognitive()`, `cyclomatic()`,
///   `cyclomatic_modified()`, the `halstead.*`, `mi.*`, `loc.*`, and
///   `abc` accessors, plus `nargs`'s `function_args() + closure_args()`.
///   These read the value for the single function space under test,
///   without rolling up nested children.
/// - *Sum / total over the subtree* — `tokens_sum()`, `nexits_sum()`,
///   `nom.total()`, `wmc.total_wmc()`, `npm.total_npm()`,
///   `npa.total_npa()`. These aggregate the space and its descendants, so
///   a threshold on, e.g., `nom` bounds the whole subtree's method count.
///
/// `nargs` moved from the second group to the first in #1196. It is the
/// one metric whose subtree sum was actively misleading: a closure's
/// parameters are not part of the enclosing function's signature, and
/// every comparable tool counts one callable at a time. Note the
/// serialized `nargs` keys are still subtree sums — only the gate's
/// reading changed — so this is also the one entry where the extractor
/// and the JSON field of the same name disagree.
///
/// The split follows each metric's library accessor and its natural unit
/// of measurement; it is intentional, not an oversight. When adding a new
/// extractor, pick the accessor that matches the metric's reported figure
/// and note which side of this split it falls on.
///
/// Each metric's threshold scope (issue #969) — the space kind it gates —
/// lives in the library [`big_code_analysis::metric_catalog`] (read via
/// [`metric_scope`]), the same single source of truth that owns the
/// lower-is-worse direction, so the CLI gate and the Python `to_sarif`
/// binding cannot disagree on which kinds a metric measures.
const EXTRACTORS: &[MetricExtractor] = &[
    MetricExtractor {
        name: "cognitive",
        extract: |m| m.cognitive.cognitive() as f64,
        metric: Metric::Cognitive,
    },
    MetricExtractor {
        name: "cyclomatic",
        extract: |m| m.cyclomatic.cyclomatic() as f64,
        metric: Metric::Cyclomatic,
    },
    MetricExtractor {
        name: "cyclomatic.modified",
        extract: |m| m.cyclomatic.cyclomatic_modified() as f64,
        metric: Metric::Cyclomatic,
    },
    MetricExtractor {
        name: "halstead.volume",
        extract: |m| m.halstead.volume(),
        metric: Metric::Halstead,
    },
    MetricExtractor {
        name: "halstead.difficulty",
        extract: |m| m.halstead.difficulty(),
        metric: Metric::Halstead,
    },
    MetricExtractor {
        name: "halstead.effort",
        extract: |m| m.halstead.effort(),
        metric: Metric::Halstead,
    },
    MetricExtractor {
        name: "halstead.time",
        extract: |m| m.halstead.time(),
        metric: Metric::Halstead,
    },
    MetricExtractor {
        name: "halstead.bugs",
        extract: |m| m.halstead.bugs(),
        metric: Metric::Halstead,
    },
    MetricExtractor {
        name: "loc.sloc",
        extract: |m| m.loc.sloc() as f64,
        metric: Metric::Loc,
    },
    MetricExtractor {
        name: "loc.ploc",
        extract: |m| m.loc.ploc() as f64,
        metric: Metric::Loc,
    },
    MetricExtractor {
        name: "loc.lloc",
        extract: |m| m.loc.lloc() as f64,
        metric: Metric::Loc,
    },
    MetricExtractor {
        name: "loc.cloc",
        extract: |m| m.loc.cloc() as f64,
        metric: Metric::Loc,
    },
    MetricExtractor {
        name: "loc.blank",
        extract: |m| m.loc.blank() as f64,
        metric: Metric::Loc,
    },
    MetricExtractor {
        name: "nom",
        extract: |m| m.nom.total() as f64,
        metric: Metric::Nom,
    },
    MetricExtractor {
        name: "tokens",
        extract: |m| m.tokens.tokens_sum() as f64,
        metric: Metric::Tokens,
    },
    MetricExtractor {
        name: "nexits",
        extract: |m| m.nexits.nexits_sum() as f64,
        metric: Metric::Nexits,
    },
    MetricExtractor {
        name: "nargs",
        // The space's OWN parameters, not `total()` (#1196). `total()` is
        // `function_args_sum() + closure_args_sum()` — subtree sums — so a
        // function was gated on its own parameters *plus every nested
        // closure's*. `write_top_offenders` has three parameters and was
        // reported at 6 because a sort comparator and a format closure
        // contributed three more, and the remediation the number implied
        // (fewer parameters) was not the one that would clear it.
        //
        // Nothing escapes. A closure that opens its own space — Rust,
        // the JS family, C#, Go, PHP, Perl, Ruby, Lua, Elixir — is gated
        // on its own row, which is also where its fix goes. Where the
        // closure's arguments fold into the enclosing function instead
        // (Python, Java, Kotlin, Groovy, C++/Mozcpp) they land in that
        // space's own `closure_args`, which is the only attribution
        // available and is why the term is added here rather than
        // dropped.
        //
        // This is what every comparable tool measures — RuboCop
        // `Metrics/ParameterLists`, ESLint `max-params`, Clippy
        // `too_many_arguments`, lizard, SonarQube S107, Pylint R0913 all
        // count a callable's own formal parameters. Two of those are the
        // anchors `default_thresholds.rs` derives the shipped limit from,
        // so before this the default was calibrated against a different
        // quantity than the gate enforced.
        extract: |m| (m.nargs.function_args() + m.nargs.closure_args()) as f64,
        metric: Metric::Nargs,
    },
    MetricExtractor {
        name: "mi.original",
        extract: |m| m.mi.original(),
        metric: Metric::Mi,
    },
    MetricExtractor {
        name: "mi.sei",
        extract: |m| m.mi.sei(),
        metric: Metric::Mi,
    },
    MetricExtractor {
        name: "mi.visual_studio",
        extract: |m| m.mi.visual_studio(),
        metric: Metric::Mi,
    },
    MetricExtractor {
        name: "abc",
        extract: |m| m.abc.magnitude(),
        metric: Metric::Abc,
    },
    MetricExtractor {
        name: "wmc",
        extract: |m| m.wmc.total_wmc() as f64,
        metric: Metric::Wmc,
    },
    MetricExtractor {
        name: "npm",
        extract: |m| m.npm.total_npm() as f64,
        metric: Metric::Npm,
    },
    MetricExtractor {
        name: "npa",
        extract: |m| m.npa.total_npa() as f64,
        metric: Metric::Npa,
    },
];

/// Names accepted by `--threshold` and the `[thresholds]` TOML table.
/// Sorted, deduplicated. Used for error messages and tests.
pub(crate) fn known_metric_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = EXTRACTORS.iter().map(|e| e.name).collect();
    names.sort_unstable();
    names
}

fn lookup_extractor(name: &str) -> Option<&'static MetricExtractor> {
    EXTRACTORS.iter().find(|e| e.name == name)
}

/// Reject a metric-gate threshold that is not a finite, non-negative
/// `f64`. NaN and infinities silently disable an `x > threshold` gate
/// (`x > NaN`/`x > inf` is always `false`), and a negative limit trips
/// on every non-negative score; both are user errors, not gates.
pub(crate) fn validate_threshold_value(value: f64, name: &str) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!(
            "limit for {name:?} must be a finite non-negative number; got {value}"
        ));
    }
    Ok(())
}

/// clap `value_parser` for the `vcs commit --fail-above` CI gate. A
/// non-finite or negative threshold would silently disable (or always
/// trip) the gate, so reject it at parse time, mirroring the `check`
/// threshold parser (issue #850).
pub(crate) fn parse_fail_above(s: &str) -> Result<f64, String> {
    let value: f64 = s
        .trim()
        .parse()
        .map_err(|e| format!("invalid fail-above threshold {s:?}: {e}"))?;
    validate_threshold_value(value, "fail-above")?;
    Ok(value)
}

/// Parse a single `--threshold metric=limit` token. Only one `=` is
/// allowed, both sides must be non-empty, and `limit` must parse as a
/// finite, non-negative `f64`.
///
/// Syntax only: the metric name is passed through verbatim, and is
/// resolved against the registry — canonicalised, then checked for
/// existence — one layer down, where the manifest and `--config` names
/// are resolved too. See `canonical_cli_thresholds` for why the two
/// halves are not split across the clap boundary (#1165).
pub(crate) fn parse_cli_threshold(s: &str) -> Result<(String, f64), String> {
    let (name, limit) = s
        .split_once('=')
        .ok_or_else(|| format!("expected `metric=limit`, got {s:?}"))?;
    let name = name.trim();
    let limit = limit.trim();
    if name.is_empty() {
        return Err(format!("empty metric name in {s:?}"));
    }
    let value: f64 = limit
        .parse()
        .map_err(|e| format!("invalid limit {limit:?} for {name:?}: {e}"))?;
    validate_threshold_value(value, name)?;
    Ok((name.to_string(), value))
}

/// TOML config schema:
/// ```toml
/// [thresholds]
/// cyclomatic = 15
/// cognitive = 20
/// "loc.lloc" = 200
///
/// [thresholds.soft]      # optional soft-tier overrides (issue #375)
/// cognitive  = 18        # absolute soft limit
/// cyclomatic = "0.9x"    # or scale-relative-to-hard
/// ```
///
/// The `[thresholds]` table is kept as raw [`toml::Value`]s rather than
/// `f64` so the nested `soft` sub-table coexists with the scalar limits;
/// [`split_thresholds_table`] separates the two layers.
#[derive(Debug, Deserialize)]
pub(crate) struct ThresholdConfig {
    #[serde(default)]
    pub(crate) thresholds: BTreeMap<String, toml::Value>,
}

/// The hard, soft, and per-language layers extracted from one
/// `[thresholds]` table.
#[derive(Debug, Default)]
pub(crate) struct ParsedThresholds {
    /// Scalar `metric = limit` entries (the hard tier).
    pub(crate) hard: BTreeMap<String, f64>,
    /// `[thresholds.soft]` overrides, unresolved (scale factors still
    /// relative to the hard tier).
    pub(crate) soft: BTreeMap<String, SoftLimit>,
    /// `[thresholds.lang.<slug>]` per-metric overrides, keyed by the
    /// canonical language slug
    /// ([`LANG::name`](big_code_analysis::LANG::name)). Each inner map
    /// holds only the metrics that language overrides; the rest are
    /// inherited from [`Self::hard`] at resolution time. See
    /// [`crate::threshold_lang`].
    pub(crate) lang: BTreeMap<&'static str, BTreeMap<String, f64>>,
}

/// Split a raw `[thresholds]` table into its hard scalar limits, the
/// nested `[thresholds.soft]` overrides, and the per-language
/// `[thresholds.lang.<slug>]` tables. Hard values must be numbers; the
/// `soft` key must be a sub-table whose values are numbers or
/// `"<ratio>x"` scale strings; the `lang` key must be a sub-table of
/// per-language sub-tables of numbers. Any other shape is a config
/// error — callers `die` on `Err` so a malformed table never silently
/// degrades into a missing limit.
pub(crate) fn split_thresholds_table(
    raw: &BTreeMap<String, toml::Value>,
) -> Result<ParsedThresholds, String> {
    let mut out = ParsedThresholds::default();
    for (key, value) in raw {
        match key.as_str() {
            SOFT_SUBTABLE_KEY => out.soft = parse_soft_table(value)?,
            crate::threshold_lang::LANG_SUBTABLE_KEY => {
                out.lang = crate::threshold_lang::parse_language_tables(value)?;
            }
            _ => {
                let limit = threshold_scalar("[thresholds]", key, value)?;
                insert_canonical_limit(&mut out.hard, "[thresholds]", key, limit)?;
            }
        }
    }
    Ok(out)
}

/// Parse the `[thresholds.soft]` sub-table into its unresolved
/// [`SoftLimit`] entries, keyed by canonical metric id.
///
/// Mirrors [`crate::threshold_lang::parse_language_tables`], the other
/// reserved sub-table of `[thresholds]`, so both nested layers are read
/// by a named parser rather than one inline loop and one delegation.
fn parse_soft_table(value: &toml::Value) -> Result<BTreeMap<String, SoftLimit>, String> {
    let table = value.as_table().ok_or_else(|| {
        "[thresholds.soft] must be a table of `metric = <number|\"ratiox\">` entries".to_string()
    })?;
    let context = format!("[thresholds.{SOFT_SUBTABLE_KEY}]");
    let mut out = BTreeMap::new();
    for (name, sub) in table {
        let limit = parse_soft_value(name, sub)?;
        insert_canonical_limit(&mut out, &context, name, limit)?;
    }
    Ok(out)
}

/// Insert `value` under `name`'s canonical metric id, so every threshold
/// map is keyed by the dotted registry id from the parse boundary onward
/// (#1165).
///
/// The bare `bca diff --metric` spelling of a `loc` sub-metric is an
/// alias for the dotted threshold id (`sloc` == `loc.sloc`, #514).
/// Canonicalising where the maps are *built* — rather than where they
/// are consumed — is what makes the manifest, `--config`,
/// `[thresholds.lang.<slug>]`, and `--threshold` layers merge by *metric*
/// instead of by spelling. Keyed by the raw spelling, a merge kept both
/// and gated the same extractor twice: two offender lines for one
/// `(function, metric)` pair, and a `--print-effective-config` that
/// printed one of the two limits while the other fired. Every consumer
/// downstream — [`resolve_tier`](crate::commands::check::resolve_tier),
/// [`ThresholdSet::build_tiered`], `--print-effective-config` — may
/// therefore assume canonical keys.
///
/// An ambiguous family head (`halstead`, `mi`) has no single threshold
/// scalar and is rejected here, as is a table naming one metric under two
/// spellings: silently keeping whichever key sorts last is the same
/// surprise this function exists to remove.
pub(crate) fn insert_canonical_limit<V>(
    into: &mut BTreeMap<String, V>,
    table: &str,
    name: &str,
    value: V,
) -> Result<(), String> {
    let canonical = crate::metric_alias::normalize_for_check(name)
        .map_err(|e| format!("{table} {e}"))?
        .into_owned();
    if into.contains_key(&canonical) {
        // Names both spellings rather than quoting the earlier key,
        // which would read as `"loc.ploc": "loc.ploc" is already set`
        // whenever the dotted form is the one written second.
        return Err(format!(
            "{table} {name:?}: {canonical:?} is already set in this table under its other \
             spelling; a bare `loc` sub-metric name and its dotted id (`ploc`, `loc.ploc`) \
             are one metric, so set it once"
        ));
    }
    into.insert(canonical, value);
    Ok(())
}

/// Parse a hard-tier scalar limit. Accepts TOML integers and floats;
/// `i64 -> f64` is exact for the small limits metrics carry in practice.
/// `table` names the enclosing table for the error message, so a
/// per-language override reports `[thresholds.lang.c]` rather than
/// blaming the global table.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn threshold_scalar(
    table: &str,
    name: &str,
    value: &toml::Value,
) -> Result<f64, String> {
    match value {
        toml::Value::Integer(i) => Ok(*i as f64),
        toml::Value::Float(f) => Ok(*f),
        other => Err(format!(
            "{table} {name:?}: expected a number, got {}",
            other.type_str()
        )),
    }
}

/// The `own + lambda` split behind an `nargs` value.
///
/// Carried only where it is not already obvious. In the grammars whose
/// closures open their own space — Rust, JavaScript, TypeScript, TSX,
/// MozJS, C#, Go, PHP, Perl, Ruby, Lua and Elixir — a closure is gated
/// on its own row and the row's number *is* its signature, so there is
/// nothing to split. In Python, Java, Kotlin, Groovy and C++/Mozcpp the
/// closure's arguments fold into the enclosing function instead, which
/// is the only attribution available: `small` there can declare one
/// parameter and be reported at 8.
///
/// A struct rather than a `(u64, u64)`, because the two are same-typed,
/// not interchangeable, and transposing them would print a fluent lie
/// (`AGENTS.md`, "do not pass two same-typed primitives where they could
/// be confused").
#[derive(Debug, Clone, Copy)]
pub(crate) struct NargsSplit {
    /// Parameters the offending function declares itself.
    pub(crate) own: u64,
    /// Parameters contributed by lambdas that open no space of their own.
    pub(crate) lambda: u64,
}

/// One offending `(function, metric)` pair.
#[derive(Debug, Clone)]
pub(crate) struct Violation {
    /// Source file path, as the user supplied it (no canonicalization).
    ///
    /// Held as [`PathBuf`] so non-UTF-8 path components round-trip
    /// through the threshold pipeline byte-for-byte; downstream
    /// consumers (Display, offender records) decide how to surface
    /// non-UTF-8 bytes at their own boundaries.
    pub(crate) path: PathBuf,
    /// 1-based start line of the offending function space.
    pub(crate) start_line: usize,
    /// 1-based end line of the offending function space.
    pub(crate) end_line: usize,
    /// Qualified symbol of the offending space: the `::`-joined chain of
    /// enclosing named container spaces (impl / class / struct / trait /
    /// namespace / interface) and the function's own name — e.g.
    /// `MyStruct::do_thing`. The top-level (`Unit`) space collapses to
    /// `<file>`; anonymous/unnamed spaces (closures, lambdas) collapse to
    /// `<anon@L{start_line}>`. This is the primary baseline-matching key
    /// (issue #377): keying on the symbol rather than the exact
    /// `start_line` lets a function survive line drift from edits above
    /// it. The field name stays `function` for source-compatibility with
    /// the many call sites built before the qualified form existed.
    pub(crate) function: String,
    /// Metric that exceeded its threshold.
    pub(crate) metric: &'static str,
    /// Observed metric value.
    pub(crate) value: f64,
    /// Configured limit for the tier the gate ran at.
    pub(crate) limit: f64,
    /// The hard-tier ceiling for this metric *under the table that
    /// gated this file's language* — equal to [`Self::limit`] at the
    /// hard tier, the un-scaled ceiling at the soft tier, and `None`
    /// for a metric that has a `[thresholds.soft]` limit but no hard
    /// one (there is no ceiling to breach).
    ///
    /// Stamped here rather than looked up afterwards because the
    /// ceiling is per-language once `[thresholds.lang.<slug>]`
    /// overrides exist (#1141), and by classification time the
    /// offender is all that is left of the file that produced it.
    /// Drives the [`CheckOutcome::HardBreach`] escalation in
    /// `classify_check_outcome`.
    ///
    /// [`CheckOutcome::HardBreach`]: crate::CheckOutcome::HardBreach
    pub(crate) hard_limit: Option<f64>,
    /// `true` when this metric is lower-is-worse (the `mi.*`
    /// Maintainability Index family): the value breached by falling
    /// *below* the limit, and [`Violation::ratio`] inverts to
    /// `limit / value` so severity ranking still reads "bigger ratio =
    /// worse" (#698).
    pub(crate) lower_is_worse: bool,
    /// Normalized hash of the function body, populated only when
    /// `--baseline-fuzzy-match` is active (see [`crate::baseline`]).
    /// `None` otherwise. Used as the last-resort baseline matcher when
    /// the qualified symbol changed (a rename that kept the body shape).
    pub(crate) body_hash: Option<u64>,
    /// `true` when an in-source `bca: suppress` / `suppress-file` marker
    /// covers this metric and `--report-suppressed` kept the violation for
    /// the report instead of dropping it. Suppressed violations never count
    /// toward the gate, the exit code, or the human stderr stream — they are
    /// only surfaced in the code-scan document (SARIF `suppressions`). Always
    /// `false` under the default policy (suppressed offenders are dropped)
    /// and under `--no-suppress` (markers ignored, so nothing is suppressed).
    pub(crate) suppressed: bool,
    /// For an `nargs` violation whose value includes spaceless-lambda
    /// arguments, the split to show the reader (#1196). `None` for every
    /// other metric, and for an `nargs` value that is purely the
    /// function's own parameter list — which is the usual case, and the
    /// one where a parenthetical would be noise.
    pub(crate) nargs_split: Option<NargsSplit>,
}

impl Violation {
    /// Render the trailing portion of [`Display`] that *excludes* the
    /// `<path>:<start>-<end>: ` prefix — i.e. `<function>: <metric> =
    /// <value> (limit <limit>)`. The full Display form is built by
    /// concatenating the path/line prefix with this tail; emitters
    /// that already carry the path and line out-of-band (GitHub
    /// Actions annotations, JSON output via `OffenderRecord`) reuse
    /// this method so their message body stays in lockstep with the
    /// human stderr line.
    pub(crate) fn summary_tail(&self) -> String {
        // The split is appended, never substituted for the value: the
        // number stays the thing compared against the limit, and tooling
        // that parses the row keeps finding it in the same position.
        let split = match self.nargs_split {
            Some(NargsSplit { own, lambda }) => {
                format!(" ({own} own + {lambda} lambda)")
            }
            None => String::new(),
        };
        format!(
            "{}: {} = {}{} (limit {})",
            self.function,
            self.metric,
            MetricScalar(self.value),
            split,
            MetricScalar(self.limit),
        )
    }

    /// The breach ratio used to rank violation severity, normalized so a
    /// larger ratio is always a worse violation regardless of metric
    /// direction.
    ///
    /// For higher-is-worse metrics this is `value / limit`. For the
    /// lower-is-worse `mi.*` family it inverts to `limit / value` (#698)
    /// — a Maintainability Index of 5 against a limit of 50 is a 10x
    /// breach, far worse than 45-against-50, and must sort above it.
    ///
    /// Saturates to `f64::INFINITY` for the degenerate denominators
    /// (`limit == 0` in the higher-is-worse case, `value <= 0` in the
    /// lower-is-worse case) so a NaN never escapes into downstream
    /// `total_cmp` sorts — the violation then ranks above all
    /// finite-ratio ones, matching the intuition that the strictest
    /// possible breach is the worst.
    pub(crate) fn ratio(&self) -> f64 {
        if self.lower_is_worse {
            if self.value > 0.0 {
                self.limit / self.value
            } else {
                f64::INFINITY
            }
        } else if self.limit > 0.0 {
            self.value / self.limit
        } else {
            f64::INFINITY
        }
    }

    /// Pick the worst violation in a slice by `value / limit` ratio.
    /// Ties break by larger absolute value, then by metric name
    /// ascending. Returns `None` only if the slice is empty.
    ///
    /// Shared between the `commands::write_summary_footer` rollup
    /// (stderr) and `check_format::write_per_file_rollup`
    /// ($GITHUB_STEP_SUMMARY markdown). Forking the tiebreak across
    /// the two emitters would let the two surfaces disagree about
    /// which violation is "worst" for the same file.
    pub(crate) fn pick_worst<'a>(vs: &[&'a Self]) -> Option<&'a Self> {
        vs.iter().copied().max_by(|a, b| {
            a.ratio()
                .total_cmp(&b.ratio())
                .then_with(|| a.value.total_cmp(&b.value))
                .then_with(|| b.metric.cmp(a.metric))
        })
    }

    pub(crate) fn group_pairs_by_path(
        pairs: &[(Self, Option<crate::baseline::Coverage>)],
    ) -> Vec<(usize, &Self, String, &Path)> {
        let mut by_path: BTreeMap<&Path, Vec<&Self>> = BTreeMap::new();
        for (v, _) in pairs {
            by_path.entry(v.path.as_path()).or_default().push(v);
        }
        let mut rows: Vec<_> = by_path
            .iter()
            .filter_map(|(path, vs)| {
                let worst = Self::pick_worst(vs)?;
                Some((vs.len(), worst, path.display().to_string(), *path))
            })
            .collect();
        rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
        rows
    }
}

impl fmt::Display for Violation {
    /// Stable, parseable single-line format:
    /// `<path>:<start>-<end>: <function>: <metric> = <value> (limit <limit>)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `Path::display` is lossy on non-UTF-8 paths (U+FFFD
        // substitution); acceptable here because Display is the
        // human-facing stderr line, not an identifier. The raw bytes
        // are preserved on `self.path` itself for downstream
        // structured consumers (offender records, GitHub Actions
        // annotations) which call `path.to_str()` with explicit
        // non-UTF-8 handling instead.
        write!(
            f,
            "{}:{}-{}: {}",
            self.path.display(),
            self.start_line,
            self.end_line,
            self.summary_tail(),
        )
    }
}

/// Render a stderr line for one violation, optionally prefixed with a
/// `[new]` / `[regr +N%]` tag derived from baseline classification.
///
/// When `tag` is `None` the output is byte-identical to
/// `format!("{v}")` — this is the load-bearing backward-compat invariant
/// for invocations without `--baseline`. CI tooling that grep-anchors on
/// the start-of-line path keeps working unchanged.
///
/// When `tag` is `Some(coverage)`, the tag and a single space are
/// prepended. Covered violations never reach this function (they are
/// filtered out before emit), so only `Coverage::New` and
/// `Coverage::Regressed` produce output here.
pub(crate) fn render_violation_line(v: &Violation, tag: Option<&Coverage>) -> String {
    match tag {
        // `Covered` is filtered out before reaching the renderer; the
        // arm here is a defensive fallback that emits an unprefixed
        // line rather than panicking or silently dropping a real
        // violation if a future refactor misroutes one.
        None | Some(Coverage::Covered { .. }) => format!("{v}"),
        Some(Coverage::New) => format!("[new] {v}"),
        Some(Coverage::Regressed { recorded }) => {
            format!("{} {v}", format_regressed_tag(*recorded, v.value))
        }
    }
}

/// Format the `[regr ...]` tag for a regression. Cases:
/// - `value.is_nan()` → `[regr NaN]` (degenerate Halstead metrics).
/// - `recorded == 0.0` → `[regr from 0]` (avoid divide-by-zero;
///   percent is undefined).
/// - `pct > 9999` → `[regr +>9999%]` (cap; 100× the baseline is
///   already screaming-loud, exact number adds nothing).
/// - else → `[regr +N%]` with N rounded to nearest integer.
fn format_regressed_tag(recorded: f64, value: f64) -> String {
    if value.is_nan() {
        return "[regr NaN]".to_string();
    }
    if recorded == 0.0 {
        return "[regr from 0]".to_string();
    }
    let pct = ((value - recorded) / recorded * 100.0).round();
    if pct > 9999.0 {
        return "[regr +>9999%]".to_string();
    }
    // `{:.0}` formats the rounded float with zero decimal digits, so
    // we avoid an f64-to-int cast that clippy flags as possibly
    // truncating. `pct` is finite (caller filtered NaN and zero
    // `recorded`), bounded above by 9999 here, and bounded below by 0
    // because the classifier only emits Regressed when
    // `value > recorded`.
    format!("[regr +{pct:.0}%]")
}

/// One resolved threshold: the registry extractor, the configured limit,
/// and whether the metric is lower-is-worse (the `mi.*` family). The
/// direction is resolved once at [`ThresholdSet::build`] time from
/// [`big_code_analysis::metric_catalog`] so the per-function evaluation
/// loop never re-derives it (#698).
#[derive(Debug)]
struct ResolvedThreshold {
    extractor: &'static MetricExtractor,
    limit: f64,
    /// The hard-tier ceiling for this metric, carried alongside the
    /// tier-resolved `limit` so each emitted [`Violation`] can be
    /// stamped with the ceiling that applies to *its* language (#1141).
    /// `None` when the metric has a soft limit but no hard one.
    hard_limit: Option<f64>,
    /// `true` for the lower-is-worse `mi.*` family: a value *below* the
    /// limit is the violation, and the breach ratio inverts to
    /// `limit / value`.
    lower_is_worse: bool,
    /// Which space kind this threshold gates (issue #969), resolved once
    /// from the library catalog at build time so the evaluation loop reads
    /// it without a per-space lookup.
    scope: MetricScope,
}

impl ResolvedThreshold {
    /// The [`Violation`] this threshold produces for `space`, or `None` when
    /// the pair is out of scope, within the limit, or suppressed.
    ///
    /// A method on the threshold rather than inlined in the walk: the walk's
    /// job is to visit every space and stamp qualified symbols, and this is
    /// the orthogonal question of whether one metric breaches on one space.
    fn violation_for(
        &self,
        space: &FuncSpace,
        qualified: &str,
        path: &Path,
        ctx: &SuppressionContext<'_>,
    ) -> Option<Violation> {
        // Per-metric scope gate (#969): a metric's subtree accessor read at
        // the file root or a container is a sum across many functions, not a
        // per-function value, so skip the (space, threshold) pair whose scope
        // excludes this space kind. Runs before the breach/suppression logic,
        // so an out-of-scope pair never produces a `Violation` and composes
        // trivially with suppression and the baseline.
        if !self.scope.admits(space.kind) {
            return None;
        }
        let value = (self.extractor.extract)(&space.metrics);
        // Direction-aware gate (#698): for the lower-is-worse `mi.*` family a
        // value *below* the limit is the violation; every other metric
        // breaches by going *above* it. A NaN value (degenerate
        // Halstead-derived MI on a trivial space) fails both comparisons and
        // is never flagged, matching the prior `value <= limit`
        // higher-is-worse behavior.
        if !breaches_limit(value, self.limit, self.lower_is_worse) {
            return None;
        }
        // A metric is suppressed when policy honors markers and an applicable
        // file- or function-scope marker covers it. Normally such offenders
        // are dropped (never reach the gate). Under `--report-suppressed`
        // they are kept and tagged so the code-scan document can surface them
        // as suppressed alerts — but they still never count toward the gate
        // or exit code (see `Violation::suppressed`).
        //
        // On the root iteration `space` *is* the file root, so the OR below
        // evaluates the same `BTreeSet::contains` twice on the same
        // reference. The second probe is O(log n) on a tiny set and dominated
        // by the walk itself; keeping the OR uniform avoids a special case.
        let suppressed = ctx.honor
            && threshold_metric_for_name(self.extractor.name)
                .is_some_and(|kind| ctx.file_scope.covers(kind) || space.suppressed.covers(kind));
        if suppressed && !ctx.report_suppressed {
            return None;
        }
        // Only `nargs`, and only where the number mixes two sources the
        // reader would otherwise conflate (#1196).
        //
        // The discriminator cannot come from the counts. A closure's own
        // space has `function_args() == 0, closure_args() == N` — and so
        // does a *zero-parameter function* containing spaceless lambdas.
        // The first is not a mix at all: `N` is that closure's own
        // parameter list, and `(0 own + N lambda)` on it would be noise.
        // The second is exactly the misleading row this exists to fix,
        // and gating on `own > 0` hid it precisely there.
        //
        // What separates them is the subject, not the arithmetic: a
        // closure space carries no name of its own. `<anonymous>` is that
        // signal — the synthesised names #1184 added (`<get>`,
        // `<static-init>`, …) are function-like and do want the split.
        let nargs_split = (self.extractor.name == "nargs")
            .then(|| {
                let (own, lambda) = (
                    space.metrics.nargs.function_args(),
                    space.metrics.nargs.closure_args(),
                );
                let is_own_closure_space = space.name.as_deref() == Some("<anonymous>");
                (lambda > 0 && !is_own_closure_space).then_some(NargsSplit { own, lambda })
            })
            .flatten();
        Some(Violation {
            path: path.to_path_buf(),
            start_line: space.start_line,
            end_line: space.end_line,
            function: qualified.to_owned(),
            metric: self.extractor.name,
            value,
            limit: self.limit,
            nargs_split,
            hard_limit: self.hard_limit,
            lower_is_worse: self.lower_is_worse,
            body_hash: None,
            suppressed,
        })
    }
}

/// The suppression state of one [`ThresholdSet::evaluate_with_policy`] walk.
///
/// All three fields are constant for every `(space, threshold)` pair the walk
/// visits, so they travel together rather than as three parallel parameters.
struct SuppressionContext<'a> {
    /// Whether in-source markers are honored at all. `--no-suppress`
    /// (`SuppressionPolicy::Ignore`) clears it, emitting every threshold
    /// violation regardless of source markers.
    honor: bool,
    /// Keep suppressed violations, tagged, instead of dropping them.
    report_suppressed: bool,
    /// The top-level Unit's markers — every `allow-file` marker in the file.
    /// They apply to every nested function as well, so each per-function
    /// check ORs them with the function's own scope.
    file_scope: &'a SuppressionScope,
}

/// Whether the metric named `name` is lower-is-worse (the `mi.*`
/// Maintainability Index family). Thin alias for the library catalog's
/// [`big_code_analysis::metric_catalog::lower_is_worse`] — the single
/// source of truth shared with the offender wording and Code Climate
/// severity inversion, the soft-tier scaling direction (#1166), and the
/// per-function gate.
///
/// `name` must be canonical — every layer that can introduce a threshold
/// key resolves the bare `diff --metric` alias spelling at its own parse
/// boundary (#1165), so an alias never reaches this lookup.
pub(crate) fn metric_is_lower_is_worse(name: &str) -> bool {
    big_code_analysis::metric_catalog::lower_is_worse(name)
}

/// Direction-aware breach test (#698, #837): for the lower-is-worse
/// `mi.*` family a value *below* `limit` is the breach; every other
/// metric breaches by going *above* it. A NaN value fails both
/// comparisons and is never flagged, matching the pre-#698
/// higher-is-worse `value <= limit` non-breach behavior. Shared between
/// the per-function gate and the soft-tier hard-breach escalation in
/// `classify_check_outcome` so the two cannot drift on metric direction.
pub(crate) fn breaches_limit(value: f64, limit: f64, lower_is_worse: bool) -> bool {
    if lower_is_worse {
        value < limit
    } else {
        value > limit
    }
}

/// Pre-resolved set of thresholds: every name has been validated against
/// the registry, so evaluation can skip name lookups.
#[derive(Debug)]
pub(crate) struct ThresholdSet {
    entries: Vec<ResolvedThreshold>,
}

impl ThresholdSet {
    /// Build a hard-tier set, where every limit doubles as its own
    /// ceiling. Test-only shorthand for [`Self::build_tiered`]: the
    /// resolver always knows both layers and passes them separately.
    #[cfg(test)]
    pub(crate) fn build(raw: &BTreeMap<String, f64>) -> Result<Self, String> {
        Self::build_tiered(raw, raw)
    }

    /// Build from the tier-resolved limits plus the un-scaled hard-tier
    /// ceilings, so each emitted [`Violation`] carries both (#385).
    /// Unknown metric names produce an error listing the valid set,
    /// rather than being silently ignored.
    ///
    /// A metric absent from `hard` — a `[thresholds.soft]` absolute
    /// limit with no hard counterpart — gets no ceiling, so breaching it
    /// stays a soft-band encroachment rather than escalating to exit 5.
    pub(crate) fn build_tiered(
        raw: &BTreeMap<String, f64>,
        hard: &BTreeMap<String, f64>,
    ) -> Result<Self, String> {
        let mut entries = Vec::with_capacity(raw.len());
        for (name, limit) in raw {
            // Names arrive canonical: every layer that can introduce one
            // — the manifest and `--config` tables, the per-language
            // tables, the `--threshold` flags — resolves the bare
            // `diff --metric` alias spelling at its own parse boundary
            // (#1165, via `insert_canonical_limit`). Re-normalising here
            // is what let a merge key two spellings of one metric and
            // gate it twice, so this layer now takes canonical keys as an
            // invariant rather than restoring it after the fact.
            let extractor = lookup_extractor(name).ok_or_else(|| {
                let known = known_metric_names();
                format!(
                    "unknown threshold metric {name:?}{}; known metrics: {}",
                    crate::threshold_suggestion::format_suggestion(name, &known),
                    known.join(", ")
                )
            })?;
            validate_threshold_value(*limit, name)?;
            let lower_is_worse = metric_is_lower_is_worse(extractor.name);
            let hard_limit = hard.get(name).copied();
            // A soft limit looser than its own hard ceiling inverts the
            // tier: the early-warning gate stays quiet while the hard
            // gate fires, and any offender that *does* trip the soft band
            // exceeds the ceiling too and escalates straight to exit 5.
            // `parse_scale_str` already rejects the equivalent
            // `"<ratio>x"` form (a factor above 1); this closes the
            // absolute form, which per-language hard overrides make easy
            // to hit by accident (#1141).
            //
            // Both directions, since #1166: a lower-is-worse `mi.*`
            // limit is a floor, so "looser" means *below* the hard floor
            // — which is exactly what `breaches_limit` tests for it.
            if let Some(hard) = hard_limit
                && breaches_limit(*limit, hard, lower_is_worse)
            {
                return Err(format!(
                    "[thresholds.soft] {name:?}: soft limit {} is looser than the hard \
                     limit {}; the soft tier must fire before the hard gate, not after it",
                    MetricScalar(*limit),
                    MetricScalar(hard),
                ));
            }
            entries.push(ResolvedThreshold {
                extractor,
                limit: *limit,
                hard_limit,
                lower_is_worse,
                scope: metric_scope(extractor.name),
            });
        }
        Ok(Self { entries })
    }

    /// True when no thresholds are configured. A check run with no
    /// thresholds is a usage error, not a clean pass.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The metric families this set actually reads, for
    /// [`MetricsOptions::with_only`](big_code_analysis::MetricsOptions::with_only)
    /// (#1113).
    ///
    /// A `bca check` gating one or two metrics previously paid for the
    /// whole suite — Halstead above all — and discarded the rest. Every
    /// consumer downstream of the walk reads [`Violation`] records, never
    /// a [`CodeMetrics`], so narrowing the computation is invisible to
    /// the gate, the baseline, and every report format.
    ///
    /// Deduplicated (five `halstead.*` thresholds name one family) but
    /// otherwise in registry order, so the value is deterministic.
    /// `with_only` resolves each family's dependencies, so an `mi.*`
    /// threshold still pulls in Loc, Cyclomatic, and Halstead.
    pub(crate) fn selected_metrics(&self) -> Vec<Metric> {
        let mut selected: Vec<Metric> = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            if !selected.contains(&entry.extractor.metric) {
                selected.push(entry.extractor.metric);
            }
        }
        selected
    }

    /// Iterate the resolved `(name, limit)` pairs. Used by
    /// `--print-effective-config` to serialize the post-merge view of
    /// the threshold layers (TOML config + `--threshold` CLI overrides)
    /// without re-deriving the order or duplicating the registry's
    /// canonical metric names.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&'static str, f64)> + '_ {
        self.entries
            .iter()
            .map(|entry| (entry.extractor.name, entry.limit))
    }

    /// Walk `space`, comparing each function's metrics against every
    /// configured threshold, and append a [`Violation`] per offending
    /// `(function, metric)` pair to `out`. `policy` decides whether to
    /// honor in-source suppression markers.
    ///
    /// The walk is iterative (not recursive) so an adversarially deeply
    /// nested AST cannot overflow the worker thread's stack — the
    /// thread pool's default 2 MiB stack is small enough that pathological
    /// input matters. See lesson 13 in `docs/development/lessons_learned.md`
    /// for the analogous web-service DoS vector.
    ///
    /// `path` is the source-file path to stamp on each emitted
    /// violation. It is held as [`Path`] (and stored as [`PathBuf`] on
    /// the resulting [`Violation`]) so non-UTF-8 components survive
    /// the pipeline byte-for-byte rather than being collapsed through
    /// `to_str()` / `to_string_lossy()` at this boundary.
    ///
    /// Each violation's function slot carries the *qualified* symbol of
    /// its space (issue #377) — the `::`-joined chain of enclosing named
    /// containers plus the function name, e.g. `MyStruct::do_thing`. The
    /// top-level (`SpaceKind::Unit`) space collapses to the literal
    /// `<file>`: `FuncSpace::name` is the file path there (post #128), so
    /// without the substitution the offender line would read
    /// `path:1-100: path: cyclomatic = ...` — the path doubled. `<file>`
    /// keeps the file-level emission distinguishable and keeps aggregate
    /// metrics like `loc.sloc` usable. See [`qualified_symbol`].
    ///
    /// File-scoped suppressions live on the top-level Unit space; they
    /// apply to every nested function as well. Function-scoped
    /// suppressions live on the function's own space and apply only
    /// there.
    pub(crate) fn evaluate_with_policy(
        &self,
        path: &Path,
        space: &FuncSpace,
        policy: SuppressionPolicy,
        report_suppressed: bool,
        out: &mut Vec<Violation>,
    ) {
        // Resolved once for the whole walk; see [`SuppressionContext`] and
        // [`ResolvedThreshold::violation_for`] for how each field is applied.
        let ctx = SuppressionContext {
            honor: matches!(policy, SuppressionPolicy::Honor),
            report_suppressed,
            file_scope: &space.suppressed,
        };

        // Each stack frame carries the qualified-symbol prefix of the
        // popped space's *parent* chain (issue #377), so a violation can
        // be stamped with the full `Container::method` symbol. The root
        // file space starts with an empty prefix — it contributes no
        // symbol segment of its own (its identity is the path key). The
        // prefix is an `Rc<str>` so descending into a space's children
        // is a refcount bump, not a per-child string copy — the walk
        // visits every space in every file under `bca check`.
        let mut stack: Vec<(&FuncSpace, Rc<str>)> = vec![(space, Rc::from(""))];
        while let Some((current, parent_prefix)) = stack.pop() {
            let qualified = qualified_symbol(current, &parent_prefix);
            out.extend(
                self.entries
                    .iter()
                    .filter_map(|entry| entry.violation_for(current, &qualified, path, &ctx)),
            );
            // Children inherit this space's qualified symbol as their
            // prefix, except the file root, which stays empty so a
            // top-level function is `foo`, not `<file>::foo`. Building
            // the `Rc<str>` consumes `qualified` (no extra copy).
            let child_prefix: Rc<str> = if matches!(current.kind, SpaceKind::Unit) {
                Rc::from("")
            } else {
                Rc::from(qualified)
            };
            // Push children in reverse so `pop()` visits them in source
            // order, matching the recursive form's traversal.
            for child in current.spaces.iter().rev() {
                stack.push((child, Rc::clone(&child_prefix)));
            }
        }
    }
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
#[path = "thresholds_tests.rs"]
mod tests;
