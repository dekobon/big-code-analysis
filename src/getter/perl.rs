//! `Getter` implementation for Perl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for PerlCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Perl::FunctionDefinition
            | Perl::FunctionDefinitionWithoutSub
            | Perl::AnonymousFunction => SpaceKind::Function,
            Perl::SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use Perl as P;

        match node.kind_id().into() {
            // Regex delimiter punctuation. The bare match form is the
            // only one of Perl's five regex literals that spells its
            // delimiters with an operator token kind: `bca dump` at the
            // pinned grammar shows `/abc/` emitting two `SLASH` under
            // `PatternMatcher`, while `m//`, `qr//`, `s///` and `tr///`
            // use the dedicated `StartDelimiter` / `SeparatorDelimiter`
            // / `EndDelimiter` kinds that no arm here classifies. So
            // `$s =~ /abc/` fabricated two division operators (#1312,
            // the Perl sibling of Elixir #1256). Suppress them exactly
            // when their parent is the literal — the compound-leaf
            // guard of grammar-dispatch §5 — which leaves the bare form
            // contributing what its three synonyms already contribute
            // (#1314 made that "one operand" rather than "nothing";
            // see the pattern-value arm below).
            P::SLASH
                if ancestors.parent_has_kind(node, P::PatternMatcher as u16) =>
            {
                HalsteadType::Unknown
            }
            // Control-flow and declaration keywords. `Perl::Sub` is the
            // `sub` keyword (token id 16); `Perl::SUB` is the `__SUB__`
            // literal (token id 7) — that one is an operand, not an
            // operator. Same split for `Package` (keyword) vs `PACKAGE`
            // (`__PACKAGE__` literal).
            P::If | P::Unless | P::Else | P::Elsif | P::While | P::Until | P::For
            | P::Foreach | P::When | P::Continue | P::Next | P::Last | P::Redo | P::Goto
            | P::Return | P::Sub | P::Package | P::My | P::Our
            | P::Local | P::State | P::Use | P::No | P::Require | P::Bless | P::And | P::Or
            | P::Xor | P::Not | P::Eq | P::Ne | P::Lt | P::Gt | P::Le | P::Ge | P::Cmp
            // Punctuation acting as operators
            | P::SEMI | P::COMMA | P::COLON | P::COLONCOLON | P::LBRACE | P::LBRACK
            | P::LPAREN | P::DOT | P::DOTDOT | P::DOTDOTDOT | P::FatComma | P::DASHGT
            | P::QMARK | P::BSLASH | P::DOLLAR | P::DOLLARHASH | P::AT | P::PERCENT | P::HASH
            // Arithmetic / comparison / logical / bitwise / assignment operators
            | P::EQ | P::PLUS | P::DASH | P::STAR | P::SLASH | P::STARSTAR | P::BANG
            | P::TILDE | P::EQTILDE | P::BANGTILDE | P::EQEQ | P::BANGEQ | P::LT | P::GT
            | P::LTEQ | P::GTEQ | P::AMPAMP | P::PIPEPIPE | P::SLASHSLASH | P::PIPE
            | P::CARET | P::LTLT | P::GTGT | P::TILDETILDE | P::PLUSPLUS | P::DASHDASH
            | P::PLUSEQ | P::DASHEQ | P::STAREQ | P::SLASHEQ | P::PERCENTEQ | P::STARSTAREQ
            | P::AMPEQ | P::PIPEEQ | P::CARETEQ | P::LTLTEQ | P::GTGTEQ | P::AMPAMPEQ
            | P::PIPEPIPEEQ | P::SLASHSLASHEQ | P::DOTEQ | P::XEQ
            | P::LTEQGT | P::AMPDOTEQ | P::PIPEDOTEQ | P::CARETDOTEQ | P::Isa
            // Substitution and transliteration. These are *operations
            // applied to a target*, not pattern values, so they are
            // operators while the three pattern spellings below are
            // operands (#1314). `y///` is a synonym of `tr///` and
            // shares `TransliterationTrOrY`, so the two fold to one
            // entry, which is what synonyms should do.
            //
            // Their *literal* pattern and replacement text is
            // invisible either way: `bca dump` shows `s/a/b/` emitting
            // only the `s` keyword and its `start` / `separator` /
            // `end` delimiters, with no content node, so classifying
            // them as operators loses nothing that treating them as
            // values would have kept. An *interpolated* `s/$x/$y/` does
            // emit `Interpolation` children, and those count as
            // operands under either choice — so the reason for the
            // split is the semantic one above, not this. `get_operator_id_as_str` below renders them
            // `s///` and `tr///` rather than as raw kind names.
            | P::SubstitutionPatternS | P::TransliterationTrOrY
                => HalsteadType::Operator,
            // `package_name`, `package_variable` and `typeglob` each
            // spell a whole name — `Data::Dumper`, `$Foo::count`,
            // `*STDOUT` — and are operands in their own right below, so
            // an operand-classified node they *contain* is already
            // billed by the wrapper. Before #1355 the wrapper and its
            // contents both counted: `use strict;` scored N2 2 for one
            // name, `require Data::Dumper;` n2 3 / N2 3 for one, and
            // `our $Foo::count = 3;` n2 5 / N2 6 for two — the
            // vocabulary even growing a bare `::` entry, because a
            // `package_variable` spells its separator as a second,
            // childless `package_name` beside the qualifier.
            //
            // Keyed on the parent alone rather than on a list of child
            // kinds, because the invariant is positional: a wrapper's
            // span contains its children's, so whatever sits directly
            // inside one is already paid for. node-types.json admits ten
            // (wrapper, child) pairings across eight operand kinds, all
            // of them reachable — `package_name` ⊃ `typeglob` is
            // `*Foo::glob`, `package_name` ⊃ `package_variable` the
            // `$Foo::Bar::baz` nesting — and enumerating them here would
            // be a coverage claim to re-derive on every grammar bump for
            // no gain, the trade grammar-dispatch §1 recommends taking.
            // `perl_name_wrappers_bill_the_name_once_1355` witnesses all
            // ten and fails on an eleventh. Naming the three wrappers as
            // *parents* also resolves arbitrary qualifier depth to the
            // outermost, the way PHP handles `$$$x` (#1259).
            //
            // It sits below the operator arm on purpose: `::` inside a
            // `package_name`, and `*` and the opening `{` inside a
            // `typeglob`, are matched there and keep their operator
            // reading. (The closing `}` has never been classified —
            // `get_operator_id_as_str` folds the pair to one `{}`
            // glyph — so the guard changes nothing for it.)
            //
            // Guarded, not deleted, per grammar-dispatch §6: these
            // leaves are ordinary operands everywhere else (`my $x`,
            // `foo()`, the `bar` of `Foo::bar()`, which is a *sibling*
            // of the `package_name` rather than a child).
            //
            // Keeping the wrapper's whole span is the opposite call from
            // PHP's `qualified_name` (#1293) and Groovy's
            // `QualifiedName` (#1263 / #1352), which bill the leaves and
            // drop the wrapper. Three reasons it goes the other way in
            // Perl. `package_variable` and `typeglob` are sigil-bearing
            // *variable references*, the shape #1259 settled by keeping
            // the wrapper — the leaves-only reading detaches the sigil
            // from the variable and glues it to the qualifier, billing
            // `$Foo::Bar::baz` as `$Foo` + `Bar` + `baz`, three operands
            // for one variable. The three kinds nest into each other, so
            // they must be decided together; splitting `package_name`
            // off would need a grandparent-aware guard. And the
            // vocabulary-sharing argument of #1293 / #1352 — that
            // `java.util.List` and `List` name the same class — has no
            // Perl analogue, since a package is always spelled in full.
            //
            // `module_name` is not here, though #1355 expected it to
            // be: it is the quoted `use 'Foo.pm'` form, and it holds
            // nothing but its two quote tokens — not the `identifier`
            // the issue thought it shared. It stays an operand, pinned
            // by `perl_qualified_name_leaves_still_count_elsewhere_1355`.
            _ if matches!(
                ancestors.parent(node).map(|p| p.kind_id().into()),
                Some(P::PackageName | P::PackageVariable | P::Typeglob)
            ) =>
            {
                HalsteadType::Unknown
            }
            // Operands: identifiers and literals. Non-interpolating
            // string literals (`'…'`, `q{…}`) are leaf operands; the
            // interpolating kinds (`"…"`, `qq{…}`, `` `…` ``, `qx{…}`)
            // are handled separately below so their inner
            // scalar/array/hash variables are not double-counted.
            P::Identifier | P::ScalarVariable | P::ArrayVariable | P::HashVariable
            | P::PackageVariable | P::SpecialScalarVariable | P::PackageName | P::ModuleName
            | P::BarewordImport | P::Typeglob | P::FileHandle
            | P::Integer | P::FloatingPoint | P::ScientificNotation | P::Hexadecimal | P::Octal
            | P::True | P::False | P::SpecialLiteral
            | P::StringSingleQuoted | P::StringQQuoted
            | P::FILE | P::LINE | P::SUB | P::PACKAGE
                => HalsteadType::Operand,
            // Perl's interpolating string-like literals count as one
            // operand when inert. When they carry an `Interpolation`
            // child the inner scalar / array / hash variables are
            // already walked and classified as operands; counting the
            // wrapping literal too would double-count the inner
            // variables' contribution to `N2` (issue #199, same
            // pattern as #180 for Elixir/Bash, #183 for C#, #184 for
            // PHP, #191 for Kotlin). `HeredocBodyStatement` is the
            // visible body of `<<TAG ... TAG` heredocs (issue #287);
            // it is interpolation-capable, so it joins the same
            // dispatch — inert heredocs are one operand, interpolating
            // heredocs let the inner variables carry the count.
            // The pattern *values* — `/abc/`, `m/abc/`, `qr/abc/` —
            // join the same dispatch (#1314). `m/abc/` is exactly
            // `/abc/` and `qr/abc/` is the same pattern as a value, so
            // all three must score alike; that equality is what
            // `perl_every_pattern_value_spelling_scores_alike` protects
            // and why #1312 declined to promote the bare form on its
            // own. Promoting the three together closes the gap without
            // reintroducing the spelling sensitivity: each contributes
            // one operand, matching Ruby's `Regex` and Elixir's
            // `Sigil`. `s///` and `tr///` are *not* here — they are
            // operations applied to a target and sit in the operator
            // arm above.
            //
            // Sharing the string dispatch is not incidental, which is
            // why the arms are merged rather than duplicated: the bare
            // form keeps an interpolated variable inside an
            // unclassified `regex_pattern_content`, but `m/$x/` and
            // `qr/$x/` emit a real `Interpolation` whose
            // `scalar_variable` is already counted. A plain operand arm
            // would score `/$x/` at one operand and `m/$x/` at two —
            // the very divergence the equality above exists to prevent.
            P::StringDoubleQuoted | P::StringQqQuoted | P::BacktickQuoted
            | P::CommandQxQuoted | P::HeredocBodyStatement
            | P::PatternMatcher | P::PatternMatcherM | P::RegexPatternQr => {
                Self::string_operand_type(node, &[P::Interpolation as u16])
            }
            _ => HalsteadType::Unknown,
        }
    }

    /// Folds paired delimiters to one glyph, and names the two
    /// pattern *operations* after their source spelling.
    ///
    /// Hand-written rather than `get_operator!(Perl)` because
    /// `SubstitutionPatternS` and `TransliterationTrOrY` are named
    /// nodes rather than punctuation tokens (#1314): the macro's
    /// fallback renders a kind's own name, so `bca ops` would list
    /// `substitution_pattern_s` and `transliteration_tr_or_y` among
    /// the operators, which reads as a bug rather than as `s///`.
    /// `tr///` covers `y///` too — one kind, one glyph, because they
    /// are synonyms.
    #[inline]
    fn get_operator_id_as_str(id: u16) -> &'static str {
        let typ = id.into();
        match typ {
            Perl::LPAREN => "()",
            Perl::LBRACK => "[]",
            Perl::LBRACE => "{}",
            Perl::SubstitutionPatternS => "s///",
            Perl::TransliterationTrOrY => "tr///",
            _ => typ.into(),
        }
    }
}
