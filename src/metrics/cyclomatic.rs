// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]
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
use crate::macros::implement_metric_trait;
use crate::*;

/// The `Cyclomatic` metric.
#[derive(Debug, Clone)]
pub struct Stats {
    cyclomatic_sum: f64,
    cyclomatic: f64,
    n: usize,
    cyclomatic_max: f64,
    cyclomatic_min: f64,
    cyclomatic_modified_sum: f64,
    cyclomatic_modified: f64,
    cyclomatic_modified_max: f64,
    cyclomatic_modified_min: f64,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            cyclomatic_sum: 0.,
            cyclomatic: 1.,
            n: 1,
            cyclomatic_max: 0.,
            cyclomatic_min: f64::MAX,
            cyclomatic_modified_sum: 0.,
            cyclomatic_modified: 1.,
            cyclomatic_modified_max: 0.,
            cyclomatic_modified_min: f64::MAX,
        }
    }
}

/// Serialised shape for the `modified` sub-object.
struct ModifiedStats<'a>(&'a Stats);

impl Serialize for ModifiedStats<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let s = self.0;
        let mut st = serializer.serialize_struct("cyclomatic_modified", 4)?;
        st.serialize_field("sum", &s.cyclomatic_modified_sum())?;
        st.serialize_field("average", &s.cyclomatic_modified_average())?;
        st.serialize_field("min", &s.cyclomatic_modified_min())?;
        st.serialize_field("max", &s.cyclomatic_modified_max())?;
        st.end()
    }
}

impl Serialize for Stats {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut st = serializer.serialize_struct("cyclomatic", 5)?;
        st.serialize_field("sum", &self.cyclomatic_sum())?;
        st.serialize_field("average", &self.cyclomatic_average())?;
        st.serialize_field("min", &self.cyclomatic_min())?;
        st.serialize_field("max", &self.cyclomatic_max())?;
        st.serialize_field("modified", &ModifiedStats(self))?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "sum: {}, average: {}, min: {}, max: {}, \
             modified_sum: {}, modified_average: {}, modified_min: {}, modified_max: {}",
            self.cyclomatic_sum(),
            self.cyclomatic_average(),
            self.cyclomatic_min(),
            self.cyclomatic_max(),
            self.cyclomatic_modified_sum(),
            self.cyclomatic_modified_average(),
            self.cyclomatic_modified_min(),
            self.cyclomatic_modified_max(),
        )
    }
}

impl Stats {
    /// Merges a second `Cyclomatic` metric into the first one
    pub fn merge(&mut self, other: &Stats) {
        self.cyclomatic_max = self.cyclomatic_max.max(other.cyclomatic_max);
        self.cyclomatic_min = self.cyclomatic_min.min(other.cyclomatic_min);
        self.cyclomatic_sum += other.cyclomatic_sum;
        self.n += other.n;

        self.cyclomatic_modified_max = self
            .cyclomatic_modified_max
            .max(other.cyclomatic_modified_max);
        self.cyclomatic_modified_min = self
            .cyclomatic_modified_min
            .min(other.cyclomatic_modified_min);
        self.cyclomatic_modified_sum += other.cyclomatic_modified_sum;
    }

    /// Returns the `Cyclomatic` metric value for the current space.
    #[must_use]
    pub fn cyclomatic(&self) -> f64 {
        self.cyclomatic
    }

    /// Returns the sum of standard cyclomatic values across all spaces.
    #[must_use]
    pub fn cyclomatic_sum(&self) -> f64 {
        self.cyclomatic_sum
    }

    /// Returns the average standard cyclomatic complexity.
    #[must_use]
    pub fn cyclomatic_average(&self) -> f64 {
        self.cyclomatic_sum() / self.n as f64
    }

    /// Returns the maximum standard cyclomatic complexity.
    #[must_use]
    pub fn cyclomatic_max(&self) -> f64 {
        self.cyclomatic_max
    }

    /// Returns the minimum standard cyclomatic complexity.
    ///
    /// Collapses the `f64::MAX` sentinel that `Stats::default()` plants
    /// into `cyclomatic_min` to `0.0`, so a never-observed space
    /// serializes to a meaningful number rather than `1.7976931e308`.
    #[allow(clippy::float_cmp)]
    #[must_use]
    pub fn cyclomatic_min(&self) -> f64 {
        if self.cyclomatic_min == f64::MAX {
            0.0
        } else {
            self.cyclomatic_min
        }
    }

    /// Returns the modified cyclomatic complexity for the current space.
    ///
    /// Modified cyclomatic counts each switch/match/when/select container as
    /// one decision point regardless of how many case arms it contains.  All
    /// other branching constructs are weighted identically to standard CCN.
    ///
    /// Edge case: an empty switch (`switch (x) {}`) yields modified = 1
    /// and standard = 0, so modified can exceed standard for arm-less
    /// containers.  This matches Lizard's `-m` convention, which keys on
    /// the switch keyword rather than the presence of arms.
    #[must_use]
    pub fn cyclomatic_modified(&self) -> f64 {
        self.cyclomatic_modified
    }

    /// Returns the sum of modified cyclomatic values across all spaces.
    #[must_use]
    pub fn cyclomatic_modified_sum(&self) -> f64 {
        self.cyclomatic_modified_sum
    }

    /// Returns the average modified cyclomatic complexity.
    #[must_use]
    pub fn cyclomatic_modified_average(&self) -> f64 {
        self.cyclomatic_modified_sum() / self.n as f64
    }

    /// Returns the maximum modified cyclomatic complexity.
    #[must_use]
    pub fn cyclomatic_modified_max(&self) -> f64 {
        self.cyclomatic_modified_max
    }

    /// Returns the minimum modified cyclomatic complexity.
    ///
    /// Same `f64::MAX` sentinel collapse as `cyclomatic_min`.
    #[allow(clippy::float_cmp)]
    #[must_use]
    pub fn cyclomatic_modified_min(&self) -> f64 {
        if self.cyclomatic_modified_min == f64::MAX {
            0.0
        } else {
            self.cyclomatic_modified_min
        }
    }

    #[inline]
    pub(crate) fn compute_sum(&mut self) {
        self.cyclomatic_sum += self.cyclomatic;
        self.cyclomatic_modified_sum += self.cyclomatic_modified;
    }

    #[inline]
    pub(crate) fn compute_minmax(&mut self) {
        self.cyclomatic_max = self.cyclomatic_max.max(self.cyclomatic);
        self.cyclomatic_min = self.cyclomatic_min.min(self.cyclomatic);
        self.cyclomatic_modified_max = self.cyclomatic_modified_max.max(self.cyclomatic_modified);
        self.cyclomatic_modified_min = self.cyclomatic_modified_min.min(self.cyclomatic_modified);
        self.compute_sum();
    }
}

#[doc(hidden)]
/// Per-language computation of cyclomatic complexity.
pub trait Cyclomatic
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    ///
    /// `code` is the source bytes the node spans, so that languages
    /// whose branching constructs surface as untyped `Call` nodes
    /// (Elixir's `if`/`unless`/`for`/`while`/`with`/`case`/`cond`,
    /// for example) can identify them by inspecting the call target's
    /// text. Most languages discard the parameter with `_`.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats);

    /// Like [`Cyclomatic::compute`], but honors per-traversal options.
    ///
    /// `count_try` toggles whether Rust's `?` operator (the
    /// `try_expression` grammar node) contributes to cyclomatic
    /// complexity. The default body ignores `count_try` and delegates
    /// to [`Cyclomatic::compute`], so every language whose grammar has
    /// no `try_expression` node keeps its existing behaviour with no
    /// per-language edit. Only [`RustCode`] overrides this to act on
    /// the flag (#409).
    #[inline]
    fn compute_with_options<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        stats: &mut Stats,
        count_try: bool,
    ) {
        let _ = count_try;
        Self::compute(node, code, stats);
    }
}

impl Cyclomatic for PythonCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Python::*;

        // Python's `match`/`case` (PEP 634, 3.10+) is treated like Rust's
        // `match` and the C-family `switch`: each non-bare-wildcard arm
        // counts toward standard CCN, and the containing `match_statement`
        // adds the modified count. A bare `case _:` (no guard) is skipped,
        // mirroring Rust's `MatchArm` filter and Java/C#'s `default:`
        // exclusion. A guard (`case _ if g:`) still escapes the filter.
        // `with` (and `async with`) is deliberately absent from the
        // decision-point arm below: it is unconditional resource
        // management, not a branch. Standard McCabe does not count it,
        // and the C-family `using` / try-with-resources siblings are
        // likewise uncounted, so counting it here would be an
        // undocumented divergence. The `__exit__`-can-suppress-an-
        // exception argument is rejected for parity with those
        // siblings and with the textbook definition. Both plain
        // `with` and `async with` surface the same `with` keyword
        // token (`With`), so omitting it stops counting both. See #418.
        match node.kind_id().into() {
            If | Elif | For | While | Except | Assert | And | Or => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            CaseClause
                if crate::metrics::npa::python_case_clause_counts(node, UNDERSCORE as u16) =>
            {
                stats.cyclomatic += 1.;
            }
            MatchStatement => {
                stats.cyclomatic_modified += 1.;
            }
            // Python's `for/else`, `while/else`, and `try/except/else`
            // attach an `else_clause` whose body runs only on the
            // "normal" completion path (loop finishes without `break`;
            // try block finishes without raising). That conditional
            // execution is a distinct decision point, so count it
            // toward both standard and modified cyclomatic. Plain
            // `if/else` is unconditional once the `if` has been
            // counted, so we must NOT fire for `else_clause` parents
            // of `if_statement` — see #229.
            Else if node.parent_grandparent_match(
                |parent| parent.kind_id() == ElseClause,
                |grand| {
                    matches!(
                        grand.kind_id().into(),
                        ForStatement | WhileStatement | TryStatement
                    )
                },
            ) =>
            {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

/// C-family cyclomatic: `Case` adds standard, `SwitchStatement` adds
/// modified, and the shared branching kinds add both.  The ternary token
/// name varies (`TernaryExpression` for JS-family, `ConditionalExpression`
/// for Cpp), so it's a parameter.  The short-circuit operator list is
/// also a parameter because JS-family languages include nullish
/// coalescing (`??`, token `QMARKQMARK`) and the three compound short-
/// circuit assignment forms `&&=` (`AMPAMPEQ`), `||=` (`PIPEPIPEEQ`),
/// `??=` (`QMARKQMARKEQ`) on top of `&&` and `||`, while C++ has only
/// `&&` and `||` (issues #226, #231, #248).
///
/// **`If` / `For` / `While` are keyword tokens in the per-language
/// enums (e.g. `Cpp::While == "while"`), not statement nodes.** The
/// `while` token therefore fires once inside both `WhileStatement` AND
/// `DoStatement` (the `while` keyword of `do { … } while (…)`), and
/// the `for` token fires once inside `ForStatement`, C++
/// `ForRangeLoop`, Java `EnhancedForStatement`, and any other
/// grammar-specific loop form that spells the keyword `for`. So
/// adding the statement nodes themselves would double-count those
/// loops — see issue #284 for the false-positive analysis. The
/// regression tests `cpp_do_statement_counts_in_cyclomatic`,
/// `cpp_for_range_loop_counts_in_cyclomatic`,
/// `java_do_statement_counts_in_cyclomatic`, and
/// `java_enhanced_for_statement_counts_in_cyclomatic` pin the
/// correct keyword-driven counts.
macro_rules! impl_cyclomatic_c_family {
    ($code:ty, $lang:ident, $ternary:ident, [$($short_circuit:ident),+ $(,)?]) => {
        impl Cyclomatic for $code {
            fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
                use $lang::*;
                match node.kind_id().into() {
                    Case => stats.cyclomatic += 1.,
                    SwitchStatement => stats.cyclomatic_modified += 1.,
                    If | For | While | Catch | $ternary $(| $short_circuit)+ => {
                        stats.cyclomatic += 1.;
                        stats.cyclomatic_modified += 1.;
                    }
                    _ => {}
                }
            }
        }
    };
}

// JS-family: include nullish coalescing (`??`) and the three compound
// short-circuit assignments `&&=`, `||=`, `??=` as short-circuit
// decisions in addition to `&&` and `||` (issues #226, #231, #248).
// Each `op=` is semantically `x = x op y` — one short-circuit decision
// edge, same as the bare operator. Cognitive parity comes from #236.
//
// Optional chaining `?.` is also short-circuit (it skips the rest of
// the chain when the LHS is nullish) and adds one decision point per
// occurrence (issue #281). The token varies across grammars:
// JS/MozJS expose only `OptionalChain` (which IS the `?.` token in
// those grammars), while TS/TSX expose both an `optional_chain`
// wrapper and a child `?.` token (`QMARKDOT`); counting `QMARKDOT`
// matches every textual `?.` exactly once in TS/TSX.
macro_rules! impl_cyclomatic_js_family {
    ($code:ty, $lang:ident, $opt_chain:ident) => {
        impl_cyclomatic_c_family!(
            $code,
            $lang,
            TernaryExpression,
            [
                AMPAMP,
                PIPEPIPE,
                QMARKQMARK,
                AMPAMPEQ,
                PIPEPIPEEQ,
                QMARKQMARKEQ,
                $opt_chain
            ]
        );
    };
}
impl_cyclomatic_js_family!(MozjsCode, Mozjs, OptionalChain);
impl_cyclomatic_js_family!(JavascriptCode, Javascript, OptionalChain);
impl_cyclomatic_js_family!(TypescriptCode, Typescript, QMARKDOT);
impl_cyclomatic_js_family!(TsxCode, Tsx, QMARKDOT);

impl Cyclomatic for RustCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        // The default (#409): `?` counts toward cyclomatic, matching
        // upstream rust-code-analysis and every published metric value.
        Self::compute_with_options(node, code, stats, true);
    }

    fn compute_with_options<'a>(
        node: &Node<'a>,
        _code: &'a [u8],
        stats: &mut Stats,
        count_try: bool,
    ) {
        rust_cyclomatic_increment(node, stats, count_try);
    }
}

/// Rust's per-node cyclomatic increment, shared by both
/// [`Cyclomatic::compute`] and [`Cyclomatic::compute_with_options`].
///
/// Extracted as a free function so the `impl Cyclomatic for RustCode`
/// block carries only the two thin trait methods — the bare-wildcard
/// closure lives here instead, keeping the impl block's aggregate
/// `nargs` within the self-scan gate.
///
/// `count_try` toggles the `?` operator's contribution (#409): `true`
/// counts it toward both standard and modified cyclomatic, `false`
/// treats it as linear error propagation.
fn rust_cyclomatic_increment(node: &Node<'_>, stats: &mut Stats, count_try: bool) {
    use Rust::*;

    match node.kind_id().into() {
        // Standard-only: individual match arms.
        // Lizard counts `match` as a single control-flow keyword; we count
        // each arm, so the modified metric collapses them back to the
        // container.
        // Bare wildcard `_ =>` arms are skipped to match C-family
        // `default:` treatment. Patterns like `Some(_)`, `(_, x)`,
        // or `_ if guard` are not bare wildcards and still count.
        // The check scans NAMED children of `match_pattern`, so
        // anonymous tokens like a leading `|` (legal in or-patterns:
        // `| _ => ...`) don't throw off detection, and a guard
        // (`_ if g`) adds a second named child so it correctly
        // escapes the filter. Shared helper with the `Abc` impl
        // (`super::npa::pattern_is_bare_underscore`).
        MatchArm | MatchArm2 => {
            let is_bare_wildcard = node.child_by_field_name("pattern").is_some_and(|pat| {
                crate::metrics::npa::pattern_is_bare_underscore(&pat, UNDERSCORE as u16)
            });
            if !is_bare_wildcard {
                stats.cyclomatic += 1.;
            }
        }
        // Modified-only: the match expression container.
        MatchExpression => {
            stats.cyclomatic_modified += 1.;
        }
        // The `?` operator. Counted toward both standard and modified by
        // default; when `count_try` is false the arm's guard fails and
        // `?` falls through to `_ => {}`, treating it as linear error
        // propagation (#409). Gated separately from the unconditional
        // branching kinds below.
        TryExpression if count_try => {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
        }
        // Both standard and modified.
        If | For | While | Loop | AMPAMP | PIPEPIPE => {
            stats.cyclomatic += 1.;
            stats.cyclomatic_modified += 1.;
        }
        _ => {}
    }
}

// C++ has only `&&` and `||` short-circuit operators.
// Grammar-specific loop kinds (`DoStatement`, `ForRangeLoop`) are NOT
// listed here because the `While` / `For` keyword-token arms above
// already fire inside them; adding the statement nodes would
// double-count (issue #284).
impl_cyclomatic_c_family!(CppCode, Cpp, ConditionalExpression, [AMPAMP, PIPEPIPE]);

// Java and Groovy share the same decision-kind set for cyclomatic
// complexity; Groovy adds `Assert` as an extra branch (its `assert`
// keyword is a runtime check that branches on its condition,
// matching Sonar's standard-CCN treatment). `impl_cyclomatic_java_like!`
// emits the same match body against each enum, with an
// `[$($extra:ident),*]` list for any language-specific decision kinds
// (issue #300; mirrors `impl_npm_java_like!` / `impl_npa_java_like!`).
//
// Why a dedicated macro instead of reusing `impl_cyclomatic_c_family!`:
// the C-family macro uses `SwitchStatement` (the wrapping node) as the
// modified-CCN container marker, whereas Java/Groovy use the `Switch`
// keyword token — which fires exactly once per switch (both classic
// switch statements and Java 14+ switch expressions). Counting the
// keyword keeps the modified-CCN tally aligned with the standard-CCN
// `Case` arms.
//
// Keyword-vs-statement (issue #284): `If` / `For` / `While` here are
// the *keyword* tokens (`Java::While == "while"`, etc.), not the
// statement nodes. The `while` keyword therefore fires inside both
// `WhileStatement` and `DoStatement`, and the `for` keyword fires
// inside both `ForStatement` and `EnhancedForStatement`. The
// grammar-specific loop forms are already counted via their inner
// keyword tokens; listing the statement nodes here would
// double-count. The regression tests
// `java_do_statement_counts_in_cyclomatic`,
// `java_enhanced_for_statement_counts_in_cyclomatic`,
// `groovy_do_statement_counts_in_cyclomatic`, and
// `groovy_enhanced_for_statement_counts_in_cyclomatic` pin the
// correct keyword-driven counts.
//
// Groovy note: under the pinned dekobon grammar (root Cargo.toml),
// Elvis `?:` and the safe-navigation operators `?.` / `??.` all parse
// cleanly to dedicated nodes with real lexer tokens, so they are
// counted as branches via the GroovyCode extra-token list below (see
// the per-call rationale at that invocation). This differs from
// amaanq's grammar, which emitted ERROR nodes for those constructs.
macro_rules! impl_cyclomatic_java_like {
    ($code:ty, $lang:ident, [$($extra:ident),* $(,)?]) => {
        impl Cyclomatic for $code {
            fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
                use $lang::*;

                match node.kind_id().into() {
                    Case => {
                        stats.cyclomatic += 1.;
                    }
                    Switch => {
                        stats.cyclomatic_modified += 1.;
                    }
                    If | For | While | Catch | TernaryExpression | AMPAMP | PIPEPIPE
                    $(| $extra)* => {
                        stats.cyclomatic += 1.;
                        stats.cyclomatic_modified += 1.;
                    }
                    _ => {}
                }
            }
        }
    };
}

impl_cyclomatic_java_like!(JavaCode, Java, []);
// Groovy extra branches under the pinned dekobon grammar:
// - `Assert` (cyclomatic branch — same as Java; its `assert` keyword
//   is a runtime check that branches on its condition).
// - Elvis operator token `?:` (`QMARKCOLON`): the grammar surfaces
//   Elvis as a distinct `elvis_expression` node with `?:` as a real
//   lexer token, so the macro picks it up as +1 per occurrence
//   (closes #246 cyclomatic case).
// - Safe-navigation `?.` (`QMARKDOT`) and `??.` (`QMARKQMARKDOT`):
//   both are short-circuit — they skip the member access/call when the
//   LHS is null — so each occurrence is one decision point, mirroring
//   the Kotlin/PHP/JS/C# treatment of `?.` (issues #281, #452). The
//   grammar emits the `?.` token once per operator inside a
//   `safe_navigation_expression` and the `??.` token inside a
//   `safe_chain_dot_expression`, so matching the tokens counts each
//   textual operator exactly once, including in chains (`a?.b?.c` is
//   +2). Matching the wrapper nodes instead would miscount nested
//   chains; the token is the single granularity that fires once per
//   textual operator, paralleling Kotlin/TS which match `QMARKDOT`.
impl_cyclomatic_java_like!(
    GroovyCode,
    Groovy,
    [Assert, QMARKCOLON, QMARKDOT, QMARKQMARKDOT]
);

impl Cyclomatic for CsharpCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Csharp::*;

        match node.kind_id().into() {
            // Standard-only: individual switch statement arms. The `case`
            // keyword token is what is matched here; `default:` uses a
            // distinct `Default` token and is correctly excluded.
            Case => {
                stats.cyclomatic += 1.;
            }
            // Standard-only: switch expression arms, except the bare
            // discard arm `_ =>` (and `var _ =>`), which is C#'s analogue
            // of `default:` and must NOT contribute to standard CCN
            // (issue #282 / lesson 11). A guarded discard
            // (`_ when g => …`) still counts because the guard introduces
            // a non-trivial decision, mirroring Rust's `_ if g` rule.
            SwitchExpressionArm if !csharp_switch_expression_arm_is_bare_discard(node) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: the switch statement and switch expression
            // containers each collapse to one decision point.
            SwitchStatement | SwitchExpression => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            IfStatement
            | ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | CatchClause
            | ConditionalExpression
            | ConditionalAccessExpression
            | AMPAMP
            | PIPEPIPE
            | QMARKQMARK
            | QMARKQMARKEQ => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for GoCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        // Aliased because `Go::Go` (the `go` keyword variant) collides with
        // the bare enum name in pattern position under `use Go::*;`.
        use Go as G;

        match node.kind_id().into() {
            // Standard-only: individual case arms inside switch/select.
            G::ExpressionCase | G::TypeCase | G::CommunicationCase => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: each distinct switch/select container collapses
            // all its arms into one decision point.
            G::ExpressionSwitchStatement | G::TypeSwitchStatement | G::SelectStatement => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            G::IfStatement | G::ForStatement | G::AMPAMP | G::PIPEPIPE => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for PerlCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Perl as P;

        match node.kind_id().into() {
            P::IfStatement
            | P::UnlessStatement
            | P::ElsifClause
            | P::WhileStatement
            | P::UntilStatement
            | P::ForStatement1
            | P::ForStatement2
            | P::WhenSimpleStatement
            | P::IfSimpleStatement
            | P::UnlessSimpleStatement
            | P::WhileSimpleStatement
            | P::UntilSimpleStatement
            | P::ForSimpleStatement
            | P::AMPAMP
            | P::PIPEPIPE
            | P::SLASHSLASH
            // Compound short-circuit assignments `&&=`, `||=`, `//=`
            // are semantically `x = x op y` and each carries one short-
            // circuit decision edge, parallel to the JS-family fix in
            // #248 (issue #249).
            | P::AMPAMPEQ
            | P::PIPEPIPEEQ
            | P::SLASHSLASHEQ
            | P::And
            | P::Or
            | P::TernaryExpression => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for KotlinCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Kotlin::*;

        match node.kind_id().into() {
            // Standard-only: individual when entries (arms), except the
            // `else -> …` arm which is Kotlin's analogue of `default:`
            // and must NOT contribute to standard CCN (issue #282 /
            // lesson 11). tree-sitter-kotlin-ng attaches a `condition`
            // field to every case-style entry; the else arm has no
            // `condition` field.
            WhenEntry if !kotlin_when_entry_is_else(node) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: the when expression container.
            WhenExpression => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            //
            // Kotlin's Elvis operator `?:` (`QMARKCOLON`) is a short-circuit
            // nullish operator analogous to JS `??` and each occurrence is a
            // distinct decision point, mirroring `&&` / `||`.
            //
            // Safe-navigation `?.` (`QMARKDOT`) is short-circuit too — it
            // skips the member access when the LHS is null — so each
            // occurrence is one decision point, mirroring the JS/C#
            // treatment of `?.` (issues #281, #436). The grammar emits the
            // `?.` token (`QMARKDOT`, id 140) once per operator inside a
            // `navigation_expression`, so matching the token counts each
            // textual `?.` exactly once, including in chains (`a?.b?.c` is
            // +2), and parallels TS/TSX which also match the `QMARKDOT`
            // token rather than the wrapper node.
            IfExpression | ForStatement | WhileStatement | DoWhileStatement | CatchBlock
            | AMPAMP | PIPEPIPE | QMARKCOLON | QMARKDOT => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for LuaCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        match node.kind_id().into() {
            Lua::IfStatement
            | Lua::ElseifStatement
            | Lua::ForStatement
            | Lua::WhileStatement
            | Lua::RepeatStatement
            | Lua::And
            | Lua::Or => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for PhpCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Php::*;

        match node.kind_id().into() {
            // Standard-only: individual case arms in switch/match.
            CaseStatement | MatchConditionalExpression => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: each switch/match container collapses to one
            // decision point.
            SwitchStatement | MatchExpression => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            IfStatement
            | ElseIfClause
            | ElseIfClause2
            | ForStatement
            | ForeachStatement
            | WhileStatement
            | DoStatement
            | ConditionalExpression
            | CatchClause
            | AMPAMP
            | PIPEPIPE
            | And
            | Or
            | Xor
            | QMARKQMARK
            | QMARKQMARKEQ
            // Nullsafe operator `?->` (`QMARKDASHGT`) is short-circuit — it
            // skips the member access/call when the LHS is null — so each
            // occurrence is one decision point, mirroring the JS/C#
            // treatment of `?.` (issues #281, #436). Matching the `?->`
            // token (`QMARKDASHGT`, id 129) counts each operator exactly
            // once across BOTH `nullsafe_member_access_expression`
            // (property access) and `nullsafe_member_call_expression`
            // (method call), and in chains (`$a?->b?->c` is +2). Matching
            // either node kind instead would miss the other form and could
            // double-count nested access/call; the token is the single
            // granularity that fires once per textual `?->`, paralleling
            // TS/TSX's `QMARKDOT` token approach.
            | QMARKDASHGT => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

// Real defaults — no executable branches. Audited in #188.
implement_metric_trait!(Cyclomatic, PreprocCode, CcommentCode);

impl Cyclomatic for RubyCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Ruby as R;

        match node.kind_id().into() {
            // Standard-only: individual when/in arms inside a case construct.
            R::When | R::InClause => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: each case container collapses its arms.
            R::Case | R::CaseMatch => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            R::If
            | R::Unless
            | R::Elsif
            | R::IfModifier
            | R::UnlessModifier
            | R::While
            | R::Until
            | R::For
            | R::WhileModifier
            | R::UntilModifier
            | R::Rescue
            | R::RescueModifier
            | R::RescueModifier2
            | R::RescueModifier3
            | R::Conditional
            | R::AMPAMP
            | R::PIPEPIPE
            | R::And
            | R::Or
            // Safe-navigation `&.` (`AMPDOT`) is short-circuit — it
            // skips the method call when the receiver is nil — so each
            // occurrence is one decision point, mirroring the
            // Kotlin/PHP/JS/C# treatment of `?.` (issues #281, #452).
            // The grammar emits the `&.` token once per operator inside
            // a `call` node, so matching the token counts each textual
            // `&.` exactly once, including in chains (`a&.b&.c` is +2),
            // paralleling Kotlin's `QMARKDOT` token approach.
            | R::AMPDOT => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for ElixirCode {
    // Elixir's control-flow constructs are not distinct grammar
    // productions: `if`/`unless`/`for`/`while`/`with`/`case`/`cond`/`try`
    // all surface as `Call` nodes whose `target` field is an
    // `Identifier` whose text spells the keyword. We must consult the
    // source bytes (mirroring `impl Exit for ElixirCode`) to identify
    // them.
    //
    // The split between standard and modified CCN mirrors the C-family
    // case/switch treatment: per-arm `stab_clause` nodes contribute
    // standard, while the multi-arm container Calls (`case`/`cond`/
    // `with`/`try`) contribute modified. Single-branch keyword Calls
    // (`if`/`unless`/`for`/`while`) contribute to both. Short-circuit
    // booleans (`&&`, `||`, `and`, `or`) contribute to both.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        use Elixir as E;

        match node.kind_id().into() {
            // Per-arm decisions: each `stab_clause` is one arm of a
            // `case`/`cond`/`with`/anonymous-fn body or a `rescue`/
            // `catch` handler. Standard-only — modified counts the
            // container Call once.
            E::StabClause => {
                stats.cyclomatic += 1.;
            }
            // Short-circuit booleans add a decision point in both
            // metrics.
            E::AMPAMP | E::PIPEPIPE | E::And | E::Or => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            E::Call => {
                if let Some(target) = node.child_by_field_name("target")
                    && target.kind_id() == E::Identifier
                    && let Some(name) = target.utf8_text(code)
                {
                    match name {
                        // Single-branch constructs: count for both.
                        // There are no per-arm `stab_clause`s exposing
                        // themselves separately, so the Call itself
                        // must carry the decision point.
                        "if" | "unless" | "for" | "while" => {
                            stats.cyclomatic += 1.;
                            stats.cyclomatic_modified += 1.;
                        }
                        // Multi-arm containers: count once for modified
                        // (the container collapses to a single decision).
                        // Per-arm `stab_clause`s already contribute to
                        // standard above.
                        "case" | "cond" | "with" | "try" => {
                            stats.cyclomatic_modified += 1.;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

/// Detects C# `switch_expression_arm`s whose pattern is a bare discard
/// (`_` or `var _`) and which carry no `when` guard — the analogue of
/// the C-family `default:` arm. Such arms must NOT contribute to
/// standard CCN, mirroring Rust's `_ =>` and Java/C#'s `default:`
/// treatment (lesson 11 / parity family 5). A guarded discard
/// (`_ when g => …`) still counts because the guard introduces a
/// non-trivial decision, matching Rust's `_ if g` rule.
fn csharp_switch_expression_arm_is_bare_discard(node: &Node) -> bool {
    use Csharp::*;

    /// Classification of a `switch_expression_arm`'s pattern child.
    /// `BareDiscard` means `_` or `var _` (the C# analogue of
    /// `default:`); any concrete type test, constant, or composite
    /// pattern is `NotDiscard` and still contributes to standard CCN.
    enum PatternKind {
        BareDiscard,
        NotDiscard,
    }

    fn classify_pattern(child: &Node) -> PatternKind {
        match child.kind_id().into() {
            // `pattern` is a supertype: tree-sitter flattens it to the
            // concrete subtype in the parse tree, so a bare `_` arm
            // surfaces as a direct `discard` child.
            Discard => PatternKind::BareDiscard,
            // `var _` parses as a `declaration_pattern` with children
            // `implicit_type` (`var`) and `discard` (`_`) rather than
            // as a `var_pattern` — tree-sitter-c-sharp treats `var` as
            // an implicit type designator. A `declaration_pattern`
            // whose only named children are `implicit_type` and
            // `discard` is therefore semantically the bare discard.
            // A non-implicit type (`int _`) is NOT excluded — the
            // type test is still a non-trivial decision.
            DeclarationPattern => {
                let mut saw_discard = false;
                let mut saw_implicit_type = false;
                for sub in child.children().filter(Node::is_named) {
                    match sub.kind_id().into() {
                        Discard => saw_discard = true,
                        ImplicitType => saw_implicit_type = true,
                        _ => return PatternKind::NotDiscard,
                    }
                }
                if saw_discard && saw_implicit_type {
                    PatternKind::BareDiscard
                } else {
                    PatternKind::NotDiscard
                }
            }
            _ => PatternKind::NotDiscard,
        }
    }

    let mut named = node.children().filter(Node::is_named);
    let Some(pattern) = named.next() else {
        return false;
    };
    let PatternKind::BareDiscard = classify_pattern(&pattern) else {
        return false;
    };
    // A guarded discard (`_ when g => …`) still counts because the
    // guard introduces a non-trivial decision, matching Rust's
    // `_ if g` rule.
    !named.any(|c| c.kind_id() == WhenClause)
}

/// Detects Kotlin `when_entry` nodes that are `else -> …` arms — the
/// analogue of the C-family `default:` arm. tree-sitter-kotlin-ng
/// attaches a `condition` field to every case-style entry; the `else`
/// arm has no `condition` field (only an anonymous `else` keyword
/// child). Such arms must NOT contribute to standard CCN.
fn kotlin_when_entry_is_else(node: &Node) -> bool {
    node.child_by_field_name("condition").is_none()
}

/// Detects Bash `*)` catch-all arms inside `case … esac`. Returns
/// `true` when the case_item has exactly one `value` field whose
/// source text is the literal `*`. Multi-value patterns (`a|b`,
/// `*|b`) are NOT bare and still count as decisions.
fn bash_case_item_is_bare_wildcard(node: &Node, code: &[u8]) -> bool {
    // tree-sitter-bash attaches the `value` field to each alternation
    // in the case pattern (`a|b)` produces two `value` children).
    // Walk via a single `TreeCursor`: `field_name()` exposes the field
    // for the current position and `goto_next_sibling()` is O(1), so
    // total cost is linear in child count — avoiding the per-call
    // O(i) `Node::child(i)` access that an index-based loop would
    // pay on every iteration.
    let mut cursor = node.0.walk();
    if !cursor.goto_first_child() {
        return false;
    }
    let mut value_count = 0usize;
    let mut sole_value_is_star = false;
    loop {
        if cursor.field_name() == Some("value") {
            value_count += 1;
            if value_count > 1 {
                return false;
            }
            sole_value_is_star = cursor.node().utf8_text(code).is_ok_and(|s| s.trim() == "*");
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    value_count == 1 && sole_value_is_star
}

impl Cyclomatic for BashCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats) {
        match node.kind_id().into() {
            // Standard-only: individual case arms (matches C-family `case:`
            // treatment — only arms contribute, not the container). The
            // bare-wildcard arm `*)` is Bash's analogue of the C-family
            // `default:` and is excluded from the standard count, matching
            // every other switch-bearing language. A multi-value pattern
            // (`a|b)`, `*|b)`) is NOT bare and still counts. Closes #211.
            Bash::CaseItem | Bash::CaseItem2 if !bash_case_item_is_bare_wildcard(node, code) => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: the case…esac container collapses all arms
            // into one decision point.
            Bash::CaseStatement => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            Bash::IfStatement
            | Bash::ElifClause
            | Bash::ForStatement
            | Bash::CStyleForStatement
            | Bash::WhileStatement
            | Bash::AMPAMP
            | Bash::PIPEPIPE => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for TclCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        match node.kind_id().into() {
            Tcl::If
            | Tcl::Elseif
            | Tcl::Foreach
            | Tcl::While
            | Tcl::Catch
            | Tcl::TernaryExpr
            | Tcl::AMPAMP
            | Tcl::PIPEPIPE => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
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
    /// observation must not leak the `f64::MAX` sentinel for
    /// `cyclomatic_min` or `cyclomatic_modified_min`. Both getters
    /// collapse the sentinel to `0.0` so JSON never emits
    /// `1.7976931e308`.
    #[test]
    fn cyclomatic_empty_file_min_is_zero() {
        let stats = Stats::default();
        assert_eq!(stats.cyclomatic_min(), 0.0);
        assert_eq!(stats.cyclomatic_modified_min(), 0.0);
    }

    /// A plain `if/else` must not be credited
    /// as a loop-`else`. The `Else` arm of `impl Cyclomatic for
    /// PythonCode` previously fired for every `else_clause` because
    /// the old `has_ancestors` helper only verified the second
    /// predicate; the rewritten `parent_grandparent_match` requires
    /// the grandparent to be `for/while/try`.
    ///
    /// Expected: unit(1) + fn(1) + if(1) = 3. No contribution from
    /// `else`.
    #[test]
    fn python_if_else_does_not_overcount_229() {
        check_metrics::<PythonParser>(
            "def f(x):
    if x > 0:
        y = 1
    else:
        y = 2
",
            "foo.py",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Companion to #229: a chained `if/elif/else` must count one
    /// per `if` and per `elif`, never the bare `else`.
    ///
    /// Expected: unit(1) + fn(1) + if(1) + elif(1) + elif(1) = 5.
    #[test]
    fn python_if_elif_else_chain_229() {
        check_metrics::<PythonParser>(
            "def f(x):
    if x == 1:
        return 10
    elif x == 2:
        return 20
    elif x == 3:
        return 30
    else:
        return 0
",
            "foo.py",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
            },
        );
    }

    /// The for/else feature must still count: the `else` body runs
    /// only when the loop completes without `break`, which is a
    /// distinct decision point.
    ///
    /// Expected: unit(1) + fn(1) + for(1) + else(1) = 4.
    #[test]
    fn python_for_else_still_counts_229() {
        check_metrics::<PythonParser>(
            "def f(xs):
    for x in xs:
        if x < 0:
            break
    else:
        return True
    return False
",
            "foo.py",
            |metric| {
                // fn body has: for(1) + if(1) + for/else(1) = 3 over base 1 -> max = 4
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
            },
        );
    }

    /// Symmetric to for/else: while/else also runs only on normal
    /// completion of the loop.
    ///
    /// Expected: unit(1) + fn(1) + while(1) + else(1) = 4.
    #[test]
    fn python_while_else_still_counts_229() {
        check_metrics::<PythonParser>(
            "def f(n):
    while n > 0:
        n -= 1
    else:
        return True
    return False
",
            "foo.py",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    /// try/except/else: the `else` body runs only when no exception
    /// was raised in `try`, mirroring loop-`else` semantics. Counts
    /// alongside the `except` arm.
    ///
    /// Expected: unit(1) + fn(1) + except(1) + try/else(1) = 4.
    #[test]
    fn python_try_except_else_counts_229() {
        check_metrics::<PythonParser>(
            "def f():
    try:
        x = risky()
    except ValueError:
        x = -1
    else:
        x = x + 1
    return x
",
            "foo.py",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    /// `with` is unconditional resource management, not a branch, so it
    /// must not add to cyclomatic complexity — matching the C-family
    /// `using` sibling and textbook McCabe. Regression test for #418.
    ///
    /// Expected: unit(1) + fn(1) = 2; the `with` adds nothing.
    #[test]
    fn python_with_is_not_a_decision_point_418() {
        check_metrics::<PythonParser>(
            "def f():
    with open('a') as fp:
        return fp.read()
",
            "foo.py",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 1.0);
            },
        );
    }

    /// A `with` managing multiple context managers (`with a, b:`) parses
    /// as a single `with_statement` with one `with` keyword token, so it
    /// stays uncounted just like the single-manager form. Companion to
    /// #418.
    ///
    /// Expected: unit(1) + fn(1) = 2.
    #[test]
    fn python_with_multiple_managers_is_not_a_decision_point_418() {
        check_metrics::<PythonParser>(
            "def f(a, b):
    with a, b:
        return 1
",
            "foo.py",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
            },
        );
    }

    /// `async with` reuses the same `with` keyword token as plain
    /// `with`, so dropping `With` from the decision arm stops counting
    /// it too. Companion to #418.
    ///
    /// Expected: unit(1) + fn(1) = 2; neither `async` nor `with` counts.
    #[test]
    fn python_async_with_is_not_a_decision_point_418() {
        check_metrics::<PythonParser>(
            "async def f():
    async with open('a') as fp:
        return fp.read()
",
            "foo.py",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
            },
        );
    }

    /// Dropping `With` must not suppress real branches *inside* a `with`
    /// body: an `if` in the body still counts. Guards against an
    /// over-broad fix. Companion to #418.
    ///
    /// Expected: unit(1) + fn(1) + if(1) = 3; the `with` adds nothing.
    #[test]
    fn python_with_body_branch_still_counts_418() {
        check_metrics::<PythonParser>(
            "def f(x):
    with open('a') as fp:
        if x:
            return fp.read()
        return None
",
            "foo.py",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
            },
        );
    }

    #[test]
    fn python_simple_function() {
        check_metrics::<PythonParser>(
            "def f(a, b): # +2 (+1 unit space)
                if a and b:  # +2 (+1 and)
                   return 1
                if c and d: # +2 (+1 and)
                   return 1",
            "foo.py",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0,
                      "modified": {
                        "sum": 6.0,
                        "average": 3.0,
                        "min": 1.0,
                        "max": 5.0
                      }
                    }"###
                );
            },
        );
    }

    /// Python `match`/`case` (PEP 634, 3.10+): each non-bare-wildcard
    /// arm contributes one standard decision; the containing
    /// `match_statement` contributes one modified decision. A bare
    /// `case _:` (no guard) is skipped, mirroring Rust's `MatchArm`
    /// bare-wildcard filter. Regression test for #212.
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
                // standard: 1 (unit) + 1 (fn) + 1 (case 1; case _ skipped) = 3
                // modified: 1 (unit) + 1 (fn) + 1 (match_statement) = 3
                // function space alone holds 1 decision -> max = 2
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// `case _ if guard:` still counts because the guard is an
    /// `if_clause` sibling on the `case_clause`, escaping the bare-
    /// wildcard filter. The guard's own `if` keyword token is also
    /// counted via the existing `If` arm (every `if` keyword in
    /// Python contributes a decision) — long-standing behaviour
    /// shared with regular `if` statements. Companion to the
    /// `python_match_case_guarded_wildcard_counts` test in `abc.rs`.
    #[test]
    fn python_match_guarded_wildcard_counts() {
        check_metrics::<PythonParser>(
            "def f(x):
    match x:
        case 1:
            return 'one'
        case _ if x > 0:
            return 'positive'
        case _:
            return 'other'
",
            "foo.py",
            |metric| {
                // standard: 1 (unit) + 1 (fn) + 1 (case 1)
                //         + 1 (guarded `case _ if ...` — bare-_ filter
                //              escaped by the guard)
                //         + 1 (`if` keyword inside the guard)
                //         = 5; bare `case _:` is filtered.
                // modified: 1 (unit) + 1 (fn) + 1 (match_statement)
                //         + 1 (`if` keyword in the guard) = 4.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn python_1_level_nesting() {
        check_metrics::<PythonParser>(
            "def f(a, b): # +2 (+1 unit space)
                if a:  # +1
                    for i in range(b):  # +1
                        return 1",
            "foo.py",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn rust_1_level_nesting() {
        check_metrics::<RustParser>(
            "fn f() { // +2 (+1 unit space)
                 if true { // +1
                     match true {
                         true => println!(\"test\"), // +1
                         false => println!(\"test\"), // +1
                     }
                 }
             }",
            "foo.rs",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 5.0,
                  "average": 2.5,
                  "min": 1.0,
                  "max": 4.0,
                  "modified": {
                    "sum": 4.0,
                    "average": 2.0,
                    "min": 1.0,
                    "max": 3.0
                  }
                }
                "#
                );
            },
        );
    }

    /// Modified CCN: a match with N arms counts as 1 decision, not N.
    /// Bare `_ =>` wildcard arm does not count toward standard CCN (same
    /// as C-family `default:`).
    #[test]
    fn rust_match_modified() {
        check_metrics::<RustParser>(
            "fn f(x: u8) -> &'static str { // standard: +1 (unit) +1 (fn) +2 (arms 1,2) = 4; modified: +1 (unit) +1 (fn) +1 (MatchExpr) = 3
                 match x {
                     1 => \"one\",
                     2 => \"two\",
                     _ => \"other\",
                 }
             }",
            "foo.rs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    // The `?` operator (TryExpression) is the configurable arm (#409).
    // Fixture has exactly N=3 `?` operators in a single function. With
    // counting on (the default) each adds +1 to both standard and
    // modified; with counting off they add nothing. The two runs must
    // therefore differ by exactly N on both sub-metrics.
    const RUST_TRY_FIXTURE: &str = "fn f(s: &str) -> Result<i64, std::num::ParseIntError> {
             let a: i64 = s.parse()?;
             let b: i64 = s.parse()?;
             let c: i64 = s.parse()?;
             Ok(a + b + c)
         }";
    const RUST_TRY_COUNT: f64 = 3.0;

    fn rust_cyclomatic_with_try(count_try: bool) -> super::Stats {
        let func_space = crate::analyze(
            crate::Source::new(crate::LANG::Rust, RUST_TRY_FIXTURE.as_bytes())
                .with_name(Some("try.rs".to_owned())),
            crate::MetricsOptions::default().with_count_cyclomatic_try(count_try),
        )
        .expect("analyze must succeed on a well-formed Rust fixture");
        func_space.metrics.cyclomatic
    }

    #[test]
    fn rust_try_toggle_differs_by_exactly_n() {
        let with = rust_cyclomatic_with_try(true);
        let without = rust_cyclomatic_with_try(false);

        // Headline acceptance (#409): the toggle's whole effect is the N
        // `?` operators, on both standard and modified cyclomatic.
        assert_eq!(
            with.cyclomatic_sum() - without.cyclomatic_sum(),
            RUST_TRY_COUNT,
            "standard cyclomatic must drop by exactly N when `?` is not counted"
        );
        assert_eq!(
            with.cyclomatic_modified_sum() - without.cyclomatic_modified_sum(),
            RUST_TRY_COUNT,
            "modified cyclomatic must drop by exactly N when `?` is not counted"
        );
        // Guard against a no-op toggle: the two runs must actually differ.
        assert_ne!(with.cyclomatic_sum(), without.cyclomatic_sum());
    }

    #[test]
    fn rust_try_default_counts() {
        // The default (no options) must keep counting `?`, preserving
        // every published metric value (#409). Equivalent to the
        // `count_try == true` run above.
        let default_path = {
            let func_space = crate::analyze(
                crate::Source::new(crate::LANG::Rust, RUST_TRY_FIXTURE.as_bytes())
                    .with_name(Some("try.rs".to_owned())),
                crate::MetricsOptions::default(),
            )
            .expect("analyze must succeed on a well-formed Rust fixture");
            func_space.metrics.cyclomatic
        };
        let explicit_on = rust_cyclomatic_with_try(true);
        assert_eq!(default_path.cyclomatic_sum(), explicit_on.cyclomatic_sum());
        assert_eq!(
            default_path.cyclomatic_modified_sum(),
            explicit_on.cyclomatic_modified_sum()
        );
        // unit(1) + fn(entry 1 + 3*`?` = 4) = 5 standard; modified same
        // shape (no match container here): 1 + 4 = 5.
        assert_eq!(default_path.cyclomatic_sum(), 5.0);
        assert_eq!(default_path.cyclomatic_modified_sum(), 5.0);
    }

    #[test]
    fn c_switch() {
        check_metrics::<CppParser>(
            "void f() { // +2 (+1 unit space)
                 switch (1) {
                     case 1: // +1
                         printf(\"one\");
                         break;
                     case 2: // +1
                         printf(\"two\");
                         break;
                     case 3: // +1
                         printf(\"three\");
                         break;
                     default:
                         printf(\"all\");
                         break;
                 }
             }",
            "foo.c",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Modified CCN: 3 case arms in one switch collapse to 1 decision.
    #[test]
    fn c_switch_modified() {
        check_metrics::<CppParser>(
            "void f() {
                 switch (x) {
                     case 1: break;
                     case 2: break;
                     case 3: break;
                     default: break;
                 }
             }",
            "foo.c",
            |metric| {
                // standard: unit(1) + fn(1) + 3 cases = 5
                // modified: unit(1) + fn(1) + switch(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_real_function() {
        check_metrics::<CppParser>(
            "int sumOfPrimes(int max) { // +2 (+1 unit space)
                 int total = 0;
                 OUT: for (int i = 1; i <= max; ++i) { // +1
                   for (int j = 2; j < i; ++j) { // +1
                       if (i % j == 0) { // +1
                          continue OUT;
                       }
                   }
                   total += i;
                 }
                 return total;
            }",
            "foo.c",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 2.5,
                        "min": 1.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn c_unit_before() {
        check_metrics::<CppParser>(
            "
            int a=42;
            if(a==42) //+2(+1 unit space)
            {

            }
            if(a==34) //+1
            {

            }
            int sumOfPrimes(int max) { // +1
                 int total = 0;
                 OUT: for (int i = 1; i <= max; ++i) { // +1
                   for (int j = 2; j < i; ++j) { // +1
                       if (i % j == 0) { // +1
                          continue OUT;
                       }
                   }
                   total += i;
                 }
                 return total;
            }",
            "foo.c",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 7.0,
                      "average": 3.5,
                      "min": 3.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 7.0,
                        "average": 3.5,
                        "min": 3.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    /// Test to handle the case of min and max when merge happen before the final value of one module are set.
    /// In this case the min value should be 3 because the unit space has 2 branches and a complexity of 3
    /// while the function sumOfPrimes has a complexity of 4.
    #[test]
    fn c_unit_after() {
        check_metrics::<CppParser>(
            "
            int sumOfPrimes(int max) { // +1
                 int total = 0;
                 OUT: for (int i = 1; i <= max; ++i) { // +1
                   for (int j = 2; j < i; ++j) { // +1
                       if (i % j == 0) { // +1
                          continue OUT;
                       }
                   }
                   total += i;
                 }
                 return total;
            }

            int a=42;
            if(a==42) //+2(+1 unit space)
            {

            }
            if(a==34) //+1
            {

            }",
            "foo.c",
            |metric| {
                // nspace = 2 (func and unit)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 7.0,
                      "average": 3.5,
                      "min": 3.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 7.0,
                        "average": 3.5,
                        "min": 3.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_simple_class() {
        check_metrics::<JavaParser>(
            "
            public class Example { // +2 (+1 unit space)
                int a = 10;
                boolean b = (a > 5) ? true : false; // +1
                boolean c = b && true; // +1

                public void m1() { // +1
                    if (a % 2 == 0) { // +1
                        b = b || c; // +1
                    }
                }
                public void m2() { // +1
                    while (a > 3) { // +1
                        m1();
                        a--;
                    }
                }
            }",
            "foo.java",
            |metric| {
                // nspace = 4 (unit, class and 2 methods)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 9.0,
                      "average": 2.25,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 9.0,
                        "average": 2.25,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn java_real_class() {
        check_metrics::<JavaParser>(
            "
            public class Matrix { // +2 (+1 unit space)
                private int[][] m = new int[5][5];

                public void init() { // +1
                    for (int i = 0; i < m.length; i++) { // +1
                        for (int j = 0; j < m[i].length; j++) { // +1
                            m[i][j] = i * j;
                        }
                    }
                }
                public int compute(int i, int j) { // +1
                    try {
                        return m[i][j] / m[j][i];
                    } catch (ArithmeticException e) { // +1
                        return -1;
                    } catch (ArrayIndexOutOfBoundsException e) { // +1
                        return -2;
                    }
                }
                public void print(int result) { // +1
                    switch (result) {
                        case -1: // +1
                            System.out.println(\"Division by zero\");
                            break;
                        case -2: // +1
                            System.out.println(\"Wrong index number\");
                            break;
                        default:
                            System.out.println(\"The result is \" + result);
                    }
                }
            }",
            "foo.java",
            |metric| {
                // nspace = 5 (unit, class and 3 methods)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 11.0,
                      "average": 2.2,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 10.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    /// Modified CCN: Java switch with 2 cases counts as 1 (not 2).
    #[test]
    fn java_switch_modified() {
        check_metrics::<JavaParser>(
            "public class A {
                public void print(int result) {
                    switch (result) {
                        case -1:
                            System.out.println(\"minus one\");
                            break;
                        case -2:
                            System.out.println(\"minus two\");
                            break;
                        default:
                            System.out.println(\"other\");
                    }
                }
            }",
            "foo.java",
            |metric| {
                // standard: unit(1) + class(1) + fn(1) + 2 cases = 5
                // modified: unit(1) + class(1) + fn(1) + switch(1) = 4
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 1.6666666666666667,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 1.3333333333333333,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_simple_class() {
        check_metrics::<CsharpParser>(
            "public class Example {
                int a = 10;
                bool b = (a > 5) ? true : false;
                bool c = b && true;

                public void M1() {
                    if (a % 2 == 0) {
                        b = b || c;
                    }
                }
                public void M2() {
                    while (a > 3) {
                        M1();
                        a--;
                    }
                }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 9.0,
                      "average": 2.25,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 9.0,
                        "average": 2.25,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_real_class() {
        check_metrics::<CsharpParser>(
            "public class Matrix {
                private int[,] m = new int[5, 5];

                public void Init() {
                    for (int i = 0; i < 5; i++) {
                        for (int j = 0; j < 5; j++) {
                            m[i, j] = i * j;
                        }
                    }
                }
                public int Compute(int i, int j) {
                    try {
                        return m[i, j] / m[j, i];
                    } catch (System.DivideByZeroException) {
                        return -1;
                    } catch (System.IndexOutOfRangeException) {
                        return -2;
                    }
                }
                public void Print(int result) {
                    switch (result) {
                        case -1:
                            System.Console.WriteLine(\"Division by zero\");
                            break;
                        case -2:
                            System.Console.WriteLine(\"Wrong index number\");
                            break;
                        default:
                            System.Console.WriteLine(\"The result is \" + result);
                            break;
                    }
                }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 11.0,
                      "average": 2.2,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 10.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_anonymous_method() {
        check_metrics::<CsharpParser>(
            "public class A {
                public void M() {
                    System.Action f = delegate(int x) {
                        if (x > 0) {
                            System.Console.WriteLine(x);
                        }
                    };
                }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 1.25,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 1.25,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_switch_expression_arms() {
        // Each non-default arm of a switch_expression contributes +1.
        // The discard arm `_ =>` is excluded (issue #282), mirroring
        // Rust's `_ =>` and Java/C#'s `default:` treatment.
        check_metrics::<CsharpParser>(
            "public class A {
                public string Name(int n) =>
                    n switch {
                        1 => \"one\",
                        2 => \"two\",
                        3 => \"three\",
                        _ => \"other\"
                    };
            }",
            "foo.cs",
            |metric| {
                // expected: unit(1) + class(1) + fn(base 1 + 3 explicit arms;
                //           `_ =>` skipped) = sum 6, max 4. modified =
                //           unit(1) + class(1) + fn(base 1 + switch expr 1) = 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 6.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 1.3333333333333333,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Regression #282: the bare discard arm `_ =>` in a C# switch
    /// expression must NOT contribute to standard CCN, mirroring the
    /// C-family `default:` rule.
    #[test]
    fn csharp_switch_expression_discard_arm_not_counted() {
        check_metrics::<CsharpParser>(
            "public class A {
                public string Name(int n) =>
                    n switch {
                        1 => \"one\",
                        _ => \"other\"
                    };
            }",
            "foo.cs",
            |metric| {
                // expected: unit(1) + class(1) + fn(base 1 + 1 explicit;
                //           `_ =>` skipped) = 4, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
            },
        );
    }

    /// Regression #282: `var _` is also a discard pattern and must be
    /// excluded from standard CCN.
    #[test]
    fn csharp_switch_expression_var_underscore_not_counted() {
        check_metrics::<CsharpParser>(
            "public class A {
                public string Name(int n) =>
                    n switch {
                        1 => \"one\",
                        var _ => \"other\"
                    };
            }",
            "foo.cs",
            |metric| {
                // expected: unit(1) + class(1) + fn(base 1 + 1 explicit;
                //           `var _ =>` skipped) = 4, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
            },
        );
    }

    /// Regression #282: a guarded discard arm `_ when g => …` is NOT a
    /// bare wildcard — the `when` guard adds a non-trivial decision —
    /// so the arm still contributes one standard decision, mirroring
    /// Rust's `_ if g` rule.
    #[test]
    fn csharp_switch_expression_guarded_discard_still_counts() {
        check_metrics::<CsharpParser>(
            "public class A {
                public string Name(int n) =>
                    n switch {
                        1 => \"one\",
                        _ when n > 10 => \"big\",
                        _ => \"other\"
                    };
            }",
            "foo.cs",
            |metric| {
                // expected: unit(1) + class(1) + fn(base 1 + 1 explicit +
                //           1 guarded discard; bare `_ =>` skipped) = 5,
                //           max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    /// Regression #303 / #282: a typed-discard arm `int _ =>` is NOT
    /// a bare discard — the type test (`predefined_type`) is a
    /// non-trivial decision — so the arm still contributes one
    /// standard decision. Locks in the
    /// `DeclarationPattern → _ => return NotDiscard` catch-all in
    /// `csharp_switch_expression_arm_is_bare_discard`.
    #[test]
    fn csharp_switch_expression_typed_discard_still_counts() {
        check_metrics::<CsharpParser>(
            "public class A {
                public string Name(object n) =>
                    n switch {
                        1 => \"one\",
                        int _ => \"int\",
                        _ => \"other\"
                    };
            }",
            "foo.cs",
            |metric| {
                // expected: unit(1) + class(1) + fn(base 1 + 1 explicit `1` +
                //           1 typed-discard `int _`; bare `_ =>` skipped) = 5,
                //           max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    /// Regression #303 / #282: a guarded `var _ when g =>` is NOT a
    /// bare discard — the `when` guard adds a non-trivial decision —
    /// so the arm still contributes one standard decision. Exercises
    /// the `DeclarationPattern` arm of `classify_pattern` combined
    /// with the post-pattern `WhenClause` sweep.
    #[test]
    fn csharp_switch_expression_guarded_var_underscore_still_counts() {
        check_metrics::<CsharpParser>(
            "public class A {
                public string Name(int n) =>
                    n switch {
                        1 => \"one\",
                        var _ when n > 10 => \"big\",
                        _ => \"other\"
                    };
            }",
            "foo.cs",
            |metric| {
                // expected: unit(1) + class(1) + fn(base 1 + 1 explicit `1` +
                //           1 guarded `var _`; bare `_ =>` skipped) = 5,
                //           max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    /// Modified CCN: C# switch statement with 2 cases counts as 1.
    #[test]
    fn csharp_switch_modified() {
        check_metrics::<CsharpParser>(
            "public class A {
                public string Describe(int n) {
                    switch (n) {
                        case 1:
                            return \"one\";
                        case 2:
                            return \"two\";
                        default:
                            return \"other\";
                    }
                }
            }",
            "foo.cs",
            |metric| {
                // standard: unit(1) + class(1) + fn(1) + 2 cases = 5
                // modified: unit(1) + class(1) + fn(1) + switch(1) = 4
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 1.6666666666666667,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 1.3333333333333333,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn csharp_null_coalescing_and_conditional_access() {
        // Each `??` and `?.` is +1 cyclomatic.
        check_metrics::<CsharpParser>(
            "public class A {
                public int? Get(string s, A b) {
                    return s?.Length ?? b?.Get(null, null) ?? 0;
                }
            }",
            "foo.cs",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 7.0,
                      "average": 2.3333333333333335,
                      "min": 1.0,
                      "max": 5.0,
                      "modified": {
                        "sum": 7.0,
                        "average": 2.3333333333333335,
                        "min": 1.0,
                        "max": 5.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_simple_function() {
        check_metrics::<JavascriptParser>(
            "function f(a, b) { // +2 (+1 unit space)
                 if (a) { // +1
                     return a;
                 } else if (b) { // +1
                     return b;
                 }
                 return 0;
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_switch() {
        check_metrics::<JavascriptParser>(
            "function f() { // +2 (+1 unit space)
                 switch (x) {
                     case 1: // +1
                         console.log(\"one\");
                         break;
                     case 2: // +1
                         console.log(\"two\");
                         break;
                     case 3: // +1
                         console.log(\"three\");
                         break;
                     default:
                         console.log(\"other\");
                         break;
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Modified CCN: JS switch with 3 cases collapses to 1.
    #[test]
    fn javascript_switch_modified() {
        check_metrics::<JavascriptParser>(
            "function f(x) {
                 switch (x) {
                     case 1: return 'one';
                     case 2: return 'two';
                     case 3: return 'three';
                 }
             }",
            "foo.js",
            |metric| {
                // standard: unit(1) + fn(1) + 3 cases = 5
                // modified: unit(1) + fn(1) + switch(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_simple_function() {
        check_metrics::<GoParser>(
            "package main
            func f() {}",
            "foo.go",
            |metric| {
                // nspace = 2 (file unit + func), each base 1.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 1.0,
                      "max": 1.0,
                      "modified": {
                        "sum": 2.0,
                        "average": 1.0,
                        "min": 1.0,
                        "max": 1.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_if_else() {
        check_metrics::<GoParser>(
            "package main
            func f(x bool) { // +2 (+1 unit)
                if x { // +1
                } else {
                }
            }",
            "foo.go",
            |metric| {
                // `else` clause attaches to the same if_statement node and is
                // not counted again.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_else_if_chain() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) { // +2 (+1 unit)
                if x > 0 { // +1
                } else if x < 0 { // +1 (nested if_statement)
                } else if x == 0 { // +1 (nested if_statement)
                } else {
                }
            }",
            "foo.go",
            |metric| {
                // tree-sitter-go represents `else if` as a nested
                // if_statement under the parent's `else` clause; each nested
                // if contributes +1.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 2.5,
                        "min": 1.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_for_loop() {
        check_metrics::<GoParser>(
            "package main
            func f() { // +2 (+1 unit)
                for i := 0; i < 10; i++ { // +1
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_for_range() {
        check_metrics::<GoParser>(
            "package main
            func f(xs []int) { // +2 (+1 unit)
                for _, v := range xs { // +1
                    _ = v
                }
            }",
            "foo.go",
            |metric| {
                // range_clause is a child of for_statement; only the
                // for_statement contributes.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_switch() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) { // +2 (+1 unit)
                switch x {
                case 1: // +1
                case 2: // +1
                default: // not counted
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Modified CCN: Go switch with 3 cases collapses to 1.
    #[test]
    fn go_switch_modified() {
        check_metrics::<GoParser>(
            "package main
            func f(x int) {
                switch x {
                case 1:
                    println(\"one\")
                case 2:
                    println(\"two\")
                case 3:
                    println(\"three\")
                }
            }",
            "foo.go",
            |metric| {
                // standard: unit(1) + fn(1) + 3 cases = 5
                // modified: unit(1) + fn(1) + switch(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_type_switch() {
        check_metrics::<GoParser>(
            "package main
            func f(x interface{}) { // +2 (+1 unit)
                switch x.(type) {
                case int: // +1
                case string: // +1
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_select() {
        check_metrics::<GoParser>(
            "package main
            func f(c1, c2 chan int) { // +2 (+1 unit)
                select {
                case <-c1: // +1
                case <-c2: // +1
                default: // not counted
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_logical_operators() {
        check_metrics::<GoParser>(
            "package main
            func f(a, b, c bool) { // +2 (+1 unit)
                if a && b || c { // +1 if, +1 &&, +1 ||
                }
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 2.5,
                        "min": 1.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn go_defer_and_go_do_not_count() {
        check_metrics::<GoParser>(
            "package main
            func f() { // +2 (+1 unit)
                defer cleanup()
                go work()
            }",
            "foo.go",
            |metric| {
                // defer_statement and go_statement are not branches.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 1.0,
                      "max": 1.0,
                      "modified": {
                        "sum": 2.0,
                        "average": 1.0,
                        "min": 1.0,
                        "max": 1.0
                      }
                    }"###
                );
            },
        );
    }

    // As reported here:
    // https://github.com/sebastianbergmann/php-code-coverage/issues/607
    // An anonymous class declaration is not considered when computing the Cyclomatic Complexity metric for Java
    // Only the complexity of the anonymous class content is considered for the computation
    #[test]
    fn java_anonymous_class() {
        check_metrics::<JavaParser>(
            "
            abstract class A { // +2 (+1 unit space)
                public abstract boolean m1(int n); // +1
                public abstract boolean m2(int n); // +1
            }
            public class B { // +1
                public void test() { // +1
                    A a = new A() {
                        public boolean m1(int n) { // +1
                            if (n % 2 == 0) { // +1
                                return true;
                            }
                            return false;
                        }
                        public boolean m2(int n) { // +1
                            if (n % 5 == 0) { // +1
                                return true;
                            }
                            return false;
                        }
                    };
                }
            }",
            "foo.java",
            |metric| {
                // nspace = 8 (unit, 2 classes and 5 methods)
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 10.0,
                      "average": 1.25,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 10.0,
                        "average": 1.25,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Java `do { … } while (…)` contributes exactly +1 to both
    /// standard and modified CCN. The +1 comes from the `while`
    /// keyword token (`Java::While`) inside the do-statement, which
    /// the dedicated `JavaCode` impl already counts. Adding
    /// `Java::DoStatement` would double-count — see issue #284. This
    /// test pins the correct keyword-driven count.
    #[test]
    fn java_do_statement_counts_in_cyclomatic() {
        check_metrics::<JavaParser>(
            "class Parity {
                 static void f() {
                     int i = 0;
                     do {           // +1 (via inner `while` keyword)
                         ++i;
                     } while (i < 10);
                 }
             }",
            "foo.java",
            |metric| {
                // standard: unit(1) + class(1) + method(1) + do(1) = 4
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 4.0);
                assert_eq!(s.cyclomatic_max(), 2.0);
                assert_eq!(s.cyclomatic_modified_sum(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 1.3333333333333333,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 1.3333333333333333,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Java enhanced-for `for (T x : xs)` contributes exactly +1 to
    /// both standard and modified CCN — the `for` keyword token
    /// (`Java::For`) fires inside the `EnhancedForStatement` node
    /// just like inside a classic `ForStatement`. Pinning this
    /// prevents reintroducing the double-count from issue #284's
    /// incorrect fix proposal.
    #[test]
    fn java_enhanced_for_statement_counts_in_cyclomatic() {
        check_metrics::<JavaParser>(
            "class Parity {
                 static void f(int[] xs) {
                     for (int x : xs) {  // +1 (via `for` keyword)
                         g(x);
                     }
                 }
             }",
            "foo.java",
            |metric| {
                // standard: unit(1) + class(1) + method(1) + enhanced-for(1) = 4
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 4.0);
                assert_eq!(s.cyclomatic_max(), 2.0);
                assert_eq!(s.cyclomatic_modified_sum(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 1.3333333333333333,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 1.3333333333333333,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn groovy_simple_class() {
        check_metrics::<GroovyParser>(
            "
            class Example {
                int a = 10
                boolean b = (a > 5) ? true : false
                boolean c = b && true

                void m1() {
                    if (a % 2 == 0) {
                        b = b || c
                    }
                }
                void m2() {
                    while (a > 3) {
                        m1()
                        a--
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // Same shape as `java_simple_class`. nspace = 4
                // (unit, class, 2 methods); branches mirror Java's.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 9.0);
            },
        );
    }

    #[test]
    fn groovy_nested_control_flow() {
        check_metrics::<GroovyParser>(
            "void f(int x) {
                if (x > 0) {
                    while (x < 100) {
                        x = x + 1
                    }
                }
            }",
            "foo.groovy",
            |metric| {
                // unit(1) + fn(1) + if(1) + while(1) = 4
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
            },
        );
    }

    #[test]
    fn groovy_switch_with_cases() {
        check_metrics::<GroovyParser>(
            "void print(int result) {
                switch (result) {
                    case -1:
                        println 'minus one'
                        break
                    case -2:
                        println 'minus two'
                        break
                    default:
                        println 'other'
                }
            }",
            "foo.groovy",
            |metric| {
                // standard: unit(1) + fn(1) + 2 cases = 4
                // modified: unit(1) + fn(1) + switch(1) = 3
                // (default does NOT add a branch — same as Java/lesson #106)
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_try_catch() {
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
                // unit(1) + fn(1) + catch(1) = 3
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_closure_body_short_circuit() {
        // Top-level `def pred = { … }` collapses the closure into the
        // unit scope (no class wrapper), so the `&&` inside still
        // contributes one branch but no extra function space is
        // created. Mirrors Java's top-level-lambda behavior.
        check_metrics::<GroovyParser>(
            "def pred = { x -> x > 0 && x < 100 }",
            "foo.groovy",
            |metric| {
                // unit(1) + && (1) = 2
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 2.0);
            },
        );
    }

    #[test]
    fn groovy_assert_adds_branch() {
        // Groovy `assert` is a runtime check that branches on its
        // condition; mirror Sonar's standard-CCN treatment.
        check_metrics::<GroovyParser>(
            "void check(int x) {
                assert x > 0
            }",
            "foo.groovy",
            |metric| {
                // unit(1) + fn(1) + assert(1) = 3
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
            },
        );
    }

    /// Groovy `do { … } while (…)` contributes exactly +1 to both
    /// standard and modified CCN — the `while` keyword token
    /// (`Groovy::While`) inside the do-statement is already counted
    /// by the dedicated `GroovyCode` impl. Adding `Groovy::DoStatement`
    /// would double-count (issue #284). This test pins the correct
    /// keyword-driven count.
    #[test]
    fn groovy_do_statement_counts_in_cyclomatic() {
        check_metrics::<GroovyParser>(
            "def f() {
                 int i = 0
                 do {           // +1 (via inner `while` keyword)
                     ++i
                 } while (i < 10)
             }",
            "foo.groovy",
            |metric| {
                // standard: unit(1) + fn(1) + do(1) = 3
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 3.0);
                assert_eq!(s.cyclomatic_max(), 2.0);
                assert_eq!(s.cyclomatic_modified_sum(), 3.0);
            },
        );
    }

    /// Groovy enhanced-for `for (T x : xs)` contributes exactly +1 to
    /// both standard and modified CCN — the `for` keyword token
    /// (`Groovy::For`) fires inside `EnhancedForStatement` just like
    /// inside a classic `ForStatement`. Pinning this prevents
    /// reintroducing the double-count from issue #284's incorrect fix
    /// proposal.
    #[test]
    fn groovy_enhanced_for_statement_counts_in_cyclomatic() {
        check_metrics::<GroovyParser>(
            "def f(int[] xs) {
                 for (int x : xs) {  // +1 (via `for` keyword)
                     println(x)
                 }
             }",
            "foo.groovy",
            |metric| {
                // standard: unit(1) + fn(1) + enhanced-for(1) = 3
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 3.0);
                assert_eq!(s.cyclomatic_max(), 2.0);
                assert_eq!(s.cyclomatic_modified_sum(), 3.0);
            },
        );
    }

    #[test]
    fn groovy_safe_navigation_cyclomatic() {
        // Issue #452: Groovy's safe-navigation `?.` (QMARKDOT) is a
        // short-circuit decision point per link, mirroring the
        // Kotlin/PHP/JS/C# treatment of `?.` (#281). The chain
        // `a?.b?.c` adds +2 to both standard and modified CCN.
        check_metrics::<GroovyParser>("def read(a){ return a?.b?.c }", "foo.groovy", |metric| {
            // unit(1) + fn(base 1 + ?. 1 + ?. 1) = sum 4, max 3.
            let s = &metric.cyclomatic;
            assert_eq!(s.cyclomatic_sum(), 4.0);
            assert_eq!(s.cyclomatic_max(), 3.0);
            assert_eq!(s.cyclomatic_modified_sum(), 4.0);
            assert_eq!(s.cyclomatic_modified_max(), 3.0);
        });
    }

    #[test]
    fn groovy_safe_chain_dot_cyclomatic() {
        // Issue #452: Groovy's `??.` (QMARKQMARKDOT, the spread-safe
        // chain-dot operator) is also a short-circuit decision point,
        // counted once per occurrence like `?.`.
        check_metrics::<GroovyParser>("def read(a){ return a??.b }", "foo.groovy", |metric| {
            // unit(1) + fn(base 1 + ??. 1) = sum 3, max 2.
            let s = &metric.cyclomatic;
            assert_eq!(s.cyclomatic_sum(), 3.0);
            assert_eq!(s.cyclomatic_max(), 2.0);
            assert_eq!(s.cyclomatic_modified_sum(), 3.0);
            assert_eq!(s.cyclomatic_modified_max(), 2.0);
        });
    }

    #[test]
    fn perl_nested_control_flow() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                for my $i (1..10) { # +1 for_statement_2
                    if ($i % 2) { # +1 if_statement
                        print $i;
                    }
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0,
                  "modified": {
                    "sum": 4.0,
                    "average": 2.0,
                    "min": 1.0,
                    "max": 3.0
                  }
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_postfix_conditionals() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                return 1 if $_[0]; # +1 if_simple_statement
                return 0 unless $_[1]; # +1 unless_simple_statement
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0,
                  "modified": {
                    "sum": 4.0,
                    "average": 2.0,
                    "min": 1.0,
                    "max": 3.0
                  }
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_unless_and_until() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                unless ($x) { # +1 unless_statement
                    print 'a';
                }
                until ($n == 0) { # +1 until_statement
                    $n--;
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0,
                  "modified": {
                    "sum": 4.0,
                    "average": 2.0,
                    "min": 1.0,
                    "max": 3.0
                  }
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_logical_operators_and_ternary() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                my $x = $a && $b; # +1 (&&)
                my $y = $c || $d; # +1 (||)
                my $z = $e // $f; # +1 (//)
                my $t = $g ? 1 : 0; # +1 ternary
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 6.0,
                  "average": 3.0,
                  "min": 1.0,
                  "max": 5.0,
                  "modified": {
                    "sum": 6.0,
                    "average": 3.0,
                    "min": 1.0,
                    "max": 5.0
                  }
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_word_logical_operators() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                my $x = $a and $b; # +1 (and)
                my $y = $c or $d; # +1 (or)
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0,
                  "modified": {
                    "sum": 4.0,
                    "average": 2.0,
                    "min": 1.0,
                    "max": 3.0
                  }
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_compound_short_circuit_assignment_249() {
        // Regression for issue #249: `&&=`, `||=`, `//=` are each one
        // short-circuit decision edge — semantically `$x = $x op $y`.
        // Perl exposes the operator token inside `binary_expression`,
        // so adding the three `*EQ` tokens to the cyclomatic arm picks
        // them up alongside the bare `&&` / `||` / `//`.
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                my ($x, $y, $z) = @_;
                $x ||= 1; # +1 (||=)
                $y &&= 2; # +1 (&&=)
                $z //= 3; # +1 (//=)
                return $x;
            }",
            "foo.pl",
            |metric| {
                // unit(1) + fn(entry 1 + 3 assignments = 4) = sum 5, max 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 2.5,
                        "min": 1.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn perl_foreach_loop() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                foreach my $i (@list) { # +1 for_statement_2
                    print $i;
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic, @r#"
                {
                  "sum": 3.0,
                  "average": 1.5,
                  "min": 1.0,
                  "max": 2.0,
                  "modified": {
                    "sum": 3.0,
                    "average": 1.5,
                    "min": 1.0,
                    "max": 2.0
                  }
                }
                "#);
            },
        );
    }

    #[test]
    fn perl_else_does_not_count_but_elsif_does() {
        check_metrics::<PerlParser>(
            "sub f { # +1 (unit) +1 (sub)
                if ($x) { # +1 if_statement
                    print 'a';
                } elsif ($y) { # +1 elsif_clause
                    print 'b';
                } else {
                    print 'c';
                }
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0,
                  "modified": {
                    "sum": 4.0,
                    "average": 2.0,
                    "min": 1.0,
                    "max": 3.0
                  }
                }
                 "#
                );
            },
        );
    }

    #[test]
    fn tsx_simple_function() {
        check_metrics::<TsxParser>(
            "function f(a: number, b: number) { // +2 (+1 unit space)
                 if (a > 0) { // +1
                     return a;
                 } else if (b > 0) { // +1
                     return b;
                 }
                 return 0;
             }",
            "foo.tsx",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_if_else_and_switch() {
        check_metrics::<TypescriptParser>(
            "function classify(value: number): string {
                 if (value < 0) { // +1
                     return 'negative';
                 } else if (value === 0) { // +1
                     return 'zero';
                 }
                 switch (value) {
                     case 1: // +1
                         return 'one';
                     case 2: // +1
                         return 'two';
                     default:
                         return 'other';
                 }
             }",
            "foo.ts",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 2.5,
                        "min": 1.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    /// Modified CCN: TypeScript switch with 3 cases collapses to 1.
    #[test]
    fn typescript_switch_modified() {
        check_metrics::<TypescriptParser>(
            "function f(x: number): string {
                 switch (x) {
                     case 1: return 'one';
                     case 2: return 'two';
                     case 3: return 'three';
                     default: return 'other';
                 }
             }",
            "foo.ts",
            |metric| {
                // standard: unit(1) + fn(1) + 3 cases = 5
                // modified: unit(1) + fn(1) + switch(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_if_else_and_switch() {
        check_metrics::<MozjsParser>(
            "function f(x) { // +2 (+1 unit space)
                 if (x > 0) { // +1
                     return 1;
                 } else if (x < 0) { // +1
                     return -1;
                 }
                 switch (x) {
                     case 0: // +1
                         return 0;
                     case 42: // +1
                         return 42;
                     default:
                         return -2;
                 }
             }",
            "foo.js",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 2.5,
                        "min": 1.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    /// Modified CCN: MozJS switch with 2 cases collapses to 1.
    #[test]
    fn mozjs_switch_modified() {
        check_metrics::<MozjsParser>(
            "function f(x) {
                 switch (x) {
                     case 1: return 1;
                     case 2: return 2;
                 }
             }",
            "foo.js",
            |metric| {
                // standard: unit(1) + fn(1) + 2 cases = 4
                // modified: unit(1) + fn(1) + switch(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn kotlin_cyclomatic_mixed() {
        check_metrics::<KotlinParser>(
            "class Calc {
                fun compute(x: Int, y: Int): Int {
                    if (x > 0) {            // +1
                        for (i in 1..x) {   // +1
                            println(i)
                        }
                    }
                    when (y) {
                        1 -> println(\"one\")  // +1 (WhenEntry)
                        2 -> println(\"two\")  // +1
                        else -> println(\"?\") // skipped (else is default)
                    }
                    val ok = x > 0 && y > 0  // +1
                    try {
                        println(x / y)
                    } catch (e: Exception) { // +1
                        println(\"err\")
                    }
                    return x + y
                }
            }",
            "foo.kt",
            |metric| {
                // expected: unit(1) + class(1) + fn(base 1 + if 1 + for 1 +
                //           2 explicit when arms; else skipped + && 1 +
                //           catch 1) = sum 9, max 7.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 9.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 7.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 9.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 7.0,
                      "modified": {
                        "sum": 8.0,
                        "average": 2.6666666666666665,
                        "min": 1.0,
                        "max": 6.0
                      }
                    }
                    "###
                );
            },
        );
    }

    /// Modified CCN: Kotlin when with 3 entries collapses to 1.
    #[test]
    fn kotlin_when_modified() {
        check_metrics::<KotlinParser>(
            "fun describe(x: Int): String {
                 return when (x) {
                     1 -> \"one\"
                     2 -> \"two\"
                     3 -> \"three\"
                     else -> \"other\"
                 }
             }",
            "foo.kt",
            |metric| {
                // standard: unit(1) + fn(base 1 + 3 explicit when arms;
                //           else skipped per #282) = 5
                // modified: unit(1) + fn(1) + WhenExpression(1) = 3
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Regression #282: the `else -> …` arm in a Kotlin `when`
    /// expression must NOT contribute to standard CCN, mirroring the
    /// C-family `default:` rule.
    #[test]
    fn kotlin_when_else_arm_not_counted() {
        check_metrics::<KotlinParser>(
            "fun describe(x: Int): String {
                 return when (x) {
                     1 -> \"one\"
                     else -> \"other\"
                 }
             }",
            "foo.kt",
            |metric| {
                // expected: unit(1) + fn(base 1 + 1 explicit; else skipped) = 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
            },
        );
    }

    /// Cross-check #282: every case-style arm in a Kotlin `when`
    /// contributes one standard decision; only the `else ->` arm is
    /// skipped. Pairs with `kotlin_when_else_arm_not_counted` (which
    /// pins the single-explicit case) to confirm the count scales
    /// linearly with explicit arms and is not accidentally hard-coded
    /// to one.
    #[test]
    fn kotlin_when_multiple_explicit_arms_each_count() {
        check_metrics::<KotlinParser>(
            "fun describe(x: Int): String {
                 return when (x) {
                     1 -> \"one\"
                     2 -> \"two\"
                     3 -> \"three\"
                     else -> \"other\"
                 }
             }",
            "foo.kt",
            |metric| {
                // expected: unit(1) + fn(base 1 + 3 explicit; else skipped) = 5, max 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
            },
        );
    }

    #[test]
    fn lua_1_level_nesting() {
        // chunk: base=1; f: base=1 + for=1 + if=1 = 3; sum=4
        check_metrics::<LuaParser>(
            "local function f(t)
  for i = 1, #t do
    if t[i] > 0 then
      return t[i]
    end
  end
  return 0
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic, @r###"
                {
                  "sum": 4.0,
                  "average": 2.0,
                  "min": 1.0,
                  "max": 3.0,
                  "modified": {
                    "sum": 4.0,
                    "average": 2.0,
                    "min": 1.0,
                    "max": 3.0
                  }
                }
                "###);
            },
        );
    }

    #[test]
    fn lua_elseif_branches() {
        // chunk: base=1; classify: base=1 + if=1 + elseif=1 + elseif=1 = 4
        // else does NOT add a branch; sum=5
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
                insta::assert_json_snapshot!(metric.cyclomatic, @r###"
                {
                  "sum": 5.0,
                  "average": 2.5,
                  "min": 1.0,
                  "max": 4.0,
                  "modified": {
                    "sum": 5.0,
                    "average": 2.5,
                    "min": 1.0,
                    "max": 4.0
                  }
                }
                "###);
            },
        );
    }

    #[test]
    fn lua_logical_operators() {
        // chunk: base=1; f: base=1 + if=1 + and=1 + or=1 = 4; sum=5
        check_metrics::<LuaParser>(
            "local function f(a, b, c)
  if a and b or c then
    return 1
  end
  return 0
end",
            "foo.lua",
            |metric| {
                insta::assert_json_snapshot!(metric.cyclomatic, @r###"
                {
                  "sum": 5.0,
                  "average": 2.5,
                  "min": 1.0,
                  "max": 4.0,
                  "modified": {
                    "sum": 5.0,
                    "average": 2.5,
                    "min": 1.0,
                    "max": 4.0
                  }
                }
                "###);
            },
        );
    }

    #[test]
    fn bash_nested_control_flow() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    if [ $1 -eq 1 ]; then
        for i in 1 2 3; do
            echo $i
        done
    elif [ $1 -eq 2 ]; then
        echo 'two'
    fi
}",
            "foo.sh",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    {".sum" => insta::rounded_redaction(2)}
                );
            },
        );
    }

    /// Regression test for #107: case…esac must not double-count the container.
    /// Standard CCN counts only arms (matching C-family `switch` semantics).
    /// Modified CCN counts only the container.
    #[test]
    fn bash_case_modified() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    case $1 in
        one)   echo 1 ;;
        two)   echo 2 ;;
        three) echo 3 ;;
    esac
}",
            "foo.sh",
            |metric| {
                // standard: unit(1) + fn(1) + 3 case_items = 5
                // modified: unit(1) + fn(1) + case_stmt(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn tcl_1_level_nesting() {
        // chunk: base=1; f: base=1 + while=1 + if=1 = 3; sum=4
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
                // unit(1) + proc(base 1 + while 1 + if 1) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tcl_elseif_branch() {
        // if=1, elseif=1; else does NOT add a branch; sum=3 (chunk base=1)
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
                // unit(1) + proc(base 1 + if 1 + elseif 1) = sum 4, max 3.
                // else does NOT add a branch.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tcl_logical_operators() {
        check_metrics::<TclParser>(
            "proc f {x y z} {
    if {$x > 0 && $y > 0 || $z > 0} {
        puts ok
    }
}",
            "foo.tcl",
            |metric| {
                // unit(1) + proc(base 1 + if 1 + && 1 + || 1) = sum 5, max 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tcl_catch_branch() {
        // `catch` command adds +1 (conditional handler); `try` does NOT add a branch.
        // source_file(1) + proc_space(base=1 + catch=1 = 2) = sum=3
        check_metrics::<TclParser>(
            "proc f {} {
    catch {
        expr {1 / 0}
    } msg
}",
            "foo.tcl",
            |metric| {
                // unit(1) + proc(base 1 + catch 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tcl_try_no_branch() {
        // `try` is NOT a conditional construct; it does not add cyclomatic complexity.
        // Only the base counts: source_file(1) + proc_space(base=1) = sum=2, average=1.
        check_metrics::<TclParser>(
            "proc f {} {
    try {
        expr {1 / 0}
    } finally {
        puts done
    }
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r#"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 1.0,
                      "max": 1.0,
                      "modified": {
                        "sum": 2.0,
                        "average": 1.0,
                        "min": 1.0,
                        "max": 1.0
                      }
                    }
                    "#
                );
            },
        );
    }

    #[test]
    fn mozjs_for_loop() {
        check_metrics::<MozjsParser>(
            "function f(n) { // +2 (+1 unit)
             var s = 0;
             for (var i = 0; i < n; i++) { // +1
                 s += i;
             }
             return s;
         }",
            "foo.js",
            |metric| {
                // unit(1) + fn(base 1 + for 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn mozjs_logical_operators() {
        check_metrics::<MozjsParser>(
            "function f(a, b, c) { // +2 (+1 unit)
             if (a && b || c) { // +1 if, +1 &&, +1 ||
                 return 1;
             }
             return 0;
         }",
            "foo.js",
            |metric| {
                // unit(1) + fn(base 1 + if 1 + && 1 + || 1) = sum 5, max 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn javascript_nullish_coalescing_chain_226() {
        // `??` is short-circuit and must count as
        // a decision point in cyclomatic complexity.  `a ?? b ?? c` adds two
        // `??` decisions on top of the function entry.
        check_metrics::<JavascriptParser>(
            "function pick(a, b, c) { // +1 (entry)
                 return a ?? b ?? c; // +2 (two `??`)
             }",
            "foo.js",
            |metric| {
                // unit(1) + fn(entry 1 + 2*?? = 3) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_nullish_coalescing_with_if_226() {
        // TypeScript must count `??` as a
        // decision.  This mirrors the example in the issue body.
        check_metrics::<TypescriptParser>(
            "function classify(x: string | null, fallback: string | null): string { // +1 (entry)
                 if (x === \"y\") return \"yes\"; // +1 (if)
                 return x ?? fallback ?? \"unknown\"; // +2 (two `??`)
             }",
            "foo.ts",
            |metric| {
                // unit(1) + fn(entry 1 + if 1 + 2*?? = 4) = sum 5, max 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 2.5,
                        "min": 1.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn tsx_nullish_coalescing_chain_226() {
        // TSX must count `??` the same as JS/TS.
        check_metrics::<TsxParser>(
            "function pick(a: number | null, b: number | null, c: number): number { // +1 (entry)
                 return a ?? b ?? c; // +2 (two `??`)
             }",
            "foo.tsx",
            |metric| {
                // unit(1) + fn(entry 1 + 2*?? = 3) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_nullish_coalescing_chain_226() {
        // Mozjs must count `??` the same as JS.
        check_metrics::<MozjsParser>(
            "function pick(a, b, c) { // +1 (entry)
                 return a ?? b ?? c; // +2 (two `??`)
             }",
            "foo.js",
            |metric| {
                // unit(1) + fn(entry 1 + 2*?? = 3) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_nullish_coalescing_assignment_231() {
        // `x ??= y` is `x = x ?? y` — one short-circuit decision edge,
        // same as `??`. Two `??=` assignments add +2 on top of the entry.
        check_metrics::<JavascriptParser>(
            "function pick(o) { // +1 (entry)
                 o.x ??= 1; // +1 (??=)
                 o.y ??= 2; // +1 (??=)
                 return o;
             }",
            "foo.js",
            |metric| {
                // unit(1) + fn(entry 1 + 2*??= = 3) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_nullish_coalescing_assignment_231() {
        // TypeScript must count `??=` the same as JS.
        check_metrics::<TypescriptParser>(
            "function pick(o: { x?: number; y?: number }) { // +1 (entry)
                 o.x ??= 1; // +1 (??=)
                 o.y ??= 2; // +1 (??=)
                 return o;
             }",
            "foo.ts",
            |metric| {
                // unit(1) + fn(entry 1 + 2*??= = 3) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn tsx_nullish_coalescing_assignment_231() {
        // TSX must count `??=` the same as JS/TS.
        check_metrics::<TsxParser>(
            "function pick(o: { x?: number; y?: number }) { // +1 (entry)
                 o.x ??= 1; // +1 (??=)
                 o.y ??= 2; // +1 (??=)
                 return o;
             }",
            "foo.tsx",
            |metric| {
                // unit(1) + fn(entry 1 + 2*??= = 3) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_nullish_coalescing_assignment_231() {
        // Mozjs must count `??=` the same as JS.
        check_metrics::<MozjsParser>(
            "function pick(o) { // +1 (entry)
                 o.x ??= 1; // +1 (??=)
                 o.y ??= 2; // +1 (??=)
                 return o;
             }",
            "foo.js",
            |metric| {
                // unit(1) + fn(entry 1 + 2*??= = 3) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn javascript_short_circuit_assignments_248() {
        // `&&=`, `||=`, `??=` are each one short-circuit decision edge —
        // semantically `x = x op y`. #231 added only `??=`; #248 adds the
        // sibling `&&=` and `||=`.
        check_metrics::<JavascriptParser>(
            "function f(x, y, z) { // +1 (entry)
                 x ??= 1; // +1 (??=)
                 y &&= 2; // +1 (&&=)
                 z ||= 3; // +1 (||=)
                 return x;
             }",
            "foo.js",
            |metric| {
                // unit(1) + fn(entry 1 + 3 assignments = 4) = sum 5, max 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 2.5,
                        "min": 1.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn typescript_short_circuit_assignments_248() {
        // TypeScript parallel of #248: `&&=` / `||=` / `??=` each +1.
        check_metrics::<TypescriptParser>(
            "function f(x: number | null, y: number | null, z: number | null): number { // +1 (entry)
                 x ??= 1; // +1 (??=)
                 y &&= 2; // +1 (&&=)
                 z ||= 3; // +1 (||=)
                 return x ?? 0; // +1 (??)
             }",
            "foo.ts",
            |metric| {
                // unit(1) + fn(entry 1 + 3 op= + 1 `??` = 5) = sum 6, max 5.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 6.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 5.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0,
                      "modified": {
                        "sum": 6.0,
                        "average": 3.0,
                        "min": 1.0,
                        "max": 5.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn tsx_short_circuit_assignments_248() {
        // TSX parallel of #248: `&&=` / `||=` / `??=` each +1.
        check_metrics::<TsxParser>(
            "function f(x: number | null, y: number | null, z: number | null): number { // +1 (entry)
                 x ??= 1; // +1 (??=)
                 y &&= 2; // +1 (&&=)
                 z ||= 3; // +1 (||=)
                 return x ?? 0; // +1 (??)
             }",
            "foo.tsx",
            |metric| {
                // unit(1) + fn(entry 1 + 3 op= + 1 `??` = 5) = sum 6, max 5.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 6.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 5.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0,
                      "modified": {
                        "sum": 6.0,
                        "average": 3.0,
                        "min": 1.0,
                        "max": 5.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_short_circuit_assignments_248() {
        // Mozjs parallel of #248: `&&=` / `||=` / `??=` each +1.
        check_metrics::<MozjsParser>(
            "function f(x, y, z) { // +1 (entry)
                 x ??= 1; // +1 (??=)
                 y &&= 2; // +1 (&&=)
                 z ||= 3; // +1 (||=)
                 return x;
             }",
            "foo.js",
            |metric| {
                // unit(1) + fn(entry 1 + 3 assignments = 4) = sum 5, max 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 2.5,
                        "min": 1.0,
                        "max": 4.0
                      }
                    }"###
                );
            },
        );
    }

    // Issue #281: optional chaining (`?.`) is short-circuit (it skips
    // the rest of the chain when the LHS is nullish), so each `?.`
    // adds one cyclomatic decision point. Before the fix, JS-family
    // cyclomatic ignored `?.` entirely. The four tests below mirror
    // the existing `nullish_coalescing_chain_226` pattern but for
    // `?.`: two `?.` in a chain add +2 on top of the function entry.
    #[test]
    fn javascript_optional_chain_counted_in_cyclomatic_281() {
        check_metrics::<JavascriptParser>(
            "function pick(a) { // +1 (entry)
                 return a?.b?.c; // +2 (two `?.`)
             }",
            "foo.js",
            |metric| {
                // unit(1) + fn(entry 1 + 2*?. = 3) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    #[test]
    fn mozjs_optional_chain_counted_in_cyclomatic_281() {
        check_metrics::<MozjsParser>(
            "function pick(a) { // +1 (entry)
                 return a?.b?.c; // +2 (two `?.`)
             }",
            "foo.js",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    #[test]
    fn typescript_optional_chain_counted_in_cyclomatic_281() {
        // TS exposes `?.` as both an `optional_chain` wrapper (over
        // member expressions) and a bare token (over call
        // expressions). We dispatch on `QMARKDOT` so every textual
        // `?.` adds exactly one decision point regardless of context.
        check_metrics::<TypescriptParser>(
            "function pick(a: any) { // +1 (entry)
                 return a?.b?.c; // +2 (two `?.`)
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    #[test]
    fn tsx_optional_chain_counted_in_cyclomatic_281() {
        check_metrics::<TsxParser>(
            "function pick(a: any) { // +1 (entry)
                 return a?.b?.c; // +2 (two `?.`)
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    // Mix of member-expression `?.` and call-expression `?.()`:
    // ensures the TS/TSX dispatch on `QMARKDOT` (not the wrapper)
    // counts both forms exactly once. Both forms emit the bare `?.`
    // token; the wrapper only appears around member expressions.
    #[test]
    fn typescript_optional_chain_call_form_counted_281() {
        check_metrics::<TypescriptParser>(
            "function pick(a: any) { // +1 (entry)
                 return a?.b?.(); // +2 (member `?.` + call `?.`)
             }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    #[test]
    fn tsx_optional_chain_call_form_counted_281() {
        check_metrics::<TsxParser>(
            "function pick(a: any) { // +1 (entry)
                 return a?.b?.(); // +2 (member `?.` + call `?.`)
             }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
            },
        );
    }

    #[test]
    fn csharp_nullish_coalescing_assignment_231() {
        // C#'s `??=` is short-circuit (RHS evaluates only when LHS is null)
        // and must add +1 cyclomatic per occurrence (#231).
        check_metrics::<CsharpParser>(
            "public class A {
                public int? x;
                public int? y;
                public void Pick() { // +1 (entry)
                    x ??= 1; // +1 (??=)
                    y ??= 2; // +1 (??=)
                }
            }",
            "foo.cs",
            |metric| {
                // unit(1) + class(1) + Pick(entry 1 + 2*??= = 3) = sum 5,
                // max 3 (Pick).
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 1.6666666666666667,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 5.0,
                        "average": 1.6666666666666667,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn mozjs_while_loop() {
        check_metrics::<MozjsParser>(
            "function f(n) { // +2 (+1 unit)
             var i = 0;
             while (i < n) { // +1
                 i++;
             }
             return i;
         }",
            "foo.js",
            |metric| {
                // unit(1) + fn(base 1 + while 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn bash_while_loop() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    local n=$1
    while [ $n -gt 0 ]; do
        echo $n
        n=$((n - 1))
    done
}",
            "foo.sh",
            |metric| {
                // unit(1) + fn(base 1 + while 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn bash_case_statement() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    case $1 in
        start) echo starting ;;
        stop)  echo stopping ;;
        *)     echo unknown  ;;
    esac
}",
            "foo.sh",
            |metric| {
                // standard: unit(1) + fn(base 1 + 2 explicit case_items;
                //          `*)` skipped per #211) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    /// Regression #211: a bare `*)` arm is Bash's analogue of the
    /// C-family `default:` and must NOT contribute to standard CCN.
    /// Without the fix, this 2-arm case reports `cyclomatic_max == 3`
    /// (1 base + 2 arms); with the fix it reports `2` (1 base + 1
    /// explicit arm), matching every other switch-bearing language
    /// in `tests/cyclomatic_cross_language_parity.rs`.
    #[test]
    fn bash_case_bare_wildcard_excluded() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    case \"$1\" in
        one) echo 1 ;;
        *)   echo 0 ;;
    esac
}",
            "foo.sh",
            |metric| {
                // standard: unit(1) + fn(base 1 + 1 explicit; `*)` skipped) = 3, max 2.
                // modified: unit(1) + fn(base 1 + case_stmt 1) = 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    /// A multi-value pattern containing `*` (`a|*)`) is NOT a bare
    /// wildcard — both alternations make it a non-default case. The
    /// arm still contributes one standard decision.
    #[test]
    fn bash_case_multi_value_with_star_counts() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    case \"$1\" in
        a|*) echo any ;;
    esac
}",
            "foo.sh",
            |metric| {
                // standard: unit(1) + fn(base 1 + 1 arm) = 3, max 2.
                // The `a|*` pattern has TWO `value` fields, so the
                // bare-wildcard filter (`value_count == 1`) skips it.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
            },
        );
    }

    #[test]
    fn bash_simple_function() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    echo hello
}",
            "foo.sh",
            |metric| {
                // unit(1) + fn(base 1) = sum 2, max 1.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 1.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn kotlin_for_loop() {
        check_metrics::<KotlinParser>(
            "fun sum(n: Int): Int {  // +2 (+1 unit)
             var s = 0
             for (i in 1..n) {  // +1
                 s += i
             }
             return s
         }",
            "foo.kt",
            |metric| {
                // unit(1) + fn(base 1 + for 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn kotlin_while_loop() {
        check_metrics::<KotlinParser>(
            "fun countdown(n: Int): Int { // +2 (+1 unit)
             var i = n
             while (i > 0) { // +1
                 i--
             }
             return i
         }",
            "foo.kt",
            |metric| {
                // unit(1) + fn(base 1 + while 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn kotlin_logical_operators() {
        check_metrics::<KotlinParser>(
            "fun check(a: Boolean, b: Boolean, c: Boolean): Boolean { // +2 (+1 unit)
             return a && b || c  // +1 &&, +1 ||
         }",
            "foo.kt",
            |metric| {
                // unit(1) + fn(base 1 + && 1 + || 1) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn kotlin_elvis_operator_239() {
        // Regression for issue #239: Kotlin's Elvis operator `?:` is a
        // short-circuit nullish operator analogous to JS `??` and each
        // occurrence is a distinct decision point, mirroring `&&` /
        // `||`. `a ?: b ?: c` contributes +2 to the function's
        // cyclomatic complexity (base 1 + two `?:` = 3).
        check_metrics::<KotlinParser>(
            "fun pick(a: String?, b: String?, c: String): String { // +2 (+1 unit)
             return a ?: b ?: c  // +2 (two ?: short-circuits)
         }",
            "foo.kt",
            |metric| {
                // unit(1) + fn(base 1 + ?: 1 + ?: 1) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn kotlin_safe_navigation_436() {
        // Issue #436: Kotlin's safe-navigation `?.` is a short-circuit
        // decision point, mirroring the JS/TS/C# treatment of `?.`
        // (#281). Each `?.` adds +1; the chain `a?.b?.c` adds +2.
        check_metrics::<KotlinParser>(
            "fun read(a: A?): String? { // +2 (+1 unit)
             return a?.b?.c  // +2 (two ?. short-circuits)
         }",
            "foo.kt",
            |metric| {
                // unit(1) + fn(base 1 + ?. 1 + ?. 1) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                // modified mirrors standard: each `?.` is both-metric.
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_max(), 3.0);
            },
        );
    }

    #[test]
    fn typescript_for_loop() {
        check_metrics::<TypescriptParser>(
            "function sum(n: number): number { // +2 (+1 unit)
             let s = 0;
             for (let i = 0; i < n; i++) { // +1
                 s += i;
             }
             return s;
         }",
            "foo.ts",
            |metric| {
                // unit(1) + fn(base 1 + for 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn typescript_while_loop() {
        check_metrics::<TypescriptParser>(
            "function countdown(n: number): number { // +2 (+1 unit)
             let i = n;
             while (i > 0) { // +1
                 i--;
             }
             return i;
         }",
            "foo.ts",
            |metric| {
                // unit(1) + fn(base 1 + while 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn typescript_logical_operators() {
        check_metrics::<TypescriptParser>(
            "function check(a: boolean, b: boolean, c: boolean): boolean { // +2 (+1 unit)
             return a && b || c;  // +1 &&, +1 ||
         }",
            "foo.ts",
            |metric| {
                // unit(1) + fn(base 1 + && 1 + || 1) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn typescript_try_catch() {
        check_metrics::<TypescriptParser>(
            "function safe(x: number): number { // +2 (+1 unit)
             try {
                 return 1 / x;
             } catch (e) { // +1
                 return 0;
             }
         }",
            "foo.ts",
            |metric| {
                // unit(1) + fn(base 1 + catch 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tsx_for_loop() {
        check_metrics::<TsxParser>(
            "function sum(n: number): number { // +2 (+1 unit)
             let s = 0;
             for (let i = 0; i < n; i++) { // +1
                 s += i;
             }
             return s;
         }",
            "foo.tsx",
            |metric| {
                // unit(1) + fn(base 1 + for 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tsx_while_loop() {
        check_metrics::<TsxParser>(
            "function countdown(n: number): number { // +2 (+1 unit)
             let i = n;
             while (i > 0) { // +1
                 i--;
             }
             return i;
         }",
            "foo.tsx",
            |metric| {
                // unit(1) + fn(base 1 + while 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tsx_logical_operators() {
        check_metrics::<TsxParser>(
            "function check(a: boolean, b: boolean, c: boolean): boolean { // +2 (+1 unit)
             return a && b || c;  // +1 &&, +1 ||
         }",
            "foo.tsx",
            |metric| {
                // unit(1) + fn(base 1 + && 1 + || 1) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tsx_try_catch() {
        check_metrics::<TsxParser>(
            "function safe(x: number): number { // +2 (+1 unit)
             try {
                 return 1 / x;
             } catch (e) { // +1
                 return 0;
             }
         }",
            "foo.tsx",
            |metric| {
                // unit(1) + fn(base 1 + catch 1) = sum 3, max 2.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 2.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn tsx_switch() {
        check_metrics::<TsxParser>(
            "function describe(x: number): string { // +2 (+1 unit)
             switch (x) {
                 case 1: // +1
                     return 'one';
                 case 2: // +1
                     return 'two';
                 default:
                     return 'other';
             }
         }",
            "foo.tsx",
            |metric| {
                // unit(1) + fn(base 1 + 2 cases) = sum 4, max 3.
                // default does NOT add a branch.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    /// Modified CCN: TSX switch with 2 cases collapses to 1.
    #[test]
    fn tsx_switch_modified() {
        check_metrics::<TsxParser>(
            "function f(x: number): string {
                 switch (x) {
                     case 1: return 'one';
                     case 2: return 'two';
                     default: return 'other';
                 }
             }",
            "foo.tsx",
            |metric| {
                // standard: unit(1) + fn(1) + 2 cases = sum 4, max 3.
                // modified: unit(1) + fn(1) + switch(1) = sum 3, max 2.
                // default does NOT add a branch.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn php_1_level_nesting() {
        // Mirrors java_simple_class' if-inside-method shape:
        // unit (+1) + function (+1) + if (+1) + && (+1) = sum 4.
        check_metrics::<PhpParser>(
            "<?php
            function f(int $a, int $b): bool {
                if ($a > 0 && $b > 0) {
                    return true;
                }
                return false;
            }",
            "foo.php",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    // `case`/`cond`/`with` arms surface as `stab_clause` nodes and
    // contribute to standard CCN, mirroring the C-family `case:` arm
    // treatment. The container Call (`case`) contributes once to
    // modified CCN, collapsing arms back to a single decision point.
    // Three func spaces (Unit + defmodule Class + def Function) each
    // seed one entry: standard = 3 entries + 3 stabs = 6; modified =
    // 3 entries + 1 case Call = 4.
    #[test]
    fn elixir_case_arms() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def classify(x) do\n    case x do\n      1 -> :one\n      2 -> :two\n      _ -> :other\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 6.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
            },
        );
    }

    // Each short-circuit boolean (`&&`, `||`, `and`, `or`) is one
    // decision point — Elixir does not expose `if`/`unless` as a
    // distinct kind_id, so this is the only operator-driven path the
    // metric can see.
    #[test]
    fn elixir_logical_operators() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x, y) do\n    x and y or (x && y) || x\n  end\nend\n",
            "foo.ex",
            |metric| {
                // 4 short-circuit ops + 3 entries (Unit, defmodule, def) = 7.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 7.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 7.0);
            },
        );
    }

    // `try`/`rescue`/`catch` is a multi-arm container Call: the `try`
    // Call contributes once to modified CCN, while each rescue/catch
    // arm's matched pattern (a `stab_clause`) contributes once to
    // standard CCN. This mirrors C-family `try`/`catch` semantics.
    #[test]
    fn elixir_try_rescue() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def safe do\n    try do\n      do_it()\n    rescue\n      ArgumentError -> :bad\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // standard: 3 entries + 1 rescue stab = 4
                // modified: 3 entries + 1 try Call = 4
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
            },
        );
    }

    // `if x do ... else ... end` surfaces as a `Call(target=if)`; the
    // metric inspects the source text of the call's target field to
    // identify it. Single-branch keyword Calls (`if`/`unless`/`for`/
    // `while`) contribute to both standard and modified CCN.
    #[test]
    fn elixir_if_else_counts() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    if x > 0 do\n      :pos\n    else\n      :neg\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // 1 if Call + 3 entries (Unit, defmodule Class, def Function) = 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
            },
        );
    }

    // `if x do ... end` without an `else` clause still surfaces as
    // `Call(target=if)` and is counted identically to the if/else
    // form — the `else` keyword is a do-block keyword argument, not
    // an extra `stab_clause`, so its presence does not change the
    // cyclomatic count.
    #[test]
    fn elixir_if_without_else_counts() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    if x > 0 do\n      :pos\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // 1 if Call + 3 entries = 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
            },
        );
    }

    // `unless x do ... end` is the negated `if`; it surfaces as
    // `Call(target=unless)` and is treated identically to `if`.
    #[test]
    fn elixir_unless_counts() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    unless x > 0 do\n      :nonpos\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
            },
        );
    }

    // `for x <- list, do: ...` is Elixir's comprehension generator —
    // a `Call(target=for)`. Counts once for both standard and
    // modified, mirroring `if`/`unless`.
    #[test]
    fn elixir_for_comprehension_counts() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(xs) do\n    for x <- xs do\n      x * 2\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
            },
        );
    }

    // `fn ... end` is its own function space (`get_space_kind` →
    // `Function`), so its cyclomatic gets its own `+1` entry path
    // alongside the Unit / defmodule Class / def Function entries.
    // Each `stab_clause` arm contributes to standard CCN; the anon-fn
    // itself is not a `Call`, so it does not add a modified-CCN
    // container decision. Standard = 4 entries (Unit, defmodule, def,
    // anon-fn) + 2 stab clauses = 6; modified = 4 entries = 4.
    #[test]
    fn elixir_anonymous_fn_arms_count() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    multi = fn 0 -> :zero; _ -> :other end\n    multi.(0)\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 6.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
            },
        );
    }

    // `cond do ... end` is the standard Elixir multi-way conditional.
    // Each clause is a `stab_clause` (standard CCN), and the `cond`
    // Call is a multi-arm container (modified CCN, once).
    #[test]
    fn elixir_cond_arms() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    cond do\n      x < 0 -> :neg\n      x == 0 -> :zero\n      true -> :pos\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // standard: 3 entries + 3 stabs = 6
                // modified: 3 entries + 1 cond Call = 4
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 6.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
            },
        );
    }

    // `with` chains use `<-` arrows, which parse as `binary_operator`
    // nodes — NOT `stab_clause`s — so the `with`-head clauses do not
    // contribute to standard CCN per-arm. The fallthrough `else`
    // branch, when present, contains `stab_clause`s that count for
    // standard. The `with` Call itself is a multi-arm container Call
    // that contributes once to modified CCN.
    #[test]
    fn elixir_with_else_only_counts_else_arms() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x) do\n    with {:ok, v} <- fetch(x),\n         {:ok, w} <- fetch(v) do\n      {:ok, w}\n    else\n      :error -> :nope\n      other -> {:bad, other}\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // standard: 3 entries + 2 else-block stabs = 5
                // modified: 3 entries + 1 with Call = 4
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
            },
        );
    }

    #[test]
    fn php_match_expression() {
        // Each `match_conditional_expression` arm (+1) but the default arm
        // does NOT add a branch (mirrors switch/case Java semantics).
        check_metrics::<PhpParser>(
            "<?php
            function color(string $c): int {
                return match ($c) {
                    'red' => 1,
                    'green' => 2,
                    'blue' => 3,
                    default => 0,
                };
            }",
            "foo.php",
            |metric| {
                // unit (+1) + function (+1) + 3 match arms (+3) = sum 5.
                // Default arm contributes 0.
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Modified CCN: PHP switch with 3 cases collapses to 1.
    #[test]
    fn php_switch_modified() {
        check_metrics::<PhpParser>(
            "<?php
            function describe(int $n): string {
                switch ($n) {
                    case 1:
                        return 'one';
                    case 2:
                        return 'two';
                    case 3:
                        return 'three';
                    default:
                        return 'other';
                }
            }",
            "foo.php",
            |metric| {
                // standard: unit(1) + fn(1) + 3 cases = sum 5, max 4.
                // modified: unit(1) + fn(1) + switch(1) = sum 3, max 2.
                // default does NOT add a branch.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 4.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn php_null_coalescing() {
        // `??` and `??=` are each one short-circuit decision (#231).
        // Tree-sitter emits `??=` as the single token `QMARKQMARKEQ`, so it
        // is matched independently from the binary `??`.
        check_metrics::<PhpParser>(
            "<?php
            function pick($x, $y) {
                $a = $x ?? $y;
                $a ??= 0;
                return $a;
            }",
            "foo.php",
            |metric| {
                // unit (+1) + function (+1) + ?? (+1) + ??= (+1) = sum 4.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn php_nullsafe_operator_436() {
        // Issue #436: PHP's nullsafe operator `?->` is a short-circuit
        // decision point, mirroring the JS/TS/C# treatment of `?.`
        // (#281). The `QMARKDASHGT` token fires once per operator across
        // both property access (`$a?->b`) and method call (`$a?->c()`),
        // and once per link in a chain. Here: one access + one chained
        // call (`$a?->b?->c()`) = +2 for that statement.
        check_metrics::<PhpParser>(
            "<?php
            function read($a) {
                return $a?->b?->c();
            }",
            "foo.php",
            |metric| {
                // unit(1) + fn(base 1 + ?-> 1 + ?-> 1) = sum 4, max 3.
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_max(), 3.0);
                // modified mirrors standard: each `?->` is both-metric.
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_max(), 3.0);
            },
        );
    }

    /// Modified CCN: nested switches contribute one decision each, not one
    /// total — the outer container does not absorb the inner one.
    #[test]
    fn cpp_nested_switch_modified() {
        check_metrics::<CppParser>(
            "void f() {
                 switch (x) {
                     case 1:
                         switch (y) {
                             case 10: break;
                             case 20: break;
                         }
                         break;
                     case 2: break;
                 }
             }",
            "foo.c",
            |metric| {
                // standard: unit(1) + fn(1) + 4 cases  = 6
                // modified: unit(1) + fn(1) + 2 switches = 4
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    /// Modified CCN: nested Rust matches each contribute one container.
    /// Bare `_ =>` arms are skipped.
    #[test]
    fn rust_nested_match_modified() {
        check_metrics::<RustParser>(
            "fn f(x: u8) -> u8 {
                 match x {
                     1 => match x {
                         10 => 1,
                         20 => 2,
                         _ => 0,
                     },
                     _ => 0,
                 }
             }",
            "foo.rs",
            |metric| {
                // standard: unit(1) + fn(1) + 3 arms (1,10,20; both _ skipped) = 5
                // modified: unit(1) + fn(1) + 2 matches  = 4
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    /// Pin the empty-switch edge case: standard counts no arms (0) while
    /// modified still counts the container (+1) per Lizard's `-m`.
    #[test]
    fn cpp_empty_switch_modified() {
        check_metrics::<CppParser>("void f() { switch (x) {} }", "foo.c", |metric| {
            // standard: unit(1) + fn(1) + 0 cases    = 2
            // modified: unit(1) + fn(1) + 1 switch   = 3
            insta::assert_json_snapshot!(
                metric.cyclomatic,
                @r###"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 1.0,
                      "max": 1.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
            );
        });
    }

    /// Two nested `for` loops contribute +1 each on top of the function and
    /// unit decisions.  No condition expressions, so `&&` / `||` do not fire.
    #[test]
    fn c_nested_loops() {
        check_metrics::<CppParser>(
            "void f() {
                 for (int i = 0; i < 10; ++i) {     // +1
                     for (int j = 0; j < 10; ++j) { // +1
                         g(i, j);
                     }
                 }
             }",
            "foo.c",
            |metric| {
                // standard: unit(1) + fn(1) + 2 for = 4
                // modified: identical (no switch container, no extra arms)
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 4.0);
                assert_eq!(s.cyclomatic_max(), 3.0);
                assert_eq!(s.cyclomatic_modified_sum(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    /// C++ `do { … } while (…)` contributes exactly +1 to both
    /// standard and modified CCN. The +1 comes from the `while`
    /// keyword token inside the do-statement (`Cpp::While`), which the
    /// C-family macro already counts. Adding the `DoStatement`
    /// statement node would double-count — see the macro doc comment
    /// and issue #284. This test pins the correct keyword-driven
    /// count.
    #[test]
    fn cpp_do_statement_counts_in_cyclomatic() {
        check_metrics::<CppParser>(
            "void f() {
                 int i = 0;
                 do {           // +1 (via inner `while` keyword)
                     ++i;
                 } while (i < 10);
             }",
            "foo.cpp",
            |metric| {
                // standard: unit(1) + fn(1) + do(1) = 3
                // modified: identical (no switch, no extra arms)
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 3.0);
                assert_eq!(s.cyclomatic_max(), 2.0);
                assert_eq!(s.cyclomatic_modified_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// C++ range-based `for (auto x : xs)` contributes exactly +1 to
    /// both standard and modified CCN — the `for` keyword token
    /// (`Cpp::For`) fires inside the `ForRangeLoop` node just like
    /// inside a classic `ForStatement`. Pinning this prevents
    /// reintroducing the double-count from issue #284's incorrect fix
    /// proposal.
    #[test]
    fn cpp_for_range_loop_counts_in_cyclomatic() {
        check_metrics::<CppParser>(
            "void f(std::vector<int> xs) {
                 for (auto x : xs) {   // +1 (via `for` keyword)
                     g(x);
                 }
             }",
            "foo.cpp",
            |metric| {
                // standard: unit(1) + fn(1) + for-range(1) = 3
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 3.0);
                assert_eq!(s.cyclomatic_max(), 2.0);
                assert_eq!(s.cyclomatic_modified_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// `?:` ternary is matched by `Cpp::ConditionalExpression` in the
    /// C-family macro and contributes +1 standard *and* +1 modified.
    /// Two nested ternaries in one expression therefore add 2 to each.
    #[test]
    fn c_ternary_chain() {
        check_metrics::<CppParser>(
            "int f(int a, int b, int c) {
                 return a > 0 ? a : (b > 0 ? b : c); // +2 ternaries (?: each)
             }",
            "foo.c",
            |metric| {
                // standard: unit(1) + fn(1) + 2 ?: = 4
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 4.0);
                assert_eq!(s.cyclomatic_max(), 3.0);
                assert_eq!(s.cyclomatic_modified_sum(), 4.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    /// Short-circuit `&&` / `||` chains each contribute +1 — every binary
    /// operator token in the chain is a separate decision (Lizard parity).
    #[test]
    fn c_short_circuit_chain() {
        check_metrics::<CppParser>(
            "int f(int a, int b, int c, int d) {
                 if (a && b || c && d) {            // 3 logical ops + 1 if = 4
                     return 1;
                 }
                 return 0;
             }",
            "foo.c",
            |metric| {
                // standard: unit(1) + fn(1) + if(1) + && (2) + || (1) = 6
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 6.0);
                assert_eq!(s.cyclomatic_max(), 5.0);
                assert_eq!(s.cyclomatic_modified_sum(), 6.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0,
                      "modified": {
                        "sum": 6.0,
                        "average": 3.0,
                        "min": 1.0,
                        "max": 5.0
                      }
                    }"###
                );
            },
        );
    }

    /// Switch with intentional fall-through: every `case` adds +1 standard
    /// regardless of whether the arm `break`s.  Modified collapses all three
    /// arms into one switch container.
    #[test]
    fn c_switch_fallthrough() {
        check_metrics::<CppParser>(
            "int f(int x) {
                 int r = 0;
                 switch (x) {
                     case 1:                // +1
                     case 2:                // +1
                         r = 10;
                         break;
                     case 3:                // +1
                         r = 20;
                         break;
                 }
                 return r;
             }",
            "foo.c",
            |metric| {
                // standard: unit(1) + fn(1) + 3 cases = 5
                // modified: unit(1) + fn(1) + 1 switch container = 3
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 5.0);
                assert_eq!(s.cyclomatic_modified_sum(), 3.0);
                assert!(s.cyclomatic_modified_sum() < s.cyclomatic_sum());
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// `goto` is not a recognised decision keyword in the C-family macro
    /// (only `If | For | While | Catch | ConditionalExpression | && | ||`
    /// add complexity, plus `Case` / `SwitchStatement`).  The label and the
    /// `goto` jump are control-flow, but the metric deliberately mirrors
    /// Lizard, which also does not count `goto`.  This test pins that
    /// decision so a future change that adds `Cpp::GotoStatement` to the
    /// macro fires here first.
    #[test]
    fn c_goto_not_counted() {
        check_metrics::<CppParser>(
            "int f(int n) {
                 int i = 0;
             retry:
                 if (i < n) {     // +1
                     ++i;
                     goto retry;  // ignored
                 }
                 return i;
             }",
            "foo.c",
            |metric| {
                // standard: unit(1) + fn(1) + if(1) = 3
                // goto/label add nothing.
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_sum(), 3.0);
                assert_eq!(s.cyclomatic_modified_sum(), 3.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 3.0,
                      "average": 1.5,
                      "min": 1.0,
                      "max": 2.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Direct accessor coverage: assert the modified-CCN getters return
    /// the values we expect from a known fixture, bypassing the JSON
    /// serializer.  Modified must never exceed standard for non-degenerate
    /// inputs (a switch with at least one arm).
    #[test]
    fn cyclomatic_modified_accessors() {
        check_metrics::<RustParser>(
            "fn f(x: u8) -> u8 {
                 match x {
                     1 => 1,
                     2 => 2,
                     _ => 0,
                 }
             }",
            "foo.rs",
            |metric| {
                // standard sum: unit(1) + fn(1 + 2 arms, _ skipped) = 4
                // modified sum: unit(1) + fn(1 + 1 MatchExpr)       = 3
                let s = &metric.cyclomatic;
                assert_eq!(s.cyclomatic_modified_sum(), 3.0);
                assert_eq!(s.cyclomatic_modified_min(), 1.0);
                assert_eq!(s.cyclomatic_modified_max(), 2.0);
                assert_eq!(s.cyclomatic_modified_average(), 1.5);
                assert!(s.cyclomatic_modified_sum() <= s.cyclomatic_sum());
            },
        );
    }

    /// Bare `_ =>` wildcard is not counted (matches C-family `default:`).
    #[test]
    fn rust_wildcard_only_match() {
        check_metrics::<RustParser>(
            "fn f(x: u8) -> &'static str {
                 match x {
                     _ => \"fallback\",
                 }
             }",
            "foo.rs",
            |metric| {
                // standard: unit(1) + fn(1) + 0 arms (bare wildcard skipped) = 2
                // modified: unit(1) + fn(1) + MatchExpr(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 1.0,
                      "max": 1.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Wildcard arm plus explicit arms: only explicit arms count.
    #[test]
    fn rust_wildcard_plus_explicit_arms() {
        check_metrics::<RustParser>(
            "fn f(x: u8) -> &'static str {
                 match x {
                     1 => \"one\",
                     2 => \"two\",
                     3 => \"three\",
                     _ => \"other\",
                 }
             }",
            "foo.rs",
            |metric| {
                // standard: unit(1) + fn(1) + 3 arms (1,2,3) = 5
                // modified: unit(1) + fn(1) + MatchExpr(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// `Some(_)` is NOT a bare wildcard — still counts.
    #[test]
    fn rust_some_wildcard_still_counts() {
        check_metrics::<RustParser>(
            "fn f(x: Option<u8>) -> u8 {
                 match x {
                     Some(_) => 1,
                     None => 0,
                 }
             }",
            "foo.rs",
            |metric| {
                // standard: unit(1) + fn(1) + 2 arms (Some(_), None) = 4
                // modified: unit(1) + fn(1) + MatchExpr(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Tuple pattern `(_, x)` is NOT a bare wildcard — still counts.
    #[test]
    fn rust_tuple_wildcard_still_counts() {
        check_metrics::<RustParser>(
            "fn f(x: (u8, u8)) -> u8 {
                 match x {
                     (0, y) => y,
                     (_, y) => y + 1,
                 }
             }",
            "foo.rs",
            |metric| {
                // standard: unit(1) + fn(1) + 2 arms = 4
                // modified: unit(1) + fn(1) + MatchExpr(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// `_ if guard` is NOT a bare wildcard — still counts.
    /// The `if` keyword inside the guard also contributes +1 standard/modified.
    #[test]
    fn rust_guarded_wildcard_still_counts() {
        check_metrics::<RustParser>(
            "fn f(x: u8) -> &'static str {
                 match x {
                     1 => \"one\",
                     _ if x > 100 => \"big\",
                     _ => \"other\",
                 }
             }",
            "foo.rs",
            |metric| {
                // standard: unit(1) + fn(1 + arm(1) + guarded_arm(1) + if_kw(1)) = 5
                // modified: unit(1) + fn(1 + MatchExpr(1) + if_kw(1)) = 4
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 5.0,
                      "average": 2.5,
                      "min": 1.0,
                      "max": 4.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    /// Regression #107: empty case…esac has no arms, so standard adds 0 and
    /// modified adds 1 (the container).
    #[test]
    fn bash_case_empty() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    case $1 in
    esac
}",
            "foo.sh",
            |metric| {
                // standard: unit(1) + fn(1) + 0 arms = 2
                // modified: unit(1) + fn(1) + case_stmt(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 2.0,
                      "average": 1.0,
                      "min": 1.0,
                      "max": 1.0,
                      "modified": {
                        "sum": 3.0,
                        "average": 1.5,
                        "min": 1.0,
                        "max": 2.0
                      }
                    }"###
                );
            },
        );
    }

    /// Regression #107: nested case…esac — each container contributes to
    /// modified independently, and each arm contributes to standard.
    #[test]
    fn bash_nested_case() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    case $1 in
        a)
            case $2 in
                x) echo ax ;;
                y) echo ay ;;
            esac
            ;;
        b) echo b ;;
    esac
}",
            "foo.sh",
            |metric| {
                // standard: unit(1) + fn(1) + outer arms(a,b = 2) + inner arms(x,y = 2) = 6
                // modified: unit(1) + fn(1) + 2 case_stmts = 4
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    /// Nested matches with wildcards: only bare `_` skipped at each level.
    #[test]
    fn rust_nested_match_with_wildcards() {
        check_metrics::<RustParser>(
            "fn f(x: u8, y: u8) -> &'static str {
                 match x {
                     1 => match y {
                         1 => \"one-one\",
                         _ => \"one-other\",
                     },
                     _ => \"other\",
                 }
             }",
            "foo.rs",
            |metric| {
                // standard: unit(1) + fn(1) + outer arm 1(+1) + inner arm 1(+1)
                //           + outer bare _(0) + inner bare _(0) = 4
                // modified: unit(1) + fn(1) + 2 MatchExpr(+2) = 4
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 4.0,
                      "average": 2.0,
                      "min": 1.0,
                      "max": 3.0,
                      "modified": {
                        "sum": 4.0,
                        "average": 2.0,
                        "min": 1.0,
                        "max": 3.0
                      }
                    }"###
                );
            },
        );
    }

    #[test]
    fn ruby_nested_branches() {
        // expected: unit(1) + method(1 + `if` + `while`) = 1 + 3 = 4
        // standard CCN.
        check_metrics::<RubyParser>(
            "def foo(a)\n  if a > 0\n    while a > 0\n      a -= 1\n    end\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn ruby_case_when_arms() {
        // Each `when` arm adds standard CCN; the `case` container is
        // counted ONCE in modified CCN.
        // expected: standard = unit(1) + method(1 + 3 when) = 5;
        // modified = unit(1) + method(1 + 1 case) = 3.
        check_metrics::<RubyParser>(
            "def foo(x)\n  case x\n  when 1 then 'one'\n  when 2 then 'two'\n  when 3 then 'three'\n  end\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 3.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn ruby_ternary_conditional() {
        // Ruby's `cond ? a : b` parses as `Conditional` and counts as a
        // branch in both standard and modified CCN.
        // expected: standard = unit(1) + method(1 + 1) = 3.
        check_metrics::<RubyParser>(
            "def foo(x)\n  x.positive? ? :pos : :nonpos\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 3.0);
            },
        );
    }

    #[test]
    fn ruby_and_or_keywords() {
        // Word-form `and` / `or` are distinct grammar kinds from
        // `&&` / `||` and must each contribute one decision point.
        // expected: standard = unit(1) + method(1 + and + or) = 4.
        check_metrics::<RubyParser>(
            "def foo(a, b, c)\n  a and b or c\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
            },
        );
    }

    /// Cross-language parity for cyclomatic: an `if/else if/else` chain
    /// of three arms must produce the same per-function (max-space)
    /// cyclomatic score across Ruby, Rust, and Java. Per-language
    /// snapshot tests pin each language's history but cannot detect
    /// drift on the same logical construct — lesson 11
    /// (`docs/development/lessons_learned.md`) catalogues real
    /// incidents (#106 Rust-vs-C-family wildcard counting; #107 Bash
    /// double-counting case containers) that survived per-language
    /// suites for years. `cyclomatic_max()` is the function-level
    /// cyclomatic and is independent of unit/class space stacking, so
    /// the comparison is meaningful across languages with different
    /// space hierarchies.
    ///
    /// Expected per function: 1 (base) + 1 (`if`) + 1 (`else if`) = 3.
    /// The `else` arm is unconditional and does not contribute. Each
    /// language asserts the literal 3.0 in its own closure so a future
    /// drift in any single language fails THIS test (and only this
    /// test), making cross-language disagreement visible at a glance.
    #[test]
    fn cyclomatic_if_elseif_else_chain_cross_language() {
        check_metrics::<RubyParser>(
            "def classify(x)\n  if x > 0\n    :pos\n  elsif x < 0\n    :neg\n  else\n    :zero\n  end\nend\n",
            "foo.rb",
            |m| {
                assert_eq!(m.cyclomatic.cyclomatic_max(), 3.0, "ruby");
            },
        );
        check_metrics::<RustParser>(
            "fn classify(x: i32) -> &'static str {\n    if x > 0 { \"pos\" } else if x < 0 { \"neg\" } else { \"zero\" }\n}\n",
            "foo.rs",
            |m| {
                assert_eq!(m.cyclomatic.cyclomatic_max(), 3.0, "rust");
            },
        );
        check_metrics::<JavaParser>(
            "class C {\n    String classify(int x) {\n        if (x > 0) return \"pos\";\n        else if (x < 0) return \"neg\";\n        else return \"zero\";\n    }\n}\n",
            "Foo.java",
            |m| {
                assert_eq!(m.cyclomatic.cyclomatic_max(), 3.0, "java");
            },
        );
    }

    /// Parity gate for the `impl_cyclomatic_java_like!` macro (#300):
    /// every decision kind shared by Java and Groovy must produce the
    /// same per-function cyclomatic score for a common decision-rich
    /// method body. Dropping a kind from the macro body (e.g.,
    /// removing `For` or `TernaryExpression`) would fail BOTH language
    /// assertions; dropping a kind from only one invocation would fail
    /// only that language's assertion.
    ///
    /// The body intentionally exercises every shared kind:
    /// `If`, `For`, `While`, `Catch`, `TernaryExpression`, `AMPAMP`,
    /// `PIPEPIPE`, plus a `switch` with two `Case` arms (one is the
    /// default and contributes nothing under standard CCN). Expected
    /// per-function: 1 (base) + if + for + while + catch + ternary +
    /// && + || + 2 cases = 10 (standard).
    ///
    /// Modified CCN is asserted in parallel: the multi-kind arm
    /// bumps both counters, and `Switch` (one keyword token per
    /// switch construct) replaces the standard CCN's two `Case`
    /// arms. Expected modified per-function: 1 (base) + if + for +
    /// while + catch + ternary + && + || + switch = 9. Without the
    /// modified assertion a mutation that drops
    /// `stats.cyclomatic_modified += 1.` from any shared arm (or
    /// drops the `Switch` arm entirely) would pass.
    #[test]
    fn cyclomatic_java_groovy_parity_300() {
        const JAVA_SRC: &str = "class C {\n\
            int decide(int x, int y, int[] xs) {\n\
                int r = 0;\n\
                if (x > 0 && y > 0) r = 1;\n\
                for (int i = 0; i < 3; i++) r++;\n\
                while (x > 0) { x--; r++; }\n\
                try { r += xs[0]; } catch (Exception e) { r = -1; }\n\
                r = (x > 0 || y < 0) ? r : -r;\n\
                switch (x) { case 1: r++; break; case 2: r--; break; default: break; }\n\
                return r;\n\
            }\n\
        }\n";
        const GROOVY_SRC: &str = "class C {\n\
            int decide(int x, int y, int[] xs) {\n\
                int r = 0\n\
                if (x > 0 && y > 0) r = 1\n\
                for (int i = 0; i < 3; i++) r++\n\
                while (x > 0) { x--; r++ }\n\
                try { r += xs[0] } catch (Exception e) { r = -1 }\n\
                r = (x > 0 || y < 0) ? r : -r\n\
                switch (x) { case 1: r++; break; case 2: r--; break; default: break }\n\
                return r\n\
            }\n\
        }\n";
        check_metrics::<JavaParser>(JAVA_SRC, "Foo.java", |m| {
            assert_eq!(m.cyclomatic.cyclomatic_max(), 10.0, "java parity");
            assert_eq!(
                m.cyclomatic.cyclomatic_modified_max(),
                9.0,
                "java modified parity"
            );
        });
        check_metrics::<GroovyParser>(GROOVY_SRC, "foo.groovy", |m| {
            assert_eq!(m.cyclomatic.cyclomatic_max(), 10.0, "groovy parity");
            assert_eq!(
                m.cyclomatic.cyclomatic_modified_max(),
                9.0,
                "groovy modified parity"
            );
        });
    }

    /// Groovy-only delta in `impl_cyclomatic_java_like!`: the `Assert`
    /// extra-kind invocation must keep Groovy's `assert` branching at
    /// +1 while Java does not count anything for an identical-looking
    /// construct (Java has no `assert`-as-branch token; its `assert`
    /// statement is grammar-distinct and not in this macro's arm).
    /// Dropping `[Assert]` from the Groovy invocation would fail this
    /// test.
    #[test]
    fn cyclomatic_groovy_assert_arm_300() {
        check_metrics::<GroovyParser>("void check(int x) { assert x > 0 }", "foo.groovy", |m| {
            // unit(1) + fn(1) + assert(1) = 3
            assert_eq!(m.cyclomatic.cyclomatic_sum(), 3.0, "groovy assert sum");
            assert_eq!(m.cyclomatic.cyclomatic_max(), 2.0, "groovy assert max");
            // Assert contributes to BOTH standard and modified CCN, so the
            // fn-level modified score is also base(1) + assert(1) = 2.
            // Without this assertion, a mutation that dropped
            // `stats.cyclomatic_modified += 1.` from the multi-kind arm
            // would pass.
            assert_eq!(
                m.cyclomatic.cyclomatic_modified_max(),
                2.0,
                "groovy assert modified max"
            );
        });
    }

    /// Regression for issue #246: Groovy's Elvis operator `?:` is a
    /// short-circuit nullish operator that introduces a branch — each
    /// occurrence in a chain adds +1 to cyclomatic complexity. The
    /// dekobon Groovy grammar models Elvis as a distinct
    /// `elvis_expression` node with a real `QMARKCOLON` token, so the
    /// `impl_cyclomatic_java_like!(GroovyCode, Groovy, [Assert,
    /// QMARKCOLON])` invocation picks it up directly.
    #[test]
    fn cyclomatic_groovy_elvis_chain_246() {
        check_metrics::<GroovyParser>(
            "def pick(a, b, c) { return a ?: b ?: c }",
            "foo.groovy",
            |m| {
                // unit(1) + fn(1) + two `?:` short-circuits(2) = 4
                assert_eq!(m.cyclomatic.cyclomatic_sum(), 4.0, "groovy elvis sum");
                assert_eq!(m.cyclomatic.cyclomatic_max(), 3.0, "groovy elvis max");
                assert_eq!(
                    m.cyclomatic.cyclomatic_modified_max(),
                    3.0,
                    "groovy elvis modified max"
                );
            },
        );
    }

    #[test]
    fn ruby_rescue_modifier() {
        // Postfix `x rescue y` parses as a `RescueModifier` node that
        // wraps the recovery clause. Both wrapper and clause fire the
        // cyclomatic branch arm; the method body therefore contributes
        // +2 to its space.
        // expected: standard = unit(1) + method(1 + 1) = 3.
        check_metrics::<RubyParser>(
            "def foo\n  parse(x) rescue nil\nend\n",
            "foo.rb",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                insta::assert_json_snapshot!(metric.cyclomatic);
            },
        );
    }

    #[test]
    fn ruby_safe_navigation_cyclomatic() {
        // Issue #452: Ruby's safe-navigation `&.` (AMPDOT) is a
        // short-circuit decision point per link, mirroring the
        // Kotlin/PHP/JS/C# treatment of `?.` (#281). The chain
        // `a&.b&.c` adds +2 to both standard and modified CCN.
        check_metrics::<RubyParser>("def read(a); a&.b&.c; end\n", "foo.rb", |metric| {
            // unit(1) + method(base 1 + &. 1 + &. 1) = sum 4, max 3.
            let s = &metric.cyclomatic;
            assert_eq!(s.cyclomatic_sum(), 4.0);
            assert_eq!(s.cyclomatic_max(), 3.0);
            assert_eq!(s.cyclomatic_modified_sum(), 4.0);
            assert_eq!(s.cyclomatic_modified_max(), 3.0);
        });
    }
}
