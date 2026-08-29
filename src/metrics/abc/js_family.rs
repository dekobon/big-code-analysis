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
    (
        $Lang:ident,
        $inspect:ident,
        $count:ident,
        $count_condition:ident,
        $walk_ternary:ident,
        $walk_for:ident,
        $terminals:path
    ) => {
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
        // The condition goes through `$count_condition`, whose top-level
        // terminal check is what stops a bare `a ? … : …` scoring zero.
        // Branch operands get no such check: an unnegated branch is
        // type-free and contributes nothing, which is what keeps
        // `(a > 0) ? b : -b` at 2 (the `?` and the `>`).
        fn $walk_ternary(node: &Node, conditions: &mut f64) {
            if let Some(condition) = node.child_by_field_name("condition") {
                $count_condition(&condition, node, conditions);
            }
            for field in ["consequence", "alternative"] {
                if let Some(branch) = node.child_by_field_name(field) {
                    $inspect(&branch, node, conditions);
                }
            }
        }

        // Classifies one condition-slot expression: a bare boolean
        // terminal counts directly, anything else is offered to the
        // `(...)` / `!...` unwrap chain. Shared by the ternary condition
        // slot and the `for` header's condition slot — the two places a
        // JS-family condition arrives *unwrapped*. `if` / `while` / `do`
        // hand `$inspect` a `parenthesized_expression` that supplies the
        // unwrap step itself, so they must not take the top-level
        // terminal count.
        fn $count_condition(condition: &Node, parent: &Node, conditions: &mut f64) {
            if matches!(condition.kind_id().into(), $terminals!()) {
                *conditions += 1.;
            } else {
                $inspect(condition, parent, conditions);
            }
        }

        // Phase-2B (issues #403 / #1276): the `for (init; condition;
        // update)` condition slot is a Fitzpatrick Rule 9 unary
        // condition, exactly like the `if` / `while` slots the
        // dispatchers already walk. Without this the JS family scored
        // `for (; a; ) {}` zero where `if (a) {}` scores one, and
        // `$inspect`'s `ForStatement` boolean-context seed was
        // unreachable. Comparison-shaped conditions (`i < n`) were never
        // affected — the `<` token arm counts those.
        //
        // Addressed by grammar FIELD: the JS `for_statement` marks the
        // condition field on *both* the expression and the `;` that
        // terminates it, and the initializer's own shape (an
        // `empty_statement`, an `expression_statement`, or a
        // `lexical_declaration` that swallows its `;`) moves every child
        // index. `child_by_field_name` returns the first such child,
        // which is the expression.
        //
        // An empty condition (`for (;;)`) fills the slot with an
        // `empty_statement` rather than leaving it absent — the one
        // family where it does. That kind is neither a terminal nor a
        // paren / `!` wrapper, so it falls through and counts zero,
        // agreeing with every other language; see the `Stats` doc
        // comment's cross-language empty-`for`-condition policy.
        fn $walk_for(node: &Node, conditions: &mut f64) {
            if let Some(condition) = node.child_by_field_name("condition") {
                $count_condition(&condition, node, conditions);
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
    typescript_count_condition,
    typescript_walk_ternary,
    typescript_walk_for_statement,
    typescript_bool_terminal_kinds
);

impl_js_family_unary_walker!(
    Tsx,
    tsx_inspect_container,
    tsx_count_unary_conditions,
    tsx_count_condition,
    tsx_walk_ternary,
    tsx_walk_for_statement,
    tsx_bool_terminal_kinds
);

impl_js_family_unary_walker!(
    Javascript,
    javascript_inspect_container,
    javascript_count_unary_conditions,
    javascript_count_condition,
    javascript_walk_ternary,
    javascript_walk_for_statement,
    javascript_bool_terminal_kinds
);

impl_js_family_unary_walker!(
    Mozjs,
    mozjs_inspect_container,
    mozjs_count_unary_conditions,
    mozjs_count_condition,
    mozjs_walk_ternary,
    mozjs_walk_for_statement,
    mozjs_bool_terminal_kinds
);

// Generates the per-language predicate deciding whether an `=` token
// initialises a `const` binding, whose initializer is part of the
// declaration and therefore not an ABC assignment (Fitzpatrick).
//
// The decision is structural: the `=` must belong to a
// `variable_declarator` whose parent is a `lexical_declaration` whose
// `kind` field is the `const` keyword. "Belong to" admits the
// destructuring-pattern layers a default's `=` sits under —
// `const {a = 1} = o`, `const [b = 2] = xs`, the nested
// `const {p: {q = 3} = {}} = o` — because a pattern default declares
// the binding's value exactly as `const a = 1` does; the pre-#1277 stack
// suppressed those too, and counting them would make `const {a = 1} = o`
// score where `const a = o.a ?? 1` does not. The climb stops at the
// first kind outside that pattern set, so an `=` inside the initializer
// *value* (`const x = (o.p = 1)`, `const x = a || (b = 1)`) is an
// `assignment_expression` and counts — a real assignment the stack
// wrongly blanket-suppressed. Every other `=` counts too — a `let` /
// `var` initializer, a class `field_definition`, and a parameter
// default (`g(p = 2)`, `g({q = 3} = {})`), whose climb ends at
// `formal_parameters` rather than a declarator. `for (const x of xs)`
// has no `=` at all, and `for (const [k = 1] of xs)` climbs to
// `for_in_statement`, so both keep their pre-#1277 answer.
//
// This replaces the pre-#1277 declaration stack, which pushed a sentinel
// on `lexical_declaration` / `variable_declaration` and cleared it only
// on a `SEMI` token. JavaScript's automatic semicolon insertion makes the
// terminator optional, so a `const` written without one never popped its
// sentinel and suppressed every later `=` until the next `;` — the same
// design failure #455 root-caused for Kotlin, whose grammar emits no
// `SEMI` at all. TypeScript's `x as const` reached the sentinel from the
// other side, promoting a live `let` slot to `Const`.
//
// Every hop reads the ancestor chain the walk already descended through,
// so the climb is O(pattern nesting) and `Node::parent`'s O(depth) is
// never paid (#1096, #1122). The keyword is read through the `kind`
// field rather than by scanning the declaration's children: that scan
// ran once per declarator and walked every sibling declarator, so a
// `let` list of N declarators cost O(N²) — 6 s for 5 000 of them on a
// debug build, against 0.04 s for the same list under `var`.
macro_rules! impl_js_family_const_binding {
    ($Lang:ident, $name:ident) => {
        fn $name<'a>(eq_node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
            use $Lang::*;

            let mut climb = ancestors.iter(eq_node).map(|(ancestor, _)| ancestor);
            let mut owner = climb.next();
            while let Some(node) = owner
                && matches!(
                    node.kind_id().into(),
                    ObjectPattern
                        | ArrayPattern
                        | PairPattern
                        | ObjectAssignmentPattern
                        | AssignmentPattern
                        | RestPattern
                )
            {
                owner = climb.next();
            }
            if !owner.is_some_and(|node| node.kind_id() == VariableDeclarator) {
                return false;
            }
            climb.next().is_some_and(|declaration| {
                declaration.kind_id() == LexicalDeclaration
                    && declaration
                        .child_by_field_name("kind")
                        .is_some_and(|keyword| keyword.kind_id() == Const)
            })
        }
    };
}

impl_js_family_const_binding!(Typescript, typescript_eq_initializes_const_binding);
impl_js_family_const_binding!(Tsx, tsx_eq_initializes_const_binding);
impl_js_family_const_binding!(Javascript, javascript_eq_initializes_const_binding);
impl_js_family_const_binding!(Mozjs, mozjs_eq_initializes_const_binding);

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
// Declaration initializers: a plain `=` counts as an assignment unless
// `$const_binding` finds it initialising a `const` binding (a compile-time
// constant, so not a mutable assignment). That is a structural question
// about the `=` token's parent chain, not a stateful one — see
// `impl_js_family_const_binding!` above for why the pre-#1277 sentinel
// stack could not answer it. `let` and `var` initializers still count.
// Augmented assignments (`+=`) and update expressions (`++`, `--`) always
// count.
macro_rules! ts_abc_compute {
    (
        $lang:ident,
        $count_unary:path,
        $inspect_container:path,
        $walk_ternary:path,
        $walk_for:path,
        $const_binding:path
    ) => {
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
                // Plain `=` outside `const` declarations is an assignment
                // (issue #1277).
                EQ if !$const_binding(node, ancestors) => {
                    stats.assignments += 1.;
                }
                // Function invocation and object construction count as
                // branches. Member calls and chained calls all surface
                // as `CallExpression`.
                CallExpression | NewExpression => {
                    stats.branches += 1.;
                }
                // Comparison and equality operators, `??`, `instanceof`,
                // `else`, `case`, `catch`, `try`. The `default` arm of a
                // `switch` is intentionally NOT a
                // condition: it is the unconditional fallthrough, so
                // cyclomatic counts only the `Case` arms (issue #469).
                // Both the statement (`default:`) and arrow
                // (`default ->`) forms emit the same `Default` token, so
                // omitting it here covers both.
                EQEQ | EQEQEQ | BANGEQ | BANGEQEQ | LTEQ | GTEQ | QMARKQMARK | Instanceof
                | Else | Case | Try | Catch => {
                    stats.conditions += 1.;
                }
                // A bare `?` opens a ternary in only one of the eleven
                // productions that emit it. The other ten are type
                // syntax carrying no runtime decision:
                // `optional_parameter` (`f(x?: T)`);
                // `property_signature`, `method_signature` and
                // `abstract_method_signature`
                // (`interface I { a?: T; m?(): void }`);
                // `public_field_definition` (`class K { f?: T }`) and
                // `method_definition` (`class K { m?() {} }`);
                // `optional_type` and `optional_tuple_parameter`
                // (`[number, string?]`); `flow_maybe_type`; and
                // `conditional_type` (`T extends U ? X : Y`) — every one
                // of which scored a condition before #1275.
                //
                // Allowlist polarity, deliberately — unlike the C#
                // denylist in `csharp_count_token_condition`. Ten type-
                // syntax parents against one decision parent is not a
                // set worth restating, and TypeScript keeps growing type
                // syntax, so the safe failure here is the closed one: a
                // production the grammar adds later stops counting
                // rather than starting to.
                //
                // `conditional_type` is an explicit decision, not an
                // omission: `T extends U ? X : Y` is resolved by the
                // type checker and erased before runtime, so it is no
                // more a branch than the `<` / `>` excluded below.
                //
                // `?.` never reaches this arm — it is the distinct
                // `QMARKDOT` token, inside an `optional_chain` node —
                // and neither does `??` / `??=` (`QMARKQMARK` /
                // `QMARKQMARKEQ`) nor a mapped type's `?:`
                // (`QMARKCOLON`).
                QMARK if ancestors.parent_has_kind(node, TernaryExpression as u16) => {
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
                // `for (init; cond; update)` — the condition slot, read
                // by grammar field (issue #1276). `for (;;)` fills the
                // slot with an `empty_statement` and counts nothing.
                ForStatement => {
                    $walk_for(node, &mut stats.conditions);
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
        typescript_walk_ternary,
        typescript_walk_for_statement,
        typescript_eq_initializes_const_binding
    );
}

impl Abc for TsxCode {
    ts_abc_compute!(
        Tsx,
        tsx_count_unary_conditions,
        tsx_inspect_container,
        tsx_walk_ternary,
        tsx_walk_for_statement,
        tsx_eq_initializes_const_binding
    );
}

// JavaScript / Mozjs share TypeScript's expression / statement
// vocabulary. The `js_abc_compute!` macro expands the same
// token-level Fitzpatrick rules as `ts_abc_compute!`, with two
// adjustments:
//
//   1. `LT` / `GT` are always comparison operators in plain JS — there
//      are no `TypeArguments` / `TypeParameters` nodes to gate against.
//   2. JS runs the same `$const_binding` structural check so `const x = 5`
//      does not count the initializer `=` as an assignment. `let x = 5`
//      and `var x = 5` DO count their initializer `=` as an assignment —
//      only `const` suppresses, matching the TS impl above. This
//      deliberately deviates from a strict reading of Fitzpatrick's
//      "declaration initialiser is not an assignment" rule because
//      `let`/`var` bindings can be reassigned and the initial value is
//      the first assignment of the binding's lifetime.
macro_rules! js_abc_compute {
    (
        $lang:ident,
        $count_unary:path,
        $inspect_container:path,
        $walk_ternary:path,
        $walk_for:path,
        $const_binding:path
    ) => {
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
                // See the TS macro above: a `const` initializer is part of
                // the declaration, every other `=` is an assignment (#1277).
                EQ if !$const_binding(node, ancestors) => {
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
                // `for (init; cond; update)` — the condition slot, read
                // by grammar field (issue #1276). `for (;;)` fills the
                // slot with an `empty_statement` and counts nothing.
                ForStatement => {
                    $walk_for(node, &mut stats.conditions);
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
        javascript_walk_ternary,
        javascript_walk_for_statement,
        javascript_eq_initializes_const_binding
    );
}

impl Abc for MozjsCode {
    js_abc_compute!(
        Mozjs,
        mozjs_count_unary_conditions,
        mozjs_inspect_container,
        mozjs_walk_ternary,
        mozjs_walk_for_statement,
        mozjs_eq_initializes_const_binding
    );
}
