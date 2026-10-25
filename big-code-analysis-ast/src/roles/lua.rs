//! Lua's role rows. The Halstead keyword roles (#1693) and the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — are
//! declared; the rest are pending the package that declares each (#1692).

use crate::Lua;
use crate::role_table;

role_table! {
    lang: Lua;
    Call { owns [FunctionCall] }
    StringLit { owns [String] }
    // The one `binary_expression` kind carries every binary operator;
    // its `operator` field says which, so the owner is gated on the
    // field. `~=` is Lua's inequality. `<` / `>` are gated to that
    // parent for a token error recovery strands; the rest occur
    // nowhere else.
    Comparison {
        owns [BinaryExpression(holds_payer)]
        pays [
            EQEQ, TILDEEQ, LTEQ, GTEQ,
            LT under BinaryExpression, GT under BinaryExpression,
        ]
        project "operator"
    }
    // `and` / `or`, the word spellings, each a kind of its own that
    // occurs only as a `binary_expression`'s operator.
    ShortCircuit {
        owns [BinaryExpression(holds_payer)]
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
    // The three aliased `function_declaration` ids (`function f`,
    // `function M.f`, `local function f`) are the callables; an
    // anonymous `function () … end` is the closure and opens a space.
    FuncOpener {
        owns [
            FunctionDeclaration(callable opens Function),
            FunctionDeclaration2(callable opens Function),
            FunctionDeclaration3(callable opens Function),
            FunctionDefinition(opens Function),
        ]
    }
    Closure { owns [FunctionDefinition] }
    Container { owns [Chunk(opens Unit)] }
    Member { absent "Lua has no container to hold a member" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Lua documents with LDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
