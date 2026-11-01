//! Perl's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — and
//! the documentation rows (#1699) are declared.

use crate::lang_helpers::doc_comment::{node_bytes, pod_comment_region};
use crate::lang_helpers::perl::{
    perl_and_is_operator, perl_bareword_is_call, perl_bareword_is_key,
};
use crate::role_table;
use crate::{Ancestors, Node, Perl};

/// Whether a POD block is documentation rather than a `=begin comment`
/// region — Perl's idiom for commenting code out, which perldoc skips —
/// read from the bytes.
fn pod_documents<'a>(node: &Node<'a>, code: &[u8], _: Ancestors<'a, '_>) -> bool {
    !pod_comment_region(node_bytes(node, code))
}

/// Whether an expression holds one of the `ShortCircuit` payers among
/// its tokens, with an `and` counted only where it is the operator and
/// not an auto-quoted key (see `applied_and`).
fn short_circuit_operator<'a>(
    node: &Node<'a>,
    _code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    node.children().any(|token| {
        let kind = token.kind_id();
        if kind == Perl::And as u16 {
            perl_and_is_operator(&token, node)
        } else {
            short_circuit::PAYERS.contains(&kind)
        }
    })
}

/// Whether an `and` token is the operator of its chain rather than an
/// auto-quoted hash key (`(and => 1)`), which tree-sitter-perl parses
/// as a `unary_expression` whose first child is the token (#1539).
fn applied_and<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors
        .parent(node)
        .is_some_and(|parent| perl_and_is_operator(node, &parent))
}

/// A bareword-only call (`shift`, `time`, …) that is a call: only when
/// this node is the outermost dispatch site — when the bareword sits
/// inside one of the argument-bearing wrappers, the outer node is the
/// call and this child would double it — and not an auto-quoted key
/// (FIXME(#1571 upstream)): the grammar reads `(name => 1)`, `(-name =>
/// 1)` and `$h{-name}` as calls of `name`, and `$h{-foo}` as a file test
/// on a bareword `oo`, where Perl holds a string (#1545, #1572).
/// `perl_bareword_is_call` reads both off the chain.
fn bareword_is_call<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    perl_bareword_is_call(node, ancestors)
}

/// Whether a bareword call spells `die` or `exit` — the abrupt-exit
/// builtins — or their `CORE::` spellings (the way code that has
/// overridden `die` reaches the real one), and is not an auto-quoted
/// hash key (`(die => 1)`, FIXME(#1571 upstream)). The text test runs
/// first: nearly every bareword fails it, and the key test reads a
/// sibling off the chain. Invalid UTF-8 in the word reads as no match.
fn die_or_exit_not_key<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    matches!(
        node.utf8_text(code),
        Some("die" | "exit" | "CORE::die" | "CORE::exit")
    ) && !perl_bareword_is_key(node, ancestors)
}

role_table! {
    lang: Perl;
    // The grammar spells a call by its argument shape — six
    // `call_expression_*` kinds — plus a method invocation. A bareword
    // call (`foo;`) is the one spelling that is also a hash key
    // (`{ foo => 1 }`, `-foo`) and the callee of a spaced call, which
    // `perl_bareword_is_call` tells apart by the parent and the key
    // position; `NArgs` asks the same question, so the two count the
    // same calls.
    Call {
        owns [
            CallExpressionWithSpacedArgs,
            CallExpressionWithSub,
            CallExpressionWithArgsWithBrackets,
            CallExpressionWithVariable,
            CallExpressionWithBareword(gated_by bareword_is_call),
            CallExpressionRecursive,
            MethodInvocation,
        ]
    }
    // The seven quoting forms, `heredoc_body_statement` included: it
    // wraps the body text (and any `interpolation` children) that
    // appears as a top-level statement after the `<<TAG`, the visible
    // literal node, treated as a string the way Bash's `heredoc_body`
    // is (#287). An interpolating form `wraps` an `interpolation` whose
    // scalar / array / hash variable is billed on its own (#199); the
    // `list_item` is `qw()`'s element, one operand each (#1353), which
    // the operand arm reads through the same `wraps` list since no
    // string admits a `list_item` and `word_list_qw` no `interpolation`
    // — `word_list_qw` and the pattern values are operands by the
    // getter's reading (#1314), not strings to `find`.
    StringLit {
        owns [
            StringSingleQuoted,
            StringDoubleQuoted,
            StringQQuoted,
            StringQqQuoted,
            BacktickQuoted,
            CommandQxQuoted,
            HeredocBodyStatement,
        ]
        wraps [Interpolation, ListItem]
    }
    // tree-sitter-perl names no `operator` field, so a
    // `binary_expression` owns a comparison when one of the row's
    // payers is among its children and the projection is the lookup.
    // The payers are the numeric `== != < > <= >= <=>`, the string
    // `eq ne lt gt le ge cmp`, and the bindings `=~` / `!~`, which bind
    // a match, a test or a rewrite — the rewrite (`$x =~ s///`) is an
    // edit and no test, a reading the ABC consumer keeps as its own
    // extra (#1540), as it keeps the bare `/re/` match against `$_`
    // (#1467), which has no token at all. The ABC consumer's
    // `perl_condition_scores_itself` reads this owner's projection
    // directly, so a gate added here must be read there too.
    Comparison {
        owns [BinaryExpression(holds_payer)]
        pays [
            EQEQ, BANGEQ, LTEQ, GTEQ, LTEQGT, Eq, Ne, Lt, Gt, Le, Ge, Cmp, EQTILDE, BANGTILDE,
            LT under BinaryExpression, GT under BinaryExpression,
        ]
        project "operator"
    }
    // `&&` / `||` and the low-precedence `and` / `or`; tree-sitter-perl
    // spells `and` as a two-operand `unary_expression`, so that kind
    // owns a chain too, and an `and` that is an auto-quoted key pays
    // nothing (#1539). The defined-or `//` is `NullSafe`'s and `xor`
    // is `EagerLogical`'s: it does not short-circuit (#1536).
    ShortCircuit {
        owns [
            BinaryExpression(gated_by short_circuit_operator),
            UnaryExpression(gated_by short_circuit_operator),
        ]
        pays [AMPAMP, PIPEPIPE, Or, And gated_by applied_and]
        project "operator"
    }
    // `xor` evaluates both operands, so it is a value as `^` is, not a
    // chain: no Rule-9 chain increment, and the slot holding one pays
    // once under the ordinary slot rule (#1536). The token is a kind of
    // its own that the grammar spells only as the operator — a hash key
    // `(xor => 1)` or `$h{xor}` is a bareword `identifier` — and Halstead
    // bills it as the operator it is, as before the row.
    EagerLogical {
        owns [BinaryExpression(holds_payer)]
        pays [Xor]
        project "operator"
    }
    // The `ternary_expression` owns it and its `?` pays, ungated: the
    // grammar spells the bare token nowhere else (a regex quantifier is
    // inside its pattern token). The ABC consumer pays at the node
    // rather than the token, for the #1273 misparse it has to route
    // around; one pays per ternary either way.
    Ternary {
        owns [TernaryExpression]
        pays [QMARK]
        project "operator"
    }
    // The defined-or `a // b`, a `binary_expression` owning it through
    // the `//` among its tokens: a short-circuit decision — cyclomatic
    // pays the token, cognitive folds it into a sequence under a key of
    // its own (#1530), and ABC walks its chain's operands as it walks an
    // `||`'s (Rule 9). The compound `//=` is an assignment to ABC, a
    // sequence to cognitive and a decision to cyclomatic (#249): the
    // consumers' own, recorded here. Perl has no optional navigation.
    NullSafe {
        owns [BinaryExpression(holds_payer)]
        pays [SLASHSLASH]
        project "operator"
        excludes [SLASHSLASHEQ "the compound `//=`, an assignment ABC pays as one and a decision the others pay as their own (#249)"]
    }
    // A `when` statement (`given` / `when`, feature "switch") owns a
    // decision and pays at the node; its keyword bills as the keyword
    // it is. tree-sitter-perl reaches the kind only under error recovery
    // at the pin, as the cyclomatic impl records.
    CaseArm {
        owns [WhenSimpleStatement]
        excludes [When "the arm's keyword, billed as such; the arm pays"]
    }
    Guard { absent "Perl has no guard clause" }
    // The block forms and the statement-modifier forms (`return 1 if
    // $c;`), each its own kind; the modifier's condition is a real
    // branch even though its body is one expression. The owner is the
    // paying node: Perl's cyclomatic counts statements, not keywords.
    // The `_if_simple` rule (`IfSimple`) is hidden and never emitted.
    Branch {
        owns [IfStatement, UnlessStatement, IfSimpleStatement, UnlessSimpleStatement]
    }
    // `given` / `when` are not productions of the pinned grammar:
    // `given ($x) { when (1) { … } }` parses as a bareword call
    // (`call_expression_with_bareword` spelling `given`) with its block
    // stranded under an `ERROR`, and the `when_simple_statement` the
    // `CaseArm` row owns is reached only by recovery. There is no
    // switch-like container to recognise.
    Switch { absent "the pinned tree-sitter-perl grammar has no `given` production; `given ($x) \
         { … }` parses as a bareword call with an ERROR tail" }
    // `for` / `foreach` are `ForStatement1` (C-style) and
    // `ForStatement2` (list iteration); the modifier forms
    // (`$a++ while $c;`, `print for @l;`) are their own kinds.
    Loop {
        owns [
            WhileStatement, UntilStatement, ForStatement1, ForStatement2,
            WhileSimpleStatement, UntilSimpleStatement, ForSimpleStatement,
        ]
    }
    Catch { absent "the pinned tree-sitter-perl grammar has no try or catch production; `eval { }` \
         parses as a block and `die` as a call" }
    // The grammar gives `elsif` a clause node of its own, a direct
    // child of the `if_statement`, so the clause *is* the else-if and
    // pays for itself; no nested `if` is ever an else-if here. A split
    // row with no payers: the clause pays at the leaf because it is
    // marked, and nothing else does (#1697).
    ElseIf { owns [ElsifClause(pays_itself)] pays [] }
    // `return`, and the abrupt-exit builtins `die` / `exit` (and their
    // `CORE::` spellings) — each a `call_expression_with_bareword`
    // whatever wrapper carries the arguments, so one owner per
    // occurrence (#1270), told by text, and not when the bareword is
    // an auto-quoted hash key (`(die => 1)`, #1585). `croak` /
    // `confess` are `Carp` library functions, not builtins, and are
    // left out; `Carp::croak` and any other package-qualified callee
    // keep the qualifier in the bareword text and never match; and
    // `$obj->die` is a `method_invocation` whose callee is a plain
    // `identifier` — no bareword call at all, so the owner kind never
    // fires.
    Exit {
        owns [ReturnExpression, CallExpressionWithBareword(text_gated_by die_or_exit_not_key)]
    }
    Modifier { absent "Perl has no modifier keywords; `my` / `our` are declarators" }
    // `sub` and `package` (the keyword kinds; `SUB` / `PACKAGE` are the
    // `__SUB__` / `__PACKAGE__` literals, operands) and the four
    // declarators `my` / `our` / `local` / `state`, each the leaf of an
    // unlisted `*_declaration` / `variable_declaration`.
    DeclKeyword { owns [Sub, Package, My, Our, Local, State] }
    Primitive { absent "Perl has no type names" }
    // A `sub f { }` in both grammar spellings is the callable; an
    // anonymous `sub { }` is the closure and opens a space. A `package`
    // is not a space, so Perl has no container but the file.
    FuncOpener {
        owns [
            FunctionDefinition(callable opens Function),
            FunctionDefinitionWithoutSub(callable opens Function),
            AnonymousFunction(opens Function),
        ]
    }
    Closure { owns [AnonymousFunction] }
    Container { owns [SourceFile(opens Unit)] }
    Member { absent "Perl has no container space to hold a member; a package is not one" }
    // POD is Perl's documentation syntax and its own kind; a `#`
    // comment never is. `pod_statement` sits on `DocComment` alone —
    // under a byte gate, since a `=begin comment` region is POD the
    // tool does not read — and `Comment` holds the `#` kind;
    // `is_comment` is the two kind tables as one, which keeps a comment
    // region a comment (#1699).
    Comment { owns [Comments] }
    DocComment { owns [PodStatement(text_gated_by pod_documents)] }
    Docstring { absent "Perl documents with POD, not string literals" }
    // perldoc extracts POD wherever it sits, attached to nothing —
    // there is no `DocBinding` and no declaration a block binds to.
    DocTarget { absent "perldoc reads POD wherever it sits; no declaration is its target" }
    DocRunOpener { absent "a POD block is documentation by its own kind, not by opening a run" }
    DocRunDirective { absent "perldoc reads no directive comment" }
    // perldoc extracts POD from a sub body too
    // (`DOC_COMMENTS_IN_FUNCTION_BODIES`), so no ancestor withholds
    // one: nothing to name here.
    HoldsDocInBody { absent "perldoc reads POD in any body; no kind withholds it" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PerlCode;
    use crate::test_support::for_each_node_with_chain;

    // The `EagerLogical` row owns an expression exactly when its
    // operator is `xor`, projects to that token, and pays at it — and
    // the chain operators beside it are the `ShortCircuit` row's alone
    // (#1695).
    #[cfg(feature = "perl")]
    #[test]
    fn eager_logical_owns_xor_and_nothing_beside_it() {
        // `xor` binds looser than `=`, so the owner is the assignment.
        let source = b"my $c = $a xor $b; my $d = $a or $b; my %h = (xor => 1);\n";
        let (mut owned, mut paid) = (Vec::new(), Vec::new());
        for_each_node_with_chain::<PerlCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let text = || node.utf8_text(source).expect("fixture is UTF-8").to_owned();
            if eager_logical::owns(node, source, ancestors) {
                let op = eager_logical::project(node).expect("an owner projects to its token");
                assert_eq!(op.utf8_text(source), Some("xor"), "{}", text());
                owned.push(text());
            }
            if eager_logical::pays_at_leaf(node, source, ancestors) {
                paid.push(text());
            }
            assert!(
                !(eager_logical::owns(node, source, ancestors)
                    && short_circuit::owns(node, source, ancestors)),
                "{}",
                text()
            );
        });
        assert_eq!(owned, ["my $c = $a xor $b"]);
        assert_eq!(paid, ["xor"]);
    }
}
