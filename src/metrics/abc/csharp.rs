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
use crate::macros::{
    csharp_bool_terminal_kinds, csharp_paren_expr_kinds, csharp_prefix_unary_expr_kinds,
};
use crate::*;

fn csharp_inspect_container(container_node: &Node, parent: &Node, conditions: &mut f64) {
    use Csharp::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();

    // Seed the boolean-context flag from the parent: known-boolean
    // contexts (loop / if / binary expression) imply the contained
    // expression evaluates as a condition.
    let mut has_boolean_content = match parent.kind_id().into() {
        BinaryExpression | IfStatement | WhileStatement | DoStatement | ForStatement => true,
        ConditionalExpression => parent
            .child_by_field_name("condition")
            .is_some_and(|condition| condition.id() == node.id()),
        _ => false,
    };

    // Walk down through `(...)` and `!...` wrappers until we either hit
    // the underlying operand or run out of nesting. The C# grammar
    // aliases each of these kinds across multiple `kind_id`s
    // (lesson #2): match every numbered variant.
    loop {
        let is_parens = matches!(node_kind, csharp_paren_expr_kinds!());
        let is_not = matches!(node_kind, csharp_prefix_unary_expr_kinds!())
            && node
                .child(0)
                .is_some_and(|c| matches!(c.kind_id().into(), BANG));

        if !is_parens && !is_not {
            break;
        }

        // A `!` wrapper proves the contained value is boolean even
        // when the parent context didn't (e.g. `return !x;`).
        if !has_boolean_content && is_not {
            has_boolean_content = true;
        }

        // Both `parenthesized_expression` and `prefix_unary_expression`
        // store their inner expression at child index 1.
        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        // Found the innermost operand; count it if a boolean context
        // was established up the chain. The `csharp_bool_terminal_kinds!()`
        // set bundles invocation aliases, the `Identifier` /
        // `BooleanLiteral` leaves, and the five bool-evaluating kinds
        // restored by #372 (member access / await / cast / is-pattern /
        // element access).
        if matches!(node_kind, csharp_bool_terminal_kinds!()) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

fn csharp_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Csharp::*;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            // `csharp_bool_terminal_kinds!()` bundles invocation aliases,
            // `Identifier`, `BooleanLiteral`, and the bool-evaluating
            // expression kinds restored by #372 (member access / await /
            // cast / is-pattern / element access).
            if matches!(node_kind, csharp_bool_terminal_kinds!())
                && matches!(list_kind, BinaryExpression)
            {
                *conditions += 1.;
            } else {
                csharp_inspect_container(&node, list_node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// ABC token-level helpers for C#. Mirror of Java's helper layout with
// C#-specific deltas: every aliased kind id is matched via the
// `csharp_*_kinds!()` macros (lesson #2); `ObjectCreationExpression`
// joins `InvocationExpression*` as a branch; the `<` / `>` parent
// guard widens to `TypeArgumentList | TypeParameterList |
// FunctionPointerType`; `ConditionalExpression` replaces Java's
// `TernaryExpression`; `for_statement` exposes its condition via the
// named `condition` field rather than positional index.

// Whether `eq_node` initialises a `const` binding — a compile-time
// constant, so its initializer is part of the declaration and not an
// assignment (the C# spelling of Java's `final`). One hop deeper than
// Java: the declarator sits in a `variable_declaration` inside the
// `local_declaration_statement` / `field_declaration` that carries the
// `modifier` nodes, one of which wraps the `const` token. `readonly` is
// not `const`; its initializer counts. A `const` initializer must be a
// constant expression, so nothing can nest an `=` inside it — the
// sentinel stack this replaces could not leak here as it did in Java,
// and the structural form is adopted so the three sibling dispatchers
// share one rule. The modifiers precede the declaration, so the scan
// stops at it.
fn csharp_eq_initializes_const_binding<'a>(
    eq_node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    use Csharp::*;
    let mut climb = ancestors.iter(eq_node).map(|(ancestor, _)| ancestor);
    let is_declaration = |node: &Node| {
        matches!(
            node.kind_id().into(),
            VariableDeclaration | VariableDeclaration2
        )
    };
    if !climb.next().is_some_and(|declarator| {
        matches!(
            declarator.kind_id().into(),
            VariableDeclarator | VariableDeclarator2
        )
    }) {
        return false;
    }
    if !climb
        .next()
        .is_some_and(|declaration| is_declaration(&declaration))
    {
        return false;
    }
    climb.next().is_some_and(|statement| {
        matches!(
            statement.kind_id().into(),
            LocalDeclarationStatement | FieldDeclaration
        ) && statement
            .children()
            .take_while(|child| !is_declaration(child))
            .any(|child| child.kind_id() == Modifier && child.is_child(Const as u16))
    })
}

fn csharp_count_token_assignment<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) -> bool {
    use Csharp::*;
    match node.kind_id().into() {
        STAREQ | SLASHEQ | PERCENTEQ | DASHEQ | PLUSEQ | LTLTEQ | GTGTEQ | GTGTGTEQ | AMPEQ
        | PIPEEQ | CARETEQ | QMARKQMARKEQ | PLUSPLUS | DASHDASH => {
            stats.assignments += 1.;
        }
        // Count `=` unless it is the initializer of a `const` declaration.
        EQ => {
            if !csharp_eq_initializes_const_binding(node, ancestors) {
                stats.assignments += 1.;
            }
        }
        _ => return false,
    }
    true
}

// Counts branch tokens: every invocation, `new` allocation, and
// constructor delegation.
// `ConstructorInitializer` is the `: base(…)` / `: this(…)` delegation on a
// constructor — a call by Fitzpatrick's rule, and the C# spelling of the
// shape Java and Groovy count (#1279). Unlike the invocation kinds it
// carries no numeric-suffix aliases, and it does not wrap an
// `InvocationExpression` for the delegation itself, so no double count
// arises; calls in its argument list are separate nodes counted on their own.
fn csharp_count_token_branch(node: &Node, stats: &mut Stats) -> bool {
    use Csharp::*;
    if matches!(
        node.kind_id().into(),
        crate::Csharp::InvocationExpression
            | crate::Csharp::InvocationExpression2
            | crate::Csharp::InvocationExpression3
            | ObjectCreationExpression
            | ConstructorInitializer
    ) {
        stats.branches += 1.;
        return true;
    }
    false
}

fn csharp_count_token_condition<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) -> bool {
    use Csharp::*;
    match node.kind_id().into() {
        // The statement `switch` counts its `Case` arms; the `default:`
        // arm (token `Default`, shared by both classic `default:` and
        // arrow `default ->` forms) is the unconditional fallthrough and
        // is excluded, mirroring cyclomatic's `Case`-only count and the
        // expression-arm discard rule below (issues #456, #469).
        GTEQ | LTEQ | EQEQ | BANGEQ | Else | Case | Try | Catch => {
            stats.conditions += 1.;
        }
        // tree-sitter-c-sharp emits a bare `?` from exactly four
        // productions: `nullable_type` (`int? x`),
        // `type_parameter_constraint` (`where T : class?`),
        // `conditional_expression` (the ternary) and
        // `conditional_access_expression` (`a?.b` *and* `a?[0]` — both
        // spell their operator as this same bare token, not a distinct
        // `?.` / `?[` kind). The first two are type syntax and no more a
        // decision than the `<` / `>` around a generic (#1275); the last
        // two are decisions.
        //
        // Denylist polarity, deliberately — the opposite of the TS/TSX
        // allowlist this fix installs alongside it and of the Java one
        // #1274 already landed. An allowlist would have to name
        // `ConditionalAccessExpression` explicitly to keep counting
        // `a?.b`, and that counting is load-bearing: C# cyclomatic
        // counts the `ConditionalAccessExpression` node
        // (`src/metrics/cyclomatic/csharp.rs`), so dropping it would put
        // ABC *below* C#'s own cyclomatic decision count on a safe-
        // navigation chain. Denying the two type-syntax parents keeps
        // that count without an allowlist entry a later "consistency"
        // pass could drop. It is agreement on this one token, not on
        // the metric: `??` is a C# cyclomatic decision (`QMARKQMARK`
        // there) and no ABC condition here, a pre-existing gap the
        // JS-family arms do not share.
        //
        // The agreement being protected is C#-internal, not cross-
        // language: `a?.b?.c` scores ABC conditions 2 in C# and 0 in
        // TypeScript, JavaScript, Kotlin and Groovy, whose `?.` is a
        // distinct token their ABC arms never list — even though
        // `safe_navigation_chain_parity` pins all five at +2
        // *cyclomatic*. That ABC-side divergence is pre-existing and
        // out of scope for #1275; this arm preserves C#'s side of it
        // rather than silently changing it.
        //
        // The cost of that polarity is that it fails *open*: type syntax
        // the grammar gains later starts counting. That is not
        // hypothetical — `type_parameter_constraint` was the second half
        // of #1275 and is absent from the issue's own deny set. The four
        // productions above are the closed enumeration at the pinned
        // `=0.23.5`; re-derive it on a grammar bump.
        QMARK
            if ancestors.parent(node).is_some_and(|parent| {
                !matches!(
                    parent.kind_id().into(),
                    NullableType | TypeParameterConstraint
                )
            }) =>
        {
            stats.conditions += 1.;
        }
        // A `switch` *expression* arm (`x switch { 1 => …, _ => … }`) is a
        // decision point. The statement `switch` counts via its `Case`
        // tokens above; an expression arm carries none, so it scored zero
        // conditions before #456 even though C# cyclomatic counts it. The
        // bare-discard arm (`_ =>` / `var _ =>`, no `when` guard) is the
        // `default:` analogue and is excluded — mirroring the cyclomatic
        // gate (lesson 11).
        SwitchExpressionArm
            if !crate::metrics::cyclomatic::csharp_switch_expression_arm_is_bare_discard(node) =>
        {
            stats.conditions += 1.;
        }
        // Excludes `<` and `>` used as type-syntax delimiters: generic
        // type arguments (`Dictionary<K, V>`), type parameter
        // declarations (`class Foo<T> { }`), and the parameter-list
        // delimiters of unsafe function-pointer types
        // (`delegate*<int, int>`).
        GT | LT => {
            if let Some(parent) = ancestors.parent(node)
                && !matches!(
                    parent.kind_id().into(),
                    TypeArgumentList | TypeParameterList | FunctionPointerType
                )
            {
                stats.conditions += 1.;
            }
        }
        _ => return false,
    }
    true
}

fn csharp_walk_for_conditions<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    use Csharp::*;
    let conds = &mut stats.conditions;
    match node.kind_id().into() {
        AMPAMP | PIPEPIPE => {
            if let Some(parent) = ancestors.parent(node) {
                csharp_count_unary_conditions(&parent, conds);
            }
        }
        ArgumentList => csharp_count_unary_conditions(node, conds),
        // tree-sitter-c-sharp `if_statement` / `while_statement` shape:
        // [`if`/`while`, `(`, condition, `)`, body, …]. The parens are
        // anonymous string children, NOT a wrapping
        // `parenthesized_expression` as in tree-sitter-java — so the
        // condition lives at child(2). Targeting child(1) (the literal
        // `(` token) was the #370 bug: every unary / bare-identifier
        // condition silently scored 0. See issue #370.
        IfStatement | WhileStatement => {
            if let Some(condition) = node.child(2) {
                csharp_count_condition(&condition, node, conds);
            }
        }
        // tree-sitter-c-sharp `do_statement` shape:
        // [`do`, body, `while`, `(`, condition, `)`, `;`]. The
        // condition lives at child(4), not child(3) (which is the
        // literal `(` token). Targeting child(3) was the second half
        // of the #370 bug.
        DoStatement => {
            if let Some(condition) = node.child(4) {
                csharp_count_condition(&condition, node, conds);
            }
        }
        // `return value;` — child(1) is the value expression.
        ReturnStatement => csharp_inspect_child(node, 1, conds),
        // Child 2: declarator / assignment RHS, lambda body
        // (`params => body`).
        crate::Csharp::VariableDeclarator
        | crate::Csharp::VariableDeclarator2
        | AssignmentExpression
        | LambdaExpression => csharp_inspect_child(node, 2, conds),
        ConditionalExpression => csharp_walk_conditional(node, stats),
        ForStatement => csharp_walk_for_statement(node, stats),
        _ => {}
    }
}

// `cond ? a : b`, addressed by the grammar's `condition` /
// `consequence` / `alternative` fields rather than by index (#1181 — a
// comment between a token and its operand shifted every positional
// read). The cond-classifier match is shared with
// `csharp_walk_for_conditions`'s `if`/`while`/`do` arms via
// `csharp_count_condition`; the two branch slots go straight to
// `csharp_inspect_container`, so a parenthesised or `!`-prefixed branch
// contributes one condition just like a bare
// invocation/identifier/boolean would.
fn csharp_walk_conditional(node: &Node, stats: &mut Stats) {
    let conds = &mut stats.conditions;
    // By grammar FIELD, not index — see `java_walk_ternary` for why the
    // positional form dropped a negated branch operand behind a comment
    // (#1181).
    if let Some(condition) = node.child_by_field_name("condition") {
        csharp_count_condition(&condition, node, conds);
    }
    for field in ["consequence", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            csharp_inspect_container(&branch, node, conds);
        }
    }
}

// Counts unary / single-token conditions inside `for` statements. The
// C# grammar exposes the loop condition via the named `condition` field
// on `for_statement`, so we look it up by name rather than positional
// index. Comparison-operator conditions like `i < n` are still counted
// by the standard `GT | LT | ...` arms — this only fires when the
// condition is a bare identifier, invocation, boolean literal,
// parenthesised expression, or `!`-prefixed unary expression.
fn csharp_walk_for_statement(node: &Node, stats: &mut Stats) {
    if let Some(condition) = node.child_by_field_name("condition") {
        csharp_count_condition(&condition, node, &mut stats.conditions);
    }
}

impl Abc for CsharpCode {
    // See `impl Abc for JavaCode` for the short-circuit-chain rationale
    // and the cross-helper-exclusivity invariant.
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        if csharp_count_token_assignment(node, ancestors, stats) {
            return;
        }
        if csharp_count_token_branch(node, stats) {
            return;
        }
        if csharp_count_token_condition(node, ancestors, stats) {
            return;
        }
        csharp_walk_for_conditions(node, ancestors, stats);
    }
}

// C# mirror of `java_inspect_child` / `groovy_inspect_child`: passes
// `node.child(idx)` to `csharp_inspect_container`, which is a no-op on
// kinds other than `csharp_paren_expr_kinds!()` / `!`-prefixed
// `csharp_prefix_unary_expr_kinds!()`.
fn csharp_inspect_child(node: &Node, idx: usize, conditions: &mut f64) {
    if let Some(child) = node.child(idx) {
        csharp_inspect_container(&child, node, conditions);
    }
}

fn csharp_count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
    let kind = condition.kind_id().into();
    if matches!(kind, csharp_bool_terminal_kinds!()) {
        *conditions += 1.;
    } else if matches!(kind, csharp_paren_expr_kinds!())
        || matches!(kind, csharp_prefix_unary_expr_kinds!())
    {
        csharp_inspect_container(condition, parent, conditions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_each_node_with_chain;

    /// The predicate climbs two hops — declarator, then declaration —
    /// and each hop fails closed when the chain runs out. The pinned
    /// grammar never puts a `variable_declarator` anywhere but under a
    /// `variable_declaration`, so on a real chain the second hop can
    /// only succeed; a truncated `Ancestors::known` chain is the one
    /// way to reach its `false` arm, and the walker does hand out short
    /// chains at the root. Pinning it keeps a future "simplify the
    /// climb" from turning a missing ancestor into a counted `const`.
    #[test]
    fn const_predicate_fails_closed_on_a_truncated_chain() {
        let source = b"class A { const int x = 1; }";
        let mut seen = 0;
        for_each_node_with_chain::<CsharpCode>(source, |node, chain| {
            if node.kind() != "=" {
                return;
            }
            let Some(declarator) = chain.last() else {
                return;
            };
            if declarator.kind_id() != Csharp::VariableDeclarator as u16 {
                return;
            }
            seen += 1;
            assert!(
                csharp_eq_initializes_const_binding(node, Ancestors::known(chain)),
                "the full chain reaches the `const` declaration"
            );
            let declarator_only = &chain[chain.len() - 1..];
            assert!(
                !csharp_eq_initializes_const_binding(node, Ancestors::known(declarator_only)),
                "a chain that ends at the declarator must not read as `const`"
            );
            assert!(
                !csharp_eq_initializes_const_binding(node, Ancestors::known(&[])),
                "an empty chain must not read as `const`"
            );
        });
        assert_eq!(seen, 1, "fixture must carry exactly one declarator `=`");
    }
}
