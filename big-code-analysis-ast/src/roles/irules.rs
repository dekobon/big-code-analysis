//! iRules' role rows; declared through the Tcl template once R06 lands
//! (contract 5 of #1690). The Halstead keyword role is declared (#1693).

use crate::Irules;
use crate::role_table;
use crate::roles::tcl_family::{
    tcl_family_comparison_payers, tcl_family_gates, tcl_family_short_circuit_payers,
};

tcl_family_gates!();

role_table! {
    lang: Irules;
    Call { pending R08 }
    StringLit { pending R08 }
    // Tcl's rows (`roles/tcl_family.rs`) plus iRules' own operators.
    // The comparison row adds the string and glob comparators
    // `starts_with`, `ends_with`, `contains`, `equals`, `matches`,
    // `matches_regex` and `matches_glob`; the short-circuit row adds the
    // word spellings `and` / `or`. Each is a kind of its own that the
    // grammar emits only inside a `binop_expr`.
    Comparison {
        owns [BinopExpr(gated_by comparison_operator)]
        pays [
            ..tcl_family_comparison_payers!(),
            StartsWith, EndsWith, Contains, Equals, Matches, MatchesRegex, MatchesGlob,
        ]
        project "operator"
    }
    ShortCircuit {
        owns [BinopExpr(gated_by short_circuit_operator)]
        pays [..tcl_family_short_circuit_payers!(), And, Or]
        project "operator"
    }
    EagerLogical { absent "Tcl's `&` / `|` inside `expr` are bitwise" }
    Ternary { pending R05 }
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
    // Tcl's keywords (`Proc`, `Namespace2`, `Set2`, `Global2`) plus
    // `when`, which opens an event handler — the F5 construct the
    // dialect exists for.
    DeclKeyword { owns [Proc, When, Namespace2, Set2, Global2] }
    Primitive { absent "Tcl has no type names" }
    FuncOpener { pending R06 }
    Closure { absent "Tcl has no anonymous function kind; `apply` takes a list" }
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
