//! Threshold engine for `bca check`.
//!
//! Maps stable metric names (the same set surfaced by `bca list-metrics`,
//! plus dotted names for sub-metrics that don't reduce to a single scalar
//! such as `halstead.volume` or `loc.lloc`) to scalar extractors that read
//! per-function values out of [`big_code_analysis::CodeMetrics`].
//!
//! `ThresholdSet::evaluate` walks a [`FuncSpace`] tree and yields one
//! [`Violation`] per `(function, metric)` pair whose value exceeds its
//! configured limit.

#![allow(clippy::doc_markdown)]

use std::collections::BTreeMap;
use std::fmt;

use big_code_analysis::{CodeMetrics, FuncSpace, SpaceKind};
use serde::Deserialize;

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
    pub(crate) path: String,
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

impl fmt::Display for Violation {
    /// Stable, parseable single-line format:
    /// `<path>:<start>-<end>: <function>: <metric> = <value> (limit <limit>)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}-{}: {}: {} = {} (limit {})",
            self.path,
            self.start_line,
            self.end_line,
            self.function,
            self.metric,
            MetricScalar(self.value),
            MetricScalar(self.limit),
        )
    }
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
    /// `(function, metric)` pair to `out`.
    ///
    /// The walk is iterative (not recursive) so an adversarially deeply
    /// nested AST cannot overflow the worker thread's stack — the
    /// thread pool's default 2 MiB stack is small enough that pathological
    /// input matters. See lesson 13 in `docs/development/lessons_learned.md`
    /// for the analogous web-service DoS vector.
    ///
    /// `path` is the UTF-8 path to use in violation records; the caller
    /// is responsible for handling non-UTF-8 paths (skip + warn) so this
    /// module never has to emit a lossy U+FFFD into the structured
    /// stderr contract.
    ///
    /// For the top-level (`SpaceKind::Unit`) space we substitute the
    /// literal `<file>` for the function slot: `FuncSpace::name` is the
    /// file path there (post #128), so without the substitution the
    /// offender line would read `path:1-100: path: cyclomatic = ...`
    /// — the path doubled. `<file>` makes the file-level emission
    /// distinguishable and keeps aggregate metrics like `loc.sloc`
    /// usable.
    pub(crate) fn evaluate(&self, path: &str, space: &FuncSpace, out: &mut Vec<Violation>) {
        let mut stack: Vec<&FuncSpace> = vec![space];
        while let Some(current) = stack.pop() {
            let function = function_token(current);
            for (extractor, limit) in &self.entries {
                let value = (extractor.extract)(&current.metrics);
                if value > *limit {
                    out.push(Violation {
                        path: path.to_string(),
                        start_line: current.start_line,
                        end_line: current.end_line,
                        function: function.to_owned(),
                        metric: extractor.name,
                        value,
                        limit: *limit,
                    });
                }
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
mod tests {
    use super::*;

    #[test]
    fn parse_cli_threshold_accepts_integer() {
        let (name, limit) = parse_cli_threshold("cyclomatic=15").expect("parses");
        assert_eq!(name, "cyclomatic");
        assert_eq!(limit, 15.0);
    }

    #[test]
    fn parse_cli_threshold_accepts_dotted_name_and_float() {
        let (name, limit) = parse_cli_threshold("halstead.volume=12.5").expect("parses");
        assert_eq!(name, "halstead.volume");
        assert_eq!(limit, 12.5);
    }

    #[test]
    fn parse_cli_threshold_accepts_zero() {
        // `0` is meaningful: "no value allowed" is distinct from "no
        // threshold set". Must parse, not be rejected as falsy.
        let (_, limit) = parse_cli_threshold("nargs=0").expect("parses");
        assert_eq!(limit, 0.0);
    }

    #[test]
    fn parse_cli_threshold_rejects_missing_equals() {
        let err = parse_cli_threshold("cyclomatic15").expect_err("missing `=` must error");
        assert!(err.contains("metric=limit"), "{err}");
    }

    #[test]
    fn parse_cli_threshold_rejects_empty_name() {
        let err = parse_cli_threshold("=15").expect_err("empty name must error");
        assert!(err.contains("empty metric name"), "{err}");
    }

    #[test]
    fn parse_cli_threshold_rejects_negative_limit() {
        let err = parse_cli_threshold("cyclomatic=-1").expect_err("negative limit must error");
        assert!(err.contains("non-negative"), "{err}");
    }

    #[test]
    fn parse_cli_threshold_rejects_nan_limit() {
        let err = parse_cli_threshold("cyclomatic=nan").expect_err("NaN limit must error");
        assert!(err.contains("non-negative"), "{err}");
    }

    #[test]
    fn build_rejects_unknown_metric() {
        let mut raw = BTreeMap::new();
        raw.insert("not_a_metric".to_string(), 1.0);
        let err = ThresholdSet::build(&raw).expect_err("unknown name");
        assert!(err.contains("unknown threshold metric"), "{err}");
        assert!(err.contains("not_a_metric"), "{err}");
    }

    #[test]
    fn build_accepts_zero_limit() {
        let mut raw = BTreeMap::new();
        raw.insert("nargs".to_string(), 0.0);
        ThresholdSet::build(&raw).expect("zero limit is valid");
    }

    #[test]
    fn known_metric_names_contains_core_set() {
        let names = known_metric_names();
        for required in [
            "cognitive",
            "cyclomatic",
            "halstead.volume",
            "loc.lloc",
            "nargs",
        ] {
            assert!(
                names.contains(&required),
                "missing {required:?} in {names:?}"
            );
        }
    }

    #[test]
    fn config_parses_thresholds_table() {
        let toml_src = "[thresholds]\ncyclomatic = 15\n\"loc.lloc\" = 200\n";
        let cfg: ThresholdConfig = toml::from_str(toml_src).expect("parses");
        assert_eq!(cfg.thresholds.get("cyclomatic"), Some(&15.0));
        assert_eq!(cfg.thresholds.get("loc.lloc"), Some(&200.0));
    }

    #[test]
    fn violation_display_is_stable() {
        let v = Violation {
            path: "src/foo.rs".into(),
            start_line: 10,
            end_line: 25,
            function: "do_stuff".into(),
            metric: "cyclomatic",
            value: 17.0,
            limit: 15.0,
        };
        assert_eq!(
            v.to_string(),
            "src/foo.rs:10-25: do_stuff: cyclomatic = 17 (limit 15)"
        );
    }

    #[test]
    fn violation_display_keeps_fractional_precision() {
        let v = Violation {
            path: "x".into(),
            start_line: 1,
            end_line: 1,
            function: String::new(),
            metric: "halstead.volume",
            value: 12.5,
            limit: 10.0,
        };
        assert!(v.to_string().contains("= 12.5"), "{v}");
        assert!(v.to_string().contains("limit 10)"), "{v}");
    }
}
