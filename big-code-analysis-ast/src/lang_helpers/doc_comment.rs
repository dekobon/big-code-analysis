//! Doc-comment binding: which comments document, by where they sit and
//! by the declaration after them (#1597, #1613). The per-tool marker
//! tests these rules consult live in [`super::doc_text`] and are
//! re-exported here, so a `Checker` or role row names one path for both
//! halves (#1649).

pub(crate) use super::doc_text::{
    csharp_doc, doxygen, go_directive, java_doc, javadoc_block, jsdoc_virtual, kotlin_doc,
    ldoc_opener, lua_long_comment, phpdoc, pod_comment_region, ruby_directive_opener,
    ruby_magic_comment, tsdoc_block, tsdoc_module,
};
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
/// `ancestors` is the error's chain, and its children are judged with
/// [`Ancestors::of_children`] of it, which borrows the walk's chain and
/// carries the walk's memo, so Doxygen's conditionals under the error
/// are read once per walk like any other (#1686 review). Without a
/// chain the children are judged with none, which a target test that
/// climbs pays for at `O(depth)`; every production walk lends one, so
/// only a test reaches that fallback.
fn error_holds_target<'a, T: Checker>(
    error: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    let under = ancestors.of_children(error);
    let inside = under.get();
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

/// The trailing comment that heads `first`'s LDoc group, when `first` sits
/// directly under a row that ends in one: LDoc lexes a line comment with
/// its newline and gathers every consecutive comment token into one group
/// whose first token decides whether it is an LDoc block (`parse.lua`), so
/// a `---` row under `x = 1 -- note` belongs to that `--` group. The run
/// `leading_doc_run` hands over leaves such a comment out, as go/doc does,
/// so it is looked up here.
pub(crate) fn lua_group_head_above<'a, T: Checker>(
    first: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> Option<Node<'a>> {
    ancestors
        .siblings_before(first)
        .next()
        .filter(|above| T::is_comment(above) && run_continues_to::<T>(above, first, code))
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

    /// A wide parse `ERROR` holding a `package` — whose target test asks
    /// for a *sibling* — is judged on a derived chain that lends no
    /// `SiblingCache`, so the lookup seeks rather than asking a cache
    /// that never recorded the error's children. Carrying the walk's
    /// sibling cache onto the derived chain tripped the walk-must-record
    /// assertion in `Ancestors::lookup` on this exact shape (#1686
    /// review): a debug-build panic on malformed Java, where a release
    /// build answered quietly. Discriminates only through that
    /// `debug_assert!`, so it is meaningful under `make pre-commit`'s
    /// debug build and vacuous under `--release` (lesson #80).
    #[cfg(feature = "java")]
    #[test]
    fn a_wide_error_s_package_is_judged_without_the_walk_s_sibling_cache() {
        use crate::langs::JavaCode;
        use crate::node::{FoldCache, SiblingCache, Tree, WIDE_PARENT};
        // `class {` opens a body the stray `@`s never close, so recovery
        // gathers the package and every `@` into one flat `ERROR`.
        let source = format!(
            "/** d */\nclass {{\npackage a;\n{}",
            "@ \n".repeat(WIDE_PARENT + 40)
        );
        let tree = Tree::new::<JavaCode>(source.as_bytes());
        let root = tree.get_root();
        let error = root
            .children()
            .find(Node::is_error)
            .expect("recovery leaves an ERROR under the root");
        assert!(
            error.child_count() > WIDE_PARENT,
            "the ERROR must be wide, or the assertion this guards cannot fire"
        );
        assert!(
            error
                .children()
                .any(|child| child.kind() == "package_declaration"),
            "the ERROR must hold the package, whose target test seeks a sibling"
        );

        // What the walk lends: the root recorded, the ERROR not yet.
        let chain = [root];
        let mut siblings = SiblingCache::default();
        siblings.record(&root, 0, root.children());
        let folds = FoldCache::default();
        let walk = Ancestors::known(&chain)
            .with_siblings(&siblings)
            .with_folds(&folds);
        assert!(
            error_holds_target::<JavaCode>(&error, source.as_bytes(), walk),
            "a package with no declaration after it is javadoc's target"
        );
    }

    /// A nest of Doxygen conditionals under a parse `ERROR`, asked the
    /// way a stretch asks — `read_past_error` judges the error's
    /// children on a chain *derived* from the walk's — reads each
    /// conditional once per walk, as the same nest does outside an
    /// error. The derived chain used to be rebuilt with
    /// `Ancestors::known`, which carries no cache, so the nest was read
    /// `O(depth²)` on every walk despite the memo (#1686 review, lesson
    /// 101). Rebuilding it that way again fails the memoised count.
    #[cfg(feature = "c")]
    #[test]
    fn an_error_s_conditionals_are_read_through_the_walk_s_memo() {
        use crate::lang_helpers::doxygen::conditionals_read;
        use crate::langs::CCode;
        use crate::node::{FoldCache, Tree};
        const DEPTH: usize = 40;
        // An unclosed anonymous `struct {` makes recovery wrap everything
        // after it in one `ERROR` whose only named child is the outermost
        // conditional — a tag name would be a second and make the error
        // itself a target (`no_entry_inside`). The nest's bottom holds no
        // declaration, so the error holds no target and is read past.
        let source = format!(
            "struct {{\n{}#include <x.h>\n{}",
            "/** d */\n#ifdef A\n".repeat(DEPTH),
            "#endif\n".repeat(DEPTH)
        );
        let tree = Tree::new::<CCode>(source.as_bytes());
        let root = tree.get_root();
        let error = root
            .children()
            .find(Node::is_error)
            .expect("recovery leaves an ERROR under the root");
        let nested = error
            .children()
            .filter(|child| crate::lang_helpers::doxygen::preproc_conditional::<CCode>(child))
            .count();
        assert_eq!(
            nested, 1,
            "the outermost conditional sits directly under the ERROR"
        );

        let cache = FoldCache::default();
        let chain = [root];
        let walk = Ancestors::known(&chain).with_folds(&cache);
        let before = conditionals_read::observed();
        for _ in 0..DEPTH {
            assert!(
                read_past_error::<CCode>(&error, source.as_bytes(), walk),
                "an ERROR holding no target is read past"
            );
        }
        assert_eq!(
            conditionals_read::observed() - before,
            DEPTH,
            "each conditional under the ERROR is read once per walk, however \
             often the error is asked about"
        );

        let before = conditionals_read::observed();
        assert!(read_past_error::<CCode>(
            &error,
            source.as_bytes(),
            Ancestors::known(&chain)
        ));
        assert_eq!(
            conditionals_read::observed() - before,
            DEPTH,
            "one unmemoised ask reads the whole nest once"
        );
    }
}
