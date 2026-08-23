//! Pins the harness's orphan-snapshot guard (#1282): a `.snap` on disk
//! that no resolved corpus file accounts for must be reported. Runs
//! read-only over the committed PHP snapshot directory — withholding a
//! file from the expected set simulates the orphan — so it needs no
//! temp state and cannot race the corpus tests reading the same files.
#![allow(missing_docs)]
use crate::common;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[test]
fn orphan_snapshot_guard_reports_withheld_snapshot() {
    let php_root = Path::new(common::SNAPSHOT_PATH).join("php");
    // Enumerate the flat directory with read_dir rather than the guard's
    // own walker, so the expectation is independent of the code under
    // test.
    let mut all: Vec<PathBuf> = std::fs::read_dir(&php_root)
        .expect("php snapshot corpus missing; run `make worktree-setup`")
        .map(|entry| entry.expect("readable directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "snap"))
        .collect();
    all.sort();
    assert!(
        all.len() >= 2,
        "expected the committed php snapshots, found only {all:?}; the \
         withheld-file scenario below needs at least two",
    );

    let complete: HashSet<PathBuf> = all.iter().cloned().collect();
    assert_eq!(
        common::orphan_snapshots(&php_root, &complete),
        Vec::<PathBuf>::new(),
        "a fully-accounted-for snapshot tree must report no orphans",
    );

    let withheld = all.remove(0);
    let expected: HashSet<PathBuf> = all.into_iter().collect();
    assert_eq!(
        common::orphan_snapshots(&php_root, &expected),
        vec![withheld],
        "a snapshot absent from the expected set must be reported as an orphan",
    );
}
