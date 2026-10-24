//! `Cyclomatic` implementation for C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// C cannot reuse `impl_cyclomatic_c_family_rows!`: that macro hard-codes a
// `Catch` decision arm, and C has no exceptions (no `catch` token). The
// decision-kind set is otherwise the C-family one — `if`/`for`/`while`,
// `case`, the `?:` ternary, and the `&&`/`||` short-circuit operators,
// with `switch` adding only to the modified count (#284).
impl Cyclomatic for CCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use crate::roles::c::{case_arm, short_circuit, ternary};
        use C::*;
        let kind = node.kind_id();
        // `&&` / `||`, the `ShortCircuit` row's payers, and the
        // `conditional_expression` the `Ternary` row owns (#1695).
        if short_circuit::pays_at_leaf_kind(kind, node, code, ancestors)
            || ternary::owns_kind(kind, node, code, ancestors)
        {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
            return;
        }
        // A `case` label, the `CaseArm` row's payer; `default` is excluded
        // there (lesson 11, #469).
        if case_arm::pays_at_leaf_kind(kind, node, code, ancestors) {
            stats.cyclomatic += 1.;
            return;
        }
        match kind.into() {
            SwitchStatement => stats.cyclomatic_modified += 1.,
            If | For | While => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
