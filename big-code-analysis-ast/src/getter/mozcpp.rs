//! `Getter` implementation for MozC++.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;
use crate::c_declarator::declarator_name;

// `GT2` is listed defensively, as in the Cpp getter.
const GENERIC_ANGLES: GenericAngleKinds = GenericAngleKinds {
    lists: &[
        Mozcpp::TemplateArgumentList as u16,
        Mozcpp::TemplateParameterList as u16,
    ],
    openers: &[Mozcpp::LT as u16],
    closers: &[Mozcpp::GT as u16, Mozcpp::GT2 as u16],
    is_misparse: never_misparsed,
};

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
        // role-bypass: allow(FuncOpener) — a naming rule, not whether the
        // definition opens a space
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

    // The mapping is the `opens` annotations of `roles/mozcpp.rs`
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
        crate::roles::mozcpp::opens_space(node, code, ancestors)
    }

    keyword_roles!(mozcpp);

    paying_roles!(mozcpp);

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Mozcpp::*;

        if GENERIC_ANGLES.is_closer(node, ancestors) {
            return TokenRole::Unknown;
        }

        // `LPAREN2` is a defensive arm (collapsed to `LPAREN` before
        // `kind_id()`; #768, see the Cpp note).
        // The modifiers, the declaration keywords and the primitive
        // types are not here: `roles/mozcpp.rs` declares them and
        // `get_keyword_role` bills them (#1693).
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
                TokenRole::Unknown
            }
            // The comparison and short-circuit tokens sit in this arm for
            // the spellings that apply no operator, the rows paying the
            // applied ones — the derivation is on the Cpp twin (#1694).
            // The ternary `?` and the `case` label are not here: the
            // `Ternary` and `CaseArm` rows pay them ungated.
            // role-bypass: allow(Comparison, ShortCircuit) — the operator classification of the
            // spellings the rows' `applied` gate leaves out
            LT | LTEQ | EQEQ | BANGEQ | GTEQ | GT | GT2 | LTEQGT | NotEq | AMPAMP | PIPEPIPE
            | And | Or
            | DOT | DOTSTAR | LPAREN | LPAREN2 | COMMA | STAR | GTGT | COLON | SEMI | Return
            | Break | Continue | If | Else | Switch | Default | For | While | Goto | Do
            | Delete | New | Try | Try2 | Catch | Throw | EQ | DASH
            | DASHDASH | DASHGT | DASHGTSTAR | PLUS | PLUSPLUS | SLASH | PERCENT | PIPE | AMP
            | LTLT | TILDE | PLUSEQ
            | DASHEQ | BANG | STAREQ | SLASHEQ | PERCENTEQ | GTGTEQ | LTLTEQ | AMPEQ | CARET
            | CARETEQ | PIPEEQ | LBRACK | LBRACE | COLONCOLON | Sizeof
            // The ISO alternative tokens (`not`, `compl`, `bitand`, …) —
            // the derivation is on the Cpp twin (#1522).
            | Not | Compl | Bitand | Bitor | Xor | AndEq | OrEq | XorEq
            // An overloaded operator's name, the `operator` keyword and
            // `co_await` — the derivation is on the Cpp twin (#1296).
            | LPARENRPAREN | LBRACKRBRACK | DQUOTEDQUOTE | CoAwait | Operator
            // `co_return` / `co_yield`, billed as `return` is — the
            // derivation is on the Cpp twin (#1547).
            | CoReturn | CoYield
                => TokenRole::Operator,
            // `CharLiteral` — the full derivation lives on the same arm
            // in `src/getter/c.rs` (#1316): the wrapper is the only
            // classified node in a character literal, so it bills one
            // operand per literal, keyed by text, and `Checker::is_string`
            // deliberately stays without a `CharLiteral` arm.
            //
            // `This` — the operator-vs-operand derivation lives on the
            // same arm in `src/getter/cpp.rs` (#1361), where the fork's
            // upstream carries it. In short: `this` had been in neither
            // arm and so contributed nothing, and it is an operand
            // because it is the *receiver* half of `<recv> -> <field>`,
            // exactly where `p` stands in `p->x`. Mozcpp owns no file
            // extension, so nothing routes to it and no integration
            // snapshot reaches this clone — `cpp_and_mozcpp_agree_on_this`
            // in `tests/parity/cpp_mozcpp_parity.rs` is what keeps it
            // from drifting, the same guard the `RawStringLiteral`
            // `LPAREN` arm above relies on.
            Identifier | TypeIdentifier | FieldIdentifier | RawStringLiteral | StringLiteral
            | CharLiteral | NumberLiteral | True | False | Null | This | DOTDOTDOT => {
                TokenRole::Operand
            }
            // A namespace identifier is an operand only where it
            // *names* a namespace; the same kind also spells the
            // qualifier in `ns::thing`, which the final arm leaves
            // `Unknown` (#1096).
            NamespaceIdentifier if ancestors.parent_has_kind(node, NamespaceDefinition as u16) => {
                TokenRole::Operand
            }
            _ => TokenRole::Unknown,
        }
    }

    fn get_operator_spelling<'a>(
        node: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
    ) -> Option<&'static str> {
        GENERIC_ANGLES.opener_spelling(node, ancestors)
    }

    // The overloaded operator names render as the operator they name;
    // the derivation is on the Cpp twin (#1296).
    fn get_operator_id_as_str(id: u16) -> &'static str {
        let typ = id.into();
        match typ {
            Mozcpp::LPAREN => "()",
            Mozcpp::LBRACK => "[]",
            Mozcpp::LBRACE => "{}",
            Mozcpp::LPARENRPAREN => "operator()",
            Mozcpp::LBRACKRBRACK => "operator[]",
            _ => typ.into(),
        }
    }
}
