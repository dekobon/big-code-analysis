//! `Cognitive` implementation for C++.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

/// The C++ cognitive walk, one body for the two dialects, as
/// `cpp_family_abc!` is for ABC: Mozcpp is a vendored fork with its own
/// kind ids, pinned equal by `tests/parity/cpp_mozcpp_parity.rs`.
macro_rules! cpp_family_cognitive {
    ($code:ty, $lang:ident, $roles:path) => {
        impl Cognitive for $code {
            fn compute<'a>(
                node: &Node<'a>,
                code: &'a [u8],
                ancestors: Ancestors<'a, '_>,
                stats: &mut Stats,
                nesting_map: &mut NestingMap,
            ) {
                // bca: suppress(cyclomatic, halstead) — exhaustive kind dispatch table
                // One arm per grammar kind, like `CppCode::compute`'s ABC twin:
                // the count is the number of node kinds the grammar can hand us,
                // not branching a reader must hold, and the effort is the
                // table's vocabulary; the row reads (#1695) are arms like the
                // kind arms they replaced.
                use $lang::*;
                use $roles::{closure, func_opener, short_circuit, ternary};

                // Macro expansion is not tracked; macros are treated as opaque tokens.
                let mut nesting = get_nesting_from_map(node, nesting_map);
                // A requires clause or requires-expression is compile-time and
                // branches nothing, parenthesised or not, so nothing in one
                // scores; cyclomatic and ABC agree (#1533).
                let opens = matches!(node.kind_id().into(), RequiresClause | RequiresExpression);
                let in_constraint = stats.constraint.covers(node, opens);

                let kind = node.kind_id();

                match kind.into() {
                    _ if in_constraint => {}
                    IfStatement if !Self::is_else_if(node, ancestors) => {
                        increase_nesting(stats, &mut nesting);
                    }
                    ForStatement | ForRangeLoop | WhileStatement | DoStatement
                    | SwitchStatement | CatchClause => {
                        increase_nesting(stats, &mut nesting);
                    }
                    // The `conditional_expression` the `Ternary` row owns (#1695).
                    _ if ternary::owns_kind(kind, node, code, ancestors) => {
                        increase_nesting(stats, &mut nesting);
                    }
                    // `Else` here is the `else` keyword token, which the grammar
                    // also emits for the `else` of an `else if` — so this arm
                    // covers both.
                    GotoStatement | Else => {
                        increment_by_one(stats);
                    }
                    // A `binary_expression` or fold the `ShortCircuit` row owns.
                    // `and` / `or` are the ISO alternative spellings of `&&` /
                    // `||` ([lex.digraph]) and get kinds of their own, so each is
                    // keyed to its symbol: a chain that mixes the spellings of
                    // one operator is one sequence, as it is when spelled alike
                    // (#1522). A fold applies its operator once, so `(... && a)`
                    // is one sequence too, as cyclomatic counts it once (#1533);
                    // a binary fold's second spelling continues it. The row says
                    // which tokens pay; which of them share a sequence key is
                    // this consumer's reading of them.
                    // role-bypass: allow(ShortCircuit) — the sequence key the row's payers share, not
                    // membership: the row is read on the arm's guard
                    _ if short_circuit::owns_kind(kind, node, code, ancestors) => {
                        compute_booleans_keyed(node, stats, |id| match id.into() {
                            AMPAMP | And => Some(AMPAMP as u16),
                            PIPEPIPE | Or => Some(PIPEPIPE as u16),
                            _ => None,
                        });
                    }
                    // A lambda — the `Closure` row — charges the lambda channel
                    // and does not reset nesting.
                    _ if closure::owns(node, code, ancestors) => {
                        nesting.lambda += 1;
                    }
                    // At a (possibly nested) function-definition boundary — the
                    // `FuncOpener` row's callables, the four aliased
                    // `function_definition` ids — reset structural nesting to
                    // zero and bump the function-depth surcharge when this
                    // definition is itself nested inside another, with the
                    // row's callable kinds as the stop list. Without this, a
                    // method defined inside a control construct inherited the
                    // enclosing nesting and every nested definition missed the
                    // SonarSource B-nesting amplification (#696).
                    _ if func_opener::is_callable(node, code, ancestors) => {
                        enter_function_boundary(
                            &mut nesting,
                            node,
                            ancestors,
                            func_opener::CALLABLE,
                        );
                    }
                    _ => {}
                }
                nesting_map.insert(node.id(), nesting);
            }
        }
    };
}
pub(super) use cpp_family_cognitive;

cpp_family_cognitive!(CppCode, Cpp, crate::roles::cpp);
