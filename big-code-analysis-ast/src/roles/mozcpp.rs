//! Mozcpp's role rows. The dialect owns no file extension and shares
//! Cpp's grammar shape; contract 5 of #1690 declares it through Cpp's
//! template once R04 lands. Until then every row is pending or absent as
//! Cpp's is.

use crate::Mozcpp;
use crate::role_table;
use crate::roles::c_family::{c_family_decl_keywords, c_family_modifiers, c_family_primitives};

role_table! {
    lang: Mozcpp;
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
    // Cpp's three keyword rows, verbatim. The dialect shares Cpp's
    // grammar shape and owns no file extension, so nothing routes to it
    // and no corpus snapshot reaches these rows;
    // `tests/parity/cpp_mozcpp_parity.rs` holds the two vocabularies
    // together. The derivations are on the Cpp rows.
    Modifier {
        owns [
            ..c_family_modifiers!(),
            Virtual, Mutable, Constinit, Consteval, Explicit, Friend, Public, Private,
            Protected, Override, Final, Noexcept2,
        ]
    }
    DeclKeyword { owns [..c_family_decl_keywords!(), Class, Namespace, Template, Typename, Using] }
    Primitive { owns [..c_family_primitives!()] }
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
