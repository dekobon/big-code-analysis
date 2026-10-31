//! Go's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — are declared; the rest are pending
//! the package that declares each (#1692).

use crate::role_table;
use crate::{Ancestors, Go, Node};

/// Whether an `if_statement` continues an `else if` chain: its parent is
/// the outer `if_statement`, read off the chain (#1084).
fn parent_is_if<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent_has_kind(node, Go::IfStatement as u16)
}

/// Whether a `call_expression` is the built-in `panic(…)`: its
/// `function` field is a bare `identifier` spelling `panic`. Invalid
/// UTF-8 in the callee reads as no match.
fn calls_panic<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    node.child_by_field_name("function")
        .is_some_and(|f| f.kind_id() == Go::Identifier as u16 && f.utf8_text(code) == Some("panic"))
}

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
    // Go counts statement nodes, so every row here is plain (#1697).
    // An `if` with an init statement (`if x := f(); x`) is the same kind.
    Branch { owns [IfStatement] }
    // The expression switch, the type switch, and `select`, whose
    // communication arms the `CaseArm` row owns.
    Switch { owns [ExpressionSwitchStatement, TypeSwitchStatement, SelectStatement] }
    // Every `for` spelling — three-clause, condition-only, range and
    // infinite — is one `for_statement`.
    Loop { owns [ForStatement] }
    Catch { absent "Go has no exception handling; `recover` is a call" }
    // Go spells `else if` as an `if_statement` whose parent is the outer
    // `if_statement` (the grammar has no else clause node); a nested `if`
    // inside `else { … }` has a `block` parent. Recognition only: the
    // `Else` token pays cognitive's +1 and the statement `Branch`'s.
    ElseIf { owns [IfStatement(gated_by parent_is_if)] pays [] }
    // `return`, and the built-in `panic(…)`: a `call_expression` whose
    // `function` field is the bare identifier `panic` — not a
    // `selector_expression`, so a package-qualified `foo.panic()` is a
    // user function and stays uncounted.
    Exit { owns [ReturnStatement, CallExpression(text_gated_by calls_panic)] }
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
