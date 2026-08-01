#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::float_cmp,
    clippy::needless_pass_by_value,
    clippy::needless_raw_string_hashes,
    clippy::similar_names,
    clippy::too_many_lines
)]

use std::num::NonZeroUsize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::{DirEntry, WalkDir};

use big_code_analysis::LANG;
use big_code_analysis::*;

#[allow(dead_code)]
pub mod fixtures;

#[allow(dead_code)]
pub mod validators;

/// Deterministic git-repo builder for change-history (VCS) tests.
/// Gated behind the backend feature it exercises.
#[cfg(feature = "vcs-git")]
#[allow(dead_code)]
pub mod vcs_fixture;

#[allow(dead_code)]
const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/", "repositories");
const SNAPSHOT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/",
    "repositories/big-code-analysis-output/snapshots"
);

#[derive(Debug)]
struct Config {
    language: Option<LANG>,
    source_root: PathBuf,
    /// Files that reached the snapshot assertion.
    ///
    /// Counting resolved paths is not the same as counting assertions:
    /// `act_on_file` returns `Ok(())` early for a file too short to read
    /// and for one whose language cannot be guessed, so a resolved file
    /// can still silently skip its snapshot. Shared across the runner's
    /// consumer threads, hence the atomic.
    snapshotted: Arc<AtomicUsize>,
}

fn act_on_file(path: PathBuf, cfg: &Config) -> std::io::Result<()> {
    // Open file
    let Some(source) = read_file_with_eol(&path)? else {
        return Ok(());
    };

    // Guess programming language
    let language = if let Some(language) = cfg.language {
        language
    } else if let Some(language) = guess_language(&source, &path).0 {
        language
    } else {
        return Ok(());
    };

    // Get FuncSpace struct.
    //
    // Snapshot fixtures key on the file path as the top-level
    // identifier, so use `Source::name` to thread the path string
    // through `analyze`. This matches the behaviour the deprecated
    // `get_function_spaces` shim had (lossy-stringified path) for
    // the valid-UTF-8 paths the integration corpora carry.
    let name = Some(path.to_string_lossy().into_owned());
    let funcspace_struct = analyze(
        Source::new(language, &source)
            .with_name(name)
            .with_preproc_path(Some(&path)),
        MetricsOptions::default(),
    )
    .expect("analyze returned Err for fixture; the parser may have rejected the source");

    cfg.snapshotted.fetch_add(1, Ordering::Relaxed);

    insta::with_settings!({snapshot_path => Path::new(SNAPSHOT_PATH)
                .join(path.strip_prefix(&cfg.source_root).unwrap())
                .parent()
                .unwrap(),
                prepend_module_to_snapshot => false,
                sort_maps => true,
    }, {
        insta::assert_yaml_snapshot!(
            path.file_name().unwrap().to_string_lossy().as_ref(),
            funcspace_struct,
            {
                // Round floating point values to three decimal places since the can differ from
                // system to system.
                ".spaces[].**.metrics.*.*" => insta::rounded_redaction(3),
                ".metrics.*.*" => insta::rounded_redaction(3),
                // Redact away the name since paths are different on different systems.
                ".name" => "[filepath]",
            }
        );

    });

    Ok(())
}

/// Produces metrics runtime and compares them with previously generated json files
///
/// `expected_files` is the number of files the corpus root must resolve to.
/// Asserting it keeps a shrinking corpus from reading as a passing test.
#[allow(dead_code)]
pub fn compare_rca_output_with_files(
    repo_name: &str,
    include: &[&str],
    exclude: &[&str],
    expected_files: usize,
) {
    compare_rca_output_with_files_under(
        Path::new(REPO),
        repo_name,
        include,
        exclude,
        expected_files,
    );
}

/// Same as [`compare_rca_output_with_files`] but with an explicit source root.
///
/// `source_root` is the directory whose layout mirrors the snapshot directory:
/// each input file's path under `source_root` becomes its snapshot path under
/// `SNAPSHOT_PATH`. Use this when the corpus lives nested under the
/// `big-code-analysis-output` submodule (as for the synthetic PHP corpus) so
/// snapshots land at `snapshots/<repo_name>/...` rather than picking up the
/// submodule directory as an extra path component.
///
/// `expected_files` is the number of files the corpus root must resolve to.
/// Asserting it keeps a shrinking corpus from reading as a passing test.
#[allow(dead_code)]
pub fn compare_rca_output_with_files_under(
    source_root: &Path,
    repo_name: &str,
    include: &[&str],
    exclude: &[&str],
    expected_files: usize,
) {
    // One consumer per core. `num_jobs` is the consumer count directly
    // since #1114 moved dispatch onto the calling thread; before that
    // `ConcurrentRunner` reserved a slot for a producer thread and
    // spawned `max(2, n) - 1` consumers, so this asked for one more than
    // the machine's parallelism to compensate. Passing the parallelism
    // figure straight through now means what the old `+ 1` was for.
    //
    // Several corpus tests can run concurrently under nextest, each with
    // its own runner, so this nominally oversubscribes. Measured, it does
    // not cost anything: only DeepSpeech runs longer than ~1.2s, so the
    // overlap window is short, and the small corpora leave most of their
    // consumers parked on an empty queue.
    let num_jobs = std::thread::available_parallelism().map_or(4, NonZeroUsize::get);

    let snapshotted = Arc::new(AtomicUsize::new(0));
    let cfg = Config {
        language: None,
        source_root: source_root.to_path_buf(),
        snapshotted: Arc::clone(&snapshotted),
    };

    let mut gsbi = GlobSetBuilder::new();
    for file in include {
        gsbi.add(Glob::new(file).unwrap());
    }

    let mut gsbe = GlobSetBuilder::new();
    for file in exclude {
        gsbe.add(Glob::new(file).unwrap());
    }

    // The library runner is a terminal file-set processor (#495): it no
    // longer walks directories or applies globsets. Resolve the corpus
    // root into a filtered file list here — skipping hidden entries and
    // applying the include/exclude globsets against the emitted path —
    // then hand the runner the resolved list.
    let include = gsbi.build().unwrap();
    let exclude = gsbe.build().unwrap();
    let corpus_root = source_root.join(repo_name);
    let paths = resolve_corpus_files(&corpus_root, &include, &exclude);

    // A corpus that resolves to *fewer* files than expected makes the
    // runner skip the missing files' snapshot assertions while still
    // returning `Ok(())`, so the test passes having verified less than it
    // claims. Zero files is the degenerate case (#938): an uninitialized
    // submodule leaves the directory empty and every assertion is skipped.
    //
    // The corpora are submodules pinned to a fixed SHA, so the resolved
    // count is deterministic for a given checkout. Asserting it exactly
    // turns any silent change in corpus coverage — an over-eager exclude
    // glob, a partially-initialized submodule, a change to the traversal
    // filters — into a loud failure instead of a quiet coverage loss. A
    // deliberate corpus bump updates this number alongside the snapshots.
    assert_eq!(
        paths.len(),
        expected_files,
        "unexpected corpus file count under {}. If it resolved 0 files the \
         integration corpus is empty or missing — initialize the submodules \
         with `git submodule update --init --recursive`. Otherwise the \
         corpus or the include/exclude globs changed; update the expected \
         count alongside the snapshots.",
        corpus_root.display(),
    );

    let files_data = FilesData { paths };

    if let Err(e) = ConcurrentRunner::new(num_jobs, act_on_file).run(cfg, files_data) {
        // Use panic! rather than process::exit so the failure surfaces
        // through cargo test's per-test reporting and lets the rest of
        // the binary's tests produce their own diagnostics.
        panic!("ConcurrentRunner failed: {e:?}");
    }

    // Resolving a file is not the same as asserting its snapshot: the two
    // early returns in `act_on_file` let a resolved file slip through
    // without one. Close that gap so "the corpus shrank" and "a file
    // stopped being analyzed" are both loud.
    assert_eq!(
        snapshotted.load(Ordering::Relaxed),
        expected_files,
        "resolved {expected_files} files under {} but only asserted a \
         snapshot for some of them; a file became unreadable or its \
         language stopped being guessable.",
        corpus_root.display(),
    );
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|s| s.starts_with('.'))
}

/// Walk `root` recursively, returning the regular files that pass the
/// include/exclude globsets (matched against the emitted path, as the
/// pre-#495 library walk did) and aren't under a hidden directory.
#[allow(dead_code)]
fn resolve_corpus_files(root: &Path, include: &GlobSet, exclude: &GlobSet) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
        .filter_map(Result::ok)
        .map(walkdir::DirEntry::into_path)
        .filter(|path| {
            path.is_file()
                && (include.is_empty() || include.is_match(path))
                && (exclude.is_empty() || !exclude.is_match(path))
        })
        .collect()
}
