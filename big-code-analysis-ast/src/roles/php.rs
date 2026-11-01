//! Php's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — and
//! the documentation rows (#1699) are declared.

use crate::lang_helpers::doc_comment::phpdoc;
use crate::role_table;
use crate::{Ancestors, Node, Php};

// Whether a comment opens with the DocBlock marker PHP's scanner reads
// (`/**` followed by whitespace), from the bytes.
crate::roles::byte_gate!(opens_with_docblock_marker, phpdoc);

/// Whether an `if_statement` continues a two-word `else if`: its parent
/// is an `else_clause` (either alias), read off the chain (#1084).
fn continues_else_clause<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors
        .parent(node)
        .is_some_and(|parent| matches!(parent.kind_id().into(), Php::ElseClause | Php::ElseClause2))
}

role_table! {
    lang: Php;
    // Intentionally narrower than ABC's `branches` set: ABC additionally
    // counts `ObjectCreationExpression` (`new Foo()`) as a branch, but
    // `is_call` drives `bca ops` and should match the user's mental
    // model of "function / method call sites" (mirrors Java's
    // `MethodInvocation` while ABC counts `MethodInvocation | New`). The
    // nullsafe call is a call; the `NullSafe` row claims it for its `?->`.
    Call {
        owns [
            FunctionCallExpression,
            MemberCallExpression,
            ScopedCallExpression,
            NullsafeMemberCallExpression,
        ]
    }
    // `String` (368) is the named single-quoted literal. `String2` (25)
    // and `String3` (378) are aliased kind ids the enum also maps to
    // `"string"`, and they are opposites: `String2` is the `string`
    // *type* keyword — `bca dump` finds it only as the sole child of a
    // `primitive_type` wrapper, in a property type, a parameter type, a
    // `?string`, a `string|int` union and a return type — excluded so
    // `find string` / `count string` report literals, matching TS's
    // `String2` and TSX's `String3` (#1261; #288 listed it for kind-name
    // identity and parity with `get_op_type` and the dump, #1293 then
    // moved it into `get_op_type`'s `primitive_type`-suppression arm, so
    // that parity argues the other way — #1474; a `(string)` cast never
    // reaches either question, it is a childless `cast_type`). `String3`
    // is the hidden `_string` supertype covering any string literal,
    // kept as a defensive owner (lesson 34) and pinned unreachable by
    // `php_hidden_string_supertype_is_unreachable`. The interpolating
    // forms `wraps` a `variable_name` / `dynamic_variable_name` /
    // subscript / member access / `{` child directly (#184); a
    // `heredoc` holds them one level down under a `heredoc_body`, which
    // the operand arm descends on its own.
    StringLit {
        owns [String, String3, EncapsedString, Heredoc, Nowdoc, ShellCommandExpression]
        wraps [
            VariableName,
            DynamicVariableName,
            SubscriptExpression,
            SubscriptExpression2,
            SubscriptExpression3,
            MemberAccessExpression,
            MemberAccessExpression2,
            MemberAccessExpression3,
            LBRACE,
        ]
        excludes [String2 "the `string` type keyword, not a literal (#1261, #1474)"]
    }
    // The one `binary_expression` kind carries every binary operator,
    // `instanceof` included; its `operator` field says which, so the
    // owner is gated on the field. The ten comparison tokens are the
    // four equalities, `<>` (the second spelling of `!=`), the four
    // orderings and the spaceship `<=>`, plus the `instanceof`
    // keyword, every one ungated: PHP has no generic brackets, and
    // the pinned grammar spells each only as a `binary_expression`'s
    // operator, so a token error recovery strands under an `ERROR`
    // pays as it always did.
    Comparison {
        owns [BinaryExpression(holds_payer)]
        pays [EQEQ, EQEQEQ, BANGEQ, BANGEQEQ, LTGT, LTEQ, GTEQ, LTEQGT, Instanceof, LT, GT]
        project "operator"
    }
    // The punctuation forms `&&` / `||` and the low-precedence keyword
    // forms `and` / `or`, each a token of its own inside a
    // `binary_expression`. `xor` is `EagerLogical`'s: it evaluates both
    // operands (#1536). `??` is `NullSafe`'s.
    ShortCircuit {
        owns [BinaryExpression(holds_payer)]
        pays [AMPAMP, PIPEPIPE, And, Or]
        project "operator"
    }
    // `xor` evaluates both operands, so it is a value as `^` is, not a
    // chain: no Rule-9 chain increment, no cyclomatic decision, no
    // boolean sequence, and the slot holding one pays once under the
    // ordinary slot rule (#1536). The token is a kind of its own the
    // grammar spells only as the operator — `[xor => 1]` and `$o->xor`
    // are a `name` — and Halstead bills it as the operator it is, as
    // before the row.
    EagerLogical {
        owns [BinaryExpression(holds_payer)]
        pays [Xor]
        project "operator"
    }
    // The `conditional_expression` owns it and its `?` pays, gated to
    // that owner — `?int` spells the same token as type syntax. The
    // short form `$a ?: $b` is the same node with its `body` field
    // empty, and pays once as the long form does; `??` is `NullSafe`'s.
    Ternary {
        owns [ConditionalExpression]
        pays [QMARK under ConditionalExpression]
        project "operator"
    }
    // Two structural forms, each one decision per token (#436): the
    // null coalescing `a ?? b`, a `binary_expression` owned through its
    // `operator` field, and the nullsafe access `$o?->p` / call
    // `$o?->m()`, whose `?->` sits in the
    // `nullsafe_member_access_expression` (two aliased ids) or
    // `nullsafe_member_call_expression`. Cyclomatic pays each token
    // once per textual operator, across both nullsafe kinds and in a
    // chain; cognitive folds `??` into a boolean sequence (an operation
    // of its own, #1530) and reads `?->` not at all; ABC pays neither —
    // a nullsafe call is a branch as every call is. The compound `??=`
    // is an assignment to ABC, a sequence to cognitive and a decision to
    // cyclomatic (#236): the consumers' own, recorded here.
    NullSafe {
        owns [
            BinaryExpression(holds_payer),
            NullsafeMemberAccessExpression, NullsafeMemberAccessExpression2,
            NullsafeMemberCallExpression,
        ]
        pays [QMARKQMARK, QMARKDASHGT]
        project "operator"
        excludes [QMARKQMARKEQ "the compound `??=`, an assignment ABC pays as one and a decision the others pay as their own (#236)"]
    }
    // A `case_statement` of a `switch` and a `match_conditional_expression`
    // of a `match` each own a decision and pay at the node (the `case`
    // keyword bills as the keyword it is; a match arm has none); the
    // `default_statement` / `match_default_expression` and their
    // keyword are the fallback and pay nothing (lesson 11, #469, #473).
    CaseArm {
        owns [CaseStatement, MatchConditionalExpression]
        excludes [
            DefaultStatement "the fallback arm, no decision (lesson 11, #469)",
            MatchDefaultExpression "the fallback arm of a `match`, no decision (#473)",
            Case "the arm's keyword, billed as such; the arm pays",
            Default "the fallback label's keyword",
        ]
    }
    Guard { absent "PHP has no guard clause" }
    // PHP counts statement nodes, so every row here is plain (#1697).
    Branch { owns [IfStatement] }
    // `switch`, and the PHP 8 `match` expression.
    Switch { owns [SwitchStatement, MatchExpression] }
    Loop { owns [ForStatement, ForeachStatement, WhileStatement, DoStatement] }
    Catch { owns [CatchClause] }
    // PHP has both spellings (#529): the one-word `elseif` is a dedicated
    // clause node (two aliases, the brace and the `:` … `endif` forms),
    // which *is* the else-if and pays for itself; the two-word `else if`
    // is an `if_statement` directly inside an `else_clause` (either
    // alias), recognised so cognitive opens no level for it and paying
    // nothing — its enclosing clause pays the +1.
    ElseIf {
        owns [
            ElseIfClause(pays_itself),
            ElseIfClause2(pays_itself),
            IfStatement(gated_by continues_else_clause),
        ]
        pays []
    }
    // `return`, a generator's `yield`, `throw` (an expression since PHP
    // 8), and `exit` / `die` — tree-sitter-php's `exit_statement` covers
    // `exit` with or without parentheses; `die(…)` parses as a
    // `function_call_expression` and is deliberately not matched by
    // callee text, which would false-match a user `die`.
    Exit { owns [ReturnStatement, YieldExpression, ThrowExpression, ExitStatement] }
    // Each modifier is its keyword leaf, one kind apiece, under an
    // unlisted `*_modifier` wrapper or bare in its declaration. `var`
    // is the `var_modifier` node, the grammar's one spelling of the
    // PHP 4 property keyword. `static` and `const` are one kind each
    // for the modifier and the statement (`static $x`, `const K = 1`),
    // and `global` for the statement alone; all bill once per
    // spelling.
    Modifier {
        owns [Public, Protected, Private, Static, Final, Abstract, Readonly, VarModifier, Const, Global]
    }
    // The declaration keywords, each its leaf under an unlisted
    // `*_declaration`; `fn` opens an arrow function and `declare` a
    // `declare(strict_types=1)` block.
    DeclKeyword { owns [Function, Fn, Class, Interface, Trait, Enum, Namespace, Use, Declare] }
    // PHP bills a primitive as an *operand* — the type name is what a
    // declaration acts on, keyed by its text — and `getter/php.rs`
    // says so where it reads this row. `primitive_type` is the node
    // that carries it: the wrapper is childless for `callable`,
    // `iterable`, `mixed`, `void`, `false` and `true` (the grammar
    // emits no token node for those spellings), so the keyword leaves
    // it does hold (`int`, `bool`, `array`, `object`, `string`,
    // `float`, `null`) are the duplicate, suppressed by the getter
    // under this wrapper (#1293). `bottom_type` (`never`) and
    // `cast_type` (`(int)`) are leaves of their own and bill with it;
    // a cast never emits a keyword node (#1484).
    Primitive { owns [PrimitiveType, BottomType, CastType] }
    // A function and a method are the callables; a `function () {}` and
    // an `fn () =>` are the closures, each opening a space. A trait is
    // a class-like mixin whose methods roll into the consuming class's
    // WMC, so it opens `Class` (load-bearing: `wmc.rs`'s PHP impl
    // matches no `Trait`); an anonymous class opens one like a named
    // one; an enum is class-like.
    FuncOpener {
        owns [
            FunctionDefinition(callable opens Function),
            MethodDeclaration(callable opens Function),
            AnonymousFunction(opens Function),
            ArrowFunction(opens Function),
        ]
    }
    Closure { owns [AnonymousFunction, ArrowFunction] }
    Container {
        owns [
            Program(opens Unit),
            ClassDeclaration(opens Class),
            AnonymousClass(opens Class),
            EnumDeclaration(opens Class),
            TraitDeclaration(opens Class),
            InterfaceDeclaration(opens Interface),
        ]
    }
    // A property declaration (`public $a, $b;` — one declaration, two
    // `property_element`s the consumer counts) and a class constant in
    // its two aliased ids, which only an interface counts as a member
    // (`npa/php.rs`); an enum case is a tag, not a member.
    Member { owns [PropertyDeclaration, ConstDeclaration, ConstDeclaration2] }
    // The one comment kind, a `DocComment` owner under the DocBlock
    // marker gate (#1699).
    Comment { owns [Comment] }
    DocComment { owns [Comment(text_gated_by opens_with_docblock_marker)] }
    Docstring { absent "PHP documents with PHPDoc comments, not string literals" }
    // The structural elements phpDocumentor's reflection builds: a
    // class, interface, trait or enum, a function or method, a
    // property, a class or global constant and an enum case. `define()`
    // is a call the grammar parses as a statement, so a DocBlock above
    // it stays a plain comment, where phpDocumentor reads it as a
    // constant's; a `use`, a `namespace` or any other statement takes
    // none. Outside the space-tree rows: the enum case (a tag, not a
    // member) — the parity exception `roles/doc_tests/parity.rs` records.
    DocTarget {
        owns [
            ClassDeclaration,
            InterfaceDeclaration,
            TraitDeclaration,
            EnumDeclaration,
            FunctionDefinition,
            MethodDeclaration,
            PropertyDeclaration,
            ConstDeclaration,
            ConstDeclaration2,
            EnumCase,
        ]
    }
    DocRunOpener { absent "phpDocumentor attaches every DocBlock to an element or the file; none documents by its own content" }
    DocRunDirective { absent "phpDocumentor reads no directive comment" }
    // Nothing beyond the callables and the closures: which child is the
    // body — a `compound_statement`, or an arrow function's expression —
    // is the Checker's body-child test, not a kind.
    HoldsDocInBody { absent "only the callables and closures hold a doc comment in a body; the body-child test is the Checker's" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_each_node_with_chain;
    use crate::{Ancestors, PhpCode};

    // The `EagerLogical` row owns an expression exactly when its
    // operator is `xor`, projects to that token, and pays at it — and
    // the chain operators beside it are the `ShortCircuit` row's alone
    // (#1695).
    #[cfg(feature = "php")]
    #[test]
    fn eager_logical_owns_xor_and_nothing_beside_it() {
        // `xor` binds looser than `=`, so the owner is the assignment.
        let source = b"<?php $c = $a xor $b; $d = $a or $b; $e = [xor => 1]; $o->xor;\n";
        let (mut owned, mut paid) = (Vec::new(), Vec::new());
        for_each_node_with_chain::<PhpCode>(source, |node, chain| {
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
        assert_eq!(owned, ["$c = $a xor $b"]);
        assert_eq!(paid, ["xor"]);
    }
}
