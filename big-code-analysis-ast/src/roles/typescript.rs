//! Typescript's role rows. The Halstead keyword roles are declared (#1693);
//! every other role is pending the package that declares it (#1692).

use crate::Typescript;
use crate::role_table;
use crate::roles::js_family::{
    js_family_comparison_payers, js_family_decl_keywords, js_family_modifiers,
    js_family_operator_gates, js_family_short_circuit_payers, ts_family_decl_keywords,
    ts_family_gates, ts_family_modifiers,
};

ts_family_gates!(Typescript);
js_family_operator_gates!(Typescript);

role_table! {
    lang: Typescript;
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
    EagerLogical { absent "TypeScript's `&` / `|` / `^` are bitwise" }
    Ternary { pending R05 }
    NullSafe { pending R05 }
    CaseArm { pending R05 }
    Guard { absent "TypeScript has no guard clause" }
    Branch { pending R07 }
    Switch { pending R07 }
    Loop { pending R07 }
    Catch { pending R07 }
    ElseIf { pending R07 }
    Exit { pending R07 }
    // The shared JS-family `static` (`roles/js_family.rs`) plus the
    // TS-family member modifiers — the fragment TSX splices too — that
    // #1563 bills as C# and Kotlin bill theirs:
    // `public`, `private`, `protected`, `override`, `readonly`,
    // `abstract` and `accessor`, and `declare` (#1557). Each is its
    // keyword leaf, one kind apiece. `public` / `private` / `protected`
    // sit under an `accessibility_modifier` and `override` under an
    // `override_modifier`; both wrappers stay unlisted, so a modifier
    // bills once. The same leaves bill outside a class too: a parameter
    // property (`constructor(private x)`), `readonly T[]`, a mapped
    // type's `-readonly`, `abstract class` and `abstract new () => T`.
    // Used as a name (`let readonly = 1`, `o.public`, `abstract()` in an
    // expression, a field named `static`) the grammar emits an
    // identifier. The pinned grammar misreads a few valid name
    // spellings as the keyword under or beside an ERROR — a statement
    // `abstract();`, a field `abstract;` or `accessor = 1`, and an
    // object-type member `abstract: T` — so those bill the operator.
    // They billed nothing before, and none occurs in the corpora.
    Modifier { owns [..js_family_modifiers!(), ..ts_family_modifiers!()] }
    // The shared JS-family keywords plus the TS-family fragment TSX
    // splices too: `interface` and `enum`, the
    // type-declaration leaves billed as the shared `class` is (#1552)
    // and as C# and Java bill theirs, and `type`, `namespace`, `module`
    // and `global` (#1557); every `*_declaration` wrapper stays
    // unlisted. `Module2` is the `module` keyword leaf; the unsuffixed
    // `Module` is the declaration node that wraps it. Each word is
    // contextual, but used as a name (`let type = 1`, `module.exports`)
    // the grammar emits an `identifier`, never the keyword leaf — except
    // a binding named `type` in an export clause, which is what the
    // gate on `Type` reads. `type` also bills in `import type` /
    // `export type`, where it is the same leaf.
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
            String2 "the `: string` keyword is the wrapper's child, billed once through it (#1261)",
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
    Docstring { absent "TypeScript documents with JSDoc comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
