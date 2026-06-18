//! `Cyclomatic` implementation for Lua.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for LuaCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        match node.kind_id().into() {
            Lua::IfStatement
            | Lua::ElseifStatement
            | Lua::ForStatement
            | Lua::WhileStatement
            | Lua::RepeatStatement
            | Lua::And
            | Lua::Or => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
