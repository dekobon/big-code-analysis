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

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use big_code_analysis::FuncSpace;
use big_code_analysis::defang_formula;
use big_code_analysis::vcs::{
    self, AuthorHashKey, CacheConfig, Options, attach, build_history_index_cached, hotspot,
    parse_timestamp, parse_window, score, stats,
};
use big_code_analysis::wire;

use crate::formats::{
    VcsFormat, write_buffered, write_cbor, write_json, write_text, write_toml, write_yaml,
};
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
    /// How many entries were ranked before `--top` truncated `files` —
    /// the tracked files the walk admitted and, under `--include-deleted`,
    /// the deleted ones appended — so the HTML severity-heat bands
    /// (`vcs_report::risk_heat_class`) grade each row against the whole
    /// population rather than the rows shown; with `--top 1` the
    /// repository's riskiest file was otherwise painted green.
    /// Presentation state, never serialized. Invariant: at least
    /// `files.len()`, since truncation only removes.
    #[serde(skip)]
    pub(crate) ranked_total: usize,
}

impl Report {
    /// The population the risk ranking was drawn from.
    pub(crate) fn ranked_population(&self) -> usize {
        debug_assert!(
            self.ranked_total >= self.files.len(),
            "ranked_total ({}) below the rows carried ({})",
            self.ranked_total,
            self.files.len()
        );
        self.ranked_total
    }

    /// Test-fixture finisher: a report whose population is exactly the
    /// rows it carries, as an untruncated ranking produces. A struct
    /// literal cannot read its own `files`, so fixtures write a
    /// placeholder `ranked_total` and call this.
    #[cfg(test)]
    pub(crate) fn with_untruncated_total(mut self) -> Self {
        self.ranked_total = self.files.len();
        self
    }
}

/// Entry point for `Command::Vcs`.
pub(crate) fn run(mut globals: GlobalOpts, args: VcsArgs) {
    // Default the walk root to the current directory so a bare
    // `bca vcs` ranks the repository the user is standing in.
    if globals.paths.is_empty() {
        globals.paths.push(PathBuf::from("."));
    }
    let root = resolve_root(&globals);

    // Cache controls apply only to the bare `bca vcs` file ranking. Reject
    // them up front when combined with a subcommand so the CLI does not
    // silently ignore a knob the caller set, exactly as the web `/vcs/trend`
    // endpoint used to (issue #961).
    if let Err(message) = reject_cache_flags_with_subcommand(&args) {
        die(format_args!("{message}"));
    }

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

    let (entries, ranked_total) = rank(&globals, &index, args.top, args.include_deleted);
    let report = Report {
        long_window_days: options.long_window_days(),
        recent_window_days: options.recent_window_days(),
        risk_score_version: score::RISK_SCORE_VERSION,
        vcs_schema_version: stats::VCS_SCHEMA_VERSION,
        truncated_shallow_clone: index.truncated_shallow_clone(),
        vcs_aggregate: index.vcs_aggregate(),
        files: entries,
        ranked_total,
    };

    crate::die_unless_broken_pipe(emit(&report, &args), "writing vcs output");
}

/// Reject the cache controls when they accompany a `vcs` subcommand.
///
/// `--no-cache` / `--clear-cache` / `--cache-dir` live on the parent
/// [`VcsArgs`] but only the bare `bca vcs` ranking consults them; `commit`
/// and `trend` never touch the persistent cache, so accepting the flags in
/// the parent position (`bca vcs --no-cache trend`) would silently drop
/// them — the misleading behaviour the web `/vcs/trend` endpoint had
/// (issue #961). The bare ranking (no subcommand) is unaffected.
pub(crate) fn reject_cache_flags_with_subcommand(args: &VcsArgs) -> Result<(), &'static str> {
    if args.command.is_some() && (args.no_cache || args.clear_cache || args.cache_dir.is_some()) {
        return Err(
            "--no-cache / --clear-cache / --cache-dir apply only to the bare \
                    `bca vcs` file ranking, not to `bca vcs commit` or `bca vcs trend` \
                    (which do not use the cache)",
        );
    }
    Ok(())
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
    let (entries, ranked_total) = rank(globals, &index, top, false);
    let report = Report {
        long_window_days: options.long_window_days(),
        recent_window_days: options.recent_window_days(),
        risk_score_version: score::RISK_SCORE_VERSION,
        vcs_schema_version: stats::VCS_SCHEMA_VERSION,
        truncated_shallow_clone: index.truncated_shallow_clone(),
        vcs_aggregate: index.vcs_aggregate(),
        files: entries,
        ranked_total,
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
    /// churn` exactly as `bca metrics --vcs` does.
    ///
    /// `complexities` is the stream the report walk emits: the absolute
    /// file path paired with its [`attach::file_complexity`] figure. Each
    /// absolute path is canonicalized and stripped against the index
    /// work-tree — the same match [`inject`] performs — so a row gets its
    /// score regardless of
    /// `--strip-prefix` or how the walk root was spelled. Files with no
    /// matching change-history row (untracked / outside the work-tree)
    /// are silently skipped; rows with no matching AST file (a tracked
    /// non-source file, e.g. a deleted entry) and rows whose file carries
    /// no computed complexity (#1742) keep `hotspot_score == None`.
    pub(crate) fn join_hotspot_scores(&mut self, complexities: &[(PathBuf, Option<f64>)]) {
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
        for (abs, complexity) in complexities {
            let Some(rel) = repo_relative(abs, workdir).and_then(|rel| path_to_string(&rel)) else {
                continue;
            };
            if let Some(idx) = by_path.remove(rel.as_str()) {
                let entry = &mut self.report.files[idx];
                entry.vcs.hotspot_score =
                    complexity.map(|c| hotspot::hotspot_score(c, entry.vcs.churn_recent));
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
    options.author_hash_key = resolve_author_hash_key(args);
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

/// Environment variable holding the author-hash key: the recommended,
/// security-preferred channel (off the process list and shell history),
/// though the `--author-hash-key` flag wins when both are set (#956).
const AUTHOR_HASH_KEY_ENV: &str = "BCA_AUTHOR_HASH_KEY";

/// Resolve the optional author-hash key (issue #956): the
/// `--author-hash-key` flag takes precedence, else the
/// [`AUTHOR_HASH_KEY_ENV`] environment variable.
///
/// The key only hardens *emitted* digests, so it is read only when
/// `--emit-author-details` is set; an ambient environment variable never
/// fails an unrelated run. An explicit flag without
/// `--emit-author-details` is a usage error rather than a silent no-op,
/// and an empty key is rejected by [`AuthorHashKey::new`].
fn resolve_author_hash_key(args: &VcsArgs) -> Option<AuthorHashKey> {
    if args.author_hash_key.is_some() && !args.emit_author_details {
        die(format_args!(
            "--author-hash-key requires --emit-author-details (the key only \
             hardens the emitted author digests)"
        ));
    }
    if !args.emit_author_details {
        return None;
    }
    let raw = args.author_hash_key.clone().or_else(|| {
        std::env::var(AUTHOR_HASH_KEY_ENV)
            .ok()
            .filter(|value| !value.is_empty())
    })?;
    Some(
        AuthorHashKey::new(raw.into_bytes())
            .unwrap_or_else(|e| die(format_args!("--author-hash-key: {e}"))),
    )
}

/// Select the tracked files the global filters admit, sort them by risk
/// score (descending; path as a stable tie-break), and truncate to the
/// top `top` (0 = all). Also returns how many files were ranked before
/// truncation, for [`Report::ranked_total`].
fn rank(
    globals: &GlobalOpts,
    index: &vcs::HistoryIndex,
    top: usize,
    include_deleted: bool,
) -> (Vec<FileEntry>, usize) {
    // Reuse the standard walk so `--paths/--include/--exclude/--no-ignore`
    // behave exactly as elsewhere; intersect the result with the tracked
    // set (untracked / binary files are simply absent from the index).
    let (resolved, _jobs) = crate::resolve_walk_files(globals.clone());
    // A directory the walk could not list silently shortens the ranking,
    // and a ranking is only as useful as its completeness — the same
    // argument the analysing subcommands make in `enforce_complete_walk`
    // (#1131). `vcs` reads history, never file contents, so this is the
    // only I/O tally it has.
    if resolved.walk_errors.count() > 0 {
        die(crate::walk_failure_summary(resolved.walk_errors.count()));
    }
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

    let ranked_total = entries.len();
    vcs::rank_by_risk(&mut entries, top, |e| (e.path.as_str(), e.vcs.risk_score));
    (entries, ranked_total)
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
    // normalize at this single output chokepoint, with the helper `bca
    // diff` keys its sets by so the two cannot drift.
    crate::format_util::slash_path(path).or_else(|| {
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
        Some(VcsFormat::Csv) => write_csv(report, output.map(PathBuf::as_path)),
        Some(VcsFormat::Json) => write_json(report, args.pretty, output),
        Some(VcsFormat::Yaml) => write_yaml(report, output),
        Some(VcsFormat::Toml) => write_toml(report, args.pretty, output),
        Some(VcsFormat::Cbor) => write_cbor(report, output),
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
fn write_csv(report: &Report, output: Option<&Path>) -> std::io::Result<()> {
    write_buffered(output, |sink| write_csv_rows(report, sink))
}

/// Emit the header plus one row per ranked file into `sink`. Split from
/// [`write_csv`] so the destination-and-buffering choice stays in one
/// place and the row shape in another.
fn write_csv_rows(report: &Report, sink: &mut dyn Write) -> std::io::Result<()> {
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
        // `path` is the only free-text, user-controlled cell: it is a
        // repository-relative path from the VCS walk, so a checked-in file
        // literally named `=cmd|'/C calc'!A0` would execute as a formula on
        // open. Defang it with the shared OWASP leading-apostrophe helper
        // (CWE-1236, #703, #794). The csv crate's RFC-4180 quoting does NOT
        // defang formulas. Every other cell is a non-negative numeric metric
        // (f64 scores / entropy / shares, u32 counts and day spans) that can
        // never begin with a trigger character, so they need no guard.
        let path = defang_formula(entry.path.as_str());
        wtr.write_record([
            path.as_str(),
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
/// with the hotspot scored from the subtree cyclomatic sum when
/// cyclomatic was computed ([`attach::attach_file_history`]). A file
/// with no index entry (untracked / binary) is left untouched, so its
/// `vcs` field stays `None`.
pub(crate) fn inject(space: &mut FuncSpace, path: &Path, index: &vcs::HistoryIndex) {
    attach::attach_file_history(space, path, index);
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

/// Blame `path` once through this thread's session and attach a `vcs`
/// block to every nested function / method / class space
/// ([`attach::attach_per_function_history`], issue #329). The file-level
/// (root) space keeps the file block that [`inject`] attached; only its
/// descendants are touched here.
///
/// A blame failure — an untracked file, a path outside the work tree, or
/// a genuine backend error — is warned about and leaves the per-function
/// blocks unset (the file still emits its AST metrics and file-level
/// `vcs`), so one unblameable file never aborts the walk.
pub(crate) fn inject_per_function(
    space: &mut FuncSpace,
    path: &Path,
    blame: &Arc<vcs::PerFunctionBlame>,
) {
    // Nothing to attach: return before the session is opened, so a file
    // with no descendants never costs a `BlameSession`.
    if space.spaces.is_empty() {
        return;
    }
    let attached = with_thread_session(blame, |session| {
        attach::attach_per_function_history(space, path, session)
    });
    if let Err(e) = attached {
        warn(format_args!(
            "--vcs-per-function: skipping {}: {e}",
            path.display()
        ));
    }
}

thread_local! {
    /// The calling worker's [`vcs::BlameSession`], reused across every
    /// file that thread blames.
    ///
    /// A session is `!Sync` (it owns a `gix::Repository`), so it cannot
    /// live in the shared `Config` alongside the engine; and
    /// `ConcurrentRunner`'s per-file callback is a plain
    /// `Fn(PathBuf, &Config)` with no per-thread state slot to thread one
    /// through. A thread-local is the seam that exists. It is dropped when
    /// the worker exits, which is the end of the run.
    static BLAME_SESSION: RefCell<Option<vcs::BlameSession>> = const { RefCell::new(None) };
}

/// Run `blame_with` against this thread's session, opening one (or
/// replacing a session belonging to a different engine) on first use.
///
/// The engine check is not decoration: one process can build more than
/// one `Config` — the test suite does — and a session carries its
/// engine's repository, target ref, windows, and bot filter. Serving a
/// second engine from the first one's session would answer with the
/// wrong repository's history rather than fail.
fn with_thread_session<T>(
    blame: &Arc<vcs::PerFunctionBlame>,
    blame_with: impl FnOnce(&mut vcs::BlameSession) -> T,
) -> T {
    // The borrow spans the whole blame rather than just the slot swap:
    // the session is what the blame runs against, and moving it out and
    // back would lose it on an unwind. `with_borrow_mut`'s panic-on-
    // re-entry is unreachable so long as no closure passed from this
    // module re-enters `with_thread_session`: `BLAME_SESSION` is private
    // to this module and read here only, and `blame_with` is the
    // caller's, so that invariant is on every `blame_with` written here.
    BLAME_SESSION.with_borrow_mut(|slot| {
        if slot
            .as_ref()
            .is_none_or(|session| !Arc::ptr_eq(session.engine(), blame))
        {
            *slot = None;
        }
        blame_with(slot.get_or_insert_with(|| blame.session()))
    })
}

// The scratch-repository `git` every CLI suite builds its fixtures
// with (`big-code-analysis-cli/tests/common/git_fixture.rs`): the
// inherited `GIT_*` scrub, the host-configuration pins, the fixed
// identity and dates, and the hostile host that proves them. Declared at
// module scope so the `#[path]` resolves relative to `src/`, as
// `html_report.rs` reaches its validators.
#[cfg(test)]
#[path = "../tests/common/git_fixture.rs"]
#[allow(dead_code)]
mod git_fixture_for_tests;

#[cfg(test)]
mod tests {
    use super::git_fixture_for_tests::{
        DAY, FIXED_NOW, HostileGitHost, assert_no_user_name, git_at_with_env, git_output,
        is_git_var,
    };
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
            ranked_total: 0,
        }
        .with_untruncated_total();
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
        let complexities = vec![(abs_a.clone(), Some(7.0)), (abs_extra, Some(99.0))];
        default.join_hotspot_scores(&complexities);

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
            ranked_total: 0,
        }
        .with_untruncated_total();
        let mut default = DefaultReport {
            report,
            workdir: None,
        };
        default.join_hotspot_scores(&[(PathBuf::from("/whatever/src/a.rs"), Some(7.0))]);
        assert_eq!(default.report.files[0].vcs.hotspot_score, None);
    }

    #[test]
    fn join_hotspot_scores_clears_a_row_whose_file_has_no_computed_complexity() {
        // `attach::file_complexity` is `None` when the space computed no
        // cyclomatic; the join must then assign `None` — not skip the
        // row and leave whatever the entry carried (#1742 for `report`).
        let workdir = tempfile::tempdir().expect("tempdir");
        let workdir_root = workdir.path().canonicalize().expect("canonicalize workdir");
        let abs_a = workdir.path().join("a.rs");
        std::fs::write(&abs_a, b"fn a() {}").expect("write a");
        let mut entry = file_entry("a.rs", 40);
        entry.vcs.hotspot_score = Some(9.0);
        let report = Report {
            long_window_days: 365,
            recent_window_days: 90,
            risk_score_version: 1,
            vcs_schema_version: 1,
            truncated_shallow_clone: false,
            vcs_aggregate: None,
            files: vec![entry],
            ranked_total: 0,
        }
        .with_untruncated_total();
        let mut default = DefaultReport {
            report,
            workdir: Some(workdir_root),
        };
        default.join_hotspot_scores(&[(abs_a, None)]);
        assert_eq!(
            default.report.files[0].vcs.hotspot_score, None,
            "no computed complexity clears the seeded score"
        );
    }

    /// Render a one-entry VCS report to CSV via a temp file and return
    /// the file's contents. `write_csv` only targets a file path or
    /// stdout, so a temp file is the capturable sink.
    fn render_csv(entry: FileEntry) -> String {
        let report = Report {
            long_window_days: 365,
            recent_window_days: 90,
            risk_score_version: 1,
            vcs_schema_version: 1,
            truncated_shallow_clone: false,
            vcs_aggregate: None,
            files: vec![entry],
            ranked_total: 0,
        }
        .with_untruncated_total();
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("vcs.csv");
        write_csv(&report, Some(out.as_path())).expect("write_csv to temp file");
        std::fs::read_to_string(&out).expect("read CSV back")
    }

    #[test]
    fn csv_path_cell_is_defanged_against_formula_injection() {
        // A checked-in file whose repository-relative path begins with a
        // spreadsheet formula trigger must not execute as a formula when the
        // VCS CSV is opened in Excel / LibreOffice / Sheets (CWE-1236, #703,
        // #794). The `path` cell is the only free-text, user-controlled
        // column; it must carry the same OWASP leading-apostrophe defang as
        // the main FuncSpace CSV writer. Cover every trigger character.
        for trigger in ['=', '+', '-', '@'] {
            let path = format!("{trigger}cmd|'/C calc'!A0.rs");
            let out = render_csv(file_entry(&path, 10));
            let data_row = out.lines().nth(1).expect("a data row after the header");
            assert!(
                data_row.starts_with(&format!("'{trigger}")),
                "path cell must be defanged with a leading quote for trigger \
                 {trigger:?}, got:\n{out}"
            );
            assert!(
                !data_row.starts_with(trigger),
                "the raw formula trigger must not lead the row for {trigger:?}:\n{out}"
            );
        }
    }

    #[test]
    fn csv_plain_path_is_not_quoted() {
        // A normal path must pass through untouched — no spurious leading
        // quote that would corrupt the value for downstream consumers.
        let out = render_csv(file_entry("src/main.rs", 10));
        let data_row = out.lines().nth(1).expect("a data row after the header");
        assert!(
            data_row.starts_with("src/main.rs,"),
            "a plain path must be emitted verbatim, got:\n{out}"
        );
    }

    /// The issue #1742 fixture: a nested function so that both the
    /// file-level and the per-function wrappers attach a block, analysed
    /// with only `nargs` selected so no space computed cyclomatic. The
    /// rule itself — which figure scores a hotspot, and that an
    /// uncomputed one scores nothing — is pinned in the library's
    /// `vcs::attach` tests; this file pins only that the CLI's wrappers
    /// reach it.
    fn nargs_only_space() -> FuncSpace {
        use big_code_analysis::{Ast, LANG, Metric, MetricsOptions, Source};

        let code = br"
def outer(x):
    if x:
        pass
    def inner(y):
        if y:
            return 1
        return 2
    return inner
";
        let ast = Ast::parse(Source::new(LANG::Python, code)).expect("parse python");
        ast.metrics(MetricsOptions::default().with_only(&[Metric::Nargs]))
            .expect("metrics walk")
    }

    /// `inject` drives `attach::attach_file_history`: the index entry is
    /// attached, and its seeded score is cleared because the space has no
    /// computed cyclomatic — the library rule, reached through the CLI.
    #[test]
    fn inject_attaches_the_file_block_through_the_library() {
        let mut space = nargs_only_space();
        let workdir = tempfile::tempdir().expect("tempdir");
        let workdir_root = workdir.path().canonicalize().expect("canonicalize workdir");
        let file = workdir.path().join("nested.py");
        std::fs::write(&file, b"").expect("write nested.py");
        let files = std::iter::once((
            PathBuf::from("nested.py"),
            vcs::Stats {
                churn_recent: 8,
                hotspot_score: Some(9.0),
                ..Default::default()
            },
        ))
        .collect();
        let index = vcs::HistoryIndex::new(files, Some(workdir_root), false);
        inject(&mut space, &file, &index);
        let file_vcs = space
            .metrics
            .vcs
            .as_ref()
            .expect("file space received a vcs block");
        assert_eq!((file_vcs.churn_recent, file_vcs.hotspot_score), (8, None));
    }

    #[test]
    fn path_to_string_normalizes_separators_to_forward_slash() {
        // A path assembled from components uses the OS separator (`\` on
        // Windows, `/` on Unix); the emitted git path must always be
        // forward-slash so the JSON / CSV / table output is byte-identical
        // cross-platform. On Windows this guards the `src\work.rs`
        // regression that failed `tests/vcs/vcs_rank.rs` on windows-latest.
        let rel: PathBuf = ["src", "work.rs"].iter().collect();
        assert_eq!(path_to_string(&rel).as_deref(), Some("src/work.rs"));
    }

    /// A throwaway repo with `src/work.rs` created 200 days before
    /// [`FIXED_NOW`] and edited 5 days before it — two commits, one per
    /// side of a 30-day window.
    fn blame_fixture() -> tempfile::TempDir {
        blame_fixture_with_env(&[])
    }

    /// [`blame_fixture`] with extra environment on every `git`, so a
    /// test can point the fixture at a hostile host configuration and
    /// check it is not read.
    fn blame_fixture_with_env(env: &[(&str, &str)]) -> tempfile::TempDir {
        let git = |dir: &Path, secs: i64, args: &[&str]| git_at_with_env(dir, secs, args, env);

        let dir = tempfile::tempdir().expect("tempdir");
        git(dir.path(), FIXED_NOW, &["init", "-q", "-b", "main"]);
        git(
            dir.path(),
            FIXED_NOW,
            &["config", "commit.gpgsign", "false"],
        );
        // Neutralise a contributor's global `core.hooksPath` (#941):
        // `--no-verify` skips the commit hooks, not `post-commit`.
        let no_hooks = dir.path().join(".bca-empty-hooks");
        std::fs::create_dir_all(&no_hooks).expect("mkdir empty hooks dir");
        let no_hooks = no_hooks.to_str().expect("hooks path is valid UTF-8");
        git(
            dir.path(),
            FIXED_NOW,
            &["config", "core.hooksPath", no_hooks],
        );
        git(dir.path(), FIXED_NOW, &["config", "core.autocrlf", "false"]);
        std::fs::create_dir(dir.path().join("src")).expect("mkdir src");
        std::fs::write(
            dir.path().join("src/work.rs"),
            "fn f() {\n  let x = 1;\n}\n",
        )
        .expect("write");
        git(dir.path(), FIXED_NOW - 200 * DAY, &["add", "-A"]);
        git(
            dir.path(),
            FIXED_NOW - 200 * DAY,
            &["commit", "-q", "--no-verify", "-m", "create"],
        );
        std::fs::write(
            dir.path().join("src/work.rs"),
            "fn f() {\n  let x = 2;\n}\n",
        )
        .expect("rewrite");
        git(dir.path(), FIXED_NOW - 5 * DAY, &["add", "-A"]);
        git(
            dir.path(),
            FIXED_NOW - 5 * DAY,
            &["commit", "-q", "--no-verify", "-m", "edit"],
        );
        dir
    }

    /// The fixture's `git` must not read the host's configuration, and
    /// must not let a caller's environment switch it back on (#1771).
    ///
    /// Every `git` is handed a [`HostileGitHost`] through the fixture's
    /// caller-environment parameter. The fixture's own assertions do
    /// the discriminating: read, the host's `XDG_CONFIG_HOME/git/ignore`
    /// makes `git add -A` stage nothing so the first commit fails. Its
    /// `core.hooksPath` reaches only `git init` (on a git whose `init`
    /// writes `HEAD` through a ref transaction): the fixture pins a
    /// repository-local hooks directory before its first commit. The
    /// `user.name` sentinel guards `GIT_CONFIG_GLOBAL` and
    /// `GIT_CONFIG_NOSYSTEM` on every git and platform, since the pinned
    /// `core.excludesFile` overrides the host's. The closing assertion
    /// cannot fail from host configuration being read —
    /// `GIT_AUTHOR_NAME` decides `%an` and `-b main` overrides
    /// `init.defaultBranch` — so it only pins that the fixture produced
    /// its commits on `main`, read back through the same isolation (a
    /// host `log.decorate = full` would otherwise change `%D`).
    #[test]
    fn blame_fixture_ignores_the_hosts_global_and_system_config() {
        let host = HostileGitHost::new();
        let repo = blame_fixture_with_env(&host.env());
        assert_no_user_name(repo.path(), &host.env());
        assert_eq!(
            git_output(repo.path(), &["log", "-1", "--format=%an %D"], &host.env()),
            "Ada HEAD -> main"
        );
    }

    /// The fixture alone, read back through its own `git`: two commits
    /// on `main`, the file tracked. The target the child-process tests
    /// below re-run.
    #[test]
    fn blame_fixture_builds_the_two_commit_repo() {
        let repo = blame_fixture();
        let read = |args: &[&str]| git_output(repo.path(), args, &[]);
        assert_eq!(read(&["rev-list", "--count", "HEAD"]), "2");
        assert_eq!(read(&["ls-files"]), "src/work.rs");
        assert_eq!(read(&["log", "-1", "--format=%an %D"]), "Ada HEAD -> main");
    }

    /// The `git` that builds and reads the victim repository: its own
    /// removal loop over the inherited `GIT_*` variables, so a defect in
    /// the shared fixture's `command` loop cannot reach it (the two share
    /// only the `is_git_var` prefix test), and the host's configuration
    /// kept away, the per-user ignore and attributes files included,
    /// since those are read whatever the global and system variables say
    /// and a host `XDG_CONFIG_HOME/git/ignore` of `*` would fail the
    /// victim's `git add` before the child under test ever ran.
    #[cfg(unix)]
    fn clean_git(dir: &Path, args: &[&str]) -> String {
        let mut cmd = std::process::Command::new("git");
        for (key, _) in std::env::vars_os() {
            if is_git_var(&key) {
                cmd.env_remove(key);
            }
        }
        let out = cmd
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@x",
                "-c",
                "core.excludesFile=/dev/null",
                "-c",
                "core.attributesFile=/dev/null",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("spawn git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Re-runs [`blame_fixture_builds_the_two_commit_repo`] in a child
    /// test binary with `vars` exported, and asserts that it passed. A
    /// test cannot set its own process's environment without `unsafe`,
    /// and the fixture reads it through `Command`, so the variables are
    /// set on a child instead. The name is module-qualified: libtest's
    /// `--exact` matches the full path.
    #[cfg(unix)]
    fn rerun_fixture_test_with(vars: &[(&str, std::path::PathBuf)]) {
        let child = format!(
            "{}::blame_fixture_builds_the_two_commit_repo",
            module_path!()
                .strip_prefix(concat!(env!("CARGO_CRATE_NAME"), "::"))
                .expect("the test module is nested under the crate root")
        );
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", &child, "--format", "pretty"])
            .envs(vars.iter().map(|(k, v)| (k, v)))
            .output()
            .expect("spawn the child");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("test result: ok. 1 passed"),
            "{child} must pass with {vars:?} inherited; stdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A hook, and `git rebase --exec` in a linked worktree, export
    /// `GIT_DIR` and `GIT_INDEX_FILE`, and both outrank `current_dir`
    /// (#1846). The fixture test is re-run in a child with that pair
    /// naming another repository: the child must pass, and that
    /// repository must come through untouched. Verified by revert:
    /// without the scrub in the shared fixture's `command` the child's
    /// `init`, `add` and `commit` land in the victim — its HEAD moves and
    /// its index lists `src/work.rs`.
    #[cfg(unix)]
    #[test]
    fn blame_fixture_ignores_an_inherited_repository() {
        let victim = tempfile::tempdir().expect("tempdir");
        std::fs::write(victim.path().join("keep.rs"), "fn keep() {}\n").expect("write");
        clean_git(victim.path(), &["init", "-q", "-b", "main"]);
        clean_git(victim.path(), &["add", "keep.rs"]);
        clean_git(victim.path(), &["commit", "-q", "-m", "victim"]);
        let state = || {
            (
                clean_git(victim.path(), &["rev-parse", "HEAD"]),
                clean_git(victim.path(), &["ls-files", "-s"]),
            )
        };
        let before = state();
        assert!(before.1.contains("keep.rs"), "the victim tracks keep.rs");
        let git_dir = victim.path().join(".git");
        rerun_fixture_test_with(&[
            ("GIT_DIR", git_dir.clone()),
            ("GIT_INDEX_FILE", git_dir.join("index")),
        ]);
        assert_eq!(state(), before, "the inherited repository was written to");
    }

    /// `GIT_OBJECT_DIRECTORY`, which a `receive-pack` quarantine hook
    /// exports, replaces the object store git reads; once the quarantine
    /// it names is gone, discovery itself fails with `not a git
    /// repository`. The fixture test is re-run in a child with it naming
    /// a directory whose parent does not exist either: `git init`
    /// creates one missing level of an object directory, so a path one
    /// level down would let the unscrubbed fixture succeed. Kept apart
    /// from the repository case above: a fixture that failed to start
    /// would leave that case's victim untouched for the wrong reason.
    /// Verified by revert: without the scrub the child's `git init`
    /// fails.
    #[cfg(unix)]
    #[test]
    fn blame_fixture_ignores_an_inherited_object_directory() {
        let gone = tempfile::tempdir().expect("tempdir");
        rerun_fixture_test_with(&[(
            "GIT_OBJECT_DIRECTORY",
            gone.path().join("gone").join("quarantine"),
        )]);
        // Pins the path, not only the child's success: a git that created
        // both levels, or a scrub that dropped the variable only after
        // `init`, would pass the rerun and leave the directory behind.
        assert!(
            !gone.path().join("gone").exists(),
            "the fixture's git used the inherited object directory"
        );
    }

    /// The per-thread blame session must be rebuilt when the engine it is
    /// asked to serve is not the one it was opened from (issue #1117).
    ///
    /// Both engines here point at the **same** work tree and differ only
    /// in their long window, so serving the second from the first's
    /// session returns a plausible number rather than an error — exactly
    /// the silent failure the `Arc::ptr_eq` guard exists to prevent.
    /// Deleting that guard makes the `narrow` assertion below report 2.
    #[test]
    fn thread_session_is_rebuilt_for_a_different_engine() {
        let repo = blame_fixture();
        let file = repo.path().join("src/work.rs");
        let spans = [vcs::LineSpan::new(1, 3)];

        let mut wide_opts = Options::default();
        wide_opts.as_of = Some(FIXED_NOW);
        let mut narrow_opts = wide_opts.clone();
        // 30 days: only the second commit falls inside.
        narrow_opts.long_window_secs = 30 * DAY;
        narrow_opts.recent_window_secs = 30 * DAY;

        let wide = Arc::new(
            vcs::PerFunctionBlame::open(repo.path(), wide_opts).expect("open wide engine"),
        );
        let narrow = Arc::new(
            vcs::PerFunctionBlame::open(repo.path(), narrow_opts).expect("open narrow engine"),
        );

        let blame = |engine: &Arc<vcs::PerFunctionBlame>| {
            with_thread_session(engine, |session| session.per_function(&file, &spans))
        };

        let first = blame(&wide).expect("wide blame");
        assert_eq!(first[0].commits_long, 2, "365d window sees both commits");

        let second = blame(&narrow).expect("narrow blame");
        assert_eq!(
            second[0].commits_long, 1,
            "30d window must see only the recent commit — a stale session \
             would answer with the wide engine's 2"
        );

        // Switching back must rebuild again rather than keep the narrow
        // session, so the guard is not a one-way latch.
        let third = blame(&wide).expect("wide re-blame");
        assert_eq!(third[0].commits_long, 2, "switching back must rebuild");
    }

    /// `inject_per_function` drives `attach::attach_per_function_history`
    /// through this thread's session: the one function in the fixture
    /// receives its blame block, scored from its own cyclomatic.
    #[test]
    fn inject_per_function_attaches_blame_blocks_through_the_library() {
        use big_code_analysis::{Ast, LANG, MetricsOptions, Source};

        let repo = blame_fixture();
        let file = repo.path().join("src/work.rs");
        let source = std::fs::read(&file).expect("read the fixture");
        let mut space = Ast::parse(Source::new(LANG::Rust, &source))
            .expect("parse rust")
            .metrics(MetricsOptions::default())
            .expect("metrics walk");
        let mut options = Options::default();
        options.as_of = Some(FIXED_NOW);
        let engine =
            Arc::new(vcs::PerFunctionBlame::open(repo.path(), options).expect("open engine"));

        inject_per_function(&mut space, &file, &engine);

        assert!(space.metrics.vcs.is_none(), "the root is the file block's");
        let f = space.spaces.first().expect("the fixture's one function");
        let vcs = f
            .metrics
            .vcs
            .as_ref()
            .expect("the function received a block");
        assert_eq!(vcs.commits_long, 2, "both fixture commits touch `f`");
        // The fixture's `f` has no branches, so own cyclomatic is 1 and the
        // score is the churn itself.
        assert_eq!(f.metrics.cyclomatic.cyclomatic(), 1);
        assert_eq!(
            vcs.hotspot_score,
            Some(hotspot::hotspot_score(1.0, vcs.churn_recent)),
            "scored from the function's own cyclomatic"
        );
    }

    /// `is_git_var` reads the name's first four bytes, so a short name, a
    /// name that merely starts `GIT` (`GITHUB_TOKEN`) and an empty one are
    /// not git's; a lowercase spelling is on Windows alone, where
    /// environment names are case-insensitive. Host-independent, so the
    /// Windows branch runs there while the child reruns stay unix-only.
    /// Reads the shared fixture's predicate through the `#[path]` module,
    /// so it also pins that wiring.
    #[test]
    fn is_git_var_reads_the_prefix_as_the_host_does() {
        use std::ffi::OsStr;
        assert!(is_git_var(OsStr::new("GIT_DIR")));
        assert!(is_git_var(OsStr::new("GIT_")));
        assert!(!is_git_var(OsStr::new("GITHUB_TOKEN")));
        assert!(!is_git_var(OsStr::new("GIT")));
        assert!(!is_git_var(OsStr::new("")));
        assert_eq!(is_git_var(OsStr::new("git_dir")), cfg!(windows));
    }
}
