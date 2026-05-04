use std::sync::OnceLock;

use aho_corasick::AhoCorasick;
use regex::bytes::Regex;

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
        #[inline(always)]
        fn is_func(node: &Node) -> bool {
            use $language::*;
            is_js_func!($parser, node)
        }

        #[inline(always)]
        fn is_closure(node: &Node) -> bool {
            use $language::*;
            is_js_closure!($parser, node)
        }
    };
}

#[inline(always)]
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

    #[inline(always)]
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

    fn is_else_if(_: &Node) -> bool {
        false
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

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline(always)]
    fn is_else_if(node: &Node) -> bool {
        if node.kind_id() != Typescript::IfStatement {
            return false;
        }
        if let Some(parent) = node.parent() {
            return parent.kind_id() == Typescript::ElseClause;
        }
        false
    }

    #[inline(always)]
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

    #[inline(always)]
    fn is_primitive(id: u16) -> bool {
        id == Tsx::PredefinedType
    }
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

    #[inline(always)]
    fn is_else_if(node: &Node) -> bool {
        if node.kind_id() != Rust::IfExpression {
            return false;
        }
        if let Some(parent) = node.parent() {
            return parent.kind_id() == Rust::ElseClause;
        }
        false
    }

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline(always)]
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

#[cfg(test)]
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
}
