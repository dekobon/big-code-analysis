---
name: improve-crate
description: Safe code improvement workflow for a single crate. Creates an integration branch, analyzes code quality, and applies fixes in parallel worktrees.
---

# Improve Crate

Run a safe code improvement workflow on the Rust crate `$ARGUMENTS`. Creates
an integration branch, analyzes the crate for improvement opportunities,
applies fixes in parallel worktrees, and integrates successful changes.

## Arguments

Parse `$ARGUMENTS` as: `<crate-name> [--dry-run]`

- `<crate-name>` (required): one of `big-code-analysis`,
  `big-code-analysis-cli`, `big-code-analysis-web`
- `--dry-run` (optional): stop after Step 2 (analysis) and print the change
  areas without spawning worktree agents

## File Scope

- All `.rs` files in the crate's directory
- For the root `big-code-analysis` crate, scope is `src/` and `tests/`
- For `big-code-analysis-cli` / `big-code-analysis-web`, scope is the
  matching subdirectory's `src/` and `tests/`

## Constraints

- **Safe refactors only**: no public API changes, no data model changes,
  no cross-crate changes
- **No public API breaks**: this is a published library — `lib.rs`
  re-exports, public traits (`ParserTrait`, `LanguageInfo`, etc.), and
  public types (`Metrics`, `FuncSpace`, language enums) are off-limits
  unless the user explicitly authorizes a version bump
- **Cross-language parity**: per-language modules under `src/languages/`
  deliberately mirror each other; any change to one usually requires the
  same change to all sibling language modules
- **Do not merge to master**: leave the integration branch for the user
- **Skip on failure**
- **No re-examination**: skip files/symbols already reviewed in prior runs

---

## Step 0: Validate and setup

### 0a: Validate crate name

```bash
WORKSPACE_CRATES=$(cargo metadata --format-version 1 --no-deps \
  | jq -r '.packages[].name' | sort | paste -sd, -)
CRATE_DIR=$(cargo metadata --format-version 1 --no-deps \
  | jq -r --arg name "$CRATE_NAME" \
    '.packages[] | select(.name == $name) | .manifest_path' \
  | xargs dirname 2>/dev/null)

if [[ -z "$CRATE_DIR" || ! -d "$CRATE_DIR/src" ]]; then
  echo "Error: crate '$CRATE_NAME' not found." >&2
  echo "Valid crates: $WORKSPACE_CRATES" >&2
  exit 1
fi
```

### 0b: Ensure clean working tree

```bash
git status --porcelain
```

If dirty, abort.

### 0c: Create integration branch

```bash
git checkout -b improve/<crate-name> master
```

If the branch already exists from a prior partial run, check it out and
continue.

### 0d: Detect isolation mode

```bash
PROJECT_ROOT="$(git rev-parse --show-toplevel)"
if [[ "$PROJECT_ROOT" == *".claude/worktrees/"* ]]; then
  ISOLATION_MODE="worktree"
else
  ISOLATION_MODE="branch"
fi
```

---

## Step 1: Load prior state

Read Serena memories via `serena:read_memory`:

1. `audit-state-<crate-name>` — prior audit findings
2. `code-improvement/<crate-name>` — prior improvement runs

If `code-improvement/<crate-name>` exists, identify reviewed-clean,
changed, and skipped symbols.

---

## Step 2: Analysis

### 2a: Collect code metrics

This project is itself a code-metrics tool. Use its CLI to measure the
crate under improvement:

```bash
cargo build -p big-code-analysis-cli >/dev/null 2>&1
./target/debug/big-code-analysis-cli -m -O json -p "$CRATE_DIR" \
  > /tmp/improve-metrics.json || true
```

Parse the metrics output (see `src/output/dump_metrics.rs` and the
serializers under `src/output/` for the JSON schema). Extract
improvement targets:

| Metric threshold | Improvement dimension | What to look for |
|------------------|----------------------|-------------------|
| Cyclomatic complexity > 10 | Clarity | Decompose into smaller functions |
| Cognitive complexity > 15 | Clarity | Flatten nesting, extract helpers |
| SLOC > 100 | Clarity | Split along logical seams |
| Parameters > 3 | Clarity | Consolidate into option/config structs |
| Halstead estimated bugs > 0.5 | Correctness risk | Prioritize for review |
| Maintainability Index < 10 | All dimensions | Most work needed here |

If the CLI build fails, skip this substep and proceed without metrics —
the Explore agent in 2b can still discover targets by reading files,
just less efficiently. Do NOT substitute `clippy::pedantic`: its style
warnings are not a substitute for cyclomatic / cognitive / Halstead
ranking and would steer the agent toward noise.

### 2b: Explore agent

Launch a single Explore agent (`subagent_type: "Explore"`) with the prior
state from Step 1 and the metrics target list from Step 2a.

The agent must:

1. Use Serena `get_symbols_overview` on each `.rs` source file (fall back
   to Read if unavailable).
2. **Start with functions flagged by metrics**. For each, use
   `find_symbol` with `include_body=true` (or Read with line ranges) to
   read the full body. Then scan remaining symbols that look complex.
3. Cross-reference with audit state. Skip symbols marked clean or changed.
4. Identify safe refactor opportunities across:
   - **Reuse**: duplicated logic, missing `From`/`TryFrom` impls,
     copy-paste patterns, helpers that should be promoted from
     per-language modules to a shared location
   - **Clarity**: unnecessary complexity, missed idioms, `unwrap()` /
     `expect()` / `panic!()` in non-test code (forbidden by project
     conventions), overly long functions, `pub` that should be
     `pub(crate)` (but check `lib.rs` re-exports first), `to_string_lossy()`
     on identifier paths
   - **Efficiency**: unnecessary clones, `String` where `&str` works,
     missing `with_capacity`
5. Group findings into **logical change areas**, each:
   - Touches a cohesive set of symbols (ideally within one file)
   - Can be described in a single conventional commit message
   - Is independent of other change areas
   - For changes under `src/languages/`, ALL affected sibling language
     modules are included in the same area (you cannot improve one
     language module without bringing the rest along)
6. Return as structured list:

```
## Change Area 1: <conventional commit message>
- file.rs: symbol_a -- <what to improve>
- file.rs: symbol_b -- <what to improve>
```

### Dry-run exit point

If `--dry-run`, print the change areas and stop:
"Dry run complete. N change areas identified. Re-run without --dry-run
to apply."

---

## Step 3: Fix cycle (agents)

#### Worktree mode

**CRITICAL**: every agent MUST be launched with `isolation: "worktree"`.
Launch all change area agents in parallel.

#### Branch mode

All agents MUST be processed **sequentially**. For each change area:

1. Create a feature branch:

```bash
BRANCH="improve/${CRATE_NAME}-area-${AREA_NUMBER}"
if git rev-parse --verify "$BRANCH" >/dev/null 2>&1; then
  git branch -D "$BRANCH"
fi
git checkout -b "$BRANCH" "improve/${CRATE_NAME}"
```

2. Launch ONE Agent (NO `isolation: "worktree"`).

3. On **SUCCESS**:

```bash
git checkout "improve/${CRATE_NAME}"
git merge "$BRANCH" --no-edit
git branch -d "$BRANCH"
```

4. On **SKIPPED**:

```bash
git checkout -- .
git reset HEAD
git checkout "improve/${CRATE_NAME}"
git branch -D "$BRANCH"
```

5. Verify clean state before next area.

#### Agent prompt

**BEGIN AGENT PROMPT**

You are improving code in crate `<CRATE>`. Your change area:

```
<CHANGE_AREA>
```

### 3a: Setup — Environment Verification (MANDATORY)

```bash
PROJECT_ROOT="$(git rev-parse --show-toplevel)"
if [[ "$PROJECT_ROOT" == *".claude/worktrees/"* ]]; then
  ISOLATION_MODE="worktree"
else
  ISOLATION_MODE="branch"
fi
echo "ISOLATION_MODE=$ISOLATION_MODE PROJECT_ROOT=$PROJECT_ROOT"

AGENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
echo "AGENT_BRANCH=$AGENT_BRANCH"
```

**HARD GATE**: If `AGENT_BRANCH` is `master`, `main`, or `HEAD` (detached),
abort:

```
SKIPPED: Agent is on disallowed branch. AGENT_BRANCH=<branch>
```

**BRANCH SAFETY**: Do NOT switch branches.

In worktree mode: ALL file operations within `PROJECT_ROOT`.

If Serena MCP available, call `serena:activate_project`. Otherwise use
Read, Edit, Grep, Glob.

### 3b: Apply improvements

1. Understand the code first:
   - **With Serena**: `get_symbols_overview`, `find_symbol` with
     `include_body=true`, `find_referencing_symbols` before changing
     anything callable externally.
   - **Without Serena**: Read full files, Grep for callers.
2. **Verify public API safety**: if a symbol is re-exported from
   `src/lib.rs`, it is part of the published API surface. Do NOT change
   its signature or behavior. Limit changes to internal implementation.
3. **Cross-language parity**: if your change area includes a symbol in
   one `src/languages/language_<X>.rs`, apply the equivalent change in
   every sibling `language_*.rs` that defines the same symbol.
4. Apply improvements:
   - **With Serena**: `replace_symbol_body`, `insert_before_symbol`,
     `insert_after_symbol`.
   - **Without Serena**: Edit for targeted replacements.
5. Stay within scope.

### 3c: Simplify (inline review)

After your changes, run `git diff HEAD` and review each changed file in
full. Apply fixes directly:

**Reuse**:

- Repeated conversion code → `From`/`TryFrom` impl
- Copy-pasted validation/formatting across functions
- Manual error-mapping chains replaceable by a single `From` impl
- Identical match arms that can be consolidated
- Helpers duplicated across `language_*.rs` that could move to
  `src/macros.rs` / `c_langs_macros/` / a shared module

**Clarity**:

- `unwrap()`, `expect()`, `panic!()`, `assert!()` in non-test code →
  must use `?` or `Result`/`Option` (project convention forbids these
  outside tests; documented invariants in `expect("reason")` are the
  only exception)
- Complex nested `if`/`match` → flatten with early returns or `?`
  (let-else and let-chains are available — edition 2024)
- Boolean flags / stringly-typed APIs → enums or newtypes
- `pub` items not used outside the crate → `pub(crate)` (but check
  `lib.rs` re-exports first)
- Functions > ~40 lines mixing unrelated concerns
- Redundant type annotations the compiler infers
- `to_string_lossy()` on paths used as identifiers (use `to_str()`
  with explicit error handling)
- Numeric literals missing underscore separators

**Efficiency**:

- `.clone()` where a borrow suffices
- `String` parameters where `&str` works
- `Vec` allocations where a slice or iterator would do
- Unnecessary `.collect()` into intermediate `Vec`
- Missing `with_capacity()` for collections built in loops

Do NOT: extract tiny helpers that obscure flow, simplify clear `for`
loops into unreadable iterator chains, or add lifetime annotations the
compiler infers.

```bash
cargo check -p <CRATE>
```

### 3d: Review (inline audit)

Review `git diff HEAD` against this checklist:

For each issue, classify:

- **bug** or **security** (any effort) → fix if safe, else SKIP
- **performance** or **code-smell** (medium+ effort) → fix if safe, else SKIP
- **trivial code-smell** or **test-gap** → fix if trivial, else note

**Correctness**:

- Off-by-one in ranges, indexes, slices
- Unreachable match arms or dead branches after the change
- Error cases silently swallowed
- Edge cases: empty input, single element, `None`, non-UTF-8 paths,
  deeply nested ASTs, pathological tree-sitter input
- Changed behavior for existing callers
- For metrics code: did the formula change? Snapshot tests will catch
  this, but verify with `cargo insta test --review` rather than blindly
  accepting

**Performance**:

- Unnecessary allocations in hot AST-traversal paths
- O(n²) where O(n) is feasible
- Repeated work that should be hoisted out of loops

**Security**:

- Path traversal risk
- `to_string_lossy()` on identifier paths
- Stack overflow on pathological recursion

If any finding requires an unsafe change (API change, data model
change, public-trait change), STOP and report SKIPPED.

### 3e: Validate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
```

If `pre-commit` is installed, run `pre-commit run --all-files`.

If validation fails:

- If the failure is in code you changed, attempt to fix (one retry).
- Otherwise SKIP.

For snapshot test changes: run `cargo insta test --review` and accept or
reject each snapshot deliberately. Do NOT blindly accept.

### 3f: Commit

```bash
CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [ "$CURRENT_BRANCH" != "$AGENT_BRANCH" ]; then
  echo "ERROR: Branch drift. Expected $AGENT_BRANCH, on $CURRENT_BRANCH"
  git checkout -- .
  git reset HEAD
  # Report SKIPPED
fi

git status
git add <file1> <file2> ...
git commit -m "<conventional commit message>"
```

Commit format: `<type>(<scope>): <subject>`, e.g.
`refactor(big-code-analysis): simplify halstead operator classification`.

### 3g: Report result

SUCCESS (branch, commit, files, summary) or SKIPPED (reason).

**END AGENT PROMPT**

---

## Step 4: Integrate successful changes

> **Branch mode**: Skip — integration happens inline in Step 3.

```bash
git checkout improve/<crate-name>
git merge <worktree-branch-name> --no-edit
```

If conflict: `git merge --abort` and log.

After all merges, run the full validation suite. If it fails, bisect:

```bash
git revert -m 1 <merge-commit>
```

---

## Step 5: Update Serena memories

### 5a: Update improvement progress

Write `code-improvement/<crate-name>`:

```
# Code Improvement: <crate-name>
last_run: YYYY-MM-DD
integration_branch: improve/<crate-name>

## Reviewed (clean)
- file.rs: symbols X, Y, Z -- no issues found (YYYY-MM-DD)

## Changed
- file.rs: symbols A, B -- commit <hash> (YYYY-MM-DD)
  - <one-line summary>

## Skipped
- file.rs: symbol C -- reason: <why> (YYYY-MM-DD)
```

### 5b: Update audit state

Update `audit-state-<crate-name>` so improved files are marked re-audited
at depth `full`.

---

## Step 6: Summary

```
## Crate Improvement: <crate-name>
Branch: improve/<crate-name>

### Applied
| # | Commit | Change | Files |

### Skipped
| # | Change Area | Reason |

### Statistics
- Change areas: N
- Applied: N
- Skipped: N
```

Remind the user: "Integration branch `improve/<crate-name>` is ready for
review. Merge to `master` when satisfied."

---

## Guardrails

- Do NOT merge `improve/<crate-name>` into `master`
- Do NOT change public APIs, public traits, or data models
- Do NOT change items re-exported from `src/lib.rs` without authorization
- Do NOT introduce per-language inconsistency in `src/languages/`
- Do NOT touch code outside the target crate
- Do NOT loosen tree-sitter grammar version pins in `Cargo.toml`
- Do NOT re-examine symbols marked clean or changed unless the file has
  new git changes
- Do NOT use `git push --force` or destructive git operations
- Do NOT delete worktrees
- Do NOT blindly accept `insta` snapshot updates — review each one
- If a worktree agent encounters anything it cannot safely resolve, SKIP
