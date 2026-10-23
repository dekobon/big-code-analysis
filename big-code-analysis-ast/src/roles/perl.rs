//! Perl's role rows: every role pending the package that declares it
//! (#1692). The census reads this file; nothing else does until then.

use crate::role_table;

role_table! {
    lang: Perl;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { pending R05 }
    Ternary { pending R05 }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { absent "Perl has no guard clause" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { absent "the pinned tree-sitter-perl grammar has no try or catch production; `eval { }` \
         parses as a block and `die` as a call" }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Perl has no modifier keywords; `my` / `our` are declarators" }
    DeclKeyword { pending R03 }
    Primitive { absent "Perl has no type names" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Perl documents with POD, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
