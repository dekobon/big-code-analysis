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

/// Aggregated report formats accepted by `bca report`. Markdown today;
/// HTML is reserved for a future implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "lower")]
pub(crate) enum ReportFormat {
    Markdown,
}

impl MetricsFormat {
    /// Formats that aggregate offender records across the entire walk
    /// rather than emitting one document per source file. The CLI
    /// short-circuits the per-file dispatch for these.
    pub(crate) fn is_aggregated(self) -> bool {
        matches!(
            self,
            Self::Checkstyle | Self::Sarif | Self::ClangWarning | Self::MsvcWarning
        )
    }

    /// True for formats whose row shape is fixed and therefore not
    /// representable through the generic `T: Serialize` dispatch (CSV
    /// today). The Metrics action handles these on a separate code
    /// path that takes a concrete `&FuncSpace`; the Ops action
    /// rejects them at runtime since CSV columns are metric-shaped.
    pub(crate) fn requires_funcspace(self) -> bool {
        matches!(self, Self::Csv)
    }

    pub(crate) fn dump<T: Serialize>(
        self,
        space: T,
        path: PathBuf,
        output_path: Option<&PathBuf>,
        pretty: bool,
    ) -> std::io::Result<()> {
        if let Some(output_path) = output_path {
            match self {
                Self::Cbor => Cbor::with_writer(space, path, output_path),
                Self::Json => Json::with_pretty_writer(space, path, output_path, pretty),
                Self::Toml => Toml::with_pretty_writer(space, path, output_path, pretty),
                Self::Yaml => Yaml::with_writer(space, path, output_path),
                // Aggregated formats are emitted once after the walk,
                // not per file — skip silently here.
                Self::Checkstyle | Self::Sarif | Self::ClangWarning | Self::MsvcWarning => Ok(()),
                // CSV is dispatched via `dump_csv` from the Metrics
                // action; reaching this arm means the dispatcher
                // missed a case.
                Self::Csv => unreachable_csv(),
            }
        } else {
            match self {
                Self::Json => Json::write_on_stdout_pretty(space, pretty),
                Self::Toml => Toml::write_on_stdout_pretty(space, pretty),
                Self::Yaml => Yaml::write_on_stdout(space),
                Self::Cbor => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    CBOR_STDOUT_ERROR,
                )),
                Self::Checkstyle | Self::Sarif | Self::ClangWarning | Self::MsvcWarning => Ok(()),
                Self::Csv => unreachable_csv(),
            }
        }
    }
}

fn unreachable_csv() -> std::io::Result<()> {
    Err(std::io::Error::other(
        "internal error: CSV format must be dispatched via dump_csv, not dump",
    ))
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
    if let Some(output_path) = output_path {
        let format_path = handle_path(path.clone(), output_path, CSV_EXTENSION);
        if let Some(parent) = format_path.parent() {
            create_dir_all(parent)?;
        }
        write_csv(space, &path, File::create(format_path)?)
    } else {
        write_csv(space, &path, std::io::stdout().lock())
    }
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
    if let Some(path) = output_path {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            create_dir_all(parent)?;
        }
        write_checkstyle(offenders, File::create(path)?)
    } else {
        write_checkstyle(offenders, std::io::stdout().lock())
    }
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
    if let Some(path) = output_path {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            create_dir_all(parent)?;
        }
        write_sarif(offenders, File::create(path)?)
    } else {
        write_sarif(offenders, std::io::stdout().lock())
    }
}

/// Emit Clang/GCC-style warning lines for `offenders`. If
/// `output_path` is `Some`, the output is written to that single
/// `.txt` file (parent directories created as needed); otherwise it
/// streams to stdout, one line per offender.
pub(crate) fn dump_clang_warning(
    offenders: &[OffenderRecord],
    output_path: Option<&Path>,
) -> std::io::Result<()> {
    if let Some(path) = output_path {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            create_dir_all(parent)?;
        }
        write_clang_warning(offenders, File::create(path)?)
    } else {
        write_clang_warning(offenders, std::io::stdout().lock())
    }
}

/// Emit MSVC-style warning lines for `offenders`. If `output_path` is
/// `Some`, the output is written to that single `.txt` file (parent
/// directories created as needed); otherwise it streams to stdout,
/// one line per offender.
pub(crate) fn dump_msvc_warning(
    offenders: &[OffenderRecord],
    output_path: Option<&Path>,
) -> std::io::Result<()> {
    if let Some(path) = output_path {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            create_dir_all(parent)?;
        }
        write_msvc_warning(offenders, File::create(path)?)
    } else {
        write_msvc_warning(offenders, std::io::stdout().lock())
    }
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

fn handle_path(path: PathBuf, output_path: &Path, extension: &str) -> PathBuf {
    // Remove root /
    let path = path.as_path().strip_prefix("/").unwrap_or(path.as_path());

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

    fn open_file(path: PathBuf, output_path: &Path) -> std::io::Result<File> {
        let format_path = handle_path(path, output_path, Self::EXTENSION);
        if let Some(parent) = format_path.parent() {
            create_dir_all(parent)?;
        }
        File::create(format_path)
    }

    fn with_writer<T: Serialize>(
        content: T,
        path: PathBuf,
        output_path: &Path,
    ) -> std::io::Result<()>;
}

trait WritePrettyFile: WriteFile {
    fn with_pretty_writer<T: Serialize>(
        content: T,
        path: PathBuf,
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
        path: PathBuf,
        output_path: &Path,
    ) -> std::io::Result<()> {
        serde_json::to_writer(Self::open_file(path, output_path)?, &content).map_err(ser_err)
    }
}

impl WritePrettyFile for Json {
    fn with_pretty_writer<T: Serialize>(
        content: T,
        path: PathBuf,
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
        path: PathBuf,
        output_path: &Path,
    ) -> std::io::Result<()> {
        Self::open_file(path, output_path)?.write_all(Self::format(content)?.as_bytes())
    }
}

impl WritePrettyFile for Toml {
    fn with_pretty_writer<T: Serialize>(
        content: T,
        path: PathBuf,
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
        path: PathBuf,
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
        path: PathBuf,
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
        let result = handle_path(PathBuf::from("/foo/bar.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/foo/bar.rs.json"));
    }

    #[test]
    fn handle_path_strips_dot_slash() {
        let result = handle_path(PathBuf::from("./foo/bar.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/foo/bar.rs.json"));
    }

    #[test]
    fn handle_path_replaces_dotdot_with_dot() {
        let result = handle_path(PathBuf::from("a/../b.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/a/./b.rs.json"));
    }

    #[test]
    fn handle_path_plain_relative() {
        let result = handle_path(PathBuf::from("src/main.rs"), Path::new("out"), ".json");
        assert_eq!(result, PathBuf::from("out/src/main.rs.json"));
    }

    #[cfg(unix)]
    #[test]
    fn handle_path_skips_non_utf8_components() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let bad_component = OsStr::from_bytes(b"\xff\xfe");
        let path = PathBuf::from("src").join(bad_component).join("bar.rs");
        let result = handle_path(path, Path::new("out"), ".json");
        // The non-UTF-8 component is dropped and a warning is emitted to stderr;
        // only the valid components (src, bar.rs) appear in the output path.
        assert_eq!(result, PathBuf::from("out/src/bar.rs.json"));
    }
}
