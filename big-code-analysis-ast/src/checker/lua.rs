//! `Checker` implementation for Lua.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for LuaCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Lua::Comment
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Lua::Chunk
                | Lua::FunctionDeclaration
                | Lua::FunctionDeclaration2
                | Lua::FunctionDeclaration3
                | Lua::FunctionDefinition
        )
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Lua::FunctionDeclaration | Lua::FunctionDeclaration2 | Lua::FunctionDeclaration3
        )
    }

    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Lua::FunctionDefinition
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Lua::FunctionCall
    }

    fn is_non_arg(node: &Node) -> bool {
        // NOTE: `impl NArgs for LuaCode` overrides `compute` with a positive
        // filter on `Identifier | VarargExpression` and never calls `is_non_arg`.
        // This implementation satisfies the trait contract but is unused for NArgs.
        matches!(
            node.kind_id().into(),
            Lua::LPAREN | Lua::COMMA | Lua::RPAREN
        )
    }

    impl_simple_is_string!(Lua, String);

    // Lua uses a dedicated elseif_statement node rather than nesting a
    // second if_statement inside the outer one (as Go does).
    impl_is_else_if_clause!(Lua, ElseifStatement);
}
