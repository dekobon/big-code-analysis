//! Integration tests for the public [`Ast`] parse-once seam (#264).
//!
//! Two correctness guarantees underpin every test here:
//!
//! 1. **Concrete metric values flow end-to-end.** `Ast::parse` /
//!    `Ast::from_tree_sitter` must produce a [`FuncSpace`] whose
//!    headline metrics match a hand-computed expected value for the
//!    fixture source — not just match whatever the one-shot entry
//!    points happen to produce. Asserting against `analyze` here
//!    would be tautological because the production code makes
//!    `analyze` delegate to `Ast::parse(s)?.metrics(o)`.
//! 2. **Reuse is real.** A single `Ast` must support repeated
//!    [`Ast::metrics`] calls with different
//!    [`MetricsOptions::with_only`] selections; the resulting `FuncSpace`
//!    objects must carry only the requested metric families (and their
//!    declared dependencies).
//!
//! Per-language smoke tests are gated on the relevant Cargo feature so
//! the minimal-langs CI matrix entry
//! (`--no-default-features --features rust,typescript`) still compiles
//! and runs.

#![allow(clippy::float_cmp)]

use std::path::PathBuf;

#[cfg(not(feature = "javascript"))]
use big_code_analysis::MetricsError;
use big_code_analysis::{Ast, LANG, Metric, MetricsOptions, Source, SpaceKind};

// ----- End-to-end smoke tests, per language ------------------------------
//
// These previously asserted parity between `Ast::parse(s)?.metrics(o)` and
// `analyze(s, o)`, but since the production refactor makes the latter a
// thin delegate of the former (`src/spaces.rs`: `analyze` is literally
// `Ast::parse(source)?.metrics(options)`), that comparison reduced to
// `x == x` and would have passed under any mutation that broke both
// paths identically. The tests now anchor on concrete expected values
// per [`docs/development/lessons_learned.md`] lesson 2 — a mutation that
// drops the name, ignores `options.metrics`, or miscounts CCN would
// flip at least one of these assertions.

#[cfg(feature = "rust")]
#[test]
fn parse_then_metrics_rust_matches_hand_computed_values() {
    // `fn f` has CCN base 1 + the `if` branch = 2; rolled up under the
    // synthetic Unit FuncSpace the file-level base of 1 adds → 3.
    // Cognitive complexity of one non-nested `if` is 1 on the function
    // and 1 propagated up = 2.
    let source = b"fn f(x: i32) -> i32 { if x > 0 { 1 } else { -1 } }".as_slice();
    let space =
        Ast::parse(Source::new(LANG::Rust, source).with_name(Some("snippet.rs".to_owned())))
            .expect("rust feature enabled")
            .metrics(MetricsOptions::default())
            .expect("walker succeeds");

    assert_eq!(space.name.as_deref(), Some("snippet.rs"));
    assert_eq!(space.kind, SpaceKind::Unit);
    assert_eq!(space.metrics.cyclomatic.cyclomatic_sum(), 3.0);
    assert_eq!(space.metrics.cognitive.cognitive_sum(), 2.0);
}

#[cfg(feature = "python")]
#[test]
fn parse_then_metrics_python_matches_hand_computed_values() {
    // Class `C` with one method `m` containing one `if`. Python rolls
    // every scope (file + class + method) through the CCN sum, so
    // method-1 + class-1 + file-1 + if-1 = 4. NOM counts the single
    // user-defined function.
    let source =
        b"class C:\n    def m(self, x):\n        if x:\n            return 1\n        return 0\n"
            .as_slice();
    let space = Ast::parse(Source::new(LANG::Python, source))
        .expect("python feature enabled")
        .metrics(MetricsOptions::default())
        .expect("walker succeeds");

    assert_eq!(space.metrics.cyclomatic.cyclomatic_sum(), 4.0);
    assert_eq!(space.metrics.nom.functions_sum(), 1.0);
}

#[cfg(feature = "cpp")]
#[test]
fn parse_then_metrics_cpp_with_preproc_matches_hand_computed_values() {
    use std::collections::HashMap;
    use std::sync::Arc;

    use big_code_analysis::{PreprocFile, PreprocResults};

    // `DBG` is declared as a macro, so `c_macro::replace` runs before
    // the parser sees the source and substitutes `DBG` → `$$$`. The
    // resulting `int f(int x) { return $$$ ? x : 0; }` still has one
    // ternary, so cyclomatic_sum = file-1 + fn-1 + ?:-1 = 3.
    let source = b"int f(int x) { return DBG ? x : 0; }".as_slice();
    let path = PathBuf::from("foo.c");
    let files = HashMap::from([(path.clone(), PreprocFile::new_macros(&["DBG"]))]);
    let pr = Arc::new(PreprocResults { files });

    let space = Ast::parse(
        Source::new(LANG::Cpp, source)
            .with_name(Some("foo.c".to_owned()))
            .with_preproc_path(Some(&path))
            .with_preproc(Some(pr)),
    )
    .expect("cpp feature enabled")
    .metrics(MetricsOptions::default())
    .expect("walker succeeds");

    assert_eq!(space.name.as_deref(), Some("foo.c"));
    assert_eq!(space.metrics.cyclomatic.cyclomatic_sum(), 3.0);
}

// ----- Reuse: two with_only calls against the same parse -----------------

#[cfg(feature = "rust")]
#[test]
fn metrics_can_be_recomputed_with_different_selections() {
    let source_bytes = b"fn f(x: i32) -> i32 { if x > 0 { 1 } else { -1 } }".as_slice();
    let ast = Ast::parse(Source::new(LANG::Rust, source_bytes)).expect("rust feature enabled");

    let loc_only = ast
        .metrics(MetricsOptions::default().with_only(&[Metric::Loc]))
        .expect("walker succeeds");
    let cyc_only = ast
        .metrics(MetricsOptions::default().with_only(&[Metric::Cyclomatic]))
        .expect("walker succeeds");

    // `Loc` was requested in the first call, `Cyclomatic` in the
    // second. Each call's `FuncSpace` should carry a populated value
    // for the requested family and a `Default` (zero) for the other —
    // confirms `MetricsOptions::with_only` is honored per call rather
    // than carried over from a previous walk.
    assert!(loc_only.metrics.loc.ploc() > 0.0);
    assert_eq!(loc_only.metrics.cyclomatic.cyclomatic_sum(), 0.0);

    assert!(cyc_only.metrics.cyclomatic.cyclomatic_sum() > 0.0);
    assert_eq!(cyc_only.metrics.loc.ploc(), 0.0);
}

// ----- Static `Send + Sync` contract -------------------------------------
//
// `big-code-analysis-book/src/library/parse-once.md` advertises `Ast` as
// `Send + Sync`. Both traits are auto-derived from the held
// `AstInner` (which currently wraps a per-language `Parser<T>` containing
// only `Vec<u8>` + `tree_sitter::Tree` + `PhantomData<T>`) plus
// `Option<String>`. A future field on `AstInner` or `Parser<T>` carrying
// `Rc<T>`, `RefCell<T>`, a raw `*mut`, or any non-`Sync` smart pointer
// would silently strip the auto-trait — and this assertion would then
// fail to compile, alerting the author before the docs go out of sync.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Ast>();
};

// ----- Tree-adoption path produces the same headline values --------------
//
// Previously compared against `metrics_from_tree`, but the production
// refactor made that function a thin delegate of `ast_from_tree_dispatch
// + AstInner::run_metrics` — i.e. exactly what `Ast::from_tree_sitter`
// dispatches through — so the comparison was tautological. The test now
// asserts the adoption path against the same hand-computed CCN as the
// `Ast::parse` Rust test above; if either path diverges from those
// values, the assertion fires.

// Differential check that the user-supplied tree is the one the metric
// walker reads — not a silent re-parse of `code` inside
// `Ast::from_tree_sitter`. The tree is built with
// `set_included_ranges` so it sees only the first two of the three
// top-level functions in `code`; a silent re-parse would see all three
// and `nom.functions_sum()` would jump from 2 to 3.
#[cfg(feature = "rust")]
#[test]
fn from_tree_sitter_walks_supplied_tree_not_a_reparse() {
    let code = b"fn a() {} fn b() {} fn c() {}".to_vec();
    let restricted_end = b"fn a() {} fn b() {}".len();

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(
            &LANG::Rust
                .get_tree_sitter_language()
                .expect("rust feature enabled"),
        )
        .expect("rust grammar compatible");
    parser
        .set_included_ranges(&[tree_sitter::Range {
            start_byte: 0,
            end_byte: restricted_end,
            start_point: tree_sitter::Point { row: 0, column: 0 },
            end_point: tree_sitter::Point {
                row: 0,
                column: restricted_end,
            },
        }])
        .expect("range covers a single contiguous slice");
    let tree = parser
        .parse(&code, None)
        .expect("parser has a language and a range set");

    let space = Ast::from_tree_sitter(LANG::Rust, tree, code, None)
        .expect("rust feature enabled")
        .metrics(MetricsOptions::default())
        .expect("walker succeeds");

    assert_eq!(space.metrics.nom.functions_sum(), 2.0);
}

#[cfg(feature = "rust")]
#[test]
fn from_tree_sitter_adopts_caller_built_tree() {
    let source = b"fn f(x: i32) -> i32 { if x > 0 { 1 } else { -1 } }".to_vec();
    let path = PathBuf::from("foo.rs");

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(
            &LANG::Rust
                .get_tree_sitter_language()
                .expect("rust feature enabled"),
        )
        .expect("rust grammar compatible");
    let tree = parser
        .parse(&source, None)
        .expect("parser has a language set");

    let space = Ast::from_tree_sitter(
        LANG::Rust,
        tree,
        source,
        Some(path.to_string_lossy().into_owned()),
    )
    .expect("rust feature enabled")
    .metrics(MetricsOptions::default())
    .expect("walker succeeds");

    // Same fixture as `parse_then_metrics_rust_matches_hand_computed_values`
    // — adoption must produce the same CCN as bytes-based parsing.
    assert_eq!(space.name.as_deref(), Some("foo.rs"));
    assert_eq!(space.kind, SpaceKind::Unit);
    assert_eq!(space.metrics.cyclomatic.cyclomatic_sum(), 3.0);
}

// ----- `as_tree_sitter` + `source` are consistent ------------------------

#[cfg(feature = "rust")]
#[test]
fn as_tree_sitter_walks_held_source() {
    let source_bytes = b"fn f() { 42 }".as_slice();
    let ast = Ast::parse(Source::new(LANG::Rust, source_bytes)).expect("rust feature enabled");

    let root = ast.as_tree_sitter().root_node();
    assert_eq!(root.kind(), "source_file");

    // The held bytes the tree references must round-trip through
    // every node's byte range — if `source()` ever returned a
    // different buffer than `as_tree_sitter()` was built against,
    // `utf8_text` would either panic or return mojibake.
    let text = root.utf8_text(ast.source()).expect("source is valid utf-8");
    assert_eq!(text, "fn f() { 42 }");
}

// ----- `LanguageDisabled` propagation ------------------------------------

// Mirror `langs::tests::disabled_language_dispatch_returns_language_disabled`:
// gated on the per-language feature being OFF, so it only fires under
// the minimal-langs CI matrix entry.
#[cfg(not(feature = "javascript"))]
#[test]
fn ast_parse_returns_language_disabled_for_off_feature() {
    let err = Ast::parse(Source::new(LANG::Javascript, b"")).unwrap_err();
    assert!(matches!(
        err,
        MetricsError::LanguageDisabled(LANG::Javascript)
    ));
}

#[cfg(all(feature = "rust", not(feature = "javascript")))]
#[test]
fn ast_from_tree_sitter_returns_language_disabled_for_off_feature() {
    // The dispatch arm rejects the disabled language *before* touching
    // the tree, so it is fine to hand it a tree built from an enabled
    // grammar (Rust here). This exercises the `Err(LanguageDisabled)`
    // arm of `ast_from_tree_dispatch` directly, instead of the proxy
    // assertion on `LANG::get_tree_sitter_language` that an earlier
    // version of this test used.
    let source = b"fn f() {}".to_vec();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(
            &LANG::Rust
                .get_tree_sitter_language()
                .expect("rust feature enabled"),
        )
        .expect("rust grammar compatible");
    let tree = parser
        .parse(&source, None)
        .expect("parser has a language set");

    let err = Ast::from_tree_sitter(LANG::Javascript, tree, source, None).unwrap_err();
    assert!(matches!(
        err,
        MetricsError::LanguageDisabled(LANG::Javascript)
    ));
}

// ----- C++ preprocessor: `Ast::source` returns expanded bytes ------------

#[cfg(feature = "cpp")]
#[test]
fn cpp_ast_source_reflects_preproc_expansion() {
    use std::collections::HashMap;
    use std::sync::Arc;

    use big_code_analysis::{PreprocFile, PreprocResults};

    // `DBG` is a macro; `c_macro::replace` blanks every macro
    // identifier to `$`-padded sentinels before the parser sees it.
    // The parsed (and held) source bytes are therefore not the
    // original input.
    let original = b"int f(int x) { return DBG ? x : 0; }".as_slice();
    let path = PathBuf::from("foo.c");

    let mut files = HashMap::new();
    files.insert(path.clone(), PreprocFile::new_macros(&["DBG"]));
    let pr = Arc::new(PreprocResults { files });

    let ast = Ast::parse(
        Source::new(LANG::Cpp, original)
            .with_preproc_path(Some(&path))
            .with_preproc(Some(pr)),
    )
    .expect("cpp feature enabled");

    let expanded = ast.source();
    // The exact replacement is `c_macro::replace`'s contract: macro
    // identifiers become same-length `$` runs. We avoid duplicating
    // the algorithm in the test by asserting the observable
    // properties:
    //
    // 1. The buffer length is preserved (replacement is in-place
    //    same-width substitution).
    // 2. `DBG` no longer appears.
    // 3. The same number of `$` bytes appear as the macro's length.
    assert_eq!(expanded.len(), original.len());
    assert!(!expanded.windows(3).any(|w| w == b"DBG"));
    #[allow(clippy::naive_bytecount)]
    let dollars = expanded.iter().filter(|&&b| b == b'$').count();
    assert_eq!(dollars, "DBG".len());
}

// ----- `language()` + `name()` accessors ---------------------------------

#[cfg(feature = "rust")]
#[test]
fn language_and_name_accessors_match_constructors() {
    let named =
        Ast::parse(Source::new(LANG::Rust, b"fn f() {}").with_name(Some("snippet.rs".to_owned())))
            .expect("rust feature enabled");
    assert_eq!(named.language(), LANG::Rust);
    assert_eq!(named.name(), Some("snippet.rs"));

    let nameless = Ast::parse(Source::new(LANG::Rust, b"fn f() {}")).expect("rust feature enabled");
    assert_eq!(nameless.name(), None);
}
