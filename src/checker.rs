// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use std::sync::OnceLock;

use aho_corasick::AhoCorasick;
use regex::bytes::Regex;

use crate::macros::csharp_invocation_expr_kinds;
use crate::*;

static AHO_CORASICK: OnceLock<AhoCorasick> = OnceLock::new();
static RE: OnceLock<Regex> = OnceLock::new();

macro_rules! check_if_func {
    ($parser: ident, $node: ident) => {
        $node.count_specific_ancestors::<$parser>(
            |node| {
                matches!(
                    node.kind_id().into(),
                    VariableDeclarator | AssignmentExpression | LabeledStatement | Pair
                )
            },
            |node| {
                matches!(
                    node.kind_id().into(),
                    StatementBlock | ReturnStatement | NewExpression | Arguments
                )
            },
        ) > 0
            || $node.is_child(Identifier as u16)
    };
}

macro_rules! check_if_arrow_func {
    ($parser: ident, $node: ident) => {
        $node.count_specific_ancestors::<$parser>(
            |node| {
                matches!(
                    node.kind_id().into(),
                    VariableDeclarator | AssignmentExpression | LabeledStatement
                )
            },
            |node| {
                matches!(
                    node.kind_id().into(),
                    StatementBlock | ReturnStatement | NewExpression | CallExpression
                )
            },
        ) > 0
            || $node.has_sibling(PropertyIdentifier as u16)
    };
}

macro_rules! is_js_func {
    ($parser: ident, $node: ident) => {
        match $node.kind_id().into() {
            FunctionDeclaration | MethodDefinition => true,
            FunctionExpression => check_if_func!($parser, $node),
            ArrowFunction => check_if_arrow_func!($parser, $node),
            _ => false,
        }
    };
}

macro_rules! is_js_closure {
    ($parser: ident, $node: ident) => {
        match $node.kind_id().into() {
            GeneratorFunction | GeneratorFunctionDeclaration => true,
            FunctionExpression => !check_if_func!($parser, $node),
            ArrowFunction => !check_if_arrow_func!($parser, $node),
            _ => false,
        }
    };
}

macro_rules! is_js_func_and_closure_checker {
    ($parser: ident, $language: ident) => {
        #[inline]
        fn is_func(node: &Node) -> bool {
            use $language::*;
            is_js_func!($parser, node)
        }

        #[inline]
        fn is_closure(node: &Node) -> bool {
            use $language::*;
            is_js_closure!($parser, node)
        }
    };
}

#[inline]
fn get_aho_corasick_match(code: &[u8]) -> bool {
    AHO_CORASICK
        .get_or_init(|| AhoCorasick::new(vec![b"<div rustbindgen"]).unwrap())
        .is_match(code)
}

pub trait Checker {
    fn is_comment(_: &Node) -> bool;
    fn is_useful_comment(_: &Node, _: &[u8]) -> bool;
    fn is_func_space(_: &Node) -> bool;
    fn is_func(_: &Node) -> bool;
    fn is_closure(_: &Node) -> bool;
    fn is_call(_: &Node) -> bool;
    fn is_non_arg(_: &Node) -> bool;
    fn is_string(_: &Node) -> bool;
    fn is_else_if(_: &Node) -> bool;
    fn is_primitive(_id: u16) -> bool;

    fn is_error(node: &Node) -> bool {
        node.has_error()
    }

    /// Return `true` to elide this node and all its descendants from
    /// every metric. Used by language modules to filter
    /// test-only / generated / preprocessor-disabled subtrees.
    ///
    /// The default returns `false` for every node, preserving the
    /// pre-#182 behavior. Language overrides drive opt-in skips
    /// (currently: `RustCode` filters `#[cfg(test)]` items, gated
    /// by the runtime `MetricsOptions::exclude_tests` flag).
    #[inline]
    fn should_skip_subtree(_node: &Node, _code: &[u8]) -> bool {
        false
    }
}

impl Checker for PreprocCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Preproc::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(_: &Node) -> bool {
        false
    }

    fn is_func(_: &Node) -> bool {
        false
    }

    fn is_closure(_: &Node) -> bool {
        false
    }

    fn is_call(_: &Node) -> bool {
        false
    }

    fn is_non_arg(_: &Node) -> bool {
        false
    }

    fn is_string(node: &Node) -> bool {
        node.kind_id() == Preproc::StringLiteral || node.kind_id() == Preproc::RawStringLiteral
    }

    fn is_else_if(_: &Node) -> bool {
        false
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for CcommentCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Ccomment::Comment
    }

    fn is_useful_comment(node: &Node, code: &[u8]) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    fn is_func_space(_: &Node) -> bool {
        false
    }

    fn is_func(_: &Node) -> bool {
        false
    }

    fn is_closure(_: &Node) -> bool {
        false
    }

    fn is_call(_: &Node) -> bool {
        false
    }

    fn is_non_arg(_: &Node) -> bool {
        false
    }

    fn is_string(node: &Node) -> bool {
        node.kind_id() == Ccomment::StringLiteral || node.kind_id() == Ccomment::RawStringLiteral
    }

    fn is_else_if(_: &Node) -> bool {
        false
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for CppCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Cpp::Comment
    }

    fn is_useful_comment(node: &Node, code: &[u8]) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::TranslationUnit
                | Cpp::FunctionDefinition
                | Cpp::FunctionDefinition2
                | Cpp::FunctionDefinition3
                | Cpp::StructSpecifier
                | Cpp::ClassSpecifier
                | Cpp::NamespaceDefinition
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::FunctionDefinition
                | Cpp::FunctionDefinition2
                | Cpp::FunctionDefinition3
                | Cpp::FunctionDefinition4
        )
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Cpp::LambdaExpression
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Cpp::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::LPAREN | Cpp::LPAREN2 | Cpp::COMMA | Cpp::RPAREN
        )
    }

    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::StringLiteral | Cpp::ConcatenatedString | Cpp::RawStringLiteral
        )
    }

    fn is_else_if(node: &Node) -> bool {
        if node.kind_id() != Cpp::IfStatement {
            return false;
        }
        if let Some(parent) = node.parent() {
            return parent.kind_id() == Cpp::ElseClause;
        }
        false
    }

    #[inline]
    fn is_primitive(id: u16) -> bool {
        id == Cpp::PrimitiveType
    }
}

impl Checker for PythonCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Python::Comment
    }

    fn is_useful_comment(node: &Node, code: &[u8]) -> bool {
        // comment containing coding info are useful
        node.start_row() <= 1
            && RE
                .get_or_init(|| {
                    Regex::new(r"^[ \t\f]*#.*?coding[:=][ \t]*([-_.a-zA-Z0-9]+)").unwrap()
                })
                .is_match(&code[node.start_byte()..node.end_byte()])
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Python::Module | Python::FunctionDefinition | Python::ClassDefinition
        )
    }

    fn is_func(node: &Node) -> bool {
        node.kind_id() == Python::FunctionDefinition
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Python::Lambda
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Python::Call
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Python::LPAREN | Python::COMMA | Python::RPAREN
        )
    }

    fn is_string(node: &Node) -> bool {
        node.kind_id() == Python::String || node.kind_id() == Python::ConcatenatedString
    }

    fn is_else_if(_: &Node) -> bool {
        false
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for JavaCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Java::LineComment || node.kind_id() == Java::BlockComment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Java::Program | Java::ClassDeclaration | Java::InterfaceDeclaration
        )
    }

    fn is_func(node: &Node) -> bool {
        node.kind_id() == Java::MethodDeclaration || node.kind_id() == Java::ConstructorDeclaration
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Java::LambdaExpression
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Java::MethodInvocation
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Java::LPAREN | Java::COMMA | Java::RPAREN
        )
    }

    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Java::StringLiteral | Java::MultilineStringLiteral
        )
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // tree-sitter-java models `else if` as an `Else` keyword token followed
        // by a nested `if_statement` (no wrapping `else_clause` node).
        node.kind_id() == Java::IfStatement
            && node
                .previous_sibling()
                .is_some_and(|prev| prev.kind_id() == Java::Else)
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for CsharpCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Csharp::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
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
                | Csharp::IndexerDeclaration
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::MethodDeclaration
                | Csharp::ConstructorDeclaration
                | Csharp::DestructorDeclaration
                | Csharp::LocalFunctionStatement
                | Csharp::AccessorDeclaration
                | Csharp::OperatorDeclaration
                | Csharp::ConversionOperatorDeclaration
                | Csharp::IndexerDeclaration
        )
    }

    fn is_closure(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::LambdaExpression | Csharp::AnonymousMethodExpression
        )
    }

    fn is_call(node: &Node) -> bool {
        // The C# grammar emits three aliased `kind_id`s for
        // `invocation_expression`; matching only the unsuffixed variant
        // silently drops the rest (lesson #2 in lessons_learned.md).
        matches!(node.kind_id().into(), csharp_invocation_expr_kinds!())
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::LPAREN | Csharp::COMMA | Csharp::RPAREN
        )
    }

    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Csharp::StringLiteral
                | Csharp::VerbatimStringLiteral
                | Csharp::RawStringLiteral
                | Csharp::InterpolatedStringExpression
        )
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // tree-sitter-c-sharp models `else if` as an `Else` keyword token
        // followed by a nested `if_statement` (no wrapping `else_clause` node).
        node.kind_id() == Csharp::IfStatement
            && node
                .previous_sibling()
                .is_some_and(|prev| prev.kind_id() == Csharp::Else)
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for MozjsCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Mozjs::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozjs::Program
                | Mozjs::FunctionExpression
                | Mozjs::Class
                | Mozjs::GeneratorFunction
                | Mozjs::FunctionDeclaration
                | Mozjs::MethodDefinition
                | Mozjs::GeneratorFunctionDeclaration
                | Mozjs::ClassDeclaration
                | Mozjs::ArrowFunction
        )
    }

    is_js_func_and_closure_checker!(MozjsParser, Mozjs);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Mozjs::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Mozjs::LPAREN | Mozjs::COMMA | Mozjs::RPAREN
        )
    }

    fn is_string(node: &Node) -> bool {
        node.kind_id() == Mozjs::String || node.kind_id() == Mozjs::TemplateString
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        if node.kind_id() != Mozjs::IfStatement {
            return false;
        }
        if let Some(parent) = node.parent() {
            return parent.kind_id() == Mozjs::ElseClause;
        }
        false
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for JavascriptCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Javascript::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Javascript::Program
                | Javascript::FunctionExpression
                | Javascript::Class
                | Javascript::GeneratorFunction
                | Javascript::FunctionDeclaration
                | Javascript::MethodDefinition
                | Javascript::GeneratorFunctionDeclaration
                | Javascript::ClassDeclaration
                | Javascript::ArrowFunction
        )
    }

    is_js_func_and_closure_checker!(JavascriptParser, Javascript);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Javascript::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Javascript::LPAREN | Javascript::COMMA | Javascript::RPAREN
        )
    }

    fn is_string(node: &Node) -> bool {
        node.kind_id() == Javascript::String || node.kind_id() == Javascript::TemplateString
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Javascript::IfStatement
            && node
                .parent()
                .is_some_and(|p| p.kind_id() == Javascript::ElseClause)
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for TypescriptCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Typescript::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Typescript::Program
                | Typescript::FunctionExpression
                | Typescript::Class
                | Typescript::GeneratorFunction
                | Typescript::FunctionDeclaration
                | Typescript::MethodDefinition
                | Typescript::GeneratorFunctionDeclaration
                | Typescript::ClassDeclaration
                | Typescript::AbstractClassDeclaration
                | Typescript::InterfaceDeclaration
                | Typescript::ArrowFunction
        )
    }

    is_js_func_and_closure_checker!(TypescriptParser, Typescript);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Typescript::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Typescript::LPAREN | Typescript::COMMA | Typescript::RPAREN
        )
    }

    fn is_string(node: &Node) -> bool {
        node.kind_id() == Typescript::String || node.kind_id() == Typescript::TemplateString
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        if node.kind_id() != Typescript::IfStatement {
            return false;
        }
        if let Some(parent) = node.parent() {
            return parent.kind_id() == Typescript::ElseClause;
        }
        false
    }

    #[inline]
    fn is_primitive(id: u16) -> bool {
        id == Typescript::PredefinedType
    }
}

impl Checker for TsxCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Tsx::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Tsx::Program
                | Tsx::FunctionExpression
                | Tsx::Class
                | Tsx::GeneratorFunction
                | Tsx::FunctionDeclaration
                | Tsx::MethodDefinition
                | Tsx::GeneratorFunctionDeclaration
                | Tsx::ClassDeclaration
                | Tsx::AbstractClassDeclaration
                | Tsx::InterfaceDeclaration
                | Tsx::ArrowFunction
        )
    }

    is_js_func_and_closure_checker!(TsxParser, Tsx);

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Tsx::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Tsx::LPAREN | Tsx::COMMA | Tsx::RPAREN
        )
    }

    fn is_string(node: &Node) -> bool {
        node.kind_id() == Tsx::String || node.kind_id() == Tsx::TemplateString
    }

    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Tsx::IfStatement
            && node
                .parent()
                .is_some_and(|p| p.kind_id() == Tsx::ElseClause)
    }

    #[inline]
    fn is_primitive(id: u16) -> bool {
        id == Tsx::PredefinedType
    }
}

fn rust_attribute_marks_test(body: &str) -> bool {
    let trimmed = body.trim();
    let check = |s: &str| {
        matches!(s, "test" | "rstest" | "wasm_bindgen_test" | "test_case")
            || s.starts_with("cfg(test)")
            || s.starts_with("cfg(test,")
            || s.starts_with("cfg(all(test,")
            || s.starts_with("cfg(all(test)")
            || s.starts_with("cfg(any(test,")
            || s.starts_with("cfg(any(test)")
            || s.ends_with("::test")
            || s.contains("::test(")
    };
    // Fast path: no allocation for the idiomatic forms where the body
    // has no internal whitespace.
    if check(trimmed) {
        return true;
    }
    // Slow path: tolerate unusual spacing like `# [ cfg ( test ) ]`
    // by collapsing all ASCII whitespace before re-checking.
    if trimmed.bytes().any(|b| b.is_ascii_whitespace()) {
        let compact: String = trimmed
            .bytes()
            .filter(|b| !b.is_ascii_whitespace())
            .map(char::from)
            .collect();
        return check(&compact);
    }
    false
}

/// Strip the `#` / `#!` marker plus the `[...]` brackets from a
/// Rust `AttributeItem` / `InnerAttributeItem` token's raw text,
/// returning the inner body. Returns `None` if the input shape is
/// unexpected — callers skip silently rather than feed the matcher
/// the literal `#[...]` form.
fn rust_attribute_body<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.trim()
        .strip_prefix(marker)
        .and_then(|t| t.trim_start().strip_prefix('['))
        .and_then(|t| t.trim().strip_suffix(']'))
}

fn rust_item_is_test_only(node: &Node, code: &[u8]) -> bool {
    // The tree-sitter Rust grammar exposes outer attributes
    // (`#[...]`) as `AttributeItem` siblings *before* the decorated
    // item. Walk backward across consecutive attribute siblings; any
    // match short-circuits.
    let mut sibling = node.previous_sibling();
    while let Some(s) = sibling {
        if s.kind_id() != Rust::AttributeItem {
            break;
        }
        if let Some(text) = s.utf8_text(code)
            && let Some(inner) = rust_attribute_body(text, "#")
            && rust_attribute_marks_test(inner)
        {
            return true;
        }
        sibling = s.previous_sibling();
    }

    // `mod_item` additionally accepts inner attributes
    // (`#![cfg(test)]`). The grammar nests these inside the module's
    // `declaration_list` body, not as direct `mod_item` children, so
    // descend one level via the `body` field before scanning.
    if node.kind_id() == Rust::ModItem
        && let Some(body) = node.child_by_field_name("body")
    {
        for child in body.children() {
            if child.kind_id() != Rust::InnerAttributeItem {
                continue;
            }
            if let Some(text) = child.utf8_text(code)
                && let Some(inner) = rust_attribute_body(text, "#!")
                && rust_attribute_marks_test(inner)
            {
                return true;
            }
        }
    }
    false
}

impl Checker for RustCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Rust::LineComment || node.kind_id() == Rust::BlockComment
    }

    fn is_useful_comment(node: &Node, code: &[u8]) -> bool {
        if let Some(parent) = node.parent()
            && parent.kind_id() == Rust::TokenTree
        {
            // A comment could be a macro token
            return true;
        }
        let code = &code[node.start_byte()..node.end_byte()];
        code.starts_with(b"/// cbindgen:")
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Rust::SourceFile
                | Rust::FunctionItem
                | Rust::ImplItem
                | Rust::TraitItem
                | Rust::ClosureExpression
        )
    }

    fn is_func(node: &Node) -> bool {
        node.kind_id() == Rust::FunctionItem
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Rust::ClosureExpression
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Rust::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Rust::LPAREN | Rust::COMMA | Rust::RPAREN | Rust::PIPE | Rust::AttributeItem
        )
    }

    fn is_string(node: &Node) -> bool {
        node.kind_id() == Rust::StringLiteral || node.kind_id() == Rust::RawStringLiteral
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        if node.kind_id() != Rust::IfExpression {
            return false;
        }
        if let Some(parent) = node.parent() {
            return parent.kind_id() == Rust::ElseClause;
        }
        false
    }

    #[inline]
    fn is_primitive(id: u16) -> bool {
        matches!(
            id.into(),
            Rust::PrimitiveType
                | Rust::PrimitiveType2
                | Rust::PrimitiveType3
                | Rust::PrimitiveType4
                | Rust::PrimitiveType5
                | Rust::PrimitiveType6
                | Rust::PrimitiveType7
                | Rust::PrimitiveType8
                | Rust::PrimitiveType9
                | Rust::PrimitiveType10
                | Rust::PrimitiveType11
                | Rust::PrimitiveType12
                | Rust::PrimitiveType13
                | Rust::PrimitiveType14
                | Rust::PrimitiveType15
                | Rust::PrimitiveType16
                | Rust::PrimitiveType17
        )
    }

    /// Skip the subtree when `node` is a `mod`, `fn`, `impl`,
    /// `trait`, `const`, or `static` item marked test-only by an
    /// outer or inner attribute (`#[test]`, `#[cfg(test)]`,
    /// `#[tokio::test]`, `#![cfg(test)]`, …). The runtime guard
    /// in `spaces::metrics_with_options` only consults this hook
    /// when the caller opts in via `MetricsOptions::exclude_tests`,
    /// so the default `metrics()` entry point is unaffected.
    fn should_skip_subtree(node: &Node, code: &[u8]) -> bool {
        if !matches!(
            node.kind_id().into(),
            Rust::ModItem
                | Rust::FunctionItem
                | Rust::ImplItem
                | Rust::TraitItem
                | Rust::ConstItem
                | Rust::StaticItem
        ) {
            return false;
        }
        rust_item_is_test_only(node, code)
    }
}

impl Checker for GoCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Go::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Go::SourceFile | Go::FunctionDeclaration | Go::MethodDeclaration | Go::FuncLiteral
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Go::FunctionDeclaration | Go::MethodDeclaration
        )
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Go::FuncLiteral
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Go::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(node.kind_id().into(), Go::LPAREN | Go::COMMA | Go::RPAREN)
    }

    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Go::InterpretedStringLiteral | Go::RawStringLiteral
        )
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Go::IfStatement
            && node
                .parent()
                .is_some_and(|p| p.kind_id() == Go::IfStatement)
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for KotlinCode {
    fn is_comment(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LineComment | Kotlin::BlockComment
        )
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::SourceFile | Kotlin::ClassDeclaration | Kotlin::ObjectDeclaration
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::FunctionDeclaration | Kotlin::SecondaryConstructor
        )
    }

    fn is_closure(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LambdaLiteral | Kotlin::AnonymousFunction
        )
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Kotlin::CallExpression
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::LPAREN | Kotlin::COMMA | Kotlin::RPAREN
        )
    }

    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Kotlin::StringLiteral | Kotlin::MultilineStringLiteral
        )
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // tree-sitter-kotlin models `else if` as an `else` keyword sibling
        // followed by an `if_expression`, not a wrapping clause node.
        node.kind_id() == Kotlin::IfExpression
            && node
                .previous_sibling()
                .is_some_and(|prev| prev.kind_id() == Kotlin::Else)
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for PerlCode {
    fn is_comment(node: &Node) -> bool {
        matches!(node.kind_id().into(), Perl::Comments | Perl::PodStatement)
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::SourceFile
                | Perl::FunctionDefinition
                | Perl::FunctionDefinitionWithoutSub
                | Perl::AnonymousFunction
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::FunctionDefinition | Perl::FunctionDefinitionWithoutSub
        )
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Perl::AnonymousFunction
    }

    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::CallExpressionWithSpacedArgs
                | Perl::CallExpressionWithSub
                | Perl::CallExpressionWithArgsWithBrackets
                | Perl::CallExpressionWithVariable
                | Perl::CallExpressionWithBareword
                | Perl::CallExpressionRecursive
                | Perl::MethodInvocation
        )
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::LPAREN | Perl::COMMA | Perl::RPAREN | Perl::FatComma
        )
    }

    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Perl::StringSingleQuoted
                | Perl::StringDoubleQuoted
                | Perl::StringQQuoted
                | Perl::StringQqQuoted
                | Perl::BacktickQuoted
                | Perl::CommandQxQuoted
        )
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // tree-sitter-perl emits `elsif_clause` as a direct child of the
        // surrounding `if_statement` (not as a wrapper around a nested
        // `if`), so the clause node itself is the else-if.
        node.kind_id() == Perl::ElsifClause
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for LuaCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Lua::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Lua::Chunk
                | Lua::FunctionDeclaration
                | Lua::FunctionDeclaration2
                | Lua::FunctionDeclaration3
                | Lua::FunctionDefinition
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Lua::FunctionDeclaration | Lua::FunctionDeclaration2 | Lua::FunctionDeclaration3
        )
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Lua::FunctionDefinition
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Lua::FunctionCall
    }

    fn is_non_arg(node: &Node) -> bool {
        // NOTE: `impl NArgs for LuaCode` overrides `compute` with a positive
        // filter on `Identifier | VarargExpression` and never calls `is_non_arg`.
        // This implementation satisfies the trait contract but is unused for NArgs.
        matches!(
            node.kind_id().into(),
            Lua::LPAREN | Lua::COMMA | Lua::RPAREN
        )
    }

    fn is_string(node: &Node) -> bool {
        node.kind_id() == Lua::String
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // Lua uses a dedicated elseif_statement node rather than nesting a
        // second if_statement inside the outer one (as Go does).
        node.kind_id() == Lua::ElseifStatement
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for BashCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Bash::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Bash::Program | Bash::FunctionDefinition
        )
    }

    fn is_func(node: &Node) -> bool {
        node.kind_id() == Bash::FunctionDefinition
    }

    fn is_closure(_node: &Node) -> bool {
        false
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Bash::Command
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Bash::LPAREN | Bash::RPAREN | Bash::COMMA | Bash::SEMI
        )
    }

    fn is_string(node: &Node) -> bool {
        // tree-sitter-bash 0.25.1 only emits the `heredoc_body`
        // parser-node symbol (`HeredocBody2`) in observed parse trees;
        // the duplicate `HeredocBody` entry plus the hidden
        // `_heredoc_body` (`HeredocBody3`) and `_simple_heredoc_body`
        // (`SimpleHeredocBody`) rules do not surface, so they are
        // intentionally omitted here.
        matches!(
            node.kind_id().into(),
            Bash::String
                | Bash::RawString
                | Bash::AnsiCString
                | Bash::TranslatedString
                | Bash::HeredocBody2
        )
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Bash::ElifClause
    }

    fn is_primitive(_id: u16) -> bool {
        false
    }
}

impl Checker for TclCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Tcl::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(node.kind_id().into(), Tcl::SourceFile | Tcl::Procedure)
    }

    fn is_func(node: &Node) -> bool {
        node.kind_id() == Tcl::Procedure
    }

    // Tcl closures (`apply`) are ordinary commands; the grammar has no distinct closure node.
    fn is_closure(_: &Node) -> bool {
        false
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Tcl::Command
    }

    // Tcl arguments are whitespace-separated; no punctuation to exclude.
    fn is_non_arg(_: &Node) -> bool {
        false
    }

    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Tcl::QuotedWord | Tcl::BracedWord | Tcl::BracedWordSimple
        )
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        // Tcl grammar has a dedicated `elseif` named node, not a nested `if`.
        node.kind_id() == Tcl::Elseif
    }

    fn is_primitive(_: u16) -> bool {
        false
    }
}

impl Checker for PhpCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Php::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
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

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::FunctionDefinition | Php::MethodDeclaration
        )
    }

    fn is_closure(node: &Node) -> bool {
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

    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::String
                | Php::EncapsedString
                | Php::Heredoc
                | Php::Nowdoc
                | Php::ShellCommandExpression
        )
    }

    #[inline]
    fn is_else_if(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Php::ElseIfClause | Php::ElseIfClause2
        )
    }

    fn is_primitive(_: u16) -> bool {
        false
    }
}

impl Checker for ElixirCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Elixir::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    // Elixir has no syntactic function-definition node: `def`/`defp` are
    // ordinary `Call` nodes with the macro identifier in the `target`
    // field. Distinguishing them would require source-text inspection at
    // every `is_func_space` call, which the trait does not support, so
    // we treat the file root and explicit anonymous functions as the
    // only function spaces.
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Elixir::Source | Elixir::AnonymousFunction
        )
    }

    fn is_func(_: &Node) -> bool {
        false
    }

    fn is_closure(node: &Node) -> bool {
        node.kind_id() == Elixir::AnonymousFunction
    }

    fn is_call(node: &Node) -> bool {
        node.kind_id() == Elixir::Call
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Elixir::LPAREN | Elixir::LPAREN2 | Elixir::RPAREN | Elixir::COMMA
        )
    }

    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Elixir::String | Elixir::Charlist | Elixir::Sigil
        )
    }

    // Elixir lacks an `else if` chain construct. Multi-way branching uses
    // `cond do ... end` (a `Call` whose `do_block` holds many
    // `stab_clause`s, each `+1` nesting in cognitive) or nested
    // `if/else`. No tail-recursive chain to collapse here.
    #[inline]
    fn is_else_if(_: &Node) -> bool {
        false
    }

    fn is_primitive(_: u16) -> bool {
        false
    }
}

impl Checker for RubyCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Ruby::Comment
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Ruby::Program
                | Ruby::Method
                | Ruby::SingletonMethod
                | Ruby::Lambda
                | Ruby::Block
                | Ruby::DoBlock
                | Ruby::Class
                | Ruby::SingletonClass
                | Ruby::Module
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(node.kind_id().into(), Ruby::Method | Ruby::SingletonMethod)
    }

    fn is_closure(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Ruby::Lambda | Ruby::Block | Ruby::DoBlock
        )
    }

    // tree-sitter-ruby 0.23.1 emits four aliased visible variants of the
    // `call` rule (`Call`, `Call2`, `Call3`, `Call4`); `Call5` ("_call")
    // is the hidden inner production and does not surface.
    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Ruby::Call | Ruby::Call2 | Ruby::Call3 | Ruby::Call4
        )
    }

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

    // Mirrors the string-literal set preserved verbatim by
    // `Alterator::alterate`. `HeredocBeginning` is the `<<EOF` marker
    // token rather than a literal body and is intentionally excluded.
    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Ruby::String
                | Ruby::ChainedString
                | Ruby::BareString
                | Ruby::Subshell
                | Ruby::Regex
                | Ruby::HeredocBody
                | Ruby::DelimitedSymbol
                | Ruby::SimpleSymbol
                | Ruby::StringArray
                | Ruby::SymbolArray
                | Ruby::Character
        )
    }

    // tree-sitter-ruby exposes `elsif` as its own named clause node, so the
    // dedicated-clause-node strategy applies here (same as Lua/Bash/PHP).
    #[inline]
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Ruby::Elsif
    }

    fn is_primitive(_: u16) -> bool {
        false
    }
}

impl Checker for GroovyCode {
    fn is_comment(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::LineComment | Groovy::BlockComment
        )
    }

    fn is_useful_comment(_: &Node, _: &[u8]) -> bool {
        false
    }

    fn is_func_space(node: &Node) -> bool {
        // Mirrors `impl Checker for JavaCode` exactly. `EnumDeclaration`,
        // `AnnotationTypeDeclaration`, and `RecordDeclaration` are
        // intentionally excluded for parity with Java's classification —
        // counting them as class-shaped spaces would make identical-shape
        // sources disagree on Npa/Npm/Wmc between languages.
        matches!(
            node.kind_id().into(),
            Groovy::Program | Groovy::ClassDeclaration | Groovy::InterfaceDeclaration
        )
    }

    fn is_func(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::MethodDeclaration | Groovy::ConstructorDeclaration | Groovy::FunctionDefinition
        )
    }

    fn is_closure(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::Closure | Groovy::LambdaExpression
        )
    }

    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::MethodInvocation
                | Groovy::JuxtFunctionCall
                | Groovy::ObjectCreationExpression
                | Groovy::ExplicitConstructorInvocation
        )
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::LPAREN | Groovy::COMMA | Groovy::RPAREN
        )
    }

    // `StringLiteral2` is the aliased lexer variant of the same rule.
    fn is_string(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::StringLiteral | Groovy::StringLiteral2 | Groovy::CharacterLiteral
        )
    }

    // tree-sitter-groovy inherits Java's `if_statement` shape: the `else`
    // keyword token is emitted inline inside the outer if_statement and
    // the inner `if_statement` follows it as a sibling.
    #[inline]
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Groovy::IfStatement
            && node
                .previous_sibling()
                .is_some_and(|prev| prev.kind_id() == Groovy::Else)
    }

    fn is_primitive(_: u16) -> bool {
        false
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use super::*;
    use crate::count::count;
    use crate::langs::BashParser;
    use std::path::PathBuf;

    fn parse(source: &str) -> BashParser {
        BashParser::new(source.as_bytes().to_vec(), &PathBuf::from("test.sh"), None)
    }

    fn count_strings(source: &str) -> usize {
        count(&parse(source), &["string".to_string()]).0
    }

    // `count`'s filter parser accepts a numeric string as a `kind_id` match
    // (parser.rs `get_filters`), so `has_kind` reuses the same primitive.
    fn has_kind(source: &str, kind_id: u16) -> bool {
        count(&parse(source), &[kind_id.to_string()]).0 > 0
    }

    #[test]
    fn bash_is_string_excludes_word_tokens() {
        // `echo hello world` produces three Word nodes — none of them are
        // string literals. Regression for #44 (Word must not match
        // is_string).
        assert_eq!(count_strings("echo hello world\n"), 0);
        assert_eq!(
            count_strings("if [ -f file.txt ]; then cat file.txt; fi\n"),
            0
        );
    }

    #[test]
    fn bash_is_string_matches_quoted_literals() {
        // Regular double-quoted string -> `string` (Bash::String).
        assert_eq!(count_strings("echo \"double\"\n"), 1);
        // Single-quoted string -> `raw_string` (Bash::RawString).
        assert_eq!(count_strings("echo 'single'\n"), 1);
        // ANSI-C quoting -> `ansi_c_string` (Bash::AnsiCString).
        assert_eq!(count_strings("echo $'ansi-c'\n"), 1);
    }

    #[test]
    fn bash_is_string_matches_translated_string() {
        // tree-sitter-bash only emits a visible `translated_string` node
        // in assignment-style contexts; in command arguments the `$"..."`
        // tokenizes as `$` plus a regular `string`. Use an assignment so
        // the wrapper actually appears in the AST.
        let src = "x=$\"translated\"\n";
        assert!(
            has_kind(src, Bash::TranslatedString as u16),
            "expected a translated_string node in {src:?}"
        );
        // The wrapper plus its inner `string` child both match is_string,
        // so count is 2.
        assert_eq!(count_strings(src), 2);
    }

    #[test]
    fn bash_is_string_matches_heredoc_bodies() {
        // Plain heredoc body.
        assert_eq!(
            count_strings("cat <<EOF\nhello world\nEOF\n"),
            1,
            "heredoc body should be counted as a string literal"
        );
        // Quoted-tag heredoc disables expansions but is still a string.
        assert_eq!(
            count_strings("cat <<'EOF'\nliteral $not_expanded\nEOF\n"),
            1
        );
        // Heredoc with an embedded expansion still yields exactly one
        // body node (parallel to a JS template string with `${x}`).
        assert_eq!(count_strings("cat <<EOF\nhi $name\nEOF\n"), 1);
    }

    // Walk the AST and return the first node whose `kind_id` equals
    // `target`. Used by the `is_else_if` tests below to fish a
    // specific node out of the parse tree without depending on the
    // `count` helper above.
    fn find_first_kind<P: ParserTrait>(parser: &P, target: u16) -> Option<Node<'_>> {
        let mut stack = vec![parser.get_root()];
        while let Some(node) = stack.pop() {
            if node.kind_id() == target {
                return Some(node);
            }
            for i in (0..node.child_count()).rev() {
                if let Some(c) = node.child(i) {
                    stack.push(c);
                }
            }
        }
        None
    }

    #[test]
    fn groovy_is_else_if_recognises_else_followed_by_if() {
        // Direct assertion that `GroovyCode::is_else_if` returns true
        // for an `if_statement` whose previous sibling is the `else`
        // token. Defends the sibling-token strategy against accidental
        // regression to a `false` stub (lesson 10, #115 / #239).
        let src = "if (x) { } else if (y) { } else { }";
        let parser =
            GroovyParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.groovy"), None);
        let outer =
            find_first_kind(&parser, Groovy::IfStatement as u16).expect("outer if_statement");
        // Locate the inner if_statement (in the `alternative` slot of
        // the outer if, after the `else` token).
        let mut inner: Option<Node> = None;
        for i in 0..outer.child_count() {
            if let Some(c) = outer.child(i)
                && c.kind_id() == Groovy::IfStatement as u16
            {
                inner = Some(c);
                break;
            }
        }
        let inner = inner.expect("expected an inner if_statement");
        assert!(
            GroovyCode::is_else_if(&inner),
            "inner if_statement after `else` must be recognised as else-if"
        );
        assert!(
            !GroovyCode::is_else_if(&outer),
            "outer if_statement must not be recognised as else-if"
        );
    }

    #[test]
    fn groovy_is_else_if_false_for_standalone_if() {
        // A bare `if` (no `else` preceding it) must NOT register as
        // an else-if.
        let src = "if (x) { println x }";
        let parser =
            GroovyParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.groovy"), None);
        let node = find_first_kind(&parser, Groovy::IfStatement as u16).expect("if_statement");
        assert!(!GroovyCode::is_else_if(&node));
    }
}
