---
name: add-lang
description: End-to-end workflow for adding a new tree-sitter language to big-code-analysis. Wires up the grammar crate, regenerates the language enum, implements Checker/Getter/Alterator/metrics, adds per-metric tests, updates docs, then runs simplify-rust, rust-optimize, and audit-tests. Does NOT commit — leaves the working tree dirty for final user review.
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
commits when looking for a structural template. The upstream Mozilla
guide
(<https://mozilla.github.io/rust-code-analysis/developers/new-language.html>)
is also useful background, adapted to the current API and metric set.

## Arguments

Parse `$ARGUMENTS` as: `<lang-name> <grammar-crate>=<version> [<file-ext>...]`

- `<lang-name>` (required): PascalCase enum variant name, e.g. `Go`,
  `Ruby`, `Swift`. Must not collide with existing variants in
  `src/langs.rs` or `enums/src/languages.rs`.
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
- **Pin the grammar version** with `=X.Y.Z` in both `Cargo.toml` and
  `enums/Cargo.toml`. Never use a range without explicit user
  approval.
- **Cross-language parity.** All 12 metric trait impls
  (`Abc`, `Cognitive`, `Cyclomatic`, `Exit`, `Halstead`, `Loc`, `Mi`,
  `NArgs`, `Nom`, `Npa`, `Npm`, `Wmc`) must list the new `<Lang>Code`
  in either a real impl or `implement_metric_trait!`.
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
  `src/langs.rs` (search for `Lang::<lang-name>`).
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
solely to regenerate `src/languages/language_<lang>.rs` from a tree-sitter
grammar's node-kind table. Wire it up first so we can produce the enum
file before touching the main crate.

### 1a: Add the grammar crate to `enums/Cargo.toml`

Insert the dep alphabetically among the other `tree-sitter-*` lines:

```toml
tree-sitter-<lang> = "=<version>"
```

### 1b: Register the language in `enums/src/languages.rs`

Append a tuple to the `mk_langs!` invocation, keeping rough alphabetical
order:

```rust
(<LangName>, tree_sitter_<lang>),
```

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
let mut name = camel_case(name);
if name.is_empty() {
    name = format!("Anon{i}");
}
```

This was added during Go support because `tree-sitter-go` emits an
empty kind name at one position. If it is missing, add it.

### 1e: Generate `language_<lang>.rs`

From the repo root, mirroring `recreate-grammars.sh`:

```bash
cargo run --manifest-path ./enums/Cargo.toml -- -l rust -o ./src/languages
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
git status -- src/languages/
git diff src/languages/
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
17 in `lessons_learned.md`.

Confirm the new file exists at `src/languages/language_<lang>.rs` and
that it begins with `// Code generated; DO NOT EDIT.`.

If the project also depends on C-macro tables for the new language
(only relevant for C/C++ family preprocessor work), also run:

```bash
cargo run --manifest-path ./enums/Cargo.toml -- -l c_macros -o ./src/c_langs_macros
```

Most languages do not need this step.

---

## Step 2: Wire the grammar into the main crate

### 2a: Add the grammar to root `Cargo.toml`

Same pinned-version line as 1a, inserted alphabetically among the
other `tree-sitter-*` deps:

```toml
tree-sitter-<lang> = "=<version>"
```

### 2b: Export the generated module from `src/languages/mod.rs`

```rust
pub mod language_<lang>;
pub use language_<lang>::*;
```

Insert alphabetically.

### 2c: Add the language definition to `src/langs.rs`

Append a `mk_langs!` tuple alphabetically:

```rust
(
    <LangName>,
    "The `<LangName>` language",
    "<lowercase-lang>",
    <LangName>Code,
    <LangName>Parser,
    tree_sitter_<lang>,
    [<ext1>, <ext2>, ...],
    ["<emacs-mode>"]
),
```

The emacs mode is conventionally the lowercase language name (e.g.
`"rust"`, `"go"`, `"ruby"`). Use the file extensions parsed in 0
(without dots).

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
`checker.rs`, `getter.rs`, and the per-language metric impls must
terminate in an `Unknown` / `_` arm — never `unreachable!()`. The
next grammar bump WILL add an aliased kind_id (Step 3.5 / lesson 2)
that flips an `unreachable!()` arm reachable in production.

**Synthetic `Unit` root.** Tree-sitter can return a non-`Unit` root
(an `ERROR` node, or an inner `struct` / `function` / `namespace`
promoted to root) on partially-parseable input. `src/spaces.rs`
pushes a synthetic `Unit` whenever the parser's root kind is not the
language's canonical `Unit` kind, anchored to the full input range.
Confirm this fallback covers the new language — write a regression
test that parses a deliberately malformed fixture and asserts
`blank ≥ 0` and `kind == Unit` at the file level. See lesson 9 in
`lessons_learned.md` (issue #80, `dc09eb3`).

### 3a: `Checker` impl in `src/checker.rs`

Append an `impl Checker for <LangName>Code` block. Required methods:

- `is_comment` — match the grammar's comment node kind id.
- `is_useful_comment` — usually `false`.
- `is_func_space` — match top-level/source-file plus all
  function/method/closure/lambda kinds.
- `is_func` — function and method declarations.
- `is_closure` — anonymous function / lambda / func literal.
- `is_call` — call-expression kind.
- `is_non_arg` — punctuation kinds inside argument lists
  (`LPAREN`, `COMMA`, `RPAREN`, etc.).
- `is_string` — string-literal kinds.
- `is_else_if` — `#[inline(always)]`. **Do not blind-copy the Go
  form.** Before writing the predicate, parse a representative
  `if x { … } else if y { … } else { … }` snippet with the new
  grammar (the `tree-sitter` CLI, or a quick test fixture) and look
  at the AST shape. Lesson 10 in `lessons_learned.md` catalogues
  four distinct strategies among current languages:

  | Grammar model | Languages | Check |
  | --- | --- | --- |
  | `else_clause` wrapper | C++, Mozjs, JS, TS, TSX, Rust | `parent().kind_id() == ElseClause` |
  | `Else` keyword sibling | Java, C#, Kotlin | `prev_sibling().kind_id() == Else` |
  | Nested `if_statement` | Go | `parent().kind_id() == IfStatement` |
  | Dedicated clause node | Python, Perl, Lua, Bash, Tcl, PHP | kind match on the `else_if` / `elif` / `elsif` node |

  Pick the strategy whose model matches the grammar; an
  always-`false` stub is a bug, not a deferral. Issue #115
  (`013bff9`) caught exactly this in Java and C# after-the-fact —
  both languages had shipped the stub, and lesson 10 was written in
  response. Step 5 requires an `<lang>_else_if_chain` cognitive test
  that asserts the chain produces a *lower* score than the same
  number of `if` blocks nested inside one another — that test would
  have caught both #115 stubs the moment they were written.

- `is_primitive` — usually `false` unless the grammar emits a
  primitive-type kind (most don't).

### 3b: `Getter` impl in `src/getter.rs`

Append an `impl Getter for <LangName>Code` block. Required methods:

- `get_space_kind` — map function/method/closure kinds to
  `SpaceKind::Function`, the source-file kind to `SpaceKind::Unit`.
  Other kinds → `SpaceKind::Unknown`.
- `get_op_type` — Halstead operator/operand classification. **This is
  the largest piece of language-specific work.** Read the grammar's
  `node-types.json` (in the grammar crate's published source) or run
  the CLI against a representative file to see what kinds appear.
  Bucket every relevant kind into `HalsteadType::Operator`,
  `HalsteadType::Operand`, or `HalsteadType::Unknown`. Cover at
  minimum:
  - **Operators**: control-flow keywords, declaration keywords,
    punctuation acting structurally (`{`, `[`, `(`, `,`, `;`, `:`,
    `.`, `...`), arithmetic/comparison/logical/bitwise/assignment
    operators.
  - **Operands**: identifiers (plain, field, package, type, label,
    etc.), literals (int, float, string, char/rune, bool, nil, etc.).
- `get_operator!(<LangName>);` — final macro line.

**Naming-collision gotcha** (from Go): if the language enum's name
collides with a variant, alias it. Go's `Go::Go` (the `go` keyword)
clashed with `use Go::*` in pattern position; the fix was
`use Go as G;`. Detect the collision proactively after Step 1e:

```bash
rg "^\s*<LangName>\s*=" src/languages/language_<lang>.rs
```

If the search returns a hit, alias the import at the top of the
metric file with `use <LangName> as <ShortAlias>;`.

**No kind_id classified as both operator and operand.** Each
`get_op_type` arm must route a given kind_id to exactly one of
`HalsteadType::Operator`, `HalsteadType::Operand`, or
`HalsteadType::Unknown`. A copy-paste that lands the same kind in
two arms produces `n1` / `n2` counts that disagree with the `--ops`
output (TypeScript `String2` shipped this exact bug in `2248bcc`).
After classifying, the Halstead unit test added in Step 5 must
assert the load-bearing invariants from lesson 4: run both
`metrics()` and `operands_and_operators()` on the same input and
assert that `len(dedupe(ops.operators)) == n1` and
`len(dedupe(ops.operands)) == n2`.

### 3c: `Alterator` impl in `src/alterator.rs`

If the language has string/raw-string/char-literal node kinds whose
default text representation should be preserved verbatim (no whitespace
trimming, etc.), add an `impl Alterator for <LangName>Code` block that
matches those kinds and calls `Self::get_text_span(..., true)`. Use
the Go impl as the template:

```rust
impl Alterator for <LangName>Code {
    fn alterate(node: &Node, code: &[u8], span: bool, children: Vec<AstNode>) -> AstNode {
        match <LangName>::from(node.kind_id()) {
            <LangName>::InterpretedStringLiteral
            | <LangName>::RawStringLiteral
            | <LangName>::RuneLiteral => {
                let (text, span) = Self::get_text_span(node, code, span, true);
                AstNode::new(node.kind(), text, span, Vec::new())
            }
            _ => Self::get_default(node, code, span, children),
        }
    }
}
```

If the language has no special-case kinds, a bare `impl Alterator for
<LangName>Code {}` (using all defaults) is acceptable — see the Java
and Python impls.

### 3d: Compile-check again

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
LANG_FILE="src/languages/language_<lang>.rs"
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
on the underlying rule (`src/checker.rs`, `src/getter.rs`,
`src/alterator.rs`, `src/metrics/*.rs`, `src/spaces.rs`):

1. The variant is explicitly listed in the relevant arm (typically
   alongside its unsuffixed sibling:
   `Identifier | Identifier2 | Identifier3 => …`).
2. The variant is explicitly excluded with a one-line comment
   explaining why (e.g. "`HeredocBody` id 153 never surfaces in
   real parse trees per upstream").
3. The match has been rewritten to compare on `node.kind()` (the
   string), which sidesteps the entire numeric-variant problem.

Option 3 is the most forward-compatible choice (the *next* grammar
bump will add a new aliased ID that option 1 misses) and is
preferred unless the per-call hot path is provably
allocation-sensitive — see lesson 2.

**Coverage targets for this audit:**

- `checker.rs` — every `is_*` predicate that touches an aliasable
  kind: identifiers, member/field expressions, string literals,
  primitive types.
- `getter.rs` — `get_op_type` (Halstead classification) and any
  helper that branches on aliasable kinds.
- `alterator.rs` — string / raw-string / char-literal preservation
  arms. (Issue #119 was *only* in `alterator.rs` for JS/TS/TSX —
  `checker.rs` and `getter.rs` were already correct.)
- `metrics/cognitive.rs`, `metrics/cyclomatic.rs`, `metrics/abc.rs`,
  `metrics/npa.rs`, `metrics/loc.rs` — anywhere a per-language impl
  matches on a kind that the grammar aliases.

Re-run this audit after every grammar version bump in the same
crate's lifetime — lesson 2 explicitly warns the next aliased
variant appears on the next bump.

---

## Step 4: Register `<LangName>Code` with all 12 metric traits

The minimum-viable wiring is the no-op default for every metric, then
real impls for the metrics that have meaningful semantics for the
language. This mirrors how Go was landed: the parser shipped with all
defaults in commit `6ecc582`, and real impls followed in `e0701e1`.

### 4a: Default-impl all 12 metrics first

Append `<LangName>Code` to each `implement_metric_trait!(...)` macro
invocation in:

| File | Trait |
|------|-------|
| `src/metrics/abc.rs` | `Abc` |
| `src/metrics/cognitive.rs` | `Cognitive` |
| `src/metrics/cyclomatic.rs` | `Cyclomatic` |
| `src/metrics/exit.rs` | `Exit` |
| `src/metrics/halstead.rs` | `Halstead` |
| `src/metrics/loc.rs` | `Loc` |
| `src/metrics/mi.rs` | `Mi` |
| `src/metrics/nargs.rs` | `NArgs` |
| `src/metrics/nom.rs` | `Nom` |
| `src/metrics/npa.rs` | `Npa` |
| `src/metrics/npm.rs` | `Npm` |
| `src/metrics/wmc.rs` | `Wmc` |

Run `cargo check -p big-code-analysis` — it should now succeed.

### 4b: Replace defaults with real impls for the language's primary metrics

**Always** replace the default impl with a real one for these four:

- **Cyclomatic** (`src/metrics/cyclomatic.rs`): `+1` per `if`, `for`,
  `while`, `case` (each switch arm), `&&`, `||`,
  catch/except/rescue, ternary. **Default/wildcard arms do NOT add a
  branch** — this is consistent with the existing Java and Cpp impls
  in this repo, and was reaffirmed for Go in commit `e0701e1`. Match
  the language's actual node kinds, not generic names.
- **Exit** (`src/metrics/exit.rs`): `+1` per `return`/`yield` /
  language-specific exit. A multi-value return counts once.
- **Halstead** (`src/metrics/halstead.rs`): wire
  `compute_halstead::<Self>` through the existing
  `Getter::get_op_type` from step 3b — usually a one-method impl.
- **Loc** (`src/metrics/loc.rs`): see Step 4b-loc below — this is the
  largest and most subtle of the four primary metrics.

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
   src/languages/language_<lang>.rs
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
- **Cognitive** (`src/metrics/cognitive.rs`): substantial work; only
  add if the language has well-defined nesting semantics. OK to leave
  as default for now.
- **Abc**, **Mi**, **Npa**, **Npm**, **Wmc**: usually fine as
  defaults for procedural languages; add real impls only if the
  language has classes/methods/attributes with clear semantics.

### 4b-loc: Implementing the Loc metric

The Loc metric is more involved than the other primary metrics because
each of its five sub-counts has a distinct definition. Implement
`impl Loc for <LangName>Code` with a `compute` function that mirrors a
sibling language's structure (Java for curly-brace languages, Python
for indent-sensitive ones).

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
  in `src/metrics/loc.rs` and copy the pattern; do not re-invent.
- **CLOC** — for every comment node, count `end_row - start_row + 1`.
  Block comments span multiple lines and must be counted per-line, not
  per-node.
- **BLANK** — `SLOC - (PLOC ∪ CLOC line-set)`. The existing helpers
  in `loc.rs` compute this from the line-set unions; do not double-count
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

If `compute` ends up >150 lines, that is normal — Java's impl is
~200 lines. Do not extract speculative helpers to make it look
shorter; flow follows the AST.

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

For every metric with a real impl in 4b, add tests in the same file
under `mod tests`. Use the existing per-language test conventions
(see `cyclomatic.rs::tests` for examples — Go has 10 tests there).

### 5a: Coverage target — match Rust

The new language must reach **roughly the same per-metric test count
as the existing Rust coverage**. Rust is the reference because it
exercises every metric the project supports and its tests are the
gold standard for what "thoroughly tested" looks like.

Run this before starting to anchor the targets to current state (the
file paths match `src/metrics/*.rs`):

```bash
for f in src/metrics/*.rs; do
  printf "%-12s rust=%d\n" "$(basename "$f" .rs)" \
    "$(rg -c '^\s*fn rust_' "$f" 2>/dev/null || echo 0)"
done
```

At the time this skill was written, Rust's per-metric test counts
were:

| Metric | Rust tests | Notes / scope a `<lang>_*` test should cover |
|--------|-----------:|--------------------------------------------|
| `cognitive` | 9 | no-op file, simple function, sequence of same/different booleans, negated booleans, 1-level nesting, 2-level nesting, `break`/`continue`, complex `if let / else if / else`. |
| `cyclomatic` | 1 | nested-control-flow representative test. (Go currently overshoots Rust here at 10 — match Rust's 1, optionally add a couple more if the language has unusual constructs.) |
| `exit` | 2 | function with no exit, function with `?` / language equivalent of early-exit. |
| `halstead` | 1 | one comprehensive `<lang>_operators_and_operands` test pinning every Halstead field via `insta`. |
| `loc` | 15 | blank, no-zero-blank, cloc, lloc, then **at least one `<lang>_no_*_lloc` test for each gating decision** (Rust has: `field_expression`, `parenthesized_expression`, `array_expression`, `tuple_expression`, `unit_expression`, `call_function`, `macro_invocation`, `function_in_loop`, `function_in_if`, `function_in_return`, `closure_expression`). Mirror this — every node kind your `compute` excludes from LLOC needs a dedicated test that would fail if the gating is removed. |
| `nargs` | 5 | no functions/closures, single function, single closure, multiple functions, nested functions. |
| `nom` | 1 | one comprehensive `<lang>_nom` test exercising functions, methods, and closures together. |
| `abc`, `mi`, `npa`, `npm`, `wmc` | 0 | Rust has no per-language tests for these in `src/metrics/`. Skip per-language tests for these unless the new language has a real impl that diverges from defaults. |

**Floor:** total per-language tests across `src/metrics/` should be
**≥ 34** (Rust's current sum). Re-count Rust's tests at run-time
(the command above) — these numbers may have grown since.

**Ceiling:** do not pad with redundant tests. Each test must exercise
a distinct construct or gating decision; copy-pasted near-duplicates
fail `audit-tests` in Step 8c.

### 5b: Suggested per-metric test list

The 5a table is the source of truth for *count*; this list is the
shape each test should take. Where 5a says `cyclomatic = 1`, that
means one test minimum — Rust-parity. The Go module currently has 10
cyclomatic tests, which is *acceptable but not required*; add extras
only when the grammar has constructs the existing repo has not seen
(e.g. Go's `select`, Erlang's `receive`).

- **`cyclomatic`** — Rust's single test (`rust_1_level_nesting`) is
  the floor: a nested-control-flow test that exercises an `if` inside
  a loop. **Stop there if the grammar's branching is conventional**
  (`if`/`for`/`while`/`switch`/`&&`/`||`). Add at most one extra test
  per *novel* construct the grammar exposes — e.g. Go's `select`,
  Ruby's `rescue`, Python's exception handlers — plus, where
  applicable, a "non-branching feature that should NOT count" test
  (Go's `defer`/`go` test is the template). Do not pad the count by
  adding one test per ordinary branching keyword; that is what the
  single nested test already covers.
- **`exit`** — `<lang>_no_exit` (function with no return), and one
  test per language-specific exit path (e.g. Rust's `?`, Go's
  multi-value return, Python's `yield`/`raise`, Ruby's `next`/`break`
  out of a block if you treat them as exits — document the choice).
- **`halstead`** — one `<lang>_operators_and_operands` test using a
  source snippet that exercises every operator family classified in
  Step 3b (control-flow keywords, declaration keywords, structural
  punctuation, arithmetic, comparison, logical, bitwise, assignment).
  Pin **every** field via `insta::assert_json_snapshot!` — no loose
  inequalities (see 5c).
- **`loc`** — `<lang>_blank`, `<lang>_no_zero_blank`, `<lang>_cloc`,
  `<lang>_lloc`, plus one `<lang>_no_<kind>_lloc` per LLOC-gated node
  kind from Step 4b-loc. Aim for the same count as Rust (15).
- **`cognitive`** — `<lang>_no_cognitive`, `<lang>_simple_function`,
  `<lang>_sequence_same_booleans`, `<lang>_sequence_different_booleans`,
  `<lang>_not_booleans`, `<lang>_1_level_nesting`,
  `<lang>_2_level_nesting`, `<lang>_break_continue`,
  `<lang>_else_if_chain` (asserts an `if … else if … else if … else`
  chain produces a *lower* score than the same number of `if` blocks
  nested inside one another — this is the test that would have
  caught the #115 `is_else_if` stubs in Java and C# the moment they
  were written; see Step 3a and lesson 10), and one
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
assert the lesson-4 invariants
(`len(dedupe(ops.operators)) == n1`, `len(dedupe(ops.operands)) == n2`)
on the same input; existing Go and Bash Halstead tests show the
accessor pattern.

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
check_metrics::<<LangName>Parser>(
    "<source code>",
    "foo.<ext>",
    |metric| { /* assertions */ },
);
```

The source must be syntactically valid for the grammar, or
tree-sitter will produce an error tree and the metric counts will be
undefined.

### 5e: Coverage gate

Before moving to Step 6, run this comparison and verify the new
language's per-metric counts are at or above Rust's:

```bash
LANG="<lowercase-lang>"
for f in src/metrics/*.rs; do
  rust_n=$(rg -c '^\s*fn rust_' "$f" 2>/dev/null || echo 0)
  lang_n=$(rg -c "^\s*fn ${LANG}_" "$f" 2>/dev/null || echo 0)
  status="OK"
  [ "$lang_n" -lt "$rust_n" ] && status="UNDER (need $((rust_n - lang_n)) more)"
  printf "%-12s rust=%2d  %s=%2d  %s\n" \
    "$(basename "$f" .rs)" "$rust_n" "$LANG" "$lang_n" "$status"
done
```

If any metric reports `UNDER`, add tests until at parity. The only
acceptable shortfalls are:

1. metrics where the language has no real impl (left as default in
   Step 4b) — but flag this in the summary;
2. metrics where Rust has zero tests (`abc`, `mi`, `npa`, `npm`,
   `wmc` at the time of writing).

### 5f: Cross-language metric parity

Per-language snapshot tests pin each language's metric output
against its own history. They cannot detect that two languages
disagree about the same construct (lesson 11). Rust counted wildcard
`_ =>` while C-family did not count `default:` (#106, `a54b073`);
Bash double-counted `case…esac` container plus arms (#107,
`e668f14`) — both bugs that survived per-language snapshot suites
for years.

Before declaring the language done, write a small fixture for each
of the constructs below in the new language AND in two existing
languages (Rust + Java is a good baseline), then assert the
per-metric sums agree:

- 2-arm conditional with a `default` / `else` / wildcard arm:
  cyclomatic and cognitive must agree across all three impls
  (modulo documented per-language quirks).
- An `if … else if … else` chain of length 3: cognitive must agree.
- A loop body with one early-exit (`return` / `break`): exit must
  agree.
- A function with three formal parameters: `nargs` must agree.

A new shared test (`cyclomatic_cross_language_parity_minimal` /
`cognitive_cross_language_parity_minimal`) is the right home.
Per-language snapshot tests stay in their respective `mod tests`
blocks; the parity tests live alongside them and fail the moment a
language drifts. Any *intentional* per-language divergence (e.g.
Rust's standard-CCN wildcard-arm exception) is documented in the
parity test's comment and asserted with `!=` to keep the
divergence visible.

---

## Step 6: Documentation

### 6a: Add the language to the supported-languages list

Edit `big-code-analysis-book/src/languages.md` and insert the new
language alphabetically:

```markdown
- [x] <LangName>
```

### 6b: Update CHANGELOG

`CHANGELOG.md` is present at the repo root and used. A new language
warrants its own entry — do not consolidate into a batch-fix line.
Append under `Added`:

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
`cargo +nightly udeps`, `make enums-check` (the workspace-excluded
`enums` crate's lint gate — a new language modifies it, see
lesson 15), `make snapshot-anchors` (enforces 5c, see lesson 6),
and the markdown / TOML / shell / Makefile lint families in one
parallel pass.

```bash
make pre-commit
```

`make ci` runs the same checks without auto-fix (mirrors CI
behaviour). If GNU Make 4 or any optional tool (`taplo`,
`markdownlint-cli2`, `shellcheck`, `shfmt`, `checkmake`) is
unavailable, fall back to the raw equivalents:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
RUSTFLAGS="-D warnings" cargo clippy --manifest-path enums/Cargo.toml \
  --all-targets --locked -- -D warnings
./check-snapshot-anchors.py
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

1. `feat(languages): add <LangName> language support` — wiring +
   default metric impls (Steps 1–4a).
2. `feat(<lang>): implement metric traits properly for <LangName>Code`
   — real impls (Step 4b).
3. `test(<lang>): add metric test matrix per issue #N spec` — tests
   (Step 5).
4. `docs(book): add <LangName> to supported languages list` —
   docs (Step 6).
5. Any cleanup commits from Step 8.

Before exiting, print a one-screen summary:

```
Added <LangName> language support.

Files changed:
  Cargo.toml
  enums/Cargo.toml
  enums/src/languages.rs
  enums/src/macros.rs
  src/langs.rs
  src/languages/mod.rs
  src/languages/language_<lang>.rs   (generated)
  src/checker.rs
  src/getter.rs
  src/alterator.rs                   (if applicable)
  src/metrics/{abc,cognitive,cyclomatic,exit,halstead,loc,mi,nargs,nom,npa,npm,wmc}.rs
  big-code-analysis-book/src/languages.md
  *.snap                             (new insta snapshots)

Validation: cargo fmt / clippy / test / insta — all passing.
Code-quality skills run: simplify-rust, rust-optimize, audit-tests.

NOT committed. Review `git status` and `git diff` and commit
manually.
```

**Heads up: mutation testing.** Quarterly mutation testing
(`.github/workflows/mutation-test.yml`, see
`docs/development/mutation_testing.md`) runs against
`src/metrics/`, `src/checker.rs`, and `src/getter.rs`. Within one
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
