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
    csharp_invocation_expr_kinds, csharp_paren_expr_kinds, csharp_prefix_unary_expr_kinds,
    implement_metric_trait,
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
    ///
    /// Collapses the `f64::MAX` sentinel that `Stats::default()` plants
    /// into `assignments_min` to `0.0`, so a never-observed space
    /// serializes to a meaningful number rather than `1.7976931e308`.
    #[allow(clippy::float_cmp)]
    #[must_use]
    pub fn assignments_min(&self) -> f64 {
        if self.assignments_min == f64::MAX {
            0.0
        } else {
            self.assignments_min
        }
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
    ///
    /// Same `f64::MAX` sentinel collapse as `assignments_min`.
    #[allow(clippy::float_cmp)]
    #[must_use]
    pub fn branches_min(&self) -> f64 {
        if self.branches_min == f64::MAX {
            0.0
        } else {
            self.branches_min
        }
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
    ///
    /// Same `f64::MAX` sentinel collapse as `assignments_min`.
    #[allow(clippy::float_cmp)]
    #[must_use]
    pub fn conditions_min(&self) -> f64 {
        if self.conditions_min == f64::MAX {
            0.0
        } else {
            self.conditions_min
        }
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

#[doc(hidden)]
/// Per-language computation of the ABC metric.
pub trait Abc
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    ///
    /// `code` is the source bytes underlying the parsed tree. Most
    /// languages ignore it: assignments, branches, and conditions all
    /// surface as distinct grammar productions and a `kind_id()` match
    /// is enough. Elixir is the exception — `case` / `cond` / `if` /
    /// `with` / guard `when` arms surface as `Call` nodes whose keyword
    /// target lives only in the source text. Matching the `Cyclomatic`
    /// / `Halstead` / `Exit` / `Cognitive` pattern keeps the signature
    /// uniform.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats);
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

// Groovy mirror of `java_inspect_container`. The dekobon Groovy grammar
// inherits `parenthesized_expression`, `unary_expression`, and the
// standard boolean-context kinds from tree-sitter-java verbatim, so the
// body is structurally identical to Java's helper. The terminal set
// includes `CommandChain` (the new grammar's distinct node for Groovy's
// parens-less call form `println foo`), keeping it in sync with the
// `impl Abc for GroovyCode` branches dispatch.
fn groovy_inspect_container(container_node: &Node, conditions: &mut f64) {
    use Groovy::*;

    let mut node = *container_node;
    let mut node_kind = node.kind_id().into();

    let Some(parent) = node.parent() else { return };
    let mut has_boolean_content = match parent.kind_id().into() {
        BinaryExpression | IfStatement | WhileStatement | DoWhileStatement | ForStatement => true,
        TernaryExpression => node
            .previous_sibling()
            .is_none_or(|prev_node| !matches!(prev_node.kind_id().into(), QMARK | COLON)),
        _ => false,
    };

    loop {
        let is_parenthesised_exp = matches!(node_kind, ParenthesizedExpression);
        let is_not_operator = matches!(node_kind, UnaryExpression)
            && node
                .child(0)
                .is_some_and(|c| matches!(c.kind_id().into(), BANG));

        if !is_parenthesised_exp && !is_not_operator {
            break;
        }

        if !has_boolean_content && is_not_operator {
            has_boolean_content = true;
        }

        let Some(child) = node.child(1) else { break };
        node = child;
        node_kind = node.kind_id().into();

        if matches!(
            node_kind,
            MethodInvocation | CommandChain | Identifier | True | False
        ) {
            if has_boolean_content {
                *conditions += 1.;
            }
            break;
        }
    }
}

fn groovy_count_unary_conditions(list_node: &Node, conditions: &mut f64) {
    use Groovy::*;

    let list_kind = list_node.kind_id().into();
    let mut cursor = list_node.cursor();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let node_kind = node.kind_id().into();

            if matches!(
                node_kind,
                MethodInvocation | CommandChain | Identifier | True | False
            ) && matches!(list_kind, BinaryExpression)
            {
                *conditions += 1.;
            } else {
                groovy_inspect_container(&node, conditions);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// Default no-op `Abc` impls. Audited in #188; the matrix below
// records the rationale for every entry so the no-op default is a
// deliberate choice, not scaffolding leftover.
//
// Real defaults (the language has no construct ABC measures, so the
// metric is genuinely 0):
//   - PreprocCode, CcommentCode: no executable code (comments /
//     preprocessor lines only).
implement_metric_trait!(Abc, PreprocCode, CcommentCode);

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
        fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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

// JavaScript / Mozjs share TypeScript's expression / statement
// vocabulary. The `js_abc_compute!` macro expands the same
// token-level Fitzpatrick rules as `ts_abc_compute!`, with two
// adjustments:
//
//   1. `LT` / `GT` are always comparison operators in plain JS — there
//      are no `TypeArguments` / `TypeParameters` nodes to gate against.
//   2. JS retains the same `LexicalDeclaration` / `VariableDeclaration`
//      sentinel handling so `const x = 5` does not double-count the
//      initializer `=` as an assignment. `let x = 5` and `var x = 5`
//      DO count their initializer `=` as an assignment — only `const`
//      suppresses, matching the TS impl above. This deliberately
//      deviates from a strict reading of Fitzpatrick's "declaration
//      initialiser is not an assignment" rule because `let`/`var`
//      bindings can be reassigned and the initial value is the first
//      assignment of the binding's lifetime.
macro_rules! js_abc_compute {
    ($lang:ident) => {
        fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
            use $lang::*;

            match node.kind_id().into() {
                PLUSEQ | DASHEQ | STAREQ | SLASHEQ | PERCENTEQ | STARSTAREQ | AMPEQ | PIPEEQ
                | CARETEQ | LTLTEQ | GTGTEQ | GTGTGTEQ | AMPAMPEQ | PIPEPIPEEQ | QMARKQMARKEQ
                | PLUSPLUS | DASHDASH => {
                    stats.assignments += 1.;
                }
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
                EQ if !matches!(stats.declaration.last(), Some(DeclKind::Const)) => {
                    stats.assignments += 1.;
                }
                CallExpression | NewExpression => {
                    stats.branches += 1.;
                }
                EQEQ | EQEQEQ | BANGEQ | BANGEQEQ | LTEQ | GTEQ | LT | GT | QMARK | QMARKQMARK
                | Instanceof | Else | Case | Default | Try | Catch => {
                    stats.conditions += 1.;
                }
                _ => {}
            }
        }
    };
}

impl Abc for JavascriptCode {
    js_abc_compute!(Javascript);
}

impl Abc for MozjsCode {
    js_abc_compute!(Mozjs);
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
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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

// Ruby ABC rules follow the Fitzpatrick paper's spirit, adapted to
// tree-sitter-ruby:
// - Assignments: `assignment` (plain `=`) and `operator_assignment`
//   (`+=`, `-=`, `||=`, `&&=`, …). Tree-sitter wraps both forms in a
//   dedicated node, so we count one assignment per node and avoid
//   double-counting the inner `=` / augmented token.
// - Branches: every Ruby method invocation kind (`Call` / `Call2` /
//   `Call3` / `Call4`) plus `super` and `yield`. `yield` is grammar-
//   level a "block invocation" but ABC's branch bucket is "message
//   pass / function call", so it belongs here. `attr_*` macros are
//   `Call3` nodes and are counted as branches like any other call.
// - Conditions: comparison and equality operator tokens emitted inside
//   `binary` (`==`, `!=`, `===`, `<`, `>`, `<=`, `>=`, `<=>`,
//   `=~`, `!~`), plus the control-flow arms that the Fitzpatrick rules
//   list — the named clause nodes `Else` / `Elsif` / `When` and the
//   `?` ternary marker, plus `Rescue` (the rescue clause) and rescue
//   modifiers. `if` / `unless` themselves are not counted (the head
//   condition appears as the inner comparison); the `Then` clause is
//   an implicit grammar wrapper around every `if` / `elsif` body and
//   is NOT counted as a separate arm.
impl Abc for RubyCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Ruby::*;

        match node.kind_id().into() {
            Assignment | Assignment2 | OperatorAssignment | OperatorAssignment2 => {
                stats.assignments += 1.;
            }
            Call | Call2 | Call3 | Call4 | Super | Yield | Yield2 => {
                stats.branches += 1.;
            }
            EQEQ | BANGEQ | EQEQEQ | LT | GT | LTEQ | GTEQ | LTEQGT | EQTILDE | BANGTILDE
            | Else | Elsif | When | QMARK | Rescue | RescueModifier | RescueModifier2
            | RescueModifier3 => {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

// Fitzpatrick's ABC rules adapted for Python.
//
// - Assignments: every `Assignment` node that contains an explicit `=`
//   token (plain assignment, walrus `:=` lives in `NamedExpression`,
//   handled separately), plus every `AugmentedAssignment` (`+=`,
//   `-=`, …) and every `NamedExpression` (walrus). Bare type-only
//   annotations like `x: int` also parse as `Assignment` but have no
//   `=` child — these are excluded so a class-level type annotation
//   does not inflate the assignment count.
// - Branches: every `Call` node. Python's "object construction" is
//   syntactically a `Call` (`Foo()` parses as `call`), so the same
//   arm covers it without a separate `New`-style case.
// - Conditions: comparison operators (`ComparisonOperator` wraps
//   `<`, `>`, `==`, `!=`, `is`, `is not`, `in`, `not in`, etc. as a
//   single node), `BooleanOperator` (`and`/`or`), `ConditionalExpression`
//   (ternary `a if c else b`), and the explicit arms of control flow:
//   `ElifClause`, `ElseClause`, `ExceptClause`, `FinallyClause`,
//   `CaseClause`. We do not separately count the `if` / `while`
//   keyword: the condition expression itself is already covered by
//   `ComparisonOperator` or `BooleanOperator`. This matches the
//   token-level approach used for PHP / Bash.
impl Abc for PythonCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Python::*;

        match node.kind_id().into() {
            // Plain `=` assignment. tree-sitter-python emits an
            // `Assignment` node for both `x = 1` (LHS, `=`, RHS) and
            // bare annotations `x: int` (LHS, `:`, type, *no* `=`).
            // Filtering on the presence of an `EQ` child keeps the
            // annotation-only case out of the count.
            Assignment if node.first_child(|id| id == EQ).is_some() => {
                stats.assignments += 1.;
            }
            // Augmented assignment (`+=`, `-=`, `*=`, …) always counts;
            // walrus `name := expr` is a PEP-572 `NamedExpression` and
            // also binds a value, so it counts as one assignment under
            // Fitzpatrick's rule.
            AugmentedAssignment | NamedExpression => {
                stats.assignments += 1.;
            }
            // Every call — function call, method call, type
            // construction — is one branch. Python parses `Foo()` as
            // `Call`, so object construction folds into this arm.
            Call => {
                stats.branches += 1.;
            }
            // `x < y`, `a == b`, `c is None`, `n in xs`, `m not in xs`
            // all parse as a single `ComparisonOperator` node — one
            // node, one condition, regardless of how many comparison
            // operators are chained.
            ComparisonOperator
            | BooleanOperator
            | ConditionalExpression
            | ElifClause
            | ElseClause
            | ExceptClause
            | FinallyClause
            | NotOperator => {
                // `NotOperator` is Python's unary `not`. Counting it
                // mirrors Java's `!x` / C#'s `!x` Abc condition rule
                // and closes the parity gap noted in #214 — without
                // it, `if not flag:` reports 0 conditions while
                // `if !flag` in Java reports 1. Nested combos like
                // `not (x > 0)` count both the unary and the
                // comparison once each (one logical "is-negation",
                // one logical "comparison"), matching Java's
                // `!(x > 0)`.
                stats.conditions += 1.;
            }
            // A non-wildcard `case` arm contributes one condition,
            // matching Rust's bare-`_` MatchArm filter and Java/C#'s
            // `default:` rule. The bare wildcard is detected by: (a)
            // `case_pattern` is `_`, AND (b) no `if_clause` sibling
            // on the `case_clause` — `case _ if g:` carries a guard
            // and still counts. The shared classifier lives in
            // `super::npa` next to `pattern_is_bare_underscore`.
            CaseClause if super::npa::python_case_clause_counts(node, UNDERSCORE as u16) => {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

impl Abc for RustCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Rust::*;

        match node.kind_id().into() {
            // Plain `x = expr` (assignment_expression) and augmented
            // forms `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`,
            // `<<=`, `>>=` (compound_assignment_expr) both bind a
            // value; each counts as one assignment. Rust grammar
            // isolates both in distinct named nodes, so there is no
            // risk of double-counting the contained `EQ` token here.
            AssignmentExpression | CompoundAssignmentExpr => {
                stats.assignments += 1.;
            }
            // Every call expression — including method calls
            // (`a.b.c()` parses as `call_expression` whose callee is a
            // `field_expression`) — plus every `try_expression` (the
            // `?` operator, a short-circuit return on Result / Option)
            // contributes one branch. Macro invocations parse as
            // `macro_invocation`, NOT `call_expression`, so they are
            // intentionally NOT counted as branches.
            CallExpression | TryExpression => {
                stats.branches += 1.;
            }
            // Comparison operators emitted as token children of a
            // `binary_expression`, `if let` / `while let` conditions,
            // and the `else` keyword each count as one condition.
            // `let_condition` covers both `if let` and `while let`
            // (Rust's grammar uses the same node for both); inside a
            // `let_chain` each `let_condition` counts separately.
            // Java counts the `Else` token directly; Rust's grammar
            // exposes the same token and we follow that lead.
            LTEQ | GTEQ | EQEQ | BANGEQ | LetCondition | Else => {
                stats.conditions += 1.;
            }
            // `<` / `>` doubles as type-argument delimiter; the
            // `BinaryExpression` parent check disambiguates without
            // needing to inspect siblings.
            LT | GT
                if node
                    .parent()
                    .is_some_and(|p| matches!(p.kind_id().into(), BinaryExpression)) =>
            {
                stats.conditions += 1.;
            }
            // Every non-wildcard `match_arm` is one condition. A bare
            // `_ => ...` arm is the C / Java `default:` equivalent and
            // is excluded — mirrors the cyclomatic treatment and
            // Kotlin's `when` / Java's `case` rules. Patterns like
            // `Some(_)`, `(_, x)`, or `_ if guard` are not bare
            // wildcards and still count. The check scans only NAMED
            // children of `match_pattern` so anonymous tokens like a
            // leading `|` (allowed in or-patterns: `| _ => ...`) do
            // not throw off the detection. A guard (`_ if g`) adds a
            // second named child to `match_pattern` and so escapes
            // the bare-wildcard filter.
            MatchArm | MatchArm2 => {
                let is_bare_wildcard = node.child_by_field_name("pattern").is_some_and(|pat| {
                    super::npa::pattern_is_bare_underscore(&pat, UNDERSCORE as u16)
                });
                if !is_bare_wildcard {
                    stats.conditions += 1.;
                }
            }
            _ => {}
        }
    }
}

impl Abc for GoCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        // Aliased because `Go::Go` (the `go` keyword variant) collides
        // with the bare enum name in pattern position under
        // `use Go::*;` (same workaround as in cyclomatic / cognitive).
        use Go as G;

        match node.kind_id().into() {
            // Plain `=`, augmented `+=`, `-=`, … all parse as
            // `assignment_statement`. `:=` is a short variable
            // declaration. `x++` / `x--` rebind too.
            G::AssignmentStatement | G::ShortVarDeclaration | G::IncStatement | G::DecStatement => {
                stats.assignments += 1.;
            }
            // Every call expression — including method calls
            // (`r.Method()` parses as `call_expression` whose callee is
            // a `selector_expression`) — contributes one branch.
            // Composite literals (`Point{X: 1}`) are NOT calls.
            G::CallExpression => {
                stats.branches += 1.;
            }
            // Comparison operators emitted as token children of a
            // `binary_expression`, `else`, and each non-default switch
            // / type-switch / select arm all contribute one condition.
            // `<` / `>` double as type-argument delimiters in generic
            // instantiations (`f[T any]`, `List[int]`); the
            // `BinaryExpression` parent guard filters those out
            // without inspecting siblings. `default_case` is
            // intentionally excluded — like Java / C# `default:`, it
            // does not introduce a new decision point.
            G::EQEQ
            | G::BANGEQ
            | G::LTEQ
            | G::GTEQ
            | G::Else
            | G::ExpressionCase
            | G::TypeCase
            | G::CommunicationCase => {
                stats.conditions += 1.;
            }
            G::LT | G::GT
                if node
                    .parent()
                    .is_some_and(|p| matches!(p.kind_id().into(), G::BinaryExpression)) =>
            {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

impl Abc for CppCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Cpp::*;

        match node.kind_id().into() {
            // `assignment_expression` covers both plain `=` and every
            // compound form (`+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`,
            // `^=`, `<<=`, `>>=`); the grammar lifts them all into a
            // single named node so we count once per
            // `assignment_expression`. `update_expression` covers both
            // prefix and postfix `++` / `--`. Variable initialisers
            // (`int x = 0`) parse as `init_declarator` inside
            // `declaration` and never become `assignment_expression` —
            // they correctly stay out.
            AssignmentExpression | AssignmentExpression2 | UpdateExpression => {
                stats.assignments += 1.;
            }
            // Every call counts (method calls fold in as
            // `call_expression` with a `field_expression` callee). The
            // C++ grammar exposes two aliased `call_expression` ids.
            // `new T(...)` allocations count as a branch — they invoke
            // a constructor, mirroring Java's `New` and C#'s
            // `ObjectCreationExpression` rule.
            CallExpression | CallExpression2 | NewExpression => {
                stats.branches += 1.;
            }
            // Comparison operators emitted as token children of a
            // `binary_expression`. The C++20 spaceship `<=>` (`LTEQGT`)
            // is a comparison operator and counts once per use.
            // `&&` / `||` add one each per Fitzpatrick. `else` opens
            // an alternative branch path; `case` (non-default) adds
            // one per switch arm; `?` opens a ternary; `try` / `catch`
            // count per Fitzpatrick (and Java's rule). `Try2` is the
            // second token-id alias the C++ grammar emits for `try`
            // (it appears under structured-exception forms).
            LTEQ | GTEQ | EQEQ | BANGEQ | LTEQGT | AMPAMP | PIPEPIPE | Else | Case | QMARK
            | Try | Try2 | Catch => {
                stats.conditions += 1.;
            }
            // Plain `<` / `>` doubles as template-argument and
            // template-parameter delimiter (`std::vector<int>`,
            // `template <typename T>`). The `binary_expression` parent
            // check disambiguates without inspecting siblings — only
            // comparison uses of `<` / `>` count. Both kind-id aliases
            // (`BinaryExpression`, `BinaryExpression2`) are accepted
            // because the C++ grammar emits the same node under two
            // production-rule paths.
            LT | GT
                if node.parent().is_some_and(|p| {
                    matches!(p.kind_id().into(), BinaryExpression | BinaryExpression2)
                }) =>
            {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

impl Abc for BashCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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

// Groovy's ABC mirrors Java's directly because the dekobon Groovy
// grammar shares Java's expression / statement vocabulary for the
// shapes ABC cares about (assignments, branches, conditions).
// Groovy-specific touches over Java:
//   - `CommandChain` (parens-less calls like `println foo`) counts as
//     a branch alongside `MethodInvocation` (#247).
//   - `DoWhileStatement` (the new grammar's name for `do { } while`)
//     replaces the prior `DoStatement` (which the amaanq grammar used).
//   - Closures (`{ x -> ... }`) have block bodies — no implicit-return
//     "single-expression arm" like a Java lambda, so the prior
//     `LambdaExpression` arm is intentionally absent.
//   - The dekobon grammar inlines the parens of `if (…)` / `while (…)`
//     / `do { … } while (…)` as `(` / `)` token children rather than
//     wrapping the condition in a `parenthesized_expression`, so the
//     condition is at a different child index than under Java's
//     grammar and must be inspected differently.
impl Abc for GroovyCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Groovy::*;

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
            EQ if stats
                .declaration
                .last()
                .is_none_or(|decl| matches!(decl, DeclKind::Var)) =>
            {
                stats.assignments += 1.;
            }
            MethodInvocation | CommandChain | New => {
                stats.branches += 1.;
            }
            GTEQ | LTEQ | EQEQ | BANGEQ | Else | Case | Default | QMARK | Try | Catch => {
                stats.conditions += 1.;
            }
            GT | LT => {
                // Excludes `<` / `>` used for generic types (e.g.
                // `List<String>`).
                if let Some(parent) = node.parent()
                    && !matches!(parent.kind_id().into(), TypeArguments)
                {
                    stats.conditions += 1.;
                }
            }
            AMPAMP | PIPEPIPE => {
                if let Some(parent) = node.parent() {
                    groovy_count_unary_conditions(&parent, &mut stats.conditions);
                }
            }
            ArgumentList => {
                groovy_count_unary_conditions(node, &mut stats.conditions);
            }
            VariableDeclarator | AssignmentExpression => {
                if let Some(right_operand) = node.child(2)
                    && matches!(
                        right_operand.kind_id().into(),
                        ParenthesizedExpression | UnaryExpression
                    )
                {
                    groovy_inspect_container(&right_operand, &mut stats.conditions);
                }
            }
            IfStatement | WhileStatement => {
                // dekobon `if_statement` / `while_statement` shape:
                // [keyword, `(`, condition, `)`, body, …]. Condition
                // lives at child index 2 (not 1 as under tree-sitter-
                // java, where the parens come wrapped in a
                // `parenthesized_expression`).
                if let Some(condition) = node.child(2) {
                    groovy_count_condition(&condition, &mut stats.conditions);
                }
            }
            DoWhileStatement => {
                // dekobon shape: [`do`, body, `while`, `(`, condition,
                // `)`]. Condition is at child index 4.
                if let Some(condition) = node.child(4) {
                    groovy_count_condition(&condition, &mut stats.conditions);
                }
            }
            ForStatement => {
                // Two shapes: a present condition lives at child(3);
                // an empty condition shows up as a bare `SEMI` token at
                // child(3) with the next slot (child(4)) holding either
                // the update expression or `;`/`)` for `for(;;)`-style
                // empty-condition loops, which still count as a single
                // condition slot.
                if let Some(condition) = node.child(3) {
                    if matches!(condition.kind_id().into(), SEMI) {
                        if let Some(cond) = node.child(4) {
                            if matches!(cond.kind_id().into(), SEMI | RPAREN) {
                                stats.conditions += 1.;
                            } else {
                                groovy_count_condition(&cond, &mut stats.conditions);
                            }
                        }
                    } else {
                        groovy_count_condition(&condition, &mut stats.conditions);
                    }
                }
            }
            ReturnStatement => {
                if let Some(value) = node.child(1)
                    && matches!(
                        value.kind_id().into(),
                        ParenthesizedExpression | UnaryExpression
                    )
                {
                    groovy_inspect_container(&value, &mut stats.conditions);
                }
            }
            TernaryExpression => {
                if let Some(condition) = node.child(0) {
                    groovy_count_condition(&condition, &mut stats.conditions);
                }
                for branch_idx in [2, 4] {
                    if let Some(expression) = node.child(branch_idx)
                        && matches!(
                            expression.kind_id().into(),
                            ParenthesizedExpression | UnaryExpression
                        )
                    {
                        groovy_inspect_container(&expression, &mut stats.conditions);
                    }
                }
            }
            _ => {}
        }
    }
}

// Counts a single boolean-context condition expression for the dekobon
// Groovy grammar. The grammar inlines `(` / `)` as token children of
// `if_statement` / `while_statement` / `do_while_statement` rather than
// wrapping the condition in a `parenthesized_expression`, so the
// condition shows up bare. A bare identifier / boolean / call / command
// chain contributes one condition directly; parenthesised and unary
// containers delegate to `groovy_inspect_container`; binary / ternary
// expressions are picked up by their own arms.
fn groovy_count_condition(condition: &Node, conditions: &mut f64) {
    use Groovy::*;
    match condition.kind_id().into() {
        MethodInvocation | CommandChain | Identifier | True | False => {
            *conditions += 1.;
        }
        ParenthesizedExpression | UnaryExpression => {
            groovy_inspect_container(condition, conditions);
        }
        _ => {}
    }
}

impl Abc for CsharpCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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
            // Counts unary / single-token conditions inside `for`
            // statements. The C# grammar exposes the loop condition via
            // the named `condition` field on `for_statement`, so we look
            // it up by name rather than positional index (Java's arm
            // relies on positional indices because its grammar does not
            // name the field). Comparison-operator conditions like
            // `i < n` are still counted by the standard `GT | LT | ...`
            // arms — this arm only fires when the condition is a bare
            // identifier, invocation, boolean literal, parenthesised
            // expression, or `!`-prefixed unary expression.
            ForStatement => {
                if let Some(condition) = node.child_by_field_name("condition") {
                    let kind = condition.kind_id().into();
                    if matches!(kind, csharp_invocation_expr_kinds!())
                        || matches!(kind, Identifier | BooleanLiteral)
                    {
                        stats.conditions += 1.;
                    } else if matches!(kind, csharp_paren_expr_kinds!())
                        || matches!(kind, csharp_prefix_unary_expr_kinds!())
                    {
                        csharp_inspect_container(&condition, &mut stats.conditions);
                    }
                }
            }
            _ => {}
        }
    }
}

impl Abc for ElixirCode {
    // Elixir's pattern-match `=` is a `BinaryOperator` whose middle
    // child is an `EQ` token. The same wrapper node also hosts `+=`-
    // style augmented assignments, but Elixir is purely functional —
    // augmented assignment does not exist in the grammar; `EQ` is the
    // only assignment-shaped operator. `|>` (`PIPEGT`) is a
    // BinaryOperator too but its operator token differs, so the EQ
    // child check is what filters assignments from pipelines and from
    // comparison operators that share the wrapper.
    //
    // Branches cover `|>` (the pipe operator dispatches one call per
    // step) and every `Call` node (function / method / macro
    // invocation). `RemoteCallWithParentheses` and `LocalCallWith*`
    // variants are subordinate nodes to `Call`, so the single `Call`
    // match captures every dispatch site.
    //
    // Conditions cover `when` (guard token `Elixir::When`), the six
    // comparison operator tokens (`==`, `===`, `!=`, `!==`, `<`, `>`,
    // `<=`, `>=`), and the keyword-shaped `Call`s that introduce a
    // decision point (`if`, `unless`, `case`, `cond`, `with`).
    // `for` / `while` are looping forms — not condition-shaped per
    // the issue body's literal list — so we omit them.
    //
    // Limitations:
    // - `case` is counted once on the container, not once per arm
    //   (`stab_clause`). The issue body says "conditions = case,
    //   cond, if, with, guard when" — i.e. one condition per
    //   construct, not per arm. Matches the Rust impl's "MatchExpression
    //   once" rule.
    // - Higher-order calls like `Enum.reduce` are `RemoteCallWithParentheses`
    //   nodes; they are still `Call` nodes and so contribute one branch
    //   each, matching the issue's "branches = `|>`, function calls"
    //   instruction.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        use Elixir as E;

        match node.kind_id().into() {
            // A `BinaryOperator` whose operator token is `EQ` is a
            // pattern-match assignment. The grammar shape is
            // `(left, operator, right)`, so the operator token is
            // always at child index 1 — looking it up directly is
            // O(1) vs. an `any()` scan of all children. This arm
            // fires on every Elixir binary op (comparisons, pipes,
            // boolean ops, arithmetic) so the constant-time check
            // matters.
            E::BinaryOperator | E::BinaryOperator2 | E::BinaryOperator3
                if node
                    .child(1)
                    .is_some_and(|c| c.kind_id() == E::EQ as u16) =>
            {
                stats.assignments += 1.;
            }
            // `|>` pipeline operator: every step in `foo |> bar |> baz`
            // is one branch (the pipe dispatches one call per step).
            E::PIPEGT => {
                stats.branches += 1.;
            }
            // Every Call (function, method, macro, sigil-call) is one
            // branch — `RemoteCallWith*`, `LocalCallWith*`,
            // `AnonymousCall`, and `DoubleCall` are all subordinate
            // node kinds underneath the top-level `Call` wrapper, so
            // matching `Call` alone captures every dispatch site.
            //
            // Method-defining macros (`def`/`defp`/`defmacro`/`defmacrop`)
            // and module/struct/protocol declarations (`defmodule`/
            // `defstruct`/`defprotocol`/`defimpl`) are *not* runtime
            // dispatch and must not inflate `branches` — they parse as
            // `Call` nodes because Elixir's grammar uses the same
            // shape for all keyword-introduced forms. Aliasing/import
            // directives (`alias`, `import`, `require`, `use`) are
            // similarly declarative and excluded.
            //
            // Cognitive's `elixir_call_keyword` lookup is reused to
            // identify the target keyword. Note: Cognitive only acts
            // on a subset of these keywords (the four method-definers
            // for nesting reset, plus the 7 control-flow keywords for
            // +nesting); Abc's broader filter additionally drops the
            // module/struct/protocol declarators and aliasing
            // directives that Cognitive ignores entirely. Filter sets
            // are intentionally different — both impls use the same
            // helper to look up the keyword, but apply different
            // policies on top.
            E::Call => {
                let keyword = super::cognitive::elixir_call_keyword(node, code);
                let is_definition_or_directive = matches!(
                    keyword,
                    Some(
                        "def" | "defp" | "defmacro" | "defmacrop"
                        | "defmodule" | "defstruct" | "defprotocol" | "defimpl"
                        | "alias" | "import" | "require" | "use"
                    )
                );
                if !is_definition_or_directive {
                    stats.branches += 1.;
                }
                // Keyword-shaped control-flow Calls also contribute
                // one condition.
                if matches!(keyword, Some("if" | "unless" | "case" | "cond" | "with")) {
                    stats.conditions += 1.;
                }
            }
            // Comparison operator tokens. `Elixir::LT` / `Elixir::GT`
            // are unambiguously comparison ops here — unlike Go's
            // generic-instantiation `<` / `>`, Elixir has no type
            // parameter brackets that share the token.
            E::EQEQ | E::EQEQEQ | E::BANGEQ | E::BANGEQEQ
            | E::LT | E::GT | E::LTEQ | E::GTEQ
            // Guard `when` token: introduces the guard clause of a
            // function head or `case` arm.
            | E::When => {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

// Fitzpatrick's ABC rules adapted for Perl.
//
// - Assignments: every assignment operator token — plain `=` plus the
//   compound forms `+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `.=`, `x=`,
//   `&=`, `|=`, `^=`, `<<=`, `>>=`, `&&=`, `||=`, `//=`, and the
//   bitstring forms `&.=`, `|.=`, `^.=`. Each token fires exactly
//   once per textual occurrence inside a `binary_expression`.
// - Branches: every call expression dispatch — `call_expression_with_*`
//   (bareword / spaced args / args-with-brackets / sub / variable /
//   recursive) plus `method_invocation`. The grammar nests an inner
//   `call_expression_with_bareword` (just the function name)
//   underneath the wrapper kinds carrying argument lists, so we only
//   count `CallExpressionWithBareword` when it stands on its own;
//   when its parent is another call form, the outer wrapper has
//   already contributed the branch.
// - Conditions: numeric and string comparison operators (`==`, `!=`,
//   `<`, `>`, `<=`, `>=`, `<=>`, `eq`, `ne`, `lt`, `gt`, `le`, `ge`,
//   `cmp`, `=~`, `!~`), short-circuit / logical operators (`&&`,
//   `||`, `//`, `and`, `or`, `xor`), the ternary operator
//   (`TernaryExpression`), and each `elsif` / `else` clause of an
//   `if` / `unless` statement. Bare predicates that have no
//   comparison (e.g. `if ($x)`) are not separately counted; we let
//   the comparison tokens carry the metric, mirroring the Bash /
//   Python token-level approach.
impl Abc for PerlCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Perl as P;

        match node.kind_id().into() {
            // Plain `=` and every compound assignment operator. The
            // grammar tokenises each operator separately, so one
            // textual `+=` produces exactly one token and there is no
            // double-counting via a wrapper.
            P::EQ
            | P::PLUSEQ
            | P::DASHEQ
            | P::STAREQ
            | P::SLASHEQ
            | P::PERCENTEQ
            | P::STARSTAREQ
            | P::DOTEQ
            | P::XEQ
            | P::AMPEQ
            | P::PIPEEQ
            | P::CARETEQ
            | P::LTLTEQ
            | P::GTGTEQ
            | P::AMPAMPEQ
            | P::PIPEPIPEEQ
            | P::SLASHSLASHEQ
            | P::AMPDOTEQ
            | P::PIPEDOTEQ
            | P::CARETDOTEQ => {
                stats.assignments += 1.;
            }
            // Argument-bearing call wrappers always count.
            P::CallExpressionWithSpacedArgs
            | P::CallExpressionWithSub
            | P::CallExpressionWithArgsWithBrackets
            | P::CallExpressionWithVariable
            | P::CallExpressionRecursive
            | P::MethodInvocation => {
                stats.branches += 1.;
            }
            // Bareword-only call (`shift`, `time`, …) — count only
            // when this node is the outermost dispatch site. When the
            // bareword sits inside one of the wrappers above, the
            // outer node has already been counted and this child
            // would double the branch tally.
            P::CallExpressionWithBareword
                if !node.parent().is_some_and(|p| {
                    matches!(
                        p.kind_id().into(),
                        P::CallExpressionWithSpacedArgs
                            | P::CallExpressionWithSub
                            | P::CallExpressionWithArgsWithBrackets
                            | P::CallExpressionWithVariable
                            | P::CallExpressionRecursive
                    )
                }) =>
            {
                stats.branches += 1.;
            }
            // Numeric, string, and pattern-match comparison operators
            // plus the spaceship / `cmp` three-way comparisons.
            P::EQEQ | P::BANGEQ | P::LT | P::GT | P::LTEQ | P::GTEQ | P::LTEQGT
            | P::Eq | P::Ne | P::Lt | P::Gt | P::Le | P::Ge | P::Cmp
            | P::EQTILDE | P::BANGTILDE
            // Short-circuit / logical operators (high- and low-
            // precedence forms).
            | P::AMPAMP | P::PIPEPIPE | P::SLASHSLASH
            | P::And | P::Or | P::Xor
            // Ternary `a ? b : c` and each `elsif` / `else` clause of
            // an `if` / `unless` chain.
            | P::TernaryExpression
            | P::ElsifClause
            | P::ElseClause => {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

// Fitzpatrick's ABC rules adapted for Lua.
//
// - Assignments: every `assignment_statement` node. Lua has no
//   compound assignment operators (`+=` and friends do not exist in
//   the grammar), so the wrapper kind is the sole assignment node
//   and there is no per-operator alternative to track. `local x = 1`
//   wraps an `assignment_statement` under a `variable_declaration`,
//   so initialisers count the same as later mutations.
// - Branches: every `function_call`. The Lua grammar collapses
//   `obj.method(args)`, `obj:method(args)`, and `f(args)` into the
//   same `function_call` node, so one arm covers all dispatch forms.
// - Conditions: comparison operators (`==`, `~=`, `<`, `>`, `<=`,
//   `>=`), short-circuit operators (`and`, `or`), each elseif / else
//   arm of an `if`. Lua has no ternary operator (`cond and a or b`
//   is the idiom, captured by the `and` / `or` tokens above).
impl Abc for LuaCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        match node.kind_id().into() {
            Lua::AssignmentStatement | Lua::AssignmentStatement2 => {
                stats.assignments += 1.;
            }
            Lua::FunctionCall => {
                stats.branches += 1.;
            }
            Lua::EQEQ
            | Lua::TILDEEQ
            | Lua::LT
            | Lua::GT
            | Lua::LTEQ
            | Lua::GTEQ
            | Lua::And
            | Lua::Or
            | Lua::ElseifStatement
            | Lua::ElseStatement => {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

// Names of Tcl commands that mutate a variable. Each invocation of
// one of these commands counts as an assignment, not a branch — the
// command is acting as an assignment operator, not as a generic
// dispatch. The list is intentionally narrow: only commands that
// every Tcl programmer recognises as primary mutators. Less-common
// mutators (`dict set`, `array set`, `lset`, `regsub … name`) are
// left as branches; treating them as assignments would require
// inspecting the command's second word, and the additional
// fidelity is not worth the complexity for the ABC magnitude.
const TCL_ASSIGNMENT_COMMANDS: &[&[u8]] = &[b"incr", b"append", b"lappend"];

// Fitzpatrick's ABC rules adapted for Tcl.
//
// - Assignments: every `set` production (`set name value`) plus
//   every `command` whose first word is one of the recognised
//   mutator commands in `TCL_ASSIGNMENT_COMMANDS`. Tcl has no
//   assignment operators — variable mutation is always a command
//   invocation, so we filter on the command name. The `set` form
//   has its own grammar production (`Tcl::Set`) and counts directly
//   without any source-text inspection.
// - Branches: every other `command` node. Like Bash, `return` and
//   `error` builtins parse as plain `command` nodes and count here
//   too — Tcl treats every dispatch the same regardless of whether
//   the command is a procedure call, a control-flow primitive, or a
//   builtin. The grammar productions for `if`, `while`, `foreach`,
//   etc. live separately from `command` and do not double-count.
// - Conditions: numeric (`==`, `!=`, `<`, `>`, `<=`, `>=`) and
//   string (`eq`, `ne`, `in`, `ni`) comparison tokens, short-circuit
//   operators (`&&`, `||`), the ternary expression production, and
//   each `elseif` / `else` clause of an `if`.
impl Abc for TclCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        match node.kind_id().into() {
            // The `set` production wraps `set name value` as a
            // first-class node distinct from generic commands.
            Tcl::Set => {
                stats.assignments += 1.;
            }
            // Generic command: branch by default, assignment when
            // the first word names a known mutator. The first word
            // can be either a `simple_word` or a wrapped form; both
            // surface their literal text via `utf8_text`.
            Tcl::Command => {
                if tcl_command_is_assignment(node, code) {
                    stats.assignments += 1.;
                } else {
                    stats.branches += 1.;
                }
            }
            Tcl::EQEQ
            | Tcl::BANGEQ
            | Tcl::LT
            | Tcl::GT
            | Tcl::LTEQ
            | Tcl::GTEQ
            | Tcl::Eq
            | Tcl::Ne
            | Tcl::In
            | Tcl::Ni
            | Tcl::AMPAMP
            | Tcl::PIPEPIPE
            | Tcl::TernaryExpr
            | Tcl::Elseif
            | Tcl::Else => {
                stats.conditions += 1.;
            }
            _ => {}
        }
    }
}

// Returns true when the `command` node's first word is one of the
// recognised Tcl assignment commands. The first word is the leftmost
// non-comment child; we slice the source bytes directly using the
// child node's byte range, which is robust to `simple_word` wrappers
// and avoids depending on a particular grammar shape.
fn tcl_command_is_assignment(node: &Node, code: &[u8]) -> bool {
    let Some(first) = node.child(0) else {
        return false;
    };
    let start = first.start_byte();
    let end = first.end_byte();
    if end > code.len() || start >= end {
        return false;
    }
    let word = &code[start..end];
    TCL_ASSIGNMENT_COMMANDS.contains(&word)
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

    /// Regression for #227: a `Stats::default()` that never sees an
    /// observation must not leak the `f64::MAX` sentinel for
    /// `assignments_min`, `branches_min`, or `conditions_min`. All
    /// three getters collapse the sentinel to `0.0` so JSON never
    /// emits `1.7976931e308`.
    #[test]
    fn abc_empty_file_min_is_zero() {
        let stats = Stats::default();
        assert_eq!(stats.assignments_min(), 0.0);
        assert_eq!(stats.branches_min(), 0.0);
        assert_eq!(stats.conditions_min(), 0.0);
    }

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
    fn groovy_no_abc() {
        // Comment-only file has no executable code → all-zero ABC.
        check_metrics::<GroovyParser>(
            "// just a comment, no executable code",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
            },
        );
    }

    #[test]
    fn groovy_single_assignment() {
        // `int x = 1` is a local-variable declaration whose `=` counts
        // as one assignment (matches Java's semantics).
        check_metrics::<GroovyParser>("int x = 1", "foo.groovy", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 1.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
        });
    }

    #[test]
    fn groovy_assignments() {
        check_metrics::<GroovyParser>(
            "void f() {
                int a = 1
                int b = 2
                a = 3
                b = 4
                a += 1
                b -= 1
            }",
            "foo.groovy",
            |metric| {
                // Six `=` tokens total. The two `Final`-less local
                // var-decls (`int a = 1`, `int b = 2`) and the two
                // bare assignments (`a = 3`, `b = 4`) each contribute
                // one assignment via the `EQ` arm; the `+=` / `-=`
                // each contribute one via the compound-assign arm.
                assert_eq!(metric.abc.assignments_sum(), 6.0);
            },
        );
    }

    #[test]
    fn groovy_branches() {
        check_metrics::<GroovyParser>(
            "void f() {
                doStuff()
                helper.invoke()
                new Worker()
            }",
            "foo.groovy",
            |metric| {
                // 2 method invocations + 1 object creation = 3 branches
                assert_eq!(metric.abc.branches_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_conditions_in_if() {
        check_metrics::<GroovyParser>(
            "void f(int a) {
                if (a == 0) { println(a) }
                if (a >= 1) { println(a) }
                if (a != 2) { println(a) }
            }",
            "foo.groovy",
            |metric| {
                // Three relational ops = 3 conditions
                assert_eq!(metric.abc.conditions_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_branches_with_juxt_call() {
        // Groovy's parens-less call form `println foo` must be counted
        // as a branch (`JuxtFunctionCall`).
        check_metrics::<GroovyParser>(
            "void f() {
                println 'hi'
                println 'bye'
            }",
            "foo.groovy",
            |metric| {
                // 2 juxt calls = 2 branches.
                assert_eq!(metric.abc.branches_sum(), 2.0);
            },
        );
    }

    #[test]
    fn groovy_try_catch_conditions() {
        // Each `try` and `catch` keyword token contributes +1 to
        // conditions (mirrors Java).
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
                // try + catch = 2 conditions
                assert_eq!(metric.abc.conditions_sum(), 2.0);
            },
        );
    }

    #[test]
    fn groovy_ternary_conditions() {
        check_metrics::<GroovyParser>(
            "void f(int x) {
                def y = x > 0 ? 1 : 2
            }",
            "foo.groovy",
            |metric| {
                // QMARK alone is +1 condition, plus the `>` condition = 2.
                assert_eq!(metric.abc.conditions_sum(), 2.0);
            },
        );
    }

    #[test]
    fn groovy_constant_excluded_from_assignments() {
        // `final` declarations are not counted as assignments
        // (mirrors Java's `Final` handling).
        check_metrics::<GroovyParser>(
            "class A {
                final int CONST = 42
                int field = 0
            }",
            "foo.groovy",
            |metric| {
                // The `=` on `final int CONST = 42` is a constant
                // initialiser (skipped). Only `field = 0` counts.
                assert_eq!(metric.abc.assignments_sum(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_malformed_parenthesized_no_panic() {
        // Regression: malformed Groovy input must not panic the ABC
        // walker; the `spaces.rs` Unit fallback (lesson 9) covers
        // structural recovery. amaanq's grammar treats `def x = (((`
        // as a `local_variable_declaration` whose initialiser is the
        // first opening paren — the `=` still fires the assignment
        // arm.
        check_metrics::<GroovyParser>("def x = (((", "foo.groovy", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 1.0);
        });
    }

    #[test]
    fn groovy_if_multiple_conditions() {
        // Mirrors `java_if_multiple_conditions`: `&&` / `||` chains
        // and parenthesised unary forms each contribute one
        // condition per primitive comparison; the inspect-container
        // pass picks up the unary `!a` / `!b` arguments inside the
        // `BinaryExpression` and counts them too.
        check_metrics::<GroovyParser>(
            "void f(boolean a, boolean b, boolean c) {
                if (a || b || c) { println(a) }
                if (a && b && c) { println(a) }
                if (!a && !b) { println(a) }
            }",
            "foo.groovy",
            |metric| {
                // Conditions counted via the AMPAMP/PIPEPIPE arms
                // (one count per identifier in the chain — three
                // for `||`, three for `&&`, two for the unary chain)
                // = 8.
                assert_eq!(metric.abc.conditions_sum(), 8.0);
                // Three `println a` juxt calls — each is a branch.
                assert_eq!(metric.abc.branches_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_while_and_do_while_conditions() {
        // Covers the WhileStatement and DoStatement arms in
        // `impl Abc for GroovyCode`. Each `while` / `do-while` has
        // its condition inspected through `groovy_inspect_container`.
        check_metrics::<GroovyParser>(
            "void f(boolean a, boolean b) {
                while (a) {
                    a = false
                }
                do {
                    b = !b
                } while (b)
            }",
            "foo.groovy",
            |metric| {
                // `while(a)` + `while(b)` each contribute one condition;
                // the unary `!b` on the do body's right-hand side adds
                // one more via the assignment-arm inspection = 3.
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                // Two assignments to existing variables (`a = false`,
                // `b = !b`).
                assert_eq!(metric.abc.assignments_sum(), 2.0);
            },
        );
    }

    #[test]
    fn groovy_methods_arguments_with_conditions() {
        // Mirror of `java_methods_arguments_with_conditions`: a
        // unary `!x` inside an argument list must count both the
        // method invocation as a branch AND the unary as a
        // condition. The `ArgumentList | ArgumentList2` arm in
        // `impl Abc for GroovyCode` is what exercises this.
        check_metrics::<GroovyParser>(
            "void f(boolean a, boolean b, boolean c) {
                m1(a)
                m1(!a)
                m2(!a, !b)
            }",
            "foo.groovy",
            |metric| {
                // 3 method invocations (m1, m1, m2) — each fires the
                // branches arm.
                assert_eq!(metric.abc.branches_sum(), 3.0);
                // Three `!` unaries — `m1(!a)` and the two args of
                // `m2(!a, !b)` — each contribute one condition via
                // the ArgumentList inspection.
                assert_eq!(metric.abc.conditions_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_return_with_conditions() {
        // Mirror of `java_return_with_conditions`: a parenthesised
        // or unary expression inside `return` flows through the
        // `ReturnStatement` arm to `groovy_inspect_container`.
        check_metrics::<GroovyParser>(
            "boolean f(boolean a) {
                return (a)
            }
            boolean g(boolean a) {
                return !a
            }",
            "foo.groovy",
            |metric| {
                // Only one of the two return forms surfaces a
                // condition: `return !a` hits the UnaryExpression
                // path and adds one; `return (a)` reaches
                // `groovy_inspect_container` but the inner
                // identifier `a` is not in a boolean-context-firing
                // parent, so no condition is added.
                assert_eq!(metric.abc.conditions_sum(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_for_with_variable_declaration() {
        // Classical `for (int i = 0; cond; i++)` form. The init
        // slot's `int i = 0` is suppressed from assignments by the
        // `LocalVariableDeclaration` push/pop dance; the `i++` in
        // the update slot contributes one assignment via the
        // `PLUSPLUS` arm. The condition `i < 10` flows through the
        // `ForStatement` arm.
        check_metrics::<GroovyParser>(
            "void f() {
                for (int i = 0; i < 10; i++) {
                    println(i)
                }
            }",
            "foo.groovy",
            |metric| {
                // `int i = 0` fires the EQ arm + `i++` fires the
                // PLUSPLUS arm = 2 assignments.
                assert_eq!(metric.abc.assignments_sum(), 2.0);
                // `i < 10` is one condition (the LT arm).
                assert_eq!(metric.abc.conditions_sum(), 1.0);
            },
        );
    }

    #[test]
    fn groovy_eq_arm_increments_when_no_declaration() {
        // Bare reassignment of an already-declared variable: the
        // `EQ` arm fires when the declaration stack is empty
        // (`stats.declaration.last().is_none()`), so the `=` counts
        // as one assignment. Mirrors `java_eq_arm_increments_when_
        // declaration_stack_is_empty`.
        check_metrics::<GroovyParser>(
            "void f(int x) {
                x = 42
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
            },
        );
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
    fn csharp_for_identifier_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                void M(bool ready) {
                    for (; ready ;) { }
                }
            }",
            "foo.cs",
            |metric| {
                // expected: assignments=0 (no `=` / `++` / `--`),
                // branches=0 (no invocation / object creation),
                // conditions=1 (bare-identifier for-loop condition).
                // Averages divide by 3 spaces (top-level + class + method).
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                {
                  "assignments": 0.0,
                  "branches": 0.0,
                  "conditions": 1.0,
                  "magnitude": 1.0,
                  "assignments_average": 0.0,
                  "branches_average": 0.0,
                  "conditions_average": 0.3333333333333333,
                  "assignments_min": 0.0,
                  "assignments_max": 0.0,
                  "branches_min": 0.0,
                  "branches_max": 0.0,
                  "conditions_min": 0.0,
                  "conditions_max": 1.0
                }
                "###
                );
            },
        );
    }

    #[test]
    fn csharp_for_invocation_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                bool Ok() { return true; }
                void M() {
                    for (; Ok() ;) { }
                }
            }",
            "foo.cs",
            |metric| {
                // expected: assignments=0, branches=1 (the `Ok()` call),
                // conditions=1 (invocation as for-loop condition).
                // Averages divide by 4 spaces (top-level + class + two
                // methods).
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                {
                  "assignments": 0.0,
                  "branches": 1.0,
                  "conditions": 1.0,
                  "magnitude": 1.4142135623730951,
                  "assignments_average": 0.0,
                  "branches_average": 0.25,
                  "conditions_average": 0.25,
                  "assignments_min": 0.0,
                  "assignments_max": 0.0,
                  "branches_min": 0.0,
                  "branches_max": 1.0,
                  "conditions_min": 0.0,
                  "conditions_max": 1.0
                }
                "###
                );
            },
        );
    }

    // Regression coverage for #279: the C# grammar wraps a literal
    // `true` / `false` for-loop condition in a `boolean_literal` node.
    // The `BooleanLiteral` arm in the `ForStatement` dispatch must
    // attribute one condition; without it, `for (; true ;)` would
    // contribute 0 (the bug fixed by this commit also affected this
    // shape).
    #[test]
    fn csharp_for_boolean_literal_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                void M() {
                    for (; true ;) { }
                }
            }",
            "foo.cs",
            |metric| {
                // expected: assignments=0, branches=0,
                // conditions=1 (the `true` literal as condition).
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
            },
        );
    }

    // Regression coverage for #279: an empty for-loop condition such as
    // `for (; ;) {}` must contribute 0 to conditions — there is no
    // condition node to count.
    #[test]
    fn csharp_for_empty_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                void M() {
                    for (; ;) { }
                }
            }",
            "foo.cs",
            |metric| {
                // expected: assignments=0, branches=0, conditions=0
                // (no condition expression in `for (; ;)`).
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r###"
                {
                  "assignments": 0.0,
                  "branches": 0.0,
                  "conditions": 0.0,
                  "magnitude": 0.0,
                  "assignments_average": 0.0,
                  "branches_average": 0.0,
                  "conditions_average": 0.0,
                  "assignments_min": 0.0,
                  "assignments_max": 0.0,
                  "branches_min": 0.0,
                  "branches_max": 0.0,
                  "conditions_min": 0.0,
                  "conditions_max": 0.0
                }
                "###
                );
            },
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

    // --- Ruby ABC tests ---------------------------------------------------
    //
    // Each Ruby `assignment` / `operator_assignment` is one assignment
    // regardless of whether the LHS is a local, instance, or class
    // variable. Every `call` / `super` / `yield` is one branch. Every
    // comparison-operator token inside a `binary` node plus each
    // `else` / `elsif` / `when` / `then` / `?` / `rescue` clause is
    // one condition.

    #[test]
    fn ruby_zero_abc() {
        check_metrics::<RubyParser>("\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn ruby_simple_assignment() {
        check_metrics::<RubyParser>("def f\n  a = 1\n  b = 2\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 2.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn ruby_augmented_assignment() {
        // `+=`, `-=`, `*=` are `operator_assignment` nodes — each is
        // one assignment. Plain `=` to set the initial value adds one
        // more.
        check_metrics::<RubyParser>(
            "def f(x)\n  a = 0\n  a += x\n  a -= 1\n  a *= 2\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_logical_augmented_assignment() {
        // `||=` and `&&=` are also `operator_assignment` nodes.
        check_metrics::<RubyParser>("def f\n  @x ||= 0\n  @x &&= 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 2.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn ruby_method_call_branch() {
        // Each method invocation is one branch.
        check_metrics::<RubyParser>(
            "def f(obj)\n  foo()\n  obj.bar(1)\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_super_and_yield_branches() {
        // `super` and `yield` both count as branches (control-pass).
        check_metrics::<RubyParser>("def f\n  super\n  yield\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.branches_sum(), 2.0);
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn ruby_attr_macro_is_branch() {
        // `attr_accessor` is a `Call3` node and registers as a branch
        // like any method invocation.
        check_metrics::<RubyParser>("class A\n  attr_accessor :x\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.branches_sum(), 1.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn ruby_comparison_conditions() {
        // Each comparison operator is one condition.
        check_metrics::<RubyParser>(
            "def f(a, b)\n  a == b\n  a != b\n  a < b\n  a > b\n  a <= b\n  a >= b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 6.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_spaceship_and_case_equality() {
        // `<=>` and `===` are comparison operators (conditions).
        check_metrics::<RubyParser>(
            "def f(a, b)\n  a <=> b\n  a === b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_ternary_condition() {
        // The `?` ternary marker is one condition; the inner `==` is
        // another.
        check_metrics::<RubyParser>("def f(x)\n  x == 0 ? :z : :nz\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn ruby_case_when_arms() {
        // Each `when` named clause and the `else` clause count as one
        // condition each; the `case` head and the implicit `then`
        // wrappers do not.
        check_metrics::<RubyParser>(
            "def f(x)\n  case x\n  when 1 then 'one'\n  when 2 then 'two'\n  else 'other'\n  end\nend\n",
            "foo.rb",
            |metric| {
                // 2 `when` + 1 `else` = 3 conditions.
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_elsif_and_else() {
        // `elsif` and `else` named clauses are conditions; their inner
        // `then` wrappers are not.
        check_metrics::<RubyParser>(
            "def f(x)\n  if x > 0\n    1\n  elsif x < 0\n    -1\n  else\n    0\n  end\nend\n",
            "foo.rb",
            |metric| {
                // `>`(1) + `elsif`(1) + `<`(1) + `else`(1) = 4.
                assert_eq!(metric.abc.conditions_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_rescue_clause_condition() {
        // The `rescue` named clause is one condition; the `rescue`
        // keyword token (`Rescue2`) is not counted on its own.
        // `do_it` without parens is an `identifier`, not a `call`, so
        // it contributes no branch. `handle(e)` is a `call` (1 branch).
        check_metrics::<RubyParser>(
            "def f\n  begin\n    do_it\n  rescue StandardError => e\n    handle(e)\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_class_complex_function() {
        // Mixed: assignment(=), branch(call), conditions(`>` and `==`).
        check_metrics::<RubyParser>(
            "class A\n  def f(a, b)\n    sum = a + b\n    if sum > 0 && b == 0\n      foo(sum)\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 1.0);
                // `>`(1) + `==`(1) = 2 conditions. `if` is not a token;
                // `&&` is `AMPAMP` which is NOT a Fitzpatrick condition
                // in our Ruby impl (it's a logical operator, not a
                // comparison). The Fitzpatrick paper allows either
                // choice; we follow the comparison-only rule like
                // Java/PHP.
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ---------------------------------------------------------------
    // Default-impl placeholder smoke tests (audited in #188).
    //
    // These tests assert that the *current* default-impl languages
    // return ABC = 0/0/0 for source that DOES contain branches,
    // conditions, and assignments. When the real impl lands for any
    // of these languages, the corresponding assertion below will fire
    // — the implementer must update the expected values, which is the
    // gate. Tag the follow-up issue in each test.
    // ---------------------------------------------------------------

    // --- Python ABC ---------------------------------------------------

    #[test]
    fn python_empty_module_zero() {
        check_metrics::<PythonParser>("", "empty.py", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_plain_assignments_count() {
        // Three plain `=` assignments → A=3. No branches, no conditions.
        check_metrics::<PythonParser>("x = 1\ny = 2\nz = x\n", "foo.py", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 3.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_typed_assignment_counts_bare_annotation_does_not() {
        // `x: int = 1` carries an `=`, so it counts.
        // `y: int` is a bare annotation (no `=`) — declares a type but
        // binds nothing; it must NOT inflate the assignment count.
        check_metrics::<PythonParser>("x: int = 1\ny: int\n", "foo.py", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 1.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_augmented_assignments_count() {
        // Each augmented op counts once.
        check_metrics::<PythonParser>("x = 0\nx += 1\nx -= 1\nx *= 2\n", "foo.py", |metric| {
            // 1 plain `=` + 3 augmented = 4 assignments.
            assert_eq!(metric.abc.assignments_sum(), 4.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_walrus_counts_as_assignment() {
        // `x := 10` is a `NamedExpression` (PEP 572). It binds a value
        // → one assignment under Fitzpatrick's rule.
        check_metrics::<PythonParser>("if (n := 10) > 5:\n    pass\n", "foo.py", |metric| {
            // 1 assignment (walrus) + 1 condition (`> 5` is a
            // ComparisonOperator).
            assert_eq!(metric.abc.assignments_sum(), 1.0);
            assert_eq!(metric.abc.conditions_sum(), 1.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_calls_are_branches() {
        // `foo()`, `bar()`, `Baz()` (constructor) all parse as `Call`
        // → three branches.
        check_metrics::<PythonParser>(
            "def foo():\n    pass\ndef bar():\n    pass\nclass Baz:\n    pass\nfoo()\nbar()\nBaz()\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 3.0);
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_comparisons_count_conditions() {
        // `x > 0`, `x == y`, `x is None` are each a single
        // `ComparisonOperator` node — three conditions.
        check_metrics::<PythonParser>(
            "def f(x, y):\n    a = x > 0\n    b = x == y\n    c = x is None\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                // 3 plain assignments; the comparisons are operands.
                assert_eq!(metric.abc.assignments_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_chained_comparison_counts_once() {
        // tree-sitter-python collapses `0 < x < 10` into a single
        // `ComparisonOperator` — one condition, not two.
        check_metrics::<PythonParser>("def f(x):\n    return 0 < x < 10\n", "foo.py", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 1.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_boolean_operators_count_conditions() {
        // `and` / `or` are each a `BooleanOperator` node → one condition
        // per logical-binop instance.
        check_metrics::<PythonParser>(
            "def f(a, b, c):\n    if a and b or c:\n        pass\n",
            "foo.py",
            |metric| {
                // `a and b or c` parses as `BooleanOperator(or,
                // BooleanOperator(and, a, b), c)` → 2 BooleanOperator
                // nodes → 2 conditions.
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    /// Python's unary `not` operator parses as `NotOperator` and now
    /// counts as one condition, matching Java's `!x` rule. Closes
    /// the parity gap noted in #214: without this, `if not flag:`
    /// reported 0 conditions while the Java equivalent reports 1.
    #[test]
    fn python_unary_not_counts_as_condition() {
        check_metrics::<PythonParser>(
            "def f(flag):\n    if not flag:\n        return 1\n    return 0\n",
            "foo.py",
            |metric| {
                // One `NotOperator` -> 1 condition. The `if` itself
                // is structural and doesn't add an Abc condition.
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    /// `return not flag` — the unary `not` is the entire return
    /// expression. Without `NotOperator` counted, this reports zero
    /// conditions; with it, one. Java's `return !flag;` is one.
    #[test]
    fn python_return_unary_not_counts() {
        check_metrics::<PythonParser>("def f(flag):\n    return not flag\n", "foo.py", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 1.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    /// `foo(not ready, value)` — the unary `not` inside an argument
    /// list still contributes. Mirrors Java's
    /// `java_count_unary_conditions` walk over argument lists.
    #[test]
    fn python_unary_not_in_argument_list_counts() {
        check_metrics::<PythonParser>(
            "def f(ready, value):\n    log(not ready, value)\n",
            "foo.py",
            |metric| {
                // 1 Call (log) -> 1 branch.
                // 1 NotOperator (not ready) -> 1 condition.
                assert_eq!(metric.abc.branches_sum(), 1.0);
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    /// Nested `not` + comparison counts each unique node once.
    /// `not (x > 0)` parses as `NotOperator(ParenthesizedExpression(
    /// ComparisonOperator))`; both the unary and the comparison
    /// contribute one condition (mirrors Java's `!(x > 0)` = 2
    /// conditions).
    #[test]
    fn python_unary_not_with_comparison_counts_each_once() {
        check_metrics::<PythonParser>(
            "def f(x):\n    if not (x > 0):\n        return 1\n    return 0\n",
            "foo.py",
            |metric| {
                // NotOperator (1) + ComparisonOperator (1) = 2.
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    /// `not x and y` parses as `BooleanOperator(NotOperator(x), and,
    /// y)`. The BooleanOperator counts (and/or = 1 condition); the
    /// inner NotOperator also counts. Total: 2.
    #[test]
    fn python_unary_not_with_boolean_combinator_counts_each() {
        check_metrics::<PythonParser>(
            "def f(x, y):\n    if not x and y:\n        return 1\n    return 0\n",
            "foo.py",
            |metric| {
                // BooleanOperator (1) + NotOperator (1) = 2.
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_control_flow_arms_count_conditions() {
        // `elif`, `else`, `except`, `finally`, `case` each contribute
        // one condition. The comparisons in the `if`/`elif`/`while`
        // headers contribute their own ComparisonOperator counts.
        check_metrics::<PythonParser>(
            "def f(x):\n    if x > 0:\n        a = 1\n    elif x > -1:\n        a = 2\n    else:\n        a = 3\n",
            "foo.py",
            |metric| {
                // 2 ComparisonOperator (`x > 0`, `x > -1`) + 1
                // ElifClause + 1 ElseClause = 4 conditions.
                assert_eq!(metric.abc.conditions_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_ternary_counts_as_condition() {
        // `a if c else b` is `ConditionalExpression` → 1 condition.
        // `c > 0` adds 1 more (ComparisonOperator).
        check_metrics::<PythonParser>(
            "def f(c):\n    return 1 if c > 0 else 0\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_try_except_finally_count_conditions() {
        // ExceptClause + FinallyClause → 2 conditions.
        check_metrics::<PythonParser>(
            "def f():\n    try:\n        pass\n    except ValueError:\n        pass\n    finally:\n        pass\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_match_case_counts_conditions() {
        // Each non-wildcard `CaseClause` → 1 condition. The bare
        // `case _:` arm is the language-neutral `default:` equivalent
        // and is excluded (matches Rust's bare-`_` MatchArm filter and
        // Java/C#'s `default:` rule). Source has `case 1:` (counts) +
        // `case _:` (excluded) → C = 1.
        check_metrics::<PythonParser>(
            "def f(x):\n    match x:\n        case 1:\n            pass\n        case _:\n            pass\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_match_case_guarded_wildcard_counts() {
        // `case _ if g:` is NOT a bare wildcard — the guard
        // contributes real branching, so the arm counts as a
        // condition. Mirrors Rust's `_ if g => ...` behavior.
        // Source: `case 1:` (counts) + `case _ if x > 0:` (guarded
        // wildcard, counts) + `case _:` (bare wildcard, excluded) →
        // C from CaseClause = 2; the guard's `x > 0` adds one
        // ComparisonOperator → total C = 3.
        check_metrics::<PythonParser>(
            "def f(x):\n    match x:\n        case 1:\n            pass\n        case _ if x > 0:\n            pass\n        case _:\n            pass\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_complex_function_abc() {
        // Mixed-shape regression: assignments, calls, conditions all in
        // a single function.
        check_metrics::<PythonParser>(
            "def f(items, threshold):\n\
             \x20   result = []\n\
             \x20   for item in items:\n\
             \x20       if item > threshold:\n\
             \x20           result.append(item)\n\
             \x20   return result\n",
            "foo.py",
            |metric| {
                // assignments: `result = []` → 1
                // branches: `result.append(item)` is one call → 1
                // conditions: `item > threshold` is one
                // ComparisonOperator → 1
                assert_eq!(metric.abc.assignments_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 1.0);
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_empty_unit_zero() {
        // No code at all → A=B=C=0. Establishes the trait is wired up
        // and the per-language compute is reachable.
        check_metrics::<RustParser>("", "empty.rs", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn rust_assignments_count_outside_let() {
        // `let x = 0` is a declaration — its `=` is NOT a Fitzpatrick
        // assignment (mirrors Java's local-variable-declaration rule).
        // `x = 5` and `x = 7` are plain `=` assignments → 2. `x += 2`
        // is a compound assignment → 1. Total A = 3.
        check_metrics::<RustParser>(
            "fn f() { let mut x = 0; x = 5; x += 2; x = 7; }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 3.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_calls_are_branches() {
        // Free function call + method call (parses as call_expression
        // with a field_expression callee) + associated-fn call. All
        // three are `call_expression` → B = 3. Macro invocations like
        // `println!` parse as `macro_invocation`, NOT `call_expression`,
        // so they are not branches.
        check_metrics::<RustParser>(
            "fn f() { g(); 1.to_string(); String::new(); }\nfn g() {}\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 3.0);
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_try_operator_is_branch() {
        // `?` parses as `try_expression` and counts as one branch
        // (short-circuit return on Err / None). The `Err(())` call
        // contributes one branch in addition (call_expression).
        check_metrics::<RustParser>(
            "fn f() -> Result<i32, ()> { let r: Result<i32, ()> = Err(()); Ok(r?) }",
            "foo.rs",
            |metric| {
                // Err(()) + Ok(...) + r? → 2 calls + 1 try = 3 branches.
                assert_eq!(metric.abc.branches_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_comparisons_count_conditions() {
        // `<`, `>`, `<=`, `>=`, `==`, `!=` each count once. Six
        // comparisons → C = 6.
        check_metrics::<RustParser>(
            "fn f(a: i32, b: i32) -> bool { a < b || a > b || a <= b || a >= b || a == b || a != b }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 6.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_generic_brackets_not_conditions() {
        // `<` / `>` in `Vec<i32>` are TypeArguments delimiters, not
        // comparison operators. The parent-check in the LT/GT arms
        // must filter them out. Expected C = 0.
        check_metrics::<RustParser>(
            "fn f() -> Vec<i32> { Vec::<i32>::new() }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_if_let_counts_as_condition() {
        // `if let Some(v) = opt { ... }` introduces a `let_condition`
        // → 1 condition. The `if` keyword itself does not add another
        // count — Fitzpatrick counts conditions, not branch keywords.
        check_metrics::<RustParser>(
            "fn f(opt: Option<i32>) { if let Some(_v) = opt { } }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_while_let_counts_as_condition() {
        // `while let Some(y) = it.next() { ... }` is also a
        // `let_condition` (the `while` form). One condition; the
        // `it.next()` call adds one branch.
        check_metrics::<RustParser>(
            "fn f(mut it: std::vec::IntoIter<i32>) { while let Some(_y) = it.next() { } }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_match_arms_count_conditions_wildcard_excluded() {
        // Three arms: `0 => 1`, `n if n > 0 => n`, `_ => -1`. The
        // bare wildcard is the `default:` equivalent and is skipped.
        // The guarded arm has a `n if n > 0` pattern (more than one
        // child in the match_pattern) and still counts. Two non-wildcard
        // arms → C = 2 from MatchArm. Plus the comparison `n > 0`
        // adds one more → C = 3.
        check_metrics::<RustParser>(
            "fn f(x: i32) -> i32 { match x { 0 => 1, n if n > 0 => n, _ => -1, } }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_else_counts_as_condition() {
        // `if a > b { ... } else { ... }` → `a > b` is one condition,
        // `else` is one condition → C = 2.
        check_metrics::<RustParser>(
            "fn f(a: i32, b: i32) -> i32 { if a > b { a } else { b } }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_complex_function_abc() {
        // Mixed-shape regression: assignments, calls, conditions, `?`,
        // `if let`, `match` in one body. Verified by hand:
        // - assignments: `x = 5`, `x += 2` → A = 2 (the `let` initialisers
        //   are not assignments).
        // - branches: `xs.iter()`, `.next()`, `Err(())`, `r?` → B = 4
        //   (3 calls + 1 try).
        // - conditions: `if let Some(v) = opt` → 1, `match x` arms
        //   `0`, `n if n>0` (wildcard excluded) → 2, `n > 0` → 1.
        //   Total C = 4.
        check_metrics::<RustParser>(
            "fn f(opt: Option<i32>, xs: Vec<i32>) -> Result<i32, ()> {\n\
             \x20   let mut x = 0;\n\
             \x20   x = 5;\n\
             \x20   x += 2;\n\
             \x20   if let Some(_v) = opt { }\n\
             \x20   let _ = xs.iter().next();\n\
             \x20   let r: Result<i32, ()> = Err(());\n\
             \x20   let _v = r?;\n\
             \x20   Ok(match x {\n\
             \x20       0 => 1,\n\
             \x20       n if n > 0 => n,\n\
             \x20       _ => -1,\n\
             \x20   })\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2.0);
                // calls: xs.iter(), .next(), Err(()), Ok(...) → 4 calls
                // plus 1 try (`r?`) → 5 branches.
                assert_eq!(metric.abc.branches_sum(), 5.0);
                // 1 let_condition + 2 non-wildcard match_arms + 1
                // comparison (`n > 0`) → 4.
                assert_eq!(metric.abc.conditions_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ----- Go -----

    #[test]
    fn go_empty_unit_zero() {
        // Package declaration only — no Fitzpatrick events. Confirms the
        // GoCode Abc trait is wired up and emits zero counts.
        check_metrics::<GoParser>("package main\n", "empty.go", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn go_assignments_count_plain_compound_short_var_and_incdec() {
        // `x := 0` (short var decl), `x = 5` and `x = 7` (plain `=`),
        // `x += 2` (compound), `x++` (inc) → A = 5. `var y = 1` is a
        // declaration — its `=` is not counted (matches the Rust/Java
        // rule for `let` / `int y = 1`).
        check_metrics::<GoParser>(
            "package main\nfunc f() { var y = 1; _ = y; x := 0; x = 5; x += 2; x = 7; x++ }\n",
            "foo.go",
            |metric| {
                // `_ = y` is itself an assignment_statement → +1.
                // x:= + x=5 + x+=2 + x=7 + x++ + _=y → 6
                assert_eq!(metric.abc.assignments_sum(), 6.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_calls_are_branches() {
        // Three calls: free function `g()`, method call `r.Inc()`, and
        // builtin call `len(s)`. All parse as `call_expression` → B = 3.
        // Composite literal `Foo{}` is NOT a call.
        check_metrics::<GoParser>(
            "package main\n\
             type R struct{}\n\
             func (r R) Inc() {}\n\
             func g() {}\n\
             func f(s string) { g(); var r R = R{}; r.Inc(); _ = len(s) }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_comparisons_count_conditions() {
        // `<`, `>`, `<=`, `>=`, `==`, `!=` each count once. Six
        // comparisons → C = 6.
        check_metrics::<GoParser>(
            "package main\nfunc f(a, b int) bool { return a < b || a > b || a <= b || a >= b || a == b || a != b }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 6.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_generic_brackets_not_conditions() {
        // Generic instantiation `Min[int](a, b)` puts `int` inside
        // `TypeArguments`, not `BinaryExpression`. The parent guard on
        // `<` / `>` must not count these. Expected C = 0; B = 1 (one call).
        check_metrics::<GoParser>(
            "package main\nfunc Min[T int | float64](a, b T) T { return a }\nfunc f() { _ = Min[int](1, 2) }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_switch_arms_count_conditions_default_excluded() {
        // Four arms: `case 1:`, `case 2:`, `case 3:`, `default:`. The
        // bare `default` is the C/Java `default:` equivalent and is
        // excluded — 3 conditions from ExpressionCase. The switch
        // expression `x` is bare (no comparison), so no extra
        // condition from `==`-style operators.
        check_metrics::<GoParser>(
            "package main\nfunc f(x int) int { switch x { case 1: return 1; case 2: return 2; case 3: return 3; default: return 0 } }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_type_switch_arms_count_conditions() {
        // Type switch: `case int:`, `case string:`, `default:`. Two
        // non-default type-case arms → C = 2.
        check_metrics::<GoParser>(
            "package main\nfunc f(v interface{}) { switch v.(type) { case int: return; case string: return; default: return } }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_select_arms_count_conditions() {
        // `select { case <-ch: ...; case ch <- 1: ...; default: ... }`.
        // Two non-default communication cases → C = 2.
        check_metrics::<GoParser>(
            "package main\nfunc f(ch chan int) { select { case <-ch: return; case ch <- 1: return; default: return } }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_else_counts_as_condition() {
        // `if a > b { ... } else { ... }` → `a > b` is one condition,
        // `else` is one condition → C = 2.
        check_metrics::<GoParser>(
            "package main\nfunc f(a, b int) int { if a > b { return a } else { return b } }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_complex_function_abc() {
        // Mixed shape, verified by hand:
        // - Assignments: `_ = x` (after `var`), `n := 0`,
        //   `n = n + 1`, `n += 2`, `n++`, `_ = len(s)` → A = 6.
        //   `var x = 10` is a declaration, not counted. Every `_ = ...`
        //   IS counted as an assignment_statement.
        // - Branches: `len(s)` → B = 1.
        // - Conditions: `n < 10` → 1, `else` → 1, switch arms `case 0:`
        //   and `case 1:` (default excluded) → 2 → total C = 4.
        check_metrics::<GoParser>(
            "package main\nfunc f(s string) int {\n\
             \x20   var x = 10\n\
             \x20   _ = x\n\
             \x20   n := 0\n\
             \x20   if n < 10 { n = n + 1 } else { n += 2 }\n\
             \x20   n++\n\
             \x20   _ = len(s)\n\
             \x20   switch n {\n\
             \x20   case 0: return 0\n\
             \x20   case 1: return 1\n\
             \x20   default: return n\n\
             \x20   }\n\
             }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 6.0);
                assert_eq!(metric.abc.branches_sum(), 1.0);
                assert_eq!(metric.abc.conditions_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ----- Elixir -----

    // No top-level Calls and no operators → all three vectors are
    // zero. Uses a bare expression rather than a `defmodule` wrapper
    // (which would itself be a Call → 1 branch). Confirms the
    // ElixirCode Abc trait is wired up and the metric emits.
    #[test]
    fn elixir_empty_unit_zero() {
        check_metrics::<ElixirParser>(":ok\n", "foo.ex", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    // An empty `defmodule Foo do ... end` is itself ONE `Call` →
    // Documents that module-/function-defining macros (`defmodule`,
    // `def`, `defp`, `defmacro`, `defmacrop`) and declarative
    // directives (`alias`, `import`, `require`, `use`) are NOT
    // runtime dispatch and therefore do NOT inflate `branches`,
    // matching Cognitive's treatment.
    #[test]
    fn elixir_defmodule_is_zero_branches() {
        check_metrics::<ElixirParser>("defmodule Foo do\nend\n", "foo.ex", |metric| {
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    // Pattern-match `=` counts as an assignment. Two bindings → A = 2.
    // `defmodule` and `def` are declarative-Call wrappers and are
    // filtered out of branches; the assertion focuses on assignments
    // so we only pin that vector.
    #[test]
    fn elixir_pattern_match_is_assignment() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    x = 1\n    y = x + 1\n    y\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // `|>` pipeline operator: each `|>` token contributes one branch.
    // Two `|>` ops → +2 from the pipe operator itself. Each pipeline
    // step also dispatches a Call (`String.upcase(...)`,
    // `String.trim(...)`) — these are wrapped inside the outer
    // pipeline Call tree, contributing additional Call branches.
    // The headline assertion confirms (a) `|>` is detected and (b)
    // pipeline steps are not silently dropped.
    #[test]
    fn elixir_pipeline_each_step_is_branch() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def normalize(s) do\n    s |> String.trim() |> String.upcase()\n  end\nend\n",
            "foo.ex",
            |metric| {
                // Pipeline yields 2 `|>` branches plus Calls for
                // String.trim, String.upcase, and the outer pipeline
                // (which surfaces as a Call wrapping the binary
                // operator). `def` and `defmodule` are declarative
                // and excluded. Empirical total: B = 5.
                assert_eq!(metric.abc.branches_sum(), 5.0);
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Comparison operators all count as conditions. Six comparisons
    // (`==`, `!=`, `<`, `>`, `<=`, `>=`) → C = 6.
    #[test]
    fn elixir_comparisons_are_conditions() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(a, b) do\n    a == b or a != b or a < b or a > b or a <= b or a >= b\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 6.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Strict-equality operators `===` / `!==` count as conditions too.
    #[test]
    fn elixir_strict_equality_is_condition() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(a, b) do\n    a === b or a !== b\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Guard `when` clause counts as a condition. One `when` → +1.
    // `def f(x) when x > 0` also has `>` → +1, totalling 2.
    #[test]
    fn elixir_guard_when_is_condition() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) when x > 0 do\n    :pos\n  end\nend\n",
            "foo.ex",
            |metric| {
                // when (+1) + > (+1) = 2
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Keyword-shaped Calls (`case`, `cond`, `if`, `with`) each count
    // as one condition AND one branch. `case` here adds 1 condition
    // (the keyword Call) + 1 branch (the Call itself).
    #[test]
    fn elixir_case_is_condition_and_branch() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    case x do\n      1 -> :one\n      _ -> :other\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // conditions: case → 1
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // `cond` is structurally identical to `case` for Abc.
    #[test]
    fn elixir_cond_is_condition() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    cond do\n      x > 0 -> :pos\n      true -> :other\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // conditions: cond (+1) + > (+1) = 2
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // `for` is a comprehension/loop, NOT in the issue's condition
    // list. It is still a Call so it contributes one branch, but no
    // condition.
    #[test]
    fn elixir_for_is_branch_not_condition() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(xs) do\n    for x <- xs, do: x * 2\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Mixed shape, verified by hand: defmodule Call + def Call + if Call
    // + Call to side_effect/0 + assignment `x = 1` + comparison `x > 0`.
    // - Assignments: `x = 1` → A = 1.
    // - Branches: `defmodule` and `def` are declarative and excluded;
    //   `if` Call + `side_effect()` Call → 2 Calls, plus 0 `|>` → B = 2.
    // - Conditions: `if` keyword → 1, `x > 0` → 1 → C = 2.
    #[test]
    fn elixir_mixed_abc() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    x = 1\n    if x > 0 do\n      side_effect()\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 2.0);
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ----- C++ -----

    #[test]
    fn cpp_empty_unit_zero() {
        // No code → A=B=C=0. Wires up the trait and exercises the
        // per-language compute reachability.
        check_metrics::<CppParser>("", "empty.cpp", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn cpp_plain_and_compound_assignments_count() {
        // `int x = 0` is an `init_declarator` (declaration initialiser)
        // and NOT a Fitzpatrick assignment. `x = 5`, `x += 2`, `x = 7`
        // all parse as `assignment_expression` → A = 3.
        check_metrics::<CppParser>(
            "void f() { int x = 0; x = 5; x += 2; x = 7; }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 3.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_increment_and_decrement_count_as_assignment() {
        // `x++` / `--x` / prefix and postfix forms each parse as
        // `update_expression` and count as 1 assignment per Fitzpatrick.
        check_metrics::<CppParser>(
            "void f() { int x = 0; x++; --x; ++x; x--; }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_calls_are_branches() {
        // Free call + member-fn call (parses as `call_expression` with
        // a `field_expression` callee) + `new` allocation. All three
        // are branches → B = 3.
        check_metrics::<CppParser>(
            "struct S { void m(); }; void g(); void f() { g(); S s; s.m(); auto* p = new int(5); }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_comparisons_count_conditions() {
        // `<`, `>`, `<=`, `>=`, `==`, `!=`, and the C++20 spaceship
        // `<=>` each contribute one condition. Seven comparisons → C = 7.
        check_metrics::<CppParser>(
            "#include <compare>\n\
             bool f(int a, int b) {\n\
                 return a < b || a > b || a <= b || a >= b || a == b || a != b || (a <=> b) == 0;\n\
             }\n",
            "foo.cpp",
            |metric| {
                // 6 plain comparisons + 1 spaceship + 1 `||` adds = 7? Let's
                // pin the exact count by hand:
                // `<`, `>`, `<=`, `>=`, `==`, `!=` → 6 from the
                // chained `||` expression. `(a <=> b) == 0` adds the
                // spaceship → 7, plus its `== 0` adds one more → 8.
                // Six `||` short-circuits add → 8 + 6 = 14.
                assert_eq!(metric.abc.conditions_sum(), 14.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_short_circuit_ops_count_conditions() {
        // `&&` and `||` each count once per occurrence (Fitzpatrick
        // rule). Two short-circuits → C = 2 (plus two comparisons → 4).
        check_metrics::<CppParser>(
            "bool f(int a, int b) { return a == b && a > 0 || b < 0; }",
            "foo.cpp",
            |metric| {
                // == 1, > 1, < 1, && 1, || 1 → 5.
                assert_eq!(metric.abc.conditions_sum(), 5.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_generic_brackets_not_conditions() {
        // `<` / `>` in `std::vector<int>` are `template_argument_list`
        // delimiters, NOT comparison operators. The `binary_expression`
        // parent check must filter them out → C = 0.
        check_metrics::<CppParser>(
            "#include <vector>\nstd::vector<int> f() { return std::vector<int>{}; }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_else_and_ternary_count_conditions() {
        // `if (cond) ... else ...` + ternary `cond ? a : b`. The
        // `if`-keyword is NOT a condition (its condition is the
        // comparison inside, which counts separately). `else` adds 1,
        // `?` adds 1. Two comparisons (`a > b`, `b < 0`) → 2. Total = 4.
        check_metrics::<CppParser>(
            "int f(int a, int b) {\n\
                 if (a > b) { return a; } else { return b; }\n\
                 return (b < 0) ? -b : b;\n\
             }\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_switch_cases_count_default_excluded() {
        // `case 1`, `case 2` → 2 conditions. `default` is intentionally
        // excluded (matches the C-family precedent in Rust / Go / Python
        // and Java's omission of `Default` from this rule? — actually
        // Java DOES count `Default`. We follow Rust / Go and exclude
        // it). C = 2.
        check_metrics::<CppParser>(
            "void f(int x) {\n\
                 switch (x) {\n\
                     case 1: break;\n\
                     case 2: break;\n\
                     default: break;\n\
                 }\n\
             }\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_try_catch_count_conditions() {
        // `try` and `catch` each add one condition (Fitzpatrick's rule;
        // Java's impl above counts them too).
        check_metrics::<CppParser>(
            "void f() { try { } catch (int) { } catch (...) { } }",
            "foo.cpp",
            |metric| {
                // 1 `try` + 2 `catch` arms = 3.
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_complex_function_abc() {
        // Mixed-shape regression: assignments, calls, conditions,
        // ternary, switch, new. Verified by hand:
        // - assignments: `x = 5`, `x += 2`, `x++`, `x = (a > b) ? a : b`,
        //   `x = b` → A = 5. (`int x = 0`, `auto y = ...`, `auto* p = ...`
        //   are declaration initialisers and don't count.)
        // - branches: `f(a, b)` self-call + `new int(5)` → B = 2.
        // - conditions: `a == b`, `&&`, `a > 0` → 3 inside the if.
        //   `else` (1) + `a > b`, `?` → 2 in the ternary. `a < b`,
        //   `||` → 2 in the else-if. `case 1`, `case 2` → 2.
        //   default excluded. Total C = 10.
        check_metrics::<CppParser>(
            "int f(int a, int b) {\n\
                 int x = 0;\n\
                 x = 5;\n\
                 x += 2;\n\
                 x++;\n\
                 if (a == b && a > 0) {\n\
                     x = (a > b) ? a : b;\n\
                 } else if (a < b || !x) {\n\
                     x = b;\n\
                 }\n\
                 switch (x) {\n\
                     case 1: break;\n\
                     case 2: break;\n\
                     default: break;\n\
                 }\n\
                 auto* p = new int(5);\n\
                 return f(a, b);\n\
             }\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 5.0);
                assert_eq!(metric.abc.branches_sum(), 2.0);
                assert_eq!(metric.abc.conditions_sum(), 10.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_empty_unit_zero() {
        // No code → A=B=C=0. Wires up the trait and exercises the
        // per-language compute reachability.
        check_metrics::<JavascriptParser>("", "empty.js", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn javascript_plain_and_compound_assignments_count() {
        // `let` / `var` declarations behave like TypeScript: the `Var`
        // sentinel is pushed but only `const` suppresses the
        // initializer `=`. So `let x = 0` does count as A=+1; only
        // `const PI = 3.14` would be elided. Plain `x = 5`, `x += 2`,
        // `x = 7` all count → A = 4 total here.
        check_metrics::<JavascriptParser>(
            "function f() { let x = 0; x = 5; x += 2; x = 7; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_const_initializer_not_assignment() {
        // `const PI = 3.14` must NOT count as an assignment — the
        // `Const` sentinel suppresses the initializer `=`. `let x = 1`
        // and `var y = 2` still count (matches the TS impl: only
        // `const` suppresses).
        check_metrics::<JavascriptParser>(
            "function f() { const PI = 3.14; let x = 1; var y = 2; x = 9; }",
            "foo.js",
            |metric| {
                // `const PI` suppressed; `let x = 1`, `var y = 2`,
                // `x = 9` all count → A = 3.
                assert_eq!(metric.abc.assignments_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_increment_and_decrement_count_as_assignment() {
        // `x++` (post) and `--x` (pre) both update an lvalue and so
        // count as assignments. Combined with the `let x = 0`
        // initializer (which counts under the JS/TS sentinel rule —
        // only `const` suppresses), A = 3.
        check_metrics::<JavascriptParser>(
            "function f() { let x = 0; x++; --x; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_calls_are_branches() {
        // `g(1)` is a `call_expression` → B = 1. `new Foo(2)` is a
        // `new_expression` → B = 1. Total B = 2.
        check_metrics::<JavascriptParser>(
            "function f() { g(1); new Foo(2); }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_comparisons_count_conditions() {
        // `==`, `===`, `!=`, `!==`, `<`, `>`, `<=`, `>=` each count
        // once. The `&&` / `||` short-circuit operators are NOT
        // counted as conditions in this impl (matches the TS
        // precedent — short-circuit ops are folded into the
        // surrounding `if` / control-flow arm, not separately).
        // Total C = 8.
        check_metrics::<JavascriptParser>(
            "function f(a, b) { return a == b && a === b && a != b && a !== b && a < b && a > b && a <= b && a >= b; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 8.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_nullish_coalescing_counts_condition() {
        // `a ?? b` is one nullish-coalescing operator → C = 1.
        check_metrics::<JavascriptParser>(
            "function f(a, b) { return a ?? b; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_else_ternary_case_default_try_catch() {
        // `else`, `?` (ternary), `case`, `default`, `try`, `catch`
        // all count. With the comparisons:
        //   - `a > 0` → 1
        //   - `else` opens an else_clause → 1
        //   - `?` ternary → 1
        //   - `case 1` → 1
        //   - `default` → 1
        //   - `try` + `catch` → 2
        // Total C = 7.
        check_metrics::<JavascriptParser>(
            "function f(a) { if (a > 0) {} else {} let x = a ? 1 : 2; switch (x) { case 1: break; default: break; } try { } catch (e) { } }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 7.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_instanceof_counts_condition() {
        // `x instanceof Foo` is a binary expression whose operator is
        // the `instanceof` keyword token → C = 1.
        check_metrics::<JavascriptParser>(
            "function f(x) { return x instanceof Foo; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_complex_function_abc() {
        // Mixed-shape regression. Verified by hand:
        // - assignments: `let x = 0` (Var sentinel does not suppress)
        //   + `x = 5`, `x += 2`, `x++`, `x = (a>b)?a:b`, `x = b`,
        //   `let p = ...` (Var sentinel) → A = 7.
        // - branches: `f(a, b)` self-call + `new Bar()` → B = 2.
        // - conditions: `a == b`, `a > 0` → 2 inside the if header
        //   (`&&` is not counted). `else` (1) + `a > b`, `?` → 2 in
        //   the ternary. `a < b` → 1 in the else-if (`||` not
        //   counted). `case 1`, `default` → 2 in the switch. Total
        //   C = 8.
        check_metrics::<JavascriptParser>(
            "function f(a, b) {\n\
                 let x = 0;\n\
                 x = 5;\n\
                 x += 2;\n\
                 x++;\n\
                 if (a == b && a > 0) {\n\
                     x = (a > b) ? a : b;\n\
                 } else if (a < b || !x) {\n\
                     x = b;\n\
                 }\n\
                 switch (x) {\n\
                     case 1: break;\n\
                     default: break;\n\
                 }\n\
                 let p = new Bar();\n\
                 return f(a, b);\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 7.0);
                assert_eq!(metric.abc.branches_sum(), 2.0);
                assert_eq!(metric.abc.conditions_sum(), 8.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn mozjs_complex_function_abc() {
        // Mozjs shares JavaScript's expression / statement vocabulary;
        // the `js_abc_compute!` macro expands identical token-level
        // rules for both. This test pins parity against the JS impl.
        check_metrics::<MozjsParser>(
            "function f(a, b) {\n\
                 let x = 0;\n\
                 x = 5;\n\
                 x += 2;\n\
                 x++;\n\
                 if (a == b && a > 0) {\n\
                     x = (a > b) ? a : b;\n\
                 } else if (a < b || !x) {\n\
                     x = b;\n\
                 }\n\
                 switch (x) {\n\
                     case 1: break;\n\
                     default: break;\n\
                 }\n\
                 let p = new Bar();\n\
                 return f(a, b);\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 7.0);
                assert_eq!(metric.abc.branches_sum(), 2.0);
                assert_eq!(metric.abc.conditions_sum(), 8.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ---------- Perl ABC tests ----------

    #[test]
    fn perl_empty_unit_zero() {
        // Empty source produces zero ABC magnitude — pins the trait
        // wiring without exercising any compute branch.
        check_metrics::<PerlParser>("", "empty.pl", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn perl_plain_and_compound_assignments_count() {
        // `my $x = 0` parses as a `binary_expression` with an `=`
        // token, so the initialiser counts (Perl has no equivalent of
        // the JS `const` initialiser-suppression rule). Each
        // assignment operator token contributes one assignment:
        // `=`, `=`, `+=`, `.=`, `**=` → A = 5. Two of those `=` come
        // from the `my $x = 0` initialiser and the later `$x = 5`
        // reassignment.
        check_metrics::<PerlParser>(
            "sub f { my $x = 0; $x = 5; $x += 2; $x .= \"a\"; $x **= 3; }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 5.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_calls_are_branches() {
        // `foo()` parses as `call_expression_with_args_with_brackets`
        // wrapping an inner `call_expression_with_bareword(foo)`;
        // `bar 1, 2` wraps `bar` likewise under spaced-args; `shift`
        // appears as a standalone bareword. The bareword-inside-
        // wrapper case must NOT double-count — only the outer wrapper
        // contributes a branch. So B = 3 (foo, bar, shift), not 5.
        check_metrics::<PerlParser>(
            "sub f { foo(); bar 1, 2; my $a = shift; }",
            "foo.pl",
            |metric| {
                // shift's `my $a = shift` initialiser contributes one
                // assignment via the `=` token.
                assert_eq!(metric.abc.assignments_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 3.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_method_invocation_counts_as_branch() {
        // `$obj->method(...)` parses as `method_invocation`. Any
        // arrow-dispatch counts as one branch regardless of how the
        // arguments are passed.
        check_metrics::<PerlParser>(
            "sub f { my $obj = shift; $obj->run($x); $obj->ping; }",
            "foo.pl",
            |metric| {
                // `my $obj = shift` → A=1, B=1 (shift bareword).
                // `$obj->run($x)` and `$obj->ping` → 2 more branches.
                assert_eq!(metric.abc.assignments_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 3.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_numeric_and_string_comparisons_count_conditions() {
        // Numeric ops `==`, `!=`, `<`, `>`, `<=`, `>=`, `<=>` and
        // string ops `eq`, `ne`, `lt`, `gt`, `le`, `ge`, `cmp` each
        // fire once per token. The sample below uses one of each →
        // C = 14. No assignments, no branches.
        check_metrics::<PerlParser>(
            "sub f {\n\
                 my $r;\n\
                 $r = $a == $b;\n\
                 $r = $a != $b;\n\
                 $r = $a <  $b;\n\
                 $r = $a >  $b;\n\
                 $r = $a <= $b;\n\
                 $r = $a >= $b;\n\
                 $r = $a <=> $b;\n\
                 $r = $a eq $b;\n\
                 $r = $a ne $b;\n\
                 $r = $a lt $b;\n\
                 $r = $a gt $b;\n\
                 $r = $a le $b;\n\
                 $r = $a ge $b;\n\
                 $r = $a cmp $b;\n\
             }",
            "foo.pl",
            |metric| {
                // 15 `=` tokens: one declaration `my $r` (no `=`),
                // then 14 `$r = …` plus there's no `=` in `my $r;`.
                // Actually: `my $r;` has no `=`; the 14 `$r = …` are
                // 14 `=` tokens. So A=14, C=14.
                assert_eq!(metric.abc.assignments_sum(), 14.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 14.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_short_circuit_and_ternary_count_conditions() {
        // `&&`, `||`, `//`, low-precedence `and`, `or`, `xor`, plus
        // ternary `? :` each contribute one condition.
        check_metrics::<PerlParser>(
            "sub f {\n\
                 my $r;\n\
                 $r = $a && $b;\n\
                 $r = $a || $b;\n\
                 $r = $a // $b;\n\
                 $r = $a and $b;\n\
                 $r = $a or  $b;\n\
                 $r = $a xor $b;\n\
                 $r = $a ? 1 : 2;\n\
             }",
            "foo.pl",
            |metric| {
                // 7 `=` tokens (one per reassignment line).
                assert_eq!(metric.abc.assignments_sum(), 7.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 7.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_elsif_and_else_count_conditions() {
        // `if (… == …) { … } elsif (… < …) { … } else { … }` →
        // 2 comparison tokens (`==`, `<`), plus `elsif_clause` and
        // `else_clause` each + 1 → C = 4. Branches: 0 (only
        // assignments). Assignments: just the `=` initialisers /
        // reassignments — there are 4 here (`$x` init plus three
        // `$x = …` reassigns).
        check_metrics::<PerlParser>(
            "sub f {\n\
                 my $x = 0;\n\
                 if ($a == $b) {\n\
                     $x = 1;\n\
                 } elsif ($a < $b) {\n\
                     $x = 2;\n\
                 } else {\n\
                     $x = 3;\n\
                 }\n\
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_regex_match_operators_count_conditions() {
        // `=~` and `!~` are pattern-match operators; we count both
        // as conditions because they evaluate the regex match in a
        // boolean context.
        check_metrics::<PerlParser>(
            "sub f { my $s = shift; my $m = $s =~ /foo/; my $n = $s !~ /bar/; }",
            "foo.pl",
            |metric| {
                // 3 `=` tokens, 0 branches except `shift` bareword.
                assert_eq!(metric.abc.assignments_sum(), 3.0);
                assert_eq!(metric.abc.branches_sum(), 1.0);
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_complex_function_abc() {
        // Mixed program exercising every category. Computed
        // expected:
        //   Assignments: `my $i = 0` (1), `$i++` is a unary
        //     increment — Perl's grammar emits `PLUSPLUS` not an `=`
        //     operator, so it does NOT count under the operator-
        //     token rule. The for-loop's `$i++` is similarly
        //     uncounted.
        //     Total A: 1 from `my $i = 0`, 1 from `$total += $i`
        //     (the `+=` token) → A = 2.
        //   Branches: `do_work($i)` → 1; `print "done\n"` is a
        //     call_expression_with_spaced_args → 1; `return $total`
        //     uses the `return` keyword not a call → 0. B = 2.
        //   Conditions: `$i < 10` (`<`) → 1; `$i % 2 == 0` (`==`) →
        //     1; `else_clause` → 1. C = 3.
        check_metrics::<PerlParser>(
            "sub run {\n\
                 my $total = 0;\n\
                 for (my $i = 0; $i < 10; $i++) {\n\
                     if ($i % 2 == 0) {\n\
                         do_work($i);\n\
                     } else {\n\
                         $total += $i;\n\
                     }\n\
                 }\n\
                 print \"done\\n\";\n\
                 return $total;\n\
             }",
            "foo.pl",
            |metric| {
                // `my $total = 0` is one `=`; `my $i = 0` is another
                // `=`; `$total += $i` is one `+=`. Total = 3.
                assert_eq!(metric.abc.assignments_sum(), 3.0);
                assert_eq!(metric.abc.branches_sum(), 2.0);
                assert_eq!(metric.abc.conditions_sum(), 3.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ---------- Lua ABC tests ----------

    #[test]
    fn lua_empty_unit_zero() {
        check_metrics::<LuaParser>("", "empty.lua", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn lua_assignments_count_locals_and_plain() {
        // `local x = 0` wraps an `assignment_statement` under a
        // `variable_declaration`; the inner wrapper still counts.
        // Multi-target assignment `a, b = 1, 2` is a single
        // `assignment_statement` and contributes 1, NOT 2 — the
        // wrapper is the unit of counting (matches the Python rule:
        // one `Assignment` node, one assignment).
        check_metrics::<LuaParser>(
            "function f()\n\
                 local x = 0\n\
                 x = 1\n\
                 local a, b = 1, 2\n\
                 a, b = b, a\n\
             end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_calls_are_branches() {
        // `print(x)`, `obj.m(x)`, `obj:m(x)`, `f(g(1))` — every
        // call form is a `function_call` node. The nested
        // `f(g(1))` counts as 2 branches (one per dispatch).
        check_metrics::<LuaParser>(
            "function r(x)\n\
                 print(x)\n\
                 obj.m(x)\n\
                 obj:m(x)\n\
                 return f(g(1))\n\
             end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 5.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_comparisons_and_boolean_ops_count_conditions() {
        // Each comparison / logical operator token contributes one
        // condition.
        check_metrics::<LuaParser>(
            "function f(a, b)\n\
                 local r\n\
                 r = a == b\n\
                 r = a ~= b\n\
                 r = a <  b\n\
                 r = a >  b\n\
                 r = a <= b\n\
                 r = a >= b\n\
                 r = a and b\n\
                 r = a or  b\n\
             end",
            "foo.lua",
            |metric| {
                // 8 `r = …` reassignments, plus `local r` (no `=`).
                assert_eq!(metric.abc.assignments_sum(), 8.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 8.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_elseif_and_else_count_conditions() {
        // Each elseif / else arm of the if contributes one
        // condition, mirroring the Python rule.
        check_metrics::<LuaParser>(
            "function f(x)\n\
                 if x > 0 then\n\
                     return 1\n\
                 elseif x < 0 then\n\
                     return -1\n\
                 else\n\
                     return 0\n\
                 end\n\
             end",
            "foo.lua",
            |metric| {
                // Comparisons: `>`, `<` → 2; elseif_statement → 1;
                // else_statement → 1. C = 4. No branches (no calls).
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_complex_function_abc() {
        // Combines every category to pin the metric.
        check_metrics::<LuaParser>(
            "function run(n)\n\
                 local total = 0\n\
                 for i = 1, n do\n\
                     if i % 2 == 0 then\n\
                         do_work(i)\n\
                     else\n\
                         total = total + i\n\
                     end\n\
                 end\n\
                 print(\"done\")\n\
                 return total\n\
             end",
            "foo.lua",
            |metric| {
                // Assignments: `local total = 0` (1), `total = total + i` (1) → 2.
                // Branches: `do_work(i)` (1), `print(\"done\")` (1) → 2.
                // Conditions: `==` (1), `else_statement` (1) → 2.
                assert_eq!(metric.abc.assignments_sum(), 2.0);
                assert_eq!(metric.abc.branches_sum(), 2.0);
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ---------- Tcl ABC tests ----------

    #[test]
    fn tcl_empty_unit_zero() {
        check_metrics::<TclParser>("", "empty.tcl", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0.0);
            assert_eq!(metric.abc.branches_sum(), 0.0);
            assert_eq!(metric.abc.conditions_sum(), 0.0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn tcl_set_command_counts_assignment() {
        // `set` has its own grammar production; each invocation is
        // one assignment.
        check_metrics::<TclParser>(
            "proc f {} {\n\
                 set x 1\n\
                 set y 2\n\
                 set x [expr {$x + $y}]\n\
             }",
            "foo.tcl",
            |metric| {
                // 3 `set` invocations → A=3. The inner `expr` is a
                // sub-command (`command_substitution` + `expr_cmd`),
                // not a `command` node, so it doesn't add a branch.
                assert_eq!(metric.abc.assignments_sum(), 3.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_incr_append_lappend_count_assignment() {
        // Variable-mutation commands (`incr`, `append`, `lappend`)
        // are recognised by name and count as assignments, not
        // branches.
        check_metrics::<TclParser>(
            "proc f {} {\n\
                 set x 0\n\
                 incr x\n\
                 append s \"hi\"\n\
                 lappend lst 1\n\
             }",
            "foo.tcl",
            |metric| {
                // `set` (1) + `incr` (1) + `append` (1) + `lappend`
                // (1) → A=4. No branches, no conditions.
                assert_eq!(metric.abc.assignments_sum(), 4.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_generic_commands_are_branches() {
        // Anything that isn't `set` or a known mutator command
        // counts as a branch — including builtins like `puts` and
        // `return`.
        check_metrics::<TclParser>(
            "proc f {} {\n\
                 puts \"hello\"\n\
                 do_work 1 2\n\
                 return 0\n\
             }",
            "foo.tcl",
            |metric| {
                // 3 commands, all branches.
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 3.0);
                assert_eq!(metric.abc.conditions_sum(), 0.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_comparisons_and_boolean_ops_count_conditions() {
        // `expr` predicates expose comparison / logical tokens at
        // the leaf level; each token contributes one condition.
        check_metrics::<TclParser>(
            "proc f {a b} {\n\
                 set r [expr {$a == $b}]\n\
                 set r [expr {$a != $b}]\n\
                 set r [expr {$a <  $b}]\n\
                 set r [expr {$a >  $b}]\n\
                 set r [expr {$a <= $b}]\n\
                 set r [expr {$a >= $b}]\n\
                 set r [expr {$a eq $b}]\n\
                 set r [expr {$a ne $b}]\n\
                 set r [expr {$a && $b}]\n\
                 set r [expr {$a || $b}]\n\
             }",
            "foo.tcl",
            |metric| {
                // 10 `set` assignments. Each `expr` predicate
                // produces exactly one comparison/logical token.
                assert_eq!(metric.abc.assignments_sum(), 10.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 10.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_ternary_counts_condition() {
        // `$a ? $b : $c` inside an `expr` is one `ternary_expr`
        // node → 1 condition.
        check_metrics::<TclParser>(
            "proc f {a b c} {\n\
                 set r [expr {$a ? $b : $c}]\n\
             }",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1.0);
                assert_eq!(metric.abc.branches_sum(), 0.0);
                assert_eq!(metric.abc.conditions_sum(), 1.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_elseif_and_else_count_conditions() {
        // `if` / `elseif` / `else` clause productions each
        // contribute one condition. The leaf comparison inside the
        // predicate is counted independently.
        check_metrics::<TclParser>(
            "proc f {x} {\n\
                 if {$x > 0} {\n\
                     return 1\n\
                 } elseif {$x < 0} {\n\
                     return -1\n\
                 } else {\n\
                     return 0\n\
                 }\n\
             }",
            "foo.tcl",
            |metric| {
                // Branches: three `return` commands → 3.
                // Conditions: `>` (1), `<` (1), `elseif` (1), `else`
                // (1) → 4.
                assert_eq!(metric.abc.assignments_sum(), 0.0);
                assert_eq!(metric.abc.branches_sum(), 3.0);
                assert_eq!(metric.abc.conditions_sum(), 4.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_complex_function_abc() {
        // Mixed program covering every category. Tcl's grammar
        // re-parses braced content that looks command-shaped as a
        // nested `command` node, which inflates the branch count
        // relative to a naive read of the source — see breakdown.
        check_metrics::<TclParser>(
            "proc run {n} {\n\
                 set total 0\n\
                 for {set i 0} {$i < $n} {incr i} {\n\
                     if {$i % 2 == 0} {\n\
                         do_work $i\n\
                     } else {\n\
                         incr total $i\n\
                     }\n\
                 }\n\
                 puts \"done\"\n\
                 return $total\n\
             }",
            "foo.tcl",
            |metric| {
                // Assignments: `set total 0` (1), `set i 0` (1),
                // `incr i` (1), `incr total $i` (1) → A = 4.
                // Branches: the outer `for …` is one `command` node;
                // the `{$i < $n}` predicate ALSO re-parses as a
                // `command` node (tree-sitter-tcl treats braced
                // predicates as nested commands at the pinned
                // grammar version); plus `do_work $i`, `puts
                // "done"`, and `return $total`. The for-loop body's
                // `incr` and `incr total $i` are assignment commands
                // and don't add branches. Total B = 5.
                // Conditions: `==` (1) and `else` (1) → C = 2. The
                // `<` inside `{$i < $n}` is NOT `Tcl::LT`: because
                // that predicate re-parses as a `command`, the `<`
                // is emitted as `simple_word`. Only `<` inside a
                // real `expr` production becomes `Tcl::LT`.
                assert_eq!(metric.abc.assignments_sum(), 4.0);
                assert_eq!(metric.abc.branches_sum(), 5.0);
                assert_eq!(metric.abc.conditions_sum(), 2.0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }
}
