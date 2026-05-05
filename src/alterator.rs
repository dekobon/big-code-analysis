use crate::*;

/// A trait to create a richer `AST` node for a programming language, mainly
/// thought to be sent on the network.
pub trait Alterator
where
    Self: Checker,
{
    /// Creates a new `AST` node containing the code associated to the node,
    /// its span, and its children.
    ///
    /// This function can be overloaded according to the needs of each
    /// programming language.
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        Self::get_default(node, code, span, children)
    }

    /// Gets the code as text and the span associated to a node.
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

    /// Gets a default `AST` node containing the code associated to the node,
    /// its span, and its children.
    fn get_default(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        let (text, span) = Self::get_text_span(node, code, span, node.child_count() == 0);
        AstNode::new(node.kind(), text, span, children)
    }

    /// Gets a new `AST` node if and only if the code is not a comment,
    /// otherwise [`None`] is returned.
    fn get_ast_node(
        node: &Node,
        code: &[u8],
        children: Vec<AstNode>,
        span: bool,
        comment: bool,
    ) -> Option<AstNode> {
        if comment && Self::is_comment(node) {
            None
        } else {
            Some(Self::alterate(node, code, span, children))
        }
    }
}

impl Alterator for PreprocCode {}

impl Alterator for CcommentCode {}

impl Alterator for CppCode {
    fn alterate(node: &Node, code: &[u8], span: bool, mut children: Vec<AstNode>) -> AstNode {
        match Cpp::from(node.kind_id()) {
            Cpp::StringLiteral | Cpp::CharLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            Cpp::PreprocDef | Cpp::PreprocFunctionDef | Cpp::PreprocCall => {
                if let Some(last) = children.last()
                    && last.r#type == "\n"
                {
                    children.pop();
                }
                Self::get_default(node, code, span, children)
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for PythonCode {}

impl Alterator for JavaCode {}
impl Alterator for KotlinCode {}

impl Alterator for CsharpCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Csharp::from(node.kind_id()) {
            Csharp::StringLiteral
            | Csharp::VerbatimStringLiteral
            | Csharp::RawStringLiteral
            | Csharp::InterpolatedStringExpression
            | Csharp::CharacterLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for GoCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Go::from(node.kind_id()) {
            Go::InterpretedStringLiteral | Go::RawStringLiteral | Go::RuneLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for LuaCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Lua::from(node.kind_id()) {
            Lua::String => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for MozjsCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Mozjs::from(node.kind_id()) {
            Mozjs::String | Mozjs::String2 => {
                // TODO: have a thought about template_strings:
                // they may have children for replacement...
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for JavascriptCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Javascript::from(node.kind_id()) {
            Javascript::String => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for TypescriptCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Typescript::from(node.kind_id()) {
            Typescript::String => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for TsxCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Tsx::from(node.kind_id()) {
            Tsx::String => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for RustCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Rust::from(node.kind_id()) {
            Rust::StringLiteral | Rust::CharLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for PerlCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Perl::from(node.kind_id()) {
            Perl::StringSingleQuoted
            | Perl::StringDoubleQuoted
            | Perl::StringQQuoted
            | Perl::StringQqQuoted
            | Perl::BacktickQuoted
            | Perl::CommandQxQuoted => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for BashCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Bash::from(node.kind_id()) {
            Bash::String | Bash::RawString | Bash::AnsiCString | Bash::TranslatedString => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for TclCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Tcl::from(node.kind_id()) {
            // Preserve string literals verbatim to avoid whitespace trimming.
            Tcl::QuotedWord | Tcl::BracedWord | Tcl::BracedWordSimple => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

impl Alterator for PhpCode {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match Php::from(node.kind_id()) {
            Php::String
            | Php::EncapsedString
            | Php::Heredoc
            | Php::Nowdoc
            | Php::ShellCommandExpression => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}

#[cfg(test)]
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
}
