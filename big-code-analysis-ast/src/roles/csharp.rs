//! Csharp's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::checker::csharp_is_aliased_modifier;
use crate::getter::node_text;
use crate::lang_helpers::case_arms::{
    csharp_case_token_is_switch_arm, csharp_switch_expression_arm_is_bare_discard,
};
use crate::role_table;
use crate::{Ancestors, Csharp, Node};

/// Whether a childless `modifier` node — the grammar's alias for a
/// bare keyword in a parameter (`ref` / `out` / `in` / `scoped` /
/// `readonly`, #1418) or on a lambda (`static` / `async`, #1482) —
/// spells one of those keywords. The aliased `this` of an extension
/// receiver (`M(this Foo f)`) is not one: it is the operand `this` is
/// everywhere, and `CsharpCode::get_op_type_with_code` bills it so
/// *before* reading this row, which is why adding `"this"` to the list
/// below changes no output — measured under #1693; the ordering, not
/// the list, is what keeps it an operand. A `modifier` *with* a child
/// wraps a keyword leaf the row already owns and is nothing itself. A
/// grammar bump that aliases another spelling lands here unclaimed
/// rather than billed as whichever word happened to be last.
fn aliases_a_modifier_keyword<'a>(
    node: &Node<'a>,
    code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    csharp_is_aliased_modifier(node)
        && matches!(
            node_text(code, node),
            Some("ref" | "out" | "in" | "scoped" | "readonly" | "static" | "async")
        )
}

/// Whether a comparison token applies its operator: its parent is a
/// `binary_expression` (either alias). A `grammar.json` sweep of
/// tree-sitter-c-sharp 0.23.5 finds each token in these productions,
/// and only `binary_expression` (`a < b`) qualifies:
///
/// - `<` / `>` — `binary_expression`, `relational_pattern`,
///   `operator_declaration`, `type_argument_list`,
///   `type_parameter_list`, `function_pointer_type`
/// - `<=` / `>=` — `binary_expression`, `relational_pattern`,
///   `operator_declaration`
/// - `==` / `!=` — `binary_expression`, `preproc_binary_expression`
///   (`#if A == B`, parsed as `BinaryExpression` at this pin),
///   `operator_declaration`
///
/// Three reasons to exclude, one per family. The generic brackets and
/// `delegate*<int, int>` are type syntax, no more a decision than the
/// `?` of a nullable type (#1275). An `operator_declaration` names the
/// operator being *defined* rather than applying it; #1297 found this
/// for `<` / `>`, and the other four still scored 1 against their 0 on
/// a class overloading all six, every member of which has
/// `cyclomatic()` 1 (#1420). A `relational_pattern`'s operator is not
/// its own decision: the enclosing `switch_expression_arm` or `if`
/// slot already scores one, so counting it charged `x switch { > 5 =>
/// … }` twice what the constant arm scores (#1383). Allowlist
/// polarity, so a grammar bump that grows a seventh production fails
/// closed, and a token error recovery reparents under `{ERROR}` stops
/// counting — valid input is unaffected and no C# corpus file parses
/// with an `{ERROR}` node, measured.
fn under_binary_expression<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent(node).is_some_and(|p| {
        p.kind_id() == Csharp::BinaryExpression as u16
            || p.kind_id() == Csharp::BinaryExpression2 as u16
    })
}

/// Whether a `switch_section` holds a `case` label rather than only
/// `default:`. One section carries every label of a fall-through run
/// (`case 1: case 2: body`), each a decision its `case` keyword pays.
fn is_case_section<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    node.is_child(Csharp::Case as u16)
}

/// Whether a `switch_expression_arm` is a decision: not the bare
/// discard `_ =>` / `var _ =>`, the expression form's `default:`,
/// unless a `when` guard makes it one (`case_arms` has the shapes).
fn is_decision_arm<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    !csharp_switch_expression_arm_is_bare_discard(node)
}

/// Whether a `case` keyword is a `switch` arm's rather than the one in
/// `goto case 2;`, an unconditional jump that spells the same token
/// (#1450).
fn is_switch_arm_case<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    csharp_case_token_is_switch_arm(node, ancestors)
}

/// Whether a `binary_expression`'s `operator` field is `??`.
fn null_coalescing_operator<'a>(
    node: &Node<'a>,
    _code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    null_safe::project(node).is_some_and(|op| op.kind_id() == Csharp::QMARKQMARK as u16)
}

/// Whether a `?` is the ternary's. tree-sitter-c-sharp spells the bare
/// token in four productions: `conditional_expression`,
/// `conditional_access_expression` (`a?.b` and `a?[0]`, the `NullSafe`
/// row's), `nullable_type` (`int? x`) and `type_parameter_constraint`
/// (`where T : class?`), the last two type syntax (#1275).
fn under_ternary<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent_has_kind(node, Csharp::ConditionalExpression as u16)
}

/// Whether an `is` keyword sits under one of its two type-test
/// expressions. The grammar spells it in no other production, but
/// error recovery keeps a keyword whose expression it lost (`var x = o
/// is` at end of input) as a bare child of an `ERROR`, where no type
/// test exists for it to pay for.
fn under_type_test<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent(node).is_some_and(|p| {
        p.kind_id() == Csharp::IsExpression as u16
            || p.kind_id() == Csharp::IsPatternExpression as u16
    })
}

/// Whether a `binary_expression`'s `operator` field is one of the
/// `Comparison` payers.
fn comparison_operator<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_some_and(|op| comparison::PAYERS.contains(&op.kind_id()))
}

/// Whether a `binary_expression`'s `operator` field is `&&` or `||`.
fn short_circuit_operator<'a>(
    node: &Node<'a>,
    _code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    short_circuit::project(node).is_some_and(|op| short_circuit::PAYERS.contains(&op.kind_id()))
}

role_table! {
    lang: Csharp;
    Call { pending R08 }
    StringLit { pending R08 }
    // A `binary_expression` (both aliases: `BinaryExpression2` is the
    // second id carrying the kind string, listed per grammar-dispatch
    // §1 and unreachable at this pin, where `#if A == B` parses as the
    // first) owns a comparison through its `operator` field, and every
    // comparison token is gated to that parent. The type tests
    // `is_expression` (`o is T`) and `is_pattern_expression` (`o is T
    // t`, `o is not null`) are disjoint alternatives that each wrap one
    // `is` keyword, which pays for them, gated to that owner: the
    // grammar spells `is` nowhere else, but a keyword error recovery
    // strands under an `ERROR` pays for no test. `??` is `NullSafe`'s
    // and the `and` / `or` / `not` pattern combinators are no
    // comparison.
    Comparison {
        owns [
            BinaryExpression(gated_by comparison_operator),
            BinaryExpression2(gated_by comparison_operator),
            IsExpression, IsPatternExpression,
        ]
        pays [
            EQEQ gated_by under_binary_expression, BANGEQ gated_by under_binary_expression,
            LTEQ gated_by under_binary_expression, GTEQ gated_by under_binary_expression,
            LT gated_by under_binary_expression, GT gated_by under_binary_expression,
            Is gated_by under_type_test,
        ]
        project "operator"
    }
    ShortCircuit {
        owns [
            BinaryExpression(gated_by short_circuit_operator),
            BinaryExpression2(gated_by short_circuit_operator),
        ]
        pays [AMPAMP, PIPEPIPE]
        project "operator"
    }
    // `&` / `|` / `^` on boolean operands evaluate both sides, but the
    // grammar emits them as the same `binary_expression` as the integral
    // forms, so the syntax alone cannot tell them apart; no consumer
    // treats them as a decision, and the three tokens bill as the
    // operators they are. A recognition boundary, recorded as the
    // kinds considered rather than as an absence.
    EagerLogical {
        owns []
        excludes [
            AMP "a boolean `&` is the integral `&`'s kind, which the grammar does not tell apart",
            PIPE "a boolean `|` is the integral `|`'s kind, which the grammar does not tell apart",
            CARET "a boolean `^` is the integral `^`'s kind, which the grammar does not tell apart",
        ]
    }
    // The `conditional_expression` owns it and its `?` pays, gated to
    // that owner; the same bare token is a conditional access's (the
    // `NullSafe` row's) and type syntax in two productions.
    Ternary {
        owns [ConditionalExpression]
        pays [QMARK gated_by under_ternary]
        project "operator"
    }
    // Two structural forms, each one decision (#436, #1459): the null
    // coalescing `a ?? b`, a `binary_expression` (either alias) owned
    // through its `operator` field and paid at the `??` token; and the
    // conditional access `a?.b` / `a?[0]`, a
    // `conditional_access_expression` that pays for itself — its
    // operator is the same bare `?` the ternary and two type
    // productions spell (#1275), and one node is one `?`, so the node is
    // the payer and the token bills through the byte-less arm as it
    // did. Which consumer reads which: cyclomatic pays `??` at the token
    // and the access at the node; cognitive folds `??` into a boolean
    // sequence; ABC pays both as conditions, which keeps ABC level with
    // C#'s own cyclomatic on a safe-navigation chain — a C#-internal
    // agreement no sibling shares (`a?.b?.c` is 0 ABC conditions in the
    // JS family, Kotlin and Groovy). The compound `??=` is an assignment
    // to ABC, a sequence to cognitive and a decision to cyclomatic
    // (#236): the consumers' own, recorded here.
    NullSafe {
        owns [
            BinaryExpression(gated_by null_coalescing_operator),
            BinaryExpression2(gated_by null_coalescing_operator),
            ConditionalAccessExpression(pays_itself),
        ]
        pays [QMARKQMARK]
        project "operator"
        excludes [QMARKQMARKEQ "the compound `??=`, an assignment ABC pays as one and a decision the others pay as their own (#236)"]
    }
    // Two arm shapes. A statement `switch_section` owns a decision when
    // it holds a `case` label, and each `case` keyword pays — gated to
    // the section, since `goto case 2;` spells the same token (#1450);
    // `default:` is the fallback and pays nothing (lesson 11, #469). A
    // `switch_expression_arm` has no keyword: the node owns and pays,
    // unless it is the bare discard `_ =>` / `var _ =>`, the expression
    // form's `default:` — a guarded discard still counts.
    CaseArm {
        owns [
            SwitchSection(gated_by is_case_section),
            SwitchExpressionArm(pays_itself gated_by is_decision_arm),
        ]
        pays [Case gated_by is_switch_arm_case]
        excludes [Default "the fallback label, no decision (lesson 11, #469); `goto default;` spells it too"]
    }
    // A `when` guard on a switch-expression arm or a pattern `case`
    // (`when_clause`) and on a `catch` (`catch_filter_clause`): each
    // node is one decision, paid at the node (#1422). The `when` token
    // is not the payer: it is also a `_reserved_identifier` at this
    // pin, so `int when = 1;` emits it under an `identifier` and a
    // token payer would score a decision per mention of the variable
    // (`bca dump`, not inferred); it bills as the keyword it is through
    // the byte-less arm.
    Guard {
        owns [WhenClause, CatchFilterClause]
        excludes [When "also a reserved identifier at this pin: `int when = 1;` spells it"]
    }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // Every modifier keyword leaf, plus the childless `modifier` alias
    // the grammar emits for a bare keyword in a parameter or on a
    // lambda, under its text gate. `ref` / `out` / `in` / `scoped` are
    // listed here as the aliased spelling only: as their own kinds they
    // are the expression keywords of `ref x` / `out var y` / `in` / a
    // `scoped` local, and bill with `await` and `is`. `new` is the
    // hiding modifier and the allocation keyword in one kind, billed
    // with the expression keywords for the same reason. The `modifier`
    // rule with a child wraps a leaf this row owns and is nothing
    // itself (grammar-dispatch section 5).
    Modifier {
        owns [
            Public, Private, Protected, Internal, Static, Abstract, Virtual, Override, Sealed,
            Partial, Readonly, Const, Extern, Unsafe, Volatile, Async, Required, File, Fixed,
            Implicit, Explicit, Modifier(text_gated_by aliases_a_modifier_keyword),
        ]
    }
    // The declaration and namespace keywords, each its leaf under an
    // unclassified `*_declaration` (#1552). `using` bills, and so does
    // the `static` of `using static` through the row above: C# bills
    // its directive keywords where Java and Groovy bill none of theirs
    // (#1600).
    DeclKeyword { owns [Class, Struct, Interface, Enum, Record, Delegate, Namespace, Using] }
    // `predefined_type` is one kind for every spelling (`int`, `string`,
    // `bool`, …), so `Checker::is_primitive` keys it by text (#286).
    // `var` is excluded: it names no type, and the `implicit_type`
    // wrapping it bills nothing.
    Primitive {
        owns [PredefinedType]
        excludes [Var "`var` names no type; its `implicit_type` wrapper bills nothing"]
    }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "C# documents with `///` comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
