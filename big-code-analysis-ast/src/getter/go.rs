//! `Getter` implementation for Go.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for GoCode {
    // The mapping is the `opens` annotations of `roles/go.rs`
    // (#1696). The rows read no bytes and no chain, so the byte-less
    // spelling forwards with none.
    fn get_space_kind(node: &Node) -> SpaceKind {
        Self::get_space_kind_with_code(node, &[], Ancestors::unknown())
    }

    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::go::opens_space(node, code, ancestors)
    }

    keyword_roles!(go);

    paying_roles!(go);

    fn get_op_type<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Go as G;

        // The declaration keywords are not here: `roles/go.rs` declares
        // them and `get_keyword_role` bills them (#1693).
        match node.kind_id().into() {
            // Control flow keywords and the `map` / `chan` type constructors
            // `<` / `>` for the spelling the `Comparison` row's parent gate
            // leaves out — a token error recovery strands — which billed
            // before the row existed; the other comparison and
            // short-circuit tokens are the rows' and `get_paying_role`
            // bills them (#1695).
            // role-bypass: allow(Comparison) — the operator classification of the spelling the row's
            // `under BinaryExpression` gate leaves out: a recovered token
            G::LT | G::GT
            | G::If | G::Else | G::Switch | G::Case | G::Default | G::For | G::Range
            | G::Continue | G::Break | G::Fallthrough | G::Goto | G::Return | G::Select
            | G::Defer | G::Go | G::Map | G::Chan
            // Punctuation acting as operators
            | G::SEMI | G::COMMA | G::COLON | G::LBRACE | G::LBRACK | G::LPAREN
            | G::DOT | G::DOTDOTDOT
            // Operators
            | G::EQ | G::COLONEQ | G::PLUS | G::DASH | G::STAR | G::SLASH | G::PERCENT
            | G::AMP | G::PIPE | G::CARET | G::TILDE | G::BANG
            | G::LTLT | G::GTGT | G::AMPCARET | G::LTDASH | G::PLUSPLUS | G::DASHDASH
            | G::PLUSEQ | G::DASHEQ | G::STAREQ | G::SLASHEQ | G::PERCENTEQ | G::AMPEQ
            | G::PIPEEQ | G::CARETEQ | G::LTLTEQ | G::GTGTEQ | G::AMPCARETEQ
                => TokenRole::Operator,
            // Operands: identifiers and literals
            G::Identifier | G::Identifier2 | G::Identifier3 | G::BlankIdentifier
            | G::FieldIdentifier | G::PackageIdentifier | G::TypeIdentifier
            | G::LabelName | G::IntLiteral | G::FloatLiteral | G::ImaginaryLiteral
            | G::RuneLiteral | G::InterpretedStringLiteral | G::RawStringLiteral | G::Nil
            | G::True | G::False | G::Iota
                => TokenRole::Operand,
            _ => TokenRole::Unknown,
        }
    }

    get_operator!(Go);
}
