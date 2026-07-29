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

impl Getter for KotlinCode {
    fn get_space_kind(node: &Node) -> SpaceKind {
        use Kotlin::*;

        match node.kind_id().into() {
            // The Kotlin grammar models classes and interfaces under a single
            // `class_declaration` node with the discriminating keyword
            // (`class` vs `interface`) appearing as a direct child token.
            // Distinguishing them at the space-kind level lets the OOP
            // metrics (`wmc`, `npa`, `npm`) attribute counts to the right
            // bucket without re-inspecting the AST.
            ClassDeclaration => {
                if node.first_child(|id| id == Interface).is_some() {
                    SpaceKind::Interface
                } else {
                    SpaceKind::Class
                }
            }
            // `object MyObject { ... }` is a singleton; treat it as a class
            // for OOP metric purposes (it has properties, methods, init
            // blocks, exactly like a class).
            //
            // `companion object { ... }` is the anonymous (or named)
            // singleton nested in a class. Its body is a `class_body`
            // holding methods and properties, exactly like a named
            // object, so it opens its own Class space rather than
            // folding its members into the enclosing class (#431). The
            // default `get_func_space_name` reads the optional `name`
            // field, yielding the declared name for `companion object
            // Named` and `<anonymous>` for the nameless form.
            //
            // `object : T { ... }` (`object_literal`) is the anonymous
            // object expression — Kotlin's analogue of a Java anonymous
            // class. The grammar models its body as a `class_body` of
            // methods and properties, identical in shape to a named
            // object, so it opens its own Class space rather than folding
            // its members into the enclosing function (#463). It is always
            // nameless, so the default `get_func_space_name` yields
            // `<anonymous>`.
            ObjectDeclaration | CompanionObject | ObjectLiteral => SpaceKind::Class,
            FunctionDeclaration | SecondaryConstructor | LambdaLiteral | AnonymousFunction => {
                SpaceKind::Function
            }
            SourceFile => SpaceKind::Unit,
            _ => SpaceKind::Unknown,
        }
    }

    fn get_op_type<'a>(node: &Node<'a>, _ancestors: Ancestors<'a, '_>) -> HalsteadType {
        use Kotlin::*;

        match node.kind_id().into() {
            // Operator: control flow keywords
            If | Else | When | For | While | Do | Try | Catch | Finally | Throw | Return
            | ReturnAT
            // Operator: other keywords
            | Class | Fun | Object | Val | Var | In | Is | As | AsQMARK | BANGis | BANGin
            | This | Super | Constructor
            // Operator: brackets, separators, terminators
            | SEMI | COMMA | COLONCOLON | DOT | LBRACE | LBRACK | LPAREN
            // Operator: assignment and arithmetic
            | EQ | PLUS | DASH | STAR | SLASH | PERCENT
            | PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ
            | PLUSPLUS | DASHDASH
            // Operator: comparison and equality
            | LT | GT | LTEQ | GTEQ | EQEQ | EQEQEQ | BANGEQ | BANGEQEQ
            // Operator: logical and misc
            | AMPAMP | PIPEPIPE | BANG | BANGBANG
            | QMARK | QMARKCOLON | QMARKDOT
            | DOTDOT | DOTDOTLT | DASHGT | COLON => HalsteadType::Operator,
            // Operands: identifiers and literals
            Identifier | NumberLiteral | FloatLiteral | CharacterLiteral | Label => {
                HalsteadType::Operand
            }
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
            _ => HalsteadType::Unknown,
        }
    }

    fn get_op_type_with_code<'a>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> HalsteadType {
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
                HalsteadType::Operand
            }
            // The wrapping literal is Unknown when it interpolates in
            // either form (long `${expr}` or short `$name`); the inner
            // expression / recovered variable is the only operand. A
            // plain literal has no interpolation and stays one operand.
            StringLiteral | MultilineStringLiteral => {
                if kotlin_string_has_interp(node, code) {
                    HalsteadType::Unknown
                } else {
                    HalsteadType::Operand
                }
            }
            _ => Self::get_op_type(node, ancestors),
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

    get_operator!(Kotlin);
}
