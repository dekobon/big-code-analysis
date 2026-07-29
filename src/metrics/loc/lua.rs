//! `Loc` implementation for Lua.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::struct_field_names,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Loc for LuaCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            // Skip root.
            Lua::Chunk => {}

            // A Lua long-bracket string (`[[…]]`) can span several rows;
            // credit every spanned row to PLOC to match Python's #415
            // decision (#778).
            Lua::String => {
                add_multiline_string_ploc(node, ancestors, stats, start, end);
            }

            // Skip tokens that are children of comment nodes.
            // Lua's comment nodes have children: DASHDASH / LBRACKLBRACK (openers),
            // CommentContent / CommentContent2 (body), and RBRACKRBRACK (block closer).
            // Without this guard they hit the `_` arm and add their rows to `ploc`,
            // which rows are already counted in `only_comment_lines`, producing
            // negative `blank`. LBRACKLBRACK / RBRACKRBRACK also appear as children of
            // string nodes, so we guard on the parent kind to avoid skipping them there.
            Lua::DASHDASH | Lua::CommentContent | Lua::CommentContent2 => {}
            Lua::LBRACKLBRACK | Lua::RBRACKRBRACK
                if ancestors
                    .parent(node)
                    .is_some_and(|p| p.kind_id() == Lua::Comment) => {}

            Lua::Comment => {
                add_cloc_lines(stats, start, end);
            }

            // Standalone assignment (`x = 1`). Skip when nested inside a local variable
            // declaration (`local x = 1`) — the parent VariableDeclaration already counts.
            Lua::AssignmentStatement | Lua::AssignmentStatement2
                if !ancestors.parent(node).is_some_and(|p| {
                    matches!(
                        p.kind_id().into(),
                        Lua::VariableDeclaration
                            | Lua::VariableDeclaration2
                            | Lua::ImplicitVariableDeclaration
                    )
                }) =>
            {
                stats.lloc.logical_lines += 1;
            }

            Lua::IfStatement
            | Lua::ForStatement
            | Lua::WhileStatement
            | Lua::RepeatStatement
            | Lua::DoStatement
            | Lua::ReturnStatement
            | Lua::BreakStatement
            | Lua::GotoStatement
            | Lua::LabelStatement
            | Lua::VariableDeclaration
            | Lua::VariableDeclaration2
            | Lua::ImplicitVariableDeclaration
            | Lua::FunctionDeclaration
            | Lua::FunctionDeclaration2
            | Lua::FunctionDeclaration3 => {
                stats.lloc.logical_lines += 1;
            }

            _ => {
                check_comment_ends_on_code_line(stats, start);
                stats.ploc.lines.insert(start);
            }
        }
    }
}
