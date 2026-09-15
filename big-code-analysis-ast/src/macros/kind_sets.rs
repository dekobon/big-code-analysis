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
// with the bare `Identifier` / `BooleanLiteral` leaves *and* the six
// expression kinds whose evaluated value is implicitly boolean in any
// idiomatic codebase:
//
// - `MemberAccessExpression` — `cfg.Enabled`, `Request.IsHttps`
// - `AwaitExpression`        — `await CheckAsync()`
// - `CastExpression`         — `(bool)v`, `(IDisposable)x is not null`
// - `IsPatternExpression`    — `x is null`, `x is not Foo f`
// - `IsExpression`           — `x is int`, the bare type test
// - `ElementAccessExpression` — `flags[0]`, `dict["key"]`
//
// Before #372 only the first three (invocation / identifier /
// boolean) were recognised, so all five kinds above silently scored
// zero conditions in `if` / `while` / `do` / ternary contexts.
//
// `IsExpression` (391) and `IsPatternExpression` (392) are distinct
// kinds, not aliases: the grammar emits the first for a bare type test
// (`x is int`) and the second only once a pattern is involved
// (`x is int y`, `x is null`, `x is not Foo`). Listing only the second
// scored `if (x is int)` zero conditions against a cyclomatic decision
// of one, while `if (x is int y)` scored one — an asymmetry between two
// spellings of the same test, and the C# half of the gap #1421 closed
// for Kotlin by adding `IsExpression | InExpression` there. It also
// left #1422's guard slot spelling-dependent in the one case that fix
// claims to have fixed: `when x is int` scored 1 where
// `when IsEven(x)` scored 2. No arm counts the `is` token itself, so
// there is nothing to double-count (§5).
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
            | $crate::Csharp::IsExpression
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
// the dekobon Groovy grammar has no `await` analogue, so that one
// collapses out of the C# set.
//
// It DOES have an indexing analogue, and four navigation kinds beside
// it — `subscript_expression`, `safe_subscript_expression`,
// `safe_navigation_expression`, `safe_chain_dot_expression` and
// `direct_field_access_expression`, all alternatives of `_expression`
// and all legal in a boolean slot. All five join the set in #1466:
// until then `if (l[0])`, `if (a?.b)` and `if (a.@b)` scored zero
// where `if (a)` scored one, while C# scored `l[0]` through
// `ElementAccessExpression` and Kotlin scored both through
// `IndexExpression` / `NavigationExpression` — a per-language
// asymmetry rather than a policy difference. `a?.b` was the worst of
// them: Groovy cyclomatic counts `?.` as a decision, so ABC sat *two*
// below its own decision count on an idiomatic predicate. An earlier
// revision of this comment claimed the analogue did not exist, which
// is the sort of claim that stops the next reader looking.
//
// None of the five double-counts a token (§5). ABC's condition-token
// arm (`groovy_count_token_condition`) lists no navigation operator:
// `?.` (`QMARKDOT`), `??.` (`QMARKQMARKDOT`) and `?[`
// (`QMARKLBRACK` — its own token, verified with `bca dump`, not a
// bare `QMARK` that the ternary-gated arm could see) are cyclomatic
// decisions only. So listing the wrapper is the sole place each is
// scored.
//
// The remaining `_expression` alternatives are absent on purpose. The
// relational trio (`identity_expression`, `regex_find_expression`,
// `regex_match_expression`) comes through the token arm, see below;
// `binary_expression`, `ternary_expression`, `elvis_expression` and
// `switch_expression` are scored by their own operator token or
// nested condition; and the rest — `closure`,
// `object_creation_expression`, `range_expression`, `power_expression`,
// `update_expression`, `method_pointer_expression`,
// `method_reference_expression`, `spread_dot_expression` — are shapes
// whose Groovy-truth value is either constant or degenerate in a
// predicate slot. `spread_dot_expression` (`a*.b`) is the closest call
// of those; it is recorded in #1466 rather than added blind, as is
// `object_creation_expression`, which #1462 measured short and left
// alone because `new Foo()` is not a literal.
//
// Four of that list moved into the set in #1462: `string_literal`,
// `null_literal`, `list_literal` and `map_literal`. An earlier revision
// of this comment excluded them as "constant or degenerate … and none
// has a sibling-language precedent", and both halves of that stopped
// being true. The precedent now exists in every truthy-valued sibling —
// JavaScript's `string` / `null` / `object` / `array`, Python's
// `string` / `none` / `list` / `dictionary`, PHP's `string` /
// `array_creation_expression` / `null`, Lua's `string` /
// `table_constructor` — and constant-ness never was the test, since
// `BooleanLiteral` has been here since #403 and `NumberLiteral` since
// #1410. All four measured a condition short of a `b` control in both
// the `&&` chain and the `if` predicate. Landing only `string_literal`,
// which is the one #1466 flagged, would have left `if ([])` scoring
// zero beside `if ("s")` scoring one — the within-language asymmetry
// this issue exists to close, one kind narrower.
//
// One `string_literal` kind covers every spelling: `'s'`, `"s"`, the
// triple-quoted `"""s"""` and the slashy `/re/` all lex to it. The
// grammar's `SlashyString` variant is the hidden `_slashy_string`
// supertype the parser never emits (grammar-dispatch §2), which is why
// `Checker::is_string` names only `string_literal` and this set follows
// it (§7); `groovy_hidden_slashy_string_is_unreachable` in
// `metrics/abc.rs` pins that.
//
// Groovy truth makes every non-zero number truthy, so `NumberLiteral`
// is a unary condition here for the same reason Python's `Integer` /
// `Float` are. Without it `a && 1` scored one condition against
// `a && b`'s two (#1410).
//
// One kind covers every spelling: the dekobon grammar consolidated the
// per-radix rules the prior one split (`getter/groovy.rs`), so `0x1f`,
// `0b101`, `017`, `1_000`, `1e3` and every type suffix (`1L`, `1.5f`,
// `1G`, `1I`, `1.5d`, `1.2g`) all lex as `number_literal` — verified by
// `bca dump`, not read off the grammar, because #1379's misses were
// sibling rules under a hidden choice rather than aliases of one rule.
//
// #1379 deferred this on the stated grounds that "the integration
// corpora carry Groovy files"; they do not. No corpus carries a
// `.groovy` file at all, and DeepSpeech's 16 `.gradle` files are
// outside every test glob (`tests/corpus/deepspeech_test.rs` globs
// `*.cc` / `*.cpp` / `*.h` / `*.hh`), so this fix moves no snapshot.
//
// `membership_expression` (`a in l`, `a !in l`) is the Groovy spelling
// of Kotlin's `in_expression`, and joins the set for the same reason
// #1421 added that one: the grammar gives membership its own
// production rather than a `binary_expression`, so no comparison-token
// arm ever sees it and `if (a in l)` scored zero conditions against a
// cyclomatic decision of one.
//
// It is the one Groovy relational form that has to come through the
// terminal set rather than through `groovy_count_token_condition`'s
// token arm, and a `grammar.json` sweep of dekobon-tree-sitter-groovy
// 0.2.2 says why on both halves: the `in` token is shared with
// `for_in_statement`, so an ungated token arm would score every
// `for (x in list)` header, and the `!in` spelling emits **no operator
// token at all** — `bca dump` shows `membership_expression` with two
// `identifier` children and nothing between them — so a token arm
// could not reach the negated form however it were gated. Listing the
// wrapper covers both spellings at once and double-counts neither,
// since neither token is counted anywhere (§5).
//
// The sibling relational productions `identity_expression` (`===`,
// `!==`) and `regex_find_expression` / `regex_match_expression` (`=~`,
// `==~`) are deliberately **absent** here: each emits an operator
// token that the same sweep finds in that one production and nowhere
// else, so they are counted as plain comparison tokens beside `==` /
// `!=` in `groovy_count_token_condition` — the spelling every other
// language in the workspace uses for those operators (Kotlin, JS,
// PHP, Elixir for `===`; Perl, Ruby, Bash for `=~`), and the one that
// also scores them outside a boolean slot, where `def r = (a == b)`
// already scores and `def r = (a === b)` did not. Listing them in
// both places would score each twice (§5).
#[macro_export]
#[doc(hidden)]
macro_rules! groovy_bool_terminal_kinds {
    () => {
        $crate::Groovy::MethodInvocation
            | $crate::Groovy::CommandChain
            | $crate::Groovy::Identifier
            | $crate::Groovy::BooleanLiteral
            | $crate::Groovy::NumberLiteral
            | $crate::Groovy::StringLiteral
            | $crate::Groovy::NullLiteral
            | $crate::Groovy::ListLiteral
            | $crate::Groovy::MapLiteral
            | $crate::Groovy::FieldAccess
            | $crate::Groovy::CastExpression
            | $crate::Groovy::ParenthesizedTypeCast
            | $crate::Groovy::InstanceofExpression
            | $crate::Groovy::MembershipExpression
            | $crate::Groovy::SubscriptExpression
            | $crate::Groovy::SafeSubscriptExpression
            | $crate::Groovy::SafeNavigationExpression
            | $crate::Groovy::SafeChainDotExpression
            | $crate::Groovy::DirectFieldAccessExpression
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
    //
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

// The C family is integer-truthy — `if (1)`, `while (1)`,
// `do { … } while (0)` and `a && 1` are all legal and idiomatic — so
// `number_literal` and `char_literal` belong here for the same reason
// `"true"` does. Omitting them made the gap visible *within* one
// language: `if (true)` scored one condition and `if (1)` none (#1410).
//
// One `number_literal` arm covers every spelling in all four grammars.
// `0x1f`, `017`, `0b101`, `1u`, `1L`, `1ULL`, `1.5f`, `1e3` and C++'s
// `1'000` digit separator all lex to it, verified by `bca dump` per
// language rather than read off the grammar — #1379's misses were
// sibling rules under a hidden choice, which an alias sweep cannot see.
//
// `char_literal` is here because a character literal has integral type
// (`int` in C, `char` in C++) and is contextually convertible to bool
// exactly as a number is; leaving it out would reproduce the same
// within-language asymmetry one kind narrower. It is also the correct
// half of the wrapper/leaf pair (grammar-dispatch §5): an operand slot
// always holds the `char_literal`, never its inner `character` /
// `escape_sequence` child, so the wrapper is the only node reachable
// here and there is nothing to double-count.
//
// `string_literal` / `concatenated_string` / `nullptr` are **not**
// here, and that is a deferral rather than a decision: `if ("s")` is
// legal C and always true, so by this set's own integer-truthiness
// argument they belong. #1462 added the equivalent kinds to the eight
// truthy-valued sets and left the C family out because it is the one
// group in that class with integration-corpus exposure (the DeepSpeech
// `native_client` tree), so the snapshot delta wants its own change.
//
// Two neighbouring kinds are deliberately absent. C++'s
// `user_defined_literal` (`1.0_km`) wraps a `number_literal` but
// evaluates to whatever `operator""` returns, which need not be
// numeric or contextually boolean. Objective-C's `version_number` is
// the `@available(iOS 13.0, *)` token, not a literal, and never
// occupies an operand slot.
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
    // `available_expression` is Objective-C's runtime OS-version check
    // (`@available(iOS 13.0, *)`), in the same inert-elsewhere position
    // as `message_expression`: no C / C++ / Mozcpp grammar has a node by
    // that name. It is the one entry here that *is* a boolean rather
    // than something contextually converted to one, which is why it
    // belongs in a set otherwise justified by integer truthiness.
    // Without it `if (@available(iOS 13.0, *))` scored zero conditions
    // where `if (a)` scored one (#1457).
    //
    // The wrapper is the keeper, not any child (grammar-dispatch §6):
    // tree-sitter-objc's one `available_expression` rule spans both
    // spellings of the construct (`@available` and `__builtin_available`
    // are alternatives of its leading token) and makes the `version`
    // child optional, so `@available(iOS, *)` carries no numeric node at
    // all. Only the wrapper is present for every spelling, and it is
    // the node that occupies the operand slot. That is also why the
    // neighbouring exclusion above stands: `version_number` is a
    // fragment of this node's interior, never an operand itself.
    () => {
        "identifier"
            | "true"
            | "false"
            | "number_literal"
            | "char_literal"
            | "call_expression"
            | "message_expression"
            | "available_expression"
            | "field_expression"
            | "subscript_expression"
            | "cast_expression"
            | "qualified_identifier"
    };
}

// PHP treats every non-zero number as truthy, so `Integer` and `Float`
// are unary conditions here for the same reason Python's `Integer` /
// `Float` are. Naming neither scored `$a && 1` one condition against
// `$a && $b`'s two, and `if ($a && 1.0)` one against two (#1410).
//
// The unit to check is the supertype, not the alias list (#1379), and
// PHP has neither to miss: every radix prefix, `_` separator and
// exponent folds into one of those two kinds — `0x1f`, `0b101`, `017`,
// `0o17` and `1_000` all lex as `integer`, and `1e3`, `1.5e10`, `.5`
// and `1.` as `float` (verified by `bca dump`).
//
// `Float2` (52) is **not** a third numeric kind despite rendering to
// the same `"float"` string: it is the `float` *type* keyword of a
// parameter type or a `(float)` cast, which `Getter::get_op_type`
// groups with `Int` / `Bool` / `String2` rather than with the `Integer`
// / `Float` value operands (`getter/php.rs`). Listing it would be the
// `Number2` mistake `typescript_bool_terminal_kinds!` records below.
//
// #1462 added the non-numeric literals and the cast, each measured a
// condition short of a `$b` control in both the `&&` chain and the `if`
// predicate:
//
// - `String` (368) is the single-quoted literal and `EncapsedString`
//   (367) the interpolating double-quoted one — separate rules, not
//   aliases. `Heredoc` (371) and `Nowdoc` (373) are the two block
//   spellings. All four are what `Checker::is_string` already lists
//   (grammar-dispatch §7).
// - `String3` (378) is the hidden `_string` supertype the parser never
//   emits (grammar-dispatch §2) — listed defensively so a grammar that
//   starts emitting it counts, and pinned as hidden by
//   `php_hidden_string_supertype_is_unreachable` in `metrics/abc.rs`.
//   `is_string` carries the same defensive arm.
// - The `Float2` rule keeps two neighbours out. `String2` (25) is the
//   `string` *type* keyword of `function f(): string`, and `Null2` (55)
//   the `null` type keyword PHP 8 allows in the same position; neither
//   is a value. `is_string` does list `String2`, which is a separate
//   question about `find string` rather than a precedent for this set.
// - `ArrayCreationExpression` (355) covers both `[]` and `array()`.
// - `Null` (377) is a falsy constant and counts for the reason `False`
//   does — see `perl_bool_terminal_kinds!`.
// - `CastExpression` / `CastExpression2` (322, 323) close the second
//   finding of #1462: PHP was the only set in the Java / C# / Groovy /
//   PHP group naming no cast kind, so `if ((bool)$x)` scored zero where
//   the other three scored one through `CastExpression` /
//   `ParenthesizedTypeCast`. Two ids, both listed per lesson 2, though
//   only 322 is reachable at this pin — every cast spelling the
//   language has (`(bool)`, `(int)`, `(double)`, `(string)`,
//   `(binary)`, `(array)`, `(object)`, `(unset)`) parses to it, so 323
//   is a defensive arm in the `Perl::Octal` sense.
// - `ShellCommandExpression` (`` `ls` ``) evaluates to the command's
//   output, so it fills a boolean slot exactly as
//   `FunctionCallExpression` does, and measured short beside it.
//
// None of these double counts (§5): the PHP ABC impl's condition arm
// lists comparison and logical *tokens* only, and no arm matches a
// literal, a cast, or a backtick.
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
            | $crate::Php::Integer
            | $crate::Php::Float
            | $crate::Php::String
            | $crate::Php::String3
            | $crate::Php::EncapsedString
            | $crate::Php::Heredoc
            | $crate::Php::Nowdoc
            | $crate::Php::ArrayCreationExpression
            | $crate::Php::Null
            | $crate::Php::CastExpression
            | $crate::Php::CastExpression2
            | $crate::Php::ShellCommandExpression
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
    //
    // The remaining seven literal kinds joined in #1462 on the same
    // argument, each measured a condition short of an identifier
    // control in both the `and`/`or` chain and the `if` predicate:
    //
    // - `String` covers every quoting, prefix and interpolation
    //   spelling — `'s'`, `"""s"""`, `f'x{a}'` and `b'x'` all lex to
    //   it, verified by reading ids off a parsed fixture. Its neighbour
    //   `ConcatenatedString` (`'a' 'b'`, the implicit-join form) is a
    //   separate rule, not an alias, and is listed for the reason
    //   #1379's Perl numerals were: the supertype's arm list is the
    //   unit to check, and an alias sweep comes back clean on both.
    //   Pairing them also matches `Checker::is_string`, which has
    //   listed exactly `String | ConcatenatedString` since #301
    //   (grammar-dispatch §7).
    // - `None` is a falsy constant, and counts for the same reason
    //   `False` has since #403 — see `perl_bool_terminal_kinds!` on
    //   why the slot, not the value, is what the set measures.
    // - `List` / `Set` / `Tuple` / `Dictionary` are the four collection
    //   displays. `if items:` on a *name* already scored through
    //   `Identifier`; the literal spelling scored zero.
    // - `Ellipsis` completes the set. `if ...:` is rare, but leaving
    //   the one remaining literal kind out would reproduce the same
    //   within-language asymmetry one kind narrower, which is the
    //   defect this issue is about rather than a smaller version of it.
    //
    // A collection literal holding an expression (`a and [x > 1]`)
    // does not double count (§5): the walker never descends into the
    // operand, and the inner comparison reaches `conditions` through
    // the top-level `ComparisonOperator` arm — the same split that
    // already governs `a and f(x > 1)`, where `Call` and the
    // comparison each score once.
    () => {
        $crate::Python::Identifier
            | $crate::Python::True
            | $crate::Python::False
            | $crate::Python::None
            | $crate::Python::Integer
            | $crate::Python::Float
            | $crate::Python::String
            | $crate::Python::ConcatenatedString
            | $crate::Python::List
            | $crate::Python::Set
            | $crate::Python::Tuple
            | $crate::Python::Dictionary
            | $crate::Python::Ellipsis
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
// The sets for C#, Java, Kotlin, Rust and Go deliberately name no
// numeric kind: a bare number in a boolean slot is a compile error in
// those five, so there is nothing to count. **That reasoning extends to
// every other literal kind**, which is why #1462 left all five alone
// while adding strings, `null`, and collection literals to the eight
// truthy-valued sets: `if ("s")` and `if (null)` are compile errors in
// the same five for the same reason `if (1)` is.
//
// #1462 is also where the *value* of the literal stopped being the
// question. Every set here has listed `False` since #403 and several
// list `Nil` / `Null`, so the rule these sets encode is already "a
// literal **fills** the operand slot", not "a literal is truthy" — a
// falsy constant is a Fitzpatrick unary condition exactly as `false`
// is. The issue title says truthy because that is the idiom that
// exposed the gap (`x || "default"`), not because a `null` operand
// scores differently.
//
// The one exclusion that survives in a truthy-valued language is a
// kind that is not a **value**: a type keyword rendering to the same
// node-kind name as its literal. PHP's `Float2` records the original,
// and #1462 added four more — TypeScript's `String2` / `Object2`,
// Tsx's `String3` / `Object2`, and PHP's `String2` / `Null2`, each the
// annotation spelling (`a: string`, `function f(): null`) rather than
// a value. Each set names the ids so the next reader can check them.
//
// That rationale does **not** extend to the C family, which an earlier
// revision of this comment wrongly grouped with them: C and C++ are
// integer-truthy, so they carried the same gap PHP and Groovy did.
// #1410 closed all six — PHP, Groovy and the name-keyed C, C++, Mozcpp
// and Objective-C set — and each carries its own rationale above.
//
// #1379 deferred those six on the grounds that all three sets had
// integration-corpus files. That held only for the C family (the
// DeepSpeech `native_client` tree, seven snapshots): PHP's corpus files
// carry no truthy numeric operand, and no corpus carries a `.groovy`
// file at all.
//
// `pattern_matcher` (`/^#/`) and `pattern_matcher_m` (`m{^#}`) are the
// two spellings of a match against the implicit `$_`. They are
// **sibling rules, not aliases** — ids 337 and 336, the distinction
// this file's own numeric-literal note warns is invisible to an alias
// sweep — so both have to be listed or the `m{}` half stays at zero.
// A bound match (`$x =~ /^#/`) is a `binary_expression` whose `=~`
// token the dispatcher already counts; the bare form carries no
// operator token at all, which is why `if (/^#/)` scored zero
// conditions against `if ($x)`'s one.
//
// Listing them double-counts nothing (§5). In `$x =~ /^#/` the
// pattern is a child of the `binary_expression`, and every walker that
// consumes this set either breaks on that `binary_expression`
// (`perl_inspect_container`, `perl_count_condition`) or requires the
// list node itself to be the `&&`-chain parent
// (`perl_count_unary_conditions`), so the pattern node is never
// reached alongside its own `=~`.
//
// Three further rules in the same grammar family are deliberately
// absent because whether they are boolean *tests* is a judgement call,
// not a dispatch gap: `substitution_pattern_s` (`s///`) and
// `transliteration_tr_or_y` (`tr///`) each evaluate to a count rather
// than a bool, and `regex_pattern_qr` (`qr//`) to a compiled-pattern
// object that is always true. All three measure zero conditions today.
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
            | $crate::Perl::PatternMatcher
            | $crate::Perl::PatternMatcherM
    };
}

// Lua's `number` is one kind for the integer and the float spelling
// alike, so `a and 1` and `a and 1.0` both score through `Number` and
// the language has no counterpart of the #1379 Ruby / Elixir / Perl gap.
// The same holds for Tcl, iRules and the four JS-family sets, each
// measured rather than read off the grammar.
//
// `String` and `TableConstructor` joined in #1462. Lua's truth rule is
// the strongest case in the workspace for counting them: everything but
// `false` and `nil` is truthy, which is why `cond and "a" or "b"` *is*
// the language's ternary — and it scored 1 where `cond and a or b`
// scored 2. One `string` kind covers all three spellings (`"s"`, `'s'`
// and the long-bracket `[[s]]`), verified by reading ids off a parsed
// fixture; `Checker::is_string` likewise lists only `String`.
#[macro_export]
#[doc(hidden)]
macro_rules! lua_bool_terminal_kinds {
    () => {
        $crate::Lua::Identifier
            | $crate::Lua::True
            | $crate::Lua::False
            | $crate::Lua::Nil
            | $crate::Lua::Number
            | $crate::Lua::String
            | $crate::Lua::TableConstructor
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
    //
    // The seven non-numeric literal kinds join it in #1462 — every one
    // measured a condition short of the identifier control in both
    // slots. `x || "default"` is the language's commonest truthy-default
    // idiom and scored 1 against `x || y`'s 2.
    //
    // **Both `string` ids are listed, and that is per language.** The
    // grammar declares `string` under two kind_ids here (196, 221) and
    // the one an operand slot carries is `String2` (221) — but
    // `Checker::is_string` already lists both for JavaScript, Mozjs and
    // Tsx and only `String` for TypeScript, having made exactly this
    // per-language alias decision (grammar-dispatch §7). Mirroring it
    // keeps `find string` and ABC answering the same question about the
    // same node; diverging would be the drift §7 exists to prevent.
    //
    // `String` (196) is a defensive arm, like Perl's `Octal`: at this
    // grammar pin nothing emits it — an import specifier, an `export
    // from` clause, a quoted object key and a JSX attribute value all
    // parse to 221 — so removing it fails no test. It stays because the
    // alias exists in the enum, a pin bump renumbers ids freely (#732),
    // and the cost of a grammar that starts emitting it is a silent
    // zero rather than a build error.
    () => {
        $crate::Javascript::Identifier
            | $crate::Javascript::Identifier2
            | $crate::Javascript::True
            | $crate::Javascript::False
            | $crate::Javascript::Number
            | $crate::Javascript::String
            | $crate::Javascript::String2
            | $crate::Javascript::TemplateString
            | $crate::Javascript::Regex
            | $crate::Javascript::Null
            | $crate::Javascript::Undefined
            | $crate::Javascript::Object
            | $crate::Javascript::Array
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
    // numeric-truthy operand (#772, mirrors the Lua fix), and the seven
    // non-numeric literal kinds joined in #1462 — see
    // `javascript_bool_terminal_kinds!` for both. Mozjs renumbers every
    // id (`string` is 222 here, 221 there) but its alias *shape* is
    // JavaScript's: two `string` ids, and `Checker::is_string` lists
    // both for this language too.
    () => {
        $crate::Mozjs::Identifier
            | $crate::Mozjs::Identifier2
            | $crate::Mozjs::True
            | $crate::Mozjs::False
            | $crate::Mozjs::Number
            | $crate::Mozjs::String
            | $crate::Mozjs::String2
            | $crate::Mozjs::TemplateString
            | $crate::Mozjs::Regex
            | $crate::Mozjs::Null
            | $crate::Mozjs::Undefined
            | $crate::Mozjs::Object
            | $crate::Mozjs::Array
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
    //
    // The seven non-numeric literal kinds joined in #1462, and the
    // `Number2` rule decides two of them here. TypeScript spells
    // `string` under two ids and `object` under two: the *literals* are
    // `String` (247) and `Object` (213), while `String2` (135) and
    // `Object2` (137) are the `predefined_type` keywords of `a: string`
    // / `a: object`. Only the literals are listed — which is also the
    // split `Checker::is_string` already made, listing `String` alone
    // for TypeScript where it lists both ids for JavaScript, Mozjs and
    // Tsx (grammar-dispatch §7). Verified by parsing a fixture carrying
    // both spellings and reading the ids, not by reading the grammar:
    // all four render to the same node-kind string, so an alias sweep
    // cannot tell them apart.
    () => {
        $crate::Typescript::Identifier
            | $crate::Typescript::True
            | $crate::Typescript::False
            | $crate::Typescript::Number
            | $crate::Typescript::String
            | $crate::Typescript::TemplateString
            | $crate::Typescript::Regex
            | $crate::Typescript::Null
            | $crate::Typescript::Undefined
            | $crate::Typescript::Object
            | $crate::Typescript::Array
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
    //
    // The seven non-numeric literal kinds joined in #1462. Tsx is the
    // reason this file has four JS macros rather than one: it spells
    // `string` under **three** ids where TypeScript has two and
    // JavaScript has two different ones. `String` (233) and `String2`
    // (261) are both value literals — an operand slot carries 261, a
    // JSX attribute value 233 — and `String3` (141) is the
    // `predefined_type` keyword, the `Number2` case one kind over.
    // `Object` (219) is the literal, `Object2` (143) the type keyword.
    // Same split as `Checker::is_string`, which lists 233 and 261 and
    // not 141 (grammar-dispatch §7). Measured, 233 turns out to be the
    // same defensive-arm case as JavaScript's `String` (196): no
    // position reaches it at this pin, JSX attribute values included.
    () => {
        $crate::Tsx::Identifier
            | $crate::Tsx::Identifier2
            | $crate::Tsx::True
            | $crate::Tsx::False
            | $crate::Tsx::Number
            | $crate::Tsx::String
            | $crate::Tsx::String2
            | $crate::Tsx::TemplateString
            | $crate::Tsx::Regex
            | $crate::Tsx::Null
            | $crate::Tsx::Undefined
            | $crate::Tsx::Object
            | $crate::Tsx::Array
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
// so the comparison-token arms never see them and nothing else in the
// Kotlin impl counts them. They are terminal for every consumer of this
// set — neither carries a nested chain link, and neither's own operator
// token (`is` / `!is` / `in` / `!in`) is counted anywhere — so listing
// them here scores each exactly once, as Fitzpatrick Rule 5 scores any
// other relational operator. Before #1421 `if (a is String)` scored
// zero conditions against a cyclomatic decision of one.
#[macro_export]
#[doc(hidden)]
macro_rules! kotlin_bool_terminal_kinds {
    () => {
        $crate::Kotlin::Identifier
            | $crate::Kotlin::CallExpression
            | $crate::Kotlin::NavigationExpression
            | $crate::Kotlin::IndexExpression
            | $crate::Kotlin::ThisExpression
            | $crate::Kotlin::IsExpression
            | $crate::Kotlin::InExpression
            | $crate::Kotlin::InfixExpression
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
//
// `test_pattern` is Ruby 3.0's one-line pattern test (`a in Integer`),
// which evaluates to a boolean. The grammar gives it its own
// production, so the comparison-token arm in `metrics/abc/ruby.rs`
// never sees it — that arm is gated on a `binary` parent and lists no
// `in` token — and `if a in Integer` scored zero conditions against a
// cyclomatic decision of one. Nothing counts the `in` token itself, so
// listing the wrapper scores it exactly once (§5).
//
// Its neighbour `match_pattern` (`expr => pat`, id 252) is **not**
// here and must not be added: that spelling raises `NoMatchingPattern`
// on failure rather than yielding a boolean, so it is a destructuring
// assignment, not a condition.
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
            | $crate::Ruby::TestPattern
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
