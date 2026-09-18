//! Pins the **deliberate disagreement** between ABC and Halstead over a
//! `<` / `>` that is not a comparison.
//!
//! #1274, #1275, #1280 and #1297 taught ABC that a bracket outside a
//! `binary_expression` is not a decision, so `conditions` is 0 for a
//! JSX tag, a Lua `<const>` attribute, a C# `operator <` declaration, a
//! Kotlin `super<A>` and a Perl `<FH>` readline. The Halstead getters
//! kept billing every one of those brackets as an operator, and nothing
//! recorded whether that was drift or a decision.
//!
//! #1395 decided it is a decision. Halstead counts the *vocabulary* a
//! program is written in and has no notion of a branch, so a delimiter
//! is an operator whatever it delimits; ABC asks whether a branch is
//! taken, and these are not branches. The rule and its one exception —
//! a literal's own delimiter, where the enclosing literal node is
//! itself the operand — are written up on `Getter::get_op_type` in
//! `big-code-analysis-ast/src/getter.rs` and in the book's Halstead
//! section.
//!
//! **This test's job is to fail if a later "make these consistent" pass
//! flips either side.** Both directions are live hazards and neither is
//! visible from one metric alone: adding a `parent_has_kind` gate to a
//! getter's `LT` / `GT` arm would silently drop `<` from `bca ops`, and
//! dropping a gate from an ABC arm would silently reintroduce the
//! phantom conditions #1297 removed. Each row therefore asserts *both*
//! halves over the same fixture. The tail of the assertion — the whole
//! operator vocabulary, and `N1` — is what stops a row decaying into a
//! claim about some surviving token after someone edits the fixture.

use big_code_analysis::{Ast, LANG, MetricsOptions, Source, analyze};

/// One row per construct #1395 settled, plus the two sibling grammars
/// that share the JS-family getter macro.
///
/// Returns `(source, extension, operators, n1_occurrences)`. Every
/// fixture is deliberately free of a genuine comparison, so
/// `conditions_sum() == 0` is a claim about the bracket rather than an
/// aggregate that happens to cancel.
///
/// `operators` is the complete deduplicated operator vocabulary, which
/// [`Ast::ops`] returns byte-lexicographically sorted (#1091), and
/// `n1_occurrences` is `N1` over the whole file.
///
/// The wildcard arm is deliberate, unlike the exhaustive `match` in
/// [`super::ops_metrics_space_parity::fixture`]. Eighteen languages
/// carry the ABC bracket gate — the generic and template brackets of
/// C++, Rust, Go, Java, Groovy and the rest are the same shape — so an
/// exhaustive table would need a row for each rather than a `None` arm
/// asserting they have no such construct, which would be false. What
/// this pins is the policy, and a policy flip lands in a shared getter
/// macro or a shared ABC arm, which these seven rows already reach.
#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
fn fixture(lang: LANG) -> Option<(&'static str, &'static str, &'static [&'static str], u64)> {
    // The JSX fixture, named once rather than spelled in each of the
    // three rows that use it. Their claim is that the vendored Mozilla
    // fork, upstream JavaScript and the TSX dialect measure the *same*
    // source identically, so a delta is a fork or dialect divergence;
    // three literal copies would leave that a convention an edit to one
    // row could break in silence. Only the extension differs per row,
    // and it is cosmetic — `Source::new` is given the `LANG`, so the
    // name reaches nothing but the file-level space. Local to this
    // function so it inherits the gate above rather than restating it.
    //
    // `<br />` is the element #1395 is about: before the fix it billed
    // one bracket operator where the source spells two.
    const JSX_SOURCE: &str =
        "const e = <div className=\"a\"><span>hi</span></div>;\nconst b = <br />;\n";
    // The complete deduplicated operator vocabulary, byte-
    // lexicographically sorted as `Ast::ops` returns it (#1091).
    const JSX_OPERATORS: &[&str] = &["/>", ";", "<", "</", "=", ">", "const"];
    // N1: line 1 is `const`, `=` x2, `<` x2, `>` x4, `</` x2 and `;`;
    // line 2 is `const`, `=`, `<`, `/>` and `;`.
    const JSX_N1: u64 = 17;

    let row = match lang {
        // The JSX rows carry all four delimiter tokens: `<` and `>` for
        // the open tags, `</` for the closers and `/>` for the
        // self-closing element. The last two were in neither arm of
        // `impl_js_family_get_op_type!` until #1395, and all three
        // grammars reach that macro, so all three share the one
        // `JSX_SOURCE` above. TypeScript is absent because it is the
        // one grammar of the four with no JSX.
        LANG::Tsx => (JSX_SOURCE, "tsx", JSX_OPERATORS, JSX_N1),
        LANG::Javascript => (JSX_SOURCE, "js", JSX_OPERATORS, JSX_N1),
        LANG::Mozjs => (JSX_SOURCE, "jsm", JSX_OPERATORS, JSX_N1),
        // Lua 5.4 brackets a variable attribute with the two bare
        // comparison tokens. `const` is the attribute *name*, an
        // operand, which is why it is absent from the operator list.
        //
        // expected N1 = 4: `local`, `<`, `>`, `=`.
        LANG::Lua => (
            "local x <const> = 1\n",
            "lua",
            ["<", "=", ">", "local"].as_slice(),
            4,
        ),
        // A comparison-operator overload names the operator it defines
        // with the same bare token applying one would use.
        //
        // expected N1 = 22: `class` and the class body `{}`, plus ten
        // per overload — `public`, `static`, `bool`, `operator`, the
        // bracket, `()`, `,`, `{}`, `return`, `;`.
        LANG::Csharp => (
            "class V {\n    public static bool operator <(V a, V b) { return true; }\n    \
             public static bool operator >(V a, V b) { return true; }\n}\n",
            "cs",
            [
                "()", ",", ";", "<", ">", "bool", "class", "operator", "public", "return",
                "static", "{}",
            ]
            .as_slice(),
            22,
        ),
        // A qualified super call disambiguates its supertype with the
        // same two bare tokens. `override` is in no arm of the Kotlin
        // getter, which is why it is absent below.
        //
        // expected N1 = 13: the header's `class`, `:`, `()` and `{}`,
        // then `fun`, the parameter `()`, the return-type `:`, the body
        // `{}`, `return`, `<`, `>`, `.` and the call's `()`.
        LANG::Kotlin => (
            "class B : A() {\n    override fun g(): Int { return super<A>.g() }\n}\n",
            "kt",
            ["()", ".", ":", "<", ">", "class", "fun", "return", "{}"].as_slice(),
            13,
        ),
        // A filehandle readline. `<STDIN>` is deliberately not the
        // fixture: the grammar lexes it as one `standard_input` token,
        // so it has no brackets to classify and the row would assert
        // nothing. The closing `>` is `Perl::GT`, not the `GT2` alias
        // the enum also carries — see
        // `perl_readline_closer_alias_never_reaches_kind_id`.
        //
        // expected N1 = 11: `sub`, `{}`, `my`, `$` x2 (one per
        // `scalar_variable`), `=`, `<`, `>`, `;` x2, `return`.
        LANG::Perl => (
            "sub r { my $l = <FH>; return $l; }\n",
            "pl",
            ["$", ";", "<", "=", ">", "my", "return", "sub", "{}"].as_slice(),
            11,
        ),
        _ => return None,
    };
    Some(row)
}

#[cfg(any(
    feature = "bash",
    feature = "c",
    feature = "c-family-helpers",
    feature = "cpp",
    feature = "csharp",
    feature = "elixir",
    feature = "go",
    feature = "groovy",
    feature = "irules",
    feature = "java",
    feature = "javascript",
    feature = "kotlin",
    feature = "lua",
    feature = "mozcpp",
    feature = "mozjs",
    feature = "objc",
    feature = "perl",
    feature = "php",
    feature = "python",
    feature = "ruby",
    feature = "rust",
    feature = "tcl",
    feature = "typescript",
))]
#[test]
fn a_non_comparison_bracket_is_a_halstead_operator_and_not_an_abc_condition() {
    let mut checked = 0;

    for lang in LANG::into_enum_iter() {
        if !lang.is_enabled() {
            continue;
        }
        let Some((source, ext, operators, n1_occurrences)) = fixture(lang) else {
            continue;
        };
        checked += 1;

        let name = format!("parity.{ext}");
        let space = analyze(
            Source::new(lang, source.as_bytes()).with_name(Some(name.clone())),
            MetricsOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));
        let ops = Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name)))
            .unwrap_or_else(|e| panic!("{lang:?}: parse failed: {e}"))
            .ops()
            .unwrap_or_else(|e| panic!("{lang:?}: ops failed: {e}"));

        // The ABC half. `conditions_sum` folds every space in the tree,
        // so a row whose construct sits in a nested function is covered
        // without the test walking the tree itself.
        assert_eq!(
            space.metrics.abc.conditions_sum(),
            0,
            "{lang:?}: a non-comparison `<` / `>` must score no ABC condition",
        );

        // The Halstead half, stated twice. The membership assertions
        // name the two tokens the issue is about, so a failure reads as
        // the policy question rather than as a vocabulary diff; the
        // equality below is what keeps the row from decaying if the
        // fixture is edited, and is what a new delimiter arm has to be
        // declared in.
        for bracket in ["<", ">"] {
            assert!(
                ops.operators.iter().any(|o| o == bracket),
                "{lang:?}: `{bracket}` must stay a Halstead operator; operators were {:?}",
                ops.operators,
            );
        }
        assert_eq!(
            ops.operators, operators,
            "{lang:?}: operator vocabulary changed",
        );
        // The occurrence axis. The vocabulary above is deduplicated, so
        // it cannot see a gate that drops one of several occurrences of
        // the same bracket — the JSX rows spell `>` four times.
        assert_eq!(
            space.metrics.halstead.total_operators(),
            n1_occurrences,
            "{lang:?}: N1 changed",
        );
    }

    // Every language is feature-gated, so a build enabling none of the
    // seven leaves a zero-iteration loop and a test that reports green
    // while asserting nothing. What keeps this from firing spuriously
    // is the `#[cfg(any(…))]` on this file's `mod` declaration in
    // `tests/parity/main.rs`, which names exactly the features whose
    // rows above are `Some`.
    assert!(
        checked > 0,
        "at least one language with a non-comparison bracket construct must be \
         enabled for this test to mean anything",
    );
}
