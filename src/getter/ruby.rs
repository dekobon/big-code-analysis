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

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use Ruby as R;

        match node.kind_id().into() {
            // Regex delimiter punctuation. tree-sitter-ruby aliases
            // every regex delimiter spelling to `/`: `bca dump` at the
            // pinned grammar shows `/…/`, `%r{…}` and `%r|…|` all
            // emitting `SLASH` (kind 85) for both delimiters — the very
            // kind id real division uses. Counting them fabricated two
            // `/` operators per regex literal and made n1/N1 depend on
            // the author's delimiter choice (#1312, the Ruby sibling of
            // Elixir #1256). The `Regex` node itself is the operand
            // (below), so the delimiters are suppressed exactly when
            // their parent is that node — the compound-leaf guard of
            // grammar-dispatch §5. Parent, not ancestor: a `/` nested
            // deeper, such as a division inside `#{…}` interpolation,
            // has `Binary` as its parent and must still count.
            // `SLASH2` is the aliased regex-start token; the runtime
            // `public_symbol_map` collapses it to `SLASH` so it never
            // reaches `kind_id()` (the `LPAREN2` class of #768, pinned
            // by `ruby_regex_start_alias_never_reaches_kind_id`). It
            // appears here and *only* here — it was dropped from the
            // arithmetic arm below, because a regex delimiter is the
            // only thing it could ever be. Should a bump surface it
            // outside a `Regex`, it now falls to `Unknown` rather than
            // being counted as a division.
            R::SLASH | R::SLASH2
                if ancestors.parent_has_kind(node, R::Regex as u16) =>
            {
                HalsteadType::Unknown
            }
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
            // Structural punctuation acting as operators. `HASHLBRACE`
            // — the `#{` interpolation opener — was here until #1314
            // and is deliberately gone: an interpolation opener is
            // spelling rather than an operation, the interpolated
            // expression's own operators being counted already. Unlike
            // PHP's `{`, Ruby's is a token of its own, so it needs no
            // parent guard; dropping the arm is the whole change, and
            // it applies to every literal that interpolates — string,
            // symbol, regex, heredoc, subshell. See the PHP sibling in
            // `src/getter/php.rs` for the cross-language measurement
            // this policy rests on. This half is a behaviour change,
            // not a fabrication fix: Ruby Halstead operator counts drop
            // for interpolated literals.
            //
            // Only the *opening* delimiter counts (the pair folds to
            // one glyph in `get_operator_id_as_str`); the former closing arms
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
            | R::COLON | R::COLON2 | R::DASHGT
            // Method-name operator markers (`def +@`, `def -@`, `def ~@`)
            // and indexer methods.
            | R::PLUSAT | R::DASHAT | R::TILDEAT
            | R::LBRACKRBRACK | R::LBRACKRBRACKEQ
            // Arithmetic
            | R::PLUS | R::DASH | R::DASH2 | R::DASH3 | R::DASH4 | R::STAR | R::STAR2 | R::STAR3
            | R::SLASH | R::PERCENT
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

            // String-like literals contribute one operand each when
            // inert. The wrapper is suppressed exactly when it holds a
            // child the walk already counts, so its contribution is not
            // billed twice — the shared spelling of C# #183 / Elixir
            // #180 (`Interpolation`) and of #1353 (the element kinds).
            //
            // `String`, `BareString`, `BareSymbol`, `Subshell`, `Regex`,
            // `HeredocBody` and `DelimitedSymbol` admit only
            // `string_content` / `escape_sequence` / `interpolation` as
            // named children (`heredoc_body` also `heredoc_content` /
            // `heredoc_end`), of which `Interpolation` alone is
            // classified elsewhere; the delimiter tokens they also hold
            // are anonymous and no arm names them.
            //
            // The three *element containers* hold classified operands
            // instead, so before #1353 the guard never fired for them
            // and the wrapper was billed alongside every element:
            // `b = %w[x y]` scored n2 4 / N2 4 for three operands, the
            // spare entry being the wrapper's whole-span text. That made
            // the vocabulary grow with how the author grouped the
            // literals — the spelling sensitivity #695 removed for
            // Python's implicit concatenation and #1312 / #1314 for
            // delimiter tokens.
            //
            //   `chained_string` ⊃ `string`      (`"a" "b"`)
            //   `string_array`   ⊃ `bare_string` (`%w[…]`, `%W[…]`)
            //   `symbol_array`   ⊃ `bare_symbol` (`%i[…]`, `%I[…]`)
            //
            // Gated rather than deleted, per grammar-dispatch §6:
            // node-types.json marks the two arrays' children optional
            // and `%w[]` / `%i[]` really do parse to a wrapper holding
            // nothing but its delimiter tokens, so dropping the kinds
            // would score an empty literal zero. (`chained_string` is
            // `seq($.string, repeat1($.string))` and so carries at
            // least two elements; the guard therefore always fires and
            // its membership here is unobservable — listed for symmetry
            // with its siblings rather than out of need. See
            // `ruby_element_containers_count_elements_not_the_composite_1353`,
            // which says so rather than pretending to pin it.)
            //
            // One list serves both groups because no kind here can hold
            // an element kind it is not paired with above, and none of
            // the containers admits an `Interpolation` as a *direct*
            // child — `%W[a#{n}]` nests it inside the `bare_string`,
            // which is guarded by this same arm one level down.
            //
            // `BareSymbol` reached this arm in #1353; until then it sat
            // unguarded in the plain operand arm below, so `%I[a#{n}b]`
            // billed the element `a#{n}b` *and* the `n` inside it while
            // the `%W[…]` spelling of the same thing billed only `n`.
            R::String | R::ChainedString | R::BareString | R::BareSymbol
            | R::Subshell | R::Regex | R::HeredocBody | R::DelimitedSymbol
            | R::StringArray | R::SymbolArray => Self::string_operand_type(
                node,
                &[
                    R::Interpolation as u16,
                    R::String as u16,
                    R::BareString as u16,
                    R::BareSymbol as u16,
                ],
            ),

            // Operands: identifiers and literals.
            R::Identifier | R::IdentifierSuffix | R::IdentifierSuffixToken1
            | R::Constant | R::ConstantSuffix | R::ConstantSuffixToken1
            | R::InstanceVariable | R::ClassVariable | R::GlobalVariable
            | R::Integer | R::Float | R::Complex | R::Rational
            | R::Character | R::SimpleSymbol | R::HashKeySymbol
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
