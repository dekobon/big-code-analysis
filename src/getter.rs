// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use crate::metrics::halstead::HalsteadType;

use crate::spaces::SpaceKind;
use crate::traits::Search;

use crate::*;

/// Bounds- and UTF-8-checked text extraction for a node's byte span.
///
/// `code` is `&[u8]`, so slicing it carries no char-boundary
/// precondition; the two guards here cover two unrelated failure modes:
///
/// * `std::str::from_utf8` rejects non-UTF-8 bytes. This one is
///   reachable from ordinary use — [`crate::Ast::parse`] accepts
///   arbitrary bytes, so a node span in a partially-binary source need
///   not be valid UTF-8.
/// * `code.get` bounds-checks the range. This one is reachable only by
///   violating the same-parse precondition documented on `Getter`, i.e.
///   [`crate::Ast::from_tree_sitter`] adopting a tree built from longer
///   source than the `code` passed alongside it.
///
/// Both degrade to `None`, which `get_func_space_name` already returns
/// for a nameless node, so the walker records an unnamed space instead
/// of crashing. The unguarded sibling slice sites return infallible
/// types feeding metric arithmetic or rendered output; there the only
/// available fallback would be a fabricated empty value that silently
/// corrupts a count, so they rely on the precondition instead. The
/// asymmetry is deliberate: guarding is free here because `Option` is
/// already part of this signature's contract (#1059).
#[inline]
fn node_text<'a>(code: &'a [u8], node: &Node) -> Option<&'a str> {
    code.get(node.start_byte()..node.end_byte())
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
}

macro_rules! get_operator {
    ($language:ident) => {
        #[inline]
        fn get_operator_id_as_str(id: u16) -> &'static str {
            let typ = id.into();
            match typ {
                $language::LPAREN => "()",
                $language::LBRACK => "[]",
                $language::LBRACE => "{}",
                _ => typ.into(),
            }
        }
    };
}

// Emit a `Getter::get_op_type` body for a JS-family language. The four
// JS-family grammars (JavaScript, MozJS, TypeScript, TSX) share most of
// their Halstead operator/operand kind classifications; per-language
// deltas are passed as bracketed extras so all four impls stay in
// lockstep when a kind is added or removed (issue #299).
//
// `$op_extras` per language:
//   * JavaScript / MozJS: `OptionalChain` — the bare `?.` token (these
//     grammars expose no `optional_chain` wrapper).
//   * TypeScript / TSX:   `QMARKDOT`, `PredefinedType` — `QMARKDOT` is
//     the bare `?.` token under the `optional_chain` wrapper (issue
//     #281); `PredefinedType` is the TS type keyword set (`string`,
//     `number`, `boolean`, …).
//
// `$operand_extras` per language:
//   * JavaScript / MozJS: `Identifier2`, `String2` — anonymous keyword
//     aliases the JS grammar exposes for `Identifier` and `String`.
//   * TypeScript: `String2`, `NestedIdentifier`, `MemberExpression4`.
//     `String2` is the TS-only anonymous `"string"` alias the grammar
//     emits for the `string` type-annotation keyword (kind_id 135, in
//     the type-keyword range alongside `Boolean` / `Symbol`); it must
//     be in `operand_extras` to agree with `Checker::is_string` which
//     also matches it (issue #313, parallel to #283).
//     `NestedIdentifier` and `MemberExpression4` are other TS-only
//     productions.
//   * TSX: union of the above plus `String3`. TSX uniquely exposes
//     *two* anonymous `"string"` aliases: `String2` (kind_id 261, the
//     string-literal alias) and `String3` (kind_id 141, the
//     type-annotation keyword — the role TS's `String2` plays).
//     `Checker::is_string` matches both, so both must be operands
//     (#313).
//
// The `TemplateString` interpolation guard is shared verbatim (issue
// #192): a bare `` `...` `` mirrors a `"..."` operand, but an
// interpolated template must yield `Unknown` because its inner
// `TemplateSubstitution` expressions are walked separately.
macro_rules! impl_js_family_get_op_type {
    (
        $lang:ident,
        op_extras: [$($op_extra:ident),* $(,)?],
        operand_extras: [$($operand_extra:ident),* $(,)?]
        $(, predefined_void: $predefined_type:ident)? $(,)?
    ) => {
        fn get_op_type(node: &Node) -> HalsteadType {
            use $lang::*;

            // TS/TSX only: a `void` return / parameter type is parsed as a
            // `predefined_type` wrapper around an inner `void` token. Both
            // the wrapper (routed through `is_primitive` into the text-keyed
            // `primitive_operators` map as `"void"`) and the inner `Void`
            // token (a standalone expression operator, e.g. `void 0`) would
            // otherwise classify as operators, double-counting one source
            // `void` as two Halstead operators (issue #453). Other predefined
            // types (`: string`, `: number`, …) do not have an operator-kind
            // child, so only `void` collides. Suppress the wrapper here and
            // let the inner `Void` token carry the single operator, keeping
            // the kind_id-keyed count consistent with expression `void 0`
            // (the lesson-4 `n1 == dedupe(ops.operators)` invariant).
            $(
                if node.kind_id() == $predefined_type as u16
                    && node
                        .child(0)
                        .is_some_and(|child| child.kind_id() == Void as u16)
                {
                    return HalsteadType::Unknown;
                }
            )?

            match node.kind_id().into() {
                Export | Import | Import2 | Extends | DOT | From | LPAREN | COMMA | As | STAR
                | GTGT | GTGTGT | COLON | Return | Delete | Throw | Break | Continue | If
                | Else | Switch | Case | Default | Async | Do | For | In | Of | While | Try
                | Catch | Finally | With | EQ | AT | AMPAMP | PIPEPIPE | PLUS | DASH | DASHDASH
                | PLUSPLUS | SLASH | PERCENT | STARSTAR | PIPE | AMP | LTLT | TILDE | LT | LTEQ
                | EQEQ | BANGEQ | GTEQ | GT | PLUSEQ | BANG | BANGEQEQ | EQEQEQ | DASHEQ
                | STAREQ | SLASHEQ | PERCENTEQ | STARSTAREQ | GTGTEQ | GTGTGTEQ | LTLTEQ | AMPEQ
                | CARET | CARETEQ | PIPEEQ | Yield | LBRACK | LBRACE | Await | QMARK
                | QMARKQMARK | EQGT | DOTDOTDOT | New | Let | Var | Const | Function
                | FunctionExpression | SEMI | Typeof | Instanceof | Void
                // `get`/`set` accessor keywords are operators, matching the
                // C# getter's `Get | Set | Init | Add | Remove` accessor arm.
                // Before #695 the JS family classified them as operands, so
                // the same accessor keyword landed in opposite Halstead
                // groups across languages, skewing n1/n2 for accessor-heavy
                // code (#695).
                | Set | Get
                $(| $op_extra)* => HalsteadType::Operator,
                Identifier | MemberExpression | MemberExpression2 | MemberExpression3
                | PropertyIdentifier | String | Number | True | False | Null | This | Super
                | Undefined
                $(| $operand_extra)* => HalsteadType::Operand,
                // A `` `...` `` is a string literal; without interpolation it
                // mirrors `"..."` and contributes one operand. When it has a
                // `TemplateSubstitution` child the inner expression is already
                // walked and classified, so counting the wrapper too would
                // double-count its contribution to `N2` (issue #192, same
                // pattern as #183 C# / #191 Kotlin / #199 Perl).
                TemplateString => {
                    Self::string_operand_type(node, &[TemplateSubstitution as u16])
                }
                _ => HalsteadType::Unknown,
            }
        }
    };
}

/// Per-language accessors the space walker and the Halstead
/// operator/operand classification dispatch through.
///
/// # Precondition
///
/// Every method taking a `code: &[u8]` next to a `&Node` slices `code`
/// by that node's byte range. `code` must be the exact buffer `node` was
/// parsed from — [`crate::Ast::source`] for a node obtained from the
/// same [`crate::Ast`]. Pairing a node with any other buffer reads the
/// wrong bytes at best and panics on an out-of-bounds index at worst;
/// the same precondition is documented on [`crate::dump_node`] (#795).
/// Only `node_text`, behind the default `get_func_space_name`,
/// bounds-checks — see its docs for why the rest deliberately do not.
#[doc(hidden)]
pub(crate) trait Getter {
    fn get_func_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        Self::get_func_space_name(node, code)
    }

    fn get_func_space_name<'a>(node: &Node, code: &'a [u8]) -> Option<&'a str> {
        // we're in a function or in a class
        if let Some(name) = node.child_by_field_name("name") {
            node_text(code, &name)
        } else {
            Some("<anonymous>")
        }
    }

    fn get_space_kind(_node: &Node) -> SpaceKind {
        SpaceKind::Unknown
    }

    /// Source-aware variant of [`get_space_kind`]. The default
    /// forwards to the byte-less classifier; languages whose space
    /// kinds are encoded in macro identifier text (Elixir's
    /// `defmodule` / `def` / `defp` / `defmacro` / `defmacrop` Calls)
    /// override this so the walker can attribute the correct
    /// `SpaceKind` to each promoted func space (#275).
    #[inline]
    fn get_space_kind_with_code(node: &Node, _code: &[u8]) -> SpaceKind {
        Self::get_space_kind(node)
    }

    fn get_op_type(_node: &Node) -> HalsteadType {
        HalsteadType::Unknown
    }

    /// Source-aware variant of [`get_op_type`]. The default forwards
    /// to the byte-less classifier; languages whose Halstead operand
    /// classification depends on token text override this. Kotlin uses
    /// it to recover the variable in a short-form string template
    /// (`"Hi $name"`), which the grammar emits as bare `string_content`
    /// tokens with no structured interpolation node — the distinction
    /// between an interpolated `$name` and a literal `$5` is only
    /// visible in the source bytes (#454).
    #[inline]
    fn get_op_type_with_code(node: &Node, _code: &[u8]) -> HalsteadType {
        Self::get_op_type(node)
    }

    /// Returns the source-byte slice used to key a Halstead *operand*.
    /// The default keys on the operand node's full byte range. Kotlin
    /// overrides this to narrow a short-interpolation name token
    /// (`$name`) to its leading identifier prefix, because the grammar
    /// glues trailing inter-segment text onto the name token
    /// (`"$a $b"` → `"a "`); keying the raw bytes would record a
    /// distinct `"a "` operand and break parity with the long `${a}`
    /// form (#454).
    #[inline]
    fn get_operand_id<'a>(node: &Node, code: &'a [u8]) -> &'a [u8] {
        &code[node.start_byte()..node.end_byte()]
    }

    /// Classifies a string-literal `node` as a single Halstead
    /// operand, *unless* it wraps an interpolation child drawn from
    /// `interp_kinds` — in which case the wrapper yields
    /// [`HalsteadType::Unknown`] because the inner expressions are
    /// walked and counted separately. Counting the wrapper too would
    /// double-count their contribution to `N2`.
    ///
    /// This declares the per-language interpolation skip once (issue
    /// #420), replacing nine independently-added regression fixes
    /// (#183 / #184 / #191 / #192 / #199 / #277, …). Each language
    /// supplies only its own grammar's interpolation child-kind ids;
    /// the per-call rationale lives at each call site.
    fn string_operand_type(node: &Node, interp_kinds: &[u16]) -> HalsteadType {
        if node.wraps_any(interp_kinds) {
            HalsteadType::Unknown
        } else {
            HalsteadType::Operand
        }
    }

    fn get_operator_id_as_str(_id: u16) -> &'static str {
        ""
    }
}

mod bash;
mod c;
mod ccomment;
mod cpp;
mod csharp;
mod elixir;
mod go;
mod groovy;
mod irules;
mod java;
mod javascript;
mod kotlin;
mod lua;
mod mozcpp;
mod mozjs;
mod objc;
mod perl;
mod php;
mod preproc;
mod python;
mod ruby;
mod rust;
mod tcl;
mod tsx;
mod typescript;

#[cfg(test)]
mod node_text_tests {
    use super::node_text;
    use crate::langs::RustParser;
    use crate::traits::ParserTrait;
    use std::path::PathBuf;

    /// A node whose span lies inside the buffer it was parsed from
    /// yields its exact source text.
    #[test]
    fn in_bounds_span_returns_text() {
        let src = "fn x() {}";
        let parser = RustParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.rs"), None);
        let root = parser.root();
        assert_eq!(node_text(parser.code(), &root), Some(src));
    }

    /// Reslicing a node against a *shorter* buffer (the stale-span hazard
    /// the guard exists for) must degrade to `None`, not panic. A direct
    /// `&code[start..end]` would panic here — this is the revert check.
    #[test]
    fn out_of_bounds_span_returns_none_not_panic() {
        let src = "fn x() {}";
        let parser = RustParser::new(src.as_bytes().to_vec(), &PathBuf::from("t.rs"), None);
        let root = parser.root();
        assert!(root.end_byte() > 2);
        let truncated = &src.as_bytes()[..2];
        assert_eq!(node_text(truncated, &root), None);
    }

    /// The UTF-8 guard is the *other* failure mode, and unlike the range
    /// guard it is reachable without violating the same-parse
    /// precondition: `Ast::parse` accepts arbitrary bytes. A span whose
    /// bytes are not valid UTF-8 must yield `None`, not a panic and not
    /// lossy replacement characters.
    #[test]
    fn non_utf8_span_returns_none() {
        let mut src = b"fn ".to_vec();
        src.extend_from_slice(&[0xF0, 0x9F]);
        src.extend_from_slice(b"() {}");
        let parser = RustParser::new(src.clone(), &PathBuf::from("t.rs"), None);
        let root = parser.root();
        assert_eq!(root.end_byte(), src.len());
        assert_eq!(node_text(parser.code(), &root), None);
    }
}
