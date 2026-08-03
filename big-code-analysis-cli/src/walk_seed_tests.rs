use super::{
    anchor_against_seeds, file_seed_match_path, match_path_for, reanchor_seed, strip_cur_dir,
    strip_dot_slash,
};
use std::path::{Path, PathBuf};

#[cfg(unix)]
#[test]
fn relative_tail_canonicalizes_both_sides_when_forms_diverge() {
    // Imported here (not at module scope) so the symbol is not "unused"
    // on non-unix targets, where this is the only caller in tests.
    use super::relative_tail;
    // The third fallback: when `path` and `cwd` are spelled in forms that
    // share no lexical prefix even after canonicalizing `path` (on Windows
    // a `\\?\`-verbatim canonical path vs a non-verbatim CWD; simulated
    // here with a symlinked `cwd`), canonicalizing BOTH sides must still
    // recover the relative tail. Pre-fix this returned `None` and the
    // explicit-file include anchoring silently fell back to the absolute
    // as-spelled form on Windows CI.
    let td = tempfile::tempdir().expect("tempdir");
    let real_root = td.path().canonicalize().expect("canonical tempdir");
    let sub = real_root.join("sub");
    std::fs::create_dir(&sub).expect("create subdir");
    std::fs::write(sub.join("f.rs"), "fn f() {}\n").expect("write fixture");
    let link = real_root.join("root-link");
    std::os::unix::fs::symlink(&real_root, &link).expect("create symlink");

    // `path` is canonical; `cwd` is spelled through the symlink, so the
    // lexical strip and the canonical-`path` strip both fail — only the
    // canonical-`cwd` retry succeeds.
    assert_eq!(
        relative_tail(&sub.join("f.rs"), &link),
        Some(PathBuf::from("sub/f.rs")),
        "diverging path/cwd spellings must still yield the relative tail"
    );
}

/// #1164: a manifest glob is written against the `bca.toml` directory,
/// so both spellings a per-file caller produces — the path as typed and
/// the absolute one — must reduce to the same manifest-relative form
/// even when the process is standing in a subdirectory.
///
/// The root is deliberately an *ancestor* of the working directory —
/// the workspace root while the test process stands in the crate
/// directory, exactly the shape a `bca.toml` at the repo root sees from
/// a subdirectory. With the two the same directory the anchors coincide
/// and the assertion holds under either rule. `set_current_dir` is
/// process-global and would race the other tests in this binary, so the
/// subdirectory comes from cargo's own cwd rather than a fixture.
/// Build a [`super::ManifestAnchor`] for a test. Exists so the tests
/// read as `(root, cwd)` at each call rather than repeating the struct
/// literal, and so a future field lands in one place.
fn anchor<'a>(root: Option<&'a Path>, cwd: &'a std::path::PathBuf) -> super::ManifestAnchor<'a> {
    super::ManifestAnchor::resolve(root, cwd)
}

#[test]
fn root_relative_match_path_anchors_both_spellings_from_a_subdirectory() {
    let cwd = std::env::current_dir().expect("cwd available in test");
    let root = cwd.parent().expect("crate dir has a parent");
    let crate_dir = cwd.file_name().expect("crate dir has a name");
    let expected = Path::new(crate_dir).join("src").join("lib.rs");

    // Spelled relative to the CWD, and spelled absolutely: one answer.
    assert_eq!(
        super::root_relative_match_path(anchor(Some(root), &cwd), Path::new("src/lib.rs")),
        Some(expected.clone())
    );
    assert_eq!(
        super::root_relative_match_path(anchor(Some(root), &cwd), &cwd.join("src").join("lib.rs")),
        Some(expected)
    );
}

/// `strip_prefix` is purely lexical and keeps `..` components, so a
/// path spelled through a parent must be folded before it is compared:
/// otherwise `bca check ../src/lib.rs` from a sibling directory reduces
/// to `<sibling>/../src/lib.rs` and a `<sibling>/**` glob — describing a
/// directory the file is *not* in — exempts it from the gate.
#[test]
fn root_relative_match_path_folds_parent_components_before_comparing() {
    let cwd = std::env::current_dir().expect("cwd available in test");
    let root = cwd.parent().expect("crate dir has a parent");

    // `../src/lib.rs` from the crate directory is the *workspace's*
    // `src/lib.rs`, not the crate's, and must not read as living under
    // the crate directory.
    assert_eq!(
        super::root_relative_match_path(anchor(Some(root), &cwd), Path::new("../src/lib.rs")),
        Some(PathBuf::from("src/lib.rs"))
    );
}

/// A path outside the manifest's tree has no manifest-relative
/// identity, so [`super::manifest_match_path`] hands back the
/// working-directory form its caller already computed rather than
/// matching nothing. Same for a run with no manifest at all.
#[test]
fn manifest_match_path_falls_back_to_the_cwd_form() {
    let td = tempfile::tempdir().expect("tempdir");
    let root = td.path().canonicalize().expect("canonical tempdir");
    let outside = if cfg!(windows) {
        PathBuf::from(r"C:\definitely\not\under\root\f.rs")
    } else {
        PathBuf::from("/definitely/not/under/root/f.rs")
    };
    let cwd_form = Path::new("vendor/f.rs");
    let cwd = std::env::current_dir().expect("cwd available in test");

    assert_eq!(
        super::manifest_match_path(
            anchor(Some(&root), &cwd),
            &outside,
            super::CwdForm(cwd_form)
        )
        .as_ref(),
        cwd_form,
        "a path outside the manifest tree keeps the CWD form"
    );
    assert_eq!(
        super::manifest_match_path(anchor(None, &cwd), &outside, super::CwdForm(cwd_form)).as_ref(),
        cwd_form,
        "no manifest means no second anchor to try"
    );
}

#[test]
fn file_seed_match_path_anchors_absolute_file_under_cwd_to_relative_tail() {
    // #726 include-side: `--paths "$PWD/src/lib.rs" --include 'src/**'`
    // must match like `--paths src/lib.rs`. The match form (not the
    // emitted name) becomes the CWD-relative tail.
    let mut seed = std::env::current_dir().expect("cwd available in test");
    seed.push("src");
    seed.push("lib.rs");
    assert!(seed.is_file(), "crate src/lib.rs must exist for this test");
    assert_eq!(file_seed_match_path(&seed), Path::new("src/lib.rs"));
}

#[test]
fn file_seed_match_path_leaves_relative_and_outside_cwd_seeds_as_spelled() {
    // A relative seed already is its own match form.
    assert_eq!(
        file_seed_match_path(Path::new("vendor/drop.py")),
        Path::new("vendor/drop.py")
    );
    // A file outside the CWD has no relative identity: as spelled.
    let outside = if cfg!(windows) {
        PathBuf::from(r"C:\definitely\not\under\cwd\f.rs")
    } else {
        PathBuf::from("/definitely/not/under/cwd/f.rs")
    };
    assert_eq!(file_seed_match_path(&outside), outside);
}

#[test]
fn strip_dot_slash_normalises_only_a_single_leading_dot_slash() {
    // #726: `dir/**` and `./dir/**` must compile to the identical glob.
    assert_eq!(strip_dot_slash("./cve-corpus/**"), "cve-corpus/**");
    assert_eq!(strip_dot_slash("cve-corpus/**"), "cve-corpus/**");
    // Forms that already work are untouched: `**/`, `*`, and absolute.
    assert_eq!(strip_dot_slash("**/cve-corpus/**"), "**/cve-corpus/**");
    assert_eq!(strip_dot_slash("*.rs"), "*.rs");
    assert_eq!(strip_dot_slash("/abs/dir/**"), "/abs/dir/**");
    // A lone `.` (no slash) is not a `./` prefix and is left as-is.
    assert_eq!(strip_dot_slash("."), ".");
    // A doubled-slash spelling is NOT stripped: `.//x` minus `./` would
    // be the absolute-anchored `/x`, silently changing the pattern's
    // meaning. Malformed input keeps its (non-matching) form.
    assert_eq!(strip_dot_slash(".//x"), ".//x");
    // A bare `./` strips to empty; `mk_globset` skips it post-strip.
    assert_eq!(strip_dot_slash("./"), "");
}

#[test]
fn strip_cur_dir_strips_only_a_leading_curdir_component() {
    // #726: the match-path side drops a leading `./` so it compares in the
    // same no-`./` space as a `strip_dot_slash`-normalised pattern.
    assert_eq!(
        strip_cur_dir(Path::new("./cve-corpus/foo.c")),
        Path::new("cve-corpus/foo.c")
    );
    // Bare-relative and absolute paths have no leading `CurDir`: untouched.
    assert_eq!(
        strip_cur_dir(Path::new("cve-corpus/foo.c")),
        Path::new("cve-corpus/foo.c")
    );
    assert_eq!(
        strip_cur_dir(Path::new("/abs/cve-corpus/foo.c")),
        Path::new("/abs/cve-corpus/foo.c")
    );
}

#[test]
fn bare_relative_pattern_matches_match_path_for_every_seed_form() {
    // #726 core parity: a globset built from the bare-relative `cve-corpus/**`
    // must exclude the `match_path_for(seed, file)` form for every seed
    // spelling, exactly as the `./cve-corpus/**` spelling already did.
    use globset::{Glob, GlobSet, GlobSetBuilder};

    fn globset_of(pattern: &str) -> GlobSet {
        let mut b = GlobSetBuilder::new();
        b.add(Glob::new(strip_dot_slash(pattern)).expect("valid glob"));
        b.build().expect("valid globset")
    }

    let bare = globset_of("cve-corpus/**");
    let dotted = globset_of("./cve-corpus/**");

    // Absolute walk root, `.` walk root, and bare-relative subdir seed all
    // emit a `./`-prefixed match path that the bare pattern must now match.
    let cases = [
        (
            PathBuf::from("/repo"),
            PathBuf::from("/repo/cve-corpus/x.c"),
        ),
        (PathBuf::from("."), PathBuf::from("./cve-corpus/x.c")),
        (PathBuf::from("src"), PathBuf::from("src/cve-corpus/x.c")),
    ];
    for (seed, file) in cases {
        let match_path = match_path_for(&seed, &file);
        let stripped = strip_cur_dir(&match_path);
        assert!(
            bare.is_match(stripped),
            "bare `cve-corpus/**` must match {match_path:?} (seed {seed:?})"
        );
        assert_eq!(
            bare.is_match(stripped),
            dotted.is_match(stripped),
            "`cve-corpus/**` and `./cve-corpus/**` must agree for {match_path:?}"
        );
    }

    // A sibling directory must NOT be excluded by either spelling — guards
    // against the strip widening the match.
    let keep = match_path_for(&PathBuf::from("/repo"), &PathBuf::from("/repo/src/x.c"));
    let keep = strip_cur_dir(&keep);
    assert!(
        !bare.is_match(keep),
        "bare pattern must not match a sibling dir"
    );
    assert!(
        !dotted.is_match(keep),
        "dotted pattern must not match a sibling dir"
    );
}

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

#[cfg(unix)]
#[test]
fn symlinked_seed_under_cwd_becomes_relative_remainder() {
    // Regression: `current_dir()` returns the canonical CWD (getcwd
    // resolves every symlink), so a seed spelled through a symlinked
    // ancestor shares no lexical prefix with it. `reanchor_seed` must
    // canonicalize the seed before stripping, or the seed stays absolute
    // and every emitted file nests under its full path. This is the
    // default on macOS — a `TempDir` (`/var/folders/…`) and `/tmp` are
    // both symlinks into `/private` — and is what flattened the
    // `metrics -o` output the include/exclude integration tests assert on.
    //
    // The symlink targets the crate `src/` dir (a real directory under the
    // CWD); the symlink itself lives in a tempdir *outside* the CWD, so
    // pre-fix the raw `strip_prefix(cwd)` fails and the absolute symlink
    // path survives — exactly the bug.
    let td = tempfile::tempdir().expect("tempdir");
    let target = std::env::current_dir()
        .expect("cwd available in test")
        .join("src");
    assert!(target.is_dir(), "crate `src/` must exist for this test");
    let link = td.path().join("seed-link");
    std::os::unix::fs::symlink(&target, &link).expect("create symlink");
    assert_eq!(
        reanchor_seed(link),
        Path::new("src"),
        "a symlinked seed resolving under the CWD must reanchor to its \
         canonical relative tail, not stay an absolute path"
    );
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
fn anchor_against_seeds_lets_a_later_dir_seed_anchor_a_file_seed_path() {
    // A path equal to an *earlier* file seed must not shadow a *later*
    // directory seed that contains it: the file-seed match is skipped so
    // the dir seed anchors the path to the `./`-form a `./`-anchored
    // `[check.exclude]` expects. (Regressed when the loop `break`'d
    // instead of trying the next seed.)
    let seeds = vec![PathBuf::from("/abs/repo/x.rs"), PathBuf::from("/abs/repo")];
    assert_eq!(
        anchor_against_seeds(&seeds, Path::new("/abs/repo/x.rs")),
        Path::new("./x.rs")
    );
}

#[test]
fn anchor_against_seeds_skips_a_non_containing_seed_and_anchors_under_a_later_one() {
    // A seed that does not contain the path is skipped; a later seed
    // that does contain it anchors the path to the `./`-form.
    let seeds = vec![PathBuf::from("/other"), PathBuf::from("/abs/repo")];
    assert_eq!(
        anchor_against_seeds(&seeds, Path::new("/abs/repo/src/a.rs")),
        Path::new("./src/a.rs")
    );
}

#[test]
fn anchor_against_seeds_passes_through_when_no_seed_contains_path() {
    // No seed is a prefix of the path: returned unchanged.
    assert_eq!(
        anchor_against_seeds(&[PathBuf::from("/nope")], Path::new("/abs/repo/v.rs")),
        Path::new("/abs/repo/v.rs")
    );
}
