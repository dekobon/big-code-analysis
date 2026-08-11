//! `Getter` implementation for Tcl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

impl Getter for TclCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        match node.kind_id().into() {
            Tcl::Procedure => SpaceKind::Function,
            Tcl::SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> HalsteadType {
        match node.kind_id().into() {
            // Braced-word delimiter punctuation. A braced *word* — a
            // literal value such as `set a {braced word}` — carries its
            // `{` as an `LBRACE` child, the kind id a real block uses,
            // so the value reported a `{}` operator with no block in
            // the source (#1314, the Tcl sibling of Elixir #1256 and
            // Ruby/Perl #1312). `BracedWordSimple` is already an
            // operand (below), so the delimiter is suppressed exactly
            // when its parent is that node — the compound-leaf guard of
            // grammar-dispatch section 5.
            //
            // A *script* body is a `BracedWord` (88) — verified across
            // `proc`, `if`, `catch`, `eval`, `after` and `uplevel` — an
            // `if`/`while` condition is an `Expr` (97), and a `proc`
            // parameter list is `Arguments` (94). All three keep their
            // braces as operators, which is what makes keying the guard
            // on `BracedWordSimple` safe.
            //
            // Parent, not ancestor, and here the distinction is
            // *observable*: the grammar parses a `[…]` command
            // substitution inside a braced word, so
            // `set z {x [if {$q} {puts w}] v}` nests an `Expr` and a
            // `BracedWord` — each with its own `{` — under a
            // `BracedWordSimple` ancestor. An ancestor scan swallows
            // both. Pinned by
            // `tcl_braced_word_guard_is_parent_scoped_not_ancestor_scoped`.
            //
            // What this arm does *not* reach: the grammar emits
            // `BracedWordSimple` only in the value slot of the commands
            // it special-cases, so `lappend x {a b}` parses its literal
            // as a `BracedWord` script and still fabricates a `{}`.
            // Guarding that kind too would drop every real block, so
            // closing it needs command-name recognition
            // (grammar-dispatch §9) rather than another kind arm —
            // FIXME(#1318). iRules carries the twin.
            Tcl::LBRACE
                if ancestors.parent_has_kind(node, Tcl::BracedWordSimple as u16) =>
            {
                HalsteadType::Unknown
            }
            // Anonymous keyword tokens (control-flow and declaration keywords).
            Tcl::Proc
            | Tcl::If2
            | Tcl::Elseif2
            | Tcl::Else2
            | Tcl::While2
            | Tcl::Foreach2
            | Tcl::Set2
            | Tcl::Global2
            | Tcl::Namespace2
            | Tcl::Try2
            | Tcl::Catch2
            | Tcl::Finally2
            | Tcl::Regexp2
            | Tcl::Expr2
            // String comparison operators.
            | Tcl::Eq
            | Tcl::Ne
            | Tcl::In
            | Tcl::Ni
            // Structural punctuation. Only the *opening* delimiter is an
            // operator (the pair folds to one glyph in
            // `get_operator_id_as_str`); counting the matching closer too
            // (former `RBRACE`/`RBRACK`/`RPAREN` arms) double-counted every
            // balanced pair, inflating n1/N1 (#695).
            // `LPAREN2` is defensive — the runtime collapses it to `LPAREN`
            // before `kind_id()`, so it never fires (#768; see the Cpp note).
            | Tcl::LBRACE
            | Tcl::LBRACK
            | Tcl::LPAREN
            | Tcl::LPAREN2
            | Tcl::SEMI
            | Tcl::COLON
            | Tcl::COLONCOLON
            | Tcl::COLONCOLON2
            // Arithmetic / exponent operators.
            | Tcl::PLUS
            | Tcl::DASH
            | Tcl::STAR
            | Tcl::SLASH
            | Tcl::PERCENT
            | Tcl::STARSTAR
            // Bitwise operators.
            | Tcl::AMP
            | Tcl::PIPE
            | Tcl::CARET
            | Tcl::TILDE
            | Tcl::LTLT
            | Tcl::GTGT
            // Comparison operators.
            | Tcl::EQEQ
            | Tcl::BANGEQ
            | Tcl::LT
            | Tcl::GT
            | Tcl::LTEQ
            | Tcl::GTEQ
            // Logical operators.
            | Tcl::BANG
            | Tcl::AMPAMP
            | Tcl::PIPEPIPE
            // Ternary conditional operator.
            | Tcl::QMARK => HalsteadType::Operator,

            // The anonymous `id` token (`Id2`) is the kind the parser
            // actually emits, in *both* of its positions: as a standalone
            // `set` target (`set s …` → `s`, a real operand) and as the
            // inner leaf of every `variable_substitution` (`$s` →
            // `(variable_substitution (id "s"))`), whose wrapper is already
            // the operand for the reference. A blanket `Id2` exclusion
            // therefore dropped every `set` target from n2/N2 (#1294); the
            // correct guard is a parent check, the same shape as iRules'
            // `Id` arm. The named `Id` never surfaces at the pinned grammar
            // (drift marker: `tcl_named_id_variant_is_unreachable`) and is
            // listed defensively so a grammar bump that starts emitting it
            // classifies it identically.
            Tcl::Id | Tcl::Id2 => {
                if ancestors.parent_has_kind(node, Tcl::VariableSubstitution as u16) {
                    HalsteadType::Unknown
                } else {
                    HalsteadType::Operand
                }
            }

            // Operands: identifiers and literals.
            Tcl::SimpleWord
            | Tcl::Number
            | Tcl::BracedWord
            | Tcl::BracedWordSimple
            | Tcl::VariableSubstitution => HalsteadType::Operand,

            // Double-quoted strings count as a single operand when inert
            // (`"hello world"`). When they carry a `$var` or `[cmd]`
            // interpolation child, the inner `variable_substitution` /
            // `command_substitution` nodes are walked separately and
            // contribute their own operands; counting the wrapping
            // `QuotedWord` too would double-count `N2` (issue #277, same
            // pattern as #180/#183/#184 for Bash/C#/PHP).
            Tcl::QuotedWord => Self::string_operand_type(
                node,
                &[
                    Tcl::VariableSubstitution as u16,
                    Tcl::CommandSubstitution as u16,
                ],
            ),

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Tcl);
}
