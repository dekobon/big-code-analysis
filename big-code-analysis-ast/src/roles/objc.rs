//! Objc's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — and
//! the documentation rows (#1699) are declared.

use crate::role_table;
use crate::roles::c_family::{
    c_doc_declarations, c_family_binary_owners, c_family_calls, c_family_comparison_payers,
    c_family_decl_keywords, c_family_doc_entries, c_family_doc_gates, c_family_loops,
    c_family_modifiers, c_family_primitives, c_family_short_circuit_payers, c_family_strings,
    objc_doc_conditionals,
};
use crate::roles::control_flow::else_if_parent_clause_gate;
use crate::{Ancestors, Node, Objc};

// The documentation gates, per dialect (#1699).
c_family_doc_gates!(with_has_body ObjcCode);

/// Whether an `implementation_definition` is one Doxygen documents —
/// not a `@synthesize` / `@dynamic` property implementation, which
/// declares nothing of its own.
fn implementation_is_not_a_property<'a>(node: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
    node.child(0)
        .is_some_and(|d| d.kind_id() != Objc::PropertyImplementation as u16)
}

// The chained `if` sits directly inside an `else_clause`; a nested one
// under `else { … }` sits inside the compound statement (#1697).
else_if_parent_clause_gate!(Objc, ElseClause);

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
    // A C `call_expression` (aliased to `CallExpression2`) or a message
    // send `[receiver method]`.
    Call { owns [..c_family_calls!(), MessageExpression] }
    // ObjC has no raw string literal; `@"…"` parses as `AT` + a plain
    // `string_literal`, so the family's two kinds apply.
    StringLit { owns [..c_family_strings!()] }
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
    // The C family's control flow (#1697): the one-kind rows are spelled
    // inline and the loop list is the family's fragment
    // (`roles/c_family.rs`); the `ElseIf` gate is emitted beside this
    // table. ObjC's own shape:
    // `Catch` pays at the owner — its `@catch` is the `ATcatch` keyword,
    // so the handler node is what pays, as the cyclomatic reading always
    // had it — and `@throw` (the `throw_statement` node) joins C's
    // `return`, without C++'s coroutine exits.
    Branch { owns [IfStatement] pays [If] }
    Switch { owns [SwitchStatement] }
    Loop { owns [..c_family_loops!()] pays [For, While] }
    Catch { owns [CatchClause] }
    ElseIf { owns [IfStatement(gated_by continues_else_clause)] pays [] }
    Exit { owns [ReturnStatement, ThrowStatement] }
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
    // Both aliased `function_definition` ids (#285) and a
    // `method_definition` — the `@implementation`-side method with a
    // body; the `@interface`-side `method_declaration` has none and is
    // no function — are the callables. A block literal (`^{ … }`) is a
    // closure that opens no space, as the C++ lambda. `@implementation`
    // carries the method bodies and opens `Class`; `@interface` and
    // `@protocol` declare without bodies and open `Interface`.
    FuncOpener {
        owns [
            FunctionDefinition(callable opens Function),
            FunctionDefinition2(callable opens Function),
            MethodDefinition(callable opens Function),
        ]
    }
    Closure { owns [BlockLiteral] }
    Container {
        owns [
            TranslationUnit(opens Unit),
            ClassImplementation(opens Class),
            ClassInterface(opens Interface),
            ProtocolDeclaration(opens Interface),
        ]
    }
    // A `@property` declaration, direct in an `@interface` /
    // `@implementation` / `@protocol` or grouped under a
    // `@required` / `@optional` marker, and an instance variable in
    // the `{ … }` block, which the consumer counts by declarator and
    // reads the access section of.
    Member { owns [PropertyDeclaration, InstanceVariable] }
    // The one comment kind, a `DocComment` owner under Doxygen's marker
    // gate (#1699).
    Comment { owns [Comment] }
    DocComment { owns [Comment(text_gated_by opens_with_doxygen_marker)] }
    Docstring { absent "Objective-C documents with comments, not string literals" }
    // What Doxygen documents from a doc block before it: the entries of
    // `roles/c_family.rs`'s fragment
    // plus Objective-C's containers (a protocol's `@optional` /
    // `@required` section included), property, methods and instance
    // variable, and every
    // preprocessor conditional holding an entry in any branch
    // (`conditional_declares`, memoised). The `ERROR` node holding an
    // entry is the Checker's clause. Most targets are outside the
    // space-tree rows — a macro, a typedef, an enumerator, a field, a
    // conditional — the parity exceptions `roles/doc_tests/parity.rs` records.
    DocTarget {
        owns [
            ..c_family_doc_entries!(),
            ..c_doc_declarations!(),
            ClassInterface,
            ClassImplementation,
            ProtocolDeclaration,
            QualifiedProtocolInterfaceDeclaration,
            PropertyDeclaration,
            MethodDeclaration,
            MethodDefinition,
            InstanceVariable,
            ImplementationDefinition(gated_by implementation_is_not_a_property),
            ..objc_doc_conditionals!(),
        ]
    }
    DocRunOpener { absent "Doxygen attaches every block to an entry or the file; none documents by its own content" }
    DocRunDirective { absent "Doxygen reads no directive comment" }
    HoldsDocInBody { absent "Doxygen reads every block in a body as the function's in-body description; nothing withholds one" }
}
