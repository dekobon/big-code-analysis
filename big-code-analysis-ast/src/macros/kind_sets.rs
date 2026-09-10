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
#[macro_export]
#[doc(hidden)]
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
#[macro_export]
#[doc(hidden)]
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
//
// FIXME(#1410): Groovy truth makes every non-zero number truthy, so this
// set is missing the numeric literal kinds — `a && 1` scores one
// condition where `a && b` scores two. Deferred out of #1379 because the
// integration corpora carry Groovy files and the fix moves snapshots.
#[macro_export]
#[doc(hidden)]
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

#[macro_export]
#[doc(hidden)]
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

#[macro_export]
#[doc(hidden)]
macro_rules! cpp_bool_terminal_kinds {
    // Matches on node-kind NAMES, not one grammar's enum discriminants,
    // so it is correct for every C-family grammar: the shared ABC helpers
    // run over both upstream `Cpp` and the Mozilla `Mozcpp` fork (#720),
    // which assign *different* kind_ids to the same kinds (#732, mirroring
    // the npa fix in #731). `qualified_identifier` has four numeric ids
    // (573..576) in tree-sitter-cpp's production-rule path, but all four
    // render to the one base name, so a single arm covers them — the
    // ABC walker needs them so `if (ns::flag) {}` reaches the terminal
    // count. `cast_expression` (`(bool)v`) evaluates to a boolean in
    // idiomatic C++ — mirrors the C# fix in #372 (lesson 19).
    // `message_expression` is Objective-C's call spelling (`[obj ok]`),
    // the twin of `call_expression`: the ObjC ABC impl already counts
    // it as a Branch beside `call_expression`, and no C / C++ / Mozcpp
    // grammar has a node by that name, so the arm is inert there.
    // Without it every `if ([a ok])` / `for (; [a ok]; )` scored zero
    // conditions where `if (ok())` scored one.
    () => {
        "identifier"
            | "true"
            | "false"
            | "call_expression"
            | "message_expression"
            | "field_expression"
            | "subscript_expression"
            | "cast_expression"
            | "qualified_identifier"
    };
}

// FIXME(#1410): PHP treats every non-zero number as truthy, so this set
// is missing the numeric literal kinds — `$a && 1` scores one condition
// where `$a && $b` scores two. Deferred out of #1379 because the
// integration corpora carry PHP files and the fix moves snapshots.
#[macro_export]
#[doc(hidden)]
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

#[macro_export]
#[doc(hidden)]
macro_rules! python_bool_terminal_kinds {
    // `Await` (`await ready()`) evaluates to a boolean in idiomatic
    // async Python — mirrors the C# fix in #372 (lesson 19) which
    // closed the same gap for `AwaitExpression`.
    //
    // `Integer` / `Float` are numeric-truthy operands: Python treats
    // every non-zero number as truthy, so `if 5:` / `x and 5` each
    // count their numeric literal as a Fitzpatrick unary condition.
    // Mirrors the Lua `Number` fix (#772). Statically-typed languages
    // omit numerics (a bare int in a bool slot is a type error); a
    // dynamically-typed language must count them.
    () => {
        $crate::Python::Identifier
            | $crate::Python::True
            | $crate::Python::False
            | $crate::Python::Integer
            | $crate::Python::Float
            | $crate::Python::Call
            | $crate::Python::Attribute
            | $crate::Python::Subscript
            | $crate::Python::Await
    };
}

// Terminal-bool operand kinds for Perl's ABC unary-conditional walker
// (Fitzpatrick Rule 9; issue #557): the bare boolean operands of a
// `binary_expression` short-circuit chain and of an `if` / `while` /
// `unless` / `until` / ternary / C-style-`for` condition slot.
//
// Perl is truthy-valued — every scalar but `0`, `"0"`, `""` and `undef`
// is true — so a numeric literal is a unary condition here for the same
// reason `Number` is in Lua and JavaScript (#772) and `Integer` /
// `Float` are in Python. Naming none of them scored `$a && 1` one
// condition against Python's two for `a and 1`, and `if (1)` zero
// against Python's one for `if 1:` (#1379).
//
// **The unit to check is the supertype, not the alias list.**
// tree-sitter-perl 1.1.2 has no numeric-suffix aliases at all, so an
// alias sweep (grammar-dispatch §1) comes back clean and proves nothing:
// the numerals are five *sibling rules* under the hidden
// `_numeric_literals` choice (`Perl::NumericLiterals`) — `integer`,
// `floating_point`, `scientific_notation`, `hexadecimal`, `octal`, ids
// 128-132. #1379 first landed with only the first two, leaving `$a &&
// 0xff` and `$a && 1.5e10` scoring 1 against `$a && $b`'s 2. Read the
// supertype's arm list before calling such a set complete.
//
// `octal` is currently unreachable and listed defensively: the lexer
// resolves `017` to `integer` (verified by `bca dump`), and `0o17` is
// not Perl syntax — it parses as a bareword call. A future grammar that
// starts emitting it should count it, so there is nothing to guard
// against by omission (grammar-dispatch §2).
//
// The statically-typed sets (C#, Java, Kotlin, Rust, Go, C, C++)
// deliberately name no numeric kind: a bare number in a boolean slot is
// a compile error there, so there is nothing to count. PHP and Groovy
// are the two remaining truthy-valued languages that still omit one —
// tracked in #1410, not deliberate.
#[macro_export]
#[doc(hidden)]
macro_rules! perl_bool_terminal_kinds {
    () => {
        $crate::Perl::Identifier
            | $crate::Perl::Boolean
            | $crate::Perl::True
            | $crate::Perl::False
            | $crate::Perl::Integer
            | $crate::Perl::FloatingPoint
            | $crate::Perl::ScientificNotation
            | $crate::Perl::Hexadecimal
            | $crate::Perl::Octal
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

// Lua's `number` is one kind for the integer and the float spelling
// alike, so `a and 1` and `a and 1.0` both score through `Number` and
// the language has no counterpart of the #1379 Ruby / Elixir / Perl gap.
// The same holds for Tcl, iRules and the four JS-family sets, each
// measured rather than read off the grammar.
#[macro_export]
#[doc(hidden)]
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

#[macro_export]
#[doc(hidden)]
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

// iRules counterpart of `tcl_bool_terminal_kinds!` (the grammar is a Tcl
// dialect, so the terminal-operand set is the same shape).
#[macro_export]
#[doc(hidden)]
macro_rules! irules_bool_terminal_kinds {
    () => {
        $crate::Irules::SimpleWord
            | $crate::Irules::BracedWord
            | $crate::Irules::BracedWordSimple
            | $crate::Irules::QuotedWord
            | $crate::Irules::VariableSubstitution
            | $crate::Irules::CommandSubstitution
            | $crate::Irules::Boolean
            | $crate::Irules::Number
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

#[macro_export]
#[doc(hidden)]
macro_rules! javascript_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19). `Number` is a
    // numeric-truthy operand: JS treats every non-zero number as
    // truthy, so `while (5)` / `x && 5` count their numeric literal
    // as a Fitzpatrick unary condition (#772, mirrors the Lua fix).
    () => {
        $crate::Javascript::Identifier
            | $crate::Javascript::Identifier2
            | $crate::Javascript::True
            | $crate::Javascript::False
            | $crate::Javascript::Number
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

#[macro_export]
#[doc(hidden)]
macro_rules! mozjs_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19). `Number` is a
    // numeric-truthy operand (#772, mirrors the Lua fix) — see
    // `javascript_bool_terminal_kinds!`.
    () => {
        $crate::Mozjs::Identifier
            | $crate::Mozjs::Identifier2
            | $crate::Mozjs::True
            | $crate::Mozjs::False
            | $crate::Mozjs::Number
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

#[macro_export]
#[doc(hidden)]
macro_rules! typescript_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19). `Number` (the numeric
    // *literal*, id 110) is a numeric-truthy operand (#772). The
    // grammar's other `number` alias, `Number2` (id 133), is the
    // `predefined_type` keyword `number` in a type annotation — NOT a
    // value — so it is deliberately omitted from the terminal-bool set.
    () => {
        $crate::Typescript::Identifier
            | $crate::Typescript::True
            | $crate::Typescript::False
            | $crate::Typescript::Number
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

#[macro_export]
#[doc(hidden)]
macro_rules! tsx_bool_terminal_kinds {
    // `AwaitExpression` (`await ready()`) is in the terminal set
    // mirroring the C# reference (lesson 19). `Number` (the numeric
    // *literal*, id 116) is a numeric-truthy operand (#772). The
    // grammar's other `number` alias, `Number2` (id 139), is the
    // `predefined_type` keyword `number` in a type annotation — NOT a
    // value — so it is deliberately omitted from the terminal-bool set.
    () => {
        $crate::Tsx::Identifier
            | $crate::Tsx::Identifier2
            | $crate::Tsx::True
            | $crate::Tsx::False
            | $crate::Tsx::Number
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
#[macro_export]
#[doc(hidden)]
macro_rules! kotlin_bool_terminal_kinds {
    () => {
        $crate::Kotlin::Identifier
            | $crate::Kotlin::CallExpression
            | $crate::Kotlin::NavigationExpression
            | $crate::Kotlin::IndexExpression
            | $crate::Kotlin::ThisExpression
    };
}

// Terminal-bool operand kinds for Ruby's ABC unary-conditional walker
// (Fitzpatrick Rule 9; issue #557). tree-sitter-ruby parses `a && b` as
// a `binary` node with `&&` / `||` / `and` / `or` operator tokens. Bare
// boolean operands surface as: `identifier`, every `call` alias
// (`Call`..`Call4` — lesson #2; a bare predicate method `ready?` is a
// `call`), the literals `true` / `false` / `nil`, the variable sigils
// (`@ivar`, `@@cvar`, `$gvar`), `constant`, `element_reference`
// (`items[0]`), and the four numeric literal kinds `integer` / `float` /
// `rational` / `complex`. Comparison operands (`x > 0`) are nested
// `binary` nodes, so they are absent here and contribute nothing.
//
// Ruby is truthy-valued — every number including `0` and `0.0` is
// truthy — so a bare numeric operand is a Fitzpatrick unary condition
// exactly as it is in Python and Lua (#772). Listing `integer` alone
// scored `a && 1.0` / `a && 1r` / `a && 2i` one condition where
// `a && 1` scores two (#1379).
//
// `rational` and `complex` are WRAPPERS over the numeral (`1r` is
// `rational(integer)`, `2i` is `complex(integer)`, `1ri` is
// `complex(rational(integer))` — verified by `bca dump`), and the
// **wrapper** is what has to be listed: `ruby_inspect_container` breaks
// out of its descent for any node that is neither
// `parenthesized_statements` nor a `!` / `not` unary, so the walker
// cannot reach the inner numeral at all. Listing `Integer` alone scores
// all three suffixed literals zero, which is what #1379 measured.
//
// The mirror-image hazard — grammar-dispatch §5's container/contained
// double-count — is absent here for the same reason, and `Integer`
// staying in the set alongside them is not redundancy: it is what scores
// a bare `1`. Do not "simplify" by removing either half. (#1359 reached
// the same keep-the-wrapper answer for Halstead operand identity, where
// the walk *does* visit every node and the double-count is real.)
//
// None of the four kinds has a numeric-suffix alias in tree-sitter-ruby
// 0.23.1; `_int_or_float` (`Ruby::IntOrFloat`) is a hidden supertype the
// parser never emits (grammar-dispatch §2).
#[macro_export]
#[doc(hidden)]
macro_rules! ruby_bool_terminal_kinds {
    () => {
        $crate::Ruby::Identifier
            | $crate::Ruby::Call
            | $crate::Ruby::Call2
            | $crate::Ruby::Call3
            | $crate::Ruby::Call4
            | $crate::Ruby::True
            | $crate::Ruby::False
            | $crate::Ruby::Nil
            | $crate::Ruby::InstanceVariable
            | $crate::Ruby::ClassVariable
            | $crate::Ruby::GlobalVariable
            | $crate::Ruby::Constant
            | $crate::Ruby::ElementReference
            | $crate::Ruby::Integer
            | $crate::Ruby::Float
            | $crate::Ruby::Rational
            | $crate::Ruby::Complex
    };
}

// Terminal-bool operand kinds for Elixir's ABC unary-conditional walker
// (Fitzpatrick Rule 9; issue #557). tree-sitter-elixir parses `a && b`
// as a `binary_operator` (aliased `BinaryOperator`..`BinaryOperator3`,
// lesson #2) with `&&` / `||` / `and` / `or` operator tokens. Bare
// boolean operands surface as: `identifier`, `call` (both `ready?()` and
// the no-paren dot access `cfg.enabled` parse as `call`), `dot`
// (`Mod.fun` reference), the `boolean` literal wrapper (`true` / `false`
// parse as `boolean`, verified by AST dump), `nil`, `atom`, the three
// numeric literal kinds `integer` / `float` / `char`, and `access_call`
// (`xs[i]`). Comparison operands are nested `binary_operator` nodes and
// so contribute nothing.
//
// Elixir's `&&` / `||` are truthy operators (everything but `false` and
// `nil` is truthy), so a bare numeric operand counts as a Fitzpatrick
// unary condition. `integer` alone scored `a && 1.0` one condition where
// `a && 1` scores two (#1379).
//
// The grammar's numeric family is `integer` / `float` / `char`, none of
// them aliased, and there are no rational or complex kinds. `char` is
// here because `?a` **is** an integer in Elixir — it evaluates to the
// codepoint 97 — so it is a numeric literal wearing a sigil, not a
// string; `x && ?a` scored 1 against `x && b`'s 2 until it was listed.
// Radix prefixes (`0x`, `0o`, `0b`) fold into `integer`, verified by
// measurement.
#[macro_export]
#[doc(hidden)]
macro_rules! elixir_bool_terminal_kinds {
    () => {
        $crate::Elixir::Identifier
            | $crate::Elixir::Call
            // `dot` is an alias family (`Dot`/`Dot2`/`Dot3`, lesson #2): a
            // `Mod.fun` reference used as a bare `&&`/`||` operand parses to
            // a different alias by position, so all three must count or the
            // operand silently contributes 0 (mirrors Ruby's `Call..Call4`).
            | $crate::Elixir::Dot
            | $crate::Elixir::Dot2
            | $crate::Elixir::Dot3
            | $crate::Elixir::Boolean
            | $crate::Elixir::Nil
            | $crate::Elixir::Atom
            | $crate::Elixir::Integer
            | $crate::Elixir::Float
            | $crate::Elixir::Char
            | $crate::Elixir::AccessCall
    };
}

// Legacy single-macro form, no longer consumed by the walker after
// the per-language split above. Kept here strictly for documentation
// of the former (Identifier|True|False|CallExpression|NewExpression|
// MemberExpression|SubscriptExpression) intersection that all four
// JS-family languages share — every per-language macro above is a
// strict superset (the per-language sets have since grown
// `AwaitExpression` and other shapes; this body is the historical
// floor, not the current set).
#[allow(unused_macros)]
#[macro_export]
#[doc(hidden)]
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
