//! Workspace task runner.
//!
//! `cargo xtask` (no args) regenerates the man pages for `bca` and
//! `bca-web` under `man/` at the repo root, one `.1` per top-level
//! binary plus one per `bca` subcommand. CI gates the output with
//! `utils/check-manpage-drift.py` — modified, deleted, *and* newly
//! added pages — so adding a flag or a subcommand without re-running
//! `cargo xtask` and committing the result fails the manpage job.
#![allow(missing_docs)]
#![allow(clippy::pedantic)]
// Production-only `unwrap()` ban. See `[workspace.lints.clippy]` in the
// root `Cargo.toml` for why this is a per-root attribute and not a
// Cargo lint (#1227).
#![cfg_attr(not(test), warn(clippy::unwrap_used))]

use std::{
    env,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::CommandFactory;

fn main() -> ExitCode {
    let workspace_root = workspace_root();
    let mut args = env::args_os().skip(1);
    // `to_str()` returns None for non-UTF-8 — route those to the
    // unknown arm so a stray non-UTF-8 byte cannot silently invoke
    // man-page generation.
    match args.next().as_deref().map(OsStr::to_str) {
        None => run_manpages(&workspace_root).map_or_else(io_exit, |()| ExitCode::SUCCESS),
        Some(other) => {
            let label = other.unwrap_or("<non-utf8>");
            eprintln!("xtask: unknown subcommand `{label}` (expected none)");
            ExitCode::from(2)
        }
    }
}

fn io_exit(e: io::Error) -> ExitCode {
    eprintln!("xtask: {e}");
    ExitCode::FAILURE
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask must be a workspace member")
        .to_path_buf()
}

fn run_manpages(workspace_root: &Path) -> io::Result<()> {
    let out_dir = workspace_root.join("man");
    fs::create_dir_all(&out_dir)?;

    // Top-level binary + every subcommand (recursive). The `version`
    // string read off the parent `Command` is the source-of-truth for
    // every page in the tree — clap does not propagate `version` down
    // to subcommands unless the parser opts in with
    // `propagate_version`, and turning that on would surface a
    // pointless `bca metrics --version` at runtime.
    let mut expected = Vec::<String>::new();
    render_tree(
        big_code_analysis_cli::Cli::command(),
        &out_dir,
        &mut expected,
    )?;
    // `bca-web` has no subcommands; the recursion is a no-op for it.
    render_tree(
        big_code_analysis_web::cli::Opts::command(),
        &out_dir,
        &mut expected,
    )?;

    // Sweep orphan `.1` files (renamed/removed subcommands) so the CI
    // man-page drift gate flips red on stale pages instead of silently
    // shipping them. The deletion shows up in `git diff`; the page the
    // rename *adds* needs the gate's untracked half (#1249). A
    // *case-only* rename is the one shape the sweep refuses to resolve
    // on its own and reports as an error (#1250).
    sweep_orphans(&out_dir, &expected)?;

    println!("Wrote man pages to {}", out_dir.display());
    Ok(())
}

/// Build a root `Command` so its `global = true` args reach every
/// subcommand before we render any page from the tree.
///
/// clap copies a global arg into its subcommands only inside
/// `Command::build()` (`_build_self` -> `_propagate_global_args`).
/// `clap_mangen::Man::new` does call `build()`, but on the *isolated*
/// clone it is handed — a subcommand plucked out of an unbuilt parent
/// has no parent left to inherit from, so the globals were simply
/// absent. That silently dropped `-w/--warnings` and `--report-skipped`
/// from all 18 subcommand pages, and the whole `vcs` history-tuning
/// family from `bca-vcs-commit.1` / `bca-vcs-trend.1` (#1248).
///
/// `build()` is recursive, so one call per root also fixes the nested
/// `vcs commit` / `vcs trend` pages, and re-building inside `Man::new`
/// is idempotent (guarded by clap's `AppSettings::Built`).
///
/// The same `build()` also inserts clap's own `help` subcommand, which
/// `clap_mangen` then lists in SUBCOMMANDS as `bca\-help(1)` — a page
/// nothing writes, because a `help` page documents the `-h` flag every
/// other page already documents. Both halves of that were true before
/// this function existed, so `man/bca.1` and `man/bca-vcs.1` shipped a
/// cross-reference to a nonexistent page. Suppressing the subcommand
/// removes the reference at its source, which listing it and writing a
/// contentless page would not.
///
/// This affects only the throwaway `Command` xtask renders from: the
/// shipped binary builds its own, so `bca help metrics` keeps working.
fn built(cmd: clap::Command) -> clap::Command {
    // `disable_help_subcommand` is a *global* setting, so one call on
    // the root reaches `vcs` and every future nested group during
    // `build()` — there is no per-level call to forget.
    let mut cmd = cmd.disable_help_subcommand(true);
    cmd.build();
    cmd
}

/// Whether two `*.1` filenames can name the same physical file.
///
/// The one definition of the equivalence relation both halves of the
/// man-page mechanism apply: `render_man_page`'s collision guard and
/// `sweep_orphans`' classification. They spelled it separately once,
/// disagreed, and the sweep deleted the page the guard had just
/// written (#1250) — sharing it makes that divergence unrepresentable.
///
/// Only ASCII case is folded, so a non-ASCII pair such as `café.1` /
/// `CAFÉ.1` is *not* matched — on APFS those name one file, making that
/// pair #1250 unfixed. What keeps the gap unreachable is that no clap
/// command in this workspace is non-ASCII. `render_man_page`'s
/// `debug_assert` is a tripwire for that fact rather than a guard on
/// it: it panics instead of returning an error, and is compiled out
/// under `--release` (every invocation today is a debug build).
/// Adopting a Unicode-folding comparison (e.g. `unicase`) is the
/// prerequisite for lifting the restriction.
fn names_same_file(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Delete `man/*.1` files that no command in `expected` accounts for.
///
/// A directory entry has *three* verdicts against `expected`, not two,
/// so sharing the write guard's relation is not simply a matter of
/// swapping in a case-insensitive comparison (#1250):
///
/// * byte-equal to an expected name — the page just written; keep.
/// * no [`names_same_file`] match — a renamed or removed command's
///   stale page; remove.
/// * a [`names_same_file`] match that is not byte-equal — a case-only
///   command rename; error, because neither other verdict is right.
///   *Removing* destroys the page `render_man_page` just wrote: on
///   APFS / NTFS the write for `BCA.1` lands in the existing `bca.1`
///   directory entry, since those filesystems are case-*preserving*
///   and opening a file under a different spelling does not rename it.
///   *Keeping* strands a stale page on ext4 / btrfs, where the two
///   files are distinct. Measured against
///   `utils/check-manpage-drift.py` (#1249): the untracked `BCA.1`
///   makes the gate red once and `git add man/` clears it, but the
///   rename never touches `bca.1`'s bytes, so that page is invisible
///   to both halves of the gate from the outset and simply stays
///   tracked — after which a case-insensitive sweep can never remove
///   it and it ships forever.
///
/// Erroring is the only verdict *derivable from the filenames alone*
/// that reads the same on every filesystem, which is what the guard's
/// comment says this crate normalises for, and it keeps the two sites'
/// *polarity* aligned: both reject a case-only collision rather than
/// one rejecting and the other silently retaining. The sweep could
/// instead ask the filesystem whether the two spellings are one file
/// (`st_dev` / `st_ino`, or `file_index` /
/// `volume_serial_number` on Windows) and resolve it with no human
/// step. That is two platform-gated code paths for an event that has
/// happened zero times; it is the escape hatch if case-only renames
/// ever become routine.
fn sweep_orphans(out_dir: &Path, expected: &[String]) -> io::Result<()> {
    // Load-bearing for the single `find` below, not merely for its
    // cost: if `expected` held both `bca.1` and `BCA.1`, `find` would
    // answer with whichever came first and the entry `bca.1` would be
    // classified by list order — kept or rejected. `render_man_page`
    // rules that out by refusing to push a second name that matches an
    // earlier one, so the candidate is unique when it exists.
    debug_assert!(
        !expected
            .iter()
            .enumerate()
            .any(|(i, a)| expected[..i].iter().any(|b| names_same_file(a, b))),
        "render_man_page must reject case-insensitively equal page names before they reach \
         the sweep; got {expected:?}",
    );
    // Classify every entry before unlinking any of it. `fs::read_dir`
    // order is unspecified, so removing as we go and bailing on the
    // first conflict would delete a different subset of the orphans on
    // each run, and would report only the first of several renames.
    // Two phases make "the error path removes nothing" a postcondition
    // a test can assert.
    let mut orphans = Vec::<PathBuf>::new();
    let mut conflicts = Vec::<String>::new();
    for entry in fs::read_dir(out_dir)? {
        let entry = entry?;
        let path = entry.path();
        // `file_type()` does not traverse symlinks, so a symlink whose
        // target is a directory still reports `is_symlink()` here and
        // falls through to `remove_file`, which unlinks the symlink
        // itself rather than touching the target.
        let file_type = entry.file_type()?;
        // Only sweep .1 files — leave any future README / .gitkeep
        // committed alongside untouched. Skip real directories so a
        // stray `foo.1/` doesn't error out the whole sweep.
        if file_type.is_dir() || path.extension().is_none_or(|e| e != "1") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // At most one candidate, per the assertion above, so
        // byte-equality alone decides between the keep and error arms.
        match expected.iter().find(|n| names_same_file(n, name)) {
            // The page just written.
            Some(want) if want == name => {}
            Some(want) => conflicts.push(format!(
                "  `{name}` differs from expected `{want}` only in ASCII case — `rm man/{name}`"
            )),
            None => orphans.push(path),
        }
    }

    if !conflicts.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "a case-only man-page rename cannot be swept safely — on a case-insensitive \
                 filesystem the stale spelling *is* the page just written, so neither keeping \
                 nor removing it is right everywhere. No orphan pages were removed. Delete the \
                 file(s) below and re-run `cargo xtask`:\n{}\n(plain `rm`, not `git rm`: on a \
                 case-insensitive filesystem the file now holds the freshly rendered content, \
                 and `git rm` refuses a path with local modifications)",
                conflicts.join("\n"),
            ),
        ));
    }

    for path in orphans {
        fs::remove_file(&path)?;
        println!("Removed orphan {}", path.display());
    }
    Ok(())
}

/// Render `root` and every subcommand under it.
///
/// Takes the root **by value** and builds it here rather than accepting
/// a `&Command` the caller prepared: rendering from an *unbuilt* root is
/// precisely the #1248 bug, and owning the build is what stops a future
/// caller from reintroducing it.
fn render_tree(root: clap::Command, out_dir: &Path, expected: &mut Vec<String>) -> io::Result<()> {
    let root = built(root);
    let version = root.get_version().unwrap_or("unknown").to_string();
    render_man_page(&root, &version, out_dir, expected)?;
    render_subcommands(&root, root.get_name(), &version, out_dir, expected)
}

fn render_subcommands(
    parent: &clap::Command,
    prefix: &str,
    version: &str,
    out_dir: &Path,
    expected: &mut Vec<String>,
) -> io::Result<()> {
    for sub in parent.get_subcommands() {
        let full_name = format!("{prefix}-{}", sub.get_name());
        render_subcommands(sub, &full_name, version, out_dir, expected)?;
        // Re-pin the bin name alongside the page name. Building the root
        // (see `built`) also runs clap's `_build_bin_names_internal`,
        // which stamps each subcommand with a space-separated bin name
        // (`bca metrics`) that `clap_mangen` renders into SYNOPSIS —
        // and `Man::new`'s own `build()` is a no-op on an already-built
        // command, so that bin name would survive the `name` rename and
        // churn every SYNOPSIS line away from the committed
        // `bca\-metrics` spelling. #1248 is about restoring missing
        // *options*; keeping both names in step holds that diff to pure
        // additions. (`display_name` needs no pin — clap derives it with
        // the same hyphen this rename applies.)
        // The clone is a real allocation — `bin_name` takes an owned
        // `String` and `Str::Inner::Owned` is a `Box<str>`, so neither
        // side is a cheap handle copy. One short string per page.
        let sub_cmd = sub.clone().bin_name(full_name.clone()).name(full_name);
        render_man_page(&sub_cmd, version, out_dir, expected)?;
    }
    Ok(())
}

fn render_man_page(
    cmd: &clap::Command,
    version: &str,
    out_dir: &Path,
    expected: &mut Vec<String>,
) -> io::Result<()> {
    let name = cmd.get_name().to_string();
    // The collision check below is `eq_ignore_ascii_case`, which only
    // folds case in the ASCII range. Every clap command in this
    // workspace is ASCII today, so this is sufficient — but if a
    // future contributor adds a non-ASCII command name (e.g.
    // `Café` vs `café`), the case-insensitive guard would silently
    // miss the collision on case-insensitive filesystems. Trip the
    // debug-build assertion instead of letting the latent gap reach
    // the filesystem.
    debug_assert!(
        name.is_ascii(),
        "non-ASCII command name `{name}` would defeat the ASCII-case-insensitive collision guard; \
         switch to a Unicode case-folding comparison (e.g. unicase) before adding non-ASCII names",
    );
    let man = clap_mangen::Man::new(cmd.clone())
        .title(name.to_uppercase())
        .section("1")
        .source(format!("big-code-analysis {version}"))
        .manual("big-code-analysis Manual".to_string());

    let mut buffer = Vec::<u8>::new();
    man.render(&mut buffer)?;
    let filename = format!("{name}.1");
    // Defensive: if a future top-level binary (e.g. `bca-web`) ever
    // collides with a `bca` subcommand name, or two recursion paths
    // produce the same `{prefix}-{sub}` filename, the second
    // `fs::write` would silently overwrite the first. Fail loudly
    // instead so the conflict surfaces in `cargo xtask` / CI.
    //
    // The comparison is ASCII-case-insensitive. clap command names
    // are ASCII (enforced by the `debug_assert` above), but the
    // resulting `*.1` files are written to the user's filesystem —
    // APFS (macOS default) and NTFS (Windows default) are
    // case-insensitive, so `Bca.1` and `bca.1` map to the same
    // physical file. A case-sensitive `contains` check would let one
    // entry pass the guard and silently overwrite the other on
    // case-insensitive filesystems while case-sensitive ext4/btrfs
    // see two distinct files. Normalising to ASCII case here makes
    // the gate behave identically on every developer workstation.
    //
    // The relation lives in `names_same_file` because `sweep_orphans`
    // must apply the same one; the two spelling it separately, and
    // diverging, is #1250.
    if expected.iter().any(|n| names_same_file(n, &filename)) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("duplicate man page filename `{filename}` for command `{name}`"),
        ));
    }
    fs::write(out_dir.join(&filename), buffer)?;
    expected.push(filename);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{render_man_page, sweep_orphans};
    use std::{fs, io};
    use tempfile::TempDir;

    fn touch(dir: &std::path::Path, name: &str) {
        fs::write(dir.join(name), b"").expect("write fixture file");
    }

    #[test]
    fn sweep_keeps_non_man_files() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "README.md");
        touch(tmp.path(), "foo.1");
        touch(tmp.path(), "bar.1");

        sweep_orphans(tmp.path(), &["foo.1".to_string()]).expect("sweep");

        assert!(
            tmp.path().join("README.md").exists(),
            "README.md must survive"
        );
        assert!(
            tmp.path().join("foo.1").exists(),
            "expected .1 must survive"
        );
        assert!(
            !tmp.path().join("bar.1").exists(),
            "orphan .1 must be removed"
        );
    }

    #[test]
    fn sweep_keeps_expected_pages() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "foo.1");

        sweep_orphans(tmp.path(), &["foo.1".to_string()]).expect("sweep");

        assert!(
            tmp.path().join("foo.1").exists(),
            "expected .1 must survive"
        );
    }

    #[test]
    fn sweep_removes_orphan_pages() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "foo.1");
        touch(tmp.path(), "bar.1");
        touch(tmp.path(), "baz.1");

        sweep_orphans(tmp.path(), &["foo.1".to_string()]).expect("sweep");

        assert!(
            tmp.path().join("foo.1").exists(),
            "expected .1 must survive"
        );
        assert!(
            !tmp.path().join("bar.1").exists(),
            "orphan bar.1 must be removed"
        );
        assert!(
            !tmp.path().join("baz.1").exists(),
            "orphan baz.1 must be removed"
        );
    }

    // A case-only command rename (`bca` -> `BCA`) as it presents on a
    // case-insensitive filesystem (APFS, NTFS): `render_man_page`
    // wrote `BCA.1`, the write landed in the pre-existing `bca.1`
    // directory entry, and that single entry is what the sweep sees.
    //
    // Both competing designs fail this: the case-*sensitive* original
    // deletes the file and returns `Ok`, and a plain
    // `eq_ignore_ascii_case` keep returns `Ok` without naming the
    // conflict.
    #[test]
    fn sweep_errors_on_case_only_rename_with_one_entry() {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("bca.1"), b"freshly rendered BCA page")
            .expect("write fixture file");

        let err = sweep_orphans(tmp.path(), &["BCA.1".to_string()])
            .expect_err("case-only rename must not be swept silently");

        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        let msg = err.to_string();
        assert!(
            msg.contains("bca.1") && msg.contains("BCA.1"),
            "error must name both spellings, got: {msg}"
        );
        assert!(
            tmp.path().join("bca.1").exists(),
            "the page just written must survive the sweep"
        );
        assert_eq!(
            fs::read(tmp.path().join("bca.1")).expect("read page"),
            b"freshly rendered BCA page",
            "surviving page contents must be untouched",
        );
    }

    // The same rename as it presents on a case-*sensitive* filesystem
    // (ext4, btrfs): `BCA.1` is a new file and the stale `bca.1`
    // remains. The verdict must be the error raised above — that
    // identical-on-every-filesystem property is the whole reason the
    // third arm exists, and it is what a plain `eq_ignore_ascii_case`
    // keep gives up (there the stale page survives, is committed by the
    // drift gate's own `git add man/` remedy, and is then permanently
    // unsweepable).
    //
    // On a case-insensitive filesystem the fixture collapses: the second
    // write lands in the first file's directory entry, so there is one
    // page holding the fresh bytes rather than two. The verdict and the
    // remedy are asserted in both worlds; only the claim about the stale
    // page's *contents* is specific to the two-file case, and it is
    // branched on below rather than assumed.
    #[test]
    fn sweep_errors_on_case_only_rename_with_both_entries() {
        let tmp = TempDir::new().expect("tempdir");
        let two_files = dir_is_case_sensitive(tmp.path());
        fs::write(tmp.path().join("bca.1"), b"stale lowercase page").expect("write stale page");
        fs::write(tmp.path().join("BCA.1"), b"freshly rendered BCA page")
            .expect("write fresh page");

        let err = sweep_orphans(tmp.path(), &["BCA.1".to_string()])
            .expect_err("case-only rename must not be swept silently");

        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        // The remedy must name the *stale* spelling. An implementation
        // that interpolated the expected name here would tell the
        // contributor to delete the page just generated, and every
        // `exists()` assertion below would still pass, so this string
        // is what catches the transposition. It holds on either
        // filesystem: the on-disk entry is spelled `bca.1` in both.
        assert!(
            err.to_string().contains("`rm man/bca.1`"),
            "remedy must name the stale spelling, got: {err}"
        );
        assert_eq!(
            fs::read(tmp.path().join("BCA.1")).expect("read expected page"),
            b"freshly rendered BCA page",
            "the expected page must survive with its contents intact",
        );

        if two_files {
            assert_eq!(
                fs::read(tmp.path().join("bca.1")).expect("read stale page"),
                b"stale lowercase page",
                "the stale page must be left for the contributor to remove, not silently deleted",
            );
        } else {
            // The hazard #1250 exists for, observed directly: one entry,
            // still spelled with the stale case, now holding the freshly
            // rendered bytes. Asserting the count is what makes this a
            // real check rather than a restatement of the assertion
            // above — the two paths resolve to the same file here.
            let pages = man_page_names(tmp.path());
            assert_eq!(
                pages,
                vec!["bca.1".to_string()],
                "a case-insensitive filesystem must leave exactly the stale entry",
            );
        }
    }

    /// Whether `dir` resolves filenames case-sensitively.
    ///
    /// Probed rather than inferred from the target OS. `cfg!(unix)` is
    /// true on macOS, whose default APFS is case-*insensitive*, and a
    /// Linux checkout can sit on a case-insensitive mount — so a `cfg`
    /// gate answers a different question than the one that decides
    /// whether two spellings are two files.
    fn dir_is_case_sensitive(dir: &std::path::Path) -> bool {
        let probe = dir.join("zz-case-probe.tmp");
        fs::write(&probe, b"probe").expect("write case probe");
        let distinct = !dir.join("ZZ-CASE-PROBE.TMP").exists();
        fs::remove_file(&probe).expect("remove case probe");
        distinct
    }

    /// The `.1` entries in `dir`, sorted, as the filesystem spells them.
    fn man_page_names(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("read_dir")
            .map(|e| e.expect("dir entry").file_name())
            .filter_map(|n| n.into_string().ok())
            .filter(|n| n.ends_with(".1"))
            .collect();
        names.sort();
        names
    }

    // The error path is a refusal, not a partial sweep: `fs::read_dir`
    // order is unspecified, so a sweep that unlinked as it classified
    // would remove a different subset of the genuine orphans on each
    // run before bailing. `zz-orphan.1` here is a true orphan that the
    // same call would delete were it not for the conflict.
    #[test]
    fn sweep_removes_nothing_when_a_case_only_rename_is_detected() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "bca.1");
        touch(tmp.path(), "zz-orphan.1");

        sweep_orphans(tmp.path(), &["BCA.1".to_string()])
            .expect_err("case-only rename must not be swept silently");

        assert!(
            tmp.path().join("zz-orphan.1").exists(),
            "the error path must not have unlinked a genuine orphan"
        );
    }

    // Classifying before unlinking also means every conflict is known
    // by the time the error is built, so a contributor renaming two
    // commands at once fixes both in one pass instead of rediscovering
    // the second on the next run. Order is `fs::read_dir` order and so
    // unspecified — assert presence, never position.
    #[test]
    fn sweep_error_names_every_case_only_rename() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "bca.1");
        touch(tmp.path(), "bca-web.1");

        let err = sweep_orphans(tmp.path(), &["BCA.1".to_string(), "BCA-WEB.1".to_string()])
            .expect_err("case-only renames must not be swept silently");

        let msg = err.to_string();
        for stale in ["bca.1", "bca-web.1"] {
            assert!(
                msg.contains(&format!("`rm man/{stale}`")),
                "error must name every conflict; `{stale}` missing from: {msg}"
            );
        }
    }

    // The shared relation is `eq_ignore_ascii_case`, which folds only
    // ASCII — so a non-ASCII case difference is *not* a match and the
    // entry is swept as an ordinary orphan. That limitation is the one
    // `render_man_page`'s `debug_assert` exists to keep unreachable
    // (no clap command in this workspace is non-ASCII). Pinning it here
    // makes a future switch to Unicode case folding a deliberate change
    // rather than a silent one, and discriminates the ASCII relation
    // from a full-folding `to_lowercase()` comparison, which would
    // instead error.
    #[test]
    fn sweep_treats_non_ascii_case_difference_as_an_orphan() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "café.1");

        sweep_orphans(tmp.path(), &["CAFÉ.1".to_string()])
            .expect("non-ASCII case difference is not a case-insensitive match");

        assert!(
            !tmp.path().join("café.1").exists(),
            "eq_ignore_ascii_case does not fold `é`, so the entry is an orphan"
        );
    }

    #[test]
    fn sweep_keeps_subdirectory() {
        let tmp = TempDir::new().expect("tempdir");
        let subdir = tmp.path().join("foo.1.dir");
        fs::create_dir(&subdir).expect("mkdir subdir");
        let bare_dir = tmp.path().join("bar.1");
        fs::create_dir(&bare_dir).expect("mkdir bar.1");

        sweep_orphans(tmp.path(), &[]).expect("sweep must skip directories");

        assert!(subdir.exists(), "non-matching subdirectory must survive");
        assert!(
            bare_dir.exists(),
            "directory with .1 extension must be skipped"
        );
    }

    #[test]
    fn render_man_page_rejects_duplicate_filename() {
        let tmp = TempDir::new().expect("tempdir");
        let mut expected = Vec::<String>::new();

        // First write succeeds and seeds `expected` with `bca-web.1`.
        let first = clap::Command::new("bca-web").version("0.1.0");
        render_man_page(&first, "0.1.0", tmp.path(), &mut expected)
            .expect("first render must succeed");
        assert_eq!(expected, vec!["bca-web.1".to_string()]);

        // Second command rendered under the same filename — simulates a
        // future `bca web` subcommand colliding with the `bca-web`
        // top-level binary, or any other prefix collision.
        let second = clap::Command::new("bca-web").version("0.1.0");
        let err = render_man_page(&second, "0.1.0", tmp.path(), &mut expected)
            .expect_err("second render must fail on filename collision");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(
            err.to_string().contains("bca-web.1"),
            "error must name the colliding filename, got: {err}"
        );
        // `expected` must not gain a duplicate entry on the error path —
        // otherwise `sweep_orphans` would skip the legitimate page on a
        // later run that no longer produces the collision.
        assert_eq!(expected, vec!["bca-web.1".to_string()]);
    }

    // ASCII case-insensitive guard for case-insensitive filesystems
    // (APFS on macOS, NTFS on Windows). The collision detector must
    // catch `Bca` vs `bca` even though Rust's `String == String`
    // would see them as distinct.
    #[test]
    fn render_man_page_rejects_case_only_filename_collision() {
        let tmp = TempDir::new().expect("tempdir");
        let mut expected = Vec::<String>::new();

        // First write: lowercase `bca.1`.
        let first = clap::Command::new("bca").version("0.1.0");
        render_man_page(&first, "0.1.0", tmp.path(), &mut expected)
            .expect("first render must succeed");
        assert_eq!(expected, vec!["bca.1".to_string()]);

        // Second command differs only in case. On APFS / NTFS this
        // would write to the same physical file; on ext4 / btrfs the
        // two files are distinct but the man-page output set would
        // still mislead users running `man bca` on a case-insensitive
        // filesystem. Fail loudly on every developer workstation.
        let second = clap::Command::new("BCA").version("0.1.0");
        let err = render_man_page(&second, "0.1.0", tmp.path(), &mut expected)
            .expect_err("second render must fail on case-only collision");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(
            err.to_string().contains("BCA.1"),
            "error must name the colliding filename, got: {err}"
        );
        // `expected` must not gain the BCA.1 entry on the error path.
        assert_eq!(expected, vec!["bca.1".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn sweep_unlinks_symlink_not_target() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new().expect("tempdir");
        let outside_dir = TempDir::new().expect("outside tempdir");
        let outside_target = outside_dir.path().join("outside.txt");
        fs::write(&outside_target, b"keep me").expect("write outside target");

        let link = tmp.path().join("bar.1");
        symlink(&outside_target, &link).expect("symlink");

        sweep_orphans(tmp.path(), &[]).expect("sweep");

        assert!(!link.exists(), "orphan symlink must be unlinked");
        // `Path::exists()` follows symlinks, but we already asserted
        // the link is gone — read the target directly to confirm.
        assert!(
            outside_target.exists(),
            "symlink target must not be touched"
        );
        assert_eq!(
            fs::read(&outside_target).expect("read target"),
            b"keep me",
            "symlink target contents must be intact",
        );
    }
}

/// Global-arg propagation into subcommand pages (#1248).
///
/// These render the *real* `bca` tree rather than a fixture, because the
/// bug was that a subcommand plucked from an unbuilt parent silently
/// loses its inherited options — a shape only a live parent/child pair
/// exhibits. The synthetic case at the end isolates the same contract
/// from the CLI's own churn, so a flag rename cannot leave the
/// propagation itself unguarded.
#[cfg(test)]
mod global_arg_propagation {
    use super::{built, render_tree};
    use clap::{Arg, ArgAction, Command, CommandFactory};
    use std::{fs, path::Path};
    use tempfile::TempDir;

    /// Render `root`'s whole tree into `dir` and return the page `page`.
    fn render_page(root: Command, dir: &Path, page: &str) -> String {
        let mut expected = Vec::<String>::new();
        render_tree(root, dir, &mut expected).expect("render tree");
        assert!(
            expected.iter().any(|n| n == page),
            "`{page}` must be among the rendered pages; got {expected:?}",
        );
        fs::read_to_string(dir.join(page)).expect("read page")
    }

    /// The OPTIONS section of `page`, i.e. everything after `.SH OPTIONS`.
    ///
    /// Slicing here rather than searching the whole page is what makes
    /// the assertions below discriminating. `bca-vcs-commit.1`'s
    /// DESCRIPTION already quotes `\-\-ref` in prose, so a page-wide
    /// `contains` would pass for an option that never got an entry.
    fn options_section(page: &str) -> &str {
        page.split_once("\n.SH OPTIONS\n")
            .expect("page must have an OPTIONS section")
            .1
    }

    /// How man(7) spells `flag` where it heads its own OPTIONS entry:
    /// bold, with every literal hyphen escaped as `\-`.
    fn option_entry(flag: &str) -> String {
        format!(r"\fB{}\fR", flag.replace('-', r"\-"))
    }

    fn assert_documents(page_name: &str, page: &str, flags: &[&str]) {
        let options = options_section(page);
        for flag in flags {
            let entry = option_entry(flag);
            assert!(
                options.contains(&entry),
                "`{page_name}` must document the inherited global `{flag}` \
                 (looked for `{entry}` in its OPTIONS section)",
            );
        }
    }

    fn cli_page(dir: &Path, page: &str) -> String {
        render_page(big_code_analysis_cli::Cli::command(), dir, page)
    }

    // The two `UniversalArgs` flags (`cli_args/mod.rs`) are `global =
    // true`, so `bca metrics --help` prints them and the page must too.
    #[test]
    fn top_level_globals_reach_a_subcommand_page() {
        let tmp = TempDir::new().expect("tempdir");
        let page = cli_page(tmp.path(), "bca-metrics.1");
        assert_documents("bca-metrics.1", &page, &["--warnings", "--report-skipped"]);
    }

    // One level deeper: `vcs`' own history-tuning globals
    // (`cli_args/vcs.rs`) must reach `vcs commit`, which is reachable
    // only because `Command::build()` recurses.
    #[test]
    fn group_level_globals_reach_a_nested_subcommand_page() {
        let tmp = TempDir::new().expect("tempdir");
        let page = cli_page(tmp.path(), "bca-vcs-commit.1");
        assert_documents(
            "bca-vcs-commit.1",
            &page,
            &["--long-window", "--recent-window"],
        );
    }

    // Building the root also stamps subcommands with a space-separated
    // bin name (`bca metrics`), which clap_mangen renders into SYNOPSIS.
    // `render_subcommands` re-pins it so the fix adds options without
    // renaming every page's synopsis; without that pin this fails.
    #[test]
    fn synopsis_keeps_the_hyphenated_page_name() {
        let tmp = TempDir::new().expect("tempdir");
        // Both depths, because clap derives the bin name from the whole
        // command path: `commit` is stamped `bca vcs commit`, so a pin
        // applied only to top-level subcommands would still churn the
        // nested pages.
        for (page_name, want) in [
            ("bca-metrics.1", r"\fBbca\-metrics\fR "),
            ("bca-vcs-commit.1", r"\fBbca\-vcs\-commit\fR "),
        ] {
            let page = cli_page(tmp.path(), page_name);
            let synopsis = page
                .split_once("\n.SH SYNOPSIS\n")
                .expect("page must have a SYNOPSIS section")
                .1
                .lines()
                .next()
                .expect("SYNOPSIS must have a line");
            assert!(
                synopsis.starts_with(want),
                "`{page_name}` SYNOPSIS must open with `{want}`, got: {synopsis}"
            );
        }
    }

    // Hidden args stay hidden through the rebuild: `--headroom` is
    // `hide = true` on `check` and must not surface now that the page is
    // rendered from a built command.
    #[test]
    fn building_does_not_surface_hidden_args() {
        let tmp = TempDir::new().expect("tempdir");
        // Assert the subject still exists and is still hidden. Without
        // this the negative below passes just as well once `--headroom`
        // is deleted — and its help text schedules exactly that ("pass
        // `--tier=soft=<RATIO>` instead"), so the test would quietly
        // become a tautology while staying green.
        let root = built(big_code_analysis_cli::Cli::command());
        let check = root
            .find_subcommand("check")
            .expect("the CLI must have a `check` subcommand");
        assert!(
            check
                .get_arguments()
                .any(|a| a.get_long() == Some("headroom") && a.is_hide_set()),
            "`check` must still carry a hidden `--headroom` for this test to mean anything",
        );

        let page = cli_page(tmp.path(), "bca-check.1");
        assert!(
            !page.contains(&option_entry("--headroom")),
            "`--headroom` is `hide = true` and must stay out of the page"
        );
        // Guards against a silently-empty page: the globals this fix
        // adds prove the same page really was rendered from a built
        // command.
        assert_documents("bca-check.1", &page, &["--warnings"]);
    }

    // A plain `build()` inserts clap's `help` subcommand at every level
    // that has subcommands; `built` suppresses it so neither a page nor
    // a SUBCOMMANDS cross-reference to one is produced. Both halves are
    // asserted, because the suppression is only observable against a
    // baseline that shows `help` would otherwise be there.
    #[test]
    fn built_suppresses_the_auto_inserted_help_subcommand() {
        let mut baseline = big_code_analysis_cli::Cli::command();
        baseline.build();
        assert!(
            baseline.get_subcommands().any(|s| s.get_name() == "help"),
            "a plain `build()` must insert clap's `help` subcommand, \
             otherwise this test cannot observe the suppression",
        );
        let vcs = baseline
            .get_subcommands()
            .find(|s| s.get_name() == "vcs")
            .expect("the CLI has a `vcs` subcommand group");
        assert!(
            vcs.get_subcommands().any(|s| s.get_name() == "help"),
            "the nested group must get one too, otherwise the global \
             setting's propagation is untested",
        );

        let root = built(big_code_analysis_cli::Cli::command());
        assert!(
            !root.get_subcommands().any(|s| s.get_name() == "help"),
            "`built` must suppress the root's `help` subcommand",
        );
        let vcs = root
            .get_subcommands()
            .find(|s| s.get_name() == "vcs")
            .expect("the CLI has a `vcs` subcommand group");
        assert!(
            !vcs.get_subcommands().any(|s| s.get_name() == "help"),
            "the suppression is a global setting and must reach nested \
             groups — `bca-vcs.1` referenced `bca-vcs-help(1)` when it \
             did not",
        );
    }

    // Every `name(1)` a page cross-references must be a page the same
    // run wrote. `man/bca.1` shipped `bca\-help(1)` and `man/bca-vcs.1`
    // shipped `bca\-vcs\-help(1)` for exactly as long as the renderer
    // listed a subcommand it declined to write, and no test could see
    // it: both halves were individually reasonable.
    #[test]
    fn no_page_cross_references_a_page_that_is_not_written() {
        let tmp = TempDir::new().expect("tempdir");
        let mut expected = Vec::<String>::new();
        render_tree(
            big_code_analysis_cli::Cli::command(),
            tmp.path(),
            &mut expected,
        )
        .expect("render tree");

        let mut refs = 0_usize;
        for name in &expected {
            let page = fs::read_to_string(tmp.path().join(name)).expect("read page");
            for line in page.lines() {
                // A cross-reference is a whole line of its own, in the
                // `.TP` entry above each subcommand's blurb. roff
                // escapes the hyphens, so `bca\-vcs(1)` is the page
                // `bca-vcs.1`.
                let Some(target) = line.strip_suffix("(1)") else {
                    continue;
                };
                let target = target.replace("\\-", "-");
                if !target
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                {
                    continue;
                }
                refs += 1;
                let want = format!("{target}.1");
                assert!(
                    expected.contains(&want),
                    "`{name}` cross-references `{target}(1)`, which no page satisfies; \
                     written pages: {expected:?}",
                );
            }
        }
        // A selector that matches nothing makes the loop above
        // vacuously true — the CLI's ~18 subcommands are all listed in
        // `bca.1`, so a single-digit count means the parse broke.
        assert!(
            refs > 15,
            "expected the SUBCOMMANDS sections to yield many cross-references, found {refs}",
        );
    }

    // The same contract, isolated from the live CLI: a global on a
    // synthetic parent must appear in its subcommand's page. This keeps
    // the propagation guarded even if `--warnings` / `--long-window` are
    // renamed or lose their `global = true` some day.
    #[test]
    fn a_synthetic_parents_global_reaches_its_subcommand_page() {
        let tmp = TempDir::new().expect("tempdir");
        let root = Command::new("root").version("0.0.0").arg(
            Arg::new("g")
                .long("global-flag")
                .action(ArgAction::SetTrue)
                .global(true),
        );
        let page = render_page(
            root.subcommand(Command::new("sub")),
            tmp.path(),
            "root-sub.1",
        );
        assert_documents("root-sub.1", &page, &["--global-flag"]);
    }

    // The negative control for the test above: an arg that is *not*
    // global must stay on the parent's page only. Without this, the
    // propagation assertions would also pass for an implementation that
    // copied every parent arg down.
    #[test]
    fn a_non_global_parent_arg_stays_off_the_subcommand_page() {
        let tmp = TempDir::new().expect("tempdir");
        let root = Command::new("root")
            .version("0.0.0")
            .arg(Arg::new("l").long("local-flag").action(ArgAction::SetTrue))
            .subcommand(Command::new("sub"));
        let mut expected = Vec::<String>::new();
        render_tree(root, tmp.path(), &mut expected).expect("render tree");

        let parent = fs::read_to_string(tmp.path().join("root.1")).expect("read parent page");
        assert!(
            options_section(&parent).contains(&option_entry("--local-flag")),
            "the parent's own page must document its non-global arg"
        );
        let sub = fs::read_to_string(tmp.path().join("root-sub.1")).expect("read sub page");
        assert!(
            !sub.contains(&option_entry("--local-flag")),
            "a non-global parent arg must not reach the subcommand page"
        );
    }
}
