//! iRules' role rows; declared through the Tcl template once R06 lands
//! (contract 5 of #1690). The Halstead keyword role is declared (#1693).

use crate::lang_helpers::case_arms::irules_switch_arm_is_decision;
use crate::role_table;
use crate::roles::tcl_family::{tcl_family_comparison_payers, tcl_family_short_circuit_payers};
use crate::{Ancestors, Irules, Node};

/// Whether a `switch_arm` is a decision arm rather than `default`,
/// which only its `pattern` bytes tell (lesson 11, #106).
fn is_decision_arm<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    irules_switch_arm_is_decision(node, code, ancestors)
}

role_table! {
    lang: Irules;
    Call { pending R08 }
    StringLit { pending R08 }
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
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    Modifier { absent "Tcl has no modifier keywords" }
    // Tcl's keywords (`Proc`, `Namespace2`, `Set2`, `Global2`) plus
    // `when`, which opens an event handler — the F5 construct the
    // dialect exists for.
    DeclKeyword { owns [Proc, When, Namespace2, Set2, Global2] }
    Primitive { absent "Tcl has no type names" }
    FuncOpener { pending R06 }
    Closure { absent "Tcl has no anonymous function kind; `apply` takes a list" }
    Container { pending R06 }
    Member { absent "Tcl has no container to hold a member" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Tcl documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
