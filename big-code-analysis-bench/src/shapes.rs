//! Synthetic scaling inputs, and the probes built from them.
//!
//! # The two axes
//!
//! A tree grows in two directions, and a walk can be linear in one
//! while being quadratic in the other. [`Axis::Depth`] shapes nest: the
//! size parameter is the number of AST levels. [`Axis::Width`] shapes
//! do not nest at all; the size parameter is the number of siblings
//! under one fixed-depth parent. #1100 is why both are here — the fix
//! it originally proposed was linear on every nesting shape and made a
//! flat 2 000-item file 94x slower by that issue's measurement, which
//! no depth probe can see.
//!
//! # Why every generator is affine in its size
//!
//! A generator that indents each nesting level makes the *input* grow
//! quadratically, so a walk that is perfectly linear in bytes still
//! looks superlinear in depth. That mistake invalidated two published
//! measurements during the #1052 / #1062 work before it was spotted.
//! Every generator here therefore emits a constant number of bytes per
//! level (or per sibling) and no indentation at all, which the
//! `byte_growth_is_affine` unit test below pins, rather than leaving it
//! a convention someone has to remember.

use std::fmt::Write as _;
use std::hint::black_box;

use big_code_analysis::{Ast, CodeMetrics, LANG, Metric, MetricsError, MetricsOptions, Ops};

/// Renders a source shape at a given size — a nesting depth on
/// [`Axis::Depth`], a sibling count on [`Axis::Width`].
///
/// Every implementation must be affine in that size: `len(n)` is
/// `base + per_unit * n`. See the module docs for why.
pub type Render = fn(usize) -> String;

/// Rust: `fn f() -> i32 { (((…1…))) }`.
///
/// The paren chain is the shape that first surfaced both the `tokens`
/// per-leaf ancestor walk (#1052) and the `cognitive` nesting lookup
/// (#1062): it is pure nesting with two leaves per level and no
/// statement structure to dilute the walk.
#[must_use]
pub fn nested_parens(depth: usize) -> String {
    format!(
        "fn f() -> i32 {{ {}1{} }}\n",
        "(".repeat(depth),
        ")".repeat(depth)
    )
}

/// Python: `x = (a and (a and (… a …)))`.
///
/// The linear control for [`nested_ternaries`]: one `and` operator per
/// nesting level, which `Cyclomatic` counts from the token's own kind.
/// Python's block syntax cannot nest statements without indenting them,
/// which would make the input grow quadratically; parenthesised
/// expressions are the only Python shape that nests at constant bytes
/// per level.
#[must_use]
pub fn nested_ands(depth: usize) -> String {
    format!("x = {}a{}\n", "(a and ".repeat(depth), ")".repeat(depth))
}

/// Python: `x = (1 if a else (1 if a else … 1 …))`.
///
/// One `else` token per nesting level. Python's `Cyclomatic` asks each
/// one whether it opens a `for` / `while` / `try` else-clause — a
/// two-link parent/grandparent test that short-circuits on the first
/// link here, because a conditional expression's `else` has no
/// `else_clause` parent. That one link was still `O(depth)` while it
/// went through `Node::parent`, which is what made the shape quadratic
/// before #1096; it now indexes the walker's ancestor chain.
/// [`nested_ands`] is the same nesting through an arm that never looks
/// up.
#[must_use]
pub fn nested_ternaries(depth: usize) -> String {
    format!(
        "x = {}1{}\n",
        "(1 if a else ".repeat(depth),
        ")".repeat(depth)
    )
}

/// C: `int main(){ while (a) { … 1; … } }`.
///
/// The linear control for [`nested_ifs`]: structurally identical, but
/// `while_statement` has no `Checker::is_else_if` predicate hanging off
/// it, so it exercises the nesting map without the per-node
/// `Node::parent` call.
#[must_use]
pub fn nested_whiles(depth: usize) -> String {
    format!(
        "int main(){{ {}1;{} }}\n",
        "while (a) { ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// C: `int main(){ if (a) { … 1; … } }`.
///
/// `Checker::is_else_if` asks every `if_statement` whether its parent
/// is an `else` clause. It reads that parent off the ancestor chain the
/// walker hands down, so the answer is `O(1)`; recovering it with
/// `Node::parent` instead costs `O(depth)`, because `tree_sitter` stores
/// no parent pointer and resolves a parent by descending from the root.
/// This probe is what keeps that regression (#1084) from returning.
#[must_use]
pub fn nested_ifs(depth: usize) -> String {
    format!(
        "int main(){{ {}1;{} }}\n",
        "if (a) { ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// Rust: `fn f(x: bool) -> bool { !!!…x }`.
///
/// One `!` per nesting level, and `Getter::get_op_type`'s `BANG` arm
/// asks each one whether its parent is an `inner_doc_comment_marker`
/// (`//!`). That parent now comes off the walker's ancestor chain;
/// calling `Node::parent` instead is `O(depth)` each, which made
/// `Halstead` quadratic in nesting depth for the six grammars whose
/// operator classification consults a parent (#1096).
/// [`nested_parens`] is the same nesting through an arm that does not.
#[must_use]
pub fn nested_nots(depth: usize) -> String {
    format!("fn f(x: bool) -> bool {{ {}x }}\n", "!".repeat(depth))
}

/// C: `int main(){ int x; { x = 1; { x = 1; … } } }`.
///
/// The linear control for [`nested_ifs`] under `Abc`: one
/// `assignment_expression` per nesting level, which the metric counts
/// from the node's own kind. No `if` / `while` head means no condition
/// slot, so the C-family container walker — the one that reads the
/// slot's parent to decide whether it sits in boolean context — is
/// never entered.
#[must_use]
pub fn nested_blocks(depth: usize) -> String {
    format!(
        "int main(){{ int x; {}x = 1;{} }}\n",
        "{ x = 1; ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// C: `int main(){ while (a) { int x; … } }`.
///
/// One `declaration` per nesting level. `loc`'s C-family arm resolves
/// each declaration's logical-line contribution with
/// `Node::count_specific_ancestors`, whose `stop` predicate is
/// `compound_statement` — the brace directly above every declaration
/// here. The probe pins two things at once: that the stop predicate
/// keeps the walk to one step per declaration, and that the step reads
/// the walker's ancestor chain rather than calling `Node::parent`
/// (#1084). Losing either turns this shape quadratic.
#[must_use]
pub fn nested_declarations(depth: usize) -> String {
    format!(
        "int main(){{ {}1;{} }}\n",
        "while (a) { int x; ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// Elixir: a module of nested `quote` blocks, each wrapping a `def`.
///
/// Elixir's `is_func` asks `elixir_is_inside_quote_block` for every
/// `def`-like call, because a `def` inside `quote` is quoted AST
/// rather than a definition. That predicate walks the ancestor chain
/// reading each ancestor's call keyword out of the source bytes. It
/// short-circuits on the first `quote` ancestor, so a `def` directly
/// inside one costs `O(1)` — provided the steps come off the walker's
/// chain and not from `Node::parent`, which is `O(depth)` each (#1084).
/// Lose either the short-circuit or the chain and the shape turns
/// quadratic.
///
/// One `def` sits *outside* the nesting so the metric has something
/// to count — every `def` inside a `quote` is correctly not a
/// function, which would otherwise make the reading zero at every
/// depth.
#[must_use]
pub fn nested_quotes(depth: usize) -> String {
    format!(
        "defmodule M do\ndef g do\n:ok\nend\n{}:ok\n{}end\n",
        "quote do\ndef f do\n:ok\nend\n".repeat(depth),
        "end\n".repeat(depth)
    )
}

/// Rust: `fn f() { if a {} fn f() { … let x = 1; … } }`.
///
/// Each level opens a `FuncSpace`, so this drives the space-nesting
/// bookkeeping and the recursive `FuncSpace` tree that #1056 had to
/// bound. Inner `fn f` shadows are legal — each sits in its own block
/// scope.
///
/// The `if` is what makes the shape readable by `cognitive`. A chain of
/// bare functions carries no cognitive weight at all, so the metric
/// would score zero at every depth; with one `if` per level the reading
/// is `n(n+1)/2`, because `increment_function_depth` gives the function
/// at level *k* a function-nesting depth of *k* and every `if` is
/// penalised by the depth of the function holding it. That makes the
/// value column sensitive to the function-depth walk this probe times
/// (#1062), not only to the walk's cost.
#[must_use]
pub fn nested_fns(depth: usize) -> String {
    format!(
        "{}let x = 1;{}\n",
        "fn f() { if a {} ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// Rust: [`nested_fns`] with one nesting level per source row.
///
/// The same space nesting, spread over rows instead of packed onto one.
/// That is what makes it a `loc` probe: `Ploc` / `Cloc` keep a set of
/// physical rows per space and union each child's into its parent, so
/// here the set folded upward at level *k* holds `O(depth - k)` rows
/// where [`nested_fns`] folds a single row at every level (#1109).
///
/// Still affine in depth — nine bytes for the opening row, two for the
/// closing one, and no indentation, per the module docs.
#[must_use]
pub fn nested_fns_by_row(depth: usize) -> String {
    format!(
        "{}let x = 1;\n{}",
        "fn f() {\n".repeat(depth),
        "}\n".repeat(depth)
    )
}

/// JavaScript: `function f() { function f() { … 1; … } }`.
///
/// The linear control for [`nested_arrows`]: one function per nesting
/// level and one `FuncSpace` per level, as there, but *declared* rather
/// than written as an expression. `function_declaration` is a distinct
/// grammar production, so `Checker::is_func` answers it from the node's
/// own kind and never starts the ancestor walk. What is left is the
/// space-nesting bookkeeping the two shapes share.
///
/// Named for the distinction that matters — declared vs expression —
/// rather than for the language, so it does not read as a variant of
/// [`nested_fns`], which is the Rust shape.
#[must_use]
pub fn nested_declared_functions(depth: usize) -> String {
    format!(
        "{}1;{}\n",
        "function f() { ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// Rust: `#[inline] fn f() { #[inline] fn f() { … } }`.
///
/// [`nested_fns`] with an outer attribute on every function, which is
/// what makes it an `exclude_tests` probe: the prune hook only scans
/// for a preceding `#[…]` run once the node's kind is an item kind, so
/// a shape without attributes leaves the scan with nothing to walk. The
/// attribute is `#[inline]` rather than `#[test]` on purpose — a test
/// attribute prunes the outermost function and the walk stops there,
/// timing nothing (#1100).
///
/// Still affine in depth, and no `if`: the reading comes from `nom`, so
/// nothing here needs cognitive weight.
#[must_use]
pub fn nested_attributed_fns(depth: usize) -> String {
    format!(
        "{}let x = 1;{}\n",
        "#[inline] fn f() { ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// Rust: `#[inline] fn f() {} #[inline] fn f() {} …`, all at file
/// scope.
///
/// [`nested_attributed_fns`] rotated onto the width axis: the same
/// `exclude_tests` attribute scan, but the attributed items are
/// siblings of one another rather than nested, so the size parameter
/// is the parent's child count and the AST depth never moves. This is
/// the shape #1100's rejected fix was quadratic on — an unconditional
/// forward pass over the parent's children costs `O(children)` per
/// item, which on a flat file is `O(n^2)`; #1100 measured 94x on
/// 2 000 items. It is also the shape `bindgen` output has.
///
/// Duplicate `fn f` names are a type-check error, not a parse error,
/// so the grammar accepts the repetition and the walk sees exactly the
/// shape it would on distinctly-named items.
#[must_use]
pub fn wide_attributed_fns(width: usize) -> String {
    format!("{}\n", "#[inline] fn f() {} ".repeat(width))
}

/// Rust: `fn f000000() { let v000000 = 000000; } fn f000001() { … }`,
/// all at file scope.
///
/// The width shape for the space-merge arm. Each function opens its own
/// `FuncSpace`, so the size parameter is the number of *direct children*
/// the file's `Unit` space accumulates — the `C` in the `O(C x U)` that
/// #1106 was filed about, where `U` is the parent's merged Halstead
/// vocabulary.
///
/// Every identifier and literal is unique to its function, which is what
/// makes `U` grow with `C` rather than saturate: `HalsteadMaps::operands`
/// is keyed by source text, so a file of N identical functions has a
/// vocabulary of constant size and would keep the merge arm linear no
/// matter how it is written.
///
/// Six-digit zero padding, not the bare index, so the bytes per function
/// stay constant up to a million siblings. `byte_growth_is_affine` does
/// **not** cover this: it samples 10 / 20 / 30, where the unpadded form
/// is affine too. What the padding buys is affine growth across the 4-
/// to 5-digit boundary, which this probe's own ladder crosses at 10 000
/// — unpadded, 4 000 / 8 000 / 16 000 render 128 670 / 260 670 / 542 670
/// bytes and inflate the fitted exponent by ~0.06 with nothing in
/// `mod tests` able to see it.
///
/// Rust permits leading zeros in a decimal literal, so `000000` is an
/// ordinary `integer_literal` and not an octal escape or a parse error.
#[must_use]
pub fn wide_distinct_fns(width: usize) -> String {
    let mut source = String::new();
    for i in 0..width {
        // `fmt::Write for String` never returns `Err`, so this is the
        // one shape whose generator has a `Result` to discard. The
        // alternatives clippy leaves are `format!`-into-`push_str` and
        // `map(format!).collect()`, and it rejects both.
        let _ = writeln!(source, "fn f{i:06}() {{ let v{i:06} = {i:06}; }}");
    }
    source
}

/// JavaScript: `function f() { let v000000 = 0, v000001 = 1, …; return
/// v000000; }` — one `let` statement with `width` declarators.
///
/// The width shape for the JS-family ABC `const` predicate (#1277).
/// Each `=` asks whether its declarator sits under a `const`
/// declaration, and the first form of that predicate answered by
/// scanning every sibling declarator for the keyword — `O(width)` per
/// `=`, so `O(width²)` per statement, invisible to every depth probe.
/// `let` is the spelling that pays it in full: `const` finds its
/// keyword at child 0 and `var` opens no `lexical_declaration` at all.
/// Six-digit padding for the reason `wide_distinct_fns` gives.
#[must_use]
pub fn wide_let_declarators(width: usize) -> String {
    let mut source = String::from("function f() { let ");
    for i in 0..width {
        if i > 0 {
            source.push_str(", ");
        }
        let _ = write!(source, "v{i:06} = {i:06}");
    }
    source.push_str("; return v000000; }\n");
    source
}

/// Rust: `#[cfg(all(all(… test …)))] fn gone() {}` plus one retained
/// function.
///
/// Nesting inside one *attribute* rather than in the code: the cfg
/// predicate is classified by a string-level mini-parser reading the
/// attribute's text, so what grows with `depth` is the length of a
/// single attribute while the file keeps its two items. (The token
/// tree nests along with the text, which is what
/// `shapes_nest_proportionally_to_depth` sees.)
///
/// The retained `fn keep` is what gives the probe a non-zero reading:
/// the attributed item is pruned, so a file holding only it scores zero
/// at every depth.
#[must_use]
pub fn nested_cfg_predicate(depth: usize) -> String {
    format!(
        "#[cfg({}test{})] fn gone() {{}}\nfn keep() {{ let x = 1; }}\n",
        "all(".repeat(depth),
        ")".repeat(depth)
    )
}

/// JavaScript: `const f = a => { a => { … 1 … } };`.
///
/// The JS grammars have no production for "named function": `const f =
/// () => …` and `g(() => …)` are the same `arrow_function` node, and
/// `Checker::is_func` tells them apart by walking upward until it meets
/// either a name binding or a frame that proves positional use. Here
/// the enclosing `statement_block` stops the walk after two steps —
/// which is the point. Two steps was still `O(depth)`, because
/// `Node::parent` descends from the root, and the `PropertyIdentifier`
/// adjacency check the predicate ends with scanned siblings the same
/// way; both are why a shape with one arrow per level was quadratic
/// before #1088 despite every walk being O(1) steps long.
///
/// Every level is an arrow function, so `nom` reads `depth`: the
/// outermost is bound to `f` and counts as a function, the rest are
/// closures. [`nested_declared_functions`] is the same nesting without
/// the walk.
#[must_use]
pub fn nested_arrows(depth: usize) -> String {
    format!(
        "const f = {}1{};\n",
        "a => { ".repeat(depth),
        " }".repeat(depth)
    )
}

/// The walk a probe times.
///
/// Two seams reach the AST walker — `Ast::metrics` and `Ast::ops` — and
/// they share no options type, so the selection a metric probe needs
/// lives on the variant that uses it rather than on every probe (#1110).
#[derive(Clone, Copy)]
pub enum Workload {
    /// `Ast::metrics` under `selection`.
    Metrics {
        /// Metric selection handed to `MetricsOptions::with_only`. Kept
        /// as narrow as the probe allows so an unrelated metric's cost
        /// cannot dominate and misattribute a regression.
        selection: &'static [Metric],
        /// Whether the walk runs under `MetricsOptions::exclude_tests`,
        /// which is the only way to reach `Checker::should_skip_subtree`
        /// and the cfg-predicate classifier behind it (#1100). Lives on
        /// this variant rather than on [`Probe`] for the reason
        /// `selection` does: `Ast::ops` takes no options at all, so a
        /// probe that could set it there would be setting something
        /// nothing reads.
        exclude_tests: bool,
        /// Headline value the selection produces on the probe's shape.
        reading: fn(&CodeMetrics) -> u64,
    },
    /// `Ast::ops`, the Halstead operator/operand walk, which takes no
    /// metric selection.
    Ops {
        /// Headline value the walk produces on the probe's shape.
        reading: fn(&Ops) -> u64,
    },
}

impl Workload {
    /// The options this workload walks under, resolved once so option
    /// resolution stays outside the timed region. `Ast::ops` takes none,
    /// so the default stands in and is ignored.
    #[must_use]
    pub fn options(self) -> MetricsOptions {
        match self {
            Self::Metrics {
                selection,
                exclude_tests,
                ..
            } => MetricsOptions::default()
                .with_only(selection)
                .with_exclude_tests(exclude_tests),
            Self::Ops { .. } => MetricsOptions::default(),
        }
    }

    /// Walks `ast` once and returns the headline reading.
    ///
    /// The walk's product is `black_box`ed before the reading is taken,
    /// so an optimiser cannot narrow the walk to whatever the reading
    /// happens to touch.
    ///
    /// # Errors
    ///
    /// Whatever the underlying seam raises — in practice, a language
    /// feature disabled in the build.
    pub fn walk(self, ast: &Ast, options: MetricsOptions) -> Result<u64, MetricsError> {
        Ok(match self {
            Self::Metrics { reading, .. } => reading(&black_box(ast.metrics(options)?).metrics),
            Self::Ops { reading } => reading(&black_box(ast.ops()?)),
        })
    }
}

/// The direction a probe's shape grows in.
///
/// A walk can be linear in one direction and quadratic in the other,
/// so the axis is what says which claim a probe's exponent supports.
/// It also selects the shape invariant the probe must satisfy:
/// `shapes_nest_proportionally_to_depth` for one, and
/// `shapes_widen_without_deepening` for the other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// The size parameter is the shape's nesting depth.
    Depth,
    /// The size parameter is the number of siblings under one parent,
    /// at a depth that does not change with it.
    Width,
}

/// One scaling probe: a shape, the axis it grows along, the workload
/// that exercises the hot path under test, and the complexity class
/// the walk is expected to stay within.
///
/// A probe's `reading` is reported alongside every timing so a reader
/// can see the walk did real work, and asserted non-zero by
/// `probe_workload_is_exercised`: a shape paired with a workload that
/// scores zero on it would time the walk's fixed overhead and report an
/// excellent exponent forever.
#[derive(Clone, Copy)]
pub struct Probe {
    /// Stable `<metric>/<shape>` identifier, used as the report key.
    pub name: &'static str,
    /// Language whose grammar the shape is written for.
    pub lang: LANG,
    /// Direction [`Probe::sizes`] grows the shape in.
    pub axis: Axis,
    /// The walk this probe times, and how its headline reading is taken.
    pub workload: Workload,
    /// Generator for the probe's input.
    pub render: Render,
    /// The three sizes measured — the values handed to [`Probe::render`]
    /// — each a doubling of the previous so the fitted exponent reads
    /// directly as "cost per doubling".
    pub sizes: [usize; 3],
    /// Upper bound on the fitted log-log exponent. A linear walk sits
    /// near 1.0 and a quadratic one near 2.0; the bounds below leave
    /// enough headroom that measurement noise cannot cross them but
    /// not enough that a class change hides.
    pub max_exponent: f64,
    /// What the probe covers, and why its bound is where it is.
    pub rationale: &'static str,
}

/// Depths for the probes whose walk is expected to be linear.
///
/// Large enough that the walk dominates the fixed per-analysis cost
/// (option resolution, root `FuncSpace` construction), small enough
/// that a *quadratic* regression is abandoned partway up the ladder
/// rather than run to completion. Bounding the total cost of such a
/// regression is `MAX_CELL_WALK`'s job, not this constant's: at these
/// depths a pre-#1052-magnitude blow-up costs a couple of minutes
/// before the budget fires, which is a report rather than the hung
/// job the retired wall-clock assertions were guarding against.
const LINEAR_DEPTHS: [usize; 3] = [1_000, 2_000, 4_000];

/// Sibling counts for the probes whose walk is expected to be linear
/// in a parent's child count.
///
/// Reasoned independently of [`LINEAR_DEPTHS`] rather than copied: a
/// width shape spends nothing on nesting, so the same numbers would
/// buy a different amount of walk. The top of the ladder is the scale
/// #1100 measured its 94x regression at — a flat file of 2 000
/// attributed items. The bottom is set from the measurement below it:
/// the 500-wide cell runs 1.86 ms, an order of magnitude above the
/// cheapest cells in the set (~0.2 ms), so fixed per-analysis cost is
/// a rounding error in the fit rather than a term flattening it.
const LINEAR_WIDTHS: [usize; 3] = [500, 1_000, 2_000];

/// Sibling counts for a width probe whose quadratic term is a *second*
/// pass over data the linear walk already touched, rather than extra
/// work per node.
///
/// Eight times [`LINEAR_WIDTHS`], and deliberately not harmonised down
/// to it. #1106's redundant pass costs one map visit per (child,
/// vocabulary-entry) pair while the walk it rides on costs a fixed
/// amount per token, so the ratio between the two terms is set by the
/// sibling count alone — enriching each function's vocabulary raises
/// both terms together and does not move it. On [`LINEAR_WIDTHS`] the
/// regression fits 1.37-1.39 against a 1.07-1.09 baseline: a real 2.1x
/// slowdown at 2 000 siblings that the bound would have passed. Here
/// the gate reads 1.09 clean and 2.17 regressed.
///
/// The taller ladder costs ~0.8 s of a `--gate` run over nine rounds
/// and a 624 KB largest input. A reintroduced quadratic pays ~6 s
/// there, and its slowest single walk — 0.5 s — is well inside
/// `scaling::MAX_CELL_WALK`, so the gate reports the exponent rather
/// than abandoning the probe as over budget.
const SPACE_MERGE_WIDTHS: [usize; 3] = [4_000, 8_000, 16_000];

/// Bound for a probe expected to be linear in its size parameter.
///
/// Set from measurement, not from theory. A genuinely linear walk does
/// not fit exactly 1.0 over these sizes: the tree outgrows cache as
/// depth rises, so per-byte cost drifts up by roughly a quarter from
/// the shallowest cell to the deepest and every depth probe here fits
/// 0.94-1.31 on an idle host. Before #1084 the three ancestor-walk
/// probes fit 1.95-2.01; the midpoint of those two bands is the cut.
///
/// The width probe re-measured onto the same cut rather than
/// inheriting it. `nom/wide-attributed-fn` fits 0.97-1.00, with no
/// per-byte drift up its ladder to spend the headroom on, and #1100's
/// rejected forward-always scan takes it to 1.99 — 560.6 ms against
/// 7.2 ms at 2 000 items, a 78x of its own. One bound still covers
/// the set.
const LINEAR_BOUND: f64 = 1.5;

/// The scaling probe set.
///
/// One entry per hot path identified during the #1052 / #1062 / #1084 /
/// #1096 / #1109 work, plus the controls that make those readings
/// interpretable. Two kinds of control appear:
///
/// - *metric* controls, such as `nom/nested-while` and
///   `nom/nested-fn-rows`: the same shape under a metric that does not
///   run the path under test. `Cognitive` declares `Nom` as a
///   dependency, so the cognitive-attributable cost of a `cognitive/…`
///   row is its difference from the `nom/…` row on the same shape, not
///   the `cognitive` reading alone.
/// - *shape* controls, such as `loc/nested-while` and `loc/nested-fn`:
///   the same metric on the same nesting with the one feature that
///   triggers the path removed. That pairing is what attributed the
///   pre-#1084 quadratic readings to those calls rather than to nesting
///   in general, and it is what would localise a future regression the
///   same way.
///
/// See `docs/development/benchmarking.md` for the per-probe table.
pub const PROBES: &[Probe] = &[
    Probe {
        name: "tokens/nested-paren",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Tokens],
            reading: |m| m.tokens.tokens_sum(),
        },
        render: nested_parens,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1052: `tokens` inherits the in-comment flag down the \
                    traversal. Reverting to the per-leaf ancestor walk is \
                    quadratic in nesting depth.",
    },
    // Deliberately no `cognitive/nested-paren`: parentheses carry no
    // cognitive weight, so the metric scores zero on that shape at
    // every depth and the probe would be timing a walk whose output
    // never changes. #1062's `get_nesting_from_map` is covered by
    // `cognitive/nested-while` below — the shape #1062's own
    // regression test uses, and one whose reading grows as
    // `n(n+1)/2`, so a walk that stopped inheriting nesting is
    // visible in the value column and not only in the timing.
    Probe {
        name: "cognitive/nested-while",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: nested_whiles,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1062 on a statement shape, and the linear control for \
                    `cognitive/nested-if`: identical structure, no \
                    `is_else_if` predicate.",
    },
    Probe {
        name: "nom/nested-while",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_whiles,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Metric control. `Cognitive` declares `Nom` as a \
                    dependency, so the cognitive-attributable cost is the \
                    difference between this row and `cognitive/nested-while`.",
    },
    Probe {
        name: "cognitive/nested-if",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: nested_ifs,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1084: `Checker::is_else_if` reads the enclosing \
                    `else` clause off the walker's ancestor chain. \
                    Recovering it with `Node::parent` is `O(depth)` per \
                    `if_statement`, which was quadratic across the 13 \
                    languages with an `is_else_if` impl.",
    },
    Probe {
        name: "loc/nested-while",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.lloc(),
        },
        render: nested_whiles,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `loc/nested-declaration`: the same \
                    nesting with no `declaration` node, so `loc` never \
                    reaches `Node::count_specific_ancestors`.",
    },
    Probe {
        name: "loc/nested-declaration",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.lloc(),
        },
        render: nested_declarations,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1084: `loc`'s C-family arm calls \
                    `Node::count_specific_ancestors` for every \
                    `declaration`. Its `stop` predicate fires on the \
                    enclosing `compound_statement` immediately, so the \
                    walk is one step, and that step now indexes the \
                    walker's ancestor chain instead of calling \
                    `Node::parent`. `loc/nested-while` is the same \
                    nesting without a declaration.",
    },
    Probe {
        name: "halstead/nested-paren",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.length(),
        },
        render: nested_parens,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `halstead/nested-not`: the same \
                    nesting through `get_op_type` arms that classify a \
                    token from its own kind, with no parent read.",
    },
    Probe {
        name: "halstead/nested-not",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.length(),
        },
        render: nested_nots,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1096: `Getter::get_op_type` asks every `!` token \
                    whether its parent is an inner-doc-comment marker. \
                    The answer now indexes the walker's ancestor chain; \
                    `Node::parent` was `O(depth)` per token across the \
                    six grammars whose operator classification reads a \
                    parent. `halstead/nested-paren` is the same nesting \
                    without the read.",
    },
    Probe {
        name: "abc/nested-block",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.assignments_sum(),
        },
        render: nested_blocks,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `abc/nested-if`: the same nesting \
                    with no condition slot, so the C-family container \
                    walker is never entered.",
    },
    Probe {
        name: "abc/nested-if",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: nested_ifs,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1096: every `if (…)` head routes its condition \
                    through the C-family container walker, which seeds \
                    its boolean-context flag from the slot's parent. \
                    That parent is now passed in by the caller that \
                    descended from it; resolving it with `Node::parent` \
                    was `O(depth)` per `if` across all twenty `Abc` \
                    impls. `abc/nested-block` is the same nesting \
                    without a condition slot.",
    },
    Probe {
        name: "cyclomatic/nested-and",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cyclomatic],
            reading: |m| m.cyclomatic.cyclomatic_sum(),
        },
        render: nested_ands,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `cyclomatic/nested-ternary`: the \
                    same nesting through an arm that counts the token \
                    from its own kind and never looks up.",
    },
    Probe {
        name: "cyclomatic/nested-ternary",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cyclomatic],
            reading: |m| m.cyclomatic.cyclomatic_sum(),
        },
        render: nested_ternaries,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1096: Python's `Cyclomatic` asks every `else` token \
                    whether it opens a loop or `try` else-clause, through \
                    `Node::parent_grandparent_match`. Both links now index \
                    the walker's ancestor chain. `cyclomatic/nested-and` \
                    is the same nesting without the lookup.",
    },
    Probe {
        name: "loc/nested-quote",
        lang: LANG::Elixir,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.lloc(),
        },
        render: nested_quotes,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1096: Elixir's `loc` catch-all arm asks every named \
                    node whether its parent is a statement container, so \
                    unlike the other group-3 arms it fires per node \
                    rather than per construct. The parent now comes off \
                    the walker's ancestor chain. `nom/nested-quote` is \
                    the same shape under a metric that does not ask.",
    },
    Probe {
        name: "nom/nested-quote",
        lang: LANG::Elixir,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_quotes,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1084: Elixir's `is_func` asks \
                    `elixir_is_inside_quote_block` for every `def`. The \
                    predicate short-circuits on the first `quote` \
                    ancestor, and each step now indexes the walker's \
                    ancestor chain rather than calling `Node::parent`.",
    },
    Probe {
        name: "nom/nested-fn",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "One `FuncSpace` per level: the space-nesting \
                    bookkeeping and the recursive `FuncSpace` tree #1056 \
                    had to bound. Also the metric control for \
                    `cognitive/nested-fn` below, which selects `Cognitive` \
                    on the same shape and so pays this row's cost too.",
    },
    Probe {
        name: "cognitive/nested-fn",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: nested_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1062: `increment_function_depth` asks every function \
                    node whether a function encloses it. The answer now \
                    comes off the walker's ancestor chain and is found two \
                    steps up; climbing with `Node::parent` instead cost \
                    `O(depth)` per step across the 19 call sites — 22 \
                    languages, counting the four the JS-family macro \
                    expands to. `nom/nested-fn` is the same shape without \
                    the cognitive walk.",
    },
    Probe {
        name: "ops/nested-fn",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Ops {
            // Depth-invariant on purpose: `nested_fns` reuses one
            // identifier at every level, so the root vocabulary is the
            // same four operands at any depth. It is here to prove the
            // walk produced something, which is all
            // `probe_workload_is_exercised` asks of it; the depth
            // signal is in the timing column.
            reading: |ops| ops.operands.len() as u64,
        },
        render: nested_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1110: the only probe that runs `ops_inner`, which \
                    was otherwise unmeasured. It covers the walk — the \
                    space stack, the Halstead map merge up it, and the \
                    per-space vocabulary render — on a shape whose \
                    vocabulary is constant, so the reading is the walk \
                    alone. The *vocabulary* term cannot be probed the \
                    same way: `Ops` publishes a `Vec<String>` per space \
                    and a parent's vocabulary is a superset of every \
                    descendant's, so a shape with distinct identifiers \
                    per level has quadratic output by construction and no \
                    implementation can fit under a linear bound. \
                    `nom/nested-fn` is the same shape through \
                    `Ast::metrics`, which merges the same maps without \
                    rendering them.",
    },
    Probe {
        name: "loc/nested-fn",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.lloc(),
        },
        render: nested_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `loc/nested-fn-rows`: the same \
                    function nesting with every level on one physical row, \
                    so each `Ploc::merge` up the space stack folds a \
                    one-row set. Isolates the per-merge overhead from the \
                    per-row cost the row-spread shape adds.",
    },
    Probe {
        name: "loc/nested-fn-rows",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.ploc(),
        },
        render: nested_fns_by_row,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1109: `Ploc` / `Cloc` union each space's physical-row \
                    set into its parent, so a row inside `D` nested spaces \
                    is folded `D` times. The sets are word-array bitsets \
                    and the fold is a word-wise OR; re-inserting element by \
                    element into a hash set instead put a probe per row on \
                    that path. `loc/nested-fn` is the same nesting with one \
                    row in total, and `nom/nested-fn-rows` is the same \
                    shape under a metric that keeps no per-row set.",
    },
    Probe {
        name: "nom/nested-fn-rows",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_fns_by_row,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Metric control for `loc/nested-fn-rows`: the same \
                    row-spread nesting under a metric whose merge is a \
                    counter add, so the loc-attributable cost of that row \
                    is its difference from this one.",
    },
    Probe {
        name: "nom/nested-declared-function",
        lang: LANG::Javascript,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_declared_functions,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `nom/nested-arrow`: one function and \
                    one `FuncSpace` per level as there, but declared with \
                    `function`, which `Checker::is_func` answers from the \
                    node's own kind without an ancestor walk.",
    },
    Probe {
        name: "nom/nested-arrow",
        lang: LANG::Javascript,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_arrows,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1088: the JS-family `Checker::is_func` / `is_closure` \
                    decide whether an `arrow_function` is bound to a name by \
                    walking upward. The enclosing `statement_block` stops \
                    that walk after two steps, but `Node::parent` is \
                    `O(depth)` per step, so the shape was quadratic; the \
                    steps now index the walker's ancestor chain. \
                    `nom/nested-declared-function` is the same nesting \
                    without the walk.",
    },
    Probe {
        name: "nom/nested-attributed-fn",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: true,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_attributed_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1100: under `exclude_tests` the walker asks every \
                    node whether it opens a test-only subtree, and Rust \
                    answers by reading the run of `#[…]` siblings before \
                    an item. Walking that run backwards with \
                    `Node::previous_sibling` re-resolved the parent from \
                    the root per step; it is now one forward pass over \
                    the parent the walker already holds. `nom/nested-fn` \
                    renders the same nesting without the attributes and \
                    with `exclude_tests` off, so it controls for both \
                    the nesting and the flag at once.",
    },
    Probe {
        name: "nom/wide-attributed-fn",
        lang: LANG::Rust,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: true,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: wide_attributed_fns,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1100 on the other axis, and the first probe on it. \
                    The fix the issue originally proposed — read the \
                    attribute run forward from the parent, always — is \
                    `O(children)` per item, so on a flat file it is \
                    quadratic in the file's item count: #1100 measured \
                    a generated 2 000-item file going from 6.0 ms to \
                    569 ms while every depth probe stayed green. \
                    Reintroducing that scan takes this probe to 1.99. \
                    The scan now budgets the parent's child count \
                    against the node's depth, and this probe is what \
                    keeps the shallow-wide half of that trade measured.",
    },
    Probe {
        name: "nom/nested-cfg-predicate",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: true,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_cfg_predicate,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1105: `cfg(all(all(… test …)))` is classified by a \
                    string-level mini-parser, which re-scanned each \
                    region's whole interior to find its split points — \
                    O(len^2) in the attribute body, and a denial-of-\
                    service vector on machine-generated Rust. The commas \
                    are now indexed by paren depth in one pass. The \
                    only probe that grows one *attribute* rather than \
                    the code around it, so `nom/nested-fn` is not a \
                    control for it; the bound alone is the guard.",
    },
    Probe {
        name: "abc/wide-let-declarators",
        lang: LANG::Javascript,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.assignments_sum(),
        },
        render: wide_let_declarators,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1277's structural `const` predicate. Its first form \
                    scanned every sibling declarator for the `const` \
                    keyword once per `=`, so a `let` list of N \
                    declarators cost O(N²) — 6 s for 5 000 on a debug \
                    build, against 0.04 s for the same list under `var` \
                    — while every depth probe stayed green. The keyword \
                    is now read through the declaration's `kind` field. \
                    The reading is the `let` initializer count, which \
                    grows with the width, so a walk that stopped \
                    counting them is visible in the value column too.",
    },
    Probe {
        name: "halstead/wide-distinct-fn",
        lang: LANG::Rust,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.unique_operands(),
        },
        render: wide_distinct_fns,
        sizes: SPACE_MERGE_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1106: popping a child space merged its Halstead maps \
                    into the parent's and then re-derived the parent's \
                    `Stats` from them — three map traversals, one of them \
                    over the parent's whole accumulated operand \
                    vocabulary, once per child, for a result the parent's \
                    own finalize overwrites. That is \
                    `O(children x vocabulary)`, quadratic in a file's \
                    function count. Restoring the per-child pass takes \
                    this probe from 1.09 to 2.17 — the gate's only \
                    failure — and its 16 000-sibling cell from 45.6 ms to \
                    507.4 ms. A depth probe cannot see it whatever it \
                    nests: the cost is per popped child, and a nesting \
                    shape has one child per space at every depth.",
    },
];

#[cfg(test)]
mod tests {
    use big_code_analysis::{Ast, Source};

    use super::{Axis, PROBES, Probe, Workload};

    /// Deepest `tree_sitter` node depth reachable from the root.
    ///
    /// Iterative on an explicit stack: the shapes here nest thousands
    /// of levels deep, which is exactly the input a recursive helper
    /// would overflow on.
    fn ast_depth(ast: &Ast) -> usize {
        let mut stack = vec![(ast.as_tree_sitter().root_node(), 1_usize)];
        let mut deepest = 0;
        while let Some((node, depth)) = stack.pop() {
            deepest = deepest.max(depth);
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor).map(|child| (child, depth + 1)));
        }
        deepest
    }

    /// Most children any one node in the tree has — the quantity a
    /// width shape grows, and the one the `exclude_tests` attribute
    /// scan is priced against.
    fn max_child_count(ast: &Ast) -> usize {
        let mut stack = vec![ast.as_tree_sitter().root_node()];
        let mut widest = 0;
        while let Some(node) = stack.pop() {
            widest = widest.max(node.child_count());
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
        widest
    }

    /// The probes on `axis`, asserted non-empty.
    ///
    /// Each shape invariant below applies to one axis, and a loop over
    /// an empty filtered set passes every assertion inside it. Without
    /// this, deleting the last probe of an axis would silently retire
    /// that axis's invariant rather than fail — which for the width
    /// axis is exactly the state #1133 was filed about.
    fn probes_on(axis: Axis) -> Vec<&'static Probe> {
        let probes: Vec<&Probe> = PROBES.iter().filter(|probe| probe.axis == axis).collect();
        assert!(!probes.is_empty(), "PROBES must cover the {axis:?} axis");
        probes
    }

    fn parse(probe: &Probe, size: usize) -> Ast {
        let source = (probe.render)(size);
        Ast::parse(Source::new(probe.lang, source.as_bytes()))
            .unwrap_or_else(|e| panic!("{} at size {size} must parse: {e}", probe.name))
    }

    /// Every generator emits a constant number of bytes per unit of
    /// its size parameter.
    ///
    /// This is the trap that invalidated two measurements during the
    /// #1052 / #1062 work: an indented generator makes the input grow
    /// quadratically, so a linear walk reads as superlinear. A
    /// constant second difference is exactly "affine in depth".
    #[test]
    fn byte_growth_is_affine() {
        for probe in PROBES {
            let len = |d: usize| (probe.render)(d).len();
            let (a, b, c) = (len(10), len(20), len(30));
            assert_eq!(
                b - a,
                c - b,
                "{}: bytes must grow linearly with size, got {a} -> {b} -> {c}",
                probe.name,
            );
            assert!(
                b > a,
                "{}: size must actually add bytes, got {a} -> {b}",
                probe.name,
            );
        }
    }

    /// Every shape parses cleanly at the smallest size the gate
    /// measures it at.
    ///
    /// Without this, a grammar bump that stops accepting one of these
    /// snippets would leave the probe measuring `tree_sitter`'s error
    /// recovery while still reporting a plausible exponent. The size
    /// comes from the probe rather than from a literal because a
    /// parser limit — a recursion cap, a token-count ceiling — is
    /// reached at scale and not on a toy input, so a small fixed size
    /// answers a different question than the gate asks.
    #[test]
    fn shapes_parse_without_error() {
        for probe in PROBES {
            let size = probe.sizes[0];
            let ast = parse(probe, size);
            assert!(
                !ast.as_tree_sitter().root_node().has_error(),
                "{}: shape must parse without an ERROR node at size {size}:\n{}…",
                probe.name,
                (probe.render)(size).chars().take(200).collect::<String>(),
            );
        }
    }

    /// Every depth shape actually nests: doubling the size adds at
    /// least that many more levels of AST.
    ///
    /// A shape that flattened — because a grammar started folding the
    /// repetition into a list node, say — would still parse, still
    /// grow linearly in bytes, and still produce a tidy exponent near
    /// 1.0 while measuring nothing the probe claims to measure.
    #[test]
    fn shapes_nest_proportionally_to_depth() {
        for probe in probes_on(Axis::Depth) {
            let shallow = ast_depth(&parse(probe, 16));
            let deep = ast_depth(&parse(probe, 32));
            assert!(
                deep >= shallow + 16,
                "{probe_name}: doubling depth 16 -> 32 grew the tree \
                 {shallow} -> {deep} AST levels; the shape is not \
                 nesting",
                probe_name = probe.name,
            );
        }
    }

    /// Every width shape widens, and *only* widens.
    ///
    /// The positive half mirrors
    /// [`shapes_nest_proportionally_to_depth`]: doubling the size must
    /// add at least that many children to the widest node, so a shape
    /// that stopped growing is caught.
    ///
    /// The negative half is the one that matters. A "width" probe that
    /// quietly began nesting — a generator edited to wrap its items,
    /// or a grammar that started grouping them — would satisfy the
    /// first half while measuring the depth axis, and report a healthy
    /// exponent for a width bound nothing is testing any more. Pinning
    /// the AST depth *constant* across the two sizes is what
    /// distinguishes the two axes; nothing else here can.
    ///
    /// Sixteen and thirty-two, not the probe's own ladder as the two
    /// tests below use: both halves are structural claims about the
    /// generator, true at any pair of sizes, and 500 vs 1 000 would
    /// cost a second of parsing to assert the same thing.
    #[test]
    fn shapes_widen_without_deepening() {
        for probe in probes_on(Axis::Width) {
            let (narrow, wide) = (parse(probe, 16), parse(probe, 32));
            let (narrow_width, wide_width) = (max_child_count(&narrow), max_child_count(&wide));
            assert!(
                wide_width >= narrow_width + 16,
                "{probe_name}: doubling width 16 -> 32 grew the widest \
                 node {narrow_width} -> {wide_width} children; the \
                 shape is not widening",
                probe_name = probe.name,
            );
            assert_eq!(
                ast_depth(&narrow),
                ast_depth(&wide),
                "{}: a width shape must not gain AST levels with its \
                 size, or its exponent is measuring the depth axis",
                probe.name,
            );
        }
    }

    /// Every probe's workload produces a non-zero reading on its own
    /// shape, at the smallest size the gate measures it at.
    ///
    /// Pairing a shape with a workload that scores zero on it would
    /// benchmark the walk's fixed overhead and nothing else, and the
    /// resulting exponent would look excellent forever.
    ///
    /// The size is the probe's own rather than a fixed small one: a
    /// literal that happens to suit the nesting shapes is sixteen-odd
    /// siblings on a width shape — too small to be the shape the probe
    /// stands for, and a reading that only becomes non-zero at scale
    /// would fail here for a reason unrelated to the pairing. The
    /// whole set costs ~0.4 s in a debug build at these sizes.
    #[test]
    fn probe_workload_is_exercised() {
        for probe in PROBES {
            let size = probe.sizes[0];
            let ast = parse(probe, size);
            let reading = probe
                .workload
                .walk(&ast, probe.workload.options())
                .unwrap_or_else(|e| panic!("{}: walker must succeed: {e}", probe.name));
            assert!(
                reading > 0,
                "{}: workload scored zero on its own shape at size {size}",
                probe.name,
            );
        }
    }

    /// Every probe whose *shape* the exclusion hook prunes walks with
    /// `exclude_tests` on.
    ///
    /// [`probe_workload_is_exercised`] cannot see this. The two
    /// attributed-function shapes carry `#[inline]`, which no
    /// `exclude_tests` rule prunes, so `nom.total()` reads the size
    /// parameter with the flag on or off: flipping
    /// `exclude_tests: true` to `false` on `nom/wide-attributed-fn`
    /// deletes the whole `Checker::should_skip_subtree` attribute scan
    /// the probe exists to price, and fails nothing (measured, #1133).
    ///
    /// The discriminator is deliberately the shape and not the flag.
    /// Selecting the probes *by* `exclude_tests: true` and asserting
    /// something about them is vacuous against exactly that flip — the
    /// flipped probe leaves the selected set and the loop passes
    /// (measured, too). So each shape is classified by walking it both
    /// ways under forced options: one whose reading moves is one the
    /// hook prunes, and that probe must be running with the flag set.
    ///
    /// `#[inline]` is rewritten to `#[test]` first, which is what makes
    /// the attributed shapes prunable at all. Reading the source back
    /// through `probe.render` rather than restating it keeps the guard
    /// coupled to the generator: one that stopped emitting attributes
    /// stops being classified and trips the floor below.
    ///
    /// Size 16 rather than the probe's own ladder, as
    /// [`shapes_widen_without_deepening`] uses: "the hook prunes this
    /// shape" is structural and true at any size.
    #[test]
    fn shapes_the_exclusion_hook_prunes_are_probed_with_it_on() {
        let mut pruned_shapes = Vec::new();
        for probe in PROBES {
            let Workload::Metrics { exclude_tests, .. } = probe.workload else {
                continue;
            };
            let source = (probe.render)(16).replace("#[inline]", "#[test]");
            let ast = Ast::parse(Source::new(probe.lang, source.as_bytes())).unwrap_or_else(|e| {
                panic!("{}: test-attributed shape must parse: {e}", probe.name)
            });
            let walk = |options| {
                probe
                    .workload
                    .walk(&ast, options)
                    .unwrap_or_else(|e| panic!("{}: walker must succeed: {e}", probe.name))
            };
            let options = probe.workload.options();
            if walk(options.with_exclude_tests(true)) == walk(options.with_exclude_tests(false)) {
                continue;
            }
            pruned_shapes.push(probe.name);
            assert!(
                exclude_tests,
                "{}: the exclusion hook prunes this shape, so the probe \
                 exists to price that walk — but its workload sets \
                 `exclude_tests: false`, which never reaches it",
                probe.name,
            );
        }
        assert!(
            pruned_shapes.len() >= 2,
            "only {pruned_shapes:?} are pruned by the exclusion hook; \
             `Checker::should_skip_subtree` needs a probe on each axis, \
             and a shape that stopped carrying attributes drops out of \
             this check silently"
        );
    }

    /// Probe names are unique — they key the report and the gate.
    #[test]
    fn probe_names_are_unique() {
        let mut names: Vec<&str> = PROBES.iter().map(|probe| probe.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(count, names.len(), "duplicate probe name in PROBES");
    }

    /// Sizes double, which is what makes the fitted exponent readable
    /// as "cost per doubling".
    #[test]
    fn probe_sizes_double() {
        for probe in PROBES {
            let [a, b, c] = probe.sizes;
            assert_eq!(b, a * 2, "{}: sizes must double", probe.name);
            assert_eq!(c, b * 2, "{}: sizes must double", probe.name);
        }
    }
}
