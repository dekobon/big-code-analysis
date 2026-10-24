//! C's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::C;
use crate::role_table;
use crate::roles::c_family::{
    c_family_comparison_owners, c_family_comparison_payers, c_family_decl_keywords, c_family_gates,
    c_family_modifiers, c_family_primitives, c_family_short_circuit_owners,
    c_family_short_circuit_payers,
};

c_family_gates!(C);

role_table! {
    lang: C;
    Call { pending R08 }
    StringLit { pending R08 }
    // The C binary operators are one `binary_expression` kind, emitted
    // under two production paths (`BinaryExpression`, `BinaryExpression2`)
    // and told apart by the `operator` field, so each row's owners are
    // gated on that field holding one of its payers
    // (`roles/c_family.rs` has the lists and their derivations). An
    // `#if A <= B` operand is a `preproc_binary_expression` aliased onto
    // the same node and pays alike.
    Comparison {
        owns [..c_family_comparison_owners!()]
        pays [..c_family_comparison_payers!()]
        project "operator"
    }
    ShortCircuit {
        owns [..c_family_short_circuit_owners!()]
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
    CaseArm { pending R05 }
    Guard { absent "C has no guard clause on a case or a function head" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // The shared C-family lists (`roles/c_family.rs`) plus `auto`,
    // which is a storage class in C — the only place this grammar emits
    // it — and a placeholder type in C++, where the row leaves it out.
    Modifier { owns [..c_family_modifiers!(), Auto] }
    DeclKeyword { owns [..c_family_decl_keywords!()] }
    Primitive { owns [..c_family_primitives!()] }
    FuncOpener { pending R06 }
    Closure { absent "C has no anonymous function" }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "C documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
