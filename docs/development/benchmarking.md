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

Each probe pairs a generated source shape with the metric selection
that exercises one hot path. The shape is rendered at three doubling
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
- `reading` is the headline metric value the walk produced. It is
  there so a probe that stopped measuring anything is visible: a shape
  paired with a metric that scores zero on it would time the walk's
  fixed overhead and report an excellent exponent forever.

### What the probes cover

| Probe | Language | Hot path | Class today |
|---|---|---|---|
| `tokens/nested-paren` | Rust | inherited in-comment flag (#1052) | linear |
| `cognitive/nested-while` | C | `get_nesting_from_map` (#1062) | linear |
| `nom/nested-while` | C | metric control for the row above | linear |
| `cognitive/nested-if` | C | `Checker::is_else_if` | quadratic |
| `loc/nested-while` | C | shape control for the row below | linear |
| `loc/nested-declaration` | C | `Node::count_specific_ancestors` | quadratic |
| `nom/nested-quote` | Elixir | `elixir_is_inside_quote_block` | quadratic |
| `nom/nested-fn` | Rust | `increment_function_depth`, `FuncSpace` nesting | linear |

The three quadratic probes share one cause: `tree_sitter` stores no
parent pointer, so `Node::parent` resolves by descending from the root
and is itself `O(depth)`. Any predicate in the walk that asks a node
for its parent is therefore `O(depth)` per node and `O(depth^2)` over a
deeply nested file, however few steps it takes. That is tracked as
[#1084][parent-walk]; their bounds here pin the current class rather
than endorsing it, so a further degradation is still caught. When that
issue is fixed, those probes move to the linear bound in the same
change.

[parent-walk]: https://github.com/dekobon/big-code-analysis/issues/1084

The three control probes are what make the other readings mean
something.

- `nom/nested-while` is a **metric** control. `Cognitive` declares
  `Nom` as a dependency in `src/metric_set.rs`, so the
  cognitive-attributable cost is the difference between the two rows,
  not `cognitive` alone.
- `loc/nested-while` and `cognitive/nested-while` are **shape**
  controls: each is the same nesting as the quadratic probe it sits
  next to, with the one node that triggers an ancestor walk removed.
  Each fits near 1.0 where its quadratic counterpart fits near 2.0,
  which is what attributes the quadratic behaviour to that call rather
  than to nesting in general.

### Adding a probe

Add a `Probe` to `PROBES` in `big-code-analysis-bench/src/shapes.rs`.
The unit tests in that module enforce what a probe has to satisfy:
bytes affine in depth, no parse errors, AST depth growing with the
depth parameter, a non-zero metric reading, and depths that double.
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
