//! The `get_op_type` template the four JS-family grammars share
//! (JavaScript, MozJS, TypeScript, TSX), kept apart from the trait so
//! the per-language deltas and their rationale read as one unit.

// Emit a `Getter::get_op_type` body for a JS-family language. The four
// JS-family grammars (JavaScript, MozJS, TypeScript, TSX) share most of
// their Halstead operator/operand kind classifications; per-language
// deltas are passed as bracketed extras so all four impls stay in
// lockstep when a kind is added or removed (issue #299).
//
// `$op_extras` per language:
//   * JavaScript / MozJS: `OptionalChain` — the bare `?.` token (these
//     grammars expose no `optional_chain` wrapper).
//   * TypeScript / TSX:   `QMARKDOT`, the bare `?.` token under the
//     `optional_chain` wrapper (issue #281).
//   * JavaScript / MozJS / TSX: `LTSLASH`, `SLASHGT` — the JSX closing
//     and self-closing tag delimiters, which these grammars spell as
//     their own two-character tokens rather than as a `<` / `>` pair.
//     Until #1395 neither was in either arm, so a JSX element billed
//     its opening `<` and its `>`s but nothing for `</`, and `<br />`
//     reported one bracket operator where the source spells two. They
//     are extras rather than members of the shared arm because
//     TypeScript is the one grammar of the four with no JSX and
//     therefore no such variants to name.
//   * TypeScript / TSX: the type-level operator keywords listed at
//     each invocation.
//
// `roles:` names the grammar's module under `crate::roles`, whose
// `Modifier`, `DeclKeyword` and `Primitive` rows the emitted
// `get_keyword_role` reads (#1693): `static` and `Staticget`, the
// declaration keywords, and TypeScript's `predefined_type` with its
// `void` gate and the `export { type }` gate all live there now, not
// in the arm below.
//
// `$operand_extras` per language:
//   * JavaScript / MozJS / TSX: `Identifier2`, `String2` — anonymous
//     keyword aliases the JS grammar exposes for `Identifier` and
//     `String`. TSX's `String2` (kind_id 261) is its string-literal
//     alias, so it stays an operand like JS's.
//   * TypeScript: none. TS's own aliases are either operators or
//     deliberately unclassified (below).
//     The `: string` type-keyword aliases (TS `String2`, kind_id 135;
//     TSX `String3`, kind_id 141) are deliberately absent: they are
//     emitted only as the child of a `predefined_type` wrapper, which
//     already contributes the text-keyed `"string"` operator, and
//     #313's listing of the keyword child as an operand counted one
//     source token twice (#1261, which also narrowed
//     `Checker::is_string` to match).
//
// **A member access contributes its leaves, never the composite.**
// `a.b` is the operands `a` and `b` plus the `.` operator — three
// vocabulary entries, not four. Until #1263 the operand arm listed the
// `member_expression` wrapper *and* the walker descended into its
// `object` / `property` children, so every access billed one extra
// `N2` entry keyed on the whole `a.b` text (and `a.b.c` billed two:
// `a.b` and `a.b.c`). The composite is what the rest of the workspace
// already excludes — C, C++, Java, Rust, Python, Go, Kotlin, Ruby, Lua
// and PHP were all measured leaves-only for the identical shape — so
// identical code scored differently by language. The same call applies
// to TS's `nested_identifier` (`namespace N.M`), C#'s
// `qualified_name` / `generic_name` / `alias_qualified_name`, and
// Groovy's `qualified_name` / `qualified_type`.
//
// This is grammar-dispatch section 5's "any predicate listing both a
// container and a kind it can contain double-counts", and its section
// 6 corollary is why `PrivatePropertyIdentifier` joins the operand arm
// in the same change: `this.#x`'s `#x` leaf was in no operand list, so
// the composite had been its only count, and a bare deletion would
// have regressed private-field access to zero operands. (The `#x` in
// the *field definition* `#x = 1` had never been counted at all — no
// wrapper covered it — so this fixes that half too.)
//
// `jsx: [<text kind>, <entity kind>]` (JavaScript, MozJS, TSX) bills
// the content of a JSX element (#1483). A `jsx_text` node is an
// operand, the way a string's contents are; it is childless and no
// node containing it (`jsx_element`) is classified, so nothing bills it
// twice. Two refinements need the source bytes and so live in the
// `get_op_type_with_code` / `get_operand_id` overrides the parameter
// also emits:
//
//   * A whitespace-only `jsx_text` is `Unknown`. The scanner emits no
//     token for whitespace that starts with a newline, but same-line
//     spaces between tags (`<a/> <b/>`) are a node of their own, and
//     billing them would make `N2` follow the source's layout.
//   * The operand is keyed on the text with its surrounding whitespace
//     trimmed. A token keeps the newlines and indentation around it, so
//     `Start` at two indentation depths would otherwise be two distinct
//     operands. This is the same shape as Kotlin's #454 narrowing.
//     Only the ends are trimmed: a multi-line text keeps its inner
//     newlines and indentation, so the same lines at two depths, or
//     wrapped differently, stay distinct operands although JSX joins
//     them into one string. Folding them needs an owned key, and
//     `get_operand_id` returns a slice of the source.
//
// An `html_character_reference` (`&amp;`) splits the text around it
// into siblings, so `a &amp; b` is three operands: `a`, `&amp;` and
// `b`. Each node is one vocabulary entry, and the entity denotes a
// character the surrounding text does not, so it is not folded into
// either neighbour. The same kind also sits inside a JSX attribute's
// `string`, which is already the operand, so it is billed only when
// its parent is not a string.
//
// `Checker::is_string` deliberately does not list `jsx_text`: `find
// string` reports delimited string literals, the set the alterator
// flattens (#283), and element text is undelimited markup content.
//
// The `TemplateString` interpolation guard is shared verbatim (issue
// #192): a bare `` `...` `` mirrors a `"..."` operand, but an
// interpolated template must yield `Unknown` because its inner
// `TemplateSubstitution` expressions are walked separately.
//
// The `Regex` arms are shared the same way (issue #1314), and for the
// same reason: all four grammars spell a regex literal identically —
// a `regex` wrapper whose two delimiters are `SLASH`, the kind id real
// division uses — so neither arm has a per-language delta to hoist to
// a call site. Only the ids differ (`SLASH` 87/81/90/87, `Regex`
// 224/250/264/225) and the macro names both through the enum, so no
// invocation mentions either. See the arms themselves for the
// fabrication and the missing-operand halves.
macro_rules! impl_js_family_get_op_type {
    (
        $lang:ident,
        roles: $roles:ident,
        op_extras: [$($op_extra:ident),* $(,)?],
        operand_extras: [$($operand_extra:ident),* $(,)?]
        $(, generic_angles: $generic_angles:ident)?
        $(, jsx: [$jsx_text:ident, $jsx_entity:ident])? $(,)?
    ) => {
        keyword_roles!($roles);

        paying_roles!($roles);

        fn get_op_type<'a>(node: &Node<'a>, ancestors: Ancestors<'a, '_>) -> TokenRole {
            use $lang::*;

            $(
                if $generic_angles.is_closer(node, ancestors) {
                    return TokenRole::Unknown;
                }
            )?

            // The modifiers, the declaration keywords and TypeScript's
            // `predefined_type` are not here: each grammar's rows in
            // `roles/<lang>.rs` declare them and `get_keyword_role` bills
            // them (#1693), gates included. Nor are the equality and
            // relational tokens, `instanceof`, `&&` and `||`: the
            // `Comparison` and `ShortCircuit` rows pay them and
            // `get_paying_role` bills them (#1695). `<` / `>` stay for
            // the spellings the rows' parent gate leaves out — a JSX tag
            // delimiter, a TypeScript generic opener (the `<>` pair; its
            // closer returned `Unknown` above) and a recovered token —
            // which billed before the rows existed. `?` likewise stays for
            // the spellings the `Ternary` row's `under_ternary` gate leaves
            // out — TypeScript's optional members and types and its
            // `conditional_type` — which billed before the row existed.
            // role-bypass: allow(Comparison, Ternary) — the operator classification of the
            // spellings the rows' `under_binary_expression` / `under_ternary` gates leave out
            match node.kind_id().into() {
                // Regex delimiter punctuation. A `regex` literal spells
                // both of its delimiters with the same kind id real
                // division uses, so `const a = /abc/g;` reported a `/`
                // operator with no division in the source and n1/N1
                // counted the literal's punctuation as arithmetic
                // (#1314, the JS-family sibling of Elixir #1256 and
                // Ruby/Perl #1312). The `Regex` node itself is the
                // operand (below), so the delimiters are suppressed
                // exactly when their parent is that node — the
                // compound-leaf guard of grammar-dispatch section 5.
                //
                // Parent, not ancestor, for correctness by
                // construction rather than by observation: unlike
                // Ruby's `#{…}`, a JS regex admits no nested
                // expression at all (`regex_pattern` and `regex_flags`
                // are leaves), so no fixture can tell this guard from
                // an ancestor-scanning one. The halstead test
                // `js_regex_delimiter_guard_is_parent_scoped_is_unobservable`
                // records that, and pins the grammar property it rests
                // on, rather than implying coverage the suite lacks.
                //
                // `SLASH2` — the aliased regex-start token every one of
                // these grammars carries — needs no arm: it is absent
                // from the operator list below, so it already lands on
                // `Unknown`, which is what a delimiter should be. That
                // is the one place this differs from Ruby's guard,
                // where `SLASH2` had to be moved off the arithmetic arm.
                SLASH
                    if ancestors.parent_has_kind(node, Regex as u16) =>
                {
                    TokenRole::Unknown
                }
                LT | GT
                | Export | Import | Import2 | Extends | DOT | From | LPAREN | COMMA | As | STAR
                | GTGT | GTGTGT | COLON | Return | Delete | Throw | Break | Continue | If
                | Else | Switch | Case | Default | Async | Do | For | In | Of | While | Try
                | Catch | Finally | With | EQ | AT | PLUS | DASH | DASHDASH
                | PLUSPLUS | SLASH | PERCENT | STARSTAR | PIPE | AMP | LTLT | TILDE
                | PLUSEQ | BANG | DASHEQ
                | STAREQ | SLASHEQ | PERCENTEQ | STARSTAREQ | GTGTEQ | GTGTGTEQ | LTLTEQ | AMPEQ
                | CARET | CARETEQ | PIPEEQ | Yield | LBRACK | LBRACE | Await | QMARK
                | QMARKQMARK | EQGT | DOTDOTDOT | New | SEMI | Typeof | Void
                // `get`/`set` accessor keywords are operators, matching the
                // C# getter's `Get | Set | Init | Add | Remove` accessor arm.
                // Before #695 the JS family classified them as operands, so
                // the same accessor keyword landed in opposite Halstead
                // groups across languages, skewing n1/n2 for accessor-heavy
                // code (#695).
                | Set | Get
                $(| $op_extra)* => TokenRole::Operator,
                // `Regex` is the literal's own node and contributes one
                // operand, the way Ruby's `Regex` and Elixir's `Sigil`
                // do. It was in neither arm before #1314, so `/abc/g`
                // reached the vocabulary from *neither* side: a
                // fabricated `/` operator and no operand at all. Its
                // `regex_pattern` / `regex_flags` children stay
                // unclassified, so the wrapper cannot double-count
                // them, and no interpolation is possible inside a JS
                // regex — hence a plain arm here rather than the
                // `string_operand_type` dispatch the template literal
                // below needs.
                // `PrivatePropertyIdentifier` is the `#x` leaf of a
                // private class field, in both its declaration
                // (`#x = 1`) and its access (`this.#x`). See the
                // leaves-not-composites note above the macro for why
                // it had to be added in the same change that dropped
                // `MemberExpression*`.
                //
                // `MetaProperty` is `import.meta` / `new.target`: one
                // atomic operand, like `this`, whose `meta` / `target`
                // leaves are anonymous tokens no arm classifies. It is
                // the one composite the #1263 drop has to keep
                // (grammar-dispatch §6) — without it the meta-object
                // contributes no operand at all while `this.env.x`
                // still yields three.
                Identifier | PropertyIdentifier | PrivatePropertyIdentifier | MetaProperty
                | String | Number | True | False | Null | This | Super | Undefined | Regex
                $(| $operand_extra)* => TokenRole::Operand,
                // A `` `...` `` is a string literal; without interpolation it
                // mirrors `"..."` and contributes one operand. When it has a
                // `TemplateSubstitution` child the inner expression is already
                // walked and classified, so counting the wrapper too would
                // double-count its contribution to `N2` (issue #192, same
                // pattern as #183 C# / #191 Kotlin / #199 Perl).
                TemplateString => {
                    Self::string_operand_type(node, &[TemplateSubstitution as u16])
                }
                $(
                    $jsx_text => TokenRole::Operand,
                    $jsx_entity
                        if !ancestors
                            .parent(node)
                            .is_some_and(|p| matches!(p.kind_id().into(), String | String2)) =>
                    {
                        TokenRole::Operand
                    }
                )?
                _ => TokenRole::Unknown,
            }
        }

        $(
            fn get_operator_spelling<'a>(
                node: &Node<'a>,
                ancestors: Ancestors<'a, '_>,
            ) -> Option<&'static str> {
                $generic_angles.opener_spelling(node, ancestors)
            }
        )?

        $(
            fn get_op_type_with_code<'a>(
                node: &Node<'a>,
                code: &[u8],
                ancestors: Ancestors<'a, '_>,
            ) -> TokenRole {
                if node.kind_id() == $lang::$jsx_text as u16
                    && code[node.start_byte()..node.end_byte()].trim_ascii().is_empty()
                {
                    return TokenRole::Unknown;
                }
                keyword_or_op_type::<Self>(node, code, ancestors)
            }

            fn get_operand_id<'a>(
                node: &Node<'a>,
                code: &'a [u8],
                _ancestors: Ancestors<'a, '_>,
            ) -> &'a [u8] {
                let text = &code[node.start_byte()..node.end_byte()];
                if node.kind_id() == $lang::$jsx_text as u16 {
                    text.trim_ascii()
                } else {
                    text
                }
            }
        )?
    };
}

pub(super) use impl_js_family_get_op_type;
