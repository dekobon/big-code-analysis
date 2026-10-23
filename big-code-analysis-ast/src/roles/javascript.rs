//! Javascript's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::Javascript;
use crate::role_table;
use crate::roles::js_family::{js_family_decl_keywords, js_family_modifiers};

role_table! {
    lang: Javascript;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { absent "JavaScript's `&` / `|` / `^` are bitwise" }
    Ternary { pending R05 }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { absent "JavaScript has no guard clause" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // The shared JS-family `static` (`roles/js_family.rs`) plus
    // `Staticget`, one token spelling `static get` followed by a
    // newline — the static getter whose name starts the next line —
    // which these two grammars alone have. It is billed beside `static`
    // (#1563) so that form bills a modifier rather than nothing; being
    // one node it is one operator.
    Modifier { owns [..js_family_modifiers!(), Staticget] }
    DeclKeyword { owns [..js_family_decl_keywords!()] }
    Primitive { absent "JavaScript has no type names" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "JavaScript documents with JSDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
