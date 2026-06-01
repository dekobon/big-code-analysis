// Sibling-file unit tests for `src/concurrent_files.rs`, wired in via
// `#[path = "concurrent_files_tests.rs"] mod tests;`. The
// `./**/*_tests.rs` rule in `.bcaignore` keeps this file out of the
// self-scan walker so production-file metric caps stay tight.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::Builder;

#[test]
fn consumer_terminates_on_poison_pill() {
    // The `consumer` loop terminates when the sender sends `None`
    // (the poison-pill used in `ConcurrentRunner::run`). Before the
    // refactor this relied on `if job.is_none() { break; }` followed
    // by `job.unwrap()`; the equivalent `while let Ok(Some(job))`
    // pattern must still terminate cleanly without panic.
    let (sender, receiver): (JobSender<()>, JobReceiver<()>) = unbounded();

    // Count how many times the supplied closure is invoked so the
    // test would notice if the consumer mistakenly tried to process
    // the poison-pill.
    let invocations = Arc::new(AtomicUsize::new(0));
    let invocations_for_closure = Arc::clone(&invocations);
    let func = Arc::new(move |_path: PathBuf, _cfg: &()| {
        invocations_for_closure.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });

    let handle = thread::spawn(move || consumer(receiver, func));

    // Send only the poison-pill — no real job.
    sender.send(None).expect("send should succeed");

    // The consumer must exit cleanly without `recv` errors or
    // panics on the now-`None` job item.
    handle.join().expect("consumer thread should not panic");
    assert_eq!(
        invocations.load(Ordering::SeqCst),
        0,
        "consumer must not invoke the closure for the poison-pill",
    );
}

#[test]
fn consumer_processes_jobs_then_terminates_on_poison_pill() {
    // Mixed sequence: real jobs first, then the `None` poison-pill.
    // Each `Some(job)` must be processed; the `None` must terminate
    // the loop without panicking.
    let (sender, receiver): (JobSender<()>, JobReceiver<()>) = unbounded();

    let invocations = Arc::new(AtomicUsize::new(0));
    let invocations_for_closure = Arc::clone(&invocations);
    let func = Arc::new(move |_path: PathBuf, _cfg: &()| {
        invocations_for_closure.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });

    let handle = thread::spawn(move || consumer(receiver, func));

    let cfg = Arc::new(());
    for name in ["a.rs", "b.rs", "c.rs"] {
        sender
            .send(Some(JobItem {
                path: PathBuf::from(name),
                cfg: Arc::clone(&cfg),
            }))
            .expect("send should succeed");
    }
    sender.send(None).expect("send should succeed");

    handle.join().expect("consumer thread should not panic");
    assert_eq!(
        invocations.load(Ordering::SeqCst),
        3,
        "all three real jobs must be processed before the poison-pill",
    );
}

// ── Terminal file-list dispatch (post-#495) ──────────────────────
//
// The runner no longer walks directories or filters globs: `paths` is
// the resolved, terminal file list and every regular-file entry is
// dispatched exactly once. The tests below pin that contract.

#[test]
fn run_dispatches_every_file_in_the_terminal_list() {
    let tmp = Builder::new()
        .prefix("visible-run")
        .tempdir()
        .expect("tempdir");
    let root = tmp.path();
    let a = root.join("a.rs");
    let b = root.join("b.py");
    std::fs::write(&a, b"// a").expect("write a");
    std::fs::write(&b, b"# b").expect("write b");

    let processed = Arc::new(AtomicUsize::new(0));
    let processed_for_closure = Arc::clone(&processed);
    let runner = ConcurrentRunner::new(4, move |_path: PathBuf, _cfg: &()| {
        processed_for_closure.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });

    let files_data = FilesData { paths: vec![a, b] };
    runner.run((), files_data).expect("run should succeed");

    // Both regular files are dispatched — no glob filtering happens in
    // the runner, so an entry's extension is irrelevant.
    assert_eq!(processed.load(Ordering::SeqCst), 2);
}

#[test]
fn run_skips_directories_and_missing_paths_without_walking() {
    let tmp = Builder::new()
        .prefix("visible-skip")
        .tempdir()
        .expect("tempdir");
    let root = tmp.path();
    let file = root.join("keep.rs");
    std::fs::write(&file, b"// keep").expect("write keep");
    // A directory entry and a nested file under it: the runner must
    // NOT descend into the directory (it is not a regular file), and
    // must skip the non-existent path with a warning.
    let subdir = root.join("sub");
    std::fs::create_dir(&subdir).expect("mkdir sub");
    std::fs::write(subdir.join("nested.rs"), b"// nested").expect("write nested");
    let missing = root.join("does-not-exist.rs");

    let processed = Arc::new(AtomicUsize::new(0));
    let processed_for_closure = Arc::clone(&processed);
    let runner = ConcurrentRunner::new(4, move |_path: PathBuf, _cfg: &()| {
        processed_for_closure.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });

    let files_data = FilesData {
        paths: vec![file, subdir, missing],
    };
    runner.run((), files_data).expect("run should succeed");

    // Only the single regular file is processed: the directory is not
    // recursed into (nested.rs is never dispatched) and the missing
    // path is skipped.
    assert_eq!(processed.load(Ordering::SeqCst), 1);
}
