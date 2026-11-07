# AGENTS.md

Shared instructions for coding agents working in this repository. Keep
this file short: it loads into every session. Procedures live in the
documents the routing table names and in `.claude/skills/`. There is
deliberately no `CLAUDE.md`: Claude Code reads this file natively, and
an `@AGENTS.md` shim is not expanded when a session starts in a
subdirectory. A personal `CLAUDE.local.md` suppresses native reading;
start it with `@AGENTS.md` and work from the repository root.

`big-code-analysis` is a published Rust library (crates.io, `2.x`) with
two binaries, `bca` (CLI) and `bca-web`. It is a Cargo workspace; the
`big-code-analysis-ast` crate is internal plumbing the root pins at
`=X.Y.Z`. Default branch: `main`. Read code before asking about layout.

## Commands

```bash
make worktree-setup          # once per checkout: corpora submodules + Python venv
make pre-commit              # the full validation gate; run before declaring work done
cargo test --workspace --all-features
cargo nextest run --workspace --all-features -E 'test(name)'   # one test
cargo run -p big-code-analysis-cli -- <args>                  # bca
```

`make pre-commit` ends with exactly one `BCA_GATE: pass …` or
`BCA_GATE: fail …` line on stdout. Capture the run to a `mktemp` log and
read that line; make's `Error N` lines are not the verdict, and no
`BCA_GATE:` line at all means the run did not finish. See
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the stage list, the reading of
the verdict, and the raw cargo fallback when `make` is unavailable.
Report any stage that was skipped or unavailable.

Checks that are **not** in `make pre-commit` and have their own
triggers: `make bench-scaling` after changing AST traversal, a `Checker`
/ `Getter`, or a metric's `compute`; `make chain-audit` after changing
walk truncate/push bookkeeping; `make fuzz-check` after editing `fuzz/`;
`make check-test-lang-gates-compare COMPARE_REF=origin/main` after
changing a test's `#[cfg(feature)]` gate. Each is described in the
document the routing table names.

## Conventions the code cannot tell you

- **Stable API.** `lib.rs` re-exports, public traits and public types
  follow [`STABILITY.md`](STABILITY.md). Enumerate every call site
  before changing a public signature. Record user-visible changes under
  `## [Unreleased]` in `CHANGELOG.md`; mark a source-level break
  **(breaking)** and defer it to the next major.
- **No `unsafe`**, except the documented PyO3 lifetime erasure in
  `big-code-analysis-py/src/node.rs`. No `unwrap` / `expect` / `panic!`
  / `assert!` outside tests; propagate with `?`. `expect("invariant")`
  is allowed only for a provably unreachable case, with the invariant
  in the message.
- **Per-language modules mirror each other.** A bug in one
  `big-code-analysis-ast/src/{languages,checker,getter,roles}/` sibling or
  `src/metrics/<metric>/<lang>.rs` usually exists in several. Dump the
  construct in each sibling grammar and fix them together; add a test
  per affected language.
- **Regression tests for every bug fix**, and prove a test can fail
  (see the testing guide). Never rewrite a test file to add one test.
- **Snapshots.** Every new `insta::assert_json_snapshot!` is anchored
  with an inline expected block, an `assert_eq!` on an integer headline
  value, or an `// expected:` derivation. Review snapshot diffs with
  `cargo insta test --review`; accept in bulk only for already-anchored
  snapshots after confirming the diff is value-only.
- **Behaviour-changing metric fixes** also move snapshots in the
  `tests/repositories/big-code-analysis-output` submodule. The same
  change must accept them, record the new gitlink, and refresh
  `.role-coverage-observed.txt` (`make role-coverage-observe`). Pushing
  the submodule is publication and needs explicit instruction.
- **Baseline refresh runs both ways.** A change that moves a metric
  past, or below, its `.bca-baseline.toml` record refreshes it in the
  same commit: `make self-scan-write-baseline-headroom`. Never
  hand-resolve a baseline conflict; regenerate.
- **Paths as identifiers** (map keys, JSON output, error correlation)
  go through `to_str()` with an error, never `to_string_lossy()`;
  `display()` is for log output only. Wrap domain identifiers in
  newtypes rather than passing two same-typed primitives. Prefer
  `pub(crate)`, and borrowing over cloning.
- **Diagnostics** go through the lowercase helpers in `src/diag.rs`
  (`warn`) and `big-code-analysis-cli/src/diag.rs` (`warn` / `die` /
  `note`); never spell `warning:` / `error:` in a literal.
- **clap help text** carries no issue references; after any help/about
  edit run `cargo xtask` and commit the regenerated `man/` pages.
- **Grammar pins.** Grammar crates and the `tree-sitter` runtime are
  pinned `=X.Y.Z` in the root manifest and each workspace-excluded
  crate's own (`tree-sitter-language`, the trait shim, stays
  caret-ranged; members inherit via `workspace = true`). A bump is a
  separate, deliberate change.
- **Comments:** none by default; add one only when the *why* is
  non-obvious. Never put an issue number in a `///` on a clap type: it
  ships in `--help` and `man/` (`help_text_carries_no_issue_references`
  pins this).
- **Docs:** prose follows `docs/conventions/documentation.md`; keep
  `README.ja.md` in step with `README.md`; pin `{#anchor}` ids on
  headings you fragment-link to.

## Working rules

- **Worktrees.** Work only in the worktree you were given. Never remove
  or prune a worktree you did not create, `cd` into another checkout,
  or write outside your worktree except to the scratch locations below.
- **Publication needs explicit instruction**: `git push`,
  `gh pr create`, pushing a submodule. Issue-tracker activity
  (`gh issue …`) is ordinary work and needs no separate approval.
- **Delegation.** Do the work yourself unless it is independent and
  larger than a handful of tool calls; a fresh-context reviewer is
  worth spawning, re-checking your own output is not. Child agents do
  not see this conversation: restate what they need. Fan-out follows
  [`docs/development/agent_isolation.md`](docs/development/agent_isolation.md).
- **Shell.** Build argument lists as arrays and expand them
  `"${ARR[@]}"`; an unquoted `$VAR` is not split in every shell. After
  a pipeline, `$?` is the last stage's status. `bca check` writes its
  offender rows to stderr, so `2>/dev/null` reads as clean. Before
  trusting a table
  a measurement loop produced, hand-run one case in full and compare;
  a tidy column of zeros is the shape every shell mistake makes. Do not
  poll for work the harness tracks; a wait loop you must write is
  bounded and keys on an artefact, not on `pgrep` of its own command
  line.
- **Tool output.** A truncated preview is not the result; read the
  persisted file or aggregate in the command (`wc -l`, `rg -c`,
  `sort -rn | head`). "There are no other X" needs the full output.
- **History rewrites.** Before a scripted rebase, `git branch
  backup/<name> HEAD`; afterwards compare trees against it, not test
  results. Build todo lists from the current `git log` only.
- **Temporary files.** Scratch goes under `$TMPDIR` or `/tmp/opencode`
  via `mktemp "${TMPDIR:-/tmp}/bca-<purpose>.XXXXXX"`, never a fixed
  name. Expensive, regenerable artefacts needed across sessions go in a
  `mktemp -d` run directory under `/var/tmp/bca/<issue>/`, each with a
  `PROVENANCE.md` naming the commit and the command that rebuilds it;
  first check `[ -O /var/tmp/bca ] && [ ! -L /var/tmp/bca ]` and stop
  if it fails. Anything irreplaceable becomes a test, a `utils/`
  script, or an issue attachment; ask when none fits, and never delete
  the only copy meanwhile.
- **Evidence for a claim** (parity, zero drift, a measurement) goes in
  the issue or PR comment: revisions, versions, the exact command, the
  numbers. A path on one machine is not a citation.
- **Issues and commits** follow Conventional Commits. Write non-trivial
  `gh` bodies to a `mktemp` file and pass `--body-file`; put a long
  record in a gist and link it (the host proxy has rejected multi-KB
  comments), and re-read the issue to confirm a comment landed. Update
  the body *and* add a comment; close only when every item is resolved.
  Re-verify a deferred issue's premises against the current tree before
  building on them.
- **Tools.** `rg` and `fd`, not `grep` and `find`. Context7 or
  `cargo doc` before web search for library documentation. Any edit
  under `.github/workflows/` runs `make actionlint`; composite actions
  under `.github/actions/` are not auto-discovered and must be passed
  explicitly.
- **Metric feedback from `bca`** (the post-edit hook, `make self-scan`)
  follows the policy below. Markers: `// bca: suppress(<metrics>) --
  <why>`; canonical names only (`nexits`, never `exit`).

## Before you start, read

| Task | Read first |
| --- | --- |
| Changing a metric, `Checker`, `Getter`, `Alterator`, a role table, or anything under `big-code-analysis-ast/src/` | [`docs/development/grammar_dispatch.md`](docs/development/grammar_dispatch.md) and the module doc of `big-code-analysis-ast/src/roles/mod.rs` |
| Adding or changing tests, perturbation sweeps, feature gates | [`docs/development/testing.md`](docs/development/testing.md) |
| A file `make rustfmt-bail` names, or consolidating code into a `macro_rules!` | [`docs/development/formatting.md`](docs/development/formatting.md) |
| AST traversal, `Checker` / `Getter`, or a metric's `compute` | [`docs/development/benchmarking.md`](docs/development/benchmarking.md) |
| `fuzz/` | [`docs/development/fuzzing.md`](docs/development/fuzzing.md) |
| Thresholds, `bca.toml`, `.bca-baseline.toml` | `CONTRIBUTING.md` "Metric baseline", [`docs/development/baseline_freshness.md`](docs/development/baseline_freshness.md), the book's `recipes/thresholds.md` |
| Any Rust change | `CONTRIBUTING.md` "Project conventions" |
| Python bindings, `uv.lock`, ruff | `CONTRIBUTING.md` "Python bindings" |
| Fanning work out to child agents | [`docs/development/agent_isolation.md`](docs/development/agent_isolation.md) |
| Adding a language or bumping a grammar | the `add-lang` skill; `big-code-analysis-book/src/developers/` |
| Writing or revising prose docs | the `doc-author` skill; `docs/conventions/documentation.md` |

## Responding to bca metric feedback

**Responding to `bca` metric feedback.** A threshold violation is a
reason to review readability, not proof of a defect. Improve the code
where that helps the current task; lowering a score is not itself a goal.

- **Do not game the metric.** Do not extract a helper that exists only
  to move complexity off one function, split a cohesive function at an
  arbitrary line, collapse readable branches into a dense expression,
  or inline/obfuscate logic to dodge the count. These lower the
  per-function score without necessarily improving readability.
  New helpers also add names and interfaces that readers must understand.
- **Refactor only when it truly clarifies.** A good split has a name
  that means something and a boundary a reader would have drawn anyway.
  If you cannot name the extracted piece without inventing a
  `foo_part2`, reconsider the split.
- **When the complexity is essential, suppress with a reason.** Some
  functions are clearest left whole, such as a
  dispatch `match`, a hand-rolled parser table, an exhaustive state
  machine. For these, do not contort the code: add a suppression marker
  with the rationale on the same line:
  `// bca: suppress(cognitive) -- exhaustive opcode dispatch`.
  A clear function with an honest marker is better than a
  "compliant" tangle.
- **Keep the fix within the task.** Review findings relevant to the
  current change. Findings may concern other functions or the whole
  file; mention unrelated findings without expanding the edit.

Attribute a score before sizing a fix: Rust's `?` feeds cyclomatic and
nexits, string literals feed halstead, inline tests feed file `sloc`.
Size any new helper against every gated metric, reading the live
`bca.toml` for the limits.

## Tone

Criticism is welcome: point out mistakes, suggest better approaches,
cite relevant standards. Be skeptical and concise.
