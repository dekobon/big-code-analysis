//! Ruby's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::role_table;
use crate::{Ancestors, Node, Ruby};

/// Whether a `binary`'s `operator` field is one of the `Comparison`
/// payers; the one node (three aliased ids) carries every binary
/// operator.
fn comparison_operator<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    comparison::project(node).is_some_and(|op| comparison::PAYERS.contains(&op.kind_id()))
}

/// Whether a `binary`'s `operator` field is a short-circuit payer.
fn short_circuit_operator<'a>(
    node: &Node<'a>,
    _code: &[u8],
    _ancestors: Ancestors<'a, '_>,
) -> bool {
    short_circuit::project(node).is_some_and(|op| short_circuit::PAYERS.contains(&op.kind_id()))
}

/// Whether a comparison token applies its operator: its parent is a
/// `binary` (any alias). Every one of the ten doubles as a *method
/// name* in a `def` (`def <(other)`, `def ==(other)`, …), where the
/// grammar parents it under `operator`; `<` additionally spells a
/// class's superclass clause (`class Foo < Bar`, parent `superclass`).
/// Neither is a comparison, and the positive guard stays correct for
/// any future non-comparison spelling (#1280). `a < b` emits the
/// `Binary2` alias; the siblings are listed defensively
/// (grammar-dispatch §1).
fn under_binary<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors
        .parent(node)
        .is_some_and(|p| comparison::OWNERS.contains(&p.kind_id()))
}

role_table! {
    lang: Ruby;
    Call { pending R08 }
    StringLit { pending R08 }
    // The `binary` node — three aliased ids — carries every binary
    // operator; its `operator` field says which, so each row's owner is
    // gated on the field. The ten comparison tokens are the equalities
    // `==` `!=` `===`, the orderings, the spaceship `<=>` and the
    // pattern matches `=~` / `!~`, each gated to a `binary` parent
    // since each is also a method name an operator `def` spells.
    Comparison {
        owns [
            Binary(gated_by comparison_operator),
            Binary2(gated_by comparison_operator),
            Binary3(gated_by comparison_operator),
        ]
        pays [
            EQEQ gated_by under_binary, BANGEQ gated_by under_binary,
            EQEQEQ gated_by under_binary, LT gated_by under_binary, GT gated_by under_binary,
            LTEQ gated_by under_binary, GTEQ gated_by under_binary, LTEQGT gated_by under_binary,
            EQTILDE gated_by under_binary, BANGTILDE gated_by under_binary,
        ]
        project "operator"
    }
    // `&&` / `||` and the low-precedence keyword forms `and` / `or`;
    // under error recovery a token's parent can be an `ERROR` (`f(&&)`)
    // with no operands to score, which the consumer handles. The safe
    // navigation `&.` is `NullSafe`'s.
    ShortCircuit {
        owns [
            Binary(gated_by short_circuit_operator),
            Binary2(gated_by short_circuit_operator),
            Binary3(gated_by short_circuit_operator),
        ]
        pays [AMPAMP, PIPEPIPE, And, Or]
        project "operator"
    }
    // `&` / `|` / `^` are method calls on any receiver — `true & x`
    // evaluates both sides, as `1 & x` does — and the grammar emits
    // them as the same `binary` as every other operator method; no
    // consumer treats them as a decision, and the tokens bill as the
    // operators they are. A recognition boundary, recorded as the kinds
    // considered rather than as an absence.
    EagerLogical {
        owns []
        excludes [
            AMP "a boolean `&` is the operator method's kind, which the grammar does not tell from the integral one",
            PIPE "a boolean `|` is the operator method's kind, which the grammar does not tell from the integral one",
            CARET "a boolean `^` is the operator method's kind, which the grammar does not tell from the integral one",
        ]
    }
    // The `conditional` owns it and its `?` pays, ungated: the grammar
    // spells the bare token nowhere else (a character literal `?a` and
    // a predicate name `x?` are single tokens of their own).
    Ternary {
        owns [Conditional]
        pays [QMARK]
        project "operator"
    }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { pending R05 }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Ruby's `private` / `protected` are method calls, declared text-gated under \
         `Member`, not keywords" }
    // `def`, `class` and `module` as keyword tokens: `Class2` /
    // `Module2` are the anonymous keyword kinds, `Class` / `Module` the
    // named constructs the space walk opens. `alias` and `undef` are
    // declarations too (`Alias2` / `Undef2` the keywords). `end` is the
    // closer every construct shares and bills with the control tokens.
    DeclKeyword { owns [Def, Class2, Module2, Alias2, Undef2] }
    Primitive { absent "Ruby has no type names" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Ruby documents with RDoc / YARD comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
