//! Function-space names for the C-family declarator shapes (#1208).
//!
//! The declarator unwrapping these exercise lives in
//! `big-code-analysis-ast`, but what they assert is the name the metric
//! walk puts on a `FuncSpace` — a whole-`analyze` outcome — so the tests
//! belong on this side. Keeping them here is also what lets the parse
//! layer carry no dependency on this crate at all (#1376).

use crate::test_support::space_verbatim;
use crate::{FuncSpace, LANG, MetricsOptions, SpaceKind};

/// Declarator shapes all four C-family grammars parse alike, with
/// the name each one's function space must carry.
///
/// The first three are #1208 itself: every one resolved to `None`
/// before the getters moved onto [`super::innermost_declarator`].
/// The macro spelling is the shape that dominates the corpora — 354
/// nameless C-family function spaces across `DeepSpeech` and
/// `pdf.js`, clustered in TensorFlow's JNI shims — and it carries no
/// `parenthesized_declarator` at all, so a fix keyed on that kind
/// would pass the first row and miss the population.
/// `RUN_STATS_METHOD` is the macro's name, not the function's, which
/// after `##` pasting is not in the source at all; it is kept
/// because it is the token a reader greps for (#1213).
///
/// The two macro rows after it are #1213: the arity moved to the
/// outer declarator there, so the name is the only thing still read
/// from inside the invocation, and `A` additionally pins the descent
/// through a *run* of nested invocations.
///
/// The next three are controls. `g` in particular resolved
/// correctly *before* this change — its outer declarator is an
/// `array_declarator`, so the old leftmost pre-order search happened
/// to reach the right `function_declarator` — and would regress
/// silently if the walk stopped one link too early.
///
/// The last row expects no name at all; its comment says why.
const SHARED_SHAPES: &[(&str, Option<&str>)] = &[
    ("int (*fp(int a, int b))(int c) { return 0; }", Some("fp")),
    (
        "int (__cdecl *w(int a, int b))(int c) { return 0; }",
        Some("w"),
    ),
    (
        "void RUN_STATS_METHOD(allocate)(int a) { }",
        Some("RUN_STATS_METHOD"),
    ),
    ("void MACRO(a, b)(int x) { }", Some("MACRO")),
    ("void A(b, c)(d)(int x, int y, int z) { }", Some("A")),
    ("int (*g(void))[4] { return 0; }", Some("g")),
    ("int plain(int a, int b) { return a; }", Some("plain")),
    ("FILE *ptr(int a) { return 0; }", Some("ptr")),
    // Why the fallback may not simply require each link to be a
    // `*_declarator`, which is the tidier-looking rule. Every
    // grammar recovers this TensorFlow C-API signature into a
    // `qualified_identifier` holding a **zero-width** `::` and the
    // real `pointer_type_declarator`, so the chain has to descend
    // through a link that is not a declarator at all to reach the
    // name. Gating the fallback on the kind suffix loses this and
    // two more names in the corpora: a non-declarator link is not
    // always a name.
    (
        "TF_CAPI_EXPORT extern TF_ConcreteFunction* TF_GetFn(TF_SavedModel* m) { return 0; }",
        Some("TF_GetFn"),
    ),
    // The one row the gate must *reject*. Redundant parentheses
    // around the name are legal C and put a
    // `parenthesized_declarator` where every grammar's name kinds
    // would be, so each getter's `matches!` falls through and the
    // space stays nameless — emitting no name rather than whatever
    // text happens to sit there is what that gate is for, and no
    // other row reaches its `false` branch.
    //
    // A boundary, not a bug-lock: the shape has zero occurrences
    // across `DeepSpeech` and `pdf.js`, so there is nothing to fix
    // and no issue to open. Teach the walk to unwrap the
    // parentheses and this is the row to update.
    ("int (fp)(int a) { return a; }", None),
];

/// C++ name forms C and Objective-C have no syntax for. None of
/// these is a #1208 shape; they are here because the rewrite
/// replaced the `child(0)` the identifier-kind `match` used to read
/// with the `declarator` field, and each of these rows is a
/// different kind arriving in that slot — `destructor_name`,
/// `qualified_identifier`, `operator_name`, `template_function`.
/// The conversion operator additionally pins the `OperatorCast`
/// early return, which the shared walk cannot answer for: a
/// conversion operator's declarator field is the type it converts
/// *to*, so [`super::innermost_declarator`] deliberately cuts the
/// chain there and returns `None`.
const CPP_ONLY_SHAPES: &[(&str, Option<&str>)] = &[
    ("struct S { ~S() { } };", Some("~S")),
    ("void Foo::bar(int a) { }", Some("Foo::bar")),
    (
        "struct S { operator int() const { return 0; } };",
        Some("operator int() const"),
    ),
    (
        "struct S { int operator+(int o) const { return o; } };",
        Some("operator+"),
    ),
    (
        "Foo &Bar::get(int a) { static Foo f; return f; }",
        Some("Bar::get"),
    ),
    (
        "template <typename T> T tfree(T a) { return a; }",
        Some("tfree"),
    ),
    // The two shapes the fallback's `template_argument_list`
    // exclusion exists for. Both spell an explicit template argument
    // of function type, so the chain would otherwise leave the name
    // side entirely — down `template_function` into its
    // `template_argument_list` — and settle on the argument's own
    // `abstract_function_declarator`. That node spells no
    // identifier, so the name came back `None`, and `nargs` read the
    // argument's two parameters instead of the function's one.
    //
    // Both parse without an `ERROR` node, so neither is covered by
    // the recovery caveat on [`super::innermost_declarator`]. The
    // second is the more reachable of the two: an out-of-line
    // member with explicit template arguments needs no `template <>`
    // preamble.
    (
        "template <> void tspec<int (*)(int x, int y)>(int a) { }",
        Some("tspec<int (*)(int x, int y)>"),
    ),
    (
        "void Foo::tmem<int (*)(int x, int y)>(int a) { }",
        Some("Foo::tmem<int (*)(int x, int y)>"),
    ),
];

/// Each fixture is padded with a leading and a trailing comment
/// line, so the asserted span is `(2, 2)` — a value a
/// default-constructed or off-by-one span does not also satisfy,
/// unlike the `(1, 1)` a bare one-line fixture would produce.
const FIXTURE_LINE: usize = 2;

fn pad(source: &str) -> String {
    format!("// leading\n{source}\n// trailing\n")
}

/// Every `Function` space in the tree, in source order.
///
/// The C++ rows nest their function inside a `struct` space, so the
/// assertion cannot read `root.spaces[0]`; collecting the whole
/// subtree also lets each row assert that the fixture opened
/// *exactly one* function space, which is `get_space_kind` and
/// `is_func_space` agreeing with the name — `.claude/rules/
/// grammar-dispatch.md` §6.
fn function_spaces(space: &FuncSpace, found: &mut Vec<(Option<String>, usize, usize)>) {
    if space.kind == SpaceKind::Function {
        found.push((space.name.clone(), space.start_line, space.end_line));
    }
    for child in &space.spaces {
        function_spaces(child, found);
    }
}

fn check(lang: LANG, shapes: &[(&str, Option<&str>)], failures: &mut Vec<String>) {
    for (source, expected) in shapes {
        let root = space_verbatim(lang, pad(source).as_bytes(), MetricsOptions::default());
        let mut found = Vec::new();
        function_spaces(&root, &mut found);
        let want = vec![(expected.map(str::to_owned), FIXTURE_LINE, FIXTURE_LINE)];
        if found != want {
            failures.push(format!(
                "{lang:?}: {source:?}\n  want {want:?}\n  got  {found:?}"
            ));
        }
    }
}

/// Fail with every mismatched row, and fail *differently* when a
/// feature set left the loop empty.
///
/// Shared so the failure formatting exists once: it is by
/// construction unreachable while the suite is green, so a second
/// copy is coverage the tests can never earn.
#[track_caller]
fn assert_all_matched(failures: &[String], checked: usize, what: &str) {
    assert!(
        failures.is_empty(),
        "{}/{checked} {what}:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // Non-vacuity: a feature set that disabled all four languages
    // would otherwise leave every assertion above unrun.
    assert!(checked > 0, "no C-family language was enabled");
}

/// A C-family function's name comes off the declarator walk its
/// arity comes off (#1208) — from the innermost declarator itself
/// for most shapes, and from inside the macro invocation that
/// declarator wraps for the three macro rows (#1213). "Same walk"
/// rather than "same node" is why this is not named for the
/// innermost declarator alone.
#[test]
fn the_declarator_walk_names_the_function_space() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for lang in [LANG::C, LANG::Cpp, LANG::Mozcpp, LANG::Objc]
        .into_iter()
        .filter(LANG::is_enabled)
    {
        check(lang, SHARED_SHAPES, &mut failures);
        checked += SHARED_SHAPES.len();
        if matches!(lang, LANG::Cpp | LANG::Mozcpp) {
            check(lang, CPP_ONLY_SHAPES, &mut failures);
            checked += CPP_ONLY_SHAPES.len();
        }
    }
    assert_all_matched(
        &failures,
        checked,
        "declarator shapes named the wrong space",
    );
}

/// [`check`] must be *able* to fail.
///
/// It collects rather than asserts, so nothing in the table above
/// would notice if `function_spaces` selected no space at all — the
/// comparison would just find two empty expectations equal, and
/// every row would pass vacuously
/// (`.claude/rules/testing.md`, "Review the selector as carefully as
/// the assertion"). Feeding it a name that is deliberately wrong is
/// the cheapest proof that the selector reaches a real space and the
/// comparison discriminates.
#[cfg(feature = "c")]
#[test]
fn the_table_reports_a_name_that_does_not_match() {
    let mut failures = Vec::new();
    check(
        LANG::C,
        &[(
            "int plain(int a, int b) { return a; }",
            Some("deliberately_wrong"),
        )],
        &mut failures,
    );
    let [only] = failures.as_slice() else {
        panic!("one wrong expectation must produce one failure, got {failures:?}");
    };
    // `Some("plain")` rather than a bare `plain`: the message echoes
    // the fixture source, which contains the word too, so the bare
    // substring passes even when the selector found *nothing* —
    // measured, by filtering `function_spaces` on `SpaceKind::Class`.
    // Matching the rendered `Option` is what ties the assertion to
    // the space rather than to the input.
    assert!(
        only.contains("deliberately_wrong") && only.contains("Some(\"plain\")"),
        "the failure must name both the expectation and the space found: {only}"
    );
}

/// An unexpanded macro where a trailing attribute belongs, which is
/// the one input in this module that reaches
/// [`super::declarator_name`]'s `?` — the arm taken when *no* link
/// on the chain carries a `parameters` field, so there is no
/// declarator to read a name from at all. Every other row resolves
/// an owner and is answered by the identifier-kind gate instead.
///
/// The grammars split two-two on it, which is the reason this is its
/// own test rather than a table row — and the split is not the one
/// the language families would suggest:
///
/// | grammar | parse | name |
/// | --- | --- | --- |
/// | C, **mozcpp** | clean — `function_declarator` admits the trailing identifier | `f` |
/// | C++, Objective-C | `ERROR` around the declarator | none |
///
/// Where the parse is clean the chain follows a real `declarator`
/// field and the name resolves. Where it is not, the declarator sits
/// inside an `ERROR` and the macro is left as the
/// `pointer_declarator`'s last named child, so the fallback follows
/// the macro into a dead end.
///
/// mozcpp siding with C rather than with the upstream `tree-sitter-cpp`
/// it forked from is the finding worth keeping here: it owns no file
/// extension, so nothing routes to it and only a unit test can see
/// it at all (`.claude/rules/grammar-dispatch.md`, "when you fix one
/// language, sweep the rest").
///
/// Recovery trees are outside the walk's contract — see
/// [`super::innermost_declarator`], which measured this exact shape
/// as one of the two corpus spaces #1208 un-named. This pins what
/// the walk does there, not a claim that it is the right answer.
#[test]
fn a_macro_where_an_attribute_belongs_divides_the_grammars() {
    const SOURCE: &str = "int *f() TF_ATTRIBUTE_NOINLINE { return 0; }";

    let mut failures = Vec::new();
    let mut checked = 0;
    for lang in [LANG::C, LANG::Cpp, LANG::Mozcpp, LANG::Objc]
        .into_iter()
        .filter(LANG::is_enabled)
    {
        let expected = matches!(lang, LANG::C | LANG::Mozcpp).then_some("f");
        check(lang, &[(SOURCE, expected)], &mut failures);
        checked += 1;
    }
    assert_all_matched(
        &failures,
        checked,
        "grammars disagreed about the recovery shape",
    );
}

/// The same macro carrying an argument, which is the spelling the
/// annotation idiom actually takes — `TF_LOCKS_EXCLUDED(mu_)`,
/// `TF_GUARDED_BY(mu_)`, `ABSL_EXCLUSIVE_LOCKS_REQUIRED(mu_)`.
///
/// Where the grammar recovers, it is worse than the parameterless
/// spelling above rather than the same. A bare trailing identifier
/// carries no `parameters`, so the chain dead-ends and the space
/// merely goes nameless; a parenthesised one is a
/// `function_declarator` that *does*, so it becomes the walk's
/// answer and the space is named after the macro. `nargs` reads the
/// macro's argument off the same node, and two members of one class
/// sharing an annotation collapse onto a single `bca check` offender
/// key (`K::TF_LOCKS_EXCLUDED` twice).
///
/// This is the one shape #1208 made worse: the leftmost pre-order
/// search it replaced descended into the `ERROR` and got `f` right.
/// One corpus space is affected — `resource()` in TensorFlow's
/// `resource_op_kernel_test.cc`, which #1208 renamed to
/// `TF_LOCKS_EXCLUDED`. Pinned rather than fixed, like its sibling
/// above: reaching into a recovery subtree is a strategy decision of
/// its own, and every rule the walk follows is void inside an
/// `ERROR`. Teach the walk to unwrap one and this is a row to
/// update, not a row to delete.
#[test]
fn a_parenthesised_macro_takes_the_name_of_the_function_it_annotates() {
    const SOURCE: &str = "int *f() TF_LOCKS_EXCLUDED(mu_) { return 0; }";

    let mut failures = Vec::new();
    let mut checked = 0;
    for lang in [LANG::C, LANG::Cpp, LANG::Mozcpp, LANG::Objc]
        .into_iter()
        .filter(LANG::is_enabled)
    {
        // Only C parses this cleanly, and there the chain follows a
        // real `declarator` field past the macro to `f`. The split
        // is *not* the two-two of the parameterless spelling:
        // mozcpp sides with C there and with C++ here, so a fixture
        // in either spelling alone would misreport what the other
        // does.
        let expected = if lang == LANG::C {
            "f"
        } else {
            "TF_LOCKS_EXCLUDED"
        };
        check(lang, &[(SOURCE, Some(expected))], &mut failures);
        checked += 1;
    }
    assert_all_matched(
        &failures,
        checked,
        "grammars disagreed about the annotated recovery shape",
    );
}
