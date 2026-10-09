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
        // `(expr)` — the inner expression follows the `(` token.
        csharp_paren_expr_kinds!() => wrapped_operand(node).map(|o| (o, false)),
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
// a condition: an applied comparison or `??` (the token arms), a ternary
// (its `?`), an `is` test (its own arm), or an `&&` / `||` chain, whose
// operands each pay through `csharp_count_condition`.
//
// An assignment is not peeled, because the `AssignmentExpression` arm
// already scores its right-hand side as a negated operand: peeling
// `if (y = !b)` down to `b` would pay the slot for the negation that arm
// counts. It scores itself exactly when its value does, as Java's does.
fn csharp_condition_scores_itself(expr: &Node) -> bool {
    use Csharp::*;

    match expr.kind_id().into() {
        ConditionalExpression | IsExpression | IsPatternExpression => true,
        BinaryExpression | BinaryExpression2 => {
            expr.child_by_field_name("operator").is_some_and(|op| {
                matches!(
                    op.kind_id().into(),
                    EQEQ | BANGEQ | LT | GT | LTEQ | GTEQ | QMARKQMARK | AMPAMP | PIPEPIPE
                )
            })
        }
        AssignmentExpression => expr.child_by_field_name("right").is_some_and(|right| {
            let (value, proves_boolean) = peel(&right, csharp_wrapper_operand);
            proves_boolean || csharp_condition_scores_itself(&value)
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
fn csharp_count_condition(condition: &Node, conditions: &mut f64) {
    count_boolean_slot(
        condition,
        csharp_wrapper_operand,
        csharp_condition_scores_itself,
        conditions,
    );
}

// An operand outside a boolean slot — a `return` value, a declarator or
// assignment value, a lambda body, a ternary branch — scores only when
// a `!` proves it boolean (see `count_negated_operand`).
fn csharp_count_negated(operand: &Node, conditions: &mut f64) {
    count_negated_operand(
        operand,
        csharp_wrapper_operand,
        csharp_condition_scores_itself,
        conditions,
    );
}

fn csharp_count_negated_slot(slot: Option<Node>, conditions: &mut f64) {
    if let Some(operand) = slot {
        csharp_count_negated(&operand, conditions);
    }
}

// Each argument of a call is a negated operand.
fn csharp_count_arguments(arguments: &Node, conditions: &mut f64) {
    count_each_operand(arguments, csharp_count_argument, conditions);
}

// tree-sitter-c-sharp wraps every argument in an `argument` node —
// `seq(optional(seq(field('name', …), ':')), optional('ref' | 'out' |
// 'in'), expression | declaration_expression)` — so the value is its
// last operand, after any named-argument label (`name: !a`) or
// passing-mode keyword. Handing the wrapper itself to the peel scored
// every argument zero, `G(!a)` included (#1537). The value is located
// by role rather than by index because both prefixes are optional
// (grammar-dispatch §3).
fn csharp_count_argument(argument: &Node, conditions: &mut f64) {
    if let Some(value) = last_operand(argument) {
        csharp_count_negated(&value, conditions);
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
        Else | Try | Catch | QMARKQMARK | IsExpression | IsPatternExpression => {
            stats.conditions += 1.;
        }
        // `case` comes from two productions — `switch_section`, a real
        // arm, and `goto_statement`, where `goto case 2;` is an
        // unconditional jump to one. Both spell the same token, so the
        // jump scored a condition it does not earn: a method whose only
        // difference from a control was a `goto case` read one condition
        // *and* one cyclomatic decision higher (#1450 / #1451). The gate
        // is shared with `src/metrics/cyclomatic/csharp.rs` and carries
        // the grammar sweep and the allowlist rationale in its doc
        // comment.
        //
        // The two arms moved in one change because they measure the same
        // token, not because any global law binds them: `conditions ==
        // cyclomatic() - 1` is an opt-in fixture property asserted by two
        // of `src/metrics/abc.rs`'s three helpers, and the third
        // documents it as knowingly false in general. Gating one side
        // alone broke no test in the suite — no fixture spelled `goto
        // case` outside the cognitive tests — which is the reason to
        // write the pair as one commit rather than trust the gate to
        // notice.
        //
        // A gated-out `Case` falls through to
        // `csharp_walk_for_conditions`, which has no `Case` arm, so the
        // fall-through is a no-op.
        Case if crate::metrics::cyclomatic::csharp_case_token_is_switch_arm(node, ancestors) => {
            stats.conditions += 1.;
        }
        // All six C# comparison tokens, counted only where they *apply*
        // an operator. A `grammar.json` sweep of tree-sitter-c-sharp
        // 0.23.5 finds each one in these productions, and only
        // `binary_expression` (`a < b`) qualifies:
        //
        //   `<` / `>`   — `binary_expression`, `relational_pattern`,
        //                 `operator_declaration`, `type_argument_list`,
        //                 `type_parameter_list`, `function_pointer_type`
        //   `<=` / `>=` — `binary_expression`, `relational_pattern`,
        //                 `operator_declaration`
        //   `==` / `!=` — `binary_expression`, `preproc_binary_expression`
        //                 (`#if A == B`), `operator_declaration`
        //
        // Three reasons to exclude, one per production family:
        //
        // `type_argument_list` (`Dictionary<K, V>`), `type_parameter_list`
        // (`class Foo<T>`) and `function_pointer_type` (`delegate*<int,
        // int>`) are type syntax, no more a decision than the `?` of a
        // nullable type (#1275).
        //
        // `operator_declaration` — `public static bool operator <(V a,
        // V b)` — names the operator being *defined* rather than applying
        // it. #1297 found this for `<` / `>`, whose denylist at the time
        // named the three type-syntax parents and not this one, so every
        // comparison-operator overload scored a condition. C# overloads
        // six operators, though, and the other four are distinct tokens
        // that reached a different arm: `<=` `>=` `==` `!=` each still
        // scored 1 against `<` / `>`'s 0 on a class overloading all six,
        // every member of which has `cyclomatic()` 1 (#1420).
        //
        // `relational_pattern` is excluded for a third reason (#1383): a
        // pattern's comparison operator is not its own decision, since the
        // enclosing `switch_expression_arm` or `if` condition slot already
        // scores one. Counting the operator too charged
        // `x switch { > 5 => … }` twice what the constant arm
        // `x switch { 5 => … }` scores, and twice C#'s own cyclomatic
        // decision count.
        //
        // Allowlist polarity, matching Java's #1274 fix and unlike the
        // `QMARK` arm below: one decision parent against six excluded
        // productions, and a grammar bump that grows a seventh should fail
        // closed (`.claude/rules/grammar-dispatch.md` §1). #1383 landed
        // half of this as a denylist naming `relational_pattern` alone,
        // which is what let `operator_declaration` through for four of the
        // six tokens; the allowlist subsumes that denial. The `QMARK` arm
        // takes the opposite polarity for a reason specific to that token
        // — see its comment.
        //
        // `BinaryExpression2` is the second enum id carrying the
        // `binary_expression` kind string, listed per §1. Measured at this
        // pin it is unreachable from here: the `preproc_binary_expression`
        // of `#if A == B` parses as `BinaryExpression` (369), so the
        // preprocessor spelling counts through the first entry, and C#'s
        // preprocessor admits only `== != && || !` in any case.
        //
        // A failed guard returns `false` and falls through to
        // `csharp_walk_for_conditions`, which matches none of these six
        // token kinds — so the fall-through is a no-op.
        //
        // Failing closed also covers error recovery, where the sweep
        // above says nothing: a token the parser reparents under `{ERROR}`
        // stops counting. `<` / `>` have behaved that way since #1297 and
        // the other four now agree, valid input is unaffected, and no C#
        // corpus file parses with an `{ERROR}` node — measured. Nothing
        // pins it, because a fixture the language rejects would make the
        // grammar's present recovery the contract (§6).
        GT | LT | GTEQ | LTEQ | EQEQ | BANGEQ
            if ancestors.parent(node).is_some_and(|parent| {
                matches!(
                    parent.kind_id().into(),
                    BinaryExpression | BinaryExpression2
                )
            }) =>
        {
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
        // pass could drop. The sibling `??` gap this comment used to
        // record — a C# cyclomatic decision that was no ABC condition —
        // is closed for `??` by `QMARKQMARK` joining the ungated
        // condition-token arm at the top of this match (#1459). It is
        // *not* closed for `??=`: `QMARKQMARKEQ` is a cyclomatic
        // decision and stays an ABC assignment only, so `a ??= b` still
        // sits one below C#'s decision count. That is deliberate rather
        // than missed — the JS family scores `??=` the same way — but it
        // is a live divergence, not a closed one.
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
        // Fitzpatrick Rule 9: each operand of an `&&` / `||` chain is a
        // boolean slot. `a && b || c` is a left-nested chain of
        // `binary_expression`s, so an operand that is itself a chain is
        // paid by its own operator's visit.
        AMPAMP | PIPEPIPE => {
            if let Some(chain) = ancestors
                .parent(node)
                .filter(|p| matches!(p.kind_id().into(), BinaryExpression | BinaryExpression2))
            {
                count_field_operands(&chain, csharp_count_condition, conds);
            }
        }
        // Every `argument_list` but the one a `base_list` holds directly:
        // that one is itself the branch, so `compute` scores its
        // arguments on the branch path instead. A `bracketed_argument_list`
        // (an indexer, `o[!a]`) is a distinct kind and is not walked, as
        // Java's array index is not.
        ArgumentList => csharp_count_arguments(node, conds),
        // tree-sitter-c-sharp spells the parens of `if` / `while` /
        // `do … while` as anonymous tokens, NOT a wrapping
        // `parenthesized_expression` as in tree-sitter-java, so the
        // slot holds the bare condition. Read by grammar field: a fixed
        // index landed on the `(` token (#370), and after that fix on a
        // comment, since `if (/*c*/ b)` puts one at child(2) (#1455).
        IfStatement | WhileStatement | DoStatement => {
            if let Some(condition) = node.child_by_field_name("condition") {
                csharp_count_condition(&condition, conds);
            }
        }
        // C#'s two guard spellings, each modelled as a condition slot
        // exactly like the `if` / `while` / `do` slots above (#1422).
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
        WhenClause | CatchFilterClause => {
            if let Some(guard) = wrapped_operand(node) {
                csharp_count_condition(&guard, conds);
            }
        }
        // The value slots, none of which a fixed index can address: a
        // comment may precede the value (`return /*c*/ !b;`), and a
        // lambda's optional `async` / attribute preamble shifts its body.
        // `return` and a declarator name no field for the value, so the
        // first and last operand stand in: a `return` holds only its
        // value, and a declarator's initialiser follows its name.
        ReturnStatement => csharp_count_negated_slot(wrapped_operand(node), conds),
        crate::Csharp::VariableDeclarator | crate::Csharp::VariableDeclarator2 => {
            csharp_count_negated_slot(last_operand(node), conds);
        }
        AssignmentExpression => csharp_count_negated_slot(node.child_by_field_name("right"), conds),
        LambdaExpression => csharp_count_negated_slot(node.child_by_field_name("body"), conds),
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
// `csharp_count_condition`; the two branch slots are negated operands,
// so only a `!`-prefixed branch contributes a condition.
fn csharp_walk_conditional(node: &Node, stats: &mut Stats) {
    let conds = &mut stats.conditions;
    // By grammar FIELD, not index — see `java_walk_ternary` for why the
    // positional form dropped a negated branch operand behind a comment
    // (#1181).
    if let Some(condition) = node.child_by_field_name("condition") {
        csharp_count_condition(&condition, conds);
    }
    for field in ["consequence", "alternative"] {
        csharp_count_negated_slot(node.child_by_field_name(field), conds);
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
        csharp_count_condition(&condition, &mut stats.conditions);
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
        if csharp_count_token_branch(node, ancestors, stats) {
            // `class S(bool a) : Base(!a)`: the `argument_list` under the
            // `base_list` is the branch node itself, so the walk below
            // never sees it. The record spelling nests its list inside a
            // `primary_constructor_base_type` and reaches the walk, so
            // without this the two spellings of one base call disagree.
            if node.kind_id() == Csharp::ArgumentList {
                csharp_count_arguments(node, &mut stats.conditions);
            }
            return;
        }
        if csharp_count_token_condition(node, ancestors, stats) {
            return;
        }
        csharp_walk_for_conditions(node, ancestors, stats);
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
