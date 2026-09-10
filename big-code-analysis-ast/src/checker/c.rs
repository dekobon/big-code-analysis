//! `Checker` implementation for C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for CCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == C::Comment
    }

    fn is_useful_comment<'a>(node: &Node<'a>, code: &[u8], _ancestors: Ancestors<'a, '_>) -> bool {
        get_aho_corasick_match(&code[node.start_byte()..node.end_byte()])
    }

    // C has no classes/namespaces and no methods, so the only code
    // spaces are the translation unit and function definitions. Struct /
    // union / enum specifiers hold no functions in C, so — unlike the
    // C++ checker — they are deliberately NOT spaces (including them
    // would create empty FuncSpaces for every aggregate type).
    // `FunctionDefinition2` is the grammar's `function_definition` alias
    // (kind_id 197); enumerate it alongside the primary so neither is
    // silently dropped from FuncSpace creation (#285, lesson 2).
    fn is_func_space(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            C::TranslationUnit | C::FunctionDefinition | C::FunctionDefinition2
        )
    }

    // Keep in sync with `is_func_space` and the C getters (#285).
    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(
            node.kind_id().into(),
            C::FunctionDefinition | C::FunctionDefinition2
        )
    }

    // C has no closures/lambdas.
    fn is_closure<'a>(_node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        false
    }

    // Every `call_expression` the pinned grammar emits carries the
    // aliased `CallExpression2` kind_id: the unsuffixed symbol is the
    // grammar's `preproc_call_expression`, which is only ever referenced
    // under an `alias(…, $.call_expression)`, so it never reaches
    // `kind_id()` (absent from `node-types.json` too). Matching the
    // unsuffixed variant alone left this predicate dead — `bca count -t
    // call` reported 0 on ordinary C (#1254). Keep it listed defensively:
    // it is the symbol a grammar bump is most likely to start emitting
    // (grammar-dispatch §1). Note this is an alias-mapped *visible* rule,
    // not the `_`-prefixed hidden rule of §2 / lesson #34 — different
    // mechanism, same symptom, same defensive treatment.
    fn is_call(node: &Node) -> bool {
        matches!(
            node.kind_id().into(),
            C::CallExpression | C::CallExpression2
        )
    }

    // C's `(void)` marker, shared with C++, Mozcpp and Objective-C.
    fn is_empty_param_marker(param: &Node, code: &[u8]) -> bool {
        c_family_void_parameter(param, code)
    }

    fn is_non_arg(node: &Node) -> bool {
        matches!(node.kind_id().into(), C::LPAREN | C::COMMA | C::RPAREN)
    }

    // C has no raw string literals.
    impl_simple_is_string!(C, StringLiteral, ConcatenatedString);

    impl_is_else_if_parent_clause!(C, IfStatement, ElseClause);

    #[inline]
    fn is_primitive(node: &Node) -> bool {
        node.kind_id() == C::PrimitiveType
    }
}
