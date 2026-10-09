// Per-language `kind_id`-set and alias macros.
//
// Split out of `macros.rs` (now `macros/mod.rs`) so that file is no
// longer dominated, by line count, by these ~20 near-identical
// match-pattern bundles (which sank its maintainability index). These
// macros MUST stay `macro_rules!`: each expands to a `|`-separated
// list of per-language `kind_id` enum variants consumed inside
// `matches!()` at the call site, where a `use <Lang>::*` glob brings
// the variants into scope. They carry no logic -- only the variant
// membership -- and every per-call grammar rationale comment travels
// with its macro (see `.claude/rules/macro-comments.md`).
//
// Re-exported below via `pub use` and again from
// `macros/mod.rs`, so every existing `crate::macros::<name>` import in
// `checker.rs`, `metrics/npa.rs`, and `metrics/abc.rs` keeps resolving
// unchanged.

// Aliased C# `kind_id` unions. The C# tree-sitter grammar emits multiple
// numbered variants for several rules (lesson #2 in
// `docs/development/lessons_learned.md`); centralizing the alias sets
// here keeps every match site in lockstep, so a future grammar bump that
// adds another numbered variant is a one-line edit instead of a scatter
// of 4-5 sites.
#[macro_export]
#[doc(hidden)]
macro_rules! csharp_invocation_expr_kinds {
    () => {
        $crate::Csharp::InvocationExpression
            | $crate::Csharp::InvocationExpression2
            | $crate::Csharp::InvocationExpression3
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! csharp_paren_expr_kinds {
    () => {
        $crate::Csharp::ParenthesizedExpression
            | $crate::Csharp::ParenthesizedExpression2
            | $crate::Csharp::ParenthesizedExpression3
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! csharp_prefix_unary_expr_kinds {
    () => {
        $crate::Csharp::PrefixUnaryExpression | $crate::Csharp::PrefixUnaryExpression2
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! csharp_var_decl_kinds {
    () => {
        $crate::Csharp::VariableDeclaration | $crate::Csharp::VariableDeclaration2
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! csharp_var_declarator_kinds {
    () => {
        $crate::Csharp::VariableDeclarator | $crate::Csharp::VariableDeclarator2
    };
}

// Terminal-bool operand kinds for the Phase-2 unary-conditional walker
// (issue #403). Each `<lang>_bool_terminal_kinds!()` macro lists the
// expression kinds whose evaluated value is implicitly boolean in an
// `if` / `while` / `&&` / `||` operand slot for that language.
//
// Only Go and Kotlin still pay a slot through such a list. Every other
// language's slot pays one condition unless an arm inside the predicate
// already did (`count_boolean_slot` in `src/metrics/abc.rs`, #1526),
// because each list kept missing kinds — `-x`, `x + 1`, `this`, `*p` —
// that scored 0 against a cyclomatic decision of 1.
//
// **These sets hold operands, never operators** (#1461). Membership is
// slot-scoped by construction — a kind here scores only where a walker
// consults the set, which is inside a boolean slot — and that is the
// right scope for a *value*, whose truthiness is interesting only
// because the slot reads it. It is the wrong scope for a **relational
// operator**, which Fitzpatrick Rule 5 scores by use: `a == b` counts
// wherever it is written, so `a is String` must too. The type-test and
// membership productions a grammar spells as their own node sat here
// until #1461 and so scored one inside a predicate and zero outside
// it, while the comparison token beside them scored in both. Each now
// has an unconditional arm in its language's ABC `compute` and is
// listed in neither place twice — which would score it twice
// (`.claude/rules/grammar-dispatch.md` §5). The five affected sets say
// which of their members moved.
//
// The line is the construct's *value*, not its type. A cast, a type
// assertion and an `@available` query all yield something the slot
// then reads as a boolean, so they stay; a type test yields the
// comparison's own result, so it does not.

#[macro_export]
#[doc(hidden)]
macro_rules! go_bool_terminal_kinds {
    // Aliased Identifier kind_ids (lesson #2): tree-sitter-go emits
    // `identifier` under three numeric ids (1, 60, 61) depending on
    // the production rule path. Halstead's getter already matches all
    // three (the `G::Identifier | G::Identifier2 | G::Identifier3` arm
    // in `impl Getter for GoCode::get_op_type`, `src/getter.rs`).
    () => {
        $crate::Go::Identifier
            | $crate::Go::Identifier2
            | $crate::Go::Identifier3
            | $crate::Go::True
            | $crate::Go::False
            | $crate::Go::CallExpression
            | $crate::Go::SelectorExpression
            | $crate::Go::IndexExpression
            | $crate::Go::TypeAssertionExpression
    };
}

// Terminal-bool operand kinds for Kotlin's ABC unary-conditional walker
// (Fitzpatrick Rule 9; issue #557). tree-sitter-kotlin-ng parses `a &&
// b` as a flat `binary_expression` with `&&` / `||` operator tokens, so
// bare boolean operands surface as leaf expressions: `identifier` (which
// also covers the `true` / `false` keyword literals — the grammar emits
// them as `identifier`, verified by AST dump), `call_expression`
// (`ready()`), `navigation_expression` (`o.flag`), `index_expression`
// (`arr[0]`), and `this_expression`. Comparison operands (`x > 0`) are
// themselves `binary_expression` nodes, so they are absent from this set
// and contribute nothing — matching the paper's "only unary conditions".
//
// `infix_expression` is a call to an infix function, and Kotlin spells
// boolean `and` / `or` / `xor` that way — `a and b` is `a.and(b)`. It
// belongs here for the same reason `call_expression` does, and its
// absence was a regression of #1421 rather than a pre-existing gap: the
// blanket per-entry count that fix removed had been covering it, so
// `when { a and b -> … }` fell from one condition to zero while the
// `if (a and b)` it is supposed to agree with still scored one. The set
// does not discriminate on return type — `f()` counts in a boolean slot
// whatever it returns — so a non-boolean infix call in a boolean slot is
// out of scope here for the same reason.
//
// `is_expression` (`a is String`, `a !is String`) and `in_expression`
// (`a in 1..2`, `a !in 1..2`) are the two relational forms the grammar
// spells as their own production rather than as a `binary_expression`,
// so the comparison-token arms never see them. #1421 added them here
// and #1461 moved them to an unconditional arm in
// `src/metrics/abc/kotlin.rs`, slot-scoping having scored
// `val b = a is String` zero beside `val b = a == c`'s one — see the
// operands-not-operators note on the Phase-2 block below.
//
// Nothing else in the Kotlin impl counts either, and the node is the
// half to count rather than the token: `!is` / `!in` are their own
// spellings of the same production, and a bare `in` token is also the
// `for (x in xs)` header's. `bca dump` confirms a subject-ful `when`
// arm spells its patterns `range_test` / `type_test` rather than these
// two, so the entry's own count and this arm never both fire (§5).
#[macro_export]
#[doc(hidden)]
macro_rules! kotlin_bool_terminal_kinds {
    () => {
        $crate::Kotlin::Identifier
            | $crate::Kotlin::CallExpression
            | $crate::Kotlin::NavigationExpression
            | $crate::Kotlin::IndexExpression
            | $crate::Kotlin::ThisExpression
            | $crate::Kotlin::InfixExpression
    };
}
