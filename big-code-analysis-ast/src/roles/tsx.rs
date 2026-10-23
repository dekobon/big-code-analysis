//! TSX's role rows; declared through the JS-family template once R08
//! lands (contract 5 of #1690). The `Tsx` variant rides the `typescript`
//! feature.

use crate::role_table;

role_table! {
    lang: Tsx;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { absent "TSX's `&` / `|` / `^` are bitwise" }
    Ternary { pending R05 }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { absent "TSX has no guard clause" }
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
    Docstring { absent "TSX documents with JSDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
