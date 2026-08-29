//! `Getter` implementation for C++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::c_declarator::declarator_name;

impl Getter for CppCode {
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        _ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // Issue #285 contract: every `Cpp::FunctionDefinition*` alias
        // must be enumerated here AND in `get_space_kind` below AND
        // in `is_func` / `is_func_space` (see `src/checker.rs`).
        // The aliased kind_ids 489/491/494 are not emitted by the
        // currently pinned `tree-sitter-cpp` parse tables, so a
        // dropped variant would silently fall through to the
        // `_ => name-field` arm and yield the wrong name (or `None`).
        match node.kind_id().into() {
            Cpp::FunctionDefinition
            | Cpp::FunctionDefinition2
            | Cpp::FunctionDefinition3
            | Cpp::FunctionDefinition4 => {
                if let Some(op_cast) = node.first_child(|id| Cpp::OperatorCast == id) {
                    return node_text(code, &op_cast);
                }
                // The name is not a child of the function node — see
                // `crate::c_declarator` (#1208).
                if let Some(name) = declarator_name::<Self>(node)
                    && matches!(
                        name.kind_id().into(),
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
                            | Cpp::TemplateMethod
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
        use Cpp::*;

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
        use Cpp::*;

        // `LPAREN2` here (and the `LBRACK2`/`LBRACK3` aliases in the
        // Elixir/Ruby impls) is a defensive arm, not an active one: every
        // grammar's `public_symbol_map` collapses the second-alias opener
        // to its base before `Node::kind_id()` (`ts_node_symbol`) returns,
        // so `kind_id()` never yields the alias id and the arm cannot fire
        // for real source. It guards against a future grammar bump that
        // drops that collapse — at which point the alias would also need
        // folding to its pair glyph in `get_operator_id_as_str`. The
        // invariant is pinned by `second_alias_opener_collapses_to_base_kind_id`
        // in `metrics/halstead.rs` (issue #768).
        match node.kind_id().into() {
            // Raw-string delimiter punctuation. A `raw_string_literal`
            // carries its `R"(` opener as a bare `LPAREN` child — the
            // kind id a call or a grouping uses — so `R"(raw)"`
            // reported a `()` operator with no call in the source
            // (#1314, the C++ sibling of Elixir #1256 and Ruby/Perl
            // #1312). The literal is already an operand (below), so the
            // delimiter is suppressed exactly when its parent is that
            // node — the compound-leaf guard of grammar-dispatch
            // section 5. Verified across `R"(x)"`, the custom-delimiter
            // `R"tag(x)tag"` (which adds a `raw_string_delimiter` but
            // keeps the same `(`), and the `LR` / `u8R` prefixed forms;
            // the closing `)` needs no arm because #695 dropped every
            // closer from the operator set.
            //
            // Parent, not ancestor. That distinction is unobservable
            // here — `raw_string_content` is a leaf, so no `LPAREN` is
            // ever a deeper descendant of a raw string — but parent
            // scoping is correct by construction and keeps this arm the
            // same shape as its siblings. `mozcpp` carries the twin.
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

    get_operator!(Cpp);
}
