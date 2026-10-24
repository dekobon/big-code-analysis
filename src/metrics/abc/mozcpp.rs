#![allow(
    clippy::enum_glob_use,
    clippy::too_many_lines,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::cpp::{
    cpp_count_arguments, cpp_count_chain_operands, cpp_count_condition_slot, cpp_count_return,
    cpp_family_abc, cpp_walk_for_statement, cpp_walk_ternary,
};
use super::{Abc, ScoresItself, Stats, rows_own};
use crate::roles::mozcpp::{comparison, short_circuit, ternary};
use crate::*;

// The row-keyed `ScoresItself` of the Mozilla fork — the Cpp twin's,
// against this dialect's rows and ids; the walk below is `CppCode`'s
// body against them too (#1695), pinned equal by
// `tests/parity/cpp_mozcpp_parity.rs`.
fn mozcpp_condition_scores_itself(expr: &Node, code: &[u8]) -> bool {
    rows_own!(expr, code; comparison, short_circuit, ternary)
}

cpp_family_abc!(
    MozcppCode,
    Mozcpp,
    crate::roles::mozcpp,
    mozcpp_condition_scores_itself
);
