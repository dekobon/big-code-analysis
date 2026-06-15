//! Single inventory of one-cycle CLI deprecations (issue #646).
//!
//! Before this module, the 2.0 line carried several unrelated
//! deprecation mechanisms — hidden clap `alias =` attributes (silently
//! accepted, no runtime signal), renamed flags folded at resolution time
//! (`--headroom`, `--strict-exit-codes`, which *did* warn via
//! [`crate::warn_deprecated_flag`]), and renamed manifest keys (warned in
//! [`crate::manifest`]). The clap-alias flags were the gap: a user still
//! typing `--num-jobs` or `--warning` got **no signal** before the next
//! major release removed the spelling — silent breakage at the bump.
//!
//! clap normalizes an `alias = "num-jobs"` to its canonical id (`jobs`)
//! before [`clap::ArgMatches`] is built, so neither `value_source` nor the
//! matched-id set can tell whether the user typed `--jobs` or `--num-jobs`.
//! Detecting a deprecated *spelling* therefore requires scanning the raw
//! `argv` against the table below — there is no post-parse signal to read.
//!
//! This module owns the inventory and the argv scan; emission routes
//! through [`crate::warn_deprecated_flag`] so every deprecation notice
//! reads alike under the one `warning:` prefix. The table is also
//! the canonical removal checklist for the next major bump: deleting an
//! `alias =` attribute means deleting its row here, and vice versa.

use std::ffi::OsString;

use crate::warn_deprecated_flag;

/// Deprecated long-flag spellings retained as hidden clap aliases for one
/// release cycle, paired with their canonical replacement. Every entry is
/// a spelling that is **never** canonical on any subcommand, so a bare
/// argv scan can flag it without knowing which subcommand consumed it.
///
/// Deliberately excluded:
/// - `--format` / `-O`: canonical on `metrics` / `ops` / `report`, only a
///   deprecated alias for `--report-format` on `check` (#659). A global
///   argv scan cannot tell the two contexts apart, so it would mis-warn on
///   a legitimate `bca metrics --format json`. The `check`-only `--format`
///   alias keeps its per-command help note rather than a runtime warning.
/// - The `--format text` value spelling: `text` is a *permanent*,
///   documented way to request the default human-readable output
///   explicitly (e.g. to override a `bca.toml`-set structured format), not
///   a one-cycle deprecation. Warning on it would penalize a supported use.
const DEPRECATED_FLAG_ALIASES: &[(&str, &str)] = &[
    // `--warning` (singular) -> `--warnings` (#604).
    ("--warning", "--warnings"),
    // `--language-type` -> `--language` (#595).
    ("--language-type", "--language"),
    // `--num-jobs` -> `--jobs` (#604).
    ("--num-jobs", "--jobs"),
    // `--fail-over` -> `--fail-above` on `vcs commit` (#603).
    ("--fail-over", "--fail-above"),
    // `--ls` / `--le` -> `--line-start` / `--line-end` on `dump` / `find`
    // (#518).
    ("--ls", "--line-start"),
    ("--le", "--line-end"),
    // `--only-*` -> `--*-only` on `exemptions` (#587 UX sweep).
    ("--only-markers", "--markers-only"),
    ("--only-excludes", "--excludes-only"),
    ("--only-baseline", "--baseline-only"),
];

/// `--output-format` is deprecated on every subcommand that accepts it,
/// but its canonical replacement is context-dependent: `--report-format`
/// on `check` (#659), `--format` everywhere else (#513). Handled out of
/// [`DEPRECATED_FLAG_ALIASES`] so the warning names the right replacement.
const OUTPUT_FORMAT_ALIAS: &str = "--output-format";

/// Deprecated subcommand spelling kept as a hidden clap alias for one
/// release cycle, paired with its canonical name. `vcs jit` -> `vcs
/// commit` (#603). A subcommand alias is positional, so it is detected by
/// presence in subcommand position rather than by a flag-style scan.
const DEPRECATED_SUBCOMMAND_ALIASES: &[(&str, &str)] = &[("jit", "commit")];

/// Emit a one-cycle deprecation warning for every deprecated alias
/// spelling present in `argv`, once per spelling. Called at the parse
/// chokepoint after clap has *accepted* the input (a rejected parse never
/// reaches here), so the canonical spellings emit nothing and only a
/// genuine alias use draws a notice.
///
/// Warnings are always-on, independent of `--warnings` / `-w`: the whole
/// point of #646 is that alias users get no migration signal today, and
/// gating the notice behind an off-by-default flag would perpetuate the
/// silence.
pub(crate) fn warn_deprecated_aliases(argv: impl IntoIterator<Item = OsString>) {
    let tokens: Vec<String> = argv
        .into_iter()
        .skip(1) // program name
        .filter_map(|s| s.into_string().ok())
        .collect();

    // A `--` end-of-options marker stops flag interpretation; anything
    // after it is a positional value (e.g. a path literally named
    // `--num-jobs`) and must not draw a flag warning.
    let flag_tokens = match tokens.iter().position(|t| t == "--") {
        Some(end) => &tokens[..end],
        None => &tokens[..],
    };

    for (deprecated, canonical) in DEPRECATED_FLAG_ALIASES {
        if flag_tokens.iter().any(|t| is_flag_spelling(t, deprecated)) {
            warn_deprecated_flag(deprecated, canonical);
        }
    }

    if flag_tokens
        .iter()
        .any(|t| is_flag_spelling(t, OUTPUT_FORMAT_ALIAS))
    {
        // `--report-format` on `check`, `--format` everywhere else.
        // Scope the `check` test to the subcommand position, not a scan
        // over all argv (a path/value literally named `check` must not
        // flip the suggested replacement).
        let canonical = if top_subcommand(flag_tokens) == Some("check") {
            "--report-format"
        } else {
            "--format"
        };
        warn_deprecated_flag(OUTPUT_FORMAT_ALIAS, canonical);
    }

    for (deprecated, canonical) in DEPRECATED_SUBCOMMAND_ALIASES {
        // Scan `flag_tokens`, not `tokens`: a path literally named `jit`
        // after a `--` marker is a positional value, never a subcommand
        // (clap stops subcommand dispatch at `--`), so it must not warn
        // (#836).
        if subcommand_used(flag_tokens, deprecated) {
            warn_deprecated_flag(deprecated, canonical);
        }
    }
}

/// Whether `token` is the long flag `flag`, in either the bare
/// `--flag` form or the `--flag=value` form. Matching `--flag=` as well
/// catches `--num-jobs=4`; a bare prefix test (`starts_with`) would
/// wrongly match an unrelated longer flag, so the `=` boundary is
/// required.
fn is_flag_spelling(token: &str, flag: &str) -> bool {
    token == flag
        || token
            .strip_prefix(flag)
            .is_some_and(|rest| rest.starts_with('='))
}

/// The top-level subcommand: the first token that is not a flag. Used to
/// resolve the context-dependent `--output-format` replacement without a
/// blind argv scan. A global flag that takes a separate value before the
/// subcommand could shadow it, but the only consequence is a less-precise
/// deprecation hint, never a wrong flag being accepted.
fn top_subcommand(tokens: &[String]) -> Option<&str> {
    tokens
        .iter()
        .map(String::as_str)
        .find(|t| !t.starts_with('-'))
}

/// Separated value-taking `global = true` flags on `vcs` (`lib.rs`):
/// each consumes the *following* token as its value, so that value token
/// must be skipped when scanning for the subcommand position. Without
/// this, `vcs --long-window 6mo jit` would read `6mo` as the subcommand
/// and miss the deprecated `jit` (#834). The `--flag=value` form is
/// self-contained and needs no skip. Boolean globals (`-w`,
/// `--full-history`, …) take no value and are simply skipped as flags.
const VCS_VALUE_TAKING_GLOBALS: &[&str] = &[
    "--long-window",
    "--recent-window",
    "--ref",
    "--bot-pattern",
    "--as-of",
    "--risk-formula",
];

/// Whether the positional subcommand `name` is the one invoked under
/// `vcs`. A subcommand alias has no leading dashes, so a value token
/// equal to the alias would be a false positive; the subcommand is the
/// first non-flag token after `vcs`, found by scanning forward and
/// skipping flags and the separated values of value-taking globals. This
/// tolerates any `global = true` flag preceding the subcommand (#834)
/// while still rejecting `jit` in a non-subcommand position — e.g. a path
/// argument after the canonical `commit` (#835).
fn subcommand_used(tokens: &[String], name: &str) -> bool {
    let Some(vcs_index) = tokens.iter().position(|t| t == "vcs") else {
        return false;
    };

    let mut rest = tokens[vcs_index + 1..].iter();
    while let Some(token) = rest.next() {
        if token.starts_with('-') {
            // A separated value-taking global consumes the next token as
            // its value; skip it so the value is not mistaken for the
            // subcommand. The `--flag=value` form is one token already.
            if VCS_VALUE_TAKING_GLOBALS.contains(&token.as_str()) {
                rest.next();
            }
            continue;
        }
        // First non-flag token is the subcommand position.
        return token == name;
    }
    false
}

#[cfg(test)]
#[path = "deprecations_tests.rs"]
mod deprecations_tests;
