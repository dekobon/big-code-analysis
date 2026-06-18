//! `Checker` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Checker for RubyCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Ruby::Comment
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
        match node.kind_id().into() {
            Ruby::Lambda => true,
            // A stabby lambda `->(z) { … }` parses as a `Lambda` node that
            // CONTAINS the `Block`/`DoBlock` for its body, so the `Lambda`
            // arm above already counts it. Counting the inner block again
            // would double-count one closure as two (#465). The keyword
            // forms `lambda { }` / `proc { }` parse as a `Call` carrying a
            // `Block`/`DoBlock` argument (parent is not a `Lambda`), so they
            // still count exactly once.
            Ruby::Block | Ruby::DoBlock => node
                .parent()
                .is_none_or(|parent| parent.kind_id() != Ruby::Lambda),
            _ => false,
        }
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

    impl_simple_is_string!(
        Ruby,
        String,
        ChainedString,
        BareString,
        Subshell,
        Regex,
        HeredocBody,
        DelimitedSymbol,
        SimpleSymbol,
        StringArray,
        SymbolArray,
        Character,
    );

    // tree-sitter-ruby exposes `elsif` as its own named clause node, so the
    // dedicated-clause-node strategy applies here (same as Lua/Bash/PHP).
    impl_is_else_if_clause!(Ruby, Elsif);
}
