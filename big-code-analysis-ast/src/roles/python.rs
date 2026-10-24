//! Python's role rows: the per-consumer projection fixture (#1691). A
//! comparison is one `comparison_operator` wrapper holding one or more
//! operator tokens; one row names both occurrences, and which one a
//! consumer reads is decided where that consumer lives, not here.

use crate::role_table;
use crate::{Ancestors, Node, Python};

/// Whether a bare `in` / `is` token is a comparison: its parent is the
/// `comparison_operator` wrapper. The same tokens sit inside the
/// compound `not in` / `is not` nodes, whose parent is the compound
/// and which pay as one operator (#413), and `in` opens a `for`
/// clause, whose parent is the `for_statement`; the one parent check
/// excludes both, so no separate "not inside a compound" gate is
/// needed — a perturbation of such a gate failed nothing (#1691).
fn under_comparison<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent_has_kind(node, Python::ComparisonOperator as u16)
}

role_table! {
    lang: Python;

    // `a < b < c` is one `comparison_operator` node holding two tokens:
    // the wrapper is one occurrence of the construct, each token one
    // occurrence of an operator. `not in` / `is not` are compound nodes
    // the grammar nests the bare leaves inside, so the compounds pay and
    // their leaves are gated out (grammar-dispatch §5). `In` also opens
    // a `for` clause; it and `Is` are gated to the wrapper so `for x in
    // y` pays nothing here.
    Comparison {
        owns [ComparisonOperator]
        pays [
            LT, LTEQ, EQEQ, BANGEQ, GTEQ, GT, LTGT,
            Notin, Isnot,
            In gated_by under_comparison,
            Is gated_by under_comparison,
        ]
    }

    // `lambda` is a closure and opens no space: `is_closure` says yes,
    // `get_space_kind` says `Unknown` (`checker/python.rs`,
    // `getter/python.rs`).
    // `Lambda2` is the hidden alias (#419 / #422); the drift marker in
    // `checker/python.rs` pins it unseen.
    Closure { owns [Lambda, Lambda2] }

    FuncOpener { owns [FunctionDefinition(opens Function)] }
    Container { owns [Module(opens Unit), ClassDefinition(opens Class)] }
    Exit { owns [ReturnStatement, RaiseStatement, Yield] }
    Call { owns [Call] }
    // `"a" "b"` is a `concatenated_string` holding two `string`s; the
    // wrapper is a string literal to `find` / `count` (`checker/python.rs`)
    // and holds no interpolation of its own — its parts do.
    StringLit { owns [String, ConcatenatedString] wraps [Interpolation] }

    // `a and b or c` is a left-nested chain of `boolean_operator`
    // nodes, each wrapping one `and` / `or` token; the two tokens occur
    // in no other production, so they pay ungated. The wrapper is the
    // owner, and which of the two a consumer pays at is its own policy
    // (ABC scores the operands and never the operator, #395 / #1526;
    // cyclomatic counts the token).
    ShortCircuit { owns [BooleanOperator] pays [And, Or] }

    NullSafe { absent "Python has no `?.` or `??`" }
    // `and` / `or` are lazy. `&` / `|` / `^` on two `bool`s evaluate
    // both sides and return a `bool`, but the grammar emits them as the
    // same `binary_operator` as the integral forms, so the syntax alone
    // cannot tell them apart; no consumer treats them as a decision,
    // and the tokens bill as the operators they are. A recognition
    // boundary, recorded as the kinds considered rather than as an
    // absence.
    EagerLogical {
        owns []
        excludes [
            AMP "a boolean `&` is the integral `&`'s kind, which the grammar does not tell apart",
            PIPE "a boolean `|` is the integral `|`'s kind, which the grammar does not tell apart",
            CARET "a boolean `^` is the integral `^`'s kind, which the grammar does not tell apart",
        ]
    }
    Guard { pending R05 }
    // `a if c else b` is a `conditional_expression` with no field and
    // no operator of its own: its `if` / `else` are the statement
    // keywords' kinds, which Halstead bills wherever they appear, so
    // the row is owner-only and the node pays. ABC pays it there and
    // reads only the condition (the branches would double-count through
    // the top-level walk); cognitive nests on it; cyclomatic pays the
    // node and declines the `if` keyword under it, which it would
    // otherwise count as a statement's.
    Ternary { owns [ConditionalExpression] }
    CaseArm { pending R05 }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Modifier { absent "Python has no modifier keywords" }
    // `def` and `class` (#1550), the leaves of `function_definition` /
    // `class_definition`, neither listed, and `lambda`'s keyword token
    // `Lambda3` (#413): `Lambda` / `Lambda2` are the expression nodes
    // that wrap it and bill nothing, so the leaf is one operator.
    // `global` / `nonlocal` are not here: each rebinds the scope of a
    // name declared elsewhere rather than declaring one, so they bill
    // with the statement keywords (`pass`, `del`) in `getter/python.rs`.
    DeclKeyword {
        owns [Def, Class, Lambda3]
        excludes [
            Global "scopes a name declared elsewhere; bills as a statement keyword",
            Nonlocal "scopes a name declared elsewhere; bills as a statement keyword",
        ]
    }
    Primitive { absent "Python's built-in types are ordinary identifiers" }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { absent "Python documents with docstrings, not comments" }
    Docstring { pending R09 }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_each_node_with_chain;
    use crate::{Checker, Getter, PythonCode, SpaceKind};

    // The rows restate `checker/python.rs` and `getter/python.rs`; this
    // pins the restatement node by node (lesson 35). The per-shape tests
    // below do not cover membership: emptying `Exit` or dropping
    // `Lambda2` from `Closure` failed none of them.
    #[cfg(feature = "python")]
    #[test]
    fn membership_rows_agree_with_the_live_classifiers() {
        let source = b"class C:\n    def m(self):\n        f = lambda x: x\n        s = \"a\" \"b\"\n        g(1)\n        yield 1\n        raise E\n    def n(self):\n        return 1\n";
        let mut seen_kinds = std::collections::BTreeSet::new();
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind = node.kind();
            assert_eq!(
                call::owns(node, source, ancestors),
                PythonCode::is_call_with_code(node, source, ancestors),
                "{kind}: Call"
            );
            assert_eq!(
                string_lit::owns(node, source, ancestors),
                PythonCode::is_string_with_code(node, source, ancestors),
                "{kind}: StringLit"
            );
            assert_eq!(
                closure::owns(node, source, ancestors),
                PythonCode::is_closure(node, ancestors),
                "{kind}: Closure"
            );
            assert_eq!(
                func_opener::owns(node, source, ancestors)
                    || container::owns(node, source, ancestors),
                PythonCode::promotes_to_func_space_with_code(node, source, ancestors),
                "{kind}: space promotion"
            );
            assert_eq!(
                opens_space(node, source, ancestors),
                PythonCode::get_space_kind_with_code(node, source, ancestors),
                "{kind}: SpaceKind"
            );
            // `Exit` has no classifier in this crate; its three kinds are
            // pinned by name. `yield` is the expression node, not the
            // `Yield2` keyword inside it.
            assert_eq!(
                exit::owns(node, source, ancestors),
                matches!(kind, "return_statement" | "raise_statement" | "yield")
                    && node.kind_id() != Python::Yield2 as u16,
                "{kind}: Exit"
            );
            seen_kinds.insert(node.kind_id());
        });
        for kinds in [
            exit::OWNERS,
            call::OWNERS,
            string_lit::OWNERS,
            closure::OWNERS,
            func_opener::OWNERS,
            container::OWNERS,
        ] {
            for &k in kinds {
                // `Lambda2` is the hidden alias, pinned unseen in
                // `checker/python.rs`'s drift guard.
                if k == Python::Lambda2 as u16 {
                    continue;
                }
                assert!(seen_kinds.contains(&k), "fixture never emits kind {k}");
            }
        }
        // No parsed node can reach `Lambda2`, so the alias set is pinned
        // as a slice against the chokepoint `is_closure` reads.
        assert_eq!(
            closure::OWNERS,
            &[Python::Lambda as u16, Python::Lambda2 as u16]
        );
    }

    /// `(text, wrapper, leaf)` for every node either projection claims.
    #[cfg(feature = "python")]
    fn paying(source: &[u8]) -> Vec<(String, bool, bool)> {
        let mut seen = Vec::new();
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let wrapper = comparison::pays_at_wrapper(node, source, ancestors);
            let leaf = comparison::pays_at_leaf(node, source, ancestors);
            if wrapper || leaf {
                seen.push((
                    node.utf8_text(source).expect("fixture is UTF-8").to_owned(),
                    wrapper,
                    leaf,
                ));
            }
        });
        seen
    }

    // A chained comparison is one wrapper and two leaves; the compound
    // `not in` pays as one leaf and its inner `not` / `in` do not; the
    // `for` clause's `in` pays nothing; `not x` pays nothing.
    #[cfg(feature = "python")]
    #[test]
    fn wrapper_and_leaf_projections_name_different_occurrences() {
        let source = b"def f(a, b, c, xs):\n    for x in xs:\n        if not (a < b <= c) and x not in xs and a is not b:\n            pass\n";
        assert_eq!(
            paying(source),
            [
                ("a < b <= c".to_owned(), true, false),
                ("<".to_owned(), false, true),
                ("<=".to_owned(), false, true),
                ("x not in xs".to_owned(), true, false),
                ("not in".to_owned(), false, true),
                ("a is not b".to_owned(), true, false),
                ("is not".to_owned(), false, true),
            ]
        );
    }

    // The bare `in` / `is` only pay inside a comparison; the `for`
    // clause's `in` is the one occurrence no projection claims.
    #[cfg(feature = "python")]
    #[test]
    fn bare_in_and_is_pay_only_under_the_wrapper() {
        let source = b"for x in xs:\n    y = x in xs\n    z = x is None\n";
        assert_eq!(
            paying(source),
            [
                ("x in xs".to_owned(), true, false),
                ("in".to_owned(), false, true),
                ("x is None".to_owned(), true, false),
                ("is".to_owned(), false, true),
            ]
        );
        // Fixture guard: the `for` clause's `in` is present and distinct.
        let mut in_tokens = 0;
        for_each_node_with_chain::<PythonCode>(source, |node, _| {
            in_tokens += usize::from(node.kind_id() == Python::In as u16);
        });
        assert_eq!(in_tokens, 2);
    }

    // The concatenation wrapper and its parts are each owned; only the
    // part holding an `{x}` wraps interpolation.
    #[cfg(feature = "python")]
    #[test]
    fn string_lit_row_owns_the_concatenation_and_its_parts() {
        let source = b"s = \"a\" f\"{x}\"\n";
        let mut literals = Vec::new();
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            if string_lit::owns(node, source, Ancestors::known(chain)) {
                literals.push((node.kind(), node.wraps_any(string_lit::WRAPS)));
            }
        });
        assert_eq!(
            literals,
            [
                ("concatenated_string", false),
                ("string", false),
                ("string", true),
            ]
        );
    }

    #[cfg(feature = "python")]
    #[test]
    fn a_lambda_is_a_closure_that_opens_no_space() {
        let source = b"f = lambda x: x\ndef g():\n    pass\n";
        let mut seen = Vec::new();
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let closure = closure::owns(node, source, ancestors);
            let space = opens_space(node, source, ancestors);
            if closure || space != SpaceKind::Unknown {
                seen.push((node.kind(), closure, space));
            }
        });
        assert_eq!(
            seen,
            [
                ("module", false, SpaceKind::Unit),
                ("lambda", true, SpaceKind::Unknown),
                ("function_definition", false, SpaceKind::Function),
            ]
        );
    }
}
