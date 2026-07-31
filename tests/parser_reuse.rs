//! Integration tests for the per-thread parser reuse behind
//! `Tree::new` (#1118).
//!
//! `Tree::new` no longer builds a `tree_sitter::Parser` per file; it
//! borrows one from a thread-local slot, rebinds the grammar, parses,
//! and puts the parser back. Everything that can go wrong with that is
//! invisible in the metric values of a single file and only shows up
//! across a *sequence* of parses on one thread, so every test here
//! parses more than once and compares against a reference parser built
//! fresh for that one input:
//!
//! 1. **No stale grammar.** The slot caches the parser but not the
//!    language bound to it, so `set_language` runs on every parse.
//!    Alternating languages on one thread is what would catch a
//!    regression that started skipping it.
//! 2. **No stale parse state.** A parser that just recovered from a
//!    syntax error must produce the same tree for the next file as a
//!    parser that has never been used.
//! 3. **Per-thread isolation, and trees that outlive their parser.**
//!    A `tree_sitter::Tree` owns its subtrees and its own grammar
//!    handle, so it must survive both the cached parser and the thread
//!    that produced it.
//! 4. **No panic during thread-local teardown.** A parse issued from
//!    another thread-local's destructor may find the parser slot
//!    already destroyed; it must fall back to a fresh parser rather
//!    than panicking, which `LocalKey::with` would.
//!
//! Language-specific tests are gated on their Cargo feature so the
//! minimal-langs CI entry (`--no-default-features --features
//! rust,typescript`) still compiles and runs.

#[cfg(all(feature = "rust", feature = "typescript"))]
mod parser_reuse {
    use std::sync::atomic::{AtomicBool, Ordering};

    use big_code_analysis::{Ast, LANG, Source, tree_sitter};

    const RUST_SRC: &str = r#"
fn classify(n: i32) -> &'static str {
    if n < 0 {
        "negative"
    } else if n == 0 {
        "zero"
    } else {
        "positive"
    }
}

struct Point { x: f64, y: f64 }

impl Point {
    fn norm(&self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}
"#;

    const TS_SRC: &str = r"
function classify(n: number): string {
    if (n < 0) {
        return 'negative';
    } else if (n === 0) {
        return 'zero';
    }
    return 'positive';
}

class Point {
    constructor(readonly x: number, readonly y: number) {}
    norm(): number {
        return Math.sqrt(this.x * this.x + this.y * this.y);
    }
}
";

    /// Rust source the grammar cannot parse cleanly. Error recovery is
    /// what leaves the most state behind on a parser, so this is the
    /// worst thing to have parsed just before the file under test.
    const BROKEN_SRC: &str = "fn oops( { let ] = ; if while }} impl for 42";

    /// Parses `code` on a parser built for this one call, bypassing the
    /// thread-local slot entirely. This is the oracle every assertion
    /// below compares against — comparing two `Ast::parse` results to
    /// each other would pass even if both were wrong.
    fn reference_sexp(lang: LANG, code: &str) -> String {
        let language = lang
            .tree_sitter_language()
            .expect("language feature enabled");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language)
            .expect("pinned grammar is compatible");
        let tree = parser
            .parse(code.as_bytes(), None)
            .expect("language is set, no cancellation");
        tree.root_node().to_sexp()
    }

    /// Parses `code` through the public seam, which routes to the
    /// thread-local parser.
    fn cached_sexp(lang: LANG, code: &str) -> String {
        let ast = Ast::parse(Source::new(lang, code.as_bytes())).expect("language feature enabled");
        ast.as_tree_sitter().root_node().to_sexp()
    }

    /// The fixture a language is exercised with. Single source of truth:
    /// the reference tree and the tree under test must come from the same
    /// bytes, and selecting them at two separate sites is how they drift.
    fn fixture(lang: LANG) -> &'static str {
        if lang == LANG::Rust { RUST_SRC } else { TS_SRC }
    }

    /// A tree is only evidence if it actually has structure in it — a
    /// grammar that failed to bind would yield a tiny ERROR tree, and
    /// every "identical to the reference" assertion would still hold if
    /// the reference were equally broken.
    fn assert_parsed_cleanly(sexp: &str, what: LANG) {
        assert!(
            !sexp.contains("ERROR") && !sexp.contains("MISSING"),
            "{what}: fixture must parse without errors, got {sexp}"
        );
        // Both fixtures define a function, and every grammar here names
        // that node with a `function`-prefixed kind. A structural check
        // beats a length threshold: it stays meaningful if the fixtures
        // shrink, and it fails loudly if a fixture degenerates to a bare
        // ERROR node whose reference would be equally broken.
        assert!(
            sexp.contains("function"),
            "{what}: expected a function node in the tree, got {sexp}"
        );
    }

    #[test]
    fn cached_parser_matches_a_fresh_parser_per_language() {
        for lang in [LANG::Rust, LANG::Typescript] {
            let reference = reference_sexp(lang, fixture(lang));
            assert_parsed_cleanly(&reference, lang);
            assert_eq!(
                cached_sexp(lang, fixture(lang)),
                reference,
                "{lang}: cached parser must produce the reference tree"
            );
        }
    }

    /// The test that would catch a stale-grammar regression: if
    /// `set_language` were skipped when the slot already held a parser,
    /// the second language in each pair would be parsed under the first
    /// language's grammar.
    #[test]
    fn alternating_languages_on_one_thread_stay_correct() {
        let rust_reference = reference_sexp(LANG::Rust, fixture(LANG::Rust));
        let ts_reference = reference_sexp(LANG::Typescript, fixture(LANG::Typescript));
        assert_parsed_cleanly(&rust_reference, LANG::Rust);
        assert_parsed_cleanly(&ts_reference, LANG::Typescript);
        assert_ne!(
            rust_reference, ts_reference,
            "the two fixtures must be distinguishable, or alternating proves nothing"
        );

        // Several rounds: the first parse on this thread populates the
        // slot, so a stale-grammar bug could only appear from the second
        // parse onwards.
        for round in 0..4 {
            assert_eq!(
                cached_sexp(LANG::Rust, RUST_SRC),
                rust_reference,
                "round {round}: rust after typescript"
            );
            assert_eq!(
                cached_sexp(LANG::Typescript, TS_SRC),
                ts_reference,
                "round {round}: typescript after rust"
            );
        }
    }

    /// The test that would catch parse state surviving between files:
    /// a failed parse must not colour the next one.
    #[test]
    fn parse_state_does_not_survive_between_files() {
        let reference = reference_sexp(LANG::Rust, fixture(LANG::Rust));
        assert_parsed_cleanly(&reference, LANG::Rust);

        // Seed the thread's parser with a parse that ends in error
        // recovery, which is the state most likely to leak forward.
        let broken = cached_sexp(LANG::Rust, BROKEN_SRC);
        assert!(
            broken.contains("ERROR"),
            "the broken fixture must actually fail to parse, got {broken}"
        );

        for round in 0..4 {
            assert_eq!(
                cached_sexp(LANG::Rust, RUST_SRC),
                reference,
                "round {round}: clean parse after a failed one"
            );
            // Re-dirty the parser before the next round so every
            // iteration starts from recovered-from-error state.
            let _ = cached_sexp(LANG::Rust, BROKEN_SRC);
        }
    }

    /// Each thread has its own slot, and the first parse on a thread
    /// takes the build-a-parser branch while later ones take the reuse
    /// branch. Both are exercised here, on threads that interleave
    /// languages so no thread can rely on another's binding.
    #[test]
    fn threads_are_isolated_and_trees_outlive_their_thread() {
        let rust_reference = reference_sexp(LANG::Rust, fixture(LANG::Rust));
        let ts_reference = reference_sexp(LANG::Typescript, fixture(LANG::Typescript));
        assert_parsed_cleanly(&rust_reference, LANG::Rust);
        assert_parsed_cleanly(&ts_reference, LANG::Typescript);

        let mut handles = Vec::new();
        for id in 0..8 {
            // Half the threads lead with Rust and half with TypeScript,
            // so the language a thread sees first differs from its
            // neighbours'.
            let (first, second) = if id % 2 == 0 {
                (LANG::Rust, LANG::Typescript)
            } else {
                (LANG::Typescript, LANG::Rust)
            };
            handles.push(std::thread::spawn(move || {
                let mut trees = Vec::new();
                for _ in 0..8 {
                    for lang in [first, second] {
                        let src = fixture(lang);
                        trees.push((
                            lang,
                            Ast::parse(Source::new(lang, src.as_bytes())).expect("feature enabled"),
                        ));
                    }
                }
                // Returned across the join: every `Ast` here outlives
                // both the thread's cached parser and the thread itself.
                trees
            }));
        }

        for handle in handles {
            let trees = handle.join().expect("worker thread must not panic");
            assert_eq!(trees.len(), 16);
            // Read the trees only *after* the producing thread has been
            // joined and torn down, so a tree that depended on its
            // parser would be reading freed memory here.
            for (lang, ast) in trees {
                let expected = if lang == LANG::Rust {
                    &rust_reference
                } else {
                    &ts_reference
                };
                assert_eq!(
                    &ast.as_tree_sitter().root_node().to_sexp(),
                    expected,
                    "{lang}: tree must still be valid after its thread exited"
                );
            }
        }
    }

    thread_local! {
        /// Parses from its destructor, which runs during thread teardown
        /// — possibly after the parser slot has already been destroyed.
        static TEARDOWN_PROBE: ParseOnDrop = const { ParseOnDrop };
    }

    /// Set by `ParseOnDrop::drop`, checked after the thread is joined.
    static TEARDOWN_PARSE_OK: AtomicBool = AtomicBool::new(false);

    struct ParseOnDrop;

    impl Drop for ParseOnDrop {
        fn drop(&mut self) {
            // Must not panic. Whether this takes the "slot already
            // destroyed" fallback or still finds a live slot depends on
            // the platform's thread-local destructor ordering, so the
            // assertion is on the result, which holds either way.
            //
            // It does take the fallback on Linux/glibc: swapping the
            // production `try_with` for `with` turns this test into
            // `fatal runtime error: thread local panicked on drop,
            // aborting` — an uncatchable SIGABRT, not a test failure.
            // Recorded rather than asserted: a panic escaping a
            // thread-local destructor aborts the process, which would
            // report as a crashed binary instead of a named test
            // failure. The check happens after `join` below.
            let sexp = cached_sexp(LANG::Rust, RUST_SRC);
            TEARDOWN_PARSE_OK.store(sexp.contains("function_item"), Ordering::SeqCst);
        }
    }

    /// A parse issued while thread-locals are being destroyed must not
    /// panic, however the platform orders the destructors.
    #[test]
    fn parsing_during_thread_local_teardown_does_not_panic() {
        std::thread::spawn(|| {
            // Register the probe's destructor *before* the parser slot
            // is initialised, so on a platform that runs destructors in
            // reverse registration order the probe parses after the
            // slot is gone.
            TEARDOWN_PROBE.with(|_| ());
            let _ = cached_sexp(LANG::Rust, RUST_SRC);
        })
        .join()
        .expect("thread-local teardown must not panic");

        assert!(
            TEARDOWN_PARSE_OK.load(Ordering::SeqCst),
            "the destructor's parse must have produced a real tree; \
             a `false` here means it ran but returned no function node"
        );
    }
}
