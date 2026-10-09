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
// Only Go still pays a slot through such a list. Every other language's
// slot pays one condition unless an arm inside the predicate already did
// (`count_boolean_slot` in `src/metrics/abc.rs`, #1526; Kotlin #1533),
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
// (`.claude/rules/grammar-dispatch.md` §5).
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
