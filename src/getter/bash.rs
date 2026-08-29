//! `Getter` implementation for Bash.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

/// Returns whether a Bash string node carries any expansion child that
/// would itself be classified as an operand by [`BashCode::get_op_type`]
/// (`$var`, `${name[…]}`, `$(cmd)`, `$((expr))`).
#[inline]
fn bash_string_has_expansion(node: &Node) -> bool {
    node.children().any(|c| {
        matches!(
            c.kind_id().into(),
            Bash::SimpleExpansion
                | Bash::Expansion
                | Bash::CommandSubstitution
                | Bash::ArithmeticExpansion
        )
    })
}

impl Getter for BashCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Bash::FunctionDefinition => SpaceKind::Function,
            Bash::Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> HalsteadType {
        match node.kind_id().into() {
            // Control flow and declaration keywords
            Bash::If | Bash::Then | Bash::Fi | Bash::Elif | Bash::Else
            | Bash::For | Bash::In | Bash::While | Bash::Until | Bash::Do | Bash::Done
            | Bash::Case | Bash::Esac
            | Bash::Function | Bash::Local | Bash::Declare | Bash::Typeset
            | Bash::Export | Bash::Readonly | Bash::Unset | Bash::Unsetenv
            // Punctuation acting as operators. Only the *opening*
            // delimiter counts — `get_operator_id_as_str` folds
            // `LPAREN`/`LBRACK`/`LBRACE` to the pair glyph, so a balanced
            // pair is one operator. Counting the matching closer too
            // (former `RPAREN`/`RBRACE`/`RBRACK`/`RBRACKRBRACK` arms)
            // double-counted every pair, inflating n1/N1 (#695). `[[`
            // (`LBRACKLBRACK`) is the test-command opener; its `]]` closer
            // is dropped for the same reason.
            | Bash::LPAREN | Bash::LBRACE
            | Bash::LBRACK | Bash::LBRACKLBRACK
            | Bash::SEMI | Bash::SEMISEMI | Bash::SEMIAMP | Bash::SEMISEMIAMP
            | Bash::COMMA | Bash::COLON
            // Assignment, arithmetic, and comparison operators
            | Bash::EQ | Bash::PLUSEQ | Bash::DASHEQ | Bash::STAREQ | Bash::SLASHEQ
            | Bash::PERCENTEQ | Bash::STARSTAREQ | Bash::LTLTEQ | Bash::GTGTEQ
            | Bash::AMPEQ | Bash::CARETEQ | Bash::PIPEEQ
            | Bash::PLUS | Bash::DASH | Bash::STAR | Bash::SLASH | Bash::PERCENT | Bash::STARSTAR
            | Bash::PLUSPLUS | Bash::DASHDASH
            | Bash::EQEQ | Bash::BANGEQ | Bash::LT | Bash::GT | Bash::LTEQ | Bash::GTEQ
            | Bash::EQTILDE
            // Logical and bitwise operators
            | Bash::AMPAMP | Bash::PIPEPIPE | Bash::PIPE | Bash::PIPEAMP
            | Bash::AMP | Bash::CARET | Bash::TILDE | Bash::BANG
            | Bash::LTLT | Bash::GTGT
            // Test operators (prefix)
            | Bash::DASHa | Bash::DASHo
            // Redirection operators
            | Bash::AMPGT | Bash::GTAMP | Bash::LTAMP | Bash::GTPIPE
            | Bash::LTAMPDASH | Bash::GTAMPDASH | Bash::LTLTDASH | Bash::AMPGTGT
            | Bash::LTLTLT
            // Ternary operator
            | Bash::QMARK | Bash::QMARK2
                => HalsteadType::Operator,

            // Quoted strings count as one operand when they are inert.
            // When they contain any `$var`/`${...}`/`$(...)`/`$((...))`
            // expansion child, those expansions are already walked and
            // classified as operands; counting the wrapping literal too
            // would double-count the inner identifiers (issue #180).
            // `RawString` is single-quoted and never interpolates, but
            // the check is uniform across the four string kinds for
            // clarity.
            Bash::String | Bash::RawString | Bash::AnsiCString | Bash::TranslatedString => {
                if bash_string_has_expansion(node) {
                    HalsteadType::Unknown
                } else {
                    HalsteadType::Operand
                }
            }

            // Operands: identifiers, literals, variables. `variable_name`
            // and `special_variable_name` each surface under multiple
            // aliased kind_ids (tree-sitter generates one per parse-table
            // context); every alias must be matched or assignment LHS
            // identifiers like `name` in `name=value` are silently
            // unclassified — see lesson 2.
            //
            // `command_name` is deliberately absent (#1351). The grammar
            // gives it exactly one required child — a `_primary_expression`
            // or a `concatenation` — and it contributes no text of its own,
            // so classifying the wrapper *and* letting the walk reach the
            // child counted every command name twice in N2 (`ls bar` scored
            // N2 3 for two operands). For a `"$cmd"` command name it also
            // planted a second vocabulary entry alongside the inner `$cmd`.
            // Deleting rather than gating is safe here because the wrapper
            // is never childless and the child kinds are the ordinary
            // operand kinds; the one residue — a brace `expansion` with no
            // `variable_name` leaf, `${#}` — scores zero in argument
            // position too, so the wrapper was masking that gap for command
            // names alone.
            //
            // `_concat` (`Bash::Concat`) is absent for a different reason:
            // it is a hidden zero-width external token the scanner emits
            // between `concatenation` parts, never a named node, and it
            // spells no operand even if a future grammar promoted it.
            // Lesson 34 prefers keeping a hidden-rule arm and pinning it,
            // but keeping this one would preserve a classification that is
            // wrong the moment it becomes reachable, so the arm goes and
            // only the marker stays:
            // `bash_hidden_concat_token_is_unreachable`.
            Bash::Word | Bash::Word2 | Bash::Word3 | Bash::Word4
            | Bash::Number | Bash::Number2 | Bash::NumberToken1 | Bash::NumberToken2
            | Bash::SimpleExpansion
                => HalsteadType::Operand,

            // `variable_name` / `special_variable_name` are operands when
            // they stand alone — the `name` in `name=value`, or the inner
            // leaf of a brace `${name}` `expansion` whose wrapper is *not*
            // itself an operand. But `$name` (and `$?`, `$1`, …) parses as a
            // `simple_expansion` wrapping the same leaf, and `SimpleExpansion`
            // is already an operand above; counting the inner leaf too would
            // double-count every bare `$var` reference (inflating n2/N2 and
            // every derived Halstead value). Exclude the leaf exactly in
            // that position — the same guard Tcl applies via its anonymous
            // `Id2` exclusion and iRules applies via a parent check on
            // `variable_substitution`.
            Bash::VariableName | Bash::VariableName2 | Bash::VariableName3
            | Bash::SpecialVariableName | Bash::SpecialVariableName2 => {
                if ancestors.parent_has_kind(node, Bash::SimpleExpansion as u16) {
                    HalsteadType::Unknown
                } else {
                    HalsteadType::Operand
                }
            }
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Bash);
}
