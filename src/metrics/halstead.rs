// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(
    clippy::doc_markdown,
    clippy::enum_glob_use,
    clippy::match_wildcard_for_single_variants,
    clippy::similar_names,
    clippy::unused_self,
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

use std::fmt;

use crate::checker::Checker;
use crate::getter::Getter;
use crate::int_hash::IntKeyHashMap;
use crate::macros::implement_metric_trait;

use crate::*;

/// The `Halstead` metric suite.
#[derive(Default, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Stats {
    u_operators: u64,
    operators: u64,
    u_operands: u64,
    operands: u64,
}

/// Specifies the type of nodes accepted by the `Halstead` metric.
pub enum HalsteadType {
    /// The node is an `Halstead` operator
    Operator,
    /// The node is an `Halstead` operand
    Operand,
    /// The node is unknown to the `Halstead` metric
    Unknown,
}

/// Per-space operator / operand occurrence maps used to compute the
/// Halstead `Stats` struct. One map per distinct operator (`kind_id`)
/// and one per distinct operand (`text`); merged across nested spaces.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HalsteadMaps<'a> {
    /// Keyed by `kind_id`, so it is hashed with [`crate::int_hash`]'s
    /// integer hasher rather than SipHash: the key is a grammar symbol
    /// this crate generated, drawn from an alphabet of at most a few
    /// hundred values, so there is nothing for a keyed hash to defend.
    pub(crate) operators: IntKeyHashMap<u16, u64>,
    /// Primitive-type operators stored by text so each distinct primitive
    /// (e.g. `int` vs `double`) counts as a separate distinct operator,
    /// even when the grammar maps them all to a single kind_id.
    ///
    /// Text-keyed, so it keeps SipHash — see the module doc on
    /// [`crate::int_hash`] for why analysed source text does not qualify
    /// for the fast hasher.
    pub(crate) primitive_operators: HashMap<&'a [u8], u64>,
    /// Text-keyed, and on SipHash for the same reason as
    /// `primitive_operators`.
    pub(crate) operands: HashMap<&'a [u8], u64>,
}

impl<'a> HalsteadMaps<'a> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn merge(&mut self, other: &HalsteadMaps<'a>) {
        for (k, v) in &other.operators {
            *self.operators.entry(*k).or_insert(0) += v;
        }
        for (k, v) in &other.primitive_operators {
            *self.primitive_operators.entry(*k).or_insert(0) += v;
        }
        for (k, v) in &other.operands {
            *self.operands.entry(*k).or_insert(0) += v;
        }
    }

    pub(crate) fn finalize(&self, stats: &mut Stats) {
        stats.u_operators = (self.operators.len() + self.primitive_operators.len()) as u64;
        stats.operators =
            self.operators.values().sum::<u64>() + self.primitive_operators.values().sum::<u64>();
        stats.u_operands = self.operands.len() as u64;
        stats.operands = self.operands.values().sum::<u64>();
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "unique_operators: {}, \
             total_operators: {}, \
             unique_operands: {}, \
             total_operands: {}, \
             length: {}, \
             estimated_program_length: {}, \
             purity_ratio: {}, \
             size: {}, \
             volume: {}, \
             difficulty: {}, \
             level: {}, \
             effort: {}, \
             time: {}, \
             bugs: {}",
            self.unique_operators(),
            self.total_operators(),
            self.unique_operands(),
            self.total_operands(),
            self.length(),
            self.estimated_program_length(),
            self.purity_ratio(),
            self.vocabulary(),
            self.volume(),
            self.difficulty(),
            self.level(),
            self.effort(),
            self.time(),
            self.bugs(),
        )
    }
}

impl Stats {
    // Intentionally a no-op. Halstead distinct-counts (`u_operators` /
    // `u_operands`) cannot be summed across sibling spaces without
    // double-counting operators/operands they share. Cross-space
    // aggregation is instead done by unioning the occurrence maps
    // (`HalsteadMaps::merge`) and re-running `finalize` on the parent
    // (see `spaces/compute.rs`). Summing the finalized fields here —
    // mirroring the sibling metrics' `merge` — would silently inflate
    // every parent space's n1/n2/N1/N2.
    pub(crate) fn merge(&mut self, _other: &Stats) {}

    /// Returns `η1`, the number of distinct operators
    #[inline]
    #[must_use]
    pub fn unique_operators(&self) -> u64 {
        self.u_operators
    }

    /// Returns `N1`, the number of total operators
    #[inline]
    #[must_use]
    pub fn total_operators(&self) -> u64 {
        self.operators
    }

    /// Returns `η2`, the number of distinct operands
    #[inline]
    #[must_use]
    pub fn unique_operands(&self) -> u64 {
        self.u_operands
    }

    /// Returns `N2`, the number of total operands
    #[inline]
    #[must_use]
    pub fn total_operands(&self) -> u64 {
        self.operands
    }

    /// Returns the program length
    ///
    /// Computed as `N = N1 + N2`, the sum of [`Self::total_operators`] and
    /// [`Self::total_operands`].
    #[inline]
    #[must_use]
    pub fn length(&self) -> u64 {
        self.total_operands() + self.total_operators()
    }

    /// Returns the calculated estimated program length
    ///
    /// Computed as `N^ = n1 * log2(n1) + n2 * log2(n2)`, where `n1` is
    /// [`Self::unique_operators`] and `n2` is [`Self::unique_operands`]. Each term is
    /// treated as `0` when its unique count is `0`.
    #[inline]
    #[must_use]
    pub fn estimated_program_length(&self) -> f64 {
        let uo = self.unique_operators() as f64;
        let ud = self.unique_operands() as f64;
        let uo_term = if uo == 0.0 { 0.0 } else { uo * uo.log2() };
        let ud_term = if ud == 0.0 { 0.0 } else { ud * ud.log2() };
        uo_term + ud_term
    }

    /// Returns the purity ratio
    ///
    /// Computed as `PR = N^ / N`, the ratio of
    /// [`Self::estimated_program_length`] to [`Self::length`].
    #[inline]
    #[must_use]
    pub fn purity_ratio(&self) -> f64 {
        let len = self.length() as f64;
        if len == 0.0 {
            0.0
        } else {
            self.estimated_program_length() / len
        }
    }

    /// Returns the program vocabulary
    ///
    /// Computed as `n = n1 + n2`, the sum of [`Self::unique_operators`] and
    /// [`Self::unique_operands`].
    #[inline]
    #[must_use]
    pub fn vocabulary(&self) -> u64 {
        self.unique_operands() + self.unique_operators()
    }

    /// Returns the program volume.
    ///
    /// Computed as `V = N * log2(n)`, where `N` is [`Self::length`] and `n`
    /// is [`Self::vocabulary`]. Returns `0` when the vocabulary is `<= 1`,
    /// since `log2` would be non-positive.
    ///
    /// Unit of measurement: bits
    #[inline]
    #[must_use]
    pub fn volume(&self) -> f64 {
        // Assumes a uniform binary encoding for the vocabulary is used.
        let vocab = self.vocabulary() as f64;
        if vocab <= 1.0 {
            0.0
        } else {
            self.length() as f64 * vocab.log2()
        }
    }

    /// Returns the estimated difficulty required to program
    ///
    /// Computed as `D = (n1 / 2) * (N2 / n2)`, where `n1` is
    /// [`Self::unique_operators`], `N2` is [`Self::total_operands`], and `n2` is
    /// [`Self::unique_operands`].
    #[inline]
    #[must_use]
    pub fn difficulty(&self) -> f64 {
        let ud = self.unique_operands() as f64;
        if ud == 0.0 {
            0.0
        } else {
            self.unique_operators() as f64 / 2. * self.total_operands() as f64 / ud
        }
    }

    /// Returns the estimated level of difficulty required to program
    ///
    /// Computed as `L = 1 / D`, the reciprocal of [`Self::difficulty`].
    #[inline]
    #[must_use]
    pub fn level(&self) -> f64 {
        let d = self.difficulty();
        if d == 0.0 { 0.0 } else { 1. / d }
    }

    /// Returns the estimated effort required to program
    ///
    /// Computed as `E = D * V`, the product of [`Self::difficulty`] and
    /// [`Self::volume`].
    #[inline]
    #[must_use]
    pub fn effort(&self) -> f64 {
        self.difficulty() * self.volume()
    }

    /// Returns the estimated time required to program.
    ///
    /// Computed as `T = E / 18`, where `E` is [`Self::effort`] and `18` is
    /// the Stroud number (see the divisor rationale below).
    ///
    /// Unit of measurement: seconds
    #[inline]
    #[must_use]
    pub fn time(&self) -> f64 {
        // The floating point `18.` aims to describe the processing rate of the
        // human brain. It is called Stoud number, S, and its
        // unit of measurement is moments/seconds.
        // A moment is the time required by the human brain to carry out the
        // most elementary decision.
        // 5 <= S <= 20. Halstead uses 18.
        // The value of S has been empirically developed from psychological
        // reasoning, and its recommended value for
        // programming applications is 18.
        //
        // Source: https://www.geeksforgeeks.org/software-engineering-halsteads-software-metrics/
        self.effort() / 18.
    }

    /// Returns the estimated number of delivered bugs.
    ///
    /// This metric represents the average amount of work a programmer can do
    /// without introducing an error.
    ///
    /// Computed as `B = E^(2/3) / 3000`, where `E` is [`Self::effort`]. This
    /// is the effort-based variant of Halstead's delivered-bugs estimate
    /// rather than the more commonly cited volume-based form `B = V / 3000`;
    /// it matches the formula used by upstream `rust-code-analysis`.
    #[inline]
    #[must_use]
    pub fn bugs(&self) -> f64 {
        // The floating point `3000.` represents the number of elementary
        // mental discriminations.
        // A mental discrimination, in psychology, is the ability to perceive
        // and respond to differences among stimuli.
        //
        // The value above is obtained starting from a constant that
        // is different for every language and assumes that natural language is
        // the language of the brain.
        // For programming languages, the English language constant
        // has been considered.
        //
        // After every 3000 mental discriminations a result is produced.
        // This result, whether correct or incorrect, is more than likely
        // either used as an input for the next operation or is output to the
        // environment.
        // If incorrect the error should become apparent.
        // Thus, an opportunity for error occurs every 3000
        // mental discriminations.
        //
        // Source: https://docs.lib.purdue.edu/cgi/viewcontent.cgi?article=1145&context=cstech
        self.effort().powf(2. / 3.) / 3000.
    }
}

#[doc(hidden)]
/// Per-language extraction of Halstead operator/operand maps.
pub(crate) trait Halstead
where
    Self: Checker + Getter,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    ///
    /// `ancestors` is the chain the walker descended through; it is
    /// handed to [`Getter::get_op_type`], six of whose impls classify a
    /// token by what encloses it (#1096).
    fn compute<'a>(
        node: &Node<'a>,
        code: &'a [u8],
        ancestors: Ancestors<'a, '_>,
        halstead_maps: &mut HalsteadMaps<'a>,
    );
}

#[inline]
fn get_id<'a>(node: &Node<'a>, code: &'a [u8]) -> &'a [u8] {
    &code[node.start_byte()..node.end_byte()]
}

#[inline]
fn compute_halstead<'a, T: Getter + Checker>(
    node: &Node<'a>,
    code: &'a [u8],
    ancestors: Ancestors<'a, '_>,
    halstead_maps: &mut HalsteadMaps<'a>,
) {
    match T::get_op_type_with_code(node, code, ancestors) {
        HalsteadType::Operator => {
            if T::is_primitive(node) {
                // Store primitive-type operators by text so distinct
                // primitives (e.g. `int` vs `double`) that share a
                // single kind_id are counted separately in n1/N1.
                *halstead_maps
                    .primitive_operators
                    .entry(get_id(node, code))
                    .or_insert(0) += 1;
            } else {
                *halstead_maps.operators.entry(node.kind_id()).or_insert(0) += 1;
            }
        }
        HalsteadType::Operand => {
            *halstead_maps
                .operands
                .entry(T::get_operand_id(node, code, ancestors))
                .or_insert(0) += 1;
        }
        _ => {}
    }
}

// Every language's `Halstead::compute` is the same forward to
// `compute_halstead`, which classifies each node through the language's
// own `Getter` / `Checker`. Nothing per-language lives here — it lives
// in `src/getter/<lang>.rs` — so writing the impls out was 23 copies of
// one signature. (This is the only metric whose per-language impls are
// all identical; every other trait has real per-language bodies.)
macro_rules! impl_halstead_forwarding {
    ($($code:ty),+ $(,)?) => {
        $(
            impl Halstead for $code {
                fn compute<'a>(
                    node: &Node<'a>,
                    code: &'a [u8],
                    ancestors: Ancestors<'a, '_>,
                    halstead_maps: &mut HalsteadMaps<'a>,
                ) {
                    compute_halstead::<Self>(node, code, ancestors, halstead_maps);
                }
            }
        )+
    };
}

impl_halstead_forwarding!(
    PythonCode,
    MozjsCode,
    JavascriptCode,
    TypescriptCode,
    TsxCode,
    RustCode,
    CppCode,
    CCode,
    ObjcCode,
    MozcppCode,
    JavaCode,
    GroovyCode,
    CsharpCode,
    GoCode,
    PerlCode,
    KotlinCode,
    LuaCode,
    PhpCode,
    RubyCode,
    ElixirCode,
    BashCode,
    TclCode,
    IrulesCode,
);

// Real defaults — no operators / operands to count. Audited in #188.
implement_metric_trait!(Halstead, PreprocCode, CcommentCode);

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
    use std::collections::HashSet;
    use std::path::PathBuf;

    use crate::test_support::{ast_has_kind_id, check_metrics_only_shim, for_each_node_with_chain};

    use super::*;

    check_metrics_only_shim!(check_metrics, Halstead);

    // Pins the lesson-4 invariant `n2 == len(dedupe(ops.operands))` by
    // running `operands_and_operators` (the text-keyed `--ops` store)
    // on the same source and comparing its deduplicated operand count
    // to the expected `n2`. The metrics store and the ops store are
    // independent (lesson 4); this catches a classification change that
    // moves one without the other.
    fn assert_ops_operands<T: crate::ParserTrait>(
        source: &str,
        file: &str,
        expected_n2: usize,
        mut expected_operands: Vec<&str>,
    ) {
        let path = PathBuf::from(file);
        let parser = T::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");

        let unique: HashSet<&str> = ops.operands.iter().map(String::as_str).collect();
        assert_eq!(
            unique.len(),
            expected_n2,
            "dedupe(ops.operands) must equal n2; operands were {:?}",
            ops.operands
        );

        let mut got: Vec<&str> = unique.into_iter().collect();
        got.sort_unstable();
        expected_operands.sort_unstable();
        assert_eq!(got, expected_operands);
    }

    /// Asserts the root space's `[n1, N1, n2, N2]`, naming `label` when
    /// it does not hold.
    ///
    /// The delimiter-invariance tests (#1256 Elixir, #1312 Ruby and
    /// Perl) each loop over spellings of one literal and need the
    /// spelling in the failure message; `check_metrics` expands to a
    /// plain `fn` that cannot capture a loop variable, so they reach
    /// for the closure-taking helper it wraps. Three copies of that
    /// dance is two too many.
    fn assert_halstead_counts<T: crate::ParserTrait>(
        source: &str,
        file: &str,
        expected: [u64; 4],
        label: &str,
    ) {
        crate::test_support::check_func_space_only::<T, _>(
            source,
            file,
            &[crate::Metric::Halstead],
            |space| {
                let halstead = &space.metrics.halstead;
                assert_eq!(
                    [
                        halstead.unique_operators(),
                        halstead.total_operators(),
                        halstead.unique_operands(),
                        halstead.total_operands(),
                    ],
                    expected,
                    "{label}"
                );
            },
        );
    }

    #[test]
    fn python_operators_and_operands() {
        check_metrics::<PythonParser>(
            "def foo():
                 def bar():
                     def toto():
                        a = 1 + 1
                     b = 2 + a
                 c = 3 + 3",
            "foo.py",
            |metric| {
                // unique operators: def, =, +
                // operators: def, def, def, =, =, =, +, +, +
                // unique operands: foo, bar, toto, a, b, c, 1, 2, 3
                // operands: foo, bar, toto, a, b, c, 1, 1, 2, a, 3, 3
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 3,
                  "total_operators": 9,
                  "unique_operands": 9,
                  "total_operands": 12,
                  "length": 21,
                  "estimated_program_length": 33.284212515144276,
                  "purity_ratio": 1.584962500721156,
                  "vocabulary": 12,
                  "volume": 75.28421251514428,
                  "difficulty": 2.0,
                  "level": 0.5,
                  "effort": 150.56842503028855,
                  "time": 8.364912501682698,
                  "bugs": 0.0094341190071077
                }
                "#
                );
            },
        );
    }

    /// Pointer-arithmetic operators: `*` (dereference), `&` (address-of),
    /// `->` (member-of-pointer), `+` (pointer + offset). Each is counted
    /// once in `n1`; multiple uses bump `N1`. The headline integer values
    /// (`u_operators`, `u_operands`) anchor the snapshot per the
    /// snapshot-anchor policy.
    #[test]
    fn c_pointer_arithmetic_operators() {
        check_metrics::<CParser>(
            "int g(int* p, int* q) {
                 return *(p + 1) + *q;
             }",
            "foo.c",
            |metric| {
                // Unique operators: int, *, (), {, }, +, ;, return  (= 8)
                //   `*` covers both pointer-type and dereference; the grammar
                //   does NOT split them.  `,` does not appear (only one
                //   parameter on each side of the body).
                // Unique operands: g, p, q, 1                       (= 4)
                assert_eq!(metric.halstead.unique_operators(), 8);
                assert_eq!(metric.halstead.unique_operands(), 4);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    /// Bitwise (`&`, `|`, `^`, `~`, `<<`, `>>`) and logical (`&&`, `||`,
    /// `!`) operators are distinct kind_ids and count as separate unique
    /// operators in Halstead.  `&` (bitwise-and) and `&&` (logical-and)
    /// must NOT collapse, even though both render as ampersands.
    #[test]
    fn c_bitwise_and_logical_operators() {
        check_metrics::<CParser>(
            "int f(int a, int b) {
                 int x = (a & b) | (a ^ b);
                 int y = ~a;
                 int z = (a << 1) >> 2;
                 return (a && b) || !x;
             }",
            "foo.c",
            |metric| {
                // Expect: 6 bitwise op kinds (& | ^ ~ << >>), 3 logical (&& || !).
                // Plus int, (), {, }, =, ;, return, , — 8 syntactic / arithmetic
                // operator kinds.  Six bitwise + three logical + eight = 17 unique
                // operators is the upper bound; actuals depend on grammar collapse,
                // so we assert a lower-bound and anchor via snapshot below.
                let s = &metric.halstead;
                assert!(
                    s.unique_operators() >= 14,
                    "expected >= 14 unique operators (bitwise + logical + syntax), got {}",
                    s.unique_operators(),
                );
                assert_eq!(s.unique_operands(), 8); // f, a, b, x, y, z, 1, 2
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    /// Increment / decrement (`++`, `--`) and `sizeof` / cast operators
    /// each contribute distinct unique operators.  C-style casts in the
    /// tree-sitter grammar surface as `cast_expression` with the type
    /// token classified as a primitive_type operator.
    #[test]
    fn c_increment_decrement_and_sizeof() {
        check_metrics::<CParser>(
            "void f(int* p) {
                 int n = sizeof(int);
                 ++p;
                 --n;
                 long w = (long) n;
             }",
            "foo.c",
            |metric| {
                // Unique operators include: void, int, long, *, =, sizeof, ++, --, (), {, }, ;
                // Unique operands: f, p, n, w
                let s = &metric.halstead;
                assert!(
                    s.unique_operators() >= 10,
                    "expected >= 10 unique operators including ++ / -- / sizeof / cast, got {}",
                    s.unique_operators(),
                );
                assert_eq!(s.unique_operands(), 4);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn cpp_operators_and_operands() {
        // Define operators and operands for C/C++ grammar according to this specification:
        // https://www.verifysoft.com/en_halstead_metrics.html
        // The only difference with the specification above is that
        // primitive types are treated as operators, since the definition of a
        // primitive type can be seen as the creation of a slot of a certain size.
        // i.e. The `int a;` definition creates a n-bytes slot.
        check_metrics::<CppParser>(
            "main()
            {
              int a, b, c, avg;
              scanf(\"%d %d %d\", &a, &b, &c);
              avg = (a + b + c) / 3;
              printf(\"avg = %d\", avg);
            }",
            "foo.c",
            |metric| {
                // unique operators: (), {}, int, &, =, +, /, ,, ;
                // unique operands: main, a, b, c, avg, scanf, "%d %d %d", 3, printf, "avg = %d"
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 9,
                  "total_operators": 24,
                  "unique_operands": 10,
                  "total_operands": 18,
                  "length": 42,
                  "estimated_program_length": 61.74860596185444,
                  "purity_ratio": 1.470204903853677,
                  "vocabulary": 19,
                  "volume": 178.41295556463058,
                  "difficulty": 8.1,
                  "level": 0.1234567901234568,
                  "effort": 1445.1449400735075,
                  "time": 80.28583000408375,
                  "bugs": 0.04260752914034329
                }
                "#
                );
            },
        );
    }

    /// A `sized_type_specifier` carries its `unsigned`/`signed`/`long`/
    /// `short` modifiers as bare keyword tokens (distinct kind_ids), not
    /// as `primitive_type` children. Prior to issue #466 those tokens
    /// fell through to the `Unknown` arm and were dropped from `n1`/`N1`,
    /// so `unsigned int` collapsed to just `int` and `signed long`
    /// contributed nothing. They must each count as a distinct operator,
    /// while `long long`'s two `long` tokens fold to one `n1` entry but
    /// two `N1` hits. Regression test for issue #466.
    #[test]
    fn cpp_sized_type_specifier_operators() {
        let source = "unsigned int u = 3; signed long b = 4; long long c = 5;";
        check_metrics::<CppParser>(source, "foo.cpp", |metric| {
            // Distinct operators (n1): unsigned, signed, long, int, =, ; = 6
            // Total operators (N1):
            //   unsigned(1) + int(1) + =(3) + ;(3) + signed(1) + long(3) = 12
            //   (`long` appears once in `signed long` and twice in `long long`)
            // Distinct/total operands: u, b, c, 3, 4, 5 = 6 / 6
            assert_eq!(metric.halstead.unique_operators(), 6);
            assert_eq!(metric.halstead.total_operators(), 12);
            assert_eq!(metric.halstead.unique_operands(), 6);
            assert_eq!(metric.halstead.total_operands(), 6);
        });

        // Pin the lesson-4 `n1 == dedupe(ops.operators)` invariant: the
        // kind_id-keyed metrics store and the text-keyed `--ops` store are
        // independent, so a modifier classified in one but not the other
        // would diverge here.
        let path = PathBuf::from("foo.cpp");
        let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
        let unique_operators: HashSet<&str> = ops.operators.iter().map(String::as_str).collect();
        assert_eq!(
            unique_operators.len(),
            6,
            "dedupe(ops.operators) must equal n1; operators were {:?}",
            ops.operators
        );
        for modifier in ["unsigned", "signed", "long"] {
            assert!(
                unique_operators.contains(modifier),
                "sized_type_specifier modifier {modifier:?} missing from ops.operators: {:?}",
                ops.operators
            );
        }
    }

    /// C++20 spaceship operator `<=>` (`Cpp::LTEQGT`) is a comparison
    /// operator and must be counted in Halstead, like its sibling
    /// comparison operators `<`, `>`, `<=`, `>=`, `==`, `!=`. Prior to
    /// this fix it fell through to the `Unknown` arm and was silently
    /// dropped from `n1` / `N1`, under-reporting volume / effort on any
    /// C++20+ codebase that defines `operator<=>`. Regression test for
    /// issue #197.
    #[test]
    fn cpp_spaceship_operator_is_halstead_operator() {
        check_metrics::<CppParser>(
            "int f(int a, int b) {
                 return (a <=> b) != 0;
             }",
            "foo.cpp",
            |metric| {
                // Unique operators (grammar collapses matched delimiters
                // to a single kind_id): int, (), {}, <=>, !=, return, ;, ,
                //   `<=>` is the regression target — without the fix it
                //   would be Unknown and `u_operators` would be 7.
                // Unique operands: f, a, b, 0
                let s = &metric.halstead;
                assert_eq!(s.unique_operators(), 8);
                assert_eq!(s.unique_operands(), 4);
                insta::assert_json_snapshot!(
                    s,
                    @r#"
                {
                  "unique_operators": 8,
                  "total_operators": 11,
                  "unique_operands": 4,
                  "total_operands": 6,
                  "length": 17,
                  "estimated_program_length": 32.0,
                  "purity_ratio": 1.8823529411764706,
                  "vocabulary": 12,
                  "volume": 60.94436251225965,
                  "difficulty": 6.0,
                  "level": 0.16666666666666666,
                  "effort": 365.6661750735579,
                  "time": 20.31478750408655,
                  "bugs": 0.01704519358507665
                }
                "#
                );
            },
        );
    }

    /// C++ compound subtract-assign `-=` (`Cpp::DASHEQ`) must be counted
    /// in Halstead like every other compound assignment (`+=`, `*=`,
    /// `/=`, etc.). Prior to the fix it fell through to the `Unknown`
    /// arm and was silently dropped from `n1` / `N1` — under-reporting
    /// volume / effort wherever C++ code subtracts in place. Regression
    /// test for issue #198.
    #[test]
    fn cpp_dash_eq_is_halstead_operator() {
        check_metrics::<CppParser>("void f(int a, int b) { a -= b; }", "foo.cpp", |metric| {
            // Unique operators: void, (), {}, int, ,, -=, ;
            //   `-=` is the regression target — without the fix it
            //   would be Unknown and `u_operators` would be 6.
            // Unique operands: f, a, b
            let s = &metric.halstead;
            assert_eq!(s.unique_operators(), 7);
            assert_eq!(s.unique_operands(), 3);
        });
    }

    /// C++ pointer-to-member access `.*` (`Cpp::DOTSTAR`) must be
    /// counted in Halstead. Prior to the fix it fell through to the
    /// `Unknown` arm and was silently dropped from `n1` / `N1`.
    /// Regression test for issue #198.
    ///
    /// The snippet uses an `operator.*` declaration because that is
    /// where the C++ tree-sitter grammar reliably emits a single
    /// `DOTSTAR` leaf; in expression position (`a.*b`) some grammar
    /// versions split the token into `DOT` + `STAR` and the regression
    /// would be masked.
    #[test]
    fn cpp_dot_star_is_halstead_operator() {
        check_metrics::<CppParser>("struct S { void operator.*(int); };", "foo.cpp", |metric| {
            // Unique operators with fix: {}, ;, (), int, void, .*
            //   `.*` is the regression target — without the fix it
            //   falls through to `Unknown` and `u_operators` is 5.
            // Unique operands: S
            let s = &metric.halstead;
            assert_eq!(s.unique_operators(), 6);
            assert_eq!(s.unique_operands(), 1);
        });
    }

    /// C++ pointer-to-member access through pointer `->*`
    /// (`Cpp::DASHGTSTAR`) must be counted in Halstead. Prior to the
    /// fix it fell through to the `Unknown` arm and was silently
    /// dropped from `n1` / `N1`. Regression test for issue #198.
    ///
    /// The snippet uses an `operator->*` declaration because that is
    /// where the C++ tree-sitter grammar reliably emits a single
    /// `DASHGTSTAR` leaf; in expression position (`a->*b`) the grammar
    /// splits the token into `DASHGT` + `STAR` and the regression would
    /// be masked.
    #[test]
    fn cpp_dash_gt_star_is_halstead_operator() {
        check_metrics::<CppParser>(
            "struct S { void operator->*(int); };",
            "foo.cpp",
            |metric| {
                // Unique operators with fix: {}, ;, (), int, void, ->*
                //   `->*` is the regression target — without the fix it
                //   falls through to `Unknown` and `u_operators` is 5.
                // Unique operands: S
                let s = &metric.halstead;
                assert_eq!(s.unique_operators(), 6);
                assert_eq!(s.unique_operands(), 1);
            },
        );
    }

    #[test]
    fn cpp_raw_string_delimiter_is_not_an_operator() {
        // Regression: issue #1314, the C++ sibling of Elixir #1256 and
        // Ruby/Perl #1312. A `raw_string_literal` carries its `R"(`
        // opener as a bare `LPAREN` child — the kind id a call uses —
        // so `auto a = R"(raw)";` reported a `()` operator with no call
        // in the source.
        //
        // The fixture holds both sides at once: two raw strings and one
        // real call. A guard widened past the literal would drop
        // `f(a)`'s parenthesis and fail here rather than silently
        // passing.
        //
        // The second literal uses the custom-delimiter form to pin that
        // shape too — it adds a `raw_string_delimiter` child but keeps
        // the same `(` — and its distinct text makes n2 differ from N2.
        //
        // expected: operators `;` × 3, `=` × 3, `int`, `()` × 1 →
        // n1 = 4, N1 = 8. Operands the two literals, `a` × 2, `b`, `c`,
        // `f` → n2 = 6, N2 = 7. Before the guard the two openers added
        // two more `()` → N1 = 10.
        check_metrics::<CppParser>(
            "auto a = R\"(raw)\";\nauto b = R\"tag(raw)tag\";\nint c = f(a);\n",
            "foo.cpp",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 4);
                assert_eq!(metric.halstead.total_operators(), 8);
                assert_eq!(metric.halstead.unique_operands(), 6);
                assert_eq!(metric.halstead.total_operands(), 7);
            },
        );
    }

    #[test]
    fn rust_operators_and_operands() {
        check_metrics::<RustParser>(
            "fn main() {
              let a = 5; let b = 5; let c = 5;
              let avg = (a + b + c) / 3;
              println!(\"{}\", avg);
            }",
            "foo.rs",
            |metric| {
                // unique operators: fn, (), {}, let, =, +, /, ;, !, ,
                // unique operands: main, a, b, c, avg, 5, 3, println, "{}"
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 10,
                  "total_operators": 23,
                  "unique_operands": 9,
                  "total_operands": 15,
                  "length": 38,
                  "estimated_program_length": 61.74860596185444,
                  "purity_ratio": 1.624963314785643,
                  "vocabulary": 19,
                  "volume": 161.42124551085624,
                  "difficulty": 8.333333333333334,
                  "level": 0.12,
                  "effort": 1345.177045923802,
                  "time": 74.7320581068779,
                  "bugs": 0.040619232256751396
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_aliased_primitive_type_classification() {
        // Regression for issue #95 (lesson #2): the Rust grammar emits 17
        // distinct `kind_id`s for `primitive_type` (one base plus 16
        // numeric-suffixed alias variants). `RustCode::is_primitive` in
        // `src/checker.rs` must list every variant; if a future regression
        // omits one, primitive type names emitted in that aliased position
        // silently drop into the kind_id-keyed operators bucket instead of
        // the text-keyed primitive_operators map, miscounting Halstead n1.
        //
        // The snippet exercises every primitive scalar type across many
        // syntactic positions (function parameter types, return types,
        // let-binding annotations, `as` casts, const items, type aliases,
        // struct fields, function pointer types, tuple types, array types,
        // reference types, generic type arguments). Empirically, ordinary
        // Rust source emits the base `Rust::PrimitiveType` variant from
        // all of these positions; the 16 suffixed alias variants are
        // produced by specific grammar productions not reachable from
        // user-written code. Mutation-verified: dropping
        // `Rust::PrimitiveType` from `is_primitive` fails this test
        // (u_operators 30→15). Dropping any single suffixed variant
        // currently leaves the test passing; if a future grammar bump
        // makes any suffixed variant reachable from idiomatic source,
        // extend the snippet so the test fires for that variant too.
        check_metrics::<RustParser>(
            "const C: u8 = 0;
            type T = i64;
            struct S { x: u32, y: u64 }
            fn g(p: fn(u8) -> u16) -> bool { let _ = p(0); true }
            fn f(a: u8, b: u16, c: u32, d: u64) -> u128 {
                let _x: i8 = 0;
                let _y: i16 = 0;
                let _z: i32 = 0;
                let _w: i64 = 0;
                let _v: i128 = 0;
                let _p: f32 = 1.0;
                let _q: f64 = 2.0;
                let _r: bool = true;
                let _s: char = 'x';
                let _t: usize = 0;
                let _u: isize = 0;
                let _arr: [u32; 4] = [0; 4];
                let _ref: &u8 = &0;
                let _tup: (u32, u64) = (0, 0);
                let _opt: Option<u32> = None;
                a as u128 + b as u128 + c as u128 + d
            }",
            "foo.rs",
            |metric| {
                // Headline: u_operators is the load-bearing assertion —
                // the 16 distinct primitive type names dedupe by text in
                // the primitive_operators map. Total operators (N1) and
                // operand counts pin the rest of the Halstead state.
                // Grew from 30 → 33 with the issue #394 fix: `const`,
                // `type`, and `struct` keywords are now classified as
                // operators (one occurrence each).
                assert_eq!(metric.halstead.unique_operators(), 33);
                assert_eq!(metric.halstead.total_operators(), 121);
                // u_operands / operands grew (was 31/50 before #390): the
                // fix now classifies TypeIdentifier (`T`, `S`, `Option`)
                // and FieldIdentifier (struct fields `x`, `y`) as operands
                // alongside the existing primitive type names.
                assert_eq!(metric.halstead.unique_operands(), 36);
                assert_eq!(metric.halstead.total_operands(), 55);
            },
        );
    }

    #[test]
    fn rust_field_identifier_is_operand() {
        // Regression for issue #390: prior to the fix, `FieldIdentifier`
        // (e.g. the `x` / `y` in `p.x`, `p.y`) fell through to
        // `HalsteadType::Unknown`, so the field names were not counted
        // as operands. Both C++ and Go already classify FieldIdentifier
        // as an operand. After the fix:
        //   unique operators: fn, (), {}, let, =, +, ;, .
        //   unique operands : main, p, Point, x, y, sum, 0, 1
        // Field names `x` and `y` each appear twice (`p.x + p.y` and
        // the struct literal `Point { x: 0, y: 1 }`).
        check_metrics::<RustParser>(
            "fn main() {
              let p = Point { x: 0, y: 1 };
              let sum = p.x + p.y;
            }",
            "foo.rs",
            |metric| {
                // Headline: pre-fix, FieldIdentifier (`x`, `y`) and
                // TypeIdentifier (`Point`) fell through to Unknown, so
                // u_operands was 5 (main, p, sum, 0, 1). After the
                // fix, +Point, +x, +y → 8 distinct names.
                assert_eq!(metric.halstead.unique_operands(), 8);
                assert_eq!(metric.halstead.total_operands(), 12);
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 9,
                  "total_operators": 14,
                  "unique_operands": 8,
                  "total_operands": 12,
                  "length": 26,
                  "estimated_program_length": 52.529325012980806,
                  "purity_ratio": 2.0203586543454155,
                  "vocabulary": 17,
                  "volume": 106.27403387250882,
                  "difficulty": 6.75,
                  "level": 0.14814814814814814,
                  "effort": 717.3497286394346,
                  "time": 39.85276270219081,
                  "bugs": 0.026711567292222575
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_type_identifier_is_operand() {
        // Regression for issue #390: `TypeIdentifier` (e.g. `Vec`,
        // `HashMap`, `String` when used as a path name) was dropped to
        // `HalsteadType::Unknown` for Rust. C++ and Go classify them as
        // operands. After the fix, u_operands = 8:
        //   main, v, m, Vec, HashMap, new, K, V
        // (`i32` is a primitive type, classified as an operator.)
        //
        // Also covers issue #394: `::` is now an operator. The snippet
        // has two `::` tokens (`Vec::new`, `HashMap::new`), so n1 grew
        // from 10 → 11 and N1 from 17 → 19.
        check_metrics::<RustParser>(
            "fn main() {
              let v: Vec<i32> = Vec::new();
              let m: HashMap<K, V> = HashMap::new();
            }",
            "foo.rs",
            |metric| {
                // Headline: u_operands includes `Vec`, `HashMap`, `K`,
                // `V` (and `i32` as a primitive operator). Without the
                // fix, Vec/HashMap/K/V silently dropped to Unknown.
                assert_eq!(metric.halstead.unique_operands(), 8);
                assert_eq!(metric.halstead.total_operands(), 11);
                // `::` appears twice (Vec::new, HashMap::new); without
                // the #394 fix u_operators was 10 and operators 17.
                assert_eq!(metric.halstead.unique_operators(), 11);
                assert_eq!(metric.halstead.total_operators(), 19);
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 11,
                  "total_operators": 19,
                  "unique_operands": 8,
                  "total_operands": 11,
                  "length": 30,
                  "estimated_program_length": 62.05374780501027,
                  "purity_ratio": 2.068458260167009,
                  "vocabulary": 19,
                  "volume": 127.43782540330756,
                  "difficulty": 7.5625,
                  "level": 0.1322314049586777,
                  "effort": 963.7485546125134,
                  "time": 53.54158636736186,
                  "bugs": 0.03252279825177962
                }
                "#
                );
            },
        );
    }

    #[test]
    fn rust_path_separator_is_operator() {
        // Regression for issue #394: `::` (`COLONCOLON`) was missing
        // from the Rust `get_op_type` operator arm even though C++,
        // Java, C#, and Kotlin all classify it as an operator. Path-
        // heavy code (`std::collections::HashMap`, `Vec::new`,
        // `T::method`) had every `::` silently dropped into
        // HalsteadType::Unknown.
        //
        // Snippet has three `::` tokens (`std::collections::HashMap`,
        // counted as two `::` separators, plus `HashMap::new`).
        check_metrics::<RustParser>(
            "fn main() {
              let m = std::collections::HashMap::new();
            }",
            "foo.rs",
            |metric| {
                // `::` appears 3 times across the two path expressions
                // (`std::collections::HashMap` contributes two; the
                // `HashMap::new` contributes one). Pre-fix all three
                // dropped to Unknown: u_operators would be 6 (no `::`
                // distinct) and total_operators() would be 7 (minus 3 `::`
                // occurrences). With the fix u_operators=7 and
                // operators=10.
                //
                // unique operators (post-fix): fn, LPAREN, LBRACE,
                // let, =, ::, ;. unique operands: main, m, std,
                // collections, HashMap, new.
                assert_eq!(metric.halstead.unique_operators(), 7);
                assert_eq!(metric.halstead.total_operators(), 10);
                assert_eq!(metric.halstead.unique_operands(), 6);
                assert_eq!(metric.halstead.total_operands(), 6);
            },
        );
    }

    #[test]
    fn rust_declaration_keywords_are_operators() {
        // Regression for issue #394: the Rust impl already accepted 17
        // keywords as operators (As, Async, Await, …, Fn) but omitted
        // 14 declaration / visibility keywords. The fix adds `Const`,
        // `Static`, `Enum`, `Struct`, `Trait`, `Impl`, `Use`, `Mod`,
        // `Pub`, `Type`, `Union`, `Where`, `Extern`, `Dyn`.
        //
        // Snippet exercises `use`, `pub`, `struct`, and `impl` (one of
        // each); together they account for 4 new operator occurrences
        // and 4 new unique operators.
        check_metrics::<RustParser>(
            "use std::fmt;
            pub struct S;
            impl S { fn n() -> u8 { 0 } }",
            "foo.rs",
            |metric| {
                // expected: unique operators (11) = use, ::, ;, pub,
                // struct, impl, LBRACE, fn, LPAREN, DASHGT, u8. Without
                // the #394 fix, `use`, `pub`, `struct`, and `impl`
                // would each drop to Unknown and u_operators would be
                // 7. unique operands (5): std, fmt, S, n, 0.
                assert_eq!(metric.halstead.unique_operators(), 11);
                assert_eq!(metric.halstead.total_operators(), 13);
                assert_eq!(metric.halstead.unique_operands(), 5);
                assert_eq!(metric.halstead.total_operands(), 6);
            },
        );
    }

    #[test]
    fn javascript_operators_and_operands() {
        check_metrics::<JavascriptParser>(
            "function main() {
              var a, b, c, avg;
              a = 5; b = 5; c = 5;
              avg = (a + b + c) / 3;
              console.log(\"{}\", avg);
            }",
            "foo.js",
            |metric| {
                // unique operators: function, (), {}, var, =, +, /, ,, ., ;
                // unique operands: main, a, b, c, avg, 3, 5, console, log, "{}"
                // `console.log` is the `.` operator applied to the two
                // identifier leaves; the composite `member_expression`
                // text is deliberately not a third operand (#1263), so
                // n2/N2 are 10/20 rather than the pre-#1263 11/21.
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 10,
                  "total_operators": 24,
                  "unique_operands": 10,
                  "total_operands": 20,
                  "length": 44,
                  "estimated_program_length": 66.43856189774725,
                  "purity_ratio": 1.5099673158578921,
                  "vocabulary": 20,
                  "volume": 190.16483617504394,
                  "difficulty": 10.0,
                  "level": 0.1,
                  "effort": 1901.6483617504396,
                  "time": 105.64713120835775,
                  "bugs": 0.05116412536051621
                }
                "#
                );
            },
        );
    }

    #[test]
    fn mozjs_operators_and_operands() {
        check_metrics::<MozjsParser>(
            "function main() {
              var a, b, c, avg;
              a = 5; b = 5; c = 5;
              avg = (a + b + c) / 3;
              console.log(\"{}\", avg);
            }",
            "foo.js",
            |metric| {
                // unique operators: function, (), {}, var, =, +, /, ,, ., ;
                // unique operands: main, a, b, c, avg, 3, 5, console, log, "{}"
                // `console.log` is the `.` operator applied to the two
                // identifier leaves; the composite `member_expression`
                // text is deliberately not a third operand (#1263), so
                // n2/N2 are 10/20 rather than the pre-#1263 11/21.
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 10,
                  "total_operators": 24,
                  "unique_operands": 10,
                  "total_operands": 20,
                  "length": 44,
                  "estimated_program_length": 66.43856189774725,
                  "purity_ratio": 1.5099673158578921,
                  "vocabulary": 20,
                  "volume": 190.16483617504394,
                  "difficulty": 10.0,
                  "level": 0.1,
                  "effort": 1901.6483617504396,
                  "time": 105.64713120835775,
                  "bugs": 0.05116412536051621
                }
                "#
                );
            },
        );
    }

    #[test]
    fn typescript_operators_and_operands() {
        check_metrics::<TypescriptParser>(
            "function main() {
              var a, b, c, avg;
              a = 5; b = 5; c = 5;
              avg = (a + b + c) / 3;
              console.log(\"{}\", avg);
            }",
            "foo.ts",
            |metric| {
                // unique operators: function, (), {}, var, =, +, /, ,, ., ;
                // unique operands: main, a, b, c, avg, 3, 5, console, log, "{}"
                // `console.log` is the `.` operator applied to the two
                // identifier leaves; the composite `member_expression`
                // text is deliberately not a third operand (#1263), so
                // n2/N2 are 10/20 rather than the pre-#1263 11/21.
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 10,
                  "total_operators": 24,
                  "unique_operands": 10,
                  "total_operands": 20,
                  "length": 44,
                  "estimated_program_length": 66.43856189774725,
                  "purity_ratio": 1.5099673158578921,
                  "vocabulary": 20,
                  "volume": 190.16483617504394,
                  "difficulty": 10.0,
                  "level": 0.1,
                  "effort": 1901.6483617504396,
                  "time": 105.64713120835775,
                  "bugs": 0.05116412536051621
                }
                "#
                );
            },
        );
    }

    #[test]
    fn tsx_operators_and_operands() {
        check_metrics::<TsxParser>(
            "function main() {
              var a, b, c, avg;
              a = 5; b = 5; c = 5;
              avg = (a + b + c) / 3;
              console.log(\"{}\", avg);
            }",
            "foo.ts",
            |metric| {
                // unique operators: function, (), {}, var, =, +, /, ,, ., ;
                // unique operands: main, a, b, c, avg, 3, 5, console, log, "{}"
                // `console.log` is the `.` operator applied to the two
                // identifier leaves; the composite `member_expression`
                // text is deliberately not a third operand (#1263), so
                // n2/N2 are 10/20 rather than the pre-#1263 11/21.
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 10,
                  "total_operators": 24,
                  "unique_operands": 10,
                  "total_operands": 20,
                  "length": 44,
                  "estimated_program_length": 66.43856189774725,
                  "purity_ratio": 1.5099673158578921,
                  "vocabulary": 20,
                  "volume": 190.16483617504394,
                  "difficulty": 10.0,
                  "level": 0.1,
                  "effort": 1901.6483617504396,
                  "time": 105.64713120835775,
                  "bugs": 0.05116412536051621
                }
                "#
                );
            },
        );
    }

    #[test]
    fn javascript_template_string_plain_is_operand() {
        // Regression: issue #192. A backtick-delimited `` `hello` ``
        // without `${...}` is semantically identical to `"hello"` /
        // `'hello'` and must contribute exactly one operand — before
        // the fix `TemplateString` fell through to `HalsteadType::Unknown`
        // and contributed zero. expected: operands are `f` (function
        // name) and the wrapping `` `hello` `` template literal →
        // u_operands = 2, N2 = 2 (matches the equivalent
        // `function f() { return "hello"; }` baseline).
        check_metrics::<JavascriptParser>("function f() { return `hello`; }", "foo.js", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });
    }

    /// Regression for #695. The `get` / `set` property-accessor keywords
    /// are operators, matching the C# getter's `Get | Set | Init | Add |
    /// Remove` accessor arm. Before #695 the JS family classified them as
    /// operands, so the same accessor keyword landed in opposite Halstead
    /// groups across languages. This pins them in the operator store and
    /// out of the operand store.
    #[test]
    fn js_get_set_accessors_are_operators() {
        let source = "class C { get x() { return 1; } set x(v) { this._x = v; } }";
        let path = PathBuf::from("foo.js");
        let parser = JavascriptParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
        assert!(
            ops.operators.iter().any(|o| o.as_str() == "get")
                && ops.operators.iter().any(|o| o.as_str() == "set"),
            "`get`/`set` accessors must be operators; operators were {:?}",
            ops.operators
        );
        assert!(
            !ops.operands.iter().any(|o| o.as_str() == "get")
                && !ops.operands.iter().any(|o| o.as_str() == "set"),
            "`get`/`set` accessors must not be operands; operands were {:?}",
            ops.operands
        );
    }

    #[test]
    fn javascript_template_string_interpolation_no_double_count() {
        // Regression: issue #192. An interpolated template literal
        // `` `Hi ${name}!` `` used to fall through to `Unknown`,
        // dropping the wrapper from the count entirely; the inner
        // `name` was still walked and counted via the
        // `TemplateSubstitution` child. Mirrors #183 (C#), #191
        // (Kotlin), #199 (Perl): the wrapper is skipped when a
        // `TemplateSubstitution` child is present so the inner
        // expression is not double-counted.
        //
        // expected: for `function f(name) { return ` + "`Hi ${name}!`"
        // + `; }`, operands are `f` and `name` (twice — `name` as the
        // parameter, then again inside the interpolation), so
        // u_operands = 2 and N2 = 3. Without the wrapper-skip guard
        // the wrapping literal would also be counted, lifting
        // u_operands to 3 and N2 to 4.
        check_metrics::<JavascriptParser>(
            "function f(name) { return `Hi ${name}!`; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 2);
                assert_eq!(metric.halstead.total_operands(), 3);
            },
        );
    }

    #[test]
    fn mozjs_template_string_plain_is_operand() {
        // Regression: issue #192. Mirrors
        // `javascript_template_string_plain_is_operand` for the
        // Firefox-mode dialect — the four JS-family `get_op_type`
        // impls share the same template-literal handling.
        check_metrics::<MozjsParser>("function f() { return `hello`; }", "foo.js", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });
    }

    #[test]
    fn mozjs_template_string_interpolation_no_double_count() {
        // Regression: issue #192. Mirrors
        // `javascript_template_string_interpolation_no_double_count`
        // for the Firefox-mode dialect.
        check_metrics::<MozjsParser>(
            "function f(name) { return `Hi ${name}!`; }",
            "foo.js",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 2);
                assert_eq!(metric.halstead.total_operands(), 3);
            },
        );
    }

    #[test]
    fn typescript_template_string_plain_is_operand() {
        // Regression: issue #192. Mirrors
        // `javascript_template_string_plain_is_operand` for
        // TypeScript — the four JS-family `get_op_type` impls share
        // the same template-literal handling.
        //
        // The `: string` annotation contributes no operand — its
        // keyword counts once, as the text-keyed operator (#1261) — so
        // the operands are `f` and `` `hello` `` (2 each). The headline
        // of this test — that the plain template literal contributes
        // one operand — is unaffected.
        check_metrics::<TypescriptParser>(
            "function f(): string { return `hello`; }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 2);
                assert_eq!(metric.halstead.total_operands(), 2);
            },
        );
    }

    #[test]
    fn typescript_template_string_interpolation_no_double_count() {
        // Regression: issue #192. Mirrors
        // `javascript_template_string_interpolation_no_double_count`
        // for TypeScript.
        //
        // The `: string` annotations contribute no operands (#1261).
        // Unique operands: `f`, `name` (2). Total operands: `f`, `name`
        // (param), `name` (in the interpolation) (3). The interpolation
        // guard from #192 still holds — the wrapping `` `Hi ${name}!` ``
        // is `Unknown`, not double-counted.
        check_metrics::<TypescriptParser>(
            "function f(name: string): string { return `Hi ${name}!`; }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 2);
                assert_eq!(metric.halstead.total_operands(), 3);
            },
        );
    }

    #[test]
    fn tsx_template_string_plain_is_operand() {
        // Regression: issue #192. Mirrors
        // `javascript_template_string_plain_is_operand` for the
        // TSX (TypeScript + JSX) variant.
        //
        // TSX's type-keyword `string` (`String3`) contributes no
        // operand, mirroring TS::String2 (#1261): operands are `f` and
        // `` `hello` `` (2 each).
        check_metrics::<TsxParser>(
            "function f(): string { return `hello`; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 2);
                assert_eq!(metric.halstead.total_operands(), 2);
            },
        );
    }

    #[test]
    fn tsx_template_string_interpolation_no_double_count() {
        // Regression: issue #192. Mirrors
        // `javascript_template_string_interpolation_no_double_count`
        // for the TSX (TypeScript + JSX) variant.
        //
        // The `: string` annotations contribute no `String3` operands
        // (#1261); see `typescript_template_string_…` for the count
        // derivation.
        check_metrics::<TsxParser>(
            "function f(name: string): string { return `Hi ${name}!`; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 2);
                assert_eq!(metric.halstead.total_operands(), 3);
            },
        );
    }

    /// The JS-family regex fixture, asserted against all four grammars.
    ///
    /// `impl_js_family_get_op_type!` is instantiated four times against
    /// four distinct `kind_id` enums (`SLASH` 87/81/90/87, `Regex`
    /// 224/250/264/225), so each expansion is a separate compiled arm
    /// and a drift in one grammar is invisible if only one is checked
    /// (grammar-dispatch section 11).
    fn assert_js_family_counts(source: &str, expected: [u64; 4]) {
        assert_halstead_counts::<JavascriptParser>(source, "foo.js", expected, "javascript");
        assert_halstead_counts::<MozjsParser>(source, "foo.jsm", expected, "mozjs");
        assert_halstead_counts::<TypescriptParser>(source, "foo.ts", expected, "typescript");
        assert_halstead_counts::<TsxParser>(source, "foo.tsx", expected, "tsx");
    }

    #[test]
    fn js_family_regex_delimiters_are_not_operators() {
        // Regression: issue #1314, the JS-family sibling of Elixir
        // #1256 and Ruby/Perl #1312. A `regex` literal spells both of
        // its delimiters `SLASH` — the kind id real division uses — so
        // `const a = /abc/g;` reported a `/` operator with no division
        // in the source, and n1/N1 counted the literal's punctuation as
        // arithmetic.
        //
        // The same fixture pins the second, independent half: `Regex`
        // was in neither arm, so the literal contributed no operand
        // either and reached the vocabulary from *neither* side.
        //
        // expected: operators `const`, `=`, `;`, `let` → n1 = 4;
        // `const` `=` `;` on line 1, `let` `=` `;` on line 2, `=` `;`
        // on line 3 → N1 = 8. Operands `a`, `/abc/g`, `b` → n2 = 3,
        // with `a` used three times and `b` twice → N2 = 6.
        //
        // Before the fix: n1 = 5 and N1 = 10 (the two fabricated `/`),
        // n2 = 2 and N2 = 5 (no operand for the literal).
        //
        // The four values are deliberately distinct so no transposition
        // of the unique-vs-total axes inside `assert_halstead_counts`
        // can pass (#1312).
        assert_js_family_counts("const a = /abc/g;\nlet b = a;\nb = a;\n", [4, 8, 3, 6]);
    }

    #[test]
    fn js_family_division_survives_the_regex_guard() {
        // Control for #1314: the guard is scoped to a `Regex` parent,
        // so real division must still count. This fixture holds both
        // sides at once — two divisions and one regex literal — so a
        // guard widened to every `SLASH` fails here rather than
        // silently passing the test above.
        //
        // expected: operators `const`, `=`, `;`, `/` → n1 = 4; two
        // `const`, two `=`, two `;` and two `/` → N1 = 8. Operands
        // `q`, `a`, `b`, `c`, `r`, `/x/` → n2 = N2 = 6.
        assert_js_family_counts("const q = a / b / c;\nconst r = /x/;\n", [4, 8, 6, 6]);
    }

    #[test]
    fn js_regex_delimiter_guard_is_parent_scoped_is_unobservable() {
        // Companion to the two above, and a statement of what they do
        // *not* cover. Ruby's guard has
        // `ruby_regex_guard_is_parent_scoped_not_ancestor_scoped`
        // because a division inside `#{…}` sits under a `Regex`
        // ancestor without being its child. No JS fixture can do that:
        // a regex literal admits no nested expression at all, its
        // `regex_pattern` and `regex_flags` children being leaves. So
        // the ancestor-scoped mutant of this guard — the one #1256's
        // post-mortem says survives every ordinary fixture — is
        // unobservable here. Measured, not assumed.
        //
        // Rather than write a fixture that would pass under both
        // spellings and read as coverage, pin the grammar property the
        // claim rests on: within a fixture that puts a division, a
        // template substitution and a regex in one file, every `SLASH`
        // reachable *below* a `Regex` is its immediate child. Should a
        // bump start nesting expressions inside a regex, this turns red
        // and the distinction becomes both observable and worth a real
        // test.
        //
        // Checked against all four grammars, not just JavaScript: the
        // guard is instantiated four times against four distinct enums,
        // and the property this test exists to watch could hold in one
        // and lapse in another.
        let source = b"const a = /abc/g;\nconst q = x / y;\nconst t = `p ${x / y} ${/zz/} q`;\n";
        assert_regex_slashes_are_immediate_children::<crate::langs::JavascriptCode>(
            source,
            Javascript::SLASH as u16,
            Javascript::Regex as u16,
            "javascript",
        );
        assert_regex_slashes_are_immediate_children::<crate::langs::MozjsCode>(
            source,
            Mozjs::SLASH as u16,
            Mozjs::Regex as u16,
            "mozjs",
        );
        assert_regex_slashes_are_immediate_children::<crate::langs::TypescriptCode>(
            source,
            Typescript::SLASH as u16,
            Typescript::Regex as u16,
            "typescript",
        );
        assert_regex_slashes_are_immediate_children::<crate::langs::TsxCode>(
            source,
            Tsx::SLASH as u16,
            Tsx::Regex as u16,
            "tsx",
        );
    }

    /// Asserts every `slash` token below a `regex` node in `source` is
    /// that node's *immediate* child, for one grammar.
    ///
    /// Backs `js_regex_delimiter_guard_is_parent_scoped_is_unobservable`
    /// — see there for why the property is worth pinning.
    fn assert_regex_slashes_are_immediate_children<L: crate::traits::LanguageInfo>(
        source: &[u8],
        slash: u16,
        regex: u16,
        label: &str,
    ) {
        let mut slashes_below_a_regex = 0;
        let visited = for_each_node_with_chain::<L>(source, |node: &Node<'_>, chain| {
            if node.kind_id() != slash {
                return;
            }
            let Some(depth) = chain.iter().position(|a| a.kind_id() == regex) else {
                return;
            };
            slashes_below_a_regex += 1;
            assert_eq!(
                depth,
                chain.len() - 1,
                "{label}: a slash at row {} has a regex ancestor that is not its parent, so \
                 the parent-vs-ancestor mutant is now observable and needs a real test",
                node.start_row()
            );
        });
        assert!(visited > 20, "{label}: fixture is too small to prove much");
        // Without this the assertion above is vacuous whenever the
        // fixture stops containing a regex at all — the failure mode a
        // filter that matches nothing always has.
        assert_eq!(
            slashes_below_a_regex, 4,
            "{label}: expected the two regex literals' four delimiters; the fixture no \
             longer exercises what this test claims"
        );
    }

    // Issue #281: optional chaining (`?.`) was double-counted as a
    // Halstead operator in TypeScript and TSX because the grammar
    // exposes both an `optional_chain` named wrapper AND a child
    // `?.` token, and both were classified as `Operator`. The fix
    // counts only the bare `?.` token (`QMARKDOT`) in TS/TSX so each
    // textual `?.` contributes exactly once, matching JS / MozJS
    // (whose grammars expose only `OptionalChain` — the `?.` token
    // itself).
    //
    // The four assertions below all compare against the same totals:
    // for `function f(a) { return a?.b?.c; }` the operator stream is
    // `function`, `(`, `{`, `return`, `?.`, `?.`, `;` (7 total, 6
    // unique — `LPAREN`/`LBRACE` count once, closing tokens are not
    // in the operator set). Before the fix, TS/TSX reported 9/7
    // instead of 7/6.
    #[test]
    fn javascript_optional_chain_not_double_counted_in_halstead_281() {
        check_metrics::<JavascriptParser>("function f(a) { return a?.b?.c; }", "foo.js", |m| {
            assert_eq!(m.halstead.unique_operators(), 6);
            assert_eq!(m.halstead.total_operators(), 7);
        });
    }

    #[test]
    fn mozjs_optional_chain_not_double_counted_in_halstead_281() {
        check_metrics::<MozjsParser>("function f(a) { return a?.b?.c; }", "foo.js", |m| {
            assert_eq!(m.halstead.unique_operators(), 6);
            assert_eq!(m.halstead.total_operators(), 7);
        });
    }

    #[test]
    fn typescript_optional_chain_not_double_counted_in_halstead_281() {
        // The TS grammar wraps member-expression `?.` in an
        // `optional_chain` named node containing the bare `?.`
        // token; classifying both as `Operator` double-counted the
        // chain. We now count only the bare token, so TS matches JS.
        check_metrics::<TypescriptParser>("function f(a) { return a?.b?.c; }", "foo.ts", |m| {
            assert_eq!(m.halstead.unique_operators(), 6);
            assert_eq!(m.halstead.total_operators(), 7);
        });
    }

    #[test]
    fn tsx_optional_chain_not_double_counted_in_halstead_281() {
        check_metrics::<TsxParser>("function f(a) { return a?.b?.c; }", "foo.tsx", |m| {
            assert_eq!(m.halstead.unique_operators(), 6);
            assert_eq!(m.halstead.total_operators(), 7);
        });
    }

    // Issue #299: parity guard for the JS-family `get_op_type` macro
    // on the optional-chain operator token (#281's prior regression
    // surface). All four languages must classify the bare `?.` token
    // identically — `OptionalChain` in JS/MozJS, `QMARKDOT` in
    // TS/TSX — and emit the same totals for
    // `function f(a) { return a?.b?.c; }`:
    //
    // * Operators: `function`, `(`, `{`, `return`, `?.`, `?.`, `;`
    //   (7 total, 6 unique).
    // * Operands: `f`, `a` (parameter), `a`, `b`, `c` — the identifier
    //   and property leaves only (5 total, 4 unique). Until #1263 the
    //   two wrapping member expressions (`a?.b`, `a?.b?.c`) were
    //   classified as `MemberExpression*` operands on top of the leaves
    //   they contain, making this 7 total / 6 unique.
    //
    // Verified by test-via-revert: dropping `OptionalChain` from
    // JS/MozJS, or `QMARKDOT` from TS/TSX, trips the test
    // (u_operators 6→5). This input does NOT exercise every operand
    // alias in the per-language `operand_extras` (`Identifier2`, the
    // JS/MozJS/TSX string-literal `String2`); drift in
    // those is out of scope for this regression guard and would need a
    // separate fixture. The `PredefinedType` operator path (`: void`
    // double-count) is now covered by `ts_void_return_type_single_operator_453`
    // below.
    #[test]
    fn js_family_get_op_type_parity_optional_chain_member_299() {
        // Non-capturing closure (coerced to the `fn` pointer that
        // `check_metrics` accepts) avoids the
        // `clippy::needless_pass_by_value` warning that a free `fn`
        // taking `CodeMetrics` by value would trigger.
        const SRC: &str = "function f(a) { return a?.b?.c; }";
        let check = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.unique_operators(), 6);
            assert_eq!(m.halstead.total_operators(), 7);
            assert_eq!(m.halstead.unique_operands(), 4);
            assert_eq!(m.halstead.total_operands(), 5);
        };

        check_metrics::<JavascriptParser>(SRC, "foo.js", check);
        check_metrics::<MozjsParser>(SRC, "foo.js", check);
        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
    }

    // Issue #1263: a member access contributes its leaves and the `.`
    // operator, never the `member_expression` composite as well. The
    // classification does not stop the walk, so `a` and `b` were always
    // counted; listing the wrapper billed a third operand keyed on the
    // whole `a.b` text, which no other language here does.
    //
    // expected, for `var r = a.b;`:
    //
    // * Operators: `var`, `=`, `.`, `;` — 4 total, 4 unique.
    // * Operands: `r`, `a`, `b` — 3 total, 3 unique. Before the fix
    //   the `member_expression` wrapper added `a.b`, making both 4.
    //
    // All four JS-family languages are asserted because
    // `impl_js_family_get_op_type!` emits one shared operand arm: the
    // lockstep is the point of the macro, and a per-language extras
    // list is exactly where a future edit could break it.
    #[test]
    fn js_family_member_access_counts_leaves_not_the_composite_1263() {
        const SRC: &str = "var r = a.b;";
        let check = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.unique_operators(), 4);
            assert_eq!(m.halstead.total_operators(), 4);
            assert_eq!(m.halstead.unique_operands(), 3);
            assert_eq!(m.halstead.total_operands(), 3);
        };

        check_metrics::<JavascriptParser>(SRC, "foo.js", check);
        check_metrics::<MozjsParser>(SRC, "foo.js", check);
        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
    }

    // Issue #1263, the grammar-dispatch section 6 half: dropping
    // `MemberExpression*` from the operand arm would have regressed
    // private-field access to *zero* operands for the field, because
    // `PrivatePropertyIdentifier` — the `#x` leaf — was in no operand
    // list and the composite `this.#x` had been its only count. Adding
    // the leaf also fixes the declaration site `#x = 1`, which no
    // wrapper covered and which therefore counted nothing at all.
    //
    // expected, for `class C { #x = 1; m() { return this.#x; } }`:
    //
    // * Operators: `{`×2 (class body, method body), `=`, `;`×2, `(`,
    //   `return`, `.` — 8 total, 6 unique. (`class` is not in the
    //   JS-family operator arm, so it contributes nothing; that is
    //   pre-existing and unrelated.)
    // * Operands: `C`, `#x`, `1`, `m`, `this`, `#x` — 6 total, 5
    //   unique under JS/MozJS. Under TS/TSX the class *name* `C`
    //   parses as `type_identifier`, which those getters do not
    //   classify, so both counts drop by one to 5/4 — a pre-existing
    //   divergence this fixture records rather than fixes.
    #[test]
    fn js_family_private_field_leaf_is_the_operand_1263() {
        const SRC: &str = "class C { #x = 1; m() { return this.#x; } }";
        let check_js = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.unique_operators(), 6);
            assert_eq!(m.halstead.total_operators(), 8);
            assert_eq!(m.halstead.unique_operands(), 5);
            assert_eq!(m.halstead.total_operands(), 6);
        };
        let check_ts = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.unique_operators(), 6);
            assert_eq!(m.halstead.total_operators(), 8);
            assert_eq!(m.halstead.unique_operands(), 4);
            assert_eq!(m.halstead.total_operands(), 5);
        };

        check_metrics::<JavascriptParser>(SRC, "foo.js", check_js);
        check_metrics::<MozjsParser>(SRC, "foo.js", check_js);
        check_metrics::<TypescriptParser>(SRC, "foo.ts", check_ts);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check_ts);
    }

    // Issue #1263, the other section 6 half: `meta_property` is the one
    // composite the leaves-not-composites drop has to keep. `import.meta`
    // / `new.target` have no classified leaf — `meta` and `target` are
    // anonymous tokens in no arm — so with `MemberExpression*` gone the
    // meta-object contributed no operand at all while `this.env.x` still
    // yielded three.
    //
    // expected operands, for `var t = import.meta.url; function f() {
    // return new.target; }`: `t`, `import.meta`, `url`, `f`,
    // `new.target` — 5 total, 5 unique. Operators are deliberately not
    // asserted: the `import` / `new` keyword tokens inside the
    // meta-property keep their pre-existing operator classification,
    // which this fixture neither pins nor contests.
    #[test]
    fn js_family_meta_property_is_one_operand_1263() {
        const SRC: &str = "var t = import.meta.url; function f() { return new.target; }";
        let check = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.unique_operands(), 5);
            assert_eq!(m.halstead.total_operands(), 5);
        };

        check_metrics::<JavascriptParser>(SRC, "foo.js", check);
        check_metrics::<MozjsParser>(SRC, "foo.js", check);
        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
    }

    // Issue #1263: TS/TSX `nested_identifier` (`namespace N.M`) is the
    // same container/leaf double-count as `member_expression`.
    //
    // expected, for `namespace N.M { }`:
    //
    // * Operators: `.`, `{` — 2 total, 2 unique. (`namespace` is not in
    //   the JS-family operator arm.)
    // * Operands: `N`, `M` — 2 total, 2 unique. Before the fix the
    //   `nested_identifier` added `N.M`, making both 3.
    #[test]
    fn ts_nested_identifier_counts_leaves_not_the_composite_1263() {
        const SRC: &str = "namespace N.M { }";
        let check = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.unique_operators(), 2);
            assert_eq!(m.halstead.total_operators(), 2);
            assert_eq!(m.halstead.unique_operands(), 2);
            assert_eq!(m.halstead.total_operands(), 2);
        };

        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
    }

    // Issue #1261 (inverting the #313 pin): the `"string"` type-keyword
    // aliases the TS / TSX grammars expose must contribute NO operand.
    // #313 put them in `operand_extras` for parity with the then-wider
    // `Checker::is_string`, but the `predefined_type` wrapper already
    // counts as the text-keyed `"string"` operator, so one `: string`
    // token tallied as operator AND operand while `: number` counted
    // once. #1261 drops the aliases from both `operand_extras` and
    // `is_string`, so the keyword counts once, as the operator.
    //
    // For the input `let x: string = "y";`:
    //
    // * TypeScript emits `Typescript::String2` for the `string` type
    //   keyword (kind_id 135, in the type-keyword block of the enum).
    // * TSX emits `Tsx::String3` for the same role (kind_id 141).
    //
    // Verified by test-via-revert: restoring `String2` to TS's
    // `operand_extras` (or `String3` to TSX's) trips this test on
    // `u_operands` / `operands` for the affected language.
    #[test]
    fn ts_family_type_keyword_counts_once_1261() {
        const SRC: &str = "let x: string = \"y\";";
        // Operators (n1 = 5, N1 = 5):
        //   `let`, `:`, `=`, `;`, plus `string` (PredefinedType wrapper,
        //   routed through `is_primitive` so it's keyed by its lexeme
        //   `"string"` in `primitive_operators`).
        // Operands (n2 = 2, N2 = 2):
        //   `x` and the `"y"` literal. Under #313 the type-keyword
        //   child of `predefined_type` added a third, phantom
        //   `"string"` operand (n2 = 3 / N2 = 3).
        let check = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.unique_operators(), 5);
            assert_eq!(m.halstead.total_operators(), 5);
            assert_eq!(m.halstead.unique_operands(), 2);
            assert_eq!(m.halstead.total_operands(), 2);
        };

        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
    }

    // Issue #1261 regression, the issue's reproducer plus a literal
    // whose contents spell the keyword: `: string` and `: number` must
    // contribute symmetrically — one text-keyed operator each, zero
    // operands — while a string *literal* `"string"` stays an operand
    // (distinct from the keyword: TS kind `String`, TSX kind `String2`,
    // both quoted in the operand key).
    #[test]
    fn ts_family_string_annotation_symmetric_with_number_1261() {
        const SRC: &str = "let x: string = \"a\";\nlet y: number = 1;\nlet s = \"string\";";
        // Operators (n1 = 6, N1 = 13):
        //   `let` ×3, `:` ×2, `=` ×3, `;` ×3, `string` ×1, `number` ×1.
        //   Pre-fix N1 was identical — the wrapper operator was always
        //   counted; the defect was the extra operand below.
        // Operands (n2 = 6, N2 = 6):
        //   `x`, `"a"`, `y`, `1`, `s`, `"string"` — one each. Pre-fix
        //   the `: string` keyword added a bare `string` operand
        //   (n2 = 7 / N2 = 7) that `: number` had no analogue of.
        let check = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.unique_operators(), 6);
            assert_eq!(m.halstead.total_operators(), 13);
            assert_eq!(m.halstead.unique_operands(), 6);
            assert_eq!(m.halstead.total_operands(), 6);
        };

        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
    }

    /// Drift marker for #1261 (lesson 34 / grammar-dispatch §2): the
    /// anonymous `string` type-keyword token appears **only** as a
    /// `predefined_type` child.
    ///
    /// That is the whole argument for classifying the keyword without a
    /// parent guard — the wrapper is guaranteed to be there to carry the
    /// operator. The two tests above measure the consequence and would
    /// still pass if the grammar started emitting the keyword somewhere
    /// else, as long as their own two fixtures kept their counts; this one
    /// measures the premise, over every position #1261's dump probes
    /// covered: annotation, parameter and return type, union member,
    /// generic argument, and template-literal type. A string *literal*
    /// spelling `"string"` is a different kind and must not be confused
    /// for the keyword, so one is in the fixture too.
    #[test]
    fn ts_family_type_keyword_only_appears_under_predefined_type_1261() {
        // Exercises each position the keyword can take. Valid in both
        // grammars: no angle-bracket cast, which TSX would read as JSX.
        const SRC: &str = "const a: string = \"string\";\n\
                           function f(x: string): string {\n\
                               return x;\n\
                           }\n\
                           type U = string | number;\n\
                           type A = Array<string>;\n\
                           type M = Map<string, number>;\n\
                           type T = `id-${string}`;\n";
        // Seven type positions: `a`, `x`, `f`'s return, the union member,
        // `Array`'s argument, `Map`'s first argument, and the template
        // placeholder. The `"string"` initialiser is a literal, not the
        // keyword, and must not be among them.
        const EXPECTED_OCCURRENCES: usize = 7;

        fn keyword_occurrences<P: ParserTrait>(
            path: &str,
            keyword: u16,
            predefined_type: u16,
        ) -> usize {
            let parser = P::new(SRC.as_bytes().to_vec(), &PathBuf::from(path), None);
            parser
                .root()
                .preorder()
                .filter(|node| node.kind_id() == keyword)
                .inspect(|node| {
                    assert_eq!(
                        node.parent().map(|parent| parent.kind_id()),
                        Some(predefined_type),
                        "the `string` type keyword surfaced outside \
                         `predefined_type` in {path}; the operator is carried \
                         by the wrapper, so `get_op_type` needs a parent guard \
                         before that arm can be trusted (#1261)",
                    );
                })
                .count()
        }

        // Kind ids re-read from the generated enums, not carried over: TS
        // `String2` = 135, TSX `String3` = 141 (TSX's `String2` = 261 is the
        // string-literal production and stays an operand).
        assert_eq!(
            keyword_occurrences::<TypescriptParser>(
                "foo.ts",
                Typescript::String2 as u16,
                Typescript::PredefinedType as u16,
            ),
            EXPECTED_OCCURRENCES,
            "TypeScript no longer emits the `string` type keyword in every \
             position #1261 probed",
        );
        assert_eq!(
            keyword_occurrences::<TsxParser>(
                "foo.tsx",
                Tsx::String3 as u16,
                Tsx::PredefinedType as u16,
            ),
            EXPECTED_OCCURRENCES,
            "TSX no longer emits the `string` type keyword in every position \
             #1261 probed",
        );
    }

    // Issue #453: a `void` return type must contribute exactly one
    // Halstead operator. The TS / TSX grammars parse `: void` as a
    // `predefined_type` wrapper around an inner `void` token. `is_primitive`
    // routes the wrapper into the text-keyed `primitive_operators` map as
    // `"void"`, while the inner `Void` token is independently a standalone
    // expression operator (`void 0`). Pre-fix both classified as operators
    // and one source `void` counted as TWO distinct Halstead operators.
    // The fix suppresses the wrapper when its child is a `Void` token, so
    // only the inner token carries the operator — matching expression
    // `void 0` and keeping the kind_id-keyed count consistent.
    //
    // For `function f(): void { return; }`:
    //
    // * Operators (n1 = 7, N1 = 7): `function`, `()`, `{}`, `:`, `return`,
    //   `;`, and a single `void`. (The untyped form is n1 = 5; the `: void`
    //   annotation adds the `:` operator and one `void`, NOT two — the
    //   issue's "n1 = 6" target overlooked the annotation colon.)
    //
    // Verified by test-via-revert: removing the `predefined_void` guard
    // restores the pre-fix `u_operators` 7 -> 8 with a duplicate `"void"`
    // (one kind_id-keyed, one in `primitive_operators`). Both `metrics()`
    // and the `ops`-list dedup invariant (`ts_void_return_and_expression_*`
    // in `ops.rs`) are pinned per lesson 4.
    #[test]
    fn ts_void_return_type_single_operator_453() {
        const SRC: &str = "function f(): void { return; }";
        let check = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.unique_operators(), 7);
            assert_eq!(m.halstead.total_operators(), 7);
        };

        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
    }

    // Issue #453 over-suppression guard: expression `void 0` (a
    // `unary_expression`, NOT a `predefined_type` wrapper) must still
    // count `void` as exactly one operator. The fix keys only on a
    // `predefined_type` whose child is a `Void` token, so the bare
    // expression operator is untouched.
    //
    // For `const x = void 0;`:
    //
    // * Operators (n1 = 4, N1 = 4): `const`, `=`, `void`, `;`.
    // * Operands (n2 = 2, N2 = 2): `x`, `0`.
    #[test]
    fn ts_void_expression_still_single_operator_453() {
        const SRC: &str = "const x = void 0;";
        let check = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.unique_operators(), 4);
            assert_eq!(m.halstead.total_operators(), 4);
            assert_eq!(m.halstead.unique_operands(), 2);
            assert_eq!(m.halstead.total_operands(), 2);
        };

        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
    }

    #[test]
    fn python_wrong_operators() {
        check_metrics::<PythonParser>("()[]{}", "foo.py", |metric| {
            insta::assert_json_snapshot!(
                metric.halstead,
                @r#"
            {
              "unique_operators": 0,
              "total_operators": 0,
              "unique_operands": 0,
              "total_operands": 0,
              "length": 0,
              "estimated_program_length": 0.0,
              "purity_ratio": 0.0,
              "vocabulary": 0,
              "volume": 0.0,
              "difficulty": 0.0,
              "level": 0.0,
              "effort": 0.0,
              "time": 0.0,
              "bugs": 0.0
            }
            "#
            );
        });
    }

    #[test]
    fn python_check_metrics() {
        check_metrics::<PythonParser>(
            "def f():
                 pass",
            "foo.py",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 2,
                  "total_operators": 2,
                  "unique_operands": 1,
                  "total_operands": 1,
                  "length": 3,
                  "estimated_program_length": 2.0,
                  "purity_ratio": 0.6666666666666666,
                  "vocabulary": 3,
                  "volume": 4.754887502163468,
                  "difficulty": 1.0,
                  "level": 1.0,
                  "effort": 4.754887502163468,
                  "time": 0.26416041678685936,
                  "bugs": 0.0009425525573729414
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_operators_and_operands() {
        check_metrics::<JavaParser>(
            "public class Main {
            public static void main(string args[]) {
                  int a, b, c, avg;
                  a = 5; b = 5; c = 5;
                  avg = (a + b + c) / 3;
                  MessageFormat.format(\"{0}\", avg);
                }
            }",
            "foo.java",
            |metric| {
                // Operators (n1=11): {} void () [] , . ; int = + /
                // Operands (n2=12): Main main args a b c avg 5 3 MessageFormat format "{0}"
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 11,
                  "total_operators": 26,
                  "unique_operands": 12,
                  "total_operands": 22,
                  "length": 48,
                  "estimated_program_length": 81.07329781366414,
                  "purity_ratio": 1.6890270377846697,
                  "vocabulary": 23,
                  "volume": 217.13097389073664,
                  "difficulty": 10.083333333333334,
                  "level": 0.09917355371900825,
                  "effort": 2189.4039867315946,
                  "time": 121.63355481842193,
                  "bugs": 0.05620341201461669
                }
                "#
                );
            },
        );
    }

    #[test]
    fn java_primitive_types_and_booleans() {
        check_metrics::<JavaParser>(
            "public class Prims {
                byte a = 1;
                short b = 2;
                int c = 3;
                long d = 4;
                char e = 'x';
                float f = 1.0f;
                double g = 2.0;
                boolean h = true;
                boolean i = false;
            }",
            "foo.java",
            |metric| {
                // Verifies all 8 Java primitive-type keywords (byte, short, int, long,
                // char, float, double, boolean) are counted as distinct operators, and
                // that true/false are counted as operands.
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 11,
                  "total_operators": 28,
                  "unique_operands": 19,
                  "total_operands": 19,
                  "length": 47,
                  "estimated_program_length": 118.76437056043838,
                  "purity_ratio": 2.526901501285923,
                  "vocabulary": 30,
                  "volume": 230.62385799360038,
                  "difficulty": 5.5,
                  "level": 0.18181818181818182,
                  "effort": 1268.4312189648022,
                  "time": 70.46840105360012,
                  "bugs": 0.03905920146699976
                }
                "#
                );
            },
        );
    }

    #[test]
    fn groovy_operators_and_operands() {
        check_metrics::<GroovyParser>(
            "class Main {
                static void main(String[] args) {
                    int a, b, c, avg;
                    a = 5; b = 5; c = 5;
                    avg = (a + b + c) / 3;
                    println(avg);
                }
            }",
            "foo.groovy",
            |metric| {
                // Groovy mirror of `java_operators_and_operands`. The juxt
                // call `println avg` exercises `juxt_function_call` in
                // place of Java's `MessageFormat.format(...)`. amaanq's
                // grammar inherits Java's tokenisation, so n1/N1/n2/N2
                // shapes match Java up to those substitutions.
                // The dekobon grammar parses primitive type names
                // (`void`, `int`, `String`) as `type_identifier`
                // rather than as distinct keyword tokens, so they
                // count as operands here — the prior amaanq grammar
                // treated them as operators. Net shift: −2 unique
                // operators (`void`, `int`), +2 unique operands
                // (`void`, `int` were the only two type_identifiers
                // not already counted as operands, since `String`
                // was already an identifier in the prior grammar's
                // counting).
                assert_eq!(metric.halstead.unique_operators(), 8);
                assert_eq!(metric.halstead.unique_operands(), 13);
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 8,
                  "total_operators": 22,
                  "unique_operands": 13,
                  "total_operands": 23,
                  "length": 45,
                  "estimated_program_length": 72.10571633583419,
                  "purity_ratio": 1.6023492519074265,
                  "vocabulary": 21,
                  "volume": 197.65428402504423,
                  "difficulty": 7.076923076923077,
                  "level": 0.14130434782608697,
                  "effort": 1398.7841638695438,
                  "time": 77.71023132608576,
                  "bugs": 0.04169134280255714
                }
                "#
                );
            },
        );
    }

    #[test]
    fn groovy_primitive_types_and_booleans() {
        check_metrics::<GroovyParser>(
            "class Prims {
                byte a = 1
                short b = 2
                int c = 3
                long d = 4
                char e = 'x'
                float f = 1.0f
                double g = 2.0
                boolean h = true
                boolean i = false
            }",
            "foo.groovy",
            |metric| {
                // The dekobon grammar consolidates the 8 primitive
                // type names (`byte`, `short`, `int`, `long`, `char`,
                // `float`, `double`, `boolean`) under `type_identifier`
                // — so they count as operands, not as distinct
                // operators. Likewise numeric literals collapse to one
                // `NumberLiteral` shape (no Hex/Octal/Binary/Decimal
                // split), and `'x'` parses as `StringLiteral` (Groovy
                // single-quoted strings) rather than as
                // `CharacterLiteral`. Operators remaining in this
                // fixture: `=` and `class`-body braces (only `{` is in
                // the operator set). True/false collapse under one
                // `BooleanLiteral`.
                assert_eq!(metric.halstead.unique_operators(), 2);
                assert_eq!(metric.halstead.unique_operands(), 27);
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 2,
                  "total_operators": 10,
                  "unique_operands": 27,
                  "total_operands": 28,
                  "length": 38,
                  "estimated_program_length": 130.38196255841365,
                  "purity_ratio": 3.4311042778529908,
                  "vocabulary": 29,
                  "volume": 184.60327781484773,
                  "difficulty": 1.037037037037037,
                  "level": 0.9642857142857143,
                  "effort": 191.44043625243467,
                  "time": 10.635579791801925,
                  "bugs": 0.01107221547116606
                }
                "#
                );
            },
        );
    }

    // Issue #1263 swept Groovy alongside the JS family and C#: its
    // operand arm listed `QualifiedName` (a `package` / `import` path)
    // and `QualifiedType` on top of the identifier leaves the walker
    // already reached.
    //
    // Only the `QualifiedName` half was observable. The runtime emits
    // `qualified_type` as the *alias* `QualifiedType2` (kind_id 228),
    // which the arm never named — a lesson-2 miss that, by accident,
    // made that half already leaves-only and is why #1263's issue body
    // recorded Groovy as compliant. Both kinds are gone rather than
    // completed.
    //
    // expected, for `package com.example`: operators `.` (1/1);
    // operands `com`, `example` (2/2). Pre-fix the `qualified_name`
    // added `com.example`, making the operand counts 3/3.
    #[test]
    fn groovy_qualified_name_counts_leaves_not_the_composite_1263() {
        check_metrics::<GroovyParser>("package com.example", "foo.groovy", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 1);
            assert_eq!(metric.halstead.total_operators(), 1);
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });
    }

    #[test]
    fn groovy_closure_operators_and_operands() {
        check_metrics::<GroovyParser>("def double = { x -> x * 2 }", "foo.groovy", |metric| {
            // Closure with arrow-style parameter list.
            // Distinct operators: def, =, {}, ->, * = 5.
            // Distinct operands: double, x, 2 = 3.
            assert_eq!(metric.halstead.unique_operators(), 5);
            assert_eq!(metric.halstead.unique_operands(), 3);
        });
    }

    /// Regression for issue #247: every Groovy-specific operator the
    /// prior amaanq grammar dropped to ERROR or mis-shaped as a Java
    /// node now parses as a distinct lexer token in the dekobon
    /// grammar, so Halstead counts each one. The fixture below
    /// exercises Elvis `?:`, safe-nav `?.`, safe-chain `??.`,
    /// spread-dot `*.`, method-pointer `.&`, direct-field `.@`,
    /// identity `===` / `!==`, spaceship `<=>`, regex `=~` / `==~`,
    /// exclusive ranges `..<` / `<..` / `<..<`, `as` coercion, and
    /// `?[` safe index — every distinct operator kind must appear in
    /// `u_operators` (the count grows by exactly the number of new
    /// distinct operator tokens introduced).
    #[test]
    fn groovy_dekobon_operator_coverage_247() {
        check_metrics::<GroovyParser>(
            "def f(a, b, list, s) {
                def x = a ?: b
                def y = a?.field
                def z = a??.field
                def items = list*.size()
                def ptr = a.&size
                def fld = a.@field
                def id1 = a === b
                def id2 = a !== b
                def ship = a <=> b
                def find = s =~ /pat/
                def match = s ==~ /^pat\\$/
                def r1 = 0..<10
                def r2 = 0<..10
                def r3 = 0<..<10
                def cast = a as String
                def safe = list?[0]
                return x
            }",
            "foo.groovy",
            |metric| {
                // Exact pin: with the dekobon Groovy grammar this
                // fixture exercises 16 Groovy-specific tokens (`?:`,
                // `?.`, `??.`, `*.`, `.&`, `.@`, `===`, `!==`, `<=>`,
                // `=~`, `==~`, `..<`, `<..`, `<..<`, `as`, `?[`) plus
                // 6 ambient Java-shaped operators the fixture also
                // uses (`def`, `=`, `,`, `{}`, `()`, `return`), for a
                // total of 22 distinct operator kinds. A regression
                // that drops any one of the 16 #247 operators would
                // push the count below 22 and fail this assertion. The
                // complementary AST walk below pins each #247
                // operator's identity individually so a grammar change
                // that adds an unrelated operator (lifting
                // `u_operators` to 23) still flags the loss of a #247
                // operator at the per-token level.
                //
                // Was 23 until #1314. The extra entry was a `/` — the
                // fixture's two slashy literals (`/pat/`, `/^pat\$/`)
                // each spelled their closing delimiter with the
                // division kind, and the arm now guards them. The
                // enumeration above was wrong in two ways at that
                // count: it listed an ambient `[`, which this fixture
                // never emits (`list?[0]` is the single `?[` token),
                // and omitted the fabricated `/` that made up the
                // difference. Both are corrected here.
                assert_eq!(
                    metric.halstead.unique_operators(),
                    22,
                    "u_operators changed; check whether a #247 operator was dropped or an unrelated operator added (and update the comment / token list above accordingly)",
                );
            },
        );
    }

    #[test]
    fn groovy_gstring_no_double_count() {
        // Issue #454: before the fix Groovy had no interpolation guard
        // at all — `StringLiteral` was classified as a plain operand, so
        // a GString counted the wrapping literal AND descended into its
        // interpolated expression, double-counting the inner identifier
        // in N2. The fix routes `StringLiteral` through
        // `string_operand_type` with both GString interpolation child
        // kinds (`gstring_brace_interpolation` / `gstring_dollar_-
        // interpolation`), so the wrapper is Unknown and only the inner
        // expression contributes.
        //
        // `def greet(name) {\n  return "Hi ${name}"\n}\n`
        //   operands by token text: `greet` × 1, `name` × 2 (param +
        //   inside `${name}`). The wrapping `"Hi ${name}"` is suppressed
        //   → u_operands = 2 (`greet`, `name`), N2 = 3. Without the fix
        //   the wrapping literal would also count → u_operands = 3,
        //   N2 = 4.
        let src = "def greet(name) {\n  return \"Hi ${name}\"\n}\n";
        check_metrics::<GroovyParser>(src, "foo.groovy", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 3);
        });
        assert_ops_operands::<GroovyParser>(src, "foo.groovy", 2, vec!["greet", "name"]);
    }

    #[test]
    fn groovy_gstring_dollar_form_no_double_count() {
        // Issue #454: the short `$name` GString form emits a distinct
        // `gstring_dollar_interpolation` child whose inner `identifier`
        // text is `$name` (the grammar's identifier node spans the
        // leading `$`). The wrapper is suppressed; the inner `$name`
        // operand is distinct from the bare `name` param.
        //
        // `def greet(name) {\n  return "Hi $name"\n}\n`
        //   operands: `greet`, `name` (param), `$name` (interp) →
        //   u_operands = 3, N2 = 3. Without the fix the wrapping
        //   `"Hi $name"` would also count → u_operands = 4, N2 = 4.
        let src = "def greet(name) {\n  return \"Hi $name\"\n}\n";
        check_metrics::<GroovyParser>(src, "foo.groovy", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 3);
            assert_eq!(metric.halstead.total_operands(), 3);
        });
        assert_ops_operands::<GroovyParser>(src, "foo.groovy", 3, vec!["greet", "name", "$name"]);
    }

    #[test]
    fn groovy_plain_string_still_operand() {
        // Counterpart to `groovy_gstring_no_double_count`: a plain
        // non-interpolated literal has neither GString interpolation
        // child and must still contribute exactly one operand.
        //
        // `def f() {\n  return "plain"\n}\n`
        //   operands: `f`, `"plain"` → u_operands = 2, N2 = 2.
        let src = "def f() {\n  return \"plain\"\n}\n";
        check_metrics::<GroovyParser>(src, "foo.groovy", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });
        assert_ops_operands::<GroovyParser>(src, "foo.groovy", 2, vec!["f", "\"plain\""]);
    }

    #[test]
    fn groovy_slashy_string_delimiter_is_not_an_operator() {
        // Regression: issue #1314, the Groovy sibling of Elixir #1256
        // and Ruby/Perl #1312. A slashy string is a `StringLiteral`
        // whose closing delimiter is a `SLASH` — the kind id real
        // division uses — so `def b = /xyz/` reported a `/` operator
        // with no division in the source. Only the closer is a child
        // (the grammar folds the opening `/` into the literal's span),
        // so this fabricated one `/` per literal rather than Ruby's two.
        //
        // expected: operators `def` × 3, `=` × 3 → n1 = 2, N1 = 6.
        // Operands `b`, `/xyz/` × 2, `c`, `s` → n2 = 4, N2 = 6. Before
        // the guard the two closers added `/` → n1 = 3, N1 = 8.
        check_metrics::<GroovyParser>(
            "def b = /xyz/\ndef c = /xyz/\ndef s = b\n",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 2);
                assert_eq!(metric.halstead.total_operators(), 6);
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 6);
            },
        );
    }

    #[test]
    fn groovy_division_survives_the_slashy_guard() {
        // Control for #1314: the guard is scoped to a `StringLiteral`
        // parent, so real division must still count. Both sides are in
        // one fixture — two divisions and one slashy literal — so a
        // guard widened to every `SLASH` fails here rather than
        // silently passing the test above.
        //
        // expected: operators `def` × 2, `=` × 2, `/` × 2 → n1 = 3,
        // N1 = 6. Operands `q`, `a`, `b`, `c`, `r`, `/x/` → n2 = N2 = 6.
        check_metrics::<GroovyParser>("def q = a / b / c\ndef r = /x/\n", "foo.groovy", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 3);
            assert_eq!(metric.halstead.total_operators(), 6);
            assert_eq!(metric.halstead.unique_operands(), 6);
            assert_eq!(metric.halstead.total_operands(), 6);
        });
    }

    #[test]
    fn groovy_slashy_guard_is_parent_scoped_not_ancestor_scoped() {
        // The input that separates the parent-scoped guard from the
        // ancestor-scanning mutant of it — the mutant #1256's
        // post-mortem says survives every ordinary fixture. Groovy is
        // one of only two languages in #1314 where such an input
        // exists at all: a slashy string may carry a GString
        // interpolation, so `/x${a / b}y/` puts a real division under a
        // `StringLiteral` *ancestor* while its parent is the
        // `binary_expression`. An ancestor scan swallows it; the parent
        // check leaves it alone. (The JS, C++ and Tcl/iRules guards
        // have no such input — see
        // `js_regex_delimiter_guard_is_parent_scoped_is_unobservable`.)
        //
        // expected: operators `def` × 2, `=` × 2, `/` (the
        // interpolated division) → n1 = 3, N1 = 5. The wrapping literal
        // is not an operand — it carries an interpolation, so
        // `string_operand_type` yields `Unknown` and the inner
        // expression's operands carry the count (#454) — leaving `r`,
        // `a`, `b`, `s` → n2 = 4, with `a` twice → N2 = 5. Under the
        // ancestor-scoped mutant the division vanishes: n1 = 2, N1 = 4.
        check_metrics::<GroovyParser>(
            "def r = /x${a / b}y/\ndef s = a\n",
            "foo.groovy",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 3);
                assert_eq!(metric.halstead.total_operators(), 5);
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 5);
            },
        );
    }

    #[test]
    fn groovy_every_string_spelling_scores_alike() {
        // Companion to the three above (#1314). Groovy has five ways to
        // write an inert one-character string, and the choice is
        // spelling rather than semantics, so all five must score
        // identically. Before the guard the two slashy forms reported
        // an extra `/` operator that the other three did not — the
        // author's delimiter choice moved n1/N1.
        //
        // The dollar-slashy and quoted forms are no-change controls:
        // `$/…/$` closes with `/$` (kind 144) and `"…"` with `"` (134),
        // neither of which the operator arm classifies. `'x'` is a
        // childless leaf. The escaped-slash row is the one that would
        // regress if the guard were ever narrowed to a literal whose
        // *only* child is the closer.
        //
        // expected per spelling: operators `def` × 3, `=` × 3 → n1 = 2,
        // N1 = 6; operands `a`, the literal, `b`, `c` → n2 = 4, with
        // `a` used three times → N2 = 6.
        for literal in ["/x/", "$/x/$", "'x'", "\"x\"", r"/esc\/aped/"] {
            assert_halstead_counts::<GroovyParser>(
                &format!("def a = {literal}\ndef b = a\ndef c = a\n"),
                "foo.groovy",
                [2, 6, 4, 6],
                &format!("literal {literal}"),
            );
        }
    }

    #[test]
    fn csharp_operators_and_operands() {
        // After issue #286, `void`, `string`, and `int` count as three
        // distinct Halstead operators rather than collapsing into one
        // `PredefinedType` kind_id entry, lifting u_operators from 13
        // to 15. Total operators (N1) is unchanged because the same
        // nodes are still counted, just keyed by lexeme.
        check_metrics::<CsharpParser>(
            "public class Main {
                public static void Run(string[] args) {
                    int a, b, c, avg;
                    a = 5; b = 5; c = 5;
                    avg = (a + b + c) / 3;
                    System.Console.WriteLine(\"{0}\", avg);
                }
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 15);
                assert_eq!(metric.halstead.total_operators(), 32);
                assert_eq!(metric.halstead.unique_operands(), 13);
                assert_eq!(metric.halstead.total_operands(), 23);
                // Pin every Halstead field; values are whatever the
                // classifier produces and become the regression spec.
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    // Issue #1263: C#'s three name *containers* — `qualified_name`
    // (`System.Text`), `generic_name` (`List<int>`) and
    // `alias_qualified_name` (`global::Foo`) — were operands alongside
    // every leaf the walker already reached, so one occurrence of each
    // billed twice. `member_access_expression` never was, which is why
    // `csharp_operators_and_operands`' `System.Console.WriteLine` is
    // unaffected by this change: the bug lived in the *name* grammar,
    // not in member access.
    //
    // Three fixtures rather than one, so a regression names which
    // container came back. Each is hand-tallied; the removed composite
    // is called out per case.
    #[test]
    fn csharp_name_containers_count_leaves_not_the_composite_1263() {
        // expected: operators `using`, `.`, `;` (3/3); operands
        // `System`, `Text` (2/2). Pre-fix the `qualified_name` added
        // `System.Text`, making the operand counts 3/3.
        check_metrics::<CsharpParser>("using System.Text;", "foo.cs", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 3);
            assert_eq!(metric.halstead.total_operators(), 3);
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });

        // expected: operators `class`, `{`×2, `void`, `(`, `;`, `<`,
        // `>`, `int` — 9 total, 8 unique (`int` is the text-keyed
        // primitive operator, per #286). Operands `C`, `M`, `List`, `l`
        // — 4/4. Pre-fix the `generic_name` added `List<int>`, making
        // them 5/5.
        check_metrics::<CsharpParser>(
            "class C { void M() { List<int> l; } }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 8);
                assert_eq!(metric.halstead.total_operators(), 9);
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 4);
            },
        );

        // expected: operators `class`, `{`×2, `void`, `(`, `;`, `=`,
        // `::`, `.` — 9 total, 8 unique. `var` has no operator arm.
        // Operands `C`, `M`, `x`, `global`, `Foo`, `Bar` — 6/6. Pre-fix
        // the `alias_qualified_name` added `global::Foo`, making them
        // 7/7. The `::` staying an operator is what makes the leaf-only
        // tally lossless here, so it is asserted by the operator count
        // rather than assumed.
        check_metrics::<CsharpParser>(
            "class C { void M() { var x = global::Foo.Bar; } }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 8);
                assert_eq!(metric.halstead.total_operators(), 9);
                assert_eq!(metric.halstead.unique_operands(), 6);
                assert_eq!(metric.halstead.total_operands(), 6);
            },
        );
    }

    #[test]
    fn csharp_primitive_types_and_booleans() {
        // After issue #286: each of `byte`, `short`, `int`, `long`,
        // `char`, `float`, `double`, `bool`, `object` is now a distinct
        // Halstead operator (9 primitives) rather than collapsing into
        // one `PredefinedType` kind_id entry. u_operators rises from 6
        // to 14 (5 non-primitive operators + 9 distinct primitives);
        // total operators (N1) is unchanged because the same nodes are
        // still counted, just keyed by lexeme.
        //
        // N2 dropped 23 → 21 with issue #1253: `true` and `false` each
        // reached the walker twice — once as `boolean_literal`, once as
        // the keyword leaf under it — so each added one spurious
        // occurrence. n2 is unchanged at 21 because operands are keyed
        // by source text, so the duplicate collapsed into the existing
        // vocabulary entry; that is exactly why the inflation was
        // invisible in n2. Every operand here is distinct, so
        // N2 == n2 == 21 after the fix.
        check_metrics::<CsharpParser>(
            "public class Prims {
                byte a = 1;
                short b = 2;
                int c = 3;
                long d = 4;
                char e = 'x';
                float f = 1.0f;
                double g = 2.0;
                bool h = true;
                bool i = false;
                object j = null;
            }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 14);
                assert_eq!(metric.halstead.total_operators(), 33);
                assert_eq!(metric.halstead.unique_operands(), 21);
                assert_eq!(metric.halstead.total_operands(), 21);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn csharp_boolean_literal_counts_once() {
        // Regression: issue #1253. `boolean_literal: choice('true',
        // 'false')` wraps the keyword leaf, and both kinds sat in the
        // operand arm, so every `true` / `false` occurrence added +1 to
        // N2. Operands are keyed by source text, so the duplicate
        // collapsed into the same vocabulary entry and n2 stayed
        // correct — which is why nothing caught it.
        //
        // Source repeats `true` so N2 exceeds n2 and the assertions can
        // tell "counted once per occurrence" from "deduplicated into
        // the vocabulary".
        //
        // Operands by text key: `A`, `M`, `a`, `b`, `c`, `d`, `true` × 2,
        // `false`, `null` ⇒ n2 = 9, N2 = 10. Before the fix the keyword
        // leaves added one occurrence per boolean ⇒ N2 = 13.
        check_metrics::<CsharpParser>(
            "class A {\n    void M() {\n        bool a = true;\n        bool b = false;\n        bool c = true;\n        object d = null;\n    }\n}\n",
            "foo.cs",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 9);
                assert_eq!(metric.halstead.total_operands(), 10);
            },
        );
    }

    #[test]
    fn csharp_boolean_keyword_outside_a_literal_still_counts() {
        // Companion to the test above (#1253): the suppression fires on
        // the *parent* kind, never on `True` / `False` alone. C#'s
        // overloadable-operator list emits a bare `true` / `false` token
        // with no `boolean_literal` wrapper — `operator_declaration` is
        // the grammar's only such position — so a blanket exclusion
        // would drop the operand that is the sole difference between
        // `operator true` and `operator false`, leaving two such
        // declarations with identical Halstead vocabularies whenever
        // their bodies match.
        //
        // Each declaration names one boolean and returns the other, so
        // the fixture exercises both the guarded and the unguarded
        // position for each keyword.
        //
        // Operands: `A` × 3 (class name, two parameter types), `a` × 2,
        // `true` × 2 (operator name + literal), `false` × 2 (likewise)
        // ⇒ n2 = 4, N2 = 9. A blanket exclusion gives N2 = 7; no guard
        // at all restores the double count at N2 = 11.
        check_metrics::<CsharpParser>(
            "class A {\n    public static bool operator true(A a) => false;\n    public static bool operator false(A a) => true;\n}\n",
            "foo.cs",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 9);
            },
        );
    }

    #[test]
    fn csharp_predefined_types_keyed_by_lexeme() {
        // Regression: issue #286. The C# grammar emits one `PredefinedType`
        // kind_id for every keyword type (`int`, `string`, `bool`, …).
        // Without keying by source text the entire family collapses into
        // a single Halstead operator (n1 += 1) instead of one per distinct
        // keyword. This test pins the post-fix behaviour using four
        // distinct primitives — `int`, `string`, `bool`, `object` —
        // appearing as parameter types so no other operators interact
        // with the count.
        //
        // expected: operators are `class`, `void`, `M`, `{}`, `()`, `,`
        // (×3 between 4 params), plus the four distinct predefined types
        // → u_operators = 5 + 4 = 9. Without the fix the four primitives
        // collapse to one entry, giving u_operators = 6.
        check_metrics::<CsharpParser>(
            "class C { void M(int a, string b, bool c, object d) {} }",
            "foo.cs",
            |metric| {
                // The headline assertion: four distinct primitive
                // keywords contribute four distinct operators, not one.
                assert_eq!(metric.halstead.unique_operators(), 9);
            },
        );
    }

    #[test]
    fn csharp_interpolated_string_no_double_count() {
        // Regression: issue #183. A C# `$"Hi {name}!"` used to be
        // classified as a Halstead operand (the wrapping
        // `InterpolatedStringExpression`) AND have its inner
        // `Interpolation`'s identifier classified as an operand too.
        // The fix routes `InterpolatedStringExpression` through a
        // conditional: when it has an `Interpolation` child, the inner
        // identifier already carries the operand contribution and the
        // wrapper is treated as `Unknown`; when it does not (static
        // `$"hello"`), the wrapper still counts as one operand.
        //
        // expected: operand contributions for
        //   `class C { void M(string name) { string s = $"Hi {name}!"; } }`
        // — `C` (class), `M` (method), `name` (param), `s` (local),
        // and the inner `name` (inside `{...}`). With the fix,
        // u_operands = 4 (C, M, name, s); N2 = 5 (`name` twice).
        // Without the fix, the wrapping `$"Hi {name}!"` would also
        // count → u_operands = 5, N2 = 6.
        check_metrics::<CsharpParser>(
            "class C { void M(string name) { string s = $\"Hi {name}!\"; } }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 5);
            },
        );
    }

    #[test]
    fn csharp_static_interpolated_string_is_operand() {
        // Regression: issue #183. A `$"..."` with no `{...}` is
        // semantically identical to `"..."` and must still contribute
        // exactly one operand — the conditional `is_child(Interpolation)`
        // check distinguishes it from a true interpolation. expected:
        // operands are `C`, `M`, `s`, `$"hello"` → u_operands = 4, N2 = 4.
        // A naive "always Unknown" fix would yield u_operands = 3, N2 = 3,
        // diverging from the plain-string equivalent below.
        check_metrics::<CsharpParser>(
            "class C { void M() { string s = $\"hello\"; } }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 4);
            },
        );
    }

    #[test]
    fn csharp_plain_string_still_operand() {
        // The fix for #183 only changes how `InterpolatedStringExpression`
        // is classified; plain `StringLiteral` (and `VerbatimStringLiteral`
        // / `RawStringLiteral`) must still contribute exactly one operand
        // each. expected: operands are `C`, `M`, `s`, `"hi"` →
        // u_operands = 4, N2 = 4.
        check_metrics::<CsharpParser>(
            "class C { void M() { string s = \"hi\"; } }",
            "foo.cs",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 4);
            },
        );
    }

    #[test]
    fn go_operators_and_operands() {
        check_metrics::<GoParser>(
            "package main
            func sum(a, b int) int {
                return a + b
            }",
            "foo.go",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 7,
                  "total_operators": 7,
                  "unique_operands": 5,
                  "total_operands": 8,
                  "length": 15,
                  "estimated_program_length": 31.26112492884004,
                  "purity_ratio": 2.0840749952560027,
                  "vocabulary": 12,
                  "volume": 53.77443751081734,
                  "difficulty": 5.6,
                  "level": 0.17857142857142858,
                  "effort": 301.1368500605771,
                  "time": 16.729825003365395,
                  "bugs": 0.014975730436275946
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_operators_and_operands() {
        check_metrics::<PerlParser>(
            "sub sum {
                my ($a, $b) = @_;
                return $a + $b;
            }",
            "foo.pl",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 10,
                  "total_operators": 14,
                  "unique_operands": 4,
                  "total_operands": 6,
                  "length": 20,
                  "estimated_program_length": 41.219280948873624,
                  "purity_ratio": 2.0609640474436812,
                  "vocabulary": 14,
                  "volume": 76.14709844115208,
                  "difficulty": 7.5,
                  "level": 0.13333333333333333,
                  "effort": 571.1032383086406,
                  "time": 31.727957683813365,
                  "bugs": 0.02294502281013948
                }
                "#
                );
            },
        );
    }

    #[test]
    fn perl_interpolated_string_no_double_count() {
        // Regression: issue #199. A `string_double_quoted` (and
        // `string_qq_quoted` / `backtick_quoted` / `command_qx_quoted`)
        // wrapping an `interpolation` child used to be counted as a
        // Halstead operand while the inner scalar/array/hash variable
        // was also walked and counted — double-counting the inner
        // variable's contribution to `N2`. Mirrors #180 (Bash/Elixir),
        // #183 (C#), #184 (PHP), #191 (Kotlin).
        //
        // expected: for
        //   sub greet { my $name = shift; my $msg = "Hi $name"; return $msg; }
        // — operands are `greet`, `$name`, `shift`, `$msg`. With the
        // fix the wrapping `"Hi $name"` is skipped (has `Interpolation`
        // child), so u_operands = 4 and N2 = 6 (`$name` x2 from the
        // `my` binding and the interpolation; `$msg` x2 from the `my`
        // binding and `return`; `greet`, `shift` once each). Without
        // the fix the wrapping literal would also be counted, lifting
        // u_operands to 5 and N2 to 7.
        check_metrics::<PerlParser>(
            "sub greet { my $name = shift; my $msg = \"Hi $name\"; return $msg; }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 6);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn perl_plain_string_still_operand() {
        // The fix for #199 only skips wrapping literals that carry an
        // `Interpolation` child; a plain `"hello"` (no `$…` inside)
        // must still contribute exactly one operand. expected: operands
        // `greet`, `$msg`, `"hello"` → u_operands = 3, N2 = 4 (`$msg`
        // appears in the `my` binding and the `return`).
        check_metrics::<PerlParser>(
            "sub greet { my $msg = \"hello\"; return $msg; }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 3);
                assert_eq!(metric.halstead.total_operands(), 4);
            },
        );
    }

    #[test]
    fn perl_single_quoted_string_never_interpolates() {
        // Single-quoted (`'…'`) and `q{…}` literals are not subject to
        // interpolation in Perl, so even when their text contains a
        // `$name`-shaped sequence the wrapper is still counted as one
        // operand and the inner text is not parsed as a variable.
        // expected: operands `greet`, `$msg`, `'Hi $name'` →
        // u_operands = 3, N2 = 4 (`$msg` x2).
        check_metrics::<PerlParser>(
            "sub greet { my $msg = 'Hi $name'; return $msg; }",
            "foo.pl",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 3);
                assert_eq!(metric.halstead.total_operands(), 4);
            },
        );
    }

    #[test]
    fn perl_plain_heredoc_counts_as_one_operand() {
        // Regression: issue #287. A plain (non-interpolating) Perl
        // heredoc body used to be classified `HalsteadType::Unknown`,
        // so its visible `HeredocBodyStatement` node contributed
        // nothing to N2 even though it is a string literal. The fix
        // adds `HeredocBodyStatement` to the interpolation-aware
        // operand arm, so an inert heredoc counts as one operand.
        //
        // Source (heredoc body lives at the source_file level, not
        // inside any sub):
        //   my $msg = <<END;
        //   hello world
        //   END
        //
        // Operands traversed:
        //   * `$msg` (`scalar_variable`)                    × 1
        //   * heredoc body (`heredoc_body_statement`)       × 1
        // expected: u_operands = 2, N2 = 2.
        check_metrics::<PerlParser>("my $msg = <<END;\nhello world\nEND\n", "foo.pl", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });
    }

    #[test]
    fn perl_interpolated_heredoc_no_double_count() {
        // Regression: issue #287. An interpolating Perl heredoc
        // (`<<"TAG"` or bare `<<TAG`) carries an `Interpolation` child
        // when its body contains a `$var`. The wrapper must drop to
        // `Unknown` so the inner scalar variable carries the operand
        // count — same dispatch as the existing double-quoted /
        // backtick / qx wrappers (issue #199) and the PHP heredoc fix
        // (issue #184).
        //
        // Source:
        //   my $name = "x";
        //   my $msg = <<"END";
        //   hi $name
        //   END
        //
        // Operands by text key:
        //   * `$name` × 2 (my-binding + interpolation inside heredoc)
        //   * `"x"`  × 1 (inert double-quoted string)
        //   * `$msg` × 1
        // expected: u_operands = 3, N2 = 4. Without the
        // interpolation-aware drop the wrapping heredoc body would
        // also count, lifting u_operands to 4 and N2 to 5.
        check_metrics::<PerlParser>(
            "my $name = \"x\";\nmy $msg = <<\"END\";\nhi $name\nEND\n",
            "foo.pl",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 3);
                assert_eq!(metric.halstead.total_operands(), 4);
            },
        );
    }

    #[test]
    fn perl_bare_pattern_delimiters_are_not_operators() {
        // Regression: issue #1312, the Perl sibling of Elixir #1256.
        // The bare match form is the only one of Perl's regex literals
        // whose delimiters are spelled with an operator token kind —
        // `bca dump` shows `/abc/` emitting two `SLASH` under
        // `PatternMatcher` — so `$s =~ /abc/;` reported a `/` operator
        // with no division in the source.
        //
        // expected: operators `$` (the `scalar_variable` sigil), `=~`
        // and `;` → n1 = N1 = 3. Operands `$s` and the `/abc/` pattern
        // → n2 = N2 = 2. Before the guard the two delimiters added `/`
        // → n1 = 4, N1 = 5; the pattern operand arrived with #1314,
        // which promoted all three pattern spellings together (see
        // `perl_every_pattern_value_spelling_scores_alike`).
        check_metrics::<PerlParser>("$s =~ /abc/;\n", "foo.pl", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 3);
            assert_eq!(metric.halstead.total_operators(), 3);
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });
    }

    #[test]
    fn perl_every_pattern_value_spelling_scores_alike() {
        // Companion to the test above (#1312, extended by #1314).
        // `m/abc/` is exactly `/abc/` in Perl and `qr/abc/` is the same
        // pattern as a value, so the three spellings of a pattern
        // *value* must score identically whatever delimiters they use.
        //
        // Until #1314 they scored alike at *zero*: no Perl pattern
        // wrapper was in the operand arm, so the literal never counted,
        // unlike Ruby's `Regex` and Elixir's `Sigil`. #1312 declined to
        // promote the bare form on its own precisely because that would
        // have scored `/abc/` at one operand and its synonyms at zero,
        // reintroducing the spelling sensitivity this test pins.
        // Promoting all three together closes the gap and keeps the
        // equality, which is what this test now asserts.
        //
        // `s///` and `tr///` are deliberately *not* rows here any more.
        // They are operations applied to a target rather than pattern
        // values, so #1314 made them operators; their own equality is
        // pinned by `perl_every_pattern_operation_spelling_scores_alike`
        // below. Splitting the one loop in two is the substantive
        // disagreement #1314 had with the reasoning recorded here: this
        // test governs *synonyms*, and `s///` is not a synonym of
        // `/abc/`.
        //
        // The fixture matches twice against one variable so that no two
        // of `[n1, N1, n2, N2]` are equal. A square tuple would leave
        // `assert_halstead_counts`' unique-vs-total axes unpinned —
        // transposing n1 with N1 inside the helper failed no test while
        // all three of its callers expected a square tuple.
        //
        // expected per variant: operators `$` × 2 (one per
        // `scalar_variable`), `=~` × 2, `and`, `;` → n1 = 4, N1 = 6.
        // The pattern contributes no *operator* — that is #1312's half
        // — and one operand, so with `$s` twice and the pattern twice
        // → n2 = 2, N2 = 4.
        for pattern in ["/abc/", "m/abc/", "m{abc}", "qr/abc/"] {
            assert_halstead_counts::<PerlParser>(
                &format!("$s =~ {pattern} and $s =~ {pattern};\n"),
                "foo.pl",
                [4, 6, 2, 4],
                &format!("pattern {pattern}"),
            );
        }
    }

    #[test]
    fn perl_every_pattern_operation_spelling_scores_alike() {
        // The other half of the split (#1314). Substitution and
        // transliteration are operations applied to a target, so they
        // are operators — and like the value spellings, their delimiter
        // choice must not move the count. `y///` is a synonym of
        // `tr///` and shares `TransliterationTrOrY`, so the two fold to
        // one operator entry, which is why all four rows agree on n1.
        //
        // expected per variant: operators `$` × 2, `=~` × 2, `and`,
        // `;`, and the operation itself × 2 → n1 = 5, N1 = 8. The
        // pattern and replacement text is invisible to this grammar —
        // `substitution_pattern_s` emits only its keyword and
        // delimiters, no content node — so the sole operand is `$s`,
        // twice → n2 = 1, N2 = 2.
        for pattern in ["s/a/b/", "s{a}{b}", "tr/a/b/", "y/a/b/"] {
            assert_halstead_counts::<PerlParser>(
                &format!("$s =~ {pattern} and $s =~ {pattern};\n"),
                "foo.pl",
                [5, 8, 1, 2],
                &format!("pattern {pattern}"),
            );
        }
    }

    #[test]
    fn perl_interpolated_pattern_operands_agree_but_operators_do_not() {
        // Two things at once (#1314), because they are the same
        // measurement: why the three pattern-value spellings route
        // through `string_operand_type` rather than a plain operand
        // arm, and what that routing does *not* fix.
        //
        // `m/$x/` and `qr/$x/` emit a real `Interpolation` wrapping a
        // `scalar_variable`, while the bare form keeps its `$x` inside
        // an unclassified `regex_pattern_content`. So:
        //
        // * Operands agree at n2 = N2 = 2 (`$s` plus one contribution
        //   from the pattern) only because of the interpolation guard.
        //   A plain operand arm would count the wrapper *and* the inner
        //   `$x` for the suffixed forms — n2 = 3 — reintroducing
        //   through the back door the divergence
        //   `perl_every_pattern_value_spelling_scores_alike` exists to
        //   prevent. Which node carries the one operand still differs
        //   by spelling: the wrapper for the bare form, the inner `$x`
        //   for the other two.
        // * Operators do *not* agree: the exposed `scalar_variable`
        //   brings a `$` sigil, an operator here, that the bare form
        //   has no node for. Over two matches N1 is 6 for `/$x/` and
        //   8 for the other two.
        //
        // The operator asymmetry is a grammar gap this classifier
        // cannot repair — there is nothing to classify in the bare
        // form — so it is pinned rather than papered over, the same
        // treatment `perl_division_emits_no_slash_token` gives the
        // missing division token. A bump that starts exposing the bare
        // form's interpolation turns this red, at which point the
        // expectations above need re-deriving.
        // Each fixture matches twice, so `N1 > n1` and `N2 > n2` and no
        // row is a square tuple that a transposition inside
        // `assert_halstead_counts` could pass (#1312).
        assert_halstead_counts::<PerlParser>(
            "$s =~ /$x/ and $s =~ /$x/;\n",
            "foo.pl",
            [4, 6, 2, 4],
            "bare /$x/",
        );
        for pattern in ["m/$x/", "qr/$x/"] {
            assert_halstead_counts::<PerlParser>(
                &format!("$s =~ {pattern} and $s =~ {pattern};\n"),
                "foo.pl",
                [4, 8, 2, 4],
                &format!("suffixed {pattern}"),
            );
        }
    }

    #[test]
    fn perl_division_emits_no_slash_token() {
        // Drift marker, not an endorsement. Ruby's counterpart
        // (`ruby_division_survives_the_regex_guard`) proves #1312's
        // guard cannot swallow a real division; Perl has no such
        // fixture to write, because at the pinned grammar `$a / $b`
        // emits *no* `SLASH` token at all — `binary_expression`'s
        // children skip straight from one `scalar_variable` to the
        // other. Perl division therefore counts zero operators today,
        // a pre-existing grammar gap this fix neither causes nor
        // repairs.
        //
        // Pinning it keeps the gap in CI: a bump that starts emitting
        // the token turns this red, at which point the division would
        // begin counting (its parent is `BinaryExpression`, not
        // `PatternMatcher`, so the guard leaves it alone) and the
        // expectations above need re-deriving.
        //
        // The same gap is why no Perl test can distinguish the
        // parent-scoped guard from an ancestor-scoped one: with no
        // `SLASH` reachable below a `PatternMatcher`, that mutant is
        // unobservable here — measured, not assumed. Perl's guard is
        // parent-scoped for correctness by construction and for
        // symmetry with Ruby's, where the distinction *is* observable
        // and is pinned by
        // `ruby_regex_guard_is_parent_scoped_not_ancestor_scoped`.
        let path = PathBuf::from("foo.pl");
        let source = "my $z = $a / $b;\n";
        let parser = PerlParser::new(source.as_bytes().to_vec(), &path, None);
        assert!(
            !ast_has_kind_id(&parser, Perl::SLASH as u16),
            "tree-sitter-perl still emits no SLASH for `{source}`"
        );
        // Anchor the negative assertion to *this* fixture. Without it
        // the test stays green when `source` is edited to something
        // containing no division at all — measured: swapping in
        // `my $z = 1;` failed nothing.
        //
        // expected: operators `my`, `=`, `$` × 3 (one per
        // `scalar_variable`), `;` → n1 = 4, N1 = 6, with no `/` among
        // them. Operands `$z`, `$a`, `$b` → n2 = N2 = 3.
        check_metrics::<PerlParser>(source, "foo.pl", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 4);
            assert_eq!(metric.halstead.total_operators(), 6);
            assert_eq!(metric.halstead.unique_operands(), 3);
            assert_eq!(metric.halstead.total_operands(), 3);
        });
        // Positive control: the same kind *is* reachable in this
        // grammar, so the assertion above is about division and not
        // about `Perl::SLASH` being enum-only dead weight.
        let matcher = PerlParser::new(b"$s =~ /abc/;\n".to_vec(), &path, None);
        assert!(
            ast_has_kind_id(&matcher, Perl::SLASH as u16),
            "Perl::SLASH must be the bare pattern delimiter kind"
        );
    }

    #[test]
    fn lua_operators_and_operands() {
        check_metrics::<LuaParser>(
            "local function add(a, b)
  local result = a + b
  if result > 0 then
    return result
  end
  return 0
end",
            "foo.lua",
            |metric| {
                // n1=11: local,function,(,,,=,+,if,>,then,return,end
                // (after #695 the `)` closer no longer counts — only the
                // folded `(` opener does; was n1=12).
                // n2=5: add,a,b,result,0
                insta::assert_json_snapshot!(metric.halstead, @r#"
                {
                  "unique_operators": 11,
                  "total_operators": 14,
                  "unique_operands": 5,
                  "total_operands": 10,
                  "length": 24,
                  "estimated_program_length": 49.66338827944708,
                  "purity_ratio": 2.0693078449769615,
                  "vocabulary": 16,
                  "volume": 96.0,
                  "difficulty": 11.0,
                  "level": 0.09090909090909091,
                  "effort": 1056.0,
                  "time": 58.666666666666664,
                  "bugs": 0.03456644293839657
                }
                "#);
            },
        );
    }

    /// Regression for #695. Lua/Bash/Tcl/iRules/PHP/Ruby/Elixir used to
    /// classify the *closing* delimiter (`)`/`]`/`}`) as a separate
    /// operator, while the C-family majority folds each balanced pair to a
    /// single glyph via `get_operator_id_as_str` and counts only the
    /// opener. A balanced `(1)` therefore double-counted as `()` + `)`,
    /// inflating n1 and N1. With the fix only the folded `(` opener counts:
    /// `local x = (1)` yields operators `local`, `=`, `()` — n1 = N1 = 3,
    /// with no standalone `)`.
    #[test]
    fn lua_balanced_paren_counts_opener_only() {
        let source = "local x = (1)\n";
        let path = PathBuf::from("foo.lua");
        let parser = LuaParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
        let paren = ops.operators.iter().filter(|o| o.as_str() == "()").count();
        assert_eq!(
            paren, 1,
            "balanced `(1)` must be one `()` operator; operators were {:?}",
            ops.operators
        );
        assert!(
            !ops.operators.iter().any(|o| o.as_str() == ")"),
            "the closing `)` must not be a separate operator; operators were {:?}",
            ops.operators
        );
    }

    /// Guard for #768. Several `get_op_type` impls (Cpp/C/Objc/Mozcpp/
    /// Tcl/iRules/Php/Elixir/Ruby) classify a grammar's *second-alias*
    /// opener — `LPAREN2`, and for Elixir/Ruby `LBRACK2`/`LBRACK3` — as a
    /// Halstead operator alongside the base `LPAREN`/`LBRACK`. #768 worried
    /// that an alias opener would reach `compute_halstead` with a kind_id
    /// distinct from the base, inflating n1 (a second `()` entry) and
    /// rendering a bare `"("` instead of the folded `"()"`.
    ///
    /// That cannot happen: tree-sitter's runtime collapses each alias to
    /// its base via the grammar's `public_symbol_map` *before*
    /// `Node::kind_id()` (`ts_node_symbol`) ever returns. So the alias
    /// kind_id is unobservable to the metric layer and the alias match arms
    /// are defensive — they only fire if a future grammar bump drops that
    /// collapse. This test pins the invariant: parsing the exact
    /// constructs each grammar produces the alias for internally
    /// (pp-conditional `defined(...)` for Cpp; call arg-list / subscript /
    /// constant-array-pattern for Ruby) must yield **no** node carrying the
    /// alias kind_id, and the balanced opener must count once and render as
    /// the pair glyph. If a grammar bump makes an alias id observable, this
    /// goes red and signals that the alias arms must additionally fold to
    /// the base in `get_operator_id_as_str` (the fix #768 proposed).
    #[test]
    fn second_alias_opener_collapses_to_base_kind_id() {
        fn assert_no_alias<T: crate::ParserTrait>(
            source: &str,
            file: &str,
            alias_id: u16,
            alias_name: &str,
        ) {
            let path = PathBuf::from(file);
            let parser = T::new(source.as_bytes().to_vec(), &path, None);
            let mut stack = vec![parser.root()];
            while let Some(node) = stack.pop() {
                assert_ne!(
                    node.kind_id(),
                    alias_id,
                    "{alias_name} (kind_id {alias_id}) must never reach kind_id() \
                     for `{source}`; the runtime public_symbol_map should have \
                     collapsed it to the base opener. If this fires after a \
                     grammar bump, fold {alias_name} to its pair glyph in \
                     get_operator_id_as_str (issue #768)."
                );
                for child in node.children() {
                    stack.push(child);
                }
            }
        }

        // Balanced openers must count once and render folded (no bare
        // `(`/`[`, no n1 inflation) — the property #768 feared was broken.
        fn assert_folded_openers<T: crate::ParserTrait>(source: &str, file: &str) {
            let path = PathBuf::from(file);
            let parser = T::new(source.as_bytes().to_vec(), &path, None);
            let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
            assert!(
                !ops.operators.iter().any(|o| o.as_str() == "("),
                "no bare `(` operator (must fold to `()`); operators were {:?}",
                ops.operators
            );
            assert!(
                !ops.operators.iter().any(|o| o.as_str() == "["),
                "no bare `[` operator (must fold to `[]`); operators were {:?}",
                ops.operators
            );
            // Each pair glyph appears at most once — the alias does not add
            // a second `()`/`[]` entry to n1.
            assert!(
                ops.operators.iter().filter(|o| o.as_str() == "()").count() <= 1,
                "`()` must be a single n1 entry; operators were {:?}",
                ops.operators
            );
            assert!(
                ops.operators.iter().filter(|o| o.as_str() == "[]").count() <= 1,
                "`[]` must be a single n1 entry; operators were {:?}",
                ops.operators
            );
        }

        // Cpp/C/Mozcpp: LPAREN2 = 20. The grammar emits it internally only
        // inside preprocessor-conditional expressions (`#if defined(FOO)`).
        assert_no_alias::<crate::CppParser>(
            "#if defined(FOO)\n#endif\n",
            "a.cpp",
            20,
            "Cpp::LPAREN2",
        );
        assert_no_alias::<crate::CParser>("#if defined(FOO)\n#endif\n", "a.c", 20, "C::LPAREN2");

        // Ruby: LPAREN2 = 47 (call arg-list), LBRACK3 = 155 (element-
        // reference subscript), LBRACK2 = 46 (constant array pattern).
        assert_no_alias::<crate::RubyParser>("f(1)\n", "a.rb", 47, "Ruby::LPAREN2");
        assert_no_alias::<crate::RubyParser>("a[0]\n", "a.rb", 155, "Ruby::LBRACK3");
        assert_no_alias::<crate::RubyParser>(
            "case p\nin Point[1, 2] then 1\nend\n",
            "a.rb",
            46,
            "Ruby::LBRACK2",
        );

        // Elixir: LPAREN2 = 95 (immediate call paren), LBRACK2 = 96
        // (access / subscript).
        assert_no_alias::<crate::ElixirParser>("f(1)\n", "a.ex", 95, "Elixir::LPAREN2");
        assert_no_alias::<crate::ElixirParser>("x[0]\n", "a.ex", 96, "Elixir::LBRACK2");

        assert_folded_openers::<crate::CppParser>("int main(){ int a[3]; return a[0]; }", "b.cpp");
        assert_folded_openers::<crate::RubyParser>("f(1)\nb = [1]\nb[0]\n", "b.rb");
    }

    #[test]
    fn kotlin_halstead_basic() {
        check_metrics::<KotlinParser>(
            "fun add(a: Int, b: Int): Int {
                val result = a + b
                return result
            }",
            "foo.kt",
            |metric| {
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 9,
                  "total_operators": 11,
                  "unique_operands": 5,
                  "total_operands": 10,
                  "length": 21,
                  "estimated_program_length": 40.13896548741762,
                  "purity_ratio": 1.9113793089246487,
                  "vocabulary": 14,
                  "volume": 79.9544533632097,
                  "difficulty": 9.0,
                  "level": 0.1111111111111111,
                  "effort": 719.5900802688873,
                  "time": 39.97722668160485,
                  "bugs": 0.026767153565498338
                }
                "#
                );
            },
        );
    }

    #[test]
    fn kotlin_string_template_no_double_count() {
        // Re-anchored for issue #454. The pre-#454 comment claimed
        // kotlin-ng emits an `identifier` node for the short `$name`
        // form whose bytes include the leading `$`. That is factually
        // false: AST dump shows the short form produces bare
        // `string_content` tokens (`$`, then `name`) with **no**
        // structured node. The old assertion (u_operands = 4, N2 = 5)
        // passed for the wrong reason (lesson 6): the wrapping literal
        // was counted (+1) and the inner `name` was dropped (-1), and
        // the two errors cancelled. The `$name!` it used also defeats
        // recovery because the grammar glues the trailing `!` onto the
        // name token.
        //
        // Correct mechanism (clean end-of-segment short form):
        // `fun greet(name: String): String {\n    return "Hi $name"\n}\n`
        //   operators: fun, (, ), :, {}, return → as classified.
        //   operands by token text:
        //     `greet` × 1, `name` × 2 (param + recovered short-interp),
        //     `String` × 2 (param type + return type).
        //   The wrapping `"Hi $name"` literal is suppressed and the
        //   inner `name` recovered → u_operands = 3 (`greet`, `name`,
        //   `String`), N2 = 5. Pre-#454: wrapper counted, inner dropped
        //   → u_operands = 4, N2 = 6.
        check_metrics::<KotlinParser>(
            "fun greet(name: String): String {\n    return \"Hi $name\"\n}\n",
            "foo.kt",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 3);
                assert_eq!(metric.halstead.total_operands(), 5);
            },
        );
        // Lesson 4: the ops store agrees on n2 and the exact operand set
        // (inner `name` present, wrapper absent).
        assert_ops_operands::<KotlinParser>(
            "fun greet(name: String): String {\n    return \"Hi $name\"\n}\n",
            "foo.kt",
            3,
            vec!["greet", "name", "String"],
        );
    }

    #[test]
    fn kotlin_short_interpolation_counts_inner_not_wrapper() {
        // Issue #454: the short `$name` template — distinct from the
        // long `${expr}` form, which the kotlin-ng grammar gives a
        // structured `interpolation` node (see
        // `kotlin_string_template_long_form_no_double_count`). The short
        // form has no such node; the variable arrives as a bare
        // `string_content` token preceded by a `$` `string_content`.
        // The fix recovers the clean-identifier variable as an operand
        // and suppresses the opaque wrapper.
        //
        // `fun f() { val x = 1; println("v=$x") }\n`
        //   operands by token text: `f`, `x` × 2 (decl + recovered),
        //   `println`, `1`. The wrapping `"v=$x"` is suppressed →
        //   u_operands = 4 (`f`, `x`, `println`, `1`), N2 = 5.
        // Pre-#454 the wrapper `"v=$x"` counted and the inner `x` was
        // dropped → u_operands = 4 but the wrapper, not `x`, was the
        // fourth operand, and N2 = 5 with the wrong member — the ops
        // assertion below pins the exact set so the cancellation cannot
        // hide it.
        let src = "fun f() { val x = 1; println(\"v=$x\") }\n";
        check_metrics::<KotlinParser>(src, "foo.kt", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 4);
            assert_eq!(metric.halstead.total_operands(), 5);
        });
        assert_ops_operands::<KotlinParser>(src, "foo.kt", 4, vec!["f", "x", "println", "1"]);
    }

    #[test]
    fn kotlin_short_interpolation_space_separated() {
        // Issue #454 follow-up: tree-sitter-kotlin-ng splits the literal
        // only at each `$`, so a `$name` segment's name token absorbs any
        // trailing inter-segment text into its byte range. For `"$a $b"`
        // the token after the first `$` is `"a "` (with the trailing
        // space). Pre-fix `kotlin_is_identifier("a ")` returned false and
        // the leading variable `a` was silently dropped, yielding
        // operands `{b, f, s}` (verified: `a` missing) — breaking parity
        // with the long form `"${a} ${b}"`, which recovers `{a, b, f, s}`.
        //
        // The fix takes the maximal leading-identifier prefix of the name
        // token, recovering `a` and keying it as the bare `"a"` (not
        // `"a "`). Short and long forms must now agree exactly.
        //
        // `fun f() { val s = "$a $b" }\n`
        //   operands by token text: `f`, `s`, `a` (recovered), `b`
        //   (recovered). Wrapper suppressed → u_operands = 4, N2 = 4.
        let short = "fun f() { val s = \"$a $b\" }\n";
        let long = "fun f() { val s = \"${a} ${b}\" }\n";
        check_metrics::<KotlinParser>(short, "foo.kt", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 4);
            assert_eq!(metric.halstead.total_operands(), 4);
        });
        // Both `a` and `b` present, wrapper absent, n2 == dedupe(operands).
        assert_ops_operands::<KotlinParser>(short, "foo.kt", 4, vec!["f", "s", "a", "b"]);
        // Exact parity with the long `${a} ${b}` form.
        assert_ops_operands::<KotlinParser>(long, "foo.kt", 4, vec!["f", "s", "a", "b"]);

        // Comma after the name (`"$a, $b"`): the first name token is
        // `"a, "`; its leading identifier prefix is `a`.
        let comma = "fun f() { val s = \"$a, $b\" }\n";
        assert_ops_operands::<KotlinParser>(comma, "foo.kt", 4, vec!["f", "s", "a", "b"]);

        // Name preceded by literal text and at end-of-segment (`"x=$a"`):
        // the `a` token has no trailing text, so recovery is unchanged.
        let prefixed = "fun f() { val s = \"x=$a\" }\n";
        assert_ops_operands::<KotlinParser>(prefixed, "foo.kt", 3, vec!["f", "s", "a"]);

        // Mid-prose `"$x is "`: the name token is `"x is "`. The leading
        // identifier prefix is `x`, matching the long form `"${x} is "`,
        // which also recovers `x` and treats `" is "` as literal text.
        let prose_short = "fun f() { val s = \"$x is \" }\n";
        let prose_long = "fun f() { val s = \"${x} is \" }\n";
        assert_ops_operands::<KotlinParser>(prose_short, "foo.kt", 3, vec!["f", "s", "x"]);
        assert_ops_operands::<KotlinParser>(prose_long, "foo.kt", 3, vec!["f", "s", "x"]);
    }

    #[test]
    fn kotlin_dollar_non_identifier_stays_literal() {
        // Issue #454 boundary: a `$` not followed by a clean identifier
        // is literal text, not an interpolation. `"price: $5"` (digit
        // after `$`) must keep the wrapping literal as a single operand
        // and recover nothing.
        //
        // `fun f() { val a = "price: $5" }\n`
        //   operands: `f`, `a`, `"price: $5"` → u_operands = 3, N2 = 3.
        let src = "fun f() { val a = \"price: $5\" }\n";
        check_metrics::<KotlinParser>(src, "foo.kt", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 3);
            assert_eq!(metric.halstead.total_operands(), 3);
        });
        assert_ops_operands::<KotlinParser>(src, "foo.kt", 3, vec!["f", "a", "\"price: $5\""]);
    }

    #[test]
    fn kotlin_string_template_long_form_no_double_count() {
        // The `${expr}` long form of a Kotlin string template also
        // produces an `Interpolation` child. The fix must apply to it
        // identically.
        //
        // Source: `fun f(x: Int): String { return "v=${x}" }\n`
        // Operands by source-byte key:
        //   `f` × 1, `x` × 2 (param + inside `${x}`),
        //   `Int` × 1, `String` × 1.
        // With the fix u_operands = 4 (`f`, `x`, `Int`, `String`),
        // N2 = 5. Without the fix the wrapping `"v=${x}"` would also
        // count → u_operands = 5, N2 = 6.
        check_metrics::<KotlinParser>(
            "fun f(x: Int): String { return \"v=${x}\" }\n",
            "foo.kt",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 5);
            },
        );
    }

    #[test]
    fn kotlin_plain_string_still_operand() {
        // The fix for #191 only skips wrapping templates that contain
        // an `Interpolation` child; a plain `"hello"` (no `$` interp)
        // must still contribute exactly one operand.
        //
        // Source: `fun f(): String { return "hello" }\n`
        // Operands: `f` × 1, `String` × 1, `"hello"` × 1 →
        // u_operands = 3, N2 = 3.
        check_metrics::<KotlinParser>(
            "fun f(): String { return \"hello\" }\n",
            "foo.kt",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 3);
                assert_eq!(metric.halstead.total_operands(), 3);
            },
        );
    }

    #[test]
    fn python_fstring_no_double_count() {
        // Regression: issue #191. A Python f-string (`f"Hi {name}!"`)
        // wraps an `Interpolation` child whose inner identifier
        // `name` is walked and counted as its own operand. Without
        // the `is_child(Interpolation)` guard the wrapping `String`
        // would also count, double-counting `name`'s contribution to
        // `N2`. Same pattern as #180 (Bash/Elixir) and #184 (PHP).
        //
        // Source: `def greet(name):\n    return f"Hi {name}!"\n`
        // Operands by source-byte key:
        //   `greet` × 1, `name` × 2 (param + inside `{name}`).
        // With the fix the wrapping `f"Hi {name}!"` is skipped →
        // u_operands = 2 (`greet`, `name`), N2 = 3. Without the fix
        // the wrapping literal would also count → u_operands = 3,
        // N2 = 4.
        check_metrics::<PythonParser>(
            "def greet(name):\n    return f\"Hi {name}!\"\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 2);
                assert_eq!(metric.halstead.total_operands(), 3);
            },
        );
    }

    #[test]
    fn python_plain_string_still_operand() {
        // The fix for #191 only skips wrapping `String` nodes that
        // contain an `Interpolation` child; a plain `"hi"` must still
        // contribute exactly one operand.
        //
        // Source: `def f():\n    return "hi"\n`
        // Operands: `f` × 1, `"hi"` × 1 → u_operands = 2, N2 = 2.
        // (The previous documentation-string filter is preserved:
        // a bare `"hi"` as a top-level `expression_statement` would
        // be skipped, but here it appears as `return "hi"`.)
        check_metrics::<PythonParser>("def f():\n    return \"hi\"\n", "foo.py", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });
    }

    #[test]
    fn python_concatenated_docstring_suppressed() {
        // Regression for #695. An implicit-concatenation docstring
        // (`"""doc""" "more"`) parses as `expression_statement >
        // concatenated_string > [string, string]`. The single-literal
        // docstring guard (`parent == expression_statement &&
        // child_count == 1`) never fired here, so each fragment counted
        // as a separate operand and the docstring's N2 contribution
        // depended on how many literals it was split into. With the fix,
        // every fragment of such a docstring is suppressed.
        //
        // Source: `def f():\n    """doc""" "more"\n    return 1\n`
        // Operands: `f`, `1` only — both docstring fragments suppressed →
        // u_operands = 2, N2 = 2.
        check_metrics::<PythonParser>(
            "def f():\n    \"\"\"doc\"\"\" \"more\"\n    return 1\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 2);
                assert_eq!(metric.halstead.total_operands(), 2);
            },
        );
    }

    #[test]
    fn python_concatenated_non_docstring_still_counts() {
        // The #695 fix must only suppress concatenated literals in the
        // *docstring* position (sole child of an `expression_statement`).
        // A concatenated string used as a value (`x = "a" "b"`) is not a
        // docstring — its `concatenated_string` parent's grandparent is
        // an assignment, not a single-child statement — so both fragments
        // must still be operands.
        //
        // Source: `def f():\n    x = "a" "b"\n    return x\n`
        // Operands: `f`, `x` (twice: assign + return), `"a"`, `"b"` →
        // u_operands = 4, N2 = 5.
        check_metrics::<PythonParser>(
            "def f():\n    x = \"a\" \"b\"\n    return x\n",
            "foo.py",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 5);
            },
        );
    }

    #[test]
    fn python_empty_file_halstead() {
        check_metrics::<PythonParser>("", "empty.py", |metric| {
            let h = &metric.halstead;
            assert_eq!(h.unique_operators(), 0);
            assert_eq!(h.total_operands(), 0);
            assert_eq!(h.estimated_program_length(), 0.0);
            assert_eq!(h.purity_ratio(), 0.0);
            assert_eq!(h.volume(), 0.0);
            assert_eq!(h.difficulty(), 0.0);
            assert_eq!(h.level(), 0.0);
            assert_eq!(h.effort(), 0.0);
            assert_eq!(h.time(), 0.0);
            assert_eq!(h.bugs(), 0.0);
        });
    }

    /// Regression #413, sub-fix (1): `await` was double-counted because the
    /// operator arm listed both the await-expression node (Await=237) and the
    /// nested `await` keyword token (Await2=95). Only the node should count,
    /// mirroring how `yield` counts only the Yield node.
    #[test]
    fn python_await_counted_once_per_use() {
        check_metrics::<PythonParser>(
            "async def f():\n    await a()\n    await b()\n    await c()\n",
            "foo.py",
            |metric| {
                // expected operators: async, def, await  (3 unique)
                //   await used three times -> N1 counts: async(1) def(1) await(3) = 5
                //   Before #413, Await + Await2 both matched, so `await` was a
                //   distinct operator twice: n1=4, N1=8.
                assert_eq!(metric.halstead.unique_operators(), 3);
                assert_eq!(metric.halstead.total_operators(), 5);
            },
        );
    }

    /// Regression #413, sub-fix (3): `lambda` was dropped entirely. Only the
    /// `lambda` keyword token (Lambda3=73) is classified, not the wrapping
    /// Lambda/Lambda2 expression nodes, to avoid an await-style double count.
    #[test]
    fn python_lambda_counted_once() {
        check_metrics::<PythonParser>("g = lambda x: x + 1\n", "foo.py", |metric| {
            // expected operators: =, lambda, +  (3 unique, each used once)
            // Before #413, lambda was absent: only =, + were counted.
            assert_eq!(metric.halstead.unique_operators(), 3);
            assert_eq!(metric.halstead.total_operators(), 3);
        });
    }

    /// Regression #413, sub-fix (2): `match` / `case` keyword tokens
    /// (Match=26, Case=27) were dropped. Each should now count as an operator,
    /// matching the cyclomatic metric which already counts every `case`.
    #[test]
    fn python_match_case_counted() {
        check_metrics::<PythonParser>(
            "match x:\n    case 1:\n        pass\n    case _:\n        pass\n",
            "foo.py",
            |metric| {
                // expected operators: match, case, pass  (3 unique)
                //   match(1) + case(2) + pass(2) = 5 total occurrences.
                // Before #413, neither match nor case was counted (only pass).
                assert_eq!(metric.halstead.unique_operators(), 3);
                assert_eq!(metric.halstead.total_operators(), 5);
            },
        );
    }

    /// Regression #413, sub-fix (2): `nonlocal` (Nonlocal=41) was dropped while
    /// `global` was already classified. Both should count, for parity.
    #[test]
    fn python_nonlocal_and_global_counted() {
        check_metrics::<PythonParser>(
            "def f():\n    global a\n    nonlocal b\n",
            "foo.py",
            |metric| {
                // expected operators: def, global, nonlocal  (3 unique)
                // Before #413, nonlocal was absent: only def, global counted.
                assert_eq!(metric.halstead.unique_operators(), 3);
                assert_eq!(metric.halstead.total_operators(), 3);
            },
        );
    }

    /// Regression #413, sub-fix (4): `not in` (Notin=193) and `is not`
    /// (Isnot=194) are single compound operators. The parent-guard suppresses
    /// the inner Not/In/Is leaves only under those compounds, so standalone
    /// `not x`, `a in b`, `a is b`, and `for x in y` still count their leaves.
    #[test]
    fn python_not_in_is_not_counted_as_single_operator() {
        check_metrics::<PythonParser>(
            "a not in b\na is not b\nnot c\nd in e\nf is g\nfor h in i:\n    pass\n",
            "foo.py",
            |metric| {
                // expected operators (7 unique):
                //   "not in" (compound, once), "is not" (compound, once),
                //   "not" (standalone `not c`, once),
                //   "in" (standalone `d in e` + `for h in i` = twice),
                //   "is" (standalone `f is g`, once),
                //   "for" (once), "pass" (once)
                // Total occurrences: 1+1+1+2+1+1+1 = 8.
                // Before #413, `a not in b` counted not+in (two) and
                // `a is not b` counted is+not (two); the compounds were
                // never classified.
                assert_eq!(metric.halstead.unique_operators(), 7);
                assert_eq!(metric.halstead.total_operators(), 8);
            },
        );
    }

    #[test]
    fn bash_operators_and_operands() {
        check_metrics::<BashParser>(
            "#!/bin/bash
f() {
    local x=1
    if [ $x -eq 1 ]; then
        echo 'one'
    fi
}",
            "foo.sh",
            |metric| {
                // Operators (9 unique, 9 occurrences): the opening
                // delimiters `()`/`{}`/`[]` (each folded to one glyph and
                // counted once per balanced pair, #695 — the closers no
                // longer add a second operator), `local`, `=`, `if`,
                // `then`, `fi`, `;`.
                // Operands (6 unique, 8 occurrences): `f`, `x` (the
                // assignment LHS `variable_name`, kind 160), `1` (twice:
                // `=1` and `-eq 1`), `$x` (the `simple_expansion` — its
                // inner `variable_name` leaf is now suppressed so `$x`
                // counts once, #695), `echo`, `'one'`.
                assert_eq!(metric.halstead.unique_operators(), 9);
                assert_eq!(metric.halstead.total_operators(), 9);
                assert_eq!(metric.halstead.unique_operands(), 6);
                assert_eq!(metric.halstead.total_operands(), 8);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn bash_interpolated_string_no_double_count() {
        // Regression: issue #180. A double-quoted Bash string containing
        // `$name`, `${name[…]}`, or `$(cmd)` used to be classified as a
        // Halstead operand AND have its inner `simple_expansion` /
        // `expansion` / `command_substitution` children classified as
        // operands too. We now skip the wrapping literal when it has an
        // expansion child so only the inner expansion contributes.
        //
        // expected: operands across `a="plain"\nb="$x"\n` —
        //   line 1: variable_name `a`, plain string `"plain"` (no
        //     expansion, still operand) → 2.
        //   line 2: variable_name `b`, wrapping `"$x"` skipped (has
        //     expansion), `simple_expansion` `$x` (its inner
        //     variable_name `x` leaf is suppressed under #695) → 2.
        // Total unique operands: 4 (`a`, `b`, `"plain"`, `$x`), each
        // appearing once → N2 = 4. Before #695 the inner `x` leaf of
        // `$x` was also counted (u_operands = 5, N2 = 5); before the
        // earlier #180 fix the wrapping `"$x"` literal was counted too.
        // The `=` is the only operator; appears twice (N1 = 2, n1 = 1).
        check_metrics::<BashParser>("a=\"plain\"\nb=\"$x\"\n", "foo.sh", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 1);
            assert_eq!(metric.halstead.total_operators(), 2);
            assert_eq!(metric.halstead.unique_operands(), 4);
            assert_eq!(metric.halstead.total_operands(), 4);
            insta::assert_json_snapshot!(metric.halstead);
        });
    }

    #[test]
    fn elixir_interpolated_string_no_double_count() {
        // Regression: issue #180. Without the fix, an interpolated
        // Elixir `String` was classified as a single operand while its
        // inner `interpolation` identifier was also walked and
        // classified as its own operand — double-counting the
        // interpolated identifier's contribution to `N2`.
        //
        // expected: operand contributions for
        //   `def greet(name) do\n  msg = "Hi #{name}"\nend\n` —
        // `def`, `greet`, `name` (param), `msg`, and the inner `name`
        // (inside `#{...}`). With the fix, the wrapping
        // `"Hi #{name}"` literal is skipped (has `Interpolation`
        // child), so `name` is the only repeated operand:
        // u_operands = 4 (def, greet, name, msg), N2 = 5. Without the
        // fix, the wrapping literal would also count → u_operands = 5,
        // N2 = 6. Operators: `do`, `end`, `(`, `=` → u = N = 4.
        // Only the *opening* delimiters count after #695, so the `)`
        // and the `}` interpolation closer add no operator; #1314 then
        // dropped the `#{` opener too, on the rule that an
        // interpolation opener is spelling rather than an operation
        // (was 5 here, and 7 before #695).
        check_metrics::<ElixirParser>(
            "def greet(name) do\n  msg = \"Hi #{name}\"\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 4);
                assert_eq!(metric.halstead.total_operators(), 4);
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 5);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn elixir_plain_string_still_operand() {
        // The fix for #180 only skips wrapping literals that contain
        // interpolation; a plain `"hello"` must still contribute exactly
        // one operand. expected: `def`, `f`, `"hello"` → 3 unique
        // operands (n2 = 3), each appearing once (N2 = 3).
        check_metrics::<ElixirParser>("def f do\n  \"hello\"\nend\n", "foo.ex", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 3);
            assert_eq!(metric.halstead.total_operands(), 3);
        });
    }

    #[test]
    fn elixir_boolean_and_nil_literals_count_once() {
        // Regression: issue #1253. `boolean: choice("true", "false")`
        // and `nil: "nil"` each wrap a keyword leaf, and both the
        // wrapper and the leaf sat in the operand arm — so every
        // literal occurrence added +1 to N2. Operands are keyed by
        // source text, so the duplicate collapsed into the same
        // vocabulary entry and n2 stayed correct, which is why nothing
        // caught it.
        //
        // Source is the issue's reproducer plus a repeat of `true` and
        // `nil`, so N2 exceeds n2 and the assertions can tell "counted
        // once per occurrence" from "deduplicated into the vocabulary".
        // All three keywords appear, so restoring any one of `True`,
        // `False`, or `Nil2` to the operand arm trips this test.
        //
        // Operands by text key: `x`, `y`, `z`, `w`, `v`, `true` × 2,
        // `nil` × 2, `false` ⇒ n2 = 8, N2 = 10. Before the fix each of
        // the five literals counted twice ⇒ N2 = 15.
        //
        // This also guards the drift in the other direction. Elixir
        // classifies the wrapper and drops the leaf outright rather
        // than parent-guarding it, so a grammar bump that stopped
        // emitting `boolean` / `nil` would leave the leaves unclassified
        // and the literals would vanish from N2 entirely (⇒ 5 / 5)
        // rather than merely being miscounted.
        check_metrics::<ElixirParser>(
            "x = true\ny = nil\nz = false\nw = true\nv = nil\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 8);
                assert_eq!(metric.halstead.total_operands(), 10);
            },
        );
    }

    #[test]
    fn elixir_reserved_word_after_a_dot_stays_an_operand() {
        // Companion to the test above (#1253). Elixir drops `True` /
        // `False` / `Nil2` from the operand arm outright, which is only
        // safe because the one grammar position that accepts a reserved
        // word outside the `boolean` / `nil` wrapper — the right-hand
        // side of a remote dot — aliases it to `identifier`. This pins
        // that alias: if a grammar bump emitted the bare keyword there
        // instead, `Foo.nil` and `Foo.true` would silently stop
        // contributing an operand.
        //
        // Source: a = Foo.nil / b = Foo.true / c = nil
        //
        // Operands by text key: `a`, `Foo` × 2, `nil` × 2 (the aliased
        // identifier and the real literal, which share a text key),
        // `b`, `true`, `c` ⇒ n2 = 6, N2 = 8. Losing the alias drops the
        // two dotted references ⇒ N2 = 6.
        check_metrics::<ElixirParser>("a = Foo.nil\nb = Foo.true\nc = nil\n", "foo.ex", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 6);
            assert_eq!(metric.halstead.total_operands(), 8);
        });
    }

    #[test]
    fn elixir_interpolated_sigil_no_double_count() {
        // Sigils mirror strings under #180. For `~r/foo#{name}/`, the
        // wrapping `Sigil` is skipped, but `SigilName` (`r`) and the
        // inner `name` identifier each contribute one operand.
        // expected: `def`, `f`, `name` (param), `re`, `r` (sigil name),
        // `name` (inside `#{...}`) → u_operands = 5, N2 = 6 (`name`
        // twice).
        check_metrics::<ElixirParser>(
            "def f(name) do\n  re = ~r/foo#{name}/\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 5);
                assert_eq!(metric.halstead.total_operands(), 6);
            },
        );
    }

    #[test]
    fn elixir_interpolated_charlist_no_double_count() {
        // Charlists mirror strings and sigils under #180. The
        // `E::String | E::Charlist | E::Sigil` arm in `get_op_type`
        // skips any wrapping literal that has an `Interpolation`
        // child; this test exercises the `Charlist` branch
        // specifically.
        //
        // expected: for `def f(name) do\n  cl = 'Hi #{name}'\nend\n` —
        // `def`, `f`, `name` (param), `cl`, and the inner `name`
        // (inside `#{...}`). With the fix, the wrapping
        // `'Hi #{name}'` is skipped → u_operands = 4 (def, f, name,
        // cl), N2 = 5 (`name` twice).
        check_metrics::<ElixirParser>(
            "def f(name) do\n  cl = 'Hi #{name}'\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 5);
            },
        );
    }

    #[test]
    fn elixir_sigil_delimiters_are_not_operators() {
        // Regression: issue #1256. Sigil delimiter tokens share their
        // kind ids with real operators (`SLASH`, `LPAREN`, `LBRACE`,
        // …) and were classified unconditionally, so `~r/abc/`
        // fabricated two division operators and the author's delimiter
        // choice moved n1/N1. The parent guard suppresses them under
        // `Sigil`; `~` stays the single per-sigil operator.
        //
        // expected: operators `=` × 3 and `~` × 3 → n1 = 2, N1 = 6.
        // Without the guard the delimiters added `/` × 2, `(`, `{` →
        // n1 = 5, N1 = 10. Operands: `a`, `~r/abc/i`, `r`, `i` (sigil
        // modifiers), `b`, `~w(one two)`, `w`, `c`, `~s{hi}`, `s` →
        // n2 = N2 = 10.
        check_metrics::<ElixirParser>(
            "a = ~r/abc/i\nb = ~w(one two)\nc = ~s{hi}\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 2);
                assert_eq!(metric.halstead.total_operators(), 6);
                assert_eq!(metric.halstead.unique_operands(), 10);
                assert_eq!(metric.halstead.total_operands(), 10);
            },
        );
    }

    #[test]
    fn elixir_sigil_delimiter_choice_is_invariant() {
        // Companion to the test above (#1256): two sigils differing
        // only in delimiter are the same literal, so every delimiter
        // choice must produce identical Halstead counts. `(` `[` `{`
        // `<` `/` `|` are the operator-kind delimiters the guard
        // covers; `"` and `'` never had an operator arm and pin the
        // already-correct path.
        //
        // expected per variant: operators `=`, `~` → n1 = 2, N1 = 2;
        // operands `x`, the sigil literal text, `w` (sigil name) →
        // n2 = 3, N2 = 3.
        for (open, close) in [
            ("(", ")"),
            ("[", "]"),
            ("{", "}"),
            ("<", ">"),
            ("/", "/"),
            ("|", "|"),
            ("\"", "\""),
            ("'", "'"),
        ] {
            assert_halstead_counts::<ElixirParser>(
                &format!("x = ~w{open}one two{close}\n"),
                "foo.ex",
                [2, 2, 3, 3],
                &format!("delimiter pair {open} {close}"),
            );
        }
    }

    #[test]
    fn elixir_standalone_operators_survive_the_sigil_guard() {
        // Control for #1256: the guard is parent-scoped, so the same
        // token kinds outside a sigil still count. Covers every guarded
        // kind standalone: `/` (division), `<` / `>` (comparison), `[`
        // and `|` (list cons), `(` (call), `{` (map literal, with its
        // `%`).
        //
        // expected: operators `=` × 6, `/`, `<`, `>`, `[`, `|`, `(`,
        // `%`, `{` → n1 = 9, N1 = 14. Operands: `x`, `a`, `b`, `y`,
        // `c`, `d`, `z`, `e`, `f`, `q`, `h`, `t`, `p`, `g`, `1`, `m`,
        // the `k:` keyword, `2` → n2 = N2 = 18.
        check_metrics::<ElixirParser>(
            "x = a / b\ny = c < d\nz = e > f\nq = [h | t]\np = g(1)\nm = %{k: 2}\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 9);
                assert_eq!(metric.halstead.total_operators(), 14);
                assert_eq!(metric.halstead.unique_operands(), 18);
                assert_eq!(metric.halstead.total_operands(), 18);
            },
        );
    }

    #[test]
    fn elixir_interpolated_sigil_keeps_inner_nodes_counting() {
        // Interpolation inside a sigil after #1256: the `{` delimiter
        // is suppressed (its parent is the `Sigil`), while the
        // `interpolation` child is a separate node whose inner
        // identifier must still count — the guard must not reach past
        // the delimiter tokens.
        //
        // expected: operators `=`, `~` → n1 = N1 = 2. Operands:
        // `v`, `s` (sigil name), `b` (interpolated identifier); the
        // wrapping sigil is skipped (`Interpolation` child, #180) and
        // `quoted_content` is unclassified → n2 = N2 = 3. The `#{`
        // marker was a third operator until #1314 dropped it.
        check_metrics::<ElixirParser>("v = ~s{a#{b} c}\n", "foo.ex", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 2);
            assert_eq!(metric.halstead.total_operators(), 2);
            assert_eq!(metric.halstead.unique_operands(), 3);
            assert_eq!(metric.halstead.total_operands(), 3);
        });

        // A guarded kind *inside* the interpolation: the `/` in
        // `#{a / b}` has `binary_operator` as its parent but the
        // `Sigil` as a further ancestor, so this input is the one
        // discriminator between the correct parent-scoped guard and a
        // wrong ancestor-scoped one that would swallow it.
        //
        // expected: operators `=`, `~`, `/` → n1 = N1 = 3 (the `#{`
        // opener stopped counting with #1314); operands `v`, `s`, `a`,
        // `b` → n2 = N2 = 4. The division is what this row is for, and
        // it still counts — the ancestor-scoped mutant drops it.
        check_metrics::<ElixirParser>("v = ~s{x #{a / b} y}\n", "foo.ex", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 3);
            assert_eq!(metric.halstead.total_operators(), 3);
            assert_eq!(metric.halstead.unique_operands(), 4);
            assert_eq!(metric.halstead.total_operands(), 4);
        });
    }

    #[test]
    fn bash_all_expansion_kinds_skip_wrapper() {
        // Exercises every node kind tested by
        // `bash_string_has_expansion`: `simple_expansion` (`$v`),
        // `expansion` (`${v[0]}`), `command_substitution` (`$(date)`),
        // and `arithmetic_expansion` (`$((1+2))`). A typo replacing
        // one kind with an aliased neighbour in `language_bash.rs`
        // (e.g., `ExpansionBody` instead of `Expansion`) would leave
        // the corresponding wrapping string counted as an operand and
        // shift the totals.
        //
        // expected: operands across the four lines —
        //   line 1 `a="$v"`: var_name `a`, simple_expansion `$v` (its
        //     inner var_name `v` leaf is suppressed under #695; wrapper
        //     skipped) → 2
        //   line 2 `b="${v[0]}"`: var_name `b`, var_name `v` (inside
        //     subscript — parent is `expansion`, not `simple_expansion`,
        //     so it still counts), number `0` (wrapper skipped,
        //     `expansion` itself is not in the operand list) → 3
        //   line 3 `c="$(date)"`: var_name `c`, command_name `date`
        //     (wrapper skipped, `command_substitution` not in operand
        //     list) → 2
        //   line 4 `d="$((1+2))"`: var_name `d`, numbers `1` and `2`
        //     (wrapper skipped, `arithmetic_expansion` not in operand
        //     list) → 3
        // Unique operands: a, b, c, d, $v, v, 0, date, 1, 2 → 10. Total
        // occurrences: 11 (`v` now appears once — only line 2's subscript
        // leaf; line 1's `$v` inner leaf is suppressed). Operators after
        // #695: only the openers `[` (folded `[]`) and `+`, plus `=` four
        // times — the `}`/`)`/`))`/`]` closers no longer count.
        check_metrics::<BashParser>(
            "a=\"$v\"\nb=\"${v[0]}\"\nc=\"$(date)\"\nd=\"$((1+2))\"\n",
            "foo.sh",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 3);
                assert_eq!(metric.halstead.total_operators(), 6);
                assert_eq!(metric.halstead.unique_operands(), 10);
                assert_eq!(metric.halstead.total_operands(), 11);
            },
        );
    }

    /// Regression for #695. A bare `$x` (outside any string) parses as a
    /// `simple_expansion` wrapping a `variable_name` leaf — and `$?` / `$1`
    /// as a `simple_expansion` wrapping a `special_variable_name` leaf. Both
    /// the wrapper and the inner leaf used to be classified as operands, so
    /// each bare variable reference double-counted (the same hazard Tcl
    /// guards with its `Id2` exclusion and iRules with a parent check). The
    /// `variable_name` / `special_variable_name` arm now yields `Unknown`
    /// when its parent is a `simple_expansion`, so `$x` contributes exactly
    /// one operand while the assignment LHS `variable_name` (`x` in `x=…`,
    /// parent is `variable_assignment`) still counts.
    #[test]
    fn bash_bare_variable_no_double_count() {
        let source = "x=1\necho $x\necho $?\n";
        let path = PathBuf::from("foo.sh");
        let parser = BashParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
        let bare_x = ops.operands.iter().filter(|o| o.as_str() == "$x").count();
        let special = ops.operands.iter().filter(|o| o.as_str() == "$?").count();
        // Each bare reference is exactly one operand; the inner leaf is not
        // double-counted. If the guard regressed, the inner `variable_name`
        // `x` would add a second `x` occurrence (text-colliding with the
        // assignment LHS) and the inner `special_variable_name` `?` would
        // appear as a standalone `?` operand.
        assert_eq!(
            bare_x, 1,
            "bare $x must be one operand; operands were {:?}",
            ops.operands
        );
        assert_eq!(
            special, 1,
            "bare $? must be one operand; operands were {:?}",
            ops.operands
        );
        assert!(
            !ops.operands.iter().any(|o| o.as_str() == "?"),
            "the inner special_variable_name `?` leaf must be suppressed; operands were {:?}",
            ops.operands
        );
        // The assignment LHS `variable_name` `x` (parent `variable_assignment`,
        // not `simple_expansion`) must still be an operand.
        assert!(
            ops.operands.iter().any(|o| o.as_str() == "x"),
            "assignment LHS `x` must still be an operand; operands were {:?}",
            ops.operands
        );
    }

    #[test]
    fn tcl_operators_and_operands() {
        check_metrics::<TclParser>(
            "proc f {a b} {
    set x [expr {$a + $b}]
    if {$x > 0 && $x != 0} {
        return $x
    }
    return 0
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn tcl_bitwise_ternary_string_ops() {
        // Exercises operator families not covered by tcl_operators_and_operands:
        // bitwise (&, |, ^, ~, <<, >>), ternary (?), and string-comparison (eq, ne, in, ni).
        check_metrics::<TclParser>(
            "proc f {a b} {
    set bits [expr {$a & $b | $a ^ ~$b}]
    set sh [expr {$a << 1 | $b >> 1}]
    set t [expr {$a > 0 ? $a : $b}]
    if {$a eq {x} || $a ne {y}} {
        return $a
    }
    return $b
}",
            "foo.tcl",
            |metric| {
                // Anchored per the snapshot policy in AGENTS.md, which
                // this call predates. N1 fell 33 → 31 with #1314: the
                // `if` condition's `{x}` and `{y}` are braced *words*,
                // so their openers stopped fabricating a `{}` operator.
                // n1 is unchanged at 18 because the `{}` entry survives
                // on the proc body and the `expr` braces — which is
                // exactly why the fabrication was invisible in n1 and
                // is the reason to assert N1 as well (#1294).
                assert_eq!(metric.halstead.unique_operators(), 18);
                assert_eq!(metric.halstead.total_operators(), 31);
                assert_eq!(metric.halstead.unique_operands(), 17);
                assert_eq!(metric.halstead.total_operands(), 30);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn tcl_bare_variable_operand() {
        // Bare `$varname` produces a VariableSubstitution node (already an operand).
        // Its anonymous Id2 child must NOT be counted separately; each reference is 1 operand.
        check_metrics::<TclParser>(
            "proc f {x} {
    return $x
}",
            "foo.tcl",
            |metric| {
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn tcl_inert_quoted_word_counts_as_operand() {
        // Regression for #277. A `"..."` literal with no `$var` / `[cmd]`
        // interpolation must contribute exactly one operand (the wrapping
        // `QuotedWord`). The string content `hello world` is exposed as a
        // single `_quoted_word_content` token (not itself classified by
        // `get_op_type`), so the only operands here are `f`, `s`, and the
        // quoted string. `set` is the anonymous `Set2` keyword and is
        // classified as an operator, not an operand.
        check_metrics::<TclParser>(
            "proc f {} {
    set s \"hello world\"
}",
            "foo.tcl",
            |metric| {
                // Operands: `f`, the proc-body `braced_word`, the `set`
                // target `s`, `"hello world"` — 4 unique, 4 total. Before
                // #1294 this read 3/3 with `s` missing (the body operand
                // made the count coincidentally plausible). The wrapping
                // `QuotedWord` must still contribute exactly one operand
                // when it carries no interpolation children; dropping to 3
                // would mean the inert case was over-guarded.
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 4);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn tcl_interpolated_quoted_word_no_double_count() {
        // Regression for #277. Before the fix, `"$x is $y"` produced an
        // extra operand for the wrapping `QuotedWord` on top of the two
        // inner `VariableSubstitution` operands (`$x`, `$y`), giving 7.
        // After the fix, the wrapper is `HalsteadType::Unknown` whenever
        // it carries an interpolation child, so operand attribution
        // belongs solely to the inner substitutions.
        check_metrics::<TclParser>(
            "proc f {x y} {
    set s \"$x is $y\"
}",
            "foo.tcl",
            |metric| {
                // Operands: `f`, `x`, `y` (proc args), the proc-body
                // `braced_word`, the `set` target `s`, `$x`, `$y` — 7
                // unique, 7 total. The wrapping `QuotedWord` contributes
                // nothing. Before #1294 this read 6/6 with `s` missing;
                // before #277 the wrapper double-counted.
                assert_eq!(metric.halstead.unique_operands(), 7);
                assert_eq!(metric.halstead.total_operands(), 7);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn tcl_command_substitution_quoted_word_no_double_count() {
        // Regression for #277. A `"...[cmd]..."` literal exposes the
        // bracketed command as a `command_substitution` child whose inner
        // identifiers/literals contribute their own operands. The wrapping
        // `QuotedWord` must not also be classified as an operand, or the
        // command's identifier would be counted alongside a phantom
        // wrapper operand.
        check_metrics::<TclParser>(
            "proc f {} {
    set s \"result: [foo]\"
}",
            "foo.tcl",
            |metric| {
                // Operands: `f`, the proc-body `braced_word`, the `set`
                // target `s`, `foo` — 4 unique, 4 total. The wrapping
                // `QuotedWord` and the inert text `result: ` do not
                // contribute extra operands. Before #1294 this read 3/3
                // with `s` missing; before #277 the wrapper double-counted.
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 4);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    /// Regression for #1294. The `set` target parses as the anonymous
    /// `id` token (`Tcl::Id2`) — the same kind as the leaf inside a
    /// `variable_substitution` — and the getter used to exclude that kind
    /// wholesale, so every variable a Tcl script assigned was absent from
    /// n2/N2. The guard is now parent-scoped: a target `id` counts, a
    /// var-sub leaf does not. Exact occurrence counts distinguish this
    /// fix from a regression in either direction: a re-blanketed
    /// exclusion drops `s`/`t` (total 2), while losing the guard
    /// double-counts the `$s` leaf as a second `s` (total 5).
    #[test]
    fn tcl_set_target_is_operand() {
        let source = "set s 1\nset t $s\n";
        let path = PathBuf::from("foo.tcl");
        let parser = TclParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
        // expected operands: targets `s` and `t`, literal `1`, reference
        // `$s` (wrapper only) — 4 total, each exactly once.
        for operand in ["s", "t", "1", "$s"] {
            assert_eq!(
                ops.operands
                    .iter()
                    .filter(|o| o.as_str() == operand)
                    .count(),
                1,
                "`{operand}` must be exactly one operand; operands were {:?}",
                ops.operands
            );
        }
        assert_eq!(
            ops.operands.len(),
            4,
            "operands must be exactly s, t, 1, $s; got {:?}",
            ops.operands
        );

        check_metrics::<TclParser>(source, "foo.tcl", |metric| {
            // expected: n2 = 4 (s, t, 1, $s), N2 = 4; operators are the
            // two `set` keywords — n1 = 1, N1 = 2.
            assert_eq!(metric.halstead.unique_operands(), 4);
            assert_eq!(metric.halstead.total_operands(), 4);
            assert_eq!(metric.halstead.unique_operators(), 1);
            assert_eq!(metric.halstead.total_operators(), 2);
        });
    }

    /// Drift marker for #1294 (lesson 34 / grammar-dispatch §2): the
    /// *named* `id` rule (`Tcl::Id`, kind_id 84) never surfaces at the
    /// pinned tree-sitter-tcl — the parser emits the anonymous `Id2` in
    /// both positions the getter guards (the `set` target and the
    /// var-sub leaf). The `Tcl::Id` arm in `get_op_type` is therefore
    /// defensive; if a grammar bump starts emitting 84 this fails and
    /// the arm's classification must be re-derived instead of trusted.
    #[test]
    fn tcl_named_id_variant_is_unreachable() {
        let source = "proc f {x} {\n    set s $x\n    foreach v {1 2} { puts \"$v\" }\n}\n";
        let path = PathBuf::from("foo.tcl");
        let parser = TclParser::new(source.as_bytes().to_vec(), &path, None);
        // Non-vacuity: the anonymous token must be present in this parse
        // (both the `set` target and the `$x` / `$v` leaves emit it).
        assert!(
            ast_has_kind_id(&parser, Tcl::Id2 as u16),
            "expected the anonymous Tcl::Id2 token to appear in the parse",
        );
        assert!(
            !ast_has_kind_id(&parser, Tcl::Id as u16),
            "the named Tcl::Id rule surfaced; re-derive the defensive \
             `Tcl::Id` arm in TclCode::get_op_type against the new grammar",
        );
    }

    #[test]
    fn tcl_braced_word_delimiter_is_not_an_operator() {
        // Regression: issue #1314, the Tcl sibling of Elixir #1256 and
        // Ruby/Perl #1312. A braced *word* — a literal value, not a
        // script — carries its `{` as an `LBRACE` child, the kind id a
        // real block uses, so `set a {braced word}` reported a `{}`
        // operator with no block in the source.
        //
        // expected: operator `set` × 3 → n1 = 1, N1 = 3. Operands
        // `a`, `b`, `c`, `$a`, `{braced word}` × 2 and its inner
        // `braced` / `word` × 2 each → n2 = 7, N2 = 10. (The inner
        // words counting alongside the word that contains them is a
        // separate, pre-existing defect, filed off #1314 as #1317;
        // this test pins today's totals rather than endorsing them.)
        // Before the guard the two openers added `{}` → n1 = 2,
        // N1 = 5.
        check_metrics::<TclParser>(
            "set a {braced word}\nset b {braced word}\nset c $a\n",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 1);
                assert_eq!(metric.halstead.total_operators(), 3);
                assert_eq!(metric.halstead.unique_operands(), 7);
                assert_eq!(metric.halstead.total_operands(), 10);
            },
        );
    }

    #[test]
    fn tcl_script_bodies_keep_their_braces() {
        // Control for #1314, and the reason a kind-scoped guard is safe
        // in Tcl where it would not be elsewhere: the grammar gives the
        // literal and the block *different* kinds. A `proc` body, an
        // `if` body and an `if` condition are `BracedWord` (88) and
        // `Expr` (97); only the value form is `BracedWordSimple` (89).
        // This fixture nests a braced word inside a real script body,
        // so a guard that keyed on the brace alone would drop the
        // block's `{}` and fail here.
        //
        // expected: operators `proc`, `set` × 2, `if`, `>`, and `{}`
        // × 4 (the proc parameter list, the proc body, the `if`
        // condition, the `if` body) → n1 = 5, N1 = 9. Operands, all
        // distinct → n2 = N2 = 13: the words `p`, `x`, `a`, `v`, `w`,
        // `$x`, `1`, `b`, `y`, the two braced *words* `{v w}` and
        // `{y}`, and — the part worth noting — the two *script* bodies
        // `{\n  set a …\n}` and `{ set b {y} }`, which are
        // `BracedWord` operands in their own right. A script body
        // therefore counts twice over: once as this operand and once
        // as the `{}` operator above. That is pre-existing and not
        // what this test guards; it is spelled out so the count is
        // re-derivable.
        check_metrics::<TclParser>(
            "proc p {x} {\n  set a {v w}\n  if {$x > 1} { set b {y} }\n}\n",
            "foo.tcl",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 5);
                assert_eq!(metric.halstead.total_operators(), 9);
                assert_eq!(metric.halstead.unique_operands(), 13);
                assert_eq!(metric.halstead.total_operands(), 13);
            },
        );
    }

    #[test]
    fn tcl_braced_word_guard_is_parent_scoped_not_ancestor_scoped() {
        // The input that separates the parent-scoped guard from the
        // ancestor-scanning mutant. I first recorded this distinction
        // as *unobservable* in Tcl, reasoning that a braced word holds
        // only simple words and nested braced words. `bca dump` says
        // otherwise: the grammar parses a `[…]` command substitution
        // inside a braced word, and the `if` inside it brings an `Expr`
        // condition and a `BracedWord` body, each with its own `{`,
        // both of them non-immediate descendants of the
        // `BracedWordSimple`. An ancestor scan swallows both.
        //
        // (Real Tcl does not substitute inside braces — this is the
        // grammar modelling structure it will not evaluate. What the
        // classifier sees is what the metric reports, so it is the
        // right fixture regardless.)
        //
        // expected: operators `set`, `[]`, `if`, and `{}` × 2 (the
        // `if` condition's `Expr` and its `BracedWord` body; the outer
        // value word's own `{` is suppressed) → n1 = 4, N1 = 5.
        // Operands `z`, `x`, `$q`, `puts`, `w`, `v` and the two braced
        // words → n2 = N2 = 8. Under the ancestor-scoped mutant both
        // surviving braces vanish: n1 = 3, N1 = 3.
        check_metrics::<TclParser>("set z {x [if {$q} {puts w}] v}\n", "foo.tcl", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 4);
            assert_eq!(metric.halstead.total_operators(), 5);
            assert_eq!(metric.halstead.unique_operands(), 8);
            assert_eq!(metric.halstead.total_operands(), 8);
        });
    }

    #[test]
    fn irules_braced_word_guard_is_parent_scoped_not_ancestor_scoped() {
        // The iRules twin of the test above — the two getters are
        // clones, so the mutant must fail in both.
        //
        // expected: operators `when`, `set`, `[]`, `if`, `{}` × 3 (the
        // handler body, the `if` condition and the `if` body) → n1 = 5,
        // N1 = 7. Operands `HTTP_REQUEST`, `z`, `x`, `$q`, `log`, `w`,
        // `v` and the braced words → n2 = N2 = 10.
        check_metrics::<IrulesParser>(
            "when HTTP_REQUEST {\n  set z {x [if {$q} {log w}] v}\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 5);
                assert_eq!(metric.halstead.total_operators(), 7);
                assert_eq!(metric.halstead.unique_operands(), 10);
                assert_eq!(metric.halstead.total_operands(), 10);
            },
        );
    }

    #[test]
    fn php_operators_and_operands() {
        check_metrics::<PhpParser>(
            "<?php
            function avg(int $a, int $b, int $c): int {
                return ($a + $b + $c) / 3;
            }",
            "foo.php",
            |metric| {
                // After #695 only the opening delimiters count: `()` and
                // `{}` fold to one operator each per balanced pair, so the
                // former `)`/`}` closers no longer inflate n1/N1 (was
                // 11 unique / 15 total).
                //
                // Operands after #1293, tallied by `get_id` (source bytes):
                //   `avg` × 1, `int` × 4 (the `primitive_type` wrapper at
                //   all four type positions — its `int` keyword child is
                //   suppressed under it), `$a` / `$b` / `$c` × 2 each,
                //   `3` × 1 ⇒ n2 = 6, N2 = 12. Between #1259 and #1293 the
                //   keyword leaf doubled the type count ⇒ 6 / 16; before
                //   #1259 each `$v` also contributed its sigil-less `name`
                //   leaf ⇒ 9 / 22.
                assert_eq!(metric.halstead.unique_operators(), 9);
                assert_eq!(metric.halstead.total_operators(), 12);
                assert_eq!(metric.halstead.unique_operands(), 6);
                assert_eq!(metric.halstead.total_operands(), 12);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn php_simple_function() {
        check_metrics::<PhpParser>(
            "<?php
            function inc(int $x): int { return $x + 1; }",
            "foo.php",
            |metric| {
                // After #695 only opening delimiters count: the `)`/`}`
                // closers no longer add operators (was 9 unique / 9 total).
                //
                // Operands after #1293: `inc` × 1, `int` × 2 (the
                // `primitive_type` wrapper at both type positions, its
                // `int` keyword child suppressed under it), `$x` × 2,
                // `1` × 1 ⇒ n2 = 4, N2 = 6. Between #1259 and #1293 the
                // keyword leaf doubled the type count ⇒ 4 / 8; before
                // #1259 `$x` also contributed its `x` leaf twice ⇒ 5 / 10.
                assert_eq!(metric.halstead.unique_operators(), 7);
                assert_eq!(metric.halstead.total_operators(), 7);
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 6);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn php_variable_reference_counts_once() {
        // Regression: issue #1259. `$x` parses as a `variable_name`
        // wrapping a `name` leaf, and both kinds were in the operand
        // arm — so every variable reference contributed twice to N2 and
        // planted a sigil-less twin (`x` beside `$x`) in the n2
        // vocabulary. Since `$var` is the most common token class in
        // PHP, that roughly doubled N2 for real files.
        //
        // Source: the issue's reproducer plus a re-reference of `$a` and
        // `$b`, so N2 exceeds n2 and the assertions can tell "counted
        // once per occurrence" from "deduplicated into the vocabulary".
        //   <?php $a = null; $b = true; $c = NULL; $a = $b;
        //
        // Operands by text key: `$a` × 2, `$b` × 2, `$c`, `null`, `true`,
        // `NULL` ⇒ n2 = 6, N2 = 8. Before the fix the `a` / `b` / `c`
        // leaves added 3 unique and 5 occurrences ⇒ 9 / 13.
        check_metrics::<PhpParser>(
            "<?php\n$a = null;\n$b = true;\n$c = NULL;\n$a = $b;\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 6);
                assert_eq!(metric.halstead.total_operands(), 8);
            },
        );
    }

    #[test]
    fn php_dynamic_variable_name_counts_once_at_any_depth() {
        // Regression: issue #1259. Variable-variable syntax nests the
        // wrappers, so the double count compounds: `$$a` is a
        // `dynamic_variable_name` → `variable_name` → `name` chain that
        // scored 3 for one reference, and `$$$b` scored 4. Only the
        // outermost wrapper may count.
        //
        // Source: <?php $$a = 1; $$$b = 2; ${$c} = 3; $$a = 4;
        // The trailing re-assignment repeats `$$a` so N2 exceeds n2 and
        // the assertions can tell "counted once per occurrence" from
        // "deduplicated into the vocabulary".
        //
        // Operands: `$$a` × 2, `$$$b`, `${$c}`, `1`, `2`, `3`, `4`
        // ⇒ n2 = 7, N2 = 8. Before the fix: 14 / 17 (measured), each
        // target contributing its whole nesting chain — `$$a` → `$a` →
        // `a` is 3 (twice over), `$$$b` → `$$b` → `$b` → `b` is 4, and
        // `${$c}` → `$c` → `c` is 3, plus the four integers.
        check_metrics::<PhpParser>(
            "<?php $$a = 1; $$$b = 2; ${$c} = 3; $$a = 4;",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 7);
                assert_eq!(metric.halstead.total_operands(), 8);
            },
        );
    }

    #[test]
    fn php_dynamic_variable_name_guard_is_parent_scoped() {
        // Companion to the two tests above (#1259): the guards fire on
        // the *parent* kind, never on the kind alone, so the two
        // positions where a nested node is a reference in its own right
        // keep counting.
        //
        // Source: <?php $y = "brace ${z} end"; $s = ${$a . 'b'};
        //
        // `"${z}"` is a `dynamic_variable_name` whose `name` child is
        // suppressed (the wrapper `${z}` carries the reference), while
        // `${$a . 'b'}` reaches its `$a` through a `binary_expression`,
        // so that `variable_name`'s parent is not a
        // `dynamic_variable_name` and it counts normally.
        //
        // Operands: `$y`, `${z}`, `$s`, `${$a . 'b'}`, `$a`, `'b'` — one
        // each ⇒ n2 = 6, N2 = 6. A guard written as a blanket kind
        // exclusion instead of a parent check would drop `$a` ⇒ 5 / 5.
        check_metrics::<PhpParser>(
            "<?php $y = \"brace ${z} end\"; $s = ${$a . 'b'};",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 6);
                assert_eq!(metric.halstead.total_operands(), 6);
            },
        );
    }

    #[test]
    fn php_type_wrappers_count_the_type_once() {
        // Regression: issue #1293. A parameter type nests wrapper nodes
        // whose text spans the node below them — `primitive_type` around
        // the `int` keyword token, `named_type` around a `name`,
        // `optional_type` around either — and every level was in the
        // operand arm, so `int` scored 2 and `?int` scored 3.
        //
        // Source: the issue's first reproducer.
        //   <?php
        //   function f(int $a, bool $b, float $c, string $d, array $e,
        //              Foo $g): ?int { return 0; }
        //
        // Operands by text key: `f`, the five `primitive_type` parameter
        // types, `Foo` (the `name` under its `named_type`), `$a`..`$g`,
        // the return `int`, and `0`. `int` occurs twice (parameter and
        // return) ⇒ n2 = 14, N2 = 15. Before the fix: 15 / 23 — the
        // extra vocabulary entry being `?int`, which the `?` operator
        // already accounts for.
        check_metrics::<PhpParser>(
            "<?php\nfunction f(int $a, bool $b, float $c, string $d, \
             array $e, Foo $g): ?int { return 0; }\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 14);
                assert_eq!(metric.halstead.total_operands(), 15);
            },
        );
    }

    #[test]
    fn php_qualified_name_counts_its_components_once() {
        // Regression: issue #1293. `Foo\Bar\Baz` parses as
        // `qualified_name` → `namespace_name` → `name` × N, and all
        // three kinds were operands, so one three-part path scored 5 and
        // planted `Foo`, `Foo\Bar` and `Foo\Bar\Baz` in the vocabulary.
        // The components carry the operand and `\` stays an operator,
        // matching how PHP's own `::` and `->` already read here.
        //
        // Source: the issue's second reproducer.
        //   <?php
        //   namespace App\Sub;
        //   use Foo\Bar\Baz;
        //   $o = new \Vendor\Pkg\Thing();
        //
        // Operands: `App`, `Sub`, `Foo`, `Bar`, `Baz`, `$o`, `Vendor`,
        // `Pkg`, `Thing` — one each ⇒ n2 = 9, N2 = 9. Before the fix:
        // 14 / 14.
        check_metrics::<PhpParser>(
            "<?php\nnamespace App\\Sub;\nuse Foo\\Bar\\Baz;\n\
             $o = new \\Vendor\\Pkg\\Thing();\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 9);
                assert_eq!(metric.halstead.total_operands(), 9);
            },
        );
    }

    #[test]
    fn php_nested_type_wrappers_count_once_at_any_depth() {
        // Companion to the two tests above (#1293): the type and
        // qualified-name wrappers compose, so a single annotation can
        // stack five levels — `?A\B` is `optional_type` → `named_type` →
        // `qualified_name` → `namespace_name` → `name`, which scored 6
        // operands for two identifiers. `union_type` and
        // `intersection_type` stack the same way over their members;
        // their `|` and `&` are already operators.
        //
        // Source:
        //   <?php function k(?A\B $p, int|string $q, C&D $r): ?A\B
        //   { return 0; }
        // The return type repeats `?A\B` so N2 exceeds n2 and the
        // assertions can tell "counted once per occurrence" from
        // "deduplicated into the vocabulary".
        //
        // Operands: `k`, `A` × 2, `B` × 2, `$p`, `int`, `string`, `$q`,
        // `C`, `D`, `$r`, `0` ⇒ n2 = 11, N2 = 13.
        check_metrics::<PhpParser>(
            "<?php function k(?A\\B $p, int|string $q, C&D $r): ?A\\B { return 0; }",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 11);
                assert_eq!(metric.halstead.total_operands(), 13);
            },
        );
    }

    #[test]
    fn php_childless_primitive_types_still_count() {
        // Guards the direction of the #1293 fix for `primitive_type`,
        // where — unlike the qualified-name wrappers — the *wrapper*
        // carries the operand and the keyword leaf is suppressed. The
        // grammar emits no token node under `primitive_type` for
        // `callable`, `iterable`, `mixed`, `void`, `false` or `true`
        // (verified with `bca dump`), so the other direction would score
        // those six types zero — grammar-dispatch §6.
        //
        // Source:
        //   <?php function q(callable $a, iterable $b, mixed $c,
        //                    false $d, true $e): void { }
        //
        // Operands: `q`, `callable`, `iterable`, `mixed`, `false`,
        // `true`, `void`, `$a`..`$e` ⇒ n2 = 12, N2 = 12. Dropping
        // `PrimitiveType` from the operand arm instead of gating its
        // leaf gives 6 / 6.
        check_metrics::<PhpParser>(
            "<?php function q(callable $a, iterable $b, mixed $c, \
             false $d, true $e): void { }",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 12);
                assert_eq!(metric.halstead.total_operands(), 12);
            },
        );
    }

    #[test]
    fn php_primitive_type_keyword_guard_is_parent_scoped() {
        // Companion to the test above (#1293): the keyword suppression
        // fires on the *parent* kind, never on the kind alone.
        // `array` is also the head token of an `array(…)` literal, where
        // it is the construct's only operand and must keep counting; a
        // `(int)` / `(string)` cast is a childless `cast_type` that
        // never reaches the guard at all.
        //
        // Source: <?php $x = array(1, 2); $y = (int) $x; $z = (string) $x;
        //
        // Operands: `$x` × 3, `array`, `1`, `2`, `$y`, `int`, `$z`,
        // `string` ⇒ n2 = 8, N2 = 10. A blanket kind exclusion instead
        // of a parent check would drop the `array` head ⇒ 7 / 9.
        check_metrics::<PhpParser>(
            "<?php $x = array(1, 2); $y = (int) $x; $z = (string) $x;",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 8);
                assert_eq!(metric.halstead.total_operands(), 10);
            },
        );
    }

    #[test]
    fn php_encapsed_string_interpolation_no_double_count() {
        // Regression: issue #184. A PHP `"Hello $name!"` used to be
        // classified as a Halstead operand (the wrapping
        // `encapsed_string`) AND have its inner `variable_name`
        // (`$name`) plus the inner `name` token classified as
        // operands too. With the fix, the wrapping literal drops to
        // `Unknown` when it carries any `$var` / `${name}` / `{$expr}`
        // child, so `$name` is counted exactly once at each text
        // occurrence.
        //
        // Source:
        //   <?php $name = "world"; echo "Hello $name!";
        //
        // Inert operand: `"world"` (no interpolation, still operand).
        // Operands by text key (`get_id` keys by source bytes):
        //   `$name` × 2 (assignment LHS and `$name` inside the
        //   interpolated string), `"world"` × 1.
        // u_operands = 2, N2 = 3.
        // Without the #184 fix the wrapping `"Hello $name!"` would also
        // count → 3 / 4. This test additionally pinned the *inner* `name`
        // leaf of each `variable_name` (a further 2 occurrences, 1 unique
        // ⇒ the historical 3 / 5) until #1259 recognised that as the same
        // double count one level down.
        check_metrics::<PhpParser>(
            "<?php $name = \"world\"; echo \"Hello $name!\";",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 2);
                assert_eq!(metric.halstead.total_operands(), 3);
            },
        );
    }

    #[test]
    fn php_encapsed_string_no_interpolation_still_operand() {
        // The fix for #184 only drops `EncapsedString`/`Heredoc` from
        // the operand arm when interpolation is present. An inert
        // double-quoted string must still count as exactly one
        // operand, identical to the single-quoted equivalent.
        //
        // Source: `<?php echo "Hello world!";`
        // Operands: `"Hello world!"` × 1 → u_operands = 1, N2 = 1.
        check_metrics::<PhpParser>("<?php echo \"Hello world!\";", "foo.php", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 1);
            assert_eq!(metric.halstead.total_operands(), 1);
        });
    }

    #[test]
    fn php_heredoc_interpolation_no_double_count() {
        // Regression: issue #184. A PHP heredoc whose body
        // interpolates `$name` previously counted both the wrapping
        // `heredoc` node and the inner `$name` as operands; the fix
        // drops the wrapper when its `heredoc_body` carries any
        // interpolation child.
        //
        // Source:
        //   <?php $name = "x"; echo <<<EOT
        //   hi $name
        //   EOT;
        //
        // Operands by text key: `$name` × 2, `"x"` × 1 (inert encapsed
        // string, still an operand). With the fix u_operands = 2,
        // N2 = 3. Without it the wrapping heredoc text would add one
        // more unique operand. The sigil-less `name` leaf inside each
        // `variable_name` was counted too until #1259.
        check_metrics::<PhpParser>(
            "<?php $name = \"x\"; echo <<<EOT\nhi $name\nEOT;\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 2);
                assert_eq!(metric.halstead.total_operands(), 3);
            },
        );
    }

    #[test]
    fn php_nowdoc_unaffected() {
        // `Nowdoc` (single-quoted heredoc) never interpolates and is
        // never matched by `php_string_has_interpolation`. It must
        // continue counting as exactly one operand regardless of the
        // text inside, mirroring single-quoted `String`.
        //
        // Source:
        //   <?php echo <<<'EOT'
        //   plain $name not interpolated
        //   EOT;
        //
        // Operands: the nowdoc literal × 1 → u_operands = 1, N2 = 1.
        check_metrics::<PhpParser>(
            "<?php echo <<<'EOT'\nplain $name not interpolated\nEOT;\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 1);
                assert_eq!(metric.halstead.total_operands(), 1);
            },
        );
    }

    #[test]
    fn php_encapsed_string_bare_member_access_no_double_count() {
        // Regression: issue #184 follow-up. The PHP grammar allows
        // bare `$obj->prop` interpolation inside `"…"` without
        // surrounding `{ … }`; tree-sitter-php emits this as a
        // direct `member_access_expression` child of
        // `encapsed_string` (kind_id 329 in the current grammar).
        // The wrapper must drop to `Unknown` for that form too —
        // otherwise the inner `$obj` and `prop` `name` tokens are
        // walked as operands while the wrapper also counts,
        // double-counting `N2`.
        //
        // Source:
        //   <?php $obj = new stdClass; $obj->prop = "x"; echo "Hi $obj->prop!";
        //
        // Operands tallied by `get_id` (keyed on source bytes):
        //   `$obj`        × 3 (LHS assignment, member-access target,
        //                      inside the interpolated string)
        //   `prop` (name) × 2 (member-access RHS twice — a bare `name`
        //                      outside any `variable_name`, so #1259's
        //                      guard leaves it an operand)
        //   `stdClass`    × 1
        //   `"x"`         × 1
        // ⇒ u_operands = 4, N2 = 7.
        // With the bug the wrapping `"Hi $obj->prop!"` text adds one
        // more unique operand and one more occurrence ⇒ 5 / 8.
        check_metrics::<PhpParser>(
            "<?php $obj = new stdClass; $obj->prop = \"x\"; echo \"Hi $obj->prop!\";",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 7);
            },
        );
    }

    #[test]
    fn php_encapsed_string_bare_subscript_no_double_count() {
        // Regression: issue #184 follow-up. Bare `$arr[0]` inside
        // `"…"` produces a `subscript_expression` child of
        // `encapsed_string` (kind_id 351). The wrapper must drop to
        // `Unknown` for that form.
        //
        // Source:
        //   <?php $arr = [1]; echo "Hi $arr[0]!";
        //
        // Operands tallied by `get_id`:
        //   `$arr` × 2, `1` × 1, `0` × 1.
        // ⇒ u_operands = 3, N2 = 4.
        // With the bug the wrapping `"Hi $arr[0]!"` text adds 1 / 1.
        // The inner `arr` leaf of each `variable_name` added a further
        // 1 / 2 until #1259.
        check_metrics::<PhpParser>(
            "<?php $arr = [1]; echo \"Hi $arr[0]!\";",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 3);
                assert_eq!(metric.halstead.total_operands(), 4);
            },
        );
    }

    #[test]
    fn php_shell_command_expression_inert_is_operand() {
        // Regression: issue #288. Backtick command literals (PHP's
        // `shell_command_expression`) were filtered as strings by
        // `Checker::is_string` and `Alterator::alterate`, but never
        // classified as Halstead operands — so they contributed
        // nothing to N2 / eta2. An inert backtick literal must now
        // count as exactly one operand, matching `EncapsedString`
        // and `Heredoc`.
        //
        // Source: `<?php $out = ` + backtick `ls` + backtick + `;`
        // Operands tallied by `get_id`:
        //   `$out` × 1, backtick literal × 1.
        // ⇒ u_operands = 2, N2 = 2.
        // Before the fix the backtick literal vanished from the count
        // ⇒ u_operands = 1, N2 = 1. (The inner `out` leaf of the
        // `variable_name` added another 1 / 1 until #1259.)
        check_metrics::<PhpParser>("<?php $out = `ls`;", "foo.php", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });
    }

    #[test]
    fn php_shell_command_expression_interpolation_no_double_count() {
        // Regression: issue #288. PHP backtick literals DO support
        // `$var` interpolation (see tree-sitter-php node-types.json:
        // `shell_command_expression` children include `variable_name`,
        // `dynamic_variable_name`, `member_access_expression`,
        // `subscript_expression`). With the fix the wrapper drops to
        // `Unknown` when it carries any interpolation child, exactly
        // as `EncapsedString` does.
        //
        // Source: `<?php $dir = "/tmp"; $out = ` + backtick `ls $dir` +
        //   backtick + `;`
        //
        // Operands tallied by `get_id`:
        //   `$dir` × 2 (assignment LHS, inside backticks),
        //   `$out` × 1, `"/tmp"` × 1.
        // ⇒ u_operands = 3, N2 = 4.
        // Without the interpolation guard the wrapping backtick literal
        // would also count ⇒ u_operands = 4, N2 = 5. The sigil-less
        // `dir` / `out` leaves added a further 2 / 3 until #1259.
        check_metrics::<PhpParser>(
            "<?php $dir = \"/tmp\"; $out = `ls $dir`;",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operands(), 3);
                assert_eq!(metric.halstead.total_operands(), 4);
            },
        );
    }

    #[test]
    fn php_interpolation_opener_is_not_an_operator() {
        // Regression: issue #1314. `Php::LBRACE` is *both* the
        // compound-statement brace and the complex-interpolation
        // opener, so `"dq {$y} end"` reported a `{}` operator — and
        // reported it against the same vocabulary entry a real block
        // uses, which no other language does.
        //
        // expected: operators `=` × 2, `;` × 2 → n1 = 2, N1 = 4.
        // Operands `$s`, `$t`, `$y` × 2 → n2 = 3, N2 = 4. Before the
        // guard the two openers added `{}` → n1 = 3, N1 = 6.
        check_metrics::<PhpParser>(
            "<?php\n$s = \"dq {$y} end\";\n$t = \"dq {$y} end\";\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 2);
                assert_eq!(metric.halstead.total_operators(), 4);
                assert_eq!(metric.halstead.unique_operands(), 3);
                assert_eq!(metric.halstead.total_operands(), 4);
            },
        );
    }

    #[test]
    fn php_interpolation_opener_guard_covers_every_wrapper() {
        // The opener is a direct child of four distinct parents, and
        // each is an independent leg of the guard (grammar-dispatch
        // section 11) — a fixture covering only `encapsed_string`
        // leaves the other three dead.
        //
        // One row per parent, so a failure names the leg that broke.
        // The heredoc's brace hangs off `heredoc_body` rather than
        // `heredoc`; the backtick form is `shell_command_expression`;
        // and the bare `${$y}` variable-variable is a
        // `dynamic_variable_name`, the one position that is not inside
        // a string at all.
        //
        // expected per row: operators `=` × 2, `;` × 2 → n1 = 2,
        // N1 = 4; three distinct operands with one repeated → n2 = 3,
        // N2 = 4.
        for (label, source) in [
            (
                "encapsed_string",
                "<?php\n$s = \"dq {$y} end\";\n$t = \"dq {$y} end\";\n",
            ),
            (
                "heredoc_body",
                "<?php\n$h = <<<EOT\na {$y} b\nEOT;\n$i = <<<EOT\na {$y} b\nEOT;\n",
            ),
            (
                "shell_command_expression",
                "<?php\n$b = `ls {$y}`;\n$c = `ls {$y}`;\n",
            ),
            ("dynamic_variable_name", "<?php\n$q = ${$y};\n$r = ${$y};\n"),
        ] {
            assert_halstead_counts::<PhpParser>(source, "foo.php", [2, 4, 3, 4], label);
        }
    }

    #[test]
    fn php_every_interpolation_spelling_scores_alike() {
        // The policy stated as a test (#1314). PHP writes one
        // interpolation three ways; the choice is spelling, so all
        // three must score identically. Before the guard the two
        // braced forms reported a `{}` the bare `$y` form did not.
        //
        // `"${y}"` is deprecated as of PHP 8.2 and removed in 9.0, but
        // the pinned grammar still parses it and it is still in the
        // wild, so it stays a row here.
        //
        // The fixture deliberately omits a `$y = …` declaration: with
        // one, the bare and `{$y}` forms key their operand as `$y` and
        // collapse into the declaration's entry while `${y}` keys as
        // `${y}` and does not, so n2 would differ for a reason that has
        // nothing to do with this guard.
        //
        // expected per spelling: operators `=` × 2, `;` × 2 → n1 = 2,
        // N1 = 4; operands `$s`, `$t`, the interpolated reference × 2
        // → n2 = 3, N2 = 4.
        for literal in ["\"a $y b\"", "\"a {$y} b\"", "\"a ${y} b\""] {
            assert_halstead_counts::<PhpParser>(
                &format!("<?php\n$s = {literal};\n$t = {literal};\n"),
                "foo.php",
                [2, 4, 3, 4],
                &format!("interpolation {literal}"),
            );
        }
    }

    #[test]
    fn php_compound_statement_brace_still_counts() {
        // Control for #1314: the guard is scoped to the four
        // interpolating wrappers, so a real block keeps its `{}`. A
        // guard widened to every `LBRACE` would take `{}` out of the
        // operator set entirely and fail here.
        //
        // expected: operators `function`, `()`, `{}` × 2, `if`,
        // `return`, `;` → n1 = 6, N1 = 8. Operands `f`, `1`, `2` →
        // n2 = N2 = 3.
        check_metrics::<PhpParser>(
            "<?php\nfunction f() { if (1) { return 2; } }\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 6);
                assert_eq!(metric.halstead.total_operators(), 8);
                assert_eq!(metric.halstead.unique_operands(), 3);
                assert_eq!(metric.halstead.total_operands(), 3);
            },
        );
    }

    #[test]
    fn php_interpolation_guard_is_parent_scoped_not_ancestor_scoped() {
        // The input that separates the parent-scoped guard from the
        // ancestor-scanning mutant — the mutant #1256's post-mortem
        // says survives every ordinary fixture. PHP is one of only two
        // languages in #1314 where such an input exists: a closure
        // inside a complex interpolation puts a *compound-statement*
        // brace under an `encapsed_string` ancestor while its parent is
        // the `compound_statement`. An ancestor scan swallows it.
        //
        // expected: operators `=`, `;` × 2, `->`, `()` × 2, `function`,
        // `{}`, `return` → n1 = 7, N1 = 9. Operands `$s`, `$o`, `m`,
        // `1` → n2 = N2 = 4. Under the ancestor-scoped mutant the
        // closure's brace vanishes: n1 = 6, N1 = 8.
        check_metrics::<PhpParser>(
            "<?php\n$s = \"{$o->m(function() { return 1; })}\";\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 7);
                assert_eq!(metric.halstead.total_operators(), 9);
                assert_eq!(metric.halstead.unique_operands(), 4);
                assert_eq!(metric.halstead.total_operands(), 4);
            },
        );
    }

    #[test]
    fn elixir_operators_and_operands() {
        // Exercises every Halstead family classified in Elixir's
        // `get_op_type`: control-flow keywords (`do`, `end`, `fn`),
        // structural punctuation — only the *opening* delimiters `(`,
        // `[` count after #695 (the `)`/`]` closers were dropped), plus
        // `,`, `.`, `@`,
        // arithmetic (`+`, `-`, `*`, `/`), comparison (`==`, `>`),
        // logical (`&&`, `||`, `and`, `or`, `!`), pipe (`|>`), capture
        // (`&`), assignment/match (`=`), and the stab arrow (`->`).
        // The body mixes identifiers, integers, atoms, and a string.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  @doc \"add\"\n  def calc(a, b) do\n    result = a + b * 2\n    flag = result > 0 && a == b\n    out = if flag, do: result, else: -result\n    [out, a, b]\n  end\nend\n",
            "foo.ex",
            |metric| {
                // Positive headline assertions on integer counts. After
                // #695 only opening delimiters count: the `)`/`]` closers
                // no longer add operators (was 15 unique / 23 total).
                assert_eq!(metric.halstead.unique_operators(), 13);
                assert_eq!(metric.halstead.total_operators(), 21);
                assert_eq!(metric.halstead.unique_operands(), 16);
                assert_eq!(metric.halstead.total_operands(), 27);
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "unique_operators": 13,
                  "total_operators": 21,
                  "unique_operands": 16,
                  "total_operands": 27,
                  "length": 48,
                  "estimated_program_length": 112.10571633583419,
                  "purity_ratio": 2.3355357569965456,
                  "vocabulary": 29,
                  "volume": 233.18308776612344,
                  "difficulty": 10.96875,
                  "level": 0.09116809116809117,
                  "effort": 2557.7269939346666,
                  "time": 142.09594410748147,
                  "bugs": 0.062342115670886794
                }
                "#
                );
            },
        );
    }

    #[test]
    fn ruby_operators_and_operands() {
        // A small Ruby method exercising operators (def/if/end keyword
        // tokens, `+`, `==`, `<=`, structural punctuation) and operands
        // (`n`, `1`, `factorial`). Anchors the unique/total counts on
        // both sides and snapshots the full Halstead derivation.
        //
        // Lesson 4 invariants: u_operators / u_operands here equal the
        // dedupe lengths the `--ops` accessor would emit on the same
        // source. Any future grammar bump that adds an aliased kind_id
        // to either side will trip this without snapshot drift.
        check_metrics::<RubyParser>(
            "def factorial(n)\n  return 1 if n <= 1\n  n * factorial(n - 1)\nend\n",
            "foo.rb",
            |metric| {
                // After #695 only the `(` opener counts (folded `()`); the
                // `)` closer — which appeared twice across the two calls —
                // no longer adds an operator (was 9 unique / 11 total).
                assert_eq!(metric.halstead.unique_operators(), 8);
                assert_eq!(metric.halstead.total_operators(), 9);
                assert_eq!(metric.halstead.unique_operands(), 3);
                assert_eq!(metric.halstead.total_operands(), 9);
                insta::assert_json_snapshot!(metric.halstead);
            },
        );
    }

    #[test]
    fn ruby_halstead_plain_string_operand() {
        // A bare string literal contributes exactly one operand. The
        // counterpart to `ruby_halstead_interpolated_string_no_double_count`
        // — verifies the "no interpolation" branch of the same arm
        // (see `src/getter.rs::get_op_type`'s `R::String | …` case).
        // expected: operators = {def, end} = 2; operands = {f, "hello"} = 2.
        check_metrics::<RubyParser>("def f\n  \"hello\"\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 2);
            assert_eq!(metric.halstead.total_operators(), 2);
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });
    }

    #[test]
    fn ruby_halstead_interpolated_string_no_double_count() {
        // Regression mirror for #180 (Bash) / #183 (C#): when a Ruby
        // string literal carries an `Interpolation` child, the
        // wrapping `String` node is intentionally classified as
        // `Unknown` so the inner expression's identifiers are not
        // double-counted as operands.
        //
        // expected: for `def f(name)\n  "Hi #{name}"\nend\n` —
        //   operators: def, (, ), #{, }, end → u_operators = 6.
        //   operands: f, name (param), name (inside `#{name}`). The
        //   wrapping `"…#{name}"` literal is skipped by the
        //   `is_child(R::Interpolation)` guard; the operand store
        //   keys by token text so the two `name` occurrences dedupe
        //   into one distinct entry → u_operands = 2, operands = 3
        //   (`f` once, `name` twice).
        // Without the guard, the wrapping literal would also count,
        // inflating u_operands to 3 and operands to 4.
        check_metrics::<RubyParser>("def f(name)\n  \"Hi #{name}\"\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 3);
        });
    }

    #[test]
    fn ruby_halstead_symbol_literal_operand() {
        // `:foo` is a `SimpleSymbol` leaf — counts as a single
        // operand, no interpolation guard needed (only
        // `DelimitedSymbol` (`:"…#{x}…"`) can interpolate).
        // expected: operators = {def, end} = 2; operands = {f, :ok} = 2.
        check_metrics::<RubyParser>("def f\n  :ok\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 2);
            assert_eq!(metric.halstead.unique_operands(), 2);
        });
    }

    #[test]
    fn ruby_halstead_regex_operand() {
        // `/foo/` parses as a `Regex` node — one operand. Its two
        // `SLASH` delimiters used to fall through to the shared
        // arithmetic arm and add a `/` operator that is nowhere in the
        // source; #1312 parent-guards them to `Unknown`.
        // expected: u_operators = {def, (, =~, end} = 4, N1 = 4 (only
        // the `(` opener counts after #695 — the `)` closer was
        // dropped; was 5 with the fabricated `/`); u_operands =
        // {f, s, /foo/} = 3, N2 = 4 (`s` twice: parameter and use).
        check_metrics::<RubyParser>("def f(s)\n  s =~ /foo/\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 4);
            assert_eq!(metric.halstead.total_operators(), 4);
            assert_eq!(metric.halstead.unique_operands(), 3);
            assert_eq!(metric.halstead.total_operands(), 4);
        });
    }

    #[test]
    fn ruby_regex_delimiters_are_not_operators() {
        // Regression: issue #1312, the Ruby sibling of Elixir #1256.
        // Both of a `Regex` literal's delimiter tokens are `SLASH` —
        // the same kind id real division uses — so `x = /abc/`
        // reported a `/` operator with no division in the source.
        //
        // expected: operators `=` → n1 = N1 = 1. Operands `x` and the
        // `/abc/` literal → n2 = N2 = 2. Before the guard the two
        // delimiters added `/` → n1 = 2, N1 = 3.
        check_metrics::<RubyParser>("x = /abc/\n", "foo.rb", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 1);
            assert_eq!(metric.halstead.total_operators(), 1);
            assert_eq!(metric.halstead.unique_operands(), 2);
            assert_eq!(metric.halstead.total_operands(), 2);
        });
    }

    #[test]
    fn ruby_regex_delimiter_choice_is_invariant() {
        // Companion to the test above (#1312): `%r`-form regexes are
        // the same literal spelled differently, so every delimiter
        // choice must produce identical counts. tree-sitter-ruby
        // aliases all of them to `SLASH` — verified with `bca dump`,
        // which shows `%r{`/`}`, `%r(`/`)`, `%r[`/`]`, `%r<`/`>`,
        // `%r|`/`|` and `%r!`/`!` every one emitting kind `SLASH` —
        // so each row here genuinely exercises the guard rather than
        // reaching a different, already-clean path.
        //
        // expected per variant: operator `=` → n1 = N1 = 1; operands
        // `x` and the literal → n2 = N2 = 2.
        for literal in [
            "/abc/", "%r{abc}", "%r(abc)", "%r[abc]", "%r<abc>", "%r|abc|", "%r!abc!",
        ] {
            assert_halstead_counts::<RubyParser>(
                &format!("x = {literal}\n"),
                "foo.rb",
                [1, 1, 2, 2],
                &format!("regex literal {literal}"),
            );
        }
    }

    #[test]
    fn ruby_division_survives_the_regex_guard() {
        // Control for #1312: the guard is scoped to a `Regex` parent,
        // so real division still counts. Two divisions, so a mutant
        // that collapsed repeated hits would move `N1` even though
        // `n1` held (the #1294 count-only-anchor lesson).
        //
        // expected: operators `=` and `/` × 2 → n1 = 2, N1 = 3.
        // Operands `z`, `a`, `b`, `c` → n2 = N2 = 4.
        check_metrics::<RubyParser>("z = a / b / c\n", "foo.rb", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 2);
            assert_eq!(metric.halstead.total_operators(), 3);
            assert_eq!(metric.halstead.unique_operands(), 4);
            assert_eq!(metric.halstead.total_operands(), 4);
        });
    }

    #[test]
    fn ruby_regex_guard_is_parent_scoped_not_ancestor_scoped() {
        // The one input that separates the correct parent-scoped guard
        // from the ancestor-scoped mutant of it (#1312, mirroring
        // #1256's Elixir case): a division *inside* a regex's `#{…}`
        // interpolation. Its `/` has `Binary` as its parent but the
        // `Regex` as a further ancestor, so an ancestor scan would
        // swallow it. Every other fixture in this file passes under
        // both spellings. Two interpolations, so the mutant moves both
        // n1 (2 → 1) and N1 (3 → 1).
        //
        // expected: operators `=`, `/` × 2 → n1 = 2, N1 = 3. Operands
        // `w`, `p`, `q`, `r`, `t` → n2 = N2 = 5; the wrapping `Regex`
        // is skipped because it carries an `Interpolation` child (the
        // #180 double-count guard).
        //
        // Was n1 = 3, N1 = 5 until #1314 dropped `#{` from the operator
        // arm. The mutant still moves both axes, so this fixture is as
        // discriminating as it was — it just no longer counts the two
        // interpolation openers alongside the two divisions.
        check_metrics::<RubyParser>("w = /a#{p / q}c#{r / t}b/\n", "foo.rb", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 2);
            assert_eq!(metric.halstead.total_operators(), 3);
            assert_eq!(metric.halstead.unique_operands(), 5);
            assert_eq!(metric.halstead.total_operands(), 5);
        });
    }

    #[test]
    fn ruby_regex_start_alias_never_reaches_kind_id() {
        // Drift marker for the `R::SLASH2` half of #1312's guard.
        // `SLASH2` is the aliased regex-start token: it sits in the
        // enum beside the other literal-start aliases (`DQUOTE`,
        // `COLONDQUOTE`, `BQUOTE2`, `PERCENTwLPAREN`) and the runtime
        // `public_symbol_map` collapses it to `SLASH` before
        // `kind_id()`, exactly like `LPAREN2` in #768. It is listed in
        // the guard rather than the arithmetic arm because a regex
        // delimiter is the only thing it could ever be; this pins that
        // it is currently unreachable, so a grammar bump that starts
        // emitting it fails here instead of silently changing a metric.
        let path = PathBuf::from("foo.rb");
        for source in ["x = /abc/\n", "x = %r{abc}\n"] {
            let parser = RubyParser::new(source.as_bytes().to_vec(), &path, None);
            assert!(
                !ast_has_kind_id(&parser, Ruby::SLASH2 as u16),
                "Ruby::SLASH2 must stay collapsed to Ruby::SLASH for `{source}`"
            );
            // Positive control: the id the guard actually fires on is
            // present, so the assertion above cannot pass merely
            // because no delimiter was parsed at all.
            assert!(
                ast_has_kind_id(&parser, Ruby::SLASH as u16),
                "Ruby::SLASH must be the delimiter kind for `{source}`"
            );
        }
    }

    #[test]
    fn ruby_interpolation_opener_is_not_an_operator() {
        // Behaviour change, not a fabrication fix: #1314 drops
        // `HASHLBRACE` from Ruby's operator arm. `#{` is a token of its
        // own here — unlike PHP's `{`, which aliases the
        // compound-statement brace — so nothing was being miscounted;
        // the question was whether an interpolation opener is an
        // operation at all, and across the five interpolating languages
        // three already said no. Ruby Halstead operator counts drop for
        // interpolated literals as a result.
        //
        // Asserted as an invariance: the interpolated and plain
        // spellings of one string must now score identically, which is
        // the policy rather than a magic number. Before the change the
        // interpolated row was n1 = 2, N1 = 3.
        //
        // expected per row: operator `=` × 2 → n1 = 1, N1 = 2; operands
        // `s`, `t`, and the literal's content contribution × 2 →
        // n2 = 3, N2 = 4.
        for literal in ["\"a #{y} b\"", "\"a b\""] {
            assert_halstead_counts::<RubyParser>(
                &format!("s = {literal}\nt = {literal}\n"),
                "foo.rb",
                [1, 2, 3, 4],
                &format!("literal {literal}"),
            );
        }
    }

    #[test]
    fn ruby_interpolation_opener_drop_covers_every_literal() {
        // `HASHLBRACE` is one arm, but it fires under every Ruby
        // literal that interpolates, so the drop is not specific to
        // double-quoted strings. A symbol and a regex — two literals
        // whose `#{…}` reaches the same token — must contribute no
        // operator for the opener either.
        //
        // expected: operator `=` × 3 → n1 = 1, N1 = 3. Operands `y`,
        // `1`, `a`, `b`, plus the two interpolated `y` references →
        // n2 = 4, N2 = 6. Before the change each `#{` added one →
        // n1 = 2, N1 = 5.
        check_metrics::<RubyParser>("y = 1\na = :\"s#{y}\"\nb = /r#{y}/\n", "foo.rb", |metric| {
            assert_eq!(metric.halstead.unique_operators(), 1);
            assert_eq!(metric.halstead.total_operators(), 3);
            assert_eq!(metric.halstead.unique_operands(), 4);
            assert_eq!(metric.halstead.total_operands(), 6);
        });
    }

    /// Comprehensive iRules Halstead test exercising every operator family
    /// classified in `get_op_type`: declaration/control keywords (`proc`,
    /// `set`, `if`, `return`), structural punctuation (`{}` `[]` `()`),
    /// arithmetic (`+`), comparison (`>`), the word-form string comparator
    /// (`eq`), and short-circuit logical (`&&`). Anchored on the integer
    /// `n1`/`N1`/`n2`/`N2` headline values; the float fields are derived and
    /// bit-brittle, so they are not pinned.
    ///
    /// The second half pins the lesson-4 invariant: the independent
    /// text-keyed `operands_and_operators` store must dedupe to the same
    /// `n1`/`n2`. A classification change that moved one store without the
    /// other (e.g. a kind landing in both the operator and operand arms)
    /// would break this even though the snapshot stayed green.
    #[test]
    fn irules_operators_and_operands() {
        let source = "proc f { a b } {
    set x [expr { $a + $b }]
    if { $x > 0 && $a eq \"go\" } {
        return $x
    }
    return 0
}
";
        check_metrics::<IrulesParser>(source, "foo.irule", |metric| {
            // After #695 only opening delimiters count: the `}`/`]`
            // closers no longer add operators (was 12 unique / 20 total).
            assert_eq!(metric.halstead.unique_operators(), 10);
            assert_eq!(metric.halstead.total_operators(), 14);
            assert_eq!(metric.halstead.unique_operands(), 12);
            assert_eq!(metric.halstead.total_operands(), 16);
        });

        let path = PathBuf::from("foo.irule");
        let parser = IrulesParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
        let unique_operators: HashSet<&str> = ops.operators.iter().map(String::as_str).collect();
        let unique_operands: HashSet<&str> = ops.operands.iter().map(String::as_str).collect();
        assert_eq!(
            unique_operators.len(),
            10,
            "dedupe(ops.operators) must equal n1; operators were {:?}",
            ops.operators
        );
        assert_eq!(
            unique_operands.len(),
            12,
            "dedupe(ops.operands) must equal n2; operands were {:?}",
            ops.operands
        );
    }

    /// An inert `"hello world"` double-quoted string (no `$var` / `[cmd]`
    /// interpolation child) contributes exactly **one** operand — the
    /// wrapping `QuotedWord`. Operands are `f`, `s`, `"hello world"`, and
    /// the proc-body `braced_word` (counted as an operand in the Tcl
    /// family) — n2=4, the same as Tcl since #1294 restored its `set`
    /// target to the operand count. Mirrors
    /// `tcl_inert_quoted_word_counts_as_operand` (#277).
    #[test]
    fn irules_inert_quoted_word_counts_as_operand() {
        let source = "proc f {} {\n    set s \"hello world\"\n}\n";
        check_metrics::<IrulesParser>(source, "foo.irule", |metric| {
            // After #695 only the `{` opener counts; the `}` closer no
            // longer adds an operator (was 4 unique / 6 total).
            assert_eq!(metric.halstead.unique_operators(), 3);
            assert_eq!(metric.halstead.total_operators(), 4);
            assert_eq!(metric.halstead.unique_operands(), 4);
            assert_eq!(metric.halstead.total_operands(), 4);
        });

        let path = PathBuf::from("foo.irule");
        let parser = IrulesParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
        // The inert quoted word is present as exactly one operand (not
        // dropped, not split): dropping it would mean the inert branch was
        // over-guarded.
        let quoted = ops
            .operands
            .iter()
            .filter(|o| o.as_str() == "\"hello world\"")
            .count();
        assert_eq!(quoted, 1, "inert quoted word must be one operand");
        let unique_operands: HashSet<&str> = ops.operands.iter().map(String::as_str).collect();
        assert_eq!(unique_operands.len(), 4, "operands were {:?}", ops.operands);
    }

    /// Regression for the `QuotedWord` interpolation guard (the #277 /
    /// Bash-#180 / C#-#183 / PHP-#184 pattern). An interpolated
    /// `"$x is $y"` must contribute **zero** operands for the wrapping
    /// `QuotedWord`; the inner `$x` / `$y` `variable_substitution` nodes are
    /// walked separately and count on their own. Operands are `f`, `x`, `y`,
    /// `s`, `$x`, `$y`, and the proc-body `braced_word` = 7. If the guard
    /// regressed (wrapper classified `Operand`), the wrapper string would
    /// add an 8th operand. This is the branch that had no test before.
    #[test]
    fn irules_interpolated_quoted_word_no_double_count() {
        let source = "proc f {x y} {\n    set s \"$x is $y\"\n}\n";
        check_metrics::<IrulesParser>(source, "foo.irule", |metric| {
            // After #695 only the `{` opener counts; the `}` closer no
            // longer adds an operator (was 4 unique / 6 total).
            assert_eq!(metric.halstead.unique_operators(), 3);
            assert_eq!(metric.halstead.total_operators(), 4);
            assert_eq!(metric.halstead.unique_operands(), 7);
            assert_eq!(metric.halstead.total_operands(), 7);
        });

        let path = PathBuf::from("foo.irule");
        let parser = IrulesParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
        // The wrapping interpolated string must NOT appear as an operand;
        // its inner substitutions must. The wrapper, if wrongly counted,
        // would surface as the quoted literal `"$x is $y"` (with quotes,
        // like the inert `"hello world"` operand). Match that exact token —
        // a substring check would false-match the proc-body `braced_word`
        // operand, which legitimately contains the source text.
        assert!(
            !ops.operands.iter().any(|o| o.as_str() == "\"$x is $y\""),
            "interpolated wrapper must not be an operand; operands were {:?}",
            ops.operands
        );
        assert!(
            ops.operands.iter().any(|o| o.as_str() == "$x")
                && ops.operands.iter().any(|o| o.as_str() == "$y"),
            "inner $x / $y substitutions must each be operands; operands were {:?}",
            ops.operands
        );
        let unique_operands: HashSet<&str> = ops.operands.iter().map(String::as_str).collect();
        assert_eq!(unique_operands.len(), 7, "operands were {:?}", ops.operands);
    }

    /// Exercises the operator families not covered by
    /// `irules_operators_and_operands`: bitwise (`& | ^ ~ << >>`), ternary
    /// (`? :`), the keyword string comparators (`starts_with`, `ends_with`,
    /// `contains`, `matches`, `eq`, `ne`), and the keyword logical operator
    /// (`and`). Pins every operator-family arm in `get_op_type` plus the
    /// lesson-4 dedupe invariant.
    #[test]
    fn irules_bitwise_ternary_string_ops() {
        let source = "proc f { a b } {
    set bits [expr { $a & $b | $a ^ ~$b }]
    set sh [expr { $a << 2 | $b >> 1 }]
    set t [expr { $a > 0 ? $a : $b }]
    if { $a starts_with \"x\" && $b ends_with \"y\" } { return 1 }
    if { $a contains \"z\" || $b matches \"q\" } { return 2 }
    if { $a eq \"m\" and $b ne \"n\" } { return 3 }
    return $b
}
";
        check_metrics::<IrulesParser>(source, "foo.irule", |metric| {
            // After #695 only opening delimiters count: the `}`/`]`
            // closers no longer add operators (was 26 unique / 57 total).
            assert_eq!(metric.halstead.unique_operators(), 24);
            assert_eq!(metric.halstead.total_operators(), 43);
            assert_eq!(metric.halstead.unique_operands(), 23);
            assert_eq!(metric.halstead.total_operands(), 42);
        });

        let path = PathBuf::from("foo.irule");
        let parser = IrulesParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
        let unique_operators: HashSet<&str> = ops.operators.iter().map(String::as_str).collect();
        let unique_operands: HashSet<&str> = ops.operands.iter().map(String::as_str).collect();
        assert_eq!(
            unique_operators.len(),
            24,
            "dedupe(ops.operators) must equal n1; operators were {:?}",
            ops.operators
        );
        assert_eq!(
            unique_operands.len(),
            23,
            "dedupe(ops.operands) must equal n2; operands were {:?}",
            ops.operands
        );
    }

    /// A bare `$x` produces one `variable_substitution` operand. Its inner
    /// `id` leaf (the *named* `Id` node — not the anonymous `Id2` token Tcl
    /// has there) must NOT be counted separately, or every variable
    /// reference double-counts. `get_op_type` excludes `Id` whose parent is
    /// a `VariableSubstitution`. Operands: `f`, the proc arg `x`, `return`,
    /// `$x`, and the proc-body `braced_word` — five, with no duplicate
    /// (`total_operands()` == 5). If the guard regressed, the inner `id` "x"
    /// would add a sixth operand occurrence (it text-collides with the proc
    /// arg `x`, so `u_operands` would stay 5 but `total_operands()` would rise
    /// to 6 — hence the total, not just the unique count, is asserted).
    #[test]
    fn irules_bare_variable_operand() {
        let source = "proc f {x} {\n    return $x\n}\n";
        check_metrics::<IrulesParser>(source, "foo.irule", |metric| {
            // After #695 only the `{` opener counts (folded `{}`); the
            // `}` closer no longer adds an operator (was 3 unique / 5 total).
            assert_eq!(metric.halstead.unique_operators(), 2);
            assert_eq!(metric.halstead.total_operators(), 3);
            assert_eq!(metric.halstead.unique_operands(), 5);
            assert_eq!(metric.halstead.total_operands(), 5);
        });

        let path = PathBuf::from("foo.irule");
        let parser = IrulesParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::ops::ops_inner(&parser, None).expect("ops walk succeeds");
        let bare_var = ops.operands.iter().filter(|o| o.as_str() == "$x").count();
        assert_eq!(
            bare_var, 1,
            "bare $x must be exactly one operand (inner id leaf not double-counted); operands were {:?}",
            ops.operands
        );
    }

    #[test]
    fn irules_braced_word_delimiter_is_not_an_operator() {
        // Regression: issue #1314. The iRules twin of
        // `tcl_braced_word_delimiter_is_not_an_operator` — the two
        // getters are deliberate clones, so the guard lands in both and
        // is asserted in both.
        //
        // One fixture covers the guard and its control: `{braced word}`
        // and `{y}` are values whose openers must not count, while the
        // handler body, the `if` condition and the `if` body are
        // `BracedWord` / `Expr` and must keep theirs. A guard keyed on
        // the brace alone would take `{}` out of the operator set
        // entirely and fail here.
        //
        // expected: operators `when`, `set` × 2, `if`, `contains`,
        // `[]`, `{}` × 3 → n1 = 6, N1 = 9. Every operand is distinct →
        // n2 = N2 = 12. Before the guard the two value openers added
        // two more `{}` occurrences → N1 = 11.
        check_metrics::<IrulesParser>(
            "when HTTP_REQUEST {\n  set a {braced word}\n  if {[HTTP::uri] contains \"x\"} { set b {y} }\n}\n",
            "foo.irule",
            |metric| {
                assert_eq!(metric.halstead.unique_operators(), 6);
                assert_eq!(metric.halstead.total_operators(), 9);
                assert_eq!(metric.halstead.unique_operands(), 12);
                assert_eq!(metric.halstead.total_operands(), 12);
            },
        );
    }

    /// Regression for #563: the two Halstead `Display` labels must use the
    /// underscore key that matches the JSON/CSV field name, so a user can grep
    /// the same token across `Display` and JSON. The space-separated forms
    /// (`estimated program length` / `purity ratio`) were the only outliers,
    /// mirroring the `dump` fix in #562.
    #[test]
    fn display_halstead_labels_use_underscore_keys() {
        check_metrics::<CppParser>("int a = 42;", "foo.cpp", |metric| {
            let out = metric.halstead.to_string();
            assert!(
                out.contains("estimated_program_length: "),
                "Display must use the underscore key `estimated_program_length`:\n{out}"
            );
            assert!(
                out.contains("purity_ratio: "),
                "Display must use the underscore key `purity_ratio`:\n{out}"
            );
            assert!(
                !out.contains("estimated program length"),
                "Display must not emit the space-separated `estimated program length`:\n{out}"
            );
            assert!(
                !out.contains("purity ratio"),
                "Display must not emit the space-separated `purity ratio`:\n{out}"
            );
        });
    }

    /// Comprehensive Objective-C Halstead fixture exercising a message
    /// send (`[self log:@"hi"]`), an ObjC string literal (`@"hi"`), an
    /// `if`, a short-circuit `&&`, arithmetic (`+`), comparisons, and
    /// assignment. Pins every field and enforces the lesson-4 invariants
    /// `unique_operators == n1` / `unique_operands == n2` via the
    /// independent `--ops` store.
    #[test]
    fn objc_operators_and_operands() {
        let source = "@implementation Foo
- (int)bar:(int)x {
    int y = x + 1;
    if (x > 0 && y < 10) {
        [self log:@\"hi\"];
    }
    return y;
}
@end
";
        check_metrics::<ObjcParser>(source, "foo.m", |metric| {
            // n1 = 15 unique operators:
            //   `&&`, `()`, `+`, `-`, `:`, `;`, `<`, `=`, `>`, `@`,
            //   `[]` (message send), `if`, `int`, `return`, `{}`.
            // n2 = 10 unique operands:
            //   `Foo`, `bar`, `log`, `self`, `x`, `y`, `0`, `1`, `10`,
            //   `@"hi"` (the ObjC string literal).
            assert_eq!(metric.halstead.unique_operators(), 15);
            assert_eq!(metric.halstead.unique_operands(), 10);
            insta::assert_json_snapshot!(metric.halstead, @r#"
            {
              "unique_operators": 15,
              "total_operators": 23,
              "unique_operands": 10,
              "total_operands": 14,
              "length": 37,
              "estimated_program_length": 91.82263988300141,
              "purity_ratio": 2.481692969810849,
              "vocabulary": 25,
              "volume": 171.8226790216648,
              "difficulty": 10.5,
              "level": 0.09523809523809523,
              "effort": 1804.1381297274804,
              "time": 100.22989609597113,
              "bugs": 0.049399808887691035
            }
            "#);
        });
        // Lesson-4 invariant: dedupe(ops.operands) == n2 (10), via the
        // independent text-keyed `--ops` store.
        assert_ops_operands::<ObjcParser>(
            source,
            "foo.m",
            10,
            vec![
                "Foo", "bar", "log", "self", "x", "y", "0", "1", "10", "@\"hi\"",
            ],
        );
    }

    /// Builds a `HalsteadMaps` from explicit occurrence counts.
    ///
    /// The per-language tests above reach these maps only through a
    /// parse, which cannot produce a *chosen* overlap between a child
    /// and its parent — the cases `merge` exists to get right.
    fn halstead_maps_of<'a>(
        operators: &[(u16, u64)],
        primitive_operators: &[(&'a [u8], u64)],
        operands: &[(&'a [u8], u64)],
    ) -> HalsteadMaps<'a> {
        HalsteadMaps {
            operators: operators.iter().copied().collect(),
            primitive_operators: primitive_operators.iter().copied().collect(),
            operands: operands.iter().copied().collect(),
        }
    }

    /// `HalsteadMaps::operators` must stay on the crate's integer hasher.
    ///
    /// Swapping a hasher moves no metric value, so every other test in
    /// this file passes just as well with #1108 reverted. Both halves
    /// here are needed: the typed binding stops compiling if the field
    /// goes back to a default-hasher `HashMap`, and the `type_name`
    /// comparison still fails at runtime if `IntKeyHashMap` itself is
    /// ever redefined to wrap `RandomState`.
    ///
    /// The two text-keyed maps are pinned to SipHash in the same test,
    /// because moving *them* would be a regression rather than an
    /// optimisation. `crate::int_hash`'s module doc is the single place
    /// that argues why analysed source text does not qualify.
    #[test]
    fn halstead_operator_map_uses_the_int_key_hasher() {
        use std::any::{type_name, type_name_of_val};
        use std::hash::BuildHasherDefault;

        use crate::int_hash::IntKeyHasher;

        let maps = HalsteadMaps::new();

        let operators: &IntKeyHashMap<u16, u64> = &maps.operators;
        assert_eq!(
            type_name_of_val(operators.hasher()),
            type_name::<BuildHasherDefault<IntKeyHasher>>(),
            "the kind_id-keyed operator map must use the int_hash hasher"
        );

        let siphash = type_name::<std::collections::hash_map::RandomState>();
        assert_eq!(
            type_name_of_val(maps.operands.hasher()),
            siphash,
            "operand keys come from the analysed source, so the keyed hash \
             is what stops a crafted file from flooding this map"
        );
        assert_eq!(
            type_name_of_val(maps.primitive_operators.hasher()),
            siphash,
            "primitive-operator keys come from the analysed source, so the \
             keyed hash is what stops a crafted file from flooding this map"
        );
    }

    /// `merge` sums overlapping keys and adopts disjoint ones, in all
    /// three maps, and `finalize` reads the union back as n1/N1/n2/N2.
    ///
    /// Every count differs from every other and none is zero, so a
    /// dropped key, an overwrite where an addition belongs, or a map
    /// crossed with its neighbour all change the totals.
    #[test]
    fn halstead_maps_merge_sums_overlaps_and_adopts_disjoint_keys() {
        let mut parent = halstead_maps_of(
            &[(1, 2), (2, 3)],
            &[(b"int", 1)],
            &[(b"alpha", 4), (b"beta", 7)],
        );
        let child = halstead_maps_of(
            &[(2, 5), (7, 11)],
            &[(b"double", 13)],
            &[(b"alpha", 17), (b"gamma", 19)],
        );

        parent.merge(&child);

        // expected: operators {1: 2, 2: 3+5, 7: 11}; primitives
        // {int: 1, double: 13}; operands {alpha: 4+17, beta: 7,
        // gamma: 19}.
        assert_eq!(
            parent,
            halstead_maps_of(
                &[(1, 2), (2, 8), (7, 11)],
                &[(b"int", 1), (b"double", 13)],
                &[(b"alpha", 21), (b"beta", 7), (b"gamma", 19)],
            )
        );

        let mut stats = Stats::default();
        parent.finalize(&mut stats);
        // expected: n1 = 3 kind ids + 2 primitives; N1 = (2+8+11) +
        // (1+13); n2 = 3 texts; N2 = 21+7+19.
        assert_eq!(stats.unique_operators(), 5);
        assert_eq!(stats.total_operators(), 35);
        assert_eq!(stats.unique_operands(), 3);
        assert_eq!(stats.total_operands(), 47);
    }

    /// Merging an empty child leaves the parent untouched.
    ///
    /// A space with no operators or operands is the common case for a
    /// leaf getter or an empty function body, and `finalize` runs on
    /// the parent afterwards either way.
    #[test]
    fn halstead_maps_merge_of_empty_child_is_a_no_op() {
        let mut parent = halstead_maps_of(&[(3, 5)], &[(b"char", 2)], &[(b"delta", 9)]);
        let before = parent.clone();

        parent.merge(&HalsteadMaps::new());

        assert_eq!(parent, before);

        let mut stats = Stats::default();
        parent.finalize(&mut stats);
        // expected: n1 = 1 kind id + 1 primitive; N1 = 5 + 2; n2 = 1;
        // N2 = 9.
        assert_eq!(stats.unique_operators(), 2);
        assert_eq!(stats.total_operators(), 7);
        assert_eq!(stats.unique_operands(), 1);
        assert_eq!(stats.total_operands(), 9);
    }

    /// Folding a chain of nested spaces bottom-up must reach the union
    /// of every level, re-merging already-merged maps on the way up.
    ///
    /// This is what `spaces.rs` and `ops.rs` actually do: each space is
    /// merged into its parent as the walk pops it, so by the time the
    /// root sees a grandchild's counts they have already passed through
    /// one `merge`. The literal expectation below is what discriminates
    /// — the `nested == flat` cross-check on its own does not, because
    /// any entry-wise fold over the same levels agrees with itself
    /// however it is associated, including a broken one.
    #[test]
    fn halstead_maps_merge_folds_a_nested_chain() {
        let levels = [
            halstead_maps_of(&[(1, 1)], &[(b"int", 1)], &[(b"a", 1)]),
            halstead_maps_of(&[(1, 2), (2, 3)], &[], &[(b"a", 2), (b"b", 4)]),
            halstead_maps_of(&[(2, 5)], &[(b"long", 6)], &[(b"b", 7)]),
            halstead_maps_of(&[(3, 8)], &[(b"int", 9)], &[(b"c", 10)]),
        ];

        // Bottom-up: the deepest level folds into its parent, that
        // result into *its* parent, and so on up to the root.
        let mut nested = levels[levels.len() - 1].clone();
        for level in levels.iter().rev().skip(1) {
            let mut outer = level.clone();
            outer.merge(&nested);
            nested = outer;
        }

        // Flat: every level merged directly into the root.
        let mut flat = levels[0].clone();
        for level in &levels[1..] {
            flat.merge(level);
        }

        // expected: every key summed across the four levels — operators
        // {1: 1+2, 2: 3+5, 3: 8}, primitives {int: 1+9, long: 6},
        // operands {a: 1+2, b: 4+7, c: 10}.
        assert_eq!(
            nested,
            halstead_maps_of(
                &[(1, 3), (2, 8), (3, 8)],
                &[(b"int", 10), (b"long", 6)],
                &[(b"a", 3), (b"b", 11), (b"c", 10)],
            )
        );
        assert_eq!(nested, flat);

        let mut stats = Stats::default();
        nested.finalize(&mut stats);
        // expected: n1 = 3 kind ids + 2 primitives; N1 = (3+8+8) +
        // (10+6); n2 = 3 texts; N2 = 3+11+10.
        assert_eq!(stats.unique_operators(), 5);
        assert_eq!(stats.total_operators(), 35);
        assert_eq!(stats.unique_operands(), 3);
        assert_eq!(stats.total_operands(), 24);
    }

    /// A `kind_id` at the top of the `u16` range must behave like any
    /// other key.
    ///
    /// The largest grammar in the workspace (`mozcpp`) tops out around
    /// 640 symbols, so nothing near `u16::MAX` occurs today — but the
    /// map is keyed by the raw id, and a dense-array representation
    /// (the shape #1108 considered and rejected) is exactly what such a
    /// key would break. Pinning it keeps that trade-off honest if the
    /// representation is ever revisited.
    #[test]
    fn halstead_maps_handle_the_full_kind_id_range() {
        let mut parent = halstead_maps_of(&[(0, 3), (u16::MAX, 5)], &[], &[]);
        parent.merge(&halstead_maps_of(&[(u16::MAX, 7)], &[], &[]));

        let mut stats = Stats::default();
        parent.finalize(&mut stats);
        // expected: two distinct kind ids, occurrences 3 and 5+7.
        assert_eq!(stats.unique_operators(), 2);
        assert_eq!(stats.total_operators(), 15);
    }
}
