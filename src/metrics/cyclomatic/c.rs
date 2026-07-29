//! `Cyclomatic` implementation for C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

// C cannot reuse `impl_cyclomatic_c_family!`: that macro hard-codes a
// `Catch` decision arm, and C has no exceptions (no `catch` token). The
// decision-kind set is otherwise the C-family one — `if`/`for`/`while`,
// `case`, the `?:` ternary, and the `&&`/`||` short-circuit operators,
// with `switch` adding only to the modified count (#284).
impl Cyclomatic for CCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        use C::*;
        match node.kind_id().into() {
            Case => stats.cyclomatic += 1.,
            SwitchStatement => stats.cyclomatic_modified += 1.,
            If | For | While | ConditionalExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}
