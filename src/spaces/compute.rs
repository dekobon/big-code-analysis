//! Metric-computation and AST-traversal internals for [`super::analyze`].
//!
//! These free functions were split out of `spaces.rs` to keep that
//! module focused on the public API types (`SpaceKind`, `CodeMetrics`,
//! `FuncSpace`, `Source`, `Ast`, `MetricsOptions`). They are moved
//! verbatim and re-exported from the parent so the public path
//! `crate::spaces::analyze` (and `pub(crate) metrics_inner`) is preserved.

use super::*;

#[inline]
fn compute_halstead_mi_and_wmc<T: ParserTrait>(state: &mut State, selected: MetricSet) {
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
    if selected.contains(Metric::Wmc) {
        T::Wmc::compute(
            state.space.kind,
            &state.space.metrics.cyclomatic,
            &mut state.space.metrics.wmc,
        );
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

/// Runs the four per-space finalization passes (min/max, sum, Halstead +
/// MI + WMC, averages) on a single [`State`]. Shared by both the
/// single-element and pop arms of [`finalize`] so the call sequence stays
/// identical in both. The pop arm performs an *additional* Halstead
/// recompute on the parent after merging the child's maps — that extra
/// pass is intentionally left in [`finalize`], not folded in here.
fn finalize_state<T: ParserTrait>(state: &mut State, selected: MetricSet) {
    compute_minmax(state, selected);
    compute_sum(state, selected);
    compute_halstead_mi_and_wmc::<T>(state, selected);
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
        compute_halstead_mi_and_wmc::<T>(last_state, selected);

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
/// consume. Bundled rather than passed as three loose `bool`s so the
/// call site cannot transpose them — they are all same-typed flags
/// about the node currently being visited.
#[derive(Clone, Copy)]
struct NodeFacts {
    /// This node opens a new [`FuncSpace`].
    func_space: bool,
    /// This node's space kind is [`SpaceKind::Unit`].
    unit: bool,
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
    nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
) {
    let NodeFacts {
        func_space,
        unit,
        in_comment,
    } = facts;
    let selected = options.metrics;
    let last = &mut state.space;
    if selected.contains(Metric::Cognitive) {
        T::Cognitive::compute(node, code, &mut last.metrics.cognitive, nesting_map);
    }
    if selected.contains(Metric::Cyclomatic) {
        T::Cyclomatic::compute_with_options(
            node,
            code,
            &mut last.metrics.cyclomatic,
            options.count_cyclomatic_try,
        );
    }
    if selected.contains(Metric::Halstead) {
        T::Halstead::compute(node, code, &mut state.halstead_maps);
    }
    if selected.contains(Metric::Loc) {
        T::Loc::compute(node, &mut last.metrics.loc, func_space, unit);
    }
    if selected.contains(Metric::Nom) {
        T::Nom::compute(node, code, &mut last.metrics.nom);
    }
    if selected.contains(Metric::Tokens) {
        T::Tokens::compute(node, &mut last.metrics.tokens, in_comment);
    }
    if selected.contains(Metric::Nargs) {
        T::NArgs::compute(node, &mut last.metrics.nargs);
    }
    if selected.contains(Metric::Nexits) {
        T::Exit::compute(node, code, &mut last.metrics.nexits);
    }
    if selected.contains(Metric::Abc) {
        T::Abc::compute(node, code, &mut last.metrics.abc);
    }
    if selected.contains(Metric::Npm) {
        T::Npm::compute(node, code, &mut last.metrics.npm);
    }
    if selected.contains(Metric::Npa) {
        T::Npa::compute(node, code, &mut last.metrics.npa);
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
fn push_synthetic_unit_root<T: ParserTrait>(
    state_stack: &mut Vec<State>,
    node: &Node,
    code: &[u8],
    selected: MetricSet,
) {
    if T::Getter::get_space_kind_with_code(node, code) != SpaceKind::Unit {
        let mut synthetic = FuncSpace::new::<T::Getter>(node, code, SpaceKind::Unit, selected);
        synthetic
            .metrics
            .loc
            .init_unit_span(node.start_row(), node.end_row());
        state_stack.push(State {
            space: synthetic,
            halstead_maps: HalsteadMaps::new(),
        });
    }
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
/// A malformed marker is logged and dropped (no scope attached) rather
/// than aborting the walk: a typo in one file must not derail a
/// workspace-wide pass, and dropping is the conservative choice — a typo
/// should not accidentally silence anything.
fn apply_comment_suppression(
    state_stack: &mut Vec<State>,
    node: &Node,
    code: &[u8],
    diagnostic_path: &str,
    is_comment: bool,
) {
    if is_comment && let Some(text) = node.utf8_text(code) {
        match parse_suppression_marker(text) {
            Ok(Some(s)) => apply_suppression(state_stack, &s),
            Ok(None) => {}
            Err(e) => {
                // The `+ 1` converts tree-sitter's 0-based rows to the
                // 1-based line numbers `FuncSpace::start_line` and the
                // rest of this module report.
                eprintln!("warning: {}:{}: {e}", diagnostic_path, node.start_row() + 1);
            }
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
/// `Cognitive` can read the triple it inherits without calling
/// `Node::parent`.
///
/// A node's slot means two different things either side of its
/// `compute`, and the distinction is load-bearing:
///
/// - **on entry** it holds what the node inherits — this is what
///   `get_nesting_from_map` reads;
/// - **on exit** each language's `compute` has overwritten it with the
///   post-increment triple its children should see — this is what this
///   function hands down.
///
/// So the write at the end of every `Cognitive::compute` must stay at the
/// end. Moving it to the top would make every descendant inherit the
/// pre-increment triple and silently under-count.
///
/// `or_insert`, not `insert`: Python's comprehension handling pre-writes
/// its clause children's slots during the *comprehension's* `compute`
/// (the #421 fix, so clause nesting does not depend on sibling traversal
/// order), and those values deliberately differ from the comprehension's
/// own triple. A blanket overwrite would clobber them — the
/// `python_comprehension_*` tests in `metrics::cognitive` are what catch
/// it.
///
/// Grammars whose `Cognitive` impl is the macro's no-op (`Preproc`,
/// `Ccomment`) never write a slot, so they inherit the root's `(0, 0, 0)`
/// unchanged. That matches the pre-#1062 behaviour only because zero is
/// the identity here; it costs them a map entry per node that nothing
/// reads.
fn propagate_nesting_to_children(
    node: &Node,
    children: &[(Node<'_>, Walk)],
    nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
) {
    // Leaves are roughly half of a real AST, so bail before hashing a key
    // we would only read to iterate zero children.
    if children.is_empty() {
        return;
    }
    // Unreachable in practice: the root is seeded before the walk and every
    // other node is seeded by this function before it is popped. Degrading
    // to "no inheritance" would silently zero a whole subtree's nesting, so
    // fail loudly in debug rather than absorb it.
    let Some(&inherited) = nesting_map.get(&node.id()) else {
        debug_assert!(false, "node {} has no nesting slot", node.id());
        return;
    };
    for (child, _) in children {
        nesting_map.entry(child.id()).or_insert(inherited);
    }
}

/// Pushes `node`'s direct children onto the traversal `stack`, each tagged
/// with `tag`.
///
/// The `children.drain(..).rev()` ordering is load-bearing: it makes the
/// LIFO `stack` yield children in source order, which in turn governs
/// line-shared suppression attribution (issue #289). The `children`
/// scratch buffer is drained empty here so callers can reuse its
/// allocation across iterations.
///
/// `Tag` is generic because the two walkers carry different context down
/// the tree: `ops` needs only the nesting level, while `metrics_inner`
/// also propagates comment membership ([`Walk`], issue #1052).
pub(crate) fn push_children<'a, Tag: Copy>(
    cursor: &mut Cursor<'a>,
    node: &Node<'a>,
    tag: Tag,
    children: &mut Vec<(Node<'a>, Tag)>,
    stack: &mut Vec<(Node<'a>, Tag)>,
) {
    cursor.reset(node);
    if cursor.goto_first_child() {
        loop {
            children.push((cursor.node(), tag));
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        for child in children.drain(..).rev() {
            stack.push(child);
        }
    }
}

pub(crate) fn metrics_inner<T: ParserTrait>(
    parser: &T,
    name: Option<String>,
    options: MetricsOptions,
) -> Result<FuncSpace, MetricsError> {
    // bca: suppress(cognitive, abc)
    // The single AST-walk loop. Per-node work is already factored into
    // push_synthetic_unit_root / finalize / compute_per_node /
    // apply_comment_suppression / push_children; the residual branches
    // each guard a distinct walk invariant (#182/#289/#522/#722). There
    // is no cohesive sub-loop to lift without inventing a `walk_part2`.
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
    let mut children = Vec::new();
    let mut state_stack: Vec<State> = Vec::new();
    let mut last_level = 0;
    // Initialize nesting_map used for storing nesting information for cognitive
    // Three type of nesting info: conditionals, functions and lambdas
    let mut nesting_map = HashMap::<usize, (usize, usize, usize)>::default();
    nesting_map.insert(node.id(), (0, 0, 0));

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
            in_comment: false,
        },
    ));

    while let Some((node, Walk { level, in_comment })) = stack.pop() {
        // Close any spaces left open by a deeper, already-walked subtree
        // before doing anything else with this node. This must run before
        // the test-subtree prune below so that, when we skip a pruned
        // node, `state_stack.last_mut()` is the node's true enclosing
        // space (#722) — not a sibling's still-open function/impl space.
        if level < last_level {
            finalize::<T>(&mut state_stack, last_level - level, selected);
            last_level = level;
        }

        // Prune test-only subtrees before any per-metric work runs.
        // The hook is gated on `exclude_tests` so the default
        // `metrics()` entry point keeps emitting the pre-#182
        // numbers byte-for-byte.
        if options.exclude_tests && T::Checker::should_skip_subtree(&node, code) {
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
                    .exclude_test_span(node.start_row(), node.end_row());
            }
            continue;
        }

        let func_space = T::Checker::promotes_to_func_space_with_code(&node, code);

        // `kind` is consumed in exactly two places: `FuncSpace::new`
        // (only when `func_space` is true) and the `unit` flag, which
        // flows solely into `Loc::compute` (only when `Loc` is
        // selected). For some languages — notably Elixir, whose
        // `get_space_kind_with_code` runs a per-`Call` source-text
        // keyword scan — this lookup is far from a cheap enum compare,
        // so we skip it entirely when neither consumer is active.
        // When it IS computed it returns the same value as before, so
        // both consumers observe byte-identical results (issue #522).
        let kind = if func_space || selected.contains(Metric::Loc) {
            T::Getter::get_space_kind_with_code(&node, code)
        } else {
            // Unused on this path: `func_space` is false (so
            // `FuncSpace::new` is not called) and `Loc` is deselected
            // (so the `unit` flag below is never read by `Loc::compute`).
            SpaceKind::Unknown
        };
        let unit = kind == SpaceKind::Unit;

        let new_level = if func_space {
            let state = State {
                space: FuncSpace::new::<T::Getter>(&node, code, kind, selected),
                halstead_maps: HalsteadMaps::new(),
            };
            state_stack.push(state);
            last_level = level + 1;
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
                    unit,
                    in_comment: subtree_in_comment,
                },
                &mut nesting_map,
            );
        }

        let first_child = stack.len();

        push_children(
            &mut cursor,
            &node,
            Walk {
                level: new_level,
                in_comment: subtree_in_comment,
            },
            &mut children,
            &mut stack,
        );

        if selected.contains(Metric::Cognitive) {
            propagate_nesting_to_children(&node, &stack[first_child..], &mut nesting_map);
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
