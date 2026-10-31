//! `Getter` implementation for Rust.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

// `for_lifetimes` (`for<'a> fn(&'a u8)`) and `use_bounds`
// (`impl Tr + use<'a, T>`) are generic-parameter lists under other
// names, and a qualified path's `bracketed_type` (`<T as Tr>::f`,
// `<T>::f`) encloses a type (#1567). `LT2` is the
// `token(prec(1, '<'))` spelling `type_arguments` and `use_bounds` open
// with; the parser reports it as `LT`, so it is listed defensively,
// here and in the operator arm, and its absence is pinned by
// `rust_generic_opener_alias_never_reaches_kind_id`.
const GENERIC_ANGLES: GenericAngleKinds = GenericAngleKinds {
    lists: &[
        Rust::TypeArguments as u16,
        Rust::TypeParameters as u16,
        Rust::ForLifetimes as u16,
        Rust::UseBounds as u16,
        Rust::BracketedType as u16,
    ],
    openers: &[Rust::LT as u16, Rust::LT2 as u16],
    closers: &[Rust::GT as u16],
    is_misparse: never_misparsed,
};

impl Getter for RustCode {
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        _ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // we're in a function or in a class or an impl
        // for an impl: we've  'impl ... type {...'
        if let Some(name) = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("type"))
        {
            node_text(code, &name)
        } else {
            Some("<anonymous>")
        }
    }

    // The mapping is the `opens` annotations of `roles/rust.rs`
    // (#1696). The rows read no bytes and no chain, so the byte-less
    // spelling forwards with none.
    fn get_space_kind(node: &Node) -> SpaceKind {
        Self::get_space_kind_with_code(node, &[], Ancestors::known(&[]))
    }

    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::rust::opens_space(node, code, ancestors)
    }

    keyword_roles!(rust);

    /// The `Comparison` and `ShortCircuit` rows' payers bill as
    /// operators (#1695), except a `||` outside the owner the row
    /// projects it from: inside a macro's `token_tree` the parser sees
    /// no expression, and a `|| { … }` there is a closure with no
    /// parameters, whose `||` the grammar emits as this token rather
    /// than as `closure_parameters`. The counting consumers pay that
    /// token — it was always a decision to them — so the row's payer is
    /// ungated and this Halstead reading is the one place the two
    /// policies part. `&&` has no such second spelling.
    // role-bypass: allow(ShortCircuit) — the Halstead reading of a payer the row pays ungated: which
    // parent makes `||` an operator rather than an empty closure's bars
    fn get_paying_role<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> TokenRole {
        use crate::roles::rust::{branch, comparison, r#loop, short_circuit};
        let kind = node.kind_id();
        if kind == Rust::PIPEPIPE as u16
            && !ancestors
                .parent(node)
                .is_some_and(|p| short_circuit::owns(&p, code, ancestors))
        {
            return TokenRole::Unknown;
        }
        // And the `Branch` / `Loop` rows' keyword payers (#1697): `if`,
        // `for`, `while`, `loop`. A `for` in type syntax is declined by
        // its row's gate (#1720) and falls through to the byte-less arm,
        // which keeps `For`, so every `for` — the loop's, a macro's token
        // tree's, the `impl` header's — bills as the operator it always did.
        if comparison::pays_at_token_kind(kind, node, code, ancestors)
            || short_circuit::pays_at_token_kind(kind, node, code, ancestors)
            || branch::pays_at_token_kind(kind, node, code, ancestors)
            || r#loop::pays_at_token_kind(kind, node, code, ancestors)
        {
            TokenRole::Operator
        } else {
            TokenRole::Unknown
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Rust::*;

        if GENERIC_ANGLES.is_closer(node, ancestors) {
            return TokenRole::Unknown;
        }
        match node.kind_id().into() {
            // Exclude `/` when it corresponds to the third slash in `///`
            // (`OuterDocCommentMarker`). `||` is the `ShortCircuit` row's
            // and `get_paying_role` decides it (#1695).
            // role-bypass: allow(Comparison, ShortCircuit) — the `/` of `///` against a binary
            // expression's `/`: a doc-marker test, not membership
            SLASH => match ancestors.parent(node) {
                Some(parent) if matches!(parent.kind_id().into(), BinaryExpression) => {
                    TokenRole::Operator
                }
                _ => TokenRole::Unknown,
            },
            // Ensure `!` is counted as an operator unless it belongs to an `InnerDocCommentMarker` `//!`
            BANG => match ancestors.parent(node) {
                Some(parent) if !matches!(parent.kind_id().into(), InnerDocCommentMarker) => {
                    TokenRole::Operator
                }
                _ => TokenRole::Unknown,
            },
            // COLONCOLON (`::`) is the path-segment separator. C++, Java,
            // C#, and Kotlin all classify it as an operator; omitting it
            // here (issue #394) silently dropped every path expression
            // (`std::collections::HashMap`, `Vec::new`, `T::method`) into
            // TokenRole::Unknown, deflating n1/N1 for path-heavy code.
            //
            // The declaration and visibility keywords and the primitive
            // types are not here: `roles/rust.rs` declares them and
            // `get_keyword_role` bills them (#1693; #394 added them). Nor
            // are `==` `!=` `<=` `>=` `&&`: the `Comparison` and
            // `ShortCircuit` rows pay them and `get_paying_role` bills
            // them (#1695). `<` / `>` stay for the spellings the row's
            // parent gate leaves out — a generic opener (`Vec<T>`, the
            // `<>` pair) and a recovered token — which billed before the
            // row existed; `LT2` is the defensive generic-opener alias.
            // `if` / `for` / `while` / `loop` are the `Branch` / `Loop`
            // rows' payers and `get_paying_role` bills them (#1697) — the
            // `if` expression's keyword and a match guard's alike, so the
            // `Guard` row's `under MatchPattern` gate no longer leaves a
            // spelling to this arm. `for` stays: the `Loop` row's payer
            // gate declines the type-syntax spellings (`impl T for S`,
            // `for<'a>`) so cyclomatic does not count them (#1720), while
            // Halstead bills every `for` as it always has — the loop's
            // reaches `get_paying_role` first and never gets here, as
            // Python's ternary `if` does.
            // role-bypass: allow(Comparison, Loop) — the operator classification of the
            // spellings the `Comparison` row's `under BinaryExpression` gate and the `Loop`
            // row's `not_in_type_syntax` gate leave out
            LT | LT2 | GT | LPAREN | LBRACE | LBRACK | As | EQGT | PLUS | STAR | Await | Break
            | Continue | Else | For | In | Match | Return | EQ | COMMA | DASHGT | QMARK | AMP
            | DOTDOT | DOTDOTEQ | DASH | PIPE | CARET | LTLT | GTGT | PERCENT | PLUSEQ | DASHEQ
            | STAREQ | SLASHEQ | PERCENTEQ | AMPEQ | PIPEEQ | CARETEQ | LTLTEQ | GTGTEQ | DOT
            | SEMI | COLON | COLONCOLON => TokenRole::Operator,
            // FieldIdentifier (e.g. `p.x`) and TypeIdentifier (e.g. `Vec`,
            // `HashMap`) are operand-class names — C++ and Go classify them
            // the same way (see arms ~588 and ~862 below). Omitting them
            // here silently dropped both into TokenRole::Unknown,
            // deflating n2/N2 and the derived vocabulary/volume/effort
            // estimates (issue #390).
            // role-bypass: allow(StringLit) — the operand classification of the literals,
            // a projection the `StringLit` row does not answer
            Identifier | TypeIdentifier | FieldIdentifier | StringLiteral | RawStringLiteral
            | IntegerLiteral | FloatLiteral | BooleanLiteral | Zelf | CharLiteral | UNDERSCORE => {
                TokenRole::Operand
            }
            _ => TokenRole::Unknown,
        }
    }

    fn get_operator_spelling<'a>(
        node: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
    ) -> Option<&'static str> {
        GENERIC_ANGLES.opener_spelling(node, ancestors)
    }

    get_operator!(Rust);
}
