//! Rust's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::Rust;
use crate::role_table;

role_table! {
    lang: Rust;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { pending R05 }
    Ternary { absent "Rust has no ternary operator; `if` is an expression" }
    NullSafe { absent "Rust has no optional navigation or null coalescing; `?` is an exit" }
    CaseArm { pending R05 }
    Guard { pending R05 }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // `pub` is the leaf of an unlisted `visibility_modifier`, so it
    // bills once whatever the `(crate)` / `(super)` qualifier; `mut` is
    // the grammar's `mutable_specifier` node, one kind for a binding,
    // a reference and a `self` receiver. `const` and `static` are each
    // one kind for the item and the qualifier (`const fn`, `static
    // ref`), as `unsafe`, `async`, `extern` and `move` are for theirs;
    // all bill once per spelling.
    Modifier {
        owns [Pub, MutableSpecifier, Const, Static, Unsafe, Async, Extern, Move, Dyn]
    }
    // The item keywords (#394): each the leaf of an unlisted `*_item`
    // / `let_declaration` / `where_clause` wrapper.
    DeclKeyword { owns [Fn, Let, Enum, Struct, Union, Trait, Impl, Use, Mod, Type, Where] }
    // tree-sitter-rust emits `primitive_type` under seventeen aliased
    // ids, one per position the rule appears in; every one spells
    // `primitive_type` and `Checker::is_primitive` keys each by text
    // (grammar-dispatch section 1).
    Primitive {
        owns [
            PrimitiveType, PrimitiveType2, PrimitiveType3, PrimitiveType4, PrimitiveType5,
            PrimitiveType6, PrimitiveType7, PrimitiveType8, PrimitiveType9, PrimitiveType10,
            PrimitiveType11, PrimitiveType12, PrimitiveType13, PrimitiveType14, PrimitiveType15,
            PrimitiveType16, PrimitiveType17,
        ]
    }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Rust documents with `///` comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
