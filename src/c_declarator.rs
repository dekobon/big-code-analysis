//! The C-family declarator-chain walk, shared by the two surfaces that
//! need it.
//!
//! C declarator syntax nests outward from the declared name, so neither
//! a function's parameter list nor its name is reliably a child of the
//! function node itself. Both live on the same node — the innermost
//! `function_declarator` along the chain — which is what
//! [`innermost_declarator`] finds:
//!
//! - [`crate::metrics::nargs`] reads its `parameters` field (#1200).
//! - The `Getter::get_func_space_name` impls for C, C++, mozcpp and
//!   Objective-C read its `declarator` field (#1208).
//!
//! Keeping one walk keeps the two answers about the same function from
//! disagreeing, which is how #1208 arose: the arity came from this
//! chain and the name came from a leftmost pre-order search that stopped
//! one level too early.

use crate::checker::Checker;
use crate::node::Node;

/// The innermost declarator along a C-family function's declarator
/// chain: the node whose `parameters` field holds the function's *own*
/// formal arguments, and whose `declarator` field holds its name.
///
/// C declarator syntax nests outward from the declared name, so a
/// function node's `declarator` field is only the function's parameter
/// list when the return type is plain. Anything the return type
/// contributes — a `*`, a `&`, a parenthesised group — wraps the
/// `function_declarator` that owns the real list, and the outermost
/// `parameters` a chain carries can belong to the *return type* rather
/// than to the function (`int (*f(int a))(int b)` returns a pointer to a
/// one-argument function and itself takes one argument). Taking the
/// innermost is what makes both of those come out right (#1200), and
/// the same node carries the name `f` that a leftmost search misses
/// (#1208).
///
/// The walk is by field name, per `.claude/rules/grammar-dispatch.md`
/// §3, which also sidesteps §1: `PointerDeclarator2`,
/// `FunctionDeclarator2`/`3` and `ReferenceDeclarator2`/`3`/`4` are
/// numeric-suffix aliases that a `kind_id` match would have to
/// enumerate and would silently regress on the next grammar bump.
///
/// Three of the rules on the chain expose no field at all, which is why
/// the field alone is not enough. Every entry below is from the pinned
/// grammars' `node-types.json` (`tree-sitter-cpp` 0.23.4,
/// `tree-sitter-c` 0.24.2, `tree-sitter-objc` 3.0.2, vendored
/// `tree-sitter-mozcpp`), and the fieldless list is the complete set of
/// `*_declarator` rules with no fields that a *function definition's*
/// name side can reach — the rest (`variadic_declarator`,
/// `structured_binding_declarator`, Objective-C's `keyword_declarator`
/// and `struct_declarator`, and the `abstract_*` family) sit in
/// parameter, binding or type position, never here.
///
/// | rule | `declarator` field |
/// | --- | --- |
/// | `pointer_declarator` | required |
/// | `function_declarator` | required |
/// | `abstract_function_declarator` | **optional** |
/// | `reference_declarator` | **absent — no fields** |
/// | `parenthesized_declarator` | **absent — no fields** |
/// | `attributed_declarator` | **absent — no fields** |
///
/// In all three fieldless rules the inner declarator is the last
/// *named* child once attributes are set aside, so that is the
/// fallback:
///
/// - `reference_declarator` is `seq(choice('&', '&&'), _declarator)`.
/// - `parenthesized_declarator` is `seq('(',
///   optional(ms_call_modifier), _declarator, ')')` — last rather than
///   sole, so `int (__cdecl *f(int a))(int b)` does not defeat it.
/// - `attributed_declarator` is `seq(_declarator,
///   repeat1(attribute_declaration))`, the one rule that puts the
///   declarator **first**. Excluding `attribute_declaration` — its only
///   non-declarator child type in all four grammars — restores "last"
///   as the right answer, and without that exclusion
///   `int f(int a, int b) [[deprecated]]` reports 0.
///
/// `template_argument_list` is excluded for the same reason as
/// `attribute_declaration`, and it is the one exclusion the fallback
/// needs beyond the three rules above. The fallback also runs on the
/// *name* forms, which have no `declarator` field either, and two of
/// them — `template_function` and `template_method` — put their argument
/// list last: `void f<int (*)(int x, int y)>(int a)`. A type argument
/// spelling a function type carries a `parameters` field of its own, so
/// descending into it made that function read as taking two arguments
/// and made its name resolve to nothing at all, the abstract declarator
/// the chain landed on spelling no identifier. Excluding the argument
/// list leaves the name itself as the last named child, which terminates
/// the chain where it should.
///
/// Comments are excluded for the same reason, tree-sitter admitting one
/// anywhere.
///
/// The fallback stops at a node that already carries `parameters`,
/// which is the C++ lambda: `abstract_function_declarator`'s
/// `declarator` field is optional, so `[](int a, int (*cb)(int x))`
/// would otherwise descend into the `parameter_list` and return `cb`'s
/// `(int x)` — one argument instead of two.
///
/// Every step strictly descends a finite tree, so the walk terminates
/// without a depth cap.
///
/// # ERROR-recovery trees are outside this contract
///
/// Every rule above is the grammar's, and none of them holds once
/// tree-sitter starts recovering. An unexpanded macro in declarator
/// position — `T *f() TF_ATTRIBUTE_NOINLINE { … }` — puts the real
/// `function_declarator` inside an `ERROR` node and leaves the macro's
/// `field_identifier` as the `pointer_declarator`'s last named child,
/// so the fallback follows the macro and the walk answers `None`.
/// Whatever any strategy returns there is arbitrary, and the walk does
/// not try to be clever about it: measured over `DeepSpeech` and
/// `pdf.js` (14,269 files), moving the four getters onto this walk
/// named 44 previously-nameless function spaces and un-named 2, both of
/// the latter inside recovery subtrees — one of which had been
/// reporting an `if` statement's callee as a function name (#1208).
pub(crate) fn innermost_declarator<'tree, T: Checker>(node: &Node<'tree>) -> Option<Node<'tree>> {
    // The chain starts at the `declarator` field rather than at `node`
    // so the last-named-child fallback can never fire on the function
    // node itself and walk into its body. The walk runs outside-in, so
    // the innermost qualifying link is the last one it yields.
    std::iter::successors(
        node.child_by_field_name("declarator"),
        |current| match current.child_by_field_name("declarator") {
            Some(declarator) => Some(declarator),
            None if current.child_by_field_name("parameters").is_some() => None,
            None => current
                .children()
                .filter(|child| {
                    child.is_named()
                        && !T::is_comment(child)
                        && !matches!(child.kind(), ATTRIBUTE | TEMPLATE_ARGUMENTS)
                })
                .last(),
        },
    )
    // A conversion operator's `declarator` field is the type it converts
    // *to*, not its name side: `operator int (*)(int x)` takes no
    // arguments, and everything from here inward describes that
    // function-pointer type. Cutting the chain restores the 0 the
    // pre-#1200 code reported by never finding `parameters` at all.
    .take_while(|link| link.kind() != CONVERSION_OPERATOR)
    .filter(|link| link.child_by_field_name("parameters").is_some())
    .last()
}

/// The node spelling a C-family function's name.
///
/// It is the `declarator` field of [`innermost_declarator`] — the same
/// node whose `parameters` field gives the arity — and it is a separate
/// function only so the four `get_func_space_name` impls state that
/// pairing once instead of four times. Each caller still gates the
/// result on its own grammar's identifier kinds: what counts as a name
/// is where C, C++ and Objective-C differ (`destructor_name`,
/// `qualified_identifier`, `operator_name`, `template_function`), and a
/// kind this module accepted on their behalf would be a claim about
/// four grammars made in a module that reads none of them.
pub(crate) fn declarator_name<'tree, T: Checker>(node: &Node<'tree>) -> Option<Node<'tree>> {
    innermost_declarator::<T>(node)?.child_by_field_name("declarator")
}

/// Compared by `kind()` string rather than `kind_id`, per
/// `.claude/rules/grammar-dispatch.md` §1: both rules carry
/// numeric-suffix aliases across the four C-family grammars, and a
/// `kind_id` match would have to enumerate every one of them and would
/// regress silently on the next grammar bump. C and Objective-C simply
/// never emit `operator_cast`.
const CONVERSION_OPERATOR: &str = "operator_cast";
const ATTRIBUTE: &str = "attribute_declaration";
const TEMPLATE_ARGUMENTS: &str = "template_argument_list";

#[cfg(test)]
mod space_name_tests {
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
    /// would pass the first row and miss the population. `RUN_STATS_METHOD`
    /// is the *syntactic* answer: the name the declarator chain spells,
    /// which is the macro's rather than the function's, and which agrees
    /// with the single argument `nargs` reads from the same node.
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

    /// A C-family function's name comes off the same declarator its
    /// arity does (#1208).
    #[test]
    fn the_innermost_declarator_names_the_function_space() {
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
}
