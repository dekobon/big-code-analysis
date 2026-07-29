//! `Getter` implementation for Elixir.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

// Extracts the human-readable head name from the first non-`target`
// child of an Elixir `def` / `defp` / `defmacro` / `defmacrop` /
// `defmodule` Call. Handles three shapes:
//   - `Arguments` wrapper: descend one level and recurse.
//   - `Identifier` / `Alias` leaf: return its source text.
//   - inner `Call` (e.g. `def foo(x, y)`): return the target identifier
//     text of that inner Call.
// Returns `None` when the child does not match any of these shapes,
// allowing the caller to keep scanning siblings (notably the
// `do_block`, which is unconditionally present and never carries the
// name).
fn elixir_extract_head_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
    use Elixir as E;

    let text = |n: &Node| node_text(code, n);
    match node.kind_id().into() {
        E::Identifier | E::Alias => text(node),
        E::Call => text(&node.child_by_field_name("target")?),
        E::Arguments
        | E::Arguments2
        | E::Arguments3
        | E::Arguments4
        | E::Arguments5
        | E::CallArgumentsWithTrailingSeparator => node
            .children()
            .find_map(|child| elixir_extract_head_name(&child, code)),
        _ => None,
    }
}

impl Getter for ElixirCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Elixir as E;

        match node.kind_id().into() {
            E::AnonymousFunction => SpaceKind::Function,
            E::Source => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    // Source-aware classifier (#275). Elixir's `defmodule` /
    // `def` / `defp` / `defmacro` / `defmacrop` are not distinct
    // grammar productions — they all parse as `Call` nodes whose
    // `target` Identifier text spells the keyword. The walker promotes
    // these Calls to func spaces via `Checker::is_func_space_with_code`;
    // this method labels the promoted space with the right `SpaceKind`
    // so `Wmc` / `Npm` / `Npa` see a Class for `defmodule` and a
    // Function for the method-defining macros.
    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_class_macro, elixir_is_inside_quote_block,
            elixir_is_method_macro,
        };
        let kind = Self::get_space_kind(node);
        if kind != SpaceKind::Unknown {
            return kind;
        }
        match elixir_call_keyword(node, code) {
            Some(kw) if elixir_is_class_macro(kw) => SpaceKind::Class,
            // Method-defining macros nested inside a `quote do … end`
            // template are not real method declarations (#310).
            Some(kw)
                if elixir_is_method_macro(kw)
                    && !elixir_is_inside_quote_block(node, code, ancestors) =>
            {
                SpaceKind::Function
            }
            _ => SpaceKind::Unknown,
        }
    }

    // Source-aware name extraction for the macro-shaped declarations.
    // `def foo(x, y) do … end` parses (with the tree-sitter-elixir
    // grammar shipped here) as
    //   `Call { target: Identifier "def", Arguments { Call { target:
    //     Identifier "foo", Arguments { … } } }, DoBlock { … } }`
    // i.e. the head Call is wrapped in an `Arguments` node, not a
    // direct child. `defmodule Foo.Bar do … end` parses similarly with
    // the `Alias` inside `Arguments`. We descend through one
    // `Arguments` layer when present, then either:
    //   - return the `Identifier` / `Alias` text directly
    //     (`defmodule Foo`, `def foo` for an arity-zero head with no
    //     parentheses), or
    //   - return the inner head Call's `target` text
    //     (`def foo(x, y)`).
    // Falls back to the trait default behaviour (`<anonymous>` for
    // nodes without a `name` field) when the Call is not one we
    // recognise.
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        use Elixir as E;

        use crate::metrics::cognitive::{
            elixir_call_keyword, elixir_is_class_macro, elixir_is_inside_quote_block,
            elixir_is_method_macro,
        };
        // The Class kind always names its head; for method macros we
        // additionally require the Call NOT to be inside a `quote`
        // template, matching the func-space promotion rule (#310).
        //
        // The quote-block lookup reads the caller's chain rather than
        // climbing with `Node::parent` (#1088). This fires once per
        // *promoted* space rather than per node, so the win is smaller
        // than at the walk's per-node sites — but `bca function` and the
        // suppression scan reach it through `get_func_name` on every
        // function node, where the climb was `O(depth)` apiece.
        if node.kind_id() == E::Call as u16
            && let Some(kw) = elixir_call_keyword(node, code)
            && (elixir_is_class_macro(kw)
                || (elixir_is_method_macro(kw)
                    && !elixir_is_inside_quote_block(node, code, ancestors)))
        {
            let target_id = node.child_by_field_name("target").map(|t| t.id());
            if let Some(name) = node
                .children()
                .filter(|child| Some(child.id()) != target_id)
                .find_map(|child| elixir_extract_head_name(&child, code))
            {
                return Some(name);
            }
        }

        if let Some(name) = node.child_by_field_name("name") {
            return node_text(code, &name);
        }
        Some("<anonymous>")
    }

    fn get_op_type<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use Elixir as E;

        match node.kind_id().into() {
            // Reserved-word keywords that have dedicated token kinds in
            // the grammar — block delimiters, exception clauses, the
            // `fn` keyword, and word-form logical / membership operators.
            // (Macro-shaped keywords like `def`/`defp`/`if`/`case`/`cond`
            // are NOT here: they surface as `Identifier` tokens in a
            // `Call`'s `target` field and are counted as operands below.)
            E::Do | E::End | E::End2 | E::Else | E::After | E::Catch | E::Rescue | E::Fn
            | E::When | E::Not | E::Or | E::And | E::In | E::Notin
            // Structural punctuation acting as operators. Only the
            // *opening* delimiter counts (the pair folds to one glyph in
            // `get_operator_id_as_str`); the former `RPAREN`/`RBRACE`/
            // `RBRACK` arms double-counted every balanced pair, inflating
            // n1/N1 (#695). `LTLT`/`GTGT` are the bitstring `<<`/`>>`
            // delimiters — left unfolded and counted as the majority of
            // languages count their shift-like glyphs. `LPAREN2`/`LBRACK2`
            // are defensive — the runtime `public_symbol_map` collapses them
            // to `LPAREN`/`LBRACK` before `kind_id()`, so they never fire
            // (#768; see the Cpp impl note).
            | E::LPAREN | E::LPAREN2 | E::LBRACE
            | E::LBRACK | E::LBRACK2 | E::LTLT | E::GTGT
            | E::COMMA | E::SEMI | E::COLON | E::COLONCOLON | E::DOT
            | E::DOTDOT | E::DOTDOTDOT | E::PERCENT | E::HASHLBRACE | E::AT
            // Arithmetic / unary
            | E::PLUS | E::DASH | E::STAR | E::STARSTAR | E::SLASH
            // Comparison
            | E::EQEQ | E::EQEQEQ | E::BANGEQ | E::BANGEQEQ
            | E::LT | E::GT | E::LTEQ | E::GTEQ
            // Logical
            | E::AMPAMP | E::PIPEPIPE | E::BANG
            // Bitwise / Erlang-band
            | E::AMP | E::PIPE | E::CARET | E::TILDE
            | E::AMPAMPAMP | E::PIPEPIPEPIPE | E::CARETCARETCARET | E::TILDETILDETILDE
            | E::LTLTLT | E::GTGTGT
            // Assignment / match
            | E::EQ
            // Concat / list operations
            | E::PLUSPLUS | E::DASHDASH | E::LTGT
            | E::PLUSPLUSPLUS | E::DASHDASHDASH
            // Pipe / capture / generator / stab arrow
            | E::PIPEGT | E::LTPIPEGT | E::DASHGT | E::LTDASH
            // Map pair / default arg / regex match / range step
            | E::EQGT | E::BSLASHBSLASH | E::EQTILDE | E::SLASHSLASH
            // Custom / less common Elixir operators
            | E::LTTILDE | E::TILDEGT | E::LTTILDEGT | E::LTLTTILDE | E::TILDEGTGT
                => HalsteadType::Operator,

            // String literals contribute exactly one operand each when
            // they are inert. When they carry an `interpolation` child,
            // the interpolated expressions are already walked and counted
            // as operands in their own right; counting the wrapping
            // literal as well would double-count the inner identifiers'
            // contribution (issue #180). The interpolation markers
            // `#{` / `}` are classified as operators via `HASHLBRACE` /
            // `RBRACE`, so an interpolated literal still adds operator
            // weight without inflating `N2`.
            E::String | E::Charlist | E::Sigil => {
                Self::string_operand_type(node, &[E::Interpolation as u16])
            }

            // Operands: identifiers and literals. Sigil names/modifiers
            // (`~r`, the trailing `i`/`u` flags) stay as operands even
            // for interpolated sigils — they are distinct tokens with
            // their own text.
            E::Identifier | E::Alias | E::OperatorIdentifier
            | E::SigilName | E::SigilName2 | E::SigilModifiers
            | E::Keyword | E::Keyword2 | E::QuotedKeyword
            | E::Integer | E::Float | E::Char
            | E::Atom | E::Atom2 | E::QuotedAtom
            | E::Boolean | E::True | E::False
            | E::Nil | E::Nil2
                => HalsteadType::Operand,

            _ => HalsteadType::Unknown,
        }
    }

    get_operator!(Elixir);
}
