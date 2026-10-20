//! Doc-comment recognition by bytes, for the grammars that emit one
//! comment kind for every comment form (#1597).
//!
//! Rust (a `doc_comment` child), Groovy (`groovydoc_comment`) and
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

use crate::checker::Checker;
use crate::node::{Ancestors, FoldLane, Node};

/// How a language's tool attaches a doc comment to the declaration after
/// it (#1613), read from each tool's source rather than its manual
/// (lesson 98); see [`Checker::DOC_BINDING`]. A forward doc comment that
/// the binding does not attach documents nothing and is billed as a plain
/// comment.
///
/// The doc comments compared are those of one *stretch*: the comment
/// siblings between two other siblings, read past the ones
/// [`skips`](Self::skips) names. Blank rows never split a stretch; none of
/// the bound tools reads them.
#[derive(Clone, Copy, Debug)]
pub struct DocBinding {
    /// Which doc comments of a stretch the tool attaches.
    pub kept: KeptDocs,
    /// A sibling that may sit between a doc comment and its target
    /// without detaching it, so a stretch reads past it: rustc gives an
    /// item every outer doc comment above it, through its other
    /// attributes. A grammar that nests annotations inside the declaration
    /// has nothing to skip. A skipped node is never a target. The
    /// [`Ancestors`] are the node's chain.
    pub skips: for<'a, 'b> fn(&Node<'a>, Ancestors<'a, 'b>) -> bool,
    /// Whether a doc comment documents what follows it. `false` leaves it
    /// to where it sits, as in an unbound language: Rust's inner `//!`
    /// documents the item enclosing it, a doc comment in a macro token
    /// tree documents whatever the expansion makes of it, and a KDoc
    /// inside a Kotlin declaration's signature is read by `findDocComment`
    /// from there. The [`Ancestors`] are the comment's chain.
    pub forward: for<'a, 'b> fn(&Node<'a>, &[u8], Ancestors<'a, 'b>) -> bool,
    /// A doc comment that documents an entry of its own and takes with it
    /// the forward doc comments before it in its stretch, so they document
    /// even when the stretch ends at no target: Doxygen gives a `\file`
    /// block the doc blocks it has gathered since the last entry, so a
    /// licence block above one at the end of a header documents the file.
    /// It should not itself bind [`forward`](Self::forward). Where the
    /// stretch does end at a target, [`kept`](Self::kept) alone decides.
    /// `None` for a tool with no such block, which lets a stretch that
    /// ends at no target return before it is even collected.
    pub gathers: Option<fn(&Node, &[u8]) -> bool>,
}

impl DocBinding {
    /// A binding that keeps `kept`, skips no sibling, binds every doc
    /// comment forward, and has no doc comment gather the others.
    #[must_use]
    pub const fn new(kept: KeptDocs) -> Self {
        Self {
            kept,
            skips: |_, _| false,
            forward: |_, _, _| true,
            gathers: None,
        }
    }
}

/// Which doc comments of a stretch a [`DocBinding`] attaches.
#[derive(Clone, Copy, Debug)]
pub enum KeptDocs {
    /// Every doc comment of the stretch: rustc gives an item all the outer
    /// doc comments above it, across blank rows, plain comments and other
    /// attributes, which rustdoc concatenates.
    All,
    /// The last doc comment before the declaration's first token, and the
    /// doc comments before it that `joins(earlier, later, code)` makes one
    /// comment with it: javac's `Token::docComment` and Kotlin's
    /// `PrecedingDocCommentsBinder` keep the last, and javac's
    /// dangling-doc-comments lint names the rest. A skipped node is that
    /// first token, a Kotlin annotation the grammar leaves outside its
    /// declaration; when no doc comment precedes it, the first one after
    /// it is read, as `findDocComment` reads Kotlin's modifier list.
    Last {
        /// Whether two adjacent doc comments are one to the tool.
        joins: fn(&Node, &Node, &[u8]) -> bool,
    },
    /// The stretch's last comment, if it is a doc comment: Groovy's
    /// `GroovydocManager` reads the comment nearest the declaration and
    /// drops it unless it opens `/**`, so a plain comment in between
    /// detaches it. A comment for which `hidden(comment, next)` holds,
    /// `next` being the sibling after it, is invisible to the tool and
    /// read past.
    Nearest {
        /// Whether the tool cannot see a comment.
        hidden: fn(&Node, &Node) -> bool,
    },
    /// What a reverse scan over the declaration's leading trivia keeps,
    /// from its first token back: past the comments and skipped nodes
    /// `hidden(node, code)` names to the nearest other comment, kept if it
    /// is a doc comment, then on over the doc comments directly before it
    /// that `joins(earlier, later, code)` makes one with it. A member
    /// directly before them that is hidden or plain ends the run. A skipped
    /// node `hidden` does not name is the first token — a decorator the
    /// grammar leaves outside its declaration — so a doc comment after it
    /// documents nothing. Roslyn's `GetDocumentationCommentTriviaFromSyntaxNode`
    /// reads past plain comments and directives and keeps adjacent doc
    /// comments; JSDoc's `getLeadingJsdocComment` reads past line comments
    /// only; tsc reads past every plain comment.
    Leading {
        /// Whether the scan reads past a member to the one before it.
        hidden: fn(&Node, &[u8]) -> bool,
        /// Whether two adjacent doc comments are one to the tool.
        joins: fn(&Node, &Node, &[u8]) -> bool,
        /// Whether the tool reads a member on the row where the code
        /// before the stretch ends, asked of the target. tsc's
        /// `getLeadingCommentRanges` starts at the line break after the
        /// token before the declaration, so `x; /** d */` documents
        /// nothing after it, except before a parameter, whose trailing
        /// comment ranges it reads too; Roslyn and JSDoc read it.
        after_code: fn(&Node) -> bool,
    },
}

/// A [`KeptDocs::Last`] join that joins nothing.
#[must_use]
pub(crate) fn never_joins(_: &Node, _: &Node, _: &[u8]) -> bool {
    false
}

/// The bytes of `node`'s own span, or nothing when the span is outside
/// `code` — a recovery token can point past the source. Used for comment
/// markers and for the identifiers the positional rules read.
#[inline]
#[must_use]
pub(crate) fn node_bytes<'a>(node: &Node, code: &'a [u8]) -> &'a [u8] {
    code.get(node.start_byte()..node.end_byte()).unwrap_or(&[])
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

/// Whether `comment` lies outside every function and closure body, by the
/// same fold [`documents_where_it_sits`] runs, for a language whose
/// in-body doc comments document wherever they sit and so must not bind.
#[must_use]
pub(crate) fn outside_function_bodies<'a, T: Checker>(
    comment: &Node<'a>,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    !held_in_body::<T>(comment, 0, ancestors)
}

/// Whether an ancestor of `node`, past the `skip` nearest ones, holds it
/// in a function body. One step closure for the shared
/// [`FoldLane::DocInBody`] memo: `fold_down` serves a cached answer to
/// every caller on the lane, so the question must be asked the same way
/// everywhere.
fn held_in_body<'a, T: Checker>(
    node: &Node<'a>,
    skip: usize,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    ancestors.fold_down(
        node,
        skip,
        FoldLane::DocInBody,
        0,
        |held, ancestor, outer, below| {
            u8::from(held != 0 || T::holds_doc_comment_in_body(ancestor, outer, below))
        },
    ) != 0
}

// ---------------------------------------------------------------------
// Documentation by position: Go, Ruby and Lua have no doc marker the
// grammar or a tool reads per comment. go/doc's lead comment is "a
// comment group that starts and ends in a line without any other
// tokens and that is followed by a non-comment token on the line
// immediately after" (go/parser); YARD attaches the comment lines
// directly above a `def`, `class` or `module`; LDoc opens a block on
// `---` and continues it over the `--` rows that follow until a blank
// line. All three are properties of a *run* of comment siblings, not of
// one node, so they are decided once per run, at its last member.
// ---------------------------------------------------------------------

/// The place half of the documentation test, for a comment already
/// spelled or placed as documentation: it documents unless an ancestor
/// holds it in a function or closure body
/// ([`Checker::holds_doc_comment_in_body`]) without the comment opening
/// that body as its documentation ([`Checker::doc_comment_opens_body`],
/// asked of the two nearest ancestors only) and the language's tool
/// ignores in-body doc comments
/// ([`Checker::DOC_COMMENTS_IN_FUNCTION_BODIES`]). Shared by the marker
/// languages' comment arms and by Lua's LDoc opener (#1597).
#[must_use]
pub fn documents_where_it_sits<'a, T: Checker>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    if T::DOC_COMMENTS_IN_FUNCTION_BODIES {
        return true;
    }
    // The two nearest ancestors may excuse the comment itself
    // (`Checker::doc_comment_opens_body`: Rust's inner doc heading a
    // body); every ancestor above them answers by the path alone, folded
    // once per walk rather than climbed once per comment.
    let mut below = *node;
    for (ancestor, outer) in ancestors.iter(node).take(2) {
        if T::holds_doc_comment_in_body(&ancestor, outer, &below)
            && !T::doc_comment_opens_body(node, &ancestor, &below, code)
        {
            return false;
        }
        below = ancestor;
    }
    !held_in_body::<T>(node, 2, ancestors)
}

/// Whether a doc comment documents without a following declaration,
/// before [`documents_where_it_sits`] has its say: every doc comment of a
/// language with no [`Checker::DOC_BINDING`], and a bound language's that
/// does not attach forward ([`DocBinding::forward`]). A forward one
/// documents only through [`bound_doc_run`] (#1613).
#[must_use]
pub fn documents_unbound<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    T::DOC_BINDING.is_none_or(|binding| !(binding.forward)(comment, code, ancestors))
}

/// Credits the doc comments a bound language's tool attaches to the
/// declaration after them (#1613). Called from the LOC comment arm for
/// every comment; `credit` receives each attached doc comment, whose own
/// arm has already billed its rows as `cloc`. A language with no
/// [`Checker::DOC_BINDING`] credits nothing here.
///
/// The comments compared are one *stretch*: the comment siblings between
/// two other siblings, read past those [`DocBinding::skips`] names and any
/// parse error that is no doc target and holds none. Only the stretch's last
/// comment does more than one sibling step, so a stretch of `k` members
/// costs `O(k)` steps in total, the #1446 bound: it finds the sibling the
/// stretch ends at and, when that is an [`Checker::is_doc_target`], walks
/// back once to credit what the binding keeps. A stretch that ends
/// anywhere else — a statement, a macro call, a closing brace, the end of
/// the file — documents nothing. Members are siblings, so one in-body test
/// ([`documents_where_it_sits`]) answers for them all.
pub fn bound_doc_run<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    mut credit: impl FnMut(&Node<'a>),
) {
    let Some(binding) = T::DOC_BINDING else {
        return;
    };
    let end = match stretch_end::<T>(comment, code, ancestors, binding.skips) {
        // A later member decides.
        StretchEnd::Undecided => return,
        // Nothing documents, and no block gathers the stretch either.
        StretchEnd::Elsewhere if binding.gathers.is_none() => return,
        end => end,
    };
    if !documents_where_it_sits::<T>(comment, code, ancestors) {
        return;
    }
    let (stretch, code_before) = collect_stretch::<T>(comment, code, ancestors, binding.skips);
    let forward =
        |n: &Node<'a>| T::is_doc_comment(n, code) && (binding.forward)(n, code, ancestors);
    let StretchEnd::AtTarget { next, target } = end else {
        if let Some(gathers) = binding.gathers {
            bind_gathered::<T>(&stretch, code, gathers, forward)
                .iter()
                .for_each(&mut credit);
        }
        return;
    };
    let kept = match binding.kept {
        KeptDocs::All => stretch.iter().copied().filter(forward).collect(),
        KeptDocs::Last { joins } => bind_last::<T>(&stretch, code, joins, forward),
        KeptDocs::Nearest { hidden } => bind_nearest::<T>(&stretch, next, hidden, forward)
            .into_iter()
            .collect(),
        KeptDocs::Leading {
            hidden,
            joins,
            after_code,
        } => {
            // A member on the row the code before the stretch ends on is
            // after that code, where a scan from the next line break does
            // not reach.
            let after_code_row = code_before
                .filter(|_| !after_code(&target))
                .map(|before| before.end_row());
            let attaches = |n: &Node<'a>| forward(n) && Some(n.start_row()) != after_code_row;
            bind_leading::<T>(&stretch, code, hidden, joins, attaches)
        }
    };
    kept.iter().for_each(&mut credit);
}

/// Where a stretch of comments ends, asked of its last comment.
enum StretchEnd<'a> {
    /// A later comment of the stretch decides.
    Undecided,
    /// At `target`, an [`Checker::is_doc_target`] or a parse error holding
    /// one. `next` is the sibling right after the asking comment, where a
    /// walk back from the target begins.
    AtTarget { next: Node<'a>, target: Node<'a> },
    /// At a statement, a closing brace, the end of the file — anything
    /// that documents nothing.
    Elsewhere,
}

/// Where the stretch `comment` belongs to ends. The scan stops at the
/// first sibling that is neither skipped nor a parse error read past, so
/// it never passes a later comment of the stretch. Only an error is asked
/// whether it is read past, and only once the binding's own `skips` and
/// [`Checker::is_doc_target`] have passed on it, so a binding that decides
/// errors itself (Doxygen's) keeps its reading.
fn stretch_end<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    skips: for<'b> fn(&Node<'a>, Ancestors<'a, 'b>) -> bool,
) -> StretchEnd<'a> {
    let mut after = ancestors.siblings_after(comment).peekable();
    let Some(&next) = after.peek() else {
        return StretchEnd::Elsewhere;
    };
    match after.find(|n| !skips(n, ancestors) && !read_past_error::<T>(n, code, ancestors)) {
        Some(target) if T::is_comment(&target) => StretchEnd::Undecided,
        Some(target)
            if T::is_doc_target(&target, code, ancestors)
                || (target.is_error() && error_holds_target::<T>(&target, code, ancestors)) =>
        {
            StretchEnd::AtTarget { next, target }
        }
        _ => StretchEnd::Elsewhere,
    }
}

/// The stretch `comment` ends, in source order, and the sibling before it:
/// `comment` and the siblings before it that are comments or skipped,
/// read past the parse errors [`stretch_end`] reads past with the same
/// test, so every member whose scan stopped at the next comment is in
/// the stretch that decides it. The errors themselves are left out: the
/// `safe` of a `safe fn` is between the item's doc comments and its
/// first token, not that token. An error the scan does not read past —
/// one holding a target — ends the stretch and is the sibling before it.
fn collect_stretch<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    skips: for<'b> fn(&Node<'a>, Ancestors<'a, 'b>) -> bool,
) -> (Vec<Node<'a>>, Option<Node<'a>>) {
    let mut stretch = vec![*comment];
    let mut before = None;
    // An error read past is no member of the stretch: neither a comment
    // nor the first token a binder stops at. Left in, a binder that keeps
    // the last doc comment before the first token (tsc's, JSDoc's,
    // javac's) would read the error as that token and keep the comment
    // above it instead of the one the tool attaches.
    for sibling in ancestors
        .siblings_before(comment)
        .filter(|sibling| !read_past_error::<T>(sibling, code, ancestors))
    {
        if T::is_comment(&sibling) || skips(&sibling, ancestors) {
            stretch.push(sibling);
        } else {
            before = Some(sibling);
            break;
        }
    }
    stretch.reverse();
    (stretch, before)
}

/// Whether a stretch reads past `node`: a parse error that is no doc
/// target and holds none — tokens the grammar could not place, such as the
/// `safe` of a Rust 2024 foreign item, which tree-sitter-rust leaves as an
/// error between the item's doc comment and the rest of it. Given the
/// benefit of the doubt, as Doxygen's binding gives one (`doxygen_skips`),
/// rather than dropping the doc comment on a parse failure.
fn read_past_error<'a, T: Checker>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    node.is_error()
        && !T::is_doc_target(node, code, ancestors)
        && !error_holds_target::<T>(node, code, ancestors)
}

/// Whether a child of the parse error `error` is a doc target: a
/// declaration parsed amid code the grammar could not place, which the
/// error then stands in for as Doxygen's `doxygen_target` reads one.
/// `ancestors` is the error's chain. Errors are rare, so the copy of the
/// chain its children are judged with costs nothing on the common path.
/// Without a chain the children are judged with none, which a target
/// test that climbs pays for at `O(depth)`; every production walk lends
/// one, so only a test reaches that fallback.
fn error_holds_target<'a, T: Checker>(
    error: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let chain = ancestors.children_chain(error);
    let inside = chain
        .as_deref()
        .map_or_else(Ancestors::unknown, Ancestors::known);
    error
        .children()
        .any(|child| child.is_named() && T::is_doc_target(&child, code, inside))
}

/// What a stretch that ends at no target documents: what its last
/// gathering comment ([`DocBinding::gathers`]) takes — the forward doc
/// comments before it, and the line doc comments on the rows directly
/// below it, which a line-comment tool reads as one block with it. The
/// gathering comment does not bind forward, so `forward` drops it; it
/// documents where it sits.
fn bind_gathered<'a, T: Checker>(
    stretch: &[Node<'a>],
    code: &[u8],
    gathers: fn(&Node, &[u8]) -> bool,
    forward: impl Fn(&Node<'a>) -> bool,
) -> Vec<Node<'a>> {
    let Some(gatherer) = stretch
        .iter()
        .rposition(|n| T::is_doc_comment(n, code) && gathers(n, code))
    else {
        return Vec::new();
    };
    let line = |n: &Node| node_bytes(n, code).starts_with(b"//");
    // Only a line-comment gatherer reads the line doc comments below it
    // as one block with itself.
    let gathers_below = line(&stretch[gatherer]);
    let mut last = gatherer;
    while let Some(below) = stretch.get(last + 1)
        && gathers_below
        && line(below)
        && T::is_doc_comment(below, code)
        && below.start_row() == stretch[last].end_row().saturating_add(1)
    {
        last += 1;
    }
    stretch[..=last].iter().copied().filter(forward).collect()
}

/// [`KeptDocs::Last`] over a stretch in source order: the last doc
/// comment before the first skipped node — the declaration's first token —
/// with the doc comments it joins, or else the first doc comment after it.
fn bind_last<'a, T: Checker>(
    stretch: &[Node<'a>],
    code: &[u8],
    joins: fn(&Node, &Node, &[u8]) -> bool,
    forward: impl Fn(&Node<'a>) -> bool,
) -> Vec<Node<'a>> {
    let split = stretch
        .iter()
        .position(|n| !T::is_comment(n))
        .unwrap_or(stretch.len());
    let (before, after) = stretch.split_at(split);
    let Some(last) = before.iter().rposition(&forward) else {
        return after.iter().copied().find(forward).into_iter().collect();
    };
    joined_run(before, last, code, joins, forward)
}

/// [`KeptDocs::Leading`] over a stretch in source order: the members
/// before the first token, read back past the hidden ones to the nearest
/// other one, kept if it is a doc comment that binds forward, with the doc
/// comments directly before it that it joins.
fn bind_leading<'a, T: Checker>(
    stretch: &[Node<'a>],
    code: &[u8],
    hidden: fn(&Node, &[u8]) -> bool,
    joins: fn(&Node, &Node, &[u8]) -> bool,
    forward: impl Fn(&Node<'a>) -> bool,
) -> Vec<Node<'a>> {
    let first_token = stretch
        .iter()
        .position(|n| !T::is_comment(n) && !hidden(n, code))
        .unwrap_or(stretch.len());
    let before = &stretch[..first_token];
    let Some(last) = before.iter().rposition(|n| !hidden(n, code)) else {
        return Vec::new();
    };
    if !forward(&before[last]) {
        return Vec::new();
    }
    joined_run(before, last, code, joins, forward)
}

/// `before[last]` with the doc comments directly before it that it
/// joins, in source order. `joins` is asked first: for a binding whose
/// `joins` is [`never_joins`] it answers at once, where `forward`
/// would scan the comment's bytes for nothing.
fn joined_run<'a>(
    before: &[Node<'a>],
    last: usize,
    code: &[u8],
    joins: fn(&Node, &Node, &[u8]) -> bool,
    forward: impl Fn(&Node<'a>) -> bool,
) -> Vec<Node<'a>> {
    let mut first = last;
    while first > 0
        && joins(&before[first - 1], &before[first], code)
        && forward(&before[first - 1])
    {
        first -= 1;
    }
    before[first..=last].to_vec()
}

/// [`KeptDocs::Nearest`] over a stretch in source order whose last
/// member `next` follows: the comment nearest the declaration that the
/// tool can see, if it is a doc comment.
fn bind_nearest<'a, T: Checker>(
    stretch: &[Node<'a>],
    next: Node<'a>,
    hidden: fn(&Node, &Node) -> bool,
    forward: impl Fn(&Node<'a>) -> bool,
) -> Option<Node<'a>> {
    let mut later = next;
    for member in stretch.iter().rev() {
        if T::is_comment(member) && !hidden(member, &later) {
            return Some(*member).filter(forward);
        }
        later = *member;
    }
    None
}

/// Credits the rows of the comment run that `comment` ends, when the
/// run is documentation: it sits directly above a node the language's
/// [`Checker::is_doc_target`] accepts, or its first member opens a
/// block per [`Checker::is_doc_run_opener`]. Called from a language's
/// LOC comment arm for every comment; only the run's last member does
/// any work beyond one sibling step, so a run of `k` rows costs `O(k)`
/// sibling steps in total (#1597).
///
/// A run is the maximal chain of comment siblings on consecutive rows
/// that hold nothing but comments; a trailing comment (`x := 1 // t`)
/// is never a member and never seeds one, which the previous-sibling
/// test decides — the `ploc` row set cannot, because code that opened a
/// child space (`func A() {} // t`) lives in the child's stats. A blank
/// row ends the run, as does anything that is not a comment; a comment
/// the language says closes one ([`Checker::ends_doc_run`]) is its last
/// member. The decision needs the row after the run, so it is made at
/// the last member, which then walks back to credit the whole run through
/// `credit(first_row, last_row)` per member, skipping the members
/// [`Checker::is_doc_run_directive`] names (`//go:generate`, a Ruby
/// shebang) — part of the group, not of the documentation.
pub fn leading_doc_run<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    mut credit: impl FnMut(usize, usize),
) {
    let next = ancestors.siblings_after(comment).next();
    if next
        .as_ref()
        .is_some_and(|n| run_continues_to::<T>(comment, n, code))
    {
        // A later member decides.
        return;
    }
    let Some(first) = run_first_member::<T>(comment, code, ancestors) else {
        return;
    };
    // `next` is `comment`'s sibling, so `ancestors` is its chain too.
    let below = next.filter(|n| T::doc_target_start_row(n) == comment.end_row().saturating_add(1));
    let documents = below.is_some_and(|n| T::is_doc_target(&n, code, ancestors))
        || T::is_doc_run_opener(&first, below.as_ref(), code, ancestors);
    if !documents {
        return;
    }
    for member in std::iter::once(first).chain(ancestors.siblings_after(&first)) {
        if member.start_byte() > comment.start_byte() {
            break;
        }
        if !T::is_doc_run_directive(&member, code) {
            credit(member.start_row(), member.end_row());
        }
    }
}

/// Whether `next`, the sibling after `comment`, is a later member of
/// the same run: a comment on `comment`'s own row, or on the next row
/// when `comment` does not end the run ([`Checker::ends_doc_run`]).
/// Only `next` is tested as a comment, so `comment` must already be
/// one: asked about a code node, it reports that the comment on the row
/// below continues the code's run, which once let a backward scan run
/// to the top of the file for every LDoc block (#1597).
pub(crate) fn run_continues_to<T: Checker>(comment: &Node, next: &Node, code: &[u8]) -> bool {
    T::is_comment(next)
        && (next.start_row() <= comment.end_row()
            || (next.start_row() == comment.end_row().saturating_add(1)
                && !T::ends_doc_run(comment, code)))
}

/// The first member of the run `comment` ends: the comment siblings
/// before it on consecutive rows that hold nothing but comments, so two
/// comments sharing a row (`/* a */ /* b */`) are one row of the run.
/// `None` when code precedes `comment` on its own row: a trailing
/// comment belongs to no run, and a row where code precedes its
/// comments ends the run below it. One backward pass over one sibling
/// iterator.
fn run_first_member<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    // `row_first` is the earliest comment found on the row being scanned;
    // `first` is the first member of the earliest row known to be clean.
    // Each step moves `row_first` to the sibling just before it, so one
    // iterator walks the whole run.
    let mut before = ancestors.siblings_before(comment);
    let mut first = None;
    let mut row_first = *comment;
    loop {
        match before.next() {
            // Another token on this row: a comment extends the row, and
            // code before it makes every comment on the row a trailing one.
            Some(prev) if prev.end_row() >= row_first.start_row() => {
                if !T::is_comment(&prev) {
                    return first;
                }
                row_first = prev;
            }
            // A comment ending on the row above: this row is clean, and
            // the run continues upward unless that comment ends a run.
            Some(prev)
                if T::is_comment(&prev)
                    && prev.end_row().saturating_add(1) == row_first.start_row()
                    && !T::ends_doc_run(&prev, code) =>
            {
                first = Some(row_first);
                row_first = prev;
            }
            _ => return Some(row_first),
        }
    }
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

    /// Every language's `DOC_BINDING` spreads `..DocBinding::new(kept)`
    /// and relies on its defaults: nothing skipped, every doc comment
    /// binding forward, no gathering comment. Each const is evaluated at
    /// compile time, so this is the one runtime call that pins them.
    #[cfg(feature = "rust")]
    #[test]
    fn doc_binding_defaults_skip_nothing_and_bind_everything_forward() {
        use crate::langs::RustCode;
        use crate::node::Tree;
        let source = b"/// d\nfn f() {}\n";
        let tree = Tree::new::<RustCode>(source);
        let comment = tree
            .get_root()
            .preorder()
            .find(|node| RustCode::is_comment(node))
            .expect("the fixture has a comment");
        let binding = DocBinding::new(KeptDocs::All);
        assert!(matches!(binding.kept, KeptDocs::All));
        assert!(!(binding.skips)(&comment, Ancestors::unknown()));
        assert!((binding.forward)(&comment, source, Ancestors::unknown()));
        assert!(binding.gathers.is_none());
    }

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

    /// The walk back from a stretch's last comment reads past the parse
    /// errors the scan forward from each member read past, so a member
    /// whose scan stopped at the next comment is decided by the stretch
    /// holding it. Not valid Rust — no qualifier may precede a doc comment
    /// — but an error between two doc comments needs one, and a binding
    /// that keeps every doc comment is where an undecided member shows. The
    /// test pins that the two scans agree, not a reading of Rust.
    #[cfg(feature = "rust")]
    #[test]
    fn both_scans_read_past_the_same_error() {
        use crate::langs::RustCode;
        use crate::node::Tree;
        let source = b"unsafe extern \"C\" {\n    /// a\n    safe\n    /// b\n    fn g();\n}\n";
        let tree = Tree::new::<RustCode>(source);
        let comments: Vec<_> = tree
            .get_root()
            .preorder()
            .filter(|node| RustCode::is_comment(node))
            .collect();
        assert_eq!(comments.len(), 2);
        assert!(
            tree.get_root().preorder().any(|node| node.is_error()),
            "the fixture no longer parses `safe` as an error"
        );
        let mut credited = Vec::new();
        for comment in &comments {
            bound_doc_run::<RustCode>(comment, source, Ancestors::unknown(), |doc| {
                credited.push(doc.start_row());
            });
        }
        assert_eq!(credited, [1, 3]);
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
