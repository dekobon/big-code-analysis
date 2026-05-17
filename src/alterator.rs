// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use crate::*;

/// A trait to create a richer `AST` node for a programming language, mainly
/// thought to be sent on the network.
pub trait Alterator
where
    Self: Checker,
{
    /// Creates a new `AST` node containing the code associated to the node,
    /// its span, the grammar field name through which the parent reaches
    /// it (if any), and its children.
    ///
    /// This function can be overloaded according to the needs of each
    /// programming language.
    #[must_use]
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        Self::get_default(node, code, span, field_name, children)
    }

    /// Gets the code as text and the span associated to a node.
    #[must_use]
    fn get_text_span(node: &Node, code: &[u8], span: bool, text: bool) -> (String, Span) {
        let text = if text {
            // Source may contain non-UTF-8 byte strings (e.g. binary literals); replacement
            // characters are acceptable in the AST payload produced by dump functions.
            String::from_utf8_lossy(&code[node.start_byte()..node.end_byte()]).into_owned()
        } else {
            String::new()
        };
        if span {
            let (spos_row, spos_column) = node.start_position();
            let (epos_row, epos_column) = node.end_position();
            (
                text,
                Some((spos_row + 1, spos_column + 1, epos_row + 1, epos_column + 1)),
            )
        } else {
            (text, None)
        }
    }

    /// Gets a default `AST` node containing the code associated to the
    /// node, its span, its grammar field name (if any), and its
    /// children.
    #[must_use]
    fn get_default(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        let (text, span) = Self::get_text_span(node, code, span, node.child_count() == 0);
        AstNode::with_field_name(node.kind(), text, span, field_name, children)
    }

    /// Gets a new `AST` node if and only if the code is not a comment,
    /// otherwise [`None`] is returned.
    #[must_use]
    fn get_ast_node(
        node: &Node,
        code: &[u8],
        children: Vec<AstNode>,
        span: bool,
        comment: bool,
        field_name: Option<&'static str>,
    ) -> Option<AstNode> {
        if comment && Self::is_comment(node) {
            None
        } else {
            Some(Self::alterate(node, code, span, field_name, children))
        }
    }
}

impl Alterator for PreprocCode {}

impl Alterator for CcommentCode {}

impl Alterator for CppCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        mut children: Vec<AstNode>,
    ) -> AstNode {
        match Cpp::from(node.kind_id()) {
            Cpp::StringLiteral | Cpp::CharLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            Cpp::PreprocDef | Cpp::PreprocFunctionDef | Cpp::PreprocCall => {
                if let Some(last) = children.last()
                    && last.r#type == "\n"
                {
                    children.pop();
                }
                Self::get_default(node, code, span, field_name, children)
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for PythonCode {}

impl Alterator for JavaCode {}
impl Alterator for KotlinCode {}

impl Alterator for CsharpCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Csharp::from(node.kind_id()) {
            Csharp::StringLiteral
            | Csharp::VerbatimStringLiteral
            | Csharp::RawStringLiteral
            | Csharp::InterpolatedStringExpression
            | Csharp::CharacterLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for GoCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Go::from(node.kind_id()) {
            Go::InterpretedStringLiteral | Go::RawStringLiteral | Go::RuneLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for LuaCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Lua::from(node.kind_id()) {
            Lua::String => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for MozjsCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Mozjs::from(node.kind_id()) {
            Mozjs::String | Mozjs::String2 => {
                // Template strings may have interpolation children;
                // stripping them here is intentional (by design).
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for JavascriptCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Javascript::from(node.kind_id()) {
            Javascript::String | Javascript::String2 => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for TypescriptCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Typescript::from(node.kind_id()) {
            Typescript::String | Typescript::String2 => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for TsxCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Tsx::from(node.kind_id()) {
            Tsx::String | Tsx::String2 | Tsx::String3 => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for RustCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Rust::from(node.kind_id()) {
            Rust::StringLiteral | Rust::CharLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for PerlCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Perl::from(node.kind_id()) {
            Perl::StringSingleQuoted
            | Perl::StringDoubleQuoted
            | Perl::StringQQuoted
            | Perl::StringQqQuoted
            | Perl::BacktickQuoted
            | Perl::CommandQxQuoted => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for BashCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Bash::from(node.kind_id()) {
            Bash::String | Bash::RawString | Bash::AnsiCString | Bash::TranslatedString => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for TclCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Tcl::from(node.kind_id()) {
            // Preserve string literals verbatim to avoid whitespace trimming.
            Tcl::QuotedWord | Tcl::BracedWord | Tcl::BracedWordSimple => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for PhpCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Php::from(node.kind_id()) {
            Php::String
            | Php::EncapsedString
            | Php::Heredoc
            | Php::Nowdoc
            | Php::ShellCommandExpression => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for RubyCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        // Preserve verbatim text for string-like literals (regular strings,
        // chained strings, heredocs, regexes, subshells, symbol arrays,
        // delimited/simple symbols, character literals). Interpolation
        // children are intentionally collapsed into the flat text payload.
        match Ruby::from(node.kind_id()) {
            Ruby::String
            | Ruby::ChainedString
            | Ruby::BareString
            | Ruby::Subshell
            | Ruby::Regex
            | Ruby::HeredocBody
            | Ruby::StringArray
            | Ruby::SymbolArray
            | Ruby::DelimitedSymbol
            | Ruby::SimpleSymbol
            | Ruby::Character => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for ElixirCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        // Preserve string-like literal text verbatim. `Charlist` (single-
        // quoted) and `Sigil` (`~r/.../`, `~w[...]`, etc.) are treated
        // alongside ordinary strings so report output never trims their
        // bodies.
        match Elixir::from(node.kind_id()) {
            Elixir::String | Elixir::Charlist | Elixir::Sigil => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for GroovyCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        // Preserve string and GString fragment text verbatim so report
        // output never trims their bodies. `StringLiteral2` and
        // `MultilineStringFragment2` are the aliased lexer variants of
        // the same rule.
        match Groovy::from(node.kind_id()) {
            Groovy::StringLiteral
            | Groovy::StringLiteral2
            | Groovy::CharacterLiteral
            | Groovy::StringFragment
            | Groovy::MultilineStringFragment
            | Groovy::MultilineStringFragment2 => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
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
    use std::path::PathBuf;

    use crate::{CppCode, CppParser, ParserTrait};

    use super::*;

    #[test]
    fn get_text_span_non_utf8_uses_replacement_char() {
        // Regression: `String::from_utf8(...).unwrap()` panicked on non-UTF-8
        // source bytes (e.g. binary literals). Now uses from_utf8_lossy so the
        // resulting AstNode text contains U+FFFD rather than causing a crash.
        let code = b"char c = '\xff';";
        let path = PathBuf::from("test.c");
        let parser = CppParser::new(code.to_vec(), &path, None);
        let root = parser.get_root();
        let (text, _) = CppCode::get_text_span(&root, code, false, true);
        assert!(
            text.contains('\u{FFFD}'),
            "expected U+FFFD replacement char for non-UTF-8 source, got: {text:?}"
        );
    }

    /// Collects all AstNode entries whose type matches `target_kind`,
    /// recursively walking the tree.
    fn collect_nodes_by_kind<'a>(node: &'a AstNode, target_kind: &str, out: &mut Vec<&'a AstNode>) {
        if node.r#type == target_kind {
            out.push(node);
        }
        for child in &node.children {
            collect_nodes_by_kind(child, target_kind, out);
        }
    }

    /// Builds an AST from source code using the given parser type.
    fn build_ast<P: ParserTrait>(code: &[u8], filename: &str) -> AstNode {
        let path = PathBuf::from(filename);
        let parser = P::new(code.to_vec(), &path, None);
        let cfg = crate::AstCfg {
            id: String::new(),
            comment: false,
            span: false,
        };
        let resp = crate::AstCallback::call(cfg, &parser);
        resp.root.expect("parser should produce a root AST node")
    }

    /// Asserts that every `"string"` node in the AST is flattened:
    /// non-empty text value and no children.
    fn assert_strings_flattened(root: &AstNode) {
        let mut strings = Vec::new();
        collect_nodes_by_kind(root, "string", &mut strings);
        assert!(
            !strings.is_empty(),
            "expected at least one 'string' node in the AST"
        );
        for node in &strings {
            assert!(
                node.children.is_empty(),
                "string node should be flattened (no children), got {} children; value={:?}",
                node.children.len(),
                node.value,
            );
            assert!(
                !node.value.is_empty(),
                "flattened string node should have non-empty text value"
            );
        }
    }

    // Regression tests for #119: String2 (and String3) variants must be
    // flattened the same way as String. These exercises string literals in
    // multiple grammatical positions to cover aliased kind_ids.
    #[test]
    fn javascript_string_nodes_all_flattened() {
        // Strings in expression, property key, and import positions
        // exercise different grammar productions (String vs String2).
        let code = br#"
            const a = 'single';
            const b = "double";
            const obj = {"key": 1};
            import "module";
        "#;
        let root = build_ast::<crate::JavascriptParser>(code, "test.js");
        assert_strings_flattened(&root);
    }

    #[test]
    fn typescript_string_nodes_all_flattened() {
        let code = br#"
            const a: string = 'single';
            const b: string = "double";
            const obj: Record<string, number> = {"key": 1};
            import "module";
        "#;
        let root = build_ast::<crate::TypescriptParser>(code, "test.ts");
        assert_strings_flattened(&root);
    }

    #[test]
    fn tsx_string_nodes_all_flattened() {
        // TSX has String, String2, and String3 — exercise JSX attribute
        // strings and regular string expressions.
        let code = br#"
            const a = 'single';
            const b = "double";
            const el = <div className="cls">{"text"}</div>;
        "#;
        let root = build_ast::<crate::TsxParser>(code, "test.tsx");
        assert_strings_flattened(&root);
    }

    #[test]
    fn groovy_string_literal_preserved_verbatim() {
        // Regression for the `impl Alterator for GroovyCode` arms:
        // single-line `StringLiteral`/`StringLiteral2` and the
        // `CharacterLiteral` arm must keep the literal's text intact.
        let code = br#"
            class A {
                String single = 'hello'
                String double = "world"
                char ch = 'x'
            }
        "#;
        let root = build_ast::<crate::GroovyParser>(code, "test.groovy");
        // tree-sitter-groovy reports string_literal / character_literal
        // as the kind names; both should be flattened to a non-empty
        // value with no children.
        let mut strings = Vec::new();
        collect_nodes_by_kind(&root, "string_literal", &mut strings);
        collect_nodes_by_kind(&root, "character_literal", &mut strings);
        assert!(
            !strings.is_empty(),
            "expected at least one string/character literal in the AST"
        );
        for node in &strings {
            assert!(
                node.children.is_empty(),
                "string-like node should be flattened (no children); got {} children, value={:?}",
                node.children.len(),
                node.value,
            );
            assert!(
                !node.value.is_empty(),
                "flattened string-like node should have non-empty text value"
            );
        }
    }

    #[test]
    fn groovy_multiline_string_fragment_preserves_newlines() {
        // The `StringLiteral`/`MultilineStringFragment` arms route
        // through `get_text_span(..., true)` to keep the body text
        // verbatim. A regression that flips that boolean to `false`
        // would trim embedded whitespace — including the newlines
        // inside triple-quoted strings. The outer `StringLiteral`
        // node is flattened by the alterator and carries the entire
        // triple-quoted body as its value.
        let code = b"def s = \"\"\"first line\nsecond line\nthird line\"\"\"";
        let root = build_ast::<crate::GroovyParser>(code, "test.groovy");
        let mut strings = Vec::new();
        collect_nodes_by_kind(&root, "string_literal", &mut strings);
        assert!(
            !strings.is_empty(),
            "expected at least one string_literal node for the triple-quoted body"
        );
        let any_with_newline = strings.iter().any(|n| n.value.contains('\n'));
        assert!(
            any_with_newline,
            "expected the flattened string_literal to keep its embedded newlines; got values: {:?}",
            strings.iter().map(|n| &n.value).collect::<Vec<_>>()
        );
    }
}
