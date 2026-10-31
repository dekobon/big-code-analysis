//! `Cyclomatic` implementation for Lua.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/lua.rs`, read at the projection the
// metric counts (#1695, #1697). Lua counts statement nodes, so `Branch`
// / `Loop` are plain rows read at the owner — `for` (numeric and
// generic), `while` and `repeat … until` — and the `elseif` statement,
// the `ElseIf` row's self-paying owner, pays as the branch it is.
// `and` / `or` from `ShortCircuit` at the token. Lua has no switch, no
// ternary, no catch (`pcall` is a call) and no null-safe operator.
impl_cyclomatic_rows!(
    LuaCode,
    Lua,
    roles = crate::roles::lua,
    both = [
        short_circuit::pays_at_leaf_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        else_if::pays_at_leaf_kind,
    ],
    standard = [],
    modified = [],
    extra = [],
    constraints = [],
);
