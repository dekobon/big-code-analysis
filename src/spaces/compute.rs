//! Metric-computation and AST-traversal internals for [`super::analyze`].
//!
//! These free functions were split out of `spaces.rs` to keep that
//! module focused on the public API types (`SpaceKind`, `CodeMetrics`,
//! `FuncSpace`, `Source`, `Ast`, `MetricsOptions`). They are moved
//! verbatim and re-exported from the parent so the public path
//! `crate::spaces::analyze` (and `pub(crate) metrics_inner`) is preserved.

use std::hash::BuildHasherDefault;

use super::*;
use crate::diag::warn;

/// Derives the two metrics that read from a space's *complete* state:
/// Halstead's `Stats` from the accumulated occurrence maps, and MI from
/// the resulting volume plus the space's final LOC and cyclomatic.
///
/// Both are single-assignment — each call overwrites the previous
/// result rather than accumulating — so running this before a space has
/// absorbed all of its children is wasted work, not a partial sum. Only
/// [`finalize_state`] calls it, once per space (#1106).
#[inline]
fn compute_halstead_and_mi<T: ParserTrait>(state: &mut State, selected: MetricSet) {
    if selected.contains(Metric::Halstead) {
        state
            .halstead_maps
            .finalize(&mut state.space.metrics.halstead);
    }
    if selected.contains(Metric::Mi) {
        // `MetricsOptions::with_only` guarantees Mi's dependencies
        // (Loc + Cyclomatic + Halstead) are also selected, so the
        // Stats values feeding into the MI formula here are populated
        // — not the zero defaults that would silently produce a
        // garbage MI score.
        T::Mi::compute(
            &state.space.metrics.loc,
            &state.space.metrics.cyclomatic,
            &state.space.metrics.halstead,
            &mut state.space.metrics.mi,
        );
    }
}

/// Records the space kind `wmc::Stats::merge` dispatches on, for the
/// kinds WMC recognises, plus the cumulative cyclomatic those kinds
/// contribute.
///
/// Unlike [`compute_halstead_and_mi`] this must also run on a *parent*
/// before each child merges into it: `wmc::Stats::merge` routes the
/// child's contribution on `self.space_kind`, which stays `Unknown`
/// until this runs, and an `Unknown` parent silently drops every
/// method's cyclomatic from its class WMC.
#[inline]
fn compute_wmc<T: ParserTrait>(state: &mut State, selected: MetricSet) {
    if selected.contains(Metric::Wmc) {
        T::Wmc::compute(
            state.space.kind,
            &state.space.metrics.cyclomatic,
            &mut state.space.metrics.wmc,
        );
    }
}

/// Records the space kind that decides whether `npm` / `npa` are
/// serialized on this space.
///
/// Both are emitted only on a member scope — a container, or the file
/// unit that rolls its containers up — and since #1203 the space's own
/// kind is the whole of that decision. Doing it here rather than letting
/// each language raise a flag from its own grammar node kinds is what
/// makes the rule hold for every language, including one added later:
/// there is no per-language surface left to deviate on, in either
/// direction.
///
/// `HAS_MEMBERS` is the one exception, and it is a language-level opt
/// out rather than a per-space one: a grammar with no class-shaped
/// construct anywhere (C, Bash, Lua, …) would otherwise report an
/// all-zero block on every file root, since a unit is a member scope
/// like any other.
#[inline]
fn note_member_scope<T: ParserTrait>(state: &mut State, selected: MetricSet) {
    let kind = state.space.kind;
    if selected.contains(Metric::Npm) && <T::Npm as Npm>::HAS_MEMBERS {
        state.space.metrics.npm.set_space_kind(kind);
    }
    if selected.contains(Metric::Npa) && <T::Npa as Npa>::HAS_MEMBERS {
        state.space.metrics.npa.set_space_kind(kind);
    }
}

#[inline]
fn compute_averages(state: &mut State, selected: MetricSet) {
    // The per-function averages for Cognitive, Exit, and NArgs divide
    // by counts sourced from `Nom`. `Metric::dependencies` declares
    // `Nom` as a dependency of all three, so `with_only` pulls it into
    // any selection that includes them and these divisors reflect the
    // real function/closure counts. As defense-in-depth, each `average`
    // accessor additionally guards its divisor with `.max(1)`, so even
    // a zero divisor degrades to `sum / 1` rather than `inf`/`NaN`
    // (#428). Compute the divisors once and feed them into each gated
    // finalize.
    let nom_functions = state.space.metrics.nom.functions_sum() as usize;
    let nom_closures = state.space.metrics.nom.closures_sum() as usize;
    let nom_total = state.space.metrics.nom.total() as usize;
    // Cognitive average
    if selected.contains(Metric::Cognitive) {
        state.space.metrics.cognitive.finalize(nom_total);
    }
    // Nexit average
    if selected.contains(Metric::Nexits) {
        state.space.metrics.nexits.finalize(nom_total);
    }
    // Nargs average
    if selected.contains(Metric::Nargs) {
        state
            .space
            .metrics
            .nargs
            .finalize(nom_functions, nom_closures);
    }
}

#[inline]
fn compute_minmax(state: &mut State, selected: MetricSet) {
    if selected.contains(Metric::Cyclomatic) {
        state.space.metrics.cyclomatic.compute_minmax();
    }
    if selected.contains(Metric::Nexits) {
        state.space.metrics.nexits.compute_minmax();
    }
    if selected.contains(Metric::Cognitive) {
        state.space.metrics.cognitive.compute_minmax();
    }
    if selected.contains(Metric::Nargs) {
        state.space.metrics.nargs.compute_minmax();
    }
    if selected.contains(Metric::Nom) {
        state.space.metrics.nom.compute_minmax();
    }
    if selected.contains(Metric::Loc) {
        state.space.metrics.loc.compute_minmax();
    }
    if selected.contains(Metric::Abc) {
        state.space.metrics.abc.compute_minmax();
    }
    if selected.contains(Metric::Tokens) {
        state.space.metrics.tokens.compute_minmax();
    }
}

#[inline]
fn compute_sum(state: &mut State, selected: MetricSet) {
    if selected.contains(Metric::Wmc) {
        state.space.metrics.wmc.compute_sum();
    }
    if selected.contains(Metric::Npm) {
        state.space.metrics.npm.compute_sum();
    }
    if selected.contains(Metric::Npa) {
        state.space.metrics.npa.compute_sum();
    }
}

/// Re-anchors the file-level unit's `sloc` row span to the span the unit
/// itself reports, discarding the measured start row seeded during the
/// walk.
///
/// Three places held a copy of "where does the unit's row span start",
/// and #1195 anchored only one of them — [`crate::spaces::line_span`],
/// which gives the unit line 1 because the unit *is* the file.
/// `loc::shared::init` seeds `sloc.start` from the visited node's first
/// token for every func-space, the root included, so a file opening with
/// blank lines dropped those rows from `sloc` (and so from `blank`, and
/// from MI's `ln(sloc)` term) while its reported `start_line..end_line`
/// still covered them. A leading *comment* was always fine, since
/// comments are in the tree — which is what made byte-identical content
/// score differently for being shifted down by one comment row (#1247).
///
/// Reading the span back off the finished [`FuncSpace`] leaves
/// `line_span` the single owner of the rule, degenerate cases included:
/// an empty file reports `0..0`, which converts to the default `0..0`
/// sloc span and keeps `sloc` at 0.
///
/// Done here rather than in [`FuncSpace::new`] because the root node's
/// own `Loc::compute` runs *after* construction and would overwrite an
/// anchor set there; and here rather than in the 20-odd per-language
/// `Loc` impls because none of them knows the space kind — re-widening
/// the trait with a unit flag is exactly what #1067 removed for perf.
#[inline]
fn anchor_unit_sloc_span(state: &mut State, selected: MetricSet) {
    if selected.contains(Metric::Loc) && state.space.kind == SpaceKind::Unit {
        // `start_line` is 1-based and `Sloc::start` is a 0-based row;
        // `saturating_sub` covers the empty-file `0..0`, whose 0-based
        // start is 0 either way.
        let start_row = state.space.start_line.saturating_sub(1);
        state
            .space
            .metrics
            .loc
            .init_unit_span(start_row, state.space.end_line);
    }
}

/// Runs the per-space finalization passes (unit-span anchoring, min/max,
/// sum, Halstead, MI, WMC, averages) on a single [`State`]. Shared by both
/// the single-element and pop arms of [`finalize`] so the call sequence
/// stays identical in both, and reached exactly once per space — every
/// state is finalized either when it is popped or, for the root, in the
/// single-element arm.
///
/// [`anchor_unit_sloc_span`] runs first because everything after it reads
/// the span it fixes: `compute_minmax` folds `sloc` into the unit's
/// `sloc_min`/`sloc_max`, and `compute_halstead_and_mi` feeds it into MI's
/// `ln(sloc)` term.
///
/// [`finalize`]'s pop arm additionally calls [`compute_wmc`] on the
/// *parent* before each child merges into it, because `wmc::Stats::merge`
/// dispatches on the parent's recorded `space_kind`. It deliberately does
/// **not** re-run [`compute_halstead_and_mi`] there: `halstead::Stats` and
/// `mi::Stats` both have no-op `merge`s, so nothing reads a parent's
/// intermediate Halstead/MI, and this call overwrites them from the final
/// maps anyway (#1106).
fn finalize_state<T: ParserTrait>(state: &mut State, selected: MetricSet) {
    anchor_unit_sloc_span(state, selected);
    compute_minmax(state, selected);
    compute_sum(state, selected);
    compute_halstead_and_mi::<T>(state, selected);
    compute_wmc::<T>(state, selected);
    note_member_scope::<T>(state, selected);
    compute_averages(state, selected);
}

fn finalize<T: ParserTrait>(state_stack: &mut Vec<State>, diff_level: usize, selected: MetricSet) {
    if state_stack.is_empty() {
        return;
    }
    for _ in 0..diff_level {
        if state_stack.len() == 1 {
            let last_state = state_stack
                .last_mut()
                .expect("invariant: state_stack has exactly one element");
            finalize_state::<T>(last_state, selected);
            break;
        }
        let mut state = state_stack
            .pop()
            .expect("invariant: state_stack has more than one element");
        finalize_state::<T>(&mut state, selected);

        let last_state = state_stack
            .last_mut()
            .expect("invariant: state_stack has remaining elements after pop");
        last_state.halstead_maps.merge(&state.halstead_maps);
        compute_wmc::<T>(last_state, selected);

        // Merge function spaces
        last_state.space.metrics.merge(&state.space.metrics);
        last_state.space.spaces.push(state.space);
    }
}

/// Compute every metric for a [`Source`].
///
/// This is the recommended library entry point. It does not conflate
/// the top-level [`FuncSpace::name`] with a filesystem path: callers
/// supply an explicit `Source::name` and an optional
/// `Source::preproc_path` for C++ preprocessor lookup.
///
/// `options` controls per-traversal flags (e.g.
/// `MetricsOptions::default().with_exclude_tests(true)` to elide
/// Rust `#[test]` / `#[cfg(test)]` subtrees).
///
/// # Errors
///
/// The return type carries [`MetricsError::EmptyRoot`] for forward
/// compatibility, but the walker always pushes a synthetic top-level
/// [`SpaceKind::Unit`][crate::SpaceKind] `FuncSpace` before walking,
/// so this function does not return `Err` in practice today (see
/// the variant doc).
///
/// # Examples
///
/// Analysing an in-memory snippet without constructing a `Path`:
///
/// ```
/// use big_code_analysis::{analyze, MetricsOptions, Source, LANG};
///
/// let space = analyze(
///     Source::new(LANG::Rust, b"fn main() { let x = 1 + 2; }")
///         .with_name(Some("snippet.rs".to_owned())),
///     MetricsOptions::default(),
/// )
/// .expect("snippet has a top-level FuncSpace");
/// assert_eq!(space.name.as_deref(), Some("snippet.rs"));
/// ```
pub fn analyze(source: Source<'_>, options: MetricsOptions) -> Result<FuncSpace, MetricsError> {
    Ast::parse(source)?.metrics(options)
}

/// Per-node classification the walker derives once and the metrics
/// consume. Bundled rather than passed as loose `bool`s so the
/// call site cannot transpose them — they are all same-typed flags
/// about the node currently being visited.
#[derive(Clone, Copy)]
struct NodeFacts {
    /// This node opens a new [`FuncSpace`].
    func_space: bool,
    /// Whether this node lies inside a comment subtree — the node
    /// **itself** or any ancestor is a comment. Contrast
    /// [`Walk::in_comment`], which covers ancestors only (#1052).
    in_comment: bool,
}

// Per-node metric dispatch. Each `compute` call is paired with a bit
// check against the caller's selection. The bit tests are cheap
// (single AND-and-compare on the `MetricSet` bitfield) and an
// unselected metric saves both the call overhead and any per-node
// text-slice / token-table work the metric does internally — Halstead
// in particular owns `HalsteadMaps` allocations and is the headline
// cost saving for `with_only(&[Metric::Loc])`. Extracted from
// `metrics_inner` so the walker stays under clippy's 100-line ceiling.
#[inline]
fn compute_per_node<'a, T: ParserTrait>(
    state: &mut State<'a>,
    node: &Node<'a>,
    code: &'a [u8],
    options: MetricsOptions,
    facts: NodeFacts,
    ancestors: Ancestors<'a, '_>,
    nesting_map: &mut NestingMap,
) {
    let NodeFacts {
        func_space,
        in_comment,
    } = facts;
    let selected = options.metrics;
    let last = &mut state.space;
    if selected.contains(Metric::Cognitive) {
        T::Cognitive::compute(
            node,
            code,
            ancestors,
            &mut last.metrics.cognitive,
            nesting_map,
        );
    }
    if selected.contains(Metric::Cyclomatic) {
        T::Cyclomatic::compute_with_options(
            node,
            code,
            ancestors,
            &mut last.metrics.cyclomatic,
            options.count_cyclomatic_try,
        );
    }
    if selected.contains(Metric::Halstead) {
        T::Halstead::compute(node, code, ancestors, &mut state.halstead_maps);
    }
    if selected.contains(Metric::Loc) {
        T::Loc::compute(node, ancestors, &mut last.metrics.loc, func_space);
    }
    if selected.contains(Metric::Nom) {
        T::Nom::compute(node, code, ancestors, &mut last.metrics.nom);
    }
    if selected.contains(Metric::Tokens) {
        T::Tokens::compute(node, &mut last.metrics.tokens, in_comment);
    }
    if selected.contains(Metric::Nargs) {
        T::NArgs::compute(node, code, ancestors, &mut last.metrics.nargs);
    }
    if selected.contains(Metric::Nexits) {
        T::Exit::compute(node, code, &mut last.metrics.nexits);
    }
    if selected.contains(Metric::Abc) {
        T::Abc::compute(node, code, ancestors, &mut last.metrics.abc);
    }
    if selected.contains(Metric::Npm) {
        T::Npm::compute(node, code, ancestors, &mut last.metrics.npm);
    }
    if selected.contains(Metric::Npa) {
        T::Npa::compute(node, code, ancestors, &mut last.metrics.npa);
    }
}

/// Pushes a synthetic `Unit` root onto the state stack when the grammar
/// hands us a non-`Unit` root.
///
/// Some grammars (e.g. tree-sitter-mozcpp on unparseable input) return a
/// non-Unit root. Wrapping with a synthetic Unit space spanning the whole
/// file keeps the top-level `FuncSpace` upholding the LOC invariant
/// `blank = sloc - ploc - only_comment_lines >= 0`. A `Unit` root needs
/// no wrapper, so nothing is pushed in that case.
///
/// The frame's `loc` row span is *not* seeded here. The walk never visits
/// this synthetic node, so before #1247 this was the one place that could
/// give it one — a third copy of "where does the unit's row span start",
/// and one that copied the measured start row the anchored reported span
/// had already stopped agreeing with. [`anchor_unit_sloc_span`] now
/// derives every unit's span from the one [`crate::spaces::line_span`]
/// recorded, this frame included.
fn push_synthetic_unit_root<T: ParserTrait>(
    state_stack: &mut Vec<State>,
    node: &Node,
    code: &[u8],
    selected: MetricSet,
) {
    // `Ancestors::unknown()`: `node` is the tree root here, so it has no
    // ancestors to hand over either way.
    if T::Getter::get_space_kind_with_code(node, code, Ancestors::unknown()) != SpaceKind::Unit {
        let synthetic = FuncSpace::new::<T::Getter>(
            node,
            code,
            Ancestors::unknown(),
            SpaceKind::Unit,
            selected,
        );
        state_stack.push(State {
            space: synthetic,
            halstead_maps: HalsteadMaps::new(),
        });
    }
}

/// Pushes a new [`FuncSpace`] frame for `node` and returns the nesting
/// level its children inherit.
///
/// Only called once the walker has decided `node` opens a space, so the
/// `SpaceKind` lookup stays off the per-node path. That matters for some
/// languages — notably Elixir, whose `get_space_kind_with_code` runs a
/// per-`Call` source-text keyword scan, so it is far from a cheap enum
/// compare (issue #522; the `Loc` unit flag that used to force it on
/// every node went away with #1067).
fn open_func_space<'a, T: ParserTrait>(
    state_stack: &mut Vec<State<'a>>,
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
    level: usize,
    selected: MetricSet,
) -> usize {
    let kind = T::Getter::get_space_kind_with_code(node, code, ancestors);
    let mut space = FuncSpace::new::<T::Getter>(node, code, ancestors, kind, selected);
    // Membership is decided here, at the one point that still holds the
    // node the space was opened from — `finalize` sees only the finished
    // `FuncSpace`. A function the enclosing container does not own (e.g.
    // a C++ inline `friend`) keeps its own space and its own metrics;
    // only the container's WMC roll-up declines it (#1301).
    if selected.contains(Metric::Wmc) && T::Checker::is_non_member_function(node, code, ancestors) {
        space.metrics.wmc.mark_non_member();
    }
    state_stack.push(State {
        space,
        halstead_maps: HalsteadMaps::new(),
    });
    level + 1
}

/// Scans a comment node for a suppression marker and applies it against
/// `state_stack` immediately.
///
/// Doing this inline during the walk (rather than queueing markers for a
/// post-walk pass keyed on line number) pins each marker to the
/// syntactically nearest enclosing function space — the only frame on the
/// stack that the grammar nested the comment inside. Line-only matching
/// was ambiguous when two sibling functions shared a source line and the
/// first-by-source-order won regardless of which body actually contained
/// the comment (issue #289).
///
/// Every complaint the parse produced is logged, and whatever directive
/// it still yielded is applied: an unusable metric name costs its own
/// name and nothing else, while a body that parses to no directive at all
/// is logged and dropped (issue #1168). The walk never aborts — a typo in
/// one file must not derail a workspace-wide pass — and dropping stays
/// the conservative choice, since a marker can only ever lose coverage
/// this way, never gain it.
fn apply_comment_suppression(
    state_stack: &mut Vec<State>,
    node: &Node,
    code: &[u8],
    diagnostic_path: &str,
    is_comment: bool,
) {
    if is_comment && let Some(text) = node.utf8_text(code) {
        let scan = parse_suppression_marker(text);
        for diagnostic in &scan.diagnostics {
            // The `+ 1` converts tree-sitter's 0-based rows to the
            // 1-based line numbers `FuncSpace::start_line` and the
            // rest of this module report.
            warn(format_args!(
                "{}:{}: {diagnostic}",
                diagnostic_path,
                node.start_row() + 1
            ));
        }
        if let Some(suppression) = &scan.suppression {
            apply_suppression(state_stack, suppression);
        }
    }
}

/// Context carried down the metrics walk alongside each node.
///
/// `in_comment` replaces the per-leaf ancestor walk `Tokens::compute` used
/// to do. That walk was `O(depth)` per leaf and, because `Node::parent`
/// is itself `O(depth)`, made the metric `O(leaves × depth²)` — a few
/// kilobytes of deeply nested source burned minutes of CPU (issue #1052).
/// Propagating the flag down the traversal computes the same predicate in
/// `O(1)` per node: a node is inside a comment iff its parent was, or the
/// node itself is a comment.
#[derive(Clone, Copy)]
struct Walk {
    /// Nesting level, used to close func-spaces on the way back up.
    level: usize,
    /// AST depth — the number of ancestors this node has, so the root
    /// sits at `0`.
    ///
    /// Distinct from `level`, which only advances at func-space
    /// boundaries. This one indexes the ancestor chain the walk keeps
    /// for [`Ancestors`], which is why it has to count every step.
    depth: usize,
    /// Whether an **ancestor** of this node is a comment.
    ///
    /// Deliberately excludes the node itself — the walk ORs in
    /// `is_comment(node)` on arrival to get the node's own membership,
    /// and tags its children with that. Note this differs from
    /// [`NodeFacts::in_comment`], which *does* include the node: passing
    /// this field where that one is expected would stop excluding a
    /// comment's own leaves and reintroduce the #1052 miscount.
    in_comment: bool,
}

/// Seeds each child's `nesting_map` slot from `node`'s own, so
/// `Cognitive` can read the [`Nesting`] it inherits without calling
/// `Node::parent`.
///
/// A node's slot means two different things either side of its
/// `compute`, and the distinction is load-bearing:
///
/// - **on entry** it holds what the node inherits — this is what
///   `get_nesting_from_map` reads;
/// - **on exit** each language's `compute` has overwritten it with the
///   post-increment `Nesting` its children should see — this is what this
///   function hands down.
///
/// So the write at the end of every `Cognitive::compute` must stay at the
/// end. Moving it to the top would make every descendant inherit the
/// pre-increment `Nesting` and silently under-count.
///
/// `or_insert`, not `insert`: Python's comprehension handling pre-writes
/// its clause children's slots during the *comprehension's* `compute`
/// (the #421 fix, so clause nesting does not depend on sibling traversal
/// order), and those values deliberately differ from the comprehension's
/// own `Nesting`. A blanket overwrite would clobber them — the
/// `python_comprehension_*` tests in `metrics::cognitive` are what catch
/// it.
///
/// Grammars whose `Cognitive` impl is the macro's no-op (`Preproc`,
/// `Ccomment`) never write a slot, so the root lookup misses and the
/// walk seeds nothing for them at all — no map, no allocation.
fn propagate_nesting_to_children(
    node: &Node,
    children: &[(Node<'_>, Walk)],
    nesting_map: &mut NestingMap,
) {
    // Leaves are roughly half of a real AST, so bail before hashing a key
    // we would only read to iterate zero children.
    if children.is_empty() {
        return;
    }
    // A miss here is a *root-only* path. Every non-root slot is created
    // by this function's `or_insert` below, before that node is ever
    // popped, so reaching a node with no slot means it had no parent to
    // seed it. The root's own slot exists iff the root's `compute` wrote
    // one — which the two no-op grammars never do, so for them the walk
    // seeds nothing and the map stays empty.
    //
    // Note this does *not* depend on every real impl writing on every
    // path: a real impl that skipped its write would still leave its
    // children seeded, and would show up as wrong nesting values, not as
    // a missing slot.
    let Some(&inherited) = nesting_map.get(&node.id()) else {
        return;
    };
    for (child, _) in children {
        nesting_map.entry(child.id()).or_insert(inherited);
    }
}

/// Pushes `node`'s direct children onto the traversal `stack`, each tagged
/// with `tag`.
///
/// The ordering is load-bearing: pushing in source order and reversing
/// the freshly-pushed tail makes the LIFO `stack` yield children in
/// source order, which in turn governs line-shared suppression
/// attribution (issue #289).
///
/// `Tag` is generic because the two walkers carry different context down
/// the tree: `ops` needs only the nesting level, while `metrics_inner`
/// also propagates comment membership ([`Walk`], issue #1052).
///
/// Returns the children just pushed, as a slice borrowed from `stack` —
/// empty for a leaf, and in reverse source order like the stack itself.
///
/// Borrowing rather than returning indices is what makes "these are
/// exactly this node's children" a compiler-checked claim: the borrow
/// forbids touching `stack` while the slice is alive, so the slice
/// cannot drift from the pushes it describes. Recording `stack.len()`
/// around the call instead only holds while the two stay adjacent, and
/// nothing enforces that.
pub(crate) fn push_children<'a, 's, Tag: Copy>(
    cursor: &mut Cursor<'a>,
    node: &Node<'a>,
    tag: Tag,
    stack: &'s mut Vec<(Node<'a>, Tag)>,
) -> &'s [(Node<'a>, Tag)] {
    // Children go on in source order and the freshly-pushed tail is
    // reversed in place, so the LIFO `stack` yields the leftmost child
    // first. Equivalent to the `children.drain(..).rev()` this replaced,
    // without the caller-threaded scratch buffer, and each child is
    // copied once rather than twice.
    let first = stack.len();
    stack.extend(node.children_with(cursor).map(|child| (child, tag)));
    stack[first..].reverse();
    &stack[first..]
}

pub(crate) fn metrics_inner<T: ParserTrait>(
    parser: &T,
    name: Option<String>,
    options: MetricsOptions,
) -> Result<FuncSpace, MetricsError> {
    // bca: suppress(cognitive, abc)
    // The single AST-walk loop. Per-node work is already factored into
    // push_synthetic_unit_root / finalize / open_func_space /
    // compute_per_node / apply_comment_suppression / push_children; the
    // residual branches each guard a distinct walk invariant
    // (#182/#289/#522/#722/#1084). There is no cohesive sub-loop left to
    // lift without inventing a `walk_part2`.
    // The suppression-warning diagnostic uses the caller-supplied
    // name when present; otherwise we fall back to a placeholder so
    // the warning still locates the offending line. All path-based
    // shims pass a lossy-stringified path here, matching pre-#254
    // behaviour byte-for-byte.
    let diagnostic_path = name.as_deref().unwrap_or("<input>");
    let selected = options.metrics;
    let code = parser.code();
    let node = parser.root();
    let mut cursor = node.cursor();
    let mut stack = Vec::new();
    // Ancestor chain of the node currently being visited, root first.
    // Maintained so per-node predicates can read an ancestor as a slice
    // index instead of through `Node::parent`, which `tree_sitter`
    // resolves by descending from the root (#1084).
    let mut chain: Vec<Node<'_>> = Vec::new();
    let mut state_stack: Vec<State> = Vec::new();
    let mut last_level = 0;
    // Per-node cognitive nesting, inherited down the walk. Deliberately
    // not pre-seeded with the root: `get_nesting_from_map` already falls
    // back to `Nesting::default()`, so a seed would change nothing for
    // grammars that compute cognitive — while for the two whose impl is
    // the macro's no-op it is the one write that would make the walk
    // build an entry per node that nothing ever reads.
    //
    // Sized up front rather than grown: every real `Cognitive::compute`
    // ends by writing its own node's slot, so the map converges on one
    // entry per visited node and a default-capacity map rehashes its way
    // there a doubling at a time. `descendant_count` is that final size,
    // known in O(1) — an upper bound rather than an exact one only when
    // `exclude_tests` prunes a subtree the walk never descends into.
    //
    // Both guards exist to keep an empty map unallocated: an unselected
    // `Cognitive` never calls `compute` at all, and the two grammars
    // whose impl is the macro's no-op (`Preproc`, `Ccomment`) report
    // `SEEDS_NESTING = false` because they write no slot.
    let mut nesting_map = if selected.contains(Metric::Cognitive)
        && <T::Cognitive as Cognitive>::SEEDS_NESTING
    {
        NestingMap::with_capacity_and_hasher(node.descendant_count(), BuildHasherDefault::default())
    } else {
        NestingMap::default()
    };

    // Suppression markers are resolved inline during the walk rather
    // than queued for a post-finalize pass. When we visit a comment
    // node, the active `state_stack` already encodes the comment's
    // syntactic context: the topmost `SpaceKind::Function` entry is
    // the *innermost enclosing function* by construction, with no
    // ambiguity when sibling functions share a source line (issue
    // #289). The root `Unit` state — always at index 0 once the walk
    // has visited the AST root — owns file-scoped markers.

    push_synthetic_unit_root::<T>(&mut state_stack, &node, code, selected);

    stack.push((
        node,
        Walk {
            level: 0,
            depth: 0,
            in_comment: false,
        },
    ));

    while let Some((
        node,
        Walk {
            level,
            depth,
            in_comment,
        },
    )) = stack.pop()
    {
        // The ancestors of the node about to be visited, root first.
        // Pre-order guarantees every one of them has already been
        // visited and appended, so truncating to `depth` drops the
        // sibling subtree we just finished and leaves exactly this
        // node's chain (#1084). Correcting it here rather than on the
        // way out also keeps it right across the `continue` below.
        chain.truncate(depth);

        // Close any spaces left open by a deeper, already-walked subtree
        // before doing anything else with this node. This must run before
        // the test-subtree prune below so that, when we skip a pruned
        // node, `state_stack.last_mut()` is the node's true enclosing
        // space (#722) — not a sibling's still-open function/impl space.
        if level < last_level {
            finalize::<T>(&mut state_stack, last_level - level, selected);
            last_level = level;
        }

        // Bound above the prune because the prune reads it too: Rust's
        // hook finds the `#[…]` run before an item through the parent,
        // and `chain` is already correct here — the truncate above is
        // the only thing that touches it between the pop and this
        // point (#1100).
        let ancestors = Ancestors::checked(&chain, &node);

        // Prune test-only subtrees before any per-metric work runs.
        // The hook is gated on `exclude_tests` so the default
        // `metrics()` entry point keeps emitting the pre-#182
        // numbers byte-for-byte.
        if options.exclude_tests && T::Checker::should_skip_subtree(&node, code, ancestors) {
            // `sloc` is span-based, not node-accumulated, so unlike every
            // other loc sub-metric it does not shrink just because we
            // skip the subtree. Record the pruned node's row span on the
            // innermost enclosing func-space so its `sloc` drops in step
            // (#722); `Sloc::merge` then folds that count upward so every
            // enclosing space — including the unit, which feeds MI's SLOC
            // term — drops too, even when the test item is nested in a
            // retained `impl`/`trait`/closure (#741). Gated on the `Loc`
            // selection so deselecting loc keeps the walk's work identical.
            if selected.contains(Metric::Loc)
                && let Some(state) = state_stack.last_mut()
            {
                state
                    .space
                    .metrics
                    .loc
                    .exclude_test_span(node.start_row(), node.end_line());
            }
            continue;
        }

        let func_space = T::Checker::promotes_to_func_space_with_code(&node, code, ancestors);

        let new_level = if func_space {
            last_level =
                open_func_space::<T>(&mut state_stack, &node, code, ancestors, level, selected);
            last_level
        } else {
            level
        };

        // Computed once and reused: suppression needs it for this node,
        // and the children need it to inherit comment membership (#1052).
        let is_comment = T::Checker::is_comment(&node);

        // Pin each suppression marker to its innermost enclosing
        // function space (issue #289); see `apply_comment_suppression`.
        // Deliberately called before `subtree_in_comment` is bound: the
        // two are adjacent same-typed `bool`s, and passing the inclusive
        // one here would re-apply a marker once per descendant leaf.
        apply_comment_suppression(&mut state_stack, &node, code, diagnostic_path, is_comment);

        let subtree_in_comment = in_comment || is_comment;

        if let Some(state) = state_stack.last_mut() {
            compute_per_node::<T>(
                state,
                &node,
                code,
                options,
                NodeFacts {
                    func_space,
                    in_comment: subtree_in_comment,
                },
                ancestors,
                &mut nesting_map,
            );
        }

        chain.push(node);

        let pushed = push_children(
            &mut cursor,
            &node,
            Walk {
                level: new_level,
                depth: depth + 1,
                in_comment: subtree_in_comment,
            },
            &mut stack,
        );

        if selected.contains(Metric::Cognitive) {
            propagate_nesting_to_children(&node, pushed, &mut nesting_map);
        }
    }

    finalize::<T>(&mut state_stack, usize::MAX, selected);

    // Reserved error path: `MetricsError::EmptyRoot` is unreachable
    // today because the synthetic Unit push above (and every
    // language's translation_unit / module / source_file being a
    // `func_space`) keeps the state stack non-empty for every input,
    // including empty / whitespace-only / comment-only sources. The
    // `ok_or` is retained so a future walker change that legitimately
    // drains the stack surfaces a distinct error variant rather than
    // panicking or returning a bare `None`. See `MetricsError::EmptyRoot`
    // for the matching variant doc.
    let mut state = state_stack.pop().ok_or(MetricsError::EmptyRoot)?;
    state.space.name = name;
    Ok(state.space)
}

pub(super) fn apply_suppression(state_stack: &mut [State], suppression: &Suppression) {
    // Both arms ultimately call `merge` on a `FuncSpace::suppressed`;
    // they differ only in *which* frame on the stack to target.
    //
    // - `File`: the topmost `Unit` frame — by construction the root
    //   `state_stack[0]`, but we match on `SpaceKind::Unit` rather
    //   than index 0 so the invariant is runtime-checked. The
    //   synthetic Unit pushed by `metrics_inner` for non-Unit-root
    //   grammars and every translation-unit/module/source-file being
    //   a `func_space` keep `state_stack[0]` populated for every
    //   input; a marker with no Unit frame on the stack would be a
    //   bug elsewhere and is silently dropped rather than landing on
    //   an arbitrary frame.
    // - `Function`: the topmost `SpaceKind::Function` frame — the
    //   syntactically nearest enclosing function body. Class / struct
    //   / trait spaces are skipped so a marker at class scope but
    //   outside any method does not silence thresholds on the entire
    //   class; authors who want class-wide suppression use `bca:
    //   suppress-file` or repeat the marker on each method. A marker
    //   outside every function body finds no `Function` frame and is
    //   silently dropped — the issue's "no enclosing function" rule.
    let target = match suppression.kind {
        SuppressionKind::File => state_stack
            .iter_mut()
            .find(|s| matches!(s.space.kind, SpaceKind::Unit)),
        SuppressionKind::Function => state_stack
            .iter_mut()
            .rev()
            .find(|s| matches!(s.space.kind, SpaceKind::Function)),
    };
    if let Some(state) = target {
        state.space.suppressed.merge(&suppression.scope);
    }
}
