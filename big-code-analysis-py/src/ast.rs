//! The `Ast` parse-once handle exposed to Python (#727).
//!
//! Binds the Rust [`big_code_analysis::Ast`] seam so a Python caller can
//! parse a source file **once** and then draw both metrics and the AST from
//! the same parse, instead of paying for a second parse in py-tree-sitter.
//! Every accessor delegates to the upstream handle and serializes its result
//! through the same `serde_json::to_string` → `json.loads` path the
//! `analyze*` functions use, so the dict shapes are byte-for-byte identical
//! to the CLI / web surfaces (`bca dump`, `/ast`, `/metrics`).
//!
//! The handle is immutable (`frozen`) and holds a `Send + Sync`
//! `tree_sitter::Tree`, so it needs no `unsendable` and composes with
//! `ThreadPoolExecutor` fan-out exactly like `analyze`.

use std::path::Path;
use std::path::PathBuf;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes};

use big_code_analysis::{Ast, AstCfg, FromPathError, MetricsError, MetricsOptions, Source};

use crate::analysis::AnalysisError;
use crate::{
    UnsupportedLanguageError, analysis_error_to_py, conversion, extract_source_bytes, language,
    resolve_metric_set,
};

/// Map a [`MetricsError`] (from [`Ast::parse`] / [`Ast::metrics`] /
/// [`Ast::ops`]) to a Python exception by routing it through the same
/// `AnalysisError` taxonomy `analyze` uses (`LanguageDisabled` →
/// `UnsupportedLanguageError`, the reserved `EmptyRoot` → `ParseError`).
fn metrics_error_to_py(err: MetricsError) -> PyErr {
    analysis_error_to_py(AnalysisError::from(err))
}

/// Map a [`FromPathError`] (from [`Ast::from_path`]) to a Python exception.
///
/// I/O faults, non-UTF-8 paths, unknown languages, and disabled-language
/// builds reuse the shared `analyze` taxonomy so the exception types match
/// the rest of the bindings. `Unreadable` (empty / binary / non-UTF-8 file)
/// has no `analyze` analogue — `analyze` silently skips those — so it raises
/// a `ValueError` explaining that the no-magic `from_path` cannot parse it.
fn from_path_error_to_py(err: FromPathError, path: &Path) -> PyErr {
    match err {
        FromPathError::Io(source) => analysis_error_to_py(AnalysisError::Io {
            source,
            path: path.to_path_buf(),
        }),
        FromPathError::NonUtf8Path => analysis_error_to_py(AnalysisError::NonUtf8Path),
        FromPathError::UnknownLanguage => UnsupportedLanguageError::new_err(format!(
            "no language is registered for path {}",
            path.display()
        )),
        FromPathError::Parse(e) => metrics_error_to_py(e),
        FromPathError::Unreadable => PyValueError::new_err(format!(
            "{} is empty, binary, or not valid UTF-8 source text; from_path cannot \
             parse it (analyze() would skip such a file)",
            path.display()
        )),
        // `FromPathError` is `#[non_exhaustive]`; a future variant lands here
        // as a generic `ValueError` until this match learns a richer mapping.
        other => PyValueError::new_err(format!("could not parse {}: {other}", path.display())),
    }
}

/// Wrap a `serde_json` serialization failure as a `ValueError`. The bound
/// types all serialize infallibly in practice, but the crate forbids
/// `unwrap`/`expect` in non-test code, so the error is surfaced explicitly.
// Used directly as a `.map_err(serialize_error_to_py)` callback, which hands
// the error in by value; taking `&Error` would force a closure at every call
// site for no gain.
#[allow(clippy::needless_pass_by_value)]
fn serialize_error_to_py(err: serde_json::Error) -> PyErr {
    PyValueError::new_err(format!(
        "internal error: failed to serialise AST data to JSON: {err}"
    ))
}

/// A parsed source file: the AST plus its source bytes, served from a single
/// parse.
///
/// Construct one with [`Ast.parse`](PyAst::parse) (from in-memory source) or
/// [`Ast.from_path`](PyAst::from_path) (from a file). Then draw metrics
/// ([`metrics`](PyAst::metrics)), the node tree ([`dump`](PyAst::dump)),
/// function spans ([`functions`](PyAst::functions)), the Halstead
/// operator/operand tree ([`ops`](PyAst::ops)), suppression markers, a node
/// count, or the comment-stripped source — all without re-parsing.
#[pyclass(name = "Ast", module = "big_code_analysis._native", frozen)]
pub(crate) struct PyAst {
    inner: Ast,
}

impl PyAst {
    /// Borrow the held [`Ast`] for crate-internal use (the lazy
    /// [`PyNode`](crate::node::PyNode) reads `source()` for
    /// [`text`](crate::node::PyNode::text) and re-roots traversal here).
    pub(crate) fn ast_ref(&self) -> &Ast {
        &self.inner
    }
}

#[pymethods]
impl PyAst {
    /// Parse in-memory `code` (`str` / `bytes` / `bytearray`) in `language`
    /// (a name from `supported_languages()`, case-insensitive).
    ///
    /// `name` is an optional logical file name recorded on the resulting
    /// top-level space (the `FuncSpace.name` field), matching the CLI's
    /// per-file labelling; it does not need to be a real path.
    ///
    /// Raises `UnsupportedLanguageError` when `language` is not recognised or
    /// its grammar is not compiled into this build.
    #[staticmethod]
    #[pyo3(signature = (code, language, /, *, name = None))]
    fn parse(
        py: Python<'_>,
        code: &Bound<'_, PyAny>,
        language: &str,
        name: Option<String>,
    ) -> PyResult<Self> {
        let bytes = extract_source_bytes(code)?;
        let lang = language::parse_language_name(language).ok_or_else(|| {
            UnsupportedLanguageError::new_err(language::unknown_language_message(language))
        })?;
        // Release the GIL across the parse (the heavy, Python-object-free
        // step), exactly like `analyze_source`. The owned `bytes` / `name`
        // move into the closure so it stays `Send`.
        let inner = py
            .detach(move || Ast::parse(Source::from_bytes(lang, bytes).with_name(name)))
            .map_err(metrics_error_to_py)?;
        Ok(Self { inner })
    }

    /// Read, language-detect, and parse `path` in one call — the file-backed
    /// counterpart to [`parse`](PyAst::parse).
    ///
    /// Reads through the same text reader `analyze` uses (so EOL
    /// normalization and metric values match `analyze` for the same file),
    /// but is *no-magic*: it does not skip generated files and does not run
    /// the C/C++ preprocessor. Unlike `analyze`, it never silently returns
    /// nothing — every failure raises:
    ///
    /// * `OSError` (and subclasses) — the file could not be read;
    /// * `UnsupportedLanguageError` — no language registered for the path;
    /// * `ValueError` — a non-UTF-8 path, or an empty / binary / non-UTF-8
    ///   file that cannot be parsed as text.
    #[staticmethod]
    #[pyo3(signature = (path, /))]
    // `path: PathBuf` (not `&Path`) is mandated by PyO3: `FromPyObject`
    // materializes a fresh `PathBuf` from the `os.PathLike` argument, with no
    // borrow to extract a `&Path` from — mirroring the top-level `analyze`.
    #[allow(clippy::needless_pass_by_value)]
    fn from_path(py: Python<'_>, path: PathBuf) -> PyResult<Self> {
        let inner = py
            .detach(|| Ast::from_path(&path))
            .map_err(|err| from_path_error_to_py(err, &path))?;
        Ok(Self { inner })
    }

    /// The canonical lowercase language slug that parsed this source (e.g.
    /// `"rust"`, `"cpp"`).
    #[getter]
    fn language(&self) -> &'static str {
        self.inner.language().name()
    }

    /// The parsed source bytes, after EOL normalization. Byte offsets in
    /// `dump()` spans index into exactly these bytes.
    #[getter]
    fn source<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, self.inner.source())
    }

    /// Compute metrics from the held parse, returning the same dict shape as
    /// `analyze` / `analyze_source` (a `FuncSpaceDict`).
    ///
    /// `exclude_tests` mirrors the CLI `--exclude-tests` flag; `metrics`
    /// selects which metrics to compute (the full suite when omitted). Two
    /// calls with different selections reuse the one parse.
    #[pyo3(signature = (*, exclude_tests = false, metrics = None))]
    fn metrics<'py>(
        &self,
        py: Python<'py>,
        exclude_tests: bool,
        metrics: Option<Vec<String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let metric_set = resolve_metric_set(metrics)?;
        let options = MetricsOptions::default()
            .with_exclude_tests(exclude_tests)
            .with_metric_set(metric_set);
        let space = py
            .detach(move || self.inner.metrics(options))
            .map_err(metrics_error_to_py)?;
        let json = serde_json::to_string(&space).map_err(serialize_error_to_py)?;
        conversion::json_string_to_py(py, &json)
    }

    /// Return the AST node tree as nested dicts (an `AstNodeDict`): the
    /// `root` of the same tree `bca dump` and the web `/ast` endpoint emit,
    /// so node shapes are byte-for-byte identical.
    ///
    /// With `span=True` (the default) every node carries a span object
    /// `{start_line, start_col, end_line, end_col, start_byte, end_byte}`;
    /// the byte offsets index into `source`. `comment` follows the CLI /
    /// `/ast` convention: with `comment=False` (the default) comment nodes
    /// are kept, and `comment=True` omits them from the tree. Returns `None`
    /// only if the parse produced no root.
    #[pyo3(signature = (*, span = true, comment = false))]
    fn dump<'py>(
        &self,
        py: Python<'py>,
        span: bool,
        comment: bool,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let cfg = AstCfg {
            // The Python handle returns the bare node tree; the `id` /
            // `language` envelope fields of the `/ast` response are dropped
            // (`language` is already a property here).
            id: String::new(),
            language: self.inner.language().name().to_owned(),
            comment,
            span,
        };
        let response = py.detach(move || self.inner.dump(cfg));
        match response.root {
            None => Ok(None),
            Some(root) => {
                let json = serde_json::to_string(&root).map_err(serialize_error_to_py)?;
                conversion::json_string_to_py(py, &json).map(Some)
            }
        }
    }

    /// Return the function spans in the file (a list of `FunctionSpanDict`):
    /// each function's name and 1-based line range, mirroring the web
    /// `/function` endpoint.
    fn functions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let spans = py.detach(|| self.inner.functions());
        let wire: Vec<_> = spans
            .iter()
            .map(big_code_analysis::FunctionSpan::to_wire)
            .collect();
        let json = serde_json::to_string(&wire).map_err(serialize_error_to_py)?;
        conversion::json_string_to_py(py, &json)
    }

    /// Return the Halstead operator/operand tree for the file (an `OpsDict`),
    /// the deduplicated operators (`n1`) and operands (`n2`) per space.
    fn ops<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let ops = py
            .detach(|| self.inner.ops())
            .map_err(metrics_error_to_py)?;
        let json = serde_json::to_string(&ops.to_wire()).map_err(serialize_error_to_py)?;
        conversion::json_string_to_py(py, &json)
    }

    /// Count nodes matching `filters` (tree-sitter node kinds), returning
    /// `(matching, total)` — the same pair `bca count` reports.
    #[pyo3(signature = (filters, /))]
    fn count(&self, py: Python<'_>, filters: Vec<String>) -> (usize, usize) {
        py.detach(move || self.inner.count(&filters))
    }

    /// Return the source with comment nodes removed, or `None` when the
    /// grammar defines no comment nodes to strip.
    fn strip_comments<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        py.detach(|| self.inner.strip_comments())
            .map(|bytes| PyBytes::new(py, &bytes))
    }

    /// Return every in-source suppression marker (`// bca: suppress(...)` and
    /// the file-scoped form) with its location, scope, dialect, and
    /// enclosing function — the data behind the `bca exemptions` audit.
    fn suppressions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let markers = py.detach(|| self.inner.suppressions());
        let json = serde_json::to_string(&markers).map_err(serialize_error_to_py)?;
        conversion::json_string_to_py(py, &json)
    }

    /// Return the root [`Node`](crate::node::PyNode) of the held parse for
    /// lazy, py-tree-sitter-style traversal — `kind`, byte offsets, points,
    /// `children`, `walk()`, … — without materialising the tree into dicts
    /// the way [`dump`](PyAst::dump) does (#728).
    ///
    /// Node kinds are the **raw** grammar kinds, not the `Alterator`-curated
    /// kinds `dump()` emits, so they intentionally disagree on altered
    /// nodes; the returned node and all its descendants keep this `Ast`
    /// alive.
    #[getter]
    fn root_node(slf: &Bound<'_, Self>) -> crate::node::PyNode {
        let root = slf.get().inner.root_node();
        crate::node::PyNode::wrap(slf.clone().unbind(), root.as_tree_sitter())
    }

    /// Return every node in the held parse whose kind matches one of
    /// `filters`, as lazy [`Node`](crate::node::PyNode) handles (#728).
    ///
    /// `filters` accepts the same vocabulary as [`count`](PyAst::count) —
    /// `all`, `call`, `comment`, `error`, `string`, `function`, a numeric
    /// `kind_id`, or an exact `node.kind()` — so a structural extractor can
    /// jump straight to the nodes it cares about instead of walking the
    /// whole tree from Python.
    #[pyo3(signature = (filters, /))]
    // PyO3 `FromPyObject` materializes an owned `Vec<String>` from the Python
    // sequence; there is no borrowed `&[String]` to take instead (mirrors
    // `count` / `from_path`).
    #[allow(clippy::needless_pass_by_value)]
    fn find(slf: &Bound<'_, Self>, filters: Vec<String>) -> PyResult<Vec<crate::node::PyNode>> {
        let nodes = slf
            .get()
            .inner
            .find(&filters)
            .map_err(metrics_error_to_py)?;
        Ok(nodes
            .into_iter()
            .map(|node| crate::node::PyNode::wrap(slf.clone().unbind(), node.as_tree_sitter()))
            .collect())
    }

    fn __repr__(&self) -> String {
        format!(
            "Ast(language='{}', source_len={})",
            self.inner.language().name(),
            self.inner.source().len()
        )
    }
}

/// Return the pinned tree-sitter grammar crate version backing `language`
/// (e.g. `"0.25.1"` for `"bash"`).
///
/// For languages backed by an upstream crates.io grammar this is the exact
/// upstream version, so a consumer migrating matchers off py-tree-sitter can
/// line node-kind vocabularies up against the same pin. For the vendored
/// big-code-analysis forks (`mozcpp`, `mozjs`, `tcl`, `kotlin`) it is the
/// fork crate's version, not an upstream grammar semver.
///
/// Raises `UnsupportedLanguageError` when `language` is not a recognised
/// language name.
#[pyfunction]
#[pyo3(signature = (language, /))]
pub(crate) fn language_grammar_version(language: &str) -> PyResult<&'static str> {
    language::parse_language_name(language)
        .map(|lang| lang.grammar_version())
        .ok_or_else(|| {
            UnsupportedLanguageError::new_err(language::unknown_language_message(language))
        })
}
