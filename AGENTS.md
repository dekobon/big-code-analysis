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
| `big-code-analysis-web` | `big-code-analysis-web/` | REST API server wrapping the library |
| `enums` | `enums/` (excluded from default workspace) | Code-generation helper for language enums |

Vendored / path-dependent grammar crates also live in the repo:
`tree-sitter-ccomment`, `tree-sitter-mozcpp`, `tree-sitter-mozjs`,
`tree-sitter-preproc`. External grammar crates are pinned with `=X.Y.Z`
versions in the root `Cargo.toml`.

The default branch is **`main`**.

## Project layout

- `src/lib.rs` — public re-exports; this is the published API surface.
- `src/languages/` — one `language_<lang>.rs` per supported language. These
  modules deliberately mirror each other; macros under `c_langs_macros/`,
  `src/macros.rs`, and `src/c_macro.rs` generate the shared structure. A
  bug in one language module typically exists in several — fix all
  affected siblings together.
- `src/metrics/` — individual metric implementations: `cognitive.rs`,
  `cyclomatic.rs`, `halstead.rs`, `loc.rs`, `mi.rs`, `nargs.rs`, `nom.rs`,
  `npa.rs`, `npm.rs`, `abc.rs`, `exit.rs`, `wmc.rs`.
- `src/output/` — JSON / YAML / TOML / CBOR serializers for metric output.
- `src/parser.rs`, `src/node.rs`, `src/spaces.rs`, `src/checker.rs`,
  `src/getter.rs`, `src/alterator.rs`, `src/traits.rs` — core AST plumbing.
- `tests/` — integration tests, including `insta` snapshot tests
  (`*.snap` / `*.snap.new`).
- `big-code-analysis-book/` — mdBook documentation source.
- `enums/` — separate workspace member (excluded from the root workspace)
  that generates language enum tables.
- Helper scripts: `check-grammar-crate.py`, `check-grammars-crates.sh`,
  `recreate-grammars.sh`, `split-minimal-tests.py`,
  `generate-grammars/`.

## Editing principles

- This is a published library (`big-code-analysis` on crates.io). Treat
  `lib.rs` re-exports, public traits (`ParserTrait`, `LanguageInfo`, etc.),
  and public types (`Metrics`, `FuncSpace`, language enums) as a stable API
  surface — break them only with an intentional version bump.
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

Before considering a change done, run these from the repo root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
```

If `pre-commit` is installed, also run `pre-commit run --all-files`. The
project's `.pre-commit-config.yaml` runs clippy, `cargo +nightly udeps`,
and the test suite.

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

- Do not push or open PRs without explicit user instruction.
- Only close an issue when ALL items are resolved.
- When updating issues, update BOTH the body AND add a comment.

## Tone

Criticism is welcome — point out mistakes, suggest better approaches,
cite relevant standards. Be skeptical and concise.
