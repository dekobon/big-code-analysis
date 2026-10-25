//! `Getter` implementation for iRules.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

use crate::lang_helpers::irules::BRACED_WORD_KINDS;

impl Getter for IrulesCode {
    // The mapping is the `opens` annotations of `roles/irules.rs`
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
        crate::roles::irules::opens_space(node, code, ancestors)
    }

    keyword_roles!(irules);

    paying_roles!(irules);

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        match node.kind_id().into() {
            // The braced-word rule (#1314, #1354 / #1317) — the twin of
            // the Tcl arm, which carries the derivation and every
            // caveat, over the shared
            // `Getter::is_subsumed_braced_word`. The same split holds
            // at this grammar's own ids: handler and `if` bodies are
            // `BracedWord` (132), conditions are `Expr` (141), and only
            // the value form is `BracedWordSimple` (133), which admits
            // the same six child kinds Tcl's does.
            _ if Self::is_subsumed_braced_word(node, ancestors, &BRACED_WORD_KINDS) => {
                TokenRole::Unknown
            }
            // Anonymous keyword tokens (the `*2` aliases are the keyword
            // literals; the unsuffixed high-id variants are the statement
            // nodes counted by the branching metrics, not here). The
            // declaration keywords (`proc`, `when`, `namespace`, `set`,
            // `global`) are not here: `roles/irules.rs` declares them
            // and `get_keyword_role` bills them (#1693).
            Irules::On
            | Irules::Off
            | Irules::Trap
            | Irules::If2
            | Irules::Elseif2
            | Irules::Else2
            | Irules::While2
            | Irules::For2
            | Irules::Foreach2
            | Irules::Switch2
            | Irules::Try2
            | Irules::Catch2
            | Irules::Finally2
            | Irules::Regexp2
            | Irules::Expr2
            | Irules::Dict
            | Irules::Update
            | Irules::With
            // The comparison and short-circuit operators — symbolic,
            // the string comparators (`eq`, `contains`, `matches`, …) and
            // the word forms `and` / `or` — are not here: the `Comparison`
            // and `ShortCircuit` rows of `roles/irules.rs` pay them and
            // `get_paying_role` bills them (#1695).
            // Structural punctuation. Only the *opening* delimiter is an
            // operator (the pair folds to one glyph in
            // `get_operator_id_as_str`); counting the matching closer too
            // (former `RBRACE`/`RBRACK`/`RPAREN` arms) double-counted every
            // balanced pair, inflating n1/N1 (#695).
            // `LPAREN2` is defensive — the runtime collapses it to `LPAREN`
            // before `kind_id()`, so it never fires (#768; see the Cpp note).
            | Irules::LBRACE
            | Irules::LBRACK
            | Irules::LPAREN
            | Irules::LPAREN2
            | Irules::SEMI
            | Irules::COLON
            | Irules::COLONCOLON
            | Irules::NsDelim
            // Arithmetic / exponent operators.
            | Irules::PLUS
            | Irules::DASH
            | Irules::STAR
            | Irules::SLASH
            | Irules::PERCENT
            | Irules::STARSTAR
            // Bitwise operators.
            | Irules::AMP
            | Irules::PIPE
            | Irules::CARET
            | Irules::TILDE
            | Irules::LTLT
            | Irules::GTGT
            // Logical negation, symbolic and keyword. The ternary `?` is
            // the `Ternary` row's and `get_paying_role` bills it (#1695).
            | Irules::BANG
            | Irules::Not => TokenRole::Operator,

            // `Id` (named, id 49) is a standalone identifier operand — e.g.
            // a `set` target (`set s …` → `s`). But it is ALSO the inner
            // leaf of every `variable_substitution` (`$s` → `(variable_
            // substitution (id "s"))`). The `VariableSubstitution` node is
            // already the operand for the reference, so counting its inner
            // `Id` too would double-count every `$var` (inflating n2/N2 and
            // every derived Halstead value). Exclude `Id` exactly in that
            // position. NB: Tcl has the identical shape — its anonymous
            // `Id2` token is emitted both as the `set` target and as the
            // var-sub leaf, so it needs this same parent guard; the blanket
            // `Id2` kind exclusion it used to carry dropped every `set`
            // target from n2/N2 (#1294). (`Id2` here is a different,
            // non-surfacing token.)
            Irules::Id => {
                if ancestors.parent_has_kind(node, Irules::VariableSubstitution as u16) {
                    TokenRole::Unknown
                } else {
                    TokenRole::Operand
                }
            }

            // Operands: identifiers and literals.
            //
            // `BracedWord` (132) reaches this arm only when the guard
            // above let it through, which since #1354 means only when
            // it holds no command — the reason the Tcl twin spells
            // out. A `when` handler body is the largest instance of the
            // over-count that removed: its operand text was the entire
            // event handler.
            //
            // `ArrayIndex` is deliberately absent, as in the Tcl twin:
            // the grammar makes `(k)` a pure wrapper around one of the
            // six word kinds, and its parens are already the `()`
            // operator, so listing it billed `$arr(k)` three times —
            // the reference, the wrapper and the index (grammar-
            // dispatch §5). The reference and the index are the two
            // operands, the index being walked because Tcl substitutes
            // inside the parens (`$arr($i)`), unlike inside braces.
            Irules::SimpleWord
            | Irules::Number
            | Irules::Boolean
            | Irules::EventName
            | Irules::BracedWord
            | Irules::BracedWordSimple
            | Irules::VariableSubstitution => TokenRole::Operand,

            // Double-quoted strings count as a single operand when inert
            // (`"hello world"`). When they carry a `$var` or `[cmd]`
            // interpolation child, the inner substitution nodes are walked
            // separately and contribute their own operands; counting the
            // wrapping `QuotedWord` too would double-count `N2` (same pattern
            // as Tcl #277 / Bash #180 / C# #183 / PHP #184).
            Irules::QuotedWord => Self::string_operand_type(
                node,
                &[
                    Irules::VariableSubstitution as u16,
                    Irules::CommandSubstitution as u16,
                ],
            ),

            _ => TokenRole::Unknown,
        }
    }

    // The byte-reading half of the braced-word rule (#1318) — the twin
    // of the Tcl override, which carries the derivation. This grammar
    // models more of the script positions than Tcl's does (`for`,
    // `switch` with `switch_arm` children, `when`, and the three `dict`
    // loops all have nodes of their own), so fewer braced words reach
    // the command-name recognition here; the ones that do — `eval`,
    // `uplevel`, `after`, `time` and every user proc — behave exactly
    // as they do in Tcl. Two entries in that list exist for this
    // dialect alone: `on_handler` / `trap_handler` are emitted only
    // under `try` (pinned by
    // `irules_try_handler_kinds_appear_only_under_try`), so a
    // statement-level `on error {…}` or `trap {…}` is a generic
    // command and needs its name recognised.
    fn get_op_type_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> TokenRole {
        Self::braced_word_op_type(node, code, ancestors, &BRACED_WORD_KINDS)
    }

    get_operator!(Irules);
}
