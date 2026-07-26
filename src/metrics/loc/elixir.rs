//! `Loc` implementation for Elixir.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::struct_field_names,
    clippy::wildcard_imports
)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::*;

impl Loc for ElixirCode {
    fn compute(node: &Node, stats: &mut Stats, is_func_space: bool) {
        use Elixir as E;

        let (start, end) = init(node, stats, is_func_space);

        match node.kind_id().into() {
            // Root of the file — handled by `init` above.
            E::Source => {}

            // CLOC: every line a comment spans.
            E::Comment => add_cloc_lines(stats, start, end),

            // The `stab_clause` itself is a control-flow noise node
            // (case/cond/with arm header). Its `body` child holds the
            // actual statements executed when the pattern matches, and
            // those count via the parent-container check below. Skipping
            // the `stab_clause` keeps the count consistent with C-family
            // languages where `case:` labels don't count but the body
            // statements do. A `stab_clause` always has at least the
            // `->` token plus a `body`, so there is no leaf-PLOC path
            // to handle here.
            E::StabClause => {}

            // LLOC: any named node whose parent is a statement container
            // is one logical line. This catches `def`/`if`/`case`/`cond`
            // calls (themselves `Call` nodes at the top level),
            // assignment `binary_operator`s in function bodies, and bare
            // expressions used as statements. The container kinds are
            // every grammar node whose direct named children represent
            // a sequence of executable expressions. The `is_named()`
            // check runs first so unnamed leaves (`do`, `end`, `,`, …)
            // skip the parent lookup entirely.
            _ => {
                if node.as_tree_sitter().is_named()
                    && node.parent().is_some_and(|p| {
                        matches!(
                            p.kind_id().into(),
                            E::Source
                                | E::Body
                                | E::Block
                                | E::DoBlock
                                | E::AfterBlock
                                | E::RescueBlock
                                | E::CatchBlock
                                | E::ElseBlock
                        )
                    })
                {
                    stats.lloc.logical_lines += 1;
                }
                if node.child_count() == 0 {
                    check_comment_ends_on_code_line(stats, start);
                    stats.ploc.lines.insert(start);
                }
            }
        }
    }
}
