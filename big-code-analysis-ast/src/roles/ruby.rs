//! Ruby's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — and
//! the documentation rows (#1699) are declared.

use crate::lang_helpers::case_arms::ruby_in_clause_counts;
use crate::lang_helpers::doc_comment::{node_bytes, ruby_directive_opener, ruby_magic_comment};
use crate::lang_helpers::ruby::{
    YardScope, is_stabby_lambda_body, ruby_documented_call, ruby_yard_reads,
};
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

/// Whether a `call` is `raise` / `exit` / `exit!` with no receiver or
/// the `Kernel` constant as receiver. The callee is read before the
/// receiver: every call has a `method` and the text test rejects nearly
/// all of them. Invalid UTF-8 reads as no match.
fn kernel_raise_or_exit<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    node.child_by_field_name("method").is_some_and(|method| {
        method.kind_id() == Ruby::Identifier as u16
            && matches!(method.utf8_text(code), Some("raise" | "exit" | "exit!"))
    }) && node.child_by_field_name("receiver").is_none_or(|receiver| {
        receiver.kind_id() == Ruby::Constant as u16 && receiver.utf8_text(code) == Some("Kernel")
    })
}

/// Whether an `alias` names a method: `Alias` is the `alias new old`
/// statement, which YARD's alias handler documents; a global-variable
/// alias (`alias $new $old`) names no method, and YARD takes none.
fn alias_names_a_method<'a>(node: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
    node.child_by_field_name("name")
        .is_none_or(|name| name.kind_id() != Ruby::GlobalVariable as u16)
}

/// Whether a `class << X` opens on a constant: YARD registers a class for
/// `class << Const`, but nothing for `class << self` or another object.
fn singleton_of_a_constant<'a>(node: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
    node.child_by_field_name("value").is_some_and(|value| {
        matches!(
            value.kind_id().into(),
            Ruby::Constant | Ruby::ScopeResolution | Ruby::ScopeResolution2
        )
    })
}

/// Whether an assignment binds what YARD's constant or class-variable
/// handler documents — `MAX = 10`, the scoped `Foo::BAR = 1`, `@@count
/// = 0` — and not a local or an instance variable.
fn binds_a_constant_or_class_variable<'a>(node: &Node<'a>, _: &[u8], _: Ancestors<'a, '_>) -> bool {
    node.child_by_field_name("left").is_some_and(|left| {
        // role-bypass: allow(Member) — which assignment YARD documents,
        // not whether the target is a container's member
        matches!(
            left.kind_id().into(),
            Ruby::Constant | Ruby::ScopeResolution | Ruby::ScopeResolution2 | Ruby::ClassVariable
        )
    })
}

/// Whether a call is one YARD documents from the comment run above it:
/// an `attr*` macro, `alias_method`, or a visibility-wrapped `def` —
/// told by the method name's bytes.
fn yard_documented_call<'a>(node: &Node<'a>, code: &[u8], _: Ancestors<'a, '_>) -> bool {
    ruby_documented_call(node, code)
}

/// Whether a comment opens an RDoc `##` or YARD `@!` directive block
/// where the tools read one — a namespace body, never a method's: such
/// a block documents a method that has no `def` to sit above
/// (`define_method`, a DSL call).
fn opens_a_directive_block_where_yard_reads<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ruby_directive_opener(node_bytes(node, code))
        && ruby_yard_reads(YardScope::Namespace, node, ancestors)
}

// Whether a comment is a shebang or a magic comment (`# frozen_string_literal:
// true`), a run member that is not documentation text.
crate::roles::byte_gate!(is_magic_comment, ruby_magic_comment);

role_table! {
    lang: Ruby;
    // tree-sitter-ruby 0.23.1 emits four aliased visible variants of the
    // `call` rule; `Call5` (`_call`) is the hidden inner production and
    // does not surface. The `NullSafe` row claims the same kinds for the
    // safe-navigation `a&.b` a `call` can also be, told by its token.
    Call { owns [Call, Call2, Call3, Call4] }
    // The string-like literals: regular and chained strings, the bare
    // element of a `%w[]` / `%i[]` array and the arrays themselves, the
    // subshell, the regex, the heredoc body, the delimited and simple
    // symbols and the character literal. An interpolating form `wraps`
    // an `interpolation` whose expression is billed on its own; the
    // array forms `wraps` their elements (`string`, `bare_string`,
    // `bare_symbol`) — one operand per element, or one for the empty
    // array (#1353) — which the operand arm reads through the same list
    // since no kind here admits an element it is not paired with.
    // `bare_symbol` is an operand by the getter's reading and not a
    // string to `find`; `simple_symbol` and `character` are the other
    // way, strings that the operand arm bills as plain operands.
    StringLit {
        owns [
            String,
            ChainedString,
            BareString,
            Subshell,
            Regex,
            HeredocBody,
            DelimitedSymbol,
            SimpleSymbol,
            StringArray,
            SymbolArray,
            Character,
        ]
        wraps [Interpolation, String, BareString, BareSymbol]
    }
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
    // Ruby counts statement nodes, so every row here is plain (#1697).
    // The block forms and the statement-modifier forms (`return 1 if
    // a`), each its own kind.
    Branch { owns [If, Unless, IfModifier, UnlessModifier] }
    // `case … when` and the Ruby 3 `case … in` pattern match.
    Switch { owns [Case, CaseMatch] }
    Loop { owns [While, Until, For, WhileModifier, UntilModifier] }
    // A `rescue` clause, and the `expr rescue fallback` modifier under
    // its three aliases.
    Catch { owns [Rescue, RescueModifier, RescueModifier2, RescueModifier3] }
    // The grammar gives `elsif` a clause node of its own, a direct child
    // of the `if`, so the clause *is* the else-if and pays for itself.
    ElseIf { owns [Elsif(pays_itself)] pays [] }
    // `return` under its two aliased visible kinds — the pin emits only
    // `Return2`; `Return` is the defensive alias, and `Return3` the bare
    // keyword inside them, not counted on its own — and the Kernel
    // builtins `raise` / `exit` / `exit!` — ordinary `call`s whose
    // `method` field is a bare identifier spelling one of the three, with
    // no receiver or the explicit `Kernel` constant (the spelling reached
    // for when a DSL shadows the bare name). `obj.raise` / `self.exit`
    // are user methods; a bare argument-less `raise` parses as an
    // `identifier`, indistinguishable from a local, and goes uncounted;
    // `fail` / `abort` / `throw` are common DSL names and stay out
    // (#1270).
    Exit {
        owns [
            Return, Return2,
            Call(text_gated_by kernel_raise_or_exit),
            Call2(text_gated_by kernel_raise_or_exit),
            Call3(text_gated_by kernel_raise_or_exit),
            Call4(text_gated_by kernel_raise_or_exit),
        ]
    }
    Modifier { absent "Ruby's `private` / `protected` are method calls read by name in the \
         method consumer, not keywords" }
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
    // The one comment kind. RDoc and YARD read a comment run by its
    // position above a statement, never by a marker, so no single
    // comment is a `DocComment` (#1699).
    Comment { owns [Comment] }
    DocComment { absent "YARD reads a comment run by its position above a statement, never by a marker" }
    Docstring { absent "Ruby documents with RDoc / YARD comments, not string literals" }
    // The statements YARD documents from the comment run directly above
    // them: a `def`, `class` or `module`; a `class << Const`; a constant
    // or class-variable assignment (`Assignment2` and `ScopeResolution2`
    // are the grammar's aliased productions, grammar-dispatch §1); a
    // method alias (`alias`, or the `alias_method` call); an `attr*`
    // call; a visibility-wrapped `def` (`private def m`) — the last two
    // told by the call's bytes across the four `call` aliases. `Alias2`
    // is the keyword token inside the statement, never a statement, and
    // is left out. The row is the candidate; where YARD *reads* — a
    // namespace or `def` body, never a `case` or loop body — and the
    // `then` / modifier wrapper the grammar parks a comment ahead of are
    // the Checker's context over the original node. Outside the
    // space-tree rows: the singleton class, the alias, the assignment
    // and the calls — the parity exceptions `roles/doc_tests/parity.rs`
    // records.
    DocTarget {
        owns [
            Method,
            SingletonMethod,
            Class,
            Module,
            Alias(gated_by alias_names_a_method),
            SingletonClass(gated_by singleton_of_a_constant),
            Assignment(gated_by binds_a_constant_or_class_variable),
            Assignment2(gated_by binds_a_constant_or_class_variable),
            Call(text_gated_by yard_documented_call),
            Call2(text_gated_by yard_documented_call),
            Call3(text_gated_by yard_documented_call),
            Call4(text_gated_by yard_documented_call),
        ]
    }
    // RDoc's `##` and YARD's `@!` directive blocks document a method
    // that has no `def` to sit above, where the tools read — a
    // namespace body, never a method's.
    DocRunOpener { owns [Comment(text_gated_by opens_a_directive_block_where_yard_reads)] }
    // A shebang or a magic comment: a run member that is not
    // documentation text.
    DocRunDirective { owns [Comment(text_gated_by is_magic_comment)] }
    HoldsDocInBody { absent "YARD reads a comment run by scope, which the Checker folds; nothing beyond the `def` withholds one" }
}
