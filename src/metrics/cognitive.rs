use std::collections::HashMap;

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::fmt;

use crate::checker::Checker;
use crate::macros::{csharp_prefix_unary_expr_kinds, implement_metric_trait};
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
    pub fn cognitive(&self) -> f64 {
        self.structural as f64
    }
    /// Returns the `Cognitive Complexity` sum metric value
    pub fn cognitive_sum(&self) -> f64 {
        self.structural_sum as f64
    }

    /// Returns the `Cognitive Complexity` minimum metric value
    pub fn cognitive_min(&self) -> f64 {
        self.structural_min as f64
    }
    /// Returns the `Cognitive Complexity` maximum metric value
    pub fn cognitive_max(&self) -> f64 {
        self.structural_max as f64
    }

    /// Returns the `Cognitive Complexity` metric average value
    ///
    /// This value is computed dividing the `Cognitive Complexity` value
    /// for the total number of functions/closures in a space.
    ///
    /// If there are no functions in a code, its value is `NAN`.
    pub fn cognitive_average(&self) -> f64 {
        self.cognitive_sum() / self.total_space_functions as f64
    }
    #[inline(always)]
    pub(crate) fn compute_sum(&mut self) {
        self.structural_sum += self.structural;
    }
    #[inline(always)]
    pub(crate) fn compute_minmax(&mut self) {
        self.structural_min = self.structural_min.min(self.structural);
        self.structural_max = self.structural_max.max(self.structural);
        self.compute_sum();
    }

    pub(crate) fn finalize(&mut self, total_space_functions: usize) {
        self.total_space_functions = total_space_functions;
    }
}

pub trait Cognitive
where
    Self: Checker,
{
    fn compute(
        node: &Node,
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    );
}

fn compute_booleans<T: PartialEq + From<u16>>(node: &Node, stats: &mut Stats, typs1: T, typs2: T) {
    let enclosing_end = node.end_byte();
    for child in node.children() {
        let id = child.kind_id();
        let converted: T = id.into();
        if typs1 == converted || typs2 == converted {
            stats.structural =
                stats
                    .boolean_seq
                    .eval_based_on_prev(id, enclosing_end, stats.structural);
        }
    }
}

/// Folds a Perl `binary_expression`'s short-circuit operator children into
/// the boolean-sequence counter. `compute_booleans` only takes two operator
/// kinds; Perl needs five (`&&`, `||`, `//`, `and`, `or`).
fn compute_perl_booleans(node: &Node, stats: &mut Stats) {
    let enclosing_end = node.end_byte();
    for child in node.children() {
        let id = child.kind_id();
        if matches!(
            id.into(),
            Perl::AMPAMP | Perl::PIPEPIPE | Perl::SLASHSLASH | Perl::And | Perl::Or
        ) {
            stats.structural =
                stats
                    .boolean_seq
                    .eval_based_on_prev(id, enclosing_end, stats.structural);
        }
    }
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

    fn not_operator(&mut self) {
        // NOT resets the sequence so the next boolean always scores +1
        self.reset();
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

#[inline(always)]
fn increment(stats: &mut Stats) {
    stats.structural += stats.nesting + 1;
}

#[inline(always)]
fn increment_by_one(stats: &mut Stats) {
    stats.structural += 1;
}

#[inline(always)]
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

#[inline(always)]
fn increase_nesting(stats: &mut Stats, nesting: &mut usize, depth: usize, lambda: usize) {
    stats.nesting = *nesting + depth + lambda;
    increment(stats);
    *nesting += 1;
    stats.boolean_seq.reset();
}

impl Cognitive for PythonCode {
    fn compute(
        node: &Node,
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Python::*;

        // Get nesting of the parent
        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement | ForStatement | WhileStatement | ConditionalExpression => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ElifClause => {
                // No nesting increment for them because their cost has already
                // been paid by the if construct
                increment_branch_extension(stats);
            }
            ElseClause | FinallyClause => {
                // No nesting increment for them because their cost has already
                // been paid by the if construct
                increment_by_one(stats);
            }
            ExceptClause => {
                nesting += 1;
                increment(stats);
            }
            ExpressionList | ExpressionStatement | Tuple => {
                stats.boolean_seq.reset();
            }
            NotOperator => {
                stats.boolean_seq.not_operator();
            }
            BooleanOperator => {
                if node.count_specific_ancestors::<PythonParser>(
                    |node| node.kind_id() == BooleanOperator,
                    |node| node.kind_id() == Lambda,
                ) == 0
                {
                    stats.structural += node.count_specific_ancestors::<PythonParser>(
                        |node| node.kind_id() == Lambda,
                        |node| {
                            matches!(
                                node.kind_id().into(),
                                ExpressionList | IfStatement | ForStatement | WhileStatement
                            )
                        },
                    );
                }
                compute_booleans(node, stats, And, Or);
            }
            Lambda => {
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
    fn compute(
        node: &Node,
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Rust::*;
        //TODO: Implement macros
        let (mut nesting, mut depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfExpression if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForExpression | WhileExpression | MatchExpression => {
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
            UnaryExpression => {
                stats.boolean_seq.not_operator();
            }
            BinaryExpression => {
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
    fn compute(
        node: &Node,
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Cpp::*;

        //TODO: Implement macros
        let (mut nesting, depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement | WhileStatement | DoStatement | SwitchStatement | CatchClause => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            GotoStatement | Else /* else-if also */ => {
                increment_by_one(stats);
            }
            UnaryExpression2 => {
                stats.boolean_seq.not_operator();
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
        fn compute(node: &Node, stats: &mut Stats, nesting_map: &mut HashMap<usize, (usize, usize, usize)>) {
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
                ExpressionStatement => {
                    // Reset the boolean sequence
                    stats.boolean_seq.reset();
                }
                UnaryExpression => {
                    stats.boolean_seq.not_operator();
                }
                BinaryExpression => {
                    compute_booleans(node, stats, AMPAMP, PIPEPIPE);
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
    fn compute(
        node: &Node,
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Java::*;

        let (mut nesting, depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement if !Self::is_else_if(node) => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ForStatement | WhileStatement | DoStatement | SwitchBlock | CatchClause => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            Else /* else-if also */ => {
                increment_by_one(stats);
            }
            UnaryExpression => {
                stats.boolean_seq.not_operator();
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

impl Cognitive for CsharpCode {
    fn compute(
        node: &Node,
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Csharp::*;

        let (mut nesting, depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            // `Checker::is_else_if` is `false` for C# (the grammar has no
            // `else_clause`); plain `if` always increases nesting.
            IfStatement | ForStatement | ForeachStatement | WhileStatement | DoStatement
            | SwitchStatement | SwitchExpression | CatchClause => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            // `else` is an anonymous keyword token. Each occurrence carries
            // a flat +1 for the alternative branch (matches Java's `Else`
            // handling).
            Else => {
                increment_by_one(stats);
            }
            // The grammar emits two aliased `kind_id`s for
            // `prefix_unary_expression`; both must signal `!` to the
            // boolean sequence tracker (lesson #2).
            csharp_prefix_unary_expr_kinds!() => {
                stats.boolean_seq.not_operator();
            }
            BinaryExpression => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
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
    fn compute(
        node: &Node,
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
            // `goto` is a non-local control transfer.
            P::Goto | P::GotoExpression => {
                increment_by_one(stats);
            }
            // `last LABEL` / `next LABEL` / `redo LABEL` — only the
            // labeled forms count, since the bare forms are subsumed by
            // the surrounding loop's nesting.
            P::LoopControlStatement if node.children().any(|c| c.kind_id() == P::Label) => {
                increment_by_one(stats);
            }
            P::UnaryExpression => {
                stats.boolean_seq.not_operator();
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
    fn compute(
        node: &Node,
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
            UnaryExpression => {
                stats.boolean_seq.not_operator();
            }
            BinaryExpression => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
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
    fn compute(
        node: &Node,
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
            G::BreakStatement | G::ContinueStatement
                if node.children().any(|c| c.kind_id() == G::LabelName) =>
            {
                increment_by_one(stats);
            }
            G::UnaryExpression => {
                stats.boolean_seq.not_operator();
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
    fn compute(
        node: &Node,
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
    fn compute(
        node: &Node,
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
            // Track `!` prefix so that `!$a && !$b` counts the && only once.
            UnaryExpr if node.child(0).is_some_and(|c| c.kind_id() == Tcl::BANG) => {
                stats.boolean_seq.not_operator();
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
    fn compute(
        node: &Node,
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
            // `else` increments without nesting; `break`/`goto` are unconditional
            // jumps that add cognitive load. Lua has no `continue`.
            ElseStatement | BreakStatement | GotoStatement => {
                increment_by_one(stats);
            }
            UnaryExpression => {
                stats.boolean_seq.not_operator();
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
    fn compute(
        node: &Node,
        stats: &mut Stats,
        nesting_map: &mut HashMap<usize, (usize, usize, usize)>,
    ) {
        use Php::*;

        let (mut nesting, depth, mut lambda) = get_nesting_from_map(node, nesting_map);

        match node.kind_id().into() {
            IfStatement | ForStatement | ForeachStatement | WhileStatement | DoStatement
            | SwitchStatement | MatchExpression | CatchClause => {
                increase_nesting(stats, &mut nesting, depth, lambda);
            }
            ElseClause | ElseClause2 | ElseIfClause | ElseIfClause2 => {
                increment_branch_extension(stats);
            }
            UnaryOpExpression | UnaryOpExpression2 => {
                stats.boolean_seq.not_operator();
            }
            BinaryExpression => {
                compute_booleans(node, stats, AMPAMP, PIPEPIPE);
            }
            AnonymousFunction | ArrowFunction => {
                lambda += 1;
            }
            _ => {}
        }
        nesting_map.insert(node.id(), (nesting, depth, lambda));
    }
}

implement_metric_trait!(Cognitive, PreprocCode, CcommentCode);

#[cfg(test)]
mod tests {
    use crate::tools::check_metrics;

    use super::*;

    #[test]
    fn python_no_cognitive() {
        check_metrics::<PythonParser>("a = 42", "foo.py", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn rust_no_cognitive() {
        check_metrics::<RustParser>("let a = 42;", "foo.rs", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn c_no_cognitive() {
        check_metrics::<CppParser>("int a = 42;", "foo.c", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn mozjs_no_cognitive() {
        check_metrics::<MozjsParser>("var a = 42;", "foo.js", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
            );
        });
    }

    #[test]
    fn javascript_no_cognitive() {
        check_metrics::<JavascriptParser>("var a = 42;", "foo.js", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r###"
                    {
                      "sum": 0.0,
                      "average": null,
                      "min": 0.0,
                      "max": 0.0
                    }"###
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
            "fn f() {
                 if a && !(b && c) { // +3 (+1 &&, +1 &&)
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
    fn c_not_booleans() {
        check_metrics::<CppParser>(
            "void f() {
                 if (a && !(b && c)) { // +3 (+1 &&, +1 &&)
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
        check_metrics::<MozjsParser>(
            "function f() {
                 if (a && !(b && c)) { // +3 (+1 &&, +1 &&)
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
                @r###"
            {
              "sum": 0.0,
              "average": null,
              "min": 0.0,
              "max": 0.0
            }
            "###
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
        check_metrics::<JavaParser>(
            "class X {
              public static void print(boolean a, boolean b, boolean c, boolean d){
                if (a && !(b && c)) { // +3 (+1 &&, +1 &&)
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
    fn csharp_no_cognitive() {
        check_metrics::<CsharpParser>("int a = 42;", "foo.cs", |metric| {
            insta::assert_json_snapshot!(
                metric.cognitive,
                @r###"
            {
              "sum": 0.0,
              "average": null,
              "min": 0.0,
              "max": 0.0
            }
            "###
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
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn csharp_not_booleans() {
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
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn perl_no_cognitive() {
        check_metrics::<PerlParser>("my $a = 42;", "foo.pl", |metric| {
            insta::assert_json_snapshot!(metric.cognitive, @r#"
            {
              "sum": 0.0,
              "average": null,
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
    fn perl_not_booleans() {
        check_metrics::<PerlParser>(
            "sub f {
                if ($a && !($b && $c)) { # +1 if, +1 &&, +1 inner &&
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
                @r###"
                {
                  "sum": 0.0,
                  "average": null,
                  "min": 0.0,
                  "max": 0.0
                }
                "###
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
                @r###"
                {
                  "sum": 0.0,
                  "average": null,
                  "min": 0.0,
                  "max": 0.0
                }"###
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
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_not_booleans() {
        // Each `!` marks the start of a new boolean-operator sequence; the single `&&`
        // between the two negations contributes +1, plus +1 for the surrounding `if`.
        check_metrics::<TclParser>(
            "proc f {a b} {
    if {!$a && !$b} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
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
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_not_booleans_nested() {
        // `$a && !($b && $c)`: if(+1) + outer &&(+1) + inner && after not_operator(+1) = 3.
        check_metrics::<TclParser>(
            "proc f {a b c} {
    if {$a && !($b && $c)} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn tcl_not_booleans_double_nested() {
        // `!($a || $b) && !($c || $d)`: if(+1) + &&(+1) + first || after not(+1) + second || after not(+1) = 4.
        check_metrics::<TclParser>(
            "proc f {a b c d} {
    if {!($a || $b) && !($c || $d)} {
        puts yes
    }
}",
            "foo.tcl",
            |metric| {
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
                @r###"
                {
                  "sum": 0.0,
                  "average": null,
                  "min": 0.0,
                  "max": 0.0
                }"###
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
        // `not a and not b`: each `not` resets the boolean sequence so the
        // single `and` between them counts once. if(+1) + and(+1) = 2.
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
        // Lua has no `continue` keyword; `break` is the only structural jump
        // (other than `goto`). for(+1) + if at depth 1 (+2) + break(+1) = 4.
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
                insta::assert_json_snapshot!(metric.cognitive);
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
                insta::assert_json_snapshot!(metric.cognitive);
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
                insta::assert_json_snapshot!(metric.cognitive);
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
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_no_cognitive() {
        check_metrics::<PhpParser>("<?php $a = 42;", "foo.php", |metric| {
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
                insta::assert_json_snapshot!(metric.cognitive);
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
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }

    #[test]
    fn php_not_booleans() {
        // `!` operator resets the boolean sequence so the chain re-counts.
        check_metrics::<PhpParser>(
            "<?php
            function f(bool $a, bool $b, bool $c): bool {
                return $a && !($b && $c);
            }",
            "foo.php",
            |metric| {
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
                insta::assert_json_snapshot!(metric.cognitive);
            },
        );
    }
}
