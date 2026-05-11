# Changelog

All notable changes to `big-code-analysis` are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html)
from the fork onwards.

Pre-1.0 caveat: while in `0.x`, the public Rust API surface
(`big-code-analysis` library re-exports, the `bca` CLI argument grammar,
and the `bca-web` REST schema) may change between minor versions. Breaking
changes are marked with **(breaking)** in the entries below.

## [Unreleased]

### Changed

- Cargo workspace now uses `resolver = "3"` and inherits shared
  package metadata (`version`, `edition`, `rust-version`, `license`,
  `authors`) via `[workspace.package]` so the three shipping crates
  have a single source of truth. Per-crate `repository` URLs are
  preserved so each crate's crates.io page still links to its own
  subdirectory ([#150](https://github.com/dekobon/big-code-analysis/issues/150)).
- MSRV is now declared as `1.94` in `[workspace.package]`
  ([#150](https://github.com/dekobon/big-code-analysis/issues/150)).
- `[profile.release]` drops `strip = "debuginfo"` and sets
  `debug = "line-tables-only"` so release packaging can split
  symbols into separate `.dbg` artefacts and panic backtraces still
  carry line numbers. The same change applies to `enums/`'s
  independent release profile
  ([#150](https://github.com/dekobon/big-code-analysis/issues/150)).
- The 5 vendored grammars (`tree-sitter-ccomment`, `tree-sitter-mozcpp`,
  `tree-sitter-mozjs`, `tree-sitter-preproc`, `tree-sitter-tcl`) and
  the `enums` codegen helper are now marked `publish = false` and
  excluded from the workspace member list, leaving exactly three
  publishable packages (`big-code-analysis`, `big-code-analysis-cli`,
  `big-code-analysis-web`)
  ([#150](https://github.com/dekobon/big-code-analysis/issues/150)).
- The 18 shared `tree-sitter*` version pins (13 external, 5 vendored
  path-deps) are now consolidated in `[workspace.dependencies]` in the
  root `Cargo.toml`; the root crate inherits them via
  `.workspace = true`. `enums/Cargo.toml` is `[workspace].exclude`d and
  cannot inherit, so it keeps literal pins with a lockstep-update
  comment in both manifests
  ([#159](https://github.com/dekobon/big-code-analysis/issues/159)).

### Added

- Minimal `.markdownlint-cli2.jsonc` enabling `MD024 siblings_only`
  so Keep-a-Changelog repeated `### Added` / `### Changed` headers
  across version sections don't trip the no-duplicate-heading rule.
  Extended in this release with `MD013` (line_length 120,
  tables/code_blocks false) and an `ignores` list covering `target/**`,
  `node_modules/**`, `.claude/**`, `tests/repositories/**`, and
  `big-code-analysis-book/book/**`
  ([#151](https://github.com/dekobon/big-code-analysis/issues/151)).
- Contributor-facing and release-process documentation: `CONTRIBUTING.md`,
  `SECURITY.md`, `RELEASING.md`, and `.github/ISSUE_TEMPLATE/` (bug
  report and feature request)
  ([#156](https://github.com/dekobon/big-code-analysis/issues/156)).
- Supply-chain hygiene configuration at the repo root: `deny.toml`
  (cargo-deny: yanked-as-deny, license allow-list including MPL-2.0,
  wildcards-as-deny, unknown-registry/git-as-deny), `about.toml` and
  `about.hbs` (cargo-about template covering the 8 release targets),
  and a `minisign.pub` placeholder the release preflight grep-matches
  to fail fast on un-rotated keys
  ([#151](https://github.com/dekobon/big-code-analysis/issues/151)).
- Per-PR GitHub Actions pipeline (`.github/workflows/ci.yml`): `fmt`,
  `clippy`, `docs`, `test` (3-OS matrix), `msrv` (1.94 build-only),
  `feature-matrix`, `deny`, `license-audit`, `lint`, and an
  `if: always()` aggregator `ci` job intended as the single required
  status check for branch protection. All third-party actions are
  pinned to commit SHAs. The standalone `snapshot-anchors.yml`
  workflow is removed; `check-snapshot-anchors.py` now runs inside
  the new `lint` job
  ([#152](https://github.com/dekobon/big-code-analysis/issues/152)).

## [0.0.25] - 2026-05-10

> **Fork-anchor note.** Forked from Mozilla's
> [`rust-code-analysis`](https://github.com/mozilla/rust-code-analysis)
> at commit `007ee15` on 2026-04-26 and renamed to `big-code-analysis`.
> This entry consolidates all changes through the first
> public release; there were no intermediate tagged releases between
> the fork point and `0.0.25`.

### Added

#### New languages

- **Bash** — full Checker / Getter / Alterator and metric implementations.
- **C#** — full implementation with Java-parity test coverage, including
  shebang-free detection and aliased-`kind_id` variant handling.
- **Lua** — full implementation.
- **Perl** — full implementation with metrics.
- **PHP** — full implementation with per-metric test matrix at Java parity
  and integration-suite wiring into the `big-code-analysis-output` submodule.
- **Tcl** — full implementation.
- **Kotlin / Go** — promoted from default `implement_metric_trait!` stubs
  to real per-language metric implementations. Kotlin gained Checker,
  Getter, and all seven metric traits; Go gained a real Cognitive
  complexity implementation. Both languages parsed pre-fork but emitted
  default/no-op metric values.

#### New metrics and metric variants

- Per-function **Tokens** metric with markdown-report column wiring.
- **Modified cyclomatic complexity** exposed alongside the standard count
  for languages that distinguish bare-wildcard / fall-through arms.

#### CLI (`bca`)

- New `check` subcommand with a threshold engine for CI gates
  (per-metric ceilings, exit-code-driven).
- CLI restructured into **subcommand verbs** **(breaking)** — e.g.
  `bca metrics`, `bca check`, `bca find`, `bca count`. Old top-level
  flag invocations no longer work; see the migration notes in
  `big-code-analysis-book/`.
- `--list-metrics` command to enumerate every metric the binary supports.
- `-O markdown` aggregated hotspot report with `--top` and
  `--strip-prefix` flags, padded for plain-text readability.
- `-O html` aggregated hotspot report (separate from the per-file HTML
  output): hover tooltips on aggregate headers, per-language section
  tinting with a stable palette.
- Gitignore-aware path traversal and `--paths-from <file>` for piping
  pre-computed file lists.
- Mutually exclusive action flags enforced via clap `ArgGroup` so
  conflicting modes fail at parse time.
- Auto-skip files marked as generated (e.g. `@generated` headers).
- Shebang-based language detection for extensionless scripts.

#### Output formats

- **CSV** output.
- **Checkstyle XML** output (with reusable `OffenderRecord` stub).
- **SARIF 2.1.0** output for GitHub Code Scanning ingestion.
- **Clang/GCC** and **MSVC** warning-line output formats for editor /
  CI integration.
- **Self-contained HTML** per-file report.

#### REST API (`bca-web`)

- Synchronous parsing offloaded to the blocking thread pool so the
  async runtime stays responsive under load.
- Bounded tracking of orphaned blocking tasks; new requests are
  rejected with a clear status when the threshold is exceeded.
- HTTP 500 responses now sanitise internal error details before
  emission.

#### Tooling and CI

- Makefile-based developer and CI gate (`make pre-commit`,
  `make ci`); install targets built with `target-cpu=native`.
- Workspace builds the CLI and web crates by default
  (no opt-in feature flag required).
- Per-PR snapshot-anchor lint
  (`check-snapshot-anchors.py` + `.github/workflows/snapshot-anchors.yml`)
  enforced via baseline file `.snapshot-anchor-baseline.txt`.
- Scheduled `cargo-mutants` job over `src/metrics/`,
  `src/checker.rs`, and `src/getter.rs` (quarterly cron;
  auto-files GitHub issues on escapes).
- CI lint blocking *new* bare insta snapshots in `src/metrics/`.

#### Documentation

- mdBook documentation tree at `big-code-analysis-book/` with
  Recipes section, file/language detection workflow, per-output-format
  chapters, and a developer guide for adding new languages.
- `add-lang` skill under `.claude/skills/` codifying the end-to-end
  workflow for wiring a new tree-sitter language.
- Lessons learned 9–14 added to
  `docs/development/lessons_learned.md`.

### Changed

- **Project renamed** from `rust-code-analysis` to `big-code-analysis`
  (fork anchor `007ee15`).
- **Binaries renamed** **(breaking)** to `bca` (formerly
  `rust-code-analysis-cli`) and `bca-web` (formerly
  `rust-code-analysis-web`). Distribution package names follow.
- Default branch renamed from `master` to `main`.
- Integration-snapshot submodule renamed from `tests/repositories/rca-output`
  to `tests/repositories/big-code-analysis-output`
  (remote: `dekobon/big-code-analysis-output`).
- `tree-sitter` bumped to `0.26.8` (with `Node::child(u32)` signature
  adaptation in our wrapper).
- CLI `Format` enum replaced with clap `ValueEnum` derivation —
  `-O` / `--format` accepts the same values, but error messages and
  shell completions are now generated from the type.
- Output writers consolidated under a single dispatch path; HTML
  per-function metrics format folded into the unified writer set.
- `FindCfg` / `CountCfg` filter lists now stored as `Arc<[String]>`
  **(breaking, library-level)** for cheaper cloning; downstream
  callers constructing these structs by hand must wrap their
  `Vec<String>` accordingly. `bca`'s CLI internals also moved to
  `Arc<[String]>` for find/count filters.
- `FuncSpace` and `Ops` now carry a `name_was_lossy` flag so callers
  can detect when a non-UTF-8 path component was lossily converted
  for display.
- Internal cleanup: numerous `refactor:`, `chore:`, and `style:`
  commits across the workspace tightened visibility, removed dead
  code, consolidated test helpers, modernised Rust syntax, and
  bumped internal-only dependencies (e.g. `askama` 0.15 → 0.16 in
  the `enums/` codegen helper crate, which is excluded from the
  default workspace). See
  `git log 007ee15..HEAD --grep '^refactor\|^chore\|^style\|^build(deps)'`
  for the full list.

### Fixed

#### Metrics

- **Cognitive**: handle unary negation in Kotlin and Go boolean
  sequences; exclude `else` arms of Kotlin `when` expressions from
  cognitive complexity; correct sibling boolean-sequence detection;
  generalise depth-stop tracking with correct per-language
  boundaries; implement `is_else_if` for Java and C# to fix
  `else if` over-counting.
- **Cyclomatic**: skip bare wildcard `_ =>` arms in Rust standard
  CCN; remove the spurious `CaseStatement` container increment
  in Bash standard CCN.
- **Nargs**: count bare-identifier arrow-function parameters in
  JS/TS; correct Java and Kotlin argument counting.
- **Nom**: add missing comma separators in the `Stats` `Display`
  implementation.
- **Loc**: wrap parses with a synthetic `Unit` space when the
  grammar's root node is not `Unit` (e.g. languages whose root is
  `program` / `module`).
- **C#**: match all aliased `kind_id` variants so that aliased
  syntax doesn't silently fall through.

#### Output

- `dump_metrics` now uses `cognitive_sum` / `cyclomatic_sum`
  (was previously emitting per-function values where sums were
  expected).
- Eliminated panic paths in the alterator + output pipeline; added
  regression tests.
- Flattened the `String2` variant in JS / TS / TSX alterators so
  template-literal substrings serialise consistently.

#### Web (`bca-web`)

- Comment-stripping handler swaps the C++ grammar to `Ccomment`
  (matches the CLI's behaviour for plain-text comment removal).
- Explicit `serde` `derive` feature flag enabled (was relying on
  transitive activation).

#### Robustness

- Normalise CR and CRLF line endings before parsing
  (previously, lone-CR and CRLF inputs could drift line counts).
- Walk the **parent's** children in `Node::has_sibling`
  (was walking the wrong node and missing siblings).
- Spaces: handle non-UTF-8 paths via lossy conversion when
  computing the top-level space name.
- CLI: trim whitespace from `--paths-from` lines before
  `PathBuf` construction; warn instead of silently dropping when
  non-UTF-8 path components appear in `handle_path`.
- C macro lookup: switched to `binary_search` with short-circuit
  `||` for hit-path branch prediction; dropped the static
  `DOLLARS` buffer to avoid a panic on long identifiers.

#### Build / scripts / documentation

- `ops.rs` — removed stray `println!` debug output.
- `loc.rs` — fixed `cloc_min` / `cloc_max` doc comments that
  said `Ploc` instead of `Cloc`.
- `WebCommentResponse.code` doc comment corrected.
- `enums/` build script — regenerate language enums after
  grammar version bumps.
- `split-minimal-tests.py` — use a raw f-string so regex
  metacharacters in metric names aren't misinterpreted; escape
  `metric_name` before regex interpolation.
- Cargo `repository` URLs updated to reference the `main` branch.

### Removed

- HTML per-function metrics output format
  (`refactor(output): remove HTML metrics output format`,
  commit `eb57500`). HTML output remains available via the new
  self-contained per-file HTML report (commit `7af09d1`) and the
  aggregated hotspot HTML report (commit `5eb41fd`); migrate
  depending on whether you want per-file or cross-file output.

### Security

- **`bca-web`** error sanitisation: HTTP 500 responses no longer
  leak internal error details (`fix(web): sanitize internal error
  details from HTTP 500 responses`, commit `99a2691`).
- **`bca-web`** orphan-task tracking: bounded tracking of orphaned
  blocking tasks rejects new requests when a configurable threshold
  is exceeded, mitigating slow-loris-style resource exhaustion of
  the blocking thread pool (`fix(web): track orphaned blocking
  tasks and reject when threshold exceeded`, commit `94c8141`).

<!-- Release-cutter: when the v0.0.25 tag is created, retarget both
links below at `v0.0.25` (Unreleased: `v0.0.25...HEAD`; 0.0.25:
`007ee15...v0.0.25`). They currently point at `HEAD` so the
comparison links resolve before the tag exists. -->

[Unreleased]: https://github.com/dekobon/big-code-analysis/compare/HEAD...HEAD
[0.0.25]: https://github.com/dekobon/big-code-analysis/compare/007ee15...HEAD
