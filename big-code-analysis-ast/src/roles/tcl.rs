//! Tcl's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — are declared; the rest are pending
//! the package that declares each (#1692).

use crate::lang_helpers::case_arms::{tcl_command_is_switch_arm, tcl_switch_arm_list};
use crate::lang_helpers::tcl::{TclCommandWord, tcl_command_word, tcl_try_handler_bodies};
use crate::role_table;
use crate::roles::tcl_family::{
    tcl_family_comparison_payers, tcl_family_short_circuit_payers, tcl_family_string_gates,
    tcl_family_strings,
};
use crate::{Ancestors, Node, Tcl};

/// Whether a `command` is a decision arm of a brace-list `switch` — a
/// child of the arm list of a command whose leading word is `switch`,
/// and not the `default` arm (lesson 11, #467). Reads the bytes and the
/// ancestor chain, since `switch` is a generic `command` with no kind
/// of its own (lesson 19).
fn is_switch_arm<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    tcl_command_is_switch_arm(node, code, ancestors)
}

tcl_family_string_gates!(Tcl, TclCode, crate::lang_helpers::tcl::BRACED_WORD_KINDS);

/// A `command` spelling `switch` whose sole braced argument is its arm
/// list — the form the arms can be read from.
fn is_brace_list_switch<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    tcl_command_word(node, code, ancestors) == TclCommandWord::Switch
        && tcl_switch_arm_list(node).is_some()
}

/// A `command` spelling `for`.
fn is_for_command<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    tcl_command_word(node, code, ancestors) == TclCommandWord::For
}

/// A `command` spelling one of the four abrupt exits.
fn is_exit_command<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    tcl_command_word(node, code, ancestors).is_exit()
}

/// A `try` with at least one `on error` / `trap` handler: an `arguments`
/// child followed by a body (`tcl_try_handler_bodies`).
fn has_handler<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    tcl_try_handler_bodies(node).next().is_some()
}

role_table! {
    lang: Tcl;
    // Every command is a call. The `CaseArm` row claims the same kind
    // for the `switch` arms a `command` can also be, told by position
    // and text.
    Call { owns [Command] }
    // The three word kinds (`roles/tcl_family.rs`), the braced word
    // gated on being a literal rather than a script body (#1381).
    StringLit {
        owns [..tcl_family_strings!()]
        wraps [VariableSubstitution, CommandSubstitution]
    }
    // A `binop_expr` names no operator field; its operator is one token
    // among its children, so each row's owner is gated on holding one
    // of the row's payers, and `project` — the first payer child — is
    // the lookup (`roles/tcl_family.rs` has the lists). A chained
    // `$a < $b && $c` is a left-nested pair of `binop_expr`s, each
    // owning its own operator.
    Comparison {
        owns [BinopExpr(holds_payer)]
        pays [..tcl_family_comparison_payers!()]
        project "operator"
    }
    ShortCircuit {
        owns [BinopExpr(holds_payer)]
        pays [..tcl_family_short_circuit_payers!()]
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
    // A `switch` is a generic `command` whose arms are the nested
    // `command`s of its sole braced argument (the split form has no arm
    // node and is not counted, #467); each arm owns a decision and pays
    // at the node, told from any other command by its position and its
    // parent's leading word, and the `default` arm pays nothing (lesson
    // 11). The owner reads the ancestor chain, so it is never asked with
    // `Ancestors::unknown()`.
    CaseArm { owns [Command(text_gated_by is_switch_arm)] }
    Guard { absent "Tcl has no guard clause" }
    // Tcl gives `if`, `elseif`, `foreach`, `while`, `catch` and `try`
    // kinds of their own and pays at the node; `switch`, `for` and the
    // exits are generic `command`s told by their leading word (#467,
    // #1264), read once per `command` per walk through
    // `tcl_command_word` (#1697).
    Branch { owns [If] }
    // A brace-list `switch` — the supported form, whose arms are the
    // `CaseArm` row's nested commands; the split form has no arm list
    // and owns nothing (#467), in cyclomatic's modified tier and
    // cognitive's nesting alike.
    Switch { owns [Command(text_gated_by is_brace_list_switch)] }
    Loop { owns [Foreach, While, Command(text_gated_by is_for_command)] }
    // `catch`, and a `try` whose handlers are the `on error` / `trap`
    // groups the grammar spells as flat `arguments` + body pairs with no
    // handler node (#1266): the `try` owns a decision only when it has
    // one, and the consumers count the handlers themselves.
    Catch { owns [Catch, Try(gated_by has_handler)] }
    // The grammar gives `elseif` a node of its own, a direct child of the
    // `if`, so the clause *is* the else-if and pays for itself.
    ElseIf { owns [Elseif(pays_itself)] pays [] }
    // `return`, `error`, the Tcl 8.6 `throw` and `exit`, each a generic
    // `command` told by its leading word; `error` in argument position
    // (`puts error`) is a `word_list` child and not a command (#1270).
    Exit { owns [Command(text_gated_by is_exit_command)] }
    Modifier { absent "Tcl has no modifier keywords" }
    // `proc` and `namespace` as keyword tokens (`Namespace2`; the
    // unsuffixed `Namespace` is the command node). `set` and `global`
    // declare a variable, as `local` does in Bash; `Set2` / `Global2`
    // are the keyword kinds.
    DeclKeyword { owns [Proc, Namespace2, Set2, Global2] }
    Primitive { absent "Tcl has no type names" }
    FuncOpener { owns [Procedure(callable opens Function)] }
    Closure { absent "Tcl has no anonymous function kind; `apply` takes a list" }
    Container { owns [SourceFile(opens Unit)] }
    Member { absent "Tcl has no container to hold a member" }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Tcl documents with comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
