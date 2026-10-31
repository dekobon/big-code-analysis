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

The job:

1. Checks out the repo with submodules.
2. Installs `cargo-mutants` via
   [`taiki-e/install-action@v2`](https://github.com/taiki-e/install-action).
3. Runs `cargo mutants` against `src/metrics/**/*.rs`,
   `big-code-analysis-ast/src/checker.rs`, and
   `big-code-analysis-ast/src/getter.rs`, under `set -o pipefail` so
   the `| tee` capturing stdout cannot mask a failing exit. The
   per-language modules under `checker/` and `getter/` are not in the
   selection; widening to them is weighed in #1726 together with
   sharding, since it would roughly double the count.
4. Uploads `target/mutants/` as the `cargo-mutants-report` artifact
   (90-day retention).
5. On non-zero exit, opens a GitHub issue labelled
   `mutation-testing` with the missed / timed-out counts and a link
   back to the run. A non-zero exit with no missed or timed-out
   mutants recorded is reported as an infrastructure failure instead
   and files nothing; the red job is the signal.

GitHub cancels a hosted-runner job at six hours. By GitHub's
documentation a cancelled step is not a `failure()`, so step 5 would
not run and nothing would be filed; the red job and the partial
`cargo-mutants-report` artifact would be all there is. No run has yet
timed out to confirm that reading. The selection is several thousand
mutants run serially (`cargo mutants --list` with the workflow's
arguments counts them) and is not expected to fit; #1726 tracks
sharding it. Until then, read a red run with no issue as the timeout
first.

The job has `issues: write` permission and uses `GITHUB_TOKEN`; no
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
mutants need separate copies, so an in-place run is serial. Note the
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
4. Timeouts often mean a mutation introduced an infinite loop. The
   `minimum_test_timeout = 120` in `.cargo/mutants.toml` guards
   against spurious timeouts on slow runners; if a specific test case
   is chronically slow, prefer fixing the test.

## When the workflow files an issue

The auto-filed issue is the canonical entry point. It contains the
missed / timed-out counts, a link to the run, and the first 50
missed lines for quick scanning. Download the
`cargo-mutants-report` artifact from the run for the full list, then
follow the triage steps above.

Close the issue once every escape has either:

- A new test that kills the mutant, **or**
- An explicit `exclude_re` entry in `.cargo/mutants.toml` with a
  justification commented beside it or in the relevant source file.
