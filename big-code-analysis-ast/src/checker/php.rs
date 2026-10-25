//! `Checker` implementation for PHP.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{never_joins, node_bytes, phpdoc};

/// A top-level node phpDocumentor's `createFileDocBlock` passes over
/// looking for the file's first element: `Declare_` and `InlineHTML`, which
/// tree-sitter-php spells as a `declare` statement and the `<?php` / `?>`
/// text around inline HTML.
fn php_file_preamble(node: &Node) -> bool {
    matches!(
        node.kind_id().into(),
        Php::PhpTag | Php::Text | Php::TextInterpolation | Php::DeclareStatement
    )
}

/// Whether `comment` is phpDocumentor's file DocBlock (#1613), which
/// documents the file wherever it sits. `createFileDocBlock` gathers the
/// comments before the first top-level node that is not a preamble; if that
/// node is not a class, function, constant, interface or trait, the first
/// DocBlock among them is the file's, and otherwise it is only when exactly
/// two DocBlocks precede it. The backward test stops at the previous
/// DocBlock, so only the file's first one scans forward.
fn php_file_docblock<'a>(comment: &Node<'a>, code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
    let preamble_or_plain = |n: Node| {
        php_file_preamble(&n) || (PhpCode::is_comment(&n) && !PhpCode::is_doc_comment(&n, code))
    };
    if !ancestors.parent_has_kind(comment, Php::Program as u16)
        || !ancestors.siblings_before(comment).all(preamble_or_plain)
    {
        return false;
    }
    let mut docs = 1_usize;
    for node in ancestors.siblings_after(comment) {
        if PhpCode::is_doc_comment(&node, code) {
            docs += 1;
        } else if !PhpCode::is_comment(&node) && !php_file_preamble(&node) {
            return docs == 2
                // role-bypass: allow(FuncOpener, Container, Member) — doc attachment, not membership: what a file docblock precedes is `DocTarget`'s own row (R09)
                || !matches!(
                    node.kind_id().into(),
                    Php::ClassDeclaration
                        | Php::FunctionDefinition
                        | Php::ConstDeclaration
                        | Php::ConstDeclaration2
                        | Php::InterfaceDeclaration
                        | Php::TraitDeclaration
                );
        }
    }
    true
}

impl Checker for PhpCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Php::Comment
    }

    fn is_doc_comment(node: &Node, code: &[u8]) -> bool {
        Self::is_comment(node) && phpdoc(node_bytes(node, code))
    }

    // phpDocumentor gives an element PHP-Parser's `getDocComment`: the
    // last DocBlock among the comments before the node's first token, so
    // plain comments in between are read past and only the last of several
    // DocBlocks documents it (#1613). A DocBlock among a declaration's own
    // children — after its `#[…]` attributes, before its body, on a
    // promoted parameter — is the one `ReflectionFunction::getDocComment`
    // reads, decided as before (#1612), as is the file DocBlock, which
    // documents the file rather than what follows. A property, a constant
    // and an enum case hold their `#[…]` attributes as children too, so
    // each is a declaration here like a method.
    const DOC_BINDING: Option<DocBinding> = Some(DocBinding {
        forward: |comment, code, ancestors| {
            let in_signature = ancestors.parent(comment).is_some_and(|parent| {
                // role-bypass: allow(FuncOpener, Closure, Container, Member) — doc attachment, not membership: what a doc comment binds to is `DocTarget`'s own row (R09)
                matches!(
                    parent.kind_id().into(),
                    Php::ClassDeclaration
                        | Php::InterfaceDeclaration
                        | Php::TraitDeclaration
                        | Php::EnumDeclaration
                        | Php::FunctionDefinition
                        | Php::MethodDeclaration
                        | Php::PropertyDeclaration
                        | Php::ConstDeclaration
                        | Php::ConstDeclaration2
                        | Php::EnumCase
                        | Php::AnonymousFunction
                        | Php::ArrowFunction
                        | Php::FormalParameters
                        | Php::PropertyPromotionParameter
                )
            });
            !in_signature && !php_file_docblock(comment, code, ancestors)
        },
        ..DocBinding::new(KeptDocs::Leading {
            hidden: |node, code| !Self::is_doc_comment(node, code),
            joins: never_joins,
            // PHP-Parser gives a node every comment since the token before
            // it, whatever row that token ends on.
            after_code: |_| true,
        })
    });

    // The structural elements phpDocumentor's reflection builds: a class,
    // interface, trait or enum, a function or method, a property, a class
    // or global constant and an enum case. `define()` is a call the
    // grammar parses as a statement, so a DocBlock above it stays a plain
    // comment, where phpDocumentor reads it as a constant's; a `use`, a
    // `namespace` or any other statement takes none.
    fn is_doc_target<'a>(node: &Node<'a>, _code: &[u8], _: Ancestors<'a, '_>) -> bool {
        // role-bypass: allow(FuncOpener, Container, Member) — doc attachment, not membership: what a doc comment binds to is `DocTarget`'s own row (R09)
        matches!(
            node.kind_id().into(),
            Php::ClassDeclaration
                | Php::InterfaceDeclaration
                | Php::TraitDeclaration
                | Php::EnumDeclaration
                | Php::FunctionDefinition
                | Php::MethodDeclaration
                | Php::PropertyDeclaration
                | Php::ConstDeclaration
                | Php::ConstDeclaration2
                | Php::EnumCase
        )
    }

    // PHP's parser binds the last docblock before a function's name to it,
    // so one written after `#[...]` attributes still documents it, and a
    // promoted constructor property keeps its parameter's docblock
    // (`ReflectionProperty::getDocComment`). Only the body is in-body: the
    // `compound_statement` child, or an arrow function's expression, which
    // is its last child since tree-sitter moves trailing comments out of a
    // node — except before a zero-width MISSING body, which leaves the
    // comment itself ending where the arrow function does. Decided by
    // `below`'s kind and span, because the `body` field lookup walks every
    // comment in the signature (#1612).
    fn holds_doc_comment_in_body<'a>(
        ancestor: &Node<'a>,
        _ancestors: Ancestors<'a, '_>,
        below: &Node<'a>,
    ) -> bool {
        // role-bypass: allow(FuncOpener, Closure) — where a comment sits in a
        // body, not whether the node opens a space
        match ancestor.kind_id().into() {
            Php::FunctionDefinition | Php::MethodDeclaration | Php::AnonymousFunction => {
                below.kind_id() == Php::CompoundStatement
            }
            Php::ArrowFunction => {
                below.end_byte() == ancestor.end_byte() && !Self::is_comment(below)
            }
            _ => false,
        }
    }

    // The space tree is declared in `roles/php.rs` (#1696): the
    // `FuncOpener` and `Container` rows are what the walk promotes and
    // `opens_space` labels. The rows read no bytes and no chain, so the
    // byte-less spelling forwards with none and answers the same.
    fn is_func_space(node: &Node) -> bool {
        Self::is_func_space_with_code(node, &[], Ancestors::unknown())
    }

    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        crate::roles::php::opens_space(node, code, ancestors) != SpaceKind::Unknown
    }

    fn is_func<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::php::func_opener::is_callable(node, &[], ancestors)
    }

    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        crate::roles::php::closure::owns(node, &[], ancestors)
    }

    // Intentionally narrower than ABC's `branches` set: ABC additionally
    // counts `ObjectCreationExpression` (`new Foo()`) as a branch, but
    // `is_call` drives the `--ops` CLI feature and should match the
    // user's mental model of "function/method call sites" (mirrors
    // Java's `is_call` = `MethodInvocation` while ABC counts
    // `MethodInvocation | New`).
    // role-bypass: allow(NullSafe) — every call is a call; the row claims the nullsafe call
    // for its `?->`, which `find call` reports as the call it is
    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::FunctionCallExpression
                | Php::MemberCallExpression
                | Php::ScopedCallExpression
                | Php::NullsafeMemberCallExpression
        )
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::LPAREN | Php::LPAREN2 | Php::COMMA | Php::RPAREN | Php::RPAREN2 | Php::DOTDOTDOT
        )
    }

    // `String` (368) is the named single-quoted literal. `String2` (25)
    // and `String3` (378) are aliased kind_ids the language enum also
    // maps to `"string"`, and they are opposites:
    //
    // - `String2` is the `string` *type* keyword — `bca dump` finds it
    //   only as the sole child of a `primitive_type` wrapper, in a
    //   property type, a parameter type, a `?string`, a `string|int`
    //   union and a return type. It is deliberately excluded so
    //   `find string` / `count string` report literals, matching TS's
    //   `String2` and TSX's `String3` (#1261). #288 listed
    //   it for kind-name identity and for parity with `get_op_type` and
    //   the `Alterator`; #1293 then moved it into `get_op_type`'s
    //   `primitive_type`-suppression arm, so that parity now argues the
    //   other way (#1474). A `(string)` cast never reaches either
    //   question — it is a childless `cast_type`.
    // - `String3` is the hidden `_string` supertype covering any string
    //   literal, kept as a defensive arm (lesson #34) and pinned
    //   unreachable by `php_hidden_string_supertype_is_unreachable`.
    impl_simple_is_string!(
        Php,
        String,
        String3,
        EncapsedString,
        Heredoc,
        Nowdoc,
        ShellCommandExpression,
    );

    // PHP models `else if` in two distinct shapes, so it cannot use a
    // single is_else_if macro:
    //
    // 1. The one-word `elseif` keyword parses as a dedicated
    //    `else_if_clause` node (`ElseIfClause`); both the brace
    //    `} elseif {` and the alternative `elseif: … endif;` colon forms
    //    parse as `ElseIfClause`. `ElseIfClause2` is an aliased kind_id
    //    the grammar maps to the same `else_if_clause` rule name but which
    //    does not surface in observed parse trees; it is kept as a
    //    defensive arm (lesson #34) so a future grammar revision that emits
    //    it is handled rather than silently dropped.
    // 2. The two-word `else if` form (only valid in the brace syntax)
    //    parses as an `else_clause` wrapping a nested `if_statement`
    //    (`else_clause → if_statement`, the `impl_is_else_if_parent_clause!`
    //    shape used by C++/JS/Rust). The inner `IfStatement` must be
    //    recognized as an else-if continuation, or the cognitive
    //    `IfStatement` arm double-counts it and inflates nesting for later
    //    arms (#529). A plain `else { if (…) {} }` does NOT match: there the
    //    inner `if` nests under a `compound_statement`, not directly under
    //    the `else_clause`.
    //
    // The alternative colon syntax (`if …: … endif;`) only accepts the
    // one-word `elseif` (shape 1); two-word `else if` there is a PHP fatal
    // parse error and the grammar emits an `ERROR` node, so it is
    // intentionally not handled here — there is no well-defined metric for
    // syntactically invalid source.
    //
    // Matching shapes 1 and 2 lets the cognitive guard
    // `IfStatement if !Self::is_else_if(node)` suppress the nested-if
    // penalty while the wrapping `else_clause` still scores its +1
    // branch extension. The dedicated-clause match (shape 1) is
    // behaviorally inert today — `count_specific_ancestors` is not used by
    // PHP cognitive and the guard only fires on `IfStatement` — but is kept
    // for parity with the PHP cyclomatic/ABC/cognitive dispatch which list
    // both clause variants.
    #[inline]
    fn is_else_if<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        let kind = node.kind_id();
        matches!(kind.into(), Php::ElseIfClause | Php::ElseIfClause2)
            || (kind == Php::IfStatement
                && ancestors.parent(node).is_some_and(|parent| {
                    matches!(parent.kind_id().into(), Php::ElseClause | Php::ElseClause2)
                }))
    }
}
