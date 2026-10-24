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

use super::{
    Abc, Stats, count_boolean_slot, count_each_operand, count_field_operands,
    count_negated_operand, last_operand, wrapped_operand,
};
use crate::*;

// JS / TS / TSX / Mozjs share an expression / statement vocabulary, and
// their grammars spell every kind below with the same name, so the slot
// rule is written once over `node.kind()` rather than once per enum
// (grammar-dispatch §1: TypeScript alone emits two ids for
// `parenthesized_expression` and for `unary_expression`).
//
// One step of the value peel (see `PeelStep`). A parenthesis, an await, a TS type
// cast (`x as T`, `x satisfies T`, `<T>x`) and a non-null assertion
// (`x!`) evaluate to their operand; a comma sequence to its last
// operand; an assignment to its `right` value, so `if (y = x > 1)` tests
// the comparison its own arm already counts. `!` is the one wrapper that
// proves its operand boolean. The other unary operators (`-x`,
// `typeof x`, `void x`) yield a value rather than wrapping a test, so the
// peel stops on them and the slot pays. Every operand is read by role
// rather than at child(1), where a comment may sit (`(/*c*/ b)`,
// `! /*c*/ b` — #1455).
fn js_family_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    match node.kind() {
        "parenthesized_expression"
        | "await_expression"
        | "as_expression"
        | "satisfies_expression"
        | "non_null_expression" => wrapped_operand(node).map(|o| (o, false)),
        "sequence_expression" | "type_assertion" => last_operand(node).map(|o| (o, false)),
        "assignment_expression" => node.child_by_field_name("right").map(|o| (o, false)),
        "unary_expression" => node
            .child_by_field_name("operator")
            .filter(|op| op.kind() == "!")
            .and_then(|_| node.child_by_field_name("argument"))
            .map(|o| (o, true)),
        _ => None,
    }
}

// Whether an arm of the `compute` macros below already charges `expr`
// (already peeled) as a condition: a ternary (its `?`), a comparison or
// `instanceof` (the `Comparison` row's owner, paid by its token), `??`
// (its token arm, pending `NullSafe`), or an `&&` / `||` chain (the
// `ShortCircuit` row's owner), whose operands each pay through
// `count_condition`. `in` is a relational operator no arm counts, so a
// slot holding one pays for it.
//
// Each grammar supplies its own, reading its rows, through
// `js_family_scores_itself!`; the helpers below take it as a parameter
// so the slot rule has this one home for all four.
type ScoresItself<'c> = &'c dyn Fn(&Node<'_>) -> bool;

// The row-keyed `ScoresItself` of one JS grammar. Neither owner gate
// reads ancestors, so the peeled descendant is asked with none.
macro_rules! js_family_scores_itself {
    ($roles:ident, $code:ident) => {
        &|expr: &Node| {
            use crate::roles::$roles::{comparison, short_circuit, ternary};
            let unknown = Ancestors::unknown();
            comparison::owns(expr, $code, unknown)
                || short_circuit::owns(expr, $code, unknown)
                || ternary::owns(expr, $code, unknown)
                // role-bypass: allow(Comparison, ShortCircuit) — the `??` owner, which the
                // `NullSafe` row of #1695 takes over; the rows are read above
                || (expr.kind() == "binary_expression"
                    && expr
                        .child_by_field_name("operator")
                        .is_some_and(|op| op.kind() == "??"))
        }
    };
}

// Scores one boolean slot — an `if` / `while` / `do` / `for` condition,
// a ternary condition, an operand of an `&&` / `||` chain (see
// `count_boolean_slot`). JavaScript is truthy-valued, so `if (-x)`,
// `if (this)`, `if (typeof x)` and `if (x + 1)` are each a decision, and
// each scored 0 while the slot paid only for a fixed list of terminal
// kinds (#1526).
fn count_condition(condition: &Node, scores: ScoresItself, conditions: &mut f64) {
    count_boolean_slot(condition, js_family_wrapper_operand, scores, conditions);
}

// An operand outside a boolean slot — a `return` value, a call
// argument, a ternary branch — scores only when a `!` proves it boolean
// (see `count_negated_operand`), which keeps `(a > 0) ? b : -b` at 2
// (the `?` and the `>`).
fn count_negated(operand: &Node, scores: ScoresItself, conditions: &mut f64) {
    count_negated_operand(operand, js_family_wrapper_operand, scores, conditions);
}

// Phase-2B (issues #403 / #1102): a ternary's condition is a boolean
// slot, and each branch operand a negated operand, exactly as
// `java_walk_ternary` counts them. Without this the JS family scored
// `a ? !b : !c` as 1 (the `?` token alone) against Java's 4. Slots are
// addressed by grammar FIELD rather than by child index, so a grammar
// re-order cannot silently retarget them.
fn js_family_walk_ternary(node: &Node, scores: ScoresItself, conditions: &mut f64) {
    if let Some(condition) = node.child_by_field_name("condition") {
        count_condition(&condition, scores, conditions);
    }
    for field in ["consequence", "alternative"] {
        if let Some(branch) = node.child_by_field_name(field) {
            count_negated(&branch, scores, conditions);
        }
    }
}

// Phase-2B (issues #403 / #1276): the `for (init; condition; update)`
// condition slot, exactly like the `if` / `while` slots. Without this
// the JS family scored `for (; a; ) {}` zero where `if (a) {}` scores
// one.
//
// Addressed by grammar FIELD: the JS `for_statement` marks the
// condition field on *both* the expression and the `;` that terminates
// it, and the initializer's own shape (an `empty_statement`, an
// `expression_statement`, or a `lexical_declaration` that swallows its
// `;`) moves every child index. `child_by_field_name` returns the first
// such child, which is the expression.
//
// An empty condition fills the slot with an `empty_statement` (`for
// (;;)`) or the bare `;` token rather than leaving it absent — the one
// family where it does. Neither is a value, so neither is a slot, which
// agrees with every other language; see the `Stats` doc comment's
// cross-language empty-`for`-condition policy.
fn js_family_walk_for(node: &Node, scores: ScoresItself, conditions: &mut f64) {
    if let Some(condition) = node
        .child_by_field_name("condition")
        .filter(|slot| slot.is_named() && slot.kind() != "empty_statement")
    {
        count_condition(&condition, scores, conditions);
    }
}

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
                    // role-bypass: allow(DeclKeyword) — which declaration keyword, not whether
                    // the token is one
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
// `IfStatement` / `WhileStatement` / `DoStatement` / `ForStatement`
// / ternary slots, which route through `js_family_count_condition`, and
// the `ReturnStatement` / `Arguments` operands, which route through
// `js_family_count_negated`.
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
        $roles:ident,
        $const_binding:path
    ) => {
        fn compute<'a>(
            node: &Node<'a>,
            code: &'a [u8],
            ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
        ) {
            use crate::roles::$roles::{comparison, short_circuit, ternary};
            use $lang::*;

            let scores: ScoresItself = js_family_scores_itself!($roles, code);
            let kind = node.kind_id();
            match kind.into() {
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
                // `??`, `else`, `case`, `catch`, `try`; the comparison
                // and equality operators and `instanceof` are the
                // `Comparison` row's, below. The `default` arm of a
                // `switch` is intentionally NOT a
                // condition: it is the unconditional fallthrough, so
                // cyclomatic counts only the `Case` arms (issue #469).
                // Both the statement (`default:`) and arrow
                // (`default ->`) forms emit the same `Default` token, so
                // omitting it here covers both.
                QMARKQMARK | Else | Case | Try | Catch => {
                    stats.conditions += 1.;
                }
                // The `Ternary` row's `?`, gated by the row to the
                // ternary: a bare `?` opens one in only one of the eleven
                // productions that emit it, the other ten being type
                // syntax carrying no runtime decision (#1275) — the row's
                // gate says which, and why `conditional_type` is an
                // explicit decision rather than an omission. `?.` never
                // reaches this arm (the distinct `QMARKDOT` token inside
                // an `optional_chain`), nor does `??` / `??=`
                // (`QMARKQMARK` / `QMARKQMARKEQ`) nor a mapped type's `?:`
                // (`QMARKCOLON`).
                _ if ternary::pays_at_leaf_kind(kind, node, code, ancestors) => {
                    stats.conditions += 1.;
                }
                // The comparison tokens, the `Comparison` row's payers
                // (`roles/js_family.rs`): the four equalities, `<=` /
                // `>=`, `instanceof`, and `<` / `>` gated to a
                // `binary_expression` parent — the allowlist polarity C /
                // C++ / Rust / Go / Java use. The previous denylist named
                // `type_arguments` and `type_parameters` only, so every
                // JSX tag delimiter scored a condition: a
                // `jsx_opening_element` contributes `<` and `>`, a
                // `jsx_closing_element` a `>` (its `</` is one token),
                // and a `jsx_self_closing_element` a `<` (its `/>` is
                // one token) — six conditions for
                // `<div className="a"><span>hi</span></div>` with no
                // decision in it (#1297). A `grammar.json` sweep of
                // tree-sitter-typescript 0.23.2 finds a bare `<` / `>`
                // in exactly six productions — `binary_expression`, the
                // three JSX ones, `type_arguments` and
                // `type_parameters` — so the allowlist is the inverse of
                // a five-entry denylist today and, unlike it, needs no
                // revisiting when the grammar grows a seventh
                // (`.claude/rules/grammar-dispatch.md` §1). The shifts
                // (`<<`, `>>`, `>>>`) are their own tokens.
                _ if comparison::pays_at_leaf_kind(kind, node, code, ancestors) => {
                    stats.conditions += 1.;
                }
                // Fitzpatrick Rule 9: each operand of a `&&` / `||`
                // chain is one condition (issue #403); the `ShortCircuit`
                // row is read at its leaf only to find the chain, the
                // token's parent. `a && b || c` is a left-nested chain of
                // `binary_expression`s, so an operand that is itself a
                // chain is paid by its own operator's visit.
                _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
                    if let Some(chain) = ancestors.parent(node) {
                        count_field_operands(
                            &chain,
                            |operand, conds| count_condition(operand, scores, conds),
                            &mut stats.conditions,
                        );
                    }
                }
                // Phase-2B (issue #403): condition slots. JS / TS
                // wrap `if (...)` / `while (...)` / `do {…} while
                // (...)` in `parenthesized_expression`, which
                // `js_family_wrapper_operand` peels before the slot
                // pays (`if (true)` counts 1).
                // Read by grammar field, not index: a comment before
                // the slot (`if /*c*/ (b)`) shifted every positional
                // read onto it (#1455).
                IfStatement | WhileStatement | DoStatement => {
                    if let Some(cond) = node.child_by_field_name("condition") {
                        count_condition(&cond, scores, &mut stats.conditions);
                    }
                }
                // `return value;` names no field; the value is its only
                // operand. The bare `return;` form has none.
                ReturnStatement => {
                    if let Some(value) = wrapped_operand(node) {
                        count_negated(&value, scores, &mut stats.conditions);
                    }
                }
                // Method-argument walker for `f(!a, !b)`.
                Arguments => {
                    count_each_operand(
                        node,
                        |operand, conds| count_negated(operand, scores, conds),
                        &mut stats.conditions,
                    );
                }
                // `a ? !b : !c` — the `ternary_expression` the `Ternary`
                // row owns: its `?` is already paid by the row's arm
                // above; this walks the three operand slots (issue #1102).
                _ if ternary::owns_kind(kind, node, code, ancestors) => {
                    js_family_walk_ternary(node, scores, &mut stats.conditions);
                }
                // `for (init; cond; update)` — the condition slot, read
                // by grammar field (issue #1276). `for (;;)` fills the
                // slot with an `empty_statement` and counts nothing.
                ForStatement => {
                    js_family_walk_for(node, scores, &mut stats.conditions);
                }
                _ => {}
            }
        }
    };
}

impl Abc for TypescriptCode {
    ts_abc_compute!(
        Typescript,
        typescript,
        typescript_eq_initializes_const_binding
    );
}

impl Abc for TsxCode {
    ts_abc_compute!(Tsx, tsx, tsx_eq_initializes_const_binding);
}

// JavaScript / Mozjs share TypeScript's expression / statement
// vocabulary. The `js_abc_compute!` macro expands the same
// token-level Fitzpatrick rules as `ts_abc_compute!`, with two
// adjustments:
//
//   1. `LT` / `GT` take the same `binary_expression` gate, against a
//      shorter list of non-comparison producers: plain JS has no
//      `TypeArguments` / `TypeParameters`, but it does have the three
//      JSX productions (#1297).
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
        $roles:ident,
        $const_binding:path
    ) => {
        fn compute<'a>(
            node: &Node<'a>,
            code: &'a [u8],
            ancestors: Ancestors<'a, '_>,
            stats: &mut Stats,
        ) {
            use crate::roles::$roles::{comparison, short_circuit, ternary};
            use $lang::*;

            let scores: ScoresItself = js_family_scores_itself!($roles, code);
            let kind = node.kind_id();
            match kind.into() {
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
                QMARKQMARK | Else | Case | Try | Catch => {
                    stats.conditions += 1.;
                }
                // The `Ternary` row's `?`: plain JS spells the bare token
                // nowhere else, so the row pays it ungated — a recovered
                // `?` with its ternary lost still pays, as it always did.
                _ if ternary::pays_at_leaf_kind(kind, node, code, ancestors) => {
                    stats.conditions += 1.;
                }
                // The comparison tokens, the `Comparison` row's payers
                // (`roles/js_family.rs`). Plain JS has no generics, but
                // it does have JSX, and both grammars here parse it
                // unconditionally — a `.js` file returning
                // `<div><span>hi</span></div>` scored six conditions from
                // tag delimiters alone (#1297). The `grammar.json` sweep
                // finds a bare `<` / `>` in exactly four productions in
                // tree-sitter-javascript 0.25.0 and in the vendored mozjs
                // fork — `binary_expression` plus the same three JSX ones
                // as TypeScript — so the row's parent gate is the TS one
                // with the two type-syntax productions absent. See that
                // arm for the polarity rationale.
                _ if comparison::pays_at_leaf_kind(kind, node, code, ancestors) => {
                    stats.conditions += 1.;
                }
                // Fitzpatrick Rule 9: each operand of a `&&` / `||`
                // chain is one condition (issue #403); the `ShortCircuit`
                // row is read at its leaf only to find the chain, the
                // token's parent. `a && b || c` is a left-nested chain of
                // `binary_expression`s, so an operand that is itself a
                // chain is paid by its own operator's visit.
                _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
                    if let Some(chain) = ancestors.parent(node) {
                        count_field_operands(
                            &chain,
                            |operand, conds| count_condition(operand, scores, conds),
                            &mut stats.conditions,
                        );
                    }
                }
                // Phase-2B (issue #403): condition slots. Same shape
                // as the TypeScript impl above — see that macro's
                // arm-block for why each slot is read by role.
                IfStatement | WhileStatement | DoStatement => {
                    if let Some(cond) = node.child_by_field_name("condition") {
                        count_condition(&cond, scores, &mut stats.conditions);
                    }
                }
                ReturnStatement => {
                    if let Some(value) = wrapped_operand(node) {
                        count_negated(&value, scores, &mut stats.conditions);
                    }
                }
                Arguments => {
                    count_each_operand(
                        node,
                        |operand, conds| count_negated(operand, scores, conds),
                        &mut stats.conditions,
                    );
                }
                // `a ? !b : !c` — the `ternary_expression` the `Ternary`
                // row owns: its `?` is already paid by the row's arm
                // above; this walks the three operand slots (issue #1102).
                _ if ternary::owns_kind(kind, node, code, ancestors) => {
                    js_family_walk_ternary(node, scores, &mut stats.conditions);
                }
                // `for (init; cond; update)` — the condition slot, read
                // by grammar field (issue #1276). `for (;;)` fills the
                // slot with an `empty_statement` and counts nothing.
                ForStatement => {
                    js_family_walk_for(node, scores, &mut stats.conditions);
                }
                _ => {}
            }
        }
    };
}

impl Abc for JavascriptCode {
    js_abc_compute!(
        Javascript,
        javascript,
        javascript_eq_initializes_const_binding
    );
}

impl Abc for MozjsCode {
    js_abc_compute!(Mozjs, mozjs, mozjs_eq_initializes_const_binding);
}
