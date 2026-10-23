//! Ruby's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::Ruby;
use crate::role_table;

role_table! {
    lang: Ruby;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { pending R05 }
    Ternary { pending R05 }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { pending R05 }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Ruby's `private` / `protected` are method calls, declared text-gated under \
         `Member`, not keywords" }
    // `def`, `class` and `module` as keyword tokens: `Class2` /
    // `Module2` are the anonymous keyword kinds, `Class` / `Module` the
    // named constructs the space walk opens. `alias` and `undef` are
    // declarations too (`Alias2` / `Undef2` the keywords). `end` is the
    // closer every construct shares and bills with the control tokens.
    DeclKeyword { owns [Def, Class2, Module2, Alias2, Undef2] }
    Primitive { absent "Ruby has no type names" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Ruby documents with RDoc / YARD comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
