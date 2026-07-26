//! Synthetic depth-scaling inputs, and the probes built from them.
//!
//! # Why every generator is affine in `depth`
//!
//! A generator that indents each nesting level makes the *input* grow
//! quadratically, so a walk that is perfectly linear in bytes still
//! looks superlinear in depth. That mistake invalidated two published
//! measurements during the #1052 / #1062 work before it was spotted.
//! Every generator here therefore emits a constant number of bytes per
//! nesting level and no indentation at all, which the
//! `byte_growth_is_affine` unit test below pins, rather than leaving it
//! a convention someone has to remember.

use big_code_analysis::{CodeMetrics, LANG, Metric};

/// Renders a source shape at a given nesting depth.
///
/// Every implementation must be affine in `depth`: `len(d)` is
/// `base + per_level * d`. See the module docs for why.
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
/// `Checker::is_else_if` calls `Node::parent` for every
/// `if_statement`, and `tree_sitter`'s parent lookup is itself
/// `O(depth)` because the tree stores no parent pointer. The shape is
/// therefore quadratic today — the probe pins how quadratic, so a
/// further degradation is still caught.
#[must_use]
pub fn nested_ifs(depth: usize) -> String {
    format!(
        "int main(){{ {}1;{} }}\n",
        "if (a) { ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// C: `int main(){ while (a) { int x; … } }`.
///
/// One `declaration` per nesting level. `loc`'s C-family arm resolves
/// each declaration's logical-line contribution with
/// `Node::count_specific_ancestors`, whose `stop` predicate is
/// `compound_statement` — the brace directly above every declaration
/// here. The probe pins that the stop predicate keeps the walk `O(1)`
/// per declaration; dropping or widening it turns this shape
/// quadratic.
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
/// inside one costs `O(1)`; if the short-circuit is lost, every level
/// walks to the root and the shape turns quadratic.
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

/// Rust: `fn f() { fn f() { … let x = 1; … } }`.
///
/// Each level opens a `FuncSpace`, so this drives `increment_function
/// _depth`, the space-nesting bookkeeping, and the recursive
/// `FuncSpace` tree that #1056 had to bound. Inner `fn f` shadows are
/// legal — each sits in its own block scope.
#[must_use]
pub fn nested_fns(depth: usize) -> String {
    format!(
        "{}let x = 1;{}\n",
        "fn f() { ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// One depth-scaling probe: a shape, the metric selection that
/// exercises the hot path under test, and the complexity class the
/// walk is expected to stay within.
#[derive(Clone, Copy)]
pub struct Probe {
    /// Stable `<metric>/<shape>` identifier, used as the report key.
    pub name: &'static str,
    /// Language whose grammar the shape is written for.
    pub lang: LANG,
    /// Metric selection handed to `MetricsOptions::with_only`. Kept as
    /// narrow as the probe allows so an unrelated metric's cost cannot
    /// dominate and misattribute a regression.
    pub metrics: &'static [Metric],
    /// Generator for the probe's input.
    pub render: Render,
    /// Headline value the probe's metric selection produces.
    ///
    /// Reported alongside every timing so a reader can see the walk
    /// did real work, and asserted non-zero by
    /// `probe_metric_selection_is_exercised`: a shape paired with a
    /// metric that scores zero on it would time the walk's fixed
    /// overhead and report an excellent exponent forever.
    pub reading: fn(&CodeMetrics) -> u64,
    /// The three depths measured, each a doubling of the previous so
    /// the fitted exponent reads directly as "cost per doubling".
    pub depths: [usize; 3],
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
/// that a *quadratic* regression fails the gate in seconds instead of
/// hanging it.
const LINEAR_DEPTHS: [usize; 3] = [1_000, 2_000, 4_000];

/// Depths for the probes whose walk is already quadratic.
///
/// Scaled down by 4x from [`LINEAR_DEPTHS`] so the deepest cell costs
/// about the same wall clock as a linear probe's deepest cell.
const QUADRATIC_DEPTHS: [usize; 3] = [250, 500, 1_000];

/// Bound for a probe expected to be linear in nesting depth.
///
/// Set from measurement, not from theory. A genuinely linear walk does
/// not fit exactly 1.0 over these depths: the tree outgrows cache as
/// depth rises, so per-byte cost drifts up by roughly a quarter from
/// the shallowest cell to the deepest and the linear probes fit
/// 0.94-1.17 on an idle host. The quadratic probes fit 1.95-2.01. The
/// midpoint of those two observed bands is the cut.
const LINEAR_BOUND: f64 = 1.5;

/// Bound for a probe whose walk is quadratic today (#1084).
///
/// Not an endorsement — it pins the known-bad path so a *third* factor
/// of depth is still caught. Midway between the observed quadratic
/// band (1.95-2.01) and the cubic it would become. When #1084 lands,
/// the probes carrying this bound move to [`LINEAR_BOUND`] in the same
/// change.
const QUADRATIC_BOUND: f64 = 2.5;

/// The depth-scaling probe set.
///
/// One entry per hot path identified during the #1052 / #1062 work,
/// plus the controls that make those readings interpretable:
///
/// - a *metric* control, `nom/nested-while`. `Cognitive` declares
///   `Nom` as a dependency, so the cognitive-attributable cost is the
///   difference between the two `nested-while` rows, not `cognitive`
///   alone.
/// - two *shape* controls, `cognitive/nested-while` and
///   `loc/nested-while`. Each is the same nesting as the quadratic
///   probe below it with the one node that triggers an ancestor walk
///   removed, which is what attributes the quadratic reading to that
///   call rather than to nesting in general.
pub const PROBES: &[Probe] = &[
    Probe {
        name: "tokens/nested-paren",
        lang: LANG::Rust,
        metrics: &[Metric::Tokens],
        render: nested_parens,
        reading: |m| m.tokens.tokens_sum(),
        depths: LINEAR_DEPTHS,
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
        metrics: &[Metric::Cognitive],
        render: nested_whiles,
        reading: |m| m.cognitive.cognitive_sum(),
        depths: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1062 on a statement shape, and the linear control for \
                    `cognitive/nested-if`: identical structure, no \
                    `is_else_if` predicate.",
    },
    Probe {
        name: "nom/nested-while",
        lang: LANG::C,
        metrics: &[Metric::Nom],
        render: nested_whiles,
        reading: |m| m.nom.total(),
        depths: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Metric control. `Cognitive` declares `Nom` as a \
                    dependency, so the cognitive-attributable cost is the \
                    difference between this row and `cognitive/nested-while`.",
    },
    Probe {
        name: "cognitive/nested-if",
        lang: LANG::C,
        metrics: &[Metric::Cognitive],
        render: nested_ifs,
        reading: |m| m.cognitive.cognitive_sum(),
        depths: QUADRATIC_DEPTHS,
        max_exponent: QUADRATIC_BOUND,
        rationale: "`Checker::is_else_if` calls `Node::parent` per \
                    `if_statement`, and that lookup is itself `O(depth)`. \
                    Quadratic today (#1084) across the 13 languages with \
                    an `is_else_if` impl.",
    },
    Probe {
        name: "loc/nested-while",
        lang: LANG::C,
        metrics: &[Metric::Loc],
        render: nested_whiles,
        reading: |m| m.loc.lloc(),
        depths: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `loc/nested-declaration`: the same \
                    nesting with no `declaration` node, so `loc` never \
                    reaches `Node::count_specific_ancestors`.",
    },
    Probe {
        name: "loc/nested-declaration",
        lang: LANG::C,
        metrics: &[Metric::Loc],
        render: nested_declarations,
        reading: |m| m.loc.lloc(),
        depths: QUADRATIC_DEPTHS,
        max_exponent: QUADRATIC_BOUND,
        rationale: "`loc`'s C-family arm calls \
                    `Node::count_specific_ancestors` for every \
                    `declaration`. Its `stop` predicate fires on the \
                    enclosing `compound_statement` immediately, so the \
                    walk is one step — but that one step is \
                    `Node::parent`, which `tree_sitter` resolves by \
                    descending from the root. Quadratic today (#1084); \
                    `loc/nested-while` is the same nesting without a \
                    declaration and stays linear.",
    },
    Probe {
        name: "nom/nested-quote",
        lang: LANG::Elixir,
        metrics: &[Metric::Nom],
        render: nested_quotes,
        reading: |m| m.nom.total(),
        depths: QUADRATIC_DEPTHS,
        max_exponent: QUADRATIC_BOUND,
        rationale: "Elixir's `is_func` asks `elixir_is_inside_quote_block` \
                    for every `def`. The predicate short-circuits on the \
                    first `quote` ancestor, so it takes one step — but \
                    each step is a `Node::parent` call, which \
                    `tree_sitter` resolves by descending from the root. \
                    Quadratic today (#1084); the bound pins how \
                    quadratic.",
    },
    Probe {
        name: "nom/nested-fn",
        lang: LANG::Rust,
        metrics: &[Metric::Nom],
        render: nested_fns,
        reading: |m| m.nom.total(),
        depths: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "One `FuncSpace` per level: `increment_function_depth`, \
                    the space-nesting bookkeeping, and the recursive \
                    `FuncSpace` tree #1056 had to bound.",
    },
];

#[cfg(test)]
mod tests {
    use big_code_analysis::{Ast, MetricsOptions, Source};

    use super::{PROBES, Probe};

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

    fn parse(probe: &Probe, depth: usize) -> Ast {
        let source = (probe.render)(depth);
        Ast::parse(Source::new(probe.lang, source.as_bytes()))
            .unwrap_or_else(|e| panic!("{} at depth {depth} must parse: {e}", probe.name))
    }

    /// Every generator emits a constant number of bytes per nesting
    /// level.
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
                "{}: bytes must grow linearly with depth, got {a} -> {b} -> {c}",
                probe.name,
            );
            assert!(
                b > a,
                "{}: depth must actually add bytes, got {a} -> {b}",
                probe.name,
            );
        }
    }

    /// Every shape parses cleanly.
    ///
    /// Without this, a grammar bump that stops accepting one of these
    /// snippets would leave the probe measuring `tree_sitter`'s error
    /// recovery while still reporting a plausible exponent.
    #[test]
    fn shapes_parse_without_error() {
        for probe in PROBES {
            let ast = parse(probe, 8);
            assert!(
                !ast.as_tree_sitter().root_node().has_error(),
                "{}: shape must parse without an ERROR node:\n{}",
                probe.name,
                (probe.render)(8),
            );
        }
    }

    /// Every shape actually nests: doubling `depth` adds at least
    /// `depth` more levels of AST.
    ///
    /// A shape that flattened — because a grammar started folding the
    /// repetition into a list node, say — would still parse, still
    /// grow linearly in bytes, and still produce a tidy exponent near
    /// 1.0 while measuring nothing the probe claims to measure.
    #[test]
    fn shapes_nest_proportionally_to_depth() {
        for probe in PROBES {
            let shallow = ast_depth(&parse(probe, 16));
            let deep = ast_depth(&parse(probe, 32));
            assert!(
                deep - shallow >= 16,
                "{probe_name}: doubling depth 16 -> 32 added only \
                 {added} AST levels ({shallow} -> {deep}); the shape is \
                 not nesting",
                probe_name = probe.name,
                added = deep - shallow,
            );
        }
    }

    /// Every probe's metric selection produces a non-zero reading on
    /// its own shape.
    ///
    /// Pairing a shape with a metric that scores zero on it would
    /// benchmark the walk's fixed overhead and nothing else, and the
    /// resulting exponent would look excellent forever.
    #[test]
    fn probe_metric_selection_is_exercised() {
        for probe in PROBES {
            let ast = parse(probe, 8);
            let space = ast
                .metrics(MetricsOptions::default().with_only(probe.metrics))
                .unwrap_or_else(|e| panic!("{}: walker must succeed: {e}", probe.name));
            assert!(
                (probe.reading)(&space.metrics) > 0,
                "{}: selected metrics scored zero on their own shape",
                probe.name,
            );
        }
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

    /// Depths double, which is what makes the fitted exponent readable
    /// as "cost per doubling".
    #[test]
    fn probe_depths_double() {
        for probe in PROBES {
            let [a, b, c] = probe.depths;
            assert_eq!(b, a * 2, "{}: depths must double", probe.name);
            assert_eq!(c, b * 2, "{}: depths must double", probe.name);
        }
    }
}
