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
/// that a *quadratic* regression is abandoned partway up the ladder
/// rather than run to completion. Bounding the total cost of such a
/// regression is `MAX_CELL_WALK`'s job, not this constant's: at these
/// depths a pre-#1052-magnitude blow-up costs a couple of minutes
/// before the budget fires, which is a report rather than the hung
/// job the retired wall-clock assertions were guarding against.
const LINEAR_DEPTHS: [usize; 3] = [1_000, 2_000, 4_000];

/// Bound for a probe expected to be linear in nesting depth.
///
/// Set from measurement, not from theory. A genuinely linear walk does
/// not fit exactly 1.0 over these depths: the tree outgrows cache as
/// depth rises, so per-byte cost drifts up by roughly a quarter from
/// the shallowest cell to the deepest and every probe here fits
/// 0.94-1.31 on an idle host. Before #1084 the three ancestor-walk
/// probes fit 1.95-2.01; the midpoint of those two bands is the cut,
/// and it is now the only bound in the set.
const LINEAR_BOUND: f64 = 1.5;

/// The depth-scaling probe set.
///
/// One entry per hot path identified during the #1052 / #1062 / #1084
/// work, plus the controls that make those readings interpretable:
///
/// - two *metric* controls, `nom/nested-while` and `nom/nested-fn`.
///   `Cognitive` declares `Nom` as a dependency, so the
///   cognitive-attributable cost of a `cognitive/…` row is its
///   difference from the `nom/…` row on the same shape, not the
///   `cognitive` reading alone.
/// - two *shape* controls, `cognitive/nested-while` and
///   `loc/nested-while`. Each is the same nesting as the ancestor-walk
///   probe below it with the one node that triggers the walk removed.
///   That pairing is what attributed the pre-#1084 quadratic readings
///   to those calls rather than to nesting in general, and it is what
///   would localise a future regression the same way.
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
        depths: LINEAR_DEPTHS,
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
        depths: LINEAR_DEPTHS,
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
        metrics: &[Metric::Halstead],
        render: nested_parens,
        reading: |m| m.halstead.length(),
        depths: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `halstead/nested-not`: the same \
                    nesting through `get_op_type` arms that classify a \
                    token from its own kind, with no parent read.",
    },
    Probe {
        name: "halstead/nested-not",
        lang: LANG::Rust,
        metrics: &[Metric::Halstead],
        render: nested_nots,
        reading: |m| m.halstead.length(),
        depths: LINEAR_DEPTHS,
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
        metrics: &[Metric::Abc],
        render: nested_blocks,
        reading: |m| m.abc.assignments_sum(),
        depths: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `abc/nested-if`: the same nesting \
                    with no condition slot, so the C-family container \
                    walker is never entered.",
    },
    Probe {
        name: "abc/nested-if",
        lang: LANG::C,
        metrics: &[Metric::Abc],
        render: nested_ifs,
        reading: |m| m.abc.conditions_sum(),
        depths: LINEAR_DEPTHS,
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
        metrics: &[Metric::Cyclomatic],
        render: nested_ands,
        reading: |m| m.cyclomatic.cyclomatic_sum(),
        depths: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `cyclomatic/nested-ternary`: the \
                    same nesting through an arm that counts the token \
                    from its own kind and never looks up.",
    },
    Probe {
        name: "cyclomatic/nested-ternary",
        lang: LANG::Python,
        metrics: &[Metric::Cyclomatic],
        render: nested_ternaries,
        reading: |m| m.cyclomatic.cyclomatic_sum(),
        depths: LINEAR_DEPTHS,
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
        metrics: &[Metric::Loc],
        render: nested_quotes,
        reading: |m| m.loc.lloc(),
        depths: LINEAR_DEPTHS,
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
        metrics: &[Metric::Nom],
        render: nested_quotes,
        reading: |m| m.nom.total(),
        depths: LINEAR_DEPTHS,
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
        metrics: &[Metric::Nom],
        render: nested_fns,
        reading: |m| m.nom.total(),
        depths: LINEAR_DEPTHS,
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
        metrics: &[Metric::Cognitive],
        render: nested_fns,
        reading: |m| m.cognitive.cognitive_sum(),
        depths: LINEAR_DEPTHS,
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
        name: "nom/nested-declared-function",
        lang: LANG::Javascript,
        metrics: &[Metric::Nom],
        render: nested_declared_functions,
        reading: |m| m.nom.total(),
        depths: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `nom/nested-arrow`: one function and \
                    one `FuncSpace` per level as there, but declared with \
                    `function`, which `Checker::is_func` answers from the \
                    node's own kind without an ancestor walk.",
    },
    Probe {
        name: "nom/nested-arrow",
        lang: LANG::Javascript,
        metrics: &[Metric::Nom],
        render: nested_arrows,
        reading: |m| m.nom.total(),
        depths: LINEAR_DEPTHS,
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
