//! Mozjs's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::Mozjs;
use crate::role_table;
use crate::roles::js_family::{
    js_family_comparison_payers, js_family_decl_keywords, js_family_modifiers,
    js_family_operator_gates, js_family_short_circuit_payers,
};

js_family_operator_gates!(Mozjs);

role_table! {
    lang: Mozjs;
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
    EagerLogical { absent "JavaScript's `&` / `|` / `^` are bitwise" }
    Ternary { pending R05 }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { absent "JavaScript has no guard clause" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // The shared JS-family `static` (`roles/js_family.rs`) plus
    // `Staticget`, one token spelling `static get` followed by a
    // newline — the static getter whose name starts the next line —
    // which these two grammars alone have. It is billed beside `static`
    // (#1563) so that form bills a modifier rather than nothing; being
    // one node it is one operator.
    Modifier { owns [..js_family_modifiers!(), Staticget] }
    DeclKeyword { owns [..js_family_decl_keywords!()] }
    Primitive { absent "JavaScript has no type names" }
    FuncOpener { pending R06 }
    Closure { pending R06 }
    Container { pending R06 }
    Member { pending R06 }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "JavaScript documents with JSDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
