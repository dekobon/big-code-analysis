//! `Cyclomatic` implementation for Lua.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for LuaCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::lua::short_circuit;
        let kind = node.kind_id();
        // `and` / `or`, the `ShortCircuit` row's payers (#1695).
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors) {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        match kind.into() {
            Lua::IfStatement
            | Lua::ElseifStatement
            | Lua::ForStatement
            | Lua::WhileStatement
            | Lua::RepeatStatement => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
