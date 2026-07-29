//! `Cyclomatic` implementation for Rust.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Cyclomatic for RustCode {
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    ) {
        // The default (#409): `?` counts toward cyclomatic, matching
        // upstream rust-code-analysis and every published metric value.
        Self::compute_with_options(node, code, ancestors, stats, true);
    }

    fn compute_with_options<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        _ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
        count_try: bool,
    ) {
        rust_cyclomatic_increment(node, stats, count_try);
    }
}

/// Rust's per-node cyclomatic increment, shared by both
/// [`Cyclomatic::compute`] and [`Cyclomatic::compute_with_options`].
///
/// Extracted as a free function so the `impl Cyclomatic for RustCode`
/// block carries only the two thin trait methods — the bare-wildcard
/// closure lives here instead, keeping the impl block's aggregate
/// `nargs` within the self-scan gate.
///
/// `count_try` toggles the `?` operator's contribution (#409): `true`
/// counts it toward both standard and modified cyclomatic, `false`
/// treats it as linear error propagation.
fn rust_cyclomatic_increment(node: &Node<'_>, stats: &mut Stats, count_try: bool) {
    use Rust::*;

    match node.kind_id().into() {
        // Standard-only: individual match arms.
        // Lizard counts `match` as a single control-flow keyword; we count
        // each arm, so the modified metric collapses them back to the
        // container.
        // Bare wildcard `_ =>` arms are skipped to match C-family
        // `default:` treatment. Patterns like `Some(_)`, `(_, x)`,
        // or `_ if guard` are not bare wildcards and still count.
        // The check scans NAMED children of `match_pattern`, so
        // anonymous tokens like a leading `|` (legal in or-patterns:
        // `| _ => ...`) don't throw off detection, and a guard
        // (`_ if g`) adds a second named child so it correctly
        // escapes the filter. Shared helper with the `Abc` impl
        // (`super::npa::pattern_is_bare_underscore`).
        MatchArm | MatchArm2 => {
            let is_bare_wildcard = node.child_by_field_name("pattern").is_some_and(|pat| {
                crate::metrics::npa::pattern_is_bare_underscore(&pat, UNDERSCORE as u16)
            });
            if !is_bare_wildcard {
                stats.cyclomatic += 1.;
            }
        }
        // Modified-only: the match expression container.
        MatchExpression => {
            stats.cyclomatic_modified += 1.;
        }
        // The `?` operator. Counted toward both standard and modified by
        // default; when `count_try` is false the arm's guard fails and
        // `?` falls through to `_ => {}`, treating it as linear error
        // propagation (#409). Gated separately from the unconditional
        // branching kinds below.
        TryExpression if count_try => {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
        }
        // Both standard and modified.
        If | For | While | Loop | AMPAMP | PIPEPIPE => {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
        }
        _ => {}
    }
}
