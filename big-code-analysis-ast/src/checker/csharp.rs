//! `Checker` implementation for C#.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for CsharpCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Csharp::Comment
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
        // The five parameter-modifier keywords join it for the mirror
        // reason (#1418). Each has *two* kind spellings for one
        // operator: its own token, and the childless `modifier` the
        // grammar aliases it to in parameter position. Keying only the
        // alias by text would split one `ref` across the two maps and
        // count it twice in `n1` — the #453 shape — while keying the
        // alias by `kind_id` would instead collapse all five spellings
        // into one operator that `bca ops` renders as the word
        // `modifier`, which appears nowhere in the source. Text is the
        // only key under which the two positions are the same operator.
        //
        // Listing the five bare kinds moves no existing count: each
        // renders exactly one text through `get_operator_id_as_str` and
        // no other kind renders that text, so the vocabulary entry only
        // changes which map holds it.
        //
        // The shared predicate is wider than those five, and deliberately
        // so: it holds for every aliased `modifier`, including the `this`
        // receiver that `get_op_type_with_code` bills as an operand and
        // the `static` / `async` of a lambda that it leaves unclassified.
        // Neither is a disagreement between the two methods, because
        // `compute_halstead` consults this one only under its `Operator`
        // arm — so the answer is never read for anything the getter did
        // not call an operator. Widening the getter later (billing a
        // lambda's `static`) therefore needs no change here, and gets the
        // lexeme key it would want.
        matches!(
            node.kind_id().into(),
            Csharp::PredefinedType
                | Csharp::Ref
                | Csharp::Out
                | Csharp::In
                | Csharp::Readonly
                | Csharp::Scoped
        ) || csharp_is_aliased_modifier(node)
    }
}
