# Testing guide

Project-specific testing practice for `big-code-analysis`: how to prove
a test can fail, which fixture shapes are vacuous by construction, what
the harness normalises away, and how feature gates work. It is for
anyone adding or changing tests here. The general lessons it rests on
are in [`lessons_learned.md`](lessons_learned.md).

## Verify defensive refactor tests by reverting the production code

A regression test earns its place by failing against the code it
guards. For a defensive refactor (one that pins an invariant rather
than fixing a live bug), demonstrate that before trusting it:

1. Commit or `git stash push <file>` the production change first. A
   file-level `git checkout -- <file>` over uncommitted work discards
   the fix along with the perturbation.
2. Revert or neutralise the production line with a precise edit, or
   patch it to a no-op in place when a partial revert would not build.
3. Run the new test and confirm it fails for the expected reason.
4. Restore with the exact inverse edit, then confirm `git diff --stat`
   shows only the edits you intend.

When reverting is impossible (the refactor deleted a helper the test
needs, or a newtype makes the old shape uncompilable), perturb instead:
transpose the fields the newtype protects, or flip the bound the test
pins, and confirm the test fails. Run the perturbation against the
whole suite. A perturbation that fails nothing means the invariant is
currently unobservable; say so in the test's comment rather than
overstating what the test proves. Claim "only this test catches it"
only after sweeping every crate that depends on the mutated one
(`cargo tree --workspace -i <crate> -e normal` lists them).

### A sweep of several perturbations needs a script

A sweep of several perturbations is a script, never a shell loop.
`utils/perturb.py` is that script: it refuses a target that is
modified, untracked or absent, keeps the backup in memory and in a
`mktemp` directory, rejects an anchor equal to its replacement, and
restores in a `finally`. Its manifest format and runners (`cargo`,
`unittest`) are in its module docstring. Rules a hand-rolled driver
must also follow:

- Assert `old != new` and `count(old) == 1` before every write; an
  anchor that stopped matching reports zero for the unperturbed code.
- The result parser is the other half of the subject, and so is the
  invocation: pass `--no-fail-fast` to any multi-target `cargo test`;
  the first failing target otherwise hides every later guard.
- Parse `failures:` lists and exit status together; `cargo test -q`
  prints dots, and a parser scanning for `FAILED` reads every mutant as
  caught by nothing. Classify a build failure by `error[` or
  `could not compile`, never bare `error`.
- Rebuild inside the measurement step for a compiled subject; a stale
  binary reports "0 files changed" for any edit.
- Run Python subjects with no bytecode cache (`python3 -B` after
  deleting `__pycache__`); a same-length edit restored within the same
  second runs the stale `.pyc`.
- After an interrupted sweep, `pgrep -af '<driver>[.]py'` and kill it
  first, then `cmp` each touched file against the backup. A mutant left
  in the tree looks like a half-finished fix.

A uniform result across perturbations (all zero, all the same count)
describes the harness, not the code. A perturbation that passes is a
finding: check whether the behaviour is enforced by the mechanism or
merely absent from a list before strengthening anything, and re-run the
perturbations whenever two guards merge into one predicate.

## Fixture shapes that cannot fail

### Seed the state you claim to assert on

- **Seed non-default state.** A reset asserted from `Default::default()`
  and an accumulation asserted from zero both hold whether or not the
  production line exists. Seed values that differ from the default and
  from each other; for a fold over a sub-population, give the excluded
  members a non-zero contribution.

### Perturb the fixture as well as the production line

- **Anchor the construct on its own axis.** A fixture pairing an
  excluded construct with a real condition still passes after someone
  trims the construct out. Assert something only the construct
  contributes (an `assignments_sum()`, a per-space count), and verify by
  deleting the construct. Where nothing remains to anchor on, say so in
  a comment.
- **Retiring a test retires its fixture's shapes.** Before deleting a
  hand-written test in favour of a derived one, list the constructs its
  fixture spelled and confirm the replacement corpus spells each.
- **Never let the measured value be the defect's output.** A probe
  whose only non-zero reading is produced by the bug scores zero once
  the bug is fixed. Build the measured quantity from content the fix
  does not touch.
- **Validate a scaling probe by removing what it prices.** A probe that
  reads ~1.0 with the memo removed is timing a path its workload never
  reaches. Record the perturbed exponent in the probe's `rationale`.
- **Prove a reachability probe can fire** with a throwaway
  `#[should_panic]` test before reading zero fires as a finding; a
  `debug_assert!` in a release build is dead.

### Coverage measures execution, not discrimination

- **Coverage measures execution, not discrimination.** Perturb the
  sub-expression carrying the invariant; compare covered counts, never
  percentages; diff sets rather than scalar summaries. Before quoting
  patch coverage, `cargo llvm-cov clean --workspace`, run the `coverage`
  job's commands, and read `utils/patch-coverage.py`.

### Normalise the expectation, never the observation

- **Normalise the expectation, never the observation.** A helper that
  sorts, trims or rounds the returned value deletes that property from
  every caller's assertions at once.

### Review the selector as carefully as the assertion

- **Review the selector.** A filter that matches nothing makes every
  assertion after it vacuous. Assert the match count; compare whole
  lines for indentation-structured output.

### Take the expected value from outside the fix

- **Take the expected value from outside the fix:** the construct's
  spelled-out twin as a row beside it, and the same input on `main`.

### Assertion shapes that are wrong by construction

- **Wrong by construction:** string-matching the repository's own
  source; asserting the absence of a structural marker; asserting only
  that a call returned `Ok`; building an equality fixture with `Clone`;
  comparing order-insensitive containers to pin an ordering.

### Assert a whole-run invariant in the run

- **Whole-run invariants** (a drained scratch structure, a balanced
  stack) get an O(1) `debug_assert!` on the hot path in addition to the
  named test; the suite then checks it on every corpus file.

## Know what the harness normalises away

`check_metrics` → `test_support::check_func_space_with` trims and
re-appends a trailing newline; the integration suites'
`read_file_with_eol` appends one unconditionally. A node ending at EOF
is unreachable from either. Use `metrics_verbatim` / `space_verbatim`
in `src/test_support.rs`, which reach `analyze(Source::new(..))`
untouched. When a test cannot be made to fail, look for the stage that
filters or normalises the input before it reaches the code under test,
and pair any end-to-end test with a direct unit test on the function
whose contract is verified.

## Platform and feature gates

### Gate `#[cfg]` on the `fn`, never on an inner block

An empty test is a passing test, so `#[cfg(unix)] { … }` around a
body compiles to green with zero assertions off target. Put the gate on
the function's attribute stack. Mirror the production code's platform
logic in fixtures (`bca{EXE}`), not one OS's spelling.

### Gate a feature-gated fixture table on the union of its rows

- A fixture table built from `#[cfg(feature = …)]` rows needs both
  `#[cfg(any(<the union of its rows' features>))]` on the `fn` and a
  non-vacuity assertion (`assert_fixtures_present`, or `ran > 0`)
  inside it. Verify against a feature set that enables none of the rows (a
  single non-listed language such as `--features go`), with
  `-p big-code-analysis`: a workspace-wide run unifies features
  through the CLI and web crates and isolates nothing.

### Every test that names a language carries that language's gate

- `mk_langs!` generates every `*Parser` alias unconditionally, so a test
  naming `check_metrics::<PythonParser>` compiles with `python` off and
  panics. `make check-test-lang-gates` derives the gate each test and
  helper needs (`all(…)` of what a test names, `any(…)` of a helper's
  users) and fails in both directions: too narrow and too wide. `--fix`
  writes markers, `--show` prints the derivation. What it cannot see,
  mark by hand with a comment the gate reads:

  ```rust
  // test-lang-gates: hand-written(cpp) — the corpus walk picks a
  //     language per file from its extension
  #[cfg(feature = "cpp")]
  ```

  Cases needing the marker: a language chosen from a string, a trait
  used only through its methods, anything `pub`, a re-export, and a
  parent module's private item reached through `use super::*`. A
  comparison (`lang == LANG::Go`, any `matches!` arm) is not a use; a
  parser named as a type parameter inside an `is_enabled` sweep is.
- `make check-test-lang-gates-compare COMPARE_REF=origin/main` catches
  a gate that narrowed since the base revision, which the per-tree gate
  cannot see. CI runs it per PR; a deliberate narrowing is declared
  with the `gate-narrowing-intended` label, never an in-source marker.

### Why the import lint is off on a partial build

- The library roots and integration crate roots carry
  `#![cfg_attr(not(feature = "all-languages"), allow(unused_imports))]`
  because import liveness depends on the feature set; dead-item
  allowances exist only in the two library roots, so every helper in
  `tests/` still needs its own gate.
