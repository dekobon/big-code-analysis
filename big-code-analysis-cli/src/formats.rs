use std::fs::{File, create_dir_all};
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Serialize;

use big_code_analysis::{
    CSV_EXTENSION, FuncSpace, OffenderRecord, write_checkstyle, write_clang_warning, write_csv,
    write_msvc_warning, write_sarif,
};

pub(crate) const CBOR_STDOUT_ERROR: &str =
    "CBOR is binary and cannot be printed to stdout; use --output";

fn ser_err(e: impl std::error::Error + Send + Sync + 'static) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

/// Per-file serialization formats accepted by `bca metrics` and `bca ops`.
/// Aggregated formats (e.g. markdown) live on `bca report` instead — see
/// [`ReportFormat`]. CI/IDE formats (e.g. Checkstyle) aggregate offender
/// records across the whole walk and bypass the per-file dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub(crate) enum MetricsFormat {
    Cbor,
    Checkstyle,
    #[value(name = "clang-warning")]
    ClangWarning,
    Csv,
    Json,
    #[value(name = "msvc-warning")]
    MsvcWarning,
    Sarif,
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
    /// Single document aggregated across the whole walk; emitted
    /// after the walk completes.
    Aggregated(AggregatedFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenericFormat {
    Cbor,
    Json,
    Toml,
    Yaml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AggregatedFormat {
    Checkstyle,
    Sarif,
    ClangWarning,
    MsvcWarning,
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

impl AggregatedFormat {
    /// Human-readable name used in error messages when the writer
    /// fails.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Checkstyle => "checkstyle",
            Self::Sarif => "sarif",
            Self::ClangWarning => "clang-warning",
            Self::MsvcWarning => "msvc-warning",
        }
    }

    /// Emit a well-formed (and stable) document for the given offender
    /// records. Until the threshold engine (#96) lands, callers pass
    /// an empty slice so CI consumers can wire up the format
    /// immediately.
    pub(crate) fn dump(
        self,
        offenders: &[OffenderRecord],
        output_path: Option<&Path>,
    ) -> std::io::Result<()> {
        match self {
            Self::Checkstyle => dump_checkstyle(offenders, output_path),
            Self::Sarif => dump_sarif(offenders, output_path),
            Self::ClangWarning => dump_clang_warning(offenders, output_path),
            Self::MsvcWarning => dump_msvc_warning(offenders, output_path),
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
            Self::Json => MetricsDispatch::Generic(GenericFormat::Json),
            Self::Toml => MetricsDispatch::Generic(GenericFormat::Toml),
            Self::Yaml => MetricsDispatch::Generic(GenericFormat::Yaml),
            Self::Csv => MetricsDispatch::Csv,
            Self::Checkstyle => MetricsDispatch::Aggregated(AggregatedFormat::Checkstyle),
            Self::Sarif => MetricsDispatch::Aggregated(AggregatedFormat::Sarif),
            Self::ClangWarning => MetricsDispatch::Aggregated(AggregatedFormat::ClangWarning),
            Self::MsvcWarning => MetricsDispatch::Aggregated(AggregatedFormat::MsvcWarning),
        }
    }
}

/// Run `write` against either `path` (creating any missing parent
/// directories) or stdout. Shared scaffolding for the aggregated
/// `dump_*` helpers; the writer signature is generic over `W: Write`,
/// and `&mut dyn Write` satisfies that bound.
fn write_to_path_or_stdout<F>(output_path: Option<&Path>, write: F) -> std::io::Result<()>
where
    F: FnOnce(&mut dyn Write) -> std::io::Result<()>,
{
    if let Some(path) = output_path {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            create_dir_all(parent)?;
        }
        let mut file = File::create(path)?;
        write(&mut file)
    } else {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        write(&mut handle)
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

/// Emit a Checkstyle 4.3 XML document for `offenders`. If
/// `output_path` is `Some`, the document is written there (parent
/// directories created as needed); otherwise it goes to stdout.
///
/// Until the threshold engine (#96) lands, the CLI invokes this with
/// an empty slice so `--format checkstyle` produces a well-formed
/// (and stable) document that CI consumers can already wire up.
pub(crate) fn dump_checkstyle(
    offenders: &[OffenderRecord],
    output_path: Option<&Path>,
) -> std::io::Result<()> {
    write_to_path_or_stdout(output_path, |w| write_checkstyle(offenders, w))
}

/// Emit a SARIF 2.1.0 JSON document for `offenders`. If `output_path`
/// is `Some`, the document is written there (parent directories
/// created as needed); otherwise it goes to stdout.
///
/// Until the threshold engine (#96) lands, the CLI invokes this with
/// an empty slice so `--format sarif` produces a well-formed (and
/// stable) document that GitHub Code Scanning can already ingest.
pub(crate) fn dump_sarif(
    offenders: &[OffenderRecord],
    output_path: Option<&Path>,
) -> std::io::Result<()> {
    write_to_path_or_stdout(output_path, |w| write_sarif(offenders, w))
}

/// Emit Clang/GCC-style warning lines for `offenders`. If
/// `output_path` is `Some`, the output is written to that single
/// `.txt` file (parent directories created as needed); otherwise it
/// streams to stdout, one line per offender.
pub(crate) fn dump_clang_warning(
    offenders: &[OffenderRecord],
    output_path: Option<&Path>,
) -> std::io::Result<()> {
    write_to_path_or_stdout(output_path, |w| write_clang_warning(offenders, w))
}

/// Emit MSVC-style warning lines for `offenders`. If `output_path` is
/// `Some`, the output is written to that single `.txt` file (parent
/// directories created as needed); otherwise it streams to stdout,
/// one line per offender.
pub(crate) fn dump_msvc_warning(
    offenders: &[OffenderRecord],
    output_path: Option<&Path>,
) -> std::io::Result<()> {
    write_to_path_or_stdout(output_path, |w| write_msvc_warning(offenders, w))
}

#[inline(always)]
fn print_on_stdout(content: String) -> std::io::Result<()> {
    writeln!(std::io::stdout().lock(), "{content}")
}

trait WriteOnStdout {
    #[inline(always)]
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

fn handle_path(path: &Path, output_path: &Path, extension: &str) -> PathBuf {
    // Remove root /
    let path = path.strip_prefix("/").unwrap_or(path);

    // Remove root ./
    let path = path.strip_prefix("./").unwrap_or(path);

    // Replace .. with . to keep files inside the output folder, warn on non-UTF-8 components
    let mut cleaned = PathBuf::new();
    for component in path.iter() {
        let Some(s) = component.to_str() else {
            eprintln!(
                "Warning: non-UTF-8 path component dropped from output path: {}",
                path.display()
            );
            continue;
        };
        cleaned.push(if s == ".." { "." } else { s });
    }

    // Append the extension and build the final path
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
        serde_cbor::to_writer(Self::open_file(path, output_path)?, &content).map_err(ser_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn handle_path_replaces_dotdot_with_dot() {
        let result = handle_path(Path::new("a/../b.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/a/./b.rs.json"));
    }

    #[test]
    fn handle_path_plain_relative() {
        let result = handle_path(Path::new("src/main.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/src/main.rs.json"));
    }

    #[cfg(unix)]
    #[test]
    fn handle_path_skips_non_utf8_components() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let bad_component = OsStr::from_bytes(b"\xff\xfe");
        let path = PathBuf::from("src").join(bad_component).join("bar.rs");
        let result = handle_path(&path, Path::new("out"), ".json");
        // The non-UTF-8 component is dropped and a warning is emitted to stderr;
        // only the valid components (src, bar.rs) appear in the output path.
        assert_eq!(result, PathBuf::from("out/src/bar.rs.json"));
    }
}
