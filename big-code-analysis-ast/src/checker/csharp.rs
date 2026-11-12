//! `Checker` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

/// A preprocessor directive that is trivia to Roslyn: one that wraps no
/// code, so the declaration after it keeps the doc comment above it.
fn csharp_directive(node: &Node) -> bool {
    matches!(
        node.kind_id().into(),
        Csharp::PreprocRegion
            | Csharp::PreprocEndregion
            | Csharp::PreprocLine
            | Csharp::PreprocPragma
            | Csharp::PreprocNullable
            | Csharp::PreprocError
            | Csharp::PreprocWarning
            | Csharp::PreprocDefine
            | Csharp::PreprocUndef
    )
}

impl Checker for CsharpCode {
    // The `Comment` and `DocComment` rows of `roles/csharp.rs` (#1699):
    // the one comment kind, documentation under Roslyn's marker.
    impl_comment_from_rows!(csharp);
    impl_doc_comment_from_row!(csharp);

    // Roslyn's `GetDocumentationCommentTriviaFromSyntaxNode` reads a
    // member's leading trivia backwards: past plain comments and
    // directives to the nearest doc comment, then over every doc comment
    // before it that only blank rows separate, all of which it
    // concatenates. A plain comment or a directive in that run ends it, and
    // every doc comment it leaves is CS1587 (#1613). A directive that
    // wraps code (`#if`) is a node the stretch cannot read past, so a doc
    // comment above one stays a plain comment, where Roslyn would give it
    // to the first member inside. A doc comment after code on its row
    // documents the member after it too.
    const DOC_BINDING: Option<DocBinding> = Some(DocBinding {
        skips: |node, _| csharp_directive(node),
        ..DocBinding::new(KeptDocs::Leading {
            hidden: |node, code| !Self::is_doc_comment(node, code),
            joins: |_, _, _| true,
            after_code: |_| true,
        })
    });

    // The `DocTarget` row of `roles/csharp.rs` (#1699).
    impl_doc_target_from_row!(csharp);

    // The `DocBodyResidual` row of `roles/csharp.rs` — a property or
    // indexer, which holds every doc comment in it but those in its
    // accessor list — read *before* the callable test, whose accessor
    // search would rescan every comment in the list or the header for
    // each of them (#1612); then the callables and the closures (#1699).
    fn holds_doc_comment_in_body<'a>(
        ancestor: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
        below: &Node<'a>,
    ) -> bool {
        if doc_body_residual_kind!(csharp, ancestor, code, ancestors) {
            return below.kind_id() != Csharp::AccessorList;
        }
        Self::is_func_with_code(ancestor, code, ancestors) || Self::is_closure(ancestor, ancestors)
    }

    // A bodied indexer (`this[int i] { get; set; }`) or property
    // (`int X { get; set; }`) defers to its `accessor_declaration` children
    // for its function spaces — counting it here too double-counts (property,
    // #472) or triple-counts (indexer, #464) the member in nom/wmc. Only the
    // accessor-less expression-bodied form (`this[int i] => _d[i];` /
    // `int W => _w;`) opens a space directly, matching the npm `.max(1)`
    // fallback and the way the implicit getter is the sole callable.
    // The space tree is declared in `roles/csharp.rs` (#1696): the
    // `FuncOpener` row (every callable, the closures, and an indexer or
    // property gated to its accessor-less form — #464, #472) and the
    // `Container` row are what the walk promotes, and `opens_space`
    // labels them. The rows read no bytes (the accessor gate reads the
    // node's children), but since #1828 the opener row reads the chain:
    // a `parameter_list` opens a space only under a type declaration,
    // where it is the primary constructor. So this byte-less spelling
    // is hand-written, as Ruby's is, and forwards with a *known-empty*
    // chain — the node read as if it were the root, so the parent gate
    // sees no type declaration and the list answers `false`, the
    // pre-#1828 answer for every parameter list. Not
    // `Ancestors::unknown()`, which climbs through `Node::parent` and
    // would answer the gated result here. The walk calls the
    // `_with_code` one. Where Ruby's gate over-approximates under the
    // empty chain, this one under-approximates;
    // `csharp_byte_less_spellings_omit_the_primary_constructor`
    // (`roles/space_tree_tests.rs`) pins the disagreement by value.
    fn is_func_space(node: &Node) -> bool {
        const _: () = assert!(
            !crate::roles::csharp::func_opener::TEXT_GATED
                && !crate::roles::csharp::container::TEXT_GATED
                && !crate::roles::csharp::container::CHAIN_GATED,
            "a space-opening row reads bytes; the byte-less spelling cannot forward with none"
        );
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::csharp::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func_with_code<'a>(node: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::csharp::func_opener::is_callable(node, code, ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::csharp::closure::owns(node, &[], ancestors)
    }

    /// `x => x + 1` puts a childless `implicit_parameter` in the
    /// `parameters` field, where `(x) => …` puts a `parameter_list`
    /// (#1185). Same shape as Java's bare-identifier lambda.
    fn is_bare_param(node: &Node) -> bool {
        node.kind_id() == Csharp::ImplicitParameter as u16
    }

    // `Call` and `StringLit` are the rows of `roles/csharp.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(csharp, call, is_call, is_call_with_code);
    impl_row_predicates!(csharp, string_lit, is_string, is_string_with_code);

    // No `is_non_arg`: `impl NArgs for CsharpCode` counts a parameter
    // list positively (`parameter` and `params`), since the grammar
    // flattens a `params` array into the list, so the trait's negative
    // filter is never consulted and the default `false` stands.

    // The `ElseIf` row of `roles/csharp.rs`: an `if_statement` whose
    // previous sibling is the bare `else` token (no clause wrapper), read
    // off the caller's chain. The row reads no bytes (#1697).
    impl_else_if_from_row!(csharp);

    // A primitive is the `Primitive` row's kind: `predefined_type`, one
    // kind for `int`, `string`, `bool`, `object`, ….
    #[inline]
    fn is_primitive(node: &Node) -> bool {
        crate::roles::csharp::primitive::lists_kind(node.kind_id())
    }

    #[inline]
    fn keys_operator_by_text<'a>(
        node: &Node<'a>,
        _code: &[u8],
        _ancestors: Ancestors<'a, '_>,
    ) -> bool {
        // Without this, every `PredefinedType` keyword (`int`, `string`,
        // `bool`, `object`, …) collapses into a single Halstead operator
        // because they share one `kind_id`. Keying them by text routes
        // them through the lexeme-keyed `primitive_operators` map so
        // distinct keywords count as distinct operators (issue #286).
        //
        // Every keyword the grammar aliases to a childless `modifier`
        // joins it for the mirror reason: the five parameter modifiers
        // (#1418) and a lambda's `static` / `async` (#1482). Each has
        // *two* kind spellings for one operator: its own token, and the
        // aliased `modifier`. Keying only the alias by text would split
        // one `ref` or `static` across the two maps and count it twice
        // in `n1` — the #453 shape — while keying the alias by `kind_id`
        // would instead collapse every spelling into one operator that
        // `bca ops` renders as the word `modifier`, which appears
        // nowhere in the source. Text is the only key under which the
        // two positions are the same operator, so the bare kind of every
        // keyword the getter bills through the alias must be listed too.
        //
        // Listing the bare kinds moves no existing count: each renders
        // exactly one text through `get_operator_id_as_str` and no other
        // kind renders that text, so the vocabulary entry only changes
        // which map holds it.
        //
        // The predicate also holds for the aliased `this` receiver,
        // which `get_op_type_with_code` bills as an operand. That is no
        // disagreement, because `compute_halstead` consults this method
        // only under its `Operator` arm.
        // role-bypass: allow(Primitive, Modifier) — key selection after
        // the rows have classified the node: which of their kinds has two
        // spellings for one operator and so is keyed by text, not whether
        // the node is a primitive or a modifier
        matches!(
            node.kind_id().into(),
            Csharp::PredefinedType
                | Csharp::Ref
                | Csharp::Out
                | Csharp::In
                | Csharp::Readonly
                | Csharp::Scoped
                | Csharp::Static
                | Csharp::Async
        ) || csharp_is_aliased_modifier(node)
    }
}
