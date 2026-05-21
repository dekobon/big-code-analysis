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

// Generate an `is_string` impl for a JS-family `Checker` block. The
// MozJS / JavaScript / TypeScript grammars expose `String2` as the
// anonymous `"string"` keyword alias for `String`; TSX additionally
// exposes `String3` (the JSX-attribute string production). The
// alterator flattens these aliases; the generic `string` filter must
// agree (issue #283).
macro_rules! impl_js_family_is_string {
    ($lang:ident $(, $extra:ident)* $(,)?) => {
        fn is_string(node: &Node) -> bool {
            matches!(
                node.kind_id().into(),
                $lang::String | $lang::String2 | $lang::TemplateString
                    $(| $lang::$extra)*
            )
        }
    };
}

#[inline]
fn get_aho_corasick_match(code: &[u8]) -> bool {
    AHO_CORASICK
        .get_or_init(|| AhoCorasick::new(vec![b"<div rustbindgen"]).unwrap())
        .is_match(code)
}

#[doc(hidden)]
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

    /// Source-aware variant of [`is_func_space`]. The default forwards
    /// to the byte-less predicate so languages whose function-space
    /// classification is encoded in distinct grammar productions (Java,
    /// Rust, Python, …) need no override. Languages whose function
    /// boundaries are macro-shaped — Elixir's `def` / `defp` /
    /// `defmacro` / `defmacrop` / `defmodule` — override this to
    /// disambiguate `Call` nodes by their target identifier text
    /// (#275).
    #[inline]
    fn is_func_space_with_code(node: &Node, _code: &[u8]) -> bool {
        Self::is_func_space(node)
    }

    /// Source-aware variant of [`is_func`]. Same rationale as
    /// [`is_func_space_with_code`] (#275).
    #[inline]
    fn is_func_with_code(node: &Node, _code: &[u8]) -> bool {
        Self::is_func(node)
    }

    /// Combined predicate the walker uses to decide whether to promote
    /// `node` to a new function-space frame. Default forwards to
    /// `is_func_with_code || is_func_space_with_code` so existing
    /// languages need no override — each kept the freedom to expose
    /// `is_func` and `is_func_space` as disjoint sets (Rust includes
    /// closures in `is_func_space`, Java keeps lambdas out, …) and
    /// this method preserves that flexibility while letting Elixir
    /// halve its per-`Call` source-text lookups: a single
    /// `elixir_call_keyword` call answers both halves at once
    /// (#310 follow-on perf).
    #[inline]
    fn promotes_to_func_space_with_code(node: &Node, code: &[u8]) -> bool {
        Self::is_func_with_code(node, code) || Self::is_func_space_with_code(node, code)
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

    // Issue #285 contract: every `Cpp::FunctionDefinition*` alias must
    // be enumerated here AND in `is_func`, `get_func_space_name`, and
    // `get_space_kind` (see `src/getter.rs`). Aliased kind_ids
    // 489/491/494 are not emitted by the currently pinned
    // `tree-sitter-mozcpp` parse tables on any input we can construct,
    // so a missing variant won't fail a parse-and-assert test — it
    // will silently drop those nodes from FuncSpace creation the next
    // time a grammar bump starts emitting them (see lesson 2 in
    // `docs/development/lessons_learned.md`).
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Cpp::TranslationUnit
                | Cpp::FunctionDefinition
                | Cpp::FunctionDefinition2
                | Cpp::FunctionDefinition3
                | Cpp::FunctionDefinition4
                | Cpp::StructSpecifier
                | Cpp::ClassSpecifier
                | Cpp::NamespaceDefinition
        )
    }

    // Issue #285 contract: keep this in sync with `is_func_space` and
    // the C++ getters — see comment above `is_func_space`.
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

    // Python models `elif` as a dedicated `elif_clause` node, which is
    // handled directly by cognitive/cyclomatic dispatch as a branch
    // extension — `is_else_if` is intentionally never invoked for
    // `elif_clause` because it is not an `if_statement` and is not in
    // any of the structural kind sets that `count_specific_ancestors`
    // walks for nesting (issue #274).
    //
    // `else: if x: ...` chains also exist — semantically equivalent to
    // `else if` — but the grammar wraps the inner `if_statement` in a
    // `block` node, so the shape is `else_clause → block → if_statement`
    // rather than the direct `else_clause → if_statement` used by
    // C++/JS/TS/TSX/Rust. Match the chained shape by walking through
    // the `block` and requiring the inner `if` to be the block's sole
    // named child; sibling statements would mean a real nested-if, not
    // a chain (issue #276).
    //
    // `block` has two aliased kind_ids in tree-sitter-python
    // (`Block` = 135, `Block2` = 160 — both surface as `"block"`); we
    // accept either per lesson 2 in `docs/development/lessons_learned.md`.
    fn is_else_if(node: &Node) -> bool {
        node.kind_id() == Python::IfStatement
            && node.parent().is_some_and(|parent| {
                matches!(parent.kind_id().into(), Python::Block | Python::Block2)
                    && parent.children().filter(Node::is_named).count() == 1
                    && parent
                        .parent()
                        .is_some_and(|gp| gp.kind_id() == Python::ElseClause)
            })
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

    // `EnumDeclaration`, `RecordDeclaration`, and `AnnotationTypeDeclaration`
    // are class-like declarations that can contain fields and methods,
    // so they open a class space alongside `ClassDeclaration` /
    // `InterfaceDeclaration` (issue #280). Without them, `Npa`/`Npm`/`Wmc`
    // never see their bodies as class scopes and silently produce zero
    // counts. Annotation types map to `Interface` in `get_space_kind`
    // (their elements are abstract methods at the bytecode level);
    // enums and records map to `Class`.
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Java::Program
                | Java::ClassDeclaration
                | Java::InterfaceDeclaration
                | Java::EnumDeclaration
                | Java::RecordDeclaration
                | Java::AnnotationTypeDeclaration
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

    #[inline]
    fn is_primitive(id: u16) -> bool {
        // Without this, every `PredefinedType` keyword (`int`, `string`,
        // `bool`, `object`, …) collapses into a single Halstead operator
        // because they share one `kind_id`. Returning `true` here routes
        // them through the lexeme-keyed `primitive_operators` map so
        // distinct keywords count as distinct operators (issue #286).
        id == Csharp::PredefinedType as u16
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

    impl_js_family_is_string!(Mozjs);

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

    impl_js_family_is_string!(Javascript);

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

    impl_js_family_is_string!(Typescript);

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

    impl_js_family_is_string!(Tsx, String3);

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
    // Path-form `#[test]`, `#[tokio::test]`, `#[ext::module::test(args)]`, etc.
    // are detected without entering the cfg predicate walker.
    // `cfg(...)` predicates extract the inner predicate text and walk
    // the predicate tree so `test` is matched regardless of its
    // position inside `all(...)` / `any(...)` (#278). Production
    // builds still see items gated on `not(test)`, so the walker
    // refuses to descend into a `not(...)` operand.
    let matches_test = |s: &str| {
        matches!(s, "test" | "rstest" | "wasm_bindgen_test" | "test_case")
            || s.ends_with("::test")
            || s.contains("::test(")
            || cfg_inner(s).is_some_and(cfg_predicate_marks_test)
    };

    let trimmed = body.trim();
    if matches_test(trimmed) {
        return true;
    }
    // Slow path: tolerate unusual spacing like `# [ cfg ( test ) ]`
    // by collapsing all ASCII whitespace before re-checking. Only
    // worthwhile if the input actually has spacing inside the body.
    if trimmed.bytes().any(|b| b.is_ascii_whitespace()) {
        let compact: String = trimmed
            .bytes()
            .filter(|b| !b.is_ascii_whitespace())
            .map(char::from)
            .collect();
        return matches_test(&compact);
    }
    false
}

/// Return the inner predicate text of a `cfg(...)` attribute body,
/// stripping the `cfg(` prefix and matching `)`. Whitespace inside
/// is tolerated; callers receive a slice with surrounding spacing
/// preserved so the predicate walker can re-split on commas / parens.
fn cfg_inner(body: &str) -> Option<&str> {
    let rest = body.trim_start().strip_prefix("cfg")?.trim_start();
    let after_open = rest.strip_prefix('(')?;
    let inner = after_open.strip_suffix(')')?;
    Some(inner)
}

fn cfg_predicate_marks_test(pred: &str) -> bool {
    let trimmed = pred.trim();
    if trimmed == "test" {
        return true;
    }
    // `all(...)` and `any(...)` use the same "contains a `test`
    // operand" rule here. Strictly, `any(test, foo)` is over-broad
    // (the item is included in production when `foo` holds), but the
    // pre-#278 code treated both identically and the issue spec
    // preserves that behavior.
    if let Some(rest) = trimmed
        .strip_prefix("all")
        .or_else(|| trimmed.strip_prefix("any"))
        && let Some(args) = rest.trim_start().strip_prefix('(')
        && let Some(args) = args.strip_suffix(')')
    {
        return cfg_args_any_marks_test(args);
    }
    // Bare comma-separated predicate lists like `cfg(test, foo)`
    // — pre-#278 callers relied on this form being treated as
    // `cfg(all(test, foo))`. Skip if no top-level comma exists, so a
    // single ident does not accidentally fall through.
    if cfg_split_top_level_args(trimmed).nth(1).is_some() {
        return cfg_args_any_marks_test(trimmed);
    }
    false
}

/// Iterator over the comma-separated arguments of a cfg predicate
/// body, splitting at top-level commas only (commas inside nested
/// parens belong to a child predicate). Single-pass byte scan.
fn cfg_split_top_level_args(args: &str) -> impl Iterator<Item = &str> {
    let mut depth = 0_i32;
    let mut start = 0_usize;
    let mut done = false;
    let bytes = args.as_bytes();
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let mut i = start;
        while i < bytes.len() {
            match bytes[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b',' if depth == 0 => {
                    let slice = &args[start..i];
                    start = i + 1;
                    return Some(slice);
                }
                _ => {}
            }
            i += 1;
        }
        done = true;
        Some(&args[start..])
    })
}

/// Walk a comma-separated argument list of a cfg predicate and return
/// true if any operand marks the item as test-only. Key=value forms
/// like `feature = "test"` never match.
fn cfg_args_any_marks_test(args: &str) -> bool {
    cfg_split_top_level_args(args).any(cfg_arg_marks_test)
}

/// Classify a single cfg predicate operand. Bare `test` matches;
/// `not(...)` never matches (its presence flips the gate); `all(...)`
/// and `any(...)` recurse; everything else (including `feature =
/// "test"`, plain idents, key=value pairs) does not match.
fn cfg_arg_marks_test(arg: &str) -> bool {
    let arg = arg.trim();
    if arg == "test" {
        return true;
    }
    // `not(...)` short-circuits: we do not look inside, because
    // `not(test)` excludes the item from test builds.
    if let Some(rest) = arg.strip_prefix("not")
        && let rest = rest.trim_start()
        && rest.starts_with('(')
        && rest.ends_with(')')
    {
        return false;
    }
    cfg_predicate_marks_test(arg)
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
        // `HeredocBodyStatement` wraps the heredoc body text (and any
        // `Interpolation` children) that appears as a top-level
        // statement after the heredoc-introducing `<<TAG`; it is the
        // visible literal node and is treated as a string here, the
        // same way Bash's `heredoc_body` is treated as a string.
        matches!(
            node.kind_id().into(),
            Perl::StringSingleQuoted
                | Perl::StringDoubleQuoted
                | Perl::StringQQuoted
                | Perl::StringQqQuoted
                | Perl::BacktickQuoted
                | Perl::CommandQxQuoted
                | Perl::HeredocBodyStatement
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
        // `String` is the named single-quoted literal; `String2` and
        // `String3` are aliased kind_ids that the language enum also
        // maps to `"string"` (`String2` is the `string` type keyword
        // and `String3` is the hidden `_string` supertype that covers
        // any string literal). Include all three so generic
        // string-filtering stays consistent with `get_op_type` and the
        // `Alterator` text-preservation arm (issue #288).
        matches!(
            node.kind_id().into(),
            Php::String
                | Php::String2
                | Php::String3
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

    // Elixir has no syntactic function-definition node: `def`/`defp` /
    // `defmacro`/`defmacrop` / `defmodule` are ordinary `Call` nodes
    // with the macro identifier in the `target` field. The byte-less
    // [`is_func_space`] and [`is_func`] cannot distinguish them from
    // any other `Call`, so they conservatively return zero (only the
    // file root and explicit anonymous functions surface as func
    // spaces). The text-aware [`is_func_space_with_code`] /
    // [`is_func_with_code`] overrides below promote the macro-shaped
    // declarations to first-class function / class spaces (#275). The
    // walker passes the source bytes through, so the metrics
    // attributed to a `def`'s body now correctly nest under a Function
    // space and `Wmc` / `Npm` / `Npa` see a `defmodule` Class.
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Elixir::Source | Elixir::AnonymousFunction
        )
    }

    fn is_func(_: &Node) -> bool {
        false
    }

    fn is_func_space_with_code(node: &Node, code: &[u8]) -> bool {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_class_macro, elixir_is_inside_quote_block,
            elixir_is_method_macro,
        };
        if Self::is_func_space(node) {
            return true;
        }
        let Some(kw) = elixir_call_keyword(node, code) else {
            return false;
        };
        if elixir_is_class_macro(kw) {
            return true;
        }
        // A `def` / `defp` / `defmacro` / `defmacrop` nested inside a
        // `quote do … end` template does NOT declare a method of any
        // enclosing module — the syntax tree there is a code template
        // emitted later, on macro expansion (#310).
        elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code)
    }

    fn is_func_with_code(node: &Node, code: &[u8]) -> bool {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_inside_quote_block, elixir_is_method_macro,
        };
        let Some(kw) = elixir_call_keyword(node, code) else {
            return false;
        };
        elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code)
    }

    fn promotes_to_func_space_with_code(node: &Node, code: &[u8]) -> bool {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_class_macro, elixir_is_inside_quote_block,
            elixir_is_method_macro,
        };
        // Cheap path: byte-less `is_func_space` matches `Source` and
        // `AnonymousFunction` without needing any text inspection.
        if Self::is_func_space(node) {
            return true;
        }
        // Otherwise one `elixir_call_keyword` lookup answers the
        // combined question instead of the two the default impl would
        // have made via `is_func_with_code || is_func_space_with_code`.
        let Some(kw) = elixir_call_keyword(node, code) else {
            return false;
        };
        if elixir_is_class_macro(kw) {
            return true;
        }
        elixir_is_method_macro(kw) && !elixir_is_inside_quote_block(node, code)
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

    // Mirrors `impl Checker for JavaCode` exactly: `EnumDeclaration`,
    // `RecordDeclaration`, and `AnnotationTypeDeclaration` open class
    // spaces so `Npa`/`Npm`/`Wmc` walk their bodies. Cross-language parity
    // with Java is the point — identical-shape sources must agree on
    // class-shaped metrics (issue #280).
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Groovy::Program
                | Groovy::ClassDeclaration
                | Groovy::InterfaceDeclaration
                | Groovy::EnumDeclaration
                | Groovy::RecordDeclaration
                | Groovy::AnnotationTypeDeclaration
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
    use crate::langs::{
        BashParser, JavascriptParser, MozjsParser, PhpParser, TsxParser, TypescriptParser,
    };
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

    // ===== PHP `is_string` regression tests (issue #288) =====

    fn parse_php(source: &str) -> PhpParser {
        PhpParser::new(source.as_bytes().to_vec(), &PathBuf::from("test.php"), None)
    }

    fn count_php_strings(source: &str) -> usize {
        count(&parse_php(source), &["string".to_string()]).0
    }

    #[test]
    fn php_is_string_matches_single_quoted_literal() {
        // `Php::String` is the named single-quoted literal. Inert
        // single-quoted strings have always been matched; this anchors
        // the baseline before exercising the alias kinds.
        assert_eq!(count_php_strings("<?php $x = 'single';"), 1);
    }

    #[test]
    fn php_is_string_matches_encapsed_heredoc_nowdoc_shell() {
        // `EncapsedString` (double-quoted), `Heredoc`, `Nowdoc`, and
        // `ShellCommandExpression` (backticks) must all match
        // `is_string`. Pre-#288 the alterator/checker arms were almost
        // aligned for these named literals — this test locks in the
        // shape.
        assert_eq!(count_php_strings("<?php $x = \"double\";"), 1);
        assert_eq!(
            count_php_strings("<?php $x = <<<EOT\nbody\nEOT;\n"),
            1,
            "heredoc should match is_string"
        );
        assert_eq!(
            count_php_strings("<?php $x = <<<'EOT'\nbody\nEOT;\n"),
            1,
            "nowdoc should match is_string"
        );
        assert_eq!(
            count_php_strings("<?php $x = `ls`;"),
            1,
            "shell command (backtick) should match is_string"
        );
    }

    #[test]
    fn php_is_string_matches_string_alias_kinds() {
        // Regression for #288. Before the fix, only `Php::String`
        // (kind_id 368, the named single-quoted literal) matched
        // `is_string`. The `Php::String2` (anonymous `string` type
        // keyword, kind_id 25) and `Php::String3` (the hidden `_string`
        // supertype, kind_id 378) alias kinds — both of which the
        // language enum maps to `"string"` — were missed. A function
        // with a `: string` return type produces a `Php::String2`
        // anonymous-keyword node, so we exercise it here. The named
        // `Php::String` literal in the body matches too.
        let src = "<?php function f(): string { return 'x'; }";
        // Two string-matching nodes: the `string` return-type keyword
        // (Php::String2) and the `'x'` literal (Php::String). Pre-fix
        // only the literal matched (count would be 1).
        assert_eq!(count_php_strings(src), 2);
    }

    // ===== JS-family `is_string` regression tests (issue #283) =====

    // Walk the AST and return true iff any node has `kind_id == target`.
    // Used to confirm an alias kind actually surfaces in a real parse
    // before asserting it routes through `is_string`.
    fn ast_has_kind_id<P: ParserTrait>(parser: &P, target: u16) -> bool {
        let mut stack = vec![parser.get_root()];
        while let Some(node) = stack.pop() {
            if node.kind_id() == target {
                return true;
            }
            for i in (0..node.child_count()).rev() {
                if let Some(c) = node.child(i) {
                    stack.push(c);
                }
            }
        }
        false
    }

    // For each language, count nodes whose kind_id is exactly `target`
    // *and* simultaneously match `is_string`. A non-zero result proves
    // both that the alias appears in the parse and that the checker
    // accepts it. Pre-fix this would be zero for the alias kinds.
    fn count_string_matches_for_kind<P: ParserTrait, F: Fn(&Node) -> bool>(
        parser: &P,
        target: u16,
        is_string: F,
    ) -> usize {
        let mut stack = vec![parser.get_root()];
        let mut hits = 0;
        while let Some(node) = stack.pop() {
            if node.kind_id() == target && is_string(&node) {
                hits += 1;
            }
            for i in (0..node.child_count()).rev() {
                if let Some(c) = node.child(i) {
                    stack.push(c);
                }
            }
        }
        hits
    }

    #[test]
    fn javascript_is_string_matches_string2_alias() {
        // `Javascript::String2` (kind_id 221) aliases to `"string"`
        // (see `language_javascript.rs`). The alterator already
        // flattens it (#119); the generic `string` filter must agree
        // (#283). Use a source mix that exercises both the primary
        // `String` and the anonymous `String2` productions.
        let src = "const a = 'single';\nconst b = \"double\";\nimport \"m\";\n";
        let parser = JavascriptParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.js"), None);
        // First confirm String2 actually surfaces in this parse —
        // otherwise the assertion below would be vacuously true.
        assert!(
            ast_has_kind_id(&parser, Javascript::String2 as u16),
            "expected Javascript::String2 to appear in the parse",
        );
        // Then assert every String2 node matches is_string.
        assert!(
            count_string_matches_for_kind(
                &parser,
                Javascript::String2 as u16,
                JavascriptCode::is_string,
            ) > 0,
            "Javascript::String2 nodes must match is_string",
        );
    }

    #[test]
    fn mozjs_is_string_matches_string2_alias() {
        // Parallel coverage for the MozJS dialect; same `String2`
        // alias as upstream JavaScript (kind_id 220 here).
        let src = "const a = 'single';\nconst b = \"double\";\nimport \"m\";\n";
        let parser = MozjsParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.js"), None);
        assert!(
            ast_has_kind_id(&parser, Mozjs::String2 as u16),
            "expected Mozjs::String2 to appear in the parse",
        );
        assert!(
            count_string_matches_for_kind(&parser, Mozjs::String2 as u16, MozjsCode::is_string) > 0,
            "Mozjs::String2 nodes must match is_string",
        );
    }

    #[test]
    fn typescript_is_string_matches_string2_alias() {
        // TypeScript exposes both the primary `String` literal and
        // a `String2` alias (kind_id 135). The latter sits among the
        // type-keyword tokens in the enum, so a `: string` annotation
        // is a reliable producer.
        let src = "const a: string = 'x';\nfunction f(): string { return 'y'; }\n";
        let parser = TypescriptParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.ts"), None);
        assert!(
            ast_has_kind_id(&parser, Typescript::String2 as u16),
            "expected Typescript::String2 to appear in the parse",
        );
        assert!(
            count_string_matches_for_kind(
                &parser,
                Typescript::String2 as u16,
                TypescriptCode::is_string,
            ) > 0,
            "Typescript::String2 nodes must match is_string",
        );
    }

    #[test]
    fn tsx_is_string_matches_string2_and_string3_aliases() {
        // TSX uniquely carries two anonymous `"string"` aliases:
        // `String3` (kind_id 141, the type-annotation keyword) and
        // `String2` (kind_id 261). Both must appear in this fixture:
        // the `: string` annotation produces `String3`, and the
        // `'x'` / `"y"` / `"m"` / `"c"` literals produce `String2`.
        // Asserting presence of both *before* checking `is_string`
        // ensures a future grammar bump that stops emitting either
        // alias fails loudly here rather than silently dropping
        // coverage (which would invalidate the regression for #283).
        let src = "const a: string = 'x';\n\
                   const b = \"y\";\n\
                   import \"m\";\n\
                   const el = <div className=\"c\">{\"t\"}</div>;\n";
        let parser = TsxParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.tsx"), None);
        assert!(
            ast_has_kind_id(&parser, Tsx::String3 as u16),
            "expected Tsx::String3 (type-keyword `string`) in the parse",
        );
        assert!(
            ast_has_kind_id(&parser, Tsx::String2 as u16),
            "expected Tsx::String2 (string-literal alias) in the parse",
        );
        assert!(
            count_string_matches_for_kind(&parser, Tsx::String3 as u16, TsxCode::is_string) > 0,
            "Tsx::String3 nodes must match is_string",
        );
        assert!(
            count_string_matches_for_kind(&parser, Tsx::String2 as u16, TsxCode::is_string) > 0,
            "Tsx::String2 nodes must match is_string",
        );
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

    fn parse_python(src: &str) -> PythonParser {
        PythonParser::new(src.as_bytes().to_vec(), &PathBuf::from("test.py"), None)
    }

    // Walk the AST and return every node whose `kind_id` equals `target`,
    // in DFS pre-order. Used by the Python `is_else_if` tests below to
    // distinguish the outer if from the inner one in an `else: if` chain.
    fn find_all_kinds<P: ParserTrait>(parser: &P, target: u16) -> Vec<Node<'_>> {
        let mut out = Vec::new();
        let mut stack = vec![parser.get_root()];
        while let Some(node) = stack.pop() {
            if node.kind_id() == target {
                out.push(node);
            }
            for i in (0..node.child_count()).rev() {
                if let Some(c) = node.child(i) {
                    stack.push(c);
                }
            }
        }
        out
    }

    #[test]
    fn python_is_else_if_recognises_if_inside_else_clause() {
        // `else: if b:` chains parse as `else_clause → block → if_statement`
        // (the `block` wrapper is Python-specific). `is_else_if` must
        // walk through that wrapper. Regression for #276 (the stub
        // returned `false` unconditionally).
        let src = "if a:\n    pass\nelse:\n    if b:\n        pass\n";
        let parser = parse_python(src);
        let outer =
            find_first_kind(&parser, Python::IfStatement as u16).expect("outer if_statement");
        let inner =
            find_python_if_inside_else_block(&parser).expect("inner if_statement under else");
        assert!(
            PythonCode::is_else_if(&inner),
            "if_statement inside else_clause's block must be recognised as else-if"
        );
        assert!(
            !PythonCode::is_else_if(&outer),
            "outer if_statement must not be recognised as else-if"
        );
    }

    #[test]
    fn python_is_else_if_false_for_standalone_if() {
        // A bare `if` whose parent is the module / function body must
        // NOT register as an else-if.
        let src = "if a:\n    pass\n";
        let parser = parse_python(src);
        let node = find_first_kind(&parser, Python::IfStatement as u16).expect("if_statement");
        assert!(!PythonCode::is_else_if(&node));
    }

    #[test]
    fn python_is_else_if_false_for_outer_if_with_elif_alternative() {
        // `elif` parses as an `ElifClause`, not an `IfStatement`, so the
        // only `IfStatement` in `if … elif …` is the outer one. Its
        // parent is the module / function body, not an `else_clause`.
        // Pins that the presence of `elif` in the AST does not trip
        // `is_else_if` for the outer `if`.
        let src = "if a:\n    pass\nelif b:\n    pass\n";
        let parser = parse_python(src);
        let outer = find_first_kind(&parser, Python::IfStatement as u16).expect("if_statement");
        assert!(!PythonCode::is_else_if(&outer));
    }

    // Return the inner `if_statement` that sits directly inside an
    // `else_clause`'s `block` wrapper, or `None` if no such node exists.
    // Used by tests below instead of relying on `find_all_kinds`'s DFS
    // pre-order to land at `ifs[1]`.
    fn find_python_if_inside_else_block(parser: &PythonParser) -> Option<Node<'_>> {
        find_all_kinds(parser, Python::IfStatement as u16)
            .into_iter()
            .find(|n| {
                n.parent().is_some_and(|p| {
                    matches!(p.kind_id().into(), Python::Block | Python::Block2)
                        && p.parent()
                            .is_some_and(|gp| gp.kind_id() == Python::ElseClause)
                })
            })
    }

    #[test]
    fn python_is_else_if_false_when_else_body_has_siblings() {
        // `else: if b:` followed by another statement at the same indent
        // is a real nested-if, not a chain. The block has 2 named
        // children, so `is_else_if` must return false.
        let src = "if a:\n    pass\nelse:\n    if b:\n        pass\n    pass\n";
        let parser = parse_python(src);
        let inner =
            find_python_if_inside_else_block(&parser).expect("inner if_statement under else");
        assert!(
            !PythonCode::is_else_if(&inner),
            "inner if must NOT be recognised as else-if when its block has siblings"
        );
    }

    // ===== Rust `rust_attribute_marks_test` regression tests (#278) =====

    #[test]
    fn rust_attr_test_marks_bare_test_attribute() {
        // Direct attribute names (and aliases) match without ever
        // entering the cfg predicate walker. Locks in pre-#278
        // behavior so the rewrite does not regress the common case.
        assert!(rust_attribute_marks_test("test"));
        assert!(rust_attribute_marks_test("rstest"));
        assert!(rust_attribute_marks_test("wasm_bindgen_test"));
        assert!(rust_attribute_marks_test("test_case"));
        assert!(rust_attribute_marks_test("tokio::test"));
        assert!(rust_attribute_marks_test(
            "tokio::test(flavor = \"current_thread\")"
        ));
    }

    #[test]
    fn rust_attr_test_marks_cfg_test_variants() {
        // Pre-#278 forms with `test` in the first position must
        // still match.
        assert!(rust_attribute_marks_test("cfg(test)"));
        assert!(rust_attribute_marks_test("cfg(test, foo)"));
        assert!(rust_attribute_marks_test("cfg(all(test, unix))"));
        assert!(rust_attribute_marks_test("cfg(any(test, foo))"));
    }

    #[test]
    fn rust_attr_test_marks_cfg_with_test_not_first() {
        // Regression for #278. `test` was previously required to be
        // the first operand of `all(...)` / `any(...)`. The predicate
        // walker now matches it anywhere.
        assert!(
            rust_attribute_marks_test("cfg(all(unix, test))"),
            "test as second all() operand must mark test-only"
        );
        assert!(
            rust_attribute_marks_test("cfg(any(feature = \"x\", test))"),
            "test as second any() operand must mark test-only"
        );
        // Nested predicate: `any(test, ...)` inside `all(...)` still
        // counts as test-only via recursion.
        assert!(rust_attribute_marks_test(
            "cfg(all(unix, any(test, feature = \"x\")))"
        ));
    }

    #[test]
    fn rust_attr_test_skips_not_test_and_feature_named_test() {
        // `cfg(not(test))` is *production-only*; it must not be
        // treated as test-only or `exclude_tests` would strip
        // production code.
        assert!(!rust_attribute_marks_test("cfg(not(test))"));
        assert!(!rust_attribute_marks_test("cfg(all(unix, not(test)))"));
        // A feature literally named "test" is a string-valued
        // key/value pair, not the bare `test` predicate.
        assert!(!rust_attribute_marks_test("cfg(feature = \"test\")"));
        assert!(!rust_attribute_marks_test(
            "cfg(all(unix, feature = \"test\"))"
        ));
        // Unrelated predicates remain unmatched.
        assert!(!rust_attribute_marks_test("cfg(unix)"));
        assert!(!rust_attribute_marks_test("derive(Debug)"));
        // `all(...)` / `any(...)` with no `test` operand anywhere must
        // not match — guards against an over-eager walker that treats
        // any combinator as test-only regardless of contents.
        assert!(!rust_attribute_marks_test(
            "cfg(all(unix, target_os = \"linux\"))"
        ));
        assert!(!rust_attribute_marks_test("cfg(any(unix, windows))"));
        assert!(!rust_attribute_marks_test(
            "cfg(all(unix, any(feature = \"x\", feature = \"y\")))"
        ));
        // Nested `not(test)` inside `any(...)` is still non-matching;
        // `not(...)` short-circuits at any depth.
        assert!(!rust_attribute_marks_test("cfg(any(unix, not(test)))"));
    }

    #[test]
    fn rust_attr_test_tolerates_internal_whitespace() {
        // The slow path strips ASCII whitespace before re-running
        // both checks, so spaced forms still resolve correctly.
        assert!(rust_attribute_marks_test("cfg( all( unix , test ) )"));
        assert!(!rust_attribute_marks_test("cfg( not ( test ) )"));
    }
}
