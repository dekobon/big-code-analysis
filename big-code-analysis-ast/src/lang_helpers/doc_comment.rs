//! Doc-comment recognition by bytes, for the grammars that emit one
//! comment kind for every comment form (#1597).
//!
//! Rust (a `doc_comment` child), Groovy (`groovydoc_comment`) and
//! Perl (`pod_statement`) mark documentation structurally. Every other
//! grammar parses `/** … */` or `///` to the same `comment` node
//! as a plain comment, so the opening marker is the only evidence, and
//! each language's own tool decides which markers count: Doxygen, javac,
//! Kotlin's lexer, JSDoc, PHP's scanner and Roslyn disagree at the
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

use crate::checker::Checker;
use crate::node::{Ancestors, FoldLane, Node};

/// The bytes of `node`'s own span, or nothing when the span is outside
/// `code` — a recovery token can point past the source. Used for comment
/// markers and for the identifiers the positional rules read.
#[inline]
#[must_use]
pub(crate) fn node_bytes<'a>(node: &Node, code: &'a [u8]) -> &'a [u8] {
    code.get(node.start_byte()..node.end_byte()).unwrap_or(&[])
}

/// Whether `inner`'s byte span lies within `outer`'s.
#[inline]
#[must_use]
pub(crate) fn spans(outer: &Node, inner: &Node) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}

/// [`Checker::holds_doc_comment_in_body`] for a language whose tool reads
/// doc comments in a function's signature and whose function and closure
/// kinds name their body in a `body` field: only that node holds a comment
/// in-body, and a bodiless declaration holds none. `below` is the node
/// under `ancestor` on the comment's path, so the body holds the comment
/// exactly when it spans `below` (#1597).
#[inline]
#[must_use]
pub(crate) fn holds_in_body_field<'a, T: Checker>(
    ancestor: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
    below: &Node<'a>,
) -> bool {
    (T::is_func(ancestor, ancestors) || T::is_closure(ancestor, ancestors))
        && ancestor
            .child_by_field_name("body")
            .is_some_and(|body| spans(&body, below))
}

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

/// The place half of the documentation test, for a comment already
/// spelled or placed as documentation: it documents unless an ancestor
/// holds it in a function or closure body
/// ([`Checker::holds_doc_comment_in_body`]) and the language's tool
/// ignores in-body doc comments
/// ([`Checker::DOC_COMMENTS_IN_FUNCTION_BODIES`]). Shared by the marker
/// languages' comment arms (#1597).
#[must_use]
pub fn documents_where_it_sits<'a, T: Checker>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    if T::DOC_COMMENTS_IN_FUNCTION_BODIES {
        return true;
    }
    // The two nearest ancestors are asked with the comment itself (Rust's
    // inner doc heading a body is decided there); every ancestor above
    // them answers by the path alone, folded once per walk rather than
    // climbed once per comment.
    let mut below = *node;
    for (ancestor, outer) in ancestors.iter(node).take(2) {
        if T::holds_doc_comment_in_body(&ancestor, outer, &below, Some(node), code) {
            return false;
        }
        below = ancestor;
    }
    ancestors.fold_down(
        node,
        2,
        FoldLane::DocInBody,
        0,
        |held, ancestor, outer, below| {
            u8::from(held != 0 || T::holds_doc_comment_in_body(ancestor, outer, below, None, code))
        },
    ) == 0
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
