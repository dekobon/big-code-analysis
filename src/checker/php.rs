//! `Checker` implementation for PHP.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for PhpCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Php::Comment
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::Program
                | Php::FunctionDefinition
                | Php::MethodDeclaration
                | Php::AnonymousFunction
                | Php::ArrowFunction
                | Php::ClassDeclaration
                | Php::InterfaceDeclaration
                | Php::TraitDeclaration
                | Php::EnumDeclaration
                | Php::AnonymousClass
        )
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Php::FunctionDefinition | Php::MethodDeclaration
        )
    }

    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Php::AnonymousFunction | Php::ArrowFunction
        )
    }

    // Intentionally narrower than ABC's `branches` set: ABC additionally
    // counts `ObjectCreationExpression` (`new Foo()`) as a branch, but
    // `is_call` drives the `--ops` CLI feature and should match the
    // user's mental model of "function/method call sites" (mirrors
    // Java's `is_call` = `MethodInvocation` while ABC counts
    // `MethodInvocation | New`).
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

    // `String` is the named single-quoted literal; `String2` and
    // `String3` are aliased kind_ids that the language enum also
    // maps to `"string"` (`String2` is the `string` type keyword
    // and `String3` is the hidden `_string` supertype that covers
    // any string literal). Include all three so generic
    // string-filtering stays consistent with `get_op_type` and the
    // `Alterator` text-preservation arm (issue #288).
    impl_simple_is_string!(
        Php,
        String,
        String2,
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
