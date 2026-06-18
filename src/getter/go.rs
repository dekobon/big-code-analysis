//! `Getter` implementation for Go.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for GoCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        // Aliased because `Go::Go` (the `go` keyword variant) collides with
        // the bare enum name in pattern position under `use Go::*;`.
        use Go as G;

        match node.kind_id().into() {
            G::FunctionDeclaration | G::MethodDeclaration | G::FuncLiteral => SpaceKind::Function,
            G::SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Go as G;

        match node.kind_id().into() {
            // Control flow and declaration keywords
            G::If | G::Else | G::Switch | G::Case | G::Default | G::For | G::Range
            | G::Continue | G::Break | G::Fallthrough | G::Goto | G::Return | G::Select
            | G::Defer | G::Go | G::Func | G::Type | G::Struct | G::Interface | G::Map
            | G::Chan | G::Const | G::Var | G::Package | G::Import
            // Punctuation acting as operators
            | G::SEMI | G::COMMA | G::COLON | G::LBRACE | G::LBRACK | G::LPAREN
            | G::DOT | G::DOTDOTDOT
            // Operators
            | G::EQ | G::COLONEQ | G::PLUS | G::DASH | G::STAR | G::SLASH | G::PERCENT
            | G::AMP | G::PIPE | G::CARET | G::TILDE | G::BANG | G::LT | G::GT
            | G::LTEQ | G::GTEQ | G::EQEQ | G::BANGEQ | G::AMPAMP | G::PIPEPIPE
            | G::LTLT | G::GTGT | G::AMPCARET | G::LTDASH | G::PLUSPLUS | G::DASHDASH
            | G::PLUSEQ | G::DASHEQ | G::STAREQ | G::SLASHEQ | G::PERCENTEQ | G::AMPEQ
            | G::PIPEEQ | G::CARETEQ | G::LTLTEQ | G::GTGTEQ | G::AMPCARETEQ
                => HalsteadType::Operator,
            // Operands: identifiers and literals
            G::Identifier | G::Identifier2 | G::Identifier3 | G::BlankIdentifier
            | G::FieldIdentifier | G::PackageIdentifier | G::TypeIdentifier
            | G::LabelName | G::IntLiteral | G::FloatLiteral | G::ImaginaryLiteral
            | G::RuneLiteral | G::InterpretedStringLiteral | G::RawStringLiteral | G::Nil
            | G::True | G::False | G::Iota
                => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Go);
}
