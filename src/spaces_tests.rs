use crate::MetricsOptions;
use crate::node::Ancestors;
use crate::spaces::metrics_inner;
use crate::test_support::check_func_space;
use crate::{CppParser, ParserTrait, SpaceKind};

/// `SpaceKind` is `#[non_exhaustive]` (#551); the attribute is a
/// compile-time forward-compat contract and must not change the
/// serialized form. Every variant still round-trips through its
/// lowercase token, `Display` agrees with serde, and
/// [`SpaceKind::from_serialized`] (the string-to-kind path the JSON
/// front-ends use for threshold scope, #969) agrees with serde
/// deserialization so the two cannot drift.
#[test]
fn space_kind_non_exhaustive_serde_roundtrip_unchanged() {
    for kind in [
        SpaceKind::Unknown,
        SpaceKind::Function,
        SpaceKind::Class,
        SpaceKind::Struct,
        SpaceKind::Trait,
        SpaceKind::Impl,
        SpaceKind::Unit,
        SpaceKind::Namespace,
        SpaceKind::Interface,
    ] {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, format!("\"{kind}\""));
        let back: SpaceKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
        // `from_serialized` consumes the bare token (no JSON quotes)
        // and must match serde's mapping for every variant.
        assert_eq!(SpaceKind::from_serialized(&kind.to_string()), kind);
    }
    // An unrecognized token degrades to `Unknown` rather than panicking.
    assert_eq!(SpaceKind::from_serialized("not_a_kind"), SpaceKind::Unknown);
}

/// Positive coverage for the C++ function-space predicates on the
/// only `function_definition` `kind_id` (343) that
/// `tree-sitter-mozcpp` currently emits. The structural
/// `FunctionDefinition*` contract for the aliased kind_ids
/// (489/491/494) that no observed input parses to is documented
/// at the predicate call sites in `src/checker.rs` and
/// `src/getter.rs` — see issue #285.
#[test]
fn cpp_function_definition_is_classified_as_function() {
    use crate::Cpp;
    use crate::checker::Checker;
    use crate::getter::Getter;
    use crate::langs::CppCode;

    let source = "int the_func(int x) { return x; }\n";
    let path = std::path::PathBuf::from("fd.cc");
    let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
    let root = parser.root();

    // Walk for any `FunctionDefinition*` variant (FD/FD2/FD3/FD4)
    // so the test stays valid if a future grammar bump starts
    // emitting one of the higher-numbered aliases.
    let fn_node = root
        .preorder()
        .find(|node| {
            let id = node.kind_id();
            Cpp::FunctionDefinition == id
                || Cpp::FunctionDefinition2 == id
                || Cpp::FunctionDefinition3 == id
                || Cpp::FunctionDefinition4 == id
        })
        .expect("parse must produce a function_definition node");

    assert!(
        CppCode::is_func(&fn_node, Ancestors::unknown()),
        "is_func must return true for a function_definition"
    );
    assert!(
        CppCode::is_func_space(&fn_node),
        "is_func_space must return true for a function_definition"
    );
    assert_eq!(
        CppCode::get_space_kind(&fn_node),
        SpaceKind::Function,
        "get_space_kind must classify function_definition as Function"
    );
    assert_eq!(
        CppCode::get_func_space_name(&fn_node, source.as_bytes(), Ancestors::unknown()),
        Some("the_func"),
        "get_func_space_name must extract the declarator identifier"
    );
}

#[test]
fn cpp_scope_resolution_operator() {
    check_func_space::<CppParser, _>(
        "void Foo::bar(){
                return;
            }",
        "foo.cpp",
        |func_space| {
            insta::assert_json_snapshot!(
                func_space.spaces[0].name,
                @r###""Foo::bar""###
            );
        },
    );
}

/// Regression for issue #80 — when tree-sitter-mozcpp returns a non-Unit
/// root (e.g. an `ERROR` root for code it cannot fully parse, as
/// happens for parts of DeepSpeech's KenLM and OpenFst sources), the
/// top-level `FuncSpace` must still be a `Unit` spanning the whole
/// file, with `blank >= 0` and `sloc >= ploc`.
#[test]
fn cpp_error_root_yields_unit_top_level_space() {
    // This snippet (a chunk of kenlm/lm/model.hh shape) is rejected by
    // tree-sitter-mozcpp as a clean translation_unit and surfaces as an
    // ERROR root node in the parse tree. Verified at the time of writing
    // against tree-sitter-mozcpp 0.20.4.
    let source = "#ifndef A\n\
                      namespace a { namespace b { namespace c {\n\
                      template <class S, class V> class C : publi\n";

    let path = std::path::PathBuf::from("error_root.cc");
    let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
    // Sanity: the grammar really does fall back to a non-Unit root for
    // this snippet — otherwise the synthetic-Unit code path is not
    // exercised by this test.
    assert!(
        parser.root().as_tree_sitter().is_error(),
        "test premise broken: grammar must yield ERROR root for this snippet"
    );

    let space = metrics_inner(
        &parser,
        path.to_str().map(str::to_owned),
        MetricsOptions::default(),
    )
    .unwrap();

    assert_eq!(
        space.kind,
        SpaceKind::Unit,
        "top-level FuncSpace must be Unit, not {:?}",
        space.kind
    );

    let loc = &space.metrics.loc;
    let sloc = loc.sloc();
    let ploc = loc.ploc();
    let blank = loc.blank();
    let line_count = source.lines().count();

    assert!(
        sloc >= ploc,
        "sloc ({sloc}) must be >= ploc ({ploc}) for the file-level space"
    );
    // `blank` is `u64`, so non-negativity is type-guaranteed; assert the
    // real invariant instead — blank lines cannot exceed source lines, so
    // a saturating-subtraction underflow (#437) cannot inflate the count.
    assert!(blank <= sloc, "blank ({blank}) must be <= sloc ({sloc})");
    assert_eq!(
        sloc as usize, line_count,
        "sloc ({sloc}) should match the file's line count ({line_count})"
    );
}

/// Lesson-9 contract (`docs/development/lessons_learned.md` §9,
/// issue #193): for every supported language, parsing any input —
/// including malformed or truncated — must yield a file-level
/// `FuncSpace` whose `kind == SpaceKind::Unit` with `sloc >= ploc`
/// and `blank >= 0`.
///
/// This helper pins the **contract** at the public API surface
/// (`metrics()` always returns a `Unit` top-level space). For most
/// grammars the parse root is already the canonical translation-
/// unit kind regardless of input, so the synthetic-Unit wrapper
/// (`src/spaces.rs:~385`) is not actually exercised by tests
/// using this helper alone. They serve as future-proofing: a
/// grammar bump that starts promoting an inner kind to root on
/// partial input would fail here before shipping a non-`Unit`
/// top-level space to downstream consumers.
///
/// Tests that need to exercise the synthetic-Unit wrapper itself
/// (i.e., the path triggered by an `ERROR`-root parse) must also
/// assert `parser.root().as_tree_sitter().is_error()` before calling this
/// helper. See `cpp_error_root_yields_unit_top_level_space` and
/// `lua_partial_input_yields_synthetic_unit_wrapper` — those two
/// are the only tests in the corpus that today exercise the
/// wrapper path. Issue #220 tracks finding additional per-grammar
/// fixtures that surface ERROR roots so each language can have
/// both a contract test and a wrapper-exercising test.
fn assert_top_level_space_is_unit_contract<P: ParserTrait>(source: &str, filename: &str) {
    let path = std::path::PathBuf::from(filename);
    let parser = P::new(source.as_bytes().to_vec(), &path, None);
    let space = metrics_inner(
        &parser,
        path.to_str().map(str::to_owned),
        MetricsOptions::default(),
    )
    .expect("metrics must yield a top-level space");
    assert_eq!(
        space.kind,
        SpaceKind::Unit,
        "top-level FuncSpace for {filename:?} must be Unit, not {:?}",
        space.kind
    );
    let loc = &space.metrics.loc;
    let sloc = loc.sloc();
    let ploc = loc.ploc();
    let blank = loc.blank();
    assert!(
        sloc >= ploc,
        "sloc ({sloc}) must be >= ploc ({ploc}) for the file-level space of {filename:?}",
    );
    // `blank` is `u64`; non-negativity is type-guaranteed. Assert the
    // real invariant — blank lines cannot exceed source lines (#437).
    assert!(
        blank <= sloc,
        "blank ({blank}) must be <= sloc ({sloc}) for the file-level space of {filename:?}",
    );
}

/// Like [`assert_top_level_space_is_unit_contract`] but additionally
/// asserts the parse root is an `ERROR` node, so the test actually
/// exercises the synthetic-Unit wrapper in `metrics()` rather than
/// the contract-only path. Use this for languages where a fixture
/// is known to make the grammar return ERROR (currently: Lua, C++
/// via mozcpp).
fn assert_partial_input_yields_synthetic_unit_wrapper<P: ParserTrait>(
    source: &str,
    filename: &str,
) {
    let path = std::path::PathBuf::from(filename);
    let parser = P::new(source.as_bytes().to_vec(), &path, None);
    assert!(
        parser.root().as_tree_sitter().is_error(),
        "test premise broken: grammar must yield ERROR root for {filename:?}",
    );
    assert_top_level_space_is_unit_contract::<P>(source, filename);
}

#[test]
fn python_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::PythonParser>(
        "def foo(x):\n    return x +\n",
        "partial.py",
    );
}

#[test]
fn javascript_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::JavascriptParser>(
        "function foo(x) {\n  return x +\n",
        "partial.js",
    );
}

#[test]
fn mozjs_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::MozjsParser>(
        "function foo(x) {\n  return x +\n",
        "partial.js",
    );
}

#[test]
fn typescript_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::TypescriptParser>(
        "function foo(x: number): number {\n  return x +\n",
        "partial.ts",
    );
}

#[test]
fn tsx_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::TsxParser>(
        "function Foo(x: number): JSX.Element {\n  return <div>{x +\n",
        "partial.tsx",
    );
}

#[test]
fn java_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::JavaParser>(
        "class Foo {\n  void bar(int x) {\n    return x +\n",
        "Partial.java",
    );
}

#[test]
fn kotlin_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::KotlinParser>(
        "class Foo {\n  fun bar(x: Int): Int {\n    return x +\n",
        "Partial.kt",
    );
}

#[test]
fn go_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::GoParser>(
        "package main\nfunc foo(x int) int {\n  return x +\n",
        "partial.go",
    );
}

#[test]
fn rust_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::RustParser>(
        "fn foo(x: i32) -> i32 {\n    return x +\n",
        "partial.rs",
    );
}

#[test]
fn csharp_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::CsharpParser>(
        "class Foo {\n  void Bar(int x) {\n    return x +\n",
        "Partial.cs",
    );
}

/// Regression for #429: a C# `EnumDeclaration` opens a FuncSpace
/// via `is_func_space`, so `get_space_kind` must classify it as
/// `Class` (matching Java/PHP/Groovy) rather than letting it fall
/// through to `SpaceKind::Unknown`. The enum is the only declared
/// space, so it appears as a direct child of the top-level Unit.
#[test]
fn csharp_enum_space_kind_is_class() {
    let src = "enum Color { Red, Green, Blue }\n";
    let path = std::path::PathBuf::from("Color.cs");
    let parser = crate::CsharpParser::new(src.as_bytes().to_vec(), &path, None);
    let space = metrics_inner(
        &parser,
        path.to_str().map(str::to_owned),
        MetricsOptions::default(),
    )
    .expect("metrics must yield a top-level space");

    let enum_space = space.spaces.first().expect("enum must open a child space");
    assert_eq!(enum_space.kind, SpaceKind::Class);
    assert_eq!(enum_space.name.as_deref(), Some("Color"));
}

#[test]
fn bash_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::BashParser>(
        "function foo() {\n  echo \"x +\n",
        "partial.sh",
    );
}

/// Lua's grammar surfaces an `ERROR` root for this fixture
/// (tree-sitter-lua 0.4.x), so this test exercises the
/// synthetic-Unit wrapper directly, on par with the C++
/// regression in `cpp_error_root_yields_unit_top_level_space`.
/// The 16 sibling `*_top_level_space_is_unit_contract` tests
/// only pin the public-API contract; only this and the C++ test
/// actually trigger the wrapper code path. See #220.
#[test]
fn lua_partial_input_yields_synthetic_unit_wrapper() {
    assert_partial_input_yields_synthetic_unit_wrapper::<crate::LuaParser>(
        "function foo(x)\n  return x +\n",
        "partial.lua",
    );
}

#[test]
fn tcl_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::TclParser>(
        "proc foo {x} {\n  return [expr {$x +\n",
        "partial.tcl",
    );
}

/// Lesson-9 contract for iRules: an unclosed `when` handler (truncated
/// mid-body) must still yield a `Unit` top-level space. Like Tcl, the
/// grammar keeps `source_file` as the root with an inner `ERROR`, so
/// this pins the contract rather than the synthetic-Unit wrapper path.
#[test]
fn irules_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::IrulesParser>(
        "when HTTP_REQUEST { if { $a } {\n",
        "partial.irule",
    );
}

#[test]
fn perl_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::PerlParser>(
        "sub foo {\n  my $x = shift;\n  return $x +\n",
        "partial.pl",
    );
}

#[test]
fn php_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::PhpParser>(
        "<?php\nfunction foo($x) {\n  return $x +\n",
        "partial.php",
    );
}

#[test]
fn elixir_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::ElixirParser>(
        "defmodule Foo do\n  def bar(x) do\n    x +\n",
        "partial.ex",
    );
}

// Regression for #275: the source-aware Getter must extract the
// human-readable head name from each macro-shaped declaration.
// The wave 2 implementation initially looked for an `Identifier` /
// `Alias` / `Call` as a *direct* child of the outer Call, but the
// tree-sitter-elixir grammar wraps the head in an `Arguments`
// node, so every promoted Class / Function space was labelled
// `<anonymous>` despite the source carrying a name.
#[test]
fn elixir_func_space_names_resolve_through_arguments_wrapper() {
    let src = "defmodule Foo.Bar do\n  def hello(x), do: x\n  defp helper, do: :ok\n  defmodule Inner do\n    def i, do: 1\n  end\nend\n";
    let path = std::path::PathBuf::from("foo.ex");
    let parser = crate::ElixirParser::new(src.as_bytes().to_vec(), &path, None);
    let space = metrics_inner(
        &parser,
        path.to_str().map(str::to_owned),
        MetricsOptions::default(),
    )
    .expect("metrics must yield a top-level space");

    // Top-level Unit -> file name.
    assert_eq!(space.name.as_deref(), Some("foo.ex"));

    // Outer defmodule Class is named `Foo.Bar`.
    let outer = space.spaces.first().expect("outer class space");
    assert_eq!(outer.kind, SpaceKind::Class);
    assert_eq!(outer.name.as_deref(), Some("Foo.Bar"));

    // Direct child names: `hello`, `helper`, `Inner`.
    let names: Vec<&str> = outer
        .spaces
        .iter()
        .map(|s| s.name.as_deref().unwrap_or("?"))
        .collect();
    assert_eq!(names, vec!["hello", "helper", "Inner"]);

    // Nested defmodule's child def resolves too.
    let inner = outer
        .spaces
        .iter()
        .find(|s| s.kind == SpaceKind::Class)
        .expect("nested class");
    let inner_names: Vec<&str> = inner
        .spaces
        .iter()
        .map(|s| s.name.as_deref().unwrap_or("?"))
        .collect();
    assert_eq!(inner_names, vec!["i"]);
}

/// `Preproc` and `Ccomment` are auxiliary grammars (preprocessor
/// directives and comments respectively). They expose the same
/// `ParserTrait` API, so the lesson-9 contract must hold for them
/// too — a grammar bump promoting an inner construct to root would
/// otherwise produce a non-`Unit` file-level space.
#[test]
fn preproc_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::PreprocParser>(
        "#ifdef FOO\n#define BAR(x) (x +\n",
        "partial.h",
    );
}

#[test]
fn ccomment_top_level_space_is_unit_contract() {
    assert_top_level_space_is_unit_contract::<crate::CcommentParser>(
        "/* unterminated comment\n  spanning several\n",
        "partial.c",
    );
}

/// Ruby uses tree-sitter-ruby which always returns a `program`
/// (Unit) root regardless of input — the synthetic-Unit fallback
/// path is unreachable today. The test pins the contract so a
/// future grammar bump that starts promoting an inner kind to
/// root would fail here.
#[test]
fn ruby_top_level_space_is_unit_contract() {
    // Truncated method definition (missing `end`) plus an
    // incomplete parameter list — tree-sitter-ruby treats both as
    // ERROR children of `program`.
    assert_top_level_space_is_unit_contract::<crate::RubyParser>(
        "class Foo\n  def bar(\n    x\n  ",
        "partial.rb",
    );
}

// The former `non_utf8_path_yields_lossy_top_level_name` test
// (issue #128) pinned the lossy-UTF-8 name derivation performed by
// the path-positional `metrics_with_options` shim, retired with the
// rest of the path-positional surface in #570. The `Ast` /
// `metrics_inner` seam carries an explicit `Option<String>` name
// end-to-end and never derives one from a `&Path`, so there is no
// production code path left to regress; callers own any lossy
// path-to-string conversion. The explicit-name contract is covered
// by the `analyze_in_memory_snippet_carries_caller_supplied_name`
// test below.

/// `analyze` with a caller-supplied `Source::name` skips the
/// lossy round-trip entirely — the top-level name is whatever
/// string the caller passed, byte-for-byte. This is the
/// post-#254 contract: callers analysing in-memory snippets no
/// longer need a `Path` to identify the resulting `FuncSpace`.
#[test]
fn analyze_in_memory_snippet_carries_caller_supplied_name() {
    use crate::{Source, analyze};

    let source =
        Source::new(crate::LANG::Cpp, b"int a = 42;").with_name(Some("in-memory.cpp".to_owned()));
    let space =
        analyze(source, MetricsOptions::default()).expect("analyze must yield a top-level space");
    assert_eq!(
        space.name.as_deref(),
        Some("in-memory.cpp"),
        "top-level name must be the caller-supplied string, byte-for-byte"
    );
}

/// `analyze` with `Source::name = None` leaves the top-level
/// `FuncSpace::name` as `None`. The pre-#254 entry points always
/// forced a `Some(...)`; the new API lets callers opt out.
#[test]
fn analyze_without_name_leaves_top_level_name_none() {
    use crate::{Source, analyze};

    let space = analyze(
        Source::new(crate::LANG::Cpp, b"int a = 42;"),
        MetricsOptions::default(),
    )
    .expect("analyze must yield a top-level space");
    assert!(
        space.name.is_none(),
        "top-level name must be None when Source::name is None, got {:?}",
        space.name
    );
}

// --- #306: file-scope suppression requires a Unit target ------
//
// `apply_suppression` historically picked `state_stack.first_mut()`
// for the `File` arm, relying on the convention that the root
// frame is always `SpaceKind::Unit`. The fix tightens that to an
// explicit `SpaceKind::Unit` predicate so an accidentally
// non-Unit root cannot silently swallow a file marker. These
// tests pin the new behaviour: they construct a `State` slice by
// hand (bypassing the parser) so the invariant violation is
// observable in isolation.

fn make_state<'a>(kind: SpaceKind) -> super::State<'a> {
    // Synthetic State constructor for `apply_suppression` tests.
    // Line spans are zeroed because these tests only inspect
    // `space.kind` and `space.suppressed`; do not reuse this helper
    // for tests that depend on `start_line` / `end_line` /
    // `metrics`.
    super::State {
        space: super::FuncSpace {
            name: None,
            start_line: 0,
            end_line: 0,
            kind,
            spaces: Vec::new(),
            metrics: super::CodeMetrics::default(),
            suppressed: super::SuppressionScope::default(),
        },
        halstead_maps: crate::metrics::halstead::HalsteadMaps::new(),
    }
}

fn file_suppression_all() -> crate::suppression::Suppression {
    crate::suppression::Suppression {
        kind: crate::suppression::SuppressionKind::File,
        scope: crate::suppression::SuppressionScope::All,
        source: crate::suppression::SuppressionSource::Native,
    }
}

#[test]
fn file_suppression_attaches_to_unit_frame() {
    let mut stack = vec![make_state(SpaceKind::Unit), make_state(SpaceKind::Function)];
    super::apply_suppression(&mut stack, &file_suppression_all());
    assert!(
        stack[0].space.suppressed.is_all(),
        "file marker (scope=All) must attach to the Unit root frame"
    );
    assert!(
        stack[1].space.suppressed.is_empty(),
        "file marker must not attach to a non-Unit frame"
    );
}

#[test]
fn file_suppression_skips_non_unit_root_frame() {
    // Synthetic stack where index 0 is *not* `Unit` — simulates
    // the broken-invariant case the explicit predicate guards
    // against. With the old `first_mut()` code this would
    // erroneously attach the file marker to a Function frame.
    let mut stack = vec![
        make_state(SpaceKind::Function),
        make_state(SpaceKind::Class),
    ];
    super::apply_suppression(&mut stack, &file_suppression_all());
    assert!(
        stack.iter().all(|s| s.space.suppressed.is_empty()),
        "file marker must be silently dropped when no Unit frame exists"
    );
}

#[test]
fn file_suppression_finds_unit_deeper_in_stack() {
    // The new predicate is "first frame whose kind is Unit",
    // not "first frame". If the root invariant is violated and
    // a Unit frame sits below a non-Unit frame, the marker must
    // still land on the Unit frame rather than being dropped.
    // Under the old `first_mut()` code, the Function root would
    // have absorbed the marker; this test pins the new search
    // semantics.
    let mut stack = vec![make_state(SpaceKind::Function), make_state(SpaceKind::Unit)];
    super::apply_suppression(&mut stack, &file_suppression_all());
    assert!(
        stack[0].space.suppressed.is_empty(),
        "non-Unit frame above the Unit must not absorb the file marker"
    );
    assert!(
        stack[1].space.suppressed.is_all(),
        "file marker must land on the Unit frame even when not at index 0"
    );
}

#[test]
fn file_suppression_empty_stack_is_silent_noop() {
    // No frames on the stack — `apply_suppression` must not
    // panic and must remain a silent no-op. Reaching the end of
    // this body proves no-panic; the stack cannot grow through
    // `&mut [State]`, so an explicit `is_empty()` check would
    // be a dead assertion.
    let mut stack: Vec<super::State<'_>> = Vec::new();
    super::apply_suppression(&mut stack, &file_suppression_all());
}

// --- #182: exclude_tests for Rust -----------------------------
//
// These exercise both flag values (`exclude_tests = false` is
// the documented backward-compatible default; `true` opts in to
// the new pruning). They are anchored on integer-valued
// accessors (`nom_functions_sum`, `cyclomatic_sum`,
// `cognitive_sum`, `n_operators`) rather than float magnitudes,
// because Halstead floats are bit-brittle (lessons_learned.md).

mod exclude_tests_rust {
    use crate::spaces::metrics_inner;
    use crate::{MetricsOptions, ParserTrait, RustParser};
    use std::path::PathBuf;

    fn analyse(source: &str, exclude_tests: bool) -> crate::FuncSpace {
        let path = PathBuf::from("lib.rs");
        let parser = RustParser::new(source.as_bytes().to_vec(), &path, None);
        metrics_inner(
            &parser,
            path.to_str().map(str::to_owned),
            MetricsOptions::default().with_exclude_tests(exclude_tests),
        )
        .expect("metrics must yield a top-level space")
    }

    // Production function plus an outer-attribute `#[test]`
    // function. With pruning on, the unit-level counts must
    // drop to the production function alone.
    #[test]
    fn outer_test_attribute_elides_function() {
        let source = "\
fn prod() -> i32 { 1 + 2 }

#[test]
fn t() { assert_eq!(1 + 1, 2); }
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);

        // Baseline: both functions counted (2 functions).
        assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
        // Pruned: only the production function (1 function).
        assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
        // Cyclomatic should also drop: prod has 1, test fn body
        // adds its own branches via assert_eq!. We use
        // non-strict inequality (`pruned <= baseline`) here so
        // grammar tweaks that flatten `assert_eq!` expansion to
        // zero cyclomatic branches don't make this test brittle;
        // the load-bearing pruning check is `functions_sum`
        // above.
        assert!(
            pruned.metrics.cyclomatic.cyclomatic_sum()
                <= baseline.metrics.cyclomatic.cyclomatic_sum()
        );
    }

    // `#[cfg(test)] mod tests { fn helper() {} #[test] fn t() {}
    // }` — every function inside the gated module disappears.
    #[test]
    fn cfg_test_mod_elides_entire_module() {
        let source = "\
fn prod() -> i32 { 1 }

#[cfg(test)]
mod tests {
    fn helper() -> i32 { 2 }
    fn another_helper() -> i32 { 3 }
    #[test] fn t() { assert_eq!(1, 1); }
}
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);

        // Baseline: prod + helper + another_helper + t = 4 functions.
        assert_eq!(baseline.metrics.nom.functions_sum() as usize, 4);
        // Pruned: only prod survives.
        assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
    }

    // `#[tokio::test]` is the most common async-runtime variant
    // and must be elided too. Baseline anchored at 2 so a grammar
    // regression that stops counting `async fn` cannot make this
    // test pass without pruning actually doing work.
    #[test]
    fn tokio_test_attribute_is_elided() {
        let source = "\
fn prod() -> i32 { 1 }

#[tokio::test]
async fn async_t() { let _x = 1; }
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);
        assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
        assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
    }

    // `#[cfg(all(test, target_arch = \"x86_64\"))]` — the
    // attribute parser must accept commas inside `all(...)`.
    // Baseline anchored at 2 to guard against silent grammar
    // regressions (see `tokio_test_attribute_is_elided`).
    #[test]
    fn cfg_all_test_with_extras_is_elided() {
        let source = "\
fn prod() -> i32 { 1 }

#[cfg(all(test, target_arch = \"x86_64\"))]
fn arch_specific_test() { let _x = 1; }
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);
        assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
        assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
    }

    // Plain prod-only file must be unchanged by either flag
    // value — i.e. the flag is genuinely a no-op when there's
    // no test code. Anchor the absolute count (2) so the
    // "they're equal" assertion can't be satisfied by both
    // values being 0.
    #[test]
    fn pure_production_unaffected_by_flag() {
        let source = "\
fn prod() -> i32 { 1 + 2 }
fn helper(x: i32) -> i32 { x * 2 }
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);
        assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
        assert_eq!(pruned.metrics.nom.functions_sum() as usize, 2);
        assert_eq!(
            baseline.metrics.cyclomatic.cyclomatic_sum(),
            pruned.metrics.cyclomatic.cyclomatic_sum(),
        );
    }

    // Backward compat: with the flag off (the default), every
    // node is still counted even when the source contains
    // test items.
    #[test]
    fn default_flag_off_preserves_baseline() {
        let source = "\
fn prod() -> i32 { 1 }

#[test]
fn t() { assert_eq!(1, 1); }
";
        let baseline_default = analyse(source, false);
        assert_eq!(baseline_default.metrics.nom.functions_sum() as usize, 2);
    }

    // Stacked attributes: tree-sitter exposes multiple
    // `#[...]` decorations as a chain of `AttributeItem`
    // siblings before the decorated item. The matcher must
    // walk all of them, not just the immediately-preceding
    // one, so a `#[cfg(target_arch = "x86_64")]` on top of
    // `#[cfg(test)]` still prunes.
    #[test]
    fn stacked_attributes_walk_all_siblings() {
        let source = "\
fn prod() -> i32 { 1 }

#[cfg(target_arch = \"x86_64\")]
#[cfg(test)]
fn t() { let _x = 1; }
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);
        assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
        assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
    }

    // Regression for #278. `test` was previously required to be
    // the first operand of `all(...)` / `any(...)`; forms like
    // `cfg(all(unix, test))` and `cfg(any(feature = "x", test))`
    // were silently kept. Baseline anchored at 3 (prod + two
    // gated fns) so a grammar regression cannot satisfy the test
    // without pruning doing real work.
    #[test]
    fn cfg_with_test_not_first_is_elided() {
        let source = "\
fn prod() -> i32 { 1 }

#[cfg(all(unix, test))]
fn unix_only_test() { let _x = 1; }

#[cfg(any(feature = \"slow\", test))]
fn slow_or_test() { let _x = 2; }
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);
        assert_eq!(baseline.metrics.nom.functions_sum() as usize, 3);
        assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
    }

    // Negative coverage: attribute shapes that look like "test"
    // but must NOT trigger pruning. Production code marked with
    // `#[cfg(not(test))]`, a feature flag named "test", or a
    // user macro whose path contains "test" must survive
    // pruning intact.
    #[test]
    fn lookalike_attributes_are_not_pruned() {
        let source = "\
#[cfg(not(test))]
fn only_outside_tests() -> i32 { 1 }

#[cfg(feature = \"test\")]
fn behind_test_feature() -> i32 { 2 }

#[my_crate::test_helper]
fn decorated_helper() -> i32 { 3 }

#[cfg(all(unix, not(test)))]
fn unix_prod_only() -> i32 { 4 }
";
        let pruned = analyse(source, true);
        // None of the four attributes mark test-only code.
        // All four functions must survive — particularly the
        // last one, which combines `not(test)` with another
        // operand (regression sibling to #278).
        assert_eq!(pruned.metrics.nom.functions_sum() as usize, 4);
    }

    // Inner attribute on a module: `mod tests { #![cfg(test)] ... }`
    // is the idiomatic form when you want to put the gate inside
    // the module body rather than on the declaration. Baseline
    // anchored at 3 (prod + helper + t) so a grammar regression
    // that drops the module body cannot satisfy this test with
    // pruning disabled.
    #[test]
    fn inner_cfg_test_attribute_elides_module() {
        let source = "\
fn prod() -> i32 { 1 }

mod tests {
    #![cfg(test)]
    fn helper() -> i32 { 2 }
    #[test] fn t() { assert_eq!(1, 1); }
}
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);
        assert_eq!(baseline.metrics.nom.functions_sum() as usize, 3);
        assert_eq!(pruned.metrics.nom.functions_sum() as usize, 1);
    }

    // #722: `sloc` is the lone loc sub-metric computed by span
    // subtraction rather than node accumulation, so before the fix
    // it stayed pinned at the full-file extent while `ploc`/`cloc`/
    // `lloc` correctly dropped — leaving an internally inconsistent
    // loc block (and a `blank` derived from the stale `sloc` that
    // over-counted). Under `exclude_tests`, unit `sloc` must drop in
    // step with the pruned test module.
    //
    // Layout (0-based rows): prod body rows 0..=3, blank row 4,
    // the `#[cfg(test)]` attribute (a sibling of `mod_item`, NOT
    // pruned) row 5, and the pruned `mod tests { … }` rows 6..=11.
    // Baseline `sloc` is the full 12-row span; pruned drops the six
    // module rows to 6, which equals the retained `ploc 5` (prod's
    // four lines + the surviving attribute line) plus the single
    // real `blank` line. Pre-fix, pruned `sloc` stayed 12 and
    // `blank` reported 7 — six phantom blanks from the elided module.
    #[test]
    fn sloc_drops_with_pruned_cfg_test_mod() {
        let source = "\
fn prod() {
    let x = 1;
    println!(\"{x}\");
}

#[cfg(test)]
mod tests {
    #[test]
    fn a() {
        assert_eq!(1, 1);
    }
}
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);

        assert_eq!(baseline.metrics.loc.sloc(), 12);
        assert_eq!(baseline.metrics.loc.ploc(), 11);

        // The headline fix: `sloc` falls in step with `ploc`.
        assert_eq!(pruned.metrics.loc.sloc(), 6);
        assert_eq!(pruned.metrics.loc.ploc(), 5);
        // Internal consistency restored: one real blank line, not the
        // pre-fix seven.
        assert_eq!(pruned.metrics.loc.blank(), 1);
    }

    // Adjacent pruned modules: two top-level `#[cfg(test)] mod`
    // blocks. Their spans are disjoint, so the excluded line counts
    // simply add (no interval merge). Rows (0-based): prod row 0,
    // attr row 1, `mod a` rows 2..=5, attr row 6, `mod b` rows
    // 7..=10 — a 11-row span. Pruning removes both four-row modules,
    // leaving rows 0/1/6 (prod + the two surviving sibling
    // attributes) → `sloc 3`, matching `ploc 3` with zero blanks.
    #[test]
    fn sloc_drops_for_adjacent_test_modules() {
        let source = "\
fn prod() {}
#[cfg(test)]
mod a {
    #[test]
    fn x() {}
}
#[cfg(test)]
mod b {
    #[test]
    fn y() {}
}
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);

        assert_eq!(baseline.metrics.loc.sloc(), 11);
        assert_eq!(pruned.metrics.loc.sloc(), 3);
        assert_eq!(pruned.metrics.loc.ploc(), 3);
        assert_eq!(pruned.metrics.loc.blank(), 0);
    }

    // Nested pruned modules: a non-test `mod inner` lives inside a
    // `#[cfg(test)] mod outer`. The walk `continue`s on `outer` and
    // never descends, so `inner`'s span is folded into `outer`'s and
    // never double-counted. Rows (0-based): prod row 0, attr row 1,
    // `mod outer` rows 2..=7 (an 8-row span). Pruning removes the
    // six outer rows, leaving rows 0/1 → `sloc 2`, matching `ploc 2`.
    #[test]
    fn sloc_drops_for_nested_test_modules() {
        let source = "\
fn prod() {}
#[cfg(test)]
mod outer {
    mod inner {
        #[test]
        fn t() {}
    }
}
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);

        assert_eq!(baseline.metrics.loc.sloc(), 8);
        assert_eq!(pruned.metrics.loc.sloc(), 2);
        assert_eq!(pruned.metrics.loc.ploc(), 2);
        assert_eq!(pruned.metrics.loc.blank(), 0);
    }

    // A pruned test function nested inside a *retained* `impl` block
    // must shrink BOTH that impl space's `sloc` AND every enclosing
    // space up to the unit root. The prune hook records the span on the
    // walker's current enclosing func-space (after `finalize`), and
    // `Sloc::merge` folds the pruned line count upward so the unit's
    // span-based `sloc` drops in step — mirroring how `Ploc` unions its
    // line-set upward (issue #741, a #722 follow-up). Rows (0-based):
    // `impl Foo {` row 0, `fn prod` row 1, the `#[test]` attribute (a
    // sibling, retained) row 2, the pruned single-line `fn t() {}` row
    // 3, `}` row 4 — a five-row impl span inside a six-row unit span.
    // Pruning removes the one test-fn row, so both the impl-level and
    // the unit-level `sloc` drop by exactly one.
    #[test]
    fn sloc_drops_for_test_fn_nested_in_impl() {
        let source = "\
impl Foo {
    fn prod(&self) {}
    #[test]
    fn t() {}
}
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);

        // The unit wraps exactly one `impl` space; the length guard
        // makes the single-child assumption explicit rather than
        // relying on `[0]` ordering.
        assert_eq!(baseline.spaces.len(), 1);
        assert_eq!(pruned.spaces.len(), 1);

        // Enclosing-space attribution: the impl space shrinks by the
        // one pruned test-fn row.
        let baseline_impl = &baseline.spaces[0];
        let pruned_impl = &pruned.spaces[0];
        assert_eq!(baseline_impl.metrics.loc.sloc(), 5);
        assert_eq!(pruned_impl.metrics.loc.sloc(), 4);

        // Unit-root propagation (the #741 fix): the pruned line count
        // folds upward through `Sloc::merge`, so the unit's `sloc` drops
        // by the same one row. Before the fix this stayed at the
        // baseline value because only the impl's `excluded_lines` grew.
        assert_eq!(baseline.metrics.loc.sloc(), 5);
        assert_eq!(pruned.metrics.loc.sloc(), 4);
    }

    // A `#[test] fn` directly inside a production `impl` (no separate
    // `#[cfg(test)] mod`): the unit's span-based `sloc` must still drop
    // by the pruned test-fn rows. Rows (0-based): `impl Calc {` row 0,
    // `fn add` rows 1..=3 (a retained production method), `#[test]` row
    // 4, `fn t` rows 5..=7 (pruned), `}` row 8 — a nine-row unit span.
    // Pruning removes the three test-fn rows (5..=7), so the unit `sloc`
    // drops from 9 to 6, matching `ploc`.
    #[test]
    fn sloc_drops_for_test_fn_in_production_impl() {
        let source = "\
impl Calc {
    fn add(&self, x: i32) -> i32 {
        x + 1
    }
    #[test]
    fn t() {
        assert_eq!(1 + 1, 2);
    }
}
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);

        assert_eq!(baseline.metrics.loc.sloc(), 9);
        assert_eq!(pruned.metrics.loc.sloc(), 6);
        assert_eq!(pruned.metrics.loc.ploc(), pruned.metrics.loc.sloc());
    }

    // A pruned test item nested inside a *retained* closure: the closure
    // body opens its own func-space (Rust `ClosureExpression`), so the
    // prune hook records the span on the closure, not the unit. The fix
    // must still propagate the count up to the unit. Rows (0-based):
    // `fn make() {` row 0, `let f = || {` row 1, `#[test]` row 2,
    // `fn t() {}` row 3 (pruned), `};` row 4, `}` row 5 — a six-row unit
    // span. Pruning removes the one test-fn row, so the unit `sloc`
    // drops from 6 to 5.
    #[test]
    fn sloc_drops_for_test_fn_nested_in_closure() {
        let source = "\
fn make() {
    let f = || {
        #[test]
        fn t() {}
    };
}
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);

        assert_eq!(baseline.metrics.loc.sloc(), 6);
        assert_eq!(pruned.metrics.loc.sloc(), 5);
    }

    // A non-test `impl` with no test items must be unaffected by the
    // upward-propagation fold: with nothing pruned, `excluded_lines`
    // stays zero at every level, so pruned and baseline `sloc` agree.
    #[test]
    fn non_test_impl_sloc_unaffected_by_pruning() {
        let source = "\
impl Calc {
    fn add(&self, x: i32) -> i32 {
        x + 1
    }
}
";
        let baseline = analyse(source, false);
        let pruned = analyse(source, true);

        assert_eq!(baseline.metrics.loc.sloc(), pruned.metrics.loc.sloc());
        assert_eq!(pruned.spaces.len(), 1);
        assert_eq!(
            baseline.spaces[0].metrics.loc.sloc(),
            pruned.spaces[0].metrics.loc.sloc()
        );
    }
}

// Non-Rust languages must ignore `exclude_tests = true` because
// they don't override `should_skip_subtree`. This is the
// "spot-check non-Rust" check from issue #182.
mod exclude_tests_non_rust {
    use crate::spaces::metrics_inner;
    use crate::{CppParser, MetricsOptions, ParserTrait};
    use std::path::PathBuf;

    #[test]
    fn cpp_ignores_exclude_tests_flag() {
        let source = "\
int prod() { return 1; }
int helper() { return 2; }
";
        let path = PathBuf::from("foo.cpp");
        let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
        let baseline = metrics_inner(
            &parser,
            path.to_str().map(str::to_owned),
            MetricsOptions::default().with_exclude_tests(false),
        )
        .expect("baseline must yield a top-level space");
        let parser = CppParser::new(source.as_bytes().to_vec(), &path, None);
        let pruned = metrics_inner(
            &parser,
            path.to_str().map(str::to_owned),
            MetricsOptions::default().with_exclude_tests(true),
        )
        .expect("pruned must yield a top-level space");
        // Anchor on the absolute count (2) so a regression that
        // dropped all C++ functions wouldn't satisfy a bare
        // `baseline == pruned` check.
        assert_eq!(baseline.metrics.nom.functions_sum() as usize, 2);
        assert_eq!(pruned.metrics.nom.functions_sum() as usize, 2);
    }
}

// --- #257: per-metric selection via with_only --------------------
//
// Exercise the gating bitfield through the recommended public
// entry point (`analyze` + `Source`) rather than the deprecated
// path-positional shims, so the tests pin the surface library
// consumers actually use.

mod with_only {
    use crate::{LANG, Metric, MetricSet, MetricsOptions, Source, analyze};

    const SOURCE: &str = "\
fn prod(x: i32) -> i32 {
    if x > 0 { x + 1 } else { x - 1 }
}
";

    fn analyse(metrics: &[Metric]) -> crate::FuncSpace {
        let opts = MetricsOptions::default().with_only(metrics);
        analyze(
            Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
            opts,
        )
        .expect("analyze must yield a top-level space")
    }

    // `with_only(&[Metric::Loc])` records exactly that bit on
    // `CodeMetrics.selected` and leaves the dependent metrics
    // (cognitive / cyclomatic / halstead / ...) at their default
    // values. The dependent-metric anchors guard against the
    // walker silently running them anyway.
    #[test]
    fn loc_only_skips_other_metrics() {
        let full = analyze(
            Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
            MetricsOptions::default(),
        )
        .expect("full analyze must yield a top-level space");
        let pruned = analyse(&[Metric::Loc]);

        assert_eq!(
            pruned.metrics.selected(),
            MetricSet::empty().with(Metric::Loc),
            "with_only(&[Loc]) must record exactly the Loc bit"
        );
        // LoC populated: the production function span is >= 1 ploc.
        assert!(pruned.metrics.loc.ploc() >= 1);
        // Full run has > 0 cognitive/cyclomatic; pruned must be
        // exactly zero because the compute call is gated off.
        assert!(full.metrics.cognitive.cognitive_sum() > 0);
        assert_eq!(pruned.metrics.cognitive.cognitive_sum(), 0);
        assert!(full.metrics.cyclomatic.cyclomatic_sum() > 0);
        assert_eq!(pruned.metrics.cyclomatic.cyclomatic_sum(), 0);
        // Halstead operators count is at the default (0) — no
        // per-node token text was hashed.
        assert_eq!(pruned.metrics.halstead.unique_operators(), 0);
    }

    // Selecting `Mi` alone must auto-add its dependencies
    // (Loc + Cyclomatic + Halstead) — otherwise the MI formula
    // would compute against zero inputs and return a meaningless
    // score.
    #[test]
    fn mi_auto_pulls_dependencies() {
        let pruned = analyse(&[Metric::Mi]);
        let sel = pruned.metrics.selected();
        assert!(sel.contains(Metric::Mi));
        assert!(sel.contains(Metric::Loc), "Mi depends on Loc");
        assert!(sel.contains(Metric::Cyclomatic), "Mi depends on Cyclomatic");
        assert!(sel.contains(Metric::Halstead), "Mi depends on Halstead");
        // Unrelated metrics must NOT be selected.
        assert!(!sel.contains(Metric::Abc));
        assert!(!sel.contains(Metric::Tokens));
        // The dependencies must actually be populated — not just
        // selected. Otherwise the MI formula receives zero inputs
        // and `mi_original`'s `inputs_are_empty` short-circuit
        // returns 0.0, which would also be `is_finite`. We anchor
        // on the dependency values themselves (Loc ploc > 0,
        // Cyclomatic sum > 0) so the test would fail if the
        // walker silently skipped the dependency compute.
        assert!(
            pruned.metrics.loc.ploc() > 0,
            "Loc must have run (Mi dependency); got ploc=0"
        );
        assert!(
            pruned.metrics.cyclomatic.cyclomatic_sum() > 0,
            "Cyclomatic must have run (Mi dependency); got sum=0"
        );
        // With non-zero inputs feeding the MI formula, the result
        // is a finite non-zero number (the MI for this snippet is
        // around 150 — a positive value well above the 0.0 that
        // `inputs_are_empty` would short-circuit to).
        let mi_value = pruned.metrics.mi.original();
        assert!(
            mi_value.is_finite() && mi_value != 0.0,
            "MI must be finite and non-default when its dependencies were computed; got {mi_value}"
        );
    }

    // `with_only(&[Metric::Wmc])` auto-adds Cyclomatic + Nom.
    #[test]
    fn wmc_auto_pulls_dependencies() {
        let pruned = analyse(&[Metric::Wmc]);
        let sel = pruned.metrics.selected();
        assert!(sel.contains(Metric::Wmc));
        assert!(
            sel.contains(Metric::Cyclomatic),
            "Wmc depends on Cyclomatic"
        );
        assert!(sel.contains(Metric::Nom), "Wmc depends on Nom");
        assert!(!sel.contains(Metric::Halstead));
        // Dependency must actually be computed, not just bit-set:
        // selecting Wmc alone must populate Cyclomatic & Nom.
        assert!(
            pruned.metrics.cyclomatic.cyclomatic_sum() > 0,
            "Cyclomatic must have run (Wmc dependency); got sum=0"
        );
        assert!(
            pruned.metrics.nom.functions_sum() > 0,
            "Nom must have run (Wmc dependency); got functions_sum=0"
        );
    }

    // #428: selecting Cognitive/Exit/NArgs alone must auto-pull
    // Nom so their per-function averages divide by the real
    // function count instead of the `Stats` default (0), which
    // would otherwise yield inf/NaN. `SOURCE` is a single
    // function with one `if` branch and one argument, so the
    // function count is exactly 1 and each average equals its
    // own sum.
    #[test]
    fn cognitive_only_pulls_nom_and_average_is_finite() {
        let pruned = analyse(&[Metric::Cognitive]);
        let sel = pruned.metrics.selected();
        assert!(sel.contains(Metric::Cognitive));
        assert!(sel.contains(Metric::Nom), "Cognitive depends on Nom (#428)");
        // Nom must actually run, supplying the divisor.
        assert!(
            pruned.metrics.nom.total() > 0,
            "Nom must have run (Cognitive dependency); got total=0"
        );
        let avg = pruned.metrics.cognitive.cognitive_average();
        assert!(
            avg.is_finite(),
            "cognitive_average must be finite when Nom is pulled in; got {avg}"
        );
        // The `if`/`else` over one function => cognitive sum == 2
        // => average == 2 (one increment for the `if`, one for the
        // `else` branch).
        assert_eq!(pruned.metrics.cognitive.cognitive_sum(), 2);
        assert_eq!(avg, 2.0);
    }

    #[test]
    fn exit_only_pulls_nom_and_average_is_finite() {
        let pruned = analyse(&[Metric::Nexits]);
        let sel = pruned.metrics.selected();
        assert!(sel.contains(Metric::Nexits));
        assert!(sel.contains(Metric::Nom), "Exit depends on Nom (#428)");
        assert!(
            pruned.metrics.nom.total() > 0,
            "Nom must have run (Exit dependency); got total=0"
        );
        let avg = pruned.metrics.nexits.nexits_average();
        assert!(
            avg.is_finite(),
            "nexits_average must be finite when Nom is pulled in; got {avg}"
        );
        // `prod` has no explicit `return`, so nexits_sum == 0 and the
        // guarded divisor (1) keeps the average a finite 0.0.
        assert_eq!(pruned.metrics.nexits.nexits_sum(), 0);
        assert_eq!(avg, 0.0);
    }

    #[test]
    fn nargs_only_pulls_nom_and_average_is_finite() {
        let pruned = analyse(&[Metric::Nargs]);
        let sel = pruned.metrics.selected();
        assert!(sel.contains(Metric::Nargs));
        assert!(sel.contains(Metric::Nom), "NArgs depends on Nom (#428)");
        assert!(
            pruned.metrics.nom.total() > 0,
            "Nom must have run (NArgs dependency); got total=0"
        );
        let avg = pruned.metrics.nargs.average();
        assert!(
            avg.is_finite(),
            "average must be finite when Nom is pulled in; got {avg}"
        );
        // One argument over one function => average == 1.
        assert_eq!(avg, 1.0);
    }

    // `MetricsOptions::default()` selects every metric (#257's
    // default-preservation contract).
    #[test]
    fn default_options_select_every_metric() {
        let full = analyze(
            Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
            MetricsOptions::default(),
        )
        .expect("analyze must yield a top-level space");
        assert_eq!(full.metrics.selected(), MetricSet::all());
    }

    // JSON serialization elides unselected metrics. Anchored on
    // the field names emitted at the top level of the
    // `metrics` object rather than the full payload so a future
    // additive change (new metric, new sub-field) doesn't shift
    // unrelated tests.
    #[test]
    fn unselected_metrics_are_skipped_in_json() {
        let pruned = analyse(&[Metric::Loc]);
        let json =
            serde_json::to_value(&pruned.metrics).expect("CodeMetrics must serialize cleanly");
        let metrics = json.as_object().expect("CodeMetrics serializes as object");

        assert!(
            metrics.contains_key("loc"),
            "loc must be serialized when selected"
        );
        for skipped in [
            "cognitive",
            "cyclomatic",
            "halstead",
            "nom",
            "tokens",
            "nargs",
            "nexits",
            "abc",
            "mi",
            "wmc",
            "npm",
            "npa",
        ] {
            assert!(
                !metrics.contains_key(skipped),
                "{skipped} must be elided when not selected"
            );
        }
    }

    // #522: `kind` (via `get_space_kind_with_code`) is computed
    // lazily — skipped when a node is neither a func space nor a
    // Loc consumer. The lazy path must be byte-equivalent for
    // every consumer:
    //   - promoted (func_space) nodes still compute `kind`, so the
    //     space tree's `SpaceKind`s are unchanged;
    //   - non-Loc metrics never read `unit`, so their values are
    //     unchanged when Loc is deselected.
    // Elixir is the canonical regression target: its
    // `get_space_kind_with_code` runs a per-`Call` source-text
    // keyword scan, and `defmodule` / `def` promote to Class /
    // Function spaces whose kind would be lost if the lazy gate
    // skipped a node it shouldn't.
    #[test]
    fn elixir_loc_deselected_preserves_kinds_and_metrics() {
        use crate::SpaceKind;

        const ELIXIR_SOURCE: &str = "\
defmodule Greeter do
  def hello(name) do
    if name == \"\" do
      :anon
    else
      name
    end
  end

  def bye() do
    :ok
  end
end
";

        fn collect_kinds(space: &crate::FuncSpace, out: &mut Vec<SpaceKind>) {
            out.push(space.kind);
            for sub in &space.spaces {
                collect_kinds(sub, out);
            }
        }

        fn analyse_elixir(metrics: Option<&[Metric]>) -> crate::FuncSpace {
            let opts = match metrics {
                Some(m) => MetricsOptions::default().with_only(m),
                None => MetricsOptions::default(),
            };
            analyze(
                Source::new(LANG::Elixir, ELIXIR_SOURCE.as_bytes())
                    .with_name(Some("greeter.ex".to_owned())),
                opts,
            )
            .expect("analyze must yield a top-level space")
        }

        let full = analyse_elixir(None);
        // Cognitive is a non-Loc metric, exercised without Loc so
        // the lazy gate takes the skip branch on non-promoted
        // nodes. Cognitive auto-pulls Nom; neither selects Loc.
        let pruned = analyse_elixir(Some(&[Metric::Cognitive]));

        assert!(
            !pruned.metrics.selected().contains(Metric::Loc),
            "test premise: Loc must be deselected on the pruned run"
        );

        // The promoted-space kinds must be identical: defmodule =>
        // Class, the two def macros => Function, the module file
        // root => Unit. If the lazy gate wrongly skipped a promoted
        // node, FuncSpace::new would have seen SpaceKind::Unknown.
        let mut full_kinds = Vec::new();
        let mut pruned_kinds = Vec::new();
        collect_kinds(&full, &mut full_kinds);
        collect_kinds(&pruned, &mut pruned_kinds);
        assert_eq!(
            full_kinds, pruned_kinds,
            "lazy `kind` computation must not change the space-tree SpaceKinds"
        );
        assert!(
            full_kinds.contains(&SpaceKind::Class),
            "test premise: defmodule must promote to a Class space"
        );
        assert!(
            full_kinds.contains(&SpaceKind::Function),
            "test premise: def must promote to a Function space"
        );

        // The non-Loc metric value must be byte-identical between
        // the full and Loc-deselected runs (the `unit` flag only
        // feeds Loc, so deselecting Loc cannot move it).
        assert!(
            full.metrics.cognitive.cognitive_sum() > 0,
            "test premise: the source has cognitive complexity (the `if`/`else`)"
        );
        assert_eq!(
            full.metrics.cognitive.cognitive_sum(),
            pruned.metrics.cognitive.cognitive_sum(),
            "deselecting Loc must not change cognitive complexity"
        );
    }

    // Empty slice = nothing selected. Every metric must be
    // elided from JSON output; the space tree is still
    // produced.
    #[test]
    fn empty_slice_selects_nothing() {
        let pruned = analyse(&[]);
        assert_eq!(pruned.metrics.selected(), MetricSet::empty());
        let json =
            serde_json::to_value(&pruned.metrics).expect("CodeMetrics must serialize cleanly");
        let metrics = json.as_object().expect("CodeMetrics serializes as object");
        assert!(
            metrics.is_empty(),
            "with_only(&[]) must elide every metric, got keys {:?}",
            metrics.keys().collect::<Vec<_>>()
        );
    }

    // #743: `with_metric_set` must close the caller-supplied set
    // under its dependencies, exactly like `with_only`. A verbatim
    // `empty().with(Mi)` would otherwise compute the MI formula
    // against zero-valued Loc / Cyclomatic / Halstead inputs and
    // emit a meaningless score with no error.
    #[test]
    fn with_metric_set_resolves_dependency_closure() {
        let unresolved = MetricSet::empty().with(Metric::Mi);
        assert!(
            !unresolved.contains(Metric::Loc),
            "test premise: the verbatim set must omit Mi's deps"
        );

        let opts = MetricsOptions::default().with_metric_set(unresolved);
        let space = analyze(
            Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
            opts,
        )
        .expect("analyze must yield a top-level space");

        let sel = space.metrics.selected();
        assert!(sel.contains(Metric::Mi));
        assert!(sel.contains(Metric::Loc), "Mi depends on Loc");
        assert!(sel.contains(Metric::Cyclomatic), "Mi depends on Cyclomatic");
        assert!(sel.contains(Metric::Halstead), "Mi depends on Halstead");

        // The dependencies must actually be computed, not just
        // bit-set: Loc ploc > 0 and Cyclomatic sum > 0 prove the
        // walker ran them.
        assert!(
            space.metrics.loc.ploc() > 0,
            "Loc must have run (Mi dependency); got ploc=0"
        );
        assert!(
            space.metrics.cyclomatic.cyclomatic_sum() > 0,
            "Cyclomatic must have run (Mi dependency); got sum=0"
        );

        // The resolved `with_metric_set` form must match the
        // `with_only(&[Mi])` result bit-for-bit, both in the
        // selected set and the computed MI value.
        let via_only = analyze(
            Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
            MetricsOptions::default().with_only(&[Metric::Mi]),
        )
        .expect("analyze must yield a top-level space");
        assert_eq!(sel, via_only.metrics.selected());
        let mi_value = space.metrics.mi.original();
        assert!(
            mi_value.is_finite() && mi_value != 0.0,
            "MI must be finite and non-default once its deps are computed; got {mi_value}"
        );
        assert_eq!(mi_value, via_only.metrics.mi.original());
    }

    // An already-resolved set passes through `with_metric_set`
    // unchanged (idempotence at the builder level).
    #[test]
    fn with_metric_set_passes_resolved_set_unchanged() {
        let resolved = MetricSet::from_slice_with_deps(&[Metric::Mi]);
        let opts = MetricsOptions::default().with_metric_set(resolved);
        let space = analyze(
            Source::new(LANG::Rust, SOURCE.as_bytes()).with_name(Some("lib.rs".to_owned())),
            opts,
        )
        .expect("analyze must yield a top-level space");
        assert_eq!(space.metrics.selected(), resolved);
    }
}

// --- #1127: a restricted selection must not move any value -------
//
// The per-metric unit modules assert one family through
// `test_support::check_metrics_only`, which computes that family plus
// the dependencies `Metric::dependencies` declares — not all thirteen.
// Thousands of assertions therefore rest on one property the `with_only`
// tests above never state: for every selected metric, a restricted walk
// produces *exactly* the value the full walk does. Prove it here rather
// than inferring it from a green suite, which cannot distinguish "the
// value is unchanged" from "the value is wrong in both runs".
//
// It doubles as the guard on the gating itself. Measured, not assumed:
// narrowing one compute gate — `if selected.contains(Metric::Npa)` to
// `… && selected.contains(Metric::Npm)`, the shape a mistakenly-coupled
// metric would take — failed **only** this test out of the 3,245 in the
// lib target. Dropping a declared `Metric::dependencies` edge is
// already covered elsewhere (`with_dependencies_pulls_in_*`,
// `cognitive_only_pulls_nom_and_average_is_finite`); an *undeclared*
// coupling between the walker's per-metric gates was not.
mod metric_selection_parity {
    use crate::{CodeMetrics, FuncSpace, LANG, Metric, MetricsOptions, Source, SpaceKind, analyze};
    // Only the `rust`-gated parity test resolves a selection, so an
    // ungated import is dead under `--no-default-features`.
    #[cfg(feature = "rust")]
    use crate::MetricSet;
    use serde_json::Value;

    // Deliberately non-default for all thirteen metrics at once: public
    // struct fields (npa), public inherent methods (npm, wmc, nom), a
    // multi-clause `if` (cognitive, cyclomatic, abc, halstead, tokens),
    // an early `return` (nexits), parameters (nargs), and a comment
    // (loc). It is the fixture the non-vacuity assertion below leans on.
    #[cfg(feature = "rust")]
    const RUST: &str = "\
pub struct Counter {
    pub total: u32,
    step: u32,
}

impl Counter {
    // Applies one step.
    pub fn bump(&mut self, by: u32) -> u32 {
        if by > 0 && self.step > 0 {
            self.total += by * self.step;
            return self.total;
        }
        self.total
    }

    fn reset(&mut self) {
        self.total = 0;
    }
}

fn choose(a: u32, b: u32) -> u32 {
    let double = |x: u32| x * 2;
    if a > b { double(a) } else { double(b) }
}
";

    #[cfg(feature = "java")]
    const JAVA: &str = "\
public class Shape {
    public int width;
    private int height;

    public int area(int scale) {
        if (scale > 0 && width > 0) {
            return width * height * scale;
        }
        return 0;
    }
}
";

    #[cfg(feature = "python")]
    const PYTHON: &str = "\
class Bag:
    def __init__(self, size):
        self.size = size

    def take(self, n):
        if n > self.size:
            return 0
        while n > 0:
            n -= 1
        return n
";

    // The Java and Python entries widen the grammar coverage of the
    // parity claim; only the Rust one is load-bearing for non-vacuity.
    fn fixtures() -> Vec<(LANG, &'static str, &'static str)> {
        vec![
            #[cfg(feature = "rust")]
            (LANG::Rust, "counter.rs", RUST),
            #[cfg(feature = "java")]
            (LANG::Java, "Shape.java", JAVA),
            #[cfg(feature = "python")]
            (LANG::Python, "bag.py", PYTHON),
        ]
    }

    fn analyse(lang: LANG, filename: &str, source: &str, options: MetricsOptions) -> FuncSpace {
        analyze(
            Source::new(lang, source.as_bytes()).with_name(Some(filename.to_owned())),
            options,
        )
        .expect("analyze must yield a top-level space")
    }

    fn analyse_only(lang: LANG, filename: &str, source: &str, metrics: &[Metric]) -> FuncSpace {
        analyse(
            lang,
            filename,
            source,
            MetricsOptions::default().with_only(metrics),
        )
    }

    // Serialization is the comparison vehicle because the per-metric
    // `Stats` types implement `Serialize` but not `PartialEq`; it also
    // compares every public field of each family rather than the
    // handful an accessor exposes.
    fn metric_json(metrics: &CodeMetrics, metric: Metric) -> Value {
        // `Metric` is `#[non_exhaustive]`, which is inert in-crate, so
        // this match is exhaustive without a wildcard on purpose: a new
        // variant must be given an arm here or the parity claim would
        // silently stop covering it.
        let value = match metric {
            Metric::Cognitive => serde_json::to_value(&metrics.cognitive),
            Metric::Cyclomatic => serde_json::to_value(&metrics.cyclomatic),
            Metric::Halstead => serde_json::to_value(&metrics.halstead),
            Metric::Loc => serde_json::to_value(&metrics.loc),
            Metric::Nom => serde_json::to_value(&metrics.nom),
            Metric::Tokens => serde_json::to_value(&metrics.tokens),
            Metric::Nargs => serde_json::to_value(&metrics.nargs),
            Metric::Nexits => serde_json::to_value(&metrics.nexits),
            Metric::Abc => serde_json::to_value(&metrics.abc),
            Metric::Npm => serde_json::to_value(&metrics.npm),
            Metric::Npa => serde_json::to_value(&metrics.npa),
            Metric::Mi => serde_json::to_value(&metrics.mi),
            Metric::Wmc => serde_json::to_value(&metrics.wmc),
        };
        value.expect("every metric Stats serializes cleanly")
    }

    // Keyed by (name, kind) so a shape divergence surfaces as a
    // mismatched row rather than a silently misaligned comparison.
    fn collect(space: &FuncSpace, metric: Metric) -> Vec<(Option<String>, SpaceKind, Value)> {
        let mut out = Vec::new();
        let mut stack = vec![space];
        while let Some(current) = stack.pop() {
            out.push((
                current.name.clone(),
                current.kind,
                metric_json(&current.metrics, metric),
            ));
            stack.extend(current.spaces.iter());
        }
        out
    }

    #[test]
    // Gated on the language whose fixture makes the non-vacuity
    // assertion satisfiable for all thirteen metrics.
    #[cfg(feature = "rust")]
    fn restricted_selection_reproduces_the_full_run() {
        let fixtures = fixtures();
        crate::test_support::assert_fixtures_present(&fixtures);

        // A metric is "exercised" once some fixture gives it a value
        // distinguishable from the uncomputed one. `with_only(&[])`
        // computes nothing, so its output *is* the `Stats` default —
        // which makes it the reference for "this parity assertion is
        // comparing something". Without it, a metric that happened to
        // stay at its default in every fixture would compare equal for
        // the one reason that proves nothing.
        let mut exercised: Vec<Metric> = Vec::new();

        for (lang, filename, source) in fixtures {
            let full = analyse(lang, filename, source, MetricsOptions::default());
            let uncomputed = analyse_only(lang, filename, source, &[]);

            for &selected in Metric::ALL {
                let pruned = analyse_only(lang, filename, source, &[selected]);
                // Check every metric the selection *resolves* to, not
                // just the one asked for. Roughly thirty migrated
                // assertions read a dependency-pulled family rather than
                // their module's own — `nargs.rs` asserts
                // `metric.nom.functions_sum()` under `{Nargs, Nom}`, and
                // `mi.rs` runs under `{Mi, Loc, Cyclomatic, Halstead}`.
                // Comparing `Nom` only under `with_only(&[Nom])` would
                // leave exactly those out.
                let resolved = MetricSet::from_slice_with_deps(&[selected]);
                for &metric in Metric::ALL.iter().filter(|&&m| resolved.contains(m)) {
                    let full_rows = collect(&full, metric);
                    assert_eq!(
                        full_rows,
                        collect(&pruned, metric),
                        "{lang:?}: with_only(&[{selected}]) must reproduce the full run's \
                         {metric} values in every space"
                    );
                    if full_rows != collect(&uncomputed, metric) {
                        exercised.push(metric);
                    }
                }
            }
        }

        for &metric in Metric::ALL {
            assert!(
                exercised.contains(&metric),
                "no fixture gives {metric} a non-default value, so its parity \
                 assertion compares two uncomputed defaults"
            );
        }
    }
}

// Gated on `rust`: the happy-path tests parse a `.rs` fixture, so they
// bind a live `Ast` — which is uninhabited when no grammar feature is
// compiled in (`--no-default-features`), making the binding's tail
// unreachable and tripping the `-D warnings` gate. The error-path tests
// here are feature-independent but live in the same cohesive module; the
// canonical all-features test run (and the minimal-langs leg, which
// enables `rust`) still exercises every one.
#[cfg(feature = "rust")]
mod from_path_tests {
    use crate::{Ast, FromPathError, LANG, MetricsOptions, SpaceKind};
    use std::io::Write;

    fn write_file(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).expect("create temp file");
        f.write_all(bytes).expect("write temp file");
        path
    }

    #[test]
    fn from_path_parses_and_detects_language() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "foo.rs", b"fn main() {\n    let x = 1;\n}\n");
        let ast = Ast::from_path(&path).expect("from_path parses a .rs file");
        assert_eq!(ast.language(), LANG::Rust);
        assert_eq!(ast.source(), b"fn main() {\n    let x = 1;\n}\n");
        let space = ast
            .metrics(MetricsOptions::default())
            .expect("metrics from parsed file");
        // The walker always pushes a synthetic top-level Unit space.
        assert_eq!(space.kind, SpaceKind::Unit);
        assert_eq!(space.name.as_deref(), path.to_str());
    }

    #[test]
    fn from_path_normalizes_crlf_for_analyze_parity() {
        // `from_path` reads through the same text reader `analyze` uses,
        // so CRLF newlines are normalized to LF before parsing — the
        // tree (and therefore metrics) cannot disagree with `analyze`
        // over the same file (#727).
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(
            dir.path(),
            "crlf.rs",
            b"fn a() {\r\n    let x = 1;\r\n}\r\n",
        );
        let ast = Ast::from_path(&path).expect("from_path parses CRLF source");
        assert!(
            !ast.source().contains(&b'\r'),
            "carriage returns must be normalized away; got {:?}",
            ast.source()
        );
    }

    #[test]
    fn from_path_unknown_extension_is_unknown_language() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_file(dir.path(), "mystery.zzz", b"some unrecognized content\n");
        assert!(matches!(
            Ast::from_path(&path),
            Err(FromPathError::UnknownLanguage)
        ));
    }

    #[test]
    fn from_path_missing_file_is_io_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.rs");
        assert!(matches!(Ast::from_path(&path), Err(FromPathError::Io(_))));
    }

    #[test]
    fn from_path_tiny_or_binary_file_is_unreadable() {
        // Files of three bytes or fewer, and binary/non-UTF-8 files, are
        // the inputs `analyze` skips; `from_path` reports them as
        // `Unreadable` rather than handing back an empty tree.
        let dir = tempfile::tempdir().expect("tempdir");
        let tiny = write_file(dir.path(), "tiny.rs", b"ab");
        assert!(matches!(
            Ast::from_path(&tiny),
            Err(FromPathError::Unreadable)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn from_path_non_utf8_path_is_rejected() {
        use std::os::unix::ffi::OsStrExt;
        // The path doubles as the FuncSpace name (an identifier); a
        // non-UTF-8 path is rejected before any read.
        let bad = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/tmp/\xff\xfe.rs"));
        assert!(matches!(
            Ast::from_path(bad),
            Err(FromPathError::NonUtf8Path)
        ));
    }
}

/// `CodeMetrics`'s `Display` (extracted to `spaces/code_metrics.rs` in
/// the per-type split) was exercised by no test. It must concatenate
/// the nine reported sub-metrics in a fixed order — nargs, nexits,
/// cognitive, cyclomatic, halstead, loc, nom, tokens, mi — each
/// `writeln!`-separated, with `mi` last and no trailing newline. Pin
/// that contract so a future reorder or stray newline is caught.
#[test]
fn code_metrics_display_concatenates_reported_submetrics_in_order() {
    use crate::{Source, analyze};

    let space = analyze(
        Source::new(crate::LANG::Cpp, b"int add(int a, int b) { return a + b; }"),
        MetricsOptions::default(),
    )
    .expect("analyze must yield a top-level space");
    let m = &space.metrics;

    let shown = format!("{m}");
    let expected = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        m.nargs, m.nexits, m.cognitive, m.cyclomatic, m.halstead, m.loc, m.nom, m.tokens, m.mi
    );
    assert_eq!(
        shown, expected,
        "CodeMetrics Display must be the nine sub-metric Displays in order, newline-joined"
    );
    assert!(
        !shown.ends_with('\n'),
        "mi is the final block, so Display must not end with a trailing newline: {shown:?}"
    );
}

/// `Ast`'s `Debug` (extracted to `spaces/ast.rs`) was untested. It must
/// honor the public `Ast: Debug` promise by reporting `language` and
/// `name` via `finish_non_exhaustive` (the held tree-sitter tree and
/// raw source have no meaningful `Debug` projection), without panicking
/// or leaking the opaque internals.
// `Ast` is uninhabited when no language grammar is compiled in, so this
// test (which builds an `Ast` via `Ast::parse`) is gated on a concrete
// language feature — `cpp`, the language it parses — mirroring the
// `#[cfg(feature = "rust")]` gate on the `from_path_tests` module below.
#[cfg(feature = "cpp")]
#[test]
fn ast_debug_reports_language_and_name_non_exhaustively() {
    use crate::{Ast, Source};

    let ast = Ast::parse(
        Source::new(crate::LANG::Cpp, b"int a = 42;").with_name(Some("dbg.cpp".to_owned())),
    )
    .expect("parse of a trivial C++ snippet must succeed");
    let shown = format!("{ast:?}");

    // `debug_struct` renders `Ast { language: <lang>, name: <name>, .. }`.
    // Assert the field *values* — not just the labels — so a wrong
    // `language()` or a dropped name is caught, and that
    // `finish_non_exhaustive` elides the opaque tree/source (the trailing
    // `..`) rather than printing them.
    assert!(
        shown.starts_with("Ast {"),
        "Debug must render the struct by name: {shown:?}"
    );
    assert!(
        shown.contains("language: Cpp"),
        "Debug must report the resolved language value, not merely the label: {shown:?}"
    );
    assert!(
        shown.contains("name: Some(\"dbg.cpp\")"),
        "Debug must report the caller-supplied name value: {shown:?}"
    );
    assert!(
        shown.trim_end().ends_with(".. }"),
        "finish_non_exhaustive must elide the tree/source as a trailing `..`: {shown:?}"
    );
}

/// ObjC FuncSpace naming + kind. The per-type getter split isolated
/// `ObjcCode::get_func_space_name` / `get_space_kind`, and the existing
/// ObjC `check_func_space` test asserts only a metric sum — which
/// passes even if a container's name resolves to `None`. Pin the
/// names *and* kinds for an `@interface`, an `@implementation` + its
/// method, and a free function, so an ObjC naming regression cannot
/// hide behind a vacuous metric assertion (#724; lessons 2 & 31).
#[test]
fn objc_func_space_tree_carries_names_and_kinds() {
    use crate::ObjcParser;

    let src = "\
@interface Greeter : NSObject
- (int)compute:(int)x;
@end

@implementation Greeter
- (int)compute:(int)x {
    return x + 1;
}
@end

int helper(int y) {
    return y * 2;
}
";
    check_func_space::<ObjcParser, _>(src, "greeter.m", |root| {
        assert_eq!(root.kind, SpaceKind::Unit, "root is the translation unit");
        let top: Vec<(SpaceKind, Option<&str>)> = root
            .spaces
            .iter()
            .map(|s| (s.kind, s.name.as_deref()))
            .collect();
        assert_eq!(
            top,
            vec![
                (SpaceKind::Interface, Some("Greeter")),
                (SpaceKind::Class, Some("Greeter")),
                (SpaceKind::Function, Some("helper")),
            ],
            "ObjC top-level spaces (kind, name) in source order; got {top:?}"
        );
        let impl_methods: Vec<Option<&str>> = root.spaces[1]
            .spaces
            .iter()
            .map(|s| s.name.as_deref())
            .collect();
        assert_eq!(
            impl_methods,
            vec![Some("compute")],
            "the @implementation's method is a named nested Function space"
        );
    });
}

/// The `line_span` carve-out for a root that parses to no children
/// (#1195).
///
/// These inputs are unreachable through `check_metrics` and the
/// integration harnesses, both of which append a trailing newline (see
/// `.claude/rules/testing.md`), so they go through `space_verbatim`.
#[cfg(all(test, feature = "rust"))]
mod childless_root_span {
    use crate::test_support::space_verbatim;
    use crate::{LANG, MetricsOptions};

    fn span(source: &str) -> (usize, usize) {
        let space = space_verbatim(LANG::Rust, source.as_bytes(), MetricsOptions::default());
        (space.start_line, space.end_line)
    }

    #[test]
    fn a_whitespace_only_file_spans_its_lines() {
        // tree-sitter collapses a childless root to a point at the end
        // of the whitespace, so `start_row` is the *last* row: before
        // #1195 the guard returned `0..0` and the general rule would
        // have returned the inverted `3..2`. Neither is the answer every
        // other space gives, which is `1..line_count`.
        for source in ["\n", "\n\n", "   \n  \n", "   ", "\t\n\n\n"] {
            let lines = source.lines().count();
            assert_eq!(
                span(source),
                (1, lines),
                "whitespace-only source {source:?} has {lines} lines"
            );
        }
    }

    #[test]
    fn an_empty_file_keeps_the_degenerate_zero_span() {
        // The one input with no lines at all. `1..0` would be inverted,
        // which is what the containment invariant forbids, so `0..0` is
        // deliberate rather than an oversight.
        assert_eq!(span(""), (0, 0));
        assert_eq!("".lines().count(), 0);
    }

    #[test]
    fn a_file_with_code_is_unaffected() {
        // Pins that the anchoring is scoped to the unit: a *nested*
        // space still takes `start_row + 1`, so a fix that hard-coded 1
        // everywhere would pass the cases above and fail here.
        assert_eq!(span("fn a() {}\n"), (1, 1));
        assert_eq!(span("fn a() {}\nfn b() {}\n"), (1, 2));

        let space = space_verbatim(
            LANG::Rust,
            b"\n\nfn a() {\n    let x = 1;\n}\n",
            MetricsOptions::default(),
        );
        assert_eq!(
            (space.spaces[0].start_line, space.spaces[0].end_line),
            (3, 5),
            "the nested function keeps its measured span"
        );
    }

    #[test]
    fn leading_blank_lines_do_not_shorten_the_unit() {
        // The second half of the same defect, found by the control test
        // above. tree-sitter starts the root at the first *token*, so a
        // file opening with blank lines reported a unit that omitted
        // them: 6 lines of source, span 4..6.
        for source in ["\n\nfn a() {}\n", "\nfn a() {}\n", "\n\n\nfn a() {}\n\n\n"] {
            let lines = source.lines().count();
            assert_eq!(
                span(source),
                (1, lines),
                "source {source:?} has {lines} lines"
            );
        }
        // A leading comment was never affected: comments are in the
        // tree, so the root already started at line 1.
        assert_eq!(span("// c\nfn a() {}\n"), (1, 2));
    }

    /// The other half of #1195's anchor, and the whole of #1247: the unit
    /// reported an anchored span while its `loc.sloc` was still measured
    /// from the root node's first token, so the two disagreed about how
    /// many lines the file has. Every case above is re-asserted here as a
    /// row count, because the span half passed throughout the year the
    /// `sloc` half was wrong.
    #[test]
    fn the_units_sloc_agrees_with_its_reported_span() {
        for source in [
            "\n\nfn a() {}\n",
            "\nfn a() {}\n",
            "\n\n\nfn a() {}\n\n\n",
            "// c\nfn a() {}\n",
            "fn a() {}\n",
            "   \n  \n",
        ] {
            let space = space_verbatim(LANG::Rust, source.as_bytes(), MetricsOptions::default());
            let rows = space.end_line - space.start_line + 1;
            assert_eq!(
                space.metrics.loc.sloc() as usize,
                rows,
                "source {source:?} spans {}..{}",
                space.start_line,
                space.end_line
            );
            assert_eq!(
                rows,
                source.lines().count(),
                "source {source:?} — and that span is the file's real line count"
            );
        }
    }
}

/// Constructs that carry executable code but no name token now open a
/// function space of their own (#1184).
///
/// Each was referenced nowhere outside the generated language enum, so
/// it opened no `FuncSpace` at all: its control flow was charged to the
/// enclosing class, `nom` / `wmc` under-counted, and `bca check` could
/// never flag one however complex it got. A Kotlin file of nothing but
/// property accessors reported `nom.functions == 0`.
///
/// Every assertion here is made on **both** the new space and the space
/// it took the value from. Asserting only the new one passes even if the
/// parent kept a duplicate count, which is the defect #1160 found in the
/// first draft of the equivalent Java fix.
#[cfg(test)]
mod nameless_construct_spaces {
    use crate::test_support::space_verbatim;
    use crate::{FuncSpace, LANG, MetricsOptions, SpaceKind};

    fn analyse(lang: LANG, source: &str) -> FuncSpace {
        space_verbatim(lang, source.as_bytes(), MetricsOptions::default())
    }

    /// The `(name, kind)` of every descendant space, in preorder.
    fn shape(space: &FuncSpace) -> Vec<(Option<&str>, SpaceKind)> {
        let mut out = vec![(space.name.as_deref(), space.kind)];
        for child in &space.spaces {
            out.extend(shape(child));
        }
        out
    }

    fn child<'a>(space: &'a FuncSpace, name: &str) -> &'a FuncSpace {
        fn find<'a>(s: &'a FuncSpace, name: &str) -> Option<&'a FuncSpace> {
            if s.name.as_deref() == Some(name) {
                return Some(s);
            }
            s.spaces.iter().find_map(|c| find(c, name))
        }
        find(space, name).unwrap_or_else(|| panic!("no space named {name:?} in {:?}", shape(space)))
    }

    #[test]
    #[cfg(feature = "kotlin")]
    fn kotlin_accessors_and_init_open_named_spaces() {
        let root = analyse(
            LANG::Kotlin,
            "class C {\n\
             \x20   var p: Int = 0\n\
             \x20       get() { if (field > 0) { return field } else { return 0 } }\n\
             \x20       set(v) { if (v > 0) { field = v } }\n\
             \x20   init { if (p > 0) { println(\"x\") } }\n\
             }\n",
        );
        assert_eq!(
            shape(&root),
            vec![
                (None, SpaceKind::Unit),
                (Some("C"), SpaceKind::Class),
                (Some("<get>"), SpaceKind::Function),
                (Some("<set>"), SpaceKind::Function),
                (Some("<init>"), SpaceKind::Function),
            ],
        );

        // The value moved rather than being duplicated: the accessor
        // owns its branch, and the class no longer owns it directly.
        let class = child(&root, "C");
        assert_eq!(
            class.metrics.cognitive.cognitive(),
            0,
            "class own cognitive"
        );
        assert_eq!(child(&root, "<get>").metrics.cognitive.cognitive(), 2);
        assert_eq!(child(&root, "<set>").metrics.cognitive.cognitive(), 1);
        assert_eq!(child(&root, "<init>").metrics.cognitive.cognitive(), 1);

        // `nom.functions` deliberately stays 0. The issue calls an
        // accessor-only file reporting `nom.functions == 0` the worst of
        // the set, and this fix does *not* change that number: `Nom`
        // keys on `is_func`, and these constructs are `is_func_space`
        // only, because a Kotlin accessor is not a callable anyone names
        // at a call site (`p.foo`, not `p.getFoo()`) and counting it as
        // a method would have `npm` bill the same property once as an
        // attribute and again as a method.
        //
        // What the fix does change is everything the missing *space*
        // caused: each accessor now has its own metric scope, so its
        // complexity is no longer charged to the class and `bca check`
        // can flag it. WMC picks them up because it keys on the space
        // kind rather than on `is_func`.
        assert_eq!(
            root.metrics.nom.functions_sum(),
            0,
            "accessors are is_func_space, not is_func"
        );
        assert_eq!(
            root.metrics.wmc.class_wmc_sum(),
            6,
            "WMC keys on SpaceKind::Function, so it does pick them up"
        );
    }

    #[test]
    #[cfg(feature = "java")]
    fn java_static_initializer_opens_a_named_space() {
        let root = analyse(
            LANG::Java,
            "class C { static int x; static { if (x > 0) { x = 1; } else { x = 2; } } }\n",
        );
        assert_eq!(
            shape(&root),
            vec![
                (None, SpaceKind::Unit),
                (Some("C"), SpaceKind::Class),
                (Some("<static-init>"), SpaceKind::Function),
            ],
        );
        assert_eq!(child(&root, "C").metrics.cognitive.cognitive(), 0);
        assert_eq!(
            child(&root, "<static-init>").metrics.cognitive.cognitive(),
            2
        );
    }

    #[test]
    #[cfg(feature = "javascript")]
    fn javascript_class_static_block_opens_a_named_space() {
        let root = analyse(
            LANG::Javascript,
            "class C { static f; static { if (C.f) { C.f = 1; } else { C.f = 2; } } }\n",
        );
        assert_eq!(
            shape(&root),
            vec![
                (None, SpaceKind::Unit),
                (Some("C"), SpaceKind::Class),
                (Some("<static-init>"), SpaceKind::Function),
            ],
        );
        assert_eq!(child(&root, "C").metrics.cognitive.cognitive(), 0);
        assert_eq!(
            child(&root, "<static-init>").metrics.cognitive.cognitive(),
            2
        );
    }

    #[test]
    #[cfg(feature = "groovy")]
    fn groovy_static_initializer_opens_a_named_space() {
        // Groovy's `StaticInitializer` arm is hand-mirrored against its
        // own enum in checker/getter/cognitive; the Java test one screen
        // up cannot see it drift (#1184).
        let root = analyse(
            LANG::Groovy,
            "class C { static int x; static { if (x > 0) { x = 1 } else { x = 2 } } }\n",
        );
        assert_eq!(
            shape(&root),
            vec![
                (None, SpaceKind::Unit),
                (Some("C"), SpaceKind::Class),
                (Some("<static-init>"), SpaceKind::Function),
            ],
        );
        assert_eq!(child(&root, "C").metrics.cognitive.cognitive(), 0);
        assert_eq!(
            child(&root, "<static-init>").metrics.cognitive.cognitive(),
            2
        );
    }

    /// `ClassStaticBlock` is a distinct kind_id in each JS-family enum
    /// (Javascript 231, Typescript 258, Tsx 272, Mozjs 234), and each
    /// grammar's arm is hand-mirrored — a kind_id that drifted in one
    /// grammar would be invisible if only Javascript were checked
    /// (#1184). Javascript has its own test above; these sweep the
    /// other three on the identical source, gated per feature so a
    /// build with one grammar family still checks it.
    #[cfg(any(feature = "typescript", feature = "mozjs"))]
    fn assert_class_static_block_space(lang: LANG) {
        let root = analyse(
            lang,
            "class C { static f; static { if (C.f) { C.f = 1; } else { C.f = 2; } } }\n",
        );
        assert_eq!(
            shape(&root),
            vec![
                (None, SpaceKind::Unit),
                (Some("C"), SpaceKind::Class),
                (Some("<static-init>"), SpaceKind::Function),
            ],
            "space shape for {lang:?}"
        );
        assert_eq!(
            child(&root, "<static-init>").metrics.cognitive.cognitive(),
            2,
            "static block cognitive for {lang:?}"
        );
    }

    #[test]
    #[cfg(feature = "typescript")]
    fn typescript_and_tsx_class_static_blocks_open_named_spaces() {
        assert_class_static_block_space(LANG::Typescript);
        assert_class_static_block_space(LANG::Tsx);
    }

    #[test]
    #[cfg(feature = "mozjs")]
    fn mozjs_class_static_block_opens_a_named_space() {
        assert_class_static_block_space(LANG::Mozjs);
    }

    /// Regression for #1257: a stabby lambda (`->(z) { … }` /
    /// `->(z) do … end`) parses as a `Lambda` node that CONTAINS the
    /// `Block` / `DoBlock` for its body, and the unconditional
    /// `Block | DoBlock` arm in `RubyCode::is_func_space` promoted
    /// both — the `Lambda` space plus a phantom zero-metric anonymous
    /// Function nested inside it. The phantom inflated every
    /// space-count consumer: per-space threshold evaluation, the
    /// `function_spaces` average divisor (pinning per-file `min` at 0),
    /// and the ops tree. Each stabby form must open exactly one space.
    #[test]
    #[cfg(feature = "ruby")]
    fn ruby_stabby_lambda_opens_exactly_one_space() {
        for source in [
            "def m\n  h = ->(z) { z + 1 }\nend\n",
            "def m\n  h = ->(z) do\n    z + 1\n  end\nend\n",
        ] {
            let root = analyse(LANG::Ruby, source);
            assert_eq!(
                shape(&root),
                vec![
                    (None, SpaceKind::Unit),
                    (Some("m"), SpaceKind::Function),
                    (Some("<anonymous>"), SpaceKind::Function),
                ],
                "phantom lambda-body space for {source:?}"
            );
        }
    }

    /// The #1257 fixture end to end: a keyword `lambda`, an iterator
    /// block, and a `do…end` stabby lambda. The first two hang their
    /// `Block` off a `Call` — not a `Lambda` — so the block itself must
    /// STILL open its space; only the stabby form's own body is
    /// suppressed. `nom` is asserted alongside because the closure
    /// count (#465) and the space tree now share one lambda-body
    /// predicate and must move together: 3 closures, 4 functions total.
    #[test]
    #[cfg(feature = "ruby")]
    fn ruby_mixed_lambda_forms_open_one_space_each() {
        let root = analyse(
            LANG::Ruby,
            "def m\n  g = lambda { |z| z + 1 }\n  [1].each { |x| x }\n  h = ->(z) do z end\nend\n",
        );
        assert_eq!(
            shape(&root),
            vec![
                (None, SpaceKind::Unit),
                (Some("m"), SpaceKind::Function),
                (Some("<anonymous>"), SpaceKind::Function),
                (Some("<anonymous>"), SpaceKind::Function),
                (Some("<anonymous>"), SpaceKind::Function),
            ],
        );
        // `shape` flattens preorder, so the same list would also match a
        // wrongly *nested* arrangement: pin the three lambda spaces as
        // leaf siblings under `m`.
        assert_eq!(root.spaces.len(), 1);
        let m = &root.spaces[0];
        assert_eq!(m.spaces.len(), 3);
        assert!(m.spaces.iter().all(|s| s.spaces.is_empty()));
        // expected: closures = keyword lambda + each-block + stabby
        // lambda = 3; total adds the named method `m` = 4.
        assert_eq!(root.metrics.nom.closures_sum(), 3);
        assert_eq!(root.metrics.nom.total(), 4);
    }

    /// A stabby lambda nested inside another's body: the inner `Lambda`
    /// still opens its own space (its parent is the outer body block,
    /// not a `Lambda`), nested under the outer's — one space per
    /// lambda, no phantoms at either depth.
    #[test]
    #[cfg(feature = "ruby")]
    fn ruby_nested_stabby_lambdas_open_one_space_each() {
        let root = analyse(LANG::Ruby, "f = ->(x) { ->(y) { x + y } }\n");
        assert_eq!(
            shape(&root),
            vec![
                (None, SpaceKind::Unit),
                (Some("<anonymous>"), SpaceKind::Function),
                (Some("<anonymous>"), SpaceKind::Function),
            ],
        );
        // `shape` flattens preorder, so pin the nesting explicitly: the
        // inner lambda's space sits inside the outer's, not beside it.
        assert_eq!(root.spaces.len(), 1);
        assert_eq!(root.spaces[0].spaces.len(), 1);
        assert!(root.spaces[0].spaces[0].spaces.is_empty());
    }

    /// Sibling constructs with the same synthesised name are allowed to
    /// collide, exactly as multiple `<anonymous>` siblings already do.
    /// Pinned so the collision reads as a decision rather than an
    /// oversight — inventing an index would make the name unstable under
    /// an unrelated edit.
    #[test]
    #[cfg(feature = "java")]
    fn two_static_initializers_produce_two_identically_named_spaces() {
        let root = analyse(
            LANG::Java,
            "class C { static int x; static { x = 1; } static { x = 2; } }\n",
        );
        let names: Vec<_> = shape(&root)
            .into_iter()
            .filter(|(_, k)| *k == SpaceKind::Function)
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, vec![Some("<static-init>"), Some("<static-init>")]);
    }
}

/// Every cognitive nesting slot is freed by the node that last reads
/// it, so the map is bounded by the traversal stack rather than the
/// tree (#1375). No metric value can tell the two apart — the retained
/// entries were dead — so the walk records whether any slot survived
/// it, and this asserts none did on each path that seeds one: a plain
/// nested-control-flow walk, the two overrides that write a child's
/// slot ahead of the walk (Python comprehension clauses, Tcl `try`
/// handler bodies), and an `exclude_tests` prune, which pops a seeded
/// node without computing or propagating it.
#[test]
#[cfg(all(feature = "cpp", feature = "python", feature = "tcl", feature = "rust"))]
fn walk_frees_every_cognitive_nesting_slot() {
    use crate::spaces::compute::nesting_slots_retained;
    use crate::test_support::metrics_verbatim;
    use crate::{LANG, Metric};

    const CASES: &[(LANG, &str, bool)] = &[
        (
            LANG::Cpp,
            "int f(int a) { if (a) { while (a) { a--; } } else { return 1; } return 0; }\n",
            false,
        ),
        (
            LANG::Python,
            "def f(xs):\n    return [x for x in xs if x for y in xs]\n",
            false,
        ),
        (
            LANG::Tcl,
            "try { risky } on error {msg} { puts $msg } finally { cleanup }\n",
            false,
        ),
        (
            LANG::Rust,
            "fn f(a: u8) -> u8 { if a > 1 { a } else { 0 } }\n\
             #[test]\nfn t() { if true { assert!(f(2) == 2); } }\n",
            true,
        ),
    ];

    for &(lang, source, exclude_tests) in CASES {
        let before = nesting_slots_retained::observed();
        // Restricted to the one metric that seeds a slot, per
        // `metrics_verbatim`'s own guidance (#1127): it is also the
        // minimal selection that still enables both freeing sites, so
        // nothing else can be what keeps the map empty.
        let metrics = metrics_verbatim(
            lang,
            source.as_bytes(),
            MetricsOptions::default()
                .with_only(&[Metric::Cognitive])
                .with_exclude_tests(exclude_tests),
        );
        assert!(
            metrics.cognitive.cognitive_sum() > 0,
            "{lang:?}: fixture must exercise cognitive, else no slot was ever seeded"
        );
        assert_eq!(
            nesting_slots_retained::observed(),
            before,
            "{lang:?}: a nesting slot outlived the walk"
        );
    }
}
