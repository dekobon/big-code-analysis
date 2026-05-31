# AGENTS.md

Universal project instructions for AI coding assistants.

## Project Overview

`big-code-analysis` is a Mozilla-maintained Rust library that extracts
maintainability metrics from source code in many languages. It is built on
[tree-sitter](https://tree-sitter.github.io/tree-sitter/) and is published on
crates.io as a library plus two binaries.

The repository is a Cargo workspace:

| Crate | Path | Purpose |
|-------|------|---------|
| `big-code-analysis` | `./` (root) | Library: parsers, AST traversal, metric computation |
| `big-code-analysis-cli` | `big-code-analysis-cli/` | CLI for invoking the library on files / trees |
| `big-code-analysis-py` | `big-code-analysis-py/` (excluded from default-members; needs Python headers + maturin) | PyO3 Python bindings |
| `big-code-analysis-web` | `big-code-analysis-web/` | REST API server wrapping the library |
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
  modules deliberately mirror each other; macros under `c_langs_macros/`,
  `src/macros.rs`, and `src/c_macro.rs` generate the shared structure. A
  bug in one language module typically exists in several — fix all
  affected siblings together.
- `src/metrics/` — individual metric implementations: `abc.rs`,
  `cognitive.rs`, `cyclomatic.rs`, `exit.rs`, `halstead.rs`, `loc.rs`,
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
- Helper scripts: `check-grammar-crate.py`, `check-grammars-crates.sh`,
  `recreate-grammars.sh`, `generate-grammars/`. (The grammar-bump diff
  step now uses the native `bca diff`; the former
  `split-minimal-tests.py` + external `json-minimal-tests` chain was
  retired in #487.)

## Editing principles

- This is a published `1.x` library (`big-code-analysis` on crates.io)
  with a written stability contract in [`STABILITY.md`](./STABILITY.md).
  Treat `lib.rs` re-exports, public traits (`ParserTrait`,
  `LanguageInfo`, etc.), and public types (`Metrics`, `FuncSpace`,
  language enums) as a stable API surface. Within `1.x`, additive
  changes belong under a minor bump; breaking shape changes are
  reserved for `2.0` and must be planned deliberately — never slip a
  SemVer break into a patch or minor release. Public-API changes
  must be cross-referenced against `STABILITY.md` and recorded in
  the `## [Unreleased]` section of `CHANGELOG.md`; if the change is
  a source-level break that must wait for `2.0`, mark the entry
  **(breaking)** and note that it is deferred to the next major
  bump (the release-prep commit then moves the entry into the
  appropriate version section).
- For code files: prefer LSP / symbol-level editing
  (`replace_symbol_body`, `insert_before/after_symbol`) over line-based
  edits when available. Read the file (or use a symbol overview) before
  editing.
- For non-code files (Markdown, TOML, YAML, JSON): use targeted edits with
  scoped `old_string` / `new_string` pairs. Avoid `sed` for multi-line
  edits.
- Never rewrite an entire test file to add or fix one test. Modify only
  the specific tests that need changing.
- Verify previously passing tests still pass before committing
  (`cargo test --workspace --all-features`).
- When fixing a bug, add a regression test that would catch the exact bug
  if reintroduced.
- Default to writing no comments. Only add one when the *why* is
  non-obvious.
- **MANDATORY** before any public API change: enumerate every call site
  (`find_referencing_symbols` if an LSP tool is available, otherwise a
  workspace-wide search). Cross-crate breakage is silent until CI.
- When a change touches metric computation, AST traversal, or anything
  under `src/languages/`, exercise **every** language affected — passing
  tests in one language do not catch regressions in another. Per-language
  modules deliberately mirror each other; a bug in one typically exists in
  several.

## Tool choice

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
  `make py-relock` (which runs `uv lock`) to regenerate the lockfile;
  bootstrap will fail-loud rather than silently rewriting it. Install
  uv with `curl -LsSf https://astral.sh/uv/install.sh | sh`,
  `brew install uv`, or `pipx install uv`. Alternative provisioning
  paths (`mise install` via `mise.toml`, direct `pipx install`, plain
  `pip install -e .[dev]`) remain functional but bypass `uv.lock` —
  resolved versions can drift from peers and from CI. CI itself does
  not yet consume `uv.lock` (workflows pip-install pyproject floors);
  closing that gap is tracked as a follow-up.
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

- No `unsafe` code anywhere in the workspace.
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
default-features and `--all-features` flavours, `cargo test
--workspace --all-features`), `cargo doc --no-deps --workspace
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
`pytest` stages for `big-code-analysis-py` (each Python stage is
skipped with a clear "X not found" message when the corresponding
tool is absent). `make ci` runs the same checks without auto-fix,
mirroring CI behaviour.

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

If GNU Make 4 or any of the optional tools (`taplo`, `rumdl`,
`shellcheck`, `shfmt`, `checkmake`, `actionlint`, `ruff`, `mypy`, `pyright`,
`maturin`) are unavailable, fall back to the raw cargo commands:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features
```

If `pre-commit` is installed, also run `pre-commit run --all-files`. The
project's `.pre-commit-config.yaml` runs clippy, `cargo +nightly udeps`,
and the test suite.

**Mutation testing** runs out-of-band on a quarterly cron via
`.github/workflows/mutation-test.yml` against `src/metrics/`,
`src/checker.rs`, and `src/getter.rs`. It is intentionally not part of
the per-PR gate (a full run is tens of minutes per file). Escapes
auto-file a GitHub issue labelled `mutation-testing`. See
[`docs/development/mutation_testing.md`](docs/development/mutation_testing.md)
for local invocation and triage guidance.

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
  `class_nargs_sum()`, `u_operators()`, …) — float magnitude / volume /
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
`./check-snapshot-anchors.py`, which scans every
`insta::assert_json_snapshot!(metric.…)` call under `src/metrics/`
and counts the unanchored ones per file. The current per-file
counts are checked in at `.snapshot-anchor-baseline.txt`; CI fails
on any *increase*. Decreases are silent and may be locked in with
`./check-snapshot-anchors.py --update`, which regenerates the
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

## Tree-sitter grammars

External grammar crates are version-pinned (`=0.23.x`, etc.) in the root
`Cargo.toml`. Treat the pinned version as fixed:

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
