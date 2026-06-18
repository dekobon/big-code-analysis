//! `Cognitive` implementation for Ruby.
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

/// Folds a Ruby `binary`'s short-circuit operator children into the
/// boolean-sequence counter — Ruby has four (`&&`, `||`, word-form
/// `and`, word-form `or`).
fn compute_ruby_booleans(node: &Node, stats: &mut Stats) {
    compute_booleans_with(node, stats, |id| {
        matches!(
            id.into(),
            Ruby::AMPAMP | Ruby::PIPEPIPE | Ruby::And | Ruby::Or
        )
    });
}

impl Cognitive for RubyCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Ruby as R;

        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // Nesting-increasing constructs. tree-sitter-ruby models
            // `elsif` as its own `Elsif` clause (handled in the
            // branch-extension arm below) rather than nesting a second
            // `If` inside the outer one, so the `is_else_if` guard is
            // defensive only — mirrors the equivalent pattern in the
            // Lua impl.
            R::If if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            R::Unless
            | R::While
            | R::Until
            | R::For
            | R::Case
            | R::CaseMatch
            | R::Conditional
            | R::IfModifier
            | R::UnlessModifier
            | R::WhileModifier
            | R::UntilModifier
            | R::Rescue
            | R::RescueModifier
            | R::RescueModifier2
            | R::RescueModifier3 => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `elsif` and the `else` of an `if`/`elsif` chain extend the
            // parent branch at the same nesting level (+1).
            R::Elsif => {
                increment_branch_extension(stats);
            }
            // The `Else` clause node is shared across three Ruby
            // constructs, distinguished here by parent kind:
            //
            //   * `case/when` — the `else` is the default arm of a
            //     switch-like construct. The `case`/`case_match` parent
            //     already paid nesting (+1) above, so the default arm adds
            //     +0; counting it would double-count the construct (#451).
            //     This matches Kotlin's `when`-`else` (gated on `WhenEntry`
            //     above), Java/C# switch-`default`, and the SonarSource
            //     Cognitive Complexity spec, in which the catch-all arm of a
            //     switch carries no extra cost. See lesson #11 — the same
            //     logical construct must score identically across languages.
            //
            //   * `if`/`elsif` chain — the `else` is the alternative branch
            //     of a conditional and adds +1, mirroring `R::Elsif`.
            //
            //   * `begin/rescue` — the `else` runs when no exception was
            //     raised. Unlike `case`, the `begin` parent pays no nesting
            //     here (only `R::Rescue` does), so this `else` is *not*
            //     switch-like; it is the no-exception branch and adds +1,
            //     matching Python's `try`/`except`/`else` (the `ElseClause`
            //     arm in the Python impl). Suppressing it would re-introduce
            //     a cross-language divergence in the opposite direction.
            R::Else => {
                // The `case`/`case_match` default arm adds +0 (the parent
                // construct already paid nesting); every other `else` — the
                // `if`/`elsif` alternative and the `begin/rescue` no-exception
                // branch — extends the parent branch at +1. Mirrors Kotlin's
                // single `Else` arm gated on `WhenEntry`.
                let is_case_default = node
                    .parent()
                    .is_some_and(|p| matches!(p.kind_id().into(), R::Case | R::CaseMatch));
                if !is_case_default {
                    increment_branch_extension(stats);
                }
            }
            // Ruby has no labeled loops: `break`/`next` are always
            // unlabeled (the token's only optional child is a return-value
            // expression, never a label). Per SonarSource Cognitive
            // Complexity §B2, an unlabeled break/continue adds +0 — the
            // enclosing loop's nesting already accounts for it — so they are
            // intentionally excluded here. `redo` (restart the current loop
            // iteration) and `retry` (re-run a rescued `begin` block) are
            // genuinely unstructured jumps with no structured equivalent and
            // each add +1.
            R::Redo | R::Retry => {
                increment_by_one(stats);
            }
            R::Binary | R::Binary2 | R::Binary3 => {
                // Ruby has four short-circuit forms (`&&`, `||`, `and`,
                // `or`); use the dedicated helper rather than the
                // two-operator `compute_booleans` so word-form
                // operators land in the sequence too.
                compute_ruby_booleans(node, stats);
            }
            R::Method | R::SingletonMethod => {
                nesting = 0;
                increment_function_depth(&mut depth, node, &[R::Method, R::SingletonMethod]);
            }
            // Blocks, do-blocks and lambdas are the closure/lambda forms.
            R::Block | R::DoBlock | R::Lambda => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}
