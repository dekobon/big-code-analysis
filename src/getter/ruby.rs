//! `Getter` implementation for Ruby.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for RubyCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Ruby as R;

        match node.kind_id().into() {
            R::Class | R::SingletonClass => SpaceKind::Class,
            R::Module => SpaceKind::Namespace,
            R::Method | R::SingletonMethod | R::Lambda | R::Block | R::DoBlock => {
                SpaceKind::Function
            }
            R::Program => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use Ruby as R;

        match node.kind_id().into() {
            // Control-flow keyword tokens. tree-sitter-ruby gives each
            // keyword its own anonymous numbered variant (e.g. `If2` is
            // the `if` keyword token; `If` is the named statement node).
            R::If2 | R::Unless2 | R::While2 | R::Until2 | R::For2 | R::In2 | R::Do2
            | R::Case2 | R::When2 | R::Elsif2 | R::Else2 | R::Then2
            | R::Begin2 | R::Ensure2 | R::Rescue2
            | R::Return3 | R::Yield3 | R::Break3 | R::Next3 | R::Redo2 | R::Retry2
            // Declaration keywords. `End`/`End2` are the two aliased
            // visible kinds for the `end` block closer (kind_ids 0 and
            // 13) that every `def`/`class`/`module`/`begin`/`if`/loop
            // construct emits; `BEGIN`/`END` are the special `BEGIN { }`
            // / `END { }` block-form keywords (kinds 4 / 7) and are
            // distinct from the lowercase `end` closer.
            | R::Def | R::End | R::End2 | R::Class2 | R::Module2
            | R::BEGIN | R::END
            | R::Undef2 | R::Alias2
            // Logical / definedness keywords
            | R::And | R::Or | R::Not | R::DefinedQMARK
            // Structural punctuation acting as operators. Only the
            // *opening* delimiter counts (the pair folds to one glyph in
            // `get_operator_id_as_str`); the former closing arms
            // (`RPAREN`/`RPAREN2`/`RBRACE`/`RBRACK`) double-counted every
            // balanced pair, inflating n1/N1 (#695). The `LBRACKRBRACK` /
            // `LBRACKRBRACKEQ` indexer *method names* below (`def [](i)`)
            // are whole-token operators, not a balanced pair, and stay.
            // `LPAREN2`/`LBRACK2`/`LBRACK3` are defensive — the runtime
            // `public_symbol_map` collapses them to `LPAREN`/`LBRACK` before
            // `kind_id()`, so they never fire (#768; see the Cpp impl note
            // and `second_alias_opener_collapses_to_base_kind_id`).
            | R::LPAREN | R::LPAREN2
            | R::LBRACE | R::LBRACK | R::LBRACK2 | R::LBRACK3
            | R::COMMA | R::SEMI | R::DOT | R::COLONCOLON | R::COLONCOLON2 | R::AMPDOT
            | R::COLON | R::COLON2 | R::HASHLBRACE | R::DASHGT
            // Method-name operator markers (`def +@`, `def -@`, `def ~@`)
            // and indexer methods.
            | R::PLUSAT | R::DASHAT | R::TILDEAT
            | R::LBRACKRBRACK | R::LBRACKRBRACKEQ
            // Arithmetic
            | R::PLUS | R::DASH | R::DASH2 | R::DASH3 | R::DASH4 | R::STAR | R::STAR2 | R::STAR3
            | R::SLASH | R::SLASH2 | R::PERCENT
            | R::STARSTAR | R::STARSTAR2 | R::STARSTAR3
            // Comparison
            | R::EQEQ | R::BANGEQ | R::EQEQEQ
            | R::LT | R::GT | R::LTEQ | R::GTEQ | R::LTEQGT
            | R::EQTILDE | R::BANGTILDE
            // Logical / unary
            | R::AMPAMP | R::PIPEPIPE | R::BANG | R::TILDE
            // Bitwise / shift
            | R::AMP | R::AMP2 | R::PIPE | R::CARET | R::LTLT | R::LTLT2 | R::GTGT
            // Assignment
            | R::EQ | R::EQ2
            | R::PLUSEQ | R::DASHEQ | R::STAREQ | R::SLASHEQ | R::PERCENTEQ
            | R::STARSTAREQ | R::AMPEQ | R::AMPAMPEQ | R::PIPEEQ | R::PIPEPIPEEQ
            | R::CARETEQ | R::LTLTEQ | R::GTGTEQ
            // Hash arrow, ternary, range
            | R::EQGT | R::QMARK | R::DOTDOT | R::DOTDOTDOT
            // Subshell backtick used as method-name marker (def `...)
            | R::BQUOTE
                => HalsteadType::Operator,

            // String-like literals contribute one operand each when inert.
            // If the literal carries an `Interpolation` child the inner
            // expressions are already walked and counted as operands; the
            // wrapping literal would otherwise double-count them
            // (same pattern as C# #183 / Elixir #180).
            R::String | R::ChainedString | R::BareString | R::Subshell
            | R::Regex | R::HeredocBody | R::StringArray | R::SymbolArray
            | R::DelimitedSymbol => {
                Self::string_operand_type(node, &[R::Interpolation as u16])
            }

            // Operands: identifiers and literals.
            R::Identifier | R::IdentifierSuffix | R::IdentifierSuffixToken1
            | R::Constant | R::ConstantSuffix | R::ConstantSuffixToken1
            | R::InstanceVariable | R::ClassVariable | R::GlobalVariable
            | R::Integer | R::Float | R::Complex | R::Rational
            | R::Character | R::SimpleSymbol | R::BareSymbol | R::HashKeySymbol
            // `Nil2` is the leaf `nil` keyword token; `Nil` (named) wraps
            // it. Counting both would double-count every `nil` literal —
            // only the wrapping named node contributes one operand.
            | R::True | R::False | R::Nil
            | R::Zelf | R::Super
            | R::Line | R::File | R::Encoding
                => HalsteadType::Operand,

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Ruby);
}
