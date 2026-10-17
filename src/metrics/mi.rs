// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::enum_glob_use, clippy::unused_self, clippy::wildcard_imports)]
// `u64` integral metric accessors (Halstead length/vocabulary, cyclomatic
// sum, sloc/cloc) are widened to `f64` for the MI formulas; the casts are
// bounded by the counts they came from (#530).
#![allow(clippy::cast_precision_loss)]

use std::fmt;

use super::cyclomatic;
use super::halstead;
use super::loc;

use crate::checker::Checker;
use crate::macros::implement_metric_trait;

use crate::*;

/// The `Mi` metric.
#[derive(Default, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Stats {
    halstead_length: f64,
    halstead_vocabulary: f64,
    halstead_volume: f64,
    cyclomatic: f64,
    /// The size term: `sloc` less the documentation rows (`dloc`) that
    /// carry no code (#1597). Comment and blank rows stay charged, as
    /// the published formulas charge them; `compute` has the rule.
    code_lines: f64,
    /// Non-documentation comment lines as a percentage of `code_lines`, in
    /// [0, 100] (not a ratio in [0, 1]). Only `sei` consumes this — the
    /// SEI MI formula uses `perCM` on the percentage scale; see issue
    /// #241.
    comments_percentage: f64,
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "original: {}, sei: {}, visual_studio: {}",
            self.original(),
            self.sei(),
            self.visual_studio()
        )
    }
}

impl Stats {
    // Intentionally a no-op. MI is a derived metric: the parent space
    // recomputes it from its merged Loc / Cyclomatic / Halstead inputs
    // (`compute_halstead_and_mi` in `spaces/compute.rs`), so there is
    // nothing to roll up from a child's finalized `Stats`. Combining
    // the fields here would double-apply inputs already captured by the
    // parent's recompute. (Same rationale as `halstead::Stats::merge`.)
    pub(crate) fn merge(&mut self, _other: &Stats) {}

    #[inline]
    fn inputs_are_empty(&self) -> bool {
        self.halstead_volume <= 0.0 || self.code_lines <= 0.0
    }

    /// Returns the `Mi` metric calculated using the original formula.
    ///
    /// Its value can be negative.
    #[inline]
    #[must_use]
    pub fn original(&self) -> f64 {
        if self.inputs_are_empty() {
            return 0.0;
        }
        // http://www.projectcodemeter.com/cost_estimation/help/GL_maintainability.htm
        171.0
            - 5.2 * (self.halstead_volume).ln()
            - 0.23 * self.cyclomatic
            - 16.2 * self.code_lines.ln()
    }

    /// Returns the `Mi` metric calculated using the derivative formula
    /// employed by the Software Engineering Insitute (SEI).
    ///
    /// Its value can be negative.
    #[inline]
    #[must_use]
    pub fn sei(&self) -> f64 {
        if self.inputs_are_empty() {
            return 0.0;
        }
        // http://www.projectcodemeter.com/cost_estimation/help/GL_maintainability.htm
        171.0
            - 5.2 * self.halstead_volume.log2()
            - 0.23 * self.cyclomatic
            - 16.2 * self.code_lines.log2()
            + 50.0 * (self.comments_percentage * 2.4).sqrt().sin()
    }

    /// Returns the `Mi` metric calculated using the derivative formula
    /// employed by Microsoft Visual Studio.
    #[inline]
    #[must_use]
    pub fn visual_studio(&self) -> f64 {
        if self.inputs_are_empty() {
            return 0.0;
        }
        // http://www.projectcodemeter.com/cost_estimation/help/GL_maintainability.htm
        let formula = 171.0
            - 5.2 * self.halstead_volume.ln()
            - 0.23 * self.cyclomatic
            - 16.2 * self.code_lines.ln();
        (formula * 100.0 / 171.0).max(0.)
    }
}

#[doc(hidden)]
/// Per-language computation of the maintainability index.
pub(crate) trait Mi
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
        stats.halstead_length = halstead.length() as f64;
        stats.halstead_vocabulary = halstead.vocabulary() as f64;
        stats.halstead_volume = halstead.volume();
        stats.cyclomatic = cyclomatic.cyclomatic_sum() as f64;
        // Documentation rows (`dloc`: doc comments, docstrings, POD) are
        // not code: they leave the size term and both sides of the comment
        // ratio, so a function scores the same with and without its
        // documentation (#1597). Halstead agrees: it never sees a comment
        // and drops every bare string's operands (#695).
        //
        // A row the documentation shares with code stays in the size term:
        // `def f(): """doc"""` is one code row, not none.
        //
        // The subtractions are exact, since `dloc <= cloc <= sloc` holds
        // after `loc::Stats::settle_excluded_rows`; `saturating_sub` is
        // only the release-build guard.
        let dloc = loc.dloc();
        let code_lines = loc.sloc().saturating_sub(loc.dloc_without_code());
        let comment_lines = loc.cloc().saturating_sub(dloc);
        stats.code_lines = code_lines as f64;
        // The SEI Maintainability Index expects `perCM` as a percentage
        // in [0, 100], not a ratio in [0, 1] — `50·sin(√(2.4·CM))` is
        // nonsensical when CM is two orders of magnitude too small. See
        // issue #241 and Welker/Oman's original MI definition.
        stats.comments_percentage = if code_lines == 0 {
            0.0
        } else {
            // Clamp to [0, 100]: a comment ratio is a percentage of
            // source lines and cannot exceed 100%. The SEI term
            // `50·sin(√(2.4·CM))` has no clamp of its own, so an
            // out-of-range CM (e.g. cloc > sloc) would distort
            // `sei` by tens of points (issue #461).
            (comment_lines as f64 / stats.code_lines * 100.0).clamp(0.0, 100.0)
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
    MozcppCode,
    CCode,
    ObjcCode,
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
    GroovyCode,
    IrulesCode
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
    use crate::test_support::check_metrics_only_shim;
    #[cfg(any(feature = "python", all(feature = "rust", feature = "java")))]
    use crate::test_support::metrics_verbatim;

    use super::*;

    // Mi's dependency closure adds Loc + Cyclomatic + Halstead — the
    // three inputs its formula consumes. No assertion here reads them
    // directly; without them MI would be computed from zero-valued
    // defaults.
    check_metrics_only_shim!(check_metrics, Mi);

    /// Analyses `source` byte-for-byte with the walk restricted to MI
    /// and its three inputs (#1127), returning the metrics so two parses
    /// can be compared.
    #[cfg(feature = "python")]
    fn python_mi(source: &[u8]) -> CodeMetrics {
        metrics_verbatim(
            crate::LANG::Python,
            source,
            MetricsOptions::default().with_only(&[Metric::Mi]),
        )
    }

    #[cfg(feature = "python")]
    #[test]
    fn mi_empty_file() {
        check_metrics::<PythonParser>("", "empty.py", |metric| {
            let mi = &metric.mi;
            assert_eq!(mi.original(), 0.0);
            assert_eq!(mi.sei(), 0.0);
            assert_eq!(mi.visual_studio(), 0.0);
        });
    }

    #[cfg(feature = "python")]
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
                    @r#"
                {
                  "original": 146.56146936754593,
                  "sei": 135.94629276875443,
                  "visual_studio": 85.70846161844791
                }
                "#
                );
            },
        );
    }

    /// The #1597 reproducer: the same function with and without a
    /// docstring scores identically on all three variants, because the
    /// docstring's rows leave the size term and both sides of the
    /// comment ratio. Before #1597 the documented side scored lower on
    /// every variant; the CHANGELOG entry records by how much.
    #[cfg(feature = "python")]
    #[test]
    fn python_docstring_does_not_move_mi() {
        let undocumented = b"def clamp(value, low, high):\n    if value < low:\n        return low\n    if value > high:\n        return high\n    return value\n";
        let documented = b"def clamp(value, low, high):\n    \"\"\"Return ``value`` held within ``[low, high]``.\n\n    ``low`` and ``high`` are inclusive bounds; a value outside them is\n    replaced by the nearer bound, and a value inside is returned as it\n    is. The bounds are not checked against each other, so ``low > high``\n    returns ``high`` for every input.\n    \"\"\"\n    if value < low:\n        return low\n    if value > high:\n        return high\n    return value\n";
        let bare = python_mi(undocumented);
        let doc = python_mi(documented);
        // The fixture really carries the docstring the test is about.
        assert_eq!((doc.loc.dloc(), doc.loc.cloc(), doc.loc.sloc()), (7, 7, 13));
        assert_eq!((bare.loc.dloc(), bare.loc.sloc()), (0, 6));
        assert_eq!(
            (doc.mi.original(), doc.mi.sei(), doc.mi.visual_studio()),
            (bare.mi.original(), bare.mi.sei(), bare.mi.visual_studio()),
            "a docstring must not move any MI variant"
        );
        // And the values are the undocumented file's known ones, so the
        // equality above is not two zeros agreeing.
        assert!((bare.mi.original() - 117.676_252_636_666_53).abs() < 1e-9);
    }

    #[cfg(feature = "python")]
    #[test]
    fn python_additional_docstrings_do_not_move_mi() {
        for (bare, documented, rows) in [
            (
                "def f():\n    return 1\n",
                "def f():\n    \"\"\"F.\"\"\"\n    \"\"\"More.\"\"\"\n    \"\"\"Still more.\"\"\"\n    return 1\n",
                3,
            ),
            ("x = 1\n", "x = 1\n\"\"\"X.\"\"\"\n\"\"\"More.\"\"\"\n", 2),
        ] {
            let bare = python_mi(bare.as_bytes());
            let documented = python_mi(documented.as_bytes());
            assert_eq!(documented.loc.dloc(), rows);
            assert!(bare.mi.original() > 0.0);
            assert_eq!(
                (bare.mi.original(), bare.mi.sei(), bare.mi.visual_studio()),
                (
                    documented.mi.original(),
                    documented.mi.sei(),
                    documented.mi.visual_studio(),
                )
            );
        }
    }

    /// The comment ratio side of the same rule: with a real comment in
    /// the body, `sei`'s `50·sin(√(2.4·CM))` term reads
    /// `(cloc − dloc) / (sloc − dloc)` when no documentation row carries
    /// code, so adding a docstring changes neither numerator nor
    /// denominator. Subtracting the docstring from only the size term
    /// would leave `CM` inflated by the docstring rows and move `sei` on
    /// its own.
    #[cfg(feature = "python")]
    #[test]
    fn python_docstring_is_invisible_to_the_sei_comment_term() {
        let commented =
            b"def f(n):\n    # a real comment\n    if n:\n        return 1\n    return 0\n";
        let both = b"def f(n):\n    \"\"\"Doc one.\n    Doc two.\n    \"\"\"\n    # a real comment\n    if n:\n        return 1\n    return 0\n";
        let a = python_mi(commented);
        let b = python_mi(both);
        assert_eq!((a.loc.cloc(), a.loc.dloc()), (1, 0));
        assert_eq!((b.loc.cloc(), b.loc.dloc()), (4, 3));
        assert_eq!(
            a.mi.sei(),
            b.mi.sei(),
            "sei must see the one comment row in both"
        );
        assert_eq!(a.mi.original(), b.mi.original());
        // The comment term is live in this fixture: `sei` is not just the
        // comment-free formula, so the equality above exercises it.
        let bare = python_mi(b"def f(n):\n    if n:\n        return 1\n    return 0\n");
        assert_ne!(
            a.mi.sei(),
            bare.mi.sei(),
            "the comment row must move sei on its own"
        );
    }

    /// A docstring that shares a row with code keeps that row in the
    /// size term: `def f(x): """doc"""; return x` is one code row, not
    /// none, so it scores from `ln(1)` rather than as the `0.0`
    /// empty-input sentinel, and a docstring whose closing quote is
    /// followed by a statement gives up only the rows it has to itself.
    /// Each fixture's `cloc` is all docstring, so the SEI comment term
    /// is `sin(0)` and both variants reduce to the size formula.
    #[cfg(feature = "python")]
    #[test]
    fn python_docstring_sharing_a_code_row_keeps_the_row() {
        // (source, docstring rows, code rows MI must size on)
        let fixtures: [(&[u8], u64, f64); 3] = [
            (b"def f(x): \"\"\"Return x.\"\"\"\n", 1, 1.0),
            (b"def f(x): \"\"\"Return x.\"\"\"; return x\n", 1, 1.0),
            (
                b"def f(x):\n    \"\"\"Doc.\n    More.\"\"\"; y = 1\n    return x\n",
                2,
                3.0,
            ),
        ];
        for (source, dloc, code_lines) in fixtures {
            let m = python_mi(source);
            assert_eq!((m.loc.dloc(), m.loc.cloc()), (dloc, dloc));
            let volume = m.halstead.volume();
            let cyclomatic = m.cyclomatic.cyclomatic_sum() as f64;
            let original = 171.0 - 5.2 * volume.ln() - 0.23 * cyclomatic - 16.2 * code_lines.ln();
            let sei = 171.0 - 5.2 * volume.log2() - 0.23 * cyclomatic - 16.2 * code_lines.log2();
            assert!(
                (m.mi.original() - original).abs() < 1e-9 && (m.mi.sei() - sei).abs() < 1e-9,
                "{:?}: original {} / sei {} must size on {code_lines} code rows ({original} / {sei})",
                std::str::from_utf8(source),
                m.mi.original(),
                m.mi.sei()
            );
            assert!(m.mi.original() > 0.0, "not the empty-input sentinel");
        }
    }

    /// Commented-out code in a triple-quoted string mid-function is not
    /// documentation (#1597): MI charges it exactly as it charges the same
    /// rows commented out with `#`.
    #[cfg(feature = "python")]
    #[test]
    fn python_code_commented_out_in_a_string_is_charged_like_a_comment() {
        let in_string = b"def f(n):\n    if n:\n        return 1\n    \"\"\"\n    old = n * 2\n    return old\n    \"\"\"\n    return 0\n";
        let with_hash = b"def f(n):\n    if n:\n        return 1\n    #\n    # old = n * 2\n    # return old\n    #\n    return 0\n";
        let a = python_mi(in_string);
        let b = python_mi(with_hash);
        assert_eq!(
            (a.loc.cloc(), a.loc.dloc()),
            (4, 0),
            "the string is comment rows, not docs"
        );
        assert_eq!((b.loc.cloc(), b.loc.dloc()), (4, 0));
        assert_eq!(
            (a.mi.original(), a.mi.sei(), a.mi.visual_studio()),
            (b.mi.original(), b.mi.sei(), b.mi.visual_studio()),
            "a string-wrapped and a #-commented block must cost the same"
        );
    }

    /// The documentation rule is language-agnostic once `dloc` is: a
    /// Rust `///` block and a Javadoc comment leave every variant
    /// unmoved, exactly as the Python docstring does above (#1597).
    #[cfg(all(feature = "rust", feature = "java"))]
    #[test]
    fn doc_comments_do_not_move_mi_in_other_languages() {
        let cases: &[(crate::LANG, &[u8], &[u8], u64)] = &[
            (
                crate::LANG::Rust,
                b"fn f(x: i32) -> i32 {\n    if x > 0 { x } else { 0 }\n}\n",
                b"/// Clamp `x` at zero.\n///\n/// Negative inputs collapse to zero.\nfn f(x: i32) -> i32 {\n    if x > 0 { x } else { 0 }\n}\n",
                3,
            ),
            (
                crate::LANG::Java,
                b"class C {\n    int f(int x) {\n        if (x > 0) { return x; }\n        return 0;\n    }\n}\n",
                b"class C {\n    /**\n     * Clamp x at zero.\n     */\n    int f(int x) {\n        if (x > 0) { return x; }\n        return 0;\n    }\n}\n",
                3,
            ),
        ];
        let mi_only = || MetricsOptions::default().with_only(&[Metric::Mi]);
        for (lang, bare, documented, rows) in cases {
            let a = metrics_verbatim(*lang, bare, mi_only());
            let b = metrics_verbatim(*lang, documented, mi_only());
            assert_eq!(
                (b.loc.dloc(), b.loc.cloc()),
                (*rows, *rows),
                "{lang:?}: the doc rows"
            );
            assert_eq!(a.loc.dloc(), 0);
            assert_eq!(
                (a.mi.original(), a.mi.sei(), a.mi.visual_studio()),
                (b.mi.original(), b.mi.sei(), b.mi.visual_studio()),
                "{lang:?}: a doc comment must not move any MI variant"
            );
            assert!(
                a.mi.original() > 0.0,
                "{lang:?}: the parity is not two zeros agreeing"
            );
        }
    }

    #[test]
    fn mi_sei_uses_comments_as_percentage() {
        // Regression test for #241. `Stats::comments_percentage` is stored
        // as a percentage in [0, 100], so `sei` plugs it directly into
        // `50·sin(√(2.4·CM))`. Constructing `Stats` directly isolates the
        // formula from the parsing pipeline and pins the scale the SEI
        // formula expects: `perCM` is a percentage, not a ratio. With
        // the pre-fix ratio scaling, this assertion would fail by ~50.
        let stats = Stats {
            halstead_length: 4.0,
            halstead_vocabulary: 3.0,
            halstead_volume: 4.0 * f64::log2(3.0),
            cyclomatic: 1.0,
            code_lines: 10.0,
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
            - 16.2 * stats.code_lines.log2()
            + 50.0 * (2.4_f64 * 50.0).sqrt().sin();
        let actual = stats.sei();
        assert!(
            (actual - expected).abs() < 1e-9,
            "sei = {actual}, expected {expected}",
        );
        // Sanity check against the pre-fix (ratio) behaviour: ensure
        // the value is nowhere near the ratio-scaled answer.
        let buggy = 171.0
            - 5.2 * stats.halstead_volume.log2()
            - 0.23 * stats.cyclomatic
            - 16.2 * stats.code_lines.log2()
            + 50.0 * (2.4_f64 * 0.5).sqrt().sin();
        // The ratio-vs-percentage flip moves the sin term by roughly
        // its full ±50 amplitude; pin the bound at 50.0 so a partial
        // regression (e.g. accidentally dividing by 10 instead of by 1)
        // still fails this check instead of slipping under a generous
        // threshold.
        assert!(
            (actual - buggy).abs() > 50.0,
            "sei should differ from the ratio-scaled value by >50; got actual={actual}, buggy={buggy}",
        );
    }

    #[cfg(feature = "rust")]
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
            assert!(mi.original() > 0.0);
            assert!(mi.sei() > 0.0);
            assert!(mi.visual_studio() > 0.0);
        });
    }

    #[cfg(feature = "go")]
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
                assert!(mi.original() > 0.0);
                assert!(mi.sei() > 0.0);
                assert!(mi.visual_studio() > 0.0);
            },
        );
    }

    #[cfg(feature = "elixir")]
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
                assert!(mi.original() > 0.0);
                assert!(mi.sei() > 0.0);
                assert!(mi.visual_studio() > 0.0);
            },
        );
    }

    #[cfg(feature = "cpp")]
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
                assert!(mi.original() > 0.0);
                assert!(mi.sei() > 0.0);
                assert!(mi.visual_studio() > 0.0);
            },
        );
    }

    #[cfg(feature = "javascript")]
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
                assert!(mi.original() > 0.0);
                assert!(mi.sei() > 0.0);
                assert!(mi.visual_studio() > 0.0);
            },
        );
    }

    #[cfg(feature = "mozjs")]
    #[test]
    fn mozjs_mi_smoke() {
        // Mozjs shares JavaScript's MI cascade; this is a parity pin.
        check_metrics::<MozjsParser>(
            "function f(x) { if (x > 0) return 1; return 0; }",
            "foo.js",
            |metric| {
                let mi = &metric.mi;
                assert!(mi.original() > 0.0);
                assert!(mi.sei() > 0.0);
                assert!(mi.visual_studio() > 0.0);
            },
        );
    }

    /// `comments_percentage` feeds the unclamped SEI term
    /// `50·sin(√(2.4·CM))`. A degenerate `Loc` with `cloc > sloc`
    /// (which the loc.rs fix prevents for parsed input, but which the
    /// clamp defends regardless) must yield a `comments_percentage`
    /// capped at exactly 100, not the ~209 the raw ratio would give
    /// (issue #461). Here `cloc = 2`, `sloc = 1` => raw 200%. Reverting
    /// the `.clamp(0.0, 100.0)` in `Mi::compute` makes this fail.
    #[cfg(feature = "python")]
    #[test]
    fn mi_comments_percentage_clamped() {
        // cloc = 2 (degenerate), sloc = 1 (single non-unit row) => raw
        // comments_percentage = 200%.
        let loc = loc::Stats::with_cloc_sloc(2, 0);
        assert_eq!(loc.cloc(), 2);
        assert_eq!(loc.sloc(), 1);

        let cyclomatic = cyclomatic::Stats::default();
        let halstead = halstead::Stats::default();
        let mut mi = Stats::default();
        PythonCode::compute(&loc, &cyclomatic, &halstead, &mut mi);

        assert!(
            (mi.comments_percentage - 100.0).abs() < f64::EPSILON,
            "comments_percentage must clamp to 100, got {}",
            mi.comments_percentage
        );
    }
}
