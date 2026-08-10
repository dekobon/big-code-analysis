//! `Checker` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

/// Whether `node` — a `Block` / `DoBlock` — is the body of a stabby
/// lambda (`->(z) { … }` / `->(z) do … end`), which parses as a `Lambda`
/// node that CONTAINS the block. The `Lambda` wrapper already counts as
/// the closure (#465) and opens the function space (#1257), so
/// classifying the body again would double-count one lambda as two
/// closures, or open a phantom nested Function space. The keyword forms
/// `lambda { }` / `proc { }` and iterator blocks (`[1].each { |x| x }`)
/// parse as a `Call` carrying the block argument — the parent is not a
/// `Lambda` — so they still classify exactly once.
///
/// One helper shared by [`Checker::is_closure`],
/// [`Checker::is_func_space_with_code`], and the cognitive lambda-nesting
/// arm (`crate::metrics::cognitive`'s Ruby impl), so the closure count,
/// the space tree, and the nesting surcharge cannot drift apart
/// (`.claude/rules/grammar-dispatch.md` §6).
///
/// The parent comes off the caller's chain: all consumers run per node
/// from a walk, and `Node::parent` costs `O(depth)` because
/// `tree_sitter` resolves it by descending from the root (#1088).
pub(crate) fn is_stabby_lambda_body<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
    ancestors
        .parent(node)
        .is_some_and(|parent| parent.kind_id() == Ruby::Lambda)
}

impl Checker for RubyCode {
    fn is_comment(node: &Node) -> bool {
        node.kind_id() == Ruby::Comment
    }

    // Over-approximates for `Block` / `DoBlock`: a stabby lambda's own
    // body block is NOT a space of its own (the wrapping `Lambda` is),
    // but telling the two apart needs the parent and this spelling has
    // no ancestor access. The walker promotes through the
    // [`Checker::is_func_space_with_code`] override below, which
    // carries the lambda-body gate (#1257).
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

    // Derives from `is_func_space` so the kind set lives in one place;
    // this override only subtracts the stabby-lambda body blocks the
    // byte-less spelling over-approximates on.
    fn is_func_space_with_code<'a>(
        node: &Node<'a>,
        _code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        Self::is_func_space(node)
            && !(matches!(node.kind_id().into(), Ruby::Block | Ruby::DoBlock)
                && is_stabby_lambda_body(node, ancestors))
    }

    fn is_func<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> bool {
        matches!(node.kind_id().into(), Ruby::Method | Ruby::SingletonMethod)
    }

    // Ruby is the one non-JS grammar whose closure test is not
    // answerable from the node's own kind: a `Block` / `DoBlock` counts
    // only when it is not a stabby lambda's own body (#465, see
    // `is_stabby_lambda_body`).
    fn is_closure<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> bool {
        match node.kind_id().into() {
            Ruby::Lambda => true,
            Ruby::Block | Ruby::DoBlock => !is_stabby_lambda_body(node, ancestors),
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
