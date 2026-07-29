//! `Getter` implementation for Lua.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for LuaCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Lua::FunctionDeclaration
            | Lua::FunctionDeclaration2
            | Lua::FunctionDeclaration3
            | Lua::FunctionDefinition => SpaceKind::Function,
            Lua::Chunk => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> HalsteadType {
        match node.kind_id().into() {
            // Control-flow and declaration keywords
            Lua::If
            | Lua::Then
            | Lua::Else
            | Lua::Elseif
            | Lua::End2
            | Lua::For
            | Lua::In
            | Lua::While
            | Lua::Do
            | Lua::Repeat
            | Lua::Until
            | Lua::Return
            | Lua::Goto
            | Lua::Local
            | Lua::Function
            // Logical operators (keywords in Lua)
            | Lua::And
            | Lua::Or
            | Lua::Not
            // Structural punctuation. Only the *opening* delimiter is an
            // operator: `get_operator_id_as_str` folds `LPAREN`/`LBRACK`/
            // `LBRACE` to the pair glyph `()`/`[]`/`{}`, so a balanced pair
            // is one operator with one occurrence — the convention the
            // C-family majority follows. Counting the matching closer too
            // (the former `RPAREN`/`RBRACK`/`RBRACE` arms) double-counted
            // every balanced pair, inflating n1 and N1 (#695).
            | Lua::SEMI
            | Lua::COMMA
            | Lua::COLON
            | Lua::COLONCOLON
            | Lua::LBRACE
            | Lua::LBRACK
            | Lua::LPAREN
            | Lua::DOT
            | Lua::DOTDOT
            // Arithmetic / concat / length
            | Lua::PLUS
            | Lua::DASH
            | Lua::STAR
            | Lua::SLASH
            | Lua::SLASHSLASH
            | Lua::PERCENT
            | Lua::CARET
            | Lua::HASH
            // Bitwise (Lua 5.3+)
            | Lua::AMP
            | Lua::PIPE
            | Lua::TILDE
            | Lua::LTLT
            | Lua::GTGT
            // Comparison
            | Lua::EQEQ
            | Lua::TILDEEQ
            | Lua::LT
            | Lua::GT
            | Lua::LTEQ
            | Lua::GTEQ
            // Assignment
            | Lua::EQ
            // `break` is a named leaf node (no anonymous keyword child), so it must be
            // matched directly here — unlike `return`/`goto` which are anonymous tokens.
            | Lua::BreakStatement => HalsteadType::Operator,

            // Operands: identifiers and literals
            Lua::Identifier | Lua::Number | Lua::String | Lua::True | Lua::False | Lua::Nil
            | Lua::VarargExpression => HalsteadType::Operand,

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Lua);
}
