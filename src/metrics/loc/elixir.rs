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

/// The grammar rule holding the literal text of every Elixir string form.
///
/// Matched by name rather than by `kind_id`: the Elixir grammar aliases
/// this one rule to twenty consecutive ids (`Elixir::QuotedContent`
/// through `QuotedContent20`), and a name comparison stays correct when a
/// grammar bump adds a twenty-first
/// (`.claude/rules/grammar-dispatch.md` section 1).
///
/// The rule calls that a small runtime cost, and it is: against a
/// twenty-variant `kind_id` pattern over a 36 000-row Elixir file, the
/// name comparison measured +1.3% of total `bca metrics` wall time with
/// identical minima (interleaved A/B, 25 samples each). A first,
/// non-interleaved measurement of the same pair reported +29% — build
/// order and thermal drift, not the comparison.
const QUOTED_CONTENT: &str = "quoted_content";

impl Loc for ElixirCode {
    fn compute(node: &Node, ancestors: Ancestors<'_, '_>, stats: &mut Stats, is_func_space: bool) {
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

            // PLOC: every row a string literal spans. `quoted_content` is
            // the literal text of *every* Elixir string form — `"…"`, the
            // `"""` heredoc, a `'''` charlist, and both `~s`/`~S` sigil
            // shapes all wrap one — and it is childless, so the leaf branch
            // of the catch-all below credited only its opening row. The
            // interior rows reached neither PLOC nor CLOC and
            // `blank = sloc - ploc - cloc` mislabelled them as blank
            // (#1260); crediting them to PLOC is the decision #778 took for
            // thirteen other languages and #415 took for Python.
            //
            // That covers `@doc """…"""` and `@moduledoc """…"""` as PLOC,
            // deliberately. Python's one carve-out to CLOC is a *bare*
            // string expression statement whose value is discarded — a
            // docstring by position. An Elixir module attribute is not that
            // shape: `@doc "…"` is an assignment whose value the compiler
            // stores and `Code.fetch_docs/1` reads back, so its Python
            // analogue is `x = """…"""`, which Python already counts as
            // PLOC.
            //
            // Routing it here rather than routing `E::String` keeps the
            // LLOC branch below intact: a bare string *is* a valid Elixir
            // statement and must keep counting one logical line, while
            // `quoted_content`'s parent is always a string / charlist /
            // sigil and so never one of the statement containers that
            // branch tests for.
            _ if node.kind() == QUOTED_CONTENT => {
                add_multiline_string_ploc(node, ancestors, stats, start);
            }

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
                    && ancestors.parent(node).is_some_and(|p| {
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
                    stats.lloc.count_logical_line();
                }
                if node.child_count() == 0 {
                    check_comment_ends_on_code_line(stats, start);
                    stats.ploc.lines.insert(start);
                }
            }
        }
    }
}
