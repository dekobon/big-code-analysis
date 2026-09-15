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
// - Safe indexing `?[` (`QMARKLBRACK`): the same short-circuit on a
//   null receiver, spelled for a subscript instead of a member access
//   (#1471). It was the one member of that family with no arm, so
//   `l?[0]` read level with the unconditional `l[0]` while `l?.get(0)`
//   read one higher. The token granularity is the same choice for the
//   same reason: the grammar emits one `?[` per operator inside a
//   `safe_subscript_expression`, which nests for a chain (`l?[0]?[1]`
//   is one wrapper inside another), so the token counts each operator
//   once and the wrapper would not. No §5 double count — that wrapper
//   node reaches no cyclomatic arm; it is an ABC bool-terminal only.
impl_cyclomatic_java_like!(
    GroovyCode,
    Groovy,
    [Assert, QMARKCOLON, QMARKDOT, QMARKQMARKDOT, QMARKLBRACK]
);
