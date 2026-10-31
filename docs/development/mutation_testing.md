# Mutation testing

`big-code-analysis` runs [cargo-mutants][cm] quarterly against every
metric implementation under `src/metrics/`, plus the two dispatcher
files `big-code-analysis-ast/src/checker.rs` and
`big-code-analysis-ast/src/getter.rs`. Mutation testing complements
the regular test suite by mechanically mutating production code
(flipping `>` to `>=`, replacing a function body with
`Default::default()`) and re-running the tests. Any mutant that
survives is a gap in test coverage: the suite did not catch a
deliberate bug.

This page covers running it locally, reading the results, and
responding to the issue the scheduled job files.

## Why quarterly, not per-PR

A full mutants run on `src/metrics/` takes hours on a GitHub-hosted
runner: every mutant rebuilds and re-tests two crates, and the
selection is in the thousands. Gating per-PR CI on it would either burn
hours of runner time on every change or force aggressive scoping that
defeats the point. Instead the workflow runs four times a year and
files an issue when escapes are detected, so the project gets a
quarterly health check without slowing day-to-day work.

[cm]: https://mutants.rs/

## The scheduled workflow

`.github/workflows/mutation-test.yml` runs:

- On `cron: '0 7 1 1,4,7,10 *'`, which is 07:00 UTC on the 1st of
  January, April, July, and October.
- On `workflow_dispatch` for ad-hoc runs from the Actions tab.

The selection is `src/metrics/**/*.rs`,
`big-code-analysis-ast/src/checker.rs` and
`big-code-analysis-ast/src/getter.rs`: several thousand mutants, each
an incremental rebuild plus two crates' test suites, run serially
because `--in-place` rules out `-j`. That is far past the six hours
at which GitHub cancels a hosted-runner job, so the run is split
across a matrix (#1726). Three jobs:

1. `prepare` declares the shard list once, as a JSON output both
   later jobs read. The count lives nowhere else.
2. `mutants` runs one matrix job per shard: checkout with submodules,
   a pinned `cargo-mutants` (every shard must generate the identical
   mutant list for the partition to be disjoint, so the version
   cannot float between installs), then `cargo mutants … --shard k/n
   --sharding round-robin` under `set -o pipefail`. Round-robin
   spreads the ast-crate mutants, which rebuild both crates, across
   every shard; the default contiguous slicing would put all of them
   in the last two. The step captures cargo-mutants' exit status, has
   `utils/mutation-report.py summarize` join that status to the result
   files into one `shard-summary-<k>.json`, then exits with the
   status so the matrix still shows which shards found escapes. The
   summary is uploaded first and separately from the shard's full
   `target/mutants/`; a cancelled or crashed shard never reaches the
   summarize line and so uploads none.
3. `report` runs after the matrix whatever its outcome, downloads
   every summary that arrived, and has `utils/mutation-report.py
   report` render one body: a per-shard table with each shard's run
   attempt, the totals across the shards that completed, the files
   with the most escapes, and the first fifty missed mutants. A shard
   with no well-formed summary is named with the reason and its
   mutants are not counted, so a timed-out shard cannot pass for a
   clean one. The body is uploaded as the `mutation-report` artifact
   before anything talks to GitHub, then filed: the issue title is
   fixed (`Mutation testing: escaped mutants`), and an open issue by
   that title gets a comment rather than a sibling, so dispatches
   while sizing the shards do not pile up near-duplicates.

The run is red whenever any shard did not complete or any mutant
escaped, and stays red until the issue is resolved, the same policy
as `baseline-freshness.yml`. Nothing filed with a red run means the
`report` job itself could not run: look at the workflow log, and at
the `mutation-report` artifact if one was uploaded.

Re-running the failed jobs is the recovery for a shard that timed
out: the other shards' artifacts persist from the earlier attempt,
the re-run shard overwrites its own, and `report` re-aggregates. The
table's attempt column shows which shards came from which attempt,
and the body says so when they differ. One case the report cannot
tell apart: a re-run shard that fails before uploading keeps its
earlier attempt's summary, which then looks like a shard that was not
re-run. When the attempt column disagrees with what you re-ran, that
is what happened.

Size the shard count from the previous complete run's slowest shard.
The step timeout is 270 minutes; if any shard's `Run cargo-mutants`
step exceeds about 200, raise the count in `prepare` before the next
cron. The current ten rests on the first complete run (41219179754,
2026-10-31): eight shards of ~475 mutants took 145 to 231 minutes,
18 to 29 seconds per mutant on `ubuntu-latest`, with three past the
200-minute mark. The laptop figure the count was first sized from,
about ten seconds per mutant, is roughly half the hosted cost. Both
halves of `utils/mutation-report.py` are covered by `make
mutation-report-test`, and the workflow's four `run:` bodies by `make
mutation-workflow-test`, which executes them verbatim under `bash -e`
with stub `cargo` and `gh` and includes the check that weakening
`pipefail` makes the summarizer refuse a masked status. Both run in
`make pre-commit`, so the result handling and the shell that feeds it
are checked on every pull request rather than once a quarter. What
GitHub evaluates rather than bash is outside both: the `if:`
expressions between steps, the `env:` plumbing into each step, the
matrix's `fromJSON`, and the artifact upload and download that fan the
summaries in. actionlint checks the expressions' syntax; the rest is
exercised only by a real run.

The jobs have `issues: write` permission and use `GITHUB_TOKEN`; no
extra secrets are required.

## Running locally

Install `cargo-mutants` once. `cargo install` takes the latest
release; 25.0 is the oldest that reads `.cargo/mutants.toml` (older
ones reject its `output` key), and 26.0 the oldest that runs mutants
in the same deterministic order CI does:

```bash
cargo install cargo-mutants --locked
```

`.cargo/mutants.toml` carries the settings local runs and CI share:
the test-timeout floor, the output directory, and the test-package
set (every mutant runs both the root crate's and
`big-code-analysis-ast`'s tests, because the checker and getter are
pinned almost entirely by the root crate's metric tests). cargo-mutants
reads it from the workspace root, so a plain `cargo mutants` picks it
up. Which packages to mutate and `--in-place` have no config-file key
and stay on the command line; which files could be configured
(`examine_globs`) but is not, because cargo-mutants 27 unions that key
with `-f` and a single-file local run would silently widen to the
whole CI surface.

To exercise a single metric file (the typical local case), from the
repository root:

```bash
cargo mutants --in-place -f src/metrics/cognitive.rs
```

For a file under `big-code-analysis-ast/`, add `--package
big-code-analysis-ast`. Without it cargo-mutants mutates only the
package the working directory is in, which at the root is the root
crate, and the `-f` then matches nothing: the run lists zero mutants
and exits 0.

To exercise the same surface as CI, which spans both the root crate
and `big-code-analysis-ast`:

```bash
cargo mutants --package big-code-analysis --package big-code-analysis-ast \
  --in-place \
  -f 'src/metrics/**/*.rs' -f big-code-analysis-ast/src/checker.rs \
  -f big-code-analysis-ast/src/getter.rs
```

Keep `--in-place`. Without it cargo-mutants copies the whole tree,
including the corpus submodules, into a scratch directory before each
run. The trade is that `--in-place` conflicts with `-j`: parallel
mutants need separate copies, so an in-place run is serial. To run
one slice of the CI surface, add the `--shard k/n --sharding
round-robin` pair the workflow uses; the partition is the same on any
machine, since the mutant list follows `mod` declaration order and
never a directory listing. Note the
`**/*.rs` on the metrics glob: a `-f` pattern containing `/` matches
the whole path, so `src/metrics/` selects nothing and `src/metrics/*.rs`
misses the per-language subdirectories. `cargo mutants --list -f …`
previews the selection.

Two cautions for an in-place run. cargo-mutants restores each file
after testing it, and sets `INSTA_UPDATE=no` on the test processes so a
caught mutant cannot write a snapshot, but check `git status` once it
finishes all the same. And the baseline run tests only the packages
that hold mutants, while each mutant then tests the packages named in
`test_package`; a narrow selection inside `big-code-analysis-ast` is
therefore baselined without the root crate's tests, and a pre-existing
root failure would read as a caught mutant. Run a plain `cargo test -p
big-code-analysis -p big-code-analysis-ast` first.

Each mutant costs an incremental rebuild plus the two crates' test
suites, around ten seconds on a fast laptop, so a metric file with a
few dozen mutants takes minutes and the larger ones tens of minutes.

## Interpreting an escaped mutant

`cargo-mutants` nests its result directory, `mutants.out/`, under the
configured `output` directory, so results land in
`target/mutants/mutants.out/` locally and in CI alike. An existing
`mutants.out/` is rotated to `mutants.out.old/` at the start of each
run.

| File | Meaning |
|------|---------|
| `missed.txt` | Mutants that survived; the suite did not catch them |
| `timeout.txt` | Tests that hit the timeout (often infinite loops, occasionally real gaps) |
| `caught.txt` | Mutants the suite correctly killed |
| `unviable.txt` | Mutants that did not compile (no signal either way) |
| `log/` | One cargo log per mutant, plus the baseline |

CI additionally captures the run's stdout as
`target/mutants/mutants.log`, beside `mutants.out/`, through the `tee`
in the workflow; a local run writes no such file.

Each line in `missed.txt` looks like:

```text
src/metrics/cognitive.rs:142:9: replace > with >= in compute
```

Triage:

1. Read the mutant location and the surrounding code.
2. If the mutation produces observably different output (different
   metric value, different control flow), add a test that pins the
   correct behaviour. This is the common case and the whole point.
3. If the mutation is genuinely behaviour-equivalent (a `match`
   arm that is provably unreachable, an unused branch in a helper
   that always early-returns), document why and add the location to
   `exclude_re` in `.cargo/mutants.toml`, where local runs and CI
   both read it. Do this sparingly: most "looks equivalent" mutants
   are real coverage gaps.
4. A timeout is not a miss: the suite noticed the mutant, by never
   finishing. Read the mutant's log under the shard artifact's `log/`
   to tell the two causes apart. If the mutation made a loop
   non-terminating, a test that bounds the loop (an iteration count,
   a deadline) kills it outright; where no such test is cheap, an
   `exclude_re` entry with the reason `non-terminating under
   mutation` is the accepted treatment, since the mutant is not
   equivalent and cannot be killed by a value assertion. If instead
   the test is merely slow, fix the test; the
   `minimum_test_timeout = 120` in `.cargo/mutants.toml` guards
   against spurious timeouts on slow runners, not against slow tests.

## When the workflow files an issue

The auto-filed issue (title `Mutation testing: escaped mutants`; a
later run comments on it while it is open) is the canonical entry
point. Each report opens by saying what a missed mutant is and what
closes the issue, so it reads on its own; then a link to the run, a
per-shard table with each shard's attempt number and counts, the
totals over the shards that completed, the files with the most
escapes with a one-line repro for the worst, and a sample of the
missed lines with a note on how to read one. A shard that did not complete is
named with the reason and its mutants are not in the totals; re-run
the failed jobs and the next report fills it in. The same body is the
run's `mutation-report` artifact. For a shard's full `mutants.out/`,
with the per-mutant logs, download its `cargo-mutants-report-<shard>`
artifact, then follow the triage steps above.

Close the issue once every escape has either:

- A new test that kills the mutant, **or**
- An explicit `exclude_re` entry in `.cargo/mutants.toml` with a
  justification commented beside it or in the relevant source file.

The confirmation is a run in which every shard completed and the
report shows zero missed and zero timed out. That run files nothing,
since the issue step is gated on a non-zero status, so read its
`mutation-report` artifact rather than waiting for a comment.
