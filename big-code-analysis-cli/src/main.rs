mod formats;
mod markdown_report;
mod metric_catalog;

use std::collections::{HashMap, hash_map};
use std::ffi::OsString;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread::available_parallelism;

use clap::{Args, Parser, Subcommand};
use globset::{Glob, GlobSet, GlobSetBuilder};

use formats::{
    CBOR_STDOUT_ERROR, MetricsFormat, ReportFormat, dump_checkstyle, dump_clang_warning, dump_csv,
    dump_msvc_warning, dump_sarif,
};
use markdown_report::{FunctionSummary, extract_summaries, generate_report};
use metric_catalog::{ListMetricsMode, write_metrics};

use big_code_analysis::LANG;
use big_code_analysis::ParserTrait;
use big_code_analysis::{
    CommentRm, CommentRmCfg, ConcurrentRunner, Count, CountCfg, Dump, DumpCfg, FilesData, Find,
    FindCfg, Function, FunctionCfg, Metrics, MetricsCfg, OpsCfg, OpsCode, PreprocParser,
    PreprocResults,
};
use big_code_analysis::{
    action, fix_includes, get_from_ext, get_function_spaces, get_ops, guess_language, is_generated,
    preprocess, read_file, read_file_with_eol, write_file,
};

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("Error: {msg}");
    process::exit(1);
}

/// Write `bytes` to stdout, tolerating `BrokenPipe` (the typical case when
/// the consumer is `head`, `less`, etc.) and `die`ing on anything else.
fn write_stdout_or_die(bytes: &[u8]) {
    if let Err(e) = std::io::stdout().lock().write_all(bytes)
        && e.kind() != ErrorKind::BrokenPipe
    {
        die(e);
    }
}

#[derive(Parser, Debug)]
#[clap(
    name = "big-code-analysis-cli",
    version,
    author,
    about = "Analyze source code.",
    subcommand_required = true,
    arg_required_else_help = true,
    after_help = "Migrating from the flag-style CLI? See the migration guide:\n  big-code-analysis-book/src/migration.md"
)]
struct Cli {
    #[clap(flatten)]
    globals: GlobalOpts,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Debug, Default)]
struct GlobalOpts {
    /// Input files or directories to analyze.
    #[clap(long, short, value_parser, global = true)]
    paths: Vec<PathBuf>,
    /// Glob to include files.
    #[clap(long, short = 'I', num_args(0..), global = true)]
    include: Vec<String>,
    /// Glob to exclude files.
    #[clap(long, short = 'X', num_args(0..), global = true)]
    exclude: Vec<String>,
    /// Number of jobs.
    #[clap(long, short = 'j', global = true)]
    num_jobs: Option<usize>,
    /// Force a language type instead of inferring from extension.
    #[clap(long, short = 'l', global = true)]
    language_type: Option<String>,
    /// Line start (used by `dump` and `find`).
    #[clap(long = "ls", global = true)]
    line_start: Option<usize>,
    /// Line end (used by `dump` and `find`).
    #[clap(long = "le", global = true)]
    line_end: Option<usize>,
    /// Print warnings (skipped files, unrecognized languages).
    #[clap(long, short, global = true)]
    warning: bool,
    /// Disable auto-skip of files marked as generated (e.g. `@generated`,
    /// `DO NOT EDIT`, `GENERATED CODE` near the top). By default the CLI
    /// skips such files so generated bindings do not skew metrics.
    #[clap(long, global = true)]
    no_skip_generated: bool,
    /// Log a "skipped (generated): <path>" line to stderr for each file
    /// auto-skipped by the generated-code detector. Useful for auditing
    /// which files were excluded.
    #[clap(long, global = true)]
    report_skipped: bool,
    /// Existing preprocessor-data JSON to consume during C/C++ analysis.
    /// Use `bca preproc` to produce one.
    #[clap(long, value_parser, global = true)]
    preproc_data: Option<PathBuf>,
    /// Read newline-separated input paths from a file. Use `-` to read
    /// from stdin. Combined as a union with any `--paths` values; globs
    /// still apply. Blank lines are skipped; `#` is treated as a path
    /// character (not a comment). To pass a file literally named `-`,
    /// use `./-`.
    #[clap(long = "paths-from", value_parser, global = true)]
    paths_from: Option<PathBuf>,
    /// Disable `.gitignore` / `.ignore` / global gitignore awareness
    /// when expanding directory seeds. Explicit file paths are always
    /// honored regardless of this flag.
    #[clap(long = "no-ignore", global = true)]
    no_ignore: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compute per-file metrics and emit them in a structured format.
    Metrics(StructuredArgs),
    /// Extract per-file operands and operators.
    Ops(StructuredArgs),
    /// Generate an aggregated report across the analyzed source.
    Report(ReportArgs),
    /// Dump the AST to stdout.
    Dump,
    /// Find nodes of one or more types.
    Find(NodesArgs),
    /// Count nodes of one or more types.
    Count(NodesArgs),
    /// List functions/methods and their spans.
    Functions,
    /// Remove comments from source files.
    StripComments(StripCommentsArgs),
    /// Generate preprocessor-data JSON for C/C++ analysis.
    Preproc(PreprocArgs),
    /// List the metrics this tool can compute and exit.
    ListMetrics(ListMetricsArgs),
}

/// Shared shape for `metrics` and `ops`: same format set, same output
/// semantics (directory of per-file emissions; stdout if omitted).
#[derive(Args, Debug)]
struct StructuredArgs {
    /// Output format.
    #[clap(long, short = 'O', value_enum)]
    output_format: Option<MetricsFormat>,
    /// Output directory. Filenames mirror input paths plus the format
    /// extension. Stdout if omitted (CBOR requires this flag).
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Pretty-print JSON / TOML output.
    #[clap(long)]
    pretty: bool,
}

#[derive(Args, Debug)]
struct ReportArgs {
    /// Report format.
    #[clap(value_enum)]
    format: ReportFormat,
    /// Output file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Maximum number of entries per hotspot table.
    #[clap(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..))]
    top: u32,
    /// Path prefix to strip from displayed file paths.
    #[clap(long, default_value = "")]
    strip_prefix: String,
}

#[derive(Args, Debug)]
struct NodesArgs {
    /// Node-type names. Pass one or more, space-separated.
    #[clap(required = true, num_args = 1..)]
    nodes: Vec<String>,
}

#[derive(Args, Debug)]
struct StripCommentsArgs {
    /// Rewrite each input file in place instead of writing to stdout.
    #[clap(long)]
    in_place: bool,
}

#[derive(Args, Debug)]
struct PreprocArgs {
    /// Output JSON file. Stdout if omitted.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct ListMetricsArgs {
    /// What to print: `names` (one per line) or `descriptions`
    /// (name + one-line summary).
    #[clap(value_enum, default_value_t = ListMetricsMode::Names)]
    mode: ListMetricsMode,
}

/// What `act_on_file` should do per file. Drives the inner dispatch and
/// replaces the prior cluster of mutually-exclusive bool flags.
#[derive(Debug)]
enum Action {
    Dump,
    Metrics {
        format: Option<MetricsFormat>,
        pretty: bool,
    },
    Ops {
        format: Option<MetricsFormat>,
        pretty: bool,
    },
    StripComments {
        in_place: bool,
    },
    Functions,
    Find(Arc<[String]>),
    Count(Arc<[String]>),
    /// Same walk as `Metrics`, but taps each space tree to stream
    /// `FunctionSummary` records for the post-walk aggregator.
    Report,
    /// Walks source to accumulate preprocessor data (no per-file output).
    PreprocProduce,
}

#[derive(Debug)]
struct Config {
    action: Action,
    output: Option<PathBuf>,
    language: Option<LANG>,
    line_start: Option<usize>,
    line_end: Option<usize>,
    preproc_lock: Option<Arc<Mutex<PreprocResults>>>,
    preproc: Option<Arc<PreprocResults>>,
    count_lock: Option<Arc<Mutex<Count>>>,
    /// Sender for streaming `FunctionSummary` records when running `report`.
    /// Wrapped in `Mutex` because `mpsc::Sender` is `Send` but not `Sync`.
    markdown_tx: Option<Mutex<std::sync::mpsc::Sender<FunctionSummary>>>,
    /// Path prefix stripped from file paths in the markdown report.
    strip_prefix: String,
    warning: bool,
    /// When true, files whose head matches a generated-code marker are
    /// skipped before parsing. Defaults on; flipped off by
    /// `--no-skip-generated`.
    skip_generated: bool,
    /// When true, log a stderr line for each file auto-skipped by the
    /// generated-code detector. Also enabled by `warning` (which logs
    /// every skip reason); `report_skipped` is the dedicated flag for
    /// users who want the generated-skip audit without the rest of the
    /// warning stream.
    report_skipped: bool,
}

impl Config {
    /// Build a `Config` for `action`, populating the fields every command
    /// shares from `globals`. Per-command extras (`output`, `count_lock`,
    /// `markdown_tx`, `strip_prefix`) are set on the returned value at the
    /// call site.
    fn new(action: Action, globals: &GlobalOpts, preproc: Option<Arc<PreprocResults>>) -> Self {
        let language = resolve_language(globals.language_type.as_deref(), &action);
        Self {
            action,
            output: None,
            language,
            line_start: globals.line_start,
            line_end: globals.line_end,
            preproc_lock: None,
            preproc,
            count_lock: None,
            markdown_tx: None,
            strip_prefix: String::new(),
            warning: globals.warning,
            skip_generated: !globals.no_skip_generated,
            report_skipped: globals.report_skipped,
        }
    }
}

fn mk_globset(elems: Vec<String>) -> Result<GlobSet, String> {
    if elems.is_empty() {
        return Ok(GlobSet::empty());
    }

    let mut globset = GlobSetBuilder::new();
    for e in &elems {
        if e.is_empty() {
            continue;
        }
        globset.add(Glob::new(e).map_err(|err| format!("invalid glob pattern {e:?}: {err}"))?);
    }
    globset
        .build()
        .map_err(|err| format!("failed to build glob set: {err}"))
}

fn act_on_file(path: PathBuf, cfg: &Config) -> std::io::Result<()> {
    let Some(source) = read_file_with_eol(&path)? else {
        if cfg.warning {
            eprintln!("warning: skipping empty file: {}", path.display());
        }
        return Ok(());
    };

    // The generated-code skip runs before language detection so we don't
    // pay parse cost for files we'll discard. It's a CLI-level filter
    // (preproc has its own pipeline that genuinely needs every C/C++ file
    // walked), so leave Action::PreprocProduce alone.
    if cfg.skip_generated && !matches!(cfg.action, Action::PreprocProduce) && is_generated(&source)
    {
        if cfg.report_skipped || cfg.warning {
            eprintln!("skipped (generated): {}", path.display());
        }
        return Ok(());
    }

    let Some(language) = cfg.language.or_else(|| guess_language(&source, &path).0) else {
        if cfg.warning {
            eprintln!(
                "warning: skipping file with unrecognized language: {}",
                path.display()
            );
        }
        return Ok(());
    };

    let pr = cfg.preproc.clone();
    match &cfg.action {
        Action::Dump => {
            let dump_cfg = DumpCfg {
                line_start: cfg.line_start,
                line_end: cfg.line_end,
            };
            action::<Dump>(&language, source, &path, pr, dump_cfg)
        }
        Action::Metrics { format, pretty } => {
            if let Some(fmt) = format {
                if let Some(space) = get_function_spaces(&language, source, &path, pr) {
                    if fmt.requires_funcspace() {
                        // CSV (and any future per-file format with a
                        // metric-shaped row schema) takes a concrete
                        // &FuncSpace rather than going through the
                        // generic Serialize dispatch. Today CSV is
                        // the sole such format.
                        debug_assert!(matches!(fmt, MetricsFormat::Csv));
                        dump_csv(&space, path, cfg.output.as_ref())?;
                    } else {
                        fmt.dump(space, path, cfg.output.as_ref(), *pretty)?;
                    }
                }
                Ok(())
            } else {
                let metrics_cfg = MetricsCfg { path };
                let path = metrics_cfg.path.clone();
                action::<Metrics>(&language, source, &path, pr, metrics_cfg)
            }
        }
        Action::Ops { format, pretty } => {
            if let Some(fmt) = format {
                if let Some(ops) = get_ops(&language, source, &path, pr) {
                    fmt.dump(ops, path, cfg.output.as_ref(), *pretty)?;
                }
                Ok(())
            } else {
                let ops_cfg = OpsCfg { path };
                let path = ops_cfg.path.clone();
                action::<OpsCode>(&language, source, &path, pr, ops_cfg)
            }
        }
        Action::StripComments { in_place } => {
            let comment_cfg = CommentRmCfg {
                in_place: *in_place,
                path,
            };
            let path = comment_cfg.path.clone();
            // C++ comment removal goes through the dedicated Ccomment grammar
            // even when the file's primary language is Cpp.
            let lang = if language == LANG::Cpp {
                LANG::Ccomment
            } else {
                language
            };
            action::<CommentRm>(&lang, source, &path, pr, comment_cfg)
        }
        Action::Functions => {
            let fn_cfg = FunctionCfg { path: path.clone() };
            action::<Function>(&language, source, &path, pr, fn_cfg)
        }
        Action::Find(filters) => {
            let find_cfg = FindCfg {
                path: path.clone(),
                filters: Arc::clone(filters),
                line_start: cfg.line_start,
                line_end: cfg.line_end,
            };
            action::<Find>(&language, source, &path, pr, find_cfg)
        }
        Action::Count(filters) => {
            let stats = cfg
                .count_lock
                .clone()
                .expect("Count handler initializes count_lock before dispatch");
            let count_cfg = CountCfg {
                filters: Arc::clone(filters),
                stats,
            };
            action::<Count>(&language, source, &path, pr, count_cfg)
        }
        Action::Report => {
            if let Some(space) = get_function_spaces(&language, source, &path, pr)
                && let Some(ref tx) = cfg.markdown_tx
                && !matches!(language, LANG::Preproc | LANG::Ccomment)
            {
                let Some(file_str) = path.to_str() else {
                    if cfg.warning {
                        eprintln!(
                            "warning: skipping non-UTF-8 path in report: {}",
                            path.display()
                        );
                    }
                    return Ok(());
                };
                let mut summaries = Vec::new();
                extract_summaries(
                    &space,
                    file_str,
                    language,
                    &cfg.strip_prefix,
                    &mut summaries,
                );
                let Ok(sender) = tx.lock() else {
                    if cfg.warning {
                        eprintln!(
                            "warning: skipping {}: report channel lock poisoned",
                            path.display()
                        );
                    }
                    return Ok(());
                };
                for s in summaries {
                    let _ = sender.send(s);
                }
            }
            Ok(())
        }
        Action::PreprocProduce => {
            if let Some(preproc_lock) = &cfg.preproc_lock
                && let Some(language) = guess_language(&source, &path).0
                && language == LANG::Cpp
            {
                let mut results = preproc_lock.lock().expect("mutex not poisoned");
                preprocess(
                    &PreprocParser::new(source, &path, None),
                    &path,
                    &mut results,
                );
            }
            Ok(())
        }
    }
}

fn process_dir_path(all_files: &mut HashMap<String, Vec<PathBuf>>, path: &Path, cfg: &Config) {
    if !matches!(cfg.action, Action::PreprocProduce) {
        return;
    }
    let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let file_name = fname.to_string();
    match all_files.entry(file_name) {
        hash_map::Entry::Occupied(l) => {
            l.into_mut().push(path.to_path_buf());
        }
        hash_map::Entry::Vacant(p) => {
            p.insert(vec![path.to_path_buf()]);
        }
    }
}

fn resolve_language(typ: Option<&str>, action: &Action) -> Option<LANG> {
    // Force `Preproc` for the producer so `act_on_file`'s "skip
    // unrecognized" guard never fires — every walked file must reach the
    // dispatch where the producer runs its own Cpp check.
    if matches!(action, Action::PreprocProduce) {
        return Some(LANG::Preproc);
    }
    match typ.unwrap_or("") {
        "" => None,
        "ccomment" => Some(LANG::Ccomment),
        "preproc" => Some(LANG::Preproc),
        other => get_from_ext(other),
    }
}

fn resolve_num_jobs(requested: Option<usize>) -> usize {
    requested.map_or_else(
        || {
            std::cmp::max(
                2,
                available_parallelism()
                    .unwrap_or_else(|e| {
                        die(format_args!("could not get available parallelism: {e}"))
                    })
                    .get(),
            ) - 1
        },
        |num_jobs| std::cmp::max(2, num_jobs) - 1,
    )
}

/// Load existing preproc JSON for the consumer side. The producer side
/// (`bca preproc`) builds its own `Mutex<PreprocResults>` directly.
fn load_preproc_data(path: &Path) -> Arc<PreprocResults> {
    let data = read_file(path).unwrap_or_else(|e| {
        die(format_args!(
            "failed to read preproc data {}: {e}",
            path.display()
        ))
    });
    let parsed = serde_json::from_slice::<PreprocResults>(&data).unwrap_or_else(|e| {
        die(format_args!(
            "failed to parse preproc JSON from {}: {e}",
            path.display()
        ))
    });
    Arc::new(parsed)
}

/// Read newline-separated paths from `src` (a path on disk or `-`
/// for stdin). Skips blank/whitespace-only lines. `die`s on I/O
/// failure with the failing line number.
fn read_paths_from(src: &Path) -> Vec<PathBuf> {
    if src.as_os_str() == "-" {
        collect_path_lines(std::io::stdin().lock(), "--paths-from -")
    } else {
        let label = format!("--paths-from {}", src.display());
        let f = std::fs::File::open(src).unwrap_or_else(|e| die(format_args!("{label}: {e}")));
        collect_path_lines(std::io::BufReader::new(f), &label)
    }
}

/// Drain `reader` into `PathBuf`s, one per non-blank line. `die`s on
/// I/O failure, prefixing the message with `label` and the failing
/// line number so the caller can identify the source.
fn collect_path_lines<R: std::io::BufRead>(reader: R, label: &str) -> Vec<PathBuf> {
    reader
        .lines()
        .enumerate()
        .filter_map(|(i, r)| {
            let line = r.unwrap_or_else(|e| {
                die(format_args!("{label}: read error on line {}: {e}", i + 1))
            });
            let trimmed = line.trim();
            (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
        })
        .collect()
}

/// Expand seed paths for the walk: union `--paths` with
/// `--paths-from`, then for each seed:
///   - file → keep as-is (explicit override of any ignore rules);
///   - directory → expand via `ignore::WalkBuilder`, gitignore-aware
///     unless `no_ignore` is set.
///
/// Returns a flat `Vec<PathBuf>` of files. Include/exclude globs are
/// applied later by `explore()`, matching today's semantics.
fn expand_seed_paths(
    paths: Vec<PathBuf>,
    paths_from: Option<PathBuf>,
    no_ignore: bool,
) -> Vec<PathBuf> {
    use ignore::WalkBuilder;
    let mut seeds = paths;
    if let Some(src) = paths_from {
        seeds.extend(read_paths_from(&src));
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for seed in seeds {
        if !seed.exists() {
            // Match today's `explore()` behavior: warn, do not die.
            eprintln!("Warning: File doesn't exist: {}", seed.display());
            continue;
        }
        if seed.is_file() {
            out.push(seed);
            continue;
        }
        let mut wb = WalkBuilder::new(&seed);
        wb.hidden(true)
            .follow_links(false)
            .require_git(false)
            .git_ignore(!no_ignore)
            .git_exclude(!no_ignore)
            .git_global(!no_ignore)
            .ignore(!no_ignore)
            .parents(!no_ignore);
        for entry in wb.build() {
            let entry = entry
                .unwrap_or_else(|e| die(format_args!("walk error in {}: {e}", seed.display())));
            if entry.file_type().is_some_and(|t| t.is_file()) {
                out.push(entry.into_path());
            }
        }
    }
    out
}

fn run_walk(globals: GlobalOpts, cfg: Config) -> HashMap<String, Vec<PathBuf>> {
    let include = mk_globset(globals.include).unwrap_or_else(|e| die(e));
    let exclude = mk_globset(globals.exclude).unwrap_or_else(|e| die(e));
    let num_jobs = resolve_num_jobs(globals.num_jobs);
    let paths = expand_seed_paths(globals.paths, globals.paths_from, globals.no_ignore);
    let files_data = FilesData {
        include,
        exclude,
        paths,
    };
    ConcurrentRunner::new(num_jobs, act_on_file)
        .set_proc_dir_paths(process_dir_path)
        .run(cfg, files_data)
        .unwrap_or_else(|e| die(format_args!("{e:?}")))
}

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            if matches!(
                err.kind(),
                clap::error::ErrorKind::UnknownArgument
                    | clap::error::ErrorKind::InvalidSubcommand
                    | clap::error::ErrorKind::MissingSubcommand
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) && let Some(hint) = legacy_hint(std::env::args_os())
            {
                eprintln!("{hint}");
            }
            err.exit();
        }
    };

    let preproc = cli
        .globals
        .preproc_data
        .as_ref()
        .map(|p| load_preproc_data(p));

    match cli.command {
        Command::ListMetrics(args) => {
            let mut buf = Vec::new();
            write_metrics(&mut buf, args.mode).expect("writing to Vec<u8> is infallible");
            write_stdout_or_die(&buf);
        }
        Command::Dump => {
            let cfg = Config::new(Action::Dump, &cli.globals, preproc);
            run_walk(cli.globals, cfg);
        }
        Command::Functions => {
            let cfg = Config::new(Action::Functions, &cli.globals, preproc);
            run_walk(cli.globals, cfg);
        }
        Command::Metrics(args) => {
            if matches!(args.output_format, Some(MetricsFormat::Cbor)) && args.output.is_none() {
                die(CBOR_STDOUT_ERROR);
            }
            // Aggregated formats (e.g. checkstyle) emit a single
            // document covering every offender, so `--output` (when
            // present) names a *file*, not a directory. The threshold
            // producer (#96) is not wired yet — for now we emit an
            // empty offender list, which yields a well-formed, stable
            // document that CI consumers can already integrate against.
            if matches!(args.output_format, Some(fmt) if fmt.is_aggregated()) {
                if let Some(ref out) = args.output
                    && out.exists()
                    && out.is_dir()
                {
                    die(
                        "--output must be a file path for aggregated formats (e.g. checkstyle, sarif, clang-warning, msvc-warning)",
                    );
                }
                match args.output_format {
                    Some(MetricsFormat::Sarif) => dump_sarif(&[], args.output.as_deref())
                        .unwrap_or_else(|e| die(format_args!("failed to write sarif: {e}"))),
                    Some(MetricsFormat::ClangWarning) => {
                        dump_clang_warning(&[], args.output.as_deref()).unwrap_or_else(|e| {
                            die(format_args!("failed to write clang-warning: {e}"))
                        })
                    }
                    Some(MetricsFormat::MsvcWarning) => {
                        dump_msvc_warning(&[], args.output.as_deref()).unwrap_or_else(|e| {
                            die(format_args!("failed to write msvc-warning: {e}"))
                        })
                    }
                    _ => dump_checkstyle(&[], args.output.as_deref())
                        .unwrap_or_else(|e| die(format_args!("failed to write checkstyle: {e}"))),
                }
                return;
            }
            if args.output_format.is_some()
                && let Some(ref out) = args.output
                && out.exists()
                && !out.is_dir()
            {
                die("--output must be a directory for `metrics`");
            }
            let action = Action::Metrics {
                format: args.output_format,
                pretty: args.pretty,
            };
            let cfg = Config {
                output: args.output,
                ..Config::new(action, &cli.globals, preproc)
            };
            run_walk(cli.globals, cfg);
        }
        Command::Ops(args) => {
            if matches!(args.output_format, Some(MetricsFormat::Cbor)) && args.output.is_none() {
                die(CBOR_STDOUT_ERROR);
            }
            if matches!(args.output_format, Some(fmt) if fmt.is_aggregated()) {
                die(
                    "aggregated formats (checkstyle, sarif, clang-warning, msvc-warning) are not supported by `ops`; use `bca metrics --output-format <fmt>`",
                );
            }
            if matches!(args.output_format, Some(fmt) if fmt.requires_funcspace()) {
                die(
                    "CSV is not supported by `ops` because its column schema is metric-shaped; use `bca metrics --output-format csv`",
                );
            }
            if args.output_format.is_some()
                && let Some(ref out) = args.output
                && out.exists()
                && !out.is_dir()
            {
                die("--output must be a directory for `ops`");
            }
            let action = Action::Ops {
                format: args.output_format,
                pretty: args.pretty,
            };
            let cfg = Config {
                output: args.output,
                ..Config::new(action, &cli.globals, preproc)
            };
            run_walk(cli.globals, cfg);
        }
        Command::Report(args) => {
            if let Some(ref output) = args.output {
                if output.exists() && output.is_dir() {
                    die("--output must be a file path for `report`");
                }
                if let Some(parent) = output.parent()
                    && !parent.as_os_str().is_empty()
                    && !parent.exists()
                {
                    die(format_args!(
                        "parent directory of --output does not exist: {}",
                        parent.display()
                    ));
                }
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let cfg = Config {
                markdown_tx: Some(Mutex::new(tx)),
                strip_prefix: args.strip_prefix,
                ..Config::new(Action::Report, &cli.globals, preproc)
            };
            run_walk(cli.globals, cfg);

            // ConcurrentRunner::run() consumed Config (and thus the Sender).
            // All worker threads have joined, so `rx.into_iter()` terminates.
            let summaries: Vec<FunctionSummary> = rx.into_iter().collect();
            let report = match args.format {
                ReportFormat::Markdown => generate_report(&summaries, args.top as usize),
            };
            if let Some(ref output_path) = args.output {
                std::fs::write(output_path, &report).unwrap_or_else(|e| {
                    die(format_args!(
                        "failed to write to {}: {e}",
                        output_path.display()
                    ))
                });
            } else {
                write_stdout_or_die(report.as_bytes());
            }
        }
        Command::Find(args) => {
            let cfg = Config::new(Action::Find(args.nodes.into()), &cli.globals, preproc);
            run_walk(cli.globals, cfg);
        }
        Command::Count(args) => {
            let count_lock = Arc::new(Mutex::new(Count::default()));
            let cfg = Config {
                count_lock: Some(count_lock.clone()),
                ..Config::new(Action::Count(args.nodes.into()), &cli.globals, preproc)
            };
            run_walk(cli.globals, cfg);

            let count = Arc::try_unwrap(count_lock)
                .expect("all worker threads have joined; Arc refcount is 1")
                .into_inner()
                .expect("mutex not poisoned");
            println!("{count}");
        }
        Command::StripComments(args) => {
            let action = Action::StripComments {
                in_place: args.in_place,
            };
            let cfg = Config::new(action, &cli.globals, preproc);
            run_walk(cli.globals, cfg);
        }
        Command::Preproc(args) => {
            let preproc_lock = Arc::new(Mutex::new(PreprocResults::default()));
            let output = args.output;
            let cfg = Config {
                preproc_lock: Some(preproc_lock.clone()),
                ..Config::new(Action::PreprocProduce, &cli.globals, None)
            };
            let all_files = run_walk(cli.globals, cfg);

            let mut data = Arc::try_unwrap(preproc_lock)
                .expect("all worker threads have joined; Arc refcount is 1")
                .into_inner()
                .expect("mutex not poisoned");
            fix_includes(&mut data.files, &all_files);

            let serialized = serde_json::to_string(&data)
                .unwrap_or_else(|e| die(format_args!("failed to serialize preproc data: {e}")));
            if let Some(output_path) = output {
                write_file(&output_path, serialized.as_bytes()).unwrap_or_else(|e| {
                    die(format_args!(
                        "failed to write output to {}: {e}",
                        output_path.display()
                    ))
                });
            } else {
                println!("{serialized}");
            }
        }
    }
}

/// Names of every subcommand on the new CLI. Kept in sync with the
/// `Command` enum by `tests::subcommands_match_command_enum`, which
/// fails if the two ever drift.
const SUBCOMMANDS: &[&str] = &[
    "metrics",
    "ops",
    "report",
    "dump",
    "find",
    "count",
    "functions",
    "strip-comments",
    "preproc",
    "list-metrics",
];

/// If `argv` looks like an invocation of the pre-restructure CLI, return a
/// hint pointing the user at the new equivalent. Called only when clap
/// rejects the input, so the goal is to make the failure actionable.
///
/// The hint is best-effort and conservative: it triggers only on tokens
/// that are unambiguously legacy (action flags removed in the rewrite, or
/// `-O markdown` whose value no longer exists on `metrics`).
fn legacy_hint(argv: impl IntoIterator<Item = OsString>) -> Option<String> {
    let args: Vec<String> = argv
        .into_iter()
        .skip(1) // program name
        .filter_map(|s| s.into_string().ok())
        .collect();
    if args.is_empty() {
        return None;
    }

    // If the user invoked a known new-CLI subcommand, they're not on the
    // legacy CLI; stay quiet so we don't second-guess legitimate args
    // that happen to look like old flags (e.g. `find --dump` where the
    // user intended `--dump` as a positional node-type value).
    if args.iter().any(|a| SUBCOMMANDS.contains(&a.as_str())) {
        return None;
    }

    // Action flags removed by the rewrite. Each one is unambiguously legacy.
    let action_map: &[(&str, &str)] = &[
        ("--metrics", "bca metrics"),
        ("-m", "bca metrics"),
        ("--ops", "bca ops"),
        ("--dump", "bca dump"),
        ("-d", "bca dump"),
        ("--comments", "bca strip-comments [--in-place]"),
        ("--function", "bca functions"),
        ("-F", "bca functions"),
        ("--find", "bca find <NODE> [<NODE>...]"),
        ("-f", "bca find <NODE> [<NODE>...]"),
        ("--count", "bca count <NODE> [<NODE>...]"),
        ("-C", "bca count <NODE> [<NODE>...]"),
        ("--list-metrics", "bca list-metrics [names|descriptions]"),
        (
            "--preproc",
            "bca preproc -o OUT.json  (or --preproc-data on consumers)",
        ),
    ];

    let mut lines: Vec<String> = Vec::new();
    let mut saw_legacy_action = false;

    for arg in &args {
        let head = arg.split('=').next().unwrap_or(arg);
        if let Some((_, replacement)) = action_map.iter().find(|(old, _)| *old == head) {
            saw_legacy_action = true;
            lines.push(format!("  {head}  ->  {replacement}"));
        }
    }

    // -O markdown / --output-format markdown is the canonical legacy form
    // for the aggregated report. `markdown` is no longer a valid metrics
    // format value, so seeing it here is unambiguous.
    let format_value = args.iter().enumerate().find_map(|(i, a)| {
        let s = a.as_str();
        if s == "-O" || s == "--output-format" {
            args.get(i + 1).map(String::as_str)
        } else if let Some(rest) = s.strip_prefix("--output-format=") {
            Some(rest)
        } else {
            s.strip_prefix("-O").filter(|r| !r.is_empty())
        }
    });
    if format_value == Some("markdown") {
        saw_legacy_action = true;
        lines.push(String::from(
            "  -O markdown  ->  bca report markdown [--top N] [--strip-prefix P]",
        ));
    } else if let Some(fmt) = format_value
        && saw_legacy_action
    {
        // Only suggest a metrics-format mapping when we already confirmed
        // this is a legacy invocation; otherwise `-O json` survives in the
        // new CLI and we shouldn't second-guess it.
        lines.push(format!("  -O {fmt}  ->  bca metrics -O {fmt}"));
    }

    if !saw_legacy_action {
        return None;
    }

    let mut hint = String::from(
        "note: the CLI was restructured into subcommands. See migration.md for the full mapping.\n",
    );
    for line in &lines {
        hint.push_str(line);
        hint.push('\n');
    }
    hint.push_str("  Run `bca --help` for the new command list.\n");
    Some(hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(action: Action) -> Config {
        Config {
            action,
            output: None,
            language: None,
            line_start: None,
            line_end: None,
            preproc_lock: None,
            preproc: None,
            count_lock: None,
            markdown_tx: None,
            strip_prefix: String::new(),
            warning: false,
            skip_generated: true,
            report_skipped: false,
        }
    }

    #[test]
    fn process_dir_path_noop_outside_preproc() {
        let cfg = test_config(Action::Dump);
        let mut all_files = HashMap::new();
        process_dir_path(&mut all_files, Path::new("/some/file.cpp"), &cfg);
        assert!(all_files.is_empty());
    }

    #[test]
    fn process_dir_path_inserts_valid_utf8_filename() {
        let cfg = test_config(Action::PreprocProduce);
        let mut all_files = HashMap::new();
        process_dir_path(&mut all_files, Path::new("/some/dir/foo.cpp"), &cfg);
        assert_eq!(all_files.len(), 1);
        assert_eq!(
            all_files["foo.cpp"],
            vec![PathBuf::from("/some/dir/foo.cpp")]
        );
    }

    #[test]
    fn process_dir_path_groups_duplicate_filenames() {
        let cfg = test_config(Action::PreprocProduce);
        let mut all_files = HashMap::new();
        process_dir_path(&mut all_files, Path::new("/a/foo.cpp"), &cfg);
        process_dir_path(&mut all_files, Path::new("/b/foo.cpp"), &cfg);
        assert_eq!(all_files.len(), 1);
        assert_eq!(
            all_files["foo.cpp"],
            vec![PathBuf::from("/a/foo.cpp"), PathBuf::from("/b/foo.cpp")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_dir_path_skips_non_utf8_filename() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let cfg = test_config(Action::PreprocProduce);
        let mut all_files = HashMap::new();
        let bad_name = OsStr::from_bytes(b"\xff\xfe");
        let path = PathBuf::from("/some/dir").join(bad_name);
        process_dir_path(&mut all_files, &path, &cfg);
        assert!(all_files.is_empty());
    }

    // CLI parsing tests. The shape is now subcommand-driven, so these
    // exercise the shape of the top-level parser, not the legacy flag
    // mutual-exclusion rules.

    fn parse(args: &[&str]) -> clap::error::Result<Cli> {
        Cli::try_parse_from(std::iter::once(&"cli").chain(args.iter()))
    }

    #[test]
    fn no_subcommand_prints_help() {
        // arg_required_else_help: no args -> clap prints help and exits.
        // We just check parsing fails (either DisplayHelp or MissingSubcommand).
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn metrics_alone_parses() {
        assert!(parse(&["metrics"]).is_ok());
    }

    #[test]
    fn metrics_with_format_parses() {
        assert!(parse(&["metrics", "-O", "json"]).is_ok());
    }

    #[test]
    fn metrics_accepts_checkstyle_format() {
        assert!(parse(&["metrics", "-O", "checkstyle"]).is_ok());
    }

    #[test]
    fn metrics_accepts_sarif_format() {
        assert!(parse(&["metrics", "-O", "sarif"]).is_ok());
    }

    #[test]
    fn metrics_accepts_clang_warning_format() {
        assert!(parse(&["metrics", "-O", "clang-warning"]).is_ok());
    }

    #[test]
    fn metrics_accepts_msvc_warning_format() {
        assert!(parse(&["metrics", "-O", "msvc-warning"]).is_ok());
    }

    #[test]
    fn ops_rejects_checkstyle_format_at_runtime() {
        // clap parses it (Checkstyle is on the shared MetricsFormat
        // enum), but `ops` rejects it at dispatch with a die() — we
        // can only assert parsing here without spawning a process.
        assert!(parse(&["ops", "-O", "checkstyle"]).is_ok());
    }

    #[test]
    fn metrics_rejects_markdown_format() {
        // ReportFormat::Markdown is not in MetricsFormat by construction.
        assert!(parse(&["metrics", "-O", "markdown"]).is_err());
    }

    #[test]
    fn metrics_rejects_top_flag() {
        // --top lives only on `report`.
        assert!(parse(&["metrics", "--top", "5"]).is_err());
    }

    #[test]
    fn metrics_rejects_strip_prefix_flag() {
        assert!(parse(&["metrics", "--strip-prefix", "/x"]).is_err());
    }

    #[test]
    fn report_markdown_parses() {
        assert!(parse(&["report", "markdown"]).is_ok());
    }

    #[test]
    fn report_requires_format() {
        assert!(parse(&["report"]).is_err());
    }

    #[test]
    fn report_with_top_and_strip_prefix() {
        assert!(parse(&["report", "markdown", "--top", "10", "--strip-prefix", "/x/"]).is_ok());
    }

    #[test]
    fn report_top_zero_rejected() {
        assert!(parse(&["report", "markdown", "--top", "0"]).is_err());
    }

    #[test]
    fn ops_parses() {
        assert!(parse(&["ops", "-O", "json"]).is_ok());
    }

    #[test]
    fn dump_parses() {
        assert!(parse(&["dump"]).is_ok());
    }

    #[test]
    fn find_requires_a_node() {
        assert!(parse(&["find"]).is_err());
        assert!(parse(&["find", "call_expression"]).is_ok());
    }

    #[test]
    fn count_requires_a_node() {
        assert!(parse(&["count"]).is_err());
        assert!(parse(&["count", "if_statement"]).is_ok());
    }

    #[test]
    fn functions_parses() {
        assert!(parse(&["functions"]).is_ok());
    }

    #[test]
    fn strip_comments_parses() {
        assert!(parse(&["strip-comments"]).is_ok());
        assert!(parse(&["strip-comments", "--in-place"]).is_ok());
    }

    #[test]
    fn preproc_parses() {
        assert!(parse(&["preproc"]).is_ok());
        assert!(parse(&["preproc", "-o", "/tmp/x.json"]).is_ok());
    }

    #[test]
    fn list_metrics_parses() {
        let cli = parse(&["list-metrics"]).expect("parses");
        assert!(matches!(cli.command, Command::ListMetrics(_)));
    }

    #[test]
    fn list_metrics_with_descriptions() {
        let cli = parse(&["list-metrics", "descriptions"]).expect("parses");
        match cli.command {
            Command::ListMetrics(args) => assert_eq!(args.mode, ListMetricsMode::Descriptions),
            _ => panic!("expected ListMetrics"),
        }
    }

    #[test]
    fn list_metrics_invalid_mode_rejected() {
        assert!(parse(&["list-metrics", "bogus"]).is_err());
    }

    #[test]
    fn global_paths_works_before_or_after_subcommand() {
        assert!(parse(&["--paths", "x", "metrics"]).is_ok());
        assert!(parse(&["metrics", "--paths", "x"]).is_ok());
    }

    fn os_args(args: &[&str]) -> Vec<OsString> {
        args.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn legacy_hint_recognizes_old_metrics() {
        let hint = legacy_hint(os_args(&["cli", "--metrics", "-O", "markdown"])).expect("hint");
        assert!(hint.contains("report markdown"), "{hint}");
        assert!(hint.contains("--metrics"), "{hint}");
    }

    #[test]
    fn legacy_hint_recognizes_output_format_json_with_legacy_action() {
        // -O json next to --metrics is unambiguously legacy and should
        // map to `bca metrics -O json`.
        let hint = legacy_hint(os_args(&["cli", "-m", "--output-format", "json"])).expect("hint");
        assert!(hint.contains("metrics -O json"), "{hint}");
    }

    #[test]
    fn legacy_hint_returns_none_for_clean_args() {
        // Valid new-CLI args that just happen to also contain `-O` should
        // not trigger a legacy hint.
        let hint = legacy_hint(os_args(&["cli", "metrics", "-O", "json"]));
        assert!(hint.is_none());
    }

    #[test]
    fn legacy_hint_returns_none_for_no_args() {
        let hint = legacy_hint(os_args(&["cli"]));
        assert!(hint.is_none());
    }

    #[test]
    fn legacy_hint_recognizes_dash_o_markdown_alone() {
        // -O markdown is unambiguously legacy: markdown is not a
        // MetricsFormat value, so this pattern can only have come from the
        // pre-restructure CLI.
        let hint = legacy_hint(os_args(&["cli", "-O", "markdown"])).expect("hint");
        assert!(hint.contains("report markdown"), "{hint}");
    }

    #[test]
    fn legacy_hint_quiet_when_user_invoked_known_subcommand() {
        // `bca find --dump` — user wants `--dump` as a positional node
        // type, not a legacy flag. Presence of a known subcommand (`find`)
        // suppresses the hint; clap's own "to pass '--dump' as a value,
        // use '-- --dump'" tip remains the right guidance.
        let hint = legacy_hint(os_args(&["cli", "find", "--dump"]));
        assert!(hint.is_none());
    }

    #[test]
    fn legacy_hint_recognizes_dash_d() {
        // -d was the short form of --dump in the legacy CLI.
        let hint = legacy_hint(os_args(&["cli", "-d", "--paths", "."])).expect("hint");
        assert!(hint.contains("bca dump"), "{hint}");
    }

    /// Sanity: `Cli::command()` builds without panicking. Catches misconfigured
    /// derive attributes (e.g., conflicting short flags) at test time.
    #[test]
    fn cli_is_well_formed() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    /// `SUBCOMMANDS` (used by `legacy_hint` to gate the migration message)
    /// must list every variant of the `Command` enum. If a future verb is
    /// added to `Command` and this list is not updated, `legacy_hint` will
    /// false-positive on that verb's arguments.
    #[test]
    fn subcommands_match_command_enum() {
        use clap::CommandFactory;
        use std::collections::HashSet;
        let from_clap: HashSet<String> = Cli::command()
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .filter(|n| n != "help") // clap auto-generates `help`
            .collect();
        let from_const: HashSet<String> = SUBCOMMANDS.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(
            from_clap,
            from_const,
            "SUBCOMMANDS const drifted from Command enum: \
             missing from const = {missing:?}, missing from enum = {extra:?}",
            missing = from_clap.difference(&from_const).collect::<Vec<_>>(),
            extra = from_const.difference(&from_clap).collect::<Vec<_>>(),
        );
    }
}
