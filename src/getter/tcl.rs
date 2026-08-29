//! `Getter` implementation for Tcl.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

/// The braced-word kinds `Getter::is_subsumed_braced_word` is
/// instantiated with (#1354): the literal *value* form the guard
/// keys on, and the *script* form it gates on child-presence. Named
/// here so the guard reads as one line inside an already long match.
const BRACED_VALUE: u16 = Tcl::BracedWordSimple as u16;
const BRACED_SCRIPT: u16 = Tcl::BracedWord as u16;

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
            // The braced-word rule (#1354 / #1317), whose two halves
            // `Getter::is_subsumed_braced_word` states once for both
            // dialects. This arm is only its Tcl instantiation, plus
            // the derivation and the caveats.
            //
            // A braced *word* is one literal value: Tcl evaluates
            // nothing between the braces, so `{$x}` is the
            // two-character string `$x` and not a variable reference.
            // The grammar models the interior structurally all the
            // same — `braced_word_simple` (89) admits `simple_word`,
            // `escaped_character`, `quoted_word`,
            // `variable_substitution`, `command_substitution` and a
            // nested `braced_word_simple` — so the word and every part
            // of it were billed together: `set x {literal here}` scored
            // n2 4 / N2 4 for one value, and `set a {braced word}`
            // reported four operands where its synonym
            // `set a "quoted word"` reported two.
            //
            // The wrapper keeps the operand and its children are
            // suppressed — the opposite polarity from `QuotedWord`
            // below, and for the reason that decides it: a quoted word
            // *does* interpolate, so its `$var` children are real
            // references the walk has to count, while a braced word's
            // are letters. Billing the whole word is also what keeps
            // the two spellings of one literal scoring alike, the
            // spelling insensitivity #695, #1312 and #1314 each
            // restored elsewhere — and it is the contract the book
            // already documents: "a Tcl braced word … contributes *one
            // operand* — the literal" (`metrics.md`, Halstead).
            //
            // The value half is keyed on the parent alone rather than
            // on a list of child kinds (the #1355 shape): the invariant
            // is positional — a wrapper's span already contains its
            // children's — and an enumerated list would be a coverage
            // claim to re-derive on every grammar bump.
            // `tcl_braced_word_bills_its_content_once_1354` witnesses all six
            // child kinds and fails on a seventh.
            //
            // It subsumes #1314's narrower `LBRACE` guard: that `{` is
            // one more child of the word, and reporting it as a `{}`
            // operator fabricated a block the source does not contain.
            // A *script* is a `BracedWord` (88) — verified across
            // `proc`, `if`, `catch`, `eval`, `after` and `uplevel` — an
            // `if`/`while` condition is an `Expr` (97), and a `proc`
            // parameter list is `Arguments` (94). All three keep their
            // braces as operators, which is what makes keying the value
            // half on `BracedWordSimple` safe.
            //
            // Two limits of the value half, both deliberate and both
            // pinned rather than left implied.
            //
            // It is *parent*-scoped, so "Tcl evaluates nothing between
            // the braces" holds for what sits directly inside the word
            // and not for what sits deeper: the grammar parses a `[…]`
            // command substitution and a `"…"` quoted word inside a
            // braced word, and the operands *they* contain still count
            // (`set a {x "$q" v}` bills `$q`). Widening to an ancestor
            // scan would also swallow the `{` of every `Expr` and
            // `BracedWord` nested under a value word, which is the
            // fabrication #1314 removed in the other direction —
            // `tcl_braced_word_guard_is_parent_scoped_not_ancestor_scoped`
            // pins the operators, `tcl_braced_word_bills_its_content_once_1354`
            // the surviving operands. The residual is the same one
            // #1314 recorded: the grammar models a substitution Tcl
            // will not perform, and what the classifier sees is what
            // the metric reports.
            //
            // And the grammar emits `BracedWordSimple` only in the
            // value slot of the commands it special-cases, so
            // `lappend x {a b}` parses its literal as a `BracedWord`
            // script and still fabricates a `{}`. Guarding that kind
            // too would drop every real block, so closing it needs
            // command-name recognition (grammar-dispatch §9) rather
            // than another kind arm — FIXME(#1318). iRules carries the
            // twin of all of this.
            //
            // Cross-walked against the sibling predicates
            // (grammar-dispatch §7) and left as it was:
            // `Checker::is_string` and `Alterator::alterate` both list
            // `BracedWord` beside the two literal forms, so
            // `bca find -f string` reports a `proc` body as a string
            // literal while this arm gives it no operand. That
            // disagreement predates #1354 in shape — an interpolating
            // `QuotedWord` is already `Unknown` here and a string
            // there — and calling a script body a string is #1318's
            // role conflation rather than this arm's, so changing
            // `find`'s answer is left to that issue.
            _ if Self::is_subsumed_braced_word(node, ancestors, BRACED_VALUE, BRACED_SCRIPT) => {
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
            //
            // `BracedWord` reaches this arm only when the guard above
            // let it through, which since #1354 means only when it
            // holds no named child. A script kind holding commands —
            // a `proc` body, an `if` body, a `catch` or `eval` script —
            // was billed as one operand spanning its entire text
            // *beside* the commands the walk descends into, which made
            // `n2` grow with the size and uniqueness of the code rather
            // than with its vocabulary; nested bodies compounded it,
            // one vocabulary entry per block, each containing the next.
            //
            // Gated rather than deleted (grammar-dispatch §6): the same
            // kind is the value slot of every command the grammar does
            // not special-case, so `lappend l {}` is an empty list
            // whose brace pair is its only carrier, and deleting the
            // kind scored it zero where its `lappend l ""` synonym
            // scores one. An empty `proc` body is spelled identically
            // and so also counts one — accepted rather than argued
            // away, since no kind-scoped arm can separate the two
            // roles (the same conflation FIXME(#1318) tracks on the
            // operator side).
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
