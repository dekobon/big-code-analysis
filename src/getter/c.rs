//! `Getter` implementation for C.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::c_declarator::declarator_name;

impl Getter for CCode {
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        _ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // Issue #285 contract: every `C::FunctionDefinition*` alias must
        // be enumerated here AND in `get_space_kind` below AND in
        // `is_func` / `is_func_space` (see `src/checker.rs`). C has no
        // C++ name forms (operator-cast / destructor / operator / qualified
        // / template names), so the declarator name is a plain identifier.
        match node.kind_id().into() {
            C::FunctionDefinition | C::FunctionDefinition2 => {
                // The name is not a child of the function node — see
                // `crate::c_declarator` (#1208).
                if let Some(name) = declarator_name::<Self>(node)
                    && matches!(
                        name.kind_id().into(),
                        C::TypeIdentifier | C::Identifier | C::FieldIdentifier
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
        use C::*;

        // C has no classes/namespaces, and struct/union/enum hold no
        // functions, so the only spaces are functions and the unit
        // (matching `is_func_space` in `src/checker.rs`, #285).
        match node.kind_id().into() {
            FunctionDefinition | FunctionDefinition2 => SpaceKind::Function,
            TranslationUnit => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use C::*;

        // C's operator alphabet is the C++ set minus the C++-only forms
        // (`.*` / `->*`, `new` / `delete`, `try` / `catch` / `throw`,
        // `<=>`, the `>>`-closing `GT2`). Raw string literals and the
        // `namespace`-qualified identifier likewise do not exist in C.
        // `LPAREN2` is a defensive arm (collapsed to `LPAREN` before
        // `kind_id()`; #768, see the Cpp note).
        match node.kind_id().into() {
            DOT | LPAREN | LPAREN2 | COMMA | STAR | GTGT | COLON | SEMI | Return | Break
            | Continue | If | Else | Switch | Case | Default | For | While | Goto | Do | EQ
            | AMPAMP | PIPEPIPE | DASH | DASHDASH | DASHGT | PLUS | PLUSPLUS | SLASH | PERCENT
            | PIPE | AMP | LTLT | TILDE | LT | LTEQ | EQEQ | BANGEQ | GTEQ | GT | PLUSEQ
            | DASHEQ | BANG | STAREQ | SLASHEQ | PERCENTEQ | GTGTEQ | LTLTEQ | AMPEQ | CARET
            | CARETEQ | PIPEEQ | LBRACK | LBRACE | QMARK | PrimitiveType
            | TypeSpecifier | Sizeof
            // A `sized_type_specifier` carries its `unsigned`/`signed`/`long`/
            // `short` modifiers as bare keyword tokens, not as `primitive_type`
            // children (`unsigned int` is `unsigned` + `primitive_type int`;
            // `signed long` and `long long` have no `primitive_type` at all).
            // Without these arms the modifiers fall into `Unknown` and are
            // dropped, so `unsigned int` collapses to just `int` and a standalone
            // `signed long` contributes nothing to n1/N1 (issue #466). Each
            // modifier has a distinct kind_id, so keying by kind_id (the default
            // `operators` store) keeps them distinct in n1 while `long long`'s
            // two `long` tokens correctly fold to one n1 entry but two N1 hits.
            | Signed | Unsigned | Long | Short => HalsteadType::Operator,
            // `CharLiteral` joins the operand list here for the whole
            // C family — `cpp.rs`, `mozcpp.rs` and `objc.rs` carry the
            // same arm and point back at this note (#1316). Before it a
            // character literal contributed *nothing*: not an operator
            // (correct) and not an operand (wrong), so `char b = 'x';`
            // scored `b` alone while Rust, Java, Kotlin, C#, Go and
            // Elixir all counted their character literal.
            //
            // Listing the wrapper is safe against the grammar-dispatch
            // section 5 double count because none of `char_literal`'s
            // children is classified: the opening delimiter (`'`, and
            // the `L'` / `u'` / `U'` / `u8'` prefixed spellings, each a
            // distinct kind), the closing `'`, and the `character` /
            // `escape_sequence` payload are all absent from every arm
            // in this match. So a literal bills exactly one operand
            // however many `character` leaves it holds — a multi-char
            // constant like `'ab'` has two and still counts once.
            //
            // Operands are keyed by source text (`get_operand_id`), so
            // `'x'` and `L'x'` are separate vocabulary entries while a
            // repeated `'x'` folds to one `n2` with two `N2` hits.
            //
            // `Checker::is_string` deliberately does **not** grow a
            // `CharLiteral` arm to match: a char is not a string, the
            // same split Rust and Go apply to their char / rune
            // literals. The alterator flattens `CharLiteral` in all four
            // languages and records that decision alongside each arm;
            // `c_family_char_literal_is_not_a_string` in `checker.rs`
            // pins it both ways.
            Identifier | TypeIdentifier | FieldIdentifier | StringLiteral | CharLiteral
            | NumberLiteral | True | False | Null | DOTDOTDOT => HalsteadType::Operand,
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(C);
}
