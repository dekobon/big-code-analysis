# AGENTS.md

Universal project instructions for AI coding assistants.

## Project Overview

`big-code-analysis` is a Rust library that extracts maintainability
metrics from source code in many languages. It is a hard fork of
Mozilla's
[rust-code-analysis](https://github.com/mozilla/rust-code-analysis),
maintained in this repository. It is built on
[tree-sitter](https://tree-sitter.github.io/tree-sitter/) and is published on
crates.io as a library plus two binaries.

The repository is a Cargo workspace:

| Crate | Path | Purpose |
|-------|------|---------|
| `big-code-analysis` | `./` (root) | Library: parsers, AST traversal, metric computation |
| `big-code-analysis-cli` | `big-code-analysis-cli/` | CLI for invoking the library on files / trees |
| `big-code-analysis-py` | `big-code-analysis-py/` (excluded from default-members; needs Python headers + maturin) | PyO3 Python bindings |
| `big-code-analysis-web` | `big-code-analysis-web/` | REST API server wrapping the library |
| `big-code-analysis-bench` | `big-code-analysis-bench/` (excluded from default-members) | Out-of-band benchmark harness for the metric walk: the complexity-class gate (`make bench-scaling`) and criterion measurements (`make bench-walk`); see [`docs/development/benchmarking.md`](docs/development/benchmarking.md) |
| `xtask` | `xtask/` (excluded from default-members) | Build-time helper that renders man pages from the live clap definitions (see `man/`) |
| `enums` | `enums/` (excluded from default workspace) | Code-generation helper for language enums |

Vendored / path-dependent grammar crates also live in the repo:
`tree-sitter-ccomment`, `tree-sitter-mozcpp`, `tree-sitter-mozjs`,
`tree-sitter-preproc`, `tree-sitter-tcl`. External grammar crates are
pinned with `=X.Y.Z` versions in the root `Cargo.toml`.

The default branch is **`main`**.

The CLI binary is **`bca`** (package `big-code-analysis-cli`); the
web-server binary is **`bca-web`** (package `big-code-analysis-web`).
From a checkout, run them via `cargo run -p big-code-analysis-cli --`
and `cargo run -p big-code-analysis-web --`.

## Project layout

- `src/lib.rs` — public re-exports; this is the published API surface.
- `src/languages/` — one `language_<lang>.rs` per supported language. These
  modules deliberately mirror each other; macros under
  `src/c_langs_macros/`, `src/macros/`, and `src/c_macro.rs` generate the
  shared structure. A
  bug in one language module typically exists in several — fix all
  affected siblings together.
- `src/metrics/` — individual metric implementations: `abc.rs`,
  `cognitive.rs`, `cyclomatic.rs`, `nexits.rs`, `halstead.rs`, `loc.rs`,
  `mi.rs`, `nargs.rs`, `nom.rs`, `npa.rs`, `npm.rs`, `tokens.rs`,
  `wmc.rs`.
- `src/output/` — JSON / YAML / TOML / CBOR serializers for metric output.
- `src/parser.rs`, `src/node.rs`, `src/spaces.rs`, `src/checker.rs`,
  `src/getter.rs`, `src/alterator.rs`, `src/traits.rs` — core AST plumbing.
- `tests/` — integration tests, including `insta` snapshot tests
  (`*.snap` / `*.snap.new`).
- `big-code-analysis-book/` — mdBook documentation source.
- `enums/` — separate workspace member (excluded from the root workspace)
  that generates language enum tables.
- `utils/` — repo-maintenance helper scripts, including the gates run
  by `make pre-commit` / `make ci`: `check-versions.py`,
  `check-snapshot-anchors.py`, `check-manpage-assets.py`,
  `check-grammar-marker-sync.py`, `check-enums-codegen-drift.sh`,
  `check-grammar-crate.py`, `check-grammars-crates.sh`,
  `check-excluded-manifests.py`, `verify-name-only-churn.py`, and each
  gate's `*-test.py` self-tests.
  Each resolves the repository root from its own location
  (`Path(__file__).resolve().parents[1]`) rather than the cwd, so it
  runs correctly from anywhere; callers invoke them as `utils/<name>`.
- Other helper scripts: `recreate-grammars.sh`, `generate-grammars/`.
  (The grammar-bump diff step now uses the native `bca diff`; the former
  `split-minimal-tests.py` + external `json-minimal-tests` chain was
  retired in #487.)

## Editing principles

- This is a published `2.x` library (`big-code-analysis` on crates.io)
  with a written stability contract in [`STABILITY.md`](./STABILITY.md).
  Treat `lib.rs` re-exports, public traits (`ParserTrait`,
  `LanguageInfo`, etc.), and public types (`Metrics`, `FuncSpace`,
  language enums) as a stable API surface. Within the current major
  line, additive changes belong under a minor bump; breaking shape
  changes are reserved for the next major (`3.0`) and must be planned
  deliberately — never slip a SemVer break into a patch or minor
  release. Public-API changes must be cross-referenced against
  `STABILITY.md` and recorded in the `## [Unreleased]` section of
  `CHANGELOG.md`; if the change is a source-level break that must
  wait for the next major, mark the entry **(breaking)** and note
  that it is deferred to the next major bump (the release-prep
  commit then moves the entry into the appropriate version section).
- For code files: prefer LSP / symbol-level editing
  (`replace_symbol_body`, `insert_before/after_symbol`) over line-based
  edits when available. Read the file (or use a symbol overview) before
  editing.
- For non-code files (Markdown, TOML, YAML, JSON): use targeted edits with
  scoped `old_string` / `new_string` pairs. Avoid `sed` for multi-line
  edits.
- The book is translated (Japanese, `big-code-analysis-book/po/ja.po`)
  via the gettext workflow in
  [`docs/development/translations.md`](docs/development/translations.md).
  English doc edits never block on translation — changed paragraphs
  fall back to English on the `/ja/` site until someone runs
  `make book-po-update` and fills the new/fuzzy entries. Two rules do
  apply: pin an explicit `{#anchor}` id on any heading you
  fragment-link to, and keep `README.ja.md` in step when you edit
  `README.md`.
- Never rewrite an entire test file to add or fix one test. Modify only
  the specific tests that need changing.
- Verify previously passing tests still pass before committing
  (`cargo test --workspace --all-features`).
- When fixing a bug, add a regression test that would catch the exact bug
  if reintroduced.
- Default to writing no comments. Only add one when the *why* is
  non-obvious.
- Issue and PR references belong in `//` maintainer comments, never in a
  `///` doc comment on a clap type. clap renders `///` into
  `bca <cmd> --help` and `cargo xtask` renders the same text into
  `man/`, so an internal note ships to users twice and drifts the
  committed man pages. `help_text_carries_no_issue_references` pins
  this — a failure is a real leak, not a test to relax. Any edit to a
  clap help/about/value doc, even a pure wording fix, must be followed
  by `cargo xtask` with the regenerated `man/` pages in the *same*
  commit.
- **MANDATORY** before any public API change: enumerate every call site
  (`find_referencing_symbols` if an LSP tool is available, otherwise a
  workspace-wide search). Cross-crate breakage is silent until CI.
- When a change touches metric computation, AST traversal, or anything
  under `src/languages/`, exercise **every** language affected — passing
  tests in one language do not catch regressions in another. Per-language
  modules deliberately mirror each other; a bug in one typically exists in
  several.

## Tool choice

- **Shell**: assistant tooling runs **zsh**, which does not field-split
  an unquoted parameter expansion the way bash does — `cmd $FLAGS`
  passes one argument, not several. The failure is silent and yields a
  plausible result rather than an error, so it has already produced
  fabricated measurement tables here. Build argument lists as arrays and
  expand them `"${ARR[@]}"`. See
  [`.claude/rules/shell.md`](.claude/rules/shell.md).
- **Code search**: `rg` (ripgrep). Never `grep` via Bash.
- **File search**: `fd` (or `fdfind` on Debian/Ubuntu). Never `find` via
  Bash.
- **Code intelligence**: when an LSP-based tool such as Serena is
  available, use it as the default for read / search / edit / refactor
  (`get_symbols_overview`, `find_symbol`, `find_referencing_symbols`,
  `replace_symbol_body`).
- **External docs**: prefer Context7 / `cargo doc` over web search for
  library / crate documentation.
- **Python environment**: `uv` is the canonical, lockfile-driven
  bootstrap for `big-code-analysis-py`. `make py-bootstrap` runs
  `uv sync --locked --extra dev` from the checked-in `uv.lock`,
  producing a reproducible dev environment shared across contributors
  who use this path. After editing `pyproject.toml` deps, run
  `make py-relock`, which regenerates `uv.lock` **and** the
  hash-pinned exports under `big-code-analysis-py/requirements/`
  (`dev.txt`, `examples.txt`); bootstrap will fail-loud rather than
  silently rewriting the lockfile. Install uv with
  `curl -LsSf https://astral.sh/uv/install.sh | sh`,
  `brew install uv`, or `pipx install uv`. Alternative provisioning
  paths (`mise install` via `mise.toml`, direct `pipx install`, plain
  `pip install -e .[dev]`) remain functional but bypass `uv.lock` —
  resolved versions can drift from peers and from CI. CI consumes
  `uv.lock` through those requirements exports (`pip install
  --require-hashes -r …` in the workflows, per OpenSSF Scorecard
  Pinned-Dependencies), so a `uv.lock` change and its regenerated
  exports must land in the same commit.
- **GitHub Actions linting**: any edit under `.github/workflows/`
  must be validated with `make actionlint` before commit. The
  Makefile target invokes `actionlint` at the repo root, which
  discovers workflows automatically and shells out to `shellcheck`
  for embedded `run:` scripts. `make actionlint` is wired into `make
  lint`, `make pre-commit`, and `make ci`, and into the `actionlint`
  hook in `.pre-commit-config.yaml`. Suppress shellcheck false
  positives inside a `run:` block with a scoped `# shellcheck
  disable=SCxxxx` directive (and a short why-comment), not by
  loosening actionlint configuration. If composite actions are ever
  introduced under `.github/actions/*/action.yml`, extend the
  Makefile recipe and the pre-commit hook to pass those files to
  actionlint explicitly — bare `actionlint` does not discover them.

## Rust conventions

- No `unsafe` code anywhere in the workspace, with one narrow,
  documented exception: the PyO3 bindings (`big-code-analysis-py`)
  erase the lifetime brand on a `Copy`, borrow-free `tree_sitter` FFI
  value so a `#[pyclass]` (which cannot carry a lifetime) can hold it,
  keeping the owning tree alive through a strong `Py<...>` handle. The
  canonical soundness argument lives at
  `big-code-analysis-py/src/node.rs` (the `# Safety` module doc and
  `detach`). Any `unsafe` outside this exact pattern remains banned and
  needs a deliberate amendment to this rule. (The PyO3-macro-generated
  FFI shims under `#![allow(unsafe_op_in_unsafe_fn)]` in `src/lib.rs`
  are source-level `unsafe`-free and not covered by this exception.)
- No `unwrap()` / `expect()` / `panic!()` / `assert!()` in non-test code;
  propagate errors with `?`. `expect("reason")` and `assert!()` are
  acceptable in tests and may be acceptable in production for
  provably-unreachable invariants — document the invariant in the
  `expect` message.
- Prefer `pub(crate)` over `pub`; widen visibility only when an item is
  re-exported from `lib.rs`.
- Prefer borrowing over cloning. Use `&str` over `String` parameters
  unless ownership is required downstream.
- Newtype wrappers for domain identifiers; do not pass two same-typed
  primitives where they could be confused.
- Never use `to_string_lossy()` on paths used as identifiers (map keys,
  JSON output, error correlation). Use `to_str()` with explicit error
  handling. `path.display()` is fine for log output only.
- Edition is 2024 — `let-else`, let-chains, and other 2024 features are
  available.

## Validation gates

Before considering a change done, run `make pre-commit` from the repo
root. It is the canonical entry point for the full validation gate
and runs, in one parallel pass: the cargo trio (`cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings` in both
default-features and `--all-features` flavours, the full test suite
via `make test` + `make test-doc` — `make test` runs `cargo nextest
run --workspace --all-features` when `cargo-nextest` is on PATH and
falls back to `cargo test --workspace --all-features --lib --bins
--tests` otherwise, and `make test-doc` covers the doctests nextest
cannot run), `cargo doc --no-deps --workspace
--all-features` with `RUSTDOCFLAGS="-D warnings"`,
`cargo +nightly udeps`, the markdown /
TOML / shell / Makefile / GitHub Actions lint families, the man-page
drift gate (`cargo xtask` + `git diff --exit-code -- man/`, mirroring
the `manpage` CI job), the bca self-scan threshold gate at both
tiers (`make self-scan` mirroring the `Threshold gate` step in
`.github/workflows/pages.yml`, plus `make self-scan-headroom`
which scales every limit by `BCA_HEADROOM` — default `0.95` — so
functions encroaching into the 95-100% band fail before the hard
gate trips; `make self-scan-write-baseline-headroom` refreshes
`.bca-baseline.toml` — prefer the headroom variant over the bare
`make self-scan-write-baseline`, otherwise the soft-tier
`self-scan-headroom` gate re-fires on untouched files), and the
Python `ruff` lint /
`ruff format` / `mypy --strict` + `pyright` / `maturin develop` +
`pytest` / `mypy stubtest` stages for `big-code-analysis-py` (each
Python stage is skipped with a clear "X not found" message when the
corresponding tool is absent). `make ci` runs the same checks without
auto-fix, mirroring CI behaviour.

**`_native.pyi` is stubtest-gated (#673).** The hand-written PyO3 stub
`big-code-analysis-py/python/big_code_analysis/_native.pyi` is no
longer "kept in lockstep by hand" on trust alone: `make py-stubtest`
runs `python -m mypy.stubtest big_code_analysis._native
big_code_analysis.vcs` against the freshly `maturin develop`-built
extension, diffing names, signatures, and **defaults** — catching the
`#[pyo3(signature = …)]`-vs-stub drift that the usage-only `make
py-typecheck` (mypy/pyright over call sites) cannot see (#583 shipped
one such drift). The second module argument extends the same gate to
the `vcs` submodule (`rank` / `trend` / `commit` / `score_diff` / the
`Options` constructor) via its `big_code_analysis.vcs` facade and
`vcs.pyi` stub (#854 — the submodule was previously allowlisted out
wholesale, reopening the #583 gap). It is wired into `make
pre-commit` and `make ci` (chained after `py-test`, sharing its
`maturin develop` build) and skips cleanly when the venv / maturin /
stubtest are absent. When you change a PyO3 signature or default
(`src/lib.rs`, `src/batch.rs`, `src/analysis.rs`, the `vcs` module),
update `_native.pyi` **or** `vcs.pyi` to match and re-run `make
py-stubtest`; the only remaining allowlisted entries are genuinely-
deliberate facade differences (the `_native.vcs` submodule *attribute*
— its signatures are checked via the facade run — and runtime
`__all__`), which live in
`big-code-analysis-py/stubtest-allowlist.txt` — keep that list
minimal so it never masks real drift. Note PyO3 exposes a
`#[new]` constructor as `__new__` (not `__init__`) and renders a
computed default like `commit = "HEAD".to_owned()` as `...`
(Ellipsis) in `__text_signature__`, so the stub must mirror those
runtime shapes for the gate to pass.

**Baseline-refresh discipline.** Any change that moves a *baselined*
metric past its recorded `.bca-baseline.toml` value must refresh the
baseline in the **same PR**. The baseline filter only suppresses a
violation while the live measurement stays at or below the recorded
value; once a file grows past it (e.g., #445 grew `src/count.rs`'s
`halstead.effort` from ~103k to ~191k), the filter no longer covers
the offender and `make self-scan` goes red on a clean checkout — for
everyone, not just the author (#449). The existing `bca-self-scan`
pre-commit hook and the `Threshold gate` step in
`.github/workflows/pages.yml` already catch this, so the safeguard is
purely procedural: do not bypass pre-commit, and refresh the baseline
with `make self-scan-write-baseline-headroom` in the commit that moved
the metric. A red gate on `main` traces directly to skipping this step.

**Price a candidate limit at both tiers before calling it free.**
Converging a limit onto a cluster of existing values is never free while
a proportional soft tier is active — the soft tier measures *distance to
the limit*, so a limit chosen to sit exactly on a population's value
maximises soft-tier breach by construction, and none of those functions
can ever clear the band because they *are* the limit. The natural
measurement is the misleading one: `bca check --threshold <m>=<limit>`
is applied last and absolutely, never scaled, so it has no soft tier to
report. Use `bca check --explain-threshold <metric>=<limit>`, which
reports both tiers plus how many offenders each already has in the
baseline, and weigh the *new-entry* count. This repo's `nargs 7 → 6`
was approved on a hard-tier zero and would have bought 74 permanent
baseline entries (#1143, #1169); the same trap applies to a
`[thresholds.lang.<slug>]` override (#1141). See
[Tightening a limit onto a cluster](big-code-analysis-book/src/recipes/thresholds.md#converging-onto-a-cluster).

If GNU Make 4 or any of the optional tools (`taplo`, `rumdl`,
`shellcheck`, `shfmt`, `checkmake`, `actionlint`, `cargo-nextest`,
`ruff`, `mypy`, `pyright`, `maturin`) are unavailable, fall back to the
raw cargo commands:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features
```

If `pre-commit` is installed, also run `pre-commit run --all-files`. The
project's `.pre-commit-config.yaml` runs clippy, `cargo +nightly udeps`,
and the test suite.

**The ancestor-chain audit** (`make chain-audit`) re-runs the library
tests with `--cfg chain_audit`, which restores the exact
`chain.last() == node.parent()` assertion in `Ancestors::checked`. The
default build keeps only an `O(1)` approximation of it, because the
exact form is `Node::parent`'s `O(depth)` per node and made every
debug-build walk quadratic (#1122). The approximation misses a chain
that is short by exactly one, so run this around any change to a walk's
truncate/push bookkeeping — `src/spaces/compute.rs`, `src/ops.rs`,
`src/comment_rm.rs`, `src/suppression.rs`, `Search::act_on_node`. It is
not part of `make pre-commit`; the `chain-audit` CI job runs it per PR.
See [Benchmarking](docs/development/benchmarking.md#chain-audit).

**Mutation testing** runs out-of-band on a quarterly cron via
`.github/workflows/mutation-test.yml` against `src/metrics/`,
`src/checker.rs`, and `src/getter.rs`. It is intentionally not part of
the per-PR gate (a full run is tens of minutes per file). Escapes
auto-file a GitHub issue labelled `mutation-testing`. See
[`docs/development/mutation_testing.md`](docs/development/mutation_testing.md)
for local invocation and triage guidance.

**Benchmarking** is the other out-of-band quarterly gate
(`.github/workflows/benchmark.yml`, `big-code-analysis-bench`). It is
deliberately not per-PR — shared runners cannot produce stable numbers,
and a timing assertion in the unit suite already produced false
failures in four environments. The consequence is that a reintroduced
quadratic walk makes the unit tests *slow* rather than red, so run
`make bench-scaling` around any change to AST traversal, `Checker`,
`Getter`, or a metric's `compute`. See
[`docs/development/benchmarking.md`](docs/development/benchmarking.md).

For snapshot test changes, run `cargo insta test --review` and accept or
reject each snapshot rather than blindly updating files.

**Bulk snapshot refresh** (grammar bumps, metric computation changes,
Halstead operator reclassification): these cause hundreds of snapshots
to shift in metric values. Use `cargo insta test --accept` per test
file to accept in batch after verifying the diff pattern is
metric-value-only (no structural changes). Run `cargo insta test
--accept` rather than incremental `mv *.snap.new` — accepting snapshots
one at a time can shift `assertion_line` fields, causing a cascade
where previously-matching snapshots become stale.

**Anchor every `insta::assert_json_snapshot!` call.** A bare
`insta::assert_json_snapshot!(metric.X)` records whatever production
emitted at acceptance time — including bugs (see issue #95 and lesson 2
in `docs/development/lessons_learned.md`; the `InvocationExpression`
aliasing miscount fixed by #94 was masked for the entire C# language-
support PR by exactly this pattern). Every new snapshot assertion must
carry one of:

- An inline expected block: `insta::assert_json_snapshot!(metric.X, @r###"…"###)`.
- A positive `assert_eq!` on the headline value(s) immediately above
  the snapshot call, using integer-valued accessors (`branches()`,
  `class_npm_sum()`, `unique_operators()`, …) — float magnitude / volume /
  difficulty / effort / `*_average` are bit-brittle and not safe for
  exact equality.
- A `// expected: <derivation>` comment explaining what the values
  should be and why, sufficient for a reviewer to verify without
  re-deriving from scratch.

Bulk `cargo insta accept --workspace` is allowed only when every
accepted snapshot is already anchored — a grammar bump that shifts
metric values for anchored tests is fine; first-time acceptance of
bare snapshots is not. Existing bare snapshots predate this rule and
are tracked under #95; new tests must follow it.

This policy is enforced automatically. `make snapshot-anchors` (run
as part of `make pre-commit` and `make ci`, the
`.pre-commit-config.yaml` hooks, and the `lint` job in
`.github/workflows/ci.yml`) invokes
`./utils/check-snapshot-anchors.py`, which scans every
`insta::assert_json_snapshot!(metric.…)` call under `src/metrics/`
and counts the unanchored ones per file. The current per-file
counts are checked in at `.snapshot-anchor-baseline.txt`; CI fails
on any *increase*. Decreases are silent and may be locked in with
`./utils/check-snapshot-anchors.py --update`, which regenerates the
baseline from the working tree.

**Integration snapshots live in the `big-code-analysis-output`
submodule** (`tests/repositories/big-code-analysis-output/`). Any
behaviour-changing fix that touches metric computation, AST traversal,
or alterator rules — cognitive, cyclomatic, Halstead, exit, ABC,
etc. — generates `.snap.new` files **inside the submodule**, not the
parent repo. A fix is not done until **all four** of these have
happened in the same change:

1. `cargo test --workspace --all-features` exits clean from a fresh
   working tree (no `.snap.new` left behind under
   `tests/repositories/big-code-analysis-output/`).
2. The accepted snapshots are committed and pushed to the submodule's
   remote (`dekobon/big-code-analysis-output`, `main` branch).
3. The parent records the new submodule SHA — `git add
   tests/repositories/big-code-analysis-output` — in the **same
   parent commit** as the metric/alterator fix, never as a follow-up.
4. After any rebase, force-push, or long-running batch fix, re-run
   the integration tests before declaring done. The submodule's
   history is force-pushed often enough that prior accepts cannot be
   assumed to survive — see lesson 8 in
   `docs/development/lessons_learned.md`.

A behaviour-changing fix without the matching submodule bump leaves
the next fresh clone with either an unfetchable submodule SHA or
stranded `.snap.new` drift that blocks CI on every subsequent change.

## Metric thresholds

The limits `bca init` scaffolds are defined once, in
`big-code-analysis-cli/src/default_thresholds.rs`. They are derived
from published thresholds plus a 20-language corpus measurement (#1140),
and each carries its rationale inline. Change them there, not in a
copy: the scaffolded `bca.toml` renders its `[thresholds]` block from
that table, so there is no second copy of the numbers to drift.

Two copies of the *summary table* do exist — the one below and the one
in the book's recipe — and `doc_summary_tables_match_the_default_table`
pins both, in each direction. Do not add a third: it would be
unguarded, since that test only knows the paths listed in
`DOCS_REPEATING_THE_TABLE`.

| Metric | Default | Scope |
|--------|---------|-------|
| `cognitive` | 15 | function |
| `cyclomatic` | 15 | function |
| `abc` | 40 | function |
| `nargs` | 5 | function |
| `nexits` | 5 | function |
| `halstead.effort` | 50000 | function |
| `loc.ploc` | 600 | file |
| `loc.sloc` | 1200 | file |
| `nom` | 30 | container |
| `wmc` | 60 | container |

Three things to know before quoting a number at anyone:

- **These are language-agnostic defaults, and no language is
  agnostic.** Measured 97.5th-percentile `cognitive` runs from 4 (C#)
  to 50 (C). C, Tcl, Bash, Lua, Perl, and Go want roughly double the
  defaults; Java, Ruby, Kotlin, C#, and Elixir sit far under them. The
  per-language table lives in
  [Choosing thresholds](big-code-analysis-book/src/recipes/thresholds.md).
- **The right limit depends on the job.** A blocking CI gate, an agent
  edit loop, a legacy triage pass, and a safety-critical build want
  four different tables. That page has one profile for each.
- **This repository's own `bca.toml` deliberately differs from the
  shipped defaults.** It is a Rust-specific calibration with
  `exclude_tests = true` and a comment-dense house style. `cognitive`,
  `nargs`, and `abc` are still at their pre-#1140 values here, tracked
  in #1143. Do not "fix" one file to match the other; they answer
  different questions.

## Responding to bca metric feedback

This repo dogfoods its own analyzer: a `bca check` threshold violation
may surface mid-edit (via the optional Claude Code `PostToolUse` hook in
`.claude/hooks/bca-check.sh`) or at the task boundary (`make self-scan`,
`make pre-commit`). A violation (cognitive, cyclomatic, ABC, …) means
*this function is hard for a human to follow* — the number is a proxy
for that, not the goal. Make the code genuinely simpler, not the number
smaller.

- **Do not game the metric.** Do not extract a helper that exists only
  to move complexity off one function, split a cohesive function at an
  arbitrary line, collapse readable branches into a dense expression, or
  inline/obfuscate logic to dodge the count. These lower the
  per-function score while making the code worse — and a spurious helper
  often *raises* file-level `nom`/`nargs`, so the file is no better off.
- **Refactor only when it truly clarifies.** A good split has a name
  that means something and a boundary a reader would have drawn anyway.
  If you cannot name the extracted piece without inventing a `foo_part2`,
  the split is gaming — stop.
- **When the complexity is essential, suppress with a reason.** Some
  functions are irreducibly complex *and clearest left whole* — a
  dispatch `match`, a hand-rolled parser table, an exhaustive state
  machine. For these, add an in-source marker rather than contorting
  the code, with the rationale on the same line:
  `// bca: suppress(<metrics>) — <why>` inside the function (per-file:
  `// bca: suppress-file(<metrics>) — <why>`). Anything after the metric
  list is free text; no separator is required. After a *bare* verb
  (`// bca: suppress`, no list) the rationale must open with `-`, `:`,
  `//`, `#`, or a dash, so prose about the marker is not read as one.
  Use canonical metric names — it is `nexits`, **never** `exit`; an
  unknown identifier warns and is skipped, while the recognised names in
  the same marker still suppress. `tokens` is not suppressible. See
  [Suppression markers](big-code-analysis-book/src/commands/suppression.md)
  and the full recipe at
  [`recipes/agent-feedback.md`](big-code-analysis-book/src/recipes/agent-feedback.md).
- **Keep the fix where the violation is.** The flag is scoped to the
  function you just edited. Fix it there, mention anything larger you
  noticed, and do not widen the change into a module rewrite to bring
  the number down.
- **Attribute the score before sizing the fix, then size against every
  gated metric.** Re-derive the genuine decision count by hand and
  compare it to the measurement: part of the headline may be a metric
  artifact that adds a CFG edge without adding anything a reader must
  reason about. Rust's `?` is the canonical one — it feeds cyclomatic
  *and* nexits, and only ~12 of `dump_tree_helper`'s "cyclomatic 32"
  was real branching (#401). Large string literals do the same to
  halstead, inline tests to file-level `sloc`. Where the gap is an
  artifact, say so and justify the refactor on what actually improves.
  Then size each new helper against **all** the metrics it could trip —
  cyclomatic, nexits, nargs, abc, halstead.effort — because one
  construct moves several gauges: #401's proposed split passed
  cyclomatic and would have breached nexits. Read the live `bca.toml`
  for the limits; an issue's quoted threshold table is routinely wrong.

The per-edit hook is an early-warning convenience; the task-boundary
gate (`make self-scan` / `make pre-commit`) is the real check before
declaring work done. Thresholds are proxies, not correctness gates —
weigh them, do not drive them to zero at any cost.

## Tree-sitter grammars

External grammar crates are version-pinned (`=0.23.5`, `=0.26.10`,
etc.) in the root `Cargo.toml` **and in each workspace-excluded crate's
own manifest** (the five vendored grammars plus `enums`) —
`utils/check-excluded-manifests.py` enforces both, so neither a root
pin nor a new vendored grammar can reintroduce a caret range (#1151).
Member crates take their grammars through `workspace = true` and carry
no requirement of their own. The gate reads manifests with `tomllib`,
so a literal string (`tree-sitter-cpp = '0.23.4'`) is checked like any
other, and it matches `tree-sitter` anywhere in a dependency name —
`dekobon-tree-sitter-groovy` and any future `bca-tree-sitter-*` are in
scope. A pin means exactly `=X.Y.Z` (whitespace after `=` is fine); a
compound requirement such as `=0.25.0, <0.26` is rejected, because the
`.grammar-marker-baseline.toml` entry that `check-grammar-marker-sync.py`
compares against can name only one version.

One deliberate exception: `tree-sitter-language`, the ecosystem's shared
`LanguageFn` trait shim, is **not** a grammar and must stay caret-ranged.
`=`-pinning it makes the workspace unresolvable (`tree-sitter-irules
0.1.1` requires `^0.1.7`, and cargo unifies 0.1.x deps) and would break
downstream consumers of the published `bca-tree-sitter-*` crates. The
gate carries it in `PIN_EXEMPT_DEPS`.

The `tree-sitter` runtime is **not** exempt, though the "not a grammar"
half of that rationale fits it too. The exemption is about unification
pressure and the runtime has none — the lockfile shows 25 crates
depending on `tree-sitter-language` against one external dependent
(`tree-sitter-perl`) for `tree-sitter` — and every manifest here already
pins it at `=0.26.11` with the workspace resolving. Its ABI version is
also what each vendored `parser.c` was generated against, so an
accidental bump is precisely the drift the gate exists to catch.

Treat the pinned version as fixed:

- Do not loosen pins to a range without explicit user approval.
- Bumping a grammar version is a deliberate, separate change — usually
  driven by `recreate-grammars.sh` or `generate-grammars/`. Snapshot tests
  will move; review every diff.
- If a bug is in the grammar (wrong node type, wrong field name) rather
  than in our wrapper, document it as upstream-grammar and either
  workaround locally or coordinate the upstream fix; do not paper over it
  silently.

## GitHub workflow

- Issue and commit messages follow Conventional Commits
  (`feat(scope): …`, `fix(scope): …`, `refactor(scope): …`).
- For non-trivial `gh issue` / `gh pr` bodies, write to a temp file and
  pass via `--body-file` to avoid quoting issues:

  ```bash
  cat > /tmp/issue-body.md <<'EOF'
  Content with $variables, `backticks`, and "quotes"
  EOF
  gh issue create --title "Title" --label "bug" --body-file /tmp/issue-body.md
  ```

- Do not push (`git push`, `git push --force`) or open pull
  requests (`gh pr create`) without explicit user instruction.
  This rule covers **publishing code** only — it does **not**
  extend to issue tracker activity. Updating, commenting on,
  labelling, creating, and closing issues (`gh issue comment`,
  `gh issue close`, `gh issue edit`, `gh issue create`,
  `gh issue reopen`) are part of the normal fix-issue workflow
  and require no separate authorization beyond the user's
  initial request to work on the issue. Treat the issue tracker
  as a working surface, not a publish gate.
- Only close an issue when ALL items are resolved.
- When updating issues, update BOTH the body AND add a comment.

## Tone

Criticism is welcome — point out mistakes, suggest better approaches,
cite relevant standards. Be skeptical and concise.
