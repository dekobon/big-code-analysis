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

use super::{Abc, DeclKind, Stats};
use crate::macros::{
    javascript_bool_terminal_kinds, mozjs_bool_terminal_kinds, tsx_bool_terminal_kinds,
    typescript_bool_terminal_kinds,
};
use crate::*;

// JS / TS / TSX / Mozjs share an expression / statement vocabulary;
// the helper macro below generates the per-language unary-conditional
// walker pair (Fitzpatrick Rule 9 / Listing 2; issue #403). Each
// `&&` / `||` token in the dispatcher routes through
// `<lang>_count_unary_conditions` and counts the immediate operands
// of the parent `binary_expression` once. Operands wrapped in `(…)` /
// `!…` are unwrapped via `<lang>_inspect_container`. Terminal-bool
// kinds include `Identifier`, the boolean literal tokens `True` /
// `False`, plus `CallExpression` / `NewExpression` (object
// construction in JS / TS) / `MemberExpression` / `SubscriptExpression`
// — every expression kind whose evaluated value is implicitly boolean
// in an `if` / `while` / ternary slot.
macro_rules! impl_js_family_unary_walker {
    ($Lang:ident, $inspect:ident, $count:ident, $walk_ternary:ident, $terminals:path) => {
        fn $inspect(container_node: &Node, parent: &Node, conditions: &mut f64) {
            use $Lang::*;

            let mut node = *container_node;
            let mut node_kind = node.kind_id().into();
            let parent_kind = parent.kind_id().into();
            let mut has_boolean_content = matches!(
                parent_kind,
                BinaryExpression | IfStatement | WhileStatement | DoStatement | ForStatement
            ) || (matches!(parent_kind, TernaryExpression)
                && parent
                    .child_by_field_name("condition")
                    .is_some_and(|condition| condition.id() == node.id()));

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

                let Some(child) = node.child(1) else { break };
                node = child;
                node_kind = node.kind_id().into();

                if matches!(node_kind, $terminals!()) {
                    if has_boolean_content {
                        *conditions += 1.;
                    }
                    break;
                }
            }
        }

        // Phase-2B (issues #403 / #1102): a ternary's condition and its
        // two branch operands are each a Fitzpatrick Rule 9 unary
        // condition, exactly as `java_walk_ternary` already counts them.
        // Without this the JS family scored `a ? !b : !c` as 1 (the `?`
        // token alone) against Java's 4, and `$inspect`'s
        // `TernaryExpression` boolean-context seed was unreachable.
        //
        // Slots are addressed by grammar FIELD rather than by child
        // index, so a grammar re-order cannot silently retarget them.
        // The condition needs its own terminal check because `$inspect`
        // only counts *after* unwrapping a `(...)` / `!...` layer, so a
        // bare `a ? … : …` would otherwise score zero. Branch operands
        // get no such check: an unnegated branch is type-free and
        // contributes nothing, which is what keeps `(a > 0) ? b : -b`
        // at 2 (the `?` and the `>`).
        fn $walk_ternary(node: &Node, conditions: &mut f64) {
            if let Some(condition) = node.child_by_field_name("condition") {
                if matches!(condition.kind_id().into(), $terminals!()) {
                    *conditions += 1.;
                } else {
                    $inspect(&condition, node, conditions);
                }
            }
            for field in ["consequence", "alternative"] {
                if let Some(branch) = node.child_by_field_name(field) {
                    $inspect(&branch, node, conditions);
                }
            }
        }

        fn $count(list_node: &Node, conditions: &mut f64) {
            use $Lang::*;

            let list_kind = list_node.kind_id().into();
            let mut cursor = list_node.cursor();

            if cursor.goto_first_child() {
                loop {
                    let node = cursor.node();
                    let node_kind = node.kind_id().into();

                    if matches!(node_kind, $terminals!()) && matches!(list_kind, BinaryExpression) {
                        *conditions += 1.;
                    } else if node.is_named() {
                        $inspect(&node, list_node, conditions);
                    }

                    if !cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
        }
    };
}

impl_js_family_unary_walker!(
    Typescript,
    typescript_inspect_container,
    typescript_count_unary_conditions,
    typescript_walk_ternary,
    typescript_bool_terminal_kinds
);

impl_js_family_unary_walker!(
    Tsx,
    tsx_inspect_container,
    tsx_count_unary_conditions,
    tsx_walk_ternary,
    tsx_bool_terminal_kinds
);

impl_js_family_unary_walker!(
    Javascript,
    javascript_inspect_container,
    javascript_count_unary_conditions,
    javascript_walk_ternary,
    javascript_bool_terminal_kinds
);

impl_js_family_unary_walker!(
    Mozjs,
    mozjs_inspect_container,
    mozjs_count_unary_conditions,
    mozjs_walk_ternary,
    mozjs_bool_terminal_kinds
);

// TypeScript / TSX share the same expression / statement vocabulary;
// the `ts_abc_compute!` macro expands the same token-level
// Fitzpatrick rules for both. Conditions capture every comparison and
// control-flow arm (the original token-level set), plus Phase-2 walker
// arms for `&&` / `||` operand counting and the
// `IfStatement` / `WhileStatement` / `DoStatement` / `ReturnStatement`
// / `Arguments` slots — each of those arms routes through the
// language's `$inspect_container` (paren / unary unwrap) and
// `$count_unary` (operand walker) helpers generated by
// `impl_js_family_unary_walker!`.
//
// Declaration sentinel: `lexical_declaration` and `variable_declaration`
// push a `Var` sentinel that suppresses counting the initializer `=` as
// an assignment. The `Const` token promotes to `Const` (compile-time
// constant — initializer is not a mutable assignment). `let` and `var`
// keep the `Var` slot. Augmented assignments (`+=`) and update
// expressions (`++`, `--`) always count.
macro_rules! ts_abc_compute {
    ($lang:ident, $count_unary:path, $inspect_container:path, $walk_ternary:path) => {
        fn compute<'a>(
            node: &Node<'a>,
            _code: &'a [u8],
            ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
        ) {
            use $lang::*;

            match node.kind_id().into() {
                // Augmented assignments and pre/post increment/decrement
                // always count.
                PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ | STARSTAREQ | AMPEQ | PIPEEQ
                | CARETEQ | LTLTEQ | GTGTEQ | GTGTGTEQ | AMPAMPEQ | PIPEPIPEEQ | QMARKQMARKEQ
                | PLUSPLUS | DASHDASH => {
                    stats.assignments += 1.;
                }
                // Variable declarations push a `Var` sentinel; the `Const`
                // keyword promotes the top to `Const` so the initializer
                // `=` is treated as a constant binding.
                LexicalDeclaration | VariableDeclaration => {
                    stats.declaration.push(DeclKind::Var);
                }
                Const => {
                    if let Some(DeclKind::Var) = stats.declaration.last() {
                        stats.declaration.push(DeclKind::Const);
                    }
                }
                SEMI => {
                    if let Some(DeclKind::Const | DeclKind::Var) = stats.declaration.last() {
                        stats.declaration.clear();
                    }
                }
                // Plain `=` outside `const` declarations is an assignment.
                EQ if !matches!(stats.declaration.last(), Some(DeclKind::Const)) => {
                    stats.assignments += 1.;
                }
                // Function invocation and object construction count as
                // branches. Member calls and chained calls all surface
                // as `CallExpression`.
                CallExpression | NewExpression => {
                    stats.branches += 1.;
                }
                // Comparison and equality operators, ternary `?`, `??`,
                // `instanceof`, `else`, `case`, `catch`, `try`. The
                // `default` arm of a `switch` is intentionally NOT a
                // condition: it is the unconditional fallthrough, so
                // cyclomatic counts only the `Case` arms (issue #469).
                // Both the statement (`default:`) and arrow
                // (`default ->`) forms emit the same `Default` token, so
                // omitting it here covers both.
                EQEQ | EQEQEQ | BANGEQ | BANGEQEQ | LTEQ | GTEQ | QMARK | QMARKQMARK
                | Instanceof | Else | Case | Try | Catch => {
                    stats.conditions += 1.;
                }
                // `<` and `>` may also delimit type arguments / type
                // parameters (`Array<number>`, `class Foo<T> {}`); skip
                // those, count only comparison usage.
                GT | LT
                    if ancestors.parent(node).is_some_and(|p| {
                        !matches!(p.kind_id().into(), TypeArguments | TypeParameters)
                    }) =>
                {
                    stats.conditions += 1.;
                }
                // Fitzpatrick Rule 9: each operand of a `&&` / `||`
                // chain is one condition (issue #403).
                AMPAMP | PIPEPIPE => {
                    if let Some(parent) = ancestors.parent(node) {
                        $count_unary(&parent, &mut stats.conditions);
                    }
                }
                // Phase-2B (issue #403): condition slots. JS / TS
                // wrap `if (...)` / `while (...)` / `do {…} while
                // (...)` in `parenthesized_expression`, so
                // `<lang>_inspect_container`'s paren-unwrap handles
                // the boolean-literal case (`if (true)` counts 1).
                // The condition sits at child(1) for if and while.
                // For `do_statement`, the condition is at child(3)
                // (children: `do`(0), body(1), `while`(2),
                // parenthesized condition(3), `;`(4)).
                IfStatement | WhileStatement => {
                    if let Some(cond) = node.child(1) {
                        $inspect_container(&cond, node, &mut stats.conditions);
                    }
                }
                DoStatement => {
                    // children: `do`(0), body(1), `while`(2),
                    // parenthesized condition(3), `;`(4).
                    if let Some(cond) = node.child(3) {
                        $inspect_container(&cond, node, &mut stats.conditions);
                    }
                }
                // `return value;` — value at child(1). The bare
                // `return;` (no value) form has no child(1).
                ReturnStatement => {
                    if let Some(value) = node.child(1) {
                        $inspect_container(&value, node, &mut stats.conditions);
                    }
                }
                // Method-argument walker for `f(!a, !b)`.
                Arguments => {
                    $count_unary(node, &mut stats.conditions);
                }
                // `a ? !b : !c` — the ternary's own `?` token is
                // already counted by the condition arm above; this
                // walks the three operand slots (issue #1102).
                TernaryExpression => {
                    $walk_ternary(node, &mut stats.conditions);
                }
                _ => {}
            }
        }
    };
}

impl Abc for TypescriptCode {
    ts_abc_compute!(
        Typescript,
        typescript_count_unary_conditions,
        typescript_inspect_container,
        typescript_walk_ternary
    );
}

impl Abc for TsxCode {
    ts_abc_compute!(
        Tsx,
        tsx_count_unary_conditions,
        tsx_inspect_container,
        tsx_walk_ternary
    );
}

// JavaScript / Mozjs share TypeScript's expression / statement
// vocabulary. The `js_abc_compute!` macro expands the same
// token-level Fitzpatrick rules as `ts_abc_compute!`, with two
// adjustments:
//
//   1. `LT` / `GT` are always comparison operators in plain JS — there
//      are no `TypeArguments` / `TypeParameters` nodes to gate against.
//   2. JS retains the same `LexicalDeclaration` / `VariableDeclaration`
//      sentinel handling so `const x = 5` does not double-count the
//      initializer `=` as an assignment. `let x = 5` and `var x = 5`
//      DO count their initializer `=` as an assignment — only `const`
//      suppresses, matching the TS impl above. This deliberately
//      deviates from a strict reading of Fitzpatrick's "declaration
//      initialiser is not an assignment" rule because `let`/`var`
//      bindings can be reassigned and the initial value is the first
//      assignment of the binding's lifetime.
macro_rules! js_abc_compute {
    ($lang:ident, $count_unary:path, $inspect_container:path, $walk_ternary:path) => {
        fn compute<'a>(
            node: &Node<'a>,
            _code: &'a [u8],
            ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
        ) {
            use $lang::*;

            match node.kind_id().into() {
                PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ | STARSTAREQ | AMPEQ | PIPEEQ
                | CARETEQ | LTLTEQ | GTGTEQ | GTGTGTEQ | AMPAMPEQ | PIPEPIPEEQ | QMARKQMARKEQ
                | PLUSPLUS | DASHDASH => {
                    stats.assignments += 1.;
                }
                LexicalDeclaration | VariableDeclaration => {
                    stats.declaration.push(DeclKind::Var);
                }
                Const => {
                    if let Some(DeclKind::Var) = stats.declaration.last() {
                        stats.declaration.push(DeclKind::Const);
                    }
                }
                SEMI => {
                    if let Some(DeclKind::Const | DeclKind::Var) = stats.declaration.last() {
                        stats.declaration.clear();
                    }
                }
                EQ if !matches!(stats.declaration.last(), Some(DeclKind::Const)) => {
                    stats.assignments += 1.;
                }
                CallExpression | NewExpression => {
                    stats.branches += 1.;
                }
                // The `default` arm is the unconditional fallthrough and
                // is excluded, mirroring cyclomatic's `Case`-only count
                // (issue #469); see the TS macro above for the rationale.
                EQEQ | EQEQEQ | BANGEQ | BANGEQEQ | LTEQ | GTEQ | LT | GT | QMARK | QMARKQMARK
                | Instanceof | Else | Case | Try | Catch => {
                    stats.conditions += 1.;
                }
                // Fitzpatrick Rule 9: each operand of a `&&` / `||`
                // chain is one condition (issue #403).
                AMPAMP | PIPEPIPE => {
                    if let Some(parent) = ancestors.parent(node) {
                        $count_unary(&parent, &mut stats.conditions);
                    }
                }
                // Phase-2B (issue #403): condition slots. Same shape
                // as the TypeScript impl above — see that macro's
                // arm-block for the per-child-index rationale.
                IfStatement | WhileStatement => {
                    if let Some(cond) = node.child(1) {
                        $inspect_container(&cond, node, &mut stats.conditions);
                    }
                }
                DoStatement => {
                    // children: `do`(0), body(1), `while`(2),
                    // parenthesized condition(3), `;`(4).
                    if let Some(cond) = node.child(3) {
                        $inspect_container(&cond, node, &mut stats.conditions);
                    }
                }
                ReturnStatement => {
                    if let Some(value) = node.child(1) {
                        $inspect_container(&value, node, &mut stats.conditions);
                    }
                }
                Arguments => {
                    $count_unary(node, &mut stats.conditions);
                }
                // `a ? !b : !c` — the ternary's own `?` token is
                // already counted by the condition arm above; this
                // walks the three operand slots (issue #1102).
                TernaryExpression => {
                    $walk_ternary(node, &mut stats.conditions);
                }
                _ => {}
            }
        }
    };
}

impl Abc for JavascriptCode {
    js_abc_compute!(
        Javascript,
        javascript_count_unary_conditions,
        javascript_inspect_container,
        javascript_walk_ternary
    );
}

impl Abc for MozjsCode {
    js_abc_compute!(
        Mozjs,
        mozjs_count_unary_conditions,
        mozjs_inspect_container,
        mozjs_walk_ternary
    );
}
