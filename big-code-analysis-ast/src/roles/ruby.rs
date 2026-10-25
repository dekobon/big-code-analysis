//! Ruby's role rows. The Halstead keyword roles (#1693) and the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — are
//! declared; the rest are pending the package that declares each (#1692).

use crate::checker::ruby::is_stabby_lambda_body;
use crate::lang_helpers::case_arms::ruby_in_clause_counts;
use crate::role_table;
use crate::{Ancestors, Node, Ruby};

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

/// Whether a `case … in` arm is a decision: its pattern is not a bare
/// `_`, or it carries an `if` / `unless` guard (#977; lesson 11).
fn is_decision_in_clause<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    ruby_in_clause_counts(node, code)
}

/// Whether a `block` / `do_block` is a closure and a space of its own:
/// not the body of a stabby lambda (`->(z) { … }` / `->(z) do … end`),
/// whose `lambda` wrapper already is both (#465, #1257). The keyword
/// forms `lambda { }` / `proc { }` and iterator blocks parse as a
/// `call` carrying the block, so those classify once.
fn not_a_lambda_body<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    !is_stabby_lambda_body(node, ancestors)
}

/// Whether an `@ivar` / `@@cvar` is the assignment target it declares:
/// the first child of an `assignment` (`@x = 1`); a read or an operand
/// is no declaration.
fn is_assignment_target<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    ancestors.parent(node).is_some_and(|parent| {
        matches!(
            parent.kind_id().into(),
            Ruby::Assignment | Ruby::Assignment2
        ) && parent.child(0).is_some_and(|first| first.id() == node.id())
    })
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
            Binary(holds_payer),
            Binary2(holds_payer),
            Binary3(holds_payer),
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
            Binary(holds_payer),
            Binary2(holds_payer),
            Binary3(holds_payer),
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
    // The safe navigation `a&.b`, a `call` (four aliased ids, per
    // grammar-dispatch §1; the pinned grammar emits `Call3` for every
    // spelling) owned when it holds the `&.` token, which cyclomatic
    // pays once per textual operator (#452); ABC and cognitive read it
    // not at all, and every other reader of a `call` — `is_call`, the
    // ABC branch, the `attr_*` and visibility walks — reads the call,
    // not the navigation. Ruby has no null coalescing: `a || b` and
    // `a ||= b` are the `||` they spell.
    NullSafe {
        owns [
            Call(holds_payer), Call2(holds_payer),
            Call3(holds_payer), Call4(holds_payer),
        ]
        pays [AMPDOT]
        project "operator"
    }
    // A `when` arm of `case … when` owns a decision and pays at the
    // node (its `when` keyword, `When2`, bills as the keyword it is), as
    // does an `in_clause` of `case … in` unless its pattern is a bare
    // `_` with no guard, the pattern match's default arm (#977). The
    // `else` of either is the fallback and pays nothing (lesson 11).
    CaseArm {
        owns [When, InClause(text_gated_by is_decision_in_clause)]
        excludes [
            Else "the fallback arm of a `case`, no decision (lesson 11); an `if`'s `else` is `Branch`'s",
            When2 "the arm's keyword, billed as such; the arm pays",
            In2 "the pattern arm's keyword, billed as such; the arm pays",
        ]
    }
    // An `in X if g` / `in X unless g` guard is the `if_guard` /
    // `unless_guard` node (`Guard` the hidden `_guard` supertype, listed
    // per lesson 34), one decision paid at the node: cyclomatic counts
    // it and ABC walks its condition field; the keyword bills as the
    // `if` / `unless` it is.
    Guard { owns [Guard, IfGuard, UnlessGuard] }
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
    // A `def` and a `def self.` are the callables. A stabby lambda is
    // a closure and opens a space; a `{ … }` / `do … end` block is both
    // too, unless it is a stabby lambda's own body (`not_a_lambda_body`
    // — #465, #1257), in which case the `lambda` above it already is.
    FuncOpener {
        owns [
            Method(callable opens Function),
            SingletonMethod(callable opens Function),
            Lambda(opens Function),
            Block(opens Function gated_by not_a_lambda_body),
            DoBlock(opens Function gated_by not_a_lambda_body),
        ]
    }
    Closure {
        owns [
            Lambda,
            Block(gated_by not_a_lambda_body),
            DoBlock(gated_by not_a_lambda_body),
        ]
    }
    // A `module` opens `Namespace`; `class << self` is a class-like
    // body.
    Container {
        owns [
            Program(opens Unit),
            Class(opens Class),
            SingletonClass(opens Class),
            Module(opens Namespace),
        ]
    }
    // An instance or class variable where it is declared — the target
    // of an assignment (`@x = 1`, `@@n = 0`); where in the body it
    // sits, the `attr_*` macro calls that declare attributes by name,
    // and the visibility markers between them are the consumer's
    // reading (`npa/shared.rs`).
    Member {
        owns [
            InstanceVariable(gated_by is_assignment_target),
            ClassVariable(gated_by is_assignment_target),
        ]
    }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Ruby documents with RDoc / YARD comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
