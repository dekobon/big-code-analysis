//! `Checker` implementation for Java.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::node_bytes;

/// Whether two `///` doc comments on consecutive rows are one Markdown
/// doc comment, as javac's scanner lexes them.
fn java_markdown_lines_join(prev: &Node, next: &Node, code: &[u8]) -> bool {
    let line = |node: &Node| node_bytes(node, code).starts_with(b"///");
    line(prev) && line(next) && prev.end_row().saturating_add(1) == next.start_row()
}

impl Checker for JavaCode {
    // The `Comment` and `DocComment` rows of `roles/java.rs` (#1699):
    // both comment kinds, documentation when they open with javac's
    // marker, a byte gate on the row.
    impl_comment_from_rows!(java);
    impl_doc_comment_from_row!(java);

    // javac's `Token::docComment` gives a declaration the last doc comment
    // before its first token, its annotations and modifiers included, and
    // its dangling-doc-comments lint names every earlier one; blank rows
    // and plain comments in between do not matter (#1613).
    // The scanner lexes `///` lines on consecutive rows as one doc comment
    // (`JavaTokenizer`, JEP 467).
    const DOC_BINDING: Option<DocBinding> = Some(DocBinding::new(KeptDocs::Last {
        joins: java_markdown_lines_join,
    }));

    // The `DocTarget` row of `roles/java.rs` (#1699): what javac
    // attaches a doc comment to, with the package's file-shape gate
    // (#1446) on the row, reading the node's own chain.
    impl_doc_target_from_row!(java);

    // The callables and the lambda, then the `HoldsDocInBody` row of
    // `roles/java.rs` — `static { … }`, a bare `block`, a
    // `switch_block` (#1699). The whole node counts: javac binds a doc
    // comment only before a declaration's first token.
    fn holds_doc_comment_in_body<'a>(
        ancestor: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
        _below: &Node<'a>,
    ) -> bool {
        Self::is_func(ancestor, ancestors)
            || Self::is_closure(ancestor, ancestors)
            || holds_doc_in_body_kind!(java, ancestor, ancestors)
    }

    // The space tree is declared in `roles/java.rs` (#1696): the
    // `Container` row (class-like declarations, an anonymous class gated
    // on its body — #280, #463) and the `FuncOpener` row (the callables
    // and `static { … }`, #1184) are what the walk promotes;
    // `opens_space` labels them. The byte-less spellings here are the
    // legacy wrappers: Java's rows read no bytes and no chain, so they
    // answer identically with none.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::known(&[]))
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::java::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    // A record's compact constructor (`record R(int a) { R { … } }`, JLS
    // 8.10.4) is a `callable` owner of the `FuncOpener` row beside the
    // canonical constructor (#1160). Everything keyed on `is_func`
    // follows from the row: `Nom`, `NArgs`, `Npm`'s `direct_child_funcs`,
    // and the space tree that `Wmc` and `NExits` are attributed through.
    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::java::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::java::closure::owns(node, &[], ancestors)
    }

    /// `x -> x + 1` puts a bare `identifier` in the `parameters` field;
    /// `(x) -> …` and `(x, y) -> …` put an `inferred_parameters` list
    /// and `() -> …` a `formal_parameters` one (#1185).
    fn is_bare_param(node: &Node) -> bool {
        node.kind_id() == Java::Identifier as u16
    }

    // `Call` and `StringLit` are the rows of `roles/java.rs` (#1698);
    // the byte-less spellings are their kind tables, gates aside.
    impl_row_predicates!(java, call, is_call, is_call_with_code);
    impl_row_predicates!(java, string_lit, is_string, is_string_with_code);

    fn is_non_arg(node: &Node) -> bool {
        // Java's explicit receiver parameter (`void m(S this, int a)`, JLS
        // 8.4.1) parses as a `receiver_parameter` child of
        // `formal_parameters`, distinct from a real `formal_parameter`. It
        // binds `this`, not a value, so it is not a formal parameter and
        // must be excluded — matching Rust's `SelfParameter` (#457), Go's
        // `receiver` field, and C++'s implicit `this` (#470).
        matches!(
            node.kind_id().into(),
            Java::LPAREN | Java::COMMA | Java::RPAREN | Java::ReceiverParameter
        )
    }

    // The `ElseIf` row of `roles/java.rs`: an `if_statement` whose
    // previous sibling is the bare `else` token (no clause wrapper), read
    // off the caller's chain. The row reads no bytes (`TEXT_GATED` is
    // pinned false), so the empty code is exact (#1697).
    impl_else_if_from_row!(java);
}
