//! Go's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::role_table;
use crate::{Ancestors, Go, Node};

/// Whether a `binary_expression`'s `operator` field is one of the
/// `Comparison` payers. The one kind carries every binary operator.
fn comparison_operator<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_some_and(|op| comparison::PAYERS.contains(&op.kind_id()))
}

/// Whether a `binary_expression`'s `operator` field is a short-circuit
/// payer.
fn short_circuit_operator<'a>(
    node: &Node<'a>,
    _code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    short_circuit::project(node).is_some_and(|op| short_circuit::PAYERS.contains(&op.kind_id()))
}

/// Whether a bare `<` / `>` is the operator of a `binary_expression`
/// rather than a token error recovery strands.
fn under_binary_expression<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent_has_kind(node, Go::BinaryExpression as u16)
}

role_table! {
    lang: Go;
    Call { pending R08 }
    StringLit { pending R08 }
    // The one `binary_expression` kind carries every binary operator;
    // its `operator` field says which, so the owner is gated on the
    // field. `<` / `>` are gated to that parent — a type-parameter list
    // uses brackets, but error recovery strands either token — and the
    // other tokens occur nowhere else.
    Comparison {
        owns [BinaryExpression(gated_by comparison_operator)]
        pays [
            EQEQ, BANGEQ, LTEQ, GTEQ,
            LT gated_by under_binary_expression, GT gated_by under_binary_expression,
        ]
        project "operator"
    }
    ShortCircuit {
        owns [BinaryExpression(gated_by short_circuit_operator)]
        pays [AMPAMP, PIPEPIPE]
        project "operator"
    }
    EagerLogical { absent "Go's `&` / `|` / `^` are bitwise" }
    Ternary { absent "Go has no ternary expression" }
    NullSafe { absent "Go has no optional navigation or null coalescing" }
    // An expression-switch, type-switch or `select` arm owns a decision
    // and pays at the node — Go's `case` keyword is one kind for all
    // three and bills as the keyword it is; the `default_case` and its
    // keyword are the fallback and pay nothing (lesson 11, #469).
    CaseArm {
        owns [ExpressionCase, TypeCase, CommunicationCase]
        excludes [
            DefaultCase "the fallback arm, no decision (lesson 11, #469)",
            Case "the arm's keyword, billed as such; the arm pays",
            Default "the fallback label's keyword",
        ]
    }
    Guard { absent "Go has no guard clause on a case or a function head" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { absent "Go has no exception handling; `recover` is a call" }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Go has no modifier keywords; visibility is the identifier's case" }
    // Each the leaf of an unlisted `*_declaration`; `package` and
    // `import` bill, as C# bills `using`. `map` and `chan` are type
    // constructors, not declarations, and bill with the operators.
    DeclKeyword { owns [Func, Type, Struct, Interface, Const, Var, Package, Import] }
    Primitive { absent "Go's built-in types (`int`, `string`) are ordinary `type_identifier`s, \
         operands like every other type name" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Go documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
