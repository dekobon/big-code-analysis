//! Severity-prefixed stderr diagnostics for the CLI.
//!
//! One helper per severity so the lowercase `error:` / `warning:` /
//! `note:` prefixes — matching clap's own usage-error formatter (#609) —
//! are enforced structurally rather than re-spelled at each call site.

use std::sync::{MutexGuard, PoisonError};

use super::*;

/// Print an `error:`-prefixed diagnostic and exit with [`EXIT_TOOL_ERROR`].
///
/// The bare lowercase `error:` prefix matches clap's own usage-error
/// formatter (#609) so every stderr line — clap's and ours — reads in the
/// one rustc/cargo/git diagnostic family. Routing all fatal tool errors
/// through this single helper keeps the prefix structurally enforced
/// rather than re-spelled at each call site.
///
/// Every live [`TempTree`] is removed first: `process::exit` runs no
/// destructors, so without that a failure deep inside a walk would leave
/// the tree behind.
pub(crate) fn die(msg: impl Display) -> ! {
    eprintln!("error: {msg}");
    remove_temp_trees();
    process::exit(EXIT_TOOL_ERROR);
}

/// The paths of the live [`TempTree`]s, for [`die`] to remove.
static TEMP_TREES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// A temporary directory that is removed on drop, like the
/// [`tempfile::TempDir`] it wraps, and also by [`die`].
///
/// A failure far from the code that created the tree (a seed missing on
/// one side of a walk, an unknown `--language`) ends the process through
/// [`die`]; registering the tree here makes its removal independent of
/// how the process exits, so no call site has to be kept free of `die`.
/// A signal still ends the process without removing it.
pub(crate) struct TempTree(tempfile::TempDir);

impl TempTree {
    pub(crate) fn new() -> std::io::Result<Self> {
        let dir = tempfile::TempDir::new()?;
        lock_temp_trees().push(dir.path().to_path_buf());
        Ok(Self(dir))
    }

    pub(crate) fn path(&self) -> &Path {
        self.0.path()
    }
}

impl Drop for TempTree {
    /// Deregister only; the wrapped `TempDir` removes the tree when it
    /// drops right after this.
    fn drop(&mut self) {
        let mut trees = lock_temp_trees();
        if let Some(i) = trees.iter().position(|p| p == self.0.path()) {
            trees.swap_remove(i);
        }
    }
}

/// The registry is a plain list whose every update is a single push or
/// remove, so a panic elsewhere while it was held leaves it consistent.
fn lock_temp_trees() -> MutexGuard<'static, Vec<PathBuf>> {
    TEMP_TREES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Best-effort removal of every live [`TempTree`]; the process is about
/// to exit with its own error, so a failure here is not reported.
fn remove_temp_trees() {
    let trees = std::mem::take(&mut *lock_temp_trees());
    for tree in trees {
        // A walk runs with the working directory inside the tree it
        // walks, and Windows refuses to remove a directory that is some
        // process's working directory. Step out to the parent first.
        if let Some(parent) = tree.parent() {
            let _ = std::env::set_current_dir(parent);
        }
        let _ = std::fs::remove_dir_all(&tree);
    }
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
