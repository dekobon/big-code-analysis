//! Severity-prefixed stderr diagnostics for the library.
//!
//! The library counterpart to `big-code-analysis-cli/src/diag.rs`: one
//! helper per severity so the lowercase `warning:` prefix — matching
//! clap's own usage-error formatter, and therefore the rustc/cargo/git
//! diagnostic family (#609) — is written in exactly one place per crate
//! rather than re-spelled at each call site (#1199).
//!
//! Only `warning` exists here. The library never exits the process, so
//! it has no `error:`/`die` counterpart: fatal conditions are returned
//! as [`MetricsError`](crate::MetricsError) or [`std::io::Error`] and
//! the CLI decides how to present them. The few diagnostics that *are*
//! printed rather than returned are all best-effort skips — a path that
//! is not valid UTF-8, an entry that is not a regular file — where the
//! run continues with that item omitted.
//!
//! `utils/check-diagnostic-prefix.py` (`make lint`) blocks a capitalised
//! `Warning:` / `Error:` / `Note:` literal from reappearing at a call
//! site that bypasses these helpers.

use std::fmt::Display;

/// Print a `warning:`-prefixed diagnostic to stderr (non-fatal).
///
/// A multi-line `msg` is prefixed on its first line only; the remaining
/// lines are emitted verbatim, so a message whose continuation lines are
/// already indented reads as one block under the header.
pub(crate) fn warn(msg: impl Display) {
    eprintln!("warning: {msg}");
}
