//! `Checker` implementation for Objective-C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for ObjcCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Objc::Comment
    }

    fn is_useful_comment(node: &Node, code: &[u8]) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    // Objective-C builds on C, so the code spaces are the translation
    // unit, free functions, and ObjC-specific containers: `@interface`
    // / `@protocol` (declarations only → `Interface`), `@implementation`
    // (method bodies → `Class`), and method definitions. `BlockLiteral`
    // (`^{ … }`) is a closure counted by `nom`, not its own space —
    // mirroring how the C++ checker treats `LambdaExpression`. Keep this
    // list in sync with `is_func`, `get_func_space_name`, and
    // `get_space_kind` (see `src/getter.rs`); the `FunctionDefinition2`
    // alias must ride alongside the primary or it is silently dropped
    // from FuncSpace creation (#285, lesson 2).
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Objc::TranslationUnit
                | Objc::FunctionDefinition
                | Objc::FunctionDefinition2
                | Objc::MethodDefinition
                | Objc::ClassInterface
                | Objc::ClassImplementation
                | Objc::ProtocolDeclaration
        )
    }

    // Keep in sync with `is_func_space` and the ObjC getters (#285).
    // A `method_definition` is the `@implementation`-side method with a
    // body; the `@interface`-side `method_declaration` has no body and
    // is therefore not a function.
    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            Objc::FunctionDefinition | Objc::FunctionDefinition2 | Objc::MethodDefinition
        )
    }

    // ObjC blocks `^{ … }` are the language's closures.
    fn is_closure<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        node.kind_id() == Objc::BlockLiteral
    }

    // A C `call_expression` (aliased to `CallExpression2`) or an ObjC
    // message send `[receiver method]` are both calls.
    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Objc::CallExpression | Objc::CallExpression2 | Objc::MessageExpression
        )
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            Objc::LPAREN | Objc::LPAREN2 | Objc::COMMA | Objc::RPAREN
        )
    }

    // ObjC has no raw string literals; `@"…"` parses as `AT` + a plain
    // `string_literal`, so the same string kinds as C apply.
    impl_simple_is_string!(Objc, StringLiteral, ConcatenatedString);

    impl_is_else_if_parent_clause!(Objc, IfStatement, ElseClause);

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        node.kind_id() == Objc::PrimitiveType
    }
}
