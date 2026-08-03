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
        ),
        language_forced: false,
    };
    let mut errors = WalkErrors::default();
    let found = walk_directory_seed(root, true, 8, &filters, &mut errors);

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
