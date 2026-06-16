// bca: suppress-file(halstead, loc, nargs, nexits, nom)
// SARIF rendering for the Python bindings; file-level halstead/loc and summed
// nargs/exit/nom are mechanical-writer / many-fn aggregation artifacts.
// cognitive is deliberately NOT suppressed — `collect_offenders`'s nested
// tree-walk is genuine logic and stays baselined.

//! SARIF 2.1.0 rendering for the Python bindings (phase 5/9, #269).
//!
//! `bca.to_sarif(result_or_iter, *, thresholds=None) -> str` walks the
//! `FuncSpace` JSON dict(s) returned by [`analyze`] /
//! [`analyze_source`] / [`analyze_batch`], compares each space's
//! headline metric values against the supplied `thresholds`, and hands
//! the resulting `Vec<OffenderRecord>` to the upstream
//! [`big_code_analysis::write_sarif`] writer so the output shape is
//! byte-equivalent to the CLI's `bca check -O sarif` for the metrics
//! the JSON exposes.
//!
//! # Why this lives in Rust
//!
//! Issue #269 lists two implementation paths: re-serialise the dict
//! back to internal types (option 1) or capture offender records at
//! analyse-time (option 2). When this module was written the upstream
//! `FuncSpace` was `Serialize`-only, ruling out option 1; #532 has
//! since added `big_code_analysis::wire::FuncSpace`
//! (`Serialize` + `Deserialize`), so a typed round-trip is now
//! possible. This module nonetheless still walks the JSON shape
//! directly from Rust (via `PyDict` navigation), emits
//! [`OffenderRecord`]s from the values present in the dict, and
//! delegates to the existing `write_sarif` writer: the bindings
//! already hold the metrics as a Python dict, so navigating it
//! directly avoids re-serialising to text only to deserialise into
//! `wire::FuncSpace`. Revisiting that trade-off is a possible
//! follow-up now that option 1 is available. Either way SARIF schema
//! concerns stay on the Rust side (one source of truth) and the
//! duplicate offender cache that option 2 would require is avoided.
//!
//! # Threshold semantics
//!
//! Each entry in the `thresholds` dict maps a CLI-style metric name to
//! a finite, non-negative limit. The mapping table below pins which
//! JSON field is compared against the limit — mirroring the CLI's
//! `EXTRACTORS` table in `big-code-analysis-cli/src/thresholds.rs`.
//!
//! # Interior-space emission
//!
//! The binding emits a finding wherever a space's **own** metric value
//! breaches its limit — at every space level (unit, container, leaf)
//! alike — matching the CLI's per-space accessors exactly.
//!
//! For most metrics the JSON's headline field at any space already IS
//! that space's own value (e.g. `loc.sloc`, `wmc.total`, `mi.original`,
//! `halstead.volume`). Four metrics — `cyclomatic`, `cyclomatic.modified`,
//! `cognitive`, and `abc` — additionally expose a `sum`/`*_sum` aggregate
//! across child spaces that, at interior spaces, exceeds the per-space
//! scalar the CLI thresholds against. Before #958 the wire shape exposed
//! *only* that aggregate for the four, so the binding could compare it
//! safely only at leaf spaces (where aggregate == own) and conservatively
//! skipped interior spaces — under-emitting the interior breaches the CLI
//! reports (#855 closed the over-emission; this residual under-emission
//! remained). As of #958 the wire shape serializes each of the four's
//! per-space own value alongside the aggregate (`cyclomatic.value`,
//! `cyclomatic.modified.value`, `cognitive.value`, `abc.value`), so
//! [`METRIC_FIELDS`] points every metric at the own-value field and the
//! binding emits at every space — no leaf-only special-casing remains.
//!
//! Defaults: `thresholds=None` is equivalent to `thresholds={}` — the
//! CLI itself has no built-in defaults (every check run must supply
//! its own limits), so this binding adopts the same posture. An empty
//! `thresholds` produces a well-formed SARIF run with `results: []`
//! and `rules: []`, matching the CLI's empty case.

use std::path::{Path, PathBuf};

use pyo3::Bound;
use pyo3::PyResult;
use pyo3::Python;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyAnyMethods, PyBool, PyDict, PyDictMethods, PyMapping, PyString};

use big_code_analysis::{OffenderRecord, Severity, write_sarif};

use crate::batch::PyAnalysisError;

/// The literal name every grammar surfaces for an anonymous space
/// (closure / lambda). The CLI's `space_segment` rewrites both this
/// sentinel and the `None`-name parse-failure case to `<anon@L{line}>`
/// so anonymous spaces keep a stable-within-a-snapshot identity; the
/// binding mirrors that rewrite in [`space_segment`].
const ANONYMOUS_SPACE_NAME: &str = "<anonymous>";

/// Symbol emitted for the file-level `unit` space. Matches the CLI's
/// `qualified_symbol` collapse in `big-code-analysis-cli/src/thresholds.rs`
/// (the file path itself lives on `artifactLocation.uri`, so the function
/// slot carries the `<file>` sentinel instead of duplicating it).
const FILE_SYMBOL: &str = "<file>";

/// One metric entry in the threshold-name → JSON-path table.
///
/// Every entry's `path` reaches that space's **own** per-space scalar —
/// the value the CLI thresholds against — so a breach is emitted at
/// every space level. For `cyclomatic`, `cyclomatic.modified`,
/// `cognitive`, and `abc` that scalar is the `value` field the wire
/// shape gained in #958 (the sibling `sum`/`magnitude` field is a
/// subtree aggregate and is deliberately *not* used here); for every
/// other metric the headline JSON field already is the per-space value.
#[derive(Clone, Copy)]
struct MetricField {
    name: &'static str,
    path: &'static [&'static str],
}

/// Mapping from CLI threshold name → JSON `metrics.<…>` path segments.
///
/// Mirrors the CLI's `EXTRACTORS` table in
/// `big-code-analysis-cli/src/thresholds.rs` so the threshold-name
/// surface is identical across the two front-ends. Each entry is the
/// sequence of dict keys to walk on the space's `metrics` sub-dict to
/// reach the comparison scalar.
///
/// **Sync requirement:** the metric-name set is derived from a single
/// shared registry in the library crate —
/// [`big_code_analysis::metric_catalog::METRICS`] — which both the CLI's
/// `EXTRACTORS` table and this table are checked against (#442).
/// `metric_fields_agree_with_shared_registry` below fails the build if a
/// metric is added to one front-end but not the other. The JSON `path`
/// is the only column unique to this binding (the CLI reaches the same
/// scalar through a typed `CodeMetrics` accessor, not a JSON walk); it is
/// pinned by `metric_field_paths_are_pinned` so a path edit is a
/// deliberate, reviewed change rather than silent drift.
const METRIC_FIELDS: &[MetricField] = &[
    // The four aggregate-shaped metrics read the per-space `value` field
    // (the wire shape's own-scalar projection added in #958), never the
    // sibling `sum`/`magnitude` subtree aggregate — so a breach is
    // reported at every space exactly as the CLI's per-space accessor
    // does (#441, #855, #958).
    MetricField {
        name: "cognitive",
        path: &["cognitive", "value"],
    },
    MetricField {
        name: "cyclomatic",
        path: &["cyclomatic", "value"],
    },
    MetricField {
        name: "cyclomatic.modified",
        path: &["cyclomatic", "modified", "value"],
    },
    MetricField {
        name: "abc",
        path: &["abc", "value"],
    },
    // Everything below: the headline JSON field already is the CLI's
    // per-space accessor.
    MetricField {
        name: "halstead.volume",
        path: &["halstead", "volume"],
    },
    MetricField {
        name: "halstead.difficulty",
        path: &["halstead", "difficulty"],
    },
    MetricField {
        name: "halstead.effort",
        path: &["halstead", "effort"],
    },
    MetricField {
        name: "halstead.time",
        path: &["halstead", "time"],
    },
    MetricField {
        name: "halstead.bugs",
        path: &["halstead", "bugs"],
    },
    MetricField {
        name: "loc.sloc",
        path: &["loc", "sloc"],
    },
    MetricField {
        name: "loc.ploc",
        path: &["loc", "ploc"],
    },
    MetricField {
        name: "loc.lloc",
        path: &["loc", "lloc"],
    },
    MetricField {
        name: "loc.cloc",
        path: &["loc", "cloc"],
    },
    MetricField {
        name: "loc.blank",
        path: &["loc", "blank"],
    },
    MetricField {
        name: "nom",
        path: &["nom", "total"],
    },
    MetricField {
        name: "tokens",
        path: &["tokens", "tokens"],
    },
    MetricField {
        name: "nexits",
        path: &["nexits", "sum"],
    },
    MetricField {
        name: "nargs",
        path: &["nargs", "total"],
    },
    MetricField {
        name: "mi.original",
        path: &["mi", "original"],
    },
    MetricField {
        name: "mi.sei",
        path: &["mi", "sei"],
    },
    MetricField {
        name: "mi.visual_studio",
        path: &["mi", "visual_studio"],
    },
    MetricField {
        name: "wmc",
        path: &["wmc", "total"],
    },
    MetricField {
        name: "npm",
        path: &["npm", "total"],
    },
    MetricField {
        name: "npa",
        path: &["npa", "total"],
    },
];

/// Pre-validated `(JSON-path, limit)` triple.
struct Threshold {
    /// CLI-style metric name (e.g. `"cyclomatic"`, `"loc.lloc"`); used
    /// as the SARIF `ruleId` so the rule descriptions the upstream
    /// writer attaches resolve correctly.
    name: &'static str,
    /// Sequence of dict keys to walk to reach the per-space scalar value
    /// on a space's `metrics` sub-dict.
    path: &'static [&'static str],
    /// Threshold the metric value is compared against to emit a finding.
    /// The comparison direction depends on `lower_is_worse`; in both
    /// directions the exact-limit value is acceptable (strict `<`/`>`,
    /// not `<=`/`>=`), mirroring the CLI.
    limit: f64,
    /// True for lower-is-worse metrics (`mi.*`): a value strictly BELOW
    /// the limit is the violation, not above it. Sourced from the
    /// library metric catalog so it stays in lockstep with the CLI gate
    /// (#698).
    lower_is_worse: bool,
}

/// Builds the "unknown threshold metric" error for `name`, listing the
/// known metric names. The known-metrics list is joined lazily here so
/// it is only allocated on the error path, never for valid lookups.
fn unknown_threshold_metric_err(name: &str) -> PyErr {
    let known = METRIC_FIELDS
        .iter()
        .map(|m| m.name)
        .collect::<Vec<_>>()
        .join(", ");
    PyValueError::new_err(format!(
        "unknown threshold metric {name:?}; known metrics: {known}"
    ))
}

fn resolve_thresholds(thresholds: Option<&Bound<'_, PyDict>>) -> PyResult<Vec<Threshold>> {
    let Some(dict) = thresholds else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(dict.len());
    for (key, value) in dict {
        let name: String = key
            .extract()
            .map_err(|_| PyTypeError::new_err("thresholds keys must be strings (metric names)"))?;
        // Match the CLI's `parse_cli_threshold` rejection (empty
        // metric name with the same dedicated message — not the
        // generic "unknown threshold metric" path).
        if name.is_empty() {
            return Err(PyValueError::new_err(
                "empty metric name in thresholds; metric names must be non-empty",
            ));
        }
        let limit: f64 = value.extract().map_err(|_| {
            PyTypeError::new_err(format!("threshold for {name:?} must be a number"))
        })?;
        if !limit.is_finite() || limit < 0.0 {
            return Err(PyValueError::new_err(format!(
                "threshold for {name:?} must be a finite non-negative number; got {limit}"
            )));
        }
        let entry = METRIC_FIELDS
            .iter()
            .copied()
            .find(|m| m.name == name)
            .ok_or_else(|| unknown_threshold_metric_err(&name))?;
        let lower_is_worse = big_code_analysis::metric_catalog::lower_is_worse(entry.name);
        out.push(Threshold {
            name: entry.name,
            path: entry.path,
            limit,
            lower_is_worse,
        });
    }
    Ok(out)
}

/// Walk one `metrics.<…>` JSON path and return the final scalar, if
/// every step lands on a dict that contains the next key and the leaf
/// is a finite `f64`.
///
/// Returns `None` rather than raising for any missing-key /
/// wrong-type case: the result dict may legitimately omit metric
/// families (when `analyze(metrics=[…])` was called with a subset),
/// and an absent metric must not synthesise a finding. NaN and
/// non-finite values also return `None` — comparing NaN against the
/// limit would suppress the finding anyway (NaN > x is false), but
/// rejecting it explicitly keeps the contract self-documenting.
///
/// Python `bool` is rejected explicitly: `bool` inherits from `int`
/// in Python, so `PyO3`'s `extract::<f64>()` would silently coerce
/// `True` → `1.0` / `False` → `0.0`. That coercion is never what the
/// upstream library serialises (metric values are always JSON numbers),
/// and accepting it would mean an adversarial / hand-crafted dict
/// could smuggle a finding through.
fn extract_metric(metrics: &Bound<'_, PyAny>, path: &[&str]) -> Option<f64> {
    let mut current: Bound<'_, PyAny> = metrics.clone();
    for key in path {
        let dict = current.cast::<PyDict>().ok()?;
        current = dict.get_item(key).ok().flatten()?;
    }
    if current.is_instance_of::<PyBool>() {
        return None;
    }
    let value: f64 = current.extract().ok()?;
    value.is_finite().then_some(value)
}

/// Extract a `u32` line number, tolerating values that exceed `u32::MAX`
/// by clamping to `u32::MAX` (matching the CLI's
/// `u32::try_from(usize).unwrap_or(u32::MAX)` in
/// `check_format::violation_to_offender`). Returns `None` for missing
/// keys, non-integer values, negatives, and Python `bool`.
///
/// Python `bool` is rejected for the same reason [`extract_metric`]
/// rejects it: `bool` inherits from `int`, so `PyO3`'s
/// `extract::<i64>()` would silently coerce `True` → 1 / `False` → 0.
/// A hand-crafted dict with `start_line: True` should not produce a
/// finding at line 1 by accident.
fn extract_line_number(
    space: &Bound<'_, PyDict>,
    key: &Bound<'_, PyString>,
) -> PyResult<Option<u32>> {
    let Some(value) = space.get_item(key)? else {
        return Ok(None);
    };
    if value.is_instance_of::<PyBool>() {
        return Ok(None);
    }
    // `i64` covers Python's typical int range; reject negatives and
    // clamp positive overflow to `u32::MAX` to match the CLI fallback.
    let Ok(raw) = value.extract::<i64>() else {
        return Ok(None);
    };
    if raw < 0 {
        return Ok(None);
    }
    Ok(Some(u32::try_from(raw).unwrap_or(u32::MAX)))
}

/// The per-space fields [`collect_offenders`] needs once per space,
/// independent of how many thresholds are checked against it. Computed
/// in [`extract_space_fields`] so the threshold loop stays a thin
/// compare-and-push.
struct SpaceFields {
    /// Whether the space is the file-level `unit` space (drives the
    /// `<file>` function-name rule and the empty child prefix).
    is_unit: bool,
    /// This space's own `::`-segment in the qualified-symbol chain (the
    /// CLI's `space_segment`): the AST-derived name for a named space, or
    /// `<anon@L{start_line}>` for anonymous / unnamed spaces. The unit
    /// root contributes no segment (its identity is the path key); the
    /// final SARIF function name is assembled from the enclosing chain in
    /// [`collect_offenders`], mirroring the CLI's `qualified_symbol`.
    segment: String,
    /// 1-based start line (0 when absent), clamped to `u32::MAX`.
    start_line: u32,
    /// 1-based end line (falls back to `start_line` when absent).
    end_line: u32,
}

/// Extracts the per-space fields used to build an [`OffenderRecord`].
///
/// Mixed error semantics, replicated from the original inline code:
/// `kind` and `name` swallow extraction errors to `None` (a missing or
/// malformed kind/name must not abort the walk), while `start_line` /
/// `end_line` propagate via `?` (a hard `PyO3` error there is a real
/// failure, not a tolerable absence — [`extract_line_number`] already
/// maps tolerable cases to `Ok(None)`).
fn extract_space_fields(space: &Bound<'_, PyDict>) -> PyResult<SpaceFields> {
    let py = space.py();

    let kind: Option<String> = space
        .get_item(intern!(py, "kind"))?
        .and_then(|k| k.extract::<String>().ok())
        // Normalise case so an upstream rename or a hand-crafted dict
        // using "Unit" instead of "unit" still resolves to the unit
        // space (driving the `<file>` symbol and the empty child
        // prefix). Upstream serialises with
        // `#[serde(rename_all = "lowercase")]` today; this is a
        // defensive lowercase guard against future drift.
        .map(|s| s.to_ascii_lowercase());
    let is_unit = kind.as_deref() == Some("unit");

    let space_name: Option<String> = space
        .get_item(intern!(py, "name"))?
        .and_then(|n| n.extract().ok());
    let start_line: u32 = extract_line_number(space, intern!(py, "start_line"))?.unwrap_or(0);
    let end_line: u32 = extract_line_number(space, intern!(py, "end_line"))?.unwrap_or(start_line);

    // This space's own `::`-segment in the qualified-symbol chain.
    // Mirrors the CLI's `space_segment`
    // (`big-code-analysis-cli/src/thresholds.rs`): a named space
    // contributes its AST name; an anonymous space — the literal
    // `<anonymous>` every grammar emits for closures/lambdas, plus the
    // `None`-name parse-failure case — collapses to `<anon@L{start_line}>`
    // so it keeps a stable-within-a-snapshot identity. The unit root's
    // segment is unused (`collect_offenders` emits `<file>` for it), so
    // computing it unconditionally here is harmless.
    let segment = match space_name.as_deref() {
        Some(name) if name != ANONYMOUS_SPACE_NAME => name.to_string(),
        _ => format!("<anon@L{start_line}>"),
    };

    Ok(SpaceFields {
        is_unit,
        segment,
        start_line,
        end_line,
    })
}

/// The qualified symbol of a space, given the `::`-joined symbol of its
/// enclosing chain (`parent_prefix`, empty at the file top level).
/// Mirrors the CLI's `qualified_symbol`: a unit space collapses to
/// `<file>`; every other space appends its own [`SpaceFields::segment`]
/// to the parent prefix.
fn qualified_symbol(fields: &SpaceFields, parent_prefix: &str) -> String {
    if fields.is_unit {
        return FILE_SYMBOL.to_string();
    }
    if parent_prefix.is_empty() {
        fields.segment.clone()
    } else {
        format!("{parent_prefix}::{}", fields.segment)
    }
}

/// Evaluate one space's `metrics` against every threshold, pushing an
/// [`OffenderRecord`] for each breach. Factored out of [`collect_offenders`]
/// so the tree walk reads as "evaluate this space, then descend" rather than
/// inlining the per-threshold comparison.
fn record_threshold_breaches(
    metrics: &Bound<'_, PyAny>,
    fields: &SpaceFields,
    qualified: &str,
    path: &Path,
    thresholds: &[Threshold],
    out: &mut Vec<OffenderRecord>,
) {
    for threshold in thresholds {
        let Some(value) = extract_metric(metrics, threshold.path) else {
            continue;
        };
        // `mi.*` is lower-is-worse: a value below the limit is the violation.
        // Every other metric flags above the limit. The exact-limit value is
        // acceptable either way (#698).
        let breaches = if threshold.lower_is_worse {
            value < threshold.limit
        } else {
            value > threshold.limit
        };
        if !breaches {
            continue;
        }
        out.push(OffenderRecord {
            path: path.to_path_buf(),
            function: Some(qualified.to_string()),
            start_line: fields.start_line,
            end_line: fields.end_line,
            start_col: None,
            metric: threshold.name.to_string(),
            value,
            limit: threshold.limit,
            severity: Severity::default(),
        });
    }
}

/// Push each child space of `space` onto the walk `stack`, tagged with
/// `child_prefix` as the parent qualified prefix. Non-dict children are
/// skipped (mirroring the original inline tolerance). Factored out of
/// [`collect_offenders`].
fn push_child_spaces<'py>(
    space: &Bound<'py, PyDict>,
    child_prefix: &str,
    stack: &mut Vec<(Bound<'py, PyDict>, String)>,
) -> PyResult<()> {
    let py = space.py();
    if let Some(spaces) = space.get_item(intern!(py, "spaces"))?
        && let Ok(seq) = spaces.try_iter()
    {
        for child in seq {
            let child = child?;
            if let Ok(child_dict) = child.cast_into::<PyDict>() {
                stack.push((child_dict, child_prefix.to_string()));
            }
        }
    }
    Ok(())
}

fn collect_offenders(
    result: &Bound<'_, PyDict>,
    thresholds: &[Threshold],
    out: &mut Vec<OffenderRecord>,
) -> PyResult<()> {
    if thresholds.is_empty() {
        return Ok(());
    }
    let py = result.py();
    let path: PathBuf = match result.get_item(intern!(py, "name"))? {
        Some(v) if !v.is_none() => PathBuf::from(v.extract::<String>()?),
        _ => PathBuf::from("<source>"),
    };

    // Each stack frame carries the popped space's *parent* qualified
    // prefix so a violation can be stamped with the full
    // `Container::method` symbol — mirroring the CLI's
    // `evaluate_with_policy` (issue #377, #706). The file root starts
    // with an empty prefix (it contributes no segment of its own).
    let mut stack: Vec<(Bound<'_, PyDict>, String)> = vec![(result.clone(), String::new())];
    while let Some((space, parent_prefix)) = stack.pop() {
        let Some(metrics) = space.get_item(intern!(py, "metrics"))? else {
            continue;
        };

        let fields = extract_space_fields(&space)?;
        let qualified = qualified_symbol(&fields, &parent_prefix);

        record_threshold_breaches(&metrics, &fields, &qualified, &path, thresholds, out);

        // Children inherit this space's qualified symbol as their prefix,
        // except the file root, which stays empty so a top-level function
        // is `foo`, not `<file>::foo` (matching the CLI walk).
        let child_prefix = if fields.is_unit {
            String::new()
        } else {
            qualified
        };
        push_child_spaces(&space, &child_prefix, &mut stack)?;
    }
    Ok(())
}

/// Render SARIF 2.1.0 JSON for a single result dict or an iterable of
/// them.
///
/// `result` accepts:
/// * a single ``dict`` matching :func:`analyze` / :func:`analyze_source`
///   output, or
/// * a scalar ``None`` (the documented return of :func:`analyze` for
///   generated files); produces a well-formed empty SARIF run, or
/// * any iterable yielding such dicts, :class:`AnalysisError`
///   instances, and/or ``None`` (e.g. the return of
///   :func:`analyze_batch`, or a list comprehension over
///   :func:`analyze` which returns ``None`` for generated files).
///   :class:`AnalysisError` and ``None`` entries are skipped silently
///   — they represent files for which no record was emitted (either
///   the pipeline could not analyse them, or they were classified as
///   generated), not findings.
///
/// Pass ``thresholds={"cyclomatic": 15, "loc.lloc": 200, …}`` to drive
/// finding emission. ``thresholds=None`` (the default) is equivalent
/// to an empty dict and produces a well-formed SARIF run with no
/// results, matching the CLI's no-threshold posture.
///
/// Returns a ``str`` containing SARIF 2.1.0 JSON; the bytes are UTF-8
/// because Python ``str`` is the documented return type. Generated by
/// the upstream :func:`big_code_analysis::write_sarif` writer, so the
/// rule descriptions, tool driver name / version, and schema URL
/// match the CLI's `bca check -O sarif` byte-for-byte.
#[pyfunction]
#[pyo3(signature = (result, /, *, thresholds = None))]
pub(crate) fn to_sarif(
    py: Python<'_>,
    result: &Bound<'_, PyAny>,
    thresholds: Option<&Bound<'_, PyDict>>,
) -> PyResult<String> {
    let offenders = collect_offenders_for_input(result, thresholds)?;
    render(py, &offenders)
}

fn collect_offenders_for_input(
    result: &Bound<'_, PyAny>,
    thresholds: Option<&Bound<'_, PyDict>>,
) -> PyResult<Vec<OffenderRecord>> {
    let thresholds = resolve_thresholds(thresholds)?;
    let mut offenders: Vec<OffenderRecord> = Vec::new();
    dispatch_by_input_kind(result, &thresholds, &mut offenders)?;
    Ok(offenders)
}

fn dispatch_by_input_kind(
    result: &Bound<'_, PyAny>,
    thresholds: &[Threshold],
    offenders: &mut Vec<OffenderRecord>,
) -> PyResult<()> {
    // Scalar `None` — symmetric with the iterable arm's silent-skip
    // contract below. `analyze()` returns `None` for generated files
    // (the documented default), so the natural single-call pattern
    // `bca.to_sarif(bca.analyze(generated_file))` must yield an empty
    // SARIF run rather than raise `TypeError: 'NoneType' is not
    // iterable`. Issue #341 closed the list-comprehension form; this
    // closes the scalar form so both compose with `analyze()`.
    if result.is_none() {
        return Ok(());
    }
    // Single dict — accept as a result without forcing the caller to
    // wrap it in a list.
    if let Ok(dict) = result.clone().cast_into::<PyDict>() {
        return collect_offenders(&dict, thresholds, offenders);
    }
    // Reject Mapping-but-not-dict and `str` early with clear errors —
    // see [`mapping_or_str_error`] for the rationale on each branch.
    if let Some(err) = mapping_or_str_error(result) {
        return Err(err);
    }
    collect_offenders_from_iter(result, thresholds, offenders)
}

/// Return a `TypeError` for the two iterable-shaped inputs that would
/// otherwise yield confusing per-item failures: `Mapping`s that are
/// not `dict` (e.g. `types.MappingProxyType`) iterate as keys, and
/// `str` iterates as one-character substrings. Both fall through the
/// iterable path to a masked "got str" error from the per-item dict
/// cast unless caught here.
fn mapping_or_str_error(result: &Bound<'_, PyAny>) -> Option<PyErr> {
    if result.cast::<PyMapping>().is_ok() {
        return Some(PyTypeError::new_err(concat!(
            "to_sarif: result must be a plain dict ",
            "(got a Mapping that is not a dict — pass `dict(mapping)` ",
            "if you need to wrap a MappingProxyType or similar)",
        )));
    }
    if result.is_instance_of::<PyString>() {
        return Some(PyTypeError::new_err(
            "to_sarif: result must be a dict or an iterable of dicts, not str",
        ));
    }
    None
}

fn collect_offenders_from_iter(
    result: &Bound<'_, PyAny>,
    thresholds: &[Threshold],
    offenders: &mut Vec<OffenderRecord>,
) -> PyResult<()> {
    // `try_iter()` errors if the value is not iterable — let that
    // propagate to the caller as a `TypeError`. Per the documented
    // contract, `AnalysisError` entries are skipped silently (they
    // represent files we couldn't analyse) and `None` entries are
    // likewise skipped (the documented return of :func:`analyze` for
    // generated files — issue #341); anything else that isn't a dict
    // is a programmer error.
    for item in result.try_iter()? {
        let item = item?;
        if item.is_none() || item.is_instance_of::<PyAnalysisError>() {
            continue;
        }
        let dict = cast_iter_item_to_dict(&item)?;
        collect_offenders(&dict, thresholds, offenders)?;
    }
    Ok(())
}

fn cast_iter_item_to_dict<'py>(item: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    item.clone().cast_into::<PyDict>().map_err(|_| {
        // `get_type().name()` can itself fail on objects with a broken
        // `__class__` — fall back to a literal placeholder so the
        // caller still sees a clear "wrong type" error instead of the
        // masked introspection error.
        let type_name = item
            .get_type()
            .name()
            .map_or_else(|_| "<unknown type>".to_string(), |n| n.to_string());
        PyTypeError::new_err(format!(
            "to_sarif expected a result dict, AnalysisError, or None, got {type_name}"
        ))
    })
}

fn render(py: Python<'_>, offenders: &[OffenderRecord]) -> PyResult<String> {
    py.detach(|| {
        let mut buf: Vec<u8> = Vec::new();
        write_sarif(offenders, &mut buf).map_err(|e| {
            PyValueError::new_err(format!("internal error: SARIF serialisation failed: {e}"))
        })?;
        // The upstream writer goes through `serde_json::to_writer_pretty`,
        // which is documented to emit valid UTF-8. This branch is
        // effectively unreachable; surface the violation explicitly
        // rather than papering over it with a lossy conversion.
        String::from_utf8(buf).map_err(|e| {
            PyValueError::new_err(format!(
                "internal error: write_sarif violated its UTF-8 output contract: {e}"
            ))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_fields_table_is_unique_and_non_empty() {
        let mut names: Vec<&str> = METRIC_FIELDS.iter().map(|m| m.name).collect();
        names.sort_unstable();
        let len = names.len();
        names.dedup();
        assert_eq!(names.len(), len, "metric fields table has duplicates");
        assert!(!names.is_empty());
        for entry in METRIC_FIELDS {
            assert!(
                !entry.path.is_empty(),
                "metric field path must be non-empty"
            );
            assert!(
                !entry.name.is_empty(),
                "metric field name must be non-empty"
            );
        }
    }

    /// Cross-crate drift guard (#442). The metric-name set must agree
    /// with the shared library registry
    /// [`big_code_analysis::metric_catalog::METRICS`], which the CLI's
    /// `EXTRACTORS` table is also pinned to (parity test in
    /// `big-code-analysis-cli/src/thresholds.rs`). A metric added to the
    /// CLI but not here (or vice versa) fails this assertion with the
    /// offending name — turning what used to be silent divergence into a
    /// build failure.
    ///
    /// The registry's `skip_at_unit` flag is no longer mirrored here:
    /// since #958 every metric reads its per-space own value (the four
    /// aggregate-shaped metrics via their `value` field), so the binding
    /// emits at every space and the flag has no consumer in this front-end
    /// (it still documents the `sum`-field divergence in the registry).
    #[test]
    fn metric_fields_agree_with_shared_registry() {
        use big_code_analysis::metric_catalog::METRICS;

        let mut ours: Vec<&str> = METRIC_FIELDS.iter().map(|m| m.name).collect();
        let mut registry: Vec<&str> = METRICS.iter().map(|m| m.id).collect();
        ours.sort_unstable();
        registry.sort_unstable();
        assert_eq!(
            ours, registry,
            "METRIC_FIELDS and library metric_catalog::METRICS disagree on \
             metric names; a metric was added to one front-end but not the other"
        );
    }

    /// (iii) JSON-path drift guard (#442). The `path` column is unique to
    /// this binding — the CLI reaches each scalar through a typed
    /// `CodeMetrics` accessor, not a JSON walk — so it cannot be checked
    /// against the registry. Pin it here so a path edit (which silently
    /// changes which JSON field `to_sarif` compares against the limit) is
    /// a deliberate, reviewed change. The expected paths mirror the
    /// `CodeMetrics::Serialize` shape the upstream library emits.
    #[test]
    fn metric_field_paths_are_pinned() {
        let actual: Vec<(&str, Vec<&str>)> = METRIC_FIELDS
            .iter()
            .map(|m| (m.name, m.path.to_vec()))
            .collect();
        let expected: &[(&str, &[&str])] = &[
            ("cognitive", &["cognitive", "value"]),
            ("cyclomatic", &["cyclomatic", "value"]),
            ("cyclomatic.modified", &["cyclomatic", "modified", "value"]),
            ("abc", &["abc", "value"]),
            ("halstead.volume", &["halstead", "volume"]),
            ("halstead.difficulty", &["halstead", "difficulty"]),
            ("halstead.effort", &["halstead", "effort"]),
            ("halstead.time", &["halstead", "time"]),
            ("halstead.bugs", &["halstead", "bugs"]),
            ("loc.sloc", &["loc", "sloc"]),
            ("loc.ploc", &["loc", "ploc"]),
            ("loc.lloc", &["loc", "lloc"]),
            ("loc.cloc", &["loc", "cloc"]),
            ("loc.blank", &["loc", "blank"]),
            ("nom", &["nom", "total"]),
            ("tokens", &["tokens", "tokens"]),
            ("nexits", &["nexits", "sum"]),
            ("nargs", &["nargs", "total"]),
            ("mi.original", &["mi", "original"]),
            ("mi.sei", &["mi", "sei"]),
            ("mi.visual_studio", &["mi", "visual_studio"]),
            ("wmc", &["wmc", "total"]),
            ("npm", &["npm", "total"]),
            ("npa", &["npa", "total"]),
        ];
        let expected: Vec<(&str, Vec<&str>)> =
            expected.iter().map(|(n, p)| (*n, p.to_vec())).collect();
        assert_eq!(
            actual, expected,
            "METRIC_FIELDS JSON paths drifted; if this is intentional, update \
             the pinned list and confirm the path still reaches the scalar the \
             CLI accessor reads"
        );
    }
}
