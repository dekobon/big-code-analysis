//! `Getter` implementation for Objective-C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::c_declarator::declarator_name;

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
                // The name is not a child of the function node — see
                // `crate::c_declarator` (#1208).
                if let Some(name) = declarator_name::<Self>(node)
                    && matches!(
                        name.kind_id().into(),
                        Objc::TypeIdentifier | Objc::Identifier | Objc::FieldIdentifier
                    )
                {
                    return node_text(code, &name);
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

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> HalsteadType {
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
            // `@"…"` is one `string_literal` holding its `@` as a child
            // (tree-sitter-objc 3.0.2), unlike `@42` / `@[…]` / `@{…}`,
            // where the `@` is a child of the `at_expression` /
            // `array_literal` / `dictionary_literal` it boxes. The
            // literal is already the operand, keyed by its whole text,
            // so billing the marker as an operator too paid the same
            // byte twice and planted a phantom `@` in n1 for a file
            // whose only `@` was in NSString literals (grammar-dispatch
            // §5, the compound-leaf guard).
            AT if ancestors.parent_has_kind(node, StringLiteral as u16) => HalsteadType::Unknown,
            // The C operator set, then the ObjC-specific structural
            // keywords / markers from `In` onwards.
            DOT | LPAREN | LPAREN2 | COMMA | STAR | GTGT | COLON | SEMI | Return | Break
            | Continue | If | Else | Switch | Case | Default | For | While | Goto | Do | EQ
            | AMPAMP | PIPEPIPE | DASH | DASHDASH | DASHGT | PLUS | PLUSPLUS | SLASH | PERCENT
            | PIPE | AMP | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ | GT | PLUSEQ
            | DASHEQ | BANG | STAREQ | SLASHEQ | PERCENTEQ | GTGTEQ | LTLTEQ | AMPEQ | CARET
            | CARETEQ | PIPEEQ | LBRACK | LBRACE | QMARK | PrimitiveType | TypeSpecifier
            | Sizeof | Signed | Unsigned | Long | Short | In | AT | ATtry | ATcatch | ATfinally
            | ATthrow | ATsynchronized | ATautoreleasepool | ATselector | ATencode => {
                HalsteadType::Operator
            }
            // `CharLiteral` — the full derivation lives on the same arm
            // in `src/getter/c.rs` (#1316): the wrapper is the only
            // classified node in a character literal, so it bills one
            // operand per literal, keyed by text, and `Checker::is_string`
            // deliberately stays without a `CharLiteral` arm.
            Identifier | TypeIdentifier | FieldIdentifier | StringLiteral | CharLiteral
            | NumberLiteral | True | False | Null | DOTDOTDOT => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Objc);
}
