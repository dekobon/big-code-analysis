# Test suite map

This directory holds the integration-test layer of the workspace:
everything that runs out-of-process from the parser / metric code in
`src/`. Per-metric unit tests live next to their implementation
(`src/metrics/<metric>.rs#[cfg(test)]`, and for the metrics split into
per-language modules, `src/metrics/<metric>/<lang>.rs`); this file
documents only what sits under `tests/`.

Start here to find *where* an existing test lives or *how* to add one.

## Layout at a glance

Since [#1124](https://github.com/dekobon/big-code-analysis/issues/1124)
the test files are grouped into directory targets rather than one crate
root per file: each `tests/<group>/main.rs` is a single test binary that
declares the former `tests/*.rs` files as `mod`s.

```text
tests/
├── README.md                              (this file)
├── common/                                shared harness used by every integration test
│   ├── mod.rs                             snapshot driver + per-corpus comparators
│   ├── fixtures.rs                        small constructors for OffenderRecord
│   ├── validators.rs                      SARIF + Checkstyle structural validators
│   └── vcs_fixture.rs                     in-process git repositories for the VCS tests
├── fixtures/                              vendored external schemas (SARIF, Checkstyle)
│   └── README.md                          refresh procedure + provenance
├── repositories/                          integration corpora (4 git submodules: 3 upstream projects + 1 fixture+snapshot store)
│   ├── DeepSpeech/                        submodule: mozilla/DeepSpeech                 (C++ corpus)
│   ├── pdf.js/                            submodule: mozilla/pdf.js                     (JS corpus)
│   ├── serde/                             submodule: serde-rs/serde                     (Rust corpus)
│   └── big-code-analysis-output/          submodule: dekobon/big-code-analysis-output
│       ├── csharp/                        hand-written synthetic .cs fixtures           (C# corpus)
│       ├── php/                           hand-written synthetic .php fixtures          (PHP corpus)
│       └── snapshots/                     accepted YAML snapshots for every corpus
├── api/                                   public API: AST seam, parser reuse, derives, book examples, suppression
├── corpus/                                corpus tests: serde, DeepSpeech, pdf.js, C#, PHP, iRules, orphan-snapshot guard
├── grammars/                              grammar-specific: C, mozcpp, alterator string flattening
├── output_formats/                        output-format tests: CSV, SARIF, Checkstyle
│   └── snapshots/                         accepted insta snapshots owned by this target
├── parity/                                cross-language and cross-parser parity suites
└── vcs/                                   VCS metrics: rank, trend, cache, bus factor, per-function
```

## Test categories

### 1. Per-metric unit tests (`src/metrics/<metric>.rs`)

Not in this directory, but the bulk of test coverage. Each metric
module ends in a `#[cfg(test)] mod tests` block with `<lang>_*` test
functions exercising the metric against a parser. The function-name
prefix follows the parser: `c_*` tests use `CParser` (`LANG::C`, which
owns `.c` since #721) and `cpp_*` tests use `CppParser` (`.cpp`, `.cc`,
`.h`, `.hh`). The two are distinct grammars, though a few older
`c_*` tests run their fixture through both parsers and carry a
`cfg(all(feature = "c", feature = "cpp"))` gate to say so.

Per-language coverage is uneven. See [Coverage matrix](#coverage-matrix)
below; the canonical reference language for each parser family is:

- OOP languages: `java_*` (most complete metric set)
- Curly-brace scripting: `javascript_*` or `csharp_*`
- Shell / dynamic: `python_*` or `bash_*`

### 2. Output-format tests (`tests/output_formats/`)

Single-purpose: assert that the writers in `src/output/` emit
schema-conformant documents.

| File | What it validates |
|---|---|
| `sarif_test.rs` | Every emitted [SARIF](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html) doc parses against the vendored [Draft-07](https://json-schema.org/draft-07/) schema in `tests/fixtures/sarif-2.1.0.json` |
| `checkstyle_test.rs` | Every emitted [Checkstyle](https://checkstyle.org/) XML doc passes a [`quick-xml`](https://crates.io/crates/quick-xml) structural walker mirroring `checkstyle-report-1.0.0.xsd` (no pure-Rust XSD validator exists; see `tests/fixtures/README.md`) |
| `csv_test.rs` | CSV writer round-trip + header stability |

(`tests/corpus/serde_test.rs` is, despite the name, **not** a
serialization test: it is the Rust corpus test, named after the
`serde-rs/serde` upstream project. See corpus tests below.)

### 3. Corpus tests (`tests/corpus/`)

Each runs the parse-then-metric pipeline over every file in one
`tests/repositories/<repo>/` corpus, then diffs the result against
accepted YAML snapshots in
`tests/repositories/big-code-analysis-output/snapshots/<repo>/`.

The corpus tests share the same driver: `tests/common/mod.rs::act_on_file`,
called concurrently via `ConcurrentRunner` with one consumer thread per
core (sized from `available_parallelism()`, #1123), and dispatched by
one of:

- `compare_rca_output_with_files(repo, include, exclude, expected_files)`:
  the corpus is a sibling of `big-code-analysis-output/` under
  `tests/repositories/`
- `compare_rca_output_with_files_under(source_root, repo, …, expected_files)`:
  the corpus lives *inside* the `big-code-analysis-output` submodule
  (needed for C# and PHP so snapshot paths don't pick up the submodule
  directory as an extra component)

`expected_files` is the exact number of files the corpus must resolve
to, asserted twice: once against the resolved path list and once
against the number of snapshot assertions actually executed. The two
differ when a file resolves but `act_on_file` returns early on it, so
both are needed to make a silent coverage loss loud. **After a
deliberate corpus bump, update this count alongside the snapshots**;
a legitimate corpus growth is expected to fail these assertions first.

The harness also asserts the inverse direction: every on-disk `.snap`
under `snapshots/<repo>/` must correspond to a resolved corpus file.
[insta](https://insta.rs/) is loud about a *missing* snapshot, but a
stale extra one is
asserted by nothing and silently pins values no test verifies. That is
how 118 pdf.js and 5 DeepSpeech snapshots came to pin grammar-era-old
output (#1282). Excluding or removing a corpus file therefore requires
deleting its snapshot in the same change.

Floats are rounded to 3 decimal places before comparison (machine
portability) and the `name` field is redacted to `[filepath]` (path
portability).

### 4. Cross-language parity (`tests/parity/`)

Where behaviour is pinned *across* languages and parsers rather than
within one. `cyclomatic_cross_language_parity.rs` is representative: it
asserts that each control-flow family (`switch_with_default`,
`switch_without_default`, `if_else_if_else_chain`, `single_if_no_else`,
`two_arm_switch_with_wildcard`, `do_while_loop`, `range_for_loop`,
`safe_navigation_chain`) produces the same cyclomatic sum in every
enabled language that has a fixture row for it. A bug fixed in one
language module that drifts another silently is exactly what these
catch.

## The corpus divergence

The integration corpora split into two structurally different
patterns. They are complementary, not interchangeable, and no
language currently has both.

### Pattern A: real upstream project, full-scale regression

Submodule pinned to a tagged release. Run the metric pipeline over
every file matching the include glob; accept exclusion FIXMEs for
files the current `tree-sitter-*` grammar mis-parses. The file and
snapshot counts below are each pinned by the corpus test's
`expected_files` assertion, described under
[Corpus tests](#3-corpus-tests-testscorpus).

| Language | Submodule | Pinned at | Source files (post-exclude) | Snapshots | Grammar-bug excludes |
|---|---|---|---|---|---|
| C++ (DeepSpeech) | `mozilla/DeepSpeech` | `v0.10.0-alpha.3-137-gaa1d2853` | 1042 (`*.cc`/`*.cpp`/`*.h`/`*.hh`) | 1042 | 5 files (see [#83](https://github.com/dekobon/big-code-analysis/issues/83), tracked in [#86](https://github.com/dekobon/big-code-analysis/issues/86)) plus `tensorflow/**` and `kenlm/**` (vendored, ~8500+ files, no snapshot coverage) |
| JavaScript (pdf.js) | `mozilla/pdf.js` | `65c4a4b3f` | 384 (`*.js`) | 384 | none; the mozjs-era 118-file exclude list was retired by [#1282](https://github.com/dekobon/big-code-analysis/issues/1282); every file parses cleanly under the post-[#507](https://github.com/dekobon/big-code-analysis/issues/507) upstream grammar |
| Rust (serde) | `serde-rs/serde` | `v1.0.159` | 172 (`*.rs`) | 172 | none |

### Pattern B: synthetic curated fixtures

Hand-written `.cs` / `.php` files inside the `big-code-analysis-output`
submodule. Each file is a topic study of one language-feature surface;
the `Acme.Synthetic` / `namespace Acme\Synthetic` marker makes the
hand-rolled origin obvious.

| Language | Files | Lines per file | Snapshots |
|---|---|---|---|
| C# | `anonymous.cs`, `classes.cs`, `control_flow.cs`, `generics.cs`, `linq.cs`, `parameters.cs`, `strings.cs` | 65–100 | 7 |
| PHP | `anonymous.php`, `classes.php`, `control_flow.php`, `embedded.php`, `strings.php`, `traits_enums.php` | 57–93 | 6 |

### When to use which

- **Pattern A** answers *"does this still parse and score sensibly on
  real production code?"* It scales: a metric-rule change shifts
  hundreds-to-thousands of `.snap` files. Maintenance cost tracks
  upstream-grammar quality, not our code.
- **Pattern B** answers *"does each named language feature compute the
  expected metrics?"* Review cost for a metric change is small (a
  handful of files per language). It is the *only* place OOP metrics
  (`abc`, `npa`, `npm`, `wmc`) are exercised against multi-file or
  whole-project corpora; per-metric unit tests in
  `src/metrics/<metric>.rs` cover the same metrics at the
  single-snippet level.

## Coverage matrix

Which languages each metric has at least one `<lang>_*` unit test for,
under `src/metrics/`. Presence only, not counts: the counts move with
every test added and are cheap to re-derive (`rg -c '#\[test\]'
src/metrics/<metric>*`). `c` is `CParser` and `cpp` is `CppParser`;
`js` combines the `javascript_*` and `js_*` prefixes, `ts` the
`typescript_*` and `ts_*` ones.

| Metric | bash | c | cpp | cs | ex | go | groovy | irules | java | js | kt | lua | mozcpp | mozjs | objc | perl | php | py | rb | rs | tcl | tsx | ts |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| abc | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x |
| cognitive | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x |
| cyclomatic | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x |
| nexits | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x |
| halstead | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x | x |
| loc | x | x | x | x | x | x | x | x | x | x | x | x | – | x | x | x | x | x | x | x | x | x | x |
| mi | – | – | x | – | x | x | – | – | – | x | – | – | – | x | – | – | – | x | – | x | – | – | – |
| nargs | – | x | x | x | x | x | x | x | x | x | x | x | – | x | x | x | x | x | x | x | x | x | x |
| nom | x | x | x | x | x | x | x | x | x | x | x | x | – | x | x | x | x | x | x | x | x | x | x |
| npa (OOP) | – | – | x | x | x | x | x | – | x | x | x | – | – | x | x | – | x | x | x | x | – | x | x |
| npm (OOP) | – | – | x | x | x | x | x | – | x | x | x | – | x | x | x | – | x | x | x | x | – | x | x |
| tokens | – | – | x | – | – | – | x | – | x | x | – | – | – | – | – | x | – | x | – | x | – | – | – |
| wmc (OOP) | – | – | x | x | x | x | x | – | x | x | x | – | x | x | x | – | x | x | x | x | – | x | x |

Plus module-level tests not counted above: `bash_*` tests in
`big-code-analysis-ast/src/checker.rs`, `cpp_*` tests in
`src/spaces_tests.rs`, and `javascript` / `typescript` / `tsx` string
flattening tests in `big-code-analysis-ast/src/alterator.rs`.

The OOP metrics (`npa`, `npm`, `wmc`) have no per-language tests for
Bash, C, iRules, Lua, Perl or Tcl, and `nargs` has none for Bash. `mi`
is a derived metric (composed from `halstead` + `cyclomatic` + `loc`):
most tests in `src/metrics/mi.rs` exercise the composition logic and
the doc-comment charging modes, with a per-language smoke test for a
few grammars.

## Known gaps and tracking issues

The C/C++ coverage gaps once tracked in
[#167](https://github.com/dekobon/big-code-analysis/issues/167) and
[#170](https://github.com/dekobon/big-code-analysis/issues/170) are
closed; the matrix above is the current picture.

No language currently has *both* Pattern A and Pattern B
coverage. Converging them would mean adding a real-repo submodule for
the Pattern B and unit-test-only languages (C#, PHP, Kotlin, TypeScript,
Go, Python, and the rest) and adding synthetic fixtures for C++, JS and
Rust.

## Adding a test

### A per-metric unit test

Add to `src/metrics/<metric>.rs#[cfg(test)] mod tests`. Function name
is `<parser_prefix>_<descriptive_case>`. Use `check_metrics::<XParser>`,
the module-local shim each `mod tests` declares with
`check_metrics_only_shim!`, which computes that module's metric plus
the dependencies `Metric::dependencies` resolves, not all thirteen
families ([#1127](https://github.com/dekobon/big-code-analysis/issues/1127)).

If the assertion also reads a *different* metric family, do not widen
that shim: declare a second one next to it
(`check_metrics_only_shim!(check_nom_and_npm, Nom, Npm);`) and call it
from those tests alone. Asserting a deselected family is the failure
mode to watch for: it reads as zero, so a comparison like
`tokens_sum() > halstead_total` keeps passing while testing nothing.

Every `insta::assert_json_snapshot!` call must be **anchored**; see
`AGENTS.md` "Validation gates". Either inline the expected block,
add a positive `assert_eq!` on an integer accessor immediately above,
or include a `// expected: …` derivation comment. `make pre-commit`
runs `./utils/check-snapshot-anchors.py` against
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

Heavier: it adds a git submodule and bumps the workspace's network /
disk-fetch footprint on first clone. Coordinate with maintainers
before proposing one; mirror the structure of `serde_test.rs` (the
simplest existing example) and add a `snapshots/<repo>/` directory
under `big-code-analysis-output`.

### A new output-format test

Mirror `tests/output_formats/sarif_test.rs`: vendor the schema under
`tests/fixtures/`, document provenance in `tests/fixtures/README.md`,
and validate every emitted document via `tests/common/validators.rs`.
Keep the validator hermetic: no network access.

## Moving a test file

Moving a file that owns `insta` snapshots renames every one of them.
`insta` builds the snapshot file name from the **whole** module path of
the asserting file (`::` replaced by `__`) followed by the snapshot
name. It is not the last component. A test in
`tests/output_formats/csv_test.rs` writes
`output_formats__csv_test__<name>.snap`; one in `src/metrics/abc.rs`'s
`mod tests` writes `big_code_analysis__metrics__abc__tests__<name>.snap`.
The `snapshots/` directory moves as well, since insta resolves it
relative to the asserting file.

The resulting failure does not mention the move. insta reports
`snapshot assertion for '<name>' failed`, prints the entire value as
new, and writes a `.snap.new` under the new name, the same output a
genuine behaviour change produces. The name in the source is correct and
the reviewed values are still on disk under the old name, so nothing
points at the rename. #1124 hit this merging the root suite's test
binaries into directory targets: `tests/csv_test.rs` became
`tests/output_formats/csv_test.rs`, and five snapshots had to be renamed,
`tests/snapshots/csv_test__csv_cpp_widget.snap` to
`tests/output_formats/snapshots/output_formats__csv_test__csv_cpp_widget.snap`.

Rename the files; do not re-accept them:

```bash
git mv tests/snapshots/csv_test__csv_cpp_widget.snap \
       tests/output_formats/snapshots/output_formats__csv_test__csv_cpp_widget.snap
```

`cargo insta test --accept` also turns the suite green, but it records
whatever production emits today rather than the value that was reviewed
(the hazard the snapshot-anchor policy exists to prevent), and it leaves
the old file behind as an orphan. The `source:` line in the snapshot
header is metadata and is not compared, so a stale one fails nothing;
`cargo insta test --force-update-snapshots` refreshes it.
