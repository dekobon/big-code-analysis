// bca: suppress-file(halstead, nargs, nexits)
// Parallel walker orchestration; the offenders are many-fn / early-return
// aggregation artifacts (no cognitive/cyclomatic offender here — those stay
// enforced), not per-function logic complexity.

#![allow(clippy::needless_pass_by_value)]

use std::fmt;
use std::io::ErrorKind;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::thread::available_parallelism;

/// A boxed, thread-safe error cause carried by [`ConcurrentErrors`].
///
/// The runner moves results across thread boundaries (the producer /
/// consumer join seam in [`ConcurrentRunner::run`]), so any carried
/// source must be `Send`; it is also `'static` so it can outlive the
/// worker threads that produced it and so [`ConcurrentErrors`] stays
/// non-generic over the user's `Config`.
type BoxedCause = Box<dyn std::error::Error + Send + Sync + 'static>;

use crossbeam::channel::{Receiver, Sender, unbounded};

type ProcFilesFunction<Config> = dyn Fn(PathBuf, &Config) -> std::io::Result<()> + Send + Sync;

#[derive(Debug)]
struct JobItem<Config> {
    path: PathBuf,
    cfg: Arc<Config>,
}

type JobReceiver<Config> = Receiver<Option<JobItem<Config>>>;
type JobSender<Config> = Sender<Option<JobItem<Config>>>;

/// Parsed worker-count selector for [`ConcurrentRunner`], shared by the
/// `bca` CLI and the `bca-web` server so both binaries resolve the same
/// `<N|auto>` contract.
///
/// `Auto` is the default and resolves to the OS-reported effective CPU
/// count via [`std::thread::available_parallelism`], which honors Linux
/// cgroup CPU quotas, cgroup v2 `cpu.max`, and `sched_setaffinity`
/// cpusets. On macOS / Windows it falls back to the OS CPU count.
/// `Explicit(n)` is an integer override; [`NumJobs::from_str`] rejects
/// `0` with a clear error message rather than silently degrading to
/// serial mode.
///
/// This type is intentionally clap-agnostic — it is a plain [`FromStr`]
/// the binaries wire into clap via `value_parser` / `value_name`, so the
/// core library gains no clap dependency.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NumJobs {
    /// Use the OS-reported effective CPU count (cgroup / cpuset aware).
    #[default]
    Auto,
    /// Use an explicit, non-zero worker count.
    Explicit(NonZeroUsize),
}

impl NumJobs {
    /// Resolve to the worker count handed to [`ConcurrentRunner`].
    ///
    /// `Auto` falls back to `1` if [`available_parallelism`] errors —
    /// keeping the caller alive even in unusual sandboxes where the
    /// syscall fails. The returned value is always `>= 1`.
    #[must_use]
    pub fn resolve(self) -> usize {
        match self {
            Self::Auto => available_parallelism().map_or(1, NonZeroUsize::get),
            Self::Explicit(n) => n.get(),
        }
    }
}

/// Error returned by [`NumJobs::from_str`] when the input is neither
/// `auto` nor a usable positive integer.
///
/// Named (rather than a bare `String`) so callers can match the failure
/// mode and recover the rejected input via [`ParseNumJobsError::input`],
/// matching the typed-error convention of
/// [`ParseMetricError`](crate::ParseMetricError) /
/// [`ParseLangError`](crate::ParseLangError).
///
/// Deliberately exhaustive: `usize::from_str` collapses every malformed
/// integer (negative, overflowing, non-numeric) into the single
/// [`NotAPositiveInteger`](Self::NotAPositiveInteger) arm, so the only
/// other distinct mode is the in-range-but-zero rejection. A new mode
/// would be a deliberate change, and callers benefit from matching both
/// arms without a wildcard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseNumJobsError {
    /// The input parsed as `0`, below the `>= 1` worker-count floor.
    Zero {
        /// The rejected input, verbatim (e.g. `"0"`).
        input: String,
    },
    /// The input was not `auto` and did not parse as a positive integer
    /// (non-numeric, negative, or overflowing).
    NotAPositiveInteger {
        /// The rejected input, verbatim.
        input: String,
    },
}

impl ParseNumJobsError {
    /// The rejected input that failed to parse as a [`NumJobs`] value.
    ///
    /// Lets callers recover the offending string programmatically rather
    /// than scraping it out of the [`Display`](fmt::Display) output.
    #[must_use]
    pub fn input(&self) -> &str {
        match self {
            Self::Zero { input } | Self::NotAPositiveInteger { input } => input,
        }
    }
}

impl fmt::Display for ParseNumJobsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero { .. } => f.write_str(
                "--num-jobs must be >= 1 (use `--num-jobs 1` to force serial mode, \
                 or `--num-jobs auto` for the OS-reported CPU count)",
            ),
            Self::NotAPositiveInteger { input } => write!(
                f,
                "--num-jobs: expected a positive integer or `auto`, got `{input}`"
            ),
        }
    }
}

impl std::error::Error for ParseNumJobsError {}

impl FromStr for NumJobs {
    type Err = ParseNumJobsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        match s.parse::<usize>() {
            Ok(n) => {
                NonZeroUsize::new(n)
                    .map(Self::Explicit)
                    .ok_or_else(|| ParseNumJobsError::Zero {
                        input: s.to_owned(),
                    })
            }
            Err(_) => Err(ParseNumJobsError::NotAPositiveInteger {
                input: s.to_owned(),
            }),
        }
    }
}

fn consumer<Config, ProcFiles>(receiver: JobReceiver<Config>, func: Arc<ProcFiles>)
where
    ProcFiles: Fn(PathBuf, &Config) -> std::io::Result<()> + Send + Sync,
{
    // `Ok(None)` is the poison-pill terminating the consumer loop;
    // `Err(_)` means the channel was closed (sender dropped).
    while let Ok(Some(job)) = receiver.recv() {
        let path = job.path.clone();

        if let Err(err) = func(job.path, &job.cfg)
            && let Some(message) = per_file_error_message(&path, &err)
        {
            eprintln!("{message}");
        }
    }
}

/// Format a per-file processing error for stderr, or return `None` when it
/// should be swallowed silently.
///
/// `BrokenPipe` is swallowed to match the CLI's `write_stdout_or_die` policy
/// (`big-code-analysis-cli/src/lib.rs`): a closed downstream pipe (`| head`,
/// `| less`, …) is the routine case, not a failure. Every other error is
/// `Display`-formatted so internal type structure does not leak into
/// user-facing diagnostics (a Debug `Os { code, kind, .. }` struct).
fn per_file_error_message(path: &Path, err: &std::io::Error) -> Option<String> {
    if err.kind() == ErrorKind::BrokenPipe {
        return None;
    }
    Some(format!("error processing {}: {err}", path.display()))
}

fn send_file<T: 'static + Send + Sync>(
    path: PathBuf,
    cfg: &Arc<T>,
    sender: &JobSender<T>,
) -> Result<(), ConcurrentErrors> {
    sender
        .send(Some(JobItem {
            path,
            cfg: Arc::clone(cfg),
        }))
        .map_err(|e| ConcurrentErrors::Sender(Box::new(e)))
}

/// Producer body: dispatch each resolved file in `files_data.paths`
/// to the consumer pool.
///
/// `paths` is a **terminal file list** — already resolved, anchored,
/// and filtered by the caller (the `big-code-analysis-cli` walk seam
/// resolves it via its gitignore-aware `expand_seed_paths`). This
/// function therefore performs no directory traversal and no glob
/// filtering of its own: it skips entries that are missing or are not
/// regular files (warning to stderr) and sends the rest. For the
/// `big-code-analysis-cli` caller this skip is a safety net only —
/// `expand_seed_paths` already yields existing regular files — but it
/// keeps this public entry point robust for a direct library caller
/// that hands in an arbitrary path. Re-walking or re-filtering here
/// would re-introduce the emitted-path-form dependence that #488/#489
/// removed (see #495).
fn explore<Config: 'static + Send + Sync>(
    files_data: FilesData,
    cfg: &Arc<Config>,
    sender: &JobSender<Config>,
) -> Result<(), ConcurrentErrors> {
    for path in files_data.paths {
        if !path.is_file() {
            eprintln!("Warning: not a regular file, skipping: {}", path.display());
            continue;
        }
        send_file(path, cfg, sender)?;
    }

    Ok(())
}

/// Series of errors that might happen when processing files concurrently.
///
/// Marked `#[non_exhaustive]` so future failure modes can be added
/// without a SemVer break. Variants whose construction site has a
/// concrete underlying [`std::error::Error`] (`Sender`, `Thread`)
/// carry it as a boxed source and surface it through
/// [`std::error::Error::source`]; variants whose only available
/// information is a thread-panic payload (`Producer`, `Receiver`)
/// carry a message and return `None` from `source`, because a join
/// failure yields a `Box<dyn Any + Send>`, not an `Error`.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConcurrentErrors {
    /// Producer side error.
    ///
    /// The producer thread panicked and joining it failed. The panic
    /// payload is not a [`std::error::Error`], so this variant carries
    /// only a message and has no [`source`](std::error::Error::source).
    Producer(String),
    /// Sender side error.
    ///
    /// An item (or the poison-pill) could not be placed on the channel
    /// because every receiver was dropped. Carries the originating
    /// channel send error as its [`source`](std::error::Error::source).
    Sender(BoxedCause),
    /// Receiver side error.
    ///
    /// A consumer thread panicked and joining it failed. The panic
    /// payload is not a [`std::error::Error`], so this variant carries
    /// only a message and has no [`source`](std::error::Error::source).
    Receiver(String),
    /// Thread side error.
    ///
    /// A worker thread (producer or consumer) could not be spawned.
    /// Carries the originating [`std::io::Error`] as its
    /// [`source`](std::error::Error::source).
    Thread(BoxedCause),
}

impl fmt::Display for ConcurrentErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Producer(msg) => write!(f, "producer thread failed: {msg}"),
            Self::Sender(cause) => write!(f, "failed to send a file to a worker: {cause}"),
            Self::Receiver(msg) => write!(f, "consumer thread failed: {msg}"),
            Self::Thread(cause) => write!(f, "failed to spawn a worker thread: {cause}"),
        }
    }
}

impl std::error::Error for ConcurrentErrors {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            // Producer / Receiver originate from a thread-join panic
            // payload (`Box<dyn Any + Send>`), which is not an Error.
            Self::Producer(_) | Self::Receiver(_) => None,
            Self::Sender(cause) | Self::Thread(cause) => Some(cause.as_ref()),
        }
    }
}

/// A resolved, terminal file list for [`ConcurrentRunner`].
///
/// Each entry in `paths` is processed as a single regular file: the
/// runner does **not** walk directories and does **not** apply any
/// include/exclude filtering. Callers are responsible for resolving,
/// anchoring, and filtering the file set before constructing this
/// struct (the `big-code-analysis-cli` walk does so via its
/// gitignore-aware, walk-root-anchored `expand_seed_paths`). This is
/// the single filtering seam: there is no second, emitted-path-form
/// matcher in the library that could re-inherit the path-form
/// dependence #488/#489 removed (see #495).
#[derive(Debug)]
pub struct FilesData {
    /// The resolved files to process. Each path is treated as a
    /// terminal regular file; directories and non-existent paths are
    /// skipped with a warning.
    pub paths: Vec<PathBuf>,
}

/// A runner to process files concurrently.
pub struct ConcurrentRunner<Config> {
    proc_files: Box<ProcFilesFunction<Config>>,
    num_jobs: usize,
}

impl<Config: 'static + Send + Sync> ConcurrentRunner<Config> {
    /// Creates a new `ConcurrentRunner`.
    ///
    /// * `num_jobs` - Total worker budget (one producer thread plus the
    ///   consumer threads). [`run`](Self::run) always spawns a single
    ///   producer thread, so the number of consumer threads it spawns is
    ///   `max(2, num_jobs) - 1`. This means `0`, `1`, and `2` all collapse
    ///   to a single consumer (no panic, no linear scaling below 2);
    ///   values of `3` or more yield `num_jobs - 1` consumers.
    /// * `proc_files` - Function that processes each file in the list.
    pub fn new<ProcFiles>(num_jobs: usize, proc_files: ProcFiles) -> Self
    where
        ProcFiles: 'static + Fn(PathBuf, &Config) -> std::io::Result<()> + Send + Sync,
    {
        let num_jobs = std::cmp::max(2, num_jobs) - 1;
        Self {
            proc_files: Box::new(proc_files),
            num_jobs,
        }
    }

    /// Runs the producer-consumer pool over the terminal file list in
    /// `files_data`. Each path is dispatched to a worker as a single
    /// regular file; this runner performs no directory traversal or
    /// glob filtering (the caller resolves and filters the file set —
    /// see [`FilesData`]).
    ///
    /// * `config` - Information used to process a file.
    /// * `files_data` - The resolved, terminal file list to process.
    ///
    /// # Errors
    ///
    /// Returns [`ConcurrentErrors::Thread`] when any worker thread
    /// (the single producer OR one of the `num_jobs` consumers)
    /// cannot be spawned via [`std::thread::Builder::spawn`];
    /// [`ConcurrentErrors::Producer`] when the producer thread
    /// panics and join fails;
    /// [`ConcurrentErrors::Sender`] when a worker cannot place an
    /// item (or the post-dispatch `None` poison-pill) on the channel;
    /// [`ConcurrentErrors::Receiver`] when a consumer thread panics
    /// and its join fails. Per-file processing errors raised by the
    /// user-supplied callback are surfaced through the callback
    /// itself, not through this `Result`.
    pub fn run(self, config: Config, files_data: FilesData) -> Result<(), ConcurrentErrors> {
        let cfg = Arc::new(config);

        let (sender, receiver) = unbounded();

        let producer = {
            let sender = sender.clone();

            match thread::Builder::new()
                .name(String::from("Producer"))
                .spawn(move || explore(files_data, &cfg, &sender))
            {
                Ok(producer) => producer,
                Err(e) => return Err(ConcurrentErrors::Thread(Box::new(e))),
            }
        };

        let mut receivers = Vec::with_capacity(self.num_jobs);
        let proc_files = Arc::new(self.proc_files);
        for i in 0..self.num_jobs {
            let receiver = receiver.clone();
            let proc_files = proc_files.clone();

            let t = match thread::Builder::new()
                .name(format!("Consumer {i}"))
                .spawn(move || {
                    consumer(receiver, proc_files);
                }) {
                Ok(receiver) => receiver,
                Err(e) => return Err(ConcurrentErrors::Thread(Box::new(e))),
            };

            receivers.push(t);
        }

        let Ok(walk_result) = producer.join() else {
            return Err(ConcurrentErrors::Producer(
                "Child thread panicked".to_owned(),
            ));
        };
        walk_result?;

        // Poison the receiver, now that the producer is finished.
        for _ in 0..self.num_jobs {
            if let Err(e) = sender.send(None) {
                return Err(ConcurrentErrors::Sender(Box::new(e)));
            }
        }

        for receiver in receivers {
            if receiver.join().is_err() {
                return Err(ConcurrentErrors::Receiver(
                    "A thread used to process a file panicked".to_owned(),
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "concurrent_files_tests.rs"]
mod tests;
