//! `Getter` implementation for Java.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for JavaCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Java::*;

        // `EnumDeclaration` and `RecordDeclaration` are class-like
        // (they extend `Object`, hold fields, and can declare methods)
        // so they share `SpaceKind::Class`. `AnnotationTypeDeclaration`
        // implicitly extends `java.lang.annotation.Annotation` (an
        // interface) and its elements are abstract methods at the
        // bytecode level, so it maps to `SpaceKind::Interface`.
        match node.kind_id().into() {
            ClassDeclaration | EnumDeclaration | RecordDeclaration => SpaceKind::Class,
            MethodDeclaration | ConstructorDeclaration | LambdaExpression => SpaceKind::Function,
            InterfaceDeclaration | AnnotationTypeDeclaration => SpaceKind::Interface,
            // An anonymous class (`new Runnable() { ... }`) is an
            // `object_creation_expression` carrying a `class_body` child;
            // it opens a Class space, matching PHP's `AnonymousClass` and
            // C#'s anonymous forms (#463). A plain `new Foo()` has no
            // `class_body`, so it falls through to `Unknown` (no space).
            // The shared helper keeps this gate identical to the one in
            // `JavaCode::is_func_space`.
            ObjectCreationExpression
                if crate::checker::java_anonymous_class_body(node).is_some() =>
            {
                SpaceKind::Class
            }
            Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> HalsteadType {
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
            | SEMI | COMMA | COLONCOLON | DOT | DASHGT | LBRACE | LBRACK | LPAREN
            // Operator: operators
            | EQ | LT | GT | BANG | TILDE | QMARK | COLON
            | EQEQ | LTEQ | GTEQ | BANGEQ | AMPAMP | PIPEPIPE | PLUSPLUS | DASHDASH
            | PLUS | DASH | STAR | SLASH | AMP | PIPE | CARET | PERCENT| LTLT | GTGT | GTGTGT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | AMPEQ | PIPEEQ | CARETEQ | PERCENTEQ | LTLTEQ | GTGTEQ | GTGTGTEQ
            // primitive types
            | Byte | Short | Int | Long | Char | Float | Double | BooleanType
            => {
                HalsteadType::Operator
            },
            // Operands: variables, constants, literals
            Identifier | NullLiteral | ClassLiteral | True | False | StringLiteral | CharacterLiteral | HexIntegerLiteral | OctalIntegerLiteral | BinaryIntegerLiteral | DecimalIntegerLiteral | HexFloatingPointLiteral | DecimalFloatingPointLiteral  => {
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
