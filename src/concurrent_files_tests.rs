// Sibling-file unit tests for `src/concurrent_files.rs`, wired in via
// `#[path = "concurrent_files_tests.rs"] mod tests;`. The
// `./**/*_tests.rs` rule in `.bcaignore` keeps this file out of the
// self-scan walker so production-file metric caps stay tight.

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

// ── Filters::matches truth table ───────────────────────────────────

fn globset(patterns: &[&str]) -> GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for p in patterns {
        builder.add(globset::Glob::new(p).expect("valid glob"));
    }
    builder.build().expect("globset")
}

#[test]
fn filters_matches_empty_include_means_accept_all() {
    let include = GlobSet::empty();
    let exclude = GlobSet::empty();
    let f = Filters {
        include: &include,
        exclude: &exclude,
    };
    assert!(f.matches(Path::new("any.rs")));
    assert!(f.matches(Path::new("nested/dir/file.py")));
}

#[test]
fn filters_matches_include_pattern_filters_out_misses() {
    let include = globset(&["**/*.rs"]);
    let exclude = GlobSet::empty();
    let f = Filters {
        include: &include,
        exclude: &exclude,
    };
    assert!(f.matches(Path::new("src/lib.rs")));
    assert!(!f.matches(Path::new("docs/notes.md")));
}

#[test]
fn filters_matches_exclude_pattern_overrides_include() {
    let include = globset(&["**/*.rs"]);
    let exclude = globset(&["**/target/**"]);
    let f = Filters {
        include: &include,
        exclude: &exclude,
    };
    assert!(f.matches(Path::new("src/lib.rs")));
    // Excluded by `target/` despite matching `*.rs`.
    assert!(!f.matches(Path::new("target/debug/build.rs")));
}

#[test]
fn filters_matches_empty_exclude_means_only_include_filters() {
    let include = globset(&["**/*.py"]);
    let exclude = GlobSet::empty();
    let f = Filters {
        include: &include,
        exclude: &exclude,
    };
    assert!(f.matches(Path::new("script.py")));
    assert!(!f.matches(Path::new("script.rs")));
}

// ── walk_dir_files lazy-allocation path ───────────────────────────

#[test]
fn walk_dir_files_yields_only_included_regular_files() {
    let tmp = Builder::new()
        .prefix("visible-walk")
        .tempdir()
        .expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("keep.rs"), b"// kept").expect("write keep");
    std::fs::write(root.join("skip.txt"), b"// skipped").expect("write skip");
    std::fs::create_dir(root.join("sub")).expect("mkdir sub");
    std::fs::write(root.join("sub/inner.rs"), b"// nested").expect("write inner");

    let include = globset(&["**/*.rs"]);
    let exclude = GlobSet::empty();
    let filters = Filters {
        include: &include,
        exclude: &exclude,
    };

    let mut yielded: Vec<PathBuf> = walk_dir_files(root, &filters)
        .collect::<Result<Vec<_>, _>>()
        .expect("no walkdir errors on a fresh tempdir");
    yielded.sort();

    let mut expected = vec![root.join("keep.rs"), root.join("sub/inner.rs")];
    expected.sort();
    assert_eq!(yielded, expected);
}

#[test]
fn walk_dir_files_excludes_hidden_directories() {
    let tmp = Builder::new()
        .prefix("visible-hidden")
        .tempdir()
        .expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("keep.rs"), b"// kept").expect("write keep");
    std::fs::create_dir(root.join(".hidden")).expect("mkdir hidden");
    std::fs::write(root.join(".hidden/secret.rs"), b"// hidden").expect("write secret");

    let include = globset(&["**/*.rs"]);
    let exclude = GlobSet::empty();
    let filters = Filters {
        include: &include,
        exclude: &exclude,
    };

    let yielded: Vec<PathBuf> = walk_dir_files(root, &filters)
        .collect::<Result<Vec<_>, _>>()
        .expect("walkdir ok");
    // `.hidden/secret.rs` must NOT appear; the hidden-dir filter is what
    // makes `walk_dir_files` safe for use on `--paths .` with a workspace
    // that has dotfile directories like `.git`.
    assert_eq!(yielded, vec![root.join("keep.rs")]);
}

#[test]
fn walk_dir_files_skips_directories_with_excluded_glob() {
    let tmp = Builder::new()
        .prefix("visible-exclude")
        .tempdir()
        .expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("keep.rs"), b"// kept").expect("write keep");
    std::fs::create_dir(root.join("target")).expect("mkdir target");
    std::fs::write(root.join("target/gen.rs"), b"// generated").expect("write gen");

    let include = globset(&["**/*.rs"]);
    let exclude = globset(&["**/target/**"]);
    let filters = Filters {
        include: &include,
        exclude: &exclude,
    };

    let yielded: Vec<PathBuf> = walk_dir_files(root, &filters)
        .collect::<Result<Vec<_>, _>>()
        .expect("walkdir ok");
    assert_eq!(yielded, vec![root.join("keep.rs")]);
}
