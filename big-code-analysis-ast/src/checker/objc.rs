//! `Checker` implementation for Objective-C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for ObjcCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Objc::Comment
    }

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
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

    // A plain C function written inside `@implementation` / `@interface`
    // / `@protocol` is a file-static helper, not a method: no receiver,
    // absent from the method table, unsendable. `npm` already declines
    // to count it (`src/metrics/npm/objc.rs` counts `method_definition`),
    // so `wmc` must not weight it into the container either (#1356) —
    // the C++ `friend` divergence of #1301 in a sibling language.
    //
    // The node kind is the whole predicate, because an Objective-C
    // method is *always* a `method_definition`. Neither shape of parent
    // check that worked for C++ transfers:
    //
    // - `implementation_definition` wraps `function_definition` and
    //   `method_definition` alike, so a parent check alone answers
    //   `true` for every real method and zeroes the class's WMC
    //   (`.claude/rules/grammar-dispatch.md` §6).
    // - A parent-kind *list* cannot be completed. `@interface` and
    //   `@protocol` take `function_definition` as a direct child with no
    //   wrapper, and both also admit `preproc_if` — so a helper inside
    //   `#if` has a parent kind it shares with a file-scope `#if`.
    //
    // `true` at file scope is deliberate and inert: `wmc::Stats::merge`
    // credits a `Function` child only to a `Class` or `Interface`
    // parent. Both aliases ride together per the #285 contract this file
    // follows in `is_func` / `is_func_space`.
    fn is_non_member_function<'a>(
        node: &Node<'a>,
        _code: &[u8],
        _ancestors: Ancestors<'a, '_>,
    ) -> bool {
        matches!(
            node.kind_id().into(),
            Objc::FunctionDefinition | Objc::FunctionDefinition2
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

    // C's `(void)` marker, shared with C++, Mozcpp and Objective-C.
    fn is_empty_param_marker(param: &Node, code: &[u8]) -> bool {
        c_family_void_parameter(param, code)
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
