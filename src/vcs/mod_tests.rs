use super::*;
use std::path::{Path, PathBuf};

fn index_with(workdir: Option<&str>, files: &[&str]) -> HistoryIndex {
    let map: HashMap<PathBuf, Stats> = files
        .iter()
        .map(|f| (PathBuf::from(f), Stats::default()))
        .collect();
    HistoryIndex::new(map, workdir.map(PathBuf::from), false)
}

#[test]
fn accessors_reflect_construction() {
    let idx = index_with(Some("/repo"), &["a.rs", "b.rs"]);
    assert_eq!(idx.len(), 2);
    assert!(!idx.is_empty());
    assert!(idx.get(Path::new("a.rs")).is_some());
    assert!(idx.get(Path::new("missing.rs")).is_none());
    assert_eq!(idx.workdir(), Some(Path::new("/repo")));
    assert!(!idx.truncated_shallow_clone());
}

#[test]
fn default_index_is_empty() {
    let idx = HistoryIndex::default();
    assert_eq!(idx.len(), 0);
    assert!(idx.is_empty());
    assert!(idx.workdir().is_none());
    assert!(!idx.truncated_shallow_clone());
}

#[test]
fn get_for_path_strips_the_workdir_prefix() {
    let idx = index_with(Some("/repo"), &["src/a.rs"]);
    // Absolute path inside the work tree resolves to its relative entry.
    assert!(idx.get_for_path(Path::new("/repo/src/a.rs")).is_some());
    // A path outside the work tree cannot be made relative → None.
    assert!(idx.get_for_path(Path::new("/elsewhere/src/a.rs")).is_none());
}

#[test]
fn get_for_path_returns_none_for_a_bare_repo() {
    // No work tree (bare repo) → an absolute path can never be resolved.
    let idx = index_with(None, &["src/a.rs"]);
    assert!(idx.get_for_path(Path::new("/repo/src/a.rs")).is_none());
}

#[test]
fn iter_and_into_files_expose_the_map() {
    let idx = index_with(Some("/repo"), &["a.rs", "b.rs"]);
    let mut seen: Vec<_> = idx.iter().map(|(p, _)| p.clone()).collect();
    seen.sort();
    assert_eq!(seen, vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
    assert_eq!(idx.into_files().len(), 2);
}

#[derive(Debug)]
struct Entry {
    path: String,
    risk: f64,
}

fn ranked(entries: Vec<(&str, f64)>, top: usize) -> Vec<String> {
    let mut entries: Vec<Entry> = entries
        .into_iter()
        .map(|(path, risk)| Entry {
            path: path.to_owned(),
            risk,
        })
        .collect();
    rank_by_risk(&mut entries, top, |e| (e.path.as_str(), e.risk));
    entries.into_iter().map(|e| e.path).collect()
}

#[test]
fn rank_orders_by_descending_risk() {
    assert_eq!(
        ranked(vec![("low", 1.0), ("high", 9.0), ("mid", 5.0)], 0),
        vec!["high", "mid", "low"]
    );
}

#[test]
fn rank_breaks_ties_on_path_ascending() {
    // Equal risk must yield a deterministic, path-ordered result.
    assert_eq!(
        ranked(vec![("zeta", 5.0), ("alpha", 5.0)], 0),
        vec!["alpha", "zeta"]
    );
}

#[test]
fn rank_truncates_to_top_n() {
    assert_eq!(
        ranked(vec![("a", 1.0), ("b", 2.0), ("c", 3.0)], 2),
        vec!["c", "b"]
    );
}

#[test]
fn rank_top_zero_keeps_all() {
    assert_eq!(ranked(vec![("a", 1.0), ("b", 2.0)], 0).len(), 2);
}

#[test]
fn rank_nan_risk_falls_back_to_path_order() {
    // NaN never occurs in production, but the `unwrap_or(Equal)` branch
    // must still produce a stable path-ordered result, not a panic.
    assert_eq!(
        ranked(vec![("beta", f64::NAN), ("alpha", f64::NAN)], 0),
        vec!["alpha", "beta"]
    );
}
