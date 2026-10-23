//! `Checker` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{csharp_doc, node_bytes};

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
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Csharp::Comment
    }

    fn is_doc_comment(node: &Node, code: &[u8]) -> bool {
        Self::is_comment(node) && csharp_doc(node_bytes(node, code))
    }

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

    // The members `DocumentationCommentCompiler` writes: a type, a
    // delegate, a member and an enum member. A namespace (block or
    // file-scoped), a `using`, an assembly attribute, a top-level
    // statement and an accessor take none; checked with `dotnet build`
    // and CS1587.
    fn is_doc_target<'a>(node: &Node<'a>, _code: &[u8], _: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::ClassDeclaration
                | Csharp::StructDeclaration
                | Csharp::RecordDeclaration
                | Csharp::InterfaceDeclaration
                | Csharp::EnumDeclaration
                | Csharp::DelegateDeclaration
                | Csharp::MethodDeclaration
                | Csharp::ConstructorDeclaration
                | Csharp::DestructorDeclaration
                | Csharp::OperatorDeclaration
                | Csharp::ConversionOperatorDeclaration
                | Csharp::FieldDeclaration
                | Csharp::EventFieldDeclaration
                | Csharp::EventDeclaration
                | Csharp::PropertyDeclaration
                | Csharp::IndexerDeclaration
                | Csharp::EnumMemberDeclaration
        )
    }

    // A property or indexer holds every doc comment in it but those in its
    // accessor list: Roslyn binds one only before the member, so a comment
    // in its header or initializer documents nothing, as one after a
    // method's attribute does. Decided by the node on the comment's path
    // rather than by `is_func`, whose accessor search rescans every
    // comment in the list or the header for each of them (#1612).
    fn holds_doc_comment_in_body<'a>(
        ancestor: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
        below: &Node<'a>,
    ) -> bool {
        if matches!(
            ancestor.kind_id().into(),
            Csharp::IndexerDeclaration | Csharp::PropertyDeclaration
        ) {
            return below.kind_id() != Csharp::AccessorList;
        }
        Self::is_func(ancestor, ancestors) || Self::is_closure(ancestor, ancestors)
    }

    // A bodied indexer (`this[int i] { get; set; }`) or property
    // (`int X { get; set; }`) defers to its `accessor_declaration` children
    // for its function spaces — counting it here too double-counts (property,
    // #472) or triple-counts (indexer, #464) the member in nom/wmc. Only the
    // accessor-less expression-bodied form (`this[int i] => _d[i];` /
    // `int W => _w;`) opens a space directly, matching the npm `.max(1)`
    // fallback and the way the implicit getter is the sole callable.
    fn is_func_space(node: &Node) -> bool {
        if matches!(
            node.kind_id().into(),
            Csharp::IndexerDeclaration | Csharp::PropertyDeclaration
        ) {
            return !csharp_member_has_accessors(node);
        }
        matches!(
            node.kind_id().into(),
            Csharp::CompilationUnit
                | Csharp::ClassDeclaration
                | Csharp::StructDeclaration
                | Csharp::RecordDeclaration
                | Csharp::InterfaceDeclaration
                | Csharp::EnumDeclaration
                | Csharp::MethodDeclaration
                | Csharp::ConstructorDeclaration
                | Csharp::DestructorDeclaration
                | Csharp::LocalFunctionStatement
                | Csharp::LambdaExpression
                | Csharp::AnonymousMethodExpression
                | Csharp::AccessorDeclaration
                | Csharp::OperatorDeclaration
                | Csharp::ConversionOperatorDeclaration
        )
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        if matches!(
            node.kind_id().into(),
            Csharp::IndexerDeclaration | Csharp::PropertyDeclaration
        ) {
            return !csharp_member_has_accessors(node);
        }
        matches!(
            node.kind_id().into(),
            Csharp::MethodDeclaration
                | Csharp::ConstructorDeclaration
                | Csharp::DestructorDeclaration
                | Csharp::LocalFunctionStatement
                | Csharp::AccessorDeclaration
                | Csharp::OperatorDeclaration
                | Csharp::ConversionOperatorDeclaration
        )
    }

    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::LambdaExpression | Csharp::AnonymousMethodExpression
        )
    }

    // Invocations only, by the contract on `Checker::is_call`: C#'s three
    // other call-shaped constructs — `new T(…)`, the `: base(x)` /
    // `: this(a)` constructor initializer, and the C# 12 primary
    // constructor's `: Base(x)` — are object construction and constructor
    // delegation, which ABC's Fitzpatrick branch rule counts and this
    // filter deliberately does not. The matching arm on the other side of
    // that split is `csharp_count_token_branch` in
    // `src/metrics/abc/csharp.rs`; `csharp_is_call_excludes_constructors`
    // (`checker.rs` tests) pins the exclusion (#1456).
    fn is_call(node: &Node) -> bool {
        // The C# grammar emits three aliased `kind_id`s for
        // `invocation_expression`; matching only the unsuffixed variant
        // silently drops the rest (lesson #2 in lessons_learned.md).
        matches!(node.kind_id().into(), csharp_invocation_expr_kinds!())
    }

    /// `x => x + 1` puts a childless `implicit_parameter` in the
    /// `parameters` field, where `(x) => …` puts a `parameter_list`
    /// (#1185). Same shape as Java's bare-identifier lambda.
    fn is_bare_param(node: &Node) -> bool {
        node.kind_id() == Csharp::ImplicitParameter as u16
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::LPAREN | Csharp::COMMA | Csharp::RPAREN
        )
    }

    impl_simple_is_string!(
        Csharp,
        StringLiteral,
        VerbatimStringLiteral,
        RawStringLiteral,
        InterpolatedStringExpression,
    );

    // tree-sitter-c-sharp models `else if` as an `Else` keyword token
    // followed by a nested `if_statement` (no wrapping `else_clause` node).
    impl_is_else_if_prev_sibling!(Csharp, IfStatement, Else);

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        // Without this, every `PredefinedType` keyword (`int`, `string`,
        // `bool`, `object`, …) collapses into a single Halstead operator
        // because they share one `kind_id`. Returning `true` here routes
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
        // The shared predicate also holds for the aliased `this`
        // receiver, which `get_op_type_with_code` bills as an operand.
        // That is no disagreement, because `compute_halstead` consults
        // this method only under its `Operator` arm.
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
