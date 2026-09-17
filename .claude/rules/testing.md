# Testing Rules

Project-specific testing practices for big-code-analysis. These
supplement, not replace, the test-quality lessons in
[`docs/development/lessons_learned.md`](../../docs/development/lessons_learned.md)
(particularly #6, #7, #23, #31).

## Verify defensive refactor tests by reverting the production code

When you add a regression test for a *defensive* refactor — one that
fixes no current bug but pins an invariant against future drift
(e.g., #306's `apply_suppression` Unit-kind predicate) — the test
must demonstrably fail against the pre-refactor code. A test that
passes under both the old and new implementations protects nothing.

**Verification procedure:**

1. Stash or note the production change you just made.
2. `git checkout HEAD~1 -- <file>` (or manually revert the specific
   lines).
3. Run the new test(s). Confirm they fail with the assertion message
   you expected.
4. Restore the production change.
5. Re-run the tests. Confirm they pass.

Two minutes of test-via-revert gives higher confidence than a
passing test alone — it proves the test exercises the changed line,
not an unrelated path. Without this step, defensive refactors often
ship with tests that would pass against the bug they claim to guard.

**Revert safely — a file-level restore discards your fix too.** When
the file you are perturbing also carries your *uncommitted* production
fix, `git checkout HEAD~1 -- <file>` (or any `git checkout -- <file>`)
restores the whole file and wipes the fix along with the perturbation.
This destroyed in-progress work twice in one batch (during the #605
and #615 fix sessions — commits `b8ceb8cf`, `e02ed585`; the incidents
live in the session record, not the issues); in one case the follow-up
build "passed" only because an orphaned-import warning masked the
regression. Step 1 above is therefore load-bearing,
not optional. Concretely, before perturbing a file with uncommitted
work, do one of:

- commit or `git stash push <file>` the real change first;
- apply the perturbation as a precise edit and undo it with the exact
  inverse edit — never a file-level restore;
- when a partial revert would not build, patch the production line to
  a no-op in place instead (the approach used in the #615 fix session).

**A sweep of several perturbations needs a script, not a shell loop.**
The inverse-edit advice above does not scale past one or two: a loop
that perturbs, tests, and restores is where `git checkout -- <file>`
gets reached for, and it destroyed an entire uncommitted rewrite a third
time during #1219. Read the file once into a variable, write each
perturbation from that string, and restore from it in a `finally` — the
backup then lives in memory and cannot be defeated by the working tree
changing underneath. Put the driver in a file rather than nesting quotes
through zsh into `python3 -c`, which is how those edits came to be
applied inconsistently in the first place.

**A broken harness reports a plausible failure count, not an error.**
In that same sweep the restore silently reverted the gate to its `main`
version, so every subsequent perturbation ran the *new* test suite
against the *old* implementation and reported an identical 34 — six
failures and twenty-eight errors, reproducible today by checking out
either side alone. Three perturbations in a row returning the same
number reads as "well covered"; it meant the file under test was no
longer the file being edited. Before believing any perturbation result,
confirm the subject still contains the change: `rg -c <new symbol>` on
the file, or a `git diff --stat` that shows what you expect.

**For a compiled subject the rebuild must be inside the measurement
step.** `rg` on the source proves the *source* changed; a sweep that
shells out to `target/release/bca` is measuring the last binary someone
built. During #1412 a corpus sweep reported "0 of 438 files changed" —
a perfectly plausible result — because the restore ran without a
rebuild, so the before and after runs used the same stale binary. Put
`cargo build` in the same step as the run, and assert a known fixture
whose answer differs between the two states immediately after each
build; that guard is what caught it on the rewrite.

**The result *parser* is the other half of the subject.** During #1238 a
sweep drove three perturbations of one match arm and reported zero Rust
failures for all three, while the Python leg of the same sweep reported
the expected four, five and eight. Nothing was stale — the driver ran
`cargo test -q`, which prints dots rather than per-test lines, so a
parser scanning for `... FAILED` matched nothing and every mutation read
as "no test noticed". A uniform zero across perturbations is the same
tell as a uniform 34: it describes the harness, not the code. Cross-check
any parsed count against the process exit status and treat a
disagreement in *either* direction as a harness bug, not a result.

**So is the invocation.** `cargo test` stops at the first failing
*target*, so a sweep that names a unit target and an integration target
in one invocation never reaches the second once the first fails — every
perturbation then reports the integration guard as passing, which reads
as a dead guard rather than a harness artifact (#1270). Pass
`--no-fail-fast` in any multi-target perturbation run.

**Classify a build failure by `error[` or `could not compile`, never by
`error`.** `cargo test` prints `error: test failed` to stderr on an
ordinary red run, so a driver matching bare `error` reads every
discriminating perturbation as a build break. One sweep returned
BUILD-ERROR for six of seven cases that were in fact all working (#1466)
— a uniform non-answer of exactly the shape the uniform-34 and uniform-0
tells above describe.

**A perturbation that *passes* is a finding, not a null result.** It says
the assertion does not depend on the line you neutralised, and the usual
cause is that the test's stated claim is wrong rather than that the test
is weak. Check whether the behaviour is *enforced by the mechanism* or
merely *absent from a list* before strengthening anything: Perl's
statement-modifier walker keys on the grammar's `condition` field, so
adding `ForSimpleStatement` to its arm changes nothing — the `for`
modifier exposes no such field — and a comment claiming the test would
catch a widened arm was false until the sweep proved it (#1464). Correct
the comment; do not invent a fixture to justify it. The inverse error is
equally common: two candidate rankings in #1465 were merely *different
valid orderings* rather than defects, and the honest fix was to trim the
test comment's claim.

After restoring, `git status` / `git diff --stat` must show exactly
the edits you intend — nothing extra, nothing missing.

Applies in particular to:

- `apply_suppression`-style "make implicit invariant explicit" fixes
- Any `matches!()` predicate tightening
- Any newly-explicit `kind` check that replaces a position-based
  index
- Test-quality follow-ups that add `check_func_space` / structural
  assertions to previously-vacuous tests (see lesson #31)

If you cannot test-via-revert because reverting the production
change does not produce a buildable tree (e.g., the change deleted a
helper the new test depends on), construct a synthetic input the
test exercises directly — do not assume the test is correct just
because it passes against the fixed code.

## When the refactor is compile-time-only, perturb instead of reverting

A newtype or signature refactor that exists purely to make a
transposition unrepresentable cannot be tested by reverting: undo the
signature and the new test no longer compiles, so there is nothing to
observe failing. Perturb the production line instead — transpose the
two fields the newtype exists to protect — and confirm the new test is
the **only** failure.

That last clause is the point, not a formality. During #1070, a
transposed `function_depth` / `lambda` inside
`python_comprehension_clause_nesting` left **every cognitive test but
the new one passing** — the whole lib suite reported exactly one
failure — because every consumer folds the three `Nesting` fields into
one sum (`conditional + function_depth + lambda`) and none of the
per-field increments branches on a value. No metric moves under *any*
transposition of those fields. So the issue's framing — that a swap
"would be a straightforwardly wrong metric on every Python
comprehension" — was too strong, and a test asserting a cognitive score
would have proved nothing. Field-level assertions on the map slot were
the only coverage such a refactor can have.

Two things follow:

- Run the perturbation against the **whole** suite, not just the new
  test. A perturbation that fails hundreds of tests is too coarse to
  isolate anything, and one that fails none means the invariant is
  currently unobservable — which is worth knowing before you claim the
  refactor prevents a bug.
- If the perturbation is unobservable in output, say so in the test's
  comment. "Correct-by-construction is the only available defense here"
  is a stronger justification for a newtype than an overstated bug
  claim, and it stops the next reader from deleting the test as
  redundant with the metric assertions.

Reconstructing a *removed* branch faithfully is often not worth it.
In #1067 the pre-fix rule needed an `is_unit` parameter the fix had
deleted from 23 language modules; restoring it to run one perturbation
would have been a larger edit than the fix. Where that happens, fall
back to establishing the old behaviour by inspection (the removed
arithmetic, plus the issue's own reproducer) and say plainly that the
evidence is inspection rather than a perturbation run.

See lesson #82 for the related walker case, where the production
bookkeeping and the test's replica of it are two different things and
only a debug assertion in the real walker covers the former.

## Seed the state you claim to assert on

An assertion that a function *resets* or *accumulates* something proves
nothing when the fixture starts from the default-constructed value.
`Foo::default()` is usually all zeroes and `None`s, which is exactly the
state a reset produces and exactly the value `+=` and `=` agree on — so
the assertion holds whether or not the line under test exists.

This is not a hypothetical failure mode. Both instances below shipped in
the *same* test during #1086, and both were measured, not guessed:

- **A reset asserted from a default fixture** (#1086). The test built
  `Stats::default()`, called `increase_nesting`, and asserted
  `stats.boolean_seq == BoolSequence::default()`. Deleting
  `stats.boolean_seq.reset()` from the production helper failed
  **zero** tests across the whole 3,130-test lib suite — the line was
  entirely uncovered, and the new test that claimed to cover it did not.
  Seeding `boolean_op = Some((1, 0))` first made it the only failure.
- **An accumulation asserted from zero** (#1086). The same test asserted
  `stats.structural == 8` from a zeroed `structural`, where
  `increment`'s `stats.structural += stats.nesting + 1` is
  indistinguishable from a plain `=`. Seeding `structural: 5` and
  asserting `13` made the `+=`-to-`=` perturbation fail.

The tell is that the expected value equals the default. When you write
`assert_eq!(x.field, 0)` or compare against `Default::default()` after
calling something that is *supposed* to zero it, stop: either seed a
distinguishable value first, or accept that the assertion is decoration.

**Lesson:** pick fixture values that differ from both the default and
each other, so the assertion can only pass for the intended reason. Then
confirm it by perturbing the exact production line the assertion names —
per the sections above, a test that cannot fail is worse than no test,
because it reads as coverage.

## Perturb the fixture as well as the production line

A revert test proves the construct reaches the arm *today*. It says
nothing about whether the fixture still contains the construct tomorrow.
A test that pairs an excluded construct with a genuine condition and
asserts the non-zero total keeps passing after someone trims the
construct out of the fixture, because the surviving comparison supplies
the total on its own — and every #1297 test had exactly that shape, so
each could decay into an assertion about the comparison alone with no
signal.

Anchor the fixture on a second axis that only the construct contributes,
then verify by deleting the construct from the fixture and watching
exactly that test fail:

- The three JSX fixtures anchor on `assignments_sum()`: the
  `className="x"` attribute `=` is each fixture's only assignment.
- The C# operator overloads are asserted per space rather than through
  the file total, which is 3 with the overloads and without them.
- The Perl readlines are three of the sub's four assignments.

Where the construct contributes to no axis once excluded — a Lua
`<const>` attribute, a TypeScript type argument — there is nothing to
anchor on, and the revert test is the only coverage available. Say so in
a comment, so the missing anchor is not read as an oversight.

### Never let the measured value *be* the defect's output

A fixture whose discriminating quantity is *produced by the bug* stops
measuring anything the moment the bug is fixed. It is the inverse of the
decay above, it bites benchmark probes hardest, and it fails as "the
workload scored zero on its own shape" — which reads like a broken
fixture rather than like the fix working.

`loc/wide-cfg-test-mod` (`big-code-analysis-bench/src/shapes.rs`) read
`sloc` under `exclude_tests` on a file of nothing but `#[cfg(test)]
mod m {}` repeated. Its only non-zero row was the phantom attribute row
that #1431 then removed, so the probe scored zero and tripped
`probe_workload_is_exercised`. Left unnoticed it would have timed the
walk's fixed overhead and reported an excellent exponent forever. The
repair was to render a retained `fn p() {}` per item, so the reading
survives the fix.

When a change alters what a shape measures, re-check every probe or
fixture reading *that* metric on *that* shape before comparing a
before/after result. Build the measured quantity out of something the
fix does not touch — for a metric fix, usually retained content
alongside the construct under test.

## Coverage measures execution, not discrimination

A coverage report answers "did any test run this line?" It never answers
"would any test notice if this line were wrong?" Those come apart
whenever many tests reach a line while all supplying the same value to
the part that matters, and the tool cannot see the difference because
which-inputs-varied is not what it measures.

`CommaIndex::splits` (`big-code-analysis-ast/src/cfg_predicate.rs`) measured 11 of 11 regions
covered and was entered 150,200 times in one run. Replacing its
`region.start` lower bound with `0` panics on ordinary input — and
before #1105 that perturbation failed **none** of the 3,969 tests then
in the lib targets.

- Never accept a percentage as evidence a line is guarded. Perturb it.
- Perturb the **sub-expression carrying the invariant** — a bound, an
  offset, a comparison direction — not the whole statement, which
  usually fails loudly for the wrong reason.
- Treat "the perturbation failed nothing" as a finding to act on.
- When the question is "did this change lose coverage", compare the
  **covered count** (`count - missed`) per file and in total, never the
  percentage: the denominator moves for reasons unrelated to the change.
  This workspace links `big-code-analysis` into five crates, so a file's
  report aggregates instantiations that never execute.
- Any scalar summary of a set — coverage percent, test count, snapshot
  count — can hold steady while the set changes. Diff the sets when the
  comparison is the point. Two `cargo-nextest` listings of the same tree
  gave 4,741 and 4,731 purely because one counts `#[ignore]`d entries.

## Normalise the expectation, never the observation

Shared test support earns its terseness by canonicalising — sort the
collection, trim the whitespace, round the float. Applied to the
*expected* value that is fine. Applied to the value returned by the code
under test it silently stops testing a property, for every caller at
once, and no individual test looks wrong.

`check_ops` sorted `operators_str` / `operands_str` — the observed
values — so eighteen per-language callers could not see that `Ops`
vocabularies came back in `HashMap` order and `bca ops` printed
byte-different output for an unchanged input on consecutive runs
(#1091). The fix sorts in production and drops only the actual-side
sort, turning those same eighteen callers into ordering guards for free.

Before adding a `sort`, `trim`, `round`, or `to_lowercase` to a returned
value, ask what property you are deleting and whether anything else
asserts it — usually nothing does, because the helper exists so callers
need not restate shared properties. Where a helper already normalises,
remove the normalisation and count the failures: none means the
dimension is untested; many means you just recovered a guard across the
whole caller list.

## Review the selector as carefully as the assertion

A test over rendered output must first *select* what to assert on —
filter by indent, find by prefix, search for a substring. The assertion
gets the review attention, but the selector decides whether the claim is
about the subject at all.

`last_emitted_metric_group_uses_closing_connector` filtered group lines
at three columns while metric groups sit six columns in, so it matched
exactly one line — the `metrics` header — and asserted something true
about it. The test passed with every metric group rendering a dangling
`|-`, the precise defect its name claims to prevent (#1054).

- Assert the match count: `> 1` where several are required, an exact
  number where the shape is fixed. A filter that matches nothing makes
  every following assertion vacuously true.
- For indentation-structured output, compare whole lines or whole line
  sequences. `contains` cannot discriminate: a deeper rail *ends with*
  the shallower one, and labels like `sum` and `value` recur.
- When a test's name states a property, check the selected rows are
  capable of violating it.

## Know what the harness normalises away

Two chokepoints strip the input class you may be trying to test, and
they sit upstream of nearly every test in the workspace:

| Path | Normalisation |
| --- | --- |
| a module's `check_metrics` shim → `test_support::check_func_space_with` | `trim_end().trim_matches('\n')`, then `push(b'\n')` |
| integration suites → `read_file_with_eol` → `normalize_line_endings` | unconditional `data.push(b'\n')` |

Both guarantee a trailing newline, so **"a node ending at EOF" is
unreachable from either harness**. A regression test for that class
written the ordinary way passes against unfixed code. That is why both
issue #1051 (a `usize` underflow on a Rust `DocComment` at EOF) and its
sibling #1067 (`Sloc` keyed on `is_unit` rather than the span's end
column) survived. Use the verbatim helpers in `src/test_support.rs`, which reach
`analyze(Source::new(..))` with no normalisation: `metrics_verbatim` for
a root-aggregate assertion, `space_verbatim` when the claim is about a
*nested* space (#1067's per-function `sloc`). Do not "simplify" either
back to `check_metrics` — their doc comments say why.

The general form: when a test cannot be made to fail, check for an
intermediate stage that filters, normalises, or short-circuits the input
before it reaches the code under test. Pair any end-to-end test with a
direct unit test on the function whose contract is being verified.

## Assertion shapes that are wrong by construction

- **Never `include_str!` or `fs::read_to_string` the codebase's own
  source and string-match it.** The grep is brittle to rustfmt reflow
  and satisfied vacuously by adding the identifier in a comment; the
  production `matches!()` pattern already *is* the contract. If the
  kind_id is grammar-unreachable, document the contract at each call
  site instead. The one such test ever written here (#285's
  `FunctionDefinition4` regression) was removed as vacuous within
  months (#302).
- **Never assert the absence of a structural marker.** A
  `!report.contains("### Functions With Many Parameters")` passes
  *because* the heading is missing, encoding the bug as the contract —
  and inverts into a bug-lock the moment that absence *is* the symptom
  (#681). Assert the positive: the heading and its `id` anchor are
  present, the suppressed row is absent.
- **Never assert only that a section rendered or a call returned
  `Ok`.** Assert the value. `markdown_strip_prefix_accepted` passed
  against a no-op implementation.
- **Never build a hash/equality fixture with `Clone`.** A clone is
  byte-identical by definition, so the test verifies the derive and not
  the constructor — it holds even if the constructor mixes in a counter
  or timestamp. Construct both instances through the production
  constructor twice. Verified by revert on `PyAnalysisError`: the
  clone-based form passed with an `AtomicU64` interleaved into the
  field; the two-call form failed.
- **Never compare two structurally-equivalent containers to pin an
  ordering contract.** `dict ==` and `HashMap ==` are order-insensitive;
  compare against a hand-pinned sequence whose source order is
  deliberately non-alphabetical, or compare raw bytes positionally.

## Gate `#[cfg]` on the `fn`, never on an inner block

`#[cfg(unix)] { … }` wrapping a test body compiles to an empty body off
target — and an empty `#[test]` is a *passing* test. The harness reports
green on Windows with zero assertions run. The only correct placement is
on the function's attribute stack, alongside `#[test]`, so the test is
hidden rather than vacuous.

The inverse trap is fabricating a platform-shaped *input* — an
executable name, a path separator, a line ending — in one OS's spelling.
`test_conftest_helpers.py` created `debug/bca` while the locator appends
`.exe` on Windows: green on Linux and macOS, failing only on the Windows
leg. Mirror the production code's platform logic in the fixture
(`bca{EXE}`) rather than hardcoding one OS's form.

## Gate a feature-gated fixture table on the union of its rows

A test whose case list is built from `#[cfg(feature = …)]` rows has two
failure modes, and fixing one reintroduces the other. Left ungated, a
feature set that enables none of the rows leaves an empty list, a loop
of zero iterations, and a test that passes having asserted nothing —
which is why `assert_fixtures_present` (`src/test_support.rs`) exists to
make that state loud. But loudness alone turns the same build into a
spurious *failure* that reads as a defect in whatever was being changed.

Both halves are required:

- `#[cfg(any(feature = "a", feature = "b", …))]` on the **`fn`**, naming
  the union of the features its rows use, so the test is *absent* rather
  than failing when none is enabled.
- The non-vacuity assertion (`assert_fixtures_present`, or a `ran > 0`
  counter for a hand-rolled loop) inside it, covering the residual case
  where a runtime `is_enabled()` check stops agreeing with the feature
  it compiled under.

This bit twice in one batch (#1220, PR #1221).
`the_1184_constructs_open_quiet_function_spaces` had the assertion and
no gate, so `--no-default-features --features rust` failed with "at
least one language feature must be enabled for this test to mean
anything"; its two siblings had been gated in an earlier fix and it was
missed. Then the fix for #1218
rewrote a Tcl/iRules parity test as a loop, added a `ran > 0` guard, and
reproduced the identical false failure in a second file.

Verify against a subset that enables **none** of the named features.
`rust,typescript` — the canonical minimal-langs configuration — is not
such a subset for any table containing a TypeScript or TSX row, so a
single non-listed language (`--features go`) is the reproducer.

Note the union is over *features*, not languages: `LANG::Tsx` rides
`feature = "typescript"`, so a seven-row table can need only six.

**That verification needs `-p big-code-analysis`.** A workspace-wide
`cargo test --no-default-features --features X` does not isolate features
at all: `big-code-analysis-cli` and `big-code-analysis-web` depend on the
library with default features, and cargo unifies them, so everything is
silently re-enabled. Three different feature sets reported an identical
`4 passed; 3428 filtered out` before this was noticed (#1457) — the
uniform-number tell from the harness sections above. Only `-p` narrowed
it, and then the counts varied (0 / 2 / 3). A feature-gate check run
without `-p` proves nothing, and proves it while looking green; confirm
real isolation by watching the *filtered-out* count differ across sets.

## Every test that names a language carries that language's gate

The rule above is about a *fixture table*. The same requirement applies
one level down, to every individual test, and for a blunter reason:
`mk_langs!` generates each `*Parser` alias, `*Code` tag and `LANG`
variant unconditionally, so `check_metrics::<PythonParser>(…)` compiles
with `python` off and then panics inside `Tree::new`. Until #1472,
about 2,950 tests named a grammar without gating on it — 2,804 in
`src/`, 104 in `big-code-analysis-ast/src/`, 33 under `tests/`, plus 229
helpers and fixture tables. Approximate because the figure moves with
the derivation rules themselves: each refinement that stops reading
something as a use lowers it, and this section has already quoted a
higher number taken before the comparison rules landed. A
partial-feature build failed in the thousands.

`make check-test-lang-gates` derives and enforces it across all three of
those roots: for every item in a test scope it collects the languages
the body reaches — directly, and through same-module helpers that
hardcode a parser — and fails when the item's `cfg` would still admit it
into a build lacking one. A test needs
`all(…)` of what it names; a helper, a `const` fixture table or an
import needs `any(…)` of its users. `--fix` writes the markers and
`--show` prints the derivation.

Four things the derivation cannot see, so write those by hand and say
why in a comment:

- **A language chosen from a string.** A helper that maps a path
  extension or a filename glob onto a `LANG` hides it — the corpus tests
  and `suppression_test.rs`'s `analyze_lang` both did. Prefer passing
  the `LANG` explicitly where you can; that is a better call site
  anyway.
- **A trait used through its methods.** Nothing names `ParserTrait` in a
  body that calls `SomeParser::new`, so its import falls back to the
  module's whole union, so it carries no derived gate at all.
- **Anything `pub`.** Its users are in other files, which this
  single-file scanner does not see; `src/test_support.rs` is the whole
  story.
- **Which grammar a re-export serves.** Same reason, one step further
  out — a scoped `#[allow(unused_imports)]` beats a hand-copied union of
  seven files' gates.

Say so with a marker the gate reads, not prose alone:

```rust
// test-lang-gates: hand-written(cpp) — the corpus walk picks a
//     language per file from its extension, so the glob list decides
//     it and nothing in the body names it
#[cfg(feature = "cpp")]
```

Because the gate checks the *other* direction too (#1478). A gate wider
than the item needs keeps it out of builds it could have run in, and
that is the failure nothing else can see: too wide panics on the leg
that lacks the grammar, too narrow just drops the test and the leg still
looks green. Sixteen gates in this tree are wider than their bodies
justify, every one for a reason above; the marker is how you say which,
and a gate that grows a feature nobody can account for fails the gate.

A marker that stops being load-bearing fails too. Narrow the gate and
the feature it named is no longer over-declared, so the marker now
claims a reason that does not apply — and sixteen accepted gates only
read as a census while every one of them is still doing something. The
gate names the stale entry; drop the feature, or the whole marker when
it names nothing else.

Two things follow for anyone editing a marker by hand:

- **A comparison is not a use.** `lang == LANG::Go` asks which variant a
  value is; the enum is generated unconditionally, so it parses nothing
  and needs no grammar. Counting one as a requirement is what conjoined
  `feature = "go"` onto `container_scope_tests.rs` and dropped the
  positive half of the #1197 contract from every build without Go. This
  covers *every* alternative of a `matches!`, not only the leading one:
  the call has no arms and drives no table, so `matches!(lang,
  LANG::Ccomment | LANG::Preproc)` written to **skip** two languages is
  not a requirement for either. Reading the later arms as uses is what
  gated two `every_*_in_every_language` parity sweeps down to the one
  `c-family-helpers` that leaked out of that exclusion — they ran in
  four builds instead of twenty-three, and `--compare` was the only
  check that could see it.
- **A sweep still needs the parsers it hardcodes.** `is_enabled`
  filtering earns a row set only `any(…)`, because the loop skips what
  is missing. A parser named through a *type parameter*
  (`check_metrics::<PythonParser>`) cannot be skipped by any runtime
  filter, so it is required even inside a sweep — otherwise the
  exemption is a way round the whole gate. It propagates through
  helpers exactly as `needs` does: a sweep whose only fixed-parser call
  sits one hop away pins that parser just the same.

  **The pin covers a parser named as a *type*, and nothing else.** A
  `LANG::Rust` literal handed to `analyze` inside a sweep is not pinned,
  because the gate cannot tell one that sits in the iterated row table —
  which the `is_enabled` filter does skip — from one outside the loop,
  which it does not. Every sweep in the tree today filters per language,
  so nothing is currently wrong; but a sweep that mentions `is_enabled`
  anywhere and *also* parses a hard-coded `LANG` outside the loop would
  pass this gate and panic (#1480). Write the fixed-parser call as
  `check_metrics::<RustParser>` and it is pinned correctly.
- **A sweep over the whole enum needs `any(<every language>)`**, when it
  carries both halves of the rule above — an `is_enabled()` row filter
  *and* a non-vacuity assertion. Those two together mean it *fails*
  rather than skips with no language enabled, so it has to be absent
  then. Do not try to read a narrower set off the body: the fixtures
  come from a `LANG`-parameterised helper whose arms are deliberately
  not attributed to callers, so the body names almost nothing and
  whatever leaks through becomes the whole gate. A full-enum sweep
  *without* that guard is a different animal — `Display`, `FromStr` and
  slug round-trips walk the same enum over variants that exist without
  their grammars, and gating those stops them running on the
  `--no-default-features` leg that is exactly where they belong.

### Why the import lint is off on a partial build

Both library roots and the five integration-test crate roots carry:

```rust
#![cfg_attr(not(feature = "all-languages"), allow(unused_imports))]
```

Per-language gating makes "is this import live" a function of the
enabled feature set. A `use` that every test in a module needs under
`--all-features` is used by none of them once their grammars are gated
out, and `unused_imports` cannot express that. Nor can a `cfg` on the
import itself: a trait is reached through its methods and so is named
nowhere, a glob binds an unknowable set, and a macro is invoked from
item position. A derived union is wrong in both directions — too wide
and the import is unused, too narrow and it vanishes from under a test
that still compiles.

The scoping is what keeps it honest. `all-languages` is on by default
and under `--all-features`, so the build CI gates on and the one a
contributor runs both report an unused import exactly as before; only
the single-language legs, where the answer cannot mean anything, go
quiet.

Dead *items* are relaxed only in the two library roots, which already
carried an `allow(dead_code)` on the same condition before #1472. The
test crates do not, so every helper, `const`, macro and test in
`tests/` still needs its own gate.

### The direction that needs history

Both checks above compare a marker against the derivation. When the two
agree and are *both* wrong there is nothing left to compare against —
the marker faithfully mirrors a derivation that is itself too wide, and
no amount of re-reading either one says so.

`--compare <ref>` is the answer, and it needs no cargo: it scans the
tree at `ref` as well (`git archive` into a scratch directory), computes
which single-language builds compile each test in each, and fails on a
test that still exists but stopped being built somewhere. The probe set
comes from the language table, so a new language extends it for free.

CI runs it per pull request against the base branch head — the PR's
`base.sha`, not `git merge-base`, because the job checks out shallow and
cannot compute one. The difference shows on a PR that is behind: if
`main` widens a gate meanwhile, this reports it against a PR that never
touched it, and the label is then the right answer. It is deliberately
not in `make pre-commit`, which has no base revision to be meaningful
against; by hand it is
`make check-test-lang-gates-compare COMPARE_REF=origin/main`.

A test that was *deleted* or renamed reads as one name gone and another
arrived, and is not reported — a deliberate removal is not a gate
narrowing under it.

A *deliberate* narrowing looks identical, though, and does happen: the
gate really was too wide and the test does not need that grammar. Say so
with the `gate-narrowing-intended` label on the pull request, not with
an in-source marker. A marker would be permanently stale the moment the
branch lands — the comparison is relative to a moving base — and would
then sit in the tree as a hole nothing can detect, which is the failure
mode this whole gate exists to prevent.

The step reads that label **live**, not from the event payload, so
applying it after the failing run — which is when you find out you need
it — takes effect on the next run, and re-running the failed job works
too. Reading it from `github.event.pull_request.labels` did not: that
payload is a snapshot from when the run was queued, and a re-run replays
the same stale copy, so the documented remedy could never take effect at
all (measured on #1479).

## Assert a whole-run invariant in the run, not in a fixture list

When a change establishes an invariant that holds at the end of *every*
execution of a hot path — a scratch structure fully drained, a stack
balanced, a counter back at zero — a `debug_assert!` on that path
recruits the entire test corpus as input. A named test covers the
fixtures you thought of; the assertion covers every language, every
corpus file, and every integration suite that happens to run the path,
which is a different order of coverage for one line.

This matters most when the invariant has more than one owner. #1375's
nesting-slot lifetime is enforced in two places — the walk's
`exclude_tests` prune arm and `propagate_nesting_to_children` — so a
third `continue` added later would leak with nothing to notice. The
targeted test names four seeding paths; the `debug_assert!` covered
3,590 lib tests plus every integration suite on the first run, which is
what actually established the invariant across all twenty languages.

Both, not either:

- The **named test** is the regression test `AGENTS.md` requires, it is
  what you verify by revert, and it survives into release builds via an
  `observation::counter!`. It also documents *which* paths seed state,
  which an assertion cannot say.
- The **`debug_assert!`** generalises it for free, and is compiled out
  of release, so an O(1) check on a per-walk path costs nothing shipped.
  Keep it O(1): a per-node assertion is how the exact ancestor-chain
  check went quadratic (`make chain-audit`, #1122).

The assertion earns its place only if the suite actually exercises the
path under `cfg(debug_assertions)` — which `make pre-commit` does. An
assertion that only a release build reaches is lesson #80.
