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
//! Each fragment is one `@splice` arm appending its list to the
//! accumulator `role_table!` hands it (the fragment note in `mod.rs`
//! has the shape); the row and the `opens_space` fold both walk that
//! one arm. `utils/role_tables.py` reads the same arm, and the
//! differential test in `mod.rs` holds the compiled rows to it.
//!
//! The `cpp_family_*` items below are shared by the two C++ dialects
//! only — Cpp and the vendored Mozcpp, whose grammars are one shape
//! with different ids (#1694). C and Objective-C spell no fold, no
//! `<=>` and no alternative tokens, and declare their own rows.

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
/// enum and reaches no node, which `c_family_type_specifier_is_hidden`
/// in `metrics/halstead.rs` pins for all four dialects.
macro_rules! c_family_primitives {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            PrimitiveType, TypeSpecifier, Signed, Unsigned, Long, Short,
        ] $($rest)*);
    };
}

/// The comparison tokens C++ applies under a `binary_expression` or
/// as a fold's operator, each gated by the dialect's `applied` (#1694).
/// A `grammar.json` sweep of tree-sitter-cpp 0.23.4 and the vendored
/// Mozcpp finds each in `binary_expression` (a comparison),
/// `preproc_binary_expression` (`#if A <= B`, aliased onto
/// `binary_expression`), `operator_name` (`bool operator<=(…)`
/// declares rather than applies — #1448), the fold operators (`<` `>`
/// `<=` `>=` `==` `!=`; the parent is `fold_expression`, which applies
/// its comparison once through its `operator` field — #1533) and, for
/// `<` / `>` only, the template delimiters and `system_lib_string`.
/// `NotEq` is `not_eq`, the ISO alternative token for `!=`
/// ([lex.digraph]), in every production `!=` has bar the preprocessor
/// one; `LTEQGT` is `<=>`. `GT2` is the template closer's own symbol,
/// which the parser folds onto `GT` before `kind_id()` returns, so it
/// is in no list here.
macro_rules! cpp_family_comparison_payers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            LT gated_by applied, GT gated_by applied, LTEQ gated_by applied,
            GTEQ gated_by applied, EQEQ gated_by applied, BANGEQ gated_by applied,
            NotEq gated_by applied, LTEQGT gated_by applied,
        ] $($rest)*);
    };
}

/// The short-circuit tokens, each gated by the dialect's `applied`
/// (#1694). `and` / `or` are the ISO alternative spellings of `&&` /
/// `||` ([lex.digraph]), kinds of their own that occur only in the
/// productions their symbol occurs in (#1522). The grammar also spells
/// `&&` where nothing branches — a reference declarator (`int&& x`),
/// the ref-qualifier (`void f() &&`), an overload name (`operator&&`)
/// and a requires-clause's `constraint_conjunction` (#1525) — which the
/// gate excludes by parent.
macro_rules! cpp_family_short_circuit_payers {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            AMPAMP gated_by applied, PIPEPIPE gated_by applied,
            And gated_by applied, Or gated_by applied,
        ] $($rest)*);
    };
}

/// The nodes that own a C++ binary operator: the aliased
/// `binary_expression` ids (every `binary_expression` carries
/// `BinaryExpression2` at the pinned grammars, an `#if` operand
/// included; the unsuffixed id is the never-emitted pre-alias) and the
/// fold. Which construct one owns is decided by its `operator` field,
/// so each owner is gated by the dialect's `comparison_operator`,
/// which reads the field against the `Comparison` payers.
macro_rules! cpp_family_comparison_owners {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            BinaryExpression(gated_by comparison_operator),
            BinaryExpression2(gated_by comparison_operator),
            FoldExpression(gated_by comparison_operator),
        ] $($rest)*);
    };
}

/// The same three owners under the dialect's `short_circuit_operator`.
macro_rules! cpp_family_short_circuit_owners {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)*
            BinaryExpression(gated_by short_circuit_operator),
            BinaryExpression2(gated_by short_circuit_operator),
            FoldExpression(gated_by short_circuit_operator),
        ] $($rest)*);
    };
}

/// Whether a C++ binary operator token applies its operator under its
/// parent, as opposed to spelling the same token in a declaration: an
/// overload name (`operator&&`, `operator<`), a reference declarator or
/// ref-qualifier (`int&& x`, `void f() &&`), a requires-clause
/// constraint (`requires A<T> && B<T>`), or a template delimiter.
///
/// It answers from the parent alone, so it stays O(1) per token. A
/// parenthesised constraint (`requires (A<T> && B<T>)`) parses as an
/// ordinary `binary_expression` and passes; the counting consumers
/// leave it out through the root crate's `ConstraintReach`, because
/// climbing to the enclosing `requires_clause` from every token made a
/// long `&&` chain quadratic (#1525, #1533).
///
/// An allowlist, so a grammar bump adding a production fails closed and
/// a token reparented under `{ERROR}` stops counting. Compared by kind
/// *name* so one body serves both tree-sitter-cpp and the vendored
/// Mozcpp fork, whose ids differ. A fold counts once, through its
/// `operator` field, because a binary fold `(0 && ... && a)` spells its
/// operator twice.
#[must_use]
pub fn cpp_family_operator_is_applied<'a>(
    node: &crate::Node<'a>,
    ancestors: crate::Ancestors<'a, '_>,
) -> bool {
    ancestors
        .parent(node)
        .is_some_and(|parent| match parent.kind() {
            "binary_expression" => true,
            "fold_expression" => parent
                .child_by_field_name("operator")
                .is_some_and(|op| op.id() == node.id()),
            _ => false,
        })
}

/// Whether `owner`'s `operator` field holds one of `payers`: the gate a
/// C++ `Comparison` or `ShortCircuit` owner reads, since the one
/// `binary_expression` kind carries every binary operator and only the
/// field says which construct it is. A fold's `operator` field names
/// one spelling of its operator, so a binary fold `(0 == ... == a)` is
/// owned once.
#[must_use]
pub fn cpp_family_operator_in(owner: &crate::Node<'_>, payers: &[u16]) -> bool {
    owner
        .child_by_field_name("operator")
        .is_some_and(|op| payers.contains(&op.kind_id()))
}

/// The three gates the `Comparison` and `ShortCircuit` rows read, per
/// dialect because each reads the dialect's kind ids (#1694). The
/// derivations are on the shared fragments in `roles/c_family.rs`.
macro_rules! cpp_family_gates {
    ($lang:ident) => {
        /// Whether a token applies its operator: its parent is a
        /// `binary_expression`, or a `fold_expression` whose `operator`
        /// field it is. An overload name, a reference declarator, a
        /// ref-qualifier, a template delimiter and a requires-clause's
        /// constraint conjunction all spell the same tokens and apply
        /// nothing.
        fn applied<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            $crate::roles::c_family::cpp_family_operator_is_applied(node, ancestors)
        }

        /// Whether an owner's `operator` field is a comparison token.
        fn comparison_operator<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            _ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            $crate::roles::c_family::cpp_family_operator_in(node, comparison::PAYERS)
        }

        /// Whether an owner's `operator` field is `&&` / `||` / `and` / `or`.
        fn short_circuit_operator<'a>(
            node: &$crate::Node<'a>,
            _code: &[u8],
            _ancestors: $crate::Ancestors<'a, '_>,
        ) -> bool {
            $crate::roles::c_family::cpp_family_operator_in(node, short_circuit::PAYERS)
        }
    };
}

pub(crate) use {
    c_family_decl_keywords, c_family_modifiers, c_family_primitives, cpp_family_comparison_owners,
    cpp_family_comparison_payers, cpp_family_gates, cpp_family_short_circuit_owners,
    cpp_family_short_circuit_payers,
};
