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
//! the CLI decides how to present them. What is printed rather than
//! returned is a best-effort skip — a path that is not valid UTF-8, an
//! entry that is not a regular file — or a malformed in-source
//! suppression marker, in every case leaving the run to continue with
//! that item omitted.
//!
//! One library diagnostic deliberately stays outside this helper:
//! [`crate::ConcurrentRunner`]'s per-file failure line
//! (`error processing <path>: <err>`, `src/concurrent_files.rs`). It is
//! error-severity text emitted by a *callback* the embedder supplied,
//! so prefixing it here would claim a severity ladder the library has
//! no `error:` half of. Worth revisiting alongside a library `error`
//! helper, not before.
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
