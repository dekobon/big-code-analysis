//! Lua's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — and
//! the documentation rows (#1699) are declared.

use crate::lang_helpers::doc_comment::{
    documents_where_it_sits, ldoc_opener, lua_group_head_above, node_bytes,
};
use crate::role_table;
use crate::{Ancestors, Lua, LuaCode, Node};

/// Whether an assignment names one plain or dotted target (`x = 1`,
/// `M.x = 1`): LDoc's `get_fun_name` stops at `,`, `<` and `[`, so
/// `local a, b = …`, `local x <const> = …` and `M[1] = …` name no item.
fn assigns_one_name<'a>(node: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
    // The grammar always emits the `variable_list` first, empty when
    // recovery had to invent the target (`= 1` on its own).
    node.child(0).is_some_and(|targets| {
        let mut names = targets.children().filter(Node::is_named);
        names.next().is_some_and(|name| is_dotted_name(&name)) && names.next().is_none()
    })
}

/// An identifier, or a `.` index of one (`M.a.b`).
fn is_dotted_name(node: &Node) -> bool {
    node.preorder().filter(Node::is_named).all(|part| {
        matches!(
            part.kind_id().into(),
            Lua::Identifier | Lua::DotIndexExpression | Lua::DotIndexExpression2
        )
    })
}

/// Whether a `local` declaration assigns one name (`local x = 1`, not
/// a bare `local x`).
fn declares_one_name<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    node.children().any(|child| {
        matches!(
            child.kind_id().into(),
            Lua::AssignmentStatement | Lua::AssignmentStatement2
        ) && assigns_one_name(&child, code, ancestors)
    })
}

/// Whether a table field has a name (`new = function … end`): the
/// bracket form starts with `[` and a positional field has no `=`.
fn field_has_a_name<'a>(node: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
    node.child(0)
        .is_some_and(|name| name.kind_id() == Lua::Identifier)
        && node.child(1).is_some_and(|eq| eq.kind_id() == Lua::EQ)
}

/// Whether a `return` returns a function or a table — the module
/// shapes LDoc documents as an item, not an exit.
fn returns_a_function_or_table<'a>(node: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
    node.children()
        .find(|child| {
            matches!(
                child.kind_id().into(),
                Lua::ExpressionList | Lua::ExpressionList2
            )
        })
        .and_then(|list| list.children().find(Node::is_named))
        // role-bypass: allow(Closure) — what a returned value documents
        // as, not whether it is a closure
        .is_some_and(|value| {
            matches!(
                value.kind_id().into(),
                Lua::FunctionDefinition | Lua::TableConstructor
            )
        })
}

/// Whether a comment opens an LDoc block where LDoc reads one: it opens
/// `---`, it does not sit in a function body (LDoc documents what a
/// module declares; a `---` block in a body — LuaLS's `---@type` on a
/// local, an LDoc `@fixme` — annotates the code there), and it is not
/// the continuation of a `--` group that a trailing comment on the row
/// above opened. Whether the block then *documents* — has text, and an
/// item, a tag or the file's first position — is the content decision
/// `Checker::is_doc_run_opener` makes over the run, which needs the
/// sibling after it.
fn opens_an_ldoc_block_where_ldoc_reads<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ldoc_opener(node_bytes(node, code))
        && documents_where_it_sits::<LuaCode>(node, code, ancestors)
        && lua_group_head_above::<LuaCode>(node, code, ancestors)
            .is_none_or(|head| ldoc_opener(node_bytes(&head, code)))
}

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
    // The one comment kind. LDoc marks a *block* by its opener (`---`)
    // and continues it over the `--` rows below: documentation is a
    // property of the run, decided once at its opener (`DocRunOpener`),
    // so no single comment is a `DocComment` by itself (#1699).
    Comment { owns [Comment] }
    DocComment { absent "an LDoc block is documentation as a run opened by `---`, never one comment by a marker" }
    Docstring { absent "Lua documents with LDoc comments, not string literals" }
    // What LDoc's `item_follows` (`lang.lua`) documents under a block: a
    // `function` statement, local or not (three aliased ids); an
    // assignment to one plain or dotted name, local or not; a table
    // field with a name; a `return` of a function or a table. Read by
    // the content decision in `Checker::is_doc_run_opener` of the
    // statement after a block — *not* by `Checker::is_doc_target`,
    // which stays the trait default: LDoc attaches nothing to a
    // declaration a plain `--` run precedes, and forwarding this row
    // there would let one document. Every target here is a positional
    // item, none a space-tree member — the parity exceptions
    // `roles/doc_tests/parity.rs` records.
    DocTarget {
        owns [
            FunctionDeclaration,
            FunctionDeclaration2,
            FunctionDeclaration3,
            AssignmentStatement(gated_by assigns_one_name),
            AssignmentStatement2(gated_by assigns_one_name),
            VariableDeclaration(gated_by declares_one_name),
            VariableDeclaration2(gated_by declares_one_name),
            Field(gated_by field_has_a_name),
            ReturnStatement(gated_by returns_a_function_or_table),
        ]
    }
    // The opener candidate: a `---` comment where LDoc reads one. The
    // Checker decides the rest of the run's content.
    DocRunOpener { owns [Comment(text_gated_by opens_an_ldoc_block_where_ldoc_reads)] }
    DocRunDirective { absent "LDoc reads no directive comment" }
    HoldsDocInBody { absent "only the functions hold an LDoc block in a body; the trait default" }
}
