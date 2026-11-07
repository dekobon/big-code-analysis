# Contributing to big-code-analysis

Thanks for considering a contribution. This document covers what a
contributor needs to set up a checkout, run the validation gate, and
open a pull request. The conventions coding agents and contributors
share live in [`AGENTS.md`](AGENTS.md); the per-topic guides it routes
to are under [`docs/development/`](docs/development/) and the
[developer guide under `big-code-analysis-book/`](big-code-analysis-book/src/developers/).

## Ground rules

- By submitting a pull request, you agree to license your contribution
  under [MPL-2.0](https://www.mozilla.org/MPL/2.0/), matching the rest
  of the project (declared via `license = "MPL-2.0"` in the root
  `Cargo.toml`).
- Security-sensitive reports must **not** go in public issues; see
  [`SECURITY.md`](SECURITY.md) for the private disclosure channels.

## Getting started

```bash
git clone https://github.com/dekobon/big-code-analysis
cd big-code-analysis
make worktree-setup
cargo build --workspace
cargo test --workspace --all-features
```

`make worktree-setup` checks out the integration corpora and, if
[uv](https://docs.astral.sh/uv/) is installed, the Python-bindings
venv. It is idempotent, so re-run it any
time the environment looks wrong, including after an interrupted corpus
checkout. That interruption leaves the submodule empty at the recorded
SHA, where a plain `git submodule update --init` is a silent no-op.

MSRV is `1.94`, declared once in the root `Cargo.toml`
(`[workspace.package] rust-version = "1.94"`) and inherited by every
member crate.

Integration snapshots live in the
[`big-code-analysis-output`](https://github.com/dekobon/big-code-analysis-output)
submodule under `tests/repositories/`. The corpus-backed tests fail
until the submodules are checked out, and each says so.

The out-of-band benchmark harness (`make bench-*`) also walks
`DeepSpeech`'s own submodules, which the corpus tests exclude and
`worktree-setup` therefore does not fetch. See
[Benchmarking](docs/development/benchmarking.md) for its recursive
checkout.

The [`cargo-fuzz`](https://rust-fuzz.github.io/book/cargo-fuzz.html)
targets (`make fuzz-*`) are likewise out of band and need a nightly
toolchain plus `cargo install cargo-fuzz --locked`; every recipe skips
with a printed reason when either is missing, so a stable-only checkout
is not blocked. See [Fuzzing](docs/development/fuzzing.md).

The two binaries shipped with the workspace are:

- `bca`, the CLI (`big-code-analysis-cli`):
  `cargo run -p big-code-analysis-cli --`.
- `bca-web`, the REST API server (`big-code-analysis-web`):
  `cargo run -p big-code-analysis-web --`.

### Optional: a faster linker

Every test binary in this workspace statically links the
[tree-sitter](https://tree-sitter.github.io/tree-sitter/) runtime and
all twenty-odd grammars, so linking, not compiling, is the
tail of an incremental `cargo test` after a one-line edit. GNU `ld`,
still the default on most Linux toolchains, is single-threaded and is
usually the slowest part of that.

This is deliberately not configured in the repository: which linker is
installed, and which one works, varies per machine, and a committed
`[target.*] rustflags` that names a missing linker breaks the build for
everyone who does not have it. Install one yourself and add it to a
**local, untracked** `.cargo/config.toml`:

```toml
# mold: https://github.com/rui314/mold (apt: mold, brew: mold)
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

For LLVM's [`lld`](https://lld.llvm.org/) (`apt install lld`, or
bundled with a Homebrew LLVM), substitute `-fuse-ld=lld`. On macOS the
platform linker is already parallel and neither is needed.
`.cargo/config.toml` *is* tracked here, carrying the `cargo xtask`
alias, so add the stanza without committing it, or put it in your
user-level `~/.cargo/config.toml` instead.

## Local validation gate

`make pre-commit` is the canonical entry point for the full validation
gate. In one parallel pass it runs:

- `cargo fmt --check` ([rustfmt](https://github.com/rust-lang/rustfmt))
  over the workspace and, by manifest path, the excluded `enums/` and
  `fuzz/` crates.
- `cargo clippy --workspace --all-targets -- -D warnings`
  ([Clippy](https://doc.rust-lang.org/clippy/)) in both
  default-features and `--all-features` flavours.
- The full test suite via `make test`
  ([`cargo-nextest`](https://nexte.st/) when present, otherwise
  `cargo test`) plus `make test-doc` for doctests.
- `cargo doc --no-deps --workspace --all-features` with
  `RUSTDOCFLAGS="-D warnings"`.
- `cargo +nightly udeps`
  ([`cargo-udeps`](https://crates.io/crates/cargo-udeps)).
- Markdown / TOML / shell / Makefile / GitHub Actions lint families
  ([`rumdl`](https://github.com/rvben/rumdl),
  [`taplo`](https://taplo.tamasfe.dev/),
  [`shellcheck`](https://www.shellcheck.net/),
  [`shfmt`](https://github.com/mvdan/sh),
  [`checkmake`](https://github.com/checkmake/checkmake),
  [`actionlint`](https://github.com/rhysd/actionlint)). Any edit
  under `.github/workflows/` must pass `make actionlint`; suppress a
  shellcheck false positive with a scoped `# shellcheck disable=`
  directive, not by loosening the configuration.
- The man-page drift gate (`cargo xtask` plus
  `utils/check-manpage-drift.py`, which also catches newly added
  pages).
- The `bca` self-scan threshold gates at both tiers: `make self-scan`
  (the CI limits) and `make self-scan-headroom` (every limit scaled by
  `BCA_HEADROOM`, default `0.95`).
- `./utils/check-snapshot-anchors.py` (see "Snapshot anchors" below).
- `make rustfmt-bail` (see
  [`docs/development/formatting.md`](docs/development/formatting.md)).
- The role-table gates `make role-census`, `make role-coverage` and
  `make role-bypass`, whose baselines are exact record sets that
  `--update` can only shrink; the module doc of
  `big-code-analysis-ast/src/roles/mod.rs` describes what each cannot
  see.
- `make check-test-lang-gates` (every test carries the `#[cfg(feature)]`
  gate of the grammars it names; see
  [`docs/development/testing.md`](docs/development/testing.md)).
- The diagnostic-prefix, safety-doc-pin, grammar-build-watch,
  excluded-manifests, ruff-lockstep, rumdl-lockstep and
  publish-metadata gates, each a `utils/check-*.py` with a `*-test.py`
  self-test that runs beside it.
- The Python lint / type-check / test / stubtest stages (see below),
  skipped per stage when a tool is absent.

`make ci` runs the same checks without auto-fix, mirroring the GitHub
Actions behaviour.

Checks that run out of band, each with its own trigger:

- `make check-test-lang-gates-compare COMPARE_REF=origin/main` after
  changing a test's feature gate; it needs a base revision.
- `make chain-audit` after changing a walk's truncate/push
  bookkeeping (`src/spaces/compute.rs`, `src/ops.rs`,
  `src/suppression.rs`, `comment_rm.rs`, `Search::act_on_node`, the
  dump walk in `ast.rs`); see
  [Benchmarking](docs/development/benchmarking.md#chain-audit).
- `make bench-scaling` after changing AST traversal, a `Checker`,
  `Getter` or a metric's `compute`: a reintroduced quadratic walk makes
  the unit tests slow rather than red. See
  [Benchmarking](docs/development/benchmarking.md).
- `make fuzz-check` after editing `fuzz/`, which `cargo clippy
  --workspace` cannot see. See [Fuzzing](docs/development/fuzzing.md).
- `utils/check-deny-manifests.sh` (cargo-deny over every tracked
  lockfile) runs in CI and `make release-check`; it needs the network.
- Mutation testing, benchmarking, fuzzing, baseline freshness and the
  advisory audit run on quarterly or weekly schedules; see
  [`docs/development/`](docs/development/).

### Reading the verdict

Both gates end with exactly one machine-readable line on stdout:

```text
BCA_GATE: pass (gate=pre-commit)
BCA_GATE: fail (gate=pre-commit, exit=2, stage=_pc-fmt)
```

Grep for a line *starting* `BCA_GATE:`. Nothing else in the repository
emits one, there is exactly one per run, and it is the last thing either
gate writes.

Do **not** read an outcome out of make's `Error N` lines. The gate is a
parallel DAG, so `make[2]: *** [… _pc-fmt] Error 2` is printed the
instant a stage fails, while other stages are still running, and it is
routinely followed by several more stages' *successful* output. In one
measured run the failing stage was reported at line 58 of a 236-line
log, with 174 lines of green output after it.

`stage=` lists every stage make reported as failing, comma-separated in
the order reported: under `-j` the first failure stops *scheduling*, but
stages already running finish and can fail too. It reads `unknown` when
the gate died before make named a stage.

**No `BCA_GATE:` line at all is a third state, not a pass.** It means
the run never finished: it crashed, was killed, or was interrupted. On
the failure path make appends its own `make: *** [… pre-commit] Error 2`
epilogue after the verdict, because the gate exits non-zero and make
says so; the verdict line does not change either gate's exit status.

Capture a run like this:

```bash
log=$(mktemp /tmp/bca-pre-commit.XXXXXX.log)
make pre-commit >"$log" 2>&1
grep '^BCA_GATE:' "$log"
```

Two habits that idiom encodes:

- **Give every run its own log path.** A fixed `/tmp/pc.log` is shared
  by every checkout, worktree, and tool on the host, and a concurrent
  run's log has been read as this one's before.
- **Read the verdict from the log, not from a reported exit status.**
  Some tooling reports the status of the last command on the line, so
  `make pre-commit >log 2>&1; echo "EXIT=$?"` announces `echo`'s
  success. That is a property of the caller, not of make; the log is
  the thing that is true either way.

A `fail` naming several stages with no code in common is usually the
host (a concurrent build slowing a fixture past its timeout), not the
change. The check is the same either way: does the failing stage's
output name something the diff touched? Re-run on a quiet host before
reading it as a finding.

If GNU Make 4 or any optional tool is unavailable, fall back to the
raw cargo trio:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features
```

If [`pre-commit`](https://pre-commit.com/) is installed, also run
`pre-commit run --all-files`; the project's `.pre-commit-config.yaml`
wires clippy, `cargo +nightly udeps`, the test suite, `ruff-check` +
`ruff-format`, and a local `mypy --strict` hook into the standard hook
flow.

### Python bindings (`big-code-analysis-py`)

The Python bindings use [`ruff`](https://docs.astral.sh/ruff/) for
lint + format and [`mypy --strict`](https://mypy.readthedocs.io/) plus
[`pyright`](https://microsoft.github.io/pyright/) for type checking.
`make pre-commit` and `make ci` run these alongside the Rust gates when
the tools are present.

**Canonical install path: `make py-bootstrap`** (requires uv:
`curl -LsSf https://astral.sh/uv/install.sh | sh`,
`brew install uv`, or `pipx install uv`). This runs
`uv sync --locked --extra dev` against the checked-in `uv.lock`, so
the resolved ruff / mypy / pyright /
[maturin](https://www.maturin.rs/) / [pytest](https://docs.pytest.org/)
versions are identical across every contributor on this path.

After editing the `dev` extra in `pyproject.toml`, run
`make py-relock`, which regenerates `uv.lock` **and** the hash-pinned
exports under `big-code-analysis-py/requirements/` (`dev.txt`,
`examples.txt`). `make py-bootstrap` refuses to silently rewrite
`uv.lock`, so lockfile churn is always a deliberate, reviewable commit.
Resolve `uv.lock` rebase conflicts by re-running `make py-relock`
rather than hand-merging the file. CI consumes `uv.lock` through the
requirements exports (`pip install --require-hashes -r …` in the
workflows), so a `uv.lock` change and its regenerated exports must land
in the same commit.

`make py-relock` re-resolves but does not *upgrade*: `uv lock` keeps a
package at its locked version as long as that version still satisfies
the requirement, so widening a bound alone changes nothing. Adopting a
newer release is a second, explicit step: `uv lock --upgrade-package
<name>` from `big-code-analysis-py`, followed by the two `uv export`
commands `py-relock` runs. #1222 raised ruff's ceiling to `<0.17` and
would have shipped a still-locked 0.15.22 without it.

Alternative install paths (`mise install` via `mise.toml`, direct
`pipx install ruff/mypy/pyright/maturin`, `python -m venv .venv &&
pip install -e ".[dev]"`) still work but bypass `uv.lock`; resolved
versions can drift from peers and from CI. They remain documented
for contributors with environment constraints that preclude uv.
None of them pins ruff, deliberately; see below.

#### The ruff version is gated

One ruff version is adopted, and `uv.lock` is where it is decided.
`make check-ruff-lockstep` (wired into `make lint`, `make pre-commit`,
`make ci`, and the pre-commit hooks) fails unless three other
declarations agree with it:

- the `ruff-pre-commit` `rev:` in `.pre-commit-config.yaml`, which must
  be `v` + the locked version, otherwise `pre-commit run --all-files`
  runs a different ruff than CI;
- the `ruff==` pin in `big-code-analysis-py/requirements/dev.txt`, the
  hash-pinned export CI installs from;
- the `ruff` bound in `pyproject.toml`, which must match the one uv
  recorded resolving against; a bound edited without `make py-relock`
  leaves CI installing a version the manifest no longer admits.

Adopting a newer ruff is therefore `uv lock --upgrade-package ruff`
plus the `uv export` pair `make py-relock` runs, then the `rev:`, all
in one commit. The gate names whichever file has fallen behind.

`make py-fmt`, `py-fmt-check` and `py-lint` run
`big-code-analysis-py/.venv/bin/ruff` when it exists and fall back to
PATH otherwise, mirroring how `py-typecheck` resolves mypy and
pyright. So the local gate runs the locked ruff as soon as you have
run `make py-bootstrap`. The alternative install paths above stay
unpinned on purpose: an exact version in `mise.toml` or the
`Dockerfile` would be a fifth copy to bump on every ruff release, in a
file no gate reads.

Then build and test:

```bash
cd big-code-analysis-py
source .venv/bin/activate
maturin develop
python -m pytest
```

Or run `make py-test` from the repo root, which performs the
maturin-develop step implicitly. CI runs the same flow across
Linux / macOS / Windows on every supported Python version,
gated by a `dorny/paths-filter` job so Rust-only PRs skip it
entirely.

The hand-written stubs `python/big_code_analysis/_native.pyi` and
`vcs.pyi` are checked against the built extension by `make py-stubtest`
(`mypy.stubtest`), which diffs names, signatures and defaults. After
changing a PyO3 signature or default, update the stub and re-run it.
PyO3 exposes a `#[new]` constructor as `__new__` and renders a computed
default as `...`, so the stub mirrors those shapes. The allowlist in
`big-code-analysis-py/stubtest-allowlist.txt` holds only deliberate
facade differences; keep it minimal.

To remove Python build artifacts (`.venv`, compiled extension,
per-tool caches, `__pycache__` trees), run `make py-clean`. For a
full wipe of both cargo `target/` and Python state, run
`make distclean`.

## Project conventions

- **Rust style**: `cargo fmt`, clippy clean with
  `--workspace --all-targets -- -D warnings`. No `unsafe` code (one
  narrow PyO3 FFI exception, documented in
  `big-code-analysis-py/src/node.rs` and gated by
  `make check-safety-doc-pin`). Avoid
  `unwrap` / `expect` / `panic!` / `assert!` in non-test code;
  propagate errors with `?`.
- **Lints**: `[workspace.lints]` carries `clippy::pedantic` and
  `missing_docs`; a workspace-excluded crate needs its own copy
  (`utils/check-excluded-manifests.py` gates it), except the five
  vendored `tree-sitter-*` grammars, whose generated sources a
  regeneration replaces wholesale (`LINTS_EXEMPT_CRATES` in that
  gate carries the rationale; their `bindings/rust/build.rs` header
  watch is the one hand-maintained part, gated by
  `make check-grammar-build-watch`). Put additional `allow`s at file
  or function level.
- **Diagnostics**: stderr messages go through the lowercase `warn` /
  `die` / `note` helpers in each crate's `diag.rs`; never spell
  `warning:` / `error:` in a literal.
- **Visibility**: prefer `pub(crate)` over `pub`; widen visibility
  only when an item is re-exported from `lib.rs`.
- **Edition**: 2024; `let-else`, let-chains, and other 2024 features
  are available.
- **Borrowing**: prefer `&str` over `String` parameters unless
  ownership is required downstream. Never use `to_string_lossy()` on
  paths used as identifiers (map keys, JSON output, error correlation);
  use `to_str()` with explicit error handling.
- **Per-language modules mirror each other**: a bug in one
  `big-code-analysis-ast/src/languages/language_<lang>.rs` typically
  exists in several. Fix every affected sibling together.
- **Public API**: this is a published library on crates.io. Treat
  `lib.rs` re-exports, public traits (`ParserTrait`, `LanguageInfo`,
  …), and public types (`CodeMetrics`, `FuncSpace`, language enums) as
  a stable surface; break them only with an intentional version bump.

## Commits and pull requests

- Follow [Conventional Commits](https://www.conventionalcommits.org/):
  `feat(scope): …`, `fix(scope): …`, `refactor(scope): …`,
  `docs(scope): …`, and so on.
- Commit in small, reviewable steps. Each commit should build and pass
  tests on its own where practical.
- When fixing a bug, add a regression test that would catch the exact
  bug if reintroduced.
- For user-visible changes (API additions, behaviour changes, bug
  fixes), add an entry to [`CHANGELOG.md`](CHANGELOG.md) under
  `## [Unreleased]`. Refactors, docs-only, and CI changes don't need a
  changelog entry.
- Open the pull request against `main`. Link the issue with
  `Fixes #NNN` in the PR body.

## Snapshot anchors

Per-metric tests under `src/metrics/` use
[`insta`](https://insta.rs/) snapshot assertions. **Every
`insta::assert_json_snapshot!` call must be anchored**: a bare
`insta::assert_json_snapshot!(metric.X)` records whatever production
emitted at acceptance time, including bugs.

Each new snapshot assertion must carry one of:

- An inline expected block: `insta::assert_json_snapshot!(metric.X,
  @r###"…"###)`.
- A positive `assert_eq!` on the headline value(s) immediately above
  the snapshot call, using integer-valued accessors (`branches()`,
  `class_npm_sum()`, `unique_operators()`, …). Float magnitudes,
  averages, and Halstead volume/difficulty/effort are bit-brittle and
  not safe for exact equality.
- A `// expected: <derivation>` comment explaining what the values
  should be and why, sufficient for a reviewer to verify without
  re-deriving the metric from scratch.

The policy is enforced automatically by
`./utils/check-snapshot-anchors.py`, which `make pre-commit`, `make ci`,
the pre-commit hooks, and the `lint` job in
[`.github/workflows/ci.yml`](.github/workflows/ci.yml) all invoke.
The per-file baseline of pre-existing unanchored snapshots lives in
[`.snapshot-anchor-baseline.txt`](.snapshot-anchor-baseline.txt); CI
fails on any *increase* against that baseline.

Background reading:

- [`docs/development/lessons_learned.md`](docs/development/lessons_learned.md),
  lesson 2 ("Tree-sitter aliases one rule across many kind_ids") and
  lesson 6 ("Snapshot tests pin behaviour, not correctness"): the
  bug classes the anchor rule exists to catch.

For bulk snapshot refresh after a grammar bump or a deliberate
metric-computation change, use `cargo insta test --accept` per test
file. Accepting snapshots one at a time via `mv *.snap.new` shifts
`assertion_line` fields and cascades stale-snapshot churn. Three traps
in that loop:

- The corpus runner aborts after its first panics, so collect with
  `INSTA_FORCE_PASS=1 cargo test --no-fail-fast` before counting what
  moved.
- A forced pass leaves an inline snapshot that moved as a
  `src/**/.*.pending-snap` file, not a `.snap.new`; review both kinds.
- `cargo insta test --accept` accepts every pending snapshot in the
  workspace, including ones an earlier run left behind. Review
  everything pending, or accept a filtered set with
  `cargo insta accept --snapshot <file>`.

## Metric baseline

`make self-scan` reads `.bca-baseline.toml`, which suppresses a known
offender only while its measurement stays at or below the record. A
change that moves a baselined metric past its record, or below it (the
`--baseline` run warns on stderr), refreshes the file in the same
commit with `make self-scan-write-baseline-headroom`; the bare
`write-baseline` recipe drops the soft-tier entries. The file is
`-merge` in `.gitattributes`: on a conflict, regenerate rather than
hand-resolve. For a change split into several commits, regenerate at
each step, since the refresh measures the working tree:

```bash
git rebase --exec 'make self-scan-write-baseline-headroom && (git diff --quiet .bca-baseline.toml || git commit -q --amend --no-edit .bca-baseline.toml)' <base>
```

The scan builds `bca` under the dev profile (a few seconds after a
Rust edit, near-instant otherwise); `BCA_SELF_SCAN_PROFILE=release`
restores CI's exact profile and `make self-scan-profile-parity` checks
the two agree. Before tightening a limit, run `bca check
--explain-threshold <metric>=<limit>`, which reports both tiers and
how many new baseline entries the change would buy; a limit set on a
cluster of existing values maximises soft-tier breach by construction.
See [Baseline freshness](docs/development/baseline_freshness.md) and
the book's [Choosing thresholds](big-code-analysis-book/src/recipes/thresholds.md).

## Integration snapshots and the submodule

Behaviour-changing fixes that touch metric computation, AST traversal,
or alterator rules also shift snapshots inside the
`big-code-analysis-output` submodule. A fix is not done until all of
the following have happened in the same parent commit:

1. `cargo test --workspace --all-features` exits clean from a fresh
   working tree (no `.snap.new` files left behind under
   `tests/repositories/big-code-analysis-output/`).
2. The accepted snapshots are committed and pushed to the submodule's
   remote (`dekobon/big-code-analysis-output`, `main` branch).
3. The parent records the new submodule SHA (`git add
   tests/repositories/big-code-analysis-output`) in the **same
   parent commit** as the metric/alterator fix.
4. After any rebase, force-push, or long-running batch fix, re-run
   the integration tests before declaring done.
5. `make role-coverage-observe` regenerates
   `.role-coverage-observed.txt`, which records the submodule SHA as
   provenance; commit it in the same parent commit, or
   `make role-coverage` fails on the stale file.

See lesson 8 in
[`docs/development/lessons_learned.md`](docs/development/lessons_learned.md)
for why this matters.

## Adding a new language or bumping a grammar

The two workflows most likely to trip a new contributor have dedicated
guides under `big-code-analysis-book/src/developers/`:

- [Adding a new language](big-code-analysis-book/src/developers/new-language.md).
- [Updating tree-sitter grammars](big-code-analysis-book/src/developers/update-grammars.md).

External grammar crates are version-pinned (`=0.23.5`, `=0.25.1`, and
so on) in the root `Cargo.toml` and in each workspace-excluded crate's
own manifest; `utils/check-excluded-manifests.py` enforces both.
`tree-sitter-language`, the shared trait shim, is the one deliberate
exception and stays caret-ranged. Do not loosen a pin without explicit
approval; a grammar bump is a deliberate, separate change that moves
snapshots and, for the `tree-sitter` runtime, the `unsafe` soundness
argument in `big-code-analysis-py/src/node.rs`. If a bug is in the
grammar rather than our wrapper, document it as upstream and work
around it visibly. The per-language checklist is
[`docs/development/grammar_dispatch.md`](docs/development/grammar_dispatch.md).

## Code review

Criticism is welcome: point out mistakes, suggest better approaches,
cite relevant standards. Be skeptical and concise. Reviews focus on
correctness, API shape, and test coverage before style.

## Questions

Open a GitHub Discussion or a low-priority issue. We'd rather answer a
question than review a PR that went the wrong direction.
