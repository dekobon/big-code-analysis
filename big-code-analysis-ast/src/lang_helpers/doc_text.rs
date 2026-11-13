//! Doc-comment recognition by bytes, for the grammars that emit one
//! comment kind for every comment form (#1597).
//!
//! Rust (a doc marker child), Groovy (`groovydoc_comment`) and
//! Perl (`pod_statement`) mark documentation structurally. Every other
//! grammar parses `/** … */`, `///` or `---` to the same `comment` node
//! as a plain comment, so the opening marker is the only evidence, and
//! each language's own tool decides which markers count: Doxygen, javac,
//! Kotlin's lexer, JSDoc, PHP's scanner, Roslyn and LDoc disagree at the
//! edges (`/***`, `////`, `/**/`), and the predicates below follow each
//! tool rather than one rule for all. The slice they inspect is the
//! comment node's own span, which never reaches `to_string_lossy`
//! (grammar-dispatch rule 10): a comment's bytes may be any encoding,
//! and a marker is ASCII.
//!
//! Deliberate deviations, all on the strict side: `////` is never
//! documentation (tree-sitter-rust and Roslyn reject it; Doxygen's and
//! javac's lexers accept it, but a line of slashes is a banner, not
//! prose); the empty `/**/` is never documentation (javac lexes it as
//! an empty doc comment, which documents nothing); and a doc marker has
//! to open the comment — a `/**` found later in a comment's text is
//! text.
//!
//! Every test here reads bytes only and never a `Node`; which of the
//! comments they accept document, and what, is the binding walk's
//! question in [`super::doc_comment`] (#1649).

/// `/**` opened by a byte that is neither `*` nor `/`: the Javadoc /
/// JSDoc / Doxygen block form. Rejects the empty `/**/` and the banner
/// `/***`, as JSDoc ("more than 3 stars will be ignored"), Doxygen
/// (`JAVADOC_BANNER` defaults to `NO`) and Roslyn do.
#[inline]
#[must_use]
pub(crate) fn javadoc_block(s: &[u8]) -> bool {
    matches!(s, [b'/', b'*', b'*', b, ..] if *b != b'*' && *b != b'/')
}

/// A JSDoc comment as tsc reads it (`isJSDocLikeText`): `/**` followed
/// by any byte but `/`. Unlike JSDoc itself it takes the banner `/***`;
/// only the empty `/**/` is rejected. TypeScript and TSX follow tsc, since
/// tsc is what reads their doc comments (#1597).
#[inline]
#[must_use]
pub(crate) fn tsdoc_block(s: &[u8]) -> bool {
    matches!(s, [b'/', b'*', b'*', b, ..] if *b != b'/')
}

/// `///` not followed by a fourth slash: the C# / Doxygen / Rust line
/// form, including the after-member `///<`. Rejects `////`.
#[inline]
#[must_use]
fn triple_slash(s: &[u8]) -> bool {
    matches!(s, [b'/', b'/', b'/', rest @ ..] if rest.first() != Some(&b'/'))
}

/// A Doxygen comment block in a C-family language: the Javadoc block,
/// the Qt `/*!` block, and the `///` / `//!` line forms, each also in
/// its after-member `<` spelling (Doxygen, "Comment blocks for C-like
/// languages"). `////` is rejected — stricter than Doxygen's own lexer,
/// see the module doc.
#[inline]
#[must_use]
pub(crate) fn doxygen(s: &[u8]) -> bool {
    javadoc_block(s) || s.starts_with(b"/*!") || triple_slash(s) || s.starts_with(b"//!")
}

/// A Javadoc comment: any `/**` block but the empty `/**/` (a third
/// `*` is just a leading asterisk the spec discards), plus the JEP 467
/// Markdown form `///`. Javac's lexer also takes `/**/` and `////` as
/// doc comments; both are rejected here, as the module doc says.
#[inline]
#[must_use]
pub(crate) fn java_doc(s: &[u8]) -> bool {
    kotlin_doc(s) || triple_slash(s)
}

/// A KDoc block as Kotlin's lexer tokenises it: `/**` except the empty
/// `/**/`. `/***` is KDoc; line comments never are.
#[inline]
#[must_use]
pub(crate) fn kotlin_doc(s: &[u8]) -> bool {
    s.starts_with(b"/**") && s != b"/**/"
}

/// A PHPDoc block as the Zend scanner tokenises `T_DOC_COMMENT`: `/**`
/// followed by whitespace. `/**/`, `/***` and `/**x` are plain comments.
#[inline]
#[must_use]
pub(crate) fn phpdoc(s: &[u8]) -> bool {
    s.starts_with(b"/**") && s.get(3).is_some_and(u8::is_ascii_whitespace)
}

/// A C# XML documentation comment as Roslyn lexes it: `///` (not
/// `////`) or `/**` (not `/**/` nor `/***`). `//!` and `/*!` are the
/// Doxygen spellings and are plain comments here.
#[inline]
#[must_use]
pub(crate) fn csharp_doc(s: &[u8]) -> bool {
    triple_slash(s) || javadoc_block(s)
}

/// A doc comment's line with the delimiters JSDoc's `unwrap` strips: the
/// indent, one leading `*` and the space after it.
fn jsdoc_line(line: &[u8]) -> &[u8] {
    let line = line.trim_ascii_start();
    let line = line.strip_prefix(b"*").unwrap_or(line);
    line.strip_prefix(b" ").unwrap_or(line)
}

/// The lines of a `/** … */` body, delimiters and the extra stars at
/// either end stripped (`unwrap`), each with its leading ` * ` stripped
/// ([`jsdoc_line`]). `Clone`, so a tag's text — every line up to the next
/// tag, as JSDoc's `toTags` reads it — can be read in place from the line
/// that opened it.
fn jsdoc_lines(s: &[u8]) -> impl Iterator<Item = &[u8]> + Clone {
    let body = s.strip_prefix(b"/**").unwrap_or(s);
    let body = body.strip_suffix(b"*/").unwrap_or(body);
    let start = body.iter().position(|&b| b != b'*').unwrap_or(body.len());
    let end = body
        .iter()
        .rposition(|&b| b != b'*')
        .map_or(start, |at| at + 1);
    body[start..end].split(|&b| b == b'\n').map(jsdoc_line)
}

/// The `(title, text)` a line opens when it starts a tag.
fn jsdoc_tag_open(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let tag = line.trim_ascii_start().strip_prefix(b"@")?;
    let title = tag.iter().take_while(|b| !b.is_ascii_whitespace()).count();
    (title > 0).then(|| tag.split_at(title))
}

/// Whether a JSDoc tag's text names a symbol: anything left once the
/// `{type}` that may lead it is set aside.
fn jsdoc_names<'a>(lines: impl IntoIterator<Item = &'a [u8]>) -> bool {
    // The lines joined, read once without joining them: a `{type}` is
    // skipped by brace depth, and anything non-blank after it names.
    let mut bytes = lines
        .into_iter()
        .flat_map(|line| line.iter().copied())
        .skip_while(u8::is_ascii_whitespace)
        .peekable();
    if bytes.peek() == Some(&b'{') {
        let mut depth = 0_usize;
        for b in bytes.by_ref() {
            match b {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                break;
            }
        }
    }
    bytes.any(|b| !b.is_ascii_whitespace())
}

/// A JSDoc *virtual* comment, which stands alone: it documents the symbol
/// its tags name rather than the code after it, and the code after it gets
/// no doc from it (`createSymbolDoclet` drops a comment that set a name).
/// `@file`, `@fileoverview`, `@overview` and `@module` always set one; the
/// tags that take a name set one only when given it. `@license` sets none,
/// so a licence comment documents what follows like any other (#1613).
/// Titles match in any case, as the tag dictionary's `normalize` does.
#[must_use]
pub(crate) fn jsdoc_virtual(s: &[u8]) -> bool {
    const ALWAYS: [&[u8]; 4] = [b"file", b"fileoverview", b"overview", b"module"];
    const NAMING: [&[u8]; 18] = [
        b"name",
        b"function",
        b"func",
        b"method",
        b"class",
        b"constructor",
        b"constant",
        b"const",
        b"member",
        b"var",
        b"namespace",
        b"mixin",
        b"interface",
        b"event",
        b"external",
        b"host",
        b"typedef",
        b"callback",
    ];
    let is = |title: &[u8], names: &[&[u8]]| names.iter().any(|n| title.eq_ignore_ascii_case(n));
    let mut lines = jsdoc_lines(s);
    while let Some(line) = lines.next() {
        let Some((title, text)) = jsdoc_tag_open(line) else {
            continue;
        };
        if is(title, &ALWAYS) {
            return true;
        }
        // The tag's text runs over every line up to the next tag.
        let rest = lines.clone().take_while(|l| jsdoc_tag_open(l).is_none());
        if is(title, &NAMING) && jsdoc_names(std::iter::once(text).chain(rest)) {
            return true;
        }
    }
    false
}

/// A TypeDoc module comment: one carrying `@module` or
/// `@packageDocumentation`, which `getCommentImpl` withholds from any
/// declaration and reads as the file's documentation (#1613).
#[must_use]
pub(crate) fn tsdoc_module(s: &[u8]) -> bool {
    jsdoc_lines(s)
        .filter_map(jsdoc_tag_open)
        .any(|(title, _)| title == b"module" || title == b"packageDocumentation")
}

/// A go/doc directive line, part of a lead comment group but not of
/// its text (go.dev/doc/comment, "Directives"): `//go:build`,
/// `//line `, `//extern `, `//export `, and the general
/// `//[a-z0-9]+:[a-z0-9]` form. A directive has no space after `//`.
#[must_use]
pub(crate) fn go_directive(s: &[u8]) -> bool {
    let Some(rest) = s.strip_prefix(b"//") else {
        return false;
    };
    if rest.starts_with(b"line ") || rest.starts_with(b"extern ") || rest.starts_with(b"export ") {
        return true;
    }
    let word = rest
        .iter()
        .take_while(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        .count();
    word > 0
        && rest.get(word) == Some(&b':')
        && rest
            .get(word.saturating_add(1))
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// A Ruby comment that opens a documentation block whatever follows it:
/// RDoc's `##`, which introduces a metaprogrammed method's docs
/// (`RDoc::Parser::Ruby`), or a YARD `@!` directive (`# @!method`,
/// `# @!attribute`, `# @!macro`, `# @!parse`), which documents a DSL call
/// rather than a `def` (#1597). The `##` is exactly two `#`: YARD sets its
/// hash flag only for a `##` prefix and RDoc reads a lone `##` line, so a
/// `####…` banner or a `##### Helpers #####` divider opens nothing.
#[must_use]
pub(crate) fn ruby_directive_opener(s: &[u8]) -> bool {
    if s.starts_with(b"##") && s.get(2) != Some(&b'#') {
        return true;
    }
    s.strip_prefix(b"#").is_some_and(|rest| {
        rest.iter()
            .position(|b| *b != b' ' && *b != b'\t')
            .is_some_and(|at| rest[at..].starts_with(b"@!"))
    })
}

/// The comment lines YARD drops before documenting: a shebang, an
/// encoding line and the `frozen_string_literal` magic comment, as
/// YARD's `SHEBANG_LINE`, `ENCODING_LINE` and `FROZEN_STRING_LINE` name
/// them, case-insensitively. YARD applies them only before the first
/// token; here a line is excluded wherever it sits, so the match is on
/// the whole comment — `# coding: utf-8`, `# -*- coding: utf-8 -*-`,
/// `# vim: set fileencoding=utf-8 :`, `# frozen_string_literal: true` —
/// and a documentation row that only mentions an encoding (`# Encoding:
/// defaults to UTF-8.`) stays documentation. What this loses is a
/// comment that repeats a magic line mid-file.
#[must_use]
pub(crate) fn ruby_magic_comment(s: &[u8]) -> bool {
    if s.starts_with(b"#!") {
        return true;
    }
    s.strip_prefix(b"#")
        .map(<[u8]>::trim_ascii)
        .is_some_and(|body| ruby_modeline(body) || ruby_magic_setting(body))
}

fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// An Emacs (`-*- … -*-`) or Vim (`vim: …`) modeline that carries the
/// magic setting among others: an encoding, or for Emacs the
/// frozen-string flag. A modeline's key is never a magic one, so
/// `ruby_magic_setting` cannot also match it.
fn ruby_modeline(body: &[u8]) -> bool {
    if body.len() >= 6 && body.starts_with(b"-*-") && body.ends_with(b"-*-") {
        return contains_ci(body, b"coding") || contains_ci(body, b"frozen");
    }
    body.get(..4)
        .is_some_and(|head| head.eq_ignore_ascii_case(b"vim:"))
        && contains_ci(body, b"coding")
}

/// A comment that is one magic setting and nothing else: `key: value`
/// or `key=value`, the key a magic one and the value a single word.
fn ruby_magic_setting(body: &[u8]) -> bool {
    let Some(separator) = body.iter().position(|b| matches!(b, b':' | b'=')) else {
        return false;
    };
    let (key, value) = body.split_at(separator);
    let value = value.get(1..).unwrap_or_default().trim_ascii_start();
    [
        &b"coding"[..],
        b"encoding",
        b"frozen_string_literal",
        b"frozen-string-literal",
    ]
    .iter()
    .any(|known| key.trim_ascii_end().eq_ignore_ascii_case(known))
        && !value.is_empty()
        && value
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// A POD block that is nothing but `comment` regions — `=begin comment`
/// … `=end comment` or a `=for comment` paragraph — which perlpod leaves
/// to no formatter and perlfaq7 recommends for commenting code out: POD
/// syntax, not documentation. A region covers only its own text, as
/// perlpod scopes it: a `=for comment` paragraph ends at the next blank
/// line and a `=begin comment` region at its `=end comment`, so a block
/// that goes on to document something (`=head1 NAME` …) is documentation.
#[must_use]
pub(crate) fn pod_comment_region(s: &[u8]) -> bool {
    enum Region {
        Outside,
        Paragraph,
        Begin,
    }
    let mut region = Region::Outside;
    let mut regions = 0_usize;
    for line in s.split(|&byte| byte == b'\n') {
        let blank = line.trim_ascii().is_empty();
        match region {
            Region::Paragraph if blank => region = Region::Outside,
            Region::Begin if pod_command(line, b"=end").is_some_and(is_comment_format) => {
                region = Region::Outside;
            }
            Region::Paragraph | Region::Begin => {}
            Region::Outside if blank => {}
            Region::Outside => {
                if pod_command(line, b"=for").is_some_and(is_comment_format) {
                    region = Region::Paragraph;
                } else if pod_command(line, b"=begin").is_some_and(is_comment_format) {
                    region = Region::Begin;
                } else {
                    // `=cut` closes the block; any other line documents, or
                    // opens a block that never was a comment region.
                    return regions > 0 && line.starts_with(b"=cut");
                }
                regions += 1;
            }
        }
    }
    regions > 0
}

/// The text after a POD command line's `=<command>` and the whitespace
/// separating it from its argument, or `None` when `line` is not that
/// command.
fn pod_command<'s>(line: &'s [u8], command: &[u8]) -> Option<&'s [u8]> {
    let rest = line.strip_prefix(command)?;
    rest.first()
        .is_some_and(u8::is_ascii_whitespace)
        .then(|| rest.trim_ascii_start())
}

/// Whether a POD command's format name is `comment`.
fn is_comment_format(format: &[u8]) -> bool {
    format
        .strip_prefix(b"comment")
        .is_some_and(|after| after.first().is_none_or(u8::is_ascii_whitespace))
}

/// An LDoc block opener, per `lang.lua`: a line comment starting with
/// three or more dashes (`start_comment_ = '^%-%-%-+'`, so a line of
/// dashes alone opens too) unless it ends in a dash run preceded by a
/// non-dash (`end_comment_ = '[^%-]%-%-+[^-]*\n$'` — `--- x ---` is an
/// end marker), or a long comment whose body opens with a dash
/// (`block_comment = '^%-%-%[=*%[%-+'`).
#[must_use]
pub(crate) fn ldoc_opener(s: &[u8]) -> bool {
    if let Some(body) = lua_long_comment_body(s) {
        return body.first() == Some(&b'-');
    }
    s.starts_with(b"---") && !ldoc_end_marker(s)
}

/// A Lua long comment, `--[[ … ]]` or a levelled `--[==[ … ]==]`, as
/// opposed to a line comment that merely starts `--[`.
#[must_use]
pub(crate) fn lua_long_comment(s: &[u8]) -> bool {
    lua_long_comment_body(s).is_some()
}

/// The text after a long comment's opening bracket, or `None` when `s`
/// is not a long comment.
fn lua_long_comment_body(s: &[u8]) -> Option<&[u8]> {
    let rest = s.strip_prefix(b"--[")?;
    let levels = rest.iter().take_while(|b| **b == b'=').count();
    rest.get(levels..)?.strip_prefix(b"[")
}

/// `[^-]--+[^-]*$`: the line's final dash run has two or more dashes and
/// something other than a dash before it.
fn ldoc_end_marker(s: &[u8]) -> bool {
    let trimmed = s.len() - s.iter().rev().take_while(|b| **b != b'-').count();
    let dashes = s[..trimmed]
        .iter()
        .rev()
        .take_while(|b| **b == b'-')
        .count();
    dashes >= 2 && trimmed > dashes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruby_directive_opener_accepts_rdoc_and_yard_forms() {
        assert!(ruby_directive_opener(b"##"));
        assert!(ruby_directive_opener(b"## Section"));
        assert!(ruby_directive_opener(b"# @!method foo"));
        assert!(ruby_directive_opener(b"#\t@!macro bar"));
        assert!(!ruby_directive_opener(b"# plain"));
        assert!(!ruby_directive_opener(b"# @param x"));
        assert!(!ruby_directive_opener(b"#!/usr/bin/env ruby"));
        // A banner or divider: three or more `#` are neither RDoc's lone
        // `##` nor YARD's `##` prefix.
        assert!(!ruby_directive_opener(b"###"));
        assert!(!ruby_directive_opener(b"########################"));
        assert!(!ruby_directive_opener(b"##### Private helpers #####"));
    }

    #[test]
    fn javadoc_block_accepts_the_block_form_only() {
        assert!(javadoc_block(b"/** doc */"));
        assert!(javadoc_block(b"/**\n * doc\n */"));
        assert!(!javadoc_block(b"/**/"), "empty block");
        assert!(!javadoc_block(b"/*** banner */"), "three stars");
        assert!(!javadoc_block(b"/* plain */"));
        assert!(!javadoc_block(b"// plain"));
        assert!(!javadoc_block(b"/**"), "nothing after the marker");
    }

    #[test]
    fn triple_slash_rejects_a_fourth_slash() {
        assert!(triple_slash(b"/// doc"));
        assert!(triple_slash(b"///"), "bare marker still opens a doc line");
        assert!(triple_slash(b"///< after member"));
        assert!(!triple_slash(b"//// banner"));
        assert!(!triple_slash(b"// plain"));
    }

    #[test]
    fn doxygen_covers_the_four_markers_and_their_member_forms() {
        for doc in [
            &b"/** d */"[..],
            b"/*! d */",
            b"/// d",
            b"//! d",
            b"/**< d */",
            b"/*!< d */",
            b"///< d",
            b"//!< d",
        ] {
            assert!(doxygen(doc), "{}", String::from_utf8_lossy(doc));
        }
        for plain in [&b"/* d */"[..], b"// d", b"/**/", b"/*** d */", b"//// d"] {
            assert!(!doxygen(plain), "{}", String::from_utf8_lossy(plain));
        }
    }

    #[test]
    fn java_doc_follows_javac_on_three_stars_and_jep_467() {
        assert!(java_doc(b"/** d */"));
        assert!(java_doc(b"/*** d */"), "javac discards leading asterisks");
        assert!(java_doc(b"/// d"), "JEP 467 Markdown doc comment");
        assert!(!java_doc(b"/**/"));
        assert!(!java_doc(b"//// d"));
        assert!(!java_doc(b"/* d */"));
        assert!(!java_doc(b"// d"));
    }

    #[test]
    fn kotlin_doc_is_any_block_but_the_empty_one() {
        assert!(kotlin_doc(b"/** d */"));
        assert!(kotlin_doc(b"/*** d */"));
        assert!(!kotlin_doc(b"/**/"));
        assert!(!kotlin_doc(b"/// d"), "KDoc has no line form");
    }

    #[test]
    fn phpdoc_needs_whitespace_after_the_marker() {
        assert!(phpdoc(b"/** d */"));
        assert!(phpdoc(b"/**\n * d\n */"));
        assert!(!phpdoc(b"/**/"));
        assert!(!phpdoc(b"/*** d */"));
        assert!(!phpdoc(b"/**d */"));
        assert!(!phpdoc(b"# d"));
        assert!(!phpdoc(b"// d"));
    }

    #[test]
    fn csharp_doc_rejects_the_doxygen_only_spellings() {
        assert!(csharp_doc(b"/// <summary>d</summary>"));
        assert!(csharp_doc(b"/** d */"));
        for plain in [
            &b"//! d"[..],
            b"/*! d */",
            b"//// d",
            b"/**/",
            b"/*** d */",
            b"// d",
        ] {
            assert!(!csharp_doc(plain), "{}", String::from_utf8_lossy(plain));
        }
    }

    /// Each row is what `jsdoc -X` 4.x made of the comment above a call:
    /// a doclet of its own (virtual) or nothing. A name may sit on the
    /// line after its tag, and a tag inline in the description is text.
    #[test]
    fn jsdoc_virtual_follows_jsdoc_naming_tags() {
        crate::test_support::assert_classifies(
            jsdoc_virtual,
            &[
                b"/** @file */",
                b"/**\n * @fileoverview x\n */",
                b"/** @overview */",
                b"/** @module */",
                b"/**\n * @typedef {Object} T\n */",
                b"/**\n * @typedef {Object}\n * T\n */",
                b"/**\n * @function f\n */",
                b"/**\n * @callback cb\n */",
                b"/** @FILE */",
                b"/** @FileOverview */",
                b"/** @Typedef {Object} T */",
                b"/** @function f **/",
            ],
            &[
                b"/**\n * @license MIT\n */",
                b"/**\n * @typedef {Object}\n */",
                b"/**\n * @function\n */",
                b"/** @function **/",
                // A nested `{type}` is one type: no name follows it.
                b"/**\n * @typedef {{a: number}}\n */",
                b"/** x @file */",
                b"/** d */",
            ],
        );
        assert!(tsdoc_module(b"/**\n * @packageDocumentation\n */"));
        assert!(tsdoc_module(b"/**\n * @module m\n */"));
        assert!(!tsdoc_module(b"/**\n * @license MIT\n */"));
    }

    #[test]
    fn go_directive_matches_go_ast_is_directive() {
        for directive in [
            &b"//go:build linux"[..],
            b"//go:generate go run x",
            b"//line foo.go:12",
            b"//extern f",
            b"//export F",
            b"//nolint:errcheck",
            b"//x:1",
        ] {
            assert!(
                go_directive(directive),
                "{}",
                String::from_utf8_lossy(directive)
            );
        }
        for text in [
            &b"// go:build linux"[..],
            b"//Note: not a directive",
            b"//x:",
            b"//exported F",
            b"// d",
        ] {
            assert!(!go_directive(text), "{}", String::from_utf8_lossy(text));
        }
    }

    /// A block is a comment only while nothing but `comment` regions fill
    /// it: POD after a region's end (`=head1 NAME` past a `=for comment`
    /// paragraph or an `=end comment`) is documentation pod2text renders.
    #[test]
    fn pod_comment_region_is_the_comment_format_only() {
        for region in [
            &b"=begin comment\n\nold();\n\n=end comment\n\n=cut"[..],
            b"=for comment\nold();\n\n=cut",
            b"=begin  comment\n=end comment\n=cut",
            b"=for comment\nold();\n",
            b"=begin comment\nold();\n",
            b"=for comment a\n\n=for comment b\n\n=cut",
            b"=begin comment\nx\n=end comment\n=for comment y\n\n=cut",
            b"=for comment\r\nold();\r\n\r\n=cut\r\n",
        ] {
            assert!(
                pod_comment_region(region),
                "{}",
                String::from_utf8_lossy(region)
            );
        }
        for doc in [
            &b"=pod\n\nText.\n\n=cut"[..],
            b"=head1 NAME\n\nFoo\n\n=cut",
            b"=begin html\n\n<p>x</p>\n\n=end html\n\n=cut",
            b"=begin comments\n=end comments\n=cut",
            b"=beginning",
            b"=for comment\nInternal note.\n\n=head1 NAME\n\nFoo - bar\n\n=cut",
            b"=begin comment\n\nold();\n\n=end comment\n\n=head1 X\n\nDoc.\n\n=cut",
            b"=for comment\nnote\n\nPlain text paragraph.\n\n=cut",
        ] {
            assert!(!pod_comment_region(doc), "{}", String::from_utf8_lossy(doc));
        }
    }

    /// The match is on the whole comment, because it is applied anywhere
    /// in the file: a documentation row that mentions an encoding, or
    /// names `frozen_string_literal` in prose, is not a magic line.
    #[test]
    fn ruby_magic_comment_matches_whole_magic_lines_only() {
        for magic in [
            &b"#!/usr/bin/env ruby"[..],
            b"# coding: utf-8",
            b"# encoding: UTF-8",
            b"# Encoding: binary",
            b"#coding=utf-8",
            b"# -*- coding: utf-8 -*-",
            b"# -*- mode: ruby; encoding: utf-8 -*-",
            b"# -*- frozen_string_literal: true -*-",
            b"# vim: set fileencoding=utf-8 :",
            b"# frozen_string_literal: true",
            b"# frozen-string-literal: false",
        ] {
            assert!(
                ruby_magic_comment(magic),
                "{}",
                String::from_utf8_lossy(magic)
            );
        }
        for doc in [
            &b"# Encoding: defaults to UTF-8."[..],
            b"# Handles transcoding: see below",
            b"# Pass `frozen_string_literal: true` to opt in.",
            b"# Sets the encoding= attribute",
            b"# d",
            b"#",
        ] {
            assert!(!ruby_magic_comment(doc), "{}", String::from_utf8_lossy(doc));
        }
    }
}
