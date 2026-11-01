//! `Checker` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::ruby::{YardScope, ruby_yard_reads};

/// The statement a comment run directly above `node` documents: `node`
/// itself, or the statement a wrapper stands for. A statement modifier
/// (`X = 1 unless defined?(X)`, `def m; end if c`) documents its
/// statement, as YARD reads it. The grammar parks a doc comment that
/// precedes the first statement of a `class` / `module` body or of a
/// brace block outside the `body_statement` / `block_body` wrapper, so
/// the wrapper stands in for its first child; `BodyStatement2` is the
/// hidden `_body_statement`, kept as a defensive arm, and `dloc.rs` pins
/// that the grammar never emits it. A loop rather than recursion: a chain
/// of modifiers as long as the file must not exhaust the walker's stack
/// (#1597, as for Lua's dotted targets). `None` for an empty wrapper.
/// The core comes with its own chain, so a row gate that reads the chain
/// answers for the core and not for the wrapper it sits in (lesson
/// 103): `node`'s own chain when `node` is the core, else `node`'s
/// extended by each wrapper descended through, ending in the core's
/// parent. The copy is made on the first wrapper hit and not before —
/// an unwrapped statement, the common case, borrows — since a copy per
/// question is `O(depth)` and a documented statement at each nesting
/// level made that quadratic (`loc/ruby-nested-doc-constants`).
fn ruby_documented_core<'a, 'c>(
    node: &Node<'a>,
    ancestors: Ancestors<'a, 'c>,
) -> Option<(Node<'a>, Option<ChildAncestors<'a, 'c>>)> {
    let mut node = *node;
    let mut chain: Option<ChildAncestors<'a, 'c>> = None;
    loop {
        let inner = match node.kind_id().into() {
            // role-bypass: allow(Branch) — the modifier's body is the
            // documented item; doc attachment, not membership
            Ruby::IfModifier | Ruby::UnlessModifier => node.child_by_field_name("body")?,
            Ruby::BodyStatement | Ruby::BodyStatement2 | Ruby::BlockBody => node.child(0)?,
            _ => return Some((node, chain)),
        };
        match &mut chain {
            Some(chain) => chain.descend(&node),
            None => chain = Some(ancestors.of_children(&node)),
        }
        node = inner;
    }
}

/// The first statement of a `then` wrapper (`Then`, 243 — `Then2`, 55, is
/// the keyword token, never a wrapper), or `None` for any other node.
fn ruby_then_first_statement<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    (node.kind_id() == Ruby::Then)
        .then(|| node.children().find(Node::is_named))
        .flatten()
}

impl Checker for RubyCode {
    // The `Comment` row of `roles/ruby.rs` (#1699); no `DocComment`.
    impl_comment_from_rows!(ruby);

    // The `DocTarget` row of `roles/ruby.rs` is the candidate; two
    // pieces of context over the *original* node stay here. The grammar
    // parks a comment that precedes the first statement of an `if`,
    // `elsif` or `unless` body ahead of the `then` wrapper, and one
    // before a body's first statement outside the `body_statement`
    // wrapper, so a wrapper stands for that statement — and the row is
    // asked of the core with the core's own chain, so a gate reading it
    // answers for the right node. YARD reads a run only in a namespace
    // or `def` body, which the scope fold decides over the original
    // node's chain, where the comment sits. The candidate test goes
    // first, so the scope fold runs only for one.
    fn is_doc_target<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        let (statement, chain) = match ruby_then_first_statement(node) {
            Some(first) => (first, Some(ancestors.of_children(node))),
            None => (*node, None),
        };
        let statement_chain = chain.as_ref().map_or(ancestors, ChildAncestors::get);
        ruby_documented_core(&statement, statement_chain).is_some_and(|(core, core_chain)| {
            let core_chain = core_chain
                .as_ref()
                .map_or(statement_chain, ChildAncestors::get);
            crate::roles::ruby::doc_target::owns(&core, code, core_chain)
                && ruby_yard_reads(YardScope::of(&core), node, ancestors)
        })
    }

    fn doc_target_start_row(node: &Node) -> usize {
        ruby_then_first_statement(node).unwrap_or(*node).start_row()
    }

    // The `DocRunDirective` and `DocRunOpener` rows of `roles/ruby.rs`
    // (#1699): a magic comment by its bytes; a directive block where
    // YARD reads, the scope fold over the opener's own chain.
    impl_doc_run_directive_from_row!(ruby);

    fn is_doc_run_opener<'a>(
        first: &Node<'a>,
        _next: Option<&Node<'a>>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::ruby::doc_run_opener::owns(first, code, ancestors)
    }

    // The space tree is declared in `roles/ruby.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The block rows read the chain — a `block` is
    // a space only outside a stabby lambda (#465, #1257) — so this
    // byte-less spelling stays the kind table, which over-approximates
    // on a lambda's own body block exactly as it did before the rows:
    // it forwards with a *known-empty* chain, the node read as if it
    // were the root, so the parent gate sees no lambda. Not
    // `Ancestors::unknown()`, which climbs through `Node::parent` and
    // would answer the gated result here — a change in this spelling's
    // answer no output could see. The walk calls the `_with_code` one.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::ruby::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::ruby::func_opener::is_callable(node, &[], ancestors)
    }

    // Ruby is the one non-JS grammar whose closure test is not
    // answerable from the node's own kind: the `Closure` row gates a
    // `block` / `do_block` on not being a stabby lambda's own body.
    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::ruby::closure::owns(node, &[], ancestors)
    }

    // `Call` and `StringLit` are the rows of `roles/ruby.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(ruby, call, is_call, is_call_with_code);
    impl_row_predicates!(ruby, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        // `PIPE` is included because block parameter lists are delimited
        // by `|` rather than parentheses (e.g. `[1,2,3].each { |x| … }`).
        matches!(
            node.kind_id().into(),
            Ruby::LPAREN
                | Ruby::LPAREN2
                | Ruby::RPAREN
                | Ruby::RPAREN2
                | Ruby::COMMA
                | Ruby::SEMI
                | Ruby::PIPE
        )
    }

    // The `ElseIf` row of `roles/ruby.rs`: the dedicated `elsif` clause
    // itself is the else-if (#1697).
    impl_else_if_from_row!(ruby);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_each_node_with_chain;

    /// The chain the core is asked with is the core's own: its last
    /// entry is the core's real parent, through a `then` wrapper, a
    /// modifier's `body`, a `body_statement` and a `block_body` alike —
    /// and `of_children` plus `descend` have no off-by-one that would
    /// hand the core its *children's* chain. An unwrapped statement
    /// borrows the chain it was asked with; only a wrapper hit copies.
    #[cfg(feature = "ruby")]
    #[test]
    fn core_is_asked_with_its_own_chain() {
        let source = b"class C\n# doc\nX = 1 unless defined?(X)\nif c then\ndef m; end if d\nend\nfoo { |a|\n# doc\nb = 1\n}\nend\n";
        let mut wrapped = Vec::new();
        let mut unwrapped = 0;
        for_each_node_with_chain::<RubyCode>(source, |node, chain| {
            let ancestors = Ancestors::known(chain);
            let (statement, then_chain) = match ruby_then_first_statement(node) {
                Some(first) => (first, Some(ancestors.of_children(node))),
                None => (*node, None),
            };
            let statement_chain = then_chain.as_ref().map_or(ancestors, ChildAncestors::get);
            let Some((core, core_chain)) = ruby_documented_core(&statement, statement_chain) else {
                return;
            };
            let asked = core_chain
                .as_ref()
                .map_or(statement_chain, ChildAncestors::get);
            assert_eq!(
                asked.parent(&core).map(|p| p.id()),
                core.parent().map(|p| p.id()),
                "{}: {} asked with a chain not its own",
                node.kind(),
                core.kind()
            );
            if core.id() == node.id() {
                assert!(
                    core_chain.is_none(),
                    "{}: copied for an unwrapped core",
                    node.kind()
                );
                unwrapped += 1;
            } else {
                wrapped.push((node.kind(), core.kind()));
            }
        });
        assert_eq!(
            wrapped,
            [
                ("body_statement", "assignment"),
                ("unless_modifier", "assignment"),
                ("then", "method"),
                ("if_modifier", "method"),
                ("block_body", "assignment"),
            ]
        );
        assert!(unwrapped > 20, "{unwrapped}");
        // Every wrapper shape is in the fixture: `then` → modifier →
        // method is two hops, so the chain grew twice from one copy.
    }
}
