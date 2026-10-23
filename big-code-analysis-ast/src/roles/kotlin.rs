//! Kotlin's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::role_table;
use crate::{Ancestors, Kotlin, Node};

/// Whether `node`, a modifier keyword leaf, sits in a modifier list the
/// grammar built (#1558), rather than in the recovery of a soft-keyword
/// *name* it failed to parse. Each modifier is a soft keyword that
/// valid Kotlin may use as a name, and where kotlin-ng fails to parse
/// that name the leaf survives in its recovery; such a leaf is no
/// modifier and bills nothing (`kotlin_modifiers_are_operators_1558`
/// in `metrics/halstead.rs` pins both). Two recoveries strand the leaf:
///
/// - an `ERROR` holds it, usually still wrapped in its `*_modifier`
///   node (`open = !open` at statement start, `val suspend = 1`), so the
///   first ancestor above the wrappers is the `ERROR`;
/// - a modifier list is followed by an inserted, zero-width name
///   (`fun f(vararg: Int)`, `class C(open: Int)`).
///
/// Kinds are compared by name: `*_modifier` covers every wrapper, and
/// `user_type` is where recovery can hang `type_modifiers`. The inserted
/// name is found by width, not `is_missing()` (grammar-dispatch).
fn modifier_was_written_as_one<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let host = ancestors
        .iter(node)
        .map(|(ancestor, _)| ancestor)
        .find(|ancestor| {
            let kind = ancestor.kind();
            !(kind.ends_with("_modifier") || kind == "type_modifiers" || kind == "user_type")
        });
    host.is_some_and(|host| !host.is_error()) && !next_token_is_inserted(node, ancestors)
}

/// Whether the first token after `node`, comments aside, is zero-width:
/// one error recovery inserted where the source has none. A comment
/// between the two is a sibling extra, so `vararg /*c*/: Int` must step
/// over it to reach the inserted name, as `vararg: Int` does. A node
/// never opens with an extra, which the parser hangs on the level above,
/// so the descent takes each first child as it is.
///
/// Every parameter's modifier asks, and the climb reaches
/// `function_value_parameters`, so the sibling is found without a scan
/// from the list's start: that made a long parameter list quadratic.
fn next_token_is_inserted<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    std::iter::once((*node, ancestors))
        .chain(ancestors.iter(node))
        .find_map(|(level, level_ancestors)| {
            level_ancestors
                .siblings_after(&level)
                .find(|sibling| !sibling.as_tree_sitter().is_extra())
        })
        .is_some_and(|mut next| {
            while let Some(first) = next.child(0) {
                next = first;
            }
            next.start_byte() == next.end_byte()
        })
}

role_table! {
    lang: Kotlin;
    Call { pending R08 }
    StringLit { pending R08 }
    Comparison { pending R05 }
    ShortCircuit { pending R05 }
    EagerLogical { pending R05 }
    Ternary { absent "Kotlin has no ternary operator; `if` is an expression" }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { pending R05 }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // Every modifier keyword, billed as C# bills all of its own (#1558;
    // Java and Groovy bill only some, #1565). Each is the keyword leaf;
    // its `*_modifier` / `modifiers` wrappers stay unlisted, so one
    // modifier bills once (grammar-dispatch section 5). Every keyword
    // has exactly one kind, so `kind_id` keys it without splitting it
    // across the two `n1` maps. `Annotation2` is the `annotation` of
    // `annotation class`; `Annotation` is an `@N` use, which is no
    // keyword. `in` is the variance modifier and the `for` / `x in y`
    // operator in one kind and bills with the operators, so `out` alone
    // is here. `reified` has only the `ReificationModifier` kind, which
    // `KotlinCode::is_primitive` keys by its text. Each word but
    // `companion` is a soft keyword gated on having been written as a
    // modifier: `companion` never parses as a name, and gating it would
    // only drop a real `companion object` from a class body that failed
    // to parse.
    Modifier {
        owns [
            Enum(gated_by modifier_was_written_as_one),
            Sealed(gated_by modifier_was_written_as_one),
            Annotation2(gated_by modifier_was_written_as_one),
            Data(gated_by modifier_was_written_as_one),
            Inner(gated_by modifier_was_written_as_one),
            Value(gated_by modifier_was_written_as_one),
            Public(gated_by modifier_was_written_as_one),
            Private(gated_by modifier_was_written_as_one),
            Protected(gated_by modifier_was_written_as_one),
            Internal(gated_by modifier_was_written_as_one),
            Abstract(gated_by modifier_was_written_as_one),
            Final(gated_by modifier_was_written_as_one),
            Open(gated_by modifier_was_written_as_one),
            Override(gated_by modifier_was_written_as_one),
            Lateinit(gated_by modifier_was_written_as_one),
            Const(gated_by modifier_was_written_as_one),
            Tailrec(gated_by modifier_was_written_as_one),
            Operator(gated_by modifier_was_written_as_one),
            Infix(gated_by modifier_was_written_as_one),
            Inline(gated_by modifier_was_written_as_one),
            External(gated_by modifier_was_written_as_one),
            Suspend(gated_by modifier_was_written_as_one),
            Vararg(gated_by modifier_was_written_as_one),
            Noinline(gated_by modifier_was_written_as_one),
            Crossinline(gated_by modifier_was_written_as_one),
            ReificationModifier(gated_by modifier_was_written_as_one),
            Out(gated_by modifier_was_written_as_one),
            Expect(gated_by modifier_was_written_as_one),
            Actual(gated_by modifier_was_written_as_one),
            Companion,
        ]
    }
    // `class`, `interface` (#1556), `fun`, `object`, `val`, `var` and
    // `typealias` (#1564), each the leaf of an unlisted
    // `class_declaration` / `function_declaration` / `object_declaration`
    // / `property_declaration` / `type_alias` wrapper. `typealias` is a
    // hard keyword.
    DeclKeyword { owns [Class, Interface, Fun, Object, Val, Var, Typealias] }
    Primitive { absent "Kotlin's built-in types are ordinary type names (`Int`, `String`)" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Kotlin documents with KDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
