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

/// C: `int main(){ if (a) { 1; } else if (a) { 1; } … else { 1; } }`.
///
/// An `else if` ladder: every `if_statement` after the first sits in
/// the previous one's `else_clause`, so the chain is `depth` levels
/// deep in the tree while reading as flat to a human. The `ElseIf`
/// row's gate asks each one for its parent and answers `true` for all
/// but the first, so the reading is `depth + 2` — a gate that stopped
/// recognising the chain would nest each `if` and read `~depth²/2`,
/// which [`nested_ifs`], whose `if`s have no `else`, cannot show; the
/// timing pins the gate's cost on the shape where it fires (#1697).
#[must_use]
pub(crate) fn else_if_ladder(depth: usize) -> String {
    format!(
        "int main(){{ if (a) {{ 1; }} {}else {{ 1; }} }}\n",
        "else if (a) { 1; } ".repeat(depth)
    )
}

/// Python: a `def` whose body is `width` sibling `if a: pass` statements.
///
/// Every `if_statement` reaches the `ElseIf` row's
/// `sole_statement_of_else_block` gate. The predicate it replaced
/// counted the whole enclosing block's named children before asking
/// whether that block was an `else:` body, so a function of `W` sibling
/// `if`s cost `O(W²)` — 20,000 took 8 s, 40,000 34 s. The gate now
/// reads the two parents off the chain first and stops its scan at the
/// second child. The reading is `width` (+1 per `if`, no nesting), so
/// a gate that started nesting siblings would read `width²/2`-ish and
/// fail the value check too (#1697).
#[must_use]
pub(crate) fn python_wide_if_siblings(width: usize) -> String {
    format!("def f(a):\n{}", "    if a: pass\n".repeat(width))
}

/// Java: a method whose body is `width` repetitions of
/// `if (a) { x(); } else if (b) { y(); } else { z(); }`.
///
/// The outer `if_statement`s are direct children of the method's
/// `block`, which is wider than the walk's `WIDE_PARENT` at every size
/// here, so the `ElseIf` row's previous-sibling gate answers each one
/// through the recorded sibling cache; each inner `if_statement` is a
/// child of its outer `if_statement` (five children) and answers
/// through the short scan. One probe therefore runs both lookup paths
/// (#1697). The reading is `3 × width` — outer `if` +1, `else if` +1,
/// `else` +1 — and a gate that stopped recognising the else-if would
/// nest the inner `if` and read `4 × width`, so the value column
/// discriminates the gate as well as the timing.
#[must_use]
pub(crate) fn wide_if_siblings(width: usize) -> String {
    format!(
        "class A {{ void f() {{ {}}} }}\n",
        "if (a) { x(); } else if (b) { y(); } else { z(); } ".repeat(width)
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
/// One `declaration` per nesting level. `loc`'s C-family arm asks each
/// declaration whether a loop or `if` header holds it, stopping at the
/// nearest `compound_statement` — the brace directly above every
/// declaration here. The probe pins that the question reads the
/// walker's ancestor chain rather than calling `Node::parent` (#1084).
/// [`cpp_nested_namespaces`] is the same question with no stop below
/// the root (#1767).
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
/// Elixir's callable test asks `elixir_is_inside_quote_block` for every
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

/// Bash: functions nested `depth` deep, each body opening with a
/// pipeline continued across a row that holds only `\`.
///
/// The bare row is the `loc` question #1445 answers: it carries no node,
/// so it is found by reading the padding between a node's children. Every
/// level is a space of its own, so a scan that read each space's whole
/// byte range would read the bytes of level *k* `depth - k` times; the
/// padding of each node is read once. Affine in depth, no indentation.
#[must_use]
pub fn bash_nested_continued_fns(depth: usize) -> String {
    format!(
        "{}{}",
        "f() {\necho a | \\\n\\\ncat\n".repeat(depth),
        "}\n".repeat(depth)
    )
}

/// Tcl: `width` comments, each continued by a row holding only `\` and
/// closed by a blank row, at the top level of one file.
///
/// Each bare row is comment text (#1445), and telling it from a code row
/// asks whether the row before it ends inside a comment node, once per
/// bare row. `descendant_for_byte_range` answers by descending the
/// root's balanced `repeat` tree; finding the comment by walking the
/// root's children instead costs the file's width per question.
#[must_use]
pub fn tcl_wide_continued_comments(width: usize) -> String {
    "# c \\\n\\\n\nputs x\n".repeat(width)
}

/// Tcl: `width` units of a `proc` whose body never closes, each holding
/// a comment continued by a row holding only `\` and closed by a blank
/// row.
///
/// The unclosed openers leave every unit a direct child of one flat root
/// `ERROR`, which tree-sitter never balances, so the lookup that tells a
/// bare row continuing a comment from a code row (#1445) costs that
/// root's width when it descends from the gap's owner. It descends from
/// the child before the gap instead, a constant per bare row.
#[must_use]
pub fn tcl_unclosed_continued_comments(width: usize) -> String {
    "proc p {} {\n# c \\\n\\\n\n".repeat(width)
}

/// Elixir: `width` units of an `fn ->` that never closes, each holding a
/// comment ending in `\`, a row holding only `\`, and a comment after it.
///
/// Elixir never continues a comment, so each bare row starts a logical
/// line and is what the row it joins becomes once spliced (#1801), read
/// off the gap's next child. The unclosed `fn`s leave every unit a direct
/// child of one flat root `ERROR`, which tree-sitter never balances, so
/// finding that comment from the gap's owner instead costs the root's
/// width per bare row.
#[must_use]
pub fn elixir_unclosed_spliced_comments(width: usize) -> String {
    "fn ->\n# c \\\n\\\n# d\n".repeat(width)
}

/// Python: one run of `width` rows holding only `\`, joining a comment
/// row, then a retained `x = 1`.
///
/// Python keeps each `\` + newline as a `line_continuation` node — all
/// `width` of them siblings under `module` — so `loc` meets the run one
/// node at a time, and the run is one logical line (#1819). Its head
/// classifies and credits the whole run; every later node answers in
/// O(1) from its own bytes. Classifying at every node reads the rest of
/// the run each time, which is quadratic in its length. The `x = 1` is
/// what keeps the reading off the construct under test.
#[must_use]
pub fn python_long_bare_run_to_comment(width: usize) -> String {
    format!("{}# note\nx = 1\n", "\\\n".repeat(width))
}

/// [`python_long_bare_run_to_comment`] joining an empty row instead,
/// the run's Blank class.
#[must_use]
pub fn python_long_bare_run_to_blank(width: usize) -> String {
    format!("{}\nx = 1\n", "\\\n".repeat(width))
}

/// Python: `width` separate one-row bare `\` runs inside a parenthesised
/// bare string's parentheses, each followed by an empty row, then the
/// `)` and a retained `x = 1`.
///
/// The two `python_long_bare_run_*` shapes above are one run; here every
/// `\` heads a run of its own, since the empty row between two ends the
/// first, and every head asks whether it is on the string's line. That
/// is a question about the `parenthesized_expression` all `width` runs
/// share, answered once per parenthesis under the walk's cache
/// (`NodeQuestion::BareStringParenthesisLine`). Finding the string from
/// each run through the siblings before it instead reads every earlier
/// run in the parenthesis, quadratic in `width` (fourth re-review of
/// #1819). The `x = 1` keeps the reading off the construct under test.
#[must_use]
pub fn python_separated_bare_runs(width: usize) -> String {
    format!("( \"\"\"doc\"\"\"\n{})\nx = 1\n", "\\\n\n".repeat(width))
}

/// Python: `width` separate one-row bare `\` runs *before* the string
/// inside a parenthesised bare string's parentheses, each joining a
/// comment row, then the string, the `)` and a retained `x = 1`.
///
/// [`python_separated_bare_runs`] with the runs ahead of the string: a
/// `\` run and a comment are named extras the parenthesis's value is
/// read past (#1830), so each run's head asks the same per-parenthesis
/// question, and that question's own descent to the string crosses the
/// whole `width`-wide prefix. The memo under
/// `NodeQuestion::BareStringParenthesisLine` answers it once; asked per
/// run, the descent is quadratic in `width`. The `x = 1` keeps the
/// reading off the construct under test.
#[must_use]
pub fn python_bare_runs_before_string(width: usize) -> String {
    format!(
        "(\n{}\"\"\"doc\"\"\"\n)\nx = 1\n",
        "\\\n# c\n".repeat(width)
    )
}

/// Python: an `if` whose block opens with `width` comment rows and then
/// holds `width` parenthesised bare strings, each with a bare `\` row
/// inside its parentheses.
///
/// Each string's parenthesis asks whether code precedes it on its
/// logical line, and the nearest level that answers is its statement's:
/// the statement before it ends on another row. The comments ahead of a
/// block's first statement are children of the `if`, before the `block`,
/// so the block's own level is a scan of `width` comments back to the
/// `:`. Only the first statement needs it. A walk that reads the levels
/// above the nearest answering one for every asker — a fold of the
/// answer down the chain evaluates each ancestor, and keys the block's
/// state by the statement under it — scans the comments once per
/// statement, quadratic in `width` (#1863).
#[must_use]
pub fn python_block_comments_paren_strings(width: usize) -> String {
    format!(
        "if x:\n{}{}",
        "    # c\n".repeat(width),
        "    (\"s\"\n\\\n)\n".repeat(width)
    )
}

/// How many `\` + `# c` pairs [`python_deep_paren_extra_prefix`] puts
/// ahead of the next parenthesis at every level: 63 children with the
/// two parentheses and the next level, the most a node holds before the
/// walk records it for sibling lookups (`WIDE_PARENT`). A level's read
/// is then a seek over the parenthesis's children rather than a slice
/// of the walk's sibling cache, which is what makes the number of
/// levels read show in the timing: with 31 pairs a climb per
/// parenthesis fits about 1.6, under no bound that would also pass the
/// memoised walk on a shared host, where these 30 fit about 2.
const PAREN_PREFIX_PAIRS: usize = 30;

/// Python: a bare string `depth` parentheses deep, each parenthesis
/// opening with `PAREN_PREFIX_PAIRS` one-row bare `\` runs, each
/// joining a comment row, before the next parenthesis; then a retained
/// `x = 1`.
///
/// [`python_bare_runs_before_string`] grows the prefix of one
/// parenthesis and [`python_paren_bare_strings`] the nesting with no
/// prefix; this grows the nesting and keeps a prefix at every level.
/// A run asks whether its parenthesis is on
/// the string's line with no code before it, and that question looks at
/// each level above the parenthesis for a token before the level's
/// node, past that level's extras. None of these levels has one, so the
/// question defers to the root from every parenthesis: read by
/// climbing, `depth` parentheses read `depth² / 2` levels, under the
/// 200 levels Python's tokenizer lets parentheses nest. Read through the
/// walk's memo of each level's nearest answer, each level is read once
/// (#1863).
#[must_use]
pub fn python_deep_paren_extra_prefix(depth: usize) -> String {
    let level = format!("(\n{}", "\\\n# c\n".repeat(PAREN_PREFIX_PAIRS));
    format!(
        "{}\"\"\"doc\"\"\"\n{}\nx = 1\n",
        level.repeat(depth),
        ")".repeat(depth)
    )
}

/// C: `width` units of a `{` that never closes, each followed by a row
/// holding only `\` and a row holding two block comments.
///
/// Each bare row joins a comment-only row, which takes reading both
/// comments (#1801). The unclosed braces leave every unit a direct child
/// of one flat root `ERROR`, so reaching the second comment with
/// `next_sibling`, which scans the parent's children from the first,
/// costs the root's width per bare row; the per-walk index reads every
/// comment in its one cursor walk instead (#1818).
#[must_use]
pub fn c_unclosed_spliced_comments(width: usize) -> String {
    "{\n\\\n/* a */ /* b */\n".repeat(width)
}

/// C: one logical line of `width` comment-only rows, each continued by
/// its `\` and each followed by a row holding only `\`, then an empty
/// row, then `int z;`.
///
/// Every bare row is a comment: the chain holds no code, and the row it
/// all joins is empty (#1818). The class of each is the whole line's,
/// settled once by the per-walk index. A scan that read the line back
/// from each bare row (the rows before it, to find the code it would
/// continue) or forward (the comments after it, to find the line's end)
/// reads `O(width)` per row and is quadratic in the chain; the pre-#1818
/// bounded lookup was linear and wrong, so this probe guards the index's
/// cost, not the bug. The `int z;` keeps the reading off the construct.
#[must_use]
pub fn c_comment_chain(width: usize) -> String {
    format!("{}\nint z;\n", "/* c */ \\\n\\\n".repeat(width))
}

/// C: `width` units, each a `{` that never closes, a row of three block
/// comments ending in `\`, a row holding only `\`, and an empty row.
///
/// [`c_comment_chain`] under the flat root `ERROR` the unclosed braces
/// leave, with each unit its own logical line: the index's comment
/// spans come from a cursor walk that visits each node once, where a
/// lookup from the gap's owner reads the root's width per bare row
/// (`loc/c-unclosed-spliced-comments`). Three comments on the row, one
/// past the window the old lookup kept.
#[must_use]
pub fn c_unclosed_comment_rows(width: usize) -> String {
    "{\n/* a */ /* b */ /* c */ \\\n\\\n\n".repeat(width)
}

/// JavaScript: `function f() { function f() { … 1; … } }`.
///
/// The linear control for [`nested_arrows`]: one function per nesting
/// level and one `FuncSpace` per level, as there, but *declared* rather
/// than written as an expression. `function_declaration` is a distinct
/// grammar production, so the callable test answers it from the node's
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
/// The run was sized `3 * depth` so its parent, `3 * depth + 4` children
/// sitting `2 * depth + 1` levels down, stayed inside the `6 * depth + 3`
/// the width-against-depth budget then allowed there, and the shape took
/// the forward `O(children)` reading for certain. Asking once per attribute
/// read the run `run` times: 0.08 / 0.34 / 1.21 / 5.18 s at depths
/// 250 / 500 / 1 000 / 2 000, against 0.05 s for the deepest with
/// `exclude_tests` off. #1447 removed the budget, and the shape stays
/// as the long-run diagonal; [`nested_fns_with_one_attribute_items`]
/// is its many-runs counterpart.
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

/// Rust: `depth` nested `fn f`, then `3 * depth` separate
/// `#[cfg(test)]\nfn t() {}` items in the innermost body.
///
/// [`nested_fns_with_attribute_run`] with the run split into one per
/// item: the same diagonal, but many short runs on one deep parent
/// instead of one long run. #1446's reach makes each run one ask, so
/// what this prices is the ask itself. Under the width-against-depth
/// budget #1100 set, the innermost body's `6 * depth + 2` children sit
/// at depth `2 * depth + 1`, where the budget was `6 * depth + 3`, so
/// each ask read the parent by a cursor pass from its first child,
/// `O(width)` per ask, `3 * depth` times: 0.20 / 0.74 / 3.00 / 11.9 s at
/// depths 250 / 500 / 1 000 / 2 000 (#1447). The ask now locates the
/// node by a search instead: here the walk's `SiblingCache`, though on
/// a `repeat()` body like this one the byte seek is logarithmic too.
///
/// The reading is `nom.total()`, the `depth` retained wrappers, for the
/// reason [`nested_fns_with_attribute_run`] gives: the pruned `fn t`
/// are the prune's own output and must not be what is measured.
#[must_use]
pub fn nested_fns_with_one_attribute_items(depth: usize) -> String {
    format!(
        "{}{}{}",
        "fn f() {\n".repeat(depth),
        "#[cfg(test)]\nfn t() {}\n".repeat(3 * depth),
        "}\n".repeat(depth),
    )
}

/// Rust: `depth` nested `fn f`, then `3 * depth` outer doc comments
/// and a plain comment each, above one `#[cfg(test)] fn t` in the
/// innermost body.
///
/// [`nested_fns_with_attribute_run`] with the run spelled as comments:
/// since #1647 an outer doc comment opens a run like an attribute, and
/// a plain comment inside one is a member of it. Each `///` row is
/// therefore a place the prune hook reads the run from, forward to the
/// item and back, and every row of it decorates the same `fn t`; the
/// verdict the first row reaches is what answers for the rest. Without
/// that reach each doc row re-reads the run, `O(run)` work `run`
/// times, which is #1446's diagonal over again on comment rows. The
/// plain rows between are what the reach answers for in `O(1)` — the
/// hook keeps them out of the run reading on purpose, so a stretch of
/// `//` rows is never quadratic in itself.
///
/// The reading is `nom.total()`, the `depth` retained wrappers, for the
/// reason [`nested_fns_with_attribute_run`] gives.
#[must_use]
pub fn nested_fns_with_doc_comment_run(depth: usize) -> String {
    format!(
        "{}{}#[cfg(test)]\nfn t() {{}}\nlet x = 1;\n{}",
        "fn f() {\n".repeat(depth),
        "/// d\n// p\n".repeat(3 * depth),
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

/// C: one documented, shallow conditional per struct level, each at a
/// greater depth than the last.
#[must_use]
pub fn c_conditional_per_struct_depth(depth: usize) -> String {
    format!(
        "{}{}",
        "struct S {\n/** d */\n#ifdef A\nint x;\n#endif\n".repeat(depth),
        "};\n".repeat(depth)
    )
}

/// C++: `namespace N { int x; namespace N { int x; … } }`.
///
/// One `declaration` per level, and no `compound_statement` anywhere
/// above any of them: a namespace body is a `declaration_list`. The
/// C-family `loc` arm asks every declaration whether a loop or `if`
/// header holds it, nearest `compound_statement` first, so on this
/// shape the question has no stop below the root (#1767).
#[must_use]
pub fn cpp_nested_namespaces(depth: usize) -> String {
    format!(
        "{}{}",
        "namespace N {\nint x;\n".repeat(depth),
        "}\n".repeat(depth)
    )
}

/// TypeScript: `function f() { let x = 1; function f() { … } }`.
///
/// One `lexical_declaration` per level. The TypeScript `loc` arm asks
/// every declaration whether an `ambient_declaration` holds it, with no
/// stop at all, so each question reaches the root; the JavaScript arm
/// asks only the export question, which a `statement_block` stops one
/// step up (#1767).
#[must_use]
pub fn ts_nested_functions(depth: usize) -> String {
    format!(
        "{}{}",
        "function f() {\nlet x = 1;\n".repeat(depth),
        "}\n".repeat(depth)
    )
}

/// JavaScript: `x = [() => 1, [() => 1, [ … 0 ] ] ];`.
///
/// One arrow per level, each an element of an array nested in the last.
/// Whether an arrow is bound to a name is decided by asking its
/// ancestors for a binding before a positional frame; an array is
/// neither, and the outer `x =` is a binding, so every arrow is a
/// function and the question climbs to that assignment, one level
/// below the root, from each (#1767).
#[must_use]
pub fn js_arrow_per_array_depth(depth: usize) -> String {
    format!(
        "x = {}0{};\n",
        "[() => 1,\n".repeat(depth),
        "]\n".repeat(depth)
    )
}

/// JavaScript: `let [a = 1, [a = 1, [ … b = 2 ] ] ] = v;`.
///
/// One default per level, each in an array pattern nested in the last.
/// ABC asks of every `=` whether it initialises a `const` binding, and a
/// default's `=` is answered by the first ancestor that is no pattern
/// layer: the declarator, two levels below the root, from every level
/// (#1868). `let`, so each default counts and the reading grows.
#[must_use]
pub fn js_nested_default_patterns(depth: usize) -> String {
    format!(
        "let {}b = 2{} = v;\n",
        "[a = 1, ".repeat(depth),
        "]".repeat(depth)
    )
}

/// JavaScript: `let [a, [a, [ … b ] ] ] = v;` — the nest of
/// [`js_nested_default_patterns`] with its defaults removed, so the one
/// `=` left is the declarator's and nothing climbs.
#[must_use]
pub fn js_nested_patterns(depth: usize) -> String {
    format!("let {}b{} = v;\n", "[a, ".repeat(depth), "]".repeat(depth))
}

/// Python: `x = [a and b, [a and b, [ … 0 ] ] ]`.
///
/// One boolean operator per level, each an element of a list nested in
/// the last. Each asks its ancestors whether it sits inside another
/// boolean operator before a `lambda`, and how many `lambda`s enclose it
/// before a statement boundary; a list is neither, so both questions
/// reach the root from every operator (#1767).
#[must_use]
pub fn python_and_per_list_depth(depth: usize) -> String {
    format!(
        "x = {}0{}\n",
        "[a and b,\n".repeat(depth),
        "]\n".repeat(depth)
    )
}

/// Python: `(self.a00000, (self.a00001, ( … self.aNNNNN))) = v` in a
/// method, one `self` attribute per level of a nested assignment target.
/// Whether each is bound was a climb through every wrapper above it to
/// the assignment, `depth` steps from the innermost (#1867). The names
/// are distinct, so the attribute count's dedup absorbs none of them,
/// and fixed-width, so the bytes stay affine in `depth`.
#[must_use]
pub fn python_nested_self_targets(depth: usize) -> String {
    python_nested_targets(depth, "self", &format!("self.a{depth:05}"))
}

/// [`python_nested_self_targets`] with a foreign `db.` receiver at
/// every level and one `self.z` innermost: the same target nest, whose
/// elements a receiver test declines before any climb, so only the
/// innermost attribute ever needed one.
#[must_use]
pub fn python_nested_foreign_targets(depth: usize) -> String {
    python_nested_targets(depth, "db", "self.z")
}

fn python_nested_targets(depth: usize, receiver: &str, innermost: &str) -> String {
    let mut source = String::from("class C:\n    def f(self, v):\n        ");
    for i in 0..depth {
        // `fmt::Write for String` never fails; see `wide_distinct_fns`.
        let _ = write!(source, "({receiver}.a{i:05}, ");
    }
    let _ = writeln!(source, "{innermost}{} = v", ")".repeat(depth));
    source
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

/// Ruby: a class body holding `depth` nested `if`s, a `def` at every
/// level beside the next `if`, so the member walk reads one wrapper
/// per level and declares one method under each.
#[must_use]
pub fn ruby_nested_if_defs(depth: usize) -> String {
    let mut source = String::from("class C\n");
    for i in 0..depth {
        // `fmt::Write for String` never fails; see `wide_distinct_fns`.
        let _ = writeln!(source, "if c{i}\ndef m{i}; end");
    }
    source.push_str(&"end\n".repeat(depth + 1));
    source
}

/// Ruby: a class body of `width` `def`s, each wrapped in a `private`
/// call, so the member walk reads one call's argument list per member
/// and re-files each declared name against the declarations so far.
/// Half the names are distinct and half redefine one method, so both
/// shapes of the re-filing index are on the ladder: a lookup that
/// scanned the name's declarations would be quadratic on the second
/// half alone.
#[must_use]
pub fn ruby_wide_private_defs(width: usize) -> String {
    let mut source = String::from("class C\n");
    for i in 0..width {
        // `fmt::Write for String` never fails; see `wide_distinct_fns`.
        let _ = writeln!(
            source,
            "private def m{}; end",
            if i % 2 == 0 { i } else { 0 }
        );
    }
    source.push_str("end\n");
    source
}

/// Ruby: a class body holding `depth` nested `if`s, an instance-variable
/// assignment and an `attr_reader` at every level beside the next `if`,
/// so the attribute walk reads one wrapper per level and asks the
/// `Member` row about one assignment target under each.
#[must_use]
pub fn ruby_nested_if_attrs(depth: usize) -> String {
    let mut source = String::from(
        "class C
",
    );
    for i in 0..depth {
        // `fmt::Write for String` never fails; see `wide_distinct_fns`.
        let _ = writeln!(source, "if c{i}\n@a{i} = 1\nattr_reader :r{i}");
    }
    source.push_str(&"end\n".repeat(depth + 1));
    source
}

/// Ruby: `depth` nested classes, each declaring one attribute and one
/// method before the next class opens. Every level is a container whose
/// body the member walks read, and each adds one attribute and one
/// method: with the members only in the innermost class the readings
/// would hold at one whatever the depth.
#[must_use]
pub fn ruby_nested_classes(depth: usize) -> String {
    format!(
        "{}{}",
        "class C\nattr_reader :a\ndef m; end\n".repeat(depth),
        "end\n".repeat(depth)
    )
}

/// Java and Groovy, which read it alike: `depth` nested classes, each
/// declaring one method with one `if` before the next class opens. No
/// method has an enclosing function, which is the answer the cognitive
/// depth surcharge has to reach the root to give, and the `if` is what
/// the reading counts: an abstract method would score nothing.
#[must_use]
pub fn nested_classes_with_method(depth: usize) -> String {
    format!(
        "{}{}",
        "class C {\nvoid m(boolean a) { if (a) { } }\n".repeat(depth),
        "}\n".repeat(depth)
    )
}

/// How many alternative constructors [`record_alternative_ctors`]
/// renders beyond its `width`: the header's list is ten components and
/// their commas, 21 children, so the body needs more than that at the
/// smallest size `shapes_widen_without_deepening` parses (16) to be the
/// node that widens.
const RECORD_BASE_CTORS: usize = 4;

/// Java and Groovy, which read it alike: one record of ten components
/// whose body holds `width` alternative constructors and
/// `RECORD_BASE_CTORS` more, each spelling the first nine component
/// types and a tenth that differs.
///
/// The record's header stands for the implicit canonical constructor
/// only where the body writes none, and a written one is told from an
/// alternative by its parameter types against the components' (#1842).
/// So the header's gate scans the body, and for each constructor named
/// after the record spells its types until one differs — here the
/// last, the most a constructor can cost. Every consumer that asks the
/// gate pays one such scan; a consumer asking it once per *member*
/// would pay `width` of them. The constructors are identical, which no
/// compiler accepts and both grammars parse, so the bytes per unit of
/// `width` are constant.
#[must_use]
pub fn record_alternative_ctors(width: usize) -> String {
    const COMPONENTS: &str = "int a, int b, int c, int d, int e, int f, int g, int h, int i";
    let ctor = format!("R({COMPONENTS}, long j) {{ }}\n");
    format!(
        "record R({COMPONENTS}, int j) {{\n{}}}\n",
        ctor.repeat(width + RECORD_BASE_CTORS)
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
        // `fmt::Write for String` never returns `Err`, so a generator
        // that writes a line per item has a `Result` to discard; the
        // alternatives clippy leaves are `format!`-into-`push_str` and
        // `map(format!).collect()`, and it rejects both. The Tcl and
        // Ruby generators below do the same and cite this.
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
/// the callable test tells them apart by asking the ancestors for a
/// name binding before a frame that proves positional use. Here the
/// enclosing `statement_block` is that frame two steps up — which is
/// the point; [`js_arrow_per_array_depth`] is the shape with no frame
/// near. Two steps was still `O(depth)`, because
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

/// JavaScript: `run(() => 1, () => 2, …);` — `width` positional arrows
/// as one call's arguments, the shape whose binding walk reaches the
/// parent's sibling scan (a bound `const cb = [...]` does not).
#[must_use]
pub fn wide_arrow_array(width: usize) -> String {
    let arrows = (0..width)
        .map(|i| format!("() => {i}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("run({arrows});\n")
}

/// C++: `class C { public: int f0; template<class T> static T t0; … }` —
/// `width` ordinary fields interleaved with `width` static data member
/// templates in one class body, so the attribute walk meets both of
/// its member arms as siblings of the same `field_declaration_list`.
#[must_use]
pub fn cpp_wide_class_members(width: usize) -> String {
    let members = (0..width)
        .map(|i| format!("int f{i}; template<class T> static T t{i};"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("class C {{\npublic:\n{members}\n}};\n")
}

/// Java: `class C { int f() { return ((…(1)…)); } }` — one field and
/// one method in a class body, with `depth` parentheses nested inside
/// the method, so the member walks meet one body and many nodes that
/// are no body.
#[must_use]
pub fn java_deep_expression_in_class(depth: usize) -> String {
    format!(
        "class C {{ int x; int f() {{ return {}1{}; }} }}\n",
        "(".repeat(depth),
        ")".repeat(depth)
    )
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

/// Perl: `sub f { return g(-name => g(-name => … 1)); }` — `depth`
/// calls, each the value of a signed bareword key in the one above.
///
/// The depth shape for the ABC value peel's sign test (#1603); its
/// width twin, reaching the same call site, is [`perl_bareword_keys`].
/// Every call's argument list is a negated-operand slot, and the peel
/// asks of the `-` under each `unary_expression` whether it signs a
/// key, a test that reads the `-`'s parent and the token after it. The
/// peel holds no ancestor chain, so the parent is handed in; replace
/// `perl_dash_signs_keyword_key_under(&op, node)` at `abc/perl.rs`'s
/// `P::DASH` arm with `perl_dash_signs_keyword_key(&op,
/// Ancestors::unknown())` and the test climbs `Node::parent` for both
/// reads — `O(depth)` per key and `O(depth²)` over the shape, fitting
/// ~2 against ~1 (2.02 at 1.15 s for 4 000 keys against 1.09 at
/// 5.9 ms on one host, 2.14 against 1.06 on a slower one).
#[must_use]
pub fn perl_nested_signed_keys(depth: usize) -> String {
    format!(
        "sub f {{ return {}1{}; }}\n",
        "g(-name => ".repeat(depth),
        ")".repeat(depth)
    )
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

/// Perl: `my @r = (` + `width` comment rows + `/a/, /a/, …, 1);` — the
/// list of [`perl_pattern_list`] behind as many comments as it holds
/// matches.
///
/// The grammar stores a list's operands as a `repeat()`, which a seek by
/// byte crosses in `O(log n)`, but the comments between the `(` and the
/// first operand are the list's own children, outside that repeat: a
/// seek or a scan from the list's start steps over each. The
/// pattern-role check reads the sibling after every pattern of a plain
/// list, so reading it from the list's start costs the comment run once
/// per pattern (#1868). It asks whether an operand precedes a pattern
/// only in a call's list, which [`perl_commented_split_list`] renders.
#[must_use]
pub fn perl_commented_pattern_list(width: usize) -> String {
    format!(
        "my @r = (\n{}{}1);\n",
        "# c\n".repeat(width),
        "/a/, ".repeat(width)
    )
}

/// Perl: `my @r = split(` + `width` comment rows + `/a/, /a/, …, 1);` —
/// the list of [`perl_commented_pattern_list`] as `split`'s arguments.
///
/// A plain list is asked only for the sibling after each pattern. A
/// call's argument list is asked instead whether an operand precedes
/// each pattern, which is what makes the first one `split`'s delimiter,
/// so the two shapes price one read each (#1868).
#[must_use]
pub fn perl_commented_split_list(width: usize) -> String {
    format!(
        "my @r = split(\n{}{}1);\n",
        "# c\n".repeat(width),
        "/a/, ".repeat(width)
    )
}

/// Perl: `my $x = !!…!name;` — one bareword under `depth` negations.
///
/// Whether a bareword is an auto-quoted key is read off the token after
/// it, found by climbing while the node is its parent's last child. Here
/// that is every level, so the one bareword climbs to the statement
/// (#1868).
#[must_use]
pub fn perl_negated_bareword(depth: usize) -> String {
    format!("my $x = {}name;\n", "!".repeat(depth))
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

/// Elixir: one call whose trailing keyword list holds `width` `else:`
/// pairs with the `do:` first, one with it last, and one with none.
///
/// The width shape for #1602's section gate, which asks of every pair
/// whether a `do:` sits in its list: an uncached scan per pair is
/// quadratic in the list, and the walk memoises the answer per list
/// (`MemoQuestion::KeywordListHasDo`). The reading is `width` per
/// `do:`-bearing list (+1 per `else:` section) and nothing for the
/// third.
#[must_use]
pub(crate) fn elixir_keyword_sections(width: usize) -> String {
    let elses = vec!["else: 1"; width].join(", ");
    format!("foo(do: 0, {elses})\nfoo({elses}, do: 0)\nfoo({elses})\n")
}

/// Elixir: two calls that each drive one of the section lookup's
/// position checks past `width` comments. `foo([pairs], comments… x)`:
/// a bracketed list that an operand follows, so the *trailing* check
/// rejects it — the comments between the list and `x` are what that
/// scan walks, and the leading check never runs. `bar([comments…
/// pairs])`: a trailing bracketed list with the comments *before* its
/// pairs, so the trailing check passes and the *leading* check walks
/// the comments and accepts. Then a plain `if` per pair, to give the
/// reading something the lookup does not produce.
///
/// Each answer is memoised — per argument
/// (`MemoQuestion::ArgumentIsTrailing`, #1602 review) and per list
/// (`KeywordsLeadTheirList`, #1697 cost audit); uncached, each of the
/// `width` pairs rescanned its run. The reading is `2 × width`: `width`
/// retained `if`s plus `bar`'s `width` `else:` sections (each with a
/// `do:` beside it); `foo`'s rejected list contributes nothing.
#[must_use]
pub(crate) fn elixir_rejected_keyword_list(width: usize) -> String {
    let pairs = vec!["do: 0, else: 1"; width].join(", ");
    let comments = "# c\n".repeat(width);
    let ifs = "if(a, do: 1)\n".repeat(width);
    format!("foo([{pairs}],\n{comments}x)\nbar([\n{comments}{pairs}])\n{ifs}")
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

/// Tcl: `x {{…{a b}…}}`, `depth` braces deep on one row.
///
/// #1416's fixture: one `braced_word` per level, two AST levels apart
/// (`braced_word` → `command` → `braced_word`), each a single-row match
/// for `-t braced_word` whose span holds every level inside it.
#[must_use]
pub fn tcl_nested_braces(depth: usize) -> String {
    format!("x {}a b{}\n", "{".repeat(depth), "}".repeat(depth))
}

/// Tcl: `width` sibling `{{a}}` words inside 66 nested braces, all on one
/// row, so the innermost brace is past `bca find`'s 64 whole-printed
/// levels and elides each word, and each word elides the brace inside it.
#[must_use]
pub fn tcl_wide_braces_past_the_limit(width: usize) -> String {
    let depth = 66;
    format!(
        "x {}{}{}\n",
        "{".repeat(depth),
        "{{a}} ".repeat(width),
        "}".repeat(depth)
    )
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
/// (#1110). `Find` is the count walk plus the renderer `bca find` prints
/// its matches through.
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
    /// `bca find`: `Ast::find` under one filter, then the CLI's
    /// renderer (`write_found`, the sink form of what `bca find` prints
    /// through) into a sink that reads every byte (#1416). Its reading
    /// is the lines rendered, one per match, which equals the `Count`
    /// reading on the same shape.
    Find {
        /// The filter, as `bca find --type` spells it.
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
            Self::Ops { .. } | Self::Count { .. } | Self::Find { .. } | Self::Dump { .. } => {
                MetricsOptions::default()
            }
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
    ///
    /// # Panics
    ///
    /// Never in practice: the only fallible step outside the seams is
    /// [`Workload::Find`]'s render, whose sink accepts every write.
    pub fn walk(self, ast: &Ast, options: MetricsOptions) -> Result<u64, MetricsError> {
        Ok(match self {
            Self::Metrics { reading, .. } => reading(&black_box(ast.metrics(options)?).metrics),
            Self::Ops { reading } => reading(&black_box(ast.ops()?)),
            Self::Count { filter } => {
                let (matching, _) = black_box(ast.count(&[filter.to_owned()]));
                u64::try_from(matching).unwrap_or(u64::MAX)
            }
            Self::Find { filter } => {
                let found = ast.find(&[filter.to_owned()])?;
                let mut sink = RenderedLines(0);
                big_code_analysis::write_found(&mut sink, ast, &found, ..)
                    .expect("RenderedLines never fails a write");
                sink.0
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

/// The `Find` workload's sink: counts the lines written to it.
///
/// It reads every byte rather than discarding them as `io::sink` does,
/// because the volume a render writes is half of what the probe prices:
/// a renderer that printed each nested match's text again would cost
/// nothing against a sink that never looks.
struct RenderedLines(u64);

impl std::io::Write for RenderedLines {
    // `bytecount` would be a dependency for one bench sink; this loop is
    // what the probe means by reading every byte.
    #[allow(clippy::naive_bytecount)]
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let lines = buf.iter().filter(|byte| **byte == b'\n').count();
        self.0 += u64::try_from(lines).unwrap_or(u64::MAX);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
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

/// Sibling counts for the two `perl-commented-*-list` probes, the ladder
/// of [`SPACE_MERGE_WIDTHS`]. With its sibling read put back on a pass
/// from the list's first child the plain list read 2.08 here, 0.9 s at
/// the top rung against 8.9 ms, and 1.07 without. The `split` list was
/// measured only on a host under heavy load, where times mean little:
/// 2.43 against 1.07. They were not measured on
/// [`LINEAR_WIDTHS`], the ladder of `abc/perl-pattern-list`.
const COMMENTED_LIST_WIDTHS: [usize; 3] = SPACE_MERGE_WIDTHS;

/// Depths for `nom/deep-attribute-run`, whose shape grows *two* axes
/// with one parameter.
///
/// Half [`LINEAR_DEPTHS`] at every rung, and reasoned rather than
/// harmonised. The shape renders `5 * depth + 2` rows to a depth probe's
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
///
/// `nom/deep-one-attribute-items` shares the ladder at 1.6 times the
/// bytes (eight rows per unit of depth, 160 KB at the top rung) and fits
/// 1.08 on it, so the same reasoning holds there with room to spare
/// (#1447).
const ATTRIBUTE_RUN_DEPTHS: [usize; 3] = [500, 1_000, 2_000];

/// Depths for `loc/python-paren-bare-strings`, held under the 200 levels
/// Python's tokenizer lets parentheses nest. Past them a parenthesis's
/// climb to its statement stops at the cap, so the per-statement
/// quadratic the probe guards cannot show on [`LINEAR_DEPTHS`]; under it,
/// it did — 3.9 s for 80 KB of 200-deep statements (#1610).
const PAREN_CAP_DEPTHS: [usize; 3] = [50, 100, 200];

/// Depths for `loc/c-conditional-per-struct-depth`, the next doubling
/// above [`LINEAR_DEPTHS`]. The copy it prices is one short chain per
/// conditional, so at [`LINEAR_DEPTHS`] the constant hides the exponent:
/// with the copy reinstated it read 1.50 there, *on* the `<=` bound and
/// not past it, and 1.81 here (#1699 review). A probe that passes with
/// the defect present is no probe; these sizes are where it goes red.
const CONDITIONAL_PER_DEPTH_DEPTHS: [usize; 3] = [4_000, 8_000, 16_000];

/// Depths for the three #1864 `nested-classes` probes, the ladder of
/// [`CONDITIONAL_PER_DEPTH_DEPTHS`]. With the chain scan per function
/// reinstated the two cognitive probes read 2.45 and 2.62 here, 4.2 s
/// and 5.8 s at the top rung, inside `MAX_CELL_WALK`; a rung higher the
/// scan would run past it and the gate would report an abandoned walk
/// rather than an exponent. Without the scan they read 0.60–0.91. They
/// were not measured on [`LINEAR_DEPTHS`].
const ENCLOSING_FUNCTION_DEPTHS: [usize; 3] = CONDITIONAL_PER_DEPTH_DEPTHS;

/// Depths for `abc/js-nested-default-patterns` and its control, the
/// ladder of [`CONDITIONAL_PER_DEPTH_DEPTHS`]. With the climb per
/// default reinstated the probe read 2.66 here, 2.1 s at the top rung,
/// inside `MAX_CELL_WALK`; without it, 1.18 and 9.3 ms. The control
/// read 0.78 and 0.84. They were not measured on [`LINEAR_DEPTHS`].
const PATTERN_DEFAULT_DEPTHS: [usize; 3] = CONDITIONAL_PER_DEPTH_DEPTHS;

/// Depths for the three `ruby-nested-classes` probes, one rung above
/// [`CONDITIONAL_PER_DEPTH_DEPTHS`]. The copy they price is the walk's
/// chain per class body, and a class body walk is cheap beside it only
/// once the chain outgrows cache: with the copy present they read
/// 1.39–1.51 on that ladder, straddling the bound (`npa` was over it on
/// two runs of three), and 2.18–2.19 here; with the chain borrowed,
/// 0.99–1.12 there and 1.05–1.19 here (#1805). The `loc` control reads
/// 1.22–1.28 here, the cache drift of a 64,000-entry chain. The
/// regressed top cell is 1.8 s, inside `MAX_CELL_WALK`. The clean cells
/// of the three probes take about 1.2 s per pass, and a gate run walks
/// every cell ten times (calibration, two warm-up rounds, seven
/// measured), so they cost about 12 s of it, close to half its walk
/// time: the top rung is what separates the copy from the bound.
const NESTED_CONTAINER_DEPTHS: [usize; 3] = [8_000, 16_000, 32_000];

/// Depths for `find/tcl-nested-braces` and its control, the same two
/// rungs above [`LINEAR_DEPTHS`] and for the same reason. A renderer
/// that printed each match's whole span wrote `depth²` bytes against a
/// per-match cost the bytes take thousands of levels to overtake: on
/// [`LINEAR_DEPTHS`] it read 1.56, 0.06 past the bound (#1416).
const FIND_RENDER_DEPTHS: [usize; 3] = [4_000, 8_000, 16_000];

/// Depths for the three #1767 probes, one rung above [`LINEAR_DEPTHS`].
/// Each prices an ancestor question with no stop near, folded down the
/// walk's chain instead of climbed per node. On [`LINEAR_DEPTHS`] the
/// TypeScript probe read 1.65–1.69 with the climb, 0.15 past the bound;
/// here the three read 2.10–2.18, 1.95–2.04 and 1.87–1.97 with it and
/// 1.26–1.32, 1.00–1.03 and 0.70–0.73 without, the clean cells 3–19 ms
/// rather than 1.4–8 ms. The C++ probe's clean fit leaves 0.18 under
/// [`LINEAR_BOUND`]; the walk is linear there, so a reading that creeps
/// toward the bound is cache drift on a deep tree before it is a
/// regression. The Python probe added later read 2.00–2.01 with its climbs
/// and 1.02–1.09 without on the same ladder.
const ANCESTOR_GATE_DEPTHS: [usize; 3] = [2_000, 4_000, 8_000];

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
        name: "count/tcl-nested-braces",
        lang: LANG::Tcl,
        axis: Axis::Depth,
        workload: Workload::Count {
            filter: "braced_word",
        },
        render: tcl_nested_braces,
        sizes: FIND_RENDER_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "The walk control for `find/tcl-nested-braces`: the same \
                    shape and filter through the walk `bca find` shares, \
                    with nothing rendered, so a regression that moves the \
                    find probe and not this one is in the renderer (#1416). \
                    The reading is one match per brace level.",
    },
    Probe {
        name: "find/tcl-nested-braces",
        lang: LANG::Tcl,
        axis: Axis::Depth,
        workload: Workload::Find {
            filter: "braced_word",
        },
        render: tcl_nested_braces,
        sizes: FIND_RENDER_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "`bca find`'s render after the walk (#1416). Each match \
                    once chose its connector through `Node::parent`, an \
                    `O(depth)` climb per match: reinstating it fits \
                    2.00 against 0.99. Each single-row match also \
                    printed its whole span, `depth²` bytes on this shape; \
                    now only the first 64 levels do, and the rest elide \
                    what is nested in them. Printing every level whole \
                    again (the limit at `usize::MAX`) fits 1.80 against \
                    1.01, which is why the ladder starts at 4 000. The \
                    reading is the lines rendered, one per match.",
    },
    Probe {
        name: "find/tcl-wide-braces-past-the-limit",
        lang: LANG::Tcl,
        axis: Axis::Width,
        workload: Workload::Find {
            filter: "braced_word",
        },
        render: tcl_wide_braces_past_the_limit,
        sizes: SPACE_MERGE_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "The width half of `find/tcl-nested-braces` (#1416): past \
                    the 64 whole-printed levels a match elides each match \
                    nested in it, stepping from one to the next through \
                    `next_outside`, so the level holding `width` words \
                    visits each word once. Finding each next word by walking \
                    the siblings again from the first prints the same text \
                    and passes every renderer test; it fits 1.81 here \
                    against 1.01, while the depth probe, a chain with one \
                    match per level, stays at 1.02. The reading is the lines \
                    rendered, one per match.",
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
        name: "cognitive/else-if-ladder",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: else_if_ladder,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1697: the `ElseIf` row's parent-clause gate on the \
                    shape `cognitive/nested-if` cannot reach — every `if` \
                    after the first is inside an `else_clause`, so the \
                    gate answers `true` on each and the reading is \
                    `depth + 2`, where a gate that stopped recognising \
                    the chain nests every `if` and reads `~depth²/2`. A \
                    gate that climbed the chain is quadratic here too.",
    },
    Probe {
        name: "cognitive/wide-if-siblings",
        lang: LANG::Java,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: wide_if_siblings,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1697: the `ElseIf` row's previous-sibling gate (Java, \
                    C#, Groovy, Kotlin) reads `Ancestors::previous_sibling`, \
                    a binary search through the walk's `SiblingCache` on a \
                    wide parent and a short scan on a narrow one. A gate \
                    that seeks from the parent's first child is `O(index)` \
                    per `if`, quadratic in a wide block (#1584).",
    },
    Probe {
        name: "cognitive/python-wide-if-siblings",
        lang: LANG::Python,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: python_wide_if_siblings,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1697: Python's `ElseIf` gate asks every `if_statement` \
                    whether it is the sole child of an `else:` block. The \
                    predicate it replaced counted the enclosing block's \
                    children before reading whose block it was, quadratic \
                    in the block's width; the gate reads the chain first \
                    and stops its scan at the second child.",
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
                    asks the declaration's ancestor question.",
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
        rationale: "#1084: `loc`'s C-family arm asks every `declaration` \
                    whether a loop or `if` header holds it, stopping at \
                    the enclosing `compound_statement` one step up. The \
                    question reads the walker's ancestor chain instead of \
                    calling `Node::parent`, and since #1767 folds down it \
                    on `GateLane::InsideHeader`. `loc/nested-while` is \
                    the same nesting without a declaration; \
                    `loc/cpp-nested-namespaces` the same question with no \
                    stop near.",
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
        rationale: "#1084: Elixir's callable test asks \
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
        name: "cognitive/elixir-keyword-sections",
        lang: LANG::Elixir,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: elixir_keyword_sections,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1602: a keyword section scores when a `do:` sits in \
                    its list, asked of every pair; the answer is memoised \
                    per list, so a list of N pairs costs one scan, not N — \
                    exponent 1.97 with the memo replaced by a per-pair \
                    scan, ~1.0 with it. The `do:` sits first in one list, \
                    last in another and nowhere in the third.",
    },
    Probe {
        name: "cognitive/elixir-rejected-keyword-list",
        lang: LANG::Elixir,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: elixir_rejected_keyword_list,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1602 review, #1697 cost audit: the section lookup's \
                    two position checks — is the bracketed list the \
                    trailing argument; do its pairs lead the brackets — \
                    are asked of every pair, and each scans a sibling run \
                    the shape fills with N comments: one call's list is \
                    rejected by the first check, the other's accepted \
                    after the second. Uncached, each rescanned its run per \
                    pair — exponent 1.92 on the trailing scan alone, 1.68 \
                    with the retained `if`s beside it, 1.67 on the leading \
                    scan (~2 on `main`) — and the walk memoises both, per \
                    argument and per list (1.0). The `do:` memo is on the \
                    accepted list's path, so the probe prices all three.",
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
                    node whether a function encloses it. The answer comes \
                    off the walker's ancestor chain, folded down it since \
                    #1864; the scan #1062 introduced found the function \
                    two steps up, and climbing with `Node::parent` cost \
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
        name: "loc/bash-nested-continued-fn",
        lang: LANG::Bash,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.ploc(),
        },
        render: bash_nested_continued_fns,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1445: a row holding only `\\` has no node, so `Loc` \
                    finds it in the padding between a node's children. Each \
                    padding byte belongs to one node, so the scan is linear \
                    however deep the spaces nest. Scanning every node's \
                    whole range instead, which reads level k once per \
                    enclosing level, took this from 1.09 to 1.93. The \
                    reading counts the bare rows too, one in five of the \
                    file's code rows.",
    },
    Probe {
        name: "loc/tcl-wide-continued-comment",
        lang: LANG::Tcl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: tcl_wide_continued_comments,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1445: a bare `\\` row after a comment is that \
                    comment's text, decided by looking up the comment node \
                    under the row before it, once per bare row, from the \
                    child just before the bare row's gap. Finding the \
                    comment by walking the root's children instead measured \
                    2.00 against 1.01 at widths 1 000-4 000; at this probe's \
                    own sizes one such walk at 16 000 exceeds the 20 s \
                    budget. The reading is the comment rows, half of them \
                    the bare ones.",
    },
    Probe {
        name: "loc/tcl-unclosed-continued-comments",
        lang: LANG::Tcl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: tcl_unclosed_continued_comments,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1445 review: an unclosed opener per unit leaves every \
                    unit a direct child of one flat root `ERROR`, which \
                    tree-sitter never balances, and its \
                    `descendant_for_byte_range` scans each level's children \
                    from the first. The comment lookup descends from the \
                    child before the bare row's gap, a constant per bare \
                    row. Descending from the gap's owner, the root, instead \
                    measured 2.09 against 1.10. The reading is the comment \
                    rows, half of them the bare ones.",
    },
    Probe {
        name: "loc/elixir-unclosed-spliced-comments",
        lang: LANG::Elixir,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: elixir_unclosed_spliced_comments,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1801 review: Elixir never continues a comment, so a \
                    bare `\\` row after `# c \\` starts a logical line that \
                    `spliced_into` classifies by the comment it joins, read \
                    off the gap's next child under the flat root `ERROR` \
                    the unclosed `fn`s leave. Looking that comment up from \
                    the gap's owner instead measured 1.95 / 2.00 against \
                    0.94 / 0.96 (dev `bca` end to end at these sizes). The \
                    reading is the comment rows, a third of them the bare \
                    ones.",
    },
    Probe {
        name: "loc/c-unclosed-spliced-comments",
        lang: LANG::C,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: c_unclosed_spliced_comments,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1801 review: a bare row joining `/* a */ /* b */` is a \
                    comment only once both comments are read, and the \
                    second is a sibling of the first under the flat root \
                    `ERROR` the unclosed braces leave. The per-walk index \
                    reads both in its one cursor walk (#1818; before it the \
                    gap scan stepped a copy of its own cursor to the \
                    second); `next_sibling` instead measured 1.91 / 1.97 \
                    against 0.91 / 0.95 (dev `bca` end to end at these \
                    sizes). The reading is the comment rows, half of them \
                    the bare ones.",
    },
    Probe {
        name: "loc/c-comment-chain",
        lang: LANG::C,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: c_comment_chain,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1818: one logical line of `width` comment-only rows \
                    and `width` bare rows, every bare row a comment. The \
                    per-walk index settles the line once; reading it back \
                    from each bare row to its start, or forward to its end, \
                    is `width` per row. On `LINEAR_WIDTHS` that rescan \
                    fitted 1.41 against 1.01 clean — a quadratic term over \
                    a cheap walk, the `SPACE_MERGE_WIDTHS` case — so the \
                    ladder is the taller one, where it fits 1.90 against \
                    1.05. The reading is the comment rows, half of them \
                    the bare ones.",
    },
    Probe {
        name: "loc/c-unclosed-comment-rows",
        lang: LANG::C,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: c_unclosed_comment_rows,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1818: `width` comment-only rows of three comments each \
                    under one flat root `ERROR`, each continued onto a bare \
                    row that joins an empty row. The index's comment spans \
                    are one cursor walk; a per-row lookup from the gap's \
                    owner scans the root's children each time. The reading \
                    is the comment rows, half of them the bare ones.",
    },
    Probe {
        name: "loc/python-long-bare-run",
        lang: LANG::Python,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: python_long_bare_run_to_comment,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1819: Python keeps each bare `\\` row as a \
                    `line_continuation` node, and a run of them is one \
                    logical line credited by its head alone; a later node \
                    of the run returns in O(1) from its own bytes. \
                    Classifying at every node, which reads the rest of the \
                    run each time, measured 2.00 against 1.33 — the 1.3 \
                    is the walk over `width` extras under one `module`, \
                    which `main` without the arm reads the same (1.37), \
                    not the arm. The reading is the comment rows: the run \
                    and the row it joins.",
    },
    Probe {
        name: "loc/python-long-bare-run-blank",
        lang: LANG::Python,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.blank(),
        },
        render: python_long_bare_run_to_blank,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1819: `loc/python-long-bare-run` with the run joining \
                    an empty row, the Blank class, whose head reads no \
                    sibling at all. Classifying at every node measured \
                    1.99 against 1.31; `main` without the arm reads 1.19, \
                    the walk over `width` extras under one `module`, so \
                    the exponent is the walk's, not the arm's. The reading \
                    is the blank rows: the run and the empty row.",
    },
    Probe {
        name: "loc/python-separated-bare-runs",
        lang: LANG::Python,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: python_separated_bare_runs,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Fourth re-review of #1819: `width` separate bare `\\` \
                    runs inside one parenthesised bare string's parentheses, \
                    each asking whether it is on the string's line. The \
                    answer is the parenthesis's, memoised per parenthesis \
                    under `NodeQuestion::BareStringParenthesisLine`; finding \
                    the string from each run through the siblings before it \
                    (117576c1) measured 2.05 against 1.26, and 1852f4e8 \
                    before the finder, which read every such run forward, \
                    1.32 — the walk over `width` extras under one \
                    parenthesis, as `loc/python-long-bare-run` reads it. The \
                    reading is the comment rows: the string's, the `)`'s and \
                    the `width` bare rows, which the runs alone supply — a \
                    run read forward makes its row blank, and 1852f4e8 read 2.",
    },
    Probe {
        name: "loc/python-bare-runs-before-string",
        lang: LANG::Python,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: python_bare_runs_before_string,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1830: `width` separate bare `\\` runs, each joining a \
                    comment row, *before* the string inside one parenthesised \
                    bare string's parentheses. Both extras are named, and the \
                    parenthesis's value is read past them, so each run's head \
                    asks the parenthesis's memoised question \
                    (`NodeQuestion::BareStringParenthesisLine`) whose own \
                    descent crosses the whole prefix; asked per run it is \
                    quadratic in `width`, 2.08 against \
                    1.02 with the memo. The reading is the comment \
                    rows: the parentheses', the string's, the `width` \
                    comments and the `width` bare rows. The bare rows read \
                    as comments before #1830 too, by joining `# c` forward; \
                    the two parenthesis rows are what the fix moved, so the \
                    pre-fix reading was `2 × width + 1`.",
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
                    `function`, which the callable test answers from the \
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
        rationale: "#1088: the JS-family callable test and `is_closure` \
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
        rationale: "#1696: the arrow form of the JS binding walk asks whether \
                    the parent holds a `property_identifier` child, an \
                    `O(width)` scan of the parent's children, for every arrow \
                    the ancestor test leaves unbound — each of a call's \
                    positional arguments. Asked of the parent once per arrow, \
                    4,000 of them took 3.4 s on the base; memoised per parent \
                    through the walk's `FoldCache` they take 0.01 s. A bound \
                    arrow (`const cb = [...]`) never reaches the scan, so a \
                    bound shape measures only the lazy path.",
    },
    Probe {
        name: "npa/java-deep-expression",
        lang: LANG::Java,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Npa, Metric::Npm],
            reading: |m| m.npa.class_na_sum() + m.npm.class_nm_sum(),
        },
        render: java_deep_expression_in_class,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1696 review: the member walks ask the `Member` and \
                    `FuncOpener` rows under each member's own chain, built \
                    by `Ancestors::of_children` — then an `O(depth)` copy \
                    per body. Built before the body test, it ran for every \
                    node the walk visits and a 64,000-deep expression took \
                    3–6 s in each of `npa` and `npm`; built inside the body \
                    arms, 0.04 s. Since #1805 the derivation borrows the \
                    walk's chain, so building it per node would cost one \
                    allocation per node: a constant factor this probe no \
                    longer prices as a change of class. The reading is the \
                    one field and one method, which the expression's depth \
                    does not touch.",
    },
    Probe {
        name: "npa/cpp-wide-class-members",
        lang: LANG::Cpp,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Npa, Metric::Npm],
            reading: |m| m.npa.class_na_sum() + m.npm.class_nm_sum(),
        },
        render: cpp_wide_class_members,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1299: the one C++ shape the member walks are timed on. \
                    Each `field_declaration` and each static data member \
                    template is asked of `cpp_declares_function` by both \
                    the attribute and the method consumer, a declarator \
                    chain walk that must stay bounded by the member's own \
                    wrappers and never read a sibling; the reading is the \
                    `2 × width` attributes, which a dropped arm halves.",
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
                    the root per step; it is now read off the walker's \
                    chain, the parent an index (#1447). `nom/nested-fn` \
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
                    #1100 then budgeted the parent's width against the \
                    node's depth; #1447 replaced the budget with the \
                    walker's `SiblingCache`, and this probe keeps the \
                    shallow-wide axis of that reading measured.",
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
                    place. The run is `3 * depth` so its parent stayed \
                    inside the width budget #1447 later removed \
                    (`6 * depth + 3` children at that depth); the shape \
                    stays as the long-run axis of the diagonal.",
    },
    Probe {
        name: "nom/deep-doc-comment-run",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: true,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_fns_with_doc_comment_run,
        sizes: ATTRIBUTE_RUN_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1647: `nom/deep-attribute-run` with the run spelled as \
                    `3 * depth` pairs of an outer doc comment and a plain \
                    comment above one `#[cfg(test)] fn t`. A comment is \
                    read as part of the run now — rustc attaches a `///` \
                    as `#[doc]` and reads `//` as no token — so each doc \
                    row opens the run like an attribute and the first \
                    one's verdict answers for the rest through \
                    `SubtreeSkip`'s reach. With the reach removed \
                    (`covering_through_node` dropped from \
                    `rust_attribute_run_verdict`) the shape fits 1.86 \
                    (4.3 / 15.5 / 56.2 ms); with it, 1.08 (0.6 / 1.2 / \
                    2.6 ms).",
    },
    Probe {
        name: "nom/deep-one-attribute-items",
        lang: LANG::Rust,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: true,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_fns_with_one_attribute_items,
        sizes: ATTRIBUTE_RUN_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1447: the many-runs axis of #1446's diagonal — \
                    `3 * depth` one-attribute items in the innermost of \
                    `depth` nested bodies. One ask per run is fine; the \
                    ask itself read the parent by a cursor pass from its \
                    first child, because #1100's width-against-depth \
                    budget admitted the parent, so `3 * depth` asks of \
                    `O(width)` each fitted 2.00 (0.74 / 2.99 / 11.97 s). \
                    The attribute run and the decorated item now locate \
                    the node by a search rather than that pass, which \
                    fits 1.08 (1.8 / 3.7 / 7.9 ms). The search is the \
                    walker's `SiblingCache` here, but a `repeat()` body \
                    seeks in `O(log n)` too, so this prices the pass, \
                    not the cache.",
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
        name: "nargs/java-record-alternative-ctors",
        lang: LANG::Java,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nargs],
            reading: |m| m.nargs.function_args_sum(),
        },
        render: record_alternative_ctors,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1863: a record's header gate scans the body for a \
                    canonical constructor, spelling each same-named \
                    constructor's parameter types until one differs from \
                    the component's. One scan is linear in the body, and \
                    each consumer of the gate pays one per record: 1.01. \
                    With the gate asked once per constructor instead, one \
                    walk at the top rung took 47 s against 32 ms and the \
                    gate abandons the probe over its per-walk budget. \
                    The reading is ten arguments per constructor and ten \
                    for the header.",
    },
    Probe {
        name: "nargs/groovy-record-alternative-ctors",
        lang: LANG::Groovy,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nargs],
            reading: |m| m.nargs.function_args_sum(),
        },
        render: record_alternative_ctors,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1863: the same gate on the same source as \
                    `nargs/java-record-alternative-ctors`, under the \
                    Groovy grammar, whose header is a `record_components` \
                    list and whose gate is a twin in `roles/groovy.rs`: \
                    1.01, and 49 s against 26 ms at the top rung with the \
                    gate asked once per constructor.",
    },
    Probe {
        name: "loc/rust-attributed-docs",
        lang: LANG::Rust,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: true,
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
                    item. Every doc comment is a doc row. The flag is on \
                    because the whole stretch is one attribute run to the \
                    prune since #1647 — a doc comment opens it and a \
                    `#[test]` in place of `#[inline]` would take every \
                    row — so the hook reads the run once from its first \
                    doc row here too; `#[inline]` prunes nothing, and the \
                    reading is unchanged.",
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
        name: "loc/c-conditional-per-struct-depth",
        lang: LANG::C,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.dloc(),
        },
        render: c_conditional_per_struct_depth,
        sizes: CONDITIONAL_PER_DEPTH_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1699: `conditional_declares` asks each child of a \
                    conditional with a context whose `parent` is the \
                    conditional, built from one local and no copy of the \
                    walk's chain. A copy of the chain per conditional — \
                    `children_chain`, the first shape this took — was \
                    `O(depth)` each, and independent conditionals at \
                    growing depths made it quadratic: 1.81 measured \
                    with the copy at these sizes, 0.94 without (1.50 \
                    and 1.04 at `LINEAR_DEPTHS`, where the copy sits on \
                    the bound rather than past it). The \
                    nested-conditional probe grows the other axis and \
                    cannot see it. A struct nest rather than a namespace \
                    nest: nested C++ namespaces are quadratic under `loc` \
                    on their own (#1767), which would hide what this \
                    prices. Every block binds, so the reading is the doc \
                    rows, one per level.",
    },
    Probe {
        name: "loc/cpp-nested-namespaces",
        lang: LANG::Cpp,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.lloc(),
        },
        render: cpp_nested_namespaces,
        sizes: ANCESTOR_GATE_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1767: every C++ `declaration` asks whether a loop or \
                    `if` header holds it, stopping at the nearest \
                    `compound_statement`. A namespace body is a \
                    `declaration_list`, so in a namespace nest nothing \
                    stops the climb short of the root and the shape was \
                    quadratic in depth. The walk now folds the answer \
                    down its chain on `GateLane::InsideHeader`: \
                    2.10–2.18 with the climb reinstated, 1.26–1.32 \
                    without. \
                    `loc/nested-declaration` is the same question with \
                    the stop one step up. The reading is `lloc`, one per \
                    declaration, so a shape that lost them stops growing.",
    },
    Probe {
        name: "loc/ts-nested-fn",
        lang: LANG::Typescript,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.lloc(),
        },
        render: ts_nested_functions,
        sizes: ANCESTOR_GATE_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1767: every TypeScript declaration asks whether an \
                    `ambient_declaration` holds it, a question with no \
                    stop at all, so each climbed to the root and a plain \
                    function nest was quadratic. Folded on \
                    `GateLane::InsideAmbient`: 1.95–2.04 with the climb \
                    reinstated, 1.00–1.03 without. The export question on \
                    `GateLane::InsideExport` is asked of the same nodes \
                    and stops at the first `statement_block`, so this \
                    shape prices the ambient lane alone. The reading is \
                    `lloc`, one per declaration.",
    },
    Probe {
        name: "nom/js-arrow-per-array-depth",
        lang: LANG::Javascript,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.functions_sum(),
        },
        render: js_arrow_per_array_depth,
        sizes: ANCESTOR_GATE_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1767: whether a JS arrow is bound to a name is asked \
                    of its ancestors until a binding or a positional \
                    frame. An array is neither, so an arrow per array \
                    level climbed to the outer `x =` from every arrow and \
                    the shape was quadratic. Folded on \
                    `FoldLane::NamedBinding`: 1.87–1.97 with the climb \
                    reinstated, 0.70–0.73 without. Every arrow is bound, so \
                    the reading is `functions_sum`, one per level; an \
                    arrow misread as a closure stops it growing.",
    },
    Probe {
        name: "cognitive/python-and-per-list-depth",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: python_and_per_list_depth,
        sizes: ANCESTOR_GATE_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1767: every Python boolean operator asks whether it \
                    sits inside another before a `lambda`, and how many \
                    `lambda`s enclose it before a statement boundary. A \
                    list is neither, so with an operator per list level \
                    both questions climbed to the root and the shape was \
                    quadratic. Folded on `GateLane::InsideBooleanChain` \
                    and `FoldLane::LambdaDepth`: 2.00–2.01 with both climbs \
                    reinstated, 1.02–1.09 without. Every operator is the \
                    outermost of its chain, so the reading is the \
                    cognitive sum, one per level.",
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
                    conditional: bypassing the memo fits 1.95 against \
                    0.92 with it. Every block binds, so the reading is the \
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
        name: "loc/python-deep-paren-extra-prefix",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: python_deep_paren_extra_prefix,
        sizes: PAREN_CAP_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1863: every parenthesis of a parenthesised bare string \
                    that opens with bare `\\` runs asks whether code precedes \
                    it on its logical line, which defers level by level to \
                    the root where the levels hold only comments and \
                    splices. Climbed from each parenthesis that is \
                    `depth² / 2` levels under CPython's 200-level cap; each \
                    level's nearest answer is memoised on \
                    `LevelQuestion::CodeBeforeOnLine` and read once. With \
                    the climb put back this reads 1.96, 100 ms at the top \
                    rung against 3.5 ms, and 0.99 without. The reading is \
                    the comment rows: each level's `(` row, bare rows and \
                    comments, the string's and the `)` row.",
    },
    Probe {
        name: "loc/python-block-comments-paren-strings",
        lang: LANG::Python,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.cloc(),
        },
        render: python_block_comments_paren_strings,
        sizes: SLOT_SCAN_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1863: `width` parenthesised bare strings, each holding \
                    a bare `\\` row, in an `if` block that opens with \
                    `width` comment rows. Each parenthesis asks whether code \
                    precedes it on its logical line, and its own statement's \
                    level answers. The block's level above it is a scan of \
                    the `width` comments back to the `:`, which only the \
                    first statement needs. Read for every statement, as a \
                    fold of the answer down the chain reads it, the walk is \
                    quadratic in `width`: 2.02 and 763 ms at the top rung \
                    with that fold, 1.00 and 26 ms with the memoised climb. \
                    The reading is the comment rows: the `width` comments \
                    and three rows per string.",
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
        name: "npm/ruby-nested-if-defs",
        lang: LANG::Ruby,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Npm],
            reading: |m| m.npm.class_nm_sum(),
        },
        render: ruby_nested_if_defs,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1401: the Ruby member walk reads a `def` through every \
                    conditional, loop and `begin` at class scope, moving one \
                    ancestor chain down and back up as it enters and leaves \
                    each wrapper. Built afresh per level with \
                    `Ancestors::of_children`, the chain was an `O(depth)` \
                    copy per wrapper, and since #1805 copies every level \
                    derived so far: quadratic over the nest either way. \
                    Moved, it is one push and pop. The reading is one \
                    method per level.",
    },
    Probe {
        name: "npm/ruby-wide-private-defs",
        lang: LANG::Ruby,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Npm],
            reading: |m| m.npm.class_nm_sum(),
        },
        render: ruby_wide_private_defs,
        sizes: LINEAR_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1401: every call in a Ruby class body is read for a `def` \
                    in its arguments, and a visibility call re-files each \
                    name its arguments declare. A re-file records one \
                    `(covered, bit)` on the name's index entry and the tally \
                    applies it once over that prefix at the end, so `W` \
                    `private def`s re-file `W` names at one lookup each \
                    whether the names are distinct or one name redefined — \
                    before the index, a scan of every declaration per name, \
                    32,000 distinct took 0.69 s against 0.19 s undecorated; \
                    writing each position's bit on every re-file, 32,000 of \
                    one name took 0.32 s against 0.18 s on `main` (#1401 \
                    reviews). The shape renders both halves. The body's \
                    child list is read once through a held iterator, not \
                    re-sought per statement. The reading is one method per \
                    call.",
    },
    Probe {
        name: "npa/ruby-nested-if-attrs",
        lang: LANG::Ruby,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Npa],
            reading: |m| m.npa.class_na_sum(),
        },
        render: ruby_nested_if_attrs,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1719: the Ruby attribute walk reads the statements the \
                    method walk does, through every conditional at class \
                    scope, and asks the `Member` row whether an assignment's \
                    target is an attribute under the chain the walk holds, \
                    one push and pop below the statement. Asked with \
                    `Ancestors::unknown()` instead, the gate's parent lookup \
                    climbs from the root and the walk measured quadratic \
                    (exponent 1.98 against 1.10). The reading is two \
                    attributes per level.",
    },
    Probe {
        name: "npa/ruby-nested-classes",
        lang: LANG::Ruby,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Npa],
            reading: |m| m.npa.class_na_sum(),
        },
        render: ruby_nested_classes,
        sizes: NESTED_CONTAINER_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1805: every member walk reads a container's body \
                    under `Ancestors::of_children`, once per container. \
                    Built as a copy of the walk's chain plus the body, \
                    that is `O(depth)` per class and quadratic over a \
                    nest; borrowing the walk's chain and owning only the \
                    body makes it one allocation per class. With the copy \
                    reinstated this reads 2.18–2.19, and 1.11–1.19 without; \
                    on `CONDITIONAL_PER_DEPTH_DEPTHS` the copy read \
                    1.49–1.51, straddling the bound, which is why the \
                    ladder is taller. Every language's member walks \
                    derive the chain the same way; Ruby's class nest is \
                    the shape that prices it. The reading is one \
                    attribute per level.",
    },
    Probe {
        name: "npm/ruby-nested-classes",
        lang: LANG::Ruby,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Npm],
            reading: |m| m.npm.class_nm_sum(),
        },
        render: ruby_nested_classes,
        sizes: NESTED_CONTAINER_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1805: the method walk on the same shape and the same \
                    per-container chain as `npa/ruby-nested-classes`. With \
                    the copy reinstated this reads 2.18–2.19, and 1.05–1.07 \
                    without; 1.39–1.45 on `CONDITIONAL_PER_DEPTH_DEPTHS`, \
                    under the bound with the defect present. The reading \
                    is one method per level.",
    },
    Probe {
        name: "loc/ruby-nested-classes",
        lang: LANG::Ruby,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Loc],
            reading: |m| m.loc.ploc(),
        },
        render: ruby_nested_classes,
        sizes: NESTED_CONTAINER_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1805: metric control for the two `ruby-nested-classes` \
                    rows: the same nest under a metric that derives no \
                    chain for a class body, so what they read above it is \
                    the member walks' own. It reads 1.22–1.28 with and \
                    without the copy: the drift of a walk under a chain of \
                    64,000 entries, which the two probes ride on as well. \
                    The reading is the physical rows, four per level.",
    },
    Probe {
        name: "cognitive/java-nested-classes",
        lang: LANG::Java,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: nested_classes_with_method,
        sizes: ENCLOSING_FUNCTION_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1864: `increment_function_depth` asks every function \
                    whether a function encloses it. `cognitive/nested-fn` \
                    prices the shape where one does, two steps up; here \
                    none does, so a scan of the chain per method read \
                    every ancestor and was quadratic over the nest. The \
                    answer is folded down the walk's chain on \
                    `KindLane::InsideFunction`. With the scan reinstated \
                    this reads 2.45, 4.2 s at the top rung, and 0.60–0.66 \
                    without, 82–92 ms; the first rung reads high in the \
                    `nom` control too, which is what puts both under 1 and \
                    leaves room under the bound for a relapse milder than \
                    the scan. The exact counts in \
                    `the_cognitive_walk_folds_the_enclosing_function_question` \
                    are the guard for that. The reading is one `if` per \
                    level.",
    },
    Probe {
        name: "nom/java-nested-classes",
        lang: LANG::Java,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Nom],
            reading: |m| m.nom.total(),
        },
        render: nested_classes_with_method,
        sizes: ENCLOSING_FUNCTION_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1864: metric control for \
                    `cognitive/java-nested-classes`: the same nest under \
                    `Nom`, which `Cognitive` selects as a dependency and \
                    which asks no ancestor question. It reads 0.72–0.90 \
                    with and without the scan. The reading is one method \
                    per level.",
    },
    Probe {
        name: "cognitive/groovy-nested-classes",
        lang: LANG::Groovy,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Cognitive],
            reading: |m| m.cognitive.cognitive_sum(),
        },
        render: nested_classes_with_method,
        sizes: ENCLOSING_FUNCTION_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1864: the same question on the same source as \
                    `cognitive/java-nested-classes`, under the Groovy \
                    grammar, which parses nested type declarations since \
                    0.5.0 and so reaches the shape. With the scan \
                    reinstated this reads 2.62, 5.8 s at the top rung, \
                    and 0.82–0.91 without, 75–83 ms. The reading is one \
                    `if` per level.",
    },
    Probe {
        name: "npa/python-nested-self-targets",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Npa],
            reading: |m| m.npa.class_na_sum(),
        },
        render: python_nested_self_targets,
        sizes: ANCESTOR_GATE_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1867: each `self.` attribute of a nested assignment \
                    target was asked whether it is bound by climbing the \
                    pattern wrappers above it to the assignment, `k` \
                    steps at depth `k`, so a target `n` deep cost `n²`. \
                    The attribute count walks down from the target \
                    through those wrappers only, where the climb can only \
                    say yes, so it reads the `Member` row's kind table \
                    and the receiver instead: 1.95 with the row asked per \
                    element, 0.51–0.64 without. Without the climb the \
                    gate reads the first rung high, which puts the fit \
                    under 1; the same walk timed in a loop on one tree \
                    reads 2.3, 4.8 and 9.4 ms at the three rungs. The \
                    names are distinct, so the reading is one attribute \
                    per level and the innermost: one name repeated would \
                    let a fix that asked the dedup set first read linear \
                    here and stay quadratic on distinct names.",
    },
    Probe {
        name: "npa/python-nested-foreign-targets",
        lang: LANG::Python,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Npa],
            reading: |m| m.npa.class_na_sum(),
        },
        render: python_nested_foreign_targets,
        sizes: ANCESTOR_GATE_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1867: shape control for \
                    `npa/python-nested-self-targets`: the same target \
                    nest with a foreign `db.` receiver at every level and \
                    one `self.z` innermost, so the receiver test declines \
                    every element but one before any climb. It reads \
                    0.96–1.01 with and without the row asked per element, \
                    so what the probe reads above it is the climb. The \
                    reading is the one attribute, constant by design.",
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
        name: "abc/js-nested-default-patterns",
        lang: LANG::Javascript,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.assignments_sum(),
        },
        render: js_nested_default_patterns,
        sizes: PATTERN_DEFAULT_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1868: ABC asks every `=` whether it initialises a \
                    `const` binding, and a destructuring default's `=` is \
                    answered by the first ancestor that is no pattern \
                    layer. Patterns nest without bound with a default at \
                    every level, so a climb from each default read \
                    `depth² / 2` levels. Each pattern level's answer is \
                    memoised on `LevelQuestion::InConstBinding` and read \
                    once. With the climb put back this reads 2.66, 2.1 s \
                    at the top rung against 9.3 ms, and 1.18 without. \
                    The reading is the assignment count: one per default \
                    under `let`, and the declarator's.",
    },
    Probe {
        name: "abc/js-nested-patterns",
        lang: LANG::Javascript,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.assignments_sum(),
        },
        render: js_nested_patterns,
        sizes: PATTERN_DEFAULT_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "Shape control for `abc/js-nested-default-patterns`: \
                    the same pattern nest with no default, so the one \
                    `=` is the declarator's and its parent answers. It \
                    read 0.78 with the climb per default put back and \
                    0.84 without, which attributes the other row's 2.66 \
                    to the defaults and not to the nest. The reading is \
                    that one assignment at every size; the signal is the \
                    timing.",
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
        name: "abc/perl-nested-signed-keys",
        lang: LANG::Perl,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.branches_sum(),
        },
        render: perl_nested_signed_keys,
        sizes: LINEAR_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "the ABC value peel's sign test, asked of the `-` \
                    under every `unary_expression` it steps through \
                    (#1603); `abc/perl-bareword-keys` is the width twin \
                    reaching the same call site. The peel keeps no \
                    ancestor chain, so the test is handed the `-`'s \
                    parent, which the peel holds. Perturbation: replace \
                    `perl_dash_signs_keyword_key_under(&op, node)` at \
                    `abc/perl.rs`'s `P::DASH` arm with \
                    `perl_dash_signs_keyword_key(&op, \
                    Ancestors::unknown())`, which climbs `Node::parent` \
                    per key, `O(depth)` each; this shape then fits ~2 \
                    against ~1 (2.02 at 1.15 s for 4 000 keys against \
                    1.09 at 5.9 ms on one host, 2.14 against 1.06 on a \
                    slower one). The reading is the call count, one per \
                    level.",
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
        name: "abc/perl-commented-pattern-list",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: perl_commented_pattern_list,
        sizes: COMMENTED_LIST_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1868: the pattern-role check reads the sibling after \
                    each pattern, to tell a list's last operand. A list's \
                    leading comments are its own children, outside the \
                    operands' `repeat()`, so a read that starts at the \
                    list's first child crosses all of them, once per \
                    pattern: `width²` steps for `width` comments and \
                    `width` patterns, which `abc/perl-pattern-list` cannot \
                    show with no comment in it. The read goes through the \
                    walk's `SiblingCache`. This read 1.07 and 8.9 ms at \
                    the top rung while a plain list still made both reads, \
                    and 2.08 and 0.9 s with this one back on \
                    `Node::children_after`. A plain list is no longer \
                    asked for the operands before a pattern; \
                    `abc/perl-commented-split-list` prices that read. The \
                    reading is one condition per bare match.",
    },
    Probe {
        name: "abc/perl-commented-split-list",
        lang: LANG::Perl,
        axis: Axis::Width,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.conditions_sum(),
        },
        render: perl_commented_split_list,
        sizes: COMMENTED_LIST_WIDTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1868: the list of `abc/perl-commented-pattern-list` \
                    as `split`'s arguments. A call's list is asked whether \
                    an operand precedes each pattern, which is what makes \
                    the first one the delimiter, and not for the sibling \
                    after it. Read from the list's first child, that is \
                    `width²` steps over the comments again. On a host \
                    under heavy load this read 1.07, and 2.43 \
                    with the read back on `children().find(..)`; with it \
                    on `Node::children_before`, which reads each earlier \
                    sibling by index, one walk at the top rung ran past \
                    the 20 s budget. The same two reads on the plain \
                    list, while that list still made them, read 2.02 \
                    each on a quieter host, 2.2 s and 2.5 s at the top \
                    rung against 8.9 ms. The reading is one condition per \
                    pattern but the delimiter.",
    },
    Probe {
        name: "abc/perl-negated-bareword",
        lang: LANG::Perl,
        axis: Axis::Depth,
        workload: Workload::Metrics {
            exclude_tests: false,
            selection: &[Metric::Abc],
            reading: |m| m.abc.branches_sum(),
        },
        render: perl_negated_bareword,
        sizes: ANCESTOR_GATE_DEPTHS,
        max_exponent: LINEAR_BOUND,
        rationale: "#1868: whether a bareword is an auto-quoted key is \
                    read off the token after it, found by climbing while \
                    the node is its parent's last child. Under `depth` \
                    negations that is every level, so the one bareword's \
                    climb reads `depth` ancestors: linear over the walk \
                    while each step is `O(1)` and one asker climbs the \
                    path. Perturbation: hand the climb no chain \
                    (`is_key_position(word, Ancestors::unknown())` in \
                    `perl_bareword_is_key`), so each step finds its parent \
                    from the root; this shape then read 2.09 against \
                    1.38, on a host under heavy load. The reading \
                    is the one bareword call at every depth; the signal \
                    is the timing.",
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

    use super::{
        Axis, PAREN_PREFIX_PAIRS, PROBES, Probe, RECORD_BASE_CTORS, Workload, exported_nodes,
    };

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
        // The flat root `ERROR` is the shape: its unclosed openers are
        // what keep tree-sitter from balancing the root's children.
        "loc/tcl-unclosed-continued-comments",
        "loc/elixir-unclosed-spliced-comments",
        "loc/c-unclosed-spliced-comments",
        "loc/c-unclosed-comment-rows",
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

    /// The probes that read a count *growing* with the shape — one
    /// decision per `switch` arm or guard, one operator per guard, one
    /// assignment per default, one condition per match —
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
            "npa/cpp-wide-class-members",
            "npm/ruby-nested-if-defs",
            "npm/ruby-wide-private-defs",
            "npa/ruby-nested-if-attrs",
            "loc/tcl-unclosed-continued-comments",
            "loc/cpp-nested-namespaces",
            "loc/ts-nested-fn",
            "nom/js-arrow-per-array-depth",
            "cognitive/python-and-per-list-depth",
            "abc/js-nested-default-patterns",
            "abc/perl-commented-pattern-list",
            "abc/perl-commented-split-list",
            "npa/python-nested-self-targets",
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

    /// `cognitive/else-if-ladder`'s premise, pinned on the shape rather
    /// than the timing: every `if_statement` after the first is the
    /// direct child of an `else_clause` — the one position the `ElseIf`
    /// row's parent-clause gate fires on — and the reading is
    /// `depth + 2` (the first `if` +1, each `else if` +1 as a branch
    /// extension, the final `else` +1), which a gate that stopped
    /// recognising the chain would turn into a nesting sum of order
    /// `depth²` (#1697).
    #[test]
    fn else_if_ladder_chains_under_else_clauses_and_reads_depth_plus_two() {
        let probe = PROBES
            .iter()
            .find(|p| p.name == "cognitive/else-if-ladder")
            .expect("cognitive/else-if-ladder is a probe");
        for size in [8, probe.sizes[0]] {
            let ast = parse(probe, size);
            let mut stack = vec![ast.as_tree_sitter().root_node()];
            let (mut ifs, mut chained) = (0, 0);
            while let Some(node) = stack.pop() {
                if node.kind() == "if_statement" {
                    ifs += 1;
                    if node.parent().is_some_and(|p| p.kind() == "else_clause") {
                        chained += 1;
                    }
                }
                let mut cursor = node.walk();
                stack.extend(node.children(&mut cursor));
            }
            assert_eq!(
                (ifs, chained),
                (size + 1, size),
                "N chained ifs under one head"
            );
            let reading = probe
                .workload
                .walk(&ast, probe.workload.options())
                .expect("walker must succeed");
            assert_eq!(reading, size as u64 + 2, "if +1, N else-ifs +1, else +1");
        }
    }

    /// The bare-row probes (#1445, #1801) read their bare rows: each unit's
    /// reading counts the `\`-only row with the rows around it, so a shape
    /// that lost the bare row reads one row per unit fewer — `1 × size`
    /// for the Tcl and C comment rows, `2 × size` for Elixir's, `4 × size`
    /// for Bash's code rows. The timing bound
    /// cannot tell — the shape would still grow — and nor can
    /// `probe_workload_is_exercised` or the growth check.
    #[test]
    fn bare_row_probes_read_their_bare_rows() {
        for (name, rows_per_unit) in [
            ("loc/bash-nested-continued-fn", 5),
            ("loc/tcl-wide-continued-comment", 2),
            ("loc/tcl-unclosed-continued-comments", 2),
            ("loc/elixir-unclosed-spliced-comments", 3),
            ("loc/c-unclosed-spliced-comments", 2),
            // #1818: before the index each unit's bare row read code,
            // `1 × size`.
            ("loc/c-comment-chain", 2),
            ("loc/c-unclosed-comment-rows", 2),
        ] {
            let probe = PROBES
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{name} is a probe"));
            for size in [3, probe.sizes[0]] {
                let ast = parse(probe, size);
                let reading = probe
                    .workload
                    .walk(&ast, probe.workload.options())
                    .expect("metrics walk");
                assert_eq!(reading, (rows_per_unit * size) as u64, "{name} at {size}");
            }
        }
    }

    /// The two `loc/python-long-bare-run*` probes (#1819) read their
    /// run: the `size` bare rows and the one row they join, `size + 1`,
    /// where a run credited as code — the pre-#1819 reading, or a head
    /// that stopped crediting its tail — reads 1 or less. The retained
    /// `x = 1` reaches neither `cloc` nor `blank`, so nothing but the
    /// construct can supply the reading. The head-only design itself —
    /// a later node of the run re-reading it — moves no value here or in
    /// the `loc` suite, since the classification is idempotent; the
    /// scaling bench's exponent is its only guard.
    #[test]
    fn python_bare_run_probes_read_their_run() {
        for name in ["loc/python-long-bare-run", "loc/python-long-bare-run-blank"] {
            let probe = PROBES
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{name} is a probe"));
            for size in [3, probe.sizes[0]] {
                let ast = parse(probe, size);
                let root = ast.as_tree_sitter().root_node();
                assert!(!root.has_error(), "{name}: the shape parses cleanly");
                assert_eq!(
                    count_kind(&ast, "line_continuation"),
                    size,
                    "{name}: one node per row"
                );
                let reading = probe
                    .workload
                    .walk(&ast, probe.workload.options())
                    .expect("metrics walk");
                assert_eq!(reading, size as u64 + 1, "{name} at {size}");
            }
        }
    }

    /// `loc/python-block-comments-paren-strings` (#1863) puts its
    /// comments where the block's level must scan them: tree-sitter
    /// hangs a comment ahead of a block's first statement on the
    /// compound statement, before the `block`, so the `if` holds all
    /// `size` of them and the block one statement per string. A grammar
    /// that moved them into the block would leave the probe measuring
    /// nothing. The reading is every row but the `if`.
    #[test]
    fn python_block_comments_probe_hangs_its_comments_on_the_if() {
        let name = "loc/python-block-comments-paren-strings";
        let probe = PROBES
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("{name} is a probe"));
        for size in [3, probe.sizes[0]] {
            let ast = parse(probe, size);
            let root = ast.as_tree_sitter().root_node();
            assert!(!root.has_error(), "{name} at {size}");
            let compound = root.child(0).expect("the `if`");
            assert_eq!(compound.kind(), "if_statement");
            let mut cursor = compound.walk();
            let kinds: Vec<_> = compound.children(&mut cursor).map(|c| c.kind()).collect();
            assert_eq!(
                kinds.iter().filter(|kind| **kind == "comment").count(),
                size,
                "every comment is the `if`'s child"
            );
            assert_eq!(kinds.last(), Some(&"block"), "and sits before the block");
            assert_eq!(count_kind(&ast, "parenthesized_expression"), size);
            assert_eq!(count_kind(&ast, "line_continuation"), size);
            let reading = probe
                .workload
                .walk(&ast, probe.workload.options())
                .expect("metrics walk");
            // expected: a row per comment, and the string's, the bare
            // row and the `)` row per statement.
            assert_eq!(reading, 4 * size as u64, "{name} at {size}");
        }
    }

    /// `loc/python-deep-paren-extra-prefix` (#1863) keeps every
    /// parenthesis as wide as a node can be before the walk records its
    /// children for sibling lookups, so each level's read is a seek and
    /// the count of levels read shows in the timing. One pair more and
    /// the levels are served from the sibling cache: the probe would
    /// still fit, and would read about 1.6 with the climb put back
    /// where it reads about 2. The threshold is the walk's `WIDE_PARENT`, read from
    /// the AST crate so a raised or lowered threshold fails here rather
    /// than weakening the probe unseen. The reading is every row but
    /// the retained `x = 1`.
    #[test]
    fn python_deep_paren_probe_levels_stay_under_the_sibling_cache() {
        use big_code_analysis_ast::node::WIDE_PARENT;
        let name = "loc/python-deep-paren-extra-prefix";
        let probe = PROBES
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("{name} is a probe"));
        for size in [3, probe.sizes[0]] {
            let ast = parse(probe, size);
            assert_eq!(
                count_kind(&ast, "parenthesized_expression"),
                size,
                "one parenthesis per level"
            );
            // expected: `(`, two nodes per pair, the next level, `)`.
            let widest = 2 * PAREN_PREFIX_PAIRS + 3;
            assert_eq!(max_child_count(&ast), widest, "{name} at {size}");
            assert!(
                widest <= WIDE_PARENT && widest + 2 > WIDE_PARENT,
                "{widest} children per parenthesis must be the most that stays \
                 under WIDE_PARENT ({WIDE_PARENT})"
            );
            let reading = probe
                .workload
                .walk(&ast, probe.workload.options())
                .expect("metrics walk");
            // expected: per level the `(` row and a bare row and a
            // comment row per pair; then the string's row and the `)` row.
            assert_eq!(
                reading,
                ((2 * PAREN_PREFIX_PAIRS + 1) * size + 2) as u64,
                "{name} at {size}"
            );
        }
    }

    /// The two `record-alternative-ctors` probes (#1863) read every
    /// list: ten arguments for each of the `size` constructors and the
    /// base ones, and ten for the header, which stays open because
    /// every constructor is an alternative. A constructor read as
    /// canonical closes the header and drops its ten; a header gate
    /// that stopped scanning leaves the reading unchanged, which is why
    /// the node count is pinned beside it: the scan is priced by the
    /// constructors the body really holds.
    #[test]
    fn record_alternative_ctor_probes_read_every_list() {
        for name in [
            "nargs/java-record-alternative-ctors",
            "nargs/groovy-record-alternative-ctors",
        ] {
            let probe = PROBES
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{name} is a probe"));
            for size in [3, probe.sizes[0]] {
                let ast = parse(probe, size);
                let constructors = size + RECORD_BASE_CTORS;
                assert_eq!(
                    count_kind(&ast, "constructor_declaration"),
                    constructors,
                    "{name}: one constructor per unit of width, and the base"
                );
                let reading = probe
                    .workload
                    .walk(&ast, probe.workload.options())
                    .expect("metrics walk");
                // expected: 10 per constructor, plus the header's 10.
                assert_eq!(reading, 10 * (constructors as u64 + 1), "{name} at {size}");
            }
        }
    }

    /// A probe of `size` one-`line_continuation` runs under one
    /// parenthesis reads `reading(size)`: the two tests below share
    /// the shape and differ in the formula.
    fn python_parenthesised_runs_probe_reads(name: &str, reading: impl Fn(u64) -> u64) {
        let probe = PROBES
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("{name} is a probe"));
        for size in [3, probe.sizes[0]] {
            let ast = parse(probe, size);
            let root = ast.as_tree_sitter().root_node();
            assert!(!root.has_error(), "the shape parses cleanly at {size}");
            assert_eq!(
                count_kind(&ast, "line_continuation"),
                size,
                "one node per run at {size}"
            );
            assert_eq!(
                count_kind(&ast, "parenthesized_expression"),
                1,
                "every run shares one parenthesis at {size}"
            );
            let got = probe
                .workload
                .walk(&ast, probe.workload.options())
                .expect("metrics walk");
            assert_eq!(got, reading(size as u64), "{name} at {size}");
        }
    }

    /// `loc/python-separated-bare-runs` (fourth re-review of #1819)
    /// reads its runs: `size` separate runs, one `line_continuation`
    /// each, and the reading is the string's row, the `)`'s and the
    /// `size` bare rows — `size + 2` comment rows, every one of the
    /// `size` from the runs, which `GapScan`-style forward reading
    /// would file as blank (each joins an empty row). The retained
    /// `x = 1` reaches `cloc` on no reading. The scaling bench's
    /// exponent is the only guard of the per-parenthesis memo itself,
    /// since asking per run answers the same.
    #[test]
    fn python_separated_bare_runs_probe_reads_its_runs() {
        python_parenthesised_runs_probe_reads("loc/python-separated-bare-runs", |size| size + 2);
    }

    /// `loc/python-bare-runs-before-string` (#1830) reads its runs:
    /// `size` runs of one `line_continuation` each, all before the
    /// string under one parenthesis, and the reading is the `(` row,
    /// the string's, the `)`'s, the `size` comment rows and the `size`
    /// bare rows — `2 × size + 3` comment rows. Before #1830 the
    /// parenthesis was not the string's, so its two rows were code and
    /// the reading `2 × size + 1` (measured 7 at 3): each bare row
    /// joined its comment and was a comment row already, by the forward
    /// reading a run takes outside a bare string's parentheses, so what
    /// the runs pin here is that each one reaches the memoised question
    /// — the bench's exponent, not this count, is what the memo guards.
    /// The retained `x = 1` reaches `cloc` on no reading.
    #[test]
    fn python_bare_runs_before_string_probe_reads_its_runs() {
        python_parenthesised_runs_probe_reads("loc/python-bare-runs-before-string", |size| {
            2 * size + 3
        });
    }

    /// `cognitive/elixir-keyword-sections`: the two `do:`-bearing lists
    /// score one `else:` section per pair each and the third nothing, so
    /// the reading is exactly `2 × width` over three calls. The timing
    /// bound catches a per-pair scan; this catches the gate reading the
    /// `do:` from the wrong list (#1602).
    #[test]
    fn elixir_keyword_sections_read_two_per_width() {
        let probe = PROBES
            .iter()
            .find(|p| p.name == "cognitive/elixir-keyword-sections")
            .expect("cognitive/elixir-keyword-sections is a probe");
        for size in [3, probe.sizes[0]] {
            let ast = parse(probe, size);
            let root = ast.as_tree_sitter().root_node();
            assert!(!root.has_error(), "the shape parses cleanly");
            assert_eq!(count_kind(&ast, "call"), 3);
            let reading = probe
                .workload
                .walk(&ast, probe.workload.options())
                .expect("metrics walk");
            assert_eq!(reading, (2 * size) as u64);
        }
    }

    /// `cognitive/elixir-rejected-keyword-list`: `width` comments sit in
    /// `foo`'s `arguments` *between* the bracketed list and the trailing
    /// operand — the siblings the trailing-argument check scans — and
    /// `width` more inside `bar`'s list *before* its pairs — the siblings
    /// the leading check scans. The reading is `width` retained `if`s
    /// plus `bar`'s `width` `else:` sections: `foo`'s list contributes
    /// nothing because the operand makes it no section list. The timing
    /// bound catches either per-pair rescan; this catches the comments
    /// sitting where no scan reaches them, `foo`'s list being read as
    /// sections (`3 × width`), or `bar`'s not (`width`) (#1602 review,
    /// #1697 cost audit).
    #[test]
    fn elixir_rejected_keyword_list_is_scanned_past_its_comments() {
        let probe = PROBES
            .iter()
            .find(|p| p.name == "cognitive/elixir-rejected-keyword-list")
            .expect("cognitive/elixir-rejected-keyword-list is a probe");
        for size in [3, probe.sizes[0]] {
            let ast = parse(probe, size);
            let root = ast.as_tree_sitter().root_node();
            assert!(!root.has_error(), "the shape parses cleanly");
            let mut stack = vec![root];
            let (mut comments, mut between_list_and_operand, mut before_the_pairs) = (0, 0, 0);
            while let Some(node) = stack.pop() {
                if node.kind() == "comment" {
                    comments += 1;
                    let parent = node.parent().expect("a comment has a parent");
                    if parent.kind() == "arguments"
                        && node
                            .prev_named_sibling()
                            .is_some_and(|prev| matches!(prev.kind(), "list" | "comment"))
                        && node.next_named_sibling().is_some()
                    {
                        between_list_and_operand += 1;
                    }
                    if parent.kind() == "list"
                        && node
                            .next_named_sibling()
                            .is_some_and(|next| matches!(next.kind(), "keywords" | "comment"))
                    {
                        before_the_pairs += 1;
                    }
                }
                let mut cursor = node.walk();
                stack.extend(node.children(&mut cursor));
            }
            assert_eq!(
                (comments, between_list_and_operand, before_the_pairs),
                (2 * size, size, size)
            );
            // Two lists of `2 × width` pairs plus the `if`s' `do:` pairs.
            assert_eq!(count_kind(&ast, "pair"), 5 * size);
            let reading = probe
                .workload
                .walk(&ast, probe.workload.options())
                .expect("metrics walk");
            assert_eq!(reading, 2 * size as u64);
        }
    }

    // Named nodes only: a grammar may give a keyword token the kind of
    // the node it opens, as tree-sitter-ruby does `class`.
    fn count_kind(ast: &Ast, kind: &str) -> usize {
        let mut stack = vec![ast.as_tree_sitter().root_node()];
        let mut n = 0;
        while let Some(node) = stack.pop() {
            n += usize::from(node.is_named() && node.kind() == kind);
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
        n
    }

    /// The three `ruby-nested-classes` probes share one shape and
    /// ladder, which parses into exactly one named `class` per level at
    /// every rung, not only at the first, where the generic checks
    /// parse: a parser limit reached at scale would leave the top rungs
    /// timing error recovery under a plausible exponent. Each level holds
    /// one attribute, one method and four physical rows (#1805), read
    /// exactly at a small size and at the first rung as the sibling probe
    /// tests read theirs; the scaling gate prints the reading at every
    /// rung. Walking every probe at every rung cost several seconds of an
    /// unoptimised test run for no further guarantee.
    #[test]
    fn ruby_nested_classes_probes_parse_one_class_per_level_at_every_rung() {
        let probes: Vec<(&Probe, u64)> = [
            ("npa/ruby-nested-classes", 1),
            ("npm/ruby-nested-classes", 1),
            ("loc/ruby-nested-classes", 4),
        ]
        .into_iter()
        .map(|(name, per_level)| {
            let probe = PROBES
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{name} is a probe"));
            (probe, per_level)
        })
        .collect();
        let (first, _) = probes[0];
        for (probe, _) in &probes {
            assert_eq!(
                (probe.lang, (probe.render)(3), probe.sizes),
                (first.lang, (first.render)(3), first.sizes),
                "{}: the probes must share one shape and ladder",
                probe.name
            );
        }
        for size in first.sizes {
            let ast = parse(first, size);
            let root = ast.as_tree_sitter().root_node();
            assert!(!root.has_error(), "the shape parses cleanly at {size}");
            assert_eq!(
                count_kind(&ast, "class"),
                size,
                "one class per level at {size}"
            );
        }
        for size in [3, first.sizes[0]] {
            let ast = parse(first, size);
            for (probe, per_level) in &probes {
                let reading = probe
                    .workload
                    .walk(&ast, probe.workload.options())
                    .expect("metrics walk");
                assert_eq!(reading, per_level * size as u64, "{} at {size}", probe.name);
            }
        }
    }

    /// The three `nested-classes` probes of #1864 share one source and
    /// ladder, which parses into exactly one class holding one method
    /// per level at every rung under both grammars: a parser limit
    /// reached at scale would leave the top rungs timing error recovery
    /// under a plausible exponent, and a grammar that stopped reading
    /// `void m(…) { … }` as a method would leave the Groovy row, which
    /// has no `nom` control, timing a walk that never asks the question.
    /// The readings are exact at a small size and at the first rung: one
    /// `if` per level at no surcharge for the two cognitive rows, and one
    /// method per level for `nom`.
    #[test]
    fn nested_classes_probes_parse_one_class_per_level_at_every_rung() {
        let [java, nom, groovy] = [
            "cognitive/java-nested-classes",
            "nom/java-nested-classes",
            "cognitive/groovy-nested-classes",
        ]
        .map(|name| {
            PROBES
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{name} is a probe"))
        });
        assert_eq!(
            (java.lang, groovy.lang, nom.lang),
            (
                big_code_analysis::LANG::Java,
                big_code_analysis::LANG::Groovy,
                big_code_analysis::LANG::Java
            ),
            "one cognitive row per grammar, and the control on Java's"
        );
        for probe in [nom, groovy] {
            assert_eq!(
                ((probe.render)(3), probe.sizes),
                ((java.render)(3), java.sizes),
                "{}: the probes must share one source and ladder",
                probe.name
            );
        }
        for probe in [java, groovy] {
            for size in probe.sizes {
                let ast = parse(probe, size);
                let root = ast.as_tree_sitter().root_node();
                assert!(
                    !root.has_error(),
                    "{}: the shape parses cleanly at {size}",
                    probe.name
                );
                assert_eq!(
                    (
                        count_kind(&ast, "class_declaration"),
                        count_kind(&ast, "method_declaration")
                    ),
                    (size, size),
                    "{}: one class and one method per level at {size}",
                    probe.name
                );
            }
        }
        for probe in [java, nom, groovy] {
            for size in [3, probe.sizes[0]] {
                let reading = probe
                    .workload
                    .walk(&parse(probe, size), probe.workload.options())
                    .expect("metrics walk");
                assert_eq!(reading, size as u64, "{} at {size}", probe.name);
            }
        }
    }

    /// `npa/python-nested-self-targets` and its control are one nest: a
    /// clean parse with one `tuple_pattern` per level at every rung, and
    /// two renders that differ in the receiver and the innermost name
    /// alone. The probe binds a distinct `self` attribute at each level
    /// and the innermost, the control only its innermost `self.z`, so
    /// the control prices the same target walk with nothing for the
    /// attribute count to collect (#1867). A control given the probe's
    /// receiver, or a probe whose names repeat and are absorbed by the
    /// count's dedup, reads differently here and nowhere else.
    ///
    /// expected: `size + 1` attributes on the probe and 1 on the control,
    /// at a small size and at the first rung.
    #[test]
    fn python_nested_target_probes_share_a_nest_and_differ_in_the_receiver() {
        let [probe, control] = [
            "npa/python-nested-self-targets",
            "npa/python-nested-foreign-targets",
        ]
        .map(|name| {
            PROBES
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{name} is a probe"))
        });
        assert_eq!(probe.sizes, control.sizes, "one ladder");
        assert_eq!(
            (probe.render)(3)
                .replace("self.a", "db.a")
                .replace("db.a00003", "self.z"),
            (control.render)(3),
            "the control is the probe's nest with a foreign receiver and its own innermost name"
        );
        for row in [probe, control] {
            for size in row.sizes {
                let ast = parse(row, size);
                assert!(
                    !ast.as_tree_sitter().root_node().has_error(),
                    "{}: the shape parses cleanly at {size}",
                    row.name
                );
                assert_eq!(
                    count_kind(&ast, "tuple_pattern"),
                    size,
                    "{}: one pattern per level at {size}",
                    row.name
                );
            }
        }
        for size in [3, probe.sizes[0]] {
            let read = |row: &Probe| {
                row.workload
                    .walk(&parse(row, size), row.workload.options())
                    .expect("metrics walk")
            };
            assert_eq!(read(probe), size as u64 + 1, "the probe at {size}");
            assert_eq!(read(control), 1, "the control at {size}");
        }
    }

    /// `cognitive/python-wide-if-siblings`: every `if` is a direct child of
    /// the function's block (one `block` wider than `WIDE_PARENT`), none
    /// is under an `else_clause`, and the reading is exactly `width` —
    /// one +1 per sibling `if` and no nesting. The timing bound catches
    /// the quadratic scan; this catches the gate answering `true` for a
    /// sibling (#1697).
    #[test]
    fn python_wide_if_siblings_are_flat_and_read_one_each() {
        let probe = PROBES
            .iter()
            .find(|p| p.name == "cognitive/python-wide-if-siblings")
            .expect("cognitive/python-wide-if-siblings is a probe");
        for size in [65, probe.sizes[0]] {
            let ast = parse(probe, size);
            let root = ast.as_tree_sitter().root_node();
            let mut stack = vec![root];
            let (mut ifs, mut under_block, mut under_else) = (0, 0, 0);
            while let Some(node) = stack.pop() {
                if node.kind() == "if_statement" {
                    ifs += 1;
                    match node.parent().map(|p| p.kind()) {
                        Some("block") => under_block += 1,
                        Some("else_clause") => under_else += 1,
                        _ => {}
                    }
                }
                let mut cursor = node.walk();
                stack.extend(node.children(&mut cursor));
            }
            assert_eq!((ifs, under_block, under_else), (size, size, 0));
            assert!(
                max_child_count(&ast) >= size,
                "the def's block holds every if"
            );
            let reading = probe
                .workload
                .walk(&ast, probe.workload.options())
                .expect("walker must succeed");
            assert_eq!(reading, size as u64, "+1 per sibling if, no nesting");
        }
    }

    /// `cognitive/wide-if-siblings` makes two structural claims its
    /// rationale rests on, pinned here on the shape rather than on the
    /// timing: the method `block` is wider than the walk's sibling-cache
    /// threshold at the smallest size, so the outer `if`s really do
    /// answer through the cache; and the reading is `3 × width`, which
    /// is `4 × width` the moment the else-if gate stops recognising the
    /// chained `if` and the walk nests it (#1697).
    ///
    /// The threshold is the walk's `WIDE_PARENT`, read from the AST
    /// crate rather than hand-copied, as its doc asks.
    #[test]
    fn wide_if_siblings_is_wide_and_reads_three_per_statement() {
        use big_code_analysis_ast::node::WIDE_PARENT;
        let probe = PROBES
            .iter()
            .find(|p| p.name == "cognitive/wide-if-siblings")
            .expect("cognitive/wide-if-siblings is a probe");
        for size in [WIDE_PARENT + 1, probe.sizes[0]] {
            let ast = parse(probe, size);
            // The method body is the one wide node, and the `if`s are its
            // direct children: `{`, N `if_statement`s, `}`. Each chained
            // `if` is a child of its outer `if_statement` with four
            // siblings (`if`, condition, block, `else`), so the inner
            // lookups take the narrow scan.
            let root = ast.as_tree_sitter().root_node();
            let mut stack = vec![root];
            let mut blocks = Vec::new();
            let mut inner_parent_widths = Vec::new();
            while let Some(node) = stack.pop() {
                if node.kind() == "block"
                    && node
                        .parent()
                        .is_some_and(|p| p.kind() == "method_declaration")
                {
                    blocks.push(node);
                }
                if node.kind() == "if_statement"
                    && let Some(parent) = node.parent()
                    && parent.kind() == "if_statement"
                {
                    inner_parent_widths.push(parent.child_count());
                }
                let mut cursor = node.walk();
                stack.extend(node.children(&mut cursor));
            }
            let [block] = blocks[..] else {
                panic!("one method body, found {}", blocks.len());
            };
            assert_eq!(block.child_count(), size + 2, "`{{`, N ifs, `}}`");
            assert!(block.child_count() > WIDE_PARENT, "the block must be wide");
            let mut cursor = block.walk();
            let outer = block
                .children(&mut cursor)
                .filter(|c| c.kind() == "if_statement")
                .count();
            assert_eq!(outer, size, "every outer if is a direct child of the block");
            assert_eq!(
                inner_parent_widths.len(),
                size,
                "one chained if per statement"
            );
            assert!(
                inner_parent_widths.iter().all(|&w| w == 5),
                "a chained if's parent has five children"
            );
            let reading = probe
                .workload
                .walk(&ast, probe.workload.options())
                .expect("walker must succeed");
            assert_eq!(
                reading,
                3 * size as u64,
                "outer if +1, else if +1, else +1 per statement"
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

    /// `find/tcl-nested-braces` renders one line per match — the
    /// `count` control's reading, which is one match per brace level —
    /// at every rung. [`probe_workload_is_exercised`] would pass a
    /// renderer that wrote one line in total, and the timing would then
    /// price nothing but the walk the control already prices (#1416).
    #[test]
    fn the_find_probe_renders_one_line_per_match() {
        let probe = |name: &str| {
            PROBES
                .iter()
                .find(|probe| probe.name == name)
                .unwrap_or_else(|| panic!("{name} is a probe"))
        };
        let (find, count) = (
            probe("find/tcl-nested-braces"),
            probe("count/tcl-nested-braces"),
        );
        assert_eq!(find.sizes, count.sizes);
        for size in find.sizes {
            let ast = parse(find, size);
            let read = |probe: &Probe| probe.workload.walk(&ast, probe.workload.options());
            let lines = read(find).expect("find walks");
            assert_eq!(lines, read(count).expect("count walks"), "at depth {size}");
            assert_eq!(lines, u64::try_from(size).expect("a small size"));
        }
    }

    /// The wide find probe renders one line per match, and its words sit
    /// past the whole-printed levels: the innermost brace prints a `…` per
    /// word. A shape that never reached the elision would time only the
    /// whole-span path, which the depth probe already prices.
    #[test]
    fn the_wide_find_probe_elides_every_word() {
        let probe = PROBES
            .iter()
            .find(|probe| probe.name == "find/tcl-wide-braces-past-the-limit")
            .expect("the wide find probe exists");
        let width = 10;
        let ast = parse(probe, width);
        let lines = probe
            .workload
            .walk(&ast, probe.workload.options())
            .expect("find walks");
        // 66 braces around the words, and each word is two.
        assert_eq!(lines, u64::try_from(66 + 2 * width).expect("small"));
        let found = ast.find(&["braced_word".to_owned()]).expect("find walks");
        let mut out = Vec::new();
        big_code_analysis::write_found(&mut out, &ast, &found, ..).expect("render to memory");
        let text = String::from_utf8(out).expect("utf-8 fixture");
        let innermost = text.lines().nth(65).expect("the innermost brace's line");
        assert_eq!(innermost.matches('…').count(), width, "{innermost}");
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
