//! C's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — are declared; the rest are pending
//! the package that declares each (#1692).

use crate::C;
use crate::role_table;
use crate::roles::c_family::{
    c_family_binary_owners, c_family_calls, c_family_comparison_payers, c_family_decl_keywords,
    c_family_loops, c_family_modifiers, c_family_primitives, c_family_short_circuit_payers,
    c_family_strings,
};
use crate::roles::control_flow::else_if_parent_clause_gate;

// The chained `if` sits directly inside an `else_clause`; a nested one
// under `else { … }` sits inside the compound statement (#1697).
else_if_parent_clause_gate!(C, ElseClause);

role_table! {
    lang: C;
    Call { owns [..c_family_calls!()] }
    // C has no raw string literal.
    StringLit { owns [..c_family_strings!()] }
    // The C binary operators are one `binary_expression` kind, emitted
    // under two production paths (`BinaryExpression`, `BinaryExpression2`)
    // and told apart by the `operator` field, so each row's owners are
    // gated on that field holding one of its payers
    // (`roles/c_family.rs` has the lists and their derivations). An
    // `#if A <= B` operand is a `preproc_binary_expression` aliased onto
    // the same node and pays alike.
    Comparison {
        owns [..c_family_binary_owners!()]
        pays [..c_family_comparison_payers!()]
        project "operator"
    }
    ShortCircuit {
        owns [..c_family_binary_owners!()]
        pays [..c_family_short_circuit_payers!()]
        project "operator"
    }
    EagerLogical { absent "C's `&` / `|` / `^` are bitwise; the language has no eager boolean \
         operator" }
    // The `conditional_expression` owns the ternary and its `?` pays,
    // ungated: the grammar spells a bare `?` in no other production. The
    // GNU elision `a ?: b` is the same node with its `consequence` field
    // empty, which the ABC slot walker reads by field (#1102).
    Ternary {
        owns [ConditionalExpression]
        pays [QMARK]
        project "operator"
    }
    NullSafe { absent "C has no optional navigation or null coalescing" }
    // A `case_statement` is the grammar's node for a `case X:` label
    // and a `default:` label alike; the arm owns a decision when it
    // holds the `case` keyword, which pays — one per label, so a
    // fall-through run `case 2: case 3:` is two. The `default` keyword
    // is the fallback and pays nothing (lesson 11, #469); Halstead
    // bills it as the keyword it is through the byte-less arm.
    CaseArm {
        owns [CaseStatement(holds_payer)]
        pays [Case]
        excludes [Default "the fallback label, no decision (lesson 11, #469)"]
    }
    Guard { absent "C has no guard clause on a case or a function head" }
    // The C family's control flow (#1697): the one-kind rows are spelled
    // inline and the loop list is the family's fragment
    // (`roles/c_family.rs`); the `ElseIf` gate is emitted beside this
    // table. C's own shape is what it lacks: no exceptions, so `Catch`
    // is absent and `return` is the whole exit set.
    Branch { owns [IfStatement] pays [If] }
    Switch { owns [SwitchStatement] }
    Loop { owns [..c_family_loops!()] pays [For, While] }
    Catch { absent "C has no exception handling; `setjmp` / `longjmp` are calls" }
    ElseIf { owns [IfStatement(gated_by continues_else_clause)] pays [] }
    Exit { owns [ReturnStatement] }
    // The shared C-family lists (`roles/c_family.rs`) plus `auto`,
    // which is a storage class in C — the only place this grammar emits
    // it — and a placeholder type in C++, where the row leaves it out.
    Modifier { owns [..c_family_modifiers!(), Auto] }
    DeclKeyword { owns [..c_family_decl_keywords!()] }
    Primitive { owns [..c_family_primitives!()] }
    // Both aliased `function_definition` ids (#285, lesson 2). C has no
    // container space: a `struct` is not a space.
    FuncOpener {
        owns [
            FunctionDefinition(callable opens Function),
            FunctionDefinition2(callable opens Function),
        ]
    }
    Container { owns [TranslationUnit(opens Unit)] }
    Member { absent "C has no container space to hold a member; a struct is not one" }
    Closure { absent "C has no anonymous function" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "C documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
