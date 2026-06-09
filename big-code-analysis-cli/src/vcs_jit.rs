// bca: suppress-file(halstead, nargs)
// File-level halstead/nargs are many-fn aggregation artifacts (the
// format-dispatch `emit` match + the small write helpers), not
// per-function logic complexity (cognitive/cyclomatic stay enforced) —
// mirrors the sibling `vcs_command.rs`.

//! `bca vcs jit` — score a single commit (or an arbitrary diff) for
//! just-in-time (commit-level) defect-induction risk (issues #331 / #580).
//!
//! Unlike `bca vcs` (which ranks files at a ref), this scores one commit
//! against its first parent and emits a single structured document: the
//! size / diffusion / history / experience features, their per-group
//! contributions, and the ordinal composite score. `--fail-over` turns
//! it into a CI gate.
//!
//! With `--diff <file>` (or `--diff -` for stdin) it instead scores an
//! arbitrary unified diff. A bare diff carries no author / parent /
//! history, so only the size and diffusion groups are computable: the
//! result is a deliberately partial `JitDiffReport` whose other groups are
//! absent (not zero) and whose score is **not comparable** to a commit
//! score.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;

use big_code_analysis::vcs::{score_commit, score_diff};
use serde::Serialize;

use crate::formats::{CBOR_STDOUT_ERROR, JitFormat};
use crate::{JitArgs, VcsArgs, die};

/// Entry point for `bca vcs jit`. `root` is the repository-discovery
/// seed already resolved by [`crate::vcs_command::run`]; `args` carries
/// the shared window / bot / merge / rename flags, `jit` the jit-only
/// ones. Dispatches to the commit path or — when `--diff` is given — the
/// arbitrary-diff path.
pub(crate) fn run(root: &Path, args: &VcsArgs, jit: &JitArgs) {
    if let Some(diff_source) = &jit.diff {
        run_diff(diff_source, jit);
    } else {
        run_commit(root, args, jit);
    }
}

/// Commit path: score the resolved revision and gate on the composite
/// score.
fn run_commit(root: &Path, args: &VcsArgs, jit: &JitArgs) {
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

/// Arbitrary-diff path (issue #580): read the unified diff from a file (or
/// stdin when `source` is `-`), score its size / diffusion groups only, and
/// gate on the *partial* score. The partial score is not comparable to a
/// commit score, but the same `--fail-over` mechanics still let a hook gate
/// a raw diff against a diff-calibrated threshold.
fn run_diff(source: &Path, jit: &JitArgs) {
    let diff = read_diff(source).unwrap_or_else(|e| die(format_args!("reading diff: {e}")));
    let report = score_diff(&diff).unwrap_or_else(|e| die(format_args!("{e}")));

    emit(&report, jit).unwrap_or_else(|e| die(format_args!("writing jit output: {e}")));

    if let Some(threshold) = jit.fail_over
        && report.partial_score >= threshold
    {
        eprintln!(
            "vcs jit: partial diff score {:.4} >= fail-over threshold {threshold:.4}",
            report.partial_score
        );
        process::exit(2);
    }
}

/// Read the unified diff from `source`, treating the single path `-` as
/// stdin (the conventional CLI marker).
fn read_diff(source: &Path) -> std::io::Result<String> {
    if source.as_os_str() == "-" {
        let mut buf = String::new();
        std::io::stdin().lock().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        std::fs::read_to_string(source)
    }
}

/// Serialize a report in the requested structured format to a single
/// file or stdout (CBOR to a file only — it is binary). Generic over the
/// commit (`JitReport`) and diff (`JitDiffReport`) shapes so both modes
/// share one format-dispatch path.
fn emit<R: Serialize>(report: &R, jit: &JitArgs) -> std::io::Result<()> {
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
