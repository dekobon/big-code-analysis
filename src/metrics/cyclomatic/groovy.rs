//! `Cyclomatic` implementation for Groovy.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Groovy extra branches under the pinned dekobon grammar:
// - `Assert` (cyclomatic branch — same as Java; its `assert` keyword
//   is a runtime check that branches on its condition).
// - Elvis operator token `?:` (`QMARKCOLON`): the grammar surfaces
//   Elvis as a distinct `elvis_expression` node with `?:` as a real
//   lexer token, so the macro picks it up as +1 per occurrence
//   (closes #246 cyclomatic case).
// - Safe-navigation `?.` (`QMARKDOT`) and `??.` (`QMARKQMARKDOT`):
//   both are short-circuit — they skip the member access/call when the
//   LHS is null — so each occurrence is one decision point, mirroring
//   the Kotlin/PHP/JS/C# treatment of `?.` (issues #281, #452). The
//   grammar emits the `?.` token once per operator inside a
//   `safe_navigation_expression` and the `??.` token inside a
//   `safe_chain_dot_expression`, so matching the tokens counts each
//   textual operator exactly once, including in chains (`a?.b?.c` is
//   +2). Matching the wrapper nodes instead would miscount nested
//   chains; the token is the single granularity that fires once per
//   textual operator, paralleling Kotlin/TS which match `QMARKDOT`.
impl_cyclomatic_java_like!(
    GroovyCode,
    Groovy,
    [Assert, QMARKCOLON, QMARKDOT, QMARKQMARKDOT]
);
