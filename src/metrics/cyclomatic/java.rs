//! `Cyclomatic` implementation for Java.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/java.rs`, read at the projection
// the metric counts (#1694, #1695, #1697):
//
// - `&&` / `||` from `ShortCircuit` at its leaf, the token.
// - The ternary from `Ternary` at its wrapper, the `ternary_expression`
//   node — not its ungated `?`, which recovery can strand.
// - `Guard`, the Java 21 pattern-switch guard (`case Integer i when
//   i > 5 ->`), at its wrapper: a decision the enclosing `case` does
//   not already pay for (#1454, transferring #1422's C# rule) — a
//   guarded arm fails two ways while contributing one decision. The
//   clause node, not the `when` keyword it contains: both count once
//   per guard at this pin, and the node stays correct if a later
//   grammar aliases the keyword the way C#'s does.
// - `Branch`, `Loop` and `Catch` at their leaves — the `if` / `for` /
//   `while` / `catch` *keyword tokens* (#284): the `while` keyword
//   fires inside both `WhileStatement` and `DoStatement`, and the
//   `for` keyword inside both `ForStatement` and
//   `EnhancedForStatement`, so the statement nodes would double-count.
//   `java_do_statement_counts_in_cyclomatic` and
//   `java_enhanced_for_statement_counts_in_cyclomatic` pin the
//   keyword-driven counts.
// - The `case` label from `CaseArm` at its leaf; its row excludes
//   `default`, so that is the only paying label.
// - The `switch` keyword from `Switch` at its leaf — once per switch,
//   statement and expression form alike, which keeps the modified
//   count aligned with the `case` arms (#300).
//
// No double count (grammar-dispatch §5): a guard's body is an ordinary
// expression, so the only keyword token inside it is whatever the guard
// itself spells, and `if` / `for` / `while` / `catch` cannot appear in
// an expression position. `ElseIf` is not read here: its row pays
// nothing (the chained `if`'s own keyword is `Branch`'s), and `NullSafe`
// is absent.
impl_cyclomatic_rows!(
    JavaCode,
    Java,
    roles = crate::roles::java,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        guard::owns_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        catch::pays_at_leaf_kind,
    ],
    standard = [case_arm::pays_at_leaf_kind],
    modified = [switch::pays_at_leaf_kind],
    extra = [],
    constraints = [],
);
