#![allow(clippy::needless_pass_by_value)]

use std::fs::{File, create_dir_all};
use std::io::Write;
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
    /// Dispatch a generic per-file format through its
    /// `T: Serialize` writer. Exhaustive over `GenericFormat` — every
    /// variant is handled, no wildcards.
    pub(crate) fn dump<T: Serialize>(
        self,
        space: T,
        path: PathBuf,
        output_path: Option<&PathBuf>,
        pretty: bool,
    ) -> std::io::Result<()> {
        if let Some(output_path) = output_path {
            match self {
                Self::Cbor => Cbor::with_writer(space, &path, output_path),
                Self::Json => Json::with_pretty_writer(space, &path, output_path, pretty),
                Self::Toml => Toml::with_pretty_writer(space, &path, output_path, pretty),
                Self::Yaml => Yaml::with_writer(space, &path, output_path),
            }
        } else {
            match self {
                Self::Cbor => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    CBOR_STDOUT_ERROR,
                )),
                Self::Json => Json::write_on_stdout_pretty(space, pretty),
                Self::Toml => Toml::write_on_stdout_pretty(space, pretty),
                Self::Yaml => Yaml::write_on_stdout(space),
            }
        }
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

/// Run `write` against either a per-file path under `output_dir`
/// (with `extension` appended and any missing parent directories
/// created) or stdout. Shared scaffolding for the per-file `dump_*`
/// helpers (CSV).
fn write_per_file_or_stdout<F>(
    input_path: &Path,
    output_dir: Option<&PathBuf>,
    extension: &str,
    write: F,
) -> std::io::Result<()>
where
    F: FnOnce(&mut dyn Write) -> std::io::Result<()>,
{
    if let Some(output_dir) = output_dir {
        let format_path = handle_path(input_path, output_dir, extension);
        if let Some(parent) = format_path.parent() {
            create_dir_all(parent)?;
        }
        let mut file = File::create(format_path)?;
        write(&mut file)
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        write(&mut handle)
    }
}

/// Emit a CSV document for the metric tree rooted at `space`. If
/// `output_path` is `Some`, the document is written to a file in the
/// directory whose name mirrors the input path (with `.csv`
/// appended); otherwise it goes to stdout.
pub(crate) fn dump_csv(
    space: &FuncSpace,
    path: PathBuf,
    output_path: Option<&PathBuf>,
) -> std::io::Result<()> {
    write_per_file_or_stdout(&path, output_path, CSV_EXTENSION, |w| {
        write_csv(space, &path, w)
    })
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
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent)?;
    }
    let mut file = File::create(output)?;
    match format {
        GenericFormat::Json => {
            if pretty {
                serde_json::to_writer_pretty(&mut file, &items).map_err(ser_err)
            } else {
                serde_json::to_writer(&mut file, &items).map_err(ser_err)
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
            file.write_all(text.as_bytes())
        }
        GenericFormat::Yaml => serde_yaml::to_writer(&mut file, &items).map_err(ser_err),
        GenericFormat::Cbor => ciborium::into_writer(&items, &mut file).map_err(ser_err),
    }
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
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent)?;
    }
    let file = File::create(output)?;
    write_csv_aggregate(
        spaces.iter().map(|(space, path)| (space, path.as_path())),
        file,
    )
}

#[inline]
fn print_on_stdout(content: String) -> std::io::Result<()> {
    writeln!(std::io::stdout().lock(), "{content}")
}

trait WriteOnStdout {
    #[inline]
    fn write_on_stdout<T: Serialize>(content: T) -> std::io::Result<()> {
        print_on_stdout(Self::format(content)?)
    }

    fn format<T: Serialize>(content: T) -> std::io::Result<String>;
}

trait WritePrettyOnStdout: WriteOnStdout {
    fn write_on_stdout_pretty<T: Serialize>(content: T, pretty: bool) -> std::io::Result<()> {
        print_on_stdout(if pretty {
            Self::format_pretty(content)?
        } else {
            Self::format(content)?
        })
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

    fn open_file(path: &Path, output_path: &Path) -> std::io::Result<File> {
        let format_path = handle_path(path, output_path, Self::EXTENSION);
        if let Some(parent) = format_path.parent() {
            create_dir_all(parent)?;
        }
        File::create(format_path)
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

impl WriteOnStdout for Json {
    fn format<T: Serialize>(content: T) -> std::io::Result<String> {
        serde_json::to_string(&content).map_err(ser_err)
    }
}

impl WritePrettyOnStdout for Json {
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
        serde_json::to_writer(Self::open_file(path, output_path)?, &content).map_err(ser_err)
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
            serde_json::to_writer_pretty(Self::open_file(path, output_path)?, &content)
                .map_err(ser_err)
        } else {
            Self::with_writer(content, path, output_path)
        }
    }
}

struct Toml;

impl WriteOnStdout for Toml {
    fn format<T: Serialize>(content: T) -> std::io::Result<String> {
        toml::to_string(&content).map_err(ser_err)
    }
}

impl WritePrettyOnStdout for Toml {
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
        Self::open_file(path, output_path)?.write_all(Self::format(content)?.as_bytes())
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
            Self::open_file(path, output_path)?.write_all(Self::format_pretty(&content)?.as_bytes())
        } else {
            Self::with_writer(content, path, output_path)
        }
    }
}

struct Yaml;

impl WriteOnStdout for Yaml {
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
        serde_yaml::to_writer(Self::open_file(path, output_path)?, &content).map_err(ser_err)
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
        ciborium::into_writer(&content, Self::open_file(path, output_path)?).map_err(ser_err)
    }
}

/// Create the parent directory of `path` if it does not yet exist, so a
/// `--output sub/dir/report.json` to a missing directory succeeds rather
/// than failing with "No such file or directory" (issue #709).
pub(crate) fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
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
    match output {
        Some(path) => {
            ensure_parent_dir(path)?;
            std::fs::write(path, content)
        }
        None => std::io::stdout().lock().write_all(content.as_bytes()),
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
}
