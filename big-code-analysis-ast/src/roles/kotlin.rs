//! Kotlin's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::getter::node_text;
use crate::lang_helpers::case_arms::kotlin_when_entry_is_else;
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

/// Whether an `infix_expression` applies one of the eager boolean
/// functions `and` / `or` / `xor`. tree-sitter-kotlin-ng gives an infix
/// call no field and no operator kind: it is three children — the left
/// operand, which may be any expression, an `identifier` leaf spelling
/// the function's name, and the right operand — so only the bytes tell
/// `a and b` from `a shl b` or `a to b`, and the name is the second
/// child that is not a comment. The name is all that is read: a user
/// `infix fun and` on another type, or `Int.and`, spells the same call,
/// which no syntax can tell from `Boolean.and`.
fn eager_infix<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    node.children()
        .filter(|child| !child.as_tree_sitter().is_extra())
        .nth(1)
        .filter(|name| name.kind_id() == Kotlin::Identifier as u16)
        .and_then(|name| node_text(code, &name))
        .is_some_and(|name| matches!(name, "and" | "or" | "xor"))
}

/// Whether a `binary_expression`'s `operator` field is one of the
/// `Comparison` payers; the one kind carries every binary operator.
fn comparison_operator<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_some_and(|op| comparison::PAYERS.contains(&op.kind_id()))
}

/// Whether a `binary_expression`'s `operator` field is `&&` or `||`.
fn short_circuit_operator<'a>(
    node: &Node<'a>,
    _code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    short_circuit::project(node).is_some_and(|op| short_circuit::PAYERS.contains(&op.kind_id()))
}

/// Whether a bare `<` / `>` is the operator of a `binary_expression`.
/// tree-sitter-kotlin-ng 1.1.0 emits the tokens from four productions —
/// `binary_expression`, `super_expression` (`super<A>.g()`),
/// `type_arguments` and `type_parameters` (#1297) — and error recovery
/// strands either. The enumeration is a claim about the grammar, not
/// about every parse it produces: the grammar resolves a generic
/// *call* the wrong way, so `id<Int>(a)` comes back as nested
/// `binary_expression`s and both brackets pass (#1394).
fn under_binary_expression<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent_has_kind(node, Kotlin::BinaryExpression as u16)
}

/// Whether a `when_entry` is a case-style arm rather than the `else ->`
/// fallback, which has no `condition` field (lesson 11, #469).
fn is_case_entry<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    !kotlin_when_entry_is_else(node)
}

/// Whether an `is` / `!is` keyword is the operator of an
/// `is_expression`: the same tokens open a `when` entry's `type_test`,
/// where the entry pays.
fn under_is_expression<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent_has_kind(node, Kotlin::IsExpression as u16)
}

/// Whether an `in` / `!in` keyword is the operator of an
/// `in_expression`: the same tokens open a `for (x in xs)` header and
/// a `when` entry's `range_test`, where the entry pays.
fn under_in_expression<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent_has_kind(node, Kotlin::InExpression as u16)
}

role_table! {
    lang: Kotlin;
    Call { pending R08 }
    StringLit { pending R08 }
    // A `binary_expression` owns a comparison through its `operator`
    // field, with `<` / `>` gated to that parent; the type test
    // `is_expression` and the membership test `in_expression` own
    // theirs through their keyword, each a token of its own in both
    // the positive and the negated spelling (`is` / `!is`, `in` /
    // `!in`) and gated to the expression, since the `when` entry forms
    // `is Int ->` / `in xs ->` and the `for` header spell the same
    // keywords. The safe cast `as?` is a decision the ABC consumer
    // counts beside these, but no comparison; it stays its own arm.
    Comparison {
        owns [
            BinaryExpression(gated_by comparison_operator),
            IsExpression, InExpression,
        ]
        pays [
            EQEQ, EQEQEQ, BANGEQ, BANGEQEQ, LTEQ, GTEQ,
            LT gated_by under_binary_expression, GT gated_by under_binary_expression,
            Is gated_by under_is_expression, BANGis gated_by under_is_expression,
            In gated_by under_in_expression, BANGin gated_by under_in_expression,
        ]
        project "operator"
    }
    // `&&` / `||`, which the grammar emits only under a
    // `binary_expression`. The elvis `?:` is `NullSafe`'s, and the
    // infix `and` / `or` / `xor` are `EagerLogical`'s.
    ShortCircuit {
        owns [BinaryExpression(gated_by short_circuit_operator)]
        pays [AMPAMP, PIPEPIPE]
        project "operator"
    }
    // The infix functions `and` / `or` / `xor` evaluate both operands
    // (`Boolean.and` is a function, not the `&&` operator), so the
    // decision-counting consumers add no chain increment for them: a
    // slot holding one pays under the ordinary slot rule, as it would
    // for any call. The grammar spells the name as a plain `identifier`
    // inside the `infix_expression`, which Halstead bills as an operand
    // and keeps billing — the row records the construct and nothing
    // reads it to count (#1695). Owner-only and text-gated, since
    // claiming `Identifier` as a payer would claim every name.
    EagerLogical { owns [InfixExpression(text_gated_by eager_infix)] }
    Ternary { absent "Kotlin has no ternary operator; `if` is an expression" }
    NullSafe { pending R05 }
    // A `when_entry` owns a decision and pays at the node — the grammar
    // gives the arm no keyword of its own; its `->` is punctuation —
    // unless it is the `else ->` fallback, told by the absence of a
    // `condition` field (lesson 11, #469). A subject-ful entry's `is` /
    // `in` tests are the entry's, not the `Comparison` row's.
    CaseArm { owns [WhenEntry(gated_by is_case_entry)] }
    Guard { absent "Kotlin 2.x's `when` guards are not in the pinned grammar; `if` is an expression" }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KotlinCode;
    use crate::test_support::for_each_node_with_chain;

    // The infix eager functions are told from every other infix call by
    // their name alone (#1695): `and` / `or` / `xor` own the construct,
    // `shl` / `to` / a user infix do not. The row lists no payer — the
    // name is an `identifier` that nothing bills as an operator — so
    // the owner is the paying node at both projections, and no token
    // pays.
    #[cfg(feature = "kotlin")]
    #[test]
    fn eager_infix_is_told_by_its_name() {
        let source = b"fun f(a: Boolean, b: Boolean, x: Int) { val c = a and b; val d = a or b; val e = a xor b; val s = x shl 1; val p = 1 to 2; val m = a myInfix b; val n = (a && b) xor a; val o = a and /* c */ b and a; val q = (a) to and }\n";
        let (mut owned, mut paid) = (Vec::new(), Vec::new());
        for_each_node_with_chain::<KotlinCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let text = || node.utf8_text(source).expect("fixture is UTF-8").to_owned();
            if eager_logical::owns(node, source, ancestors) {
                owned.push(text());
            }
            if eager_logical::pays_at_leaf(node, source, ancestors) {
                paid.push(text());
            }
        });
        // A compound left operand, a comment before the name, a nested
        // chain (each `and` owns its own node) and a right operand
        // *named* `and`, which is no function.
        assert_eq!(
            owned,
            [
                "a and b",
                "a or b",
                "a xor b",
                "(a && b) xor a",
                "a and /* c */ b and a",
                "a and /* c */ b",
            ]
        );
        assert_eq!(paid, owned);
        assert_eq!(eager_logical::PAYERS, &[] as &[u16]);
    }
}
