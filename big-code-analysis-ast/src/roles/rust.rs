//! Rust's role rows. The Halstead keyword roles (#1693), the
//! paying-construct roles — `Comparison`, `ShortCircuit`, `EagerLogical`,
//! `Ternary`, `CaseArm`, `Guard`, `NullSafe` (#1694, #1695) — the space
//! tree (#1696) and the control flow — `Branch`, `Switch`, `Loop`,
//! `Catch`, `ElseIf`, `Exit` (#1697) — are declared; the rest are pending
//! the package that declares each (#1692).

use crate::lang_helpers::case_arms::pattern_is_bare_underscore;
use crate::role_table;
use crate::roles::control_flow::else_if_parent_clause_gate;
use crate::{Ancestors, Node, Rust};

// The chained `if` sits directly inside an `else_clause`; a nested one
// under `else { … }` sits inside the block (#1697).
else_if_parent_clause_gate!(Rust, ElseClause);

/// Whether a `match_arm` is a decision: its `pattern` is not a bare `_`
/// (lesson 11, #469). `Some(_)`, `(_, x)` and `_ if g` are not bare and
/// still count; a leading `|` of an or-pattern is skipped. The field is
/// required at the pinned grammar, so an arm without one — error
/// recovery only — counts, the polarity both consumers had (#1454).
fn is_decision_arm<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    !node
        .child_by_field_name("pattern")
        .is_some_and(|pat| pattern_is_bare_underscore(&pat, Rust::UNDERSCORE as u16))
}

/// Whether a `let_declaration` is `let PAT = e else { … }`: it carries
/// the `alternative` field. Read by field, not by the `else` token the
/// `if` expression also emits.
fn has_alternative<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    node.child_by_field_name("alternative").is_some()
}

/// Whether a `for` keyword is a loop's and not type syntax (#1720): the
/// token's parent is not an `impl_item` (`impl T for S`), a
/// `higher_ranked_trait_bound` (`F: for<'a> Fn(&'a u8)`, in a generic
/// list or a `where` clause), a `for_lifetimes` (`for<'a> fn(&'a u8)`)
/// or an `abstract_type` (`impl for<'a> Fn(&'a u8)` in argument or
/// return position, where the grammar puts the token directly under the
/// `impl` type rather than under a bound). Read off the chain, so
/// `O(1)`. A `for` inside a macro's `token_tree` / `token_repetition`
/// has the tree as its parent and stays paid — a policy about opaque
/// macro bodies that applies to `if` / `while` / `loop` there too and
/// is pinned separately (`rust_macro_token_tree_keywords_score`).
fn not_in_type_syntax<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    !ancestors.parent(node).is_some_and(|parent| {
        matches!(
            parent.kind_id().into(),
            Rust::ImplItem | Rust::HigherRankedTraitBound | Rust::ForLifetimes | Rust::AbstractType
        )
    })
}

/// Whether a `match_pattern` carries a guard: its `condition` field.
fn has_guard<'a>(node: &Node<'a>, _code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
    node.child_by_field_name("condition").is_some()
}

role_table! {
    lang: Rust;
    // A `macro_invocation` is not a call to this row, as before it.
    Call { owns [CallExpression] }
    // A `char_literal` is not a string, though the dump flattens it
    // beside the two (#391).
    StringLit { owns [StringLiteral, RawStringLiteral] }
    // The one `binary_expression` kind carries every binary operator;
    // its `operator` field says which, so the owner is gated on the
    // field and `<` / `>` on the parent: a generic list and a turbofish
    // spell the same tokens. The four other tokens occur nowhere else.
    Comparison {
        owns [BinaryExpression(holds_payer)]
        pays [
            EQEQ, BANGEQ, LTEQ, GTEQ,
            LT under BinaryExpression, GT under BinaryExpression,
        ]
        project "operator"
    }
    // A `binary_expression` whose operator is `&&` / `||`, and a Rust
    // 2024 let-chain (`if let Some(x) = a && b`) — `LetChain` the
    // visible alias, `LetChain2` the hidden `_let_chain` supertype —
    // whose `&&` tokens are its direct children, so the chain owns them
    // with no field to project through and the row's payer fallback
    // finds the first. The tokens pay ungated: the grammar spells them
    // bare only inside a macro's `token_tree` (`m!(a || b)`), where the
    // parser sees no expression and the counting consumers have always
    // paid them (an empty closure's `||` is two `|` tokens under
    // `closure_parameters`, not this kind).
    ShortCircuit {
        owns [
            BinaryExpression(holds_payer),
            LetChain, LetChain2,
        ]
        pays [AMPAMP, PIPEPIPE]
        project "operator"
    }
    // `&` / `|` / `^` on boolean operands evaluate both sides, but the
    // grammar emits them as the same `binary_expression` as the integral
    // forms, so the syntax alone cannot tell them apart; no consumer
    // treats them as a decision, and the three tokens bill as the
    // operators they are. A recognition boundary, recorded as the
    // kinds considered rather than as an absence.
    EagerLogical {
        owns []
        excludes [
            AMP "a boolean `&` is the integral `&`'s kind, which the grammar does not tell apart",
            PIPE "a boolean `|` is the integral `|`'s kind, which the grammar does not tell apart",
            CARET "a boolean `^` is the integral `^`'s kind, which the grammar does not tell apart",
        ]
    }
    Ternary { absent "Rust has no ternary operator; `if` is an expression" }
    NullSafe { absent "Rust has no optional navigation or null coalescing; `?` is an exit" }
    // A `match_arm` (`MatchArm2` the second id carrying the kind, per
    // grammar-dispatch §1) owns a decision and pays at the node — its
    // `=>` is punctuation — unless its pattern is a bare `_`, the
    // C-family `default:` (lesson 11, #469).
    CaseArm { owns [MatchArm(gated_by is_decision_arm), MatchArm2(gated_by is_decision_arm)] }
    // A match guard is the `match_pattern`'s `condition` field, opened
    // by an `if` keyword the grammar shares with the `if` expression;
    // the pattern owns it where the field is present and the keyword
    // pays, gated to the pattern. Cyclomatic counts that keyword
    // wherever it is written, so the guard pays as an `if` does; ABC
    // walks the field as a boolean slot.
    Guard {
        owns [MatchPattern(gated_by has_guard)]
        pays [If under MatchPattern]
    }
    // Rust counts keyword tokens (#284), so each row below pays at its
    // keyword. The `if` keyword is one kind for the `if` expression,
    // `if let`, and a match guard (the `Guard` row's payer, gated to the
    // pattern); cyclomatic counts it once wherever it is written, so the
    // two rows share the token and a consumer reads them as one. A
    // `let … else` either binds or diverges: one decision, as its `if
    // let` twin pays, told by the `alternative` field rather than the
    // `else` token the `if` expression also emits; it has no keyword of
    // its own to pay at, so the node pays (`pays_itself`).
    Branch {
        owns [IfExpression, LetDeclaration(pays_itself gated_by has_alternative)]
        pays [If]
    }
    // The `match` expression, the modified tier's container; its arms
    // are the `CaseArm` row's.
    Switch { owns [MatchExpression] }
    // `for`, `while` (and `while let`, the same kind) and `loop`, each
    // paid at its keyword. The `for` keyword is also type syntax —
    // `impl T for S`, `for<'a> Fn(…)`, `for<'a> fn(…)`, `impl for<'a>
    // Fn(…)` — which decides nothing and is declined by the payer's gate
    // (#1720); a `for` inside a macro's `token_tree` pays and stays paid.
    Loop {
        owns [ForExpression, WhileExpression, LoopExpression]
        pays [For gated_by not_in_type_syntax, While, Loop]
    }
    Catch { absent "Rust has no exception handling; `?` is an exit and `match` on a `Result` a switch" }
    // `else if` is an `if_expression` directly inside an `else_clause`,
    // as the C family spells it; a nested `if` under `else { … }` sits
    // inside the block. Recognition only: the chained `if`'s keyword is
    // `Branch`'s and the `else` token pays cognitive's +1.
    ElseIf { owns [IfExpression(gated_by continues_else_clause)] pays [] }
    // Explicit `return`, and `?`, which returns the error early (#243:
    // the implicit final expression is not an exit). `?` is also what
    // cyclomatic's `count_try` toggles; the row says it is an exit,
    // the option says whether it is a decision.
    Exit { owns [ReturnExpression, TryExpression] }
    // `pub` is the leaf of an unlisted `visibility_modifier`, so it
    // bills once whatever the `(crate)` / `(super)` qualifier; `mut` is
    // the grammar's `mutable_specifier` node, one kind for a binding,
    // a reference and a `self` receiver. `const` and `static` are each
    // one kind for the item and the qualifier (`const fn`, `static
    // ref`), as `unsafe`, `async`, `extern` and `move` are for theirs;
    // all bill once per spelling.
    Modifier {
        owns [Pub, MutableSpecifier, Const, Static, Unsafe, Async, Extern, Move, Dyn]
    }
    // The item keywords (#394): each the leaf of an unlisted `*_item`
    // / `let_declaration` / `where_clause` wrapper.
    DeclKeyword { owns [Fn, Let, Enum, Struct, Union, Trait, Impl, Use, Mod, Type, Where] }
    // tree-sitter-rust spells `primitive_type` as
    // `alias(choice(u8, i8, …, char), $.primitive_type)`: one generated
    // id per spelling, seventeen in all, every one named
    // `primitive_type`. The parser canonicalises them, so written source
    // reaches `kind_id()` as the base `PrimitiveType` and the sixteen
    // suffixed ids are defensive (grammar-dispatch section 1;
    // `rust_aliased_primitive_type_classification` records which is
    // observed). `Checker::is_primitive` keys each by text.
    Primitive {
        owns [
            PrimitiveType, PrimitiveType2, PrimitiveType3, PrimitiveType4, PrimitiveType5,
            PrimitiveType6, PrimitiveType7, PrimitiveType8, PrimitiveType9, PrimitiveType10,
            PrimitiveType11, PrimitiveType12, PrimitiveType13, PrimitiveType14, PrimitiveType15,
            PrimitiveType16, PrimitiveType17,
        ]
    }
    // A `fn` item opens a `Function` space and is the one callable; a
    // closure opens one too and is the `Closure` (Rust's closures open
    // spaces where Java's and Python's do not — the book's "which
    // lambdas open a space" paragraph is this row). `impl` and `trait`
    // blocks are the class-like containers; a `struct_item` is not a
    // space, so its fields land on the enclosing one (`npa/rust.rs`).
    FuncOpener { owns [FunctionItem(callable opens Function), ClosureExpression(opens Function)] }
    Closure { owns [ClosureExpression] }
    Container { owns [SourceFile(opens Unit), ImplItem(opens Impl), TraitItem(opens Trait)] }
    // A struct's named field, an `impl` or `trait` body's associated
    // `const` / `static`, and a trait's associated `type`. Which body a
    // member sits in, its visibility, and a tuple struct's positional
    // fields — which are type nodes, not declarations — stay with the
    // attribute consumer.
    Member { owns [FieldDeclaration, ConstItem, StaticItem, AssociatedType] }
    Comment { pending R09 }
    DocComment { pending R09 }
    Docstring { absent "Rust documents with `///` comments, not string literals" }
    DocTarget { pending R09 }
    DocRunOpener { pending R09 }
    DocRunDirective { pending R09 }
    HoldsDocInBody { pending R09 }
}
