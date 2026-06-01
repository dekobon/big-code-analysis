use super::{anchor_against_seeds, match_path_for, reanchor_seed};
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
    // An absolute path to an existing *directory* under the CWD keeps
    // only its relative tail, matching what `--paths <subdir>` would
    // have produced. `src` is a real directory in this crate, so the
    // `is_dir()` gate (directory-only re-anchoring) is satisfied.
    let mut seed = std::env::current_dir().expect("cwd available in test");
    seed.push("src");
    assert!(seed.is_dir(), "crate `src/` must exist for this test");
    assert_eq!(reanchor_seed(seed), Path::new("src"));
}

#[test]
fn absolute_file_seed_is_unchanged() {
    // Regression for #488's emission fix: an absolute path to a single
    // *file* under the CWD must NOT be re-anchored. Excludes only
    // filter tree walks, never an explicit file seed, so the file's
    // emitted `name` must keep the absolute form the caller passed —
    // this is what `bca metrics --paths /abs/file.rs` echoes and what
    // the single-file `bca.analyze()` API matches. Re-anchoring it to a
    // CWD-relative path silently broke that parity (the Python binding
    // CLI-parity tests caught it; `cargo test` did not). `Cargo.toml`
    // is a real file at the crate root, i.e. under the CWD.
    let mut seed = std::env::current_dir().expect("cwd available in test");
    seed.push("Cargo.toml");
    assert!(
        seed.is_file(),
        "crate `Cargo.toml` must exist for this test"
    );
    assert_eq!(
        reanchor_seed(seed.clone()),
        seed.as_path(),
        "an absolute single-file seed must keep its as-given absolute path"
    );
}

#[test]
fn nonexistent_absolute_seed_is_unchanged() {
    // A seed that does not exist has unknown kind; `is_dir()` is false,
    // so it is left verbatim. The walker's downstream "File doesn't
    // exist" warning then reports the path the user actually spelled.
    let mut seed = std::env::current_dir().expect("cwd available in test");
    seed.push("definitely-not-a-real-entry-zzz");
    assert!(!seed.exists(), "guard: this path must not exist");
    assert_eq!(reanchor_seed(seed.clone()), seed.as_path());
}

#[test]
fn match_path_anchors_absolute_walk_root_to_dot_relative() {
    // The #489 core: a file emitted under an *absolute* walk root
    // (a manifest `paths = ["."]` resolved to its dir, which may be an
    // ancestor of the CWD) must match against its `./`-prefixed tail so
    // the `./`-anchored deny-set still applies.
    let seed = PathBuf::from("/repo");
    let path = PathBuf::from("/repo/vendor/drop.rs");
    assert_eq!(
        match_path_for(&seed, &path),
        Path::new("./vendor/drop.rs"),
        "absolute walk root must anchor matching to ./-relative tail"
    );
}

#[test]
fn match_path_handles_reanchored_dot_seed_without_double_prefix() {
    // The reanchored `.` seed emits files already carrying a leading
    // `./`; stripping `.` is lexical and skips the `CurDir` component,
    // so the result is a single `./`-prefixed path, never `././`.
    let seed = PathBuf::from(".");
    let path = PathBuf::from("./vendor/drop.rs");
    assert_eq!(
        match_path_for(&seed, &path),
        Path::new("./vendor/drop.rs"),
        "reanchored `.` seed must not double the `./` prefix"
    );
}

#[test]
fn match_path_relative_subdir_seed_anchors_to_dot_relative() {
    // A bare relative subdir seed (`--paths src`) emits `src/...`; the
    // match path strips the seed and re-prefixes, so `./`-anchored
    // patterns are evaluated against the walk-root tail.
    let seed = PathBuf::from("src");
    let path = PathBuf::from("src/languages/language_rust.rs");
    assert_eq!(
        match_path_for(&seed, &path),
        Path::new("./languages/language_rust.rs")
    );
}

#[test]
fn match_path_returns_unchanged_when_not_under_seed() {
    // Defensive fallback: a path the walker could not have produced from
    // `seed` is returned verbatim rather than mangled.
    let seed = PathBuf::from("/repo");
    let path = PathBuf::from("/elsewhere/foo.rs");
    assert_eq!(match_path_for(&seed, &path), Path::new("/elsewhere/foo.rs"));
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

#[test]
fn anchor_against_seeds_anchors_path_under_an_absolute_seed() {
    // #493: a violation path emitted under an absolute walk root (a
    // manifest `paths=["."]` resolved above the CWD) anchors to the
    // `./`-relative form so a `./`-anchored `[check.exclude]` matches.
    let seeds = vec![PathBuf::from("/abs/repo/.")];
    assert_eq!(
        anchor_against_seeds(&seeds, Path::new("/abs/repo/./vendor/v.rs")),
        Path::new("./vendor/v.rs")
    );
    // A relative `.` seed leaves an already-anchored path unchanged.
    assert_eq!(
        anchor_against_seeds(&[PathBuf::from(".")], Path::new("./vendor/v.rs")),
        Path::new("./vendor/v.rs")
    );
}

#[test]
fn anchor_against_seeds_leaves_single_file_seed_as_spelled() {
    // path == seed (a single explicit file `--paths`): matched as the
    // caller spelled it, mirroring the walk's file-seed branch.
    let seeds = vec![PathBuf::from("/abs/repo/x.rs")];
    assert_eq!(
        anchor_against_seeds(&seeds, Path::new("/abs/repo/x.rs")),
        Path::new("/abs/repo/x.rs")
    );
}

#[test]
fn anchor_against_seeds_passes_through_when_no_seed_contains_path() {
    // First matching seed wins; an unrelated seed leaves the path as-is.
    let seeds = vec![PathBuf::from("/other"), PathBuf::from("/abs/repo")];
    assert_eq!(
        anchor_against_seeds(&seeds, Path::new("/abs/repo/src/a.rs")),
        Path::new("./src/a.rs")
    );
    assert_eq!(
        anchor_against_seeds(&[PathBuf::from("/nope")], Path::new("/abs/repo/v.rs")),
        Path::new("/abs/repo/v.rs")
    );
}
