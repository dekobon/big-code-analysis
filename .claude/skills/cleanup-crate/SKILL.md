---
name: cleanup-crate
description: Remove dead code, unused imports, and unreachable paths from a single Rust crate. Creates an integration branch and applies removals in parallel worktrees.
---

# Cleanup Crate

Run a systematic dead-code cleanup on the Rust crate `$ARGUMENTS`. Creates an
integration branch, analyzes the crate for removable code, applies deletions in
parallel worktrees, and integrates successful changes.

## Arguments

Parse `$ARGUMENTS` as: `<crate-name> [--dry-run] [--aggressive]`

- `<crate-name>` (required): one of `rust-code-analysis`,
  `rust-code-analysis-cli`, `rust-code-analysis-web`
- `--dry-run` (optional): stop after Step 2 (analysis) and print the removal
  candidates without spawning worktree agents
- `--aggressive` (optional): also flag items that require user approval

## What Gets Removed

| Category | Examples |
|----------|----------|
| **Dead code** | Functions, structs, enums, traits, consts, type aliases with zero references |
| **Unused imports** | `use` statements not referenced in the file |
| **Empty blocks** | Empty `impl` blocks, empty modules with no re-exports |
| **Unreachable code** | Match arms after a wildcard, code after unconditional `return`/`break`/`continue` |
| **Redundant code** | Redundant type annotations the compiler infers, redundant `clone()` on `Copy` types |
| **Dead feature gates** | `#[cfg(...)]` blocks where the feature is never enabled |

## What Does NOT Get Removed

- Public API items (`pub` without `(crate)`) — the root crate is published
  on crates.io, so removing public items is a breaking change
- Items re-exported from `src/lib.rs` — these are the stable API surface
- Trait implementations (downstream crates may need them)
- Test utilities and fixtures (in `#[cfg(test)]` or `tests/` directories)
- Code with `#[allow(dead_code)]` annotations (explicit developer intent)
- Items referenced in doc comments, examples, or the mdBook under
  `rust-code-analysis-book/`

## Auto-remove vs Approval Required

**Auto-remove** (applied without prompting):

- `pub(crate)` or private items with zero references
- Unused `use` imports
- Empty `impl` blocks with no methods
- Unreachable code after unconditional control flow

**Approval required** (listed in output, only applied with `--aggressive`):

- `pub` items with zero references (may have external callers — this is a
  published library)
- Items referenced only in tests
- Items behind `#[cfg(...)]` feature gates
- Entire modules or files

## File Scope

- All `.rs` files in the crate's directory
- For the root `rust-code-analysis` crate, scope is `src/` and `tests/`
- For `rust-code-analysis-cli` / `rust-code-analysis-web`, scope is the
  matching subdirectory's `src/` and `tests/`

## Constraints

- **Deletions only**: no refactoring, no API changes, no behavioral changes
- **Do not merge to master**: leave the integration branch for the user
  (note: this repo's default branch is `master`, not `main`)
- **Skip on failure**: if any removal area fails, discard it and log the reason
- **No re-examination**: skip files already reviewed in prior runs
- **Never remove test code**
- **Conservative by default**: when in doubt, leave code alone

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
  echo "Error: crate '$CRATE_NAME' not found in workspace." >&2
  echo "Valid crates: $WORKSPACE_CRATES" >&2
  exit 1
fi
```

### 0b: Ensure clean working tree

```bash
git status --porcelain
```

If there are uncommitted changes, abort:
"Error: working tree is dirty. Please commit or stash your changes before running /cleanup-crate."

### 0c: Create integration branch

```bash
git checkout -b cleanup/<crate-name> master
```

If the branch already exists from a prior partial run, check it out and
continue from where the last run left off (consult Serena memory
`cleanup/<crate-name>`).

### 0d: Detect isolation mode

```bash
PROJECT_ROOT="$(git rev-parse --show-toplevel)"
if [[ "$PROJECT_ROOT" == *".claude/worktrees/"* ]]; then
  ISOLATION_MODE="worktree"
else
  ISOLATION_MODE="branch"
fi
```

- **Worktree mode**: agents run in parallel with `isolation: "worktree"`.
- **Branch mode**: agents run sequentially using feature branches.

---

## Step 1: Load prior state

Read Serena memories via the Serena MCP tool `serena:read_memory`:

1. `audit-state-<crate-name>` — prior audit findings
2. `cleanup/<crate-name>` — prior cleanup runs

If `cleanup/<crate-name>` exists, parse it to identify reviewed-clean,
removed, and skipped files.

---

## Step 2: Analysis

### 2a: Compiler-assisted detection

```bash
cargo clippy -p <crate-name> -- -W dead_code -W unused_imports -W unreachable_code 2>&1
```

Parse the output for file paths, symbol names, and warning types.

### 2b: Symbol-level analysis (Explore agent)

Launch a single Explore agent (`subagent_type: "Explore"`) to scan the crate.
Pass it the prior state from Step 1 and clippy findings from Step 2a.

The agent must:

1. Try to use Serena `get_symbols_overview` on each `.rs` source file. If
   Serena/MCP tools are unavailable, fall back to direct Read.

2. For each symbol, check for references:
   - **With Serena**: `find_referencing_symbols` for each non-trivial symbol.
     - For `pub(crate)` or private items: search within the crate directory.
     - For `pub` items (when `--aggressive`): search the entire workspace.
   - **Without Serena**: use Grep across the crate (`pub(crate)`/private) or
     workspace (`pub`).

3. Per-language modules under `src/languages/` deliberately mirror each
   other. A symbol that *appears* unused in one language may be present so
   the language modules expose the same shape — verify by checking sibling
   modules before flagging.

4. Also check for: unused `use` imports, empty `impl` blocks, unreachable
   code, redundant `clone()` on `Copy` types.

5. Cross-reference with audit state. Skip symbols marked clean or removed
   in the cleanup memory.

6. Classify each finding as **auto-remove** or **approval-required**.

7. Group findings into **logical removal areas** where each:
   - Removes a cohesive set of related dead items
   - Can be described in a single conventional commit message
   - Is independent of other removal areas
   - Without `--aggressive`, contains only auto-remove items

8. Return as structured list:

```
## Removal Area 1: <conventional commit message>
- file.rs: symbol_a [auto-remove] -- zero references
- file.rs: symbol_b [auto-remove] -- only called by symbol_a

## Removal Area 2: <conventional commit message>
- file.rs: symbol_c [approval-required] -- pub, zero workspace references
```

### Dry-run exit point

If `--dry-run` was specified, print the removal areas and stop. Print:
"Dry run complete. N removal areas identified (M auto-remove, K
approval-required). Re-run without --dry-run to apply."

---

## Step 3: Fix cycle (agents)

#### Worktree mode

**CRITICAL**: Every agent MUST be launched with `isolation: "worktree"`.
Launch all removal area agents in parallel.

#### Branch mode

All agents MUST be processed **sequentially**. For each removal area, in
order:

1. Create a feature branch from the integration branch:

```bash
BRANCH="cleanup/${CRATE_NAME}-area-${AREA_NUMBER}"
if git rev-parse --verify "$BRANCH" >/dev/null 2>&1; then
  git branch -D "$BRANCH"
fi
git checkout -b "$BRANCH" "cleanup/${CRATE_NAME}"
```

2. Launch ONE Agent (NO `isolation: "worktree"`).

3. On **SUCCESS**:

```bash
git checkout "cleanup/${CRATE_NAME}"
git merge "$BRANCH" --no-edit
git branch -d "$BRANCH"
```

4. On **SKIPPED**:

```bash
git checkout -- .
git reset HEAD
git checkout "cleanup/${CRATE_NAME}"
git branch -D "$BRANCH"
```

5. Verify clean state before the next area.

#### Agent prompt

**BEGIN AGENT PROMPT**

You are cleaning up dead code in crate `<CRATE>`. Your removal area:

```
<REMOVAL_AREA>
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
abort immediately:

```
SKIPPED: Agent is on disallowed branch. AGENT_BRANCH=<branch>
```

**BRANCH SAFETY**: Do NOT switch branches. Do NOT run `git checkout`,
`git switch`, or `git checkout -b`.

In worktree mode: ALL file operations must be within `PROJECT_ROOT`.

If Serena MCP is available, call `serena:activate_project` first. Otherwise
use Read, Edit, Grep, Glob.

### 3b: Verify before removing

Before deleting any code, confirm the finding is still valid (re-run
`find_referencing_symbols` or Grep). Skip any symbol that now has references.

### 3c: Apply removals

1. Use Edit to delete the entire symbol definition including signature, doc
   comments, and attributes. Do NOT use `replace_symbol_body` — it leaves
   empty stubs.
2. After removing symbols, check for newly-broken references and remove
   one level of cascading dead code. Beyond one level, stop and note in
   the result.
3. Stay within scope — only remove items in your removal area.
4. Never remove items in `#[cfg(test)]` blocks or `tests/` directories.
5. **Per-language consistency**: if you remove a symbol from
   `src/languages/language_<X>.rs`, verify the same symbol is also unused
   in every sibling `language_*.rs`. If a sibling still uses it, do NOT
   remove — flag as inconsistent and SKIP this area.

### 3d: Review removals

Review `git diff HEAD`:

- No false removals (every deleted item truly had zero references)
- No broken references in remaining code
- No test breakage
- Clean boundaries (no dangling commas or orphaned comments)

### 3e: Validate

```bash
cargo check -p <CRATE>
```

If check fails, attempt to fix (one retry); otherwise SKIP.

If check passes, run the full validation:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
```

(If `pre-commit` is installed, also run `pre-commit run --all-files`.)

If validation fails:

- If the failure is in code you changed, attempt to fix it (one retry).
- Otherwise SKIP.

### 3f: Commit

Verify branch:

```bash
CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [ "$CURRENT_BRANCH" != "$AGENT_BRANCH" ]; then
  echo "ERROR: Branch drift. Expected $AGENT_BRANCH, on $CURRENT_BRANCH"
  git checkout -- .
  git reset HEAD
  # Report SKIPPED
fi
```

Stage only intentional changes (do NOT use `git add -A`):

```bash
git add <file1> <file2> ...
git commit -m "<conventional commit message>"
```

Commit message: `<type>(<scope>): <subject>`, typically `refactor` or
`chore`, scope is the crate (e.g., `refactor(rust-code-analysis): remove
unused traversal helpers`).

### 3g: Report result

Return SUCCESS (branch, commit hash, files, removed symbols) or SKIPPED
(reason).

**END AGENT PROMPT**

---

## Step 4: Integrate successful changes

> **Branch mode**: Skip — integration happens inline in Step 3.

For each successful worktree agent:

```bash
git checkout cleanup/<crate-name>
git merge <worktree-branch-name> --no-edit
```

If a merge conflict: `git merge --abort` and log as conflict.

After all merges, run the full validation suite on the integration branch.

---

## Step 5: Update Serena memories

### 5a: Update cleanup progress

Write `cleanup/<crate-name>`:

```
# Cleanup: <crate-name>
last_run: YYYY-MM-DD
integration_branch: cleanup/<crate-name>

## Reviewed (clean)
- file.rs -- no dead code found (YYYY-MM-DD)

## Removed
- file.rs: symbols X, Y, Z -- commit <hash> (YYYY-MM-DD)

## Skipped
- file.rs: symbol C -- reason: <why> (YYYY-MM-DD)

## Approval Required (not applied)
- file.rs: symbol D [pub] -- zero workspace references
```

### 5b: Update audit state

Update `audit-state-<crate-name>` so cleaned files are marked re-reviewed
at depth `full`.

---

## Step 6: Summary

```
## Crate Cleanup: <crate-name>
Branch: cleanup/<crate-name>

### Applied
| # | Commit | Removed | Files |

### Skipped
| # | Removal Area | Reason |

### Approval Required (not applied)
| # | Symbol | Reason |

### Statistics
- Removal areas: N
- Applied: N
- Skipped: N
- Approval required: N
- Total symbols removed: N
```

Remind the user: "Integration branch `cleanup/<crate-name>` is ready for
review. Merge to `master` when satisfied."

If approval-required items exist, suggest `--aggressive`.

---

## Guardrails

- Do NOT merge `cleanup/<crate-name>` into `master`
- Do NOT remove public API items unless `--aggressive` is set
- Do NOT remove items re-exported from `src/lib.rs`
- Do NOT remove trait implementations
- Do NOT remove items with `#[allow(dead_code)]`
- Do NOT remove test utilities or fixtures
- Do NOT touch code outside the target crate
- Do NOT re-examine files marked clean unless they have new git changes
- Do NOT use `git push --force` or destructive git operations
- Do NOT delete worktrees
- If a removal would create cross-language inconsistency in `src/languages/`,
  SKIP it
- When in doubt, leave it alone
