// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(
    clippy::enum_glob_use,
    clippy::too_many_lines,
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

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::fmt;

use crate::checker::Checker;
use crate::macros::{
    csharp_paren_expr_kinds, csharp_prefix_unary_expr_kinds, implement_metric_trait,
};
use crate::node::Node;
use crate::*;

/// The `ABC` metric.
///
/// The `ABC` metric measures the size of a source code by counting
/// the number of Assignments (`A`), Branches (`B`) and Conditions (`C`).
/// The metric defines an ABC score as a vector of three elements (`<A,B,C>`).
/// The ABC score can be represented by its individual components (`A`, `B` and `C`)
/// or by the magnitude of the vector (`|<A,B,C>| = sqrt(A^2 + B^2 + C^2)`).
///
/// Official paper and definition:
///
/// Fitzpatrick, Jerry (1997). "Applying the ABC metric to C, C++ and Java". C++ Report.
///
/// <https://www.softwarerenovation.com/Articles.aspx>
#[derive(Debug, Clone)]
pub struct Stats {
    assignments: f64,
    assignments_sum: f64,
    assignments_min: f64,
    assignments_max: f64,
    branches: f64,
    branches_sum: f64,
    branches_min: f64,
    branches_max: f64,
    conditions: f64,
    conditions_sum: f64,
    conditions_min: f64,
    conditions_max: f64,
    space_count: usize,
    declaration: Vec<DeclKind>,
}

#[derive(Debug, Clone)]
enum DeclKind {
    Var,
    Const,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            assignments: 0.,
            assignments_sum: 0.,
            assignments_min: f64::MAX,
            assignments_max: 0.,
            branches: 0.,
            branches_sum: 0.,
            branches_min: f64::MAX,
            branches_max: 0.,
            conditions: 0.,
            conditions_sum: 0.,
            conditions_min: f64::MAX,
            conditions_max: 0.,
            space_count: 1,
            declaration: Vec::new(),
        }
    }
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("abc", 13)?;
        st.serialize_field("assignments", &self.assignments_sum())?;
        st.serialize_field("branches", &self.branches_sum())?;
        st.serialize_field("conditions", &self.conditions_sum())?;
        st.serialize_field("magnitude", &self.magnitude_sum())?;
        st.serialize_field("assignments_average", &self.assignments_average())?;
        st.serialize_field("branches_average", &self.branches_average())?;
        st.serialize_field("conditions_average", &self.conditions_average())?;
        st.serialize_field("assignments_min", &self.assignments_min())?;
        st.serialize_field("assignments_max", &self.assignments_max())?;
        st.serialize_field("branches_min", &self.branches_min())?;
        st.serialize_field("branches_max", &self.branches_max())?;
        st.serialize_field("conditions_min", &self.conditions_min())?;
        st.serialize_field("conditions_max", &self.conditions_max())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "assignments: {}, branches: {}, conditions: {}, magnitude: {}, \
            assignments_average: {}, branches_average: {}, conditions_average: {}, \
            assignments_min: {}, assignments_max: {}, \
            branches_min: {}, branches_max: {}, \
            conditions_min: {}, conditions_max: {}",
            self.assignments_sum(),
            self.branches_sum(),
            self.conditions_sum(),
            self.magnitude_sum(),
            self.assignments_average(),
            self.branches_average(),
            self.conditions_average(),
            self.assignments_min(),
            self.assignments_max(),
            self.branches_min(),
            self.branches_max(),
            self.conditions_min(),
            self.conditions_max()
        )
    }
}

impl Stats {
    /// Merges a second `Abc` metric into the first one.
    pub fn merge(&mut self, other: &Stats) {
        // Calculates minimum and maximum values
        self.assignments_min = self.assignments_min.min(other.assignments_min);
        self.assignments_max = self.assignments_max.max(other.assignments_max);
        self.branches_min = self.branches_min.min(other.branches_min);
        self.branches_max = self.branches_max.max(other.branches_max);
        self.conditions_min = self.conditions_min.min(other.conditions_min);
        self.conditions_max = self.conditions_max.max(other.conditions_max);

        self.assignments_sum += other.assignments_sum;
        self.branches_sum += other.branches_sum;
        self.conditions_sum += other.conditions_sum;

        self.space_count += other.space_count;
    }

    /// Returns the `Abc` assignments metric value.
    #[must_use]
    pub fn assignments(&self) -> f64 {
        self.assignments
    }

    /// Returns the `Abc` assignments sum metric value.
    #[must_use]
    pub fn assignments_sum(&self) -> f64 {
        self.assignments_sum
    }

    /// Returns the `Abc` assignments average value.
    ///
    /// This value is computed dividing the `Abc`
    /// assignments value for the number of spaces.
    #[must_use]
    pub fn assignments_average(&self) -> f64 {
        self.assignments_sum() / self.space_count as f64
    }

    /// Returns the `Abc` assignments minimum value.
    #[must_use]
    pub fn assignments_min(&self) -> f64 {
        self.assignments_min
    }

    /// Returns the `Abc` assignments maximum value.
    #[must_use]
    pub fn assignments_max(&self) -> f64 {
        self.assignments_max
    }

    /// Returns the `Abc` branches metric value.
    #[must_use]
    pub fn branches(&self) -> f64 {
        self.branches
    }

    /// Returns the `Abc` branches sum metric value.
    #[must_use]
    pub fn branches_sum(&self) -> f64 {
        self.branches_sum
    }

    /// Returns the `Abc` branches average value.
    ///
    /// This value is computed dividing the `Abc`
    /// branches value for the number of spaces.
    #[must_use]
    pub fn branches_average(&self) -> f64 {
        self.branches_sum() / self.space_count as f64
    }

    /// Returns the `Abc` branches minimum value.
    #[must_use]
    pub fn branches_min(&self) -> f64 {
        self.branches_min
    }

    /// Returns the `Abc` branches maximum value.
    #[must_use]
    pub fn branches_max(&self) -> f64 {
        self.branches_max
    }

    /// Returns the `Abc` conditions metric value.
    #[must_use]
    pub fn conditions(&self) -> f64 {
        self.conditions
    }

    /// Returns the `Abc` conditions sum metric value.
    #[must_use]
    pub fn conditions_sum(&self) -> f64 {
        self.conditions_sum
    }

    /// Returns the `Abc` conditions average value.
    ///
    /// This value is computed dividing the `Abc`
    /// conditions value for the number of spaces.
    #[must_use]
    pub fn conditions_average(&self) -> f64 {
        self.conditions_sum() / self.space_count as f64
    }

    /// Returns the `Abc` conditions minimum value.
    #[must_use]
    pub fn conditions_min(&self) -> f64 {
        self.conditions_min
    }

    /// Returns the `Abc` conditions maximum value.
    #[must_use]
    pub fn conditions_max(&self) -> f64 {
        self.conditions_max
    }

    /// Returns the `Abc` magnitude metric value.
    #[must_use]
    pub fn magnitude(&self) -> f64 {
        (self.assignments.powi(2) + self.branches.powi(2) + self.conditions.powi(2)).sqrt()
    }

    /// Returns the `Abc` magnitude sum metric value.
    #[must_use]
    pub fn magnitude_sum(&self) -> f64 {
        (self.assignments_sum.powi(2) + self.branches_sum.powi(2) + self.conditions_sum.powi(2))
            .sqrt()
    }

    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.assignments_sum += self.assignments;
        self.branches_sum += self.branches;
        self.conditions_sum += self.conditions;
    }

    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        self.assignments_min = self.assignments_min.min(self.assignments);
        self.assignments_max = self.assignments_max.max(self.assignments);
        self.branches_min = self.branches_min.min(self.branches);
        self.branches_max = self.branches_max.max(self.branches);
        self.conditions_min = self.conditions_min.min(self.conditions);
        self.conditions_max = self.conditions_max.max(self.conditions);
        self.compute_sum();
    }
}

/// Per-language computation of the ABC metric.
pub trait Abc
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    fn compute(node: &Node, stats: &mut Stats);
}

// Inspects the content of Java parenthesized expressions
// and `Not` operators to find unary conditional expressions
fn java_inspect_container(container_node: &Node, conditions: &mut f64) {
    use Java::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();

    // Initializes the flag to true if the container is known to contain a boolean value
    let Some(parent) = node.parent() else { return };
    let mut has_boolean_content = match parent.kind_id().into() {
        BinaryExpression | IfStatement | WhileStatement | DoStatement | ForStatement => true,
        TernaryExpression => node
            .previous_sibling()
            .is_none_or(|prev_node| !matches!(prev_node.kind_id().into(), QMARK | COLON)),
        _ => false,
    };

    // Looks inside parenthesized expressions and `Not` operators to find what they contain
    loop {
        // Checks if the node is a parenthesized expression or a `Not` operator
        // The child node of index 0 contains the unary expression operator (we look for the `!` operator)
        let is_parenthesised_exp = matches!(node_kind, ParenthesizedExpression);
        let is_not_operator = matches!(node_kind, UnaryExpression)
            && node
                .child(0)
                .is_some_and(|c| matches!(c.kind_id().into(), BANG));

        // Stops the exploration if the node is neither
        // a parenthesized expression nor a `Not` operator
        if !is_parenthesised_exp && !is_not_operator {
            break;
        }

        // Sets the flag to true if a `Not` operator is found
        // This is used to prove if a variable or a value returned by a method is actually boolean
        // e.g. `return (!x);`
        if !has_boolean_content && is_not_operator {
            has_boolean_content = true;
        }

        // Parenthesized expressions and `Not` operators nodes
        // always store their expressions in the children nodes of index one
        // https://github.com/tree-sitter/tree-sitter-java/blob/master/src/grammar.json#L2472
        // https://github.com/tree-sitter/tree-sitter-java/blob/master/src/grammar.json#L2150
        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        // Stops the exploration when the content is found
        if matches!(node_kind, MethodInvocation | Identifier | True | False) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

// C# analogue of `java_inspect_container`: walks parenthesised expressions
// and `!` (PrefixUnaryExpression) wrappers to surface a unary boolean
// condition contained within.
fn csharp_inspect_container(container_node: &Node, conditions: &mut f64) {
    use Csharp::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();

    // Seed the boolean-context flag from the parent: known-boolean
    // contexts (loop / if / binary expression) imply the contained
    // expression evaluates as a condition.
    let Some(parent) = node.parent() else { return };
    let mut has_boolean_content = match parent.kind_id().into() {
        BinaryExpression | IfStatement | WhileStatement | DoStatement | ForStatement => true,
        ConditionalExpression => node
            .previous_sibling()
            .is_none_or(|prev| !matches!(prev.kind_id().into(), QMARK | COLON)),
        _ => false,
    };

    // Walk down through `(...)` and `!...` wrappers until we either hit
    // the underlying operand or run out of nesting. The C# grammar
    // aliases each of these kinds across multiple `kind_id`s
    // (lesson #2): match every numbered variant.
    loop {
        let is_parens = matches!(node_kind, csharp_paren_expr_kinds!());
        let is_not = matches!(node_kind, csharp_prefix_unary_expr_kinds!())
            && node
                .child(0)
                .is_some_and(|c| matches!(c.kind_id().into(), BANG));

        if !is_parens && !is_not {
            break;
        }

        // A `!` wrapper proves the contained value is boolean even
        // when the parent context didn't (e.g. `return !x;`).
        if !has_boolean_content && is_not {
            has_boolean_content = true;
        }

        // Both `parenthesized_expression` and `prefix_unary_expression`
        // store their inner expression at child index 1.
        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        // Found the innermost operand; count it if a boolean context
        // was established up the chain.
        if matches!(
            node_kind,
            crate::Csharp::InvocationExpression
                | crate::Csharp::InvocationExpression2
                | crate::Csharp::InvocationExpression3
                | Identifier
                | True
                | False
        ) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

fn csharp_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Csharp::*;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(
                node_kind,
                crate::Csharp::InvocationExpression
                    | crate::Csharp::InvocationExpression2
                    | crate::Csharp::InvocationExpression3
                    | Identifier
                    | True
                    | False
            ) && matches!(list_kind, BinaryExpression)
                && !matches!(list_kind, ArgumentList)
            {
                *conditions += 1.;
            } else {
                csharp_inspect_container(&node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// Inspects a list of elements and counts any unary conditional expression found
fn java_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Java::*;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    // Scans the immediate children nodes of the argument node
    if cursor.goto_first_child() {
        loop {
            // Gets the current child node and its kind
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            // Checks if the node is a unary condition
            if matches!(node_kind, MethodInvocation | Identifier | True | False)
                && matches!(list_kind, BinaryExpression)
                && !matches!(list_kind, ArgumentList)
            {
                *conditions += 1.;
            } else {
                // Checks if the node is a unary condition container
                java_inspect_container(&node, conditions);
            }

            // Moves the cursor to the next sibling node of the current node
            // Exits the scan if there is no next sibling node
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

implement_metric_trait!(
    Abc,
    PythonCode,
    MozjsCode,
    JavascriptCode,
    RustCode,
    CppCode,
    PreprocCode,
    CcommentCode,
    GoCode,
    PerlCode,
    LuaCode,
    TclCode
);

// TypeScript / TSX share the same expression / statement vocabulary; the
// `ts_abc_compute!` macro expands the same token-level Fitzpatrick rules
// for both. Compared with the Java / C# impls we stay at the leaf-token
// level rather than walking parenthesised / unary containers — TS source
// rarely uses C-style `if (x)` conditions, so the
// "unary-boolean-in-a-container" heuristic adds noise without catching
// many real conditions. Conditions still capture every comparison and
// control-flow arm.
//
// Declaration sentinel: `lexical_declaration` and `variable_declaration`
// push a `Var` sentinel that suppresses counting the initializer `=` as
// an assignment. The `Const` token promotes to `Const` (compile-time
// constant — initializer is not a mutable assignment). `let` and `var`
// keep the `Var` slot. Augmented assignments (`+=`) and update
// expressions (`++`, `--`) always count.
macro_rules! ts_abc_compute {
    ($lang:ident) => {
        fn compute(node: &Node, stats: &mut Stats) {
            use $lang::*;

            match node.kind_id().into() {
                // Augmented assignments and pre/post increment/decrement
                // always count.
                PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ | STARSTAREQ | AMPEQ | PIPEEQ
                | CARETEQ | LTLTEQ | GTGTEQ | GTGTGTEQ | AMPAMPEQ | PIPEPIPEEQ | QMARKQMARKEQ
                | PLUSPLUS | DASHDASH => {
                    stats.assignments += 1.;
                }
                // Variable declarations push a `Var` sentinel; the `Const`
                // keyword promotes the top to `Const` so the initializer
                // `=` is treated as a constant binding.
                LexicalDeclaration | VariableDeclaration => {
                    stats.declaration.push(DeclKind::Var);
                }
                Const => {
                    if let Some(DeclKind::Var) = stats.declaration.last() {
                        stats.declaration.push(DeclKind::Const);
                    }
                }
                SEMI => {
                    if let Some(DeclKind::Const | DeclKind::Var) = stats.declaration.last() {
                        stats.declaration.clear();
                    }
                }
                // Plain `=` outside `const` declarations is an assignment.
                EQ if !matches!(stats.declaration.last(), Some(DeclKind::Const)) => {
                    stats.assignments += 1.;
                }
                // Function invocation and object construction count as
                // branches. Member calls and chained calls all surface
                // as `CallExpression`.
                CallExpression | NewExpression => {
                    stats.branches += 1.;
                }
                // Comparison and equality operators, ternary `?`, `??`,
                // `instanceof`, `else`, `case`, `default`, `catch`,
                // `try`.
                EQEQ | EQEQEQ | BANGEQ | BANGEQEQ | LTEQ | GTEQ | QMARK | QMARKQMARK
                | Instanceof | Else | Case | Default | Try | Catch => {
                    stats.conditions += 1.;
                }
                // `<` and `>` may also delimit type arguments / type
                // parameters (`Array<number>`, `class Foo<T> {}`); skip
                // those, count only comparison usage.
                GT | LT
                    if node.parent().is_some_and(|p| {
                        !matches!(p.kind_id().into(), TypeArguments | TypeParameters)
                    }) =>
                {
                    stats.conditions += 1.;
                }
                _ => {}
            }
        }
    };
}

impl Abc for TypescriptCode {
    ts_abc_compute!(Typescript);
}

impl Abc for TsxCode {
    ts_abc_compute!(Tsx);
}

// Fitzpatrick's ABC rules adapted for Kotlin syntax. Kotlin shares the
// JVM and Java's spec roots: assignments count once per `=` / augmented
// assignment / ++ / --, branches count once per function invocation or
// object construction, conditions count comparison operators plus the
// `else` / `when`-entry / `catch` arms. Compared with the Java impl we
// stay token-level (matching the leaf kind_ids) rather than walking
// `Modifiers` children; the Kotlin grammar exposes the relevant
// operators directly as token nodes inside `binary_expression`,
// `assignment`, `prefix_expression`, and `postfix_expression`.
impl Abc for KotlinCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Kotlin::*;

        match node.kind_id().into() {
            // Property / local-variable declaration and primary-constructor
            // parameter property (`class C(val a: Int = 5)`) both push a
            // sentinel so the `=` operator initialising the binding is NOT
            // counted as a standalone assignment (Fitzpatrick:
            // "initialisation is part of the declaration", mirroring Java).
            // The `Val` keyword arm below promotes the sentinel to `Const`
            // for immutable bindings.
            PropertyDeclaration | ClassParameter => {
                stats.declaration.push(DeclKind::Var);
            }
            // `val` introduces an immutable binding; promote the pending
            // declaration to `Const` so the upcoming `=` is suppressed
            // (constants are not assignments in ABC).
            Val => {
                if let Some(DeclKind::Var) = stats.declaration.last() {
                    stats.declaration.push(DeclKind::Const);
                }
            }
            // Statement terminator: the grammar emits an explicit `SEMI`
            // only for explicit semicolons. Property declarations also
            // terminate without one when the next token starts a new
            // statement. We clear the sentinel on the explicit `SEMI`
            // here; the implicit-terminator case is benign because the
            // EQ arm reads only `last()`, which is the most recently
            // pushed sentinel — any older entries left on the stack
            // from preceding implicit terminators do not affect the
            // assignment count.
            SEMI => {
                if let Some(DeclKind::Const | DeclKind::Var) = stats.declaration.last() {
                    stats.declaration.clear();
                }
            }
            // Augmented assignments and pre/post increment-decrement
            // always count, regardless of declaration context.
            PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ | PLUSPLUS | DASHDASH => {
                stats.assignments += 1.;
            }
            // Plain `=` token. Skip when inside a `val` declaration; count
            // when inside a `var` declaration (initialiser of mutable
            // binding) or a standalone `Assignment`. The DeclKind stack is
            // cleared at the property statement boundary above.
            EQ if stats
                .declaration
                .last()
                .is_none_or(|decl| matches!(decl, DeclKind::Var)) =>
            {
                stats.assignments += 1.;
            }
            // Branches: every call expression plus object construction.
            // Kotlin's `new` is implicit — `Foo()` parses as
            // `CallExpression` with a type-named receiver. The
            // Halstead-side classification treats it uniformly. Indexed
            // access (`arr[i]`) is NOT a branch (it's an operator on a
            // sequence), matching the Java rule of "method invocation
            // only".
            CallExpression => {
                stats.branches += 1.;
            }
            // Conditions: comparison operators, identity equality,
            // ternary-elvis (`?:`), `as?` safe-cast, and the arms of
            // control-flow constructs (`else`, `catch`, `when` entries).
            // Kotlin's `if`-expression does not need an extra count for
            // the `if` keyword itself — Fitzpatrick counts the
            // *conditions*, and the unary condition is already implicit
            // in the boolean operand. We add the `if` arm via the `Else`
            // keyword for else-branches and via `WhenEntry` for `when`.
            LTEQ | GTEQ | EQEQ | EQEQEQ | BANGEQ | BANGEQEQ | WhenEntry | CatchBlock
            | QMARKCOLON | AsQMARK => {
                stats.conditions += 1.;
            }
            // `else` is a keyword token used in both `if_expression`'s
            // else-clause and `when`'s `else ->` entry. Only count it
            // when it belongs to an `if_expression`; the `WhenEntry`
            // wrapper above already covers the `when` case.
            Else if node.parent().is_some_and(|p| p.kind_id() == IfExpression) => {
                stats.conditions += 1.;
            }
            // `<` and `>` may appear as type-argument brackets
            // (`List<Int>`); exclude those by checking the parent kind.
            LT | GT
                if node.parent().is_some_and(|p| {
                    !matches!(p.kind_id().into(), TypeArguments | TypeParameters)
                }) =>
            {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

impl Abc for PhpCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Php::*;

        match node.kind_id().into() {
            // Assignments: explicit assignment expressions and augmented forms,
            // plus pre/post increment and decrement. `const_declaration` and
            // `enum_case` use their own `const_element` / value-assignment
            // shapes, so they do not produce `AssignmentExpression` nodes —
            // matching the assignment-expression kinds naturally excludes
            // them.
            AssignmentExpression
            | AugmentedAssignmentExpression
            | ReferenceAssignmentExpression
            | PLUSPLUS
            | DASHDASH => {
                stats.assignments += 1.;
            }
            // Branches: every PHP call kind plus object construction.
            FunctionCallExpression
            | MemberCallExpression
            | ScopedCallExpression
            | NullsafeMemberCallExpression
            | ObjectCreationExpression => {
                stats.branches += 1.;
            }
            // Conditions: comparison and identity operators (anonymous tokens
            // inside `binary_expression`), `instanceof`, ternary `?`, and
            // control-flow arms.
            EQEQ
            | EQEQEQ
            | BANGEQ
            | BANGEQEQ
            | LT
            | GT
            | LTEQ
            | GTEQ
            | LTEQGT
            | LTGT
            | Instanceof
            | ConditionalExpression
            | ElseClause
            | ElseClause2
            | ElseIfClause
            | ElseIfClause2
            | CaseStatement
            | DefaultStatement
            | MatchConditionalExpression
            | MatchDefaultExpression
            | CatchClause => {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

impl Abc for BashCode {
    fn compute(node: &Node, stats: &mut Stats) {
        match node.kind_id().into() {
            // Each `variable_assignment` is one assignment regardless of
            // operator (`=`, `+=`, `-=`, …) — counting the parent node
            // avoids double-counting `Bash::EQ`, which is also produced
            // for the `=` inside `[ a = b ]` test expressions.
            Bash::VariableAssignment | Bash::VariableAssignment2 => {
                stats.assignments += 1.;
            }
            // Every command invocation is a branch in the ABC sense
            // (function-call / message-pass). `return` and `exit` builtins
            // are also `Bash::Command` nodes and count here too.
            Bash::Command => {
                stats.branches += 1.;
            }
            // Comparison operators inside `[[ … ]]` and `(( … ))`, plus
            // the prefix test operators `-z`, `-n`, `-eq`, `-lt`, … which
            // the grammar emits as `Bash::TestOperator`.
            Bash::EQEQ
            | Bash::BANGEQ
            | Bash::LT
            | Bash::GT
            | Bash::LTEQ
            | Bash::GTEQ
            | Bash::EQTILDE
            | Bash::TestOperator => {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

// Fitzpatrick, Jerry (1997). "Applying the ABC metric to C, C++ and Java". C++ Report.
// Source: https://www.softwarerenovation.com/Articles.aspx
// ABC Java rules: (page 8, figure 4)
// ABC Java example: (page 15, listing 4)
impl Abc for JavaCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Java::*;

        match node.kind_id().into() {
            STAREQ | SLASHEQ | PERCENTEQ | DASHEQ | PLUSEQ | LTLTEQ | GTGTEQ | AMPEQ | PIPEEQ
            | CARETEQ | GTGTGTEQ | PLUSPLUS | DASHDASH => {
                stats.assignments += 1.;
            }
            FieldDeclaration | LocalVariableDeclaration => {
                stats.declaration.push(DeclKind::Var);
            }
            Final => {
                if let Some(DeclKind::Var) = stats.declaration.last() {
                    stats.declaration.push(DeclKind::Const);
                }
            }
            SEMI => {
                if let Some(DeclKind::Const | DeclKind::Var) = stats.declaration.last() {
                    stats.declaration.clear();
                }
            }
            // Excludes constant declarations
            EQ if stats
                .declaration
                .last()
                .is_none_or(|decl| matches!(decl, DeclKind::Var)) =>
            {
                stats.assignments += 1.;
            }
            MethodInvocation | New => {
                stats.branches += 1.;
            }
            GTEQ | LTEQ | EQEQ | BANGEQ | Else | Case | Default | QMARK | Try | Catch => {
                stats.conditions += 1.;
            }
            GT | LT => {
                // Excludes `<` and `>` used for generic types
                if let Some(parent) = node.parent()
                    && !matches!(parent.kind_id().into(), TypeArguments)
                {
                    stats.conditions += 1.;
                }
            }
            // Counts unary conditions in elements separated by `&&` or `||` boolean operators
            AMPAMP | PIPEPIPE => {
                if let Some(parent) = node.parent() {
                    java_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            // Counts unary conditions among method arguments
            ArgumentList => {
                java_count_unary_conditions(node, &mut stats.conditions);
            }
            // Counts unary conditions inside assignments
            VariableDeclarator | AssignmentExpression => {
                // The child node of index 2 contains the right operand of an assignment operation
                if let Some(right_operand) = node.child(2)
                    && matches!(
                        right_operand.kind_id().into(),
                        ParenthesizedExpression | UnaryExpression
                    )
                {
                    java_inspect_container(&right_operand, &mut stats.conditions);
                }
            }
            // Counts unary conditions inside if and while statements
            IfStatement | WhileStatement => {
                // The child node of index 1 contains the condition
                if let Some(condition) = node.child(1)
                    && matches!(condition.kind_id().into(), ParenthesizedExpression)
                {
                    java_inspect_container(&condition, &mut stats.conditions);
                }
            }
            // Counts unary conditions do-while statements
            DoStatement => {
                // The child node of index 3 contains the condition
                if let Some(condition) = node.child(3)
                    && matches!(condition.kind_id().into(), ParenthesizedExpression)
                {
                    java_inspect_container(&condition, &mut stats.conditions);
                }
            }
            // Counts unary conditions inside for statements
            ForStatement => {
                // The child node of index 3 contains the `condition` when
                // the initialization expression is a variable declaration
                // e.g. `for ( int i=0; `condition`; ... ) {}`
                if let Some(condition) = node.child(3) {
                    match condition.kind_id().into() {
                        SEMI => {
                            // The child node of index 4 contains the `condition` when
                            // the initialization expression is not a variable declaration
                            // e.g. `for ( i=0; `condition`; ... ) {}`
                            if let Some(cond) = node.child(4) {
                                match cond.kind_id().into() {
                                    MethodInvocation | Identifier | True | False | SEMI
                                    | RPAREN => {
                                        stats.conditions += 1.;
                                    }
                                    ParenthesizedExpression | UnaryExpression => {
                                        java_inspect_container(&cond, &mut stats.conditions);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        MethodInvocation | Identifier | True | False => {
                            stats.conditions += 1.;
                        }
                        ParenthesizedExpression | UnaryExpression => {
                            java_inspect_container(&condition, &mut stats.conditions);
                        }
                        _ => {}
                    }
                }
            }
            // Counts unary conditions inside return statements
            ReturnStatement => {
                // The child node of index 1 contains the return value
                if let Some(value) = node.child(1)
                    && matches!(
                        value.kind_id().into(),
                        ParenthesizedExpression | UnaryExpression
                    )
                {
                    java_inspect_container(&value, &mut stats.conditions);
                }
            }
            // Counts unary conditions inside implicit return statements in lambda expressions
            LambdaExpression => {
                // The child node of index 2 contains the return value
                if let Some(value) = node.child(2)
                    && matches!(
                        value.kind_id().into(),
                        ParenthesizedExpression | UnaryExpression
                    )
                {
                    java_inspect_container(&value, &mut stats.conditions);
                }
            }
            // Counts unary conditions inside ternary expressions
            TernaryExpression => {
                // The child node of index 0 contains the condition
                if let Some(condition) = node.child(0) {
                    match condition.kind_id().into() {
                        MethodInvocation | Identifier | True | False => {
                            stats.conditions += 1.;
                        }
                        ParenthesizedExpression | UnaryExpression => {
                            java_inspect_container(&condition, &mut stats.conditions);
                        }
                        _ => {}
                    }
                }
                // The child node of index 2 contains the first expression
                if let Some(expression) = node.child(2)
                    && matches!(
                        expression.kind_id().into(),
                        ParenthesizedExpression | UnaryExpression
                    )
                {
                    java_inspect_container(&expression, &mut stats.conditions);
                }
                // The child node of index 4 contains the second expression
                if let Some(expression) = node.child(4)
                    && matches!(
                        expression.kind_id().into(),
                        ParenthesizedExpression | UnaryExpression
                    )
                {
                    java_inspect_container(&expression, &mut stats.conditions);
                }
            }
            _ => {}
        }
    }
}

impl Abc for CsharpCode {
    fn compute(node: &Node, stats: &mut Stats) {
        use Csharp::*;

        match node.kind_id().into() {
            STAREQ | SLASHEQ | PERCENTEQ | DASHEQ | PLUSEQ | LTLTEQ | GTGTEQ | GTGTGTEQ | AMPEQ
            | PIPEEQ | CARETEQ | QMARKQMARKEQ | PLUSPLUS | DASHDASH => {
                stats.assignments += 1.;
            }
            FieldDeclaration | LocalDeclarationStatement => {
                stats.declaration.push(DeclKind::Var);
            }
            // C# `const` modifier marks a compile-time constant — exclude
            // its initializer from the assignment count (matches Java's
            // treatment of `final`).
            Const => {
                if let Some(DeclKind::Var) = stats.declaration.last() {
                    stats.declaration.push(DeclKind::Const);
                }
            }
            SEMI => {
                if let Some(DeclKind::Const | DeclKind::Var) = stats.declaration.last() {
                    stats.declaration.clear();
                }
            }
            // Count `=` as an assignment unless it's the initializer of a
            // `const` declaration (those are constant bindings, not mutable
            // assignments). `None` means we're outside any declaration —
            // still count.
            EQ if !matches!(stats.declaration.last(), Some(DeclKind::Const)) => {
                stats.assignments += 1.;
            }
            crate::Csharp::InvocationExpression
            | crate::Csharp::InvocationExpression2
            | crate::Csharp::InvocationExpression3
            | ObjectCreationExpression => {
                stats.branches += 1.;
            }
            GTEQ | LTEQ | EQEQ | BANGEQ | Else | Case | Default | QMARK | Try | Catch => {
                stats.conditions += 1.;
            }
            GT | LT => {
                // Excludes `<` and `>` used as type-syntax delimiters:
                // generic type arguments (`Dictionary<K, V>`), type
                // parameter declarations (`class Foo<T> { }`), and the
                // parameter-list delimiters of unsafe function-pointer
                // types (`delegate*<int, int>`).
                if let Some(parent) = node.parent()
                    && !matches!(
                        parent.kind_id().into(),
                        TypeArgumentList | TypeParameterList | FunctionPointerType
                    )
                {
                    stats.conditions += 1.;
                }
            }
            AMPAMP | PIPEPIPE => {
                if let Some(parent) = node.parent() {
                    csharp_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            ArgumentList => {
                csharp_count_unary_conditions(node, &mut stats.conditions);
            }
            crate::Csharp::VariableDeclarator
            | crate::Csharp::VariableDeclarator2
            | AssignmentExpression => {
                // Child 2 is the RHS of `lhs = rhs`.
                inspect_csharp_child(node, 2, &mut stats.conditions);
            }
            IfStatement | WhileStatement => {
                // Child 1 is the parenthesised condition: `if (cond) ...`.
                if let Some(condition) = node.child(1)
                    && matches!(condition.kind_id().into(), csharp_paren_expr_kinds!())
                {
                    csharp_inspect_container(&condition, &mut stats.conditions);
                }
            }
            DoStatement => {
                // `do { ... } while (cond);` — condition sits at child 3
                // (children: `do`, body, `while`, `(cond)`, `;`).
                if let Some(condition) = node.child(3)
                    && matches!(condition.kind_id().into(), csharp_paren_expr_kinds!())
                {
                    csharp_inspect_container(&condition, &mut stats.conditions);
                }
            }
            ReturnStatement => {
                // Child 1 is the returned expression (child 0 is `return`).
                inspect_csharp_child(node, 1, &mut stats.conditions);
            }
            LambdaExpression => {
                // Child 2 is the lambda body for `params => body`.
                inspect_csharp_child(node, 2, &mut stats.conditions);
            }
            ConditionalExpression => {
                // `cond ? a : b` — children are [cond, ?, a, :, b].
                if let Some(condition) = node.child(0) {
                    match condition.kind_id().into() {
                        crate::Csharp::InvocationExpression
                        | crate::Csharp::InvocationExpression2
                        | crate::Csharp::InvocationExpression3
                        | Identifier
                        | True
                        | False => {
                            stats.conditions += 1.;
                        }
                        crate::Csharp::ParenthesizedExpression
                        | crate::Csharp::ParenthesizedExpression2
                        | crate::Csharp::ParenthesizedExpression3
                        | crate::Csharp::PrefixUnaryExpression
                        | crate::Csharp::PrefixUnaryExpression2 => {
                            csharp_inspect_container(&condition, &mut stats.conditions);
                        }
                        _ => {}
                    }
                }
                inspect_csharp_child(node, 2, &mut stats.conditions);
                inspect_csharp_child(node, 4, &mut stats.conditions);
            }
            // NOTE: Java's Abc impl has an explicit `ForStatement` arm to
            // count single-token (Identifier / InvocationExpression / True
            // / False) for-loop conditions. The C# grammar wraps for-loop
            // conditions in `_for_statement_conditions` rather than at
            // direct child positions, so a port of that arm requires
            // grammar inspection. Conditions using comparison operators
            // (`<`, `==`, etc.) are still counted by the standard
            // `GT | LT | ...` arms. See issue tracker for the gap.
            _ => {}
        }
    }
}

// Shared helper: if `node.child(idx)` is a parenthesised or `!`-prefixed
// expression, descend into it to count any unary boolean condition.
// Used by every C# Abc match arm whose condition sits at a known child
// index (assignments, returns, lambdas, ternaries).
fn inspect_csharp_child(node: &Node, idx: usize, conditions: &mut f64) {
    if let Some(child) = node.child(idx)
        && matches!(
            child.kind_id().into(),
            crate::Csharp::ParenthesizedExpression
                | crate::Csharp::ParenthesizedExpression2
                | crate::Csharp::ParenthesizedExpression3
                | crate::Csharp::PrefixUnaryExpression
                | crate::Csharp::PrefixUnaryExpression2
        )
    {
        csharp_inspect_container(&child, conditions);
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

    // Regression test for the `EQ` arm guard in `JavaCode::compute`:
    // the rewrite from `.map().unwrap_or_else()` to
    // `is_none_or(|decl| matches!(decl, DeclKind::Var))` must preserve
    // the three-way truth table — None → ++, Some(Var) → ++,
    // Some(Const) → no-op.
    #[test]
    fn java_eq_arm_increments_when_declaration_stack_is_empty() {
        // No surrounding `int x = ...` / `Final` token → declaration
        // stack is empty when the `EQ` token is visited, so the None
        // branch must increment `assignments`.
        check_metrics::<JavaParser>(
            "class A { void m() { int x = 0; x = 1; x = 2; x = 3; } }",
            "foo.java",
            |metric| {
                // `int x = 0;` adds 1 (Some(Var) branch),
                // each subsequent `x = N;` adds 1 (None branch).
                assert_eq!(metric.abc.assignments_sum(), 4.0);
            },
        );
    }

    #[test]
    fn java_eq_arm_skips_when_declaration_stack_top_is_const() {
        // `final` pushes `DeclKind::Const` on top of the active `Var`
        // entry, so the Some(non-Var) branch must skip the increment.
        check_metrics::<JavaParser>(
            "class A {
                final int X = 1;
                final int Y = 2;
                void m() { final int Z = 3; }
            }",
            "foo.java",
            |metric| {
                // All three `=` tokens land under a `Const` top, so
                // assignments should be 0 across all spaces.
                assert_eq!(metric.abc.assignments_sum(), 0.0);
            },
        );
    }

    // Constant declarations are not counted as assignments
    #[test]
    fn java_constant_declarations() {
        check_metrics::<JavaParser>(
            "class A {
                private final int X1 = 0, Y1 = 0;
                public final float PI = 3.14f;
                final static String HELLO = \"Hello,\";
                protected String world = \" world!\";   // +1a
                public float e = 2.718f;                // +1a
                private int x2 = 1, y2 = 2;             // +2a

                void m() {
                    final int Z1 = 0, Z2 = 0, Z3 = 0;
                    final float T = 0.0f;
                    int z1 = 1, z2 = 2, z3 = 3;         // +3a
                    float t = 60.0f;                    // +1a
                }
            }",
            "foo.java",
            |metric| {
                // magnitude: sqrt(64 + 0 + 0) = sqrt(64)
                // space count: 3 (1 unit, 1 class and 1 method)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 8.0,
                      "branches": 0.0,
                      "conditions": 0.0,
                      "magnitude": 8.0,
                      "assignments_average": 2.6666666666666665,
                      "branches_average": 0.0,
                      "conditions_average": 0.0,
                      "assignments_min": 0.0,
                      "assignments_max": 4.0,
                      "branches_min": 0.0,
                      "branches_max": 0.0,
                      "conditions_min": 0.0,
                      "conditions_max": 0.0
                    }"###
                );
            },
        );
    }

    // "In computer science, conditionals (that is, conditional statements, conditional expressions
    // and conditional constructs,) are programming language commands for handling decisions."
    // Source: https://en.wikipedia.org/wiki/Conditional_(computer_programming)
    // According to this definition, boolean expressions that are evaluated to make a decision are considered as conditions
    // Variables, method invocations and true or false values used inside
    // variable declarations and assignment expressions are not counted as conditions
    #[test]
    fn java_declarations_with_conditions() {
        check_metrics::<JavaParser>(
            "
            boolean a = (1 > 2);            // +1a +1c
            boolean b = 3 > 4;              // +1a +1c
            boolean c = (1 > 2) && 3 > 4;   // +1a +2c
            boolean d = b && (x > 5) || c;  // +1a +3c
            boolean e = !d;                 // +1a +1c
            boolean f = ((!false));         // +1a +1c
            boolean g = !(!(true));         // +1a +1c
            boolean h = true;               // +1a
            boolean i = (false);            // +1a
            boolean j = (((((true)))));     // +1a
            boolean k = (((((m())))));      // +1a +1b
            boolean l = (((((!m())))));     // +1a +1b +1c
            boolean m = (!(!((m()))));      // +1a +1b +1c
            List<String> n = null;          // +1a (< and > used for generic types are not counted as conditions)
            ",
            "foo.java",
          |metric| {
                // magnitude: sqrt(196 + 9 + 144) = sqrt(349)
                // space count: 1 (1 unit)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 14.0,
                      "branches": 3.0,
                      "conditions": 12.0,
                      "magnitude": 18.681541692269406,
                      "assignments_average": 14.0,
                      "branches_average": 3.0,
                      "conditions_average": 12.0,
                      "assignments_min": 14.0,
                      "assignments_max": 14.0,
                      "branches_min": 3.0,
                      "branches_max": 3.0,
                      "conditions_min": 12.0,
                      "conditions_max": 12.0
                    }"###
                );
            },
        );
    }

    // Conditions can be found in assignment expressions
    #[test]
    fn java_assignments_with_conditions() {
        check_metrics::<JavaParser>(
            "
            a = 2 < 1;                  // +1a +1c
            b = (4 >= 3) && 2 <= 1;     // +1a +2c
            c = a || (x != 10) && b;    // +1a +3c
            d = !false;                 // +1a +1c
            e = (!false);               // +1a +1c
            f = !(false);               // +1a +1c
            g = (!(((true))));          // +1a +1c
            h = ((true));               // +1a
            i = !m();                   // +1a +1b +1c
            j = !((m()));               // +1a +1b +1c
            k = (!(m()));               // +1a +1b +1c
            l = ((!(m())));             // +1a +1b +1c
            m = !B.<Integer>m(2);       // +1a +1b +1c
            n = !((B.<Integer>m(4)));   // +1a +1b +1c
            ",
            "foo.java",
            |metric| {
                // magnitude: sqrt(196 + 36 + 256) = sqrt(488)
                // space count: 1 (1 unit)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 14.0,
                      "branches": 6.0,
                      "conditions": 16.0,
                      "magnitude": 22.090722034374522,
                      "assignments_average": 14.0,
                      "branches_average": 6.0,
                      "conditions_average": 16.0,
                      "assignments_min": 14.0,
                      "assignments_max": 14.0,
                      "branches_min": 6.0,
                      "branches_max": 6.0,
                      "conditions_min": 16.0,
                      "conditions_max": 16.0
                    }"###
                );
            },
        );
    }

    // Conditions can be found in method arguments
    #[test]
    fn java_methods_arguments_with_conditions() {
        check_metrics::<JavaParser>(
            "
            m1(a);                                  // +1b
            m2(a, b);                               // +1b
            m3(true, (false), (((true))));          // +1b
            m3(m1(false), m1(true), m1(false));     // +4b
            m1(!a);                                 // +1b +1c
            m2((((a))), (!b));                      // +1b +1c
            m3(!(a), b, !!!c);                      // +1b +2c
            m3(a, !b, m2(!a, !m2(!b, !m1(!c))));    // +4b +6c
            ",
            "foo.java",
            |metric| {
                // magnitude: sqrt(196 + 36 + 256) = sqrt(488)
                // space count: 1 (1 unit)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 0.0,
                      "branches": 14.0,
                      "conditions": 10.0,
                      "magnitude": 17.204650534085253,
                      "assignments_average": 0.0,
                      "branches_average": 14.0,
                      "conditions_average": 10.0,
                      "assignments_min": 0.0,
                      "assignments_max": 0.0,
                      "branches_min": 14.0,
                      "branches_max": 14.0,
                      "conditions_min": 10.0,
                      "conditions_max": 10.0
                    }"###
                );
            },
        );
    }

    // "A unary conditional expression is an implicit condition that uses no relational operators."
    // Source: Fitzpatrick, Jerry (1997). "Applying the ABC metric to C, C++ and Java". C++ Report.
    // https://www.softwarerenovation.com/Articles.aspx (page 5)
    #[test]
    fn java_if_single_conditions() {
        check_metrics::<JavaParser>(
            "
            if ( a < 0 ) {}             // +1c
            if ( ((a != 0)) ) {}        // +1c
            if ( !(a > 0) ) {}          // +1c
            if ( !(((a == 0))) ) {}     // +1c
            if ( b.m1() ) {}            // +1b +1c
            if ( !b.m1() ) {}           // +1b +1c
            if ( !!b.m2() ) {}          // +1b +1c
            if ( (!(b.m1())) ) {}       // +1b +1c
            if ( (!(!b.m1())) ) {}      // +1b +1c
            if ( ((b.m2())) ) {}        // +1b +1c
            if ( ((b.m().m1())) ) {}    // +2b +1c
            if ( c ) {}                 // +1c
            if ( !c ) {}                // +1c
            if ( !!!!!!!!!!c ) {}       // +1c
            if ( (((c))) ) {}           // +1c
            if ( (((!c))) ) {}          // +1c
            if ( ((!(c))) ) {}          // +1c
            if ( true ) {}              // +1c
            if ( !true ) {}             // +1c
            if ( ((false)) ) {}         // +1c
            if ( !(!(false)) ) {}       // +1c
            if ( !!!false ) {}          // +1c
            ",
            "foo.java",
            |metric| {
                // magnitude: sqrt(0 + 64 + 484) = sqrt(548)
                // space count: 1 (1 unit)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 0.0,
                      "branches": 8.0,
                      "conditions": 22.0,
                      "magnitude": 23.40939982143925,
                      "assignments_average": 0.0,
                      "branches_average": 8.0,
                      "conditions_average": 22.0,
                      "assignments_min": 0.0,
                      "assignments_max": 0.0,
                      "branches_min": 8.0,
                      "branches_max": 8.0,
                      "conditions_min": 22.0,
                      "conditions_max": 22.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_if_multiple_conditions() {
        check_metrics::<JavaParser>(
            "
            if ( a || b || c || d ) {}              // +4c
            if ( a || b && c && d ) {}              // +4c
            if ( x < y && a == b ) {}               // +2c
            if ( ((z < (x + y))) ) {}               // +1c
            if ( a || ((((b))) && c) ) {}           // +3c
            if ( a && ((((a == b))) && c) ) {}      // +3c
            if ( a || ((((a == b))) || ((c))) ) {}  // +3c
            if ( x < y && B.m() ) {}                // +1b +2c
            if ( x < y && !(((B.m()))) ) {}         // +1b +2c
            if ( !(x < y) && !B.m() ) {}            // +1b +2c
            if ( !!!(!!!(a)) && B.m() ||            // +1b +2c
                 !B.m() && (((x > 4))) ) {}         // +1b +2c
            ",
            "foo.java",
            |metric| {
                // magnitude: sqrt(0 + 25 + 900) = sqrt(925)
                // space count: 1 (1 unit)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 0.0,
                      "branches": 5.0,
                      "conditions": 30.0,
                      "magnitude": 30.4138126514911,
                      "assignments_average": 0.0,
                      "branches_average": 5.0,
                      "conditions_average": 30.0,
                      "assignments_min": 0.0,
                      "assignments_max": 0.0,
                      "branches_min": 5.0,
                      "branches_max": 5.0,
                      "conditions_min": 30.0,
                      "conditions_max": 30.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_while_and_do_while_conditions() {
        check_metrics::<JavaParser>(
            "
            while ( (!(!(!(a)))) ) {}                   // +1c
            while ( b || 1 > 2 ) {}                     // +2c
            while ( x.m() && (((c))) ) {}               // +1b +2c
            do {} while ( !!!(((!!!a))) );              // +1c
            do {} while ( a || (b && c) );              // +3c
            do {} while ( !x.m() && 1 > 2 || !true );   // +1b +3c
            ",
            "foo.java",
            |metric| {
                // magnitude: sqrt(0 + 4 + 144) = sqrt(148)
                // space count: 1 (1 unit)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 0.0,
                      "branches": 2.0,
                      "conditions": 12.0,
                      "magnitude": 12.165525060596439,
                      "assignments_average": 0.0,
                      "branches_average": 2.0,
                      "conditions_average": 12.0,
                      "assignments_min": 0.0,
                      "assignments_max": 0.0,
                      "branches_min": 2.0,
                      "branches_max": 2.0,
                      "conditions_min": 12.0,
                      "conditions_max": 12.0
                    }"###
                );
            },
        );
    }

    // GMetrics, a Groovy source code analyzer, provides the following definition of unary conditional expression:
    // "These are cases where a single variable/field/value is treated as a boolean value.
    // Examples include `if (x)` and `return !ready`."
    // According to this definition, unary conditional expressions are counted also in function return values.
    // Source: https://dx42.github.io/gmetrics/metrics/AbcMetric.html
    // Examples: https://github.com/dx42/gmetrics/blob/master/src/test/groovy/org/gmetrics/metric/abc/AbcMetric_MethodTest.groovy
    #[test]
    fn java_return_with_conditions() {
        check_metrics::<JavaParser>(
            "class A {
                boolean m1() {
                    return !(z >= 0);       // +1c
                }
                boolean m2() {
                    return (((!x)));        // +1c
                }
                boolean m3() {
                    return x && y;          // +2c
                }
                boolean m4() {
                    return y || (z < 0);    // +2c
                }
                boolean m5() {
                    return x || y ?         // +3c (two unary conditions and one ?)
                        true : false;
                }
            }",
            "foo.java",
            |metric| {
                // magnitude: sqrt(0 + 0 + 81) = sqrt(81)
                // space count: 7 (1 unit, 1 class and 5 methods)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 0.0,
                      "branches": 0.0,
                      "conditions": 9.0,
                      "magnitude": 9.0,
                      "assignments_average": 0.0,
                      "branches_average": 0.0,
                      "conditions_average": 1.2857142857142858,
                      "assignments_min": 0.0,
                      "assignments_max": 0.0,
                      "branches_min": 0.0,
                      "branches_max": 0.0,
                      "conditions_min": 0.0,
                      "conditions_max": 3.0
                    }"###
                );
            },
        );
    }

    // Variables, method invocations, and true or false values
    // inside return statements are not counted as conditions
    #[test]
    fn java_return_without_conditions() {
        check_metrics::<JavaParser>(
            "class A {
                boolean m1() {
                    return x;
                }
                boolean m2() {
                    return (x);
                }
                boolean m3() {
                    return y.m();   // +1b
                }
                boolean m4() {
                    return false;
                }
                void m5() {
                    return;
                }
            }",
            "foo.java",
            |metric| {
                // magnitude: sqrt(0 + 1 + 0) = sqrt(1)
                // space count: 7 (1 unit, 1 class and 5 methods)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 0.0,
                      "branches": 1.0,
                      "conditions": 0.0,
                      "magnitude": 1.0,
                      "assignments_average": 0.0,
                      "branches_average": 0.14285714285714285,
                      "conditions_average": 0.0,
                      "assignments_min": 0.0,
                      "assignments_max": 0.0,
                      "branches_min": 0.0,
                      "branches_max": 1.0,
                      "conditions_min": 0.0,
                      "conditions_max": 0.0
                    }"###
                );
            },
        );
    }

    // Variables, method invocations, and true or false values
    // in lambda expression return values are not counted as conditions
    #[test]
    fn java_lambda_expressions_return_with_conditions() {
        check_metrics::<JavaParser>(
            "
            Predicate<Boolean> p1 = a -> a;                         // +1a
            Predicate<Boolean> p2 = b -> true;                      // +1a
            Predicate<Boolean> p3 = c -> m();                       // +1a
            Predicate<Integer> p4 = d -> d > 10;                    // +1a +1c
            Predicate<Boolean> p5 = (e) -> !e;                      // +1a +1c
            Predicate<Boolean> p6 = (f) -> !((!f));                 // +1a +1c
            Predicate<Boolean> p7 = (g) -> !g && true;              // +1a +2c
            BiPredicate<Boolean, Boolean> bp1 = (h, i) -> !h && !i; // +1a +2c
            BiPredicate<Boolean, Boolean> bp2 = (j, k) -> {
                return j || k;                                      // +1a +2c
            };
            ",
            "foo.java",
            |metric| {
                // magnitude: sqrt(81 + 1 + 81) = sqrt(163)
                // space count: 1 (1 unit)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 9.0,
                      "branches": 1.0,
                      "conditions": 9.0,
                      "magnitude": 12.767145334803704,
                      "assignments_average": 9.0,
                      "branches_average": 1.0,
                      "conditions_average": 9.0,
                      "assignments_min": 9.0,
                      "assignments_max": 9.0,
                      "branches_min": 1.0,
                      "branches_max": 1.0,
                      "conditions_min": 9.0,
                      "conditions_max": 9.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_for_with_variable_declaration() {
        check_metrics::<JavaParser>(
            "
            for ( int i1 = 0; !(!(!(!a))); i1++ ) {}                // +2a +1c
            for ( int i2 = 0; !B.m(); i2++ ) {}                     // +2a +1b +1c
            for ( int i3 = 0; a || false; i3++ ) {}                 // +2a +2c
            for ( int i4 = 0; a && B.m() ? true : false; i4++ ) {}  // +2a +1b +3c
            for ( int i5 = 0; true; i5++ ) {}                       // +2a +1c
            ",
            "foo.java",
            |metric| {
                // magnitude: sqrt(100 + 4 + 64) = sqrt(168)
                // space count: 1 (1 unit)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 10.0,
                      "branches": 2.0,
                      "conditions": 8.0,
                      "magnitude": 12.96148139681572,
                      "assignments_average": 10.0,
                      "branches_average": 2.0,
                      "conditions_average": 8.0,
                      "assignments_min": 10.0,
                      "assignments_max": 10.0,
                      "branches_min": 2.0,
                      "branches_max": 2.0,
                      "conditions_min": 8.0,
                      "conditions_max": 8.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_for_without_variable_declaration() {
        check_metrics::<JavaParser>(
            "class A{
                void m1() {
                    for (i = 0; x < y; i++) {}          // +2a +1c
                    for (i = 0; ((x < y)); i++) {}      // +2a +1c
                    for (i = 0; !(!(x < y)); i++) {}    // +2a +1c
                    for (i = 0; true; i++) {}           // +2a +1c
                }
                void m2() {
                    for ( ; true; ) {}  // +1c
                }
                void m3() {
                    for ( ; ; ) {}      // +1c (one implicit unary condition set to true)
                }
            }",
            "foo.java",
            |metric| {
                // magnitude: sqrt(64 + 0 + 36) = sqrt(100)
                // space count: 5 (1 unit, 1 class and 3 methods)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 8.0,
                      "branches": 0.0,
                      "conditions": 6.0,
                      "magnitude": 10.0,
                      "assignments_average": 1.6,
                      "branches_average": 0.0,
                      "conditions_average": 1.2,
                      "assignments_min": 0.0,
                      "assignments_max": 8.0,
                      "branches_min": 0.0,
                      "branches_max": 0.0,
                      "conditions_min": 0.0,
                      "conditions_max": 4.0
                    }"###
                );
            },
        );
    }

    // Variables, method invocations, and true or false values
    // in ternary expression return values are not counted as conditions
    #[test]
    fn java_ternary_conditions() {
        check_metrics::<JavaParser>(
            "
            a = true;                                   // +1a
            b = a ? true : false;                       // +1a +2c
            c = ((((a)))) ? !false : !b;                // +1a +4c
            d = !this.m() ? !!a : (false);              // +1a +1b +3c
            e = !(a) && b ? ((c)) : !d;                 // +1a +4c
            if ( this.m() ? a : !this.m() ) {}          // +2b +3c
            if ( x > 0 ? !(false) : this.m() ) {}       // +1b +3c
            if ( x > 0 && x != 3 ? !(a) : (!(b)) ) {}   // +5c
            ",
            "foo.java",
            |metric| {
                // magnitude: sqrt(25 + 16 + 576) = sqrt(617)
                //  space count: 1 (1 unit)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 5.0,
                      "branches": 4.0,
                      "conditions": 24.0,
                      "magnitude": 24.839484696748443,
                      "assignments_average": 5.0,
                      "branches_average": 4.0,
                      "conditions_average": 24.0,
                      "assignments_min": 5.0,
                      "assignments_max": 5.0,
                      "branches_min": 4.0,
                      "branches_max": 4.0,
                      "conditions_min": 24.0,
                      "conditions_max": 24.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_assignments_only() {
        check_metrics::<BashParser>(
            "f() {
                 a=1
                 b=2
                 c+=3
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 3.0,
                      "branches": 0.0,
                      "conditions": 0.0,
                      "magnitude": 3.0,
                      "assignments_average": 1.5,
                      "branches_average": 0.0,
                      "conditions_average": 0.0,
                      "assignments_min": 0.0,
                      "assignments_max": 3.0,
                      "branches_min": 0.0,
                      "branches_max": 0.0,
                      "conditions_min": 0.0,
                      "conditions_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_commands_only() {
        check_metrics::<BashParser>(
            "f() {
                 echo a
                 ls
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 0.0,
                      "branches": 2.0,
                      "conditions": 0.0,
                      "magnitude": 2.0,
                      "assignments_average": 0.0,
                      "branches_average": 1.0,
                      "conditions_average": 0.0,
                      "assignments_min": 0.0,
                      "assignments_max": 0.0,
                      "branches_min": 0.0,
                      "branches_max": 2.0,
                      "conditions_min": 0.0,
                      "conditions_max": 0.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_conditions_mix() {
        // Exercises every condition path: `==` and `!=` inside `[[ ]]`,
        // arithmetic `<` inside `(( ))`, and the prefix `-z` test operator
        // inside `[ ]`. Each `if` body's `echo` contributes a branch.
        check_metrics::<BashParser>(
            "f() {
                 if [[ \"$a\" == \"$b\" ]]; then
                     echo eq
                 fi
                 if [[ \"$x\" != \"$y\" ]]; then
                     echo ne
                 fi
                 if (( $a < $b )); then
                     echo lt
                 fi
                 if [ -z \"$x\" ]; then
                     echo empty
                 fi
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 0.0,
                      "branches": 4.0,
                      "conditions": 4.0,
                      "magnitude": 5.656854249492381,
                      "assignments_average": 0.0,
                      "branches_average": 2.0,
                      "conditions_average": 2.0,
                      "assignments_min": 0.0,
                      "assignments_max": 0.0,
                      "branches_min": 0.0,
                      "branches_max": 4.0,
                      "conditions_min": 0.0,
                      "conditions_max": 4.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn bash_magnitude() {
        // Combined assignments + branches + conditions: magnitude must
        // equal sqrt(2² + 1² + 1²) = sqrt(6).
        check_metrics::<BashParser>(
            "f() {
                 a=1
                 b=2
                 if [[ \"$a\" == \"$b\" ]]; then
                     echo eq
                 fi
             }",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                    {
                      "assignments": 2.0,
                      "branches": 1.0,
                      "conditions": 1.0,
                      "magnitude": 2.449489742783178,
                      "assignments_average": 1.0,
                      "branches_average": 0.5,
                      "conditions_average": 0.5,
                      "assignments_min": 0.0,
                      "assignments_max": 2.0,
                      "branches_min": 0.0,
                      "branches_max": 1.0,
                      "conditions_min": 0.0,
                      "conditions_max": 1.0
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_malformed_parenthesized_no_panic() {
        check_metrics::<JavaParser>("class A { void m() { if (( }) }", "foo.java", |metric| {
            // tree-sitter emits ERROR nodes for this malformed source, so no
            // IfStatement, branch, or condition is recognised — all counts are 0.
            // Primary goal: the unwrap-free path does not panic.
            assert_eq!(metric.abc.assignments(), 0.0);
            assert_eq!(metric.abc.branches(), 0.0);
            assert_eq!(metric.abc.conditions(), 0.0);
            assert_eq!(metric.abc.magnitude(), 0.0);
        });
    }

    #[test]
    fn csharp_constant_declarations() {
        check_metrics::<CsharpParser>(
            "class A {
                private const int X1 = 0, Y1 = 0;
                public const float PI = 3.14f;
                const string HELLO = \"Hello,\";
                protected string world = \" world!\";
                public float e = 2.718f;
                private int x2 = 1, y2 = 2;
                void M() {
                    const int Z1 = 0, Z2 = 0, Z3 = 0;
                    const float T = 0.0f;
                    int z1 = 1, z2 = 2, z3 = 3;
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_declarations_with_conditions() {
        check_metrics::<CsharpParser>(
            "class A {
                bool a = (1 == 2);
                bool b = (1 < 2);
                bool c = !true;
                bool d = !false;
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_assignments_with_conditions() {
        check_metrics::<CsharpParser>(
            "class A {
                void M() {
                    int a = 0;
                    a += 1;
                    a -= 2;
                    a *= 3;
                    a /= 4;
                    a %= 5;
                    a++;
                    a--;
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_methods_arguments_with_conditions() {
        check_metrics::<CsharpParser>(
            "class A {
                void M(int x, int y) {
                    F(x == y, x < y, !x.Equals(y));
                }
                void F(bool a, bool b, bool c) {}
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_if_single_conditions() {
        check_metrics::<CsharpParser>(
            "class A {
                void M(int x) {
                    if (x > 0) { System.Console.WriteLine(\"a\"); }
                    if (x < 0) { System.Console.WriteLine(\"b\"); }
                    if (x == 0) { System.Console.WriteLine(\"c\"); }
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_if_multiple_conditions() {
        check_metrics::<CsharpParser>(
            "class A {
                void M(int x, int y) {
                    if (x > 0 && y > 0) { System.Console.WriteLine(\"a\"); }
                    if (x < 0 || y < 0) { System.Console.WriteLine(\"b\"); }
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_while_and_do_while_conditions() {
        check_metrics::<CsharpParser>(
            "class A {
                void M(int x) {
                    while (x > 0) { x--; }
                    do { x++; } while (x < 10);
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_return_with_conditions() {
        check_metrics::<CsharpParser>(
            "class A {
                bool M(int x) {
                    return (x > 0);
                }
                bool N(int x) {
                    return !(x < 0);
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_return_without_conditions() {
        check_metrics::<CsharpParser>(
            "class A {
                int M() { return 42; }
                string N() { return \"hi\"; }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_lambda_expressions_return_with_conditions() {
        check_metrics::<CsharpParser>(
            "class A {
                public void M() {
                    System.Func<int, bool> f = x => (x > 0);
                    System.Func<int, bool> g = x => !(x < 0);
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_for_with_variable_declaration() {
        check_metrics::<CsharpParser>(
            "class A {
                void M() {
                    for (int i = 0; i < 10; i++) {
                        System.Console.WriteLine(i);
                    }
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_for_without_variable_declaration() {
        check_metrics::<CsharpParser>(
            "class A {
                void M() {
                    int i;
                    for (i = 0; i < 10; i++) {
                        System.Console.WriteLine(i);
                    }
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_ternary_conditions() {
        check_metrics::<CsharpParser>(
            "class A {
                int Sign(int x) {
                    return (x > 0) ? 1 : (x < 0 ? -1 : 0);
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_malformed_parenthesized_no_panic() {
        check_metrics::<CsharpParser>("class A { void M() { if (( }) }", "foo.cs", |metric| {
            // Don't panic on malformed source.
            assert_eq!(metric.abc.assignments(), 0.0);
            assert_eq!(metric.abc.branches(), 0.0);
        });
    }

    #[test]
    fn csharp_function_pointer_type_no_double_count() {
        // EC1 extension — `<` and `>` are also parameter-list delimiters
        // for unsafe function-pointer types. `FunctionPointerType` must
        // be in the LT/GT exclusion list, otherwise these brackets
        // accumulate spurious `conditions` counts.
        check_metrics::<CsharpParser>(
            "unsafe class A {
                public delegate*<int, int, int> Adder;
                public delegate*<string, void> Logger;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(
                    metric.abc.conditions(),
                    0.0,
                    "function-pointer-type angle brackets must not count"
                );
            },
        );
    }

    #[test]
    fn csharp_generic_type_args_no_double_count() {
        // EC1 — `<` and `>` inside TypeArgumentList must not count as
        // boolean conditions.
        check_metrics::<CsharpParser>(
            "class A {
                void M(System.Collections.Generic.Dictionary<string, System.Collections.Generic.List<int>> d) {
                    System.Console.WriteLine(d);
                }
            }",
            "foo.cs",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn csharp_aliased_invocation_expression_branches() {
        // Regression for issue #94 (lesson #2): the C# grammar emits three
        // aliased `kind_id`s for `invocation_expression`. Code that matches
        // only the unsuffixed `Csharp::InvocationExpression` undercounts ABC
        // branches whenever the AST emits an aliased variant. The three
        // method calls live in `M`, so the per-method maximum (visible at
        // the unit-space aggregate as `branches_max`) must be 3.
        check_metrics::<CsharpParser>(
            "class A {
                void M() {
                    System.Console.WriteLine(1);
                    System.Console.WriteLine(2);
                    System.Console.WriteLine(3);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.abc.branches_max(), 3.0);
                assert_eq!(metric.abc.conditions_max(), 0.0);
            },
        );
    }

    #[test]
    fn php_zero_abc() {
        check_metrics::<PhpParser>("<?php\n", "foo.php", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn php_simple_assignment() {
        check_metrics::<PhpParser>(
            "<?php
function f(): void {
    $a = 1;
    $b = 2;
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_augmented_assignment() {
        check_metrics::<PhpParser>(
            "<?php
function f(int $x): int {
    $a = 0;
    $a += $x;
    $a -= 1;
    $a *= 2;
    return $a;
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_const_excluded() {
        // Constant declarations and enum cases are NOT counted as
        // assignments — they declare immutable values.
        check_metrics::<PhpParser>(
            "<?php
class A {
    const PI = 3.14;
    const E = 2.71;
}
enum Color {
    case Red;
    case Green;
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_function_call() {
        check_metrics::<PhpParser>(
            "<?php
function f(): void {
    foo();
    bar(1, 2);
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_method_call() {
        check_metrics::<PhpParser>(
            "<?php
function f($obj): void {
    $obj->m1();
    $obj->m2(1);
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_static_call() {
        check_metrics::<PhpParser>(
            "<?php
function f(): void {
    Foo::bar();
    Foo::baz(1);
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_nullsafe_call() {
        check_metrics::<PhpParser>(
            "<?php
function f($obj): void {
    $obj?->m1();
    $obj?->m2(1);
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_object_creation() {
        check_metrics::<PhpParser>(
            "<?php
function f(): void {
    new Foo();
    new Bar(1);
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_comparison_eq() {
        check_metrics::<PhpParser>(
            "<?php
function f(int $a, int $b): bool {
    return $a == $b || $a != $b;
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_comparison_strict() {
        check_metrics::<PhpParser>(
            "<?php
function f(int $a, int $b): bool {
    return $a === $b || $a !== $b;
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_spaceship() {
        check_metrics::<PhpParser>(
            "<?php
function f(int $a, int $b): int {
    return $a <=> $b;
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_instanceof() {
        check_metrics::<PhpParser>(
            "<?php
function f($x): bool {
    return $x instanceof Foo;
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    #[test]
    fn php_complex_function() {
        // One snippet exercising A, B, C buckets together.
        check_metrics::<PhpParser>(
            "<?php
function f(int $a, int $b): int {
    $sum = $a + $b;
    $prod = $a * $b;
    if ($sum > 0 && $prod === 0) {
        return foo($sum);
    }
    return bar()->double();
}",
            "foo.php",
            |metric| insta::assert_json_snapshot!(metric.abc),
        );
    }

    // --- Kotlin ABC tests -------------------------------------------------

    #[test]
    fn kotlin_empty_class() {
        check_metrics::<KotlinParser>("class C {}", "foo.kt", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn kotlin_val_declarations_are_not_assignments() {
        // `val` introduces an immutable binding — the `=` initialising it
        // is not an assignment in the ABC sense.
        check_metrics::<KotlinParser>(
            "class C {
                val a: Int = 1
                val b: Int = 2
                val c: Int = 3
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_var_declarations_count_assignment() {
        // `var` initialisers count as assignments (mutable binding).
        check_metrics::<KotlinParser>(
            "class C {
                var a: Int = 1
                var b: Int = 2
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_augmented_assignments_count() {
        // Augmented operators (+=, -=, etc.) and ++/-- always count.
        check_metrics::<KotlinParser>(
            "fun m() {
                var x = 0
                x += 1
                x -= 2
                x *= 3
                x++
                --x
            }",
            "foo.kt",
            |metric| {
                // var declaration (var x = 0): +1
                // x += 1, x -= 2, x *= 3, x++, --x: +5
                assert_eq!(metric.abc.assignments_sum(), 6.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_branches_call_expression() {
        check_metrics::<KotlinParser>(
            "fun m() {
                println(\"a\")
                println(\"b\")
                println(\"c\")
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_object_construction_branch() {
        // Kotlin's object construction is just `Foo()` — a `CallExpression`.
        check_metrics::<KotlinParser>(
            "class P(val x: Int)
            fun m(): P = P(1)",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_comparisons_count_conditions() {
        check_metrics::<KotlinParser>(
            "fun m(a: Int, b: Int): Boolean {
                val r1 = a < b
                val r2 = a > b
                val r3 = a <= b
                val r4 = a >= b
                val r5 = a == b
                val r6 = a != b
                return r1 || r2 || r3 || r4 || r5 || r6
            }",
            "foo.kt",
            |metric| {
                // Six binary operators: <, >, <=, >=, ==, != → 6 conditions.
                assert_eq!(metric.abc.conditions_sum(), 6.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_identity_equality_conditions() {
        // `===` / `!==` are referential equality in Kotlin; they count too.
        check_metrics::<KotlinParser>(
            "fun m(a: Any, b: Any): Boolean {
                return a === b || a !== b
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_else_branch_counts() {
        check_metrics::<KotlinParser>(
            "fun m(x: Int): Int {
                return if (x > 0) 1 else -1
            }",
            "foo.kt",
            |metric| {
                // condition: > (1) + else (1) = 2
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_when_entries_count() {
        check_metrics::<KotlinParser>(
            "fun m(x: Int): Int {
                return when (x) {
                    1 -> 10
                    2 -> 20
                    else -> 0
                }
            }",
            "foo.kt",
            |metric| {
                // Each WhenEntry counts once (including `else`).
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_catch_block_counts() {
        check_metrics::<KotlinParser>(
            "fun m() {
                try {
                    println(\"ok\")
                } catch (e: Exception) {
                    println(\"err\")
                }
            }",
            "foo.kt",
            |metric| {
                // CatchBlock contributes 1 condition.
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_elvis_and_safe_cast() {
        // `?:` (elvis) and `as?` (safe cast) are condition-like.
        check_metrics::<KotlinParser>(
            "fun m(s: String?): Int {
                val n = (s as? Int) ?: 0
                return n
            }",
            "foo.kt",
            |metric| {
                // as? (+1) + ?: (+1) = 2 conditions.
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_generic_brackets_not_conditions() {
        // `<` / `>` used as type-parameter brackets must not be counted.
        check_metrics::<KotlinParser>(
            "class Box<T>(val v: T)
            fun <T> wrap(x: T): Box<T> = Box(x)",
            "foo.kt",
            |metric| {
                // No comparisons — only generic brackets.
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_class_with_methods_and_branches() {
        check_metrics::<KotlinParser>(
            "class C {
                var counter: Int = 0
                fun bump() {
                    counter += 1
                    println(counter)
                }
            }",
            "foo.kt",
            |metric| {
                // assignments: var counter = 0 (+1), counter += 1 (+1) = 2
                // branches: println(counter) = 1
                assert_eq!(metric.abc.assignments_sum(), 2.0);
                assert_eq!(metric.abc.branches_sum(), 1.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_object_singleton_abc() {
        check_metrics::<KotlinParser>(
            "object Util {
                fun work(x: Int): Int {
                    var y = x
                    y += 1
                    if (y > 0) {
                        return y
                    }
                    return -1
                }
            }",
            "foo.kt",
            |metric| {
                // assignments: var y = x (+1), y += 1 (+1) = 2
                // branches: 0 (return is not a call)
                // conditions: y > 0 (+1) = 1
                assert_eq!(metric.abc.assignments_sum(), 2.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_interface_abc() {
        // Pure-abstract interface with no bodies — all-zero.
        check_metrics::<KotlinParser>(
            "interface I {
                fun work(): Int
                fun describe(): String
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_nested_class_abc() {
        check_metrics::<KotlinParser>(
            "class Outer {
                var o: Int = 0
                class Nested {
                    var n: Int = 0
                    fun bump() { n += 1 }
                }
            }",
            "foo.kt",
            |metric| {
                // Outer: var o = 0 (+1)
                // Nested: var n = 0 (+1), n += 1 (+1) = 2
                // total assignments = 3
                assert_eq!(metric.abc.assignments_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_data_class_abc() {
        // `data class` with primary-constructor `val`s — no assignments
        // (vals don't count) and no body conditions.
        check_metrics::<KotlinParser>(
            "data class Point(val x: Int, val y: Int)",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_primary_constructor_default_value_not_assignment() {
        // Regression: default values on primary-constructor `val`
        // parameters are initialisers, not assignments. Without
        // `ClassParameter` pushing a declaration sentinel, the `=` token
        // here would be counted unconditionally as a standalone
        // assignment.
        check_metrics::<KotlinParser>("class C(val a: Int = 5)", "foo.kt", |metric| {
            // `val a = 5` → suppressed (Const sentinel).
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    // --- TypeScript / TSX ABC tests --------------------------------------
    //
    // Assignment, branch, condition counting per Fitzpatrick:
    // - Augmented assignment / `++` / `--` always count.
    // - Plain `=` counts unless inside `const` declaration.
    // - `call_expression` / `new_expression` count as branches.
    // - Comparison / equality operators, ternary `?`, `??`, control-flow
    //   arms (`else`, `case`, `default`, `catch`, `try`, `instanceof`),
    //   and `<`/`>` (outside `type_arguments` / `type_parameters`) count
    //   as conditions.

    #[test]
    fn typescript_assignments_basic() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(): void {
                    let x = 0;          // const-sentinel suppressed since `let`, but x is Var → +1
                    x = 1;              // +1
                    x += 2;             // +1
                    x++;                // +1
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_const_excluded_from_assignments() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(): void {
                    const a = 1;        // suppressed (Const sentinel)
                    const b = 2;        // suppressed
                    let c = 3;          // +1 (Var sentinel)
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_branches_function_calls() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(): void {
                    foo();              // +1
                    bar(1, 2);          // +1
                    new Date();         // +1
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_conditions_comparison_operators() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(x: number, y: number): boolean {
                    return x == y       // +1
                        || x === y      // +1
                        || x != y       // +1
                        || x !== y      // +1
                        || x < y        // +1
                        || x <= y       // +1
                        || x > y        // +1
                        || x >= y;      // +1
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 8.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_conditions_control_flow_arms() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(x: number): number {
                    try {                       // +1 (try)
                        if (x > 0) {            // +1 (>)
                            return 1;
                        } else {                // +1 (else)
                            return -1;
                        }
                    } catch (e) {               // +1 (catch)
                        return 0;
                    }
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_conditions_switch_case() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(x: number): number {
                    switch (x) {
                        case 1:                 // +1
                            return 1;
                        case 2:                 // +1
                            return 2;
                        default:                // +1
                            return 0;
                    }
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_ternary_and_nullish() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(x: number | null): number {
                    return x !== null           // +1 (!==)
                        ? x                     // +1 (ternary ?)
                        : 0;
                }
                n(x: number | null): number {
                    return x ?? 0;              // +1 (??)
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_instanceof_counts_as_condition() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(o: unknown): boolean {
                    return o instanceof C;      // +1
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_generic_lt_gt_not_a_condition() {
        // `<T>` in `class C<T>` and `Array<number>` should not contribute
        // to conditions even though the tokens are `<` and `>`.
        check_metrics::<TypescriptParser>(
            "class C<T> {
                xs: Array<number> = [];
                m(): void {
                    const arr: Array<string> = [];   // suppressed const
                    void arr;
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_abstract_class_abc() {
        // Abstract methods have no body — they contribute nothing.
        check_metrics::<TypescriptParser>(
            "abstract class C {
                abstract a(): void;
                m(x: number): number {
                    if (x > 0) return 1;        // +1 condition
                    return 0;
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_interface_abc_zero() {
        check_metrics::<TypescriptParser>(
            "interface I {
                a(): void;
                b(): number;
                p: string;
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_arrow_field_contributes_abc() {
        // Arrow function class members are function spaces; their
        // assignments/branches/conditions are counted.
        check_metrics::<TypescriptParser>(
            "class C {
                arrow = (x: number) => {
                    if (x > 0) {                // +1 condition
                        return foo();           // +1 branch
                    }
                    return 0;
                };
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_parameter_property_init_not_assignment() {
        // Parameter properties don't introduce a `=` token themselves;
        // only the explicit `let z = 0` body assignment is counted.
        // The class field initializer `f: number = 0` likewise has a `=`
        // that DOES count (matches `typescript_assignments_basic`).
        check_metrics::<TypescriptParser>(
            "class C {
                f: number = 0;
                constructor(public x: number, private y: string) {
                    let z = 0;
                }
            }",
            "foo.ts",
            |metric| {
                // f's initializer + `let z = 0` = 2 assignments; the
                // parameter properties contribute zero.
                assert_eq!(metric.abc.assignments_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // TSX parity

    #[test]
    fn tsx_assignments_basic() {
        check_metrics::<TsxParser>(
            "class C {
                m(): void {
                    let x = 0;
                    x = 1;
                    x += 2;
                    x++;
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_const_excluded_from_assignments() {
        check_metrics::<TsxParser>(
            "class C {
                m(): void {
                    const a = 1;
                    let b = 2;
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_branches_function_calls() {
        check_metrics::<TsxParser>(
            "class C {
                m(): void {
                    foo();
                    new Date();
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_conditions_comparison_operators() {
        check_metrics::<TsxParser>(
            "class C {
                m(x: number, y: number): boolean {
                    return x == y || x < y || x >= y;
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_conditions_control_flow_arms() {
        check_metrics::<TsxParser>(
            "class C {
                m(x: number): number {
                    try {
                        if (x > 0) return 1;
                        else return -1;
                    } catch (e) {
                        return 0;
                    }
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_conditions_switch_case() {
        check_metrics::<TsxParser>(
            "class C {
                m(x: number): number {
                    switch (x) {
                        case 1: return 1;
                        case 2: return 2;
                        default: return 0;
                    }
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_ternary_and_nullish() {
        check_metrics::<TsxParser>(
            "class C {
                m(x: number | null): number {
                    return x !== null ? x : 0;
                }
                n(x: number | null): number { return x ?? 0; }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_instanceof_counts_as_condition() {
        check_metrics::<TsxParser>(
            "class C { m(o: unknown): boolean { return o instanceof C; } }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_generic_lt_gt_not_a_condition() {
        check_metrics::<TsxParser>(
            "class C<T> { xs: Array<number> = []; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_abstract_class_abc() {
        check_metrics::<TsxParser>(
            "abstract class C {
                abstract a(): void;
                m(x: number): number {
                    if (x > 0) return 1;
                    return 0;
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_interface_abc_zero() {
        check_metrics::<TsxParser>(
            "interface I { a(): void; p: string; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_arrow_field_contributes_abc() {
        check_metrics::<TsxParser>(
            "class C {
                arrow = (x: number) => {
                    if (x > 0) return foo();
                    return 0;
                };
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_parameter_property_init_not_assignment() {
        // Parameter properties contribute no `=`; the body's `let z = 0`
        // and the field initializer do.
        check_metrics::<TsxParser>(
            "class C {
                f: number = 0;
                constructor(public x: number) { let z = 0; }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }
}
