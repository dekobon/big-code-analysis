//! Severity-prefixed stderr diagnostics for the CLI.
//!
//! One helper per severity so the lowercase `error:` / `warning:` /
//! `note:` prefixes — matching clap's own usage-error formatter (#609) —
//! are enforced structurally rather than re-spelled at each call site.

use super::*;

/// Print an `error:`-prefixed diagnostic and exit with [`EXIT_TOOL_ERROR`].
///
/// The bare lowercase `error:` prefix matches clap's own usage-error
/// formatter (#609) so every stderr line — clap's and ours — reads in the
/// one rustc/cargo/git diagnostic family. Routing all fatal tool errors
/// through this single helper keeps the prefix structurally enforced
/// rather than re-spelled at each call site.
pub(crate) fn die(msg: impl Display) -> ! {
    eprintln!("error: {msg}");
    process::exit(EXIT_TOOL_ERROR);
}

/// Print a `warning:`-prefixed diagnostic to stderr (non-fatal). The
/// counterpart to [`die`] / [`note`]: one helper per severity so the
/// lowercase `warning:` prefix is enforced structurally (#609).
pub(crate) fn warn(msg: impl Display) {
    eprintln!("warning: {msg}");
}

/// Print a `note:`-prefixed diagnostic to stderr — supplementary context
/// attached to a warning or to surprising-but-valid input. The lowest of
/// the three diagnostic severities (#609).
pub(crate) fn note(msg: impl Display) {
    eprintln!("note: {msg}");
}

/// Emit a one-line stderr deprecation notice when a deprecated flag (or
/// subcommand) spelling is used in place of its replacement (issues
/// #688/#666/#646; the one-cycle alias horizon). The shared emission
/// point for the CLI flag/subcommand deprecations: the resolution-time
/// folds (`--headroom`, `--strict-exit-codes`) and the argv-scan alias
/// detector in [`crate::deprecations`] both route through here, so all
/// flag-deprecation chatter reads alike under the one `warning:` prefix
/// (via [`warn`]). The manifest's deprecated-key notices
/// ([`crate::manifest`]) are emitted at their own site but share it.
pub(crate) fn warn_deprecated_flag(old: &str, new: &str) {
    warn(format_args!(
        "`{old}` is deprecated; use `{new}` instead (removed in the next major release)"
    ));
}

/// Die with `failed to <verb> <path>: <err>`. Centralizes the most common
/// I/O error shape: open/read/parse/write of a user-supplied path that
/// failed with an error implementing `Display`.
pub(crate) fn die_io(verb: &str, path: &Path, err: impl Display) -> ! {
    die(format_args!("failed to {verb} {}: {err}", path.display()))
}
