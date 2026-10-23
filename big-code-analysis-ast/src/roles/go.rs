//! Go's role rows: every role pending the package that declares it
//! (#1692). The census reads this file; nothing else does until then.

use crate::role_table;

role_table! {
    lang: Go;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { absent "Go's `&` / `|` / `^` are bitwise" }
    Ternary { absent "Go has no ternary expression" }
    NullSafe { absent "Go has no optional navigation or null coalescing" }
    CaseArm { pending R05 }
    Guard { absent "Go has no guard clause on a case or a function head" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { absent "Go has no exception handling; `recover` is a call" }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Go has no modifier keywords; visibility is the identifier's case" }
    DeclKeyword { pending R03 }
    Primitive { pending R03 }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Go documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
