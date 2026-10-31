//! Python's role rows: the per-consumer projection fixture (#1691). A
//! comparison is one `comparison_operator` wrapper holding one or more
//! operator tokens; one row names both occurrences, and which one a
//! consumer reads is decided where that consumer lives, not here. The
//! Halstead keyword roles (#1693), the paying-construct roles (#1694,
//! #1695), the space tree (#1696) and the control flow — `Branch`,
//! `Switch`, `Loop`, `Catch`, `ElseIf`, `Exit` (#1697) — are declared;
//! the rest are pending the package that declares each (#1692).

use crate::lang_helpers::case_arms::python_case_clause_counts;
use crate::lang_helpers::python::{python_is_block, python_is_bound_self_attribute};
use crate::role_table;
use crate::{Ancestors, Node, Python};

/// Whether a `case_clause` is a decision: its pattern is not a bare `_`,
/// or it carries an `if` guard (lesson 11, #469).
fn is_decision_clause<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    python_case_clause_counts(node)
}

/// Whether an `if` keyword is the ternary's (`a if c else b`), whose
/// `conditional_expression` the `Ternary` row owns and pays at the node:
/// that one is declined here so the keyword is not counted twice. Read
/// off the chain; the ternary has no field for its keyword, so the
/// parent's kind is the whole test.
fn not_under_ternary<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    !ancestors.parent_has_kind(node, Python::ConditionalExpression as u16)
}

/// Whether an `if_statement` is Python's `else if` (#276): the sole
/// named child of a `block` whose parent is an `else_clause` — the
/// shape `else_clause → block → if_statement`, where C's grammar has
/// `else_clause → if_statement`. A sibling statement in the block makes
/// it a real nested `if`; so does a comment there, an extra named
/// child, as it always has.
///
/// The two chain reads come first and the child scan last, and the
/// scan stops at the second named child: every `if` in a function body
/// reaches this gate, and the predicate it replaced counted the whole
/// enclosing block's children before asking whose block it was — 20,000
/// sibling `if`s under one `def` took 8 s, 40,000 took 34 s (#1697).
/// Now the count runs only under an `else:` block, over
/// at most two of its children. The block's two aliased kinds go through
/// `python_is_block` (#419).
fn sole_statement_of_else_block<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.iter(node).next().is_some_and(|(parent, above)| {
        python_is_block(&parent)
            && above.parent_has_kind(&parent, Python::ElseClause as u16)
            && parent.children().filter(Node::is_named).nth(1).is_none()
    })
}

/// Whether an `if_clause` is a `case` guard rather than a comprehension's
/// filter (`[y for y in x if y]`), which the grammar spells with the same
/// kind. An owner's parent gate, which the DSL's `under` (a payer
/// attribute) does not spell.
fn under_case_clause<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent_has_kind(node, Python::CaseClause as u16)
}

/// `python_is_bound_self_attribute`, under the row's gate signature.
fn is_bound_self_attribute<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    python_is_bound_self_attribute(node, code, ancestors)
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
            In under ComparisonOperator,
            Is under ComparisonOperator,
        ]
    }

    // `lambda` is a closure and opens no space: `is_closure` says yes,
    // `get_space_kind` says `Unknown` (`checker/python.rs`,
    // `getter/python.rs`).
    // `Lambda2` is the hidden alias (#419 / #422); the drift marker in
    // `checker/python.rs` pins it unseen.
    Closure { owns [Lambda, Lambda2] }

    FuncOpener { owns [FunctionDefinition(callable opens Function)] }
    Container { owns [Module(opens Unit), ClassDefinition(opens Class)] }
    // A `self.x` / `cls.x` where it is bound — the target of an
    // assignment inside a method, in the flat and the destructuring
    // spellings — which the attribute count reads by name (#412). The
    // row recognises the binding; which class it belongs to, the
    // nested-scope boundary, a class-level `x = 1`'s plain name and the
    // dedup across both are the consumer's reading (`npa/python.rs`).
    Member { owns [Attribute(text_gated_by is_bound_self_attribute)] }
    // `return`, `raise`, and the yield *expression* (`Yield`, kind text
    // "yield" — Python has no yield-statement node): a generator's
    // suspension hands control back to the caller, so the function does
    // leave even though it may later resume, as C#'s and PHP's `yield`
    // count.
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
    // A `case X if g:` guard is an `if_clause` under the `case_clause` —
    // the same kind a comprehension filter has, so the owner is gated to
    // the clause — one decision paid at the node: ABC walks its
    // expression as a boolean slot, and cyclomatic counts the `if`
    // keyword wherever it is written, which Halstead bills likewise.
    Guard {
        owns [IfClause(gated_by under_case_clause)]
        excludes [If "the keyword the `if` statement's kind spells, counted and billed as such"]
    }
    // `a if c else b` is a `conditional_expression` with no field and
    // no operator of its own: its `if` / `else` are the statement
    // keywords' kinds, which Halstead bills wherever they appear, so
    // the row is owner-only and the node pays. ABC pays it there and
    // reads only the condition (the branches would double-count through
    // the top-level walk); cognitive nests on it; cyclomatic pays the
    // node and declines the `if` keyword under it, which it would
    // otherwise count as a statement's.
    Ternary { owns [ConditionalExpression] }
    // A `case_clause` owns a decision and pays at the node — its `case`
    // keyword is contextual and bills as the keyword it is — unless its
    // pattern is a bare `_` with no guard, the `default:` of a `match`
    // (lesson 11, #469); a guarded `case _ if g:` still counts.
    CaseArm {
        owns [CaseClause(gated_by is_decision_clause)]
        excludes [Case "the arm's keyword, billed as such; the clause pays"]
    }
    // Python counts keyword tokens (#284), so each row below pays at
    // its keyword. The `if` keyword is one kind in three positions: the
    // statement's, the ternary's (`a if c else b`, the `Ternary` row's
    // owner, whose node pays instead) and a comprehension filter's
    // (`[x for x in xs if x]`, a decision of its own). Only the ternary's
    // is declined, by the payer gate; `assert` is a branch on its
    // condition, as every cyclomatic reading of Python has had it.
    Branch {
        owns [IfStatement, AssertStatement]
        pays [If gated_by not_under_ternary, Assert]
    }
    // The PEP 634 `match` statement, the modified tier's container; its
    // `case` clauses are the `CaseArm` row's. The `match` keyword is a
    // soft keyword the grammar spells as `Match`.
    Switch { owns [MatchStatement] }
    // `for` and `while` statements, and the comprehension `for … in`
    // clause — a loop of its own (#417), paid at the same `for` keyword.
    Loop {
        owns [ForStatement, WhileStatement, ForInClause]
        pays [For, While]
    }
    // An `except` clause, paid at its keyword; `except*` (PEP 654) is the
    // same clause and keyword at this pin. `try`, `else` and `finally`
    // are unconditional and pay nothing here — the loop / try `else`
    // that cyclomatic does count is a consumer policy on the `Else`
    // keyword, not a handler.
    Catch { owns [ExceptClause] pays [Except] }
    // `elif` is a clause of its own, a direct child of the
    // `if_statement`, paid at its keyword. The chained spelling
    // `else:` + a sole `if` in the block (#276) is the other owner:
    // recognised so cognitive opens no level for it — the `else`
    // already extended the branch — and paying nothing, since its `if`
    // keyword is `Branch`'s.
    ElseIf {
        owns [ElifClause, IfStatement(gated_by sole_statement_of_else_block)]
        pays [Elif]
    }
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

    // A `case` guard and a comprehension filter are one `if_clause`
    // kind; the `Guard` row owns only the one under a `case_clause`
    // (#1695). The `CaseArm` row owns every clause but the bare `case
    // _:`, and a guarded `_` still.
    #[cfg(feature = "python")]
    #[test]
    fn guard_owns_the_case_guard_and_not_a_comprehension_filter() {
        let source = b"match x:\n    case 1 if b: pass\n    case _ if b: pass\n    case _: pass\ny = [z for z in x if z]\n";
        let (mut guards, mut arms) = (Vec::new(), Vec::new());
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let text = || node.utf8_text(source).expect("fixture is UTF-8").to_owned();
            if guard::owns(node, source, ancestors) {
                guards.push(text());
            }
            if case_arm::owns(node, source, ancestors) {
                arms.push(text());
            }
        });
        assert_eq!(guards, ["if b", "if b"]);
        assert_eq!(arms, ["case 1 if b: pass", "case _ if b: pass"]);
    }

    // Until #1696 the space rows restated `checker/python.rs` and
    // `getter/python.rs` and this compared the two; now the classifiers
    // read the rows, so the expected `(is_func, is_closure, promoted,
    // SpaceKind, is_member)` per node is pinned independently, as are
    // the `Call` and `StringLit` rows by kind string since R08 (#1698)
    // made the classifiers read those too. The per-shape tests below do
    // not cover membership: emptying `Exit` or dropping `Lambda2` from
    // `Closure` failed none of them.
    #[cfg(feature = "python")]
    #[test]
    fn classifiers_read_the_rows_and_the_rows_say_what_the_walk_did() {
        let source = b"class C:\n    cv = 1\n    def m(self):\n        f = lambda x: x\n        s = \"a\" \"b\"\n        g(1)\n        self.x = 1\n        self.a, (self.b, c) = t\n        self.x += 1\n        print(self.x)\n        other.y = 2\n        w = self.z\n        yield 1\n        raise E\n    @classmethod\n    def n(cls):\n        cls.k = 1\n        return 1\n";
        let mut attributes = Vec::new();
        let mut seen_kinds = std::collections::BTreeSet::new();
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let kind = node.kind();
            let promoted = PythonCode::promotes_to_func_space_with_code(node, source, ancestors);
            let got = (
                PythonCode::is_func(node, ancestors),
                PythonCode::is_closure(node, ancestors),
                promoted,
                PythonCode::get_space_kind_with_code(node, source, ancestors),
                member::owns(node, source, ancestors),
            );
            seen_kinds.insert(node.kind_id());
            assert_eq!(
                got.3 != SpaceKind::Unknown,
                promoted,
                "{kind}: opens_space vs promotion"
            );
            assert_eq!(
                PythonCode::is_func_space_with_code(node, source, ancestors),
                promoted,
                "{kind}: is_func_space is what the walk promotes"
            );
            // The byte-less pair is the kind table: the rows read no
            // chain and no bytes, so it agrees with the `_with_code` pair
            // on every node.
            assert_eq!(
                (
                    PythonCode::is_func_space(node),
                    PythonCode::get_space_kind(node)
                ),
                (promoted, got.3),
                "{kind}: the byte-less spellings are the kind table"
            );
            if kind == "attribute" {
                attributes.push((node.utf8_text(source).expect("UTF-8").to_owned(), got.4));
                assert_eq!(
                    (got.0, got.1, got.2, got.3),
                    (false, false, false, SpaceKind::Unknown)
                );
                return;
            }
            // The `lambda` *keyword* leaf (`Lambda3`) shares the kind
            // string; only the named node is the closure.
            let want = match kind {
                "module" => (false, false, true, SpaceKind::Unit, false),
                "class_definition" => (false, false, true, SpaceKind::Class, false),
                "function_definition" => (true, false, true, SpaceKind::Function, false),
                "lambda" if node.is_named() => (false, true, false, SpaceKind::Unknown, false),
                _ => (false, false, false, SpaceKind::Unknown, false),
            };
            assert_eq!(got, want, "{kind}");
            assert_eq!(
                call::owns(node, source, ancestors),
                kind == "call",
                "{kind}: Call"
            );
            assert_eq!(
                string_lit::owns(node, source, ancestors),
                matches!(kind, "string" | "concatenated_string"),
                "{kind}: StringLit"
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
        });
        // A bound `self.` / `cls.` attribute is a member — flat and
        // nested unpacking included; an augmented assignment, a call's
        // argument, a foreign receiver and an assignment's *value*
        // (`w = self.z`, the same `assignment` kind on the other side)
        // are not.
        assert_eq!(
            attributes,
            [
                ("self.x".to_owned(), true),
                ("self.a".to_owned(), true),
                ("self.b".to_owned(), true),
                ("self.x".to_owned(), false),
                ("self.x".to_owned(), false),
                ("other.y".to_owned(), false),
                ("self.z".to_owned(), false),
                ("cls.k".to_owned(), true),
            ]
        );
        for kinds in [
            exit::OWNERS,
            call::OWNERS,
            string_lit::OWNERS,
            closure::OWNERS,
            func_opener::OWNERS,
            container::OWNERS,
            member::OWNERS,
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

    /// The `Member` gate climbs only unpacking wrappers to the
    /// assignment. A `self.` attribute under any other node on the
    /// target side — the receiver of `self.x.y = …`, the index of
    /// `a[self.i] = …` — is declined by that wrapper-kind check, which
    /// nothing reaches through the `npa` walk (it stops at the outer
    /// node), so the row is asked here directly with each node's real
    /// chain. Dropping the check admits both (perturbation-verified).
    #[cfg(feature = "python")]
    #[test]
    fn member_declines_a_self_attribute_under_a_non_pattern_node() {
        let source = b"class C:\n    def __init__(self):\n        self.x.y = 1\n        a[self.i] = 2\n        self.k = 3\n";
        let mut answers = Vec::new();
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            if node.kind() == "attribute" {
                answers.push((
                    node.utf8_text(source).expect("fixture is UTF-8").to_owned(),
                    member::owns(node, source, Ancestors::known(chain)),
                ));
            }
        });
        assert_eq!(
            answers,
            [
                ("self.x.y".to_owned(), false),
                ("self.x".to_owned(), false),
                ("self.i".to_owned(), false),
                ("self.k".to_owned(), true),
            ]
        );
        // A `self.` attribute that is a `for` target, and one asked with
        // no chain at all (the climb runs out before any assignment):
        // neither is bound.
        let source = b"class C:\n    def m(self):\n        for self.i in xs:\n            pass\n";
        let mut answers = Vec::new();
        for_each_node_with_chain::<PythonCode>(source, |node, chain| {
            if node.kind() == "attribute" {
                answers.push((
                    member::owns(node, source, Ancestors::known(chain)),
                    member::owns(node, source, Ancestors::known(&[])),
                ));
            }
        });
        assert_eq!(answers, [(false, false)]);
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
