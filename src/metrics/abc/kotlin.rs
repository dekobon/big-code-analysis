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

use super::{Abc, Stats, count_boolean_slot, count_field_operands, rows_own, wrapped_operand};
use crate::roles::kotlin::{branch, call, case_arm, catch, comparison, null_safe, short_circuit};
use crate::*;

// Fitzpatrick's ABC rules adapted for Kotlin syntax. Kotlin shares the
// JVM and Java's spec roots: assignments count once per `=` / augmented
// assignment / ++ / --, branches count once per function invocation or
// object construction, conditions count comparison operators plus the
// `else` / `when`-entry / `catch` arms. Compared with the Java impl we
// stay token-level (matching the leaf kind_ids) rather than walking
// `Modifiers` children; the Kotlin grammar exposes the relevant
// operators directly as token nodes inside `binary_expression` and
// `assignment`.
//
// It does *not* have `prefix_expression` / `postfix_expression`, which
// this comment named until #1459. tree-sitter-kotlin-ng spells both
// unary positions as one `unary_expression` told apart only by its
// `operator` field — and believing the two-production version is how
// `kotlin_inspect_container` came to handle the prefix `!` and drop the
// postfix `!!` while reading as though it covered both.

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
    // role-bypass: allow(Member, DocTarget) — the binding shape a `=` initialises,
    // a local or a property alike; not whether it is a container's member or
    // a doc target
    if !matches!(
        parent.kind_id().into(),
        PropertyDeclaration | ClassParameter
    ) {
        return false;
    }
    // role-bypass: allow(DeclKeyword) — which declaration keyword, not whether the node is one
    parent.children().any(|child| child.kind_id() == Val)
}

// The operand a transparent wrapper wraps, and whether the wrapper
// itself proves that operand is boolean. `None` means the node is not a
// wrapper and the peel stops there.
//
// Only the prefix `!` proves booleanness: `!x` is boolean whatever `x`
// is, while parentheses, a null assertion and a cast are all
// type-preserving (`a!!` and `a as T` are boolean exactly when the slot
// they sit in is). So they inherit the caller's verdict rather than
// setting it.
//
// Two of the four shapes were not here from the start, and both were
// #1459 fallout from #1421 turning the `when` entry's blanket count
// into a condition slot: before that, `when { a!! -> … }` was paid for
// by the entry, and after it by this peel, which scored zero.
//
// - `unary_expression` is one kind for *both* Kotlin unary spellings,
//   and the peel recognised only the prefix one. A postfix `a!!` stores
//   its operand *before* the token, so the positional read for `!x`
//   found the `!!` and stopped on a node the slot had already routed
//   here as handled.
// - `as_expression` was not recognised at all, so a cast fell off the
//   end of the slot's `else if`.
//
// By field, not index (`.claude/rules/grammar-dispatch.md` §3): both
// kinds name their parts (`operator` / `argument`, `left` / `right`), so
// one read serves the prefix and postfix spellings alike instead of the
// per-spelling index that produced the defect, and it survives a grammar
// re-order. It also survives an interposed `extra`: `when { ! /*c*/ a }`
// scores its condition, where the positional read scored zero.
//
// `parenthesized_expression` names nothing — its only child in
// node-types.json is the unlabelled inner `expression` — so it takes the
// first operand that is not an extra. The positional `child(1)` it
// replaced scored `when { ( /*c*/ a) -> … }` zero, because that child is
// the comment (#1455).
fn kotlin_wrapper_operand<'a>(node: &Node<'a>) -> Option<(Node<'a>, bool)> {
    use Kotlin::*;

    match node.kind_id().into() {
        // `(expr)` — the inner expression follows the `(` token.
        ParenthesizedExpression => wrapped_operand(node).map(|o| (o, false)),
        AnnotatedExpression | LabeledExpression => node
            .children()
            .find(|child| {
                super::is_operand(child) && !matches!(child.kind(), "annotation" | "label")
            })
            .map(|o| (o, false)),
        UnaryExpression => {
            let operand = node.child_by_field_name("argument")?;
            match node.child_by_field_name("operator")?.kind_id().into() {
                BANG => Some((operand, true)),
                BANGBANG => Some((operand, false)),
                // `-x`, `x++` and friends: arithmetic, never a boolean
                // slot's operand, so the peel declines rather than
                // reaching a bare `identifier` and counting it.
                _ => None,
            }
        }
        // `x as T` — `left` is the operand, `right` the target type.
        //
        // The *safe* cast `x as? T` is excluded on purpose (§5, one kind
        // per operator): `AsQMARK` is already a condition token below, so
        // peeling through it too would score `when { a as? T -> … }` two
        // where every other spelling scores one. The plain `as` has no
        // such token — `As` is not in that arm — so the peel is the only
        // thing that can count it, and the two spellings come out level.
        AsExpression if !node.is_child(AsQMARK as u16) => {
            Some((node.child_by_field_name("left")?, false))
        }
        _ => None,
    }
}

// Whether an arm of `compute` already charges `expr` (already peeled) as
// a condition: a comparison, an elvis `?:` or an `&&` / `||` chain
// (their operator tokens, a chain's operands each paying through
// `kotlin_count_condition`), an `is` / `in` test, a safe cast `as?`, an
// `if` expression (its `else`, as a C-family ternary pays its `?`) or a
// `try` (its `try` and `catch`), each of which pays.
//
// A `when` is not one: its entries pay as clauses, and the slot holding
// it pays the decision of using the result, as the slot holding Java's
// `switch`, C#'s `switch` expression or Rust's `match` does. So
// `if (when (x) { 1 -> true; else -> false })` scores 2 like those
// twins (it scored 1), and `if (when (a) { else -> true })` 1 (#1533).
//
// The comparison, `is` / `in` and `&&` / `||` owners are the
// `Comparison`, `ShortCircuit` and `NullSafe` rows' (no owner gate
// reads ancestors, so the peeled descendant is asked with none); of
// the last row's two owners only the elvis is a boolean test.
fn kotlin_condition_scores_itself(expr: &Node, code: &[u8]) -> bool {
    use Kotlin::*;

    // The elvis owner, asked of the row's projection; the row's other
    // owner, a safe navigation, is no boolean test and scores itself
    // nowhere.
    // role-bypass: allow(NullSafe) — which of the row's two owners this consumer scores, told
    // by the projected token: the elvis, not the safe navigation
    // An `if` expression — the `Branch` row's owner — used as a value
    // is a boolean test in its own right, as a `try` expression is (no
    // row owns `try`; `Catch` owns its handler).
    rows_own!(expr, code; comparison, short_circuit, branch)
        || null_safe::owns_via(QMARKCOLON as u16, expr, code, Ancestors::unknown())
        || match expr.kind_id().into() {
            TryExpression => true,
            AsExpression => expr.is_child(AsQMARK as u16),
            _ => false,
        }
}

// Scores one boolean slot — an `if` / `while` / `do-while` condition, a
// subject-less `when` entry's condition, an operand of an `&&` / `||`
// chain (see `count_boolean_slot`). A wrapper — parentheses, `!`, `!!`,
// `as` — is peeled to what it wraps, so `if ((a!! as Boolean))` pays
// once, and a comparison or chain pays through its own arms instead.
// Without the slot, idiomatic bare predicates reported 0 ABC conditions
// against Kotlin's own cyclomatic decision (#773).
fn kotlin_count_condition(condition: &Node, code: &[u8], conditions: &mut f64) {
    count_boolean_slot(
        condition,
        kotlin_wrapper_operand,
        |expr| kotlin_condition_scores_itself(expr, code),
        conditions,
    );
}

// Returns true when the `when_expression` enclosing `entry` carries a
// subject — `when (x) { … }` rather than `when { … }`. The distinction
// decides how the entry's condition is scored (#1421): a subject-ful
// entry lists a *pattern* compared against the subject, a subject-less
// one lists an ordinary boolean expression.
//
// A `when_entry`'s parent IS the `when_expression`, and the grammar
// exposes no field for the subject (`when_expression` has an empty
// `fields` map in node-types.json), so the membership test is a scan of
// the parent's children rather than a `child_by_field_name`. Scanning
// rather than reading a fixed index also keeps a leading `extra` — a
// comment between `when` and `(x)` — from displacing the answer.
//
// The scan stops at the opening brace, which is what keeps it `O(1)`.
// The subject can only sit in the header, between `when` and `{`, so
// everything past the brace is arms — and an unbounded `any()` over a
// *subject-less* `when` never short-circuits, so it walks every arm
// once per arm. That is quadratic in the arm count, and measurable:
// before the bound, a generated 8,000-arm subject-less `when` took
// 2.5 s against 0.03 s for the same-size subject-ful control. The
// bound preserves the `extra` tolerance above — `when /*c*/ (x) {`
// still finds the subject, `when /*c*/ {` still stops at the brace —
// and degrades to the old whole-child scan only when error recovery
// leaves the brace out entirely, where the answer is unchanged.
fn kotlin_enclosing_when_has_subject<'a>(entry: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent(entry).is_some_and(|when_expression| {
        when_expression
            .children()
            .take_while(|child| child.kind_id() != Kotlin::LBRACE)
            .any(|child| child.kind_id() == Kotlin::WhenSubject)
    })
}

// Scores one non-`else` `when` entry. Both `when` shapes contribute the
// same single decision that cyclomatic counts, but they pay for it in
// different places, and #1421 is the case where ABC charged for it twice.
//
// A subject-ful entry (`when (x) { in 1..2 -> … }`) lists a *pattern*,
// not an independent boolean expression: the decision is the implicit
// `x == pattern`, which nothing in the source spells, so the entry
// itself contributes the condition. A `range_test`, `type_test` or bare
// constant carries no token the comparison arms would count, and #1383's
// logic applies unchanged.
//
// A subject-less entry (`when { x > 5 -> … }`) lists an ordinary boolean
// expression — textually identical to an `if` predicate, and compiled as
// one — so it goes through the same slot `IfExpression` uses. Before
// this the entry added a blanket 1 *on top of* the comparison operator
// its condition already scored through the token arms, so
// `when { x > 5 -> 1; x < 0 -> 2; else -> 0 }` reported 4 conditions
// against a cyclomatic decision count of 2. The slot scores a bare
// terminal (`when { x -> … }`) directly and leaves a comparison, an
// `is` / `in` test (#1461) or an `&&` / `||` chain to the arms that
// already own it. Suppressing the operator instead would have been the wrong half to
// give way: a compound condition `when { a > 1 && b < 2 -> … }` needs
// both its comparisons, and suppression collapses it to one.
//
// The `condition` field is `multiple` — `when { a, b -> … }` lists
// alternatives — and only the first reaches the slot. That is
// deliberate: cyclomatic scores a multi-alternative entry as one
// decision, so routing every alternative through the slot would move
// `when { x, y -> … }` off that count rather than onto it. Alternatives
// after the first are still seen by the token arms.
fn kotlin_count_when_entry<'a>(
    entry: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    conditions: &mut f64,
) {
    if kotlin_enclosing_when_has_subject(entry, ancestors) {
        *conditions += 1.;
    } else if let Some(condition) = entry.child_by_field_name("condition") {
        kotlin_count_condition(&condition, code, conditions);
    }
}

impl Abc for KotlinCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // bca: suppress(cyclomatic, halstead) — exhaustive kind dispatch table
        // One arm per grammar kind, like `CppCode::compute`: the count is
        // the number of node kinds the Kotlin grammar can hand us, not
        // branching a reader must hold, and the effort is the table's
        // vocabulary; the two row reads (#1695) are arms like the token
        // arms they replaced.
        use Kotlin::*;

        let kind = node.kind_id();
        match kind.into() {
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
            // `ConstructorDelegationCall` is a secondary constructor's
            // `: super(…)` / `: this(…)`. It is a distinct production
            // rather than a `CallExpression`, so it scored zero while the
            // same source in Java, C# and Groovy scores one (#1279).
            _ if call::owns_kind(kind, node, code, ancestors)
                || kind == ConstructorDelegationCall as u16 =>
            {
                stats.branches += 1.;
            }
            // `ConstructorInvocation` is the *primary*-constructor
            // superclass call — `class Sub : Base(1, 2)`, and the same
            // production inside `object : Base(1) { }`. #1279 added the
            // secondary form above and left this one at zero (#1384).
            //
            // The parent gate is not optional. tree-sitter-kotlin-ng gives
            // the production exactly three parents (node-types.json), and
            // two of them are annotations: `@Suppress("x")` parses as
            // `annotation > constructor_invocation` and `@file:Suppress("x")`
            // as `file_annotation > constructor_invocation`, so an ungated
            // arm bills every argument-carrying annotation as a branch.
            // Gating *positively* on the delegation specifier — rather than
            // denying the two annotation kinds — keeps any future
            // annotation-shaped parent at zero too. A supertype with no
            // argument list (`class Sub : Marker`) is a plain `user_type`
            // and never reaches here.
            //
            // Where this branch lands is decided by the space tree, and it
            // is not where the secondary spelling above lands. The
            // superclass call sits in the class declaration's
            // `delegation_specifiers` — outside every member, and a
            // *sibling* of the `primary_constructor` node rather than its
            // child — so the innermost enclosing space is the **class**.
            // (Verified by perturbation: labelling `PrimaryConstructor` a
            // `SpaceKind::Function` moves nothing, precisely because the
            // delegation specifier is not inside it.) A
            // `SecondaryConstructor`, which `KotlinCode::get_space_kind`
            // does label a function, takes its `: super(x)` with it.
            // File-level `branches_sum` agrees between the two spellings;
            // per-function `branches_max` / `branches_average` and a
            // per-space `bca check --threshold abc=N` do not. That is the
            // same intended attribution C# has, stated at length in
            // `src/metrics/abc/csharp.rs`, and is pinned here by
            // `kotlin_primary_constructor_superclass_call_is_a_branch`
            // (#1456).
            ConstructorInvocation
                if ancestors.parent_has_kind(node, DelegationSpecifier as u16) =>
            {
                stats.branches += 1.;
            }
            // An enum entry carrying constructor arguments — `A(1)` in
            // `enum class E(val v: Int) { A(1), B(2) }` — invokes the enum's
            // constructor, so it is an object construction under
            // Fitzpatrick's "function invocation or object construction"
            // rule. It scored zero until #1407, which is the inconsistent
            // position once #1279 and #1384 decided the two sibling
            // delegation forms (`: this(…)` and `class Sub : Base(1, 2)`):
            // all three are a constructor call the source spells out.
            //
            // The counter-argument is that an enum entry is a declaration,
            // not a call site a reader navigates to. It loses because the
            // same is true of `class Sub : Base(1, 2)` — also a declaration
            // — and because the arguments still have to be understood as a
            // constructor's, which is the effort ABC is measuring.
            //
            // The child gate is what makes this a §6 narrowing rather than
            // a new node: a bare `B` is `enum_entry > identifier` with no
            // `value_arguments` child and must stay at zero, as must every
            // entry of an enum with no constructor at all (`enum class E {
            // A, B }`). Verified with `bca dump`. `value_arguments` is the
            // only argument-list production the entry can carry, and a
            // Kotlin `annotation` is a `constructor_invocation` above, not
            // an entry, so nothing else satisfies the gate. An argument
            // that is itself a call (`A(f())`) scores 2: `value_arguments`
            // is not a branch node, so the inner `call_expression` is the
            // only other node counted (no double count, §5).
            // role-bypass: allow(DocTarget) — whether the entry calls a constructor,
            // an ABC branch reading, not whether it takes a doc comment
            EnumEntry if node.is_child(ValueArguments as u16) => {
                stats.branches += 1.;
            }
            // Conditions: the ternary-elvis (`?:`, the `NullSafe` row's),
            // the `as?` safe-cast, and the arms of control-flow
            // constructs (`else`, `catch`, `when` entries); the
            // comparison, identity and `is` / `in` tests are the
            // `Comparison` row's, below. Kotlin's `if`-expression does
            // not need an extra count for the `if` keyword itself —
            // Fitzpatrick counts the *conditions*, and the unary
            // condition is already implicit in the boolean operand. We
            // add the `if` arm via the `Else` keyword for else-branches
            // and via `WhenEntry` for `when`. `Try` is the `try` keyword
            // token of a `try_expression`. Fitzpatrick counts both `try`
            // and `catch` as conditions, and Java / C# / C++ / Groovy
            // already count both; Kotlin previously counted only
            // `CatchBlock`, so `try {} catch (e) {}` scored one fewer
            // condition here than in every sibling (#696).
            //
            // No double count with the `when` arms below (§5). A
            // subject-ful entry pays its own condition, and `bca dump`
            // shows its patterns spelled `range_test` / `type_test` —
            // separate productions whose `is` / `in` the row's gate
            // leaves out. A subject-less entry routes its condition
            // through `kotlin_count_condition`, which declines an `is` /
            // `in` test the way it declines a comparison, leaving it to
            // the row's arm.
            Try | AsQMARK => {
                stats.conditions += 1.;
            }
            // And the `catch_block`, the `Catch` row's owner and leaf
            // alike (#1697).
            _ if catch::pays_at_leaf_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            // The elvis `?:`, the one `NullSafe` payer that is a boolean
            // test, once per token; the safe navigation's `?.` is no test
            // and ABC does not score it.
            // role-bypass: allow(NullSafe) — which of the row's two payers this consumer scores:
            // the elvis, not the safe navigation
            _ if kind == QMARKCOLON as u16
                && null_safe::pays_at_leaf_kind(kind, node, code, ancestors) =>
            {
                stats.conditions += 1.;
            }
            // The `Comparison` row's payers (`roles/kotlin.rs`): the six
            // equality and ordering tokens, `<` / `>` gated to a
            // `binary_expression` — tree-sitter-kotlin-ng 1.1.0 emits a
            // bare `<` / `>` in exactly four productions
            // (`binary_expression`, `super_expression`, `type_arguments`,
            // `type_parameters`), so the gate is the allowlist polarity
            // C / C++ / Rust / Go / Java use (#1297); a generic *call*
            // this grammar misparses as nested `binary_expression`s still
            // scores two (#1394) — and `is` / `!is` / `in` / `!in` gated
            // to their expressions, since the `when` entry forms and the
            // `for` header spell the same keywords. Fitzpatrick Rule 5
            // scores the operator wherever it is written (#1461).
            _ if comparison::pays_at_leaf_kind(kind, node, code, ancestors) => {
                stats.conditions += 1.;
            }
            // Phase-2B condition slot: the bare predicate of an
            // `if`/`while`/`do-while` is one unary condition. The
            // `condition` field locates the predicate position-
            // independently across all three forms. `kotlin_count_condition`
            // peels paren / `!` / `!!` / `as` wrappers and pays for any
            // predicate `kotlin_condition_scores_itself` does not name; a
            // comparison or `&&`/`||` chain pays through its own arms
            // instead — so no double-count (#773, #1526).
            // Every `Branch` owner, and the `Loop` owners with a `condition`
            // field — `for` iterates and has none.
            // role-bypass: allow(Loop) — the loop owners with a `condition`
            // slot, a subset the row cannot express; `Branch` is read in full
            _ if branch::owns_kind(kind, node, code, ancestors)
                || matches!(kind.into(), WhileStatement | DoWhileStatement) =>
            {
                if let Some(condition) = node.child_by_field_name("condition") {
                    kotlin_count_condition(&condition, code, &mut stats.conditions);
                }
            }
            // A `when` entry is a decision point except for the `else ->`
            // fallback arm, which is the analogue of C-family `default:`
            // and Rust's wildcard `_ =>`. The `CaseArm` row leaves it
            // out, and cyclomatic reads the same row, which is what keeps
            // the two metrics' decision counts together (#456, lesson
            // 11). `kotlin_count_when_entry` decides where the decision is
            // paid for — the entry, or the operators of its condition
            // (#1421).
            _ if case_arm::owns_kind(kind, node, code, ancestors) => {
                kotlin_count_when_entry(node, code, ancestors, &mut stats.conditions);
            }
            // `else` is a keyword token used in both `if_expression`'s
            // else-clause and `when`'s `else ->` entry. Only count it
            // when it belongs to an `if_expression`; the `WhenEntry`
            // wrapper above already covers the `when` case.
            Else if ancestors.parent_has_kind(node, IfExpression as u16) => {
                stats.conditions += 1.;
            }
            // Fitzpatrick Rule 9 walker: each non-comparison operand of a
            // `&&` / `||` chain is one condition (issue #557). The short-
            // circuit operators are not counted directly (cross-language
            // policy, #395); the walker fires off the operator token and
            // inspects the parent `binary_expression`. `a && b || c` is a
            // left-nested chain, so an operand that is itself a chain is
            // paid by its own operator's visit.
            _ if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) => {
                if let Some(parent) = ancestors.parent(node) {
                    count_field_operands(
                        &parent,
                        |operand, conds| kotlin_count_condition(operand, code, conds),
                        &mut stats.conditions,
                    );
                }
            }
            _ => {}
        }
    }
}
