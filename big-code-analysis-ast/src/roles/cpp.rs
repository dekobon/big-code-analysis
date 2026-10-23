//! Cpp's role rows: every role pending the package that declares it
//! (#1692). The census reads this file; nothing else does until then.

use crate::Cpp;
use crate::role_table;
use crate::roles::c_family::{c_family_decl_keywords, c_family_modifiers, c_family_primitives};

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
    // The shared C-family lists (`roles/c_family.rs`) plus C++'s own
    // modifiers: `virtual`, `mutable`, `constinit`, `consteval`,
    // `explicit`, `friend`, the access specifiers, the `override` /
    // `final` virt-specifiers and the `noexcept` of an exception
    // specification. Each is the keyword leaf of an unclassified wrapper
    // (`virtual_specifier`, `access_specifier`,
    // `explicit_function_specifier`, the `noexcept` node that `Noexcept`
    // names and `Noexcept2` is the leaf of) or sits bare in its
    // declaration (`virtual`, `friend`), so each bills once. `override`
    // and `final` are contextual: used as a name (`int override = 1;`)
    // they parse as an `identifier`, so they need no gate. `auto` is
    // not here: in C++ it is a placeholder type, not a storage class,
    // and the grammar emits it only under `placeholder_type_specifier`.
    Modifier {
        owns [
            ..c_family_modifiers!(),
            Virtual, Mutable, Constinit, Consteval, Explicit, Friend, Public, Private,
            Protected, Override, Final, Noexcept2,
        ]
    }
    // The shared list plus `class`, `namespace`, `template`, `typename`
    // and `using` (#1552, #1557), each the leaf of an unclassified
    // wrapper (`class_specifier`, `namespace_definition`,
    // `template_declaration`, `type_parameter_declaration`,
    // `alias_declaration` / `using_declaration`). `enum class` holds two
    // of these leaves and bills both. A template-parameter `class` is
    // the synonym of `typename`, so the two spellings of
    // `template <class T>` score alike.
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
