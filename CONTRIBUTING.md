# Contributing to big-code-analysis

Thanks for considering a contribution. This document covers the
essentials; the deeper conventions live in
[`AGENTS.md`](AGENTS.md), [`CLAUDE.md`](CLAUDE.md), and the
[developer guide under `big-code-analysis-book/`](big-code-analysis-book/src/developers/).

## Ground rules

- By submitting a pull request, you agree to license your contribution
  under [MPL-2.0](https://www.mozilla.org/MPL/2.0/), matching the rest
  of the project (declared via `license = "MPL-2.0"` in the root
  `Cargo.toml`).
- Security-sensitive reports must **not** go in public issues — see
  [`SECURITY.md`](SECURITY.md) for the private disclosure channels.

## Getting started

```bash
git clone https://github.com/dekobon/big-code-analysis
cd big-code-analysis
git submodule update --init --recursive
cargo build --workspace
cargo test --workspace --all-features
```

MSRV is `1.94`, declared once in the root `Cargo.toml`
(`[workspace.package] rust-version = "1.94"`) and inherited by every
member crate.

Integration snapshots live in the
[`big-code-analysis-output`](https://github.com/dekobon/big-code-analysis-output)
submodule under `tests/repositories/`. Initialize submodules before
running the test suite, otherwise integration tests fail with a missing
fixture.

The two binaries shipped with the workspace are:

- `bca` — the CLI (`big-code-analysis-cli`):
  `cargo run -p big-code-analysis-cli --`.
- `bca-web` — the REST API server (`big-code-analysis-web`):
  `cargo run -p big-code-analysis-web --`.

## Local validation gate

`make pre-commit` is the canonical entry point for the full validation
gate. It runs, in one parallel pass:

- `cargo fmt --check`.
- `cargo clippy --workspace --all-targets -- -D warnings` in both
  default-features and `--all-features` flavours.
- `cargo test --workspace --all-features`.
- `cargo +nightly udeps`.
- Markdown / TOML / shell / Makefile lint families
  (`markdownlint-cli2`, `taplo`, `shellcheck`, `shfmt`, `checkmake`).
- `./check-snapshot-anchors.py` (see "Snapshot anchors" below).

`make ci` runs the same checks without auto-fix, mirroring the GitHub
Actions behaviour.

If GNU Make 4 or any optional tool is unavailable, fall back to the
raw cargo trio:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
```

If `pre-commit` is installed, also run `pre-commit run --all-files` —
the project's `.pre-commit-config.yaml` wires clippy, `cargo +nightly
udeps`, and the test suite into the standard hook flow.

## Snapshot anchors

Per-metric tests under `src/metrics/` use
[`insta`](https://insta.rs/) snapshot assertions. **Every
`insta::assert_json_snapshot!` call must be anchored**: a bare
`insta::assert_json_snapshot!(metric.X)` records whatever production
emitted at acceptance time — including bugs.

Each new snapshot assertion must carry one of:

- An inline expected block: `insta::assert_json_snapshot!(metric.X,
  @r###"…"###)`.
- A positive `assert_eq!` on the headline value(s) immediately above
  the snapshot call, using integer-valued accessors (`branches()`,
  `class_nargs_sum()`, `u_operators()`, …). Float magnitudes,
  averages, and Halstead volume/difficulty/effort are bit-brittle and
  not safe for exact equality.
- A `// expected: <derivation>` comment explaining what the values
  should be and why, sufficient for a reviewer to verify without
  re-deriving the metric from scratch.

The policy is enforced automatically by
`./check-snapshot-anchors.py`, which `make pre-commit`, `make ci`,
the pre-commit hooks, and the `lint` job in
[`.github/workflows/ci.yml`](.github/workflows/ci.yml) all invoke.
The per-file baseline of pre-existing unanchored snapshots lives in
[`.snapshot-anchor-baseline.txt`](.snapshot-anchor-baseline.txt); CI
fails on any *increase* against that baseline.

Background reading:

- [`AGENTS.md`](AGENTS.md), section "Validation gates" — the full
  policy and the bulk-acceptance rules around grammar bumps.
- [`docs/development/lessons_learned.md`](docs/development/lessons_learned.md),
  lesson 2 ("Tree-sitter aliases one rule across many kind_ids") and
  lesson 6 ("Snapshot tests pin behaviour, not correctness") — the
  bug classes the anchor rule exists to catch.

For bulk snapshot refresh after a grammar bump or a deliberate
metric-computation change, use `cargo insta test --accept` per test
file. Accepting snapshots one at a time via `mv *.snap.new` shifts
`assertion_line` fields and cascades stale-snapshot churn.

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
3. The parent records the new submodule SHA — `git add
   tests/repositories/big-code-analysis-output` — in the **same
   parent commit** as the metric/alterator fix.
4. After any rebase, force-push, or long-running batch fix, re-run
   the integration tests before declaring done.

See lesson 8 in
[`docs/development/lessons_learned.md`](docs/development/lessons_learned.md)
for why this matters.

## Project conventions

- **Rust style**: `cargo fmt`, clippy clean with
  `--workspace --all-targets -- -D warnings`. No `unsafe` code. Avoid
  `unwrap` / `expect` / `panic!` / `assert!` in non-test code;
  propagate errors with `?`.
- **Visibility**: prefer `pub(crate)` over `pub`; widen visibility
  only when an item is re-exported from `lib.rs`.
- **Edition**: 2024 — `let-else`, let-chains, and other 2024 features
  are available.
- **Borrowing**: prefer `&str` over `String` parameters unless
  ownership is required downstream. Never use `to_string_lossy()` on
  paths used as identifiers (map keys, JSON output, error correlation)
  — use `to_str()` with explicit error handling.
- **Per-language modules mirror each other**: a bug in one
  `src/languages/language_<lang>.rs` typically exists in several. Fix
  every affected sibling together.
- **Public API**: this is a published library on crates.io. Treat
  `lib.rs` re-exports, public traits (`ParserTrait`, `LanguageInfo`,
  …), and public types (`Metrics`, `FuncSpace`, language enums) as a
  stable surface; break them only with an intentional version bump.

## Commits and pull requests

- Follow [Conventional Commits](https://www.conventionalcommits.org/):
  `feat(scope): …`, `fix(scope): …`, `refactor(scope): …`,
  `docs(scope): …`, etc.
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

## Adding a new language or bumping a grammar

The two workflows most likely to trip a new contributor have dedicated
guides under `big-code-analysis-book/src/developers/`:

- [Adding a new language](big-code-analysis-book/src/developers/new-language.md).
- [Updating tree-sitter grammars](big-code-analysis-book/src/developers/update-grammars.md).

External grammar crates are version-pinned (`=0.23.x`, etc.) in the
root `Cargo.toml`. Do not loosen those pins without explicit approval —
a grammar bump is a deliberate, separate change and snapshot tests
shift accordingly.

## Code review

Criticism is welcome — point out mistakes, suggest better approaches,
cite relevant standards. Be skeptical and concise. Reviews focus on
correctness, API shape, and test coverage before style.

## Questions

Open a GitHub Discussion or a low-priority issue. We'd rather answer a
question than review a PR that went the wrong direction.
