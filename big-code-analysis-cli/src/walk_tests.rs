// Sibling-file unit tests for the walk seam. Wired via
// `#[path = "walk_tests.rs"] mod tests;`. Matched by the
// `./**/*_tests.rs` rule in `.bcaignore`.

use super::*;

/// A directory-seed walk must return its files in sorted order (#1114).
///
/// `ignore`'s parallel walker hands entries to its visitors in whatever
/// order the threads finish, so the resolved list is sorted before it
/// reaches dispatch. Without that, two runs over an unchanged tree would
/// emit their per-file documents in different orders at `--jobs 1` —
/// output that used to be reproducible.
///
/// Asserted here rather than through the binary on purpose: end to end,
/// the ordering bug is a race, so a test that shells out passes or fails
/// by luck. It did — a five-run comparison through `bca metrics` stayed
/// green with the sort deleted. Checking the returned list is the only
/// version of this that fails every time.
///
/// The fixture names files so that creation order, readdir order, and
/// sorted order all disagree: `z*` files are created before `a*` ones,
/// and `-` (0x2d) sorts before `_` (0x5f) while both follow the digits.
#[test]
fn walk_directory_seed_returns_sorted_paths() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let root = tmp.path();
    let mut created = Vec::new();
    for d in 0..6 {
        let dir = root.join(format!("pkg{d}")).join("nested");
        std::fs::create_dir_all(&dir).expect("create dir");
        for f in 0..8 {
            for stem in [format!("z{f}_mod"), format!("a{f}-impl")] {
                let path = dir.join(format!("{stem}.rs"));
                std::fs::write(&path, b"pub fn f() {}\n").expect("write fixture");
                created.push(path);
            }
        }
    }

    let empty_include = mk_globset(Vec::new()).expect("empty globset");
    let empty_exclude = build_exclude_globset(Vec::new(), None, "--exclude-from");
    let cwd = std::env::current_dir().unwrap_or_default();
    let filters = WalkFilters {
        include: &empty_include,
        excludes: crate::walk_seed::AnchoredExcludes::new(
            &empty_exclude,
            &empty_exclude,
            None,
            &cwd,
            None,
        ),
        language_forced: false,
        measure_ignored: false,
    };
    let mut errors = WalkErrors::default();
    let found = walk_directory_seed(root, true, 8, &filters, &mut errors).files;

    assert_eq!(
        errors.count(),
        0,
        "a fully readable fixture tree must record no walk errors"
    );
    assert_eq!(
        found.len(),
        created.len(),
        "the walk must find every fixture file"
    );
    let mut want = found.clone();
    want.sort_unstable();
    assert_eq!(found, want, "walk_directory_seed must return sorted paths");
    // The fixture has to be one the sort actually reorders, or the
    // assertion above holds for any implementation.
    assert_ne!(
        created, want,
        "fixture creation order already matches sorted order; \
         the sortedness assertion above would pass unsorted"
    );
}

/// The ignore measurement's merge keeps the earliest seed's spelling of
/// a path both seeds reached, whatever order the parts arrive in (#1307).
///
/// The measurement's workers hand their parts back in completion order,
/// so the input here is deliberately the later seed first — the order a
/// stable sort keyed on the path alone would turn into the output,
/// letting thread timing choose which spelling `--report-skipped`
/// prints. `p/./src` and `p/src` compare equal as paths (interior `.`
/// is not a component), so only the rank can decide between them; the
/// assertion compares raw bytes for the same reason.
#[test]
fn merge_measured_keeps_the_first_seeded_spelling() {
    let later_seed = PathBuf::from("p/./src/v.rs");
    let earlier_seed = PathBuf::from("p/src/v.rs");
    assert_eq!(
        later_seed, earlier_seed,
        "fixture: the two spellings must be one path to Path's comparison"
    );
    let merged = merge_measured(vec![
        (1, later_seed),
        (1, PathBuf::from("p/./src/b.rs")),
        (0, earlier_seed),
        (0, PathBuf::from("./p/a.rs")),
    ]);

    let spelled: Vec<&std::ffi::OsStr> = merged.iter().map(|path| path.as_os_str()).collect();
    assert_eq!(
        spelled,
        ["./p/a.rs", "p/./src/b.rs", "p/src/v.rs"],
        "sorted in the `./`-stripped space, one mention per path, \
         and the rank-0 spelling of the shared path"
    );
}

/// The merge sorts and dedupes in the `./`-stripped space: `./q.rs`
/// (rank 0) and `q.rs` (rank 1) are one path and sort after `p/z.rs`.
/// Keyed on the raw spelling instead, `./q.rs` sorts first and the two
/// spellings are no longer adjacent, so both survive.
#[test]
fn merge_measured_sorts_and_dedupes_in_the_stripped_space() {
    let merged = merge_measured(vec![
        (1, PathBuf::from("q.rs")),
        (0, PathBuf::from("p/z.rs")),
        (0, PathBuf::from("./q.rs")),
    ]);
    let spelled: Vec<&std::ffi::OsStr> = merged.iter().map(|path| path.as_os_str()).collect();
    assert_eq!(spelled, ["p/z.rs", "./q.rs"]);
}

/// The merge's last key, the raw spelling, leaves no tie to the sort:
/// two spellings of one path under one seed rank keep the same survivor
/// whichever the workers reported first. `p/./src/v.rs` sorts before
/// `p/src/v.rs` byte-wise (`.` < `s`), so it survives both orders; with
/// the key dropped, each order keeps whichever spelling came first.
#[test]
fn merge_measured_breaks_a_same_rank_tie_on_the_spelling() {
    let dotted = PathBuf::from("p/./src/v.rs");
    let plain = PathBuf::from("p/src/v.rs");
    for input in [
        vec![(0, dotted.clone()), (0, plain.clone())],
        vec![(0, plain.clone()), (0, dotted.clone())],
    ] {
        let merged = merge_measured(input);
        let spelled: Vec<&std::ffi::OsStr> = merged.iter().map(|path| path.as_os_str()).collect();
        assert_eq!(
            spelled,
            ["p/./src/v.rs"],
            "one survivor, the byte-wise first"
        );
    }
}

/// The parallel ignore measurement finds exactly what the serial one
/// does (#1307): every worker's share reaches the merge, including the
/// calling thread's, and no directory is skipped.
///
/// The root and its 48 subdirectories each hold one ignored file and
/// one ignored directory beside a kept file, so every job contributes
/// exactly one of each and a lost job moves an exact count. The root
/// carries them too because it is usually the first job claimed:
/// without them, skipping job 0 went unnoticed here. A directory
/// claimed twice is not observable here: the merge dedupes the repeat,
/// so a double claim costs time, not output.
#[test]
fn parallel_ignore_measurement_matches_serial() {
    const DIRS: usize = 48;
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let proj = tmp.path().join("proj");
    std::fs::create_dir(&proj).expect("create proj");
    std::fs::write(proj.join(".gitignore"), "*.gen.rs\nbuild/\n").expect("write .gitignore");
    let walked =
        std::iter::once(proj.clone()).chain((0..DIRS).map(|d| proj.join(format!("pkg{d}"))));
    for dir in walked {
        std::fs::create_dir_all(dir.join("build")).expect("create dirs");
        std::fs::write(dir.join("kept.rs"), b"pub fn f() {}\n").expect("write kept");
        std::fs::write(dir.join("dropped.gen.rs"), b"pub fn f() {}\n").expect("write dropped");
    }

    let empty_include = mk_globset(Vec::new()).expect("empty globset");
    let empty_exclude = build_exclude_globset(Vec::new(), None, "--exclude-from");
    let cwd = std::env::current_dir().unwrap_or_default();
    let filters = WalkFilters {
        include: &empty_include,
        excludes: crate::walk_seed::AnchoredExcludes::new(
            &empty_exclude,
            &empty_exclude,
            None,
            &cwd,
            None,
        ),
        language_forced: false,
        measure_ignored: true,
    };
    let mut errors = WalkErrors::default();
    let walk = walk_directory_seed(&proj, false, 4, &filters, &mut errors);
    let mut found = SeedSet::default();
    for path in walk.files {
        found.push_walked(path);
    }
    let walked_dirs = vec![(proj, walk.dirs)];

    let serial = measure_ignored_entries(&walked_dirs, &found, &filters, 1);
    let parallel = measure_ignored_entries(&walked_dirs, &found, &filters, 8);

    assert_eq!(
        serial.files.len(),
        DIRS + 1,
        "one ignored file per walked directory"
    );
    assert_eq!(
        serial.dirs.len(),
        DIRS + 1,
        "one pruned directory per walked directory"
    );
    assert_eq!(parallel.files, serial.files);
    assert_eq!(parallel.dirs, serial.dirs);
}
