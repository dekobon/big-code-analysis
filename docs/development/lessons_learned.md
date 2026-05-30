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

**`bca.analyze_source(code, "javascript")` rejected the canonical
CLI display name** (#265 batch, `182974b`). The first cut of the
PyO3 bindings published the lowercased Rust variant name
(`"mozjs"`) as the language identifier on
`bca.analyze_source(code, language)`, while
`bca metrics --output-format json` showed `"language":
"javascript"` (via `LANG::Mozjs::get_name() == "javascript"`).
Users reading the CLI output and feeding that string back to the
bindings hit `UnsupportedLanguageError("javascript")`. The
inverse of the TSX case above: there, production matched on a
name the enum can never emit; here, the *binding's public API*
exposed a name the canonical identity method can never emit. Same
enum-identity-collapse root, opposite-direction symptom — the fix
routes `parse_language_name` through `lang_to_name`, the same
helper the rest of the binding already uses for the inverse
lookup.

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

## 30. User-facing comment markers should match the codebase's internal vocabulary

In-source suppression markers originally used `bca: allow` /
`bca: allow-file`, mirroring Rust's `#[allow(clippy::…)]` attribute.
Hard-renamed to `bca: suppress` / `bca: suppress-file` in #263 before
the markers shipped widely. The verb `allow` reads correctly inside
`#[allow]` only because it sits in a closed four-level lint
vocabulary (`allow`/`warn`/`deny`/`forbid`) inside attribute syntax.
Stripped of both — dropped into a free-text comment in a codebase
with no `warn`/`deny`/`forbid` siblings — the verb is a bare English
imperative that reads as "this code is permitted to be complex"
rather than "suppress the violation report." The marker also has to
read well in Python, JavaScript, C++, Java, Kotlin — where the
ecosystem lint-suppression vocabulary is `disable` (ESLint, pylint,
shellcheck), `ignore` (mypy, pyright, staticcheck), or `suppress`
(cppcheck, Java `@SuppressWarnings`, SpotBugs, Detekt). `allow` as
an embedded-comment verb is essentially unique to Rust attributes.

**Lesson:** When choosing a verb for a user-facing marker that lives
inside source comments across many languages, pick the verb that
*the rest of the codebase's own internal vocabulary already uses*
(here: `src/suppression.rs`, `SuppressionPolicy`,
`FuncSpace::suppressed`, `--no-suppress` — all "suppress"). That
alignment eliminates the cognitive bridge between the comment a user
writes and the module a reviewer reads. Industry precedent comes
second; internal naming consistency comes first because it's the
thing future contributors will keep tripping on. Cross-check with at
least three lint-suppression tools from outside the host language
(`#noqa`, `eslint-disable`, `@SuppressWarnings`, `cppcheck-suppress`,
`# type: ignore`) before committing — if your verb is the outlier
across that set and against your own internal model, redesign.

---

## 31. Shared structural fixes need a structural assertion in every per-metric test

When one fix changes both a shared classification predicate (e.g.,
`is_func_space` recognising a new node kind) *and* the body-walker
counts each metric derives from it, the per-metric tests must each
guard *both* halves of the change. A body-walker assertion alone is
not enough: counts can fire from a fallback scope (a synthetic Unit
wrapper, a `SpaceKind::Class` default, a zero) and pass vacuously
even after the structural arm is reverted. Three sibling per-metric
tests can each look complete while collectively guarding nothing
about the shared structural change. Related to lessons #7 (test
infrastructure rigor) and #23 (compensation constants that blind a
test to its own purpose); this lesson addresses the distinct
*coverage decomposition* problem that arises when one structural
fix is spread across several per-metric tests.

**Java/Groovy annotation-type recognition** (#280, #307, ba2a8e3,
d637a98). The #280 fix wired `AnnotationTypeDeclaration` into
`JavaCode::is_func_space` and `GroovyCode::is_func_space` so `Npa`,
`Npm`, and `Wmc` would walk annotation-type bodies and produce
non-zero counts. Three per-metric regression tests were added —
one each for Npa, Npm, Wmc. An `audit-tests` pass later revealed
only the Wmc test caught a revert of the `is_func_space` arm: it
asserted `interface_wmc_sum() == 0`, vacuously true when no
Interface FuncSpace opens. Npa and Npm both passed even with
`AnnotationTypeDeclaration` removed, because their counts (`2.0`)
came from the file-level Unit scope. ba2a8e3 tightened Wmc first;
\#307 (d637a98) then tightened both Npm and Npa with
`check_func_space` assertions that the annotation type opens a
`SpaceKind::Interface` FuncSpace named `Marker`, and factored the
six structural assertion sites across the three metrics into a
shared `tools::assert_child_space_kind` helper.

**Plain `interface I {...}` declarations share the bug** (#311).
The same pattern exists for ordinary Java/Groovy interface
declarations: tests in `npm.rs` and `npa.rs` assert non-zero
`interface_*_sum` without a structural check, so a revert of the
`InterfaceDeclaration` arm in `is_func_space` would also pass them
vacuously. Filed as #311 after the wave-2 audit on the #307 fix.

**Lesson:** When a single fix has both a structural arm (a predicate
or dispatch table that opens a FuncSpace) and per-metric body-walker
arms (the metric counts inside it), every per-metric test must
assert *both* halves: the structural side (FuncSpace opens with the
expected `SpaceKind` and name) and the body-walker side (the metric
sum matches the expected value). Use `check_func_space` (or the
`tools::assert_child_space_kind` helper) at the top of each test;
follow with the existing `check_metrics` value assertion. Coverage
that *looks* complete because three metrics each have a regression
test can in reality be split — three vacuous tests guard nothing
about the structural change.

---

## 32. Source-grep regression tests are theater

A test that reads its own source files via `include_str!` or
`fs::read_to_string` and string-matches their contents to assert a
"structural contract" provides no real protection. The grep is
brittle to cosmetic edits (comment wording, rustfmt reflow, `impl`
header rename) and easily satisfied vacuously by adding the
identifier in an unrelated comment. If the contract is "predicate X
explicitly names variant Y," the production `matches!()` pattern
already *is* the contract — the grep test asserts the same thing the
reader can see, just less reliably. Related to lesson #2
(tree-sitter aliases must be matched on every variant — the
contract this anti-pattern most often tries to protect) and lesson
\#6 (snapshot tests pin behaviour, not correctness — both are
"test asserts the wrong thing" failure modes); the distinct
property of source-grep is that the *test mechanism itself* is
indirect, not the value it captures.

**`FunctionDefinition4` source-grep test** (#285, #302, fe5bf6a).
The original #285 fix wired
`Cpp::FunctionDefinition4` into four predicates
(`is_func_space`, `is_func`, `get_func_space_name`,
`get_space_kind`). Because the pinned tree-sitter-mozcpp does not
emit the FD4 kind_id on any input the author could construct, a
parse-and-assert regression test was impossible. The fix added a
test that read `src/checker.rs` and `src/getter.rs` from disk and
counted `FunctionDefinition4` occurrences inside each `impl` block.
That test passed code review and shipped, but #302's investigation
showed it was both fragile (a rustfmt pass that joined two `impl`
lines would break the block-extraction helper) and vacuous
(adding `// FunctionDefinition4` to one impl block would satisfy
the count without wiring the variant). The remediation deleted
the test entirely and added contract comments at the four
production sites citing #285 and listing the sister sites; the
`matches!()` patterns themselves are the structural protection.

**Lesson:** If your test reaches for `include_str!`,
`fs::read_to_string`, or any string-matching against the codebase's
own source, the test is almost certainly broken or about to be.
Three viable alternatives in priority order: (a) construct an input
the grammar can actually parse and assert the parse-tree consequence
(the gold standard — write the regression at the AST level the
production code reads); (b) if that's impossible because the kind_id
is grammar-unreachable, document the contract as a comment at every
production call site and rely on code review of the `matches!()`
pattern; (c) if neither works because the contract is large or
cross-file, design a compile-time check (exhaustive `match` over the
enum, a `const`-evaluated assertion, a `pub(crate) fn` whose call
sites are themselves the contract). Source-grep — the anti-pattern
this lesson is against — adds the visual appearance of protection
while providing none; the only such test ever written in this
codebase (the #285 FD4 regression) was identified as vacuous and
removed within months of merging.

---

## 33. Test-via-revert proves coverage one slot at a time

When a refactor consolidates N near-identical sites behind a macro
or shared helper that takes per-site *delta slots* (operator/operand
extras, per-language variant lists, decision-kind extras), a single
test-via-revert proof — "I dropped one thing from one site and the
test failed" — protects only that one thing at that one site. The
remaining N×slots arguments are still ungated. The test-via-revert
discipline must be applied to *each* slot type and, for multi-slot
sites, to *each* slot's distinct contents. Otherwise the test reads
as a four-way parity guard but is actually a one-way regression
guard for a single operator token. Related to lesson #23
(compensation constants blinding a parity test) and lesson #31
(per-metric coverage decomposition); the distinct property of this
lesson is the *delta-slot* granularity — a macro's parameter list
is itself a partition the test must cover slot-by-slot.

**JS-family `get_op_type` parity test only revert-proved the
operator token** (#299, 45d907f, 06f6a68). 45d907f introduced
`js_family_get_op_type_parity_optional_chain_member_299` asserting
four-way parity (`u_operators`, `operators`, `u_operands`,
`operands`) for `function f(a){return a?.b?.c;}`. Its comment
claimed "dropping a common variant from any one language's macro
invocation must fail this test." A follow-up `audit-tests` pass
perturbed every slot and found this true only for the operator-
token slot (`OptionalChain` in JS/MozJS, `QMARKDOT` in TS/TSX). Dropping any entry from the per-language `operand_extras`
lists (`Identifier2`, `String2`, `NestedIdentifier`,
`MemberExpression4`, or TS's `PredefinedType`) left the test
silently passing — the `a?.b?.c` fixture never produced those
node kinds. 06f6a68 rewrote the comment to scope the claim
honestly; one of the operand-extras gaps it could have caught —
TS classifying `String2` differently in `Checker::is_string`
versus `Getter::get_op_type` — was independently surfaced during
the same review and filed as #313.

**`impl_simple_is_string!` positive test only revert-proved one
variant per language** (#301, 7192d56, 5829560). The initial
`simple_is_string_macro_recognises_each_language` test exercised
one canonical string literal per consolidated language. Test-via-
revert with `Rust::StringLiteral` proved the macro arm was
reached, but Csharp (4 variants), Php (7), Ruby (11), Perl (7),
Bash (5), Groovy (3) were each defended by a single literal —
dropping `Csharp::VerbatimStringLiteral`, `Ruby::Subshell`,
`Php::Heredoc`, or `Cpp::ConcatenatedString` from any macro
invocation left the test passing. 5829560 hardened the test
to one assertion per variant per language (via the
`assert_variant_is_string` helper with `stringify!`-derived
labels); the resulting failure messages name both language and
variant on drift. The reusable pattern is in
`src/checker.rs`'s test module.

**Lesson:** When proving a parity or coverage test catches drift,
the test-via-revert discipline must visit every *delta slot* the
refactor introduced, not just one. For macros with bracketed
extras lists (`op_extras: [...]`, `operand_extras: [...]`,
`[$($variant),+]`), each list is its own slot and each list's
contents must be revert-proved. For multi-variant languages
inside a single macro invocation, each variant is its own slot.
A useful remediation pattern: route assertions through a helper
that takes the language path and variant name as strings (via
`stringify!`) so a failed assertion identifies *which* of the
N×slots arguments is broken — and so future grammar/refactor
work that adds a slot fails loudly if the helper is not extended
to cover it.

---

## 34. Tree-sitter hidden-rule variants exist in the enum but never surface

Tree-sitter language grammars expose every node-kind name through
the generated enum (`Java::*`, `Groovy::*`, `Php::*`, etc.), but
rule names beginning with an underscore (`_string_literal`,
`_multiline_string_literal`, `_string`) are *hidden* — the parser
flattens them away in real ASTs and never emits a node carrying
that kind_id. A defensive arm in `is_string` / `get_op_type` / etc.
listing a hidden-rule variant is dead code today and a
correctness promise *if* a future grammar revision ever promotes
the rule to a concrete node. Without an "asserted-absent"
drift-marker test pinning the hidden status, that promotion goes
undetected: the parser starts emitting the variant, the predicate
either silently misses it (if the arm was forgotten) or silently
catches it (if listed defensively) — either way, the codebase
loses visibility into what changed.

**Java/Groovy/Php `is_string` consolidation made the heuristic
explicit** (#301, 7192d56, 5829560). Per-variant positive coverage for the
new `impl_simple_is_string!` macro required exercising every
variant in every invocation. Three variants would not appear in
any constructible source: `Java::MultilineStringLiteral` (Java
text blocks parse as regular `StringLiteral`),
`Groovy::StringLiteral2` (Groovy triple-quoted strings parse as
regular `StringLiteral`), and `Php::String3` (the `_string` hidden
supertype never surfaces). The `kind_for_id` mapping confirmed
the heuristic: each maps to a name beginning with `_`. The
remediation pattern was a paired assertion: the macro arm stays
(future-proof against grammar promotion) and a sibling test
asserts `!ast_has_kind_id(&parser, Lang::HiddenVariant as u16)`
with a message naming both the variant and the hidden-rule it
maps to, so a future parser that starts emitting the variant
trips the assertion-absent loudly and the maintainer is forced
to replace it with a positive assertion.

**Lesson:** Before listing a "looks like an alias" variant in a
classification predicate, check the grammar's `kind_for_id`
mapping for that variant (or grep `src/languages/language_<lang>.rs`
for the `Lang::Variant => "name"` arm). If the name starts with
`_`, the rule is hidden: the variant exists in the enum but does
not appear in real ASTs. Either omit the defensive arm (and rely
on the underlying concrete variant), or — preferred — keep the
arm *and* add a drift-marker assertion (`!ast_has_kind_id`) with a
message that explains the hidden status and demands replacement
on drift. Hidden-rule variants without a drift-marker test are
invisible promises: they pretend to be coverage but protect
nothing observable today.

---

## 35. Two predicates classifying the same node must agree, or Halstead drifts silently

For every supported language, `Checker::is_string`,
`Getter::get_op_type`, `Checker::is_call`, `Checker::is_func_space`,
and the per-metric body walkers all classify the same AST nodes
through parallel `matches!()` predicates. When two predicates that
should agree on a node's classification disagree, the metric
output silently drifts: `find string` / `count string` reports a
node that Halstead classifies as `Unknown`, or vice versa. The
disagreement is invisible from either predicate read in isolation
— it surfaces only by walking the cross-product of (node kind, set
of predicates that classify it). Refactoring one predicate
without parity-walking the others ships the drift. Related to
lesson #2 (tree-sitter aliases must be matched on every variant
of *one* predicate) and lesson #19 (missing arms in a dispatch
table score valid constructs as zero); the distinct property of
this lesson is *cross-predicate parity* — both predicates may be
internally consistent and still disagree on the same node.

**TypeScript `String2` agrees with `is_string`, disagrees with
`get_op_type`** (#313, surfaced during #299 review). The
`impl_js_family_is_string!(Typescript)` macro matches `String`,
`String2`, and `TemplateString`, so a `String2` node — the `string`
type-keyword alias — is counted by `find string` and contributes
to Halstead string-operand totals via `is_string`. But the TS
`impl_js_family_get_op_type!` invocation's `operand_extras` list
omits `String2`, so the same node is classified as `HalsteadType::
Unknown` by the Halstead walker and does not contribute to
`operator/operand` totals at all. JS, MozJS, and TSX all include
`String2` in `operand_extras`; only TS does not. The drift
predates #299 — the four pre-refactor impls had the same
asymmetry — but the macro consolidation made the parity table
legible enough for a reviewer walking each invocation to spot it.

**Lesson:** When refactoring or extending a per-language
classification predicate, walk the *other* predicates that
classify the same nodes for parity. The minimum cross-walk is:
`Checker::is_string` ↔ `Getter::get_op_type` operand classification
of string-bearing kinds; `Checker::is_call` ↔ `Getter::get_op_type`
operator classification of call kinds; `is_func_space` ↔ each
metric's body-walker (already covered by lesson #31 for the
structural-arm half). The reusable diagnostic test is a parsed
fixture asserting *both* predicates agree: parse a source that
contains the node in question, locate every occurrence by kind_id,
and assert each predicate returns the expected verdict for each
occurrence. Disagreement that ships becomes a Halstead drift bug
that takes a cross-file review to spot — far cheaper to catch at
predicate-edit time with a parity walk than at user-report time
from a metric mismatch.

---

## 36. `serde_json::to_value` re-sorts JSON object keys via `BTreeMap`

`serde_json::Value::Object` is backed by `BTreeMap<String, Value>`
unless the `preserve_order` Cargo feature is enabled — which the
workspace does not enable. Any round trip that goes
`Serialize → to_value → ... → re-emit` silently alphabetises the
keys, regardless of the original `Serialize` impl's declaration
order. Code that bridges serde-Rust output into another
insertion-ordered runtime (CPython's `dict`, Lua's `pairs`, an
ordered-map-backed JS object) loses the field order at the
`to_value` boundary; the loss is invisible unless a reader
compares the original struct's field order against the output
bytes.

**`bca.analyze()` field order silently re-sorted alphabetically
through `to_value`** (#265 batch, `6574aff`). The first cut of
the PyO3 bindings serialised `FuncSpace` via
`serde_json::to_value` and then walked the `Value` tree to build
a Python `dict`. The output came out
`{"end_line", "kind", "metrics", "name", "spaces", "start_line"}`
— alphabetical — instead of the `Serialize` impl's
`{"name", "start_line", "end_line", "kind", "spaces", "metrics"}`
declaration order, which the `bca` CLI emits via `to_string`.
Byte-for-byte parity with the CLI was the bindings' stated
contract; the trap was invisible because `dict ==` is
order-insensitive in Python, every test that compared dicts to
dicts passed. The fix routes the bindings through
`serde_json::to_string(&space)` followed by CPython's
`json.loads`, which builds the `dict` in input order (CPython
3.7+ guarantees insertion-order iteration). A
`nested_structure_preserves_funcspace_field_order` Rust test now
pins the contract by serialising a local struct whose declaration
order differs from alphabetical and walking the resulting `dict`
keys verbatim.

**Lesson:** When crossing a serde→insertion-ordered-runtime
boundary, route through `serde_json::to_string` and re-parse on
the other side, not through `to_value`. The `preserve_order`
feature is an alternative but applies workspace-wide and may
interact with downstream crates expecting the default sort. The
diagnostic test that catches this regression *cannot* compare
structurally-equivalent containers — it must compare the emitted
key order against a hand-pinned sequence whose source order is
deliberately non-alphabetical (or it must compare raw JSON bytes
position-by-position). A test that asserts equality of two dicts
walks right past the bug.

---

## 37. CPython `OSError(errno, msg, filename)` dispatches to the right subclass; the 1-arg form collapses to bare `OSError`

CPython's `OSError` constructor is overloaded by arity: passing
`OSError(errno, message, filename)` dispatches to the matching
subclass (`FileNotFoundError` for `ENOENT`, `PermissionError` for
`EACCES`, `IsADirectoryError` for `EISDIR`, …) and populates
`err.errno` / `err.filename` so idiomatic Python handling —
`except FileNotFoundError as e: log(e.filename)` — works. Passing
`OSError(message)` (1-arg) loses the dispatch entirely: every
I/O failure surfaces as bare `OSError` with `errno is None`, no
subclass match, no `filename` field. The 1-arg form is the
natural shape when bridging Rust's `std::io::Error::to_string()`
into Python and is the easy default that everyone reaches for
first.

**`bca.analyze(missing_path)` raised bare `OSError`, not
`FileNotFoundError`** (#265 batch, `f91fac0`). The PyO3 bindings'
`AnalysisError::Io { source }` arm originally mapped to
`PyOSError::new_err(source.to_string())` — string-only. Python
callers writing `except FileNotFoundError as e: e.filename` never
matched the subclass and never saw the path. The fix carries the
originating `PathBuf` through `AnalysisError::Io { source, path }`
and constructs the PyError as
`PyOSError::new_err((source.raw_os_error(), source.to_string(),
path.display().to_string()))`. A regression test pins the
contract:
`pytest.raises(FileNotFoundError) as exc_info;
exc_info.value.errno == errno.ENOENT; exc_info.value.filename ==
str(missing)`.

**Lesson:** Every PyO3 binding that surfaces a Rust
`std::io::Error` must build the Python exception with the 3-tuple
`(errno, msg, filename)` form — `errno` from
`io::Error::raw_os_error()`, `filename` from the path that
triggered the failure. This requires capturing the `Path` at the
failure site, not just the `io::Error` (a blanket
`From<io::Error>` impl would drop it). The 1-arg
`PyOSError::new_err(message)` form is wrong by default for I/O
bridges; reserve it for non-I/O `OSError` usage where no path or
errno applies. Verify via a `FileNotFoundError` round-trip test
that inspects `err.errno` and `err.filename`, not just the
exception class — a test that catches `OSError` would pass
against the buggy code.

---

## 38. Co-pinned runtime + build-time companion crates must share an exact patch, not a caret range

When a crate ecosystem splits its FFI contract across two crates
— one for the runtime ABI, one for the build-time link-args /
codegen — Cargo's default caret semver can resolve the two to
different patches even though they implement two halves of the
same contract. Once the drift happens, a build-time symbol or
link flag emitted under the older patch can disagree with what
the runtime crate of the newer patch expects on the search path,
and the symptom is a mysterious link error at test time, not a
compile error in either crate. The cheap defence is to catch
the drift at the pin, before any observed failure — once it
surfaces, bisecting two interlocked patches is far more
expensive than spelling `= "X.Y.Z"` at both sites up front.

**`pyo3 = "0.28"` paired with `pyo3-build-config = "0.28"` was
pinned preventatively before the drift could surface** (#265
batch, `50c7fca`). The `big-code-analysis-py` build script called
`pyo3_build_config::add_libpython_rpath_link_args()` to bake the
libpython rpath into binaries that embed Python (i.e.
`cargo test` with `pyo3/auto-initialize`). The rpath link-args
contract is part of pyo3's build-time API; whether it emits
`-Wl,-rpath,…` or `-Wl,-rpath-link,…`, and where the path comes
from (interpreter probe vs. `PYO3_PYTHON` env var), depend on the
pyo3-build-config patch. Both deps were originally spelled
`"0.28"` (caret `^0.28.0`), so cargo was free to resolve them
independently to e.g. `pyo3 0.28.3` + `pyo3-build-config
0.28.1`. At the time of the fix both crates happened to be
resolving to `0.28.3` — `cargo tree -d` was clean, no link
failure had been triaged — but the next `cargo update` could
have moved one without the other. The pin to `= "0.28.3"`
forecloses the drift: a future patch bump (`pyo3 → 0.28.4`) now
requires a deliberate paired edit of `pyo3-build-config`, and
the comment at each pin names the partner crate so the lockstep
survives the next contributor.

**Lesson:** Identify co-pinned crate pairs that span the
runtime / build-time FFI boundary in any dependency family you
adopt (pyo3 / pyo3-build-config, sqlx / sqlx-macros,
opentelemetry / opentelemetry-otlp). Use exact pins (`=
"X.Y.Z"`) on every crate
in the pair, not the caret default, and put a one-line comment at
each pin naming the partner crate and the contract they share.
The diagnostic for "did this happen?" is `cargo tree -d` (look
for the same crate at two versions) or
`cargo metadata | jq '.packages[] | select(.name |
startswith("pyo3"))'`. A `cargo update` PR that bumps one crate
without the other should fail review immediately — the lockstep
is more important than chasing the latest patch.

---

## 39. `#[non_exhaustive]` enum wildcards are required, not tripwires

When an upstream crate marks an enum `#[non_exhaustive]`,
downstream `match` expressions outside that crate must include a
wildcard arm — the compiler refuses to compile an exhaustive
match without one. The wildcard is a *legal requirement*, not a
hook for future audits. Comments that describe the wildcard as a
"tripwire" that will fire when a new variant lands are wrong
twice over: the compiler accepts new variants silently (they hit
the wildcard), and the variant's downstream classification
defaults to whatever the wildcard maps to — usually the most
generic bucket. A reviewer relying on the tripwire framing will
not audit the match on `cargo update`; the regression slips in
unmapped.

**`From<MetricsError> for AnalysisError` claimed its wildcard was
a tripwire** (#265 batch, `e8ec96b` / corrected in `8d7ef17`).
The bindings' mapping from upstream `MetricsError` to the
Python-side `AnalysisError` included a catch-all
`_ => Self::Parse(err)` arm with a comment claiming that
"a `cargo update` that introduces a new variant should be paired
with an explicit arm above so the Python-side taxonomy stays
intentional rather than defaulting." The framing was load-bearing:
it implied a reviewer would notice. But `MetricsError` is
`#[non_exhaustive]`, so the wildcard is *required* outside the
defining crate; a new upstream variant compiles fine, lands in
`Self::Parse`, and the Python exception class silently changes to
`ParseError` until someone manually audits the From impl. The
fix corrected the comment to acknowledge the wildcard's
non-exhaustive requirement and called out that the only real
tripwire would be removing the wildcard — which doesn't compile.

**Lesson:** A "tripwire" is an *exhaustive* match on a closed
enum, where adding a variant produces a compile error at the
match site. `#[non_exhaustive]` forecloses that mechanism by
definition. If you need a real audit signal when an upstream
variant is added, the only options are: (1) opt into `cargo deny`
/ `cargo semver-checks` rules that flag the variant addition; (2)
add explicit named arms for every variant you've audited and
document the default for unmapped ones honestly (not as a
tripwire); or (3) generate the match from the upstream enum's
variants via a build script that fails when the set changes. Do
*not* describe a wildcard arm as a tripwire — the comment will
mislead the next reader, and the next `cargo update` will land an
unmapped variant unaudited.

---

## 40. `#[cfg(unix)] { ... }` inside a test body silently passes on other targets

A `#[cfg(unix)]` attribute placed on an inner block inside a
`#[test]` function compiles to an empty body on non-Unix
targets — and an empty `#[test]` function is a passing test.
The harness sees one more green check on Windows / WASI / any
non-target, but the test exercises zero assertions. The
pattern is one character away from the platform-correct form
(`#[cfg(unix)]` on the `fn` itself, so the test is hidden
cleanly instead of vacuously passing), and it is easy to
write because it reads like "guard the platform-specific
setup" rather than "skip this test off-platform".

**`from_internal_preserves_byte_uniqueness_for_distinct_non_utf8_paths`**
(`big-code-analysis-py/src/batch.rs`; caught in a session draft
prepared for commit `515e840`, never reached `main`). The
audit-tests pass on the `analyze_batch` work caught the draft
test wrapping its entire body in `#[cfg(unix)] { … }`. On
Linux it correctly exercised the byte-uniqueness contract for
non-UTF-8 paths; on Windows it would have compiled to an
empty function — `cargo test` on Windows would report it as
passing, with zero coverage of the dedup invariant. The
committed form hoisted `#[cfg(unix)]` onto the `fn`
(matching the existing pattern at
`analyze_path_rejects_non_utf8_path_by_default` in the same
crate) so the test is emitted only on Unix. A `git show
515e840` of the test therefore shows the corrected shape,
not the bug — the lesson here is the *pattern*, not a
historical regression.

**Lesson:** When a test needs platform-gated *fixtures*
(e.g. `OsStrExt::from_bytes` on Unix, `OsStringExt::from_wide`
on Windows), gate the entire `fn` — not the inner block. An
inner-block `#[cfg]` produces a vacuously-passing test
off-target; the function-level form hides the test off-target
so the harness does not report bogus coverage. Audit any
`#[cfg(target_os = …)]` / `#[cfg(unix)]` / `#[cfg(windows)]`
attribute inside a `#[test]` body — the only correct
placement is on the function attribute stack, alongside
`#[test]` itself.

---

## 41. Clone-based hash/eq tests don't pin the dedup contract — construct two independent instances

A test that builds one instance, clones it, and asserts the
clone hashes / compares equal to the original verifies the
`Clone` derive, not the production constructor's invariants.
`Clone` produces a byte-identical struct by definition, so
the test holds regardless of what the constructor does —
including under regressions that mix per-call state (a static
counter, a UUID, a timestamp) into one of the fields. Two
*independently-constructed* equal-by-value instances are the
only shape that pins the dedup contract production code
relies on: that the constructor produces deterministic
output, and that equal-by-input means equal-by-hash.

**`equal_errors_hash_equal` on `PyAnalysisError`**
(`big-code-analysis-py/src/batch.rs`; introduced in `96fe3ab`
(`feat(bindings-py): analyze_batch + AnalysisError`),
corrected in `515e840`). The audit-tests pass found the test
using `let a = …; let b = a.clone();` for the `Hash` / `Eq`
contract that `set(results)` deduplication promises in
Python. Verified via test-via-revert: perturbing
`new_internal` to interleave a `static AtomicU64` counter
into the `error` field left the clone-based test passing
(counter never advanced because the clone bypasses the
constructor) while the strengthened two-`new_internal` form
correctly failed (each call advanced the counter, the second
instance's `error` differed from the first). The fix
constructs two instances via `PyAnalysisError::new_internal`
and asserts they collide in a `HashSet`.

**Lesson:** Any test that pins a hash/equality contract for
a value type — especially one used as a set / dict / dedup
key — must construct the two compared instances through the
production constructor *twice*, not once-plus-clone. The
clone path tests only the derive; the two-call path tests
the constructor's determinism. Apply the discipline anywhere
`#[derive(Hash, PartialEq, Eq)]` (or PyO3's
`#[pyclass(eq, hash)]`) reaches a downstream consumer that
calls `.contains()`, `.iter().collect::<HashSet<_>>()`, or
any other equality-keyed lookup. Related to lesson #33
(test-via-revert): use the revert technique to verify the
test exercises the constructor path, not merely the language
semantics of `Clone`.

---

## 42. `unreachable!()` at a PyO3 FFI boundary surfaces as `PanicException`, bypassing `except Exception`

Rust's `unreachable!()` macro panics at runtime when reached.
PyO3 catches that panic at the FFI boundary and re-raises it
as `pyo3.PanicException`, which extends `BaseException`
directly — *not* `Exception`. A Python caller's idiomatic
`except Exception:` block (or any of its narrower forms like
`except (TypeError, ValueError):`) does not catch it. Any
function whose docstring promises "never raises on per-file
errors" or any equivalent never-raise contract is silently
broken by an `unreachable!()` arm the moment a future change
makes that arm reachable: the panic aborts the call, every
accumulated result is discarded, and the caller sees an
uncatchable exception. The Rust idiom that reads as
"defensive" is, at the FFI boundary, the inverse.

**`analyze_batch`'s `Ok(None)` arm**
(`big-code-analysis-py/src/batch.rs`; the original `96fe3ab`
shipped with a defensive `PyAnalysisError` fallback, then a
review-remediation pass in `e670f8b`
(`fix(bindings-py): address code-review findings for
analyze_batch`) regressed it to `unreachable!()`, and
`515e840` restored a defensive fallback). The single-file
`analyze_path` bridge returns `Ok(None)` only when
`skip_generated=true` and the file matches the
`is_generated` predicate; `analyze_batch` hard-codes
`skip_generated=false` and therefore treats `Ok(None)` as
unreachable. The `e670f8b` shape was
`unreachable!("bridge layer returned Ok(None) despite skip_generated=false …")`
with a comment claiming it would "fail loudly in
development" — exactly the failure mode this lesson warns
against. A follow-up /review pass flagged it: the contract
`analyze_batch` documents (`never raises on per-file errors`)
demands a structured `AnalysisError` in the result slot, not
a `PanicException`. The fix replaced the panic with a
synthetic `PyAnalysisError` (`error_kind="IoError"`, message
naming the invariant break and telling the operator to audit
`analyze_path` for new skip surfaces) so the never-raise
contract survives any future `analyze_path` refactor that
adds a second skip surface (gitignore filter, size cap,
etc.).

**Lesson:** Refines lesson #5 (no panic on reachable error
paths) with a PyO3-specific corollary: at any FFI boundary
that documents a never-raise contract, even
*unreachable-today* panics violate the contract because PyO3
surfaces them as `PanicException` — outside the `Exception`
hierarchy Python callers' handlers cover. Replace
`unreachable!()` / `panic!()` / `assert!()` on those
boundaries with a defensive structured-error fallback (a
synthetic error in the result slot, an explicit `Err(…)`
branch with a loud message). The fallback should name the
broken invariant in its message so telemetry surfaces it for
triage, but it must not abort the call. Apply this
discipline to every PyO3 `#[pyfunction]` / `#[pymethods]`
that documents partial-success semantics —
`analyze_batch`-style sweeps, bulk APIs, anything where the
contract is "process N items, never short-circuit on a single
failure".

---

## 43. `to_string_lossy()` on a path field promoted into `Hash` / `PartialEq` keys silently collapses dedup

AGENTS.md already forbids `to_string_lossy()` on "identifier
paths" (map keys, JSON output, error correlation). The
non-obvious second-order hazard: a struct field that
participates in a derived `Hash` / `PartialEq` is *de facto*
an identifier the moment a downstream consumer puts the
struct in a `HashSet` or `dict` key — even when the field's
docstring calls it "diagnostic" or "user-facing text".
`to_string_lossy()` substitutes U+FFFD for every invalid
byte, collapsing every distinct non-UTF-8 path with the same
length-and-position pattern onto one rendered string. Two
genuinely-distinct failures then `__eq__`-compare equal,
hash to the same bucket, and silently de-duplicate in the
`set(results)` pattern the API was specifically designed to
support.

**`PyAnalysisError.path` collapsing distinct non-UTF-8 paths
under `set` dedup**
(`big-code-analysis-py/src/batch.rs`; introduced in `96fe3ab`,
corrected in `515e840`). The original `from_internal` used
`path.to_str().map_or_else(|| path.to_string_lossy().into_owned(), str::to_owned)`
for the `path` field; the docstring described the lossy
fallback as "diagnostic only". But `#[pyclass(eq, hash)]`
promoted `path` into the equality / hash key, and the
documented `set(results)` dedup pattern keyed on
`(path, error, error_kind)`. Two distinct non-UTF-8 paths
(e.g. `b"/a\xff"` and `b"/a\xfe"`) both rendered to `/a` +
U+FFFD; their `PyAnalysisError` instances compared equal
under `__eq__`, hashed identically, and silently merged in
`set(results)` — exactly the contract `__hash__`/`__eq__`
was advertised to serve. Verified via the
`from_internal_preserves_byte_uniqueness_for_distinct_non_utf8_paths`
Rust unit test added in the same commit, which constructs
two `PyAnalysisError` instances from byte-distinct non-UTF-8
paths and asserts both `path` strings *and* `PartialEq`
differ. The fix routes the non-UTF-8 fallback through Rust's
`Debug` formatting on `Path` / `OsStr` (`\xNN` hex escapes
for invalid bytes — casing matches Rust's default Debug
output: uppercase hex, so `b"\xff"` renders as `"\xFF"`),
which is byte-preserving and surrounded with double
quotes — a visible cue that the path was not valid UTF-8.

**Lesson:** Audit every struct field that participates in
`#[derive(Hash, PartialEq, Eq)]` or PyO3's
`#[pyclass(eq, hash)]` for lossy rendering. If a string
field can be constructed from non-UTF-8 bytes via
`to_string_lossy()`, `from_utf8_lossy()`,
`escape_default()`, or any other lossy projection, distinct
inputs can collapse to equal hashes — even when the field is
documented as "for display only". The available fixes: (1)
render via a byte-preserving projection like
`format!("{:?}", path)` (Rust's `OsStr` Debug uses `\xNN`
hex escapes); (2) exclude the lossy field from the derive
(custom impl); or (3) carry the raw bytes in a separate
field that participates in the hash. Default to (1) — it
preserves the visual cue and is the smallest change.

---

## 44. Rust's `{:?}` Debug format escapes non-printables as `\u{N}`, which Python's parser rejects

A PyO3 `__repr__` implemented as
`format!("Cls(field={:?})", self.field)` looks correct,
passes every test with ASCII fixtures, and silently breaks
`eval(repr(x))` round-trip on any input containing a
non-printable character. Rust's `Debug` for `str` escapes
characters outside the printable-ASCII range as `\u{N}` —
curly braces, hex codepoint. Python's source parser does not
accept that syntax: it expects `\xNN`, `\uNNNN`, or
`\UNNNNNNNN` (no braces). A single control character
(`\x01`), a multi-byte Unicode codepoint outside the BMP, or
even some characters Rust's `escape_debug` considers
non-printable produces a `repr` that `eval` rejects with
`SyntaxError: 'unicodeescape' codec can't decode bytes`. The
repr's documented "debuggable" property — copy it into a
REPL to reconstruct the value — silently fails when the
input is exactly the kind of weird data a debugger is most
useful for.

**`PyAnalysisError.__repr__` breaking on control-char paths**
(`big-code-analysis-py/src/batch.rs`; introduced in `96fe3ab`,
corrected in `515e840`). The original `__repr__` used
`format!("AnalysisError(path={:?}, error={:?}, error_kind={:?})", self.path, self.error, self.error_kind)`
with a docstring promising that `eval(repr(x))` would
reconstruct an equivalent object. The /review pass and a
follow-up Python test
(`test_analysis_error_repr_round_trips_through_eval_for_non_ascii`)
caught the regression:
`bca.AnalysisError("/tmp/\x01中.py", "boom ሴ", "IoError")`
produced `path="/tmp/\u{1}中.py"` under `{:?}`, and `eval`
raised `SyntaxError` on the `\u{1}` token. The fix routes
each field through Python's own `repr()` builtin —
`py.import("builtins").getattr("repr").call1((&self.path,))?.extract::<String>()?` —
so the output uses `\xNN` / `\uNNNN` / `\UNNNNNNNN` escapes
the parser accepts.

**Lesson:** Any hand-written `__repr__` / `__str__` on a
`#[pyclass]` that handles string fields must delegate the
per-field escape to Python's `repr()`, not Rust's `{:?}`.
Rust's `Debug` escape vocabulary and Python's source-parser
escape vocabulary disagree on non-printable codepoints —
`Debug` emits `\u{N}` (braces, variable-width), Python
accepts `\uNNNN` / `\UNNNNNNNN` (no braces, fixed-width)
and `\xNN`. The mismatch only shows up for inputs containing
control characters or characters Rust's `escape_debug` flags
non-printable; ASCII-only test fixtures never reach the
broken path. Test the round-trip explicitly with
non-printable / non-ASCII / non-BMP inputs — a fixture with
a single control character is enough to expose the failure
mode. The implementation cost is one `py.import("builtins")`
and three `repr_fn.call1((&field,))?.extract()?` per
`__repr__` call; the correctness gain restores the
`eval(repr(x))` contract the docstring almost always
promises.

---

## 45. XML attribute-value normalization collapses raw TAB / LF / CR — emit numeric character references

XML 1.0 §3.3.3 ("Attribute-Value Normalization") mandates that
any whitespace character inside an attribute value other than
the result of a character reference is normalized to a single
space (`U+0020`) by a conforming parser on read. The bytes
survive on disk, but every conforming consumer (Jenkins,
SonarQube, GitLab CI, libxml2-based tooling) sees the value
with `\t` / `\n` / `\r` collapsed to spaces — irrecoverable
data loss the emitter cannot detect through byte inspection.
Numeric character references (`&#x9;` / `&#xA;` / `&#xD;`) are
the spec-blessed escape: they survive normalization because
the value the parser publishes is the post-replacement scalar
(0x09 / 0x0A / 0x0D), not the bytes they came from.

**`XmlAttr::fmt` emitted literal TAB / LF / CR inside
Checkstyle attribute values** (#340, `1dfe7a1`). The source
comment justified the literal pass-through with "CI consumers
are friendlier when newlines stay literal — keep them as-is",
actively misstating the XML spec. The bug was latent because
no production code path fed a path with embedded `\n` / `\t`
into an attribute value today — POSIX permits them in
filenames, and a future multi-line message template would
silently lose its whitespace structure on every consumer. The
fix replaces the three literal arms with `&#x9;` / `&#xA;` /
`&#xD;` writes, and the regression test re-parses the emitted
XML with `quick_xml::reader::Reader` to confirm the
round-tripped attribute value byte-equals the original —
emitter-side byte inspection alone could not have caught the
bug.

**Lesson:** Any new XML writer that uses attribute values for
data (paths, messages, identifiers carrying user-supplied
text) must escape TAB / LF / CR as numeric character
references, not as literal bytes. The conforming-parser
behavior is silent — no error, no warning, just normalization
on read — so the only way to validate the round-trip is to
re-parse with a real parser and compare scalar-for-scalar.
Cite §3.3.3 in the escape function's comment so the next
contributor doesn't revert it on aesthetic grounds.

---

## 46. Source-literal `"ï»¿"` is three Latin-1 codepoints, not the UTF-8 BOM

The string `"ï»¿"` in Rust source is three Unicode codepoints
(U+00EF U+00BB U+00BF) — the three bytes of the UTF-8 BOM
(`EF BB BF`) reinterpreted as individual Latin-1 chars. The
*canonical* UTF-8 BOM that any UTF-8 decoder produces is a
single codepoint, U+FEFF, three UTF-8 bytes long. The two
strings have disjoint `chars()` iterators and `==` returns
`false` between them. The mojibake form arises when content
with a UTF-8 BOM is copy-pasted into source via a
Latin-1-aware editor (or a terminal that mis-decodes the
input) — the visible "ï»¿" glyphs match the BOM bytes
one-for-one, but the underlying codepoints are wrong.
Production code that compares against the mojibake literal
silently misses the canonical form a UTF-8 parser actually
produces, and vice versa.

**`sanitize_identifier`'s BOM check matched only the mojibake
form** (#345, `fed31a4`). `enums/src/common.rs`'s `if name ==
"ï»¿"` was intended to map the UTF-8 BOM token from a
tree-sitter grammar to a stable `"BOM"` identifier. The
literal was the three-codepoint mojibake form; tree-sitter
exposes node kinds as valid UTF-8 strings, so a future grammar
that surfaced a BOM token would return the single-codepoint
U+FEFF form and the branch would silently miss. The
fall-through path landed in the generic character loop where
U+FEFF is not in the punctuation match, hit the `_ =>
continue` catch-all, produced an empty identifier, and
triggered the `Anon{i}` fallback — generating an `Anon<N>`
enum variant instead of the stable `BOM` identifier the code
claimed to emit. Reachable but latent: no grammar in scope
hits this path today. The fix matches both forms explicitly
with `\u{FEFF}` / `\u{00EF}\u{00BB}\u{00BF}` Unicode escapes,
removing the source-encoding dependence.

**Lesson:** Any non-ASCII source literal that exists to match
a runtime value should be written with `\u{...}` escapes, not
as the rendered glyphs. The Rust compiler accepts both forms;
only runtime comparison reveals the divergence. The
mojibake-vs-canonical class of bug recurs any time you
copy-paste BOM / zero-width / right-to-left / Asian-range text
from an editor that mis-decodes the input. Defensive
accept-both is safer than canonical-only when the production
source-of-truth (here, tree-sitter's UTF-8 decoder) is
well-understood; explicit `\u{...}` escapes make the intent
reviewable.

---

## 47. Bound the thread stack to make stack-overflow tests deterministic across platforms

A regression test for an iterative-walk refactor that builds
a fixed-depth synthetic tree and runs the walker without
overflowing the stack is only meaningful if the equivalent
recursive form would overflow the same stack. Libtest spawns
each test on a thread whose stack size is governed by Rust's
spawn defaults (historically 2 MiB, but overridable via
`RUST_MIN_STACK` and not stable across Rust versions or build
profiles). A recursion frame for a small walker (`&FuncSpace`,
an `out: &mut Vec`, a few locals) is roughly 150–250 bytes;
in release builds with inlining and tail-call collapsing,
10_000 frames may fit comfortably in 2 MiB and the test passes
against the very bug it claims to catch. Spawning the test
body on a thread with a deliberately tiny stack
(`std::thread::Builder::new().stack_size(256 * 1024)`) makes
the failure mode deterministic: any recursive descent at DEPTH
overflows the budget regardless of platform or optimization;
the iterative form's working memory is independent of
recursion depth and succeeds. The Drop side needs the same
care — `FuncSpace` contains `Vec<FuncSpace>` so dropping the
chained tree walks one frame per level and overflows the same
tight stack at function exit. `std::mem::forget` on the
test-local tree sidesteps the Drop-side overflow; the OS
reclaims memory at process exit, which is fine for a test.

**`deeply_nested_spaces_do_not_overflow_stack` initially used
DEPTH=10_000 on libtest's default stack** (#338's regression
test, hardened in review-fix `940a56a`). The first attempt
pinned the iterative walk via the count-and-name assertions
but left the bug-side failure (the reverted recursive form)
at the mercy of platform stack defaults. A code review pass
flagged that release-mode optimization could leave 10_000
small frames fitting in 2 MiB; the test would then pass
against the reintroduced bug, violating the test-via-revert
rule in `.claude/rules/testing.md`. The fix spawned the body
on a 256 KiB-stack worker via `std::thread::Builder` and
bumped `DEPTH` to 50_000 so the budget is overwhelmed under
every optimization level. A trailing `std::mem::forget(current)`
keeps the chained-tree Drop from overflowing the same tight
stack on test exit and masking the production-side assertion
with a Drop-side abort.

**Lesson:** Stack-overflow regression tests must spawn the
walker on a thread with `stack_size` explicitly bounded — not
on libtest's default. The bound should be tight enough that
any plausible recursive descent at the test's chosen DEPTH
overflows it under every realistic compiler optimization, and
loose enough that the iterative form's working memory fits.
If the structure under test has a recursive `Drop`, route the
root through `std::mem::forget` after the assertions or
iteratively unwind it before returning — otherwise a
Drop-side overflow on test exit shadows the production-side
correctness check and the test fails for the wrong reason.

---

## 48. Hand-written enum lists need a match-based companion to enforce exhaustiveness

A `const FOO: &[Enum] = &[Enum::A, Enum::B, ...]` looks like
"every variant in the enum" but the Rust compiler does not
enforce it. Adding `Enum::C` without extending the array
compiles cleanly; only `match` expressions on `Enum` trigger
the `non-exhaustive patterns` error. Tests that drive from the
hand-written list — parameterized round-trip tests, dispatch
tables, name-lookup matrices — silently lose coverage for the
new variant until some unrelated `match` arm forces the
contributor to remember. The fix is a private guard function
near the array whose body matches every variant with `=> ()`:
the match arms must be kept in lockstep with the array (and
with any other hand-written list of variants), and the
compiler enforces it the moment a new variant lands.
`#[non_exhaustive]` does not weaken the guarantee — within
the defining crate, exhaustiveness is still checked; only
cross-crate matches require the wildcard (the
opposite-direction concern covered by lesson #39).

**`ALL_VARIANTS` in `src/metric_set.rs::tests` was advertised
as compile-error-on-drift but was not** (#339, hardened in
review-fix `654f24c`). The original doc comment claimed "a
newly-added variant surfaces as a compile error here". Five
tests — `from_str_round_trips_every_variant_display_name`,
`names_table_parses_to_every_variant`,
`distinct_bits_per_variant`,
`all_variants_round_trip_through_all_contains`, and
`storage_width_covers_every_variant` — iterated over the
list. Adding `Metric::Foo` without extending the array would
silently lose coverage for the new variant in all five tests
until `Display`/`FromStr`'s `match self` surfaced the omission
through an unrelated path. The fix added a sibling
`fn _all_variants_exhaustive_guard(m: Metric) match m {
Metric::A | Metric::B | ... => () }` whose match arms must be
extended in lockstep with the array; a missing arm fires
`E0004: non-exhaustive patterns` under `cargo test` (which
runs as part of `make pre-commit` and CI).

**Lesson:** Any time you maintain a hand-written list of enum
variants — for parameterized tests, dispatch tables,
name-lookup matrices, or "the canonical iteration order" —
add a co-located match-based guard whose arms list every
variant. The guard does not need to be called; the compile
error is the guarantee. `#[allow(dead_code)]` is the right
attribute for the function. Note the placement: a guard
inside `#[cfg(test)]` only fires under the test target, so
the validation gate (`cargo test --workspace --all-features`,
which `make pre-commit` and CI run) is what catches the
drift — a bare `cargo build` will not. Cite that placement
in the guard's doc comment so a future reader knows the guard
isn't compiled out of production builds for any other reason.

---

## 49. Unused `macro_rules!` captures are documentation lies that survive every refactor

`macro_rules! foo { ( $( ($camel:ident, $name:ident) ),* ) =>
{ ... } }` accepts a tuple per call-site entry, but if the
expansion body never expands `$name`, the second tuple element
is decorative. Worse, decorative is not neutral: the
call-site `(Cpp, tree_sitter_cpp)` *looks* declarative — like
the macro dispatches to `tree_sitter_cpp::LANGUAGE` — when in
fact the hand-rolled body picks a completely different crate.
The declared intent and the production code path diverge
silently, the disagreement is invisible to `cargo build` and
`cargo test`, and a code reader trusting the call-site syntax
draws the wrong conclusion about what crate the variant
resolves to. The remediation is one of two: (a) drop the
unused capture from the macro signature and the call-site, so
the syntax matches what the body actually uses; or (b) wire
the capture through the body so the call-site becomes
load-bearing and disagreement becomes a compile error.

**`enums::mk_get_language!` captured `$name` but hardcoded
every match arm** (#344, `0b417f2`). The `mk_langs!` driver
in `enums/src/languages.rs` listed `(Cpp, tree_sitter_cpp)`,
`(Mozjs, tree_sitter_mozjs)`, etc. — a declarative-looking
tuple table that pinned each variant to its backing
grammar-crate ident. But `mk_get_language!`'s expansion was a
21-arm hand-written `match` where `Lang::Cpp =>
tree_sitter_mozcpp::LANGUAGE.into()` (a different crate from
the call-site's declared `tree_sitter_cpp`). The decorative
ident drifted silently — verified via `cargo tree`:
`tree-sitter-cpp` was pulled in only as a transitive of
`bca-tree-sitter-mozcpp`, never directly. Option A applied:
the second tuple element was dropped from the macro
signature, the call-site collapsed to a bare `Cpp`, and
non-obvious mappings (`Cpp` → mozcpp, `Mozjs` → mozjs, the
vendored `bca-tree-sitter-*` forks, the `LANGUAGE_TYPESCRIPT`
/ `LANGUAGE_PHP` per-language consts) now carry per-line
`// -> <crate>` comments anchored to each entry per
[macro-comments.md](../../.claude/rules/macro-comments.md).

**Lesson:** Audit every `macro_rules!` capture against the
expansion body during review. A capture the body never
expands is a documentation lie — the call-site syntax says
the value matters when it doesn't, and the drift is invisible
to every standard gate. Two acceptable fixes: drop the
capture (so the syntax matches the semantics), or wire it
through the body (so the syntax becomes load-bearing). Pick
the easier one. The cost of dropping is one call-site sweep
and a follow-up annotation pass to preserve the per-call
rationale comments; the cost of wiring through is
occasionally needing import aliases or a third tuple element
for special-case constants (`LANGUAGE_TSX` etc.). When the
macro is hand-rolled because variants need bespoke per-arm
logic, drop the unused capture and lean on
[`.claude/rules/macro-comments.md`](../../.claude/rules/macro-comments.md)
to preserve the narrative at the call-site.

---

## 50. Independent dispatch paths counting the same event mask each other's bugs

When a metric has two structurally-independent paths that contribute
to the same headline count — a *token-arm* path classifying single
AST node kinds and a *walker-arm* path descending through container
nodes, or a *structural* arm opening a `FuncSpace` and a *body* arm
summing inside it — both paths add into the same `Stats` field.
Tests that exercise inputs covered by *either* path read the right
total and pass. The dead path is invisible from the result alone;
only an input the alternate path cannot classify exposes it. This
is distinct from lesson #19 (a missing arm in a single dispatch
table) and lesson #7 (an *upstream* filter masking the buggy code
from the test input) — here the arm is present, the input reaches
it, and the test still passes because a *parallel* path summed the
same count by a different route.

**C# `csharp_walk_for_conditions` was dead code for every existing
test** (#370, `6384590`). The `IfStatement` / `WhileStatement` /
`DoStatement` dispatch arms in `src/metrics/abc.rs` targeted
`csharp_inspect_child(node, 1, …)` / `csharp_inspect_child(node, 3,
…)`, which in tree-sitter-c-sharp's grammar shape land on the
literal `(` and `while` token children, not the condition expression
(condition lives at child(2) for if/while, child(4) for do-while).
Every C# ABC test (`csharp_if_single_conditions`,
`csharp_while_and_do_while_conditions`, …) used a comparison
operator inside its condition — `if (x > 0)`, `while (x < 10)` —
and the comparison tokens (`GT`, `LT`, `EQEQ`, …) were counted by an
*independent* token-arm path in the same metric. The if/while/do
helper contributed zero on every test input. The bug existed since
C# language support landed and survived the #369 refactor
(monolithic compute → per-category helpers, `f8b8829`) verbatim
because the refactor preserved dispatch shapes without altering
input coverage. The dead arm could only be exposed by a
bare-identifier condition (`if (x)`) or unary `!` (`if (!x)`) —
input shapes the token-arm path cannot classify.

**The same masking pattern surfaced again on `BooleanLiteral` while
reviewing the #370 fix** (#371, `efe38b7`; Groovy follow-up
`f132990`). The new `csharp_count_condition` helper matched the leaf
tokens `Csharp::True` / `Csharp::False` but not the
`Csharp::BooleanLiteral` wrapper the grammar interposes when `true`
/ `false` appear as a condition. `if (true)`, `while (false)`,
ternary `true ? a : b`, and `!true` all scored 0 conditions — but
only when no other condition token also fired in the same
statement, because the existing ternary `?` / comparison-operator
token arms covered most real test inputs. Discovered during
`/rust-optimize` review of the #370 fix, not from a user report.
Same root cause (dead walker arm, masked by alternate token path),
different node-kind shape (wrapper vs. literal vs. child-index),
all within one week of activity.

**Lesson:** When a metric has multiple independent code paths
summing into the same field, write at least one regression test
whose input *only* the path-under-test can classify — bare
identifiers for a walker arm that handles `!`/paren-wrappers, an
empty container for an arm that descends into children, a
single-arm `switch` for a container-vs.-arm counter. Test-via-revert
each new arm independently (per lesson #33) and confirm it fails
when that *one* arm is dropped — a passing test against an
alternate-path-firing input proves nothing about the path you just
wrote. When auditing or refactoring an existing metric, identify
every independent path that contributes to the same field and
ensure each has at least one input no other path covers; symmetric
paths whose test fixtures all happen to exercise both will pass
even after one path is dead-coded.

---

## 51. Hand-rolled match arms drift from their enum list without an integration coverage guard

A `macro_rules!` macro that hand-codes one match arm per variant
(`mk_get_language!`, `mk_get_language_name!`) is *correspondence by
convention* with the variant list its companion macro emits
(`mk_langs!`). The two share no compile-time tie: a typo in one
arm's backing crate, a missing arm for a newly-added variant, or a
copy-paste that resolves `Cpp` to `tree_sitter_mozjs` all type-check
fine and ship silently. The bug surfaces only at runtime, when the
wrong dispatch result reaches a caller — and if the crate is
workspace-excluded (lesson #15), even `cargo test --workspace` won't
exercise it. Distinct from lesson #15 (excluded crates drift outside
*lint* gates) and lesson #48 (hand-written enum lists need a
*match-based* exhaustiveness companion): here the runtime *dispatch
target* — which backing crate, which name — has no compile-time
check against the variant list it claims to cover.

**Removing the unused capture (#49) fixed the documentation lie;
the dispatch table still had no runtime coverage** (#344, fix
`0b417f2`; integration coverage added in #350, `0f16162`). Lesson
\#49 traces #344's root cause to a `mk_langs!` macro that captured
`(Cpp, tree_sitter_cpp)` but discarded the second element in
`mk_get_language!`'s hand-written match, which actually resolved
`Cpp` to `tree_sitter_mozcpp`. Dropping the unused capture
eliminated the *lie*, but it left every one of the 21 hand-written
match arms untested: a future typo, a missing arm for a newly-added
variant, or a copy-paste that swaps two backing crates would still
type-check and ship silently. The enums crate is workspace-excluded
(no `cargo test --workspace` exercises it) and previously shipped no
`tests/` directory, so the only signal would be a runtime panic when
the wrong dispatch result reached a caller. #350 added
`enums/tests/dispatch.rs` with two load-bearing pieces: (1) a
`lang_<variant>_resolves_to_<crate>` test per `Lang` variant
comparing `get_language(&Lang::X)` to
`tree_sitter_<crate>::LANGUAGE.into()` *directly imported* (not via
`get_language` itself — avoiding the tautology trap), and (2) a
`coverage_every_lang_variant_is_dispatched` guard that iterates
`Lang::into_enum_iter()` and asserts the variant count equals an
`EXPECTED_LANG_VARIANT_COUNT` constant. Test-via-revert: swapping
the `Cpp` arm to `tree_sitter_mozjs::LANGUAGE` makes
`lang_cpp_resolves_to_mozcpp` fail with `Cpp grammar mismatch`;
adding a new `mk_langs!` variant without a per-variant test trips
the coverage guard.

**Lesson:** Any hand-rolled dispatch macro that emits one match arm
per enum variant — `mk_get_language!`, `mk_action!`-style routers,
manually-typed `From<X> for Y` impls over an enum — needs a sibling
integration test that walks every variant of the source enum and
pins the dispatch result to a *directly-imported* reference (the
backing crate's `LANGUAGE`, the canonical string, the expected
behaviour). Compare against the import, not the macro under test,
or the test is tautological. Pair the per-variant tests with a
variant-count coverage guard
(`Lang::into_enum_iter().count() == EXPECTED_VARIANT_COUNT`) so
adding a new variant without extending the test trips the guard.
Workspace-excluded codegen crates need this gate locally (a
per-crate `cargo test` recipe wired into `make pre-commit` / `make
ci`, mirroring `enums-check` from lesson #15) — `--workspace`
doesn't touch them.

---

## 52. Pre-order traversal evaluates parents before children — child-arm state resets fire too late

A state-machine metric that walks the AST in pre-order *cannot* use a
child node's visit to influence the parent's already-completed
evaluation. The model is tempting: "when we see a `!` / `not` /
`NotOperator`, reset the running boolean sequence so the next
combinator scores +1." But pre-order visits the `BinaryExpression`
parent first, evaluates the combinator against the prior-sibling
sequence, and only *then* descends into the `UnaryExpression` operand
where the reset fires. The reset still happens, but the value it was
supposed to alter has already been counted. The arm looks live, the
test suite passes (because the test suite asserts the values currently
emitted, not the values the algorithm claims to compute), and the
intent encoded in the comment quietly diverges from runtime behaviour.
The bug is invisible from any single language module read in isolation
— it is visible only by tracing the *order* of node visits against the
*order* of state mutations.

**`BoolSequence::not_operator()` was dead code at 15 call sites
across 18 language impls** (#392, `0b30837`). Cognitive's
`BoolSequence` state machine had a `not_operator()` method that
called `reset()`, with the documented intent "NOT resets the sequence
so the next boolean always scores +1." Every cognitive impl —
`PythonCode`, `RustCode`, `CppCode`, the `js_cognitive!` macro
(invoked once for each of Mozjs/JavaScript/TypeScript/TSX, so one
source call site expanding to four), `JavaCode`, `GroovyCode`,
`CSharpCode`, `PerlCode`, `KotlinCode`, `GoCode`, `TclCode`,
`LuaCode`, `PhpCode`, `ElixirCode`, `RubyCode` — matched a unary
node (`NotOperator`, `UnaryExpression`,
`UnaryExpression2`, …) and called the reset. In pre-order, the
`BinaryExpression` parent of `!a && !b && !c` was visited *first* —
`eval_based_on_prev` ran against the empty prior sequence and the `&&`
combinator scored its +1 — and only then did the walker descend to the
`UnaryExpression` children where the reset fired. By the time the
reset ran, the `&&` had already been counted, and the only thing the
reset could affect was *future, unvisited* `BinaryExpression` nodes,
which `eval_based_on_prev`'s span check already prevented from
continuing the sequence. Empirically `if !a && !b && !c`, `if *a && *b
&& *c` (the over-broad `UnaryExpression` arm also matched
dereference / negate / bitwise-NOT), and `if a && b && c` all scored
identically in Rust, C++, and Python. The arms were removed wholesale;
the only behaviour change was that nested `a && !(b && c)` collapsed
into a single boolean sequence (the inner `BinaryExpression` visited
after its `UnaryExpression` parent — the one case where the reset
genuinely fired before the value it should affect — now matches the
SonarSource intent that `!` does not start a new sequence). Three
parent-repo snapshots (`csharp_not_booleans`, `php_not_booleans`,
`tcl_not_booleans_nested`) plus five integration snapshots in the
`big-code-analysis-output` submodule absorbed the shift.

**Comprehension nesting relied on a sibling write-back the element node
never saw (#421, `620c5aa8`, refining #417 `0f499b41`).** When Python
cognitive complexity learned to count comprehension `for` / `if` clauses
(#417), those clauses are *siblings* under the comprehension node, not
parent/child, so a `for_in_clause`'s nesting increment was written back
onto the shared comprehension `nesting_map` slot for later sibling
clauses to read. But a comprehension nested in another comprehension's
*element* position (`[[y for y in x if y] for x in xs if x]`) is visited
— pre-order — *before* the outer `for_in_clause` writes its nesting
back, so the inner comprehension's clauses never observed the outer loop
depth and under-counted (6, where the equivalent doubly-nested explicit
loop+if scores 10). #417 shipped this as a documented known-limitation;
the follow-up deleted the `propagate_comprehension_nesting` write-back
and instead derives the depth on the comprehension node itself —
visited first in pre-order, so every descendant inherits it regardless
of sibling order (`comprehension_element_nesting` +
`preceding_for_clauses`). This is the working direction of the rule
below applied deliberately: state established on the ancestor that
pre-order reaches first, read by all descendants — never written back
from a later sibling to an earlier one.

**Lesson:** Any AST-walking metric written in pre-order treats the
parent's combinator as a *completed* value before any child node has
been visited. Arms keyed on a child node that try to influence the
parent's already-computed result — "the `!` resets the sequence,"
"the modifier downgrades the score," "the keyword retroactively
changes the operator class" — are running too late to do what their
comment says. The reverse direction (parent-visit mutates state that
the child-visit then reads) works, but child-visit-mutates-parent-
result does not. When proposing such an arm, write the failing test
*first* (e.g., assert `cognitive("!a && !b && !c") >
cognitive("a && b && c")`); if the test passes against the
current implementation, the arm was probably already dead. If the
test fails, the fix has to live at the token level — dispatch on the
`AMPAMP` / `PIPEPIPE` token (visited after its `UnaryExpression`
siblings in pre-order, not its `BinaryExpression` parent) — not at
the expression level. The dead-code arm should not stay in the
codebase as documentation of intent; it misleads every subsequent
maintainer about what the algorithm actually does.

---

## 53. Positional `node.child(idx)` breaks when the grammar permits an optional preamble slot

Tree-sitter grammars frequently expose statement shapes whose
*positions* differ by syntactic mode: `if (cond)` vs `if (init; cond)`,
`if (cond)` vs `if constexpr (cond)`, `m(value)` (positional) vs
`m(name: value)` (named-argument), `repeat … until cond` with body
present vs body BLANK ALIAS, …. A dispatcher arm that reaches for the
condition via `node.child(1)` works on the form the test fixture
happened to write and silently returns the wrong child on every other
form. The grammar exposes the role-by-name (`child_by_field_name(
"condition")`, `child_by_field_name("value")`, …) precisely because
the position is not load-bearing; positional lookups are valid only
when the grammar guarantees no optional preamble can appear at the
chosen index. Each language's grammar makes a slightly different
choice about which slots are optional, so the bug is per-language and
per-statement-kind, not per-walker.

**Phase 2B condition-slot dispatcher had four positional-child bugs
from one code-review pass** (#395 / #403, `57547a1`, `5db8078`). The
unary-conditional walker was extended from Java/Groovy/C# to 11 more
languages, and a recall-biased review pass across the new code
surfaced four silent miscounts — three closed immediately in
`57547a1` and the deferred Lua finding closed in `5db8078`:
(1) PHP `Argument` wraps both `m(!$a)` (positional, one named child)
and `m(name: !$a)` (named, three children: name, `:`, value); the
dispatcher took `child(0)` (the *name*) and missed the value at the
last child — `m(name: !$a)` reported zero conditions. (2) Go's
`if x := f(); x { … }` puts the `short_var_declaration` at `child(1)`
and the condition expression at `child(2)`; the dispatcher used
`child(1)` unconditionally and counted the assignment instead of the
condition. (3) C++'s `if constexpr (cond) { … }` shifts the
`condition_clause` from `child(1)` (where it sits in the plain `if (
cond)` form) to `child(2)`; the constexpr form returned zero. (4)
Lua's `repeat … until cond` exposes a `condition` field on if /
while / repeat in `tree-sitter-lua`, but the dispatcher used
positional `child(1)` / `child(3)` — fragile to body-BLANK-ALIAS
shifts and unnecessarily so. All four fixes followed the same shape:
switch to `child_by_field_name("condition")` (or the equivalent
field name) when the grammar exposes the field; iterate named
children and pick by role when it does not (the PHP `Argument`
case — `child_by_field_name("name")` and "last named child" together
distinguish the two forms). The bugs survived the Phase 2B feature
commit (`11bf750`) and a simplify-rust pass (`5153f19`) and an
audit-tests review pass (`e896a7b`) because no pre-existing test
exercised the optional-preamble form for any of the four languages —
the fixture corpus had grown around the simpler shape.

**Lesson:** When writing a dispatcher arm against a tree-sitter
statement node, prefer `child_by_field_name(role)` over positional
`node.child(idx)` for any slot whose grammar permits an optional
preamble (init-statement, constexpr-keyword, async-modifier,
named-argument label, BLANK ALIAS bodies). The field lookup is
grammar-version-robust — when upstream tree-sitter re-orders or
inserts a new optional slot, the field name carries; the positional
index does not. If the grammar does not expose a field for the slot
you need (some grammars expose `condition` on `if` but not on
`while`, or vice versa), iterate `node.named_children(cursor)` and
pick by role with an explicit comment naming the variant set you
verified against the grammar. The minimum new-test bar for a new
dispatcher arm is *at least one fixture exercising every optional
preamble the grammar permits* — `if (cond)` and `if (init; cond)`,
`if (cond)` and `if constexpr (cond)`, `m(positional)` and
`m(named: value)` — not just the form the existing corpus already
has. The drift surface is per-language; the fix shape is uniform
(field-name lookup); the test discipline is per-arm.

---

## 54. A "no-op regen" must be proven by an actual regen + diff, never asserted — and a marker is not the artifact

A version marker, a baseline file, or a pin records an *intended*
version; it is not evidence that the generated artifact was actually
rebuilt at that version. A gate that compares one declaration against
another (marker string vs baseline string) stays green over a stale
artifact, because nothing in the comparison ever touches the bytes the
artifact is built from. The same trap catches a human who eyeballs
"what the regen changed" and commits the subset they expected instead
of the generator's complete output. In both cases the committed
generated file was never produced by a real run of the generator at
the declared version — and the discrepancy is invisible until someone
finally runs the tool and diffs.

**The bundled `tree-sitter-mozjs` parser was stale at JavaScript
0.23.1 for months while every declaration claimed 0.25.0** (#407,
`48bc293`; root cause in #1207 / #400). The `tree-sitter-javascript`
marker was bumped `0.23.1 → 0.25.0` in #1207 without re-running
`generate-grammars/generate-mozjs.sh`, and #400 then pinned the
`grammar-marker-sync` baseline at 0.25.0 on the recorded belief that
"the 0.25.0 regen is a no-op against tree-sitter CLI 0.26.9." That
belief was never verified by an actual regen. The real regen against
the genuine 0.25.0 base grammar rewrites ~110k lines of `parser.c` and
adds the `using` / `await using` explicit-resource-management
declaration (`using_declaration`) that 0.23.1 lacked — `grammar.json`
is ABI-independent, so this is a true base-grammar difference, not an
ABI-14-vs-15 artifact. The
`grammar-marker-sync` gate stayed green the entire time because it
compares the marker in `Cargo.toml` against the baseline string, never
against the bundled `src/parser.c`. The bump turned out metric-neutral
for the existing corpus (no fixture uses `using`), but that was luck,
not design — a drift-marker test (`mozjs_parses_using_declaration`)
now pins the capability so a reversion fails loudly.

**The same root cause shipped a subtler half-regen in
`tree-sitter-mozcpp`** (#406, `c3c58930`; gap fixed in #407,
`48bc293`). That fix advanced `parser.c` (version stamp) and
`parser.h` (dropped a forward declaration) to the 0.26.9 form but
left `src/tree_sitter/array.h` at the pre-0.26 layout — so the crate was
stamped as 0.26.9 output yet a bare `tree-sitter generate` would
re-diff `array.h` every time. The mismatch was caught only because a
*full* regen on a sibling leaf grammar (run for #407) surfaced it: the
runtime-header template (which is grammar-independent and therefore
byte-identical across all forks) did not match mozcpp's committed one.
Committing the subset of generated output you expected to change, in
place of the tool's complete output, is the same stale-artifact bug
wearing a smaller hat.

**Lesson:** Treat any "this regeneration is a no-op / metric-neutral"
claim as a hypothesis that must be discharged by running the generator
at the pinned version and diffing the *full* output — `grammar.json`,
`node-types.json`, `parser.c`, and every file under
`src/tree_sitter/` — never by asserting it from a marker, a baseline,
or a previous contributor's note. When you bump a notification-only
marker, run the matching `generate-grammars/generate-*.sh` in the same
change; when you hand-apply generated output, commit exactly what the
tool emits, not the hunks you anticipated. A drift gate that compares
two declarations (marker vs baseline) gives false confidence unless it
is paired with a test that exercises a construct only the *declared*
version can parse — that test, not the gate, is what actually pins the
artifact.

---

## 55. A complexity score can be a metric artifact — verify it, and clear every gated metric

Before splitting a function because it trips a complexity threshold,
check what the score is actually made of. The headline number may be
dominated by a construct that adds a control-flow edge without adding
anything a reader must reason about — in which case the function is not
genuinely complex, the split is partly metric-gaming, and the durable
justification has to come from elsewhere (readability, testability,
argument count). Attributing the score is the first step of the fix,
not an afterthought.

**Rust's `?` inflates both cyclomatic *and* nexits, and
`dump_tree_helper`'s "cyclomatic 32" was mostly `?` noise (#401,
ce36a04).** Each `?` is a `TryExpression`, which
`src/metrics/cyclomatic.rs:398` counts as a decision point (`expr?`
desugars to a `match` with an early-return `Err` arm — a real CFG edge)
*and* which the Rust `impl Exit` (`src/metrics/exit.rs`) counts as an
exit. `dump_tree_helper` measured cyclomatic 32 / nexits 20, but only
~12 of the cyclomatic was real branching (`if`, the glyph chain, the
child loop) — already under the 15 gate. The other ~20 points were a
linear, easy-to-read sequence of fallible `write!` / `color` calls. The
split was still worth doing (it killed an eight-argument signature and
made the writers unit-testable), but the "32 → 4" headline overstates
the maintainability win. Whether `?` should count toward cyclomatic at
all is a separate metric-design question (#409).

**The proposed split would have failed nexits, because `?` is an exit
too (#401).** The issue's plan grouped the write calls into helpers of
~6 `?` each — fine for cyclomatic, but each carried nexits 6, over the
limit of 5 (and over the 0.95 headroom band). The fix was a `paint`
helper that folds set-color + write into one fallible call, dropping
each writer to ≤3 exits. A split sized only against the *headline*
metric silently breaches a sibling metric that the same construct
feeds.

**Trust `bca-thresholds.toml`, not an issue's threshold table (#401).**
The issue stated the `nargs` limit was 5; it is actually 7. The
original `nargs = 8` was the real breach (fixed by bundling the
recursion-invariant state into a struct), but a helper sized to a
fictional limit of 5 would have been needlessly fragmented. Read the
live config before sizing anything to it.

**Lesson:** When a refactor is driven by a complexity gate, first
attribute the score — re-derive the genuine decision count by hand and
compare it to the measured value. If the gap is a metric artifact (Rust
`?` toward cyclomatic *and* nexits; large string literals toward
halstead; inline tests toward file-level sloc/effort), say so, and base
the refactor on the qualities that actually improve. Then size every
new helper against **all** gated metrics it could trip — cyclomatic,
nexits, nargs, abc, halstead.effort — using the real thresholds from
`bca-thresholds.toml`, not a number quoted in an issue. A single `?`
moves two gauges; a split that watches only one will pass the gate it
was aimed at and fail the one it forgot.

---

## 56. A similarity hash must exclude the dimension it claims to be insensitive to

When you hash a chunk of code to match it "fuzzily" — the same function
after a cosmetic change — the digest must omit precisely the thing the
match is meant to tolerate. A hash that still contains that dimension
silently fails to match the exact case it was built for, and because the
fallback path is the rare one, no common test exercises it.

**The fuzzy-baseline body hash matched everything *except* a rename,
which was its entire reason to exist (#377).** Issue #377's
`--baseline-fuzzy-match` keeps a renamed-but-unchanged function covered
by hashing its body instead of keying on the (now-changed) qualified
symbol. The first implementation hashed the function's full source span
(`start_line..=end_line`) — but the **declaration line carrying the name
is inside that span**, so `fn classify(...)` and `fn categorize(...)`
produced different digests and the rename still surfaced as `[new]`. The
headline feature was a no-op for its headline use case. It only showed
up because the integration test `fuzzy_match_covers_renamed_function`
asserted the *with-fuzzy* run succeeded and it didn't — the unit tests
of the hash (`body_hash_*`) all passed, because they never renamed
anything. The fix elides whole-word occurrences of the function's own
bare name (declaration and recursive self-calls) before hashing, so the
two digests agree; `body_hash_elides_own_name_so_rename_matches` and
`body_hash_elision_is_whole_word_only` pin it.

**Lesson:** A near-duplicate/similarity digest is defined as much by what
it drops as by what it keeps. Enumerate every transformation the match is
supposed to survive — whitespace, the symbol's own name, ordering — and
prove the digest is invariant under each one with a test that actually
applies that transformation. A test that hashes two unrelated strings
and checks they differ does not prove insensitivity; only a
before/after-the-edit pair does.

---

## 57. A structural AST shape is not a semantic identity check — read the source bytes

When a metric needs to know *which* thing a node refers to — is this
write targeting the receiver `self`, is this name the class under
analysis — the node's structural shape is a tempting but lossy proxy.
"An `Attribute` whose first child is an `Identifier`" *looks* like
`self.x`, but it is equally `db.x`, `logger.x`, and every other
`obj.attr`. The shape answers "is this an attribute access," never
"whose attribute is it." When the source bytes are already threaded
through the metric, the proxy is not even cheaper — it is just wrong in
a way no shape-only test reveals.

**Python NPA counted every `obj.x = …` as a class attribute (#412,
`a06a07fa`).** `python_lhs_is_self_attribute` classified an assignment
LHS purely by structure — an `Attribute` node with an `Identifier` first
child — and never read the receiver text, even though `code` was in
scope. So a `Service.__init__` wiring `self.name = "svc"` alongside
`db.connection = None` and `logger.level = "INFO"` reported three class
attributes instead of one; dependency-injection wiring, the most common
shape of `__init__`, inflated NPA directly. The fix reads the receiver
bytes and matches them against `self` / `cls` (`PYTHON_SELF_RECEIVERS`),
borrowing from `code` so the slice doubles as the dedup key. The same
change separated `self.f.g = 1` (writes attribute `g` on `self.f`, not a
new attribute of the class) from `self.g = 1` — a distinction the
structural proxy could never make.

**Lesson:** When a metric's correctness hinges on *identity* — which
object, which name, which symbol — and the source text is available,
compare the bytes. Reserve structural pattern-matching for structural
questions ("is this an attribute access," "is this a loop"). A
shape-only proxy for an identity question passes every test whose
fixtures happen to use the expected identity (every test writing
`self.x`) and silently mis-handles every other receiver; if you must
approximate, document it as an under-approximation and prove the
boundary with a fixture using a *foreign* receiver. Do not reach for
`to_string_lossy()` here (lesson #43) — use `code.get(start..end)` with
explicit bounds.

---

## 58. A wrapper-node + keyword leaf is one operator, not two — and compound operators must guard their leaves

tree-sitter routinely emits an operator as an outer expression node that
*contains* the keyword token as a leaf: `await a()` is an
`await`-expression node wrapping an `await` keyword token; `yield`,
`lambda`, and others follow the same shape. Halstead keys distinct
operators by `kind_id`, so listing **both** the wrapper and the leaf in
the operator arm counts every occurrence twice — inflating `N1` and, via
the two distinct kind ids, `n1`. The inverse trap is the *compound*
operator: `not in` / `is not` parse as a single node that *wraps* the
ordinary `not` / `in` / `is` leaves, each independently a valid operator
elsewhere. Classifying the compound without suppressing its inner leaves
counts `a not in b` as two operators.

**Python Halstead double-counted `await` and split `not in` / `is not`
(#413, `4adf1a24`).** The operator arm listed `Await | Await2` — the
expression node *and* its keyword token — so three `await`s scored
`n1=4, N1=8` instead of `n1=3, N1=5`, while `yield` was already correct
(only the `Yield` node, not `Yield2`), making `await` internally
inconsistent with its sibling. The same arm dropped `lambda`, `match`,
`case`, and `nonlocal` entirely. The fix lists exactly one kind per
operator (the `Await` node, the `Lambda3` keyword token) and classifies
the compound `Notin` / `Isnot` while a guarded arm returns `Unknown`
for `Not | In | Is` **only when their parent is `Notin` / `Isnot`** —
so standalone `not x`, `a in b`, `a is b`, and `for x in y` keep
counting. The guard reads `node.parent()` and falls through to
`Operator` on `None`.

**Lesson:** For any operator a grammar emits as wrapper-node + keyword
leaf, classify exactly **one** kind and verify with `bca ops` that the
occurrence count matches the source. This is the mirror image of
lesson #50 — there, two independent paths summing one field *masked* a
zero; here, one arm listing two aliases of the same token *inflates* the
count — both invisible until you assert the exact operator stream. For a
compound operator that wraps reusable leaves, classify the compound and
parent-guard the leaves to `Unknown` under that compound only; a blanket
suppression of `not` / `in` / `is` would silently drop every legitimate
standalone use.

---

## 59. A rule re-implemented in every language module is a recurring regression class — give it one home

When the same semantic rule is copied into each per-language
implementation, every copy is an independent opportunity to forget it.
The cost is not the duplication itself but the *omission by default*: a
newly added language — or a sibling cloned from a template that predates
the rule — ships without it and silently produces wrong output until its
own bespoke fix lands. The regression history is the smell: when one
issue's fix cites a chain of prior issues that fixed "the same thing" in
other languages, the rule wants a single home.

**The Halstead string-interpolation operand skip was re-patched in nine
sites across seven issues (#420, `0b899836`).** The rule — "a string
literal is one operand *unless* it wraps interpolation, in which case the
wrapper yields `Unknown` because the inner expressions are walked
separately" — was implemented independently in the JS-family macro,
Python, C#, Kotlin, Perl, Tcl, PHP, Elixir, and Ruby `get_op_type`
arms, with three different mechanisms, each added as its own regression
fix: the trail `#180 → #183 → #184 → #191 → #192 → #199 → #277` is the
same skip rediscovered per language. Any new interpolating language
double-counted the wrapper into `N2` by omission. The fix introduces a
`Getter::string_operand_type(node, interp_kinds)` trait default over a
`Node::wraps_any(&[u16])` primitive; each language supplies only its own
grammar's interpolation child-kind set, so a new language gets the skip
for free. The two bespoke Tcl/PHP multi-kind helpers were retired, their
exact kind sets preserved (PHP's `heredoc_body` one-level descend
included), and the per-language rationale comments kept at each call
site per `.claude/rules/macro-comments.md`.

**Lesson:** When a rule must hold identically across the per-language
modules that deliberately mirror each other, a `Getter` trait default
(or a `Node` primitive) is the place to declare it once; each language
contributes only its grammar-specific kind set. This is distinct from
lessons #48 / #51 (a hand-rolled *list* drifting from its enum) — here
the duplication is *behavioral*, and the compiler gives no signal when a
language omits it. A recurring-regression issue trail (one fix per
language for "the same bug") is the trigger to consolidate; a pure
refactor like this is verified by zero snapshot drift across every
affected language, including the integration snapshots.

---

## 60. A metric-computation change is only validated workspace-wide — downstream crates pin the numbers

When a change alters how a metric is *computed* — a merge convention, a
divisor guard, a new counted node kind — the library's own test suite is
not the whole story. `big-code-analysis-cli`, `big-code-analysis-web`, and
`big-code-analysis-py` each assert concrete metric values: the web crate's
`test_web_metrics_json` compares a full serialized metrics blob byte-for-byte,
and the py SARIF / threshold tests pin per-metric numbers. A `cargo test -p
big-code-analysis` run never compiles, let alone exercises, any of them, so a
value shift the library's own (often snapshot-anchored) tests accept can leave
a downstream crate's hardcoded expectation stale — and the regression merges
green.

**The #437 LOC min/max fix went stale in the web crate, invisible to a
lib-only review** (#437, `cbe18b21`; fix `bdc44a13`). Making `compute_minmax`
include each container's own span legitimately raised the unit-level
`sloc_max` / `cloc_max` / `blank_max`, which shifted `test_web_metrics_json`'s
expected JSON. The review for that wave ran only `cargo test -p
big-code-analysis` and merged the change clean; the failure surfaced only at
the full-workspace gate. A subsequent agent then dismissed it as
"pre-existing" without checking `main` — but it passed on `main` and failed on
the branch, which is the definition of a regression, not a pre-existing
failure.

**Lesson:** After any change to metric computation or AST traversal, validate
with `cargo test --workspace --all-features`, never just `-p
big-code-analysis` — the downstream crates that pin metric numbers are exactly
the ones a library-scoped run skips. And "fails on the integration branch" is
not "fails on `main`": bisect against `main` before calling a failure
pre-existing. A downstream assertion that is green on `main` and red on your
branch is your regression to fix, not a background condition to step around.

---

## 61. The label-child node kind is grammar-specific — a copied kind-gate silently no-ops

A predicate that gates on a child node's *kind* — for example, scoring a jump
as an unstructured (cognitive +1) jump only when it carries a label,
`BreakStatement | ContinueStatement if node.is_child(Identifier as u16)` —
bakes in one grammar's kind id for that child. The *semantic* child (a jump
label, a condition, a name) is shared across languages, but the node *kind* it
surfaces under is per-grammar. Copying a sibling language's gate verbatim into
another language compiles, runs, and matches nothing, because the kind id it
names never appears under that grammar's construct. There is no compiler or
clippy signal, and no test failure unless a fixture exercises the exact gated
shape — an unlabeled-only test suite scores the gated branch at zero forever.

**The SonarSource jump-statement fix had to use a different label kind for
each grammar family** (#435, `e81b3f31`). Adding labeled-`break`/`continue`
gating to the shared `js_cognitive!` macro required
`is_child(StatementIdentifier as u16)`: JS-family labels surface as
`statement_identifier`, not the `Identifier` that Java and Groovy use, nor
Go's `LabelName` or Perl's `Label`. Copying Java's `is_child(Identifier)` into
the JS macro would have silently scored every `outer: for (…) { break outer; }`
at +0. The fix was verified to resolve `StatementIdentifier` correctly for all
four enums the macro instantiates (Javascript / Typescript / Tsx / mozjs)
before relying on it.

**Lesson:** Before reusing a sibling's child-kind-gated predicate in another
language, dump the AST for the construct in the *target* grammar and confirm
the gating kind actually appears there — the semantic role transfers, the node
kind does not. Add a fixture that exercises the gated branch (a labeled jump,
not a plain one) and test-via-revert that branch alone; a suite of only
unlabeled inputs proves nothing. This is the cousin of lesson #53 (a positional
`child(idx)` breaking on an optional grammar slot): both are "the grammar's
actual shape is not the shape you assumed from a sibling."

---

## 62. Recovering a poisoned `Mutex` with `into_inner()` alone leaves it poisoned — `clear_poison()` is what stops the cascade

`Mutex` poisoning is sticky. When a thread panics while holding the guard, the
lock is flagged poisoned and *stays* poisoned: every later `.lock()` returns
`Err(PoisonError)`. The idiomatic recovery, `lock().unwrap_or_else(|e|
e.into_inner())`, hands back the inner data for *this* acquisition but does not
clear the flag. So if more than one site acquires the same lock — a worker pool
plus a final aggregation read, say — recovering in one place fixes only that
place; every other acquirer still sees `Err` and, if it does `.unwrap()` /
`.expect()`, re-panics. The cascade you set out to stop continues one frame
later, often in a *different crate* from the one you patched, so a green
library-crate test hides it.

**Degrading `Count::call` on a poisoned `stats` mutex needed `clear_poison()`,
not just `into_inner()`** (#445, `995c6fbb`). The library worker
`Count::call` aggregates into a shared `Arc<Mutex<Count>>`; a panicked peer
poisons it and the old `.lock().unwrap()` cascaded a pool-wide abort — the same
hazard #425 fixed in `dispatch_preproc`. A bare `into_inner()` recovery would
have let `Count::call` return `Ok(())`, but the CLI's final
`run_command_count` reads the same lock with `into_inner().expect(...)` and
would have re-panicked on the still-poisoned flag. The fix was
`lock().unwrap_or_else(|poisoned| { cfg.stats.clear_poison(); poisoned.into_inner() })`;
`clear_poison()` is the load-bearing call, and the regression test asserts
`!stats.is_poisoned()` after the call — not just no-panic — so reverting to a
bare `into_inner()` fails the test on that exact line.

**Lesson:** When you degrade rather than propagate on a poisoned shared lock,
enumerate *every* site that acquires that lock — across crates — before
deciding the recovery is complete. If any downstream acquirer would re-panic,
the local `into_inner()` is a half-fix; call `clear_poison()` so peers and
downstream readers see a usable lock. Anchor the regression test on the
poison-cleared invariant (`!is_poisoned()`), not merely on the absence of a
panic, or a bare-`into_inner()` regression slips through. Recovery is only
justified when the guarded data tolerates a partial peer update — here, two
monotonically-incremented counters where the worst case is a slight undercount,
never an unsafe state.

---
