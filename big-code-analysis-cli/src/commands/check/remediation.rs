//! `bca check` stderr remediation block (next-steps, baseline-refresh command).

use super::*;

pub(crate) fn format_remediation_block(
    globals: &GlobalOpts,
    args: &CheckArgs,
    tier: TierSpec,
) -> Option<String> {
    use std::fmt::Write as _;
    if args.no_remediation {
        return None;
    }
    let mut out = String::from("\n--- next steps ---\n");
    let _ = writeln!(out, "* Detailed reports: {}", artifact_link());
    let _ = writeln!(
        out,
        "* To refresh baseline: {}",
        refresh_baseline_command(globals, args, tier)
    );
    // The command now mirrors every flag that changes what
    // `--write-baseline` records (#1243), so the enumerated
    // "re-add any --include / --language / …" caveat this line used
    // to carry is gone — each of those flags is emitted. What no
    // printed command can reproduce is a list that came from stdin,
    // so that one caveat stays, conditional on it applying.
    if any_list_flag_read_stdin(globals, args) {
        out.push_str(
            "  (a `-` list flag read stdin — point it at the file those patterns came from)\n",
        );
    }
    out.push_str(
        "* Adoption guide: https://dekobon.github.io/big-code-analysis/recipes/baselines.html\n",
    );
    Some(out)
}

/// Whether any of the three list-from-file flags the refresh command
/// mirrors was given as `-` (stdin). Every other flag re-runs
/// identically; a stdin list cannot, because the pipe that fed the
/// gate is gone by the time anyone reads the log.
fn any_list_flag_read_stdin(globals: &GlobalOpts, args: &CheckArgs) -> bool {
    [
        globals.paths_from.as_deref(),
        globals.exclude_from.as_deref(),
        args.check_exclude_from.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|p| p == Path::new("-"))
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

/// The copy-paste baseline-refresh invocation, rendered for a POSIX
/// shell. Every element of [`refresh_baseline_argv`] is quoted, so a
/// value containing a space, a glob metacharacter, or a quote survives
/// the paste as one argument.
pub(crate) fn refresh_baseline_command(
    globals: &GlobalOpts,
    args: &CheckArgs,
    tier: TierSpec,
) -> String {
    refresh_baseline_argv(globals, args, tier)
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The baseline-refresh invocation as argv, `bca` at index 0 and the
/// `check` subcommand at index 1.
///
/// **The subcommand comes before the flags.** `--paths` / `--exclude` /
/// `--exclude-from` have been subcommand-scoped since #597; the
/// pre-#597 `bca <walk flags> check` shape this used to emit is a clap
/// usage error, so every failing gate printed a next-steps command that
/// exited 1 when run (#1243).
///
/// Built as argv rather than as a string so the flag list and its shell
/// rendering are separable: the tests feed this straight to
/// `Cli::try_parse_from` and compare the parsed flags against the run
/// that produced them. A string-only builder can only be *string*-
/// matched, which is how the broken order survived eight assertions.
///
/// # What is mirrored, and why
///
/// The criterion is baseline *composition*: a flag that changes which
/// violations `--write-baseline` records must be echoed, or the
/// suggested refresh writes a different baseline than the gate
/// measured. That covers the walk scope (which files), the walker
/// tuning (what the metrics come out as), and the gate's own inputs
/// (which offenders survive to the writer) — `apply_check_exclude`
/// runs before `write_check_baseline` (#378), so `--check-exclude`
/// belongs here as much as `--exclude` does, and a `--threshold`
/// override decides the offender set outright.
///
/// Deliberately omitted: `--jobs` (throughput only); the reporting and
/// exit-code flags (`--report-format`, `--output`, `--no-fail`,
/// `--exit-codes`, `--summary-file`, `--github-annotations`,
/// `--no-summary`, `--no-remediation`), none of which reach the
/// writer; `--baseline-line-tolerance`, which is read-side matching;
/// and `--since` / `--changed-only`, which clap forbids alongside
/// `--write-baseline` on purpose — a diff-scoped baseline would look
/// like a complete snapshot to the next run.
///
/// Manifest-supplied settings need no echoing: the refresh runs from
/// the same directory and rediscovers the same `bca.toml`. The one
/// exception is `--no-config`, which is mirrored precisely so the
/// refresh keeps ignoring it.
pub(crate) fn refresh_baseline_argv(
    globals: &GlobalOpts,
    args: &CheckArgs,
    tier: TierSpec,
) -> Vec<String> {
    let mut argv = vec!["bca".to_owned(), "check".to_owned()];
    push_walk_args(&mut argv, globals);
    push_gate_args(&mut argv, args, tier);
    argv
}

/// Mirror the walk: which files are visited and how their metrics are
/// computed. Sourced from [`GlobalOpts`], which by this point carries
/// the CLI flags with any `bca.toml` values already merged in.
fn push_walk_args(argv: &mut Vec<String>, globals: &GlobalOpts) {
    if globals.paths.is_empty() {
        // Default-when-absent — mirror the walker's `expand_seed_paths`
        // `.` fallback (#596) so the printed refresh command behaves
        // identically to the pathless invocation that produced it.
        push_pair(argv, "--paths", ".");
    }
    for p in &globals.paths {
        push_pair(argv, "--paths", &path_arg(p));
    }
    push_repeated(argv, "--include", &globals.include);
    push_repeated(argv, "--exclude", &globals.exclude);
    push_path(argv, "--paths-from", globals.paths_from.as_deref());
    push_path(argv, "--exclude-from", globals.exclude_from.as_deref());
    push_value(argv, "--language", globals.language.as_deref());
    push_path(argv, "--preproc-data", globals.preproc_data.as_deref());
    push_switch(argv, "--no-skip-generated", globals.no_skip_generated);
    push_switch(argv, "--no-ignore", globals.no_ignore);
    push_switch(argv, "--no-config", globals.no_config);
    push_switch(argv, "--exclude-tests", globals.exclude_tests);
    push_equals(argv, "--cyclomatic-count-try", globals.count_cyclomatic_try);
}

/// Mirror the gate: which of the walk's violations reach the baseline
/// writer, and where the file lands. `tier` is the *resolved* tier —
/// `args.tier` alone loses a deprecated `--headroom <R>`, and
/// re-resolving here would emit that flag's deprecation warning a
/// second time.
fn push_gate_args(argv: &mut Vec<String>, args: &CheckArgs, tier: TierSpec) {
    push_path(argv, "--config", args.config.as_deref());
    for (metric, limit) in &args.thresholds {
        push_pair(argv, "--threshold", &format!("{metric}={limit}"));
    }
    push_equals(argv, "--tier", tier_value(tier));
    push_switch(argv, "--no-suppress", args.no_suppress);
    push_repeated(argv, "--check-exclude", &args.check_exclude);
    push_path(
        argv,
        "--check-exclude-from",
        args.check_exclude_from.as_deref(),
    );
    // Not just a read-side matcher: a fuzzy write populates each
    // entry's `body_hash`, so the flag changes the file's content.
    push_equals(argv, "--baseline-fuzzy-match", args.baseline_fuzzy_match);
    // `--baseline` and `--write-baseline` conflict in clap, so we
    // prefer the user's baseline path if they ran with it (the
    // refresh writes back to the same file). Fall back to the
    // documented default `.bca-baseline.toml`.
    let target = args
        .baseline
        .as_deref()
        .map_or_else(|| DEFAULT_BASELINE_FILE.to_owned(), path_arg);
    push_pair(argv, "--write-baseline", &target);
}

/// The `--tier` value for the resolved tier, or `None` at the `hard`
/// default, which needs no flag at all.
fn tier_value(tier: TierSpec) -> Option<String> {
    match tier {
        TierSpec::Hard => None,
        TierSpec::Soft(None) => Some("soft".to_owned()),
        TierSpec::Soft(Some(ratio)) => Some(format!("soft={ratio}")),
    }
}

fn push_pair(argv: &mut Vec<String>, flag: &str, value: &str) {
    argv.push(flag.to_owned());
    argv.push(value.to_owned());
}

fn push_repeated(argv: &mut Vec<String>, flag: &str, values: &[String]) {
    for value in values {
        push_pair(argv, flag, value);
    }
}

fn push_value(argv: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value {
        push_pair(argv, flag, value);
    }
}

fn push_path(argv: &mut Vec<String>, flag: &str, value: Option<&Path>) {
    if let Some(value) = value {
        push_pair(argv, flag, &path_arg(value));
    }
}

fn push_switch(argv: &mut Vec<String>, flag: &str, enabled: bool) {
    if enabled {
        argv.push(flag.to_owned());
    }
}

/// Push a flag whose clap definition sets `require_equals`, so name and
/// value must arrive as one argv element (`--tier=soft=0.9`). Splitting
/// them across two elements is a usage error, which is exactly the
/// class of mistake the argv round-trip test exists to catch.
fn push_equals(argv: &mut Vec<String>, flag: &str, value: Option<impl std::fmt::Display>) {
    if let Some(value) = value {
        argv.push(format!("{flag}={value}"));
    }
}

/// A path as one argv element. The refresh command is an *identifier*
/// in the user's shell — running it must reach the same file `bca`
/// walked. Non-UTF-8 paths cannot be expressed as a shell argument
/// verbatim, so surface them as a clearly-broken placeholder rather
/// than emit a `to_string_lossy` form that silently points at the
/// wrong file (AGENTS.md: identifier paths use `to_str()` with
/// explicit non-UTF-8 handling, not `path.display()`).
///
/// The placeholder contains `<`, `>` and spaces, which force
/// [`shell_quote`]'s slow path → single-quoted literal. Nothing here
/// quotes: every element of the argv is rendered through
/// `shell_quote` by [`refresh_baseline_command`], so an unquoted
/// `<non-UTF-8 path: …>` that a shell would read as input redirection
/// is unreachable by construction.
fn path_arg(p: &Path) -> String {
    p.to_str().map_or_else(
        || format!("<non-UTF-8 path: {}>", p.display()),
        str::to_string,
    )
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
