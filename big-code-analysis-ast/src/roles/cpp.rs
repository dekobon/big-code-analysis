//! Cpp's role rows: every role pending the package that declares it
//! (#1692). The census reads this file; nothing else does until then.

use crate::role_table;

role_table! {
    lang: Cpp;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R04 }
    ShortCircuit { pending R04 }
    EagerLogical { absent "C++'s `&` / `|` / `^` are bitwise; `and` / `or` are the lazy spellings" }
    Ternary { pending R05 }
    NullSafe { absent "C++ has no optional navigation or null coalescing" }
    CaseArm { pending R05 }
    Guard { absent "C++ has no guard clause on a case or a function head" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { pending R03 }
    DeclKeyword { pending R03 }
    Primitive { pending R03 }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "C++ documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
