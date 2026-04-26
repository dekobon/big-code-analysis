---
name: audit-naming
description: Audit naming quality in a crate or directory for misleading, inconsistent, or unclear names. Use when asked to audit or review naming.
---

# Audit Naming Quality

Audit naming quality in `$ARGUMENTS` for misleading, inconsistent, or unclear
names across the Rust source under audit. Distinct from `audit` (logic,
security, complexity) -- this focuses exclusively on whether names tell the
truth.

If `$ARGUMENTS` is empty, default to `big-code-analysis` — the root library
crate. Other crates in this workspace are `big-code-analysis-cli` and
`big-code-analysis-web`. `$ARGUMENTS` may also be a directory path; in that
case the audit is scoped to that directory.

**Resolve `$ARGUMENTS` once at the very start of the run** and use the
resolved value for every subsequent reference (memory keys, `cargo -p`, issue
titles).

**Memory-key sanitization**: when `$ARGUMENTS` is a directory path (e.g.,
`src/languages`), replace `/` with `-` before composing memory keys so the
key is a single flat token (e.g., `naming-audit-state-src-languages`, not
`naming-audit-state-src/languages`). Apply this sanitization to every
memory-key reference below.

## ABSOLUTE CONSTRAINTS

**This skill is READ-ONLY. It MUST NOT leave any trace on the filesystem.**

- **NEVER commit code.** No `git commit`, no `git add`, no staging.
- **NEVER leave uncommitted files.** No new files, no modified files, no temp
  files in the worktree. If you accidentally create or modify a file, revert
  it immediately with `git checkout -- .`.
- **NEVER modify source files.** Not even "harmless" formatting or comment fixes.
- **NEVER push branches.**
- The ONLY side effects of this skill are: GitHub issues filed, Serena
  memories updated, and terminal output printed.

---

## Step 0: Launch isolated agent

**This step is MANDATORY and must be the very first action.**

The audit runs in isolation to guarantee the main working tree is never touched.

### Environment detection

```bash
PROJECT_ROOT="$(git rev-parse --show-toplevel)"
if [[ "$PROJECT_ROOT" == *".claude/worktrees/"* ]]; then
  ISOLATION_MODE="worktree"
else
  ISOLATION_MODE="branch"
fi
```

- **Worktree mode**: You are already inside a worktree. Keep all existing
  behavior (agent launched with `isolation: "worktree"`).
- **Branch mode**: You are in the main project directory. Agents run without
  worktree isolation. Serena LSP works correctly in this mode.

### Branch mode prerequisites

```bash
if [[ "$ISOLATION_MODE" == "branch" ]]; then
  DIRTY="$(git status --porcelain)"
  if [[ -n "$DIRTY" ]]; then
    echo "Error: branch mode requires a clean repository (no uncommitted or untracked files)." >&2
    echo "$DIRTY" >&2
    exit 1
  fi
fi
```

### Launch the agent

If you are the top-level orchestrator, immediately launch a single Agent that
executes Steps 1-8. Do NOT perform any audit work directly.

- **Worktree mode**: Launch with `isolation: "worktree"`.
- **Branch mode**: Launch WITHOUT `isolation: "worktree"`.

Sub-agents inherit context (read-only) and do NOT need their own
`isolation: "worktree"`.

### Agent verification (MANDATORY)

Every agent must verify its environment before doing any work:

```bash
PROJECT_ROOT="$(git rev-parse --show-toplevel)"
if [[ "$PROJECT_ROOT" == *".claude/worktrees/"* ]]; then
  ISOLATION_MODE="worktree"
else
  ISOLATION_MODE="branch"
fi
echo "ISOLATION_MODE=$ISOLATION_MODE PROJECT_ROOT=$PROJECT_ROOT"
```

In worktree mode: confirm `PROJECT_ROOT` contains `.claude/worktrees/`.
In branch mode: confirm `git status --porcelain` is empty.

**Worktree cleanup**: NEVER run `git worktree remove` or `git worktree prune`.

---

## Step 1: Load audit history

Read the Serena memory `naming-audit-state-$ARGUMENTS` via the Serena MCP tool
`serena:read_memory`. If Serena is not active, call `serena:activate_project`
first.

If the memory exists, parse the per-file coverage table:

```
# Naming Audit State: $ARGUMENTS
last_audit: YYYY-MM-DD
last_model: <model-id>

## File Coverage
<relative_path> | <depth> | <date> | <findings_count> findings | <model-id>
```

Use it to **prioritize files** in Step 2:

| Priority | Condition | Action |
|----------|-----------|--------|
| 1 (highest) | `none` -- never audited | Deep audit required |
| 2 | `skimmed` -- quick scan only | Deep audit required |
| 3 | `partial` AND older than 30 days | Audit uncovered areas |
| 4 | `partial` AND recent | Spot-check, focus on new changes |
| 5 (lowest) | `full` AND recent (< 3 days) | Skip unless file changed |

New files default to priority 1. If the memory does not exist, treat all
files as priority 1.

---

## Step 2: Discover scope and run automated pre-checks

### Check for duplicate issues

```bash
gh issue list --label naming
```

Note any relevant open issues as context but do NOT let them constrain the
audit.

### Resolve the target

Try `cargo metadata` first. If no crate matches, check if `$ARGUMENTS` is a
valid directory.

```bash
CRATE_NAME="$ARGUMENTS"
SCOPE_DIR=$(cargo metadata --format-version 1 --no-deps \
  | jq -r --arg name "$CRATE_NAME" \
    '.packages[] | select(.name == $name) | .manifest_path' \
  | xargs dirname 2>/dev/null)

if [[ -z "$SCOPE_DIR" ]]; then
  CRATE_NAME=""
  if [[ -d "$ARGUMENTS" ]]; then
    SCOPE_DIR="$ARGUMENTS"
  else
    echo "ERROR: '$ARGUMENTS' is not a known crate name or valid directory." >&2
    exit 1
  fi
fi
```

`CRATE_NAME` is set when the target is a crate (used for clippy), empty when
the target is a bare directory.

### Enumerate files

This project is Rust-only. Find all `.rs` files in scope:

```bash
FD=$(command -v fd 2>/dev/null || command -v fdfind 2>/dev/null)
"$FD" -e rs . "$SCOPE_DIR"
```

### Run automated naming lints first

```bash
if [[ -n "$CRATE_NAME" ]]; then
  cargo clippy -p "$CRATE_NAME" -- \
    -W clippy::module_name_repetitions \
    -W clippy::enum_variant_names \
    -W clippy::struct_field_names \
    -W clippy::similar_names \
    -W clippy::disallowed_names \
    -W clippy::wrong_self_convention 2>&1 | tail -30
fi
```

Note the output as context. The manual audit in Step 3 **MUST skip issues
already caught by clippy** and focus on semantic naming quality clippy cannot
detect.

### Map file groups

| Group | Contents |
|-------|----------|
| A — Library core | `src/lib.rs`, `src/languages/`, `src/metrics/`, `src/output/`, `src/parser.rs`, `src/checker.rs`, `src/getter.rs`, `src/alterator.rs`, `src/spaces.rs`, `src/node.rs`, `src/traits.rs`, etc. |
| B — Tests | `tests/` directory and `#[cfg(test)]` modules |
| C — Workspace binaries | `big-code-analysis-cli/src/`, `big-code-analysis-web/src/` (when those are the audit target) |

Per-language modules under `src/languages/` deliberately mirror each other —
naming inconsistency *between* languages (same concept named differently)
is a primary target for this audit.

**Use the priority table from Step 1** to order files. Read each file
**entirely** before applying checklist questions.

**Track depth**: `full` (all checks applied), `partial` (key APIs only), or
`skimmed` (quick scan).

---

## Step 3: Naming audit checklist

Apply every applicable question. Record each finding as:

```
FINDING: <short title>
FILE: <path>:<line range>
CHECKLIST: <check ID(s)> (e.g., U2, R1)
CURRENT NAME: <the problematic name>
EVIDENCE: <why the name misleads, with code context>
SUGGESTED NAME: <proposed alternative>
SEVERITY: misleading | inconsistent | unclear | convention
```

Severity definitions:

- **misleading**: Name actively implies wrong behavior, type, or purpose
- **inconsistent**: Same concept named differently across the codebase
- **unclear**: Name requires reading the implementation to understand
- **convention**: Violates Rust API Guidelines

### Universal Checks

| ID | Check |
|----|-------|
| U1 | Name reveals intent without requiring a comment |
| U2 | Name does not mislead about behavior/type/purpose |
| U3 | No noise words for meaningless distinctions (`Data` vs `Info`) |
| U4 | Same concept uses same word everywhere (no `fetch`/`get`/`retrieve` mix) |
| U5 | Different concepts use different words |
| U6 | Name length proportional to scope |
| U7 | Boolean names are positive (no double negation like `not_disabled`) |
| U8 | No unexplained abbreviations (domain-standard ones like `ast`, `loc`, `mi` are fine) |
| U9 | Naming patterns applied consistently across similar entities |
| U10 | No linguistic antipatterns (name/behavior mismatch) |

### Rust-Specific Checks

| ID | Check |
|----|-------|
| R1 | Conversion prefixes match semantics: `as_` (free borrow), `to_` (expensive copy), `into_` (consuming), `from_` (constructor) |
| R2 | Getters omit `get_` prefix (Rust API Guidelines C-GETTER) |
| R3 | `into_*` consumes self; `as_*` borrows (signature matches prefix) |
| R4 | Iterator methods follow `iter`/`iter_mut`/`into_iter` convention |
| R5 | Word order follows stdlib patterns (`ParseError` not `ErrorParse`) |
| R6 | `is_*`/`has_*` methods return `bool` |
| R7 | Struct/enum field names match their types semantically (no `count: String`) |
| R8 | Plural names hold collections; singular names hold single values |

### Project-Specific Checks (big-code-analysis)

| ID | Check |
|----|-------|
| P1 | Cross-language naming parity: when `language_rust.rs`, `language_python.rs`, etc. expose the same concept, the names match (no `is_func` in one and `is_function` in another) |
| P2 | Metric names follow the canonical metric vocabulary (`abc`, `cognitive`, `cyclomatic`, `halstead`, `loc`, `mi`, `nargs`, `nom`, `npa`, `npm`, `wmc`) — abbreviations are standard, expanded forms or alternates are inconsistent |
| P3 | Tree-sitter node-type strings used as identifiers match the upstream grammar's exact spelling (deviations are bugs, not style) |

---

## Step 4: Group findings into issues

**Key difference from `audit`**: Group findings into coherent tickets:

1. Group by **file** if multiple findings in the same file
2. Group by **theme** if the same naming pattern repeats across files
3. Group by **severity** only as a last resort

Each grouped issue should have 1-5 findings; never more than ~8 (split if
needed).

For each finding:

1. Re-read and confirm the evidence is concrete (file + line + reasoning).
2. Cross-check against existing open issues (`gh issue list --label naming`).
   Drop exact duplicates.
3. Verify the finding is NOT already caught by clippy in Step 2.

---

## Step 5: File GitHub issues

### 5a: Ensure label exists

```bash
ensure_label() {
  local name="$1" color="$2" desc="$3"
  if ! gh label list --limit 200 --json name --jq '.[].name' | grep -qx "$name"; then
    gh label create "$name" --color "$color" --description "$desc"
  fi
}
ensure_label naming "c2e0c6" "Naming quality finding"
```

### 5b: Issue template

```markdown
## Summary

<One-sentence theme of the naming issues in this group>

## Findings

### 1. `<current_name>` in `<file>:<lines>`

**Check**: <ID> | **Severity**: <level>

<Evidence: what the name implies vs what the code does>

**Suggested**: `<proposed_name>`

### 2. ...

## References

- Checks: <list of check IDs referenced>
- Sources: <Rust API Guidelines or relevant standards>
```

```bash
cat > /tmp/issue-body.md <<'EOF'
...body here...
EOF
gh issue create --title "$SCOPE: <theme>" --label "naming" --body-file /tmp/issue-body.md
```

For each issue filed, add a fix-plan comment:

```markdown
## Fix Plan

### Changes

1. Rename `<old>` to `<new>` in `<file>`
2. ...

### Verification

- [ ] `cargo build --workspace` compiles after renames
- [ ] `cargo test --workspace --all-features` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] No remaining references to old name(s)
```

```bash
cat > /tmp/fix-plan.md <<'EOF'
...plan here...
EOF
gh issue comment <NUMBER> --body-file /tmp/fix-plan.md
```

---

## Step 6: Summary table

```
## Naming Audit: $ARGUMENTS

| # | Issue | Theme | Findings | Highest Severity |
|---|-------|-------|----------|------------------|

Total: X findings in Y issues
Skipped: Z findings already caught by clippy
```

---

## Step 7: Save audit state

Persist coverage state to Serena memory `naming-audit-state-$ARGUMENTS` via
`serena:write_memory`.

**Merge** the previous state (Step 1) with current session coverage:

- Files audited this session: update depth, date, model, findings count
- Files NOT audited this session: preserve existing record
- Files no longer in scope: remove from table
- New files discovered but not audited: add with depth `none`

Format:

```
# Naming Audit State: $ARGUMENTS
last_audit: YYYY-MM-DD
last_model: <model-id>

## File Coverage
src/lib.rs | full | 2026-04-26 | 3 findings | claude-opus-4-7
src/languages/language_rust.rs | partial | 2026-04-26 | 1 finding | claude-opus-4-7
src/metrics/halstead.rs | none | - | - | -
```

---

## Step 8: Verify clean working tree

```bash
git status --porcelain
```

If any output:

- For modifications to tracked files: `git checkout -- <path>` per file. Do
  NOT blanket-revert in worktree mode.
- For untracked files (`??`): do NOT delete automatically; surface paths in
  the summary so the user can decide.

Report any anomaly in the final summary.

---

## Guardrails

All ABSOLUTE CONSTRAINTS apply. Additionally:

- **NEVER delete worktrees.**
- Do NOT implement fixes. This is audit-only.
- Do NOT file findings without concrete evidence.
- Do NOT file duplicate issues.
- Do NOT file findings already caught by clippy in Step 2.
- Use `--body-file` for all `gh issue create` and `gh issue comment` calls.
