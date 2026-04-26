use crate::metrics::halstead::HalsteadType;

use crate::spaces::SpaceKind;
use crate::traits::Search;

use crate::*;

macro_rules! get_operator {
    ($language:ident) => {
        #[inline(always)]
        fn get_operator_id_as_str(id: u16) -> &'static str {
            let typ = id.into();
            match typ {
                $language::LPAREN => "()",
                $language::LBRACK => "[]",
                $language::LBRACE => "{}",
                _ => typ.into(),
            }
        }
    };
}

pub trait Getter {
    fn get_func_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        Self::get_func_space_name(node, code)
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        // we're in a function or in a class
        if let Some(name) = node.child_by_field_name("name") {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            Some("<anonymous>")
        }
    }

    fn get_space_kind(_node: &Node) -> SpaceKind {
        SpaceKind::Unknown
    }

    fn get_op_type(_node: &Node) -> HalsteadType {
        HalsteadType::Unknown
    }

    fn get_operator_id_as_str(_id: u16) -> &'static str {
        ""
    }
}

impl Getter for PythonCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Python::FunctionDefinition => SpaceKind::Function,
            Python::ClassDefinition => SpaceKind::Class,
            Python::Module => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Python::*;

        match node.kind_id().into() {
            Import | DOT | From | COMMA | As | STAR | GTGT | Assert | COLONEQ | Return | Def
            | Del | Raise | Pass | Break | Continue | If | Elif | Else | Async | For | In
            | While | Try | Except | Finally | With | DASHGT | EQ | Global | Exec | AT | Not
            | And | Or | PLUS | DASH | SLASH | PERCENT | SLASHSLASH | STARSTAR | PIPE | AMP
            | CARET | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ | GT | LTGT | Is | PLUSEQ
            | DASHEQ | STAREQ | SLASHEQ | ATEQ | SLASHSLASHEQ | PERCENTEQ | STARSTAREQ | GTGTEQ
            | LTLTEQ | AMPEQ | CARETEQ | PIPEEQ | Yield | Await | Await2 | Print => {
                HalsteadType::Operator
            }
            Identifier | Integer | Float | True | False | None => HalsteadType::Operand,
            String => {
                let mut operator = HalsteadType::Unknown;
                // check if we've a documentation string or a multiline comment
                if let Some(parent) = node.parent()
                    && (parent.kind_id() != ExpressionStatement || parent.child_count() != 1)
                {
                    operator = HalsteadType::Operand;
                };
                operator
            }
            _ => HalsteadType::Unknown,
        }
    }

    fn get_operator_id_as_str(id: u16) -> &'static str {
        Into::<Python>::into(id).into()
    }
}

impl Getter for MozjsCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Mozjs::*;

        match node.kind_id().into() {
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction => SpaceKind::Function,
            Class | ClassDeclaration => SpaceKind::Class,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        if let Some(name) = node.child_by_field_name("name") {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            // We can be in a pair: foo: function() {}
            // Or in a variable declaration: var aFun = function() {}
            if let Some(parent) = node.parent() {
                match parent.kind_id().into() {
                    Mozjs::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    Mozjs::VariableDeclarator => {
                        if let Some(name) = parent.child_by_field_name("name") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    _ => {}
                }
            }
            Some("<anonymous>")
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Mozjs::*;

        match node.kind_id().into() {
            Export | Import | Import2 | Extends | DOT | From | LPAREN | COMMA | As | STAR
            | GTGT | GTGTGT | COLON | Return | Delete | Throw | Break | Continue | If | Else
            | Switch | Case | Default | Async | For | In | Of | While | Try | Catch | Finally
            | With | EQ | AT | AMPAMP | PIPEPIPE | PLUS | DASH | DASHDASH | PLUSPLUS | SLASH
            | PERCENT | STARSTAR | PIPE | AMP | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ
            | GT | PLUSEQ | BANG | BANGEQEQ | EQEQEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ
            | STARSTAREQ | GTGTEQ | GTGTGTEQ | LTLTEQ | AMPEQ | CARET | CARETEQ | PIPEEQ
            | Yield | LBRACK | LBRACE | Await | QMARK | QMARKQMARK | New | Let | Var | Const
            | Function | FunctionExpression | SEMI => HalsteadType::Operator,
            Identifier | Identifier2 | MemberExpression | MemberExpression2
            | PropertyIdentifier | String | String2 | Number | True | False | Null | Void
            | This | Super | Undefined | Set | Get | Typeof | Instanceof => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Mozjs);
}

impl Getter for JavascriptCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Javascript::*;

        match node.kind_id().into() {
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction => SpaceKind::Function,
            Class | ClassDeclaration => SpaceKind::Class,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        if let Some(name) = node.child_by_field_name("name") {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            // We can be in a pair: foo: function() {}
            // Or in a variable declaration: var aFun = function() {}
            if let Some(parent) = node.parent() {
                match parent.kind_id().into() {
                    Mozjs::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    Mozjs::VariableDeclarator => {
                        if let Some(name) = parent.child_by_field_name("name") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    _ => {}
                }
            }
            Some("<anonymous>")
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Javascript::*;

        match node.kind_id().into() {
            Export | Import | Import2 | Extends | DOT | From | LPAREN | COMMA | As | STAR
            | GTGT | GTGTGT | COLON | Return | Delete | Throw | Break | Continue | If | Else
            | Switch | Case | Default | Async | For | In | Of | While | Try | Catch | Finally
            | With | EQ | AT | AMPAMP | PIPEPIPE | PLUS | DASH | DASHDASH | PLUSPLUS | SLASH
            | PERCENT | STARSTAR | PIPE | AMP | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ
            | GT | PLUSEQ | BANG | BANGEQEQ | EQEQEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ
            | STARSTAREQ | GTGTEQ | GTGTGTEQ | LTLTEQ | AMPEQ | CARET | CARETEQ | PIPEEQ
            | Yield | LBRACK | LBRACE | Await | QMARK | QMARKQMARK | New | Let | Var | Const
            | Function | FunctionExpression | SEMI => HalsteadType::Operator,
            Identifier | Identifier2 | MemberExpression | MemberExpression2
            | PropertyIdentifier | String | String2 | Number | True | False | Null | Void
            | This | Super | Undefined | Set | Get | Typeof | Instanceof => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Javascript);
}

impl Getter for TypescriptCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Typescript::*;

        match node.kind_id().into() {
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction => SpaceKind::Function,
            Class | ClassDeclaration => SpaceKind::Class,
            InterfaceDeclaration => SpaceKind::Interface,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        if let Some(name) = node.child_by_field_name("name") {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            // We can be in a pair: foo: function() {}
            // Or in a variable declaration: var aFun = function() {}
            if let Some(parent) = node.parent() {
                match parent.kind_id().into() {
                    Mozjs::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    Mozjs::VariableDeclarator => {
                        if let Some(name) = parent.child_by_field_name("name") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    _ => {}
                }
            }
            Some("<anonymous>")
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Typescript::*;

        match node.kind_id().into() {
            Export | Import | Import2 | Extends | DOT | From | LPAREN | COMMA | As | STAR
            | GTGT | GTGTGT | COLON | Return | Delete | Throw | Break | Continue | If | Else
            | Switch | Case | Default | Async | For | In | Of | While | Try | Catch | Finally
            | With | EQ | AT | AMPAMP | PIPEPIPE | PLUS | DASH | DASHDASH | PLUSPLUS | SLASH
            | PERCENT | STARSTAR | PIPE | AMP | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ
            | GT | PLUSEQ | BANG | BANGEQEQ | EQEQEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ
            | STARSTAREQ | GTGTEQ | GTGTGTEQ | LTLTEQ | AMPEQ | CARET | CARETEQ | PIPEEQ
            | Yield | LBRACK | LBRACE | Await | QMARK | QMARKQMARK | New | Let | Var | Const
            | Function | FunctionExpression | SEMI => HalsteadType::Operator,
            Identifier | NestedIdentifier | MemberExpression | PropertyIdentifier | String
            | Number | True | False | Null | Void | This | Super | Undefined | Set | Get
            | Typeof | Instanceof => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Typescript);
}

impl Getter for TsxCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Tsx::*;

        match node.kind_id().into() {
            FunctionExpression
            | MethodDefinition
            | GeneratorFunction
            | FunctionDeclaration
            | GeneratorFunctionDeclaration
            | ArrowFunction => SpaceKind::Function,
            Class | ClassDeclaration => SpaceKind::Class,
            InterfaceDeclaration => SpaceKind::Interface,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        if let Some(name) = node.child_by_field_name("name") {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            // We can be in a pair: foo: function() {}
            // Or in a variable declaration: var aFun = function() {}
            if let Some(parent) = node.parent() {
                match parent.kind_id().into() {
                    Mozjs::Pair => {
                        if let Some(name) = parent.child_by_field_name("key") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    Mozjs::VariableDeclarator => {
                        if let Some(name) = parent.child_by_field_name("name") {
                            let code = &code[name.start_byte()..name.end_byte()];
                            return std::str::from_utf8(code).ok();
                        }
                    }
                    _ => {}
                }
            }
            Some("<anonymous>")
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Tsx::*;

        match node.kind_id().into() {
            Export | Import | Import2 | Extends | DOT | From | LPAREN | COMMA | As | STAR
            | GTGT | GTGTGT | COLON | Return | Delete | Throw | Break | Continue | If | Else
            | Switch | Case | Default | Async | For | In | Of | While | Try | Catch | Finally
            | With | EQ | AT | AMPAMP | PIPEPIPE | PLUS | DASH | DASHDASH | PLUSPLUS | SLASH
            | PERCENT | STARSTAR | PIPE | AMP | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ
            | GT | PLUSEQ | BANG | BANGEQEQ | EQEQEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ
            | STARSTAREQ | GTGTEQ | GTGTGTEQ | LTLTEQ | AMPEQ | CARET | CARETEQ | PIPEEQ
            | Yield | LBRACK | LBRACE | Await | QMARK | QMARKQMARK | New | Let | Var | Const
            | Function | FunctionExpression | SEMI => HalsteadType::Operator,
            Identifier | NestedIdentifier | MemberExpression | PropertyIdentifier | String
            | String2 | Number | True | False | Null | Void | This | Super | Undefined | Set
            | Get | Typeof | Instanceof => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Tsx);
}

impl Getter for RustCode {
    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        // we're in a function or in a class or an impl
        // for an impl: we've  'impl ... type {...'
        if let Some(name) = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("type"))
        {
            let code = &code[name.start_byte()..name.end_byte()];
            std::str::from_utf8(code).ok()
        } else {
            Some("<anonymous>")
        }
    }

    fn get_space_kind(node: &Node) -> SpaceKind {
        use Rust::*;

        match node.kind_id().into() {
            FunctionItem | ClosureExpression => SpaceKind::Function,
            TraitItem => SpaceKind::Trait,
            ImplItem => SpaceKind::Impl,
            SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Rust::*;

        match node.kind_id().into() {
            // `||` is treated as an operator only if it's part of a binary expression.
            // This prevents misclassification inside macros where closures without arguments (e.g., `let closure = || { /* ... */ };`)
            // are not recognized as `ClosureExpression` and their `||` node is identified as `PIPEPIPE` instead of `ClosureParameters`.
            //
            // Similarly, exclude `/` when it corresponds to the third slash in `///` (`OuterDocCommentMarker`)
            PIPEPIPE | SLASH => match node.parent() {
                Some(parent) if matches!(parent.kind_id().into(), BinaryExpression) => {
                    HalsteadType::Operator
                }
                _ => HalsteadType::Unknown,
            },
            // Ensure `!` is counted as an operator unless it belongs to an `InnerDocCommentMarker` `//!`
            BANG => match node.parent() {
                Some(parent) if !matches!(parent.kind_id().into(), InnerDocCommentMarker) => {
                    HalsteadType::Operator
                }
                _ => HalsteadType::Unknown,
            },
            LPAREN | LBRACE | LBRACK | EQGT | PLUS | STAR | Async | Await | Continue | For | If
            | Let | Loop | Match | Return | Unsafe | While | EQ | COMMA | DASHGT | QMARK | LT
            | GT | AMP | MutableSpecifier | DOTDOT | DOTDOTEQ | DASH | AMPAMP | PIPE | CARET
            | EQEQ | BANGEQ | LTEQ | GTEQ | LTLT | GTGT | PERCENT | PLUSEQ | DASHEQ | STAREQ
            | SLASHEQ | PERCENTEQ | AMPEQ | PIPEEQ | CARETEQ | LTLTEQ | GTGTEQ | Move | DOT
            | PrimitiveType | Fn | SEMI => HalsteadType::Operator,
            Identifier | StringLiteral | RawStringLiteral | IntegerLiteral | FloatLiteral
            | BooleanLiteral | Zelf | CharLiteral | UNDERSCORE => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Rust);
}

impl Getter for CppCode {
    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        match node.kind_id().into() {
            Cpp::FunctionDefinition | Cpp::FunctionDefinition2 | Cpp::FunctionDefinition3 => {
                if let Some(op_cast) = node.first_child(|id| Cpp::OperatorCast == id) {
                    let code = &code[op_cast.start_byte()..op_cast.end_byte()];
                    return std::str::from_utf8(code).ok();
                }
                // we're in a function_definition so need to get the declarator
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    let declarator_node = declarator;
                    if let Some(fd) = declarator_node.first_occurrence(|id| {
                        Cpp::FunctionDeclarator == id
                            || Cpp::FunctionDeclarator2 == id
                            || Cpp::FunctionDeclarator3 == id
                    }) && let Some(first) = fd.child(0)
                    {
                        match first.kind_id().into() {
                            Cpp::TypeIdentifier
                            | Cpp::Identifier
                            | Cpp::FieldIdentifier
                            | Cpp::DestructorName
                            | Cpp::OperatorName
                            | Cpp::QualifiedIdentifier
                            | Cpp::QualifiedIdentifier2
                            | Cpp::QualifiedIdentifier3
                            | Cpp::QualifiedIdentifier4
                            | Cpp::TemplateFunction
                            | Cpp::TemplateMethod => {
                                let code = &code[first.start_byte()..first.end_byte()];
                                return std::str::from_utf8(code).ok();
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {
                if let Some(name) = node.child_by_field_name("name") {
                    let code = &code[name.start_byte()..name.end_byte()];
                    return std::str::from_utf8(code).ok();
                }
            }
        }
        None
    }

    fn get_space_kind(node: &Node) -> SpaceKind {
        use Cpp::*;

        match node.kind_id().into() {
            FunctionDefinition | FunctionDefinition2 | FunctionDefinition3 => SpaceKind::Function,
            StructSpecifier => SpaceKind::Struct,
            ClassSpecifier => SpaceKind::Class,
            NamespaceDefinition => SpaceKind::Namespace,
            TranslationUnit => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Cpp::*;

        match node.kind_id().into() {
            DOT | LPAREN | LPAREN2 | COMMA | STAR | GTGT | COLON | SEMI | Return | Break
            | Continue | If | Else | Switch | Case | Default | For | While | Goto | Do | Delete
            | New | Try | Try2 | Catch | Throw | EQ | AMPAMP | PIPEPIPE | DASH | DASHDASH
            | DASHGT | PLUS | PLUSPLUS | SLASH | PERCENT | PIPE | AMP | LTLT | TILDE | LT
            | LTEQ | EQEQ | BANGEQ | GTEQ | GT | GT2 | PLUSEQ | BANG | STAREQ | SLASHEQ
            | PERCENTEQ | GTGTEQ | LTLTEQ | AMPEQ | CARET | CARETEQ | PIPEEQ | LBRACK | LBRACE
            | QMARK | COLONCOLON | PrimitiveType | TypeSpecifier | Sizeof => HalsteadType::Operator,
            Identifier | TypeIdentifier | FieldIdentifier | RawStringLiteral | StringLiteral
            | NumberLiteral | True | False | Null | DOTDOTDOT => HalsteadType::Operand,
            NamespaceIdentifier => match node.parent() {
                Some(parent) if matches!(parent.kind_id().into(), NamespaceDefinition) => {
                    HalsteadType::Operand
                }
                _ => HalsteadType::Unknown,
            },
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Cpp);
}

impl Getter for PreprocCode {}
impl Getter for CcommentCode {}

impl Getter for JavaCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Java::*;

        match node.kind_id().into() {
            ClassDeclaration => SpaceKind::Class,
            MethodDeclaration | ConstructorDeclaration | LambdaExpression => SpaceKind::Function,
            InterfaceDeclaration => SpaceKind::Interface,
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Java::*;
        // Some guides that informed grammar choice for Halstead
        // keywords, operators, literals: https://docs.oracle.com/javase/specs/jls/se18/html/jls-3.html#jls-3.12
        // https://www.geeksforgeeks.org/software-engineering-halsteads-software-metrics/?msclkid=5e181114abef11ecbb03527e95a34828
        match node.kind_id().into() {
            // Operator: control flow
            | If | Else | Switch | Case | Try | Catch | Throw | Throws | Throws2 | For | While | Continue | Break | Do | Finally
            // Operator: keywords
            | New | Return | Default | Abstract | Assert | Instanceof | Extends | Final | Implements | Transient | Synchronized | Super | This | VoidType
            // Operator: brackets and comma and terminators (separators)
            | SEMI | COMMA | COLONCOLON | LBRACE | LBRACK | LPAREN // | RBRACE | RBRACK | RPAREN | DOTDOTDOT | DOT
            // Operator: operators
            | EQ | LT | GT | BANG | TILDE | QMARK | COLON // no grammar for lambda operator ->
            | EQEQ | LTEQ | GTEQ | BANGEQ | AMPAMP | PIPEPIPE | PLUSPLUS | DASHDASH
            | PLUS | DASH | STAR | SLASH | AMP | PIPE | CARET | PERCENT| LTLT | GTGT | GTGTGT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | AMPEQ | PIPEEQ | CARETEQ | PERCENTEQ | LTLTEQ | GTGTEQ | GTGTGTEQ
            // primitive types
            | Int | Float
            => {
                HalsteadType::Operator
            },
            // Operands: variables, constants, literals
            Identifier | NullLiteral | ClassLiteral | StringLiteral | CharacterLiteral | HexIntegerLiteral | OctalIntegerLiteral | BinaryIntegerLiteral | DecimalIntegerLiteral | HexFloatingPointLiteral | DecimalFloatingPointLiteral  => {
                HalsteadType::Operand
            },
            _ => {
                HalsteadType::Unknown
            },
        }
    }

    fn get_operator_id_as_str(id: u16) -> &'static str {
        let typ = id.into();
        match typ {
            Java::LPAREN => "()",
            Java::LBRACK => "[]",
            Java::LBRACE => "{}",
            Java::VoidType => "void",
            _ => typ.into(),
        }
    }
}

impl Getter for KotlinCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Kotlin::*;

        match node.kind_id().into() {
            ClassDeclaration | ObjectDeclaration => SpaceKind::Class,
            FunctionDeclaration | SecondaryConstructor | LambdaLiteral | AnonymousFunction => {
                SpaceKind::Function
            }
            SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Kotlin::*;

        match node.kind_id().into() {
            // Operator: control flow keywords
            If | Else | When | For | While | Do | Try | Catch | Finally | Throw | Return
            | ReturnAT
            // Operator: other keywords
            | Class | Fun | Object | Val | Var | In | Is | As | AsQMARK | BANGis | BANGin
            | This | Super | Constructor
            // Operator: brackets, separators, terminators
            | SEMI | COMMA | COLONCOLON | LBRACE | LBRACK | LPAREN
            // Operator: assignment and arithmetic
            | EQ | PLUS | DASH | STAR | SLASH | PERCENT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ
            | PLUSPLUS | DASHDASH
            // Operator: comparison and equality
            | LT | GT | LTEQ | GTEQ | EQEQ | EQEQEQ | BANGEQ | BANGEQEQ
            // Operator: logical and misc
            | AMPAMP | PIPEPIPE | BANG | BANGBANG
            | QMARK | QMARKCOLON | QMARKDOT
            | DOTDOT | DOTDOTLT | DASHGT | COLON => HalsteadType::Operator,
            // Operands: identifiers and literals
            Identifier | NumberLiteral | FloatLiteral | StringLiteral
            | MultilineStringLiteral | CharacterLiteral | Label => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Kotlin);
}

impl Getter for GoCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        // Aliased because `Go::Go` (the `go` keyword variant) collides with
        // the bare enum name in pattern position under `use Go::*;`.
        use Go as G;

        match node.kind_id().into() {
            G::FunctionDeclaration | G::MethodDeclaration | G::FuncLiteral => SpaceKind::Function,
            G::SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Go as G;

        match node.kind_id().into() {
            // Control flow and declaration keywords
            G::If | G::Else | G::Switch | G::Case | G::Default | G::For | G::Range
            | G::Continue | G::Break | G::Fallthrough | G::Goto | G::Return | G::Select
            | G::Defer | G::Go | G::Func | G::Type | G::Struct | G::Interface | G::Map
            | G::Chan | G::Const | G::Var | G::Package | G::Import
            // Punctuation acting as operators
            | G::SEMI | G::COMMA | G::COLON | G::LBRACE | G::LBRACK | G::LPAREN
            | G::DOT | G::DOTDOTDOT
            // Operators
            | G::EQ | G::COLONEQ | G::PLUS | G::DASH | G::STAR | G::SLASH | G::PERCENT
            | G::AMP | G::PIPE | G::CARET | G::TILDE | G::BANG | G::LT | G::GT
            | G::LTEQ | G::GTEQ | G::EQEQ | G::BANGEQ | G::AMPAMP | G::PIPEPIPE
            | G::LTLT | G::GTGT | G::AMPCARET | G::LTDASH | G::PLUSPLUS | G::DASHDASH
            | G::PLUSEQ | G::DASHEQ | G::STAREQ | G::SLASHEQ | G::PERCENTEQ | G::AMPEQ
            | G::PIPEEQ | G::CARETEQ | G::LTLTEQ | G::GTGTEQ | G::AMPCARETEQ
                => HalsteadType::Operator,
            // Operands: identifiers and literals
            G::Identifier | G::FieldIdentifier | G::PackageIdentifier | G::TypeIdentifier
            | G::LabelName | G::IntLiteral | G::FloatLiteral | G::ImaginaryLiteral
            | G::RuneLiteral | G::InterpretedStringLiteral | G::RawStringLiteral | G::Nil
            | G::True | G::False | G::Iota
                => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Go);
}

impl Getter for PerlCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Perl::FunctionDefinition
            | Perl::FunctionDefinitionWithoutSub
            | Perl::AnonymousFunction => SpaceKind::Function,
            Perl::SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type(node: &Node) -> HalsteadType {
        use Perl as P;

        match node.kind_id().into() {
            // Control-flow and declaration keywords. `Perl::Sub` is the
            // `sub` keyword (token id 16); `Perl::SUB` is the `__SUB__`
            // literal (token id 7) — that one is an operand, not an
            // operator. Same split for `Package` (keyword) vs `PACKAGE`
            // (`__PACKAGE__` literal).
            P::If | P::Unless | P::Else | P::Elsif | P::While | P::Until | P::For
            | P::Foreach | P::When | P::Continue | P::Next | P::Last | P::Redo | P::Goto
            | P::Return | P::Sub | P::Package | P::My | P::Our
            | P::Local | P::State | P::Use | P::No | P::Require | P::Bless | P::And | P::Or
            | P::Xor | P::Not | P::Eq | P::Ne | P::Lt | P::Gt | P::Le | P::Ge | P::Cmp
            // Punctuation acting as operators
            | P::SEMI | P::COMMA | P::COLON | P::COLONCOLON | P::LBRACE | P::LBRACK
            | P::LPAREN | P::DOT | P::DOTDOT | P::DOTDOTDOT | P::FatComma | P::DASHGT
            | P::QMARK | P::BSLASH | P::DOLLAR | P::DOLLARHASH | P::AT | P::PERCENT | P::HASH
            // Arithmetic / comparison / logical / bitwise / assignment operators
            | P::EQ | P::PLUS | P::DASH | P::STAR | P::SLASH | P::STARSTAR | P::BANG
            | P::TILDE | P::EQTILDE | P::BANGTILDE | P::EQEQ | P::BANGEQ | P::LT | P::GT
            | P::LTEQ | P::GTEQ | P::AMPAMP | P::PIPEPIPE | P::SLASHSLASH | P::PIPE
            | P::CARET | P::LTLT | P::GTGT | P::TILDETILDE | P::PLUSPLUS | P::DASHDASH
            | P::PLUSEQ | P::DASHEQ | P::STAREQ | P::SLASHEQ | P::PERCENTEQ | P::STARSTAREQ
            | P::AMPEQ | P::PIPEEQ | P::CARETEQ | P::LTLTEQ | P::GTGTEQ | P::AMPAMPEQ
            | P::PIPEPIPEEQ | P::SLASHSLASHEQ | P::DOTEQ | P::XEQ
                => HalsteadType::Operator,
            // Operands: identifiers and literals
            P::Identifier | P::ScalarVariable | P::ArrayVariable | P::HashVariable
            | P::PackageVariable | P::SpecialScalarVariable | P::PackageName | P::ModuleName
            | P::BarewordImport | P::Typeglob | P::FileHandle
            | P::Integer | P::FloatingPoint | P::ScientificNotation | P::Hexadecimal | P::Octal
            | P::True | P::False | P::SpecialLiteral
            | P::StringSingleQuoted | P::StringDoubleQuoted | P::StringQQuoted
            | P::StringQqQuoted | P::BacktickQuoted | P::CommandQxQuoted
            | P::FILE | P::LINE | P::SUB | P::PACKAGE
                => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Perl);
}
