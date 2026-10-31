//! Lua's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — are declared; the rest are pending
//! the package that declares each (#1692).

use crate::role_table;
use crate::{Ancestors, Lua, Node};

/// Whether a `function_call` is `error(…)` or `os.exit(…)`, by the text
/// of its `name` field. Invalid UTF-8 reads as no match.
fn error_or_os_exit<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    node.child_by_field_name("name")
        .is_some_and(|name| matches!(name.utf8_text(code), Some("error" | "os.exit")))
}

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
    // Lua counts statement nodes, so every row here is plain (#1697).
    Branch { owns [IfStatement] }
    Switch { absent "Lua has no switch-like construct" }
    // The numeric and generic `for` are one `for_statement`; `repeat …
    // until` is a loop with its test at the end.
    Loop { owns [ForStatement, WhileStatement, RepeatStatement] }
    Catch { absent "Lua has no exception handling; `pcall` is a call" }
    // The grammar gives `elseif` a statement node of its own, a direct
    // child of the `if_statement`, so the clause *is* the else-if and pays
    // for itself; no nested `if` is ever an else-if here.
    ElseIf { owns [ElseifStatement(pays_itself)] pays [] }
    // `return`, and the abrupt-exit builtins `error(…)` (unwinds to the
    // nearest `pcall`) and `os.exit(…)`, each a `function_call` whose
    // `name` field spells the callee — `error` a bare identifier,
    // `os.exit` a `dot_index_expression` — told by text, so a user `foo()`
    // or `myError()` is not counted.
    Exit { owns [ReturnStatement, FunctionCall(text_gated_by error_or_os_exit)] }
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
