//! `bca check` stderr remediation block (next-steps, baseline-refresh command).

use super::super::*;
use super::*;

pub(crate) fn format_remediation_block(globals: &GlobalOpts, args: &CheckArgs) -> Option<String> {
    use std::fmt::Write as _;
    if args.no_remediation {
        return None;
    }
    let mut out = String::from("\n--- next steps ---\n");
    let _ = writeln!(out, "* Detailed reports: {}", artifact_link());
    let _ = writeln!(
        out,
        "* To refresh baseline: {}",
        refresh_baseline_command(globals, args)
    );
    // The refresh command mirrors path filters (`--paths`,
    // `--exclude`, `--exclude-from`, `--config`, `--baseline`) but
    // intentionally omits selectors that don't affect baseline
    // composition (`--num-jobs`) and ones that would bloat the
    // common-case command (`--include`, `--language`,
    // `--paths-from`, `--exclude-tests`). Surface the omission so a
    // user with a non-trivial scope re-adds them rather than
    // assuming the printed command is complete.
    out.push_str(
        "  (mirrors path filters only — re-add any --include / --language / --exclude-tests / --paths-from flags as needed)\n",
    );
    out.push_str(
        "* Adoption guide: https://dekobon.github.io/big-code-analysis/recipes/baselines.html\n",
    );
    Some(out)
}

pub(crate) fn artifact_link() -> String {
    artifact_link_for(
        std::env::var(check_format::GITHUB_REPOSITORY_ENV).ok(),
        std::env::var(check_format::GITHUB_RUN_ID_ENV).ok(),
    )
}

/// Pure inner: render the detailed-reports bullet given explicit env
/// values (rather than reading them from the process environment).
/// Extracted so tests can pin both the SOME and NONE branches without
/// depending on whether the test process happens to have GHA env
/// vars set. Empty strings are treated as absent — GitHub Actions
/// does set these vars but the spec doesn't promise non-empty values
/// on every event type.
///
/// Only when both env vars are present (i.e. running inside a GitHub
/// Actions job) do we point at the uploaded `bca-reports` artifact;
/// outside CI there is no run and no artifact, so claiming one sends
/// developers hunting for an upload that does not exist (#676). The
/// local fallback suggests `bca report` for the detailed view.
pub(crate) fn artifact_link_for(repo: Option<String>, run_id: Option<String>) -> String {
    let repo = repo.filter(|s| !s.is_empty());
    let run_id = run_id.filter(|s| !s.is_empty());
    match (repo, run_id) {
        (Some(repo), Some(run_id)) => {
            format!("bca-reports artifact at https://github.com/{repo}/actions/runs/{run_id}")
        }
        _ => "run `bca report` to see them locally".to_string(),
    }
}

pub(crate) fn refresh_baseline_command(globals: &GlobalOpts, args: &CheckArgs) -> String {
    let mut cmd = String::from("bca");
    let paths: Vec<&Path> = if globals.paths.is_empty() {
        // Default-when-absent — mirror the walker's `expand_seed_paths`
        // `.` fallback (#596) so the printed refresh command behaves
        // identically to the pathless invocation that produced it.
        vec![Path::new(".")]
    } else {
        globals.paths.iter().map(PathBuf::as_path).collect()
    };
    for p in &paths {
        cmd.push_str(" --paths ");
        cmd.push_str(&shell_quote_path(p));
    }
    for ex in &globals.exclude {
        cmd.push_str(" --exclude ");
        cmd.push_str(&shell_quote(ex));
    }
    if let Some(p) = &globals.exclude_from {
        cmd.push_str(" --exclude-from ");
        cmd.push_str(&shell_quote_path(p));
    }
    cmd.push_str(" check");
    if let Some(p) = &args.config {
        cmd.push_str(" --config ");
        cmd.push_str(&shell_quote_path(p));
    }
    // `--baseline` and `--write-baseline` conflict in clap, so we
    // prefer the user's baseline path if they ran with it (the
    // refresh writes back to the same file). Fall back to the
    // documented default `.bca-baseline.toml`.
    cmd.push_str(" --write-baseline ");
    match args.baseline.as_deref() {
        Some(p) => cmd.push_str(&shell_quote_path(p)),
        None => cmd.push_str(&shell_quote(".bca-baseline.toml")),
    }
    cmd
}

fn shell_quote_path(p: &Path) -> String {
    // The printed command is an *identifier* in the user's shell —
    // running it must reach the same file `bca` walked. Non-UTF-8
    // paths cannot be expressed as a shell argument verbatim, so
    // surface them as a clearly-broken placeholder rather than emit
    // a `to_string_lossy` form that silently points at the wrong
    // file (AGENTS.md: identifier paths use `to_str()` with explicit
    // non-UTF-8 handling, not `path.display()`).
    //
    // The placeholder contains `<`, `>` and spaces, which force
    // `shell_quote`'s slow path → single-quoted literal. Combining
    // the to_str + quote here (instead of leaving callers to chain
    // them) makes the discipline structural: a future caller can't
    // accidentally `eprintln!("{}", path_for_shell(p))` and emit an
    // unquoted `<non-UTF-8 path: …>` that bash would parse as input
    // redirection.
    let raw = p.to_str().map_or_else(
        || format!("<non-UTF-8 path: {}>", p.display()),
        str::to_string,
    );
    shell_quote(&raw)
}

/// Shell-quote `s` for inclusion in the remediation block's
/// copy-paste command. Uses single-quoting for simplicity: every
/// character is literal inside `'...'` except `'` itself, which we
/// escape via `'\''`. ASCII-safe and POSIX-compatible.
///
/// **POSIX-only**: This quoting is correct for bash / zsh / dash /
/// sh, which is what GitHub Actions runs every step in. It is NOT
/// safe for `cmd.exe` or `PowerShell` — a Windows user copy-pasting
/// the refresh command from a Windows CI log would need to
/// re-escape. The remediation block is a GHA/POSIX-CI feature by
/// design; Windows-host CI is out of scope.
fn shell_quote(s: &str) -> String {
    // Fast path: identifiers / paths without metacharacters need no
    // quoting at all. Keeping them unquoted makes the copy-paste
    // command read naturally for the common case.
    if !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '=' | ',' | '@')
        })
    {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}
