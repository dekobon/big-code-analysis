//! `Cyclomatic` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// Every decision is a row of `roles/csharp.rs`, read at the projection
// the metric counts (#1694, #1695, #1697). C# counts statement nodes, so
// `Branch` / `Loop` / `Catch` / `Switch` are plain rows read at the
// owner. `&&` / `||` from `ShortCircuit` at the token; the
// `conditional_expression` from `Ternary` at its wrapper; the two guard
// clauses from `Guard` — `when_clause` and `catch_filter_clause`, each a
// decision the construct it guards does not already pay for (#1422), so
// a guarded discard scores two; the `NullSafe` row's payers, `??` at its
// token and a conditional access at its node, each a short-circuit
// decision (#436); the `case` label from `CaseArm` at the token (its row
// handles the C# switch-label / pattern-arm shapes); the `switch`
// statement and expression from `Switch`, the modified tier's container.
// The compound `??=` is excluded on the row as the consumers' own and is
// the one extra.
impl_cyclomatic_rows!(
    CsharpCode,
    Csharp,
    roles = crate::roles::csharp,
    both = [
        short_circuit::pays_at_leaf_kind,
        ternary::owns_kind,
        guard::owns_kind,
        null_safe::pays_at_leaf_kind,
        branch::pays_at_leaf_kind,
        r#loop::pays_at_leaf_kind,
        catch::pays_at_leaf_kind,
    ],
    standard = [case_arm::pays_at_leaf_kind],
    modified = [switch::owns_kind],
    extra = [QMARKQMARKEQ],
    constraints = [],
);
