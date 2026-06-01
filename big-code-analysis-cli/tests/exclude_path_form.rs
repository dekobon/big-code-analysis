//! Regression tests for #488: `--exclude-from` / `.bcaignore` glob
//! matching must be independent of how the walk root was spelled.
//!
//! The `.bcaignore` patterns are `./`-anchored to match the walker's
//! emitted form. Before #488 the walker emitted each file prefixed by
//! its raw seed, so an absolute walk root (`--paths "$PWD"` or a
//! manifest-resolved `paths = ["."]`) produced absolute file paths
//! that the `./`-anchored deny-set never matched — every exclude was
//! silently defeated. The walker now re-anchors the seed, so all three
//! forms (`--paths .`, `--paths <abs>`, and a discovered `bca.toml`
//! `paths = ["."]`) must walk the *same* file set.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

mod common;

fn cli(env_dir: &Path) -> Command {
    let mut cmd = common::bca_command();
    // Isolate from any user-level global gitignore so the walk is
    // deterministic across machines.
    cmd.env("HOME", env_dir)
        .env("XDG_CONFIG_HOME", env_dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    cmd
}

/// Build a fixture tree:
///   src/keep.py        — kept
///   vendor/drop.py     — excluded by `./vendor/**`
///   tests/drop.py      — excluded by `./tests/**`
/// plus a `./`-anchored `.bcaignore`.
fn make_tree(dir: &Path) {
    for (sub, name) in [
        ("src", "keep.py"),
        ("vendor", "drop.py"),
        ("tests", "drop.py"),
    ] {
        let d = dir.join(sub);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(name), "def f(): return 1\n").unwrap();
    }
    // `./`-anchored patterns: the form the project's own `.bcaignore`
    // and the walker's `--paths .` output both use.
    std::fs::write(dir.join(".bcaignore"), "./vendor/**\n./tests/**\n").unwrap();
}

/// Recursively collect the basenames of every emitted `*.json` metric
/// file, sorted. This is the walked-and-not-excluded set: the offender
/// set's underlying file set, expressed without any path-form noise.
fn emitted_json(out: &Path) -> Vec<String> {
    fn visit(dir: &Path, found: &mut Vec<String>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    visit(&p, found);
                } else if p.extension().and_then(|e| e.to_str()) == Some("json") {
                    found.push(
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .expect("UTF-8 fixture name")
                            .to_owned(),
                    );
                }
            }
        }
    }
    let mut found = Vec::new();
    visit(out, &mut found);
    found.sort();
    found
}

/// Run `bca metrics` with the given `--paths` seed (relative to
/// `cwd`) and return the emitted JSON basenames. `cwd` is the walk
/// root the command runs from; `seed` is whatever is handed to
/// `--paths`.
fn walked_with_seed(fixture: &Path, cwd: &Path, seed: &str) -> Vec<String> {
    let out = TempDir::new().unwrap();
    cli(fixture)
        .current_dir(cwd)
        .args([
            "--paths",
            seed,
            "--exclude-from",
            // `.bcaignore` lives at the fixture root; reference it
            // absolutely so the form under test is the *walk seed*,
            // not the exclude-file path.
            fixture.join(".bcaignore").to_str().unwrap(),
            "metrics",
            "-O",
            "json",
            "-o",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    emitted_json(out.path())
}

/// Run `bca metrics` letting an auto-discovered `bca.toml` supply both
/// `paths = ["."]` and `exclude_from = ".bcaignore"` — no path or
/// exclude flags on the command line. This resolves `paths` to an
/// absolute root against the manifest directory, exactly the form #488
/// fixed.
fn walked_via_manifest(fixture: &Path) -> Vec<String> {
    std::fs::write(
        fixture.join("bca.toml"),
        "paths = [\".\"]\nexclude_from = \".bcaignore\"\n",
    )
    .unwrap();
    let out = TempDir::new().unwrap();
    cli(fixture)
        .current_dir(fixture)
        .args(["metrics", "-O", "json", "-o", out.path().to_str().unwrap()])
        .assert()
        .success();
    let names = emitted_json(out.path());
    // Drop the manifest so it cannot leak into a sibling test.
    std::fs::remove_file(fixture.join("bca.toml")).unwrap();
    names
}

#[test]
fn exclude_set_is_identical_across_path_forms() {
    let dir = TempDir::new().unwrap();
    // Resolve symlinks (macOS `/var` → `/private/var`) so the absolute
    // seed strictly equals the canonical CWD the binary observes;
    // otherwise `strip_prefix` would not match and the abs form would
    // (correctly) keep its absolute identity, diverging from `.`.
    let fixture = dir.path().canonicalize().unwrap();
    make_tree(&fixture);

    // Form 1: `--paths .`, run from inside the fixture.
    let dot = walked_with_seed(&fixture, &fixture, ".");
    // Form 2: `--paths <abs>`, run from anywhere.
    let abs_seed: PathBuf = fixture.clone();
    let abs = walked_with_seed(&fixture, &fixture, abs_seed.to_str().unwrap());
    // Form 3: discovered manifest `paths = ["."]` (→ absolute root).
    let manifest = walked_via_manifest(&fixture);

    // The deny-set drops vendor/ and tests/, leaving only keep.py —
    // the same for every path form. Pin the exact contents so a
    // regression that re-broke absolute-root exclusion (leaking
    // drop.py back in) turns this red, not just an equality that an
    // empty-everywhere bug could satisfy.
    let expected = vec!["keep.py.json".to_string()];
    assert_eq!(dot, expected, "--paths . should exclude vendor/ and tests/");
    assert_eq!(
        abs, expected,
        "--paths <abs> must exclude the same files as --paths ."
    );
    assert_eq!(
        manifest, expected,
        "manifest paths=[\".\"] must exclude the same files as --paths ."
    );

    // Byte-identical across all three forms — the #376 contract.
    assert_eq!(dot, abs);
    assert_eq!(dot, manifest);
}

/// Run `bca metrics` with an auto-discovered `bca.toml` (`paths =
/// ["."]`, `exclude_from = ".bcaignore"`) from `run_from`, which may be
/// a subdirectory below the manifest directory. The manifest is climbed
/// to from `run_from`, so `paths = ["."]` resolves against the manifest
/// dir — an ancestor of `run_from` — exercising the #489 case.
fn walked_via_manifest_from(fixture: &Path, run_from: &Path) -> Vec<String> {
    std::fs::write(
        fixture.join("bca.toml"),
        "paths = [\".\"]\nexclude_from = \".bcaignore\"\n",
    )
    .unwrap();
    let out = TempDir::new().unwrap();
    cli(fixture)
        .current_dir(run_from)
        .args(["metrics", "-O", "json", "-o", out.path().to_str().unwrap()])
        .assert()
        .success();
    let names = emitted_json(out.path());
    std::fs::remove_file(fixture.join("bca.toml")).unwrap();
    names
}

#[test]
fn manifest_excludes_apply_when_run_from_subdir() {
    // #489: a manifest-driven walk invoked from a subdirectory *below*
    // the manifest directory must still honor the `./`-anchored
    // `.bcaignore`. `paths = ["."]` resolves to the manifest dir (an
    // ancestor of the CWD), which #488's CWD-relative seed re-anchoring
    // cannot collapse — pre-fix the absolute walk root leaked the
    // vendored file straight past the deny-set. Glob matching is now
    // anchored to the walk root, so the exclude applies regardless of
    // which subdir launched the run.
    let dir = TempDir::new().unwrap();
    let fixture = dir.path().canonicalize().unwrap();
    make_tree(&fixture);
    let subdir = fixture.join("src");
    assert!(subdir.is_dir(), "fixture must contain the src/ subdir");

    let from_subdir = walked_via_manifest_from(&fixture, &subdir);
    assert_eq!(
        from_subdir,
        vec!["keep.py.json".to_string()],
        "running from a subdir must still exclude vendor/ and tests/ \
         via the manifest's ./-anchored .bcaignore (#489)"
    );

    // No regression for #488's repo-root invocation nor the `--paths
    // \"$PWD\"` form: both must still drop the vendored/test files.
    let from_root = walked_via_manifest_from(&fixture, &fixture);
    assert_eq!(
        from_root,
        vec!["keep.py.json".to_string()],
        "repo-root manifest invocation must still exclude (no #488 regression)"
    );
    let pwd_seed = walked_with_seed(&fixture, &fixture, fixture.to_str().unwrap());
    assert_eq!(
        pwd_seed,
        vec!["keep.py.json".to_string()],
        "--paths \"$PWD\" must still exclude (no #488 regression)"
    );
}

#[test]
fn absolute_subdir_seed_still_excludes() {
    // A seed that is an absolute path to a *subdirectory* of the walk
    // root re-anchors to its relative tail, so a `./`-anchored pattern
    // beneath it still matches. Here `./tests/**` must still drop the
    // file when the seed is the absolute fixture root, while `src/` is
    // kept.
    let dir = TempDir::new().unwrap();
    let fixture = dir.path().canonicalize().unwrap();
    make_tree(&fixture);

    let abs = walked_with_seed(&fixture, &fixture, fixture.to_str().unwrap());
    assert_eq!(
        abs,
        vec!["keep.py.json".to_string()],
        "absolute root seed must honor the ./-anchored deny-set"
    );
}
