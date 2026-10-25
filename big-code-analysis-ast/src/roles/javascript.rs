//! Javascript's role rows. The Halstead keyword roles (#1693) and the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — are
//! declared; the rest are pending the package that declares each (#1692).

use crate::Javascript;
use crate::role_table;
use crate::roles::js_family::{
    js_family_binding_gates, js_family_closures, js_family_comparison_payers, js_family_containers,
    js_family_decl_keywords, js_family_func_openers, js_family_modifiers,
    js_family_short_circuit_payers,
};

js_family_binding_gates!(Javascript, JavascriptCode, FieldDefinition);

role_table! {
    lang: Javascript;
    Call { pending R08 }
    StringLit { pending R08 }
    // The one `binary_expression` kind carries every binary operator;
    // its `operator` field says which, so each row's owner holds its
    // payer (`roles/js_family.rs` has the lists). `??` is `NullSafe`'s
    // and `&&=` / `||=` are assignments.
    Comparison {
        owns [BinaryExpression(holds_payer)]
        pays [..js_family_comparison_payers!()]
        project "operator"
    }
    ShortCircuit {
        owns [BinaryExpression(holds_payer)]
        pays [..js_family_short_circuit_payers!()]
        project "operator"
    }
    EagerLogical { absent "JavaScript's `&` / `|` / `^` are bitwise" }
    // The `ternary_expression` owns it and its `?` pays, ungated: the
    // JavaScript grammars spell the bare token nowhere else, and a `?`
    // error recovery strands under an `ERROR` (`let c = a ?` at end of
    // input) has always paid (the TypeScript grammars gate theirs, for
    // the ten type productions `roles/js_family.rs` names).
    Ternary {
        owns [TernaryExpression]
        pays [QMARK]
        project "operator"
    }
    // Two structural forms, each one decision per token (#281, #248):
    // the null coalescing `a ?? b`, a `binary_expression` owned through
    // its `operator` field, and the optional chain `a?.b` / `a?.[0]` /
    // `a?.()`, whose `?.` the JavaScript grammars emit as the bare
    // `optional_chain` token (no wrapper) — the TypeScript grammars wrap
    // it, and their rows say so. Which consumer reads which: cyclomatic
    // pays both tokens; cognitive folds `??` into a boolean sequence
    // and reads `?.` not at all; ABC pays `??` as a condition and `?.`
    // not at all (its `?.` is no boolean test). The compound `??=` is
    // an assignment to ABC, a sequence to cognitive and a decision to
    // cyclomatic (#236, #248): it stays the consumers' own, recorded
    // here as considered.
    NullSafe {
        owns [BinaryExpression(holds_payer)]
        pays [QMARKQMARK, OptionalChain]
        project "operator"
        excludes [QMARKQMARKEQ "the compound `??=`, an assignment ABC pays as one and a decision the others pay as their own (#236, #248)"]
    }
    // A `switch_case` owns a decision and its `case` keyword pays; the
    // `switch_default` and its `default` keyword are the fallback and
    // pay nothing (lesson 11, #469).
    CaseArm {
        owns [SwitchCase(holds_payer)]
        pays [Case]
        excludes [
            SwitchDefault "the fallback arm, no decision (lesson 11, #469)",
            Default "the fallback label's keyword",
        ]
    }
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
    // The shared JS-family openers, closures and containers
    // (`roles/js_family.rs`); the binding gates are emitted above, with
    // this grammar's class-field kind.
    FuncOpener { owns [..js_family_func_openers!()] }
    Closure { owns [..js_family_closures!()] }
    Container { owns [..js_family_containers!()] }
    // A class field (`x = v;`); one whose initialiser is a function is
    // a method written as a field, which the consumers tell apart
    // themselves — the row recognises the declaration, the consumer
    // reads its value.
    Member { owns [FieldDefinition] }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "JavaScript documents with JSDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
