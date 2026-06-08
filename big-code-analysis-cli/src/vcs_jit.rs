// bca: suppress-file(halstead, nargs)
// File-level halstead/nargs are many-fn aggregation artifacts (the
// format-dispatch `emit` match + the small write helpers), not
// per-function logic complexity (cognitive/cyclomatic stay enforced) —
// mirrors the sibling `vcs_command.rs`.

//! `bca vcs jit` — score a single commit for just-in-time (commit-level)
//! defect-induction risk (issue #331).
//!
//! Unlike `bca vcs` (which ranks files at a ref), this scores one commit
//! against its first parent and emits a single structured document: the
//! size / diffusion / history / experience features, their per-group
//! contributions, and the ordinal composite score. `--fail-over` turns
//! it into a CI gate.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;

use big_code_analysis::vcs::{self, score_commit};

use crate::formats::{CBOR_STDOUT_ERROR, JitFormat};
use crate::{JitArgs, VcsArgs, die};

/// Entry point for `bca vcs jit`. `root` is the repository-discovery
/// seed already resolved by [`crate::vcs_command::run`]; `args` carries
/// the shared window / bot / merge / rename flags, `jit` the jit-only
/// ones.
pub(crate) fn run(root: &Path, args: &VcsArgs, jit: &JitArgs) {
    let options = crate::vcs_command::build_options(args);
    let report =
        score_commit(root, &jit.commit, &options).unwrap_or_else(|e| die(format_args!("{e}")));

    emit(&report, jit).unwrap_or_else(|e| die(format_args!("writing jit output: {e}")));

    // CI gate: a score at or above the threshold exits 2 (the `check`
    // "metric gate" convention; exit 1 stays reserved for tool errors).
    // Done after emitting so the breakdown is still available to the gate.
    if let Some(threshold) = jit.fail_over
        && report.score >= threshold
    {
        eprintln!(
            "vcs jit: score {:.4} >= fail-over threshold {threshold:.4} for {}",
            report.score, report.commit.id
        );
        process::exit(2);
    }
}

/// Serialize the report in the requested structured format to a single
/// file or stdout (CBOR to a file only — it is binary).
fn emit(report: &vcs::JitReport, jit: &JitArgs) -> std::io::Result<()> {
    let output = jit.output.as_ref();
    match jit.format {
        JitFormat::Json => {
            let json = if jit.pretty {
                serde_json::to_string_pretty(report)
            } else {
                serde_json::to_string(report)
            }
            .map_err(std::io::Error::other)?;
            write_text(&json, output)
        }
        JitFormat::Yaml => {
            let yaml = serde_yaml::to_string(report).map_err(std::io::Error::other)?;
            write_text(&yaml, output)
        }
        JitFormat::Toml => {
            let toml = if jit.pretty {
                toml::to_string_pretty(report)
            } else {
                toml::to_string(report)
            }
            .map_err(std::io::Error::other)?;
            write_text(&toml, output)
        }
        JitFormat::Cbor => match output {
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                CBOR_STDOUT_ERROR,
            )),
            Some(path) => ciborium::into_writer(report, std::fs::File::create(path)?)
                .map_err(std::io::Error::other),
        },
    }
}

/// Write a rendered text document to a single file or stdout.
fn write_text(content: &str, output: Option<&PathBuf>) -> std::io::Result<()> {
    match output {
        Some(path) => std::fs::write(path, content),
        None => std::io::stdout().lock().write_all(content.as_bytes()),
    }
}
