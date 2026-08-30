# Lessons Learned

Hard-won principles from debugging real bugs in this workspace. Each
entry is grounded in specific issues and pull requests and is written to
be re-applicable to future work — not a postmortem of one incident.

**Each entry leads with `**Lesson:**` — the instruction.** What follows
is the mechanism that makes the failure invisible, then the evidence. If
you arrived here from a `(lesson #N)` comment in the source, the first
paragraph is what you came for.

**Standing obligations live in rules, not here.** A lesson is evidence
that a bug class is real; a rule is what you follow without re-reading
the evidence. Three rule files carry the checklists these lessons
produced, and entries below point at them rather than restating them:

- [`.claude/rules/grammar-dispatch.md`](../../.claude/rules/grammar-dispatch.md)
  — per-language `match node.kind_id()` arms, aliased variants, child
  addressing, cross-predicate parity.
- [`.claude/rules/testing.md`](../../.claude/rules/testing.md) —
  test-via-revert, perturbation, coverage-vs-protection, harness
  normalisation, assertion shapes that are wrong by construction.
- [`.claude/rules/formatting.md`](../../.claude/rules/formatting.md) —
  rustfmt's silent bail on in-pattern comments.

Project policy lives in [`AGENTS.md`](../../AGENTS.md). Where a lesson's
takeaway became policy, the entry keeps its evidence and points there;
the policy is not duplicated, because the copy nothing enforces is the
one that drifts.

New entries cite issue and PR numbers, not commit hashes: this
repository rebase-merges, so a hash written alongside the change it
describes is rewritten when the PR lands. Older entries still carry
hashes and are left as they are.

New entries are appended at the end with the next sequential number.
Roughly sixty files — including production source, the cross-language
parity tests, `AGENTS.md`, `CONTRIBUTING.md`, the book, and the
`Makefile` — reference lessons by number. **Renumbering is a breaking
change.** When two entries merge, the text survives under the lower
number and the higher number stays as a redirect.

## Index

| # | Lesson |
|---|--------|
| [1](#1-trait-implementations-are-not-metric-implementations) | A no-op `implement_metric_trait!` arm emits zero with no signal |
| [2](#2-tree-sitter-aliases-one-rule-across-many-kind_ids--match-every-variant) | Tree-sitter aliases one rule across many kind_ids |
| [3](#3-per-language-modules-mirror-each-other--fix-the-bug-in-every-sibling) | Per-language modules mirror each other — fix every sibling |
| [4](#4-halstead-n1n2-and---ops-come-from-different-stores--keep-them-in-sync) | Halstead `n1`/`n2` and `--ops` come from different stores |
| [5](#5-library-code-must-not-panic-on-reachable-error-paths) | Library code must not panic on reachable error paths |
| [6](#6-snapshot-tests-pin-behaviour-not-correctness) | Snapshot tests pin behaviour, not correctness |
| [7](#7-test-infrastructure-deserves-the-rigor-of-production-code) | Test infrastructure deserves the rigor of production code |
| [8](#8-integration-snapshot-drift-hides-in-the-submodule-not-the-parent) | Integration snapshot drift hides in the submodule |
| [9](#9-the-grammars-root-may-not-be-unit--push-a-synthetic-wrapper) | The grammar's root may not be `Unit` |
| [10](#10-same-language-construct-different-ast-shape--detection-must-be-grammar-aware) | Same construct, different AST shape across grammars |
| [11](#11-the-same-metric-across-languages-must-agree-on-the-same-logical-construct) | The same metric must agree across languages |
| [12](#12-tree-sitter-nodechildrencursor-resets-the-cursor-to-self) | `Node::children(cursor)` resets the cursor to `self` |
| [13](#13-tokiotaskspawn_blocking-is-uncancellable) | `tokio::task::spawn_blocking` is uncancellable |
| [14](#14-forked-language-enums-collapse-via-shared-identifiers) | Forked language enums collapse via shared identifiers |
| [15](#15-workspace-excluded-crates-drift-outside-every-workspace-scoped-gate) | Workspace-excluded crates drift outside every gate |
| [16](#16-shellchecks-default-severity-is-style-not-warning) | shellcheck's default severity is `style`, not `warning` |
| [17](#17-merged-into-lesson-15) | *merged into 15* |
| [18](#18-cargo-clippy---fix-is-one-lint-at-a-time) | `cargo clippy --fix` is one lint at a time |
| [19](#19-metric-dispatch-enumerates-kinds--missing-arms-score-valid-constructs-as-zero) | Missing dispatch arms score valid constructs as zero |
| [20](#20-pathbufjoinabsolute-silently-replaces-the-base) | `PathBuf::join(absolute)` silently replaces the base |
| [21](#21-hidden-rule-alias-nodes-extend-their-byte-range-to-the-shared-delimiter) | Alias nodes extend their byte range past the delimiter |
| [22](#22-text-keyed-semantic-markers-force-trait-signatures-to-carry-source-bytes) | Text-keyed markers force `&[u8]` into trait signatures |
| [23](#23-compensation-constants-in-parity-tests-blind-the-test-to-its-own-purpose) | Compensation constants blind a parity test |
| [24](#24-a-cross-cutting-traversal-feature-must-reach-finalize-and-span-derived-metrics) | Cross-cutting traversal features miss non-accumulated metrics |
| [25](#25-crate-root-pub-use-module-silently-leaks-every-newly-pub-sub-module-item) | Crate-root `pub use module::*` leaks the API surface |
| [26](#26-feature-gating-a-generic-dispatcher-forces-the-return-type-to-widen-to-result) | Feature-gating a generic dispatcher widens it to `Result` |
| [27](#27-share-a-private-walker-across-deprecation-shims-to-keep-them-thin) | Share a private walker across deprecation shims |
| [28](#28-hand-rolled-serialize-with-conditional-fields-must-pre-count-for-cbor) | Hand-rolled `Serialize` must pre-count for CBOR |
| [29](#29-compile-test-api-doc-samples-by-linking-against-a-scratch-crate-not-mdbook-test) | Compile-test doc samples against a scratch crate |
| [30](#30-user-facing-comment-markers-should-match-the-codebases-internal-vocabulary) | User-facing markers match internal vocabulary |
| [31](#31-shared-structural-fixes-need-a-structural-assertion-in-every-per-metric-test) | Shared structural fixes need a structural assertion each |
| [32](#32-source-grep-regression-tests-are-theater) | Source-grep regression tests are theater |
| [33](#33-test-via-revert-proves-coverage-one-slot-at-a-time) | Test-via-revert proves coverage one slot at a time |
| [34](#34-tree-sitter-hidden-rule-variants-exist-in-the-enum-but-never-surface) | Hidden-rule variants exist in the enum but never surface |
| [35](#35-two-predicates-classifying-the-same-node-must-agree) | Two predicates classifying the same node must agree |
| [36](#36-serde_jsonto_value-re-sorts-json-object-keys-via-btreemap) | `serde_json::to_value` re-sorts object keys |
| [37](#37-cpython-oserrorerrno-msg-filename-dispatches-to-the-right-subclass) | CPython `OSError` arity picks the subclass |
| [38](#38-co-pinned-runtime--build-time-companion-crates-must-share-an-exact-patch) | Co-pinned runtime + build-time crates need exact pins |
| [39](#39-non_exhaustive-enum-wildcards-are-required-not-tripwires) | `#[non_exhaustive]` wildcards are required, not tripwires |
| [40](#40-cfgunix----inside-a-test-body-silently-passes-on-other-targets) | `#[cfg]` inside a test body passes vacuously off-target |
| [41](#41-clone-based-hasheq-tests-dont-pin-the-dedup-contract) | Clone-based hash/eq tests pin only the derive |
| [42](#42-unreachable-at-a-pyo3-ffi-boundary-surfaces-as-panicexception) | `unreachable!()` at a PyO3 boundary bypasses `except` |
| [43](#43-to_string_lossy-on-a-path-field-promoted-into-hash--partialeq-keys-silently-collapses-dedup) | Lossy rendering in a `Hash` key collapses dedup |
| [44](#44-rusts--debug-format-escapes-non-printables-as-un-which-pythons-parser-rejects) | Rust `{:?}` escapes break Python's `eval(repr(x))` |
| [45](#45-xml-attribute-value-normalization-collapses-raw-tab--lf--cr) | XML attribute normalization collapses TAB/LF/CR |
| [46](#46-a-pasted-bom-literal-is-three-latin-1-codepoints-not-ufeff) | A pasted BOM literal is three Latin-1 codepoints |
| [47](#47-bound-the-thread-stack-to-make-stack-overflow-tests-deterministic) | Bound the stack to make overflow tests deterministic |
| [48](#48-hand-written-enum-lists-need-a-match-based-companion-to-enforce-exhaustiveness) | Hand-written enum lists need a match-based companion |
| [49](#49-unused-macro_rules-captures-are-documentation-lies) | Unused `macro_rules!` captures are documentation lies |
| [50](#50-independent-dispatch-paths-counting-the-same-event-mask-each-others-bugs) | Independent paths summing one field mask each other |
| [51](#51-hand-rolled-match-arms-drift-from-their-enum-list-without-an-integration-coverage-guard) | Hand-rolled dispatch arms need a non-tautological test |
| [52](#52-pre-order-traversal-evaluates-parents-before-children) | Pre-order visits parents first — child-arm resets are late |
| [53](#53-positional-nodechildidx-breaks-when-the-grammar-permits-an-optional-preamble-slot) | Positional `node.child(idx)` breaks on optional preambles |
| [54](#54-a-no-op-regen-must-be-proven-by-an-actual-regen--diff) | A "no-op regen" must be proven by regen + diff |
| [55](#55-a-complexity-score-can-be-a-metric-artifact) | A complexity score can be a metric artifact |
| [56](#56-a-similarity-hash-must-exclude-the-dimension-it-claims-to-be-insensitive-to) | A similarity hash must exclude what it tolerates |
| [57](#57-a-structural-ast-shape-is-not-a-semantic-identity-check) | Structural shape is not a semantic identity check |
| [58](#58-a-wrapper-node--keyword-leaf-is-one-operator-not-two) | A wrapper node + keyword leaf is one operator |
| [59](#59-a-rule-re-implemented-in-every-module-is-a-recurring-regression-class--give-it-one-home) | A rule re-implemented everywhere needs one home |
| [60](#60-fails-on-the-branch-is-not-fails-on-main) | "Fails on the branch" is not "fails on `main`" |
| [61](#61-the-label-child-node-kind-is-grammar-specific) | The label-child node kind is grammar-specific |
| [62](#62-recovering-a-poisoned-mutex-needs-clear_poison-not-just-into_inner) | A poisoned `Mutex` needs `clear_poison()` |
| [63](#63-opening-a-funcspace-for-a-method-nested-node-double-counts-the-ancestors-wmc) | A method-nested FuncSpace double-counts the class's WMC |
| [64](#64-a-defaultfallback-arm-exclusion-is-per-construct) | A default-arm exclusion is per-construct |
| [65](#65-removing-a-node-kind-from-is_func--is_func_space-zeroes-its-childless-variant) | Removing a dispatch kind zeroes its childless variant |
| [66](#66-a-control-flow-construct-with-no-dedicated-grammar-kind-escapes-every-kind-based-dispatcher) | Control flow with no dedicated kind escapes dispatchers |
| [67](#67-compute-it-once-is-the-wrong-altitude-when-the-consumers-dont-share-the-transforms-parameters) | "Compute it once" fails on heterogeneous parameters |
| [68](#68-a-branch-that-looks-unreachable-may-need-a-non-obvious-grammar-shape) | An unreachable-looking branch may need an odd grammar shape |
| [69](#69-a-line-prefix-parser-must-disambiguate-structural-markers-from-body-content) | A line-prefix parser must disambiguate by parser state |
| [70](#70-a-string-keyed-metadata-lookup-shared-across-tables-resolves-collisions-to-the-wrong-context) | A string-keyed lookup silently inherits the wrong metadata |
| [71](#71-invalid-input-that-collapses-into-the-not-provided-branch-fails-as-success) | Invalid input collapsing into "not provided" exits 0 |
| [72](#72-a-breaking-change-must-sweep-the-encodings-of-the-old-contract-not-just-its-name) | Sweep the encodings of the old contract, not its name |
| [73](#73-a-filter-that-silently-matches-nothing-is-load-bearing) | A filter that matches nothing is load-bearing |
| [74](#74-a-language-that-owns-no-file-extension-has-no-snapshot-coverage) | A language owning no extension has no snapshot coverage |
| [75](#75-a-metric-assertion-that-passes-under-the-wrong-grammar-verifies-nothing) | An assertion passing under the wrong grammar verifies nothing |
| [76](#76-merged-into-lesson-24) | *merged into 24* |
| [77](#77-issue-references-in-a--doc-comment-leak-into---help-and-the-man-pages) | Issue references in `///` leak into `--help` and `man/` |
| [78](#78-merged-into-lesson-59) | *merged into 59* |
| [79](#79-key-the-derived-digest-not-the-pre-image) | Key the derived digest, not the pre-image |
| [80](#80-an-assertion-that-only-runs-at-release-time-rots-silently) | A release-only assertion rots silently |
| [81](#81-de-recursing-a-traversal-does-not-de-recurse-the-type-it-walks) | De-recursing walks does not de-recurse the type |
| [82](#82-when-several-predicates-need-an-ancestor-propagate-the-chain-not-a-flag-per-predicate) | Propagate the ancestor chain, not a flag per predicate |
| [83](#83-a-categorical-proxy-for-a-positional-property-is-wrong-in-both-directions) | A categorical proxy for a positional property fails both ways |
| [84](#84-a-factual-claim-in-prose-is-untested-code) | A factual claim in prose is untested code |
| [85](#85-coverage-measures-execution-not-discrimination) | Coverage measures execution, not discrimination |
| [86](#86-a-test-helper-that-normalizes-the-value-under-test-blinds-every-caller-at-once) | A helper normalising the observation blinds every caller |
| [87](#87-an-assertion-can-be-correct-and-still-be-about-the-wrong-rows) | An assertion can be about the wrong rows |
| [88](#88-a-text-scan-that-does-not-lex-the-language-measures-noise) | A text scan that does not lex the language measures noise |
| [89](#89-a-positive-enumeration-and-a-negative-filter-differ-on-what-neither-names) | A positive enumeration and a negative filter differ on what neither names |
| [90](#90-re-reading-a-single-consumption-source-yields-empty-not-an-error) | A re-read of a consumable source yields empty, not an error |
| [91](#91-a-gate-can-filter-out-its-own-subject-before-the-check-runs) | A gate can filter out its own subject before the check runs |
| [92](#92-an-optimizations-rationale-can-encode-the-waste-it-optimizes-for) | An optimization's rationale can encode the waste it optimizes for |

---

## 1. Trait implementations are not metric implementations

**Lesson:** When adding a language, audit which metrics genuinely do not
apply (`wmc`/`npa`/`npm` for non-class languages, `nargs` for languages
without formal parameters) and which were merely deferred. A new entry
in `implement_metric_trait!(Cognitive, ...)` — or any of the seven
metric-trait no-op blocks — must be a deliberate decision carrying a
one-line justification, not a leftover from scaffolding. Add a positive
test that exercises non-trivial control flow and asserts a **non-zero**
value before declaring the language done.

Routing a language through the default no-op macro in `src/macros.rs`
satisfies the type system and silently emits zero for every input.
There is no compile-time signal, no runtime warning, and the suite
passes because zero is a valid metric value, not a sentinel for
"unimplemented". Genuine no-ops must stay (`PreprocCode`,
`CcommentCode`, `wmc`/`npa`/`npm` for classless languages) — they just
cannot be the default for a newly-added language.

**Bash Cognitive / Exit / ABC silently zero for every script** (#71,
`d2be869`). Every Bash file in every report read `0` on those columns
regardless of script complexity, and Maintainability Index ranked Bash
as artificially clean. The fix needed real implementations plus a
breaking signature change to `Exit::compute` (now `code: &[u8]`, because
Bash parses `return` and `exit` as ordinary `Bash::Command` nodes
discriminated by source text, not node kind).

**Audit (#188).** The full default-impl matrix is now documented at each
`implement_metric_trait!` invocation site. Every (language, metric) pair
is classified as a *real default* (a comment gives the reason) or a
*placeholder* (a comment cites the follow-up issue and a smoke test pins
the current 0 so the assertion fires when the real impl lands). Note the
bracketed-trait arms (`[Tokens]`, `[Nom]`, `[NArgs]`, `[Mi]`) are *not*
no-ops — they inherit a working trait default; only the named-trait arms
(`Abc`, `Cognitive`, `Halstead`, …) emit silent-zero bodies (#207).

---

## 2. Tree-sitter aliases one rule across many kind_ids — match every variant

**Lesson:** Follow
[`grammar-dispatch.md` §1](../../.claude/rules/grammar-dispatch.md) —
after any grammar bump, regenerate the enum *first* (both manifests in
lockstep), then confirm every numeric-suffix variant of every matched
rule is listed or excluded with a comment. Named-node stability is not
evidence the ids held. When a rule has many aliases, prefer one
`node.kind()` string comparison over enumerating seventeen variants.

When one grammar rule appears in several positions, the generator emits
N distinct kind_ids all mapping to the same `node.kind()` string.
`PrimitiveType` and `PrimitiveType2` are different `u16`s. Code matching
only the unsuffixed variant compiles, runs, and returns wrong numbers —
invisible until a snapshot happens to exercise the aliased keyword or a
downstream metric goes inexplicably low.

Seven instances, one bug class: Rust `PrimitiveType2`–`PrimitiveType17`
unmatched in `is_primitive` and Halstead (#40, `274eb74`); 6 of 9 Java
primitive types missing from Halstead operators (#36, `4e55756`);
JS-family `MemberExpression3`/`4` and `Identifier2` (#50, `c744809`); Go
`BlankIdentifier` and aliased `Identifier2`/`3` (#49, `e884abc`); Bash
`HeredocBody2` — where the *implemented* `HeredocBody` was the id that
never surfaces (#44, `e487a25`); C# `InvocationExpression` /
`ParenthesizedExpression` / `PrefixUnaryExpression` / `VariableDeclaration`
across four files (#94, `f042659`); and JS/TS/TSX `String2` plus a TSX
`String3` for JSX attribute strings that the issue description itself
missed (#119, `fbf047d`). C# is notable because it shipped *after* this
lesson was written — the class applies as much to a fresh language as to
a grammar bump. The reach extends past `getter.rs` / `checker.rs` to
`alterator.rs`, `spaces.rs`, and every file under `src/metrics/`.

**Ruby needs the named clause, not the keyword token** (#190,
`c42edf2`). tree-sitter-ruby emits *two* kinds per control-flow
boundary: a keyword token (`Else2`, `Elsif2`, `When2`) and a named
clause (`Else`, `Elsif`, `When`), plus an implicit `Then` clause around
every `if` body even with no `then` in source. Counting both
double-counts; counting only the keyword misses implicit-`then` bodies.
The Ruby `Abc` impl matches the named clause — a template for any
grammar with this paired shape.

**A grammar bump renumbers anonymous tokens while every named node holds
still** (#519, `ada17475`). tree-sitter-groovy v0.2.2 added a `;`
terminator at kind_id 2, pushing `Shebang`, `COMMA`, `If`, `LPAREN`,
`RPAREN`, `Else`, `While`, `For` each up by one. Named nodes (151+),
operator tokens, and the variant *set* were untouched, so the alias
sweep above finds nothing — but the stale enum claimed `Else = 10` while
the live grammar used 10 = `)`. Cognitive's Groovy impl dispatches on
the `else` keyword token, so it fired on every `)` and missed every real
`else`. This was **not** silent — four cognitive tests went red — and
that is the trap: stable named-node ids tempt you to skip regeneration
and blame your own change. The first hint was `bca dump` showing `{;:2}`
where the enum said `Shebang = 2`.

**An un-emitted alias can cancel a misclassification, so a "compliant"
sibling may be two bugs deep** (#1263). Groovy's operand arm listed
`QualifiedType` (221) while the runtime emits the alias `QualifiedType2`
(228), so the container-as-operand bug being fixed for the JS family
never fired there — and the issue's own survey pronounced Groovy
leaves-only "empirically". Probe a sibling-compliance claim with
`bca ops` on the exact construct rather than trusting a survey, and when
the fix is to *remove* a kind, remove its un-fired aliases too instead
of completing the list.

---

## 3. Per-language modules mirror each other — fix the bug in every sibling

**Lesson:** Before claiming a fix in one module is complete, grep the
siblings for the same identifier and apply the same change. Land them in
**one commit** so the diff makes the symmetry visible; splitting across
PRs hides it. `AGENTS.md` carries this as policy; the recurrence record
below is why it is worth a mechanical check rather than good intentions.

```bash
rg '<symbol_or_match_arm>' \
  src/languages/language_{javascript,mozjs,typescript,tsx}.rs \
  src/{getter,checker}.rs
```

The four JS-family modules are deliberate structural twins — Mozjs was
the original and the rest were forked from it — so a defect in one
usually exists in two to four. The same holds for any fork
(Java/Kotlin, C/C++/Mozcpp) and for the **per-metric** axis.

Five same-omission-four-modules instances: `get_func_space_name`
returning the wrong enum because imports were never updated after the
copy-paste, so anonymous functions rendered in Mozjs's namespace (#37,
`64c80b8`); modern operators `=>`, `...`, `?.` missing from all four
Halstead classifications (#42, `b0e27f2`); `typeof` / `instanceof` /
`void` misclassified as operands (#45, `18f6c48`); and `Do` not counted
as an operator (#35, `68db037`).

**"Fork of Mozjs" is the common case, not a universal** (#38,
`6fd6f79`). `is_else_if` checked `IfStatement` instead of `ElseClause`
in JavaScript and TSX while Mozjs and TypeScript were already correct.
Grep all four; do not assume the bug is everywhere or nowhere.

**The per-metric axis fails the same way** (#227, `e347260`).
`tokens.rs` documented and applied a `usize::MAX → 0.0` sentinel
collapse in `tokens_min()`, and `loc.rs` did the same at three sites —
while `cognitive.rs`, `cyclomatic.rs`, `nom.rs`, `nargs.rs`, `exit.rs`,
and `abc.rs` leaked the raw sentinel (1.8446744e19) straight into JSON
for any space that never observed a value. The `tokens.rs` guard
explicitly *anticipated* the propagation in its doc comment; it had
never landed. A defensive guard added to one file under `src/metrics/`
must be propagated across the family with the same `rg` checklist.

---

## 4. Halstead `n1`/`n2` and `--ops` come from different stores — keep them in sync

**Lesson:** Treat `len(dedupe(ops.operators)) == n1` and
`len(dedupe(ops.operands)) == n2` as a load-bearing invariant. Whenever
you change Halstead classification, add a `kind_id` to `is_primitive`,
or touch finalize / parent-merge, add a regression test that runs both
`metrics()` and `operands_and_operators()` on the same input and asserts
it. When auditing a new language, also check no kind_id is classified as
*both* operator and operand — `HalsteadType` is exhaustive but the
routing in `getter.rs` is not, and a copy-paste can land one kind_id in
two arms.

`HalsteadMaps::operators` is an `IntKeyHashMap<u16, u64>` keyed by
`kind_id`; `--ops` is built from a parallel text-keyed structure plus the
`primitive_types` `HashSet<String>`. Three independent failure modes
have produced visibly disagreeing counts:

**Many tokens collapse to one kind_id** (#31, `2b1083b`).
`Cpp::PrimitiveType` covers `int`/`float`/`double`/`char`/`void`;
`Rust::PrimitiveType` covers `i32`/`u8`/`f64`/`bool`;
`Typescript::PredefinedType` covers `string`/`number`/`boolean`. N
textually-distinct operators collapse to one map entry, so `n1`
undercounts by `N - 1` while `--ops` correctly lists all N. The fix
stores primitive-type operators by source text.

**Parent scopes accumulate without recomputing** (#32, `b12d899`). The
finalize pass merged children into parents but never recomputed parent
ops, double-counting at every nesting level.

**One kind_id in both maps** (`2248bcc`). TypeScript `String2` — the
`string` type keyword — was classified as an operator (correct) *and* an
operand (wrong). Not a collision, but the visible symptom is identical.

---

## 5. Library code must not panic on reachable error paths

**Lesson:** Before writing `unwrap` / `expect` / `panic!` / `assert!` /
`unreachable!()` outside `#[cfg(test)]`, ask: can this branch be
triggered by source the user supplies, a metric value the parser
produces (NaN, infinity, zero), a grammar node the next tree-sitter
version emits, or concurrent state in the web service? If yes, propagate
via `Result`/`Option` or pick a total-order primitive (`f64::total_cmp`
over `partial_cmp`). If no, put the invariant that makes it unreachable
in the `expect` message itself. Tests exercising a panic path must call
the function **directly** with the panicking input — wrapper-level tests
almost always have an upstream filter that masks the bug.

`AGENTS.md` bans these outright; the substance is that Rust makes them
ergonomic enough to slip past review on paths that turn out reachable.
Callers of this published crate treat a panic as an unrecoverable crash.

**`partial_cmp().unwrap()` reached by NaN metric values** (`011c556a`).
Two markdown-report sort comparators used it on `f64` metric fields. A
higher-level `nan_safe_sort_does_not_panic` test asserted the report did
not panic — but a `> 0.0` guard upstream filtered NaN before the sort
ever saw it, so it passed for the wrong reason.

**`unreachable!()` arms become reachable on grammar bumps.** When a bump
emits new aliased kind_ids (lesson 2), match expressions falling through
to an `Unknown` arm are safe; `unreachable!()` crashes. The same applies
to `MetricsFormat` matches in the CLI when a variant is added in one
place and forgotten in another.

Two boundaries refine this: **lesson 42** (at a PyO3 FFI boundary even
an unreachable-today panic breaks a never-raise contract, because PyO3
surfaces it as `PanicException` outside the `Exception` hierarchy) and
**lesson 62** (degrading on a poisoned `Mutex` in one place still lets
every other acquirer re-panic).

---

## 6. Snapshot tests pin behaviour, not correctness

**Lesson:** When writing or accepting a snapshot, ask: "if the code were
wrong in a plausible way, would this snapshot still pass?" If yes,
derive at least one assertion from an external source — the metric
specification, a hand-computed value, or a reference implementation in
another language module — never from the current code's output. Keep at
least one hand-derived test per metric per language as an external
anchor; snapshots are scaffolding around it, not a substitute.

`AGENTS.md` carries the enforceable form of this ("Anchor every
`insta::assert_json_snapshot!` call", with the three acceptable anchor
shapes), and `utils/check-snapshot-anchors.py` gates it against
`.snapshot-anchor-baseline.txt`. What follows is why that gate exists.

`insta` records whatever the code emits when the test is written. If the
code is wrong, the snapshot freezes the wrong value, and
`cargo insta test --accept` rubber-stamps it with no human verification.

**JS-family Halstead snapshots agreed with misclassified operators.**
Issues #35, #42, #45, and #50 each involved an operator wrong in two to
four modules. Every snapshot passed, because each pinned the buggy `n1`
/ `n2` rather than values derived from the Halstead specification. The
bugs survived until someone audited the operator list against language
documentation.

**The human-readable derivation drifts while the snapshot stays right**
(#143, `2799547`). The Tcl `tcl_logical_operators` test carried a
`// &&=1 and ||=1 inside expr; sum=3` comment against an accepted value
of 5 — the comment forgot the outer `if`. The snapshot was correct; only
the prose had drifted, and the mismatch was invisible until the bare
snapshot gained an `assert_eq!(…, 5.0)` immediately above it. A comment
can silently desync from reality; a literal value in source cannot.

For grammar bumps, run `cargo insta test --accept` per file only after
spot-checking that the diff is metric values shifting in a direction
consistent with the grammar change, not structural changes hiding a
regression.

---

## 7. Test infrastructure deserves the rigor of production code

**Lesson:** Every test asserts a specific value or a specific failure,
never `is_ok()` or "the section rendered". When fixing a bug, write the
test against the function whose contract is wrong, not against a wrapper
that may filter the bug-triggering input.
[`testing.md`](../../.claude/rules/testing.md) carries the mechanical
form: the harness-normalisation map, the assertion shapes that are wrong
by construction, and the perturbation procedure that proves a test can
fail.

This is distinct from lesson 6: there the *provenance of the asserted
value* is wrong; here the *structure of the test* is.

**Wrapper-level tests masked by upstream filters** (`011c556a`).
`nan_safe_sort_does_not_panic` drove `generate_report` end-to-end with
NaN inputs; a `> 0.0` filter removed NaN before the sort saw it. Any
test exercising behaviour through a high-level entry point can pass for
the wrong reason whenever an intermediate stage filters, normalizes, or
short-circuits the input.

**Two chokepoints put an entire input class out of reach of the whole
suite** (#1051, `aaddda98`). `Loc` discounts the extra row a Rust
`DocComment` spans when its scanner consumes the trailing newline; at
EOF there is none, so the unconditional `end - 1` underflowed —
panicking in debug, wrapping to `usize::MAX` in release and surfacing
far away as a hash-table capacity overflow. But `check_func_space`
trims trailing newlines and appends one, and the integration suites
reach `normalize_line_endings`, which also ends with an unconditional
`data.push(b'\n')`. "A node ending at EOF" was unreachable from
*either* harness, so a regression test written the ordinary way would
have passed against the unfixed code. The fix added a verbatim helper
calling `analyze(Source::new(..))` — landed as `loc_verbatim`, since
unified into `metrics_verbatim` / `space_verbatim` — carrying a doc
comment on why it must not be "simplified" back to `check_metrics`.
Sharper than the NaN case: there one test's call path
had a filter; here two independent chokepoints normalized the input
class away from the entire suite.

**Presence without value** (`df84dd27`).
`wmc_section_present_with_class_summaries`, `nexits_section_present`,
and `abc_section_present` asserted only that a markdown header
rendered. Wrong WMC values, wrong NEXITS counts, or zero ABC magnitudes
would all have preserved the header.

**Absence assertions that pin the bug** (#681, `296e304a`). A
fully-suppressed hotspot table rendered its omission note with no
heading, and five tests asserted
`!report.contains("### Functions With Many Parameters …")` — passing
*because* the heading was missing, encoding the buggy output as the
contract. Fixing the bug turned all five red, and the reflex to "update
the test to match" would have re-asserted it.

---

## 8. Integration snapshot drift hides in the submodule, not the parent

**Lesson:** `AGENTS.md` carries the four-step submodule discipline as
policy — clean `cargo test` from a fresh tree, accepts committed *inside*
the submodule, those commits pushed to its remote, and the parent
recording the new SHA in the **same commit** as the fix. The incident
below is why step four is not a follow-up and why re-running integration
tests after any rebase is not paranoia.

**`ed8adb6` lost 4 of its 69 cognitive snapshot accepts.** The sibling
boolean-sequence fix landed on parent `main` with a submodule pointer
bump to `4c2a17c2`, which contained all 69 accepts. Later,
`dekobon/big-code-analysis-output` was force-pushed onto a chain that
rebased away the cognitive accepts and kept only the Halstead NaN/Inf
ones (`8bb237d`). The parent still referenced `4c2a17c2`, which no
longer existed on the remote — submodule fetch failed outright on a
fresh clone. After repointing, four correctly-accepted snapshots were
missing: `farcreate.cc`, `farcompilestrings.cc`, `viewer.js`, and
`build.rs`. The fix itself was never broken; the snapshots proving it
were stranded by history rewrites.

---

## 9. The grammar's root may not be `Unit` — push a synthetic wrapper

**Lesson:** Never trust the root node's `kind()` to be the language's
canonical translation-unit kind. When adding or auditing a language,
verify the file-level `FuncSpace` is anchored to the parser's **full
input range** and carries the language's `Unit` kind, not the kind of
whatever the parser happened to return. Assert `blank ≥ 0` for every
fixture in the corpus — the invariant is cheap and catches this whole
class plus arithmetic errors in the LOC computation itself.

Grammars normally return `translation_unit` / `source_file` / `program`
at the root. When input contains constructs the grammar cannot parse,
the parser can return an `ERROR` root or promote an inner declaration
instead. Code adopting the root's span as the file's `FuncSpace` then
reports that inner declaration's span as the file's LOC while child
traversal still aggregates `ploc` from the whole file, violating
`blank = sloc − ploc − only_comment_lines ≥ 0`.

**`tree-sitter-mozcpp` promotes inner declarations on partially
unparseable C/C++** (#80, `dc09eb3`). Four DeepSpeech files reported
nonsense: `model.hh` gave `kind=namespace, sloc=1, ploc=55, blank=−109`,
and both Cython-generated `pywrapfst.cc` files gave a `struct` or
`function` root with `blank` in the tens of thousands negative — frozen
into snapshots long enough to read as background noise.
`getopt_win.h` had been quietly *excluded* from the snapshot test for
the same root cause; the fix re-includes it. The fix pushes a synthetic
`Unit` space at the bottom of the state stack whenever the root kind is
not `Unit`, anchored to the parser's full input range.

---

## 10. Same language construct, different AST shape — detection must be grammar-aware

**Lesson:** Before implementing a semantic check that depends on AST
*structure* rather than node kind, examine the grammar's
`node-types.json` or parse a representative snippet to confirm how the
construct is actually represented. The per-family table is in
[`grammar-dispatch.md` §4](../../.claude/rules/grammar-dispatch.md).
Treat a stub returning a constant as a to-do, not a finished
implementation, and add a test that would fail if it were a no-op — an
`else if` chain must score lower than the same chain with independent
`if` blocks.

Unlike the aliased-variant problem (lesson 2), where one rule generates
several kind_ids, this is *structural divergence*: the node
relationships themselves differ between grammars.

**Java and C# `is_else_if` always returned `false`** (#115, `013bff9`).
The C++/JS-family grammars wrap `else if` in an `ElseClause`, so
`is_else_if` checks the parent. Java and C# emit `else` as a bare
keyword token preceding a nested `if_statement` — no wrapping node — and
both implementations returned `false` unconditionally. Every `else if`
received a nesting increment instead of a flat +1, so cognitive
complexity was systematically inflated: the error grew linearly with
chain length and exponentially with nesting depth, because each false
increment inflated the penalty for everything nested inside. The fix
adopted Kotlin's strategy (`previous_sibling().kind_id() == Else`), and
the post-fix audit of the 16 implementations then in the tree produced
the first strategy table. That table is now maintained in
[`grammar-dispatch.md` §4](../../.claude/rules/grammar-dispatch.md) with
the command to re-derive it — the #115 version went stale within a few
language additions, which is lesson 84 in miniature.

---

## 11. The same metric across languages must agree on the same logical construct

**Lesson:** When adding or touching a metric implementation, write the
fixture in *every* affected language and assert the metrics agree on
logically equivalent code, modulo documented exceptions. One fixture per
language under a shared test — `cyclomatic_cross_language_parity` and
its siblings — is enough; it fails the moment a language drifts.
Per-language snapshots pin behaviour against that language's own
history and cannot detect that two languages disagree. Whenever a
"modified" or "alternative" metric variant is introduced to mask a
per-language quirk, audit the **standard** variant too: the variant
probably exists because the standard one is wrong, and the standard one
is what most consumers read.

Each implementation under `src/metrics/` is written against its own
grammar, not a shared specification. Every language's snapshots still
pass, because each was written against that language's own wrong output.
Lesson 6 covers per-language provenance; this covers cross-language
agreement, which even an externally-anchored single-language test cannot
catch.

**Rust counts wildcard `_ =>`; the C family does not count `default:`**
(#106, `a54b073`). `impl Cyclomatic for RustCode` matched every match
arm including the wildcard, while the equivalent `default:` in nine
C-family languages is intentionally uncounted. Two-branch
`match { 1 => …, _ => … }` reported CCN +2 where the equivalent `switch`
reported +1.

**Bash double-counts `case…esac`** (#107, `e668f14`). It matched
`CaseStatement` *and* `CaseItem`, incrementing for the wrapper and once
per arm, where every other language counts only arms. A 3-arm `case`
reported CCN 6 against C's 5. In both cases the modified-CCN variant
(`16cd610`) papered over the asymmetry and left standard CCN divergent.

**Parity tests found real divergences during fixture authoring** (#211
Bash `28aafd6`; #212 Python `d8ed3b5`; #228 exit `6de7d58`). `e2fbd2b`
wired the four parity tests this lesson prescribes. Bash's bare `*)`
contributed a decision no other language counted; Python's `match`/`case`
was an entire dispatch hole (`cyclomatic_max == 1`, `cognitive_max == 0`
— lesson 19 class); and a later sweep found Python, the JS family, Java,
and C++ all missed `throw`/`raise` as exits. Findings on day one of the
test landing, not latent debt years later.

**The rule extends across metrics, not just languages** (#451
`30a435ae`; #456 `7021f209`; #469 `51725a9f`; #473 `43c1086b`). Where
ABC and cyclomatic must agree on the same construct, reuse cyclomatic's
already-correct gate rather than re-deriving the predicate, and pin
`abc.conditions == cyclomatic() - 1` per space (not `cyclomatic_sum()`,
which carries a per-space base of 1). Excluding the switch `default`
arm from ABC took seven languages sharing one `Default` token, plus PHP
separately through two distinct kinds. See lesson 64 for the trap in
propagating such an exclusion.

---

## 12. tree-sitter `Node::children(cursor)` resets the cursor to `self`

**Lesson:** The cursor passed to a tree-sitter iteration method does not
determine its scope — the node the method is called on does. Whenever a
helper takes a `TreeCursor` and calls `node.children(cursor)` on a node
that isn't the cursor's root, the argument is dead weight. Call
iteration methods directly on the node you want to traverse
(`parent.children(&mut parent.walk())`), and use the parameter only when
you genuinely need to share an allocated cursor across siblings.

`Node::children(cursor)` calls `cursor.reset(self)` before iterating, so
the argument's prior position is silently discarded. The compiler
accepts it — `TreeCursor` has no compile-time binding to a node.

**`Node::has_sibling` was structurally identical to `Node::is_child`**
(#127, `7a0d4ac`):

```rust
self.0.parent().is_some_and(|parent| {
    self.0.children(&mut parent.walk())          // parent.walk() ignored
        .any(|child| child.kind_id() == id)
})
```

The single call site, `check_if_arrow_func!`, invoked
`has_sibling(PropertyIdentifier)` to detect `{ foo: x => x }` shorthand
arrow functions; since `PropertyIdentifier` is never a child of
`ArrowFunction`, it returned `false` unconditionally. The bug was masked
because `count_specific_ancestors` caught the common case by a different
traversal — the dead branch only mattered when the ancestor walk exited
early. Write a unit test that distinguishes "iterates self's children"
from "iterates parent's children" with a fixture where the two disagree;
without it the bug is invisible.

---

## 13. `tokio::task::spawn_blocking` is uncancellable

**Lesson:** `tokio::time::timeout` cancels the *await* of the join
handle and nothing else. Anywhere `spawn_blocking` (or
`actix_web::web::block`) runs against user-controlled input with a
non-trivial worst case, one of three must hold: the work checks for
cancellation periodically and exits early; the server tracks orphaned
tasks and rejects new work once the count crosses a threshold; or the
input is size-bounded so the worst case is a small multiple of the
timeout. **A semaphore alone is not sufficient.** When adding a blocking
endpoint, test that submitting above `blocking_pool_size / timeout_secs`
per second makes the server reject rather than queue.

Dropping the `JoinHandle` does not cancel the task — the Tokio docs say
so explicitly, and `actix-web`'s `web::block` inherits it. Pairing a
semaphore with a timeout bounds neither the blocking pool nor the CPU
time of one request: timed-out tasks release the permit but keep their
thread-pool slot.

**Pathological-source DoS in `big-code-analysis-web`** (#110,
`94c8141`). `run_parse` acquired a permit, called `web::block`, and
wrapped the handle in a timeout. On expiry the handler returned 504 and
dropped the permit while the parse kept running. About 18 req/s at a 30s
deadline saturates the 512-thread default pool, after which every new
request — including healthy ones — queues until an orphan finishes.
Permit limits do nothing, because the bottleneck is the pool. The fix
added an orphan counter that 503s once `BCA_MAX_ORPHANED_TASKS` is
exceeded. The combination *looks* defensive in review precisely because
each piece is correct in isolation; the gap is at the seam.

---

## 14. Forked language enums collapse via shared identifiers

**Lesson:** Whenever a helper branches on the output of a domain enum's
identity method (`get_name()`, `to_str()`), test it **through the enum**,
not through literal strings — a literal test exercises a path production
cannot reach. Before adding a per-variant arm, check whether the identity
method collapses two variants to the same value; if so, the arm is dead.
When the helper feeds a downstream artifact (a CSS rule, a JSON key),
add a test walking every slug the helper can emit and asserting the
artifact exists. And when the enum implements `Display` + `FromStr`, pin
injectivity with one test iterating **every** variant and asserting
`from_str(to_string(v)) == Ok(v)` — a spot-check misses exactly the
variant you forgot.

When two variants are dialects of one language, an identity method
typically collapses them: `LANG::Tsx::get_name()` returned
`"typescript"`. Any helper branching on that canonical name has
unreachable arms, while a unit test driving it with a literal string
happily simulates a path that does not exist.

**`lang-tsx` palette arm dead in the HTML report** (#139, `0a9eca1`).
`language_palette_slug` had explicit arms for `"tsx"` and the stylesheet
shipped `.lang-tsx` rules for light and dark mode.
`language_palette_slug_known_and_fallback` asserted on the helper with
literal inputs and agreed. But production called
`language_palette_slug(lang.get_name())`, and TSX collapses to
TypeScript before reaching the helper. The fix dropped the dead arm,
replaced the `match` with a `LANGUAGE_PALETTE` table an enforcement test
introspects for light+dark rules, and added an end-to-end test through
`LANG::Tsx`.

**The same root, opposite direction** (#265 batch, `182974b`). The first
PyO3 bindings published the lowercased Rust variant name (`"mozjs"`)
while the CLI showed `"javascript"`. Users feeding the CLI's own output
back to the bindings hit `UnsupportedLanguageError("javascript")`. There
production matched a name the enum cannot emit; here the public API
exposed a name the identity method cannot emit.

**The collapse root, finally eliminated** (#540, `57e056d9`). `mk_langs!`
gave both `Tsx` and `Typescript` the string `"typescript"`, so
`FromStr("typescript")` resolved to whichever was declared first —
leaving `LANG::Typescript` impossible to parse or round-trip. The
existing test `aliased_typescript_name_resolves_to_first_declared_variant`
*documented* the collapse as expected, cementing the bug. The 2.0 fix
gave every variant a distinct slug, which also let the Python override
above be deleted, and replaced the per-variant assertions with one
iterating every variant — proving round-trip fidelity and injectivity
together — plus a `no_variant_slug_contains_punctuation` guard.

---

## 15. Workspace-excluded crates drift outside every workspace-scoped gate

**Lesson:** Any crate in `[workspace].exclude` needs an explicit
lint/check target that does **not** go through `--workspace`. That gate
must be (a) invoked from every place the workspace gates are — the local
`make` aggregate, the pre-commit hooks, and CI; (b) carrying the same
`RUSTFLAGS="-D warnings"` posture so local and CI behave alike; and (c)
ideally backed by a sabotage test in CI, so if the recipe ever stops
failing on warnings, that test fires. Add the gate the moment you add
the exclusion.

**And sweep the crate's *output*, not just its source.** An excluded
crate that emits code into the workspace — `enums/templates/rust.rs`
generates every `src/languages/language_*.rs` — is protected from the
workspace gate while its emitted code is not. Grep `enums/templates/`
and `generate-grammars/` for any pattern a cleanup pass removes.

The exclusion is intentional: the `enums/` codegen binary ships nowhere,
so running pedantic clippy and per-PR tests against it is waste. The
foot-gun is that *every* gate following the workspace boundary silently
skips it — `cargo check/clippy/test --workspace`, the per-PR `lint` and
`test` CI jobs, `make pre-commit`'s cargo trio, and the pre-commit
clippy/test hooks.

**`unused_imports` in `enums/src/lib.rs` sat for the entire fork** (#162,
fix `157d20f`). `pub use crate::macros::*;` could not re-export
`macro_rules!` definitions — macros use a separate namespace and none
carried `#[macro_export]` — so rustc warned on every build of the codegen
binary, invisible to every gate. Only a manual one-shot check found it.
The fix (#164, `d6c96e5`) added an `enums-check` target running
`RUSTFLAGS="-D warnings" cargo check --manifest-path enums/Cargo.toml
--all-targets --locked`, wired into `make pre-commit` / `make ci` / `make
lint`, the CI `lint` job twice, and the pre-commit hook set. The CI job
also injects a known warning, asserts the gate exits non-zero, and
restores the file — pinning the gate's *effectiveness*, not just its
existence.

**A cleanup buys exactly one regeneration cycle if it skips the
template** (#158 batch 1, `a59a0e9`; formerly lesson 17). Rewriting ~254
`#[inline(always)]` attributes to `#[inline]` across the language modules
touched three attribute strings that live in `enums/templates/rust.rs`
(`impl From<u16>`, `impl PartialEq<u16>`). Left alone, the next
`recreate-grammars.sh` run would have re-emitted them and silently
undone the cleanup. The template owns the long-term posture; the
generated artifact is downstream of it.

Lesson 51 covers the complementary gap: an excluded crate also needs a
per-crate **test** recipe, because `--workspace` never exercises its
runtime dispatch either.

---

## 16. shellcheck's default severity is `style`, not `warning`

**Lesson:** Re-run `shellcheck` against the actual file set *before*
trusting an issue body's category list — the body may have been authored
at a non-default severity or simply missed style-tier findings. As an
issue author, paste the raw output rather than a hand-curated list. As a
fix agent, run the tool at default severity on each target file and
reconcile; divergent extras are in scope and belong in the same commit,
or `make shellcheck` will not exit clean afterwards.

`shellcheck` defaults to `--severity style`, looser than its formal
`[warning]` tier, so `make shellcheck` fails on `SC2006` (legacy
backticks) and similar style-only findings — not just the SC2086 /
SC2164 family people associate with shell lint.

**Issue #165 enumerated SC2164 / SC1083 / SC2086 only**, but the actual
failures included SC2006 backticks in `generate-grammars/generate-mozcpp.sh`
and `generate-mozjs.sh`. The fix landed all four categories in one commit
(`532a6d0`); the conversions were mechanical, but a fix agent treating
the issue body as authoritative would have missed them.

---

## 17. *Merged into lesson 15*

The workspace-excluded `enums/` crate emits code into the workspace via
`enums/templates/rust.rs`, so a lint cleanup that rewrites the emitted
output without the template is undone by the next regeneration. Kept as
a sub-example of **[lesson 15](#15-workspace-excluded-crates-drift-outside-every-workspace-scoped-gate)**;
this number is retained because existing citations reference it.

---

## 18. `cargo clippy --fix` is one lint at a time

**Lesson:** After every `cargo clippy --fix` pass — especially with
`-W <single lint>` scoping the rewrite — re-run the full project gate in
**both** `--all-features` and default-features flavours before
committing. `cargo clippy --workspace --all-targets -- -D warnings` is
the load-bearing verification. Treat `--fix` as a proposal generator,
not a verification.

`--fix` runs the borrow checker once, applies the suggestion for the
warned lint, and exits. It never re-runs the full default lint set
against the rewritten code, so an auto-applied fix can satisfy its
target while introducing a different lint the `-D warnings` gate cares
about.

**#158 batch 1 (`a59a0e9`)** ran `--fix -W clippy::implicit_clone` over
`path_min.drain(..).map(|p| p.to_path_buf()).collect()` in `guess_file`.
The rewrite to `.clone()` satisfied `implicit_clone` and made
`clippy::map_clone` fire on the same line, because `.map(|p| p.clone())`
is redundant with `.cloned()`. The `--all-features` gate — the only one
re-run — missed it, since `map_clone` was default-feature scoped on that
clippy version. The next default-features CI tick would have failed the
build.

---

## 19. Metric dispatch enumerates kinds — missing arms score valid constructs as zero

**Lesson:** Treat a metric impl's arm list as a coverage claim, not a
complete spec. After touching or auditing one, grep
`src/languages/language_<lang>.rs` for every kind whose name suggests the
construct (`rg 'For[A-Z]'` for loops, `rg 'Conditional|Ternary'` for
`?:`) and confirm each is matched or excluded with a comment. **When
fixing one language's omission, build the audit table for the other
~20** — a survey table in the fix issue catches sibling bugs in the same
pass. Anchor each known-wrong-but-unfixed case in a regression test with
an inline `FIXME(#NNN)`, so the bug stays visible in CI and the eventual
fix flips a literal value rather than re-deriving the right one.

Related to lesson 1 (a whole-metric no-op returns zero) and lesson 2
(aliased kind_ids within one rule). The distinct failure here: an
*already-implemented* metric has a populated dispatch table that simply
does not enumerate every kind the grammar emits for the construct.

**C/C++ ternary `?:` uncounted for cognitive** (#172, `b2ae93f`). The
nesting arm listed `ForStatement | WhileStatement | DoStatement |
SwitchStatement | CatchClause` and omitted `ConditionalExpression`,
while every JS-family impl already had `TernaryExpression`. Every C/C++
file in the corpus scored 0 for ternaries; the fix moved 363 DeepSpeech
snapshots.

**C++ range-based `for` uncounted** (#173, `7eef01a`). The same arm
matched only `ForStatement`, missing `ForRangeLoop`. Classic loops
scored `+1 (+nesting)`; range-based scored 0. 99 snapshots moved.

**Java enhanced-for uncounted** (#178, `96b73d6`). Discovered via the
cross-language audit table built off #173 — without that sweep it would
have stayed invisible. The same audit confirmed JS `for...of` was fine
(the grammar folds `for...in` and `for...of` into one kind) and locked
four regression tests so a future grammar split fails loudly.

**The audit table paid off across eight more languages within a week**
(#212 `d8ed3b5`; #224 `baf98d8`; #225 `ea75e35`; #226 `7fce6f7`).
Python `match`/`case` contributed 0 to both cyclomatic and cognitive —
the dispatch predated PEP 634 and was never updated. Cognitive ternary
was missing from Java, C#, and PHP; cyclomatic `??` from all four
JS-family languages; cognitive labeled `break`/`continue` from Java, and
every form of `goto` from C#.

**The FIXME anchor made three bugs visible in CI before their fixes were
scheduled** (#167, `4b41187`; links added in `e8b9a4e`). `c_ternary`,
`c_range_based_for`, and `c_recursion` deliberately asserted the
current-wrong values with inline FIXMEs later retargeted at `FIXME(#172)`
/ `FIXME(#173)`. Each failed loudly the moment its dispatch arm changed.

---

## 20. `PathBuf::join(absolute)` silently replaces the base

**Lesson:** When normalizing a path for "place this under a base",
iterate `Path::components()` and discriminate by the `Component` enum
(`Prefix`, `RootDir`, `CurDir`, `ParentDir`, `Normal`) rather than
stripping prefix bytes. `Component` is cross-platform — it surfaces the
Windows `Prefix` variant explicitly, so one code path handles
`/tmp/a/b`, `./a/b`, and `D:\a\b`. Whenever a normalized path is about
to reach `PathBuf::join`, assert (or design so) it cannot be absolute on
**any** platform.

`PathBuf::from("/tmp").join("/etc/passwd")` returns `/etc/passwd`. The
behaviour is documented and easy to miss: a normalizer stripping Unix
`/` or `./` prefixes leaves the Windows `Prefix` component (`D:\`)
intact, after which `join` treats the path as absolute and drops the
base. Invisible on Unix.

**`bca metrics -o tmpdir` wrote files to the drive root on Windows**
(`4113bc6`). `handle_path` stripped Unix-style prefixes before
`output_path.join(cleaned)`. Input `D:\a\src\foo.rs` left `cleaned`
starting with `D:\`, so output landed under `D:\a\src\…` instead of the
requested base. Three Windows smoke tests caught it; Unix CI was clean.
Windows-only coverage is load-bearing here — a fix verified only on Unix
can ship a regression that wipes out user output.

---

## 21. Hidden-rule alias nodes extend their byte range to the shared delimiter

**Lesson:** Never assume a node's source text matches its visible token
name. Before pinning Halstead operand counts — or any text-keyed metric
— on an interpolation-bearing snippet, dump the AST with byte ranges and
confirm what each visible node actually spans. `node.utf8_text(src)` is
the source of truth; kind names like `identifier` describe the *rule*,
not the bytes. When a count comes out one higher than expected, check
whether the AST is splitting an identifier the way you assumed before
touching production code.

A visible node's `kind()` names the rule it came from; its
`start_byte()` / `end_byte()` describe which bytes the rule consumed.
When a grammar uses a hidden rule to consume a sigil together with an
identifier (`seq('$', $._foo)`, `seq('#{', $._expr, '}')`, alias
inlining), the visible node can span the delimiter. `utf8_text` then
returns `"$name"` for a node of kind `identifier`, making `$name` and
`name` distinct entries in any text-keyed store.

**Kotlin short-form string templates** (#191, `7a8ccac`). The production
`is_child(Interpolation)` guard was correct; the *test's* expected counts
assumed `name` inside `$name` shared an operand bucket with the
parameter `name` outside the string. tree-sitter-kotlin-ng emits a
visible `identifier` whose byte range starts at the `$`, so
`u_operands` is 4, not 3. The same hazard applies to
`template_substitution` wrappers, heredoc body splices, and Perl sigil
variables.

---

## 22. Text-keyed semantic markers force trait signatures to carry source bytes

**Lesson:** When implementing a metric for a new language, ask first:
does this language encode any branch/visibility/attribute semantic in
**bare identifier text** rather than a distinct token kind? If yes, the
trait needs `&[u8]` at `compute` — plan the widening as part of the
impl, not a follow-up refactor. Standardise on
`<'a>(node: &Node<'a>, code: &'a [u8], stats: &mut Stats)` for any new
metric trait; impls that do not need the bytes discard them with `_`.
The marginal cost is zero — the slice is already on hand at the call
site — and it avoids widening retroactively across every existing impl
plus the macro-generated defaults.

No `kind_id`-based dispatch can classify semantics carried in identifier
text. The addition propagates: the supertrait, every per-language impl
(explicit and macro-generated), the call site in `spaces.rs`, and any
downstream signature checks.

**`Cyclomatic::compute` widened for Elixir keyword `Call`s** (#179).
Elixir's `if` / `unless` / `for` / `case` / `cond` surface as `Call`
nodes with untyped targets — there is no `IfStatement` kind — so
distinguishing branch-contributing calls from ordinary invocations
required reading the target's text. `Exit::compute` was already that
shape.

**`Npa::compute` and `Npm::compute` widened for Ruby visibility
markers** (#190, `c42edf2`). Ruby's `private` / `public` / `protected`
parse as bare `Identifier`s sharing a kind with every other identifier.
Every per-language impl and both call sites in `spaces.rs` were updated
in one commit. The `Checker` supertrait is `pub(crate)`, so this is
invisible downstream — but the convergence is now load-bearing for any
future metric needing source bytes.

---

## 23. Compensation constants in parity tests blind the test to its own purpose

**Lesson:** When a parity test catches a real bug you cannot fix in the
same change, choose visibility over passability: `#[ignore]` with the
issue number, or FIXME-lock the wrong literal per lesson 19. Both
preserve the test's ability to detect *future* drift on the same input.
A per-target offset constant looks defensive but neutralises the test —
any future regression shifting the same metric by `±OFFSET` becomes
invisible, and an explanatory comment is no substitute for a failing
test, because reviewers skim comments and CI cannot. The rule
generalises: anywhere a calibration constant compensates for a known
asymmetry, that test cannot catch bugs in the asymmetric path.

**`PYTHON_ELSE_BUG_OFFSET` hid the Python over-count from the parity
test designed to catch it** (#229, `a239cf6`). `if_else_if_else_chain_parity`
detected that Python over-counted plain `if/else` by 1. Root cause:
`Node::has_ancestors(typ, typs)` returned `true` whenever the immediate
parent matched the *second* predicate, regardless of the first, so
Python's `Else` cyclomatic arm fired for every `else_clause` rather than
only loop-`else`. Instead of ignoring or FIXME-locking, the author added
`const PYTHON_ELSE_BUG_OFFSET: f64 = 1.0` with an 8-line explanatory
comment. That made the test pass for **every** Python case, including
any future regression shifting the count in a different direction. #229
fixed `has_ancestors` (renamed `parent_grandparent_match`, strictly
checking both predicates), updated the sole call site, and removed the
offset in the same commit.

---

## 24. A cross-cutting traversal feature must reach finalize and span-derived metrics

**Lesson:** "Skip / gate / prune metric X" must reach **every** place X
is read, aggregated, or derived — not just the per-node `compute` call.
Audit each metric against the new feature by how it is *produced*:
node-accumulated metrics honour a walk-level `continue`, but span
subtractions and `finalize` roll-ups never see it, and anything derived
from a stale field inherits the error. Either route every metric through
the node-accumulated path, or give each non-accumulated metric an
explicit hook the feature calls.

Then test it. Audit the **default** of every `Stats` type first: any
non-zero default (Cyclomatic's `1.0` McCabe baseline is the canonical
one) propagates through finalize and looks indistinguishable from a real
computation. Write at least one test asserting `== 0.0` on an
*unselected* metric whose default is non-zero; a `> 0` anchor on the
selected metric is necessary but not sufficient.

**`MetricsOptions::with_only` gated compute but ran finalize for every
metric** (#257, `1169231`, `d758f89`, `d5f9ff2`). The first cut of
`with_only(&[Metric::Loc])` correctly gated each `T::X::compute` call
but left `compute_minmax` / `compute_sum` / `compute_averages` /
`compute_halstead_mi_and_wmc` unconditional, so `cyclomatic_sum`
reported the McCabe baseline (`1.0` × function count) with Cyclomatic
deselected. `loc_only_skips_other_metrics` caught it only because it
asserted strict `== 0.0`. The fix threaded `selected: MetricSet` through
every finalize helper. `mi_auto_pulls_dependencies` was strengthened for
the same reason — asserting only `mi.is_finite()` would have passed
against the `inputs_are_empty` short-circuit returning `0.0`.

**`exclude_tests` pruning left unit-level `loc.sloc` at the full-file
extent** (#722, `1f52b742`; formerly lesson 76). `Checker::should_skip_subtree`
— a `continue` in the walk loop — correctly dropped `ploc`/`cloc`/`lloc`
and the node-counted metrics for a pruned `#[cfg(test)]` subtree. But
`sloc` for the file space is a pure span subtraction over the root node,
so skipping children never moved it: a file reported `sloc 11757` beside
`ploc 2451`, `blank` inflated by the elided rows, and `mi.*`'s
`16.2·ln(SLOC)` term never benefited. The fix records each pruned
subtree's row span on its enclosing space and subtracts it in `sloc()` —
and had to move the `finalize` call *ahead* of the prune `continue`, so
`state_stack.last_mut()` is the pruned node's true enclosing space
rather than a sibling's still-open one.

---

## 25. Crate-root `pub use module::*` silently leaks every newly-`pub` sub-module item

**Lesson:** Replace crate-root globs with explicit
`pub use module::{X, Y, Z};` before stabilising anything that depends on
the API surface (a `prelude`, a `cargo-public-api` baseline, a
`STABILITY.md`). Internal callers reaching previously-leaked items have
to be re-routed via `pub(crate) use` or fully-qualified paths — surface
that drift in the same change, not as a chase later. Do not add a new
`pub use module::*` to `lib.rs` once it has been curated.

A glob makes every `pub` item in the submodule part of the published
API, whether intended or not. Reviewers cannot see what the line exports
without enumerating the file: internal CLI helpers, test-only types, and
trait methods bumped to `pub` for one call site all become
SemVer-relevant. The leak is invisible until someone removes the glob and
watches the curated list grow.

**Seventeen globs in `src/lib.rs` hid accidentally-public items** (#255,
`bab3da9`). Replacing them revealed several items the crate's own
internals reached at `crate::X` paths, working only because the glob made
them `pub` at the root. Tightening required adding `pub(crate) use`
lines for `metrics_inner`, `Search`, `check_func_space`, and the
per-metric type tags — all public-by-accident, with nothing but the glob
making it look deliberate.

---

## 26. Feature-gating a generic dispatcher forces the return type to widen to `Result`

**Lesson:** A generic dispatch signature returning an associated type
cannot be feature-gated without widening the return to a `Result` (or
another error-carrying shape). Plan the widening into the same change as
the feature flag — splitting into separate PRs costs an unbuildable
intermediate state. Do **not** discharge the widened `Result` at the
call site with an `expect`, however well the invariant is documented:
propagate it onto whatever error channel the caller already has.

When per-language features remove `LANG` variants from the build, the
dispatch macro must still match every variant of the always-defined
enum, so disabled variants need a `#[cfg(not(feature))]` arm returning
*something*. `fn action<T>(...) -> T::Res` is uninhabitable there: there
is no way to construct an arbitrary `T::Res` when the type defining it
is cfg'd out.

**Per-language features widened `action::<T>` and
`LANG::get_tree_sitter_language`** (#252, `b923919`). Keeping `LANG`
always-defined introduced paired `#[cfg]` arms across `mk_action!` and
`mk_lang!`; the `not` arms had no `T::Res` to return, so both signatures
widened to `Result<_, MetricsError>`. This rippled into the CLI and web
crates, where every call site became `.expect(FEATURES_PINNED)` because
both pin `features = ["all-languages"]` and the disabled arm is provably
unreachable. Recorded in `CHANGELOG.md` and `STABILITY.md`.

**The `expect` half of that was wrong, and #1152 removed all fifteen
call sites.** The invariant it named was real — the feature pin does
make `LanguageDisabled` unreachable — but it was the wrong invariant to
rest a panic on. `MetricsError` is `#[non_exhaustive]`, and its own
documentation reserves the right to add variants in a *minor* release,
so "provably unreachable" held only for the variants that existed on the
day it was written. A future variant would have turned a routine
dependency bump into a panic, in a library whose input is
attacker-controlled source. Naming an invariant once does not make it
load-bearing; what makes it safe is that violating it cannot panic.

Both crates already had an error channel to propagate onto, which is the
tell that the `expect` was never necessary: the CLI's dispatch helpers
return `std::io::Result<()>` and its runner prints a per-file line and
continues, and the web handlers return `Result<HttpResponse, Error>`
with an existing sanitized `500`. Neither needed a new failure mode —
only for the existing one to be used.

---

## 27. Share a private walker across deprecation shims to keep them thin

**Lesson:** Any deprecation cycle where the old and new APIs share most
of the work should land a **single private worker with the union shape**,
fronted by two thin public shims. Avoid "leave the old code alone, fork
a copy": it ships two implementations, doubles the surface where future
fixes must land, and lets the deprecated-on-paper path silently drift.
The same applies when adding a `from_X` constructor next to `new` —
extract the common construction body rather than copying it.

**`Source` and `analyze` kept the old walker thin via `metrics_inner`**
(#254, `41d5005`, `8b460fb`). Landing `Source<'a>` and
`analyze(source, options) -> Result<FuncSpace, MetricsError>` alongside
the deprecated `metrics` / `metrics_with_options` / `get_function_spaces*`
entry points used `pub(crate) fn metrics_inner(name: Option<String>, …)`
to carry the actual walk; the deprecated shims build
`name = Some(path.to_string_lossy().into_owned())` and call it, and
`analyze` destructures `Source` and calls the same. The follow-up
dropped a redundant `diagnostic_path` parameter once the path/name
relationship was consolidated — the diagnostic string now derives from
`name.as_deref().unwrap_or("<input>")`, eliminating one parameter and a
double allocation at every shim.

---

## 28. Hand-rolled `Serialize` with conditional fields must pre-count for CBOR

**Lesson:** A hand-rolled `Serialize` emitting a conditional field set
must compute its field count from the **same predicates** it uses in the
body; the two halves cannot drift. If the format mix includes CBOR /
MessagePack / any length-prefixed binary encoding, only those formats
catch a tally bug — never trust a JSON-only test pass. A local macro
pairing the predicate with the field name in one place is the cheapest
defence; the alternative is a comment asking future authors to keep the
tally in sync, which everyone ignores.

`serialize_struct(name, len)` writes `len` before the first field, and
CBOR / MessagePack reject the payload at `st.end()` if the emitted count
diverges. JSON writes no length prefix and tolerates the mismatch
silently.

**`CodeMetrics::serialize` tracks the count across 13 conditional arms**
(#257, `1169231`, simplified by `66a0d8c`):

```rust
let field_count = always_on.iter().filter(|m| sel.contains(**m)).count()
    + usize::from(emit_wmc)
    + usize::from(emit_npm)
    + usize::from(emit_npa);
```

The simplify pass collapsed the arms into a local `emit_if!` macro,
making the 1:1 correspondence visually obvious without changing the
invariant. The integration snapshot suites are JSON-based and would
**not** catch a tally bug; only an actual CBOR consumer does, and there
was no end-to-end CBOR test in the workspace at the time.

---

## 29. Compile-test API doc samples by linking against a scratch crate, not `mdbook test`

**Lesson:** Treat API doc samples as production code under `cargo
check`. Use a scratch crate (or a `tests/` integration file with
`#[allow(dead_code)]`) that depends on the library via `path = "../"` and
compiles every sample against the local checkout. Run it before
committing book chapters and whenever a diff touches
`big-code-analysis-book/src/`. The cost is one scratch file; the
avoidance is reader-facing API typos.

`mdbook test` runs each fenced ```rust block as a doctest, but only
against the book's own `Cargo.toml` dependency list — it does not
resolve `use crate_under_test::…` against the local checkout. Typos,
wrong argument counts after a signature change, and silently-renamed
re-exports all sail through `mdbook build` and `mdbook test` until a
reader copies the sample and reports back.

**The "Using as a Library" chapter caught `LANG::JavaScript` before
publish** (#259, `8ee83ea`). Eight new pages carried samples driving
`get_function_spaces` / `analyze`. Writing them against rustdoc surfaced
one real typo — the variant is `Javascript` — and one outdated method
name. The samples were compiled in a scratch crate depending on the
library by path, fixed, then copied back. `mdbook test` alone would have
shipped the typo.

---

## 30. User-facing comment markers should match the codebase's internal vocabulary

**Lesson:** When choosing a verb for a marker that lives inside source
comments across many languages, pick the verb **the codebase's own
internal vocabulary already uses**. That alignment removes the cognitive
bridge between the comment a user writes and the module a reviewer
reads. Industry precedent comes second; internal consistency comes first,
because it is what future contributors keep tripping on. Cross-check
against at least three suppression vocabularies outside the host
language (`#noqa`, `eslint-disable`, `@SuppressWarnings`,
`cppcheck-suppress`, `# type: ignore`) — if your verb is the outlier
across that set *and* against your internal model, redesign.

Suppression markers originally used `bca: allow`, mirroring Rust's
`#[allow]`. Hard-renamed to `bca: suppress` in #263 before they shipped
widely. `allow` reads correctly inside `#[allow]` only because it sits
in a closed four-level vocabulary (`allow`/`warn`/`deny`/`forbid`) inside
attribute syntax. Stripped of both — a free-text comment in a codebase
with no `warn`/`deny`/`forbid` siblings — it reads as "this code is
permitted to be complex" rather than "suppress the violation report".
Everything internal already said suppress: `src/suppression.rs`,
`SuppressionPolicy`, `FuncSpace::suppressed`, `--no-suppress`. And
`allow` as an embedded-comment verb is essentially unique to Rust
attributes; the rest of the ecosystem uses `disable`, `ignore`, or
`suppress`.

---

## 31. Shared structural fixes need a structural assertion in every per-metric test

**Lesson:** When one fix has both a **structural** arm (a predicate that
opens a FuncSpace) and per-metric **body-walker** arms (the counts
inside it), every per-metric test must assert *both* halves: that the
FuncSpace opens with the expected `SpaceKind` and name, and that the
metric sum matches. Use `check_func_space` (or
`test_support::assert_child_space_kind`) at the top of each test,
followed by the `check_metrics` value assertion. Coverage that *looks*
complete because three metrics each have a regression test can be split
three ways and guard nothing about the structural change.

A body-walker assertion alone is not enough: counts can fire from a
fallback scope — a synthetic Unit wrapper, a `SpaceKind::Class` default,
a zero — and pass vacuously even after the structural arm is reverted.

**Java/Groovy annotation-type recognition** (#280, #307, `ba2a8e3`,
`d637a98`). The #280 fix wired `AnnotationTypeDeclaration` into
`is_func_space` for both languages so `Npa`, `Npm`, and `Wmc` would walk
annotation-type bodies. Three per-metric regression tests were added. An
`audit-tests` pass later found only the Wmc test caught a revert — and
only because it asserted `interface_wmc_sum() == 0`, vacuously true when
no Interface FuncSpace opens. Npa and Npm both passed with
`AnnotationTypeDeclaration` removed, because their counts came from the
file-level Unit scope. #307 tightened both with `check_func_space`
assertions and factored the six structural assertion sites into a shared
helper.

**Plain `interface I {…}` shares the bug** (#311). Tests in `npm.rs` and
`npa.rs` assert non-zero `interface_*_sum` with no structural check, so
reverting the `InterfaceDeclaration` arm would pass them vacuously too.

---

## 32. Source-grep regression tests are theater

**Lesson:** Never string-match the codebase's own source in a test. The
rule and its three ranked alternatives are in
[`testing.md`](../../.claude/rules/testing.md) — construct a parseable
input and assert the parse-tree consequence; failing that, document the
contract at every production call site; failing that, design a
compile-time check (an exhaustive `match`, a `const`-evaluated
assertion).

The grep is brittle to cosmetic edits — comment wording, rustfmt reflow,
an `impl` header rename — and satisfied vacuously by adding the
identifier in an unrelated comment. If the contract is "predicate X
names variant Y", the production `matches!()` pattern already *is* the
contract; the grep asserts the same thing a reader can see, less
reliably.

**The `FunctionDefinition4` source-grep test** (#285, #302, `fe5bf6a`).
Because the pinned tree-sitter-mozcpp emits no input producing that
kind_id, a parse-and-assert test was impossible, so the fix read
`src/checker.rs` and `src/getter.rs` from disk and counted occurrences
inside each `impl` block. It passed code review and shipped. #302 showed
it was both fragile — a rustfmt pass joining two `impl` lines breaks the
block extraction — and vacuous: adding `// FunctionDefinition4` to one
block satisfies the count without wiring the variant. The remediation
deleted the test and added contract comments at the four production
sites citing #285 and listing the sister sites. The only such test ever
written here was identified as vacuous and removed within months.

---

## 33. Test-via-revert proves coverage one slot at a time

**Lesson:** When proving a parity or coverage test catches drift, visit
**every delta slot** the refactor introduced, not just one. For macros
with bracketed extras lists (`op_extras: [...]`, `operand_extras: [...]`,
`[$($variant),+]`), each list is its own slot and each list's *contents*
must be revert-proved. For multi-variant languages in one macro
invocation, each variant is its own slot. Route assertions through a
helper taking the language path and variant name as strings (via
`stringify!`) so a failure identifies *which* of the N×slots is broken,
and so future work that adds a slot fails loudly if the helper is not
extended.

A single test-via-revert proof — "I dropped one thing from one site and
the test failed" — protects that one thing at that one site. The
remaining N×slots stay ungated, and the test reads as a four-way parity
guard while being a one-way regression guard for a single token.

**JS-family `get_op_type` parity only revert-proved the operator token**
(#299, `45d907f`, `06f6a68`). The test asserted four-way parity on
`function f(a){return a?.b?.c;}` and its comment claimed "dropping a
common variant from any one language's macro invocation must fail this
test." An `audit-tests` pass perturbed every slot and found that true
only for the operator-token slot. Dropping any entry from the
per-language `operand_extras` lists — `Identifier2`, `String2`,
`NestedIdentifier`, `MemberExpression4`, TS's `PredefinedType` — left the
test passing, because the `a?.b?.c` fixture never produces those kinds.
One of the gaps it could have caught (TS classifying `String2`
differently in `is_string` versus `get_op_type`) was surfaced
independently and filed as #313.

**`impl_simple_is_string!` only revert-proved one variant per language**
(#301, `7192d56`, `5829560`). The initial test exercised one canonical
string literal per language. Test-via-revert with `Rust::StringLiteral`
proved the macro arm was reached, but Csharp (4 variants), Php (7), Ruby
(11), Perl (7), Bash (5), and Groovy (3) were each defended by a single
literal — dropping `Csharp::VerbatimStringLiteral`, `Ruby::Subshell`, or
`Php::Heredoc` left the test passing. The hardened form asserts one
variant per language via an `assert_variant_is_string` helper with
`stringify!`-derived labels, so failures name both language and variant.

---

## 34. Tree-sitter hidden-rule variants exist in the enum but never surface

**Lesson:** Before listing a "looks like an alias" variant in a
classification predicate, check the grammar's `kind_for_id` mapping (or
grep the `Lang::Variant => "name"` arm in
`src/languages/language_<lang>.rs`). If the name starts with `_` the rule
is hidden: the variant exists in the enum and never appears in a real
AST. Either omit the defensive arm, or — preferred — keep it **and** add
a drift-marker assertion
(`!ast_has_kind_id(&parser, Lang::HiddenVariant as u16)`) whose message
names the hidden rule and demands replacement on drift. A hidden-rule
variant without a drift marker is an invisible promise: it looks like
coverage and protects nothing observable today. The underscore
identifies a hidden rule; it does not establish that a non-underscore
alias is *reachable*. When the name is ordinary, probe every context the
construct appears in before concluding the variant is live — the remedy
is the same drift marker either way.

The parser flattens hidden rules away, so a defensive arm listing one is
dead code today and a correctness promise *if* a future grammar revision
promotes the rule. Without the asserted-absent test, that promotion goes
undetected: the parser starts emitting the variant and the predicate
either silently misses it or silently catches it — either way the
codebase loses visibility into what changed.

**Java/Groovy/Php `is_string` consolidation made the heuristic
explicit** (#301, `7192d56`, `5829560`). Per-variant coverage for the new
`impl_simple_is_string!` macro required exercising every variant in
every invocation, and three would not appear in any constructible
source: `Java::MultilineStringLiteral` (text blocks parse as regular
`StringLiteral`), `Groovy::StringLiteral2` (triple-quoted strings do
the same), and `Php::String3` (the `_string` hidden supertype). Each
maps to a name beginning with `_`, confirming the heuristic. The
remediation keeps the arm and pins the absence.

**An alias with an ordinary name that the grammar never emits** (#1268).
`Bash::TernaryExpression2` maps to `"ternary_expression"` with no leading
underscore, so the heuristic above answers "not hidden". Across all eight
arithmetic contexts the grammar admits — `$(( … ))`, a bare `(( … ))`
statement, a c-style `for ((…))` header, `let`, an array subscript, an
`if (( … ))` condition, a `declare -i` initializer, and a plain
assignment — the parser emits only the unsuffixed kind. The defensive arm
stays and carries the same `ast_has_kind_id` marker; what changed is that
identifying the case took a probe rather than a grep.

---

## 35. Two predicates classifying the same node must agree

**Lesson:** When refactoring or extending a per-language classification
predicate, walk the *other* predicates that classify the same nodes. The
minimum cross-walk is in
[`grammar-dispatch.md` §7](../../.claude/rules/grammar-dispatch.md). The
reusable diagnostic is a parsed fixture asserting **both** predicates
agree: parse a source containing the node, locate every occurrence by
kind_id, and assert each predicate's verdict per occurrence.

`Checker::is_string`, `Getter::get_op_type`, `Checker::is_call`,
`Checker::is_func_space`, and the per-metric body walkers all classify
the same nodes through parallel `matches!()`. When two that should agree
disagree, output drifts silently — `find string` reports a node Halstead
calls `Unknown`, or the reverse. Neither predicate looks wrong read in
isolation; the disagreement surfaces only by walking the cross-product
of (node kind, predicates that classify it).

**TypeScript `String2` agrees with `is_string`, disagrees with
`get_op_type`** (#313, surfaced during #299 review).
`impl_js_family_is_string!(Typescript)` matches `String`, `String2`, and
`TemplateString`, so a `String2` node — the `string` type-keyword alias
— is counted by `find string` and contributes to Halstead string-operand
totals. But the TS `impl_js_family_get_op_type!` invocation's
`operand_extras` omits `String2`, so the same node is `HalsteadType::
Unknown` to the Halstead walker. JS, MozJS, and TSX all include it; only
TS does not. The drift predates #299 — the four pre-refactor impls had
the same asymmetry — but the macro consolidation made the parity table
legible enough for a reviewer to spot it.

---

## 36. `serde_json::to_value` re-sorts JSON object keys via `BTreeMap`

**Lesson:** When crossing a serde → insertion-ordered-runtime boundary,
route through `serde_json::to_string` and re-parse on the other side,
not through `to_value`. The `preserve_order` feature is an alternative
but applies workspace-wide and may interact with downstream crates
expecting the default sort. The diagnostic test **cannot** compare
structurally-equivalent containers: it must compare the emitted key
order against a hand-pinned sequence whose source order is deliberately
non-alphabetical, or compare raw JSON bytes positionally.

`Value::Object` is backed by `BTreeMap` unless `preserve_order` is
enabled, which this workspace does not. Any `Serialize → to_value →
re-emit` round trip alphabetises the keys regardless of the `Serialize`
impl's declaration order.

**`bca.analyze()` field order silently re-sorted** (#265 batch,
`6574aff`). The first PyO3 cut serialised `FuncSpace` via `to_value` and
walked the tree to build a Python `dict`, producing
`{"end_line", "kind", "metrics", "name", "spaces", "start_line"}` instead
of the declaration order the CLI emits. Byte-for-byte parity with the
CLI was the bindings' stated contract, and the trap was invisible
because `dict ==` is order-insensitive in Python, so every test
comparing dicts passed. The fix routes through
`serde_json::to_string(&space)` plus CPython's `json.loads`, which builds
the dict in input order.

---

## 37. CPython `OSError(errno, msg, filename)` dispatches to the right subclass

**Lesson:** Every PyO3 binding surfacing a Rust `std::io::Error` must
build the Python exception with the 3-tuple `(errno, msg, filename)`
form — `errno` from `io::Error::raw_os_error()`, `filename` from the
path that triggered the failure. This requires capturing the `Path` at
the failure site, not just the `io::Error`; a blanket `From<io::Error>`
impl drops it. Verify with a round-trip test inspecting `err.errno` and
`err.filename`, not just the exception class — a test catching `OSError`
passes against the buggy code.

The constructor is overloaded by arity. `OSError(errno, message,
filename)` dispatches to the matching subclass (`FileNotFoundError` for
`ENOENT`, `PermissionError` for `EACCES`) and populates `err.errno` /
`err.filename`. `OSError(message)` loses the dispatch entirely: every
I/O failure surfaces as bare `OSError` with `errno is None`. The 1-arg
form is the natural shape when bridging `io::Error::to_string()` and the
default everyone reaches for first.

**`bca.analyze(missing_path)` raised bare `OSError`** (#265 batch,
`f91fac0`). Python callers writing `except FileNotFoundError as e:
e.filename` never matched the subclass and never saw the path. The fix
carries the originating `PathBuf` through `AnalysisError::Io { source,
path }` and constructs `PyOSError::new_err((source.raw_os_error(),
source.to_string(), path.display().to_string()))`.

---

## 38. Co-pinned runtime + build-time companion crates must share an exact patch

**Lesson:** Identify co-pinned crate pairs spanning the runtime /
build-time FFI boundary in any dependency family you adopt (pyo3 /
pyo3-build-config, sqlx / sqlx-macros, opentelemetry /
opentelemetry-otlp). Use exact pins (`= "X.Y.Z"`) on **every** crate in
the pair, not the caret default, and put a one-line comment at each pin
naming the partner and the contract they share. The diagnostic for "did
this happen?" is `cargo tree -d`. A `cargo update` PR bumping one
without the other should fail review immediately.

Cargo's default caret can resolve two crates implementing halves of one
contract to different patches. A build-time symbol or link flag emitted
under the older patch can then disagree with what the runtime crate of
the newer patch expects, and the symptom is a mysterious link error at
test time, not a compile error in either crate.

**`pyo3` / `pyo3-build-config` were pinned preventatively, before the
drift could surface** (#265 batch, `50c7fca`). The build script calls
`pyo3_build_config::add_libpython_rpath_link_args()`; whether it emits
`-Wl,-rpath` or `-Wl,-rpath-link`, and where the path comes from, depend
on the build-config patch. Both were spelled `"0.28"`, so cargo was free
to resolve them independently. At fix time both happened to resolve to
`0.28.3` and `cargo tree -d` was clean — but the next `cargo update`
could have moved one. Pinning to `= "0.28.3"` forecloses it: a future
bump now requires a deliberate paired edit. Catching the drift at the
pin is far cheaper than bisecting two interlocked patches after a link
failure.

---

## 39. `#[non_exhaustive]` enum wildcards are required, not tripwires

**Lesson:** A "tripwire" is an **exhaustive** match on a closed enum,
where adding a variant produces a compile error at the match site.
`#[non_exhaustive]` forecloses that mechanism by definition. For a real
audit signal when an upstream variant is added, the options are: opt
into `cargo deny` / `cargo semver-checks` rules that flag it; add
explicit named arms for every variant you have audited and document the
unmapped default *honestly*; or generate the match from the upstream
enum via a build script that fails when the set changes. Do **not**
describe a wildcard arm as a tripwire.

Downstream matches on a `#[non_exhaustive]` enum must include a wildcard
— the compiler refuses otherwise. It is a legal requirement, not a hook
for future audits: new variants compile silently into the wildcard, and
their downstream classification defaults to whatever it maps to, usually
the most generic bucket. A reviewer relying on the tripwire framing will
not audit the match on `cargo update`.

**`From<MetricsError> for AnalysisError` claimed its wildcard was a
tripwire** (#265 batch, `e8ec96b`, corrected in `8d7ef17`). The comment
claimed a `cargo update` introducing a new variant "should be paired
with an explicit arm above." The framing was load-bearing — it implied a
reviewer would notice — but `MetricsError` is `#[non_exhaustive]`, so a
new variant lands in `Self::Parse` and the Python exception class
silently becomes `ParseError` until someone manually audits the impl.
The fix corrected the comment and noted the only real tripwire would be
removing the wildcard, which does not compile.

---

## 40. `#[cfg(unix)] { … }` inside a test body silently passes on other targets

**Lesson:** Gate the entire `fn`, never an inner block — the rule and its
inverse trap are in
[`testing.md`](../../.claude/rules/testing.md). An inner-block `#[cfg]`
compiles to an empty body off-target, and an empty `#[test]` is a
*passing* test: the harness reports green with zero assertions run. The
function-level form hides the test instead.

**`from_internal_preserves_byte_uniqueness_for_distinct_non_utf8_paths`**
(`big-code-analysis-py/src/batch.rs`; caught in a draft prepared for
`515e840`, never reached `main`). The audit-tests pass on the
`analyze_batch` work caught the draft wrapping its entire body in
`#[cfg(unix)] { … }`. On Linux it exercised the byte-uniqueness contract
for non-UTF-8 paths; on Windows it would have compiled to an empty
function reported as passing, with zero coverage of the dedup invariant.

**The dual failure mode: a fixture baking in one platform's spelling
fails *spuriously* off-target** (#920 follow-up).
`test_conftest_helpers.py` fabricated `debug/bca` and `release/bca` to
exercise the CLI-locator helper, but `_locate_workspace_binary` appends
the platform executable suffix. On `windows-latest` the locator looked
for `bca.exe`, matched nothing, and two assertions failed — while Linux
and macOS were green because the suffix is empty there. Invisible to
every non-Windows runner and to local `make pre-commit`.

---

## 41. Clone-based hash/eq tests don't pin the dedup contract

**Lesson:** Any test pinning a hash/equality contract for a value type —
especially one used as a set, dict, or dedup key — must construct both
compared instances through the **production constructor twice**, not
once-plus-clone. The clone path tests only the derive; the two-call path
tests the constructor's determinism. Apply this anywhere
`#[derive(Hash, PartialEq, Eq)]` or PyO3's `#[pyclass(eq, hash)]` reaches
a consumer calling `.contains()` or collecting into a `HashSet`.

`Clone` produces a byte-identical struct by definition, so a clone-based
test holds regardless of what the constructor does — including under
regressions that mix per-call state (a static counter, a UUID, a
timestamp) into a field.

**`equal_errors_hash_equal` on `PyAnalysisError`**
(`big-code-analysis-py/src/batch.rs`; introduced in `96fe3ab`, corrected
in `515e840`). The audit-tests pass found `let a = …; let b = a.clone();`
pinning the `Hash` / `Eq` contract that `set(results)` dedup promises in
Python. Verified by revert: perturbing `new_internal` to interleave a
`static AtomicU64` into the `error` field left the clone-based test
passing — the counter never advances, because the clone bypasses the
constructor — while the two-call form correctly failed.

---

## 42. `unreachable!()` at a PyO3 FFI boundary surfaces as `PanicException`

**Lesson:** Refines lesson 5 with a PyO3 corollary: at any FFI boundary
documenting a never-raise contract, even an *unreachable-today* panic
violates it, because PyO3 re-raises it as `pyo3.PanicException` —
extending `BaseException` directly, outside the `Exception` hierarchy
Python callers' handlers cover. Replace `unreachable!()` / `panic!()` /
`assert!()` on those boundaries with a defensive structured-error
fallback: a synthetic error in the result slot, or an explicit `Err`
branch. The fallback should name the broken invariant in its message so
telemetry surfaces it, but it must not abort the call. Apply to every
`#[pyfunction]` / `#[pymethods]` documenting partial-success semantics.

`except Exception:` — and every narrower form — does not catch it. The
panic aborts the call, every accumulated result is discarded, and the
caller sees an uncatchable exception. The Rust idiom that reads as
"defensive" is, at the FFI boundary, the inverse.

**`analyze_batch`'s `Ok(None)` arm** (`big-code-analysis-py/src/batch.rs`).
The original `96fe3ab` shipped a defensive `PyAnalysisError` fallback; a
review-remediation pass in `e670f8b` regressed it to `unreachable!()`
with a comment claiming it would "fail loudly in development" — exactly
the failure mode this lesson warns against — and `515e840` restored the
fallback. At the time, the single-file bridge returned `Ok(None)` only
when `skip_generated=true` and `analyze_batch` hard-coded `false`, so
the arm was unreachable. But the documented contract is "never raises on
per-file errors", which demands a structured `AnalysisError` in the
result slot. The restored fallback names the invariant break and tells
the operator to audit `analyze_path` for new skip surfaces, so the
contract survives any future refactor adding a second one (a gitignore
filter, a size cap).

**Both premises have since fallen, and the fallback did not survive to
catch it.** #706 routed the bindings through the walker's
`read_file_with_eol` gate — a second and *unconditional* `Ok(None)`
source (three bytes or fewer, a UTF-16 BOM, a non-UTF-8 leading
window) — and the #542 commit (`3220e2a0`), which made the arm
legitimately reachable by flipping `skip_generated` to default `true`,
**deleted the synthetic fallback** and replaced it with a bare
`Ok(None) => {}`. That deletion is precisely why #1238 was silent: the
fallback's message — "audit `analyze_path()` for new skip surfaces" —
was written for this exact event, and the event arrived with the guard
already gone. The batch docstrings' "`skip_generated=False` guarantees
one element per input" was unsatisfiable for two releases, and the
endorsed `zip(inputs, results)` attributed metrics to the wrong paths
with nothing to observe. The corollary this lesson missed the first
time: a defensive fallback for an unreachable arm is at maximum risk at
the moment the arm becomes *partially* reachable, because the commit
that legitimises one source reads the whole fallback as obsolete and
removes it — taking the guard against every *other* source with it.
Keep the fallback scoped to the still-invalid residue, or replace it
with something a refactor cannot silently drop (an exhaustive reason
enum from the callee).

---

## 43. `to_string_lossy()` on a path field promoted into `Hash` / `PartialEq` keys silently collapses dedup

**Lesson:** Audit every struct field participating in
`#[derive(Hash, PartialEq, Eq)]` or `#[pyclass(eq, hash)]` for lossy
rendering. If a string field can be built from non-UTF-8 bytes via
`to_string_lossy()`, `from_utf8_lossy()`, or any other lossy projection,
distinct inputs collapse to equal hashes — even when the field is
documented "for display only". Three fixes, in preference order: render
via a byte-preserving projection (`format!("{:?}", path)`, whose `OsStr`
Debug uses `\xNN` escapes); exclude the lossy field from the derive; or
carry the raw bytes in a separate field that participates in the hash.
Default to the first — it preserves the visual cue and is the smallest
change.

`AGENTS.md` already forbids `to_string_lossy()` on identifier paths. The
non-obvious second-order hazard: a field participating in a derived
`Hash` **is** an identifier the moment a consumer puts the struct in a
`HashSet` or dict key, whatever its docstring says.

**`PyAnalysisError.path` collapsed distinct non-UTF-8 paths under `set`
dedup** (`big-code-analysis-py/src/batch.rs`; `96fe3ab`, corrected in
`515e840`). The docstring described the lossy fallback as "diagnostic
only", but `#[pyclass(eq, hash)]` promoted `path` into the key and the
documented `set(results)` pattern dedups on `(path, error, error_kind)`.
Two distinct paths (`b"/a\xff"` and `b"/a\xfe"`) both rendered to `/a` +
U+FFFD, compared equal, hashed identically, and silently merged —
exactly the contract `__hash__` was advertised to serve.

---

## 44. Rust's `{:?}` Debug format escapes non-printables as `\u{N}`, which Python's parser rejects

**Lesson:** Any hand-written `__repr__` / `__str__` on a `#[pyclass]`
handling string fields must delegate the per-field escape to Python's
own `repr()`, not Rust's `{:?}`. Test the round-trip explicitly with
non-printable, non-ASCII, and non-BMP inputs — a single control
character is enough to expose the failure. The cost is one
`py.import("builtins")` and a `repr_fn.call1((&field,))?.extract()?` per
field; the gain is the `eval(repr(x))` contract the docstring almost
always promises.

Rust's `Debug` for `str` escapes outside printable ASCII as `\u{N}` —
curly braces, variable width. Python's source parser accepts `\xNN`,
`\uNNNN`, and `\UNNNNNNNN`, none with braces. So a repr looks correct,
passes every ASCII-fixture test, and breaks `eval(repr(x))` on exactly
the weird data a debugger is most useful for.

**`PyAnalysisError.__repr__` broke on control-char paths**
(`big-code-analysis-py/src/batch.rs`; `96fe3ab`, corrected in
`515e840`). The docstring promised `eval(repr(x))` would reconstruct an
equivalent object. A follow-up Python test caught it:
`bca.AnalysisError("/tmp/\x01中.py", "boom ሴ", "IoError")` produced
`path="/tmp/\u{1}中.py"` under `{:?}`, and `eval` raised `SyntaxError` on
the `\u{1}` token.

---

## 45. XML attribute-value normalization collapses raw TAB / LF / CR

**Lesson:** Any XML writer using attribute values for data — paths,
messages, identifiers carrying user text — must escape TAB / LF / CR as
numeric character references (`&#x9;` / `&#xA;` / `&#xD;`), not literal
bytes. The conforming-parser behaviour is silent — no error, no warning,
just normalization on read — so the only way to validate is to re-parse
with a real parser and compare scalar-for-scalar. Cite §3.3.3 in the
escape function's comment so the next contributor does not revert it on
aesthetic grounds.

XML 1.0 §3.3.3 mandates that any whitespace inside an attribute value
other than the result of a character reference is normalized to a single
space on read. The bytes survive on disk, but every conforming consumer
— Jenkins, SonarQube, GitLab CI, libxml2-based tooling — sees them
collapsed. Numeric character references survive because the value the
parser publishes is the post-replacement scalar.

**`XmlAttr::fmt` emitted literal TAB / LF / CR inside Checkstyle
attribute values** (#340, `1dfe7a1`). The source comment justified the
pass-through with "CI consumers are friendlier when newlines stay
literal", actively misstating the spec. Latent, because no production
path fed a path with embedded `\n` into an attribute value — POSIX
permits them in filenames, and a future multi-line message template
would have silently lost its structure on every consumer. The regression
test re-parses the emitted XML with `quick_xml` and confirms the
round-tripped value byte-equals the original; emitter-side byte
inspection could not have caught it.

---

## 46. A pasted BOM literal is three Latin-1 codepoints, not U+FEFF

**Lesson:** Any non-ASCII source literal that exists to match a runtime
value should be written with `\u{...}` escapes, not rendered glyphs. The
compiler accepts both; only runtime comparison reveals the divergence.
The mojibake-vs-canonical class recurs whenever you copy-paste BOM,
zero-width, right-to-left, or Asian-range text from an editor that
mis-decodes the input. Defensive accept-both is safer than
canonical-only when the source of truth is well understood.

`"ï»¿"` is three codepoints (U+00EF U+00BB U+00BF) — the UTF-8 BOM's
three bytes reinterpreted as Latin-1 chars. The canonical BOM any UTF-8
decoder produces is one codepoint, U+FEFF. Their `chars()` iterators are
disjoint and `==` returns `false`.

**`sanitize_identifier`'s BOM check matched only the mojibake form**
(#345, `fed31a4`). `enums/src/common.rs` had `if name == "ï»¿"`, intended
to map a BOM token to a stable `"BOM"` identifier. tree-sitter exposes
node kinds as valid UTF-8, so a grammar surfacing a BOM token returns
U+FEFF and the branch misses. The fall-through landed in the generic
character loop, where U+FEFF hits the `_ => continue` catch-all, produced
an empty identifier, and triggered the `Anon{i}` fallback — generating an
`Anon<N>` variant instead of the `BOM` identifier the code claimed to
emit. Reachable but latent. The fix matches both forms explicitly with
`\u{FEFF}` / `\u{00EF}\u{00BB}\u{00BF}`.

---

## 47. Bound the thread stack to make stack-overflow tests deterministic

**Lesson:** Spawn the walker on a thread with `stack_size` explicitly
bounded — never libtest's default. The bound must be tight enough that
any plausible recursive descent at the chosen DEPTH overflows it under
every realistic optimization, and loose enough that the iterative form's
working memory fits. Prefer letting the tree drop normally over
`mem::forget`, so teardown is exercised rather than stepped around.

Libtest's per-test stack size follows Rust's spawn defaults —
historically 2 MiB, overridable via `RUST_MIN_STACK`, not stable across
versions or build profiles. A recursion frame for a small walker is
roughly 150–250 bytes, so in release builds 10,000 frames may fit
comfortably and the test passes against the very bug it claims to catch.
A deliberately tiny stack (`stack_size(256 * 1024)`) makes the failure
deterministic.

**`deeply_nested_spaces_do_not_overflow_stack` initially used
DEPTH=10_000 on the default stack** (#338's regression test, hardened in
`940a56a`). Review flagged that release optimization could leave 10,000
small frames fitting in 2 MiB, so the test would pass against the
reintroduced bug. The fix spawned the body on a 256 KiB worker and
bumped DEPTH to 50,000.

**Update (#1056): the `mem::forget` half of this advice no longer applies
here.** `FuncSpace`, `Ops`, `AstNode`, and the two `wire` mirrors now
carry hand-written iterative `Drop` impls, so a deep chain tears down in
constant stack. The three `mem::forget` / flatten-before-drop workarounds
this lesson originally prescribed were removed, and letting the tree drop
normally is now the *stronger* test. The advice still stands for any
recursive type lacking such a `Drop`.

---

## 48. Hand-written enum lists need a match-based companion to enforce exhaustiveness

**Lesson:** Any time you maintain a hand-written list of enum variants —
for parameterized tests, dispatch tables, name-lookup matrices, or "the
canonical iteration order" — add a co-located match-based guard whose
arms list every variant. The guard need not be called; the compile error
is the guarantee, and `#[allow(dead_code)]` is the right attribute. Note
the placement: a guard inside `#[cfg(test)]` fires only under the test
target, so `cargo test --workspace --all-features` is what catches the
drift and a bare `cargo build` will not. Say so in the guard's doc
comment.

`const FOO: &[Enum] = &[Enum::A, Enum::B]` looks like "every variant"
but the compiler does not enforce it. Adding `Enum::C` without extending
the array compiles cleanly; only `match` expressions trigger
`non-exhaustive patterns`. `#[non_exhaustive]` does not weaken this —
within the defining crate exhaustiveness is still checked; only
cross-crate matches need the wildcard (lesson 39's opposite-direction
concern).

**`ALL_VARIANTS` in `src/metric_set.rs::tests` was advertised as
compile-error-on-drift and was not** (#339, hardened in `654f24c`). Five
tests iterated the list. Adding `Metric::Foo` without extending the array
would have silently lost coverage in all five until `Display`/`FromStr`'s
`match self` surfaced it through an unrelated path. The fix added a
sibling `fn _all_variants_exhaustive_guard(m: Metric)` whose arms must be
extended in lockstep; a missing one fires `E0004`.

Lesson 51 covers the runtime half: a compile-time guard proves the list
is complete, not that each arm dispatches to the right target.

---

## 49. Unused `macro_rules!` captures are documentation lies

**Lesson:** Audit every `macro_rules!` capture against the expansion body
during review. A capture the body never expands is a documentation lie —
the call-site syntax says the value matters when it does not, and the
drift is invisible to every standard gate. Two acceptable fixes: drop the
capture, so the syntax matches the semantics; or wire it through the
body, so the syntax becomes load-bearing and disagreement is a compile
error. Pick the easier one. When the macro is hand-rolled because
variants need bespoke per-arm logic, drop the capture and lean on
[`macro-comments.md`](../../.claude/rules/macro-comments.md) to preserve
the per-call narrative at the call site.

Decorative is not neutral: a call site reading `(Cpp, tree_sitter_cpp)`
*looks* declarative — as if the macro dispatches to that crate — when the
hand-rolled body picks a different one entirely.

**`enums::mk_get_language!` captured `$name` but hardcoded every match
arm** (#344, `0b417f2`). The `mk_langs!` driver listed
`(Cpp, tree_sitter_cpp)`, but `mk_get_language!` expanded to a 21-arm
hand-written match where `Lang::Cpp => tree_sitter_mozcpp::LANGUAGE`.
Verified via `cargo tree`: `tree-sitter-cpp` was pulled in only as a
transitive of `bca-tree-sitter-mozcpp`, never directly. The fix dropped
the second tuple element, collapsed the call site to a bare `Cpp`, and
gave every non-obvious mapping a per-line `// -> <crate>` comment.

---

## 50. Independent dispatch paths counting the same event mask each other's bugs

**Lesson:** When a metric has multiple independent paths summing into the
same field, write at least one regression test whose input **only** the
path under test can classify — a bare identifier for a walker arm that
handles `!`/paren wrappers, an empty container for an arm that descends
into children, a single-arm `switch` for a container-vs-arm counter. Then
test-via-revert each new arm independently and confirm it fails when
that *one* arm is dropped. When auditing an existing metric, identify
every independent path contributing to the field and ensure each has an
input no other path covers.

Both paths add into the same `Stats` field, so any fixture covered by
*either* reads the right total and passes. The dead path is invisible
from the result alone. Distinct from lesson 19 (a missing arm in a single
dispatch table) and lesson 7 (an *upstream* filter masking the code from
the input): here the arm is present, the input reaches it, and the test
still passes because a parallel path summed the same count.

**C# `csharp_walk_for_conditions` was dead code for every existing test**
(#370, `6384590`). The `IfStatement` / `WhileStatement` / `DoStatement`
arms targeted `csharp_inspect_child(node, 1, …)` and `(node, 3, …)`,
which in tree-sitter-c-sharp land on the literal `(` and `while` token
children — the condition lives at child(2), or child(4) for do-while.
Every C# ABC test used a comparison operator inside its condition, and
those tokens were counted by an *independent* token arm. The helper
contributed zero on every input. The bug predates C# support and survived
the #369 refactor verbatim, because the refactor preserved dispatch
shapes without altering input coverage. Only a bare-identifier `if (x)`
or unary `if (!x)` could expose it.

**The same masking recurred on `BooleanLiteral` while reviewing that
fix** (#371, `efe38b7`; Groovy follow-up `f132990`). The new
`csharp_count_condition` matched the leaf tokens `True` / `False` but not
the `BooleanLiteral` wrapper the grammar interposes for a condition, so
`if (true)`, `while (false)`, and `!true` all scored 0 — but only when no
other condition token fired in the same statement. Same root cause,
different node shape, within one week.

---

## 51. Hand-rolled match arms drift from their enum list without an integration coverage guard

**Lesson:** Any hand-rolled dispatch macro emitting one match arm per
enum variant — `mk_get_language!`, `mk_action!`-style routers, manually
typed `From<X> for Y` impls — needs a sibling integration test walking
every variant and pinning the result to a **directly imported**
reference. Compare against the import, not the macro under test, or the
test is tautological. Pair the per-variant tests with a variant-count
guard (`Lang::into_enum_iter().count() == EXPECTED_VARIANT_COUNT`) so
adding a variant without extending the test trips it. Workspace-excluded
crates need this wired into a per-crate recipe in `make pre-commit` /
`make ci`, mirroring `enums-check` from lesson 15 — `--workspace` does
not touch them.

Correspondence-by-convention has no compile-time tie: a typo in one
arm's backing crate, a missing arm for a new variant, or a copy-paste
resolving `Cpp` to `tree_sitter_mozjs` all type-check and ship silently.

**Dropping the unused capture fixed the lie; the dispatch table still had
no coverage** (#344 fix `0b417f2`; coverage added in #350, `0f16162`).
Lesson 49 traces #344 to `mk_langs!` capturing `(Cpp, tree_sitter_cpp)`
while the hand-written match resolved `Cpp` to mozcpp. Dropping the
capture eliminated the *lie* but left all 21 arms untested, in a crate
with no `tests/` directory that `cargo test --workspace` never reaches —
so the only signal would be a runtime panic reaching a caller. #350
added `enums/tests/dispatch.rs` with a per-variant
`lang_<variant>_resolves_to_<crate>` test comparing against a directly
imported `LANGUAGE`, plus the count guard. Test-via-revert: swapping the
`Cpp` arm to `tree_sitter_mozjs` fails `lang_cpp_resolves_to_mozcpp`.

---

## 52. Pre-order traversal evaluates parents before children

**Lesson:** Any pre-order AST metric treats the parent's combinator as a
**completed value** before any child is visited. Arms keyed on a child
node that try to influence the parent's already-computed result — "the
`!` resets the sequence", "the modifier downgrades the score" — run too
late to do what their comment says. The reverse direction works: state
established on the ancestor that pre-order reaches first, read by every
descendant. Never write back from a later sibling to an earlier one. A
sentinel cleared by a later token is only as reliable as its clear
event: when you replace one, enumerate every arm that *sets* it and
every path that skips the clear — the reported trigger is usually the
most visible member of a family — and perturb the *new* predicate, since
a preservation pin cannot fail against the old code.

When proposing such an arm, **write the failing test first** (assert
`cognitive("!a && !b && !c") > cognitive("a && b && c")`). If it passes
against the current implementation, the arm was already dead. If it
fails, the fix belongs at the token level — dispatch on the `AMPAMP` /
`PIPEPIPE` token, visited *after* its `UnaryExpression` siblings — not at
the expression level. A reset can also be dead because its *trigger*
never occurs; there, classify from a structural parent node instead of
threading state through a terminator the grammar may never emit. Do not
leave the dead arm in place as documentation of intent: it misleads
every subsequent maintainer about what the algorithm does.

**`BoolSequence::not_operator()` was dead at 15 call sites across 18
language impls** (#392, `0b30837`). The documented intent was "NOT resets
the sequence so the next boolean always scores +1." In pre-order the
`BinaryExpression` parent of `!a && !b && !c` is visited first —
`eval_based_on_prev` runs against the empty prior sequence and the `&&`
scores its +1 — and only then does the walker reach the `UnaryExpression`
children where the reset fires. By then the `&&` is counted, and the
reset can only affect future `BinaryExpression` nodes that
`eval_based_on_prev`'s span check already prevents from continuing.
Empirically `!a && !b && !c`, `*a && *b && *c`, and `a && b && c` all
scored identically. The arms were removed wholesale; the only behaviour
change was that `a && !(b && c)` now collapses into one sequence — the
one case where the reset genuinely fired first — matching SonarSource's
intent.

**A sibling write-back the element node never saw** (#421, `620c5aa8`,
refining #417 `0f499b41`). Comprehension `for` / `if` clauses are
*siblings* under the comprehension node, so a `for_in_clause`'s nesting
increment was written back onto the shared `nesting_map` slot for later
siblings. But a comprehension nested in another's *element* position
(`[[y for y in x if y] for x in xs if x]`) is visited before the outer
clause writes back, so it under-counted (6 against 10 for the equivalent
explicit loops). #417 shipped this as a documented limitation; the
follow-up deleted the write-back and derives depth on the comprehension
node itself — visited first, so every descendant inherits it regardless
of sibling order.

**A reset keyed on a delimiter the grammar never emits is dead the same
way** (#455, `f633300f`). Kotlin's ABC assignment counter pushed a
`Const` sentinel on a `val` declaration and cleared the stack only on an
explicit `SEMI` — but tree-sitter-kotlin emits *no* `SEMI`, even for an
explicit semicolon. The sentinel leaked past the declaration and
suppressed every later standalone `=`, so `val x = …; a = 1; b = 2`
reported zero assignments; the comment calling the implicit-terminator
case "benign" actively misled, and a permissive `Var` left by `var`
declarations masked it in the existing tests. The fix classifies the `=`
structurally from its parent (`property_declaration` /
`class_parameter`), so no sentinel can leak.

**The same stack leaked two more ways** (#1277). ASI makes JavaScript's
`SEMI` optional, so `const a = 1` without one suppressed every later
`=`, and TypeScript's `x as const` promoted a live `let` slot even in
semicolon-terminated code. The stack was kept for Java, Groovy and C# on
the belief that their grammars require the `;` (Groovy's does not) and
leaked anyway, arriving too late: live from `final` to the next `;`, it
scored `final Runnable r = () -> { x = 1; };` as zero. All seven
languages now classify the `=` from its parent chain.

---

## 53. Positional `node.child(idx)` breaks when the grammar permits an optional preamble slot

**Lesson:** Prefer `child_by_field_name(role)` over positional
`node.child(idx)` for any slot whose grammar permits an optional
preamble. The full rule, including the preamble inventory and the
fallback when no field is exposed, is in [`grammar-dispatch.md`
§3](../../.claude/rules/grammar-dispatch.md). The minimum new-test bar
for a dispatcher arm is **one fixture per optional preamble the grammar
permits** — not just the form the corpus already has. When migrating a
slot from positional to field addressing, dump the AST for *each*
spelling of the construct before choosing the fixture: two spellings
that look interchangeable can put the `;` at different indices, and an
empty-slot regression test written against the wrong one passes
vacuously under the old code.

Each language's grammar makes a different choice about which slots are
optional, so the bug is per-language and per-statement-kind, not
per-walker. The grammar exposes roles by name precisely because position
is not load-bearing.

**One review pass found four positional-child bugs** (#395 / #403,
`57547a1`, `5db8078`). Extending the unary-conditional walker from three
languages to eleven produced four silent miscounts: PHP `Argument` wraps
both `m(!$a)` (one named child) and `m(name: !$a)` (three), and the
dispatcher took `child(0)` — the *name* — so the named form reported zero
conditions. Go's `if x := f(); x` puts the declaration at `child(1)` and
the condition at `child(2)`, so the dispatcher counted the assignment.
C++'s `if constexpr (cond)` shifts the condition clause from `child(1)`
to `child(2)`, so the constexpr form returned zero. Lua's `repeat … until`
exposes a `condition` field the dispatcher ignored in favour of
positional lookups fragile to BLANK-ALIAS shifts. All four survived the
feature commit, a simplify-rust pass, *and* an audit-tests review,
because no pre-existing test exercised the optional-preamble form for any
of the four languages — the fixture corpus had grown around the simpler
shape.

**Two `for` headers that read alike put the `;` at different indices**
(#1276). `for (int i = 0; ; i++)` and `for (i = 0; ; i++)` scored
differently under the positional cascade because Java's
`local_variable_declaration` swallows its own `;` and Groovy's does not
— so the empty-condition fixture first written for the fix passed
against the old code and had to be respelled. In the same change, Go's
`node.child(1)` survived the author's own rewrite of the function while
comment-survival tests shipped for the two languages beside it. Every
other grammar the fix touched exposes a `condition` field. One level
deeper, every `*_inspect_container` but Tcl's and iRules' still unwraps
with `node.child(1)`, so `if ((/* n */ a))` scores zero there — #1181's
defect inside the wrapper, left open.

---

## 54. A "no-op regen" must be proven by an actual regen + diff

**Lesson:** Treat any "this regeneration is a no-op / metric-neutral"
claim as a hypothesis to discharge by running the generator at the pinned
version and diffing the **full** output — `grammar.json`,
`node-types.json`, `parser.c`, and every file under `src/tree_sitter/`.
Never assert it from a marker, a baseline, or a previous contributor's
note. When you bump a notification-only marker, run the matching
`generate-grammars/generate-*.sh` in the same change; when you hand-apply
generated output, commit exactly what the tool emits, not the hunks you
anticipated. A gate comparing two *declarations* (marker vs baseline)
gives false confidence unless paired with a test exercising a construct
only the declared version can parse — that test, not the gate, is what
pins the artifact.

**The bundled `tree-sitter-mozjs` parser was stale at JavaScript 0.23.1
for months while every declaration claimed 0.25.0** (#407, `48bc293`;
root cause in #1207 / #400). The marker was bumped without re-running
`generate-mozjs.sh`, and #400 then pinned the `grammar-marker-sync`
baseline at 0.25.0 on the recorded belief that the regen was a no-op — a
belief never verified. The real regen rewrites ~110k lines of `parser.c`
and adds the `using` / `await using` declaration that 0.23.1 lacked;
`grammar.json` is ABI-independent, so this is a true base-grammar
difference. The gate stayed green throughout because it compares the
`Cargo.toml` marker against a baseline string, never against the bundled
`src/parser.c`. The bump turned out metric-neutral for the existing
corpus — luck, not design — and `mozjs_parses_using_declaration` now
pins the capability.

**A subtler half-regen in `tree-sitter-mozcpp`** (#406, `c3c58930`; gap
fixed in #407). That fix advanced `parser.c` and `parser.h` to the 0.26.9
form but left `src/tree_sitter/array.h` at the pre-0.26 layout, so the
crate was stamped as 0.26.9 output while a bare `tree-sitter generate`
would re-diff `array.h` every time. Caught only because a *full* regen on
a sibling grammar surfaced that the grammar-independent runtime header
did not match. Committing the subset you expected to change, in place of
the tool's complete output, is the same stale-artifact bug in a smaller
hat.

---

## 55. A complexity score can be a metric artifact

**Lesson:** `AGENTS.md` carries this as policy under "Responding to bca
metric feedback": attribute the score before sizing the fix, then size
every new helper against **all** gated metrics using the live `bca.toml`,
never an issue's quoted table. The evidence below is why all three
clauses are there.

**Rust's `?` inflates cyclomatic *and* nexits** (#401, `ce36a04`). Each
`?` is a `TryExpression`, counted as a decision point (it desugars to a
match with an early-return `Err` arm — a real CFG edge) *and* counted as
an exit. `dump_tree_helper` measured cyclomatic 32 / nexits 20, but only
~12 of the cyclomatic was real branching — already under the 15 gate. The
other ~20 points were a linear, easy-to-read sequence of fallible
`write!` calls. The split was still worth doing (it killed an
eight-argument signature and made the writers unit-testable), but the
"32 → 4" headline overstates the win. Whether `?` should count toward
cyclomatic at all is a separate metric-design question (#409).

**The proposed split would have failed nexits** (#401). The issue's plan
grouped the write calls into helpers of ~6 `?` each — fine for
cyclomatic, but each carried nexits 6, over the limit of 5 and over the
0.95 headroom band. The fix was a `paint` helper folding set-color plus
write into one fallible call, dropping each writer to ≤3 exits.

**The issue's threshold table was wrong** (#401). It stated the `nargs`
limit was 5; it is 7. The original `nargs = 8` was the real breach, but a
helper sized to a fictional limit of 5 would have been needlessly
fragmented.

---

## 56. A similarity hash must exclude the dimension it claims to be insensitive to

**Lesson:** A near-duplicate digest is defined as much by what it drops
as by what it keeps. Enumerate every transformation the match is
supposed to survive — whitespace, the symbol's own name, ordering — and
prove the digest is invariant under each one with a test that actually
applies that transformation. A test hashing two unrelated strings and
checking they differ does not prove insensitivity; only a
before/after-the-edit pair does.

**The fuzzy-baseline body hash matched everything *except* a rename,
which was its entire reason to exist** (#377).
`--baseline-fuzzy-match` keeps a renamed-but-unchanged function covered
by hashing its body instead of keying on the now-changed qualified
symbol. The first implementation hashed the full source span — but the
**declaration line carrying the name is inside that span**, so
`fn classify(…)` and `fn categorize(…)` produced different digests and
the rename still surfaced as `[new]`. The headline feature was a no-op
for its headline use case. Only the integration test
`fuzzy_match_covers_renamed_function` caught it; every unit test of the
hash passed, because none of them renamed anything. The fix elides
whole-word occurrences of the function's own bare name before hashing.

---

## 57. A structural AST shape is not a semantic identity check

**Lesson:** When a metric's correctness hinges on *identity* — which
object, which name, which symbol — and the source text is available,
compare the bytes with `code.get(start..end)`. Reserve structural
pattern-matching for structural questions ("is this an attribute
access"). A shape-only proxy passes every test whose fixtures happen to
use the expected identity and silently mis-handles every other receiver;
if you must approximate, document it as an under-approximation and prove
the boundary with a fixture using a *foreign* receiver. Do not reach for
`to_string_lossy()` here (lesson 43).

"An `Attribute` whose first child is an `Identifier`" *looks* like
`self.x`, but it is equally `db.x` and `logger.x`. The shape answers "is
this an attribute access", never "whose attribute is it" — and when the
source bytes are already threaded through the metric, the proxy is not
even cheaper.

**Python NPA counted every `obj.x = …` as a class attribute** (#412,
`a06a07fa`). `python_lhs_is_self_attribute` classified an assignment LHS
purely structurally and never read the receiver, though `code` was in
scope. A `Service.__init__` wiring `self.name` alongside `db.connection`
and `logger.level` reported three class attributes instead of one —
dependency-injection wiring, the most common shape of `__init__`,
inflated NPA directly. The fix matches receiver bytes against
`PYTHON_SELF_RECEIVERS`, borrowing from `code` so the slice doubles as
the dedup key, and separates `self.f.g = 1` (an attribute on `self.f`)
from `self.g = 1` — a distinction the structural proxy could never make.

---

## 58. A wrapper-node + keyword leaf is one operator, not two

**Lesson:** Classify exactly **one** kind per operator and verify with
`bca ops` that the occurrence count matches the source. For a compound
operator wrapping reusable leaves, classify the compound and
parent-guard the leaves to `Unknown` under that compound only — a
blanket suppression drops every legitimate standalone use. See
[`grammar-dispatch.md` §5](../../.claude/rules/grammar-dispatch.md).

This is the mirror image of lesson 50: there two independent paths
summing one field *masked* a zero; here one arm listing two aliases of
the same token *inflates* the count. Both are invisible until you assert
the exact operator stream.

**Python Halstead double-counted `await` and split `not in` / `is not`**
(#413, `4adf1a24`). The operator arm listed `Await | Await2` — the
expression node *and* its keyword token — so three `await`s scored
`n1=4, N1=8` instead of `n1=3, N1=5`, while `yield` was already correct
(only `Yield`, not `Yield2`), making `await` inconsistent with its own
sibling. The same arm dropped `lambda`, `match`, `case`, and `nonlocal`
entirely. The fix lists one kind per operator and classifies the
compound `Notin` / `Isnot` while a guarded arm returns `Unknown` for
`Not | In | Is` **only when their parent is the compound** — so
standalone `not x`, `a in b`, and `for x in y` keep counting.

**The same double-count hit Ruby closures, in a different metric** (#465,
`7e4328a0`). tree-sitter-ruby parses a stabby lambda `->(z) { … }` as a
`Lambda` node *containing* its body `Block`/`DoBlock`, and
`RubyCode::is_closure` matched all three, so one lambda scored
`nom.closures = 2`. The keyword forms `lambda { }` / `proc { }` were
already correct, so only the stabby form was asymmetric. Same
parent-guard fix. The trap generalises past Halstead: any predicate
listing both a container and a kind it can contain double-counts.

---

## 59. A rule re-implemented in every module is a recurring regression class — give it one home

**Lesson:** When a rule must hold identically across modules that
deliberately mirror each other, declare it once — a `Getter` trait
default, a `Node` primitive, a shared predicate — and let each caller
contribute only its own parameters. **A recurring-regression issue trail
(one fix per language, or per surface, for "the same bug") is the trigger
to consolidate.** A pure consolidation is verified by zero snapshot drift
across every affected caller, including the integration snapshots.

**Enumerate the sites by the quantity they compute, not by the shape of
the bug.** A site that arrives at the same wrong answer by different code
does not match the pattern you are grepping for, and consolidating the
sites that *do* match converts one surface's bug into a divergence
between surfaces — a worse failure, and one no existing parity test
covers. A report's list of affected sites is a starting point, not a
census: #1271 named five window-boundary subtractions, and a sweep for
the quantity found a sixth in `BlameWalk::new` and a seventh in
`days_between`, whose shape matched no search for the reported one.

The cost is not the duplication but *omission by default*: a newly added
language — or a sibling cloned from a template predating the rule —
ships without it and silently produces wrong output until its own
bespoke fix lands. The compiler gives no signal. Distinct from lessons 48
and 51, where a hand-rolled *list* drifts from its enum; here the
duplication is behavioral.

**The Halstead string-interpolation operand skip was re-patched in nine
sites across seven issues** (#420, `0b899836`). The rule — a string
literal is one operand *unless* it wraps interpolation, in which case the
wrapper yields `Unknown` because the inner expressions are walked
separately — was implemented independently in the JS-family macro,
Python, C#, Kotlin, Perl, Tcl, PHP, Elixir, and Ruby, with three
different mechanisms. The trail
`#180 → #183 → #184 → #191 → #192 → #199 → #277` is the same skip
rediscovered per language, and any new
interpolating language double-counted the wrapper into `N2` by
omission. The fix introduces
`Getter::string_operand_type(node, interp_kinds)` over a
`Node::wraps_any(&[u16])` primitive; each language supplies only its own
interpolation child-kind set, so a new language gets the skip for free.
The bespoke Tcl/PHP multi-kind helpers were retired with their exact
kind sets preserved, and per-language rationale comments kept at each
call site.

**The same shape across CLI surfaces, where the minority case is where
they first diverge** (#825, #827, #837; formerly lesson 78). Most metrics
are higher-is-worse; the Maintainability Index family is not — a *drop*
is the regression. Three surfaces each re-derived "did this get worse?":
`diff-baseline`'s worsened/improved bucketing, the `baseline` `Covered`
ratchet, and `check`'s hard-breach escalation. All three handled
higher-is-worse correctly and all three got `mi.*` backwards, so an MI
decrease was reported as an improvement and `--worsened-only` selected
the wrong rows. The bugs persisted because **no test ever set
`lower_is_worse`**, so their vacuous coverage agreed with the inverted
code. The fix routes all three through one
`thresholds::breaches_limit(value, limit, lower_is_worse)` keyed on the
`metric_catalog::lower_is_worse` registry, and derives the baseline
`Covered` arm as `!breaches_limit(…)` so the ratchet and the gate cannot
disagree. When you add a metric or any surface that ranks or compares
values, wire it to the shared predicate and add a test exercising a
*lower-is-worse* metric specifically — a test covering only the majority
case passes against inverted direction logic.

**The third producer did not look like the bug** (#1163). The span
end-row rule was reported in two places, `FuncSpace::new` and
`Ops::new`, both spelling it as a `match` on `SpaceKind`. `function.rs`
computed the same quantity as a blanket `end_row() + 1` with no kind
branch at all, so it matched neither the buggy pattern nor a search for
`SpaceKind`. Fixing only the two named sites would have left
`bca functions` disagreeing with `bca metrics` about where a Perl
function ends — a cross-command divergence introduced *by* the fix, in
the release that closed the original report.

**Delete the extra copy; a corrected duplicate re-arms the bug**
(#1195, #1247). Three sites held "where does the unit's row span start"; #1195
anchored one and #1247 was the drift between the other two. The fix
removed both duplicates and reads the span back from the single owner —
teaching each copy the same rule would only have staged the next drift.

---

## 60. "Fails on the branch" is not "fails on `main`"

**Lesson:** Bisect against `main` before calling a failure pre-existing.
A downstream assertion green on `main` and red on your branch is your
regression to fix, not a background condition to step around. And run
`cargo test --workspace --all-features` after any change to metric
computation or AST traversal — `AGENTS.md` requires it because the
crates that pin concrete metric numbers are exactly the ones a
library-scoped run skips.

`big-code-analysis-cli`, `-web`, and `-py` each assert concrete values:
the web crate's `test_web_metrics_json` compares a full serialized blob
byte-for-byte, and the py SARIF / threshold tests pin per-metric numbers.
`cargo test -p big-code-analysis` never compiles any of them.

**The #437 LOC min/max fix went stale in the web crate** (#437,
`cbe18b21`; fix `bdc44a13`). Making `compute_minmax` include each
container's own span legitimately raised `sloc_max` / `cloc_max` /
`blank_max`, shifting `test_web_metrics_json`'s expected JSON. The review
ran only `-p big-code-analysis` and merged clean; the failure surfaced at
the full-workspace gate. A subsequent agent then dismissed it as
"pre-existing" without checking `main` — but it passed on `main` and
failed on the branch, which is the definition of a regression.

---

## 61. The label-child node kind is grammar-specific

**Lesson:** Before reusing a sibling's child-kind-gated predicate in
another language, dump the AST for the construct in the *target* grammar
and confirm the gating kind appears there — the semantic role transfers,
the node kind does not. The per-family table is in
[`grammar-dispatch.md` §4](../../.claude/rules/grammar-dispatch.md). Add
a fixture exercising the gated branch (a labeled jump, not a plain one)
and test-via-revert that branch alone; a suite of only unlabeled inputs
proves nothing.

A copied gate compiles, runs, and matches nothing, because the kind it
names never appears under that grammar's construct. No compiler signal,
no clippy warning, and no test failure unless a fixture exercises the
exact gated shape.

**The SonarSource jump-statement fix needed a different label kind per
grammar family** (#435, `e81b3f31`). Adding labeled-`break`/`continue`
gating to the shared `js_cognitive!` macro required
`is_child(StatementIdentifier)`: JS-family labels surface as
`statement_identifier`, not the `Identifier` Java and Groovy use, nor
Go's `LabelName` or Perl's `Label`. Copying Java's gate would have
silently scored every `outer: for (…) { break outer; }` at +0. The fix
was verified to resolve `StatementIdentifier` for all four enums the
macro instantiates before relying on it.

---

## 62. Recovering a poisoned `Mutex` needs `clear_poison()`, not just `into_inner()`

**Lesson:** When you degrade rather than propagate on a poisoned shared
lock, enumerate **every** site that acquires it — across crates — before
deciding the recovery is complete. If any downstream acquirer would
re-panic, a local `into_inner()` is a half-fix; call `clear_poison()` so
peers see a usable lock. Anchor the regression test on the
poison-cleared invariant (`!is_poisoned()`), not merely on the absence of
a panic, or a bare-`into_inner()` regression slips through. Recovery is
only justified when the guarded data tolerates a partial peer update.

Poisoning is sticky: `lock().unwrap_or_else(|e| e.into_inner())` hands
back the inner data for *this* acquisition without clearing the flag. So
if more than one site acquires the lock, recovering in one fixes only
that one; every other acquirer still sees `Err` and, if it unwraps,
re-panics — often in a *different crate* from the one you patched, so a
green library-crate test hides it.

**Degrading `Count::call` on a poisoned `stats` mutex** (#445,
`995c6fbb`). The worker aggregates into a shared `Arc<Mutex<Count>>`; a
panicked peer poisons it and the old `.lock().unwrap()` cascaded a
pool-wide abort — the hazard #425 fixed in `dispatch_preproc`. A bare
`into_inner()` would have let `Count::call` return `Ok(())`, but the
CLI's `run_command_count` reads the same lock with
`into_inner().expect(...)` and would have re-panicked on the still-set
flag. Here the guarded data is two monotonically-incremented counters,
so the worst case is a slight undercount, never an unsafe state.

---

## 63. Opening a FuncSpace for a method-nested node double-counts the ancestor's WMC

**Lesson:** Whenever you make the space-builder open a FuncSpace for a
node that can be lexically nested inside a method, audit WMC in the same
change. WMC's per-method contribution is a *cumulative subtree sum*, and
a newly-opened child space does not subtract itself from that sum
automatically, so the ancestor class silently double-counts. Fix it once
in the shared merge — track and subtract the nested class/interface
cyclomatic — not per language, and pin a test that a
method-with-nested-class scores the same class WMC as the nested
construct hoisted to top level.

Distinct from lesson 50 (two parallel paths summing one field) and lesson
24 (finalize gating): here a *single* path's cumulative accessor
over-counts because the tree shape changed under it.

**Adding anonymous-class spaces exposed a latent double-count** (#463,
`49ed0b20`). Making Kotlin `object_literal` and Java anonymous-class
bodies open their own `Class` space was correct in isolation, but a
`Function` space's WMC contribution reads `cyclomatic_sum()`, which
still folds in any class nested in the method body — so a method
containing an anonymous class counted it twice. `Wmc::merge` now carries
`nested_class_cyclomatic` and subtracts it
(`own_cyclomatic = other.cyclomatic - other.nested_class_cyclomatic`); a
nested `Class` records only its *own* cyclomatic, never its own nested
total, which would re-double deeper classes. The recording lives in the
shared `class_interface_compute`, so every OO language inherited it.

**Placement is not membership** (#1301). A C++ `friend` defined inline
opens its `Function` space where the grammar puts it — inside the class
— but is a free function, so `npm` excluded it while `wmc` weighted it:
the #1258 divergence surviving in another shape. The single-pass walk
cannot reparent the space and `Wmc::merge` holds no node to ask, so the
membership call is recorded at `open_func_space` via
`Checker::is_non_member_function` (whose doc carries the design
rationale) and consumed as a flag in the shared merge. Objective-C's C
helpers inside `@implementation` / `@interface` / `@protocol` are the
same shape, fixed the same way (#1356) — there the predicate keys on the
node's own kind, because a method is always a `method_definition` and no
parent-kind list can be completed.

---

## 64. A default/fallback-arm exclusion is per-construct

**Lesson:** The exclusion is anchored to the **construct**, not the node
kind. Confirm the construct's own node pays the nesting/decision
increment — so its default really is redundant — before suppressing, and
confirm the shared node kind is not also serving a chain construct where
the arm is a legitimate +1. See
[`grammar-dispatch.md` §8](../../.claude/rules/grammar-dispatch.md) and
lesson 11 for the cross-metric agreement rule.

**Ruby `case`-else is +0 but `begin`/`rescue`-else is +1** (#451,
`30a435ae`). The shared `R::Else` node appears under three constructs. A
`case` / `case_match` parent already pays a nesting increment, so its
`else` is the switch default (+0, matching Kotlin `when`-else and Java
switch-default). But `begin` pays *no* nesting — only `R::Rescue` does —
so `begin` / `rescue` / `else` is the no-exception branch, the analogue
of Python `try` / `except` / `else`, which is +1. **The issue's own fix
plan proposed suppressing rescue-else too**; doing so would have
introduced a *fresh* divergence from Python. The fix gates only on
`parent ∈ {Case, CaseMatch}`.

---

## 65. Removing a node kind from `is_func` / `is_func_space` zeroes its childless variant

**Lesson:** Before removing a node kind from a function/space dispatch
set to fix an over-count, enumerate the construct's variants and find
the one with **no qualifying children** — it is relying on the
membership you are about to delete. Gate the arm on child-presence
rather than removing it, and keep `is_func`, `is_func_space`, and
`get_space_kind` gated by the *same* predicate so the space tree stays
self-consistent. The same holds for any classification arm, not only the
space dispatch: a kind that fills several grammatical roles must be
fixtured in each role before an arm naming it is deleted — the reported
fixtures show one role.

This is the inverse of lesson 19 (a *missing* arm scores a valid
construct as zero): here the arm exists and the fix is to narrow it —
and narrowing too far re-creates that zero for the childless sub-case.

**The C# expression-bodied indexer would have dropped to zero** (#464,
`8db1a3e8`). `Csharp::IndexerDeclaration` sat in both `is_func` and
`is_func_space` while `AccessorDeclaration` was in `is_func`, so a bodied
indexer counted as three functions and folded an extra entry into wmc. A
blanket removal would have fixed that — but the expression-bodied form
(`this[i] => …;`) has no `AccessorDeclaration` child and would have
counted zero. The fix gates on `!csharp_indexer_has_accessors(node)`,
opening a space only when there are no accessors to defer to, mirroring
npm's `.max(1)` reference.

**The C# expression-bodied property had the same latent zero** (#472,
`c381c117`). `PropertyDeclaration` sat in none of the three sets, so a
bodied property correctly deferred to its accessor spaces — but
`int W => _w;` opened no space at all and counted 0 while npm reported 1.
The fix generalised the predicate to `csharp_member_has_accessors` and
gated both member kinds on it at all three sites.

**A Tcl braced word is a script body and the value slot of every other
command** (#1354, #1317). Every fixture in the issue showed
`braced_word` as a `proc` / `if` / `when` body, billed as one operand
spanning the whole block beside the commands already counted, so
deleting it from the operand arm looked safe. It is also the value slot
of every command the grammar does not special-case, and `lappend l {}` —
an empty list whose brace pair is its only carrier — dropped to zero
operands where `lappend l ""` scores one. The arm is gated on holding a
non-comment named child; `node-types.json` names both roles.

---

## 66. A control-flow construct with no dedicated grammar kind escapes every kind-based dispatcher

**Lesson:** Audit command-dispatched languages (Tcl, iRules, shell-like
grammars) for builtins that are control flow, recognise them out-of-band
by their leading word, and locate sub-parts by **structural position**
rather than a fixed index so detection survives optional option/flag
prefixes. Add a fixture scoring above the base and test-via-revert that
the arm fires. See
[`grammar-dispatch.md` §9](../../.claude/rules/grammar-dispatch.md).

The gap is not a missing enum arm — the *kind* the dispatcher would need
does not exist for that construct in that grammar at all. Cousin of
lesson 61, where the semantic role transfers but the node kind does not.

**Tcl `switch` contributed zero complexity** (#467, `867d9753`). The Tcl
cognitive and cyclomatic impls dispatch on dedicated kinds
(`If`/`Elseif`/`While`/`Foreach`/`Catch`), but Tcl's `switch` is a
generic `command` whose first word happens to be `switch` — so a
three-arm `switch` scored cognitive `0.0` and cyclomatic `1.0` against
the equivalent C `switch`'s `1.0` / `3.0`. The fix detects it by the
command's `name` field and counts non-`default` arms. Crucially the arm
list is located by structural position — the sole trailing `braced_word`
argument — not a fixed child index, because the optional `-exact` /
`-glob` / `--` options and the matched value precede it; a positional
index would have broken on every option-form switch (lesson 53's failure
mode).

---

## 67. "Compute it once" is the wrong altitude when the consumers don't share the transform's parameters

**Lesson:** Before unifying N transforms of "the same" value, list each
consumer's *parameters*. If they differ, "compute it once" yields a value
that round-trips for one consumer and silently corrupts the rest — keep
the transforms separate and share only the genuinely identical sub-step.
**When a refactor's correctness depends on a parameter, vary that
parameter in the tests**, not just the inputs.

The "duplication" you set out to remove may never have been duplication:
N legitimately different transforms that merely looked alike.

**The `WalkFile` output-name unification, attempted then reverted** (#497
follow-up, preserved on `archive/walkfile-name-normalization`). `bca`
re-derived a file's canonical identity at four places — the
`[check.exclude]` glob matcher, the baseline keyer, `--changed-only`
scope filtering, and `bca diff` pairing. Four helpers answering "which
file is this?" read as a textbook "compute it once at the walk seam"
cleanup, so they were collapsed into a single canonical `name` relative
to the longest common ancestor of the seeds. It compiled, passed the
entire workspace suite and both self-scan tiers, and shipped **three
silent correctness regressions**: single-file `bca diff --since <file>`
reported every file as both added and removed; a subdirectory scope
wrote baseline keys that lost the scope prefix, so baselines stopped
suppressing; and `--changed-only` from a subdirectory dropped violations
— a gate bypass. The consumers have **heterogeneous anchors** — exclude
wants walk-root-relative, the baseline wants baseline-file-dir-relative,
`--changed-only` resolves against the CWD, `bca diff` wants
tree-root-relative — and one LCA-anchored string is *lossy*.

Two corollaries made it expensive to catch. The regressions were silent —
wrong keys and dropped violations, no crash, no failing test. And a
*uniform* gate hid them: every CI path and the self-scan run `--paths .`
from the repo root, the one configuration where all four anchors
coincide, so the coupling stayed invisible until a review varied the
invocation shape. Full analysis:
[`output_name_normalization_design.md`](output_name_normalization_design.md).

---

## 68. A branch that looks unreachable may need a non-obvious grammar shape

**Lesson:** When a defensive mirror fix's guarded branch seems
unreachable, do not skip the test — instrument the predicate (a
temporary `eprintln!` at the call site) and sweep candidate grammar
shapes empirically until one fires. **Childless and zero-width leaves are
the usual culprits**, and they appear in positions the source text does
not advertise: command substitutions, heredocs, interpolation wrappers.
If after a genuine sweep nothing reaches the branch, document that
analysis at the call site rather than shipping a vacuous test — but
treat "unreachable" as a hypothesis to disprove, not a default.

**The Bash `Loc` leaf arm omitted `check_comment_ends_on_code_line`**
(#547, `492b86d6`). Every other `Loc` impl — Elixir most directly,
sharing the exact `child_count() == 0` leaf-gating shape — calls the
reclassification helper before inserting a row into `ploc.lines`; Bash
did not, so a row carrying both a comment and code was double-credited
and `blank` undercounted. The fix is one line, but proving it looked
impossible: an ordinary `# comment` runs to end-of-line and routes
through the `is_comment_after_code_line` path, never the leaf arm.
Sweeping ~20 candidate Bash shapes with the predicate instrumented
surfaced the trigger — `echo "$(\n  # c\n)"`, where tree-sitter-bash
emits a **zero-width `word` leaf on the same physical row** as the
comment inside the command substitution. With that input the test fails
on revert and passes with the fix.

This is the find-the-trigger counterpart to lesson 52, where an arm that
*looks* live is dead because of pre-order ordering.

---

## 69. A line-prefix parser must disambiguate structural markers from body content

**Lesson:** Enumerate the content that can masquerade as each structural
marker and disambiguate by **parser state** (position in the grammar),
never by the prefix in isolation. And a regression test for a prefix
collision is only real if its fixture actually produces the colliding
prefix — verify by revert, because a fixture one character short passes
against the bug and guards nothing.

**The unified-diff scorer dropped deletions and corrupted paths on
`--`/`++` content** (#580; shipped in `dc03417d`, fixed in `4f9f293e`).
`bca vcs jit --diff` parses by line prefix: `+++`/`---` followed by a
space are file headers, and inside a hunk `+`/`-` are body lines. Under
git's single-char prefix, a *deleted* line whose content starts with `--`
and a space (SQL/Lua/Haskell/Ada comments) renders as
`--- this is a comment`, and an *added* line whose content starts with
`++` and a space renders as `+++ foo`. The header arms were checked first and were **not** gated
on "before the first hunk", so the deletion was silently dropped and the
`+++` line silently rewrote the file's path — corrupting both the size
feature and the diffusion path key, the very metrics the feature
computes, on entirely realistic input. The fix gates the header arms on
`!saw_hunk`, which the parser already tracked.

**The first regression fixture was too weak to trigger the bug it
guarded.** The collision needs the *three*-character marker plus a space,
which a body line produces only when its content begins with `--`/`++`
and a space. The remediation's first fixture used content starting with
just `--` (no following space), producing only a two-dash run after the
diff's leading `-` — so it passed against the *unfixed* code.
Test-via-revert exposed it.

---

## 70. A string-keyed metadata lookup shared across tables resolves collisions to the wrong context

**Lesson:** A string-keyed lookup is only safe while every consumer
agrees on what the string means; the failure mode is silent inheritance,
not an error. When the same label can mean different things in different
tables, move the metadata onto each table's own spec and pass it
positionally. Treat "the lookup returned something" as the weakest
possible assertion in tests.

The lookup returns *a* value for every key, so nothing fails: the second
context inherits the first's metadata, and an "every header carries a
tooltip" coverage test passes throughout, because presence is not
correctness.

**The bus-factor "Files" column inherited the files-analysed tooltip**
(#610, #611, `7a5a8947`, `391decdd`). The HTML report resolved header
tooltips via `header_tooltip(header)`, keyed on the literal string. The
per-language overview's "Files" column (files analysed) and the
bus-factor table's "Files" column (files per directory) collided, so the
bus-factor table displayed the overview's definition. The #610 text-only
pass had to *defer* the item — the string-keyed catalogue could not
represent two meanings for one label at all. #611 resolved it
structurally with a `tooltip` field on the shared `Column` / `VcsColumn`
specs, passed positionally to `write_table_with_tooltips`, which
simultaneously fixed the ambiguity and gave the Markdown legend the same
single source the HTML `title=` attributes use.

Related to lesson 22: both are cases where keying on display text
discards the context that gave the text its meaning.

---

## 71. Invalid input that collapses into the "not provided" branch fails as success

**Lesson:** At an input boundary, a parse failure must be an error value,
never the absent-value default. If a resolver returns `Option`, reserve
`None` for "not provided" and propagate present-but-invalid as `Result`
— or terminate with a message naming the bad input **and the valid set**.
The smell to grep for: a `FromStr` or lookup whose `Err`/miss is mapped
to `None`, `unwrap_or_default()`, or a filter that quietly matches
nothing.

Downstream code takes the default path, produces plausible-looking
(often empty) output, and exits 0 — the user's typo is rewarded with
success. This is the input-boundary sibling of lesson 19: there the
dispatch arm is missing; here the error value is erased at the boundary
before any arm is reached.

**`--language-type klingon` was silently ignored, and the forced-language
use case produced empty success** (#595, `ba14e9f9`). `resolve_language`
tried an extension-table lookup and returned `None` on a miss — the same
`None` that means "flag omitted, infer per file". So the walk fell back
to per-extension inference as if the flag were never given, and in the
flag's *core* use case — forcing a language onto files whose extension
cannot resolve — those files hit the skip-unrecognized guard and
vanished: no analysis, no error, exit 0. The fix resolves through the
enum's `FromStr` with an extension fallback, and a value matching
neither is a hard `die` listing the valid languages.

---

## 72. A breaking change must sweep the encodings of the old contract, not just its name

**Lesson:** Changing a contract obligates a workspace-wide sweep for
everything that encodes the old one *incidentally*: the old literal
values (`.code(2)`), the old shapes (`<h2>` without attributes,
multi-value flag spellings, file-extension assumptions), and the doc
pages that copy-paste the old invocation. A rename-sweep on the flag or
symbol misses every one. **Expect the first sweep to miss instances** —
after each fix, re-grep with a pattern derived from what you just fixed,
not from the original symbol name.

One batch produced four independent instances:

**The exit-code remap left `.code(2)` pins in four test files** (#594,
`c9f73d68`). Moving clap usage errors from exit 2 to exit 1 invalidated
assertions in `cli_smoke.rs`, `exemptions.rs`, `include_exclude_arity.rs`,
and `vcs_jit.rs` that had pinned clap's default incidentally while
testing something else. Only the full workspace suite surfaced the set
(lesson 60).

**Fixtures encoded the silent-ignore contract** (#600, `1f13df11`).
Making `check --output` infer the format from the extension broke a SARIF
test writing to a `.json` path — tolerated when the extension was
ignored, rejected once it became meaningful. Distinguishing "fixture
encoded the old contract" from "fix regressed real behavior" was the
actual work.

**Bare-tag substring needles broke when headings gained attributes**
(#622, `8f9b35b5`). Tests matched `<h2>X</h2>` literally; adding `id=`
falsified every needle. The *negative* assertions were treacherous: a
"table omitted" caption embeds the same title after a `>`, so an
over-broad replacement still matched — each had to become a
text-plus-close (`>X</h2>`) match chosen per assertion.

**A book recipe shipped hard-failing, and the first sweep under-swept**
(#601, `c2b89e73`, `b8c060f8`). The `-I`/`-X` arity change turned the
space-separated spelling into a usage error. The recipe page used it
twice *and* carried prose describing the old greedy behavior; the first
review sweep fixed one code block and missed the second plus the prose,
which survived until a later wave re-grepped for the invocation *shape*
(`--include "a" "b"`) rather than the flag name.

---

## 73. A filter that silently matches nothing is load-bearing

**Lesson:** Before fixing a predicate or pattern that silently matched
nothing, enumerate every consumer of the shared mechanism and ask what
each consumer's behaviour *becomes* once the match starts working — the
answer may be a design decision to surface, not a mechanical fix. When
tests fail after such a fix, bisect against the pre-fix binary before
blaming the environment, and treat "this test only passed because of the
bug" as an expected finding.

The surrounding system equilibrates around the bug. Other consumers, the
test suite, and even the project's own config come to depend on the
non-matching, so fixing it un-masks all of them at once and the fallout
presents as unrelated failures far from the changed line.

**The bare-relative glob fix changed explicit-file-seed semantics**
(#726, `1a2c5f29`). `mk_globset` compiles one globset consumed by two
sites: the directory-walk filter (matching `./`-anchored paths) and the
explicit-file-seed filter (matching the seed as spelled). Stripping the
leading `./` so `dir/**` finally worked for walks also made the repo's
own `.bcaignore` (`./**/tests/**`) match the absolute fixture path
`cli_smoke` names via `--paths` — seven tests went red with "0 files
matched". Those tests had passed only because the bug kept `./`-anchored
patterns from matching anything, and the fix forced a genuine design
decision the bug had been hiding: explicit file seeds now bypass the
exclude deny-set, following the ripgrep/fd convention.

**Two overlapping causes made the failures look environmental** (#726).
The first failure in the fresh worktree *was* environmental —
uninitialized submodules. After `git submodule update --init`, the same
seven tests still failed, now with "0 files matched": a second, distinct
cause behind the same test names. The misattribution broke only by
bisecting against the pre-fix binary — stash the fix, rebuild, re-run the
exact failing command, restore. The clean binary analyzed the file; the
fixed one refused.

---

## 74. A language that owns no file extension has no snapshot coverage

**Lesson:** When a `LANG` variant owns no file extension, the snapshot
corpus cannot cover it — add a parity test driving a shared input
through the extension-less variant and its extension-owning sibling and
asserting identical output. And treat any region-based bulk edit across
the `src/languages/` mirror clones as dangerous: scope each edit to a
single `impl` block, because a clone inserted between two siblings makes
a too-wide window silently corrupt its neighbour — in the one impl no
corpus will catch. Owning an extension is necessary, not sufficient: the
corpus test's *path selection* must also admit the file. Check for a
`.<ext>.snap` in the submodule before reasoning from "the corpus has
such files".

The integration corpus only exercises a variant when some file *routes*
to it by extension. An opt-in dialect selected only by name
(`--language mozcpp`, a manifest, the API) is invisible to the entire
submodule suite; its metric impls can regress with every gate green.

**The Mozcpp ABC arms were silently stripped while adding `LANG::C`, and
`make pre-commit` passed green on the broken state** (#721; caught in
review, fixed in `7c502af1`). `MozcppCode` is a deliberate clone of
`CppCode` — the upstream-vs-fork split mirrors #507's JavaScript/Mozjs.
A region-scoped bulk edit (`lines[start:start+220]` from
`impl Abc for CCode`) overran into the **adjacent**
`impl Abc for MozcppCode` — the clone insertion order is `Cpp`, `CCode`,
`Mozcpp`, so the window past CCode landed inside Mozcpp — deleting its
`AssignmentExpression2` / `NewExpression` / `<=>` / `try` / `catch` arms.
The full gate passed on that state, because Mozcpp owns no extension, no
DeepSpeech file routes to it, and no snapshot moved. Only an adversarial
review reasoning "Mozcpp and Cpp must agree on non-Gecko C++" caught it.
The fix added `tests/cpp_mozcpp_parity.rs`, verified by revert.

**Objective-C owns `.m`, fifteen `.m` files are checked out, and no
`.m.snap` exists** (#1356, #1316). All fifteen sit under DeepSpeech's
`tensorflow/`, and `tests/corpus/deepspeech_test.rs` snapshots an
explicit list of `native_client/` paths, so a corpus file is invisible
until someone adds it. Two fixes in one batch reasoned "the corpus
contains `.m` files, so zero churn means the shape is absent" and were
wrong for the same reason: neither the WMC member-scope fix nor the
char-literal operand has corpus coverage, and only their unit tests
hold. `fd '\.m\.snap$'` over the submodule returning 0 is the check that
would have said so.

---

## 75. A metric assertion that passes under the wrong grammar verifies nothing

**Lesson:** When a test's claim is "grammar X parses construct Y" — not
"the metric value is N" — assert the property that actually
distinguishes the grammars. For a clean-parse claim that is
`!root.has_error()`, not a downstream count. Sanity-check the
discriminating power by running the assertion against the grammar the
test means to exclude: if it still passes, the test verifies the wrong
thing.

tree-sitter error recovery is the trap: a grammar that ERROR-cascades
still builds a partial tree, and that tree often retains enough
structure — the function node, the `return`s — for count-based metrics to
match the clean parse exactly.

**`c_keyword_identifiers_parse_and_returns_count` passed under the C++
grammar it was meant to exclude** (#721, `7c502af1`). The test's whole
purpose is the motivation for `LANG::C`: C code using C++ keywords
(`new`, `class`, `delete`) as identifiers parses cleanly under
`tree-sitter-c` where the C++ grammar ERROR-cascades. But it asserted
only `functions_sum() == 1` and `nexits_sum() == 2`, and a probe showed
`CppParser` yields the *same* values on that input despite the cascade —
it recovers a function and two `return`s anyway. The test would have
passed even if `.c` had stayed routed to the C++ grammar.

Complementary to lesson 33: there each assertion slot is proved
exercised; here the slot *is* exercised but the assertion does not depend
on the behaviour under test.

---

## 76. *Merged into lesson 24*

A traversal-level filter that skips a subtree with a bare `continue`
suppresses only *accumulated* metrics; span-derived and
`finalize`-derived values never see the skip. Kept as a sub-example of
**[lesson 24](#24-a-cross-cutting-traversal-feature-must-reach-finalize-and-span-derived-metrics)**;
this number is retained because existing citations reference it. See also
lesson 83, which covers the other way `sloc` goes wrong.

---

## 77. Issue references in a `///` doc comment leak into `--help` and the man pages

**Lesson:** `AGENTS.md` carries this as an editing principle: issue and
PR references go in `//` maintainer comments, never in a `///` doc
comment on a clap type, and any edit to a clap help/about/value doc must
be followed by `cargo xtask` with the regenerated `man/` pages in the
same commit.

A `///` on a CLI type is user-facing **twice**: clap derives every help
string from it, and the checked-in man pages are generated from the same
definitions.

**A doc-accuracy fix leaked `(since #661)` into `bca metrics --help`**
(#841). Rewording the `///` on `MetricsFormat::Text` to correct a stale
claim kept an issue reference in the doc text. clap surfaced it in `bca
metrics --help` *and* `bca ops --help` (which share the format enum), and
both man pages drifted. The leak survived several waves of a batch fix:
the two gates that catch it — the
`help_text_carries_no_issue_references` test and the man-page drift check
— were both masked by an unrelated tooling mistake that suppressed the
gate's real exit status.

---

## 78. *Merged into lesson 59*

Per-metric direction (`lower_is_worse` for the Maintainability Index
family) belongs in one predicate every gate and report shares; three CLI
surfaces re-derived it and all three got `mi.*` backwards. Kept as a
sub-example of
**[lesson 59](#59-a-rule-re-implemented-in-every-module-is-a-recurring-regression-class--give-it-one-home)**;
this number is retained because existing citations reference it.

---

## 79. Key the derived digest, not the pre-image

**Lesson:** Before adding a key, salt, or per-run transform to an
identity a cache materializes, ask *what does the cache actually store?*
If it stores the derived digest, layer the new transform **outside** that
digest so replay reconstructs the output from cached state, and keep the
transform out of the cache's invalidation fingerprint — it is a
finalization concern, not a walk concern. Keying the pre-image silently
couples the cache key to the secret and forces either a re-walk on every
key change or spilling the pre-image to disk.

**Opt-in keyed author hashing had to harden published output without
breaking the #334 cache-replay invariant** (#956, `4493598a`).
`--emit-author-details` emits a SHA-256 of the canonical email, and the
persistent VCS cache stores only that digest — never the plaintext (a
`from_digest` identity *is* the digest). The obvious construction,
`HMAC(key, email)`, cannot be reproduced from a cached walk: the email is
gone, so replay would emit a different value and violate the
bit-identical-replay contract. The shipped construction keys the *inner*
digest — `HMAC-SHA256(key, hex(SHA-256(email)))` — applied at
finalization over the exact value the cache retained. A cached walk
re-finalizes under any key with no re-walk, and
`keyed_emit_survives_a_cache_round_trip` pins it. Security is unchanged:
the attacker must hold the key to compute the outer HMAC for any
candidate, so the public inner digest does not weaken it.

Related to lesson 56: there the digest must *exclude* the dimension it
tolerates; here the inner digest excludes the key so the cache stays
key-agnostic while the outer includes it.

---

## 80. An assertion that only runs at release time rots silently

**Lesson:** A check that runs only on a rare trigger is **not** a per-PR
gate — treat its load-bearing assertions as untested until they are
mirrored into the suite that runs on every PR, and extract
embedded-in-YAML scripts so they are lintable and locally runnable
(`make smoke`). The mirror counts only if it *discriminates*: when an
invariant is about a value's **representation** (integer vs float, one
error type vs another), assert the property that distinguishes them and
prove the discriminating power with a negative control.

Siting an assertion *only* on the rare trigger is legitimate but narrow.
It holds when the per-PR environment cannot produce the measurement
honestly — a wall clock on a shared runner — and then only if every
mechanical part of the guard is mirrored per-PR and the rare gate fails
fast rather than hanging. Audit such a gate for **direction of
degradation**: apparatus that emits numbers rather than verdicts tends to
report a truncated or empty measurement as an excellent one, so every
partial-measurement path needs its own explicit verdict rather than a
derived statistic.

**Three `v2.0.0` smoke assertions rotted between rc1 and the stable tag**
(#995, `6e23d46e`; hot-fixed in `c53e504b`). The wheel and release
workflows only run their matrices on a `v*` tag or an opt-in label, and
the assertions lived as inline shell/Python heredocs inside the workflow
YAML — invisible to `cargo test` and `pytest`. So the #530
integer-serialization change (`cyclomatic.sum` now emits `3`, not `3.0`)
and the #614 `AnalysisError` → `AnalysisFailure` rename left the smokes
asserting pre-2.0 strings; both stayed green on every PR and failed only
when the tag forced the matrix to run, blocking publication. The fix
extracts the smokes into checked-in `scripts/smoke/*`, mirrors the
invariants into per-PR tests, and adds a path-filtered `smoke-dryrun.yml`.

**The per-PR test that should have caught #530 coerced the regression
away** (#995). `format_smoke.rs` already round-tripped `cyclomatic.sum`
across JSON / YAML / TOML / CBOR — but every extractor read the value via
`as_f64()`, which yields `3.0` for both the integer `3` and the float
`3.0`. The round-trip passed identically before and after, so it could
never have flagged a `u64` → `f64` wire regression. The discriminating
check is `serde_json::Value::is_u64()`, now pinned with
`halstead.volume` as a negative control proving the assertion
distinguishes integers from floats.

**The converse: moving an assertion onto a rare trigger, and what had to
be true first** (#1068, `0eeb6fe0` / `93c23cd4`). The `cognitive` and
`tokens` deep-nesting tests carried wall-clock assertions per-PR, and
those produced a *false failure* in four environments: `windows-latest`
against an absolute 8 s budget (10.9 s), a local `make pre-commit`
running clippy alongside the suite (5.6x), the same host under load
(3.9x), and `cargo llvm-cov`, whose instrumentation skewed even a
best-of-three ratio to 3.5x. Since the `coverage` job runs in CI, the
assertion redded the build on a measurement artefact, and a shared runner
cannot produce an honest wall clock — so #1068 moved the timing half to a
quarterly bench. Three things made that acceptable rather than a rot
vector. Everything *mechanical* stayed per-PR as ordinary unit tests:
that each generated shape is affine in bytes, parses without error, and
nests proportionally to its depth parameter; that each probe's metric
reads non-zero on its own shape; that the log-log fit recovers 1.0, 2.0
and 0.0 on synthetic data. Second, the rare gate fails fast — a probe
exceeding `MAX_CELL_WALK` is abandoned before its deeper cells are built,
so a reintroduced quadratic walk is *reported* rather than left to run
out the job timeout. Third — what the first cut got wrong twice — the
apparatus degraded toward a flattering number: `run` pushed each cell
into the schedule before comparing it against the budget, so a rejected
cell was still walked once per round; and `Report::failures` reported an
abandoned probe by its fitted exponent, computed over the cells that
finished, printing `0.00 > 1.50` — a pass-shaped number for the worst
regression the gate can see. Both were caught in review, not by a failing
test, because neither produced a failure to catch.

---

## 81. De-recursing a traversal does not de-recurse the type it walks

**Lesson:** After de-recursing the walks over a recursive type, audit the
**type**: the projection that rebuilds it, every derived `Serialize`, and
the `Drop` glue — and record that `Clone`, `PartialEq`, and `Debug`
recurse for the same reason whether or not a current path reaches them.
Exercise each stage separately on a bounded stack (lesson 47 covers how
to bound it), because a fixture that clears one stage by 10x may clear
the next by 0.5x. Where the recursion is inside a generated `Serialize`,
**bound the depth and return an error** rather than assuming the
walk-side fix reached it: the overflow is a `SIGABRT`, not a catchable
panic, so no `catch_unwind` / `spawn_blocking` layer above it will help —
the process dies, taking every in-flight request with it.

The three recursions survive a traversal audit because they are
generated, not written: there is no `fn walk(...)` to grep for.

**#700 / #709 de-recursed every walk and left the types recursive**
(#1056, `93547880`). The three small-stack regression tests those issues
left behind all passed, because all three exercise the *dump* walk and
build their fixtures by hand. Meanwhile `bca metrics -O json` on 1,000
nested functions — 11 KB of source — aborted the process, and the `/ast`
endpoint aborted on 80 KB of nested parentheses. Measured abort depths on
a default 2 MiB thread, release / debug: `wire::FuncSpace::from` at ~900
/ ~380, derived `Serialize` at 3,072 / 384 for TOML, derived `Drop`
between 32,768 and 65,536 / 8,192 and 16,384. Nesting depth is
caller-controlled in every supported language, and ~380,000 nested `fn`s
fit inside `bca-web`'s 4 MiB body cap.

**The stage that overflows first is rarely the one the symptom
implicates** (#1056). The issue was filed against `Serialize`, and its
own bisection supported that — `bca check`, which builds and drops the
identical tree without serializing, survived where `bca metrics -O json`
aborted. But the delegating `Serialize` first materialises the entire
wire projection, and *that* conversion was the overflow: ~2.3 KB per
frame in release against the JSON serializer's ~170 bytes. The same issue
wrote `Drop` off after a 10,000-level chain survived; a cheap frame is
not an iterative one, and it aborts at 16,000 in a debug build.

**`Serialize` is the one that cannot be fixed, only bounded** (#1056).
`serde` offers no iterative escape — `serialize_field` must run the
child's `Serialize` to completion before returning, so there is nowhere
to put a work stack. `serde_json`'s `Deserializer` solves this on the way
*in* with a 128-level recursion limit; `Serialize` has no equivalent, so
the crate supplies `wire::MAX_SPACE_SERIALIZE_DEPTH` and
`MAX_AST_SERIALIZE_DEPTH` and fails with an ordinary serializer error.
`Drop` *can* be fixed outright, by hoisting descendants into one flat
work list — at the cost of an `impl Drop`, which forbids moving fields
out by value (`E0509`) and is therefore a source-level SemVer break.
Landing it under a minor bump needed an explicit exception in
`STABILITY.md`.

---

## 82. When several predicates need an ancestor, propagate the chain, not a flag per predicate

**Lesson:** Before propagating a flag to kill an `O(depth)` ancestor
lookup, **count the predicates that want ancestry**. One, and a flag is
right. Several, and propagate the chain — then verify the one
equivalence that matters (a known chain answers exactly what climbing
answers) node-by-node against `Node::parent`, on a fixture per grammar
family that actually consults an ancestor. Give the chain type an
unknown/fallback variant so unconverted callers stay correct, and pin the
walker's own bookkeeping with a debug-only assertion: a chain type that
trusts `chain.last()` unvalidated turns a desynchronised walker into
wrong answers rather than a failure, and a parity test written against a
*replica* walker cannot see the real one drift.

`tree_sitter` stores no parent pointer — `Node::parent` restarts at the
root and descends — so any predicate asking a node for an ancestor is
`O(depth)` per node and quadratic over a deeply nested file. The standard
fix is to propagate state downward, since the walker visits parents
first; *what* you propagate decides how far the fix generalises.

**The same root cause was fixed three times before the general form
appeared** (#1052, #1062, #1084). #1052 (`tokens`' per-leaf ancestor
walk, filed as a DoS vector) and #1062 (`cognitive`'s
`get_nesting_from_map`) each propagated a derived flag, resolving those
two call sites and leaving the pattern intact everywhere else. The
benchmark harness from #1068 then measured three more walks as quadratic
— `Checker::is_else_if` (16 language impls consult an ancestor),
`Node::count_specific_ancestors` (reached from `loc` in 8 languages plus
four checker sites), and `elixir_is_inside_quote_block` — fitting
`time ~ depth^k` at 1.97, 1.95 and 2.01 against linear controls at
0.99–1.01 in the same run.

**A chain type with an "unknown" variant makes the blast radius a
choice** (#1084). `Ancestors` wraps either a known slice — where the
parent is `chain.last()` and each ancestor's own chain is the prefix
before it, so a predicate applied one level up stays as cheap as one
applied to the node — or `unknown()`, which climbs with `Node::parent`
for callers that reached a node some other way. Call sites that cannot
supply a chain stay correct at the old cost, so the 64-file change
stopped where its evidence did; the remaining climbs are enumerated
in #1088 rather than blocking the fix. The walker maintains the chain with
`truncate(depth)` on arrival and `push(node)` after the per-node
computes, correct for a LIFO pre-order because every node popped between
a parent and its child sits at a strictly greater depth.

---

## 83. A categorical proxy for a positional property is wrong in both directions

**Lesson:** Ask what property the computation actually depends on and
read *that*, even when a category is already in hand and agrees on every
input you have. When replacing a proxy, check **both directions** before
sizing the fix: a one-line `debug_assert` probe in the shared path, run
over the whole suite (`cargo test --workspace 2>&1 | rg -o "PROBE \w+"`),
enumerates which grammars really reach a supposedly-unreachable branch.
Treat "this per-part value exceeds the whole" as a hard invariant worth
asserting outright.

Both failures are silent, because the value stays a plausible number,
and they have **opposite sign** — so an aggregate over mixed input can
look untroubled while individual entries are wrong.

**`Sloc` keyed its row count on `is_unit` instead of the span's end
column** (#1067). The unit branch computed `end - start` and every other
span `end - start + 1`; both are proxies for "does the end position sit
at column 0, so the final row contributes no characters?". The unit
branch was correct only because a trailing newline pushes tree-sitter's
root onto a phantom extra row — so source stopping mid-line lost its last
row, a one-line unterminated file reported `sloc == 0`,
`mi::inputs_are_empty` short-circuited all three MI formulas to `0.0` for
a real file, and `cloc + ploc > sloc` for input as ordinary as
`b"fn f(){}\n/// x"`. The other direction was already in the repository
and had never been questioned: tree-sitter-perl's `function_definition`
swallows the newline after the closing brace of a file's last `sub`, so
the unconditional `+ 1` credited a row that sub does not occupy. **Two
accepted snapshots recorded a per-function `sloc` larger than the whole
file's** — an impossible value, checked in and passing.
tree-sitter-bash reaches the same shape.

The input class was unreachable from every in-tree harness, which is why
it survived — see the normalisation map in
[`testing.md`](../../.claude/rules/testing.md). The documented rustdoc
example at `src/spaces.rs` was itself demonstrating the bug in the
published docs.

**The same proxy, one file over** (#1163): `FuncSpace::new` and
`Ops::new` branched the end-row `+ 1` on `SpaceKind::Unit`, which is a
statement about *nodes ending at column 0* — true of the root, and also
true of a Perl `sub` that is the last item in its file. That sub
reported a span ending a line past its own parent unit and past EOF, the
class of past-EOF span arithmetic behind the release `usize` underflow
in #1051. Keying on the end column deletes the branch outright.

---

## 84. A factual claim in prose is untested code

**Lesson:** Treat any sentence stating a checkable fact about the code
as an assertion you owe evidence for, and get it the same way you would
for a test: read the parameter type, run the measurement both ways,
enumerate the call sites with `rg`, verify the panic actually panics.
Each of the eight below took under five minutes to settle. Be most
suspicious of prose in a change that is *itself* fixing a wrong claim,
and of counts ("four call sites", "13 languages"), which are correct
when written and rot silently. When a claim is expensive to verify or
cannot be pinned, write what was measured and under which conditions
rather than the generalisation it suggests. Be most suspicious of all of
a comment saying a fix is *already in place*: it ends the search that
would have found the gap.

No gate checks any of it. `cargo test` does not read prose, clippy does
not evaluate it, and a reviewer's eye slides over a plausible sentence —
especially a *rationale*, where the reader checks that the guard exists
rather than that its stated reason is possible.

**Eight wrong claims in one batch** (#1059, #1066, #1067, #1084).
`node_text`'s safety comment described a UTF-8 char-boundary panic that
cannot occur for its `&[u8]` parameter — byte slicing has no such
precondition — and cited hazard paths this crate does not have. The fix
for that issue shipped two *new* false claims of the same kind: that
`get_func_space_name` returns `None` for a nameless node (it returns
`Some("<anonymous>")`), and that `node_text` is reached only through that
one default (26 call sites across 10 language modules). `bca.toml`'s
`exclude_tests` comment stated the option does not lower `loc.sloc`;
measurement gave 779 with the manifest against 845 with `--no-config`,
and #722 had made it do exactly that. A test's exclusion rationale
claimed the no-op `Loc` grammars have every LOC sub-metric at 0 — `sloc`
is span-derived, so they drift like everything else. A changelog note
said the Python bindings were unaffected, but `PyAst::parse` passes bytes
to `Source::from_bytes` verbatim while only `analyze_source` calls
`normalize_eol`, and it described the per-function drift as Perl-only
when Bash shares the shape. A benchmarking doc said four `Node::parent`
climbs remained; the real figure was higher.

**The cost is a wrong decision, not a wrong sentence** (#1066). The
`bca.toml` comment was load-bearing: the file it annotates carries the
`loc.sloc` cap that gates the build, and anyone budgeting a file against
that cap from the comment would get it wrong.

**A claim about a sibling language held a wrong number in place**
(#1278). A Go ABC test asserted that an initialized `var` declaration is
not counted, "matching the Rust/Java rule for `let` / `int y = 1`".
Measured: Rust and Java each score one assignment for exactly that form,
so the rationale was backwards — and load-bearing, because the expected
value it justified was wrong too and had been for as long as the comment
stood. Prose explaining *why* a number is what it is has to be checked
together with the number, not after it.

**A comment asserting the fix existed hid a years-old gap** (#1316).
Three alterator arms had said since #699 that `CharLiteral` is an
operand, flattened, and deliberately absent from `Checker::is_string`,
and Go's own rune-literal test cited that C++ behaviour as settled
precedent. The operand half had never been implemented: a C, C++, Mozcpp
or Objective-C character literal contributed nothing to Halstead at all.
Anyone auditing the C-family operand set read the comment as
confirmation and moved on. The fix pinned the half that *was* true
(`c_family_char_literal_is_not_a_string`, two-sided per language) so it
fails loudly if it stops being true, and made the other half true.

---

## 85. Coverage measures execution, not discrimination

**Lesson:** Never accept a coverage percentage as evidence a line is
tested — it is evidence the line is *reached*. The full procedure, and
the covered-count-not-percentage rule, are in
[`testing.md`](../../.claude/rules/testing.md). The general shape is that
**any scalar summary of a set** — coverage percent, test count, file
count, snapshot count — can hold steady or improve while the set
underneath it changes; compare the sets themselves whenever the
comparison is the point.

**A 100%-covered argument that no test ever varied** (#1105, PR #1128).
`CommaIndex::splits` (`src/cfg_predicate.rs`) looks up each region's
splitting commas with
`partition_point(|entry| *entry < (depth, region.start))`. Replace that
`region.start` bound with `0` and the function panics on ordinary input
such as `any(all(unix, test), all(windows, foo))`, slicing with an
inverted range — yet before this PR added rows covering that shape, the
perturbation failed **no test at all**: 3,969 tests, every one still
green with the bound wrong. `splits` measured at 11 of 11 regions
covered, entered 150,200 times in a single run. llvm-cov was not
malfunctioning; it was answering the question it is asked. No test had
two sibling `all(...)` regions at the same depth where the *earlier* one
held a comma, so the bound was never the difference between right and
wrong.

**The file-level percentage was not measuring one copy of the code**
(PR #1128). `cfg_predicate.rs` read 73.26% region coverage — apparently poor,
and flatly inconsistent with the 100% on the function under review. Both
numbers were right: the coverage data holds **five** instantiations of
`big-code-analysis`, one per workspace crate that links it, and only one
ever executes — 25 function entries with a non-zero count against 44 at
exactly zero, inflating every denominator. The same artifact made
`spaces.rs` appear to lose coverage while its *covered* count stayed
byte-identical, and dragged the workspace figure from 86.01% to 85.99%
in a change that covered 97 more regions than it started with.

**A test *count* standing in for the test set** (#1120, PR #1128).
Switching `make test` to `cargo-nextest` had to preserve coverage, and
two listings of the *same tree* gave 4,741 and 4,731 — which reads as ten
tests dropped until you notice one listing includes `#[ignore]`d entries
and the other omits them. Diffing the executed-test set, generated the
same way on both sides, showed zero removed.

**The same shape in a different subsystem** (#1054, PR #1092). The dump
walk's `prefix.truncate` calls each restore one nesting level's rail
after a subtree renders. `cyclomatic.modified` is the only nested metric
object, so `dump_object`'s per-field truncate is the only thing keeping
the fields *following* a nested object on the right rail; removing it
mis-indents `cyclomatic`'s trailing `sum` / `value` in every dump — a
visible output defect, with all 2,869 lib tests still passing. Found by
perturbing each of the six `truncate` lines in turn rather than by
consulting a report.

**The metric a test is *named* for is the one it pins** (#1135, #1137).
Every `Loc` implementation ends its match with a catch-all inserting the
row into PLOC, so a token the author never considered silently becomes a
line of code. Tcl and iRules routed their row terminator through it and
Perl routed the `#` inside its `comments` node through it: a realistic
fourteen-row Tcl file reported `ploc 13` against a true `7`. The guards
that should have caught this are the `*_cloc` tests — and several,
`irules_cloc` among them, assert `cloc` and `blank` and leave `ploc`
unasserted. The catch-all's own metric was precisely the one no test
named, while `src/metrics/loc/tcl.rs` measured 95% line coverage carrying
the bug. The repair is a sweep
(`a_comment_row_is_never_counted_as_code`) asserting *all four* LOC
sub-metrics for every language and comment spelling; it found the Perl
case immediately after being written for the Tcl one.

---

## 86. A test helper that normalizes the value under test blinds every caller at once

**Lesson:** In a test helper, normalise the **expectation**, never the
**observation** — the rule and the removal check are in
[`testing.md`](../../.claude/rules/testing.md). Where a helper already
normalises, remove the normalisation and count the failures: none means
the dimension is untested, and a large number means you just recovered a
guard across the whole caller list for free.

Nothing in any individual test looks wrong. The assertion is still
specific, still compares real data, still fails on a real regression in
the dimensions that survive. The dimension normalised away simply stops
being tested anywhere, and the larger the caller list the more complete
the blindness.

**`check_ops` sorted both sides, so eighteen per-language tests could not
see a nondeterministic vocabulary** (#1091, PR #1093). The helper was
introduced sorting `operators_str` and `operands_str` — the values
returned by the code under test — alongside the expectations. `Ops`
vocabularies were built from `HashMap` keys, and Rust's `RandomState`
reseeds per map instance, so their order differed on every run and even
between two parses in one process. The consequence reached users: `bca
ops` printed byte-different output for an unchanged input on consecutive
invocations, with the connector glyphs moving so a different entry got
the closing `` `- `` each time. Every one of the eighteen callers parsed
real source and compared real vocabularies, and not one could observe it.

Note the order was *documented* as arbitrary, so the helper's sort
accommodated a sanctioned non-guarantee rather than hiding a stated
contract — which is exactly why it survived: nothing was being violated,
so nothing complained, and the property could never become a contract by
accident. The fix sorts in production and removes only the actual-side
sort. Reverting the production `sort_unstable` calls fails 20 of the 27
tests under `ops` and `output::dump_ops`.

---

## 87. An assertion can be correct and still be about the wrong rows

**Lesson:** Review the selector as carefully as the assertion, and treat
"how many rows did this match" as part of the test — see
[`testing.md`](../../.claude/rules/testing.md) for the assertion shapes.
When a test's name states a property, check that the rows it selected are
*capable of violating* that property; if they are not, the test is
decoration regardless of how specific its assertion looks.

Hierarchical text makes this sharper than it sounds: indentation-based
output has rows that are prefixes and suffixes of each other, so an
off-by-one in a column count, or a `contains` where a whole-line
comparison was needed, lands on a real line and asserts something true
about it.

**A connector test that asserted about the `metrics` header instead of
the metric groups** (#1054, PR #1092).
`last_emitted_metric_group_uses_closing_connector` selected group lines
with `line.starts_with("   |- ")` — three columns — while metric groups
sit six columns in: three for the root space's own connector, three more
for the `metrics` line's. The filter matched exactly one line, the
`metrics` header itself, and the assertion that the last match uses the
closing `` `- `` was a true statement about that header. The test passed
with **every metric group rendering a dangling `|-`**, the precise defect
its name claims to prevent. The repair filters at six columns, asserts
more than one group was found so the filter cannot silently empty, and
checks every non-final group.

**Substring matching cannot work on rail-indented output**
(#1054, PR #1092). A deeper rail *ends with* the shallower one, and labels like
`sum` and `value` recur across several groups, so a substring search
accepts exactly the mis-indentation under test. Whole-line sequence
comparison is the only form that discriminates.

---

## 88. A text scan that does not lex the language measures noise

**Lesson:** A tool that reads source as text to produce a number must
model the language's lexical structure — strings, raw strings, comments,
**and char literals** — before it matches anything. Without that it does
not report a wrong number occasionally; it reports a confident, uniform,
plausible one every run, which is then quoted as fact. Give the scanner
its own tests, and pin **both** directions: the under-lex that misses
constructs and the over-lex that swallows them. **Do not try to buy the
distinction back with a sharper pattern.** Whether a quote opens or
closes a literal is *state*, and a regex sees only the characters around
it, so no lookbehind can separate the two cases.

The remedy for a wrong claim is normally "run the measurement" (lesson
84). That does not help here, because the measurement *was* run. When
the instrument is wrong, re-running it reproduces the same wrong answer,
and re-running it is exactly what a careful person does before quoting
it.

**A `=>` regex over raw Rust counted other languages' lambdas** (#1136).
The probe in `.claude/rules/formatting.md` matched match-arm headers by
regex, so every JS, C# and TypeScript fixture embedded in a Rust string
literal registered as a match arm. `src/metrics/nom.rs` reports **16**
arm lines to the raw regex and **0** once string spans are excluded; it
has no bailing arms at all. The over-counting script was committed to
the rule file, and the figures it produced were handed to the agent
implementing the fix as its starting scope — which is how a measurement
error becomes a plan to edit files that were never affected.

**The same scanner then hid what it was built to find.** Adding string
and comment spans left char literals unlexed, so a `b'"'` read as an
unpaired double quote and opened a span running to the next `"` anywhere
later in the file. `src/vcs/git/diff_parse.rs` probed 4 of its 15 arms.
A gate whose entire purpose is to catch regions that *read as clean*
had, in its own implementation, a region that read as clean. Nothing
currently bailing was hidden, so no count moved — the blindness was to
future entries, which is the only thing a ratchet exists to catch.

**The sibling gate has the identical gap** (#1192).
`utils/check-snapshot-anchors.py` classifies string and comment spans
and not char literals, so a `b'"'` makes every following
`insta::assert_json_snapshot!` invisible to it. Latent today only
because no file under `src/metrics/` spells one. Lifting the fix's
`char_literal_end()` — which must return `None` for a lifetime, since
`'a` has no closing quote and treating it as a literal swallows the file
the other way — closes both.

**The third gate in the family, and the proof no pattern would have
done** (#1219, PR #1221). `utils/check-diagnostic-prefix.py` matched
raw-string opens with a regex and skipped to the next quote, so three
shapes opened a span that hid every severity literal until it closed:
`let p = "dir/r";`, where the closing quote of an ordinary string
follows an `r`; and an unterminated `r"` inside a trailing `//` or a
`/* … */` comment, neither of which a scanner that only skips
*whole-line* comments can see. The first is the one that settles the
approach — `/` is a legitimate raw-open context, so the character before
`r` carries no information; what differs is that the `"` **closes** a
literal. Stripping comments first fails the same way, because
`"http://x"` truncates mid-literal and opens a phantom span of its own.
Finding where a comment starts already requires knowing whether you are
inside a string, which is the lexer. All three were latent: replaying
both scanners over all 559 tracked Rust files gave identical output.

**Porting a lexer without porting the shape that makes its tests
discriminate** (#1219). The port's lifetime fixture used an *even* number
of lifetimes, so a greedy `char_literal_end` pairs them off, every bogus
span closes before the offender, and the test passes against the exact
bug it names. The donor's suite had recorded that trap — "Three
lifetimes, not two, and a real char literal after the call" — and the
copy dropped it. When you lift a scanner, lift its fixtures' arithmetic,
not just its assertions.

---

## 89. A positive enumeration and a negative filter differ on what neither names

**Lesson:** Replacing a bespoke `matches!(A | B)` with a shared `!is_x
&& !is_y` predicate does not just move the rule — it changes the set.
The two agree on every kind either one names and disagree on everything
else, so the inputs that move are exactly the ones no one enumerated and
no test covers. Before consolidating, enumerate the node kinds the
container can actually hold, and decide the leftovers deliberately:
adopting the shared answer is usually right, but it is a decision, not a
refactor. The same choice exists when you author the filter rather than
consolidate one: gating a token that serves several grammatical roles
*positively* — require the one parent that means what you are counting —
is closed and fails safe, while denying the roles you thought of is open
and admits the ones you did not. (cf. lesson 59 for why you are
consolidating, and lesson 65 for the structural inverse.) When a token's
roles force opposite polarities in sibling languages, pin each direction
with a test whose only job is to fail if a later "make these consistent"
pass flips it.

The direction of the change is what hides it. A positive filter is
closed — a grammar that starts emitting a new kind silently scores zero,
which is lesson 19. A negative filter is open, so the same grammar
change silently scores *one*, and neither shows up as a diff in the
snapshot suite unless a fixture happens to hold the construct. Both
forms read as "the obvious thing" at the call site.

**The Objective-C block arm inherited three exclusions and two
inclusions** (#1218, PR #1221). The `Objc::BlockLiteral` `nargs` arm
counted `matches!(ParameterDeclaration | VariadicParameter)`, which
bypassed the shared `count_args` and therefore
`Checker::is_empty_param_marker`, so `^(void){ … }` reported one
parameter where zero belongs. Routing it through `count_args` fixed
that and put the comment and punctuation rules on the shared footing
too — those had *happened* to be right, since a `comment` child is not
a `ParameterDeclaration` either, but for a reason no test asserted. It
also changed two shapes nobody had considered: on invalid source
`^(int a,,)` went 1 → 2 as an `ERROR` child began counting, and
`^({ int x; })` went 0 → 1 for a `compound_statement` child. Both were
kept, because both are what the function channel already reported
through the same helper — `int f(int a,,)` gives 2 and
`int g({ int x; })` gives 1 — so the block arm had been the one caller
answering differently. Recording that in the arm's comment is what
stops the next reader from filing it as a regression.

**One token, several grammatical roles — a denylist admits the role
nobody listed** (#1274, #1280). A bare `<` is a comparison, and in Java
and Groovy it also delimits generic type arguments and type
*parameters*. The gate denied `type_arguments` only, so every generic
*declaration* — `class Gen<T>`, `<T> void m()` — scored two phantom ABC
conditions, and Groovy has a third form, `method_type_parameters`, that
a denylist must name separately. Both were rewritten to require a
`binary_expression` parent. Ruby then arrived with two roles nobody
would have enumerated in advance — a superclass clause
(`class Foo < Bar`) and an operator-method name (`def <(other)`) — and
the positive gate covered both without being told about either. **C#,
Kotlin and the JS family still carry the denylist form**, so they are
correct only for the roles someone thought of: the state Java and Groovy
were in before #1274.

**The same token needed opposite polarities eight lines apart** (#1275,
following #1274). A `?` is the ternary operator and, in C#, also
nullable-type syntax (`int? x`) and a constraint (`where T : class?`);
in TypeScript it marks optional parameters, properties, methods, tuple
elements and conditional types. TS/TSX got the positive gate — one
`ternary_expression` parent against eleven type-syntax producers. C#
could not: safe navigation `a?.b` / `a?[0]` emits the *same bare `?`*
under `conditional_access_expression`, and counting it is deliberate (it
keeps ABC in step with C# cyclomatic), so an allowlist on
`conditional_expression` would have silently stopped counting it. C#
denies `nullable_type` and `type_parameter_constraint` instead — the
second found only by enumerating `grammar.json`; the issue's own list
would have shipped it. One test exists to fail under both an allowlist
flip and an over-suppression; nothing else in the suite noticed either.

---

## 90. Re-reading a single-consumption source yields empty, not an error

**Lesson:** Before re-deriving a value from its source instead of
threading it through, check whether the source can be read twice. A
consumable source — stdin, a drained iterator, a channel — answers a
second read with *nothing*, not with an error, so the downstream
consumer sees a plausible empty collection and every result built on it
is silently wrong. Thread the materialized value out of the stage that
consumed the source; and if a comment defends the re-read as cheaper,
answer that comment in the replacement, or the re-read comes back.

The failure is invisible because both reads succeed. The first consumer
drains the source as part of its normal work; the second read's empty
result is indistinguishable from "the user provided nothing", and code
that handles an empty list gracefully — usually a virtue — converts the
bug into a clean wrong answer with a zero exit status. No test that
supplies the input as a *file* can catch it, because files rewind for
free; only the consumable spelling of the same option does.

**`bca check --paths-from -` silently defeated `[check.exclude]`**
(#1306). `apply_check_exclude` re-read `--paths-from` to re-anchor the
exclude globs — a re-read the code defended in a comment as keeping the
hot path allocation-free. `-` resolves to stdin, which the walk had
already drained, so the second read returned an empty seed list, every
violation anchored against the bare `--paths` set, and a
`git diff --name-only | bca check --paths-from -` gate failed on files
the project had exempted. The fix materializes the list once, before
the walk, and carries it out in `CheckWalk::seeds`; the pre-existing
regression test for #497 passed throughout, because it spelled the
seed list as a file.

---

## 91. A gate can filter out its own subject before the check runs

**Lesson:** When you add a dependency or supply-chain gate, prove it
fails by reintroducing the condition it guards and watching it go red.
A tool that builds a filtered view of its input before applying your
rules can drop the very artifact a rule names, leaving an entry that is
syntactically valid, semantically correct, and permanently silent. When
the tool's view is derived, guard the raw artifact instead — the
lockfile, the manifest — and record at the site where the dead entry
would go why it is not there.

A config entry reads as coverage. A future maintainer greps `deny.toml`,
finds the crate and the version range spelled out, and stops looking;
the tool exits 0, which is the same output it gives when the tree is
genuinely clean. Nothing distinguishes "no violation" from "the subject
was never in scope", and the gap opens precisely when the guard matters,
because the condition that reintroduces the vulnerable crate is also the
condition that puts it back beyond the filter.

**cargo-deny cannot guard `h2 0.3.27`** (RUSTSEC-2026-0258, Scorecard
alert #776; `3df3b2c3`, `b60943b5` — direct pushes to `main`, no PR).
The crate reached the tree only through `actix-web`'s default `http2`
feature, and no patched 0.3 release exists, so the fix was to drop the
feature: `bca-web` binds plaintext and actix negotiates HTTP/2 only over
TLS ALPN. The obvious belt-and-braces guard, a `[bans] deny` on
`h2 <0.4.16`, turned out to be dead by construction — krates, cargo-deny's
graph builder, filters the crate out before the advisory and ban checks
run (`-L debug` logs `filtered h2 0.3.27`), on 0.19 and on the 0.20 that
CI's action bundles. Re-enabling `actix-web/http2` under both versions
left cargo-deny silent. The working guard reads `Cargo.lock` directly
from a test, and `deny.toml` carries a comment where the entry would
have been, naming the reproduction so the next reader does not add one.

---

## 92. An optimization's rationale can encode the waste it optimizes for

**Lesson:** When a collection is pre-sized to its input, ask whether it
needs to reach that size before optimizing for it — a reserve is
evidence that somebody measured the final size, never evidence the
growth was necessary. And attribute a resource peak by bisecting the
run: one metric at a time, one output destination at a time, one worker
count at a time. Reading the code ranks hypotheses it cannot
discriminate, because every allocation you suspect really is there.

A leak stated as a steady state stops looking like a leak. The sentence
justifying the reserve asserts the growth as a fact, so review checks
the arithmetic — is `descendant_count` the right size? — rather than the
premise, and the premise is the bug. Nothing else flags it either: the
retained entries are dead, so no metric value, snapshot, or exit status
moves whether they are freed or kept.

**The nesting map's reserve was sized to the leak it should have
prevented** (#1069, fixed in #1375 / PR #1377). `NestingMap` carries one
`Nesting` per node id, seeded by the parent, read once by the node's own
`Cognitive::compute`, read once more when its children are seeded — and
then never removed. #1069 measured that it "converges on one entry per
visited node", and reserved `descendant_count()` up front to skip the
doubling chain, which is a real speedup for a growth that should not
have existed. On a 28 MB generated tree-sitter `parser.c` that was
540 MB of a 1,265 MB peak. Freeing each slot in its last reader leaves
the live set bounded by the traversal stack: 735 MB, and 3.3 s against
5.1 s.

**All three hypotheses in the issue were wrong** (#1375). It proposed
the reorder buffer, a path list materialized twice, and glibc arena
retention. Selection settled it in minutes where code reading had not:
`dump` (parse only) measured 722 MB and every non-cognitive metric
723–725 MB against cognitive's 1,264 MB, `--output-dir` matched stdout
byte-for-byte in peak — ruling out both the wire clone and the reorder
buffer — and `bca check`, which has no reorder buffer at all, still
peaked at 4.5 GB on the same tree.

---
