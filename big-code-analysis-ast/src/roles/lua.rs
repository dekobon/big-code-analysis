//! Lua's role rows: every role pending the package that declares it
//! (#1692). The census reads this file; nothing else does until then.

use crate::role_table;

role_table! {
    lang: Lua;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { absent "Lua's `and` / `or` are lazy; it has no eager boolean operator" }
    Ternary { absent "Lua has no ternary expression; `a and b or c` is a chain" }
    NullSafe { absent "Lua has no optional navigation or null coalescing" }
    CaseArm { absent "Lua has no switch-like construct" }
    Guard { absent "Lua has no guard clause" }
    Branch { pending R07 }
    Switch { absent "Lua has no switch-like construct" }
    Loop { pending R07 }
    Catch { absent "Lua has no exception handling; `pcall` is a call" }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Lua has no modifier keywords; `local` is a declaration" }
    DeclKeyword { pending R03 }
    Primitive { absent "Lua has no type names" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { absent "Lua has no container to hold a member" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Lua documents with LDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
