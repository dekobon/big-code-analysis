//! `Checker` implementation for Go.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::lang_helpers::doc_comment::{go_directive, node_bytes};

/// Whether `node` lies outside every function body: its nearest
/// `source_file`, `statement_list` or `block` ancestor is the
/// `source_file`.
fn go_outside_function_body<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    // The nearest of a source file and a statement list decides; folded
    // down the chain the last one met is that nearest, and the fold is
    // memoised per ancestor, so nested struct types cost one step each
    // rather than a climb per field (#1597).
    ancestors.fold_down(
        node,
        0,
        FoldLane::DocScope,
        0,
        |outside, ancestor, _, _| match ancestor.kind_id().into() {
            Go::SourceFile => 1,
            Go::StatementList | Go::Block => 0,
            _ => outside,
        },
    ) == 1
}

impl Checker for GoCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Go::Comment
    }

    fn is_doc_target<'a>(node: &Node<'a>, _code: &[u8], ancestors: Ancestors<'a, '_>) -> bool {
        // go/doc documents the package clause, top-level declarations,
        // the specs of a grouped `type ( … )` / `var ( … )` /
        // `const ( … )` — a grouped alias (`B = int`) is a `type_alias`,
        // not a `type_spec` — struct fields and interface elements
        // (methods and embedded types); never an import. It never reads
        // a declaration inside a function body, nor a field of a type
        // declared there, so a comment above one stays a working comment.
        match node.kind_id().into() {
            Go::PackageClause | Go::FunctionDeclaration | Go::MethodDeclaration => true,
            Go::TypeDeclaration
            | Go::VarDeclaration
            | Go::ConstDeclaration
            | Go::TypeSpec
            | Go::TypeAlias
            | Go::VarSpec
            | Go::ConstSpec
            | Go::FieldDeclaration
            | Go::MethodElem => go_outside_function_body(node, ancestors),
            // An embedded element is documented only when it opens with a
            // name (`io.Reader`, `int | string`, `G[int]`), the form
            // go/parser reads as a method spec with a lead comment; a
            // `~int` constraint and other non-name-led elements carry none.
            // The grammar also parses each argument of an instantiation
            // (`Map[string, int]`) as a `type_elem`, which no lead comment
            // documents, so only an interface's own elements qualify.
            Go::TypeElem => {
                ancestors.parent_has_kind(node, Go::InterfaceType as u16)
                    && node.child(0).is_some_and(|first| {
                        matches!(
                            first.kind_id().into(),
                            Go::TypeIdentifier | Go::QualifiedType | Go::GenericType
                        )
                    })
                    && go_outside_function_body(node, ancestors)
            }
            _ => false,
        }
    }

    fn is_doc_run_directive(node: &Node, code: &[u8]) -> bool {
        go_directive(node_bytes(node, code))
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Go::SourceFile | Go::FunctionDeclaration | Go::MethodDeclaration | Go::FuncLiteral
        )
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Go::FunctionDeclaration | Go::MethodDeclaration
        )
    }

    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Go::FuncLiteral
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Go::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(node.kind_id().into(), Go::LPAREN | Go::COMMA | Go::RPAREN)
    }

    impl_simple_is_string!(Go, InterpretedStringLiteral, RawStringLiteral);

    #[inline]
    fn is_else_if<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Go::IfStatement && ancestors.parent_has_kind(node, Go::IfStatement as u16)
    }
}
