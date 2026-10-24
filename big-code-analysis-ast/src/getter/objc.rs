//! `Getter` implementation for Objective-C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::c_declarator::declarator_name;

// ObjC has no templates, but four nodes delimit a generic list with a
// bare `<` / `>`: `generic_specifier` (`NSArray<NSString *>`),
// `protocol_reference_list` (`id<NSCopying>`), `parameterized_arguments`
// (`@interface A : NSObject <NSCopying>`), and `argument_list`, whose
// `Type<…>` alternative (`f(NSArray<NSString *>)`) holds the brackets as
// direct children. A well-formed comparison's `<` / `>` is the child of
// a `binary_expression` or `preproc_binary_expression`; the one
// exception is `objc_argument_list_misparse`. `LT2` is the
// `token.immediate` `<` that `argument_list`'s form opens with; the
// parser reports it as `LT`, so it is listed defensively, here and in
// the operator arm, and its absence is pinned by
// `objc_generic_opener_alias_never_reaches_kind_id`. The call's
// `argument_list` is `ArgumentList2`; `ArgumentList` is
// `preproc_call_expression`'s alias, which holds no angle brackets.
const GENERIC_ANGLES: GenericAngleKinds = GenericAngleKinds {
    lists: &[
        Objc::GenericSpecifier as u16,
        Objc::ProtocolReferenceList as u16,
        Objc::ParameterizedArguments as u16,
        Objc::ArgumentList2 as u16,
    ],
    openers: &[Objc::LT as u16, Objc::LT2 as u16],
    closers: &[Objc::GT as u16],
    is_misparse: objc_argument_list_misparse,
};

// Whether `list` is an `argument_list` holding two comparisons that
// tree-sitter-objc read as its `Type<…>` form: an unspaced
// `g(a<b, c>d)`, where a `<` glued to the first argument opens the form.
// The real form is the whole argument list, so `f(NSArray<NSString *>)`
// parses cleanly, while the misparse leaves an `ERROR` after the `>`, or
// a MISSING `)` (`g(i<n, j>=0)`) that only `has_error` sees.
fn objc_argument_list_misparse(list: &Node, _ancestors: Ancestors) -> bool {
    list.kind_id() == Objc::ArgumentList2 as u16 && list.has_error()
}

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
                // role-bypass: allow(Modifier) — naming: which declarator leaf is the name, not
                // whether an identifier is a property-attribute word
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

    keyword_roles!(objc);

    paying_roles!(objc);

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Objc::*;

        if GENERIC_ANGLES.is_closer(node, ancestors) {
            return TokenRole::Unknown;
        }

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
            AT if ancestors.parent_has_kind(node, StringLiteral as u16) => TokenRole::Unknown,
            // The C operator set, then the ObjC-specific structural
            // keywords / markers from `In` onwards. `in` is one kind for
            // the fast-enumeration keyword and the method-type
            // qualifier, so it bills here rather than on the `Modifier`
            // row. The modifiers, the declaration keywords and the
            // primitive types are not here: `roles/objc.rs` declares
            // them and `get_keyword_role` bills them (#1693). Nor are
            // `<=` `>=` `==` `!=` `&&` `||`: the `Comparison` and
            // `ShortCircuit` rows pay them ungated and `get_paying_role`
            // bills them (#1695). `<` / `>` stay for the spellings the
            // rows' `under_binary_expression` gate leaves out — a generic
            // specifier, a protocol list (`id<NSCopying>`), a recovered
            // token — which billed before the rows existed; `LT2` is the
            // grammar's own generic-opener kind and pays nowhere.
            // role-bypass: allow(Comparison) — the operator classification of the spellings the
            // row's `under_binary_expression` gate leaves out
            LT | GT | LT2 | DOT | LPAREN | LPAREN2 | COMMA | STAR | GTGT | COLON | SEMI
            | Return | Break | Continue | If | Else | Switch | Case | Default | For | While
            | Goto | Do | EQ | DASH | DASHDASH | DASHGT | PLUS | PLUSPLUS | SLASH | PERCENT
            | PIPE | AMP | LTLT | TILDE | PLUSEQ | DASHEQ | BANG | STAREQ | SLASHEQ | PERCENTEQ
            | GTGTEQ | LTLTEQ | AMPEQ | CARET | CARETEQ | PIPEEQ | LBRACK | LBRACE | QMARK
            | Sizeof | In | AT | ATtry | ATcatch | ATfinally | ATthrow | ATsynchronized
            | ATautoreleasepool | ATselector | ATencode => TokenRole::Operator,
            // `CharLiteral` — the full derivation lives on the same arm
            // in `src/getter/c.rs` (#1316): the wrapper is the only
            // classified node in a character literal, so it bills one
            // operand per literal, keyed by text, and `Checker::is_string`
            // deliberately stays without a `CharLiteral` arm.
            //
            // An `identifier` is an operand unless it is a property
            // attribute's word, which the `Modifier` row claims under
            // its gate and `get_keyword_role` bills first.
            // role-bypass: allow(Modifier) — the operand classification of the ungated spelling
            Identifier | TypeIdentifier | FieldIdentifier | StringLiteral | CharLiteral
            | NumberLiteral | True | False | Null | DOTDOTDOT => TokenRole::Operand,
            _ => TokenRole::Unknown,
        }
    }

    fn get_operator_spelling<'a>(
        node: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
    ) -> Option<&'static str> {
        GENERIC_ANGLES.opener_spelling(node, ancestors)
    }

    get_operator!(Objc);
}
