//! The preprocessor subcommands: `strip-comments` and `preproc`.

use super::*;

pub(crate) fn run_command_strip_comments(
    globals: GlobalOpts,
    args: StripCommentsArgs,
    preproc: Option<Arc<PreprocResults>>,
) {
    // `--output` is a single-file sink; if more than one input file
    // matched, every worker would write to the same path (racing,
    // last-writer-wins, silent data loss). Resolve the file set first
    // and reject the multi-file case — `--in-place` is the multi-file
    // verb.
    let has_output = args.output.is_some();
    let action = Action::StripComments {
        in_place: args.in_place,
        output: args.output,
    };
    let mut cfg = Config::new(action, &globals, preproc);
    let (resolved, num_jobs) = resolve_walk_files(globals);
    if has_output && resolved.files.len() > 1 {
        die(format_args!(
            "--output writes a single file, but {} input files matched; \
             use --in-place to rewrite multiple files",
            resolved.files.len()
        ));
    }
    cfg.explicit_seeds = Arc::new(resolved.explicit_files);
    run_walk_resolved(resolved.files, num_jobs, cfg);
}

/// Recovers the accumulated [`PreprocResults`] from the shared worker
/// accumulator once every worker has joined.
///
/// Mirrors the panic-free recovery of [`CountCollector::into_count`]
/// (issue #445): a worker that panicked mid-update poisons the inner
/// mutex, and a worker that failed to join leaves the `Arc` shared.
/// Both failure modes degrade to the recovered data rather than
/// panicking (issue #740). The recovered guard still holds the
/// fully-applied append-collections, since each worker inserts a
/// distinct file's entry.
pub(crate) fn into_preproc_data(preproc_lock: Arc<Mutex<PreprocResults>>) -> PreprocResults {
    match Arc::try_unwrap(preproc_lock) {
        Ok(mutex) => mutex
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        Err(shared) => {
            let mut guard = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // The `Arc` is still shared (a worker failed to join), so we
            // cannot move the data out by value; take it from behind the
            // guard, leaving an empty default in its place.
            std::mem::take(&mut *guard)
        }
    }
}

pub(crate) fn run_command_preproc(globals: GlobalOpts, args: PreprocArgs) {
    let preproc_lock = Arc::new(Mutex::new(PreprocResults::default()));
    let output = args.output;
    let cfg = Config {
        preproc_lock: Some(preproc_lock.clone()),
        // PreprocProduce builds its own preproc results; any inbound
        // `--preproc-data` from globals is intentionally ignored for
        // this command (the original code passed `None` here too).
        ..Config::new(Action::PreprocProduce, &globals, None)
    };
    let paths = run_walk_collecting(globals, cfg);
    // Group the analyzed file list by basename for cross-file
    // `#include` resolution. Computing this from the same resolved
    // list the workers processed (rather than the library's old
    // directory-walk callback, which #489 left dead) is what restores
    // `bca preproc` include resolution — see #495.
    let all_files = group_files_by_basename(paths);

    let mut data = into_preproc_data(preproc_lock);
    // Include-resolution diagnostics (self-inclusion, cycles, non-UTF-8
    // paths, un-preprocessed files) are returned rather than written to
    // stderr by the library, so the CLI surfaces them here.
    for diagnostic in fix_includes(&mut data.files, &all_files) {
        eprintln!("{diagnostic}");
    }

    let serialized = serde_json::to_string(&data)
        .unwrap_or_else(|e| die(format_args!("failed to serialize preproc data: {e}")));
    if let Some(output_path) = output {
        write_file(&output_path, serialized.as_bytes())
            .unwrap_or_else(|e| die_io("write preproc output to", &output_path, e));
    } else {
        // Post-walk emission on `main`, so it needs the same fallible
        // write `count`'s tally does (#1132): the `println!` this
        // replaces exited 101 where the CLI documents 1.
        writeln_stdout_or_die(&serialized);
    }
}
