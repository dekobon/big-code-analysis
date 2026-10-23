//! The kind lists the four C-family dialects share (#1693). Each is a
//! fragment a dialect's row splices with `..name!()`; the dialect adds
//! its own kinds beside it, so a kind here is one every dialect bills
//! and nothing is promoted into a grammar that did not list it. The
//! lists are the intersection of the four `Getter::get_op_type` arms
//! as #1578, #1579 and #1557 left them, which is what #1598 asked for:
//! `Volatile2` was missing from all four copies at once, and a kind
//! named once cannot be missing from one.
//!
//! The four grammars assign these kinds different ids, so a fragment
//! can only spell variant *names*; `role_table!` resolves each against
//! the dialect enum the row names, and a variant missing from one enum
//! fails to compile there.
//!
//! Each fragment carries its list twice, once per arm, because `$$`
//! meta-variables are unstable (see the fragment note in `mod.rs`).
//! The compiled crate reads the arms independently, so
//! `utils/role_tables.py` is what holds the two to each other: the
//! role gates fail on a fragment whose arms differ.

/// Storage classes, function specifiers and type qualifiers every
/// dialect bills (#1578). Each is the keyword leaf of an unclassified
/// `storage_class_specifier` / `type_qualifier` / `alignas_qualifier`,
/// so it bills once, as `const` does in a pointer declarator
/// (`int * const p`). The vendor spellings (`__inline`, `__restrict__`,
/// `__thread`, `__volatile__`, …) are kinds of their own and bill under
/// their own name; `__volatile__` the grammars emit only as a GNU
/// `asm` qualifier, where `volatile` bills too. `__extension__` bills
/// in an expression too, where it is the prefix of an
/// `extension_expression`. `restrict` and `noreturn` are no C++
/// keywords and `noreturn` is none in C without `<stdnoreturn.h>`, but
/// every grammar parses the name as the qualifier before a zero-width
/// `identifier`, so the keyword bills and no operand does — the grammar
/// decides that, not the row.
///
/// Attributes (`__attribute__`, `__declspec`, `[[…]]`) and the MS
/// calling conventions are in no dialect's row: they annotate a
/// declaration rather than spell part of it.
macro_rules! c_family_modifiers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            Static, Extern, Register, Inline, Inline2, Inline3, Forceinline, ThreadLocal,
            Thread, Const, Constexpr, Volatile, Volatile2, Restrict, Restrict2, Atomic,
            Noreturn, Noreturn2, Nonnull, Alignas, Alignas2, Extension,
        ] $($rest)*);
    };
    (@opens_splice $ctx:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@opens_owners $ctx [$($acc)*
            Static, Extern, Register, Inline, Inline2, Inline3, Forceinline, ThreadLocal,
            Thread, Const, Constexpr, Volatile, Volatile2, Restrict, Restrict2, Atomic,
            Noreturn, Noreturn2, Nonnull, Alignas, Alignas2, Extension,
        ] $($rest)*);
    };
}

/// The type-declaration keywords every dialect bills: `struct`, `union`
/// and `enum` (#1552) and `typedef` (#1557). Each is the keyword leaf of
/// a `struct_specifier` / `union_specifier` / `enum_specifier` /
/// `type_definition`, none of which is classified, so a declaration
/// bills its keyword once. The same leaf heads an elaborated type
/// (`struct S s;`), where it bills too, as `int` does in `int s;`.
macro_rules! c_family_decl_keywords {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)* Struct, Union, Enum, Typedef,] $($rest)*);
    };
    (@opens_splice $ctx:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@opens_owners $ctx [$($acc)* Struct, Union, Enum, Typedef,] $($rest)*);
    };
}

/// The built-in type names every dialect bills. `primitive_type` is one
/// kind for every spelling (`int`, `double`, `void`, …), so each
/// dialect's `Checker::is_primitive` keys it by text and distinct
/// primitives count separately. A `sized_type_specifier` carries its
/// `unsigned` / `signed` / `long` / `short` as bare keyword tokens, not
/// as `primitive_type` children (`unsigned int` is `unsigned` plus
/// `primitive_type int`; `signed long` and `long long` have no
/// `primitive_type` at all), so each word is listed under its own kind
/// (#466): `long long`'s two tokens fold to one `n1` entry and two `N1`
/// hits. `type_specifier` is the hidden supertype; it exists in every
/// enum and reaches no node.
macro_rules! c_family_primitives {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            PrimitiveType, TypeSpecifier, Signed, Unsigned, Long, Short,
        ] $($rest)*);
    };
    (@opens_splice $ctx:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@opens_owners $ctx [$($acc)*
            PrimitiveType, TypeSpecifier, Signed, Unsigned, Long, Short,
        ] $($rest)*);
    };
}

pub(crate) use {c_family_decl_keywords, c_family_modifiers, c_family_primitives};
