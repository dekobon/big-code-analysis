// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use crate::*;

/// A trait to create a richer `AST` node for a programming language, mainly
/// thought to be sent on the network. Crate-internal extension over
/// [`Checker`], used only by the per-language `Parser<T>` impls.
pub(crate) trait Alterator
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
    fn get_text_span(node: &Node, code: &[u8], span: bool, text: bool) -> (String, Option<Span>) {
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
            // Tree-sitter positions are 0-based; the dump shape reports
            // 1-based rows and columns. Byte offsets stay 0-based and
            // half-open, mirroring tree-sitter (#727).
            (
                text,
                Some(Span::new(
                    spos_row + 1,
                    spos_column + 1,
                    epos_row + 1,
                    epos_column + 1,
                    node.start_byte(),
                    node.end_byte(),
                )),
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
    ///
    /// Parameter order mirrors [`Self::alterate`] and [`Self::get_default`]
    /// (the flags-before-data convention `span, comment, field_name,
    /// children`) so positional confusion between adjacent boolean
    /// toggles is harder to introduce on the next edit.
    #[must_use]
    fn get_ast_node(
        node: &Node,
        code: &[u8],
        span: bool,
        comment: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
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
            // RawStringLiteral (`R"(…)"`) is flattened alongside
            // StringLiteral/CharLiteral so the AST dump matches what
            // `Checker::is_string` already treats as a single
            // string-like token. Without this arm, raw strings fall
            // through to `get_default` and render with their
            // structured delimiter / `string_content` children
            // — see issue #398 (peer of #391 for Rust).
            // ConcatenatedString (`"a" "b"`) is one string-like literal
            // that `Checker::is_string` matches; flatten it too so the
            // dump collapses its adjacent `string_literal` children
            // rather than diverging from `is_string` (#699).
            // CharLiteral is operand + flattened but deliberately absent
            // from `Checker::is_string` (a char is not a string) — the
            // same split Rust/Go apply to their char / rune literals.
            Cpp::StringLiteral
            | Cpp::CharLiteral
            | Cpp::RawStringLiteral
            | Cpp::ConcatenatedString => {
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

impl Alterator for CCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        mut children: Vec<AstNode>,
    ) -> AstNode {
        match C::from(node.kind_id()) {
            // RawStringLiteral (`R"(…)"`) is flattened alongside
            // StringLiteral/CharLiteral so the AST dump matches what
            // `Checker::is_string` already treats as a single
            // string-like token. Without this arm, raw strings fall
            // through to `get_default` and render with their
            // structured delimiter / `string_content` children
            // — see issue #398 (peer of #391 for Rust).
            // ConcatenatedString (`"a" "b"`) is one string-like literal
            // that `Checker::is_string` matches; flatten it too so the
            // dump collapses its adjacent `string_literal` children
            // rather than diverging from `is_string` (#699).
            // CharLiteral is operand + flattened but deliberately absent
            // from `Checker::is_string` (a char is not a string) — the
            // same split Rust/Go apply to their char / rune literals.
            // C has no raw string literals.
            C::StringLiteral | C::CharLiteral | C::ConcatenatedString => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            C::PreprocDef | C::PreprocFunctionDef | C::PreprocCall => {
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

impl Alterator for ObjcCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        mut children: Vec<AstNode>,
    ) -> AstNode {
        // ObjC is C plus message sends; the literal-flattening and
        // preprocessor newline-trimming rules are identical to the C
        // alterator. `@"…"` is one `string_literal` whose first child
        // is the `@` token, so the `StringLiteral` arm flattens the
        // whole literal, marker included, exactly as for C.
        match Objc::from(node.kind_id()) {
            // CharLiteral is operand + flattened but deliberately absent
            // from `Checker::is_string` (a char is not a string) — the
            // same split Rust/Go apply to their char / rune literals.
            // The C / Cpp / Mozcpp sibling arms have carried this note
            // since #699; the ObjC clone was missing it (#1316), which
            // is how the missing operand half stayed unnoticed here
            // longest.
            Objc::StringLiteral | Objc::CharLiteral | Objc::ConcatenatedString => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            Objc::PreprocDef | Objc::PreprocFunctionDef | Objc::PreprocCall => {
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

impl Alterator for MozcppCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        mut children: Vec<AstNode>,
    ) -> AstNode {
        match Mozcpp::from(node.kind_id()) {
            // RawStringLiteral (`R"(…)"`) is flattened alongside
            // StringLiteral/CharLiteral so the AST dump matches what
            // `Checker::is_string` already treats as a single
            // string-like token. Without this arm, raw strings fall
            // through to `get_default` and render with their
            // structured delimiter / `string_content` children
            // — see issue #398 (peer of #391 for Rust).
            // ConcatenatedString (`"a" "b"`) is one string-like literal
            // that `Checker::is_string` matches; flatten it too so the
            // dump collapses its adjacent `string_literal` children
            // rather than diverging from `is_string` (#699).
            // CharLiteral is operand + flattened but deliberately absent
            // from `Checker::is_string` (a char is not a string) — the
            // same split Rust/Go apply to their char / rune literals.
            Mozcpp::StringLiteral
            | Mozcpp::CharLiteral
            | Mozcpp::RawStringLiteral
            | Mozcpp::ConcatenatedString => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            Mozcpp::PreprocDef | Mozcpp::PreprocFunctionDef | Mozcpp::PreprocCall => {
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

impl Alterator for PythonCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Python::from(node.kind_id()) {
            // `String` (covers f-strings, whose `{…}` interpolations are
            // collapsed into the flat text payload — same convention as
            // PHP `EncapsedString` / Ruby interpolated strings / C#
            // `InterpolatedStringExpression`) and `ConcatenatedString`
            // (`"a" "b"`) are the kinds `Checker::is_string` matches;
            // flatten both so the dump agrees with `is_string` (#699).
            Python::String | Python::ConcatenatedString => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for JavaCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Java::from(node.kind_id()) {
            // `StringLiteral` and `MultilineStringLiteral` (text blocks,
            // `"""…"""`) are the kinds `Checker::is_string` matches;
            // flatten both so the dump collapses their `string_fragment`
            // children rather than diverging from `is_string` (#699).
            Java::StringLiteral | Java::MultilineStringLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

impl Alterator for KotlinCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Kotlin::from(node.kind_id()) {
            // `StringLiteral` (whose `${…}` interpolations are collapsed
            // into the flat text payload) and `MultilineStringLiteral`
            // (`"""…"""`) are the kinds `Checker::is_string` matches;
            // flatten both so the dump agrees with `is_string` (#699).
            Kotlin::StringLiteral | Kotlin::MultilineStringLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::with_field_name(node.kind(), text, span, field_name, Vec::new())
            }
            _ => Self::get_default(node, code, span, field_name, children),
        }
    }
}

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
            // `String`/`String2` (the anonymous keyword alias) and
            // `TemplateString` are the kinds `Checker::is_string`
            // matches. `TemplateString` may carry `${…}` interpolation
            // children; collapsing them into the flat text payload here
            // is intentional, matching the dump convention for other
            // interpolating string literals (PHP `EncapsedString`, Ruby,
            // C# `InterpolatedStringExpression`). Flatten all three so the
            // dump agrees with `is_string` (#699; before this the comment
            // claimed `TemplateString` flattening the arm did not do).
            Mozjs::String | Mozjs::String2 | Mozjs::TemplateString => {
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
            // `TemplateString` joins `String`/`String2` so the dump
            // matches `Checker::is_string`; its `${…}` interpolations
            // collapse into the flat text payload (#699).
            Javascript::String | Javascript::String2 | Javascript::TemplateString => {
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
            // `TemplateString` joins `String` so the dump matches
            // `Checker::is_string`; its `${…}` interpolations collapse
            // into the flat text payload (#699). TS's `String2` (the
            // `: string` type keyword, a childless leaf for which this
            // arm is a no-op anyway) left the string set with #1261.
            Typescript::String | Typescript::TemplateString => {
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
            // `TemplateString` joins `String`/`String2` (the
            // string-literal alias) so the dump matches
            // `Checker::is_string`; its `${…}` interpolations collapse
            // into the flat text payload (#699). `String3` (the
            // `: string` type keyword, a childless leaf for which this
            // arm is a no-op anyway) left the string set with #1261.
            Tsx::String | Tsx::String2 | Tsx::TemplateString => {
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
            // RawStringLiteral (`r#"…"#`) is flattened alongside
            // StringLiteral/CharLiteral so the AST dump matches what
            // `Checker::is_string` and `Getter::get_op_type` already
            // treat as a single string-like token. Without this arm,
            // raw strings fall through to `get_default` and render
            // with their structured `_raw_string_literal_start` /
            // `string_content` / `_raw_string_literal_end` children
            // — see issue #391.
            Rust::StringLiteral | Rust::RawStringLiteral | Rust::CharLiteral => {
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
            // `HeredocBodyStatement` (`<<TAG … TAG`) is the kind
            // `Checker::is_string` matches for heredoc bodies; flatten it
            // so its `Interpolation` children collapse into the flat text
            // payload rather than diverging from `is_string` (#761, the
            // gap #699 missed — same convention as Ruby `heredoc_body` /
            // PHP `heredoc`/`nowdoc`).
            Perl::StringSingleQuoted
            | Perl::StringDoubleQuoted
            | Perl::StringQQuoted
            | Perl::StringQqQuoted
            | Perl::BacktickQuoted
            | Perl::CommandQxQuoted
            | Perl::HeredocBodyStatement => {
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
            // `HeredocBody2` (`<<EOF … EOF`, kind `heredoc_body`) is the
            // kind `Checker::is_string` matches for heredoc bodies; flatten
            // it so its interpolation / fragment children collapse into the
            // flat text payload rather than diverging from `is_string`
            // (#761, the gap #699 missed — same convention as Ruby
            // `heredoc_body` / PHP `heredoc`/`nowdoc`).
            Bash::String
            | Bash::RawString
            | Bash::AnsiCString
            | Bash::TranslatedString
            | Bash::HeredocBody2 => {
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

impl Alterator for IrulesCode {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        match Irules::from(node.kind_id()) {
            // Preserve string literals verbatim to avoid whitespace trimming.
            Irules::QuotedWord | Irules::BracedWord | Irules::BracedWordSimple => {
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
            // `String`/`String2`/`String3` are all aliased kind_ids
            // that the enum maps to `"string"`; flatten every alias to
            // preserve source text and keep the alterator aligned with
            // `Checker::is_string` and `Getter::get_op_type` (#288,
            // same pattern as #119 for JS/TS).
            Php::String
            | Php::String2
            | Php::String3
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
        // output never trims their bodies. The dekobon Groovy grammar
        // ships several `StringFragment*` aliases for the same rule
        // applied inside different string contexts (single/double-quoted,
        // triple-quoted, slashy, dollar-slashy); each one carries body
        // text we must not collapse.
        match Groovy::from(node.kind_id()) {
            Groovy::StringLiteral
            | Groovy::StringFragment
            | Groovy::StringFragment2
            | Groovy::StringFragment3
            | Groovy::StringFragment4
            | Groovy::StringFragment5 => {
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
        let root = parser.root();
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
            language: String::new(),
            comment: false,
            span: false,
        };
        let resp = crate::ast::dump_inner(&parser, cfg);
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
    fn php_string_like_nodes_all_flattened() {
        // Regression: issue #288. PHP `string`, `encapsed_string`,
        // `heredoc`, `nowdoc`, and `shell_command_expression` (backtick
        // form) must all flatten through the same `Alterator` arm.
        // The wave-1 fix also added `String2`/`String3` enum aliases
        // to the arm for defensive parity with `Checker::is_string`;
        // the current `tree-sitter-php` grammar emits `String2` only
        // as the `string` type keyword (a terminal) and never emits
        // `String3` (a hidden supertype) as a concrete node, so the
        // arm change is a no-op vs `get_default` for those aliases —
        // but locking the structural contract here keeps the three
        // sites aligned if future grammar revisions surface either id
        // as an interior node.
        let code = br#"<?php
            $single = 'single';
            $double = "double";
            $cmd = `ls`;
            $here = <<<EOT
                some text
                EOT;
            $now = <<<'EOT'
                literal
                EOT;
        "#;
        let root = build_ast::<crate::PhpParser>(code, "test.php");
        assert_strings_flattened(&root);
        // ShellCommandExpression flattens with the same shape as
        // EncapsedString — confirm it preserves source text and has
        // no children.
        let mut shells = Vec::new();
        collect_nodes_by_kind(&root, "shell_command_expression", &mut shells);
        assert!(
            !shells.is_empty(),
            "expected at least one shell_command_expression node"
        );
        for node in &shells {
            assert!(
                node.children.is_empty(),
                "shell_command_expression should be flattened; got {} children",
                node.children.len()
            );
            assert!(
                node.value.contains("ls"),
                "flattened backtick literal should preserve text; got {:?}",
                node.value
            );
        }
    }

    #[test]
    fn groovy_string_literal_preserved_verbatim() {
        // Regression for the `impl Alterator for GroovyCode` arms:
        // the `StringLiteral` arm (and its `StringFragment*` aliases
        // for interpolated / slashy / dollar-slashy contexts) must
        // keep the literal's text intact.
        let code = br#"
            class A {
                String single = 'hello'
                String double = "world"
                char ch = 'x'
            }
        "#;
        let root = build_ast::<crate::GroovyParser>(code, "test.groovy");
        // The dekobon Groovy grammar consolidates every string shape
        // under one `string_literal` kind name (single-quoted,
        // double-quoted, triple-quoted, slashy `/.../`, dollar-slashy
        // `$/.../$`, GString-interpolated); there is no separate
        // `character_literal` kind. The defensive collect-by-name for
        // `character_literal` below is a drift marker — it is expected
        // to find nothing today but would surface a future grammar
        // bump that promoted character literals to a distinct kind.
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

    #[test]
    fn rust_raw_string_literal_flattened() {
        // Regression for issue #391: `Rust::RawStringLiteral` was missing
        // from the `RustCode` Alterator arm, so `r#"hello"#` rendered with
        // structured children (`_raw_string_literal_start`,
        // `string_content`, `_raw_string_literal_end`) in the AST dump
        // while the regular `"hello"` was flattened to a single text
        // node. `Checker::is_string` and `Getter::get_op_type` already
        // treat both as equivalent; the alterator now matches.
        let code = br##"fn main() { let s = r#"hello"#; let t = "world"; }"##;
        let root = build_ast::<crate::RustParser>(code, "test.rs");

        let mut raw_strings = Vec::new();
        collect_nodes_by_kind(&root, "raw_string_literal", &mut raw_strings);
        assert_eq!(
            raw_strings.len(),
            1,
            "expected exactly one raw_string_literal node"
        );
        let raw = raw_strings[0];
        assert_eq!(
            raw.children.len(),
            0,
            "raw_string_literal should be flattened (no children)"
        );
        assert_eq!(
            raw.value, "r#\"hello\"#",
            "raw_string_literal should preserve the verbatim source text"
        );

        // The regular StringLiteral arm must still flatten too — confirm
        // the asymmetry that motivated #391 is gone.
        let mut strings = Vec::new();
        collect_nodes_by_kind(&root, "string_literal", &mut strings);
        assert_eq!(strings.len(), 1, "expected exactly one string_literal node");
        assert_eq!(
            strings[0].children.len(),
            0,
            "string_literal should be flattened (no children)"
        );
        assert_eq!(strings[0].value, "\"world\"");
    }

    #[test]
    fn cpp_raw_string_literal_flattened() {
        // Regression for issue #398: `Cpp::RawStringLiteral` was missing
        // from the `CppCode` Alterator arm, so `R"(hello)"` rendered with
        // structured delimiter / `string_content` children in the AST
        // dump while the regular `"hello"` was flattened to a single
        // text node. `Checker::is_string` already treats both as
        // equivalent; the alterator now matches. Peer of issue #391
        // for Rust.
        let code = br#"int main() { auto s = R"(hello)"; auto t = "world"; }"#;
        let root = build_ast::<crate::CppParser>(code, "test.cpp");

        let mut raw_strings = Vec::new();
        collect_nodes_by_kind(&root, "raw_string_literal", &mut raw_strings);
        assert_eq!(
            raw_strings.len(),
            1,
            "expected exactly one raw_string_literal node"
        );
        let raw = raw_strings[0];
        assert_eq!(
            raw.children.len(),
            0,
            "raw_string_literal should be flattened (no children)"
        );
        assert_eq!(
            raw.value, "R\"(hello)\"",
            "raw_string_literal should preserve the verbatim source text"
        );

        // The regular StringLiteral arm must still flatten too — confirm
        // the asymmetry that motivated #398 is gone.
        let mut strings = Vec::new();
        collect_nodes_by_kind(&root, "string_literal", &mut strings);
        assert_eq!(strings.len(), 1, "expected exactly one string_literal node");
        assert_eq!(
            strings[0].children.len(),
            0,
            "string_literal should be flattened (no children)"
        );
        assert_eq!(strings[0].value, "\"world\"");
    }

    /// Asserts that every node of kind `target_kind` in the AST is
    /// flattened (no children) and carries non-empty verbatim text.
    /// Shared by the #699 per-language string-flattening regressions.
    fn assert_kind_flattened(root: &AstNode, target_kind: &str) {
        let mut nodes = Vec::new();
        collect_nodes_by_kind(root, target_kind, &mut nodes);
        assert!(
            !nodes.is_empty(),
            "expected at least one '{target_kind}' node in the AST"
        );
        for node in &nodes {
            assert!(
                node.children.is_empty(),
                "'{target_kind}' node should be flattened (no children), got {} children; value={:?}",
                node.children.len(),
                node.value,
            );
            assert!(
                !node.value.is_empty(),
                "flattened '{target_kind}' node should have non-empty text value"
            );
        }
    }

    // ===== #699: align alterator flattening with `Checker::is_string` =====
    // Before #699 these string-like literals kept their structured
    // children in the AST dump while `is_string` already treated the whole
    // node as a string — a 3-way (alterator / is_string / get_op_type)
    // dump asymmetry. Each test pins that the dump now collapses the kind.

    #[test]
    fn javascript_template_string_flattened() {
        // #699: `is_string` matches `TemplateString`; the alterator now
        // flattens it too, collapsing `${…}` interpolation children into
        // the flat text payload (the established convention for PHP
        // `encapsed_string` / Ruby / C# interpolated strings).
        let code = br#"const a = 1; const b = `bare`; const c = `pre ${a} post`;"#;
        let root = build_ast::<crate::JavascriptParser>(code, "test.js");
        assert_kind_flattened(&root, "template_string");
    }

    #[test]
    fn typescript_template_string_flattened() {
        let code = br#"const a = 1; const b = `bare`; const c = `pre ${a} post`;"#;
        let root = build_ast::<crate::TypescriptParser>(code, "test.ts");
        assert_kind_flattened(&root, "template_string");
    }

    #[test]
    fn tsx_template_string_flattened() {
        let code = br#"const a = 1; const b = `bare`; const c = `pre ${a} post`;"#;
        let root = build_ast::<crate::TsxParser>(code, "test.tsx");
        assert_kind_flattened(&root, "template_string");
    }

    #[test]
    fn mozjs_template_string_flattened() {
        // The MozJS arm's comment previously claimed `TemplateString`
        // flattening it did not perform; #699 made the code match.
        let code = br#"const a = 1; const b = `bare`; const c = `pre ${a} post`;"#;
        let root = build_ast::<crate::MozjsParser>(code, "test.jsm");
        assert_kind_flattened(&root, "template_string");
    }

    #[test]
    fn cpp_concatenated_string_flattened() {
        // #699: `is_string` matches `concatenated_string` (`"a" "b"`);
        // the alterator now flattens the wrapper instead of leaving its
        // adjacent `string_literal` children structured.
        let code = br#"const char* s = "a" "b";"#;
        let root = build_ast::<crate::CppParser>(code, "test.cpp");
        assert_kind_flattened(&root, "concatenated_string");
    }

    #[test]
    fn python_string_and_concatenated_string_flattened() {
        // #699: Python had no alterator override, so `string` (incl.
        // f-strings) and `concatenated_string` kept structured children
        // while `is_string` matched them. The override now flattens both;
        // the f-string `{a}` interpolation collapses into the text value.
        let code = b"a = 1\nb = \"plain\"\nc = f\"x{a}y\"\nd = \"ab\" \"cd\"\n";
        let root = build_ast::<crate::PythonParser>(code, "test.py");
        assert_kind_flattened(&root, "string");
        assert_kind_flattened(&root, "concatenated_string");
    }

    #[test]
    fn java_string_literals_flattened() {
        // #699: Java had no alterator override, so `string_literal` kept
        // its `string_fragment` children. The override flattens it now.
        // Triple-quoted text blocks (`"""…"""`) surface as multi-row
        // `string_literal` nodes too (the `Java::MultilineStringLiteral`
        // enum variant is the hidden `_multiline_string_literal` supertype
        // and never appears concretely — see the checker drift marker in
        // `simple_is_string_macro_recognises_each_language`); the arm
        // lists it for defensive parity with `is_string` but the concrete
        // coverage rides on `string_literal`.
        let code = b"class T { void m() { String a = \"hi\"; String b = \"\"\"\nblock\"\"\"; } }";
        let root = build_ast::<crate::JavaParser>(code, "T.java");
        assert_kind_flattened(&root, "string_literal");
        // The text block produces a multi-row `string_literal`; confirm
        // the flattened node still carries the verbatim multi-line body.
        let mut strings = Vec::new();
        collect_nodes_by_kind(&root, "string_literal", &mut strings);
        assert!(
            strings.iter().any(|n| n.value.contains("block")),
            "expected the flattened text-block string_literal to keep its body"
        );
    }

    #[test]
    fn kotlin_string_literals_flattened() {
        // #699: Kotlin had no alterator override. `string_literal` (incl.
        // `${…}` interpolation, collapsed into the text value) and
        // `multiline_string_literal` (`"""…"""`) now flatten.
        let code = b"fun m() { val a = 1; val b = \"x${a}y\"; val c = \"\"\"block\"\"\" }";
        let root = build_ast::<crate::KotlinParser>(code, "test.kt");
        assert_kind_flattened(&root, "string_literal");
        assert_kind_flattened(&root, "multiline_string_literal");
    }

    #[test]
    fn go_rune_literal_flattened_but_not_a_string_kind() {
        // #699 verdict: Go `rune_literal` is operand + flattened but
        // deliberately excluded from `is_string` (a rune is a character,
        // not a string) — mirroring Rust/Cpp `char_literal`. This pins the
        // flattening half; the `is_string` exclusion is pinned in
        // `checker.rs::go_rune_literal_is_not_a_string`.
        let code = b"package main\nfunc main() { r := 'x'; _ = r }\n";
        let root = build_ast::<crate::GoParser>(code, "test.go");
        assert_kind_flattened(&root, "rune_literal");
    }

    #[test]
    fn bash_heredoc_body_flattened() {
        // #761: `Checker::is_string` matches Bash `heredoc_body`
        // (`Bash::HeredocBody2`) but the alterator omitted it, so a
        // heredoc body with `${…}` interpolation kept its structured
        // `expansion` children in the AST dump while `is_string` treated
        // the whole node as one string. The arm now flattens it — same
        // convention as Ruby `heredoc_body` / PHP `heredoc`.
        let code = b"x=1\ncat <<EOF\npre ${x} post\nEOF\n";
        let root = build_ast::<crate::BashParser>(code, "test.sh");
        assert_kind_flattened(&root, "heredoc_body");
        // The flattened body must carry the verbatim interpolated text,
        // not just the literal fragments.
        let mut bodies = Vec::new();
        collect_nodes_by_kind(&root, "heredoc_body", &mut bodies);
        assert!(
            bodies.iter().any(|n| n.value.contains("${x}")),
            "expected the flattened heredoc_body to keep its interpolation text; got {:?}",
            bodies.iter().map(|n| &n.value).collect::<Vec<_>>()
        );
    }

    #[test]
    fn perl_heredoc_body_statement_flattened() {
        // #761: `Checker::is_string` matches Perl `heredoc_body_statement`
        // (`Perl::HeredocBodyStatement`) but the alterator omitted it, so a
        // heredoc body with interpolation kept its structured
        // `interpolation` children in the AST dump while `is_string`
        // treated the whole node as one string. The arm now flattens it.
        let code = b"my $x = 1;\nmy $s = <<\"EOF\";\npre $x post\nEOF\n";
        let root = build_ast::<crate::PerlParser>(code, "test.pl");
        assert_kind_flattened(&root, "heredoc_body_statement");
        let mut bodies = Vec::new();
        collect_nodes_by_kind(&root, "heredoc_body_statement", &mut bodies);
        assert!(
            bodies.iter().any(|n| n.value.contains("$x")),
            "expected the flattened heredoc_body_statement to keep its interpolation text; got {:?}",
            bodies.iter().map(|n| &n.value).collect::<Vec<_>>()
        );
    }
}
