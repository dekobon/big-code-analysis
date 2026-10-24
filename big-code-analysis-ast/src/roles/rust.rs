//! Rust's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::lang_helpers::case_arms::pattern_is_bare_underscore;
use crate::role_table;
use crate::{Ancestors, Node, Rust};

/// Whether a `binary_expression`'s `operator` field is one of the
/// `Comparison` payers. The one kind carries every binary operator.
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

/// Whether a `match_arm` is a decision: its `pattern` is not a bare `_`
/// (lesson 11, #469). `Some(_)`, `(_, x)` and `_ if g` are not bare and
/// still count; a leading `|` of an or-pattern is skipped. The field is
/// required at the pinned grammar, so an arm without one — error
/// recovery only — counts, the polarity both consumers had (#1454).
fn is_decision_arm<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    !node
        .child_by_field_name("pattern")
        .is_some_and(|pat| pattern_is_bare_underscore(&pat, Rust::UNDERSCORE as u16))
}

/// Whether a `match_pattern` carries a guard: its `condition` field.
fn has_guard<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    node.child_by_field_name("condition").is_some()
}

/// Whether an `if` keyword is a match guard's rather than an `if`
/// expression's: its parent is a `match_pattern`.
fn under_match_pattern<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent_has_kind(node, Rust::MatchPattern as u16)
}

/// Whether a bare `<` / `>` is the operator of a `binary_expression`
/// rather than a generic bracket (`Vec<T>`, a turbofish) or a token
/// error recovery strands.
fn under_binary_expression<'a>(
    node: &Node<'a>,
    _code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.parent_has_kind(node, Rust::BinaryExpression as u16)
}

role_table! {
    lang: Rust;
    Call { pending R08 }
    StringLit { pending R08 }
    // The one `binary_expression` kind carries every binary operator;
    // its `operator` field says which, so the owner is gated on the
    // field and `<` / `>` on the parent: a generic list and a turbofish
    // spell the same tokens. The four other tokens occur nowhere else.
    Comparison {
        owns [BinaryExpression(gated_by comparison_operator)]
        pays [
            EQEQ, BANGEQ, LTEQ, GTEQ,
            LT gated_by under_binary_expression, GT gated_by under_binary_expression,
        ]
        project "operator"
    }
    // A `binary_expression` whose operator is `&&` / `||`, and a Rust
    // 2024 let-chain (`if let Some(x) = a && b`) — `LetChain` the
    // visible alias, `LetChain2` the hidden `_let_chain` supertype —
    // whose `&&` tokens are its direct children, so the chain owns them
    // with no field to project through and the row's payer fallback
    // finds the first. The tokens pay ungated: the grammar spells them
    // bare only inside a macro's `token_tree` (`m!(a || b)`), where the
    // parser sees no expression and the counting consumers have always
    // paid them (an empty closure's `||` is two `|` tokens under
    // `closure_parameters`, not this kind).
    ShortCircuit {
        owns [
            BinaryExpression(gated_by short_circuit_operator),
            LetChain, LetChain2,
        ]
        pays [AMPAMP, PIPEPIPE]
        project "operator"
    }
    // `&` / `|` / `^` on boolean operands evaluate both sides, but the
    // grammar emits them as the same `binary_expression` as the integral
    // forms, so the syntax alone cannot tell them apart; no consumer
    // treats them as a decision, and the three tokens bill as the
    // operators they are. A recognition boundary, recorded as the
    // kinds considered rather than as an absence.
    EagerLogical {
        owns []
        excludes [
            AMP "a boolean `&` is the integral `&`'s kind, which the grammar does not tell apart",
            PIPE "a boolean `|` is the integral `|`'s kind, which the grammar does not tell apart",
            CARET "a boolean `^` is the integral `^`'s kind, which the grammar does not tell apart",
        ]
    }
    Ternary { absent "Rust has no ternary operator; `if` is an expression" }
    NullSafe { absent "Rust has no optional navigation or null coalescing; `?` is an exit" }
    // A `match_arm` (`MatchArm2` the second id carrying the kind, per
    // grammar-dispatch §1) owns a decision and pays at the node — its
    // `=>` is punctuation — unless its pattern is a bare `_`, the
    // C-family `default:` (lesson 11, #469).
    CaseArm { owns [MatchArm(gated_by is_decision_arm), MatchArm2(gated_by is_decision_arm)] }
    // A match guard is the `match_pattern`'s `condition` field, opened
    // by an `if` keyword the grammar shares with the `if` expression;
    // the pattern owns it where the field is present and the keyword
    // pays, gated to the pattern. Cyclomatic counts that keyword
    // wherever it is written, so the guard pays as an `if` does; ABC
    // walks the field as a boolean slot.
    Guard {
        owns [MatchPattern(gated_by has_guard)]
        pays [If gated_by under_match_pattern]
    }
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
    // tree-sitter-rust spells `primitive_type` as
    // `alias(choice(u8, i8, …, char), $.primitive_type)`: one generated
    // id per spelling, seventeen in all, every one named
    // `primitive_type`. The parser canonicalises them, so written source
    // reaches `kind_id()` as the base `PrimitiveType` and the sixteen
    // suffixed ids are defensive (grammar-dispatch section 1;
    // `rust_aliased_primitive_type_classification` records which is
    // observed). `Checker::is_primitive` keys each by text.
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
