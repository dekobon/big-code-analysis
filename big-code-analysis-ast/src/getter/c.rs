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
        // Every `C::FunctionDefinition*` alias is enumerated here as on
        // the `FuncOpener` row (#285). C has no C++ name forms
        // (operator-cast / destructor / operator / qualified / template
        // names), so the declarator name is a plain identifier.
        // role-bypass: allow(FuncOpener) — a naming rule, not whether the
        // definition opens a space
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

    // The mapping is the `opens` annotations of `roles/c.rs`
    // (#1696). The rows read no bytes and no chain, so the byte-less
    // spelling forwards with none.
    fn get_space_kind(node: &Node) -> SpaceKind {
        Self::get_space_kind_with_code(node, &[], Ancestors::known(&[]))
    }

    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::c::opens_space(node, code, ancestors)
    }

    keyword_roles!(c);

    paying_roles!(c);

    fn get_op_type<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> TokenRole {
        use C::*;

        // C's operator alphabet is the C++ set minus the C++-only forms
        // (`.*` / `->*`, `new` / `delete`, `try` / `catch` / `throw`,
        // `<=>`). Raw string literals, the `namespace`-qualified
        // identifier and template lists likewise do not exist in C, so
        // nor does the C++ getters' `GT2` arm — a defensive one even
        // there, since the runtime folds the template-list closer onto
        // `GT` (`cpp_template_closer_alias_never_reaches_kind_id`).
        // `LPAREN2` is a defensive arm (collapsed to `LPAREN` before
        // `kind_id()`; #768, see the Cpp note). The modifiers, the
        // declaration keywords and the primitive types are not here:
        // `roles/c.rs` declares them and `get_keyword_role` bills them
        // (#1693). Nor are `<=` `>=` `==` `!=` `&&` `||`, the ternary
        // `?` and the `case` label: the `Comparison`, `ShortCircuit`,
        // `Ternary` and `CaseArm` rows pay them ungated and
        // `get_paying_role` bills them (#1695). `<` / `>` stay for the spelling the rows'
        // `under BinaryExpression` gate leaves out — a token error
        // recovery strands under an `ERROR` — which billed before the
        // rows existed; for those two the row is not what bills them.
        // role-bypass: allow(Comparison, StringLit) — the operator classification of the
        // spelling the row's `under BinaryExpression` gate leaves out (a
        // recovered token), and the operand classification of the string
        // literals — a projection the `StringLit` row does not answer
        match node.kind_id().into() {
            LT | GT | DOT | LPAREN | LPAREN2 | COMMA | STAR | GTGT | COLON | SEMI | Return
            | Break | Continue | If | Else | Switch | Default | For | While | Goto | Do | EQ
            | DASH | DASHDASH | DASHGT | PLUS | PLUSPLUS | SLASH | PERCENT | PIPE | AMP | LTLT
            | TILDE | PLUSEQ | DASHEQ | BANG | STAREQ | SLASHEQ | PERCENTEQ | GTGTEQ | LTLTEQ
            | AMPEQ | CARET | CARETEQ | PIPEEQ | LBRACK | LBRACE | Sizeof => TokenRole::Operator,
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
            | NumberLiteral | True | False | Null | DOTDOTDOT => TokenRole::Operand,
            _ => TokenRole::Unknown,
        }
    }

    get_operator!(C);
}
