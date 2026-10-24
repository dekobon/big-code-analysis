//! `Cyclomatic` implementation for C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// The C-family shape with no `extra`: C has no exceptions, so no
// `catch` token joins `if` / `for` / `while`. The decision-kind set is
// otherwise the family's — the `case` label, the `?:` ternary and the
// `&&` / `||` operators read from the rows of `roles/c.rs`, with
// `switch` adding only to the modified count (#284).
impl_cyclomatic_c_family_rows!(CCode, C, roles = crate::roles::c);
