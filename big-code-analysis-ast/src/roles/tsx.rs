//! Tsx's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::Tsx;
use crate::role_table;
use crate::roles::js_family::{
    js_family_comparison_payers, js_family_decl_keywords, js_family_modifiers,
    js_family_operator_gates, js_family_short_circuit_payers, ts_family_decl_keywords,
    ts_family_gates, ts_family_modifiers,
};

ts_family_gates!(Tsx);
js_family_operator_gates!(Tsx);

role_table! {
    lang: Tsx;
    Call { pending R08 }
    StringLit { pending R08 }
    // The one `binary_expression` kind carries every binary operator;
    // its `operator` field says which, so each row's owner is gated on
    // the field holding one of its payers (`roles/js_family.rs` has the
    // lists). `??` is `NullSafe`'s and `&&=` / `||=` are assignments.
    Comparison {
        owns [BinaryExpression(gated_by comparison_operator)]
        pays [..js_family_comparison_payers!()]
        project "operator"
    }
    ShortCircuit {
        owns [BinaryExpression(gated_by short_circuit_operator)]
        pays [..js_family_short_circuit_payers!()]
        project "operator"
    }
    EagerLogical { absent "TSX's `&` / `|` / `^` are bitwise" }
    // The `ternary_expression` owns it and its `?` pays, gated to that
    // owner (`roles/js_family.rs` says which other productions spell
    // the token).
    Ternary {
        owns [TernaryExpression]
        pays [QMARK gated_by under_ternary]
        project "operator"
    }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { absent "TSX has no guard clause" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // TypeScript's keyword rows through the shared JS-family and
    // TS-family fragments (`roles/js_family.rs`), with TSX's own
    // `String3` for the `: string` keyword below; the derivations are
    // on the TypeScript rows.
    Modifier { owns [..js_family_modifiers!(), ..ts_family_modifiers!()] }
    DeclKeyword { owns [..js_family_decl_keywords!(), ..ts_family_decl_keywords!()] }
    // `predefined_type` is the TS type-keyword set (`string`, `number`,
    // `boolean`, …), one kind for every spelling, so
    // `Checker::is_primitive` keys it by text; the gate leaves `void`
    // to its inner token (#453). The keyword children of the wrapper
    // are excluded: the wrapper is the one operator, and listing a
    // child too billed one source token twice (#1261).
    Primitive {
        owns [PredefinedType(gated_by names_a_type_not_void)]
        excludes [
            Any "the `predefined_type` wrapper is the operator; its keyword child is the same token",
            Number2 "the `predefined_type` wrapper is the operator; its keyword child is the same token",
            Boolean "the `predefined_type` wrapper is the operator; its keyword child is the same token",
            String3 "the `: string` keyword is the wrapper's child, billed once through it (#1261)",
            Symbol "the `predefined_type` wrapper is the operator; its keyword child is the same token",
            Object2 "the `predefined_type` wrapper is the operator; its keyword child is the same token",
            Unknown "the `predefined_type` wrapper is the operator; its keyword child is the same token",
            Never "the `predefined_type` wrapper is the operator; its keyword child is the same token",
        ]
    }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "TSX documents with JSDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
