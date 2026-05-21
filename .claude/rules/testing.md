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
