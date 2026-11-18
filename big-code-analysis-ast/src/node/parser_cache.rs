//! One reusable `tree_sitter::Parser` per analysis thread.
//!
//! A `tree_sitter::Parser` owns a lexer, a GLR stack with its stack-node
//! pool, a subtree pool and several scratch arrays. `set_language` and
//! `parse` reset that state without releasing the capacity behind it, so
//! a parser that has handled one file starts the next with those buffers
//! already grown. Construction is *not* what costs; the re-grown buffers
//! are, and only modestly — see issue #1118 for the measurements.
//!
//! What a thread keeps between files is the subtree and stack-node
//! pools, which are capped at small fixed counts, plus the scratch
//! arrays around them, which are cleared without releasing capacity —
//! so a long-lived server holds the latter per worker for the process
//! lifetime. Those arrays track parse-stack depth and reduce-action
//! counts rather than input size, so this is deliberately not stated as
//! a bound in bytes: the "tens of KiB" figure that stood here was never
//! measured, and #1375 established only that the *input* can run to
//! tens of MB, not that the retention follows it.
//!
//! Only the parser is cached, not the language bound to it. Rebinding
//! per file costs nothing measurable — the gain survives consecutive
//! files alternating language — so a cache key whose staleness would
//! misparse a file buys nothing.

use std::cell::Cell;
use std::ops::ControlFlow;

use tree_sitter::{Language, ParseOptions, ParseState, Parser, Tree};

use crate::parser::ParseCompletion;

thread_local! {
    /// The parser this thread reuses across the files it analyzes.
    static SCRATCH_PARSER: Cell<Option<Parser>> = const { Cell::new(None) };
}

// Parsers built on this thread, so a test can observe that reuse
// actually happens. Every assertion on parse *output* holds just as well
// when the write-back in `parse_on_scratch_parser` is deleted, which
// would silently restore the one-parser-per-file behaviour #1118
// removed. The counter sits on the slot-miss path, which runs once per
// thread, so it costs nothing per file.
crate::observation::counter!(parsers_built);

/// Parses `code` under `language` on this thread's reusable parser.
///
/// The parser is moved out of the thread-local for the duration of the
/// parse and moved back afterwards, so a re-entrant call finds an empty
/// slot and builds its own parser rather than observing one mid-parse.
/// Both accesses go through `try_with`, which cannot panic: a parse
/// issued from another thread-local's destructor runs after this slot
/// may already have been destroyed, and there `LocalKey::with` panics —
/// a panic escaping a destructor aborts the process.
///
/// The returned tree is independent of the parser that produced it: it
/// owns its subtrees and its own handle on the grammar, and releases
/// them through a pool of its own (`ts_tree_delete`). It outlives both
/// the cached parser and the thread that built it.
///
/// Under a cancelled [`crate::cancel::ParseCancel`] the parse stops and
/// this returns the tree of an empty input with retained cancelled provenance.
/// The stop is a
/// per-call progress callback, not parser state; the parser is reset
/// after it, because a stopped parser resumes where it left off on the
/// next call. Nothing in this crate sets a timeout, a cancellation flag,
/// or included ranges, which *are* parser state: `set_language` does not
/// clear those, so anything that starts setting one must clear it before
/// the parser goes back.
pub(super) fn parse_on_scratch_parser(language: &Language, code: &[u8]) -> (Tree, ParseCompletion) {
    let mut parser = SCRATCH_PARSER
        .try_with(Cell::take)
        .ok()
        .flatten()
        .unwrap_or_else(build_parser);
    // `ts_parser_set_language` resets the parse state itself, so the
    // `Parser::reset` tree-sitter documents for reuse is redundant here.
    parser
        .set_language(language)
        .expect("invariant: grammar version is pinned and compatible with bundled tree-sitter");
    let result = if let Some(tree) = parse_unless_cancelled(&mut parser, code) {
        (tree, ParseCompletion::Complete)
    } else {
        parser.reset();
        let tree = parser
            .parse(b"", None)
            .expect("invariant: parser has a language set and nothing stops this parse");
        (tree, ParseCompletion::Cancelled)
    };
    // Dropped rather than cached when the slot is already gone.
    let _ = SCRATCH_PARSER.try_with(|slot| slot.set(Some(parser)));
    result
}

/// Parses `code`, or answers `None` once the [`crate::cancel`] flag
/// installed on this thread is cancelled. The runtime reads the callback
/// every hundred parse operations.
fn parse_unless_cancelled(parser: &mut Parser, code: &[u8]) -> Option<Tree> {
    let mut stop_if_cancelled = |_: &ParseState| {
        if crate::cancel::requested() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let options = ParseOptions::new().progress_callback(&mut stop_if_cancelled);
    parser.parse_with_options(
        &mut |offset, _| code.get(offset..).unwrap_or_default(),
        None,
        Some(options),
    )
}

fn build_parser() -> Parser {
    parsers_built::record();
    Parser::new()
}

// Every test here parses Rust, so the module is gated on that grammar's
// feature the same way `tests/api/parser_reuse.rs` is: without it
// `RustCode::lang().get_ts_language()` returns `None` and the whole
// module fails at `expect` rather than being skipped. CI's
// `no-default-features` matrix leg only runs `cargo check`, so this was
// red only for a human running `cargo test --no-default-features`.
#[cfg(all(test, feature = "rust"))]
mod tests {
    use super::*;
    use crate::LANG;
    use crate::langs::AnyParser;
    use crate::langs::RustCode;
    use crate::traits::LanguageInfo;

    fn rust_language() -> Language {
        RustCode::lang()
            .get_ts_language()
            .expect("rust is a default feature")
    }

    /// The guard for the optimization itself, not for its output.
    /// Deleting the write-back in `parse_on_scratch_parser` leaves every
    /// tree-comparison assertion in `tests/api/parser_reuse.rs` passing while
    /// reducing the cache to a no-op; only a construction count sees it.
    ///
    /// Runs on its own thread so the count starts from zero regardless of
    /// which other tests shared this one.
    #[test]
    fn many_parses_on_one_thread_build_one_parser() {
        const PARSES: usize = 8;

        std::thread::spawn(|| {
            assert_eq!(
                parsers_built::observed(),
                0,
                "a fresh thread must start with an empty slot"
            );
            let language = rust_language();
            for i in 0..PARSES {
                let (tree, _) = parse_on_scratch_parser(&language, b"fn f() { let x = 1; }");
                assert!(
                    !tree.root_node().to_sexp().contains("ERROR"),
                    "parse {i} must succeed"
                );
            }
            assert_eq!(
                parsers_built::observed(),
                1,
                "{PARSES} parses must share one parser"
            );
        })
        .join()
        .expect("parsing thread must not panic");
    }

    /// A cancelled flag stops the parse, which then yields the tree of an
    /// empty input, and the next parse on the thread reads its own input
    /// in full (#1939). The input is long enough that the runtime asks its
    /// progress callback — every hundred parse operations — before the
    /// parse could finish, so the stop lands mid-parse. That is what makes
    /// the reset observable: without it the empty-input parse resumes the
    /// abandoned one and does not answer `0..0`.
    #[test]
    fn a_cancelled_parse_yields_an_empty_tree_and_a_clean_parser() {
        std::thread::spawn(|| {
            let language = rust_language();
            let long = "fn f() { let x = 1; }\n".repeat(200);
            let cancel = crate::cancel::ParseCancel::new();
            cancel.cancel();
            let (stopped, completion) =
                cancel.run(|| parse_on_scratch_parser(&language, long.as_bytes()));
            assert_eq!(completion, ParseCompletion::Cancelled);
            assert_eq!(stopped.root_node().byte_range(), 0..0);
            assert_eq!(stopped.root_node().child_count(), 0);

            let code = b"fn g() { let y = 2; }";
            let (next, completion) = parse_on_scratch_parser(&language, code);
            assert_eq!(completion, ParseCompletion::Complete);
            let mut fresh = Parser::new();
            fresh
                .set_language(&language)
                .expect("the pinned grammar loads");
            let expected = fresh.parse(code, None).expect("an uncancelled parse");
            assert_eq!(next.root_node().byte_range(), 0..code.len());
            assert_eq!(next.root_node().to_sexp(), expected.root_node().to_sexp());
        })
        .join()
        .expect("parsing thread must not panic");
    }

    /// An installed flag nobody cancels changes nothing.
    #[test]
    fn an_uncancelled_flag_parses_in_full() {
        let language = rust_language();
        let code = "fn f() { let x = 1; }\n".repeat(200);
        let (tree, completion) = crate::cancel::ParseCancel::new()
            .run(|| parse_on_scratch_parser(&language, code.as_bytes()));
        assert_eq!(completion, ParseCompletion::Complete);
        assert_eq!(tree.root_node().byte_range(), 0..code.len());
        assert!(!tree.root_node().has_error());
    }

    /// Each thread carries its own parser: one thread's cache must not
    /// satisfy another's first parse.
    ///
    /// The threads run one at a time, and that is the point rather than an
    /// artefact of how the loop is written. *Consecutive* threads are what
    /// make a process-global cache observable: perturbing `SCRATCH_PARSER`
    /// into a `static Mutex<Option<Parser>>` fails this assertion
    /// deterministically with `[1, 0, 0]`, because the second thread finds
    /// the parser the first one handed back. Spawning all three before
    /// joining any of them weakens exactly that — under the same
    /// perturbation the counts become whatever the interleaving produced
    /// (`[1, 2, 1]`, `[2, 1, 0]`, … over eight measured runs), and a run
    /// where all three threads reach their first parse before any hands a
    /// parser back would report `[1, 1, 1]` and pass. The loop is spelled
    /// out so that property is not resting on iterator laziness.
    #[test]
    fn each_thread_builds_its_own_parser() {
        let mut counts = Vec::new();
        for _ in 0..3 {
            let handle = std::thread::spawn(|| {
                let language = rust_language();
                parse_on_scratch_parser(&language, b"fn f() {}");
                parse_on_scratch_parser(&language, b"fn g() {}");
                parsers_built::observed()
            });
            counts.push(handle.join().expect("parsing thread must not panic"));
        }

        assert_eq!(
            counts,
            vec![1, 1, 1],
            "each thread builds exactly one parser for its own files"
        );
    }

    /// The same guard one level up, at the seam production actually uses.
    ///
    /// The two tests above call `parse_on_scratch_parser` directly, so
    /// they say nothing about whether anything *reaches* it: reverting
    /// `Tree::new` to the pre-#1118 `Parser::new()`-per-file body leaves
    /// all 3,143 lib tests and all 5 `tests/api/parser_reuse.rs` integration
    /// tests passing (measured). Driving `AnyParser::parse` and counting
    /// constructions is what fails there.
    ///
    /// Before #1376 this drove `big_code_analysis::Ast::parse`. It stops
    /// one layer lower now because `parsers_built` lives in this private
    /// module and the root crate cannot read it — and widening the
    /// module to host a test is the trade `container_scope_tests`
    /// declines for the same reason. Nothing is lost that matters:
    /// `Ast::parse` is a one-line delegate to `AnyParser::parse` and
    /// owns no parser, so this is the lowest layer at which reuse is
    /// observable at all.
    #[test]
    fn repeated_parses_through_the_dispatch_seam_share_one_parser() {
        const FILES: usize = 6;

        std::thread::spawn(|| {
            for i in 0..FILES {
                // Distinct sources, so no caching layer above the parser
                // can turn the later parses into lookups.
                let code = format!("fn f{i}() {{ let x = {i}; }}");
                let parsed = AnyParser::parse(LANG::Rust, code.into_bytes(), None, None)
                    .expect("rust is enabled for this module");
                let sexp = parsed.ts_tree().root_node().to_sexp();
                assert!(
                    sexp.contains("function_item") && !sexp.contains("ERROR"),
                    "file {i} must parse to a real tree, got {sexp}"
                );
            }
            assert_eq!(
                parsers_built::observed(),
                1,
                "{FILES} files parsed through `AnyParser::parse` must share one parser"
            );
        })
        .join()
        .expect("parsing thread must not panic");
    }
}
