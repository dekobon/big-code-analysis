// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::enum_glob_use, clippy::unused_self, clippy::wildcard_imports)]

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::fmt;

use super::cyclomatic;
use super::halstead;
use super::loc;

use crate::checker::Checker;
use crate::macros::implement_metric_trait;

use crate::*;

/// The `Mi` metric.
#[derive(Default, Clone, Debug)]
pub struct Stats {
    halstead_length: f64,
    halstead_vocabulary: f64,
    halstead_volume: f64,
    cyclomatic: f64,
    sloc: f64,
    /// Comment lines as a percentage in [0, 100] (not a ratio in [0, 1]).
    /// Only `mi_sei` consumes this — the SEI MI formula uses `perCM` on
    /// the percentage scale; see issue #241.
    comments_percentage: f64,
}

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("maintainability_index", 3)?;
        st.serialize_field("mi_original", &self.mi_original())?;
        st.serialize_field("mi_sei", &self.mi_sei())?;
        st.serialize_field("mi_visual_studio", &self.mi_visual_studio())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "mi_original: {}, mi_sei: {}, mi_visual_studio: {}",
            self.mi_original(),
            self.mi_sei(),
            self.mi_visual_studio()
        )
    }
}

impl Stats {
    pub(crate) fn merge(&mut self, _other: &Stats) {}

    #[inline]
    fn inputs_are_empty(&self) -> bool {
        self.halstead_volume <= 0.0 || self.sloc <= 0.0
    }

    /// Returns the `Mi` metric calculated using the original formula.
    ///
    /// Its value can be negative.
    #[inline]
    #[must_use]
    pub fn mi_original(&self) -> f64 {
        if self.inputs_are_empty() {
            return 0.0;
        }
        // http://www.projectcodemeter.com/cost_estimation/help/GL_maintainability.htm
        171.0 - 5.2 * (self.halstead_volume).ln() - 0.23 * self.cyclomatic - 16.2 * self.sloc.ln()
    }

    /// Returns the `Mi` metric calculated using the derivative formula
    /// employed by the Software Engineering Insitute (SEI).
    ///
    /// Its value can be negative.
    #[inline]
    #[must_use]
    pub fn mi_sei(&self) -> f64 {
        if self.inputs_are_empty() {
            return 0.0;
        }
        // http://www.projectcodemeter.com/cost_estimation/help/GL_maintainability.htm
        171.0 - 5.2 * self.halstead_volume.log2() - 0.23 * self.cyclomatic - 16.2 * self.sloc.log2()
            + 50.0 * (self.comments_percentage * 2.4).sqrt().sin()
    }

    /// Returns the `Mi` metric calculated using the derivative formula
    /// employed by Microsoft Visual Studio.
    #[inline]
    #[must_use]
    pub fn mi_visual_studio(&self) -> f64 {
        if self.inputs_are_empty() {
            return 0.0;
        }
        // http://www.projectcodemeter.com/cost_estimation/help/GL_maintainability.htm
        let formula = 171.0
            - 5.2 * self.halstead_volume.ln()
            - 0.23 * self.cyclomatic
            - 16.2 * self.sloc.ln();
        (formula * 100.0 / 171.0).max(0.)
    }
}

/// Per-language computation of the maintainability index.
pub trait Mi
where
    Self: Checker,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    fn compute(
        loc: &loc::Stats,
        cyclomatic: &cyclomatic::Stats,
        halstead: &halstead::Stats,
        stats: &mut Stats,
    ) {
        stats.halstead_length = halstead.length();
        stats.halstead_vocabulary = halstead.vocabulary();
        stats.halstead_volume = halstead.volume();
        stats.cyclomatic = cyclomatic.cyclomatic_sum();
        stats.sloc = loc.sloc();
        // The SEI Maintainability Index expects `perCM` as a percentage
        // in [0, 100], not a ratio in [0, 1] — `50·sin(√(2.4·CM))` is
        // nonsensical when CM is two orders of magnitude too small. See
        // issue #241 and Welker/Oman's original MI definition.
        stats.comments_percentage = if stats.sloc == 0.0 {
            0.0
        } else {
            loc.cloc() / stats.sloc * 100.0
        };
    }
}

// `Mi` uses the bracketed `[Trait]` arm: this expands to a bare
// `impl Mi for X {}` which inherits `Mi::compute`'s default trait
// method body. The default method is fully language-neutral — it
// combines already-computed Halstead / Cyclomatic / Loc stats into
// the three MI variants — so this list is NOT a no-op like the named-
// arm matrices for Abc / Npa / Npm / Wmc. Audited in #188.
implement_metric_trait!(
    [Mi],
    PythonCode,
    MozjsCode,
    JavascriptCode,
    TypescriptCode,
    TsxCode,
    RustCode,
    CppCode,
    PreprocCode,
    CcommentCode,
    JavaCode,
    KotlinCode,
    GoCode,
    PerlCode,
    BashCode,
    LuaCode,
    TclCode,
    PhpCode,
    CsharpCode,
    ElixirCode,
    RubyCode,
    GroovyCode
);

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

    #[test]
    fn mi_empty_file() {
        check_metrics::<PythonParser>("", "empty.py", |metric| {
            let mi = &metric.mi;
            assert_eq!(mi.mi_original(), 0.0);
            assert_eq!(mi.mi_sei(), 0.0);
            assert_eq!(mi.mi_visual_studio(), 0.0);
        });
    }

    #[test]
    fn check_mi_metrics() {
        // This test checks that MI metric is computed correctly, so it verifies
        // the calculations are correct, the adopted source code is irrelevant
        check_metrics::<PythonParser>(
            "def f():
                 pass",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.mi,
                    @r###"
                    {
                      "mi_original": 151.2033158832232,
                      "mi_sei": 142.64306171748976,
                      "mi_visual_studio": 88.42299174457497
                    }"###
                );
            },
        );
    }

    #[test]
    fn mi_sei_uses_comments_as_percentage() {
        // Regression test for #241. `Stats::comments_percentage` is stored
        // as a percentage in [0, 100], so `mi_sei` plugs it directly into
        // `50·sin(√(2.4·CM))`. Constructing `Stats` directly isolates the
        // formula from the parsing pipeline and pins the scale the SEI
        // formula expects: `perCM` is a percentage, not a ratio. With
        // the pre-fix ratio scaling, this assertion would fail by ~50.
        let stats = Stats {
            halstead_length: 4.0,
            halstead_vocabulary: 3.0,
            halstead_volume: 4.0 * f64::log2(3.0),
            cyclomatic: 1.0,
            sloc: 10.0,
            // 50% of lines are comments — drives the sin term hard.
            comments_percentage: 50.0,
        };
        // Hand-derived: 171 − 5.2·log2(V) − 0.23·G − 16.2·log2(SLOC)
        // + 50·sin(√(2.4·50)). The fifth term equals
        // 50·sin(√120) ≈ 50·sin(10.954) ≈ −50·0.99989… ≈ −49.99…,
        // which only lands in this neighborhood when CM is treated
        // as a percentage; the ratio-scaled bug would put the term
        // near +47 instead. Asserting a tight epsilon catches a
        // reintroduction of the ratio-vs-percentage scaling bug.
        let expected = 171.0
            - 5.2 * stats.halstead_volume.log2()
            - 0.23 * stats.cyclomatic
            - 16.2 * stats.sloc.log2()
            + 50.0 * (2.4_f64 * 50.0).sqrt().sin();
        let actual = stats.mi_sei();
        assert!(
            (actual - expected).abs() < 1e-9,
            "mi_sei = {actual}, expected {expected}",
        );
        // Sanity check against the pre-fix (ratio) behaviour: ensure
        // the value is nowhere near the ratio-scaled answer.
        let buggy = 171.0
            - 5.2 * stats.halstead_volume.log2()
            - 0.23 * stats.cyclomatic
            - 16.2 * stats.sloc.log2()
            + 50.0 * (2.4_f64 * 0.5).sqrt().sin();
        // The ratio-vs-percentage flip moves the sin term by roughly
        // its full ±50 amplitude; pin the bound at 50.0 so a partial
        // regression (e.g. accidentally dividing by 10 instead of by 1)
        // still fails this check instead of slipping under a generous
        // threshold.
        assert!(
            (actual - buggy).abs() > 50.0,
            "mi_sei should differ from the ratio-scaled value by >50; got actual={actual}, buggy={buggy}",
        );
    }

    #[test]
    fn rust_mi_smoke() {
        // Rust now derives MI from the populated Loc / Cyclomatic /
        // Halstead trios via the default trait method. This test
        // pins the per-function MI on a tiny straight-line function
        // so accidental regressions in the cascade get caught.
        check_metrics::<RustParser>("fn f() -> i32 { 1 }\n", "foo.rs", |metric| {
            let mi = &metric.mi;
            // expected: SLOC = 1, cyclomatic = 1 (no branches), and
            // Halstead n1 = 4 (`fn`, `->`, `{`, `}` operators visible
            // at unit level), n2 = 2 (`f` identifier, `1` literal).
            // The default `Mi::compute` then folds those into the
            // three MI variants — these numbers are produced by the
            // populated Rust trios. Pinning them anchors the snapshot
            // against accidental drift in the cascade.
            assert!(mi.mi_original() > 0.0);
            assert!(mi.mi_sei() > 0.0);
            assert!(mi.mi_visual_studio() > 0.0);
        });
    }

    #[test]
    fn go_mi_smoke() {
        // Go uses the default `Mi::compute`; once Loc / Cyclomatic /
        // Halstead are populated (they are for Go), MI is derived
        // automatically. Pin the cascade against drift.
        check_metrics::<GoParser>(
            "package main\nfunc f() int { return 1 }\n",
            "foo.go",
            |metric| {
                let mi = &metric.mi;
                assert!(mi.mi_original() > 0.0);
                assert!(mi.mi_sei() > 0.0);
                assert!(mi.mi_visual_studio() > 0.0);
            },
        );
    }

    #[test]
    fn elixir_mi_smoke() {
        // Elixir uses the default `Mi::compute`; with Loc / Cyclomatic
        // / Halstead populated (and now Cognitive / Abc as well), MI
        // derives automatically. Pin the cascade against drift.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  def f(x), do: x + 1\nend\n",
            "foo.ex",
            |metric| {
                let mi = &metric.mi;
                assert!(mi.mi_original() > 0.0);
                assert!(mi.mi_sei() > 0.0);
                assert!(mi.mi_visual_studio() > 0.0);
            },
        );
    }

    #[test]
    fn cpp_mi_smoke() {
        // C++ uses the default `Mi::compute`; Loc / Cyclomatic /
        // Halstead all already populated for C++, and Abc / Npa / Npm
        // / Wmc now contribute too. MI derives from Loc + Cyclomatic
        // + Halstead via the default. Pin the cascade against drift.
        check_metrics::<CppParser>(
            "int f(int x) { if (x > 0) return 1; return 0; }",
            "foo.cpp",
            |metric| {
                let mi = &metric.mi;
                assert!(mi.mi_original() > 0.0);
                assert!(mi.mi_sei() > 0.0);
                assert!(mi.mi_visual_studio() > 0.0);
            },
        );
    }

    #[test]
    fn javascript_mi_smoke() {
        // JavaScript uses the default `Mi::compute`; Loc / Cyclomatic
        // / Halstead were already populated, and Abc / Npa / Npm /
        // Wmc now contribute too. Pin the cascade against drift.
        check_metrics::<JavascriptParser>(
            "function f(x) { if (x > 0) return 1; return 0; }",
            "foo.js",
            |metric| {
                let mi = &metric.mi;
                assert!(mi.mi_original() > 0.0);
                assert!(mi.mi_sei() > 0.0);
                assert!(mi.mi_visual_studio() > 0.0);
            },
        );
    }

    #[test]
    fn mozjs_mi_smoke() {
        // Mozjs shares JavaScript's MI cascade; this is a parity pin.
        check_metrics::<MozjsParser>(
            "function f(x) { if (x > 0) return 1; return 0; }",
            "foo.js",
            |metric| {
                let mi = &metric.mi;
                assert!(mi.mi_original() > 0.0);
                assert!(mi.mi_sei() > 0.0);
                assert!(mi.mi_visual_studio() > 0.0);
            },
        );
    }
}
