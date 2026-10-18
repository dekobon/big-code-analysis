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
    /// The size term `L`: `ploc`, the rows that carry code, in both
    /// documentation modes. Comment, documentation and blank rows never
    /// enlarge it (#1607).
    code_lines: f64,
    /// The SEI comment ratio `CM`, in [0, 1]: comment rows over physical
    /// rows. By default documentation leaves both sides —
    /// `(cloc − dloc) / (sloc − dloc_without_code)` — and with
    /// `MetricsOptions::with_mi_charges_docs(true)` it is `cloc / sloc`.
    /// Only `sei` consumes it.
    comment_ratio: f64,
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

    /// Returns `true` when the space holds no code to measure: its
    /// Halstead volume or its size term is zero.
    ///
    /// Every variant then returns the in-band sentinel `0.0`, which
    /// cannot be told from a genuine 0 or from `mi.visual_studio`'s
    /// clamp. Consumers that aggregate MI must skip these spaces. The
    /// wire/JSON value stays `0.0` in 2.x; serialising `null` is
    /// deferred to 3.0 (#1621).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.halstead_volume <= 0.0 || self.code_lines <= 0.0
    }

    /// Returns the `Mi` metric calculated using the original formula,
    /// `171 − 5.2·ln(V) − 0.23·G − 16.2·ln(L)` with `V` the Halstead
    /// volume, `G` the cyclomatic sum and `L` the space's `ploc`.
    ///
    /// Its value can be negative.
    #[inline]
    #[must_use]
    pub fn original(&self) -> f64 {
        if self.is_empty() {
            return 0.0;
        }
        // http://www.projectcodemeter.com/cost_estimation/help/GL_maintainability.htm
        171.0
            - 5.2 * (self.halstead_volume).ln()
            - 0.23 * self.cyclomatic
            - 16.2 * self.code_lines.ln()
    }

    /// Returns the `Mi` metric calculated using the derivative formula
    /// employed by the Software Engineering Institute (SEI): the
    /// [`original`](Self::original) value plus `50·sin(√(2.4·CM))`, with
    /// `CM` the comment ratio in [0, 1]. The comment term rises
    /// monotonically from 0 to `50·sin(√2.4)` ≈ 49.99, so `sei` is never
    /// below `original` and equals it when the space has no comment.
    ///
    /// Its value can be negative.
    #[inline]
    #[must_use]
    pub fn sei(&self) -> f64 {
        if self.is_empty() {
            return 0.0;
        }
        self.original() + 50.0 * (2.4 * self.comment_ratio).sqrt().sin()
    }

    /// Returns the `Mi` metric calculated using the derivative formula
    /// employed by Microsoft Visual Studio: the
    /// [`original`](Self::original) value rescaled to `[0, 100]` as
    /// `max(0, original·100/171)`.
    #[inline]
    #[must_use]
    pub fn visual_studio(&self) -> f64 {
        // http://www.projectcodemeter.com/cost_estimation/help/GL_maintainability.htm
        // `original()` is already the `0.0` sentinel for an empty space.
        (self.original() * 100.0 / 171.0).max(0.)
    }
}

#[doc(hidden)]
/// Per-language computation of the maintainability index.
pub(crate) trait Mi
where
    Self: Checker,
{
    /// Fold the already-computed inputs into `stats`. `charges_docs` is
    /// `MetricsOptions::mi_charges_docs`: whether documentation rows
    /// count toward the SEI comment ratio.
    fn compute(
        loc: &loc::Stats,
        cyclomatic: &cyclomatic::Stats,
        halstead: &halstead::Stats,
        stats: &mut Stats,
        charges_docs: bool,
    ) {
        stats.halstead_length = halstead.length() as f64;
        stats.halstead_vocabulary = halstead.vocabulary() as f64;
        stats.halstead_volume = halstead.volume();
        stats.cyclomatic = cyclomatic.cyclomatic_sum() as f64;
        // The size term is `ploc`, the rows that carry code (#1607):
        // comments, documentation and blank rows describe the code, and
        // none of them makes it harder to maintain. A row shared between
        // documentation and code (`def f(): """doc"""`) is a code row and
        // stays in.
        stats.code_lines = loc.ploc() as f64;
        // The SEI comment ratio `CM` in [0, 1]. Documentation rows (`dloc`:
        // doc comments, docstrings, POD) leave both sides by default, so a
        // documented space reads the share of working comments among its
        // other rows (#1597); with `charges_docs` every comment row over
        // every physical row counts. Blank rows stay in the denominator.
        //
        // The subtractions are exact, since `dloc <= cloc <= sloc` holds
        // after `loc::Stats::settle_excluded_rows`; `saturating_sub` is
        // only the release-build guard.
        let (comment_lines, physical_lines) = if charges_docs {
            (loc.cloc(), loc.sloc())
        } else {
            (
                loc.cloc().saturating_sub(loc.dloc()),
                loc.sloc().saturating_sub(loc.dloc_without_code()),
            )
        };
        stats.comment_ratio = if physical_lines == 0 {
            0.0
        } else {
            // A degenerate `Loc` (cloc > sloc) must not push `CM` past 1,
            // where the `sin` term turns back down (#461).
            (comment_lines as f64 / physical_lines as f64).clamp(0.0, 1.0)
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

    /// Analyses `source` byte-for-byte in both documentation modes:
    /// `(default, mi_charges_docs)`.
    #[cfg(feature = "python")]
    fn python_modes(source: &[u8]) -> (CodeMetrics, CodeMetrics) {
        let read = |charges_docs: bool| {
            metrics_verbatim(
                crate::LANG::Python,
                source,
                MetricsOptions::default().with_mi_charges_docs(charges_docs),
            )
        };
        (read(false), read(true))
    }

    /// The documented fixture `big-code-analysis-py/tests/test_mi_charges_docs.py`
    /// drives through the Python bindings.
    #[cfg(feature = "python")]
    const DOCUMENTED: &[u8] = b"def clamp(value, low, high):\n    \"\"\"Return ``value`` held within ``[low, high]``.\n\n    The bounds are inclusive and are not checked against each\n    other, so ``low > high`` returns ``high`` for every input.\n    \"\"\"\n    if value < low:\n        return low\n    if value > high:\n        return high\n    return value\n";

    /// Pins a comment-free space's integer inputs — Halstead length `N`
    /// and vocabulary `n`, the cyclomatic sum `G`, and `ploc` — and its
    /// MI against `original` / `visual_studio` constants derived by hand
    /// from them, with `sei` equal to `original` bit for bit (`CM = 0`).
    #[cfg(any(
        feature = "cpp",
        feature = "elixir",
        feature = "go",
        feature = "javascript",
        feature = "mozjs",
        feature = "rust"
    ))]
    #[track_caller]
    fn assert_comment_free_mi(
        m: &CodeMetrics,
        inputs: (u64, u64, u64, u64),
        original: f64,
        visual_studio: f64,
    ) {
        assert_eq!(
            (
                m.halstead.length(),
                m.halstead.vocabulary(),
                m.cyclomatic.cyclomatic_sum(),
                m.loc.ploc()
            ),
            inputs,
            "(N, n, G, ploc)"
        );
        assert_eq!(m.loc.cloc(), 0, "the fixture is comment-free");
        assert!(
            (m.mi.original() - original).abs() < 1e-9,
            "original {} vs {original}",
            m.mi.original()
        );
        assert!(
            (m.mi.visual_studio() - visual_studio).abs() < 1e-9,
            "visual_studio {} vs {visual_studio}",
            m.mi.visual_studio()
        );
        assert_eq!(m.mi.sei().to_bits(), m.mi.original().to_bits());
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

    #[test]
    fn empty_inputs_are_empty_and_every_variant_is_the_sentinel() {
        let mut mi = Stats::default();
        assert!(mi.is_empty());
        assert_eq!([mi.original(), mi.sei(), mi.visual_studio()], [0.0; 3]);
        // Either input alone being zero is enough.
        mi.halstead_volume = 10.0;
        assert!(mi.is_empty(), "zero size term");
        mi.halstead_volume = 0.0;
        mi.code_lines = 3.0;
        assert!(mi.is_empty(), "zero Halstead volume");
        mi.halstead_volume = 10.0;
        assert!(!mi.is_empty());
        assert!(mi.original() > 0.0);
    }

    #[cfg(feature = "python")]
    #[test]
    fn a_trivial_function_is_not_empty() {
        check_metrics::<PythonParser>("def f():\n    pass\n", "foo.py", |metric| {
            assert!(!metric.mi.is_empty());
            assert!(metric.mi.original() > 0.0);
        });
    }

    /// An `#include`-only header has rows but no Halstead tokens, so it
    /// is empty end to end through the parser: the R3 reproducer of
    /// #1621, where `fstlib.h` dragged `bca report`'s average down.
    #[cfg(feature = "cpp")]
    #[test]
    fn an_include_only_file_is_empty() {
        check_metrics::<CppParser>(
            "#include <vector>\n#include \"fst.h\"\n",
            "fstlib.h",
            |metric| {
                assert_eq!(metric.loc.sloc(), 2);
                assert!(metric.mi.is_empty());
                assert_eq!(metric.mi.original(), 0.0);
            },
        );
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
                // expected: no comment row, so `sei` is `original` exactly.
                insta::assert_json_snapshot!(
                    metric.mi,
                    @r#"
                {
                  "original": 146.56146936754593,
                  "sei": 146.56146936754593,
                  "visual_studio": 85.70846161844791
                }
                "#
                );
            },
        );
    }

    /// The #1597 reproducer: the same function with and without a
    /// docstring scores identically on all three variants, because the
    /// docstring's rows are not `ploc` and leave both sides of the
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

    /// `with_mi_charges_docs` moves only the SEI comment ratio (#1607):
    /// the size term is `ploc` in both modes, so `original` and
    /// `visual_studio` are bit-identical, while `sei` reads `cloc / sloc`
    /// when charged and the working-comment share otherwise. The
    /// documented fixture has no working comment, so the default ratio is
    /// 0 and charging the docstring can only raise `sei`. The root and the
    /// named child are both asserted: the root is finalised on the stack's
    /// last element and a child on its pop, two paths that must both carry
    /// the flag.
    ///
    /// expected: `V = 25·log2 12` ≈ 89.6241, `L = ploc = 6`, `G = 4` at the
    /// root and 3 in `clamp`: `original = 171 − 5.2·ln V − 0.23·G −
    /// 16.2·ln 6` ≈ 117.6763 (root) / 117.9063 (`clamp`). Charged,
    /// `CM = cloc/sloc = 5/11` and `sei` adds `50·sin(√(12/11))` ≈ 43.2328.
    #[cfg(feature = "python")]
    #[test]
    fn charging_docs_moves_only_the_sei_comment_share() {
        fn child(root: &FuncSpace) -> &CodeMetrics {
            &root
                .spaces
                .iter()
                .find(|s| s.name.as_deref() == Some("clamp"))
                .expect("the function space")
                .metrics
        }
        let space = |charges_docs: bool| {
            crate::test_support::space_verbatim(
                crate::LANG::Python,
                DOCUMENTED,
                MetricsOptions::default().with_mi_charges_docs(charges_docs),
            )
        };
        let (default, charged) = (space(false), space(true));
        let pairs = [
            (
                &default.metrics,
                &charged.metrics,
                (4, 117.676_252_636_666_53, 160.909_070_973_313_36),
            ),
            (
                child(&default),
                child(&charged),
                (3, 117.906_252_636_666_52, 161.139_070_973_313_32),
            ),
        ];
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        for (d, c, (cyclomatic, original, charged_sei)) in pairs {
            assert_eq!(
                (d.loc.ploc(), d.loc.cloc(), d.loc.sloc(), d.loc.dloc()),
                (6, 5, 11, 5),
                "the fixture is documented"
            );
            assert_eq!((d.halstead.length(), d.halstead.vocabulary()), (25, 12));
            assert_eq!(d.cyclomatic.cyclomatic_sum(), cyclomatic);
            assert!(close(d.mi.original(), original), "{}", d.mi.original());
            assert!(close(c.mi.sei(), charged_sei), "{}", c.mi.sei());
            assert_eq!(d.mi.original().to_bits(), c.mi.original().to_bits());
            assert_eq!(
                d.mi.visual_studio().to_bits(),
                c.mi.visual_studio().to_bits()
            );
            assert_eq!(d.mi.sei(), d.mi.original(), "no working comment");
            assert!(
                c.mi.sei() > d.mi.sei(),
                "charged {} must exceed default {}",
                c.mi.sei(),
                d.mi.sei()
            );
        }
    }

    /// A docstring sharing its row with code leaves `ploc` holding that
    /// row in both modes, so the size term is one row either way.
    #[cfg(feature = "python")]
    #[test]
    fn a_shared_doc_code_row_sizes_the_same_in_both_modes() {
        let source = b"def f(): \"\"\"doc\"\"\"; return 1\n";
        let (d, c) = python_modes(source);
        assert_eq!((d.loc.dloc(), d.loc.ploc()), (1, 1));
        assert_eq!(d.mi.code_lines, 1.0);
        assert_eq!(c.mi.code_lines, 1.0);
        assert_eq!(d.mi.original(), c.mi.original());
        assert!(d.mi.original() > 0.0, "not the empty-input sentinel");
    }

    /// Blank rows leave the size term but stay in the comment ratio's
    /// denominator: with a comment present they dilute `CM` and lower
    /// `sei`; without one every variant is unmoved.
    #[cfg(feature = "python")]
    #[test]
    fn blank_rows_move_only_a_live_comment_share() {
        let commented =
            python_mi(b"def f(n):\n    # c\n    if n:\n        return 1\n    return 0\n");
        let commented_blanks =
            python_mi(b"def f(n):\n    # c\n\n\n    if n:\n        return 1\n    return 0\n");
        assert_eq!(commented_blanks.loc.blank(), commented.loc.blank() + 2);
        assert_eq!(commented.mi.original(), commented_blanks.mi.original());
        assert_eq!(
            commented.mi.visual_studio(),
            commented_blanks.mi.visual_studio()
        );
        assert!(
            commented_blanks.mi.sei() < commented.mi.sei(),
            "{} must be below {}",
            commented_blanks.mi.sei(),
            commented.mi.sei()
        );

        let bare = python_mi(b"def f(n):\n    if n:\n        return 1\n    return 0\n");
        let bare_blanks = python_mi(b"def f(n):\n\n\n    if n:\n        return 1\n    return 0\n");
        assert_eq!(bare_blanks.loc.blank(), bare.loc.blank() + 2);
        let mi = |m: &CodeMetrics| (m.mi.original(), m.mi.sei(), m.mi.visual_studio());
        assert_eq!(mi(&bare), mi(&bare_blanks));
        assert!(bare.mi.original() > 0.0);
    }

    /// Hand-derived values for a function with two code rows, one comment
    /// and two blank rows, in three languages, at the root and at the
    /// function space. The inputs are pinned as integers and the expected
    /// MI values are constants derived outside the production formula, so
    /// a change to it (`log2`, `sloc`, a percentage `CM`) fails here.
    ///
    /// expected: `L = ploc = 2`, `CM = 1/5` (one comment over five rows,
    /// no documentation), so the SEI term is `50·sin(√0.48)` ≈ 31.9355 in
    /// every case. `V = N·log2(n)`: Python `9·log2 8 = 27`, Rust
    /// `12·log2 10` ≈ 39.8631, C `11·log2 9` ≈ 34.8692. `G` is 2 at the
    /// root (unit + function) and 1 in `f`, so `f` reads 0.23 above the
    /// root. Python root: `171 − 5.2·ln 27 − 0.46 − 16.2·ln 2` =
    /// `171 − 17.1383 − 0.46 − 11.2290` ≈ 142.1727.
    #[cfg(all(feature = "python", feature = "rust", feature = "c"))]
    #[test]
    fn mi_values_derive_from_ploc_and_the_comment_ratio() {
        // (lang, source, (N, n), [root (original, sei, vs), f (…)])
        type Case = (crate::LANG, &'static [u8], (u64, u64), [(f64, f64, f64); 2]);
        let cases: [Case; 3] = [
            (
                crate::LANG::Python,
                b"def f(x):\n    # note\n\n\n    return x + 1\n",
                (9, 8),
                [
                    (
                        142.172_663_971_706_38,
                        174.108_154_504_556_7,
                        83.141_908_755_383_85,
                    ),
                    (
                        142.402_663_971_706_4,
                        174.338_154_504_556_7,
                        83.276_411_679_360_48,
                    ),
                ],
            ),
            (
                crate::LANG::Rust,
                b"fn f(x: i32) -> i32 {\n    // note\n\n\n    x + 1 }\n",
                (12, 10),
                [
                    (
                        140.146_665_193_717_27,
                        172.082_155_726_567_58,
                        81.957_114_148_372_67,
                    ),
                    (
                        140.376_665_193_717_3,
                        172.312_155_726_567_6,
                        82.091_617_072_349_3,
                    ),
                ],
            ),
            (
                crate::LANG::C,
                b"int f(int x) {\n    // note\n\n\n    return x + 1; }\n",
                (11, 9),
                [
                    (
                        140.842_679_026_834_13,
                        172.778_169_559_684_44,
                        82.364_139_781_774_36,
                    ),
                    (
                        141.072_679_026_834_15,
                        173.008_169_559_684_46,
                        82.498_642_705_750_97,
                    ),
                ],
            ),
        ];
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        for (lang, source, (length, vocabulary), expected) in cases {
            let root = crate::test_support::space_verbatim(lang, source, MetricsOptions::default());
            let f = root
                .spaces
                .iter()
                .find(|s| s.name.as_deref() == Some("f"))
                .unwrap_or_else(|| panic!("{lang:?}: the function space"));
            for ((m, cyclomatic), (original, sei, vs)) in [(&root.metrics, 2), (&f.metrics, 1)]
                .into_iter()
                .zip(expected)
            {
                assert_eq!(
                    (m.loc.ploc(), m.loc.cloc(), m.loc.blank(), m.loc.sloc()),
                    (2, 1, 2, 5),
                    "{lang:?}: the fixture's rows"
                );
                assert_eq!(
                    (
                        m.halstead.length(),
                        m.halstead.vocabulary(),
                        m.cyclomatic.cyclomatic_sum()
                    ),
                    (length, vocabulary, cyclomatic),
                    "{lang:?}: the Halstead and cyclomatic inputs"
                );
                assert!(
                    close(m.mi.original(), original)
                        && close(m.mi.sei(), sei)
                        && close(m.mi.visual_studio(), vs),
                    "{lang:?}: {} / {} / {} vs {original} / {sei} / {vs}",
                    m.mi.original(),
                    m.mi.sei(),
                    m.mi.visual_studio()
                );
            }
        }
    }
    /// Each mode pairs its own numerator with its own denominator. The
    /// fixture holds a three-row docstring and one working comment in
    /// eight rows, so the four pairings read four different ratios:
    /// default `(4 − 3)/(8 − 3) = 1/5`, charged `4/8`, and the crossed
    /// `1/8` and `4/5` that a swapped tuple would produce.
    ///
    /// expected: `V = 12·log2 9` ≈ 38.0391, `L = ploc = 4`, `G = 3` at the
    /// root, so `original = 171 − 5.2·ln V − 0.69 − 16.2·ln 4` ≈ 128.9312;
    /// `sei` adds `50·sin(√0.48)` ≈ 31.9355 by default and
    /// `50·sin(√1.2)` ≈ 44.4566 charged.
    #[cfg(feature = "python")]
    #[test]
    fn each_mode_pairs_its_comment_rows_with_its_own_rows() {
        let source = b"def f(n):\n    \"\"\"Doc one.\n    Doc two.\n    \"\"\"\n    # a real comment\n    if n:\n        return 1\n    return 0\n";
        let (d, c) = python_modes(source);
        assert_eq!(
            (d.loc.ploc(), d.loc.cloc(), d.loc.sloc(), d.loc.dloc()),
            (4, 4, 8, 3)
        );
        assert_eq!(d.loc.dloc_without_code(), 3);
        assert_eq!((d.halstead.length(), d.halstead.vocabulary()), (12, 9));
        assert_eq!(d.cyclomatic.cyclomatic_sum(), 3);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        for m in [&d, &c] {
            assert!(close(m.mi.original(), 128.931_235_541_416_98));
        }
        assert!(close(d.mi.sei(), 160.866_726_074_267_3), "{}", d.mi.sei());
        assert!(close(c.mi.sei(), 173.387_837_747_781_2), "{}", c.mi.sei());
    }

    /// The mode moves `mi.*` and nothing else: an undocumented space, a
    /// space with only working comments, and every non-MI metric read the
    /// same under both; the default and an explicit `false` agree.
    #[cfg(all(feature = "python", feature = "rust"))]
    #[test]
    fn charging_docs_moves_only_documented_spaces_mi() {
        let cases: &[(crate::LANG, &[u8], bool)] = &[
            // (lang, source, carries documentation)
            (
                crate::LANG::Python,
                b"def f(n):\n    # a real comment\n    if n:\n        return 1\n    return 0\n",
                false,
            ),
            (
                crate::LANG::Python,
                b"def f(n):\n    \"\"\"Doc.\"\"\"\n    if n:\n        return 1\n    return 0\n",
                true,
            ),
            (
                crate::LANG::Rust,
                b"/// Doc.\nfn f(x: i32) -> i32 {\n    if x > 0 { x } else { 0 }\n}\n",
                true,
            ),
            (crate::LANG::Rust, b"fn f() {}\n", false),
            (crate::LANG::Python, b"", false),
        ];
        for (lang, source, documented) in cases {
            let excluded = metrics_verbatim(*lang, source, MetricsOptions::default());
            let explicit = metrics_verbatim(
                *lang,
                source,
                MetricsOptions::default().with_mi_charges_docs(false),
            );
            let charged = metrics_verbatim(
                *lang,
                source,
                MetricsOptions::default().with_mi_charges_docs(true),
            );
            assert_eq!(excluded, explicit, "{lang:?}: the default is false");
            assert_eq!((excluded.loc.dloc() > 0), *documented, "{lang:?}: fixture");
            let mi = |m: &CodeMetrics| (m.mi.original(), m.mi.sei(), m.mi.visual_studio());
            assert_eq!(
                mi(&excluded) != mi(&charged),
                *documented,
                "{lang:?}: only documentation rows move mi"
            );
            let mut without_mi = charged.clone();
            without_mi.mi = excluded.mi.clone();
            assert_eq!(without_mi, excluded, "{lang:?}: nothing but mi moves");
        }
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
    /// denominator. Subtracting the docstring from only one side would
    /// move `CM` and with it `sei`.
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
    /// is `sin(0)` and `sei` equals `original`.
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
            // Every comment row is docstring, so the comment share is 0.
            let sei = original;
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

    fn ratio_stats(comment_ratio: f64) -> Stats {
        Stats {
            halstead_length: 4.0,
            halstead_vocabulary: 3.0,
            halstead_volume: 4.0 * f64::log2(3.0),
            cyclomatic: 1.0,
            code_lines: 10.0,
            comment_ratio,
        }
    }

    /// The SEI comment term `50·sin(√(2.4·CM))` on the 0–1 ratio (#1607)
    /// rises strictly over the whole range and is bounded by its value at
    /// CM = 1. On the 0–100 percentage scale of #241 it oscillated through
    /// ±50, so a grid this coarse would not be monotonic.
    #[test]
    fn mi_sei_comment_term_is_monotonic_and_bounded() {
        // expected: 50·sin(√(2.4·CM)) evaluated per CM, e.g. CM = 0.5 gives
        // 50·sin(√1.2) = 50·sin(1.0954) ≈ 44.4566.
        const CEILING: f64 = 49.988_333_226_140_14;
        let mut previous = None;
        for (cm, expected) in [
            (0.0, 0.0),
            (0.1, 23.526_792_118_944_968),
            (0.3, 37.515_409_360_932_6),
            (0.5, 44.456_602_206_364_21),
            (0.7, 48.126_033_588_861_63),
            (1.0, CEILING),
        ] {
            let stats = ratio_stats(cm);
            let term = stats.sei() - stats.original();
            assert!((term - expected).abs() < 1e-9, "CM {cm}: term {term}");
            if let Some(p) = previous {
                assert!(term > p, "CM {cm}: {term} must exceed {p}");
            }
            previous = Some(term);
        }
    }

    /// With no comment the SEI term is `sin(0)`: `sei` is `original`,
    /// bit for bit, which also pins `sei`'s size and volume terms to
    /// `original`'s natural logarithms.
    #[test]
    fn mi_sei_equals_original_without_comments() {
        let stats = ratio_stats(0.0);
        assert_eq!(stats.sei(), stats.original());
        assert!(stats.original() > 0.0);
    }

    #[cfg(feature = "rust")]
    #[test]
    fn rust_mi_smoke() {
        // Rust now derives MI from the populated Loc / Cyclomatic /
        // Halstead trios via the default trait method. This test
        // pins the per-function MI on a tiny straight-line function
        // so accidental regressions in the cascade get caught.
        check_metrics::<RustParser>("fn f() -> i32 { 1 }\n", "foo.rs", |metric| {
            // expected: N = 7, n = 7, so V = 7·log2 7 ≈ 19.6515; G = 2
            // (unit + function); L = ploc = 1, so the size term is 0.
            // original = 171 − 5.2·ln V − 0.46 ≈ 155.0536, and
            // visual_studio = original·100/171 ≈ 90.6746.
            assert_comment_free_mi(
                &metric,
                (7, 7, 2, 1),
                155.053_605_022_880_67,
                90.674_622_820_398_06,
            );
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
                // expected: V = 9·log2 9 ≈ 28.5293, G = 2, L = 2:
                // 171 − 5.2·ln V − 0.46 − 16.2·ln 2 ≈ 141.8862.
                assert_comment_free_mi(
                    &metric,
                    (9, 9, 2, 2),
                    141.886_166_643_237_35,
                    82.974_366_458_033_53,
                );
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
                // expected: V = 13·log2 12 ≈ 46.6045, G = 3, L = 3:
                // 171 − 5.2·ln V − 0.69 − 16.2·ln 3 ≈ 132.5357.
                assert_comment_free_mi(
                    &metric,
                    (13, 12, 3, 3),
                    132.535_654_592_252_29,
                    77.506_230_755_703_09,
                );
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
                // expected: V = 17·log2 11 ≈ 58.8103, G = 3, L = 1:
                // 171 − 5.2·ln V − 0.69 ≈ 149.1235.
                assert_comment_free_mi(
                    &metric,
                    (17, 11, 3, 1),
                    149.123_548_232_679_84,
                    87.206_753_352_444_35,
                );
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
                // expected: V = 16·log2 11 ≈ 55.3509, G = 3, L = 1:
                // 171 − 5.2·ln V − 0.69 ≈ 149.4388.
                assert_comment_free_mi(
                    &metric,
                    (16, 11, 3, 1),
                    149.438_796_266_125_3,
                    87.391_108_927_558_66,
                );
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
                // expected: the JavaScript fixture's inputs and values.
                assert_comment_free_mi(
                    &metric,
                    (16, 11, 3, 1),
                    149.438_796_266_125_3,
                    87.391_108_927_558_66,
                );
            },
        );
    }

    /// `comment_ratio` feeds the SEI term `50·sin(√(2.4·CM))`, which turns
    /// back down past CM = 1. A degenerate `Loc` with `cloc > sloc` (which
    /// the loc.rs fix prevents for parsed input, but which the clamp
    /// defends regardless) must yield a ratio capped at exactly 1, not the
    /// raw 2 (issue #461). Reverting the `.clamp(0.0, 1.0)` in
    /// `Mi::compute` makes this fail.
    #[cfg(feature = "python")]
    #[test]
    fn mi_comment_ratio_clamped() {
        let loc = loc::Stats::with_cloc_sloc(2, 0);
        assert_eq!(loc.cloc(), 2);
        assert_eq!(loc.sloc(), 1);

        let cyclomatic = cyclomatic::Stats::default();
        let halstead = halstead::Stats::default();
        let mut mi = Stats::default();
        // The clamp is on the shared tail, so one mode exercises it; the
        // fixture has no `dloc`, so the two modes read the same inputs.
        PythonCode::compute(&loc, &cyclomatic, &halstead, &mut mi, false);

        assert_eq!(mi.comment_ratio, 1.0, "comment_ratio must clamp to 1");
    }

    /// A space with no rows has a zero denominator in both modes; the
    /// ratio is 0, never the `NaN` of `0 / 0`.
    #[cfg(feature = "python")]
    #[test]
    fn mi_comment_ratio_is_zero_without_rows() {
        let loc = loc::Stats::default();
        assert_eq!(loc.sloc(), 0);
        for charges_docs in [false, true] {
            let mut mi = Stats::default();
            PythonCode::compute(
                &loc,
                &cyclomatic::Stats::default(),
                &halstead::Stats::default(),
                &mut mi,
                charges_docs,
            );
            assert_eq!(mi.comment_ratio.to_bits(), 0.0_f64.to_bits());
        }
    }
}
