//! Tcl's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::Tcl;
use crate::role_table;
use crate::roles::tcl_family::{
    tcl_family_comparison_payers, tcl_family_gates, tcl_family_short_circuit_payers,
};

tcl_family_gates!();

role_table! {
    lang: Tcl;
    Call { pending R08 }
    StringLit { pending R08 }
    // A `binop_expr` names no operator field; its operator is one token
    // among its children, so each row's owner is gated on holding one
    // of the row's payers, and `project` — the first payer child — is
    // the lookup (`roles/tcl_family.rs` has the lists). A chained
    // `$a < $b && $c` is a left-nested pair of `binop_expr`s, each
    // owning its own operator.
    Comparison {
        owns [BinopExpr(gated_by comparison_operator)]
        pays [..tcl_family_comparison_payers!()]
        project "operator"
    }
    ShortCircuit {
        owns [BinopExpr(gated_by short_circuit_operator)]
        pays [..tcl_family_short_circuit_payers!()]
        project "operator"
    }
    EagerLogical { absent "Tcl's `&` / `|` inside `expr` are bitwise" }
    // A `ternary_expr` owns it and its `?` pays, ungated: the grammar
    // spells the bare token nowhere else. The node names no field, so
    // the row's payer fallback finds the `?`, and the ABC slot walker
    // reads the operands relative to the `?` / `:` markers (#1180).
    Ternary {
        owns [TernaryExpr]
        pays [QMARK]
        project "operator"
    }
    NullSafe { absent "Tcl has no optional navigation or null coalescing" }
    CaseArm { pending R05 }
    Guard { absent "Tcl has no guard clause" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Tcl has no modifier keywords" }
    // `proc` and `namespace` as keyword tokens (`Namespace2`; the
    // unsuffixed `Namespace` is the command node). `set` and `global`
    // declare a variable, as `local` does in Bash; `Set2` / `Global2`
    // are the keyword kinds.
    DeclKeyword { owns [Proc, Namespace2, Set2, Global2] }
    Primitive { absent "Tcl has no type names" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { absent "Tcl has no container to hold a member" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Tcl documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
