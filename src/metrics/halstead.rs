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

use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::fmt;

use crate::checker::Checker;
use crate::getter::Getter;
use crate::macros::implement_metric_trait;

use crate::*;

/// The `Halstead` metric suite.
#[derive(Default, Clone, Debug)]
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
#[derive(Debug, Default, Clone)]
pub struct HalsteadMaps<'a> {
    pub(crate) operators: HashMap<u16, u64>,
    /// Primitive-type operators stored by text so each distinct primitive
    /// (e.g. `int` vs `double`) counts as a separate distinct operator,
    /// even when the grammar maps them all to a single kind_id.
    pub(crate) primitive_operators: HashMap<&'a [u8], u64>,
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

impl Serialize for Stats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut st = serializer.serialize_struct("halstead", 14)?;
        st.serialize_field("n1", &self.u_operators())?;
        st.serialize_field("N1", &self.operators())?;
        st.serialize_field("n2", &self.u_operands())?;
        st.serialize_field("N2", &self.operands())?;
        st.serialize_field("length", &self.length())?;
        st.serialize_field("estimated_program_length", &self.estimated_program_length())?;
        st.serialize_field("purity_ratio", &self.purity_ratio())?;
        st.serialize_field("vocabulary", &self.vocabulary())?;
        st.serialize_field("volume", &self.volume())?;
        st.serialize_field("difficulty", &self.difficulty())?;
        st.serialize_field("level", &self.level())?;
        st.serialize_field("effort", &self.effort())?;
        st.serialize_field("time", &self.time())?;
        st.serialize_field("bugs", &self.bugs())?;
        st.end()
    }
}

impl fmt::Display for Stats {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "n1: {}, \
             N1: {}, \
             n2: {}, \
             N2: {}, \
             length: {}, \
             estimated program length: {}, \
             purity ratio: {}, \
             size: {}, \
             volume: {}, \
             difficulty: {}, \
             level: {}, \
             effort: {}, \
             time: {}, \
             bugs: {}",
            self.u_operators(),
            self.operators(),
            self.u_operands(),
            self.operands(),
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
    pub(crate) fn merge(&mut self, _other: &Stats) {}

    /// Returns `η1`, the number of distinct operators
    #[inline]
    #[must_use]
    pub fn u_operators(&self) -> f64 {
        self.u_operators as f64
    }

    /// Returns `N1`, the number of total operators
    #[inline]
    #[must_use]
    pub fn operators(&self) -> f64 {
        self.operators as f64
    }

    /// Returns `η2`, the number of distinct operands
    #[inline]
    #[must_use]
    pub fn u_operands(&self) -> f64 {
        self.u_operands as f64
    }

    /// Returns `N2`, the number of total operands
    #[inline]
    #[must_use]
    pub fn operands(&self) -> f64 {
        self.operands as f64
    }

    /// Returns the program length
    ///
    /// Computed as `N = N1 + N2`, the sum of [`Self::operators`] and
    /// [`Self::operands`].
    #[inline]
    #[must_use]
    pub fn length(&self) -> f64 {
        self.operands() + self.operators()
    }

    /// Returns the calculated estimated program length
    ///
    /// Computed as `N^ = n1 * log2(n1) + n2 * log2(n2)`, where `n1` is
    /// [`Self::u_operators`] and `n2` is [`Self::u_operands`]. Each term is
    /// treated as `0` when its unique count is `0`.
    #[inline]
    #[must_use]
    pub fn estimated_program_length(&self) -> f64 {
        let uo = self.u_operators();
        let ud = self.u_operands();
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
        let len = self.length();
        if len == 0.0 {
            0.0
        } else {
            self.estimated_program_length() / len
        }
    }

    /// Returns the program vocabulary
    ///
    /// Computed as `n = n1 + n2`, the sum of [`Self::u_operators`] and
    /// [`Self::u_operands`].
    #[inline]
    #[must_use]
    pub fn vocabulary(&self) -> f64 {
        self.u_operands() + self.u_operators()
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
        let vocab = self.vocabulary();
        if vocab <= 1.0 {
            0.0
        } else {
            self.length() * vocab.log2()
        }
    }

    /// Returns the estimated difficulty required to program
    ///
    /// Computed as `D = (n1 / 2) * (N2 / n2)`, where `n1` is
    /// [`Self::u_operators`], `N2` is [`Self::operands`], and `n2` is
    /// [`Self::u_operands`].
    #[inline]
    #[must_use]
    pub fn difficulty(&self) -> f64 {
        let ud = self.u_operands();
        if ud == 0.0 {
            0.0
        } else {
            self.u_operators() / 2. * self.operands() / ud
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
pub trait Halstead
where
    Self: Checker + Getter,
{
    /// Walk `node` and update `stats` with this metric for the language
    /// implementing the trait.
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>);
}

#[inline]
fn get_id<'a>(node: &Node<'a>, code: &'a [u8]) -> &'a [u8] {
    &code[node.start_byte()..node.end_byte()]
}

#[inline]
fn compute_halstead<'a, T: Getter + Checker>(
    node: &Node<'a>,
    code: &'a [u8],
    halstead_maps: &mut HalsteadMaps<'a>,
) {
    match T::get_op_type_with_code(node, code) {
        HalsteadType::Operator => {
            if T::is_primitive(node.kind_id()) {
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
                .entry(T::get_operand_id(node, code))
                .or_insert(0) += 1;
        }
        _ => {}
    }
}

impl Halstead for PythonCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for MozjsCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for JavascriptCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for TypescriptCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for TsxCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for RustCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for CppCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for JavaCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for GroovyCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for CsharpCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for GoCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for PerlCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for KotlinCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for LuaCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for PhpCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

// Real defaults — no operators / operands to count. Audited in #188.
implement_metric_trait!(Halstead, PreprocCode, CcommentCode);

impl Halstead for RubyCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for ElixirCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for BashCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for TclCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
    }
}

impl Halstead for IrulesCode {
    fn compute<'a>(node: &Node<'a>, code: &'a [u8], halstead_maps: &mut HalsteadMaps<'a>) {
        compute_halstead::<Self>(node, code, halstead_maps);
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
    use std::collections::HashSet;
    use std::path::PathBuf;

    use crate::tools::check_metrics;

    use super::*;

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
        let ops = crate::operands_and_operators(&parser, &path).expect("ops walk succeeds");

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
                    @r###"
                    {
                      "n1": 3.0,
                      "N1": 9.0,
                      "n2": 9.0,
                      "N2": 12.0,
                      "length": 21.0,
                      "estimated_program_length": 33.284212515144276,
                      "purity_ratio": 1.584962500721156,
                      "vocabulary": 12.0,
                      "volume": 75.28421251514428,
                      "difficulty": 2.0,
                      "level": 0.5,
                      "effort": 150.56842503028855,
                      "time": 8.364912501682698,
                      "bugs": 0.0094341190071077
                    }"###
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
        check_metrics::<CppParser>(
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
                assert_eq!(metric.halstead.u_operators(), 8.0);
                assert_eq!(metric.halstead.u_operands(), 4.0);
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
        check_metrics::<CppParser>(
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
                    s.u_operators() >= 14.0,
                    "expected >= 14 unique operators (bitwise + logical + syntax), got {}",
                    s.u_operators(),
                );
                assert_eq!(s.u_operands(), 8.0); // f, a, b, x, y, z, 1, 2
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
        check_metrics::<CppParser>(
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
                    s.u_operators() >= 10.0,
                    "expected >= 10 unique operators including ++ / -- / sizeof / cast, got {}",
                    s.u_operators(),
                );
                assert_eq!(s.u_operands(), 4.0);
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
                    @r###"
                    {
                      "n1": 9.0,
                      "N1": 24.0,
                      "n2": 10.0,
                      "N2": 18.0,
                      "length": 42.0,
                      "estimated_program_length": 61.74860596185444,
                      "purity_ratio": 1.470204903853677,
                      "vocabulary": 19.0,
                      "volume": 178.41295556463058,
                      "difficulty": 8.1,
                      "level": 0.1234567901234568,
                      "effort": 1445.1449400735075,
                      "time": 80.28583000408375,
                      "bugs": 0.04260752914034329
                    }"###
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
            assert_eq!(metric.halstead.u_operators() as u64, 6);
            assert_eq!(metric.halstead.operators() as u64, 12);
            assert_eq!(metric.halstead.u_operands() as u64, 6);
            assert_eq!(metric.halstead.operands() as u64, 6);
        });

        // Pin the lesson-4 `n1 == dedupe(ops.operators)` invariant: the
        // kind_id-keyed metrics store and the text-keyed `--ops` store are
        // independent, so a modifier classified in one but not the other
        // would diverge here.
        let path = PathBuf::from("foo.cpp");
        let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::operands_and_operators(&parser, &path).expect("ops walk succeeds");
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
                assert_eq!(s.u_operators(), 8.0);
                assert_eq!(s.u_operands(), 4.0);
                insta::assert_json_snapshot!(
                    s,
                    @r###"
                    {
                      "n1": 8.0,
                      "N1": 11.0,
                      "n2": 4.0,
                      "N2": 6.0,
                      "length": 17.0,
                      "estimated_program_length": 32.0,
                      "purity_ratio": 1.8823529411764706,
                      "vocabulary": 12.0,
                      "volume": 60.94436251225965,
                      "difficulty": 6.0,
                      "level": 0.16666666666666666,
                      "effort": 365.6661750735579,
                      "time": 20.31478750408655,
                      "bugs": 0.01704519358507665
                    }"###
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
            assert_eq!(s.u_operators(), 7.0);
            assert_eq!(s.u_operands(), 3.0);
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
            assert_eq!(s.u_operators(), 6.0);
            assert_eq!(s.u_operands(), 1.0);
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
                assert_eq!(s.u_operators(), 6.0);
                assert_eq!(s.u_operands(), 1.0);
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
                    @r###"
                    {
                      "n1": 10.0,
                      "N1": 23.0,
                      "n2": 9.0,
                      "N2": 15.0,
                      "length": 38.0,
                      "estimated_program_length": 61.74860596185444,
                      "purity_ratio": 1.624963314785643,
                      "vocabulary": 19.0,
                      "volume": 161.42124551085624,
                      "difficulty": 8.333333333333334,
                      "level": 0.12,
                      "effort": 1345.177045923802,
                      "time": 74.7320581068779,
                      "bugs": 0.040619232256751396
                    }"###
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
                assert_eq!(metric.halstead.u_operators(), 33.0);
                assert_eq!(metric.halstead.operators(), 121.0);
                // u_operands / operands grew (was 31/50 before #390): the
                // fix now classifies TypeIdentifier (`T`, `S`, `Option`)
                // and FieldIdentifier (struct fields `x`, `y`) as operands
                // alongside the existing primitive type names.
                assert_eq!(metric.halstead.u_operands(), 36.0);
                assert_eq!(metric.halstead.operands(), 55.0);
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
                assert_eq!(metric.halstead.u_operands(), 8.0);
                assert_eq!(metric.halstead.operands(), 12.0);
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r###"
                {
                  "n1": 9.0,
                  "N1": 14.0,
                  "n2": 8.0,
                  "N2": 12.0,
                  "length": 26.0,
                  "estimated_program_length": 52.529325012980806,
                  "purity_ratio": 2.0203586543454155,
                  "vocabulary": 17.0,
                  "volume": 106.27403387250882,
                  "difficulty": 6.75,
                  "level": 0.14814814814814814,
                  "effort": 717.3497286394346,
                  "time": 39.85276270219081,
                  "bugs": 0.026711567292222575
                }"###
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
                assert_eq!(metric.halstead.u_operands(), 8.0);
                assert_eq!(metric.halstead.operands(), 11.0);
                // `::` appears twice (Vec::new, HashMap::new); without
                // the #394 fix u_operators was 10 and operators 17.
                assert_eq!(metric.halstead.u_operators(), 11.0);
                assert_eq!(metric.halstead.operators(), 19.0);
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r###"
                {
                  "n1": 11.0,
                  "N1": 19.0,
                  "n2": 8.0,
                  "N2": 11.0,
                  "length": 30.0,
                  "estimated_program_length": 62.05374780501027,
                  "purity_ratio": 2.068458260167009,
                  "vocabulary": 19.0,
                  "volume": 127.43782540330756,
                  "difficulty": 7.5625,
                  "level": 0.1322314049586777,
                  "effort": 963.7485546125134,
                  "time": 53.54158636736186,
                  "bugs": 0.03252279825177962
                }"###
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
                // distinct) and operators() would be 7 (minus 3 `::`
                // occurrences). With the fix u_operators=7 and
                // operators=10.
                //
                // unique operators (post-fix): fn, LPAREN, LBRACE,
                // let, =, ::, ;. unique operands: main, m, std,
                // collections, HashMap, new.
                assert_eq!(metric.halstead.u_operators(), 7.0);
                assert_eq!(metric.halstead.operators(), 10.0);
                assert_eq!(metric.halstead.u_operands(), 6.0);
                assert_eq!(metric.halstead.operands(), 6.0);
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
                assert_eq!(metric.halstead.u_operators(), 11.0);
                assert_eq!(metric.halstead.operators(), 13.0);
                assert_eq!(metric.halstead.u_operands(), 5.0);
                assert_eq!(metric.halstead.operands(), 6.0);
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
                // unique operands: main, a, b, c, avg, 3, 5, console.log, console, log, "{}"
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r###"
                    {
                      "n1": 10.0,
                      "N1": 24.0,
                      "n2": 11.0,
                      "N2": 21.0,
                      "length": 45.0,
                      "estimated_program_length": 71.27302875388389,
                      "purity_ratio": 1.583845083419642,
                      "vocabulary": 21.0,
                      "volume": 197.65428402504423,
                      "difficulty": 9.545454545454545,
                      "level": 0.10476190476190476,
                      "effort": 1886.699983875422,
                      "time": 104.81666577085679,
                      "bugs": 0.05089564733125986
                    }"###
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
                // unique operands: main, a, b, c, avg, 3, 5, console.log, console, log, "{}"
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r###"
                    {
                      "n1": 10.0,
                      "N1": 24.0,
                      "n2": 11.0,
                      "N2": 21.0,
                      "length": 45.0,
                      "estimated_program_length": 71.27302875388389,
                      "purity_ratio": 1.583845083419642,
                      "vocabulary": 21.0,
                      "volume": 197.65428402504423,
                      "difficulty": 9.545454545454545,
                      "level": 0.10476190476190476,
                      "effort": 1886.699983875422,
                      "time": 104.81666577085679,
                      "bugs": 0.05089564733125986
                    }"###
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
                // unique operands: main, a, b, c, avg, 3, 5, console.log, console, log, "{}"
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r###"
                    {
                      "n1": 10.0,
                      "N1": 24.0,
                      "n2": 11.0,
                      "N2": 21.0,
                      "length": 45.0,
                      "estimated_program_length": 71.27302875388389,
                      "purity_ratio": 1.583845083419642,
                      "vocabulary": 21.0,
                      "volume": 197.65428402504423,
                      "difficulty": 9.545454545454545,
                      "level": 0.10476190476190476,
                      "effort": 1886.699983875422,
                      "time": 104.81666577085679,
                      "bugs": 0.05089564733125986
                    }"###
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
                // unique operands: main, a, b, c, avg, 3, 5, console.log, console, log, "{}"
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r###"
                    {
                      "n1": 10.0,
                      "N1": 24.0,
                      "n2": 11.0,
                      "N2": 21.0,
                      "length": 45.0,
                      "estimated_program_length": 71.27302875388389,
                      "purity_ratio": 1.583845083419642,
                      "vocabulary": 21.0,
                      "volume": 197.65428402504423,
                      "difficulty": 9.545454545454545,
                      "level": 0.10476190476190476,
                      "effort": 1886.699983875422,
                      "time": 104.81666577085679,
                      "bugs": 0.05089564733125986
                    }"###
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
            assert_eq!(metric.halstead.u_operands(), 2.0);
            assert_eq!(metric.halstead.operands(), 2.0);
        });
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
                assert_eq!(metric.halstead.u_operands(), 2.0);
                assert_eq!(metric.halstead.operands(), 3.0);
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
            assert_eq!(metric.halstead.u_operands(), 2.0);
            assert_eq!(metric.halstead.operands(), 2.0);
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
                assert_eq!(metric.halstead.u_operands(), 2.0);
                assert_eq!(metric.halstead.operands(), 3.0);
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
        // After #313 the `: string` annotation's `String2` child also
        // counts as an operand (text `"string"`), so unique operands
        // are `f`, `` `hello` ``, `string` (3 each). The headline of
        // this test — that the plain template literal contributes one
        // operand — is unaffected.
        check_metrics::<TypescriptParser>(
            "function f(): string { return `hello`; }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 3.0);
            },
        );
    }

    #[test]
    fn typescript_template_string_interpolation_no_double_count() {
        // Regression: issue #192. Mirrors
        // `javascript_template_string_interpolation_no_double_count`
        // for TypeScript.
        //
        // After #313 each `: string` annotation contributes one
        // `"string"` operand. Unique operands: `f`, `name`, `string`
        // (3). Total operands: `f`, `name` (param), `name` (in the
        // interpolation), `string`, `string` (5). The interpolation
        // guard from #192 still holds — the wrapping `` `Hi ${name}!` ``
        // is `Unknown`, not double-counted.
        check_metrics::<TypescriptParser>(
            "function f(name: string): string { return `Hi ${name}!`; }",
            "foo.ts",
            |metric| {
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 5.0);
            },
        );
    }

    #[test]
    fn tsx_template_string_plain_is_operand() {
        // Regression: issue #192. Mirrors
        // `javascript_template_string_plain_is_operand` for the
        // TSX (TypeScript + JSX) variant.
        //
        // After #313 TSX's type-keyword `string` (`String3`) also
        // counts as an operand, mirroring TS::String2.
        check_metrics::<TsxParser>(
            "function f(): string { return `hello`; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 3.0);
            },
        );
    }

    #[test]
    fn tsx_template_string_interpolation_no_double_count() {
        // Regression: issue #192. Mirrors
        // `javascript_template_string_interpolation_no_double_count`
        // for the TSX (TypeScript + JSX) variant.
        //
        // After #313 each `: string` annotation contributes one
        // `String3` operand; see `typescript_template_string_…` for
        // the count derivation.
        check_metrics::<TsxParser>(
            "function f(name: string): string { return `Hi ${name}!`; }",
            "foo.tsx",
            |metric| {
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 5.0);
            },
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
            assert_eq!(m.halstead.u_operators(), 6.0);
            assert_eq!(m.halstead.operators(), 7.0);
        });
    }

    #[test]
    fn mozjs_optional_chain_not_double_counted_in_halstead_281() {
        check_metrics::<MozjsParser>("function f(a) { return a?.b?.c; }", "foo.js", |m| {
            assert_eq!(m.halstead.u_operators(), 6.0);
            assert_eq!(m.halstead.operators(), 7.0);
        });
    }

    #[test]
    fn typescript_optional_chain_not_double_counted_in_halstead_281() {
        // The TS grammar wraps member-expression `?.` in an
        // `optional_chain` named node containing the bare `?.`
        // token; classifying both as `Operator` double-counted the
        // chain. We now count only the bare token, so TS matches JS.
        check_metrics::<TypescriptParser>("function f(a) { return a?.b?.c; }", "foo.ts", |m| {
            assert_eq!(m.halstead.u_operators(), 6.0);
            assert_eq!(m.halstead.operators(), 7.0);
        });
    }

    #[test]
    fn tsx_optional_chain_not_double_counted_in_halstead_281() {
        check_metrics::<TsxParser>("function f(a) { return a?.b?.c; }", "foo.tsx", |m| {
            assert_eq!(m.halstead.u_operators(), 6.0);
            assert_eq!(m.halstead.operators(), 7.0);
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
    // * Operands: `f`, `a`, `a`, `b`, `c`, plus the two wrapping
    //   member expressions (`a?.b`, `a?.b?.c`) classified as
    //   `MemberExpression*` (7 total, 6 unique).
    //
    // Verified by test-via-revert: dropping `OptionalChain` from
    // JS/MozJS, or `QMARKDOT` from TS/TSX, trips the test
    // (u_operators 6→5). This input does NOT exercise every operand
    // alias in the per-language `operand_extras` (`Identifier2`,
    // `String2`, `NestedIdentifier`, `MemberExpression4`); drift in
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
            assert_eq!(m.halstead.u_operators(), 6.0);
            assert_eq!(m.halstead.operators(), 7.0);
            assert_eq!(m.halstead.u_operands(), 6.0);
            assert_eq!(m.halstead.operands(), 7.0);
        };

        check_metrics::<JavascriptParser>(SRC, "foo.js", check);
        check_metrics::<MozjsParser>(SRC, "foo.js", check);
        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
    }

    // Issue #313: parity guard for the `"string"` type-keyword aliases
    // that the TS / TSX grammars expose. `Checker::is_string` matches
    // these aliases (#283), so `Getter::get_op_type` must also classify
    // them — otherwise the same node disagrees between the two
    // predicates and Halstead silently undercounts every `: string`
    // annotation by one operand.
    //
    // For the input `let x: string = "y";`:
    //
    // * TypeScript emits `Typescript::String2` for the `string` type
    //   keyword (kind_id 135, in the type-keyword block of the enum).
    // * TSX emits `Tsx::String3` for the same role (kind_id 141).
    //
    // After #313 both kinds are in `operand_extras` and contribute one
    // `"string"` operand. Verified by test-via-revert: dropping
    // `String2` from TS's `operand_extras` (or `String3` from TSX's)
    // trips this test on `u_operands` / `operands` for the affected
    // language.
    #[test]
    fn ts_family_string2_string3_type_keyword_parity_313() {
        const SRC: &str = "let x: string = \"y\";";
        // Operators (n1 = 5, N1 = 5):
        //   `let`, `:`, `=`, `;`, plus `string` (PredefinedType wrapper,
        //   routed through `is_primitive` so it's keyed by its lexeme
        //   `"string"` in `primitive_operators`).
        // Operands (n2 = 3, N2 = 3):
        //   `x`, the `"y"` literal, and `string` (the type-keyword
        //   child of `predefined_type`, classified via the operand
        //   extras added by #313). Pre-fix the TS column reported
        //   n2 = 2 / N2 = 2 because String2 fell through to `Unknown`;
        //   the TSX column had the same gap for String3.
        let check = |m: crate::CodeMetrics| {
            assert_eq!(m.halstead.u_operators(), 5.0);
            assert_eq!(m.halstead.operators(), 5.0);
            assert_eq!(m.halstead.u_operands(), 3.0);
            assert_eq!(m.halstead.operands(), 3.0);
        };

        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
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
            assert_eq!(m.halstead.u_operators(), 7.0);
            assert_eq!(m.halstead.operators(), 7.0);
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
            assert_eq!(m.halstead.u_operators(), 4.0);
            assert_eq!(m.halstead.operators(), 4.0);
            assert_eq!(m.halstead.u_operands(), 2.0);
            assert_eq!(m.halstead.operands(), 2.0);
        };

        check_metrics::<TypescriptParser>(SRC, "foo.ts", check);
        check_metrics::<TsxParser>(SRC, "foo.tsx", check);
    }

    #[test]
    fn python_wrong_operators() {
        check_metrics::<PythonParser>("()[]{}", "foo.py", |metric| {
            insta::assert_json_snapshot!(
                metric.halstead,
                @r###"
                    {
                      "n1": 0.0,
                      "N1": 0.0,
                      "n2": 0.0,
                      "N2": 0.0,
                      "length": 0.0,
                      "estimated_program_length": 0.0,
                      "purity_ratio": 0.0,
                      "vocabulary": 0.0,
                      "volume": 0.0,
                      "difficulty": 0.0,
                      "level": 0.0,
                      "effort": 0.0,
                      "time": 0.0,
                      "bugs": 0.0
                    }"###
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
                    @r###"
                    {
                      "n1": 2.0,
                      "N1": 2.0,
                      "n2": 1.0,
                      "N2": 1.0,
                      "length": 3.0,
                      "estimated_program_length": 2.0,
                      "purity_ratio": 0.6666666666666666,
                      "vocabulary": 3.0,
                      "volume": 4.754887502163468,
                      "difficulty": 1.0,
                      "level": 1.0,
                      "effort": 4.754887502163468,
                      "time": 0.26416041678685936,
                      "bugs": 0.0009425525573729414
                    }"###
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
                  "n1": 11.0,
                  "N1": 26.0,
                  "n2": 12.0,
                  "N2": 22.0,
                  "length": 48.0,
                  "estimated_program_length": 81.07329781366414,
                  "purity_ratio": 1.6890270377846697,
                  "vocabulary": 23.0,
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
                  "n1": 11.0,
                  "N1": 28.0,
                  "n2": 19.0,
                  "N2": 19.0,
                  "length": 47.0,
                  "estimated_program_length": 118.76437056043838,
                  "purity_ratio": 2.526901501285923,
                  "vocabulary": 30.0,
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
                assert_eq!(metric.halstead.u_operators(), 8.0);
                assert_eq!(metric.halstead.u_operands(), 13.0);
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "n1": 8.0,
                  "N1": 22.0,
                  "n2": 13.0,
                  "N2": 23.0,
                  "length": 45.0,
                  "estimated_program_length": 72.10571633583419,
                  "purity_ratio": 1.6023492519074265,
                  "vocabulary": 21.0,
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
                assert_eq!(metric.halstead.u_operators(), 2.0);
                assert_eq!(metric.halstead.u_operands(), 27.0);
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r#"
                {
                  "n1": 2.0,
                  "N1": 10.0,
                  "n2": 27.0,
                  "N2": 28.0,
                  "length": 38.0,
                  "estimated_program_length": 130.38196255841365,
                  "purity_ratio": 3.4311042778529908,
                  "vocabulary": 29.0,
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

    #[test]
    fn groovy_closure_operators_and_operands() {
        check_metrics::<GroovyParser>("def double = { x -> x * 2 }", "foo.groovy", |metric| {
            // Closure with arrow-style parameter list.
            // Distinct operators: def, =, {}, ->, * = 5.
            // Distinct operands: double, x, 2 = 3.
            assert_eq!(metric.halstead.u_operators(), 5.0);
            assert_eq!(metric.halstead.u_operands(), 3.0);
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
                // Each Groovy-specific operator kind contributes one
                // distinct entry to the operator set. The 20-operator
                // floor breaks down as: 16 Groovy-specific tokens
                // exercised by the fixture (`?:`, `?.`, `??.`, `*.`,
                // `.&`, `.@`, `===`, `!==`, `<=>`, `=~`, `==~`, `..<`,
                // `<..`, `<..<`, `as`, `?[`) plus a handful of
                // ambient Java-shaped operators the fixture also
                // uses (`def`, `=`, `{`, `(`, `,`, `return`). A
                // grammar regression that drops one of the 16
                // Groovy-specific tokens would push the count below
                // this floor.
                // Exact pin: with the dekobon Groovy grammar this
                // fixture exercises 16 Groovy-specific tokens (`?:`,
                // `?.`, `??.`, `*.`, `.&`, `.@`, `===`, `!==`, `<=>`,
                // `=~`, `==~`, `..<`, `<..`, `<..<`, `as`, `?[`) plus
                // 7 ambient Java-shaped operators the fixture also
                // uses (`def`, `=`, `,`, `{`, `(`, `[`, `return`),
                // for a total of 23 distinct operator kinds. A
                // regression that drops any one of the 16 #247
                // operators would push the count below 23 and fail
                // this assertion. The complementary AST walk below
                // pins each #247 operator's identity individually so
                // a grammar change that adds an unrelated operator
                // (lifting `u_operators` to 24) still flags the loss
                // of a #247 operator at the per-token level.
                assert_eq!(
                    metric.halstead.u_operators(),
                    23.0,
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
            assert_eq!(metric.halstead.u_operands(), 2.0);
            assert_eq!(metric.halstead.operands(), 3.0);
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
            assert_eq!(metric.halstead.u_operands(), 3.0);
            assert_eq!(metric.halstead.operands(), 3.0);
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
            assert_eq!(metric.halstead.u_operands(), 2.0);
            assert_eq!(metric.halstead.operands(), 2.0);
        });
        assert_ops_operands::<GroovyParser>(src, "foo.groovy", 2, vec!["f", "\"plain\""]);
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
                assert_eq!(metric.halstead.u_operators(), 15.0);
                assert_eq!(metric.halstead.operators(), 32.0);
                assert_eq!(metric.halstead.u_operands(), 13.0);
                assert_eq!(metric.halstead.operands(), 23.0);
                // Pin every Halstead field; values are whatever the
                // classifier produces and become the regression spec.
                insta::assert_json_snapshot!(metric.halstead);
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
                assert_eq!(metric.halstead.u_operators(), 14.0);
                assert_eq!(metric.halstead.operators(), 33.0);
                assert_eq!(metric.halstead.u_operands(), 21.0);
                assert_eq!(metric.halstead.operands(), 23.0);
                insta::assert_json_snapshot!(metric.halstead);
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
                assert_eq!(metric.halstead.u_operators(), 9.0);
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
                assert_eq!(metric.halstead.u_operands(), 4.0);
                assert_eq!(metric.halstead.operands(), 5.0);
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
                assert_eq!(metric.halstead.u_operands(), 4.0);
                assert_eq!(metric.halstead.operands(), 4.0);
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
                assert_eq!(metric.halstead.u_operands(), 4.0);
                assert_eq!(metric.halstead.operands(), 4.0);
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
                    @r###"
                    {
                      "n1": 7.0,
                      "N1": 7.0,
                      "n2": 5.0,
                      "N2": 8.0,
                      "length": 15.0,
                      "estimated_program_length": 31.26112492884004,
                      "purity_ratio": 2.0840749952560027,
                      "vocabulary": 12.0,
                      "volume": 53.77443751081734,
                      "difficulty": 5.6,
                      "level": 0.17857142857142858,
                      "effort": 301.1368500605771,
                      "time": 16.729825003365395,
                      "bugs": 0.014975730436275946
                    }"###
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
                  "n1": 10.0,
                  "N1": 14.0,
                  "n2": 4.0,
                  "N2": 6.0,
                  "length": 20.0,
                  "estimated_program_length": 41.219280948873624,
                  "purity_ratio": 2.0609640474436812,
                  "vocabulary": 14.0,
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
                assert_eq!(metric.halstead.u_operands(), 4.0);
                assert_eq!(metric.halstead.operands(), 6.0);
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
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 4.0);
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
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 4.0);
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
            assert_eq!(metric.halstead.u_operands(), 2.0);
            assert_eq!(metric.halstead.operands(), 2.0);
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
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 4.0);
            },
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
                // n1=12: local,function,(,,,),=,+,if,>,then,return,end
                // n2=5: add,a,b,result,0
                insta::assert_json_snapshot!(metric.halstead, @r###"
                    {
                      "n1": 12.0,
                      "N1": 15.0,
                      "n2": 5.0,
                      "N2": 10.0,
                      "length": 25.0,
                      "estimated_program_length": 54.62919048309068,
                      "purity_ratio": 2.1851676193236274,
                      "vocabulary": 17.0,
                      "volume": 102.18657103125848,
                      "difficulty": 12.0,
                      "level": 0.08333333333333333,
                      "effort": 1226.2388523751017,
                      "time": 68.12438068750565,
                      "bugs": 0.03818816527310305
                    }
                    "###);
            },
        );
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
                    @r###"
                    {
                      "n1": 9.0,
                      "N1": 11.0,
                      "n2": 5.0,
                      "N2": 10.0,
                      "length": 21.0,
                      "estimated_program_length": 40.13896548741762,
                      "purity_ratio": 1.9113793089246487,
                      "vocabulary": 14.0,
                      "volume": 79.9544533632097,
                      "difficulty": 9.0,
                      "level": 0.1111111111111111,
                      "effort": 719.5900802688873,
                      "time": 39.97722668160485,
                      "bugs": 0.026767153565498338
                    }
                    "###
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
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 5.0);
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
            assert_eq!(metric.halstead.u_operands(), 4.0);
            assert_eq!(metric.halstead.operands(), 5.0);
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
            assert_eq!(metric.halstead.u_operands(), 4.0);
            assert_eq!(metric.halstead.operands(), 4.0);
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
            assert_eq!(metric.halstead.u_operands(), 3.0);
            assert_eq!(metric.halstead.operands(), 3.0);
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
                assert_eq!(metric.halstead.u_operands(), 4.0);
                assert_eq!(metric.halstead.operands(), 5.0);
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
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 3.0);
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
                assert_eq!(metric.halstead.u_operands(), 2.0);
                assert_eq!(metric.halstead.operands(), 3.0);
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
            assert_eq!(metric.halstead.u_operands(), 2.0);
            assert_eq!(metric.halstead.operands(), 2.0);
        });
    }

    #[test]
    fn python_empty_file_halstead() {
        check_metrics::<PythonParser>("", "empty.py", |metric| {
            let h = &metric.halstead;
            assert_eq!(h.u_operators(), 0.0);
            assert_eq!(h.operands(), 0.0);
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
                assert_eq!(metric.halstead.u_operators(), 3.0);
                assert_eq!(metric.halstead.operators(), 5.0);
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
            assert_eq!(metric.halstead.u_operators(), 3.0);
            assert_eq!(metric.halstead.operators(), 3.0);
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
                assert_eq!(metric.halstead.u_operators(), 3.0);
                assert_eq!(metric.halstead.operators(), 5.0);
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
                assert_eq!(metric.halstead.u_operators(), 3.0);
                assert_eq!(metric.halstead.operators(), 3.0);
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
                assert_eq!(metric.halstead.u_operators(), 7.0);
                assert_eq!(metric.halstead.operators(), 8.0);
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
                // `x` (assignment LHS and inside `$x`) is a `variable_name`
                // with aliased kind_id 160 — all three aliases must be in
                // the operand list (see lesson 2).
                assert_eq!(metric.halstead.u_operators(), 12.0);
                assert_eq!(metric.halstead.operators(), 12.0);
                assert_eq!(metric.halstead.u_operands(), 6.0);
                assert_eq!(metric.halstead.operands(), 9.0);
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
        //     expansion), `simple_expansion` `$x`, inner variable_name
        //     `x` → 3.
        // Total unique operands: 5 (`a`, `b`, `"plain"`, `$x`, `x`),
        // each appearing once → N2 = 5. Without the #180 fix, the
        // wrapping `"$x"` literal would also be counted, making
        // u_operands = 6 and N2 = 6. The `=` is the only operator;
        // appears twice (N1 = 2, n1 = 1).
        check_metrics::<BashParser>("a=\"plain\"\nb=\"$x\"\n", "foo.sh", |metric| {
            assert_eq!(metric.halstead.u_operators(), 1.0);
            assert_eq!(metric.halstead.operators(), 2.0);
            assert_eq!(metric.halstead.u_operands(), 5.0);
            assert_eq!(metric.halstead.operands(), 5.0);
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
        // N2 = 6. Operators (`do`, `end`, `(`, `)`, `=`, `#{`, `}`)
        // are unchanged: u = N = 7 (the `#{`/`}` interpolation
        // markers stay classified as operators).
        check_metrics::<ElixirParser>(
            "def greet(name) do\n  msg = \"Hi #{name}\"\nend\n",
            "foo.ex",
            |metric| {
                assert_eq!(metric.halstead.u_operators(), 7.0);
                assert_eq!(metric.halstead.operators(), 7.0);
                assert_eq!(metric.halstead.u_operands(), 4.0);
                assert_eq!(metric.halstead.operands(), 5.0);
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
            assert_eq!(metric.halstead.u_operands(), 3.0);
            assert_eq!(metric.halstead.operands(), 3.0);
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
                assert_eq!(metric.halstead.u_operands(), 5.0);
                assert_eq!(metric.halstead.operands(), 6.0);
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
                assert_eq!(metric.halstead.u_operands(), 4.0);
                assert_eq!(metric.halstead.operands(), 5.0);
            },
        );
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
        //   line 1 `a="$v"`: var_name `a`, simple_expansion `$v`,
        //     inner var_name `v` (wrapper skipped) → 3
        //   line 2 `b="${v[0]}"`: var_name `b`, var_name `v` (inside
        //     subscript), number `0` (wrapper skipped, `expansion`
        //     itself is not in the operand list) → 3
        //   line 3 `c="$(date)"`: var_name `c`, command_name `date`
        //     (wrapper skipped, `command_substitution` not in operand
        //     list) → 2
        //   line 4 `d="$((1+2))"`: var_name `d`, numbers `1` and `2`
        //     (wrapper skipped, `arithmetic_expansion` not in operand
        //     list) → 3
        // Unique operands (`v` shared across lines 1 and 2): a, b, c,
        // d, $v, v, 0, date, 1, 2 → 10. Total occurrences: 12 (`v`
        // appears twice). Operators include `=` four times plus the
        // `${`, `}`, `$(`, `)`, `$((`, `))`, `[`, `]`, `+` punctuation.
        check_metrics::<BashParser>(
            "a=\"$v\"\nb=\"${v[0]}\"\nc=\"$(date)\"\nd=\"$((1+2))\"\n",
            "foo.sh",
            |metric| {
                assert_eq!(metric.halstead.u_operators(), 6.0);
                assert_eq!(metric.halstead.operators(), 9.0);
                assert_eq!(metric.halstead.u_operands(), 10.0);
                assert_eq!(metric.halstead.operands(), 12.0);
            },
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
                // Operands: `f`, `s`, `"hello world"` — 3 unique, 3 total.
                // The wrapping `QuotedWord` must still contribute exactly
                // one operand when it carries no interpolation children;
                // dropping to 2 would mean the inert case was over-guarded.
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 3.0);
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
                // Operands: `f`, `x`, `y` (proc args), `s`, `$x`, `$y` — 6
                // unique, 6 total. The wrapping `QuotedWord` contributes
                // nothing. Pre-fix this read 7/7 (double-counted wrapper).
                assert_eq!(metric.halstead.u_operands(), 6.0);
                assert_eq!(metric.halstead.operands(), 6.0);
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
                // Operands: `f`, `s`, `foo` — 3 unique, 3 total. The
                // wrapping `QuotedWord` and the inert text `result: ` do
                // not contribute extra operands. Pre-fix this read 4/4
                // (double-counted wrapper).
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 3.0);
                insta::assert_json_snapshot!(metric.halstead);
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
                assert_eq!(metric.halstead.u_operators(), 11.0);
                assert_eq!(metric.halstead.operators(), 15.0);
                assert_eq!(metric.halstead.u_operands(), 9.0);
                assert_eq!(metric.halstead.operands(), 22.0);
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
                assert_eq!(metric.halstead.u_operators(), 9.0);
                assert_eq!(metric.halstead.operators(), 9.0);
                assert_eq!(metric.halstead.u_operands(), 5.0);
                assert_eq!(metric.halstead.operands(), 10.0);
                insta::assert_json_snapshot!(metric.halstead);
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
        //   interpolated string), `name` × 2 (the `name` token inside
        //   each `variable_name`), `"world"` × 1.
        // u_operands = 3, N2 = 5.
        // Without the fix the wrapping `"Hello $name!"` would also
        // count → u_operands = 4, N2 = 6.
        check_metrics::<PhpParser>(
            "<?php $name = \"world\"; echo \"Hello $name!\";",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 5.0);
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
            assert_eq!(metric.halstead.u_operands(), 1.0);
            assert_eq!(metric.halstead.operands(), 1.0);
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
        // Operands by text key: `$name` × 2, `name` × 2, `"x"` × 1
        // (inert single-interp encapsed string also operand). With
        // the fix u_operands = 3, N2 = 5. Without the fix the
        // wrapping heredoc text would add one more unique operand.
        check_metrics::<PhpParser>(
            "<?php $name = \"x\"; echo <<<EOT\nhi $name\nEOT;\n",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 5.0);
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
                assert_eq!(metric.halstead.u_operands(), 1.0);
                assert_eq!(metric.halstead.operands(), 1.0);
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
        //   `obj`  (name) × 3 (one per `variable_name`)
        //   `prop` (name) × 2 (member-access RHS twice)
        //   `stdClass`    × 1
        //   `"x"`         × 1
        // ⇒ u_operands = 5, N2 = 10.
        // With the bug the wrapping `"Hi $obj->prop!"` text adds one
        // more unique operand and one more occurrence ⇒ 6 / 11.
        check_metrics::<PhpParser>(
            "<?php $obj = new stdClass; $obj->prop = \"x\"; echo \"Hi $obj->prop!\";",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.u_operands(), 5.0);
                assert_eq!(metric.halstead.operands(), 10.0);
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
        //   `$arr` × 2, `arr` × 2 (inner `name`), `1` × 1, `0` × 1.
        // ⇒ u_operands = 4, N2 = 6.
        // With the bug the wrapping `"Hi $arr[0]!"` text adds 1 / 1.
        check_metrics::<PhpParser>(
            "<?php $arr = [1]; echo \"Hi $arr[0]!\";",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.u_operands(), 4.0);
                assert_eq!(metric.halstead.operands(), 6.0);
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
        //   `$out` × 1, `out` × 1 (inner `name`), backtick literal × 1.
        // ⇒ u_operands = 3, N2 = 3.
        // Before the fix the backtick literal vanished from the count
        // ⇒ u_operands = 2, N2 = 2.
        check_metrics::<PhpParser>("<?php $out = `ls`;", "foo.php", |metric| {
            assert_eq!(metric.halstead.u_operands(), 3.0);
            assert_eq!(metric.halstead.operands(), 3.0);
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
        //   `dir`  × 2 (inner `name`),
        //   `$out` × 1, `out` × 1, `"/tmp"` × 1.
        // ⇒ u_operands = 5, N2 = 7.
        // Without the interpolation guard the wrapping backtick literal
        // would also count ⇒ u_operands = 6, N2 = 8.
        check_metrics::<PhpParser>(
            "<?php $dir = \"/tmp\"; $out = `ls $dir`;",
            "foo.php",
            |metric| {
                assert_eq!(metric.halstead.u_operands(), 5.0);
                assert_eq!(metric.halstead.operands(), 7.0);
            },
        );
    }

    #[test]
    fn elixir_operators_and_operands() {
        // Exercises every Halstead family classified in Elixir's
        // `get_op_type`: control-flow keywords (`do`, `end`, `fn`),
        // structural punctuation (`(`, `)`, `[`, `]`, `,`, `.`, `@`),
        // arithmetic (`+`, `-`, `*`, `/`), comparison (`==`, `>`),
        // logical (`&&`, `||`, `and`, `or`, `!`), pipe (`|>`), capture
        // (`&`), assignment/match (`=`), and the stab arrow (`->`).
        // The body mixes identifiers, integers, atoms, and a string.
        check_metrics::<ElixirParser>(
            "defmodule Foo do\n  @doc \"add\"\n  def calc(a, b) do\n    result = a + b * 2\n    flag = result > 0 && a == b\n    out = if flag, do: result, else: -result\n    [out, a, b]\n  end\nend\n",
            "foo.ex",
            |metric| {
                // Positive headline assertions on integer counts.
                assert_eq!(metric.halstead.u_operators(), 15.0);
                assert_eq!(metric.halstead.operators(), 23.0);
                assert_eq!(metric.halstead.u_operands(), 16.0);
                assert_eq!(metric.halstead.operands(), 27.0);
                insta::assert_json_snapshot!(
                    metric.halstead,
                    @r###"
                {
                  "n1": 15.0,
                  "N1": 23.0,
                  "n2": 16.0,
                  "N2": 27.0,
                  "length": 50.0,
                  "estimated_program_length": 122.60335893412778,
                  "purity_ratio": 2.452067178682556,
                  "vocabulary": 31.0,
                  "volume": 247.70981551934375,
                  "difficulty": 12.65625,
                  "level": 0.07901234567901234,
                  "effort": 3135.0773526666944,
                  "time": 174.17096403703857,
                  "bugs": 0.07140208917738183
                }"###
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
                assert_eq!(metric.halstead.u_operators(), 9.0);
                assert_eq!(metric.halstead.operators(), 11.0);
                assert_eq!(metric.halstead.u_operands(), 3.0);
                assert_eq!(metric.halstead.operands(), 9.0);
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
            assert_eq!(metric.halstead.u_operators(), 2.0);
            assert_eq!(metric.halstead.operators(), 2.0);
            assert_eq!(metric.halstead.u_operands(), 2.0);
            assert_eq!(metric.halstead.operands(), 2.0);
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
            assert_eq!(metric.halstead.u_operands(), 2.0);
            assert_eq!(metric.halstead.operands(), 3.0);
        });
    }

    #[test]
    fn ruby_halstead_symbol_literal_operand() {
        // `:foo` is a `SimpleSymbol` leaf — counts as a single
        // operand, no interpolation guard needed (only
        // `DelimitedSymbol` (`:"…#{x}…"`) can interpolate).
        // expected: operators = {def, end} = 2; operands = {f, :ok} = 2.
        check_metrics::<RubyParser>("def f\n  :ok\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.halstead.u_operators(), 2.0);
            assert_eq!(metric.halstead.u_operands(), 2.0);
        });
    }

    #[test]
    fn ruby_halstead_regex_operand() {
        // `/foo/` parses as a `Regex` node — one operand. The slash
        // delimiters around it are emitted as `SLASH` tokens and
        // classified as arithmetic-or-divide operators by the shared
        // arm; they count once toward the distinct-operator set.
        // expected: u_operators = {def, (, ), =~, /, end} = 6;
        // u_operands = {f, s, /foo/} = 3.
        check_metrics::<RubyParser>("def f(s)\n  s =~ /foo/\nend\n", "foo.rb", |metric| {
            assert_eq!(metric.halstead.u_operators(), 6.0);
            assert_eq!(metric.halstead.u_operands(), 3.0);
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
            assert_eq!(metric.halstead.u_operators(), 12.0);
            assert_eq!(metric.halstead.operators(), 20.0);
            assert_eq!(metric.halstead.u_operands(), 12.0);
            assert_eq!(metric.halstead.operands(), 16.0);
        });

        let path = PathBuf::from("foo.irule");
        let parser = IrulesParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::operands_and_operators(&parser, &path).expect("ops walk succeeds");
        let unique_operators: HashSet<&str> = ops.operators.iter().map(String::as_str).collect();
        let unique_operands: HashSet<&str> = ops.operands.iter().map(String::as_str).collect();
        assert_eq!(
            unique_operators.len(),
            12,
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
    /// family). iRules additionally counts the `set` target `s`, which
    /// tree-sitter-tcl's grammar structure omits — hence n2=4 here vs Tcl's
    /// 3. Mirrors `tcl_inert_quoted_word_counts_as_operand` (#277).
    #[test]
    fn irules_inert_quoted_word_counts_as_operand() {
        let source = "proc f {} {\n    set s \"hello world\"\n}\n";
        check_metrics::<IrulesParser>(source, "foo.irule", |metric| {
            assert_eq!(metric.halstead.u_operators(), 4.0);
            assert_eq!(metric.halstead.operators(), 6.0);
            assert_eq!(metric.halstead.u_operands(), 4.0);
            assert_eq!(metric.halstead.operands(), 4.0);
        });

        let path = PathBuf::from("foo.irule");
        let parser = IrulesParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::operands_and_operators(&parser, &path).expect("ops walk succeeds");
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
            assert_eq!(metric.halstead.u_operators(), 4.0);
            assert_eq!(metric.halstead.operators(), 6.0);
            assert_eq!(metric.halstead.u_operands(), 7.0);
            assert_eq!(metric.halstead.operands(), 7.0);
        });

        let path = PathBuf::from("foo.irule");
        let parser = IrulesParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::operands_and_operators(&parser, &path).expect("ops walk succeeds");
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
            assert_eq!(metric.halstead.u_operators(), 26.0);
            assert_eq!(metric.halstead.operators(), 57.0);
            assert_eq!(metric.halstead.u_operands(), 23.0);
            assert_eq!(metric.halstead.operands(), 42.0);
        });

        let path = PathBuf::from("foo.irule");
        let parser = IrulesParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::operands_and_operators(&parser, &path).expect("ops walk succeeds");
        let unique_operators: HashSet<&str> = ops.operators.iter().map(String::as_str).collect();
        let unique_operands: HashSet<&str> = ops.operands.iter().map(String::as_str).collect();
        assert_eq!(
            unique_operators.len(),
            26,
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
    /// (`operands()` == 5). If the guard regressed, the inner `id` "x" would
    /// add a sixth operand occurrence (it text-collides with the proc arg
    /// `x`, so `u_operands` would stay 5 but `operands()` would rise to 6 —
    /// hence the total, not just the unique count, is asserted).
    #[test]
    fn irules_bare_variable_operand() {
        let source = "proc f {x} {\n    return $x\n}\n";
        check_metrics::<IrulesParser>(source, "foo.irule", |metric| {
            assert_eq!(metric.halstead.u_operators(), 3.0);
            assert_eq!(metric.halstead.operators(), 5.0);
            assert_eq!(metric.halstead.u_operands(), 5.0);
            assert_eq!(metric.halstead.operands(), 5.0);
        });

        let path = PathBuf::from("foo.irule");
        let parser = IrulesParser::new(source.as_bytes().to_vec(), &path, None);
        let ops = crate::operands_and_operators(&parser, &path).expect("ops walk succeeds");
        let bare_var = ops.operands.iter().filter(|o| o.as_str() == "$x").count();
        assert_eq!(
            bare_var, 1,
            "bare $x must be exactly one operand (inner id leaf not double-counted); operands were {:?}",
            ops.operands
        );
    }
}
