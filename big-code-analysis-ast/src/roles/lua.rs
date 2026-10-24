//! Lua's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::role_table;
use crate::{Ancestors, Lua, Node};

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
    ancestors.parent_has_kind(node, Lua::BinaryExpression as u16)
}

role_table! {
    lang: Lua;
    Call { pending R08 }
    StringLit { pending R08 }
    // The one `binary_expression` kind carries every binary operator;
    // its `operator` field says which, so the owner is gated on the
    // field. `~=` is Lua's inequality. `<` / `>` are gated to that
    // parent for a token error recovery strands; the rest occur
    // nowhere else.
    Comparison {
        owns [BinaryExpression(gated_by comparison_operator)]
        pays [
            EQEQ, TILDEEQ, LTEQ, GTEQ,
            LT gated_by under_binary_expression, GT gated_by under_binary_expression,
        ]
        project "operator"
    }
    // `and` / `or`, the word spellings, each a kind of its own that
    // occurs only as a `binary_expression`'s operator.
    ShortCircuit {
        owns [BinaryExpression(gated_by short_circuit_operator)]
        pays [And, Or]
        project "operator"
    }
    EagerLogical { absent "Lua's `and` / `or` are lazy; it has no eager boolean operator" }
    Ternary { absent "Lua has no ternary expression; `a and b or c` is a chain" }
    NullSafe { absent "Lua has no optional navigation or null coalescing" }
    CaseArm { absent "Lua has no switch-like construct" }
    Guard { absent "Lua has no guard clause" }
    Branch { pending R07 }
    Switch { absent "Lua has no switch-like construct" }
    Loop { pending R07 }
    Catch { absent "Lua has no exception handling; `pcall` is a call" }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Lua has no modifier keywords; `local` is a declaration" }
    // `local` and `function`, each a leaf under an unlisted
    // `function_declaration` / `variable_declaration` / `function_definition`.
    DeclKeyword { owns [Local, Function] }
    Primitive { absent "Lua has no type names" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { absent "Lua has no container to hold a member" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Lua documents with LDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
