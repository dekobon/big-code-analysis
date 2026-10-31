---
name: add-lang
description: End-to-end workflow for adding a new tree-sitter language to big-code-analysis. Wires up the grammar crate, regenerates the language enum, declares the role table, implements Checker/Getter/Alterator/metrics, adds per-metric tests, updates docs, then runs simplify-rust, rust-optimize, and audit-tests. Does NOT commit — leaves the working tree dirty for final user review.
---

# Add a New Language

Add support for a new tree-sitter-backed language to the workspace. The
language name and grammar crate are provided in `$ARGUMENTS`.

This skill mirrors the historical Go-support work (commits `6ecc582`,
`e0701e1`, `ab857ef`, `ecb6299`, `6ebc1e5`, `15826be`) plus the more
recent Bash (`3a2eaa3`), PHP (`ddae116`), and C# (`08c382c`)
additions. The C# commit is the freshest single-commit
language-addition reference and post-dates ~15 of the lessons in
`docs/development/lessons_learned.md` — read it alongside the Go
commits when looking for a structural template. All of those commits
predate the #1376 split into `big-code-analysis-ast` with one file per
language under `checker/`, `getter/` and `src/metrics/<metric>/`, and
the #1691 role tables; take the *shape* of a per-language impl from a
current sibling (`roles/lua.rs`, `checker/lua.rs`, `getter/go.rs`),
and only the commit history for what a complete addition touched.
The book page `big-code-analysis-book/src/developers/new-language.md`
is the short-form version of this skill and should agree with it; the
upstream Mozilla guide
(<https://mozilla.github.io/rust-code-analysis/developers/new-language.html>)
is older background, adapted to the current API and metric set.

## Arguments

Parse `$ARGUMENTS` as: `<lang-name> <grammar-crate>=<version> [<file-ext>...]`

- `<lang-name>` (required): PascalCase enum variant name, e.g. `Go`,
  `Ruby`, `Swift`. Must not collide with existing variants in
  `big-code-analysis-ast/src/langs.rs` or `enums/src/languages.rs`.
- `<grammar-crate>=<version>` (required): the tree-sitter crate name
  and pinned version, e.g. `tree-sitter-ruby=0.23.1`. The version MUST
  be pinned with `=X.Y.Z` (project convention — see `AGENTS.md`).
  The Rust path is the crate name with hyphens replaced by underscores
  (e.g. crate `tree-sitter-c-sharp` → Rust path `tree_sitter_c_sharp`).
- `<file-ext>...` (optional): file extensions to associate with the
  language, e.g. `rb`. If omitted, infer from the lowercase lang name
  (e.g. `Go` → `go`).

If any required argument is missing, ask the user once before
continuing.

## Constraints

- **No commits.** This skill leaves the working tree dirty. Final
  review and commits are the user's responsibility.
- **No public-API breaks** in unrelated crates. The new language
  variant is itself a public-API addition (acceptable; minor bump);
  do not change other variants or trait signatures.
- **Pin the grammar version** with `=X.Y.Z` in the root `Cargo.toml`
  (`[workspace.dependencies]`) and in `enums/Cargo.toml`; the
  `big-code-analysis-ast` manifest inherits the pin with
  `workspace = true`. Never use a range without explicit user
  approval. `utils/check-excluded-manifests.py` (in `make pre-commit`)
  fails on a caret range in `enums/Cargo.toml`, and the
  `grammar_version_matches_cargo_toml_pin` test in
  `big-code-analysis-ast/src/langs.rs` fails when the `mk_langs!`
  version literal (Step 2c) disagrees with the root pin.
- **Cross-language parity.** All 13 metric trait impls
  (`Abc`, `Cognitive`, `Cyclomatic`, `Exit`, `Halstead`, `Loc`, `Mi`,
  `NArgs`, `Nom`, `Npa`, `Npm`, `Tokens`, `Wmc` — the set
  `src/metric_suite.rs` keys on a parser) must list the new
  `<Lang>Code` in a real impl, an `implement_metric_trait!`
  invocation, or — for Halstead — `impl_halstead_forwarding!`.
- **A role table with zero grandfathered debt.** Every language needs
  a `role_table!` in `big-code-analysis-ast/src/roles/<lang>.rs`
  (Step 3a). The three role gates in `make pre-commit` keep
  removal-only baselines, so a new language cannot add `pending`
  cells, unclaimed kinds, or bypass sites to them.
- **Worktree safety.** If `git rev-parse --show-toplevel` returns a
  path under `.claude/worktrees/`, do not `cd` to the main repo,
  remove worktrees, or check out a different branch. Run all work in
  the current worktree.
- **Tooling.** Prefer Serena symbol-level editing for `.rs` code,
  targeted `Edit` for `.toml`/`.md`. Use `rg`/`fd` (not `grep`/`find`)
  for search.

---

## Step 0: Setup and validation

### 0a: Ensure clean working tree

```bash
git status --porcelain
```

If dirty, abort with the message: "Working tree is not clean. Stash or
commit existing changes before adding a new language." Do not stash on
the user's behalf.

### 0b: Validate name and version pin

- Confirm `<lang-name>` is not already a variant in
  `big-code-analysis-ast/src/langs.rs` (search for `LANG::<lang-name>`).
- Confirm `<grammar-crate>=<version>` parses cleanly and that
  `crates.io/crates/<grammar-crate>` actually publishes
  `<version>` — fetch the crate page or `cargo search` to verify.
- Confirm the crate's `tree-sitter` dependency range is compatible
  with the `tree-sitter` version pinned in the workspace root
  `Cargo.toml`. **Re-read `Cargo.toml` to get the current pin** — do
  not trust any version cached in this skill. Run
  `rg '^tree-sitter ' Cargo.toml` and use whatever it prints. If the
  grammar requires a newer `tree-sitter`, STOP and ask the user —
  bumping the workspace `tree-sitter` is a separate, larger change.

### 0c: Activate Serena

If a Serena MCP server is available, run `serena:activate_project` so
symbol-level navigation/editing is the default for all `.rs` edits.

---

## Step 1: Wire up the `enums` codegen helper

The `enums` crate is excluded from the default workspace and exists
solely to regenerate `big-code-analysis-ast/src/languages/language_<lang>.rs` from a tree-sitter
grammar's node-kind table. Wire it up first so we can produce the enum
file before touching the main crate.

### 1a: Add the grammar crate to `enums/Cargo.toml`

Insert the dep alphabetically among the other `tree-sitter-*` lines:

```toml
tree-sitter-<lang> = "=<version>"
```

### 1b: Register the language in `enums/src/languages.rs`

The `enums` crate's `mk_langs!` is **not** the ten-field tuple macro of
`big-code-analysis-ast/src/langs.rs` (Step 2c). It takes bare variant
identifiers, one per line, and the grammar crate is bound separately
in 1c. The matcher is `( $( $camel:ident ),* )` with no trailing-comma
option (`enums/src/macros.rs`), so the last entry carries **no** comma
and the list currently ends `Groovy // -> …`. Append the variant by
adding a comma to the previous last entry and writing the new one bare,
with a `// -> <crate>` comment only when the backing crate is not the
obvious `tree_sitter_<lang>`:

```rust
    Groovy, // -> dekobon_tree_sitter_groovy
    <LangName> // -> <crate path, if non-obvious>
);
```

A comma after the new last entry fails to compile the `enums` crate.
Then bump `EXPECTED_LANG_VARIANT_COUNT` in `enums/tests/dispatch.rs`
and add a `lang_<lang>_resolves_to_<crate>` test beside the existing
ones — `make enums-check` runs that file and fails on the count.

### 1c: Register the language in `enums/src/macros.rs`

Add a match arm to the `mk_get_language!` macro rule. The project's
current convention is `tree_sitter_<lang>::LANGUAGE.into()` (the
upstream `tree_sitter_<lang>::language()` form is the older API and
should not be used). Most modern grammar crates expose a single
`LANGUAGE` constant. If the crate exports neither — only relevant for
older 0.20-era crates — bump it to a 0.23.x release in the same
change; do not paper over with `cfg`-gated wiring:

```rust
Lang::<LangName> => tree_sitter_<lang>::LANGUAGE.into(),
```

**Variant grammars.** Some crates expose multiple LANGUAGE constants
because they ship more than one dialect. Inspect the grammar crate's
`lib.rs` (or its docs.rs page) before writing the arm. Examples
already in `enums/src/macros.rs`:

```rust
Lang::Typescript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
Lang::Tsx        => tree_sitter_typescript::LANGUAGE_TSX.into(),
```

If the new grammar exposes a single dialect, use `LANGUAGE`. If it
exposes more (e.g. `LANGUAGE_FOO` and `LANGUAGE_BAR`), pick the one
that matches the dialect you are wiring up — and consider whether
**both** dialects deserve separate `Lang::` variants (as Typescript
and Tsx do).

**Dialect identity-collapse trap.** If you do split a grammar into
two `Lang::` variants, `LANG::get_name()` typically collapses the
dialect to the canonical name (`Lang::Tsx::get_name() == "typescript"`).
Any downstream helper that branches on `get_name()` — the HTML
report's `language_palette_slug` is the canonical example
(issue #139, `0a9eca1`) — has dead arms for the collapsed variant
that no string-keyed unit test will exercise. After Step 1, grep for
`get_name()` call sites and either (a) drive their per-language
`match`es through the enum itself, not literal strings, or (b)
confirm the collapsed canonical name is what the helper sees and
delete the dead arm. See lesson 14 in `lessons_learned.md`.

### 1d: Verify the empty-kind fallback is present

`enums/src/common.rs` must include the `Anon{i}` fallback for empty
kind names:

```rust
let mut name = camel_case(&sanitize_identifier(kind));
if name.is_empty() {
    name = format!("Anon{i}");
}
```

This was added during Go support because `tree-sitter-go` emits an
empty kind name at one position. If it is missing, add it.

### 1e: Generate `language_<lang>.rs`

From the repo root, mirroring `recreate-grammars.sh`:

```bash
cargo run --manifest-path ./enums/Cargo.toml -- \
    -l rust -o ./big-code-analysis-ast/src/languages
cargo fmt --all
```

(The `--manifest-path` form avoids `cd`, which keeps the workflow
worktree-safe; `-l rust` and the default `-f language_$` are what the
project uses. `cargo fmt` is mandatory because generated files are not
formatted to project conventions, and skipping it leaves them as a
spurious diff in Step 7.)

**Heads up: this regenerates EVERY language file**, not just the new
one. The `enums` binary iterates `Lang::into_enum_iter()` and writes
one file per registered variant. After running, inspect the diff:

```bash
git status -- big-code-analysis-ast/src/languages/
git diff big-code-analysis-ast/src/languages/
```

The new `language_<lang>.rs` should appear as a new file. Existing
`language_*.rs` files should normally show no diff (or only
formatting-equivalent diffs). If a sibling language file changes
substantively, it means the codegen output drifted — investigate
before continuing; do not commit unintended changes to other
languages. The drift is almost always rooted in
`enums/templates/rust.rs` (e.g. attribute conventions in the template
no longer matching what the workspace clippy gate expects); fix the
template — not the emitted output — and re-run codegen. See lesson
15 in `lessons_learned.md` (its `enums/templates/rust.rs`
sub-example).

Confirm the new file exists at `big-code-analysis-ast/src/languages/language_<lang>.rs` and
that it carries the `// Code generated; DO NOT EDIT.` marker (after the
file-level `#![allow(...)]` lines, around line 8).

If the project also depends on C-macro tables for the new language
(only relevant for C/C++ family preprocessor work), also run:

```bash
cargo run --manifest-path ./enums/Cargo.toml -- \
    -l c_macros -o ./big-code-analysis-ast/src/c_langs_macros
```

Most languages do not need this step.

---

## Step 2: Wire the grammar into the main crate

### 2a: Wire the grammar into two manifests

Since #1376 the grammar crates are dependencies of
`big-code-analysis-ast`, not of the root crate, so a language needs
three edits across two manifests (plus the `enums/Cargo.toml` pin from
1a, which is workspace-excluded and cannot inherit — four edits, of
which the root pin, the `enums` pin and the `mk_langs!` version field
carry the version literal). Missing any of them leaves `LANG::<New>` returning
`LanguageDisabled` in every build, or leaves an unused dependency that
`cargo +nightly udeps` fails on.

1. Root `Cargo.toml`, `[workspace.dependencies]` — the version pin,
   inserted alphabetically among the other `tree-sitter-*` entries
   (same pinned-version line as 1a):

   ```toml
   tree-sitter-<lang> = "=<version>"
   ```

2. `big-code-analysis-ast/Cargo.toml` — the optional dependency and the
   feature that enables it, plus an entry in `all-languages`:

   ```toml
   [dependencies]
   tree-sitter-<lang> = { workspace = true, optional = true }

   [features]
   all-languages = [..., "<lang>", ...]
   <lang> = ["dep:tree-sitter-<lang>"]
   ```

3. Root `Cargo.toml`, `[features]` — the forwarding feature and the
   matching `all-languages` entry, so the root's feature set stays a
   superset of the sub-crate's:

   ```toml
   all-languages = [..., "<lang>", ...]
   <lang> = ["big-code-analysis-ast/<lang>"]
   ```

The feature name is the lowercase language name and must equal the
first field of the Step 2c tuple: `mk_lang!` / `mk_action!` gate every
grammar reference on `#[cfg(feature = "<that string>")]`, and
`utils/check-test-lang-gates.py` derives per-test `cfg` markers from
the same string (Step 5g).

### 2b: Export the generated module from `big-code-analysis-ast/src/languages/mod.rs`

```rust
pub mod language_<lang>;
pub use language_<lang>::*;
```

Insert alphabetically.

### 2c: Add the language definition to `big-code-analysis-ast/src/langs.rs`

Append a `mk_langs!` tuple. The live macro takes **ten** fields, in
the order the comment block at the top of the invocation numbers them
— re-read that block rather than trusting this copy:

```rust
(
    "<lowercase-lang>",            // 1) Cargo feature (Step 2a)
    <LangName>,                    // 2) LANG variant
    "The `<LangName>` language",   // 3) description (rustdoc on the variant)
    "<lowercase-lang>",            // 4) display name
    <LangName>Code,                // 5) per-language tag struct
    <LangName>Parser,              // 6) parser alias
    tree_sitter_<lang>,            // 7) grammar crate (fed to `get_language!`)
    [<ext1>, <ext2>, ...],         // 8) file extensions, no dots
    ["<emacs-mode>"],              // 9) emacs modes
    "<version>"                    // 10) pinned grammar version, no `=`
),
```

Field 10 is the `X.Y.Z` from `<grammar-crate>=<version>`, without the
`=`. `grammar_version_matches_cargo_toml_pin` (a test in the same
file) reads the root `Cargo.toml` and fails on any disagreement — and
its `dep_key` match is exhaustive over `LANG`, so add an arm mapping
the new variant to its `[workspace.dependencies]` key, or the crate
does not compile. The emacs mode is conventionally the lowercase
language name (e.g. `"rust"`, `"go"`, `"ruby"`). Use the file
extensions parsed in Step 0 (without dots).

Field 7 is resolved by `get_language!` in
`big-code-analysis-ast/src/macros/mod.rs`, whose default arm is
`$name::LANGUAGE.into()`. A crate exposing several dialect constants
(`tree_sitter_typescript::LANGUAGE_TSX`, `tree_sitter_php::LANGUAGE_PHP`)
needs a dedicated arm there, mirroring the one you wrote in 1c.

Three hand-maintained lists in the same crate must gain the language
too:

- `with_any_parser!` in `big-code-analysis-ast/src/macros/mod.rs` —
  one `AnyParser::<LangName>($p) => $body,` arm; a non-exhaustive-match
  compile error says so;
- the `tables` list in `python_reader_agrees_with_the_compiled_tables`
  (`big-code-analysis-ast/src/roles/mod.rs`) — its first assertion
  holds the list to the `LANG` enum, so the test fails until Step 3a's
  table is registered there;
- `roundtrip_tests!` in
  `big-code-analysis-ast/src/language_enum_roundtrip.rs` — the
  generated-table drift guard. Nothing fails when this one is
  forgotten; the new enum simply goes unguarded, so add it now.

Outside the AST crate, `language_display_name` in
`big-code-analysis-cli/src/markdown_report.rs` is a wildcard-free
`match LANG` and needs a display arm, and the crate docs in `src/lib.rs`
list every language with its feature name. The `tests/parity/*.rs`
suites each `match lang` over the whole enum to pick a fixture; the
ones without a wildcard arm stop compiling until the new language has
a row (Step 5f).

### 2d: Compile-check

```bash
cargo check -p big-code-analysis
```

Expect missing-impl errors for `Checker`/`Getter`/`Alterator` and the
metric traits — those are the next steps.

---

## Step 3: Implement core AST plumbing

For each impl below, **read the analogous impl for a similar language
first** (Java and Kotlin are usually the closest match for
imperative-with-classes; Rust for everything else) and mirror its
structure. Use Serena `find_symbol` / `find_referencing_symbols`.

**No panics on reachable paths.** AGENTS.md bans `unwrap` / `expect` /
`panic!` / `assert!` in non-test code (it permits `expect("reason")`
and `assert!()` in production for provably-unreachable invariants
when the invariant is documented in the `expect` message). Lesson 5
in `lessons_learned.md` extends the same discipline to
`unreachable!()`: every `match node.kind_id() { ... }` in
`checker/<lang>.rs`, `getter/<lang>.rs`, and the per-language metric
impls must terminate in an `Unknown` / `_` arm — never
`unreachable!()`. The next grammar bump WILL add an aliased kind_id
(Step 3.5 / lesson 2) that flips an `unreachable!()` arm reachable in
production.

**Per-language files, not appended blocks.** Since #1376 each trait
lives in a dispatcher module with one file per language:
`big-code-analysis-ast/src/checker/<lang>.rs`,
`big-code-analysis-ast/src/getter/<lang>.rs`,
`big-code-analysis-ast/src/roles/<lang>.rs`, and under `src/metrics/`
the `abc/`, `cognitive/`, `cyclomatic/`, `loc/`, `npa/` and `npm/`
directories. Create the new file and register it with a `mod <lang>;`
line in the parent (`checker.rs`, `getter.rs`, `roles/mod.rs`,
`metrics/<metric>.rs`); the remaining traits (`Alterator`, `Exit`,
`Halstead`, `Mi`, `NArgs`, `Nom`, `Tokens`, `Wmc`) still take an impl
block in the single file.

**Synthetic `Unit` root.** Tree-sitter can return a non-`Unit` root
(an `ERROR` node, or an inner `struct` / `function` / `namespace`
promoted to root) on partially-parseable input. `src/spaces/compute.rs`
pushes a synthetic `Unit` whenever the parser's root kind is not the
language's canonical `Unit` kind, anchored to the full input range.
Confirm this fallback covers the new language — write a regression
test that parses a deliberately malformed fixture and asserts
`blank ≥ 0` and `kind == Unit` at the file level. See lesson 9 in
`lessons_learned.md` (issue #80, `dc09eb3`).

### 3a: Role table in `big-code-analysis-ast/src/roles/<lang>.rs`

Since #1691 the kind lists the classifiers and metric dispatchers read
are **declared once per language** in a `role_table!`, and the
`Checker` / `Getter` impls of 3b–3c forward to the rows rather than
restating them. Write the table first; the impls then have something
to read. The module doc of `big-code-analysis-ast/src/roles/mod.rs` is
the reference for the row grammar and the fixed role set — read it in
full before writing a row, then read two sibling tables
(`roles/lua.rs` is the smallest kind-only one, `roles/java.rs` a
class-bearing one).

Every role in the set gets exactly one of: declared rows
(`owns [..]`, optionally `pays [..]` / `project` / `wraps` /
`excludes`), `absent "why"`, or `pending Rnn`. Register the file with a
`pub mod <lang>;` line in `roles/mod.rs` and add it to the `tables`
list of `python_reader_agrees_with_the_compiled_tables` in the same
file (Step 2c).

**Three gates read the table, and a new language starts with zero
grandfathered debt.** All three run in `make pre-commit`, each through
the single reader `utils/role_tables.py`, and each keeps a baseline
that `--update` can only shrink (`--bootstrap` refuses a file that
exists or is tracked at `HEAD`). Read the module docstring at the top
of each `utils/check-role-*.py` before the first run:

- `make role-census` (`.role-census-baseline.txt`): every
  (language, role) cell is rows, `absent`, or `pending`. A `pending`
  cell not already in the baseline fails — so for a new language write
  **no** `pending` rows. Where a sibling table says `pending R07` /
  `pending R09` for a role (`Branch`, `Loop`, `ElseIf`, `Exit`, the
  `Comment` / `Doc*` family), the new language must declare the real
  rows or an honest `absent`; the census matrix (`--matrix`) shows how
  the siblings filled the cells that are already declared.
- `make role-coverage` (`.role-coverage-baseline.txt`): every runtime
  kind id in `language_<lang>.rs` — named and anonymous, visible and
  hidden, every numbered alias — is claimed by some row (`owns`,
  `pays`, `wraps`), excluded by a row's `excludes [Kind "why"]`, or
  baselined. A language with no baseline section fails with a message
  telling you to add a `[<Lang> <crate> <version>]` section listing
  every residue entry. `--report` prints only per-language *counts*
  and the `excluded …` lines, not the entries, and the per-entry
  errors fire only once a section exists — so get the entries from a
  scratch bootstrap, which writes every language's residue in baseline
  format, and copy the new language's block across:

  ```bash
  scratch_dir=$(mktemp -d -t role-coverage-scratch.XXXXXX)
  python3 utils/check-role-coverage.py --bootstrap \
    --baseline "$scratch_dir/baseline.txt"
  # copy the `[<Lang> <crate> <version>]` section and its entries from
  # the scratch file into .role-coverage-baseline.txt, then:
  make role-coverage
  ```

  (`--bootstrap` refuses any path that already exists — the tracked
  real file, and equally a file `mktemp` has just created — so the
  scratch path must be a name inside a fresh directory, not a file.)
  That copied block is the one deliberate way to record
  residue; trim it to the kinds no role can honestly claim or exclude
  before committing, and expect a reviewer to ask about each entry.
- `make role-bypass` (`.role-bypass-baseline.txt`): inside the
  consumer boundary — `checker/`, `getter/`, `macros/kind_sets.rs`,
  `c_langs_macros/` and the `abc` / `cyclomatic` / `cognitive` /
  `nexits` / `npm` / `npa` metric modules — a kind a row claims must
  not be restated as classification (`matches!`, a match arm,
  `kind_id() == Lang::Kind`, `kind() == "…"`). Read the row instead
  (`crate::roles::<lang>::<role>::owns` / `lists_kind` /
  `pays_at_leaf_kind`). A site that is not membership — naming,
  traversal, a doc-attachment rule — takes
  `// role-bypass: allow(<Role>) — <reason>` on the line above, naming
  the role whose row claims the kind.

**Corpus observation.** `make role-coverage` runs with
`--require-observations`, so `.role-coverage-observed.txt` must be
current, and it is stale the moment `mk_langs!` has a language the
file has no section for. Run `make role-coverage-observe` (builds
`bca --release` with `--locked` and walks the three corpus submodules
under `tests/repositories/`) after Step 2 compiles; it records the new
language as `not-sampled` when no corpus file carries one of its
extensions — reported in every run, never a failure, and the only
cure is a corpus with files in that language. The record also pins
the grammar version it observed under, so re-run `--observe` after
any version change. The observer refuses a corpus submodule with local
changes; the Makefile target is a bare `--observe`, so to override run
`python3 utils/check-role-coverage.py --observe --allow-dirty` directly.

### 3b: `Checker` impl in `big-code-analysis-ast/src/checker/<lang>.rs`

Create the file, add `mod <lang>;` to `checker.rs`, and write
`impl Checker for <LangName>Code`. Required methods:

- `is_comment` — match the grammar's comment node kind id.
- `is_useful_comment` — usually `false`.
- `is_doc_comment` and the doc-run family (`is_doc_target`,
  `is_doc_run_opener`, `ends_doc_run`, `is_doc_run_directive`,
  `holds_doc_comment_in_body`) — what the language's own
  documentation tool reads (#1597). They feed `dloc`; the defaults
  answer `false`, which is right for a language with no doc
  convention. Where one exists, `lang_helpers/doc_comment.rs` has the
  shared opener tests.
- `is_func_space` / `is_func_space_with_code`, `is_func`,
  `is_closure` — forward to the table: `crate::roles::<lang>::opens_space`
  (emitted once per table as the fold over every `opens K` annotation
  on the `FuncOpener` and `Container` rows) for the space predicates,
  `func_opener::is_callable` for `is_func`, `closure::owns` for
  `is_closure` (#1696). Copy the Lua or Go shape, including the
  byte-less `is_func_space` forwarding with `Ancestors::known(&[])`.
- `is_call`, `is_call_with_code`, `is_string`,
  `is_string_with_code` — one `impl_row_predicates!(<lang>, call, …)`
  and one `impl_row_predicates!(<lang>, string_lit, …)` line each
  (#1698).
- `is_non_arg` — punctuation kinds inside argument lists
  (`LPAREN`, `COMMA`, `RPAREN`, etc.). Not a role; written by kind.
- `is_else_if` — `#[inline]`. **Do not blind-copy the Go
  form.** Before writing the predicate, parse a representative
  `if x { … } else if y { … } else { … }` snippet with the new
  grammar (the `tree-sitter` CLI, or a quick test fixture) and look
  at the AST shape. Lesson 10 in `lessons_learned.md` and
  `.claude/rules/grammar-dispatch.md` §4 catalogue the strategies;
  three have a macro in `checker.rs`. **Re-derive the membership
  rather than trusting this table** — it moves with every language:

  ```bash
  rg -o 'impl_is_else_if_(\w+)!\(\s*(\w+)' -r '$1 $2' \
     big-code-analysis-ast/src/checker/ --no-filename | sort
  rg -l 'fn is_else_if' big-code-analysis-ast/src/checker/
  ```

  | Grammar model | Macro | Languages (at writing) |
  | --- | --- | --- |
  | `else_clause` wrapper around the nested `if` | `impl_is_else_if_parent_clause!` | C, C++, Mozcpp, Objc, Rust, JS, Mozjs, TS, TSX |
  | bare `else` token, nested `if` as next sibling | `impl_is_else_if_prev_sibling!` | C#, Groovy, Java, Kotlin |
  | dedicated `elseif` / `elsif` / `elif` clause node | `impl_is_else_if_clause!` | Bash, iRules, Lua, Perl, Ruby, Tcl |
  | hand-written (nested `if` with no wrapper, or mixed) | — | Go, PHP, Python |
  | constant `false` | — | Elixir (a stub — see lesson 10) |

  Pick the strategy whose model matches the grammar; an
  always-`false` stub is a bug, not a deferral. Issue #115
  (`013bff9`) caught exactly this in Java and C# after-the-fact —
  both languages had shipped the stub, and lesson 10 was written in
  response. Step 5 requires an `<lang>_else_if_chain` cognitive test
  that asserts the chain produces a *lower* score than the same
  number of `if` blocks nested inside one another — that test would
  have caught both #115 stubs the moment they were written.

- `is_primitive` — usually `false` unless the grammar emits a
  primitive-type kind (most don't); where it does, the `Primitive`
  row is the source of truth.

### 3c: `Getter` impl in `big-code-analysis-ast/src/getter/<lang>.rs`

Create the file, add `mod <lang>;` to `getter.rs`, and write
`impl Getter for <LangName>Code`. Required methods:

- `get_space_kind` / `get_space_kind_with_code` — forward to
  `crate::roles::<lang>::opens_space`, the fold over every `opens K`
  annotation on the `FuncOpener` and `Container` rows (#1696). The
  byte-less spelling forwards with `Ancestors::known(&[])`, as the
  Lua and Go impls do; `is_func_space` and `get_space_kind` must read
  the same predicate or the space tree disagrees with itself
  (`grammar-dispatch.md` §7).
- `keyword_roles!(<lang>);` — emits `get_keyword_role`, which is
  **required** by the trait (no default), from the `Modifier`,
  `DeclKeyword` and `Primitive` rows. Pass `primitive: Operand` if the
  language bills a primitive type as an operand (PHP does).
- `paying_roles!(<lang>);` — emits `get_paying_role` from the seven
  paying-construct rows (`Comparison`, `ShortCircuit`, `EagerLogical`,
  `Ternary`, `CaseArm`, `Guard`, `NullSafe`).
- `get_op_type` — Halstead operator/operand classification of
  **everything the rows do not already bill**. This is still the
  largest piece of language-specific work. Read the grammar's
  `node-types.json` (in the grammar crate's published source) or run
  `bca dump` against a representative file to see what kinds appear.
  Bucket every remaining kind into `TokenRole::Operator`,
  `TokenRole::Operand`, or `TokenRole::Unknown`. Cover at minimum:
  - **Operators**: control-flow keywords, structural punctuation —
    only the *opening* delimiter of a pair (`{`, `[`, `(`; the
    `get_operator!` fold spells the pair as one operator, #695),
    `,`, `;`, `:`, `.`, `...` — and the arithmetic / logical / bitwise
    / assignment operators the rows do not own. Do **not** list a
    keyword a `Modifier` / `DeclKeyword` / `Primitive` row owns, or a
    token a paying row pays: `make role-bypass` reports it.
  - **Operands**: identifiers (plain, field, package, type, label,
    etc.), literals (int, float, string, char/rune, bool, nil, etc.).
- `get_operator!(<LangName>);` — final macro line.

**Naming-collision gotcha** (from Go): if the language enum's name
collides with a variant, alias it. Go's `Go::Go` (the `go` keyword)
clashed with `use Go::*` in pattern position; the fix was
`use Go as G;`. Detect the collision proactively after Step 1e:

```bash
rg "^\s*<LangName>\s*=" big-code-analysis-ast/src/languages/language_<lang>.rs
```

If the search returns a hit, alias the import at the top of the
metric file with `use <LangName> as <ShortAlias>;`.

**No kind_id classified as both operator and operand.** Each
`get_op_type` arm must route a given kind_id to exactly one of
`TokenRole::Operator`, `TokenRole::Operand`, or
`TokenRole::Unknown`. A copy-paste that lands the same kind in
two arms produces `n1` / `n2` counts that disagree with the `bca ops`
output (TypeScript `String2` shipped this exact bug in `2248bcc`).
After classifying, the Halstead unit test added in Step 5 must
assert the load-bearing invariant from lesson 4: the metric store and
the `bca ops` store are independent, so run both on the same input
and check `len(dedupe(ops.operands)) == n2`. The test module in
`src/metrics/halstead.rs` has the helpers — `ops_of::<T>(source,
file)` returns the ops store, and `assert_ops_operands::<T>(source,
file, n2, vec![..])` pins both the deduplicated count and the operand
vocabulary (the Groovy and Perl rows show the call).

### 3d: `Alterator` impl in `big-code-analysis-ast/src/alterator.rs`

String literals render in `bca dump` as one verbatim leaf, so the dump
agrees with `find string` (#699). Add an `impl Alterator for
<LangName>Code` block that reads the `StringLit` row from 3a and
flattens what it lists, adding by kind only the non-string literals
the language also wants flattened (Go's `rune_literal`). Use the Go
impl as the template:

```rust
impl Alterator for <LangName>Code {
    fn alterate(
        node: &Node,
        code: &[u8],
        span: bool,
        field_name: Option<&'static str>,
        children: Vec<AstNode>,
    ) -> AstNode {
        let kind = node.kind_id();
        if crate::roles::<lang>::string_lit::lists_kind(kind)
            || matches!(kind.into(), <LangName>::RuneLiteral)
        {
            Self::flatten_verbatim(node, code, span, field_name)
        } else {
            Self::get_default(node, code, span, field_name, children)
        }
    }
}
```

Only `Preproc` and `Ccomment` — whose dump is not a user surface —
carry a bare `impl Alterator for XCode {}`; every extension-owning
grammar flattens its `StringLit` row.

### 3e: Compile-check again

```bash
cargo check -p big-code-analysis
```

Now the only remaining errors should be missing metric impls.

---

## Step 3.5: Aliased-variant audit (MANDATORY)

Tree-sitter grammars regularly emit several `kind_id` values that all
map to the same `node.kind()` string — `PrimitiveType` /
`PrimitiveType2` / … / `PrimitiveType17` in Rust;
`InvocationExpression` / `InvocationExpression2` / `InvocationExpression3`
in C#; `Identifier` / `Identifier2` / `Identifier3` in Go;
`HeredocBody` / `HeredocBody2` in Bash; `String` / `String2` /
`String3` in JS-family grammars. The generator assigns each
appearance a distinct `u16`; a `match` arm that lists only the
unsuffixed variant silently drops the rest. **This bug class shipped
inside the C# language-support PR itself** (issue #94, fix
`f042659`) and has been re-discovered in nearly every language ever
added. The fix is mechanical, but only if done before the snapshots
freeze the wrong values.

For the new language, enumerate every alias-group base — the
rule-name root for which both an unsuffixed variant and one or more
numbered siblings exist in the generated enum:

```bash
LANG_FILE="big-code-analysis-ast/src/languages/language_<lang>.rs"
comm -12 \
  <(rg -o '^\s+([A-Z][A-Za-z]*)\d+\s*=' -r '$1' "$LANG_FILE" | sort -u) \
  <(rg -o '^\s+([A-Z][A-Za-z]*)\s*=' -r '$1' "$LANG_FILE" | sort -u)
```

The naive `rg '^\s+[A-Z][A-Za-z]*\d+\s*=' …` also surfaces
lexer-token variants like `RawStringLiteralToken1` that are *not*
part of an alias group; the `comm` form drops them by requiring an
unsuffixed sibling in the same file. The result is the set of base
names whose every numbered variant is a potential aliasing risk.

For each printed base, confirm that EVERY numbered variant in its
group holds one of the following in EVERY file that does a `match`
on the underlying rule (`big-code-analysis-ast/src/roles/<lang>.rs`,
`big-code-analysis-ast/src/checker/<lang>.rs`,
`big-code-analysis-ast/src/getter/<lang>.rs`,
`big-code-analysis-ast/src/alterator.rs`, `src/metrics/**/<lang>.rs`,
`src/spaces/compute.rs`):

1. The variant is explicitly listed in the relevant arm or row
   (typically alongside its unsuffixed sibling:
   `Identifier | Identifier2 | Identifier3 => …`, or all three in one
   `owns [..]` list).
2. The variant is explicitly excluded with a one-line comment
   explaining why (e.g. "`HeredocBody` id 153 never surfaces in
   real parse trees per upstream") — in a role table that is
   `excludes [Kind "why"]`, which the coverage gate reads.
3. The match has been rewritten to compare on `node.kind()` (the
   string), which sidesteps the entire numeric-variant problem.

Option 3 is the most forward-compatible choice (the *next* grammar
bump will add a new aliased ID that option 1 misses) and is
preferred unless the per-call hot path is provably
allocation-sensitive — see lesson 2. For a kind a role owns, the
row **is** the list: `make role-coverage` fails on any id — alias or
not — that no row claims or excludes, which turns most of this audit
into a gate (Step 3a), but only for kinds the roles cover.

**Coverage targets for this audit:**

- `roles/<lang>.rs` — every `owns` / `pays` / `wraps` list: a row
  naming `Identifier` and not `Identifier2` claims half the
  construct.
- `checker/<lang>.rs` — every hand-written `is_*` predicate that
  touches an aliasable kind: identifiers, member/field expressions,
  primitive types.
- `getter/<lang>.rs` — `get_op_type` (Halstead classification) and
  any helper that branches on aliasable kinds.
- `alterator.rs` — any kind flattened beside the `StringLit` row.
  (Issue #119 was *only* in `alterator.rs` for JS/TS/TSX —
  `checker.rs` and `getter.rs` were already correct.)
- `metrics/{cognitive,cyclomatic,abc,npa,npm,loc}/<lang>.rs` —
  anywhere a per-language impl matches on a kind that the grammar
  aliases.

Re-run this audit after every grammar version bump in the same
crate's lifetime — lesson 2 explicitly warns the next aliased
variant appears on the next bump.

---

## Step 4: Register `<LangName>Code` with all 13 metric traits

`MetricSuite` (`src/metric_suite.rs`) is implemented for
`Parser<T>` only when `T` carries all thirteen metric traits, so
`cargo check` fails until every one lists the new tag. The
minimum-viable wiring is the no-op default for every metric, then
real impls for the metrics that have meaningful semantics for the
language. This mirrors how Go was landed: the parser shipped with all
defaults in commit `6ecc582`, and real impls followed in `e0701e1`.

### 4a: Default-impl all 13 metrics first

Append `<LangName>Code` to each `implement_metric_trait!(...)` macro
invocation in:

| File | Trait | Macro arm |
|------|-------|-----------|
| `src/metrics/abc.rs` | `Abc` | named — silent no-op |
| `src/metrics/cognitive.rs` | `Cognitive` | named — silent no-op |
| `src/metrics/cyclomatic.rs` | `Cyclomatic` | named — silent no-op |
| `src/metrics/nexits.rs` | `Exit` | named — silent no-op |
| `src/metrics/halstead.rs` | `Halstead` | see below |
| `src/metrics/loc.rs` | `Loc` | named — silent no-op |
| `src/metrics/mi.rs` | `[Mi]` | bracketed — trait default |
| `src/metrics/nargs.rs` | `[NArgs]` | bracketed — trait default |
| `src/metrics/nom.rs` | `[Nom]` | bracketed — trait default |
| `src/metrics/npa.rs` | `Npa` | named — silent no-op |
| `src/metrics/npm.rs` | `Npm` | named — silent no-op |
| `src/metrics/tokens.rs` | `[Tokens]` | bracketed — trait default |
| `src/metrics/wmc.rs` | `Wmc` | named — silent no-op |

The two arm shapes differ in meaning (see the comment above
`implement_metric_trait!` in `src/macros/mod.rs`): a *named* arm emits
an empty `compute` body, so the metric reports 0 on every input and
nothing says so; a *bracketed* arm emits `impl Trait for X {}` and the
trait's own default body runs, which is correct for `Mi`, `Tokens`,
`Nom` and `NArgs`. Most invocation sites carry a comment recording
why every listed language is there (#188) — add the new language to
that comment, with its follow-up issue number if the default is a
placeholder rather than a real "this language has no such construct";
where a site has no such comment (`Cognitive`'s today), add one.

**Halstead has no default worth using.** Every language with a
grammar is listed in `impl_halstead_forwarding!` in
`src/metrics/halstead.rs`, which forwards to `compute_halstead::<Self>`
through the `Getter` from 3c; only `Preproc` and `Ccomment` take the
silent arm. Add the new tag to the forwarding list, not to
`implement_metric_trait!`.

Run `cargo check -p big-code-analysis` — it should now succeed.

### 4b: Replace defaults with real impls for the language's primary metrics

**Always** replace the default impl with a real one for these four:

- **Cyclomatic** (`src/metrics/cyclomatic/<lang>.rs`, registered with
  `mod <lang>;` in `cyclomatic.rs`): `+1` per `if`, `for`, `while`,
  `case` (each switch arm), `&&`, `||`, catch/except/rescue, ternary.
  Read the `ShortCircuit` / `Comparison` / `CaseArm` / `Ternary` rows
  from 3a for the tokens they pay (`short_circuit::pays_at_leaf_kind`,
  as the Lua impl does) rather than listing the kinds again — the
  bypass gate reports a restatement. **Default/wildcard arms do NOT
  add a branch** — this is consistent with the existing Java and Cpp
  impls in this repo, and was reaffirmed for Go in commit `e0701e1`.
  Match the language's actual node kinds, not generic names.
- **Exit** (`src/metrics/nexits.rs`): `+1` per `return`/`yield` /
  language-specific exit. A multi-value return counts once.
- **Halstead** (`src/metrics/halstead.rs`): already wired in 4a via
  `impl_halstead_forwarding!`; the language-specific work is entirely
  the `Getter` from 3c, so there is nothing further to write here —
  verify it with the Step 5 Halstead test.
- **Loc** (`src/metrics/loc/<lang>.rs`, registered in `loc.rs`): see
  Step 4b-loc below — this is the largest and most subtle of the four
  primary metrics.

**Branch-construct coverage audit.** A `match node.kind_id()` in a
metric impl is *coverage*, not a *spec*: a grammar can emit a valid
construct under a node kind the arm forgot, and the metric silently
emits zero for it. C/C++ ternaries (#172, `b2ae93f`), C++
range-based `for` (#173, `7eef01a`), and Java enhanced-for (#178,
`96b73d6`) all survived years inside already-implemented Cognitive
impls because the arm list missed the relevant kind. After writing
each branching-metric impl, grep the generated enum for every kind
whose name suggests the construct:

```bash
rg 'For[A-Z]|While[A-Z]|If[A-Z]|Switch[A-Z]|Conditional|Ternary|Try[A-Z]|Catch[A-Z]|Match[A-Z]|Case[A-Z]' \
   big-code-analysis-ast/src/languages/language_<lang>.rs
```

Confirm each hit is either explicitly matched, or explicitly excluded
with a comment. When a known-wrong-but-unfixed case is identified
during the audit (e.g. a grammar quirk requiring its own fix issue),
anchor it with a regression test that asserts the *current wrong*
value plus an inline `FIXME(#NNN)` pointing at the tracking issue —
see `4b41187` / `e8b9a4e` for the canonical template. That keeps the
gap visible in CI; the eventual fix flips a literal value rather
than re-deriving it. See lesson 19.

**Non-zero smoke check for every real impl.** AGENTS.md and lesson 1
warn that a metric routed through `implement_metric_trait!` silently
emits zero on every input — there is no compile-time signal. For
every metric you replace from default to real impl, add at least one
test that exercises non-trivial control flow and asserts
`metric.X > 0.0` (or a positive `assert_eq!` on the headline value).
This is distinct from the exact-value snapshots in Step 5c and acts
as a structural smoke check: Bash shipped Cognitive / Exit / ABC as
silent zeros (#71, `d2be869`) precisely because no such test existed.

Add real impls for the remaining metrics where the grammar exposes
the necessary nodes:

- **Nom** (`src/metrics/nom.rs`): default trait body usually works if
  `Checker::is_func` and `Checker::is_closure` are correct. Verify
  with a unit test before assuming.
- **NArgs** (`src/metrics/nargs.rs`): default counts direct children
  of the parameter list. Some grammars group same-typed parameters
  into a single node (Go does this with `parameter_declaration`,
  collapsing `func f(a, b int)` to one node). If so, add a
  language-specific `compute` that walks into each grouped node and
  counts identifier names. Use Go's `compute_go_args` as the
  reference.
- **Cognitive** (`src/metrics/cognitive/<lang>.rs`): substantial
  work; only add if the language has well-defined nesting semantics.
  OK to leave as default for now.
- **Abc** (`src/metrics/abc/<lang>.rs`), **Npa** / **Npm**
  (`src/metrics/npa/<lang>.rs`, `npm/<lang>.rs`), **Mi**, **Wmc**:
  usually fine as defaults for procedural languages; add real impls
  only if the language has classes/methods/attributes with clear
  semantics. Note that `Npa` / `Npm` are only *reported* for a space
  whose `SpaceKind` is a member scope (`MemberScopeExt`,
  `src/metrics/mod.rs`), so a `Container` row with the wrong `opens`
  silently drops the whole block — see `grammar-dispatch.md` §7.

### 4b-loc: Implementing the Loc metric

The Loc metric is more involved than the other primary metrics because
each of its sub-counts has a distinct definition. Implement
`impl Loc for <LangName>Code` in `src/metrics/loc/<lang>.rs` with a
`compute` function that mirrors a sibling language's structure (Java
for curly-brace languages, Python for indent-sensitive ones). Since
#1597 there is a sixth sub-count, **DLOC** (documentation lines), fed
by `Checker::is_doc_comment` and the `add_*_doc_run` helpers in
`loc/shared.rs`; a language with no documentation convention this crate
recognises reports `0` and needs no arm for it.

#### Sub-metric definitions

These definitions follow the Mozilla LoC guide
(<https://mozilla.github.io/rust-code-analysis/developers/loc.html>).
Use this Rust factorial example as the canonical reference for what
each count produces:

```rust
/*
Instruction: Implement factorial function
For extra credits, do not use mutable state or a imperative loop like `for` or `while`.
 */

/// Factorial: n! = n*(n-1)*(n-2)*(n-3)...3*2*1
fn factorial(num: u64) -> u64 {

    // use `product` on `Iterator`
    (1..=num).product()
}
```

Expected counts on the file above:

| Sub-metric | Value | Definition |
|------------|-------|------------|
| **SLOC** | 11 | Every line in the file: code, comments, blanks. A straight line count. |
| **PLOC** | 3 | Physical lines of *instruction* code: brackets, statements, declarations on their own line all count. Comments and blanks are excluded. |
| **LLOC** | 1 | "Logical" lines — count of statements. Statement boundaries are language-specific (e.g. `;` in C-family, newline-or-`;` in Python). The factorial body is one statement (the `(1..=num).product()` expression returned). |
| **CLOC** | 6 | Comment lines, of any kind: `//`, `///`, `/* */`, `#`, `--`, etc. Block comments count every line they span. |
| **BLANK** | 2 | Lines with only whitespace. |

#### Implementation guidance

- **SLOC** — straightforward `end_row - start_row + 1` on the source
  file root span.
- **PLOC** — record every line that contains a non-comment,
  non-whitespace token. The standard approach is to walk the AST and
  insert `node.start_position().row` into a set for non-comment leaf
  nodes; PLOC is the set's cardinality. Look at how Java/Rust do this
  in `src/metrics/loc/java.rs` / `loc/rust.rs` and copy the pattern;
  do not re-invent.
- **CLOC** — for every comment node, count `end_row - start_row + 1`.
  Block comments span multiple lines and must be counted per-line, not
  per-node.
- **BLANK** — `SLOC - (PLOC ∪ CLOC line-set)`. The existing helpers
  in `loc/shared.rs` and `loc/line_set.rs` compute this from the
  line-set unions; do not double-count
  lines that are both code and comment (a `// foo` after a `}` on the
  same line is one PLOC line and one CLOC line, but it is *not* a
  blank).
- **LLOC** — the language-specific piece. List every node kind in the
  grammar that represents a "statement" and count one per occurrence.
  For C-family languages this is roughly `expression_statement`,
  `if_statement`, `for_statement`, `while_statement`,
  `return_statement`, `var_declaration`, `assignment_statement`, etc.
  **Watch the gating quirks** — sibling languages have non-obvious
  exceptions:
  - **Rust** excludes `field_expression`, `parenthesized_expression`,
    `array_expression`, `tuple_expression`, `unit_expression`,
    macro invocations from LLOC (they are sub-expressions, not
    statements). See the `rust_no_*_lloc` tests in `loc.rs`.
  - **Go** excludes simple/short/var/const declarations inside a
    `for_clause` init or post slot so the surrounding `for_statement`
    counts as one logical line.
  - **Python** counts each top-level simple statement and each
    compound-statement header (`if`, `for`, `while`, `def`, `class`,
    etc.) once. Block bodies do not double-count their headers.
  Enumerate every kind that *could* be a statement, decide whether it
  is gated, and add a regression test in Step 5 for each gating
  decision.

Since the shared helpers landed, a per-language `loc/<lang>.rs` is
small: Java's is ~65 lines, Rust's ~100, and the largest (Perl, with
its POD and statement-modifier rules) ~225. A `compute` that runs
long is a sign something belongs in `loc/shared.rs`, not a reason to
extract speculative helpers; flow follows the AST.

### 4c: Compile and run the existing test suite

```bash
cargo check --workspace --all-targets
cargo test --workspace --all-features
```

All previously passing tests must still pass. If a sibling-language
test now fails, you have introduced cross-language regression — fix
before continuing.

---

## Step 5: Add per-language tests

For every metric with a real impl in 4b, add tests under the metric
module's `mod tests` (in `src/metrics/<metric>.rs`; the per-language
`<metric>/<lang>.rs` files hold impls, and the tests stay in the
parent's module). Use the existing per-language test conventions —
`check_metrics::<<LangName>Parser>` with the module's
`check_metrics_only_shim!`, and `#[cfg(feature = "<lang>")]` on every
test (5g).

### 5a: Coverage target — match Rust

The new language must reach **roughly the same per-metric test count
as the existing Rust coverage**. Rust is the reference because it
exercises every metric the project supports and its tests are the
gold standard for what "thoroughly tested" looks like.

Count tests from the harness, not from the source: `fn rust_*`
helpers (`rust_wrapper_operand`, `rust_condition_scores_itself`) share
the prefix with the tests and inflate an `rg` count. Run this before
starting to anchor the targets to current state:

```bash
tests=$(mktemp -t bca-tests.XXXXXX.txt)
cargo test -p big-code-analysis --lib --all-features -- --list \
  > "$tests" 2>/dev/null
for m in abc cognitive cyclomatic halstead loc mi nargs nexits nom npa npm tokens wmc; do
  printf "%-12s rust=%d\n" "$m" \
    "$(rg -c "^metrics::$m::[a-z_:]*rust_" "$tests" || echo 0)"
done
```

At the time this skill was last revised, Rust's per-metric test counts
were:

| Metric | Rust tests | Notes / scope a `<lang>_*` test should cover |
|--------|-----------:|--------------------------------------------|
| `cognitive` | 17 | no-op file, simple function, sequence of same/different booleans, negated booleans, 1-level nesting, 2-level nesting, `break`/`continue`, complex `if let / else if / else`, plus the `else_if_chain` test from 5b. |
| `cyclomatic` | 13 | nested-control-flow representative test, then one per construct the language handles unusually (`match` arms, `?`, closures). |
| `nexits` | 7 | function with no exit, function with `?` / language equivalent of early-exit, explicit vs implicit (tail-expression) return, mixed forms. |
| `halstead` | 10 | one comprehensive `<lang>_operators_and_operands` test pinning every Halstead field via `insta`, then one per classification decision worth isolating (string kinds, primitive types, bracket pairs). |
| `loc` | 24 | blank, no-zero-blank, cloc, lloc, dloc, then **at least one `<lang>_no_*_lloc` test for each gating decision** (Rust's `rust_no_*_lloc` set: `field_expression`, `parenthesized_expression`, `array_expression`, `tuple_expression`, `unit_expression`) and one positive-count `<lang>_<construct>_lloc` per construct that *does* count (Rust: `call_function`, `macro_invocation`, `function_in_loop`, `function_in_if`, `function_in_return`, `closure_expression`). Mirror this — every node kind your `compute` excludes from LLOC needs a dedicated test that would fail if the gating is removed. |
| `nargs` | 6 | no functions/closures, single function, single closure, multiple functions, nested functions. |
| `nom` | 1 | one comprehensive `<lang>_nom` test exercising functions, methods, and closures together. |
| `abc` | 29 | one per ABC slot the language fills (assignments, branches, conditions) and one per excluded construct. Skip if `Abc` is left as default. |
| `npa`, `npm`, `wmc` | 13 / 7 / 7 | member-scope tests. Skip unless the language has containers (classes, modules) and a real `Npa` / `Npm` / `Wmc` impl. |
| `mi`, `tokens` | 1 / 2 | one smoke test each; the trait default is shared across languages. |

**Floor:** for every metric the new language implements for real,
match Rust's count for that metric as the command above reports it
today — these numbers are a snapshot and have grown with every sweep.

**Ceiling:** do not pad with redundant tests. Each test must exercise
a distinct construct or gating decision; copy-pasted near-duplicates
fail `audit-tests` in Step 8c.

### 5b: Suggested per-metric test list

The 5a table is the source of truth for *count*; this list is the
shape each test should take. Add extras beyond it only when the
grammar has constructs the existing repo has not seen (e.g. Go's
`select`, Erlang's `receive`).

- **`cyclomatic`** — `rust_1_level_nesting` is the floor: a
  nested-control-flow test that exercises an `if` inside a loop. Then
  one test per *novel* construct the grammar exposes — e.g. Go's
  `select`, Ruby's `rescue`, Python's exception handlers — plus, where
  applicable, a "non-branching feature that should NOT count" test
  (Go's `defer`/`go` test is the template) and one per wildcard /
  default-arm decision (the `rust_*_wildcard_*` family). Do not pad
  the count by adding one test per ordinary branching keyword; that
  is what the single nested test already covers.
- **`nexits`** — `<lang>_no_exit` (function with no return), and one
  test per language-specific exit path (e.g. Rust's `?`, Go's
  multi-value return, Python's `yield`/`raise`, Ruby's `next`/`break`
  out of a block if you treat them as exits — document the choice).
- **`halstead`** — one `<lang>_operators_and_operands` test using a
  source snippet that exercises every operator family classified in
  Step 3a–3c (control-flow keywords, declaration keywords, structural
  punctuation, arithmetic, comparison, logical, bitwise, assignment).
  Pin **every** field via `insta::assert_json_snapshot!` — no loose
  inequalities (see 5c). Then one test per row-driven decision worth
  isolating: that a `DeclKeyword` bills, that a `Comparison` payer
  bills once and its owning `binary_expression` not at all, that a
  bracket pair is one operator.
- **`loc`** — `<lang>_blank`, `<lang>_no_zero_blank`, `<lang>_cloc`,
  `<lang>_lloc`, `<lang>_dloc` (or a test pinning `dloc == 0` when the
  language has no documentation convention), plus one
  `<lang>_no_<kind>_lloc` per LLOC-gated node kind from Step 4b-loc.
  Aim for the same count as Rust.
- **`cognitive`** — `<lang>_no_cognitive`, `<lang>_simple_function`,
  `<lang>_sequence_same_booleans`, `<lang>_sequence_different_booleans`,
  `<lang>_not_booleans`, `<lang>_1_level_nesting`,
  `<lang>_2_level_nesting`, `<lang>_break_continue`,
  `<lang>_else_if_chain` (asserts an `if … else if … else if … else`
  chain produces a *lower* score than the same number of `if` blocks
  nested inside one another — this is the test that would have
  caught the #115 `is_else_if` stubs in Java and C# the moment they
  were written; see Step 3b and lesson 10), and one
  language-specific complex-nesting test. Skip if you left Cognitive
  as a default impl in 4b — but flag this loudly in the summary.
- **`nargs`** — `<lang>_no_functions_and_closures`,
  `<lang>_single_function`, `<lang>_single_closure`,
  `<lang>_functions`, `<lang>_nested_functions`, plus extra tests for
  any language-specific argument quirk (Go's grouped same-type
  parameter declaration, Python's `*args`/`**kwargs`, Ruby's block
  parameter `&blk`).
- **`nom`** — one `<lang>_nom` test combining functions, methods,
  closures, and (if applicable) nested definitions — verifying both
  total count and the per-kind breakdown.

### 5c: Use exact-value insta snapshots, never loose inequalities

This is the lesson from commit `ecb6299` (Go Halstead/Loc tests). A
test like `assert!(metric.halstead.length > 0.0)` passes for the
wrong reason — a regression in `Getter::get_op_type` would not flip
it. (The non-zero smoke checks from Step 4b are an additional
defence against the silent-zero failure mode in lesson 1; they are
not a substitute for exact-value pinning here.)

**AGENTS.md enforces snapshot anchoring.** Every new
`insta::assert_json_snapshot!(metric.X)` call must carry one of: an
inline expected block, a positive `assert_eq!` anchor on the
headline integer value above it, or a `// expected: <derivation>`
comment. `make snapshot-anchors` (part of `make pre-commit` and
CI's `lint` job) verifies this against
`.snapshot-anchor-baseline.txt` and fails on any per-file increase.
New language tests start at zero — do not introduce unanchored
snapshots; existing bare snapshots are grandfathered under
issue #95 and are not a precedent. The Halstead test must also
assert the lesson-4 invariant (`len(dedupe(ops.operands)) == n2`) on
the same input through `assert_ops_operands` (Step 3c); the Go and
Bash `*_operators_and_operands` tests read the metric store only (Go
through an inline snapshot, Bash through the accessors plus a
snapshot), so they are the snapshot template, not the cross-store
template.

Pin every Halstead and Loc field with `insta::assert_json_snapshot!`:

```rust
insta::assert_json_snapshot!(
    metric.cyclomatic,
    @r###"
    {
      "sum": 2.0,
      "average": 1.0,
      "min": 1.0,
      "max": 1.0
    }"###
);
```

For new snapshots, run `cargo insta test --review` and accept each
one only after manually confirming the values match the expected
counts. Do not blindly accept.

### 5d: Cross-check with the `check_metrics` helper

The standard test scaffold is:

```rust
#[cfg(feature = "<lang>")]
#[test]
fn <lang>_single_function() {
    check_metrics::<<LangName>Parser>(
        "<source code>",
        "foo.<ext>",
        |metric| { /* assertions */ },
    );
}
```

`check_metrics` here is the module-local shim each `mod tests` defines
once with `check_metrics_only_shim!(check_metrics, <Metric>)` (see
`src/test_support.rs`), bound to that module's metric family; the
`#[cfg]` is the per-language gate 5g enforces. The source must be
syntactically valid for the grammar, or tree-sitter will produce an
error tree and the metric counts will be undefined.

### 5e: Coverage gate

Before moving to Step 6, run this comparison against the compiled
test list and verify the new language's per-metric counts are at or
above Rust's:

```bash
NEW_LANG="<lowercase-lang>"
tests=$(mktemp -t bca-tests.XXXXXX.txt)
cargo test -p big-code-analysis --lib --all-features -- --list \
  > "$tests" 2>/dev/null
for m in abc cognitive cyclomatic halstead loc mi nargs nexits nom npa npm tokens wmc; do
  rust_n=$(rg -c "^metrics::$m::[a-z_:]*rust_" "$tests" || echo 0)
  lang_n=$(rg -c "^metrics::$m::[a-z_:]*${NEW_LANG}_" "$tests" || echo 0)
  status="OK"
  [ "$lang_n" -lt "$rust_n" ] && status="UNDER (need $((rust_n - lang_n)) more)"
  printf "%-12s rust=%2d  %s=%2d  %s\n" "$m" "$rust_n" "$NEW_LANG" "$lang_n" "$status"
done
```

If any metric reports `UNDER`, add tests until at parity. The only
acceptable shortfalls are:

1. metrics where the language has no real impl (left as default in
   Step 4b) — but flag this in the summary;
2. `npa` / `npm` / `wmc` for a language with no containers, and the
   one-test `mi` / `tokens` smoke rows, where the trait default is
   shared and a per-language test adds nothing.

### 5f: Cross-language metric parity

Per-language snapshot tests pin each language's metric output
against its own history. They cannot detect that two languages
disagree about the same construct (lesson 11). Rust counted wildcard
`_ =>` while C-family did not count `default:` (#106, `a54b073`);
Bash double-counted `case…esac` container plus arms (#107,
`e668f14`) — both bugs that survived per-language snapshot suites
for years.

The parity suites already exist under `tests/parity/` — one file per
family (`cyclomatic_cross_language_parity.rs`,
`cognitive_cross_language_parity.rs`, `exit_cross_language_parity.rs`,
`nargs_cross_language_parity.rs`, `ops_metrics_space_parity.rs`, and
the rest). Most pick their fixture with a `match lang` over the whole
`LANG` enum. The cyclomatic, cognitive, exit, nargs and
ops-metrics-space suites are exhaustive and stop compiling the moment
Step 2c adds the variant; `abc_halstead_bracket_parity.rs` and
`self_reference_operand_parity.rs` carry a `_ =>` arm and silently
skip a language with no row, so check those two by hand;
`functions_metrics_parity.rs` and `space_span_containment.rs` reuse
the ops-metrics-space fixture and need nothing of their own, and
`ast_export`, `cpp_mozcpp` and `halstead_set_target` test fixed
languages. For every suite that has rows, add the new language's:

- 2-arm conditional with a `default` / `else` / wildcard arm:
  cyclomatic and cognitive must agree (modulo documented
  per-language quirks).
- An `if … else if … else` chain of length 3: cognitive must agree.
- A loop body with one early-exit (`return` / `break`): exit must
  agree.
- A function with three formal parameters: `nargs` must agree.
- `bca ops` and `bca metrics` must open the same space tree
  (`ops_metrics_space_parity.rs`, #1130).

Any *intentional* per-language divergence (e.g. Rust's standard-CCN
wildcard-arm exception) is documented in the parity test's comment
and asserted with an explicit offset to keep the divergence visible.

### 5g: Per-language test gates

Every test that names the new parser — `check_metrics::<<Lang>Parser>`,
`LANG::<Lang>` handed to `analyze`, a fixture table row — must carry
`#[cfg(feature = "<lang>")]` (or be inside an `any(..)` union that
includes it), because `mk_langs!` generates every `*Parser` alias
unconditionally and `Tree::new` panics in a build without the grammar
(#1472). `make check-test-lang-gates` (in `make pre-commit`) derives
the required marker for every test under `src/`,
`big-code-analysis-ast/src/` and `tests/` and fails on one that is
missing or wider than the body needs. Write the markers with the
script directly — the Makefile target forwards no arguments:

```bash
./utils/check-test-lang-gates.py --fix
./utils/check-test-lang-gates.py --show   # the derivation, per item
make check-test-lang-gates                 # the gate as pre-commit runs it
```

Sweeps that filter on `LANG::is_enabled()` and assert non-vacuity
need `any(<every language>)`; a helper that picks a language from a
string needs a hand-written gate with a
`// test-lang-gates: hand-written(<lang>) — <why>` marker. The rules
are in `.claude/rules/testing.md`, "Every test that names a language
carries that language's gate".

---

## Step 6: Documentation

### 6a: Add the language to the supported-languages list

Edit `big-code-analysis-book/src/languages.md` and insert the new
language alphabetically:

```markdown
- [x] <LangName>
```

Two more lists name every language with its feature and grammar
crate; add a row to each, alphabetically:

- `big-code-analysis-book/src/library/cargo-features.md` — the
  feature → grammar-crate table;
- the crate-level doc comment in `src/lib.rs` — the bullet list of
  languages and their feature names.

Edits to the book are not blocked on the Japanese translation
(`big-code-analysis-book/po/ja.po`); changed paragraphs fall back to
English until someone runs `make book-po-update`.

### 6b: Update CHANGELOG

`CHANGELOG.md` is present at the repo root and used. A new language
warrants its own entry — do not consolidate into a batch-fix line.
Append under `Added` within the `## [Unreleased]` section:

```markdown
- Support for <LangName> source files (`.<ext>`).
```

### 6c: Skip README counts

Do not hardcode "now supports N languages" anywhere — counts rot.

---

## Step 7: Final validation gate

`make pre-commit` is the canonical entry point per AGENTS.md. It
runs the cargo trio (fmt-check, clippy with `-D warnings` in both
default-features and `--all-features` flavours, full test suite),
`cargo +nightly udeps`, the markdown / TOML / shell / Makefile /
GitHub Actions lint families, and every repo-specific gate, in one
parallel pass. The ones a language addition touches directly:

- `make enums-check` — clippy + `cargo test` on the workspace-excluded
  `enums` crate (the 1b dispatch test and variant count; lesson 15),
  and `make enums-codegen-drift`, which re-runs the generator into a
  tempdir and diffs it against the checked-in `language_*.rs`.
- `make check-excluded-manifests` — the `=X.Y.Z` pin in
  `enums/Cargo.toml` and its `[lints]` table.
- `make role-census` / `make role-coverage` / `make role-bypass` —
  Step 3a; the first two fail outright on a language with no table or
  no baseline section.
- `make check-test-lang-gates` — Step 5g.
- `make snapshot-anchors` — enforces 5c (lesson 6).
- `make self-scan` / `make self-scan-headroom` — the repo's own
  `bca check`; a large new `Loc::compute` or `get_op_type` can trip a
  threshold. Refactor if it genuinely clarifies, otherwise suppress
  in-source with a reason (AGENTS.md, "Responding to bca metric
  feedback"); do not regenerate `.bca-baseline.toml` to hide a new
  offender.

Two gates are deliberately *not* in `make pre-commit` and a new
language is exactly what they exist for (AGENTS.md, "Benchmarking" and
"The ancestor-chain audit"): `make bench-scaling`, because a new
`Checker` / `Getter` / `compute` is where a quadratic walk arrives, and
it shows up as *slow* tests rather than red ones; and `make
chain-audit`, because the `_with_code` row predicates read the
ancestor chain and only the exact assertion sees a chain short by one.
Run both before declaring the language done.

Run `make worktree-setup` first in a checkout that has not run the
gate before — it checks out the corpus submodules `role-coverage`
reads. Then:

```bash
log=$(mktemp /tmp/bca-pre-commit.XXXXXX.log)
make pre-commit >"$log" 2>&1
grep '^BCA_GATE:' "$log"
```

Read the verdict from the `BCA_GATE:` line only; the gate is a
parallel DAG and a stage's `Error` line is routinely followed by
other stages' successful output. No `BCA_GATE:` line is a crash, not a
pass.

`make ci` runs the same checks without auto-fix (mirrors CI
behaviour). If GNU Make 4 or any optional tool (`taplo`,
`rumdl`, `shellcheck`, `shfmt`, `checkmake`, `actionlint`) is
unavailable, fall back to the raw equivalents:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
RUSTFLAGS="-D warnings" cargo clippy --manifest-path enums/Cargo.toml \
  --all-targets --locked -- -D warnings
cargo test --manifest-path enums/Cargo.toml --locked
./utils/check-snapshot-anchors.py
./utils/check-role-census.py
./utils/check-role-coverage.py --require-observations
./utils/check-role-bypass.py
./utils/check-test-lang-gates.py
```

If `pre-commit` is installed, also run `pre-commit run --all-files`.

**Snapshots.** For any new or changed `insta` snapshot in
`src/metrics/`, run `cargo insta test --review` and review each
diff manually. If the new language's Halstead operator/operand
classification causes cascading metric shifts in existing snapshots,
verify the diffs are metric-value-only (no structural changes), then
use `cargo insta test --accept` per test file rather than incremental
`mv *.snap.new` — incremental acceptance shifts `assertion_line`
fields, causing further cascading mismatches.

**Integration snapshots live in a submodule.** Behaviour-changing
work on metric computation, AST traversal, or alterator rules
generates `.snap.new` files inside
`tests/repositories/big-code-analysis-output/`, which is a separate
git submodule (`dekobon/big-code-analysis-output`). Per AGENTS.md
and lesson 8, a language addition is not done until ALL four hold:

1. `cargo test --workspace --all-features` exits clean from a
   fresh working tree with no `.snap.new` left behind under
   `tests/repositories/big-code-analysis-output/`.
2. The accepted snapshots are committed and pushed inside the
   submodule to `dekobon/big-code-analysis-output`'s `main`
   branch.
3. The parent commit records the new submodule SHA
   (`git add tests/repositories/big-code-analysis-output`) — in
   the same parent commit as the language-addition code, never as
   a follow-up.
4. After any rebase or force-push to the submodule, re-run
   integration tests before declaring done; submodule history is
   force-pushed often enough that previously-accepted snapshots
   cannot be assumed to survive.

If validation fails, fix the root cause — do not paper over with
`#[allow(...)]` or by loosening assertions.

---

## Step 8: Run code-quality skills

Run the following skills in order. Each may modify files in place;
review their output before proceeding to the next.

### 8a: `simplify-rust`

Run the `simplify-rust` skill against the changed Rust code (the
Checker/Getter/Alterator impls and the per-metric impls). The skill
reviews diffs for reuse, clarity, and efficiency improvements and
applies fixes inline.

### 8b: `rust-optimize`

Run the `rust-optimize` skill on the same changed code to reduce
verbosity, modernize syntax for edition 2024 (let-else, let-chains),
and apply pedantic-clippy triage where safe.

### 8c: `audit-tests`

Run the `audit-tests` skill on the new per-language tests added in
Step 5. The skill flags tests that pass for the wrong reason —
loose inequalities, missing assertions, or coverage gaps. Fix any
findings inline; do not defer to a follow-up.

After 8c, re-run the validation gate from Step 7. If the skills
produced changes, the gate must still pass.

---

## Step 9: Stop — DO NOT commit

This skill **does not commit**. The user runs a final review pass and
makes the commits themselves, typically as a series of conventional
commits mirroring the Go history:

1. `feat(languages): add <LangName> language support` — wiring,
   role table, classifiers and default metric impls (Steps 1–4a).
2. `feat(<lang>): implement metric traits properly for <LangName>Code`
   — real impls (Step 4b).
3. `test(<lang>): add metric test matrix per issue #N spec` — tests
   (Step 5).
4. `docs(book): add <LangName> to supported languages list` —
   docs (Step 6).
5. Any cleanup commits from Step 8.

Before exiting, print a one-screen summary:

```text
Added <LangName> language support.

Files changed:
  Cargo.toml                                               ([workspace.dependencies] pin + forwarding feature)
  big-code-analysis-ast/Cargo.toml                         (optional dep + feature)
  enums/Cargo.toml
  enums/src/languages.rs
  enums/src/macros.rs
  enums/tests/dispatch.rs
  big-code-analysis-ast/src/langs.rs
  big-code-analysis-ast/src/macros/mod.rs                  (with_any_parser! arm)
  big-code-analysis-ast/src/language_enum_roundtrip.rs
  big-code-analysis-ast/src/languages/mod.rs
  big-code-analysis-ast/src/languages/language_<lang>.rs   (generated)
  big-code-analysis-ast/src/roles/mod.rs
  big-code-analysis-ast/src/roles/<lang>.rs                (new)
  big-code-analysis-ast/src/checker.rs                     (mod line)
  big-code-analysis-ast/src/checker/<lang>.rs              (new)
  big-code-analysis-ast/src/getter.rs                      (mod line)
  big-code-analysis-ast/src/getter/<lang>.rs               (new)
  big-code-analysis-ast/src/alterator.rs
  src/lib.rs                                               (language list in crate docs)
  src/metrics/{abc,cognitive,cyclomatic,loc,npa,npm}/<lang>.rs   (new, where a real impl exists)
  src/metrics/{abc,cognitive,cyclomatic,nexits,halstead,loc,mi,nargs,nom,npa,npm,tokens,wmc}.rs
  tests/parity/*.rs                                        (fixture rows)
  big-code-analysis-cli/src/markdown_report.rs             (display-name arm)
  big-code-analysis-book/src/languages.md
  big-code-analysis-book/src/library/cargo-features.md
  .role-coverage-baseline.txt                              (new section, only if residue was recorded)
  .role-coverage-observed.txt                              (refreshed by make role-coverage-observe)
  *.snap                             (new insta snapshots)

Validation: cargo fmt / clippy / test / insta — all passing.
Code-quality skills run: simplify-rust, rust-optimize, audit-tests.

NOT committed. Review `git status` and `git diff` and commit
manually.
```

**Heads up: mutation testing.** Quarterly mutation testing
(`.github/workflows/mutation-test.yml`, see
`docs/development/mutation_testing.md`) runs against
`src/metrics/`, `big-code-analysis-ast/src/checker.rs`, and
`big-code-analysis-ast/src/getter.rs`. Within one
cycle, expect auto-filed issues labelled `mutation-testing` against
the new language's impls; treat them as standard fix-issue work.
Escapes mean the per-language test set under-specified the new
impl — exactly the failure mode lessons 1, 6, and 19 warn about.

---

## Worktree Safety Reminder

If `git rev-parse --show-toplevel` returns a path under
`.claude/worktrees/`, all worktree-safety bans apply: do not delete
worktrees, do not `cd` to the main repo, do not check out a different
branch, do not write outside the current worktree.
