// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(
    clippy::enum_glob_use,
    clippy::match_same_arms,
    clippy::needless_pass_by_value,
    clippy::wildcard_imports
)]
// Metric counts (token, function, branch, argument, etc.) are stored as
// `usize` and crossed with `f64` averages, ratios, and Halstead scores
// across the cyclomatic / MI / Halstead computations. The `usize as f64`
// and `f64 as usize` casts are intentional and snapshot-anchored — every
// site is bounded by the count it came from. Allowing the lints at the
// module level keeps the metric arithmetic legible.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::collections::HashMap;

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::fmt;

use crate::checker::Checker;
use crate::macros::implement_metric_trait;
use crate::*;

// TODO: Find a way to increment the cognitive complexity value
// for recursive code. For some kind of languages, such as C++, it is pretty
// hard to detect, just parsing the code, if a determined function is recursive
// because the call graph of a function is solved at runtime.
// So a possible solution could be searching for a crate which implements
// a light language interpreter, computing the call graph, and then detecting
// if there are cycles. At this point, it is possible to figure out if a
// function is recursive or not.

/// The `Cognitive Complexity` metric.
#[derive(Debug, Clone)]
pub struct Stats {
    structural: usize,
    structural_sum: usize,
    structural_min: usize,
    structural_max: usize,
    nesting: usize,
    total_space_functions: usize,
    boolean_seq: BoolSequence,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            structural: 0,
            structural_sum: 0,
            structural_min: usize::MAX,
            structural_max: 0,
            nesting: 0,
            total_space_functions: 1,
            boolean_seq: BoolSequence::default(),
        }
    }
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("cognitive", 4)?;
        st.serialize_field("sum", &self.cognitive_sum())?;
        st.serialize_field("average", &self.cognitive_average())?;
        st.serialize_field("min", &self.cognitive_min())?;
        st.serialize_field("max", &self.cognitive_max())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "sum: {}, average: {}, min:{}, max: {}",
            self.cognitive(),
            self.cognitive_average(),
            self.cognitive_min(),
            self.cognitive_max()
        )
    }
}

impl Stats {
    /// Merges a second `Cognitive Complexity` metric into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.structural_min = self.structural_min.min(other.structural_min);
        self.structural_max = self.structural_max.max(other.structural_max);
        self.structural_sum += other.structural_sum;
    }

    /// Returns the `Cognitive Complexity` metric value
    #[must_use]
    pub fn cognitive(&self) -> f64 {
        self.structural as f64
    }
    /// Returns the `Cognitive Complexity` sum metric value
    #[must_use]
    pub fn cognitive_sum(&self) -> f64 {
        self.structural_sum as f64
    }

    /// Returns the `Cognitive Complexity` minimum metric value.
    ///
    /// Collapses the `usize::MAX` sentinel that `Stats::default()` plants
    /// into `structural_min` to `0.0`, so a never-observed space
    /// serializes to a meaningful number rather than `1.8446744e19`.
    #[must_use]
    pub fn cognitive_min(&self) -> f64 {
        if self.structural_min == usize::MAX {
            0.0
        } else {
            self.structural_min as f64
        }
    }
    /// Returns the `Cognitive Complexity` maximum metric value
    #[must_use]
    pub fn cognitive_max(&self) -> f64 {
        self.structural_max as f64
    }

    /// Returns the `Cognitive Complexity` metric average value
    ///
    /// This value is computed dividing the `Cognitive Complexity` value
    /// for the total number of functions/closures in a space.
    ///
    /// The divisor is guarded with `.max(1)` so a space with no
    /// counted functions (or one where `Nom` was not selected)
    /// degrades to `sum / 1` instead of producing `inf`/`NaN` (#428).
    #[must_use]
    pub fn cognitive_average(&self) -> f64 {
        self.cognitive_sum() / self.total_space_functions.max(1) as f64
    }
    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.structural_sum += self.structural;
    }
    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        self.structural_min = self.structural_min.min(self.structural);
        self.structural_max = self.structural_max.max(self.structural);
        self.compute_sum();
    }

    pub(crate) fn finalize(&mut self, total_space_functions: usize) {
        self.total_space_functions = total_space_functions;
    }
}

#[doc(hidden)]
/// Per-language computation of the cognitive complexity metric.
pub trait Cognitive
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    ///
    /// `code` is the source bytes underlying the parsed tree. Most
    /// languages ignore it: their control-flow constructs surface as
    /// distinct grammar productions (`IfStatement`, `WhileStatement`,
    /// …) and a `kind_id()` match is enough. Elixir is the exception
    /// — `if` / `unless` / `case` / `cond` / `for` / `while` / `with`
    /// all surface as `Call` nodes whose keyword target lives only in
    /// the source text (the `target` field is an `Identifier`). This
    /// matches the `Cyclomatic` / `Halstead` / `Exit` pattern of
    /// taking `code` so the same source-text dispatch can run here.
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    );
}

/// Walks `node.children()` and folds each child whose `kind_id`
/// satisfies `is_op` into the boolean-sequence counter. The predicate
/// is the only thing that differs across the per-language short-
/// circuit helpers (`compute_*_booleans`); inlining the predicate as
/// a `Fn` closure lets each language declare its operator set with a
/// `matches!` pattern at the call site without duplicating the walk.
fn compute_booleans_with<F: Fn(u16) -> bool>(node: &Node, stats: &mut Stats, is_op: F) {
    let enclosing_end = node.end_byte();
    for child in node.children() {
        let id = child.kind_id();
        if is_op(id) {
            stats.structural =
                stats
                    .boolean_seq
                    .eval_based_on_prev(id, enclosing_end, stats.structural);
        }
    }
}

/// Two-operator specialization. Most call sites match exactly two
/// enum variants (`&&` + `||`, or `and` + `or`); this signature
/// keeps those call sites as plain `(node, stats, A, B)` rather than
/// forcing a closure.
fn compute_booleans<T: PartialEq + From<u16>>(node: &Node, stats: &mut Stats, typs1: T, typs2: T) {
    compute_booleans_with(node, stats, |id| {
        let converted: T = id.into();
        typs1 == converted || typs2 == converted
    });
}

/// Folds a Ruby `binary`'s short-circuit operator children into the
/// boolean-sequence counter — Ruby has four (`&&`, `||`, word-form
/// `and`, word-form `or`).
fn compute_ruby_booleans(node: &Node, stats: &mut Stats) {
    compute_booleans_with(node, stats, |id| {
        matches!(
            id.into(),
            Ruby::AMPAMP | Ruby::PIPEPIPE | Ruby::And | Ruby::Or
        )
    });
}

/// Folds a Perl `binary_expression`'s short-circuit operator children
/// into the boolean-sequence counter — Perl has five bare forms (`&&`,
/// `||`, `//`, `and`, `or`) plus three compound short-circuit
/// assignments (`&&=`, `||=`, `//=`). The grammar exposes each `op=`
/// as a distinct operator token inside the same `binary_expression`,
/// so they fold into the same predicate (issue #249).
fn compute_perl_booleans(node: &Node, stats: &mut Stats) {
    compute_booleans_with(node, stats, |id| {
        matches!(
            id.into(),
            Perl::AMPAMP
                | Perl::PIPEPIPE
                | Perl::SLASHSLASH
                | Perl::And
                | Perl::Or
                | Perl::AMPAMPEQ
                | Perl::PIPEPIPEEQ
                | Perl::SLASHSLASHEQ
        )
    });
}

/// Folds an Elixir `BinaryOperator`'s short-circuit operator children
/// into the boolean-sequence counter — Elixir has four (`&&`, `||`,
/// `and`, `or`). Single-pass walk over `node.children()` avoids the
/// 2x cost of calling the two-operator `compute_booleans` twice.
fn compute_elixir_booleans(node: &Node, stats: &mut Stats) {
    compute_booleans_with(node, stats, |id| {
        matches!(
            id.into(),
            Elixir::AMPAMP | Elixir::PIPEPIPE | Elixir::And | Elixir::Or
        )
    });
}

#[derive(Debug, Default, Clone)]
struct BoolSequence {
    boolean_op: Option<(u16, usize)>,
}

impl BoolSequence {
    fn reset(&mut self) {
        // Structural boundaries (new branches, nesting increments) end the current sequence.
        self.boolean_op = None;
    }

    fn eval_based_on_prev(
        &mut self,
        bool_id: u16,
        enclosing_end: usize,
        structural: usize,
    ) -> usize {
        match self.boolean_op {
            // Same operator type and enclosing_end fits inside the previously seen
            // binary_expression span (pre-order: parent visited before child) →
            // continuation of the same sequence, no extra cost.
            Some((prev_id, prev_end)) if prev_id == bool_id && enclosing_end <= prev_end => {
                structural
            }
            _ => {
                self.boolean_op = Some((bool_id, enclosing_end));
                structural + 1
            }
        }
    }
}

#[inline]
fn increment(stats: &mut Stats) {
    stats.structural += stats.nesting + 1;
}

#[inline]
fn increment_by_one(stats: &mut Stats) {
    stats.structural += 1;
}

#[inline]
fn increment_branch_extension(stats: &mut Stats) {
    stats.structural += 1;
    stats.boolean_seq.reset();
}

fn get_nesting_from_map(
    node: &Node,
    nesting_map: &HashMap<usize, (usize, usize, usize)>,
) -> (usize, usize, usize) {
    node.parent()
        .and_then(|parent| nesting_map.get(&parent.id()))
        .copied()
        .unwrap_or((0, 0, 0))
}

fn increment_function_depth<T: PartialEq + From<u16>>(depth: &mut usize, node: &Node, stops: &[T]) {
    let mut child = *node;
    while let Some(parent) = child.parent() {
        if stops.contains(&T::from(parent.kind_id())) {
            *depth += 1;
            break;
        }
        child = parent;
    }
}

#[inline]
fn increase_nesting(stats: &mut Stats, nesting: &mut usize, depth: usize, lambda: usize) {
    stats.nesting = *nesting + depth + lambda;
    increment(stats);
    *nesting += 1;
    stats.boolean_seq.reset();
}

/// Whether `node` is a Python `lambda` expression, under either of the
/// grammar's two aliased kind_ids: `Lambda` (196, the concrete
/// production emitted today) and `Lambda2` (197, the currently-unseen
/// hidden alias). `Lambda3` (73) is the `lambda` *keyword* token, not a
/// closure node, and is intentionally excluded.
///
/// This is the single normalization chokepoint for the lambda-alias set
/// — mirroring `npa::python_is_block` for the block aliases (#419). It
/// is reused by the cognitive lambda-scope walks below and by
/// [`PythonCode::is_closure`](crate::checker), so a future grammar bump
/// that promotes `Lambda2` to a concrete node is handled in exactly one
/// place rather than drifting across sites (#422). The
/// `python_hidden_block_and_lambda_aliases_stay_unseen` drift guard in
/// `checker.rs` trips on such a bump.
pub(crate) fn python_is_lambda(node: &Node) -> bool {
    matches!(node.kind_id().into(), Python::Lambda | Python::Lambda2)
}

impl Cognitive for PythonCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Python::*;

        // Get nesting of the parent
        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // `else: if x:` chains surface as an `if_statement` wrapped
            // in an `else_clause`; `Self::is_else_if` flags that shape
            // so the nesting increment lands only on the outer chain
            // (matching the `elif_clause` accounting one arm below).
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement | WhileStatement | ConditionalExpression | MatchStatement => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // A comprehension / generator expression is a loop with an
            // optional filter, so it carries cognitive load just like the
            // explicit `for`/`if` form it desugars to (#417). Cyclomatic
            // already counts the `for`/`if` keyword tokens inside these
            // clauses; without these arms `[x for x in xs if x > 0]` scored
            // cognitive 0 while the equivalent explicit loop scored 3.
            //
            // `for_in_clause` and `if_clause` are SIBLINGS under the
            // comprehension node, not parent/child, and each clause's nesting
            // depends on how many `for` clauses precede it. Rather than have
            // each clause re-scan its siblings (O(N^2)) or write its nesting
            // back onto the shared parent for later siblings to read, the
            // comprehension node — visited before any of its clauses in
            // pre-order — precomputes every clause's nesting in one pass and
            // stashes it in that clause's own map slot, which the
            // `ForInClause | IfClause` arm reads back. Computing it here, on
            // the ancestor pre-order reaches first, makes the result
            // independent of sibling traversal order; that is the #421 fix
            // (the original #417 sibling write-back was never seen by a
            // comprehension sitting in the *element* position, which pre-order
            // visits before the outer clauses run, so it under-counted).
            //
            // The same pass accumulates the element's own nesting onto the
            // comprehension node's slot, so a nested comprehension in element
            // position inherits the full outer loop+filter depth.
            ListComprehension
            | DictionaryComprehension
            | SetComprehension
            | GeneratorExpression => {
                // Each clause sits at the comprehension's inherited nesting
                // plus the number of `for` clauses strictly before it. The
                // element executes inside the body opened by the *last* clause,
                // so it sits `for_count` levels deep (a trailing `for` has
                // already advanced the count) plus one more when the last
                // clause is an `if`.
                let mut for_count = 0;
                let mut last_clause_is_if = false;
                for child in node.children() {
                    let kind = child.kind_id();
                    if kind == ForInClause as u16 {
                        nesting_map.insert(child.id(), (nesting + for_count, depth, lambda));
                        for_count += 1;
                        last_clause_is_if = false;
                    } else if kind == IfClause as u16 {
                        nesting_map.insert(child.id(), (nesting + for_count, depth, lambda));
                        last_clause_is_if = true;
                    }
                }
                nesting += for_count + usize::from(last_clause_is_if);
            }
            ForInClause | IfClause => {
                // Nesting was precomputed on the comprehension node (visited
                // first in pre-order) into this clause's own map slot, so read
                // it back instead of re-scanning siblings per clause.
                if let Some(&(clause_nesting, _, _)) = nesting_map.get(&node.id()) {
                    nesting = clause_nesting;
                }
                stats.nesting = nesting + depth + lambda;
                increment(stats);
                stats.boolean_seq.reset();
            }
            ElifClause => {
                // No nesting increment for them because their cost has already
                // been paid by the if construct
                increment_branch_extension(stats);
            }
            ElseClause => {
                // No nesting increment for it because its cost has already
                // been paid by the if construct. A `finally` clause, by
                // contrast, is structured cleanup that always runs and adds
                // 0 per the SonarSource Cognitive Complexity spec (#416) —
                // so `FinallyClause` deliberately falls through to `_ => {}`,
                // matching the Java sibling which has no finally arm.
                increment_by_one(stats);
            }
            ExceptClause => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ExpressionList | ExpressionStatement | Tuple => {
                stats.boolean_seq.reset();
            }
            BooleanOperator => {
                if node.count_specific_ancestors::<PythonParser>(
                    |node| node.kind_id() == BooleanOperator,
                    python_is_lambda,
                ) == 0
                {
                    stats.structural +=
                        node.count_specific_ancestors::<PythonParser>(python_is_lambda, |node| {
                            matches!(
                                node.kind_id().into(),
                                ExpressionList | IfStatement | ForStatement | WhileStatement
                            )
                        });
                }
                compute_booleans(node, stats, And, Or);
            }
            // `Lambda` (196) is the emitted lambda; `Lambda2` (197) is the
            // hidden alias `python_is_lambda` also accepts. A match arm
            // cannot route through the predicate, so the alias set is
            // spelled out here and kept in sync with it (#422; the
            // drift guard in checker.rs flags a bump that emits Lambda2).
            Lambda | Lambda2 => {
                // Increase lambda nesting
                lambda += 1;
            }
            FunctionDefinition => {
                // Increase depth function nesting if needed
                increment_function_depth(&mut depth, node, &[FunctionDefinition]);
            }
            _ => {}
        }
        // Add node to nesting map
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for RustCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Rust::*;
        // Macro expansion is not tracked; macros are treated as opaque tokens.
        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfExpression if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForExpression | WhileExpression | LoopExpression | MatchExpression => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            Else /*else-if also */ => {
                increment_by_one(stats);
            }
            BreakExpression | ContinueExpression => {
                if let Some(label_child) = node.child(1)
                    && let Label = label_child.kind_id().into()
                {
                    increment_by_one(stats);
                }
            }
            // `LetChain` (the visible alias) and `LetChain2` (the hidden
            // `_let_chain` supertype) are Rust 2024 let-chains:
            // `if let Some(x) = a && let Some(y) = b && cond`. Their `&&`
            // tokens are direct children of the chain node — not wrapped
            // in `BinaryExpression` — so without dispatching them through
            // `compute_booleans` here, let-chain `&&` is invisible to the
            // boolean-sequence counter (issue #396). Cyclomatic already
            // counts the same tokens via the AMPAMP keyword arm.
            BinaryExpression | LetChain | LetChain2 => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            FunctionItem => {
                nesting = 0;
                // Increase depth function nesting if needed
                increment_function_depth(&mut depth, node, &[FunctionItem]);
            }
            ClosureExpression => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for CppCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Cpp::*;

        // Macro expansion is not tracked; macros are treated as opaque tokens.
        let (mut nesting, depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement
            | ForRangeLoop
            | WhileStatement
            | DoStatement
            | SwitchStatement
            | CatchClause
            | ConditionalExpression => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            GotoStatement | Else /* else-if also */ => {
                increment_by_one(stats);
            }
            BinaryExpression2 => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            LambdaExpression => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

macro_rules! js_cognitive {
    ($lang:ident) => {
        fn compute<'a>(
            node: &Node<'a>,
            _code: &'a [u8],
            stats: &mut Stats,
            nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
        ) {
            use $lang::*;
            let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

            match node.kind_id().into() {
                IfStatement if !Self::is_else_if(node) => {
                    increase_nesting(stats, &mut nesting, depth, lambda);
                }
                ForStatement | ForInStatement | WhileStatement | DoStatement | SwitchStatement | CatchClause | TernaryExpression => {
                    increase_nesting(stats, &mut nesting, depth, lambda);
                }
                Else /* else-if also */ => {
                    increment_by_one(stats);
                }
                // Per SonarSource Cognitive Complexity §B2, a labeled
                // `break LABEL` / `continue LABEL` is an unstructured jump
                // and adds +1. The JS-family grammar exposes the label as a
                // `StatementIdentifier` child (not the plain `Identifier`
                // Java uses), so gate on that kind; plain `break;` /
                // `continue;` have no such child and add +0.
                BreakStatement | ContinueStatement if node.is_child(StatementIdentifier as u16) => {
                    increment_by_one(stats);
                }
                ExpressionStatement => {
                    // Reset the boolean sequence
                    stats.boolean_seq.reset();
                }
                BinaryExpression => {
                    // `??` (`QMARKQMARK`) short-circuits like `&&` /
                    // `||`, so a chain of `??` collapses to a single
                    // boolean-sequence increment under Sonar B1.
                    compute_booleans_with(node, stats, |id| {
                        matches!(id.into(), AMPAMP | PIPEPIPE | QMARKQMARK)
                    });
                }
                AugmentedAssignmentExpression => {
                    // Compound short-circuit assignments `&&=`, `||=`,
                    // `??=` are semantically `x = x op y` and each carries
                    // one boolean-sequence decision, parallel to the
                    // cyclomatic fix from #231. The operator token sits
                    // inside the augmented-assignment node rather than a
                    // `BinaryExpression`, so it needs its own arm (#236).
                    compute_booleans_with(node, stats, |id| {
                        matches!(id.into(), AMPAMPEQ | PIPEPIPEEQ | QMARKQMARKEQ)
                    });
                }
                FunctionDeclaration => {
                    // Reset lambda nesting at function for JS
                    nesting = 0;
                    lambda = 0;
                    // Increase depth function nesting if needed
                    increment_function_depth(&mut depth, node, &[FunctionDeclaration]);
                }
                ArrowFunction => {
                    lambda += 1;
                }
                _ => {}
            }
            nesting_map.insert(node.id(), (nesting, depth, lambda));
        }
    };
}

impl Cognitive for MozjsCode {
    js_cognitive!(Mozjs);
}

impl Cognitive for JavascriptCode {
    js_cognitive!(Javascript);
}

impl Cognitive for TypescriptCode {
    js_cognitive!(Typescript);
}

impl Cognitive for TsxCode {
    js_cognitive!(Tsx);
}

impl Cognitive for JavaCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Java::*;

        let (mut nesting, depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement
            | EnhancedForStatement
            | WhileStatement
            | DoStatement
            | SwitchBlock
            | CatchClause
            | TernaryExpression => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            Else /* else-if also */ => {
                increment_by_one(stats);
            }
            // Per SonarSource Cognitive Complexity §B2, labeled `break LABEL`
            // and `continue LABEL` each add +1 for breaking the structured
            // control flow. Plain `break;` / `continue;` are not penalized.
            BreakStatement | ContinueStatement
                if node.is_child(Identifier as u16) =>
            {
                increment_by_one(stats);
            }
            BinaryExpression => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            LambdaExpression => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for GroovyCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Groovy::*;

        let (mut nesting, depth, lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `for_in_statement` is the dekobon grammar's distinct node
            // for `for (x in xs)` / `for (Foo x : xs)` (the prior amaanq
            // grammar called this `enhanced_for_statement`); `do_while`
            // and `switch_block` keep their familiar names.
            ForStatement | ForInStatement | WhileStatement | DoWhileStatement | SwitchBlock
            | CatchClause | TernaryExpression => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `Else` covers plain `else` blocks *and* the chained
            // `else if` form, because the grammar inlines the
            // `else` token before the nested `if_statement` rather
            // than wrapping it in an `else_clause` node.
            Else => {
                increment_by_one(stats);
            }
            // SonarSource B2: labeled break/continue each +1 for breaking
            // structured control flow. Same shape as Java.
            BreakStatement | ContinueStatement if node.is_child(Identifier as u16) => {
                increment_by_one(stats);
            }
            BinaryExpression => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            // Groovy's Elvis `?:` is a short-circuit nullish operator
            // analogous to Kotlin's `?:` (#239) and JS `??`. Per
            // SonarSource B1, a chain of identical short-circuit
            // operators contributes a single boolean-sequence increment
            // — the same rule as `&&` / `||`. The dekobon grammar
            // models Elvis as a distinct `elvis_expression` node
            // rather than a Java-shaped `ternary_expression` with a
            // missing consequence (closes #246).
            ElvisExpression => {
                compute_booleans_with(node, stats, |id| matches!(id.into(), QMARKCOLON));
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for CsharpCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Csharp::*;

        let (mut nesting, depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | SwitchStatement
            | SwitchExpression
            | CatchClause
            | ConditionalExpression => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `else` is an anonymous keyword token. Each occurrence carries
            // a flat +1 for the alternative branch (matches Java's `Else`
            // handling).
            Else => {
                increment_by_one(stats);
            }
            // Per SonarSource Cognitive Complexity §B2, any `goto` (including
            // `goto label`, `goto case x`, `goto default`) is an unstructured
            // jump and adds +1. C#'s grammar does not allow labeled
            // `break`/`continue` (those forms are syntactically rejected), so
            // the only labeled-jump form to handle here is `goto_statement`.
            GotoStatement => {
                increment_by_one(stats);
            }
            BinaryExpression => {
                // C#'s null-coalescing `??` short-circuits like `&&` /
                // `||` and forms boolean sequences alongside them.
                // Mirrors the C# cyclomatic operator set.
                compute_booleans_with(node, stats, |id| {
                    matches!(id.into(), AMPAMP | PIPEPIPE | QMARKQMARK)
                });
            }
            AssignmentExpression => {
                // C#'s compound null-coalescing assignment `??=` is
                // semantically `x = x ?? y` and carries one boolean-
                // sequence decision, parallel to the cyclomatic fix
                // from #231. The operator token sits inside the
                // `assignment_expression` node rather than a
                // `BinaryExpression`, so it needs its own arm (#236).
                // C# grammar does not provide `&&=` or `||=`, so only
                // `??=` matters here.
                compute_booleans_with(node, stats, |id| matches!(id.into(), QMARKQMARKEQ));
            }
            LambdaExpression | AnonymousMethodExpression => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for PerlCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Perl as P;

        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // tree-sitter-perl parses `elsif_clause` as a direct child of
            // the surrounding `if_statement` (not as a nested `if`), so the
            // `IfStatement` arm here always increases nesting and the
            // `Else | ElsifClause` arm below carries the flat +1.
            P::IfStatement
            | P::UnlessStatement
            | P::WhileStatement
            | P::UntilStatement
            | P::ForStatement1
            | P::ForStatement2
            | P::TernaryExpression
            // Postfix conditional / loop forms (`return 1 if $cond;`) — the
            // condition is a real cognitive branch and contributes nesting
            // even though the body is a single expression.
            | P::IfSimpleStatement
            | P::UnlessSimpleStatement
            | P::WhileSimpleStatement
            | P::UntilSimpleStatement
            | P::ForSimpleStatement => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `else` and `elsif` each contribute a flat +1.
            P::Else | P::ElsifClause => {
                increment_by_one(stats);
            }
            // SonarSource §B2: `goto` is a non-local jump and adds +1.
            // `goto LABEL;` parses as `goto_expression` wrapping the
            // anonymous `goto` keyword token; the walker visits both, so
            // matching only the `GotoExpression` statement node counts the
            // jump once (matching `P::Goto` too would double-count — #450).
            P::GotoExpression => {
                increment_by_one(stats);
            }
            // SonarSource §B2: labeled `last LABEL` / `next LABEL` /
            // `redo LABEL` each add +1 for breaking structured control
            // flow; bare `last;` / `next;` / `redo;` are +0. The jump
            // target is carried as an `Identifier` child of
            // `loop_control_statement` (`Label` is the loop-*definition*
            // node `OUTER:`, never the target — gating on it was a dead
            // arm, #450).
            P::LoopControlStatement if node.is_child(P::Identifier as u16) => {
                increment_by_one(stats);
            }
            P::BinaryExpression => {
                compute_perl_booleans(node, stats);
            }
            P::FunctionDefinition | P::FunctionDefinitionWithoutSub => {
                nesting = 0;
                increment_function_depth(
                    &mut depth,
                    node,
                    &[P::FunctionDefinition, P::FunctionDefinitionWithoutSub],
                );
            }
            P::AnonymousFunction => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for KotlinCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Kotlin::*;

        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfExpression if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement | WhileStatement | DoWhileStatement | WhenExpression | CatchBlock => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            Else => {
                // Per the SonarSource spec, `else ->` inside a `when`
                // expression is the default arm of a switch-like construct
                // and should be +0, not +1.
                let in_when = node.parent().is_some_and(|p| p.kind_id() == WhenEntry);
                if !in_when {
                    increment_by_one(stats);
                }
            }
            // SonarSource §B2: labeled `break@outer` / `continue@outer`
            // each add +1 for breaking structured control flow; bare
            // `break` / `continue` are +0. tree-sitter-kotlin-ng has no
            // break/continue/jump statement kind — it models a labeled
            // jump as a `labeled_expression` wrapping the `break@`/
            // `continue@` token, while `return@label` is a distinct
            // `return_expression` (so it is correctly excluded here). A
            // bare `break`/`continue` parses as a plain identifier, never
            // a `labeled_expression`, so it never matches (#450).
            LabeledExpression => {
                increment_by_one(stats);
            }
            // SonarSource §B2: labeled `break@outer` / `continue@outer`
            // each add +1 for breaking structured control flow; bare
            // `break` / `continue` are +0. tree-sitter-kotlin-ng has no
            // break/continue/jump statement kind — it models a labeled
            // jump as a `labeled_expression` wrapping the `break@`/
            // `continue@` token, while `return@label` is a distinct
            // `return_expression` (so it is correctly excluded here). A
            // bare `break`/`continue` parses as a plain identifier, never
            // a `labeled_expression`, so it never matches (#450).
            BinaryExpression => {
                // Kotlin's Elvis operator `?:` (token `QMARKCOLON`) is a
                // short-circuit nullish operator analogous to JS `??` and
                // forms boolean sequences alongside `&&` / `||` per
                // SonarSource Cognitive Complexity B1.
                compute_booleans_with(node, stats, |id| {
                    matches!(id.into(), AMPAMP | PIPEPIPE | QMARKCOLON)
                });
            }
            FunctionDeclaration | SecondaryConstructor => {
                nesting = 0;
                increment_function_depth(
                    &mut depth,
                    node,
                    &[FunctionDeclaration, SecondaryConstructor],
                );
            }
            LambdaLiteral | AnonymousFunction => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for GoCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Go as G;

        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            G::IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            G::ForStatement
            | G::ExpressionSwitchStatement
            | G::TypeSwitchStatement
            | G::SelectStatement => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            G::Else | G::GotoStatement => {
                increment_by_one(stats);
            }
            G::BreakStatement | G::ContinueStatement if node.is_child(G::LabelName as u16) => {
                increment_by_one(stats);
            }
            G::BinaryExpression => {
                compute_booleans(node, stats, G::AMPAMP, G::PIPEPIPE);
            }
            G::FunctionDeclaration | G::MethodDeclaration => {
                nesting = 0;
                increment_function_depth(
                    &mut depth,
                    node,
                    &[G::FunctionDeclaration, G::MethodDeclaration],
                );
            }
            G::FuncLiteral => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for BashCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Bash::*;

        let (mut nesting, mut depth, lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // `WhileStatement` covers both `while` and `until`; `ForStatement`
            // covers both `for` and `select`. `CStyleForStatement` is the
            // `for ((…))` arithmetic form. `ElifClause` is a dedicated node,
            // not a nested `if`, so no `is_else_if` check is needed.
            IfStatement | WhileStatement | ForStatement | CStyleForStatement | CaseStatement => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ElifClause | ElseClause => {
                increment_branch_extension(stats);
            }
            // `&&` / `||` appear in two places: as direct children of
            // `Bash::List` (command level: `cmd && cmd`) and as direct
            // children of `Bash::BinaryExpression3` (inside `[[ … ]]`,
            // `(( … ))`, c-style `for ((…))` conditions, and
            // parenthesized sub-expressions). Verified empirically
            // against tree-sitter-bash 0.25.1 — the other four
            // `BinaryExpression*` enum variants never wrap `&&` / `||`.
            List | BinaryExpression3 => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            FunctionDefinition => {
                nesting = 0;
                increment_function_depth(&mut depth, node, &[FunctionDefinition]);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for TclCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Tcl::*;

        let (mut nesting, mut depth, lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // Guard kept for defensive consistency with sibling impls; Tcl's dedicated
            // Elseif node means this guard is always true in practice.
            If if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // elseif adds +1 without increasing nesting for its own children.
            Elseif => {
                increment_branch_extension(stats);
            }
            Else => {
                increment_by_one(stats);
            }
            While | Foreach | TernaryExpr => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `catch` is a conditional error handler; only executes when the body errors.
            Catch => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            BinopExpr => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            Procedure => {
                nesting = 0;
                increment_function_depth(&mut depth, node, &[Procedure]);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for LuaCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Lua::*;

        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // `is_else_if` returns true for `ElseifStatement`, but Lua's
            // grammar makes that node a child field of `IfStatement` rather
            // than a nested `if_statement`, so the guard is defensive only.
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `elseif` adds +1 at the same nesting level as the parent `if`,
            // matching how Tcl/Bash handle their dedicated elseif/elif nodes.
            ElseifStatement => {
                increment_branch_extension(stats);
            }
            ForStatement | WhileStatement | RepeatStatement => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `else` increments without nesting. Lua's `break` is always
            // unlabeled (the grammar has no labeled break, and no
            // `continue`), so per SonarSource Cognitive Complexity §B2 it
            // adds +0 — the enclosing loop's nesting already accounts for
            // it. Only `goto label` is a genuinely unstructured jump and
            // adds +1.
            ElseStatement | GotoStatement => {
                increment_by_one(stats);
            }
            BinaryExpression => {
                compute_booleans(node, stats, And, Or);
            }
            FunctionDeclaration | FunctionDeclaration2 | FunctionDeclaration3 => {
                nesting = 0;
                increment_function_depth(
                    &mut depth,
                    node,
                    &[
                        FunctionDeclaration,
                        FunctionDeclaration2,
                        FunctionDeclaration3,
                    ],
                );
            }
            FunctionDefinition => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

impl Cognitive for PhpCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Php::*;

        let (mut nesting, depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement
            | ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | SwitchStatement
            | MatchExpression
            | CatchClause
            | ConditionalExpression => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ElseClause | ElseClause2 | ElseIfClause | ElseIfClause2 => {
                increment_branch_extension(stats);
            }
            // Per SonarSource Cognitive Complexity §B2, `goto label;` is an
            // unstructured jump and adds +1 (matching C++/C#/Go/Perl/Lua
            // goto). PHP has no *labeled* `break`/`continue`; its only
            // non-default jump argument is the numeric level form
            // `break N;` / `continue N;`, which breaks out of N enclosing
            // loops. Those enclosing loops are already counted via nesting,
            // so the numeric form is a structured loop-level exit and adds
            // +0 — only `goto` is genuinely unstructured here.
            GotoStatement => {
                increment_by_one(stats);
            }
            BinaryExpression => {
                // PHP's null-coalescing `??` short-circuits like `&&` /
                // `||` and the word-form `and` / `or` / `xor`, so it
                // forms boolean sequences alongside them. Mirrors the
                // PHP cyclomatic operator set minus the assignment
                // form `??=`, which is not a `BinaryExpression`.
                compute_booleans_with(node, stats, |id| {
                    matches!(id.into(), AMPAMP | PIPEPIPE | And | Or | Xor | QMARKQMARK)
                });
            }
            AugmentedAssignmentExpression => {
                // PHP's `??=` is `x = x ?? y` and carries one boolean-
                // sequence decision, parallel to the cyclomatic fix
                // from #231. The token sits inside the augmented-
                // assignment container rather than a `BinaryExpression`,
                // so it needs its own arm (#236). PHP grammar has no
                // `&&=` / `||=`.
                compute_booleans_with(node, stats, |id| matches!(id.into(), QMARKQMARKEQ));
            }
            AnonymousFunction | ArrowFunction => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

// Reads the text of the `target` field of an Elixir `Call` node.
//
// Most of Elixir's control-flow constructs (`if`, `unless`, `for`,
// `while`, `case`, `cond`, `with`, `try`) and method-defining macros
// (`def`, `defp`, `defmacro`, …) parse as `Call` nodes whose `target`
// is an `Identifier` whose source text spells the keyword. The
// `Cyclomatic` and `Exit` impls already follow this pattern; this
// helper centralises the byte-text lookup so `Cognitive` and `Abc`
// can share it.
//
// Returns `None` for Calls whose target is not a simple identifier
// (e.g. `Module.func(…)` parses as `RemoteCallWithParentheses` with
// the dotted name as target) or when the bytes are not valid UTF-8.
pub(crate) fn elixir_call_keyword<'a>(node: &'a Node<'a>, code: &'a [u8]) -> Option<&'a str> {
    if node.kind_id() != Elixir::Call as u16 {
        return None;
    }
    let target = node.child_by_field_name("target")?;
    if target.kind_id() != Elixir::Identifier as u16 {
        return None;
    }
    target.utf8_text(code)
}

// Method-defining macros (`def`, `defp`, `defmacro`, `defmacrop`). The set
// is duplicated across checker, getter, and several metric impls
// because each consults it from a different trait surface; centralising
// the literal here keeps future additions (e.g. `defguard`) consistent.
#[inline]
pub(crate) fn elixir_is_method_macro(kw: &str) -> bool {
    matches!(kw, "def" | "defp" | "defmacro" | "defmacrop")
}

// Class-defining macro (`defmodule`). Paired with [`elixir_is_method_macro`]
// where a caller needs both ("any space-opening declaration").
#[inline]
pub(crate) fn elixir_is_class_macro(kw: &str) -> bool {
    kw == "defmodule"
}

// Returns true when `node` is lexically nested inside the `do_block` of a
// `quote do … end` Call (Elixir's metaprogramming template). A `def` /
// `defp` / `defmacro` / `defmacrop` inside `quote` does not define a
// method of any enclosing module — the syntax tree is a code template
// emitted later, when the surrounding macro is invoked. Treating those
// quoted Calls as methods inflates `Wmc` and disagrees with `Npm`'s
// direct-children classification (#310).
//
// Walks the parent chain looking for a `quote` Call ancestor. Stops at
// the first match (true) or at the root (false). O(depth); each step is
// a single `child_by_field_name("target")` + identifier byte compare.
pub(crate) fn elixir_is_inside_quote_block(node: &Node<'_>, code: &[u8]) -> bool {
    let mut current = node.parent();
    while let Some(n) = current {
        if elixir_call_keyword(&n, code) == Some("quote") {
            return true;
        }
        current = n.parent();
    }
    false
}

// Iterates the direct-child `Call` nodes inside the `do_block` of an
// Elixir Call (typically a `defmodule`). Used by `Npm` / `Npa` to scan
// a module body for method-defining macros / `defstruct` without
// descending into nested modules. Yields no items when the Call has
// no `do_block`.
pub(crate) fn elixir_do_block_call_children<'a>(
    node: &'a Node<'a>,
) -> impl Iterator<Item = Node<'a>> + 'a {
    node.children()
        .filter(|child| child.kind_id() == Elixir::DoBlock as u16)
        .flat_map(|do_block| do_block.children())
        .filter(|stmt| stmt.kind_id() == Elixir::Call as u16)
}

impl Cognitive for ElixirCode {
    // Elixir control flow is macro-shaped: `if`, `unless`, `case`,
    // `cond`, `with`, `for`, `while`, and `try` each surface as a
    // `Call` node whose `target` Identifier text spells the keyword.
    // We classify the Call once on entry (raising nesting), then let
    // the structural `Else` token and `Rescue` / `Catch` blocks inside
    // the do_block contribute their own cost without double-counting.
    //
    // Mapping (mirrors the Java/Kotlin SonarSource interpretation for
    // switch-like constructs):
    // - `if` / `unless` / `for` / `while`: single-branch control flow,
    //   `+nesting`. Their `else` (token `Elixir::Else` inside an
    //   `ElseBlock`) adds `+1` without nesting, matching Java.
    // - `case` / `cond` / `with` / `try`: switch-/multi-arm, `+nesting`
    //   once on the container. Individual `stab_clause` arms do NOT
    //   add extra cost (matches Java `SwitchBlock` / `case:` rule).
    //   `try`'s `rescue` / `catch` arms surface as `RescueBlock` /
    //   `CatchBlock` and each one adds `+nesting`, matching Java's
    //   `CatchClause` treatment.
    // - `def` / `defp` / `defmacro` / `defmacrop`: method-defining
    //   macros. Treated like Bash's `FunctionDefinition` — nesting
    //   resets, function depth bumps so nested functions amplify cost.
    // - `AnonymousFunction` (`fn x -> y end`): lambda nesting bumps.
    // - `&&` / `||` / `and` / `or`: boolean sequence cost.
    //
    // Limitations:
    // - `Enum.reduce` / `Enum.map` and friends are higher-order function
    //   calls (`RemoteCallWithParentheses`), not syntactic control flow.
    //   Cognitive complexity per the SonarSource spec does NOT count
    //   function calls; we follow suit.
    // - Recursion detection is intentionally omitted. The SonarSource
    //   spec scores recursion at +1, but reliably detecting recursion
    //   needs symbol-table awareness (the body of `def foo do foo() end`
    //   must compare names) which is out of scope for this fix. See
    //   the issue body's explicit "skip if too complex" guidance.
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Elixir as E;

        let (mut nesting, depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            E::Call => match elixir_call_keyword(node, code) {
                Some("if" | "unless" | "for" | "while" | "case" | "cond" | "with") => {
                    increase_nesting(stats, &mut nesting, depth, lambda);
                }
                // `try` is intentionally absent: it is a wrapper for
                // `rescue` / `catch` arms (each of which earns its own
                // +nesting via `RescueBlock` / `CatchBlock`). Adding the
                // `try` itself would double-count, matching Java /
                // C#'s "try is a wrapper, only catch counts" rule.
                Some(kw) if elixir_is_method_macro(kw) => {
                    // Method-defining macros reset nesting at the
                    // function boundary, mirroring Bash's
                    // `FunctionDefinition` rule. We deliberately do
                    // NOT call `increment_function_depth` here: that
                    // helper matches `Call` ancestors by `kind_id`
                    // alone, and EVERY `def` inside a `defmodule` has
                    // a `Call` ancestor (the defmodule Call) which
                    // would falsely raise the function depth. Elixir
                    // does not allow `def` nested inside another
                    // `def` (defs only live at module top level), so
                    // truly nested method definitions are not a
                    // concern — the lambda channel via
                    // `AnonymousFunction` handles the analogous
                    // higher-order case.
                    nesting = 0;
                }
                _ => {}
            },
            // `else` keyword inside an `else_block` (else arm of `if`
            // / `unless` / `with` / `try`). Matches Java/Kotlin's
            // `Else` rule: +1 without raising nesting.
            E::Else => {
                increment_by_one(stats);
            }
            // `rescue` / `catch` arms of a `try` Call each add +nesting,
            // matching Java's `CatchClause` treatment.
            E::RescueBlock | E::CatchBlock => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // Anonymous functions are Elixir's lambdas. Increment the
            // lambda depth so the cost of control flow inside them is
            // amplified, matching Kotlin's `LambdaLiteral` rule.
            E::AnonymousFunction => {
                lambda += 1;
            }
            // Short-circuit booleans (token-form `&&` / `||` and word-
            // form `and` / `or`) contribute one structural cost per
            // operator sequence. Single-pass helper (see
            // `compute_elixir_booleans`) collapses the four operator
            // kinds in one walk of `node.children()` — the previous
            // shape called `compute_booleans` twice, walking children
            // twice per BinaryOperator.
            E::BinaryOperator | E::BinaryOperator2 | E::BinaryOperator3 => {
                compute_elixir_booleans(node, stats);
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

implement_metric_trait!(Cognitive, PreprocCode, CcommentCode);

impl Cognitive for RubyCode {
    fn compute<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Ruby as R;

        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // Nesting-increasing constructs. tree-sitter-ruby models
            // `elsif` as its own `Elsif` clause (handled in the
            // branch-extension arm below) rather than nesting a second
            // `If` inside the outer one, so the `is_else_if` guard is
            // defensive only — mirrors the equivalent pattern in the
            // Lua impl.
            R::If if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            R::Unless
            | R::While
            | R::Until
            | R::For
            | R::Case
            | R::CaseMatch
            | R::Conditional
            | R::IfModifier
            | R::UnlessModifier
            | R::WhileModifier
            | R::UntilModifier
            | R::Rescue
            | R::RescueModifier
            | R::RescueModifier2
            | R::RescueModifier3 => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `elsif`/`else` extend the parent branch at the same nesting
            // level. The `Else` clause node also appears for `case/when`
            // and `begin/rescue` else branches and is treated uniformly.
            R::Elsif | R::Else => {
                increment_branch_extension(stats);
            }
            // Ruby has no labeled loops: `break`/`next` are always
            // unlabeled (the token's only optional child is a return-value
            // expression, never a label). Per SonarSource Cognitive
            // Complexity §B2, an unlabeled break/continue adds +0 — the
            // enclosing loop's nesting already accounts for it — so they are
            // intentionally excluded here. `redo` (restart the current loop
            // iteration) and `retry` (re-run a rescued `begin` block) are
            // genuinely unstructured jumps with no structured equivalent and
            // each add +1.
            R::Redo | R::Retry => {
                increment_by_one(stats);
            }
            R::Binary | R::Binary2 | R::Binary3 => {
                // Ruby has four short-circuit forms (`&&`, `||`, `and`,
                // `or`); use the dedicated helper rather than the
                // two-operator `compute_booleans` so word-form
                // operators land in the sequence too.
                compute_ruby_booleans(node, stats);
            }
            R::Method | R::SingletonMethod => {
                nesting = 0;
                increment_function_depth(&mut depth, node, &[R::Method, R::SingletonMethod]);
            }
            // Blocks, do-blocks and lambdas are the closure/lambda forms.
            R::Block | R::DoBlock | R::Lambda => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names,
    clippy::doc_markdown,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines
)]
mod tests {
    use crate::tools::check_metrics;

    use super::*;

    /// A `Stats::default()` that never sees an
    /// observation must not leak the `usize::MAX` sentinel for
    /// `structural_min`. The getter collapses the sentinel to `0.0`
    /// so JSON never emits `1.8446744e19`.
    #[test]
    fn cognitive_empty_file_min_is_zero() {
        let stats = Stats::default();
        assert_eq!(stats.cognitive_min(), 0.0);
    }

    #[test]
    fn python_no_cognitive() {
        check_metrics::<PythonParser>("a = 42", "foo.py", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn rust_no_cognitive() {
        check_metrics::<RustParser>("let a = 42;", "foo.rs", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn c_no_cognitive() {
        check_metrics::<CppParser>("int a = 42;", "foo.c", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn mozjs_no_cognitive() {
        check_metrics::<MozjsParser>("var a = 42;", "foo.js", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn javascript_no_cognitive() {
        check_metrics::<JavascriptParser>("var a = 42;", "foo.js", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn python_simple_function() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a and b:  # +2 (+1 and)
                   return 1
                if c and d: # +2 (+1 and)
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    /// Python `match`/`case` (PEP 634, 3.10+) opens cognitive nesting
    /// the same way Rust's `match_expression` and the C-family
    /// `switch_statement` do. A 2-arm match with one explicit arm
    /// plus a wildcard contributes one cognitive decision point.
    /// Regression test for #212.
    #[test]
    fn python_match_two_arm_wildcard() {
        check_metrics::<PythonParser>(
            "def f(x):
    match x:
        case 1:
            return 'one'
        case _:
            return 'other'
",
            "foo.py",
            |metric| {
                // The `match_statement` contributes one decision point;
                // case arms inside add no extra nesting (mirrors Rust /
                // C-family switch). cognitive_max = 1.
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_expression_statement() {
        // Boolean expressions containing `And` and `Or` operators were not
        // considered in assignments
        check_metrics::<PythonParser>(
            "def f(a, b):
                c = True and True",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_tuple() {
        // Boolean expressions containing `And` and `Or` operators were not
        // considered inside tuples
        check_metrics::<PythonParser>(
            "def f(a, b):
                return \"%s%s\" % (a and \"Get\" or \"Set\", b)",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_elif_function() {
        // Boolean expressions containing `And` and `Or` operators were not
        // considered in `elif` statements
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a and b:  # +2 (+1 and)
                   return 1
                elif c and d: # +2 (+1 and)
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_more_elifs_function() {
        // Boolean expressions containing `And` and `Or` operators were not
        // considered when there were more `elif` statements
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a and b:  # +2 (+1 and)
                   return 1
                elif c and d: # +2 (+1 and)
                   return 1
                elif e and f: # +2 (+1 and)
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_if_elif_elif_else_chain() {
        // Regression for #274: `if/elif/elif/else` must score as a flat
        // branch chain (each continuation contributes +1 with no extra
        // nesting). `ElifClause` is a dedicated node handled directly
        // by the cognitive dispatch as a branch extension, and the
        // generic `count_specific_ancestors` nesting walk does not
        // include `ElifClause` in its kind sets, so no ancestor-side
        // suppression via `is_else_if` is required.
        // expected: outer if +1, elif +1, elif +1, else +1 = 4.
        check_metrics::<PythonParser>(
            "def f(a, b, c, d):
                if a:
                   return 1
                elif b:
                   return 2
                elif c:
                   return 3
                else:
                   return 4",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_else_if_chain_matches_elif() {
        // Regression for #276: `else: if x:` (no `elif`) is semantically
        // an else-if chain and must score the same as the `elif`
        // equivalent. Before the fix, the inner `if_statement` was
        // double-counted (nesting +2 instead of +1), inflating the
        // cognitive score linearly with chain length.
        // expected: outer if +1, boolean `and` +1, else_clause +1,
        //   inner if suppressed by is_else_if, inner boolean `and` +1
        //   = 4 — matching the `elif` form above (python_elif_function).
        check_metrics::<PythonParser>(
            "def f(a, b, c, d):
                if a and b:
                   return 1
                else:
                   if c and d:
                      return 1",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_try_except_finally_finally_is_free() {
        // Regression for #416: a `finally` clause is structured cleanup that
        // always runs and must add 0 per the SonarSource Cognitive Complexity
        // spec. try/except/finally must score the same as try/except.
        // expected: except +1, finally +0 = 1.
        check_metrics::<PythonParser>(
            "def f():
                try:
                    x = risky()
                except ValueError:
                    x = 1
                finally:
                    cleanup()
                return x",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_try_except_matches_try_except_finally() {
        // Companion to #416: try/except (no finally) scores the same as the
        // try/except/finally form above, proving `finally` is free.
        // expected: except +1 = 1.
        check_metrics::<PythonParser>(
            "def f():
                try:
                    x = risky()
                except ValueError:
                    x = 1
                return x",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_comprehension_matches_explicit_loop() {
        // Regression for #417: a list comprehension's `for`/`if` clauses must
        // carry the same cognitive load as the explicit loop+condition they
        // desugar to. `[x for x in xs if x > 0]` was scoring 0 while the
        // equivalent explicit `for`/`if` scored 3.
        // expected: for_in_clause +1 (nesting 0), if_clause +2 (1 base +
        // 1 nesting under the for) = 3 — equal to the explicit form below.
        check_metrics::<PythonParser>(
            "def f(xs):
                return [x for x in xs if x > 0]",
            "foo.py",
            |metric| {
                // cyclomatic 4 = unit base 1 + for 1 + if 1 + function base 1.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
            },
        );
        check_metrics::<PythonParser>(
            "def g(xs):
                out = []
                for x in xs:
                    if x > 0:
                        out.append(x)
                return out",
            "foo.py",
            |metric| {
                // The explicit loop+if form the comprehension above desugars
                // to: for +1, nested if +2 = 3 (cognitive), matching f.
                // cyclomatic 4 matches f as well, confirming agreement.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
            },
        );
    }

    #[test]
    fn python_comprehension_plain_no_filter() {
        // A comprehension with no `if` filter scores just the loop.
        // expected: for_in_clause +1 = 1.
        check_metrics::<PythonParser>(
            "def f(xs):
                return [x for x in xs]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                // cyclomatic 3 = unit base 1 + for 1 + function base 1.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
            },
        );
    }

    #[test]
    fn python_comprehension_nested_for() {
        // Two `for` clauses are nested loops: the second nests under the
        // first, mirroring explicit nested `for` statements.
        // expected: for #1 +1 (nesting 0), for #2 +2 (1 base + 1 nesting) = 3.
        check_metrics::<PythonParser>(
            "def f(xs, ys):
                return [a for a in xs for b in ys]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                // cyclomatic 4 = unit base 1 + for 1 + for 1 + function base 1.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
            },
        );
    }

    #[test]
    fn python_comprehension_multiple_filters() {
        // Each `if` filter is an independent condition nested under the for.
        // Cognitive penalizes the nesting, so it exceeds cyclomatic here; the
        // two metrics legitimately diverge once filters multiply.
        // expected cognitive: for +1, if #1 +2, if #2 +2 = 5.
        check_metrics::<PythonParser>(
            "def f(xs):
                return [x for x in xs if a if b]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 5.0);
                // cyclomatic 5 = unit base 1 + for 1 + if 1 + if 1 + fn base 1.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
            },
        );
    }

    #[test]
    fn python_comprehension_variants_consistent() {
        // dict / set / generator comprehensions reuse the same for_in_clause /
        // if_clause node kinds as the list form, so all must score identically
        // to `[x for x in xs if x > 0]` (cognitive 3).
        // expected: for +1, if +2 = 3 for each variant.
        for body in [
            "{x: y for x, y in xs if x > 0}",
            "{x for x in xs if x > 0}",
            "(x for x in xs if x > 0)",
        ] {
            check_metrics::<PythonParser>(
                &format!("def f(xs):\n                return {body}"),
                "foo.py",
                |metric| {
                    assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                    // cyclomatic 4 = unit base 1 + for 1 + if 1 + fn base 1,
                    // identical to the list form, for every variant.
                    assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                },
            );
        }
    }

    #[test]
    fn python_comprehension_nested_in_element() {
        // Regression for #421: a comprehension in another comprehension's
        // element position must carry the full nesting of the outer loop+
        // filter, not the shallow depth the #417 sibling write-back left it
        // with (it under-counted at 6). The element is traversed before the
        // outer clauses, so the depth is established on the comprehension node
        // itself, independent of sibling traversal order.
        // expected cognitive: outer for +1 (nesting 0), outer if +2
        // (nesting 1), inner for +3 (nesting 2), inner if +4 (nesting 3) = 10.
        check_metrics::<PythonParser>(
            "def f(xs):
                return [[y for y in x if y] for x in xs if x]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 10.0);
            },
        );
        // The explicit doubly-nested loop+if form it desugars to: for +1,
        // if +2, for +3, if +4 = 10, matching the comprehension above.
        check_metrics::<PythonParser>(
            "def g(xs):
                out = []
                for x in xs:
                    if x:
                        for y in x:
                            if y:
                                out.append(y)
                return out",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 10.0);
            },
        );
    }

    #[test]
    fn python_comprehension_three_levels_nested() {
        // Three comprehensions nested through each other's element positions
        // must equal their explicit triply-nested loop+if form at every depth.
        // expected cognitive: for/if pairs at nesting 0..5 =
        // 1+2+3+4+5+6 = 21.
        check_metrics::<PythonParser>(
            "def f(xss):
                return [[[z for z in y if z] for y in x if y] for x in xss if x]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 21.0);
            },
        );
        check_metrics::<PythonParser>(
            "def g(xss):
                out = []
                for x in xss:
                    if x:
                        for y in x:
                            if y:
                                for z in y:
                                    if z:
                                        out.append(z)
                return out",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 21.0);
            },
        );
    }

    #[test]
    fn python_generator_in_comprehension_element() {
        // #421 edge case: a generator passed to a call (`sum(...)`) in a
        // comprehension's element still inherits the outer loop+filter depth
        // through the intervening call/argument_list nodes.
        // expected cognitive: outer for +1, outer if +2, inner for +3,
        // inner if +4 = 10.
        check_metrics::<PythonParser>(
            "def f(xs):
                return [sum(y for y in x if y) for x in xs if x]",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 10.0);
            },
        );
        check_metrics::<PythonParser>(
            "def g(xs):
                out = []
                for x in xs:
                    if x:
                        out.append(sum(y for y in x if y))
                return out",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 10.0);
            },
        );
    }

    #[test]
    fn python_try_finally_no_except_is_free() {
        // #416: try/finally with no except clause scores 0 — neither the try
        // body nor the finally cleanup carries any cognitive cost on its own.
        // expected: 0.
        check_metrics::<PythonParser>(
            "def f():
                try:
                    x = risky()
                finally:
                    cleanup()
                return x",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 0.0,
                      "average": 0.0,
                      "min": 0.0,
                      "max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_constructs_inside_finally_still_count() {
        // #416 guard: making `finally` free must not make its body invisible.
        // The finally clause itself carries no nesting increment (it never
        // called `increase_nesting`), so an `if` directly inside it is at
        // nesting depth 0 and contributes its +1 base cost.
        // expected: if inside finally = +1.
        check_metrics::<PythonParser>(
            "def f():
                try:
                    x = risky()
                finally:
                    if x:
                        cleanup()",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_simple_function() {
        check_metrics::<RustParser>(
            "fn f() {
                 if a && b { // +2 (+1 &&)
                     println!(\"test\");
                 }
                 if c && d { // +2 (+1 &&)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_simple_function() {
        check_metrics::<CppParser>(
            "void f() {
                 if (a && b) { // +2 (+1 &&)
                     printf(\"test\");
                 }
                 if (c && d) { // +2 (+1 &&)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_simple_function() {
        check_metrics::<MozjsParser>(
            "function f() {
                 if (a && b) { // +2 (+1 &&)
                     window.print(\"test\");
                 }
                 if (c && d) { // +2 (+1 &&)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_simple_function() {
        check_metrics::<JavascriptParser>(
            "function f() {
                 if (a && b) { // +2 (+1 &&)
                     console.log(\"test\");
                 }
                 if (c || d) { // +2 (+1 ||)
                     console.log(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_sequence_same_booleans() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a and b and True:  # +2 (+1 sequence of and)
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_sequence_same_booleans() {
        check_metrics::<RustParser>(
            "fn f() {
                 if a && b && true { // +2 (+1 sequence of &&)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );

        check_metrics::<RustParser>(
            "fn f() {
                 if a || b || c || d { // +2 (+1 sequence of ||)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    // Regression for issue #396: in Rust 2024 let-chains, the `&&`
    // tokens are direct children of the `_let_chain` / `let_chain`
    // node rather than a `BinaryExpression`. Before #396 these
    // tokens were invisible to the cognitive boolean-sequence
    // counter (cyclomatic already counted them via AMPAMP).
    #[test]
    fn rust_let_chain_sequence_booleans() {
        // expected: +1 for the `if`, +1 for the chain of two `&&`
        // tokens (sequence of same operator collapses to one).
        // Equivalent shape to `if a && b && true { ... }` above,
        // which scores 2.0.
        check_metrics::<RustParser>(
            "fn f(a: Option<i32>, b: Option<i32>) {
                 if let Some(x) = a && let Some(y) = b && x > y { // +2 (+1 sequence of &&)
                     println!(\"both\");
                 }
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 2);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_let_chain_vs_nested_if_let() {
        // Companion to `rust_let_chain_sequence_booleans`. The nested
        // `if let` form has no `&&` and so is unaffected by the #396
        // LetChain dispatch; this test pins that the pre-existing
        // nesting scoring (+1 outer `if`, +2 nested `if` at nesting=1)
        // still yields 3 and that the LetChain arm did not alter it.
        check_metrics::<RustParser>(
            "fn f(a: Option<i32>, b: Option<i32>) {
                 if let Some(x) = a { // +1
                     if let Some(y) = b { // +2 (nesting=1)
                         println!(\"{} {}\", x, y);
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_sequence_same_booleans() {
        check_metrics::<CppParser>(
            "void f() {
                 if (a && b && 1 == 1) { // +2 (+1 sequence of &&)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );

        check_metrics::<CppParser>(
            "void f() {
                 if (a || b || c || d) { // +2 (+1 sequence of ||)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_sequence_same_booleans() {
        check_metrics::<MozjsParser>(
            "function f() {
                 if (a && b && 1 == 1) { // +2 (+1 sequence of &&)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );

        check_metrics::<MozjsParser>(
            "function f() {
                 if (a || b || c || d) { // +2 (+1 sequence of ||)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_not_booleans() {
        check_metrics::<RustParser>(
            "fn f() {
                 if !a && !b { // +2 (+1 &&)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );

        check_metrics::<RustParser>(
            // `!` does not break boolean sequences (issue #392): the
            // outer and inner `&&`s are folded into a single sequence
            // because pre-order visits the outer BinaryExpression first
            // (recording `&&` at its end_byte) and the inner `&&` lies
            // within that span. The `!` arm was dead anyway — it fired
            // after both BinaryExpressions had already been counted.
            "fn f() {
                 if a && !(b && c) { // +2 (+1 if, +1 outer &&; inner && continues)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );

        check_metrics::<RustParser>(
            "fn f() {
                 if !(a || b) && !(c || d) { // +4 (+1 ||, +1 &&, +1 ||)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_not_does_not_affect_boolean_sequence_392() {
        // Regression test for issue #392: `!` does not affect cognitive
        // scoring for a same-operator boolean sequence. `!a && !b && !c`
        // must score identically to `a && b && c` — both are a single
        // `&&` chain under SonarSource's rule B1 (only operator switches
        // start a new sequence). The previously dead `UnaryExpression`
        // arm could not have affected this case either way (pre-order
        // visits the BinaryExpressions before the UnaryExpressions), so
        // this asserts the new and old behaviour agree where it matters.
        // if(+1) + && sequence(+1) = 2; the two trailing `&&`s are
        // continuations because all three share the outer pre-order
        // parent's end_byte.
        check_metrics::<RustParser>(
            "fn f() {
                 if !a && !b && !c {
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
        check_metrics::<RustParser>(
            "fn f() {
                 if a && b && c {
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                // Same sum as the negated form above: `!` is not a
                // boolean-sequence boundary.
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_not_booleans() {
        // `!` does not break boolean sequences (issue #392): the inner
        // `&&` is folded into the outer `&&`'s span because pre-order
        // visits the outer BinaryExpression first.
        check_metrics::<CppParser>(
            "void f() {
                 if (a && !(b && c)) { // +2 (+1 if, +1 outer &&; inner && continues)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );

        check_metrics::<CppParser>(
            "void f() {
                 if (!(a || b) && !(c || d)) { // +4 (+1 ||, +1 &&, +1 ||)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_not_booleans() {
        // `!` does not break boolean sequences (issue #392): inner `&&`
        // continues the outer `&&` sequence (pre-order visits the outer
        // BinaryExpression first, so its end_byte already covers the
        // inner one).
        check_metrics::<MozjsParser>(
            "function f() {
                 if (a && !(b && c)) { // +2 (+1 if, +1 outer &&; inner && continues)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );

        check_metrics::<MozjsParser>(
            "function f() {
                 if (!(a || b) && !(c || d)) { // +4 (+1 ||, +1 &&, +1 ||)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_sequence_different_booleans() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a and b or True:  # +3 (+1 and, +1 or)
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_sequence_different_booleans() {
        check_metrics::<RustParser>(
            "fn f() {
                 if a && b || true { // +3 (+1 &&, +1 ||)
                     println!(\"test\");
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_sequence_different_booleans() {
        check_metrics::<CppParser>(
            "void f() {
                 if (a && b || 1 == 1) { // +3 (+1 &&, +1 ||)
                     printf(\"test\");
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_sequence_different_booleans() {
        check_metrics::<MozjsParser>(
            "function f() {
                 if (a && b || 1 == 1) { // +3 (+1 &&, +1 ||)
                     window.print(\"test\");
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_formatted_sequence_different_booleans() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if (  # +1
                    a and b and  # +1
                    (c or d)  # +1
                ):
                   return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_1_level_nesting() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a:  # +1
                    for i in range(b):  # +2
                        return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_1_level_nesting() {
        check_metrics::<RustParser>(
            "fn f() {
                 if true { // +1
                     if true { // +2 (nesting = 1)
                         println!(\"test\");
                     } else if 1 == 1 { // +1
                         if true { // +3 (nesting = 2)
                             println!(\"test\");
                         }
                     } else { // +1
                         if true { // +3 (nesting = 2)
                             println!(\"test\");
                         }
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 11.0,
                      "average": 11.0,
                      "min": 0.0,
                      "max": 11.0
                    }"###
                );
            },
        );

        check_metrics::<RustParser>(
            "fn f() {
                 if true { // +1
                     match true { // +2 (nesting = 1)
                         true => println!(\"test\"),
                         false => println!(\"test\"),
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_1_level_nesting() {
        check_metrics::<CppParser>(
            "void f() {
                 if (1 == 1) { // +1
                     if (1 == 1) { // +2 (nesting = 1)
                         printf(\"test\");
                     } else if (1 == 1) { // +1
                         if (1 == 1) { // +3 (nesting = 2)
                             printf(\"test\");
                         }
                     } else { // +1
                         if (1 == 1) { // +3 (nesting = 2)
                             printf(\"test\");
                         }
                     }
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 11.0,
                      "average": 11.0,
                      "min": 0.0,
                      "max": 11.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_1_level_nesting() {
        check_metrics::<MozjsParser>(
            "function f() {
                 if (1 == 1) { // +1
                     if (1 == 1) { // +2 (nesting = 1)
                         window.print(\"test\");
                     } else if (1 == 1) { // +1
                         if (1 == 1) { // +3 (nesting = 2)
                             window.print(\"test\");
                         }
                     } else { // +1
                         if (1 == 1) { // +3 (nesting = 2)
                             window.print(\"test\");
                         }
                     }
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 11.0,
                      "average": 11.0,
                      "min": 0.0,
                      "max": 11.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_nesting() {
        check_metrics::<JavascriptParser>(
            "function f() {
                 if (a) { // +1
                     for (let i = 0; i < 10; i++) { // +2 (nesting = 1)
                         while (b) { // +3 (nesting = 2)
                             console.log(\"test\");
                         }
                     }
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_2_level_nesting() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                if a:  # +1
                    for i in range(b):  # +2
                        if b:  # +3
                            return 1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_2_level_nesting() {
        check_metrics::<RustParser>(
            "fn f() {
                 if true { // +1
                     for i in 0..4 { // +2 (nesting = 1)
                         match true { // +3 (nesting = 2)
                             true => println!(\"test\"),
                             false => println!(\"test\"),
                         }
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_try_construct() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                try:
                    for foo in bar:  # +1
                        return a
                except Exception:  # +1
                    if a < 0:  # +2
                        return a",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_flat_try_except() {
        // Regression for #242: flat try/except at function top level
        // must still score +1 for the except clause (no enclosing
        // control-flow nesting). Before the fix this happened to be
        // correct because `stats.nesting` was zero; after the fix the
        // value is the same — `increase_nesting` records nesting=0 and
        // bumps structural by 0+1.
        check_metrics::<PythonParser>(
            "def f():
                try:
                    pass
                except Exception:  # +1
                    pass",
            "foo.py",
            |metric| {
                // expected: only the except clause contributes (+1).
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 1);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_except_inside_if() {
        // Regression for #242: try/except nested inside an `if` must
        // apply a nesting penalty to the except clause. Before the
        // fix, the except contributed +1 because `stats.nesting` was
        // stale (0 from the previous `increase_nesting` call on the
        // if). After the fix the except sees nesting=1 and contributes
        // +2.
        check_metrics::<PythonParser>(
            "def f(x):
                if x:  # +1
                    try:
                        pass
                    except Exception:  # +2 (nesting = 1)
                        pass",
            "foo.py",
            |metric| {
                // expected: if (+1) + except inside if (+2) = 3
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_except_inside_for() {
        // Regression for #242: try/except nested inside a `for` must
        // apply the for's nesting penalty to the except clause.
        check_metrics::<PythonParser>(
            "def f(xs):
                for x in xs:  # +1
                    try:
                        pass
                    except Exception:  # +2 (nesting = 1)
                        pass",
            "foo.py",
            |metric| {
                // expected: for (+1) + except inside for (+2) = 3
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_multi_except_inside_if() {
        // Regression for #242: every clause in a multi-except chain
        // nested inside an `if` must reflect the nesting penalty.
        // Before the fix, all three except clauses contributed +1;
        // after the fix each contributes +2 (nesting = 1 from the
        // enclosing if).
        check_metrics::<PythonParser>(
            "def f(x):
                if x:  # +1
                    try:
                        pass
                    except ValueError:    # +2
                        pass
                    except TypeError:     # +2
                        pass
                    except Exception:     # +2
                        pass",
            "foo.py",
            |metric| {
                // expected: if (+1) + 3 * except inside if (+2 each) = 7
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 7);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 7.0,
                      "average": 7.0,
                      "min": 0.0,
                      "max": 7.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_try_construct() {
        check_metrics::<MozjsParser>(
            "function asyncOnChannelRedirect(oldChannel, newChannel, flags, callback) {
                 for (const collector of this.collectors) {
                     try {
                         collector._onChannelRedirect(oldChannel, newChannel, flags);
                     } catch (ex) {
                         console.error(
                             \"StackTraceCollector.onChannelRedirect threw an exception\",
                              ex
                         );
                     }
                 }
                 callback.onRedirectVerifyCallback(Cr.NS_OK);
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_try_construct() {
        check_metrics::<JavascriptParser>(
            "function f() {
                 for (let i = 0; i < 10; i++) { // +1
                     try {
                         doSomething(i);
                     } catch (ex) { // +2 (nesting = 1)
                         if (ex instanceof TypeError) { // +3 (nesting = 2)
                             console.error(\"type error\");
                         }
                     } finally {
                         cleanup();
                     }
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    // The tree-sitter-javascript / -typescript grammars fold both
    // `for...in` and `for...of` into the same `for_in_statement` node
    // (only the keyword token differs). The four regression tests below
    // lock that in across every JS-family parser, so any future grammar
    // bump that splits `for...of` into its own node kind would surface
    // here rather than silently scoring `for...of` loops as 0 cognitive.

    #[test]
    fn javascript_for_of_loop() {
        check_metrics::<JavascriptParser>(
            "function f(xs) {
                 let s = 0;
                 for (const x of xs) { // +1
                     s += x;
                 }
                 return s;
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_for_of_loop() {
        check_metrics::<MozjsParser>(
            "function f(xs) {
                 let s = 0;
                 for (const x of xs) { // +1
                     s += x;
                 }
                 return s;
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_for_of_loop() {
        check_metrics::<TypescriptParser>(
            "function f(xs: number[]): number {
                 let s = 0;
                 for (const x of xs) { // +1
                     s += x;
                 }
                 return s;
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn tsx_for_of_loop() {
        check_metrics::<TsxParser>(
            "function f(xs: number[]): number {
                 let s = 0;
                 for (const x of xs) { // +1
                     s += x;
                 }
                 return s;
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_break_continue() {
        // Only labeled break and continue statements are considered
        check_metrics::<RustParser>(
            "fn f() {
                 'tens: for ten in 0..3 { // +1
                     '_units: for unit in 0..=9 { // +2 (nesting = 1)
                         if unit % 2 == 0 { // +3 (nesting = 2)
                             continue;
                         } else if unit == 5 { // +1
                             continue 'tens; // +1
                         } else if unit == 6 { // +1
                             break;
                         } else { // +1
                             break 'tens; // +1
                         }
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 11.0,
                      "average": 11.0,
                      "min": 0.0,
                      "max": 11.0
                    }"###
                );
            },
        );
    }

    // Regression for #389: Rust's `loop {}` has a dedicated grammar node
    // (LoopExpression) distinct from WhileExpression. The cognitive nesting
    // arm previously matched only For/While/Match, so `loop {}` silently
    // contributed neither a structural +1 nor a nesting bump.
    #[test]
    fn rust_loop_single() {
        check_metrics::<RustParser>(
            "fn f() {
                 loop { // +1
                     if true { // +2 (nesting = 1)
                         break;
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                // expected: loop=+1, nested if=+2 (1 + nesting depth 1) = 3
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 3);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    // Regression for #389: nested `loop` blocks must accrue nesting just
    // like nested `while`/`for` would.
    #[test]
    fn rust_loop_nested() {
        check_metrics::<RustParser>(
            "fn f() {
                 loop { // +1
                     loop { // +2 (nesting = 1)
                         if true { // +3 (nesting = 2)
                             break;
                         }
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                // expected: outer loop=+1, inner loop=+2, inner if=+3 = 6
                assert_eq!(metric.cognitive.cognitive_sum() as u32, 6);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_goto() {
        check_metrics::<CppParser>(
            "void f() {
             OUT: for (int i = 1; i <= max; ++i) { // +1
                      for (int j = 2; j < i; ++j) { // +2 (nesting = 1)
                          if (i % j == 0) { // +3 (nesting = 2)
                              goto OUT; // +1
                          }
                      }
                  }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 7.0,
                      "average": 7.0,
                      "min": 0.0,
                      "max": 7.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_switch() {
        check_metrics::<CppParser>(
            "void f() {
                 switch (1) { // +1
                     case 1:
                         printf(\"one\");
                         break;
                     case 2:
                         printf(\"two\");
                         break;
                     case 3:
                         printf(\"three\");
                         break;
                     default:
                         printf(\"all\");
                         break;
                 }
             }",
            "foo.c",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_ternary() {
        // Sonar's rule scores the C++ ternary `?:` as +1 (and +nesting), matching
        // the JS/Java/Python/Rust families. `CppCode::compute` now matches on
        // `ConditionalExpression`, so the operator participates in nesting like
        // any other conditional construct.
        check_metrics::<CppParser>(
            "int f(int a) {
                 if (a) { // +1
                     return a > 0 ? 1 : -1; // +2 (1 + nesting 1)
                 }
                 return a > 0 ? 0 : -1; // +1
             }",
            "foo.c",
            // expected: 1 (if) + 2 (nested ternary, nesting=1) + 1 (top-level
            // ternary) = 4. max is 4 for the only function.
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                assert_eq!(metric.cognitive.cognitive_max(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_try_catch_single() {
        check_metrics::<CppParser>(
            "void f() {
                 try {
                     g();
                 } catch (const std::exception& e) { // +1
                     h();
                 }
             }",
            "foo.cpp",
            |metric| {
                // Single catch clause +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_try_multiple_catches() {
        check_metrics::<CppParser>(
            "void f() {
                 try {
                     g();
                 } catch (const std::runtime_error& e) { // +1
                     h();
                 } catch (const std::logic_error& e) { // +1
                     i();
                 } catch (...) { // +1
                     j();
                 }
             }",
            "foo.cpp",
            |metric| {
                // Three catch clauses, each +1 at nesting 0 → 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_try_catch_in_loop() {
        check_metrics::<CppParser>(
            "void f() {
                 for (int i = 0; i < 10; ++i) { // +1
                     try {
                         g();
                     } catch (const std::exception& e) { // +2 (nesting = 1)
                         h();
                     }
                 }
             }",
            "foo.cpp",
            |metric| {
                // for +1, catch +2 (nesting = 1) → 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_range_based_for() {
        check_metrics::<CppParser>(
            "int sum(const std::vector<int>& v) {
                 int s = 0;
                 for (int x : v) { // +1
                     s += x;
                 }
                 return s;
             }",
            "foo.cpp",
            |metric| {
                // C++11 range-based `for (auto x : v)` parses as
                // `for_range_loop`; it is a control-flow construct and
                // counts the same as a classic `for_statement` → +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_nested_range_based_for() {
        check_metrics::<CppParser>(
            "void f(const std::vector<std::vector<int>>& vv) {
                 for (const auto& row : vv) { // +1
                     for (int x : row) { // +2 (nesting = 1)
                         g(x);
                     }
                 }
             }",
            "foo.cpp",
            |metric| {
                // Nested range-fors compound by nesting, matching the
                // behaviour of nested classic `for` loops: 1 + 2 = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_nested_for() {
        check_metrics::<CppParser>(
            "void f(int n, int m) {
                 for (int i = 0; i < n; ++i) { // +1
                     for (int j = 0; j < m; ++j) { // +2 (nesting = 1)
                         for (int k = 0; k < 4; ++k) { // +3 (nesting = 2)
                             g(i, j, k);
                         }
                     }
                 }
             }",
            "foo.c",
            |metric| {
                // Three nested `for` loops → 1 + 2 + 3 = 6.
                assert_eq!(metric.cognitive.cognitive_sum(), 6.0);
                assert_eq!(metric.cognitive.cognitive_max(), 6.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_nested_while() {
        check_metrics::<CppParser>(
            "void f(int n) {
                 while (n > 0) { // +1
                     while (n % 2 == 0) { // +2 (nesting = 1)
                         n /= 2;
                     }
                     n -= 1;
                 }
             }",
            "foo.c",
            |metric| {
                // Two nested `while` loops → 1 + 2 = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_recursion() {
        // Sonar's rule scores each recursive call to the enclosing function
        // as +1, but the file-level comment in `cognitive.rs` documents that
        // recursion is not tracked for C/C++ because the call graph is only
        // resolvable at run time. The body of `fact` therefore costs only
        // the explicit `if`.
        check_metrics::<CppParser>(
            "int fact(int n) {
                 if (n <= 1) { // +1
                     return 1;
                 }
                 return n * fact(n - 1); // recursion: currently not counted
             }",
            "foo.c",
            |metric| {
                // Only the `if` contributes; recursion is a documented gap.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_goto_sibling_jump() {
        check_metrics::<CppParser>(
            "void f(int n) {
                 if (n < 0) { // +1
                     goto err; // +1
                 }
                 if (n > 100) { // +1
                     goto err; // +1
                 }
                 return;
             err:
                 abort();
             }",
            "foo.c",
            |metric| {
                // Two `if` (+1 each) and two `goto` (+1 each) at nesting 0
                // (the `goto` cost is flat, not multiplied by nesting) → 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                assert_eq!(metric.cognitive.cognitive_max(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_lambda_inside_function() {
        // Per `increase_nesting`, entering a lambda bumps the effective nesting
        // by one — so an `if` directly inside a top-level lambda is +2 charged
        // to the enclosing function (Cpp lambdas are not split into a separate
        // FuncSpace by `getter.rs`, so the `if` is not double-counted).
        // The lambda *is* counted as a closure by NoM, so the cognitive
        // average is sum / (1 function + 1 closure) = 2 / 2 = 1.0.
        check_metrics::<CppParser>(
            "int f(const std::vector<int>& v) {
                 auto pred = [](int x) {
                     if (x > 0) { // +2 (lambda nesting = 1)
                         return true;
                     }
                     return false;
                 };
                 return std::count_if(v.begin(), v.end(), pred);
             }",
            "foo.cpp",
            |metric| {
                // Single `if` inside lambda at lambda-nesting 1 → +2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_switch_fall_through() {
        // A `case` without `break` (fall-through) does not add cognitive cost
        // beyond the enclosing `switch` itself: only `switch` is in the match
        // arm. Same accounting as `c_switch` above — switch +1 only.
        check_metrics::<CppParser>(
            "void f(int n) {
                 switch (n) { // +1
                     case 1:
                     case 2:
                         g();
                         // fall-through
                     case 3:
                         h();
                         break;
                     default:
                         i();
                         break;
                 }
             }",
            "foo.c",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_switch_in_loop() {
        check_metrics::<CppParser>(
            "void f(int n) {
                 for (int i = 0; i < n; ++i) { // +1
                     switch (i % 3) { // +2 (nesting = 1)
                         case 0:
                             a();
                             break;
                         case 1:
                             b();
                             break;
                         default:
                             c();
                             break;
                     }
                 }
             }",
            "foo.c",
            |metric| {
                // for +1, switch +2 (nesting = 1) → 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_macro_expanded_control_flow() {
        // Per the file-level comment in `cognitive.rs`, macro expansion is not
        // tracked for C/C++ — macros are treated as opaque tokens. This is the
        // defensive case: a control-flow-bearing macro contributes nothing on
        // its own; only the explicit `if` in the function body is counted.
        check_metrics::<CppParser>(
            "#define CHECK(x) do { if (!(x)) return; } while (0)
             void f(int a, int b) {
                 CHECK(a);              // expansion is opaque: 0
                 if (b < 0) {           // +1
                     return;
                 }
             }",
            "foo.c",
            |metric| {
                // Only the explicit `if` contributes.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_switch() {
        check_metrics::<MozjsParser>(
            "function f() {
                 switch (1) { // +1
                     case 1:
                         window.print(\"one\");
                         break;
                     case 2:
                         window.print(\"two\");
                         break;
                     case 3:
                         window.print(\"three\");
                         break;
                     default:
                         window.print(\"all\");
                         break;
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_switch() {
        check_metrics::<JavascriptParser>(
            "function f() {
                 switch (x) { // +1
                     case 1:
                         console.log(\"one\");
                         break;
                     case 2:
                         console.log(\"two\");
                         break;
                     default:
                         console.log(\"other\");
                         break;
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_ternary_operator() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 if a % 2:  # +1
                     return 'c' if a else 'd'  # +2
                 return 'a' if a else 'b'  # +1",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_nested_functions_lambdas() {
        check_metrics::<PythonParser>(
            "def f(a, b):
                 def foo(a):
                     if a:  # +2 (+1 nesting)
                         return 1
                 # +3 (+1 for boolean sequence +2 for lambda nesting)
                 bar = lambda a: lambda b: b or True or True
                 return bar(foo(a))(a)",
            "foo.py",
            |metric| {
                // 2 functions + 2 lambdas = 4
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 1.25,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_real_function() {
        check_metrics::<PythonParser>(
            "def process_raw_constant(constant, min_word_length):
                 processed_words = []
                 raw_camelcase_words = []
                 for raw_word in re.findall(r'[a-z]+', constant):  # +1
                     word = raw_word.strip()
                         if (  # +2 (+1 if and +1 nesting)
                             len(word) >= min_word_length
                             and not (word.startswith('-') or word.endswith('-')) # +2 operators
                         ):
                             if is_camel_case_word(word):  # +3 (+1 if and +2 nesting)
                                 raw_camelcase_words.append(word)
                             else: # +1 else
                                 processed_words.append(word.lower())
                 return processed_words, raw_camelcase_words",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 9.0,
                      "average": 9.0,
                      "min": 0.0,
                      "max": 9.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_if_let_else_if_else() {
        check_metrics::<RustParser>(
            "pub fn create_usage_no_title(p: &Parser, used: &[&str]) -> String {
                 debugln!(\"usage::create_usage_no_title;\");
                 if let Some(u) = p.meta.usage_str { // +1
                     String::from(&*u)
                 } else if used.is_empty() { // +1
                     create_help_usage(p, true)
                 } else { // +1
                     create_smart_usage(p, used)
                }
            }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_if_else_if_else() {
        check_metrics::<TypescriptParser>(
            "function foo() {
                 if (this._closed) return Promise.resolve(); // +1
                 if (this._tempDirectory) { // +1
                     this.kill();
                 } else if (this.connection) { // +1
                     this.kill();
                 } else { // +1
                     throw new Error(`Error`);
                }
                helper.removeEventListeners(this._listeners);
                return this._processClosing;
            }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_no_cognitive() {
        check_metrics::<JavaParser>("int a = 42;", "foo.java", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn java_single_branch_function() {
        check_metrics::<JavaParser>(
            "class X {
                public static void print(boolean a){  
                if(a){ // +1
                  System.out.println(\"test1\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                {
                  "sum": 1.0,
                  "average": 1.0,
                  "min": 0.0,
                  "max": 1.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn java_multiple_branch_function() {
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b){  
                if(a){ // +1
                  System.out.println(\"test1\");
                }
                if(b){ // +1
                  System.out.println(\"test2\");
                }
                else { // +1
                  System.out.println(\"test3\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn java_compound_conditions() {
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b, boolean c, boolean d){  
                if(a && b){ // +2 (+1 &&)
                  System.out.println(\"test1\");
                }
                if(c && d){ // +2 (+1 &&)
                  System.out.println(\"test2\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_switch_statement() {
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b, boolean c, boolean d){
                switch(expr){ //+1
                  case 1:
                    System.out.println(\"test1\");
                    break;
                  case 2:
                    System.out.println(\"test2\");
                    break;
                  default:
                    System.out.println(\"test\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_switch_expression() {
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b, boolean c, boolean d){
                switch(expr){ // +1
                  case 1 -> System.out.println(\"test1\");
                  case 2 -> System.out.println(\"test2\");
                  default -> System.out.println(\"test\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_not_booleans() {
        // `!` does not break boolean sequences (issue #392): pre-order
        // visits the outer `&&` BinaryExpression first; the inner `&&`
        // lies within that span and is a continuation, not a new
        // sequence.
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b, boolean c, boolean d){
                if (a && !(b && c)) { // +2 (+1 if, +1 outer &&; inner && continues)
                  printf(\"test\");
                }
              }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_enhanced_for_statement() {
        check_metrics::<JavaParser>(
            "class X {
              public static int sum(int[] xs) {
                int s = 0;
                for (int x : xs) { // +1
                  s += x;
                }
                return s;
              }
            }",
            "foo.java",
            |metric| {
                // Java's enhanced-for `for (T x : c)` parses as
                // `enhanced_for_statement`; it is a control-flow construct
                // and counts the same as a classic `for_statement` → +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_nested_enhanced_for_statement() {
        check_metrics::<JavaParser>(
            "class X {
              public static void f(int[][] xss) {
                for (int[] xs : xss) { // +1
                  for (int x : xs) { // +2 (nesting = 1)
                    g(x);
                  }
                }
              }
            }",
            "foo.java",
            |metric| {
                // Nested enhanced-fors compound by nesting, matching the
                // behaviour of nested classic `for` loops: 1 + 2 = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_ternary() {
        // Java's ternary `?:` (grammar `ternary_expression`) is a
        // conditional construct: +1 base + nesting, matching the
        // SonarSource Cognitive Complexity §2 rule and the C++/JS
        // siblings.
        check_metrics::<JavaParser>(
            "class X {
              public static boolean check(int a) {
                  return a > 0 ? true : false; // +1
              }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_nested_ternary() {
        // Nested ternaries inside an `if` block compound by nesting,
        // matching the C++ regression test for issue #172.
        // expected: if (+1, nesting=0) + outer ternary (+1+1=+2,
        // nesting=1) + inner ternary (+1+2=+3, nesting=2) = 6.
        check_metrics::<JavaParser>(
            "class X {
              public static String classify(int a, int b) {
                  if (a > 0) { // +1
                      return b > 0 ? (b > 10 ? \"big\" : \"small\") : \"neg\"; // +2, +3
                  }
                  return \"zero\";
              }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 6.0);
                assert_eq!(metric.cognitive.cognitive_max(), 6.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_labeled_break_continue() {
        // Per SonarSource Cognitive Complexity §B2 (issue #225), labeled
        // `break LABEL` and `continue LABEL` each add +1 because they break
        // structured control flow. Mirrors `go_labeled_break_continue` and
        // `rust_break_continue_labeled`.
        // expected: outer for (+1, nesting=0) + inner for (+2, nesting=1)
        // + if (+3, nesting=2) + continue outer (+1)
        // + if (+3, nesting=2) + break outer (+1) = 11.
        check_metrics::<JavaParser>(
            "class X {
                void scan(int[][] m) {
                    outer:
                    for (int i = 0; i < m.length; i++) {        // +1
                        for (int j = 0; j < m[i].length; j++) {  // +2
                            if (m[i][j] < 0) continue outer;     // +3, +1
                            if (m[i][j] > 100) break outer;      // +3, +1
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 11.0);
                assert_eq!(metric.cognitive.cognitive_max(), 11.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 11.0,
                      "average": 11.0,
                      "min": 0.0,
                      "max": 11.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_unlabeled_break_continue_not_counted() {
        // Negative test for issue #225: plain `break;` / `continue;` are
        // *not* unstructured jumps under SonarSource Cognitive Complexity
        // §B2 and must add 0. Only the surrounding `for` + `if` contribute.
        // expected: for (+1) + if (+2) + if (+2) = 5.
        check_metrics::<JavaParser>(
            "class X {
                void scan(int[] m) {
                    for (int i = 0; i < m.length; i++) {  // +1
                        if (m[i] < 0) continue;            // +2, +0
                        if (m[i] > 100) break;             // +2, +0
                    }
                }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 5.0);
                assert_eq!(metric.cognitive.cognitive_max(), 5.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 5.0,
                      "min": 0.0,
                      "max": 5.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_no_cognitive() {
        check_metrics::<CsharpParser>("int a = 42;", "foo.cs", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn csharp_single_branch_function() {
        check_metrics::<CsharpParser>(
            "class X {
                public static void Print(bool a) {
                    if (a) {
                        System.Console.WriteLine(\"test1\");
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // Single `if` at nesting 0 → +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_multiple_branch_function() {
        check_metrics::<CsharpParser>(
            "class X {
                public static void Print(bool a, bool b) {
                    if (a) {
                        System.Console.WriteLine(\"test1\");
                    }
                    if (b) {
                        System.Console.WriteLine(\"test2\");
                    } else {
                        System.Console.WriteLine(\"test3\");
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // First `if` +1, second `if` +1, `else` +1 → 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_compound_conditions() {
        check_metrics::<CsharpParser>(
            "class X {
                public static void Print(bool a, bool b, bool c, bool d) {
                    if (a && b) {
                        System.Console.WriteLine(\"test1\");
                    }
                    if (c && d) {
                        System.Console.WriteLine(\"test2\");
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // Two ifs (+1 each) + two `&&` (+1 each, fresh chain per if) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                assert_eq!(metric.cognitive.cognitive_max(), 4.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_switch_statement() {
        check_metrics::<CsharpParser>(
            "class X {
                public static void Print(int expr) {
                    switch (expr) {
                        case 1:
                            System.Console.WriteLine(\"test1\");
                            break;
                        case 2:
                            System.Console.WriteLine(\"test2\");
                            break;
                        default:
                            System.Console.WriteLine(\"test\");
                            break;
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // Single `switch` +1; cases / default do not increment.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_switch_expression() {
        check_metrics::<CsharpParser>(
            "class X {
                public static string Name(int expr) =>
                    expr switch {
                        1 => \"one\",
                        2 => \"two\",
                        _ => \"other\"
                    };
            }",
            "foo.cs",
            |metric| {
                // `switch` expression +1; arms do not increment.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_not_booleans() {
        // `!` does not break boolean sequences (issue #392): pre-order
        // visits the outer `&&` BinaryExpression first, so the inner
        // `&&` lies within its span and is a continuation.
        check_metrics::<CsharpParser>(
            "class X {
                public static void Print(bool a, bool b, bool c) {
                    if (a && !(b && c)) {
                        System.Console.WriteLine(\"test\");
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // `if` +1, outer `&&` +1, inner `&&` continues outer span → 2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_ternary() {
        // C#'s ternary `?:` (grammar `conditional_expression`) is a
        // conditional construct: +1 base + nesting. Regression test for
        // issue #224.
        check_metrics::<CsharpParser>(
            "class X {
                public static bool Check(int a) {
                    return a > 0 ? true : false; // +1
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_nested_ternary() {
        // Nested ternaries inside an `if` compound by nesting (mirrors
        // the C++ regression test for #172).
        // expected: if (+1) + outer ternary (+2, nesting=1) + inner
        // ternary (+3, nesting=2) = 6.
        check_metrics::<CsharpParser>(
            "class X {
                public static string Classify(int a, int b) {
                    if (a > 0) { // +1
                        return b > 0 ? (b > 10 ? \"big\" : \"small\") : \"neg\"; // +2, +3
                    }
                    return \"zero\";
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 6.0);
                assert_eq!(metric.cognitive.cognitive_max(), 6.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_goto_statement() {
        // Per SonarSource Cognitive Complexity §B2 (issue #225), any `goto`
        // is an unstructured jump and adds +1. Mirrors C++'s `GotoStatement`
        // and Go's `GotoStatement` handling.
        // expected: if (+1, nesting=0) + goto neg (+1) = 2.
        check_metrics::<CsharpParser>(
            "class X {
                int Classify(int x) {
                    if (x < 0) goto neg;  // +1, +1
                    return x;
                    neg:
                    return -x;
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_goto_case_and_default() {
        // `goto case` and `goto default` inside a `switch` are also
        // unstructured jumps (+1 each) per SonarSource §B2.
        // expected: switch (+1, nesting=0) + goto case 2 (+1)
        // + goto default (+1) = 3.
        check_metrics::<CsharpParser>(
            "class X {
                int Walk(int x) {
                    switch (x) {  // +1
                        case 1: goto case 2;     // +1
                        case 2: return 2;
                        case 3: goto default;    // +1
                        default: return 0;
                    }
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_unlabeled_break_not_counted() {
        // Negative test for issue #225: C#'s grammar does not allow
        // labeled `break`/`continue` (those are syntactically rejected),
        // and plain `break;` / `continue;` are not unstructured jumps under
        // SonarSource §B2 — they must add 0. Only the `for` + `if`
        // contribute.
        // expected: for (+1) + if (+2) = 3.
        check_metrics::<CsharpParser>(
            "class X {
                void Scan(int[] m) {
                    for (int i = 0; i < m.Length; i++) {  // +1
                        if (m[i] < 0) break;               // +2, +0
                    }
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn perl_no_cognitive() {
        check_metrics::<PerlParser>("my $a = 42;", "foo.pl", |metric| {
            insta::assert_json_snapshot!(metric.cognitive, @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#);
        });
    }

    #[test]
    fn perl_simple_function() {
        check_metrics::<PerlParser>(
            "sub f {
                return 1;
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 0.0,
                  "average": 0.0,
                  "min": 0.0,
                  "max": 0.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_sequence_same_booleans() {
        check_metrics::<PerlParser>(
            "sub f {
                if ($a && $b && $c) { # +1 if, +1 first &&-chain
                    print 'x';
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_sequence_different_booleans() {
        check_metrics::<PerlParser>(
            "sub f {
                if ($a && $b || $c) { # +1 if, +1 &&, +1 ||
                    print 'x';
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_compound_short_circuit_assignment_249() {
        // Regression for issue #249: `&&=`, `||=`, `//=` are compound
        // short-circuit assignments (e.g. `$x //= 1` ≡ `$x = $x // 1`)
        // and each carries one boolean-sequence decision. The grammar
        // exposes the operator token inside `binary_expression`, so the
        // existing arm picks them up once `compute_perl_booleans`
        // recognises the three `*EQ` tokens.
        check_metrics::<PerlParser>(
            "sub f {
                 my ($x, $y, $z) = @_;
                 $x ||= 1; # +1 (||=)
                 $y &&= 2; # +1 (&&=)
                 $z //= 3; # +1 (//=)
                 return $x;
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn perl_not_booleans() {
        // `!` does not break boolean sequences (issue #392): pre-order
        // visits the outer `&&` BinaryExpression first, so the inner
        // `&&` lies within its span and is a continuation.
        check_metrics::<PerlParser>(
            "sub f {
                if ($a && !($b && $c)) { # +1 if, +1 outer &&; inner && continues
                    print 'x';
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_1_level_nesting() {
        check_metrics::<PerlParser>(
            "sub f {
                for my $i (1..3) { # +1 for
                    if ($i % 2) { # +2 if (nested 1)
                        print $i;
                    }
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_2_level_nesting() {
        check_metrics::<PerlParser>(
            "sub f {
                for my $i (1..3) { # +1 for
                    while ($n > 0) { # +2 while (nested 1)
                        if ($n % 2) { # +3 if (nested 2)
                            $n--;
                        }
                    }
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 6.0,
                  "average": 6.0,
                  "min": 0.0,
                  "max": 6.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_break_continue() {
        // Perl's `last`/`next` are loop-control statements; per Sonar's
        // cognitive rule, they do not add complexity in their bare form
        // (the surrounding loop already contributes +1).
        check_metrics::<PerlParser>(
            "sub f {
                while (1) { # +1 while (nesting becomes 1)
                    last if $done; # +2 postfix-if at nesting=1
                    next; # +0 bare loop control
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_if_elsif_else() {
        check_metrics::<PerlParser>(
            "sub f {
                if ($x) { # +1 if
                    print 'a';
                } elsif ($y) { # +1 elsif
                    print 'b';
                } else { # +1 else
                    print 'c';
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                 "#);
            },
        );
    }

    #[test]
    fn perl_function_definition_without_sub_depth() {
        // Regression: FunctionDefinitionWithoutSub must be a stop in
        // increment_function_depth so that a `sub` nested inside a `method`
        // block gets depth=1, making its structural elements cost +2 instead
        // of +1.  `method name { }` (Method::Signatures style) is what
        // tree-sitter-perl parses as function_definition_without_sub.
        check_metrics::<PerlParser>(
            "method outer {
                sub inner {
                    if (1) { } # +2 (depth=1)
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2.0,
                  "average": 1.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_goto_single_increment() {
        // Regression (#450): `goto LABEL;` parses as `goto_expression`
        // wrapping the anonymous `goto` keyword token. The walker visits
        // both, so matching `Goto | GotoExpression` counted the jump twice
        // (cognitive 2). Matching only `GotoExpression` scores the correct
        // +1.
        check_metrics::<PerlParser>("sub f { goto LABEL; LABEL: return; }", "foo.pl", |metric| {
            // expected: one `goto` jump (§B2) = +1
            assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
            insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 1.0,
                  "average": 1.0,
                  "min": 0.0,
                  "max": 1.0
                }
                "#);
        });
    }

    #[test]
    fn perl_labeled_loop_control() {
        // Regression (#450): the jump target of `last/next/redo LABEL` is
        // carried as an `Identifier` child of `loop_control_statement`
        // (`Label` is the loop-*definition* node `OUTER:`). Gating on
        // `Label` was a dead arm — labeled jumps scored +0. Each labeled
        // form is now +1 (§B2). The bare forms below stay +0.
        check_metrics::<PerlParser>(
            "OUTER: for my $i (@a) { # +1 for
                 last OUTER;  # +1 labeled
                 next OUTER;  # +1 labeled
                 redo OUTER;  # +1 labeled
             }",
            "foo.pl",
            |metric| {
                // expected: +1 for-loop, +1 each labeled last/next/redo = 4
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 4.0,
                  "average": 4.0,
                  "min": 4.0,
                  "max": 4.0
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_bare_loop_control_zero() {
        // Bare `last;` / `next;` / `redo;` have no `Identifier` jump-target
        // child and must stay +0 — only the surrounding loop counts (§B2).
        check_metrics::<PerlParser>(
            "for my $i (@a) { # +1 for
                 last;  # +0
                 next;  # +0
                 redo;  # +0
             }",
            "foo.pl",
            |metric| {
                // expected: only the +1 for-loop; bare jumps add nothing
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 1.0,
                  "average": 1.0,
                  "min": 1.0,
                  "max": 1.0
                }
                "#);
            },
        );
    }

    #[test]
    fn tsx_nested_if_for_with_booleans() {
        check_metrics::<TsxParser>(
            "function process(items: number[]) {
                 if (items.length > 0) { // +1
                     for (let i = 0; i < items.length; i++) { // +2 (nesting=1)
                         if (items[i] > 0 && items[i] < 100) { // +3 (nesting=2) +1 (&&)
                             console.log(items[i]);
                         }
                     }
                 }
             }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 7.0,
                      "average": 7.0,
                      "min": 0.0,
                      "max": 7.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_nested_if_with_boolean_sequence() {
        check_metrics::<TypescriptParser>(
            "function validate(input: string, strict: boolean): boolean {
                 if (input.length > 0) { // +1
                     if (strict && input.trim() === input) { // +2 (nesting=1) +1 (&&)
                         return true;
                     }
                 }
                 return false;
             }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_try_catch_with_nesting() {
        check_metrics::<TypescriptParser>(
            "function fetchData(url: string): string {
                 try {
                     if (url.length === 0) { // +1
                         throw new Error('empty url');
                     }
                     return url;
                 } catch (e) { // +1
                     if (e instanceof Error) { // +2 (nesting=1)
                         return e.message;
                     }
                     return 'unknown error';
                 }
             }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn kotlin_cognitive_control_flow() {
        check_metrics::<KotlinParser>(
            "fun process(x: Int, y: Int): String {
                if (x > 0) {                // +1
                    for (i in 1..x) {       // +2 (nesting=1)
                        if (i % 2 == 0) {   // +3 (nesting=2)
                            println(i)
                        }
                    }
                } else if (x < 0) {        // +1 (else-if: flat +1 for else, if not counted as else-if)
                    when (y) {              // +2 (nesting=1)
                        1 -> println(\"one\")
                        2 -> println(\"two\")
                        else -> println(\"other\")
                    }
                } else {                    // +1
                    while (y > 0) {         // +2
                        println(y)
                    }
                }
                return if (x > y) \"big\" else \"small\"
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 14.0,
                      "average": 14.0,
                      "min": 0.0,
                      "max": 14.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn kotlin_no_cognitive() {
        check_metrics::<KotlinParser>("fun main() { val x = 42 }", "foo.kt", |metric| {
            insta::assert_json_snapshot!(metric.cognitive, @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#);
        });
    }

    #[test]
    fn kotlin_simple_if_with_boolean() {
        check_metrics::<KotlinParser>(
            "fun test(a: Boolean, b: Boolean) { if (a && b) { val x = 1 } }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_nesting() {
        check_metrics::<KotlinParser>(
            "fun test(items: List<Int>) {
                if (items.isNotEmpty()) {
                    for (i in items) {
                        if (i > 0) {
                            println(i)
                        }
                    }
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 6.0,
                  "average": 6.0,
                  "min": 0.0,
                  "max": 6.0
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_when_expression() {
        check_metrics::<KotlinParser>(
            "fun test(x: Int) { when { x > 10 -> val a = 1; x > 5 -> val b = 2; else -> val c = 3 } }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 1.0,
                  "average": 1.0,
                  "min": 0.0,
                  "max": 1.0
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_when_else_no_increment() {
        check_metrics::<KotlinParser>(
            "fun test(x: Int) {
                when (x) {
                    1 -> println(\"one\")
                    2 -> println(\"two\")
                    else -> println(\"other\")
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 1.0,
                  "average": 1.0,
                  "min": 0.0,
                  "max": 1.0
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_labeled_break_continue() {
        // Regression (#450): tree-sitter-kotlin-ng has no break/continue
        // jump-statement kind — `break@outer` / `continue@outer` are
        // `labeled_expression` nodes. The Kotlin impl had no arm for them,
        // so labeled jumps scored +0. Each labeled jump is now +1 (§B2);
        // the bare `break` below (a plain identifier) stays +0.
        check_metrics::<KotlinParser>(
            "fun f() {
                 outer@ for (i in 1..10) { // +1 for
                     break@outer     // +1 labeled
                     continue@outer  // +1 labeled
                     break           // +0 bare
                 }
             }",
            "foo.kt",
            |metric| {
                // expected: +1 for-loop, +1 each labeled break/continue = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 3.0,
                  "average": 3.0,
                  "min": 0.0,
                  "max": 3.0
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_else_in_if_still_increments() {
        check_metrics::<KotlinParser>(
            "fun test(x: Int) {
                if (x > 0) {
                    println(\"positive\")
                } else {
                    println(\"non-positive\")
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_else_if_chain() {
        check_metrics::<KotlinParser>(
            "fun test(x: Int) {
                if (x > 10) {
                } else if (x > 5) {
                } else if (x > 0) {
                } else {
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 4.0,
                  "average": 4.0,
                  "min": 0.0,
                  "max": 4.0
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_lambda_nesting() {
        check_metrics::<KotlinParser>(
            "fun test() { val f = { if (true) { } } }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2.0,
                  "average": 1.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "#);
            },
        );
    }

    #[test]
    fn kotlin_secondary_constructor_depth() {
        // Regression: SecondaryConstructor must be a stop in increment_function_depth so
        // that a local `fun` nested inside it gets depth=1, making its structural elements
        // cost +2 instead of +1.
        check_metrics::<KotlinParser>(
            "class Foo {
                constructor(x: Int) {
                    fun inner(): Boolean {
                        if (x > 0) { return true } // +2 (depth=1)
                        return false
                    }
                }
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 2.0,
                  "average": 1.0,
                  "min": 0.0,
                  "max": 2.0
                }
                "#);
            },
        );
    }

    #[test]
    fn go_no_cognitive() {
        check_metrics::<GoParser>("package main\nvar x = 42", "foo.go", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn go_simple_function() {
        check_metrics::<GoParser>(
            "package main
            func f(a, b bool) {
                if a && b {    // +1 (if) +1 (&&)
                    return
                }
                if a || b {    // +1 (if) +1 (||)
                    return
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn go_nesting() {
        check_metrics::<GoParser>(
            "package main
            func f(x int, items []int) {
                if x > 0 {                    // +1 (nesting 0)
                    for _, v := range items {  // +2 (nesting 1)
                        if v > 0 {             // +3 (nesting 2)
                            println(v)
                        }
                    }
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn go_switch() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) {
                switch x {         // +1 (nesting 0)
                case 1:
                    if x > 0 {     // +2 (nesting 1)
                        println(x)
                    }
                default:
                    println(x)
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn go_goto() {
        check_metrics::<GoParser>(
            "package main
            func f(n int) {
                if n > 10 {    // +1 (nesting 0)
                    goto end   // +1 (goto)
                }
            end:
                return
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn go_else_if_chain() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) {
                if x > 0 {           // +1 (nesting 0)
                    println(x)
                } else if x < 0 {    // +1 (else-if)
                    println(-x)
                } else {              // +1 (else)
                    println(0)
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn go_labeled_break_continue() {
        check_metrics::<GoParser>(
            "package main
            func f() {
            outer:
                for i := 0; i < 3; i++ {       // +1 (nesting 0)
                    for j := 0; j < 3; j++ {    // +2 (nesting 1)
                        if i == j {              // +3 (nesting 2)
                            continue outer       // +1 (labeled continue)
                        }
                    }
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 7.0,
                      "average": 7.0,
                      "min": 0.0,
                      "max": 7.0
                    }
                    "###
                );
            },
        );
    }

    #[test]
    fn go_method_declaration() {
        // Coverage: MethodDeclaration is processed as a function boundary (nesting
        // reset) identically to FunctionDeclaration.  The depth-stop fix from
        // 081f893 (adding MethodDeclaration to increment_function_depth's stop
        // list) cannot be regression-tested with valid Go because method
        // declarations cannot be nested inside other functions or methods.
        check_metrics::<GoParser>(
            "package main
            type T struct{ val int }
            func (t T) positive() bool {
                if t.val > 0 { // +1
                    return true
                }
                return false
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive, @r#"
                {
                  "sum": 1.0,
                  "average": 1.0,
                  "min": 0.0,
                  "max": 1.0
                }
                "#);
            },
        );
    }

    #[test]
    fn bash_no_cognitive() {
        check_metrics::<BashParser>("a=42", "foo.sh", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn bash_simple_if() {
        check_metrics::<BashParser>(
            "f() {
                 if [ -z \"$1\" ]; then  # +1
                     echo empty
                 fi
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_if_elif_else() {
        check_metrics::<BashParser>(
            "f() {
                 if [ \"$1\" = a ]; then     # +1
                     echo a
                 elif [ \"$1\" = b ]; then   # +1
                     echo b
                 else                         # +1
                     echo other
                 fi
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_nested_loops() {
        check_metrics::<BashParser>(
            "f() {
                 for i in 1 2 3; do            # +1
                     while [ \"$x\" -lt 10 ]; do  # +2 (nested)
                         x=$((x+1))
                     done
                 done
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_until_loop() {
        // `until` parses to `Bash::WhileStatement`; this test pins that
        // assumption so a future grammar bump that adds a dedicated
        // `UntilStatement` variant is caught.
        check_metrics::<BashParser>(
            "f() {
                 until [ -z \"$x\" ]; do  # +1
                     x=$(pop)
                 done
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_case() {
        // `case` adds +1 nesting; case arms do not contribute extra cognitive
        // cost (matching Kotlin's `WhenExpression` treatment).
        check_metrics::<BashParser>(
            "f() {
                 case \"$1\" in       # +1
                     a) echo a ;;
                     b) echo b ;;
                     *) echo other ;;
                 esac
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_boolean_sequence() {
        // First if: a chain of `&&` is one boolean increment regardless of
        // length (consecutive same-operator chain). Second if: `&& … ||` is
        // two operator transitions, so two boolean increments.
        check_metrics::<BashParser>(
            "f() {
                 if [[ -n \"$x\" ]] && [[ -n \"$y\" ]] && [[ -n \"$z\" ]]; then
                     # +1 if, +1 boolean (one && chain)
                     echo all
                 fi
                 if [[ -n \"$x\" ]] && [[ -n \"$y\" ]] || [[ -n \"$z\" ]]; then
                     # +1 if, +2 boolean (&& then ||)
                     echo mixed
                 fi
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 5.0,
                      "min": 0.0,
                      "max": 5.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn tcl_no_cognitive() {
        // No proc, no control flow → cognitive complexity is zero everywhere.
        check_metrics::<TclParser>("set x 1", "foo.tcl", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
            assert_eq!(metric.cognitive.cognitive_max(), 0.0);
            insta::assert_json_snapshot!(metric.cognitive);
        });
    }

    #[test]
    fn tcl_simple_function() {
        // proc with one if and one &&: if(+1) + &&(+1) = 2.
        check_metrics::<TclParser>(
            "proc f {a} {
    if {$a > 0 && $a < 10} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_sequence_same_booleans() {
        // Sequences of the same boolean operator count as a single increment.
        // `$a && $b && $c` → +1 (one && group), not +2.
        check_metrics::<TclParser>(
            "proc f {a b c d} {
    if {$a && $b && $c} {
        puts yes
    }
    if {$a || $b || $c || $d} {
        puts no
    }
}",
            "foo.tcl",
            |metric| {
                // Two ifs (+1 each) + two single-op chains (+1 each) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                assert_eq!(metric.cognitive.cognitive_max(), 4.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_sequence_different_booleans() {
        // Switching operator type increments again: `$a && $b || $c` → +2 (one &&, one ||).
        check_metrics::<TclParser>(
            "proc f {a b c} {
    if {$a && $b || $c} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                // if(+1) + &&(+1) + ||(+1) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_not_booleans() {
        // `!` does not contribute cognitive cost on its own (issue
        // #392). The single `&&` between the two negations contributes
        // +1, plus +1 for the surrounding `if`.
        check_metrics::<TclParser>(
            "proc f {a b} {
    if {!$a && !$b} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                // if(+1) + &&(+1) = 2; the `!` operators do not increment.
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_1_level_nesting() {
        // while(+1) then if at depth 1 (+2) = 3 for the proc.
        check_metrics::<TclParser>(
            "proc f {x} {
    while {$x > 0} {
        if {$x > 10} {
            set x [expr {$x - 1}]
        }
    }
}",
            "foo.tcl",
            |metric| {
                // while(+1) + if at depth 1 (+2) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_2_level_nesting() {
        // while(+1) + foreach at depth 1 (+2) + if at depth 2 (+3) = 6.
        check_metrics::<TclParser>(
            "proc f {x} {
    while {$x > 0} {
        foreach y {1 2 3} {
            if {$y > $x} {
                puts found
            }
        }
    }
}",
            "foo.tcl",
            |metric| {
                // while(+1) + foreach at depth 1 (+2) + if at depth 2 (+3) = 6.
                assert_eq!(metric.cognitive.cognitive_sum(), 6.0);
                assert_eq!(metric.cognitive.cognitive_max(), 6.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_catch_cognitive() {
        // `catch` is a conditional handler: +1 at nesting 0, then body at nesting 1.
        // Nested if inside catch body: +2 (depth 1).
        check_metrics::<TclParser>(
            "proc f {x} {
    catch {
        if {$x < 0} {
            error negative
        }
    } msg
}",
            "foo.tcl",
            |metric| {
                // catch(+1) + if at depth 1 (+2) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_if_elseif_else() {
        // if(+1) + elseif(+1) + else(+1) = 3; nesting does not increase for elseif/else.
        check_metrics::<TclParser>(
            "proc f {x} {
    if {$x > 10} {
        puts big
    } elseif {$x > 5} {
        puts medium
    } else {
        puts small
    }
}",
            "foo.tcl",
            |metric| {
                // if(+1) + elseif(+1) + else(+1) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_not_booleans_nested() {
        // `$a && !($b && $c)`: `!` does not break boolean sequences
        // (issue #392); inner `&&` is a continuation of the outer.
        check_metrics::<TclParser>(
            "proc f {a b c} {
    if {$a && !($b && $c)} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                // if(+1) + outer &&(+1); inner && continues outer's span → 2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_not_booleans_double_nested() {
        // `!($a || $b) && !($c || $d)`: the two `||` sub-expressions and
        // the connecting `&&` are at distinct positions with distinct
        // operator tokens, so each starts a new boolean sequence
        // regardless of the `!` wrapping (issue #392). if(+1) + &&(+1)
        // + first ||(+1) + second ||(+1) = 4.
        check_metrics::<TclParser>(
            "proc f {a b c d} {
    if {!($a || $b) && !($c || $d)} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                // if(+1) + &&(+1) + first || (+1) + second || (+1) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                assert_eq!(metric.cognitive.cognitive_max(), 4.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_nested_procedure_cognitive() {
        // Inner proc is at depth=1; its `if` adds +1+1=2 instead of +1+0=1.
        check_metrics::<TclParser>(
            "proc outer {x} {
    proc inner {y} {
        if {$y > 0} {
            puts positive
        }
    }
    inner $x
}",
            "foo.tcl",
            |metric| {
                // Aggregated: inner proc's `if` at depth 1 contributes 2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_ternary_cognitive() {
        // Ternary `? :` inside expr is a conditional expression: adds +1+depth.
        // At proc body depth 0: +1. Inside a while (depth 1): +2.
        check_metrics::<TclParser>(
            "proc f {x} {
    set y [expr {$x > 0 ? $x : -$x}]
    while {$y > 10} {
        set y [expr {$y > 5 ? $y - 1 : 0}]
    }
}",
            "foo.tcl",
            |metric| {
                // outer ternary(+1) + while(+1) + inner ternary at depth 1 (+2) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                assert_eq!(metric.cognitive.cognitive_max(), 4.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn lua_cognitive_no_cognitive() {
        // Top-level local assignment, no control flow → cognitive complexity is zero.
        check_metrics::<LuaParser>("local x = 42", "foo.lua", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r#"
            {
              "sum": 0.0,
              "average": 0.0,
              "min": 0.0,
              "max": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn lua_cognitive_simple_function() {
        // Two `if … and …` statements at function scope: each contributes
        // +1 (if) + 1 (and) = 2; total 4.
        check_metrics::<LuaParser>(
            "local function f(a, b, c, d)
    if a and b then
        return 1
    end
    if c and d then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_sequence_same_booleans() {
        // Sequences of the same boolean operator count as a single increment.
        // `a and b and c` → +1 (one and-group), `a or b or c or d` → +1.
        // Plus +1 per `if` ⇒ 4 total.
        check_metrics::<LuaParser>(
            "local function f(a, b, c, d)
    if a and b and c then
        return 1
    end
    if a or b or c or d then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_not_booleans() {
        // `not a and not b`: `not` does not contribute cognitive cost
        // on its own (issue #392); the single `and` between the two
        // negations contributes +1. if(+1) + and(+1) = 2.
        check_metrics::<LuaParser>(
            "local function f(a, b)
    if not a and not b then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_sequence_different_booleans() {
        // Switching operator type increments again: `a and b or c`
        // → if(+1) + and(+1) + or(+1) = 3.
        check_metrics::<LuaParser>(
            "local function f(a, b, c)
    if a and b or c then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_1_level_nesting() {
        // for at depth 0 (+1) + if at depth 1 (+2) = 3.
        check_metrics::<LuaParser>(
            "local function f(t)
    for i = 1, #t do
        if t[i] > 0 then
            return t[i]
        end
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_2_level_nesting() {
        // outer for (+1) + inner for at depth 1 (+2) + if at depth 2 (+3) = 6.
        check_metrics::<LuaParser>(
            "local function f(t)
    for i = 1, #t do
        for j = 1, #t do
            if t[i] > t[j] then
                return t[i]
            end
        end
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_break_continue() {
        // Lua's `break` is always unlabeled (the grammar has no labeled
        // break and no `continue`), so per SonarSource Cognitive Complexity
        // §B2 it adds +0 — issue #435. for(+1) + if at depth 1 (+2) = 3.
        check_metrics::<LuaParser>(
            "local function f(t)
    for i = 1, #t do
        if t[i] < 0 then
            break
        end
    end
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_goto_counted() {
        // `goto label` is a genuinely unstructured jump and adds +1 per
        // SonarSource §B2, even though Lua's unlabeled `break` does not
        // (issue #435). Only the `goto` contributes: +1.
        check_metrics::<LuaParser>(
            "local function f()
    ::top::
    goto top
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn lua_cognitive_elseif_nesting() {
        // Lua-specific: `elseif_statement` is a dedicated grammar node that
        // stays at the same nesting level as the enclosing `if`. Chain:
        // if(+1) + elseif(+1) + elseif(+1) + else(+1) = 4.
        check_metrics::<LuaParser>(
            "local function classify(x)
    if x > 0 then
        return 1
    elseif x < 0 then
        return -1
    elseif x == 0 then
        return 0
    else
        return 0
    end
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_switch_statement() {
        check_metrics::<TypescriptParser>(
            "function describe(x: number): string {
                 switch (x) {   // +1
                     case 1:
                         return 'one';
                     case 2:
                         return 'two';
                     default:
                         return 'other';
                 }
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn typescript_no_cognitive() {
        check_metrics::<TypescriptParser>(
            "function f(a: number, b: number): number {
                 return a + b;
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
                assert_eq!(metric.cognitive.cognitive_max(), 0.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_no_cognitive() {
        check_metrics::<TsxParser>(
            "function f(a: number, b: number): number {
                 return a + b;
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
                assert_eq!(metric.cognitive.cognitive_max(), 0.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_simple_if() {
        check_metrics::<TsxParser>(
            "function f(x: number): number {
                 if (x > 0) {  // +1
                     return x;
                 }
                 return 0;
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_boolean_sequence() {
        check_metrics::<TsxParser>(
            "function f(a: boolean, b: boolean, c: boolean): boolean {
                 return a && b && c;  // +1 (&&, sequence)
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_2_level_nesting() {
        check_metrics::<TsxParser>(
            "function f(a: number[], n: number): number {
                 for (let i = 0; i < a.length; i++) {  // +1
                     if (a[i] > n) {  // +2 (nesting=1)
                         return a[i];
                     }
                 }
                 return -1;
             }",
            "foo.tsx",
            |metric| {
                // for(+1) + if at depth 1 (+2) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_else_if_chain() {
        check_metrics::<TsxParser>(
            "function classify(x: number): string {
                 if (x < 0) {         // +1
                     return 'neg';
                 } else if (x === 0) { // +1 (else if = structural, not nesting)
                     return 'zero';
                 } else {              // +1
                     return 'pos';
                 }
             }",
            "foo.tsx",
            |metric| {
                // if(+1) + else-if(+1) + else(+1) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn js_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a *new* sequence (sibling, not nested),
        // so it should score +1, giving a total of 3 (&&, ||, &&).
        // The pre-existing bug stored only (kind_id) and treated the right && as a
        // continuation of the earlier && sequence, incorrectly yielding 2.
        check_metrics::<JavascriptParser>(
            "function f(a, b, c, d) {
                 return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn js_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested inside ||, so they form
        // one sequence and only the first should score +1. Total = 2 (||, &&).
        check_metrics::<JavascriptParser>(
            "function f(a, b, c, d) {
                 return a || (b && c && d);  // +1(||) +1(&&) = 2
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn python_sibling_bool_sequences() {
        // Python uses keyword boolean operators (`and`/`or`), routed through a
        // different `T` instantiation of `compute_booleans` than the JS `&&`/`||`
        // tests. Verifies the sibling-detection fix applies across operator kinds.
        // (a and b) or (c and d) — the right-hand `and` is a sibling, not nested.
        // Expected: and_left(+1) + or(+1) + and_right(+1) = 3.
        check_metrics::<PythonParser>(
            "def f(a, b, c, d):
                 return (a and b) or (c and d)  # +1(and) +1(or) +1(and) = 3
             ",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn python_nested_bool_same_op() {
        // a or (b and c and d) — the inner `and` operators are nested inside `or`,
        // forming one sequence. Expected: or(+1) + and(+1) = 2.
        check_metrics::<PythonParser>(
            "def f(a, b, c, d):
                 return a or (b and c and d)  # +1(or) +1(and) = 2
             ",
            "foo.py",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn perl_sibling_bool_sequences() {
        // Perl uses `compute_perl_booleans` (a separate function supporting five
        // operator kinds including `//`). Verifies the sibling-detection fix also
        // covers that code path.
        // ($a && $b) || ($c && $d) — the right-hand `&&` is a sibling.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<PerlParser>(
            "sub f {
                 my ($a, $b, $c, $d) = @_;
                 return ($a && $b) || ($c && $d);  # +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn perl_nested_bool_same_op() {
        // $a || ($b && $c && $d) — the inner `&&` operators are nested inside `||`,
        // forming one sequence. Exercises the `compute_perl_booleans` continuation
        // guard (the only path distinct from `compute_booleans`).
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<PerlParser>(
            "sub f {
                 my ($a, $b, $c, $d) = @_;
                 return $a || ($b && $c && $d);  # +1(||) +1(&&) = 2
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn rust_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<RustParser>(
            "fn f(a: bool, b: bool, c: bool, d: bool) -> bool {
                 (a && b) || (c && d)  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn rust_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<RustParser>(
            "fn f(a: bool, b: bool, c: bool, d: bool) -> bool {
                 a || (b && c && d)  // +1(||) +1(&&) = 2
             }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn c_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<CppParser>(
            "int f(int a, int b, int c, int d) {
                 return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.c",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn c_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<CppParser>(
            "int f(int a, int b, int c, int d) {
                 return a || (b && c && d);  // +1(||) +1(&&) = 2
             }",
            "foo.c",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn mozjs_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<MozjsParser>(
            "function f(a, b, c, d) {
                 return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn mozjs_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<MozjsParser>(
            "function f(a, b, c, d) {
                 return a || (b && c && d);  // +1(||) +1(&&) = 2
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn typescript_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<TypescriptParser>(
            "function f(a: boolean, b: boolean, c: boolean, d: boolean): boolean {
                 return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn typescript_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<TypescriptParser>(
            "function f(a: boolean, b: boolean, c: boolean, d: boolean): boolean {
                 return a || (b && c && d);  // +1(||) +1(&&) = 2
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<TsxParser>(
            "function f(a: boolean, b: boolean, c: boolean, d: boolean): boolean {
                 return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tsx_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<TsxParser>(
            "function f(a: boolean, b: boolean, c: boolean, d: boolean): boolean {
                 return a || (b && c && d);  // +1(||) +1(&&) = 2
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn javascript_nullish_coalescing_chain_230() {
        // Regression for issue #230: `??` is a short-circuit operator and
        // must form a boolean sequence. `a ?? b ?? c` is a single chain
        // of identical operators and collapses to a single +1 under
        // Sonar B1 (same rule as `&&` / `||`).
        check_metrics::<JavascriptParser>(
            "function pick(a, b, c) {
                 return a ?? b ?? c; // +1 (chain of ??)
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_nullish_coalescing_with_if_230() {
        // Regression for issue #230: the example from the issue body.
        // Boolean sequences pay a flat +1 (no nesting penalty) per Sonar
        // B1, so the issue body's stated total of 3 was wrong — the
        // correct answer is if(+1) + ?? chain (+1) = 2. Previously the
        // `??` chain was not counted at all (= 1).
        check_metrics::<TypescriptParser>(
            "function risky(x: string | null, fallback: string | null): string {
                 if (x === \"y\") { // +1
                     return x ?? fallback ?? \"unknown\"; // +1 (chain of ??)
                 }
                 return \"no\";
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn tsx_nullish_coalescing_chain_230() {
        // Regression for issue #230: TSX parity with JS/TS for `??`.
        check_metrics::<TsxParser>(
            "function pick(a: number | null, b: number | null, c: number): number {
                 return a ?? b ?? c; // +1 (chain of ??)
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_nullish_coalescing_chain_230() {
        // Regression for issue #230: Mozjs parity with JS for `??`.
        check_metrics::<MozjsParser>(
            "function pick(a, b, c) {
                 return a ?? b ?? c; // +1 (chain of ??)
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_null_coalescing_cognitive_230() {
        // Regression for issue #230: C# `??` must form a boolean sequence
        // just like `&&` / `||`. Boolean sequences pay a flat +1 (no
        // nesting penalty) per Sonar B1.
        // if(+1) + ?? chain (+1) = 2. Previously the `??` chain
        // contributed nothing and the function scored 1.
        check_metrics::<CsharpParser>(
            "class C {
                 string Risky(string x, string fallback) {
                     if (x == \"y\") { // +1
                         return x ?? fallback ?? \"unknown\"; // +1 (chain of ??)
                     }
                     return \"no\";
                 }
             }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_null_coalescing_cognitive_230() {
        // Regression for issue #230: PHP `??` must form a boolean sequence
        // just like `&&` / `||`. Parallels the PHP cyclomatic
        // null-coalescing handling. Boolean sequences pay a flat +1 (no
        // nesting penalty) per Sonar B1.
        // if(+1) + ?? chain (+1) = 2.
        check_metrics::<PhpParser>(
            "<?php
            function risky($x, $fallback) {
                if ($x === \"y\") { // +1
                    return $x ?? $fallback ?? \"unknown\"; // +1 (chain of ??)
                }
                return \"no\";
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    // Companions to `php_null_coalescing_cognitive_230`: the PHP
    // cognitive operator set extends past `&&` / `||` / `??` to include
    // the word-form `and` / `or` / `xor`, mirroring PHP cyclomatic. A
    // chain of identical word-form operators collapses to a single
    // boolean-sequence increment under Sonar B1, the same way `&&` /
    // `||` chains do. Each word-form gets its own test so a regression
    // that drops a single variant (e.g. only `Or`) is still caught.

    #[test]
    fn php_word_form_and_forms_boolean_sequence_230() {
        check_metrics::<PhpParser>(
            "<?php
            function check_and($a, $b, $c, $d) {
                if ($a and $b and $c and $d) { // +1 (if) + 1 (and chain)
                    return true;
                }
                return false;
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
            },
        );
    }

    #[test]
    fn php_word_form_or_forms_boolean_sequence_230() {
        check_metrics::<PhpParser>(
            "<?php
            function check_or($a, $b, $c, $d) {
                if ($a or $b or $c or $d) { // +1 (if) + 1 (or chain)
                    return true;
                }
                return false;
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
            },
        );
    }

    #[test]
    fn php_word_form_xor_forms_boolean_sequence_230() {
        check_metrics::<PhpParser>(
            "<?php
            function check_xor($a, $b, $c, $d) {
                if ($a xor $b xor $c xor $d) { // +1 (if) + 1 (xor chain)
                    return true;
                }
                return false;
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
            },
        );
    }

    #[test]
    fn java_cognitive_else_if_chain() {
        // Regression for #115: else-if chains must not receive a nesting
        // increment for the `if` inside `else if`. Expected breakdown:
        // if(+1) + else(+1) + else(+1) + else(+1) = 4.
        check_metrics::<JavaParser>(
            "class X {
                public static void f(int x) {
                    if (x > 10) {
                    } else if (x > 5) {
                    } else if (x > 0) {
                    } else {
                    }
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_cognitive_nested_else_if() {
        // Regression for #115: else-if inside a loop must still respect
        // the loop's nesting for the initial `if`, but the `else if`
        // branch should only pay a flat +1 via the `else` keyword.
        // for(+1) + if at nesting=1(+2) + else(+1) + else(+1) = 5.
        check_metrics::<JavaParser>(
            "class X {
                public static void f(int x) {
                    for (int i = 0; i < x; i++) {
                        if (i > 10) {
                        } else if (i > 5) {
                        } else {
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 5.0,
                      "min": 0.0,
                      "max": 5.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_cognitive_if_inside_else_block_is_not_else_if() {
        // Regression for #115: an `if` whose previous sibling is the block's
        // opening brace (not the `else` keyword) is a nested independent
        // statement, NOT an else-if continuation. It must pay the full
        // nesting penalty.
        // if(+1, nesting=0) + else(+1) + inner if(+2, nesting=1) = 4.
        check_metrics::<JavaParser>(
            "class X {
                public static void f(int a, int c) {
                    if (a > 0) {
                    } else {
                        if (c > 0) {
                        }
                    }
                }
            }",
            "foo.java",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<JavaParser>(
            "class X {
                 boolean f(boolean a, boolean b, boolean c, boolean d) {
                     return (a && b) || (c && d);  // +1(&&) +1(||) +1(&&) = 3
                 }
             }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn java_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<JavaParser>(
            "class X {
                 boolean f(boolean a, boolean b, boolean c, boolean d) {
                     return a || (b && c && d);  // +1(||) +1(&&) = 2
                 }
             }",
            "foo.java",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn groovy_no_cognitive() {
        check_metrics::<GroovyParser>("class A { int x = 42 }", "foo.groovy", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
        });
    }

    #[test]
    fn groovy_single_branch_function() {
        check_metrics::<GroovyParser>(
            "void f(int x) {
                if (x > 0) {
                    println(x)
                }
            }",
            "foo.groovy",
            |metric| {
                // if = +1
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_nested_if() {
        check_metrics::<GroovyParser>(
            "void f(int x, int y) {
                if (x > 0) {
                    if (y > 0) {
                        println(x)
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // outer if (+1) + inner if (+2 for nesting depth 1) = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_else_if_chain() {
        // Regression for the #115 / #239 stub pattern: an `else if`
        // chain must NOT receive a nesting increment for the `if`
        // inside `else if`. Without the sibling-`Else` pattern in
        // `Checker::is_else_if`, this would have scored higher.
        check_metrics::<GroovyParser>(
            "class X {
                static void f(int x) {
                    if (x > 10) {
                    } else if (x > 5) {
                    } else if (x > 0) {
                    } else {
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // if(+1) + else(+1) + else(+1) + else(+1) = 4
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
            },
        );
    }

    #[test]
    fn groovy_else_if_chain_lower_than_nested_ifs() {
        // The `else if` chain in `groovy_else_if_chain` MUST score
        // lower than an equivalent depth of nested `if` blocks — this
        // is the inequality the test exists to defend (lesson 10).
        check_metrics::<GroovyParser>(
            "class X {
                static void f(int x) {
                    if (x > 10) {
                        if (x > 5) {
                            if (x > 0) {
                            }
                        }
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // 3 nested `if`s: 1 + 2 + 3 = 6 (each deeper layer
                // pays a higher nesting cost). The chain in
                // `groovy_else_if_chain` produces 4, so this MUST
                // exceed it.
                assert!(metric.cognitive.cognitive_sum() > 4.0);
            },
        );
    }

    #[test]
    fn groovy_sequence_booleans_same_op() {
        // SonarSource B1: a chain of identical short-circuit ops counts as one.
        check_metrics::<GroovyParser>(
            "void f(boolean a, boolean b, boolean c) {
                if (a && b && c) { println(a) }
            }",
            "foo.groovy",
            |metric| {
                // if (+1) + boolean sequence (+1) = 2
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
            },
        );
    }

    #[test]
    fn groovy_sequence_booleans_mixed_ops() {
        // A `&&` followed by `||` is two distinct sequences = +2.
        check_metrics::<GroovyParser>(
            "void f(boolean a, boolean b, boolean c) {
                if (a && b || c) { println(a) }
            }",
            "foo.groovy",
            |metric| {
                // if (+1) + && (+1) + || (+1) = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_not_operator_negation() {
        // SonarSource: `!` negation flips a boolean sequence's polarity
        // but doesn't add cognitive cost on its own.
        check_metrics::<GroovyParser>(
            "void f(boolean a, boolean b) {
                if (a && !b) { println(a) }
            }",
            "foo.groovy",
            |metric| {
                // if(+1) + && (+1) = 2
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
            },
        );
    }

    #[test]
    fn groovy_for_while_do_loops() {
        check_metrics::<GroovyParser>(
            "void f(int n) {
                for (int i = 0; i < n; i++) {
                    while (i > 0) {
                        i--
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // for(+1) + while inside for(+2) = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_enhanced_for() {
        check_metrics::<GroovyParser>(
            "void f(List items) {
                for (item in items) {
                    println(item)
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_try_catch_nesting() {
        check_metrics::<GroovyParser>(
            "void f() {
                try {
                    risky()
                } catch (Exception e) {
                    handle(e)
                }
            }",
            "foo.groovy",
            |metric| {
                // catch(+1) = 1
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_ternary_expression() {
        check_metrics::<GroovyParser>(
            "void f(int x) {
                def y = (x > 0) ? 1 : 2
            }",
            "foo.groovy",
            |metric| {
                // ternary(+1) = 1
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_elvis_chain_246() {
        // Regression for issue #246: Groovy's Elvis operator `?:` is
        // a short-circuit nullish operator analogous to Kotlin's `?:`
        // (#239) and JS `??`. `a ?: b ?: c` is a single chain of
        // identical operators and collapses to a single +1 under
        // SonarSource Cognitive Complexity B1 — the same rule applied
        // to `&&` / `||`. Closed by swapping the prior amaanq grammar
        // (which mis-parsed Elvis as `ternary_expression` + MISSING
        // identifier) for `dekobon-tree-sitter-groovy`, which models
        // Elvis as a distinct `elvis_expression` node.
        check_metrics::<GroovyParser>(
            "def pick(a, b, c) {
                return a ?: b ?: c // +1 (Elvis chain)
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_elvis_inside_if_246() {
        // Regression for issue #246: Elvis chain inside an `if` body.
        // Boolean sequences pay a flat +1 (no nesting penalty) per
        // SonarSource B1: if(+1) + Elvis chain(+1) = 2.
        check_metrics::<GroovyParser>(
            "def f(a, b) {
                if (a != null) { // +1
                    return a ?: b ?: 'x' // +1 (Elvis chain)
                }
                return 'no'
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
            },
        );
    }

    #[test]
    fn groovy_labeled_break_continue() {
        // SonarSource B2: labeled break/continue each add +1.
        check_metrics::<GroovyParser>(
            "void f() {
                outer:
                for (int i = 0; i < 10; i++) {
                    inner:
                    for (int j = 0; j < 10; j++) {
                        if (i == j) break outer
                        if (i < j) continue inner
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // for(+1) + for(+2 nested) + if(+3) + break label(+1)
                // + if(+3) + continue label(+1) = 11
                assert_eq!(metric.cognitive.cognitive_sum(), 11.0);
            },
        );
    }

    #[test]
    fn groovy_multiple_branch_function() {
        // Sibling `if` statements at the same nesting level each
        // contribute +1; an `else` at the same level adds another
        // +1 via the Else arm.
        check_metrics::<GroovyParser>(
            "class X {
                static void print(boolean a, boolean b) {
                    if (a) {
                        println 'test1'
                    }
                    if (b) {
                        println 'test2'
                    } else {
                        println 'test3'
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // if(+1) + if(+1) + else(+1) = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_unlabeled_break_continue_not_counted() {
        // SonarSource B2: plain `break` / `continue` are NOT
        // unstructured jumps and must add 0 — only labeled forms
        // pay the +1. Matches Java's identical fixture.
        check_metrics::<GroovyParser>(
            "class X {
                void scan(int[] m) {
                    for (int i = 0; i < m.length; i++) {
                        if (m[i] < 0) continue
                        if (m[i] > 100) break
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // for(+1) + if(+2) + if(+2) = 5 (break/continue add 0)
                assert_eq!(metric.cognitive.cognitive_sum(), 5.0);
            },
        );
    }

    #[test]
    fn groovy_cognitive_nested_else_if() {
        // Regression for the #115 stub pattern at deeper nesting:
        // an `else if` chain inside a `for` loop must still respect
        // the loop's nesting for the initial `if`, but each
        // `else`-chained branch pays a flat +1 via the Else arm.
        // Matches Java's identical fixture.
        check_metrics::<GroovyParser>(
            "class X {
                static void f(int x) {
                    for (int i = 0; i < x; i++) {
                        if (i > 10) {
                        } else if (i > 5) {
                        } else {
                        }
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // for(+1) + if at nesting=1(+2) + else(+1) + else(+1) = 5
                assert_eq!(metric.cognitive.cognitive_sum(), 5.0);
            },
        );
    }

    #[test]
    fn groovy_cognitive_if_inside_else_block_is_not_else_if() {
        // Regression for #115 — an inner `if` whose previous sibling
        // is the block's opening brace (not the `else` keyword) is a
        // nested independent statement, NOT an else-if continuation,
        // so it pays the full nesting penalty. Matches Java's
        // identical fixture.
        check_metrics::<GroovyParser>(
            "class X {
                static void f(int a, int c) {
                    if (a > 0) {
                    } else {
                        if (c > 0) {
                        }
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // if(+1, nesting=0) + else(+1) + inner if(+2, nesting=1) = 4
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
            },
        );
    }

    #[test]
    fn groovy_nested_ternary() {
        // Nested ternaries inside an `if` compound by nesting — same
        // rule as Java's `java_nested_ternary` (which itself mirrors
        // the C++ regression for #172).
        check_metrics::<GroovyParser>(
            "class X {
                static String classify(int a, int b) {
                    if (a > 0) {
                        return b > 0 ? (b > 10 ? 'big' : 'small') : 'neg'
                    }
                    return 'zero'
                }
            }",
            "foo.groovy",
            |metric| {
                // if(+1, nesting=0) + outer ternary(+1+1=+2, nesting=1)
                // + inner ternary(+1+2=+3, nesting=2) = 6
                assert_eq!(metric.cognitive.cognitive_sum(), 6.0);
            },
        );
    }

    #[test]
    fn csharp_cognitive_else_if_chain() {
        // Regression for #115: else-if chains must not receive a nesting
        // increment for the `if` inside `else if`. Expected breakdown:
        // if(+1) + else(+1) + else(+1) + else(+1) = 4.
        check_metrics::<CsharpParser>(
            "class X {
                public static void F(int x) {
                    if (x > 10) {
                    } else if (x > 5) {
                    } else if (x > 0) {
                    } else {
                    }
                }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_cognitive_nested_else_if() {
        // Regression for #115: else-if inside a loop must still respect
        // the loop's nesting for the initial `if`, but the `else if`
        // branch should only pay a flat +1 via the `else` keyword.
        // for(+1) + if at nesting=1(+2) + else(+1) + else(+1) = 5.
        check_metrics::<CsharpParser>(
            "class X {
                public static void F(int x) {
                    for (int i = 0; i < x; i++) {
                        if (i > 10) {
                        } else if (i > 5) {
                        } else {
                        }
                    }
                }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 5.0,
                      "min": 0.0,
                      "max": 5.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_cognitive_if_inside_else_block_is_not_else_if() {
        // Regression for #115: an `if` whose previous sibling is the block's
        // opening brace (not the `else` keyword) is a nested independent
        // statement, NOT an else-if continuation. It must pay the full
        // nesting penalty.
        // if(+1, nesting=0) + else(+1) + inner if(+2, nesting=1) = 4.
        check_metrics::<CsharpParser>(
            "class X {
                public static void F(int a, int c) {
                    if (a > 0) {
                    } else {
                        if (c > 0) {
                        }
                    }
                }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 4.0,
                      "min": 0.0,
                      "max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<CsharpParser>(
            "class X {
                bool F(bool a, bool b, bool c, bool d) {
                    return (a && b) || (c && d);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<CsharpParser>(
            "class X {
                bool F(bool a, bool b, bool c, bool d) {
                    return a || (b && c && d);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn kotlin_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<KotlinParser>(
            "fun f(a: Boolean, b: Boolean, c: Boolean, d: Boolean) =
                 (a && b) || (c && d)  // +1(&&) +1(||) +1(&&) = 3",
            "foo.kt",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn kotlin_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<KotlinParser>(
            "fun f(a: Boolean, b: Boolean, c: Boolean, d: Boolean) =
                 a || (b && c && d)  // +1(||) +1(&&) = 2",
            "foo.kt",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn kotlin_elvis_chain_239() {
        // Regression for issue #239: Kotlin's Elvis operator `?:` is a
        // short-circuit nullish operator analogous to JS `??` and must
        // form a boolean sequence. `a ?: b ?: c` is a single chain of
        // identical operators and collapses to a single +1 under Sonar
        // B1 (same rule as `&&` / `||`). Previously the Elvis chain was
        // not counted at all (= 0).
        check_metrics::<KotlinParser>(
            "fun pick(a: String?, b: String?, c: String): String = a ?: b ?: c // +1 (Elvis chain)",
            "foo.kt",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn kotlin_elvis_inside_if_239() {
        // Regression for issue #239: Elvis chain inside an `if` body.
        // Boolean sequences pay a flat +1 (no nesting penalty) per
        // Sonar B1: if(+1) + ?: chain(+1) = 2. Previously the Elvis
        // chain was not counted at all and the function scored 1.
        check_metrics::<KotlinParser>(
            "fun f(a: String?, b: String?): String {
                 if (a != null) { // +1
                     return a ?: b ?: \"x\" // +1 (Elvis chain)
                 }
                 return \"no\"
             }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_sibling_bool_sequences() {
        // (a&&b)||(c&&d) — the right-hand && is a sibling, not nested.
        // Expected: &&(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<GoParser>(
            "package main
            func f(a, b, c, d bool) bool {
                return (a && b) || (c && d)  // +1(&&) +1(||) +1(&&) = 3
            }",
            "foo.go",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn go_nested_bool_same_op() {
        // a||(b&&c&&d) — the inner && operators are nested, forming one sequence.
        // Expected: ||(+1) + &&(+1) = 2.
        check_metrics::<GoParser>(
            "package main
            func f(a, b, c, d bool) bool {
                return a || (b && c && d)  // +1(||) +1(&&) = 2
            }",
            "foo.go",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_sibling_bool_sequences() {
        // ($a && $b) || ($c && $d) — the right-hand && is a sibling, not nested.
        // Expected: if(+1) + ||(+1) + &&(+1) + &&(+1) = 4.
        check_metrics::<TclParser>(
            "proc f {a b c d} {
    if {($a && $b) || ($c && $d)} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                assert_eq!(metric.cognitive.cognitive_max(), 4.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_nested_bool_same_op() {
        // $a || ($b && $c && $d) — the inner && operators are nested, one sequence.
        // Expected: if(+1) + ||(+1) + &&(+1) = 3.
        check_metrics::<TclParser>(
            "proc f {a b c d} {
    if {$a || ($b && $c && $d)} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn lua_sibling_bool_sequences() {
        // (a and b) or (c and d) — the right-hand `and` is a sibling, not nested.
        // Expected: if(+1) + or(+1) + and(+1) + and(+1) = 4.
        check_metrics::<LuaParser>(
            "local function f(a, b, c, d)
    if (a and b) or (c and d) then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                assert_eq!(metric.cognitive.cognitive_max(), 4.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn lua_nested_bool_same_op() {
        // a or (b and c and d) — the inner `and` operators are nested, one sequence.
        // Expected: if(+1) + or(+1) + and(+1) = 3.
        check_metrics::<LuaParser>(
            "local function f(a, b, c, d)
    if a or (b and c and d) then
        return 1
    end
end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn bash_sibling_bool_sequences() {
        // [[ a ]] && [[ b ]] || [[ c ]] && [[ d ]] — bash is left-associative so this
        // parses as ((a&&b)||c)&&d with three distinct operator-type transitions.
        // Expected: if(+1) + &&(+1) + ||(+1) + &&(+1) = 4.
        check_metrics::<BashParser>(
            "f() {
                 if [[ -n \"$a\" ]] && [[ -n \"$b\" ]] || [[ -n \"$c\" ]] && [[ -n \"$d\" ]]; then
                     echo test
                 fi
             }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                assert_eq!(metric.cognitive.cognitive_max(), 4.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn bash_nested_bool_same_op() {
        // [[ a ]] || [[ b ]] && [[ c ]] && [[ d ]] — bash left-associativity gives
        // ((a||b)&&c)&&d: the two && operators are parent/child so the second is
        // a continuation (no extra increment).
        // Expected: if(+1) + &&(+1, outer chain) + ||(+1) = 3.
        check_metrics::<BashParser>(
            "f() {
                 if [[ -n \"$a\" ]] || [[ -n \"$b\" ]] && [[ -n \"$c\" ]] && [[ -n \"$d\" ]]; then
                     echo test
                 fi
             }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_no_cognitive() {
        check_metrics::<PhpParser>("<?php $a = 42;", "foo.php", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
            assert_eq!(metric.cognitive.cognitive_max(), 0.0);
            insta::assert_json_snapshot!(metric.cognitive);
        });
    }

    #[test]
    fn php_simple_function() {
        // Single `if` inside a function: +1.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a): void {
                if ($a) {
                    echo 'hi';
                }
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_ternary() {
        // PHP's ternary `?:` (grammar `conditional_expression`) is a
        // conditional construct: +1 base + nesting. Regression test for
        // issue #224. Note: this differs from PHP's
        // `match_conditional_expression` (the `match` expression),
        // which is handled separately by `MatchExpression`.
        check_metrics::<PhpParser>(
            "<?php
            function check(int $a): bool {
                return $a > 0 ? true : false; // +1
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 1.0,
                      "average": 1.0,
                      "min": 0.0,
                      "max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_nested_ternary() {
        // Nested ternaries inside an `if` compound by nesting (mirrors
        // the C++ regression test for #172).
        // expected: if (+1) + outer ternary (+2, nesting=1) + inner
        // ternary (+3, nesting=2) = 6.
        check_metrics::<PhpParser>(
            "<?php
            function classify(int $a, int $b): string {
                if ($a > 0) { // +1
                    return $b > 0 ? ($b > 10 ? 'big' : 'small') : 'neg'; // +2, +3
                }
                return 'zero';
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 6.0);
                assert_eq!(metric.cognitive.cognitive_max(), 6.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 6.0,
                      "min": 0.0,
                      "max": 6.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_sequence_same_booleans() {
        // Sequence of same-operator booleans collapses: a chain of `&&`
        // counts as +1 total, not per-operand.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a, bool $b, bool $c): bool {
                return $a && $b && $c;
            }",
            "foo.php",
            |metric| {
                // Chain of identical && collapses to a single +1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_sequence_different_booleans() {
        // Mix of `&&` and `||` — each operator switch costs +1.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a, bool $b, bool $c): bool {
                return $a && $b || $c;
            }",
            "foo.php",
            |metric| {
                // && chain (+1) + switch to || (+1) = 2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_not_booleans() {
        // `!` does not break boolean sequences (issue #392): pre-order
        // visits the outer `&&` BinaryExpression first, so the inner
        // `&&` lies within its span and is a continuation.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a, bool $b, bool $c): bool {
                return $a && !($b && $c);
            }",
            "foo.php",
            |metric| {
                // Outer && (+1); inner && continues outer's span → 1.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_1_level_nesting() {
        // if-inside-loop: outer for (+1) + inner if at depth 1 (+2) = +3.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $n): int {
                for ($i = 0; $i < $n; $i++) {
                    if ($i % 2 === 0) {
                        return $i;
                    }
                }
                return -1;
            }",
            "foo.php",
            |metric| {
                // for(+1) + if at depth 1 (+2) = 3.
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_2_level_nesting() {
        // for + while + if = +1 +2 +3 = +6.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $n): int {
                for ($i = 0; $i < $n; $i++) {
                    while ($i > 0) {
                        if ($i % 2 === 0) {
                            return $i;
                        }
                    }
                }
                return -1;
            }",
            "foo.php",
            |metric| {
                // for(+1) + while at depth 1 (+2) + if at depth 2 (+3) = 6.
                assert_eq!(metric.cognitive.cognitive_sum(), 6.0);
                assert_eq!(metric.cognitive.cognitive_max(), 6.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_break_continue() {
        // PHP `break` and `continue` are not cognitive drivers in this
        // impl; only the surrounding loops count.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $n): int {
                for ($i = 0; $i < $n; $i++) {
                    if ($i % 2 === 0) {
                        continue;
                    }
                    if ($i > 100) {
                        break;
                    }
                }
                return 0;
            }",
            "foo.php",
            |metric| {
                // for(+1) + first if at depth 1 (+2) + second if at depth 1 (+2) = 5.
                assert_eq!(metric.cognitive.cognitive_sum(), 5.0);
                assert_eq!(metric.cognitive.cognitive_max(), 5.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_goto_counted() {
        // `goto label;` is a genuinely unstructured jump and adds +1 per
        // SonarSource Cognitive Complexity §B2 (issue #435), matching
        // C++/C#/Go/Perl/Lua goto handling.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $n): int {
                if ($n < 0) {
                    goto done;
                }
                done:
                return 0;
            }",
            "foo.php",
            |metric| {
                // if(+1) + goto(+1) = 2.
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_numeric_break_not_counted() {
        // PHP has no labeled break/continue; only the numeric level form
        // `break N;` / `continue N;`, which exits N enclosing loops already
        // accounted for by nesting. Per issue #435 the numeric form is a
        // structured loop-level exit and adds +0.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $n): int {
                for ($i = 0; $i < $n; $i++) {
                    while (true) {
                        if ($i > 100) {
                            break 2;
                        }
                    }
                }
                return 0;
            }",
            "foo.php",
            |metric| {
                // for(+1) + while at depth 1 (+2) + if at depth 2 (+3) = 6;
                // `break 2` adds +0.
                assert_eq!(metric.cognitive.cognitive_sum(), 6.0);
                assert_eq!(metric.cognitive.cognitive_max(), 6.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // ----- Elixir -----

    // No control flow → cognitive complexity is 0.
    #[test]
    fn elixir_empty_function() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    x\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r#"
                {
                  "sum": 0.0,
                  "average": 0.0,
                  "min": 0.0,
                  "max": 0.0
                }
                "#
                );
            },
        );
    }

    // `if cond do … end`: single-branch construct → +1 nesting at depth
    // 0 inside `def` body → cognitive 1.
    #[test]
    fn elixir_simple_if() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    if x > 0 do\n      :pos\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // `if cond do … else … end`: +1 nesting for `if`, +1 for `else` token
    // (matches Java/Kotlin) → cognitive 2.
    #[test]
    fn elixir_if_else() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    if x > 0 do\n      :pos\n    else\n      :neg\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: if (+1) + else (+1) = 2
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // `case x do … end` with three arms: only the container Call earns
    // a nesting bump (matches Java's `SwitchBlock` rule). Individual
    // `stab_clause` arms add no extra cost. Expected cognitive 1.
    #[test]
    fn elixir_case_arms_count_once() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    case x do\n      1 -> :one\n      2 -> :two\n      _ -> :other\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: case +1 (one nesting bump on the container)
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // `cond do … end` is structurally identical to `case` for our
    // purposes: container Call earns +1 nesting; arms add nothing.
    #[test]
    fn elixir_cond_counts_once() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    cond do\n      x > 0 -> :pos\n      x < 0 -> :neg\n      true -> :zero\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: cond +1
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // Nested `if` inside another `if`: outer +1, inner +2 (nested
    // depth 1) → cognitive 3.
    #[test]
    fn elixir_nested_if_amplifies() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x, y) do\n    if x > 0 do\n      if y > 0 do\n        :both\n      end\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: outer if (+1) + nested if (+2 because nesting=1)
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // `try` with `rescue` and `catch`: the `try` wrapper itself does
    // NOT bump nesting (matches Java / C#'s "try is a wrapper" rule);
    // each `rescue` / `catch` block bumps +1 nesting at depth 0. The
    // single `stab_clause` inside each block adds no extra cost.
    #[test]
    fn elixir_try_rescue_catch() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    try do\n      :ok\n    rescue\n      _ -> :err\n    catch\n      _ -> :thrown\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: rescue (+1) + catch (+1) = 2
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // Short-circuit booleans: `x && y || z` is two operator types in
    // sequence — `&&` once, `||` once → +2. The `if` container that
    // surrounds them adds +1 → total cognitive 3.
    #[test]
    fn elixir_boolean_sequence() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x, y, z) do\n    if x && y || z do\n      :hit\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: if (+1) + && (+1) + || (+1) = 3
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // `Enum.reduce` (and friends) are higher-order calls, NOT control
    // flow per the SonarSource spec. They contribute nothing to
    // cognitive complexity. The anonymous function body inside
    // contributes +1 lambda nesting, but its only operation is a
    // function call (no control flow) → cognitive 0.
    #[test]
    fn elixir_enum_reduce_is_zero() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def sum(xs) do\n    Enum.reduce(xs, 0, fn x, acc -> acc + x end)\n  end\nend\n",
            "foo.ex",
            |metric| {
                // expected: 0 — Enum.reduce is a function call, not
                // syntactic control flow; the `fn` body has no decisions.
                assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    // Recursion: a `def` whose body calls itself by name. Per the
    // SonarSource spec recursion is +1, but our impl skips it for
    // scope reasons (documented). The body's lone Call earns nothing,
    // so cognitive stays at 0. This test pins the documented omission
    // so any future recursion work has to update it deliberately.
    #[test]
    fn elixir_recursion_is_zero_documented_limitation() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def fact(0), do: 1\n  def fact(n), do: n * fact(n - 1)\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_match_cognitive() {
        // `match` is treated like `switch`: a single nesting bump for the
        // whole construct, not per arm.
        check_metrics::<PhpParser>(
            "<?php
            function color(string $c): int {
                return match ($c) {
                    'red' => 1,
                    'green' => 2,
                    default => 0,
                };
            }",
            "foo.php",
            |metric| {
                // `match` is treated like `switch`: a single +1 for the construct.
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                assert_eq!(metric.cognitive.cognitive_max(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_no_cognitive() {
        check_metrics::<RubyParser>("a = 42\n", "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
            insta::assert_json_snapshot!(metric.cognitive);
        });
    }

    #[test]
    fn ruby_simple_function() {
        // A function body with no branching scores zero cognitive.
        check_metrics::<RubyParser>("def foo\n  a = 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 0.0);
            insta::assert_json_snapshot!(metric.cognitive);
        });
    }

    #[test]
    fn ruby_1_level_nesting() {
        // Single `if` inside a function: +1.
        check_metrics::<RubyParser>("def foo\n  if a\n    b\n  end\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
            insta::assert_json_snapshot!(metric.cognitive);
        });
    }

    #[test]
    fn ruby_2_level_nesting() {
        // expected: outer `if` (+1) + inner `if` (+2, nested) = 3.
        check_metrics::<RubyParser>(
            "def foo\n  if a\n    if b\n      c\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_sequence_same_booleans() {
        // `a && b && c`: same operator collapses to a single boolean
        // sequence (+1). Plus the enclosing `if` (+1) → 2.
        check_metrics::<RubyParser>(
            "def foo\n  if a && b && c\n    d\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_sequence_different_booleans() {
        // `a && b || c`: alternating operators add per change.
        check_metrics::<RubyParser>(
            "def foo\n  if a && b || c\n    d\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_not_booleans() {
        // `!a` (Unary) is the not-operator: it doesn't add cognitive
        // load by itself. Only the enclosing `if` counts.
        check_metrics::<RubyParser>(
            "def foo\n  if !a\n    b\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_break_next() {
        // Ruby has no labeled loops, so `break`/`next` are always
        // unlabeled. Per SonarSource Cognitive Complexity §B2 an unlabeled
        // break/continue adds +0 (issue #435) — only the enclosing `while`
        // (+1) counts → 1.
        check_metrics::<RubyParser>(
            "def foo\n  while a\n    break\n    next\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 1.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_redo_retry_counted() {
        // `redo` (restart the current loop iteration) and `retry` (re-run a
        // rescued `begin` block) are genuinely unstructured jumps with no
        // structured equivalent, so each adds +1 per SonarSource §B2
        // (issue #435) even though `break`/`next` do not.
        check_metrics::<RubyParser>(
            "def foo\n  while a\n    redo\n  end\n  begin\n    work\n  rescue\n    retry\n  end\nend\n",
            "foo.rb",
            |metric| {
                // while(+1) + redo(+1) + rescue(+1) + retry(+1) = 4.
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn ruby_else_if_chain() {
        // `elsif` extends the parent branch (no extra nesting). An
        // `if/elsif/elsif/else` chain scores strictly LESS than the
        // same number of nested `if` blocks. tree-sitter-ruby gives
        // `elsif` its own clause node, so the lesson-10 trap (a buggy
        // `is_else_if` that returns false makes `elsif` nest like
        // `if`) doesn't apply directly here — the test still pins the
        // chain vs nested cost difference so a future refactor that
        // mis-classifies `Elsif` would regress it.
        // expected: chain = 1 (`if`) + 2 (two `elsif`) + 1 (`else`) = 4;
        // nested = 1 + 2 + 3 = 6. The literal `4 < 6` asserts the
        // intended relationship.
        check_metrics::<RubyParser>(
            "def foo\n  if a\n    1\n  elsif b\n    2\n  elsif c\n    3\n  else\n    4\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 4.0);
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
        check_metrics::<RubyParser>(
            "def foo\n  if a\n    if b\n      if c\n        1\n      end\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 6.0);
            },
        );
    }

    #[test]
    fn javascript_labeled_break_continue() {
        // Per SonarSource Cognitive Complexity §B2 (issue #435), a labeled
        // `break LABEL` / `continue LABEL` is an unstructured jump and adds
        // +1. The JS-family grammar exposes the label as a
        // `statement_identifier` child of the break/continue node.
        check_metrics::<JavascriptParser>(
            "function scan(m) {
                outer:
                for (let i = 0; i < m.length; i++) {      // +1
                    for (let j = 0; j < m[i].length; j++) { // +2
                        if (m[i][j] < 0) continue outer;    // +3, +1
                        if (m[i][j] > 100) break outer;     // +3, +1
                    }
                }
            }",
            "foo.js",
            |metric| {
                // outer for(+1) + inner for(+2) + if(+3) + continue outer(+1)
                // + if(+3) + break outer(+1) = 11.
                assert_eq!(metric.cognitive.cognitive_sum(), 11.0);
                assert_eq!(metric.cognitive.cognitive_max(), 11.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 11.0,
                      "average": 11.0,
                      "min": 0.0,
                      "max": 11.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_unlabeled_break_continue_not_counted() {
        // Negative test for issue #435: plain `break;` / `continue;` are
        // not unstructured jumps under SonarSource §B2 and add +0. Only the
        // surrounding `for` + two `if`s contribute.
        check_metrics::<JavascriptParser>(
            "function scan(m) {
                for (let i = 0; i < m.length; i++) { // +1
                    if (m[i] < 0) continue;           // +2, +0
                    if (m[i] > 100) break;            // +2, +0
                }
            }",
            "foo.js",
            |metric| {
                // for(+1) + if(+2) + if(+2) = 5.
                assert_eq!(metric.cognitive.cognitive_sum(), 5.0);
                assert_eq!(metric.cognitive.cognitive_max(), 5.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 5.0,
                      "min": 0.0,
                      "max": 5.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_labeled_break_continue() {
        // TS parity with JS for labeled jumps (issue #435): labeled
        // break/continue each add +1 via the `statement_identifier` child.
        check_metrics::<TypescriptParser>(
            "function scan(m: number[][]) {
                outer:
                for (let i = 0; i < m.length; i++) {      // +1
                    for (let j = 0; j < m[i].length; j++) { // +2
                        if (m[i][j] < 0) continue outer;    // +3, +1
                        if (m[i][j] > 100) break outer;     // +3, +1
                    }
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 11.0);
                assert_eq!(metric.cognitive.cognitive_max(), 11.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 11.0,
                      "average": 11.0,
                      "min": 0.0,
                      "max": 11.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_compound_short_circuit_assignment_236() {
        // Regression for issue #236: `&&=`, `||=`, `??=` are compound
        // short-circuit assignments (e.g. `x ??= y` ≡ `x = x ?? y`)
        // and each carries one boolean-sequence decision. Each lives
        // inside its own `expression_statement`, so the boolean
        // sequence resets between them and all three count.
        check_metrics::<JavascriptParser>(
            "function f(x) {
                 x ??= 1; // +1 (??=)
                 x &&= 2; // +1 (&&=)
                 x ||= 3; // +1 (||=)
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_compound_short_circuit_assignment_236() {
        // Regression for issue #236: TS parity with JS for `&&=`,
        // `||=`, `??=`.
        check_metrics::<TypescriptParser>(
            "function f(x: number | null) {
                 x ??= 1; // +1 (??=)
                 x &&= 2; // +1 (&&=)
                 x ||= 3; // +1 (||=)
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn tsx_compound_short_circuit_assignment_236() {
        // Regression for issue #236: TSX parity with JS/TS for `&&=`,
        // `||=`, `??=`.
        check_metrics::<TsxParser>(
            "function f(x: number | null) {
                 x ??= 1; // +1 (??=)
                 x &&= 2; // +1 (&&=)
                 x ||= 3; // +1 (||=)
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_compound_short_circuit_assignment_236() {
        // Regression for issue #236: Mozjs (SpiderMonkey-flavoured JS)
        // shares the JS macro and must score `&&=` / `||=` / `??=`
        // identically.
        check_metrics::<MozjsParser>(
            "function f(x) {
                 x ??= 1; // +1 (??=)
                 x &&= 2; // +1 (&&=)
                 x ||= 3; // +1 (||=)
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 3.0);
                assert_eq!(metric.cognitive.cognitive_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 3.0,
                      "min": 0.0,
                      "max": 3.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_compound_short_circuit_assignment_236() {
        // Regression for issue #236: C#'s grammar only provides `??=`
        // among the short-circuit assignments (no `&&=` / `||=`). The
        // operator lives inside `assignment_expression` rather than a
        // `BinaryExpression`, so without the #236 fix it was silently
        // skipped.
        check_metrics::<CsharpParser>(
            "class C {
                 int? F(int? x) {
                     x ??= 1; // +1 (??=)
                     return x ?? 0;
                 }
             }",
            "foo.cs",
            |metric| {
                // Outer `??` chain (+1) + `??=` (+1) = 2 at function max.
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_compound_short_circuit_assignment_236() {
        // Regression for issue #236: PHP's only compound short-circuit
        // assignment is `??=` (no `&&=` / `||=`). It lives inside
        // `augmented_assignment_expression` rather than a
        // `BinaryExpression`, so without the #236 fix it was silently
        // skipped.
        check_metrics::<PhpParser>(
            "<?php
            function f($x) {
                $x ??= 1; // +1 (??=)
                return $x ?? 0; // +1 (??)
            }",
            "foo.php",
            |metric| {
                assert_eq!(metric.cognitive.cognitive_sum(), 2.0);
                assert_eq!(metric.cognitive.cognitive_max(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cognitive,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 2.0,
                      "min": 0.0,
                      "max": 2.0
                    }"###
                );
            },
        );
    }
}
