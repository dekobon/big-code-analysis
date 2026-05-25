#![allow(clippy::needless_pass_by_value)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use crossbeam::channel::{Receiver, Sender, unbounded};
use globset::GlobSet;
use walkdir::{DirEntry, WalkDir};

type ProcFilesFunction<Config> = dyn Fn(PathBuf, &Config) -> std::io::Result<()> + Send + Sync;

type ProcDirPathsFunction<Config> =
    dyn Fn(&mut HashMap<String, Vec<PathBuf>>, &Path, &Config) + Send + Sync;

type ProcPathFunction<Config> = dyn Fn(&Path, &Config) + Send + Sync;

// Null functions removed at compile time
fn null_proc_dir_paths<Config>(_: &mut HashMap<String, Vec<PathBuf>>, _: &Path, _: &Config) {}
fn null_proc_path<Config>(_: &Path, _: &Config) {}

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

fn send_file<T>(
    path: PathBuf,
    cfg: &Arc<T>,
    sender: &JobSender<T>,
) -> Result<(), ConcurrentErrors> {
    sender
        .send(Some(JobItem {
            path,
            cfg: Arc::clone(cfg),
        }))
        .map_err(|e| ConcurrentErrors::Sender(e.to_string()))
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|s| s.starts_with('.'))
}

fn explore<Config, ProcDirPaths, ProcPath>(
    files_data: FilesData,
    cfg: &Arc<Config>,
    proc_dir_paths: ProcDirPaths,
    proc_path: ProcPath,
    sender: &JobSender<Config>,
) -> Result<HashMap<String, Vec<PathBuf>>, ConcurrentErrors>
where
    ProcDirPaths: Fn(&mut HashMap<String, Vec<PathBuf>>, &Path, &Config) + Send + Sync,
    ProcPath: Fn(&Path, &Config) + Send + Sync,
{
    let FilesData {
        mut paths,
        include,
        exclude,
    } = files_data;
    let filters = Filters {
        include: &include,
        exclude: &exclude,
    };

    let mut all_files: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for path in paths.drain(..) {
        if !path.exists() {
            eprintln!("Warning: File doesn't exist: {}", path.display());
            continue;
        }
        if path.is_dir() {
            for entry_path in walk_dir_files(&path, &filters) {
                let entry_path = entry_path?;
                proc_dir_paths(&mut all_files, &entry_path, cfg);
                send_file(entry_path, cfg, sender)?;
            }
        } else if filters.matches(&path) && path.is_file() {
            proc_path(&path, cfg);
            send_file(path, cfg, sender)?;
        }
    }

    Ok(all_files)
}

/// Borrowed include/exclude pair, factored out so `explore` and the
/// directory walker can share one filter predicate instead of
/// re-evaluating two near-identical `&&`-chains side-by-side.
struct Filters<'a> {
    include: &'a GlobSet,
    exclude: &'a GlobSet,
}

impl Filters<'_> {
    fn matches(&self, path: &Path) -> bool {
        (self.include.is_empty() || self.include.is_match(path))
            && (self.exclude.is_empty() || !self.exclude.is_match(path))
    }
}

/// Walk `root` recursively, yielding only regular files that pass
/// `filters` and aren't hidden. `WalkDir` errors are surfaced as
/// `ConcurrentErrors::Sender` so the caller can `?`-propagate them
/// through this iterator.
fn walk_dir_files<'a>(
    root: &Path,
    filters: &'a Filters<'_>,
) -> impl Iterator<Item = Result<PathBuf, ConcurrentErrors>> + 'a {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
        .filter_map(move |entry| match entry {
            Ok(entry) => {
                let path = entry.path();
                (filters.matches(path) && path.is_file()).then(|| Ok(path.to_path_buf()))
            }
            Err(e) => Some(Err(ConcurrentErrors::Sender(e.to_string()))),
        })
}

/// Series of errors that might happen when processing files concurrently.
#[derive(Debug)]
pub enum ConcurrentErrors {
    /// Producer side error.
    ///
    /// An error occurred inside the producer thread.
    Producer(String),
    /// Sender side error.
    ///
    /// An error occurred when sending an item.
    Sender(String),
    /// Receiver side error.
    ///
    /// An error occurred inside one of the receiver threads.
    Receiver(String),
    /// Thread side error.
    ///
    /// A general error occurred when a thread is being spawned or run.
    Thread(String),
}

/// Data related to files.
#[derive(Debug)]
pub struct FilesData {
    /// Kind of files included in a search.
    pub include: GlobSet,
    /// Kind of files excluded from a search.
    pub exclude: GlobSet,
    /// List of file paths.
    pub paths: Vec<PathBuf>,
}

/// A runner to process files concurrently.
pub struct ConcurrentRunner<Config> {
    proc_files: Box<ProcFilesFunction<Config>>,
    proc_dir_paths: Box<ProcDirPathsFunction<Config>>,
    proc_path: Box<ProcPathFunction<Config>>,
    num_jobs: usize,
}

impl<Config: 'static + Send + Sync> ConcurrentRunner<Config> {
    /// Creates a new `ConcurrentRunner`.
    ///
    /// * `num_jobs` - Number of jobs utilized to process files concurrently.
    /// * `proc_files` - Function that processes each file found during
    ///   the search.
    pub fn new<ProcFiles>(num_jobs: usize, proc_files: ProcFiles) -> Self
    where
        ProcFiles: 'static + Fn(PathBuf, &Config) -> std::io::Result<()> + Send + Sync,
    {
        let num_jobs = std::cmp::max(2, num_jobs) - 1;
        Self {
            proc_files: Box::new(proc_files),
            proc_dir_paths: Box::new(null_proc_dir_paths),
            proc_path: Box::new(null_proc_path),
            num_jobs,
        }
    }

    /// Sets the function to process the paths and subpaths contained in a
    /// directory.
    #[must_use]
    pub fn set_proc_dir_paths<ProcDirPaths>(mut self, proc_dir_paths: ProcDirPaths) -> Self
    where
        ProcDirPaths:
            'static + Fn(&mut HashMap<String, Vec<PathBuf>>, &Path, &Config) + Send + Sync,
    {
        self.proc_dir_paths = Box::new(proc_dir_paths);
        self
    }

    /// Sets the function to process a single path.
    #[must_use]
    pub fn set_proc_path<ProcPath>(mut self, proc_path: ProcPath) -> Self
    where
        ProcPath: 'static + Fn(&Path, &Config) + Send + Sync,
    {
        self.proc_path = Box::new(proc_path);
        self
    }

    /// Runs the producer-consumer approach to process the files
    /// contained in a directory and in its own subdirectories.
    ///
    /// * `config` - Information used to process a file.
    /// * `files_data` - Information about the files to be included or excluded
    ///   from a search more the number of paths considered in the search.
    ///
    /// # Errors
    ///
    /// Returns [`ConcurrentErrors::Thread`] when any worker thread
    /// (the single producer OR one of the `num_jobs` consumers)
    /// cannot be spawned via [`std::thread::Builder::spawn`];
    /// [`ConcurrentErrors::Producer`] when the producer thread
    /// panics during its directory walk and join fails;
    /// [`ConcurrentErrors::Sender`] when a worker cannot place an
    /// item (or the post-walk `None` poison-pill) on the channel;
    /// [`ConcurrentErrors::Receiver`] when a consumer thread panics
    /// and its join fails. Per-file processing errors raised by the
    /// user-supplied callbacks are surfaced through the callbacks
    /// themselves, not through this `Result`.
    pub fn run(
        self,
        config: Config,
        files_data: FilesData,
    ) -> Result<HashMap<String, Vec<PathBuf>>, ConcurrentErrors> {
        let cfg = Arc::new(config);

        let (sender, receiver) = unbounded();

        let producer = {
            let sender = sender.clone();

            match thread::Builder::new()
                .name(String::from("Producer"))
                .spawn(move || {
                    explore(
                        files_data,
                        &cfg,
                        self.proc_dir_paths,
                        self.proc_path,
                        &sender,
                    )
                }) {
                Ok(producer) => producer,
                Err(e) => return Err(ConcurrentErrors::Thread(e.to_string())),
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
                Err(e) => return Err(ConcurrentErrors::Thread(e.to_string())),
            };

            receivers.push(t);
        }

        let Ok(all_files) = producer.join() else {
            return Err(ConcurrentErrors::Producer(
                "Child thread panicked".to_owned(),
            ));
        };

        // Poison the receiver, now that the producer is finished.
        for _ in 0..self.num_jobs {
            if let Err(e) = sender.send(None) {
                return Err(ConcurrentErrors::Sender(e.to_string()));
            }
        }

        for receiver in receivers {
            if receiver.join().is_err() {
                return Err(ConcurrentErrors::Receiver(
                    "A thread used to process a file panicked".to_owned(),
                ));
            }
        }

        all_files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::Builder;
    use walkdir::WalkDir;

    // `tempfile::TempDir::new()` uses a default `.tmp` prefix, which
    // would itself trip `is_hidden` and filter the entire fixture out.
    // The tests below use `Builder::new().prefix("visible-")` to land
    // on a non-hidden root.
    fn make_visible_tempdir() -> tempfile::TempDir {
        Builder::new().prefix("visible-").tempdir().unwrap()
    }

    /// Returns the visited `DirEntry` filenames for a directory tree,
    /// applying the same `filter_entry(is_hidden)` gate used by
    /// `explore`.
    fn walk_skipping_hidden(dir: &Path) -> Vec<String> {
        WalkDir::new(dir)
            .into_iter()
            .filter_entry(|e| !is_hidden(e))
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .collect()
    }

    #[test]
    fn is_hidden_skips_dotfiles_and_keeps_regular_files() {
        let dir = make_visible_tempdir();
        std::fs::write(dir.path().join("keep.rs"), "// kept\n").unwrap();
        std::fs::write(dir.path().join(".env"), "secret=1\n").unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();

        let visited = walk_skipping_hidden(dir.path());
        assert!(visited.iter().any(|n| n == "keep.rs"));
        assert!(!visited.iter().any(|n| n == ".env"));
        assert!(!visited.iter().any(|n| n == ".gitignore"));
    }

    #[test]
    fn is_hidden_prunes_hidden_directories_recursively() {
        let dir = make_visible_tempdir();
        let hidden_dir = dir.path().join(".hidden");
        std::fs::create_dir(&hidden_dir).unwrap();
        std::fs::write(hidden_dir.join("inside.rs"), "// inside hidden\n").unwrap();
        std::fs::write(dir.path().join("visible.rs"), "// visible\n").unwrap();

        let visited = walk_skipping_hidden(dir.path());
        // The hidden directory and everything inside it must be pruned.
        assert!(visited.iter().any(|n| n == "visible.rs"));
        assert!(!visited.iter().any(|n| n == ".hidden"));
        assert!(!visited.iter().any(|n| n == "inside.rs"));
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
        let invocations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let invocations_for_closure = Arc::clone(&invocations);
        let func = Arc::new(move |_path: PathBuf, _cfg: &()| {
            invocations_for_closure.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });

        let handle = thread::spawn(move || consumer(receiver, func));

        // Send only the poison-pill — no real job.
        sender.send(None).expect("send should succeed");

        // The consumer must exit cleanly without `recv` errors or
        // panics on the now-`None` job item.
        handle.join().expect("consumer thread should not panic");
        assert_eq!(
            invocations.load(std::sync::atomic::Ordering::SeqCst),
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

        let invocations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let invocations_for_closure = Arc::clone(&invocations);
        let func = Arc::new(move |_path: PathBuf, _cfg: &()| {
            invocations_for_closure.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
            invocations.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "all three real jobs must be processed before the poison-pill",
        );
    }
}
