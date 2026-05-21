# Changelog

All notable changes to `big-code-analysis` are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html)
from the fork onwards.

Pre-1.0 caveat: while in `0.x`, the public Rust API surface
(`big-code-analysis` library re-exports, the `bca` CLI argument grammar,
and the `bca-web` REST schema) may change between minor versions. Breaking
changes are marked with **(breaking)** in the entries below.

The written stability contract starts at the `[Unreleased]` line below;
see [STABILITY.md](./STABILITY.md) for what is held stable in shape and
why no value stability is offered until `1.0`. Entries above the
`[Unreleased]` heading describe pre-policy behaviour.

## [Unreleased]

### Added

- Public `Ast` type for parse-once, compute-many-times analysis. Build
  one with `Ast::parse(Source)` (re-parses bytes, mirrors `analyze`)
  or `Ast::from_tree_sitter(lang, tree, code, name)` (adopts a
  caller-built `tree_sitter::Tree`, the `Source`-flavored counterpart
  of `metrics_from_tree` with no lossy path-to-name conversion). Then
  call `Ast::metrics(options)` repeatedly against the same parse —
  with different `MetricsOptions::with_only` selections, interleaved
  with a custom tree-sitter walk via `Ast::as_tree_sitter`, or cached
  across configuration changes in an analysis pipeline. `analyze` and
  `metrics_from_tree` are now thin wrappers around the same seam, so
  the per-language dispatch table lives in exactly one place. See
  [`library/parse-once.md`](big-code-analysis-book/src/library/parse-once.md)
  and [`library/ast-traversal.md`](big-code-analysis-book/src/library/ast-traversal.md)
  for working with the held `tree_sitter::Tree` directly
  ([#264](https://github.com/dekobon/big-code-analysis/issues/264)).
- `bca check --baseline <path>` and `--write-baseline <path>` flags
  for ratcheting thresholds on an existing codebase without raising
  limits. The baseline is a sorted TOML file keyed on `(path,
  function, start_line, metric)` that records today's offender set;
  a baselined function whose value has not worsened is filtered
  from threshold checks, but regressions (`current > baseline.value`)
  and new offenders still fail. Composes with in-source suppression
  markers — `--write-baseline` excludes already-suppressed functions
  by default, and `--no-suppress --write-baseline` records every
  violation for CI-auditor flows. See
  [`commands/check.md`](big-code-analysis-book/src/commands/check.md)
  and the [Baselines recipe](big-code-analysis-book/src/recipes/baselines.md)
  for the full adoption flow
  ([#99](https://github.com/dekobon/big-code-analysis/issues/99)).
- Per-language Cargo features (default: `all-languages`) so library
  consumers can compile only the grammars they need. Each supported
  language now has its own feature (`rust`, `typescript`, `python`,
  `cpp`, …) that gates the matching `tree-sitter-*` grammar crate
  in the dependency graph. The default feature set keeps the
  library's historical "every grammar compiled in" behaviour
  (`bca` and `bca-web` both pin `features = ["all-languages"]`
  explicitly); downstream library consumers can opt into a narrower
  set with `default-features = false, features = ["rust", "typescript", …]`.
  The `LANG` enum keeps every variant defined regardless of the
  active feature set; selecting a variant whose feature is off
  produces `Err(MetricsError::LanguageDisabled(LANG))` from every
  dispatch entry point. A new `LANG::is_enabled` predicate lets
  callers query the compiled-in set without going through a
  dispatcher
  ([#252](https://github.com/dekobon/big-code-analysis/issues/252)).
- New `big_code_analysis::prelude` module exposing the recommended
  entry points for the 90% case: `analyze`, `metrics_from_tree`,
  `Source`, `MetricsOptions`, `MetricsError`, `Metric`, `LANG`,
  `FuncSpace`, `CodeMetrics`, `SpaceKind`. Callers can now write
  `use big_code_analysis::prelude::*;` instead of long
  per-import lists; everything outside the prelude is still
  reachable by its fully-qualified name from the crate root
  ([#255](https://github.com/dekobon/big-code-analysis/issues/255)).
- `MetricsOptions::with_only(&[Metric])` for selective metric
  computation. Pass a slice of [`Metric`] values to restrict the
  walker to those metrics; everything outside the set is skipped at
  the per-node level (no `T::Halstead::compute`, no
  `T::Cognitive::compute`, etc.) and elided from `CodeMetrics`
  serialization output. Derived metrics auto-resolve their
  dependencies — `with_only(&[Metric::Mi])` silently adds
  `Loc + Cyclomatic + Halstead`, and `with_only(&[Metric::Wmc])`
  adds `Cyclomatic + Nom`. The `Metric` enum is `#[non_exhaustive]`
  and the backing bitfield (`MetricSet`) is exposed alongside it so
  callers can introspect which metrics were computed via the new
  `CodeMetrics::selected()` accessor. Defaults are unchanged:
  `MetricsOptions::default()` selects every metric, matching the
  pre-#257 behaviour byte-for-byte
  ([#257](https://github.com/dekobon/big-code-analysis/issues/257)).
- New library entry point `analyze(Source, MetricsOptions) ->
  Result<FuncSpace, MetricsError>` in `src/spaces.rs`. `Source<'a>`
  is `#[non_exhaustive]` and carries the language, source bytes,
  optional caller-supplied display name (`Source::name`), optional
  C++-preprocessor path (`Source::preproc_path`), and optional
  `PreprocResults`. Construct via `Source::new(lang, code)` plus
  the `with_name` / `with_preproc_path` / `with_preproc` setters.
  This is the recommended entry point for in-memory analysis —
  callers no longer need to fabricate a `&Path` to identify a
  buffer
  ([#254](https://github.com/dekobon/big-code-analysis/issues/254)).
- Parse seam for callers who already drive `tree-sitter`. New
  `Parser::from_tree(tree, code)` accepts a pre-built
  `tree_sitter::Tree` plus the matching source bytes, skipping the
  bundled parse. A non-generic `metrics_from_tree(lang, tree,
  source, path, pr, options) -> Result<FuncSpace, MetricsError>`
  dispatches on `&LANG` for the common case. The `tree_sitter`
  crate is re-exported as `big_code_analysis::tree_sitter` so
  consumers can build trees against the exact version the metric
  walker was compiled against without taking a sibling
  dependency; `LANG::get_tree_sitter_language` returns the
  matching grammar. Both seam entry points accept `tree_sitter::Tree`
  directly, so the internal `Tree` wrapper stays crate-private.
  The re-exported `tree_sitter` API and the
  `LANG::get_tree_sitter_language` return type follow the
  underlying grammar pin and are documented as value-not-stable
  in `STABILITY.md`. The `library/reuse-tree` book chapter is
  upgraded from a stub to a working example
  ([#251](https://github.com/dekobon/big-code-analysis/issues/251)).
- Top-level `STABILITY.md` documenting the versioning contract for
  the `0.x` line: which types and entry points are shape-stable,
  why no value stability is offered until `1.0`, the escape hatches
  (`Node.0`, the still-direct `tree-sitter` dependency,
  `#[doc(hidden)]` items), and the MSRV policy
  (`rust-version = "1.94"` workspace-wide). Linked from the README
  under a new "Using as a library" section
  ([#258](https://github.com/dekobon/big-code-analysis/issues/258)).
- In-source suppression markers for metric threshold checks. Comments
  matching `bca: suppress`, `bca: suppress(metric, ...)`,
  `bca: suppress-file`, `bca: suppress-file(metric, ...)`,
  `#lizard forgives`, or `#lizard forgive global` silence offending
  `bca check` violations without editing source. A new `--no-suppress`
  flag forces all markers to be ignored for CI auditors. `FuncSpace`
  gains a `suppressed: SuppressionScope` field (elided from JSON when
  empty so existing snapshots are unchanged). New public types:
  `MetricKind`, `SuppressionScope`, and `SuppressionPolicy`. Documented
  in the new *Suppression markers* book chapter
  ([#98](https://github.com/dekobon/big-code-analysis/issues/98),
  [#263](https://github.com/dekobon/big-code-analysis/issues/263)).
- **(breaking)** `AstNode` JSON output now carries a `FieldName`
  key holding the tree-sitter grammar field through which each
  node was reached (`left`, `right`, `name`, `parameters`,
  `body`, ...). Consumers can distinguish structurally
  equivalent children without grammar-specific positional
  knowledge. The `Alterator` trait's `get_ast_node` /
  `get_default` / `alterate` methods gain a `field_name:
  Option<&'static str>` parameter; downstream `impl Alterator
  for X` consumers must update signatures. `AstNode::new` keeps
  its existing signature (defaulting `field_name` to `None`)
  and a new `AstNode::with_field_name` constructor accepts the
  field. `AstNode` also gains a public `field_name` field,
  which breaks callers using positional struct construction
  ([#244](https://github.com/dekobon/big-code-analysis/issues/244)).
- Support for Groovy source files (`.groovy`, `.gradle`, `.gvy`,
  `.gy`, `.gsh`), wired up to `tree-sitter-groovy = "=0.1.2"`
  (amaanq). All twelve metric traits get real impls mirroring Java's
  with Groovy-specific extensions for closures, `juxt_function_call`,
  GStrings, the `def` keyword, and the explicit `**` / `..` / `in`
  operator tokens. Several Groovy-specific operators (Elvis `?:`,
  safe-navigation `?.`, spread `*.` / `*:`, spaceship `<=>`, regex
  `=~` / `==~`, identity `===` / `!==`, exclusive ranges `..<`,
  trait declarations) are not yet supported by the upstream grammar
  and are tracked as follow-up issues.
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

### Changed

- Consolidate the four JS-family `Getter::get_op_type` impls
  (JavaScript, MozJS, TypeScript, TSX) behind a single
  `impl_js_family_get_op_type!` macro that takes per-language operator
  and operand `extras` lists. Mirrors the existing
  `impl_cyclomatic_js_family!` / `impl_js_family_is_string!` patterns.
  Pure refactor: Halstead operator/operand classification is
  byte-identical. Adds a four-way parity regression test for
  optional-chain member access. Reviewer cross-walk of the
  consolidated table surfaced a pre-existing TypeScript
  `Checker::is_string` / `Getter::get_op_type` disagreement on
  `String2` (the `string` type-keyword alias), tracked under
  [#313](https://github.com/dekobon/big-code-analysis/issues/313)
  for follow-up ([#299](https://github.com/dekobon/big-code-analysis/issues/299)).
- Consolidate `impl Cyclomatic for JavaCode` and `impl Cyclomatic for
  GroovyCode` behind a new `impl_cyclomatic_java_like!` macro that
  takes a list of extra decision kinds (`[]` for Java, `[Assert]` for
  Groovy). Mirrors the existing `impl_npm_java_like!` /
  `impl_npa_java_like!` patterns. Adds a Java/Groovy parity regression
  test plus a Groovy-only `Assert`-arm assertion, both with
  `cyclomatic_max` and `cyclomatic_modified_max` coverage so
  one-counter regressions can't slip past
  ([#300](https://github.com/dekobon/big-code-analysis/issues/300)).
- Introduce `impl_simple_is_string!($lang, $variants...)` and apply it
  to 17 single-or-flat-variant `Checker::is_string` impls (Preproc,
  Ccomment, Cpp, Python, Java, Csharp, Rust, Go, Kotlin, Perl, Lua,
  Bash, Tcl, Php, Elixir, Ruby, Groovy). The JS family keeps its
  dedicated `impl_js_family_is_string!` because of its
  `String + String2 + TemplateString + per-variant String3` shape.
  Adds per-variant positive coverage for every consolidated language
  plus negative coverage for all 17, with drift-marker assertions
  pinning the hidden grammar supertypes (`Java::MultilineStringLiteral`,
  `Groovy::StringLiteral2`, `Php::String3`) so a future grammar
  revision that promotes them surfaces in CI
  ([#301](https://github.com/dekobon/big-code-analysis/issues/301)).
- `tests/suppression_test.rs::deeply_nested_function_suppression_does_not_overflow_stack`
  rewritten in JavaScript (100 nested `function f<i>() { … }`)
  and unignored. The previous fixture used a 1000-level Python `def`
  pyramid whose ~1M whitespace bytes of indent took ~229 s to parse
  under tree-sitter-python's effectively O(N²) layout cost, so the
  test was marked `#[ignore]` and never ran on the default gate —
  meaning the iterative-suppression regression guard added by #292
  was effectively unprotected. The JavaScript fixture parses in
  ~0.8 s while preserving the deeply-nested integration path
  (parse → walk → suppression attachment), and an added
  `space.suppressed.is_empty()` assertion catches a regression
  where a function-scoped marker bubbles up to file scope
  ([#308](https://github.com/dekobon/big-code-analysis/issues/308)).
- Elixir `Wmc` / `Npm` / `Npa` now classify `def` / `defp` /
  `defmacro` / `defmacrop` calls inside `defmodule` blocks as
  methods and `defstruct` argument lists as attribute fields,
  instead of pinning each metric to zero on ordinary Elixir module
  code. The trait surface gains source-aware predicates
  (`Checker::is_func_space_with_code`, `Checker::is_func_with_code`,
  `Getter::get_space_kind_with_code`) with default-forwarding impls
  so non-Elixir languages need no override, and the walker threads
  the source bytes through to let the Elixir `Checker` disambiguate
  macro-shaped `Call` nodes by their target identifier text. `def`
  and `defmacro` are public (count in `class_nm_sum`); `defp` and
  `defmacrop` are private (counted in `class_wmc_sum` but not
  `class_nm_sum`, matching Java's npm semantics); a user-defined
  macro called `custom_def` is **not** misclassified as a method
  because the dispatch matches the literal target lexeme.
  Snapshot averages / min / max shifted across 16 Elixir snapshot
  files as the new Function / Class spaces changed the denominator
  (sums and decision-point counts are unchanged), and 10
  cyclomatic Elixir tests had their totals bumped by +2 from the
  `Stats::default()` entry seeds on the new spaces
  ([#275](https://github.com/dekobon/big-code-analysis/issues/275)).
- C# bare-discard switch-arm detection in `src/metrics/cyclomatic.rs`
  now dispatches through a private `PatternKind` enum + `classify_pattern`
  helper instead of five interleaved mutable booleans. Behavior is
  preserved (existing #282 regression tests still pass); two new tests
  cover typed-discard (`int _ =>`) and guarded var-underscore
  (`var _ when g =>`) paths
  ([#303](https://github.com/dekobon/big-code-analysis/issues/303)).
- `apply_suppression` (`src/spaces.rs`) now matches the file-scope
  target on `SpaceKind::Unit` explicitly instead of taking
  `state_stack.first_mut()`. The function-scope arm already used an
  explicit `SpaceKind::Function` predicate; this aligns the two arms
  so a future regression that leaves a non-Unit frame at index 0
  silently drops the file marker rather than attaching it to an
  arbitrary frame. New tests pin both the positive case (Unit root
  accepts the marker) and the defensive case (no Unit frame anywhere
  on the stack is a silent no-op)
  ([#306](https://github.com/dekobon/big-code-analysis/issues/306)).
- Extracted the `cfg(...)` predicate parser from `src/checker.rs`
  (~217 lines of string-level parsing plus five `cfg_*` helpers) into
  a dedicated `src/cfg_predicate.rs` module with a single
  `pub(crate) fn attribute_marks_test` entry point. Helpers and the
  regression tests added by #278 move with the parser. Aligns with
  the existing `c_macro.rs` / `preproc.rs` / `suppression.rs` pattern
  of top-level helper modules; pure extraction, zero behavior change
  ([#304](https://github.com/dekobon/big-code-analysis/issues/304)).
- Replaced the `FunctionDefinition4` source-grep regression test in
  `src/spaces.rs` (which read `src/checker.rs` and `src/getter.rs`
  from disk and string-matched their bodies) with documenting
  comments at the four C++ predicate call sites. The production
  `matches!` patterns already enumerate every `Cpp::FunctionDefinition`
  alias by name and are themselves the structural contract; the grep
  test was brittle to cosmetic edits and could pass vacuously
  ([#302](https://github.com/dekobon/big-code-analysis/issues/302)).
- Tightened the `Npm` and `Npa` Java/Groovy annotation-type tests to
  use `check_func_space` so each one additionally asserts that the
  `AnnotationTypeDeclaration` opens a `SpaceKind::Interface`
  FuncSpace named `Marker`, mirroring the sibling `Wmc` tightening
  in commit `ba2a8e3`. Factored the six annotation-type assertion
  blocks across `npm.rs` / `npa.rs` / `wmc.rs` into a single
  `tools::assert_child_space_kind(...)` test helper
  ([#307](https://github.com/dekobon/big-code-analysis/issues/307)).
- Tightened the `Npm` and `Npa` plain interface / class / trait
  tests with the same `check_func_space` + `assert_child_space_kind`
  pattern from #307. Each non-zero `interface_*_sum` assertion in
  `src/metrics/npa.rs` and `src/metrics/npm.rs` is now paired with
  a structural check that the corresponding declaration opens a
  `SpaceKind::Interface` (or `Class` / `Trait` for sibling spaces),
  so dropping `InterfaceDeclaration` / `TraitDeclaration` from a
  language's `is_func_space` no longer leaves the body-walker totals
  passing vacuously against the file-level Unit space. The Go test
  retains its pre-existing `check_metrics` form because
  `GoCode::is_func_space` does not promote `interface_type` to a
  FuncSpace at all — its `interface_*_sum` totals come from
  AST-level body walking, not the FuncSpace tree, and so are
  outside the failure mode this issue guards against
  ([#311](https://github.com/dekobon/big-code-analysis/issues/311)).
- **(library API, breaking)** `LANG::get_tree_sitter_language`
  returns `Result<tree_sitter::Language, MetricsError>` instead of
  `tree_sitter::Language` directly. Feature-gated builds need a
  way to report "this variant's grammar isn't compiled in" and
  panicking would violate the no-panic rule on disabled-language
  paths; the new signature surfaces the disabled state as
  `Err(MetricsError::LanguageDisabled(LANG))`. Callers that
  previously wrote `.set_language(&LANG::Rust.get_tree_sitter_language())`
  need to add `.expect("rust feature enabled")` (or propagate the
  error). This method is part of the value-not-stable surface (see
  STABILITY.md); the matching `action::<T>` shim was widened from
  `T::Res` to `Result<T::Res, MetricsError>` for the same reason
  ([#252](https://github.com/dekobon/big-code-analysis/issues/252)).
- **(library API)** `src/lib.rs` re-exports are now explicit:
  every previous `pub use module::*` glob has been replaced with a
  named `pub use module::{X, Y, Z}` list. Helpers that were only
  ever called from inside the crate but accidentally became part
  of the published surface via those globs are now `pub(crate)`.
  The known curated public types (`analyze`, `Source`,
  `MetricsOptions`, `MetricsError`, `Metric`, `MetricSet`, `LANG`,
  `FuncSpace`, `CodeMetrics`, `SpaceKind`, `Node`, `Cursor`,
  the per-language `<Lang>Code` / `<Lang>Parser` tags, the
  `metrics` / `output` sub-modules, the `tree_sitter` re-export,
  and the deprecated path-positional shims) keep their crate-root
  paths so the CLI, web crate, integration tests, and the book
  examples continue to compile unchanged. The published API as
  rendered by `cargo doc` is now noticeably smaller
  ([#255](https://github.com/dekobon/big-code-analysis/issues/255)).
- `ParserTrait`, the per-metric compute traits (`Cognitive`,
  `Cyclomatic`, `Halstead`, `Loc`, `Mi`, `Nom`, `NArgs`, `Exit`,
  `Abc`, `Npa`, `Npm`, `Tokens`, `Wmc`), and the supporting
  `Checker` / `Getter` / `Alterator` traits are now
  `#[doc(hidden)]`. `Parser<T>` and `Filter` are also `#[doc(hidden)]`.
  The generic `ParserTrait`-bound shims (`metrics`,
  `metrics_with_options`, `operands_and_operators`, `find`, `count`,
  `function`, `rm_comments`) keep their signatures (they remain
  callable from the CLI / web crates) but are likewise
  `#[doc(hidden)]` so they no longer appear in the curated rustdoc
  surface. `metrics` and `metrics_with_options` additionally carry
  `#[deprecated]` in favour of `analyze` (see #253 / #254). The non-generic
  `analyze` / `metrics_from_tree` / `get_function_spaces*` /
  `get_ops` entry points are now the documented surface for
  language-dispatched analysis. `Callback` and `action::<T>`
  remain documented and unchanged; their fate is tied to the REST
  API shape and will be re-evaluated separately
  ([#256](https://github.com/dekobon/big-code-analysis/issues/256)).
- **(breaking)** Removed `FuncSpace::name_was_lossy`. The new
  `analyze` entry point makes the top-level name an explicit
  caller-supplied `Option<String>` (via `Source::name`), so the
  lossy-conversion workaround disappears. The deprecated path-
  positional shims (`metrics`, `metrics_with_options`,
  `get_function_spaces`, `get_function_spaces_with_options`) still
  derive `FuncSpace::name` from `path` via lossy UTF-8 conversion
  for backwards compatibility but no longer surface a `name_was_lossy`
  bit. Downstream consumers that read `name_was_lossy` from
  serialized output must drop that field; consumers that need a
  stable identifier should pass `Source::name` directly via the
  new `analyze` entry point
  ([#254](https://github.com/dekobon/big-code-analysis/issues/254)).
- The path-positional entry points (`metrics`, `metrics_with_options`,
  `get_function_spaces`, `get_function_spaces_with_options`) are
  now `#[deprecated(since = "0.0.26", …)]` in favour of
  `analyze(Source, MetricsOptions)`. They remain functional for one
  minor release. The CLI and web crate still call the deprecated
  shims internally (they always have a `&Path` in hand); library
  consumers should migrate to `analyze`
  ([#254](https://github.com/dekobon/big-code-analysis/issues/254)).
- **(breaking)** Library entry points now return
  `Result<FuncSpace, MetricsError>` (and `Result<Ops, MetricsError>` /
  `Result<Vec<Node>, MetricsError>` for the sibling APIs) instead of
  `Option<…>`. Affected: `metrics`, `metrics_with_options`,
  `get_function_spaces`, `get_function_spaces_with_options`,
  `operands_and_operators`, `get_ops`, and `find`. The new
  `MetricsError` enum (`#[non_exhaustive]`, implements
  `std::error::Error` + `Display`) distinguishes empty-input
  (`EmptyRoot`), disabled-language (`LanguageDisabled(LANG)`),
  non-UTF-8 paths (`NonUtf8Path`), and strict-mode parse errors
  (`ParseHasErrors`); only `EmptyRoot` is produced today, the rest
  are reserved for the matching follow-up issues (#252, #254, and a
  future strict-parse toggle). The CLI and web crates adapt; the
  REST `WebMetricsResponse.spaces` schema is intentionally
  unchanged and keeps `Option<FuncSpace>` (parallels the
  `AstResponse.root` decision)
  ([#253](https://github.com/dekobon/big-code-analysis/issues/253)).
- Bumped `jsonschema` from `0.46.4` to `0.46.5` (patch: percent-
  encoded characters in `$ref` URI fragments are now decoded when
  stored as `schema_path`)
  ([#237](https://github.com/dekobon/big-code-analysis/issues/237)).
- Bumped seven GitHub Actions to their latest pinned versions:
  `actions/checkout` v4.3.1 → v6.0.2 (mutation-test.yml),
  `EmbarkStudios/cargo-deny-action` v2.0.17 → v2.0.18,
  `taiki-e/install-action` v2.62.x → v2.78.2,
  `actions/setup-python` v5.6.0 → v6.2.0,
  `actions/setup-node` v5.0.0 → v6.4.0,
  `github/codeql-action` v4.35.2 → v4.35.5,
  `actions/upload-artifact` v4.6.2 → v7.0.1
  (mutation-test.yml). Also corrected a stale `# v2.62.23`
  comment in release.yml that sat next to the v2.78.2 SHA
  ([#238](https://github.com/dekobon/big-code-analysis/issues/238)).
- **(breaking)** Offender-record output formats (Checkstyle, SARIF,
  clang/GCC warning lines, MSVC warning lines) moved from `bca metrics
  --output-format <fmt>` to `bca check --output-format <fmt>` with a
  new `--output <path>` option. `bca metrics` keeps the per-file
  serializations (`json` / `yaml` / `toml` / `cbor` / `csv`). Legacy
  invocations now exit with a migration hint pointing at the new
  command; the empty-document placeholder behaviour is removed. The
  CLI version bumps to `0.1.0` and the book chapters for `metrics`,
  `report`, and `check` are updated to be internally consistent about
  which command owns which output kind
  ([#235](https://github.com/dekobon/big-code-analysis/issues/235)).
- Python `case_clause` bare-`_`-plus-guard classifier is now shared
  between `Cyclomatic for PythonCode` and `Abc for PythonCode` via
  a single `python_case_clause_counts` helper in
  `src/metrics/npa.rs`. No behaviour change; pure code-quality
  refactor ([#223](https://github.com/dekobon/big-code-analysis/issues/223)).
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
  and `cpp_error_root_yields_unit_top_level_space` are the only two
  that today actually exercise the synthetic-Unit wrapper in
  `metrics()`. The naming was previously uniform and implied all
  18 tests exercised the wrapper
  ([#220](https://github.com/dekobon/big-code-analysis/issues/220)).

### Fixed

- TypeScript and TSX Halstead now classify the `string` type-keyword
  alias as an operand, matching `Checker::is_string`. The tree-sitter
  TS / TSX grammars expose the `string` keyword used in type
  annotations (`: string`) as an anonymous alias of `String` —
  `Typescript::String2` (kind_id 135) in TS and `Tsx::String3`
  (kind_id 141) in TSX. `Checker::is_string` matched both (#283),
  but `Getter::get_op_type` for `TypescriptCode` and `TsxCode`
  dropped them to `Unknown`, so every `: string` annotation was
  silently undercounted by one Halstead operand. `String2` is now
  in `operand_extras` for TypeScript and `String3` is now in
  `operand_extras` for TSX, restoring per-language parity with the
  JS / MozJS / TSX (for `String2`) classifications and closing the
  Checker/Getter agreement gap. Cross-language regression covered
  by `ts_family_string2_string3_type_keyword_parity_313` in
  `src/metrics/halstead.rs`
  ([#313](https://github.com/dekobon/big-code-analysis/issues/313)).
- The Rust `cfg(...)` slow-path whitespace collapser in
  `cfg_predicate::attribute_marks_test` now decodes UTF-8 correctly.
  The previous implementation rebuilt the compact string with
  `bytes().filter(...).map(char::from).collect()`, treating each
  byte as a Latin-1 codepoint and mangling any multi-byte UTF-8
  sequence (e.g. `é` / `0xC3 0xA9` became `Ã©`). The fix iterates
  over `chars()` so multi-byte sequences survive intact. Latent
  today — `matches_test` only recognises ASCII identifiers, so no
  current cfg rule could observe the mangling — but the pattern was
  wrong by construction and would have surprised any future rule
  that keyed off a non-ASCII identifier
  ([#312](https://github.com/dekobon/big-code-analysis/issues/312)).
- Elixir `Wmc` and `Npm` now agree on the methods of a class. A
  `def` / `defp` / `defmacro` / `defmacrop` nested inside a
  `quote do … end` template is no longer promoted to a Function
  space — that syntax tree is a code template emitted on macro
  expansion, not a real method of any enclosing `defmodule`. Before
  this fix, `Wmc` walked the entire Function-space subtree under a
  Class and counted the quoted `def`s, while `Npm` filtered by
  direct children of the module's `do_block` and excluded them. A
  new `Checker::promotes_to_func_space_with_code` predicate
  centralises the func-space decision (default impl forwards to
  `is_func_with_code || is_func_space_with_code`); Elixir overrides
  it to consult `elixir_is_inside_quote_block` once per `Call`
  node, replacing what was previously three independent
  `elixir_call_keyword` lookups per Call in the walker
  ([#310](https://github.com/dekobon/big-code-analysis/issues/310)).
- `bca check --baseline` now produces injective baseline keys for
  Windows non-UTF-8 paths. The Windows branch of `normalize_path`
  previously fell back to `to_string_lossy()`, which substitutes
  U+FFFD for invalid UTF-16 surrogates and could collide two
  distinct paths onto one baseline entry. The fix walks the WTF-16
  sequence with `OsStrExt::encode_wide`, decodes valid scalar
  values as UTF-8 (sharing the per-byte percent-encoder with the
  Unix branch), and emits `%uHHHH` (a marker disjoint from `%XX`)
  for unpaired surrogates so distinct invalid-surrogate inputs map
  to distinct keys. A `cfg(not(any(unix, windows)))` fallback
  preserves the U+FFFD prefix anti-collision marker for wasm-like
  targets, and the encoder is covered by always-on unit tests
  plus a `#[cfg(windows)]` integration test
  ([#305](https://github.com/dekobon/big-code-analysis/issues/305)).
- `bca check --baseline` no longer collides a UTF-8 path containing
  the literal text `%FF` with a non-UTF-8 path containing the byte
  `0xFF`. The UTF-8 fast path in `normalize_path` previously emitted
  `%` verbatim while the non-UTF-8 branch percent-encoded it,
  producing the same key (`foo%FF.rs`) for both inputs. The encoder
  is now total: every byte that is not in the unreserved set —
  including `%` — is escaped, so the UTF-8 input becomes `foo%25FF.rs`
  and remains disjoint from the non-UTF-8 key. **(breaking)** The
  baseline schema is bumped to `version = 2`; v1 baselines containing
  non-ASCII or `%`-bearing paths must be regenerated with
  `bca check --write-baseline` (the version mismatch surfaces the
  existing "regenerate" hint instead of silently failing to match)
  ([#298](https://github.com/dekobon/big-code-analysis/issues/298)).
- `Halstead` (C#) keys predefined type keywords (`int`, `string`,
  `bool`, `object`, …) by source text instead of collapsing every
  keyword onto a single `n1` slot. The fix flips
  `CsharpCode::is_primitive` to return true for
  `Csharp::PredefinedType` so the finalization path stores the node
  under its lexeme, mirroring how C++ `PrimitiveType` is keyed.
  `n1`, vocabulary, volume, and downstream MI now reflect the real
  number of distinct type keywords in C# source
  ([#286](https://github.com/dekobon/big-code-analysis/issues/286)).
- `Halstead` (Perl) recognises heredoc literals
  (`Perl::HeredocBodyStatement`) as both string-filter targets and
  operand sources. Inert heredocs contribute one operand; heredocs
  carrying `$var` / `@var` interpolation drop to `Unknown` so the
  inner substitution attributes exclusively (no double-count)
  ([#287](https://github.com/dekobon/big-code-analysis/issues/287)).
- `Halstead` (Tcl) guards `Tcl::QuotedWord` against double-counting
  embedded `$var` / `[cmd]` substitutions. Inert
  `"hello world"` strings still count as one operand; strings
  containing `VariableSubstitution` or `CommandSubstitution`
  classify as `Unknown` so the inner substitution carries the
  count. Matches the existing PHP / Bash / C# / Kotlin / Elixir /
  Ruby / Python interpolation guards
  ([#277](https://github.com/dekobon/big-code-analysis/issues/277)).
- PHP string-like node handling is now consistent across the
  checker, alterator, and Halstead getter. `Php::String2` and
  `Php::String3` (the anonymous "string" type-keyword alias and
  the hidden supertype) are recognised by `is_string` and
  `alterate`, and `ShellCommandExpression` (backtick command
  literals) now contributes a Halstead operand — gated by
  `php_string_has_interpolation` so interpolated backticks do not
  double-count
  ([#288](https://github.com/dekobon/big-code-analysis/issues/288)).
- `Abc` (C#) now counts unary and single-token `for`-loop
  conditions (`for (; ready ;)`, `for (; Ok() ;)`,
  `for (; true ;)`) via an explicit `ForStatement` arm that mirrors
  the existing Java logic. Empty conditions still contribute zero;
  comparison conditions retain their existing operator-arm
  contribution
  ([#279](https://github.com/dekobon/big-code-analysis/issues/279)).
- C++ now classifies `Cpp::FunctionDefinition4` as a function
  space. `is_func_space`, `get_func_space_name`, and
  `get_space_kind` all handle the fourth aliased
  `function_definition` kind identically to the other three, so
  C++ functions emitted through that alias keep their
  function-space identity instead of falling through to
  `SpaceKind::Unknown`
  ([#285](https://github.com/dekobon/big-code-analysis/issues/285)).
- Java and Groovy `enum`, `record`, and `@interface` declarations
  are now recognised as class-like spaces, so `Npa`, `Npm`, and
  `Wmc` walk their bodies and produce non-zero counts on common
  declaration forms. Enum / record bodies map to
  `SpaceKind::Class`; annotation-type bodies map to
  `SpaceKind::Interface` (annotation elements are abstract methods
  at the bytecode level)
  ([#280](https://github.com/dekobon/big-code-analysis/issues/280)).
- Optional chaining (`?.`) is now normalised across the JS family.
  TypeScript and TSX Halstead used to count both
  `OptionalChain` (the wrapping kind) and `QMARKDOT` (the bare
  token); the wrapper is now dropped so each textual `?.`
  contributes exactly one operator. JS-family cyclomatic now adds
  +1 per `?.` short-circuit (`OptionalChain` for JS/MozJS,
  `QMARKDOT` for TS/TSX) so the construct is treated as a
  decision point like `&&` / `||` / `??`
  ([#281](https://github.com/dekobon/big-code-analysis/issues/281)).
- Cyclomatic no longer over-counts wildcard switch arms in C# or
  Kotlin. C# `SwitchExpressionArm` with a bare `_` discard pattern
  (or `var _` declaration pattern) is skipped; guarded discards
  (`_ when g => …`) still count via the `WhenClause`. Kotlin
  `WhenEntry` is detected as the `else` arm via the absence of the
  `condition` field and skipped
  ([#282](https://github.com/dekobon/big-code-analysis/issues/282)).
- `Checker::is_string` (JavaScript / MozJS / TypeScript / TSX) now
  includes the anonymous `String2` (and TSX `String3`) aliases that
  the generated language enums map to `"string"`. The public
  `bca find string` / `count string` filters were previously
  silently dropping string literals on these alias kinds
  ([#283](https://github.com/dekobon/big-code-analysis/issues/283)).
- `Checker::is_else_if` (Python) detects `else: if …` chains
  wrapped in `else_clause`, matching the C++/JS/TS/TSX/Rust
  pattern. The `elif_clause` shape was already handled
  structurally by the cognitive metric via
  `increment_branch_extension`, so the predicate stayed false for
  that case by design; this is now documented inline. A regression
  test pins `if / elif / elif / else` cognitive at the documented
  flat-chain score so future refactors cannot silently re-nest the
  chain
  ([#274](https://github.com/dekobon/big-code-analysis/issues/274)).
- Cyclomatic for C++ `do { … } while (…)` / `for (auto x : …)` and
  Java/Groovy `do { … } while (…)` / `for (Foo x : …)` is now
  pinned by regression tests against the C-family keyword-token
  semantics (`While` / `For` already fire +1 via the trailing or
  leading keyword inside `DoStatement` / `ForRangeLoop` /
  `EnhancedForStatement`). The match-arm doc comments now spell
  out the contract so a future contributor cannot misread the
  keyword-token approach as a missing statement-node arm and
  introduce a double-count
  ([#284](https://github.com/dekobon/big-code-analysis/issues/284)).
- `rust_attribute_marks_test` now recognises the `test` predicate
  anywhere inside a `cfg(...)` attribute, not just as the first
  argument of `cfg(all(...))` / `cfg(any(...))`. Forms like
  `#[cfg(all(unix, test))]` and `#[cfg(any(feature = "x", test))]`
  are now elided when `MetricsOptions::exclude_tests()` is set; the
  walker refuses to descend into `not(...)` so `cfg(not(test))`
  and `cfg(all(unix, not(test)))` correctly remain production
  code, and `cfg(feature = "test")` (a feature literally named
  `"test"`) is not treated as a test predicate
  ([#278](https://github.com/dekobon/big-code-analysis/issues/278)).
- The C/C++ macro-masking prepass now tracks lexical context, so
  identifiers inside string literals (`"DBG"`), char literals
  (`'D'`), single-line comments (`// DBG`), multi-line comments
  (`/* DBG */`), and raw string literals (`R"delim(DBG)delim"`)
  are no longer rewritten. The synthetic parse buffer now matches
  real preprocessor semantics — macro masking only affects
  identifier occurrences a real C/C++ preprocessor could expand
  ([#290](https://github.com/dekobon/big-code-analysis/issues/290)).
- C/C++ `#include` resolution now preserves caller-relative `..`
  segments. `guess_file` joins the include path against the
  including file's parent before lexical normalisation, then
  matches candidates against the fully resolved relative target
  before falling back to basename / same-directory / distance
  heuristics. `#include "../foo.h"` no longer collapses to the
  basename and can no longer pick a sibling header with the same
  name in a different directory
  ([#297](https://github.com/dekobon/big-code-analysis/issues/297)).
- `bca` per-file output and baseline identity keys preserve
  non-UTF-8 path components instead of dropping them lossily.
  Output filenames carry the raw byte sequence as `OsString`, so
  two distinct non-UTF-8 paths produce two distinct output files.
  Baseline keys percent-encode non-UTF-8 bytes (Unix) so the
  TOML-stable key is injective across distinct paths; UTF-8 paths
  retain the prior byte-identical key
  ([#295](https://github.com/dekobon/big-code-analysis/issues/295)).
- `bca-web` plain-endpoint tests now exercise the same
  `application/octet-stream` `guard::Header` that the production
  `/comment`, `/metrics`, and `/function` routes are installed with
  in `run()`. The previous tests mounted bare handlers without the
  guard and sent `text/plain` requests — succeeding on a routing
  shape that would not exist in deployment. New
  `*_rejects_text_plain` cases lock in the guard contract by
  asserting a 404 when the content type does not match. No
  production routing change; this is a test-fidelity fix
  ([#294](https://github.com/dekobon/big-code-analysis/issues/294)).
- `bca-web` now re-checks the orphaned-task cap after acquiring a
  semaphore permit, closing a race where a burst of queued requests
  could all pass the pre-admission check while the orphan counter
  was still low, then drain the semaphore one at a time and each
  spawn another `spawn_blocking` task — growing the orphan pool
  past `BCA_MAX_ORPHANED_TASKS` and defeating the configured cap.
  The fast-path check is retained as a cheap rejection before the
  semaphore wait, but the post-admission re-check is now the hard
  gate. Counter updates use `Acquire`/`Release` ordering so admitted
  requests observe orphan counts published by any prior orphaning
  task ([#291](https://github.com/dekobon/big-code-analysis/issues/291)).
- In-source suppression markers (`bca: suppress`, `bca: suppress(metric,
  ...)`, and the `#lizard forgives` compat form) now attach to the
  syntactically enclosing function rather than to whichever function's
  line range covered the comment's source line. The previous resolver
  matched on `start_line..=end_line` and picked the first hit by source
  order, which silently attached a marker to the wrong sibling whenever
  two single-line functions shared a row (e.g.
  `int a(){...} int b(){/*bca: suppress*/...}` attached to `a`). The
  walker now applies markers inline against the active state stack at
  the comment node so the topmost `SpaceKind::Function` frame — the
  only function the grammar nested the comment inside — wins. A
  user-visible side effect: a marker on the closing-brace line but
  *outside* the function body (a sibling of `function_definition`, not
  a child of it) no longer attaches; previously the line-range match
  would have caught it
  ([#289](https://github.com/dekobon/big-code-analysis/issues/289)).
- Suppression attachment is now O(stack depth) per marker on the
  iterative walker stack instead of recursing once per nested
  `FuncSpace` on the Rust call stack. The pre-fix
  `attach_function_suppression` helper overflowed the default 8 MiB
  thread stack on inputs with ~1000-deep nested functions; the
  iterative replacement scales to arbitrary nesting
  ([#292](https://github.com/dekobon/big-code-analysis/issues/292)).
- `bca find <NODE>` and `bca count <NODE_TYPE>` now match node kinds
  exactly. Unknown filters that were not a hardcoded keyword
  (`all`/`call`/`comment`/`error`/`string`/`function`) or numeric
  `kind_id` previously fell through to `node.kind().contains(&f)`,
  so a filter like `expression` collapsed `binary_expression`,
  `parenthesized_expression`, `expression_statement`, etc. into one
  bucket — contradicting the CLI documentation, which describes both
  verbs as searching for *a specific node type*
  ([#293](https://github.com/dekobon/big-code-analysis/issues/293)).
- `Cyclomatic` now counts the compound short-circuit assignment
  operators `&&=` and `||=` in JavaScript / TypeScript / TSX /
  Mozjs, matching the existing `??=` handling and the cognitive
  parity from #236. Each compound short-circuit assignment is a
  distinct control-flow decision and must contribute uniformly.
  C# is unaffected (its grammar exposes only `??=`)
  ([#248](https://github.com/dekobon/big-code-analysis/issues/248)).
- `Cognitive` and `Cyclomatic` now count Perl's compound
  short-circuit assignments `&&=`, `||=`, and `//=` as boolean-
  sequence increments / decision edges. The Perl grammar exposes
  these as direct operator tokens inside `binary_expression`,
  unlike the JS family's `augmented_assignment_expression`; the
  predicates that already handle `&&`/`||`/`//` were extended in
  place
  ([#249](https://github.com/dekobon/big-code-analysis/issues/249)).
- `Cognitive` now counts the compound short-circuit assignment
  operators (`&&=`, `||=`, `??=`) in JavaScript / TypeScript /
  TSX / Mozjs and `??=` in C# / PHP. Pre-existing gap: cognitive
  inspected only `BinaryExpression` children, missing the
  `augmented_assignment_expression` container these operators sit
  in. Mirrors the cyclomatic fix from #231
  ([#236](https://github.com/dekobon/big-code-analysis/issues/236)).
- Kotlin's Elvis operator `?:` is now counted as a boolean-sequence
  operator in `Cognitive` (Sonar B1) and as a short-circuit
  decision in `Cyclomatic`, mirroring the JS `??` treatment from
  #226 / #230
  ([#239](https://github.com/dekobon/big-code-analysis/issues/239)).
- Python `Cognitive` ExceptClause now applies the correct nesting
  penalty for `except` clauses nested inside control-flow
  constructs (`if`, `for`, `while`, lambdas). The arm was using
  the stale `stats.nesting` because it bypassed the shared
  `increase_nesting` helper that every other language's
  catch/rescue path uses
  ([#242](https://github.com/dekobon/big-code-analysis/issues/242)).
- `Exit for RustCode` no longer adds a spurious `+1` for every
  Rust function with an explicit return type. The visit of the
  function's own `function_item` node was incrementing
  `stats.exit` inside the function's own state, double-counting
  any function with both an explicit return statement and a
  return type. Aligned with peer-language behaviour: only
  explicit `return` and `?` (TryExpression) count
  ([#243](https://github.com/dekobon/big-code-analysis/issues/243)).
- `mi_sei` now treats `comments_percentage` as a percentage in
  `[0, 100]` as the SEI formula `50·sin(√(2.4·CM))` requires.
  Previously stored as a ratio in `[0, 1]`, the argument to the
  `sqrt` was 100× too small and `MI_SEI` was wildly incorrect for
  any file with comments. The storage site was rescaled (private
  field; no public JSON schema change). All `mi_sei` values for
  files with non-zero comments will shift
  ([#241](https://github.com/dekobon/big-code-analysis/issues/241)).
- **(breaking — CLI internals)** `Violation::path` in
  `big-code-analysis-cli` is now `PathBuf` instead of `String`,
  and `ThresholdSet::evaluate` takes `&Path` instead of `&str`.
  The threshold pipeline previously dropped non-UTF-8 path bytes
  via `Path::to_str()` with a skip-and-warn fallback, so non-UTF-8
  source files could not surface in offender output at all. The
  bytes now round-trip through `Violation` and
  `violation_to_offender` end-to-end (lossy only at the
  human-facing `Display` boundary, via `Path::display()`)
  ([#240](https://github.com/dekobon/big-code-analysis/issues/240)).
- Dead `!matches!(list_kind, ArgumentList | …)` post-conditions
  in `java_count_unary_conditions` / `csharp_count_unary_conditions`
  removed. The preceding `matches!(list_kind, BinaryExpression)`
  already pinned `list_kind` to a single variant; the negated
  match was unreachable. Pure code-quality cleanup
  ([#245](https://github.com/dekobon/big-code-analysis/issues/245)).
- `Cognitive` now counts the nullish-coalescing operator `??` as a
  boolean-sequence operator (Sonar B1) in JavaScript, TypeScript,
  TSX, Mozjs, C#, and PHP. The `compute_booleans` two-operator helper
  is replaced at these call sites by the slice-friendly
  `compute_booleans_with`, mirroring Ruby / Perl / Elixir. Kotlin
  keeps the `&&` / `||` pair (no `??`). Closes the parity gap left by
  #226 on the cyclomatic side
  ([#230](https://github.com/dekobon/big-code-analysis/issues/230)).
- LOC `_min` getters (`sloc_min`, `ploc_min`, `lloc_min`, `cloc_min`,
  `blank_min`) now collapse the `usize::MAX` sentinel to `0.0`
  instead of leaking `1.8446744e19` from a raw `Stats::default()`
  that bypasses the metric pipeline. Mirrors the guard pattern
  already documented on `tokens::Stats::tokens_min` and applied to
  six other metrics in #227
  ([#233](https://github.com/dekobon/big-code-analysis/issues/233)).
- `NExit` now counts `yield` as an exit edge in Python, JavaScript,
  TypeScript, TSX, and Mozjs, matching the long-standing C# / PHP
  behaviour. Generator suspension hands control back to the caller —
  the function does leave its frame, just resumably — so it belongs
  alongside `return` / `throw` / `raise` in the exit-point count.
  Follow-up to #228, which closed the throw/raise parity gap and
  scoped `yield` out as a separate design call
  ([#232](https://github.com/dekobon/big-code-analysis/issues/232)).
- Python cyclomatic complexity no longer over-counts plain `if/else` by
  one. Root cause: the `has_ancestors` helper in `src/node.rs` did not
  actually verify both predicates against the expected ancestor chain;
  it returned true whenever the immediate parent matched the second
  predicate. The helper has been renamed to `parent_grandparent_match`
  and now strictly checks both. Python `try/except/else` is now
  counted alongside `for/else` and `while/else`
  ([#229](https://github.com/dekobon/big-code-analysis/issues/229)).
- Cyclomatic complexity now counts the nullish coalescing operator
  (`??`, token `QMARKQMARK`) as a short-circuit decision in
  JavaScript, TypeScript, TSX, and Mozjs, matching the existing C#
  and PHP treatment. `a ?? b` adds one decision edge to the CFG (does
  not evaluate `b` if `a` is non-null). The `impl_cyclomatic_c_family!`
  macro now takes the short-circuit operator list as a parameter so
  per-language differences (C++ has no `??`) stay explicit
  ([#226](https://github.com/dekobon/big-code-analysis/issues/226)).
- Cyclomatic complexity now counts the compound nullish-coalescing
  assignment operator (`??=`, token `QMARKQMARKEQ`) as a short-circuit
  decision in JavaScript, TypeScript, TSX, Mozjs, C#, and PHP. `a ??= b`
  is semantically `a = a ?? b` — it evaluates and assigns `b` only when
  `a` is null/undefined, the same one-decision-edge contribution as
  `??`. Sibling assignment forms `&&=` and `||=` remain uncounted and
  are tracked as a follow-up
  ([#231](https://github.com/dekobon/big-code-analysis/issues/231)).
- Cognitive complexity now counts the ternary `?:` operator with
  `+1 + nesting` for Java, C#, and PHP, matching `cyclomatic.rs`, the
  C++ fix from #172, and SonarSource Cognitive Complexity §2. Adds
  `TernaryExpression` (Java) and `ConditionalExpression` (C#, PHP) to
  each language's `increase_nesting` arm
  ([#224](https://github.com/dekobon/big-code-analysis/issues/224)).
- Cognitive complexity now counts labeled `break`/`continue` for
  Java and all forms of `goto` (`label`, `case`, `default`) for C#,
  mirroring the Rust/Go/C++/Perl/Lua handling per SonarSource
  Cognitive Complexity §B2. C#'s grammar does not allow labeled
  `break`/`continue` so only `goto_statement` is added there
  ([#225](https://github.com/dekobon/big-code-analysis/issues/225)).
- `throw`/`raise` now contribute to `NExit` in Python, JavaScript,
  TypeScript, TSX, Mozjs, Java, and C++, aligning with the existing
  C#/Kotlin/PHP/Elixir behaviour. `throw`/`raise` is a function exit
  by definition — control leaves the function and the stack unwinds.
  Fixtures containing throws see their `nexits` sum/min/max/average
  increase accordingly; no other metrics or structural fields change
  ([#228](https://github.com/dekobon/big-code-analysis/issues/228)).
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
