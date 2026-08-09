//! Shared harness behind every `big-code-analysis` fuzz target.
//!
//! The targets themselves are five lines each; everything they do lives
//! here, so a change to what gets exercised is one edit rather than ten.
//!
//! # Why this shape
//!
//! Two design points are load-bearing and neither is obvious from the
//! code alone.
//!
//! **The bytes reach the parser unmodified.** [`Source::from_bytes`]
//! applies no normalisation, which is the whole point: the file-reading
//! path (`read_file`, `read_file_with_eol`, `normalize_eol`) runs
//! `normalize_line_endings` first, and that guarantees a trailing `\n`.
//! A node ending at EOF is therefore unreachable through it — which is
//! exactly how #1051 (a `usize` underflow on a Rust doc comment at EOF)
//! survived the test suite. A harness that normalised first would pass
//! vacuously while looking like coverage.
//!
//! **One parse, every walk.** `ops`, `dump` and comment removal are not
//! separate entry points; they are seams on [`Ast`] over a tree that is
//! already built. Giving each its own target would triple the build and
//! spend three quarters of the fuzzing budget re-parsing. libFuzzer's
//! coverage feedback is edge-based across the whole binary, so a new
//! edge inside comment removal registers just as well from here.
//!
//! Serialising the results is not decoration. `FuncSpace`, `Ops` and
//! `AstResponse` are caller-controlled trees whose `Serialize` impls
//! recurse once per level, and bounding that recursion is what #1056
//! fixed. Nothing else in the fan-out reaches
//! `recursion::serialize_bounded` or `wire::map_tree`.
//!
//! # What is deliberately not called
//!
//! Two public entry points document panic preconditions a fuzzer would
//! violate on its first iteration, manufacturing crashers that say
//! nothing about the library:
//!
//! - `Ast::from_tree_sitter` panics when the tree was built from longer
//!   source than the `code` handed alongside it.
//! - `dump_node` panics unless `code` is the exact source the node was
//!   parsed from.
//!
//! `Ast::from_path` is excluded for the different reason above: it
//! normalises.

use std::hint::black_box;
use std::sync::LazyLock;

use big_code_analysis::{Ast, AstCfg, LANG, MetricsOptions, Source};

pub mod nested;

/// Node-kind filters handed to [`Ast::count`] and [`Ast::find`].
///
/// `"function"` earns its place twice over: it is the only filter that
/// reaches a `Checker` predicate taking `code`, and it applies that
/// predicate with an unknown ancestor chain, which climbs by
/// `Node::parent` at `O(depth^2)` per candidate node. That makes it the
/// one filter a deeply-nested input can turn into a complexity problem.
///
/// **The order is load-bearing, and `"all"` must stay last.**
/// `Filter::any` returns on its first matching predicate, and `"all"` is
/// `|_| true`, so listing it first makes every other entry unreachable —
/// `is_call`, `is_comment`, `is_error`, `is_string` and
/// `is_func_with_code` are then never called on any node, in any target.
/// It was first here until a review caught it, which had quietly reduced
/// `count` and `find` to a bare walk and left the `"function"` predicate
/// above — the whole reason the nesting generator is sized the way it
/// is — dead. Kept rather than dropped because it still makes every node
/// match once the real predicates have each had their say, so `find`
/// builds a maximal result vector.
///
/// Built once rather than per call: both seams take `&[String]`, and
/// this list never varies, so rebuilding it would put six heap
/// allocations in the innermost loop of every target — millions of them
/// across a run, none of them testing anything.
static FILTERS: LazyLock<Vec<String>> = LazyLock::new(|| {
    ["call", "comment", "error", "string", "function", "all"]
        .into_iter()
        .map(str::to_owned)
        .collect()
});

/// Parse `data` as `lang` and run every walk over the result.
///
/// A disabled language is skipped rather than counted as an error: with
/// a feature set that omits it, every input would fail identically at
/// the dispatcher and the target would fuzz nothing.
pub fn walk_all(lang: LANG, data: Vec<u8>) {
    if !lang.is_enabled() {
        return;
    }
    let Ok(ast) = Ast::parse(Source::from_bytes(lang, data)) else {
        return;
    };
    walk_parsed(&ast);
}

/// Run every [`Ast`] walk over an already-parsed tree.
///
/// Split out from [`walk_all`] so a target that builds its own
/// [`Source`] — the preprocessor one has to — reuses the same fan-out
/// rather than keeping a second copy of it in step.
pub fn walk_parsed(ast: &Ast) {
    // Each result is handed to `black_box` whole rather than reduced to
    // a bool or a length first: the reduction would let the optimizer
    // drop the value the walk produced while still looking like a
    // barrier, which is the one thing a fuzz harness must not do.
    if let Ok(space) = ast.metrics(MetricsOptions::default()) {
        let _ = black_box(serde_json::to_vec(&space));
    }
    if let Ok(ops) = ast.ops() {
        let _ = black_box(serde_json::to_vec(&ops.to_wire()));
    }

    black_box(ast.strip_comments());

    // `AstCfg::comment` is a *suppression* flag — `true` means "nodes
    // representing comments are ignored" — so `false` is what keeps
    // comment nodes in the dump and runs the alterator's span and text
    // extraction over them. Every per-language seed carries a comment
    // precisely for that path, and #1051 was span arithmetic on a Rust
    // doc comment. `Checker::is_comment` is still exercised, through the
    // `"comment"` entry in `FILTERS`.
    let dump = ast.dump(AstCfg {
        id: String::new(),
        language: ast.language().name().to_owned(),
        comment: false,
        span: true,
    });
    let _ = black_box(serde_json::to_vec(&dump));

    black_box(ast.functions());
    black_box(ast.suppressions());

    black_box(ast.count(&FILTERS));
    if let Ok(nodes) = ast.find(&FILTERS) {
        black_box(nodes);
    }
}
