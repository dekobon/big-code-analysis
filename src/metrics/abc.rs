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

use std::fmt;

use crate::checker::Checker;

use crate::macros::implement_metric_trait;

use crate::*;

mod bash;
mod c;
mod cpp;
mod csharp;
mod elixir;
mod go;
mod groovy;
mod irules;
mod java;
mod js_family;
mod kotlin;
mod lua;
mod mozcpp;
mod objc;
mod perl;
mod php;
mod python;
mod ruby;
mod rust;
mod tcl;

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
///
/// # Cross-language `&&` / `||` policy
///
/// Per Fitzpatrick's conditional-operator rule (Rule 5 in Figure 2
/// for C and Figure 4 for Java; Rule 7 in Figure 3 for C++), only
/// comparison operators (`==`, `!=`, `<=`, `>=`, `<`, `>`) and a
/// paper-defined keyword set (`else`, `case`, `default`, `?`, plus
/// `try` / `catch` for C++ and Java) contribute to the condition
/// count. Per-language `impl Abc` blocks narrow this set where
/// appropriate — e.g., C++/Rust/Go/Python exclude `default` since
/// it falls through unconditionally (matching the Rust `_ =>` and
/// Java `default:` precedent). The short-
/// circuit logical operators `&&` and `||` (and per-language
/// equivalents — Python's `and` / `or`, Lua's `and` / `or`, Tcl's
/// `&&` / `||`, Perl's `&&` / `||` / `//` / `and` / `or` / `xor`)
/// are deliberately **not** counted on their own. The paper's
/// worked Listing 2 annotates `(am >= 0 && am <= 0xF) ? '/' : 'C'`
/// as `accc` — three conditions for `>=`, `<=`, `?`, zero for
/// `&&`.
///
/// Fitzpatrick's Rule 7 (Figure 3, C++) / Rule 9 (Figure 4, Java) —
/// "Add one to the condition count for each unary conditional
/// expression" — instead counts each non-comparison operand of a
/// `&&` / `||` chain once. The paper's worked example for this
/// rule is `if (x || y) printf("test failure\n");`, annotated:
/// "there are two unary conditions since both `x` and `y` are
/// tested as conditional expressions" (so `||` contributes zero,
/// `x` contributes one, `y` contributes one, and `printf(...)`
/// contributes one branch). The walker machinery for this —
/// modelled on `java_count_unary_conditions` /
/// `java_inspect_container` — is present today for Java, Groovy,
/// C#, Rust, Go, JavaScript, TypeScript, TSX, Mozjs, PHP, C++,
/// Python, Perl, Lua, Tcl, iRules, Kotlin, Ruby, and Elixir. So
/// `if (a && b)` reports 2 conditions across this set, matching
/// the paper. Bash is the lone exception: its `&&` / `||` are
/// command-list separators rather than boolean-expression operands
/// with named leaf operands, so Fitzpatrick's Rule 9 does not map
/// onto its grammar and the walker is deliberately not wired.
///
/// This policy is paper-faithful and deviates from RuboCop's
/// `Metrics/AbcSize` (which counts `and` / `or` as conditions
/// directly) while matching `StepicOrg/abcmeter` and
/// `eoinnoble/python-abc`. The book's *ABC counting rules*
/// section reproduces the rule tables, a per-language deviation
/// table, and worked examples — see the chapter at
/// <https://dekobon.github.io/big-code-analysis/metrics.html#abc>.
///
/// # Cross-language empty-`for`-condition policy
///
/// `for (;;)` — and every other spelling that omits the test slot
/// (`for (init; ; update)`, Go's bare `for {}`) — counts **zero**
/// conditions. Nothing in Fitzpatrick's condition rules attributes a
/// count to a `for` keyword: they count conditional operators and
/// unary conditions that are *present*, and an omitted test is not a
/// decision. Most languages get this for free: `*_walk_for_statement`
/// asks `for_statement` for its `condition` field and an empty header
/// has none. Two do not, and neither needs a special case either —
/// the JS family fills the slot with an `empty_statement`, which is
/// not a boolean terminal and not a paren / `!` wrapper, so it falls
/// through; and Go, whose `for_statement` exposes no `condition` field
/// at all, locates the header slot structurally and finds only the
/// body. Before #1276 Java and Groovy alone disagreed, counting the
/// `;` or `)` that landed in a positional child slot as a
/// vacuously-true condition.
///
/// See issue #395 for the Phase-1 cross-language policy
/// alignment, #403 for the Phase-2 unary-conditional walker
/// fan-out, #404 for the Phase-3 book documentation, #557
/// for the Kotlin / Ruby / Elixir walker wiring, and #1276 for the
/// `for`-header condition slot.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Stats {
    pub(super) assignments: f64,
    assignments_sum: f64,
    assignments_min: f64,
    assignments_max: f64,
    pub(super) branches: f64,
    branches_sum: f64,
    branches_min: f64,
    branches_max: f64,
    pub(super) conditions: f64,
    conditions_sum: f64,
    conditions_min: f64,
    conditions_max: f64,
    space_count: usize,
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
        }
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
    pub fn assignments(&self) -> u64 {
        self.assignments as u64
    }

    /// Returns the `Abc` assignments sum metric value.
    #[must_use]
    pub fn assignments_sum(&self) -> u64 {
        self.assignments_sum as u64
    }

    /// Returns the `Abc` assignments average value.
    ///
    /// This value is computed dividing the `Abc`
    /// assignments value for the number of spaces.
    #[must_use]
    pub fn assignments_average(&self) -> f64 {
        crate::metrics::average(self.assignments_sum() as f64, self.space_count)
    }

    /// Returns the `Abc` assignments minimum value.
    ///
    /// Collapses the `f64::MAX` sentinel that `Stats::default()` plants
    /// into `assignments_min` to `0`, so a never-observed space
    /// serializes to a meaningful number rather than `1.7976931e308`.
    #[allow(clippy::float_cmp)]
    #[must_use]
    pub fn assignments_min(&self) -> u64 {
        if self.assignments_min == f64::MAX {
            0
        } else {
            self.assignments_min as u64
        }
    }

    /// Returns the `Abc` assignments maximum value.
    #[must_use]
    pub fn assignments_max(&self) -> u64 {
        self.assignments_max as u64
    }

    /// Returns the `Abc` branches metric value.
    #[must_use]
    pub fn branches(&self) -> u64 {
        self.branches as u64
    }

    /// Returns the `Abc` branches sum metric value.
    #[must_use]
    pub fn branches_sum(&self) -> u64 {
        self.branches_sum as u64
    }

    /// Returns the `Abc` branches average value.
    ///
    /// This value is computed dividing the `Abc`
    /// branches value for the number of spaces.
    #[must_use]
    pub fn branches_average(&self) -> f64 {
        crate::metrics::average(self.branches_sum() as f64, self.space_count)
    }

    /// Returns the `Abc` branches minimum value.
    ///
    /// Same `f64::MAX` sentinel collapse as `assignments_min`.
    #[allow(clippy::float_cmp)]
    #[must_use]
    pub fn branches_min(&self) -> u64 {
        if self.branches_min == f64::MAX {
            0
        } else {
            self.branches_min as u64
        }
    }

    /// Returns the `Abc` branches maximum value.
    #[must_use]
    pub fn branches_max(&self) -> u64 {
        self.branches_max as u64
    }

    /// Returns the `Abc` conditions metric value.
    #[must_use]
    pub fn conditions(&self) -> u64 {
        self.conditions as u64
    }

    /// Returns the `Abc` conditions sum metric value.
    #[must_use]
    pub fn conditions_sum(&self) -> u64 {
        self.conditions_sum as u64
    }

    /// Returns the `Abc` conditions average value.
    ///
    /// This value is computed dividing the `Abc`
    /// conditions value for the number of spaces.
    #[must_use]
    pub fn conditions_average(&self) -> f64 {
        crate::metrics::average(self.conditions_sum() as f64, self.space_count)
    }

    /// Returns the `Abc` conditions minimum value.
    ///
    /// Same `f64::MAX` sentinel collapse as `assignments_min`.
    #[allow(clippy::float_cmp)]
    #[must_use]
    pub fn conditions_min(&self) -> u64 {
        if self.conditions_min == f64::MAX {
            0
        } else {
            self.conditions_min as u64
        }
    }

    /// Returns the `Abc` conditions maximum value.
    #[must_use]
    pub fn conditions_max(&self) -> u64 {
        self.conditions_max as u64
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
pub(crate) trait Abc
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
    ///
    /// `ancestors` is the chain the walker descended through. Nearly
    /// every language classifies some token by what encloses it: `<`
    /// and `>` are comparisons only under a binary expression (a type
    /// -argument list otherwise), and `&&` / `||` reach their operands
    /// through the enclosing chain node. Reaching that parent with
    /// [`Node::parent`] costs `O(depth)` per node (#1096). The
    /// condition-slot walkers take *their* parent as an argument
    /// instead, because the caller descended from it.
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        stats: &mut Stats,
    );
}

// Shared Phase-2B helper (issue #403): walk every named child of an
// expression-list-style wrapper (Go's `expression_list`, Lua's
// `expression_list`) and route each through a language-specific
// classifier. Used for `return value1, value2, ...` arms where the
// values live one level below the return statement under a list
// wrapper. The classifier receives only named children so that
// `,` / `;` / `(` / `)` tokens never reach it, plus `list` itself as
// the child's parent — the container classifiers seed their
// boolean-context flag from the parent kind, and reaching it with
// `Node::parent` would cost `O(depth)` per node (#1096).
pub(super) fn for_each_named_child(
    list: &Node,
    conditions: &mut f64,
    f: fn(&Node, &Node, &mut f64),
) {
    let mut cursor = list.cursor();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.is_named() {
                f(&child, list, conditions);
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
    use crate::test_support::{
        ast_has_kind_id, check_func_space_only_shim, check_metrics_only_shim, metrics_verbatim,
    };
    use crate::traits::ParserTrait;

    use super::*;

    check_metrics_only_shim!(check_metrics, Abc);
    // Every `check_func_space` caller in this module is an ABC-versus-
    // cyclomatic parity test (`abc.conditions() == cyclomatic() - 1` on
    // the same space), so the func-space shim carries Cyclomatic too.
    check_func_space_only_shim!(check_func_space, Abc, Cyclomatic);

    /// `abc.conditions_sum()` of `src` under a walk restricted to ABC
    /// and the metrics it resolves, for the cross-language parity tests
    /// that compare one construct across sibling grammars.
    /// `check_metrics` expands to a plain `fn` and so cannot close over
    /// a reference value; this can. Restricted rather than
    /// `MetricsOptions::default()` per `metrics_verbatim`'s own doc
    /// (#1127).
    fn abc_conditions(lang: LANG, src: &str) -> u64 {
        metrics_verbatim(
            lang,
            src.as_bytes(),
            MetricsOptions::default().with_only(&[crate::Metric::Abc]),
        )
        .abc
        .conditions_sum()
    }

    // Recurse to the innermost function space and assert the invariant
    // named above there. `conditions()` is that one space's own count,
    // so it pairs with `cyclomatic()` and never `cyclomatic_sum()`,
    // which folds in a base of 1 per nested space.
    fn assert_deepest_conditions_match_cyclomatic(space: &crate::FuncSpace, expected: u64) {
        let mut deepest = space;
        while let Some(child) = deepest.spaces.last() {
            deepest = child;
        }
        let decisions = deepest.metrics.cyclomatic.cyclomatic() - 1;
        assert_eq!(decisions, expected);
        assert_eq!(deepest.metrics.abc.conditions(), decisions);
    }

    /// Regression for #227: a `Stats::default()` that never sees an
    /// observation must not leak the `f64::MAX` sentinel for
    /// `assignments_min`, `branches_min`, or `conditions_min`. All
    /// three getters collapse the sentinel to `0.0` so JSON never
    /// emits `1.7976931e308`.
    #[test]
    fn abc_empty_file_min_is_zero() {
        let stats = Stats::default();
        assert_eq!(stats.assignments_min(), 0);
        assert_eq!(stats.branches_min(), 0);
        assert_eq!(stats.conditions_min(), 0);
    }

    // The `EQ` arm of `java_count_token_assignment`: a plain `=` counts
    // unless `java_eq_initializes_final_binding` finds it initialising a
    // `final` binding, whether the declaration is a local or a field.
    #[test]
    fn java_eq_arm_counts_outside_final_declarations() {
        check_metrics::<JavaParser>(
            "class A { void m() { int x = 0; x = 1; x = 2; x = 3; } }",
            "foo.java",
            |metric| {
                // `int x = 0;` is not `final`, so it counts like each
                // `x = N;` that follows.
                assert_eq!(metric.abc.assignments_sum(), 4);
            },
        );
    }

    #[test]
    fn java_eq_arm_skips_final_initializers() {
        check_metrics::<JavaParser>(
            "class A {
                final int X = 1;
                @Deprecated private final int Y = 2;
                void m() { final int Z = 3; }
            }",
            "foo.java",
            |metric| {
                // All three `=` tokens are `final` initializers — the
                // second behind an annotation and an access modifier in
                // the same `modifiers` node — so assignments are 0
                // across all spaces.
                assert_eq!(metric.abc.assignments_sum(), 0);
            },
        );
    }

    #[test]
    fn java_final_initializer_does_not_suppress_the_assignments_inside_it() {
        // The sentinel stack the structural predicate replaced stayed
        // live from `final` to the next `;`, so every `=` nested in the
        // initializer — a lambda body's, an array initializer's — was
        // suppressed with the declarator's own. Java lambdas open no
        // space, so nothing else separated them. Each row names the
        // pre-fix value.
        let cases = [
            ("void m() { final Runnable r = () -> { x = 1; }; }", 1, 0),
            (
                "void m() { final Runnable r = () -> { x = 1; y = 2; }; }",
                2,
                1,
            ),
            ("void m() { final Runnable r = () -> x = 1; }", 1, 0),
            ("void m() { final int[] a = { x = 1 }; }", 1, 0),
            ("private final Runnable f = () -> { x = 1; };", 1, 0),
            // The `;` that closed the leak is the reference: the same
            // body after a `final` local counts as it always did.
            ("void m() { final int q = 1; x = 1; }", 1, 1),
        ];
        let mut ran = 0;
        for (body, expected, before) in cases {
            let src = format!("class K {{ int x, y; {body} }}\n");
            let assignments = metrics_verbatim(
                LANG::Java,
                src.as_bytes(),
                MetricsOptions::default().with_only(&[crate::Metric::Abc]),
            )
            .abc
            .assignments_sum();
            assert_eq!(assignments, expected, "`{body}` (pre-fix {before})");
            ran += 1;
        }
        assert_eq!(ran, cases.len());
        assert!(cases.iter().any(|&(_, now, before)| now != before));
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
                    @r#"
                {
                  "assignments": 8,
                  "branches": 0,
                  "conditions": 0,
                  "magnitude": 8.0,
                  "value": 0.0,
                  "assignments_average": 2.6666666666666665,
                  "branches_average": 0.0,
                  "conditions_average": 0.0,
                  "assignments_min": 0,
                  "assignments_max": 4,
                  "branches_min": 0,
                  "branches_max": 0,
                  "conditions_min": 0,
                  "conditions_max": 0
                }
                "#
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
                    @r#"
                {
                  "assignments": 14,
                  "branches": 3,
                  "conditions": 12,
                  "magnitude": 18.681541692269406,
                  "value": 18.681541692269406,
                  "assignments_average": 14.0,
                  "branches_average": 3.0,
                  "conditions_average": 12.0,
                  "assignments_min": 14,
                  "assignments_max": 14,
                  "branches_min": 3,
                  "branches_max": 3,
                  "conditions_min": 12,
                  "conditions_max": 12
                }
                "#
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
                    @r#"
                {
                  "assignments": 14,
                  "branches": 6,
                  "conditions": 16,
                  "magnitude": 22.090722034374522,
                  "value": 22.090722034374522,
                  "assignments_average": 14.0,
                  "branches_average": 6.0,
                  "conditions_average": 16.0,
                  "assignments_min": 14,
                  "assignments_max": 14,
                  "branches_min": 6,
                  "branches_max": 6,
                  "conditions_min": 16,
                  "conditions_max": 16
                }
                "#
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
                    @r#"
                {
                  "assignments": 0,
                  "branches": 14,
                  "conditions": 10,
                  "magnitude": 17.204650534085253,
                  "value": 17.204650534085253,
                  "assignments_average": 0.0,
                  "branches_average": 14.0,
                  "conditions_average": 10.0,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 14,
                  "branches_max": 14,
                  "conditions_min": 10,
                  "conditions_max": 10
                }
                "#
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
                    @r#"
                {
                  "assignments": 0,
                  "branches": 8,
                  "conditions": 22,
                  "magnitude": 23.40939982143925,
                  "value": 23.40939982143925,
                  "assignments_average": 0.0,
                  "branches_average": 8.0,
                  "conditions_average": 22.0,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 8,
                  "branches_max": 8,
                  "conditions_min": 22,
                  "conditions_max": 22
                }
                "#
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
                    @r#"
                {
                  "assignments": 0,
                  "branches": 5,
                  "conditions": 30,
                  "magnitude": 30.4138126514911,
                  "value": 30.4138126514911,
                  "assignments_average": 0.0,
                  "branches_average": 5.0,
                  "conditions_average": 30.0,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 5,
                  "branches_max": 5,
                  "conditions_min": 30,
                  "conditions_max": 30
                }
                "#
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
                    @r#"
                {
                  "assignments": 0,
                  "branches": 2,
                  "conditions": 12,
                  "magnitude": 12.165525060596439,
                  "value": 12.165525060596439,
                  "assignments_average": 0.0,
                  "branches_average": 2.0,
                  "conditions_average": 12.0,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 2,
                  "branches_max": 2,
                  "conditions_min": 12,
                  "conditions_max": 12
                }
                "#
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
                    @r#"
                {
                  "assignments": 0,
                  "branches": 0,
                  "conditions": 9,
                  "magnitude": 9.0,
                  "value": 0.0,
                  "assignments_average": 0.0,
                  "branches_average": 0.0,
                  "conditions_average": 1.2857142857142858,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 0,
                  "branches_max": 0,
                  "conditions_min": 0,
                  "conditions_max": 3
                }
                "#
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
                    @r#"
                {
                  "assignments": 0,
                  "branches": 1,
                  "conditions": 0,
                  "magnitude": 1.0,
                  "value": 0.0,
                  "assignments_average": 0.0,
                  "branches_average": 0.14285714285714285,
                  "conditions_average": 0.0,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 0,
                  "branches_max": 1,
                  "conditions_min": 0,
                  "conditions_max": 0
                }
                "#
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
                    @r#"
                {
                  "assignments": 9,
                  "branches": 1,
                  "conditions": 9,
                  "magnitude": 12.767145334803704,
                  "value": 12.767145334803704,
                  "assignments_average": 9.0,
                  "branches_average": 1.0,
                  "conditions_average": 9.0,
                  "assignments_min": 9,
                  "assignments_max": 9,
                  "branches_min": 1,
                  "branches_max": 1,
                  "conditions_min": 9,
                  "conditions_max": 9
                }
                "#
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
                    @r#"
                {
                  "assignments": 10,
                  "branches": 2,
                  "conditions": 8,
                  "magnitude": 12.96148139681572,
                  "value": 12.96148139681572,
                  "assignments_average": 10.0,
                  "branches_average": 2.0,
                  "conditions_average": 8.0,
                  "assignments_min": 10,
                  "assignments_max": 10,
                  "branches_min": 2,
                  "branches_max": 2,
                  "conditions_min": 8,
                  "conditions_max": 8
                }
                "#
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
                    for ( ; ; ) {}      // +0c — no condition to count (#1276)
                }
            }",
            "foo.java",
            |metric| {
                // magnitude: sqrt(64 + 0 + 25) = sqrt(89)
                // space count: 5 (1 unit, 1 class and 3 methods)
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r#"
                {
                  "assignments": 8,
                  "branches": 0,
                  "conditions": 5,
                  "magnitude": 9.433981132056603,
                  "value": 0.0,
                  "assignments_average": 1.6,
                  "branches_average": 0.0,
                  "conditions_average": 1.0,
                  "assignments_min": 0,
                  "assignments_max": 8,
                  "branches_min": 0,
                  "branches_max": 0,
                  "conditions_min": 0,
                  "conditions_max": 4
                }
                "#
                );
            },
        );
    }

    // Issue #1276 changed Java's answer here. `java_walk_for_statement`
    // used to read child(3), fall through to child(4) when that was the
    // `;` an expression initializer leaves behind, and count a `;` or
    // `)` landing there as a vacuously-true condition — so `for (;;)`
    // scored one. Nothing in Fitzpatrick's C dimension attributes a
    // condition to a `for` keyword; it counts conditional operators and
    // unary conditions that are present. Java and Groovy were the only
    // two impls disagreeing, and the field-addressed walker now reports
    // zero the way the C family, the JS family, PHP, C# and Go all do.
    #[test]
    fn java_empty_for_condition_counts_nothing() {
        check_metrics::<JavaParser>(
            "class A { void m() { for (;;) { break; } } }",
            "foo.java",
            |metric| assert_eq!(metric.abc.conditions_sum(), 0),
        );
        // The other empty spelling, and the one the old cascade
        // actually mis-scored: with an **expression** initializer the
        // children are `for ( init ; ; update )`, so child(3) was the
        // separating `;` and child(4) the empty condition's `;`, which
        // the vacuous-true arm counted. The `for (int i = 0; ; i++)`
        // spelling scored 0 both before and after — Java's
        // `local_variable_declaration` swallows its own `;`, putting the
        // update expression at child(4), where it matched no arm — so it
        // would be a vacuous regression test and is deliberately not
        // used here.
        check_metrics::<JavaParser>(
            "class A { void m() { int i; for (i = 0; ; i++) { break; } } }",
            "foo.java",
            |metric| assert_eq!(metric.abc.conditions_sum(), 0),
        );
    }

    // The second defect the positional cascade carried: tree-sitter
    // counts comments among a node's children, so a comment anywhere in
    // the header shifted every index and the condition went unread —
    // the same failure #1181 removed from `java_walk_ternary`. Reading
    // the `condition` field cannot shift.
    #[test]
    fn java_for_condition_survives_a_header_comment() {
        check_metrics::<JavaParser>(
            "class A { void m(boolean a) { for (; /* n */ a; ) { break; } } }",
            "foo.java",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
        );
        // Unchanged control: the same loop without the comment. Both
        // spellings must agree, which is the property the positional
        // form broke.
        check_metrics::<JavaParser>(
            "class A { void m(boolean a) { for (; a; ) { break; } } }",
            "foo.java",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
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
                    @r#"
                {
                  "assignments": 5,
                  "branches": 4,
                  "conditions": 24,
                  "magnitude": 24.839484696748443,
                  "value": 24.839484696748443,
                  "assignments_average": 5.0,
                  "branches_average": 4.0,
                  "conditions_average": 24.0,
                  "assignments_min": 5,
                  "assignments_max": 5,
                  "branches_min": 4,
                  "branches_max": 4,
                  "conditions_min": 24,
                  "conditions_max": 24
                }
                "#
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
                    @r#"
                {
                  "assignments": 3,
                  "branches": 0,
                  "conditions": 0,
                  "magnitude": 3.0,
                  "value": 0.0,
                  "assignments_average": 1.5,
                  "branches_average": 0.0,
                  "conditions_average": 0.0,
                  "assignments_min": 0,
                  "assignments_max": 3,
                  "branches_min": 0,
                  "branches_max": 0,
                  "conditions_min": 0,
                  "conditions_max": 0
                }
                "#
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
                    @r#"
                {
                  "assignments": 0,
                  "branches": 2,
                  "conditions": 0,
                  "magnitude": 2.0,
                  "value": 0.0,
                  "assignments_average": 0.0,
                  "branches_average": 1.0,
                  "conditions_average": 0.0,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 0,
                  "branches_max": 2,
                  "conditions_min": 0,
                  "conditions_max": 0
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_control_flow_counts_conditions() {
        // Regression for #696: Bash control-flow branches are ABC
        // conditions (a Bash predicate is a command, so the branch keyword
        // is the only condition signal). Each mirrors a cyclomatic decision.
        //
        // expected: 4 conditions — `if` (1) + `elif` (1) + `while` (1) +
        // the non-wildcard case arm `a)` (1). The bare-`*)` wildcard arm is
        // the Bash analogue of `default:` and is excluded, exactly as the
        // cyclomatic standard count excludes it. No comparison / test
        // operators appear, so every condition here is control-flow.
        check_metrics::<BashParser>(
            "f() {
                 if cmd; then
                     echo a
                 elif other; then
                     echo b
                 fi
                 while running; do
                     echo c
                 done
                 case \"$x\" in
                     a) echo d ;;
                     *) echo e ;;
                 esac
             }",
            "foo.sh",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
            },
        );
    }

    #[test]
    fn bash_conditions_mix() {
        // Exercises every condition path: `==` and `!=` inside `[[ ]]`,
        // arithmetic `<` inside `(( ))`, and the prefix `-z` test operator
        // inside `[ ]`. Each `if` body's `echo` contributes a branch.
        //
        // expected: 8 conditions — each of the four `if`s contributes one
        // for the control-flow branch (#696) plus one for its comparison /
        // test operator (`==`, `!=`, `<`, `-z`). 4 branches (one `echo`
        // each). magnitude = sqrt(4² + 8²) = sqrt(80).
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
                assert_eq!(metric.abc.conditions_sum(), 8);
                assert_eq!(metric.abc.branches_sum(), 4);
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r#"
                {
                  "assignments": 0,
                  "branches": 4,
                  "conditions": 8,
                  "magnitude": 8.94427190999916,
                  "value": 0.0,
                  "assignments_average": 0.0,
                  "branches_average": 2.0,
                  "conditions_average": 4.0,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 0,
                  "branches_max": 4,
                  "conditions_min": 0,
                  "conditions_max": 8
                }
                "#
                );
            },
        );
    }

    #[test]
    fn bash_redirection_is_not_a_condition() {
        // `>` and `<` spell an I/O redirection as well as a comparison, and
        // the grammar parents the redirection under `file_redirect` rather
        // than `binary_expression`. Ungated, every redirect in a script
        // scored a condition: this fixture measured 2 before the parent
        // gate — the Bash instance of #1280's positive-parent polarity.
        // expected: 0 conditions — no test, no `if`, no comparison.
        check_metrics::<BashParser>(
            "f() {\n  echo hi > out.txt\n  read x < in.txt\n}\n",
            "foo.sh",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0);
                // The two `echo` / `read` commands still count as branches,
                // so the zero above is the gate firing rather than the walk
                // skipping the function body.
                assert_eq!(metric.abc.branches_sum(), 2);
            },
        );
    }

    #[test]
    fn bash_comparison_inside_an_arithmetic_or_test_context_is_a_condition() {
        // The positive control for the gate above: the same tokens under a
        // `binary_expression` are real comparisons in both the `[[ … ]]`
        // test form and the `(( … ))` arithmetic form, and still count.
        // expected: 4 — one `if` control-flow condition and one `>` per
        // function.
        check_metrics::<BashParser>(
            "f() {\n  if [[ $a > $b ]]; then :; fi\n}\ng() {\n  if (( a > b )); then :; fi\n}\n",
            "foo.sh",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
            },
        );
    }

    #[test]
    fn bash_arithmetic_ternary_is_a_condition() {
        // The ABC half of #1268. Cyclomatic and cognitive both count Bash's
        // only ternary form; ABC did not, so the identical construct scored
        // 1 here against the C family's 2.
        // expected: 2 — the `>` comparison and the ternary itself, matching
        // `int m = a > b ? a : b;` in C.
        check_metrics::<BashParser>(
            "f() {\n  local m=$(( a > b ? a : b ))\n}\n",
            "foo.sh",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    #[test]
    fn bash_magnitude() {
        // Combined assignments + branches + conditions. The single `if`
        // contributes two conditions (the control-flow branch, #696, plus
        // the `==` operator), so magnitude = sqrt(2² + 1² + 2²) = sqrt(9).
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
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(
                    metric.abc,
                    @r#"
                {
                  "assignments": 2,
                  "branches": 1,
                  "conditions": 2,
                  "magnitude": 3.0,
                  "value": 0.0,
                  "assignments_average": 1.0,
                  "branches_average": 0.5,
                  "conditions_average": 1.0,
                  "assignments_min": 0,
                  "assignments_max": 2,
                  "branches_min": 0,
                  "branches_max": 1,
                  "conditions_min": 0,
                  "conditions_max": 2
                }
                "#
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
            assert_eq!(metric.abc.assignments(), 0);
            assert_eq!(metric.abc.branches(), 0);
            assert_eq!(metric.abc.conditions(), 0);
            assert_eq!(metric.abc.magnitude(), 0.0);
        });
    }

    #[test]
    fn java_bool_returning_terminal_kinds_count() {
        // Companion to `csharp_bool_returning_terminal_kinds_count`
        // (issue #372 / lesson #19). Java's grammar wraps every
        // if/while/do condition in `parenthesized_expression`, so
        // the gap lived in `java_inspect_container`'s terminal-arm
        // recognizer: `FieldAccess` (`cfg.flag`), `CastExpression`
        // (`(boolean)v`), `ArrayAccess` (`flags[0]`), and
        // `InstanceofExpression` (`x instanceof Foo`) were never
        // counted. Java has no `await` or `is_pattern` analogues,
        // so the C# fix's five-kind set collapses to four here.
        //
        // expected: 4 conditions (one per `if`), 0 assignments,
        // 0 branches (no invocations).
        check_metrics::<JavaParser>(
            "class Cfg { boolean flag; }
            class A {
                void m(Object v, boolean[] flags, Cfg cfg) {
                    if (cfg.flag) { }
                    if ((boolean) v) { }
                    if (v instanceof Cfg) { }
                    if (flags[0]) { }
                }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 0);
            },
        );
    }

    // Issue #1274: a generic *declaration* is type syntax, not a
    // decision. `java_count_token_condition` denied only
    // `type_arguments`, so `class Gen<T>` and `<U> U ident(U x)` — both
    // `type_parameters` — each scored two conditions. The fix counts
    // `<` / `>` only under `binary_expression`, the polarity C / C++ /
    // Rust / Go already use; a `grammar.json` sweep proves the token is
    // emitted from exactly three productions, so the two shapes agree on
    // every well-formed input.
    //
    // One fixture per production the arm must ignore: the class-level
    // `type_parameters` (`Gen<T …>`), its `extends` bound's nested
    // `type_arguments` (`Comparable<T>`), the method-level
    // `type_parameters` (`<U>`), and a nested generic
    // (`Map<String, List<T>>`) whose two closing `>` lex as separate
    // tokens under separate `type_arguments`. Pre-fix this file scored
    // 4 conditions — two per `type_parameters` bracket pair; expected 0,
    // the file contains no conditional construct at all.
    #[test]
    fn java_generic_declarations_are_not_conditions() {
        check_metrics::<JavaParser>(
            "class Gen<T extends Comparable<T>> {
                <U> U ident(U x) { return x; }
                List<T> pick(Map<String, List<T>> m) { return m.get(\"k\"); }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0);
                // Non-vacuity guard: 0 is also what an unparsed file
                // scores, so pin a value only a walked body can produce
                // — the `m.get("k")` invocation.
                assert_eq!(metric.abc.branches_sum(), 1);
            },
        );
    }

    // Second half of #1274, same premise and same match: a wildcard type
    // argument's `?` is type syntax, not a ternary. tree-sitter-java emits
    // a bare `?` from exactly two productions — `ternary_expression` and
    // `wildcard` — so the `QMARK` arm carries the same allowlist gate the
    // `<` / `>` arm does.
    //
    // The body carries a real ternary over a real comparison so the
    // expected total is 2, not 0. Asserting 0 on a wildcard-only fixture
    // would have been vacuous — an unparsable file scores 0 too, so the
    // test could not tell "the wildcard was ignored" from "the fixture
    // stopped being recognised". Each failure mode now lands on its own
    // number: 4 if the wildcard `?` counts again (the pre-fix value), 3
    // if the gate is aimed at `Wildcard` instead, 1 if it swallows the
    // genuine ternary, 0 if parsing breaks. Two wildcards against one
    // ternary is deliberate — with one of each, aiming the gate at the
    // wrong one of the two productions still totals 2 and the test
    // cannot see the difference.
    #[test]
    fn java_generic_wildcard_is_not_a_condition() {
        check_metrics::<JavaParser>(
            "class A {
                int m(List<? extends Number> xs, Map<String, ? extends Number> ys,
                      int a, int b) { return a < b ? 1 : 2; }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // The other half of #1274: narrowing the `<` / `>` arm must not
    // swallow real comparisons. `List<String>` in the signature keeps a
    // generic in scope so the fixture proves both directions at once,
    // and the assertion is the grammar-dispatch §8 pin — on the
    // method's own space the ABC condition count must equal the
    // cyclomatic decision count (`cyclomatic()` minus the per-space
    // base of 1). Both are 2, one per `if`.
    #[test]
    fn java_comparison_operators_still_count_alongside_generics() {
        check_func_space::<JavaParser, _>(
            "class A {
                int m(List<String> xs, int a, int b) {
                    if (a < b) { return 1; }
                    if (a > b) { return 2; }
                    return 0;
                }
            }",
            "foo.java",
            |space| assert_deepest_conditions_match_cyclomatic(&space, 2),
        );
    }

    #[test]
    fn java_constructor_delegation_is_a_branch() {
        // Regression for #1279: `super(…)` / `this(…)` parse as
        // `explicit_constructor_invocation`, not `method_invocation`, so
        // each scored zero branches while Groovy scored one for identical
        // source. Both constructors delegate, and neither body contains any
        // other call.
        // expected: 2 branches — one delegation each.
        check_metrics::<JavaParser>(
            "class Sub extends Base {
                Sub() { super(1); }
                Sub(int a) { this(); }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
            },
        );
    }

    #[test]
    fn java_constructor_delegation_does_not_double_count_arguments() {
        // The delegation node does not wrap a `method_invocation` for the
        // call itself, so `super(f())` is exactly two branches — the
        // delegation and the argument call — not three (#1279).
        // expected: 2 branches.
        check_metrics::<JavaParser>(
            "class Sub extends Base {
                Sub() { super(f()); }
            }",
            "foo.java",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
            },
        );
    }

    #[test]
    fn csharp_constructor_initializer_is_a_branch() {
        // C# spells the same delegation as a `constructor_initializer`
        // (`: base(…)` / `: this(…)`), which likewise scored zero (#1279).
        // expected: 2 branches — one initializer each, no other calls.
        check_metrics::<CsharpParser>(
            "class Sub : Base {
                Sub() : base(1) { }
                Sub(int a) : this() { }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
            },
        );
    }

    #[test]
    fn kotlin_constructor_delegation_is_a_branch() {
        // Kotlin's secondary-constructor delegation is a
        // `constructor_delegation_call`, a distinct production from
        // `CallExpression`, so it scored zero for the same reason (#1279).
        // expected: 1 branch — the `: super(x)` delegation.
        check_metrics::<KotlinParser>(
            "class Sub : Base {
                constructor(x: Int) : super(x) { }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_constructor_delegation_is_a_branch() {
        // Groovy already counted this shape before #1279; the assertion
        // pins the JVM-family parity the Java and Kotlin fixes restore.
        // expected: 1 branch — the `super(1)` delegation.
        check_metrics::<GroovyParser>(
            "class Sub extends Base {
                Sub() { super(1) }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_no_abc() {
        // Comment-only file has no executable code → all-zero ABC.
        check_metrics::<GroovyParser>(
            "// just a comment, no executable code",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
            },
        );
    }

    #[test]
    fn groovy_single_assignment() {
        // `int x = 1` is a local-variable declaration whose `=` counts
        // as one assignment (matches Java's semantics).
        check_metrics::<GroovyParser>("int x = 1", "foo.groovy", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 1);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 6);
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
                assert_eq!(metric.abc.branches_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.branches_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.assignments_sum(), 1);
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
            assert_eq!(metric.abc.assignments_sum(), 1);
        });
    }

    #[test]
    fn groovy_bool_returning_terminal_kinds_count() {
        // Companion to `csharp_bool_returning_terminal_kinds_count`
        // (issue #372 / lesson #19). The dekobon Groovy grammar
        // shares Java's wrapping conventions for `FieldAccess` and
        // `InstanceofExpression`, but it splits casts into two
        // distinct kinds — `cast_expression` for the Groovy-idiomatic
        // `v as Boolean` and `parenthesized_type_cast` for the
        // Java-style `(boolean) v`. The grammar has no `await` or
        // `array_access` analogues, so the C# fix's five-kind set
        // collapses to four here (with the cast slot doubled).
        //
        // expected: 4 conditions (one per `if`), 0 assignments,
        // 0 branches (no invocations).
        check_metrics::<GroovyParser>(
            "class Cfg { boolean flag }
            class A {
                void m(Object v, Cfg cfg) {
                    if (cfg.flag) { }
                    if ((boolean) v) { }
                    if (v as Boolean) { }
                    if (v instanceof Cfg) { }
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 0);
            },
        );
    }

    // Groovy half of #1274 — see `java_generic_declarations_are_not_conditions`.
    // The class-level `type_parameters` and its bound's nested
    // `type_arguments` mirror the Java fixture; pre-fix this file scored
    // 2 conditions, expected 0.
    #[test]
    fn groovy_generic_declarations_are_not_conditions() {
        check_metrics::<GroovyParser>(
            "class Gen<T extends Comparable<T>> {
                List<T> pick(Map<String, List<T>> m) { m.get(\"k\") }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0);
                // Non-vacuity guard — see the Java counterpart.
                assert_eq!(metric.abc.branches_sum(), 1);
            },
        );
    }

    // Groovy counterpart of `java_generic_wildcard_is_not_a_condition`,
    // including its choice of a non-zero expected total and its
    // two-wildcards-one-ternary shape; the dekobon grammar emits the
    // same `wildcard` node. Pre-fix: 4.
    #[test]
    fn groovy_generic_wildcard_is_not_a_condition() {
        check_metrics::<GroovyParser>(
            "class A {
                int m(List<? extends Number> xs, Map<String, ? extends Number> ys,
                      int a, int b) { a < b ? 1 : 2 }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // A generic *method* gets its own fixture because the dekobon
    // grammar gives it its own production: `def <U> U ident(U x)` emits
    // `method_type_parameters`, not the `type_parameters` the class form
    // above uses. It is the fourth and last production from which that
    // grammar emits a bare `<` / `>` (the others being
    // `binary_expression`, `type_arguments` and `type_parameters`), and
    // the one a denylist extended only with `TypeParameters` — the fix
    // this issue's own plan proposed — would still miss. Revert-verified
    // in both directions: under that denylist this test still fails
    // while `groovy_generic_declarations_are_not_conditions` passes.
    //
    // The body carries a ternary over a comparison for the same
    // non-vacuity reason as the wildcard tests: expected 2, pre-fix 4
    // (the `<U>` bracket pair), 0 if the fixture stops parsing.
    #[test]
    fn groovy_method_type_parameters_are_not_conditions() {
        check_metrics::<GroovyParser>(
            "class A {
                def <U> U ident(U x, int a, int b) { a < b ? x : null }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // Groovy counterpart of
    // `java_comparison_operators_still_count_alongside_generics`: the
    // narrowed arm keeps counting real comparisons, pinned against the
    // cyclomatic decision count on the method's own space (§8).
    #[test]
    fn groovy_elvis_counts_one_condition_per_token() {
        // `a ?: c` is a short-circuit decision Groovy cyclomatic already
        // counts and the Kotlin ABC arm counts for its identical token;
        // Groovy's arm listed neither `?:` nor its `elvis_expression`, so
        // a method whose only branching is elvis chains reported
        // cyclomatic > 1 with zero conditions. One per token, so the
        // grammar-dispatch §8 invariant holds on the chain:
        // `conditions() == cyclomatic() - 1`.
        check_func_space::<GroovyParser, _>(
            "class K { def f(a, c) { return a ?: c } }",
            "foo.groovy",
            |space| assert_deepest_conditions_match_cyclomatic(&space, 1),
        );
        check_func_space::<GroovyParser, _>(
            "class K { def g(a, b, c) { return a ?: b ?: c } }",
            "foo.groovy",
            |space| assert_deepest_conditions_match_cyclomatic(&space, 2),
        );
    }

    #[test]
    fn groovy_comparison_operators_still_count_alongside_generics() {
        check_func_space::<GroovyParser, _>(
            "class A {
                int m(List<String> xs, int a, int b) {
                    if (a < b) { return 1 }
                    if (a > b) { return 2 }
                    return 0
                }
            }",
            "foo.groovy",
            |space| assert_deepest_conditions_match_cyclomatic(&space, 2),
        );
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
                assert_eq!(metric.abc.conditions_sum(), 8);
                // Three `println a` juxt calls — each is a branch.
                assert_eq!(metric.abc.branches_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 3);
                // Two assignments to existing variables (`a = false`,
                // `b = !b`).
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn groovy_if_while_boolean_literal_condition() {
        // Regression for the Groovy half of #371-class bugs: the
        // dekobon tree-sitter-groovy grammar wraps a bare
        // `true` / `false` literal used as the condition of
        // `if` / `while` / `do` / `?:` in a `boolean_literal` node
        // (`Groovy::BooleanLiteral`, kind_id 270), not the leaf
        // `True` / `False` keyword tokens. `groovy_count_condition`
        // must therefore match `BooleanLiteral` (the wrapper).
        // Without that, every literal-condition statement silently
        // scored 0 conditions. Mirror of
        // `csharp_if_while_boolean_literal_condition`.
        check_metrics::<GroovyParser>(
            "void m() {
                if (true) { println 'a' }
                if (false) { println 'b' }
                while (true) { break }
                int t = true ? 1 : 0
            }",
            "foo.groovy",
            |metric| {
                // Four literal-condition statements contribute 4
                // `BooleanLiteral` conditions (if / if / while /
                // ternary), plus the ternary's `?` token adds one
                // more via `groovy_count_token_condition` → 5
                // total. The `println` calls contribute 2 branches
                // (the `while` body's `break` is not a branch).
                // The `int t = …` initializer contributes 1
                // assignment.
                assert_eq!(metric.abc.conditions_sum(), 5);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.assignments_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_return_unary_boolean_literal() {
        // Companion to `groovy_if_while_boolean_literal_condition`:
        // a `!true` / `!false` operand inside a `return` statement
        // routes through `groovy_inspect_container` (via
        // `groovy_inspect_child(node, 1)` on the ReturnStatement).
        // The `!` operator establishes boolean context, then the
        // innermost-operand check matches `BooleanLiteral` — that
        // helper's `BooleanLiteral` arm must be present or the
        // count silently drops. Mutation-verified: removing
        // `BooleanLiteral` from `groovy_inspect_container` leaves
        // every other Groovy test passing.
        check_metrics::<GroovyParser>(
            "boolean f() {
                return !true
            }
            boolean g() {
                return !false
            }",
            "foo.groovy",
            |metric| {
                // Each `return !X` walks into
                // `groovy_inspect_container` with a UnaryExpression
                // wrapping a `BANG` + BooleanLiteral. The `!` arm
                // seeds `has_boolean_content = true` (ReturnStatement
                // is not a known-boolean parent), then the
                // BooleanLiteral operand contributes one condition.
                // Two `return !X` → 2 conditions, no branches, no
                // assignments.
                assert_eq!(metric.abc.conditions_sum(), 2);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.assignments_sum(), 0);
            },
        );
    }

    #[test]
    fn groovy_short_circuit_with_boolean_literal_operand() {
        // Companion to `groovy_if_while_boolean_literal_condition`:
        // a bare `true` / `false` operand of `&&` / `||` lands in
        // `groovy_count_unary_conditions`, which iterates the
        // parent BinaryExpression's children. That helper must
        // match the `BooleanLiteral` wrapper just like
        // `groovy_count_condition` does — otherwise the operand
        // silently scores zero. Mutation-verified: removing
        // `BooleanLiteral` from the `groovy_count_unary_conditions`
        // arm leaves every other Groovy test passing.
        check_metrics::<GroovyParser>(
            "void m(boolean x) {
                if (x && true) { println 'a' }
                if (false || x) { println 'b' }
            }",
            "foo.groovy",
            |metric| {
                // `&&` and `||` themselves are NOT in
                // `groovy_count_token_condition`'s match list —
                // they route through
                // `groovy_walk_for_conditions::AMPAMP|PIPEPIPE`,
                // which calls `groovy_count_unary_conditions` on
                // the parent BinaryExpression. Each invocation
                // counts every child that matches the terminal-
                // operand kinds and whose parent is a
                // BinaryExpression. For `x && true`: Identifier x
                // (+1) + BooleanLiteral true (+1) = 2. For
                // `false || x`: BooleanLiteral false (+1) +
                // Identifier x (+1) = 2. Total 4.
                assert_eq!(metric.abc.conditions_sum(), 4);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.assignments_sum(), 0);
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
                assert_eq!(metric.abc.branches_sum(), 3);
                // Three `!` unaries — `m1(!a)` and the two args of
                // `m2(!a, !b)` — each contribute one condition via
                // the ArgumentList inspection.
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
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
                assert_eq!(metric.abc.assignments_sum(), 2);
                // `i < 10` is one condition (the LT arm).
                assert_eq!(metric.abc.conditions_sum(), 1);
            },
        );
    }

    /// The existing `for` test uses `i < 10`, which the `LT` token arm
    /// counts on its own — `groovy_walk_for_statement` never
    /// contributes there, so it stayed uncovered. A bare-identifier
    /// condition has no comparison token, so the count can only come
    /// from the walker.
    #[test]
    fn groovy_for_with_bare_identifier_condition() {
        check_metrics::<GroovyParser>(
            "void f(boolean go) {
                for (int i = 0; go; i++) {
                    println(i)
                }
            }",
            "foo.groovy",
            |metric| {
                // `go` is the whole condition and counts once.
                assert_eq!(metric.abc.conditions_sum(), 1);
                // `int i = 0` (EQ) + `i++` (PLUSPLUS) = 2, as in
                // `groovy_for_with_variable_declaration`.
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    /// The same slot with the initialiser hoisted out of the header.
    /// Under the pre-#1276 positional cascade this was a distinct code
    /// path — the condition moved from child(4) to child(3) — and the
    /// pair is kept as a shape guard now that the walker reads the
    /// `condition` field and cannot see the difference.
    #[test]
    fn groovy_for_with_empty_initializer_counts_the_condition() {
        check_metrics::<GroovyParser>(
            "void f(boolean go) {
                int i = 0
                for (; go; i++) {
                    println(i)
                }
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
                // `int i = 0` (EQ) + `i++` (PLUSPLUS), as above — the
                // initialiser just moved out of the loop header.
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    /// Issue #1276 changed Groovy's answer here, exactly as it changed
    /// Java's: the positional cascade counted a `;` / `)` landing at
    /// child(4) as a vacuously-true condition, so `for (;;)` scored
    /// one. An omitted test is not a decision, and every other impl
    /// scores it zero. See `java_empty_for_condition_counts_nothing`.
    #[test]
    fn groovy_empty_for_condition_counts_nothing() {
        check_metrics::<GroovyParser>("void f() { for (;;) { break } }", "foo.groovy", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 0);
        });
        // The second spelling that moved. Unlike Java's, Groovy's
        // `local_variable_declaration` does not swallow its `;`, so the
        // old cascade found `;` at child(3) and `;` at child(4) here
        // too, and counted one.
        check_metrics::<GroovyParser>(
            "void f() { for (int i = 0; ; i++) { break } }",
            "foo.groovy",
            |metric| assert_eq!(metric.abc.conditions_sum(), 0),
        );
    }

    /// The cascade's other defect, shared with Java: a comment in the
    /// header shifted every child index, so the condition went unread.
    /// Reading the `condition` field cannot shift.
    #[test]
    fn groovy_for_condition_survives_a_header_comment() {
        check_metrics::<GroovyParser>(
            "void f(boolean go) { for (; /* n */ go; ) { break } }",
            "foo.groovy",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
        );
        // Unchanged control: the same loop without the comment.
        check_metrics::<GroovyParser>(
            "void f(boolean go) { for (; go; ) { break } }",
            "foo.groovy",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
        );
    }

    /// C#'s `csharp_walk_for_statement` reads the loop condition off
    /// the named `condition` field and routes a parenthesised or
    /// `!`-prefixed one through `csharp_inspect_container`. Every other
    /// C# `for` test uses a comparison (`i < n`), which the `LT` token
    /// arm counts without entering the walker.
    #[test]
    fn csharp_for_with_negated_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                void M(bool done) {
                    for (int i = 0; !done; i++) { System.Console.WriteLine(i); }
                }
            }",
            "foo.cs",
            |metric| {
                // `!done` unwraps to the `done` terminal: one condition,
                // and no comparison token to double-count it.
                assert_eq!(metric.abc.conditions_sum(), 1);
                // `int i = 0` + `i++`.
                assert_eq!(metric.abc.assignments_sum(), 2);
                assert_eq!(metric.abc.branches_sum(), 1);
            },
        );
    }

    #[test]
    fn groovy_eq_arm_counts_outside_final_declarations() {
        // Bare reassignment of an already-declared variable: the `=`
        // belongs to no `final` declaration, so it counts. Mirrors
        // `java_eq_arm_counts_outside_final_declarations`.
        check_metrics::<GroovyParser>(
            "void f(int x) {
                x = 42
            }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
            },
        );
    }

    #[test]
    fn groovy_final_field_initializer_does_not_suppress_the_closure_body() {
        // The Groovy spelling of
        // `java_final_initializer_does_not_suppress_the_assignments_inside_it`:
        // a `final` field's closure body opens no space, and the sentinel
        // stack suppressed its `x = 1` with the declarator's own `=`.
        check_metrics::<GroovyParser>(
            "class K {
                int x
                final Closure c = { x = 1 }
                Closure d = { x = 2 }
                final int q = 3
            }",
            "foo.groovy",
            |metric| {
                // `x = 1`, `d = {…}`, `x = 2`; the two `final`
                // initializers are suppressed. Pre-fix: 2.
                assert_eq!(metric.abc.assignments_sum(), 3);
            },
        );
    }

    #[test]
    fn groovy_final_local_is_an_error_at_the_pinned_grammar() {
        // `groovy_eq_initializes_final_binding` lists
        // `local_variable_declaration` for symmetry with Java, but at
        // dekobon-tree-sitter-groovy 0.2.2 a `final` local never
        // reaches it: the parser emits an `ERROR` node for the
        // declaration, with or without a terminator, so both of its `=`
        // tokens count. This pins the grammar limitation so a bump that
        // starts parsing the local shows up here rather than as a silent
        // change in what the predicate suppresses.
        let source = b"class K { int x; def m() { final int a = 0; x = 1 } }";
        let parser = GroovyParser::new(
            source.to_vec(),
            &std::path::PathBuf::from("foo.groovy"),
            None,
        );
        assert!(
            ast_has_kind_id(&parser, u16::MAX),
            "a `final` local should still parse to an ERROR node; if the grammar \
             now accepts it, re-derive the local half of the predicate",
        );
        check_metrics::<GroovyParser>(
            "class K { int x; def m() { final int a = 0; x = 1 } }",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn csharp_const_initializer_shapes() {
        // `csharp_eq_initializes_const_binding` reads the `const`
        // through the `modifier` node one hop above the
        // `variable_declaration`, for a local and a field alike, behind
        // other modifiers; `readonly` is not `const` and its initializer
        // counts. A lambda's body opens its own space in C#, so unlike
        // Java there was never a nested `=` to leak — the row pins that
        // the structural rule keeps the count the sentinel gave.
        check_metrics::<CsharpParser>(
            "class K {
                int x;
                private const int Q = 1;
                public static readonly int R = 2;
                void M() { const int q = 3; System.Action a = () => { x = 4; }; }
            }",
            "foo.cs",
            |metric| {
                // `R = 2`, `a = () => …`, `x = 4`.
                assert_eq!(metric.abc.assignments_sum(), 3);
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

    // C# `switch` *expression* arms scored zero ABC conditions before
    // #456 — they carry no `case` / `default` token, so the token-driven
    // `csharp_count_token_condition` never saw them, even though C#
    // cyclomatic counts each non-discard arm. Revert-verified: adding the
    // gated `SwitchExpressionArm` arm is what lifts this from 0 to 2. The
    // bare `_ =>` discard arm is excluded (the `default:` analogue),
    // mirroring the cyclomatic gate (lesson 11).
    #[test]
    fn csharp_switch_expression_arm_counts_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                int M(int x) {
                    return x switch { 1 => 10, 2 => 20, _ => 0 };
                }
            }",
            "foo.cs",
            |metric| {
                // arm `1 =>` (+1) + arm `2 =>` (+1) + `_ =>` discard (+0).
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // Cross-language parity (lesson 11): a C# `switch` expression and the
    // equivalent Java arrow-`switch` must report the same ABC condition
    // count on equivalent code. Both have two concrete case arms and no
    // fallback arm, so both must count exactly 2. `check_metrics` takes a
    // non-capturing `fn` pointer, so the shared expected value (2) is
    // asserted in each callback rather than compared across closures; the
    // matching constant is what enforces parity. This guards against the
    // C# fix drifting away from the Java arrow-case treatment.
    #[test]
    fn csharp_java_switch_arm_abc_parity() {
        // C# switch expression: two arms, no fallback → 2 conditions.
        check_metrics::<CsharpParser>(
            "class A {
                int M(int x) {
                    return x switch { 1 => 10, 2 => 20 };
                }
            }",
            "foo.cs",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );

        // Equivalent Java arrow-`switch`: two case arms, no default → 2.
        check_metrics::<JavaParser>(
            "class A {
                int m(int x) {
                    return switch (x) { case 1 -> 10; case 2 -> 20; };
                }
            }",
            "foo.java",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    // Issue #469: the `default` arm of a C-family `switch` is the
    // unconditional fallthrough and must NOT count as an ABC condition,
    // mirroring cyclomatic — which counts only the `Case` arms, never
    // the `Default` token.
    //
    // expected: each fixture is a single function whose switch has two
    // concrete `case` arms plus one `default`. ABC must count exactly
    // the two case arms (conditions = 2), matching cyclomatic's two
    // case-arm decisions. Pre-fix, every language below scored 3 (the
    // `Default` token leaked into the condition tally) — revert-verified
    // against the pre-#469 condition arms. We anchor on the integer
    // `conditions_sum()` headline (the value the public JSON serializes;
    // float magnitude is bit-brittle and excluded by the snapshot
    // policy). The cyclomatic side is pinned separately in
    // `java_csharp_cpp_switch_default_cyclomatic_parity` below, where
    // the per-space `cyclomatic()` decision count is isolated.
    #[test]
    fn java_switch_default_not_a_condition() {
        // Classic statement `default:`.
        check_metrics::<JavaParser>(
            "class A {
                int m(int x) {
                    switch (x) { case 1: return 1; case 2: return 2; default: return 0; }
                }
            }",
            "foo.java",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
        // Arrow `default ->` — shares the same `Default` token.
        check_metrics::<JavaParser>(
            "class A {
                int m(int x) {
                    return switch (x) { case 1 -> 1; case 2 -> 2; default -> 0; };
                }
            }",
            "foo.java",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    // Over-exclusion guard (issue #469): a statement `switch` with two
    // `case` arms and NO `default` must still count both cases. This
    // pins that the fix excludes only the `Default` token, never a
    // `Case` arm — the count is identical before and after #469 (two
    // cases → 2), so it would catch a fix that over-eagerly dropped a
    // real case (e.g. treating the trailing case as a fallthrough).
    // expected: case 1 (+1) + case 2 (+1) = 2.
    #[test]
    fn java_switch_without_default_counts_all_cases() {
        check_metrics::<JavaParser>(
            "class A {
                int m(int x) {
                    switch (x) { case 1: return 1; case 2: return 2; }
                    return -1;
                }
            }",
            "foo.java",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    #[test]
    fn csharp_switch_default_not_a_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                int M(int x) {
                    switch (x) { case 1: return 1; case 2: return 2; default: return 0; }
                }
            }",
            "foo.cs",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    #[test]
    fn cpp_switch_default_not_a_condition() {
        // C++ (and plain C, which shares this grammar) already excluded
        // `default`; this pins the cross-language parity invariant.
        check_metrics::<CppParser>(
            "void f(int x) {
                 switch (x) { case 1: return; case 2: return; default: return; }
             }",
            "foo.cpp",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    #[test]
    fn objc_abc() {
        // ObjC ABC reuses the C/C++ walker with two additions: a message
        // send `[obj msg]` is a call (B), and `@try` / `@catch` count as
        // conditions (C) like C++ try/catch.
        //   A: `int total = 0`, `int i = 0`, `i++`, `total = total + …`,
        //      `total = -1` = 5.
        //   B: `[self valueAt:i]`, `[self risky]` = 2 message sends.
        //   C: `i < n`, `@try`, `@catch`, `total >= 0` = 4.
        check_metrics::<ObjcParser>(
            "@implementation Foo\n\
             - (int)bar:(int)n {\n\
                 int total = 0;\n\
                 for (int i = 0; i < n; i++) {\n\
                     total = total + [self valueAt:i];\n\
                 }\n\
                 @try {\n\
                     [self risky];\n\
                 } @catch (NSException *e) {\n\
                     total = -1;\n\
                 }\n\
                 if (total >= 0) {\n\
                     return total;\n\
                 }\n\
                 return 0;\n\
             }\n\
             @end\n",
            "foo.m",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 5);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 4);
            },
        );
    }

    #[test]
    fn objc_abc_conditions() {
        // Exercises the condition-slot arms shared with C/C++: a `while`
        // head, a `&&` chain, a `do … while` trailing condition, and a
        // `return <comparison>`. ObjC routes these through the same
        // grammar-agnostic `cpp_inspect_*` helpers.
        //   A: `p++`, `p--` = 2.   B: no calls = 0.
        //   C: `p != 0`, `*p > 0`, `*p < 9`, `*p == 0` = 4.
        check_metrics::<ObjcParser>(
            "@implementation Foo\n\
             - (int)g:(int *)p {\n\
                 while (p != 0 && *p > 0) {\n\
                     p++;\n\
                 }\n\
                 do {\n\
                     p--;\n\
                 } while (*p < 9);\n\
                 return *p == 0;\n\
             }\n\
             @end\n",
            "foo.m",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 4);
            },
        );
    }

    #[test]
    fn objc_abc_message_send_unary_condition() {
        // A negated boolean passed as a message-send argument is a unary
        // condition (Fitzpatrick Rule 9), the same as in a C-call argument.
        // Message args are direct children of `message_expression` (no
        // `argument_list`), so they are inspected in the `MessageExpression`
        // arm. Here: `[self use:!a]` (1 call + 1 unary condition) +
        // `cFunc(!a)` (1 call + 1 unary condition) → B=2, C=2.
        check_metrics::<ObjcParser>(
            "@implementation Foo\n\
             - (void)bar:(int)a {\n\
                 [self use:!a];\n\
                 cFunc(!a);\n\
             }\n\
             @end\n",
            "foo.m",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    #[test]
    fn objc_message_send_is_a_bool_terminal_in_condition_slots() {
        // `[obj ok]` is Objective-C's call, so in a condition slot it is
        // the unary condition `ok()` is (Fitzpatrick Rule 9). Until
        // `message_expression` joined `cpp_bool_terminal_kinds!` every
        // row here scored zero conditions where its C-call twin scored
        // one — and #1276's `for` slot inherited the gap. Each row is a
        // message send as the *whole* slot (grammar-dispatch §11: a
        // comparison would be counted by its own operator arm anyway),
        // and branches are asserted beside conditions so the arm cannot
        // be read as counting the call twice. The last three are the
        // over-count guards: a value position counts nothing, as `ok()`'s
        // does not.
        let cases = [
            ("if ([self ok]) {}", 1, 1),
            ("while (![self ok]) {}", 1, 1),
            ("do {} while ([self ok]);", 1, 1),
            ("for (; [self ok]; ) {}", 1, 1),
            ("x = [self ok] ? 1 : 2;", 2, 1),
            ("if ([self ok] && [o ok]) {}", 2, 2),
            ("return [self ok];", 0, 1),
            ("[self use:[self ok]];", 0, 2),
            ("[self use:![self ok]];", 1, 2),
        ];
        let mut ran = 0;
        for (body, conditions, branches) in cases {
            let src = format!("@implementation Foo\n- (int)bar {{\n    {body}\n}}\n@end\n");
            let abc = metrics_verbatim(
                LANG::Objc,
                src.as_bytes(),
                MetricsOptions::default().with_only(&[crate::Metric::Abc]),
            )
            .abc;
            assert_eq!(abc.conditions_sum(), conditions, "`{body}` conditions");
            assert_eq!(abc.branches_sum(), branches, "`{body}` branches");
            ran += 1;
        }
        assert_eq!(ran, cases.len());
        // Both answers are present, so a walker stuck at 0 or at 1 cannot
        // pass half the table silently.
        assert!(cases.iter().any(|&(_, c, _)| c == 0));
        assert!(cases.iter().any(|&(_, c, _)| c == 2));
    }

    #[test]
    fn objc_message_send_condition_agrees_with_c_call() {
        // The intra-ObjC parity C++ cannot express: a message send and a
        // C call in the same slot score alike. Non-degenerate by the
        // `assert_eq!(…, 1)` on the reference.
        for (send, call) in [
            ("if ([a ok]) {}", "if (ok()) {}"),
            ("for (; [a ok]; ) {}", "for (; ok(); ) {}"),
            ("x = [a ok] ? 1 : 2;", "x = ok() ? 1 : 2;"),
        ] {
            let wrap =
                |body: &str| format!("@implementation Foo\n- (int)bar {{\n    {body}\n}}\n@end\n");
            let reference = abc_conditions(LANG::Objc, &wrap(call));
            assert!(reference >= 1, "`{call}` must count at least the slot");
            assert_eq!(
                abc_conditions(LANG::Objc, &wrap(send)),
                reference,
                "`{send}`"
            );
        }
    }

    #[test]
    fn groovy_switch_default_not_a_condition() {
        check_metrics::<GroovyParser>(
            "class A {
                int m(int x) {
                    switch (x) { case 1: return 1; case 2: return 2; default: return 0 }
                }
            }",
            "foo.groovy",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    #[test]
    fn js_switch_default_not_a_condition() {
        check_metrics::<JavascriptParser>(
            "function f(x) {
                 switch (x) { case 1: return 1; case 2: return 2; default: return 0; }
             }",
            "foo.js",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    #[test]
    fn ts_switch_default_not_a_condition() {
        check_metrics::<TypescriptParser>(
            "function f(x: number): number {
                 switch (x) { case 1: return 1; case 2: return 2; default: return 0; }
             }",
            "foo.ts",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    // Cross-language parity (lesson 11): the equivalent statement-`switch`
    // with a `default` arm reports the same ABC condition count across
    // Java / C# / C++. All three have two concrete case arms plus a
    // fallthrough `default`, so all three must count exactly 2 conditions
    // (the `default` excluded). `check_metrics` takes a non-capturing
    // `fn` pointer, so the shared expected value is asserted in each
    // callback; the matching constant is what enforces parity.
    #[test]
    fn java_csharp_cpp_switch_default_abc_parity() {
        check_metrics::<JavaParser>(
            "class A {
                int m(int x) {
                    switch (x) { case 1: return 1; case 2: return 2; default: return 0; }
                }
            }",
            "foo.java",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
        check_metrics::<CsharpParser>(
            "class A {
                int M(int x) {
                    switch (x) { case 1: return 1; case 2: return 2; default: return 0; }
                }
            }",
            "foo.cs",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
        check_metrics::<CppParser>(
            "void f(int x) {
                 switch (x) { case 1: return; case 2: return; default: return; }
             }",
            "foo.cpp",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    // Pins the ABC-vs-cyclomatic agreement the fix is about (lesson 11):
    // on the method's own function space, the cyclomatic decision count
    // (`cyclomatic()` minus the per-space base of 1) must equal the ABC
    // `conditions()` for the same switch. Both must be 2 — the two case
    // arms — with the `default` excluded from each. Revert-verified: pre-
    // #469 ABC `conditions()` was 3 here while cyclomatic stayed at 2.
    #[test]
    fn java_csharp_cpp_switch_default_cyclomatic_parity() {
        check_func_space::<JavaParser, _>(
            "class A {
                int m(int x) {
                    switch (x) { case 1: return 1; case 2: return 2; default: return 0; }
                }
            }",
            "foo.java",
            |space| assert_deepest_conditions_match_cyclomatic(&space, 2),
        );
        check_func_space::<CsharpParser, _>(
            "class A {
                int M(int x) {
                    switch (x) { case 1: return 1; case 2: return 2; default: return 0; }
                }
            }",
            "foo.cs",
            |space| assert_deepest_conditions_match_cyclomatic(&space, 2),
        );
        check_func_space::<CppParser, _>(
            "void f(int x) {
                 switch (x) { case 1: return; case 2: return; default: return; }
             }",
            "foo.cpp",
            |space| assert_deepest_conditions_match_cyclomatic(&space, 2),
        );
    }

    // Issue #473: PHP `switch` `default:` (`DefaultStatement`) is the
    // unconditional fallthrough, not a condition. ABC `conditions()` must
    // equal the cyclomatic decision count (`cyclomatic() - 1`) on the
    // function's own space — both 2 for the two `case` arms, with the
    // `default` excluded. Revert-verified: re-adding `DefaultStatement` to
    // the PHP ABC condition arm makes `conditions()` 3 here while cyclomatic
    // stays at 2, failing the invariant.
    #[test]
    fn php_switch_default_not_a_condition() {
        check_func_space::<PhpParser, _>(
            "<?php
            function f($x) {
                switch ($x) {
                    case 1: return 1;
                    case 2: return 2;
                    default: return 0;
                }
            }",
            "foo.php",
            |space| assert_deepest_conditions_match_cyclomatic(&space, 2),
        );
    }

    // Issue #473: PHP `match` `default =>` (`MatchDefaultExpression`) is the
    // unconditional fallthrough, mirroring the switch `default:` case above.
    // ABC `conditions()` must equal the cyclomatic decision count
    // (`cyclomatic() - 1`) — both 2 for the two non-default match arms.
    // Revert-verified: re-adding `MatchDefaultExpression` to the PHP ABC
    // condition arm makes `conditions()` 3 here while cyclomatic stays at 2.
    #[test]
    fn php_match_default_not_a_condition() {
        check_func_space::<PhpParser, _>(
            "<?php
            function g($x) {
                return match ($x) {
                    1 => \"a\",
                    2 => \"b\",
                    default => \"z\",
                };
            }",
            "foo.php",
            |space| assert_deepest_conditions_match_cyclomatic(&space, 2),
        );
    }

    #[test]
    fn csharp_if_bare_identifier_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                void M(bool x) {
                    if (x) { System.Console.WriteLine(\"a\"); }
                }
            }",
            "foo.cs",
            |metric| {
                // `if (x)` contributes 1 condition (bare identifier).
                // `System.Console.WriteLine(...)` is the only call → 1 branch.
                // `*_sum()` is what the public JSON serializes as the
                // headline value (see `crate::wire::Abc`).
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.assignments_sum(), 0);
            },
        );
    }

    #[test]
    fn csharp_while_bare_identifier_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                void M(bool x) {
                    while (x) { x = false; }
                }
            }",
            "foo.cs",
            |metric| {
                // `while (x)` contributes 1 condition; `x = false` is 1 assignment.
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 0);
            },
        );
    }

    #[test]
    fn csharp_do_while_bare_identifier_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                void M(bool x) {
                    do { x = true; } while (x);
                }
            }",
            "foo.cs",
            |metric| {
                // `do { ... } while (x)` contributes 1 condition;
                // `x = true` is 1 assignment.
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 0);
            },
        );
    }

    #[test]
    fn csharp_if_unary_not_condition() {
        // Two cases share one test:
        //
        //   if (!x) { … }  — IfStatement is a known-boolean parent, so
        //   the unary `!` arm in `csharp_inspect_container` is *one of
        //   two* ways `has_boolean_content` gets set to true (the parent
        //   seed sets it before the `!` does). A regression that broke
        //   only the `is_not` branch wouldn't show up here.
        //
        //   return !x;  — ReturnStatement is *not* in the boolean-context
        //   seed list (BinaryExpression | IfStatement | WhileStatement |
        //   DoStatement | ForStatement | ConditionalExpression). So the
        //   `!` wrapper is the *only* path that sets
        //   `has_boolean_content = true`. Asserting the `return !x;`
        //   case isolates the unary-unwrap logic from the parent-seed
        //   path.
        check_metrics::<CsharpParser>(
            "class A {
                void M(bool x) {
                    if (!x) { System.Console.WriteLine(\"a\"); }
                }
                bool N(bool x) {
                    return !x;
                }
            }",
            "foo.cs",
            |metric| {
                // `if (!x)` contributes 1 condition (PrefixUnaryExpression
                // path with parent IfStatement seeding has_boolean_content).
                // `return !x;` contributes 1 condition (parent doesn't seed
                // — the unary `!` is the only path that sets the flag).
                // → 2 conditions total. 1 branch from WriteLine().
                assert_eq!(metric.abc.conditions_sum(), 2);
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.assignments_sum(), 0);
            },
        );
    }

    #[test]
    fn csharp_if_double_parenthesized_condition() {
        // Audit-tests follow-up: with only the
        // `csharp_prefix_unary_expr_kinds!()` arm covered by
        // `csharp_if_unary_not_condition`, the
        // `csharp_paren_expr_kinds!()` delegation arm in
        // `csharp_count_condition` was a pure dead-code candidate —
        // disabling it caused zero existing tests to fail (verified
        // 2026-05-26).
        //
        // `if ((x))` puts a `ParenthesizedExpression` at child(2) of
        // the IfStatement (child(1) is the literal `(`, child(2) is
        // the inner parenthesised expression, child(3) is the literal
        // `)`). `csharp_count_condition` must route that case to
        // `csharp_inspect_container`, which then sees parent =
        // IfStatement, seeds `has_boolean_content = true`, walks to
        // the inner Identifier, and counts it. A regression that
        // removed the paren arm would silently score 0.
        check_metrics::<CsharpParser>(
            "class A {
                void M(bool x) {
                    if ((x)) { System.Console.WriteLine(\"a\"); }
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.assignments_sum(), 0);
            },
        );
    }

    #[test]
    fn csharp_bool_returning_terminal_kinds_count() {
        // Regression for issue #372 (lesson #19): before the fix,
        // `csharp_count_condition` / `csharp_inspect_container` only
        // recognised invocation / identifier / boolean literal as
        // terminal-bool operands, so the five idiomatic boolean
        // expressions in the `if (...)` slots below silently scored
        // zero conditions:
        //
        //   - `cfg.flag`        — MemberAccessExpression
        //   - `await c.Check()` — AwaitExpression
        //   - `(bool)v`         — CastExpression
        //   - `v is not null`   — IsPatternExpression
        //   - `flags[0]`        — ElementAccessExpression
        //
        // expected: 5 conditions (one per `if`), 0 assignments,
        // 1 branch (the single `c.Check()` invocation; the other
        // `if`-condition expressions are not invocations).
        check_metrics::<CsharpParser>(
            "using System.Threading.Tasks;
            class A {
                async Task M(object v, bool[] flags, Cfg cfg, C c) {
                    if (cfg.flag) { }
                    if (await c.Check()) { }
                    if ((bool)v) { }
                    if (v is not null) { }
                    if (flags[0]) { }
                }
            }
            class Cfg { public bool flag; }
            class C { public Task<bool> Check() => null; }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 5);
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 1);
            },
        );
    }

    #[test]
    fn csharp_if_method_call_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                void M(string s) {
                    if (s.StartsWith(\"x\")) { System.Console.WriteLine(\"a\"); }
                }
            }",
            "foo.cs",
            |metric| {
                // `if (s.StartsWith("x"))` contributes 1 condition
                // (InvocationExpression) plus 1 branch for the call itself,
                // plus 1 branch for WriteLine.
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.assignments_sum(), 0);
            },
        );
    }

    #[test]
    fn csharp_if_while_boolean_literal_condition() {
        // Regression for #371: the tree-sitter-c-sharp grammar wraps a
        // bare `true` / `false` literal used as the condition of
        // `if` / `while` / `do` / `?:` in a `boolean_literal` node,
        // not the leaf `true` / `false` tokens. `csharp_count_condition`
        // must therefore match `BooleanLiteral` (the wrapper),
        // mirroring the existing `csharp_walk_for_statement` arm.
        // Without that, every literal-condition statement scored 0
        // conditions. The sibling `csharp_count_unary_conditions`
        // arm is covered separately by
        // `csharp_short_circuit_with_boolean_literal_operand` and
        // `csharp_inspect_container` is covered by
        // `csharp_declarations_with_conditions` (`!true` / `!false`).
        check_metrics::<CsharpParser>(
            "class A {
                void M() {
                    if (true) { System.Console.WriteLine(\"a\"); }
                    if (false) { System.Console.WriteLine(\"b\"); }
                    while (true) { break; }
                    do { break; } while (false);
                    int t = true ? 1 : 0;
                }
            }",
            "foo.cs",
            |metric| {
                // Five literal-condition statements contribute 5
                // `BooleanLiteral` conditions (one per if/if/while/
                // do-while/ternary), plus the ternary's `?` token
                // adds one more via `csharp_count_token_condition`
                // → 6 total. The two `System.Console.WriteLine`
                // calls contribute 2 branches; the `int t = …`
                // initializer contributes 1 assignment.
                assert_eq!(metric.abc.conditions_sum(), 6);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.assignments_sum(), 1);
            },
        );
    }

    #[test]
    fn csharp_short_circuit_with_boolean_literal_operand() {
        // Regression for #371 (companion to
        // `csharp_if_while_boolean_literal_condition`): a bare
        // `true` / `false` operand of `&&` / `||` lands in
        // `csharp_count_unary_conditions`, which iterates the parent
        // BinaryExpression's children. That helper must match the
        // `BooleanLiteral` wrapper just like `csharp_count_condition`
        // does — otherwise the operand silently scores zero. Mutation-
        // verified: removing `BooleanLiteral` from the
        // `csharp_count_unary_conditions` arm leaves every other test
        // in the suite passing, so this is the only test guarding
        // that helper's literal-operand path.
        check_metrics::<CsharpParser>(
            "class A {
                void M(bool x) {
                    if (x && true) { System.Console.WriteLine(\"a\"); }
                    if (false || x) { System.Console.WriteLine(\"b\"); }
                }
            }",
            "foo.cs",
            |metric| {
                // `&&` and `||` themselves are NOT in
                // `csharp_count_token_condition`'s match list — they
                // route through `csharp_walk_for_conditions::AMPAMP|
                // PIPEPIPE`, which calls
                // `csharp_count_unary_conditions` on the parent
                // BinaryExpression. Each invocation counts every
                // child that matches the terminal-operand kinds and
                // whose parent is a BinaryExpression. For
                // `x && true`: 1 (Identifier x) + 1 (BooleanLiteral
                // true) = 2. For `false || x`: 1 (BooleanLiteral
                // false) + 1 (Identifier x) = 2. Total 4. Without
                // the BooleanLiteral arm only the two Identifier
                // counts would land, giving 2.
                assert_eq!(metric.abc.conditions_sum(), 4);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.assignments_sum(), 0);
            },
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
                    @r#"
                {
                  "assignments": 0,
                  "branches": 0,
                  "conditions": 1,
                  "magnitude": 1.0,
                  "value": 0.0,
                  "assignments_average": 0.0,
                  "branches_average": 0.0,
                  "conditions_average": 0.3333333333333333,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 0,
                  "branches_max": 0,
                  "conditions_min": 0,
                  "conditions_max": 1
                }
                "#
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
                    @r#"
                {
                  "assignments": 0,
                  "branches": 1,
                  "conditions": 1,
                  "magnitude": 1.4142135623730951,
                  "value": 0.0,
                  "assignments_average": 0.0,
                  "branches_average": 0.25,
                  "conditions_average": 0.25,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 0,
                  "branches_max": 1,
                  "conditions_min": 0,
                  "conditions_max": 1
                }
                "#
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
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 0);
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
                    @r#"
                {
                  "assignments": 0,
                  "branches": 0,
                  "conditions": 0,
                  "magnitude": 0.0,
                  "value": 0.0,
                  "assignments_average": 0.0,
                  "branches_average": 0.0,
                  "conditions_average": 0.0,
                  "assignments_min": 0,
                  "assignments_max": 0,
                  "branches_min": 0,
                  "branches_max": 0,
                  "conditions_min": 0,
                  "conditions_max": 0
                }
                "#
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
            assert_eq!(metric.abc.assignments(), 0);
            assert_eq!(metric.abc.branches(), 0);
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
                    0,
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

    // #1275: tree-sitter-c-sharp spells `int?`, `where T : class?`, the
    // ternary and `a?.b` with one and the same bare `?` token, so the
    // unguarded `QMARK` arm scored the two type-syntax forms as
    // decisions.
    //
    // The fixture carries two `nullable_type` `?`, one
    // `type_parameter_constraint` `?`, and one real ternary over a real
    // `>` comparison, so every way of getting the gate wrong lands on
    // its own number: 5 pre-fix, 3 if only `NullableType` is denied, 4
    // if only `TypeParameterConstraint` is, 1 if the gate swallows the
    // genuine ternary, 0 if the fixture stops parsing. Asserting 0 on a
    // nullable-only body would have been vacuous — an unparsable file
    // scores 0 too.
    #[test]
    fn csharp_nullable_type_syntax_is_not_a_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                int M<T>(int? p, int a, int b) where T : class? {
                    int? q = null;
                    return a > b ? 1 : 2;
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // The polarity pin for #1275's C# half. `csharp_count_token_condition`
    // denies the two type-syntax parents rather than allowing the two
    // decision-bearing ones, so that `a?.b` and `a?[0]` keep counting by
    // construction: both spell their operator as the same bare `?` under
    // `conditional_access_expression`, and C# cyclomatic counts that node
    // (`safe_navigation_chain_parity` pins C# at one decision per
    // operator alongside every other safe-navigation language).
    //
    // This test is what stops a later "make all three languages
    // consistent" pass from flipping C# to a `ConditionalExpression`
    // allowlist: that would silently drop both counts here to 0 while
    // every other ABC test still passed. `??` is deliberately absent
    // from the expected total — C# ABC does not list `QMARKQMARK` as a
    // condition (it does in the TS family), which is pre-existing and
    // out of scope for #1275.
    #[test]
    fn csharp_conditional_access_still_counts_as_a_condition() {
        check_metrics::<CsharpParser>(
            "class A {
                object M(string s, int[] xs) {
                    return s?.Length ?? xs?[0];
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // The other direction of #1275's C# gate, isolated: narrowing
    // `QMARK` must not swallow a real ternary. The fixture carries no
    // comparison operator at all, so the ternary is the only thing that
    // can put the count above the walker's own contribution — in the
    // test above, a surviving `>` would have masked an over-suppressed
    // `?` at 1 instead of 0 (grammar-dispatch §11). A nullable
    // parameter keeps the denied production live in the same parse, so
    // one fixture proves both directions: 3 pre-fix, 2 after, 1 if the
    // allowlist/denylist is aimed at `ConditionalExpression`.
    //
    // The 2 is the `?` token plus the ternary's condition slot, which
    // `csharp_walk_for_conditions` counts as a Fitzpatrick unary
    // condition. That is why this is not written as an
    // `assert_deepest_conditions_match_cyclomatic` parity pin the way
    // the Java `<` / `>` sibling is: C# cyclomatic scores this method 1
    // decision, and the divergence is the pre-existing unary-condition
    // rule, not the `?` gate.
    #[test]
    fn csharp_ternary_still_counts_alongside_nullable_types() {
        check_metrics::<CsharpParser>(
            "class A {
                int M(int? n, bool c) {
                    return c ? 1 : 2;
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
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
                assert_eq!(metric.abc.branches_max(), 3);
                assert_eq!(metric.abc.conditions_max(), 0);
            },
        );
    }

    #[test]
    fn php_zero_abc() {
        check_metrics::<PhpParser>("<?php\n", "foo.php", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
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

    #[test]
    fn php_if_boolean_literal_condition() {
        check_metrics::<PhpParser>(
            "<?php\n\
             function f() {\n\
             \x20   if (true) {}                 // +1c\n\
             \x20   if (!false) {}               // +1c\n\
             \x20   while (true) {}              // +1c\n\
             \x20   do {} while (false);         // +1c\n\
             }\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn php_methods_arguments_with_conditions() {
        check_metrics::<PhpParser>(
            "<?php\n\
             function f($a, $b) {\n\
             \x20   m($a, $b);                   // +1b\n\
             \x20   m(!$a, !$b);                 // +1b +2c\n\
             }\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn php_return_with_conditions() {
        check_metrics::<PhpParser>(
            "<?php\n\
             function m1($z) { return !($z >= 0); }\n\
             function m2($x) { return (((!$x))); }\n\
             function m3($x, $y) { return $x && $y; }\n",
            "foo.php",
            |metric| {
                // m1: `>=` (1). m2: walker unwraps to $x (1).
                // m3: `&&` walker counts both (2). Sum: 4.
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn php_name2_hidden_rule_drift_marker() {
        // Drift marker (findings.md round-2 #3): `Php::Name2` maps
        // to the hidden grammar rule `_name`. At the pinned
        // tree-sitter-php version it is never emitted as a concrete
        // node — the visible `Name` (= 1) carries every name.
        // We list `Name2` defensively in `php_bool_terminal_kinds!()`
        // (lesson 34); if a future grammar bump promotes `_name`
        // to a visible rule, this assertion fails loudly.
        let src = "<?php\nfunction f($x) { if ($x) { foo($x); } }\n";
        let parser = PhpParser::new(
            src.as_bytes().to_vec(),
            &std::path::PathBuf::from("foo.php"),
            None,
        );
        assert!(!ast_has_kind_id(&parser, Php::Name2 as u16));
    }

    #[test]
    fn php_scoped_property_access_condition_counts() {
        // Regression for findings.md round-2 #1 (PHP):
        // `if (Config::$enabled) {}` parses with
        // `scoped_property_access_expression` as the condition
        // node (kind_id 333 at the pinned grammar version — the
        // `*2` alias). Pre-fix, neither `ScopedPropertyAccessExpression`
        // nor its alias was in `php_bool_terminal_kinds!()`. The
        // walker reached the access node, found it non-terminal,
        // and broke. Mirrors C#'s `MemberAccessExpression` rule
        // (lesson 19, #372).
        check_metrics::<PhpParser>(
            "<?php\n\
             class Config { public static $enabled = true; }\n\
             function f() { if (Config::$enabled) { } }\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn php_named_argument_unary_conditional_counts() {
        // Regression for the code-review finding: PHP 8 named-argument
        // syntax `m(name: !$a)` parses as `argument(name, ':',
        // unary_op_expression)`. Pre-fix, the count walker took
        // `argument.child(0)` (the name) and missed the value at the
        // last child. Now it picks the last named child as the value.
        check_metrics::<PhpParser>(
            "<?php\nfunction f($a) { m(name: !$a); }\n",
            "foo.php",
            |metric| {
                // 1 call (branch) + 1 unary-conditional named argument.
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn php_low_precedence_keyword_logical_ops_trigger_walker() {
        // Regression: pre-fix, `$a or $b` reported 0 conditions
        // because the dispatcher only handled `AMPAMP|PIPEPIPE`,
        // skipping the PHP-specific `and` / `or` / `xor` keyword
        // forms even though they parse under the same
        // `binary_expression` shape.
        check_metrics::<PhpParser>(
            "<?php\n\
             function f($a, $b) {\n\
             \x20   return $a or $b;\n\
             }\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn php_if_multiple_conditions() {
        check_metrics::<PhpParser>(
            "<?php\n\
             function f($a, $b, $c, $d) {\n\
             \x20   if ($a || $b || $c || $d) {}     // +4c\n\
             \x20   if ($a && $b && $c) {}           // +3c\n\
             \x20   if (!$a && !$b) {}               // +2c\n\
             }\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn php_while_and_do_while_conditions() {
        check_metrics::<PhpParser>(
            "<?php\n\
             function f($a, $b) {\n\
             \x20   while ($a || $b) {}              // +2c\n\
             \x20   do {} while ($a && !$b);         // +2c\n\
             }\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn php_short_circuit_with_boolean_literal_operand() {
        check_metrics::<PhpParser>(
            "<?php\nfunction f($a) { return $a && true; }\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Issue #1102, PHP half. See
    // `cpp_ternary_operand_slots_count_as_unary_conditions` for the
    // rule. PHP's ABC dispatcher has no `?`-token arm — the grammar
    // does emit the token, but the `conditional_expression` node is
    // what carries the tally's +1 — so the arm keeps that increment and
    // adds the operand slots.
    #[test]
    fn php_ternary_operand_slots_count_as_unary_conditions() {
        // ternary (1) + condition `$a` (1) + `!$b` (1) + `!$c` (1) = 4.
        check_metrics::<PhpParser>(
            "<?php\nfunction f() { $x = $a ? !$b : !$c; }\n",
            "foo.php",
            |metric| assert_eq!(metric.abc.conditions_sum(), 4),
        );
        // No-double-count pin: ternary (1) + `>` (1) = 2, unchanged by
        // the fix.
        check_metrics::<PhpParser>(
            "<?php\nfunction f() { $x = ($a > 0) ? $b : -$b; }\n",
            "foo.php",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
        // Nested (PHP 8 requires the inner ternary parenthesised): two
        // ternary nodes plus the two bare-variable conditions = 4.
        check_metrics::<PhpParser>(
            "<?php\nfunction f() { $x = $a ? ($b ? $c : $d) : $e; }\n",
            "foo.php",
            |metric| assert_eq!(metric.abc.conditions_sum(), 4),
        );
        // A negated condition is the only input reaching the walker's
        // `else` fallback — see the C++ sibling for why. ternary (1) +
        // `!$a` (1) = 2.
        check_metrics::<PhpParser>(
            "<?php\nfunction f() { $x = !$a ? $b : $c; }\n",
            "foo.php",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    // PHP's short ternary `$a ?: $b` elides the consequence, which the
    // grammar names `body` (not `consequence`) and marks optional. The
    // alternative lands at child(3), so addressing the slot by field
    // name rather than a fixed child(4) is what keeps `!$b` counted.
    #[test]
    fn php_elided_ternary_body_still_walks_the_alternative() {
        // ternary (1) + condition `$a` (1) + `!$b` (1) = 3.
        check_metrics::<PhpParser>(
            "<?php\nfunction f() { $x = $a ?: !$b; }\n",
            "foo.php",
            |metric| assert_eq!(metric.abc.conditions_sum(), 3),
        );
    }

    // Issue #1276, PHP half. The `for` header's condition slot was the
    // one condition slot `PhpCode::compute` never dispatched, so a
    // bare / negated / parenthesised loop condition scored zero while
    // the identical predicate in an `if` header scored one. Each
    // fixture below is a shape only the new arm can classify — a
    // comparison-shaped condition proves nothing here, because the `<`
    // token arm counts it either way (grammar-dispatch §11).
    #[test]
    fn php_for_condition_slot_counts_unary_conditions() {
        // Bare variable: the whole condition, no operator token.
        check_metrics::<PhpParser>(
            "<?php\nfunction f($a) { for (; $a; ) {} }\n",
            "foo.php",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
        );
        // Negation: reaches the terminal through
        // `php_inspect_container`'s `!` unwrap.
        check_metrics::<PhpParser>(
            "<?php\nfunction f($a) { for (; !$a; ) {} }\n",
            "foo.php",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
        );
        // Parentheses: counts only because the `for_statement` parent
        // seeds `has_boolean_content`, the seed #1276 found dead.
        check_metrics::<PhpParser>(
            "<?php\nfunction f($a) { for (; ($a); ) {} }\n",
            "foo.php",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
        );
        // No-double-count pin: the `<` token arm already counted this
        // shape before the fix, and the walker must not add a second.
        // Two assignments (`$i = 0`, `$i++`) confirm the header parsed
        // as the three-clause form rather than degenerating.
        check_metrics::<PhpParser>(
            "<?php\nfunction f($n) { for ($i = 0; $i < $n; $i++) {} }\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
        // Empty condition: no `condition` field, no decision, zero.
        check_metrics::<PhpParser>(
            "<?php\nfunction f() { for (;;) { break; } }\n",
            "foo.php",
            |metric| assert_eq!(metric.abc.conditions_sum(), 0),
        );
    }

    // --- Kotlin ABC tests -------------------------------------------------

    #[test]
    fn kotlin_empty_class() {
        check_metrics::<KotlinParser>("class C {}", "foo.kt", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_val_then_assignments_count() {
        // Regression for #455: a `val` initialiser must not suppress the
        // standalone `=` assignments that follow it. tree-sitter-kotlin
        // emits no `SEMI` token (even for explicit semicolons), so the
        // pre-#455 `SEMI`-cleared declaration stack never cleared and the
        // immutable-`val` sentinel leaked, reporting A=0 here.
        check_metrics::<KotlinParser>(
            "fun f() {
                val cfg = 0
                a = 1
                b = 2
            }",
            "foo.kt",
            |metric| {
                // val initialiser suppressed; `a = 1` and `b = 2` count.
                assert_eq!(metric.abc.assignments_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn kotlin_var_then_assignments_count() {
        // Companion to the #455 regression: a `var` declaration leaves a
        // mutable-binding sentinel that *permits* the `=` — this path
        // accidentally masked the leak (its `Var` sentinel never suppressed
        // anything), so it must keep counting both the initialiser and the
        // following standalone assignments.
        check_metrics::<KotlinParser>(
            "fun f() {
                var cfg = 0
                a = 1
                b = 2
            }",
            "foo.kt",
            |metric| {
                // var initialiser (+1) plus `a = 1` and `b = 2` (+2).
                assert_eq!(metric.abc.assignments_sum(), 3);
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
                assert_eq!(metric.abc.assignments_sum(), 6);
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
                assert_eq!(metric.abc.branches_sum(), 3);
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
                assert_eq!(metric.abc.branches_sum(), 1);
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
                // Six comparison operators in the `val` initialisers
                // (<, >, <=, >=, ==, !=) → 6, plus the six bare-identifier
                // operands of the `r1 || … || r6` return chain, each a
                // Fitzpatrick Rule 9 unary condition (issue #557) → 6.
                // Total 12. Before the Kotlin walker was wired the chain
                // operands were silently dropped and this read 6.
                assert_eq!(metric.abc.conditions_sum(), 12);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                // Non-`else` WhenEntry arms count; the `else ->` fallback
                // arm does not (issue #456). Two case arms + zero for the
                // `else` arm = 2.
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Pins the `else ->` exclusion directly: a `when` whose only fallback
    // is `else ->` must not count that arm. Revert-verified — gating the
    // `WhenEntry` arm on `!kotlin_when_entry_is_else` is what drops this
    // from 3 to 2 (issue #456, lesson 11). Mirrors the cyclomatic gate.
    #[test]
    fn kotlin_when_else_not_a_condition() {
        check_metrics::<KotlinParser>(
            "fun m(x: Int): Int {
                return when (x) { 1 -> 10; 2 -> 20; else -> 0 }
            }",
            "foo.kt",
            |metric| {
                // case `1 ->` (+1) + case `2 ->` (+1) + `else ->` (+0) = 2.
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                // `try` (+1) and `catch` (+1) each contribute one condition,
                // matching Java / C# / C++ / Groovy (Fitzpatrick counts both
                // keywords). Before #696 Kotlin counted only the catch block.
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 2);
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 2);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 1);
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
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 3);
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
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
            assert_eq!(metric.abc.assignments_sum(), 0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn kotlin_unary_conditions_in_chain() {
        // Fitzpatrick Rule 9 (issue #557): each bare boolean operand of a
        // `&&` / `||` chain is one condition. `a && b || c` → a, b, c each
        // contribute one; the `&&` / `||` operators contribute nothing.
        // expected: 3 unary conditions, no comparisons, no `if`-keyword
        // condition in Kotlin (matches the Java byte-equivalent of 3).
        check_metrics::<KotlinParser>(
            "fun f(a: Boolean, b: Boolean, c: Boolean) {
                if (a && b || c) { println(\"x\") }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3);
            },
        );
    }

    #[test]
    fn kotlin_comparison_operands_add_nothing() {
        // Isolation check: comparison operands of a `&&` chain are nested
        // `binary_expression` nodes, not bare boolean leaves, so the
        // walker adds nothing — only the two `>` comparisons count.
        // expected: 2 (the two `>` tokens), walker contributes 0.
        check_metrics::<KotlinParser>(
            "fun g(x: Int, y: Int) {
                if (x > 0 && y > 0) { println(\"x\") }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    #[test]
    fn kotlin_negated_operand_is_unary_condition() {
        // A `!`-negated operand is still a unary condition: `a && !b`
        // unwraps the `unary_expression` to reach the inner identifier.
        // expected: 2 (`a` and the `!b` operand).
        check_metrics::<KotlinParser>(
            "fun f(a: Boolean, b: Boolean) {
                if (a && !b) { println(\"x\") }
            }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    #[test]
    fn kotlin_bare_if_predicate_is_one_condition() {
        // Issue #773: a bare-boolean `if` predicate (`if (flag)`) is one
        // Fitzpatrick unary condition. Before the Phase-2B arm it counted
        // 0, so `if (flag) 1 else -1` scored 1 (only the `else`) instead of
        // 2, dropping below Kotlin's own cyclomatic decision count.
        // expected: predicate (1) + else (1) = 2.
        check_metrics::<KotlinParser>(
            "fun m(flag: Boolean): Int { return if (flag) 1 else -1 }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    #[test]
    fn kotlin_bare_while_predicate_is_one_condition() {
        // Issue #773: the bare predicate of a `while` loop counts one
        // condition via the `condition` field. expected: 1.
        check_metrics::<KotlinParser>(
            "fun m(running: Boolean) { while (running) { println(\"x\") } }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
            },
        );
    }

    #[test]
    fn kotlin_bare_do_while_predicate_is_one_condition() {
        // Issue #773: the bare predicate of a `do`/`while` loop counts one
        // condition. expected: 1.
        check_metrics::<KotlinParser>(
            "fun m(ok: Boolean) { do { println(\"x\") } while (ok) }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
            },
        );
    }

    #[test]
    fn kotlin_comparison_predicate_not_double_counted() {
        // Double-count guard (#773): a comparison predicate (`if (a == b)`)
        // is a nested `binary_expression` already counted by the `==` token
        // arm, so the Phase-2B condition-slot arm must add nothing here.
        // expected: `==` (1) + else (1) = 2 — unchanged by the new arm.
        check_metrics::<KotlinParser>(
            "fun m(a: Int, b: Int): Int { return if (a == b) 1 else -1 }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    #[test]
    fn kotlin_short_circuit_predicate_not_double_counted() {
        // Double-count guard (#773): an `&&`/`||` predicate is counted by
        // the Rule 9 chain walker (each operand once); the Phase-2B arm
        // must add nothing for it. expected: `x` (1) + `y` (1) + else (1)
        // = 3 — unchanged by the new arm.
        check_metrics::<KotlinParser>(
            "fun m(x: Boolean, y: Boolean): Int { return if (x && y) 1 else -1 }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3);
            },
        );
    }

    #[test]
    fn kotlin_parenthesised_bare_predicate_is_one_condition() {
        // A parenthesised bare predicate (`if ((flag))`) is unwrapped by
        // `kotlin_inspect_container` and still counts one condition (#773).
        // expected: 1.
        check_metrics::<KotlinParser>(
            "fun m(flag: Boolean) { if ((flag)) { println(\"x\") } }",
            "foo.kt",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
            },
        );
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
                    let x = 0;          // +1 — only a `const` initializer is suppressed
                    x = 1;              // +1
                    x += 2;             // +1
                    x++;                // +1
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_const_excluded_from_assignments() {
        check_metrics::<TypescriptParser>(
            "class C {
                m(): void {
                    const a = 1;        // suppressed (`const` initializer)
                    const b = 2;        // suppressed
                    let c = 3;          // +1 — `let` initializers count
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Regression cluster for #1277. The pre-fix implementation decided
    // "is this `=` a `const` initializer?" from a sentinel stack cleared
    // only on a `SEMI` token, so the answer for one statement depended on
    // the *previous* statement's terminator. Automatic semicolon
    // insertion makes that terminator optional in all four JS-family
    // languages. The replacement is structural — see
    // `impl_js_family_const_binding!` in `src/metrics/abc/js_family.rs`.

    #[test]
    fn typescript_asi_const_does_not_suppress_later_assignments() {
        check_metrics::<TypescriptParser>(
            "function f() {
                const a = 1
                x = 2
                y = 3
            }",
            "foo.ts",
            |metric| {
                // `const a = 1` suppressed; `x = 2` and `y = 3` count.
                // Pre-#1277 this reported 0: the unterminated `const`
                // never popped its sentinel.
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn typescript_semicolon_const_does_not_suppress_later_assignments() {
        // The semicolon-terminated spelling of the fixture above, which
        // must score the same as it. Pre-#1277 they scored 2 and 0 — the
        // pair is what pins the terminator out of the answer.
        check_metrics::<TypescriptParser>(
            "function f() {
                const a = 1;
                x = 2;
                y = 3;
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn typescript_as_const_does_not_suppress_later_assignments() {
        // The sentinel stack was also reachable from the other side: the
        // `const` token of a TypeScript `x as const` assertion promoted a
        // live `let` slot to `Const` and suppressed every `=` until the
        // next `;`. Structurally that `const` is a child of an
        // `as_expression`, not a declaration keyword.
        check_metrics::<TypescriptParser>(
            "function f(x: number) {
                let y = x as const
                w = 3
            }",
            "foo.ts",
            |metric| {
                // `let` initializer (+1) and `w = 3` (+1); pre-#1277: 1.
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn typescript_const_declarator_shapes_stay_suppressed() {
        // Shapes the sentinel stack handled implicitly, which the
        // structural predicate must reproduce: *every* declarator under a
        // `const`-bearing `lexical_declaration` is suppressed, including
        // the second element of a multi-declarator list, a destructuring
        // pattern, and the defaults inside one — `c = 5`, the nested
        // `e = 6` and its `= {}`, `g = 7`, and the rest element's `h = 8`,
        // each one more pattern layer the predicate's climb has to cross
        // — while `for (const x of xs)` carries no `=` at all. Only
        // `let i = 3` and `var j = 4` count — the deliberate deviation
        // documented on `js_abc_compute!`. Gating on
        // `variable_declaration` instead of `lexical_declaration`,
        // dropping the `const` check, or stopping the climb at the
        // declarator's own `=`, each flips one of these rows. The
        // `for`-of row is the exception: it carries no `=`, so no change
        // to the predicate can move it. It pins the grammar shape instead
        // — a future grammar emitting a `variable_declarator` there would
        // start suppressing something that never counted.
        check_metrics::<TypescriptParser>(
            "function f(o: any, xs: number[]) {
                const a = 1, b = 2
                const {c = 5, d: {e = 6} = {}} = o
                const [g = 7, ...[h = 8]] = xs
                let i = 3
                var j = 4
                for (const x of xs) { k(x) }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn typescript_const_initializer_value_assignments_still_count() {
        // TypeScript half of
        // `javascript_const_initializer_value_assignments_still_count`.
        check_metrics::<TypescriptParser>(
            "function m(o: any, a: any, b: any) { const x = (o.p = 1); const y = a || (b = 2); }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2);
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
                assert_eq!(metric.abc.branches_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 8);
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
                assert_eq!(metric.abc.conditions_sum(), 4);
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
                        default:                // +0 (fallthrough, #469)
                            return 0;
                    }
                }
            }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
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
                assert_eq!(metric.abc.conditions_sum(), 0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // #1275, TypeScript half. Eleven grammar productions emit a bare
    // `?` and only `ternary_expression` is a decision; the other ten are
    // type syntax. This fixture exercises five of them — `optional_
    // parameter`, `property_signature`, `method_signature`,
    // `abstract_method_signature` and `public_field_definition`, plus an
    // `optional_type` in a tuple — against one real ternary over one
    // real `>`.
    //
    // Six type-syntax `?` means the numbers separate cleanly: 8 pre-fix,
    // 2 once the allowlist is aimed at `TernaryExpression`, 1 if it is
    // aimed at anything else (the ternary stops counting too), 0 if the
    // fixture stops parsing. Any partial gate — one that named some of
    // the type-syntax parents in a denylist instead — lands between 3
    // and 7 and is equally visible.
    #[test]
    fn typescript_optional_type_syntax_is_not_a_condition() {
        check_metrics::<TypescriptParser>(
            "interface I { a?: string; m?(x: number): void; }
            abstract class K { f?: number; abstract g?(): void; }
            type Tup = [number, string?];
            function h(x?: number, y: number = 0): number { return y > 1 ? 1 : 2; }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // The explicit half of #1275's TypeScript decision: a conditional
    // type (`T extends U ? X : Y`) is resolved by the type checker and
    // erased before runtime, so its `?` is not a condition. That falls
    // out of the `TernaryExpression` allowlist rather than being named,
    // which is exactly why it needs its own test — the choice reads as
    // an omission otherwise, and nothing else here would notice if a
    // later edit added `ConditionalType` to the allowlist "for
    // symmetry".
    //
    // The real ternary below keeps the expectation off zero: 3 pre-fix,
    // 2 with the conditional type excluded, 1 if the ternary is
    // swallowed too.
    #[test]
    fn typescript_conditional_type_is_not_a_condition() {
        check_metrics::<TypescriptParser>(
            "type Cond<T> = T extends string ? number : boolean;
            function pick(a: number, b: number): number { return a > b ? 1 : 2; }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // grammar-dispatch §2. `Typescript::QMARK2` is the enum entry for
    // the external scanner's `_ternary_qmark` token, and
    // tree-sitter-typescript's public symbol map folds it back onto
    // `anon_sym_QMARK`, so `kind_id()` never reports it — every ternary
    // `?` in the fixture below arrives as plain `QMARK`. Listing
    // `QMARK2` in the gated arm would therefore be dead code, and
    // leaving it out is safe only for as long as that mapping holds.
    // This pins both halves so a grammar bump that starts exposing the
    // alias fails here rather than silently zeroing every TypeScript
    // ternary.
    //
    // The fixture's only `?` is the ternary's, deliberately. An optional
    // parameter (`b?: number`) would emit a `QMARK` of its own and
    // satisfy the positive assertion on its own, leaving it true no
    // matter what id the ternary's `?` came back as — decoration rather
    // than the non-vacuity guard it is here for.
    #[test]
    fn typescript_ternary_qmark_alias_stays_unreachable() {
        let parser = TypescriptParser::new(
            "function f(a: boolean): number { return a ? 1 : 2; }\n"
                .as_bytes()
                .to_vec(),
            std::path::Path::new("foo.ts"),
            None,
        );
        assert!(ast_has_kind_id(&parser, Typescript::QMARK as u16));
        assert!(!ast_has_kind_id(&parser, Typescript::QMARK2 as u16));
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
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 1);
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
                assert_eq!(metric.abc.assignments_sum(), 2);
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
                assert_eq!(metric.abc.assignments_sum(), 4);
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
                assert_eq!(metric.abc.assignments_sum(), 1);
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
                assert_eq!(metric.abc.branches_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 4);
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
                        case 1: return 1;       // +1
                        case 2: return 2;       // +1
                        default: return 0;      // +0 (fallthrough, #469)
                    }
                }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
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
                assert_eq!(metric.abc.conditions_sum(), 0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // #1275 in the second expansion of `ts_abc_compute!`. TSX shares
    // TypeScript's `?` productions and its own `TernaryExpression` /
    // `QMARK` ids, so the gate is a distinct instantiation and needs its
    // own fixture — a passing TypeScript test says nothing about the
    // macro's other expansion. Four type-syntax `?` plus one `>` and one
    // ternary: 6 pre-fix, 2 after, 1 if the allowlist is misaimed.
    #[test]
    fn tsx_optional_type_syntax_is_not_a_condition() {
        check_metrics::<TsxParser>(
            "interface I { a?: string; m?(x: number): void; }
            class K { f?: number; }
            function h(x?: number, y: number = 0): number { return y > 1 ? 1 : 2; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // The TSX half of `typescript_conditional_type_is_not_a_condition`.
    // `ts_abc_compute!` expands twice and the two expansions are
    // independent code (grammar-dispatch §11): adding `ConditionalType`
    // to the allowlist "for symmetry" fails only the TypeScript test
    // without this one, which reads as the TSX expansion being fine
    // rather than untested. `conditional_type` is in the tsx grammar's
    // `?` set exactly as it is in typescript's.
    #[test]
    fn tsx_conditional_type_is_not_a_condition() {
        check_metrics::<TsxParser>(
            "type Cond<T> = T extends string ? number : boolean;
            function pick(a: number, b: number): number { return a > b ? 1 : 2; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // The TSX half of `typescript_ternary_qmark_alias_stays_unreachable`
    // — the tsx grammar declares the same `_ternary_qmark` external and
    // maps it back onto `anon_sym_QMARK` at its own id. Same
    // single-`?` fixture rule; see that test for why.
    #[test]
    fn tsx_ternary_qmark_alias_stays_unreachable() {
        let parser = TsxParser::new(
            "function f(a: boolean): number { return a ? 1 : 2; }\n"
                .as_bytes()
                .to_vec(),
            std::path::Path::new("foo.tsx"),
            None,
        );
        assert!(ast_has_kind_id(&parser, Tsx::QMARK as u16));
        assert!(!ast_has_kind_id(&parser, Tsx::QMARK2 as u16));
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
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_asi_const_does_not_suppress_later_assignments() {
        // TSX half of the #1277 cluster; see
        // `typescript_asi_const_does_not_suppress_later_assignments`.
        // TSX has its own `Const` / `VariableDeclarator` /
        // `LexicalDeclaration` kind ids, so the predicate is generated
        // separately and needs its own fixture.
        check_metrics::<TsxParser>(
            "function f() {
                const a = 1
                x = 2
                y = 3
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn tsx_const_declarator_shapes_stay_suppressed() {
        // TSX half of `typescript_const_declarator_shapes_stay_suppressed`.
        check_metrics::<TsxParser>(
            "function f(o: any, xs: number[]) {
                const a = 1, b = 2
                const {c = 5, d: {e = 6} = {}} = o
                const [g = 7, ...[h = 8]] = xs
                let i = 3
                var j = 4
                for (const x of xs) { k(x) }
            }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2);
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
                assert_eq!(metric.abc.assignments_sum(), 2);
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
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn ruby_simple_assignment() {
        check_metrics::<RubyParser>("def f\n  a = 1\n  b = 2\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 2);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_logical_augmented_assignment() {
        // `||=` and `&&=` are also `operator_assignment` nodes.
        check_metrics::<RubyParser>("def f\n  @x ||= 0\n  @x &&= 1\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 2);
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
                assert_eq!(metric.abc.branches_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_super_and_yield_branches() {
        // `super` and `yield` both count as branches (control-pass).
        check_metrics::<RubyParser>("def f\n  super\n  yield\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.branches_sum(), 2);
            assert_eq!(metric.abc.assignments_sum(), 0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn ruby_attr_macro_is_branch() {
        // `attr_accessor` is a `Call3` node and registers as a branch
        // like any method invocation.
        check_metrics::<RubyParser>("class A\n  attr_accessor :x\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.branches_sum(), 1);
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
                assert_eq!(metric.abc.conditions_sum(), 6);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_superclass_clause_is_not_a_condition() {
        // Regression for #1280: a superclass clause spells its `<` with the
        // same `LT` token as a comparison, but parents it under
        // `superclass` rather than `binary`, so the parent gate excludes
        // it. Before the gate every subclass declaration scored a phantom
        // condition.
        // expected: 0 conditions — the file contains no conditional at all;
        // the single assignment is `x = 1`.
        check_metrics::<RubyParser>(
            "class Foo < Bar\n  def plain\n    x = 1\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0);
                assert_eq!(metric.abc.assignments_sum(), 1);
            },
        );
    }

    #[test]
    fn ruby_operator_method_name_is_not_a_condition() {
        // The `<` naming an operator method parents under `operator`, which
        // the `binary` gate likewise excludes (#1280). The body carries a
        // real comparison so the expected value is not the all-zero default:
        // 1 discriminates "only the name token is excluded" from both "the
        // gate excludes everything" (0) and "nothing is gated" (2).
        // expected: 1 condition — the `@v < other` comparison, not the `def <`.
        check_metrics::<RubyParser>("def <(other)\n  @v < other\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 1);
        });
    }

    #[test]
    fn ruby_every_comparison_operator_method_name_is_not_a_condition() {
        // The sibling half of #1280. `<` is not special: every comparison
        // and equality token Ruby lets you `def` parents under `operator`
        // in that position, so gating only `LT` / `GT` left `def ==`,
        // `def <=`, `def >=`, `def <=>`, `def !=` and `def =~` each
        // scoring a phantom condition — measured at 1 apiece for a body
        // containing no conditional at all.
        // expected: 0 conditions per definition; only the name token is on
        // the line, so a single non-zero total localises the regression.
        check_metrics::<RubyParser>(
            "def ==(o)\n  1\nend\n\
             def !=(o)\n  1\nend\n\
             def <=(o)\n  1\nend\n\
             def >=(o)\n  1\nend\n\
             def <=>(o)\n  1\nend\n\
             def =~(o)\n  1\nend\n\
             def <(o)\n  1\nend\n\
             def >(o)\n  1\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0);
            },
        );
        // The positive control: the same tokens inside a `binary` are real
        // comparisons and still count, so the gate is not blanket
        // suppression.
        check_metrics::<RubyParser>(
            "def cmp(a, b)\n  a == b || a <= b || a <=> b\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3);
            },
        );
    }

    #[test]
    fn ruby_case_match_in_arms_are_conditions() {
        // Regression for #977: each non-wildcard `case … in` arm is one
        // ABC condition, matching Python's `case_clause` handling. Using
        // literal patterns (no comparison operators) isolates the
        // `in_clause` contribution from any operand tokens.
        // expected: 2 conditions — one per `in 1` / `in 2` arm.
        check_metrics::<RubyParser>(
            "def f(x)\n  case x\n  in 1 then :one\n  in 2 then :two\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    #[test]
    fn ruby_case_match_guarded_wildcard_is_a_condition() {
        // Regression for #977: a guarded wildcard arm `in _ if x` is not a
        // bare default and counts as one ABC condition, while the trailing
        // bare `in _` adds none. The guard predicate here is a bare
        // identifier (no comparison operator), so the single counted
        // condition is the guarded `in_clause` itself.
        // expected: 1 condition — the guarded `in _ if x` arm only.
        check_metrics::<RubyParser>(
            "def f(x)\n  case x\n  in _ if x then :y\n  in _ then :default\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
            },
        );
    }

    #[test]
    fn ruby_case_match_bare_wildcard_is_not_a_condition() {
        // Regression for #977: a `case … in` whose only arm is the bare
        // wildcard `in _` (no guard) is the default arm and contributes no
        // ABC condition, keeping ABC and cyclomatic in lockstep on the
        // same construct.
        // expected: 0 conditions.
        check_metrics::<RubyParser>(
            "def f(x)\n  case x\n  in _ then :default\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 0);
            },
        );
    }

    #[test]
    fn ruby_bare_predicate_control_flow_counts_one_condition() {
        // Regression for #696: idiomatic Ruby bare predicates
        // (`if flag` / `while flag` / `unless flag` / `until flag`) each
        // count one unary condition, matching Rust / C# / PHP / Python. The
        // condition field is read for both block and modifier forms.
        //
        // expected: 8 conditions — four block forms (`if`/`unless`/`while`/
        // `until`) plus the same four as modifiers, one each.
        check_metrics::<RubyParser>(
            "def f(flag)\n  if flag\n    a\n  end\n  unless flag\n    b\n  end\n  while flag\n    c\n  end\n  until flag\n    d\n  end\n  a if flag\n  b unless flag\n  c while flag\n  d until flag\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 8);
            },
        );
    }

    #[test]
    fn ruby_bare_predicate_does_not_double_count_comparison_or_chain() {
        // `if a == b` counts only the `==` comparison (the condition field
        // is a `binary` node, adding nothing). `if a && b` counts the two
        // chain operands via the `&&` walker, again with the condition-field
        // arm adding nothing — so neither shape is double-counted (#696).
        //
        // expected: 3 — `==` (1) + the `a`,`b` operands of `&&` (2).
        check_metrics::<RubyParser>(
            "def f(a, b)\n  if a == b\n    x\n  end\n  if a && b\n    y\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_ternary_condition() {
        // The `?` ternary marker is one condition; the inner `==` is
        // another.
        check_metrics::<RubyParser>("def f(x)\n  x == 0 ? :z : :nz\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    // Issue #1161. Ruby's ternary carried only the `?` token arm, so
    // `a ? !b : !c` scored 1 against the 4 that Java, C#, Groovy, the C
    // family, the JS family, PHP and Perl all report for the same
    // expression (#1102) — and `ruby_inspect_container`'s `Conditional`
    // boolean-context seed was unreachable for the same reason.
    //
    // Every expectation below is the value its C++ sibling
    // (`cpp_ternary_operand_slots_count_as_unary_conditions`) already
    // asserts for the same expression, so the two read as one table.
    #[test]
    fn ruby_ternary_operand_slots_count_as_unary_conditions() {
        // `?` (1) + condition `a` (1) + `!b` (1) + `!c` (1) = 4.
        check_metrics::<RubyParser>("def f\n  x = a ? !b : !c\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 4);
        });
        // No-double-count pin, and the assertion that catches the trap
        // this grammar sets: `-b` and `!b` are the SAME node kind
        // (`unary:284`), separated only by child(0). Routing the branch
        // slots through `ruby_inspect_container` — which tests for the
        // `!` token, not for the kind — is what keeps this at 2. An
        // implementation keying on `Unary` reads 3 here.
        // `?` (1) + `>` (1) = 2, unchanged by the fix: the parenthesised
        // condition unwraps to a `binary`, which is not a boolean
        // terminal, and neither branch is negated.
        check_metrics::<RubyParser>("def f\n  x = (a > 0) ? b : -b\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
        });
        // Nested — Ruby needs the inner ternary parenthesised. Outer `?`
        // (1) + outer condition `a` (1) + inner `?` (1) + inner
        // condition `b` (1) = 4. The outer consequence unwraps to the
        // inner `conditional`, which is neither a boolean terminal nor a
        // further paren / `!` layer, so it adds nothing on its own; the
        // inner ternary is reached by the walk, not by descent.
        check_metrics::<RubyParser>(
            "def f\n  x = a ? (b ? c : d) : e\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
            },
        );
        // A parenthesised condition, pinning the `is_parens` unwrap on
        // the condition slot: `(a)` is `parenthesized_statements`, not a
        // boolean terminal, so it reaches the walker's `else` fallback
        // and only `ruby_inspect_container` can resolve it.
        // `?` (1) + `(a)` (1) + `!b` (1) + `!c` (1) = 4; drop the
        // fallback and this reads 3 while every other case here holds.
        check_metrics::<RubyParser>("def f\n  x = (a) ? !b : !c\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 4);
        });
        // A negated condition takes the same fallback through the `!`
        // unwrap rather than the paren one. `?` (1) + `!a` (1) = 2.
        check_metrics::<RubyParser>("def f\n  x = !a ? b : c\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
        });
    }

    // The boolean-context seed must discriminate between the condition
    // slot and the two branch slots — not merely exist. None of the
    // fixtures above can tell the difference: their branches are either
    // `!`-unaries (which set the flag inside the unwrap loop regardless
    // of the seed) or kinds the loop breaks on before any terminal test.
    // A seed that returned `true` for every slot of a `Conditional`
    // leaves all five at their asserted values and fails nothing.
    //
    // A parenthesised *branch* is the input that separates them: the
    // unwrap reaches a bare terminal, so only the seed decides whether
    // it counts. `?` (1) + condition `a` (1) = 2 in both directions.
    #[test]
    fn ruby_ternary_branch_operands_are_not_double_counted() {
        check_metrics::<RubyParser>("def f\n  x = a ? (b) : c\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
        });
        check_metrics::<RubyParser>("def f\n  x = a ? b : (c)\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
        });
        // The same pair with a comment before the operand. Comments are
        // tree-sitter `extras`, so they become the branch's previous
        // sibling — which is why the seed asks the grammar which child
        // is the `condition` field rather than testing that sibling for
        // `?` / `:` as the C family does. Under the token form both of
        // these read 3.
        check_metrics::<RubyParser>(
            "def f\n  x = a ?\n    # note\n    (b) : c\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
        check_metrics::<RubyParser>(
            "def f\n  x = a ? b :\n    # note\n    (c)\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
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
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 4);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 1);
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
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 1);
                // `>`(1) + `==`(1) = 2 conditions. `if` is not a
                // token; `&&` is `AMPAMP` and is not counted (see
                // the module-level `Stats` doc-comment for the
                // cross-language policy; #395, walker tracked in
                // #403).
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn ruby_unary_conditions_in_chain() {
        // Fitzpatrick Rule 9 (issue #557): each bare boolean operand of a
        // `&&` / `||` chain is one condition. `a && b || c` → a, b, c each
        // contribute one. Ruby's `if` keyword is not a condition token.
        // expected: 3 unary conditions (matches the Java byte-equivalent).
        check_metrics::<RubyParser>(
            "def f(a, b, c)\n  if a && b || c\n    puts \"x\"\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3);
            },
        );
    }

    #[test]
    fn ruby_keyword_and_or_chain_counts_operands() {
        // The keyword forms `and` / `or` get the same Rule 9 treatment as
        // `&&` / `||`. expected: 3 unary conditions (a, b, c).
        check_metrics::<RubyParser>(
            "def f(a, b, c)\n  if a and b or c\n    puts \"x\"\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3);
            },
        );
    }

    #[test]
    fn ruby_negated_operand_is_unary_condition() {
        // A `!`-negated operand unwraps the `unary` node to the inner
        // identifier. expected: 2 (`a` and the `!b` operand).
        check_metrics::<RubyParser>(
            "def f(a, b)\n  if a && !b\n    puts \"x\"\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    #[test]
    fn ruby_comparison_operands_add_nothing() {
        // Isolation for Rule 9 (issue #557): when the `&&` operands are
        // themselves comparisons, the unary-condition walker must add
        // nothing on top of the two `>` comparisons already counted as
        // conditions — distinguishing the gap (bare boolean operands)
        // from ordinary relational conditions. Mirrors the Kotlin and
        // Elixir isolation tests. expected: 2 (the two `>` comparisons).
        check_metrics::<RubyParser>(
            "def f(x, y)\n  if x > 0 && y > 0\n    puts \"x\"\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
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
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_plain_assignments_count() {
        // Three plain `=` assignments → A=3. No branches, no conditions.
        check_metrics::<PythonParser>("x = 1\ny = 2\nz = x\n", "foo.py", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 3);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_typed_assignment_counts_bare_annotation_does_not() {
        // `x: int = 1` carries an `=`, so it counts.
        // `y: int` is a bare annotation (no `=`) — declares a type but
        // binds nothing; it must NOT inflate the assignment count.
        check_metrics::<PythonParser>("x: int = 1\ny: int\n", "foo.py", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 1);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_augmented_assignments_count() {
        // Each augmented op counts once.
        check_metrics::<PythonParser>("x = 0\nx += 1\nx -= 1\nx *= 2\n", "foo.py", |metric| {
            // 1 plain `=` + 3 augmented = 4 assignments.
            assert_eq!(metric.abc.assignments_sum(), 4);
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
            assert_eq!(metric.abc.assignments_sum(), 1);
            assert_eq!(metric.abc.conditions_sum(), 1);
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
                assert_eq!(metric.abc.branches_sum(), 3);
                assert_eq!(metric.abc.assignments_sum(), 0);
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
                assert_eq!(metric.abc.conditions_sum(), 3);
                // 3 plain assignments; the comparisons are operands.
                assert_eq!(metric.abc.assignments_sum(), 3);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_chained_comparison_counts_once() {
        // tree-sitter-python collapses `0 < x < 10` into a single
        // `ComparisonOperator` — one condition, not two.
        check_metrics::<PythonParser>("def f(x):\n    return 0 < x < 10\n", "foo.py", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 1);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_number_truthy_condition_counts() {
        // Regression for #772: Python treats every non-zero number as
        // truthy, so `if 5:` and `x and 5` should each count their
        // numeric literal as a Fitzpatrick unary condition. Pre-fix
        // `python_bool_terminal_kinds!()` listed `True` / `False` but
        // omitted `Integer` / `Float`, so the walker dropped every
        // numeric-truthy operand (mirrors the Lua `Number` fix).
        check_metrics::<PythonParser>(
            "def f(a):\n    if 5:\n        pass\n    return a and 2\n",
            "foo.py",
            |metric| {
                // `if 5:` → walker counts the Integer literal (+1).
                // `a and 2` → `and` walker counts both operands:
                //   identifier `a` (+1), Integer `2` (+1).
                // Total: 3.
                assert_eq!(metric.abc.conditions_sum(), 3);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_boolean_operators_not_counted_directly() {
        // Python's `and` / `or` are not counted as conditions on
        // their own (Fitzpatrick Rule 5; #395). Each operand is
        // instead counted as a unary conditional by the walker
        // (Rule 9; #403). `if a and b or c:` parses left-to-right
        // with `or` lower precedence: `(a and b) or c`. Walker
        // tallies: inner `and` counts `a`, `b` (+2); outer `or`
        // counts only the new outer operand `c` (+1; the inner
        // `(a and b)` BooleanOperator is not a terminal). Total
        // C = 3.
        check_metrics::<PythonParser>(
            "def f(a, b, c):\n    if a and b or c:\n        pass\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
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
            assert_eq!(metric.abc.conditions_sum(), 1);
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
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    /// `not x and y` parses as `BooleanOperator(NotOperator(x), and,
    /// y)`. The `and` itself is NOT counted (Fitzpatrick Rule 5
    /// lists only comparison operators); the `NotOperator` is
    /// counted at the top level (Rule 7); and the `y` operand is
    /// counted by the Rule 9 walker (issue #403). Total: 2.
    /// `NotOperator` is intentionally not walked-into a second
    /// time — the walker skips it to avoid double-counting.
    #[test]
    fn python_unary_not_with_boolean_combinator_counts_each() {
        check_metrics::<PythonParser>(
            "def f(x, y):\n    if not x and y:\n        return 1\n    return 0\n",
            "foo.py",
            |metric| {
                // NotOperator (1) + walker on `and` finds `y` (1) = 2.
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 4);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Issue #1161. Python counted the `conditional_expression` node but
    // never its condition slot, so `a if c() else b` reported 1 where
    // the equivalent `c() ? a : b` reports 2 everywhere else — and
    // `python_inspect_container`'s `ConditionalExpression` boolean-
    // context seed was unreachable, no call site having passed that
    // parent.
    #[test]
    fn python_ternary_condition_slot_counts_as_a_unary_condition() {
        // ternary (1) + condition `c()` (1) = 2. `c()` is a `Call`, a
        // boolean terminal; it also adds one *branch*, not a condition.
        check_metrics::<PythonParser>(
            "def f(a, b, c):\n    return a if c() else b\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
        // A parenthesised condition, pinning the seed line this fix made
        // reachable: `(c)` is a `parenthesized_expression`, so only
        // `python_inspect_container` can resolve it, and it counts the
        // unwrapped terminal only when the parent seeds boolean context.
        // ternary (1) + `(c)` (1) = 2.
        check_metrics::<PythonParser>(
            "def f(a, b, c):\n    return a if (c) else b\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
        // A negated condition is *not* counted by the new slot: it is a
        // `NotOperator`, which already has its own top-level dispatcher
        // arm. ternary (1) + `not c` (1) = 2, not 3.
        check_metrics::<PythonParser>(
            "def f(a, b, c):\n    return a if not c else b\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
        // The cross-language reference case. `(not b) if a else (not c)`
        // is the exact semantic equivalent of `a ? !b : !c`, which every
        // other language reports as 4: ternary (1) + condition `a` (1) +
        // two `NotOperator`s (2). The negated operands come from
        // Python's own arm, the condition from the slot added here.
        //
        // #1161's resolution plan predicted this would stay 3, having
        // measured the condition slot's contribution against the pre-fix
        // total. 3 would have left Python disagreeing with every other
        // language on the reference expression — the gap the issue was
        // filed about.
        check_metrics::<PythonParser>(
            "def f(a, b, c):\n    return (not b) if a else (not c)\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
            },
        );
    }

    // The double-count pin, and the reason Python gets a condition-slot
    // helper rather than a copy of `cpp_walk_ternary`: Python's branch
    // operands are counted by the top-level `NotOperator` /
    // `ComparisonOperator` arms, a different mechanism from every other
    // language's walker. Routing the branch slots through
    // `python_inspect_container` as the C family does would count a
    // parenthesised operand that the identical unparenthesised
    // expression scores at zero.
    //
    // Both fixtures below are 2 today and 4 under such a copy, so a
    // later "make Python consistent with the others" change cannot land
    // silently.
    #[test]
    fn python_ternary_branch_operands_are_not_double_counted() {
        // ternary (1) + condition `a` (1) = 2. The two parenthesised
        // operands add nothing — an unnegated branch is type-free.
        check_metrics::<PythonParser>(
            "def f(a, b, c):\n    return (b) if a else (c)\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
        // The unparenthesised form must agree: nothing about `(b)`
        // versus `b` is a condition.
        check_metrics::<PythonParser>(
            "def f(a, b, c):\n    return b if a else c\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    // Comments are tree-sitter `extras`, so they arrive as direct
    // children of `conditional_expression` and shift every positional
    // index after them. `python_count_ternary_condition` therefore
    // anchors on the `if` keyword and skips comments after it; both
    // halves are needed and each fixture below fails without one.
    #[test]
    fn python_ternary_condition_survives_an_interposed_comment() {
        // Comment before the keyword: `child(2)` is the `if` token here,
        // so a positional lookup reads 1. ternary (1) + `f()` (1) = 2.
        check_metrics::<PythonParser>(
            "def f(b, c):\n    return (b\n            # why\n            if f() else c)\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
        // Comment after the keyword: the child immediately following
        // `if` is the comment, so taking the first rather than the first
        // non-comment reads 1. ternary (1) + `f()` (1) = 2.
        check_metrics::<PythonParser>(
            "def f(b, c):\n    return (b if\n            # why\n            f() else c)\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
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
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_if_multiple_conditions() {
        // Fitzpatrick Rule 9 walker on `and` / `or` (issue #403).
        //   - `if a or b or c or d:` → 4 (each operand counted once)
        //   - `if a and b and c:`    → 3
        //   - `if not a and not b:`  → 2 (two `NotOperator`s counted
        //     by the top-level dispatcher arm; the walker SKIPS
        //     `NotOperator` children to avoid double-counting)
        // Total: 4 + 3 + 2 = 9.
        check_metrics::<PythonParser>(
            "def f(a, b, c, d):\n\
             \x20   if a or b or c or d:           # +4c\n\
             \x20       pass\n\
             \x20   if a and b and c:              # +3c\n\
             \x20       pass\n\
             \x20   if not a and not b:            # +2c (NotOperator x2)\n\
             \x20       pass\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_while_conditions() {
        // Python has no `do { ... } while(cond);` construct, so this
        // mirrors only the `while` half of the Java suite. The
        // walker fires on each `and` / `or` token inside the loop
        // header.
        check_metrics::<PythonParser>(
            "def f(a, b):\n\
             \x20   while a or b:                  # +2c\n\
             \x20       break\n\
             \x20   while a and not b:             # +2c (a + NotOperator)\n\
             \x20       break\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_short_circuit_with_boolean_literal_operand() {
        // `a and True` reports 2 conditions: one identifier, one
        // True literal. Confirms `True` / `False` are in the walker
        // terminal set.
        check_metrics::<PythonParser>("def f(a):\n    return a and True\n", "foo.py", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_await_expression_condition_counts() {
        // Regression for findings.md round-2 #2 (Python):
        // `if await ready(): pass` parses with `await` as the
        // condition node. Adding `Python::Await` to the
        // terminal-bool set mirrors the C# reference (lesson 19).
        check_metrics::<PythonParser>(
            "async def ready(): return True\n\
             async def f():\n    if await ready(): pass\n",
            "foo.py",
            |metric| {
                // ready() is a call (1 branch); await is the
                // condition (1).
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_if_call_terminal_condition_counts_once() {
        // Pins the Phase-2B behaviour for Python's `Call` terminal-bool
        // kind: `if foo():` is a Fitzpatrick Rule 6 unary conditional
        // (a bare boolean-evaluating call as the if-condition). The
        // walker's terminal-at-top check fires once per call-condition;
        // the call itself separately contributes 1 branch. Surfaced
        // (and verified intentional) by the code-review pass on
        // Phase 2B.
        check_metrics::<PythonParser>("def f():\n    if foo(): pass\n", "foo.py", |metric| {
            assert_eq!(metric.abc.branches_sum(), 1);
            assert_eq!(metric.abc.conditions_sum(), 1);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn python_if_boolean_literal_condition() {
        // Phase 2B (issue #403): bare-boolean conditions count once.
        // Python has no paren wrap around if-conditions, so the
        // condition node is checked directly. The existing
        // NotOperator / ComparisonOperator arms continue to fire
        // for those shapes; only the bare-terminal cases (Identifier,
        // True, False, etc.) are added by the new arm.
        check_metrics::<PythonParser>(
            "def f(a):\n\
             \x20   if True: pass        # +1c\n\
             \x20   if False: pass       # +1c\n\
             \x20   while True: break    # +1c\n\
             \x20   if a: pass           # +1c (Rule 6 — bare identifier as condition)\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_methods_arguments_with_conditions() {
        // `m(not a, not b)` reports 2 conditions — both `NotOperator`
        // nodes are counted by Python's pre-existing top-level
        // NotOperator dispatcher arm. The argument-list walker does
        // not need a separate Python arm.
        check_metrics::<PythonParser>(
            "def f(a, b):\n\
             \x20   m(a, b)             # +1b\n\
             \x20   m(not a, not b)     # +1b +2c\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn python_return_with_conditions() {
        // Phase 2B (issue #403). Python uses the pre-existing top-
        // level NotOperator / ComparisonOperator arms for return
        // expressions; no dedicated ReturnStatement walker arm is
        // needed.
        check_metrics::<PythonParser>(
            "def m1(z): return not (z >= 0)\n\
             def m2(x): return (((not x)))\n\
             def m3(x, y): return x and y\n",
            "foo.py",
            |metric| {
                // m1: NotOperator (1) + ComparisonOperator (1) = 2.
                // m2: NotOperator (1).
                // m3: walker on `and` counts both operands = 2.
                // Sum: 5.
                assert_eq!(metric.abc.conditions_sum(), 5);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_empty_unit_zero() {
        // No code at all → A=B=C=0. Establishes the trait is wired up
        // and the per-language compute is reachable.
        check_metrics::<RustParser>("", "empty.rs", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn rust_assignments_let_init_plain_and_compound() {
        // `let mut x = 0` is a `let_declaration` carrying an `=`
        // initializer → counts as 1 (matches Fitzpatrick's literal
        // "every `=` is an assignment" rule and the JS impl's
        // treatment of `let x = 5`). `x = 5` and `x = 7` are plain
        // `=` assignments → 2. `x += 2` is a compound assignment → 1.
        // Total A = 4.
        check_metrics::<RustParser>(
            "fn f() { let mut x = 0; x = 5; x += 2; x = 7; }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_let_without_initializer_does_not_count() {
        // `let a;` is a `let_declaration` with NO `=` and no `value`
        // field — the binding is uninitialised. The arm only fires
        // when `value` is present, so this contributes zero to A.
        // `let _b;` is the same shape (the `_` pattern is still a
        // pattern, not a wildcard suppression of the binding).
        // Regression test for issue #393: only `=` counts, not the
        // bare declaration.
        check_metrics::<RustParser>(
            "fn f() { let a: i32; let _b: i32; a = 5; }",
            "foo.rs",
            |metric| {
                // Only `a = 5` (assignment_expression) → A = 1.
                assert_eq!(metric.abc.assignments_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_let_initializers_immutable_and_mutable_count() {
        // Issue #393: `let a = 1;`, `let b = 2;`, `let c = a + b;`,
        // `let mut d = 0;` are all `let_declaration` nodes carrying
        // an `=` initializer — each counts as 1 (Option B in the
        // issue body: literal Fitzpatrick, both `let` and `let mut`
        // count). `d = 5;` is one plain assignment_expression, `d
        // += 1;` is one compound. Total A = 4 + 1 + 1 = 6.
        check_metrics::<RustParser>(
            "fn f() { let a=1; let b=2; let c=a+b; let mut d=0; d=5; d+=1; }",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 6);
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
                assert_eq!(metric.abc.branches_sum(), 3);
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.branches_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 6);
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
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 1);
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
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_let_chain2_hidden_rule_drift_marker() {
        // Drift marker (findings.md round-2 #3): `Rust::LetChain2`
        // maps to the hidden grammar rule `_let_chain`. At the
        // pinned tree-sitter-rust version it is never emitted as a
        // concrete node — the visible `LetChain` (= 352) carries
        // every let-chain. We list `LetChain2` defensively in
        // `rust_inspect_container` and `rust_count_unary_conditions`
        // (lesson 34); if a future grammar bump promotes
        // `_let_chain` to a visible rule, this assertion fails
        // loudly so the maintainer knows to verify the walker still
        // counts correctly for the new shape.
        let src = "fn f(a: bool, b: Option<i32>) {\n\
                   \x20   if a && let Some(_) = b { }\n\
                   }\n";
        let parser = RustParser::new(
            src.as_bytes().to_vec(),
            &std::path::PathBuf::from("foo.rs"),
            None,
        );
        assert!(!ast_has_kind_id(&parser, Rust::LetChain2 as u16));
    }

    #[test]
    fn rust_scoped_identifier_condition_counts() {
        // Regression for findings.md round-2 #1 (Rust):
        // `if crate::FLAG {}` parses with `scoped_identifier` as the
        // condition node. Pre-fix, `rust_bool_terminal_kinds!()`
        // listed only `Identifier` so the walker reached the
        // `scoped_identifier` child, found it non-terminal /
        // non-paren / non-unary, and broke without counting.
        // Mirrors the C# fix in #372 (lesson 19) for
        // `MemberAccessExpression`.
        check_metrics::<RustParser>("fn f() { if crate::FLAG { } }\n", "foo.rs", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 1);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn rust_await_expression_condition_counts() {
        // Regression for findings.md round-2 #2 (Rust):
        // `if ready().await {}` parses with `await_expression` as
        // the condition node. Adding `Rust::AwaitExpression` to the
        // terminal-bool set closes the parity gap with the C#
        // reference (`csharp_bool_terminal_kinds!()`).
        check_metrics::<RustParser>(
            "async fn ready() -> bool { true }\n\
             async fn f() { if ready().await { } }\n",
            "foo.rs",
            |metric| {
                // ready() is a call (1 branch); `ready().await` is
                // the unary boolean condition (1).
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_complex_function_abc() {
        // Mixed-shape regression: assignments, calls, conditions, `?`,
        // `if let`, `match` in one body. Verified by hand:
        // - assignments: `let mut x = 0` (let init), `x = 5`, `x += 2`,
        //   `let _ = ...` (let init), `let r: ... = Err(())` (let init),
        //   `let _v = r?` (let init) → A = 6 (post-#393: every `=`
        //   initializer in a `let_declaration` is one assignment, in
        //   line with the literal Fitzpatrick reading).
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
                assert_eq!(metric.abc.assignments_sum(), 6);
                // calls: xs.iter(), .next(), Err(()), Ok(...) → 4 calls
                // plus 1 try (`r?`) → 5 branches.
                assert_eq!(metric.abc.branches_sum(), 5);
                // 1 let_condition + 2 non-wildcard match_arms + 1
                // comparison (`n > 0`) → 4.
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_let_chain_bare_identifier_operand_counts() {
        // Regression: pre-fix, `if a && let Some(_z) = y { }` reported
        // 1 condition (only the LetCondition). The bare-identifier
        // `a` operand was lost because Rust 2024 wraps let-chain
        // `&&` operands in a `LetChain` node (not `BinaryExpression`)
        // and `rust_count_unary_conditions` only counted terminals
        // under a `BinaryExpression` parent. Allowing `LetChain` /
        // `LetChain2` as known-bool list parents fixes the loss.
        // Expected: LetCondition (1) + walker on `a` (1) = 2.
        check_metrics::<RustParser>(
            "fn f(a: bool, y: Option<i32>) {\n\
             \x20   if a && let Some(_z) = y { }\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_if_multiple_conditions() {
        // Fitzpatrick Rule 7 / Listing 2 (issue #403): every operand of
        // a `&&` / `||` chain is one condition. Mirrors
        // `java_if_multiple_conditions`. Rust's `if` head has no
        // parentheses, but the walker fires on each `&&` / `||` token
        // and walks the parent `binary_expression` regardless.
        check_metrics::<RustParser>(
            "fn f(a: bool, b: bool, c: bool, d: bool) -> i32 {\n\
             \x20   if a || b || c || d { return 1; }    // +4c\n\
             \x20   if a && b && c { return 2; }         // +3c\n\
             \x20   if !a && !b { return 3; }            // +2c\n\
             \x20   0\n\
             }\n",
            "foo.rs",
            |metric| {
                // 4 + 3 + 2 = 9
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_while_conditions() {
        // Rust has no `do { ... } while(cond);` construct, so this
        // mirrors only the `while` half of `java_while_and_do_while_conditions`.
        // Each operand of the `&&` / `||` chain in the loop condition
        // counts as one Fitzpatrick condition (Rule 7).
        check_metrics::<RustParser>(
            "fn f(a: bool, b: bool) {\n\
             \x20   while a || b { break; }       // +2c\n\
             \x20   while a && !b { break; }      // +2c\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_if_boolean_literal_condition() {
        // Phase 2B (issue #403): a condition whose entire body is a
        // boolean literal counts as one Fitzpatrick condition.
        // `if true {}` → 1, `if !false {}` → 1 (unary unwrap), and
        // `while true { break }` → 1.
        check_metrics::<RustParser>(
            "fn f() {\n\
             \x20   if true { }                  // +1c\n\
             \x20   if !false { }                // +1c\n\
             \x20   while true { break; }        // +1c\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_methods_arguments_with_conditions() {
        // Phase 2B (issue #403): unary-conditional arguments to a
        // call each count once. `m(!a, !b)` → 2 conditions + 1
        // branch (the call itself). Bare identifier arguments do
        // NOT count (they reach the count_unary_conditions list with
        // list_kind = Arguments, not BinaryExpression).
        check_metrics::<RustParser>(
            "fn f(a: bool, b: bool) {\n\
             \x20   m(a, b);                     // +1b\n\
             \x20   m(!a, !b);                   // +1b +2c\n\
             \x20   m(!a, b, !a);                // +1b +2c\n\
             }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 3);
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_return_with_conditions() {
        // Phase 2B (issue #403). Mirrors `java_return_with_conditions`
        // — `return !a` / `return x && y` count their unary
        // conditional operands. Per Fitzpatrick Rule 7, a `!`-wrapped
        // relational expression contributes ONE condition (the
        // relational op itself) — the `!` does not add a second
        // count when its operand is already a comparison.
        check_metrics::<RustParser>(
            "fn m1(z: i32) -> bool { return !(z >= 0); }\n\
             fn m2(x: bool) -> bool { return (((!x))); }\n\
             fn m3(x: bool, y: bool) -> bool { return x && y; }\n\
             fn m4(y: bool, z: i32) -> bool { return y || (z < 0); }\n",
            "foo.rs",
            |metric| {
                // m1: !(z >= 0) → the `>=` contributes 1; the unary
                //     `!` wraps a paren'd BinaryExpression, which
                //     inspect_container does not unwrap further →
                //     no walker count. Total: 1.
                // m2: (((!x))) → ReturnExpression arm walks (((!x))).
                //     inspect_container unwraps three parens and one
                //     unary, reaches Identifier `x`, has_boolean_content
                //     was seeded true by the unary-not flip. +1.
                // m3: x && y → `&&` walker counts both terminals → 2.
                // m4: y || (z < 0) → `||` walker counts `y` (terminal,
                //     +1); the `<` contributes 1 via its own arm; the
                //     paren'd BinaryExpression `(z < 0)` is not
                //     terminal under the walker → no extra count.
                //     Total: 2.
                // Sum: 1 + 1 + 2 + 2 = 6.
                assert_eq!(metric.abc.conditions_sum(), 6);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn rust_short_circuit_with_boolean_literal_operand() {
        // `if a && true` reports 2 conditions: one for the identifier
        // operand, one for the boolean-literal operand. Confirms the
        // walker terminal set includes `BooleanLiteral`.
        check_metrics::<RustParser>(
            "fn f(a: bool) -> bool { a && true }\n",
            "foo.rs",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
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
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn go_assignments_count_plain_compound_short_var_and_incdec() {
        // `x := 0` (short var decl), `x = 5` and `x = 7` (plain `=`),
        // `x += 2` (compound), `x++` (inc), and the initialized
        // declaration `var y = 1` — which is counted, matching the Rust
        // and Java rules for `let y = 1` / `int y = 1` (both measured at
        // one assignment each). The comment here previously claimed the
        // opposite and pinned Go at 6 (#1278).
        check_metrics::<GoParser>(
            "package main\nfunc f() { var y = 1; _ = y; x := 0; x = 5; x += 2; x = 7; x++ }\n",
            "foo.go",
            |metric| {
                // `_ = y` is itself an assignment_statement → +1.
                // var y=1 + _=y + x:= + x=5 + x+=2 + x=7 + x++ → 7
                assert_eq!(metric.abc.assignments_sum(), 7);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_var_declarations_count_only_when_initialized() {
        // Regression for #1278: a `var` declaration with an initializer is
        // a `var_spec` carrying a `value` field, not an
        // `assignment_statement` or `short_var_declaration`, so it scored
        // zero — `var x = 5` and `x := 5` are the same binding spelled two
        // ways. Both typed and untyped initializers count; an
        // uninitialized `var z int` and a `const` do not.
        // expected: 3 assignments — `var x = 5`, `var y int = 6`, `z := 7`;
        // `var w int` and `const c = 1` contribute nothing.
        check_metrics::<GoParser>(
            "package main\nfunc f() int {\n\tvar x = 5\n\tvar y int = 6\n\tvar w int\n\tconst c = 1\n\tz := 7\n\treturn x + y + w + c + z\n}\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 3);
                assert_eq!(metric.abc.branches_sum(), 0);
            },
        );
    }

    #[test]
    fn go_grouped_var_block_counts_each_initialized_spec() {
        // A grouped `var ( … )` block is one `var_declaration` holding one
        // `var_spec` per line, so matching the spec counts each initialized
        // line on its own. A multi-name spec is still one binding
        // statement, matching `p, q := 1, 2` (#1278).
        // expected: 3 assignments — `a = 1`, `p, q = 1, 2`, and `r := 0`;
        // the uninitialized `b int` contributes nothing.
        check_metrics::<GoParser>(
            "package main\nfunc f() {\n\tvar (\n\t\ta = 1\n\t\tb int\n\t)\n\tvar p, q = 1, 2\n\tr := 0\n\t_ = a + b + p + q + r\n}\n",
            "foo.go",
            |metric| {
                // The trailing `_ = …` is itself an assignment_statement.
                assert_eq!(metric.abc.assignments_sum(), 4);
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
                assert_eq!(metric.abc.branches_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 6);
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
                assert_eq!(metric.abc.conditions_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 1);
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
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_complex_function_abc() {
        // Mixed shape, verified by hand:
        // - Assignments: `var x = 10` (an initialized declaration, #1278),
        //   `_ = x`, `n := 0`, `n = n + 1`, `n += 2`, `n++`,
        //   `_ = len(s)` → A = 7. Every `_ = ...` IS counted as an
        //   assignment_statement.
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
                assert_eq!(metric.abc.assignments_sum(), 7);
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_if_multiple_conditions() {
        // Fitzpatrick Rule 7 walker fan-out (issue #403). Mirrors
        // `rust_if_multiple_conditions`.
        check_metrics::<GoParser>(
            "package p\n\
             func F(a, b, c, d bool) int {\n\
             \x20   if a || b || c || d { return 1 }    // +4c\n\
             \x20   if a && b && c { return 2 }         // +3c\n\
             \x20   if !a && !b { return 3 }            // +2c\n\
             \x20   return 0\n\
             }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_for_with_conditions() {
        // Go has no `while` or `do { … } while(…);` — the `for` loop
        // header is the sole condition slot. Each operand of the
        // `&&` / `||` chain in the for-condition counts as one
        // Fitzpatrick condition.
        check_metrics::<GoParser>(
            "package p\n\
             func F(a, b bool) {\n\
             \x20   for a || b { break }       // +2c\n\
             \x20   for a && !b { break }      // +2c\n\
             }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_for_bare_condition_counts() {
        // Regression for findings.md #1: `for true {}` / `for !ready {}`
        // are Go's only loop-condition slot. Pre-fix, the Phase-2B
        // dispatcher had no `G::ForStatement` arm, so bare-boolean
        // and `!`-wrapped `for` conditions silently reported zero.
        // `go_count_condition`'s terminal-bool / paren / unary filter
        // makes the walker safe across all three for-statement shapes:
        // bare condition, `for_clause` (init; cond; post) — whose own
        // `condition` field #1276 taught the walker to read — and
        // `range_clause`, which has no such field and contributes
        // nothing.
        check_metrics::<GoParser>(
            "package p\n\
             func F(ready bool) {\n\
             \x20   for true { break }      // +1c\n\
             \x20   for !ready { break }    // +1c\n\
             \x20   for i := 0; i < 3; i++ { _ = i }    // +1c (the `<`)\n\
             }\n",
            "foo.go",
            |metric| {
                // `for true`: walker counts True (+1).
                // `for !ready`: walker on unary unwraps to `ready`
                //   (+1).
                // `for_clause`'s condition is `i < 3`, a
                //   `binary_expression` that `go_count_condition`
                //   filters out; the `<` itself contributes 1 via the
                //   pre-existing LT/GT arm.
                // Total: 3.
                assert_eq!(metric.abc.conditions_sum(), 3);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Issue #1276, Go's share. `for_statement`'s child(1) is the
    // condition only in the `for cond {}` spelling; the three-clause
    // form puts it one level down, in the `for_clause`'s `condition`
    // field. Letting the `for_clause` fall through — which the arm's
    // own comment used to call harmless — scored a bare three-clause
    // condition zero while `for a {}` scored one.
    #[test]
    fn go_three_clause_for_condition_counts() {
        // Bare identifier in the three-clause header: no comparison
        // token, so only the `for_clause` lookup can count it.
        check_metrics::<GoParser>(
            "package p\n\
             func F(a bool) {\n\
             \x20   for i := 0; a; i++ { _ = i }\n\
             }\n",
            "foo.go",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
        );
        // Negation, through `go_inspect_container`'s `!` unwrap.
        check_metrics::<GoParser>(
            "package p\n\
             func F(a bool) {\n\
             \x20   for i := 0; !a; i++ { _ = i }\n\
             }\n",
            "foo.go",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
        );
        // Empty condition: the `for_clause` exposes no `condition`
        // field, so zero — the same answer as Go's bare `for {}` and
        // as every other language since #1276.
        check_metrics::<GoParser>(
            "package p\n\
             func F() {\n\
             \x20   for i := 0; ; i++ { break }\n\
             }\n",
            "foo.go",
            |metric| assert_eq!(metric.abc.conditions_sum(), 0),
        );
        // A `range_clause` carries no condition either, and the walker
        // must not mistake the clause itself for one.
        check_metrics::<GoParser>(
            "package p\n\
             func F(xs []int) {\n\
             \x20   for _, v := range xs { _ = v }\n\
             }\n",
            "foo.go",
            |metric| assert_eq!(metric.abc.conditions_sum(), 0),
        );
    }

    // Go's `for_statement` is the one grammar here that exposes no
    // `condition` field, so its header slot is located structurally and
    // has to skip what the field-addressed siblings get for free. Two
    // things it must skip, each of which `node.child(1)` got wrong:
    // a leading comment (tree-sitter counts comments among a node's
    // children — the #1181 failure), and the body of a bare `for {}`,
    // which IS child(1) and would otherwise be offered to
    // `go_count_condition` as though it were a condition.
    #[test]
    fn go_for_header_slot_skips_comments_and_the_body() {
        // Each pair is (source, expected conditions). The commented
        // spelling must agree with its bare twin.
        let cases = [
            ("for a { break }", 1),
            ("for /* n */ a { break }", 1),
            ("for i := 0; a; i++ { _ = i }", 1),
            ("for /* n */ i := 0; a; i++ { _ = i }", 1),
            ("for i := 0; /* n */ a; i++ { _ = i }", 1),
            // Bare infinite loop: the body is child(1) and must not be
            // read as the condition.
            ("for { break }", 0),
            ("for /* n */ { break }", 0),
            ("for _, v := range xs { _ = v }", 0),
            ("for /* n */ _, v := range xs { _ = v }", 0),
        ];
        let mut ran = 0;
        for (body, expected) in cases {
            let src = format!("package p\nfunc F(a bool, xs []int) {{\n\t{body}\n}}\n");
            assert_eq!(abc_conditions(LANG::Go, &src), expected, "`{body}`");
            ran += 1;
        }
        // Non-vacuity, both halves: the loop must actually have run
        // every row, and the rows must carry both answers — a walker
        // stuck at 0 or at 1 would otherwise pass half the table
        // silently.
        assert_eq!(ran, cases.len());
        assert!(cases.iter().any(|&(_, n)| n == 1));
        assert!(cases.iter().any(|&(_, n)| n == 0));
    }

    #[test]
    fn go_if_init_statement_condition_counts() {
        // Regression for the code-review finding: Go's
        // `if x := f(); x { ... }` init-statement form puts the
        // short-var declaration at child(1) and the condition at
        // child(2). Pre-fix, the dispatcher used child(1) and
        // counted zero conditions for this idiomatic Go shape.
        // The fix uses `child_by_field_name("condition")` which
        // returns the condition regardless of init presence.
        check_metrics::<GoParser>(
            "package p\nfunc F() { if x := g(); x { } }\n",
            "foo.go",
            |metric| {
                // `x` bare-identifier condition contributes 1
                // (Rule 6 — bare boolean identifier in if-condition).
                // `g()` call contributes 1 branch but no condition.
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_if_boolean_literal_condition() {
        check_metrics::<GoParser>(
            "package p\n\
             func F() {\n\
             \x20   if true {}                  // +1c\n\
             \x20   if !false {}                // +1c\n\
             }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_methods_arguments_with_conditions() {
        check_metrics::<GoParser>(
            "package p\n\
             func F(a, b bool) {\n\
             \x20   m(a, b)                     // +1b\n\
             \x20   m(!a, !b)                   // +1b +2c\n\
             }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_return_with_conditions() {
        check_metrics::<GoParser>(
            "package p\n\
             func M1(z int) bool { return !(z >= 0) }\n\
             func M2(x bool) bool { return !x }\n\
             func M3(x, y bool) bool { return x && y }\n",
            "foo.go",
            |metric| {
                // M1: `>=` (1). `!(z >= 0)` walker on the unary
                //     doesn't reach a terminal — stops at the
                //     BinaryExpression z>=0 inside the parens. +1.
                // M2: walker on `!x` → 1.
                // M3: `&&` walker counts both → 2.
                // Sum: 1 + 1 + 2 = 4.
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn go_short_circuit_with_boolean_literal_operand() {
        // `a && true` reports 2 conditions: one identifier, one
        // boolean literal. Confirms the terminal set includes
        // `True` / `False`.
        check_metrics::<GoParser>(
            "package p\nfunc F(a bool) bool { return a && true }\n",
            "foo.go",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
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
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
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
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 2);
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
                assert_eq!(metric.abc.branches_sum(), 5);
                assert_eq!(metric.abc.assignments_sum(), 0);
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
                assert_eq!(metric.abc.conditions_sum(), 6);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn elixir_unary_conditions_in_chain() {
        // Fitzpatrick Rule 9 (issue #557): each bare boolean operand of a
        // `&&` / `||` chain is one condition. For `if a && b || c`: the
        // `if` keyword Call contributes 1 condition, and the walker adds
        // a, b, c → 3. expected: 4 conditions, consistent with the
        // function's cyclomatic complexity of 4 (base 1 + if + && + ||).
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(a, b, c) do\n    if a && b || c do\n      IO.puts(\"x\")\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
            },
        );
    }

    #[test]
    fn elixir_comparison_operands_add_nothing() {
        // Isolation check: comparison operands of a `&&` chain are nested
        // `binary_operator` nodes, not bare boolean leaves, so the walker
        // adds nothing. expected: 3 = `if` (1) + `>` (1) + `>` (1); the
        // `&&` walker contributes 0.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x, y) do\n    if x > 0 && y > 0 do\n      IO.puts(\"x\")\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3);
            },
        );
    }

    #[test]
    fn elixir_keyword_and_or_chain_counts_operands() {
        // The keyword forms `and` / `or` get the same Rule 9 treatment as
        // `&&` / `||`. expected: 4 = `if` (1) + operands a, b, c (3).
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(a, b, c) do\n    if a and b or c do\n      IO.puts(\"x\")\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
            },
        );
    }

    // Sigil delimiter choice must not move ABC: `~s<hi>` and `~s(hi)`
    // are the same value spelled differently, but the `<` / `>`
    // delimiter tokens carry the comparison kind ids, so the unguarded
    // condition arm scored `~s<hi>` as 2 conditions and `~s(hi)` as 0.
    // The parent-is-`Sigil` guard (mirroring the Halstead getter's,
    // #1256) suppresses the delimiter case. expected, for each
    // spelling: A = 1 (the `x =` pattern match), B = 0 (a bare sigil
    // is not a `Call` node — verified by AST dump: `binary_operator`
    // wrapping `identifier`, `=`, `sigil`), C = 0 (no comparison, no
    // guard, no keyword Call).
    #[test]
    fn elixir_sigil_delimiter_choice_is_abc_invariant() {
        for src in ["x = ~s<hi>\n", "x = ~s(hi)\n"] {
            check_metrics::<ElixirParser>(src, "foo.ex", |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
            });
        }
    }

    // Control for the guard above: `<` *outside* a sigil is a genuine
    // comparison and must keep counting even with a `<`-delimited
    // sigil in the same unit. expected: A = 2 (`x =`, `y =`), C = 1
    // (only `a < b`; the sigil's `<` / `>` delimiters are guarded).
    #[test]
    fn elixir_lt_comparison_still_counts_beside_sigil() {
        check_metrics::<ElixirParser>("x = ~s<hi>\ny = a < b\n", "foo.ex", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 2);
            assert_eq!(metric.abc.conditions_sum(), 1);
        });
    }

    // ----- C++ -----

    #[test]
    fn cpp_empty_unit_zero() {
        // No code → A=B=C=0. Wires up the trait and exercises the
        // per-language compute reachability.
        check_metrics::<CppParser>("", "empty.cpp", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn cpp_plain_and_compound_assignments_count() {
        // `int x = 0` is an `init_declarator` carrying an `=` token
        // and counts as 1 (post-#393: the literal Fitzpatrick rule
        // counts every `=` operator, matching the JS impl's
        // `let x = 5` treatment). `x = 5`, `x += 2`, `x = 7` all
        // parse as `assignment_expression` → 3. Total A = 4.
        check_metrics::<CppParser>(
            "void f() { int x = 0; x = 5; x += 2; x = 7; }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_increment_and_decrement_count_as_assignment() {
        // `x++` / `--x` / prefix and postfix forms each parse as
        // `update_expression` and count as 1 assignment per
        // Fitzpatrick — 4. `int x = 0` (init_declarator with `=`)
        // adds 1 (post-#393). Total A = 5.
        check_metrics::<CppParser>(
            "void f() { int x = 0; x++; --x; ++x; x--; }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 5);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_init_declarators_count_as_assignments() {
        // Issue #393 regression: `int a=1;`, `int b=2;`, `int c=a+b;`,
        // `int d=0;` are all `init_declarator` nodes with `=` → 4
        // assignments. `d=5;` is one plain `assignment_expression`,
        // `d+=1;` is one compound. Total A = 4 + 1 + 1 = 6.
        check_metrics::<CppParser>(
            "void f() { int a=1; int b=2; int c=a+b; int d=0; d=5; d+=1; }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 6);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_declaration_without_initializer_does_not_count() {
        // `int a;` parses as a plain declarator inside `declaration`,
        // NOT an `init_declarator` (the latter only appears when an
        // initializer is present). Regression test for issue #393:
        // un-initialised declarations contribute zero to A.
        check_metrics::<CppParser>("void f() { int a; a = 5; }", "foo.cpp", |metric| {
            // Only `a = 5` (assignment_expression) → A = 1.
            assert_eq!(metric.abc.assignments_sum(), 1);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn cpp_init_declarator_brace_paren_init_does_not_count() {
        // `init_declarator` has two grammar forms: `declarator = value`
        // (the `=` form) and `declarator argument_list_or_initializer_list`
        // (the `int x(5);` / `int x{5};` direct-init forms). Only the
        // first form contains an `=` token, so only it should count.
        // Regression test pinning that distinction so that
        // refactorings of the init_declarator arm don't accidentally
        // start counting direct-init too.
        check_metrics::<CppParser>(
            "void f() { int x(5); int y{7}; x = 1; }",
            "foo.cpp",
            |metric| {
                // Only `x = 1` (assignment_expression) → A = 1.
                assert_eq!(metric.abc.assignments_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_calls_are_branches() {
        // Free call + member-fn call (parses as `call_expression` with
        // a `field_expression` callee) + `new` allocation. All three
        // are branches → B = 3. `auto* p = new int(5)` is also an
        // `init_declarator` with `=` so it contributes one assignment
        // (post-#393); the snapshot pins that magnitude.
        check_metrics::<CppParser>(
            "struct S { void m(); }; void g(); void f() { g(); S s; s.m(); auto* p = new int(5); }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 3);
                assert_eq!(metric.abc.assignments_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_comparisons_count_conditions() {
        // `<`, `>`, `<=`, `>=`, `==`, `!=`, and the C++20 spaceship
        // `<=>` each contribute one condition. The `||` short-
        // circuits add 0 (Fitzpatrick Rule 5, issue #395). Six
        // comparisons in the `||` chain plus `<=>` (1) plus the
        // outer `== 0` (1) → C = 8.
        check_metrics::<CppParser>(
            "#include <compare>\n\
             bool f(int a, int b) {\n\
                 return a < b || a > b || a <= b || a >= b || a == b || a != b || (a <=> b) == 0;\n\
             }\n",
            "foo.cpp",
            |metric| {
                // `<`, `>`, `<=`, `>=`, `==`, `!=` → 6 comparisons
                // from the chained `||` expression. `(a <=> b) == 0`
                // adds the spaceship `<=>` (1) + the outer `== 0`
                // (1) → 8 total. The six `||` short-circuits add 0
                // (Fitzpatrick Rule 5; issue #395).
                assert_eq!(metric.abc.conditions_sum(), 8);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_short_circuit_ops_not_counted_directly() {
        // `&&` and `||` do NOT count on their own (see the
        // module-level `Stats` doc-comment; #395). Phase-2 walker
        // counts each operand of a logical chain once (#403), but
        // when every operand is itself a relational expression
        // (`a == b`, `a > 0`, `b < 0`) the walker doesn't add
        // anything on top of the existing comparison-token tally
        // — relational sub-expressions are not in
        // `cpp_bool_terminal_kinds!()` and `cpp_inspect_container`
        // does not recurse into them.
        check_metrics::<CppParser>(
            "bool f(int a, int b) { return a == b && a > 0 || b < 0; }",
            "foo.cpp",
            |metric| {
                // == 1, > 1, < 1; the walker on && and || finds
                // BinaryExpression operands (not terminal-bool) and
                // adds nothing. Total: 3.
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Issue #1102. A ternary's condition and both branch operands are
    // Fitzpatrick Rule 9 unary conditions, exactly as `java_walk_ternary`
    // has always counted them. Before the fix the C family scored
    // `a ? !b : !c` as 1 — the `?` token alone — against Java's 4.
    #[test]
    fn cpp_ternary_operand_slots_count_as_unary_conditions() {
        // `?` (1) + condition `a` (1) + `!b` (1) + `!c` (1) = 4.
        check_metrics::<CppParser>("void f() { x = a ? !b : !c; }", "foo.cpp", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 4);
        });
        // No-double-count pin: `?` (1) + `>` (1) = 2, unchanged by the
        // fix. The parenthesised condition unwraps to a
        // `binary_expression`, which is not a boolean terminal, and
        // neither branch is negated — the `!` is the type-free proxy for
        // "this operand is boolean", so an unnegated branch contributes
        // nothing.
        check_metrics::<CppParser>("void f() { x = (a > 0) ? b : -b; }", "foo.cpp", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
        });
        // Nested: outer `?` (1) + outer condition `a` (1) + inner `?`
        // (1) + inner condition `b` (1) = 4. The outer consequence is
        // the inner ternary — neither a boolean terminal nor a
        // paren / `!` wrapper — so it adds nothing on its own and the
        // inner one is reached by the walk, not by descent.
        check_metrics::<CppParser>("void f() { x = a ? b ? c : d : e; }", "foo.cpp", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 4);
        });
        // A negated *condition* is the only input that reaches the
        // walker's `else` fallback: `!a` is neither a boolean terminal
        // (so the terminal arm skips it) nor an operand slot (so
        // `cpp_inspect_container` is never called on it from anywhere
        // else). Every other condition fixture in this file wraps a
        // comparison, which the fallback resolves to 0 — delete the
        // fallback and only this case moves. `?` (1) + `!a` (1) = 2.
        check_metrics::<CppParser>("void f() { x = !a ? b : c; }", "foo.cpp", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
        });
    }

    // The GNU short-ternary `a ?: b` elides the consequence, so the
    // C-family grammar marks that field optional and the alternative
    // lands at child(3) rather than child(4). Addressing the operand
    // slots by grammar field name — never by index — is what keeps `!b`
    // counted here; a fixed `child(4)` reads `None` and scores 2.
    #[test]
    fn cpp_elided_ternary_consequence_still_walks_the_alternative() {
        // `?` (1) + condition `a` (1) + `!b` (1) = 3.
        check_metrics::<CppParser>("void f() { x = a ?: !b; }", "foo.cpp", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 3);
        });
    }

    // `cpp_walk_ternary` is shared by the C, ObjC, and Mozcpp ABC impls
    // exactly as `cpp_inspect_container` is, so each needs its own
    // dispatcher arm. Mozcpp owns no file extension and so gets no
    // integration-snapshot coverage at all — this parity assertion is
    // its only guard.
    //
    // The expected value is *derived from the C++ run*, not hardcoded,
    // so the four languages cannot silently drift apart if the C++
    // expectation ever legitimately moves.
    #[test]
    fn c_family_ternary_operand_slots_agree_with_cpp() {
        const SRC: &str = "void f() { x = a ? !b : !c; }\n";
        let conditions = abc_conditions;

        let cpp = conditions(LANG::Cpp, SRC);
        // Non-degenerate: a zeroed reference would make every
        // comparison below vacuous.
        assert_eq!(cpp, 4, "C++ reference value for `a ? !b : !c`");

        assert_eq!(conditions(LANG::C, SRC), cpp, "C must match C++");
        assert_eq!(conditions(LANG::Mozcpp, SRC), cpp, "Mozcpp must match C++");
        assert_eq!(
            conditions(
                LANG::Objc,
                "@implementation Foo\n\
                 - (void)bar {\n\
                     x = a ? !b : !c;\n\
                 }\n\
                 @end\n",
            ),
            cpp,
            "ObjC must match C++"
        );
    }

    // Issue #1276, C-family half. `cpp_walk_for_statement` is the
    // `for` header's counterpart to the `if` / `while` arms: the slot
    // is a bare expression rather than a `condition_clause`, so it
    // needs the top-level terminal check `cpp_walk_ternary` already
    // had. Every fixture is a shape only that walker can classify —
    // a comparison-shaped condition proves nothing, the `<` token arm
    // counts it either way (grammar-dispatch §11).
    #[test]
    fn cpp_for_condition_slot_counts_unary_conditions() {
        // Bare identifier: the whole condition, no operator token.
        check_metrics::<CppParser>("void f(int a) { for (; a; ) {} }", "foo.cpp", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 1);
        });
        // Negation: reaches the terminal through
        // `cpp_inspect_container`'s `!` unwrap.
        check_metrics::<CppParser>("void f(int a) { for (; !a; ) {} }", "foo.cpp", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 1);
        });
        // Parentheses: counts only because the `for_statement` parent
        // seeds `has_boolean_content` — the seed #1276 found dead.
        check_metrics::<CppParser>("void f(int a) { for (; (a); ) {} }", "foo.cpp", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 1);
        });
        // No-double-count pin: the `<` token arm already counted this
        // shape before the fix and the walker must not add a second.
        // The two assignments confirm the header parsed as the
        // three-clause form rather than degenerating.
        check_metrics::<CppParser>(
            "void f(int n) { for (int i = 0; i < n; i++) {} }",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
        // Empty condition: no `condition` field, no decision, zero.
        check_metrics::<CppParser>("void f() { for (;;) { break; } }", "foo.cpp", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 0);
        });
    }

    // `cpp_walk_for_statement` is shared by the C, ObjC and Mozcpp ABC
    // impls the way `cpp_walk_ternary` is, so each needs its own
    // dispatcher arm — and Mozcpp, which owns no file extension, has no
    // integration-snapshot coverage at all, making this its only guard.
    // The expected value is derived from the C++ run rather than
    // hardcoded, so the four cannot silently drift apart.
    #[test]
    fn c_family_for_condition_slot_agrees_with_cpp() {
        const SRC: &str = "void f(int a) { for (; !a; ) {} }\n";
        let conditions = abc_conditions;

        let cpp = conditions(LANG::Cpp, SRC);
        // Non-degenerate: a zeroed reference makes every comparison
        // below vacuous.
        assert_eq!(cpp, 1, "C++ reference value for `for (; !a; )`");

        assert_eq!(conditions(LANG::C, SRC), cpp, "C must match C++");
        assert_eq!(conditions(LANG::Mozcpp, SRC), cpp, "Mozcpp must match C++");
        assert_eq!(
            conditions(
                LANG::Objc,
                "@implementation Foo\n\
                 - (void)bar {\n\
                     for (; !a; ) {}\n\
                 }\n\
                 @end\n",
            ),
            cpp,
            "ObjC must match C++"
        );
    }

    #[test]
    fn cpp_switch_cases_count_default_excluded() {
        // `case 1`, `case 2` → 2 conditions. `default` is intentionally
        // excluded (the unconditional fallthrough, mirroring cyclomatic's
        // `Case`-only count). Since #469 every C-family language —
        // Java, C#, Groovy, JS, TS — agrees on this; C++ already did.
        // C = 2.
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
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.conditions_sum(), 3);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_complex_function_abc() {
        // Mixed-shape regression: assignments, calls, conditions,
        // ternary, switch, new. Verified by hand:
        // - assignments: `int x = 0` (init_declarator with `=`),
        //   `x = 5`, `x += 2`, `x++`, `x = (a > b) ? a : b`, `x = b`,
        //   `auto* p = new int(5)` (init_declarator with `=`) → A = 7
        //   (post-#393: every `=` in an init_declarator counts).
        // - branches: `f(a, b)` self-call + `new int(5)` → B = 2.
        // - conditions: `a == b` (1) + `a > 0` (1) inside the if;
        //   `&&` itself is NOT a condition (Fitzpatrick Rule 5,
        //   issue #395). `a > b` (1) + `?` (1) in the ternary.
        //   `else` (1, from the `else if` keyword) + `a < b` (1)
        //   in the else-if. `!x` contributes 1 via the unary-
        //   conditional walker (Fitzpatrick Rule 9, issue #403):
        //   the `||` walker treats `!x` as a unary boolean operand
        //   and counts the wrapped Identifier once. `case 1`,
        //   `case 2` → 2. `default` excluded. Total C = 9.
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
                assert_eq!(metric.abc.assignments_sum(), 7);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_if_multiple_conditions() {
        // Fitzpatrick Rule 9 walker (issue #403): each operand of a
        // `&&` / `||` chain is one condition.
        check_metrics::<CppParser>(
            "void f(bool a, bool b, bool c, bool d) {\n\
             \x20   if (a || b || c || d) {}        // +4c\n\
             \x20   if (a && b && c) {}             // +3c\n\
             \x20   if (!a && !b) {}                // +2c\n\
             }\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_while_and_do_while_conditions() {
        // Exercise both the WhileStatement and DoStatement arms via
        // the walker on the `&&` / `||` tokens inside their parens.
        check_metrics::<CppParser>(
            "void f(bool a, bool b) {\n\
             \x20   while (a || b) {}              // +2c\n\
             \x20   do {} while (a && !b);         // +2c\n\
             }\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_if_constexpr_condition_counts() {
        // Regression for the code-review finding: C++ `if constexpr
        // (cond)` puts the `constexpr` keyword at child(1) and the
        // condition_clause at child(2). Pre-fix, the dispatcher used
        // child(1) and counted zero conditions for the `constexpr`
        // form. The fix uses `child_by_field_name("condition")`
        // which returns the condition_clause regardless of the
        // optional `constexpr` keyword.
        check_metrics::<CppParser>(
            "template <int N> void f() {\n\
             \x20   if constexpr (true) { }      // +1c\n\
             \x20   if (false) { }               // +1c\n\
             }\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_cast_expression_in_logical_chain_counts() {
        // Regression for findings.md round-2 #1 (C++):
        // `if ((bool)ptr && ready) {}` had the `||` walker missing
        // the `(bool)ptr` operand because `CastExpression` was not
        // in `cpp_bool_terminal_kinds!()`. Mirrors C#'s
        // `csharp_bool_terminal_kinds!()` which lists
        // `CastExpression` (lesson 19, #372).
        check_metrics::<CppParser>(
            "void f(void* ptr, bool ready) { if ((bool)ptr && ready) { } }\n",
            "foo.cpp",
            |metric| {
                // `&&` walker counts both operands: `(bool)ptr` (1)
                // and `ready` (1). Total: 2.
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_qualified_identifier_condition_counts() {
        // Regression for findings.md #3 (C++): tree-sitter-cpp emits
        // `qualified_identifier` under four kind_ids (573..576) per
        // the production-rule path; runtime kind for `ns::flag` is
        // 574 (`QualifiedIdentifier2`). Pre-fix the
        // `cpp_bool_terminal_kinds!()` macro listed neither the
        // primary nor any alias, so `if (n::flag) {}` reported zero
        // conditions. The macro now includes all four variants
        // (lesson #2).
        check_metrics::<CppParser>(
            "namespace n { extern bool flag; }\n\
             void f() { if (n::flag) { } }\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_if_boolean_literal_condition() {
        check_metrics::<CppParser>(
            "void f() {\n\
             \x20   if (true) {}                 // +1c\n\
             \x20   if (!false) {}               // +1c\n\
             \x20   while (true) {}              // +1c\n\
             \x20   do {} while (false);         // +1c\n\
             }\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_methods_arguments_with_conditions() {
        check_metrics::<CppParser>(
            "void f(bool a, bool b) {\n\
             \x20   m(a, b);                     // +1b\n\
             \x20   m(!a, !b);                   // +1b +2c\n\
             }\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_return_with_conditions() {
        check_metrics::<CppParser>(
            "bool m1(int z) { return !(z >= 0); }\n\
             bool m2(bool x) { return (((!x))); }\n\
             bool m3(bool x, bool y) { return x && y; }\n",
            "foo.cpp",
            |metric| {
                // m1: !(z >= 0) → `>=` (1). `!` wraps a paren'd
                //     BinaryExpression — inspect_container reaches
                //     the inner BinaryExpression and stops, no
                //     walker count. +1.
                // m2: (((!x))) → ReturnStatement → inspect_container
                //     unwraps three parens + one unary → reaches `x`
                //     in has_boolean_content=true (seeded by the
                //     unary `!`). +1.
                // m3: x && y → `&&` walker counts both → +2.
                // Sum: 1 + 1 + 2 = 4.
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn cpp_short_circuit_with_boolean_literal_operand() {
        // `a && true` reports 2 conditions: one for the identifier
        // operand, one for the `True` literal operand.
        check_metrics::<CppParser>(
            "bool f(bool a) { return a && true; }\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_empty_unit_zero() {
        // No code → A=B=C=0. Wires up the trait and exercises the
        // per-language compute reachability.
        check_metrics::<JavascriptParser>("", "empty.js", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn javascript_plain_and_compound_assignments_count() {
        // `let` / `var` declarations behave like TypeScript: only a
        // `const` initializer is suppressed. So `let x = 0` does count as
        // A=+1; only `const PI = 3.14` would be elided. Plain `x = 5`,
        // `x += 2`, `x = 7` all count → A = 4 total here.
        check_metrics::<JavascriptParser>(
            "function f() { let x = 0; x = 5; x += 2; x = 7; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 4);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_const_initializer_not_assignment() {
        // `const PI = 3.14` must NOT count as an assignment — its `=`
        // initialises a `const` binding. `let x = 1` and `var y = 2`
        // still count (matches the TS impl: only `const` suppresses).
        check_metrics::<JavascriptParser>(
            "function f() { const PI = 3.14; let x = 1; var y = 2; x = 9; }",
            "foo.js",
            |metric| {
                // `const PI` suppressed; `let x = 1`, `var y = 2`,
                // `x = 9` all count → A = 3.
                assert_eq!(metric.abc.assignments_sum(), 3);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_asi_const_does_not_suppress_later_assignments() {
        // The issue #1277 reproducer verbatim. JavaScript half of the
        // cluster documented at
        // `typescript_asi_const_does_not_suppress_later_assignments`.
        check_metrics::<JavascriptParser>(
            "function f() {
                const a = 1
                x = 2
                return x
            }",
            "foo.js",
            |metric| {
                // Pre-#1277 this reported 0; the semicolon-terminated
                // spelling reported 1.
                assert_eq!(metric.abc.assignments_sum(), 1);
            },
        );
    }

    #[test]
    fn javascript_nested_arrow_const_does_not_leak() {
        // The ASI leak beside a nested space: the arrow body opens its
        // own space, so `y = 1` was always counted there, but
        // `const f = () => …` has no `;`, so `h`'s sentinel stayed live
        // and suppressed `z = 2`. The stack was per-space state that
        // `Stats::merge` never carried, so this is the terminator defect
        // and not a second route.
        check_metrics::<JavascriptParser>(
            "function h() {
                const f = () => { y = 1 }
                z = 2
            }",
            "foo.js",
            |metric| {
                // `y = 1` and `z = 2`; the `const` initializer is
                // suppressed. Pre-#1277: 1.
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn javascript_non_declarator_equals_still_count() {
        // An `=` that does not belong to a `const` declarator is always
        // an assignment: a class `field_definition` initializer, a
        // default-parameter `assignment_pattern`, and a destructured
        // parameter's defaults, whose climb crosses the same pattern
        // layers as a `const` pattern's but ends at `formal_parameters`.
        // A predicate that accepted any pattern ancestor as a declarator
        // would zero the last three.
        check_metrics::<JavascriptParser>(
            "class K { f = 1; g(p = 2, {q = 3} = {}) { let d = 4 } }",
            "foo.js",
            |metric| {
                // `f = 1`, `p = 2`, `q = 3`, `= {}`, `let d = 4`.
                assert_eq!(metric.abc.assignments_sum(), 5);
            },
        );
    }

    #[test]
    fn javascript_const_initializer_value_assignments_still_count() {
        // An `=` inside a `const` initializer's *value* is an
        // `assignment_expression`: its climb reaches no pattern layer and
        // no declarator, so it counts. The pre-#1277 sentinel
        // blanket-suppressed every `=` between `const` and `;`, which is
        // the shape that moved the pdf.js corpus snapshots
        // (`const bbox = (this.data.rect = …)`).
        check_metrics::<JavascriptParser>(
            "function m(o, a, b) { const x = (o.p = 1); const y = a || (b = 2); }",
            "foo.js",
            |metric| {
                // `o.p = 1` and `b = 2`; the two `const` initializers are
                // suppressed. Pre-#1277: 0.
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn javascript_const_declarator_shapes_stay_suppressed() {
        // JavaScript half of
        // `typescript_const_declarator_shapes_stay_suppressed`.
        check_metrics::<JavascriptParser>(
            "function f(o, xs) {
                const a = 1, b = 2
                const {c = 5, d: {e = 6} = {}} = o
                const [g = 7, ...[h = 8]] = xs
                let i = 3
                var j = 4
                for (const x of xs) { k(x) }
            }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn javascript_increment_and_decrement_count_as_assignment() {
        // `x++` (post) and `--x` (pre) both update an lvalue and so
        // count as assignments. Combined with the `let x = 0`
        // initializer (which counts under the JS/TS rule — only `const`
        // suppresses), A = 3.
        check_metrics::<JavascriptParser>(
            "function f() { let x = 0; x++; --x; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 3);
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
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.conditions_sum(), 8);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_number_truthy_condition_counts() {
        // Regression for #772: JS treats every non-zero number as
        // truthy, so `while (5)` and `x && 5` should each count their
        // numeric literal as a Fitzpatrick unary condition. Pre-fix
        // `javascript_bool_terminal_kinds!()` listed `True` / `False`
        // but omitted `Number`, so the walker dropped every numeric-
        // truthy operand (mirrors the Lua `Number` fix).
        check_metrics::<JavascriptParser>(
            "function f(x) { while (5) {} return x && 5; }",
            "foo.js",
            |metric| {
                // `while (5)` → Number literal (+1). `x && 5` → both
                // operands count: identifier `x` (+1), Number `5` (+1).
                // Total: 3.
                assert_eq!(metric.abc.conditions_sum(), 3);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_number_truthy_condition_counts() {
        // Regression for #772: TS shares the JS truthy semantics. The
        // numeric *literal* `5` (kind `Number`) counts; the type-keyword
        // `number` (kind `Number2`, the `predefined_type`) must not —
        // see `typescript_bool_terminal_kinds!`.
        check_metrics::<TypescriptParser>(
            "function f(x: number) { while (5) {} return x && 5; }",
            "foo.ts",
            |metric| {
                // `while (5)` → +1; `x && 5` → `x` (+1) + `5` (+1).
                // Total: 3. The `: number` annotation contributes 0.
                assert_eq!(metric.abc.conditions_sum(), 3);
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
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_else_ternary_case_default_try_catch() {
        // `else`, `?` (ternary), `case`, `try`, `catch` all count.
        // `default` is the unconditional fallthrough → +0 (#469).
        // With the comparisons:
        //   - `a > 0` → 1
        //   - `else` opens an else_clause → 1
        //   - `?` ternary → 1
        //   - the ternary's bare-identifier condition `a` → 1 (#1102)
        //   - `case 1` → 1
        //   - `default` → 0 (fallthrough, #469)
        //   - `try` + `catch` → 2
        // Total C = 7.
        check_metrics::<JavascriptParser>(
            "function f(a) { if (a > 0) {} else {} let x = a ? 1 : 2; switch (x) { case 1: break; default: break; } try { } catch (e) { } }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 7);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Issue #1102, JS-family half. See
    // `cpp_ternary_operand_slots_count_as_unary_conditions` for the
    // rule; the two families were behind Java by the same three units.
    #[test]
    fn javascript_ternary_operand_slots_count_as_unary_conditions() {
        // `?` (1) + condition `a` (1) + `!b` (1) + `!c` (1) = 4.
        check_metrics::<JavascriptParser>(
            "function f() { x = a ? !b : !c; }",
            "foo.js",
            |metric| assert_eq!(metric.abc.conditions_sum(), 4),
        );
        // No-double-count pin: `?` (1) + `>` (1) = 2, unchanged by the
        // fix — the parenthesised condition unwraps to a
        // `binary_expression` (not a boolean terminal) and neither
        // branch is negated.
        check_metrics::<JavascriptParser>(
            "function f() { x = (a > 0) ? b : -b; }",
            "foo.js",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
        // Nested: two `?` tokens plus the two bare-identifier
        // conditions = 4.
        check_metrics::<JavascriptParser>(
            "function f() { x = a ? b ? c : d : e; }",
            "foo.js",
            |metric| assert_eq!(metric.abc.conditions_sum(), 4),
        );
        // A negated condition is the only input reaching the walker's
        // `else` fallback — see the C++ sibling for why. `?` (1) +
        // `!a` (1) = 2.
        check_metrics::<JavascriptParser>("function f() { x = !a ? b : c; }", "foo.js", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
        });
    }

    // TypeScript expands the same `ts_abc_compute!` arm from a separate
    // macro body than JavaScript's `js_abc_compute!`, so wiring one and
    // not the other is a live failure mode; TSX and Mozjs are clones of
    // these two.
    #[test]
    fn typescript_ternary_operand_slots_count_as_unary_conditions() {
        check_metrics::<TypescriptParser>(
            "function f() { x = a ? !b : !c; }",
            "foo.ts",
            |metric| assert_eq!(metric.abc.conditions_sum(), 4),
        );
        check_metrics::<TypescriptParser>(
            "function f() { x = (a > 0) ? b : -b; }",
            "foo.ts",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
    }

    // Issue #1276, JS-family half. See
    // `cpp_for_condition_slot_counts_unary_conditions` for the rule.
    // The JS grammar marks the `condition` field on both the expression
    // and the `;` closing it, so `child_by_field_name` is the only
    // addressing that lands on the expression for every header shape.
    #[test]
    fn javascript_for_condition_slot_counts_unary_conditions() {
        // Bare identifier: no operator token anywhere in the header.
        check_metrics::<JavascriptParser>("function f(a) { for (; a; ) {} }", "foo.js", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 1);
        });
        // Negation, through the `!` unwrap.
        check_metrics::<JavascriptParser>(
            "function f(a) { for (; !a; ) {} }",
            "foo.js",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
        );
        // Parentheses: counts only via the `ForStatement`
        // boolean-context seed #1276 found dead.
        check_metrics::<JavascriptParser>(
            "function f(a) { for (; (a); ) {} }",
            "foo.js",
            |metric| assert_eq!(metric.abc.conditions_sum(), 1),
        );
        // No-double-count pin: the `<` arm already counted this shape.
        // `let i = 0` and `i++` are the two assignments, which also
        // confirms the header parsed as the three-clause form.
        check_metrics::<JavascriptParser>(
            "function f(n) { for (let i = 0; i < n; i++) {} }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
        // Empty condition: the slot holds an `empty_statement`, which
        // is neither a terminal nor a wrapper, so it counts nothing.
        check_metrics::<JavascriptParser>(
            "function f() { for (;;) { break; } }",
            "foo.js",
            |metric| assert_eq!(metric.abc.conditions_sum(), 0),
        );
    }

    // TypeScript expands the `ForStatement` arm from `ts_abc_compute!`,
    // a separate macro body from JavaScript's `js_abc_compute!`, so
    // wiring one and not the other is a live failure mode; TSX and
    // Mozjs are the clones of those two. The expected values are
    // derived from the JavaScript run rather than hardcoded.
    #[test]
    fn js_family_for_condition_slot_agrees_with_javascript() {
        const BARE: &str = "function f(a) { for (; a; ) {} }\n";
        const EMPTY: &str = "function f() { for (;;) { break; } }\n";
        let conditions = abc_conditions;

        let bare = conditions(LANG::Javascript, BARE);
        // Non-degenerate: a zeroed reference makes the comparisons
        // below vacuous.
        assert_eq!(bare, 1, "JavaScript reference value for `for (; a; )`");
        assert_eq!(
            conditions(LANG::Javascript, EMPTY),
            0,
            "JavaScript `for (;;)`"
        );

        for lang in [LANG::Mozjs, LANG::Typescript, LANG::Tsx] {
            assert_eq!(conditions(lang, BARE), bare, "{lang:?} bare for-condition");
            assert_eq!(conditions(lang, EMPTY), 0, "{lang:?} empty for-condition");
        }
    }

    #[test]
    fn js_family_ternary_operand_slots_agree_with_javascript() {
        // `a ? !b : !c` is the one shape that tells the ternary walker
        // from the `for`-header walker: both read the `condition` field
        // and share the `(&Node, &mut f64)` signature, so a transposed
        // pair in a `ts_abc_compute!` / `js_abc_compute!` invocation
        // compiles, passes every condition-slot test, and drops only the
        // two branch operands. TypeScript alone pinned those before; the
        // other three expansions now do too.
        const SRC: &str = "function f(a, b, c) { x = a ? !b : !c; }\n";
        let javascript = abc_conditions(LANG::Javascript, SRC);
        // expected: 4 — the `?`, the `a` condition slot and both negated
        // branch operands; non-degenerate by construction.
        assert_eq!(
            javascript, 4,
            "JavaScript reference value for `a ? !b : !c`"
        );
        for lang in [LANG::Mozjs, LANG::Typescript, LANG::Tsx] {
            assert_eq!(
                abc_conditions(lang, SRC),
                javascript,
                "{lang:?} ternary operand slots"
            );
        }
    }

    #[test]
    fn javascript_instanceof_counts_condition() {
        // `x instanceof Foo` is a binary expression whose operator is
        // the `instanceof` keyword token → C = 1.
        check_metrics::<JavascriptParser>(
            "function f(x) { return x instanceof Foo; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_complex_function_abc() {
        // Mixed-shape regression. Verified by hand:
        // - assignments: `let x = 0` (a `let` initializer counts)
        //   + `x = 5`, `x += 2`, `x++`, `x = (a>b)?a:b`, `x = b`,
        //   `let p = ...` (likewise) → A = 7.
        // - branches: `f(a, b)` self-call + `new Bar()` → B = 2.
        // - conditions: `a == b`, `a > 0` → 2 inside the if header
        //   (`&&` is not counted directly). `else` (1) + `a > b`,
        //   `?` → 2 in the ternary. `a < b` → 1 in the else-if.
        //   `!x` → 1 from the Fitzpatrick Rule 9 walker on `||`
        //   (issue #403): the wrapped Identifier counts once.
        //   `case 1` → 1 in the switch; `default` → 0 (fallthrough,
        //   #469). Total C = 8.
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
                assert_eq!(metric.abc.assignments_sum(), 7);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 8);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn mozjs_asi_const_does_not_suppress_later_assignments() {
        // Mozjs half of the #1277 cluster; the fork carries its own
        // kind-id numbering, so it needs its own fixture. See
        // `typescript_asi_const_does_not_suppress_later_assignments`.
        check_metrics::<MozjsParser>(
            "function f() {
                const a = 1
                x = 2
                y = 3
            }",
            "foo.jsm",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2);
            },
        );
    }

    #[test]
    fn mozjs_const_declarator_shapes_stay_suppressed() {
        // Mozjs half of
        // `javascript_const_declarator_shapes_stay_suppressed`.
        check_metrics::<MozjsParser>(
            "function f(o, xs) {
                const a = 1, b = 2
                const {c = 5, d: {e = 6} = {}} = o
                const [g = 7, ...[h = 8]] = xs
                let i = 3
                var j = 4
                for (const x of xs) { k(x) }
            }",
            "foo.jsm",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 2);
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
                assert_eq!(metric.abc.assignments_sum(), 7);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 8);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ----- JS / TS / Tsx / Mozjs Phase-2B condition slots -----

    #[test]
    fn javascript_await_expression_condition_counts() {
        // Regression for findings.md round-2 #2 (JS):
        // `if (await ready()) {}` parses with `await_expression` as
        // the condition node inside the `parenthesized_expression`.
        // `javascript_inspect_container` unwraps the paren but the
        // await child was not in the terminal-bool set, so the
        // walker broke without counting. Mirrors C# (lesson 19).
        check_metrics::<JavascriptParser>(
            "async function ready() { return true; }\n\
             async function f() { if (await ready()) { } }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_member_expression_condition_counts() {
        // Regression for findings.md #3 (JS-family): tree-sitter-
        // javascript emits `member_expression` under three kind_ids
        // (191 primary, 208, 228 — `MemberExpression2/3`) depending
        // on the production rule path. The verifier in this audit
        // confirmed runtime kind for `o.x` is 208. Pre-fix the
        // shared `js_family_bool_terminal_kinds!()` macro listed
        // only the primary, so every `if (o.x) {}` / `o.x && o.y`
        // condition silently reported zero. The per-language macro
        // now includes all three aliases (lesson #2).
        check_metrics::<JavascriptParser>(
            "function f(o) {\n\
             \x20   if (o.x) {}                  // +1c\n\
             \x20   return o.x && o.y;           // +2c (walker on &&)\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 3);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_if_boolean_literal_condition() {
        check_metrics::<JavascriptParser>(
            "function f() {\n\
             \x20   if (true) {}                 // +1c\n\
             \x20   if (!false) {}               // +1c\n\
             \x20   while (true) {}              // +1c\n\
             \x20   do {} while (false);         // +1c\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_methods_arguments_with_conditions() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) {\n\
             \x20   m(a, b);                     // +1b\n\
             \x20   m(!a, !b);                   // +1b +2c\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_return_with_conditions() {
        check_metrics::<JavascriptParser>(
            "function m1(z) { return !(z >= 0); }\n\
             function m2(x) { return (((!x))); }\n\
             function m3(x, y) { return x && y; }\n",
            "foo.js",
            |metric| {
                // m1: 1 (`>=`). m2: 1 (walker unwraps to `x`).
                // m3: 2 (`&&` walker counts both terminals).
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_if_boolean_literal_condition() {
        check_metrics::<TypescriptParser>(
            "function f() {\n\
             \x20   if (true) {}\n\
             \x20   if (!false) {}\n\
             \x20   while (true) {}\n\
             \x20   do {} while (false);\n\
             }\n",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_methods_arguments_with_conditions() {
        check_metrics::<TypescriptParser>(
            "function f(a: boolean, b: boolean) {\n\
             \x20   m(a, b);\n\
             \x20   m(!a, !b);\n\
             }\n",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_return_with_conditions() {
        check_metrics::<TypescriptParser>(
            "function m1(z: number): boolean { return !(z >= 0); }\n\
             function m2(x: boolean): boolean { return (((!x))); }\n\
             function m3(x: boolean, y: boolean): boolean { return x && y; }\n",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_if_boolean_literal_condition() {
        check_metrics::<TsxParser>(
            "function f() {\n\
             \x20   if (true) {}\n\
             \x20   if (!false) {}\n\
             \x20   while (true) {}\n\
             \x20   do {} while (false);\n\
             }\n",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_methods_arguments_with_conditions() {
        check_metrics::<TsxParser>(
            "function f(a: boolean, b: boolean) {\n\
             \x20   m(a, b);\n\
             \x20   m(!a, !b);\n\
             }\n",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_return_with_conditions() {
        check_metrics::<TsxParser>(
            "function m1(z: number): boolean { return !(z >= 0); }\n\
             function m2(x: boolean): boolean { return (((!x))); }\n\
             function m3(x: boolean, y: boolean): boolean { return x && y; }\n",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn mozjs_if_boolean_literal_condition() {
        check_metrics::<MozjsParser>(
            "function f() {\n\
             \x20   if (true) {}\n\
             \x20   if (!false) {}\n\
             \x20   while (true) {}\n\
             \x20   do {} while (false);\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn mozjs_methods_arguments_with_conditions() {
        check_metrics::<MozjsParser>(
            "function f(a, b) {\n\
             \x20   m(a, b);\n\
             \x20   m(!a, !b);\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn mozjs_return_with_conditions() {
        check_metrics::<MozjsParser>(
            "function m1(z) { return !(z >= 0); }\n\
             function m2(x) { return (((!x))); }\n\
             function m3(x, y) { return x && y; }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ----- JS / TS / Tsx / Mozjs unary-conditional walker -----

    #[test]
    fn javascript_if_multiple_conditions() {
        check_metrics::<JavascriptParser>(
            "function f(a, b, c, d) {\n\
             \x20   if (a || b || c || d) {}        // +4c\n\
             \x20   if (a && b && c) {}             // +3c\n\
             \x20   if (!a && !b) {}                // +2c\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_while_and_do_while_conditions() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) {\n\
             \x20   while (a || b) {}              // +2c\n\
             \x20   do {} while (a && !b);         // +2c\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn javascript_short_circuit_with_boolean_literal_operand() {
        check_metrics::<JavascriptParser>(
            "function f(a) { return a && true; }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_if_multiple_conditions() {
        check_metrics::<TypescriptParser>(
            "function f(a: boolean, b: boolean, c: boolean, d: boolean) {\n\
             \x20   if (a || b || c || d) {}        // +4c\n\
             \x20   if (a && b && c) {}             // +3c\n\
             \x20   if (!a && !b) {}                // +2c\n\
             }\n",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_while_and_do_while_conditions() {
        check_metrics::<TypescriptParser>(
            "function f(a: boolean, b: boolean) {\n\
             \x20   while (a || b) {}              // +2c\n\
             \x20   do {} while (a && !b);         // +2c\n\
             }\n",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn typescript_short_circuit_with_boolean_literal_operand() {
        check_metrics::<TypescriptParser>(
            "function f(a: boolean): boolean { return a && true; }\n",
            "foo.ts",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_if_multiple_conditions() {
        check_metrics::<TsxParser>(
            "function f(a: boolean, b: boolean, c: boolean, d: boolean) {\n\
             \x20   if (a || b || c || d) {}        // +4c\n\
             \x20   if (a && b && c) {}             // +3c\n\
             \x20   if (!a && !b) {}                // +2c\n\
             }\n",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_while_and_do_while_conditions() {
        check_metrics::<TsxParser>(
            "function f(a: boolean, b: boolean) {\n\
             \x20   while (a || b) {}              // +2c\n\
             \x20   do {} while (a && !b);         // +2c\n\
             }\n",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tsx_short_circuit_with_boolean_literal_operand() {
        check_metrics::<TsxParser>(
            "function f(a: boolean): boolean { return a && true; }\n",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn mozjs_if_multiple_conditions() {
        check_metrics::<MozjsParser>(
            "function f(a, b, c, d) {\n\
             \x20   if (a || b || c || d) {}        // +4c\n\
             \x20   if (a && b && c) {}             // +3c\n\
             \x20   if (!a && !b) {}                // +2c\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn mozjs_while_and_do_while_conditions() {
        check_metrics::<MozjsParser>(
            "function f(a, b) {\n\
             \x20   while (a || b) {}              // +2c\n\
             \x20   do {} while (a && !b);         // +2c\n\
             }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn mozjs_short_circuit_with_boolean_literal_operand() {
        check_metrics::<MozjsParser>(
            "function f(a) { return a && true; }\n",
            "foo.js",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
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
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 5);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 3);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 3);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 14);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 14);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_short_circuit_not_counted_directly_ternary_counts() {
        // `&&`, `||`, `//`, low-precedence `and`, `or`, `xor` are
        // NOT counted as conditions on their own (Fitzpatrick Rule
        // 5; #395) — instead each operand is counted as a unary
        // conditional by the walker (Rule 9; #403). At the pinned
        // tree-sitter-perl grammar version, only the four
        // punctuation forms plus one keyword form parse under a
        // `binary_expression` parent that triggers the walker; the
        // other two keyword forms parse under a distinct grammar
        // node and contribute zero. Net: 4 walker-firing lines × 2
        // scalar-variable operands + 1 ternary node + 1 for the
        // ternary's bare `$a` condition operand (#1102) = 10. The
        // exact mix of "which two keyword forms are silent" is
        // grammar-version-dependent; a future grammar bump that
        // normalises the keyword forms' parent kind will shift this
        // count to 14. See follow-up note above the test name.
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
                assert_eq!(metric.abc.assignments_sum(), 7);
                assert_eq!(metric.abc.branches_sum(), 0);
                // 4 walker-triggered lines × 2 operands + 1 ternary
                // node + 1 for its bare `$a` condition operand = 10.
                // The two remaining low-precedence keyword forms (one
                // of `and`/`or`/`xor`) fall under a
                // non-binary_expression parent in this grammar
                // version and contribute zero via the walker.
                assert_eq!(metric.abc.conditions_sum(), 10);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // Issue #1102, Perl half. See
    // `cpp_ternary_operand_slots_count_as_unary_conditions` for the
    // rule. Like PHP, Perl's ABC dispatcher has no `?`-token arm — the
    // grammar does emit the token, but the `ternary_expression` node is
    // what carries the tally's +1. tree-sitter-perl names the branch
    // fields `true` / `false` rather than the C-family `consequence` /
    // `alternative`, so a copied C-family gate would match nothing.
    #[test]
    fn perl_ternary_operand_slots_count_as_unary_conditions() {
        // ternary (1) + condition `$a` (1) + `!$b` (1) + `!$c` (1) = 4.
        check_metrics::<PerlParser>("sub f { my $x = $a ? !$b : !$c; }", "foo.pl", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 4);
        });
        // No-double-count pin: ternary (1) + `>` (1) = 2, unchanged by
        // the fix.
        check_metrics::<PerlParser>(
            "sub f { my $x = ($a > 0) ? $b : -$b; }",
            "foo.pl",
            |metric| assert_eq!(metric.abc.conditions_sum(), 2),
        );
        // A negated *condition* takes the walker's `else` fallback —
        // `!$a` is neither a boolean terminal nor a paren wrapper, so
        // only `perl_inspect_container` can classify it. Delete the
        // fallback and this reads 1. ternary (1) + `!$a` (1) = 2.
        check_metrics::<PerlParser>("sub f { my $x = !$a ? $b : $c; }", "foo.pl", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
        });
        // Nested: two ternary nodes plus the two bare-variable
        // conditions = 4.
        check_metrics::<PerlParser>(
            "sub f { my $x = $a ? ($b ? $c : $d) : $e; }",
            "foo.pl",
            |metric| assert_eq!(metric.abc.conditions_sum(), 4),
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
                assert_eq!(metric.abc.assignments_sum(), 4);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 4);
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
                assert_eq!(metric.abc.assignments_sum(), 3);
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.assignments_sum(), 3);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 3);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_if_multiple_conditions() {
        // Fitzpatrick Rule 9 walker (issue #403): each operand of a
        // `&&` / `||` / `//` / `and` / `or` / `xor` chain is one
        // condition. ScalarVariable operands ($a, $b, …) qualify as
        // terminal-bool kinds for the walker.
        check_metrics::<PerlParser>(
            "sub f {\n\
                 my ($a, $b, $c, $d) = @_;\n\
                 if ($a || $b || $c || $d) { return 1; }    # +4c\n\
                 if ($a && $b && $c) { return 2; }          # +3c\n\
                 if (!$a && !$b) { return 3; }              # +2c\n\
                 return 0;\n\
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_while_and_until_conditions() {
        // Perl has no `do { ... } while(cond);` shape in this grammar
        // — `while` and `until` are the loop forms with a condition
        // slot. The walker fires on each `&&` / `||` token inside
        // those headers.
        check_metrics::<PerlParser>(
            "sub f {\n\
                 my ($a, $b) = @_;\n\
                 while ($a || $b) { last; }            # +2c\n\
                 until ($a && !$b) { last; }           # +2c\n\
             }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_for_header_condition_slot_counts_unary_conditions() {
        // The Perl half of #1276. The C-style `for` header's condition
        // slot is a Rule 9 unary condition like the `if` slot: a bare
        // `$ok`, a negated `!$ok` and a parenthesised `($ok)` each count
        // one, an empty header and a `foreach` count zero, and a
        // comparison-shaped `$i < $n` stays at the one the `<` arm
        // already counts. Every C-style row is the three-clause spelling
        // because tree-sitter-perl does not parse an empty initializer.
        let cases = [
            ("for (my $i = 0; $ok; $i++) { }", 1),
            ("for (my $i = 0; !$ok; $i++) { }", 1),
            ("for (my $i = 0; ($ok); $i++) { }", 1),
            ("for (my $i = 0; $i < $n; $i++) { }", 1),
            ("for (my $i = 0; $ok && $j; $i++) { }", 2),
            ("for (;;) { last; }", 0),
            ("for my $x (@l) { }", 0),
        ];
        let mut ran = 0;
        for (body, expected) in cases {
            let src = format!("sub f {{ {body} }}\n");
            assert_eq!(abc_conditions(LANG::Perl, &src), expected, "`{body}`");
            ran += 1;
        }
        assert_eq!(ran, cases.len());
        assert!(cases.iter().any(|&(_, n)| n == 0));
        assert!(cases.iter().any(|&(_, n)| n == 2));

        // The slot agrees with the `if` slot for every shape, which is
        // the property the fix restores; the reference is non-degenerate
        // by the table above.
        for condition in ["$ok", "!$ok", "($ok)", "$ok && $j"] {
            let in_for = format!("sub f {{ for (my $i = 0; {condition}; $i++) {{ }} }}\n");
            let in_if = format!("sub f {{ if ({condition}) {{ }} }}\n");
            assert_eq!(
                abc_conditions(LANG::Perl, &in_for),
                abc_conditions(LANG::Perl, &in_if),
                "`{condition}` must score alike in a `for` header and an `if`",
            );
        }
    }

    #[test]
    fn perl_short_circuit_counts_scalar_variable_operands() {
        // `$a && $b` reports 2 conditions — one walker count per
        // `ScalarVariable` operand. Renamed from the cross-language
        // `_with_boolean_literal_operand` convention because Perl has
        // no readily-grammar-exposed boolean literal in an `&&`
        // operand slot at the pinned grammar version (the `Boolean`
        // kind only fires on the `boolean` pragma's named constants,
        // not bareword `1` / `0`). Two scalar variables are the
        // grammar-stable terminal-set witness for Perl.
        check_metrics::<PerlParser>(
            "sub f { my ($a) = @_; return $a && $b; }\n",
            "foo.pl",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_array_in_binary_operand_descends_to_scalar_context_value() {
        // Regression test for the code-review findings on the
        // Phase-2B Perl walker:
        //   - Pre-fix-A: `perl_inspect_container` descended `Array`
        //     via `node.child(1)` — the FIRST element — wrongly
        //     attributing `$x` for `($x, $y)` (semantically `$y`
        //     is the scalar-context value).
        //   - Fix-A (the `array_is_paren` guard, 5db8078): dropped
        //     Array-as-paren entirely in `BinaryExpression` operand
        //     contexts to avoid the wrong attribution — but
        //     regressed `$a || ($x)` (single paren-grouped operand)
        //     to C=1 instead of 2.
        //   - Fix-B (this change): keeps Array-as-paren unconditional
        //     but descends via the LAST named child. `$a || ($x)`
        //     reaches `$x` (count both operands → 2);
        //     `$a || ($x, $y)` reaches `$y` (count `$a` + `$y` →
        //     still 2, matching Fitzpatrick Rule 7 "one per
        //     operand"); `if ($a)` still reaches `$a` (single-
        //     element grouping → 1).
        check_metrics::<PerlParser>(
            "sub f { my ($a, $x, $y) = @_;\n\
             \x20   my $r = $a || ($x, $y);    # +2c: $a + last-named $y\n\
             \x20   my $s = $a || ($x);        # +2c: $a + only-named $x\n\
             \x20   $r + $s;\n\
             }\n",
            "foo.pl",
            |metric| {
                // 2 + 2 = 4 unary conditions from the two `||`s.
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_if_scalar_variable_condition() {
        // Renamed from the cross-language
        // `_if_boolean_literal_condition` convention because
        // Perl has no readily-grammar-exposed boolean literal in
        // an `if (cond)` slot at the pinned grammar version:
        // tree-sitter-perl's `Boolean` kind only fires for the
        // `boolean` pragma's named constants (not bareword `1` /
        // `0`, which surface as `Integer` / not in the
        // terminal-bool set). A scalar-variable condition is the
        // grammar-stable witness — `if ($a)` reaches
        // `scalar_variable` via the `Array` paren unwrap.
        check_metrics::<PerlParser>(
            "sub f { my ($a) = @_; if ($a) { return 1; } }\n",
            "foo.pl",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 1);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_methods_arguments_with_conditions() {
        // `call(!$a, !$b)` — argument list walker counts each
        // unary-conditional argument once. Cannot use `m(...)` as
        // the function name — tree-sitter-perl parses `m(...)` as
        // the regex-match operator, not a function call.
        check_metrics::<PerlParser>(
            "sub f { my ($a, $b) = @_; call($a, $b); call(!$a, !$b); }\n",
            "foo.pl",
            |metric| {
                // Two calls × 1 branch each = 2 branches.
                // `call(!$a, !$b)` contributes 2 walker conditions
                // (one per `!`-wrapped scalar-variable argument);
                // `call($a, $b)` contributes 0 (bare-args don't
                // count via the Arguments walker — list_kind !=
                // BinaryExpression).
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn perl_return_with_conditions() {
        // `return !$a` reports 1 condition via the walker (unary
        // unwrap to scalar-variable terminal). `return $a` reports
        // 0 (no paren / unary wrap, has_boolean_content stays
        // false from ReturnExpression parent).
        check_metrics::<PerlParser>(
            "sub m1 { my ($z) = @_; return !($z); }\n\
             sub m2 { my ($x) = @_; return (((!$x))); }\n\
             sub m3 { my ($x, $y) = @_; return $x && $y; }\n",
            "foo.pl",
            |metric| {
                // m1: !($z) → walker on `!` unwraps paren to $z (1).
                // m2: (((!$x))) → walker unwraps three parens + one
                //     unary to $x (1).
                // m3: $x && $y → walker on `&&` counts both (2).
                // Sum: 4.
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ---------- Lua ABC tests ----------

    #[test]
    fn lua_empty_unit_zero() {
        check_metrics::<LuaParser>("", "empty.lua", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 4);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 5);
                assert_eq!(metric.abc.conditions_sum(), 0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_comparisons_count_logical_ops_do_not() {
        // Each comparison token contributes one condition; `and` /
        // `or` are NOT counted on their own (Fitzpatrick Rule 5;
        // #395) — instead each operand is counted as a unary
        // conditional by the walker (Rule 9; #403). The two
        // `a and b` / `a or b` lines add 2 walker conditions each.
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
                assert_eq!(metric.abc.assignments_sum(), 8);
                assert_eq!(metric.abc.branches_sum(), 0);
                // 6 comparisons (+6) + 2 logical lines × 2 walker
                // operands (+4) = 10.
                assert_eq!(metric.abc.conditions_sum(), 10);
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
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 4);
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
                assert_eq!(metric.abc.assignments_sum(), 2);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_if_multiple_conditions() {
        // Fitzpatrick Rule 9 walker (issue #403). Lua's `and` / `or`
        // are keyword tokens inside a `binary_expression`.
        check_metrics::<LuaParser>(
            "function f(a, b, c, d)\n\
                 if a or b or c or d then return 1 end       -- +4c\n\
                 if a and b and c then return 2 end          -- +3c\n\
                 if not a and not b then return 3 end        -- +2c\n\
                 return 0\n\
             end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_while_conditions() {
        // Lua has no `do { ... } while(cond);` — `while cond do …
        // end` and `repeat … until cond` are the loop forms.
        check_metrics::<LuaParser>(
            "function f(a, b)\n\
                 while a or b do break end                   -- +2c\n\
                 repeat break until a and not b              -- +2c\n\
             end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_short_circuit_with_boolean_literal_operand() {
        // `a and true` reports 2 conditions: one Identifier, one
        // True keyword literal.
        check_metrics::<LuaParser>("function f(a) return a and true end", "foo.lua", |metric| {
            assert_eq!(metric.abc.conditions_sum(), 2);
            insta::assert_json_snapshot!(metric.abc);
        });
    }

    #[test]
    fn lua_number_truthy_condition_counts() {
        // Regression for findings.md #2: Lua treats every non-nil,
        // non-false value as truthy, so `if 1 then ... end` and
        // `return a and 2` should each count their numeric literal
        // as a Fitzpatrick Rule 6 / 7 unary condition. Pre-fix,
        // `lua_bool_terminal_kinds!()` listed `True` / `False` /
        // `Nil` but omitted `Number`, so the walker dropped every
        // numeric-truthy operand. The walker comment at the top of
        // `lua_inspect_container` already promised numbers were
        // terminal-bool kinds; this commit closes the gap.
        check_metrics::<LuaParser>(
            "function f(a)\n\
             \x20   if 1 then return 1 end\n\
             \x20   return a and 2\n\
             end",
            "foo.lua",
            |metric| {
                // `if 1 then` → walker counts the Number literal (+1).
                // `a and 2` → `and` walker counts both operands:
                //   identifier `a` (+1), Number `2` (+1).
                // Total: 3.
                assert_eq!(metric.abc.conditions_sum(), 3);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_if_boolean_literal_condition() {
        check_metrics::<LuaParser>(
            "function f()\n\
                 if true then end                  -- +1c\n\
                 if not false then end             -- +1c\n\
                 while true do break end           -- +1c\n\
                 repeat break until false          -- +1c\n\
             end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_methods_arguments_with_conditions() {
        // `m(not a, not b)` — argument list walker counts each
        // unary-conditional argument once. Bare-identifier args
        // (`m(a, b)`) do not count (list_kind != BinaryExpression).
        check_metrics::<LuaParser>(
            "function f(a, b) m(a, b); m(not a, not b) end",
            "foo.lua",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn lua_return_with_conditions() {
        // `return not (z >= 0)` → walker on `not` unwraps the paren
        // chain and reaches the inner BinaryExpression; the inner
        // `>=` comparison is the actual Fitzpatrick condition.
        check_metrics::<LuaParser>(
            "function m1(z) return not (z >= 0) end\n\
             function m2(x) return (((not x))) end\n\
             function m3(x, y) return x and y end",
            "foo.lua",
            |metric| {
                // m1: `>=` (1). `not` wraps a paren'd
                //     BinaryExpression — Lua's lua_inspect_container
                //     reaches the inner BinaryExpression and stops,
                //     no walker count. +1.
                // m2: ReturnStatement → iterate expression_list →
                //     inspect_container on the outermost paren →
                //     unwraps to `x` in has_boolean_content-true
                //     (seeded by the `not`). +1.
                // m3: x and y → `and` walker counts both → +2.
                // Sum: 4.
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    // ---------- Tcl ABC tests ----------

    #[test]
    fn tcl_empty_unit_zero() {
        check_metrics::<TclParser>("", "empty.tcl", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 0);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 3);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 4);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_computed_command_name_is_not_an_assignment() {
        // A command whose leading word is computed (`$cmd args`) names no
        // builtin the parser can resolve, so it stays a branch. Pins the
        // field-addressed, `simple_word`-gated read the classifier shares
        // with the Cognitive / Cyclomatic detectors (grammar-dispatch §3):
        // the earlier `child(0)` byte-slice addressed the slot by position
        // and compared whatever literal text sat there, which agrees with
        // the gated read on today's grammar only because no mutator name is
        // spelled with a leading `$`. A grammar that moved the name out of
        // `child(0)`, or a mutator list that grew a computed-looking entry,
        // would diverge — this fixture is where that surfaces.
        check_metrics::<TclParser>(
            "proc f {cmd x} {\n\
                 $cmd $x\n\
                 incr x\n\
             }",
            "foo.tcl",
            |metric| {
                // `incr x` is the only assignment; `$cmd $x` is a branch.
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 0);
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
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 3);
                assert_eq!(metric.abc.conditions_sum(), 0);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_comparisons_count_logical_ops_do_not() {
        // `expr` predicates expose comparison / logical tokens at
        // the leaf level. Each comparison token contributes one
        // condition; `&&` and `||` are NOT counted on their own
        // (Fitzpatrick Rule 5; #395) — instead each operand is
        // counted as a unary conditional by the walker (Rule 9;
        // #403). The two logical lines add 2 walker conditions
        // each (variable-substitution operands).
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
                // 10 `set` assignments.
                assert_eq!(metric.abc.assignments_sum(), 10);
                assert_eq!(metric.abc.branches_sum(), 0);
                // 8 comparisons (+8) + 2 logical lines × 2 walker
                // operands (+4) = 12.
                assert_eq!(metric.abc.conditions_sum(), 12);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_ternary_counts_condition() {
        // The `ternary_expr` node is one condition and its condition
        // slot `$a` — a bare truthy test — is another, matching C++'s
        // `int r = a ? b : c;` (also 2). The two branch operands are
        // unnegated and so contribute nothing (#1180).
        check_metrics::<TclParser>(
            "proc f {a b c} {\n\
                 set r [expr {$a ? $b : $c}]\n\
             }",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.branches_sum(), 0);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    /// The Tcl half of the ternary slot-location guard (#1180).
    ///
    /// `ternary_expr` exposes no grammar fields, so the slots are found
    /// relative to the `?` and `:` tokens. A *parenthesised* condition is
    /// the input that discriminates that from a fixed-index reading:
    /// `_expr` inlines `( … )` as anonymous children of `ternary_expr`,
    /// so `($a) ? !$b : !$c` shifts every operand right by one and
    /// `child(0)` / `child(2)` / `child(4)` land on `(`, `)` and `?`.
    /// Without this case the whole fixed-index revert passes.
    #[test]
    fn tcl_parenthesised_ternary_condition_matches_the_bare_form() {
        let conditions = |source: &str| {
            crate::test_support::metrics_verbatim(
                crate::LANG::Tcl,
                source.as_bytes(),
                crate::MetricsOptions::default(),
            )
            .abc
            .conditions_sum()
        };
        let bare = conditions("proc f {a b c} {\n set r [expr {$a ? !$b : !$c}]\n}");
        assert_eq!(bare, 4, "the bare form is the documented reference value");
        assert_eq!(
            conditions("proc f {a b c} {\n set r [expr {($a) ? !$b : !$c}]\n}"),
            bare,
            "parenthesising the condition must not change the count"
        );
    }

    /// A parenthesised operand under `!` counts the same as a bare one.
    ///
    /// `_expr` inlines `( … )` as anonymous children, so `!($a)` puts
    /// `(` where a positional read expects the operand. The walker's
    /// negation branch kept a fixed `child(1)` through the first draft of
    /// #1180 and scored these 0 while the unparenthesised forms scored 1
    /// — an inconsistency the fix itself introduced, since before it
    /// neither form counted. Found in review, not by the tests: the
    /// parenthesised fixtures added with #1180 covered the ternary
    /// *condition* slot only.
    #[test]
    fn tcl_parenthesised_negated_operands_match_the_bare_form() {
        let conditions = |source: &str| {
            crate::test_support::metrics_verbatim(
                crate::LANG::Tcl,
                source.as_bytes(),
                crate::MetricsOptions::default(),
            )
            .abc
            .conditions_sum()
        };
        for (bare, parenthesised) in [
            (
                "proc f {a} {\n if {!$a} { puts x }\n}",
                "proc f {a} {\n if {!($a)} { puts x }\n}",
            ),
            (
                "proc f {a b} {\n if {$a && !$b} { puts x }\n}",
                "proc f {a b} {\n if {$a && !($b)} { puts x }\n}",
            ),
            (
                "proc f {a b c} {\n set r [expr {$a ? !$b : !$c}]\n}",
                "proc f {a b c} {\n set r [expr {$a ? !($b) : !$c}]\n}",
            ),
        ] {
            let want = conditions(bare);
            assert!(want > 0, "the bare form must count something: {bare}");
            assert_eq!(
                conditions(parenthesised),
                want,
                "parenthesising the negated operand changed the count\n  bare:  {bare}\n  paren: {parenthesised}"
            );
        }
    }

    #[test]
    fn irules_abc_parenthesised_negated_operands_match_the_bare_form() {
        let conditions = |source: &str| {
            crate::test_support::metrics_verbatim(
                crate::LANG::Irules,
                source.as_bytes(),
                crate::MetricsOptions::default(),
            )
            .abc
            .conditions_sum()
        };
        let want = conditions("when X {\n    if { !$a } { log local0. hi }\n}\n");
        assert_eq!(want, 1);
        assert_eq!(
            conditions("when X {\n    if { !($a) } { log local0. hi }\n}\n"),
            want
        );
    }

    #[test]
    fn tcl_bare_truthy_and_negated_predicates_count_one_condition() {
        // The headline #1180 fix, on the Tcl side: both were 0 before.
        let conditions = |source: &str| {
            crate::test_support::metrics_verbatim(
                crate::LANG::Tcl,
                source.as_bytes(),
                crate::MetricsOptions::default(),
            )
            .abc
            .conditions_sum()
        };
        assert_eq!(conditions("proc f {a} {\n if {$a} { puts x }\n}"), 1);
        assert_eq!(conditions("proc f {a} {\n if {!$a} { puts x }\n}"), 1);
        assert_eq!(conditions("proc f {a} {\n while {$a} { puts x }\n}"), 1);
        assert_eq!(conditions("proc f {a} {\n while {!$a} { puts x }\n}"), 1);
    }

    #[test]
    fn tcl_bare_truthy_elseif_predicate_counts_one_condition() {
        // The `Tcl::Elseif` arm routes its predicate through
        // `tcl_condition_expr` exactly as `If` / `While` do (#1180), but
        // a comparison predicate is counted by its leaf operator, so it
        // cannot tell the routing from its absence. Only a bare truthy
        // predicate can: `$b` scores through the routed walker or not at
        // all.
        let conditions = |source: &str| {
            crate::test_support::metrics_verbatim(
                crate::LANG::Tcl,
                source.as_bytes(),
                crate::MetricsOptions::default(),
            )
            .abc
            .conditions_sum()
        };
        // `$a` truthy (1) + `elseif` clause (1) + `$b` truthy (1).
        assert_eq!(
            conditions("proc f {a b} {\n if {$a} { puts x } elseif {$b} { puts y }\n}"),
            3
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
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 3);
                assert_eq!(metric.abc.conditions_sum(), 4);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_if_multiple_conditions() {
        // Fitzpatrick Rule 9 walker (issue #403). Tcl's `expr` slot
        // exposes `&&` / `||` operands as variable substitutions
        // (`$a`, `$b`, …) inside a `binop_expr`.
        check_metrics::<TclParser>(
            "proc f {a b c d} {\n\
                 if {[expr {$a || $b || $c || $d}]} { return 1 }    \n\
                 if {[expr {$a && $b && $c}]} { return 2 }          \n\
                 return 0\n\
             }",
            "foo.tcl",
            |metric| {
                // The two chains feed the walker: 4 + 3 = 7. Each `if`
                // predicate is additionally a bare truthy test of a
                // command substitution — `{[expr {…}]}` is structurally
                // `if {[somecmd]}`, which counts 1 exactly as `if {$a}`
                // does — so 7 + 2 = 9 (#1180). Written without the
                // redundant `[expr …]` wrapper, `if {$a || $b}` scores
                // 2, matching C++'s `if (a || b)`.
                assert_eq!(metric.abc.conditions_sum(), 9);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_while_conditions() {
        // Tcl has no `do { ... } while(cond);` — `while {…} {…}` is
        // the standard loop. The walker fires on `&&` / `||` tokens
        // inside the `expr` predicate.
        check_metrics::<TclParser>(
            "proc f {a b} {\n\
                 while {[expr {$a || $b}]} { break }    \n\
                 while {[expr {$a && $b}]} { break }    \n\
             }",
            "foo.tcl",
            |metric| {
                // 2 + 2 from the chains, plus one bare truthy test per
                // `while` predicate — see `tcl_if_multiple_conditions`
                // (#1180).
                assert_eq!(metric.abc.conditions_sum(), 6);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    #[test]
    fn tcl_short_circuit_with_boolean_literal_operand() {
        // `$a && 1` reports 2 conditions: a VariableSubstitution
        // operand plus a Number-literal operand. Confirms `Number`
        // is in the walker terminal set. `true` / `false` Tcl
        // keywords are not literal tokens in tree-sitter-tcl —
        // they're emitted as the operator-context word, which is
        // captured separately by the `Tcl::Boolean` kind for
        // dedicated `expr {true}` predicates but not as a `&&`
        // operand at this iteration; using a numeric literal keeps
        // the assertion grammar-stable.
        check_metrics::<TclParser>(
            "proc f {a} { return [expr {$a && 1}] }\n",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
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
                assert_eq!(metric.abc.assignments_sum(), 4);
                assert_eq!(metric.abc.branches_sum(), 5);
                assert_eq!(metric.abc.conditions_sum(), 2);
                insta::assert_json_snapshot!(metric.abc);
            },
        );
    }

    /// The dedicated `set name value` production counts as one assignment.
    #[test]
    fn irules_abc_set_assignment() {
        check_metrics::<IrulesParser>("when X {\n    set x 1\n}\n", "foo.irule", |metric| {
            assert_eq!(metric.abc.assignments_sum(), 1);
            assert_eq!(metric.abc.branches_sum(), 0);
            assert_eq!(metric.abc.conditions_sum(), 0);
        });
    }

    /// Mutator commands (`incr` / `append` / `lappend`) count as
    /// assignments, not branches — iRules has no assignment operators, so
    /// mutation is always a command invocation.
    #[test]
    fn irules_abc_mutator_commands() {
        check_metrics::<IrulesParser>(
            "when X {\n    incr x\n    append s \"y\"\n    lappend l 1\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 3);
                assert_eq!(metric.abc.branches_sum(), 0);
            },
        );
    }

    /// Generic (non-mutator) commands count as branches.
    #[test]
    fn irules_abc_branch_commands() {
        check_metrics::<IrulesParser>(
            "when X {\n    log local0. hi\n    pool p1\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 0);
                assert_eq!(metric.abc.branches_sum(), 2);
                assert_eq!(metric.abc.conditions_sum(), 0);
            },
        );
    }

    /// A numeric comparison (`==`) is one condition; the `log` inside the
    /// `if` body is one branch.
    #[test]
    fn irules_abc_comparison_condition() {
        check_metrics::<IrulesParser>(
            "when X {\n    if { $a == 1 } { log local0. hi }\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
            },
        );
    }

    /// A word-form string comparator (`contains`) is a condition just like
    /// `==` — iRules-specific (Tcl has only `eq`/`ne`/`in`/`ni`). If
    /// `contains` were dropped from the condition set this would report 0.
    #[test]
    fn irules_abc_string_op_condition() {
        check_metrics::<IrulesParser>(
            "when X {\n    if { $a contains \"x\" } { log local0. hi }\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
            },
        );
    }

    /// Each `elseif` / `else` clause is one condition; the three `set`s are
    /// assignments. The leading `if` is not itself a condition.
    #[test]
    fn irules_abc_elseif_else_conditions() {
        check_metrics::<IrulesParser>(
            "when X {\n    if { $a } { set r 1 } elseif { $b } { set r 2 } else { set r 3 }\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 3);
                // The `elseif` and `else` clauses are one condition each,
                // as before; #1180 adds the two bare truthy predicates
                // (`{ $a }`, `{ $b }`). C++'s
                // `if(a){} else if(b){} else {}` also scores 4.
                assert_eq!(metric.abc.conditions_sum(), 4);
            },
        );
    }

    /// A ternary contributes its own condition plus the `>` comparison in
    /// its test: conditions 2; the `set` is one assignment.
    #[test]
    fn irules_abc_ternary_condition() {
        check_metrics::<IrulesParser>(
            "when X {\n    set y [expr { $a > 0 ? 1 : 0 }]\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.assignments_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    /// Fitzpatrick Rule 9: the short-circuit `&&` is not itself a condition,
    /// but each negated bare operand (`!$a`, `!$b`) in the chain is. Guards
    /// the `irules_count_unary_conditions` / `irules_inspect_container`
    /// walker — conditions 2.
    #[test]
    fn irules_abc_negated_operands_in_chain() {
        check_metrics::<IrulesParser>(
            "when X {\n    if { !$a && !$b } { log local0. hi }\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    /// A bare-truthy `if {$a}` predicate is one condition (#1180).
    ///
    /// It carries no comparison, ternary or short-circuit operator, so
    /// before the Phase 2B slot routing landed nothing invoked the
    /// unary-conditional walker and the whole predicate scored 0 — this
    /// test previously pinned that absence, and the metrics book's ABC
    /// deviation table said so. Now the `if` node routes its `expr`
    /// predicate and the count matches C++'s `if (a)`, which is also 1.
    /// The `log` command remains the single branch.
    #[test]
    fn irules_abc_bare_truthy_counts_one_condition() {
        check_metrics::<IrulesParser>(
            "when X {\n    if { $a } { log local0. hi }\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
            },
        );
    }

    /// The negated form of the same predicate, which was *also* 0 before
    /// #1180: `!$a` reached the walker but no parent seeded boolean
    /// context, so the terminal operand was never counted. Distinct from
    /// `irules_abc_negated_operands_in_chain`, whose `&&` supplied the
    /// seed the bare form lacked.
    #[test]
    fn irules_abc_negated_bare_truthy_counts_one_condition() {
        check_metrics::<IrulesParser>(
            "when X {\n    if { !$a } { log local0. hi }\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.branches_sum(), 1);
                assert_eq!(metric.abc.conditions_sum(), 1);
            },
        );
    }

    /// The ternary's three operand slots, located relative to the `?`
    /// and `:` tokens because the grammar exposes no fields (#1180).
    ///
    /// `$a ? !$b : !$c` is four: the `ternary_expr` node, the bare
    /// truthy condition, and one per negated branch — the same value
    /// Java, C#, Groovy, the C family, the JS family, PHP, Perl, Ruby
    /// and Python report for the identical expression.
    #[test]
    fn irules_abc_ternary_routes_its_operand_slots() {
        check_metrics::<IrulesParser>(
            "when X {\n    set y [expr { $a ? !$b : !$c }]\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 4);
            },
        );
    }

    /// The #1161 control: a ternary whose condition is a *comparison*
    /// must not move. The `>` already supplied its condition and the
    /// branches are unnegated, so routing the slots adds nothing.
    #[test]
    fn irules_abc_comparison_ternary_is_unchanged_by_slot_routing() {
        check_metrics::<IrulesParser>(
            "when X {\n    set y [expr { $a > 0 ? 1 : 0 }]\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.abc.conditions_sum(), 2);
            },
        );
    }

    /// A parenthesised ternary condition scores the same as a bare one.
    ///
    /// The grammar inlines `( … )` as anonymous children of
    /// `ternary_expr` rather than wrapping them in a node, so a
    /// fixed-index reading of the slots would shift right by one and
    /// mis-assign every operand. This is the input that discriminates
    /// the token-relative location the fix uses.
    #[test]
    fn irules_abc_parenthesised_ternary_condition_matches_the_bare_form() {
        // `check_metrics` takes a bare `fn`, so it cannot carry the
        // first measurement into the second comparison; `metrics_verbatim`
        // returns a value instead.
        let conditions = |source: &str| {
            crate::test_support::metrics_verbatim(
                crate::LANG::Irules,
                source.as_bytes(),
                crate::MetricsOptions::default(),
            )
            .abc
            .conditions_sum()
        };
        let bare = conditions("when X {\n    set y [expr { $a ? !$b : !$c }]\n}\n");
        assert_eq!(bare, 4, "the bare form is the documented reference value");
        assert_eq!(
            conditions("when X {\n    set y [expr { ($a) ? !$b : !$c }]\n}\n"),
            bare,
            "parenthesising the condition must not change the count"
        );
    }
}

/// A comment inside a ternary must not change its ABC conditions
/// (#1181).
///
/// Two opposite defects, one cause: tree-sitter counts a comment among a
/// node's children, so it is the operand's previous sibling *and* it
/// shifts every positional index.
///
/// * Languages whose seed asked "is my previous sibling `?` or `:`"
///   (C family, PHP, Perl, JS family) read the comment as "not a
///   ternary token", flipped the boolean-context seed on for a *branch*
///   slot, and **over**-counted: `a ? /*n*/ (b) : c` scored 3 where
///   `a ? (b) : c` scores 2.
/// * Languages whose branch walk read `child(2)` / `child(4)` (Java,
///   C#, Groovy) landed on the comment instead of the operand, never
///   inspected it, and **under**-counted: `a ? /*n*/ !b : c` scored 2
///   where `a ? !b : c` scores 3.
///
/// Both slots are now addressed by grammar field. The parenthesised
/// operand is the only input that discriminates the first defect and
/// the negated operand the only one that discriminates the second —
/// existing ternary fixtures use neither.
#[cfg(test)]
mod ternary_comment_invariance {
    use crate::test_support::metrics_verbatim;
    use crate::{LANG, MetricsOptions};

    fn conditions(lang: LANG, source: &str) -> u64 {
        metrics_verbatim(lang, source.as_bytes(), MetricsOptions::default())
            .abc
            .conditions_sum()
    }

    /// `(base, with_comment)` for a parenthesised and a negated branch
    /// operand, per language.
    fn cases(lang: LANG) -> Option<[(String, String); 2]> {
        // `{}` marks the consequence slot; `/*n*/` the inserted comment.
        let (template, paren, negated, comment) = match lang {
            LANG::Cpp | LANG::C | LANG::Objc | LANG::Mozcpp => {
                ("int f(){ int x = a ? {} : c; }", "(b)", "!b", "/*n*/ ")
            }
            LANG::Java => (
                "class K{ void f(){ int x = a ? {} : c; } }",
                "(b)",
                "!b",
                "/*n*/ ",
            ),
            LANG::Csharp => (
                "class K{ void f(){ var x = a ? {} : c; } }",
                "(b)",
                "!b",
                "/*n*/ ",
            ),
            LANG::Groovy => ("def f(){ def x = a ? {} : c }", "(b)", "!b", "/*n*/ "),
            LANG::Javascript | LANG::Typescript | LANG::Tsx | LANG::Mozjs => {
                ("function f(){ var x = a ? {} : c; }", "(b)", "!b", "/*n*/ ")
            }
            LANG::Php => (
                "<?php function f(){ $x = $a ? {} : $c; }",
                "($b)",
                "!$b",
                "/*n*/ ",
            ),
            // Perl has no block comment: `#` runs to end of line, so the
            // comment must carry its own newline.
            LANG::Perl => ("sub f { my $x = $a ? {} : $c; }", "($b)", "!$b", "# n\n "),
            _ => return None,
        };
        let build = |operand: &str, with_comment: bool| {
            let slot = if with_comment {
                format!("{comment}{operand}")
            } else {
                operand.to_owned()
            };
            template.replace("{}", &slot)
        };
        Some([
            (build(paren, false), build(paren, true)),
            (build(negated, false), build(negated, true)),
        ])
    }

    #[test]
    fn a_comment_before_a_branch_operand_changes_nothing() {
        let mut checked = 0;
        for lang in LANG::into_enum_iter() {
            if !lang.is_enabled() {
                continue;
            }
            let Some(pairs) = cases(lang) else { continue };
            checked += 1;
            for (base, commented) in pairs {
                assert_eq!(
                    conditions(lang, &commented),
                    conditions(lang, &base),
                    "{lang:?}: a comment changed the ABC conditions of a ternary\n  \
                     without: {base}\n  with:    {commented}"
                );
            }
        }
        assert!(
            checked > 0,
            "no ternary language enabled; this test asserted nothing"
        );
    }

    /// The absolute values the invariance test compares against, so a
    /// regression that moved *both* sides equally still fails.
    ///
    /// expected: `a ? (b) : c` counts the `?` marker plus the condition
    /// `a` in boolean context = 2. Negating the consequence adds one
    /// more, since `!b` establishes boolean content for that slot = 3.
    #[test]
    fn the_baseline_values_are_two_and_three() {
        let mut checked = 0;
        for lang in LANG::into_enum_iter() {
            if !lang.is_enabled() {
                continue;
            }
            let Some([(paren, _), (negated, _)]) = cases(lang) else {
                continue;
            };
            assert_eq!(
                conditions(lang, &paren),
                2,
                "{lang:?}: parenthesised branch"
            );
            assert_eq!(conditions(lang, &negated), 3, "{lang:?}: negated branch");
            checked += 1;
        }
        assert!(
            checked > 0,
            "no ternary language enabled; this test asserted nothing"
        );
    }
}

/// A keyword negation must score like its symbolic twin (#1182).
///
/// `not b` and `!b` are the same negation — they differ in precedence,
/// not in meaning, and ABC counts the negation rather than the parse.
/// Ruby and Perl tested only the `!` token, so `if not b` scored 0
/// against `if !b`'s 1, and a `not` ternary scored 2 against the `!`
/// form's 4.
///
/// Lua and Elixir were checked in the same sweep and were already
/// correct: Lua's only negation keyword *is* `not` and it was the token
/// being tested, and Elixir reaches the same count by another path.
/// They are exercised here so a future edit cannot regress them
/// silently. Python counts `not` through its own dispatcher arm and has
/// no `!` spelling to compare against.
#[cfg(test)]
mod keyword_negation_parity {
    use crate::test_support::metrics_verbatim;
    use crate::{LANG, MetricsOptions};

    fn conditions(lang: LANG, source: &str) -> u64 {
        metrics_verbatim(lang, source.as_bytes(), MetricsOptions::default())
            .abc
            .conditions_sum()
    }

    /// `(bang_form, keyword_form)` pairs that must score identically.
    fn pairs(lang: LANG) -> Option<Vec<(String, String)>> {
        let build =
            |t: &str| -> (String, String) { (t.replace("{NOT}", "!"), t.replace("{NOT}", "not ")) };
        let templates: &[&str] = match lang {
            LANG::Ruby => &[
                "def f(b)\n  if {NOT}b\n    1\n  end\nend\n",
                "def f(a, b, c)\n  x = a ? ({NOT}b) : ({NOT}c)\nend\n",
                "def f(a, b, c)\n  x = a ? b : ({NOT}c)\nend\n",
            ],
            LANG::Perl => &[
                "sub f { if ({NOT}$b) { 1; } }",
                "sub f { my $x = $a ? ({NOT}$b) : ({NOT}$c); }",
                "sub f { my $x = $a ? $b : ({NOT}$c); }",
            ],
            // Already correct before #1182; pinned so they stay that way.
            LANG::Elixir => &["def f(b) do\n  if {NOT}b do\n    1\n  end\nend\n"],
            _ => return None,
        };
        Some(templates.iter().map(|t| build(t)).collect())
    }

    #[test]
    fn the_not_keyword_scores_like_bang() {
        let mut checked = 0;
        for lang in LANG::into_enum_iter() {
            if !lang.is_enabled() {
                continue;
            }
            let Some(pairs) = pairs(lang) else { continue };
            checked += 1;
            for (bang, keyword) in pairs {
                assert_eq!(
                    conditions(lang, &keyword),
                    conditions(lang, &bang),
                    "{lang:?}: `not` and `!` scored differently\n  bang:    {bang}\n  keyword: {keyword}"
                );
            }
        }
        assert!(
            checked > 0,
            "no language enabled; this test asserted nothing"
        );
    }

    /// The absolute values, so a regression that moved both spellings
    /// equally still fails.
    ///
    /// expected: `if !b` is one condition — the negated bare operand.
    /// `a ? (!b) : (!c)` is four: the `?` marker, the condition `a` in
    /// boolean context, and one per negated branch operand.
    #[test]
    fn the_baseline_values_are_one_and_four() {
        let mut checked = 0;
        for (lang, guard, ternary) in [
            (
                LANG::Ruby,
                "def f(b)\n  if not b\n    1\n  end\nend\n",
                "def f(a, b, c)\n  x = a ? (not b) : (not c)\nend\n",
            ),
            (
                LANG::Perl,
                "sub f { if (not $b) { 1; } }",
                "sub f { my $x = $a ? (not $b) : (not $c); }",
            ),
        ] {
            if !lang.is_enabled() {
                continue;
            }
            assert_eq!(conditions(lang, guard), 1, "{lang:?}: `if not b`");
            assert_eq!(conditions(lang, ternary), 4, "{lang:?}: `not` ternary");
            checked += 1;
        }
        assert!(
            checked > 0,
            "no language enabled; this test asserted nothing"
        );
    }

    /// Lua's only negation keyword is `not`, so it has no `!` twin to
    /// compare against — its guard is that the keyword counts at all.
    #[test]
    fn lua_counts_its_only_negation_keyword() {
        if !LANG::Lua.is_enabled() {
            return;
        }
        assert_eq!(
            conditions(
                LANG::Lua,
                "function f(b)\n  if not b then return 1 end\nend\n"
            ),
            1
        );
    }
}
