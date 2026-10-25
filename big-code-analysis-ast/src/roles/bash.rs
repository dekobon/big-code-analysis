//! Bash's role rows. The Halstead keyword roles (#1693) and the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — are
//! declared; the rest are pending the package that declares each (#1692).

use crate::lang_helpers::bash::bash_test_connective;
use crate::lang_helpers::case_arms::bash_case_item_is_bare_wildcard;
use crate::role_table;
use crate::{Ancestors, Bash, Node};

/// Whether a `case_item` is a decision arm rather than the `*)`
/// catch-all: exactly one `value` field spelling `*` (lesson 11, #469).
/// `a|b)` and `*|b)` are not bare and still count.
fn is_decision_item<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    !bash_case_item_is_bare_wildcard(node, code)
}

/// Whether a `test_operator` is a comparison rather than one of the
/// `-a` / `-o` connectives: the grammar gives `-z`, `-n`, `-eq`, `-lt`,
/// `-a`, `-o`, … one kind, so only the bytes tell them apart (#1536).
fn is_test_comparison<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors
        .parent(node)
        .is_none_or(|parent| bash_test_connective(node, &parent, code).is_none())
}

/// Whether a `test_operator` is an `-a` / `-o` connective.
fn is_test_connective<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors
        .parent(node)
        .is_some_and(|parent| bash_test_connective(node, &parent, code).is_some())
}

/// Whether a `binary_expression`'s `operator` field is one of the
/// `Comparison` payers, with a `test_operator` read for its bytes.
fn comparison_operator<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_some_and(|op| {
        comparison::PAYERS.contains(&op.kind_id())
            && (op.kind_id() != Bash::TestOperator as u16
                || bash_test_connective(&op, node, code).is_none())
    })
}

/// Whether a `binary_expression`'s `operator` field is a short-circuit
/// payer, with a `test_operator` read for its bytes.
fn short_circuit_operator<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    short_circuit::project(node).is_some_and(|op| {
        let kind = op.kind_id();
        if kind == Bash::TestOperator as u16 {
            bash_test_connective(&op, node, code).is_some()
        } else {
            short_circuit::PAYERS.contains(&kind)
        }
    })
}

role_table! {
    lang: Bash;
    // Every command is a call: Bash has no other call kind, and the
    // `command` node covers the builtin, the function and the external
    // program alike.
    Call { owns [Command] }
    // The heredoc body is `HeredocBody2`: tree-sitter-bash 0.25.1 emits
    // only that `heredoc_body` parser-node symbol in observed trees; the
    // duplicate `HeredocBody` entry plus the hidden `_heredoc_body`
    // (`HeredocBody3`) and `_simple_heredoc_body` (`SimpleHeredocBody`)
    // rules never surface and are left out. An expanding string
    // `wraps` the four expansion kinds, whose inner names are billed on
    // their own (#180); `raw_string` and `ansi_c_string` cannot expand,
    // and the `translated_string` (`$"…"`) is one `$` and a `string`
    // child that is the keeper in argument position (#1358), so the
    // operand arm bills the child rather than the wrapper.
    StringLit {
        owns [String, RawString, AnsiCString, TranslatedString, HeredocBody2]
        wraps [SimpleExpansion, Expansion, CommandSubstitution, ArithmeticExpansion]
    }
    // A comparison inside `[[ … ]]` / `(( … ))` is a `binary_expression`
    // (`BinaryExpression3` at the pinned grammar; the four siblings are
    // listed defensively) owning one of `==` `!=` `<=` `>=` `=~`, or `<`
    // / `>` under that parent (any alias) since the same two tokens
    // spell an I/O redirection (`cmd > out`, `read x < in`), which the
    // grammar parents under `file_redirect` (#1280). Inside `[ … ]` the
    // comparison is a `test_operator` —
    // `-eq`, `-lt`, `-z`, `-n`, … — one kind the grammar shares with the
    // `-a` / `-o` connectives, so that payer is text-gated: a
    // `test_operator` pays as a comparison exactly when its bytes are
    // not a connective. `=` compares inside a test and assigns inside
    // `(( … ))`, which only the test's descent can tell (#1536); the
    // ABC consumer counts it from the test, as its own extra.
    Comparison {
        owns [
            BinaryExpression(gated_by comparison_operator),
            BinaryExpression2(gated_by comparison_operator),
            BinaryExpression3(gated_by comparison_operator),
            BinaryExpression4(gated_by comparison_operator),
            BinaryExpression5(gated_by comparison_operator),
        ]
        pays [
            EQEQ, BANGEQ, LTEQ, GTEQ, EQTILDE,
            LT under [
                BinaryExpression, BinaryExpression2, BinaryExpression3, BinaryExpression4,
                BinaryExpression5,
            ],
            GT under [
                BinaryExpression, BinaryExpression2, BinaryExpression3, BinaryExpression4,
                BinaryExpression5,
            ],
            TestOperator text_gated_by is_test_comparison,
        ]
        project "operator"
    }
    // `&&` / `||` join two commands in a `list` and two tests inside
    // `[[ … ]]` / `(( … ))` in a `binary_expression`; inside `[ … ]` the
    // same join is the `-a` / `-o` spelling of `test_operator`, read by
    // text. Bash's `&&` / `||` are command-list separators, so the ABC
    // consumer reads this row for nothing (#1536 — Fitzpatrick Rule 9
    // does not map onto its grammar) where cyclomatic and cognitive
    // read it.
    ShortCircuit {
        owns [
            List,
            BinaryExpression(gated_by short_circuit_operator),
            BinaryExpression2(gated_by short_circuit_operator),
            BinaryExpression3(gated_by short_circuit_operator),
            BinaryExpression4(gated_by short_circuit_operator),
            BinaryExpression5(gated_by short_circuit_operator),
        ]
        pays [AMPAMP, PIPEPIPE, TestOperator text_gated_by is_test_connective]
        project "operator"
    }
    // `-a` / `-o` inside `[ ]` are arguments to the `test` builtin,
    // which evaluates both sides (`[ 1 -eq 2 -a nope -eq 1 ]` still
    // complains about `nope`), so by the language they are eager. Every
    // consumer scores them as the `[[ … && … ]]` twin they spell
    // (#1536), so they are the `ShortCircuit` row's text-gated payer
    // above: a policy this package records and does not move. The
    // bitwise `&` / `|` / `^` inside `(( … ))` are the only other
    // candidates, and are integral.
    EagerLogical {
        owns []
        excludes [
            TestOperator "`-a` / `-o` are scored as the `&&` / `||` they spell (#1536), so the \
                 `ShortCircuit` row claims them; the builtin's eager evaluation is recorded here",
        ]
    }
    // The arithmetic `(( a ? b : c ))` is Bash's only ternary: the
    // `ternary_expression` owns it (`TernaryExpression2` is the second
    // id carrying the kind, listed per grammar-dispatch §1 and
    // unreachable at the pinned grammar) and its `?` pays, gated to that
    // owner since `${y?msg}` spells the same token. `QMARK2` is the
    // grammar's second `?` id, which no spelling at the pinned grammar
    // emits — `bash_ternary_expression_alias_is_unreachable` in the
    // cyclomatic tests pins that beside the owner alias — so it is
    // recorded as considered rather than paid; the getter still bills
    // it as the operator it names, should a bump start emitting it.
    Ternary {
        owns [TernaryExpression, TernaryExpression2]
        pays [QMARK under [TernaryExpression, TernaryExpression2]]
        project "operator"
        excludes [QMARK2 "the second `?` id, which no spelling at the pinned grammar emits"]
    }
    NullSafe { absent "Bash has no optional navigation or null coalescing" }
    // A `case_item` (`CaseItem2` the second id carrying the kind, per
    // grammar-dispatch §1) owns a decision and pays at the node — the
    // arm has no keyword; `case` and `esac` are the statement's and bill
    // as the keywords they are — unless its sole pattern is `*`, the
    // catch-all (lesson 11, #469), which only the bytes tell.
    CaseArm {
        owns [CaseItem(text_gated_by is_decision_item), CaseItem2(text_gated_by is_decision_item)]
    }
    Guard { absent "Bash has no guard clause; a `case` pattern is a glob, not a condition" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { absent "Bash has no exception handling; `trap` is a command" }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Bash declares with builtins (`local`, `readonly`), which are commands, not \
         modifier keywords" }
    // The declaration builtins the grammar gives a kind of their own:
    // `function`, and `local` / `declare` / `typeset` / `export` /
    // `readonly`, each the leaf of a `declaration_command` or a
    // `function_definition`, neither listed. `unset` / `unsetenv` are
    // the inverse and bill with the commands.
    DeclKeyword { owns [Function, Local, Declare, Typeset, Export, Readonly] }
    Primitive { absent "Bash has no type names" }
    FuncOpener { owns [FunctionDefinition(callable opens Function)] }
    Container { owns [Program(opens Unit)] }
    Closure { absent "Bash has no anonymous function" }
    Member { absent "Bash has no container to hold a member" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Bash documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BashCode;
    use crate::test_support::for_each_node_with_chain;

    // The `Ternary` row's `?` is the arithmetic ternary's alone: an
    // expansion's `${y?msg}` spells the same bare token and pays
    // nothing, and `$?` is a token of its own (#1695). ABC reads the
    // row at its wrapper and Halstead's byte-less arm keeps billing the
    // expansion's `?`, so this is the one place the payer gate is
    // observed.
    #[cfg(feature = "bash")]
    #[test]
    fn ternary_pays_at_the_arithmetic_question_mark_only() {
        let source = b"c=$(( a ? b : x ))\necho ${y?msg} $?\n";
        let (mut owned, mut paid) = (Vec::new(), Vec::new());
        for_each_node_with_chain::<BashCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let text = || node.utf8_text(source).expect("fixture is UTF-8").to_owned();
            if ternary::owns(node, source, ancestors) {
                owned.push(text());
            }
            if ternary::pays_at_leaf(node, source, ancestors) {
                paid.push((node.start_byte(), text()));
            }
        });
        assert_eq!(owned, ["a ? b : x"]);
        assert_eq!(paid, [(8, "?".to_owned())]);
    }

    // The `Comparison` and `ShortCircuit` owner gates read the one
    // `test_operator` kind for its bytes, as the payer gates do: a
    // `binary_expression` holding `-lt` / `-gt` owns a comparison and
    // one holding `-a` / `-o` owns a chain, never both. The consumers
    // read these rows at the token, so only this pins the owner side,
    // and each payer's parent is held to its own row. The first test's
    // operators nest left — `(($a -lt $b) -a $c) -gt 1` — so its
    // outermost node holds `-gt` and the chain is its left operand; the
    // `=` of the second is the `EQ` kind, which the row leaves to the
    // consumer (#1536), so that node owns nothing. (`-a` / `-o` are
    // `[ ]`'s; inside `[[ ]]` they are a syntax error.)
    #[cfg(feature = "bash")]
    #[test]
    fn test_operators_sort_their_owners_by_their_bytes() {
        let source = b"if [ $a -lt $b -a $c -gt 1 ]; then :; fi\n\
if [ \"$s\" = x -o \"$t\" != y ]; then :; fi\n";
        let mut owned: [Vec<String>; 2] = Default::default();
        let mut paid: [Vec<String>; 2] = Default::default();
        for_each_node_with_chain::<BashCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let text = || node.utf8_text(source).expect("fixture is UTF-8").to_owned();
            let rows = [
                (
                    comparison::owns(node, source, ancestors),
                    comparison::pays_at_leaf(node, source, ancestors),
                ),
                (
                    short_circuit::owns(node, source, ancestors),
                    short_circuit::pays_at_leaf(node, source, ancestors),
                ),
            ];
            assert!(!(rows[0].0 && rows[1].0), "{}", text());
            assert!(!(rows[0].1 && rows[1].1), "{}", text());
            for (row, (owns, pays)) in rows.into_iter().enumerate() {
                if owns {
                    owned[row].push(text());
                }
                if pays {
                    paid[row].push(text());
                    // The payer's parent is the owner, asked with the
                    // chain above it.
                    let (parent, above) = chain.split_last().expect("a payer has a parent");
                    let row_owns = [comparison::owns, short_circuit::owns][row];
                    assert!(
                        row_owns(parent, source, Ancestors::known(above)),
                        "{}",
                        text()
                    );
                }
            }
        });
        assert_eq!(
            owned[0],
            ["$a -lt $b -a $c -gt 1", "$a -lt $b", "x -o \"$t\" != y"]
        );
        assert_eq!(owned[1], ["$a -lt $b -a $c", "x -o \"$t\""]);
        assert_eq!(paid[0], ["-lt", "-gt", "!="]);
        assert_eq!(paid[1], ["-a", "-o"]);
    }
}
