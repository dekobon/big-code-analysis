#![allow(
    clippy::enum_glob_use,
    clippy::too_many_lines,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::{Abc, Stats};
use crate::macros::kotlin_bool_terminal_kinds;
use crate::*;

// Fitzpatrick's ABC rules adapted for Kotlin syntax. Kotlin shares the
// JVM and Java's spec roots: assignments count once per `=` / augmented
// assignment / ++ / --, branches count once per function invocation or
// object construction, conditions count comparison operators plus the
// `else` / `when`-entry / `catch` arms. Compared with the Java impl we
// stay token-level (matching the leaf kind_ids) rather than walking
// `Modifiers` children; the Kotlin grammar exposes the relevant
// operators directly as token nodes inside `binary_expression`,
// `assignment`, `prefix_expression`, and `postfix_expression`.

// Returns true when this `=` token initialises an *immutable* (`val`)
// binding, whose initialiser is part of the declaration and therefore not
// an ABC assignment. The decision is structural: the `=` must be a direct
// child of a `property_declaration` or `class_parameter`, and that parent
// must carry a `val` keyword child. A `var`/plain declaration initialiser
// and any standalone `assignment` return false (they count).
fn kotlin_eq_initializes_immutable_binding<'a>(
    eq_node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    use Kotlin::*;

    let Some(parent) = ancestors.parent(eq_node) else {
        return false;
    };
    if !matches!(
        parent.kind_id().into(),
        PropertyDeclaration | ClassParameter
    ) {
        return false;
    }
    parent.children().any(|child| child.kind_id() == Val)
}

// Kotlin ABC unary-conditional walker (Fitzpatrick Rule 9; issue #557).
// tree-sitter-kotlin-ng parses `a && b || c` as a left-nested chain of
// flat `binary_expression` nodes carrying `&&` / `||` operator tokens,
// the same shape as the Java template. Negation surfaces as
// `unary_expression` whose child(0) is the `!` token; the condition slot
// may also be wrapped in `parenthesized_expression`. Both are unwrapped
// by `kotlin_inspect_container` to reach the inner bare operand.
fn kotlin_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    use Kotlin::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();
    // A parenthesised / negated operand only contributes when it sits in
    // a boolean-evaluating slot. The chain wrapper (`binary_expression`)
    // and the control-flow headers all qualify; a `!`-operator anywhere
    // also proves the operand is boolean (`if (!x)`).
    let mut has_boolean_content = matches!(
        parent.kind_id().into(),
        BinaryExpression | IfExpression | WhileStatement | DoWhileStatement | ForStatement
    );

    loop {
        let is_parens = matches!(node_kind, ParenthesizedExpression);
        let is_not = matches!(node_kind, UnaryExpression)
            && node.child(0).is_some_and(|c| c.kind_id() == BANG as u16);

        if !is_parens && !is_not {
            break;
        }
        if !has_boolean_content && is_not {
            has_boolean_content = true;
        }

        // Parenthesised expressions wrap their inner expression at child
        // index 1 (after the `(` token); a `!` unary stores its operand
        // at index 1 (after the `!` token).
        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        if matches!(node_kind, kotlin_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// Counts each non-comparison operand of a Kotlin `&&` / `||` chain once.
// Mirrors `java_count_unary_conditions`: comparison operands are nested
// `binary_expression` nodes (absent from `kotlin_bool_terminal_kinds!()`)
// and so contribute nothing, while bare identifiers / calls / member
// accesses each add one. Inner chain links and `!` / paren wrappers are
// routed through `kotlin_inspect_container`.
fn kotlin_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Kotlin::*;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(node_kind, kotlin_bool_terminal_kinds!())
                && matches!(list_kind, BinaryExpression)
            {
                *conditions += 1.;
            } else if node.is_named() {
                kotlin_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// Count the bare-predicate condition of a Kotlin `if`/`while`/`do-while`
// as one condition — Fitzpatrick's "unary conditional expression",
// mirroring `ruby_count_condition` / `rust_count_condition`.
// tree-sitter-kotlin-ng exposes the predicate via the `condition` field on
// `if_expression`, `while_statement`, and `do_while_statement`, so the
// field lookup is position-independent across all three forms. A bare
// terminal (`if (flag)`) counts directly; a comparison or boolean chain
// (`if (a == b)`, `if (a && b)`) is a nested `binary_expression` already
// counted by the comparison-token and `&&`/`||` walker arms, so it adds
// nothing here. A parenthesised or negated predicate (`if ((flag))`,
// `if (!flag)`) is unwrapped by `kotlin_inspect_container`. Without this
// arm, idiomatic Kotlin bare predicates reported 0 ABC conditions while
// Kotlin's own cyclomatic counted the decision, breaking the
// conditions >= decisions invariant (#469/#473/#456/#696); issue #773.
fn kotlin_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    use Kotlin::*;
    let kind = condition.kind_id().into();
    if matches!(kind, kotlin_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else if matches!(kind, ParenthesizedExpression | UnaryExpression) {
        kotlin_inspect_container(condition, parent, conditions);
    }
}

impl Abc for KotlinCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use Kotlin::*;

        match node.kind_id().into() {
            // Augmented assignments and pre/post increment-decrement
            // always count, regardless of declaration context.
            PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ | PLUSPLUS | DASHDASH => {
                stats.assignments += 1.;
            }
            // Plain `=` token. A declaration initialiser (`val`/`var x = …`,
            // primary-constructor parameter default `class C(val a = 5)`) is
            // "part of the declaration" (Fitzpatrick), so the `=` is counted
            // only for *mutable* bindings; an immutable `val` initialiser is
            // suppressed. A standalone `assignment` always counts.
            //
            // This is decided structurally from the `=` token's parent node
            // rather than a persistent declaration stack. tree-sitter-kotlin
            // does NOT emit a `SEMI` token even for explicit semicolons, and
            // newline-terminated statements emit no terminator at all, so a
            // stack cleared on `SEMI` (the pre-#455 design) never cleared:
            // the immutable-`val` sentinel leaked and suppressed every later
            // standalone assignment in the same function (issue #455).
            EQ if !kotlin_eq_initializes_immutable_binding(node, ancestors) => {
                stats.assignments += 1.;
            }
            // Branches: every call expression plus object construction.
            // Kotlin's `new` is implicit — `Foo()` parses as
            // `CallExpression` with a type-named receiver. The
            // Halstead-side classification treats it uniformly. Indexed
            // access (`arr[i]`) is NOT a branch (it's an operator on a
            // sequence), matching the Java rule of "method invocation
            // only".
            CallExpression => {
                stats.branches += 1.;
            }
            // Conditions: comparison operators, identity equality,
            // ternary-elvis (`?:`), `as?` safe-cast, and the arms of
            // control-flow constructs (`else`, `catch`, `when` entries).
            // Kotlin's `if`-expression does not need an extra count for
            // the `if` keyword itself — Fitzpatrick counts the
            // *conditions*, and the unary condition is already implicit
            // in the boolean operand. We add the `if` arm via the `Else`
            // keyword for else-branches and via `WhenEntry` for `when`.
            // `Try` is the `try` keyword token of a `try_expression`.
            // Fitzpatrick counts both `try` and `catch` as conditions, and
            // Java / C# / C++ / Groovy already count both; Kotlin previously
            // counted only `CatchBlock`, so `try {} catch (e) {}` scored one
            // fewer condition here than in every sibling (#696).
            LTEQ | GTEQ | EQEQ | EQEQEQ | BANGEQ | BANGEQEQ | Try | CatchBlock | QMARKCOLON
            | AsQMARK => {
                stats.conditions += 1.;
            }
            // Phase-2B condition slot: the bare predicate of an
            // `if`/`while`/`do-while` is one unary condition. The
            // `condition` field locates the predicate position-
            // independently across all three forms. `kotlin_count_condition`
            // counts a bare terminal directly and routes paren/negation
            // wrappers through `kotlin_inspect_container`, while a comparison
            // or `&&`/`||` predicate is a `binary_expression` already counted
            // by the token arms above — so no double-count (#773).
            IfExpression | WhileStatement | DoWhileStatement => {
                if let Some(condition) = node.child_by_field_name("condition") {
                    kotlin_count_condition(&condition, node, &mut stats.conditions);
                }
            }
            // A `when` entry is a decision point except for the `else ->`
            // fallback arm, which is the analogue of C-family `default:`
            // and Rust's wildcard `_ =>`. Cyclomatic already excludes it
            // (`WhenEntry if !kotlin_when_entry_is_else`); ABC must track
            // the same decision count (issue #456, lesson 11).
            WhenEntry if !crate::metrics::cyclomatic::kotlin_when_entry_is_else(node) => {
                stats.conditions += 1.;
            }
            // `else` is a keyword token used in both `if_expression`'s
            // else-clause and `when`'s `else ->` entry. Only count it
            // when it belongs to an `if_expression`; the `WhenEntry`
            // wrapper above already covers the `when` case.
            Else if ancestors
                .parent(node)
                .is_some_and(|p| p.kind_id() == IfExpression) =>
            {
                stats.conditions += 1.;
            }
            // `<` and `>` may appear as type-argument brackets
            // (`List<Int>`); exclude those by checking the parent kind.
            LT | GT
                if ancestors.parent(node).is_some_and(|p| {
                    !matches!(p.kind_id().into(), TypeArguments | TypeParameters)
                }) =>
            {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 walker: each non-comparison operand of a
            // `&&` / `||` chain is one condition (issue #557). The short-
            // circuit operators are not counted directly (cross-language
            // policy, #395); the walker fires off the operator token and
            // inspects the parent `binary_expression`.
            AMPAMP | PIPEPIPE => {
                if let Some(parent) = ancestors.parent(node) {
                    kotlin_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            _ => {}
        }
    }
}
