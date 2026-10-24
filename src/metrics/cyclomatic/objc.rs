//! `Cyclomatic` implementation for Objective-C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// The C-family shape with `catch_clause` as its extra: ObjC's grammar
// has no bare `catch` token — `@catch` is the `ATcatch` keyword and the
// handler body is a `catch_clause` node — so counting `CatchClause` adds
// one branch per `@catch`, the same edge C++'s `catch` keyword
// contributes. Fast enumeration (`for (id x in xs)`) folds into
// `for_statement`, so the `For` keyword-token arm already covers it —
// no separate node (issue #284: count keyword tokens, never the
// statement nodes, or the `For` / `While` / `If` keyword and their
// `*Statement` wrappers double-count).
impl_cyclomatic_c_family_rows!(
    ObjcCode,
    Objc,
    roles = crate::roles::objc,
    extra = [CatchClause]
);
