//! `Cyclomatic` implementation for Objective-C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Objective-C cannot reuse `impl_cyclomatic_c_family!` either: that
// macro counts the C++ `catch` *keyword* token, but ObjC's grammar has
// no bare `catch` token — `@catch` is the `ATcatch` keyword and the
// handler body is a `catch_clause` node. Counting `CatchClause` adds one
// branch per `@catch`, the same edge `catch` contributes in C++.
// Fast enumeration (`for (id x in xs)`) folds into `for_statement`, so
// the `For` keyword-token arm already covers it — no separate node
// (issue #284: count keyword tokens, never the statement nodes, or the
// `For`/`While`/`If` keyword and their `*Statement` wrappers double-count).
impl Cyclomatic for ObjcCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Objc::*;
        match node.kind_id().into() {
            Case => stats.cyclomatic += 1.,
            SwitchStatement => stats.cyclomatic_modified += 1.,
            // `CatchClause` (the `@catch` handler) contributes the same
            // branch as the other decision kinds, so it rides the
            // combined arm — the C++ family macro folds `Catch` in the
            // same way.
            CatchClause | If | For | While | ConditionalExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
