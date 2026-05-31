use super::reanchor_seed;
use std::path::{Path, PathBuf};

#[test]
fn relative_seed_is_unchanged() {
    // `.`, `./`, and a subdir seed are already in the form the
    // exclude patterns expect — leave them untouched.
    assert_eq!(reanchor_seed(PathBuf::from(".")), Path::new("."));
    assert_eq!(reanchor_seed(PathBuf::from("./")), Path::new("./"));
    assert_eq!(reanchor_seed(PathBuf::from("src")), Path::new("src"));
    assert_eq!(reanchor_seed(PathBuf::from("a/b/c")), Path::new("a/b/c"));
}

#[test]
fn absolute_cwd_becomes_dot() {
    // `--paths "$PWD"`: the absolute CWD collapses to `.` so the
    // walker emits the same `./`-prefixed paths as `--paths .`.
    let cwd = std::env::current_dir().expect("cwd available in test");
    assert_eq!(reanchor_seed(cwd), Path::new("."));
}

#[test]
fn absolute_cwd_with_trailing_curdir_becomes_dot() {
    // Manifest `paths = ["."]` resolves to `<manifest_dir>/.`; when
    // the manifest dir is the CWD this is `<cwd>/.`, which must also
    // collapse to `.`.
    let mut seed = std::env::current_dir().expect("cwd available in test");
    seed.push(".");
    assert_eq!(reanchor_seed(seed), Path::new("."));
}

#[test]
fn absolute_subdir_becomes_relative_remainder() {
    // An absolute path under the CWD keeps only its relative tail,
    // matching what `--paths <subdir>` would have produced.
    let mut seed = std::env::current_dir().expect("cwd available in test");
    seed.push("src");
    seed.push("foo");
    assert_eq!(reanchor_seed(seed), Path::new("src/foo"));
}

#[test]
fn absolute_sibling_tree_is_unchanged() {
    // A seed outside the CWD has no relative form anchored to the
    // patterns; preserve its absolute identity verbatim.
    let outside = if cfg!(windows) {
        PathBuf::from(r"C:\definitely\not\under\cwd")
    } else {
        PathBuf::from("/definitely/not/under/cwd")
    };
    assert_eq!(reanchor_seed(outside.clone()), outside);
}
