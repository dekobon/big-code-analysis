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

**Audit (#188):** the full default-impl matrix is now documented at
each `implement_metric_trait!` invocation site in `src/macros.rs`
callers (`src/metrics/abc.rs`, `cognitive.rs`, `npa.rs`, `npm.rs`,
`wmc.rs`, `mi.rs`, `loc.rs`, `cyclomatic.rs`, `exit.rs`,
`halstead.rs`). Each (language, metric) pair is classified as either
a *real default* (the language has no construct the metric measures
— a comment captures the reason) or a *placeholder* (the language
HAS the construct but no impl exists — a comment references the
follow-up issue, and a smoke test under `mod tests` pins the current
0 value so the assertion fires when the real impl lands). Mi turned
out to be a non-issue: its `[Trait]` arm inherits the trait's
default `compute` method, which works for every language (see
issue #207). Note the bracketed-trait arm (`[Tokens]`, `[Nom]`, `[NArgs]`,
`[Mi]`) is *not* a no-op; only the named-trait arms (`Abc`,
`Cognitive`, `Halstead`, …) emit silent-zero bodies.

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
not matched** (#94, `f042659`). The C# tree-sitter grammar emits 2-3
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
(#119, `fbf047d`). The `MozjsCode` alterator correctly flattened both
`String | String2`, but the three forked JS-family alterators matched
only `String`. TSX had a third alias (`String3`) for JSX attribute
strings that even the issue description missed — discovered only by
enumerating all variants mapping to `"string"` in the language enum.
The bug class extends beyond `getter.rs` / `checker.rs` to any match
on a grammar rule: `alterator.rs`, `metrics/*.rs`, and `spaces.rs`
are equally susceptible.

**Ruby paired keyword-token and named-clause variants** (#190,
`c42edf2`). tree-sitter-ruby emits *two* node kinds for each
control-flow boundary: a keyword token (`Else2`, `Elsif2`, `When2`,
`Rescue2`, `Then2`) plus a named clause (`Else`, `Elsif`, `When`,
`Rescue`). The grammar also emits an implicit `Then` named clause
around every `if`/`elsif` body even when the source has no explicit
`then` keyword. Counting both the keyword token AND the named clause
double-counts the structure once via the keyword and once via the
clause; counting only the keyword token misses bodies that use the
implicit `then`. The Ruby Abc impl in `src/metrics/abc.rs` matches
the named clause node in each pair (the 200-range IDs), avoiding
both pitfalls — a useful template for any future grammar that
exposes the same paired shape.

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

**`_min` sentinel guard propagated unevenly across the per-metric
sibling axis** (#227, `e347260`). The lesson scope is per-language
siblings, but the same fix-one-miss-the-others failure mode applies
to per-metric siblings. `src/metrics/tokens.rs:115-127` documented
and applied a `usize::MAX → 0.0` sentinel collapse in `tokens_min()`;
`src/metrics/loc.rs` did the same at three sites for `sloc_min` /
`ploc_min` / `cloc_min`. The remaining six metric files
(`cognitive.rs`, `cyclomatic.rs`, `nom.rs`, `nargs.rs`, `exit.rs`,
`abc.rs`) left their `_min` accessors leaking the raw `usize::MAX`
(1.8446744e19) or `f64::MAX` (1.7976931e308) sentinel straight into
JSON for any space that never observed a value. The tokens.rs guard
predated and explicitly anticipated the propagation in its doc
comment, but it had never landed. The fix added all six guards plus
per-metric `<metric>_empty_file_min_is_zero` regression tests in one
commit.

**Lesson:** Before claiming any fix in a JS-family module is
complete, grep the other three for the same identifier and apply
the same change. The check is mechanical: `rg
'<symbol_or_match_arm>' src/languages/language_{javascript,mozjs,typescript,tsx}.rs
src/{getter,checker}.rs`. Land all sibling fixes in one commit so
the diff makes the pattern visible to reviewers — splitting them
across PRs hides the symmetry. The same discipline applies to any
future trio (e.g., Java/Kotlin, C/C++/Mozcpp) and to the per-metric
axis: a defensive guard added to one file under `src/metrics/`
(sentinel collapses, interpolation child-kind guards, FIXME locks)
must be propagated across the metric family with the same `rg`
checklist.

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

**Human-readable derivation comments drift while the snapshot stays
correct** (#143, `2799547`). The Tcl `tcl_logical_operators` cyclomatic
test (a `proc f` with one `if {$x > 0 && $y > 0 || $z > 0}`) carried a
`// &&=1 and ||=1 inside expr; sum=3` comment, but the accepted snapshot
value was 5 — the comment counted `&&` and `||` but forgot the outer
`if`. The snapshot was right; only the human-readable spec drifted, and
the mismatch was invisible until the bare snapshot was converted to
Layer 2 (`assert_eq!(metric.cyclomatic.cyclomatic_sum(), 5.0)`
immediately above the snapshot call). The discipline of forcing a
positive `assert_eq!` into the diff catches this entire class of drift:
the comment can silently desync from reality, but a literal value in
source cannot.

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
unparseable C/C++** (#80, `dc09eb3`). Four DeepSpeech files exhibited
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
`013bff9`). The C++/JS-family grammars wrap `else if` in an
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

**Halstead `is_child(Interpolation)` guard missed across seven
languages, with an eighth tracked** (#180 Bash, #183 C#, #184 PHP
each fixed reactively after their respective language-addition
PRs; Elixir and Ruby shipped with the guard wired correctly during
initial language addition, leaving no issue trail; #191 Python +
Kotlin in `7a8ccac`; #199 Perl filed but not yet fixed). The
logical contract is that an interpolated string literal contributes
*only* its inner expressions as Halstead operands — the wrapping
literal itself should not count, because its inner identifiers are
walked separately. Each language's `Getter::get_op_type`
implementation classifies its own `String` / `StringLiteral` /
`string_double_quoted` node independently against its own grammar's
interpolation child-kind, so the guard has been added reactively in
each language as the bug surfaces. After #191 the only known-
affected language without the guard is Perl (#199 tracks it). The
pattern across languages is identical:
`if node.is_child(<Lang>::Interpolation as u16) { Unknown } else
{ Operand }`. The shared contract is invisible at the type level —
each impl matches a different `Lang::Interpolation` variant — but
the failure mode is uniform: `u_operands` inflates by one for every
interpolated literal.

**Cross-language parity tests caught real divergences during fixture
wiring** (#211 Bash, `28aafd6`; #212 Python, `d8ed3b5`; #228 exit,
`6de7d58`). `e2fbd2b` wired the four parity tests this lesson
prescribes (`cyclomatic_if_elseif_else_chain`,
`two_arm_switch_with_wildcard`, `early_exit_in_while_loop`,
`three_parameter_function`). Two real divergences surfaced *during
fixture authoring* — before any user reported them. Bash 2-arm
`case … esac` with bare `*)` reported `cyclomatic_max == 3` against
`2` for every other switch-bearing language (the wildcard arm
contributed when it shouldn't — the C-family analogue of `default:`).
Python's `match`/`case` reported `cyclomatic_max == 1` and
`cognitive_max == 0` (the entire construct was a dispatch hole,
lesson-19 class). Both were filed and fixed within the week. A
subsequent parity audit on early-return / throw fixtures surfaced
that Python, the JS family, Java, and C++ missed `throw`/`raise` as
exits — fixed in #228, aligning with the existing C# / Kotlin / PHP
/ Elixir behaviour. The prescription "one
fixture file per language under a shared parity test" produced
these findings on day one of the test landing, not as latent debt
years later.

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
`94c8141`). `run_parse` in
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

## 14. Forked language enums collapse via shared identifiers

(Slug / lookup helpers grow dead arms when an enum's identity method
collapses two variants to the same key.)

When two `LANG` variants represent dialects of the same language (TSX
forked from TypeScript, JavaScript and Mozjs sharing the JS family), an
identity-extracting method like `LANG::get_name()` typically collapses
the dialect to a canonical name — `LANG::Tsx::get_name()` returns
`"typescript"`, not `"tsx"`. Any helper that branches per-variant on
that canonical name has unreachable arms for every variant whose name
collapses, and any unit test that drives the helper with a literal
string (`palette_slug("tsx")`) exercises the dead arm without ever
crossing the production call path through the enum. The pattern is
distinct from lesson 3 (sibling modules mirror each other) and lesson 7
(wrapper-level tests masked by upstream filters): here the *enum's own
identity-collapse* makes the branch unreachable from production while
the test happily simulates a code path that does not exist.

**`lang-tsx` palette arm dead in HTML aggregate report** (#139,
`0a9eca1`). `language_palette_slug` matched on `LANG::get_name()` with
explicit arms for `"javascript"`, `"typescript"`, `"tsx"`,
`"mozjs"`, etc., and the embedded stylesheet shipped CSS rules for
`.lang-tsx` (light + dark mode). The unit test
`language_palette_slug_known_and_fallback` asserted on the helper
directly with literal inputs (`"tsx"`, `"java"`, …), so it agreed
that `"tsx"` mapped to the `lang-tsx` slug. Production code, however,
called `language_palette_slug(lang.get_name())`, and
`LANG::Tsx::get_name() == "typescript"` collapses TSX to the
TypeScript palette before reaching the slug helper. The `lang-tsx`
arm and its CSS rules were unreachable. The fix dropped the dead arm,
replaced the per-variant `match` with a const
`LANGUAGE_PALETTE: &[(&str, &str)]` table that an enforcement test
introspects to assert every slug has both a light and dark CSS rule,
and added a `tsx_section_uses_typescript_palette` test that drives
the helper through `LANG::Tsx -> get_name() -> palette` end-to-end.

**Lesson:** Whenever a helper branches on the output of a domain
enum's identity method (`get_name()`, `to_str()`, `as_canonical()`),
test it through the enum, not through literal strings — the literal
test exercises a code path that production cannot reach. Before
adding a per-variant arm, grep the enum implementation for cases
where the identity method collapses two variants to the same value
(`rg 'fn get_name' src/languages/`); if the variant you are about to
match collapses to another variant's name, the arm is dead. When the
helper is paired with a downstream artifact (a CSS rule, a JSON key,
a config-file lookup), add a test that walks every slug the helper
can emit and asserts the artifact exists for it — without that test,
the dead arm and its dangling artifact survive review indefinitely.

---

## 15. Workspace-excluded crates drift outside every workspace-scoped gate

The root `Cargo.toml` carries `[workspace].exclude = [..., "enums",
...]` because the `enums/` codegen crate ships a non-published binary
used only by `recreate-grammars.sh` — including it in the workspace
would run pedantic clippy and per-PR tests against code that never
ships. The carve-out is intentional, but it has a foot-gun: every
gate that follows the workspace boundary (`cargo check --workspace`,
`cargo clippy --workspace --all-targets`, `cargo test --workspace`,
the per-PR `lint` / `test` CI jobs, `make pre-commit`'s cargo trio,
`.pre-commit-config.yaml`'s clippy/test hooks) silently skips it.
Lints on `enums/src/*.rs` drift undetected until someone runs the
manual `cargo check -p enums --manifest-path enums/Cargo.toml`.

**`unused_imports` in `enums/src/lib.rs` sat for the entire fork**
(#162, fix 157d20f). The line `pub use crate::macros::*;` could not
re-export the `macro_rules!` definitions in `macros.rs` (macros use
a separate name namespace and none carried `#[macro_export]`), so
rustc warned on every build of the codegen binary. The warning was
invisible to every CI / pre-commit / Make gate because all of them
went through `--workspace`. Only a manual one-shot check found it.

The fix (#164, fix d6c96e5) added a dedicated `enums-check` Make
target that runs `RUSTFLAGS="-D warnings" cargo check
--manifest-path enums/Cargo.toml --all-targets --locked`, wired into
`make pre-commit` / `make ci`'s parallel DAG, the `make lint`
aggregate, the GitHub Actions `lint` job (twice, once via `make
lint` and once explicitly, mirroring the `snapshot-anchors`
defensive pattern), and the `.pre-commit-config.yaml` hook set. The
CI job also injects a known unused-variable warning, asserts
`enums-check` exits non-zero, and restores the file — so the gate's
*effectiveness* is pinned, not just its existence.

**Lesson:** Any crate listed in `[workspace].exclude` needs an
explicit lint/check target that does NOT go through `--workspace`,
otherwise its lint surface drifts silently. The dedicated gate must
(a) be invoked from every place the workspace-scoped gates are
(local `make` aggregate, pre-commit hooks, CI), (b) carry the same
`RUSTFLAGS="-D warnings"` posture as the workspace gates so the
behaviour matches between local and CI runs, and (c) ideally be
backed by a sabotage-style "gate-effectiveness" test in CI — if the
recipe ever stops failing on warnings, that test fires. Workspace
exclusion is the right tool for binary-only sibling crates; pair it
with a dedicated gate the moment you add it.

---

## 16. shellcheck's default severity is `style`, not `warning`

`shellcheck` ships with `--severity style` as its default — looser
than its formal `[warning]` tier. That means `make shellcheck`
fails on `SC2006` (legacy backticks → `$(…)`) and similar
style-only findings, not just on the SC2086 / SC2164 family that
people typically associate with "shell lint warnings". An issue
body that enumerates findings by ticket number (e.g., "SC2164,
SC1083, SC2086") will miss style-tier hits and under-promise the
scope of the cleanup.

**Issue #165 enumerated SC2164 / SC1083 / SC2086 only**, but the
actual `make shellcheck` failures included SC2006 backticks in
`generate-grammars/generate-mozcpp.sh` and `…/generate-mozjs.sh`.
The fix landed all four categories in one commit (`532a6d0`); the
SC2006 conversions were correct and mechanical, but they could
just as easily have been missed by a fix-agent that took the issue
body's enumeration as authoritative.

**Lesson:** When triaging or fixing shell lint debt, re-run
`shellcheck` against the actual file set *before* trusting an issue
body's category list. The body may have been authored against a
non-default severity, or simply have missed style-tier findings. As
an issue-author convention, prefer pasting the raw `shellcheck`
output rather than a hand-curated category list. As a fix-agent
convention, run the tool on each target file at default severity
and reconcile against the issue body — if categories diverge, the
extras are also in scope and should land in the same commit so
`make shellcheck` actually exits clean afterward.

---

## 17. Workspace-excluded codegen templates re-introduce cleaned-up patterns

The `enums/` crate is `[workspace].exclude`d (lesson 15), but it
*emits* code into the workspace via `enums/templates/rust.rs` — the
output lands in the per-language `src/languages/language_*.rs`
files. A lint cleanup that rewrites the emitted output without
also rewriting the template is silent until the next
`recreate-grammars.sh` run, at which point every fix is reverted
in a single regenerate.

**Issue #158 batch 1 (`a59a0e9`)** rewrote ~254 `#[inline(always)]`
attributes to `#[inline]` across all language modules. Three of
those attribute strings live in `enums/templates/rust.rs`
(`impl From<u16>`, `impl PartialEq<u16>`, etc.) and would have been
re-emitted as `#[inline(always)]` on the next grammar bump,
silently undoing the workspace cleanup. The fix included
`enums/templates/rust.rs` in the rewrite even though it is in an
excluded crate, because the *output* of that template is what the
workspace clippy gate scans.

**Lesson:** When a cleanup pass touches code under `src/languages/`,
`src/`, or any other workspace-scanned directory, also grep
`enums/templates/`, `generate-grammars/`, and any other codegen
input for the pattern you are removing. Workspace exclusion
protects the *template crate* from the workspace gate, not the
*emitted code*. Generated artifacts are downstream of their
template; the template owns the long-term posture. A clippy
cleanup that ignores the template buys exactly one regeneration
cycle before the lint debt comes back.

---

## 18. `cargo clippy --fix` is one lint at a time; cross-lint regressions hide between passes

`cargo clippy --fix -- -A clippy::all -W clippy::implicit_clone`
runs the borrow checker once, applies the suggested rewrite for
the warned lint, and exits. It does not re-run the full default
clippy lint set against the rewritten code. That means an
auto-applied fix can satisfy the targeted lint while introducing
a *different* lint that the project's `-D warnings` gate cares
about.

**Issue #158 batch 1 (`a59a0e9`)** ran `cargo clippy --fix -W
clippy::implicit_clone` over a `path_min.drain(..).map(|p|
p.to_path_buf()).collect()` site in `guess_file`. The auto-fix
rewrote `.to_path_buf()` → `.clone()`, satisfying
`implicit_clone`. On the next workspace clippy run, however,
`clippy::map_clone` (a default-feature lint) fired on the same
line because `.map(|p| p.clone())` is now redundant with
`.cloned()`. The `--all-features` gate (which was the only one
re-run after the auto-fix pass) didn't surface this because
`map_clone` is default-feature scoped on the version of clippy in
use. The default-features `-D warnings` run on the next CI tick
would have failed the build.

**Lesson:** After every `cargo clippy --fix` pass — especially
when the `-W <single lint>` flag is used to scope the rewrite —
re-run the full project lint gate in both `--all-features` and
default-features flavours before committing. `cargo clippy
--workspace --all-targets -- -D warnings` is the load-bearing
verification, not the targeted lint check that `--fix` itself
does. Treat `--fix` as a *proposal generator*, not a
verification.

---

## 19. Metric dispatch enumerates kinds — missing arms score valid constructs as zero

A per-language metric impl (`impl Cognitive for CppCode`, `impl Cyclomatic
for JavaCode`, …) is built around a `match node.kind_id()` that lists the
kinds which contribute to the metric. The list is *coverage*, not
*compilation*: a grammar can emit a valid construct under a node kind the
match arm forgot, and the metric silently emits zero for it. This is
related to lesson 1 (whole-metric no-op silently returns zero), lesson 2
(aliased kind_ids inside one logical rule), and lesson 11 (cross-language
disagreement). The new failure mode: an *already-implemented* metric has
a populated dispatch table that simply doesn't enumerate every node kind
the grammar emits for the construct the metric is supposed to count.

**C/C++ ternary `?:` was not counted for cognitive** (#172, `b2ae93f`).
`impl Cognitive for CppCode` enumerated `ForStatement | WhileStatement |
DoStatement | SwitchStatement | CatchClause` in its nesting arm but
omitted `ConditionalExpression`, while every JS-family impl already
included `TernaryExpression`. Every C / C++ source file in the corpus
scored `0` cognitive for ternaries — the DeepSpeech submodule absorbed
363 snapshots' worth of upward metric shift when the fix landed.

**C++ range-based `for (auto x : v)` was not counted for cognitive**
(#173, `7eef01a`). The same dispatch arm matched only `ForStatement`
and missed `ForRangeLoop` — the distinct C++11 grammar kind. Classic
loops scored `+1 (+nesting)`; range-based loops scored `0`. The fix
moved 99 DeepSpeech snapshots.

**Java enhanced-for `for (T x : c)` was not counted for cognitive**
(#178, `96b73d6`). `JavaCode::compute` matched `ForStatement` but
missed `EnhancedForStatement`. Discovered via the cross-language audit
table built off the C++ fix in #173 — without that systematic sweep,
the bug would have stayed invisible. The same audit confirmed the
JS-family `for...of` was fine (grammar folds both `for...in` and
`for...of` into one `for_in_statement` kind), and locked four
dedicated regression tests (`javascript_for_of_loop`, `mozjs_for_of_loop`,
`typescript_for_of_loop`, `tsx_for_of_loop`) in so a future grammar
split would fail loudly.

**Locked-in tests with `FIXME` comments made the bugs visible in CI**
(#167, `4b41187`; issue links added in `e8b9a4e`). Three of the new
C/C++ cognitive tests (`c_ternary`, `c_range_based_for`, `c_recursion`)
deliberately asserted the current-wrong values with an inline FIXME;
once the fix issues were filed, a follow-up retargeted each FIXME at
its tracking issue (`FIXME(#172)`, `FIXME(#173)`). The fix commits
(`b2ae93f`, `7eef01a`) flipped a literal expected value rather than
re-deriving from scratch, and each test failed loudly the moment its
dispatch arm was changed. The "assert wrong, flip on fix" anchor is a
useful idiom whenever a bug is identified before its fix is scheduled
— it keeps the gap visible in the test suite instead of in a stale
tracker, and the issue-link upgrade is cheap to apply later once the
tracking number exists.

**Wave 3 audits closed the dispatch gaps the C/C++ table identified
across eight sibling languages** (#212, `d8ed3b5`; #224,
`baf98d8`; #225, `ea75e35`; #226, `7fce6f7`). The audit table built when
fixing Java #178 proved its value over the following week: Python
`match`/`case` (PEP 634, 3.10+) contributed 0 decision points to
both cyclomatic and cognitive — the dispatch predated Python's
structural pattern matching and was never updated (#212). Cognitive
ternary `?:` was missing from Java, C#, and PHP — the same C++
pattern from #172 applied to three more languages (#224).
Cyclomatic short-circuit `??` (nullish coalescing) was missing from
JavaScript / TypeScript / TSX / Mozjs — C# and PHP already had it
(#226). Cognitive labeled `break`/`continue` was missing from
Java; all forms of `goto` (`label` / `case` / `default`) were
missing from C# (#225). Each fix followed the audit-table
workflow: identify the omission, build the per-language coverage
table, add a regression test, apply sibling fixes in one commit.

**Lesson:** When a metric impl uses a `match` on node kinds, treat the
arm list as a coverage claim, not a complete spec. After touching or
auditing one, grep `src/languages/language_<lang>.rs` for every kind
whose name suggests the construct (`rg 'For[A-Z]' src/languages/`
for loops, `rg 'Conditional|Ternary' …` for `?:`, …) and confirm
each is either explicitly matched or explicitly excluded with a
comment. When fixing one language's omission, build the audit table
for the other ~15 — a survey table in the fix issue, like the one
in #178, catches sibling bugs in the same pass. Anchor each
known-wrong-but-unfixed case in a regression test with an inline
`FIXME(#NNN)` so the bug stays visible in CI and the eventual fix
flips a literal value rather than re-deriving the right one.

---

## 20. `PathBuf::join(absolute)` silently replaces the base — iterate `Path::components()`

`PathBuf::join(arg)` silently *replaces* the receiver when `arg` is
absolute: `PathBuf::from("/tmp").join("/etc/passwd")` returns
`/etc/passwd`, not `/tmp/etc/passwd`. The behaviour is documented but
easy to miss when writing a "normalize then place under base"
routine. A normalizer that strips Unix-style `/` or `./` prefixes
is not enough, because Windows paths carry a `Prefix` component
(`D:\`) the same normalizer leaves intact, after which `join`
happily treats the path as absolute and drops the user-supplied
base. The bug is invisible on Unix and only surfaces against
Windows test inputs.

**`bca metrics -o tmpdir` wrote files to the drive root on Windows**
(`4113bc6`). `handle_path` in `big-code-analysis-cli/src/formats.rs`
stripped Unix-style `/` and `./` prefixes before
`output_path.join(cleaned)`. On Windows, an input like
`D:\a\src\foo.rs` left `cleaned` starting with `D:\`, `join` dropped
the user-supplied `output_path`, and the output landed under
`D:\a\src\…` instead of `<output_path>/a/src/…`. Three Windows
smoke tests (`metrics_writes_per_file_json_to_output_dir`,
`metrics_pretty_emits_indented_json`,
`ops_writes_per_file_json_to_output_dir`) caught it; Unix CI was
clean. The fix walks `Path::components()` and skips `Prefix`,
`RootDir`, and `CurDir`, replaces `ParentDir` with `.` so the
output stays contained under the requested base, and preserves the
UTF-8 fallback warning for `Normal` components.

**Lesson:** When normalizing a path for "place this somewhere under a
base," iterate `Path::components()` and discriminate by the
`Component` enum (`Prefix`, `RootDir`, `CurDir`, `ParentDir`,
`Normal`) rather than stripping prefix bytes. `Component` is
cross-platform — it surfaces the Windows `Prefix` variant
explicitly, so the same code handles `/tmp/a/b`, `./a/b`, and
`D:\a\b` correctly. Whenever a path is the result of normalization
and is about to be passed to `PathBuf::join`, assert (or design so
it cannot occur) that the input is not absolute on any platform —
`join` silently throws away the base if it is. Windows-only test
coverage is load-bearing here: a fix verified only on Unix can
ship a regression that wipes out user output on Windows.

---

## 21. Hidden-rule alias nodes extend their byte range to the shared delimiter

A visible tree-sitter node's `kind()` describes the grammar rule it
came from, but its `start_byte()` / `end_byte()` describe *which
bytes the rule actually consumed*. When a grammar uses a hidden rule
to consume a sigil or delimiter together with a sibling identifier
(common shapes in tree-sitter grammars include `seq('$', $._foo)`,
`seq('#{', $._expr, '}')`, alias inlining), the resulting visible
node can span the delimiter even though its kind name suggests
otherwise. `node.utf8_text(src)` then returns text like `"$name"`
when the kind is `identifier`, making `$name` and bare `name`
distinct entries in any text-keyed store (Halstead operands keyed
by source bytes, primitive-type tables, etc.) even though they look
identical at the kind level. The asymmetry is invisible until a
test pins integer counts and the actual byte range disagrees with
the visible token.

**Kotlin short-form string templates double-count interpolated
identifiers in tests, not in production** (#191, `7a8ccac`). The Wave
3 fix added the `is_child(Interpolation)` guard correctly, but the
initial expected counts assumed `name` inside `$name` would share an
operand bucket with the parameter `name` outside the string. Empirically,
tree-sitter-kotlin-ng emits a visible `identifier` node whose source
byte range starts at the `$` — making `$name` a distinct operand from
`name`. The Wave-3 investigation attributed this to a `seq('$',
$._identifier)`-style hidden-rule alias in the grammar; whatever the
exact rule, the observable behaviour (consult `node.utf8_text(src)`
on a representative parse to confirm before relying on a count) is
what the test must be derived against. The expected counts were
re-derived against the actual byte range (u_operands = 4, not 3)
with an explanatory comment so a future reader can reconcile the
result. The production fix was already correct; the lesson is about
how the *test* was wrong because the byte-range assumption was wrong.

**Lesson:** Never assume a node's source text matches its visible
token name. Before pinning Halstead operand counts (or any text-
keyed metric) on an interpolation-bearing snippet, dump the AST
with byte ranges and confirm what each visible node *actually*
spans. `node.utf8_text(src)` is the source of truth — visible kind
names like `identifier` describe the rule, not the bytes. The same
hazard applies to any hidden-rule alias: `template_substitution`
wrappers, heredoc body splices, language-specific `$#` / `@_` /
`${` constructs, Perl sigil variables. When the test breaks because
the count is one higher than expected, the first thing to check is
not the production code but whether the AST is splitting an
identifier the way you assumed.

---

## 22. Text-keyed semantic markers force trait signatures to carry source bytes

When a language encodes semantic state (visibility, branch type,
attribute kind) in *bare identifier text* rather than a distinct
token kind, no `kind_id`-based dispatch can classify it. The
metric impl needs to read the source bytes to disambiguate
`private` from any other `Identifier`. If the per-metric trait
signature does not already accept `&[u8]`, the addition propagates:
the supertrait, every existing per-language impl (explicit and
macro-generated), the call site in `spaces.rs`, and any downstream
signature checks. This has now happened twice for two distinct
metric traits, and the underlying need — text-keyed dispatch — is
common enough across grammars that more recurrences are likely.

**`Cyclomatic::compute` widened for Elixir keyword Calls** (#179,
see CHANGELOG `### Changed`). Elixir's `if` / `unless` / `for` /
`while` / `with` / `case` / `cond` / `try` constructs surface as
`Call` nodes with untyped targets — there is no distinct
`IfStatement` kind. Distinguishing branch-contributing Calls from
regular method invocations required reading the call target's
text, which forced `Cyclomatic::compute` to widen from
`(node, stats)` to `<'a>(node, code: &'a [u8], stats)`.
`Exit::compute` was already that shape.

**`Npa::compute` and `Npm::compute` widened for Ruby visibility
markers** (#190, `c42edf2`). Ruby's `private` / `public` /
`protected` parse as bare `Identifier` nodes whose semantic
meaning is text-only — they share a kind with every other
identifier in the program. Classifying them required reading the
source bytes, which forced both `Npa::compute` and `Npm::compute`
to widen to the same `<'a>(node, code: &'a [u8], stats)` shape as
`Cyclomatic` and `Exit`. Every per-language impl — the explicit
ones in `src/metrics/npa.rs` and `src/metrics/npm.rs` plus the
macro-generated defaults emitted by `implement_metric_trait!` —
and the two call sites in `src/spaces.rs` were updated in the same
commit. The `Checker` supertrait is `pub(crate)`, so the change is
invisible to downstream crates, but the convergence is now load-
bearing for any future metric whose impl needs source bytes.

**Lesson:** When implementing a metric for a new language, the
first question is: does this language encode any
branch/visibility/attribute semantic in **bare identifier text**
rather than a distinct token kind? If yes, the metric trait will
need `&[u8]` at the `compute` signature — plan the widening as
part of the impl, not as a follow-up refactor. Standardise on the
four-argument `<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut
Stats)` shape for any new metric trait; per-language impls that do
not need the bytes discard them with `_`. The marginal cost is
zero (the source slice is already on hand at the call site); the
savings are not having to widen the signature retroactively across
every existing impl plus the macro-generated defaults. Two
incarnations are now documented (Elixir keyword Calls for
`Cyclomatic`, Ruby visibility markers for `Npa`/`Npm`); the
catalogue will grow as more languages get real impls.

---

## 23. Compensation constants in parity tests blind the test to its own purpose

A cross-language or cross-metric parity test exists to detect when one
language (or metric impl) drifts from the others on equivalent code.
When that test catches a real divergence and the divergence cannot be
fixed in the same change, two options preserve the test's signal:
leave the test failing (`#[ignore]` with the tracking issue) or lock
it in against the wrong literal value with a `FIXME(#NNN)` comment
per lesson 19. The third option — adding a per-target offset constant
that "compensates" for the bug so the test passes — destroys the
test's ability to detect that bug class. The compensation reads like
a workaround in the diff but functions as a permanent blindfold; the
test cannot fire on the bug it was designed to catch, and any future
regression that shifts the same input by `±OFFSET` becomes invisible
too.

**`PYTHON_ELSE_BUG_OFFSET` hid a Python `if/else` over-count from the
parity test designed to catch it** (#229, `a239cf6`). `e2fbd2b` wired
the four cross-language cyclomatic / cognitive / exit / nargs parity
tests prescribed by lesson 11. `if_else_if_else_chain_parity`
detected that Python over-counted plain `if/else` by 1 — root cause:
`Node::has_ancestors(typ, typs)` in `src/node.rs` did not actually
verify both predicates against the expected ancestor chain. It
returned `true` whenever the immediate parent matched the second
predicate, regardless of whether the first predicate matched, so the
Python `Else` arm of cyclomatic fired for every `else_clause`, not
just loop-`else`. Instead of `#[ignore]`-ing the failure or
FIXME-locking the wrong literal, the test author introduced
`const PYTHON_ELSE_BUG_OFFSET: f64 = 1.0` and added it to Python's
expected sum, accompanied by an 8-line comment explaining the bug.
The OFFSET made the test pass for every Python case — including any
future regression that would shift Python's count in a different
direction. #229 fixed `has_ancestors` (renamed to
`parent_grandparent_match`, strictly checks both predicates),
updated the sole call site to include `TryStatement` in the
grandparent set, and removed the OFFSET in the same commit.

**Lesson:** When a parity test catches a real bug you cannot fix in
the same change, choose visibility over passability. `#[ignore]`
with the issue number, FIXME-lock the wrong literal per lesson 19,
or assert the buggy value with a comment that gets flipped when the
fix lands — all preserve the test's ability to detect *future*
drift on the same input. A per-target offset constant looks
defensive but actually neutralises the test: any future regression
that shifts the same metric by `±OFFSET` becomes invisible, and the
explanatory comment is no substitute for a failing test (reviewers
skim comments; CI cannot). The rule generalises beyond parity tests
— anywhere a calibration constant exists to compensate for a known
asymmetry, that test cannot catch bugs in the asymmetric path.

---

## 24. Per-metric gating must cover the finalize helpers, not just per-node compute

When introducing a "compute subset of metrics" optimisation, the
obvious place to gate is the per-node `compute()` calls in the AST
walker — but that is not where the danger lives. Some `Stats::default()`
values are intentionally non-zero (e.g., `Cyclomatic` defaults to
`1.0` for the McCabe baseline so every linear function reports a
floor of 1). The finalize helpers (`compute_minmax`, `compute_sum`,
`compute_averages`, and the derived-metric finishers) sum or average
those defaults into the headline value. If finalize is left
unconditional, the headline `cyclomatic_sum` reports a non-zero
result for every function even though no `compute()` ever fired —
and the test that verifies "this metric was skipped" by asserting
`> 0` on selected metrics will still pass, because the default
baseline looks indistinguishable from a real computation. The signal
"this metric was actually skipped" can only be carried by an
`assert_eq!(_, 0.0)` against an unselected metric whose `Stats`
default is non-zero.

**`MetricsOptions::with_only` skipped per-node compute but ran
finalize for every metric** (#257, `1169231`, `d758f89`, `d5f9ff2`). The
first cut of `with_only(&[Metric::Loc])` correctly gated each
`T::X::compute(node, code, ...)` call inside `compute_per_node` but
left `compute_minmax`/`compute_sum`/`compute_averages` /
`compute_halstead_mi_and_wmc` unconditional. The resulting
`FuncSpace.metrics.cyclomatic.cyclomatic_sum` reported the McCabe
baseline (`1.0` × number of functions) even with Cyclomatic
deselected. `loc_only_skips_other_metrics` caught it by asserting
`pruned.metrics.cyclomatic.cyclomatic_sum() == 0.0` (strict zero,
not `<`); without that anchor the bug would have shipped. The fix
threaded `selected: MetricSet` through every finalize helper and
gated them at the same granularity as `compute_per_node`.
`mi_auto_pulls_dependencies` / `wmc_auto_pulls_dependencies` were
strengthened in `d5f9ff2` to anchor on the dependency *values*
(`loc.ploc() > 0`, `cyclomatic_sum() > 0`) for the same reason —
asserting only `mi.is_finite()` would have passed against the
`inputs_are_empty` short-circuit returning `0.0` from default-zero
inputs.

**Lesson:** "Skip computing metric X" must gate every place X is
read or aggregated, not just the per-node compute call. Audit the
default value of every `Stats` type before adding gating: any
non-zero default (Cyclomatic's `1.0` baseline is the canonical one,
but new metrics may add others) will silently propagate through
finalize. Write at least one test per gating point that asserts
`== 0.0` (or the metric's default) on an unselected metric whose
default is non-zero; an `> 0` anchor on the *selected* metric is
necessary but not sufficient.

---

## 25. Crate-root `pub use module::*` silently leaks every newly-`pub` sub-module item

A glob `pub use submodule::*` line at the crate root makes every
`pub` item in that submodule part of the published API surface,
whether the author intended it or not. Reviewers cannot see what
the line exports without enumerating every `pub` item in the file;
internal helpers added for the CLI, types meant for testing, and
trait methods bumped to `pub` for one call site all become
SemVer-relevant. The leak is invisible until someone removes the
glob and watches the curated list grow.

**Wave 10 of the library-DX batch exposed 17 `pub use module::*`
lines hiding accidentally-public items** (#255, `bab3da9`).
`src/lib.rs` carried `pub use alterator::*; pub use node::*;
pub use metrics::*; ...` for every sub-module — 17 globs in a row
(the issue body cited 16 against an older snapshot of the file).
Replacing them with curated lists revealed several items that the
crate's own internals had been reaching at `crate::X` paths,
working only because the glob made them `pub` at the root. Tightening required adding `pub(crate) use crate::abc::*;`,
`pub(crate) use crate::cognitive::*;`, etc. for the in-crate consumers (`metrics_inner`, `Search`, `check_func_space`,
the per-metric `Cognitive`/`Cyclomatic`/... type tags) so the
internal call sites kept compiling — those types had been
public-by-accident, and nothing other than the glob made it look
deliberate.

**Lesson:** A pre-1.0 library that uses crate-root glob re-exports
has a latent public-API surface no reviewer can fully see. Replace
globs with explicit `pub use module::{X, Y, Z};` before stabilising
anything that depends on the surface (a `prelude` module, a
`cargo-public-api` baseline, a `STABILITY.md`). The unavoidable
side-effect is that internal callers reaching previously-leaked
items have to be re-routed via `pub(crate) use ...` or
fully-qualified paths — surface that drift as part of the same
change, not as a chase later. Don't add new `pub use module::*` to
`lib.rs` once it has been curated.

---

## 26. Feature-gating a generic dispatcher forces the return type to widen to `Result`

When per-language Cargo features remove some `LANG` variants from
the build, the dispatch macro (`mk_action!`, `mk_lang!`, etc.) must
still match every variant of the always-defined enum — disabled
variants need a `#[cfg(not(feature = ...))]` arm that returns
*something*. The previous signature `fn action<T>(...) -> T::Res`
is uninhabitable at the disabled-feature site: there is no way to
construct an arbitrary `T::Res` value when the per-language type
that defines `T::Res` is itself cfg'd out. The only escape hatch
that preserves the always-defined-enum design is to widen the
return to `Result<T::Res, MetricsError>`, with the disabled arm
returning `Err(MetricsError::LanguageDisabled(lang))`. Once that
widening lands, every caller of the generic dispatcher rises to
match — including non-generic shims like
`LANG::get_tree_sitter_language` that share the macro template.

**Per-language features required widening `action::<T>` and
`LANG::get_tree_sitter_language` to `Result`** (#252, `b923919`).
Adding `#[features]` for each grammar crate kept
`LANG` always-defined (per the issue's stability rationale) but
introduced `#[cfg(feature)]` / `#[cfg(not(feature))]` paired arms
across `mk_action!` and `mk_lang!`. The `not` arms had no way to
return a `T::Res`, so `action::<T>` widened to `Result<T::Res,
MetricsError>` and `LANG::get_tree_sitter_language` likewise.
This rippled into the CLI and web crates, where every
`action::<_>(...)` call site became `action::<_>(...).expect(FEATURES_PINNED)`
because both crates pin `features = ["all-languages"]` and the
disabled arm is provably unreachable for them. The breaking
changes were called out in `CHANGELOG.md` and `STABILITY.md`.

**Lesson:** Generic dispatch signatures that return an associated
type cannot be feature-gated without widening the return to a
`Result` (or some other error-carrying shape). Plan the widening
into the same change as the feature flag — split into separate PRs
only at the cost of an unbuildable intermediate state. Always-pinned
downstream callers (the CLI / web crates here) can carry the
invariant with a single `const FEATURES_PINNED: &str = "..."` plus
`.expect(FEATURES_PINNED)` at every call site; defining the
invariant once is more honest than scattering identical literal
panic messages.

---

## 27. Share a private walker across deprecation shims to keep them thin

When introducing a new public API alongside a deprecated one (a
common pattern when widening a contract — adding a builder,
swapping `Option` for `Result`, replacing positional args with a
struct), the temptation is to fork the implementation: leave the
old one alone, write a fresh one for the new API. That doubles the
walker code, doubles the place future bug fixes have to land, and
guarantees the deprecation cycle ships two slightly-different
implementations that drift apart. The honest move is to extract a
single private function (visibility `pub(crate)` if multiple
modules call it, otherwise file-local) that takes the union of
both APIs' inputs as ordinary parameters, then make both public
entry points thin shims around it. The deprecated entry point
becomes a `#[deprecated]` one-liner that constructs the new
parameters from its old ones; the new entry point is the same
shape. Future fixes touch the shared core, not either shim.

**`Source` and `analyze` introduction kept the old walker thin via
`metrics_inner`** (#254, `41d5005`, `8b460fb`). Wave 7 of the
library-DX batch landed `Source<'a>` and
`analyze(source, options) -> Result<FuncSpace, MetricsError>`
alongside the deprecated `metrics` / `metrics_with_options` /
`get_function_spaces*` entry points. Rather than fork the walker,
the agent extracted
`pub(crate) fn metrics_inner(name: Option<String>, ...) ->
Result<FuncSpace, MetricsError>` to carry the actual tree walk.
The deprecated shims build
`name = Some(path.to_string_lossy().into_owned())` and call
`metrics_inner`; `analyze` destructures `Source` and calls the
same. `8b460fb` followed up by dropping a redundant
`diagnostic_path` parameter once the path/name relationship was
consolidated through `metrics_inner` — the diagnostic string is now
derived from `name.as_deref().unwrap_or("<input>")`, eliminating
one parameter and the matched `path.display().to_string()` /
`path.to_string_lossy().into_owned()` double allocation at every
shim.

**Lesson:** Any deprecation cycle where the old and new APIs share
most of the work should land a single private worker with the union
shape, fronted by two public shims. Avoid the "leave the old code
alone, fork a copy" pattern — it ships two implementations, doubles
the surface where future fixes must land, and lets the
deprecated-on-paper path silently drift. The same advice applies
when adding a `from_X` constructor next to `new`: extract the
common construction body, don't copy it.

---

## 28. Hand-rolled `Serialize` with conditional fields must pre-count for CBOR

`serde`-derived `Serialize` for a struct emits a fixed field count
known at compile time, so output formats that prefix the field
count (CBOR, MessagePack, BSON in object mode) Just Work. The
moment a field becomes conditional based on runtime state — and the
conditional state lives outside the field itself, so
`#[serde(skip_serializing_if = "...")]` cannot be a free function
of the field — `derive(Serialize)` no longer suffices. A hand-rolled
impl is the only escape. The trap: `SerializeStruct::serialize_struct(name, len)` writes the `len` to the
underlying format *before* the first field, and CBOR / MessagePack
reject the payload at `st.end()` if the actually-emitted field
count diverges. JSON quietly tolerates the mismatch (it doesn't
write a length prefix), so test runs that only exercise JSON pass
even with a buggy count. Every conditional `serialize_field` arm in
the body must be paired with a matching boolean in the `len`
tally — and the two must stay in sync forever.

**`CodeMetrics::serialize` had to track the field count across 13
conditional emit arms** (#257, `1169231`, simplified by `66a0d8c`).
Per-metric gating made every emitted field in `CodeMetrics`
conditional on `self.selected: MetricSet`. The hand-rolled
`Serialize` impl pre-computes the field count and only then calls
`serializer.serialize_struct("CodeMetrics", field_count)`:

```rust
let field_count = always_on
    .iter()
    .filter(|m| sel.contains(**m))
    .count()
    + usize::from(emit_wmc)
    + usize::from(emit_npm)
    + usize::from(emit_npa);
```

The body's 13 `if sel.contains(...) { st.serialize_field(...)? }`
arms had to match this tally 1:1. The simplify-rust pass
(`66a0d8c`) collapsed those arms into a local `emit_if!` macro
(`emit_if!(sel.contains(Metric::X), "name", &self.field);`), which
made the 1:1 correspondence visually obvious in code review but
did not change the underlying invariant. The integration-snapshot
suites (parent repo `tests/snapshots/` and the
`big-code-analysis-output` submodule) are JSON-based
`insta::assert_json_snapshot!` and would NOT catch a tally bug;
only an actual CBOR consumer (e.g. `bca metrics -O cbor` round-
tripping a non-trivial fixture) does. There is no end-to-end CBOR
test in the workspace today — flagged for the next round of test
hardening.

**Lesson:** Hand-rolled `Serialize` impls that emit a conditional
field set must compute their field count from the *same* predicates
they use in the body. The two halves cannot drift. If the format
mix includes CBOR / MessagePack / any length-prefixed binary
encoding, only those formats catch the bug; never trust a JSON-only
test pass. A local macro that pairs the predicate with the field
name in one place is the cheapest defence against future drift —
the alternative is a comment block warning future authors to keep
the tally in sync, which everyone ignores.

---

## 29. Compile-test API doc samples by linking against a scratch crate, not `mdbook test`

API documentation written in Markdown (book chapters, README
recipes, hand-rolled `# Examples` sections) drifts as soon as the
public API changes. `mdbook test` runs each fenced ```rust block as
a doctest, but only against the book's *own* `Cargo.toml`
dependency list — it does not resolve `use crate_under_test::...`
against the local checkout. That means typos like `LANG::JavaScript`
vs `LANG::Javascript` (the variant is `Javascript`), wrong argument
counts after a function-signature change, and silently-renamed
re-exports all sail through `mdbook build` and `mdbook test` until
a reader copies the sample, hits `cargo check`, and reports back. A
scratch crate that depends on the library via `path = "../"` and a
`cargo check` against its `src/lib.rs` (one module per book page,
each containing the page's code samples) catches every such typo
in seconds.

**The "Using as a Library" book chapter caught `LANG::JavaScript`
(correct: `Javascript`) before publish** (#259, `8ee83ea`). Wave 4
of the library-DX batch
added eight new book pages under `library/`, each carrying several
fenced Rust code samples that drive `get_function_spaces` /
`analyze` against the current public API. Writing the samples by
hand against rustdoc surfaced one real typo (`LANG::JavaScript`
where the actual variant is `Javascript`) and one outdated method
name. The agent wrote the samples into a scratch crate that
depended on `big-code-analysis 0.0.25` via `path = "../"`, ran
`cargo check`, fixed the typo, then copied the verified samples
back into the book pages. `mdbook test` alone would have shipped
the typo.

**Lesson:** Treat API doc samples as production code under
`cargo check`. The cheapest way is a scratch crate (or a `tests/`
integration file with `#[allow(dead_code)]`) that compiles every
sample against the local checkout — not `mdbook test`, which lacks
the crate's full public API in scope. Run this gate before
committing book chapters and as part of `make pre-commit` when the
diff touches `big-code-analysis-book/src/`. The cost is one scratch
file; the avoidance is reader-facing-API typos.

---
