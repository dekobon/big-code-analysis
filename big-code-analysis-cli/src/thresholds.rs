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
/// ```
#[derive(Debug, Deserialize)]
pub(crate) struct ThresholdConfig {
    #[serde(default)]
    pub(crate) thresholds: BTreeMap<String, f64>,
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
    /// Function/method name (or the file's display name for the top-level
    /// space, e.g. when a file-level metric like `loc.sloc` is checked).
    pub(crate) function: String,
    /// Metric that exceeded its threshold.
    pub(crate) metric: &'static str,
    /// Observed metric value.
    pub(crate) value: f64,
    /// Configured limit.
    pub(crate) limit: f64,
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

/// Resolve the function-slot token for a violation line. Top-level
/// (`Unit`) spaces collapse to `<file>` so the file path doesn't
/// appear twice; nested spaces carry their AST-derived name, with
/// `<unnamed>` for the rare parse-failure case.
fn function_token(space: &FuncSpace) -> &str {
    if matches!(space.kind, SpaceKind::Unit) {
        "<file>"
    } else {
        space.name.as_deref().unwrap_or("<unnamed>")
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
                format!(
                    "unknown threshold metric {name:?}; known metrics: {}",
                    known_metric_names().join(", ")
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
    /// For the top-level (`SpaceKind::Unit`) space we substitute the
    /// literal `<file>` for the function slot: `FuncSpace::name` is the
    /// file path there (post #128), so without the substitution the
    /// offender line would read `path:1-100: path: cyclomatic = ...`
    /// — the path doubled. `<file>` makes the file-level emission
    /// distinguishable and keeps aggregate metrics like `loc.sloc`
    /// usable.
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

        let mut stack: Vec<&FuncSpace> = vec![space];
        while let Some(current) = stack.pop() {
            let function = function_token(current);
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
                    function: function.to_owned(),
                    metric: extractor.name,
                    value,
                    limit: *limit,
                });
            }
            // Push children in reverse so `pop()` visits them in source
            // order, matching the recursive form's traversal.
            for child in current.spaces.iter().rev() {
                stack.push(child);
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
