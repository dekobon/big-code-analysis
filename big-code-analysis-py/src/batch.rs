// bca: suppress-file(halstead, nargs, nexits, nom)
// PyO3 batch analysis + error type; nexits is never-raise error-folding /
// FFI-repr boilerplate and the rest are many-fn aggregation artifacts, not
// per-function logic complexity (cognitive/cyclomatic stay enforced).

//! Batch entry point and the structured `AnalysisFailure` Python class.
//!
//! Where [`crate::analysis`] raises a Python exception per failing
//! file, this module's [`analyze_batch`] sweeps an iterable of paths
//! and folds per-file failures into [`PyAnalysisError`] values
//! (exposed to Python as `AnalysisFailure`) interleaved with
//! successful result dicts. The Rust type keeps the historical
//! `PyAnalysisError` spelling; only the Python-visible class name
//! changed to `AnalysisFailure` at 2.0 (#614) — it is a returned
//! value, never raised, so the `…Error` suffix that PEP 8 reserves
//! for exceptions misled readers. The contract is
//! *never-raise on per-file errors* so pipeline / workflow callers
//! can keep going past a missing file, an unknown extension, or a
//! parser failure without a `try` / `except` per path. Programmer
//! errors (non-iterable `paths`, empty `metrics=` list) still raise,
//! because they indicate a broken call site, not a recoverable input.
//!
//! The error taxonomy mirrors the design in issue #103:
//!
//! * `UnsupportedLanguage` — extension lookup and shebang inference
//!   both came up empty, or the upstream `MetricsError::LanguageDisabled`
//!   bubbled up (grammar not in this build).
//! * `ParseError` — the upstream reserved-but-not-yet-emitted
//!   `MetricsError::EmptyRoot` maps here. Today this arm is
//!   unreachable from Python for syntactically broken sources because
//!   tree-sitter is permissive (it produces an error tree, not an
//!   `Err`), but the mapping is in place for the day strict parsing
//!   lands upstream.
//! * `IoError` — `std::fs::read` failed (the most common kind), or
//!   the path is not valid UTF-8 (treated as a path-resolution
//!   failure since we cannot honour it without violating the
//!   identifier-path rule from AGENTS.md).

use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use pyo3::Bound;
use pyo3::Py;
use pyo3::PyAny;
use pyo3::PyErr;
use pyo3::PyResult;
use pyo3::Python;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyTuple, PyType};

use big_code_analysis::vcs::{HistoryIndex, PerFunctionBlame};

use crate::analysis::{self, AnalysisError, AnalyzeOptions};
use crate::conversion;
use crate::vcs as vcs_bridge;
use crate::walk::{WalkFilters, walk_paths};

/// Closed taxonomy for [`PyAnalysisError::error_kind`].
///
/// Kept as a private enum so the three permitted values live in one
/// place — every internal construction site goes through the
/// exhaustive `match` in [`PyAnalysisError::from_internal`] (a new
/// upstream `AnalysisError` variant fails the match at compile
/// time), and the public `py_new` constructor parses Python strings
/// through [`FromStr`] so out-of-taxonomy values raise `ValueError`
/// at the FFI boundary. The Python-facing `error_kind` attribute
/// still surfaces as a `str` via the [`AsRef<str>`] / `as_str`
/// projection — the enum is an internal validation device, not an
/// API surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ErrorKind {
    UnsupportedLanguage,
    ParseError,
    IoError,
}

impl ErrorKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedLanguage => "UnsupportedLanguage",
            Self::ParseError => "ParseError",
            Self::IoError => "IoError",
        }
    }
}

impl FromStr for ErrorKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "UnsupportedLanguage" => Ok(Self::UnsupportedLanguage),
            "ParseError" => Ok(Self::ParseError),
            "IoError" => Ok(Self::IoError),
            _ => Err(()),
        }
    }
}

/// Structured per-file failure surfaced by [`analyze_batch`].
///
/// `path` is the caller-supplied path, rendered via `to_str` when the
/// `PathBuf` is valid UTF-8 (the common case). Non-UTF-8 paths are
/// rendered byte-by-byte via Rust's `Debug` formatting so distinct
/// byte sequences map to distinct strings — `to_string_lossy` would
/// collapse them onto a single U+FFFD-substituted rendering and
/// silently break the documented `set(results)` dedup contract, which
/// keys on `(path, error, error_kind)`. The Debug rendering for
/// non-UTF-8 paths surrounds the value with double quotes (e.g.
/// `"/a\xff"`) — a visible cue that the path was not valid UTF-8 and
/// a deliberate signal to callers not to feed `err.path` back into
/// the filesystem as-is. The `FuncSpace.name` identifier in
/// successful results is still strict UTF-8, so the project rule
/// against lossy identifiers (AGENTS.md) is not relaxed.
///
/// `error_kind` surfaces as a `String` carrying one of the three
/// [`ErrorKind`] variants (`"UnsupportedLanguage"`, `"ParseError"`,
/// `"IoError"`). The class itself is **not** an exception subclass —
/// instances appear in the return list of [`analyze_batch`], they
/// are never raised — so `isinstance(r, AnalysisFailure)` is the
/// canonical discriminator. (`AnalysisFailure` is the Python-visible
/// name; this Rust type is `PyAnalysisError` internally.)
#[pyclass(
    frozen,
    name = "AnalysisFailure",
    module = "big_code_analysis._native",
    eq,
    hash,
    // Skip the auto `FromPyObject` derive that PyO3 generates for
    // `Clone` `#[pyclass]` types. `PyAnalysisError` only flows
    // *out* of the FFI boundary (it is built in Rust and returned
    // to Python); never accepting it as a `FromPyObject` argument
    // future-proofs the contract — and silences the 0.28
    // deprecation warning about the implicit derive.
    skip_from_py_object
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct PyAnalysisError {
    #[pyo3(get)]
    path: String,
    #[pyo3(get)]
    error: String,
    #[pyo3(get)]
    error_kind: String,
}

#[pymethods]
impl PyAnalysisError {
    /// Build an `AnalysisFailure` directly. Useful for tests and for
    /// callers that want to deduplicate batch errors into a `set`
    /// — equality / hashing covers `(path, error, error_kind)` (all
    /// three fields), so two failures of the same kind on the same
    /// path but with differing `error` messages remain distinct.
    /// Bucket on `(r.path, r.error_kind)` explicitly if message
    /// drift is undesirable for the dedup key.
    #[new]
    #[pyo3(signature = (path, error, error_kind))]
    fn py_new(path: String, error: String, error_kind: &str) -> PyResult<Self> {
        let kind = error_kind.parse::<ErrorKind>().map_err(|()| {
            PyValueError::new_err(format!(
                "error_kind must be one of [UnsupportedLanguage, ParseError, IoError], \
                 got {error_kind:?}",
            ))
        })?;
        Ok(Self::new_internal(path, error, kind))
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        // Defer each field's escape to Python's `repr()` built-in
        // instead of Rust's `{:?}` formatter. Rust's `Debug` escapes
        // non-printable codepoints (e.g. `\x01`) and non-BMP bytes
        // via `\u{N}` brace syntax, which Python's parser does NOT
        // accept — so the docstring claim that `eval(repr(x))`
        // round-trips would otherwise break for any path containing
        // a control character. Python's `repr()` emits `\xNN` /
        // `\uNNNN` / `\UNNNNNNNN` escapes that the same parser
        // accepts, restoring the round-trip property for arbitrary
        // string content.
        let builtins = py.import("builtins")?;
        let repr_fn = builtins.getattr("repr")?;
        let path_r: String = repr_fn.call1((&self.path,))?.extract()?;
        let error_r: String = repr_fn.call1((&self.error,))?.extract()?;
        let kind_r: String = repr_fn.call1((&self.error_kind,))?.extract()?;
        Ok(format!(
            "AnalysisFailure(path={path_r}, error={error_r}, error_kind={kind_r})"
        ))
    }

    /// Pickle hook so callers can ship `AnalysisError` values back
    /// from `multiprocessing.Pool` / `concurrent.futures.ProcessPoolExecutor`
    /// workers. The 2-tuple shape is the pickle convention
    /// `(callable, args)`: the class itself is the callable, and
    /// `(path, error, error_kind)` is the constructor arg tuple —
    /// which round-trips through [`py_new`]'s `ErrorKind::from_str`
    /// validation, so an out-of-taxonomy `error_kind` cannot sneak
    /// in via a tampered pickle stream.
    fn __reduce__<'py>(&self, py: Python<'py>) -> (Bound<'py, PyType>, (String, String, String)) {
        (
            py.get_type::<Self>(),
            (
                self.path.clone(),
                self.error.clone(),
                self.error_kind.clone(),
            ),
        )
    }
}

impl PyAnalysisError {
    /// Build a [`PyAnalysisError`] from a bridge-layer
    /// [`AnalysisError`] plus the path that produced it.
    ///
    /// The `path` field always comes from the caller-supplied input —
    /// even when the underlying error already captured a path of its
    /// own (the `Io` arm does) — so callers can reliably zip the
    /// result list back against their input list. Non-UTF-8 paths
    /// flow through [`encode_path_for_field`], which uses Rust's
    /// `Debug` formatting (byte-preserving) instead of `to_string_lossy`
    /// so distinct byte sequences map to distinct strings — the
    /// documented `set(results)` dedup contract depends on this
    /// because `path` participates in `__eq__` / `__hash__`.
    pub(crate) fn from_internal(err: AnalysisError, path: &Path) -> Self {
        let path_str = encode_path_for_field(path);
        let (error, kind) = match err {
            AnalysisError::Io { source, .. } => (source.to_string(), ErrorKind::IoError),
            AnalysisError::NonUtf8Path => (
                "path is not valid UTF-8 and cannot be encoded as a FuncSpace \
                 name; analyze_batch surfaces this under error_kind='IoError' \
                 to keep the public taxonomy at three kinds — filter the \
                 batch input upstream if you need to distinguish the two"
                    .to_owned(),
                ErrorKind::IoError,
            ),
            AnalysisError::UnsupportedLanguage(msg) => (msg, ErrorKind::UnsupportedLanguage),
            AnalysisError::Parse(e) => (e.to_string(), ErrorKind::ParseError),
            // `Serialization` is reachable only if a future upstream
            // `FuncSpace::Serialize` impl introduces a fallible path
            // — see `crate::analysis::AnalysisError::Serialization`.
            // Lump it into `ParseError` so the public taxonomy stays
            // at three kinds; the message preserves the JSON detail.
            AnalysisError::Serialization(e) => (
                // Mirror the prefix shape used by the synthetic
                // error sites in `analyze_batch` (`"internal:
                // <subkind>: <detail>"`) so a retry classifier or
                // log filter only has to learn one form to spot
                // an internal-bridge failure.
                format!("internal: serialization error: {e}"),
                ErrorKind::ParseError,
            ),
        };
        Self::new_internal(path_str, error, kind)
    }

    /// Internal constructor that owns the conversion from the typed
    /// [`ErrorKind`] enum to the Python-facing `String` field.
    ///
    /// Every non-test Rust-side construction site flows through
    /// here, so the exhaustive `match` in [`Self::from_internal`]
    /// is the single place a future `AnalysisError` variant must be
    /// mapped — the compiler enforces the taxonomy. `py_new`
    /// similarly parses Python strings through
    /// [`ErrorKind::from_str`] before arriving here, so the
    /// `error_kind` field is provably one of the three documented
    /// values without runtime validation. (The `equal_errors_hash_equal`
    /// unit test below does build a `PyAnalysisError` via struct
    /// literal because that test exercises the `Hash` / `Eq`
    /// derives directly — production paths never do.)
    fn new_internal(path: String, error: String, kind: ErrorKind) -> Self {
        Self {
            path,
            error,
            error_kind: kind.as_str().to_owned(),
        }
    }

    /// Build a synthetic [`PyAnalysisError`] from a caller path plus
    /// a free-form message and kind. Used by [`analyze_batch`] to
    /// surface a `json_string_to_py` failure on the success arm as a
    /// per-file [`PyAnalysisError`] value rather than propagating it
    /// as a Python exception — the never-raise-on-per-file-errors
    /// contract requires every input position that is not skipped to
    /// yield either a `dict` or an `AnalysisError`.
    fn synthetic_internal(path: &Path, message: String, kind: ErrorKind) -> Self {
        Self::new_internal(encode_path_for_field(path), message, kind)
    }
}

/// Encode a path for the diagnostic `path` field on [`PyAnalysisError`].
///
/// Valid UTF-8 paths pass through unchanged (the common case). Non-UTF-8
/// paths are rendered via Rust's `Debug` formatting — which escapes
/// invalid bytes via `\xNN` hex escapes — so two distinct non-UTF-8
/// byte sequences produce two distinct strings. `to_string_lossy` would
/// collapse them onto a single U+FFFD-substituted rendering and break
/// the documented `set(results)` dedup contract: `path` is one of the
/// three fields that participate in `__eq__` / `__hash__`. The Debug
/// fallback surrounds the rendering with double quotes — a visible cue
/// that the path was not valid UTF-8 and a deliberate signal to callers
/// not to feed `err.path` back into the filesystem as-is.
fn encode_path_for_field(path: &Path) -> String {
    // `clippy::unnecessary_debug_formatting` suggests `path.display()`
    // instead of `{:?}`, but that is precisely the bug this helper
    // exists to avoid: `Display` (and `to_string_lossy`) substitute
    // U+FFFD for invalid bytes, collapsing distinct non-UTF-8 paths
    // onto the same string. The `Debug` impl on `Path` / `OsStr` is
    // byte-preserving (`\xNN` hex escapes for invalid bytes), which
    // is what the `set(results)` dedup contract requires.
    #[allow(clippy::unnecessary_debug_formatting)]
    path.to_str()
        .map_or_else(|| format!("{path:?}"), str::to_owned)
}

// Compile-time assertion that `PyAnalysisError` continues to satisfy
// the `Hash + Eq` bounds the `#[pyclass(eq, hash)]` derives wire to
// Python's `__eq__` / `__hash__`. Using a `const _:` evaluator (not
// a free `fn`) so a future contributor reading the file does not
// mistake it for dead code — the body type-checks at compile time
// without participating in the runtime call graph.
const _: fn() = || {
    fn assert_hash_eq<T: Hash + Eq>() {}
    assert_hash_eq::<PyAnalysisError>();
};

/// Run [`crate::analysis::analyze_path`] against every path in
/// `paths` and fold per-file errors into [`PyAnalysisError`] values.
///
/// `paths` is any Python iterable of `str | os.PathLike[str]`;
/// generators work because iteration is done lazily via `PyO3`'s
/// `try_iter` (which calls Python's `iter()` builtin under the hood).
/// With `skip_generated=false` the output list has the same length as
/// the input iterable and preserves order one-to-one, so callers can
/// `zip(inputs, results)` without losing the pairing. Under the default
/// `skip_generated=true` a skipped file yields no slot (see below), so
/// the list can be shorter — `zip(inputs, results)` would then silently
/// mis-pair every entry after the first skip.
///
/// `metrics=` selects which metrics to compute (#268). `None` (the
/// default) preserves the full suite; an empty list raises
/// `ValueError("provide at least one metric, or omit the argument")`
/// and an unknown name raises `ValueError("unknown metric: <bad>;
/// valid: …")` listing every entry in `bca.METRIC_NAMES`. The
/// validation runs **before** `paths.try_iter()`, so a bad selection
/// aborts without inspecting any input path — a generator's stateful
/// `__iter__` is never invoked. Metrics outside the selected set are
/// absent (not `None`) from the resulting dicts; selecting a derived
/// metric (`"mi"`, `"wmc"`) implicitly pulls in its dependencies.
///
/// `exclude_tests`, `allow_lossy_path`, and `skip_generated` mirror
/// the keyword-only kwargs on [`crate::analyze`] verbatim (#542), so
/// migrating a comprehension from `analyze` to `analyze_batch` is
/// behaviour-preserving. In particular `skip_generated` defaults to
/// `true` here too: a generated file is *skipped* (its input position
/// yields no `dict`), so the result list can be **shorter** than the
/// input iterable when `skip_generated=true`. Pass
/// `skip_generated=false` to restore the legacy "one result per input,
/// always" behaviour (every position produces a `dict` or an
/// `AnalysisError`).
#[pyfunction]
#[pyo3(signature = (paths, /, *, exclude_tests = false, allow_lossy_path = false, skip_generated = true, metrics = None, vcs = false, vcs_per_function = false))]
// `metrics: Option<Vec<String>>` is taken by value to match the PyO3
// keyword-argument FFI shape (the macro materialises an owned `Vec`
// out of the Python list); clippy's `needless_pass_by_value` lint
// can't see across the macro boundary, so silence it here.
#[allow(
    clippy::needless_pass_by_value,
    clippy::fn_params_excessive_bools,
    // Six keyword-only kwargs mirroring `analyze`'s documented surface;
    // a params struct would only obscure the FFI signature CPython binds.
    clippy::too_many_arguments
)]
pub(crate) fn analyze_batch<'py>(
    py: Python<'py>,
    paths: &Bound<'py, PyAny>,
    exclude_tests: bool,
    allow_lossy_path: bool,
    skip_generated: bool,
    metrics: Option<Vec<String>>,
    vcs: bool,
    vcs_per_function: bool,
) -> PyResult<Vec<Py<PyAny>>> {
    // Resolve `metrics=` *before* `paths.try_iter()` so a bad name
    // (empty list, unknown metric) aborts before any iteration side
    // effects — generators with stateful `__iter__` would otherwise
    // observe a half-step. Issue #268 pins this ordering.
    let metric_set = crate::resolve_metric_set(metrics)?;

    let iter = paths.try_iter()?;
    let opts = AnalyzeOptions {
        exclude_tests,
        allow_lossy_path,
        skip_generated,
        metrics: metric_set,
    };

    // Use `__len__` (Python's `len()` builtin, which is what PyO3's
    // `Bound::len()` calls) to preallocate when the caller passes a
    // list, tuple, or any other sized container. Iterators and
    // generators have no `__len__` and fall through to `cap = 0`;
    // for them `Vec::with_capacity(0)` is identical to `Vec::new()`.
    // Only `TypeError` ("object has no len") is silently swallowed —
    // any other exception from a custom `__len__` (e.g. a lazy proxy
    // raising `RuntimeError` to signal a transient backing-store
    // failure) surfaces as a Python error rather than being lost.
    let cap = match paths.len() {
        Ok(n) => n,
        Err(e) if e.is_instance_of::<PyTypeError>(py) => 0,
        Err(e) => return Err(e),
    };
    let mut results: Vec<Py<PyAny>> = Vec::with_capacity(cap);
    // One shared VCS index / blame engine per containing repository (#670):
    // a comprehension over `analyze(p, vcs=True)` walks the same history
    // once per file, whereas batch amortises the walk across every file in
    // the repo. Keyed by the repo root each file resolves to.
    let mut vcs_repos = VcsRepoCache::new(vcs, vcs_per_function);
    for item in iter {
        let item = item?;
        let path: PathBuf = item.extract()?;
        push_one_result(py, &path, opts, &mut vcs_repos, &mut results)?;
    }
    Ok(results)
}

/// Analyse `path` and push its result (a dict or an `AnalysisFailure`)
/// onto `results`, attaching shared-index VCS blocks when requested.
/// Shared by [`analyze_batch`] and [`analyze_paths`].
fn push_one_result(
    py: Python<'_>,
    path: &Path,
    opts: AnalyzeOptions,
    vcs_repos: &mut VcsRepoCache,
    results: &mut Vec<Py<PyAny>>,
) -> PyResult<()> {
    // Release the GIL across the file read and tree-sitter parse so
    // other Python threads can run during the sequential sweep.
    // `analyze_path` touches no Python objects, so `py.detach` is sound;
    // the GIL is re-acquired before `json_string_to_py` builds the dict.
    let outcome = py.detach(|| analysis::analyze_path(path, opts));
    match outcome {
        Ok(Some(json)) => {
            // Attach VCS blocks (file-level and/or per-function) using the
            // shared per-repo index / blame engine. A VCS failure leaves
            // the AST metrics intact (graceful degradation) and never turns
            // the result into an `AnalysisFailure` (#670).
            let json = vcs_repos.attach(py, json, path)?;
            match conversion::json_string_to_py(py, &json) {
                Ok(dict) => results.push(dict.unbind()),
                Err(err) => {
                    // Fold internal JSON-to-Python conversion failures into
                    // the per-file error stream so the never-raise contract
                    // holds even on the success arm.
                    let py_err = PyAnalysisError::synthetic_internal(
                        path,
                        format!("internal: JSON-to-Python conversion failed: {err}"),
                        ErrorKind::ParseError,
                    );
                    results.push(Py::new(py, py_err)?.into_any());
                }
            }
        }
        // `Ok(None)` means `analyze_path` skipped the file — with the #542
        // default `skip_generated=true` this is the generated-file case,
        // omitted from the output entirely (matching `analyze`).
        Ok(None) => {}
        Err(err) => {
            let py_err = PyAnalysisError::from_internal(err, path);
            results.push(Py::new(py, py_err)?.into_any());
        }
    }
    Ok(())
}

/// Per-repository VCS index / blame cache for the batch entry points
/// (#670).
///
/// `analyze(p, vcs=True)` walks a repository's history once **per file**;
/// the batch path instead builds one [`HistoryIndex`] (and/or one
/// [`PerFunctionBlame`] engine) per **containing repository** and reuses it
/// across every file in that repo — the amortisation win the CLI walker
/// already gets. The cache is keyed by the discovered work-tree root
/// (`vcs::workdir_root`), so two files in different subdirectories of one
/// checkout (`src/a.rs` and `tests/b.rs`) share a single index rather than
/// building it once per directory. A file outside any repository falls back
/// to its parent directory as the key; a repo that fails to open is cached
/// as absent (`None`) so the batch degrades gracefully per file without
/// re-attempting the failed open.
struct VcsRepoCache {
    vcs: bool,
    vcs_per_function: bool,
    // Memoised work-tree-root discovery, keyed by the file's parent
    // directory: every file in one directory shares a discovery result, so
    // we run `gix::discover` at most once per directory rather than per
    // file. The value is the resolved repository root (or the parent dir
    // itself when not in a repo).
    roots: HashMap<PathBuf, PathBuf>,
    // Keyed by the resolved repository work-tree root. `None` means
    // "discovery/open already failed here" — cached so a whole out-of-repo
    // tree is probed at most once.
    indexes: HashMap<PathBuf, Option<HistoryIndex>>,
    blames: HashMap<PathBuf, Option<PerFunctionBlame>>,
}

impl VcsRepoCache {
    fn new(vcs: bool, vcs_per_function: bool) -> Self {
        Self {
            vcs,
            vcs_per_function,
            roots: HashMap::new(),
            indexes: HashMap::new(),
            blames: HashMap::new(),
        }
    }

    /// Resolve the repository work-tree root for `path`, memoising the
    /// (relatively expensive) `gix::discover` per containing directory. The
    /// discovery root is the file's parent directory, so every file in one
    /// directory shares a single discovery. Falls back to that parent
    /// directory when `path` is not inside a repository, preserving the
    /// graceful per-file degradation.
    fn resolve_root(&mut self, py: Python<'_>, path: &Path) -> PathBuf {
        let parent = vcs_bridge::repo_root_for(path).to_path_buf();
        if let Some(root) = self.roots.get(&parent) {
            return root.clone();
        }
        let resolved = py
            .detach(|| vcs_bridge::workdir_root_for(path))
            .unwrap_or_else(|| parent.clone());
        self.roots.insert(parent, resolved.clone());
        resolved
    }

    /// Attach the requested VCS blocks to `json` for `path`, building (and
    /// caching) the per-repo index / blame engine on first use. Returns the
    /// rewritten JSON; a VCS failure leaves the AST metrics intact.
    fn attach(&mut self, py: Python<'_>, json: String, path: &Path) -> PyResult<String> {
        if !self.vcs && !self.vcs_per_function {
            return Ok(json);
        }
        let root = self.resolve_root(py, path);
        let mut json = json;
        if self.vcs {
            // Build the index off-GIL (the history walk is the expensive
            // part, #620); cache it under the repo root for reuse.
            let entry = if let Some(slot) = self.indexes.get(&root) {
                slot.as_ref()
            } else {
                let built = py.detach(|| vcs_bridge::build_index_for(&root));
                self.indexes.entry(root.clone()).or_insert(built).as_ref()
            };
            if let Some(index) = entry {
                json = vcs_bridge::inject_vcs_with_index(json, path, index)?;
            }
        }
        if self.vcs_per_function {
            let opened = if self.blames.contains_key(&root) {
                self.blames.get(&root)
            } else {
                let built = py.detach(|| vcs_bridge::open_blame_for(&root));
                self.blames.insert(root.clone(), built);
                self.blames.get(&root)
            };
            if let Some(Some(blame)) = opened {
                json = vcs_bridge::inject_vcs_per_function_with_blame(json, path, blame)?;
            }
        }
        Ok(json)
    }
}

/// Walk one or more path seeds and analyse every discovered file, returning
/// the [`analyze_batch`] result shape (issue #658).
///
/// Each positional `path` may be a file or a directory; directories are
/// walked with `.gitignore` awareness (the same [`ignore`](crate::walk)
/// crate the CLI walker uses), honouring the `include` / `exclude` globs.
/// The walk is the discovery step `analyze_batch` lacks; per-file analysis,
/// the never-raise contract (failures become `AnalysisFailure` elements),
/// the generated-file filter, and language inference are identical to
/// `analyze_batch`. The kwarg surface mirrors `analyze` / `analyze_batch`
/// (`exclude_tests` / `allow_lossy_path` / `skip_generated` / `metrics` /
/// `vcs` / `vcs_per_function`), so a directory walk threads VCS attachment
/// through the same shared-per-repo index (#670).
#[pyfunction]
#[pyo3(signature = (*paths, include = None, exclude = None, respect_gitignore = true, exclude_tests = false, allow_lossy_path = false, skip_generated = true, metrics = None, vcs = false, vcs_per_function = false))]
#[allow(
    clippy::needless_pass_by_value,
    clippy::fn_params_excessive_bools,
    clippy::too_many_arguments
)]
pub(crate) fn analyze_paths<'py>(
    py: Python<'py>,
    paths: &Bound<'py, PyTuple>,
    include: Option<StrOrSeq>,
    exclude: Option<StrOrSeq>,
    respect_gitignore: bool,
    exclude_tests: bool,
    allow_lossy_path: bool,
    skip_generated: bool,
    metrics: Option<Vec<String>>,
    vcs: bool,
    vcs_per_function: bool,
) -> PyResult<Vec<Py<PyAny>>> {
    // Resolve `metrics=` first (matching `analyze_batch`'s #268 ordering)
    // so a bad selection aborts before any filesystem walk.
    let metric_set = crate::resolve_metric_set(metrics)?;
    let seeds: Vec<PathBuf> = paths
        .iter()
        .map(|p| p.extract::<PathBuf>())
        .collect::<PyResult<_>>()?;
    let filters = WalkFilters::compile(
        include.map(StrOrSeq::into_vec).unwrap_or_default(),
        exclude.map(StrOrSeq::into_vec).unwrap_or_default(),
    )?;
    // Discover the corpus off-GIL — the walk is pure filesystem traversal
    // touching no Python objects.
    let discovered = py.detach(|| walk_paths(&seeds, &filters, respect_gitignore));

    let opts = AnalyzeOptions {
        exclude_tests,
        allow_lossy_path,
        skip_generated,
        metrics: metric_set,
    };
    let mut vcs_repos = VcsRepoCache::new(vcs, vcs_per_function);
    let mut results: Vec<Py<PyAny>> = Vec::with_capacity(discovered.len());
    for path in &discovered {
        push_one_result(py, path, opts, &mut vcs_repos, &mut results)?;
    }
    Ok(results)
}

/// Either a single glob string or a sequence of them, for the `include` /
/// `exclude` kwargs on [`analyze_paths`] (mirrors the #619-widened
/// `Sequence[str] | str` accepted elsewhere). A bare `str` is matched
/// before the generic sequence extraction because a `str` is itself an
/// iterable of one-character strings.
pub(crate) struct StrOrSeq(Vec<String>);

impl StrOrSeq {
    fn into_vec(self) -> Vec<String> {
        self.0
    }
}

impl<'a, 'py> FromPyObject<'a, 'py> for StrOrSeq {
    type Error = PyErr;

    fn extract(value: pyo3::Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        if let Ok(s) = value.cast::<pyo3::types::PyString>() {
            return Ok(Self(vec![s.to_str()?.to_owned()]));
        }
        let globs: Vec<String> = value.extract().map_err(|_| {
            PyValueError::new_err(
                "include / exclude must be a str glob or a sequence of glob strings",
            )
        })?;
        Ok(Self(globs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn from_internal_maps_unsupported_language_arm() {
        let err = PyAnalysisError::from_internal(
            AnalysisError::UnsupportedLanguage("no language for `.xyz`".to_owned()),
            Path::new("/tmp/a.xyz"),
        );
        assert_eq!(err.error_kind, "UnsupportedLanguage");
        assert_eq!(err.path, "/tmp/a.xyz");
        assert!(err.error.contains("no language"));
    }

    #[test]
    fn from_internal_maps_io_arm() {
        // Use *distinct* caller and embedded paths so a regression
        // that read `AnalysisError::Io { source, path }`'s embedded
        // path instead of the caller-supplied path (a real risk
        // given `from_internal`'s docstring explicitly contrasts
        // the two — the 1:1 ordering invariant relies on the
        // caller path winning) is caught by the equality assertion
        // below. Lesson #31 in docs/development/lessons_learned.md
        // warns against the prior `/nope`-for-both pattern.
        let io = std::io::Error::from(std::io::ErrorKind::NotFound);
        let err = PyAnalysisError::from_internal(
            AnalysisError::Io {
                source: io,
                path: PathBuf::from("/embedded-from-analyze-path"),
            },
            Path::new("/caller-supplied"),
        );
        assert_eq!(err.error_kind, "IoError");
        assert_eq!(
            err.path, "/caller-supplied",
            "from_internal must surface the caller path, not the embedded \
             Io::path (1:1 ordering invariant depends on this)",
        );
        assert!(
            !err.error.is_empty(),
            "Io arm must surface std::io::Error::Display text, got empty string",
        );
    }

    #[test]
    fn from_internal_maps_parse_arm() {
        // `MetricsError::EmptyRoot` is the cheapest variant to
        // construct (no associated data) and is the canonical
        // reserved parse-failure variant upstream — exercises the
        // `Parse(_)` → `ParseError` arm without depending on a
        // grammar build.
        let err = PyAnalysisError::from_internal(
            AnalysisError::Parse(big_code_analysis::MetricsError::EmptyRoot),
            Path::new("/tmp/a.rs"),
        );
        assert_eq!(err.error_kind, "ParseError");
        assert_eq!(err.path, "/tmp/a.rs");
        assert!(
            !err.error.is_empty(),
            "Parse arm must surface MetricsError::Display text, got empty string",
        );
    }

    #[test]
    fn from_internal_maps_serialization_arm() {
        // Build a fabricated `serde_json::Error` via a guaranteed-
        // failure parse so the test is not dependent on a future
        // change to FuncSpace serialisation. The arm maps to
        // `ParseError` (the closed taxonomy keeps three kinds);
        // the message is required to carry the "internal
        // serialization error" prefix so a retry classifier
        // keyed on `error_kind` can still distinguish the case
        // by message inspection.
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err = PyAnalysisError::from_internal(
            AnalysisError::Serialization(json_err),
            Path::new("/tmp/a.rs"),
        );
        assert_eq!(err.error_kind, "ParseError");
        assert_eq!(err.path, "/tmp/a.rs");
        assert!(
            err.error.contains("internal: serialization error"),
            "Serialization arm must prefix the message so callers can \
             distinguish it from a real ParseError despite the shared kind \
             — and the prefix shape must match the other synthetic-error \
             sites in analyze_batch (`internal: <subkind>: <detail>`), \
             got: {}",
            err.error,
        );
    }

    // `#[cfg(unix)]` lives at the function level (matching the
    // pattern in `analysis::tests::analyze_path_rejects_non_utf8_path_by_default`)
    // so the test is hidden cleanly on non-Unix targets rather than
    // compiling to an empty body that vacuously passes — Windows
    // paths are WTF-8 / UTF-16 with their own non-UTF-8 surface,
    // out of scope for this fixture.
    #[cfg(unix)]
    #[test]
    fn from_internal_preserves_byte_uniqueness_for_distinct_non_utf8_paths() {
        // The two paths below differ in their non-UTF-8 byte
        // sequence; `to_string_lossy` would collapse both to
        // "/a\u{FFFD}" (one U+FFFD character) and break the
        // documented `set(results)` dedup contract. With the
        // `Debug`-format fallback they map to two distinct strings.
        use std::os::unix::ffi::OsStrExt;
        let p1 = std::ffi::OsStr::from_bytes(b"/a\xff");
        let p2 = std::ffi::OsStr::from_bytes(b"/a\xfe");
        let e1 = PyAnalysisError::from_internal(AnalysisError::NonUtf8Path, Path::new(p1));
        let e2 = PyAnalysisError::from_internal(AnalysisError::NonUtf8Path, Path::new(p2));
        assert_ne!(
            e1.path, e2.path,
            "distinct non-UTF-8 paths must produce distinct PyAnalysisError.path \
             strings; collapsing them violates the set(results) dedup contract",
        );
        assert_ne!(e1, e2, "PartialEq must distinguish the two errors");
    }

    #[test]
    fn from_internal_maps_non_utf8_path_to_io_error() {
        // The `NonUtf8Path` variant gets folded into `IoError` so the
        // public taxonomy stays at three kinds. Test-via-revert:
        // changing the arm back to a fourth kind makes this fail.
        let err = PyAnalysisError::from_internal(AnalysisError::NonUtf8Path, Path::new("/x"));
        assert_eq!(err.error_kind, "IoError");
        assert!(err.error.contains("not valid UTF-8"));
    }

    #[test]
    fn equal_errors_hash_equal() {
        // PyO3's `#[pyclass(eq, hash)]` derives `__eq__` / `__hash__`
        // from `PartialEq` / `Hash`. This Rust-side check pins the
        // contract for the Python `set` / `dict` use case called out
        // in the issue body without needing the GIL.
        //
        // Construct two *independent* instances rather than cloning
        // — a clone is byte-identical so the test would pass even
        // under a stateful `new_internal` regression (e.g. one that
        // mixed a per-call counter into the error field). Going
        // through the constructor twice exercises the contract that
        // matters: two equal-by-value batch failures must hash and
        // compare equal so `set(results)` actually de-duplicates.
        let make =
            || PyAnalysisError::new_internal("p".to_owned(), "m".to_owned(), ErrorKind::IoError);
        let a = make();
        let b = make();
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }
}
