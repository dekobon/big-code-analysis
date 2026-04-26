---
name: review
description: Audit code changes for correctness, performance, security, and quality issues. Use when asked to review changes, diffs, or pull requests.
---

# Review Changes

Audit the current change set for correctness, performance, security, and
quality problems. Produce concrete, actionable findings.

## Scope

Determine what to review based on `$ARGUMENTS`:

| Argument | Scope |
|----------|-------|
| *(empty)* | Unstaged + staged changes (`git diff HEAD`) |
| `staged` | Staged changes only (`git diff --cached`) |
| `branch` | All commits on current branch vs `master` (`git diff master...HEAD`) |
| `pr <N>` | Pull request diff (`gh pr diff <N>`) |
| `<commit>` | Single commit (`git show <commit>`) |
| `<commit>..<commit>` | Commit range |
| `<path or glob>` | All content in matching files (full-file review, no diff) |

---

## Step 1: Gather the diff and context

1. Obtain the diff for the determined scope.
2. List every file touched. For each file, read the **full file** — not just
   the diff hunks. Findings require surrounding context to be meaningful.
3. For each changed function/method, identify its callers and callees using
   LSP tools when available (`find_referencing_symbols`, `find_symbol`).
   Changes that break or subtly alter a contract are only visible when you see
   who depends on it.
4. If `docs/development/lessons_learned.md` exists, read it and check whether
   any lesson applies to the change under review.

---

## Step 2: Audit checklist

Apply every applicable question to every changed region. Record each finding:

```
FINDING: <short title>
FILE: <path>:<line range>
CHECKLIST: <question number(s)>
EVIDENCE: <what is wrong and why, with code snippet>
SEVERITY: bug | performance | security | code-smell | test-gap
EFFORT: trivial | small | medium
```

### Correctness

1. Does the change actually solve the stated problem, or only mask a symptom?
2. Are there off-by-one errors in ranges, indexes, slices, or boundary checks?
3. Are there match arms, if-else branches, or rule bodies that are unreachable
   or logically dead after this change?
4. Are error cases handled, or silently swallowed (`continue`, `_ => {}`,
   `Err(_)` discarded, `unwrap()` in non-test code)?
5. Can this change produce false positives on valid inputs or false negatives
   on invalid inputs?
6. Are edge cases handled: empty input, single-element input, maximum-size
   input, `None`/`null`, non-UTF-8 paths, missing files, concurrent access?
7. Does the change preserve existing invariants? Check callers — will any
   caller now receive different behavior without being updated?
8. If the change touches serialization or deserialization (JSON/YAML/TOML
   metric output, language enums, AST output), does the output still conform
   to the expected schema/format?
9. Is recursion bounded? Tree-sitter walks and AST traversals must not blow
   the stack on pathological inputs.
10. Are generated artifacts (e.g., the `enums/` crate output, language tables)
    still consistent with their generator logic after this change?

### Performance

11. What is the time complexity of the changed code path? Is it appropriate
    for expected input sizes (large source files, whole repositories, deep
    ASTs)? Flag O(n²) or worse when O(n) or O(n log n) is feasible.
12. Are there unnecessary allocations in hot paths? Look for: repeated
    `String::from` / `.to_string()` / `.clone()` inside loops, `collect()`
    into intermediate `Vec` that is immediately iterated again, `format!()`
    for static strings.
13. Are data structures appropriate? Examples: linear scan of a `Vec` where a
    `HashSet`/`HashMap` lookup would be O(1); a `BTreeMap` where insertion
    order does not matter and `HashMap` suffices; a `Vec` used as a set with
    `contains()` checks.
14. Is there repeated work that should be cached, memoized, or hoisted out of
    a loop? Watch for tree-sitter cursor reuse and per-node recomputation.
15. Are there I/O calls (file reads, process spawns, network) inside a loop
    that could be batched or moved outside?
16. Does the change introduce or worsen any O(n) growth in memory? Could
    large inputs (multi-MB source files) cause OOM?
17. For string processing: are there repeated scans of the same string, or
    patterns where `&str` borrowing would avoid allocation?
18. If the change adds a new collection, is its initial capacity reasonable?
    (`Vec::with_capacity`, `HashMap::with_capacity` for known sizes.)

### Security

19. Can malicious or pathological input cause stack overflow, infinite loop,
    unbounded memory growth, or CPU exhaustion? (Especially relevant for
    parser/AST code that consumes untrusted source.)
20. Is there path traversal risk in any file-loading, directory-walking, or
    path-construction logic?
21. Are symlinks handled safely, or could they be exploited to read/write
    outside the expected directory?
22. Does the change use `to_string_lossy()` on paths used as identifiers
    (map keys, JSON fields, error correlation)? This silently corrupts
    non-UTF-8 paths. Use `to_str()` with error handling instead.
23. Does any new feature allow user-supplied content to execute arbitrary
    code, shell commands, or make network calls?
24. Are there hardcoded secrets, tokens, or environment-specific paths?
25. Does the change introduce any injection risk (command injection, format
    string injection)?

### Error handling and robustness

26. Does the change propagate errors correctly, or does it convert a specific
    error into a generic one, losing diagnostic information?
27. Are new error messages actionable? Do they include the relevant context
    (file path, line number, input value) that a user needs to diagnose the
    problem?
28. Is `unwrap()` or `expect()` used outside of test code? If `expect()` is
    used, is the invariant truly guaranteed and documented?
29. Are there new `todo!()`, `unimplemented!()`, or `unreachable!()` calls
    that could be reached at runtime?

### API design and contracts

30. Are public function signatures clear? Do parameter types, return types,
    and names communicate the contract without needing to read the body?
31. If the change adds or modifies a public API, is it the minimal sufficient
    interface? Does it expose implementation details that should be private?
32. Are new types/structs using appropriate visibility (`pub`, `pub(crate)`,
    private)?
33. Does the change maintain backward compatibility where required, or is the
    break intentional and documented? (`rust-code-analysis` is published as a
    library on crates.io — public API breaks affect downstream users.)

### Tests

34. Does every new code path have a corresponding test?
35. Do existing tests still cover their intended scenarios after this change,
    or has the change shifted behavior out from under them?
36. Are test assertions specific enough? Tests that assert `is_ok()` or
    `!is_empty()` without checking the actual value are weak.
37. Are there missing negative tests (invalid input, error paths, boundary
    conditions)?
38. Are test names descriptive of the scenario and expected outcome?
39. If the change fixes a bug, is there a regression test that would catch
    the exact bug if reintroduced?
40. For language-specific changes: are tests added for **every** language
    affected, not just one?

### Code quality

41. Is this the simplest implementation that solves the problem? No
    over-engineering, premature abstraction, or speculative generality.
42. Are there duplicated logic blocks that should share a helper or macro?
    (Per-language modules in `src/languages/` deliberately use macros for
    shared structure — extend them rather than copy-pasting.)
43. Are there functions longer than ~50 lines of logic that should be
    decomposed?
44. Are variable and function names accurate and descriptive after this
    change?
45. Are there stale comments, doc-comments, or TODO markers that this change
    should have updated or removed?
46. Are there unnecessary `clone()`, `to_owned()`, or `collect()` calls where
    borrowing or iterating directly would suffice?
47. Are imports clean? No unused imports, no wildcard imports that pull in
    excess names.

---

## Step 3: Validate findings

For each finding:

1. Re-read the evidence. Confirm the file, line range, and reasoning are
   concrete and accurate. Discard anything speculative.
2. Check whether the finding existed before this change. If the issue is
   pre-existing and the change does not make it worse, note it as
   "pre-existing" but still report it — the reviewer should be aware.
3. Collapse findings that share the same root cause into a single finding.

---

## Step 4: Report

Print findings grouped by severity, highest first:

```
## Review: <scope description>

### Bugs
| # | Finding | File | Effort | Evidence |
|---|---------|------|--------|----------|

### Performance
| # | Finding | File | Effort | Evidence |
|---|---------|------|--------|----------|

### Security
| # | Finding | File | Effort | Evidence |
|---|---------|------|--------|----------|

### Test gaps
| # | Finding | File | Effort | Evidence |
|---|---------|------|--------|----------|

### Code quality
| # | Finding | File | Effort | Evidence |
|---|---------|------|--------|----------|

### Summary
- Files reviewed: N
- Findings: N (N bug, N performance, N security, N test-gap, N code-quality)
- Pre-existing issues noted: N
- Verdict: APPROVE | APPROVE WITH COMMENTS | REQUEST CHANGES
```

If there are zero findings, say so explicitly and state "APPROVE".

---

## Guardrails

- Do NOT implement fixes. This is review-only.
- Do NOT report findings without concrete evidence (file + line + reasoning).
- Do NOT flag style preferences already handled by `cargo fmt`, `clippy`, or
  `markdownlint-cli2`. Automated tools own style.
- Do NOT flag pre-existing issues as blocking unless the change makes them
  worse.
- Read full files, not just diff hunks. Context matters.
- Use LSP tools to trace callers and callees — do not guess at impact.
