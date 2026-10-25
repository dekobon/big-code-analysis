//! Go's role rows. The Halstead keyword roles (#1693) and the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — are
//! declared; the rest are pending the package that declares each (#1692).

use crate::Go;
use crate::role_table;

role_table! {
    lang: Go;
    Call { owns [CallExpression] }
    // A `rune_literal` is not a string, though the dump flattens it
    // beside the two.
    StringLit { owns [InterpretedStringLiteral, RawStringLiteral] }
    // The one `binary_expression` kind carries every binary operator;
    // its `operator` field says which, so the owner is gated on the
    // field. `<` / `>` are gated to that parent — a type-parameter list
    // uses brackets, but error recovery strands either token — and the
    // other tokens occur nowhere else.
    Comparison {
        owns [BinaryExpression(holds_payer)]
        pays [
            EQEQ, BANGEQ, LTEQ, GTEQ,
            LT under BinaryExpression, GT under BinaryExpression,
        ]
        project "operator"
    }
    ShortCircuit {
        owns [BinaryExpression(holds_payer)]
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
    // A function and a method declaration are the callables; a `func`
    // literal is the closure, and opens a space of its own. Go has no
    // container space: a `struct` type is not a space, so its fields
    // land on the enclosing one (`npa/go.rs`), and an interface's
    // method set on its own (`npm/go.rs`).
    FuncOpener {
        owns [
            FunctionDeclaration(callable opens Function),
            MethodDeclaration(callable opens Function),
            FuncLiteral(opens Function),
        ]
    }
    Closure { owns [FuncLiteral] }
    Container { owns [SourceFile(opens Unit)] }
    // A struct's field declaration (`A, b int` is one declaration, two
    // names — the consumer counts the names, and reads the export rule
    // from the bytes).
    Member { owns [FieldDeclaration] }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Go documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
