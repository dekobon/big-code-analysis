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
