//! Synthetic scaling inputs, and the probes built from them.
//!
//! # The two axes
//!
//! A tree grows in two directions, and a walk can be linear in one
//! while being quadratic in the other. [`Axis::Depth`] shapes nest: the
//! size parameter is the number of AST levels. [`Axis::Width`] shapes
//! do not nest at all; the size parameter is the number of siblings
//! under one fixed-depth parent. #1100 is why both are here — the fix
//! it originally proposed was linear on every nesting shape and made a
//! flat 2 000-item file 94x slower by that issue's measurement, which
//! no depth probe can see.
//!
//! # Why every generator is affine in its size
//!
//! A generator that indents each nesting level makes the *input* grow
//! quadratically, so a walk that is perfectly linear in bytes still
//! looks superlinear in depth. That mistake invalidated two published
//! measurements during the #1052 / #1062 work before it was spotted.
//! Every generator here therefore emits a constant number of bytes per
//! level (or per sibling) and no indentation at all, which the
//! `byte_growth_is_affine` unit test below pins, rather than leaving it
//! a convention someone has to remember.

use std::fmt::Write as _;
use std::hint::black_box;

use big_code_analysis::{
    Ast, AstCfg, AstNode, CodeMetrics, LANG, Metric, MetricsError, MetricsOptions, Ops,
};

/// Renders a source shape at a given size — a nesting depth on
/// [`Axis::Depth`], a sibling count on [`Axis::Width`].
///
/// Every implementation must be affine in that size: `len(n)` is
/// `base + per_unit * n`. See the module docs for why.
pub type Render = fn(usize) -> String;

/// C#: a qualified type with one generic argument per segment.
#[must_use]
pub fn csharp_qualified_generics(depth: usize) -> String {
    format!("class C {{ {}A<T> field; }}\n", "A<T>.".repeat(depth))
}

/// Elixir: multiple timeout clauses accepted by the grammar but rejected
/// by the compiler. An analyzer must still process them in linear time.
#[must_use]
pub fn elixir_many_timeouts(width: usize) -> String {
    format!(
        "receive(do: ({}x -> :a), after: ({}1 -> :t))\n",
        "x -> :a; ".repeat(width),
        "1 -> :t; ".repeat(width)
    )
}

/// Elixir: one function head carrying `depth` repeated guards, which
/// the grammar nests as a chain of `when` operators, one per guard.
/// Valid Elixir (the guards are alternatives), and the shape the
/// guard-position climb is quadratic in when asked once per token.
#[must_use]
pub fn elixir_repeated_guards(depth: usize) -> String {
    format!(
        "defmodule M do\n  def f(x) {}do\n    x\n  end\nend\n",
        "when is_integer(x) ".repeat(depth)
    )
}

/// Rust: `fn f() -> i32 { (((…1…))) }`.
///
/// The paren chain is the shape that first surfaced both the `tokens`
/// per-leaf ancestor walk (#1052) and the `cognitive` nesting lookup
/// (#1062): it is pure nesting with two leaves per level and no
/// statement structure to dilute the walk.
#[must_use]
pub fn nested_parens(depth: usize) -> String {
    format!(
        "fn f() -> i32 {{ {}1{} }}\n",
        "(".repeat(depth),
        ")".repeat(depth)
    )
}

/// Python: `x = (a and (a and (… a …)))`.
///
/// The linear control for [`nested_ternaries`]: one `and` operator per
/// nesting level, which `Cyclomatic` counts from the token's own kind.
/// Python's block syntax cannot nest statements without indenting them,
/// which would make the input grow quadratically; parenthesised
/// expressions are the only Python shape that nests at constant bytes
/// per level.
#[must_use]
pub fn nested_ands(depth: usize) -> String {
    format!("x = {}a{}\n", "(a and ".repeat(depth), ")".repeat(depth))
}

/// Python: `x = (1 if a else (1 if a else … 1 …))`.
///
/// One `else` token per nesting level. Python's `Cyclomatic` asks each
/// one whether it opens a `for` / `while` / `try` else-clause — a
/// two-link parent/grandparent test that short-circuits on the first
/// link here, because a conditional expression's `else` has no
/// `else_clause` parent. That one link was still `O(depth)` while it
/// went through `Node::parent`, which is what made the shape quadratic
/// before #1096; it now indexes the walker's ancestor chain.
/// [`nested_ands`] is the same nesting through an arm that never looks
/// up.
#[must_use]
pub fn nested_ternaries(depth: usize) -> String {
    format!(
        "x = {}1{}\n",
        "(1 if a else ".repeat(depth),
        ")".repeat(depth)
    )
}

/// C: `int main(){ while (a) { … 1; … } }`.
///
/// The linear control for [`nested_ifs`]: structurally identical, but
/// `while_statement` has no `Checker::is_else_if` predicate hanging off
/// it, so it exercises the nesting map without the per-node
/// `Node::parent` call.
#[must_use]
pub fn nested_whiles(depth: usize) -> String {
    format!(
        "int main(){{ {}1;{} }}\n",
        "while (a) { ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// C: `int main(){ if (a) { … 1; … } }`.
///
/// `Checker::is_else_if` asks every `if_statement` whether its parent
/// is an `else` clause. It reads that parent off the ancestor chain the
/// walker hands down, so the answer is `O(1)`; recovering it with
/// `Node::parent` instead costs `O(depth)`, because `tree_sitter` stores
/// no parent pointer and resolves a parent by descending from the root.
/// This probe is what keeps that regression (#1084) from returning.
#[must_use]
pub fn nested_ifs(depth: usize) -> String {
    format!(
        "int main(){{ {}1;{} }}\n",
        "if (a) { ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// Rust: `fn f(x: bool) -> bool { !!!…x }`.
///
/// One `!` per nesting level, and `Getter::get_op_type`'s `BANG` arm
/// asks each one whether its parent is an `inner_doc_comment_marker`
/// (`//!`). That parent now comes off the walker's ancestor chain;
/// calling `Node::parent` instead is `O(depth)` each, which made
/// `Halstead` quadratic in nesting depth for the six grammars whose
/// operator classification consults a parent (#1096).
/// [`nested_parens`] is the same nesting through an arm that does not.
#[must_use]
pub fn nested_nots(depth: usize) -> String {
    format!("fn f(x: bool) -> bool {{ {}x }}\n", "!".repeat(depth))
}

/// C: `int main(){ int x; { x = 1; { x = 1; … } } }`.
///
/// The linear control for [`nested_ifs`] under `Abc`: one
/// `assignment_expression` per nesting level, which the metric counts
/// from the node's own kind. No `if` / `while` head means no condition
/// slot, so the C-family container walker — the one that reads the
/// slot's parent to decide whether it sits in boolean context — is
/// never entered.
#[must_use]
pub fn nested_blocks(depth: usize) -> String {
    format!(
        "int main(){{ int x; {}x = 1;{} }}\n",
        "{ x = 1; ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// C: `int main(){ while (a) { int x; … } }`.
///
/// One `declaration` per nesting level. `loc`'s C-family arm resolves
/// each declaration's logical-line contribution with
/// `Node::count_specific_ancestors`, whose `stop` predicate is
/// `compound_statement` — the brace directly above every declaration
/// here. The probe pins two things at once: that the stop predicate
/// keeps the walk to one step per declaration, and that the step reads
/// the walker's ancestor chain rather than calling `Node::parent`
/// (#1084). Losing either turns this shape quadratic.
#[must_use]
pub fn nested_declarations(depth: usize) -> String {
    format!(
        "int main(){{ {}1;{} }}\n",
        "while (a) { int x; ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// Elixir: a module of nested `quote` blocks, each wrapping a `def`.
///
/// Elixir's `is_func` asks `elixir_is_inside_quote_block` for every
/// `def`-like call, because a `def` inside `quote` is quoted AST
/// rather than a definition. That predicate walks the ancestor chain
/// reading each ancestor's call keyword out of the source bytes. It
/// short-circuits on the first `quote` ancestor, so a `def` directly
/// inside one costs `O(1)` — provided the steps come off the walker's
/// chain and not from `Node::parent`, which is `O(depth)` each (#1084).
/// Lose either the short-circuit or the chain and the shape turns
/// quadratic.
///
/// One `def` sits *outside* the nesting so the metric has something
/// to count — every `def` inside a `quote` is correctly not a
/// function, which would otherwise make the reading zero at every
/// depth.
#[must_use]
pub fn nested_quotes(depth: usize) -> String {
    format!(
        "defmodule M do\ndef g do\n:ok\nend\n{}:ok\n{}end\n",
        "quote do\ndef f do\n:ok\nend\n".repeat(depth),
        "end\n".repeat(depth)
    )
}

/// Elixir: nested `if` blocks, each holding a `def`, none under a
/// `quote`.
///
/// The complement of [`nested_quotes`]. There every `def` sits one step
/// under a `quote`, so `elixir_is_inside_quote_block`'s short-circuit
/// fires at once and the probe reads linear whatever the climb costs.
/// Here no ancestor is a `quote`, so a climb runs to the root from every
/// `def`: one per level is `O(depth²)` over the file, measured at
/// exponent 2.0 (0.12 s / 0.47 s / 1.97 s at depths 1000 / 2000 / 4000)
/// before the answer was folded down the walker's chain. The `def`s are
/// counted by `nom`, so the reading grows with depth.
#[must_use]
pub fn nested_unquoted_defs(depth: usize) -> String {
    format!(
        "defmodule M do\n{}:ok\n{}end\n",
        "if true do\ndef f do\n:ok\nend\n".repeat(depth),
        "end\n".repeat(depth)
    )
}

/// Rust: `fn f() { if a {} fn f() { … let x = 1; … } }`.
///
/// Each level opens a `FuncSpace`, so this drives the space-nesting
/// bookkeeping and the recursive `FuncSpace` tree that #1056 had to
/// bound. Inner `fn f` shadows are legal — each sits in its own block
/// scope.
///
/// The `if` is what makes the shape readable by `cognitive`. A chain of
/// bare functions carries no cognitive weight at all, so the metric
/// would score zero at every depth; with one `if` per level the reading
/// is `n(n+1)/2`, because `increment_function_depth` gives the function
/// at level *k* a function-nesting depth of *k* and every `if` is
/// penalised by the depth of the function holding it. That makes the
/// value column sensitive to the function-depth walk this probe times
/// (#1062), not only to the walk's cost.
#[must_use]
pub fn nested_fns(depth: usize) -> String {
    format!(
        "{}let x = 1;{}\n",
        "fn f() { if a {} ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// Rust: [`nested_fns`] with one nesting level per source row.
///
/// The same space nesting, spread over rows instead of packed onto one.
/// That is what makes it a `loc` probe: `Ploc` / `Cloc` keep a set of
/// physical rows per space and union each child's into its parent, so
/// here the set folded upward at level *k* holds `O(depth - k)` rows
/// where [`nested_fns`] folds a single row at every level (#1109).
///
/// Still affine in depth — nine bytes for the opening row, two for the
/// closing one, and no indentation, per the module docs.
#[must_use]
pub fn nested_fns_by_row(depth: usize) -> String {
    format!(
        "{}let x = 1;\n{}",
        "fn f() {\n".repeat(depth),
        "}\n".repeat(depth)
    )
}

/// JavaScript: `function f() { function f() { … 1; … } }`.
///
/// The linear control for [`nested_arrows`]: one function per nesting
/// level and one `FuncSpace` per level, as there, but *declared* rather
/// than written as an expression. `function_declaration` is a distinct
/// grammar production, so `Checker::is_func` answers it from the node's
/// own kind and never starts the ancestor walk. What is left is the
/// space-nesting bookkeeping the two shapes share.
///
/// Named for the distinction that matters — declared vs expression —
/// rather than for the language, so it does not read as a variant of
/// [`nested_fns`], which is the Rust shape.
#[must_use]
pub fn nested_declared_functions(depth: usize) -> String {
    format!(
        "{}1;{}\n",
        "function f() { ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// Rust: `#[inline] fn f() { #[inline] fn f() { … } }`.
///
/// [`nested_fns`] with an outer attribute on every function, which is
/// what makes it an `exclude_tests` probe: the prune hook only scans
/// for a preceding `#[…]` run once the node's kind is an item kind, so
/// a shape without attributes leaves the scan with nothing to walk. The
/// attribute is `#[inline]` rather than `#[test]` on purpose — a test
/// attribute prunes the outermost function and the walk stops there,
/// timing nothing (#1100).
///
/// Still affine in depth, and no `if`: the reading comes from `nom`, so
/// nothing here needs cognitive weight.
#[must_use]
pub fn nested_attributed_fns(depth: usize) -> String {
    format!(
        "{}let x = 1;{}\n",
        "#[inline] fn f() { ".repeat(depth),
        "} ".repeat(depth)
    )
}

/// Rust: `#[inline] fn f() {} #[inline] fn f() {} …`, all at file
/// scope.
///
/// [`nested_attributed_fns`] rotated onto the width axis: the same
/// `exclude_tests` attribute scan, but the attributed items are
/// siblings of one another rather than nested, so the size parameter
/// is the parent's child count and the AST depth never moves. This is
/// the shape #1100's rejected fix was quadratic on — an unconditional
/// forward pass over the parent's children costs `O(children)` per
/// item, which on a flat file is `O(n^2)`; #1100 measured 94x on
/// 2 000 items. It is also the shape `bindgen` output has.
///
/// Duplicate `fn f` names are a type-check error, not a parse error,
/// so the grammar accepts the repetition and the walk sees exactly the
/// shape it would on distinctly-named items.
#[must_use]
pub fn wide_attributed_fns(width: usize) -> String {
    format!("{}\n", "#[inline] fn f() {} ".repeat(width))
}

/// Rust: `depth` nested `fn f`, then one run of `3 * depth`
/// `#[cfg(test)]` rows on a single innermost `fn t`.
///
/// The diagonal the two probes above cannot reach, and the shape #1446
/// was quadratic on. Each grows one axis with the other pinned:
/// [`nested_attributed_fns`] carries one attribute per level, and
/// [`wide_attributed_fns`] carries thousands of one-attribute items at
/// depth 1. Neither produces a *long run*, and the prune's cost is per
/// run, not per attribute.
///
/// The `3 *` is what makes the shape adversarial rather than merely
/// large. `forward_attribute_scan_budget` allows a parent
/// `3 * depth` children wide before the reading flips to the
/// depth-priced sibling walk, so a run sized to that budget is
/// guaranteed to take the forward, `O(children)` branch — the budget
/// selects *into* the expensive reading exactly here. Asking once per
/// attribute then read the run `run` times: 0.08 / 0.34 / 1.21 / 5.18 s
/// at depths 250 / 500 / 1 000 / 2 000, against 0.05 s for the deepest
/// with `exclude_tests` off.
///
/// The reading is `nom.total()`, which counts the `depth` retained
/// wrappers and not the pruned `fn t`. That is deliberate: a reading
/// taken from the *pruned* item would be this probe's own defect
/// output, and a future change to what the prune removes would zero it
/// and hand the gate a flattering exponent forever. The wrappers are
/// ordinary production code, so the reading is `depth` whether or not
/// the prune fires at all.
#[must_use]
pub fn nested_fns_with_attribute_run(depth: usize) -> String {
    format!(
        "{}{}fn t() {{}}\nlet x = 1;\n{}",
        "fn f() {\n".repeat(depth),
        "#[cfg(test)]\n".repeat(3 * depth),
        "}\n".repeat(depth),
    )
}

/// Rust: `fn p() {}\n#[cfg(test)]\nmod m {}\n` repeated at file scope.
///
/// The shape [`nested_attributed_fns`] and [`wide_attributed_fns`]
/// cannot be: both use `#[inline]` precisely so the walk does *not*
/// prune, which leaves the number of pruned nodes at zero on every
/// existing probe and the prune's own bookkeeping unpriced (#1417).
/// Here two nodes per item are pruned — the `mod` and, since #1431, the
/// `#[cfg(test)]` sibling marking it — so the count grows with the size
/// parameter.
///
/// The retained `fn p()` is what keeps the reading non-zero and moving
/// with the size: `sloc` is `width` against a `3 * width` file, so a
/// prune that stopped recording rows would read `3 * width` and one
/// that took the production row with them would read `0`. Before #1431
/// the surviving attribute row played that part, and the shape was two
/// rows per item with no production code at all — which now measures
/// zero and prices nothing.
///
/// The cost this prices is `Sloc::excluded_rows`: one `insert_range`
/// per pruned node during the walk, and one `retain_range` plus three
/// `subtract`s over the accumulated bitset at finalization. All four
/// are word-wise passes, so the shape is linear — but the bitset grows
/// downward with a `Vec::splice` (`LineSet::reserve`), which would be
/// quadratic in words if rows ever arrived descending. A flat file
/// visits them ascending; this probe is what keeps that measured.
#[must_use]
pub fn wide_cfg_test_mods(width: usize) -> String {
    "fn p() {}\n#[cfg(test)]\nmod m {}\n".repeat(width)
}

/// Go: one lead comment run of `width` rows above a single `func`.
#[must_use]
pub fn go_comment_run(width: usize) -> String {
    format!("package p\n\n{}func f() {{}}\n", "// d\n".repeat(width))
}

/// Rust: one opening inner-documentation run in a function body.
#[must_use]
pub fn rust_inner_doc_run(width: usize) -> String {
    format!("fn f() {{\n{}let x = 1;\n}}\n", "//! d\n".repeat(width))
}

/// C#: a property whose accessor list holds `width` doc comments before
/// its one accessor.
#[must_use]
pub fn csharp_accessor_docs(width: usize) -> String {
    format!(
        "class C {{\n  int X {{\n{}    get;\n  }}\n}}\n",
        "    /// d\n".repeat(width)
    )
}

/// PHP: `width` docblocks between a function's signature and its body.
#[must_use]
pub fn php_signature_docs(width: usize) -> String {
    format!("<?php\nfunction f()\n{}{{ }}\n", "/** d */\n".repeat(width))
}

/// Kotlin: `width` `/** */` blocks between a function's signature and
/// its body.
#[must_use]
pub fn kotlin_signature_docs(width: usize) -> String {
    format!("fun f()\n{}{{ }}\n", "/** d */\n".repeat(width))
}

/// Java: `width` dangling Javadoc comments, then a documented field, in
/// one class body — a stretch javac reads only the last of.
#[must_use]
pub fn java_dangling_docs(width: usize) -> String {
    format!(
        "class C {{\n{}    int x;\n}}\n",
        "    /** d */\n".repeat(width)
    )
}

/// Java: `width` units of a Javadoc comment and a package declaration,
/// then a type. Invalid Java that tree-sitter-java parses without error:
/// each package asks whether a type follows it.
#[must_use]
pub fn java_package_docs(width: usize) -> String {
    format!("{}class C {{}}\n", "/** d */\npackage p;\n".repeat(width))
}

/// Rust: `width` units of an outer doc comment and an attribute above
/// one function, every doc comment of which rustc attaches.
#[must_use]
pub fn rust_attributed_docs(width: usize) -> String {
    format!("{}fn f() {{}}\n", "/// d\n#[inline]\n".repeat(width))
}

/// C#: `width` units of a `///` doc comment and a plain `//` comment,
/// then a type. Roslyn keeps only the last doc comment, since each plain
/// comment ends the run read back from the type.
#[must_use]
pub fn csharp_dangling_docs(width: usize) -> String {
    format!("{}class C {{}}\n", "/// d\n// p\n".repeat(width))
}

/// PHP: `width` `/** */` blocks at the top of a file, then a class. The
/// first asks phpDocumentor's file-doc question, which reads every block
/// after it to the class; each later one stops at the block before it.
#[must_use]
pub fn php_file_docblock(width: usize) -> String {
    format!("<?php\n{}class C {{}}\n", "/** d */\n".repeat(width))
}

/// JavaScript: `width` `@file` blocks, each a `JSDoc` virtual comment that
/// documents the file, then a function.
#[must_use]
pub fn js_standalone_tags(width: usize) -> String {
    format!("{}function f() {{}}\n", "/** @file d */\n".repeat(width))
}

/// C: `width` doc blocks, each above an `#include` Doxygen reads past,
/// ending at the end of the file. The stretch runs over every member, so
/// each comment that asks where it ends must stop at the next comment
/// rather than scan to the end; none documents anything.
#[must_use]
pub fn c_dangling_docs(width: usize) -> String {
    "/** d */\n#include <x.h>\n".repeat(width)
}

/// C++: `width` `@file` blocks gathered by none after them, each read by
/// Doxygen's structural-command scan and then by the gathering walk from
/// the stretch's last comment, ending at the end of the file.
#[must_use]
pub fn cpp_structural_docs(width: usize) -> String {
    "/** @file d */\n#include <x.h>\n".repeat(width)
}

/// C: a doc block above each of `depth` nested `#ifdef`s, with one
/// declaration at the bottom. Every block asks whether the conditional
/// below it holds an entry, which reads the nest beneath it.
#[must_use]
pub fn c_nested_conditional_docs(depth: usize) -> String {
    format!(
        "{}int x;\n{}",
        "/** d */\n#ifdef A\n".repeat(depth),
        "#endif\n".repeat(depth)
    )
}

/// Python: an attribute docstring followed by additional docstrings.
#[must_use]
pub fn python_additional_docstrings(width: usize) -> String {
    format!("x = 1\n{}", "\"\"\"d\"\"\"\n".repeat(width))
}

/// Python: `x = (((…a…)))`, every `(` of which asks whether its
/// parentheses wrap a bare string statement.
#[must_use]
pub fn python_nested_parens(depth: usize) -> String {
    format!("x = {}a{}\n", "(".repeat(depth), ")".repeat(depth))
}

/// How many statements [`python_paren_bare_strings`] renders: enough that
/// the walk outweighs the fixed per-analysis cost at the shallowest depth.
const PAREN_BARE_STRING_STATEMENTS: usize = 100;

/// Python: bare string statements, `(((…"s"…)))`, each `depth`
/// parentheses deep, every parenthesis of which asks whether it wraps a
/// bare string — and here each does.
#[must_use]
pub fn python_paren_bare_strings(depth: usize) -> String {
    format!("{}\"s\"{}\n", "(".repeat(depth), ")".repeat(depth))
        .repeat(PAREN_BARE_STRING_STATEMENTS)
}

/// Rust: `/// d` above each of `depth` nested `mod m { … }`, so every
/// level adds a doc comment whose placement test sits one level deeper.
#[must_use]
pub fn rust_nested_doc_mods(depth: usize) -> String {
    format!(
        "{}{}",
        "/// d\nmod m {\n".repeat(depth),
        "}\n".repeat(depth)
    )
}

/// Go: `depth` nested anonymous struct types, each field doc-commented.
#[must_use]
pub fn go_nested_doc_fields(depth: usize) -> String {
    format!(
        "package p\n\ntype T {}int{}\n",
        "struct {\n\t// F doc\n\tF ".repeat(depth),
        "\n}".repeat(depth)
    )
}

/// Ruby: `depth` nested `if`s, each holding a documented constant after
/// a first statement, so the comment sits inside the `then` body.
#[must_use]
pub fn ruby_nested_doc_constants(depth: usize) -> String {
    format!(
        "{}{}",
        "if true\nx = 0\n# Doc.\nMAX = 1\n".repeat(depth),
        "end\n".repeat(depth)
    )
}

/// Lua: a module block, then `width` units of a tagless `---` block
/// directly under a call — no item follows it, so the first-block test
/// scans back — and a documented function, whose block is the reading.
#[must_use]
pub fn lua_tagless_blocks(width: usize) -> String {
    format!(
        "--- M.\n\nlocal M = {{}}\n{}",
        "print(0)\n--- Note.\n\nprint(1)\n--- F.\nfunction f() end\n".repeat(width)
    )
}

/// Perl: `width` statements, `__END__`, and a data section of `width`
/// units, each a `=begin comment` block, a documenting POD block and a
/// data row. The section is one opaque grammar leaf however long it is,
/// so the statements are what widen the root as the section grows.
#[must_use]
pub fn perl_data_pod(width: usize) -> String {
    format!(
        "{}__END__\n{}",
        "1;\n".repeat(width),
        "=begin comment\nx\n=end comment\n=cut\n=head1 A\n\na\n=cut\nrow\n\n".repeat(width)
    )
}

/// Rust: `fn f000000() { let v000000 = 000000; } fn f000001() { … }`,
/// all at file scope.
///
/// The width shape for the space-merge arm. Each function opens its own
/// `FuncSpace`, so the size parameter is the number of *direct children*
/// the file's `Unit` space accumulates — the `C` in the `O(C x U)` that
/// #1106 was filed about, where `U` is the parent's merged Halstead
/// vocabulary.
///
/// Every identifier and literal is unique to its function, which is what
/// makes `U` grow with `C` rather than saturate: `HalsteadMaps::operands`
/// is keyed by source text, so a file of N identical functions has a
/// vocabulary of constant size and would keep the merge arm linear no
/// matter how it is written.
///
/// Six-digit zero padding, not the bare index, so the bytes per function
/// stay constant up to a million siblings. `byte_growth_is_affine` does
/// **not** cover this: it samples 10 / 20 / 30, where the unpadded form
/// is affine too. What the padding buys is affine growth across the 4-
/// to 5-digit boundary, which this probe's own ladder crosses at 10 000
/// — unpadded, 4 000 / 8 000 / 16 000 render 128 670 / 260 670 / 542 670
/// bytes and inflate the fitted exponent by ~0.06 with nothing in
/// `mod tests` able to see it.
///
/// Rust permits leading zeros in a decimal literal, so `000000` is an
/// ordinary `integer_literal` and not an octal escape or a parse error.
#[must_use]
pub fn wide_distinct_fns(width: usize) -> String {
    let mut source = String::new();
    for i in 0..width {
        // `fmt::Write for String` never returns `Err`, so this is the
        // one shape whose generator has a `Result` to discard. The
        // alternatives clippy leaves are `format!`-into-`push_str` and
        // `map(format!).collect()`, and it rejects both.
        let _ = writeln!(source, "fn f{i:06}() {{ let v{i:06} = {i:06}; }}");
    }
    source
}

/// JavaScript: `function f() { let v000000 = 0, v000001 = 1, …; return
/// v000000; }` — one `let` statement with `width` declarators.
///
/// The width shape for the JS-family ABC `const` predicate (#1277).
/// Each `=` asks whether its declarator sits under a `const`
/// declaration, and the first form of that predicate answered by
/// scanning every sibling declarator for the keyword — `O(width)` per
/// `=`, so `O(width²)` per statement, invisible to every depth probe.
/// `let` is the spelling that pays it in full: `const` finds its
/// keyword at child 0 and `var` opens no `lexical_declaration` at all.
/// Six-digit padding for the reason `wide_distinct_fns` gives.
#[must_use]
pub fn wide_let_declarators(width: usize) -> String {
    let mut source = String::from("function f() { let ");
    for i in 0..width {
        if i > 0 {
            source.push_str(", ");
        }
        let _ = write!(source, "v{i:06} = {i:06}");
    }
    source.push_str("; return v000000; }\n");
    source
}

/// Rust: `#[cfg(all(all(… test …)))] fn gone() {}` plus one retained
/// function.
///
/// Nesting inside one *attribute* rather than in the code: the cfg
/// predicate is classified by a string-level mini-parser reading the
/// attribute's text, so what grows with `depth` is the length of a
/// single attribute while the file keeps its two items. (The token
/// tree nests along with the text, which is what
/// `shapes_nest_proportionally_to_depth` sees.)
///
/// The retained `fn keep` is what gives the probe a non-zero reading:
/// the attributed item is pruned, so a file holding only it scores zero
/// at every depth.
#[must_use]
pub fn nested_cfg_predicate(depth: usize) -> String {
    format!(
        "#[cfg({}test{})] fn gone() {{}}\nfn keep() {{ let x = 1; }}\n",
        "all(".repeat(depth),
        ")".repeat(depth)
    )
}

/// JavaScript: `const f = a => { a => { … 1 … } };`.
///
/// The JS grammars have no production for "named function": `const f =
/// () => …` and `g(() => …)` are the same `arrow_function` node, and
/// `Checker::is_func` tells them apart by walking upward until it meets
/// either a name binding or a frame that proves positional use. Here
/// the enclosing `statement_block` stops the walk after two steps —
/// which is the point. Two steps was still `O(depth)`, because
/// `Node::parent` descends from the root, and the `PropertyIdentifier`
/// adjacency check the predicate ends with scanned siblings the same
/// way; both are why a shape with one arrow per level was quadratic
/// before #1088 despite every walk being O(1) steps long.
///
/// Every level is an arrow function, so `nom` reads `depth`: the
/// outermost is bound to `f` and counts as a function, the rest are
/// closures. [`nested_declared_functions`] is the same nesting without
/// the walk.
#[must_use]
pub fn nested_arrows(depth: usize) -> String {
    format!(
        "const f = {}1{};\n",
        "a => { ".repeat(depth),
        " }".repeat(depth)
    )
}

/// JavaScript: `const cb = [() => 1, () => 2, …];` — `width` arrows as
/// siblings in one array literal.
#[must_use]
pub fn wide_arrow_array(width: usize) -> String {
    let arrows = (0..width)
        .map(|i| format!("() => {i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("const cb = [{arrows}];\n")
}

/// C++: `bool f(bool a){ return a && a && … && a; }`.
///
/// `&&` is left-associative, so the chain parses as a left-nested
/// `binary_expression` one level deeper per operand: the operator at
/// depth *k* sits under *k* chain ancestors. A per-token question about
/// what *encloses* the chain — "is this inside a `requires` clause?" —
/// answered by climbing those ancestors is `O(depth)` per operator and
/// `O(depth^2)` over the chain. That is the climb #1533 records, which
/// cost an 8 000-term chain 4.4 s in a debug build, against 0.09 s once
/// reverted.
#[must_use]
pub fn cpp_and_chain(depth: usize) -> String {
    format!("bool f(bool a){{ return {}a; }}\n", "a && ".repeat(depth))
}

/// Perl: `sub f { return g(-foo => 1, -foo => 1, …); }` — one call
/// whose argument list holds `width` dash-prefixed keys.
///
/// The width shape for the #1545 dash-key workaround. tree-sitter-perl
/// reads each `-foo` as a file test, and the workaround asks whether a
/// `=>` follows it. Its first form found that `=>` by scanning the list
/// from its start, `O(width)` per key and so `O(width²)` per list: 7.5 s
/// for 4 000 keys in a release build.
#[must_use]
pub fn perl_dash_keys(width: usize) -> String {
    format!(
        "sub f {{ return g({}); }}\n",
        vec!["-foo => 1"; width].join(", ")
    )
}

/// Perl: `sub f { return g(foo => 1, -foo => 1, …); }` — one call whose
/// argument list holds `width` bareword keys, alternately plain and
/// signed.
///
/// The width shape for the #1572 bareword-key workaround, which asks of
/// every bareword call, and of every `-` before one, whether a `=>`
/// follows it. Unlike [`perl_dash_keys`], whose `-foo` the grammar reads
/// as a file test, neither key here has a letter that is a file test, so
/// this reaches the `-` and bareword-call path instead.
#[must_use]
pub fn perl_bareword_keys(width: usize) -> String {
    let keys: Vec<&str> = (0..width)
        .map(|i| {
            if i % 2 == 0 {
                "name => 1"
            } else {
                "-name => 1"
            }
        })
        .collect();
    format!("sub f {{ return g({}); }}\n", keys.join(", "))
}

/// Perl: `my @r = (/a/, /a/, …);` — one list of `width` bare matches.
///
/// The width shape for the Perl ABC pattern-role check, which asks of
/// every pattern whether a `( … )` around it holds it as its value. Its
/// first form read the list's last operand to answer, scanning the whole
/// list per pattern: 7.7 s for 8 000 patterns in a release build.
#[must_use]
pub fn perl_pattern_list(width: usize) -> String {
    format!("my @r = ({});\n", vec!["/a/"; width].join(", "))
}

/// Kotlin: `val s = "$a $a …"` — one string holding `width` short
/// `$name` interpolations.
///
/// The width shape for Kotlin Halstead's short-interpolation test, which
/// reads the token before every name. It found that token by scanning
/// the string's children from its start, quadratic in the name count on
/// valid input: 11.5 s for 8 000 names in a release build (#1584).
#[must_use]
pub fn kotlin_short_interpolations(width: usize) -> String {
    format!("val s = \"{}\"\n", "$a ".repeat(width))
}

/// Elixir: `f(x, x, …, k: if(a, do: 1), k: if(a, do: 1), …)` — one call
/// with `width` positional arguments before `width` keyword pairs.
///
/// The width shape for the keyword-section lookup #1574 added, which
/// asks of every pair whether its keyword list is the call's trailing
/// argument, and of every keyword-form clause which Call owns it. Its
/// first form re-read the argument list from its front per pair,
/// quadratic in a call with many of both: 5.4 s for 16 000 arguments in
/// a release build. The `if` in each value is the cognitive reading.
#[must_use]
pub fn elixir_mixed_arguments(width: usize) -> String {
    format!(
        "f({}{})\n",
        "x, ".repeat(width),
        vec!["k: if(a, do: 1)"; width].join(", ")
    )
}

/// Kotlin: `inline fun f(noinline p0: () -> Unit, …) {}` — one function
/// with `width` modified parameters.
///
/// The width shape for #1558's recovery guard, which asks of every
/// parameter modifier whether the next token is an inserted name. Its
/// first form found that token by scanning the parameter list from its
/// start, quadratic in the parameter count.
#[must_use]
pub fn kotlin_noinline_params(width: usize) -> String {
    let mut source = String::from("inline fun f(");
    for i in 0..width {
        if i > 0 {
            source.push_str(", ");
        }
        let _ = write!(source, "noinline p{i:06}: () -> Unit");
    }
    source.push_str(") {}\n");
    source
}

/// Perl: `my $r = $v || $v || … || $v;`.
///
/// `||` is left-associative, so the chain is a left-nested spine one
/// `binary_expression` deeper per operand. Perl ABC asks every chain
/// node whether a misparsed comparison (#1273) absorbed its leading
/// operand, by climbing the spine of leading slots to its root. Every
/// node but the bottom one leads with a chain, which scores itself, so
/// only the bottom one climbs; climbing from each was `O(depth)` per
/// operator: ~15 s for 8 000 operands in a release build.
#[must_use]
pub fn perl_or_chain(depth: usize) -> String {
    format!("my $r = {}$v;\n", "$v || ".repeat(depth))
}

/// Perl: `my $r = $x > $v && $v && … && $v;`.
///
/// [`perl_or_chain`] behind the #1273 misparse it exists to detect:
/// tree-sitter-perl lets `>` swallow the chain to its right, so the
/// whole spine is the comparison's right operand and the bottom node's
/// climb ends on the `>`, answering "absorbed". The two shapes share
/// the climb and differ in where it stops.
#[must_use]
pub fn perl_misparsed_comparison(depth: usize) -> String {
    format!("my $r = $x > {}$v;\n", "$v && ".repeat(depth))
}

/// Perl: `sub f { $a && $b; $a && $b; … }` — one block of `width`
/// two-operand chains.
///
/// The width shape for the same misparse check. Each chain's leading
/// operand is a bare variable, so each climbs, and the climb stops one
/// step up at the block to read the operator before the chain.
/// Scanning the block's statements from its start for it was
/// `O(width)` per chain: 16.3 s for 10 000 statements in a release
/// build.
#[must_use]
pub fn perl_and_statements(width: usize) -> String {
    format!("sub f {{ {}}}\n", "$a && $b; ".repeat(width))
}

/// Perl: `my @r = (` followed by `width` copies of
/// `last if $c =~ /x/;⏎{⏎$a && $b,⏎`, never closed.
///
/// The width shape for a file whose tail fails to parse, as
/// `perl5db.pl`'s does: tree-sitter-perl gives up on the unclosed list,
/// the root becomes an `ERROR`, and the tail's tokens are its direct
/// children, one flat array. Perl ABC asks of each `=~` whether it binds
/// a rewrite, of each bareword (`last`) whether a `=>` follows it, and
/// of each `&&` chain which operator precedes it. Each question reads a
/// neighbour in the `ERROR`, and a seek by byte over a flat array scans
/// it from its start, so each was `O(width)` until the walk recorded the
/// `ERROR`'s children (#1584).
#[must_use]
pub fn perl_error_tail(width: usize) -> String {
    format!(
        "my @r = ({}",
        "last if $c =~ /x/;\n{\n$a && $b,\n".repeat(width)
    )
}

/// Perl: `my @r = (` followed by `width` copies of
/// `{⏎- and,⏎not,⏎-a => 1,⏎`, never closed.
///
/// [`perl_error_tail`]'s flat `ERROR`, holding the key shapes Halstead
/// asks about: whether a `=>` follows each `-a`, and whether a `-` signs
/// each `and` / `not` key.
#[must_use]
pub fn perl_error_tail_keys(width: usize) -> String {
    format!("my @r = ({}", "{\n- and,\nnot,\n-a => 1,\n".repeat(width))
}

/// Perl: `my @r = (` followed by `width` copies of
/// `{⏎last,⏎name => 1,⏎`, never closed.
///
/// [`perl_error_tail`]'s flat `ERROR`, holding a bareword call and a
/// bareword key the grammar also reads as a call: the `call` filter
/// asks of each whether a `=>` follows it.
#[must_use]
pub fn perl_error_tail_calls(width: usize) -> String {
    format!("my @r = ({}", "{\nlast,\nname => 1,\n".repeat(width))
}

/// Perl: `my @r = (` followed by `width` copies of
/// `{⏎die => 1,⏎exit => 1,⏎exit,⏎`, never closed.
///
/// [`perl_error_tail`]'s flat `ERROR`, holding the `die` / `exit` keys
/// nexits asks about: whether a `=>` follows each, which makes it a key
/// rather than an exit. The bare `exit` is the one exit per copy.
#[must_use]
pub fn perl_error_tail_exit_keys(width: usize) -> String {
    format!(
        "my @r = ({}",
        "{\ndie => 1,\nexit => 1,\nexit,\n".repeat(width)
    )
}

/// Bash: `if [ "$a" = 0 -o "$a" = 0 -a … ]; then :; fi`, the
/// connectives alternating.
///
/// tree-sitter-bash lets `=` swallow the connectives around it, so the
/// test parses as one left-nested spine of comparisons, one level per
/// term (#1536). Bash cognitive ends each connective's boolean
/// sequence where the outermost comparison of its run does; finding
/// that end by climbing the run from every `binary_expression` was
/// `O(depth)` per node: 1.9 s for 4 000 terms in a release build. The
/// connectives alternate so the reading — one sequence per switch, the
/// textual-order scoring the mis-nested tree forces — grows with the
/// depth.
#[must_use]
pub fn bash_test_connective_run(depth: usize) -> String {
    let mut source = String::from("if [ ");
    for i in 0..depth {
        source.push_str(if i % 2 == 0 {
            "\"$a\" = 0 -o "
        } else {
            "\"$a\" = 0 -a "
        });
    }
    source.push_str("\"$a\" = 0 ]; then :; fi\n");
    source
}

/// Bash: `(( a = a = … = 0 ))`.
///
/// A valid chained arithmetic assignment, one `binary_expression`
/// deeper per `=`. The same `=` token is a string comparison inside
/// `[ … ]` / `[[ … ]]`, and Bash ABC once told the two apart by
/// climbing from every `=` through the expression nodes above it to
/// see whether a test held them — `O(depth)` per `=` here: 3.4 s for
/// 8 000 in a release build. Each test now counts its own comparisons
/// from the top, so an arithmetic `=` asks nothing.
#[must_use]
pub fn bash_arith_assign_chain(depth: usize) -> String {
    format!("(( {}0 ))\n", "a = ".repeat(depth))
}

/// Bash: `if [[ $a = 0 && $a = 0 && … ]]; then :; fi`.
///
/// The `[[ … ]]` form of the [`bash_test_connective_run`] misparse:
/// `=` swallows the `&&` around it, so the test is one deep spine. It
/// is the shape the comparison-counting change in Bash ABC has to stay
/// linear on, since every `=` in it is a comparison: 1.2 s for 16 000
/// terms in a release build while each `=` climbed to its test.
#[must_use]
pub fn bash_eq_and_test(depth: usize) -> String {
    format!(
        "if [[ {}$a = 0 ]]; then :; fi\n",
        "$a = 0 && ".repeat(depth)
    )
}

/// C++: `int f(int a){ return a + a + … + a; }`.
///
/// The shape control for [`cpp_and_chain`]: the same left-nested
/// `binary_expression`, built from an operator no short-circuit arm
/// matches, so a regression that moves the chain probes and not this
/// one is in those arms rather than in walking a deep expression.
#[must_use]
pub fn cpp_plus_chain(depth: usize) -> String {
    format!("int f(int a){{ return {}a; }}\n", "a + ".repeat(depth))
}

/// Tcl: `eval {` nested `depth` times around `puts hi`.
///
/// `eval` evaluates every argument, so each level is a script body the
/// alterator keeps rather than flattening to a verbatim leaf — a
/// question `keeps_children` answers off the ancestor chain the export
/// walk carries (#1381). One brace level costs three exported levels
/// (`braced_word` → `command` → `word_list`), so the export nests
/// three times deeper than the source.
#[must_use]
pub fn tcl_nested_eval(depth: usize) -> String {
    format!("{}puts hi{}\n", "eval {".repeat(depth), "}".repeat(depth))
}

/// Tcl: one `eval` line of `width` `{a b} "x"` pairs with a stray `]`
/// in the middle of its argument list.
///
/// The `]` is an `ERROR` slot of the `word_list`, so the list cannot be
/// read by position and every braced word in it keeps the construct-wide
/// script answer. Each word asks `argument_slots_are_readable` whether
/// that is so; the answer is memoised per list under a walk that lends
/// its `FoldCache`, and was an `O(width)` sibling scan per word before
/// #1686 — 4 s for 16 000 words. `has_error` is transitive, so the one
/// `]` admits the scan for every word of the line.
///
/// The `"x"` beside each braced word is the `string` filter's reading:
/// a braced word of an unreadable list is a script, never a string, so
/// without it the count probe would score zero on its own shape. It is
/// the one difference from the same-named fixture in
/// `big-code-analysis-ast`'s `test_support`, which the counter tests
/// use and this crate cannot import; keep the two in step by hand.
#[must_use]
pub fn tcl_wide_erroring_eval(width: usize) -> String {
    let half = "{a b} \"x\" ".repeat(width / 2);
    format!("eval {half}] {half}\n")
}

/// A brace-list `switch` with `width` repeated `-nocase` options and
/// `width` arms, so the option list and the arm list grow together.
#[must_use]
pub fn tcl_wide_switch(width: usize) -> String {
    use std::fmt::Write as _;

    let options = "-nocase ".repeat(width);
    let mut arms = String::new();
    for i in 0..width {
        let _ = writeln!(arms, "    p{i} {{ puts {i} }}");
    }
    format!("switch {options}-- $x {{\n{arms}    default {{ puts d }}\n}}\n")
}

/// The walk a probe times.
///
/// Four seams reach an AST walk — `Ast::metrics`, `Ast::ops`, the
/// `Ast::count` walk `bca find` shares, and the `Ast::dump` export —
/// and they share no options type, so the selection a metric probe
/// needs lives on the variant that uses it rather than on every probe
/// (#1110).
#[derive(Clone, Copy)]
pub enum Workload {
    /// `Ast::metrics` under `selection`.
    Metrics {
        /// Metric selection handed to `MetricsOptions::with_only`. Kept
        /// as narrow as the probe allows so an unrelated metric's cost
        /// cannot dominate and misattribute a regression.
        selection: &'static [Metric],
        /// Whether the walk runs under `MetricsOptions::exclude_tests`,
        /// which is the only way to reach `Checker::should_skip_subtree`
        /// and the cfg-predicate classifier behind it (#1100). Lives on
        /// this variant rather than on [`Probe`] for the reason
        /// `selection` does: `Ast::ops` takes no options at all, so a
        /// probe that could set it there would be setting something
        /// nothing reads.
        exclude_tests: bool,
        /// Headline value the selection produces on the probe's shape.
        reading: fn(&CodeMetrics) -> u64,
    },
    /// `Ast::ops`, the Halstead operator/operand walk, which takes no
    /// metric selection.
    Ops {
        /// Headline value the walk produces on the probe's shape.
        reading: fn(&Ops) -> u64,
    },
    /// `Ast::count` under one `--type` filter, the walk `bca count` and
    /// `bca find` share. Its reading is the matching node count.
    Count {
        /// The filter, as `bca count --type` spells it.
        filter: &'static str,
    },
    /// `Ast::dump`, the AST export (#1684). Its reading is the number
    /// of nodes exported, counted after the export and before its
    /// teardown, so the timing is construction + that count + drop.
    ///
    /// Holds the two flags rather than an `AstCfg`, whose owned
    /// strings would make the workload non-`Copy`; the config is built
    /// at invocation with empty metadata.
    Dump {
        /// Whether each exported node carries its span.
        span: bool,
        /// Whether comment nodes are dropped from the export.
        comment: bool,
    },
}

impl Workload {
    /// The options this workload walks under, resolved once so option
    /// resolution stays outside the timed region. `Ast::ops` and
    /// `Ast::count` take none, so the default stands in and is ignored.
    #[must_use]
    pub fn options(self) -> MetricsOptions {
        match self {
            Self::Metrics {
                selection,
                exclude_tests,
                ..
            } => MetricsOptions::default()
                .with_only(selection)
                .with_exclude_tests(exclude_tests),
            Self::Ops { .. } | Self::Count { .. } | Self::Dump { .. } => MetricsOptions::default(),
        }
    }

    /// Walks `ast` once and returns the headline reading.
    ///
    /// The walk's product is `black_box`ed before the reading is taken,
    /// so an optimiser cannot narrow the walk to whatever the reading
    /// happens to touch.
    ///
    /// For [`Workload::Dump`] the reading is a second pass over the
    /// exported tree, and the tree is dropped before this returns, so
    /// the timed region is construction + that count + teardown. The
    /// count is `O(nodes)` and iterative, a constant factor on a walk
    /// that is itself `O(nodes)` at best, so it cannot move the fitted
    /// exponent; the drop is `AstNode`'s iterative one (#1056).
    ///
    /// # Errors
    ///
    /// Whatever the underlying seam raises — in practice, a language
    /// feature disabled in the build.
    pub fn walk(self, ast: &Ast, options: MetricsOptions) -> Result<u64, MetricsError> {
        Ok(match self {
            Self::Metrics { reading, .. } => reading(&black_box(ast.metrics(options)?).metrics),
            Self::Ops { reading } => reading(&black_box(ast.ops()?)),
            Self::Count { filter } => {
                let (matching, _) = black_box(ast.count(&[filter.to_owned()]));
                u64::try_from(matching).unwrap_or(u64::MAX)
            }
            Self::Dump { span, comment } => {
                let cfg = AstCfg {
                    id: String::new(),
                    language: String::new(),
                    comment,
                    span,
                };
                let response = black_box(ast.dump(cfg));
                u64::try_from(exported_nodes(response.root.as_ref())).unwrap_or(u64::MAX)
            }
        })
    }
}

/// How many nodes an export holds, counted on an explicit stack: the
/// depth shapes export thousands of levels, which a recursive count
/// would overflow on.
fn exported_nodes(root: Option<&AstNode>) -> usize {
    let mut pending: Vec<&AstNode> = root.into_iter().collect();
    let mut count = 0;
    while let Some(node) = pending.pop() {
        count += 1;
        pending.extend(&node.children);
    }
    count
}

/// The direction a probe's shape grows in.
///
/// A walk can be linear in one direction and quadratic in the other,
/// so the axis is what says which claim a probe's exponent supports.
/// It also selects the shape invariant the probe must satisfy:
/// `shapes_nest_proportionally_to_depth` for one, and
/// `shapes_widen_without_deepening` for the other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// The size parameter is the shape's nesting depth.
    Depth,
    /// The size parameter is the number of siblings under one parent,
    /// at a depth that does not change with it.
    Width,
}

/// One scaling probe: a shape, the axis it grows along, the workload
/// that exercises the hot path under test, and the complexity class
/// the walk is expected to stay within.
///
/// A probe's `reading` is reported alongside every timing so a reader
/// can see the walk did real work, and asserted non-zero by
/// `probe_workload_is_exercised`: a shape paired with a workload that
/// scores zero on it would time the walk's fixed overhead and report an
/// excellent exponent forever.
#[derive(Clone, Copy)]
pub struct Probe {
    /// Stable `<metric>/<shape>` identifier, used as the report key.
    pub name: &'static str,
    /// Language whose grammar the shape is written for.
    pub lang: LANG,
    /// Direction [`Probe::sizes`] grows the shape in.
    pub axis: Axis,
    /// The walk this probe times, and how its headline reading is taken.
    pub workload: Workload,
    /// Generator for the probe's input.
    pub render: Render,
    /// The three sizes measured — the values handed to [`Probe::render`]
    /// — each a doubling of the previous so the fitted exponent reads
    /// directly as "cost per doubling".
    pub sizes: [usize; 3],
    /// Upper bound on the fitted log-log exponent. A linear walk sits
    /// near 1.0 and a quadratic one near 2.0; the bounds below leave
    /// enough headroom that measurement noise cannot cross them but
    /// not enough that a class change hides.
    pub max_exponent: f64,
    /// What the probe covers, and why its bound is where it is.
    pub rationale: &'static str,
}

/// Depths for the probes whose walk is expected to be linear.
///
/// Large enough that the walk dominates the fixed per-analysis cost
/// (option resolution, root `FuncSpace` construction), small enough
/// that a *quadratic* regression is abandoned partway up the ladder
/// rather than run to completion. Bounding the total cost of such a
/// regression is `MAX_CELL_WALK`'s job, not this constant's: at these
/// depths a pre-#1052-magnitude blow-up costs a couple of minutes
/// before the budget fires, which is a report rather than the hung
/// job the retired wall-clock assertions were guarding against.
const LINEAR_DEPTHS: [usize; 3] = [1_000, 2_000, 4_000];

/// Sibling counts for the probes whose walk is expected to be linear
/// in a parent's child count.
///
/// Reasoned independently of [`LINEAR_DEPTHS`] rather than copied: a
/// width shape spends nothing on nesting, so the same numbers would
/// buy a different amount of walk. The top of the ladder is the scale
/// #1100 measured its 94x regression at — a flat file of 2 000
/// attributed items. The bottom is set from the measurement below it:
/// the 500-wide cell runs 1.86 ms, an order of magnitude above the
/// cheapest cells in the set (~0.2 ms), so fixed per-analysis cost is
/// a rounding error in the fit rather than a term flattening it.
const LINEAR_WIDTHS: [usize; 3] = [500, 1_000, 2_000];

/// Sibling counts for a width probe whose quadratic term is a *second*
/// pass over data the linear walk already touched, rather than extra
/// work per node.
///
/// Eight times [`LINEAR_WIDTHS`], and deliberately not harmonised down
/// to it. #1106's redundant pass costs one map visit per (child,
/// vocabulary-entry) pair while the walk it rides on costs a fixed
/// amount per token, so the ratio between the two terms is set by the
/// sibling count alone — enriching each function's vocabulary raises
/// both terms together and does not move it. On [`LINEAR_WIDTHS`] the
/// regression fits 1.37-1.39 against a 1.07-1.09 baseline: a real 2.1x
/// slowdown at 2 000 siblings that the bound would have passed. Here
/// the gate reads 1.09 clean and 2.17 regressed.
///
/// The taller ladder costs ~0.8 s of a `--gate` run over nine rounds
/// and a 624 KB largest input. A reintroduced quadratic pays ~6 s
/// there, and its slowest single walk — 0.5 s — is well inside
/// `scaling::MAX_CELL_WALK`, so the gate reports the exponent rather
/// than abandoning the probe as over budget.
const SPACE_MERGE_WIDTHS: [usize; 3] = [4_000, 8_000, 16_000];

/// Sibling counts for the four `tcl-wide-erroring-eval` probes.
///
/// Set by measurement, as [`SPACE_MERGE_WIDTHS`] was, and for the
/// opposite reason: the quadratic term is not small on [`LINEAR_WIDTHS`]
/// but the clean walk is. A clean cell there runs 0.2–1.8 ms, inside
/// scheduling jitter on a shared host, and the regressed fits wandered
/// with it — the export with its cache dropped fitted 2.08 on one run
/// and 1.62 on the next, the metric walk 1.81–1.82, each with a
/// neighbouring clean cell reading 2–3x its twin. On this ladder the
/// clean cells are 2–17 ms, every clean fit is 0.98–1.03 and every
/// regressed one 2.01–2.07 across three runs
/// (`docs/development/benchmarking.md`). The cost is the same ~0.8 s of
/// a `--gate` run the space-merge probes pay.
const SLOT_SCAN_WIDTHS: [usize; 3] = SPACE_MERGE_WIDTHS;

/// Depths for `nom/deep-attribute-run`, whose shape grows *two* axes
/// with one parameter.
///
/// Half [`LINEAR_DEPTHS`] at every rung, and reasoned rather than
/// harmonised. The shape renders `4 * depth` rows to a depth probe's
/// `depth`, so depth 2 000 here is already a 100 KB file with a
/// 6 000-attribute run — comfortably past where #1446's quadratic
/// showed (3.9x per doubling, ~2.0 fitted, 5.2 s at the top rung),
/// which is all the ladder has to reach.
///
/// Going one rung higher costs accuracy rather than buying coverage.
/// A 4 000-deep cell walks a 200 KB tree under an 8 000-entry ancestor
/// chain, and its per-byte cost runs 1.7x the shallowest cell's against
/// the ~1.25x [`LINEAR_BOUND`] is set from — enough cache drift on its
/// own to fit 1.37 with the walk linear. That is a bound with 0.13 of
/// headroom on a shared runner, and the ladder below fits 1.0-1.1
/// instead.
const ATTRIBUTE_RUN_DEPTHS: [usize; 3] = [500, 1_000, 2_000];

/// Depths for `loc/python-paren-bare-strings`, held under the 200 levels
/// Python's tokenizer lets parentheses nest. Past them a parenthesis's
/// climb to its statement stops at the cap, so the per-statement
/// quadratic the probe guards cannot show on [`LINEAR_DEPTHS`]; under it,
/// it did — 3.9 s for 80 KB of 200-deep statements (#1610).
const PAREN_CAP_DEPTHS: [usize; 3] = [50, 100, 200];

/// Bound for a probe expected to be linear in its size parameter.
///
/// Set from measurement, not from theory. A genuinely linear walk does
/// not fit exactly 1.0 over these sizes: the tree outgrows cache as
/// depth rises, so per-byte cost drifts up by roughly a quarter from
/// the shallowest cell to the deepest and every depth probe here fits
/// 0.94-1.31 on an idle host. Before #1084 the three ancestor-walk
/// probes fit 1.95-2.01; the midpoint of those two bands is the cut.
///
/// The width probe re-measured onto the same cut rather than
/// inheriting it. `nom/wide-attributed-fn` fits 0.97-1.00, with no
/// per-byte drift up its ladder to spend the headroom on, and #1100's
/// rejected forward-always scan takes it to 1.99 — 560.6 ms against
/// 7.2 ms at 2 000 items, a 78x of its own. One bound still covers
/// the set.
const LINEAR_BOUND: f64 = 1.5;

/// The scaling probe set.
///
/// One entry per hot path identified during the #1052 / #1062 / #1084 /
/// #1096 / #1109 work, plus the controls that make those readings
/// interpretable. Two kinds of control appear:
///
/// - *metric* controls, such as `nom/nested-while` and
///   `nom/nested-fn-rows`: the same shape under a metric that does not
///   run the path under test. `Cognitive` declares `Nom` as a
///   dependency, so the cognitive-attributable cost of a `cognitive/…`
///   row is its difference from the `nom/…` row on the same shape, not
///   the `cognitive` reading alone.
/// - *shape* controls, such as `loc/nested-while` and `loc/nested-fn`:
///   the same metric on the same nesting with the one feature that
///   triggers the path removed. That pairing is what attributed the
///   pre-#1084 quadratic readings to those calls rather than to nesting
///   in general, and it is what would localise a future regression the
///   same way.
///
/// See `docs/development/benchmarking.md` for the per-probe table.
pub const PROBES: &[Probe] = &[
    Probe {
        name: "dump/perl-error-tail-calls",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Dump {
            span: false,
            comment: false,
        },
        render: perl_error_tail_calls,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "The export must enumerate a flat recovery `ERROR`'s children \
                    without indexing each one: `Node::child` and \
                    `field_name_for_child` scan the child array to the index, \
                    so an indexed export is quadratic in the node's width. \
                    Routing `Ast::dump` back to the indexed builder takes this \
                    probe from 1.02 to 1.98 (#1684).",
    },
    Probe {
        name: "dump/wide-distinct-fns",
        lang: LANG::Rust,
        axis: Axis::Width,
        workload: Workload::Dump {
            span: true,
            comment: false,
        },
        render: wide_distinct_fns,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "The shape control for `dump/perl-error-tail-calls`: an \
                    ordinary wide list the grammar stores balanced, so both \
                    builders are `O(n log n)` on it and a regression that moves \
                    the recovery probe and not this one is in enumerating a \
                    flat node rather than in the export itself (#1684).",
    },
    Probe {
        name: "dump/tcl-nested-eval",
        lang: LANG::Tcl,
        axis: Axis::Depth,
        workload: Workload::Dump {
            span: true,
            comment: false,
        },
        render: tcl_nested_eval,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "The export's depth axis: every level is a script body the \
                    alterator keeps, which `keeps_children` decides off the \
                    ancestor chain the walk carries rather than by climbing \
                    with `Node::parent`. Handing it `Ancestors::unknown()` \
                    instead takes this probe from 0.75 to 2.00; the cursor \
                    walk must keep the chain in step with its own climbs \
                    (#1381, #1684).",
    },
    Probe {
        name: "dump/tcl-wide-erroring-eval",
        lang: LANG::Tcl,
        axis: Axis::Width,
        workload: Workload::Dump {
            span: false,
            comment: false,
        },
        render: tcl_wide_erroring_eval,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Every braced word of an erroring `eval` line asks \
                    `keeps_children` whether its argument list is readable, \
                    and the sibling scan that answers is memoised per list \
                    through the `FoldCache` the export lends. Dropping that \
                    cache from `Export::finish` scans the list once per \
                    word, quadratic in the line's width (#1686).",
    },
    Probe {
        name: "halstead/tcl-wide-erroring-eval",
        lang: LANG::Tcl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.total_operators(),
        },
        render: tcl_wide_erroring_eval,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "The metric walk's seam onto the same scan: each braced \
                    word's `{` asks `braced_word_op_type` whether it opens a \
                    block, which reaches the readability question through \
                    the word's parent — so the cache must survive \
                    `Ancestors::iter`, and the walk must lend one. Each \
                    walk lends its own, so this probe cannot stand in for \
                    the dump or count probes on the same shape (#1686).",
    },
    Probe {
        name: "ops/tcl-wide-erroring-eval",
        lang: LANG::Tcl,
        axis: Axis::Width,
        workload: Workload::Ops {
            // Width-invariant on purpose: every `{a b}` and `"x"` repeats
            // the same three operands, so the vocabulary is constant and
            // here only to prove the walk ran. The width signal is in
            // the timing column.
            reading: |ops| ops.operands.len() as u64,
        },
        render: tcl_wide_erroring_eval,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "`Ast::ops` reaches the same question through the same \
                    `braced_word_op_type` as the metric walk, on a walk of \
                    its own with a cache of its own (`ops_inner`), so the \
                    `halstead/` probe cannot stand in for it (#1686).",
    },
    Probe {
        name: "count/tcl-wide-erroring-eval",
        lang: LANG::Tcl,
        axis: Axis::Width,
        workload: Workload::Count { filter: "string" },
        render: tcl_wide_erroring_eval,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "The `bca count` / `bca find` seam onto the same scan: \
                    the `string` filter asks of every braced word whether it \
                    is a value, and `act_on_node` must lend the cache that \
                    memoises the answer. The reading is the one quoted word \
                    beside each braced one (#1686).",
    },
    Probe {
        name: "cyclomatic/elixir-repeated-guards",
        lang: LANG::Elixir,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cyclomatic],
            reading: |m| m.cyclomatic.cyclomatic_sum(),
        },
        render: elixir_repeated_guards,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Every `when` of a repeated guard is the `Guard` row's \
                    payer and cyclomatic pays each, so each asks whether its \
                    operator sits in a guard position — a climb through the \
                    nested `when` operators above it, quadratic per token \
                    (881 ms at 8,000 guards) until the answer was memoised \
                    per operator on the walk's `FoldCache` under \
                    `MemoQuestion::GuardOperator` (#1695). The reading is \
                    one decision per guard.",
    },
    Probe {
        name: "halstead/elixir-repeated-guards",
        lang: LANG::Elixir,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.total_operators(),
        },
        render: elixir_repeated_guards,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Every `when` of a repeated guard is the `Guard` row's \
                    payer, whose gate climbs the chain of nested `when` \
                    operators to find the guard position — quadratic in the \
                    chain when every token asks. Halstead bills the token \
                    in every position, so `ElixirCode::get_paying_role` \
                    bills it without asking; reading the row there made the \
                    Halstead-only walk quadratic (#1695). The reading is one \
                    operator per guard plus the head's own.",
    },
    Probe {
        name: "halstead/csharp-qualified-generics",
        lang: LANG::Csharp,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.total_operators(),
        },
        render: csharp_qualified_generics,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Each generic qualifier must be classified without climbing \
                    the remaining qualified-name ancestry.",
    },
    Probe {
        name: "abc/elixir-many-timeouts",
        lang: LANG::Elixir,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc, Metric::Cyclomatic],
            reading: |m| m.abc.conditions_sum(),
        },
        render: elixir_many_timeouts,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Malformed receive timeout clauses must not each rescan every \
                    message clause. The reading comes from the message clauses.",
    },
    Probe {
        name: "tokens/nested-paren",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Tokens],
            reading: |m| m.tokens.tokens_sum(),
        },
        render: nested_parens,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1052: `tokens` inherits the in-comment flag down the \
                    traversal. Reverting to the per-leaf ancestor walk is \
                    quadratic in nesting depth.",
    },
    // Deliberately no `cognitive/nested-paren`: parentheses carry no
    // cognitive weight, so the metric scores zero on that shape at
    // every depth and the probe would be timing a walk whose output
    // never changes. #1062's `get_nesting_from_map` is covered by
    // `cognitive/nested-while` below — the shape #1062's own
    // regression test uses, and one whose reading grows as
    // `n(n+1)/2`, so a walk that stopped inheriting nesting is
    // visible in the value column and not only in the timing.
    Probe {
        name: "cognitive/nested-while",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: nested_whiles,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1062 on a statement shape, and the linear control for \
                    `cognitive/nested-if`: identical structure, no \
                    `is_else_if` predicate.",
    },
    Probe {
        name: "nom/nested-while",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_whiles,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Metric control. `Cognitive` declares `Nom` as a \
                    dependency, so the cognitive-attributable cost is the \
                    difference between this row and `cognitive/nested-while`.",
    },
    Probe {
        name: "cognitive/nested-if",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: nested_ifs,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1084: `Checker::is_else_if` reads the enclosing \
                    `else` clause off the walker's ancestor chain. \
                    Recovering it with `Node::parent` is `O(depth)` per \
                    `if_statement`, which was quadratic across the 13 \
                    languages with an `is_else_if` impl.",
    },
    Probe {
        name: "loc/nested-while",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.lloc(),
        },
        render: nested_whiles,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `loc/nested-declaration`: the same \
                    nesting with no `declaration` node, so `loc` never \
                    reaches `Node::count_specific_ancestors`.",
    },
    Probe {
        name: "loc/nested-declaration",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.lloc(),
        },
        render: nested_declarations,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1084: `loc`'s C-family arm calls \
                    `Node::count_specific_ancestors` for every \
                    `declaration`. Its `stop` predicate fires on the \
                    enclosing `compound_statement` immediately, so the \
                    walk is one step, and that step now indexes the \
                    walker's ancestor chain instead of calling \
                    `Node::parent`. `loc/nested-while` is the same \
                    nesting without a declaration.",
    },
    Probe {
        name: "halstead/nested-paren",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.length(),
        },
        render: nested_parens,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `halstead/nested-not`: the same \
                    nesting through `get_op_type` arms that classify a \
                    token from its own kind, with no parent read.",
    },
    Probe {
        name: "halstead/nested-not",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.length(),
        },
        render: nested_nots,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1096: `Getter::get_op_type` asks every `!` token \
                    whether its parent is an inner-doc-comment marker. \
                    The answer now indexes the walker's ancestor chain; \
                    `Node::parent` was `O(depth)` per token across the \
                    six grammars whose operator classification reads a \
                    parent. `halstead/nested-paren` is the same nesting \
                    without the read.",
    },
    Probe {
        name: "abc/nested-block",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.assignments_sum(),
        },
        render: nested_blocks,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `abc/nested-if`: the same nesting \
                    with no condition slot, so the C-family container \
                    walker is never entered.",
    },
    Probe {
        name: "abc/nested-if",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: nested_ifs,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1096: every `if (…)` head routes its condition \
                    through the C-family container walker, which seeds \
                    its boolean-context flag from the slot's parent. \
                    That parent is now passed in by the caller that \
                    descended from it; resolving it with `Node::parent` \
                    was `O(depth)` per `if` across all twenty `Abc` \
                    impls. `abc/nested-block` is the same nesting \
                    without a condition slot.",
    },
    Probe {
        name: "cyclomatic/nested-and",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cyclomatic],
            reading: |m| m.cyclomatic.cyclomatic_sum(),
        },
        render: nested_ands,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `cyclomatic/nested-ternary`: the \
                    same nesting through an arm that counts the token \
                    from its own kind and never looks up.",
    },
    Probe {
        name: "cognitive/nested-and",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: nested_ands,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Every `boolean_operator` of a nested chain asks whether \
                    an enclosing one already paid the structural cost; \
                    counting every enclosing one, as the arm did until \
                    #1695, was quadratic in the chain (377 ms at depth 8,000 \
                    on this shape, 204 ms on the flat `a and a and …` the \
                    parser nests the same way), where stopping at the first \
                    is linear. The reading is the chain's one increment.",
    },
    Probe {
        name: "cyclomatic/nested-ternary",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cyclomatic],
            reading: |m| m.cyclomatic.cyclomatic_sum(),
        },
        render: nested_ternaries,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1096: Python's `Cyclomatic` asks every `else` token \
                    whether it opens a loop or `try` else-clause, through \
                    `Node::parent_grandparent_match`. Both links now index \
                    the walker's ancestor chain. `cyclomatic/nested-and` \
                    is the same nesting without the lookup.",
    },
    Probe {
        name: "loc/nested-quote",
        lang: LANG::Elixir,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.lloc(),
        },
        render: nested_quotes,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1096: Elixir's `loc` catch-all arm asks every named \
                    node whether its parent is a statement container, so \
                    unlike the other group-3 arms it fires per node \
                    rather than per construct. The parent now comes off \
                    the walker's ancestor chain. `nom/nested-quote` is \
                    the same shape under a metric that does not ask.",
    },
    Probe {
        name: "nom/nested-quote",
        lang: LANG::Elixir,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_quotes,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1084: Elixir's `is_func` asks \
                    `elixir_is_inside_quote_block` for every `def`. The \
                    predicate short-circuits on the first `quote` \
                    ancestor, and each step now indexes the walker's \
                    ancestor chain rather than calling `Node::parent`.",
    },
    Probe {
        name: "nom/elixir-unquoted-def",
        lang: LANG::Elixir,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_unquoted_defs,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1691: the same question as `nom/nested-quote` on the \
                    shape its short-circuit cannot help — a `def` under no \
                    `quote` at all climbed to the root, and one per level \
                    measured quadratic while that probe read linear. The \
                    answer is now folded down the walker's chain once per \
                    ancestor (lesson 82).",
    },
    Probe {
        name: "cognitive/elixir-mixed-arguments",
        lang: LANG::Elixir,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: elixir_mixed_arguments,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1574's keyword-section lookup asks of every keyword \
                    pair whether its list is the call's trailing argument \
                    (cognitive), and of every keyword-form clause which \
                    Call owns it (cyclomatic, ABC). Its first form found \
                    the trailing argument by reading the argument list \
                    from its front, once per pair: 0.10 s, 0.36 s, 1.4 s \
                    and 5.4 s for 2 000 to 16 000 arguments in a release \
                    build, exponent 1.94. The lookup now reads the list's \
                    siblings off the walk's chain. The reading is one \
                    `if` per pair.",
    },
    Probe {
        name: "nom/nested-fn",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "One `FuncSpace` per level: the space-nesting \
                    bookkeeping and the recursive `FuncSpace` tree #1056 \
                    had to bound. Also the metric control for \
                    `cognitive/nested-fn` below, which selects `Cognitive` \
                    on the same shape and so pays this row's cost too.",
    },
    Probe {
        name: "cognitive/nested-fn",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: nested_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1062: `increment_function_depth` asks every function \
                    node whether a function encloses it. The answer now \
                    comes off the walker's ancestor chain and is found two \
                    steps up; climbing with `Node::parent` instead cost \
                    `O(depth)` per step across the 19 call sites — 22 \
                    languages, counting the four the JS-family macro \
                    expands to. `nom/nested-fn` is the same shape without \
                    the cognitive walk.",
    },
    Probe {
        name: "ops/nested-fn",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Ops {
            // Depth-invariant on purpose: `nested_fns` reuses one
            // identifier at every level, so the root vocabulary is the
            // same four operands at any depth. It is here to prove the
            // walk produced something, which is all
            // `probe_workload_is_exercised` asks of it; the depth
            // signal is in the timing column.
            reading: |ops| ops.operands.len() as u64,
        },
        render: nested_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1110: the only probe that runs `ops_inner`, which \
                    was otherwise unmeasured. It covers the walk — the \
                    space stack, the Halstead map merge up it, and the \
                    per-space vocabulary render — on a shape whose \
                    vocabulary is constant, so the reading is the walk \
                    alone. The *vocabulary* term cannot be probed the \
                    same way: `Ops` publishes a `Vec<String>` per space \
                    and a parent's vocabulary is a superset of every \
                    descendant's, so a shape with distinct identifiers \
                    per level has quadratic output by construction and no \
                    implementation can fit under a linear bound. \
                    `nom/nested-fn` is the same shape through \
                    `Ast::metrics`, which merges the same maps without \
                    rendering them.",
    },
    Probe {
        name: "loc/nested-fn",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.lloc(),
        },
        render: nested_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `loc/nested-fn-rows`: the same \
                    function nesting with every level on one physical row, \
                    so each `Ploc::merge` up the space stack folds a \
                    one-row set. Isolates the per-merge overhead from the \
                    per-row cost the row-spread shape adds.",
    },
    Probe {
        name: "loc/nested-fn-rows",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.ploc(),
        },
        render: nested_fns_by_row,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1109: `Ploc` / `Cloc` union each space's physical-row \
                    set into its parent, so a row inside `D` nested spaces \
                    is folded `D` times. The sets are word-array bitsets \
                    and the fold is a word-wise OR; re-inserting element by \
                    element into a hash set instead put a probe per row on \
                    that path. `loc/nested-fn` is the same nesting with one \
                    row in total, and `nom/nested-fn-rows` is the same \
                    shape under a metric that keeps no per-row set.",
    },
    Probe {
        name: "nom/nested-fn-rows",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_fns_by_row,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Metric control for `loc/nested-fn-rows`: the same \
                    row-spread nesting under a metric whose merge is a \
                    counter add, so the loc-attributable cost of that row \
                    is its difference from this one.",
    },
    Probe {
        name: "nom/nested-declared-function",
        lang: LANG::Javascript,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_declared_functions,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `nom/nested-arrow`: one function and \
                    one `FuncSpace` per level as there, but declared with \
                    `function`, which `Checker::is_func` answers from the \
                    node's own kind without an ancestor walk.",
    },
    Probe {
        name: "nom/nested-arrow",
        lang: LANG::Javascript,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_arrows,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1088: the JS-family `Checker::is_func` / `is_closure` \
                    decide whether an `arrow_function` is bound to a name by \
                    walking upward. The enclosing `statement_block` stops \
                    that walk after two steps, but `Node::parent` is \
                    `O(depth)` per step, so the shape was quadratic; the \
                    steps now index the walker's ancestor chain. \
                    `nom/nested-declared-function` is the same nesting \
                    without the walk.",
    },
    Probe {
        name: "nom/wide-arrow-array",
        lang: LANG::Javascript,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: wide_arrow_array,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1696: the arrow form of the JS binding walk asks for a \
                    `property_identifier` sibling, an `O(width)` scan of the \
                    parent's children, and only when the ancestor test has \
                    not already bound the arrow. Evaluated unconditionally — \
                    which the first row-based spelling of the walk did — the \
                    scan ran once per arrow and an array of arrows was \
                    quadratic (4,000 arrows: 0.02 s → 4 s).",
    },
    Probe {
        name: "nom/nested-attributed-fn",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: true,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_attributed_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1100: under `exclude_tests` the walker asks every \
                    node whether it opens a test-only subtree, and Rust \
                    answers by reading the run of `#[…]` siblings before \
                    an item. Walking that run backwards with \
                    `Node::previous_sibling` re-resolved the parent from \
                    the root per step; it is now one forward pass over \
                    the parent the walker already holds. `nom/nested-fn` \
                    renders the same nesting without the attributes and \
                    with `exclude_tests` off, so it controls for both \
                    the nesting and the flag at once.",
    },
    Probe {
        name: "nom/wide-attributed-fn",
        lang: LANG::Rust,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: true,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: wide_attributed_fns,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1100 on the other axis, and the first probe on it. \
                    The fix the issue originally proposed — read the \
                    attribute run forward from the parent, always — is \
                    `O(children)` per item, so on a flat file it is \
                    quadratic in the file's item count: #1100 measured \
                    a generated 2 000-item file going from 6.0 ms to \
                    569 ms while every depth probe stayed green. \
                    Reintroducing that scan takes this probe to 1.99. \
                    The scan now budgets the parent's child count \
                    against the node's depth, and this probe is what \
                    keeps the shallow-wide half of that trade measured.",
    },
    Probe {
        name: "nom/deep-attribute-run",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: true,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_fns_with_attribute_run,
        sizes: ATTRIBUTE_RUN_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1446: the diagonal neither `nom/nested-attributed-fn` \
                    (one attribute per level) nor `nom/wide-attributed-fn` \
                    (thousands of one-attribute items at depth 1) can \
                    reach — depth and one attribute *run* growing \
                    together. #1431 made the prune answer for every \
                    `#[…]` row and not only the item, and each answer \
                    re-derived the whole run, so the run was read `run` \
                    times: 5.18 s at depth 2 000 against 0.05 s with \
                    `exclude_tests` off, fitting ~2.0. The run's verdict \
                    is now taken once and reused across its members \
                    (`SubtreeSkip`), which is what this probe holds in \
                    place. Note the sizing: the run is `3 * depth` \
                    because that is exactly `forward_attribute_scan_ \
                    budget`, so the shape is guaranteed to take the \
                    forward `O(children)` reading rather than sampling \
                    whichever branch a round number happened to select.",
    },
    Probe {
        name: "loc/wide-cfg-test-mod",
        lang: LANG::Rust,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: true,
            selection: &[Metric::Loc],
            reading: |m| m.loc.sloc(),
        },
        render: wide_cfg_test_mods,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1417: the only probe on which the number of *pruned* \
                    nodes grows with the size parameter. The two \
                    `nom/*-attributed-fn` probes deliberately use \
                    `#[inline]`, because a test attribute prunes the \
                    outermost item and stops the walk — so before this \
                    one, `exclude_tests` was exercised only through its \
                    attribute scan and never through the row bookkeeping \
                    the prune arm itself does. #1417 replaced that \
                    bookkeeping's running count with a `LineSet`, adding \
                    an `insert_range` per pruned node and a \
                    `retain_range` plus three `subtract`s per space. All \
                    are word-wise, but `LineSet::reserve` grows downward \
                    with an `O(len)` splice, so a caller that fed rows \
                    descending would be quadratic in words. This is what \
                    prices that.",
    },
    Probe {
        name: "loc/go-comment-run",
        lang: LANG::Go,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: go_comment_run,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1597: a Go lead comment run is documentation by \
                    position, decided once at the run's last member, which \
                    walks back over the run — `O(k)` per run of `k` rows. \
                    Deciding at every member instead (a forward walk per \
                    comment) is `O(k²)`, and a run is exactly the shape \
                    where that shows: this renders one run of `width` rows. \
                    The reading is the run's own row count, which the fix \
                    does not touch, so the probe keeps measuring after it.",
    },
    Probe {
        name: "loc/rust-nested-doc-mods",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: rust_nested_doc_mods,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1597: whether a doc comment sits in a function body is \
                    a question about every ancestor, and climbing them per \
                    comment was `O(depth)` each — quadratic once comments \
                    and depth grow together, as here. `Ancestors::fold_down` \
                    memoises the answer per ancestor for the walk. The \
                    reading is the doc rows, one per level, which the fold \
                    does not change.",
    },
    Probe {
        name: "loc/rust-inner-doc-run",
        lang: LANG::Rust,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: rust_inner_doc_run,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1597: each //! of an opening run is decided by the body \
                    block that holds it, with no scan of the function's \
                    prefix; a scan per comment is quadratic in the run. The \
                    reading is one doc row per member.",
    },
    Probe {
        name: "loc/java-dangling-docs",
        lang: LANG::Java,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: java_dangling_docs,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1613: which doc comments of a stretch javac attaches is \
                    decided once, at the stretch's last comment, which walks \
                    back over it; a forward scan from every comment to the \
                    declaration is quadratic in the stretch. Only one comment \
                    is a doc row, so the reading is the comment rows.",
    },
    Probe {
        name: "loc/java-package-docs",
        lang: LANG::Java,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: java_package_docs,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1613: a package Javadoc documents only with no type \
                    after it; a scan from each package to the end of the \
                    file is quadratic in the packages, so it stops at the \
                    next one. Every package but the last has a package \
                    after it, not a type, so all but one comment are doc \
                    rows; the reading counts the comment rows, which do \
                    not depend on the scan.",
    },
    Probe {
        name: "loc/rust-attributed-docs",
        lang: LANG::Rust,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: rust_attributed_docs,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1613: a doc comment followed by an attribute reads past \
                    it to the item; the stretch's members alternate with \
                    attributes, so each comment that asks for its target \
                    must stop at the next comment rather than scan to the \
                    item. Every doc comment is a doc row.",
    },
    Probe {
        name: "loc/csharp-dangling-docs",
        lang: LANG::Csharp,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: csharp_dangling_docs,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1613: Roslyn's reverse trivia scan is run once per \
                    stretch, at its last comment, over comments alternating \
                    with plain ones; only one is a doc row, so the reading \
                    is the comment rows.",
    },
    Probe {
        name: "loc/php-file-docblock",
        lang: LANG::Php,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: php_file_docblock,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1613: the file-DocBlock test scans forward from the \
                    file's first DocBlock only; every later one stops at the \
                    DocBlock before it, or the stretch is quadratic. One \
                    DocBlock is a doc row, so the reading is the comment \
                    rows.",
    },
    Probe {
        name: "loc/js-standalone-tags",
        lang: LANG::Javascript,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: js_standalone_tags,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1613: each JSDoc virtual comment is classified by its \
                    own tags, not by a scan to the declaration; every \
                    `@file` block is a doc row.",
    },
    Probe {
        name: "loc/c-dangling-docs",
        lang: LANG::C,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: c_dangling_docs,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1613: Doxygen's stretch reads past every `#include`, \
                    so it spans the whole file; it is decided once, at its \
                    last comment. None documents anything, so the reading \
                    is the comment rows.",
    },
    Probe {
        name: "loc/cpp-structural-docs",
        lang: LANG::Cpp,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: cpp_structural_docs,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1613: a stretch of `@file` blocks ending at no target \
                    is credited once, from its last gathering comment; \
                    every block is a doc row.",
    },
    Probe {
        name: "loc/c-nested-conditional-docs",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: c_nested_conditional_docs,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1613: a doc block above a preprocessor conditional \
                    documents it when any branch, at any depth, holds an \
                    entry. Asked afresh at every level that is `O(depth)` \
                    each; the walk's `FoldCache` memoises the answer per \
                    conditional. Every block binds, so the reading is the \
                    doc rows, one per level.",
    },
    Probe {
        name: "loc/python-additional-docstrings",
        lang: LANG::Python,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: python_additional_docstrings,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Additional docstrings inherit the first string's position. \
                    Decide the run once, rather than scanning back to its \
                    assignment at every string. Every string is a doc row.",
    },
    Probe {
        name: "loc/csharp-accessor-docs",
        lang: LANG::Csharp,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: csharp_accessor_docs,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1612, #1613: each comment in the accessor list asks \
                    where its stretch ends, one sibling step to the \
                    accessor, which is no doc target; the in-body test \
                    #1612 made path-decided is never reached, since a C# \
                    doc comment documents only by binding forward. A \
                    stretch scan that read on past the accessor would be \
                    quadratic here. Roslyn attaches none of them, so the \
                    reading is the comment rows.",
    },
    Probe {
        name: "loc/php-signature-docs",
        lang: LANG::Php,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: php_signature_docs,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1612: whether the body holds each docblock is decided \
                    by the node on its path; the `body` field lookup walks \
                    past every docblock in the signature. Every docblock is \
                    a doc row.",
    },
    Probe {
        name: "loc/kotlin-signature-docs",
        lang: LANG::Kotlin,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: kotlin_signature_docs,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1612: whether the body holds each KDoc is decided by \
                    the node on its path; a search of the function's \
                    children for its body walks past every KDoc in the \
                    signature. Every KDoc is a doc row.",
    },
    Probe {
        name: "halstead/python-nested-paren",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead, Metric::Loc],
            reading: |m| m.halstead.length(),
        },
        render: python_nested_parens,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1610: Halstead's `(` and LOC's `(`, `)` and \
                    parenthesized-expression arms each ask whether the \
                    parentheses wrap a bare string. These wrap none and \
                    nest past CPython's 200-level cap, which bounds an \
                    unmemoised climb to the statement; without the cap \
                    the walk is quadratic in depth. The quadratic under \
                    the cap is `loc/python-paren-bare-strings`'.",
    },
    Probe {
        name: "loc/python-paren-bare-strings",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc, Metric::Halstead],
            reading: |m| m.loc.cloc(),
        },
        render: python_paren_bare_strings,
        sizes: PAREN_CAP_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1610: every parenthesis of a bare string statement \
                    asks whether it wraps the string, which climbed to \
                    the statement and unwrapped its value again per \
                    parenthesis: O(depth²) per statement, under the cap \
                    that bounds `halstead/python-nested-paren`. The \
                    walk's FoldCache now memoises the climb, so each \
                    statement is unwrapped once. Each statement is one \
                    comment row.",
    },
    Probe {
        name: "ops/python-paren-bare-strings",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Ops {
            // The bare strings and their parentheses contribute nothing
            // to `Ops`, so the trailing assignment's `=` is the whole
            // vocabulary: constant, and here only to prove the walk ran.
            reading: |ops| ops.operators.len() as u64,
        },
        render: |depth| format!("{}x = 1\n", python_paren_bare_strings(depth)),
        sizes: PAREN_CAP_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1610: `loc/python-paren-bare-strings` on `ops_inner`, \
                    which lends the walk its own FoldCache. Without it \
                    each parenthesis's Halstead `(` test climbs to the \
                    statement and unwraps its value again, quadratic in \
                    depth under the cap, and the metric probe cannot see \
                    a walk it does not run.",
    },
    Probe {
        name: "loc/go-nested-doc-fields",
        lang: LANG::Go,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: go_nested_doc_fields,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1597: a Go field's lead comment documents only outside \
                    a function body, and finding the nearest statement list \
                    climbed every ancestor per field. Nested struct types put \
                    one candidate at every level; the fold prices it.",
    },
    Probe {
        name: "loc/ruby-nested-doc-constants",
        lang: LANG::Ruby,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: ruby_nested_doc_constants,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1597: YARD's scope rule climbs to the nearest namespace \
                    or method, through every enclosing conditional, for each \
                    documented statement. Nested `if`s holding a constant put \
                    one candidate at every level; the fold prices it.",
    },
    Probe {
        name: "loc/lua-tagless-blocks",
        lang: LANG::Lua,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: lua_tagless_blocks,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1597: a tagless LDoc block with no item is documentation \
                    only as the file's first, which it learns by scanning \
                    back to the previous qualifying block. Each block here \
                    sits directly under a call; a scan that took such a \
                    block for a run continuation ran on to the top of the \
                    file, once per block. The reading is the documented \
                    functions' rows, which the scan does not decide.",
    },
    Probe {
        name: "loc/perl-data-pod",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: perl_data_pod,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1608: the rows after `__END__` are one grammar leaf, \
                    classified by one pass over its bytes; each POD block's \
                    `comment` test reads that block's own text alone. A \
                    rescan per block or per row is quadratic in the \
                    section. Every documenting block is a doc row.",
    },
    Probe {
        name: "nom/nested-cfg-predicate",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: true,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_cfg_predicate,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1105: `cfg(all(all(… test …)))` is classified by a \
                    string-level mini-parser, which re-scanned each \
                    region's whole interior to find its split points — \
                    O(len^2) in the attribute body, and a denial-of-\
                    service vector on machine-generated Rust. The commas \
                    are now indexed by paren depth in one pass. The \
                    only probe that grows one *attribute* rather than \
                    the code around it, so `nom/nested-fn` is not a \
                    control for it; the bound alone is the guard.",
    },
    Probe {
        name: "abc/wide-let-declarators",
        lang: LANG::Javascript,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.assignments_sum(),
        },
        render: wide_let_declarators,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1277's structural `const` predicate. Its first form \
                    scanned every sibling declarator for the `const` \
                    keyword once per `=`, so a `let` list of N \
                    declarators cost O(N²) — 6 s for 5 000 on a debug \
                    build, against 0.04 s for the same list under `var` \
                    — while every depth probe stayed green. The keyword \
                    is now read through the declaration's `kind` field. \
                    The reading is the `let` initializer count, which \
                    grows with the width, so a walk that stopped \
                    counting them is visible in the value column too.",
    },
    Probe {
        name: "halstead/perl-dash-keys",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.total_operands(),
        },
        render: perl_dash_keys,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1545's dash-key workaround asks of every `-foo` \
                    whether a `=>` follows it. Finding that `=>` by \
                    scanning the argument list from its start made the \
                    walk quadratic in the key count: 1.85 s at 2 000 \
                    keys and 7.45 s at 4 000 in a release build, against \
                    0.02 s and 0.03 s once the sibling is found by index. \
                    ABC asks through the same helper. The reading is the \
                    operand count, which grows with the width.",
    },
    Probe {
        name: "abc/perl-bareword-keys",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc, Metric::Halstead],
            reading: |m| m.halstead.total_operands(),
        },
        render: perl_bareword_keys,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1572's bareword-key workaround asks of every bareword \
                    call (ABC) and of every `-` before one (Halstead) \
                    whether a `=>` follows it, the question \
                    `halstead/perl-dash-keys` pins for file-test keys. \
                    Finding that `=>` by scanning the list from its start \
                    would be quadratic in the key count, as #1545's first \
                    form was. The reading is the operand count, which \
                    grows with the width; the branch count would not, \
                    since the fix leaves only `g` scoring one.",
    },
    Probe {
        name: "abc/perl-pattern-list",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: perl_pattern_list,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Perl ABC reads a pattern's bound or delimiter role on \
                    the outermost `( … )` holding it as its value. \
                    Deciding that from the list's last operand scanned \
                    the list once per pattern: 1.9 s at 4 000 patterns \
                    and 7.7 s at 8 000 in a release build, against \
                    0.015 s and 0.032 s once it looks forward from the \
                    pattern. The reading is one condition per bare \
                    match, so it grows with the width.",
    },
    Probe {
        name: "abc/perl-or-chain",
        lang: LANG::Perl,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: perl_or_chain,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1273's misparse check climbs a chain's spine of \
                    leading slots to ask whether a comparison absorbed \
                    its first operand. A node whose leading slot scores \
                    itself answers at once, so only the bottom of a \
                    left-nested `||` spine climbs. Removing that early \
                    answer, as before the fix, takes this probe from 1.00 \
                    to 2.00 (8.8 ms to 2.33 s at depth 4 000) and \
                    `abc/perl-misparsed-comparison` to 2.01, while every \
                    other probe holds. The reading is one condition per \
                    operand.",
    },
    Probe {
        name: "abc/perl-misparsed-comparison",
        lang: LANG::Perl,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: perl_misparsed_comparison,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "The same climb as `abc/perl-or-chain`, on a chain \
                    the #1273 misparse really absorbed, so the bottom \
                    node's climb ends on the `>` and answers yes. The two \
                    probes fail together when the climb runs from every \
                    node. The reading is one condition per operand.",
    },
    Probe {
        name: "abc/perl-and-statements",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: perl_and_statements,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "The misparse check's climb from a two-operand \
                    chain stops one step up, at the block, and asks for \
                    the operator just before the chain. Scanning the \
                    block from its start for it, as before the fix, takes \
                    this probe from 1.08 to 2.01 (3.7 ms to 306 ms at \
                    2 000 statements). The kind gate that now skips a \
                    block is not what this probe sees: with the gate alone \
                    removed the lookup seeks the child by byte through \
                    the block's balanced repeat and the probe holds at \
                    1.19. The reading is two conditions per statement.",
    },
    Probe {
        name: "abc/perl-error-tail",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: perl_error_tail,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "In a file whose tail fails to parse, as in \
                    `perl5db.pl`, every `=~`, bareword and `&&` chain of \
                    the tail is a child of one flat `ERROR`. \
                    `perl_binding_rewrites` reads each `=~`'s parent's \
                    last operand, the bareword-key test the token after \
                    each `last`, and the misparse check the token before \
                    each chain. Reading those off the `ERROR` from its \
                    start made this probe fit 2.09 (#1540, #1584). A \
                    shape that does not parse (`RECOVERY_PROBES`). The \
                    reading is four conditions per copy of the body.",
    },
    Probe {
        name: "halstead/perl-error-tail-keys",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.total_operands(),
        },
        render: perl_error_tail_keys,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "`abc/perl-error-tail`'s flat `ERROR`, holding the \
                    `-and`, `not` and `-a` keys Halstead bills: the key \
                    test asks for the token after each, and the sign \
                    test for the token beside it. Read off the `ERROR` \
                    from its start, as before #1584, both were \
                    quadratic in the tail. The reading is three operands \
                    per copy of the body.",
    },
    Probe {
        name: "nexits/perl-error-tail-keys",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nexits],
            reading: |m| m.nexits.nexits_sum(),
        },
        render: perl_error_tail_exit_keys,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "`abc/perl-error-tail`'s flat `ERROR`, holding the \
                    `die` and `exit` keys nexits must not score: the key \
                    test asks for the token after each (#1585), which \
                    the walk's `SiblingCache` answers; read off the \
                    `ERROR` from its start it would be quadratic in the \
                    tail. The reading is one `exit` per copy of the body.",
    },
    Probe {
        name: "count/perl-error-tail-calls",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Count { filter: "call" },
        render: perl_error_tail_calls,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "`abc/perl-error-tail`'s flat `ERROR` under the walk \
                    `bca count` and `bca find` share. The `call` filter \
                    asks the bareword-key test of every bareword (#1580), \
                    which reads the token after it; that walk lends no \
                    `SiblingCache` unless `act_on_node` records one, and \
                    a seek from the `ERROR`'s start is quadratic in the \
                    tail. The reading is one call, `last`, per copy of \
                    the body.",
    },
    Probe {
        name: "halstead/kotlin-short-interpolations",
        lang: LANG::Kotlin,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.total_operands(),
        },
        render: kotlin_short_interpolations,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Kotlin Halstead asks of every `$name` whether a `$` \
                    token precedes it. `Ancestors::previous_sibling` \
                    answered by scanning the string's children from its \
                    start, quadratic in the name count on valid input, \
                    until it read the children the walk recorded \
                    (#1584). The reading is one operand per name plus \
                    `s`.",
    },
    Probe {
        name: "halstead/kotlin-noinline-params",
        lang: LANG::Kotlin,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.total_operators(),
        },
        render: kotlin_noinline_params,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1558's recovery guard asks of every parameter \
                    modifier whether the next token is an inserted name, \
                    climbing to the parameter list. Scanning that list \
                    from its start made the walk quadratic in the \
                    parameter count: 0.73 s at 4 000 parameters and \
                    2.81 s at 8 000 in a release build, against 0.03 s \
                    and 0.05 s once the sibling is found by index. The \
                    reading is the operator count, which grows with the \
                    width.",
    },
    Probe {
        name: "halstead/wide-distinct-fn",
        lang: LANG::Rust,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Halstead],
            reading: |m| m.halstead.unique_operands(),
        },
        render: wide_distinct_fns,
        sizes: SPACE_MERGE_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1106: popping a child space merged its Halstead maps \
                    into the parent's and then re-derived the parent's \
                    `Stats` from them — three map traversals, one of them \
                    over the parent's whole accumulated operand \
                    vocabulary, once per child, for a result the parent's \
                    own finalize overwrites. That is \
                    `O(children x vocabulary)`, quadratic in a file's \
                    function count. Restoring the per-child pass takes \
                    this probe from 1.09 to 2.17 — the gate's only \
                    failure — and its 16 000-sibling cell from 45.6 ms to \
                    507.4 ms. A depth probe cannot see it whatever it \
                    nests: the cost is per popped child, and a nesting \
                    shape has one child per space at every depth.",
    },
    Probe {
        name: "cyclomatic/tcl-wide-switch",
        lang: LANG::Tcl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cyclomatic],
            reading: |m| m.cyclomatic.cyclomatic_sum(),
        },
        render: tcl_wide_switch,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Every arm of a brace-list `switch` asks the `CaseArm` \
                    row whether its parent list is the switch's sole braced \
                    word, and the option scan that answers is memoised per \
                    `word_list` through the walk's `FoldCache`. Without the \
                    memo each of `A` arms rescans `W` options, quadratic \
                    when the two grow together (#1695). The reading is one \
                    decision per arm plus the file's base.",
    },
    Probe {
        name: "cyclomatic/cpp-plus-chain",
        lang: LANG::Cpp,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cyclomatic],
            reading: |m| m.cyclomatic.cyclomatic_sum(),
        },
        render: cpp_plus_chain,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for the three `*/cpp-and-chain` probes: \
                    the same left-nested chain through `+`, which no \
                    short-circuit arm matches. The reading is the \
                    function's base decision and the file's, 2 at every \
                    depth; the scaling signal is the timing.",
    },
    Probe {
        name: "cyclomatic/cpp-and-chain",
        lang: LANG::Cpp,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cyclomatic],
            reading: |m| m.cyclomatic.cyclomatic_sum(),
        },
        render: cpp_and_chain,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1533: every C++ `&&` / `||` asks \
                    `cpp_family_operator_is_applied` whether it applies the \
                    operator. Asking whether the chain sits in a \
                    `requires` clause by climbing to it costs the \
                    chain's depth per operator, so the first \
                    requires-clause exclusion, added in #1526's review, \
                    made a long chain quadratic in all three metrics \
                    that ask. \
                    Reinstating that climb takes this probe from 1.00 to \
                    1.97 (0.74 ms to 104.6 ms at depth 4 000) and its two \
                    `*/cpp-and-chain` siblings to 1.97 and 1.95, while \
                    `cyclomatic/cpp-plus-chain`, the same chain without \
                    the arm, holds at 0.92.",
    },
    Probe {
        name: "cognitive/cpp-and-chain",
        lang: LANG::Cpp,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: cpp_and_chain,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1533: cognitive's boolean-sequence arm runs on every \
                    `binary_expression` of the chain, so a per-node climb \
                    to an enclosing `requires` clause is quadratic here \
                    as it is for the operator tokens. The reading is the \
                    one sequence the chain starts, 1 at every depth.",
    },
    Probe {
        name: "abc/cpp-and-chain",
        lang: LANG::Cpp,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: cpp_and_chain,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1533: ABC's `&&` / `||` arm shares \
                    `cpp_family_operator_is_applied` with cyclomatic, so it \
                    inherits any climb added there. The reading is the \
                    chain's operand count.",
    },
    Probe {
        name: "cognitive/bash-test-connective-run",
        lang: LANG::Bash,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: bash_test_connective_run,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1536: each `binary_expression` of a mis-nested \
                    `[ … -o … ]` run asks where its boolean sequence ends, \
                    which is the end of the outermost comparison of the \
                    run. Climbing the run from every node for it, as \
                    before the fix, takes this probe from 0.98 to 2.06 \
                    (6.5 ms to 1.99 s at depth 4 000); the run is now \
                    tracked top-down. The reading is one sequence per \
                    switch between `-o` and `-a`.",
    },
    Probe {
        name: "abc/bash-arith-assign-chain",
        lang: LANG::Bash,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc, Metric::Halstead],
            reading: |m| m.halstead.total_operators(),
        },
        render: bash_arith_assign_chain,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1536: Bash ABC once climbed from every `=` to see \
                    whether a test holds it, which on a valid chained \
                    arithmetic assignment is the whole spine per `=`. \
                    Restoring that climb takes this probe from 1.00 to \
                    1.97 (0.79 ms to 72.5 ms at depth 4 000) and \
                    `abc/bash-eq-and-test` to 1.91. ABC counts no \
                    assignment for an arithmetic `=`, so the reading is \
                    the Halstead operator count, which grows with the \
                    depth.",
    },
    Probe {
        name: "abc/bash-eq-and-test",
        lang: LANG::Bash,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: bash_eq_and_test,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1536: the `[[ … = … && … ]]` misparse, where every \
                    `=` is a comparison ABC counts. Each test now counts \
                    its own from the top; the per-`=` climb it replaced \
                    takes this probe from 1.02 to 1.91 (2.9 ms to 75.2 ms \
                    at depth 4 000). The reading is one condition per \
                    `=` plus the `if`.",
    },
];

#[cfg(test)]
mod tests {
    use big_code_analysis::{Ast, AstCfg, AstNode, Source};

    use super::{Axis, PROBES, Probe, Workload, exported_nodes};

    /// How deep an export nests, counted on an explicit stack like
    /// [`exported_nodes`].
    fn exported_depth(root: Option<&AstNode>) -> usize {
        let mut pending: Vec<(&AstNode, usize)> = root.into_iter().map(|r| (r, 1)).collect();
        let mut deepest = 0;
        while let Some((node, depth)) = pending.pop() {
            deepest = deepest.max(depth);
            pending.extend(node.children.iter().map(|c| (c, depth + 1)));
        }
        deepest
    }

    /// Probes whose shape deliberately fails to parse; see
    /// [`shapes_parse_without_error`].
    const RECOVERY_PROBES: &[&str] = &[
        "abc/perl-error-tail",
        "halstead/perl-error-tail-keys",
        "nexits/perl-error-tail-keys",
        "count/perl-error-tail-calls",
        "dump/perl-error-tail-calls",
    ];

    /// Probes whose shape recovers *inside* a command rather than at
    /// the root: the `ERROR` is a slot of the widening `word_list`, which
    /// is what makes the list unreadable and the scan run at all. See
    /// [`shapes_parse_without_error`] and
    /// [`erroring_eval_probes_err_in_the_argument_list`].
    const ARGUMENT_SLOT_ERROR_PROBES: &[&str] = &[
        "dump/tcl-wide-erroring-eval",
        "halstead/tcl-wide-erroring-eval",
        "ops/tcl-wide-erroring-eval",
        "count/tcl-wide-erroring-eval",
    ];

    /// Deepest `tree_sitter` node depth reachable from the root.
    ///
    /// Iterative on an explicit stack: the shapes here nest thousands
    /// of levels deep, which is exactly the input a recursive helper
    /// would overflow on.
    fn ast_depth(ast: &Ast) -> usize {
        let mut stack = vec![(ast.as_tree_sitter().root_node(), 1_usize)];
        let mut deepest = 0;
        while let Some((node, depth)) = stack.pop() {
            deepest = deepest.max(depth);
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor).map(|child| (child, depth + 1)));
        }
        deepest
    }

    /// Most children any one node in the tree has — the quantity a
    /// width shape grows, and the one the `exclude_tests` attribute
    /// scan is priced against.
    fn max_child_count(ast: &Ast) -> usize {
        let mut stack = vec![ast.as_tree_sitter().root_node()];
        let mut widest = 0;
        while let Some(node) = stack.pop() {
            widest = widest.max(node.child_count());
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
        widest
    }

    /// The probes on `axis`, asserted non-empty.
    ///
    /// Each shape invariant below applies to one axis, and a loop over
    /// an empty filtered set passes every assertion inside it. Without
    /// this, deleting the last probe of an axis would silently retire
    /// that axis's invariant rather than fail — which for the width
    /// axis is exactly the state #1133 was filed about.
    fn probes_on(axis: Axis) -> Vec<&'static Probe> {
        let probes: Vec<&Probe> = PROBES.iter().filter(|probe| probe.axis == axis).collect();
        assert!(!probes.is_empty(), "PROBES must cover the {axis:?} axis");
        probes
    }

    fn parse(probe: &Probe, size: usize) -> Ast {
        let source = (probe.render)(size);
        Ast::parse(Source::new(probe.lang, source.as_bytes()))
            .unwrap_or_else(|e| panic!("{} at size {size} must parse: {e}", probe.name))
    }

    /// Every generator emits a constant number of bytes per unit of
    /// its size parameter.
    ///
    /// This is the trap that invalidated two measurements during the
    /// #1052 / #1062 work: an indented generator makes the input grow
    /// quadratically, so a linear walk reads as superlinear. A
    /// constant second difference is exactly "affine in depth".
    #[test]
    fn byte_growth_is_affine() {
        for probe in PROBES {
            let len = |d: usize| (probe.render)(d).len();
            let (a, b, c) = (len(10), len(20), len(30));
            assert_eq!(
                b - a,
                c - b,
                "{}: bytes must grow linearly with size, got {a} -> {b} -> {c}",
                probe.name,
            );
            assert!(
                b > a,
                "{}: size must actually add bytes, got {a} -> {b}",
                probe.name,
            );
        }
    }

    /// Every shape parses cleanly at the smallest size the gate
    /// measures it at.
    ///
    /// Without this, a grammar bump that stops accepting one of these
    /// snippets would leave the probe measuring `tree_sitter`'s error
    /// recovery while still reporting a plausible exponent. The size
    /// comes from the probe rather than from a literal because a
    /// parser limit — a recursion cap, a token-count ceiling — is
    /// reached at scale and not on a toy input, so a small fixed size
    /// answers a different question than the gate asks.
    ///
    /// The probes in [`RECOVERY_PROBES`] are the inverse: their shape
    /// *is* error recovery, and the cost they price is reading the
    /// `ERROR` root's children. A grammar that learned to parse one
    /// would leave it timing an ordinary narrow tree, so each must still
    /// recover into an `ERROR` root.
    #[test]
    fn shapes_parse_without_error() {
        for probe in PROBES {
            let size = probe.sizes[0];
            let ast = parse(probe, size);
            let root = ast.as_tree_sitter().root_node();
            let (parses_as_declared, declared) = if RECOVERY_PROBES.contains(&probe.name) {
                (root.is_error(), "into an ERROR root")
            } else if ARGUMENT_SLOT_ERROR_PROBES.contains(&probe.name) {
                (
                    root.has_error() && !root.is_error(),
                    "with an ERROR below a well-formed root",
                )
            } else {
                (!root.has_error(), "without an ERROR node")
            };
            let name = probe.name;
            let head: String = (probe.render)(size).chars().take(200).collect();
            assert!(
                parses_as_declared,
                "{name}: shape must parse {declared} at size {size}:\n{head}…"
            );
        }
        for (list, names) in [
            ("RECOVERY_PROBES", RECOVERY_PROBES),
            ("ARGUMENT_SLOT_ERROR_PROBES", ARGUMENT_SLOT_ERROR_PROBES),
        ] {
            for name in names {
                assert!(
                    PROBES.iter().any(|probe| probe.name == *name),
                    "{list} names {name}, which is not in PROBES",
                );
            }
        }
    }

    /// The `tcl-wide-erroring-eval` shape errs where its probes need it
    /// to: the `ERROR` is a direct slot of the `eval` command's argument
    /// list, which is the widening node, with braced words on both sides
    /// of it. At each rung of the ladder, since recovery is the workload
    /// here and not merely permitted.
    ///
    /// [`shapes_parse_without_error`] only establishes that *some* nested
    /// error exists; a grammar that moved the `]` into a command of its
    /// own, or that absorbed it into a neighbouring word, would leave the
    /// list readable and the scan never run, while the `"x"` words kept
    /// every reading non-zero.
    #[test]
    fn erroring_eval_probes_err_in_the_argument_list() {
        let mut checked = 0;
        for probe in PROBES
            .iter()
            .filter(|probe| ARGUMENT_SLOT_ERROR_PROBES.contains(&probe.name))
        {
            for size in probe.sizes {
                let ast = parse(probe, size);
                let root = ast.as_tree_sitter().root_node();
                let mut cursor = root.walk();
                let eval = root
                    .children(&mut cursor)
                    .find(|node| node.kind() == "command")
                    .unwrap_or_else(|| panic!("{}: the line is one command", probe.name));
                let list = eval
                    .child_by_field_name("arguments")
                    .unwrap_or_else(|| panic!("{}: the command has arguments", probe.name));
                assert_eq!(list.kind(), "word_list", "{}", probe.name);
                assert_eq!(
                    list.child_count(),
                    max_child_count(&ast),
                    "{}: the argument list must be the widening node",
                    probe.name
                );
                let mut cursor = list.walk();
                let kinds: Vec<&str> = list.children(&mut cursor).map(|c| c.kind()).collect();
                let errors: Vec<usize> = kinds
                    .iter()
                    .enumerate()
                    .filter_map(|(i, kind)| (*kind == "ERROR").then_some(i))
                    .collect();
                let [error] = errors[..] else {
                    panic!(
                        "{}: exactly one ERROR slot at size {size}, found {errors:?}",
                        probe.name
                    );
                };
                let braced = |slice: &[&str]| slice.iter().filter(|k| **k == "braced_word").count();
                assert_eq!(
                    (braced(&kinds[..error]), braced(&kinds[error + 1..])),
                    (size / 2, size / 2),
                    "{}: {size} braced words, half on each side of the ERROR slot",
                    probe.name
                );
                checked += 1;
            }
        }
        assert_eq!(checked, ARGUMENT_SLOT_ERROR_PROBES.len() * 3);
    }

    /// Every depth shape actually nests: doubling the size adds at
    /// least that many more levels of AST.
    ///
    /// A shape that flattened — because a grammar started folding the
    /// repetition into a list node, say — would still parse, still
    /// grow linearly in bytes, and still produce a tidy exponent near
    /// 1.0 while measuring nothing the probe claims to measure.
    #[test]
    fn shapes_nest_proportionally_to_depth() {
        for probe in probes_on(Axis::Depth) {
            let shallow = ast_depth(&parse(probe, 16));
            let deep = ast_depth(&parse(probe, 32));
            assert!(
                deep >= shallow + 16,
                "{probe_name}: doubling depth 16 -> 32 grew the tree \
                 {shallow} -> {deep} AST levels; the shape is not \
                 nesting",
                probe_name = probe.name,
            );
        }
    }

    /// Every width shape widens, and *only* widens.
    ///
    /// The positive half mirrors
    /// [`shapes_nest_proportionally_to_depth`]: doubling the size must
    /// add at least that many children to the widest node, so a shape
    /// that stopped growing is caught.
    ///
    /// The negative half is the one that matters. A "width" probe that
    /// quietly began nesting — a generator edited to wrap its items,
    /// or a grammar that started grouping them — would satisfy the
    /// first half while measuring the depth axis, and report a healthy
    /// exponent for a width bound nothing is testing any more. Pinning
    /// the AST depth *constant* across the two sizes is what
    /// distinguishes the two axes; nothing else here can.
    ///
    /// Sixteen and thirty-two, not the probe's own ladder as the two
    /// tests below use: both halves are structural claims about the
    /// generator, true at any pair of sizes, and 500 vs 1 000 would
    /// cost a second of parsing to assert the same thing.
    #[test]
    fn shapes_widen_without_deepening() {
        for probe in probes_on(Axis::Width) {
            let (narrow, wide) = (parse(probe, 16), parse(probe, 32));
            let (narrow_width, wide_width) = (max_child_count(&narrow), max_child_count(&wide));
            assert!(
                wide_width >= narrow_width + 16,
                "{probe_name}: doubling width 16 -> 32 grew the widest \
                 node {narrow_width} -> {wide_width} children; the \
                 shape is not widening",
                probe_name = probe.name,
            );
            assert_eq!(
                ast_depth(&narrow),
                ast_depth(&wide),
                "{}: a width shape must not gain AST levels with its \
                 size, or its exponent is measuring the depth axis",
                probe.name,
            );
        }
    }

    /// Every probe's workload produces a non-zero reading on its own
    /// shape, at the smallest size the gate measures it at.
    ///
    /// Pairing a shape with a workload that scores zero on it would
    /// benchmark the walk's fixed overhead and nothing else, and the
    /// resulting exponent would look excellent forever.
    ///
    /// The size is the probe's own rather than a fixed small one: a
    /// literal that happens to suit the nesting shapes is sixteen-odd
    /// siblings on a width shape — too small to be the shape the probe
    /// stands for, and a reading that only becomes non-zero at scale
    /// would fail here for a reason unrelated to the pairing. The
    /// whole set costs ~0.4 s in a debug build at these sizes.
    #[test]
    fn probe_workload_is_exercised() {
        for probe in PROBES {
            let size = probe.sizes[0];
            let ast = parse(probe, size);
            let reading = probe
                .workload
                .walk(&ast, probe.workload.options())
                .unwrap_or_else(|e| panic!("{}: walker must succeed: {e}", probe.name));
            assert!(
                reading > 0,
                "{}: workload scored zero on its own shape at size {size}",
                probe.name,
            );
        }
    }

    /// The #1695 probes that read a count *growing* with the shape — one
    /// decision per `switch` arm or guard, one operator per guard —
    /// so a shape that lost the construct it times is caught here,
    /// where [`probe_workload_is_exercised`] would not see it: a
    /// cyclomatic reading floors at one per space, and a Halstead one
    /// at the head's own operators. Several older probes read a
    /// constant by design (their signal is the timing, as each says),
    /// so the check is per probe rather than over `PROBES`.
    #[test]
    fn the_row_probes_readings_grow_with_the_shape() {
        for name in [
            "cyclomatic/tcl-wide-switch",
            "halstead/elixir-repeated-guards",
            "cyclomatic/elixir-repeated-guards",
        ] {
            let probe = PROBES
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{name} is a probe"));
            let read = |size: usize| {
                let ast = parse(probe, size);
                probe
                    .workload
                    .walk(&ast, probe.workload.options())
                    .unwrap_or_else(|e| panic!("{name}: walker must succeed: {e}"))
            };
            let (small, large) = (read(probe.sizes[0]), read(probe.sizes[1]));
            assert!(
                large > small,
                "{name}: the reading does not grow with the shape ({small} at {}, {large} at {})",
                probe.sizes[0],
                probe.sizes[1]
            );
        }
    }

    /// Every export probe's *exported* tree grows with its size, along
    /// its own axis, at the sizes the gate measures.
    ///
    /// The two shape invariants above read the raw `tree_sitter` tree,
    /// and [`probe_workload_is_exercised`] asks only for a non-zero
    /// reading. Neither can see an export that collapsed: an exporter
    /// that emitted one root would pass both while timing nothing, and
    /// a Tcl alterator that stopped keeping script bodies would leave
    /// the raw tree nesting while the export flattened to a leaf. So a
    /// width probe must export proportionally more nodes, and a depth
    /// probe must export proportionally deeper — on the ladder itself,
    /// since a parser limit shows at scale and not on a toy input.
    #[test]
    fn dump_probes_export_proportionally_to_their_size() {
        let mut checked = 0;
        for probe in PROBES {
            let Workload::Dump { span, comment } = probe.workload else {
                continue;
            };
            checked += 1;
            let cfg = || AstCfg {
                id: String::new(),
                language: String::new(),
                comment,
                span,
            };
            let [small, _, large] = probe.sizes;
            let export = |size| parse(probe, size).dump(cfg());
            let (narrow, wide) = (export(small), export(large));
            let measure = match probe.axis {
                Axis::Width => exported_nodes,
                Axis::Depth => exported_depth,
            };
            let (a, b) = (measure(narrow.root.as_ref()), measure(wide.root.as_ref()));
            assert!(
                b >= a + (large - small),
                "{}: {:?} export grew {a} -> {b} over sizes {small} -> {large}; \
                 the export is not growing along the probe's axis",
                probe.name,
                probe.axis,
            );
        }
        assert!(checked > 0, "PROBES must hold at least one Dump workload");
    }

    /// Every probe whose *shape* the exclusion hook prunes walks with
    /// `exclude_tests` on.
    ///
    /// [`probe_workload_is_exercised`] cannot see this. The two
    /// attributed-function shapes carry `#[inline]`, which no
    /// `exclude_tests` rule prunes, so `nom.total()` reads the size
    /// parameter with the flag on or off: flipping
    /// `exclude_tests: true` to `false` on `nom/wide-attributed-fn`
    /// deletes the whole `Checker::should_skip_subtree` attribute scan
    /// the probe exists to price, and fails nothing (measured, #1133).
    ///
    /// The discriminator is deliberately the shape and not the flag.
    /// Selecting the probes *by* `exclude_tests: true` and asserting
    /// something about them is vacuous against exactly that flip — the
    /// flipped probe leaves the selected set and the loop passes
    /// (measured, too). So each shape is classified by walking it both
    /// ways under forced options: one whose reading moves is one the
    /// hook prunes, and that probe must be running with the flag set.
    ///
    /// `#[inline]` is rewritten to `#[test]` first, which is what makes
    /// the attributed shapes prunable at all. Reading the source back
    /// through `probe.render` rather than restating it keeps the guard
    /// coupled to the generator: one that stopped emitting attributes
    /// stops being classified and trips the floor below.
    ///
    /// Size 16 rather than the probe's own ladder, as
    /// [`shapes_widen_without_deepening`] uses: "the hook prunes this
    /// shape" is structural and true at any size.
    #[test]
    fn shapes_the_exclusion_hook_prunes_are_probed_with_it_on() {
        let mut pruned_shapes = Vec::new();
        for probe in PROBES {
            let Workload::Metrics { exclude_tests, .. } = probe.workload else {
                continue;
            };
            let source = (probe.render)(16).replace("#[inline]", "#[test]");
            let ast = Ast::parse(Source::new(probe.lang, source.as_bytes())).unwrap_or_else(|e| {
                panic!("{}: test-attributed shape must parse: {e}", probe.name)
            });
            let walk = |options| {
                probe
                    .workload
                    .walk(&ast, options)
                    .unwrap_or_else(|e| panic!("{}: walker must succeed: {e}", probe.name))
            };
            let options = probe.workload.options();
            if walk(options.with_exclude_tests(true)) == walk(options.with_exclude_tests(false)) {
                continue;
            }
            pruned_shapes.push(probe.name);
            assert!(
                exclude_tests,
                "{}: the exclusion hook prunes this shape, so the probe \
                 exists to price that walk — but its workload sets \
                 `exclude_tests: false`, which never reaches it",
                probe.name,
            );
        }
        assert!(
            pruned_shapes.len() >= 2,
            "only {pruned_shapes:?} are pruned by the exclusion hook; \
             `Checker::should_skip_subtree` needs a probe on each axis, \
             and a shape that stopped carrying attributes drops out of \
             this check silently"
        );
    }

    /// Probe names are unique — they key the report and the gate.
    #[test]
    fn probe_names_are_unique() {
        let mut names: Vec<&str> = PROBES.iter().map(|probe| probe.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(count, names.len(), "duplicate probe name in PROBES");
    }

    /// Sizes double, which is what makes the fitted exponent readable
    /// as "cost per doubling".
    #[test]
    fn probe_sizes_double() {
        for probe in PROBES {
            let [a, b, c] = probe.sizes;
            assert_eq!(b, a * 2, "{}: sizes must double", probe.name);
            assert_eq!(c, b * 2, "{}: sizes must double", probe.name);
        }
    }
}
