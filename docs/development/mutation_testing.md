# Mutation testing

`big-code-analysis` runs [cargo-mutants][cm] on a quarterly schedule
against the highest-leverage modules: every metric implementation
under `src/metrics/`, plus `src/checker.rs` and `src/getter.rs`.
Mutation testing complements the regular test suite by mechanically
mutating production code (e.g. flipping `>` to `>=`, replacing a
function body with `Default::default()`) and re-running the tests.
Any mutant that survives is a gap in test coverage — the suite did
not catch a deliberate bug.

## Why quarterly, not per-PR

A full mutants run on `src/metrics/` takes tens of minutes per file
on a GitHub-hosted runner. Gating per-PR CI on it would either burn
hours of runner time on every change or force aggressive scoping that
defeats the point. Instead the workflow runs four times a year and
files an issue when escapes are detected, so the project gets a
quarterly health check without slowing day-to-day work.

[cm]: https://mutants.rs/

## The scheduled workflow

`.github/workflows/mutation-test.yml` runs:

- On `cron: '0 7 1 1,4,7,10 *'` — 07:00 UTC on the 1st of January,
  April, July, and October.
- On `workflow_dispatch` for ad-hoc runs from the Actions tab.

The job:

1. Checks out the repo with submodules.
2. Installs `cargo-mutants` via `taiki-e/install-action@v2`.
3. Runs `cargo mutants` against `src/metrics/`, `src/checker.rs`,
   and `src/getter.rs`.
4. Uploads `target/mutants/` as the `cargo-mutants-report` artifact
   (90-day retention).
5. On non-zero exit, opens a GitHub issue labelled
   `mutation-testing` with the missed / timed-out counts and a link
   back to the run.

The job has `issues: write` permission and uses `GITHUB_TOKEN`; no
extra secrets are required.

## Running locally

Install `cargo-mutants` once:

```bash
cargo install cargo-mutants --locked
```

The repo ships a `cargo mutants` alias (`.cargo/config.toml`) that
matches the CI flags. To exercise a single metric file (the typical
local case):

```bash
cargo mutants -f src/metrics/cognitive.rs
```

To exercise the same surface as CI:

```bash
cargo mutants -f src/metrics/ -f src/checker.rs -f src/getter.rs
```

Plan on tens of minutes per file on a laptop. Use `-j N` to bound
parallelism if the run is starving other work; the CI workflow uses
`-j 2`.

## Interpreting an escaped mutant

`cargo-mutants` writes its results to `target/mutants/`:

| File | Meaning |
|------|---------|
| `missed.txt` | Mutants that survived — the suite did not catch them |
| `timeout.txt` | Tests that hit the timeout (often infinite loops, occasionally real gaps) |
| `caught.txt` | Mutants the suite correctly killed |
| `unviable.txt` | Mutants that did not compile (no signal either way) |
| `mutants.log` | Full stdout from the run |

Each line in `missed.txt` looks like:

```text
src/metrics/cognitive.rs:142:9: replace > with >= in compute
```

Triage:

1. Read the mutant location and the surrounding code.
2. If the mutation produces observably different output (different
   metric value, different control flow), add a test that pins the
   correct behaviour. This is the common case and the whole point.
3. If the mutation is genuinely behaviour-equivalent (e.g. a `match`
   arm that is provably unreachable, an unused branch in a helper
   that always early-returns), document why and add the location to
   `--exclude-re` in a follow-up. Do this sparingly — most "looks
   equivalent" mutants are real coverage gaps.
4. Timeouts often mean a mutation introduced an infinite loop. The
   `--minimum-test-timeout 120` in the CI workflow guards against
   spurious timeouts on slow runners; if a specific test case is
   chronically slow, prefer fixing the test.

## When the workflow files an issue

The auto-filed issue is the canonical entry point. It contains the
missed / timed-out counts, a link to the run, and the first 50
missed lines for quick scanning. Download the
`cargo-mutants-report` artifact from the run for the full list, then
follow the triage steps above.

Close the issue once every escape has either:

- A new test that kills the mutant, **or**
- An explicit `--exclude-re` entry with a justification commented in
  `.github/workflows/mutation-test.yml` or the relevant source file.
