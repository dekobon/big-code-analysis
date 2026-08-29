//! `Getter` implementation for MozC++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::c_declarator::declarator_name;

impl Getter for MozcppCode {
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        _ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // Issue #285 contract: every `Mozcpp::FunctionDefinition*` alias
        // must be enumerated here AND in `get_space_kind` below AND
        // in `is_func` / `is_func_space` (see `src/checker.rs`).
        // The aliased kind_ids 489/491/494 are not emitted by the
        // currently pinned `tree-sitter-mozcpp` parse tables, so a
        // dropped variant would silently fall through to the
        // `_ => name-field` arm and yield the wrong name (or `None`).
        match node.kind_id().into() {
            Mozcpp::FunctionDefinition
            | Mozcpp::FunctionDefinition2
            | Mozcpp::FunctionDefinition3
            | Mozcpp::FunctionDefinition4 => {
                if let Some(op_cast) = node.first_child(|id| Mozcpp::OperatorCast == id) {
                    return node_text(code, &op_cast);
                }
                // The name is not a child of the function node — see
                // `crate::c_declarator` (#1208).
                if let Some(name) = declarator_name::<Self>(node)
                    && matches!(
                        name.kind_id().into(),
                        Mozcpp::TypeIdentifier
                            | Mozcpp::Identifier
                            | Mozcpp::FieldIdentifier
                            | Mozcpp::DestructorName
                            | Mozcpp::OperatorName
                            | Mozcpp::QualifiedIdentifier
                            | Mozcpp::QualifiedIdentifier2
                            | Mozcpp::QualifiedIdentifier3
                            | Mozcpp::QualifiedIdentifier4
                            | Mozcpp::TemplateFunction
                            | Mozcpp::TemplateMethod
                    )
                {
                    return node_text(code, &name);
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
        use Mozcpp::*;

        // Issue #285 contract: keep every `FunctionDefinition*` alias
        // listed here — see the comment above `get_func_space_name`.
        match node.kind_id().into() {
            FunctionDefinition | FunctionDefinition2 | FunctionDefinition3
            | FunctionDefinition4 => SpaceKind::Function,
            StructSpecifier => SpaceKind::Struct,
            ClassSpecifier => SpaceKind::Class,
            NamespaceDefinition => SpaceKind::Namespace,
            TranslationUnit => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use Mozcpp::*;

        // `LPAREN2` is a defensive arm (collapsed to `LPAREN` before
        // `kind_id()`; #768, see the Cpp note).
        match node.kind_id().into() {
            // Raw-string delimiter punctuation — the twin of the Cpp
            // arm, which carries the derivation (#1314). `.mozcpp` owns
            // no file extension, so nothing routes to it and it gets no
            // integration-snapshot coverage; the parity assertion in
            // `tests/parity/cpp_mozcpp_parity.rs` is what keeps this
            // clone from drifting.
            LPAREN
                if ancestors.parent_has_kind(node, RawStringLiteral as u16) =>
            {
                HalsteadType::Unknown
            }
            DOT | DOTSTAR | LPAREN | LPAREN2 | COMMA | STAR | GTGT | COLON | SEMI | Return
            | Break | Continue | If | Else | Switch | Case | Default | For | While | Goto | Do
            | Delete | New | Try | Try2 | Catch | Throw | EQ | AMPAMP | PIPEPIPE | DASH
            | DASHDASH | DASHGT | DASHGTSTAR | PLUS | PLUSPLUS | SLASH | PERCENT | PIPE | AMP
            | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ | GT | GT2 | LTEQGT | PLUSEQ
            | DASHEQ | BANG | STAREQ | SLASHEQ | PERCENTEQ | GTGTEQ | LTLTEQ | AMPEQ | CARET
            | CARETEQ | PIPEEQ | LBRACK | LBRACE | QMARK | COLONCOLON | PrimitiveType
            | TypeSpecifier | Sizeof
            // A `sized_type_specifier` carries its `unsigned`/`signed`/`long`/
            // `short` modifiers as bare keyword tokens, not as `primitive_type`
            // children (`unsigned int` is `unsigned` + `primitive_type int`;
            // `signed long` and `long long` have no `primitive_type` at all).
            // Without these arms the modifiers fell into `Unknown` and were
            // dropped, so `unsigned int` collapsed to just `int` and a standalone
            // `signed long` contributed nothing to n1/N1 (issue #466). Each
            // modifier has a distinct kind_id, so keying by kind_id (the default
            // `operators` store) keeps them distinct in n1 while `long long`'s
            // two `long` tokens correctly fold to one n1 entry but two N1 hits.
            | Signed | Unsigned | Long | Short => HalsteadType::Operator,
            // `CharLiteral` — the full derivation lives on the same arm
            // in `src/getter/c.rs` (#1316): the wrapper is the only
            // classified node in a character literal, so it bills one
            // operand per literal, keyed by text, and `Checker::is_string`
            // deliberately stays without a `CharLiteral` arm.
            Identifier | TypeIdentifier | FieldIdentifier | RawStringLiteral | StringLiteral
            | CharLiteral | NumberLiteral | True | False | Null | DOTDOTDOT => {
                HalsteadType::Operand
            }
            // A namespace identifier is an operand only where it
            // *names* a namespace; the same kind also spells the
            // qualifier in `ns::thing`, which the final arm leaves
            // `Unknown` (#1096).
            NamespaceIdentifier if ancestors.parent_has_kind(node, NamespaceDefinition as u16) => {
                HalsteadType::Operand
            }
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Mozcpp);
}
