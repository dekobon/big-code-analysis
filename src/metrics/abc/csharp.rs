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
    count_negated_operand, last_operand, peel, wrapped_operand,
};
use crate::macros::{csharp_paren_expr_kinds, csharp_prefix_unary_expr_kinds};
use crate::roles::csharp::{case_arm, comparison, guard, null_safe, short_circuit, ternary};
use crate::*;

// The operand a transparent wrapper wraps, and whether the wrapper
// itself proves that operand is boolean. `None` means the node is not a
// wrapper and the peel stops there.
//
// Only the prefix `!` proves booleanness: `!x` is boolean whatever `x`
// is, while parentheses and the null-forgiving postfix `x!` are
// type-preserving — `x!` is boolean exactly when the slot it sits in
// is — so they inherit the caller's verdict rather than setting it.
//
// `postfix_unary_expression` is the arm #1463 added, and the fifth
// instance of one class: a condition slot that recognises a wrapper
// kind list, and silently scores nothing for anything not on it. Before
// it, `if (b!)` scored zero conditions against a cyclomatic decision of
// one, while `if (b)` scored one — an asymmetry between two spellings
// of the same test, in the notation nullable-reference-types projects
// write everywhere. `!!` and `(b!)` nest through the same peel.
//
// The kind is one production for three operators (`++`, `--`, `!`), and
// only `!` is type-preserving. `b++` and `b--` are arithmetic, never a
// boolean slot's operand, so the peel declines rather than reaching a
// bare `identifier` and counting it — the same exclusion Groovy's
// `~a` / `+a` / `-a` and Kotlin's `-x` / `x++` take. Their tokens are
// already ABC *assignments* (`PLUSPLUS | DASHDASH`), so accepting them
// would also have scored one construct on two axes.
//
// No fixture pins that exclusion, deliberately (§6). `++` takes a
// numeric operand and yields one, so `if (i++)` and `i++ && b` are type
// errors the compiler rejects; only the grammar's over-permissiveness
// reaches the arm, and a test over input C# rejects would make that
// over-permissiveness the contract. Measured rather than reasoned:
// dropping the `is_child` guard entirely fails **none** of the 3,429
// library tests. The guard is here because the exclusion is right, not
// because anything observable depends on it.
//
// No double count for the `!` itself (§5): no C# arm counts a `BANG`
// token — it reaches `csharp_count_token_condition`'s `_ => return
// false` and then `csharp_walk_for_conditions`, which matches no token
// kind. `is_child` rather than an index read because the operator is
// the node's *last* child and an `extra` may sit before it
// (`b /*c*/ !`); the operand is `child(0)`, which no extra can precede
// because the node starts there.
//
// The paren and prefix arms read their operand with `wrapped_operand`.
// The C# grammar names nothing here — `parenthesized_expression`,
// `prefix_unary_expression` and `postfix_unary_expression` all carry an
// empty `fields` map in node-types.json — so the field read that
// Kotlin's and Groovy's equivalents use is unavailable, and the
// positional `child(1)` it replaced lost the operand to a comment:
// `if (( /*c*/ b))` and `if (! /*c*/ b)` scored zero (#1455).
//
// Every `?` below is infallible for well-formed C# — each wrapper is
// the operator token plus its operand, so `child(0)` and an operand
// both exist — and is spelled as an `Option` because `AGENTS.md` bans
// `expect` outside tests. Do not try to cover the `None` arms: only
// error recovery reaches them (`bool b = !;` parses to a one-child
// `prefix_unary_expression`, verified with `bca dump`), and that is
// invalid C#, so pinning its numbers would make the grammar's present
// over-permissiveness the contract (§6).
fn csharp_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    use Csharp::*;

    match node.kind_id().into() {
        // Parentheses, checked/unchecked and await preserve the value's condition.
        csharp_paren_expr_kinds!() => wrapped_operand(node).map(|o| (o, false)),
        CheckedExpression | AwaitExpression => wrapped_operand(node).map(|o| (o, false)),
        // `!expr` — the operand follows the operator token. Seven other
        // prefix operators (`++ -- + - ~ & ^`) share this kind, as does
        // the `*` of a pointer indirection the grammar aliases onto it;
        // none is a boolean slot's operand.
        csharp_prefix_unary_expr_kinds!()
            if node.child(0).is_some_and(|c| c.kind_id() == BANG as u16) =>
        {
            wrapped_operand(node).map(|o| (o, true))
        }
        // `expr!` — the null-forgiving operator. One kind id at the
        // pinned `=0.23.5`, no numbered aliases (§1).
        PostfixUnaryExpression if node.is_child(BANG as u16) => Some((node.child(0)?, false)),
        // `(bool)x` — type-preserving for the slot's purpose, so a cast
        // of a comparison pays once, through the comparison.
        CastExpression => node.child_by_field_name("value").map(|o| (o, false)),
        _ => None,
    }
}

// Whether an arm of `compute` already charges `expr` (already peeled) as
// a condition: an applied comparison or an `is` test (the `Comparison`
// row's owners, paid by their token), `??` (the `NullSafe` owner paid
// by its token — not a conditional access, which that row also owns:
// `!(a?.b)` is a negated operand the slot pays for, as it always did,
// beside the access's own condition), a ternary (its `?`), or an `&&`
// / `||` chain (the `ShortCircuit` row's owner), whose operands each
// pay through `csharp_count_condition`. No owner gate reads ancestors,
// so the peeled descendant is asked with none.
//
// An assignment is not peeled, because the `AssignmentExpression` arm
// already scores its right-hand side as a negated operand: peeling
// `if (y = !b)` down to `b` would pay the slot for the negation that arm
// counts. It scores itself exactly when its value does, as Java's does.
fn csharp_condition_scores_itself(expr: &Node, code: &[u8]) -> bool {
    use Csharp::*;

    let unknown = Ancestors::unknown();
    // role-bypass: allow(NullSafe) — which of the row's owners this consumer scores itself
    // by, told by the projected token: the coalescing, not the access
    let coalescing = null_safe::owns(expr, code, unknown)
        && null_safe::project(expr).is_some_and(|op| op.kind_id() == QMARKQMARK as u16);
    comparison::owns(expr, code, unknown)
        || short_circuit::owns(expr, code, unknown)
        || ternary::owns(expr, code, unknown)
        || coalescing
        || match expr.kind_id().into() {
            AssignmentExpression => expr.child_by_field_name("right").is_some_and(|right| {
                let (value, proves_boolean) = peel(&right, csharp_wrapper_operand);
                proves_boolean || csharp_condition_scores_itself(&value, code)
            }),
            _ => false,
        }
}

// Scores one boolean slot — an `if` / `while` / `do` / `for` condition,
// a ternary condition, a `when` / `catch … when` guard, an operand of an
// `&&` / `||` chain (see `count_boolean_slot`). A C# slot only admits a
// `bool`, and the terminal-kind list it used to pay for missed valid
// ones: `if ((y = b))`, `if (a & b)`, `if (a ^ b)`, `if (a | b)` and a
// boolean `switch` expression each scored 0 conditions against a
// cyclomatic decision of 1 (#1526). It also pays for an ill-typed
// `if (-x)`, which no valid program can tell apart (grammar-dispatch §6).
fn csharp_count_condition(condition: &Node, code: &[u8], conditions: &mut f64) {
    count_boolean_slot(
        condition,
        csharp_wrapper_operand,
        |expr| csharp_condition_scores_itself(expr, code),
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, a declarator or
// assignment value, a lambda body, a ternary branch — scores only when
// a `!` proves it boolean (see `count_negated_operand`).
fn csharp_count_negated(operand: &Node, code: &[u8], conditions: &mut f64) {
    count_negated_operand(
        operand,
        csharp_wrapper_operand,
        |expr| csharp_condition_scores_itself(expr, code),
        conditions,
    );
}

fn csharp_count_negated_slot(slot: Option<Node>, code: &[u8], conditions: &mut f64) {
    if let Some(operand) = slot {
        csharp_count_negated(&operand, code, conditions);
    }
}

// Each argument of a call is a negated operand.
fn csharp_count_arguments(arguments: &Node, code: &[u8], conditions: &mut f64) {
    count_each_operand(
        arguments,
        |argument, conds| csharp_count_argument(argument, code, conds),
        conditions,
    );
}

// tree-sitter-c-sharp wraps every argument in an `argument` node —
// `seq(optional(seq(field('name', …), ':')), optional('ref' | 'out' |
// 'in'), expression | declaration_expression)` — so the value is its
// last operand, after any named-argument label (`name: !a`) or
// passing-mode keyword. Handing the wrapper itself to the peel scored
// every argument zero, `G(!a)` included (#1537). The value is located
// by role rather than by index because both prefixes are optional
// (grammar-dispatch §3).
fn csharp_count_argument(argument: &Node, code: &[u8], conditions: &mut f64) {
    if let Some(value) = last_operand(argument) {
        csharp_count_negated(&value, code, conditions);
    }
}

// ABC token-level helpers for C#. Mirror of Java's helper layout with
// C#-specific deltas: every aliased kind id is matched via the
// `csharp_*_kinds!()` macros (lesson #2); `ObjectCreationExpression`
// joins `InvocationExpression*` as a branch; all six comparison tokens
// share one arm allowlisting a `binary_expression` parent, which excludes
// type syntax (#1275), the declared name of an operator overload (#1297,
// #1420) and a `relational_pattern`'s operator — the last because a C#
// pattern's operator belongs to the arm that owns it rather than scoring
// on its own (#1383);
// `ConditionalExpression` replaces Java's `TernaryExpression`;
// `for_statement` exposes its condition via the named `condition`
// field rather than positional index.

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
            // role-bypass: allow(Modifier) — which modifier, not whether the node is one
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
// constructor delegation in either of its two spellings.
// `ConstructorInitializer` is the `: base(…)` / `: this(…)` delegation on a
// constructor — a call by Fitzpatrick's rule, and the C# spelling of the
// shape Java and Groovy count (#1279). Unlike the invocation kinds it
// carries no numeric-suffix aliases, and it does not wrap an
// `InvocationExpression` for the delegation itself, so no double count
// arises; calls in its argument list are separate nodes counted on their own.
//
// This set is deliberately wider than `CsharpCode::is_call`, which answers
// `bca find --type call` and stops at the invocation kinds. Fitzpatrick's
// rule is "function invocation or object creation", so the three
// construction kinds below are branches; a `find` result that pointed at a
// constructor would not be a call site a reader navigates to. The contract
// and the full per-language survey live on the `Checker::is_call` trait
// doc; `csharp_is_call_excludes_constructors` pins the C# half (#1456).
fn csharp_count_token_branch<'a>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) -> bool {
    use Csharp::*;
    let is_branch = match node.kind_id().into() {
        // Spelled out rather than reaching for `csharp_invocation_expr_kinds!()`
        // (`big-code-analysis-ast/src/macros/kind_sets.rs`), which carries
        // exactly these three: the macro *is* an or-pattern, so combining it
        // with further alternatives here trips `clippy::unnested_or_patterns`
        // at `-D warnings`. `Checker::is_csharp_call` can use it because the
        // set is that predicate's whole answer. The §1 obligation is met by
        // listing all three aliases, not by the macro.
        crate::Csharp::InvocationExpression
        | crate::Csharp::InvocationExpression2
        | crate::Csharp::InvocationExpression3
        | ObjectCreationExpression
        | ConstructorInitializer => true,
        // The C# 12 primary-constructor superclass call — `class
        // Sub(int x) : Base(x)` — invokes the base constructor exactly as
        // `: base(x)` does, and scored zero until #1406. Kotlin's
        // equivalent was fixed in #1384; this is the C# sibling.
        //
        // tree-sitter-c-sharp 0.23.5 spells the two declaration families
        // differently, so there is no single node to match (verified with
        // `bca dump`, not inferred):
        //
        //   `record R(int x) : Base(x);`  base_list > primary_constructor_
        //                                 base_type > argument_list
        //   `class C(int x) : Base(x) {}` base_list > argument_list (flat)
        //
        // Matching both kinds under a `base_list` parent therefore covers
        // both families. This is §5's container-plus-containable shape —
        // a `primary_constructor_base_type` *holds* an `argument_list` —
        // and the parent gate is what makes it safe rather than a double
        // count: in the record spelling the inner `argument_list`'s parent
        // is the `primary_constructor_base_type`, not the `base_list`, so
        // only the outer node fires. The two are never siblings, so
        // neither spelling ever presents both. `record R(int x) : Base;`
        // and `struct S(int x) : IBase {}` emit no `argument_list` and no
        // `primary_constructor_base_type` at all, so an argument-less base
        // type still costs nothing, and `enum E : byte` / `interface I :
        // IBase` cost nothing for the same reason.
        //
        // Three grammar-reachable shapes are not valid C# and score 1
        // here: `interface I : IBase(x)`, `enum E : Base(x)`, and a
        // `class NoCtor : Base(x)` with no primary constructor. Each
        // parses cleanly — the grammar is more permissive than the
        // language — and no valid program can tell the behaviours apart,
        // so per §6 the gap is documented and left untested rather than
        // pinned, which would make the grammar's present permissiveness
        // the contract.
        //
        // The parent gate is load-bearing on `ArgumentList`, which is
        // otherwise the argument list of every call in the file. It is
        // also what keeps the sibling `attribute_argument_list` /
        // `bracketed_argument_list` / `type_argument_list` kinds out —
        // they are distinct kind ids, and none of them occurs under a
        // `base_list`. `BaseList` and `BaseList2` both render to
        // `"base_list"` and only the second is observed here; both are
        // listed per `.claude/rules/grammar-dispatch.md` §1.
        //
        // Where the branch lands is decided by the space tree, not here,
        // and the two delegation spellings land differently. The
        // `base_list` is a direct child of the class declaration — outside
        // every member, and a sibling of the primary constructor's
        // `parameter_list` rather than inside it — and a primary
        // constructor has no declaration node of its own for
        // `CsharpCode::is_func_space` to promote
        // (`big-code-analysis-ast/src/getter/csharp.rs` lists only
        // `MethodDeclaration | ConstructorDeclaration | …` as
        // `SpaceKind::Function`). So `class Sub(int x) : Base(x)` bills its
        // branch to the **class** space, while `class C : Base { C(int x) :
        // base(x) {} }` bills the same call to the constructor's *function*
        // space. File-level `branches_sum` agrees between the two
        // spellings, so nothing is wrong at the aggregate, but per-function
        // `branches_max` / `branches_average` and a per-space
        // `bca check --threshold abc=N` see them differently: a codebase
        // migrating to primary constructors shifts weight off its function
        // rows and onto its class rows with no behaviour change. That is
        // the intended reading — there is no function to attribute the call
        // to — and it is pinned by
        // `csharp_primary_constructor_base_call_is_a_branch` and
        // `csharp_base_call_and_constructor_initializer_do_not_double_count`
        // in `src/metrics/abc.rs`. Kotlin's `ConstructorInvocation` under a
        // `delegation_specifier` (`src/metrics/abc/kotlin.rs`) has the
        // identical shape and the identical attribution (#1456).
        PrimaryConstructorBaseType | ArgumentList => ancestors
            .parent(node)
            .is_some_and(|parent| matches!(parent.kind_id().into(), BaseList | BaseList2)),
        _ => false,
    };
    if is_branch {
        stats.branches += 1.;
    }
    is_branch
}

fn csharp_count_token_condition<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) -> bool {
    use Csharp::*;
    let kind = node.kind_id();
    // The `Comparison` row's payers (`roles/csharp.rs`): the six
    // comparison tokens, each gated to a `binary_expression` parent, and
    // the `is` of a type test. The gate's derivation — the six other
    // productions that spell the tokens, from the generic brackets to an
    // `operator_declaration` (#1420) and a `relational_pattern` (#1383),
    // and why the allowlist fails closed under error recovery — is on
    // the row. Fitzpatrick Rule 5 scores the operator wherever it is
    // written, so `bool b = x is string;` scores as `bool b = x == 1;`
    // does (#1461). The `Ternary` row's `?` likewise, gated by the row
    // to the `conditional_expression`; and the `CaseArm` row's leaf —
    // each `case` label of a statement arm, gated by the row to the arm
    // since `goto case 2;` spells the same token (#1450 / #1451), and
    // each `switch` *expression* arm but the bare discard `_ =>`, the
    // `default:` analogue (#456, lesson 11) — one condition per arm, as
    // cyclomatic counts them.
    if comparison::pays_at_leaf_kind(kind, node, code, ancestors)
        || ternary::pays_at_leaf_kind(kind, node, code, ancestors)
        || case_arm::pays_at_leaf_kind(kind, node, code, ancestors)
    {
        stats.conditions += 1.;
        return true;
    }
    match kind.into() {
        // The statement `switch` counts its `Case` arms; the `default:`
        // arm (token `Default`, shared by both classic `default:` and
        // arrow `default ->` forms) is the unconditional fallthrough and
        // is excluded, mirroring cyclomatic's `Case`-only count and the
        // expression-arm discard rule below (issues #456, #469).
        //
        // These three stay ungated; `EQEQ` / `BANGEQ` shared the arm until
        // #1420 and moved to the gated one below, and `Case` moved to its
        // own gated arm in #1450 / #1451. `Else`, `Try` and `Catch` come
        // from one production each (`if_statement`, `try_statement`,
        // `catch_clause`), so there is nothing to gate on.
        //
        // `QMARKQMARK` joined them in #1459 and is ungated for the same
        // reason: `??` comes from `binary_expression` alone. It is a
        // decision — `a ?? b` evaluates `b` only when `a` is null, the
        // `a != null ? a : b` the language lets you not spell — and C#
        // cyclomatic has always counted it
        // (`src/metrics/cyclomatic/csharp.rs`), so ABC sat one *below*
        // C#'s own decision count wherever a `??` appeared.
        //
        // #1422 made that gap visible rather than merely present. Its
        // guard slot claims every spelling of a `when` guard is worth
        // one condition, and `when b ?? false` was worth zero: the slot
        // sees a `binary_expression`, which it leaves to the operator
        // arms because a comparison guard's operator is already counted
        // here — and for `??` there was nothing to leave it to. Counting
        // the token is what levels the spellings *and* keeps a compound
        // guard (`when a > 1 && b < 2`) at its two conditions; a blanket
        // `+1` on the slot would have done neither.
        //
        // No double count (§5): `??` is one token, distinct from the
        // bare `QMARK` below and from the `??=` compound assignment
        // (`QMARKQMARKEQ`, counted as an assignment), and every
        // condition slot declines a `binary_expression` outright.
        // C#'s two type tests join them in #1461, scored by use rather
        // than by slot. They sat in `csharp_bool_terminal_kinds!()` until
        // then, which counts only inside a boolean slot: `var b = x is
        // int;` scored zero where the `var b = x == 1;` beside it
        // scored one, because `EQEQ` is a token arm and `is` was not.
        // Fitzpatrick Rule 5 scores a relational operator wherever it is
        // written, so the asymmetry was in the mechanism, not the rule.
        //
        // Matched as nodes rather than as an `Is` token because the
        // grammar splits the construct across two productions by
        // pattern-ness, not by keyword: `x is int` is an
        // `is_expression` and `x is null` / `x is not Foo` / `x is int
        // n` are all `is_pattern_expression` (verified with `bca dump`).
        // The two are disjoint alternatives, never nested, so exactly
        // one fires per test (§5), and no arm counts the keyword.
        //
        // The guard slot added by #1422 is unaffected: a `when x is
        // int` now reaches this arm instead of the slot's terminal-set
        // test, and still totals one.
        //
        // They share the arm rather than sitting beside it because the
        // arm's meaning is "this node is a condition, with nothing to
        // gate on" — which is as true of a production as of a token.
        Else | Try | Catch => {
            stats.conditions += 1.;
        }
        // The `NullSafe` row's payers: `??` at its token and a conditional
        // access at its node — `a?.b` / `a?[0]`, whose operator is the
        // same bare `?` the ternary and two type productions spell, so
        // the node pays once for it (#1459, #1275). That keeps ABC level
        // with C#'s own cyclomatic on a safe-navigation chain, a
        // C#-internal agreement: `a?.b?.c` scores ABC conditions 2 here
        // and 0 in TypeScript, JavaScript, Kotlin and Groovy, whose `?.`
        // is a distinct token their ABC arms never list — even though
        // `safe_navigation_chain_parity` pins all five at +2 *cyclomatic*.
        // Pre-existing, and preserved rather than silently changed. The
        // compound `??=` is excluded on the row and stays an assignment:
        // a cyclomatic decision that is no condition, so `a ??= b` sits
        // one below C#'s decision count — deliberate (the JS family
        // scores it the same way), but a live divergence.
        _ if null_safe::pays_at_leaf_kind(kind, node, code, ancestors) => {
            stats.conditions += 1.;
        }
        // tree-sitter-c-sharp emits a bare `?` from exactly four
        // productions: `nullable_type` (`int? x`) and
        // `type_parameter_constraint` (`where T : class?`), type syntax
        // and no more a decision than the `<` / `>` around a generic
        // (#1275); `conditional_expression`, the `Ternary` row's, paid
        // at the token above; and `conditional_access_expression`, the
        // `NullSafe` row's, paid at the node above. What is left for the
        // bare token is the spelling no production owns — a `?` error
        // recovery strands under an `ERROR` — which this arm has always
        // paid, by denying the four parents rather than naming the one.
        //
        // Denylist polarity, deliberately, so that it fails *open*: type
        // syntax the grammar gains later starts counting, which is not
        // hypothetical — `type_parameter_constraint` was the second half
        // of #1275 and is absent from the issue's own deny set. The four
        // productions above are the closed enumeration at the pinned
        // `=0.23.5`; re-derive it on a grammar bump.
        // role-bypass: allow(Ternary, NullSafe) — the recovered `?` no row owns, told by
        // denying the parents the two rows' owners are
        QMARK
            if ancestors.parent(node).is_some_and(|parent| {
                !matches!(
                    parent.kind_id().into(),
                    NullableType
                        | TypeParameterConstraint
                        | ConditionalExpression
                        | ConditionalAccessExpression
                )
            }) =>
        {
            stats.conditions += 1.;
        }
        _ => return false,
    }
    true
}

fn csharp_walk_for_conditions<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    stats: &mut Stats,
) {
    // bca: suppress(cyclomatic) — slot dispatch table: one arm per
    // grammar kind holding a boolean slot, each an independent routing
    // rule; the row reads (#1695) are arms like the kind arms they
    // replaced.
    use Csharp::*;
    let conds = &mut stats.conditions;
    let kind = node.kind_id();
    // Fitzpatrick Rule 9: each operand of an `&&` / `||` chain is a
    // boolean slot; the `ShortCircuit` row is read at its leaf only to
    // find the chain, the token's parent. `a && b || c` is a
    // left-nested chain of `binary_expression`s, so an operand that is
    // itself a chain is paid by its own operator's visit.
    if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) {
        if let Some(chain) = ancestors.parent(node) {
            count_field_operands(
                &chain,
                |operand, conds| csharp_count_condition(operand, code, conds),
                conds,
            );
        }
        return;
    }
    match kind.into() {
        // Every `argument_list` but the one a `base_list` holds directly:
        // that one is itself the branch, so `compute` scores its
        // arguments on the branch path instead. A `bracketed_argument_list`
        // (an indexer, `o[!a]`) is a distinct kind and is not walked, as
        // Java's array index is not.
        ArgumentList => csharp_count_arguments(node, code, conds),
        // tree-sitter-c-sharp spells the parens of `if` / `while` /
        // `do … while` as anonymous tokens, NOT a wrapping
        // `parenthesized_expression` as in tree-sitter-java, so the
        // slot holds the bare condition. Read by grammar field: a fixed
        // index landed on the `(` token (#370), and after that fix on a
        // comment, since `if (/*c*/ b)` puts one at child(2) (#1455).
        IfStatement | WhileStatement | DoStatement => {
            if let Some(condition) = node.child_by_field_name("condition") {
                csharp_count_condition(&condition, code, conds);
            }
        }
        // C#'s two guard spellings, the `Guard` row's owners, each
        // modelled as a condition slot exactly like the `if` / `while` /
        // `do` slots above (#1422).
        // Before this, a guard scored whatever operator happened to sit
        // inside it: `when x % 2 == 0` and `when x > 2` counted one via
        // the comparison-token arm while `when IsEven(x)` counted zero,
        // so three semantically identical guards produced two different
        // numbers. As a slot every spelling contributes exactly one,
        // and a compound guard (`when a > 1 && b < 2`) keeps its
        // sub-structure rather than collapsing to one.
        //
        // Suppressing the guard's operator instead would have reached
        // the same internal agreement one count *below* C#'s own
        // cyclomatic decision count, which #1422 fixes upward in
        // `src/metrics/cyclomatic/csharp.rs`.
        //
        // By role, not index (`.claude/rules/grammar-dispatch.md` §3):
        // the two clauses carry their guard as the only *expression*
        // child but disagree on where it sits, because
        // `catch_filter_clause` spells its parentheses as anonymous
        // tokens (`when`, `(`, expr, `)`) the way `if_statement` does,
        // while `when_clause` has none (`when`, expr). Neither exposes
        // a field for the slot.
        //
        // The first operand that is not an `extra`: a comment may precede
        // the expression (`when /*c*/ g`) and would pay as a slot of its
        // own.
        _ if guard::owns_kind(kind, node, code, ancestors) => {
            if let Some(guard) = wrapped_operand(node) {
                csharp_count_condition(&guard, code, conds);
            }
        }
        // The value slots, none of which a fixed index can address: a
        // comment may precede the value (`return /*c*/ !b;`), and a
        // lambda's optional `async` / attribute preamble shifts its body.
        // `return` and a declarator name no field for the value, so the
        // first and last operand stand in: a `return` holds only its
        // value, and a declarator's initialiser follows its name.
        ReturnStatement => csharp_count_negated_slot(wrapped_operand(node), code, conds),
        crate::Csharp::VariableDeclarator | crate::Csharp::VariableDeclarator2 => {
            csharp_count_negated_slot(last_operand(node), code, conds);
        }
        AssignmentExpression => {
            csharp_count_negated_slot(node.child_by_field_name("right"), code, conds);
        }
        LambdaExpression => {
            csharp_count_negated_slot(node.child_by_field_name("body"), code, conds);
        }
        ForStatement => csharp_walk_for_statement(node, code, stats),
        // The `conditional_expression` the `Ternary` row owns: its `?`
        // paid above, this walks its operand slots.
        _ if ternary::owns_kind(kind, node, code, ancestors) => {
            csharp_walk_conditional(node, code, stats);
        }
        _ => {}
    }
}

// `cond ? a : b`, addressed by the grammar's `condition` /
// `consequence` / `alternative` fields rather than by index (#1181 — a
// comment between a token and its operand shifted every positional
// read). The cond-classifier match is shared with
// `csharp_walk_for_conditions`'s `if`/`while`/`do` arms via
// `csharp_count_condition`; the two branch slots are negated operands,
// so only a `!`-prefixed branch contributes a condition.
fn csharp_walk_conditional(node: &Node, code: &[u8], stats: &mut Stats) {
    let conds = &mut stats.conditions;
    // By grammar FIELD, not index — see `java_walk_ternary` for why the
    // positional form dropped a negated branch operand behind a comment
    // (#1181).
    if let Some(condition) = node.child_by_field_name("condition") {
        csharp_count_condition(&condition, code, conds);
    }
    for field in ["consequence", "alternative"] {
        csharp_count_negated_slot(node.child_by_field_name(field), code, conds);
    }
}

// Counts unary / single-token conditions inside `for` statements. The
// C# grammar exposes the loop condition via the named `condition` field
// on `for_statement`, so we look it up by name rather than positional
// index. Comparison-operator conditions like `i < n` are still counted
// by the standard `GT | LT | ...` arms — this only fires when the
// condition is a bare identifier, invocation, boolean literal,
// parenthesised expression, or `!`-prefixed unary expression.
fn csharp_walk_for_statement(node: &Node, code: &[u8], stats: &mut Stats) {
    if let Some(condition) = node.child_by_field_name("condition") {
        csharp_count_condition(&condition, code, &mut stats.conditions);
    }
}

impl Abc for CsharpCode {
    // See `impl Abc for JavaCode` for the short-circuit-chain rationale
    // and the cross-helper-exclusivity invariant.
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        if csharp_count_token_assignment(node, ancestors, stats) {
            return;
        }
        if csharp_count_token_branch(node, ancestors, stats) {
            // `class S(bool a) : Base(!a)`: the `argument_list` under the
            // `base_list` is the branch node itself, so the walk below
            // never sees it. The record spelling nests its list inside a
            // `primary_constructor_base_type` and reaches the walk, so
            // without this the two spellings of one base call disagree.
            if node.kind_id() == Csharp::ArgumentList {
                csharp_count_arguments(node, code, &mut stats.conditions);
            }
            return;
        }
        if csharp_count_token_condition(node, code, ancestors, stats) {
            return;
        }
        csharp_walk_for_conditions(node, code, ancestors, stats);
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
    #[cfg(feature = "csharp")]
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
