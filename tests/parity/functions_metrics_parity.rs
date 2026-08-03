//! Cross-language parity test for the three seams that answer *"which
//! nodes are functions?"*.
//!
//! `spaces::compute::metrics_inner` (behind [`analyze`]) asks the
//! source-aware `Checker::promotes_to_func_space_with_code`.
//! `function::function` (behind [`Ast::functions`], `bca functions` and
//! the web `/function` endpoint) and the `"function"` filter in
//! `parser::filters` (behind [`Ast::find`], `bca find --type function`
//! and `bca count --type function`) each carry their own copy of the
//! decision. Nothing forces the three to agree.
//!
//! Both of the latter asked the byte-less `Checker::is_func`, which
//! cannot see Elixir's `def` / `defp` / `defmacro` / `defmacrop` — they
//! are not grammar productions but plain `Call` nodes whose target
//! identifier text spells the keyword (#275). So `bca functions` and
//! `bca find --type function` printed nothing, and exited 0, for an
//! Elixir file whose `bca metrics` tree was a full module/function tree
//! (#1162). #1130 had already fixed the same defect in the third seam,
//! `ops`.
//!
//! Two claims, in both directions, because either alone is satisfiable
//! by a broken seam: a filter that matches nothing satisfies the subset
//! claim, and one that matches everything satisfies the coverage claim.
//!
//! The fixture table is [`super::ops_metrics_space_parity::fixture`], an
//! exhaustive `match` on [`LANG`], so a new language variant fails to
//! compile until a fixture is supplied.

use big_code_analysis::{Ast, FuncSpace, LANG, MetricsOptions, Source, SpaceKind, analyze};

use super::ops_metrics_space_parity::fixture;

/// Whether `name` is one `Getter::get_func_space_name` synthesised for a
/// space whose AST node carries no name of its own.
///
/// These are the one legitimate asymmetry between the two sides. Every
/// language promotes such a node to a `SpaceKind::Function` space, but
/// only some call it a *function*: Rust's `ClosureExpression`, Go's
/// `FuncLiteral`, Perl's and PHP's anonymous `sub`/`function`, Ruby's
/// `lambda` and Lua's `function` expression are `is_func_space` without
/// being `is_func`, while a JavaScript arrow function and an iRules
/// `when` block are both. `bca functions` therefore reports some such
/// spaces and not others, which is a per-grammar judgement this test has
/// no business pinning — so the coverage claim below is scoped to spaces
/// that carry a real name.
///
/// The five names are enumerated rather than matched by shape. A
/// `starts_with('<') && ends_with('>')` predicate reads as equivalent
/// but is not: Ruby's spaceship operator is a real method, and
/// `def <=>(other)` produces a space named literally `<=>`, which such a
/// predicate would exempt from the coverage claim below. Enumerating
/// also makes a sixth synthesised name fail loudly rather than inherit
/// the exemption silently.
///
/// `<anonymous>` is the trait default; #1184 added the other four for
/// constructs that carry executable code but no name token, each
/// `is_func_space` without being `is_func` for exactly the reason above.
const SYNTHESISED_NAMES: &[&str] = &["<anonymous>", "<get>", "<set>", "<init>", "<static-init>"];

fn is_synthesised_name(name: &str) -> bool {
    SYNTHESISED_NAMES.contains(&name)
}

/// A space or span, reduced to the fields all three seams report.
type Fun = (Option<String>, usize, usize);

/// Every `SpaceKind::Function` space in the metrics tree, in preorder.
fn metrics_functions(space: &FuncSpace, out: &mut Vec<Fun>) {
    if space.kind == SpaceKind::Function {
        out.push((space.name.clone(), space.start_line, space.end_line));
    }
    for child in &space.spaces {
        metrics_functions(child, out);
    }
}

/// Renders one side as sorted lines, for a failure message that shows
/// both lists rather than a `Vec` debug dump the reader has to align by
/// eye.
fn render(funs: &[Fun]) -> String {
    let mut lines: Vec<String> = funs
        .iter()
        .map(|(name, start, end)| format!("  {name:?} lines {start}..{end}"))
        .collect();
    lines.sort();
    lines.join("\n")
}

#[test]
fn every_named_function_space_is_reported_by_functions_and_find() {
    let mut checked = 0;

    for lang in LANG::into_enum_iter() {
        if !lang.is_enabled() {
            continue;
        }
        checked += 1;

        let (source, ext) = fixture(lang);
        let name = format!("parity.{ext}");

        let space = analyze(
            Source::new(lang, source.as_bytes()).with_name(Some(name.clone())),
            MetricsOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{lang:?}: analyze failed: {e}"));

        let ast = Ast::parse(Source::new(lang, source.as_bytes()).with_name(Some(name)))
            .unwrap_or_else(|e| panic!("{lang:?}: parse failed: {e}"));

        let mut from_metrics = Vec::new();
        metrics_functions(&space, &mut from_metrics);

        let from_functions: Vec<Fun> = ast
            .functions()
            .into_iter()
            .map(|span| (span.name, span.start_line, span.end_line))
            .collect();

        // Every span `functions()` reports must be a function the
        // metrics walk also found, at the same lines. This is the
        // direction that catches over-reporting — swapping
        // `is_func_with_code` for `promotes_to_func_space_with_code`
        // fails here, because an Elixir `defmodule` is a Class space —
        // and it re-pins the shared `spaces::line_span` arithmetic
        // (#1163) across the two independent walks.
        for fun in &from_functions {
            assert!(
                from_metrics.contains(fun),
                "{lang:?}: functions() reports {fun:?}, which is not a Function space in \
                 the metrics tree\nmetrics Function spaces:\n{}\nfunctions():\n{}",
                render(&from_metrics),
                render(&from_functions),
            );
        }

        // …and every *named* function the metrics walk found must come
        // back out of `functions()`. This is the #1162 direction: before
        // the fix, Elixir's `bar` was here and nowhere else.
        for fun in &from_metrics {
            if fun.0.as_deref().is_some_and(is_synthesised_name) {
                continue;
            }
            assert!(
                from_functions.contains(fun),
                "{lang:?}: the metrics tree has a Function space {fun:?} that functions() \
                 does not report\nmetrics Function spaces:\n{}\nfunctions():\n{}",
                render(&from_metrics),
                render(&from_functions),
            );
        }

        // `bca find --type function` reaches a third copy of the
        // decision — the `"function"` arm of `parser::filters`, which
        // applies the predicate with `Ancestors::unknown()` rather than
        // the walk's known chain. Compared by start line rather than by
        // name because `find` yields raw nodes: two functions sharing a
        // start line would be a grammar impossibility, and the sorted
        // multiset still catches a count mismatch.
        let found = ast
            .find(&["function".to_owned()])
            .unwrap_or_else(|e| panic!("{lang:?}: find failed: {e}"));
        let mut find_starts: Vec<usize> = found
            .iter()
            .map(|node| node.as_tree_sitter().start_position().row + 1)
            .collect();
        find_starts.sort_unstable();
        let mut function_starts: Vec<usize> = from_functions.iter().map(|f| f.1).collect();
        function_starts.sort_unstable();
        assert_eq!(
            find_starts,
            function_starts,
            "{lang:?}: find(\"function\") and functions() disagree on which nodes are \
             functions\nfunctions():\n{}",
            render(&from_functions),
        );

        // A language whose fixture yields no function at all satisfies
        // every assertion above vacuously — which is exactly the state
        // #1162 left Elixir in. `Ccomment` and `Preproc` are the C-family
        // helper grammars and genuinely have no function concept (see the
        // fixture table's comment on those two arms).
        if !matches!(lang, LANG::Ccomment | LANG::Preproc) {
            assert!(
                !from_functions.is_empty(),
                "{lang:?}: functions() reports nothing, so the parity assertions above \
                 hold vacuously\nmetrics Function spaces:\n{}",
                render(&from_metrics),
            );
        }
    }

    // Every language is feature-gated, so a build with none enabled
    // leaves a zero-iteration loop and a test that reports green while
    // asserting nothing.
    assert!(
        checked > 0,
        "at least one language feature must be enabled for this test to mean anything"
    );
}
