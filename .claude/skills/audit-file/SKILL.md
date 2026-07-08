---
name: audit-file
description: Audit a single Rust source file in the big-code-analysis workspace for logic errors, complexity, bugs, security issues, and code smells. Use when asked to audit or review a specific file.
---

# Audit File

Audit the single Rust source file `$ARGUMENTS` for logic errors, unnecessary
complexity, bugs, security issues, incorrect comments, and code smells.

`$ARGUMENTS` must be a path to a single file — absolute, or relative to the
repository root (e.g., `src/metrics/halstead.rs`,
`big-code-analysis-cli/src/main.rs`).

**Resolve `$ARGUMENTS` to a canonical repo-relative path once at the very
start of the run** and use the resolved value for every subsequent reference
(memory keys, issue titles). Canonicalize with:

```bash
PROJECT_ROOT="$(git rev-parse --show-toplevel)"
FILE_PATH="$(realpath --relative-to="$PROJECT_ROOT" "$PROJECT_ROOT/$ARGUMENTS" 2>/dev/null \
  || realpath --relative-to="$PROJECT_ROOT" "$ARGUMENTS")"
```

**Memory-key sanitization**: replace `/` with `-` in `$FILE_PATH` before
composing Serena memory keys so the key is a single flat token (e.g.,
`audit-state-src-metrics-halstead-rs`, not
`audit-state-src/metrics/halstead.rs`). Apply this rule to every
`audit-state-$FILE_PATH` reference below.

## ABSOLUTE CONSTRAINTS

**This skill is READ-ONLY. It MUST NOT leave any trace on the filesystem.**

- **NEVER commit code.** No `git commit`, no `git add`, no staging. Zero commits.
- **NEVER leave uncommitted files.** No new files, no modified files, no temp files
  in the worktree. If you accidentally create or modify a file, revert it
  immediately with `git checkout -- .`.
- **NEVER modify source files.** Not even "harmless" formatting or comment fixes.
- **NEVER push branches.** The isolation branch is disposable and local-only.
- The ONLY side effects of this skill are: GitHub issues filed, Serena memories
  updated, and terminal output printed.

---

## Before Starting

Check existing open GitHub issues (`gh issue list`) to avoid filing duplicates.
Note any relevant open issues as context, but do NOT let them constrain the audit.

---

## Step 0: Launch isolated agent

**This step is MANDATORY and must be the very first action.**

The audit runs in isolation to guarantee the main working tree is never touched.

### Environment detection

Determine the isolation mode:

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

In branch mode, verify the working tree is completely clean before proceeding:

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

If you are the top-level orchestrator (invoked by the user), immediately launch
a single Agent that executes Steps 1-9. Pass the resolved file path and any
prior context. Do NOT perform any audit work directly.

- **Worktree mode**: Launch the Agent with `isolation: "worktree"`. This creates
  an isolated worktree automatically.
- **Branch mode**: Launch the Agent WITHOUT `isolation: "worktree"`. The agent
  runs in the main project directory (safe because the audit is read-only).

**CRITICAL**: In worktree mode, the Agent tool call MUST include
`isolation: "worktree"` as a required parameter. Double-check before sending.
In branch mode, do NOT include `isolation: "worktree"`.

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

**In worktree mode**: Confirm `PROJECT_ROOT` contains `.claude/worktrees/`.
If not, abort:
"ABORTED: Agent expected worktree isolation but PROJECT_ROOT=\<path\>"

**In branch mode**: Confirm the working tree is clean (`git status --porcelain`
returns empty output). If dirty, abort:
"ABORTED: Branch mode requires a clean working tree."

In worktree mode, all file operations must be within `PROJECT_ROOT`.

**Worktree cleanup**: Worktrees are automatically cleaned up by the Claude Code
runtime. NEVER run `git worktree remove` or `git worktree prune`.

---

## Step 1: Load audit history

Read the Serena memory keyed on `$FILE_PATH` (with `/` replaced by `-`) to
check for prior audit state. Invoke the Serena MCP tool `serena:read_memory`
directly with `memory_name: "audit-state-<sanitized-FILE_PATH>"`. If the Serena MCP
server is not active for this session, call `serena:activate_project` first.

If the memory exists, it contains a single-file coverage record in this format:

```
# Audit State: <file path>
last_audit: YYYY-MM-DD
last_model: <model-id>

## File Coverage
<relative_path> | <depth> | <date> | <findings_count> findings | <model-id>
```

Where `<depth>` is one of: `full`, `partial`, `skimmed`, `none`.

Use the recorded depth to calibrate effort:

| Prior depth | Age | Action |
|-------------|-----|--------|
| `none` | any | Deep audit required |
| `skimmed` | any | Deep audit required |
| `partial` | > 30 days | Audit uncovered areas at `full` depth |
| `partial` | recent | Spot-check; focus on sections not previously covered |
| `full` | < 3 days | Skip unless file changed since last audit (`git log -1 -- <file>`) |
| `full` | > 3 days | Re-audit — code may have drifted |

If the memory does not exist, treat the file as never audited (priority 1, full
depth required).

---

## Step 2: Establish context

### 2a: Identify the containing crate

Walk up from the file to find its `Cargo.toml` and extract the crate name:

```bash
FILE_ABS="$PROJECT_ROOT/$FILE_PATH"
CRATE_DIR="$(dirname "$FILE_ABS")"
while true; do
  [[ -f "$CRATE_DIR/Cargo.toml" ]] && break
  [[ "$CRATE_DIR" == "$PROJECT_ROOT" || "$CRATE_DIR" == "/" ]] && break
  CRATE_DIR="$(dirname "$CRATE_DIR")"
done
CRATE_NAME="$(cargo metadata --format-version 1 --no-deps \
  | jq -r --arg dir "$CRATE_DIR/" \
    '.packages[] | select(.manifest_path | startswith($dir)) | .name' \
  | head -1)"
echo "CRATE_DIR=$CRATE_DIR CRATE_NAME=$CRATE_NAME"
if [[ -z "$CRATE_NAME" ]]; then
  echo "error: could not identify a Cargo.toml containing $FILE_ABS" >&2
  exit 1
fi
```

### 2b: Build baseline

Run build, tests, and clippy scoped to the containing crate to establish
the pre-audit baseline. These commands serve two purposes: (1) confirm the
crate compiles and tests pass so you know what was already broken before the
audit, and (2) surface any existing clippy findings that are in scope.

```bash
cargo build -p "$CRATE_NAME" 2>&1 | tail -5
cargo test  -p "$CRATE_NAME" 2>&1 | tail -20
cargo clippy -p "$CRATE_NAME" -- -W clippy::all 2>&1 | tail -20
```

Record: passing test count, existing warnings, existing clippy findings.
Anything already broken is NOT your problem to fix — note it as context only.

### 2c: Collect code metrics for the file

Use the project's own CLI to measure the target file. The output highlights
cyclomatic / cognitive complexity hotspots, Halstead defect estimates,
function-size outliers, and maintainability-index scores — all of which direct
the audit to the highest-risk functions.

```bash
if cargo build -p big-code-analysis-cli >/dev/null 2>&1; then
  ./target/debug/bca metrics -O json "$FILE_ABS" > /tmp/audit-metrics.json || true
fi
```

Parse the output (when available) and extract per-function metrics:

- **Halstead estimated bugs > 0.5**: These functions have the highest
  probability of containing defects. Audit them first.
- **Cyclomatic complexity > 10**: Complex branching increases the chance of
  missed edge cases. Cross-reference with checklist questions 1-7.
- **Cognitive complexity > 15**: Hard-to-understand code is where incorrect
  comments and swallowed errors hide.
- **Functions > 100 SLOC or > 3 parameters**: Code smell candidates for
  checklist questions 18-23.
- **Maintainability Index < 10**: The entire file warrants deep review.

Carry this data forward to Step 3. When applying the audit checklist, start
with the functions flagged by metrics.

If the metrics tool is unavailable, skip this substep and proceed.

### 2d: Understand the file's role

Read the target file entirely. Then briefly establish:

- What module / public surface does this file implement?
- Does it export items re-exported from `lib.rs`? (If so, public-API
  stability rules apply — see checklist Q28.)
- Does it belong to a family that mirrors other language modules under
  `src/languages/`? (If so, cross-language consistency rules apply —
  see checklist Q27.)
- Does it contain metric computation or AST traversal? (If so, security
  checklist items Q7-Q10 apply with higher weight.)

This context shapes which checklist questions are most relevant in Step 3.

---

## Step 3: Audit checklist

Read the entire file, then apply every applicable question below. **Start
with functions flagged by code metrics** (Step 2c) — these have the highest
defect probability.

Record each finding as:

```
FINDING: <short title>
FILE: <path>:<line range>
CHECKLIST: <question number(s)>
EVIDENCE: <what is wrong and why, with code snippet>
CATEGORY: bug | security | enhancement | documentation
```

Use exactly these four category names — they are the GitHub labels Step 5
will apply. Mapping rules:

- **bug** — incorrect behavior, off-by-one, swallowed errors, broken contracts
- **security** — anything in the Security checklist (Q8-Q13)
- **enhancement** — code smells, refactors, complexity, dependency hygiene
- **documentation** — incorrect or stale comments, doc-tests, missing docs

### Logic and Correctness

1. Does every code path produce the correct output for its documented contract?
2. Are there off-by-one errors in ranges, indexes, or boundary checks?
3. Are there match arms, if-else branches, or rule bodies that are unreachable
   or logically dead?
4. Are error cases handled, or silently swallowed (`continue`, `_ => {}`,
   `Err(_)` arms, ignored `Result`)?
5. Are there false positives on valid inputs, or false negatives on invalid
   inputs?
6. Are all edge cases handled: empty input, max-size input, non-UTF-8 paths,
   missing files, deeply nested ASTs?
7. Is recursion bounded? AST traversal and tree-sitter walks must not blow
   the stack on pathological input.

### Security

8. Can a malicious or pathological input (e.g., a deliberately deep or
   recursive source file) cause stack overflow, infinite loop, or OOM?
9. Is there path traversal risk in any directory-walking or file-loading
   logic?
10. Are symlinks handled safely?
11. Is `to_string_lossy()` safe for all file path handling, or could non-UTF-8
    paths cause silent corruption? Never use `to_string_lossy()` for paths
    used as identifiers (map keys, JSON output, error correlation).
12. Does any feature or API allow user-supplied content to execute arbitrary
    code or make network calls?
13. Are there hardcoded secrets, tokens, or environment-specific paths?

### Incorrect Comments and Documentation

14. Do doc comments on public items accurately describe behavior, parameters,
    return values, and errors?
15. Are inline comments factually correct and not stale (old architectures,
    wrong line numbers, removed features)?
16. Do doc-test examples compile and run correctly?
17. Are there TODO/FIXME/HACK comments indicating known tech debt?

### Code Smells and Unnecessary Complexity

18. Are there duplicated helper functions across test files that should be in
    a shared module?
19. Are there overly verbose patterns that could be simplified (unnecessary
    clones, verbose match arms)?
20. Are there unused imports, functions, or struct fields?
21. Are there functions longer than ~50 lines of logic that should be
    decomposed?
22. Are there hardcoded strings that should be named constants?
23. Are any hardcoded lists a maintenance hazard (e.g., per-language node
    type lists that must stay in sync with the upstream tree-sitter grammar)?

### Dependency and Build Concerns

24. Are all `Cargo.toml` dependencies necessary? Are any runtime dependencies
    only used by a binary or optional feature?
25. Are feature flags on dependencies minimal and appropriate?
26. Is `default-features = false` used where the full default feature set is
    not needed?

### Project-Specific (big-code-analysis)

27. Per-language modules under `src/languages/` deliberately mirror each
    other. Does any change introduce a discrepancy that one language exhibits
    and another does not (different metric formula, different node-type
    handling, different operator/operand classification) without justification?
28. The crate is published on crates.io. Does any change break the public API
    (`lib.rs` re-exports, public traits, `Metrics` / `FuncSpace` / language
    enum shapes) without an intentional version bump?
29. Tree-sitter grammar versions are pinned with `=X.Y.Z` in the root
    `Cargo.toml`. Does any change loosen these pins, or use grammar features
    that were not available in the pinned version?
30. Are there `unwrap()`, `expect()`, `assert!()`, or `panic!()` calls in
    non-test code? `expect()` / `assert!()` are acceptable in tests;
    production code should propagate with `?`. Document any non-test
    `expect()` with the invariant that makes the panic unreachable.

---

## Step 4: Deduplicate and validate

For each finding:

1. Re-read and confirm the evidence is concrete (file + line + reasoning).
2. Cross-check against existing open issues (`gh issue list`). Drop exact duplicates.
3. Confirm the category from Step 3 (`bug`, `security`, `enhancement`, or
   `documentation`) — this is the GitHub label Step 5 will apply.

---

## Step 5: File GitHub issues

### 5a: Ensure category labels exist

Before filing the first issue, ensure every label the audit may use exists in
the repo. `gh issue create` rejects unknown labels. Of the four categories,
`bug`, `documentation`, and `enhancement` already exist on most GitHub repos;
`security` may need to be created on first use:

```bash
ensure_label() {
  local name="$1" color="$2" desc="$3"
  if ! gh label list --limit 200 --json name --jq '.[].name' | grep -qx "$name"; then
    gh label create "$name" --color "$color" --description "$desc"
  fi
}

ensure_label security          "ee0701" "Security-relevant finding"
ensure_label upstream-grammar  "fbca04" "Cannot be fixed locally; needs upstream tree-sitter grammar change"
```

### 5b: Create the issue

For each surviving finding, create one issue. Template:

```markdown
## Summary

<One-sentence description of the problem>

## Location

- `<file path>:<line numbers>`

## Evidence

<Code snippet or reasoning showing the problem>

## Expected Behavior

<What should happen instead>

## Actual Behavior

<What currently happens>

## Impact

<Who is affected and how>
```

Write the body to a temp file and use `gh issue create --body-file`:

```bash
cat > /tmp/issue-body.md <<'EOF'
...body here...
EOF
gh issue create --title "scope: short description" --label "bug" --body-file /tmp/issue-body.md
```

### Upstream-grammar findings

Some findings cannot be fixed in this repository because the bug lives in an
upstream tree-sitter grammar (`tree-sitter-rust`, `tree-sitter-python`,
`tree-sitter-java`, `tree-sitter-typescript`, `tree-sitter-javascript`,
`tree-sitter-kotlin-ng`, etc.) rather than in our wrapper. The grammar version
is pinned in `Cargo.toml`, so the fix requires either a workaround locally,
an upstream patch, or both.

When a finding falls into this category:

1. **Still file the issue** — the problem is real and should be tracked.
2. Add the `upstream-grammar` label alongside the normal category label
   (`bug`, `enhancement`, etc.).
3. In the issue body, add an `## Upstream Grammar` section that names the
   grammar crate and version and explains why a local fix is partial or
   impossible.
4. Do NOT add a fix plan in Step 6 that assumes a local-only fix; instead,
   the fix-plan comment should describe the upstream coordination required
   (file an issue against the grammar repository, document any local
   workaround, plan the version bump that picks up the upstream fix).

Example:

```bash
gh issue create \
  --title "python: function metrics misclassify lambda as expression" \
  --label "bug" \
  --label "upstream-grammar" \
  --body-file /tmp/issue-body.md
```

Signals that an upstream-grammar block applies include:

- Finding depends on a node type or field name produced by the grammar.
- Reproducer requires source that the grammar parses incorrectly.
- Fix would require teaching tree-sitter to emit different nodes.

---

## Step 6: Add fix-plan comments

For each issue filed, add a comment:

```markdown
## General Plan for Fix

### Root Cause

<Why the problem exists>

### Implementation Steps

1. <Smallest change that fixes the problem>
2. <Next step if needed>

### Tests to Add/Update

- <Test 1>
- <Test 2>

### Verification

- [ ] `cargo test -p <crate-name>` passes
- [ ] `cargo clippy -p <crate-name>` clean
- [ ] Manual verification: <specific scenario>
```

```bash
cat > /tmp/fix-plan.md <<'EOF'
...plan here...
EOF
gh issue comment <NUMBER> --body-file /tmp/fix-plan.md
```

---

## Step 7: Summary table

Print to the terminal:

```
| # | Title | Category | File | Issue |
|---|-------|----------|------|-------|
```

Also list any findings dropped as duplicates of existing issues.

---

## Step 8: Save audit state

After completing the audit, persist the coverage state to Serena memory by
invoking the Serena MCP tool `serena:write_memory` directly with
`memory_name: "audit-state-<sanitized-FILE_PATH>"` (i.e., `$FILE_PATH` with
`/` replaced by `-`) and the merged content as `content`.

Build the memory content by **merging** the previous state (from Step 1) with
the current session's coverage. Rules for merging:

- Update depth, date, model, and findings count for this session's audit
- If the file no longer exists in the repository, remove the record
- Preserve any other fields from the previous memory verbatim

Format the memory as:

```
# Audit State: <file path>
last_audit: YYYY-MM-DD
last_model: <model-id>

## File Coverage
<relative_path> | <depth> | <date> | <findings_count> findings | <model-id>
```

Each line: `<relative_path> | <depth> | <date> | <findings_count> findings | <model-id>`

Where:

- `<relative_path>` is relative to the repository root
- `<depth>` is `full`, `partial`, `skimmed`, or `none`
- `<date>` is YYYY-MM-DD of this audit
- `<findings_count>` is the number of issues filed
- `<model-id>` is the AI model that performed the audit

The `last_model` header records the model used in the most recent audit session.

---

## Step 9: Verify clean working tree

**This step is MANDATORY and must be the last action before returning.**

Confirm that the working tree has zero modifications and zero new files:

```bash
git status --porcelain
```

If any output is shown, something went wrong — the audit was supposed to be
read-only.

- For **modifications to tracked files** (`M ` / ` M` lines), revert with
  `git checkout -- <path>` per file. Do NOT use a blanket `git checkout -- .`
  in worktree mode without first verifying every listed path belongs to the
  audit (other agents may share the worktree).
- For **untracked files** (`??` lines), do NOT delete them automatically.
  Untracked output means the audit wrote a file it should not have. Surface
  the path(s) verbatim in the final summary so the user can decide whether
  to keep or remove them. Never run `git clean` from this skill.

Report the anomaly (which files, which step likely produced them) in your
final summary output.

---

## Guardrails

All ABSOLUTE CONSTRAINTS (top of this document) apply. Additionally:

- **NEVER delete worktrees.** Only the Claude Code runtime may do that.
- Do NOT implement fixes. This is audit-only.
- Do NOT file findings without concrete evidence (file + line + reasoning).
- Do NOT file duplicate issues. Always check `gh issue list` first.
- Use `--body-file` for all `gh issue create` and `gh issue comment` calls.
- In worktree mode, all audit work MUST happen inside the isolated worktree.
- In branch mode, audit work runs in the main directory (read-only, verified clean).
