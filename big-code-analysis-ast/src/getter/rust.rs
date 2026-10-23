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

    fn get_space_kind(node: &Node) -> SpaceKind {
        use Rust::*;

        match node.kind_id().into() {
            FunctionItem | ClosureExpression => SpaceKind::Function,
            TraitItem => SpaceKind::Trait,
            ImplItem => SpaceKind::Impl,
            SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    keyword_roles!(rust);

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Rust::*;

        if GENERIC_ANGLES.is_closer(node, ancestors) {
            return TokenRole::Unknown;
        }
        match node.kind_id().into() {
            // `||` is treated as an operator only if it's part of a binary expression.
            // This prevents misclassification inside macros where closures without arguments (e.g., `let closure = || { /* ... */ };`)
            // are not recognized as `ClosureExpression` and their `||` node is identified as `PIPEPIPE` instead of `ClosureParameters`.
            //
            // Similarly, exclude `/` when it corresponds to the third slash in `///` (`OuterDocCommentMarker`)
            PIPEPIPE | SLASH => match ancestors.parent(node) {
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
            // `get_keyword_role` bills them (#1693; #394 added them).
            LPAREN | LBRACE | LBRACK | As | EQGT | PLUS | STAR | Await | Break | Continue
            | Else | For | If | In | Loop | Match | Return | While | EQ | COMMA | DASHGT
            | QMARK | LT | LT2 | GT | AMP | DOTDOT | DOTDOTEQ | DASH | AMPAMP | PIPE | CARET
            | EQEQ | BANGEQ | LTEQ | GTEQ | LTLT | GTGT | PERCENT | PLUSEQ | DASHEQ | STAREQ
            | SLASHEQ | PERCENTEQ | AMPEQ | PIPEEQ | CARETEQ | LTLTEQ | GTGTEQ | DOT | SEMI
            | COLON | COLONCOLON => TokenRole::Operator,
            // FieldIdentifier (e.g. `p.x`) and TypeIdentifier (e.g. `Vec`,
            // `HashMap`) are operand-class names — C++ and Go classify them
            // the same way (see arms ~588 and ~862 below). Omitting them
            // here silently dropped both into TokenRole::Unknown,
            // deflating n2/N2 and the derived vocabulary/volume/effort
            // estimates (issue #390).
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
