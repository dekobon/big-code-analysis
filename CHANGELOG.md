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

- **(breaking)** `Abc::compute` and `Cognitive::compute` now take the
  source bytes as a third parameter — `fn compute<'a>(node: &Node<'a>,
  code: &'a [u8], stats: &mut Stats)` — mirroring `Cyclomatic::compute`
  and `Exit::compute`. Languages whose control-flow constructs surface
  as untyped `Call` nodes (Elixir most notably) can identify them by
  inspecting the call target's text. Per-language impls that do not
  need the bytes discard them with `_`
  ([#206](https://github.com/dekobon/big-code-analysis/issues/206)).
- **(breaking)** `Cyclomatic::compute` now takes the source bytes as
  a third parameter — `fn compute<'a>(node: &Node<'a>, code: &'a [u8],
  stats: &mut Stats)` — mirroring `Exit::compute`. Languages whose
  branching constructs surface as untyped `Call` nodes (Elixir most
  notably) can identify them by inspecting the call target's text.
  Per-language impls that do not need the bytes discard them with
  `_`. The Elixir impl now distinguishes `if`/`unless`/`for`/`while`/
  `with`/`case`/`cond`/`try` Calls: single-branch keyword Calls
  contribute to both standard and modified CCN, while multi-arm
  container Calls (`case`/`cond`/`with`/`try`) contribute to modified
  only — per-arm `stab_clause`s carry standard CCN, mirroring the
  C-family case/switch treatment
  ([#179](https://github.com/dekobon/big-code-analysis/issues/179)).
- Workspace-wide pedantic clippy + `missing_docs` lint posture is now
  enforced. `[workspace.lints.rust]` adds `missing_docs = "warn"` and
  `[workspace.lints.clippy]` adds `pedantic = "warn"` with explicit
  carve-outs (`module_name_repetitions`, `missing_errors_doc`,
  `too_many_lines`, `similar_names`,
  `doc_markdown`, `needless_pass_by_value`, `struct_field_names`,
  `if_not_else`, `unused_self`, `match_wildcard_for_single_variants`,
  `struct_excessive_bools`, `ref_option`, each justified inline). All
  three shipping crates inherit via `[lints] workspace = true`.
  `cargo clippy --workspace --all-targets --all-features -- -D
  warnings` and the default-features variant both exit clean
  ([#158](https://github.com/dekobon/big-code-analysis/issues/158)).
- Downgraded ~254 `#[inline(always)]` attributes to `#[inline]`
  across language modules, metric modules, and the `enums/`
  template, removing the `clippy::inline_always` warnings and
  letting LLVM decide on inlining. Mechanical batch alongside
  fixes for `clippy::semicolon_if_nothing_returned`,
  `clippy::redundant_else`, `clippy::redundant_closure`,
  `clippy::items_after_statements`,
  `clippy::unnecessary_debug_formatting` (path `{:?}` →
  `path.display()` in `eprintln!` warning logs),
  `clippy::unnested_or_patterns`, `clippy::implicit_clone`,
  `clippy::manual_string_new`, `clippy::needless_raw_string_hashes`,
  and `clippy::uninlined_format_args`. Public API unchanged
  ([#158](https://github.com/dekobon/big-code-analysis/issues/158)).
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
- Promoted the workspace-excluded `enums` crate's CI gate from
  `cargo check` to `cargo clippy --all-targets --locked -- -D warnings`,
  fixing three pre-existing `clippy::manual_is_ascii_check` sites in
  `enums/src/common.rs` (replaced range-based ASCII checks with
  `c.is_ascii_lowercase()` / `is_ascii_uppercase()` / `is_ascii_digit()`).
  The gate now enforces the same lint floor as the workspace
  ([#166](https://github.com/dekobon/big-code-analysis/issues/166)).
- `kotlin_loc_no_zero_blank` test (`src/metrics/loc.rs`) rewritten to
  actually exercise its advertised contract: the input now interleaves
  a blank line between trailing-comment code so the test asserts
  `blank() == 1.0` rather than `blank() == 0.0`. The original
  no-blank-input coverage is preserved under
  `kotlin_loc_blank_zero_sanity`
  ([#200](https://github.com/dekobon/big-code-analysis/issues/200)).
- Rewrote `.github/dependabot.yml`: added a `github-actions` ecosystem
  entry (grouped, weekly, `ci:` commit prefix) so SHA-pinned action
  bumps auto-update; standardised cargo entries on `deps:` prefix and
  added `version-update:semver-major` ignore rules so MSRV-bumping
  deps no longer auto-merge; trimmed `open-pull-requests-limit` from
  99 to 5 for the five vendored grammar directories and `/enums`
  (kept 99 for `/`); added a previously-missing cargo entry for
  `/tree-sitter-tcl`
  ([#154](https://github.com/dekobon/big-code-analysis/issues/154)).
- `Node::is_child(id)` avoids the per-call `TreeCursor` heap
  allocation by walking via `child(0)` + `next_sibling()` instead
  of `children(&mut self.0.walk())`. Behaviour-preserving; total
  cost stays O(n). Hot on the JS/TS/TSX/Mozjs template-literal
  arms in `src/getter.rs`
  ([#217](https://github.com/dekobon/big-code-analysis/issues/217)).
- Lesson-9 partial-input tests split into two suites for honesty:
  16 `*_top_level_space_is_unit_contract` tests pin the public API
  contract, and `lua_partial_input_yields_synthetic_unit_wrapper`
  + `cpp_error_root_yields_unit_top_level_space` are the only two
  that today actually exercise the synthetic-Unit wrapper in
  `metrics()`. The naming was previously uniform and implied all
  18 tests exercised the wrapper
  ([#220](https://github.com/dekobon/big-code-analysis/issues/220)).

### Added

- Python `Abc` impl now counts unary `not` as one condition,
  closing the parity gap with Java / C# / Kotlin. `if not flag:`
  reports `conditions = 1` (was `0`); `not (x > 0)` reports `2`
  (`NotOperator` + `ComparisonOperator`), matching Java's
  `!(x > 0)`
  ([#214](https://github.com/dekobon/big-code-analysis/issues/214)).
- New `tcl_no_string_lloc` test pins that a multi-line Tcl
  double-quoted string literal contributes only one lloc (the
  surrounding `set` command), not one lloc per body line — mirrors
  the existing Lua / Elixir / PHP heredoc-shape coverage
  ([#210](https://github.com/dekobon/big-code-analysis/issues/210)).
- New `cpp_lambda_in_function_lloc` test covers C++11 lambda LLOC
  counting, the one language missing from #195's wave-9 nested-
  function / closure / lambda coverage
  ([#213](https://github.com/dekobon/big-code-analysis/issues/213)).
- Append `("elixir", "elixir")` to `LANGUAGE_PALETTE` in the HTML report
  with matching light- and dark-mode `section.lang-elixir{…}` CSS
  rules. Elixir-only reports now render in a distinct purple instead of
  the neutral "other" grey
  ([#187](https://github.com/dekobon/big-code-analysis/issues/187)).
- Real Ruby implementations of `Abc`, `Npa`, `Npm`, and `Wmc` metrics
  (previously default no-ops). Removes Ruby from the four
  `implement_metric_trait!` default registrations in `src/macros.rs`;
  registers concrete impls mirroring the PHP / Java / C# patterns
  including visibility-flag tracking (`private` / `public` /
  `protected`), `attr_*` macro detection, singleton methods, and
  inheritance. 52 new per-metric Ruby tests reach parity with the
  Java/C#/PHP sibling counts (Abc=14, Npa=13, Npm=13, Wmc=12)
  ([#190](https://github.com/dekobon/big-code-analysis/issues/190)).
- `Npa::compute` and `Npm::compute` now take the source bytes as a
  second parameter — `fn compute<'a>(node: &Node<'a>, code: &'a [u8],
  stats: &mut Stats)` — mirroring `Cyclomatic::compute` and
  `Exit::compute`. Languages whose visibility markers are bare
  `Identifier` text (Ruby `private` / `public` / `protected`) can now
  read the source bytes to classify them. Existing per-language impls
  that do not need the bytes discard them with `_`. The `Checker`
  supertrait is `pub(crate)`, so downstream crates cannot observe this
  change ([#190](https://github.com/dekobon/big-code-analysis/issues/190)).
- Audit and document the `implement_metric_trait!` default-impl matrix
  in `src/macros.rs`. 44 (language, metric) cells classified — 21
  genuine defaults (the language has no construct the metric measures)
  and 23 placeholders (real impls owed). 29 placeholder smoke tests
  added so a future implementation that lands without updating tests
  trips the gate. Follow-up issues filed per language (#201 Python,
  #202 JS/Mozjs, #203 Rust, #204 C++, #205 Go, #206 Elixir, #208
  Perl/Lua/Tcl)
  ([#188](https://github.com/dekobon/big-code-analysis/issues/188)).
- `Abc`, `Npa`, `Npm`, `Wmc`, `Mi` metric implementations for
  **Python**, **Rust**, **C++**, **JavaScript**, and **Mozjs**
  (previously default no-op, scored as 0/0/0). Python and JavaScript
  helpers operate via the `Npa::compute` / `Npm::compute` trait
  signatures, so prototype assignments and Python name-mangling
  visibility are documented limitations. Rust maps `Impl→Class` and
  `Trait→Interface` for Wmc aggregation; C++ tracks per-class
  visibility (public/private/protected, with class-default-private
  and struct-default-public semantics). 200+ new anchored tests
  ([#201](https://github.com/dekobon/big-code-analysis/issues/201),
  [#202](https://github.com/dekobon/big-code-analysis/issues/202),
  [#203](https://github.com/dekobon/big-code-analysis/issues/203),
  [#204](https://github.com/dekobon/big-code-analysis/issues/204)).
- `Abc`, `Npa`, `Npm` metric implementations for **Go**, plus `Mi`
  via the default cascade. `Wmc` deliberately left at zero with a
  documented regression test — Go's flat space model exposes
  `SpaceKind::Function` for both `MethodDeclaration` and free
  `FunctionDeclaration`, so per-receiver grouping isn't possible
  without space-model surgery
  ([#205](https://github.com/dekobon/big-code-analysis/issues/205)).
- `Cognitive`, `Abc` metric implementations for **Elixir** (the
  highest-impact gap from the audit — Elixir is heavily branchy and
  previously scored 0 on cognitive complexity), plus `Mi` via the
  default cascade. Recursion and `Enum.reduce` are intentionally
  omitted with documented zero-pin tests
  ([#206](https://github.com/dekobon/big-code-analysis/issues/206)).
- `Abc` metric implementations for **Perl**, **Lua**, and **Tcl**
  ([#208](https://github.com/dekobon/big-code-analysis/issues/208)).
- 18 lesson-9 synthetic-Unit regression tests in `src/spaces.rs`
  covering every supported language (Python, JS, TS, TSX, Mozjs,
  Java, Kotlin, Go, Rust, C#, Bash, Lua, Tcl, Perl, PHP, Elixir,
  Preproc, Ccomment, Ruby); only Lua exercises the synthetic-Unit
  promotion path today, the rest pin the current
  translation-unit-root contract as future-proofing
  ([#193](https://github.com/dekobon/big-code-analysis/issues/193)).
- 20 nested-function/closure/lambda LLOC tests across Python, Java,
  C#, JavaScript, Kotlin, Go, PHP, Lua, Tcl, Perl, Elixir
  ([#195](https://github.com/dekobon/big-code-analysis/issues/195)).
- Three new lesson-11 cross-language parity tests
  (`cognitive_cross_language_parity`, `exit_cross_language_parity`,
  `nargs_cross_language_parity`) covering 2-arm wildcard switches,
  loops with early exit, and 3-parameter functions; the original
  `cyclomatic_if_elseif_else_chain_cross_language` was the only
  one previously implemented
  ([#196](https://github.com/dekobon/big-code-analysis/issues/196)).
- PHP heredoc (`<<<EOT … EOT;`) and nowdoc (`<<<'EOT' … EOT;`) LOC
  regression tests
  ([#194](https://github.com/dekobon/big-code-analysis/issues/194)).
- `--exclude-tests` CLI flag (and `MetricsCfg::options.exclude_tests`
  library option) elides Rust `#[test]` / `#[cfg(test)]` /
  `#[cfg(all(test, ...))]` / `#[cfg(any(test, ...))]` and common
  test-framework attributes (`#[tokio::test]`, `#[async_std::test]`,
  `#[rstest]`, `#[test_log::test]`, `#[wasm_bindgen_test]`,
  `#[test_case]`) from metric computation, plus `mod` items carrying
  outer `#[cfg(test)]` or inner `#![cfg(test)]` attributes. The skip
  is implemented as a new `Checker::should_skip_subtree(node, code)`
  trait method (default `false`, backward-compatible — only
  `RustCode` overrides; non-Rust languages are unaffected) and runs
  upstream of every per-metric `compute` call so Halstead, Cyclomatic,
  Cognitive, LOC, NOM, WMC, ABC, NPA, NPM, Exit, NArgs, and the
  derived Maintainability Index all benefit from the same gate.
  Default is OFF (tests still counted) to preserve existing numeric
  output for downstream library consumers; the issue author's
  recommendation to flip the default is left for a follow-up.
  `MetricsOptions` and `MetricsCfg` are both `#[non_exhaustive]` so
  future option fields don't struct-literal-break downstream
  callers; construct via `MetricsOptions::default()
  .with_exclude_tests(true)` and `MetricsCfg::new(path)
  .with_options(...)` (issue [#185](https://github.com/dekobon/big-code-analysis/issues/185))
  ([#182](https://github.com/dekobon/big-code-analysis/issues/182)).
- Support for Ruby source files (`.rb`, `.rake`, `.gemspec`) via
  [`tree-sitter-ruby`](https://crates.io/crates/tree-sitter-ruby)
  `=0.23.1`. Real implementations for `Cyclomatic` (if/unless/while/
  until/for/case-when/rescue/conditional/`&&`/`||`/`and`/`or`),
  `Exit` (counting `return` statements only — `yield` does not exit
  the method), `Halstead` (full keyword-token, punctuation, and
  operator/operand classification with interpolation-aware string
  literals), `Loc` (named statement nodes, postfix modifiers, and
  class/module/method declarations), and `Cognitive`
  (`is_else_if` keyed on the dedicated `elsif` clause so chains stay
  below nested-`if` cost). `Abc`, `Mi`, `NArgs`, `Nom`, `Npa`, `Npm`,
  and `Wmc` use default impls; `Tokens` is wired through.
- Support for Elixir source files (`.ex`, `.exs`) via
  [`tree-sitter-elixir`](https://crates.io/crates/tree-sitter-elixir)
  `=0.3.5`. Real implementations for `Halstead`, `Loc`, `Cyclomatic`,
  and `Exit`; remaining metric traits use default impls. Branching
  constructs (`if`/`unless`/`for`/`while`/`with`/`case`/`cond`/`try`)
  surface as `Call` nodes with text-keyed targets and are identified
  via source-byte inspection (#179); short-circuit booleans (`&&`,
  `||`, `and`, `or`) and per-arm `stab_clause`s round out cyclomatic
  detection.
- Full binary-release pipeline (`.github/workflows/release.yml`) plus
  packaging skeletons under `packaging/`. Tagging `vX.Y.Z` on `main`
  runs preflight (tag/CHANGELOG/version-parity gates), builds release
  binaries for 8 platforms (x86_64/aarch64 across linux-gnu,
  linux-musl, freebsd, darwin, windows-msvc), assembles archives
  containing both `bca` and `bca-web` alongside `README.md`,
  `LICENSE`, `CHANGELOG.md`, and per-binary
  `THIRD-PARTY-LICENSES-bca.md` / `THIRD-PARTY-LICENSES-bca-web.md`
  (the two binaries have non-overlapping direct deps — clap/ignore
  vs actix-web/tokio/futures — so a single shared notices file would
  under-attribute one side), builds
  two `.deb`/`.rpm`/`.apk`/FreeBSD-pkg artefacts per arch (one each
  for the CLI and web crates), smoke-installs every package across
  Ubuntu 22.04/24.04, Debian 12, Rocky 9, Fedora, Amazon 2023,
  Alpine 3.20, FreeBSD 14, macOS, and Windows, then signs +
  attests + uploads them. CycloneDX SBOMs and SHA256SUMS are
  minisign-signed and SLSA-build-provenance-attested. A
  `publish-crates` job (Trusted Publishing via OIDC, order
  `big-code-analysis` → `-cli` → `-web`) and the Homebrew tap /
  Scoop bucket pushes are gated by repo vars
  (`ENABLE_CRATES_PUBLISH`, `ENABLE_HOMEBREW_TAP`,
  `ENABLE_SCOOP_BUCKET`) so the binary pipeline can ship today
  while the vendored-grammar publish strategy is still deferred
  (see [#149](https://github.com/dekobon/big-code-analysis/issues/149)).
  `Makefile` gains `release-check`, `verify-changelog`,
  `pkg-deb-local`, `pkg-rpm-local` targets to surface preflight
  drift before tagging
  ([#155](https://github.com/dekobon/big-code-analysis/issues/155)).
- `#[must_use]` on 157 public accessor methods flagged by
  `clippy::must_use_candidate` — the per-metric getter families
  under `src/metrics/` (loc, abc, halstead, npa, npm, nom, nargs,
  cyclomatic, wmc, exit, cognitive, tokens, mi) plus the
  `Alterator`, `ParserTrait`, `OffenderRecord`, `Severity`, `Node`,
  `Ast`, and preproc / tools public entry points. Callers that
  ignored the return value will now see a compiler warning
  ([#158](https://github.com/dekobon/big-code-analysis/issues/158)).
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
- Explicit `cargo check` gate (under `RUSTFLAGS="-D warnings"`) for the
  workspace-excluded `enums` codegen crate, wired into the `make
  pre-commit` / `make ci` parallel DAG, the `make lint` aggregate, the
  `.github/workflows/ci.yml` `lint` job, and the `.pre-commit-config.yaml`
  hook set. The crate stays out of the workspace (so per-PR clippy
  isn't run on codegen-only code) but its lint surface no longer
  drifts silently — the gate would have caught the `unused_imports`
  warning that motivated #162
  ([#164](https://github.com/dekobon/big-code-analysis/issues/164)).
- CodeQL scanning workflow (`.github/workflows/codeql.yml`) covering
  Rust, Python, and GitHub Actions on push to `main`, PRs to `main`,
  and a weekly Monday 06:23 UTC cron. All `uses:` are pinned to commit
  SHAs and job permissions follow least-privilege
  ([#153](https://github.com/dekobon/big-code-analysis/issues/153)).
- Top-level `LICENSE` file containing the verbatim MPL-2.0 text, so
  the references in `about.hbs` (cargo-about output) and
  `CONTRIBUTING.md` resolve and downstream consumers can find the
  license at the conventional path. `Cargo.toml`'s
  `license = "MPL-2.0"` SPDX declaration is unchanged
  ([#163](https://github.com/dekobon/big-code-analysis/issues/163)).
- Real `Abc`, `Npa`, `Npm`, `Wmc` implementations for Kotlin
  (`KotlinCode`). The four metrics now report non-zero values for
  Kotlin classes / interfaces / `object` singletons / `data class`
  / nested+inner classes / companion-object members. Java is the
  parity reference; deliberate divergences are documented in-code
  (data-class compiler-synthesized members excluded; companion-object
  members folded into the enclosing class; extension functions and
  top-level `val`/`var`/`fun` excluded from class metrics;
  primary-constructor parameter properties count as class
  attributes; `init` blocks not methods). Adds 73 new Kotlin tests
  with anchored snapshots
  ([#168](https://github.com/dekobon/big-code-analysis/issues/168)).
- Real `Abc`, `Npa`, `Npm`, `Wmc` implementations for TypeScript
  (`TypescriptCode`) and TSX (`TsxCode`), sharing one compute body
  per metric via `ts_<metric>_compute!` macros. Both languages now
  score class / interface / abstract-class / generic-class /
  parameter-property / accessor / arrow-field / overload shapes.
  Documented decisions: default-public visibility; constructor
  parameter properties count as attributes; interface `property_
  signature` → npa and `method_signature` / `abstract_method_
  signature` / `construct_signature` → npm (Java parity); method-
  overload signatures are skipped (only the implementation counts);
  arrow-function class fields count as methods, not attributes;
  getters/setters each count once. Adds 99 new TS/TSX tests
  ([#169](https://github.com/dekobon/big-code-analysis/issues/169)).
- Generated Unix manpages (`man/bca.1`, `man/bca-web.1`, and one
  `man/bca-<sub>.1` per `bca` subcommand: check, count, dump, find,
  functions, list-metrics, metrics, ops, preproc, report,
  strip-comments). Produced from the live clap derive schemas by a
  new `xtask` workspace crate that depends on `clap_mangen`. The
  pages are committed to `man/` so CI can drift-check them; the new
  `manpage` job in `.github/workflows/ci.yml` runs `cargo xtask`
  and `git diff --exit-code -- man/` on every PR. Release workflow
  stages the pages into per-OS tarballs and the DEB / RPM / Alpine
  apk / FreeBSD pkg / Homebrew formula assets so `man bca` works
  after install on every shipping channel. `Cli` and `Opts` were
  lifted from each binary's `main.rs` into the corresponding crate
  `lib.rs` so `clap_mangen` can link against them. `cargo install`
  from crates.io does not currently ship manpages (the workspace-
  root `man/` directory is outside individual crate tarballs) —
  noted as a follow-up
  ([#171](https://github.com/dekobon/big-code-analysis/issues/171)).
- 13 new C/C++ cognitive complexity tests (`c_*` in
  `src/metrics/cognitive.rs`) covering ternary, try/catch, range-
  based and nested loops, recursion, multi-label `goto`, C++11
  lambdas, switch fall-through and nesting, and macro-expanded
  control flow. The exercise locked in three documented gaps in
  the C/C++ cognitive impl — `ConditionalExpression` (now tracked
  by #172), `ForRangeLoop` (now tracked by #173), and recursion
  (a static-analysis limitation documented at the top of the
  file). FIXMEs in the new tests point at the fix issues
  ([#167](https://github.com/dekobon/big-code-analysis/issues/167)).
- ~28 new C/C++ tests across `cyclomatic`, `exit`, `halstead`,
  `nargs`, `nom`, `tokens` bringing each metric near its peer-
  language high-mark. Pinned the C-family behaviour for `goto`
  (not in cyclomatic), `throw` (not in C++ `exit`), implicit
  `this` (not counted by `nargs`), template parameter packs
  (collapse to one runtime arg), lambdas-inside-functions (closures,
  not methods), and the `&` vs `&&` Halstead separation
  ([#170](https://github.com/dekobon/big-code-analysis/issues/170)).

### Fixed

- The `cognitive`, `cyclomatic`, `nom`, `nargs`, `exit`, and `abc`
  metric `_min` getters now collapse the `usize::MAX` / `f64::MAX`
  sentinel that `Stats::default()` plants to `0.0`, so a never-observed
  space serializes to a meaningful number rather than `1.8446744e19`
  (for `usize` sentinels) or `1.7976931e308` (for `f64` sentinels).
  Mirrors the existing guards in `tokens::Stats::tokens_min` and the
  three LOC variants
  ([#227](https://github.com/dekobon/big-code-analysis/issues/227)).
- Python `match`/`case` (PEP 634, 3.10+) now contributes decision
  points to both cyclomatic and cognitive complexity, matching Rust /
  C-family / Java / JS / TS / C# / PHP / Kotlin / Go / Bash. A 2-arm
  match with a wildcard previously reported `cyclomatic_max == 1` /
  `cognitive_max == 0`; it now reports `2` and `1`. Bare `case _:`
  (no guard) is filtered, mirroring Rust's `MatchArm` rule
  ([#212](https://github.com/dekobon/big-code-analysis/issues/212)).
- Bash 2-arm `case … esac` with a `*)` catch-all arm reported
  `cyclomatic_max == 3`; the bare `*)` is Bash's analogue of the
  C-family `default:` and is now excluded from the standard count,
  matching every other switch-bearing language. Multi-value patterns
  (`a|*)`) are NOT bare and still contribute a decision
  ([#211](https://github.com/dekobon/big-code-analysis/issues/211)).
- Python `Npa` impl now deduplicates `self.x = …` bindings by
  attribute identifier text. The defensive re-init pattern
  (`__init__` + `reset` both binding `self.value`) and conditional
  initialisation (`if flag: self.x = 1; else: self.x = 2`) count
  the attribute exactly once instead of inflating by one per
  re-bind. Uses the source bytes widened into the trait by #219
  ([#215](https://github.com/dekobon/big-code-analysis/issues/215)).
- Map `elixir` and `iex` shebang interpreters to `LANG::Elixir` so
  extensionless Elixir scripts (`#!/usr/bin/env elixir`) are correctly
  identified by `guess_language`
  ([#186](https://github.com/dekobon/big-code-analysis/issues/186)).
- Guard Python `String` and Kotlin `StringLiteral` /
  `MultilineStringLiteral` Halstead op-type with `is_child(Interpolation)`
  so f-strings (`f"Hi {name}!"`) and string templates (`"Hi $name!"` /
  `"${expr}"`) no longer double-count interpolated operands, matching the
  pattern already in place for Bash (#180), C# (#183), Elixir, PHP, and
  Ruby ([#191](https://github.com/dekobon/big-code-analysis/issues/191)).
- Correct nine sibling `*_no_zero_blank` tests in `src/metrics/loc.rs`
  (Elixir, Mozjs, Tcl, Bash, TypeScript, TSX, PHP, Perl, Lua) — they
  previously used no-blank input and asserted `blank == 0`, exactly
  inverting the contract their name advertised. Each now interleaves
  blank lines with code carrying trailing comments to exercise the
  `blank = sloc - (ploc ∪ cloc lines)` union math; Elixir, Lua, and
  Perl were also split into a renamed `*_blank_zero_sanity` test plus
  a real positive-case test
  ([#189](https://github.com/dekobon/big-code-analysis/issues/189)).
- C++20 spaceship operator `<=>` (`Cpp::LTEQGT`) now classified as
  Halstead operator; previously fell through to `Unknown` and was
  excluded from `n1`/`N1`
  ([#197](https://github.com/dekobon/big-code-analysis/issues/197)).
- C++ Halstead operator set now includes `-=` (`DASHEQ`), `.*`
  (`DOTSTAR`), and `->*` (`DASHGTSTAR`); previously these three
  fell through to `Unknown`
  ([#198](https://github.com/dekobon/big-code-analysis/issues/198)).
- Perl `string_double_quoted` / `string_qq_quoted` / `backtick_quoted`
  / `command_qx_quoted` literals no longer double-count their inner
  scalar/array/hash variables when an `interpolation` child is
  present; the wrapping string is now classified as `Unknown` only
  in that case, while plain (non-interpolated) Perl strings still
  count as one operand
  ([#199](https://github.com/dekobon/big-code-analysis/issues/199)).
- JavaScript / TypeScript / TSX / Mozjs template literals
  (`` `…` ``) are now Halstead operands; previously they fell
  through to `Unknown` (plain backtick strings contributed zero,
  interpolated literals dropped the wrapper entirely)
  ([#192](https://github.com/dekobon/big-code-analysis/issues/192)).


- Bash `variable_name` and `special_variable_name` are now classified
  as Halstead operands in every parse-table context. tree-sitter-bash
  emits these node kinds under three aliased `kind_id`s (`VariableName`
  / `VariableName2` / `VariableName3`) and two for special variables
  (`SpecialVariableName` / `SpecialVariableName2`); the original
  `impl Getter for BashCode::get_op_type` matched only the unsuffixed
  variant, so assignment LHS identifiers like `name` in `name=value`
  and the `name` child of `$name` simple expansions were silently
  unclassified. All five variants are now matched, restoring the
  intended operand contribution; `bash_operators_and_operands` is
  anchored with integer assertions and its snapshot refreshed to
  match. Same lesson-2 bug class as #40 / #36 / #50 / #44 / #94 / #119.
- Halstead operand counts for interpolated Elixir strings/sigils and
  Bash `$var`/`${…}`/`$(…)`/`$((…))`-bearing strings no longer
  double-count the inner identifiers. Elixir `String` / `Charlist` /
  `Sigil` and Bash `String` / `RawString` / `AnsiCString` /
  `TranslatedString` are still classified as one operand when they
  have no interpolation child, but skip classification when an
  `interpolation` (Elixir) or `simple_expansion` / `expansion` /
  `command_substitution` / `arithmetic_expansion` (Bash) child is
  present — so the inner expression contributes once instead of the
  wrapping literal contributing in addition to it. `N2`, `n2`,
  volume, and all derived metrics for code that uses interpolated
  strings idiomatically are now correspondingly lower
  ([#180](https://github.com/dekobon/big-code-analysis/issues/180)).
- Halstead operand counts for C# `$"..."` interpolated strings no
  longer double-count the inner identifiers.
  `CsharpCode::get_op_type` now routes `InterpolatedStringExpression`
  through a conditional check (mirroring the Elixir/Bash precedents
  from #180): when the literal carries any `Interpolation` child the
  inner expressions already contribute their identifiers as operands
  and the wrapper is classified as `Unknown`; when it does not (a
  static `$"hello"` with no `{...}` substitution), the wrapper still
  counts as one operand, matching the plain-string equivalent.
  `is_string` (for the LOC comment/code classifier) is unchanged. C#
  `linq.cs` / `strings.cs` integration snapshots refresh with lower
  `n2` / `N2` / volume / effort and slightly higher MI
  ([#183](https://github.com/dekobon/big-code-analysis/issues/183)).
- Halstead operand counts for PHP `"…$var…"` / `"…{$expr}…"`
  double-quoted (`EncapsedString`) and `<<<EOT … EOT;` interpolating
  heredoc literals no longer double-count the inner identifiers.
  `PhpCode::get_op_type` now routes `EncapsedString` and `Heredoc`
  through a conditional check (mirroring #180 / #183): when the
  literal carries a `$var` (`variable_name`), `${name}`
  (`dynamic_variable_name`), or `{$expr}` (a direct `{` brace child,
  or — for heredoc — any of the above inside `heredoc_body`)
  interpolation child, the inner expressions already contribute their
  identifiers as operands and the wrapper is classified as `Unknown`;
  when it does not (a plain `"hello world"` or a heredoc whose body
  is `string_content` only), the wrapper still counts as one
  operand, matching the single-quoted `String` / `Nowdoc` equivalent.
  `is_string` (for the LOC comment/code classifier) is unchanged. PHP
  `classes.php` / `control_flow.php` / `embedded.php` / `strings.php` /
  `traits_enums.php` integration snapshots refresh with lower
  `n2` / `N2` / volume and slightly higher MI
  ([#184](https://github.com/dekobon/big-code-analysis/issues/184)).
- Makefile `EXCLUDE_DIRS` no longer glob-expands the `tree-sitter-*`
  entry into absolute paths at recipe-execution time, which had
  silently neutered `make markdown-lint`, `make shellcheck`,
  `make sh-fmt`, and `make sh-fmt-check` (each piped to `xargs -r`
  against empty input and exited 0). The glob is now quoted in
  `EXCLUDE_DIRS` and the `find`-fallback path strips the quoting so
  vendored grammar trees stay excluded in both code paths
  ([#160](https://github.com/dekobon/big-code-analysis/issues/160)).
- Cleared 96 pre-existing `markdownlint-cli2` findings now that the
  markdown-lint gate actually runs. Source edits in 10 files
  (top-level README, the two crate-level READMEs, mdBook command and
  developer chapters, `docs/file-detection.md`) reflow long prose,
  demote stray H1s to H2 where appropriate, and add accessibility
  attributes to inline `<img>` badges. The remaining flagged files
  (AGENTS.md, CLAUDE.md, and book index pages) had their findings
  absorbed by widening `.markdownlint-cli2.jsonc`: MD033 now allows
  a narrow list of inline-HTML elements (`a`, `img`, `br`, `details`,
  `summary`) for legitimately GitHub-rendered constructs, and MD060
  (table-column-style) is disabled globally for content-driven tables
  ([#161](https://github.com/dekobon/big-code-analysis/issues/161)).
- Removed unused `pub use crate::macros::*;` re-export in
  `enums/src/lib.rs`. The line could not re-export the
  `macro_rules!` definitions in `enums/src/macros.rs` (macros use a
  separate name namespace and none carried `#[macro_export]`), so the
  re-export was dead. `#[macro_use] mod macros;` continues to make
  the macros visible within the crate
  ([#162](https://github.com/dekobon/big-code-analysis/issues/162)).
- Fixed shellcheck findings (SC2164 missing `|| exit` on pushd/popd,
  SC1083 literal `}` in path, SC2086 unquoted variable expansion,
  SC2006 legacy backticks) in
  `generate-grammars/{generate-grammar,generate-mozcpp,generate-mozjs}.sh`
  and applied `shfmt` formatting to `check-grammars-crates.sh` and
  `utils/check-tools.sh`. All findings were pre-existing and were
  silently masked by the Makefile `EXCLUDE_DIRS` glob bug fixed in
  [#160](https://github.com/dekobon/big-code-analysis/issues/160)
  ([#165](https://github.com/dekobon/big-code-analysis/issues/165)).
- C++ range-based `for (x : v)` loops are now scored by cognitive
  complexity. `CppCode::compute` in `src/metrics/cognitive.rs`
  previously matched only the classic `ForStatement`; the C++11
  `for_range_loop` node was missing from the dispatch, so range-fors
  cost `0` and nested range-fors did not compound. The match arm now
  includes `ForRangeLoop` alongside `ForStatement`, so range-fors
  add `1 + nesting` like every other loop. Flipped the lock-in test
  `c_range_based_for` to assert `+1`, added `c_nested_range_based_for`
  for the compounding case, and refreshed 99 DeepSpeech integration
  snapshots in the `big-code-analysis-output` submodule
  ([#173](https://github.com/dekobon/big-code-analysis/issues/173)).
- Java enhanced-for `for (T x : c)` loops are now scored by cognitive
  complexity. `JavaCode::compute` in `src/metrics/cognitive.rs`
  previously matched only the classic `ForStatement`; the
  `enhanced_for_statement` node was missing from the dispatch, so
  enhanced-fors cost `0` and nested enhanced-fors did not compound.
  The match arm now includes `EnhancedForStatement` alongside
  `ForStatement`, so enhanced-fors add `1 + nesting` like every
  other loop. Cross-language audit also locked in regression tests
  for JS / Mozjs / TypeScript / TSX `for...of`, which the upstream
  grammars fold into the same `for_in_statement` node as `for...in`
  and were therefore already scored correctly
  ([#178](https://github.com/dekobon/big-code-analysis/issues/178)).

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
