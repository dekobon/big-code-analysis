//! `Getter` implementation for Kotlin.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

use super::*;

/// Returns the byte length of the leading Kotlin identifier in `text`
/// — a Unicode letter or `_` start followed by letters / digits / `_`
/// — or `0` if `text` does not start with an identifier.
///
/// The short string template `$name` interpolates a *simple
/// identifier*, but the kotlin-ng grammar does not tokenise at the
/// identifier boundary: it splits the literal only at each `$`, so the
/// token after `$` absorbs any trailing inter-segment text into its own
/// byte range. For `"$a $b"` the token after the first `$` is `"a "`
/// (with the trailing space), and for `"$x is "` it is `"x is "`. Taking
/// the maximal leading-identifier prefix recovers the interpolated
/// variable (`a`, `x`) and treats the remainder as literal text, so the
/// short form matches the long `${a}` / `${x}` form (#454). A token with
/// no identifier prefix (`"price: $5"` → `"5"`, digit start) yields `0`
/// and stays literal.
fn kotlin_leading_identifier_len(text: &str) -> usize {
    let mut chars = text.char_indices();
    let Some((_, first)) = chars.next() else {
        return 0;
    };
    if first != '_' && !first.is_alphabetic() {
        return 0;
    }
    // The leading identifier ends at the first byte that is neither `_`
    // nor alphanumeric; everything from there on is inter-segment
    // literal text the grammar glued onto this `$name` token.
    let mut end = first.len_utf8();
    for (idx, c) in chars {
        if c == '_' || c.is_alphanumeric() {
            end = idx + c.len_utf8();
        } else {
            break;
        }
    }
    end
}

/// Returns whether the `string_content` token `node` is (or begins
/// with) the variable-name part of a Kotlin short string template
/// (`$name`). The kotlin-ng grammar emits no structured node for this
/// form — the `$` sigil and the variable name arrive as two adjacent
/// `string_content` tokens (verified by AST dump, #454). The name token
/// is recognised by a preceding `string_content` sibling spelled
/// exactly `$` plus its own text starting with an identifier. Because
/// the grammar glues trailing inter-segment text onto the name token
/// (`"$a $b"` → `"a "`, `"$x is "` → `"x is "`), a non-empty leading
/// identifier prefix is sufficient; the prefix is the recovered operand
/// and the remainder is literal text. Only a token with no identifier
/// prefix (`"price: $5"` → `"5"`) stays literal.
fn kotlin_is_short_interp_name(node: &Node, previous: Option<Node<'_>>, code: &[u8]) -> bool {
    use Kotlin::{StringContent, StringContent2, StringContent3};

    // The grammar emits three aliased `string_content` kind ids
    // (single-line uses `StringContent3`, others `StringContent` /
    // `StringContent2`); all render as the text `"string_content"`.
    const STRING_CONTENT_KINDS: [u16; 3] = [
        StringContent as u16,
        StringContent2 as u16,
        StringContent3 as u16,
    ];

    if !STRING_CONTENT_KINDS.contains(&node.kind_id()) {
        return false;
    }
    let Some(prev) = previous else {
        return false;
    };
    if !STRING_CONTENT_KINDS.contains(&prev.kind_id()) || prev.utf8_text(code) != Some("$") {
        return false;
    }
    node.utf8_text(code)
        .is_some_and(|text| kotlin_leading_identifier_len(text) > 0)
}

/// Returns the byte slice keying the Halstead operand for a Kotlin
/// short-interpolation name token (`$name`). The grammar glues trailing
/// inter-segment text onto the name token, so the raw node bytes for
/// `"$a $b"` are `"a "` — keying that would record a distinct `"a "`
/// operand and break parity with the long `${a}` form. Narrow the slice
/// to the leading identifier prefix so the operand is keyed as the bare
/// identifier (`"a"`), matching the long form (#454).
fn kotlin_short_interp_operand_bytes<'a>(node: &Node, code: &'a [u8]) -> &'a [u8] {
    let start = node.start_byte();
    let end = node.end_byte();
    let full = &code[start..end];
    match std::str::from_utf8(full) {
        Ok(text) => {
            let prefix_len = kotlin_leading_identifier_len(text);
            if prefix_len > 0 {
                &full[..prefix_len]
            } else {
                full
            }
        }
        Err(_) => full,
    }
}

/// Returns whether a Kotlin string literal `node` carries any
/// interpolation: the long `${expr}` form (a structured `Interpolation`
/// child) or the short `$name` form (adjacent `$` + identifier
/// `string_content` tokens). The wrapping literal yields Unknown in
/// either case so the inner expression is the only counted operand
/// (#454).
fn kotlin_string_has_interp(node: &Node, code: &[u8]) -> bool {
    use Kotlin::Interpolation;

    // The scan already holds each child's predecessor, so the short-form
    // test gets it for free rather than through the `O(depth)`
    // `Node::previous_sibling` (#1096).
    let mut previous = None;
    node.children().any(|child| {
        let found = child.kind_id() == Interpolation as u16
            || kotlin_is_short_interp_name(&child, previous, code);
        previous = Some(child);
        found
    })
}

// A qualified `super<A>` (a `super_expression`) encloses the supertype
// it names (#1567).
const GENERIC_ANGLES: GenericAngleKinds = GenericAngleKinds {
    lists: &[
        Kotlin::TypeArguments as u16,
        Kotlin::TypeParameters as u16,
        Kotlin::SuperExpression as u16,
    ],
    openers: &[Kotlin::LT as u16],
    closers: &[Kotlin::GT as u16],
    is_misparse: never_misparsed,
};

impl Getter for KotlinCode {
    /// Names the space, synthesising one for constructs that carry no
    /// name token (#1184).
    ///
    /// `get_func_space_name` returns `Option<&'a str>` borrowed from
    /// `code`, so the only name available for a nameless construct is a
    /// `&'static str` — a per-property spelling like `<get-foo>` would
    /// need a signature change. Angle brackets follow the existing
    /// `<anonymous>` convention and cannot collide with a real
    /// identifier in any of these grammars.
    ///
    /// Sibling collisions are accepted, exactly as multiple
    /// `<anonymous>` siblings already are: two properties each with a
    /// getter, or two `static { }` blocks in one class, produce two
    /// spaces with the same name. Nothing enforces name uniqueness
    /// among siblings, and inventing an index would make the name
    /// unstable under an unrelated edit.
    ///
    /// `<get>` / `<set>` are Kotlin's own spelling of the accessor
    /// keywords; `<init>` is what the JVM calls the constructor an
    /// `init` block compiles into.
    fn get_func_space_name<'a, 'tree>(
        node: &Node<'tree>,
        code: &'a [u8],
        ancestors: Ancestors<'tree, '_>,
    ) -> Option<&'a str> {
        // role-bypass: allow(FuncOpener) — a naming rule, not whether the
        // node opens a space
        match node.kind_id().into() {
            Kotlin::Getter => Some("<get>"),
            Kotlin::Setter => Some("<set>"),
            Kotlin::AnonymousInitializer => Some("<init>"),
            _ => crate::getter::default_func_space_name(node, code, ancestors),
        }
    }

    // The mapping is the `opens` annotations of `roles/kotlin.rs`
    // (#1696); the byte-less spelling forwards with no bytes and no
    // chain, as the old one read neither.
    fn get_space_kind(node: &Node) -> SpaceKind {
        Self::get_space_kind_with_code(node, &[], Ancestors::known(&[]))
    }

    fn get_space_kind_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> SpaceKind {
        crate::roles::kotlin::opens_space(node, code, ancestors)
    }

    keyword_roles!(kotlin);

    paying_roles!(kotlin);

    fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
        use Kotlin::*;

        if GENERIC_ANGLES.is_closer(node, ancestors) {
            return TokenRole::Unknown;
        }

        // The modifiers, the declaration keywords and `reified` are not
        // here: `roles/kotlin.rs` declares them, gate included, and
        // `get_keyword_role` bills them (#1693).
        let kind: Kotlin = node.kind_id().into();

        // The accessor keywords, billed as C# bills its `get` / `set`
        // (#1564), but only directly under their `getter` / `setter`,
        // which stay unlisted. The same leaves are the `@get:` / `@set:`
        // use-site targets, which bill nothing, like `@field:` and
        // `@property:`; and where kotlin-ng misreads a call `get(1)` after
        // a local `val f = 6`, an `ERROR` holds the leaf. Used as a name
        // otherwise (`val get = 1`, `x.set(v)`, `fun get()`) the word
        // parses as an identifier. The grammar also reads a local
        // `val f = 6` followed by `set = 2` as a setter, which bills
        // `set`. `field` in an accessor body is an identifier, so it is an
        // operand like any other variable. An early return for the same
        // reason as the modifiers'.
        let accessor = match kind {
            Get => Some(Kotlin::Getter),
            Set => Some(Setter),
            _ => None,
        };
        if let Some(accessor) = accessor {
            return if ancestors.parent_has_kind(node, accessor as u16) {
                TokenRole::Operator
            } else {
                TokenRole::Unknown
            };
        }

        match kind {
            // Operator: control flow keywords
            If | Else | When | For | While | Do | Try | Catch | Finally | Throw | Return
            | ReturnAT
            // Operator: other keywords. `init`, `where` and `by` bill as
            // C# bills `where` and TypeScript `type` (#1564), each the
            // leaf of its `anonymous_initializer` / `type_constraints` /
            // `property_delegate` or `explicit_delegation` wrapper; used
            // as names (`val by = 1`, `where(x)`, `init.f()`) they parse
            // as identifiers, so the leaf exists only in its construct.
            // `in` is the variance modifier and the operator of `for` /
            // `x in y` in one kind, billed here; `out` is the row's.
            // `is` / `!is` / `in` / `!in` for the spellings the row's
            // expression gate leaves out — a `when` entry's `type_test` /
            // `range_test` and a `for` header's `in` — which billed
            // before the row existed.
            // role-bypass: allow(Comparison) — the operator classification of the spellings the
            // row's `under IsExpression` / `under InExpression` gates leave out
            | In | Is | BANGis | BANGin
            | As | AsQMARK | Constructor | Init | Where | By
            // Operator: brackets, separators, terminators
            | SEMI | COMMA | COLONCOLON | DOT | LBRACE | LBRACK | LPAREN
            // Operator: assignment and arithmetic
            | EQ | PLUS | DASH | STAR | SLASH | PERCENT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ
            | PLUSPLUS | DASHDASH
            // `<` / `>` for the spellings the `Comparison` row's parent
            // gate leaves out — `super<A>`, a generic bracket (the `<>`
            // pair), a recovered token — which billed before the row
            // existed; the equalities, the short-circuit tokens, the
            // elvis `?:` and the safe navigation's `?.` are the rows' and
            // `get_paying_role` bills them (#1695). `!!` is excluded on
            // the `NullSafe` row and bills here.
            // role-bypass: allow(Comparison) — the operator classification of the spellings the
            // row's `under BinaryExpression` gate leaves out
            | LT | GT
            // Operator: logical and misc
            | BANG | BANGBANG
            | QMARK
            | DOTDOT | DOTDOTLT | DASHGT | COLON => TokenRole::Operator,
            // Operands: identifiers and literals, plus the self- and
            // super-reference leaves (#1380). Four leaf kinds, not two:
            // the label-qualified spellings `this@Outer` / `super@Inner`
            // are `this@` / `super@` tokens, distinct kind_ids that
            // carry no `this` / `super` leaf of their own — the same
            // shape as `return@`, already listed among the operators
            // above.
            //
            // The leaves are the keepers, not the `this_expression` /
            // `super_expression` wrappers (grammar-dispatch section 6):
            // a `constructor_delegation_call` (`constructor() : this(0)`,
            // `: super(x)`) emits a bare leaf with no wrapper at all, so
            // billing the wrapper would score constructor delegation
            // zero. Classifying both would double-count, and the
            // wrapper's span swallows the label identifier that is
            // already billed on its own (section 5).
            Identifier | NumberLiteral | FloatLiteral | CharacterLiteral | Label | This
            | Super | ThisAT | SuperAT => TokenRole::Operand,
            // Regression #191: a Kotlin string template wraps an
            // `Interpolation` child (the long `"${expr}"` form) whose
            // inner expressions are walked and counted separately, so
            // the wrapping literal yields Unknown to avoid double-
            // counting (same pattern as #180 Bash/Elixir, #184 PHP).
            // The short `"Hi $name"` form has no `Interpolation` node —
            // it needs the source bytes to distinguish `$name` from
            // literal `$5`, so it is handled in `get_op_type_with_code`;
            // this byte-less arm sees only the long form. Both single-
            // line and multi-line (triple-quoted) literals interpolate.
            StringLiteral | MultilineStringLiteral => {
                Self::string_operand_type(node, &[Interpolation as u16])
            }
            _ => TokenRole::Unknown,
        }
    }

    fn get_op_type_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> TokenRole {
        use Kotlin::*;

        match node.kind_id().into() {
            // The kotlin-ng grammar emits no structured node for the
            // short string template `"Hi $name"` — the `$` sigil and the
            // variable name arrive as two adjacent `string_content`
            // tokens (issue #454). When the name token is a clean
            // identifier, recover it as the operand the long `${name}`
            // form would have produced, so both template forms count the
            // inner variable and suppress the wrapping literal (the
            // wrapper arm below). The grammar does not tokenise at the
            // identifier boundary for mid-segment forms (`"$x is "`), so
            // those are left as literal text rather than emitting a
            // partial-literal operand.
            StringContent | StringContent2 | StringContent3
                if kotlin_is_short_interp_name(node, ancestors.previous_sibling(node), code) =>
            {
                TokenRole::Operand
            }
            // The wrapping literal is Unknown when it interpolates in
            // either form (long `${expr}` or short `$name`); the inner
            // expression / recovered variable is the only operand. A
            // plain literal has no interpolation and stays one operand.
            StringLiteral | MultilineStringLiteral => {
                if kotlin_string_has_interp(node, code) {
                    TokenRole::Unknown
                } else {
                    TokenRole::Operand
                }
            }
            _ => keyword_or_op_type::<Self>(node, code, ancestors),
        }
    }

    fn get_operand_id<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
    ) -> &'a [u8] {
        // Narrow a recovered short-interpolation name token to its
        // leading identifier prefix so `"$a $b"` keys operand `"a"`,
        // not `"a "`, matching the long `${a}` form (#454).
        if kotlin_is_short_interp_name(node, ancestors.previous_sibling(node), code) {
            kotlin_short_interp_operand_bytes(node, code)
        } else {
            &code[node.start_byte()..node.end_byte()]
        }
    }

    fn get_operator_spelling<'a>(
        node: &Node<'a>,
        ancestors: Ancestors<'a, '_>,
    ) -> Option<&'static str> {
        GENERIC_ANGLES.opener_spelling(node, ancestors)
    }

    get_operator!(Kotlin);
}
