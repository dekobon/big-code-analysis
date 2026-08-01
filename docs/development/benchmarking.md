# Benchmarking the metric walk

`big-code-analysis-bench` is the benchmark harness for the metric
walk. It exists so that a performance claim about this project can be
reproduced by someone who did not make it, and so that a change in
*complexity class* fails loudly instead of quietly making deeply
nested files unanalysable.

It has two halves, and they answer different questions.

- **The complexity-class gate** (`benches/scaling.rs`) answers "does
  doubling the nesting depth roughly double the cost?" It fails the
  process when a probe's measured exponent exceeds the bound that
  probe declares.
- **The criterion benchmarks** (`benches/metric_walk.rs`) answer "how
  fast is this, and did my change help?" They use
  [criterion][criterion], which reports a confidence interval rather
  than a point estimate and can compare two builds against a saved
  baseline.

Neither runs in per-PR CI. See [Why not in CI](#why-not-in-ci).

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
exercises one hot path — a metric selection through `Ast::metrics`, or
the operator/operand walk behind `Ast::ops`. The shape is rendered at three doubling
depths, every (probe, depth) cell is measured once per round with the
visit order rotated between rounds, and the reported figure is the
slope of `ln(time)` against `ln(depth)`. A linear walk sits near 1.0;
a quadratic one sits near 2.0.

Output looks like this:

```text
cognitive/nested-while  exponent 1.15 (bound 1.50)  ok
    depth      bytes   median ms      min ms      max ms    ns/byte   iter    reading
     1000      14017       0.759       0.723       0.771      54.17      2     500500
     2000      28017       1.792       1.658       1.834      63.98      1    2001000
     4000      56017       3.723       3.425       3.838      66.46      1    8002000
```

Read it as follows.

- `median ms` is what the fit uses. `min` and `max` bracket the
  rounds. A wide spread means the host was busy and the run should be
  repeated, not interpreted.
- `bytes` grows linearly with `depth` for every shape, by
  construction. If it did not, a walk that is linear in input size
  would read as superlinear in depth.
- `ns/byte` drifts up with depth even for a linear walk, because the
  tree outgrows cache. That drift is why the linear bound is 1.5 and
  not 1.05.
- `iter` is how many walks were folded into one timed sample. The
  cheapest cells run in a few hundred microseconds, where clock
  resolution is a visible fraction of the reading, so the harness
  repeats them until a sample reaches a millisecond.
- `reading` is the headline value the walk produced. It is there so a
  probe that stopped measuring anything is visible: a shape paired with
  a workload that scores zero on it would time the walk's fixed
  overhead and report an excellent exponent forever. A reading that
  does not grow with depth is fine as long as it is non-zero — the
  depth signal lives in the timing column — but say so in the probe's
  `rationale`, as `ops/nested-fn` does.

### What the probes cover

| Probe | Language | Hot path | Class today |
|---|---|---|---|
| `tokens/nested-paren` | Rust | inherited in-comment flag (#1052) | linear |
| `cognitive/nested-while` | C | `get_nesting_from_map` (#1062) | linear |
| `nom/nested-while` | C | metric control for the row above | linear |
| `cognitive/nested-if` | C | `Checker::is_else_if` | linear |
| `loc/nested-while` | C | shape control for the row below | linear |
| `loc/nested-declaration` | C | `Node::count_specific_ancestors` | linear |
| `nom/nested-quote` | Elixir | `elixir_is_inside_quote_block` | linear |
| `nom/nested-fn` | Rust | `FuncSpace` nesting; metric control for the row below | linear |
| `cognitive/nested-fn` | Rust | `increment_function_depth` (#1062) | linear |
| `ops/nested-fn` | Rust | the `Ast::ops` walk (#1110) | linear |
| `loc/nested-fn` | Rust | shape control for the row below | linear |
| `loc/nested-fn-rows` | Rust | `Ploc::merge` / `Cloc::merge` row-set union (#1109) | linear |
| `nom/nested-fn-rows` | Rust | metric control for the row above | linear |
| `nom/nested-declared-function` | JavaScript | shape control for the row below | linear |
| `nom/nested-arrow` | JavaScript | JS-family `is_func` / `is_closure` (#1088) | linear |
| `halstead/nested-paren` | Rust | shape control for the row below | linear |
| `halstead/nested-not` | Rust | `Getter::get_op_type`'s parent read (#1096) | linear |
| `abc/nested-block` | C | shape control for the row below | linear |
| `abc/nested-if` | C | the C-family ABC container walker (#1096) | linear |
| `cyclomatic/nested-and` | Python | shape control for the row below | linear |
| `cyclomatic/nested-ternary` | Python | `Node::parent_grandparent_match` (#1096) | linear |
| `loc/nested-quote` | Elixir | `loc`'s Elixir catch-all arm (#1096) | linear |
| `nom/nested-attributed-fn` | Rust | the `exclude_tests` outer-attribute scan (#1100) | linear |
| `nom/nested-cfg-predicate` | Rust | the `cfg(...)` predicate classifier (#1105) | linear |

Four of these were quadratic when the harness landed, and they shared
one cause: `tree_sitter` stores no parent pointer, so `Node::parent`
resolves by descending from the root and is itself `O(depth)`. Any
predicate in the walk that asked a node for its parent was therefore
`O(depth)` per node and `O(depth^2)` over a deeply nested file, however
few steps it took. [#1084][parent-walk] fixed three of them by having
the metric walk carry the ancestor chain down with it (`Ancestors` in
`src/node.rs`), so a predicate reads an ancestor as a slice index. Their
bounds moved to the linear bound in that same change, which is what now
catches a relapse. `cognitive/nested-fn` is the fourth of the same
family: `increment_function_depth` was deferred out of #1084 and fixed
the same way in [#1062][cognitive-parent], which is also where that
probe comes from — it fitted 2.04 against the climb and 1.21 against the
chain.

`nom/nested-arrow` is the fifth, added by [#1088][remaining-climbs] with
the fix it guards. It cost the same family a second lesson: threading
the chain into the JS-family predicates was *not enough* to make it
linear. The walk they run is two steps long on that shape, and the
remaining `O(depth)` term was hiding in `Node::wraps_any`, which scanned
a node's children with `child(0)` + `next_sibling()`. A sibling step
resolves its parent, so that scan was `O(children × depth)` — the same
defect one level down, in a helper whose doc comment asserted it was
`O(1)` per step. Moving the scan onto a cursor took the probe from 17.6 s
to 6.3 ms at depth 4000 (`k` 1.97 to 1.03), and a walk over the 384-file `pdf.js` corpus
from ~443 ms to ~370 ms. The lesson generalises: a chain-fed predicate
is only as linear as the primitives it calls.

The last three probes come from [#1096][halstead-climbs], which
retired the remaining `Node::parent` calls in the per-language metric
bodies: the Halstead `get_op_type` getters, every `Abc` impl, and the
`loc` / `npm` / `npa` / `cyclomatic` / `is_useful_comment` arms.
`Halstead::compute`, `Abc::compute`, `Npm::compute`, `Npa::compute`,
`Cyclomatic::compute`, and `Checker::is_useful_comment` gained an
`Ancestors` parameter there, and the comment-removal walk grew a chain
of its own. A fourth call the issue had not listed turned up
during review — Python's `Cyclomatic` `else` arm, reached through
`Node::parent_grandparent_match`, which no `rg '\.parent\(\)'` sweep
finds at the call site. All four probes fitted 1.99-2.06 before and
0.99-1.27 after; at depth 4000 `halstead/nested-not` dropped from
~478 ms to ~0.57 ms, `abc/nested-if` from ~808 ms to ~3.3 ms,
`cyclomatic/nested-ternary` from ~1.13 s to ~3.4 ms, and
`loc/nested-quote` from ~9.6 s to ~9.2 ms. Their controls held at
0.97-1.31 either side.

That change also confirmed #1088's lesson a second time from the other
direction. The ABC condition walkers reach a slot's parent to decide
whether it sits in boolean context, and the same walkers ask
`Node::previous_sibling` whether a ternary's `?` / `:` precedes the
operand — and `ts_node__prev_sibling` opens with `ts_node_parent`, so
it carries the same `O(depth)`. Passing the parent in and scanning its
children (`Node::previous_sibling_under`) was needed for the fix to
hold on a ternary shape, not only on the shapes the probes render.

The `Ancestors::unknown()` call sites that remain are deliberate rather
than deferred: the two synthetic-`Unit`-root pushes hand it a node that
*is* the root, `parser.rs`'s `--filter function` predicate is applied
outside any walk, and the `Npm` arms that test a node's children cannot
extend a borrowed slice by one element without allocating.

The last two probes are the first that walk under a non-default
`MetricsOptions`. Both hot paths are reachable only with
`exclude_tests` set, which `Workload::Metrics` now carries as its own
field — it sits on that variant rather than on `Probe` because
`Ast::ops` takes no options at all, so an `Ops` probe setting it would
be setting something nothing reads.

`nom/nested-attributed-fn` guards [#1100][attribute-scan]. Under
`exclude_tests` the walker asks every node whether it opens a test-only
subtree, and Rust answers by reading the run of `#[…]` siblings before
an item. Walking that run backward with `Node::previous_sibling` costs
`O(attributes x depth)`, so the probe fitted 2.00 and took 2.05 s at
depth 4000 against 8.3 ms for `nom/nested-fn`, the same nesting without
the attributes. Reading the run forward from the parent the walker
already holds took it to 1.21 and 8.1 ms — level with that control.

That fix trades one axis against the other, and the probe alone would
not have caught why. A forward pass over the parent's children is
`O(children)` and flat in depth; the backward walk is the reverse. The
measured costs behind that trade, and the break-even they put it at,
live on `MAX_FORWARD_ATTRIBUTE_SCAN_CHILDREN` and
`FORWARD_ATTRIBUTE_SCAN_CHILDREN_PER_DEPTH` in `src/checker.rs` — one
copy, so re-measuring updates one place. Reading forward
unconditionally fixed the depth axis and broke the width one: a
generated file of 2 000 top-level attributed items went from 6.0 ms to
569 ms, which is the shape `bindgen` output has. The scan now budgets
the parent's child count against the node's depth, so it reads forward
only where that is the cheaper bound and a shallow wide parent keeps
exactly the walk it had.

**The width axis is not guarded here.** No probe renders the wide
shape. A fixed-depth width sweep would grow its input affinely, as the
module requires, but it does not nest — and `shapes.rs` asserts that
every probe's shape gains AST levels in proportion to its parameter
(`shapes_nest_proportionally_to_depth`), which such a sweep would fail
by construction. What the unit suite pins
is the *dispatch*, not its cost —
`the_exclude_tests_prune_reads_forward_up_to_its_depth_scaled_budget`
in `src/node.rs` asserts which arm each boundary shape takes, so
widening the budget past a shallow parent fails a test rather than
slipping through. The numbers the budget is derived from have no
automated guard at all; re-measure by hand when touching either
constant.

`nom/nested-cfg-predicate` guards [#1105][cfg-predicate] and is the only
probe that grows one *attribute* rather than the code around it:
`#[cfg(all(all(… test …)))]` is classified by a string-level
mini-parser reading the attribute's text, which re-scanned each
region's whole interior to find its split points. It fits 1.00 against
the indexed scan that replaced it. `nom/nested-fn` is not a control for
it — the file keeps its two items at every depth — so the bound alone
is the guard.

Treat the linear bounds above as covering the walk's ancestor *chain*
threading, not every `O(depth)` lookup in the crate.

### Child scans, and where the allocation actually was {#child-scans}

`Node::children` builds a `tree_sitter::TreeCursor`, which heap-allocates
its stack and frees it on drop — so a traversal that calls it per node
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
every method body, and was 381 k of that language's 417 k child scans —
92 % of them. The metric walk's other per-node `children` consumers are
`Preorder` and the suppression DFS. Crate-wide there are three more —
`Search::first_occurrence`, `Search::act_on_node`, and `output::dump`'s
tree renderer — none of which a `metrics()` call runs, so they are
absent from the figures above.

`Node::children_with` lets all six hoist one cursor out of their loop,
and the counter `child_scan_cursors` in `src/node.rs` is what keeps
them there, since the change moves no metric value. All six are
asserted: the walks reachable from `node.rs` in
`the_converted_traversals_scan_a_tree_on_one_cursor`, and the renderer
in `output::dump`'s own `dump_holds_one_cursor_for_the_whole_tree`.
The counter records in `Node::children` — the allocating form — so a
hoisted cursor records **zero** and a per-node one records once per
interior node; each assertion pins the exact zero, and each was
verified by reverting its call site and watching that test alone fail
(50 cursors over 50 nodes for the two `Search` walks, 12 over 34 for
the dump).

Measured on 400 Python corpus files (694 k nodes), interleaved
best-of-nine: **414 620 cursor allocations down to 33 328 (−92 %), walk
time 159.0 ms to 155.2 ms (−2.4 %)**. Every other language is under 1 %,
which is the useful part of the result — the remaining call sites are
predicates holding a bare `&Node`, and threading a cursor to them would
cross the `Checker` / `Getter` trait surface to save roughly 17 ns per
call on the 3-6 % of nodes above (16 % in C#). Not worth it; measure
before widening this.

[child-cursor]: https://github.com/dekobon/big-code-analysis/issues/1112

### The chain audit {#chain-audit}

The chain those bounds depend on is only as good as the walker's
truncate/push bookkeeping, and `Ancestors::parent` reads `chain.last()`
without checking it — so a slip feeds every predicate a wrong ancestor
silently rather than failing. `Ancestors::checked` is the guard, and
until [#1122][chain-audit-issue] it asked the exact question:
`chain.last() == node.parent()`. That is `Node::parent` again, per node,
on all five walks that build a chain — which made every *debug* walk
quadratic while the shipped one stayed linear, and put the cost in every
`cargo test`. The deep-nesting regression tests paid it worst, which is
to say the tests that exist to pin the walk's linearity were the slowest
thing about not shipping it.

The exact assertion now lives behind `--cfg chain_audit`:

```bash
make chain-audit
```

Removing it from the default build took the lib suite from ~5.0 s to
~1.7 s, `cognitive_nesting_is_inherited_at_depth` from ~1.6 s to ~0.02 s,
and `deeply_nested_spaces_convert_to_wire_without_stack_overflow` from
~1.8 s to ~0.02 s.

What a plain debug build keeps is an `O(1)` consequence of the same
invariant — a parent's byte span contains its child's, and no node is
its own parent — which catches a `push` moved ahead of the per-node
computes and a dropped `truncate`, but not a chain that is short by
exactly one (a grandparent contains the node too). That gap is the
lane's whole justification: run `make chain-audit` around any change to
a walk's chain bookkeeping, not just around a change to its cost. The
`chain-audit` CI job runs it per PR over the library's tests.

[chain-audit-issue]: https://github.com/dekobon/big-code-analysis/issues/1122

[parent-walk]: https://github.com/dekobon/big-code-analysis/issues/1084
[cognitive-parent]: https://github.com/dekobon/big-code-analysis/issues/1062
[remaining-climbs]: https://github.com/dekobon/big-code-analysis/issues/1088
[halstead-climbs]: https://github.com/dekobon/big-code-analysis/issues/1096
[attribute-scan]: https://github.com/dekobon/big-code-analysis/issues/1100
[cfg-predicate]: https://github.com/dekobon/big-code-analysis/issues/1105

The ten control probes are what make the other readings mean
something.

- `nom/nested-while`, `nom/nested-fn` and `nom/nested-fn-rows` are
  **metric** controls.
  `Cognitive` declares `Nom` as a dependency in `src/metric_set.rs`, so
  the cognitive-attributable cost of each `cognitive/…` row is its
  difference from the `nom/…` row on the same shape, not the
  `cognitive` reading alone. `nom/nested-fn-rows` plays the same part
  for `loc/nested-fn-rows`: `Nom`'s merge is a counter add, so the
  difference is the cost of `Loc`'s per-space row sets.
- `loc/nested-while`, `cognitive/nested-while`,
  `nom/nested-declared-function`, `halstead/nested-paren`,
  `abc/nested-block`, `cyclomatic/nested-and` and `loc/nested-fn` are
  **shape** controls: each is the same nesting as the ancestor-walk
  probe it sits next to, with the one node that triggers the walk
  removed. Before [#1084][parent-walk] each fitted near 1.0 where its
  counterpart fitted near 2.0, which is what attributed the quadratic
  cost to that call rather than to nesting in general. Now that all
  twenty-four fit near 1.0, the pair is what would localise a relapse: a
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
  of the shape with the trigger removed. `nom/nested-quote` — the same
  source under a metric that does not ask for a parent — is the metric
  control that stands in for one.

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

### Adding a probe

Add a `Probe` to `PROBES` in `big-code-analysis-bench/src/shapes.rs`.
The unit tests in that module enforce what a probe has to satisfy:
bytes affine in depth, no parse errors, AST depth growing with the
depth parameter, a non-zero workload reading, and depths that double.
Set `max_exponent` from a measurement on an idle host, not from
theory, and say in `rationale` which call the probe is watching.

## Criterion measurements

Three groups:

- `corpus/parse` is `tree_sitter` parse cost over the corpus slice,
  the baseline the walk sits on top of.
- `corpus/walk` is one benchmark per metric family over the
  already-parsed slice. This is the number to quote when a change
  claims to make a metric cheaper.
- `shape/walk` is the depth-scaling shapes at a single depth, so a
  constant-factor change on a pathological input is visible even when
  its complexity class did not move.

The slice is bounded and deterministic: sorted traversal, a per-
language file quota, and size limits, which given the pinned submodule
commits select the same files on every run. Directories are deduplicated
by canonical path, because `Path::is_dir` follows symlinks and the
corpus contains aliased subtrees — without it the same file is selected
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

Interleave instead. Keep a binary per side:

```bash
cargo bench -p big-code-analysis-bench --bench metric_walk --no-run
cp target/release/deps/metric_walk-* /tmp/metric_walk_before
```

Then alternate them round by round — flipping which side goes first,
so a one-way drift does not always land on the same arm — and narrow
each run to the benchmarks under test:

```bash
taskset -c 3 /tmp/metric_walk_before --bench --discard-baseline --noplot \
  --sample-size 30 --warm-up-time 1 --measurement-time 3 'corpus/walk/loc'
```

Summarise the result as a *paired* series: one after/before ratio per
round, reported by its median and by how many rounds went the right
way. A sign test over those rounds is the honest headline, because a
preempted core produces single-round ratios of 5x or 0.2x that no mean
survives. Keep the controls in the same table — a control that moves
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
3. **A ratio between two depths is host-independent but not
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

- [`docs/development/mutation_testing.md`](mutation_testing.md) — the
  other out-of-band quarterly gate.
- `big-code-analysis-bench/src/shapes.rs` — the probe set, with a
  per-probe rationale.
- `big-code-analysis-bench/src/scaling.rs` — the measurement and fit,
  with the reasoning behind interleaving and three-point fitting.
