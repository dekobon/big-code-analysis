# Baseline freshness

`.bca-baseline.toml` records every current threshold offender so the
gate can ratchet: a listed function fails only once it measures *worse*
than its recorded value. That works in one direction. When a function
improves, the recorded value keeps describing a tree that no longer
exists, and the gap between the two is gate headroom nobody chose — a
later regression can grow back into it without tripping anything.

Since #1465 a gated run warns about part of this on stderr. It cannot
warn about the rest: the warning is emitted per *violation*, from
`filter_by_baseline`, so a function that stopped breaching its
threshold altogether produces nothing to warn about. Its entry sits in
the file, inert and invisible, until someone regenerates. That
regeneration is what `.github/workflows/baseline-freshness.yml` does,
four times a year.

## The scheduled workflow {#the-scheduled-workflow}

It runs on `cron: '0 7 22 1,4,7,10 *'` — 07:00 UTC on the 22nd of
January, April, July, and October — and on `workflow_dispatch`. The
four out-of-band jobs take one day each so no two contend for a runner:
[mutation testing](mutation_testing.md) on the 1st,
[fuzzing](fuzzing.md) on the 8th, [benchmarks](benchmarking.md) on the
15th, this a week after the last of them.

The job:

1. Checks out the repo **without** submodules. `.bcaignore` excludes
   `tests/repositories/` from the walk, so the corpora contribute no
   baseline entries.
2. Copies the committed `.bca-baseline.toml` aside, then runs
   `make self-scan-write-baseline-headroom` to regenerate it.
3. Diffs the two with `bca diff-baseline --format markdown
   --exit-code`.
4. Uploads both baselines and the diff as the `baseline-freshness`
   artifact (90-day retention).
5. On a non-empty diff, fails the job and opens — or comments on — a
   GitHub issue labelled `self-scan`.

It needs `issues: write` and `GITHUB_TOKEN`; no other secrets.

## Why it deduplicates {#why-it-deduplicates}

The other three out-of-band jobs call `gh issue create`
unconditionally, with the run id in the title, because each reports a
fresh measurement. A stale baseline is not a measurement: it persists
until someone commits a refresh, so an unconditional create would open
one issue per quarter for the same unfixed file. This job looks for an
open issue labelled `self-scan` first and comments on it with the
current diff when there is one. The title therefore carries no run id —
the issue outlives the run that opened it.

## Why `diff-baseline` and not `git diff` {#why-diff-baseline}

`git diff --exit-code .bca-baseline.toml` would report line drift as
staleness. A v6 baseline records a `start_line` only where it is
consulted — an identity triple two spaces share, such as two methods
named `is_valid` on different `impl` blocks — and for those, editing
code above the function rewrites the entry without changing what it
records. `bca diff-baseline` pairs entries on
`(path, qualified, metric)` and ignores `start_line`, so it reports
what actually changed — and it buckets the result the way
[the book's baselines recipe][recipe] already talks about it:

| Bucket | What it means here |
|--------|--------------------|
| `improved` | The offender is still over its limit, but by less. The `--baseline` warning already covers this one. |
| `removed` | The offender stopped breaching altogether. This is the half nothing else can see. |
| `added` | A new offender the committed baseline does not list — the hard gate is red on `main` too. |
| `worsened` | An offender past its recorded value — likewise already red. |

On a green `main` only the first two can appear.

[recipe]: ../../big-code-analysis-book/src/recipes/baselines.md

## Running it by hand {#running-it-by-hand}

The job is three commands, and they are the same ones locally:

```bash
cp .bca-baseline.toml /tmp/old-baseline.toml
make self-scan-write-baseline-headroom
cargo run --quiet --release -p big-code-analysis-cli -- \
  diff-baseline /tmp/old-baseline.toml .bca-baseline.toml --format markdown
```

Drop `--exit-code` — as above it is informational; with the flag a
non-empty diff exits 2 and a tool error exits 1.

Always the `-headroom` target, never the bare
`self-scan-write-baseline`. The committed file carries
`[provenance] tier = "soft", headroom = 0.95`, and a baseline written
at the hard tier omits the soft-tier offenders, so `make
self-scan-headroom` would then fire on files nobody touched.

## Responding to the issue {#responding-to-the-issue}

Refresh the file and commit it:

```bash
make self-scan-write-baseline-headroom
git add .bca-baseline.toml
```

There is nothing to fix in the code — the tree is fine, the file
describing it has aged. Read the diff first anyway: a `removed` entry
names a function that is no longer a threshold offender, which is worth
knowing, and an `added` or `worsened` one means the hard gate is red
and the refresh is not the whole answer.

The writer is deterministic, so a regeneration over an unchanged tree
is byte-identical to the committed file and the job stays quiet.

## Related {#related}

- [Choosing and refreshing a baseline][recipe] — what the ratchet does
  with a stale entry, and the stderr warning that covers the other
  half.
- `AGENTS.md`, "Baseline-refresh discipline" — the in-PR duty this job
  backstops.
