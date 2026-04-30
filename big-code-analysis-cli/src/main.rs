mod formats;
mod markdown_report;

use std::cmp::Ordering;
use std::collections::{HashMap, hash_map};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread::available_parallelism;

use clap::{ArgGroup, Parser};
use globset::{Glob, GlobSet, GlobSetBuilder};

use formats::Format;
use markdown_report::{FunctionSummary, extract_summaries, generate_report};

// Enums
use big_code_analysis::LANG;

// Structs
use big_code_analysis::{
    CommentRm, CommentRmCfg, ConcurrentRunner, Count, CountCfg, Dump, DumpCfg, FilesData, Find,
    FindCfg, Function, FunctionCfg, Metrics, MetricsCfg, OpsCfg, OpsCode, PreprocParser,
    PreprocResults,
};

// Functions
use big_code_analysis::{
    action, fix_includes, get_from_ext, get_function_spaces, get_ops, guess_language, preprocess,
    read_file, read_file_with_eol, write_file,
};

// Traits
use big_code_analysis::ParserTrait;

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("Error: {msg}");
    process::exit(1);
}

#[derive(Debug)]
struct Config {
    dump: bool,
    in_place: bool,
    comments: bool,
    find_filter: Vec<String>,
    count_filter: Vec<String>,
    language: Option<LANG>,
    function: bool,
    metrics: bool,
    ops: bool,
    output_format: Option<Format>,
    output: Option<PathBuf>,
    pretty: bool,
    line_start: Option<usize>,
    line_end: Option<usize>,
    preproc_lock: Option<Arc<Mutex<PreprocResults>>>,
    preproc: Option<Arc<PreprocResults>>,
    count_lock: Option<Arc<Mutex<Count>>>,
    /// Sender for streaming `FunctionSummary` records when `-O markdown` is active.
    /// Wrapped in `Mutex` because `mpsc::Sender` is `Send` but not `Sync`.
    markdown_tx: Option<Mutex<std::sync::mpsc::Sender<FunctionSummary>>>,
    /// Path prefix stripped from file paths in the markdown report.
    strip_prefix: String,
    warning: bool,
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
    if cfg.dump {
        let cfg = DumpCfg {
            line_start: cfg.line_start,
            line_end: cfg.line_end,
        };
        action::<Dump>(&language, source, &path, pr, cfg)
    } else if cfg.metrics {
        if let Some(output_format) = &cfg.output_format {
            if let Some(space) = get_function_spaces(&language, source, &path, pr) {
                // Skip internal pseudo-languages that don't produce standalone metrics.
                if let Some(ref tx) = cfg.markdown_tx
                    && !matches!(language, LANG::Preproc | LANG::Ccomment)
                    && let Some(file_str) = path.to_str()
                {
                    let mut summaries = Vec::new();
                    extract_summaries(
                        &space,
                        file_str,
                        language,
                        &cfg.strip_prefix,
                        &mut summaries,
                    );
                    if let Ok(sender) = tx.lock() {
                        for s in summaries {
                            let _ = sender.send(s);
                        }
                    }
                }
                output_format.dump_formats(space, path, cfg.output.as_ref(), cfg.pretty)?;
            }
            Ok(())
        } else {
            let cfg = MetricsCfg { path };
            let path = cfg.path.clone();
            action::<Metrics>(&language, source, &path, pr, cfg)
        }
    } else if cfg.ops {
        if let Some(output_format) = &cfg.output_format {
            if let Some(ops) = get_ops(&language, source, &path, pr) {
                output_format.dump_formats(ops, path, cfg.output.as_ref(), cfg.pretty)?;
            }
            Ok(())
        } else {
            let cfg = OpsCfg { path };
            let path = cfg.path.clone();
            action::<OpsCode>(&language, source, &path, pr, cfg)
        }
    } else if cfg.comments {
        let cfg = CommentRmCfg {
            in_place: cfg.in_place,
            path,
        };
        let path = cfg.path.clone();
        if language == LANG::Cpp {
            action::<CommentRm>(&LANG::Ccomment, source, &path, pr, cfg)
        } else {
            action::<CommentRm>(&language, source, &path, pr, cfg)
        }
    } else if cfg.function {
        let cfg = FunctionCfg { path: path.clone() };
        action::<Function>(&language, source, &path, pr, cfg)
    } else if !cfg.find_filter.is_empty() {
        let cfg = FindCfg {
            path: path.clone(),
            filters: cfg.find_filter.clone(),
            line_start: cfg.line_start,
            line_end: cfg.line_end,
        };
        action::<Find>(&language, source, &path, pr, cfg)
    } else if let Some(count_lock) = &cfg.count_lock {
        let cfg = CountCfg {
            filters: cfg.count_filter.clone(),
            stats: count_lock.clone(),
        };
        action::<Count>(&language, source, &path, pr, cfg)
    } else if let Some(preproc_lock) = &cfg.preproc_lock {
        if let Some(language) = guess_language(&source, &path).0
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
    } else {
        Ok(())
    }
}

fn process_dir_path(all_files: &mut HashMap<String, Vec<PathBuf>>, path: &Path, cfg: &Config) {
    if cfg.preproc_lock.is_some() {
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
        };
    }
}

#[derive(Parser, Debug)]
#[clap(
    name = "big-code-analysis-cli",
    version,
    author,
    about = "Analyze source code.",
    // The "action" group enforces that at most one action flag is supplied.
    // Action flags select what the tool does to each file. `--preproc` is
    // intentionally outside this group: with two or more paths it is a
    // standalone scan action, but with a single path it acts as a modifier
    // that loads preprocessor metadata to enrich another action (typically
    // `--metrics`). Modifier flags (--paths, --include, --exclude, --output,
    // --output-format, --language, --num-jobs, --in-place, --pr,
    // --line-start, --line-end, --warning, --top, --strip-prefix) refine the
    // chosen action and may freely combine with any of them.
    //
    // `required(true)` would not accommodate `--preproc`'s dual role, so the
    // "you must pick an action" check is enforced programmatically in
    // `main()` before any work begins.
    group(ArgGroup::new("action")
        .args(["dump", "comments", "find", "function", "count", "metrics", "ops"])
        .multiple(false)
        .required(false)),
    group(ArgGroup::new("format_action")
        .args(["metrics", "ops"])
        .required(false)),
)]
struct Opts {
    /// Input files to analyze.
    #[clap(long, short, value_parser)]
    paths: Vec<PathBuf>,
    /// Output AST to stdout.
    #[clap(long, short)]
    dump: bool,
    /// Remove comments in the specified files.
    #[clap(long, short)]
    comments: bool,
    /// Find nodes of the given type.
    #[clap(long, short, number_of_values = 1)]
    find: Vec<String>,
    /// Get functions and their spans.
    #[clap(long, short = 'F')]
    function: bool,
    /// Count nodes of the given type: comma separated list.
    #[clap(long, short = 'C', number_of_values = 1)]
    count: Vec<String>,
    /// Compute different metrics.
    #[clap(long, short)]
    metrics: bool,
    /// Retrieve all operands and operators in a code.
    #[clap(long)]
    ops: bool,
    /// Do action in place.
    #[clap(long, short)]
    in_place: bool,
    /// Glob to include files.
    #[clap(long, short = 'I', num_args(0..))]
    include: Vec<String>,
    /// Glob to exclude files.
    #[clap(long, short = 'X', num_args(0..))]
    exclude: Vec<String>,
    /// Number of jobs.
    #[clap(long, short = 'j')]
    num_jobs: Option<usize>,
    /// Language type.
    #[clap(long, short)]
    language_type: Option<String>,
    /// Output metrics as different formats.
    #[clap(long, short = 'O', value_enum, requires = "format_action")]
    output_format: Option<Format>,
    /// Dump a pretty json file.
    #[clap(long = "pr")]
    pretty: bool,
    /// Output file/directory.
    #[clap(long, short, value_parser)]
    output: Option<PathBuf>,
    /// Get preprocessor declaration for C/C++.
    #[clap(long, value_parser, number_of_values = 1)]
    preproc: Vec<PathBuf>,
    /// Line start.
    #[clap(long = "ls")]
    line_start: Option<usize>,
    /// Line end.
    #[clap(long = "le")]
    line_end: Option<usize>,
    /// Print the warnings.
    #[clap(long, short)]
    warning: bool,
    /// Maximum number of functions to include in the markdown report.
    #[clap(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..))]
    top: u32,
    /// Path prefix to strip from file paths in the markdown report.
    #[clap(long, default_value = "")]
    strip_prefix: String,
}

fn main() {
    let opts = Opts::parse();

    // Require at least one action. The clap "action" ArgGroup enforces
    // mutual exclusion among dump/comments/find/function/count/metrics/ops,
    // but `--preproc` lives outside the group because it doubles as a
    // modifier when given a single path. Reject the no-action case here so
    // users get a clear error instead of silent success.
    // Keep this list in sync with the "action" ArgGroup args([...]) above.
    let has_action = opts.dump
        || opts.metrics
        || opts.ops
        || opts.comments
        || opts.function
        || !opts.find.is_empty()
        || !opts.count.is_empty()
        || !opts.preproc.is_empty();
    if !has_action {
        die(
            "no action specified; pass one of --dump, --metrics, --ops, --comments, \
             --function, --find, --count, or --preproc",
        );
    }

    let count_lock = if !opts.count.is_empty() {
        Some(Arc::new(Mutex::new(Count::default())))
    } else {
        None
    };

    let (preproc_lock, preproc) = match opts.preproc.len().cmp(&1) {
        Ordering::Equal => {
            let data = read_file(&opts.preproc[0]).unwrap_or_else(|e| {
                die(format_args!(
                    "failed to read preproc file {}: {e}",
                    opts.preproc[0].display()
                ))
            });
            eprintln!("Load preproc data");
            let preproc_results =
                serde_json::from_slice::<PreprocResults>(&data).unwrap_or_else(|e| {
                    die(format_args!(
                        "failed to parse preproc JSON from {}: {e}",
                        opts.preproc[0].display()
                    ))
                });
            let x = (None, Some(Arc::new(preproc_results)));
            eprintln!("Load preproc data: finished");
            x
        }
        Ordering::Greater => (Some(Arc::new(Mutex::new(PreprocResults::default()))), None),
        Ordering::Less => (None, None),
    };

    let is_markdown = matches!(opts.output_format, Some(Format::Markdown));

    // Pre-run validation for markdown format.
    // Most incompatible flag combinations are caught at parse time by the
    // "action" and "format_action" ArgGroups, but -O markdown + --ops slips
    // through because --ops is in format_action.
    if is_markdown {
        // -O markdown with --ops is accepted by clap (ops is in format_action)
        // but markdown reports only work with --metrics.
        if opts.ops {
            die("-O markdown is incompatible with --ops");
        }

        // Validate --output for markdown: must be a file path, not a directory.
        if let Some(ref output) = opts.output {
            if output.is_dir() {
                die("--output must be a file path when -O markdown is used");
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
    }

    // Pre-run validation for CBOR format: binary output requires --output.
    if matches!(opts.output_format, Some(Format::Cbor)) && opts.output.is_none() {
        die(formats::CBOR_STDOUT_ERROR);
    }

    let output_is_dir = opts.output.as_ref().is_some_and(|p| p.is_dir());
    if !is_markdown && (opts.metrics || opts.ops) && opts.output.is_some() && !output_is_dir {
        die("The output parameter must be a directory");
    }

    let typ = opts.language_type.unwrap_or_default();
    let language = if preproc_lock.is_some() {
        Some(LANG::Preproc)
    } else if typ.is_empty() {
        None
    } else if typ == "ccomment" {
        Some(LANG::Ccomment)
    } else if typ == "preproc" {
        Some(LANG::Preproc)
    } else {
        get_from_ext(&typ)
    };

    let num_jobs = opts
        .num_jobs
        .map(|num_jobs| std::cmp::max(2, num_jobs) - 1)
        .unwrap_or_else(|| {
            std::cmp::max(
                2,
                available_parallelism()
                    .unwrap_or_else(|e| {
                        die(format_args!("could not get available parallelism: {e}"))
                    })
                    .get(),
            ) - 1
        });

    let include = mk_globset(opts.include).unwrap_or_else(|e| die(e));
    let exclude = mk_globset(opts.exclude).unwrap_or_else(|e| die(e));

    let (markdown_tx, markdown_rx) = if is_markdown {
        let (tx, rx) = std::sync::mpsc::channel::<FunctionSummary>();
        (Some(Mutex::new(tx)), Some(rx))
    } else {
        (None, None)
    };

    let cfg = Config {
        dump: opts.dump,
        in_place: opts.in_place,
        comments: opts.comments,
        find_filter: opts.find,
        count_filter: opts.count,
        language,
        function: opts.function,
        metrics: opts.metrics,
        ops: opts.ops,
        output_format: opts.output_format,
        pretty: opts.pretty,
        output: opts.output.clone(),
        line_start: opts.line_start,
        line_end: opts.line_end,
        preproc_lock: preproc_lock.clone(),
        preproc,
        count_lock: count_lock.clone(),
        markdown_tx,
        strip_prefix: opts.strip_prefix,
        warning: opts.warning,
    };

    let files_data = FilesData {
        include,
        exclude,
        paths: opts.paths,
    };

    let all_files = ConcurrentRunner::new(num_jobs, act_on_file)
        .set_proc_dir_paths(process_dir_path)
        .run(cfg, files_data)
        .unwrap_or_else(|e| die(format_args!("{e:?}")));

    if let Some(count) = count_lock {
        let count = Arc::try_unwrap(count)
            .expect("all worker threads have joined; Arc refcount is 1")
            .into_inner()
            .expect("mutex not poisoned");
        println!("{count}");
    }

    if is_markdown {
        // ConcurrentRunner::run() consumed Config (and thus the Sender).
        // All worker threads have joined, so the Sender is dropped and
        // rx.into_iter() will terminate.
        let summaries: Vec<FunctionSummary> = markdown_rx
            .map(|rx| rx.into_iter().collect())
            .unwrap_or_default();

        let report = generate_report(&summaries, opts.top as usize);
        if let Some(ref output_path) = opts.output {
            std::fs::write(output_path, &report).unwrap_or_else(|e| {
                die(format_args!(
                    "failed to write to {}: {e}",
                    output_path.display()
                ))
            });
        } else if let Err(e) = std::io::stdout().lock().write_all(report.as_bytes())
            && e.kind() != ErrorKind::BrokenPipe
        {
            die(e);
        }
        return;
    }

    if let Some(preproc) = preproc_lock {
        let mut data = Arc::try_unwrap(preproc)
            .expect("all worker threads have joined; Arc refcount is 1")
            .into_inner()
            .expect("mutex not poisoned");
        fix_includes(&mut data.files, &all_files);

        let data = serde_json::to_string(&data)
            .unwrap_or_else(|e| die(format_args!("failed to serialize preproc data: {e}")));
        if let Some(output_path) = opts.output {
            write_file(&output_path, data.as_bytes()).unwrap_or_else(|e| {
                die(format_args!(
                    "failed to write output to {}: {e}",
                    output_path.display()
                ))
            });
        } else {
            println!("{data}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(preproc_lock: Option<Arc<Mutex<PreprocResults>>>) -> Config {
        Config {
            dump: false,
            in_place: false,
            comments: false,
            find_filter: Vec::new(),
            count_filter: Vec::new(),
            language: None,
            function: false,
            metrics: false,
            ops: false,
            output_format: None,
            output: None,
            pretty: false,
            line_start: None,
            line_end: None,
            preproc_lock,
            preproc: None,
            count_lock: None,
            markdown_tx: None,
            strip_prefix: String::new(),
            warning: false,
        }
    }

    #[test]
    fn process_dir_path_noop_without_preproc() {
        let cfg = test_config(None);
        let mut all_files = HashMap::new();
        process_dir_path(&mut all_files, Path::new("/some/file.cpp"), &cfg);
        assert!(all_files.is_empty());
    }

    #[test]
    fn process_dir_path_inserts_valid_utf8_filename() {
        let cfg = test_config(Some(Arc::new(Mutex::new(PreprocResults::default()))));
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
        let cfg = test_config(Some(Arc::new(Mutex::new(PreprocResults::default()))));
        let mut all_files = HashMap::new();
        process_dir_path(&mut all_files, Path::new("/a/foo.cpp"), &cfg);
        process_dir_path(&mut all_files, Path::new("/b/foo.cpp"), &cfg);
        assert_eq!(all_files.len(), 1);
        assert_eq!(
            all_files["foo.cpp"],
            vec![PathBuf::from("/a/foo.cpp"), PathBuf::from("/b/foo.cpp")]
        );
    }

    #[test]
    fn reject_dump_and_metrics() {
        assert!(Opts::try_parse_from(["cli", "-d", "-m"]).is_err());
    }

    #[test]
    fn reject_metrics_and_function() {
        assert!(Opts::try_parse_from(["cli", "-m", "-F"]).is_err());
    }

    #[test]
    fn reject_dump_with_output_format() {
        assert!(Opts::try_parse_from(["cli", "-d", "-O", "json"]).is_err());
    }

    #[test]
    fn reject_output_format_without_action() {
        assert!(Opts::try_parse_from(["cli", "-O", "json"]).is_err());
    }

    #[test]
    fn accept_metrics_alone() {
        assert!(Opts::try_parse_from(["cli", "-m"]).is_ok());
    }

    #[test]
    fn accept_metrics_with_output_format() {
        assert!(Opts::try_parse_from(["cli", "-m", "-O", "json"]).is_ok());
    }

    #[test]
    fn accept_ops_with_output_format() {
        assert!(Opts::try_parse_from(["cli", "--ops", "-O", "json"]).is_ok());
    }

    #[test]
    fn accept_parse_with_no_action_flags() {
        // The "action" ArgGroup itself is `required(false)` because
        // `--preproc` lives outside it (it doubles as a modifier with a
        // single path, where another action takes precedence). The
        // "you must pick an action" check is enforced at runtime in
        // `main()` rather than at parse time.
        assert!(Opts::try_parse_from(["cli", "-p", "file.rs"]).is_ok());
    }

    #[test]
    fn reject_metrics_and_ops() {
        assert!(Opts::try_parse_from(["cli", "-m", "--ops"]).is_err());
    }

    #[test]
    fn accept_preproc_alone() {
        // --preproc with a single path is accepted standalone (acts as the
        // scan-mode action when 2+ paths are given, and is otherwise an
        // explicit no-op).
        assert!(Opts::try_parse_from(["cli", "--preproc", "data.json"]).is_ok());
    }

    #[test]
    fn accept_preproc_with_metrics() {
        // --preproc with a single path acts as a modifier that loads
        // preprocessor metadata; combining it with --metrics is the
        // intended C/C++ analysis workflow.
        assert!(Opts::try_parse_from(["cli", "--preproc", "data.json", "-m"]).is_ok());
    }

    #[test]
    fn accept_preproc_with_dump() {
        // --preproc is a modifier when paired with another action; it
        // does not occupy a slot in the action group.
        assert!(Opts::try_parse_from(["cli", "--preproc", "data.json", "-d"]).is_ok());
    }

    #[test]
    fn accept_preproc_with_multiple_paths() {
        // Scan-mode --preproc (2+ paths) is also a valid action.
        assert!(Opts::try_parse_from(["cli", "--preproc", "a.cpp", "--preproc", "b.cpp",]).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn process_dir_path_skips_non_utf8_filename() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let cfg = test_config(Some(Arc::new(Mutex::new(PreprocResults::default()))));
        let mut all_files = HashMap::new();
        let bad_name = OsStr::from_bytes(b"\xff\xfe");
        let path = PathBuf::from("/some/dir").join(bad_name);
        process_dir_path(&mut all_files, &path, &cfg);
        assert!(all_files.is_empty());
    }
}
