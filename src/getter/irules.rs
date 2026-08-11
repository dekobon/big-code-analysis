//! `Getter` implementation for iRules.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for IrulesCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            // Event handlers and procs are the function units (see the
            // `IrulesCode` Checker impl for the handler-as-function
            // rationale, and for why `try` handlers are excluded — this arm
            // must stay gated by the same set as `is_func_space`,
            // grammar-dispatch §6/§7).
            Irules::Procedure | Irules::WhenEvent => SpaceKind::Function,
            Irules::SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> HalsteadType {
        match node.kind_id().into() {
            // Braced-word delimiter punctuation — the twin of the Tcl
            // arm, which carries the derivation (#1314). The same
            // discriminator holds at this grammar's own ids: handler
            // and `if` bodies are `BracedWord` (132), conditions are
            // `Expr` (141), and only the value form is
            // `BracedWordSimple` (133).
            Irules::LBRACE
                if ancestors.parent_has_kind(node, Irules::BracedWordSimple as u16) =>
            {
                HalsteadType::Unknown
            }
            // Anonymous keyword tokens (the `*2` aliases are the keyword
            // literals; the unsuffixed high-id variants are the statement
            // nodes counted by the branching metrics, not here).
            Irules::Proc
            | Irules::When
            | Irules::On
            | Irules::Off
            | Irules::Trap
            | Irules::If2
            | Irules::Elseif2
            | Irules::Else2
            | Irules::While2
            | Irules::For2
            | Irules::Foreach2
            | Irules::Switch2
            | Irules::Set2
            | Irules::Global2
            | Irules::Namespace2
            | Irules::Try2
            | Irules::Catch2
            | Irules::Finally2
            | Irules::Regexp2
            | Irules::Expr2
            | Irules::Dict
            | Irules::Update
            | Irules::With
            // String comparison operators (`eq`, `contains`, `matches`, …):
            // operators, not branches (the branching metrics ignore them).
            | Irules::Eq
            | Irules::Ne
            | Irules::StartsWith
            | Irules::EndsWith
            | Irules::Contains
            | Irules::Equals
            | Irules::Matches
            | Irules::MatchesRegex
            | Irules::MatchesGlob
            | Irules::In
            | Irules::Ni
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
            // Comparison operators.
            | Irules::EQEQ
            | Irules::BANGEQ
            | Irules::LT
            | Irules::GT
            | Irules::LTEQ
            | Irules::GTEQ
            // Logical operators (symbolic and keyword forms).
            | Irules::BANG
            | Irules::Not
            | Irules::AMPAMP
            | Irules::And
            | Irules::PIPEPIPE
            | Irules::Or
            // Ternary conditional operator.
            | Irules::QMARK => HalsteadType::Operator,

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
                if ancestors.parent_has_kind(node, Irules::VariableSubstitution as u16)
                {
                    HalsteadType::Unknown
                } else {
                    HalsteadType::Operand
                }
            }

            // Operands: identifiers and literals.
            Irules::SimpleWord
            | Irules::Number
            | Irules::Boolean
            | Irules::EventName
            | Irules::BracedWord
            | Irules::BracedWordSimple
            | Irules::ArrayIndex
            | Irules::VariableSubstitution => HalsteadType::Operand,

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

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Irules);
}
