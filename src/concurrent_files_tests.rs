// Sibling-file unit tests for `src/concurrent_files.rs`, wired in via
// `#[path = "concurrent_files_tests.rs"] mod tests;`. The
// `./**/*_tests.rs` rule in `.bcaignore` keeps this file out of the
// self-scan walker so production-file metric caps stay tight.

use super::*;
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::Builder;

// ── ConcurrentErrors: Display + std::error::Error (#553) ─────────
//
// `ConcurrentErrors` is a public, returned error type. It must
// implement `Display` and `std::error::Error`, exposing a `source()`
// chain for the variants that carry a concrete underlying error
// (`Sender`, `Thread`) and `None` for the panic-payload variants
// (`Producer`, `Receiver`).

#[test]
fn concurrent_errors_message_variants_display_without_source() {
    // Producer / Receiver originate from a thread-join panic payload
    // (`Box<dyn Any + Send>`), which is not an Error: Display must be
    // non-empty and source() must be None.
    let producer = ConcurrentErrors::Producer("Child thread panicked".to_owned());
    let receiver = ConcurrentErrors::Receiver("worker panicked".to_owned());

    assert_eq!(
        producer.to_string(),
        "producer thread failed: Child thread panicked",
    );
    assert_eq!(
        receiver.to_string(),
        "consumer thread failed: worker panicked"
    );
    assert!(producer.source().is_none());
    assert!(receiver.source().is_none());
}

#[test]
fn concurrent_errors_thread_variant_carries_io_error_source() {
    // The Thread variant carries the io::Error from a failed spawn.
    // source() must return it and downcast back to io::Error.
    let io_err = std::io::Error::other("spawn failed");
    let err = ConcurrentErrors::Thread(Box::new(io_err));

    assert!(
        err.to_string()
            .starts_with("failed to spawn a worker thread:"),
        "unexpected Display: {err}",
    );

    let source = err.source().expect("Thread must expose a source");
    assert!(
        source.downcast_ref::<std::io::Error>().is_some(),
        "Thread source must downcast to io::Error",
    );
    assert_eq!(source.to_string(), "spawn failed");
}

#[test]
fn concurrent_errors_sender_variant_carries_send_error_source() {
    // The Sender variant carries the crossbeam SendError produced when
    // every receiver is dropped. source() must return it.
    let (sender, receiver): (JobSender<()>, JobReceiver<()>) = unbounded();
    drop(receiver);
    let send_err = sender
        .send(None)
        .expect_err("send must fail once the receiver is dropped");
    let err = ConcurrentErrors::Sender(Box::new(send_err));

    assert!(
        err.to_string()
            .starts_with("failed to send a file to a worker:"),
        "unexpected Display: {err}",
    );
    assert!(err.source().is_some(), "Sender must expose a source");
}

#[test]
fn concurrent_errors_is_usable_as_boxed_std_error() {
    // Exercises the headline contract from #553: a ConcurrentErrors can
    // be coerced into Box<dyn std::error::Error> (and thus `?` into
    // anyhow / Box<dyn Error>).
    fn returns_boxed() -> Result<(), Box<dyn Error>> {
        Err(ConcurrentErrors::Producer("boom".to_owned()))?;
        Ok(())
    }
    let boxed = returns_boxed().expect_err("must propagate as boxed error");
    assert_eq!(boxed.to_string(), "producer thread failed: boom");
}

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

#[test]
fn consumer_continues_past_processing_errors_then_terminates() {
    // A `func` that returns `Err` must not abort the consumer loop: the
    // error is reported via `per_file_error_message` (BrokenPipe swallowed,
    // every other error `eprintln!`-ed) and the loop proceeds to the next
    // job, terminating only on the poison-pill. Driving both error kinds
    // exercises both sides of the call-site `&& let Some(message)` guard.
    let (sender, receiver): (JobSender<()>, JobReceiver<()>) = unbounded();

    let invocations = Arc::new(AtomicUsize::new(0));
    let invocations_for_closure = Arc::clone(&invocations);
    let func = Arc::new(move |path: PathBuf, _cfg: &()| {
        invocations_for_closure.fetch_add(1, Ordering::SeqCst);
        // "pipe.rs" simulates a closed downstream pipe (swallowed); any
        // other path simulates a real failure (reported to stderr).
        let kind = if path == Path::new("pipe.rs") {
            ErrorKind::BrokenPipe
        } else {
            ErrorKind::PermissionDenied
        };
        Err(std::io::Error::new(kind, "simulated"))
    });

    let handle = thread::spawn(move || consumer(receiver, func));

    let cfg = Arc::new(());
    for name in ["pipe.rs", "denied.rs"] {
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
        2,
        "both jobs must be processed despite each returning an error",
    );
}

// ── Per-file error diagnostics (#665) ────────────────────────────
//
// The consumer must swallow `BrokenPipe` silently (the routine
// `| head`/`| less` case, matching the CLI's `write_stdout_or_die`)
// and `Display`-format every other error so internal Debug struct
// shape (`Os { code, kind, .. }`) never leaks into diagnostics.

#[test]
fn per_file_error_swallows_broken_pipe() {
    let err = std::io::Error::new(ErrorKind::BrokenPipe, "broken pipe");
    assert_eq!(
        per_file_error_message(Path::new("ok.rs"), &err),
        None,
        "BrokenPipe must be swallowed silently",
    );
}

#[test]
fn per_file_error_display_formats_other_errors() {
    let err = std::io::Error::new(ErrorKind::PermissionDenied, "permission denied");
    let message = per_file_error_message(Path::new("locked.rs"), &err)
        .expect("a non-BrokenPipe error must produce a diagnostic");
    assert_eq!(message, "error processing locked.rs: permission denied");
    // Guard against the regression: no Debug struct shape may leak.
    assert!(
        !message.contains("kind:") && !message.contains("Os {") && !message.contains('{'),
        "diagnostic must be Display-formatted, not a Debug struct: {message}",
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

// ── NumJobs: shared <N|auto> parser + resolve (#560) ─────────────
//
// `NumJobs` is the worker-count selector shared by the `bca` CLI and the
// `bca-web` server. The parser must accept `auto` case-insensitively, a
// positive integer, and reject `0` and non-numeric tokens (naming the
// bad token in the error). `resolve()` is always `>= 1`.

#[test]
fn num_jobs_parses_auto_case_insensitively() {
    assert_eq!(NumJobs::from_str("auto").expect("auto"), NumJobs::Auto);
    assert_eq!(NumJobs::from_str("AUTO").expect("AUTO"), NumJobs::Auto);
    assert_eq!(NumJobs::from_str("Auto").expect("Auto"), NumJobs::Auto);
}

#[test]
fn num_jobs_parses_positive_integer() {
    let parsed = NumJobs::from_str("4").expect("4 must parse");
    assert_eq!(
        parsed,
        NumJobs::Explicit(NonZeroUsize::new(4).expect("4 is non-zero"))
    );
}

#[test]
fn num_jobs_rejects_invalid_string_naming_the_token() {
    let err = NumJobs::from_str("not-a-number").expect_err("non-numeric must be rejected");
    // The typed error carries the rejected input verbatim, so callers
    // recover it via `input()` rather than scraping `Display`.
    assert!(
        matches!(&err, ParseNumJobsError::NotAPositiveInteger { .. }),
        "non-numeric input must be the `NotAPositiveInteger` variant, got: {err:?}"
    );
    assert_eq!(err.input(), "not-a-number");
    assert!(
        err.to_string().contains("not-a-number"),
        "error message must name the bad token, got: {err}"
    );
}

#[test]
fn num_jobs_rejects_zero() {
    let err = NumJobs::from_str("0").expect_err("zero must be rejected");
    assert!(
        matches!(&err, ParseNumJobsError::Zero { .. }),
        "in-range zero must be the `Zero` variant, got: {err:?}"
    );
    assert_eq!(err.input(), "0");
    assert!(
        err.to_string().contains(">= 1"),
        "zero error must mention the >= 1 floor, got: {err}"
    );
}

#[test]
fn num_jobs_resolve_is_at_least_one() {
    assert!(NumJobs::Auto.resolve() >= 1);
    assert_eq!(
        NumJobs::Explicit(NonZeroUsize::new(3).expect("3 is non-zero")).resolve(),
        3
    );
}

#[test]
fn num_jobs_default_is_auto() {
    assert_eq!(NumJobs::default(), NumJobs::Auto);
}
