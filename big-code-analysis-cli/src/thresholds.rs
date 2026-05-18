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

impl fmt::Display for Violation {
    /// Stable, parseable single-line format:
    /// `<path>:<start>-<end>: <function>: <metric> = <value> (limit <limit>)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `Path::display` is lossy on non-UTF-8 paths (U+FFFD
        // substitution); acceptable here because Display is the
        // human-facing stderr line, not an identifier. The raw bytes
        // are preserved on `self.path` itself for downstream
        // structured consumers.
        write!(
            f,
            "{}:{}-{}: {}: {} = {} (limit {})",
            self.path.display(),
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
mod tests {
    use super::*;

    /// Locks the threshold-engine extractor vocabulary against
    /// `MetricKind::for_threshold_name` so the two stay in sync.
    /// If a new threshold extractor is added without a matching
    /// suppression mapping (or vice versa), this test fails loudly
    /// rather than silently dropping suppression for the new metric.
    /// `tokens` is the documented exception: it is never suppressible
    /// (see `src/suppression.rs::for_threshold_name`).
    #[test]
    fn every_extractor_resolves_to_metric_kind_or_is_tokens() {
        for extractor in EXTRACTORS {
            let is_suppressible = MetricKind::for_threshold_name(extractor.name).is_some();
            let expected = extractor.name != "tokens";
            assert_eq!(
                is_suppressible, expected,
                "extractor `{}` suppressibility mismatch — expected {expected}, got {is_suppressible}",
                extractor.name,
            );
        }
    }

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

    /// Non-UTF-8 path bytes must survive the threshold pipeline
    /// byte-for-byte. Pre-#240 the `Violation::path: String` field
    /// (built from `&str` via `to_string()`) discarded them at the
    /// `evaluate` boundary. Gated on `cfg(unix)` because
    /// `OsString::from_vec` is Unix-only — Windows paths are
    /// constrained differently (WTF-8) and out of scope for this
    /// regression.
    #[cfg(unix)]
    #[test]
    fn violation_path_preserves_non_utf8_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        use std::path::PathBuf;

        // 0xFF / 0xFE form a lone surrogate pair under UTF-8 and
        // would have been replaced with U+FFFD by `to_string_lossy`.
        let raw_bytes: &[u8] = b"non-utf8-\xff\xfe.rs";
        let path = PathBuf::from(OsString::from_vec(raw_bytes.to_vec()));

        let v = Violation {
            path: path.clone(),
            start_line: 1,
            end_line: 1,
            function: "f".to_string(),
            metric: "cyclomatic",
            value: 5.0,
            limit: 1.0,
        };

        // Raw bytes round-trip identically — no lossy substitution.
        assert_eq!(v.path.as_os_str().as_encoded_bytes(), raw_bytes);
        // Display does not panic on non-UTF-8 bytes (uses
        // `Path::display`, which substitutes U+FFFD).
        let rendered = v.to_string();
        assert!(rendered.contains("cyclomatic"), "{rendered}");
    }

    use big_code_analysis::{SpaceKind, SuppressionScope};
    use std::collections::BTreeSet;

    /// Build a leaf `FuncSpace` with no children. Cyclomatic defaults to
    /// `1.0`, so a `limit = 0` makes the threshold fire deterministically
    /// without forcing the suppression tests to construct a real parse.
    fn space(name: &str, kind: SpaceKind, suppressed: SuppressionScope) -> FuncSpace {
        FuncSpace {
            name: Some(name.into()),
            name_was_lossy: false,
            start_line: 1,
            end_line: 10,
            kind,
            spaces: Vec::new(),
            metrics: CodeMetrics::default(),
            suppressed,
        }
    }

    fn threshold_set(name: &str, limit: f64) -> ThresholdSet {
        let mut raw = BTreeMap::new();
        raw.insert(name.into(), limit);
        ThresholdSet::build(&raw).expect("threshold builds")
    }

    fn only_func_scope(metric: MetricKind) -> SuppressionScope {
        SuppressionScope::Some(BTreeSet::from([metric]))
    }

    #[test]
    fn honor_policy_suppresses_matching_function_scope() {
        // `bca: allow(cyclomatic)` on the function silences a cyclomatic
        // violation when the policy honors markers — the headline
        // behaviour the CLI relies on.
        let mut out = Vec::new();
        let s = space(
            "noisy",
            SpaceKind::Function,
            only_func_scope(MetricKind::Cyclomatic),
        );
        threshold_set("cyclomatic", 0.0).evaluate_with_policy(
            Path::new("fixture.rs"),
            &s,
            SuppressionPolicy::Honor,
            &mut out,
        );
        assert!(
            out.is_empty(),
            "matching function-scoped marker should silence, got {out:?}",
        );
    }

    #[test]
    fn honor_policy_emits_for_non_matching_metric() {
        // A marker covering only `cognitive` must not silence a
        // `cyclomatic` violation — symmetry with the previous test.
        let mut out = Vec::new();
        let s = space(
            "noisy",
            SpaceKind::Function,
            only_func_scope(MetricKind::Cognitive),
        );
        threshold_set("cyclomatic", 0.0).evaluate_with_policy(
            Path::new("fixture.rs"),
            &s,
            SuppressionPolicy::Honor,
            &mut out,
        );
        assert_eq!(out.len(), 1, "expected one violation; got {out:?}");
        assert_eq!(out[0].metric, "cyclomatic");
    }

    #[test]
    fn ignore_policy_emits_despite_matching_marker() {
        // `--no-suppress` (Ignore) must surface violations even when the
        // function carries a covering marker — that's the audit path.
        let mut out = Vec::new();
        let s = space(
            "noisy",
            SpaceKind::Function,
            only_func_scope(MetricKind::Cyclomatic),
        );
        threshold_set("cyclomatic", 0.0).evaluate_with_policy(
            Path::new("fixture.rs"),
            &s,
            SuppressionPolicy::Ignore,
            &mut out,
        );
        assert_eq!(out.len(), 1, "expected one violation; got {out:?}");
    }

    #[test]
    fn file_scope_silences_nested_function() {
        // `allow-file(cyclomatic)` lives on the top-level Unit space
        // and must apply to every nested function too. The nested
        // function carries the default (empty) scope; suppression
        // comes entirely from the file scope.
        let mut out = Vec::new();
        let mut unit = space(
            "fixture.rs",
            SpaceKind::Unit,
            only_func_scope(MetricKind::Cyclomatic),
        );
        unit.spaces.push(space(
            "inner",
            SpaceKind::Function,
            SuppressionScope::default(),
        ));
        threshold_set("cyclomatic", 0.0).evaluate_with_policy(
            Path::new("fixture.rs"),
            &unit,
            SuppressionPolicy::Honor,
            &mut out,
        );
        assert!(
            out.is_empty(),
            "file-scoped marker should also silence nested fn; got {out:?}",
        );
    }

    #[test]
    fn tokens_threshold_never_suppressed() {
        // `MetricKind::for_threshold_name("tokens")` returns None, so
        // the evaluator cannot map the threshold name onto any
        // suppression metric family. Result: even a function carrying
        // `SuppressionScope::All` fails to silence a `tokens`
        // violation. This is intentional — `tokens` is a hard
        // resource cap (not a maintainability heuristic), and we
        // don't want markers turning it off.
        //
        // We construct ThresholdSet manually with limit `-0.5` so
        // tokens_sum default of 0.0 still exceeds it, since
        // `ThresholdSet::build` rejects negative limits.
        assert_eq!(MetricKind::for_threshold_name("tokens"), None);

        let extractor = EXTRACTORS
            .iter()
            .find(|e| e.name == "tokens")
            .expect("tokens extractor exists");
        let set = ThresholdSet {
            entries: vec![(extractor, -0.5)],
        };

        let mut out = Vec::new();
        let s = space("noisy", SpaceKind::Function, SuppressionScope::All);
        set.evaluate_with_policy(
            Path::new("fixture.rs"),
            &s,
            SuppressionPolicy::Honor,
            &mut out,
        );
        assert_eq!(
            out.len(),
            1,
            "tokens violation must survive SuppressionScope::All",
        );
        assert_eq!(out[0].metric, "tokens");
    }
}
