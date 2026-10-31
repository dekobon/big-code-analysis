//! iRules' role rows. The Halstead keyword role (#1693), the
//! paying-construct roles (#1694, #1695), the space tree (#1696) and the
//! control flow — `Branch`, `Switch`, `Loop`, `Catch`, `ElseIf`, `Exit`
//! (#1697) — are declared, the Tcl-shaped ones through the shared
//! `tcl_family` fragments (contract 5 of #1690); the rest are pending
//! the package that declares each (#1692).

use crate::lang_helpers::case_arms::irules_switch_arm_is_decision;
use crate::role_table;
use crate::roles::tcl_family::{
    tcl_family_comparison_payers, tcl_family_short_circuit_payers, tcl_family_string_gates,
    tcl_family_strings,
};
use crate::{Ancestors, Irules, Node};

/// Whether a `switch_arm` is a decision arm rather than `default`,
/// which only its `pattern` bytes tell (lesson 11, #106).
fn is_decision_arm<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    irules_switch_arm_is_decision(node, code, ancestors)
}

tcl_family_string_gates!(
    Irules,
    IrulesCode,
    crate::lang_helpers::irules::BRACED_WORD_KINDS
);

/// Whether a `command` is `return` or `error`, by the text of its `name`
/// field with a leading `::` stripped. Invalid UTF-8 reads as no match.
fn return_or_error<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    node.child_by_field_name("name").is_some_and(|name| {
        matches!(
            name.utf8_text(code)
                .map(crate::lang_helpers::strip_global_qualifier),
            Some("return" | "error")
        )
    })
}

role_table! {
    lang: Irules;
    // Every command is a call, as in Tcl; the `when` handler and its
    // event name are not one.
    Call { owns [Command] }
    // The Tcl twin's three kinds (`roles/tcl_family.rs`), the braced
    // word gated on being a literal rather than a handler body — which
    // matters more here: a `when` handler's body is the whole of a
    // typical iRules file (#1381).
    StringLit {
        owns [..tcl_family_strings!()]
        wraps [VariableSubstitution, CommandSubstitution]
    }
    // Tcl's rows (`roles/tcl_family.rs`) plus iRules' own operators.
    // The comparison row adds the string and glob comparators
    // `starts_with`, `ends_with`, `contains`, `equals`, `matches`,
    // `matches_regex` and `matches_glob`; the short-circuit row adds the
    // word spellings `and` / `or`. Each is a kind of its own that the
    // grammar emits only inside a `binop_expr`.
    Comparison {
        owns [BinopExpr(holds_payer)]
        pays [
            ..tcl_family_comparison_payers!(),
            StartsWith, EndsWith, Contains, Equals, Matches, MatchesRegex, MatchesGlob,
        ]
        project "operator"
    }
    ShortCircuit {
        owns [BinopExpr(holds_payer)]
        pays [..tcl_family_short_circuit_payers!(), And, Or]
        project "operator"
    }
    EagerLogical { absent "Tcl's `&` / `|` inside `expr` are bitwise" }
    // A `ternary_expr` owns it and its `?` pays, ungated: the grammar
    // spells the bare token nowhere else. The node names no field, so
    // the row's payer fallback finds the `?`, and the ABC slot walker
    // reads the operands relative to the `?` / `:` markers (#1180).
    Ternary {
        owns [TernaryExpr]
        pays [QMARK]
        project "operator"
    }
    NullSafe { absent "Tcl has no optional navigation or null coalescing" }
    // Unlike Tcl, the grammar gives `switch` and its arms kinds of their
    // own: a `switch_arm` under its `switch` owns a decision and pays at
    // the node, unless its pattern is `default`, the fallback (lesson
    // 11, #106). The owner reads its parent, so it is never asked with
    // `Ancestors::unknown()`.
    CaseArm { owns [SwitchArm(text_gated_by is_decision_arm)] }
    Guard { absent "Tcl has no guard clause" }
    // iRules counts statement nodes, so every row here is plain (#1697);
    // unlike Tcl, the grammar gives `if` / `switch` / `for` / `foreach` /
    // `while` / `dict for` kinds of their own.
    Branch { owns [If] }
    Switch { owns [Switch] }
    // `dict for` iterates; the non-looping `dict update` / `dict with`
    // are deliberately not here.
    Loop { owns [For, Foreach, While, DictFor] }
    // `catch`, and a `try`'s `on error` / `trap` handler groups, which
    // the grammar wraps in dedicated nodes (unlike Tcl's flat tokens,
    // #1266); `try` itself and `finally` are unconditional.
    Catch { owns [Catch, OnHandler, TrapHandler] }
    // The grammar gives `elseif` a node of its own, a direct child of the
    // `if`, so the clause *is* the else-if and pays for itself.
    ElseIf { owns [Elseif(pays_itself)] pays [] }
    // `return` and `error` are generic `command`s told by the text of
    // the `name` field, which can be a `simple_word` or a `concat_word`
    // depending on context, so the kind is not checked; `::return` is
    // `return` through the global namespace. Tcl 8.6's `throw` is not
    // matched: TMOS iRules runs a Tcl 8.4-derived interpreter with no
    // such builtin. iRules flow commands (`event disable`, `reject`,
    // `drop`) stay uncounted (#1270).
    Exit { owns [Command(text_gated_by return_or_error)] }
    Modifier { absent "Tcl has no modifier keywords" }
    // Tcl's keywords (`Proc`, `Namespace2`, `Set2`, `Global2`) plus
    // `when`, which opens an event handler — the F5 construct the
    // dialect exists for.
    DeclKeyword { owns [Proc, When, Namespace2, Set2, Global2] }
    Primitive { absent "Tcl has no type names" }
    // A `proc` and a `when EVENT { … }` handler are both callables that
    // open a `Function` space.
    FuncOpener { owns [Procedure(callable opens Function), WhenEvent(callable opens Function)] }
    Container { owns [SourceFile(opens Unit)] }
    Closure { absent "Tcl has no anonymous function kind; `apply` takes a list" }
    Member { absent "Tcl has no container to hold a member" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Tcl documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
