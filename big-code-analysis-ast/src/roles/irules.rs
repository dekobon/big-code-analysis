//! iRules' role rows; declared through the Tcl template once R06 lands
//! (contract 5 of #1690). The Halstead keyword role is declared (#1693).

use crate::Irules;
use crate::role_table;

role_table! {
    lang: Irules;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
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
