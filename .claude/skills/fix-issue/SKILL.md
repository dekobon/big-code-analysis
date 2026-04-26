---
name: fix-issue
description: Complete workflow for fixing GitHub issues including investigation, implementation, review, testing, and documentation. Use when asked to fix a GitHub issue.
---

# Fix GitHub Issue Workflow

1. Read the GitHub issue thoroughly (`gh issue view <number>` plus all comments).
2. If Serena (or another LSP-based code intelligence MCP) is available, activate
   the project (`serena:activate_project`) so symbol-level navigation and editing
   are the default. LSP tools are preferred over text-based search/edit for code;
   fall back to `rg`/`fd` and the built-in Grep/Glob tools only when LSP is
   unavailable. Never use legacy `grep`/`find`.
3. Re-read any relevant project conventions (e.g., `CLAUDE.md`, `README.md`, and
   any rule files under `.claude/rules/` if present). Note any rule that is
   directly relevant so it can be cited in the fix.
4. Investigate the codebase to understand the root cause. For tree-sitter
   grammar / language-specific behavior, examine the corresponding module under
   `src/languages/` and confirm whether the bug is in our wrapper or upstream
   in the grammar crate. If the bug is upstream, scope the fix accordingly
   (workaround locally, file an issue against the grammar repo, or both).
5. Check for the same bug pattern elsewhere in the codebase. The
   `src/languages/` modules deliberately mirror each other; a bug in one
   language's metric implementation often exists in several. Fix all
   instances — do not leave known-broken siblings for a follow-up.
6. **Plan the fix using sequential thinking.** Use the
   `sequential-thinking:sequentialthinking` MCP tool to reason through the
   resolution step by step before writing any code. The sequential thinking
   process MUST:
   - **Start** with `thoughtNumber: 1`, an initial `totalThoughts` estimate
     (typically 5-8), and `nextThoughtNeeded: true`.
   - **Analyze** the root cause — not just the symptom. Trace the data/control
     flow that leads to the bug.
   - **Enumerate approaches** and evaluate trade-offs (simplicity, correctness,
     performance, scope).
   - **Identify edge cases** — empty inputs, boundary values, deeply nested
     ASTs, non-UTF-8 source, mixed line endings, language-specific quirks
     (preprocessor directives in C/C++, JSX in JavaScript, generics in Rust),
     concurrent access. Walk through each edge case and confirm the proposed
     fix handles it.
   - **Cross-check against project rules** — if the fix would introduce a
     silent `unwrap_or_default`, an `unwrap()` in non-test code, an
     unexplained abbreviation, a `to_string_lossy()` on an identifier path,
     `unsafe` code, or any other anti-pattern, redesign before proceeding.
   - **Verify completeness** — confirm the plan covers implementation, tests
     for **every affected language**, and documentation before concluding.
   - **Conclude** with `nextThoughtNeeded: false` and a final plan summary.
   - Adjust `totalThoughts` up or down as understanding evolves. Use
     `isRevision` if earlier reasoning needs correction.
7. **Implement the fix.** Do NOT stop after planning — execute the plan from
   step 6. If the implementation reveals issues the plan missed, revise via
   sequential thinking before proceeding. Before changing any public API, run
   `find_referencing_symbols` (or equivalent) to enumerate every call site.
   Remember: this crate is published on crates.io; public API breaks affect
   downstream users.
8. **Write tests.** Sufficient testing is mandatory before review. At minimum:
   - **Unit tests**: for all new or changed public functions. Each edge case
     identified in step 6 should have a corresponding test.
   - **Integration tests**: for end-to-end behavior changes. Run
     `cargo build --workspace` before integration tests so they exercise the
     new binaries — never test against a stale binary.
   - **Per-language coverage**: if the fix touches metric computation, AST
     traversal, or any code under `src/languages/`, exercise **every**
     language affected. A regression in one language is not caught by passing
     tests in another.
   - **Regression check**: `cargo test --workspace` and
     `cargo clippy --workspace --all-targets -- -D warnings` must pass.
   - Tests must actually assert what they claim — no silent fallbacks
     (`unwrap_or_else` that swallows failures), no missing assertions, no
     coupling to incidental host/filesystem details.
9. **Review the changes** for:
   - **Correctness**: Does the fix actually address the root cause, not just
     the symptom?
   - **Performance**: Are algorithms and data structures appropriate? Avoid
     O(n²) when O(n) is feasible. Consider hot paths (whole-repo analysis,
     large source files, deep ASTs) and large inputs.
   - **Simplicity**: Is this the simplest fix that solves the problem? No
     over-engineering, no speculative abstractions.
   - **Completeness**: Are edge cases handled? Are there similar patterns
     elsewhere — especially in sibling language modules — that need the
     same fix?
   - **Test coverage**: Do the tests from step 8 cover the root cause, edge
     cases, and regression scenarios across every affected language?
   - **Conventions**: Does the diff respect the Rust principles in this
     project (no `unwrap`/`expect`/`panic` outside tests, no `unsafe`,
     newtypes for domain invariants, narrow visibility, borrowing over
     cloning)?
10. Fix any issues found in review. If the fixes were non-trivial, re-review.
    Do NOT commit with known issues and plan to fix them in a follow-up — that
    is how fix-up chains happen.
11. Run the final gate before committing:
    - `cargo fmt --all -- --check`
    - `cargo clippy --workspace --all-targets -- -D warnings`
    - `cargo test --workspace`
    - `pre-commit run --all-files` (if pre-commit is installed)
    - `markdownlint-cli2` against any Markdown files touched
    If any check fails, fix and re-run until clean.
12. Run integration / CLI tests against the NEW binary if applicable to the
    change (rebuild first; never test stale).
13. **Update all documentation.** Review and update each of the following as
    applicable:
    - `CHANGELOG.md` — add an entry under the appropriate section (Added,
      Changed, Fixed) if the project keeps one.
    - `README.md` — if user-facing behavior, install steps, or supported
      languages changed.
    - `big-code-analysis-book/` — if the change affects documented metrics,
      output formats, or CLI behavior.
    - Crate-level or module-level doc comments (`//!`) — if module intent or
      architecture changed.
    - Avoid hardcoding stale counts in any doc ("all tests passing", not "42
      tests passing").
    - Re-run `markdownlint-cli2` after editing any Markdown files.
14. If there is a hard-won, globally reusable lesson from this fix, run the
    `/lessons-learned` skill (or propose an entry directly to
    `docs/development/lessons_learned.md` if that file exists) and prompt the
    user for approval. Keep the bar high — only lessons that cost real
    debugging time and are likely to recur.
15. Commit using Conventional Commits: `<type>(<scope>): <subject>` with
    `Fixes #NN` in the **body**, not the subject line. Keep commits atomic;
    do not add `Co-Authored-By` lines unless the user asks for them.
16. Update the GitHub issue body with results AND add a comment with research
    and findings. Use `--body-file` with a temp file for non-trivial bodies.
17. Close the issue with `gh issue close <number>` only when ALL items are
    resolved. If items remain unresolved, do NOT close — instead update the
    issue body to reflect what is done and what is left.

## Worktree Safety Reminder

If this session is running inside a worktree (`git rev-parse --show-toplevel`
returns a path under `.claude/worktrees/`), worktree-safety bans apply
throughout this workflow: never delete worktrees, never `cd` to the main repo,
never check out a different branch, never write to files outside your
worktree.
