# Tool Output Rule

A truncated tool result is not the result. When output is too large to
inline, the harness persists it and shows a fragment headed
`Preview (first 2KB)` alongside the path to the full text. That fragment
is shaped exactly like a finished answer: no marker at the cut, no total
row count, nothing at the end saying more was dropped.

## Why it yields a wrong answer rather than an error

A preview is a prefix, so what survives is decided by output order, and
output order is rarely importance order. The commands used here most
often put the rows you need past the cut at least as reliably as before
it:

- `sort | uniq -c` — the counts you are hunting are the largest, and
  they sort **last** unless you passed `-rn`.
- `rg` over a tree — hits arrive in path order, so a sweep across
  `src/languages/` or `src/getter/` shows the alphabetically early
  languages and hides every one after them.
- `cargo test` / `make pre-commit` — the failure summary is at the end,
  behind all the passing output.
- `git log`, `git diff --stat` — the last-listed file is the one cut.

Reading a prefix of any of these produces a coherent, plausible, partial
answer. Nothing in it looks incomplete, which is the whole problem: the
tell that normally prompts a second look is absent.

## What it cost here

During #1127 an agent audited a `sort | uniq -c` tally from the preview
alone, missed two rows, and nearly shipped two tests that could no longer
fail.

## How to apply

- Read the persisted file in full before drawing a conclusion from it. A
  preview establishes *that* there is output. It never establishes what
  the output says.
- Better, do not generate output you will have to re-read. Aggregate in
  the command instead: `| wc -l` for a count, `sort -rn | head` to bring
  the interesting rows to the front, `rg -c` in place of `rg`,
  `--name-only` in place of a full diff.
- Treat every "there are no other X" claim as requiring the full text.
  Absence is precisely what a prefix cannot establish, and it is the
  conclusion these sweeps are usually run to reach.
- The same discipline covers filters you added yourself. A `| head -20`
  written to keep the output small is a truncation you must account for
  when reading the result, and unlike the harness preview it leaves no
  trace at all.
