//! Structured generator for the deep-nesting complexity class.
//!
//! A byte mutator reaches `((((((…))))))` only by accident, and the
//! interesting depths here are in the hundreds. This module builds the
//! shape directly: pick a language, pick a depth, pick a repeating
//! sequence of nesting constructs, and emit them around a leaf.
//!
//! # Why the constructs are valid source
//!
//! Every open/close pair below nests around an *expression* and leaves
//! the result parseable. That is not politeness — tree-sitter's error
//! recovery flattens badly-formed input, so an invalid generator would
//! produce a shallow tree with a large `ERROR` node and quietly stop
//! testing depth at all. Malformed bytes are already covered by the
//! per-language targets, which mutate freely.
//!
//! # What depth is for
//!
//! Two bounds sit on this path, both added by #1056 after nested input
//! was found able to abort the process from `Serialize`'s implicit
//! recursion (a stack overflow is `SIGABRT`, not a catchable panic):
//! `MAX_SPACE_SERIALIZE_DEPTH` (128) on the `FuncSpace` tree and
//! `MAX_AST_SERIALIZE_DEPTH` (512) on the `AstNode` tree. Reaching past
//! both is the point of [`MAX_NESTING_DEPTH`]. The lambda shapes are
//! what drive the `FuncSpace` bound specifically — a plain paren nests
//! the AST without opening a function space.

use arbitrary::Arbitrary;
use big_code_analysis::LANG;

/// Greatest number of nesting constructs a generated input may carry.
///
/// Chosen as `MAX_AST_SERIALIZE_DEPTH` (512), which is the larger of the
/// two serialization bounds and four times the `FuncSpace` one. Every
/// construct below contributes at least one AST level and most
/// contribute two or three, so this clears both.
///
/// It is also an upper bound on run time, which is why it is not simply
/// set enormous. The `"function"` filter applies its predicate with an
/// unknown ancestor chain, climbing by `Node::parent` at `O(depth^2)`
/// per candidate node; with a candidate at every level that is cubic in
/// this constant. At 512 the worst case stays comfortably inside the
/// `-timeout=10` the fuzz runs use, so a timeout report means a real
/// complexity regression rather than the generator outgrowing the
/// budget.
pub const MAX_NESTING_DEPTH: usize = 512;

/// Languages the generator knows how to nest.
///
/// A subset of the fuzzed set: each entry needs a hand-written table of
/// constructs, and these four span the interesting variation — braces
/// versus indentation, and three different lambda spellings.
#[derive(Arbitrary, Debug, Clone, Copy)]
pub enum NestLang {
    /// Rust: block expressions nest, so it has the widest shape table.
    Rust,
    /// C++: the immediately-invoked lambda is the only brace form that
    /// nests as an expression.
    Cpp,
    /// JavaScript: object literals and arrow functions both nest.
    Javascript,
    /// Python: expression nesting only, since indentation-based blocks
    /// would make the generated source quadratic in the depth.
    Python,
}

/// One nesting construct, rendered per language by [`Nesting::pair`].
#[derive(Arbitrary, Debug, Clone, Copy)]
pub enum Shape {
    /// Parenthesised expression — the cheapest AST level available.
    Paren,
    /// A call around the inner expression.
    Call,
    /// An indexing or collection-literal bracket.
    Bracket,
    /// A lambda or closure. The only shape that opens a function space,
    /// so the only one that drives the `FuncSpace` serialization bound.
    Lambda,
    /// A brace-delimited expression where the language has one; falls
    /// back to [`Shape::Paren`] where it does not.
    Block,
}

/// A generated deeply-nested input.
#[derive(Arbitrary, Debug)]
pub struct Nesting {
    /// Which language's syntax to emit. Private so it does not collide
    /// with the [`Nesting::lang`] accessor, which returns the library's
    /// `LANG` rather than this generator's smaller enum.
    lang: NestLang,
    /// Raw depth request, reduced into `1..=MAX_NESTING_DEPTH`.
    depth: u16,
    /// Constructs to cycle through while descending. An empty sequence
    /// falls back to a single [`Shape::Paren`].
    shapes: Vec<Shape>,
}

impl Nesting {
    /// The `big-code-analysis` language this input should be parsed as.
    #[must_use]
    pub fn lang(&self) -> LANG {
        match self.lang {
            NestLang::Rust => LANG::Rust,
            NestLang::Cpp => LANG::Cpp,
            NestLang::Javascript => LANG::Javascript,
            NestLang::Python => LANG::Python,
        }
    }

    /// Render the nested source.
    #[must_use]
    pub fn render(&self) -> Vec<u8> {
        let depth = usize::from(self.depth) % MAX_NESTING_DEPTH + 1;
        let fallback = [Shape::Paren];
        let shapes: &[Shape] = if self.shapes.is_empty() {
            &fallback
        } else {
            &self.shapes
        };

        let (prologue, epilogue) = self.wrapper();
        let mut out = Vec::from(prologue);
        let mut closers = Vec::with_capacity(depth);

        // Cycle the sequence rather than truncating the nest to its
        // length: a two-element `shapes` still nests 512 deep, and a
        // long one is consumed in full.
        for shape in shapes.iter().copied().cycle().take(depth) {
            let (open, close) = self.pair(shape);
            out.extend_from_slice(open);
            closers.push(close);
        }

        out.extend_from_slice(Self::LEAF);
        for close in closers.iter().rev() {
            out.extend_from_slice(close);
        }
        out.extend_from_slice(epilogue);
        out
    }

    /// Source that makes the nest a complete compilation unit.
    fn wrapper(&self) -> (&'static [u8], &'static [u8]) {
        match self.lang {
            NestLang::Rust => (b"fn main() { let _x = ", b"; }\n"),
            NestLang::Cpp => (b"int main() { auto x = ", b"; }\n"),
            NestLang::Javascript => (b"let x = ", b";\n"),
            NestLang::Python => (b"x = ", b"\n"),
        }
    }

    /// The innermost expression. An integer literal is a valid
    /// expression in all four languages, so this needs no per-language
    /// arm.
    const LEAF: &'static [u8] = b"1";

    /// The open/close byte pair for `shape` in this input's language.
    ///
    /// Where a language has no expression form for a shape the arm falls
    /// through to the parenthesised one rather than emitting something
    /// unparseable — see the module docs on why validity matters here.
    //
    // `match_same_arms` fires because several languages spell a shape
    // identically — `f(`/`)` is a call in all four. Merging them is what
    // the lint asks for and is the wrong move here: the table's whole
    // value is that a reader can check at a glance that every shape is
    // spelled for every language, and a merged arm hides which languages
    // are covered behind an or-pattern that grows with each new one.
    // Function-level rather than file-level, so a future function in
    // this module is linted by default.
    #[allow(clippy::match_same_arms)]
    fn pair(&self, shape: Shape) -> (&'static [u8], &'static [u8]) {
        // bca: suppress(cyclomatic) — a syntax table, one arm per
        // (language, shape) pair. Splitting it per language would hide
        // the one thing a reader comes here to check: that every shape
        // is spelled for every language.
        match (self.lang, shape) {
            (NestLang::Rust, Shape::Paren) => (b"(", b")"),
            (NestLang::Rust, Shape::Call) => (b"f(", b")"),
            (NestLang::Rust, Shape::Bracket) => (b"[", b"]"),
            (NestLang::Rust, Shape::Lambda) => (b"(|| ", b")()"),
            (NestLang::Rust, Shape::Block) => (b"{", b"}"),

            (NestLang::Cpp, Shape::Paren | Shape::Block) => (b"(", b")"),
            (NestLang::Cpp, Shape::Call) => (b"f(", b")"),
            (NestLang::Cpp, Shape::Bracket) => (b"std::array{", b"}[0]"),
            (NestLang::Cpp, Shape::Lambda) => (b"[]{ return ", b"; }()"),

            (NestLang::Javascript, Shape::Paren) => (b"(", b")"),
            (NestLang::Javascript, Shape::Call) => (b"f(", b")"),
            (NestLang::Javascript, Shape::Bracket) => (b"[", b"][0]"),
            (NestLang::Javascript, Shape::Lambda) => (b"(() => ", b")()"),
            (NestLang::Javascript, Shape::Block) => (b"{ a: ", b" }.a"),

            (NestLang::Python, Shape::Paren | Shape::Block) => (b"(", b")"),
            (NestLang::Python, Shape::Call) => (b"f(", b")"),
            (NestLang::Python, Shape::Bracket) => (b"[", b"][0]"),
            (NestLang::Python, Shape::Lambda) => (b"(lambda: ", b")()"),
        }
    }
}

#[cfg(test)]
mod tests {
    use big_code_analysis::{Ast, AstCfg, LANG, MetricsOptions, Source};

    use super::{MAX_NESTING_DEPTH, NestLang, Nesting, Shape};

    /// Build a `Nesting` directly. `Arbitrary` is how the fuzzer makes
    /// one; these tests need a specific depth and shape, and the fields
    /// are reachable from inside the module.
    fn nesting(lang: NestLang, depth: u16, shapes: &[Shape]) -> Nesting {
        Nesting {
            lang,
            depth,
            shapes: shapes.to_vec(),
        }
    }

    /// `MAX_NESTING_DEPTH` is chosen to clear `MAX_AST_SERIALIZE_DEPTH`,
    /// and this is the measurement rather than the hope: at the maximum
    /// depth the `AstNode` tree must be deep enough that the bounded
    /// `Serialize` impl refuses it. If this ever starts passing,
    /// `nested_depth` has stopped reaching the class it exists for and
    /// the constant needs raising — the target would still run, and
    /// would still look like coverage.
    #[test]
    fn max_depth_exceeds_the_ast_serialize_bound() {
        // `depth` is reduced modulo the cap, so `MAX_NESTING_DEPTH` maps
        // to 1. The value one below it is the deepest reachable nest.
        let deepest = u16::try_from(MAX_NESTING_DEPTH - 1).expect("cap fits in u16");
        let source = nesting(NestLang::Rust, deepest, &[Shape::Paren]).render();

        let ast = Ast::parse(Source::from_bytes(LANG::Rust, source)).expect("rust is enabled");
        let dump = ast.dump(AstCfg {
            id: String::new(),
            language: LANG::Rust.name().to_owned(),
            comment: true,
            span: true,
        });
        assert!(
            serde_json::to_vec(&dump).is_err(),
            "the deepest generated nest did not reach MAX_AST_SERIALIZE_DEPTH"
        );
    }

    /// The same measurement for the `FuncSpace` bound, which only the
    /// lambda shapes drive: a paren nests the AST without opening a
    /// function space, so a `Shape::Paren` nest would leave this bound
    /// untouched however deep it went.
    #[test]
    fn lambda_shapes_exceed_the_space_serialize_bound() {
        let deepest = u16::try_from(MAX_NESTING_DEPTH - 1).expect("cap fits in u16");
        let source = nesting(NestLang::Rust, deepest, &[Shape::Lambda]).render();

        let space = Ast::parse(Source::from_bytes(LANG::Rust, source))
            .expect("rust is enabled")
            .metrics(MetricsOptions::default())
            .expect("walker succeeds");
        assert!(
            serde_json::to_vec(&space).is_err(),
            "the deepest lambda nest did not reach MAX_SPACE_SERIALIZE_DEPTH"
        );
    }

    /// A short `shapes` sequence must still nest to the requested depth
    /// rather than stopping at its length — the cycling in `render`. A
    /// truncated nest is the silent-vacuity failure mode: the target
    /// still runs, and reaches nothing.
    // `naive_bytecount` wants the `bytecount` crate. Pulling a
    // dependency into a fuzz crate to speed up a byte tally over a few
    // hundred bytes, in a test, is not a trade worth making.
    #[allow(clippy::naive_bytecount)]
    #[test]
    fn a_short_shape_sequence_still_nests_to_the_requested_depth() {
        const RAW: u16 = 100;
        // `render` reduces the raw request into `1..=MAX_NESTING_DEPTH`,
        // so a raw 100 asks for 101 levels. Spelling the reduction out
        // rather than hard-coding 101 keeps the test honest if the cap
        // or the reduction changes.
        const LEVELS: usize = RAW as usize % MAX_NESTING_DEPTH + 1;
        // Both shapes open exactly one paren — `(` and `f(` — so every
        // level contributes one, and `fn main()` in the Rust wrapper
        // contributes one more pair of its own.
        const WRAPPER_PARENS: usize = 1;
        const EXPECTED: usize = LEVELS + WRAPPER_PARENS;

        let source = nesting(NestLang::Rust, RAW, &[Shape::Paren, Shape::Call]).render();
        assert_eq!(source.iter().filter(|b| **b == b'(').count(), EXPECTED);
        assert_eq!(source.iter().filter(|b| **b == b')').count(), EXPECTED);
    }
}
