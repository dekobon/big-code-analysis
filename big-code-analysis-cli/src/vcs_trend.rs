//! `bca vcs trend` — sample the change-history metrics at several points
//! in time and emit a per-file time series (issue #333).
//!
//! Unlike `bca vcs` (a single ranked snapshot at one ref), this re-anchors
//! at the mainline tip of each sampled moment and emits one nested
//! structured document: `as_of_points`, per-file point arrays (`null`
//! where the file did not exist), and an improving/regressing delta
//! summary. The window / bot / merge / rename / `--ref` / `--as-of` flags
//! and `--top` come from the parent `bca vcs` options.

use std::path::Path;

use big_code_analysis::vcs::{build_trend, parse_window};
use big_code_analysis::wire;

use crate::formats::{CBOR_STDOUT_ERROR, TrendFormat, write_buffered_file, write_text};
use crate::{TrendArgs, VcsArgs, die, warn};

/// Entry point for `bca vcs trend`. `root` is the repository-discovery
/// seed already resolved by [`crate::vcs_command::run`]; `args` carries the
/// shared window / bot / merge / rename / ref / as-of / top flags, `trend`
/// the trend-only ones.
pub(crate) fn run(root: &Path, args: &VcsArgs, trend: &TrendArgs) {
    let base = crate::vcs_command::build_options(args);
    let span_secs = parse_window(&trend.span).unwrap_or_else(|e| die(format_args!("--span: {e}")));

    let result = build_trend(root, &base, trend.points, span_secs)
        .unwrap_or_else(|e| die(format_args!("{e}")));
    if result.truncated_shallow_clone() {
        warn("shallow clone detected — history is truncated, so counts are lower bounds");
    }

    // `args.top` (parent `--top`) keeps the riskiest files; `top_deltas`
    // trims each delta list.
    let report = wire::VcsTrend::from_trend(&result, args.top, trend.top_deltas);
    emit(&report, trend).unwrap_or_else(|e| die(format_args!("writing vcs trend output: {e}")));
}

/// Serialize the trend in the requested structured format to a single file
/// or stdout (CBOR to a file only — it is binary).
fn emit(report: &wire::VcsTrend, trend: &TrendArgs) -> std::io::Result<()> {
    let output = trend.output.as_ref();
    match trend.format {
        TrendFormat::Json => {
            let json = if trend.pretty {
                serde_json::to_string_pretty(report)
            } else {
                serde_json::to_string(report)
            }
            .map_err(std::io::Error::other)?;
            write_text(&json, output)
        }
        TrendFormat::Yaml => {
            let yaml = serde_yaml::to_string(report).map_err(std::io::Error::other)?;
            write_text(&yaml, output)
        }
        TrendFormat::Cbor => match output {
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                CBOR_STDOUT_ERROR,
            )),
            Some(path) => write_buffered_file(path, |w| {
                ciborium::into_writer(report, w).map_err(std::io::Error::other)
            }),
        },
    }
}
