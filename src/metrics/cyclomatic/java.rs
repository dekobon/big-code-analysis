//! `Cyclomatic` implementation for Java.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// `Guard` is the Java 21 pattern-switch guard (`case Integer i when
// i > 5 ->`), and it is a decision the enclosing `case` does not
// already pay for (#1454, transferring #1422's C# rule): a guarded arm
// fails two ways — the pattern does not match, or it matches and the
// guard is false — while contributing one decision. The `switch_label`
// production is shared by the arrow and colon forms, so one arm covers
// both spellings.
//
// The clause *node* (`guard`, 184), not the `when` keyword token it
// contains. Both count once per guard at this pin — `Java::When` (76)
// is a plain keyword here, with none of the `_reserved_identifier`
// doubling that forced C#'s hand (`int when = 1;` emits an
// `identifier`, verified by `bca dump`, not inferred). The clause node
// is used anyway, because it is the construct the rule is about and it
// stays correct if a later grammar gains that alias. Neither kind
// carries a numeric-suffix alias (grammar-dispatch §1).
//
// No double count (§5): the guard's body is an ordinary expression, so
// the only keyword token inside it is whatever the guard itself spells,
// and `If` / `For` / `While` / `Catch` cannot appear in an expression
// position.
impl_cyclomatic_java_like!(JavaCode, Java, [Guard]);
