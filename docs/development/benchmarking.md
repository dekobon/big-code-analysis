# Benchmarking the metric walk

`big-code-analysis-bench` is the benchmark harness for the metric
walk. It is for contributors changing AST traversal, a `Checker` or
`Getter` predicate, or a metric's `compute`. It exists so that a
performance claim about this project can be reproduced by someone who
did not make it, and so that a change in *complexity class* fails
loudly instead of quietly making deeply nested files unanalysable.

It has two halves, and they answer different questions.

- **The complexity-class gate** (`benches/scaling.rs`) answers "does
  doubling the input (its nesting depth, or one parent's child count)
  roughly double the cost?" It fails the process when a probe's
  measured exponent exceeds the bound that probe declares.
- **The criterion benchmarks** (`benches/metric_walk.rs`) answer "how
  fast is this, and did my change help?" They use
  [criterion][criterion], which reports a confidence interval rather
  than a point estimate and can compare two builds against a saved
  baseline.

Neither runs in per-PR CI. See [Why not in CI](#why-not-in-ci). The
first two sections cover running the gate and reading its output. The
per-probe histories after them record why each probe exists and what
it was measured catching; read those when a probe fails or when adding
one.

[criterion]: https://bheisler.github.io/criterion.rs/book/index.html

## Running it

```bash
make bench-scaling                       # complexity-class gate
make bench-walk                          # criterion measurements
make bench                               # both
```

Or directly, which is what you want when passing criterion flags:

```bash
cargo bench -p big-code-analysis-bench --bench scaling
cargo bench -p big-code-analysis-bench --bench scaling -- --rounds 11
cargo bench -p big-code-analysis-bench --bench metric_walk
```

The crate is a workspace member but not a default member, so a bare
`cargo build` at the repo root does not build it or pull in criterion.

`cargo test --benches` also executes both targets, because a
`harness = false` bench target runs its `main`. The gate detects that
(cargo passes `--bench` only under `cargo bench`) and downgrades to a
shallow smoke run with no verdict, so an unoptimised build never
produces a number or a failure.

The criterion benchmarks read a slice of the corpus submodules under
`tests/repositories/`. Without them the corpus group is skipped with a
message and the synthetic shapes still run:

```bash
git submodule update --init --recursive
```

## The complexity-class gate {#complexity-gate}

Each probe pairs a generated source shape with the workload that
exercises one hot path: a metric selection through `Ast::metrics`,
the operator/operand walk behind `Ast::ops`, or a `--type` filter
through `Ast::count`. The shape is rendered at three doubling
sizes, every (probe, size) cell is measured once per round with the
visit order rotated between rounds, and the reported figure is the
slope of `ln(time)` against `ln(size)`. A linear walk sits near 1.0;
a quadratic one sits near 2.0.

The timer starts after the parse, so a probe prices the walk and
never the grammar. A scanner that is itself super-linear on a shape
shows in no exponent here: tree-sitter-python's external scanner is
quadratic in a run of comment rows inside a block and in a run of bare
`\` rows, so `bca metrics` on the shape
`loc/python-block-comments-paren-strings` renders is dominated by the
parse at the top rung while the probe fits 1.00. Time `bca count -t
error` on the same file to see the parse alone.

A probe declares which **axis** its size parameter grows along, because
a walk can be linear in one and quadratic in the other:

- `Axis::Depth`: the size is the shape's nesting depth. Most probes
  are on this axis, and they exist because
  [tree-sitter](https://tree-sitter.github.io/tree-sitter/) stores no
  parent pointer, so any predicate that resolves an ancestor by
  climbing is `O(depth)` per node.
- `Axis::Width`: the size is the number of siblings under one parent,
  at a depth that does not move. `nom/wide-attributed-fn` exists
  because #1100's fix trades the two axes against each other, and
  `halstead/wide-distinct-fn` because per-child work at a space-merge
  boundary is priced by the parent's child count (both under
  [Why each probe exists](#why-each-probe-exists)).

The axis also selects the shape invariant a probe must satisfy in
`shapes.rs`: a depth shape has to gain AST levels in proportion to its
size, and a width shape has to gain children while its AST depth stays
**constant**. That second half is what stops a "width" probe that
quietly began nesting from reporting a healthy exponent for a bound
nothing is testing any more.

Output looks like this:

```text
cognitive/nested-while  exponent 1.15 (bound 1.50)  ok
     size      bytes   median ms      min ms      max ms    ns/byte   iter    reading
     1000      14017       0.759       0.723       0.771      54.17      2     500500
     2000      28017       1.792       1.658       1.834      63.98      1    2001000
     4000      56017       3.723       3.425       3.838      66.46      1    8002000
```

Read it as follows.

- `median ms` is what the fit uses. `min` and `max` bracket the
  rounds. A wide spread means the host was busy and the run should be
  repeated, not interpreted.
- `bytes` grows linearly with `size` for every shape, by
  construction. If it did not, a walk that is linear in input size
  would read as superlinear in its size parameter.
- `ns/byte` drifts up with size on a depth shape even for a linear
  walk, because the tree outgrows cache. That drift is why the linear
  bound is 1.5 and not 1.05. The width shape shows no such drift
  (`nom/wide-attributed-fn` holds ~180 across its ladder), which is
  observed rather than explained; do not read a cache argument into
  it.
- `iter` is how many walks were folded into one timed sample. The
  cheapest cells run in a few hundred microseconds, where clock
  resolution is a visible fraction of the reading, so the harness
  repeats them until a sample reaches a millisecond.
- `reading` is the headline value the walk produced. It is there so a
  probe that stopped measuring anything is visible: a shape paired with
  a workload that scores zero on it would time the walk's fixed
  overhead and report an excellent exponent forever. A reading that
  does not grow with the size is fine as long as it is non-zero, since
  the scaling signal lives in the timing column, but say so in the
  probe's `rationale`, as `ops/nested-fn` does.

### What the probes cover

| Probe | Language | Axis | Hot path | Class today |
|---|---|---|---|---|
| `tokens/nested-paren` | Rust | depth | inherited in-comment flag (#1052) | linear |
| `cognitive/nested-while` | C | depth | `get_nesting_from_map` (#1062) | linear |
| `nom/nested-while` | C | depth | metric control for the row above | linear |
| `cognitive/nested-if` | C | depth | `Checker::is_else_if` | linear |
| `cognitive/else-if-ladder` | C | depth | the `ElseIf` row's parent-clause gate on an `else if` chain (#1697) | linear |
| `cognitive/wide-if-siblings` | Java | width | the `ElseIf` row's previous-sibling gate through the `SiblingCache` (#1697) | linear |
| `cognitive/python-wide-if-siblings` | Python | width | the `ElseIf` row's sole-child gate reading its parents before scanning the block (#1697) | linear |
| `loc/nested-while` | C | depth | shape control for the row below | linear |
| `loc/nested-declaration` | C | depth | the declaration's loop-header question, its stop one step up | linear |
| `loc/cpp-nested-namespaces` | C++ | depth | the same question with no stop below the root, folded on `GateLane::InsideHeader` (#1767) | linear |
| `loc/ts-nested-fn` | TypeScript | depth | the ambient-declaration question, which has no stop, folded on `GateLane::InsideAmbient` (#1767) | linear |
| `nom/nested-quote` | Elixir | depth | `elixir_is_inside_quote_block` | linear |
| `nom/nested-fn` | Rust | depth | `FuncSpace` nesting; metric control for the row below | linear |
| `cognitive/nested-fn` | Rust | depth | `increment_function_depth` (#1062) | linear |
| `ops/nested-fn` | Rust | depth | the `Ast::ops` walk (#1110) | linear |
| `loc/nested-fn` | Rust | depth | shape control for the row below | linear |
| `loc/nested-fn-rows` | Rust | depth | `Ploc::merge` / `Cloc::merge` row-set union (#1109) | linear |
| `nom/nested-fn-rows` | Rust | depth | metric control for the row above | linear |
| `nom/nested-declared-function` | JavaScript | depth | shape control for the row below | linear |
| `nom/nested-arrow` | JavaScript | depth | JS-family `is_func_with_code` / `is_closure` (#1088) | linear |
| `nom/js-arrow-per-array-depth` | JavaScript | depth | the same binding question with no frame below the root, folded on `FoldLane::NamedBinding` (#1767) | linear |
| `cognitive/python-and-per-list-depth` | Python | depth | a boolean operator's "inside another chain" and enclosing-`lambda` questions, with no stop in a list, folded on `GateLane::InsideBooleanChain` and `FoldLane::LambdaDepth` (#1767) | linear |
| `nom/wide-arrow-array` | JavaScript | width | the arrow binding gate's parent sibling scan on `run(() => 1, …)`, memoised per parent under `MemoQuestion::NamesAnArrow` (#1696) | linear |
| `halstead/nested-paren` | Rust | depth | shape control for the row below | linear |
| `halstead/nested-not` | Rust | depth | `Getter::get_op_type`'s parent read (#1096) | linear |
| `abc/nested-block` | C | depth | shape control for the row below | linear |
| `abc/nested-if` | C | depth | the C-family ABC container walker (#1096) | linear |
| `cyclomatic/nested-and` | Python | depth | shape control for the row below | linear |
| `cyclomatic/nested-ternary` | Python | depth | `Node::parent_grandparent_match` (#1096) | linear |
| `loc/nested-quote` | Elixir | depth | `loc`'s Elixir catch-all arm (#1096) | linear |
| `nom/nested-attributed-fn` | Rust | depth | the `exclude_tests` outer-attribute scan (#1100) | linear |
| `nom/wide-attributed-fn` | Rust | width | the same scan on the width axis (#1100) | linear |
| `nom/deep-attribute-run` | Rust | depth | the same scan on the diagonal: depth and one attribute run together (#1446) | linear |
| `nom/deep-one-attribute-items` | Rust | depth | the same diagonal with many one-attribute items on one deep parent (#1447) | linear |
| `nom/deep-doc-comment-run` | Rust | depth | the same diagonal with the run spelled as doc and plain comment rows (#1647) | linear |
| `nom/nested-cfg-predicate` | Rust | depth | the `cfg(...)` predicate classifier (#1105) | linear |
| `halstead/wide-distinct-fn` | Rust | width | per-child work at the space-merge boundary (#1106) | linear |
| `abc/wide-let-declarators` | JavaScript | width | ABC's structural `const` check per `let` declarator (#1277) | linear |
| `abc/js-nested-default-patterns` | JavaScript | depth | the same check asked by a default's `=` at every level of a pattern nest, where the first ancestor that is no pattern answers; each pattern level's answer is memoised on `LevelQuestion::InConstBinding` (#1868) | linear |
| `abc/js-nested-patterns` | JavaScript | depth | shape control for the row above: the nest with its defaults removed | linear |
| `loc/wide-cfg-test-mod` | Rust | width | the `exclude_tests` prune arm's row bookkeeping (#1417) | linear |
| `loc/go-comment-run` | Go | width | `leading_doc_run`'s walk back over a lead comment run (#1597) | linear |
| `loc/rust-nested-doc-mods` | Rust | depth | the in-body doc test, memoised per ancestor by `Ancestors::fold_down` (#1597) | linear |
| `loc/rust-inner-doc-run` | Rust | width | the inner-doc test of each `//!` opening a function body, with no prefix scan | linear |
| `loc/java-dangling-docs` | Java | width | `bound_doc_run`'s once-per-stretch decision over dangling [Javadoc](https://docs.oracle.com/en/java/javase/21/javadoc/javadoc.html) (#1613) | linear |
| `loc/java-package-docs` | Java | width | the package Javadoc's look-ahead for a type, stopped at the next package (#1613) | linear |
| `nargs/java-record-alternative-ctors` | Java | width | the record-header gate's scan of the body for a canonical constructor, told by its parameter types: one scan per consumer of the gate, never one per constructor (#1863) | linear |
| `nargs/groovy-record-alternative-ctors` | Groovy | width | the same gate on the same source under the Groovy grammar (#1863) | linear |
| `loc/rust-attributed-docs` | Rust | width | the same decision through the attributes between `///` comments (#1613) | linear |
| `loc/csharp-dangling-docs` | C# | width | [Roslyn](https://github.com/dotnet/roslyn)'s reverse trivia scan over doc comments split by plain ones (#1613) | linear |
| `loc/php-file-docblock` | PHP | width | [phpDocumentor](https://www.phpdoc.org/)'s file-DocBlock test, a forward scan from the first DocBlock only (#1613) | linear |
| `loc/js-standalone-tags` | JavaScript | width | [JSDoc](https://jsdoc.app/)'s virtual-comment test on each `@file` block (#1613) | linear |
| `loc/c-dangling-docs` | C | width | [Doxygen](https://www.doxygen.nl/)'s stretch over doc blocks alternating with the `#include`s it reads past (#1613) | linear |
| `loc/cpp-structural-docs` | C++ | width | the `@file` scan and the gathering walk of a stretch ending at no target (#1613) | linear |
| `loc/c-nested-conditional-docs` | C | depth | Doxygen's "does this conditional hold an entry" test, memoised per conditional for the walk (#1613) | linear |
| `loc/python-additional-docstrings` | Python | width | additional docstrings classified once per consecutive run | linear |
| `loc/csharp-accessor-docs` | C# | width | the in-body doc test of each comment in a property's accessor list (#1612) | linear |
| `loc/php-signature-docs` | PHP | width | the in-body doc test of each docblock between a signature and its body (#1612) | linear |
| `loc/kotlin-signature-docs` | Kotlin | width | the same test for [KDoc](https://kotlinlang.org/docs/kotlin-doc.html) in a Kotlin signature (#1612) | linear |
| `halstead/python-nested-paren` | Python | depth | the climb from each parenthesis to its statement, capped at CPython's nesting limit (#1610) | linear |
| `loc/python-paren-bare-strings` | Python | depth | each parenthesis of a bare string statement asking whether it wraps the string, memoised per statement under the cap (#1610) | linear |
| `ops/python-paren-bare-strings` | Python | depth | the same test in the `bca ops` walk, which keeps its own fold cache (#1610) | linear |
| `loc/python-deep-paren-extra-prefix` | Python | depth | each parenthesis of a parenthesised docstring asking whether code precedes it on its logical line, where every level defers to the one above; each level's nearest answer is memoised on `LevelQuestion::CodeBeforeOnLine` (#1863) | linear |
| `loc/python-block-comments-paren-strings` | Python | width | the same question where each asker's own statement answers and the block above it is a long scan: the levels above the nearest answer are never read (#1863) | linear |
| `loc/go-nested-doc-fields` | Go | depth | Go's package-level scope test over nested struct fields (#1597) | linear |
| `loc/ruby-nested-doc-constants` | Ruby | depth | YARD's namespace scope test through nested `if`s (#1597) | linear |
| `loc/lua-tagless-blocks` | Lua | width | LDoc's first-block test, scanning back to the previous block (#1597) | linear |
| `loc/perl-data-pod` | Perl | width | the one-pass POD scan of the data section after `__END__` (#1608) | linear |
| `loc/bash-nested-continued-fn` | Bash | depth | the bare-`\`-row gap scan (`credit_bare_continuation_rows`), each padding byte read by one node however deep the spaces nest (#1445) | linear |
| `loc/tcl-wide-continued-comment` | Tcl | width | the comment lookup for a bare row continuing a comment, from the child before the row's gap (#1445) | linear |
| `loc/tcl-unclosed-continued-comments` | Tcl | width | the same lookup when an unclosed opener per unit leaves every unit a direct child of one flat `ERROR` (#1445) | linear |
| `loc/elixir-unclosed-spliced-comments` | Elixir | width | the joined-row comment a bare row after a non-continuing comment is classified by, read off the gap's next child under one flat `ERROR` (#1801) | linear |
| `loc/c-unclosed-spliced-comments` | C | width | the second comment of a comment-only joined row, reached by a copy of the gap scan's cursor under one flat `ERROR` (#1801) | linear |
| `loc/c-comment-chain` | C | width | one logical line of comment-only rows and bare rows, each bare row's class settled once by the per-walk `BareRowIndex` rather than read back from the row (#1818) | linear |
| `loc/c-unclosed-comment-rows` | C | width | the same index's comment spans, one cursor walk, where a lookup from each gap's owner would scan the flat root `ERROR`'s children per row (#1818) | linear |
| `loc/python-long-bare-run` | Python | width | a run of bare `line_continuation` nodes classified and credited by its head alone, later nodes O(1) (#1819) | linear |
| `loc/python-long-bare-run-blank` | Python | width | the Blank class of the same run, whose head reads no sibling (#1819) | linear |
| `loc/python-separated-bare-runs` | Python | width | separate bare runs inside one parenthesised bare string's parentheses, each asking whether it is on the string's line — the parenthesis's answer, memoised per parenthesis under `NodeQuestion::BareStringParenthesisLine` rather than found from each run through the siblings before it (#1819) | linear |
| `loc/python-bare-runs-before-string` | Python | width | the same question asked by runs *before* the string, where the parenthesis's own descent to its value crosses the whole prefix of `\` runs and comments; 2.08 with the memo and the `BareStringClimb` fold disabled, 1.02 with them (#1830) | linear |
| `halstead/perl-dash-keys` | Perl | width | the dash-key workaround's `=>` lookup per key (#1545) | linear |
| `abc/perl-bareword-keys` | Perl | width | the bareword-key workaround's `=>` lookup per key (#1572) | linear |
| `abc/perl-nested-signed-keys` | Perl | depth | the ABC value peel's `-and` / `-not` sign test, handed the `-`'s parent rather than a chain to climb (#1603) | linear |
| `halstead/kotlin-short-interpolations` | Kotlin | width | the `$name` test's previous-token lookup in one string (#1584) | linear |
| `halstead/kotlin-noinline-params` | Kotlin | width | the modifier recovery guard's next-token lookup (#1558) | linear |
| `abc/perl-pattern-list` | Perl | width | ABC's pattern-role climb through `( … )` (#1467) | linear |
| `cognitive/elixir-mixed-arguments` | Elixir | width | the keyword-section lookup per pair and per keyword-form clause (#1574) | linear |
| `cognitive/elixir-keyword-sections` | Elixir | width | the per-list `do:` memo behind the keyword-section gate (#1602) | linear |
| `cognitive/elixir-rejected-keyword-list` | Elixir | width | the section lookup's two position memos — trailing argument (#1602) and leading pairs (#1697) — each scanned past `width` comments | linear |
| `cyclomatic/cpp-plus-chain` | C++ | depth | shape control for the three rows below | linear |
| `cyclomatic/cpp-and-chain` | C++ | depth | the `&&` / `||` applied-operator gate (#1533) | linear |
| `cognitive/cpp-and-chain` | C++ | depth | the boolean-sequence arm on the same chain (#1533) | linear |
| `abc/cpp-and-chain` | C++ | depth | ABC's chain arm on the same chain (#1533) | linear |
| `abc/perl-or-chain` | Perl | depth | the misparse check's spine climb (#1273) | linear |
| `abc/perl-misparsed-comparison` | Perl | depth | the same climb on a chain the misparse absorbed (#1273) | linear |
| `abc/perl-and-statements` | Perl | width | the misparse check's operator lookup in a block (#1273) | linear |
| `abc/perl-error-tail` | Perl | width | the binding, bareword-key and misparse lookups under a flat `ERROR` root (#1540, #1584) | linear |
| `halstead/perl-error-tail-keys` | Perl | width | the key and sign lookups under the same `ERROR` (#1584) | linear |
| `nexits/perl-error-tail-keys` | Perl | width | nexits' `die` / `exit` key lookup under the same `ERROR` (#1585) | linear |
| `count/perl-error-tail-calls` | Perl | width | the `call` filter's bareword-key lookup under the same `ERROR`, on the `find` / `count` walk (#1580) | linear |
| `cognitive/bash-test-connective-run` | Bash | depth | the comparison-run end of a `[ … -o … ]` test (#1536) | linear |
| `abc/bash-arith-assign-chain` | Bash | depth | ABC's `=` comparison test on `(( a = … ))` (#1536) | linear |
| `abc/bash-eq-and-test` | Bash | depth | the same test on a `[[ … = … && … ]]` run (#1536) | linear |
| `dump/perl-error-tail-calls` | Perl | width | the AST export's child enumeration under the same `ERROR` (#1684) | linear |
| `dump/wide-distinct-fns` | Rust | width | shape control for the row above: a balanced `repeat()` list | linear |
| `dump/tcl-nested-eval` | Tcl | depth | the export's ancestor chain under `keeps_children` (#1381, #1684) | linear |
| `dump/tcl-wide-erroring-eval` | Tcl | width | the argument-list readability scan under `keeps_children`, memoised through the cache the export lends (#1686) | linear |
| `count/tcl-nested-braces` | Tcl | depth | walk control for the row below: the same filter with nothing rendered | linear |
| `find/tcl-nested-braces` | Tcl | depth | the renderer `bca find` prints through: each match's connector decided without `Node::parent`, and past 64 nested levels a match's text printed once (#1416) | linear |
| `find/tcl-wide-braces-past-the-limit` | Tcl | width | the same renderer's step from one elided match to the next through `next_outside`; re-walking the siblings for each is quadratic in a level's width (#1416) | linear |
| `halstead/tcl-wide-erroring-eval` | Tcl | width | the same scan under `braced_word_op_type`, reached through an iterated ancestor on the metric walk (#1686) | linear |
| `ops/tcl-wide-erroring-eval` | Tcl | width | the same scan under `braced_word_op_type`, on the `ops` walk (#1686) | linear |
| `count/tcl-wide-erroring-eval` | Tcl | width | the same scan under the `string` filter, on the `find` / `count` walk (#1686) | linear |
| `cyclomatic/tcl-wide-switch` | Tcl | width | the `CaseArm` row's arm-list test, memoised per `word_list` through the walk's `FoldCache` so `A` arms over `W` options is not `A × W` (#1695) | linear |
| `halstead/elixir-repeated-guards` | Elixir | depth | `ElixirCode::get_paying_role` billing `when` without the `Guard` row's position climb, which is per token and quadratic on a nested-`when` chain (#1695) | linear |
| `cyclomatic/elixir-repeated-guards` | Elixir | depth | the `Guard` row's position climb, memoised per `when` operator under `MemoQuestion::GuardOperator` so a nested-`when` chain is climbed once per walk rather than once per token (#1695) | linear |
| `cognitive/nested-and` | Python | depth | `python_apply_boolean_operator` asking whether *any* enclosing `boolean_operator` already paid, stopping at the first, where counting them all was quadratic in the chain (#1695) | linear |
| `npa/java-deep-expression` | Java | depth | the member walks over a deep expression in a class body: no `O(depth)` work per visited node, which the per-body chain (`Ancestors::of_children`) was while it was a copy built for every node (#1696); since #1805 a per-node build would be one allocation, a constant factor this does not price | linear |
| `npm/ruby-nested-if-defs` | Ruby | depth | the Ruby member walk reading a `def` through nested conditionals on one moved ancestor chain (`ChildAncestors::descend` / `ascend`) rather than a chain copy per level (#1401) | linear |
| `npm/ruby-wide-private-defs` | Ruby | width | the same walk reading each `private` call's argument list and re-filing the name it declares as one deferred `(covered, bit)` on the name's index entry, applied once at the tally, rather than a write per declaration — half the names distinct, half one name redefined, so both halves of the index are on the ladder (#1401) | linear |
| `npa/ruby-nested-if-attrs` | Ruby | depth | the attribute walk on the shared class-body walk, reading an `attr_*` and an `@ivar` assignment through nested conditionals and asking the `Member` row under the held chain (#1719) | linear |
| `npa/ruby-nested-classes` | Ruby | depth | the chain every member walk derives per container body (`Ancestors::of_children`), borrowing the walk's chain and owning only the body rather than copying the whole chain per class (#1805) | linear |
| `npm/ruby-nested-classes` | Ruby | depth | the same derivation under the method walk (#1805) | linear |
| `loc/ruby-nested-classes` | Ruby | depth | metric control for the two rows above: the same nest under a metric that derives no chain per class body | linear |
| `cognitive/java-nested-classes` | Java | depth | `increment_function_depth` where no function encloses the asker: the answer folded down the walk's chain on `KindLane::InsideFunction` rather than scanned to the root per method (#1864) | linear |
| `nom/java-nested-classes` | Java | depth | metric control for the row above | linear |
| `cognitive/groovy-nested-classes` | Groovy | depth | the same question on the same source under the Groovy grammar (#1864) | linear |
| `npa/cpp-wide-class-members` | C++ | width | a shape control for the C++ member walks: `cpp_declares_function` is asked of every field and static data member template by both consumers and must read only the member's own declarator chain — a walk that re-sought siblings or read a `template_declaration`'s neighbours would show here (#1299) | linear |
| `nom/elixir-unquoted-def` | Elixir | depth | the `def`-under-no-`quote` question `nom/nested-quote`'s short-circuit cannot help, folded down the walker's chain once per ancestor (#1691) | linear |
| `halstead/csharp-qualified-generics` | C# | depth | each generic qualifier classified without climbing the remaining qualified-name ancestry | linear |
| `abc/elixir-many-timeouts` | Elixir | width | malformed `receive` timeout clauses that must not each rescan every message clause | linear |
| `vcs/all-equal` | — | files | `vcs::score::apply_percentile` over a population where every signal ties across all files (#1267) | linearithmic |
| `vcs/duplicate-heavy` | — | files | the same pass where each signal takes a small fixed set of values, so every file shares each rank with a large slice of the population (#1267) | linearithmic |
| `vcs/spread` | — | files | the same pass with few exact ties, so every file's binary searches land at a different index — the shape under which the search phase, which dominates the sort, costs the most (#1267) | linearithmic |

The three `vcs/*` rows are the one hot path here that is not a walk
over an AST: their input is a `Vec<Stats>` grown along the file count,
measured by `big-code-analysis-bench/src/vcs.rs` with the same
warm-up, rotation and three-point fit, and joined into the same report.
See [The ranking pass](#ranking-pass).

### The control probes

The control probes are what make the other readings mean something.

- `nom/nested-while`, `nom/nested-fn`, `nom/nested-fn-rows` and
  `nom/java-nested-classes` are **metric** controls.
  `Cognitive` declares `Nom` as a dependency in `src/metric_set.rs`, so
  the cognitive-attributable cost of each `cognitive/…` row is its
  difference from the `nom/…` row on the same shape, not the
  `cognitive` reading alone. `nom/nested-fn-rows` plays the same part
  for `loc/nested-fn-rows`: `Nom`'s merge is a counter add, so the
  difference is the cost of `Loc`'s per-space row sets.
  `count/tcl-nested-braces` is the same kind of control for
  `find/tcl-nested-braces`: the walk `bca find` runs, without the
  render that follows it.
  `loc/ruby-nested-classes` is one for `npa/ruby-nested-classes` and
  `npm/ruby-nested-classes`: the same nested classes under a metric
  that derives no chain per class body.
- `loc/nested-while`, `cognitive/nested-while`,
  `nom/nested-declared-function`, `halstead/nested-paren`,
  `abc/nested-block`, `cyclomatic/nested-and`, `cyclomatic/cpp-plus-chain`,
  `abc/js-nested-patterns`, `loc/nested-fn` and `dump/wide-distinct-fns`
  are **shape** controls:
  each is the same nesting (or, for the last, the same width) as the
  probe it sits next to, with the one node that triggers the walk
  removed. Before [#1084][parent-walk] each fitted near 1.0 where its
  counterpart fitted near 2.0, which is what attributed the quadratic
  cost to that call rather than to nesting in general. Now that every
  probe fits near 1.0, the pair is what would localise a relapse: a
  probe drifting up while its control holds means the ancestor lookup,
  not the shape.

  `loc/nested-fn` is the shape control of a different kind: it is the
  same function nesting as `loc/nested-fn-rows` compressed onto a single
  physical row, so the space stack still merges at every level but each
  merge carries one row. A regression in the per-*row* fold moves
  `loc/nested-fn-rows` and leaves it flat; a regression in the
  per-*merge* overhead moves both.

  `loc/nested-quote` is the one #1096 probe with no shape control of its
  own: its Elixir arm fires for every named node, so there is no version
  of the shape with the trigger removed. `nom/nested-quote`, the same
  source under a metric that does not ask for a parent, is the metric
  control that stands in for one.

### Adding a probe {#adding-a-probe}

Add a `Probe` to `PROBES` in `big-code-analysis-bench/src/shapes.rs`.
The unit tests in that module enforce what a probe has to satisfy:
bytes affine in the size parameter, no parse errors at the smallest
declared size (or, for a probe listed in `RECOVERY_PROBES`, an `ERROR`
root), a non-zero workload reading there, sizes that double,
and the invariant its `axis` selects: AST levels growing with the size
on `Axis::Depth`, children growing while AST depth holds still on
`Axis::Width`. Set `max_exponent` from a measurement on an idle host,
not from theory, and say in `rationale` which call the probe is
watching.

Then falsify it. Reintroduce the regression the probe claims to catch
behind a local patch, confirm the probe goes red and its controls stay
green, and record both numbers; a probe nobody has made fail is a
guard on paper. The width probe's own run is quoted below.

When a fix folds or memoises one ancestor climb, sweep for the others
in the same change. A climb that stops at the nearest matching ancestor
is linear on every shape where one is near, so its probe and its tests
pass while the same loop is quadratic where none is. #1767 folded every
climb its own sweep found and left `increment_function_depth` scanning
(#1864), and the sweep made for that fix found another (#1867). These
searches find the spellings of #1864 and #1867 and the adapter and
helper spellings around them. `Ancestors::iter` is the only `iter` in
these crates that takes an argument, which is what the first two key
on; the third finds a `successors` climb over `Node::parent`, and the
last lists the callers of the climbing helpers. They do not follow an
iterator bound to a name or passed on as an `AncestorIter`: read those
sites by hand.

```bash
rg -U '\.iter\([^)]+\)\)?\s*(\.(map|skip|by_ref|take|filter)\([^\n]*\)\s*)*\.(any|all|find|find_map|position|take_while|skip_while|map_while|last|count|fold|try_fold)\(' src big-code-analysis-ast/src
rg 'for .* in .*\.iter\([^)]+\)' src big-code-analysis-ast/src
rg 'successors\(.*parent' src big-code-analysis-ast/src
rg 'count_specific_ancestors|has_ancestor_before_stop|has_ancestor_of_kind|nearest_level_answer' src big-code-analysis-ast/src
```

For each match, name what stops the loop, then render a shape where
that stop is absent or far and time it against a control.

### Why each probe exists

Each probe below is paired with the issue that added it and the
measurement that showed it failing. Four of the probes in the table
above were quadratic when the harness landed, and they shared one
cause: `tree_sitter` stores no parent pointer, so `Node::parent`
resolves by descending from the root and is itself `O(depth)`. Any
predicate in the walk that asked a node for its parent was therefore
`O(depth)` per node and `O(depth^2)` over a deeply nested file, however
few steps it took. [#1084][parent-walk] fixed three of them by having
the metric walk carry the ancestor chain down with it (`Ancestors` in
`big-code-analysis-ast/src/node.rs`), so a predicate reads an ancestor
as a slice index. Their bounds moved to the linear bound in that same
change, which is what now
catches a relapse. `cognitive/nested-fn` is the fourth of the same
family: `increment_function_depth` was deferred out of #1084 and fixed
the same way in [#1062][cognitive-parent], which is also where that
probe comes from: it fitted 2.04 against the climb and 1.21 against the
chain.

`nom/nested-arrow` is the fifth, added by [#1088][remaining-climbs] with
the fix it guards. It cost the same family a second lesson: threading
the chain into the JS-family predicates was *not enough* to make it
linear. The walk they run is two steps long on that shape, and the
remaining `O(depth)` term was hiding in `Node::wraps_any`, which scanned
a node's children with `child(0)` + `next_sibling()`. A sibling step
resolves its parent, so that scan was `O(children × depth)`: the same
defect one level down, in a helper whose doc comment asserted it was
`O(1)` per step. Moving the scan onto a cursor took the probe from 17.6 s
to 6.3 ms at depth 4000 (`k` 1.97 to 1.03), and a walk over the
384-file `pdf.js` corpus from ~443 ms to ~370 ms. The lesson
generalises: a chain-fed predicate is only as linear as the primitives
it calls.

Three more probes come from [#1096][halstead-climbs], which
retired the remaining `Node::parent` calls in the per-language metric
bodies: the Halstead `get_op_type` getters, every `Abc` impl, and the
`loc` / `npm` / `npa` / `cyclomatic` / `is_useful_comment` arms.
`Halstead::compute`, `Abc::compute`, `Npm::compute`, `Npa::compute`,
`Cyclomatic::compute`, and `Checker::is_useful_comment` gained an
`Ancestors` parameter there, and the comment-removal walk grew a chain
of its own. A fourth call the issue had not listed turned up
during review: Python's `Cyclomatic` `else` arm, reached through
`Node::parent_grandparent_match`, which no `rg '\.parent\(\)'` sweep
finds at the call site. All four probes fitted 1.99-2.06 before and
0.99-1.27 after; at depth 4000 `halstead/nested-not` dropped from
~478 ms to ~0.57 ms, `abc/nested-if` from ~808 ms to ~3.3 ms,
`cyclomatic/nested-ternary` from ~1.13 s to ~3.4 ms, and
`loc/nested-quote` from ~9.6 s to ~9.2 ms. Their controls held at
0.97-1.31 either side.

That change also confirmed #1088's lesson a second time from the other
direction. The ABC condition walkers reach a slot's parent to decide
whether it sits in boolean context, and the same walkers used to ask
`Node::previous_sibling` whether a ternary's `?` / `:` precedes the
operand, and `ts_node__prev_sibling` opens with `ts_node_parent`, so
it carries the same `O(depth)`. Passing the parent in and scanning its
children (`Node::previous_sibling_under`) was what made the fix hold on
a ternary shape as well as on the shapes the probes render.

That helper no longer exists: #1181 moved every ternary slot onto
`child_by_field_name`, which needs no sibling scan at all, and the
method was removed with its last caller. The sibling cost described
above is still the reason to reach for a field lookup first.

The `Ancestors::unknown()` call sites that remain in production code
are deliberate rather than deferred, and each says why at the site:
the two synthetic-`Unit`-root pushes hand it a node that *is* the root,
the `Npm` arms that test a node's children cannot extend a borrowed
slice by one element without allocating, and the role-owner gates the
ABC slot helpers and Python's cognitive `and` chain ask through
`rows_own!` / `short_circuit::owns` read no ancestors at all. The
`parser.rs` filter predicates were once another until #1381 routed
`find` and `count` through `Search::act_on_node`'s chain.

`nom/nested-attributed-fn` and `nom/wide-attributed-fn` are the first
probes that walk under a non-default `MetricsOptions`. Both hot paths
are reachable only with `exclude_tests` set, which `Workload::Metrics`
now carries as its own field. It sits on that variant rather than on
`Probe` because `Ast::ops` takes no options at all, so an `Ops` probe
setting it would be setting something nothing reads.

`nom/nested-attributed-fn` guards [#1100][attribute-scan]. Under
`exclude_tests` the walker asks every node whether it opens a test-only
subtree, and Rust answers by reading the run of `#[…]` siblings before
an item. Walking that run backward with `Node::previous_sibling` costs
`O(attributes x depth)`, so the probe fitted 2.00 and took 2.05 s at
depth 4000 against 8.3 ms for `nom/nested-fn`, the same nesting without
the attributes. Reading the run forward from the parent the walker
already holds took it to 1.21 and 8.1 ms, level with that control.

That fix traded one axis against the other, and the probe alone would
not have caught why. A forward pass over the parent's children is
`O(children)` and flat in depth; the backward walk is the reverse.
Reading forward unconditionally fixed the depth axis and broke the
width one: a generated file of 2 000 top-level attributed items went
from 6.0 ms to 569 ms, which is the shape `bindgen` output has. #1100
therefore budgeted the parent's child count against the node's depth,
reading forward only where that was the cheaper bound. [#1447][one-attr]
removed the budget again, for the reason given below.

**Both halves of that trade were measured** (#1133).
`nom/wide-attributed-fn` renders the wide shape (a flat file of
`n` top-level `#[inline] fn f() {}` items, 500 / 1 000 / 2 000) under
the same `exclude_tests` walk. It fit 0.97-1.00 against the budgeted
dispatch. Reintroducing the rejected forward-always scan behind a local
patch takes it to **1.99**, at 35.6 / 145.5 / 560.6 ms against
1.9 / 3.8 / 7.2 ms: 78x at the top of the ladder, the same order as
the 94x #1100 measured. Every depth probe stayed inside its 1.5 bound,
`nom/nested-attributed-fn` (the same scan, nesting instead of widening)
among them at 1.06. That is the evidence that the probe covers what it
claims and that the depth probes do not.

**Two axes are not a plane** (#1446). Both probes above pin one axis
with the other held still, and the scan's cost is a product of the two,
so a shape that grows them *together* is outside the region either
covers. [#1431][attribute-row] made the prune answer for every `#[…]`
row rather than only for the item, and each answer re-derived the whole
run, which is `O(run)` done `run` times. It is invisible to
`nom/nested-attributed-fn`, where every run is one attribute long, and
to `nom/wide-attributed-fn`, where depth 1 kept the budget at six
children and the reading on the sibling walk.

`nom/deep-attribute-run` renders the diagonal: `depth` nested `fn f`,
then one run of `3 * depth` `#[cfg(test)]` rows on a single innermost
item. The `3 *` was the point rather than a round number: the run's
parent, `3 * depth + 4` children sitting `2 * depth + 1` levels down,
stayed inside the `6 * depth + 3` the budget allowed there, so it was
guaranteed the forward `O(children)` reading. The budget that existed
to bound that scan selected *into* it on this shape.
Measured at 319 / 1 288 / 5 172 ms across the ladder, fitting **2.01**,
against 0.05 s for the deepest cell with `exclude_tests` off. The run's
verdict is now taken once and reused across its members
(`SubtreeSkip`, `big-code-analysis-ast/src/checker.rs`), which reads
1.5 / 3.1 / 6.4 ms and fits 1.06. #1446 left the budget alone: on
this diagonal the forward pass was still the cheaper of the two
readings, and what was quadratic was asking once per attribute.

Its ladder is `ATTRIBUTE_RUN_DEPTHS`, half `LINEAR_DEPTHS` at every
rung, because the shape renders five rows per unit of depth. A
4 000-deep cell is a 200 KB tree under an 8 000-entry ancestor chain,
and its per-byte cost drifts 1.7x up the ladder against the ~1.25x
`LINEAR_BOUND` is set from: enough to fit 1.37 with the walk linear,
which is 0.13 of headroom on a shared runner. The shorter ladder still
reaches a 6 000-attribute run, well past where the quadratic shows.

**A budget between two primitives goes stale when a third lands**
(#1447). `nom/deep-one-attribute-items` keeps the diagonal but splits
the run into one per item: `depth` nested `fn f`, then `3 * depth`
separate `#[cfg(test)]\nfn t() {}` items in the innermost body. The
reach makes each run one ask, but each ask still read the parent with
a cursor pass from its first child, because the budget admitted the
parent, so `3 * depth` asks of `O(width)` each were quadratic: 0.74 /
2.99 / 11.97 s, fitting **2.00**. By then the budget was arbitrating
between two readings that a third dominated: since #1545 and
[#1584][flat-error], `Ancestors::siblings_before` / `siblings_after`
find a node by binary search in the children the walk records for a
wide parent (`SiblingCache`), or among at most `WIDE_PARENT` children by
a short seek. That reading is free of depth and at most logarithmic in
width, so the attribute run and the item it decorates now go through it
and the budget is gone. The probe reads 1.8 / 3.7 / 7.9 ms and fits
1.08; the three probes above fit 1.06-1.15 with and without the budget,
and the repository's own Rust did not move off 0.64 s. What the probe
prices is the removed pass from the first child, not the cache: its
body is a grammar `repeat()`, where the byte seek is logarithmic too.
The cache is what keeps a flat `ERROR` parent linear.

`nom/deep-doc-comment-run` is the same diagonal with the run spelled
as comment rows: `3 * depth` pairs of an outer doc comment and a plain
comment above one `#[cfg(test)] fn t`. Since #1647 a comment inside an
attribute run is part of it — rustc attaches a `///` as `#[doc]` and
reads `//` as no token, so the item is test-only either way — and an
outer doc comment opens a run the way an attribute does, so each `///`
row is a place the hook reads the run from. The first row's verdict
answers for the rest through the reach; with the reach removed the
shape fits 1.86 (4.3 / 15.5 / 56.2 ms at depths 500 / 1 000 / 2 000),
with it 1.08 (0.6 / 1.2 / 2.6 ms). The plain rows are what the hook
answers `RETAIN` for in `O(1)` when no reach covers them, which is
what keeps a long stretch of `//` rows outside any run from being
quadratic in itself.

The unit suite pins the reading only in part.
`the_exclude_tests_prune_resolves_no_sibling_from_a_node_either_side_of_wide_parent`
in `src/observation_tests.rs` asserts that the prune resolves no sibling
from a node on either side of `WIDE_PARENT`, which restoring the budget
fails. `rust_outer_attr_scans_agree` in
`big-code-analysis-ast/src/checker.rs` compares both lookups against
the raw sibling accessors, and the `exclude_tests_rust` tests in
`src/spaces_tests.rs` assert the walk's values under a cached parent.
None of them sees a pass over the parent's children or a prune handed
no chain, since both answer the same and touch none of their counters.
`the_exclude_tests_prune_scans_no_children_for_an_attribute`, beside
the first, catches the pass through the `child_scan_cursors` count that
`Node::children` keeps (#1784); only the probes above price a prune
handed no chain.

`nom/nested-cfg-predicate` guards [#1105][cfg-predicate] and is the only
probe that grows one *attribute* rather than the code around it:
`#[cfg(all(all(… test …)))]` is classified by a string-level
mini-parser reading the attribute's text, which re-scanned each
region's whole interior to find its split points. It fits 1.00 against
the indexed scan that replaced it. `nom/nested-fn` is not a control for
it, because the file keeps its two items at every depth, so the bound
alone is the guard.

`halstead/wide-distinct-fn` is the second width probe, and it comes
from a different family than every row above it. Nothing here climbs an
ancestor chain: [#1106][space-merge] found the parent's Halstead `Stats`
being re-derived from its occurrence maps *once per popped child space*,
which is three map traversals (one of them over the parent's whole
accumulated operand vocabulary) for work the parent's own finalize
repeats and overwrites. That is `O(children × vocabulary)` where
`O(vocabulary)` suffices: quadratic in a file's function count, and
invisible to every depth probe, because the cost is charged per popped
child and a nesting shape has one child per space however deep it
goes. The shape is `n` sibling `fn f000000() { let v000000 = 000000; }`
items, each with an identifier set unique to itself, because
`HalsteadMaps::operands` is keyed by source text and a file of `n`
*identical* functions has a constant vocabulary that keeps the arm
linear no matter how it is written.

Its ladder is 4 000 / 8 000 / 16 000, eight times the other width
probe's, and that is load-bearing rather than incidental. The redundant
pass is a second visit to data the walk already touched, so its cost
sits against the walk's own per-token cost in a ratio fixed by the
sibling count alone; giving each function a richer vocabulary raises
both terms together. At 500 / 1 000 / 2 000 the regression fits
1.37-1.39 against a 1.07-1.09 baseline: a real 2.1x slowdown that the
1.5 bound passes. At 4 000 / 8 000 / 16 000 the gate reads **1.09**
clean and **2.17** with the per-child pass restored behind a local
patch, the only failing probe in that run, at 507.4 ms against
45.6 ms on the 16 000-sibling cell. Do not harmonise the two width
ladders; the shorter one cannot see this class.

The three `*/cpp-and-chain` probes guard a climb that no earlier probe
could see, because none of them is a C++ boolean chain (#1533). A
parenthesised `requires (A<T> && B<T>)` parses as an ordinary
`binary_expression`, and the first attempt to exclude it from
cyclomatic, cognitive and ABC climbed from every `&&` to the enclosing
`requires_clause`. In a left-nested `a && a && …` the operator at depth
*k* has *k* chain ancestors, so the climb cost an 8 000-term chain 4.4 s
in a debug build, against 0.09 s once reverted. With that climb
reinstated behind a local patch the three probes fit 1.97, 1.97 and
1.95 (`cyclomatic/cpp-and-chain` reads 0.74 ms clean and 104.6 ms
climbing at depth 4 000), while `cyclomatic/cpp-plus-chain`, the same
chain through `+`, holds at 0.92 and every other probe stays inside its
bound. Each metric has its own probe because each reaches the chain
through its own arm, and a climb added to one of them would leave the
other two flat.

The seven Perl and Bash probes guard walks that went quadratic during
the #1273, #1536 and #1540 work and were caught only by a hand-timed
check in review ([#1575][perl-bash-probes]). Each was watched failing
with its pre-fix code put back behind a local patch, one walk at a
time:

| Walk put back | Probe that fails | Clean | Regressed | Top rung |
|---|---|---|---|---|
| every Perl spine node climbs | `abc/perl-or-chain` | 1.00 | 2.00 | 8.8 ms → 2.33 s |
| (same patch) | `abc/perl-misparsed-comparison` | 0.99 | 2.01 | 8.8 ms → 2.35 s |
| the stop parent scanned from its start | `abc/perl-and-statements` | 1.08 | 2.01 | 3.7 ms → 306 ms |
| any parent read as a binding | `abc/perl-error-tail` | 1.19 | 2.05 | 2.9 ms → 492 ms |
| the Bash comparison run climbed per node | `cognitive/bash-test-connective-run` | 0.98 | 2.06 | 6.5 ms → 1.99 s |
| every Bash `=` climbs to its test | `abc/bash-arith-assign-chain` | 1.00 | 1.97 | 0.8 ms → 72.5 ms |
| (same patch) | `abc/bash-eq-and-test` | 1.02 | 1.91 | 2.9 ms → 75.2 ms |

In each run only the named probes moved, and every other probe stayed
inside its bound.

Removing only the kind gate in front of the Perl lookup moved nothing:
`abc/perl-and-statements` held at 1.19. On a block that gate is not
what keeps the walk linear, because the lookup seeks the child by byte.
That seek is `O(log n)` only where the grammar stores the children as
a balanced repeat. An `ERROR` that recovery built by skipping tokens is
one flat array, which the seek scans from its start, and no tree-sitter
call reaches a child of one in better than its index
([#1584][flat-error]). So the metric walk and the `ops` walk record the
children of every node wider than 64 as they push them, in a
`SiblingCache` they lend through `Ancestors`, and
`Ancestors::siblings_after` / `siblings_before` binary-search that
array instead of seeking. `Ancestors::previous_sibling` reads it too:
it scanned its parent from the start, which made a valid Kotlin string
of `$name` interpolations quadratic (11.5 s for 8 000 names). With
`Ancestors::lookup` patched to ignore the cache, these four probes go
over bound, as does `count/perl-error-tail-calls` below, whose walk
reads the same cache:

| Walk put back | Probe that fails | Clean | Regressed | Top rung |
|---|---|---|---|---|
| sibling lookups seek in the flat `ERROR` | `abc/perl-error-tail` | 1.08 | 2.10 | 8.8 ms → 326 ms |
| (same patch) | `halstead/perl-error-tail-keys` | 1.04 | 1.74 | 14.5 ms → 96.0 ms |
| (same patch) | `halstead/kotlin-short-interpolations` | 0.96 | 2.02 | 1.1 ms → 589 ms |
| (same patch) | `nexits/perl-error-tail-keys` | 1.11 | 1.87 | 5.5 ms → 127 ms |

The walk `bca find` and `bca count` share, `act_on_node`, keeps the same
chain and lent no cache until Perl's `call` filter started asking the
bareword-key test of every bareword ([#1580][find-call]). It records
one now, and `count/perl-error-tail-calls` prices it:

| Walk put back | Probe that fails | Clean | Regressed | Top rung |
|---|---|---|---|---|
| `act_on_node` lends no cache | `count/perl-error-tail-calls` | 0.88 | 1.89 | 2.8 ms → 84.5 ms |

Since [#1601][sibling-capability], a walk cannot leave the cache out
by forgetting to lend it: `Ancestors::for_walk` takes the
`SiblingCache` as an argument. The three walks whose predicates ask for
no sibling (suppression, the dump and comment removal) construct their
chains through `Ancestors::for_walk_without_siblings` or the
crate-private `Ancestors::for_walk_chain_only`, which lend an empty
record. A predicate added to one of them that asks for a sibling under
a parent wider than `WIDE_PARENT` trips the walk assertion wherever a
debug build runs that walk over such a parent.
`the_walks_that_record_no_siblings_ask_for_none`
in `src/observation_tests.rs` does so for every `LANG` variant, and
fails for a variant with no fixture. It runs the three walks over a
fixture with a wide parent and requires the `sibling_seeks` counter to
stay at zero, so it also catches the asks that trip no assertion: one
under a narrow parent, and `siblings_before` or `siblings_after`
through a derived chain. The scaling gate does not price these walks'
sibling lookups, because they make none.

The metric walk's member consumers (`npa` and `npm`) read a body's
members, in most languages through a chain derived with
`Ancestors::of_children`. That chain carries no sibling record, since
the walk has not recorded the body's children while it visits the
container. Through it `siblings_before` and `siblings_after` seek, and
`previous_sibling` trips its own assertion under a wide parent.
`the_member_walks_ask_for_no_sibling_under_a_wide_body` in
`src/observation_tests.rs` holds both sibling counters at zero over a
body wider than `WIDE_PARENT`, for every language with a member walk.
Its fixtures spell a field and a method, the two plainest members.

The walk's record is not forwarded to a derived chain
([#1871][sibling-follow-ups]). A probe over the test suite and the
repository corpora found no walk asking for a sibling through one. The
one predicate found that can is Java's package gate, which
`error_holds_target` asks of a parse error's children: at most one seek
per call, counted by `a_wide_error_s_children_ask_for_one_sibling_in_java`
in `big-code-analysis-ast/src/lang_helpers/doc_comment.rs`. The same
file holds the Doxygen grammars' target tests at no seek.

What `bca find` does after that walk went unpriced until
[#1416][find-render]. It printed each match through
`dump_node_with_color` at depth 1, which chose the match's connector by
asking `Node::parent`: one `O(depth)` climb per match, 98% of the
397 ms it took on 4 000 nested Tcl braces. It also printed each
single-row match's whole span, so a match nested `k` deep was printed
`k` times, 16 MB of output for 8 KB of input. The renderer the CLI
calls now, `render_found` in `src/output/dump.rs`, decides the
connector against the tree's root, prints each match whole through 64
levels of single-row matches nested in one another — so ordinary
output is what it always was — and past that prints `…` in place of
each nested match, stepping from one to the next through
`next_outside`. `find/tcl-nested-braces` times the walk and that render
into a sink that reads every byte; `count/tcl-nested-braces` is the
walk alone. Both take `FIND_RENDER_DEPTHS`, 4 000 / 8 000 / 16 000,
because the output term needs those depths to overtake the per-match
cost: on `LINEAR_DEPTHS` the whole-span render fit only 1.56.
`find/tcl-wide-braces-past-the-limit` is the width half: 4 000 to
16 000 sibling words inside 66 braces, so the level eliding them is
past the limit, where a chain of one match per level cannot show a
rescan of the siblings.

| Render put back | Probe that fails | Clean | Regressed | Top rung |
|---|---|---|---|---|
| a `Node::parent` climb per match | `find/tcl-nested-braces` | 0.99 | 2.00 | 5.3 ms → 6.2 s |
| each match prints its whole span (the limit at `usize::MAX`) | `find/tcl-nested-braces` | 1.01 | 1.80 | 5.3 ms → 67.0 ms |
| each next match found by re-walking the siblings | `find/tcl-wide-braces-past-the-limit` | 1.01 | 1.81 | — |

`count/tcl-nested-braces` held at 0.84–0.94 under all three, and the
depth probe at 1.02 under the third.

The AST export reads the same flat `ERROR`, and until [#1684][dump-cursor]
it did so by index: `Node::child` and `Node::field_name_for_child` both
scan the parent's child array to the index, skipping hidden subtrees by
their cached counts, which is `O(log n)` under a balanced `repeat()`
and `O(index)` under a flat node. Whether an `ERROR` is flat is a
property of how recovery assembled it, not of where it sits: the Perl
root here holds every skipped token as a direct child and is the
measured flat case, but a root whose skipped run stayed under a hidden
`_ERROR` repeat is balanced, and a nested `ERROR` wrapping a few popped
tokens is flat. The probe shape is the one that is known to be wide
and flat. `Ast::dump` now walks one cursor, which steps to a sibling
in amortised `O(1)` whatever the parent's shape and reads a node's
field off the hidden levels above it, so enumerating the tree is
`O(n log n)` over a balanced list and `O(n)` over a flat one.

That bounds the enumeration, not the alterator, whose predicates
are the language's to keep linear. Tcl's was not until
[#1686][tcl-slots]: `keeps_children` asks
`argument_slots_are_readable` of every braced word under a `word_list`
that holds an error, and the answer is an `O(width)` sibling scan, so
a wide `eval` line with one stray `]` among its arguments was quadratic
in its width on either strategy, and on every other walk that reaches
a braced word's role (`Ast::metrics` through `braced_word_op_type`,
`Ast::ops`, and `find` / `count --type string`). The scan's answer now
rides in the `FoldCache` each walk lends with its chain, keyed by the
list's id, so it runs once per list per walk. Four probes price it,
one per walk, because each lends its cache independently and a
regression in one is invisible from the others. The two
Halstead-shaped rows have a second way to regress: they reach the
question through `Ancestors::iter`, which has to carry the cache onto
the ancestor's chain.

The indexed walk stays reachable as `Ast::dump_indexed`, and routing
the default back to it is what `dump/perl-error-tail-calls` prices; the
export also carries the ancestor chain `keeps_children` reads, and
handing that an unknown chain is what `dump/tcl-nested-eval` prices.
The `Dump` workload's reading is the exported node count, taken on a
second pass over the result before it is dropped, so each timing is
construction + that count + teardown. The count is a constant factor:
2-5 % of the region on the width shapes and real files, 8-11 % on the
depth probe, where each level's two-element extend has poor locality.
It is `O(nodes)`, so it preserves the walk's complexity class; on a
three-rung ladder a constant-factor term can still nudge the fitted
slope, which is why the share is recorded. The iterative drop is the
larger share everywhere, 20-30 %:

| Walk put back | Probe that fails | Clean | Regressed | Top rung |
|---|---|---|---|---|
| `Ast::dump` enumerates by index | `dump/perl-error-tail-calls` | 1.02 | 1.98 | 14 003 children under the `ERROR` |
| (same patch) | `dump/wide-distinct-fns` | 0.79 | 0.80 | unchanged, as the control should be |
| `finish` hands `keeps_children` an unknown chain | `dump/tcl-nested-eval` | 0.75 | 2.00 | 4 000 braces, 12 000 exported levels |
| `Export` lends no `FoldCache` | `dump/tcl-wide-erroring-eval` | 1.00-1.04 | 2.03-2.07 | 16 000 braced words; 15.5-17.5 ms clean against 8.7-9.4 s |
| `metrics_inner` lends no `FoldCache` | `halstead/tcl-wide-erroring-eval` | 0.98-1.02 | 2.04-2.07 | 11.9-12.8 ms clean against 8.2-11.3 s |
| `ops_inner` lends no `FoldCache` | `ops/tcl-wide-erroring-eval` | 0.99-1.01 | 2.01-2.22 | 10.3-10.4 ms clean against 7.8-16.2 s |
| `AncestorIter` drops the cache | `halstead/tcl-wide-erroring-eval` | (same) | 2.03-2.05 | 7.6-8.9 s; the dump and count rows stay at 1.0, since neither reaches the question through an ancestor |
| `act_on_node` lends no `FoldCache` | `count/tcl-wide-erroring-eval` | 0.98-1.03 | 2.01-2.05 | 6.9-7.7 ms clean against 8.1-8.8 s |

Each band is three runs at the top rung; the regressed spreads are
wide because a 9 s cell shares the host with whatever else is running.
On `LINEAR_WIDTHS` the clean cells fall to 0.2-1.8 ms and the same
regressions fit anywhere from 1.62 to 2.08 run to run, which is why
these four probes take the space-merge ladder.

Temporary storage was measured once with a counting Rust
`#[global_allocator]`, at the flat `ERROR` at width 4 000 (twice the
probe's top rung), the wide list at 2 000, the depth probe at 4 000 and
on `src/spaces/compute.rs`, as peak live bytes during the export less
the finished tree's own capacity. That counter sees the frame stack and
the ancestor chain and **not** the `TreeCursor`'s own stack: the C library
allocates that through `ts_current_malloc`, which is libc's `malloc`
unless `ts_set_allocator` was called. Both walks share one `Frame` and
one `Export`, so what it saw was byte-identical on both sides, and
every figure is a `Vec` capacity after doubling rather than a live
length: 448 bytes on the flat `ERROR`, 896 on the wide list, 3.5 KB
on `compute.rs`, 1.84 MB on the depth probe, where
16 384 slots of one 80-byte frame plus one 32-byte chain entry hold the
12 000 exported levels. The cursor's unmeasured stack is one 32-byte
entry per *raw* level on the path to the current node, hidden nodes
included, so it is bounded by the raw depth and never by the width.
Output capacity is identical by construction: both reserve each node's
`children` from `child_count()`. Those are four inputs, not a proof
for all of them.

The probes whose shape is meant not to parse are listed in
`RECOVERY_PROBES` in `shapes.rs`, which exempts them from the parse
check and asserts that each root is still an `ERROR`. `dump` probes
have one more invariant of their own,
`dump_probes_export_proportionally_to_their_size`: the raw-tree shape
checks cannot see an exporter that collapses to a root, so the
*exported* node count or depth must grow with the size too, on the
ladder itself.

`npa/ruby-nested-classes` and `npm/ruby-nested-classes` guard
[#1805][nested-containers]. Every member walk reads a container's body
under the chain `Ancestors::of_children` derives for it, once per
container, and that chain was a copy of the walk's whole chain plus the
body. One copy per container is `O(depth)`, so `n` nested containers
cost `Σ depth`: quadratic in the nest, in every language whose `npa` or
`npm` reads a body (nested C++ `struct`s and Java classes showed it as
well). A derived chain now reads the walk's chain in place and owns only
what it adds below it. Deriving from the walk's chain is one allocation
of one entry; deriving from a derived chain copies the entries that one
added, which every caller bounds by a fixed structural nesting; and an
unbounded descent still moves one held chain with
`ChildAncestors::descend` / `ascend`.

The two probes needed a taller ladder than the one that suited the
copy #1699 removed. With the copy present they read 1.39-1.51 on
4 000 / 8 000 / 16 000, straddling the bound (`npa` crossed it on two
runs of three), and 2.18-2.19 on `NESTED_CONTAINER_DEPTHS`
(8 000 / 16 000 / 32 000), where the copied chain outgrows cache;
without it they read 1.05-1.19 there.
`loc/ruby-nested-classes` reads 1.22-1.28 either way, the drift of a
walk under a chain 64 000 entries deep. The per-change guard is
`of_children_borrows_the_walk_s_chain_and_owns_only_what_it_adds` in
`big-code-analysis-ast/src/node/ancestors.rs`, which pins that a
derived chain's upper run *is* the walk's slice rather than a copy of
it.

`cognitive/java-nested-classes` and `cognitive/groovy-nested-classes`
guard [#1864][enclosing-function]. The cognitive walk asks every
function definition whether another function encloses it, to charge a
nested function its depth. #1062 moved that question off `Node::parent`
onto a scan of the walk's chain, which stops at the nearest function.
`cognitive/nested-fn` was added for that case, where one is two steps
up. A
method in a class has none, so its scan read the whole chain, and a
method in each of `n` nested classes read `Σ depth` entries. The answer
is now folded down the chain on `KindLane::InsideFunction`
(`Ancestors::has_ancestor_of_kind`), each ancestor's kind once per
child the walk asks under, the way #1767 folded the other ancestor
questions that have no stop nearby.

Both probes render the same source, `depth` nested classes each holding
one method with one `if`, on 4 000 / 8 000 / 16 000. The Groovy row is
there because that grammar parses nested type declarations only since
0.5.0, which is what made the shape reachable for it.

| Walk put back | Probe that fails | Clean | Regressed | Top rung |
|---|---|---|---|---|
| a chain scan per function | `cognitive/java-nested-classes` | 0.60-0.66 | 2.45 | 82-92 ms → 4.2 s |
| (same patch) | `cognitive/groovy-nested-classes` | 0.82-0.91 | 2.62 | 75-83 ms → 5.8 s |
| (same patch) | `nom/java-nested-classes` | 0.72-0.75 | 0.90 | 62-66 ms → 93 ms, inside its bound; `Nom` does not ask the question, so the difference is the host's |

The clean bands are two runs and the regressed column one, on a host
that was not idle. The clean exponents sit below 1 because the first
rung reads high, in the `nom` control as well, so the fold is not the
cause; what inflates the first rung is not identified. The fit
therefore leaves 0.6 to 0.9 under the bound, and a regression milder
than the full scan can pass: these probes catch the scan and nothing
subtler. A rung higher would not help, since the scan's top cell would
run past `MAX_CELL_WALK` and report an abandoned walk in place of an
exponent. No other probe left its bound with the scan put back.
`cognitive/nested-fn`, the shape the scan was cheapest on, read
0.83-0.85 clean against 0.75 with the scan, and 8.7-9.8 ms against
10.5 ms at its top rung.

The per-change guard is
`the_cognitive_walk_folds_the_enclosing_function_question` in
`src/observation_tests.rs`, which is what covers the milder
regressions. It pins two exact counts over 200 nested classes: the
ancestor states the fold stores, and the stored states it reads to find
where to resume. The scan stores none; a memo that never matched
stores about `depth²`; and a search that started at the root would
read about `depth²` slots while storing each state once, which the
first count cannot see. Under `--cfg chain_audit` every folded answer
is also checked against the scan, which is what would catch a second
kind list asked on the same lane.

`nargs/java-record-alternative-ctors` and
`nargs/groovy-record-alternative-ctors` guard the record-header gate
[#1842][record-header] added. A record's header opens a function space
only where the body writes no canonical constructor, and a written one
is told from an alternative by its parameter types, so the gate scans
the body and spells the types of every constructor named after the
record until one differs. The walk promotion, the space's kind,
`nargs`, `nom`, `npm` and cognitive each ask the gate, so a record
costs a handful of scans, each linear in its body. No probe rendered a
record before [#1863][roles-followups].

Both probes render one record of ten components whose body holds
`width + 4` alternative constructors, each spelling nine component
types and a tenth that differs, which is the most a constructor can
cost.

| Walk put back | Probe that fails | Clean | Regressed | Top rung |
|---|---|---|---|---|
| the gate asked once per constructor | `nargs/java-record-alternative-ctors` | 1.01 | abandoned | 32 ms → 47 s |
| (same patch) | `nargs/groovy-record-alternative-ctors` | 1.01 | abandoned | 26 ms → 49 s |

One run each, on a host that was not idle. The regressed walk runs past
`MAX_CELL_WALK` at the top rung, so the gate reports an abandoned walk
in place of an exponent. `loc/java-package-docs`,
`loc/rust-attributed-docs` and `cognitive/wide-if-siblings` read 0.84,
1.00 and 0.86 in the same run.

The same change reads the header's component types once per scan
rather than once per constructor. A separate timing of the `nargs`
walk alone on the top rung's shape, the minimum of 31 walks, read
44 ms before and 29 ms after for Java, and 35 ms and 21 ms for Groovy.
The exponent does not move, so no probe sees it;
`java_header_types_are_read_once_per_gate_evaluation` and its Groovy
twin count the reads.

[record-header]: https://github.com/dekobon/big-code-analysis/issues/1842
[roles-followups]: https://github.com/dekobon/big-code-analysis/issues/1863

`loc/python-deep-paren-extra-prefix` and
`loc/python-block-comments-paren-strings` guard the Python `loc`
question "does code precede this node on its logical line". A bare `\`
run inside a parenthesised docstring asks it of its parenthesis, once
per parenthesis. The answer is read at the nearest level, from the
node's own upward, that holds a token before its node; a level holding
only comments and splices defers to the one above.

The two probes exist because each of the two ways to answer it wrongly
is linear on the other's shape:

- **A climb per asker.** Every earlier Python parenthesis probe grows
  either the nesting or one level's prefix. With both, no level
  answers, so each of `depth` parentheses climbed to the root:
  `depth² / 2` levels under [CPython](https://www.python.org/)'s
  200-level cap. `loc/python-deep-paren-extra-prefix` renders a
  docstring `depth` parentheses deep, each opening with 30 one-row `\`
  runs that each join a comment row. Thirty is the most that keeps a
  parenthesis under `WIDE_PARENT`, so a level's read is a seek over the
  parenthesis's children rather than a slice of the walk's sibling
  cache, and the number of levels read shows in the timing.
- **A fold down the chain.** The first fix folded the answer on a
  `FoldCache` lane, as `has_ancestor_of_kind` does. A fold evaluates
  every ancestor, the ones above the nearest answer included, and
  stores an ancestor's state under the child on the path. That is free
  where the step is a kind test. Here a level's answer is a scan of the
  extras before it, so a level above the answering one was scanned
  again for every child asked from.
  `loc/python-block-comments-paren-strings` renders an `if` block that
  opens with `width` comment rows and then holds `width` parenthesised
  strings. Each parenthesis's own statement answers; the block's level
  above it is the scan of the `width` comments, which only the first
  statement needs.

`Ancestors::nearest_level_answer` now memoises the climb per level
(`LevelQuestion`): it stops at the first level that answers or already
holds an answer, and hands what it found to the levels it passed. A
level is read at most once per walk, and never above the nearest
answer. The run asks from itself, so its parenthesis is a level like
the rest.

| Walk put back | Probe that fails | Clean | Regressed | Top rung |
|---|---|---|---|---|
| the climb per parenthesis | `loc/python-deep-paren-extra-prefix` | 0.99 | 1.96 | 3.5 ms → 100 ms |
| the fold down the chain | `loc/python-block-comments-paren-strings` | 1.00 | 2.02 | 26 ms → 763 ms |

One run each, on a host that was not idle. With the climb put back
`loc/python-block-comments-paren-strings` reads 1.08, and with the fold
`loc/python-deep-paren-extra-prefix` reads 1.06: each passes the
other's defect. In the climb run `loc/python-paren-bare-strings`,
`ops/python-paren-bare-strings`, `halstead/python-nested-paren` and
`loc/python-bare-runs-before-string` read 0.93, 0.86, 1.12 and 1.06.
The per-change guard is
`python_code_before_a_parenthesis_reads_each_level_once` in
`src/metrics/loc.rs`, which pins the exact number of levels read on
both shapes at size 40: 42 and 81 memoised, one per level; 900 and 81
climbed; 120 and 122 folded. Under `--cfg chain_audit` every memoised
answer is also checked against the climb.

Before putting a question on `fold_down`, check what its step costs:
the step runs for every ancestor on an asker's chain, once per child
the walk asks from. A step that scans is a fold's quadratic.

The climb sweep above was re-run for this change and each match
re-read, not re-timed. Every other match stops within a fixed distance
of all but one asker per subtree, is capped, or already folds where a
walk lends a cache. The one exception was filed before this change:
`python_is_bound_self_attribute` ([#1867][python-npa]).

[python-npa]: https://github.com/dekobon/big-code-analysis/issues/1867

`abc/js-nested-default-patterns` guards [#1868][climb-audit]. JS-family
ABC asks every `=` whether it initialises a `const` binding, which is
no assignment. The `=` of a declarator is answered by its parent. A
destructuring default's `=` (`[a = 1]`, `{a = 1}`) is answered by the
first ancestor that is no pattern layer, and patterns nest without
bound with a default at every level, so a climb from each of `depth`
defaults read `Σ depth` levels: under a declaration, a parameter list
and a destructuring assignment alike, in all four grammars. No earlier
probe rendered a pattern nest; `abc/wide-let-declarators` grows the
declarator list.

Each pattern level's answer is now memoised on
`LevelQuestion::InConstBinding` through
`Ancestors::nearest_level_answer`. The `=`'s own parent is read
directly, outside the memo, so the `=` of a declarator or an assignment
costs what it did.

The probe renders `let [a = 1, [a = 1, … b = 2]] = v;`, one default per
level, and `abc/js-nested-patterns` the same nest with the defaults
removed, both on 4 000 / 8 000 / 16 000.

| Walk put back | Probe that fails | Clean | Regressed | Top rung |
|---|---|---|---|---|
| a climb per default | `abc/js-nested-default-patterns` | 1.18 | 2.66 | 9.3 ms → 2.1 s |
| (same patch) | `abc/js-nested-patterns` | 0.84 | 0.78 | 6.0 ms → 5.8 ms |

One run each, on a host that was not idle. No other probe left its
bound with the climb put back. The regressed exponent is above 2 for
the reason the #1864 probes' was: the chain a deep default climbs
outgrows cache.

The per-change guard is
`js_family_const_binding_reads_each_pattern_level_once` in
`src/metrics/abc.rs`. It pins the exact number of levels read over a
40-level array nest and a 40-level object nest, in each of the four
grammars: 83 and 123 memoised, 943 and 1 763 climbed. Under
`--cfg chain_audit` every memoised answer is also checked against the
climb.

[climb-audit]: https://github.com/dekobon/big-code-analysis/issues/1868

### Child scans, and where the allocation actually was {#child-scans}

`Node::children` builds a `tree_sitter::TreeCursor`, which heap-allocates
its stack and frees it on drop, so a traversal that calls it per node
pays a `malloc`/`free` pair per node. [#1112][child-cursor] expected that
cost to be spread across the walk's predicates, on the strength of ~137
call sites. Counting it says otherwise. Per-language, over the corpus
slice, a full `metrics()` walk reaches `children` on:

| Language | calls / node |
|---|---|
| C++ (`.cc`) | 0.031 |
| Rust, JavaScript | 0.040 |
| Java | 0.058 |
| C# | 0.164 |
| **Python** | **0.600** |

The predicates are a rounding error; one scan was not. Python's
instance-attribute walk in `metrics::npa::python` visits every node of
every method body, and was 381 k of that language's 417 k child scans,
92 % of them. The metric walk's other per-node `children` consumers are
`Preorder` and the suppression DFS. Crate-wide there are two more,
`Search::act_on_node` and `output::dump`'s tree renderer, neither of
which a `metrics()` call runs, so they are absent from the figures
above.

`Node::children_with` lets all five hoist one cursor out of their loop,
and the counter `child_scan_cursors` in
`big-code-analysis-ast/src/node.rs` is what keeps them there, since the
change moves no metric value. All five are
asserted: the walks reachable from `node.rs` in
`the_converted_traversals_scan_a_tree_on_one_cursor`, and the renderer
in `output::dump`'s own `dump_holds_one_cursor_for_the_whole_tree`.
The counter records in `Node::children`, the allocating form, so a
hoisted cursor records **zero** and a per-node one records once per
interior node; each assertion pins the exact zero, and each was
verified by reverting its call site and watching that test alone fail
(50 cursors over 50 nodes for `act_on_node`, 12 over 34 for the dump).

Measured on 400 Python corpus files (694 k nodes), interleaved
best-of-nine: **414 620 cursor allocations down to 33 328 (−92 %), walk
time 159.0 ms to 155.2 ms (−2.4 %)**. Every other language is under 1 %,
which is the useful part of the result: the remaining call sites are
predicates holding a bare `&Node`, and threading a cursor to them would
cross the `Checker` / `Getter` trait surface to save roughly 17 ns per
call on the 3-6 % of nodes above (16 % in C#). Not worth it; measure
before widening this.

[child-cursor]: https://github.com/dekobon/big-code-analysis/issues/1112

### The chain audit {#chain-audit}

`make chain-audit` re-runs the library tests with the exact
ancestor-chain assertion restored. Run it around any change to a walk's
truncate/push bookkeeping.

The chain those bounds depend on is only as good as the walker's
truncate/push bookkeeping, and `Ancestors::parent` reads `chain.last()`
without checking it, so a slip feeds every predicate a wrong ancestor
silently rather than failing. The check every walk constructor makes
(`Ancestors::for_walk_chain_only`, which the others build on) is the
guard, and until [#1122][chain-audit-issue] it asked the exact question:
`chain.last() == node.parent()`. That is `Node::parent` again, per node,
on all five walks that build a chain, which made every *debug* walk
quadratic while the shipped one stayed linear, and put the cost in every
`cargo test`. The assertion made the deep-nesting regression tests
especially slow, even though release builds did not contain it.

The exact assertion now lives behind `--cfg chain_audit`:

```bash
make chain-audit
```

Removing it from the default build took the lib suite from ~5.0 s to
~1.7 s, `cognitive_nesting_is_inherited_at_depth` from ~1.6 s to ~0.02 s,
and `deeply_nested_spaces_convert_to_wire_without_stack_overflow` from
~1.8 s to ~0.02 s.

What a plain debug build keeps is an `O(1)` consequence of the same
invariant (a parent's byte span contains its child's, and no node is
its own parent), which catches a `push` moved ahead of the per-node
computes and a dropped `truncate`, but not a chain that is short by
exactly one (a grandparent contains the node too). That gap is the
lane's whole justification: run `make chain-audit` around any change to
a walk's chain bookkeeping, not just around a change to its cost. The
`chain-audit` CI job runs it per PR over the library's tests.

[chain-audit-issue]: https://github.com/dekobon/big-code-analysis/issues/1122

### What `ops/nested-fn` does not cover {#ops-vocabulary}

`ops/nested-fn` is the only probe that runs `ops_inner`, and it covers
that walk's per-node cost: the space stack, the Halstead map merge up
it, and the per-space vocabulary render. It cannot cover the *size* of
that vocabulary, and no probe can.

`Ops` publishes a `Vec<String>` of distinct operands and operators per
space, and the walk merges each child space's Halstead maps into its
parent, so a parent's vocabulary is a superset of every descendant's. A
file with `D` nested spaces and a fresh identifier at each level
therefore has `O(D²)` entries in its *output*, and any implementation
that produces that output is quadratic. Measured at depth 2 000 on such
a shape, `Ast::ops` costs ~0.6 s and returns ~6 million vocabulary
entries. Making that linear needs a shared, interned representation in
the public type, which is a breaking change (#1110).

`nested_fns` sidesteps it by reusing one identifier at every level, so
the vocabulary is four operands at any depth and the timing is the walk
alone.

### The ranking pass {#ranking-pass}

`vcs::score::apply_percentile` re-ranks every file's ten signals to a
mid-rank percentile within the analyzed set. Until [#1267][ranking-pass]
it took both tie counts — how many files sit strictly below a value,
how many within the tie tolerance of it — by scanning the whole
population per file, per signal: `O(signals × n²)`, re-paid on every
cache hit and on every `bca vcs trend` point. Measured on the three
`vcs/*` shapes before the fix: exponents **2.00 / 1.98 / 1.98**, with
the 8 000-file cell at ~100 ms in release. The pass now sorts once per
signal and takes the counts by binary search, fitting **1.14 / 1.09 /
1.15** at ~1.5–7 ms for the same cell. The bound is 1.5: a sort is
`O(n log n)`, which fits a little above 1.0 over a ladder this short,
and the old form sits at 2.0 with nothing in between.

The three shapes exist because a mid-rank implementation's cost can
depend on the tie structure in ways a single shape hides. A per-element
scan is quadratic on every shape; a per-tie-group walk would be linear
on `all-equal` and quadratic on `spread`. All three on the ladder is
what pins the class rather than one shape's constant.

The reading is the number of distinct `risk_score` values the pass
produced, taken once per cell outside the timed repetitions — it
sorts, and inside the loop it would fold a second `O(n log n)` term into
every sample. A reading of 1 on `spread` would mean the pass collapsed
every file to one score; `readings_follow_the_tie_structure` in
`big-code-analysis-bench/src/vcs.rs` asserts the three readings grow
with the shape.

[ranking-pass]: https://github.com/dekobon/big-code-analysis/issues/1267

## Criterion measurements

Three groups:

- `corpus/parse` is `tree_sitter` parse cost over the corpus slice,
  the baseline the walk sits on top of.
- `corpus/walk` is one benchmark per metric family over the
  already-parsed slice. This is the number to quote when a change
  claims to make a metric cheaper.
- `shape/walk` is the scaling shapes at a single size, so a
  constant-factor change on a pathological input is visible even when
  its complexity class did not move.

The slice is bounded and deterministic: sorted traversal, a per-
language file quota, and size limits, which given the pinned submodule
commits select the same files on every run. Directories are deduplicated
by canonical path, because `Path::is_dir` follows symlinks and the
corpus contains aliased subtrees; without it the same file is selected
twice under two paths, which is how the Java bucket first reported
sixteen files that were really eight. Before measuring anything,
the bench prints what it selected, and says so explicitly when the byte
ceiling cut a language short. From one run:

```text
corpus slice: 177 files, 903 KiB, from 3 root(s)
  root  .../tests/repositories/serde
  ...
  rust          16 files     224 KiB
```

Quote that block alongside any number taken from a run. A published
measurement from the #1052 / #1062 work described its input as "2862
Python files" when the tree it walked was 78% C/C++, and nobody could
check it because the input was never reported.

Not every supported language appears. The three corpus submodules
carry no Elixir, Lua, PHP, Ruby, or Tcl, so those languages are
covered by the synthetic shapes only.

### Comparing two builds

```bash
git switch main
cargo bench -p big-code-analysis-bench --bench metric_walk -- --save-baseline before
git switch my-change
cargo bench -p big-code-analysis-bench --bench metric_walk -- --baseline before
```

Criterion prints a change interval and a p-value per benchmark, and
writes `target/criterion/report/index.html`.

Report the interval, not its midpoint. A "~26%" improvement quoted
from min-of-5 single runs during the #1052 / #1062 work re-measured to
a bootstrap interval spanning roughly 7% to 41%.

### When the host is not idle {#interleaved-ab}

The recipe above runs the two builds one after the other, several
minutes apart. That is fine on a quiet machine and useless on a busy
one, because anything the host starts or stops in the gap is
attributed to your change. A sequential pair taken during
[#1069][interleave] read `corpus/walk/tokens` 23% faster and
`corpus/walk/halstead` 19% slower between two builds that differ only
in a hash function neither benchmark reaches.

Interleave instead. Keep a binary per side. A paired series is
usually compared across sessions, and the base binary is slow to
rebuild, so the copy goes under `/var/tmp/bca/<issue>/` rather than
`/tmp` (see
[`AGENTS.md`](../../AGENTS.md), "Temporary files"):

```bash
mkdir -p /var/tmp/bca/<issue>
bin_dir=$(mktemp -d -p /var/tmp/bca/<issue> bench-bin.XXXXXX) && echo "BIN_DIR=$bin_dir"
exe=$(cargo bench -p big-code-analysis-bench --bench metric_walk --no-run \
  --message-format=json 2>/dev/null \
  | jq -r 'select(.reason == "compiler-artifact" and .target.name == "metric_walk") | .executable')
cp "$exe" "$bin_dir/metric_walk_before"
cat >> "$bin_dir/PROVENANCE.md" <<EOF
metric_walk_before: $(git rev-parse HEAD) at $(date -u +%FT%TZ)
  built:      cargo bench -p big-code-analysis-bench --bench metric_walk --no-run
  regenerate: git checkout $(git rev-parse --short HEAD) && the same command, then cp the executable
EOF
```

Take the executable's path from cargo's JSON rather than a glob:
`target/release/deps/metric_walk-*` matches the `.d` dependency file
as well, so `cp` gets two sources and fails. The `BIN_DIR=` line is
what a later session reads, since the variable does not survive the
tool call.

Then alternate them round by round, flipping which side goes first so
a one-way drift does not always land on the same arm, and narrow each
run to the benchmarks under test:

```bash
taskset -c 3 "$bin_dir/metric_walk_before" --bench --discard-baseline --noplot \
  --sample-size 30 --warm-up-time 1 --measurement-time 3 'corpus/walk/loc'
```

Summarise the result as a *paired* series: one after/before ratio per
round, reported by its median and by how many rounds went the right
way. A sign test over those rounds is the honest headline, because a
preempted core produces single-round ratios of 5x or 0.2x that no mean
survives. Keep the controls in the same table: a control that moves
as far as the effect means the run measured the host, not the change.

`taskset` is the smaller half of this: pinning both sides to one core
removes migration as a difference between them, but it is the
interleaving that removes the load.

[interleave]: https://github.com/dekobon/big-code-analysis/issues/1069

## Measurement traps

Every one of these produced a wrong number in this repository before
the harness existed. The harness defends against the first three
structurally; the fourth is on you.

1. **An indented generator makes the input grow quadratically.** A
   Python nested-`def` generator produced 32 MB at depth 4000, so
   time-per-byte was flat while the headline number looked
   superlinear. Two separate measurements were invalidated. Every
   shape here emits a constant number of bytes per level, and
   `byte_growth_is_affine` fails if one stops doing so.
2. **`fd -e py | wc -l` is not the corpus.** Report what was
   analysed, which `CorpusSlice::summary` does for you.
3. **A ratio between two sizes is host-independent but not
   load-independent.** The two measurements are sequential, so a load
   spike between them skews the ratio by itself; best-of-three sheds
   bursty contention but not sustained overhead. The gate interleaves
   cells across rounds and fits three points instead of comparing two.
4. **Use a control.** Without one, a cross-build code-layout shift
   reads as a real effect. An independent replication of a #1062
   measurement saw the control move 1.02% on its own.

## Why not in CI {#why-not-in-ci}

Shared runners cannot give stable numbers, and this project has the
scar tissue to prove it. The wall-clock assertion that used to guard
`cognitive`'s nesting lookup produced a false failure in four
environments:

| Environment | Reading | Guard at the time |
|---|---|---|
| `test (windows-latest)` in CI | 10.9 s | absolute 8 s budget |
| local `make pre-commit` | 5.6x | ratio, single measurement |
| local, heavy parallel load | 3.9x | ratio, single measurement |
| `cargo llvm-cov` | 3.5x | ratio, best-of-three |

The last one is the decisive one: the `coverage` job runs in CI, so
instrumentation overhead alone redded the build.

`cognitive_nesting_is_inherited_at_depth` and
`tokens_count_holds_at_depth` therefore keep their *value* assertions,
which are host-independent, and the timing half lives here. One
consequence is worth stating plainly: with no wall-clock bound in the
unit suite, a reintroduced quadratic walk makes those tests slow
rather than red. Run `make bench-scaling` around any change to AST
traversal, `Checker`, `Getter`, or a metric's `compute`.

`.github/workflows/benchmark.yml` runs the gate quarterly and on
`workflow_dispatch`, mirroring
[mutation testing](mutation_testing.md). It files an issue on a
sustained failure. The gate is ratio-based and therefore portable, but
a runner is still a runner: confirm any failure on an idle host before
treating it as a regression.

## Related

- [`docs/development/mutation_testing.md`](mutation_testing.md): the
  other out-of-band quarterly gate.
- `big-code-analysis-bench/src/shapes.rs`: the probe set, with a
  per-probe rationale.
- `big-code-analysis-bench/src/scaling.rs`: the measurement and fit,
  with the reasoning behind interleaving and three-point fitting.
- `big-code-analysis-bench/src/verbatim.rs`: not a benchmark — the
  `verbatim-oracle` binary (`cargo build -p big-code-analysis-bench
  --bin verbatim-oracle`) that `utils/drift-oracle.py` and
  `utils/recovery-oracle.py` drive for the library-level zero-drift
  view (#1744). It lives here because this is the one dev-only crate
  that depends on the root with every grammar and never ships.

[parent-walk]: https://github.com/dekobon/big-code-analysis/issues/1084
[cognitive-parent]: https://github.com/dekobon/big-code-analysis/issues/1062
[remaining-climbs]: https://github.com/dekobon/big-code-analysis/issues/1088
[halstead-climbs]: https://github.com/dekobon/big-code-analysis/issues/1096
[attribute-scan]: https://github.com/dekobon/big-code-analysis/issues/1100
[cfg-predicate]: https://github.com/dekobon/big-code-analysis/issues/1105
[attribute-row]: https://github.com/dekobon/big-code-analysis/issues/1431
[one-attr]: https://github.com/dekobon/big-code-analysis/issues/1447
[space-merge]: https://github.com/dekobon/big-code-analysis/issues/1106
[perl-bash-probes]: https://github.com/dekobon/big-code-analysis/issues/1575
[flat-error]: https://github.com/dekobon/big-code-analysis/issues/1584
[find-call]: https://github.com/dekobon/big-code-analysis/issues/1580
[sibling-capability]: https://github.com/dekobon/big-code-analysis/issues/1601
[sibling-follow-ups]: https://github.com/dekobon/big-code-analysis/issues/1871
[dump-cursor]: https://github.com/dekobon/big-code-analysis/issues/1684
[tcl-slots]: https://github.com/dekobon/big-code-analysis/issues/1686
[find-render]: https://github.com/dekobon/big-code-analysis/issues/1416
[nested-containers]: https://github.com/dekobon/big-code-analysis/issues/1805
[enclosing-function]: https://github.com/dekobon/big-code-analysis/issues/1864
