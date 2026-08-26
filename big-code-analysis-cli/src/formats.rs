#![allow(clippy::needless_pass_by_value)]

use std::fs::{File, create_dir_all};
use std::io::{BufWriter, Write};
use std::path::{Component, Path, PathBuf};

use clap::ValueEnum;
use serde::Serialize;

use big_code_analysis::{CSV_EXTENSION, FuncSpace, write_csv, write_csv_aggregate};

pub(crate) const CBOR_STDOUT_ERROR: &str =
    "CBOR is binary and cannot be printed to stdout; use --output";

fn ser_err(e: impl std::error::Error + Send + Sync + 'static) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

/// Per-file serialization formats accepted by `bca metrics` and
/// `bca ops`. Aggregated report formats (Markdown / HTML) live on
/// `bca report` — see [`ReportFormat`]. CI/IDE offender formats
/// (Checkstyle, SARIF, clang-warning, msvc-warning) live on
/// `bca check --output-format` — see
/// [`crate::check_format::AggregatedFormat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub(crate) enum MetricsFormat {
    /// Human-readable colored tree printed to stdout (`metrics` shows the
    /// metric tree, `ops` the operator/operand tree) — the default when
    /// `--format` is omitted. Selecting it explicitly produces
    /// byte-identical output to omitting the flag, so it is the way to
    /// request the default in a script or to override a `bca.toml` that
    /// set a structured format. It only ever streams to stdout, so unlike
    /// the structured serializers it has no file destination: pairing it
    /// with `--output`/`--output-dir` is a hard error, not a
    /// silent no-op — pass a structured `--format` to write files.
    // The named `text` default was introduced in issue #604; the
    // hard-error pairing rule with `--output`/`--output-dir` is #661.
    // (Issue refs stay in `//` maintainer comments, never `///` help.)
    Text,
    Cbor,
    Csv,
    Json,
    Toml,
    Yaml,
}

/// Aggregated report formats accepted by `bca report`. Both render the
/// same hotspot tables across the whole walk: Markdown is plain-text,
/// HTML is a single self-contained page with sortable tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub(crate) enum ReportFormat {
    Markdown,
    Html,
}

/// Output formats accepted by `bca vcs`. Superset of the per-file
/// [`MetricsFormat`] structured set plus the aggregated [`ReportFormat`]
/// rendered pages (`markdown` / `html`): a whole-repo change-history
/// report is a single document, so — unlike `metrics` / `ops` — every
/// format here writes one file (or stdout), never a per-file directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub(crate) enum VcsFormat {
    /// The human-readable ranked table — the same output a bare `bca vcs`
    /// prints, now explicitly selectable so the human format is named and
    /// requestable everywhere.
    // The named, requestable `text` format was unified in issue #659.
    Text,
    Cbor,
    Csv,
    Json,
    Toml,
    Yaml,
    Markdown,
    Html,
}

/// Output formats accepted by `bca vcs commit`. A single commit's JIT score
/// is one structured document, so only the structured serializers apply
/// — the ranked-report renderings (`markdown` / `html`) and the per-file
/// `csv` row shape do not. JSON is the default (the issue's headline
/// output); CBOR is binary and must go to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[clap(rename_all = "lower")]
pub(crate) enum JitFormat {
    #[default]
    Json,
    Yaml,
    Toml,
    Cbor,
}

/// Output formats accepted by `bca vcs trend` (issue #333). A historical
/// time series is one nested structured document, so the ranked-report
/// renderings (`markdown` / `html`) and the per-file `csv` row shape do
/// not apply. TOML is also excluded: a file absent at a point serializes
/// as a `null` array element, which TOML cannot represent. JSON is the
/// default; CBOR is binary and must go to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
#[clap(rename_all = "lower")]
pub(crate) enum TrendFormat {
    #[default]
    Json,
    Yaml,
    Cbor,
}

/// How a `MetricsFormat` should be dispatched. Carries enough type
/// information that the compiler — not a pair of boolean predicates
/// in lock-step with a downstream `match` — enforces that every
/// variant is routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetricsDispatch {
    /// Per-file output through the generic `T: Serialize` writer.
    Generic(GenericFormat),
    /// Per-file CSV output. CSV's row shape is metric-specific so it
    /// needs a concrete `&FuncSpace` rather than the generic
    /// `T: Serialize` writer.
    Csv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenericFormat {
    Cbor,
    Json,
    Toml,
    Yaml,
}

impl GenericFormat {
    /// Write one file's document under the `--output-dir` tree through
    /// this format's `T: Serialize` writer. Exhaustive over
    /// `GenericFormat` — every variant is handled, no wildcards.
    pub(crate) fn dump<T: Serialize>(
        self,
        space: T,
        path: &Path,
        output_path: &Path,
        pretty: bool,
    ) -> std::io::Result<()> {
        match self {
            Self::Cbor => Cbor::with_writer(space, path, output_path),
            Self::Json => Json::with_pretty_writer(space, path, output_path, pretty),
            Self::Toml => Toml::with_pretty_writer(space, path, output_path, pretty),
            Self::Yaml => Yaml::with_writer(space, path, output_path),
        }
    }

    /// Send one analyzed file's value to the run's destination:
    /// [`dump`](Self::dump) into its own file under `output_dir`, or
    /// [`render`](Self::render) back to the caller as the stdout
    /// document.
    pub(crate) fn emit<T: Serialize>(
        self,
        value: T,
        path: &Path,
        output_dir: Option<&PathBuf>,
        pretty: bool,
    ) -> std::io::Result<Document> {
        match output_dir {
            Some(dir) => self.dump(value, path, dir, pretty).map(|()| None),
            None => self.render(value, pretty).map(Some),
        }
    }

    /// Render one file's stdout document — the serialized text plus the
    /// trailing newline — instead of writing it.
    ///
    /// Rendering and emitting are split so the walk can put the
    /// documents back into walk order before they reach stdout (#1303);
    /// the bytes are byte-identical to what the `writeln!`-per-document
    /// path emitted before.
    pub(crate) fn render<T: Serialize>(self, space: T, pretty: bool) -> std::io::Result<Vec<u8>> {
        match self {
            // Rejected upstream by `resolve_structured_output`, which
            // requires a destination for CBOR; kept so this match needs
            // no wildcard.
            Self::Cbor => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                CBOR_STDOUT_ERROR,
            )),
            Self::Json => Json::document_pretty(space, pretty),
            Self::Toml => Toml::document_pretty(space, pretty),
            Self::Yaml => Yaml::document(space),
        }
    }
}

/// The stdout document one file's emission produced, or `None` when it
/// went to a destination that writes its own output (`--output-dir`,
/// `--output`, the human-readable tree) or produced nothing at all.
///
/// The stdout modes hand the bytes back rather than writing them so the
/// walk can emit the documents in walk order (#1303).
pub(crate) type Document = Option<Vec<u8>>;

/// [`GenericFormat::emit`] for the CSV row shape, which is not a
/// `GenericFormat`: its columns need the concrete `&FuncSpace` they are
/// derived from rather than a `T: Serialize`.
pub(crate) fn emit_csv(
    space: &FuncSpace,
    path: &Path,
    output_dir: Option<&PathBuf>,
) -> std::io::Result<Document> {
    match output_dir {
        Some(dir) => dump_csv(space, path, dir).map(|()| None),
        None => render_csv(space, path).map(Some),
    }
}

impl MetricsFormat {
    /// Classify this format for dispatch. Exhaustive — adding a new
    /// `MetricsFormat` variant is a compile error here, which is the
    /// point.
    pub(crate) fn dispatch(self) -> MetricsDispatch {
        match self {
            Self::Cbor => MetricsDispatch::Generic(GenericFormat::Cbor),
            // `Text` is collapsed to `None` (the human-readable tree) by
            // `StructuredArgs::normalize_text_format` before any dispatch,
            // so it never reaches the structured writers; it shares the
            // `Json` arm only to keep this match exhaustive without a
            // banned `panic!`/`unreachable!`. The path is dead in practice.
            Self::Json | Self::Text => MetricsDispatch::Generic(GenericFormat::Json),
            Self::Toml => MetricsDispatch::Generic(GenericFormat::Toml),
            Self::Yaml => MetricsDispatch::Generic(GenericFormat::Yaml),
            Self::Csv => MetricsDispatch::Csv,
        }
    }
}

/// Bytes buffered in front of every output destination.
///
/// The incremental serializers (`serde_json::to_writer`,
/// `serde_yaml::to_writer`, `ciborium::into_writer`) issue one write per
/// structural token, and both a raw `File` and a `StdoutLock` pass each
/// one straight to the kernel: a 12 MB aggregate document measured
/// 4.76 million `write(2)` calls, ~2.6 bytes apiece. 64 KiB is large
/// enough that every ordinary per-file document lands in a single write
/// and a large one amortizes to one write per 64 KiB.
const OUTPUT_BUFFER_BYTES: usize = 64 * 1_024;

/// Run `write` against `sink` through a [`BufWriter`], flushing before
/// returning.
///
/// The explicit flush is the load-bearing part, not the buffering. A
/// `BufWriter` flushed only by `Drop` discards the error it hit while
/// doing so, which would turn a full disk or a revoked mount into a
/// silently truncated file and a zero exit status. Flushing here folds
/// that failure back into the returned `Result`.
///
/// Generic over `W` rather than taking a `File` so tests can substitute
/// a sink that counts writes or fails on demand — see
/// `write_flushed_coalesces_small_writes_into_one` and
/// `write_flushed_surfaces_a_write_error_the_buffer_deferred`.
fn write_flushed<W, F>(sink: W, write: F) -> std::io::Result<()>
where
    W: Write,
    F: FnOnce(&mut dyn Write) -> std::io::Result<()>,
{
    let mut buffered = BufWriter::with_capacity(OUTPUT_BUFFER_BYTES, sink);
    write(&mut buffered)?;
    buffered.flush()
}

/// Open `path` for writing, materializing missing parent directories
/// only when the open actually failed for want of them.
///
/// The eager `create_dir_all` this replaces ran before *every*
/// `File::create`, so a `--output-dir` run paid a `mkdir` attempt per
/// output file for a directory set created once and then hit
/// repeatedly. `create_dir_all` remains race-tolerant, so two `bca`
/// processes writing into the same output directory still both succeed.
fn create_file(path: &Path) -> std::io::Result<File> {
    match File::create(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            ensure_parent_dir(path)?;
            File::create(path)
        }
        result => result,
    }
}

/// Run `write` against a buffered handle on the file `path`, creating
/// any missing parent directories.
pub(crate) fn write_buffered_file<F>(path: &Path, write: F) -> std::io::Result<()>
where
    F: FnOnce(&mut dyn Write) -> std::io::Result<()>,
{
    write_flushed(create_file(path)?, write)
}

/// Run `write` against a buffered handle on `output` — the file at that
/// path, or stdout when `None`.
///
/// Stdout is buffered too: `Stdout` is a `LineWriter`, which coalesces
/// compact JSON by accident but still emits one write per line of a
/// pretty-printed document or per row of a CSV.
pub(crate) fn write_buffered<F>(output: Option<&Path>, write: F) -> std::io::Result<()>
where
    F: FnOnce(&mut dyn Write) -> std::io::Result<()>,
{
    match output {
        Some(path) => write_buffered_file(path, write),
        // The lock is held for the whole document so a parallel walk
        // cannot interleave two files' output, exactly as before.
        None => write_flushed(std::io::stdout().lock(), write),
    }
}

/// Write a CSV document for the metric tree rooted at `space` under the
/// `--output-dir` tree, in a file whose name mirrors the input path
/// (with `.csv` appended).
fn dump_csv(space: &FuncSpace, path: &Path, output_path: &Path) -> std::io::Result<()> {
    write_buffered_file(&handle_path(path, output_path, CSV_EXTENSION), |w| {
        write_csv(space, path, w)
    })
}

/// Render the same CSV document [`dump_csv`] writes, for the stdout
/// path — which emits documents in walk order rather than in worker
/// completion order and so needs the bytes in hand (#1303).
fn render_csv(space: &FuncSpace, path: &Path) -> std::io::Result<Vec<u8>> {
    let mut document = Vec::new();
    write_csv(space, path, &mut document)?;
    Ok(document)
}

/// Serialize the whole `items` slice as ONE aggregate document to the
/// single file `output` (#669). `--output <FILE>` on `metrics` / `ops`
/// means a single file everywhere: the per-file-tree mode moved to
/// `--output-dir`. The aggregate shape is a top-level array of the same
/// per-file documents the directory mode emits, reusing the existing
/// `T: Serialize` serializers.
pub(crate) fn dump_aggregate<T: Serialize>(
    format: GenericFormat,
    items: &[T],
    output: &Path,
    pretty: bool,
) -> std::io::Result<()> {
    write_buffered_file(output, |w| match format {
        GenericFormat::Json => {
            if pretty {
                serde_json::to_writer_pretty(w, &items).map_err(ser_err)
            } else {
                serde_json::to_writer(w, &items).map_err(ser_err)
            }
        }
        GenericFormat::Toml => {
            // TOML has no top-level array; wrap under a `files` key so the
            // aggregate is a valid TOML document.
            #[derive(Serialize)]
            struct TomlAggregate<'a, T> {
                files: &'a [T],
            }
            let wrapped = TomlAggregate { files: items };
            let text = if pretty {
                toml::to_string_pretty(&wrapped).map_err(ser_err)?
            } else {
                toml::to_string(&wrapped).map_err(ser_err)?
            };
            w.write_all(text.as_bytes())
        }
        GenericFormat::Yaml => serde_yaml::to_writer(w, &items).map_err(ser_err),
        GenericFormat::Cbor => ciborium::into_writer(&items, w).map_err(ser_err),
    })
}

/// Write every space's CSV rows into ONE aggregate `--output <FILE>`
/// (#669). Delegates to [`write_csv_aggregate`], which emits the shared
/// [`CSV_HEADER`](big_code_analysis::CSV_HEADER) exactly once — unlike a
/// per-file `write_csv` loop, which would repeat the header before every
/// file's rows and corrupt the concatenated document.
pub(crate) fn dump_csv_aggregate(
    spaces: &[(FuncSpace, PathBuf)],
    output: &Path,
) -> std::io::Result<()> {
    write_buffered_file(output, |w| {
        write_csv_aggregate(
            spaces.iter().map(|(space, path)| (space, path.as_path())),
            w,
        )
    })
}

/// Terminate a rendered document exactly as the `writeln!`-per-document
/// stdout path did, so the bytes are unchanged by the #1303 split of
/// rendering from emission.
#[inline]
fn into_document(content: String) -> Vec<u8> {
    let mut document = content.into_bytes();
    document.push(b'\n');
    document
}

trait RenderDocument {
    #[inline]
    fn document<T: Serialize>(content: T) -> std::io::Result<Vec<u8>> {
        Ok(into_document(Self::format(content)?))
    }

    fn format<T: Serialize>(content: T) -> std::io::Result<String>;
}

trait RenderPrettyDocument: RenderDocument {
    fn document_pretty<T: Serialize>(content: T, pretty: bool) -> std::io::Result<Vec<u8>> {
        Ok(into_document(if pretty {
            Self::format_pretty(content)?
        } else {
            Self::format(content)?
        }))
    }
    fn format_pretty<T: Serialize>(content: T) -> std::io::Result<String>;
}

/// Escaped marker substituted for a `..` (`ParentDir`) component. A bare
/// `.` (as the previous implementation used) is a no-op in path joining,
/// so `../sibling/x.rs` collapsed onto `sibling/x.rs` and one output
/// silently clobbered the other (issue #423). `%2E%2E` is the
/// percent-encoding of `..`; pairing it with `%`-escaping of `Normal`
/// components (see [`push_escaped_component`]) keeps the mapping
/// injective — distinct input paths always yield distinct output paths.
const PARENT_DIR_MARKER: &str = "%2E%2E";

/// Append a `Normal` `component` to `out`, escaping every literal `%` to
/// `%25`.
///
/// This is what makes [`PARENT_DIR_MARKER`] collision-free: the only `%`
/// characters in a `handle_path` result are ones emitted here or by the
/// marker, so a literal directory named `%2E%2E` escapes to `%252E%252E`
/// and can never alias a genuine `..` component. The escape is done at the
/// byte (Unix) / WTF-16 code-unit (Windows) level, so non-UTF-8
/// components survive verbatim without any lossy `to_str` conversion.
#[cfg(unix)]
fn push_escaped_component(out: &mut PathBuf, component: &std::ffi::OsStr) {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let bytes = component.as_bytes();
    if !bytes.contains(&b'%') {
        out.push(component);
        return;
    }
    let mut escaped = Vec::with_capacity(bytes.len() + 2);
    for &b in bytes {
        if b == b'%' {
            escaped.extend_from_slice(b"%25");
        } else {
            escaped.push(b);
        }
    }
    out.push(std::ffi::OsString::from_vec(escaped));
}

#[cfg(windows)]
fn push_escaped_component(out: &mut PathBuf, component: &std::ffi::OsStr) {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    // `%` is U+0025 (BMP); operating on code units is lossless and never
    // touches `to_string_lossy` on a path used as an output identifier.
    const PERCENT_UNIT: u16 = b'%' as u16;
    const ESCAPED: [u16; 3] = [b'%' as u16, b'2' as u16, b'5' as u16];
    let mut units = Vec::new();
    let mut saw_percent = false;
    for unit in component.encode_wide() {
        if unit == PERCENT_UNIT {
            saw_percent = true;
            units.extend_from_slice(&ESCAPED);
        } else {
            units.push(unit);
        }
    }
    if saw_percent {
        out.push(std::ffi::OsString::from_wide(&units));
    } else {
        out.push(component);
    }
}

// Non-Unix, non-Windows targets (e.g. wasm) have no stable lossless
// `OsStr` byte view; `handle_path` is only exercised by the native CLI on
// Unix and Windows, so this `%`-free fallback keeps those builds compiling
// without claiming an injectivity guarantee the platform cannot back.
#[cfg(not(any(unix, windows)))]
fn push_escaped_component(out: &mut PathBuf, component: &std::ffi::OsStr) {
    out.push(component);
}

fn handle_path(path: &Path, output_path: &Path, extension: &str) -> PathBuf {
    // Walk components rather than iterating raw OsStr fragments: this
    // strips Windows path prefixes (`C:`, `\\?\…`) and root separators
    // alongside Unix `/` and `./`, so `output_path.join(filename)` does
    // not get overridden by an absolute input filename.
    //
    // Components are escaped through `push_escaped_component` (which keeps
    // non-UTF-8 bytes intact). `..` becomes `PARENT_DIR_MARKER` rather
    // than the no-op `.` it used to (issue #423), and any literal `%` is
    // doubled to `%25` so the marker can never alias a real directory
    // name. Within a single *normalized form* the mapping is injective:
    // distinct relative inputs yield distinct output filenames.
    //
    // The mapping is NOT injective *across* forms, because the leading
    // `Prefix` / `RootDir` / `CurDir` components are stripped (so the
    // output stays inside `output_path` rather than being overridden by
    // an absolute input filename): an absolute `/a/x.rs` and a relative
    // `a/x.rs` both collapse to `a/x.rs<ext>` (#704). The walker no longer
    // feeds both forms for the same file — `reanchor_seed` rewrites
    // under-CWD absolute seeds to their relative tail and
    // `expand_seed_paths` de-duplicates the emitted set — so this residual
    // cross-form collision is not reachable from a normal walk; it remains
    // a property of `handle_path` in isolation, not a guarantee it makes.
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            // Keep files inside the output folder while remaining
            // collision-free: a no-op `.` would let `../x` clobber `x`.
            Component::ParentDir => cleaned.push(PARENT_DIR_MARKER),
            Component::Normal(s) => push_escaped_component(&mut cleaned, s),
        }
    }

    let mut filename = cleaned.into_os_string();
    filename.push(extension);
    output_path.join(filename)
}

trait WriteFile {
    const EXTENSION: &'static str;

    /// Run `write` against a buffered handle on this format's per-file
    /// destination under `output_path`.
    fn with_file<F>(path: &Path, output_path: &Path, write: F) -> std::io::Result<()>
    where
        F: FnOnce(&mut dyn Write) -> std::io::Result<()>,
    {
        write_buffered_file(&handle_path(path, output_path, Self::EXTENSION), write)
    }

    fn with_writer<T: Serialize>(
        content: T,
        path: &Path,
        output_path: &Path,
    ) -> std::io::Result<()>;
}

trait WritePrettyFile: WriteFile {
    fn with_pretty_writer<T: Serialize>(
        content: T,
        path: &Path,
        output_path: &Path,
        pretty: bool,
    ) -> std::io::Result<()>;
}

struct Json;

impl RenderDocument for Json {
    fn format<T: Serialize>(content: T) -> std::io::Result<String> {
        serde_json::to_string(&content).map_err(ser_err)
    }
}

impl RenderPrettyDocument for Json {
    fn format_pretty<T: Serialize>(content: T) -> std::io::Result<String> {
        serde_json::to_string_pretty(&content).map_err(ser_err)
    }
}

impl WriteFile for Json {
    const EXTENSION: &'static str = ".json";

    fn with_writer<T: Serialize>(
        content: T,
        path: &Path,
        output_path: &Path,
    ) -> std::io::Result<()> {
        Self::with_file(path, output_path, |w| {
            serde_json::to_writer(w, &content).map_err(ser_err)
        })
    }
}

impl WritePrettyFile for Json {
    fn with_pretty_writer<T: Serialize>(
        content: T,
        path: &Path,
        output_path: &Path,
        pretty: bool,
    ) -> std::io::Result<()> {
        if pretty {
            Self::with_file(path, output_path, |w| {
                serde_json::to_writer_pretty(w, &content).map_err(ser_err)
            })
        } else {
            Self::with_writer(content, path, output_path)
        }
    }
}

struct Toml;

impl RenderDocument for Toml {
    fn format<T: Serialize>(content: T) -> std::io::Result<String> {
        toml::to_string(&content).map_err(ser_err)
    }
}

impl RenderPrettyDocument for Toml {
    fn format_pretty<T: Serialize>(content: T) -> std::io::Result<String> {
        toml::to_string_pretty(&content).map_err(ser_err)
    }
}

impl WriteFile for Toml {
    const EXTENSION: &'static str = ".toml";

    fn with_writer<T: Serialize>(
        content: T,
        path: &Path,
        output_path: &Path,
    ) -> std::io::Result<()> {
        Self::with_file(path, output_path, |w| {
            w.write_all(Self::format(content)?.as_bytes())
        })
    }
}

impl WritePrettyFile for Toml {
    fn with_pretty_writer<T: Serialize>(
        content: T,
        path: &Path,
        output_path: &Path,
        pretty: bool,
    ) -> std::io::Result<()> {
        if pretty {
            Self::with_file(path, output_path, |w| {
                w.write_all(Self::format_pretty(&content)?.as_bytes())
            })
        } else {
            Self::with_writer(content, path, output_path)
        }
    }
}

struct Yaml;

impl RenderDocument for Yaml {
    fn format<T: Serialize>(content: T) -> std::io::Result<String> {
        serde_yaml::to_string(&content).map_err(ser_err)
    }
}

impl WriteFile for Yaml {
    const EXTENSION: &'static str = ".yml";

    fn with_writer<T: Serialize>(
        content: T,
        path: &Path,
        output_path: &Path,
    ) -> std::io::Result<()> {
        Self::with_file(path, output_path, |w| {
            serde_yaml::to_writer(w, &content).map_err(ser_err)
        })
    }
}

struct Cbor;

impl WriteFile for Cbor {
    const EXTENSION: &'static str = ".cbor";

    fn with_writer<T: Serialize>(
        content: T,
        path: &Path,
        output_path: &Path,
    ) -> std::io::Result<()> {
        Self::with_file(path, output_path, |w| {
            ciborium::into_writer(&content, w).map_err(ser_err)
        })
    }
}

/// Create the parent directory of `path` if it does not yet exist, so a
/// `--output sub/dir/report.json` to a missing directory succeeds rather
/// than failing with "No such file or directory" (issue #709).
fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent)?;
    }
    Ok(())
}

/// Write a rendered single-document text report (Markdown / HTML / JSON /
/// YAML / TOML) to a single file — creating its parent directory — or to
/// stdout when `output` is `None`. Shared by the `vcs commit` / `vcs trend`
/// / `vcs` single-file emit paths so they cannot drift.
pub(crate) fn write_text(content: &str, output: Option<&PathBuf>) -> std::io::Result<()> {
    if let Some(path) = output {
        ensure_parent_dir(path)?;
        return std::fs::write(path, content);
    }
    // The flush is what makes the failure observable. `Stdout` is a
    // `LineWriter` over a 1 KiB buffer, so a payload with no newline
    // anywhere in it and shorter than that is accepted here and only
    // reaches the fd during the exit-time cleanup flush, whose error is
    // discarded: `bca vcs -O json`, `vcs commit`, and `vcs trend` — the
    // three that emit *compact* JSON — all exited 0 having emitted
    // nothing (#1132's sweep missed them; every other format carries a
    // newline, so the buffer spills on its own).
    let mut out = std::io::stdout().lock();
    out.write_all(content.as_bytes())?;
    out.flush()
}

/// Serialize `value` as JSON and write it through [`write_text`].
///
/// One of the four single-document serializers the `vcs` emit paths
/// share (`vcs`, `vcs commit`, `vcs trend`). Each had its own verbatim
/// copy, so a fix to one — the `map_err` that turns a serializer failure
/// into an `io::Error`, say — reached only a third of the commands.
pub(crate) fn write_json<T: Serialize>(
    value: &T,
    pretty: bool,
    output: Option<&PathBuf>,
) -> std::io::Result<()> {
    let json = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .map_err(std::io::Error::other)?;
    write_text(&json, output)
}

/// Serialize `value` as YAML and write it through [`write_text`]. See
/// [`write_json`] for why these live here.
pub(crate) fn write_yaml<T: Serialize>(value: &T, output: Option<&PathBuf>) -> std::io::Result<()> {
    let yaml = serde_yaml::to_string(value).map_err(std::io::Error::other)?;
    write_text(&yaml, output)
}

/// Serialize `value` as TOML and write it through [`write_text`]. See
/// [`write_json`] for why these live here.
pub(crate) fn write_toml<T: Serialize>(
    value: &T,
    pretty: bool,
    output: Option<&PathBuf>,
) -> std::io::Result<()> {
    let toml = if pretty {
        toml::to_string_pretty(value)
    } else {
        toml::to_string(value)
    }
    .map_err(std::io::Error::other)?;
    write_text(&toml, output)
}

/// Serialize `value` as CBOR into the file at `output`.
///
/// CBOR is binary, so it must land in a file — never stdout. That rule
/// lives here, next to [`CBOR_STDOUT_ERROR`], because the three `vcs`
/// emit paths (`vcs`, `vcs commit`, `vcs trend`) each carried a
/// byte-identical copy of it that differed only in the format enum they
/// matched on.
pub(crate) fn write_cbor<T: Serialize>(value: &T, output: Option<&PathBuf>) -> std::io::Result<()> {
    let Some(path) = output else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            CBOR_STDOUT_ERROR,
        ));
    };
    write_buffered_file(path, |w| {
        ciborium::into_writer(value, w).map_err(std::io::Error::other)
    })
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

    /// Records every `write` the buffering layer actually forwards.
    ///
    /// A count is the only thing that can distinguish a buffered
    /// destination from an unbuffered one: the bytes that reach the sink
    /// are identical either way, which is why `--output-dir` shipped at
    /// one `write(2)` per serialized token for as long as it did.
    #[derive(Default)]
    struct CountingSink {
        writes: usize,
        bytes: Vec<u8>,
    }

    impl Write for CountingSink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.writes += 1;
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Fails every `write`, the way a full filesystem does. `BufWriter`
    /// defers the first write until the buffer fills, so a small document
    /// only meets this failure at flush time.
    struct FullDiskSink;

    impl Write for FullDiskSink {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::StorageFull,
                "no space left on device",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The optimization itself, not its output. `serde_json::to_writer`
    /// and friends emit one write per structural token — 4.76 million of
    /// them for a 12 MB aggregate document before this seam existed.
    /// Reverting `write_flushed` to hand the sink straight to `write`
    /// changes no byte, so only this count fails.
    #[test]
    fn write_flushed_coalesces_small_writes_into_one() {
        // Three bytes apiece keeps the total (12 KiB) well under
        // OUTPUT_BUFFER_BYTES, so a correct implementation needs exactly
        // one write and an unbuffered one needs TOKENS.
        const TOKENS: usize = 4_000;

        let mut sink = CountingSink::default();
        write_flushed(&mut sink, |w| {
            for _ in 0..TOKENS {
                w.write_all(b"tok")?;
            }
            Ok(())
        })
        .expect("the counting sink never fails");

        assert_eq!(sink.bytes.len(), TOKENS * 3, "every byte must still land");
        assert_eq!(
            sink.writes, 1,
            "{TOKENS} token writes must coalesce into one; \
             an unbuffered destination would show {TOKENS}"
        );
    }

    /// A document larger than the buffer still reaches the sink whole,
    /// in one write per buffer-full rather than one per token.
    #[test]
    fn write_flushed_splits_only_on_buffer_boundaries() {
        const CHUNKS: usize = 4_096;
        const CHUNK_LEN: usize = 64;
        const TOTAL: usize = CHUNKS * CHUNK_LEN;

        let mut sink = CountingSink::default();
        write_flushed(&mut sink, |w| {
            for _ in 0..CHUNKS {
                w.write_all(&[b'x'; CHUNK_LEN])?;
            }
            Ok(())
        })
        .expect("the counting sink never fails");

        assert_eq!(sink.bytes.len(), TOTAL);
        assert_eq!(
            sink.writes,
            TOTAL / OUTPUT_BUFFER_BYTES,
            "a {TOTAL}-byte document must cost one write per full buffer"
        );
    }

    /// The correctness half of the change, and the more important one: a
    /// `BufWriter` left to `Drop` swallows the error it hits while
    /// flushing, so a full disk would produce a truncated file and a zero
    /// exit status. Deleting the explicit `flush()` in `write_flushed`
    /// makes this test return `Ok(())`.
    #[test]
    fn write_flushed_surfaces_a_write_error_the_buffer_deferred() {
        let err = write_flushed(FullDiskSink, |w| w.write_all(b"{\"metrics\":[]}"))
            .expect_err("a failing destination must surface as Err, not be swallowed by Drop");
        assert_eq!(err.kind(), std::io::ErrorKind::StorageFull);
    }

    /// `create_file` no longer runs `create_dir_all` ahead of every
    /// `File::create`; it retries after materializing the parents only
    /// when the open failed for want of them. Deleting that `NotFound`
    /// arm fails this test.
    #[test]
    fn create_file_materializes_missing_parents_on_retry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("a").join("b").join("out.json");

        let mut file = create_file(&nested).expect("create must recover from a missing parent");
        file.write_all(b"{}").expect("write");
        drop(file);

        assert_eq!(
            std::fs::read_to_string(&nested).expect("file written"),
            "{}"
        );
    }

    /// The common case — the directory already exists — must not depend
    /// on the retry arm at all.
    #[test]
    fn create_file_opens_directly_when_the_parent_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("out.json");

        create_file(&target).expect("create in an existing directory");

        assert!(target.exists(), "the file must be created in place");
    }

    /// A destination whose parent path component is a *file* is a user
    /// error, not a missing directory: it must surface as an error rather
    /// than be papered over by the retry.
    #[test]
    fn create_file_reports_a_non_directory_parent_as_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let blocker = dir.path().join("not-a-dir");
        std::fs::write(&blocker, b"i am a file").expect("write blocker");

        create_file(&blocker.join("out.json"))
            .expect_err("a file standing in for a directory must not be silently created");
    }

    // Regression test for issue #709: `write_text` with `--output
    // sub/dir/report.json` to a not-yet-existing directory must create the
    // parents and write the file (was "No such file or directory"). Shared
    // by `vcs commit` / `vcs trend` / `vcs`, so one guard covers all three.
    #[test]
    fn write_text_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("a").join("b").join("report.json");
        assert!(
            !nested.parent().expect("has parent").exists(),
            "parent must be absent before the write"
        );

        write_text("{}", Some(&nested)).expect("write must create parents");

        assert_eq!(
            std::fs::read_to_string(&nested).expect("file written"),
            "{}"
        );
    }

    #[test]
    fn handle_path_strips_root_slash() {
        let result = handle_path(Path::new("/foo/bar.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/foo/bar.rs.json"));
    }

    #[test]
    fn handle_path_strips_dot_slash() {
        let result = handle_path(Path::new("./foo/bar.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/foo/bar.rs.json"));
    }

    #[test]
    fn handle_path_escapes_dotdot_with_marker() {
        // `..` becomes the collision-free `%2E%2E` marker rather than the
        // no-op `.` the old implementation used (issue #423).
        let result = handle_path(Path::new("a/../b.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/a/%2E%2E/b.rs.json"));
    }

    #[test]
    fn handle_path_leading_dotdot_distinct_from_sibling() {
        // The exact collision from the issue: `../sibling/x.rs` must not
        // map onto the same output file as `sibling/x.rs`.
        let parent = handle_path(Path::new("../sibling/x.rs"), Path::new("out"), ".json");
        let sibling = handle_path(Path::new("sibling/x.rs"), Path::new("out"), ".json");
        assert_eq!(parent, PathBuf::from("out/%2E%2E/sibling/x.rs.json"));
        assert_eq!(sibling, PathBuf::from("out/sibling/x.rs.json"));
        assert_ne!(parent, sibling);
    }

    #[test]
    fn handle_path_multiple_dotdot_preserved() {
        let result = handle_path(Path::new("../../x.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/%2E%2E/%2E%2E/x.rs.json"));
    }

    #[test]
    fn handle_path_literal_marker_dir_escapes() {
        // A real directory literally named `%2E%2E` must not collide with
        // an escaped `..` component: its `%` doubles to `%25`.
        let literal = handle_path(Path::new("%2E%2E/x.rs"), Path::new("out"), ".json");
        let dotdot = handle_path(Path::new("../x.rs"), Path::new("out"), ".json");
        assert_eq!(literal, PathBuf::from("out/%252E%252E/x.rs.json"));
        assert_ne!(literal, dotdot);
    }

    #[test]
    fn handle_path_plain_relative() {
        let result = handle_path(Path::new("src/main.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/src/main.rs.json"));
    }

    #[cfg(unix)]
    #[test]
    fn handle_path_preserves_non_utf8_components() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let bad_component = OsStr::from_bytes(b"\xff\xfe");
        let path = PathBuf::from("src").join(bad_component).join("bar.rs");
        let result = handle_path(&path, Path::new("out"), ".json");
        // The non-UTF-8 component is preserved verbatim — distinct
        // input paths must produce distinct output filenames.
        let expected = PathBuf::from("out/src")
            .join(bad_component)
            .join("bar.rs.json");
        assert_eq!(result, expected);
    }

    #[cfg(unix)]
    #[test]
    fn output_filename_preserves_non_utf8_identity() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        // Two distinct non-UTF-8 byte sequences must produce two
        // distinct output paths — collapsing them onto the same name
        // (as the previous lossy implementation did by dropping the
        // component entirely) would clobber one file with the other.
        let a = OsStr::from_bytes(b"\xff\xfe");
        let b = OsStr::from_bytes(b"\xfe\xff");
        let path_a = PathBuf::from("src").join(a).join("x.rs");
        let path_b = PathBuf::from("src").join(b).join("x.rs");
        let out_a = handle_path(&path_a, Path::new("out"), ".json");
        let out_b = handle_path(&path_b, Path::new("out"), ".json");
        assert_ne!(out_a, out_b);
    }

    /// A minimal serializable stand-in for a metrics document. Two
    /// fields so a format that drops or reorders keys is visible, and a
    /// float so the numeric encodings differ per format.
    #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Item {
        name: String,
        value: f64,
    }

    /// Mirrors the private wrapper `dump_aggregate` writes for TOML, so
    /// a test can name the `files` key it must produce.
    #[derive(serde::Deserialize)]
    struct TomlAggregateDoc {
        files: Vec<Item>,
    }

    fn items() -> Vec<Item> {
        vec![
            Item {
                name: "alpha".to_owned(),
                value: 1.5,
            },
            Item {
                name: "beta".to_owned(),
                value: -2.0,
            },
        ]
    }

    /// Every `dump_aggregate` arm must produce a document that reads
    /// back as the list it was given. Round-tripping rather than
    /// asserting bytes keeps the test on the contract — that the file is
    /// a valid document of its format carrying the input — instead of on
    /// each serializer's incidental spacing.
    #[test]
    fn dump_aggregate_round_trips_every_format() {
        let dir = tempfile::tempdir().expect("tempdir");
        let expected = items();

        for pretty in [false, true] {
            let json = dir.path().join(format!("agg-{pretty}.json"));
            dump_aggregate(GenericFormat::Json, &expected, &json, pretty).expect("json");
            let got: Vec<Item> =
                serde_json::from_slice(&std::fs::read(&json).expect("read")).expect("parse json");
            assert_eq!(got, expected, "json pretty={pretty}");

            let toml_path = dir.path().join(format!("agg-{pretty}.toml"));
            dump_aggregate(GenericFormat::Toml, &expected, &toml_path, pretty).expect("toml");
            let text = std::fs::read_to_string(&toml_path).expect("read");
            // TOML has no top-level array, so the aggregate is wrapped
            // under `files`. Deserializing through a struct that names
            // that field asserts the wrapper: drop it and this fails,
            // where a bare "is it valid TOML" check would not.
            let doc: TomlAggregateDoc = toml::from_str(&text).expect("parse toml");
            assert_eq!(doc.files, expected, "toml pretty={pretty}");

            let cbor = dir.path().join(format!("agg-{pretty}.cbor"));
            dump_aggregate(GenericFormat::Cbor, &expected, &cbor, pretty).expect("cbor");
            let got: Vec<Item> = ciborium::from_reader(&std::fs::read(&cbor).expect("read")[..])
                .expect("parse cbor");
            assert_eq!(got, expected, "cbor pretty={pretty}");
        }

        let yaml = dir.path().join("agg.yaml");
        dump_aggregate(GenericFormat::Yaml, &expected, &yaml, false).expect("yaml");
        let got: Vec<Item> =
            serde_yaml::from_str(&std::fs::read_to_string(&yaml).expect("read")).expect("parse");
        assert_eq!(got, expected);
    }

    /// The `pretty` flag has to reach the serializer. Both spellings
    /// parse back to the same value, so a round-trip alone cannot tell
    /// them apart — the discriminating check is that exactly one of them
    /// is multi-line.
    #[test]
    fn pretty_selects_a_different_encoding_than_compact() {
        let dir = tempfile::tempdir().expect("tempdir");
        let value = items();

        let compact = dir.path().join("compact.json");
        let pretty = dir.path().join("pretty.json");
        write_json(&value, false, Some(&compact)).expect("compact json");
        write_json(&value, true, Some(&pretty)).expect("pretty json");
        let compact_text = std::fs::read_to_string(&compact).expect("read");
        let pretty_text = std::fs::read_to_string(&pretty).expect("read");
        assert_eq!(compact_text.lines().count(), 1, "compact json is one line");
        assert!(
            pretty_text.lines().count() > 1,
            "pretty json spans lines: {pretty_text}"
        );
        assert_eq!(
            serde_json::from_str::<Vec<Item>>(&compact_text).expect("compact parses"),
            serde_json::from_str::<Vec<Item>>(&pretty_text).expect("pretty parses"),
            "the two encodings carry the same value"
        );

        // `write_toml`'s pretty flag picks `to_string_pretty`; both are
        // valid TOML for the same document.
        let toml_compact = dir.path().join("compact.toml");
        let toml_pretty = dir.path().join("pretty.toml");
        write_toml(&value[0], false, Some(&toml_compact)).expect("compact toml");
        write_toml(&value[0], true, Some(&toml_pretty)).expect("pretty toml");
        let a: Item =
            toml::from_str(&std::fs::read_to_string(&toml_compact).expect("read")).expect("parse");
        let b: Item =
            toml::from_str(&std::fs::read_to_string(&toml_pretty).expect("read")).expect("parse");
        assert_eq!(a, b);
        assert_eq!(a, value[0]);

        let yaml = dir.path().join("out.yaml");
        write_yaml(&value, Some(&yaml)).expect("yaml");
        assert_eq!(
            serde_yaml::from_str::<Vec<Item>>(&std::fs::read_to_string(&yaml).expect("read"))
                .expect("parse"),
            value
        );
    }

    /// The reason `dump_csv_aggregate` exists: a per-file `write_csv`
    /// loop repeats the header before every file's rows, which corrupts
    /// the concatenated document. Two spaces must yield exactly one
    /// header line.
    #[test]
    fn csv_aggregate_emits_the_header_once_for_many_files() {
        let space = big_code_analysis::analyze(
            big_code_analysis::Source::new(big_code_analysis::LANG::Rust, b"fn a() {}\n"),
            big_code_analysis::MetricsOptions::default(),
        )
        .expect("snippet analyzes");

        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("agg.csv");
        dump_csv_aggregate(
            &[
                (space.clone(), PathBuf::from("a.rs")),
                (space, PathBuf::from("b.rs")),
            ],
            &out,
        )
        .expect("csv aggregate");

        let text = std::fs::read_to_string(&out).expect("read");
        // `CSV_HEADER` is the column list, so rebuild the header row
        // from it rather than assuming a rendering.
        let header = big_code_analysis::CSV_HEADER.join(",");
        let header_lines = text.lines().filter(|line| *line == header).count();
        assert_eq!(header_lines, 1, "exactly one header line in:\n{text}");
        assert!(
            text.lines().count() > 2,
            "both files contributed rows:\n{text}"
        );
    }
}
