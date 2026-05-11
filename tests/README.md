# Test suite map

This directory holds the integration-test layer of the workspace —
everything that runs out-of-process from the parser / metric code in
`src/`. Per-metric unit tests live next to their implementation
(`src/metrics/<metric>.rs#[cfg(test)]`); this file documents only what
sits under `tests/`.

If you are trying to figure out *where* an existing test lives or *how*
to add one, start here.

## Layout at a glance

```text
tests/
├── README.md                              (this file)
├── common/                                shared harness used by every integration test
│   ├── mod.rs                             snapshot driver + per-corpus comparators
│   ├── fixtures.rs                        small constructors for OffenderRecord / FuncSpace
│   └── validators.rs                      SARIF + Checkstyle structural validators
├── fixtures/                              vendored external schemas (SARIF, Checkstyle)
│   └── README.md                          refresh procedure + provenance
├── repositories/                          integration corpora (4 git submodules: 3 upstream projects + 1 fixture+snapshot store)
│   ├── DeepSpeech/                        submodule: mozilla/DeepSpeech                 (C++ corpus)
│   ├── pdf.js/                            submodule: mozilla/pdf.js                     (JS corpus)
│   ├── serde/                             submodule: serde-rs/serde                     (Rust corpus)
│   └── big-code-analysis-output/          submodule: dekobon/big-code-analysis-output
│       ├── csharp/                        hand-written synthetic .cs fixtures           (C# corpus)
│       ├── php/                           hand-written synthetic .php fixtures          (PHP corpus)
│       └── snapshots/                     accepted YAML snapshots for ALL five corpora
├── checkstyle_test.rs                     output-format test: Checkstyle XML schema
├── csv_test.rs                            output-format test: CSV writer
├── sarif_test.rs                          output-format test: SARIF JSON schema
├── serde_test.rs                          corpus test: Rust  / serde
├── deepspeech_test.rs                     corpus test: C++   / DeepSpeech
├── pdf_js_test.rs                         corpus test: JS    / pdf.js
├── csharp_test.rs                         corpus test: C#    / synthetic fixtures
├── php_test.rs                            corpus test: PHP   / synthetic fixtures
└── cyclomatic_cross_language_parity.rs    cross-language: 6 languages × 4 control shapes
```

## Test categories

### 1. Per-metric unit tests (`src/metrics/<metric>.rs`)

Not in this directory, but the bulk of test coverage. Each metric
module ends in a `#[cfg(test)] mod tests` block with `<lang>_*` test
functions exercising the metric against a parser. The function-name
prefix follows the parser, not the file extension — so `c_*` tests in
`cognitive.rs` use `CppParser` (which serves both `.c` and `.cpp`).
Some metric modules use `cpp_*` for the same `CppParser`; the two
prefixes are interchangeable and may both appear in one file (e.g.
`loc.rs` has 3 `c_*` and 10 `cpp_*` tests).

Per-language coverage is uneven. See [Coverage matrix](#coverage-matrix)
below; the canonical reference language for each parser family is:

- OOP languages → `java_*` (most complete metric set)
- Curly-brace scripting → `javascript_*` or `csharp_*`
- Shell / dynamic → `python_*` or `bash_*`

### 2. Output-format tests (`tests/<format>_test.rs`)

Single-purpose: assert that the writers in `src/output/` emit
schema-conformant documents.

| File | What it validates |
|---|---|
| `sarif_test.rs` | Every emitted SARIF doc parses against the vendored Draft-07 schema in `tests/fixtures/sarif-2.1.0.json` |
| `checkstyle_test.rs` | Every emitted Checkstyle XML doc passes a `quick-xml` structural walker mirroring `checkstyle-report-1.0.0.xsd` (no pure-Rust XSD validator exists; see `tests/fixtures/README.md`) |
| `csv_test.rs` | CSV writer round-trip + header stability |
| `serde_test.rs` | (despite the name) **not** an output test — it is the Rust corpus test, named after the `serde-rs/serde` upstream project. See corpus tests below. |

### 3. Corpus tests (`tests/<repo>_test.rs`)

Each runs the parser → metric pipeline over every file in one
`tests/repositories/<repo>/` corpus, then diffs the result against
accepted YAML snapshots in
`tests/repositories/big-code-analysis-output/snapshots/<repo>/`.

All five share the same driver: `tests/common/mod.rs::act_on_file`,
called concurrently via `ConcurrentRunner` (4 jobs) and dispatched by
one of:

- `compare_rca_output_with_files(repo, include, exclude)` — corpus is
  a sibling of `big-code-analysis-output/` under `tests/repositories/`
- `compare_rca_output_with_files_under(source_root, repo, …)` — corpus
  lives *inside* the `big-code-analysis-output` submodule (needed for
  C# and PHP so snapshot paths don't pick up the submodule directory
  as an extra component)

Floats are rounded to 3 decimal places before comparison (machine
portability) and the `name` field is redacted to `[filepath]` (path
portability).

### 4. Cross-language parity (`cyclomatic_cross_language_parity.rs`)

The only test that pins behaviour *across* languages. Asserts that
four control-flow shapes (`switch_with_default`, `switch_without_default`,
`if_else_if_else_chain`, `single_if_no_else`) produce the same
cyclomatic-sum delta in Bash, C++, Java, JavaScript, Python, and Rust.
A bug fixed in one language module that drifts another silently is
exactly what this catches.

## The corpus divergence

The five integration corpora split into two structurally different
patterns. They are complementary, not interchangeable — and no
language currently has both.

### Pattern A: real upstream project, full-scale regression

Submodule pinned to a tagged release. Run the metric pipeline over
every file matching the include glob; accept exclusion FIXMEs for
files the current `tree-sitter-*` grammar mis-parses.

| Language | Submodule | Pinned at | Source files (post-exclude) | Snapshots | Grammar-bug excludes |
|---|---|---|---|---|---|
| C++ (DeepSpeech) | `mozilla/DeepSpeech` | `v0.10.0-alpha.3-137-gaa1d2853` | 869 (`*.cc`/`*.cpp`/`*.h`/`*.hh`) | 1047 | 7 files (→ [#83](https://github.com/dekobon/big-code-analysis/issues/83), tracked in [#86](https://github.com/dekobon/big-code-analysis/issues/86)) plus `tensorflow/**` and `kenlm/**` (vendored, ~8500+ files, no snapshot coverage) |
| JavaScript (pdf.js) | `mozilla/pdf.js` | `65c4a4b3f` | 384 (`*.js`) | 384 | **118** files (→ [#84](https://github.com/dekobon/big-code-analysis/issues/84)) — `tests/pdf_js_test.rs` is 143 lines, almost entirely this exclude list |
| Rust (serde) | `serde-rs/serde` | `v1.0.159` | 172 (`*.rs`) | 172 | none — the only clean Pattern-A corpus |

### Pattern B: synthetic curated fixtures

Hand-written `.cs` / `.php` files inside the `big-code-analysis-output`
submodule. Each file is a topic study of one language-feature surface;
the `Acme.Synthetic` / `namespace Acme\Synthetic` marker makes the
hand-rolled origin obvious.

| Language | Files | Lines per file | Snapshots |
|---|---|---|---|
| C# | `anonymous.cs`, `classes.cs`, `control_flow.cs`, `generics.cs`, `linq.cs`, `strings.cs` | 79–100 | 6 |
| PHP | `anonymous.php`, `classes.php`, `control_flow.php`, `embedded.php`, `strings.php`, `traits_enums.php` | 57–93 | 6 |

### When to use which

- **Pattern A** answers *"does this still parse and score sensibly on
  real production code?"* It scales — a metric-rule change shifts
  hundreds-to-thousands of `.snap` files. Maintenance cost tracks
  upstream-grammar quality, not our code.
- **Pattern B** answers *"does each named language feature compute the
  expected metrics?"* Review cost for a metric change is trivial (6
  files). It is the *only* place OOP metrics (`abc`, `npa`, `npm`,
  `wmc`) are exercised against multi-file or whole-project corpora;
  per-metric unit tests in `src/metrics/<metric>.rs` cover the same
  metrics at the single-snippet level.

## Coverage matrix

Per-metric unit-test counts across `src/metrics/*.rs`. Numbers combine
`c_*` + `cpp_*` for the C/C++ parser (both use `CppParser`) and
`javascript_*` + `js_*` for JS.

| Metric | bash | c/cpp | csharp | go | java | js | kotlin | lua | mozjs | perl | php | python | rust | tcl | tsx | ts |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| abc           |  4 | –  | 19 | – | 18 | – | 18 | –  | –  | –  | 14 | –  | –  | –  |13 |13 |
| cognitive     |  9 | 10 | 12 | 10| 12 | 7 | 12 | 11 | 10 | 12 |  9 | 17 | 11 | 15 | 8 | 7 |
| cyclomatic    |  7 |  7 |  6 | 11|  4 | 3 |  5 |  3 |  5 |  7 |  4 |  2 |  9 |  5 | 7 | 6 |
| exit          |  5 |  1 |  4 |  5|  3 | 3 |  3 |  2 |  3 |  3 |  3 |  4 |  2 |  3 | 3 | 4 |
| halstead      |  1 |  1 |  2 |  1|  2 | 1 |  1 |  1 |  1 |  1 |  2 |  4 |  2 |  3 | 1 | 1 |
| loc           | 11 | 13 | 20 | 11| 20 | 4 | 12 | 16 | 10 | 18 | 23 | 12 | 16 | 22 |15 |15 |
| mi            | –  | –  | –  | – | –  | – | –  | –  | –  | –  | –  | –  | –  | –  | – | – |
| nargs         | –  |  5 |  5 |  6|  5 | 9 |  7 |  6 |  6 |  5 |  5 |  5 |  5 |  7 | 5 | 5 |
| nom           |  3 |  2 |  2 |  4|  2 |12 |  3 |  1 |  6 |  1 |  2 |  1 |  1 |  2 | 4 | 4 |
| npa (OOP)     | –  | –  | 14 | – | 12 | – | 16 | –  | –  | –  | 15 | –  | –  | –  |12 |13 |
| npm (OOP)     | –  | –  | 13 | – | 12 | – | 18 | –  | –  | –  | 12 | –  | –  | –  |12 |12 |
| tokens        | –  |  1 | –  | – |  1 | – | –  | –  | –  | –  | –  |  5 |  1 | –  | – | – |
| wmc (OOP)     | –  | –  | 13 | – | 13 | – | 18 | –  | –  | –  | 13 | –  | –  | –  |12 |12 |
| **Total**     | 40 | 40 |110 | 48|104 |39 |113 | 40 | 41 | 47 |102 | 50 | 47 | 57 |92 |92 |
| **Metrics ≥1 (/13)** | 7 | 8 | 11 |  7| 12 | 7 | 11 |  7 |  7 |  7 | 11 |  8 |  8 |  7 |11 |11 |

Plus module-level tests not counted above:

- `src/checker.rs` — 4 `bash_*` tests
- `src/spaces.rs` — 1 `c_*`, 1 `cpp_*` test
- `src/alterator.rs` — 1 each for `javascript`, `typescript`, `tsx`

Best-covered: **C#** (110 unit tests, 11 of 13 metrics, plus Pattern-B
corpus). Close runner-up: **Java** (104, 12 of 13, plus cross-language
parity). Third tier: **PHP** (102, 11 of 13, plus Pattern-B corpus).
`mi` is a derived metric (composed from `halstead` + `cyclomatic` +
`loc`) and has no per-language unit tests in any language; the two
tests in `src/metrics/mi.rs` exercise the composition logic only.

## Known gaps and tracking issues

- [#167](https://github.com/dekobon/big-code-analysis/issues/167) —
  C/C++ cognitive coverage thin (10 tests vs 12–17 for peers); missing
  ternary, try/catch, lambdas, recursion, switch fall-through.
- [#170](https://github.com/dekobon/big-code-analysis/issues/170) —
  C/C++ thin across `cyclomatic`, `exit`, `halstead`, `nargs`, `nom`,
  `tokens`.

Additionally: no language currently has *both* Pattern A and Pattern B
coverage. Converging them would mean adding a real-repo submodule for
C#/PHP/Kotlin/TS/Go/Python/etc. and adding synthetic fixtures for
C++/JS/Rust.

## Adding a test

### A per-metric unit test

Add to `src/metrics/<metric>.rs#[cfg(test)] mod tests`. Function name
is `<parser_prefix>_<descriptive_case>`. Use `check_metrics::<XParser>`.

Every `insta::assert_json_snapshot!` call must be **anchored** — see
`AGENTS.md` "snapshot-anchor policy". Either inline the expected block,
add a positive `assert_eq!` on an integer accessor immediately above,
or include a `// expected: …` derivation comment. `make pre-commit`
runs `./check-snapshot-anchors.py` against
`.snapshot-anchor-baseline.txt` and fails on any increase.

### A new synthetic fixture (Pattern B)

1. Open a PR against
   [`dekobon/big-code-analysis-output`](https://github.com/dekobon/big-code-analysis-output)
   adding the source file(s) under the appropriate language directory.
2. Run `cargo test --workspace --all-features` here; it will generate
   `.snap.new` files under the submodule.
3. Review with `cargo insta test --review`, accept, commit and push
   the `.snap` files to the submodule's `main` branch.
4. In the parent repo, bump the submodule pointer
   (`git add tests/repositories/big-code-analysis-output`) in the
   **same parent commit** as any matching code change. See
   `AGENTS.md` for the full four-step submodule discipline; lesson 8
   in `docs/development/lessons_learned.md` for the rebase pitfall.

### A new real-repo corpus (Pattern A)

Heavier — adds a git submodule and bumps the workspace's network /
disk-fetch footprint on first clone. Coordinate with maintainers
before proposing one; mirror the structure of `serde_test.rs` (the
simplest existing example) and add a `snapshots/<repo>/` directory
under `big-code-analysis-output`.

### A new output-format test

Mirror `tests/sarif_test.rs`: vendor the schema under
`tests/fixtures/`, document provenance in `tests/fixtures/README.md`,
and validate every emitted document via `tests/common/validators.rs`.
Keep the validator hermetic — no network access.
