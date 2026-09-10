//! The C-family declarator-chain walk, shared by the two surfaces that
//! need it.
//!
//! C declarator syntax nests outward from the declared name, so neither
//! a function's parameter list nor its name is reliably a child of the
//! function node itself. Both are found from the one node
//! [`innermost_declarator`] returns — the innermost link on the chain
//! that is the function's own declarator rather than its return type's:
//!
//! - `big_code_analysis::metrics::nargs` reads its `parameters` field (#1200).
//! - The `Getter::get_func_space_name` impls for C, C++, mozcpp and
//!   Objective-C read its name side through [`declarator_name`] (#1208).
//!
//! Keeping one walk keeps the two answers about the same function from
//! disagreeing, which is how #1208 arose: the arity came from this
//! chain and the name came from a leftmost pre-order search that stopped
//! one level too early. The invariant is one *function*, one walk —
//! not one node: an unexpanded function-like macro puts the arity and
//! the name on two links of the chain, which each function's own doc
//! below explains (#1213).

use crate::checker::Checker;
use crate::node::Node;

/// The innermost declarator along a C-family function's declarator
/// chain: the node whose `parameters` field holds the function's *own*
/// formal arguments. [`declarator_name`] takes the name from the same
/// node's `declarator` field.
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
/// "Innermost" has one exception, and it is the one shape where the
/// grammar's reading and the preprocessor's disagree. An unexpanded
/// function-like macro — `RUN_STATS_METHOD(allocate)(JNIEnv *env,
/// jclass clazz)`, which is what every JNI shim looks like — parses as
/// a `function_declarator` sitting in another one's `declarator` field,
/// so the innermost list is the macro's `(allocate)` and the function's
/// own arguments are discarded. Neither language permits that chain: a
/// function may not return a function type (C11 6.7.6.3p1, C++
/// `[dcl.fct]`), so a legitimate function returning a function pointer
/// always interposes a `parenthesized_declarator`, and the direct
/// nesting can only be a macro (or an `ERROR`, below). The walk stops
/// at the outer link there and reports the function's arity (#1213).
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
///
/// Give that macro an argument — `T *f() TF_LOCKS_EXCLUDED(mu_) { … }`,
/// which is the spelling the TensorFlow / Abseil annotations actually
/// take — and it is a `function_declarator` carrying `parameters`, so
/// the walk answers with the *macro's* name rather than with nothing.
/// That is the one shape this change made worse: the leftmost pre-order
/// search it replaced descended into the `ERROR` and got `f` right.
/// `a_parenthesised_macro_takes_the_name_of_the_function_it_annotates`
/// pins it.
///
/// Recovery also manufactures the direct `function_declarator` nesting
/// the macro rule keys on, from source containing no macro-obscured
/// declarator at all. A *statement* macro followed by an `if` —
/// TensorFlow's `TF_ASSIGN_OR_RETURN(bool ok, Try(x)); if (ok) { … }` —
/// recovers into a `function_declarator` whose `declarator` field is the
/// macro call and whose `parameters` field is the `if` **condition**. So
/// the rule changes the answer for 19 of the 46 corpus spaces it
/// touches, from the macro's argument count to the condition's, neither
/// of which is an arity. There is no fixture for it: whether the
/// grammar recovers this way depends on where the line breaks fall,
/// tree-sitter costing a recovery by the extent it skips, so any pinned
/// spelling would be a claim about whitespace (#1213).
///
/// Whatever any strategy returns there is arbitrary, and the walk does
/// not try to be clever about it. Measured over `DeepSpeech` and
/// `pdf.js` (14,269 files), moving the four getters onto this walk
/// named 46 previously-nameless function spaces, un-named 2 and renamed
/// 4 — 354 nameless spaces down to 310, a net 44. All six of the latter
/// sit inside recovery subtrees: one of the un-named had been reporting
/// an `if` statement's callee as a function name, and one of the renamed
/// is the `TF_LOCKS_EXCLUDED` case above (#1208).
#[must_use]
pub fn innermost_declarator<'tree, T: Checker>(node: &Node<'tree>) -> Option<Node<'tree>> {
    // The chain starts at the `declarator` field rather than at `node`
    // so the last-named-child fallback can never fire on the function
    // node itself and walk into its body. The walk runs outside-in, so
    // the innermost qualifying link is the last one it yields.
    std::iter::successors(
        node.child_by_field_name("declarator"),
        |current| match current.child_by_field_name("declarator") {
            // An unexpanded function-like macro standing in for the
            // declarator, which is the shape JNI shims take. Neither C
            // nor C++ lets a function return a function type (C11
            // 6.7.6.3p1, C++ `[dcl.fct]`), so a `function_declarator`
            // directly inside another one's `declarator` field is not a
            // declarator chain at all: the outer list is the function's
            // own and the inner one holds the macro's arguments. Both
            // links have to be tested — a pointer return puts a
            // `function_declarator` in a `pointer_declarator`'s
            // `declarator` field, and stopping *there* would end the
            // chain on a node carrying no `parameters` and report 0
            // (#1213).
            Some(inner)
                if current.kind() == FUNCTION_DECLARATOR && inner.kind() == FUNCTION_DECLARATOR =>
            {
                None
            }
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
/// It is the `declarator` field of [`innermost_declarator`], and it is a
/// separate function only so the four `get_func_space_name` impls state
/// that pairing once instead of four times. Each caller still gates the
/// result on its own grammar's identifier kinds: what counts as a name
/// is where C, C++ and Objective-C differ (`destructor_name`,
/// `qualified_identifier`, `operator_name`, `template_function`), and a
/// kind this module accepted on their behalf would be a claim about
/// four grammars made in a module that reads none of them.
///
/// The macro shape [`innermost_declarator`] stops at is the one place
/// the name and the arity come off different nodes. There that
/// `declarator` field is the macro *invocation* — itself a
/// `function_declarator`, which no getter's identifier gate accepts — so
/// the walk descends through it to the identifier the macro spells.
/// `RUN_STATS_METHOD` is the only name in the source: the real
/// `Java_…_allocate` exists only after `##` pasting, and it is the token
/// a reader greps for. This is why the module doc states the invariant
/// per *function* rather than per node (#1213).
#[must_use]
pub fn declarator_name<'tree, T: Checker>(node: &Node<'tree>) -> Option<Node<'tree>> {
    // A run of them rather than one: `A(b)(c)(int x)` is two nested
    // invocations and the name is still `A`. Each step descends a finite
    // tree, so this terminates for the same reason the walk above does.
    //
    // Written as a chain rather than a `while` with `?` inside it
    // deliberately. The loop form needs an early return for "this
    // `function_declarator` has no `declarator` field", which the
    // grammars declare required and only an `ERROR` could violate — two
    // arms no test can reach, and coverage counts them.
    //
    // The two forms are not identical on that unreachable input, which is
    // worth stating rather than leaving for the next reader to rediscover
    // (#1220). On an `ERROR` tree where a `function_declarator` lacks its
    // `declarator` field, the loop returned `None` and this chain yields
    // that `function_declarator` itself — the whole declarator span in
    // place of a name.
    //
    // Nothing observes the difference, and the reason is external to this
    // module: every caller gates the result on its own grammar's
    // identifier kinds (`TypeIdentifier | Identifier | FieldIdentifier`,
    // plus the C++ name forms in `getter/cpp.rs` and `getter/mozcpp.rs`),
    // and `function_declarator` is in none of those lists. The `matches!`
    // falls through and `get_func_space_name` returns `None` — the same
    // answer the loop gave. That dependency is the thing to preserve: a
    // getter that widened its gate to accept `function_declarator` would
    // start naming functions after their whole declarator span on
    // malformed input, and this comment is the only place that says so.
    std::iter::successors(
        innermost_declarator::<T>(node)?.child_by_field_name("declarator"),
        |link| {
            if link.kind() == FUNCTION_DECLARATOR {
                link.child_by_field_name("declarator")
            } else {
                None
            }
        },
    )
    .last()
}

/// Compared by `kind()` string rather than `kind_id`, per
/// `.claude/rules/grammar-dispatch.md` §1: every rule below carries
/// numeric-suffix aliases across the four C-family grammars, and a
/// `kind_id` match would have to enumerate every one of them and would
/// regress silently on the next grammar bump. C and Objective-C simply
/// never emit `operator_cast`.
const CONVERSION_OPERATOR: &str = "operator_cast";
const ATTRIBUTE: &str = "attribute_declaration";
const TEMPLATE_ARGUMENTS: &str = "template_argument_list";
/// Carries the most aliases of the four — `FunctionDeclarator2` through
/// `FunctionDeclarator5` in `tree-sitter-c` alone — so it is the one the
/// `kind()`-string rule above most needs to cover.
const FUNCTION_DECLARATOR: &str = "function_declarator";
