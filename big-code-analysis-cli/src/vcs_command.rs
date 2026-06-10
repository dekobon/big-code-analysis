// bca: suppress-file(halstead, nargs, nexits, nom)
// File-level halstead/nargs/exit/nom are many-fn aggregation artifacts
// (the options-builder + rank/emit/CSV plumbing), not per-function
// logic complexity (cognitive/cyclomatic stay enforced).

//! `bca vcs` — rank files by change-history (VCS) risk (issue #328).
//!
//! Unlike the AST commands, this runs **one** history walk over the
//! whole repository (never per file), reuses the global walk filters to
//! pick which tracked files to report, ranks them by composite risk
//! score, and emits one of: a human-readable table (default), a
//! rendered report page (`--format markdown|html`, see
//! [`crate::vcs_report`]), or a structured document
//! (`--format json|yaml|toml|cbor|csv`). Every format writes a single
//! file (or stdout) — a whole-repo report is one document, not the
//! per-file directory that `metrics`/`ops` emit (issue #573).

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use big_code_analysis::FuncSpace;
use big_code_analysis::vcs::{
    self, CacheConfig, Options, build_history_index_cached, hotspot, parse_timestamp, parse_window,
    score, stats,
};
use big_code_analysis::wire;

use crate::formats::{CBOR_STDOUT_ERROR, VcsFormat};
use crate::{GlobalOpts, VcsArgs, die};

/// One ranked file in the report: its repo-relative path plus the flat
/// VCS metric block.
#[derive(Debug, Serialize)]
pub(crate) struct FileEntry {
    pub(crate) path: String,
    #[serde(flatten)]
    pub(crate) vcs: wire::Vcs,
}

/// The full `bca vcs` report. The window lengths and version stamps are
/// hoisted to the top so a consumer reads them once, not per file.
#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(crate) long_window_days: u32,
    pub(crate) recent_window_days: u32,
    pub(crate) risk_score_version: u32,
    pub(crate) vcs_schema_version: u32,
    pub(crate) truncated_shallow_clone: bool,
    /// Directory- / repo-level bus factor (issue #332). Placed before
    /// `files` so the TOML serialization emits this table ahead of the
    /// `[[files]]` array; elided when the aggregate was not computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) vcs_aggregate: Option<vcs::VcsAggregate>,
    pub(crate) files: Vec<FileEntry>,
}

/// Entry point for `Command::Vcs`.
pub(crate) fn run(mut globals: GlobalOpts, args: VcsArgs) {
    // Default the walk root to the current directory so a bare
    // `bca vcs` ranks the repository the user is standing in.
    if globals.paths.is_empty() {
        globals.paths.push(PathBuf::from("."));
    }
    let root = resolve_root(&globals);

    // `bca vcs jit <commit>` and `bca vcs trend` are distinct paths; the
    // bare `bca vcs` ranking flow is the `None` case below.
    match args.command.as_ref() {
        Some(crate::VcsSubcommand::Jit(jit)) => {
            crate::vcs_jit::run(&root, &args, jit);
            return;
        }
        Some(crate::VcsSubcommand::Trend(trend)) => {
            crate::vcs_trend::run(&root, &args, trend);
            return;
        }
        None => {}
    }

    let mut options = build_options(&args);
    // The dedicated `bca vcs` report surfaces the directory/repo bus
    // factor; the per-file injection paths leave it off.
    options.compute_bus_factor = true;
    let mut cache_config = CacheConfig::default();
    cache_config.enabled = !args.no_cache;
    cache_config.clear = args.clear_cache;
    cache_config.dir.clone_from(&args.cache_dir);
    let index = build_history_index_cached(&root, &options, &cache_config)
        .unwrap_or_else(|e| die(format_args!("{e}")));
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
        vcs_aggregate: index.vcs_aggregate(),
        files: entries,
    };

    emit(&report, &args).unwrap_or_else(|e| die(format_args!("writing vcs output: {e}")));
}

/// Build a ranked change-history [`Report`] with **default** windows for
/// the `bca report --vcs` section, ranking the same tracked files the
/// walk admits, top `top` by risk. Like `bca metrics --vcs` (and unlike
/// `bca vcs`), this is an additive opt-in: outside a git working tree
/// [`default_index`] warns once and returns `None`, so the aggregated
/// report is still produced with the section simply omitted.
pub(crate) fn build_default_report(globals: &GlobalOpts, top: usize) -> Option<Report> {
    let index = default_aggregate_index(globals)?;
    let options = Options::default();
    let entries = rank(globals, &index, top, false);
    Some(Report {
        long_window_days: options.long_window_days(),
        recent_window_days: options.recent_window_days(),
        risk_score_version: score::RISK_SCORE_VERSION,
        vcs_schema_version: stats::VCS_SCHEMA_VERSION,
        truncated_shallow_clone: index.truncated_shallow_clone(),
        vcs_aggregate: index.vcs_aggregate(),
        files: entries,
    })
}

/// Build a **default-window** index with the bus-factor aggregate enabled,
/// for the `bca report --vcs` section. Mirrors [`default_index`]'s
/// additive-opt-in contract (warns and returns `None` outside a repo).
fn default_aggregate_index(globals: &GlobalOpts) -> Option<vcs::HistoryIndex> {
    let mut options = Options::default();
    options.compute_bus_factor = true;
    match build_history_index_cached(&resolve_root(globals), &options, &CacheConfig::default()) {
        Ok(index) => Some(index),
        Err(e) => {
            eprintln!("warning: --vcs: {e}; change-history metrics omitted");
            None
        }
    }
}

/// Translate [`VcsArgs`] into a backend [`Options`], dying on a bad
/// window or timestamp. Shared with the `jit` subcommand
/// ([`crate::vcs_jit`]), which reuses the same window / bot / merge /
/// rename flags.
pub(crate) fn build_options(args: &VcsArgs) -> Options {
    let long_window_secs =
        parse_window(&args.long_window).unwrap_or_else(|e| die(format_args!("{e}")));
    let recent_window_secs =
        parse_window(&args.recent_window).unwrap_or_else(|e| die(format_args!("{e}")));
    let as_of = args
        .as_of
        .as_deref()
        .map(|raw| parse_timestamp(raw).unwrap_or_else(|e| die(format_args!("{e}"))));
    // `--file-types` (or the manifest `[vcs] file_types` it was filled
    // from) replaces the default scope; an unset flag keeps `Metrics`.
    let file_types = args.file_types.as_deref().map_or_else(
        || vcs::FileTypeScope::Metrics,
        |raw| {
            raw.parse()
                .unwrap_or_else(|e| die(format_args!("--file-types: {e}")))
        },
    );

    let mut options = Options::default();
    options.long_window_secs = long_window_secs;
    options.recent_window_secs = recent_window_secs;
    options.reference.clone_from(&args.reference);
    options.full_history = args.full_history;
    options.include_merges = args.include_merges;
    options.follow_renames = !args.no_follow_renames;
    options.exclude_bots = !args.no_exclude_bots;
    options.bot_pattern = args
        .bot_pattern
        .clone()
        .unwrap_or_else(|| vcs::options::DEFAULT_BOT_PATTERN.to_owned());
    options.as_of = as_of;
    options.risk_formula = args.risk_formula.into();
    options.emit_author_details = args.emit_author_details;
    options.include_deleted = args.include_deleted;
    // The shared builder leaves the aggregate off (the `jit` subcommand never
    // wants it); the ranking flow turns it on below. `Default` already sets it
    // `false`, so no explicit assignment is needed here.
    options.bus_factor_threshold =
        vcs::options::validate_bus_factor_threshold(args.bus_factor_threshold)
            .unwrap_or_else(|e| die(format_args!("--bus-factor-threshold: {e}")));
    options.file_types = file_types;
    options
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
    // Repo-relative paths are git paths and must be emitted forward-slash
    // so the JSON / CSV / table output is byte-identical across platforms.
    // The on-disk walk in `rank` yields OS-native separators on Windows
    // (`strip_prefix` of a canonicalized path → `src\work.rs`), so
    // normalize at this single output chokepoint. Mirrors
    // `metric_diff::path_to_key`.
    path.to_str()
        .map(|s| s.replace(std::path::MAIN_SEPARATOR, "/"))
        .or_else(|| {
            eprintln!("Warning: skipping non-UTF-8 path {}", path.display());
            None
        })
}

/// Render the report in the requested format (or the default table). A
/// whole-repo change-history report is a single document, so every
/// `--output` here is one file (never a per-file directory like
/// `metrics`/`ops`); stdout when omitted.
fn emit(report: &Report, args: &VcsArgs) -> std::io::Result<()> {
    let output = args.output.as_ref();
    match args.format {
        None => write_table(report),
        Some(VcsFormat::Markdown) => {
            write_text(&crate::vcs_report::render_markdown(report), output)
        }
        Some(VcsFormat::Html) => write_text(&crate::vcs_report::render_html(report), output),
        Some(VcsFormat::Csv) => write_csv(report, output),
        Some(VcsFormat::Json) => {
            let json = if args.pretty {
                serde_json::to_string_pretty(report)
            } else {
                serde_json::to_string(report)
            }
            .map_err(std::io::Error::other)?;
            write_text(&json, output)
        }
        Some(VcsFormat::Yaml) => {
            let yaml = serde_yaml::to_string(report).map_err(std::io::Error::other)?;
            write_text(&yaml, output)
        }
        Some(VcsFormat::Toml) => {
            let toml = if args.pretty {
                toml::to_string_pretty(report)
            } else {
                toml::to_string(report)
            }
            .map_err(std::io::Error::other)?;
            write_text(&toml, output)
        }
        // CBOR is binary, so it must land in a file — never stdout.
        Some(VcsFormat::Cbor) => match output {
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                CBOR_STDOUT_ERROR,
            )),
            Some(path) => ciborium::into_writer(report, std::fs::File::create(path)?)
                .map_err(std::io::Error::other),
        },
    }
}

/// Write a rendered text document (Markdown / HTML / JSON / YAML / TOML)
/// to a single file or stdout.
fn write_text(content: &str, output: Option<&PathBuf>) -> std::io::Result<()> {
    match output {
        Some(path) => std::fs::write(path, content),
        None => std::io::stdout().lock().write_all(content.as_bytes()),
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
    if let Some(aggregate) = &report.vcs_aggregate {
        write_bus_factor(&mut out, &aggregate.bus_factor)?;
    }
    Ok(())
}

/// Append the repo-level bus factor and the per-directory breakdown to the
/// table. Ordinal-but-actionable: each number is the count of key
/// departures that would abandon more than the coverage threshold of the
/// group's files.
fn write_bus_factor(out: &mut impl Write, bf: &vcs::BusFactor) -> std::io::Result<()> {
    writeln!(
        out,
        "\nBus factor (Avelino DoA, coverage {:.2}): repo {} over {} file(s)",
        bf.coverage_threshold, bf.repo.bus_factor, bf.repo.files,
    )?;
    for dir in &bf.by_directory {
        writeln!(
            out,
            "  {:>4}  {} ({} file(s))",
            dir.group.bus_factor, dir.directory, dir.group.files,
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
        "change_entropy_long",
        "change_entropy_recent",
        "cochange_entropy_long",
        "cochange_entropy_recent",
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
            &format!("{:.4}", v.change_entropy_long),
            &format!("{:.4}", v.change_entropy_recent),
            &format!("{:.4}", v.cochange_entropy_long),
            &format!("{:.4}", v.cochange_entropy_recent),
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
    match build_history_index_cached(
        &resolve_root(globals),
        &Options::default(),
        &CacheConfig::default(),
    ) {
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
    set_hotspot_score(&mut stat, space);
    space.metrics.vcs = Some(stat);
}

/// Fill a `vcs` block's `hotspot_score` from `space`'s own cyclomatic sum
/// (complexity × recent churn). Shared by the file-level [`inject`] and
/// the per-function [`assign_child_stats`] so both compute it identically.
fn set_hotspot_score(stat: &mut vcs::Stats, space: &FuncSpace) {
    #[allow(clippy::cast_precision_loss)]
    let complexity = space.metrics.cyclomatic.cyclomatic_sum() as f64;
    stat.hotspot_score = Some(hotspot::hotspot_score(complexity, stat.churn_recent));
}

/// Build a per-function blame engine with **default** windows for
/// `bca metrics --vcs-per-function`, rooted like [`default_index`].
///
/// Mirrors [`default_index`]'s additive-opt-in contract: outside a git
/// working tree (or with an unborn `HEAD`) it warns once and returns
/// `None`, so the AST metrics still emit with the per-function `vcs`
/// blocks simply omitted. Returned in an [`Arc`] so the per-file walk
/// workers share one read-only engine (issue #329).
pub(crate) fn default_blame(globals: &GlobalOpts) -> Option<Arc<vcs::PerFunctionBlame>> {
    match vcs::PerFunctionBlame::open(&resolve_root(globals), Options::default()) {
        Ok(engine) => Some(Arc::new(engine)),
        Err(e) => {
            eprintln!("warning: --vcs-per-function: {e}; per-function change-history omitted");
            None
        }
    }
}

/// Blame `path` once and attach a `vcs` block to every nested function /
/// method / class space (issue #329). The file-level (root) space keeps
/// the file block that [`inject`] attached; only its descendants are
/// touched here.
///
/// A blame failure — an untracked file, a path outside the work tree, or
/// a genuine backend error — leaves the per-function blocks unset (the
/// file still emits its AST metrics and file-level `vcs`), so one
/// unblameable file never aborts the walk.
pub(crate) fn inject_per_function(
    space: &mut FuncSpace,
    path: &Path,
    blame: &vcs::PerFunctionBlame,
) {
    // Pre-order over descendants (the root is the file space). The same
    // traversal order is replayed in `assign_child_stats`, so the returned
    // stats line up with the spans one-to-one.
    let mut spans = Vec::new();
    collect_child_spans(space, &mut spans);
    if spans.is_empty() {
        return;
    }
    match blame.per_function(path, &spans) {
        Ok(stats) => {
            // `per_function` returns exactly one `Stats` per span, and
            // `assign_child_stats` replays the identical pre-order, so the
            // iterator must be fully consumed; a leftover means the two
            // traversals drifted out of lockstep (a bug to catch in tests).
            debug_assert_eq!(stats.len(), spans.len());
            let mut stats = stats.into_iter();
            assign_child_stats(space, &mut stats);
            debug_assert!(
                stats.next().is_none(),
                "per-function stats outnumbered the spaces they attach to"
            );
        }
        Err(e) => {
            eprintln!(
                "warning: --vcs-per-function: skipping {}: {e}",
                path.display()
            );
        }
    }
}

/// Collect the 1-based inclusive line span of every descendant space, in
/// pre-order. Saturates a span line past `u32::MAX` (no real source file
/// reaches that line count) rather than wrapping.
fn collect_child_spans(space: &FuncSpace, out: &mut Vec<vcs::LineSpan>) {
    for child in &space.spaces {
        let start = u32::try_from(child.start_line).unwrap_or(u32::MAX);
        let end = u32::try_from(child.end_line).unwrap_or(u32::MAX);
        out.push(vcs::LineSpan::new(start, end));
        collect_child_spans(child, out);
    }
}

/// Replay the [`collect_child_spans`] pre-order, attaching one blame
/// [`vcs::Stats`] to each descendant space and filling its per-function
/// `hotspot_score` from that function's own cyclomatic sum.
fn assign_child_stats(space: &mut FuncSpace, stats: &mut impl Iterator<Item = vcs::Stats>) {
    for child in &mut space.spaces {
        if let Some(mut stat) = stats.next() {
            set_hotspot_score(&mut stat, child);
            child.metrics.vcs = Some(stat);
        }
        assign_child_stats(child, stats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_to_string_normalizes_separators_to_forward_slash() {
        // A path assembled from components uses the OS separator (`\` on
        // Windows, `/` on Unix); the emitted git path must always be
        // forward-slash so the JSON / CSV / table output is byte-identical
        // cross-platform. On Windows this guards the `src\work.rs`
        // regression that failed `tests/vcs.rs` on windows-latest.
        let rel: PathBuf = ["src", "work.rs"].iter().collect();
        assert_eq!(path_to_string(&rel).as_deref(), Some("src/work.rs"));
    }
}
