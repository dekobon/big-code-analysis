// bca: suppress-file(halstead, nargs, exit)
// Parallel walker orchestration; the offenders are many-fn / early-return
// aggregation artifacts (no cognitive/cyclomatic offender here — those stay
// enforced), not per-function logic complexity.

#![allow(clippy::needless_pass_by_value)]

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

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

fn consumer<Config, ProcFiles>(receiver: JobReceiver<Config>, func: Arc<ProcFiles>)
where
    ProcFiles: Fn(PathBuf, &Config) -> std::io::Result<()> + Send + Sync,
{
    // `Ok(None)` is the poison-pill terminating the consumer loop;
    // `Err(_)` means the channel was closed (sender dropped).
    while let Ok(Some(job)) = receiver.recv() {
        let path = job.path.clone();

        if let Err(err) = func(job.path, &job.cfg) {
            eprintln!("{err:?} for file {}", path.display());
        }
    }
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
    /// * `num_jobs` - Number of jobs utilized to process files concurrently.
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
