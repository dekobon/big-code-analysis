//! Workspace task runner.
//!
//! `cargo xtask` (no args) regenerates the man pages for `bca` and
//! `bca-web` under `man/` at the repo root, one `.1` per top-level
//! binary plus one per `bca` subcommand. CI gates a `git diff
//! --exit-code -- man/` against the output, so adding a flag without
//! re-running `cargo xtask` fails the manpage job.
#![allow(missing_docs)]
#![allow(clippy::pedantic)]

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
        &big_code_analysis_cli::Cli::command(),
        &out_dir,
        &mut expected,
    )?;
    // `bca-web` has no subcommands; the recursion is a no-op for it.
    render_tree(
        &big_code_analysis_web::cli::Opts::command(),
        &out_dir,
        &mut expected,
    )?;

    // Sweep orphan `.1` files (renamed/removed subcommands) so the CI
    // `git diff --exit-code -- man/` gate flips red on stale pages
    // instead of silently shipping them.
    sweep_orphans(&out_dir, &expected)?;

    println!("Wrote man pages to {}", out_dir.display());
    Ok(())
}

fn sweep_orphans(out_dir: &Path, expected: &[String]) -> io::Result<()> {
    for entry in fs::read_dir(out_dir)? {
        let entry = entry?;
        let path = entry.path();
        // Only sweep .1 files — leave any future README / .gitkeep
        // committed alongside untouched.
        if path.extension().is_some_and(|e| e == "1")
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
            && !expected.iter().any(|n| n == name)
        {
            fs::remove_file(&path)?;
            println!("Removed orphan {}", path.display());
        }
    }
    Ok(())
}

fn render_tree(cmd: &clap::Command, out_dir: &Path, expected: &mut Vec<String>) -> io::Result<()> {
    let version = cmd.get_version().unwrap_or("unknown").to_string();
    render_man_page(cmd, &version, out_dir, expected)?;
    render_subcommands(cmd, cmd.get_name(), &version, out_dir, expected)
}

fn render_subcommands(
    parent: &clap::Command,
    prefix: &str,
    version: &str,
    out_dir: &Path,
    expected: &mut Vec<String>,
) -> io::Result<()> {
    for sub in parent.get_subcommands() {
        if sub.get_name() == "help" {
            continue;
        }
        let full_name = format!("{prefix}-{}", sub.get_name());
        // Recurse first so we can hand ownership of `full_name` to clap
        // on the last line — avoids cloning it for the recursion.
        render_subcommands(sub, &full_name, version, out_dir, expected)?;
        let sub_cmd = sub.clone().name(full_name);
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
    let man = clap_mangen::Man::new(cmd.clone())
        .title(name.to_uppercase())
        .section("1")
        .source(format!("big-code-analysis {version}"))
        .manual("big-code-analysis Manual".to_string());

    let mut buffer = Vec::<u8>::new();
    man.render(&mut buffer)?;
    let filename = format!("{name}.1");
    fs::write(out_dir.join(&filename), buffer)?;
    expected.push(filename);
    Ok(())
}
