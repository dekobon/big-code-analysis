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
