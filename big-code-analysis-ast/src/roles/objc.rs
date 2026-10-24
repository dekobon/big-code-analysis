//! Objc's role rows. The Halstead keyword roles (#1693) and the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — are
//! declared; the rest are pending the package that declares each (#1692).

use crate::role_table;
use crate::roles::c_family::{
    c_family_binary_owners, c_family_comparison_payers, c_family_decl_keywords, c_family_modifiers,
    c_family_primitives, c_family_short_circuit_payers,
};
use crate::{Ancestors, Node, Objc};

/// Whether `node` is the attribute word of `@property (nonatomic, copy,
/// getter=isOn)`. The grammar has no keyword for any attribute: each is
/// an `identifier`, the first child of its `property_attribute`, so the
/// word is told from a name by position alone. The `isOn` of
/// `getter=isOn` is a later child and stays an operand, the selector it
/// names. `ObjcCode::is_primitive` keys the word by its text (#1579).
fn property_attribute_name<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent(node).is_some_and(|attribute| {
        attribute.kind_id() == Objc::PropertyAttribute as u16
            && attribute.start_byte() == node.start_byte()
    })
}

role_table! {
    lang: Objc;
    Call { pending R08 }
    StringLit { pending R08 }
    // The Objective-C binary operators are one `binary_expression`
    // kind, emitted under two production paths (`BinaryExpression`,
    // `BinaryExpression2`) and told apart by the `operator` field, so
    // each row's owners are gated on that field holding one of its
    // payers (`roles/c_family.rs` has the lists and their derivations).
    // A bare `<` / `>` also opens a `generic_specifier`, a
    // `protocol_reference_list` (`id<NSCopying>`), a
    // `parameterized_arguments` and an `argument_list`, which is what
    // the payers' `under BinaryExpression` gate excludes.
    Comparison {
        owns [..c_family_binary_owners!()]
        pays [..c_family_comparison_payers!()]
        project "operator"
    }
    ShortCircuit {
        owns [..c_family_binary_owners!()]
        pays [..c_family_short_circuit_payers!()]
        project "operator"
    }
    EagerLogical { absent "Objective-C's `&` / `|` / `^` are bitwise" }
    // The `conditional_expression` owns the ternary and its `?` pays,
    // ungated: the grammar spells a bare `?` in no other production. The
    // GNU elision `a ?: b` is the same node with its `consequence` field
    // empty, which the ABC slot walker reads by field (#1102).
    Ternary {
        owns [ConditionalExpression]
        pays [QMARK]
        project "operator"
    }
    NullSafe { absent "Objective-C has no optional navigation or null coalescing (nil-messaging is \
         semantics, not syntax)" }
    // A `case_statement` is the grammar's node for a `case X:` label
    // and a `default:` label alike; the arm owns a decision when it
    // holds the `case` keyword, which pays — one per label, so a
    // fall-through run `case 2: case 3:` is two. The `default` keyword
    // is the fallback and pays nothing (lesson 11, #469); Halstead
    // bills it as the keyword it is through the byte-less arm.
    CaseArm {
        owns [CaseStatement(holds_payer)]
        pays [Case]
        excludes [Default "the fallback label, no decision (lesson 11, #469)"]
    }
    Guard { absent "Objective-C has no guard clause on a case or a function head" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // The shared C-family lists (`roles/c_family.rs`), so a `.m` scores
    // a C declaration as a `.c` does, plus `auto` (a storage class here
    // as in C) and ObjC's own (#1579): the ARC ownership qualifiers
    // (`__weak`, `__strong`, `__block`, …) and `__bridge` casts, the
    // nullability qualifiers in all three spellings (`nullable`,
    // `_Nullable`, `__nullable`), `__kindof`, the `__covariant` /
    // `__contravariant` variance of a generic parameter, the `out` /
    // `inout` / `bycopy` / `byref` / `oneway` method-type qualifiers,
    // the `@public` / `@private` / `@protected` / `@package` ivar
    // visibility labels and the `@optional` / `@required` protocol
    // section labels. Each is the keyword leaf of an unclassified
    // `storage_class_specifier` / `type_qualifier` /
    // `protocol_qualifier` / `visibility_specification` or sits bare in
    // its parent, so it bills once; none has a numeric-suffix alias.
    // `in` is the sixth method-type qualifier, but one kind with the
    // fast-enumeration `in`, so it is not a modifier here. A property
    // attribute has no keyword kind at all — its word is an
    // `identifier` told by position, hence the gate.
    //
    // Not here: the Apple SDK macros this grammar also lists as storage
    // classes and qualifiers (`NS_INLINE`, `FOUNDATION_EXPORT`,
    // `IBOutlet`, `__unused`, `__ptrauth_objc_*`, …), as Mozcpp's
    // `MOZ_*` macros are, since each annotates a declaration and a `.c`
    // file parses the same spelling as a name; and GNU's `__real` /
    // `__imag`, complex-part operators the grammar misfiles as
    // qualifiers. A variable named `nullable` parses as the qualifier
    // before a MISSING name and bills it, as C's `int noreturn = 1;`
    // bills `noreturn`: the grammar decides that, not the row.
    Modifier {
        owns [
            ..c_family_modifiers!(),
            Auto, Const2, Nullable, Nullable2, Nullable3, NullableResult, NullUnspecified,
            Nonnull2, Autoreleasing, Block, Strong, UnsafeUnretained, Weak, Bridge,
            BridgeRetained, BridgeTransfer, Kindof, Covariant, Contravariant, Out, Inout,
            Bycopy, Byref, Oneway, ATpublic, ATprivate, ATprotected, ATpackage, AToptional,
            ATrequired, Identifier(gated_by property_attribute_name),
        ]
    }
    // The shared list plus ObjC's class and protocol declarations
    // (#1552), each the leaf of an unclassified `class_interface` /
    // `class_implementation` / `protocol_declaration`. `@class Fwd;`
    // needs no entry: it parses as an `@` token, billed on its own, and
    // a separate `class` leaf, so the directive already bills one
    // operator.
    DeclKeyword { owns [..c_family_decl_keywords!(), ATinterface, ATimplementation, ATprotocol] }
    // The shared list plus `_Complex` / `__complex`, a type keyword this
    // grammar files as a qualifier.
    Primitive { owns [..c_family_primitives!(), Complex, Complex2] }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Objective-C documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
