//! Tcl's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::Tcl;
use crate::role_table;

role_table! {
    lang: Tcl;
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
