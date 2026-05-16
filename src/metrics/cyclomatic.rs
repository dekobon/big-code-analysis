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
    #[must_use]
    pub fn cyclomatic_min(&self) -> f64 {
        self.cyclomatic_min
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
    #[must_use]
    pub fn cyclomatic_modified_min(&self) -> f64 {
        self.cyclomatic_modified_min
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
        match node.kind_id().into() {
            If | Elif | For | While | Except | With | Assert | And | Or => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            CaseClause => {
                let mut pattern_is_bare_underscore = false;
                let mut has_guard = false;
                for child in node.children() {
                    match child.kind_id().into() {
                        CasePattern => {
                            pattern_is_bare_underscore =
                                crate::metrics::npa::pattern_is_bare_underscore(
                                    &child,
                                    UNDERSCORE as u16,
                                );
                        }
                        IfClause => has_guard = true,
                        _ => {}
                    }
                }
                if !pattern_is_bare_underscore || has_guard {
                    stats.cyclomatic += 1.;
                }
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
/// coalescing (`??`, token `QMARKQMARK`) on top of `&&` and `||`, while
/// C++ has only `&&` and `||` (issue #226).
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

// JS-family: include nullish coalescing (`??`) as a short-circuit
// decision in addition to `&&` and `||` (issue #226).
impl_cyclomatic_c_family!(
    MozjsCode,
    Mozjs,
    TernaryExpression,
    [AMPAMP, PIPEPIPE, QMARKQMARK]
);
impl_cyclomatic_c_family!(
    JavascriptCode,
    Javascript,
    TernaryExpression,
    [AMPAMP, PIPEPIPE, QMARKQMARK]
);
impl_cyclomatic_c_family!(
    TypescriptCode,
    Typescript,
    TernaryExpression,
    [AMPAMP, PIPEPIPE, QMARKQMARK]
);
impl_cyclomatic_c_family!(
    TsxCode,
    Tsx,
    TernaryExpression,
    [AMPAMP, PIPEPIPE, QMARKQMARK]
);

impl Cyclomatic for RustCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
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
            // Both standard and modified.
            If | For | While | Loop | TryExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

// C++ has only `&&` and `||` short-circuit operators.
impl_cyclomatic_c_family!(CppCode, Cpp, ConditionalExpression, [AMPAMP, PIPEPIPE]);

impl Cyclomatic for JavaCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Java::*;

        match node.kind_id().into() {
            Case => {
                stats.cyclomatic += 1.;
            }
            // The `switch` keyword token appears exactly once per switch
            // construct (both classic switch statements and Java 14+ switch
            // expressions), so it serves as the container marker.
            Switch => {
                stats.cyclomatic_modified += 1.;
            }
            If | For | While | Catch | TernaryExpression | AMPAMP | PIPEPIPE => {
                stats.cyclomatic += 1.;
                stats.cyclomatic_modified += 1.;
            }
            _ => {}
        }
    }
}

impl Cyclomatic for CsharpCode {
    fn compute<'a>(node: &Node<'a>, _code: &'a [u8], stats: &mut Stats) {
        use Csharp::*;

        match node.kind_id().into() {
            // Standard-only: individual switch statement arms and switch
            // expression arms.
            Case | SwitchExpressionArm => {
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
            | QMARKQMARK => {
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
            // Standard-only: individual when entries (arms).
            WhenEntry => {
                stats.cyclomatic += 1.;
            }
            // Modified-only: the when expression container.
            WhenExpression => {
                stats.cyclomatic_modified += 1.;
            }
            // Both standard and modified.
            IfExpression | ForStatement | WhileStatement | DoWhileStatement | CatchBlock
            | AMPAMP | PIPEPIPE => {
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
            | QMARKQMARK => {
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
            | R::Or => {
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

    /// Regression for #229: a plain `if/else` must not be credited
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
        // The discard arm `_ =>` is currently counted by SwitchExpressionArm
        // (the grammar does not separate discard arms into a distinct kind).
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
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 7.0,
                      "average": 2.3333333333333335,
                      "min": 1.0,
                      "max": 5.0,
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
                        1 -> println(\"one\")   // +1 (WhenEntry)
                        2 -> println(\"two\")   // +1
                        else -> println(\"?\") // +1
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
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 10.0,
                      "average": 3.3333333333333335,
                      "min": 1.0,
                      "max": 8.0,
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
                // standard: unit(1) + fn(1) + 4 WhenEntry = 6
                // modified: unit(1) + fn(1) + WhenExpression(1) = 3
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                    {
                      "sum": 6.0,
                      "average": 3.0,
                      "min": 1.0,
                      "max": 5.0,
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
        // Regression for issue #226: `??` is short-circuit and must count as
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
        // Regression for issue #226: TypeScript must count `??` as a
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
        // Regression for issue #226: TSX must count `??` the same as JS/TS.
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
        // Regression for issue #226: Mozjs must count `??` the same as JS.
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
    // A three-arm `case`: 3 stabs + 1 entry = standard 4, 1 case-Call
    // + 1 entry = modified 2.
    #[test]
    fn elixir_case_arms() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def classify(x) do\n    case x do\n      1 -> :one\n      2 -> :two\n      _ -> :other\n    end\n  end\nend\n",
            "foo.ex",
            |metric| {
                // standard: 3 stab clauses + 1 (entry path) = 4
                // modified: 1 case Call + 1 (entry path) = 2
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                {
                  "sum": 4.0,
                  "average": 4.0,
                  "min": 4.0,
                  "max": 4.0,
                  "modified": {
                    "sum": 2.0,
                    "average": 2.0,
                    "min": 2.0,
                    "max": 2.0
                  }
                }"###
                );
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
                // and + or + && + || = 4 decisions + 1 entry = cyc 5
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                {
                  "sum": 5.0,
                  "average": 5.0,
                  "min": 5.0,
                  "max": 5.0,
                  "modified": {
                    "sum": 5.0,
                    "average": 5.0,
                    "min": 5.0,
                    "max": 5.0
                  }
                }"###
                );
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
                // standard: 1 rescue stab clause + 1 entry = 2
                // modified: 1 try Call + 1 entry = 2
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
                insta::assert_json_snapshot!(
                    metric.cyclomatic,
                    @r###"
                {
                  "sum": 2.0,
                  "average": 2.0,
                  "min": 2.0,
                  "max": 2.0,
                  "modified": {
                    "sum": 2.0,
                    "average": 2.0,
                    "min": 2.0,
                    "max": 2.0
                  }
                }"###
                );
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
                // 1 if Call + 1 entry = both standard and modified 2
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
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
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
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
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
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
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 2.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
            },
        );
    }

    // `fn ... end` is its own function space (`get_space_kind` →
    // `Function`), so its cyclomatic gets its own `+1` entry path
    // alongside the Unit's `+1`. Each `stab_clause` arm contributes
    // to standard CCN; the anon-fn itself is not a `Call`, so it
    // does not add a modified-CCN container decision. A two-arm
    // `fn` therefore yields: standard = 1 (Unit) + 1 (anon-fn) + 2
    // stabs = 4; modified = 1 (Unit) + 1 (anon-fn) = 2.
    #[test]
    fn elixir_anonymous_fn_arms_count() {
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f do\n    multi = fn 0 -> :zero; _ -> :other end\n    multi.(0)\n  end\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
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
                // standard: 3 stabs + 1 entry = 4
                // modified: 1 cond Call + 1 entry = 2
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 4.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
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
                // standard: 2 else-block stab clauses + 1 entry = 3
                // modified: 1 with Call + 1 entry = 2
                assert_eq!(metric.cyclomatic.cyclomatic_sum(), 3.0);
                assert_eq!(metric.cyclomatic.cyclomatic_modified_sum(), 2.0);
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
        // `??` adds 1 (treated as a short-circuit branch). `??=` is an
        // augmented assignment, NOT a binary `??` — does not double-count.
        check_metrics::<PhpParser>(
            "<?php
            function pick($x, $y) {
                $a = $x ?? $y;
                $a ??= 0;
                return $a;
            }",
            "foo.php",
            |metric| {
                // unit (+1) + function (+1) + ?? (+1) = sum 3.
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
}
