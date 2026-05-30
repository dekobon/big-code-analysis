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
// Re-exported below via `pub(crate) use` and again from
// `macros/mod.rs`, so every existing `crate::macros::<name>` import in
// `checker.rs`, `metrics/npa.rs`, and `metrics/abc.rs` keeps resolving
// unchanged.

// Aliased C# `kind_id` unions. The C# tree-sitter grammar emits multiple
// numbered variants for several rules (lesson #2 in
// `docs/development/lessons_learned.md`); centralizing the alias sets
// here keeps every match site in lockstep, so a future grammar bump that
// adds another numbered variant is a one-line edit instead of a scatter
// of 4-5 sites.
macro_rules! csharp_invocation_expr_kinds {
    () => {
        $crate::Csharp::InvocationExpression
            | $crate::Csharp::InvocationExpression2
            | $crate::Csharp::InvocationExpression3
    };
}

macro_rules! csharp_paren_expr_kinds {
    () => {
        $crate::Csharp::ParenthesizedExpression
            | $crate::Csharp::ParenthesizedExpression2
            | $crate::Csharp::ParenthesizedExpression3
    };
}

macro_rules! csharp_prefix_unary_expr_kinds {
    () => {
        $crate::Csharp::PrefixUnaryExpression | $crate::Csharp::PrefixUnaryExpression2
    };
}

// Terminal-bool operand kinds recognised by ABC condition counting for
// the C# grammar. Anything in this set, when it appears in a known-
// boolean context (if / while / do / for / ternary / binary), counts
// as one condition. The set bundles `csharp_invocation_expr_kinds!()`
// with the bare `Identifier` / `BooleanLiteral` leaves *and* the five
// expression kinds whose evaluated value is implicitly boolean in any
// idiomatic codebase:
//
// - `MemberAccessExpression` — `cfg.Enabled`, `Request.IsHttps`
// - `AwaitExpression`        — `await CheckAsync()`
// - `CastExpression`         — `(bool)v`, `(IDisposable)x is not null`
// - `IsPatternExpression`    — `x is null`, `x is not Foo f`
// - `ElementAccessExpression` — `flags[0]`, `dict["key"]`
//
// Before #372 only the first three (invocation / identifier /
// boolean) were recognised, so all five kinds above silently scored
// zero conditions in `if` / `while` / `do` / ternary contexts.
macro_rules! csharp_bool_terminal_kinds {
    () => {
        $crate::Csharp::InvocationExpression
            | $crate::Csharp::InvocationExpression2
            | $crate::Csharp::InvocationExpression3
            | $crate::Csharp::Identifier
            | $crate::Csharp::BooleanLiteral
            | $crate::Csharp::MemberAccessExpression
            | $crate::Csharp::AwaitExpression
            | $crate::Csharp::CastExpression
            | $crate::Csharp::IsPatternExpression
            | $crate::Csharp::ElementAccessExpression
    };
}

macro_rules! csharp_var_decl_kinds {
    () => {
        $crate::Csharp::VariableDeclaration | $crate::Csharp::VariableDeclaration2
    };
}

macro_rules! csharp_var_declarator_kinds {
    () => {
        $crate::Csharp::VariableDeclarator | $crate::Csharp::VariableDeclarator2
    };
}

// Terminal-bool operand kinds recognised by ABC condition counting for
// the Java grammar. Sister of `csharp_bool_terminal_kinds!()` — bundles
// the four "bare boolean leaf" kinds (`MethodInvocation`, `Identifier`,
// `True`, `False`) with the four bool-evaluating expression kinds
// surfaced by #372 / lesson #19:
//
// - `FieldAccess`          — `cfg.flag`
// - `CastExpression`       — `(boolean) v`
// - `ArrayAccess`          — `flags[0]`
// - `InstanceofExpression` — `x instanceof Foo`
//
// Used by `java_inspect_container`, `java_count_unary_conditions`,
// `java_walk_ternary`, and the two branches of `java_walk_for_statement`
// (the latter ORs in `SEMI | RPAREN` at the call site to also recognise
// the empty-condition `for (;;)` form).
macro_rules! java_bool_terminal_kinds {
    () => {
        $crate::Java::MethodInvocation
            | $crate::Java::Identifier
            | $crate::Java::True
            | $crate::Java::False
            | $crate::Java::FieldAccess
            | $crate::Java::CastExpression
            | $crate::Java::ArrayAccess
            | $crate::Java::InstanceofExpression
    };
}

// Terminal-bool operand kinds recognised by ABC condition counting for
// the dekobon Groovy grammar. Sister of `java_bool_terminal_kinds!()`,
// with Groovy-specific replacements: `CommandChain` for the parens-less
// call form `println foo`, `BooleanLiteral` (the named wrapper around
// the leaf `True` / `False` tokens, see `groovy_count_condition`), and
// `ParenthesizedTypeCast` for the Java-style `(boolean) v` form (the
// grammar represents it as its own kind rather than nesting
// `cast_expression` inside `parenthesized_expression`). The set bundles
// the bool-evaluating terminals added by #372 (`FieldAccess`,
// `CastExpression`, `ParenthesizedTypeCast`, `InstanceofExpression`);
// the dekobon Groovy grammar has no `await` or `array_access`
// analogues, so those collapse out of the C# set.
macro_rules! groovy_bool_terminal_kinds {
    () => {
        $crate::Groovy::MethodInvocation
            | $crate::Groovy::CommandChain
            | $crate::Groovy::Identifier
            | $crate::Groovy::BooleanLiteral
            | $crate::Groovy::FieldAccess
            | $crate::Groovy::CastExpression
            | $crate::Groovy::ParenthesizedTypeCast
            | $crate::Groovy::InstanceofExpression
    };
}

// Terminal-bool operand kinds for the Phase-2 unary-conditional walker
// (issue #403). Each `<lang>_bool_terminal_kinds!()` macro lists the
// expression kinds whose evaluated value is implicitly boolean in an
// `if` / `while` / `&&` / `||` operand slot for that language. Each
// per-language walker pair (`<lang>_inspect_container` +
// `<lang>_count_unary_conditions`) consumes the same set in both
// helpers, so hoisting to a macro removes the literal duplication.

macro_rules! rust_bool_terminal_kinds {
    // `ScopedIdentifier` (`crate::FLAG`, `ns::flag`) and
    // `AwaitExpression` (`ready().await`) are both idiomatic shapes
    // for a boolean-valued condition operand. Adding them mirrors
    // the C# fix in #372 (lesson 19), which closed the same gap
    // for `CastExpression`, `MemberAccessExpression`, and
    // `AwaitExpression` on the C# side.
    () => {
        $crate::Rust::Identifier
            | $crate::Rust::BooleanLiteral
            | $crate::Rust::CallExpression
            | $crate::Rust::FieldExpression
            | $crate::Rust::IndexExpression
            | $crate::Rust::ScopedIdentifier
            | $crate::Rust::AwaitExpression
    };
}

macro_rules! go_bool_terminal_kinds {
    // Aliased Identifier kind_ids (lesson #2): tree-sitter-go emits
    // `identifier` under three numeric ids (1, 60, 61) depending on
    // the production rule path. Halstead's getter already matches
    // all three at `src/getter.rs:881`.
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

macro_rules! cpp_bool_terminal_kinds {
    // `QualifiedIdentifier` has four numeric kind_ids (573..576) per
    // tree-sitter-cpp's production-rule path. Halstead's getter
    // already matches all four; the ABC walker needs them too so
    // `if (ns::flag) {}` reaches the terminal-bool count.
    //
    // `CastExpression` (`(bool)v`) evaluates to a boolean in
    // idiomatic C++ — mirrors the C# fix in #372 (lesson 19).
    () => {
        $crate::Cpp::Identifier
            | $crate::Cpp::True
            | $crate::Cpp::False
            | $crate::Cpp::CallExpression
            | $crate::Cpp::CallExpression2
            | $crate::Cpp::FieldExpression
            | $crate::Cpp::SubscriptExpression
            | $crate::Cpp::CastExpression
            | $crate::Cpp::QualifiedIdentifier
            | $crate::Cpp::QualifiedIdentifier2
            | $crate::Cpp::QualifiedIdentifier3
            | $crate::Cpp::QualifiedIdentifier4
    };
}

macro_rules! php_bool_terminal_kinds {
    // Aliased kind_ids (lesson 2):
    //   - `name` has two ids (1, 211)
    //   - `member_access_expression` has three (328, 329, 360)
    //   - `nullsafe_member_access_expression` has two (330, 331)
    //   - `scoped_property_access_expression` has two (332, 333)
    //   - `subscript_expression` has three (351, 352, 363)
    // The matching `*_call_expression` kinds remain singular at the
    // pinned grammar version. Including the property-access form
    // (`$x?->y`, `$x->y`, and `Cls::$x`) closes the bool-typed-
    // property-access gap that the call-form alone left open.
    () => {
        $crate::Php::Name
            | $crate::Php::Name2
            | $crate::Php::VariableName
            | $crate::Php::Boolean
            | $crate::Php::FunctionCallExpression
            | $crate::Php::MemberCallExpression
            | $crate::Php::ScopedCallExpression
            | $crate::Php::NullsafeMemberCallExpression
            | $crate::Php::ObjectCreationExpression
            | $crate::Php::MemberAccessExpression
            | $crate::Php::MemberAccessExpression2
            | $crate::Php::MemberAccessExpression3
            | $crate::Php::NullsafeMemberAccessExpression
            | $crate::Php::NullsafeMemberAccessExpression2
            | $crate::Php::ScopedPropertyAccessExpression
            | $crate::Php::ScopedPropertyAccessExpression2
            | $crate::Php::SubscriptExpression
            | $crate::Php::SubscriptExpression2
            | $crate::Php::SubscriptExpression3
    };
}

macro_rules! python_bool_terminal_kinds {
    // `Await` (`await ready()`) evaluates to a boolean in idiomatic
    // async Python — mirrors the C# fix in #372 (lesson 19) which
    // closed the same gap for `AwaitExpression`.
    () => {
        $crate::Python::Identifier
            | $crate::Python::True
            | $crate::Python::False
            | $crate::Python::Call
            | $crate::Python::Attribute
            | $crate::Python::Subscript
            | $crate::Python::Await
    };
}

macro_rules! perl_bool_terminal_kinds {
    () => {
        $crate::Perl::Identifier
            | $crate::Perl::Boolean
            | $crate::Perl::True
            | $crate::Perl::False
            | $crate::Perl::ScalarVariable
            | $crate::Perl::ArrayVariable
            | $crate::Perl::HashVariable
            | $crate::Perl::ArrayAccessVariable
            | $crate::Perl::HashAccessVariable
            | $crate::Perl::HashAccessVariableSimple
            | $crate::Perl::CallExpressionWithSpacedArgs
            | $crate::Perl::CallExpressionWithSub
            | $crate::Perl::CallExpressionWithArgsWithBrackets
            | $crate::Perl::CallExpressionWithVariable
            | $crate::Perl::CallExpressionRecursive
            | $crate::Perl::CallExpressionWithBareword
            | $crate::Perl::MethodInvocation
    };
}

macro_rules! lua_bool_terminal_kinds {
    () => {
        $crate::Lua::Identifier
            | $crate::Lua::True
            | $crate::Lua::False
            | $crate::Lua::Nil
            | $crate::Lua::Number
            | $crate::Lua::FunctionCall
            | $crate::Lua::DotIndexExpression
            | $crate::Lua::DotIndexExpression2
            | $crate::Lua::BracketIndexExpression
            | $crate::Lua::MethodIndexExpression
            | $crate::Lua::MethodIndexExpression2
    };
}

macro_rules! tcl_bool_terminal_kinds {
    () => {
        $crate::Tcl::SimpleWord
            | $crate::Tcl::BracedWord
            | $crate::Tcl::BracedWordSimple
            | $crate::Tcl::QuotedWord
            | $crate::Tcl::VariableSubstitution
            | $crate::Tcl::CommandSubstitution
            | $crate::Tcl::Boolean
            | $crate::Tcl::Number
    };
}

// The JS-family languages diverge on which aliased `kind_id`s the
// grammar emits — JavaScript, Mozjs, and Tsx have `Identifier2`,
// TypeScript does not; TypeScript has `MemberExpression4` /
// `CallExpression4` / `SubscriptExpression2` that the others do not.
// Per lesson #2, every alias the grammar emits at runtime must be
// matched at compile time. Four per-language macros below replace
// the original single `js_family_bool_terminal_kinds!($Lang)`
// generic, which silently dropped `MemberExpression2` (the kind
// runtime emits for `obj.foo`) for all four languages.

macro_rules! javascript_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19).
    () => {
        $crate::Javascript::Identifier
            | $crate::Javascript::Identifier2
            | $crate::Javascript::True
            | $crate::Javascript::False
            | $crate::Javascript::CallExpression
            | $crate::Javascript::CallExpression2
            | $crate::Javascript::NewExpression
            | $crate::Javascript::MemberExpression
            | $crate::Javascript::MemberExpression2
            | $crate::Javascript::MemberExpression3
            | $crate::Javascript::SubscriptExpression
            | $crate::Javascript::AwaitExpression
    };
}

macro_rules! mozjs_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19).
    () => {
        $crate::Mozjs::Identifier
            | $crate::Mozjs::Identifier2
            | $crate::Mozjs::True
            | $crate::Mozjs::False
            | $crate::Mozjs::CallExpression
            | $crate::Mozjs::CallExpression2
            | $crate::Mozjs::NewExpression
            | $crate::Mozjs::MemberExpression
            | $crate::Mozjs::MemberExpression2
            | $crate::Mozjs::MemberExpression3
            | $crate::Mozjs::SubscriptExpression
            | $crate::Mozjs::AwaitExpression
    };
}

macro_rules! typescript_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19).
    () => {
        $crate::Typescript::Identifier
            | $crate::Typescript::True
            | $crate::Typescript::False
            | $crate::Typescript::CallExpression
            | $crate::Typescript::CallExpression2
            | $crate::Typescript::CallExpression3
            | $crate::Typescript::CallExpression4
            | $crate::Typescript::NewExpression
            | $crate::Typescript::MemberExpression
            | $crate::Typescript::MemberExpression2
            | $crate::Typescript::MemberExpression3
            | $crate::Typescript::MemberExpression4
            | $crate::Typescript::SubscriptExpression
            | $crate::Typescript::SubscriptExpression2
            | $crate::Typescript::AwaitExpression
    };
}

macro_rules! tsx_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19).
    () => {
        $crate::Tsx::Identifier
            | $crate::Tsx::Identifier2
            | $crate::Tsx::True
            | $crate::Tsx::False
            | $crate::Tsx::CallExpression
            | $crate::Tsx::CallExpression2
            | $crate::Tsx::CallExpression3
            | $crate::Tsx::CallExpression4
            | $crate::Tsx::NewExpression
            | $crate::Tsx::MemberExpression
            | $crate::Tsx::MemberExpression2
            | $crate::Tsx::MemberExpression3
            | $crate::Tsx::MemberExpression4
            | $crate::Tsx::SubscriptExpression
            | $crate::Tsx::SubscriptExpression2
            | $crate::Tsx::AwaitExpression
    };
}

// Legacy single-macro form, no longer consumed by the walker after
// the per-language split above. Kept here strictly for documentation
// of the former (Identifier|True|False|CallExpression|NewExpression|
// MemberExpression|SubscriptExpression) intersection that all four
// JS-family languages share — every per-language macro above is a
// strict superset.
#[allow(unused_macros)]
macro_rules! js_family_bool_terminal_kinds {
    ($Lang:ident) => {
        $crate::$Lang::Identifier
            | $crate::$Lang::True
            | $crate::$Lang::False
            | $crate::$Lang::CallExpression
            | $crate::$Lang::NewExpression
            | $crate::$Lang::MemberExpression
            | $crate::$Lang::SubscriptExpression
    };
}

pub(crate) use {
    cpp_bool_terminal_kinds, csharp_bool_terminal_kinds, csharp_invocation_expr_kinds,
    csharp_paren_expr_kinds, csharp_prefix_unary_expr_kinds, csharp_var_decl_kinds,
    csharp_var_declarator_kinds, go_bool_terminal_kinds, groovy_bool_terminal_kinds,
    java_bool_terminal_kinds, javascript_bool_terminal_kinds, lua_bool_terminal_kinds,
    mozjs_bool_terminal_kinds, perl_bool_terminal_kinds, php_bool_terminal_kinds,
    python_bool_terminal_kinds, rust_bool_terminal_kinds, tcl_bool_terminal_kinds,
    tsx_bool_terminal_kinds, typescript_bool_terminal_kinds,
};
