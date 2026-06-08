// bca: suppress-file(halstead, nargs, exit, nom)
// File-level halstead/nargs/exit/nom are many-fn aggregation artifacts
// (the options-builder + rank/emit/CSV plumbing), not per-function
// logic complexity (cognitive/cyclomatic stay enforced).

//! `bca vcs` — rank files by change-history (VCS) risk (issue #328).
//!
//! Unlike the AST commands, this runs **one** history walk over the
//! whole repository (never per file), reuses the global walk filters to
//! pick which tracked files to report, ranks them by composite risk
//! score, and emits either a human-readable table (default) or a
//! structured document (`--format json|yaml|toml|cbor|csv`).

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use big_code_analysis::FuncSpace;
use big_code_analysis::vcs::{
    self, Options, build_history_index, hotspot, parse_timestamp, parse_window, score, stats,
};
use big_code_analysis::wire;

use crate::die;
use crate::formats::{CBOR_STDOUT_ERROR, MetricsDispatch, MetricsFormat};
use crate::{GlobalOpts, VcsArgs};

/// One ranked file in the report: its repo-relative path plus the flat
/// VCS metric block.
#[derive(Debug, Serialize)]
struct FileEntry {
    path: String,
    #[serde(flatten)]
    vcs: wire::Vcs,
}

/// The full `bca vcs` report. The window lengths and version stamps are
/// hoisted to the top so a consumer reads them once, not per file.
#[derive(Debug, Serialize)]
struct Report {
    long_window_days: u32,
    recent_window_days: u32,
    risk_score_version: u32,
    vcs_schema_version: u32,
    truncated_shallow_clone: bool,
    files: Vec<FileEntry>,
}

/// Entry point for `Command::Vcs`.
pub(crate) fn run(mut globals: GlobalOpts, args: VcsArgs) {
    let options = build_options(&args);

    // Default the walk root to the current directory so a bare
    // `bca vcs` ranks the repository the user is standing in.
    if globals.paths.is_empty() {
        globals.paths.push(PathBuf::from("."));
    }
    let root = resolve_root(&globals);

    let index = build_history_index(&root, &options).unwrap_or_else(|e| die(format_args!("{e}")));
    if index.truncated_shallow_clone() {
        eprintln!(
            "Warning: shallow clone detected — history is truncated, so counts are lower bounds"
        );
    }

    let entries = rank(&globals, &index, args.top, args.include_deleted);
    let report = Report {
        long_window_days: options.long_window_days(),
        recent_window_days: options.recent_window_days(),
        risk_score_version: score::RISK_SCORE_VERSION,
        vcs_schema_version: stats::VCS_SCHEMA_VERSION,
        truncated_shallow_clone: index.truncated_shallow_clone(),
        files: entries,
    };

    emit(&report, &args).unwrap_or_else(|e| die(format_args!("writing vcs output: {e}")));
}

/// Translate [`VcsArgs`] into a backend [`Options`], dying on a bad
/// window or timestamp.
fn build_options(args: &VcsArgs) -> Options {
    let long_window_secs =
        parse_window(&args.long_window).unwrap_or_else(|e| die(format_args!("{e}")));
    let recent_window_secs =
        parse_window(&args.recent_window).unwrap_or_else(|e| die(format_args!("{e}")));
    let as_of = args
        .as_of
        .as_deref()
        .map(|raw| parse_timestamp(raw).unwrap_or_else(|e| die(format_args!("{e}"))));

    Options {
        long_window_secs,
        recent_window_secs,
        reference: args.reference.clone(),
        full_history: args.full_history,
        include_merges: args.include_merges,
        follow_renames: !args.no_follow_renames,
        exclude_bots: !args.no_exclude_bots,
        bot_pattern: args
            .bot_pattern
            .clone()
            .unwrap_or_else(|| vcs::options::DEFAULT_BOT_PATTERN.to_owned()),
        as_of,
        risk_formula: args.risk_formula.into(),
        emit_author_details: args.emit_author_details,
        include_deleted: args.include_deleted,
    }
}

/// Select the tracked files the global filters admit, sort them by risk
/// score (descending; path as a stable tie-break), and truncate to the
/// top `top` (0 = all).
fn rank(
    globals: &GlobalOpts,
    index: &vcs::HistoryIndex,
    top: usize,
    include_deleted: bool,
) -> Vec<FileEntry> {
    // Reuse the standard walk so `--paths/--include/--exclude/--no-ignore`
    // behave exactly as elsewhere; intersect the result with the tracked
    // set (untracked / binary files are simply absent from the index).
    let (selected, _jobs) = crate::resolve_walk_files(globals.clone());

    let mut covered: HashSet<PathBuf> = HashSet::new();
    let mut entries: Vec<FileEntry> = selected
        .iter()
        .filter_map(|abs| lookup(index, abs))
        .filter_map(|(rel, stat)| {
            let path = path_to_string(&rel)?;
            covered.insert(rel.clone());
            Some(FileEntry {
                path,
                vcs: wire::Vcs::from(stat),
            })
        })
        .collect();

    // Files deleted at the target ref never appear in the on-disk walk,
    // so pull them straight from the index when opted in.
    if include_deleted {
        append_deleted_entries(&mut entries, &covered, globals, index);
    }

    vcs::rank_by_risk(&mut entries, top, |e| (e.path.as_str(), e.vcs.risk_score));
    entries
}

/// Append entries for files present in the index but absent from the
/// on-disk walk (deleted at the target ref), filtered to the requested
/// `--paths` prefixes. The index carries deleted files only when the
/// walk ran with `include_deleted`.
fn append_deleted_entries(
    entries: &mut Vec<FileEntry>,
    covered: &HashSet<PathBuf>,
    globals: &GlobalOpts,
    index: &vcs::HistoryIndex,
) {
    let prefixes = repo_relative_prefixes(globals, index);
    for (rel, stat) in index.iter() {
        if covered.contains(rel) || !under_any_prefix(rel, &prefixes) {
            continue;
        }
        if let Some(path) = path_to_string(rel) {
            entries.push(FileEntry {
                path,
                vcs: wire::Vcs::from(stat),
            });
        }
    }
}

/// Resolve the `--paths` seeds to repo-relative prefixes for filtering
/// deleted-file entries. An empty result means "no usable prefix"; an
/// empty `PathBuf` prefix (the repo root, e.g. from `.`) matches every
/// entry.
fn repo_relative_prefixes(globals: &GlobalOpts, index: &vcs::HistoryIndex) -> Vec<PathBuf> {
    let Some(workdir) = index.workdir() else {
        return Vec::new();
    };
    globals
        .paths
        .iter()
        .filter_map(|seed| {
            let canonical = seed.canonicalize().ok()?;
            canonical.strip_prefix(workdir).ok().map(Path::to_path_buf)
        })
        .collect()
}

/// Whether `rel` sits under any of the repo-relative `prefixes`.
fn under_any_prefix(rel: &Path, prefixes: &[PathBuf]) -> bool {
    prefixes.iter().any(|prefix| rel.starts_with(prefix))
}

/// Map a walk-selected filesystem path to its index entry, returning the
/// repo-relative path alongside the stats. Paths are canonicalised so a
/// relative seed (`./src/x.rs`) still strips the work-tree prefix.
fn lookup<'a>(index: &'a vcs::HistoryIndex, abs: &Path) -> Option<(PathBuf, &'a vcs::Stats)> {
    let canonical = abs.canonicalize().ok()?;
    let workdir = index.workdir()?;
    let rel = canonical.strip_prefix(workdir).ok()?;
    index.get(rel).map(|stat| (rel.to_path_buf(), stat))
}

/// Convert a repo-relative path to a UTF-8 string for output, warning
/// and skipping (rather than lossily mangling) a non-UTF-8 path used as
/// an output identifier — per the path rules in AGENTS.md.
fn path_to_string(path: &Path) -> Option<String> {
    path.to_str().map(str::to_owned).or_else(|| {
        eprintln!("Warning: skipping non-UTF-8 path {}", path.display());
        None
    })
}

/// Render the report in the requested format (or the default table).
fn emit(report: &Report, args: &VcsArgs) -> std::io::Result<()> {
    match args.structured.output_format {
        None => write_table(report),
        Some(MetricsFormat::Csv) => write_csv(report, args.structured.output.as_ref()),
        Some(MetricsFormat::Cbor) if args.structured.output.is_none() => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            CBOR_STDOUT_ERROR,
        )),
        Some(other) => match other.dispatch() {
            MetricsDispatch::Generic(generic) => generic.dump(
                report,
                PathBuf::from("vcs"),
                args.structured.output.as_ref(),
                args.structured.pretty,
            ),
            // Csv handled above; Cbor-to-stdout rejected above.
            MetricsDispatch::Csv => unreachable!("csv handled before dispatch"),
        },
    }
}

/// A human-readable ranked table to stdout — the default output and the
/// successor to the prototype's ranked list.
fn write_table(report: &Report) -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if report.files.is_empty() {
        return writeln!(out, "No tracked files matched.");
    }
    writeln!(
        out,
        "Change-history risk (long window {}d, recent {}d, formula v{})",
        report.long_window_days, report.recent_window_days, report.risk_score_version
    )?;
    writeln!(
        out,
        "{:>5}  {:>8}  {:>11}  {:>11}  {:>7}  FILE",
        "RANK", "RISK", "COMMITS r/l", "CHURN r/l", "AUTHORS"
    )?;
    for (rank, entry) in report.files.iter().enumerate() {
        let v = &entry.vcs;
        writeln!(
            out,
            "{:>5}  {:>8.1}  {:>11}  {:>11}  {:>7}  {}",
            rank + 1,
            v.risk_score,
            format!("{}/{}", v.commits_recent, v.commits_long),
            format!("{}/{}", v.churn_recent, v.churn_long),
            v.authors_long,
            entry.path,
        )?;
    }
    Ok(())
}

/// CSV output: one flat row per file. Written by hand (rather than
/// serde) because the `#[serde(flatten)]` shape isn't representable in
/// the `csv` crate's record model.
fn write_csv(report: &Report, output: Option<&PathBuf>) -> std::io::Result<()> {
    let sink: Box<dyn Write> = match output {
        Some(path) => Box::new(std::fs::File::create(path)?),
        None => Box::new(std::io::stdout().lock()),
    };
    let mut wtr = csv::Writer::from_writer(sink);
    wtr.write_record([
        "path",
        "risk_score",
        "commits_long",
        "commits_recent",
        "churn_long",
        "churn_recent",
        "authors_long",
        "authors_recent",
        "ownership_top_share",
        "burst",
        "bug_fix_commits",
        "security_fix_commits",
        "revert_commits",
        "age_days",
        "last_modified_days",
        "hotspot_score",
    ])
    .map_err(csv_err)?;
    for entry in &report.files {
        let v = &entry.vcs;
        wtr.write_record([
            entry.path.as_str(),
            &format!("{:.4}", v.risk_score),
            &v.commits_long.to_string(),
            &v.commits_recent.to_string(),
            &v.churn_long.to_string(),
            &v.churn_recent.to_string(),
            &v.authors_long.to_string(),
            &v.authors_recent.to_string(),
            &format!("{:.4}", v.ownership_top_share),
            &format!("{:.4}", v.burst),
            &v.bug_fix_commits.to_string(),
            &v.security_fix_commits.to_string(),
            &v.revert_commits.to_string(),
            &v.age_days.to_string(),
            &v.last_modified_days.to_string(),
            &v.hotspot_score
                .map(|h| format!("{h:.4}"))
                .unwrap_or_default(),
        ])
        .map_err(csv_err)?;
    }
    wtr.flush()
}

/// Map a `csv` error into an `io::Error` for the unified `emit` result.
fn csv_err(error: csv::Error) -> std::io::Error {
    std::io::Error::other(error)
}

/// Build a change-history index with **default** windows for
/// `bca metrics --vcs`, rooted at the first `--paths` seed (or the
/// current directory). Window / formula tuning is reserved for the
/// dedicated `bca vcs` command.
///
/// Unlike `bca vcs` (which errors outside a repository), `--vcs` is an
/// *additive opt-in* on the AST walk: when there is no git working tree
/// (or `HEAD` is unborn), it warns once and returns `None` so the rest
/// of the metrics output is still produced with the `vcs` block simply
/// omitted (issue #328; matches the Python `analyze(vcs=True)`
/// behaviour). Returned in an [`Arc`] so the per-file walk workers
/// share one read-only index.
pub(crate) fn default_index(globals: &GlobalOpts) -> Option<Arc<vcs::HistoryIndex>> {
    match build_history_index(&resolve_root(globals), &Options::default()) {
        Ok(index) => Some(Arc::new(index)),
        Err(e) => {
            eprintln!("warning: --vcs: {e}; change-history metrics omitted");
            None
        }
    }
}

/// Resolve the repository-discovery seed to a directory: `gix::discover`
/// rejects a file path, so a file seed (`--paths src/foo.rs`) is mapped
/// to its parent. Defaults to the current directory.
///
/// Only the **first** seed is used to discover the repository, so a
/// multi-repo invocation (`bca vcs ~/repoA ~/repoB`) indexes `repoA`
/// only; files under a second, different working tree fall outside the
/// single index and are simply absent from the ranking (omitted, never
/// misattributed). Whole-repo single-root analysis is the intended use.
fn resolve_root(globals: &GlobalOpts) -> PathBuf {
    let seed = globals
        .paths
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    if seed.is_file() {
        seed.parent()
            .map(Path::to_path_buf)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        seed
    }
}

/// Attach the file's change-history block to its file-level metrics,
/// filling in the hotspot score from the cyclomatic sum already on the
/// space. A file with no index entry (untracked / binary) is left
/// untouched, so its `vcs` field stays `None`.
pub(crate) fn inject(space: &mut FuncSpace, path: &Path, index: &vcs::HistoryIndex) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let Some(stat) = index.get_for_path(&canonical) else {
        return;
    };
    let mut stat = stat.clone();
    #[allow(clippy::cast_precision_loss)]
    let complexity = space.metrics.cyclomatic.cyclomatic_sum() as f64;
    stat.hotspot_score = Some(hotspot::hotspot_score(complexity, stat.churn_recent));
    space.metrics.vcs = Some(stat);
}
