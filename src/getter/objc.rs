//! `Getter` implementation for Objective-C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for ObjcCode {
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        _ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // Issue #285 contract: every `Objc::FunctionDefinition*` alias
        // must be enumerated here AND in `get_space_kind` below AND in
        // `is_func` / `is_func_space` (see `src/checker.rs`). Free
        // functions reach their name through the C declarator chain; the
        // ObjC containers (`method_definition`, `class_interface` /
        // `class_implementation`, `protocol_declaration`) carry no `name`
        // field, so their name is the first `identifier` child — the
        // class / protocol name, or a method's first selector keyword.
        match node.kind_id().into() {
            Objc::FunctionDefinition | Objc::FunctionDefinition2 => {
                if let Some(declarator) = node.child_by_field_name("declarator")
                    && let Some(fd) = declarator.first_occurrence(|id| {
                        Objc::FunctionDeclarator == id
                            || Objc::FunctionDeclarator2 == id
                            || Objc::FunctionDeclarator3 == id
                            || Objc::FunctionDeclarator4 == id
                    })
                    && let Some(first) = fd.child(0)
                {
                    match first.kind_id().into() {
                        Objc::TypeIdentifier | Objc::Identifier | Objc::FieldIdentifier => {
                            return node_text(code, &first);
                        }
                        _ => {}
                    }
                }
            }
            Objc::MethodDefinition
            | Objc::ClassInterface
            | Objc::ClassImplementation
            | Objc::ProtocolDeclaration => {
                if let Some(ident) = node.first_child(|id| Objc::Identifier == id) {
                    return node_text(code, &ident);
                }
            }
            _ => {
                if let Some(name) = node.child_by_field_name("name") {
                    return node_text(code, &name);
                }
            }
        }
        None
    }

    fn get_space_kind(node: &Node) -> SpaceKind {
        use Objc::*;

        // `@interface` / `@protocol` declare members without bodies →
        // `Interface`; `@implementation` carries the method bodies →
        // `Class`; free functions and `method_definition`s are
        // `Function` spaces. ObjC blocks (`^{ … }`) are closures counted
        // by `nom` rather than their own space, mirroring the C++ lambda
        // (so they fall through to `Unknown` here). Keep the
        // `FunctionDefinition*` aliases listed (#285).
        match node.kind_id().into() {
            FunctionDefinition | FunctionDefinition2 | MethodDefinition => SpaceKind::Function,
            ClassImplementation => SpaceKind::Class,
            ClassInterface | ProtocolDeclaration => SpaceKind::Interface,
            TranslationUnit => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use Objc::*;

        // ObjC is C plus message sends, blocks, and the `@`-directives.
        // The operator alphabet is therefore the C set (`src/getter.rs`
        // `impl Getter for CCode`) extended with the ObjC structural
        // keywords: fast-enumeration `in`, the boxing/`@`-literal marker
        // `@`, the `@try` / `@catch` / `@finally` / `@throw` /
        // `@synchronized` / `@autoreleasepool` control keywords, and the
        // `@selector` / `@encode` compile-time directives. Each keeps a
        // distinct kind_id, so keying by kind_id keeps them distinct in n1.
        // `LPAREN2` is a defensive arm (collapsed to `LPAREN` before
        // `kind_id()`; #768, see the Cpp note).
        match node.kind_id().into() {
            DOT | LPAREN | LPAREN2 | COMMA | STAR | GTGT | COLON | SEMI | Return | Break
            | Continue | If | Else | Switch | Case | Default | For | While | Goto | Do | EQ
            | AMPAMP | PIPEPIPE | DASH | DASHDASH | DASHGT | PLUS | PLUSPLUS | SLASH | PERCENT
            | PIPE | AMP | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ | GT | PLUSEQ
            | DASHEQ | BANG | STAREQ | SLASHEQ | PERCENTEQ | GTGTEQ | LTLTEQ | AMPEQ | CARET
            | CARETEQ | PIPEEQ | LBRACK | LBRACE | QMARK | PrimitiveType | TypeSpecifier | Sizeof
            | Signed | Unsigned | Long | Short
            // ObjC-specific structural keywords / markers.
            | In | AT | ATtry | ATcatch | ATfinally | ATthrow | ATsynchronized
            | ATautoreleasepool | ATselector | ATencode => HalsteadType::Operator,
            Identifier | TypeIdentifier | FieldIdentifier | StringLiteral | NumberLiteral
            | True | False | Null | DOTDOTDOT => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Objc);
}
