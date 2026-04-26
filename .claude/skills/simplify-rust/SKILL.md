---
name: simplify-rust
description: Review changed Rust code for reuse, quality, and efficiency, then fix any issues found.
---

# Simplify Rust Code

Review recently changed Rust code and fix issues across three dimensions: reuse,
clarity, and efficiency. Focus on semantic problems that `cargo fmt` and `clippy`
cannot catch. Apply fixes directly -- don't just list suggestions.

## Scope

Determine what to review based on `$ARGUMENTS`:

| Argument | Scope |
|----------|-------|
| *(empty)* | Unstaged + staged changes (`git diff HEAD`) |
| `staged` | Staged changes only (`git diff --cached`) |
| `branch` | All commits on current branch vs `master` (`git diff master...HEAD`) |
| *path* | Specific file or directory (any argument that isn't a keyword above) |

## Process

1. Collect the diff for the chosen scope
2. Read each changed file to understand context around the diff hunks
3. Use the Agent tool to spawn three parallel **read-only** review agents (one per dimension below). Agents analyse and return findings — they do NOT edit files.
4. Aggregate findings, deduplicate, and classify by priority
5. Apply fixes directly to the code (orchestrator only — not the review agents)
6. Run `cargo check --workspace` to verify changes compile
7. Summarize what changed and why, with `file:line` references

## Priority Classification

| Priority | Action | Description |
|----------|--------|-------------|
| **Fix now** | Apply immediately | Correctness risks, API misuse, missing error propagation |
| **Improve** | Apply in this pass | Clarity wins, unnecessary allocations, dead abstractions |
| **Note** | Comment only | Minor style preferences, subjective naming |

Skip anything already clean. Do not create churn.

---

## Agent 1: Reuse

Look for duplicated logic and missing abstractions.

- Repeated conversion code that should be a `From`/`TryFrom` impl
- Copy-pasted validation or formatting logic across functions
- Manual error mapping chains replaceable by a single `From` impl
- Identical match arms that can be consolidated
- Helper functions that duplicate standard library or crate functionality
- Per-language duplicated logic in `src/languages/` that could be expressed via
  a trait method or macro instead of being copied across language modules

**Do NOT extract**: one-off logic into tiny helpers that obscure the flow. Three
similar lines are better than a premature abstraction.

## Agent 2: Clarity

Look for unnecessary complexity and missed Rust idioms.

- `unwrap()` / `expect()` in non-test code -- must use `?` or `Result`/`Option`
- Complex nested `if`/`match` that can be flattened with early returns or `?`
- Boolean flags or stringly-typed APIs that should be enums or newtypes
- `pub` items that should be `pub(crate)` (not used outside the crate)
- Functions longer than ~40 lines that mix unrelated concerns
- `impl` blocks far from their struct/enum definition
- Methods not grouped: constructor > getter > mutation > domain > helper
- Redundant type annotations the compiler can infer
- `to_string_lossy()` on paths used as identifiers (must use `to_str()` + error handling)
- Missing input validation on public functions (empty strings, zero-length slices)
- Unreachable code using fallback logic instead of `expect("invariant")`
- Numeric literals missing underscore separators for readability

**Do NOT simplify**: clear `for` loops into unreadable iterator chains; clear
`if`/`else` into clever `match` patterns; or add lifetime annotations the
compiler already infers.

## Agent 3: Efficiency

Look for unnecessary allocations and wasted work.

- `.clone()` where a borrow suffices (the parameter doesn't need ownership)
- `String` parameters where `&str` works
- `Vec` allocations where a slice or iterator would do
- Unnecessary `.collect()` into intermediate `Vec` before further iteration
- `String::from()` / `.to_string()` where `&'static str` or `Cow` works
- Missing `with_capacity()` for collections built in loops
- Repeated tree-sitter node walks where a single traversal would suffice
- Re-parsing source where an existing AST/parser handle could be reused

**Do NOT optimize**: hot-path micro-optimizations without evidence of impact.
Clarity beats performance for non-critical paths.

---

## Project Conventions

This project is the `big-code-analysis` Rust workspace (root crate plus
`big-code-analysis-cli` and `big-code-analysis-web`). Key Rust principles to
enforce:

- Never write `unsafe` code
- Prefer `pub(crate)` over `pub`
- Use newtype wrappers for domain invariants
- Prefer borrowing over cloning
- Use `expect("reason")` for provably unreachable code, not fallback logic
- `path.display()` for log output; `to_str()` + error handling for identifiers

## What NOT to Do

- Don't touch code outside the diff scope
- Don't add comments to self-evident code
- Don't over-genericize (`impl AsRef<str>` everywhere)
- Don't extract tiny helpers that obscure flow
- Don't refactor working patterns just because a different idiom exists
- Don't add docstrings to unchanged functions
