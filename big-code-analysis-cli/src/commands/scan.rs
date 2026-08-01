//! Read-only inspection subcommands: `list-metrics`, `dump`, `functions`, `find`, `count`.

use super::*;

pub(crate) fn run_command_list_metrics(args: ListMetricsArgs) {
    let mut buf = Vec::new();
    write_metrics(&mut buf, args.mode).expect("writing to Vec<u8> is infallible");
    write_stdout_or_die(&buf);
}

pub(crate) fn run_command_dump(
    globals: GlobalOpts,
    line: LineRange,
    preproc: Option<Arc<PreprocResults>>,
) {
    // `dump` is the documented exception to #596's default-`.` walk: a
    // whole-tree AST dump of the cwd has no plausible use and scrolls
    // thousands of interleaved lines, so bare `bca dump` errors asking
    // for a path instead of defaulting to `.` (#690). A path may arrive
    // via `--paths`, `--paths-from`, or a manifest `paths` key (all
    // merged into `globals.paths` / `paths_from` before we reach here).
    if globals.paths.is_empty() && globals.paths_from.is_none() {
        die("dump needs an explicit path: pass one with --paths <PATH> \
             (a whole-tree AST dump of the current directory is never useful)");
    }
    let cfg = Config {
        line_start: line.line_start,
        line_end: line.line_end,
        ..Config::new(Action::Dump, &globals, preproc)
    };
    run_walk(globals, cfg);
}

pub(crate) fn run_command_functions(globals: GlobalOpts, preproc: Option<Arc<PreprocResults>>) {
    let cfg = Config::new(Action::Functions, &globals, preproc);
    run_walk(globals, cfg);
}
pub(crate) fn run_command_find(
    globals: GlobalOpts,
    args: FindArgs,
    preproc: Option<Arc<PreprocResults>>,
) {
    let line = args.line;
    let nodes = args.nodes.types;
    let cfg = Config {
        line_start: line.line_start,
        line_end: line.line_end,
        ..Config::new(Action::Find(nodes.into()), &globals, preproc)
    };
    run_walk(globals, cfg);
}

pub(crate) fn run_command_count(
    globals: GlobalOpts,
    args: CountArgs,
    preproc: Option<Arc<PreprocResults>>,
) {
    let collector = CountCollector::new();
    let cfg = Config {
        count_lock: Some(collector.clone()),
        ..Config::new(Action::Count(args.nodes.types.into()), &globals, preproc)
    };
    run_walk(globals, cfg);

    // All worker threads have joined, so the collector's `Arc` refcount
    // is back to one; `into_count` recovers the tally (degrading rather
    // than panicking if a worker poisoned the inner mutex, issue #445).
    let count = collector.into_count();
    // The tally is emitted on `main` after the walk, so the per-file
    // `write_failures` tally cannot see it and the `println!` this
    // replaces panicked on an unwritable stdout (#1132).
    // `writeln_stdout_or_die` gives it the same `error: …` line,
    // `EXIT_TOOL_ERROR`, and `BrokenPipe` tolerance as every other
    // post-walk emission.
    writeln_stdout_or_die(&count.to_string());
}
