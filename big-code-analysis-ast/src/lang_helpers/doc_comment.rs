//! Doc-comment binding: which comments document, by where they sit and
//! by the declaration after them (#1597, #1613). The per-tool marker
//! tests these rules consult live in [`super::doc_text`] and are
//! re-exported here, so a `Checker` or role row names one path for both
//! halves (#1649).

pub(crate) use super::doc_text::{
    csharp_doc, doxygen, go_directive, java_doc, javadoc_block, jsdoc_virtual, kotlin_doc,
    lua_long_comment, phpdoc, pod_comment_region, ruby_directive_opener, ruby_magic_comment,
    tsdoc_block, tsdoc_module,
};
use crate::checker::Checker;
use crate::documentation::budget::{NoBudget, WorkBudget, infallible};
use crate::documentation::recognition::{PublicRules, SourceRules, source_target};
use crate::documentation::streaming::markers::CandidateView;
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

/// The spelling of `node` as its tokens: the bytes of every leaf of its
/// subtree that `C::is_comment` does not name, with ASCII whitespace
/// removed — what a layout- and comment-insensitive comparison reads, so
/// `Map<String, T>`, `Map<String,T>` and `Map</* k */ String, T>` are one
/// spelling (#1842). A comment subtree is skipped whole, so a grammar
/// whose comment holds children contributes none of them. One stack and
/// one cursor, as [`Node::preorder`] walks.
#[must_use]
pub(crate) fn token_spelling<C: Checker>(node: &Node, code: &[u8]) -> Vec<u8> {
    let mut spelling = Vec::new();
    let mut stack = vec![*node];
    let mut cursor = node.cursor();
    while let Some(current) = stack.pop() {
        if C::is_comment(&current) {
            continue;
        }
        if current.child_count() == 0 {
            spelling.extend(
                node_bytes(&current, code)
                    .iter()
                    .filter(|b| !b.is_ascii_whitespace()),
            );
            continue;
        }
        let first_child = stack.len();
        stack.extend(current.children_with(&mut cursor));
        stack[first_child..].reverse();
    }
    spelling
}

/// Whether `comment` lies outside every function and closure body, by the
/// same fold [`documents_where_it_sits`] runs, for a language whose
/// in-body doc comments document wherever they sit and so must not bind.
#[must_use]
pub(crate) fn outside_function_bodies<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> bool {
    infallible(outside_function_bodies_with_budget::<T, _>(
        comment, code, ancestors, &NoBudget,
    ))
}

pub(crate) fn outside_function_bodies_with_budget<'a, T: Checker, B: WorkBudget>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    budget: &B,
) -> Result<bool, B::Error> {
    Ok(!held_in_body_with_budget::<T, _>(
        comment, code, 0, ancestors, budget,
    )?)
}

/// Whether an ancestor of `node`, past the `skip` nearest ones, holds it
/// in a function body. One step closure for the shared
/// [`FoldLane::DocInBody`] memo: `fold_down` serves a cached answer to
/// every caller on the lane, so the question must be asked the same way
/// everywhere.
fn held_in_body<'a, T: Checker>(
    node: &Node<'a>,
    code: &[u8],
    skip: usize,
    ancestors: Ancestors<'a, '_>,
) -> bool {
    infallible(held_in_body_with_budget::<T, _>(
        node, code, skip, ancestors, &NoBudget,
    ))
}

fn held_in_body_with_budget<'a, T: Checker, B: WorkBudget>(
    node: &Node<'a>,
    code: &[u8],
    skip: usize,
    ancestors: Ancestors<'a, '_>,
    budget: &B,
) -> Result<bool, B::Error> {
    Ok(ancestors.fold_down_with_budget(
        node,
        skip,
        FoldLane::DocInBody,
        0,
        |held, ancestor, outer, below| {
            Ok(u8::from(
                held != 0 || T::holds_doc_comment_in_body(ancestor, code, outer, below),
            ))
        },
        budget,
    )? != 0)
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
        if T::holds_doc_comment_in_body(&ancestor, code, outer, &below)
            && !T::doc_comment_opens_body(node, &ancestor, &below, code)
        {
            return false;
        }
        below = ancestor;
    }
    !held_in_body::<T>(node, code, 2, ancestors)
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
    let view = CandidateView::Legacy;
    let end = match stretch_end::<T>(comment, code, ancestors, binding.skips, view) {
        // A later member decides.
        StretchEnd::Undecided => return,
        // Nothing documents, and no block gathers the stretch either.
        StretchEnd::Elsewhere if binding.gathers.is_none() => return,
        end => end,
    };
    if !documents_where_it_sits::<T>(comment, code, ancestors) {
        return;
    }
    let CollectedStretch {
        members: stretch,
        code_before,
        ..
    } = collect_stretch::<T>(comment, code, ancestors, binding.skips, view);
    let forward = |n: &Node<'a>| {
        view.marker::<T>(n, code, ancestors) && (binding.forward)(n, code, ancestors)
    };
    let StretchEnd::AtTarget { next, target } = end else {
        if let Some(gathers) = binding.gathers {
            bind_gathered(
                &stretch,
                code,
                gathers,
                |n| view.marker::<T>(n, code, ancestors),
                forward,
            )
            .kept
            .iter()
            .for_each(&mut credit);
        }
        return;
    };
    let kept = select_bound::<T>(
        &stretch,
        code,
        binding.kept,
        StretchBounds {
            code_before,
            next,
            target,
        },
        forward,
    );
    kept.nodes.iter().for_each(&mut credit);
}

#[derive(Clone, Copy)]
pub(crate) struct StretchBounds<'a> {
    pub(crate) code_before: Option<Node<'a>>,
    pub(crate) next: Node<'a>,
    pub(crate) target: Node<'a>,
}

pub(crate) struct BoundSelection<'a> {
    pub(crate) nodes: Vec<Node<'a>>,
    eligible_end: usize,
    after_code_row: Option<usize>,
    policy: KeptDocs,
    next: Node<'a>,
}

impl BoundSelection<'_> {
    pub(crate) fn eligible(&self, index: usize, stretch: &[Node], code: &[u8]) -> bool {
        let node = &stretch[index];
        index < self.eligible_end
            && Some(node.start_row()) != self.after_code_row
            && match self.policy {
                KeptDocs::Nearest { hidden } => {
                    !hidden(node, stretch.get(index + 1).unwrap_or(&self.next))
                }
                KeptDocs::Leading { hidden, .. } => !hidden(node, code),
                _ => true,
            }
    }
}

pub(crate) fn select_bound<'a, T: Checker>(
    stretch: &[Node<'a>],
    code: &[u8],
    kept: KeptDocs,
    boundaries: StretchBounds<'a>,
    forward: impl Fn(&Node<'a>) -> bool,
) -> BoundSelection<'a> {
    infallible(select_bound_with_budget::<T, _>(
        stretch,
        code,
        kept,
        boundaries,
        |node| Ok(forward(node)),
        &NoBudget,
    ))
}

pub(crate) fn select_bound_with_budget<'a, T: Checker, B: WorkBudget>(
    stretch: &[Node<'a>],
    code: &[u8],
    kept: KeptDocs,
    boundaries: StretchBounds<'a>,
    forward: impl Fn(&Node<'a>) -> Result<bool, B::Error>,
    budget: &B,
) -> Result<BoundSelection<'a>, B::Error> {
    let StretchBounds {
        code_before,
        next,
        target,
    } = boundaries;
    let mut selection = BoundSelection {
        nodes: Vec::new(),
        eligible_end: stretch.len(),
        after_code_row: None,
        policy: kept,
        next,
    };
    selection.nodes = match kept {
        KeptDocs::All => {
            let mut nodes = Vec::new();
            for node in stretch {
                budget.step()?;
                if forward(node)? {
                    nodes.push(*node);
                }
            }
            nodes
        }
        KeptDocs::Last { joins } => {
            let (nodes, eligible_end) = bind_last::<T, B>(stretch, code, joins, forward, budget)?;
            selection.eligible_end = eligible_end;
            nodes
        }
        KeptDocs::Nearest { hidden } => {
            bind_nearest::<T, B>(stretch, next, hidden, forward, budget)?
                .into_iter()
                .collect()
        }
        KeptDocs::Leading {
            hidden,
            joins,
            after_code,
        } => {
            // A member on the row the code before the stretch ends on is
            // after that code, where a scan from the next line break does
            // not reach.
            selection.after_code_row = code_before
                .filter(|_| !after_code(&target))
                .map(|before| before.end_row());
            let attaches =
                |n: &Node<'a>| Ok(forward(n)? && Some(n.start_row()) != selection.after_code_row);
            let (nodes, eligible_end) =
                bind_leading::<T, B>(stretch, code, hidden, joins, attaches, budget)?;
            selection.eligible_end = eligible_end;
            nodes
        }
    };
    Ok(selection)
}

/// Where a stretch of comments ends, asked of its last comment.
#[derive(Clone, Copy)]
pub(crate) enum StretchEnd<'a> {
    /// A later comment of the stretch decides.
    Undecided,
    /// At `target`, an [`Checker::is_doc_target`] or a parse error holding
    /// one. `next` is the sibling right after the asking comment, where a
    /// walk back from the target begins.
    AtTarget { next: Node<'a>, target: Node<'a> },
    /// At a statement, a closing brace, the end of the file — anything
    /// that documents nothing.
    Elsewhere,
    /// Excluded bytes interrupt the source stretch or target spelling.
    ExcludedGap,
    /// Parsed siblings are exhausted but excluded source remains in this scope.
    SuffixGap,
}

/// Where the stretch `comment` belongs to ends. The scan stops at the
/// first sibling that is neither skipped nor a parse error read past, so
/// it never passes a later comment of the stretch. Only an error is asked
/// whether it is read past, and only once the binding's own `skips` and
/// [`Checker::is_doc_target`] have passed on it, so a binding that decides
/// errors itself (Doxygen's) keeps its reading.
pub(crate) fn stretch_end<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    skips: for<'b> fn(&Node<'a>, Ancestors<'a, 'b>) -> bool,
    view: CandidateView,
) -> StretchEnd<'a> {
    infallible(stretch_end_with_budget::<PublicRules<T>, _>(
        comment,
        code,
        ancestors,
        |node, ancestors| Ok(skips(node, ancestors)),
        view,
        &NoBudget,
    ))
}

pub(crate) fn stretch_end_with_budget<'a, R: SourceRules, B: WorkBudget>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    skips: impl Fn(&Node<'a>, Ancestors<'a, '_>) -> Result<bool, B::Error>,
    view: CandidateView<'_>,
    budget: &B,
) -> Result<StretchEnd<'a>, B::Error> {
    let mut after = ancestors
        .siblings_after_with_budget(comment, budget)?
        .peekable();
    let Some(&next) = after.peek() else {
        return Ok(
            if view.covers_with_budget(comment.start_byte()..comment.end_byte(), budget)? {
                exhausted_stretch(comment, comment.end_byte(), code, ancestors, view, budget)?
            } else {
                StretchEnd::ExcludedGap
            },
        );
    };
    let mut boundary = comment.start_byte();
    let mut continuous = true;
    let mut target = None;
    for n in after {
        budget.step()?;
        continuous = view.covers_with_budget(boundary..n.end_byte(), budget)?;
        boundary = n.end_byte();
        if !continuous
            || (!skips(&n, ancestors)?
                && !read_past_error_with_budget::<R, _>(&n, code, ancestors, view, budget)?)
        {
            target = Some(n);
            break;
        }
    }
    if !continuous {
        return Ok(StretchEnd::ExcludedGap);
    }
    Ok(match target {
        Some(target) if R::Checker::is_comment(&target) => StretchEnd::Undecided,
        Some(target)
            if source_target::<R, _>(view, &target, code, ancestors, budget)?
                || (target.is_error()
                    && error_holds_target_with_budget::<R, _>(
                        &target, code, ancestors, view, budget,
                    )?) =>
        {
            StretchEnd::AtTarget { next, target }
        }
        None => exhausted_stretch(comment, boundary, code, ancestors, view, budget)?,
        _ => StretchEnd::Elsewhere,
    })
}

fn exhausted_stretch<'a, B: WorkBudget>(
    comment: &Node<'a>,
    last_examined_end: usize,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    view: CandidateView<'_>,
    budget: &B,
) -> Result<StretchEnd<'a>, B::Error> {
    Ok(
        if suffix_gap_with_budget(comment, last_examined_end, code, ancestors, view, budget)? {
            StretchEnd::SuffixGap
        } else {
            StretchEnd::Elsewhere
        },
    )
}

#[cfg(all(test, feature = "rust"))]
fn suffix_gap<'a>(
    comment: &Node<'a>,
    last_examined_end: usize,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    view: CandidateView<'_>,
) -> bool {
    infallible(suffix_gap_with_budget(
        comment,
        last_examined_end,
        code,
        ancestors,
        view,
        &NoBudget,
    ))
}

fn suffix_gap_with_budget<'a, B: WorkBudget>(
    comment: &Node<'a>,
    last_examined_end: usize,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    view: CandidateView<'_>,
    budget: &B,
) -> Result<bool, B::Error> {
    if matches!(view, CandidateView::Legacy) {
        return Ok(false);
    }
    let scope_end = source_scope_bounds(comment, code, ancestors).end;
    Ok(!view.covers_with_budget(last_examined_end..scope_end, budget)?)
}

// The caller's chain is authoritative, including an explicitly empty one.
// Only the unknown fallback resolves physical parents; a restricted root's span
// cannot substitute for the full source interval.
fn source_scope_bounds<'a>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
) -> std::ops::Range<usize> {
    source_scope_reads::record();
    let scope = match ancestors.depth() {
        Some(0 | 1) => None,
        Some(_) => ancestors.parent(node),
        None => node.parent().filter(|parent| parent.parent().is_some()),
    };
    scope.map_or(0..code.len(), |scope| scope.start_byte()..scope.end_byte())
}

crate::observation::counter!(source_scope_reads, cfg(any(test, feature = "test-support")));

/// The stretch `comment` ends, in source order, and the sibling before it:
/// `comment` and the siblings before it that are comments or skipped,
/// read past the parse errors [`stretch_end`] reads past with the same
/// test, so every member whose scan stopped at the next comment is in
/// the stretch that decides it. The errors themselves are left out: the
/// `safe` of a `safe fn` is between the item's doc comments and its
/// first token, not that token. An error the scan does not read past —
/// one holding a target — ends the stretch and is the sibling before it.
pub(crate) struct CollectedStretch<'a> {
    pub(crate) members: Vec<Node<'a>>,
    pub(crate) code_before: Option<Node<'a>>,
    pub(crate) prefix_gap: bool,
}

pub(crate) fn collect_stretch<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    skips: for<'b> fn(&Node<'a>, Ancestors<'a, 'b>) -> bool,
    view: CandidateView,
) -> CollectedStretch<'a> {
    infallible(collect_stretch_with_budget::<PublicRules<T>, _>(
        comment,
        code,
        ancestors,
        |node, ancestors| Ok(skips(node, ancestors)),
        view,
        &NoBudget,
    ))
}

pub(crate) fn collect_stretch_with_budget<'a, R: SourceRules, B: WorkBudget>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    skips: impl Fn(&Node<'a>, Ancestors<'a, '_>) -> Result<bool, B::Error>,
    view: CandidateView<'_>,
    budget: &B,
) -> Result<CollectedStretch<'a>, B::Error> {
    stretches_collected::record();
    let mut stretch = vec![*comment];
    let mut before = None;
    let mut prefix_gap = false;
    let mut boundary = comment.end_byte();
    // An error read past is no member of the stretch: neither a comment
    // nor the first token a binder stops at. Left in, a binder that keeps
    // the last doc comment before the first token (tsc's, JSDoc's,
    // javac's) would read the error as that token and keep the comment
    // above it instead of the one the tool attaches.
    for sibling in ancestors.siblings_before_with_budget(comment, budget)? {
        budget.step()?;
        if !view.covers_with_budget(sibling.start_byte()..boundary, budget)? {
            prefix_gap = true;
            break;
        }
        boundary = sibling.start_byte();
        if read_past_error_with_budget::<R, _>(&sibling, code, ancestors, view, budget)? {
            continue;
        }
        if R::Checker::is_comment(&sibling) || skips(&sibling, ancestors)? {
            stretch.push(sibling);
        } else {
            before = Some(sibling);
            break;
        }
    }
    if matches!(view, CandidateView::Source(_)) && before.is_none() && !prefix_gap {
        let start = source_scope_bounds(comment, code, ancestors).start;
        prefix_gap = !view.covers_with_budget(start..boundary, budget)?;
    }
    crate::documentation::budget::reverse(&mut stretch, budget)?;
    Ok(CollectedStretch {
        members: stretch,
        code_before: before,
        prefix_gap,
    })
}

/// Whether a stretch reads past `node`: a parse error that is no doc
/// target and holds none — tokens the grammar could not place, such as the
/// `safe` of a Rust 2024 foreign item, which tree-sitter-rust leaves as an
/// error between the item's doc comment and the rest of it. Given the
/// benefit of the doubt, as Doxygen's binding gives one (`doxygen_skips`),
/// rather than dropping the doc comment on a parse failure.
#[cfg(all(test, feature = "c"))]
fn read_past_error_in<'a, T: Checker>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    view: CandidateView,
) -> bool {
    infallible(read_past_error_with_budget::<PublicRules<T>, _>(
        node, code, ancestors, view, &NoBudget,
    ))
}

fn read_past_error_with_budget<'a, R: SourceRules, B: WorkBudget>(
    node: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    view: CandidateView<'_>,
    budget: &B,
) -> Result<bool, B::Error> {
    Ok(node.is_error()
        && !source_target::<R, _>(view, node, code, ancestors, budget)?
        && !error_holds_target_with_budget::<R, _>(node, code, ancestors, view, budget)?)
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
#[cfg(all(test, feature = "java"))]
fn error_holds_target_in<'a, T: Checker>(
    error: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    view: CandidateView,
) -> bool {
    infallible(error_holds_target_with_budget::<PublicRules<T>, _>(
        error, code, ancestors, view, &NoBudget,
    ))
}

fn error_holds_target_with_budget<'a, R: SourceRules, B: WorkBudget>(
    error: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    view: CandidateView<'_>,
    budget: &B,
) -> Result<bool, B::Error> {
    let under = ancestors.of_children(error);
    let inside = under.get();
    for child in error.children() {
        budget.step()?;
        if child.is_named() && source_target::<R, _>(view, &child, code, inside, budget)? {
            return Ok(true);
        }
    }
    Ok(false)
}

// Output alone cannot reveal repeated prefix collection or rejected-path work.
crate::observation::counter!(
    stretches_collected,
    cfg(any(test, feature = "test-support"))
);

/// What a stretch that ends at no target documents: what its last
/// gathering comment ([`DocBinding::gathers`]) takes — the forward doc
/// comments before it, and the line doc comments on the rows directly
/// below it, which a line-comment tool reads as one block with it. The
/// gathering comment does not bind forward, so `forward` drops it; it
/// documents where it sits.
pub(crate) struct Gathered<'a> {
    pub(crate) kept: Vec<Node<'a>>,
    pub(crate) gatherer: Option<Node<'a>>,
}

crate::observation::counter!(gatherer_probes, cfg(any(test, feature = "test-support")));

pub(crate) fn bind_gathered<'a>(
    stretch: &[Node<'a>],
    code: &[u8],
    gathers: fn(&Node, &[u8]) -> bool,
    marker: impl Fn(&Node<'a>) -> bool,
    forward: impl Fn(&Node<'a>) -> bool,
) -> Gathered<'a> {
    infallible(bind_gathered_with_budget(
        stretch,
        code,
        |node| Ok(gathers(node, code)),
        |node| Ok(marker(node)),
        |node| Ok(forward(node)),
        &NoBudget,
    ))
}

pub(crate) fn bind_gathered_with_budget<'a, B: WorkBudget>(
    stretch: &[Node<'a>],
    code: &[u8],
    gathers: impl Fn(&Node<'a>) -> Result<bool, B::Error>,
    marker: impl Fn(&Node<'a>) -> Result<bool, B::Error>,
    forward: impl Fn(&Node<'a>) -> Result<bool, B::Error>,
    budget: &B,
) -> Result<Gathered<'a>, B::Error> {
    let mut gatherer = None;
    for (at, n) in stretch.iter().enumerate().rev() {
        budget.step()?;
        gatherer_probes::record();
        if marker(n)? && gathers(n)? {
            gatherer = Some(at);
            break;
        }
    }
    let Some(gatherer) = gatherer else {
        return Ok(Gathered {
            kept: Vec::new(),
            gatherer: None,
        });
    };
    let line = |n: &Node| node_bytes(n, code).starts_with(b"//");
    // Only a line-comment gatherer reads the line doc comments below it
    // as one block with itself.
    let gathers_below = line(&stretch[gatherer]);
    let mut last = gatherer;
    while let Some(below) = stretch.get(last + 1)
        && gathers_below
        && line(below)
        && marker(below)?
        && below.start_row() == stretch[last].end_row().saturating_add(1)
    {
        budget.step()?;
        last += 1;
    }
    let mut kept = Vec::new();
    for node in &stretch[..=last] {
        budget.step()?;
        if forward(node)? {
            kept.push(*node);
        }
    }
    Ok(Gathered {
        kept,
        gatherer: Some(stretch[gatherer]),
    })
}

/// [`KeptDocs::Last`] over a stretch in source order: the last doc
/// comment before the first skipped node — the declaration's first token —
/// with the doc comments it joins, or else the first doc comment after it.
fn bind_last<'a, T: Checker, B: WorkBudget>(
    stretch: &[Node<'a>],
    code: &[u8],
    joins: fn(&Node, &Node, &[u8]) -> bool,
    forward: impl Fn(&Node<'a>) -> Result<bool, B::Error>,
    budget: &B,
) -> Result<(Vec<Node<'a>>, usize), B::Error> {
    let mut split = stretch.len();
    for (at, n) in stretch.iter().enumerate() {
        budget.step()?;
        if !T::is_comment(n) {
            split = at;
            break;
        }
    }
    let (before, after) = stretch.split_at(split);
    let mut last = None;
    for (at, node) in before.iter().enumerate().rev() {
        budget.step()?;
        if forward(node)? {
            last = Some(at);
            break;
        }
    }
    let Some(last) = last else {
        let mut found = Vec::new();
        for node in after {
            budget.step()?;
            if forward(node)? {
                found.push(*node);
                break;
            }
        }
        return Ok((found, stretch.len()));
    };
    Ok((
        joined_run(before, last, code, joins, forward, budget)?,
        split,
    ))
}

/// [`KeptDocs::Leading`] over a stretch in source order: the members
/// before the first token, read back past the hidden ones to the nearest
/// other one, kept if it is a doc comment that binds forward, with the doc
/// comments directly before it that it joins.
fn bind_leading<'a, T: Checker, B: WorkBudget>(
    stretch: &[Node<'a>],
    code: &[u8],
    hidden: fn(&Node, &[u8]) -> bool,
    joins: fn(&Node, &Node, &[u8]) -> bool,
    forward: impl Fn(&Node<'a>) -> Result<bool, B::Error>,
    budget: &B,
) -> Result<(Vec<Node<'a>>, usize), B::Error> {
    let mut first_token = stretch.len();
    for (at, n) in stretch.iter().enumerate() {
        budget.step()?;
        if !T::is_comment(n) && !hidden(n, code) {
            first_token = at;
            break;
        }
    }
    let before = &stretch[..first_token];
    let mut last = None;
    for (at, n) in before.iter().enumerate().rev() {
        budget.step()?;
        if !hidden(n, code) {
            last = Some(at);
            break;
        }
    }
    let Some(last) = last else {
        return Ok((Vec::new(), first_token));
    };
    if !forward(&before[last])? {
        return Ok((Vec::new(), first_token));
    }
    Ok((
        joined_run(before, last, code, joins, forward, budget)?,
        first_token,
    ))
}

/// `before[last]` with the doc comments directly before it that it
/// joins, in source order. `joins` is asked first: for a binding whose
/// `joins` is [`never_joins`] it answers at once, where `forward`
/// would scan the comment's bytes for nothing.
fn joined_run<'a, B: WorkBudget>(
    before: &[Node<'a>],
    last: usize,
    code: &[u8],
    joins: fn(&Node, &Node, &[u8]) -> bool,
    forward: impl Fn(&Node<'a>) -> Result<bool, B::Error>,
    budget: &B,
) -> Result<Vec<Node<'a>>, B::Error> {
    let mut first = last;
    while first > 0
        && joins(&before[first - 1], &before[first], code)
        && forward(&before[first - 1])?
    {
        budget.step()?;
        first -= 1;
    }
    let mut nodes = Vec::new();
    for node in &before[first..=last] {
        budget.step()?;
        nodes.push(*node);
    }
    Ok(nodes)
}

/// [`KeptDocs::Nearest`] over a stretch in source order whose last
/// member `next` follows: the comment nearest the declaration that the
/// tool can see, if it is a doc comment.
fn bind_nearest<'a, T: Checker, B: WorkBudget>(
    stretch: &[Node<'a>],
    next: Node<'a>,
    hidden: fn(&Node, &Node) -> bool,
    forward: impl Fn(&Node<'a>) -> Result<bool, B::Error>,
    budget: &B,
) -> Result<Option<Node<'a>>, B::Error> {
    let mut later = next;
    for member in stretch.iter().rev() {
        budget.step()?;
        if T::is_comment(member) && !hidden(member, &later) {
            return Ok(forward(member)?.then_some(*member));
        }
        later = *member;
    }
    Ok(None)
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
    let Some(run) = positional_run::<T>(comment, code, ancestors, CandidateView::Legacy) else {
        return;
    };
    let Some(first) = run.first_eligible else {
        return;
    };
    let documents = run
        .below
        .is_some_and(|n| T::is_doc_target(&n, code, ancestors))
        || T::is_doc_run_opener(&first, run.below.as_ref(), code, ancestors);
    if !documents {
        return;
    }
    for member in run.members_from(first, ancestors) {
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
pub(crate) fn run_continues_with_budget<R: SourceRules, B: WorkBudget>(
    comment: &Node,
    next: &Node,
    code: &[u8],
    budget: &B,
) -> Result<bool, B::Error> {
    Ok(R::Checker::is_comment(next)
        && (next.start_row() <= comment.end_row()
            || (next.start_row() == comment.end_row().saturating_add(1)
                && !R::ends(comment, code, budget)?)))
}

/// Constant-sized boundaries from the completed positional run. Members stream
/// forward only after legacy acceptance, or into source-only storage.
pub(crate) struct PositionalRun<'a> {
    pub(crate) first: Node<'a>,
    pub(crate) last: Node<'a>,
    pub(crate) first_eligible: Option<Node<'a>>,
    pub(crate) next: Option<Node<'a>>,
    pub(crate) below: Option<Node<'a>>,
    pub(crate) code_before: Option<Node<'a>>,
    pub(crate) prefix_gap: bool,
    pub(crate) suffix_gap: bool,
    pub(crate) end: crate::documentation::streaming::positional::RunEnd,
}

impl<'a> PositionalRun<'a> {
    pub(crate) fn materialize_with_budget<B: WorkBudget>(
        &self,
        ancestors: Ancestors<'a, '_>,
        budget: &B,
    ) -> Result<Vec<Node<'a>>, B::Error> {
        let mut members = Vec::new();
        let after = ancestors.siblings_after_with_budget(&self.first, budget)?;
        for member in std::iter::once(self.first)
            .chain(after)
            .take_while(|member| member.start_byte() <= self.last.start_byte())
        {
            budget.step()?;
            members_materialized::record();
            members.push(member);
        }
        Ok(members)
    }

    pub(crate) fn members_from<'c>(
        &self,
        first: Node<'a>,
        ancestors: Ancestors<'a, 'c>,
    ) -> impl Iterator<Item = Node<'a>> + use<'a, 'c> {
        let last_start = self.last.start_byte();
        std::iter::once(first)
            .chain(ancestors.siblings_after(&first))
            .take_while(move |member| member.start_byte() <= last_start)
    }
}

crate::observation::counter!(
    positional_runs_collected,
    cfg(any(test, feature = "test-support"))
);
crate::observation::counter!(
    positional_predecessors,
    cfg(any(test, feature = "test-support"))
);
// Counts actual node-buffer pushes; legacy boundary and coverage paths never call it.
crate::observation::counter!(
    members_materialized,
    cfg(any(test, feature = "test-support"))
);

pub(crate) fn positional_run<'a, T: Checker>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    view: CandidateView<'_>,
) -> Option<PositionalRun<'a>> {
    infallible(positional_run_with_budget::<PublicRules<T>, _>(
        comment, code, ancestors, view, &NoBudget,
    ))
}

pub(crate) fn positional_run_with_budget<'a, R: SourceRules, B: WorkBudget>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    view: CandidateView<'_>,
    budget: &B,
) -> Result<Option<PositionalRun<'a>>, B::Error> {
    use crate::documentation::streaming::positional::RunEnd;
    let next = ancestors
        .siblings_after_with_budget(comment, budget)?
        .next();
    let continuous = view.covers_with_budget(
        comment.start_byte()..next.map_or(comment.end_byte(), |n| n.end_byte()),
        budget,
    )?;
    if continuous
        && let Some(next) = next
        && run_continues_with_budget::<R, _>(comment, &next, code, budget)?
    {
        return Ok(None);
    }
    let below = if let Some(next) = next
        && R::start_row(&next, budget)? == comment.end_row().saturating_add(1)
    {
        Some(next)
    } else {
        None
    };
    let end = if !continuous {
        RunEnd::ExcludedGap
    } else if R::ends(comment, code, budget)? {
        RunEnd::Terminator
    } else if next.is_none() {
        RunEnd::EndOfScope
    } else if below.is_none() && next.is_some_and(|n| n.start_row() > comment.end_row()) {
        RunEnd::BlankLine
    } else {
        RunEnd::Token
    };
    let mut run = collect_positional_prefix::<R, B>(comment, code, ancestors, view, budget)?;
    run.next = next;
    run.below = below.filter(|_| continuous);
    run.suffix_gap = next.is_none()
        && suffix_gap_with_budget(comment, comment.end_byte(), code, ancestors, view, budget)?;
    run.end = end;
    Ok(Some(run))
}

fn collect_positional_prefix<'a, R: SourceRules, B: WorkBudget>(
    comment: &Node<'a>,
    code: &[u8],
    ancestors: Ancestors<'a, '_>,
    view: CandidateView<'_>,
    budget: &B,
) -> Result<PositionalRun<'a>, B::Error> {
    // bca: suppress(cognitive, halstead) -- one reverse state machine preserves clean rows, same-row comments, trailing rows and excluded boundaries.
    positional_runs_collected::record();
    let mut run = PositionalRun {
        first: *comment,
        last: *comment,
        first_eligible: Some(*comment),
        next: None,
        below: None,
        code_before: None,
        prefix_gap: false,
        suffix_gap: false,
        end: crate::documentation::streaming::positional::RunEnd::EndOfScope,
    };
    let mut clean_first = None;
    let mut row_first = *comment;
    let mut stopped = false;
    for prev in ancestors.siblings_before_with_budget(comment, budget)? {
        budget.step()?;
        positional_predecessors::record();
        if !view.covers_with_budget(prev.start_byte()..row_first.end_byte(), budget)? {
            run.prefix_gap = true;
            stopped = true;
            break;
        }
        if !R::Checker::is_comment(&prev) {
            run.code_before = Some(prev);
            if prev.end_row() >= row_first.start_row() {
                run.first_eligible = clean_first;
            }
            stopped = true;
            break;
        }
        if !run_continues_with_budget::<R, _>(&prev, &row_first, code, budget)? {
            run.code_before = Some(prev);
            stopped = true;
            break;
        }
        if prev.end_row() < row_first.start_row() {
            clean_first = Some(row_first);
        }
        row_first = prev;
        run.first_eligible = Some(row_first);
    }
    if !stopped && matches!(view, CandidateView::Source(_)) {
        let start = source_scope_bounds(comment, code, ancestors).start;
        run.prefix_gap = !view.covers_with_budget(start..row_first.start_byte(), budget)?;
    }
    run.first = row_first;
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "rust")]
    #[test]
    fn documentation_scope_bounds_respect_known_and_unknown_context() {
        use crate::node::node_resolved_parent_lookups;
        use crate::{AnyParser, LANG, Search};
        let code = b"\nmod m { //! audit\n}\n";
        let parser = AnyParser::parse(LANG::Rust, code.to_vec(), None, None).unwrap();
        let root = parser.root_node();
        let before = node_resolved_parent_lookups::observed();
        assert_eq!(
            source_scope_bounds(&root, code, Ancestors::unknown()),
            0..21
        );
        assert_eq!(node_resolved_parent_lookups::observed() - before, 1);
        let mut checked = 0;
        root.act_on_node(&mut |node, ancestors| {
            let expected = match node.kind() {
                "mod_item" => 0..21,
                "line_comment" => 7..20,
                _ => return,
            };
            let before = node_resolved_parent_lookups::observed();
            assert_eq!(source_scope_bounds(node, code, ancestors), expected);
            assert_eq!(
                source_scope_bounds(node, code, Ancestors::known(&[])),
                0..21
            );
            assert_eq!(node_resolved_parent_lookups::observed() - before, 0);
            assert_eq!(
                source_scope_bounds(node, code, Ancestors::unknown()),
                expected
            );
            assert_eq!(node_resolved_parent_lookups::observed() - before, 2);
            checked += 1;
        });
        assert_eq!(checked, 2);
    }

    #[cfg(feature = "rust")]
    #[test]
    fn documentation_exhausted_tail_uses_the_current_lexical_scope() {
        use crate::documentation::SourceContext;
        use crate::{AnyParser, LANG, RustCode, Search};
        let source = b"mod m { //! audit\n}\nXXXX";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANG::Rust.tree_sitter_language().unwrap())
            .unwrap();
        parser
            .set_included_ranges(&[tree_sitter::Range {
                start_byte: 0,
                end_byte: 20,
                start_point: tree_sitter::Point::new(0, 0),
                end_point: tree_sitter::Point::new(2, 0),
            }])
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let parser = AnyParser::from_tree(LANG::Rust, tree, source.to_vec()).unwrap();
        let context = SourceContext::new(parser.ts_tree(), source);
        let mut checked = 0;
        parser.root_node().act_on_node(&mut |node, ancestors| {
            if RustCode::is_comment(node) {
                assert_eq!(ancestors.parent(node).unwrap().kind(), "declaration_list");
                assert_eq!(ancestors.parent(node).unwrap().end_byte(), 19);
                // Exercise the exhaustion helper directly: the parsed closing
                // brace normally stops marker scanning before this tail check.
                assert!(!suffix_gap(
                    node,
                    node.end_byte(),
                    source,
                    ancestors,
                    CandidateView::Source(&context)
                ));
                assert!(suffix_gap(
                    &parser.root_node(),
                    20,
                    source,
                    Ancestors::known(&[]),
                    CandidateView::Source(&context)
                ));
                checked += 1;
            }
        });
        assert_eq!(checked, 1);
    }

    #[cfg(feature = "c")]
    fn read_past_error<'a, T: Checker>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        read_past_error_in::<T>(node, code, ancestors, CandidateView::Legacy)
    }

    #[cfg(feature = "java")]
    fn error_holds_target<'a, T: Checker>(
        node: &Node<'a>,
        code: &[u8],
        ancestors: Ancestors<'a, '_>,
    ) -> bool {
        error_holds_target_in::<T>(node, code, ancestors, CandidateView::Legacy)
    }

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
    /// build answered quietly. That half discriminates only through the
    /// `debug_assert!`, so it is meaningful under `make pre-commit`'s
    /// debug build and vacuous under `--release` (lesson #80).
    ///
    /// The gate's seek is counted too, in every build: one, for the one
    /// package (#1871). Here the package is the error's first named
    /// child, so the scan stops at it and the count cannot tell a gate
    /// asked of every child from one asked of the packages alone;
    /// `a_wide_error_s_children_ask_for_one_sibling_in_java` judges
    /// every child for that.
    #[cfg(feature = "java")]
    #[test]
    fn a_wide_error_s_package_is_judged_without_the_walk_s_sibling_cache() {
        use crate::langs::JavaCode;
        use crate::node::{FoldCache, SiblingCache, Tree, WIDE_PARENT, sibling_seeks};
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
        let before = sibling_seeks::observed();
        assert!(
            error_holds_target::<JavaCode>(&error, source.as_bytes(), walk),
            "a package with no declaration after it is javadoc's target"
        );
        // expected: 1. Only a `package_declaration`'s gate asks for a
        // sibling; the error holds one, and it is a target, so the scan
        // of the error's children stops there.
        assert_eq!(
            sibling_seeks::observed() - before,
            1,
            "the package gate's one seek through the derived chain"
        );
    }

    // What judging every named child of the parse error in `source`
    // comes to, each as a doc target through the chain
    // `error_holds_target` derives for them: the named children, how
    // many are targets, and the uncached sibling lookups made. That
    // helper stops at the first target; this asks every child, so a
    // target test that came to ask for a sibling of each is counted
    // once per child, where each ask seeks from the error's first.
    #[cfg(any(
        feature = "c",
        feature = "cpp",
        feature = "java",
        feature = "mozcpp",
        feature = "objc"
    ))]
    fn judge_every_child_of_the_error<T: Checker + crate::traits::LanguageInfo>(
        source: &str,
    ) -> (usize, usize, usize) {
        use crate::node::{FoldCache, SiblingCache, Tree, WIDE_PARENT, sibling_seeks};
        let code = source.as_bytes();
        let tree = Tree::new::<T>(code);
        let root = tree.get_root();
        let error = root
            .children()
            .find(Node::is_error)
            .expect("recovery leaves an ERROR under the root");
        assert!(
            error.child_count() > WIDE_PARENT,
            "the ERROR must be wide: a seek under a narrow one is a short scan"
        );
        // What the walk lends: the root recorded, the ERROR not yet.
        let chain = [root];
        let mut siblings = SiblingCache::default();
        siblings.record(&root, 0, root.children());
        let folds = FoldCache::default();
        let walk = Ancestors::known(&chain)
            .with_siblings(&siblings)
            .with_folds(&folds);
        let under = walk.of_children(&error);
        let named: Vec<_> = error.children().filter(Node::is_named).collect();
        let before = sibling_seeks::observed();
        let targets = named
            .iter()
            .filter(|child| T::is_doc_target(child, code, under.get()))
            .count();
        (named.len(), targets, sibling_seeks::observed() - before)
    }

    // The Doxygen fixture: an unclosed anonymous `struct {` (`class {`
    // in the C++ grammars) makes recovery gather every field after it
    // into one flat `ERROR`, each a `field_declaration` Doxygen
    // documents.
    #[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
    fn doxygen_wide_error(opener: &str) -> String {
        use crate::node::WIDE_PARENT;
        format!(
            "/** d */\n{opener} {{\n{}",
            "int x;\n".repeat(WIDE_PARENT + 6)
        )
    }

    // expected, for each Doxygen grammar below: `WIDE_PARENT + 6` named
    // children, all of them targets, and no sibling asked for. No
    // C-family `DocTarget` gate reads a sibling.
    #[cfg(any(feature = "c", feature = "cpp", feature = "mozcpp", feature = "objc"))]
    const DOXYGEN_JUDGED: (usize, usize, usize) = {
        use crate::node::WIDE_PARENT;
        (WIDE_PARENT + 6, WIDE_PARENT + 6, 0)
    };

    /// The children of a wide parse `ERROR` are judged as doc targets
    /// through a derived chain, which lends no sibling record, so a
    /// target test that asked for a sibling of each child would seek
    /// from the error's first child every time. In the Doxygen grammars
    /// none asks (#1871).
    #[cfg(feature = "c")]
    #[test]
    fn a_wide_error_s_children_ask_for_no_sibling_in_c() {
        let judged =
            judge_every_child_of_the_error::<crate::langs::CCode>(&doxygen_wide_error("struct"));
        assert_eq!(judged, DOXYGEN_JUDGED);
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn a_wide_error_s_children_ask_for_no_sibling_in_cpp() {
        let judged =
            judge_every_child_of_the_error::<crate::langs::CppCode>(&doxygen_wide_error("class"));
        assert_eq!(judged, DOXYGEN_JUDGED);
    }

    #[cfg(feature = "mozcpp")]
    #[test]
    fn a_wide_error_s_children_ask_for_no_sibling_in_mozcpp() {
        let judged = judge_every_child_of_the_error::<crate::langs::MozcppCode>(
            &doxygen_wide_error("class"),
        );
        assert_eq!(judged, DOXYGEN_JUDGED);
    }

    #[cfg(feature = "objc")]
    #[test]
    fn a_wide_error_s_children_ask_for_no_sibling_in_objc() {
        let judged =
            judge_every_child_of_the_error::<crate::langs::ObjcCode>(&doxygen_wide_error("struct"));
        assert_eq!(judged, DOXYGEN_JUDGED);
    }

    /// Java's `DocTarget` row holds the one gate found to ask for a
    /// sibling through this chain, the package's. With the package
    /// after `WIDE_PARENT` local declarations, none of them a target,
    /// every child is judged and the package's ask is the only one: a
    /// gate asked of each child would count one per child here (#1871).
    #[cfg(feature = "java")]
    #[test]
    fn a_wide_error_s_children_ask_for_one_sibling_in_java() {
        use crate::node::WIDE_PARENT;
        let source = format!(
            "/** d */\nclass {{\n{}package a;\n@ \n",
            "int x;\n".repeat(WIDE_PARENT)
        );
        let judged = judge_every_child_of_the_error::<crate::langs::JavaCode>(&source);
        // expected: the `WIDE_PARENT` locals and the package; the package
        // alone is a target; its gate's one seek for what follows it.
        assert_eq!(judged, (WIDE_PARENT + 1, 1, 1));
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
