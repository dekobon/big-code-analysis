# Lessons Learned

Hard-won principles from debugging real bugs in this workspace. Each
entry is grounded in specific issues or commits and is written to be
re-applicable to future work — not a postmortem of one incident.

New entries are appended at the end with the next sequential number.
Other skills under `.claude/skills/` may reference lessons by number;
treat renumbering as a breaking change.

---

## 1. Trait implementations are not metric implementations

Routing a new language through `implement_metric_trait!(...)` (the
default no-op macro in `src/macros.rs`) satisfies the type system but
silently emits zero for every input. There is no compile-time signal,
no runtime warning, and the test suite passes because zero is a valid
metric value, not a sentinel for "unimplemented." Default impls exist
for genuinely-inapplicable cases (`PreprocCode`, `CcommentCode`, or
`wmc`/`npa`/`npm` for languages without classes), so the no-op path
must stay; it just cannot be the default for every newly-added
language.

**Bash Cognitive / Exit / ABC silently zero for every script** (#71,
`d2be869`). `BashCode` was wired through the no-op macro for
Cognitive, NEXITS, and ABC. Every Bash file in every report read `0`
on those columns regardless of how complex the script was; downstream
aggregations and Maintainability Index ranked Bash as artificially
clean. The fix required real implementations, follow-up refactors,
and a breaking signature change to `Exit::compute` (it now takes
`code: &[u8]` because Bash parses `return` and `exit` as ordinary
`Bash::Command` nodes that must be discriminated by source text, not
by node kind).

**Lesson:** When adding a language, audit which metrics genuinely do
not apply (`wmc`/`npa`/`npm` for non-class languages, `nargs` for
languages without formal parameters) and which were merely deferred.
A new entry in `implement_metric_trait!(Cognitive, ...)` or any of the
seven metric-trait no-op blocks must be a deliberate decision with a
one-line justification, not a leftover from scaffolding. Add a
positive test that exercises non-trivial control flow and asserts a
non-zero metric value before declaring the language done.

---

## 2. Tree-sitter aliases one rule across many kind_ids — match every variant

When the same grammar rule (`primitive_type`, `identifier`,
`member_expression`, `heredoc_body`) appears in different positions
in a tree-sitter grammar, the generator emits N distinct kind_ids
all mapping to the same `node.kind()` string. The unsuffixed variant
(`PrimitiveType`) and the suffixed variants (`PrimitiveType2`,
`PrimitiveType3`, …) are different `u16`s. Code that matches only
the unsuffixed variant in `getter.rs` / `checker.rs` silently drops
the rest — it compiles, runs, and returns wrong numbers. The
asymmetry is invisible until a snapshot test happens to exercise the
specific keyword that maps to the aliased ID, or until a downstream
metric (Halstead `n1`, primitive-type detection) goes inexplicably
low.

**Rust `PrimitiveType2`-`PrimitiveType17` not matched in `is_primitive`
or Halstead** (#40, `274eb74`). The Rust grammar has 17 numeric
variants for `primitive_type` (one per primitive keyword position).
Only id 32 was matched; ids 33-48 fell through to `Unknown`.

**Java primitive types missing from Halstead operators** (#36,
`4e55756`). 6 of 9 (`byte`, `short`, `long`, `char`, `double`,
`boolean`) were unclassified.

**JS-family aliased operand variants** (#50, `c744809`).
`MemberExpression3`, `MemberExpression4`, and `Identifier2` were
unclassified across JavaScript, Mozjs, TypeScript, and TSX.

**Go `BlankIdentifier` and aliased `Identifier2`/`Identifier3`** (#49,
`e884abc`).

**Bash `HeredocBody2` not matched in `is_string`** (#44, `e487a25`).
The grammar exposes four body-related symbols; only `HeredocBody2`
(id 218) actually surfaces in real parse trees, so the others are
intentionally omitted — but the originally-implemented `HeredocBody`
(id 153) was the *unused* one.

**C# aliased `InvocationExpression` / `ParenthesizedExpression` /
`PrefixUnaryExpression` / `VariableDeclaration` / `VariableDeclarator`
not matched** (#94, `866c38a`). The C# tree-sitter grammar emits 2-3
numbered variants for each of those rules; the initial language-support
commit matched only the unsuffixed variant in `checker.rs`,
`metrics/abc.rs`, `metrics/cognitive.rs`, and `metrics/npa.rs`. Method
invocations and attribute walks were silently undercounted; cognitive
`!` detection and ABC parenthesised-condition descent were dropped
outright. Notable because C#
shipped *after* this lesson was already documented — the bug class
applies just as much to a fresh language addition as it does to a
grammar bump.

**JS/TS/TSX `String2` (and TSX `String3`) not matched in alterator**
(#119, `9c43253`). The `MozjsCode` alterator correctly flattened both
`String | String2`, but the three forked JS-family alterators matched
only `String`. TSX had a third alias (`String3`) for JSX attribute
strings that even the issue description missed — discovered only by
enumerating all variants mapping to `"string"` in the language enum.
The bug class extends beyond `getter.rs` / `checker.rs` to any match
on a grammar rule: `alterator.rs`, `metrics/*.rs`, and `spaces.rs`
are equally susceptible.

**Lesson:** After bumping any tree-sitter grammar pin in `Cargo.toml`,
run `rg 'Lang::([A-Za-z]+)\b' src/getter.rs src/checker.rs
src/alterator.rs src/spaces.rs src/metrics/` against
the regenerated `language_<lang>.rs` and confirm every numeric-suffix
variant of every matched rule is either explicitly listed or
explicitly excluded with a comment. Mutation tests (or simple
positive tests covering each token form) pin coverage and catch the
next aliased variant the moment it appears. When in doubt, prefer
matching by `node.kind()` string (one comparison) over enumerating
17 enum variants — pay the small runtime cost for forward
compatibility.

---

## 3. Per-language modules mirror each other — fix the bug in every sibling

The four JavaScript-family modules (`language_javascript.rs`,
`language_mozjs.rs`, `language_typescript.rs`, `language_tsx.rs`)
are deliberately structural twins — Mozjs was the original and the
others were forked from it. A defect in the original almost always
exists in 2-4 of the siblings. AGENTS.md captures this principle in
the abstract; the concrete recurrence pattern is worth its own
entry. The same applies to any future fork (e.g., a TypeScript
variant or a JSX dialect).

**JS/TS/TSX `get_func_space_name` returns wrong enum** (#37,
`64c80b8`). All three modules referenced `Mozjs::*` in their
`get_func_space_name` match because each was copy-pasted from
Mozjs and the imports were never updated. Anonymous functions in
JS, TypeScript, and TSX silently rendered as `<anonymous>` in the
wrong language's namespace. One-line fix per module — three files
modified, one bug class.

**`is_else_if` checked `IfStatement` instead of `ElseClause`** (#38,
`6fd6f79`). The parent of an `IfStatement` inside `else if` is an
`ElseClause`, not another `IfStatement`. Two of the four siblings
(JavaScript and TSX) had the wrong check while Mozjs and TypeScript
already had it right — a reminder that "fork-of-Mozjs" is the
common case but not universal: always grep all four, do not assume
the bug is everywhere or nowhere.

**Modern operators `=>`, `...`, `?.` missing from all 4 JS-family
Halstead classifications** (#42, `b0e27f2`). Same omission, four
modules.

**`typeof` / `instanceof` / `void` misclassified as operands** (#45,
`18f6c48`). Same misclassification, four modules.

**`Do` keyword not counted as Halstead operator** (#35,
`68db037`). Same omission, four modules.

**Lesson:** Before claiming any fix in a JS-family module is
complete, grep the other three for the same identifier and apply
the same change. The check is mechanical: `rg
'<symbol_or_match_arm>' src/languages/language_{javascript,mozjs,typescript,tsx}.rs
src/{getter,checker}.rs`. Land all sibling fixes in one commit so
the diff makes the pattern visible to reviewers — splitting them
across PRs hides the symmetry. The same discipline applies to any
future trio (e.g., Java/Kotlin, C/C++/Mozcpp).

---

## 4. Halstead `n1`/`n2` and `--ops` come from different stores — keep them in sync

`HalsteadMaps::operators` is a `HashMap<u16, u64>` keyed by
`node.kind_id()`. The `--ops` output is built from a parallel
text-keyed structure plus the `primitive_types` `HashSet<String>`.
Three independent failure modes have produced visibly disagreeing
counts between Halstead `n1` (`self.operators.len()`) and the
`--ops` operator list:

**Many tokens collapse to one kind_id** (#31, `2b1083b`). For
grammars that map several keywords to one kind_id
(`Cpp::PrimitiveType` covers `int`/`float`/`double`/`char`/`void`/
`unsigned`; `Rust::PrimitiveType` covers `i32`/`u8`/`f64`/`bool`/
`usize`; `Typescript::PredefinedType` covers `string`/`number`/
`boolean`), N textually-distinct operators collapse to one HashMap
entry. `n1` undercounts by `N - 1` while `--ops` correctly lists
all N. The fix stored primitive-type operators by source text.

**Parent scopes accumulate without recomputing** (#32, `b12d899`).
The finalize pass merged children into parents but did not
recompute parent ops afterward, double-counting at every nesting
level.

**Same kind_id added to both operator and operand maps** (`2248bcc`).
TypeScript `String2` (the `string` type keyword) was classified as
an operator (correct) and also as an operand (wrong) — a
single-token double-insertion, not a kind_id collision, but the
visible symptom is the same: `--ops` and the metric counts
disagree.

**Lesson:** Treat `len(dedupe(ops.operators)) == n1` and
`len(dedupe(ops.operands)) == n2` as a load-bearing invariant.
Whenever you change Halstead classification, add a `kind_id` to
`is_primitive`, or touch the finalize / parent-merge logic, add a
regression test that runs both `metrics()` and
`operands_and_operators()` on the same input and asserts the
invariant. When auditing a new language, also check that no
kind_id is classified as *both* operator and operand — the
`HalsteadType` enum should be exhaustive, but the routing in
`getter.rs` is not, and a copy-paste can land the same kind_id in
two arms.

---

## 5. Library code must not panic on reachable error paths

`big-code-analysis` is a published crate; its callers (the CLI, the
web service, and any third-party user) treat panics as crashes they
cannot recover from. AGENTS.md already bans `unwrap` / `expect` /
`panic!` / `assert!` in non-test code, but the rule is easy to
read as a style preference. The substance is that Rust makes these
ergonomic enough to slip past review on paths that turn out to be
reachable — sort comparators on user data, `unreachable!()` arms in
match expressions, lock acquisition in long-running services.

**`partial_cmp().unwrap()` reached by NaN metric values**
(`011c556a`). Two sort functions in the markdown report
(`sort_by_metric_desc`, `sort_by_metric_asc`) used
`partial_cmp(...).unwrap()` on `f64` metric fields. A higher-level
test (`nan_safe_sort_does_not_panic`) asserted the report didn't
panic, but a `> 0.0` guard upstream filtered NaN before it reached
the sort, so the test passed for the wrong reason. The audit-tests
skill flagged this; the fix added direct unit tests that pass NaN
into the sort functions and replaced `unwrap` with `f64::total_cmp`.

**`unreachable!()` arms become reachable on grammar bumps.** When
tree-sitter bumps emit new aliased kind_ids (see lesson 2), match
expressions in `getter.rs` / `checker.rs` that fall through to an
`Unknown` arm are safe; `unreachable!()` would crash. The same
applies to `MetricsFormat` matches in the CLI when a new format
variant is added in one place and forgotten in another.

**Lesson:** Treat `unwrap` / `expect` / `panic!` / `assert!` /
`unreachable!()` outside `#[cfg(test)]` as a code smell that needs a
one-line justification — preferably the invariant that makes it
provably unreachable, in the `expect` message itself. Before using
one, ask: "can this branch be triggered by source the user supplies,
a metric value the parser produces (NaN, infinity, zero), a grammar
node the next tree-sitter version emits, or concurrent state in the
web service?" If yes, propagate via `Result` or `Option`, or pick a
total-order primitive (`f64::total_cmp` over `partial_cmp`). Tests
that exercise the panic path must call the function under test
directly with the panicking input — wrapper-level tests almost
always have an upstream filter that masks the bug.

---

## 6. Snapshot tests pin behaviour, not correctness

`insta` snapshot tests record whatever the code emits at the moment
they were written. If the code is wrong, the snapshot freezes the
wrong value, and `cargo insta test --accept` after a metric or
grammar change rubber-stamps the new wrong value with no human
verification. This file already documents bug classes (lessons 2
and 4) where the metric was wrong for years and the test suite
agreed with it.

**JS-family Halstead snapshots agreed with operators that were
silently misclassified.** Issues #35, #42, #45, and #50 each
involved an operator that was wrong in 2-4 JS-family modules. The
snapshot tests passed because they pinned the buggy `n1` / `n2`
values, not values derived from the Halstead specification. The
bugs survived until someone audited the operator list against
language documentation, not against the snapshots.

**Tree-sitter grammar bumps cause hundreds of snapshots to shift.**
AGENTS.md documents this: after a grammar version bump, `cargo
insta test --accept` is the right tool, but only after verifying
the diff pattern is "metric-value-only with no structural
changes." Accepting blindly converts any newly-introduced metric
bug into a frozen snapshot.

**Lesson:** When writing or accepting a snapshot, ask: "if the code
were wrong in a plausible way, would this snapshot still pass?" If
yes, derive at least one assertion from an external source — the
metric specification, a hand-computed value on a small fixture, or
the reference implementation in another language module — not from
the current code's output. For grammar bumps, run `cargo insta test
--accept` per file only after spot-checking that the diff is metric
values shifting in a direction consistent with the grammar change,
not structural changes that hide a regression. Keep at least one
hand-derived test per metric per language as an external anchor;
snapshots are scaffolding around it, not a substitute.

---

## 7. Test infrastructure deserves the rigor of production code

The `audit-tests` skill exists because tests in this codebase have
historically passed for reasons unrelated to what they claimed to
verify. A green suite means nothing if assertions are weak,
helpers shadow real tests, or the input never reaches the code
under test. This is distinct from lesson 6: lesson 6 is about the
*provenance* of the asserted value; this lesson is about the
*structure* of the test itself.

**Wrapper-level tests masked by upstream filters** (`011c556a`).
`nan_safe_sort_does_not_panic` called `generate_report` end-to-end
with NaN inputs; a `> 0.0` filter removed NaN before the sort ever
saw it, so the test passed regardless of whether the sort was
NaN-safe. The fix added unit tests that call the sort functions
directly. General pattern: tests that exercise behavior through a
high-level entry point can pass for the wrong reason whenever any
intermediate stage filters, normalizes, or short-circuits the
input. Always pair end-to-end tests with direct unit tests on the
function whose contract is being verified.

**Section-presence tests with no value assertions** (`df84dd27`).
`wmc_section_present_with_class_summaries`,
`nexits_section_present`, and `abc_section_present` originally
asserted only that a markdown header rendered. A bug emitting wrong
WMC values, wrong NEXITS counts, or zero ABC magnitudes would have
preserved the header and passed the test. The strengthened tests
now assert exact metric values for each section.

**Strip-prefix test that asserted nothing observable**
(`011c556a`). `markdown_strip_prefix_accepted` originally asserted
only that the function returned without error; it never checked the
output's path strings. Mutation testing confirmed a no-op
implementation passed. The fix renamed it to
`markdown_strip_prefix_removes_path_prefix` and added two checks:
the stripped path must appear, and the full path must not.

**Lesson:** Hold tests to the same standard as production: every
test asserts a specific value or specific failure, never just
`is_ok()` or "the section rendered." When fixing a bug, write the
test against the function whose contract is wrong, not against a
wrapper that may filter the bug-triggering input. Run mutation
testing or audit-tests on hot regions periodically — if a no-op
implementation could pass, the test does not test what it claims.

---

## 8. Integration snapshot drift hides in the submodule, not the parent

The integration test corpus lives in the `big-code-analysis-output`
submodule (`tests/repositories/big-code-analysis-output/`). When a
behaviour-changing fix lands in the parent — a cognitive under-count
correction, a Halstead operator reclassification, an alterator-rule
change — the integration runs (`deepspeech_test`, `pdf_js_test`,
`serde_test`) generate `.snap.new` files **inside the submodule's
working tree**. The parent's `cargo test` exits non-zero until those
accepts are committed and pushed in the submodule, and the submodule
pointer in the parent is bumped to record the new SHA. Skipping any
of those three steps leaves the fix half-landed: a future fresh
clone hits an unfetchable submodule SHA or stale snapshots that
block CI.

**`ed8adb6` lost 4 of its 69 cognitive snapshot accepts.** The
sibling boolean-sequence fix (`fix(cognitive): correct sibling
boolean-sequence detection`) was committed on parent `main`
together with a submodule pointer bump to `4c2a17c2`, which
contained all 69 accepts. Later, `dekobon/big-code-analysis-output`
was force-pushed onto a chain that rebased away the cognitive
accepts and kept only Halstead-NaN/Inf accepts (current submodule
HEAD `8bb237d`). The parent pointer still referenced `4c2a17c2`,
which no longer existed on the remote — submodule fetch failed
outright on a fresh clone. After repointing to `8bb237d`, four
snapshots that had been correctly accepted in `4c2a17c2` were
missing: `farcreate.cc`, `farcompilestrings.cc`, `viewer.js`, and
`build.rs`. The fix itself was not broken; the snapshots that proved
it were stranded by submodule history rewrites.

**Lesson:** A metric, AST-traversal, or alterator-rule fix is not
done until (1) `cargo test --workspace --all-features` exits clean
from a fresh working tree, (2) any `.snap.new` files generated under
`tests/repositories/big-code-analysis-output/` have been reviewed
and committed inside the submodule, (3) those submodule commits
have been pushed to its remote, and (4) the parent records the new
submodule SHA in the same commit as the parent-side fix — never as
a follow-up. Treat the submodule pointer bump as part of the fix.
After any rebase, force-push, or long-running batch fix, re-run
integration tests before declaring done; the submodule history
is force-pushed often enough that previously accepted snapshots
cannot be assumed to survive.

---

## 9. The grammar's root may not be `Unit` — push a synthetic wrapper

Tree-sitter grammars normally return a `translation_unit` /
`source_file` / `program` node at the root, and the metric collector
treats that node's span as the file-level `FuncSpace`. When the input
contains constructs the grammar cannot fully parse, the parser can
instead return an `ERROR` root or promote an inner declaration
(struct, function, namespace) to the root position. Code that adopts
the root node's span as the file's `FuncSpace` then reports the span
of that inner declaration as the file's LOC, while child traversal
still aggregates `ploc` from the entire file — producing impossible
values that violate `blank = sloc − ploc − only_comment_lines ≥ 0`.

**`tree-sitter-mozcpp` promotes inner declarations on partially
unparseable C/C++** (#80, `5514714`). Four DeepSpeech files exhibited
nonsense LOC: `model.hh` (KenLM) reported `kind=namespace, sloc=1,
ploc=55, blank=−109`, and both Cython-generated `pywrapfst.cc` files
reported a `struct` or `function` root with `blank` in the tens of
thousands negative — those bad values had been frozen into snapshots
long enough to read as background noise in every DeepSpeech run.
`getopt_win.h` (`kind=struct, sloc=1, ploc=351, blank=−489`) had been
quietly *excluded* from the snapshot test for the same root cause; the
fix re-includes it. The fix pushes a synthetic `Unit` space at the bottom
of the state stack whenever the grammar's root kind is not `Unit`,
anchored to the parser's full input range; the misidentified
declaration becomes a subspace, and top-level metrics restore their
invariants.

**Lesson:** Never trust the root node's `kind()` to be the language's
canonical translation-unit kind. Treat "real-world C/C++/whatever
sometimes won't parse cleanly, and tree-sitter has its own ideas about
what to promote when that happens" as a load-bearing assumption. When
adding a new language or auditing an existing one, verify that the
file-level `FuncSpace` is anchored to the parser's full input range
and has the language's `Unit` kind, not the kind of whatever the
parser happened to return. Add a regression test that asserts
`blank ≥ 0` for every fixture in the corpus — the invariant is cheap
to check and catches this entire class of bug, plus arithmetic errors
in the LOC computation itself.

---

## 10. Same language construct, different AST shape — detection must be grammar-aware

A single language construct — `else if`, ternary expression, lambda,
string literal — can have fundamentally different AST representations
across tree-sitter grammars. Code that works for one grammar family
(e.g., detecting `else if` by checking whether the parent is an
`ElseClause`) silently fails for another family that models the same
construct differently (e.g., `else` as a keyword sibling with no
wrapping clause node). Unlike the aliased-variant problem (lesson 2),
where the grammar generates multiple kind_ids for the same rule, this
is a structural divergence: the node relationships themselves differ.

**Java and C# `is_else_if` always returned `false`** (#115,
`df9b10c`). The C++/JS-family grammars wrap `else if` in an
`ElseClause` parent node, so `is_else_if` checks
`parent().kind_id() == ElseClause`. Java and C# grammars emit `else`
as a bare keyword token preceding a nested `if_statement` — no
wrapping node. The initial implementations returned `false`
unconditionally, causing every `else if` to receive a nesting
increment instead of a flat +1. Cognitive complexity was
systematically inflated; the error grew linearly with chain length
and exponentially with nesting depth (each false nesting
increment inflated the penalty for all nested constructs inside
the chain). The fix adopted Kotlin's strategy:
check `previous_sibling().kind_id() == Else`. A post-fix audit of
all 16 `is_else_if` implementations catalogued four distinct detection
strategies across the supported languages:

| Grammar model | Languages | Check |
| --- | --- | --- |
| `else_clause` wrapper | C++, Mozjs, JS, TS, TSX, Rust | `parent()` |
| `Else` keyword sibling | Java, C#, Kotlin | `prev_sibling()` |
| Nested `if_statement` | Go | `parent()` |
| Dedicated clause node | Python, Perl, Lua, Bash, Tcl, PHP | kind match |

**Lesson:** When implementing a semantic check that depends on AST
structure (not just node kind), do not assume all grammars use the same
structural model. Before writing the implementation, examine the
grammar's `node-types.json` or parse a representative snippet to
confirm how the construct is actually represented. When a stub
`is_else_if`-style function returns a constant, treat it as a
to-do item, not a finished implementation — add a test that would fail
if the function were a no-op (e.g., an `else if` chain that must
produce a lower cognitive score than the same chain with independent
`if` blocks). After fixing one grammar family, audit all others for
the same stub pattern.

---

## 11. The same metric across languages must agree on the same logical construct

Each language's metric implementation under `src/metrics/` is written
against that language's grammar, not against a shared specification. When
two grammars represent the same logical construct differently (a
`switch`/`match` with a fallback arm; a `case…esac` that wraps its arms in
a parent node), the per-language `Cyclomatic` / `Cognitive` / `Halstead`
impl can quietly diverge — each language's snapshot tests still pass,
because every snapshot was written against that language's own (wrong)
output. The drift is invisible until someone compares CCN sums across
languages on equivalent code. Lesson 6 covers per-language snapshot
provenance; this lesson covers *cross-language* metric agreement, which
even an externally-anchored single-language test cannot catch.

**Rust counts wildcard `_ =>` while C-family does not count `default:`**
(#106, `a54b073`). `impl Cyclomatic for RustCode` matched `MatchArm |
MatchArm2` for every arm of a `match`, including the wildcard `_ =>`. The
equivalent `default:` clause in C / C++ / C# / Java / JS / TS / TSX / Mozjs
/ PHP is intentionally not counted (those impls match the `Case` node,
which the wildcard arm does not produce). Two-branch
`match { 1 => …, _ => … }` reported standard CCN +2 in Rust, while the
equivalent `switch { case 1: …; default: … }` reported +1 in C. The
recently-added modified-CCN variant (`16cd610`) collapsed all arms to one
container decision, which papered over the asymmetry but left standard
CCN divergent.

**Bash double-counts `case…esac` container plus arms** (#107, `e668f14`).
`impl Cyclomatic for BashCode` matched `Bash::CaseStatement` *and*
`Bash::CaseItem | Bash::CaseItem2`, incrementing once for the wrapper
node and once per arm. C / Java / C# / JavaScript / TypeScript count
only arms — the `switch` / `case` / `match` container is silent. A Bash
function with a 3-arm `case` reported standard CCN 6 against an
equivalent C `switch`'s 5. Same paper-over via `16cd610`'s modified
variant; same residual asymmetry in standard CCN.

**Lesson:** When adding or touching a metric implementation, write the
fixture in *every* affected language and assert the metrics agree on
logically equivalent code (modulo documented exceptions). One fixture
file per language under a shared test such as
`cyclomatic_cross_language_parity` is enough; the test fails the moment a
language drifts. Per-language snapshot tests pin behaviour against that
language's own history — they cannot detect that two languages disagree
about the same construct. Whenever a "modified" or "alternative"
metric variant is introduced to mask a per-language quirk, audit the
standard variant as well: the variant probably exists because the
standard variant is wrong, and the standard one is what most consumers
read.

---

## 12. tree-sitter `Node::children(cursor)` resets the cursor to `self`

`tree_sitter::Node::children(cursor)` calls `cursor.reset(self)` before
iterating, so the `TreeCursor` argument's prior position is silently
discarded. Code that constructs a cursor from one node and passes it to
another node's `children()` call iterates the second node's children, not
the first's. The compiler accepts this — `TreeCursor` has no compile-time
binding to a specific node — and the function quietly does the wrong
thing. Lesson 2 covers a related tree-sitter surprise (aliased `kind_id`s);
this lesson covers cursor scoping, a distinct gotcha class.

**`Node::has_sibling` was structurally identical to `Node::is_child`**
(#127, `7a0d4ac`). The implementation was

```rust
self.0.parent().is_some_and(|parent| {
    self.0.children(&mut parent.walk())          // parent.walk() ignored
        .any(|child| child.kind_id() == id)
})
```

The intent was "walk the parent's children," but `self.0.children(...)`
resets the cursor to `self.0` and iterates `self.0`'s children. The
single call site `check_if_arrow_func!` in `src/checker.rs:48` invoked
`has_sibling(PropertyIdentifier)` to detect `{ foo: x => x }`-style
shorthand-method arrow functions; because `PropertyIdentifier` is never
a child of `ArrowFunction`, the check returned `false` unconditionally.
The bug was masked because `count_specific_ancestors` caught the common
case via a different traversal — the dead branch only mattered for inputs
where the ancestor walk exited early. The fix calls `parent.children(...)`
directly, dropping the misleading cursor argument entirely.

**Lesson:** The cursor passed to a tree-sitter iteration method does not
determine its scope — the node the method is called on does. Whenever a
helper takes a `TreeCursor` argument and calls `node.children(cursor)`
or `node.named_children(cursor)` on a node that isn't `cursor`'s root,
the cursor argument is dead weight. Prefer calling iteration methods
directly on the node you want to traverse (`parent.children(&mut
parent.walk())`) and use the parameter only when you genuinely need to
share an allocated cursor across siblings. When reviewing helpers like
`has_sibling`, write a unit test that distinguishes "iterates self's
children" from "iterates parent's children" with a fixture where the
two would disagree — without that test, the bug is invisible.

---

## 13. `tokio::task::spawn_blocking` is uncancellable

`tokio::time::timeout(deadline, spawn_blocking_handle).await` resolves
when the deadline fires, but the underlying blocking task continues
running on Tokio's blocking thread pool until its closure returns.
Dropping the `JoinHandle` (or any future wrapping it) does **not**
cancel the task — the Tokio docs state this explicitly, and `actix-web`'s
`web::block` inherits the behavior. A request handler that pairs a
semaphore (to bound concurrency) with `tokio::time::timeout` (to bound
latency) bounds neither the blocking pool nor the actual CPU time spent
on a single request: timed-out tasks release the permit but keep their
thread-pool slot, and a sustained rate of pathological input fills the
512-thread default pool, after which all `spawn_blocking` callers queue
indefinitely.

**Pathological source code DoS in `big-code-analysis-web`** (#110,
`94c8141`, configurability follow-up `b808180`). `run_parse` in
`big-code-analysis-web/src/web/server.rs` acquired a semaphore permit,
called `web::block(parse_fn)`, and wrapped the join handle in
`tokio::time::timeout`. When the timeout fired, the handler returned a
504 to the client and dropped the permit, but the parse closure kept
running. A modest sustained rate of inputs that exceed the timeout
(e.g., ~18 req/s at a 30s deadline) saturates the 512-thread default
blocking pool; after that, every new request — including healthy
ones — queues until an orphaned task happens to finish. Permit limits
on concurrent requests do nothing because the bottleneck is the
thread pool, not the permit count. The fix added an orphan-task
counter that 503s new requests once the threshold (configurable via
`BCA_MAX_ORPHANED_TASKS`) is exceeded, giving the pool time to drain.

**Lesson:** `tokio::time::timeout` does not cancel `spawn_blocking`
work — it cancels the *await* of the join handle and nothing else.
Anywhere `spawn_blocking` (or `actix_web::web::block`) runs against
user-controlled input with a non-trivial worst-case runtime, three
things must hold: (1) the work itself must check for cancellation
periodically and exit early, OR (2) the server must explicitly track
orphaned tasks and reject new work once the orphan count or a
proxy-for-orphans (active threads minus active permits) crosses a
threshold, OR (3) the input must be size-bounded such that the
worst-case runtime is a small multiple of the timeout. A semaphore
alone is not sufficient. When adding a new blocking endpoint, write a
test that submits requests at a rate slightly above
`blocking_pool_size / timeout_seconds` per second and asserts the
server rejects rather than queues. The `tokio::time::timeout` +
`spawn_blocking` combination *looks* defensive in code review precisely
because each piece is correct in isolation; the gap is at the seam.

---
