// Per-language metric and AST modules deliberately consume the macro-
// generated tree-sitter token enums via `use crate::*` and `use Foo::*`
// inside match expressions — explicit imports would list dozens of
// variants per arm and obscure the per-language token sets that are the
// point of these files. Allowed at the module level rather than per
// function so the per-language impl blocks stay readable.
#![allow(clippy::wildcard_imports, clippy::enum_glob_use)]

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
        for (k, v) in other.operators.iter() {
            *self.operators.entry(*k).or_insert(0) += v;
        }
        for (k, v) in other.primitive_operators.iter() {
            *self.primitive_operators.entry(*k).or_insert(0) += v;
        }
        for (k, v) in other.operands.iter() {
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
    #[inline]
    #[must_use]
    pub fn length(&self) -> f64 {
        self.operands() + self.operators()
    }

    /// Returns the calculated estimated program length
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
    #[inline]
    #[must_use]
    pub fn vocabulary(&self) -> f64 {
        self.u_operands() + self.u_operators()
    }

    /// Returns the program volume.
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
    #[inline]
    #[must_use]
    pub fn level(&self) -> f64 {
        let d = self.difficulty();
        if d == 0.0 { 0.0 } else { 1. / d }
    }

    /// Returns the estimated effort required to program
    #[inline]
    #[must_use]
    pub fn effort(&self) -> f64 {
        self.difficulty() * self.volume()
    }

    /// Returns the estimated time required to program.
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

pub trait Halstead
where
    Self: Checker + Getter,
{
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
    match T::get_op_type(node) {
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
                .entry(get_id(node, code))
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

implement_metric_trait!(Halstead, PreprocCode, CcommentCode);

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
                assert_eq!(metric.halstead.u_operators(), 30.0);
                assert_eq!(metric.halstead.operators(), 118.0);
                assert_eq!(metric.halstead.u_operands(), 31.0);
                assert_eq!(metric.halstead.operands(), 50.0);
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
    fn csharp_operators_and_operands() {
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
                assert_eq!(metric.halstead.u_operators(), 13.0);
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
                assert_eq!(metric.halstead.u_operators(), 6.0);
                assert_eq!(metric.halstead.operators(), 33.0);
                assert_eq!(metric.halstead.u_operands(), 21.0);
                assert_eq!(metric.halstead.operands(), 23.0);
                insta::assert_json_snapshot!(metric.halstead);
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
                insta::assert_json_snapshot!(metric.halstead);
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
}
