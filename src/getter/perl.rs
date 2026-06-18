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

    fn get_op_type(node: &Node) -> HalsteadType {
        use Perl as P;

        match node.kind_id().into() {
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
                => HalsteadType::Operator,
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
            P::StringDoubleQuoted | P::StringQqQuoted | P::BacktickQuoted
            | P::CommandQxQuoted | P::HeredocBodyStatement => {
                Self::string_operand_type(node, &[P::Interpolation as u16])
            }
            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Perl);
}
