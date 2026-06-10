// bca: suppress-file(halstead, nargs, nexits)
// PyO3 module registration + thin analyze wrappers; `_native`'s nexits is one
// `?` per `m.add(...)` registration line (boilerplate), and the rest are many-fn
// aggregation artifacts, not per-function logic complexity.

//! `PyO3` entry point for the `big_code_analysis._native` extension
//! module.
//!
//! Everything callable from Python lives here; the heavy lifting is
//! delegated to the `analysis`, `conversion`, and `language`
//! sub-modules so the bindings can be unit-tested at the Rust level
//! without spinning up a Python interpreter.

#![allow(unsafe_op_in_unsafe_fn)]
// The `#[pymodule]` macro expands to an `extern "C"` init function
// that PyO3 marks `#[unsafe(no_mangle)]`. The expansion contains
// unsafe FFI shims that the macro itself wraps in `unsafe { ... }`;
// the lint above silences the workspace-wide warn so this crate stays
// `unsafe`-free at the source level (the only `unsafe` is generated
// by the PyO3 macro).

mod analysis;
mod batch;
// `codegen` exists only to render and drift-check the generated
// `python/big_code_analysis/_enums.py`. It is exercised purely from
// the test harness (the `enums_module_matches_checked_in` gate), so
// gating it on `cfg(test)` keeps the shipped cdylib free of unused-
// code warnings rather than littering the generator with `#[allow]`.
#[cfg(test)]
mod codegen;
mod conversion;
mod language;
mod sarif;
// `types_codegen` renders and drift-checks the generated `_types.py`
// (#623). Like `codegen`, it is exercised only from the test harness, so
// gating it on `cfg(test)` keeps the shipped cdylib free of unused-code
// warnings.
#[cfg(test)]
mod types_codegen;
mod vcs;

use std::path::PathBuf;

use pyo3::Borrowed;
use pyo3::Bound;
use pyo3::PyErr;
use pyo3::PyResult;
use pyo3::Python;
use pyo3::create_exception;
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyByteArray, PyBytes, PyModule, PyModuleMethods, PyString, PyTuple};
use pyo3::wrap_pyfunction;

use big_code_analysis::{Metric, MetricSet};

use crate::analysis::{AnalysisError, AnalyzeOptions, PACKAGE_VERSION};
use crate::batch::{PyAnalysisError, analyze_batch};
use crate::sarif::to_sarif;

// Python exception types. Both subclass `ValueError` per the API
// contract in #265 — `UnsupportedLanguageError` covers unknown
// extensions or explicit-language strings; `ParseError` covers
// tree-sitter parser failures bubbled up through the upstream
// `MetricsError::Parse` family.
create_exception!(
    big_code_analysis._native,
    UnsupportedLanguageError,
    PyValueError,
    "Raised when a file extension or explicit language name is not recognised."
);
create_exception!(
    big_code_analysis._native,
    ParseError,
    PyValueError,
    "Raised when the tree-sitter parser fails on the supplied source."
);

// VCS (change-history) exception taxonomy (#624). All subclass
// `VcsError`, which subclasses `ValueError`, so the catch-all
// `except ValueError` handlers that predate this change keep working
// — the new classes are additive (STABILITY.md error-mapping
// contract). `VcsError` itself is the bucket for client-input option
// failures (bad window / timestamp / formula / scope / threshold /
// trend / bot pattern) the caller most plausibly fixes by editing a
// keyword argument; the three named subclasses below carve out the
// triggers a caller is most likely to branch on. Environment /
// backend failures (opening the repo, walking history, diffing,
// `.mailmap`, blame, cache I/O) map to `VcsEnvironmentError`. This
// split mirrors `vcs::Error::is_client_input` (#641), which the web
// crate uses to choose `400` vs `500`.
create_exception!(
    big_code_analysis._native,
    VcsError,
    PyValueError,
    "Base class for change-history (VCS) surface errors. Subclasses ValueError."
);
create_exception!(
    big_code_analysis._native,
    NotARepositoryError,
    VcsError,
    "Raised when a path is not inside a supported version-control working tree."
);
create_exception!(
    big_code_analysis._native,
    InvalidRevisionError,
    VcsError,
    "Raised when a revision / commit reference cannot be resolved."
);
create_exception!(
    big_code_analysis._native,
    InvalidDiffError,
    VcsError,
    "Raised when a supplied unified diff cannot be parsed."
);
create_exception!(
    big_code_analysis._native,
    VcsEnvironmentError,
    VcsError,
    "Raised when a VCS operation fails for an environment / backend reason \
     (opening the repository, walking history, diffing, .mailmap, blame, cache I/O)."
);

/// Convert a Python-side `metrics=` value into a [`MetricSet`].
///
/// Shared between the three `PyO3` entry points (`analyze`,
/// `analyze_source`, `analyze_batch`) so the rejection-message
/// formatting and the dependency-closure resolution live in exactly
/// one place. `None` is the documented "no selection — compute the
/// full suite" path; otherwise the list is parsed through
/// [`analysis::parse_metric_names`] and any resulting `String` error
/// is wrapped as a `ValueError` to match the issue contract (#268).
pub(crate) fn resolve_metric_set(metrics: Option<Vec<String>>) -> PyResult<MetricSet> {
    match metrics {
        None => Ok(MetricSet::all()),
        // `ParseMetricNamesError::Display` produces the exact
        // user-facing string the issue contract pins; converting
        // via `.to_string()` keeps the formatting policy on the
        // error type rather than smearing it into the Python glue.
        Some(names) => {
            analysis::parse_metric_names(&names).map_err(|e| PyValueError::new_err(e.to_string()))
        }
    }
}

/// Strips Rust's trailing ` (os error N)` from an `io::Error` Display.
///
/// `io::Error`'s `Display` appends that suffix for OS-backed errors;
/// `CPython`'s `OSError` constructor renders `[Errno N]` from the errno
/// argument independently, so the suffix would duplicate the errno in
/// the resulting traceback. Returns the input unchanged when no such
/// suffix is present (e.g. non-OS `io::Error` kinds).
fn strip_os_error_suffix(message: &str) -> &str {
    // The suffix is always the trailing parenthesised run; matching on
    // " (os error " and requiring a `)` terminator avoids clipping a
    // legitimate parenthetical that happens to live mid-message.
    const OS_ERROR_MARKER: &str = " (os error ";
    if let Some(marker_start) = message.rfind(OS_ERROR_MARKER)
        && message.ends_with(')')
    {
        return message[..marker_start].trim_end();
    }
    message
}

/// Convert an internal `AnalysisError` to a concrete Python exception.
///
/// Kept as a free function (rather than a `From<AnalysisError>` impl
/// on `PyErr`) so the orphan rules let it live next to the exception
/// types in this crate.
fn analysis_error_to_py(err: AnalysisError) -> PyErr {
    match err {
        // CPython's `OSError(errno, msg, filename)` 3-tuple
        // constructor dispatches to the right subclass
        // (`FileNotFoundError` for ENOENT, `PermissionError` for
        // EACCES, etc.) and populates `err.errno` / `err.filename`
        // so idiomatic `except FileNotFoundError as e: e.filename`
        // works. Passing `e.to_string()` alone collapses every
        // I/O failure to bare `OSError` with `errno is None`.
        AnalysisError::Io { source, path } => {
            let errno = source.raw_os_error();
            // Rust's `io::Error` Display appends ` (os error N)` for
            // OS-backed errors, but CPython's `OSError(errno, …)`
            // constructor already renders `[Errno N]` from the errno
            // argument — passing the full Display doubles the errno
            // text in the traceback. Strip Rust's suffix so the
            // strerror carries only the human message; CPython owns
            // the errno rendering (#617).
            let msg = strip_os_error_suffix(&source.to_string()).to_owned();
            // `analyze_path` resolves the `FuncSpace.name` *before*
            // `std::fs::read`, so in strict mode (`allow_lossy_path
            // = False`) any path reaching this arm is valid UTF-8
            // and `path.display()` is lossless. In opt-in lossy mode
            // (#316) the caller has explicitly accepted U+FFFD
            // substitution at the `name`-field boundary, and the
            // same convention applies to the `filename` field on
            // the resulting `OSError`: both surfaces are user-facing
            // text, not identifiers, so `display()` is the right
            // call here — the caller has already certified that
            // lossy rendering is acceptable for this path.
            PyOSError::new_err((errno, msg, path.display().to_string()))
        }
        AnalysisError::NonUtf8Path => PyValueError::new_err(
            "path is not valid UTF-8 and cannot be used as a FuncSpace name; \
             pass allow_lossy_path=True to mirror the CLI's U+FFFD substitution",
        ),
        AnalysisError::UnsupportedLanguage(msg) => UnsupportedLanguageError::new_err(msg),
        AnalysisError::Parse(e) => ParseError::new_err(e.to_string()),
        AnalysisError::Serialization(e) => PyValueError::new_err(format!(
            "internal error: failed to serialise FuncSpace to JSON: {e}"
        )),
    }
}

/// Run the metric analysis on a single file path.
///
/// Returns a Python `dict` matching the JSON emitted by
/// `bca metrics --output-format json` for the same input at the
/// `FuncSpace` boundary — same field order, same numeric formatting,
/// same shape. Both sides serialise the same `FuncSpace` through
/// `serde_json::to_string` and the bindings parse that JSON with
/// `CPython`'s `json.loads` (which preserves insertion order).
///
/// Pass `exclude_tests=True` (keyword-only) to mirror the CLI's
/// global `--exclude-tests` flag: the bindings then thread
/// `MetricsOptions::default().with_exclude_tests(true)` into the
/// analysis, which the Rust language-checker hook uses to prune
/// Rust `#[test]` / `#[cfg(test)]` / `#[tokio::test]` subtrees
/// before any metric runs. Languages without a
/// `Checker::should_skip_subtree` override ignore the flag.
///
/// Pass `allow_lossy_path=True` (keyword-only) to mirror the CLI's
/// non-UTF-8 path handling: the `FuncSpace.name` field is filled
/// via `Path::to_string_lossy` (U+FFFD substitution) instead of
/// raising `ValueError`. Default is `False` — strict mode rejects
/// non-UTF-8 paths so the `name` field is always a round-trippable
/// identifier (#316).
///
/// Pass `skip_generated=False` (keyword-only) to opt out of the
/// CLI's generated-file walker filter. The default is `True`,
/// matching the CLI walker: files whose leading window matches
/// `@generated` / `DO NOT EDIT` / `GENERATED CODE` return `None`
/// without parsing. The `is_generated` check runs before language
/// inference so a generated file with an unknown extension still
/// returns `None` rather than raising `UnsupportedLanguageError`
/// (#317).
///
/// Language detection mirrors the CLI — path extension first,
/// then a `#!`-shebang line or emacs `-*- mode: … -*-` declaration
/// via `big_code_analysis::guess_language`. Non-UTF-8 paths match
/// the CLI byte-for-byte when `allow_lossy_path=True`. Generated
/// files are skipped on both sides when `skip_generated=True` (the
/// default), so the parity claim is now exact across all four
/// CLI-walker behaviours.
///
/// Pass `metrics=[<names>]` (keyword-only) to compute only a subset
/// of the metric suite (#268). Each name is a canonical entry in
/// `bca.METRIC_NAMES` (strict lowercase); the empty list raises
/// `ValueError`, an unknown name raises `ValueError` with the valid
/// list in the message. Validation runs *before* the file read, so
/// a bad selection raises without paying I/O cost. Unrequested
/// metrics are absent (not `None`) from the result dict; derived
/// metrics (`mi`, `wmc`) pull their dependencies in automatically.
///
/// Pass `vcs=True` to attach a file-level `vcs` block (#328), or
/// `vcs_per_function=True` to attach a `vcs` block to every nested
/// function space from a single `git blame` (#329 / #578, mirroring
/// `bca metrics --vcs-per-function`). The two are independent: set
/// either or both. Per-function blocks degrade gracefully (the AST
/// metrics still emit) when the file is outside a repository or
/// otherwise unblameable.
#[pyfunction]
#[pyo3(signature = (path, /, *, exclude_tests = false, allow_lossy_path = false, skip_generated = true, metrics = None, vcs = false, vcs_per_function = false))]
#[allow(
    clippy::needless_pass_by_value,
    clippy::fn_params_excessive_bools,
    // Eight keyword-only PyO3 args: each maps to a distinct documented
    // `analyze()` keyword, so a params struct would only obscure the FFI
    // signature CPython binds against.
    clippy::too_many_arguments
)]
// `path: PathBuf` (rather than `&Path`) is mandated by PyO3's
// path conversion: `FromPyObject` materializes a fresh `PathBuf`
// out of the `os.PathLike` argument, and there is no borrow to
// extract a `&Path` from.
fn analyze(
    py: Python<'_>,
    path: PathBuf,
    exclude_tests: bool,
    allow_lossy_path: bool,
    skip_generated: bool,
    metrics: Option<Vec<String>>,
    vcs: bool,
    vcs_per_function: bool,
) -> PyResult<Option<Bound<'_, PyAny>>> {
    // Resolve `metrics=` *before* `py.detach` so a bad name aborts
    // before any file I/O (issue #268 requires the validation to
    // precede the file read; otherwise an unknown name would
    // pointlessly stat the path first).
    let metric_set = resolve_metric_set(metrics)?;
    let opts = AnalyzeOptions {
        exclude_tests,
        allow_lossy_path,
        skip_generated,
        metrics: metric_set,
    };
    // Release the GIL across the file read + tree-sitter parse so
    // other Python threads (e.g. `concurrent.futures.ThreadPoolExecutor.map`
    // around this function, the documented parallelism pattern) can
    // actually run. `analyze_path` touches no Python objects, so the
    // release is sound; the GIL is re-acquired before
    // `json_string_to_py` materialises the Python `dict`. In PyO3
    // 0.28 the spelling is `Python::detach` (renamed from
    // `allow_threads`).
    let result = py
        .detach(|| analysis::analyze_path(&path, opts))
        .map_err(analysis_error_to_py)?;
    // `vcs=True` (#328) attaches a `vcs` block to the file's metrics from
    // a one-shot history walk of its repository. Done after the GIL is
    // re-acquired because the injection helper builds Python errors; for
    // whole-repo ranking prefer `vcs_metrics()`, which walks history once.
    match result {
        None => Ok(None),
        Some(json) => {
            let json = if vcs {
                crate::vcs::inject_vcs(json, &path)?
            } else {
                json
            };
            // `vcs_per_function=True` (#329 / #578) blames the file once and
            // attaches a `vcs` block to every nested function space. It is
            // independent of `vcs=` (which blocks only the file-level space),
            // so both can be set to cover the file and all its functions.
            let json = if vcs_per_function {
                crate::vcs::inject_vcs_per_function(json, &path)?
            } else {
                json
            };
            conversion::json_string_to_py(py, &json).map(Some)
        }
    }
    // Keyword-only kwargs stay split at the PyO3 boundary (PyO3 has
    // no struct-literal binding for `#[pyo3(signature)]`); the
    // `AnalyzeOptions` struct lives on the Rust side of the FFI so
    // every internal callsite reads its policy by name, not by
    // positional bool order.
}

/// Run the metric analysis on an in-memory source buffer.
///
/// `code` accepts `str`, `bytes`, or `bytearray`. `language` is a
/// language name from [`supported_languages`] (case-insensitive).
/// Output shape matches [`analyze`]. `exclude_tests` mirrors the
/// CLI's `--exclude-tests` flag — see [`analyze`] for the details.
///
/// `metrics=` selects which metrics to compute (#268); see
/// [`analyze`] for the full contract.
#[pyfunction]
#[pyo3(signature = (code, language, /, *, exclude_tests = false, metrics = None))]
fn analyze_source<'py>(
    py: Python<'py>,
    code: &Bound<'py, PyAny>,
    language: &str,
    exclude_tests: bool,
    metrics: Option<Vec<String>>,
) -> PyResult<Bound<'py, PyAny>> {
    let bytes = extract_source_bytes(code)?;
    // Same GIL-release pattern as `analyze`: parsing is the heavy
    // step and uses no Python objects. `Ungil` requires `Send`, so
    // `language: &str` is owned into a `String` first because the
    // input borrow ties to `'py` which the detached closure
    // outlives via PyO3's guard.
    let language = language.to_owned();
    // `metrics=` is resolved up front (before `py.detach`) so the
    // unknown-name path raises `ValueError` without paying the
    // parse cost — matches the `analyze` entry point's contract
    // (#268).
    let metric_set = resolve_metric_set(metrics)?;
    let opts = AnalyzeOptions {
        // `analyze_source` has no path / no walker, so the lossy-path
        // and generated-file fields are dead weight here, but
        // threading the whole struct keeps the bridge layer's
        // signature uniform across the three entry points.
        exclude_tests,
        allow_lossy_path: false,
        skip_generated: false,
        metrics: metric_set,
    };
    let json = py
        .detach(|| analysis::analyze_source(&language, &bytes, None, opts))
        .map_err(analysis_error_to_py)?;
    conversion::json_string_to_py(py, &json)
}

/// Pull a `Vec<u8>` out of a Python value that may be `str`, `bytes`,
/// or `bytearray`.
///
/// `str` is encoded as UTF-8 — the same encoding tree-sitter expects
/// from the upstream library when it accepts a `&[u8]`.
fn extract_source_bytes(value: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(s) = value.cast::<PyString>() {
        return Ok(s.to_str()?.as_bytes().to_vec());
    }
    if let Ok(b) = value.cast::<PyBytes>() {
        return Ok(b.as_bytes().to_vec());
    }
    if let Ok(ba) = value.cast::<PyByteArray>() {
        // `PyByteArray::to_vec` copies under the GIL, side-stepping
        // the borrow-while-mutated hazard of `as_bytes()`.
        return Ok(ba.to_vec());
    }
    Err(PyValueError::new_err(
        "code must be str, bytes, or bytearray",
    ))
}

/// Return the language name resolved from `path` by the same
/// detection pipeline `analyze` uses — path extension first, then a
/// `#!`-shebang line or emacs `-*- mode: … -*-` declaration in the
/// file's leading window (`big_code_analysis::guess_language`).
///
/// Returns `None` only when neither the extension nor the file's
/// leading bytes match a known language. Raises `OSError`
/// (dispatched to `FileNotFoundError` / `PermissionError` / …) on
/// I/O failure — same taxonomy as `analyze`, since the underlying
/// `AnalysisError::Io` is routed through the shared
/// `analysis_error_to_py` mapper. The previous extension-only,
/// never-raising contract closed an asymmetry with `analyze` for
/// extension-less shebang scripts (#318) — at the cost of
/// promoting the function from pure path inspection to a real read.
#[pyfunction]
#[pyo3(signature = (path, /))]
#[allow(clippy::needless_pass_by_value)]
// `PathBuf` (not `&Path`) is required by PyO3's path conversion —
// see the comment on `analyze` above.
fn language_for_file(py: Python<'_>, path: PathBuf) -> PyResult<Option<&'static str>> {
    // Release the GIL across the file read so other Python threads
    // wrapping the call (the documented parallelism pattern around
    // `analyze`) actually make progress. `language::language_for_file`
    // touches no Python objects, so the detach is sound.
    py.detach(|| language::language_for_file(&path))
        .map_err(analysis_error_to_py)
}

/// Return the supported language names, in declaration order.
#[pyfunction]
fn supported_languages() -> Vec<&'static str> {
    language::supported_languages()
}

/// Return the file extensions registered for `language`.
///
/// Raises [`UnsupportedLanguageError`] when `language` is not a known
/// language name.
#[pyfunction]
#[pyo3(signature = (language, /))]
fn language_extensions(language: &str) -> PyResult<Vec<&'static str>> {
    language::language_extensions(language).ok_or_else(|| {
        UnsupportedLanguageError::new_err(language::unknown_language_message(language))
    })
}

/// File-type scope accepted by the VCS kwargs: either the CLI-shaped
/// comma-separated string (`"rs,py"`, `"metrics"`, `"all"`) or a native
/// Python sequence of extensions (`["rs", "py"]`) — issue #619.
///
/// A sequence is joined back into the comma-separated spelling the
/// upstream [`big_code_analysis::vcs::FileTypeScope`] parser already
/// accepts, so both forms converge on one validation path (and the same
/// `VcsError` on a list that normalises to nothing). `str` is matched
/// before the generic sequence extraction because a `str` is itself an
/// iterable of one-character strings — extracting it as a sequence would
/// silently treat `"rs,py"` as the extensions `['r', 's', ',', 'p', 'y']`.
struct FileTypes(String);

impl<'a, 'py> FromPyObject<'a, 'py> for FileTypes {
    type Error = PyErr;

    fn extract(value: Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        if let Ok(scope) = value.cast::<PyString>() {
            return Ok(Self(scope.to_str()?.to_owned()));
        }
        let extensions: Vec<String> = value.extract().map_err(|_| {
            PyValueError::new_err(
                "file_types must be a str (\"metrics\" / \"all\" / \"rs,py\") \
                 or a sequence of extension strings",
            )
        })?;
        Ok(Self(extensions.join(",")))
    }
}

impl FileTypes {
    /// The comma-separated scope string for [`vcs::VcsParams::file_types`].
    fn into_scope(self) -> String {
        self.0
    }
}

/// Resolve the `as_of` kwarg to the timestamp string the upstream
/// [`big_code_analysis::vcs::parse_timestamp`] accepts (issue #619).
///
/// Accepts either a `str` (the existing RFC 3339 / `@unix` / git-date
/// spelling) or a Python `datetime`, from which `.isoformat()` yields an
/// RFC 3339 string `parse_timestamp` parses. A naive `datetime` produces
/// an offset-less ISO string, interpreted as the backend's default zone —
/// the same behaviour as passing that bare string today.
fn extract_as_of(value: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(s) = value.cast::<PyString>() {
        return Ok(s.to_str()?.to_owned());
    }
    // `datetime` (and any object exposing `isoformat()`) is rendered to a
    // string here so the conversion stays at the Python boundary and the
    // crate need not link a PyO3 chrono/jiff feature.
    if let Ok(iso) = value.call_method0("isoformat") {
        return Ok(iso.cast::<PyString>()?.to_str()?.to_owned());
    }
    Err(PyValueError::new_err(
        "as_of must be a str (RFC 3339 / @unix / git date) or a datetime",
    ))
}

/// Rank the files in a git repository by change-history (VCS) risk
/// (issue #328).
///
/// `repo_path` is any path inside the working tree. Returns a dict with
/// the window lengths, version stamps, a `truncated_shallow_clone`
/// flag, a `vcs_aggregate` object carrying the directory- / repo-level
/// `bus_factor` (Avelino `DoA`, issue #332), and a `files` list ranked by
/// descending `risk_score` — the programmatic analogue of `bca vcs`.
/// `bus_factor_threshold` (default `0.5`) sets the coverage/abandonment
/// fraction. Raises `ValueError` for a malformed window / timestamp /
/// formula / bus-factor threshold, or when `repo_path` is not a git
/// working tree.
///
/// The history walk holds the GIL; for per-file AST + VCS in one pass
/// use `analyze(path, vcs=True)`.
#[pyfunction]
#[pyo3(signature = (repo_path, /, *, long_window = None, recent_window = None, top = None, reference = None, risk_formula = None, file_types = None, full_history = false, include_merges = false, follow_renames = true, exclude_bots = true, bot_pattern = None, as_of = None, emit_author_details = false, include_deleted = false, bus_factor_threshold = None, no_cache = false, cache_dir = None))]
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools
)]
fn vcs_metrics<'py>(
    py: Python<'py>,
    repo_path: PathBuf,
    long_window: Option<String>,
    recent_window: Option<String>,
    top: Option<usize>,
    reference: Option<String>,
    risk_formula: Option<String>,
    file_types: Option<FileTypes>,
    full_history: bool,
    include_merges: bool,
    follow_renames: bool,
    exclude_bots: bool,
    bot_pattern: Option<String>,
    as_of: Option<Bound<'py, PyAny>>,
    emit_author_details: bool,
    include_deleted: bool,
    bus_factor_threshold: Option<f64>,
    no_cache: bool,
    cache_dir: Option<PathBuf>,
) -> PyResult<Bound<'py, PyAny>> {
    let as_of = as_of.as_ref().map(extract_as_of).transpose()?;
    let params = vcs::VcsParams {
        long_window,
        recent_window,
        top,
        reference,
        risk_formula,
        file_types: file_types.map(FileTypes::into_scope),
        full_history,
        include_merges,
        follow_renames,
        exclude_bots,
        bot_pattern,
        as_of,
        emit_author_details,
        include_deleted,
        bus_factor_threshold,
        no_cache,
        cache_dir,
    };
    let json = vcs::vcs_report_json(&repo_path, &params)?;
    conversion::json_string_to_py(py, &json)
}

/// Sample the change-history metrics at several points in time and return
/// the per-file historical trend (issue #333).
///
/// `repo_path` is any path inside the working tree. `points` (>= 2) and
/// `span` (default `12mo`) define the evenly-spaced sampling grid, ending
/// at `as_of` (or wall-clock now). Returns a dict with `as_of_points`
/// (the sample timestamps, oldest-first), a `files` map from
/// repository-relative path to a point array aligned to `as_of_points`
/// (a `None` element marks a point where the file did not exist), and an
/// improving / regressing `deltas` summary — the programmatic analogue of
/// `bca vcs trend`. `top` caps how many files the series keeps (by
/// most-recent risk); `top_deltas` trims each delta list. Raises
/// `ValueError` for a malformed option, an out-of-range point count, or
/// when `repo_path` is not a git working tree.
///
/// Each point re-anchors at the mainline tip of that moment, so the result
/// is a faithful historical snapshot; the repeated walks hold the GIL.
#[pyfunction]
#[pyo3(signature = (repo_path, /, *, points = 12, span = None, top = None, top_deltas = None, long_window = None, recent_window = None, reference = None, risk_formula = None, file_types = None, full_history = false, include_merges = false, follow_renames = true, exclude_bots = true, bot_pattern = None, as_of = None, emit_author_details = false, include_deleted = false, bus_factor_threshold = None))]
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools
)]
fn vcs_trend<'py>(
    py: Python<'py>,
    repo_path: PathBuf,
    points: usize,
    span: Option<String>,
    top: Option<usize>,
    top_deltas: Option<usize>,
    long_window: Option<String>,
    recent_window: Option<String>,
    reference: Option<String>,
    risk_formula: Option<String>,
    file_types: Option<FileTypes>,
    full_history: bool,
    include_merges: bool,
    follow_renames: bool,
    exclude_bots: bool,
    bot_pattern: Option<String>,
    as_of: Option<Bound<'py, PyAny>>,
    emit_author_details: bool,
    include_deleted: bool,
    bus_factor_threshold: Option<f64>,
) -> PyResult<Bound<'py, PyAny>> {
    let as_of = as_of.as_ref().map(extract_as_of).transpose()?;
    let params = vcs::VcsParams {
        long_window,
        recent_window,
        top,
        reference,
        risk_formula,
        file_types: file_types.map(FileTypes::into_scope),
        full_history,
        include_merges,
        follow_renames,
        exclude_bots,
        bot_pattern,
        as_of,
        emit_author_details,
        include_deleted,
        bus_factor_threshold,
        // Trend resolves a fresh historical tip per point; the file-level
        // history cache does not apply, so it is left disabled.
        no_cache: true,
        cache_dir: None,
    };
    let json = vcs::vcs_trend_json(&repo_path, &params, points, span.as_deref(), top_deltas)?;
    conversion::json_string_to_py(py, &json)
}

/// Score a single commit for just-in-time (commit-level) defect-induction
/// risk (issue #331), or — when `diff` is supplied — an arbitrary unified
/// diff (issue #580).
///
/// `repo_path` is any path inside the working tree; `commit` is any git
/// revision spelling (default `HEAD`), scored against its first parent.
/// Returns a dict with the size / diffusion / history / experience features,
/// their per-group contributions, the ordinal composite `risk_score`, and the
/// `commit` block; the programmatic analogue of `bca vcs jit`.
///
/// Pass `diff` (a unified diff string) to score a bare diff instead. A
/// bare diff has no author / parent / history, so only the size and
/// diffusion groups are computable: the returned dict has `source ==
/// "diff"`, a `partial_risk_score` that is **not comparable** to a commit score,
/// and no history / experience / purpose groups (they are absent, not
/// zero). In diff mode `repo_path` / `commit` / the window knobs are
/// ignored. Raises `ValueError` for a malformed option, an unresolvable
/// commit, a malformed diff, or when `repo_path` is not a git working tree.
#[pyfunction]
#[pyo3(signature = (repo_path = None, /, *, commit = "HEAD".to_owned(), diff = None, long_window = None, recent_window = None, full_history = false, include_merges = false, follow_renames = true, as_of = None))]
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools
)]
fn vcs_jit<'py>(
    py: Python<'py>,
    repo_path: Option<PathBuf>,
    commit: String,
    diff: Option<String>,
    long_window: Option<String>,
    recent_window: Option<String>,
    full_history: bool,
    include_merges: bool,
    follow_renames: bool,
    as_of: Option<Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let as_of = as_of.as_ref().map(extract_as_of).transpose()?;
    let params = vcs::JitParams {
        long_window,
        recent_window,
        full_history,
        include_merges,
        follow_renames,
        as_of,
    };
    let json = vcs::vcs_jit_json(repo_path.as_deref(), &commit, diff.as_deref(), &params)?;
    conversion::json_string_to_py(py, &json)
}

/// `big_code_analysis._native` module entry point.
///
/// Re-exported by the pure-Python `big_code_analysis` package so
/// users see `big_code_analysis.analyze(...)`, not
/// `big_code_analysis._native.analyze(...)`. The exception classes
/// are placed both on `_native` (for `isinstance` / `except`) and on
/// the top-level package via the same re-export.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", PACKAGE_VERSION)?;
    // `METRIC_NAMES` is a `tuple[str, ...]` (immutable) rather than a
    // list because it advertises a constant — callers should not be
    // able to clear or extend it. Single source of truth lives on
    // the upstream crate as [`Metric::NAMES`]; the bindings re-export
    // it verbatim and the upstream test module pins both alphabetic
    // ordering and FromStr round-trip coverage.
    m.add("METRIC_NAMES", PyTuple::new(m.py(), Metric::NAMES)?)?;
    m.add(
        "UnsupportedLanguageError",
        m.py().get_type::<UnsupportedLanguageError>(),
    )?;
    m.add("ParseError", m.py().get_type::<ParseError>())?;
    m.add("VcsError", m.py().get_type::<VcsError>())?;
    m.add(
        "NotARepositoryError",
        m.py().get_type::<NotARepositoryError>(),
    )?;
    m.add(
        "InvalidRevisionError",
        m.py().get_type::<InvalidRevisionError>(),
    )?;
    m.add("InvalidDiffError", m.py().get_type::<InvalidDiffError>())?;
    m.add(
        "VcsEnvironmentError",
        m.py().get_type::<VcsEnvironmentError>(),
    )?;
    m.add_class::<PyAnalysisError>()?;
    m.add_function(wrap_pyfunction!(analyze, m)?)?;
    m.add_function(wrap_pyfunction!(vcs_metrics, m)?)?;
    m.add_function(wrap_pyfunction!(vcs_trend, m)?)?;
    m.add_function(wrap_pyfunction!(vcs_jit, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_source, m)?)?;
    m.add_function(wrap_pyfunction!(analyze_batch, m)?)?;
    m.add_function(wrap_pyfunction!(language_for_file, m)?)?;
    m.add_function(wrap_pyfunction!(supported_languages, m)?)?;
    m.add_function(wrap_pyfunction!(language_extensions, m)?)?;
    m.add_function(wrap_pyfunction!(to_sarif, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::strip_os_error_suffix;

    #[test]
    fn strips_trailing_os_error_suffix() {
        assert_eq!(
            strip_os_error_suffix("No such file or directory (os error 2)"),
            "No such file or directory"
        );
    }

    #[test]
    fn leaves_message_without_suffix_untouched() {
        assert_eq!(strip_os_error_suffix("custom failure"), "custom failure");
    }

    #[test]
    fn keeps_mid_message_parenthetical() {
        // Only a trailing `(os error N)` is the duplicated-errno
        // artifact; an interior parenthetical must survive verbatim.
        assert_eq!(
            strip_os_error_suffix("failed (transient) while reading"),
            "failed (transient) while reading"
        );
    }
}
