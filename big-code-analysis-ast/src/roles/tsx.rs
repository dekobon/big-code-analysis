//! Tsx's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — and
//! the documentation rows (#1699) are declared.

use crate::Tsx;
use crate::role_table;
use crate::roles::control_flow::else_if_parent_clause_gate;
use crate::roles::js_family::{
    js_family_binding_gates, js_family_closures, js_family_comparison_payers, js_family_containers,
    js_family_decl_keywords, js_family_doc_gates, js_family_doc_targets, js_family_exits,
    js_family_func_openers, js_family_loops, js_family_modifiers, js_family_short_circuit_payers,
    js_family_strings, ts_doc_gates, ts_family_decl_keywords, ts_family_doc_targets,
    ts_family_gates, ts_family_modifiers,
};

ts_family_gates!(Tsx);
// The chained `if` sits directly inside an `else_clause`; a nested one
// under `else { … }` sits inside the statement block (#1697).
else_if_parent_clause_gate!(Tsx, ElseClause);
js_family_binding_gates!(Tsx, TsxCode, PublicFieldDefinition);

// The documentation gates, per dialect (#1699): tsc reads `/***` as a doc block; TSX follows tsc, as TypeScript does.
js_family_doc_gates!(Tsx, tsdoc_block);
ts_doc_gates!(Tsx, tsx);

role_table! {
    lang: Tsx;
    // As TypeScript's: four ids, one emitted, three defensive.
    Call { owns [CallExpression, CallExpression2, CallExpression3, CallExpression4] }
    // TSX's `String2` (kind id 261) is its string-*literal* alias —
    // ordinary and JSX-attribute literals both parse as it — so it is a
    // string (#283). `String3` (kind id 141) is the `: string`
    // type-annotation keyword (the role TS's `String2` plays), excluded
    // as there (#1261).
    StringLit {
        owns [..js_family_strings!(), String2]
        wraps [TemplateSubstitution]
        excludes [String3 "the `: string` type keyword, not a literal (#1261)"]
    }
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
    EagerLogical { absent "TSX's `&` / `|` / `^` are bitwise" }
    // The `ternary_expression` owns it and its `?` pays, gated to that
    // owner: the TypeScript grammars spell the bare token in ten type
    // productions — an optional parameter, property, method or tuple
    // member, `optional_type`, `flow_maybe_type`, `conditional_type`
    // (`T extends U ? X : Y`, resolved by the type checker and erased
    // before runtime) — none a decision (#1275). `?.` and `??` are
    // distinct tokens.
    Ternary {
        owns [TernaryExpression]
        pays [QMARK under TernaryExpression]
        project "operator"
    }
    // Two structural forms, each one decision per token (#281, #248):
    // the null coalescing `a ?? b`, a `binary_expression` owned through
    // its `operator` field, and the optional chain `a?.b` / `a?.[0]` /
    // `a?.()`, whose `?.` the TypeScript grammars spell as a `?.` token
    // inside an `optional_chain` wrapper — the wrapper is excluded so a
    // chain pays once, at the token (the JavaScript grammars have no
    // wrapper; their `optional_chain` *is* the token). Which consumer
    // reads which: cyclomatic pays both tokens; cognitive folds `??`
    // into a boolean sequence and reads `?.` not at all; ABC pays `??`
    // as a condition and `?.` not at all. The compound `??=` is an
    // assignment to ABC, a sequence to cognitive and a decision to
    // cyclomatic (#236, #248): the consumers' own, recorded here.
    NullSafe {
        owns [BinaryExpression(holds_payer)]
        pays [QMARKQMARK, QMARKDOT]
        project "operator"
        excludes [
            OptionalChain "the wrapper around the `?.` token, which pays once",
            QMARKQMARKEQ "the compound `??=`, an assignment ABC pays as one and a decision the others pay as their own (#236, #248)",
        ]
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
    Guard { absent "TSX has no guard clause" }
    // The JS family's control flow (#1697): the one-kind rows are spelled
    // inline, the loop and exit lists are fragments of
    // `roles/js_family.rs`, and no dialect has an extra — the four
    // grammars share the shape and differ only in kind ids. The `ElseIf`
    // gate is emitted beside this table.
    Branch { owns [IfStatement] pays [If] }
    Switch { owns [SwitchStatement] }
    Loop { owns [..js_family_loops!()] pays [For, While] }
    Catch { owns [CatchClause] pays [Catch] }
    ElseIf { owns [IfStatement(gated_by continues_else_clause)] pays [] }
    Exit { owns [..js_family_exits!()] }
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
    // The shared JS-family openers, closures and containers
    // (`roles/js_family.rs`); the binding gates are emitted above, with
    // this grammar's class-field kind.
    FuncOpener { owns [..js_family_func_openers!()] }
    Closure { owns [..js_family_closures!()] }
    // An abstract class is its own kind in the TS grammars (`Class`);
    // an interface opens `Interface`.
    Container {
        owns [
            ..js_family_containers!(),
            AbstractClassDeclaration(opens Class),
            InterfaceDeclaration(opens Interface),
        ]
    }
    // A class field (`x: T = v;`, any accessibility) and an interface's
    // property signature; an index or method signature is the method
    // consumer's. A field whose initialiser is a function is a method
    // written as a field, which the consumers tell apart themselves —
    // the row recognises the declaration, the consumer reads its value.
    Member { owns [PublicFieldDefinition, PropertySignature] }
    // The two comment kinds — a `/* */` or `//` `comment` and the Annex-B
    // `<!-- -->` `html_comment` (#697); only the first can open with the
    // TSDoc block marker (#1699).
    Comment { owns [Comment, HtmlComment] }
    DocComment { owns [Comment(text_gated_by opens_with_doc_block_marker)] }
    Docstring { absent "TypeScript documents with TSDoc comments, not string literals" }
    // What tsc and TypeDoc document: JSDoc's declarations without its
    // assignment statements (which tsc does not declare), plus
    // TypeScript's own — the `ts_family_doc_targets!` fragment. An
    // enum member without an initializer is a bare name under the
    // `enum_body`: an identifier or a computed name on the fragment,
    // and the `string` kind here, which TS and TSX number differently.
    // tsc reads a declaration's comments before its first token only,
    // so a declaration *under* an `export_statement` or
    // `ambient_declaration` documents nothing on its own — parent
    // context the Checker composes, not a gate here; the row answers
    // the candidate. Most targets are outside the space-tree rows —
    // the parity exceptions `roles/doc_tests/parity.rs` records.
    DocTarget {
        owns [
            ..js_family_doc_targets!(),
            ..ts_family_doc_targets!(),
            String2(gated_by under_enum_body),
        ]
    }
    DocRunOpener { absent "TSDoc attaches every doc comment to a declaration or the module; none documents by its own content" }
    DocRunDirective { absent "TSDoc reads no directive comment" }
    HoldsDocInBody { absent "only the callables and closures hold a doc comment in a body; the body-child test is the Checker's" }
}
