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
///
/// `include` / `exclude` are matched against each walked path **as the
/// walk emits it** — i.e. prefixed by the `paths` entry it was found
/// under (`./src/x.rs` for a `.` root, `/abs/root/src/x.rs` for an
/// absolute root). The match is therefore path-form sensitive: a
/// `./`-anchored glob will not match files discovered under an absolute
/// root. Callers that need path-form-independent matching must either
/// keep `paths` and the globs in the same form, or pre-filter the file
/// set and pass empty globsets here. The `big-code-analysis-cli` walk
/// does the latter: it resolves and anchors the file set itself (so a
/// `bca.toml` `paths = ["."]` resolved to an absolute root still honours
/// `./`-anchored excludes, see issues #488/#489) and hands this struct
/// empty globsets. Reconciling the two filtering layers into one
/// anchored seam is tracked by #495 (a 2.0-scoped API reshape).
#[derive(Debug)]
pub struct FilesData {
    /// Globs of files to include; matched against the emitted path form
    /// (see the type-level note on path-form sensitivity).
    pub include: GlobSet,
    /// Globs of files to exclude; matched against the emitted path form
    /// (see the type-level note on path-form sensitivity).
    pub exclude: GlobSet,
    /// Root paths to walk (files are yielded recursively under each).
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
#[path = "concurrent_files_tests.rs"]
mod tests;
