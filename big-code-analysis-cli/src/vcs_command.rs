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

use crate::formats::{CBOR_STDOUT_ERROR, VcsFormat, ensure_parent_dir, write_text};
use crate::{GlobalOpts, VcsArgs, die, warn};

/// One ranked file in the report: its repo-relative path plus the VCS
/// metric block, nested under a `vcs` key like every other metric group
/// (issue #684).
#[derive(Debug, Serialize)]
pub(crate) struct FileEntry {
    pub(crate) path: String,
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

    // `bca vcs commit <commit>` and `bca vcs trend` are distinct paths;
    // the bare `bca vcs` ranking flow is the `None` case below.
    match args.command.as_ref() {
        Some(crate::VcsSubcommand::Commit(jit)) => {
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
        warn("shallow clone detected — history is truncated, so counts are lower bounds");
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
pub(crate) fn build_default_report(globals: &GlobalOpts, top: usize) -> Option<DefaultReport> {
    let index = default_aggregate_index(globals)?;
    let options = Options::default();
    let entries = rank(globals, &index, top, false);
    let report = Report {
        long_window_days: options.long_window_days(),
        recent_window_days: options.recent_window_days(),
        risk_score_version: score::RISK_SCORE_VERSION,
        vcs_schema_version: stats::VCS_SCHEMA_VERSION,
        truncated_shallow_clone: index.truncated_shallow_clone(),
        vcs_aggregate: index.vcs_aggregate(),
        files: entries,
    };
    Some(DefaultReport {
        report,
        workdir: index.workdir().map(Path::to_path_buf),
    })
}

/// A `report --vcs` change-history [`Report`] plus the repository work-tree
/// it was indexed from. The work-tree is retained so [`join_hotspot_scores`]
/// can canonicalize the AST walk's absolute file paths to the same
/// repo-relative keys [`FileEntry::path`] carries — the join that fills the
/// hotspot column (issue #615).
pub(crate) struct DefaultReport {
    pub(crate) report: Report,
    workdir: Option<PathBuf>,
}

impl DefaultReport {
    /// Join the per-file hotspot scores from the AST walk onto the
    /// change-history rows, computing each file's `complexity × recent
    /// churn` exactly as `bca metrics --vcs` does ([`set_hotspot_score`]).
    ///
    /// `cyclomatic_sums` is the stream the report walk emits: `(absolute
    /// file path, file-level cyclomatic sum)`. Each absolute path is
    /// canonicalized and stripped against the index work-tree — the same
    /// match [`inject`] performs — so a row gets its score regardless of
    /// `--strip-prefix` or how the walk root was spelled. Files with no
    /// matching change-history row (untracked / outside the work-tree)
    /// are silently skipped; rows with no matching AST file (a tracked
    /// non-source file, e.g. a deleted entry) keep `hotspot_score == None`.
    pub(crate) fn join_hotspot_scores(&mut self, cyclomatic_sums: &[(PathBuf, f64)]) {
        let Some(workdir) = self.workdir.as_deref() else {
            return;
        };
        // Index the change-history rows by repo-relative path for an
        // O(files) join rather than a quadratic scan per source file. Keys
        // are owned so the map does not borrow `files` while we mutate it.
        let mut by_path: std::collections::HashMap<String, usize> = self
            .report
            .files
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path.clone(), i))
            .collect();
        for (abs, cyclomatic_sum) in cyclomatic_sums {
            let Some(rel) = repo_relative(abs, workdir).and_then(|rel| path_to_string(&rel)) else {
                continue;
            };
            if let Some(idx) = by_path.remove(rel.as_str()) {
                let entry = &mut self.report.files[idx];
                entry.vcs.hotspot_score = Some(hotspot::hotspot_score(
                    *cyclomatic_sum,
                    entry.vcs.churn_recent,
                ));
            }
        }
    }
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
            warn_vcs_unavailable(&e);
            None
        }
    }
}

/// Translate [`VcsArgs`] into a backend [`Options`], dying on a bad
/// window or timestamp. Shared with the `jit` subcommand
/// ([`crate::vcs_jit`]), which reuses the same window / bot / merge /
/// rename flags.
pub(crate) fn build_options(args: &VcsArgs) -> Options {
    // Name the failing flag so a long CI invocation with several window
    // flags points at the offender rather than just echoing the parser
    // error (issue #607).
    let long_window_secs =
        parse_window(&args.long_window).unwrap_or_else(|e| die(format_args!("--long-window: {e}")));
    let recent_window_secs = parse_window(&args.recent_window)
        .unwrap_or_else(|e| die(format_args!("--recent-window: {e}")));
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
    // `--ref` is optional so an explicit value is distinguishable from the
    // default (the `jit` conflict check in `vcs_jit` relies on that); the
    // `HEAD` default is applied here, the single point of use.
    options.reference = args.reference.clone().unwrap_or_else(|| "HEAD".to_owned());
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
    // `build_options` (the shared builder) leaves `compute_bus_factor` at its
    // `Default` (`false`); the ranking callers `run` / `default_aggregate_index`
    // set it `true`, so no explicit assignment is needed here.
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
    let (resolved, _jobs) = crate::resolve_walk_files(globals.clone());
    let selected = resolved.files;

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

/// Canonicalize a walk-emitted path and strip the work-tree prefix,
/// yielding the repo-relative path the index keys on. `None` for paths
/// that vanished since the walk or live outside the work-tree. Single
/// home for this match so [`lookup`] and [`Report::join_hotspot_scores`]
/// cannot drift on symlink/relative-seed handling.
fn repo_relative(abs: &Path, workdir: &Path) -> Option<PathBuf> {
    let canonical = abs.canonicalize().ok()?;
    canonical.strip_prefix(workdir).ok().map(Path::to_path_buf)
}

/// Map a walk-selected filesystem path to its index entry, returning the
/// repo-relative path alongside the stats. Paths are canonicalised so a
/// relative seed (`./src/x.rs`) still strips the work-tree prefix.
fn lookup<'a>(index: &'a vcs::HistoryIndex, abs: &Path) -> Option<(PathBuf, &'a vcs::Stats)> {
    let rel = repo_relative(abs, index.workdir()?)?;
    index.get(&rel).map(|stat| (rel.clone(), stat))
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
            warn(format_args!("skipping non-UTF-8 path {}", path.display()));
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
        // The human table is the default (no `--format`) and is also
        // explicitly selectable via `--format text` (#659).
        None | Some(VcsFormat::Text) => write_table(report),
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
            Some(path) => {
                ensure_parent_dir(path)?;
                ciborium::into_writer(report, std::fs::File::create(path)?)
                    .map_err(std::io::Error::other)
            }
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
    // `r/l` spelled out to the recent/long vocabulary the rendered reports
    // now use (issue #592): the two paired columns are the recent and long
    // windows, matching the `*_recent` / `*_long` wire keys. The Authors
    // column shows the long-window count.
    writeln!(
        out,
        "{:>5}  {:>8}  {:>16}  {:>14}  {:>12}  FILE",
        "RANK", "RISK", "COMMITS rec/long", "CHURN rec/long", "AUTHORS long"
    )?;
    for (rank, entry) in report.files.iter().enumerate() {
        let v = &entry.vcs;
        writeln!(
            out,
            "{:>5}  {:>8.1}  {:>16}  {:>14}  {:>12}  {}",
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
        Some(path) => {
            ensure_parent_dir(path)?;
            Box::new(std::fs::File::create(path)?)
        }
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
            warn_vcs_unavailable(&e);
            None
        }
    }
}

/// One-line stderr notice for the additive `--vcs` opt-in when the
/// change-history index cannot be built; shared by both index builders
/// so the wording cannot drift between `report --vcs` and
/// `metrics --vcs`.
fn warn_vcs_unavailable(e: &vcs::Error) {
    warn(format_args!("--vcs: {e}; change-history metrics omitted"));
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
    // The file-level hotspot uses the whole-file cyclomatic *sum* so the
    // `bca vcs` column matches the file-level `bca metrics --vcs` value.
    set_hotspot_score(&mut stat, space.metrics.cyclomatic.cyclomatic_sum());
    space.metrics.vcs = Some(stat);
}

/// Fill a `vcs` block's `hotspot_score` from a `complexity` value
/// (complexity × recent churn). The caller chooses which cyclomatic
/// figure to pass: file-level [`inject`] passes the subtree `*_sum`,
/// while the per-function [`assign_child_stats`] passes each function's
/// *own* cyclomatic so nested complexity is not re-counted at every
/// enclosing level (issue #709).
fn set_hotspot_score(stat: &mut vcs::Stats, cyclomatic: u64) {
    #[allow(clippy::cast_precision_loss)]
    let complexity = cyclomatic as f64;
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
            warn(format_args!(
                "--vcs-per-function: {e}; per-function change-history omitted"
            ));
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
            warn(format_args!(
                "--vcs-per-function: skipping {}: {e}",
                path.display()
            ));
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
/// `hotspot_score` from that function's *own* cyclomatic complexity.
///
/// Uses the per-space `cyclomatic()` (own), not `cyclomatic_sum()`
/// (subtree rollup): a function's hotspot should reflect its own logic
/// complexity, otherwise a nested function's complexity is re-counted at
/// every enclosing level and the outer scores are inflated (issue #709).
fn assign_child_stats(space: &mut FuncSpace, stats: &mut impl Iterator<Item = vcs::Stats>) {
    for child in &mut space.spaces {
        if let Some(mut stat) = stats.next() {
            set_hotspot_score(&mut stat, child.metrics.cyclomatic.cyclomatic());
            child.metrics.vcs = Some(stat);
        }
        assign_child_stats(child, stats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `FileEntry` whose only varied fields are the path, recent churn,
    /// and (initially absent) hotspot score — enough to exercise the join.
    fn file_entry(path: &str, churn_recent: u64) -> FileEntry {
        FileEntry {
            path: path.to_owned(),
            vcs: wire::Vcs {
                commits_long: 5,
                commits_recent: 2,
                churn_long: churn_recent * 3,
                churn_recent,
                authors_long: 2,
                authors_recent: 1,
                ownership_top_share: 0.5,
                burst: 0.4,
                bug_fix_commits: 1,
                security_fix_commits: 0,
                revert_commits: 0,
                age_days: 100,
                last_modified_days: 3,
                change_entropy_long: 1.0,
                change_entropy_recent: 0.5,
                cochange_entropy_long: 0.7,
                cochange_entropy_recent: 0.3,
                risk_score: 1.0,
                hotspot_score: None,
                author_ids: None,
            },
        }
    }

    #[test]
    fn join_hotspot_scores_fills_matching_rows_and_skips_others() {
        // The work-tree the change-history paths are relative to. Real
        // files are created so `canonicalize` (which `join_hotspot_scores`
        // mirrors from `inject`) resolves them; the join must key off the
        // canonicalized-then-stripped repo-relative path, not the raw
        // absolute path the AST walk emits.
        //
        // The workdir is canonicalized to mirror production: `repo::open`
        // stores `repo.workdir().canonicalize()`, and `join_hotspot_scores`
        // strips that canonical prefix off the canonicalized file path. A
        // raw `tempdir()` path is *not* canonical where the temp root is a
        // symlink (macOS `/var/folders`, `/tmp` -> `/private/tmp`; Windows
        // verbatim `\\?\` prefixes), so the strip would fail and the join
        // silently match nothing — a test-only artifact, not a real miss.
        let workdir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(workdir.path().join("src")).expect("mkdir src");
        let workdir_root = workdir.path().canonicalize().expect("canonicalize workdir");
        let abs_a = workdir.path().join("src/a.rs");
        let abs_b = workdir.path().join("src/b.rs");
        std::fs::write(&abs_a, b"fn a() {}").expect("write a");
        std::fs::write(&abs_b, b"fn b() {}").expect("write b");

        let report = Report {
            long_window_days: 365,
            recent_window_days: 90,
            risk_score_version: 1,
            vcs_schema_version: 1,
            truncated_shallow_clone: false,
            vcs_aggregate: None,
            // `b.rs` has no AST entry below, so it must stay `None`.
            files: vec![file_entry("src/a.rs", 40), file_entry("src/b.rs", 10)],
        };
        let mut default = DefaultReport {
            report,
            workdir: Some(workdir_root),
        };

        // `a.rs` cyclomatic sum 7; `extra.rs` resolves (it exists on disk, so
        // `repo_relative`'s `canonicalize` succeeds) but has no change-history
        // row, exercising the `by_path.remove` miss — it must be skipped
        // without panicking. Writing it is load-bearing: an absent file would
        // be dropped earlier at `canonicalize`, never reaching the row lookup.
        let abs_extra = workdir.path().join("src/extra.rs");
        std::fs::write(&abs_extra, b"fn extra() {}").expect("write extra");
        let cyclomatic_sums = vec![(abs_a.clone(), 7.0), (abs_extra, 99.0)];
        default.join_hotspot_scores(&cyclomatic_sums);

        let a = &default.report.files[0];
        assert_eq!(a.path, "src/a.rs");
        assert_eq!(
            a.vcs.hotspot_score,
            Some(hotspot::hotspot_score(7.0, 40)),
            "matched row gets complexity × recent churn"
        );
        let b = &default.report.files[1];
        assert_eq!(b.path, "src/b.rs");
        assert_eq!(
            b.vcs.hotspot_score, None,
            "a row with no AST file keeps a None score"
        );
    }

    #[test]
    fn join_hotspot_scores_no_op_without_a_workdir() {
        // Outside a work-tree (no index) the join cannot resolve paths and
        // must leave every score untouched rather than guessing.
        let report = Report {
            long_window_days: 365,
            recent_window_days: 90,
            risk_score_version: 1,
            vcs_schema_version: 1,
            truncated_shallow_clone: false,
            vcs_aggregate: None,
            files: vec![file_entry("src/a.rs", 40)],
        };
        let mut default = DefaultReport {
            report,
            workdir: None,
        };
        default.join_hotspot_scores(&[(PathBuf::from("/whatever/src/a.rs"), 7.0)]);
        assert_eq!(default.report.files[0].vcs.hotspot_score, None);
    }

    #[test]
    fn per_function_hotspot_uses_own_not_subtree_cyclomatic() {
        use big_code_analysis::{Ast, LANG, MetricsOptions, Source};

        // Recent churn shared by both probes so the only varying factor is
        // the cyclomatic figure fed to `set_hotspot_score`.
        const CHURN: u64 = 12;

        // Regression test for issue #709. A nested function inflates the
        // *outer* function's `cyclomatic_sum()` (subtree rollup) but not
        // its own `cyclomatic()`. The per-function hotspot must score each
        // function by its own complexity, otherwise the nested branches are
        // re-counted at every enclosing level. The `if`/`match` arms below
        // give the outer function branches *and* a nested function with its
        // own branches, so own ≠ sum at the outer level.
        let code = br"
fn outer(x: i32) -> i32 {
    fn inner(y: i32) -> i32 {
        if y > 0 { 1 } else if y < 0 { -1 } else { 0 }
    }
    if x > 10 { inner(x) } else { 0 }
}
";
        let ast = Ast::parse(Source::new(LANG::Rust, code)).expect("parse rust");
        let space = ast
            .metrics(MetricsOptions::default())
            .expect("metrics walk");

        // The outer function is the first child space of the synthetic unit.
        let outer = space.spaces.first().expect("outer function space");
        let own = outer.metrics.cyclomatic.cyclomatic();
        let subtree = outer.metrics.cyclomatic.cyclomatic_sum();
        assert!(
            subtree > own,
            "the nested function must make subtree ({subtree}) exceed own ({own})"
        );

        // `set_hotspot_score` consumes whichever figure the caller passes,
        // and the two figures yield different scores for the same churn —
        // so the inject (sum) and per-function (own) paths are distinct.
        let mut own_stat = vcs::Stats {
            churn_recent: CHURN,
            ..Default::default()
        };
        let mut sum_stat = vcs::Stats {
            churn_recent: CHURN,
            ..Default::default()
        };
        set_hotspot_score(&mut own_stat, own);
        set_hotspot_score(&mut sum_stat, subtree);
        assert!(
            own_stat.hotspot_score.is_some_and(|score| score > 0.0),
            "own-cyclomatic path yields a positive hotspot score"
        );
        assert_ne!(
            own_stat.hotspot_score, sum_stat.hotspot_score,
            "own vs subtree complexity must produce distinct hotspot scores"
        );

        // Drive the real `assign_child_stats` call site (not just
        // `set_hotspot_score` in isolation): a regression that reverts the
        // outer-space scoring to `cyclomatic_sum()` is invisible to the
        // isolated probes above, since they pass `own`/`subtree`
        // themselves. Feed one stat with the shared churn and assert the
        // attached outer-function score equals the *own*-based score and
        // differs from the *subtree*-based one.
        let mut driven = space;
        let mut stats = std::iter::once(vcs::Stats {
            churn_recent: CHURN,
            ..Default::default()
        });
        assign_child_stats(&mut driven, &mut stats);
        let attached = driven.spaces[0]
            .metrics
            .vcs
            .as_ref()
            .expect("outer space received a vcs block")
            .hotspot_score;
        assert_eq!(
            attached, own_stat.hotspot_score,
            "assign_child_stats must score the outer function by its own cyclomatic"
        );
        assert_ne!(
            attached, sum_stat.hotspot_score,
            "assign_child_stats must not score by the subtree cyclomatic_sum"
        );
    }

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
