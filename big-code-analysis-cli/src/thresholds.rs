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

#![allow(clippy::doc_markdown)]

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use big_code_analysis::{CodeMetrics, FuncSpace, MetricKind, SpaceKind, SuppressionPolicy};
use serde::Deserialize;

use crate::baseline::Coverage;
use crate::format_util::MetricScalar;

/// Static registry entry: stable threshold name -> scalar extractor.
#[derive(Debug)]
struct MetricExtractor {
    name: &'static str,
    /// Read the scalar value from a function's metrics. `f64` matches the
    /// library's accessor return type; integer-shaped metrics (cyclomatic,
    /// loc.*, nargs, ...) round-trip exactly through `f64` for the ranges
    /// that occur in practice.
    extract: fn(&CodeMetrics) -> f64,
}

/// Source of truth for accepted threshold names. Order matters only for
/// `--help`-style listings; lookup is by name.
const EXTRACTORS: &[MetricExtractor] = &[
    MetricExtractor {
        name: "cognitive",
        extract: |m| m.cognitive.cognitive(),
    },
    MetricExtractor {
        name: "cyclomatic",
        extract: |m| m.cyclomatic.cyclomatic(),
    },
    MetricExtractor {
        name: "cyclomatic.modified",
        extract: |m| m.cyclomatic.cyclomatic_modified(),
    },
    MetricExtractor {
        name: "halstead.volume",
        extract: |m| m.halstead.volume(),
    },
    MetricExtractor {
        name: "halstead.difficulty",
        extract: |m| m.halstead.difficulty(),
    },
    MetricExtractor {
        name: "halstead.effort",
        extract: |m| m.halstead.effort(),
    },
    MetricExtractor {
        name: "halstead.time",
        extract: |m| m.halstead.time(),
    },
    MetricExtractor {
        name: "halstead.bugs",
        extract: |m| m.halstead.bugs(),
    },
    MetricExtractor {
        name: "loc.sloc",
        extract: |m| m.loc.sloc(),
    },
    MetricExtractor {
        name: "loc.ploc",
        extract: |m| m.loc.ploc(),
    },
    MetricExtractor {
        name: "loc.lloc",
        extract: |m| m.loc.lloc(),
    },
    MetricExtractor {
        name: "loc.cloc",
        extract: |m| m.loc.cloc(),
    },
    MetricExtractor {
        name: "loc.blank",
        extract: |m| m.loc.blank(),
    },
    MetricExtractor {
        name: "nom",
        extract: |m| m.nom.total(),
    },
    MetricExtractor {
        name: "tokens",
        extract: |m| m.tokens.tokens_sum(),
    },
    MetricExtractor {
        name: "nexits",
        extract: |m| m.nexits.exit_sum(),
    },
    MetricExtractor {
        name: "nargs",
        extract: |m| m.nargs.nargs_total(),
    },
    MetricExtractor {
        name: "mi.original",
        extract: |m| m.mi.mi_original(),
    },
    MetricExtractor {
        name: "mi.sei",
        extract: |m| m.mi.mi_sei(),
    },
    MetricExtractor {
        name: "mi.visual_studio",
        extract: |m| m.mi.mi_visual_studio(),
    },
    MetricExtractor {
        name: "abc",
        extract: |m| m.abc.magnitude(),
    },
    MetricExtractor {
        name: "wmc",
        extract: |m| m.wmc.total_wmc(),
    },
    MetricExtractor {
        name: "npm",
        extract: |m| m.npm.total_npm(),
    },
    MetricExtractor {
        name: "npa",
        extract: |m| m.npa.total_npa(),
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

/// Parse a single `--threshold metric=limit` token. Only one `=` is
/// allowed, both sides must be non-empty, and `limit` must parse as a
/// finite, non-negative `f64`.
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
    if !value.is_finite() || value < 0.0 {
        return Err(format!(
            "limit for {name:?} must be a finite non-negative number; got {value}"
        ));
    }
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

/// Reserved key inside `[thresholds]` that introduces the soft-tier
/// sub-table (`[thresholds.soft]`). Every other key in the table is a
/// hard-limit metric name. No metric is named `soft`, so the reservation
/// never collides with a real threshold.
pub(crate) const SOFT_SUBTABLE_KEY: &str = "soft";

/// One soft-tier limit, before resolution against the hard tier.
///
/// `[thresholds.soft]` values are either a plain number (an absolute soft
/// limit) or a `"<ratio>x"` string (scale the metric's hard limit by
/// `ratio`). The scale form is resolved lazily because it needs the
/// merged hard limit, which is only known after the manifest and
/// `--config` layers combine — see [`SoftLimit::resolve`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SoftLimit {
    /// An explicit soft limit, used as-is.
    Absolute(f64),
    /// A factor in `(0, 1]` applied to the metric's hard limit.
    Scale(f64),
}

impl SoftLimit {
    /// Resolve to a concrete limit. `Absolute` ignores `hard`; `Scale`
    /// multiplies the metric's hard limit, erroring when no hard limit
    /// exists for the metric to scale (a scale factor relative to
    /// nothing is meaningless).
    pub(crate) fn resolve(self, name: &str, hard: Option<f64>) -> Result<f64, String> {
        match self {
            Self::Absolute(value) => Ok(value),
            Self::Scale(factor) => {
                let base = hard.ok_or_else(|| {
                    format!(
                        "[thresholds.soft] {name:?} uses scale-relative syntax but no \
                         hard [thresholds] limit exists for {name:?} to scale; give it an \
                         absolute soft limit or add a hard limit first"
                    )
                })?;
                Ok(scale_threshold(base, factor))
            }
        }
    }
}

/// Significant figures retained when scaling a threshold by a ratio
/// (`--headroom` or a `[thresholds.soft]` `"<ratio>x"` factor). Trims
/// float-multiplication artifacts (e.g. `7 * 0.95 == 6.6499999999999995`)
/// to a readable `6.65` while preserving full precision for the largest
/// thresholds seen in practice (`halstead.effort`, on the order of
/// `50000`). At 6 figures the rounding error is far below any metric's
/// granularity, so the offender set is identical to the un-rounded
/// product. This matches the `{:.6g}` rounding the now-removed
/// `bca-self-scan-headroom.py` helper used (#373), so soft-gate offender
/// lines render byte-for-byte the same whether the band came from
/// `--headroom` or a per-metric scale factor.
const HEADROOM_SIG_FIGS: i32 = 6;

/// Whether `ratio` is a valid soft-tier scaling factor: the half-open
/// interval `(0, 1]`. `1.0` is the no-op identity (parity with the hard
/// gate); a factor `> 1` would make the soft tier *looser* than the hard
/// gate, which is never the early-warning intent; `0`, negatives, and
/// `NaN` (which fails both comparisons) are usage errors. Shared by the
/// `--headroom` scalar (CLI and `bca.toml`) and the `[thresholds.soft]`
/// `"<ratio>x"` form so the accepted range is defined in exactly one
/// place; callers compose their own context-specific error message.
pub(crate) fn is_valid_scale_ratio(ratio: f64) -> bool {
    0.0 < ratio && ratio <= 1.0
}

/// Scale a threshold `limit` by `ratio`, rounding to
/// [`HEADROOM_SIG_FIGS`] significant figures. `ratio` is assumed already
/// validated (see [`is_valid_scale_ratio`]) to lie in `(0, 1]`. Shared
/// by the `--headroom` scalar path and the `[thresholds.soft]`
/// scale-relative form so both round identically.
pub(crate) fn scale_threshold(limit: f64, ratio: f64) -> f64 {
    let scaled = limit * ratio;
    // `log10(0)` is `-inf`; short-circuit the degenerate inputs so the
    // magnitude maths below only sees finite, non-zero values.
    if scaled == 0.0 || !scaled.is_finite() {
        return scaled;
    }
    // `log10` of a finite, non-zero f64 lies in roughly [-323, 308], so
    // its floor always fits an i32 — the truncating cast cannot lose
    // information here.
    #[allow(clippy::cast_possible_truncation)]
    let magnitude = scaled.abs().log10().floor() as i32;
    let decimals = (HEADROOM_SIG_FIGS - 1) - magnitude;
    let factor = 10f64.powi(decimals);
    // For an absurdly tiny limit the sig-fig `factor` overflows to
    // infinity, and `scaled * factor / factor` would be NaN. No real
    // metric threshold is subnormal, but guard it so the function is
    // total: such a value is already far below any rounding granularity,
    // so return it unrounded rather than poisoning the threshold set
    // with NaN.
    if !factor.is_finite() {
        return scaled;
    }
    (scaled * factor).round() / factor
}

/// The hard and soft layers extracted from one `[thresholds]` table.
#[derive(Debug, Default)]
pub(crate) struct ParsedThresholds {
    /// Scalar `metric = limit` entries (the hard tier).
    pub(crate) hard: BTreeMap<String, f64>,
    /// `[thresholds.soft]` overrides, unresolved (scale factors still
    /// relative to the hard tier).
    pub(crate) soft: BTreeMap<String, SoftLimit>,
}

/// Split a raw `[thresholds]` table into its hard scalar limits and the
/// nested `[thresholds.soft]` overrides. Hard values must be numbers;
/// the `soft` key must be a sub-table whose values are numbers or
/// `"<ratio>x"` scale strings. Any other shape is a config error —
/// callers `die` on `Err` so a malformed table never silently degrades
/// into a missing limit.
pub(crate) fn split_thresholds_table(
    raw: &BTreeMap<String, toml::Value>,
) -> Result<ParsedThresholds, String> {
    let mut out = ParsedThresholds::default();
    for (key, value) in raw {
        if key == SOFT_SUBTABLE_KEY {
            let table = value.as_table().ok_or_else(|| {
                "[thresholds.soft] must be a table of `metric = <number|\"ratiox\">` entries"
                    .to_string()
            })?;
            for (name, sub) in table {
                out.soft.insert(name.clone(), parse_soft_value(name, sub)?);
            }
        } else {
            out.hard.insert(key.clone(), threshold_scalar(key, value)?);
        }
    }
    Ok(out)
}

/// Parse a hard-tier scalar limit. Accepts TOML integers and floats;
/// `i64 -> f64` is exact for the small limits metrics carry in practice.
#[allow(clippy::cast_precision_loss)]
fn threshold_scalar(name: &str, value: &toml::Value) -> Result<f64, String> {
    match value {
        toml::Value::Integer(i) => Ok(*i as f64),
        toml::Value::Float(f) => Ok(*f),
        other => Err(format!(
            "[thresholds] {name:?}: expected a number, got {}",
            other.type_str()
        )),
    }
}

/// Parse one `[thresholds.soft]` value: a number (absolute) or a
/// `"<ratio>x"` scale string.
#[allow(clippy::cast_precision_loss)]
fn parse_soft_value(name: &str, value: &toml::Value) -> Result<SoftLimit, String> {
    match value {
        toml::Value::Integer(i) => Ok(SoftLimit::Absolute(*i as f64)),
        toml::Value::Float(f) => Ok(SoftLimit::Absolute(*f)),
        toml::Value::String(s) => parse_scale_str(name, s),
        other => Err(format!(
            "[thresholds.soft] {name:?}: expected a number or a \"<ratio>x\" scale \
             string (e.g. \"0.95x\"), got {}",
            other.type_str()
        )),
    }
}

/// Parse a `"<ratio>x"` scale string (case-insensitive `x` suffix). The
/// factor must lie in `(0, 1]`, matching `--headroom`: a soft tier looser
/// than the hard tier is never the intent (the soft tier is an
/// early-warning band that fires *before* the hard gate).
fn parse_scale_str(name: &str, s: &str) -> Result<SoftLimit, String> {
    let trimmed = s.trim();
    let factor_str = trimmed
        .strip_suffix('x')
        .or_else(|| trimmed.strip_suffix('X'))
        .ok_or_else(|| {
            format!(
                "[thresholds.soft] {name:?}: scale string {s:?} must end in `x` (e.g. \"0.95x\")"
            )
        })?;
    let factor: f64 = factor_str
        .trim()
        .parse()
        .map_err(|e| format!("[thresholds.soft] {name:?}: invalid scale factor in {s:?}: {e}"))?;
    if !is_valid_scale_ratio(factor) {
        return Err(format!(
            "[thresholds.soft] {name:?}: scale factor must be in (0, 1]; got {factor}"
        ));
    }
    Ok(SoftLimit::Scale(factor))
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
    /// Configured limit.
    pub(crate) limit: f64,
    /// Normalized hash of the function body, populated only when
    /// `--baseline-fuzzy-match` is active (see [`crate::baseline`]).
    /// `None` otherwise. Used as the last-resort baseline matcher when
    /// the qualified symbol changed (a rename that kept the body shape).
    pub(crate) body_hash: Option<u64>,
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
        format!(
            "{}: {} = {} (limit {})",
            self.function,
            self.metric,
            MetricScalar(self.value),
            MetricScalar(self.limit),
        )
    }

    /// The `value / limit` ratio used to rank violation severity.
    /// Saturates to `f64::INFINITY` when the configured limit is
    /// zero ("no value permitted") so a NaN never escapes into
    /// downstream sorts — `total_cmp` then ranks the violation
    /// above all finite-ratio ones, which matches the user
    /// intuition that "0 is the strictest possible limit".
    pub(crate) fn ratio(&self) -> f64 {
        if self.limit > 0.0 {
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

/// One `::`-segment for a space in the qualified-symbol chain.
///
/// The top-level (`Unit`) space is the file itself; it carries no
/// symbol segment (its identity is the `path` key) so it never prefixes
/// the functions inside it. Named spaces contribute their AST-derived
/// name. Anonymous spaces — closures and lambdas, which every grammar
/// surfaces as the literal `<anonymous>`, plus the `None`-name
/// parse-failure case — collapse to `<anon@L{start_line}>` so they keep
/// a stable-within-a-snapshot identity. Baking the line into the segment
/// means an anonymous function re-keys when it moves (the documented
/// degradation in `recipes/baselines.md`); named functions do not.
fn space_segment(space: &FuncSpace) -> String {
    const ANONYMOUS: &str = "<anonymous>";
    match space.name.as_deref() {
        Some(name) if name != ANONYMOUS => name.to_owned(),
        _ => format!("<anon@L{}>", space.start_line),
    }
}

/// The qualified symbol of `space`, given the `::`-joined symbol of its
/// enclosing chain (`parent_prefix`, empty at file top level). `Unit`
/// collapses to `<file>`; everything else appends its [`space_segment`].
fn qualified_symbol(space: &FuncSpace, parent_prefix: &str) -> String {
    if matches!(space.kind, SpaceKind::Unit) {
        return "<file>".to_owned();
    }
    let segment = space_segment(space);
    if parent_prefix.is_empty() {
        segment
    } else {
        format!("{parent_prefix}::{segment}")
    }
}

/// Pre-resolved set of thresholds: every name has been validated against
/// the registry, so evaluation can skip name lookups.
#[derive(Debug)]
pub(crate) struct ThresholdSet {
    entries: Vec<(&'static MetricExtractor, f64)>,
}

impl ThresholdSet {
    /// Build from a `metric=limit` map (CLI flags merged on top of TOML).
    /// Unknown metric names produce an error listing the valid set, rather
    /// than being silently ignored.
    pub(crate) fn build(raw: &BTreeMap<String, f64>) -> Result<Self, String> {
        let mut entries = Vec::with_capacity(raw.len());
        for (name, limit) in raw {
            let extractor = lookup_extractor(name).ok_or_else(|| {
                let known = known_metric_names();
                format!(
                    "unknown threshold metric {name:?}{}; known metrics: {}",
                    crate::threshold_suggestion::format_suggestion(name, &known),
                    known.join(", ")
                )
            })?;
            if !limit.is_finite() || *limit < 0.0 {
                return Err(format!(
                    "limit for {name:?} must be a finite non-negative number; got {limit}"
                ));
            }
            entries.push((extractor, *limit));
        }
        Ok(Self { entries })
    }

    /// True when no thresholds are configured. A check run with no
    /// thresholds is a usage error, not a clean pass.
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate the resolved `(name, limit)` pairs. Used by
    /// `--print-effective-config` to serialize the post-merge view of
    /// the threshold layers (TOML config + `--threshold` CLI overrides)
    /// without re-deriving the order or duplicating the registry's
    /// canonical metric names.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&'static str, f64)> + '_ {
        self.entries.iter().map(|(e, limit)| (e.name, *limit))
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
        out: &mut Vec<Violation>,
    ) {
        // The top-level Unit's `suppressed` carries every `allow-file`
        // marker in the file; it ORs with each function's own scope
        // during the per-violation check below. `honor` gates the
        // entire suppression path so `--no-suppress` (Ignore) emits
        // every threshold violation regardless of source markers.
        //
        // On the root iteration `current` *is* `space`, so the OR
        // below evaluates the same `BTreeSet::contains` twice on the
        // same reference. The second probe is O(log n) on a tiny set
        // and dominated by the threshold-check loop itself; keeping
        // the OR uniform avoids a special-case branch.
        let honor = matches!(policy, SuppressionPolicy::Honor);
        let file_scope = &space.suppressed;

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
            for (extractor, limit) in &self.entries {
                let value = (extractor.extract)(&current.metrics);
                if value <= *limit {
                    continue;
                }
                if honor
                    && let Some(kind) = MetricKind::for_threshold_name(extractor.name)
                    && (file_scope.covers(kind) || current.suppressed.covers(kind))
                {
                    continue;
                }
                out.push(Violation {
                    path: path.to_path_buf(),
                    start_line: current.start_line,
                    end_line: current.end_line,
                    function: qualified.clone(),
                    metric: extractor.name,
                    value,
                    limit: *limit,
                    body_hash: None,
                });
            }
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
