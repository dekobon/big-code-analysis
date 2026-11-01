//! Go's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — and
//! the documentation rows (#1699) are declared.

use crate::lang_helpers::doc_comment::go_directive;
use crate::role_table;
use crate::{Ancestors, FoldLane, Go, Node};

/// Whether `node` lies outside every function body: its nearest
/// `source_file`, `statement_list` or `block` ancestor is the
/// `source_file`. go/doc never reads a declaration inside a function
/// body, nor a field of a type declared there. The nearest of a source
/// file and a statement list decides; folded down the chain the last
/// one met is that nearest, and the fold is memoised per ancestor on
/// the walk's cache, so nested struct types cost one step each rather
/// than a climb per field (#1597).
fn outside_function_body<'a>(node: &Node<'a>, _: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    // role-bypass: allow(Container) — the doc scope's top, not whether
    // the file opens a space
    ancestors.fold_down(
        node,
        0,
        FoldLane::DocScope,
        0,
        |outside, ancestor, _, _| match ancestor.kind_id().into() {
            Go::SourceFile => 1,
            Go::StatementList | Go::Block => 0,
            _ => outside,
        },
    ) == 1
}

/// Whether an interface's `type_elem` is one go/doc documents: an
/// embedded element opening with a name (`io.Reader`, `int | string`,
/// `G[int]`), the form go/parser reads as a method spec with a lead
/// comment; a `~int` constraint and other non-name-led elements carry
/// none. The grammar also parses each argument of an instantiation
/// (`Map[string, int]`) as a `type_elem`, which no lead comment
/// documents, so only an interface's own elements qualify — and only
/// outside a function body.
fn interface_element_opens_with_a_name<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent_has_kind(node, Go::InterfaceType as u16)
        && node.child(0).is_some_and(|first| {
            matches!(
                first.kind_id().into(),
                Go::TypeIdentifier | Go::QualifiedType | Go::GenericType
            )
        })
        && outside_function_body(node, code, ancestors)
}

// Whether a comment is a go/doc directive (`//go:generate`, `//line`,
// `//export`), read from the bytes: a member of a doc run that is not
// documentation text.
crate::roles::byte_gate!(is_go_directive, go_directive);

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
    // The one comment kind. go/doc marks no comment as documentation
    // by itself: a doc comment is one whose run sits directly above a
    // target (`DocTarget`), decided per run by the consumer that reads
    // the run (`lang_helpers::doc_comment::leading_doc_run`), so there is
    // no `DocComment` row — and no `DocRunOpener`, since no run documents
    // by its own content (#1699).
    Comment { owns [Comment] }
    DocComment { absent "go/doc reads a comment as documentation by its position above a declaration, never by a marker" }
    Docstring { absent "Go documents with comments, not string literals" }
    // go/doc documents the package clause, top-level declarations, the
    // specs of a grouped `type ( … )` / `var ( … )` / `const ( … )` — a
    // grouped alias (`B = int`) is a `type_alias`, not a `type_spec` —
    // struct fields and interface elements (methods and embedded types);
    // never an import. Everything but the package clause and the two
    // function kinds is gated outside a function body. Outside the
    // space-tree rows: the package clause, the four spec kinds, the
    // alias and the interface element — the parity exceptions
    // `roles/doc_tests/parity.rs` records.
    DocTarget {
        owns [
            PackageClause,
            FunctionDeclaration,
            MethodDeclaration,
            TypeDeclaration(gated_by outside_function_body),
            VarDeclaration(gated_by outside_function_body),
            ConstDeclaration(gated_by outside_function_body),
            TypeSpec(gated_by outside_function_body),
            TypeAlias(gated_by outside_function_body),
            VarSpec(gated_by outside_function_body),
            ConstSpec(gated_by outside_function_body),
            FieldDeclaration(gated_by outside_function_body),
            MethodElem(gated_by outside_function_body),
            TypeElem(gated_by interface_element_opens_with_a_name),
        ]
    }
    DocRunOpener { absent "a go/doc comment documents by its position above a declaration, never by its own content" }
    // A go/doc directive (`//go:generate`, `//line`, `//export`) is a
    // member of a doc run that is not documentation text.
    DocRunDirective { owns [Comment(text_gated_by is_go_directive)] }
    HoldsDocInBody { absent "go/doc reads no comment inside a function body; nothing beyond the function withholds one" }
}
