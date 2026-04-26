---
name: batch-fix
description: Fix multiple GitHub issues on an integration branch. Issues touching different crates run in parallel worktrees; issues sharing a crate or affecting cross-language code run sequentially. Each goes through fix, simplify, review, and remediation before merging. Use when asked to fix several issues at once.
---

# Batch Fix GitHub Issues

Fix multiple GitHub issues on a single integration branch. Issues are
classified by affected crate(s) and triaged for quick-win priority and
cross-issue dependencies, then scheduled into waves where issues touching
different crates run in parallel. Quick wins are front-loaded for fast
feedback. Issues sharing a crate, or any issue that touches `src/languages/`
(per-language modules deliberately mirror each other), are serialized to
avoid merge conflicts. Each issue goes through the full pipeline:
investigate, fix, simplify, review, remediate, validate, commit. Successful
fixes are merged to the integration branch. Failures are logged and skipped.

## Arguments

Parse `$ARGUMENTS` as a space-separated list of issue references and flags:
`#42 #57 #63` or `42 57 63` (with or without `#` prefix).

Optional flags:

- `--sequential`: force single-issue waves (no parallel processing). Use
  when issues have cross-crate dependencies that would conflict on merge.

Extract the numeric issue numbers. If no issues are provided, abort with:
"Error: provide at least one issue number. Usage: /batch-fix #42 #57 #63"

---

## Step 0: Validate

### 0a: Validate issues exist

For each issue number, run:

```bash
gh issue view <number> --json number,title,state,labels,body,comments --jq '{number, title, state, labels: [.labels[].name], body, comments}'
```

If any issue does not exist or is already closed, warn the user and remove it
from the list. If no valid open issues remain, abort.

Record each issue's number, title, body, labels, and comments for later steps.
This data is reused in Step 2 (classification) and Step 4 (worktree agent
prompts) -- do not re-fetch.

### 0b: Ensure clean working tree

```bash
git status --porcelain
```

If there are uncommitted changes, abort with:
"Error: working tree is dirty. Please commit or stash your changes before
running /batch-fix."

### 0c: Detect isolation mode

```bash
PROJECT_ROOT="$(git rev-parse --show-toplevel)"
if [[ "$PROJECT_ROOT" == *".claude/worktrees/"* ]]; then
  ISOLATION_MODE="worktree"
else
  ISOLATION_MODE="branch"
fi
```

- **Worktree mode**: Agents are launched with `isolation: "worktree"` and run
  in parallel (existing behavior).
- **Branch mode**: Agents are launched WITHOUT `isolation: "worktree"` and run
  sequentially using feature branches. This preserves Serena LSP compatibility.
  All agents in a wave MUST be processed one at a time (they share the working
  directory).

Record `ISOLATION_MODE` for use in Step 4.

---

## Step 1: Create integration branch

Determine a unique branch name. Try `fix/batch-YYYY-MM-DD` first, then
append a sequence number if it already exists:

```bash
DATE=$(date +%Y-%m-%d)
BRANCH="fix/batch-${DATE}"
SEQ=2
while git rev-parse --verify "$BRANCH" >/dev/null 2>&1; do
  BRANCH="fix/batch-${DATE}-${SEQ}"
  SEQ=$((SEQ + 1))
done
git checkout -b "$BRANCH" main
```

Record the branch name as `INTEGRATION_BRANCH`.

---

## Step 2: Classify and triage issues

For each issue, determine which crate(s) it affects and assess complexity.
This lightweight triage improves wave scheduling without adding API calls
(all data was cached in Step 0a).

### 2a: Crate classification

The workspace crates are:

- `big-code-analysis` — root library (`./src/`, `./tests/`): parsers, AST
  traversal, metric computation, per-language modules
- `big-code-analysis-cli` — CLI binary (`big-code-analysis-cli/`)
- `big-code-analysis-web` — REST API server (`big-code-analysis-web/`)
- `enums` — code-generation helper for language enums (`enums/`, excluded
  from default workspace)

Use these signals in priority order:

1. **Labels**: GitHub labels matching crate names map directly to crates.
2. **Title/body keywords**: Look for crate names, module paths, or
   distinctive terms:
   - "parser", "tree-sitter", "AST", "node", "grammar", "ParserTrait" ->
     `big-code-analysis`
   - "metric", "halstead", "cyclomatic", "cognitive", "loc", "abc", "wmc",
     "nargs", "nom", "npa", "npm", "exit", "mi" -> `big-code-analysis`
   - "checker", "getter", "alterator", "spaces" -> `big-code-analysis`
   - "language X", a specific language name (rust, python, javascript, c,
     cpp, java, kotlin, typescript, etc.), or `src/languages/` path ->
     `big-code-analysis` with `cross_lang: true` (see special case below)
   - "CLI", "command-line", "argument", "output format" (JSON/YAML/TOML/CBOR
     CLI flags) -> `big-code-analysis-cli`
   - "REST", "API", "server", "HTTP", "endpoint", "route" ->
     `big-code-analysis-web`
   - "language enum", "enum generation", "code generation",
     `enums/templates/`, `enums/src/` -> `enums`
3. **Ambiguous**: If the crate cannot be determined from labels or keywords,
   classify as `unknown`.

**Special case — `src/languages/` and cross-language code**: The
per-language modules under `src/languages/` deliberately mirror each other;
a bug in one language often exists in several. Issues that touch this
directory or any metric implementation that walks the AST should be flagged
`cross_lang: true`. These are NOT cross-crate (they all live in
`big-code-analysis`), but they require sequential handling because parallel
agents would each touch sibling language files and conflict on merge.
Treat `cross_lang: true` like `cross_crate: true` for scheduling — schedule
in its own wave.

**Special case — `big-code-analysis` root crate**: The root crate is a
shared dependency of `big-code-analysis-cli` and `big-code-analysis-web`.
Issues that change public API (`lib.rs` re-exports, `ParserTrait`,
`LanguageInfo`, `Metrics`, `FuncSpace`, language enums) are likely to ripple
across crates. Default to `cross_crate: true` for root-crate public-API
issues unless the issue body makes clear the change is internal (e.g., a
self-contained metric bug fix with no public API impact).

### 2b: Quick-win detection

Flag issues as `quick_win: true` if they match **two or more** positive
indicators AND **zero** disqualifiers.

**Positive indicators** (from title, body, and comments):

- References a single specific file path (e.g., `src/metrics/halstead.rs`)
- Contains a clear error message, panic trace, or failing test name
- Mentions a specific function, struct, or enum variant
- Has a "good first issue" or "bug" label
- Body is short (< 500 characters) with a clear reproduction case
- Fix is described in the issue itself (e.g., "should use X instead of Y")

**Disqualifiers** (any one prevents quick-win):

- Requires new public API or architectural changes
- Spans multiple crates explicitly ("change X in big-code-analysis and
  update big-code-analysis-cli")
- Needs external input or design decision ("should we...?", "RFC")
- References missing specifications or unimplemented features
- Requires a tree-sitter grammar version bump
- Has `cross_crate: true` or `cross_lang: true` from Step 2a (cross-language
  fixes that need parity across sibling language modules are rarely true
  quick wins)

### 2c: Cross-issue dependency detection

Scan each issue's title, body, and comments for references to other issues
in the current batch:

- Patterns: `depends on #<N>`, `blocked by #<N>`, `after #<N>`,
  `requires #<N>`
- Bare `#<N>` references do NOT imply dependency -- issues commonly
  cross-reference each other for context without ordering constraints
- Only consider references to issue numbers that are in the current batch

If issue A references issue B, record: `A depends_on B`. This means B must
be scheduled in an earlier wave than A.

**Cycle detection**: If dependencies form a cycle (A->B->C->A), log a
warning and drop all edges in the cycle -- treat those issues as independent.

### 2d: Print classification

For each issue, record:

- `crate`: the primary affected crate name, or `unknown`
- `cross_crate`: `true` if the issue clearly spans multiple crates, `false`
  otherwise
- `cross_lang`: `true` if the issue touches `src/languages/` or otherwise
  requires changes mirrored across language modules
- `quick_win`: `true` if the issue matches the quick-win criteria above
- `depends_on`: list of issue numbers this issue depends on (empty if none)

Print the classification table:

```
## Issue Classification
| Issue | Title | Crate | Cross-crate | Cross-lang | Quick-win | Depends on |
|-------|-------|-------|-------------|------------|-----------|------------|
```

---

## Step 3: Schedule waves

Group issues into processing waves. The goal: maximize parallelism while
respecting crate conflicts, dependencies, and quick-win priority.

### Rules

1. Two issues can run in the same wave only if they affect **different
   crates** (neither is `unknown`, neither is `cross_crate`, neither is
   `cross_lang`, and their crate values differ).
2. `unknown`, `cross_crate`, and `cross_lang` issues are placed in their own
   wave (one at a time) after all classified issues.
3. If `--sequential` was specified, every issue gets its own wave.
4. **Dependency ordering**: If issue A `depends_on` issue B, B must appear
   in an earlier wave than A. Dependencies take precedence over quick-win
   priority.
5. **Quick-win priority**: Within each crate group, quick-win issues are
   scheduled before non-quick-win issues. This front-loads fast fixes into
   early waves, giving rapid feedback and reducing blast radius.
6. User-specified order is preserved as a tiebreaker within each crate group
   (after dependency and quick-win sorting).

### Algorithm

```
classified = issues grouped by crate (excluding unknown/cross_crate/cross_lang)
unclassified = issues marked unknown, cross_crate, or cross_lang
deps = dependency graph from Step 2c

# Sort each crate group: quick_wins first, then user order
for crate in classified:
    classified[crate].sort(key=lambda i: (not i.quick_win, user_order(i)))

# Build waves from classified issues
waves = []
scheduled = set()  # issue numbers already assigned to a wave
remaining = copy of classified (dict of crate -> [issue list])
while remaining is not empty:
    wave = []
    crates_in_wave = set()
    for crate in list(remaining.keys()) sorted by most issues first:
        if crate not in crates_in_wave:
            # Find the first issue whose dependencies are all scheduled
            candidate = None
            for issue in remaining[crate]:
                if all(dep in scheduled for dep in issue.depends_on):
                    candidate = issue
                    break
            if candidate is not None:
                remaining[crate].remove(candidate)
                wave.append(candidate)
                crates_in_wave.add(crate)
    # Clean up empty crate groups after the wave is built
    remaining = {c: issues for c, issues in remaining if issues is not empty}
    # Guard against deadlock from unresolvable dependencies
    if wave is empty and remaining is not empty:
        # Force-schedule one issue to break the deadlock
        crate = first key of remaining
        issue = remaining[crate].pop(0)
        wave = [issue]
        remaining = {c: issues for c, issues in remaining if issues is not empty}
    for issue in wave:
        scheduled.add(issue.number)
    waves.append(wave)

# Append unclassified issues as single-issue waves (respecting dependencies)
pending = list(unclassified)
while pending:
    for issue in pending:
        if all(dep in scheduled for dep in issue.depends_on):
            waves.append([issue])
            scheduled.add(issue.number)
            pending.remove(issue)
            break
    else:
        # Deadlock -- force-schedule the first pending issue
        issue = pending.pop(0)
        waves.append([issue])
        scheduled.add(issue.number)
```

Print the wave plan:

```
## Processing Plan
Isolation: <worktree (parallel) | branch (sequential, Serena-compatible)>
Wave 1 (parallel): #42 (big-code-analysis, quick-win), #57 (big-code-analysis-cli)
Wave 2 (parallel): #63 (big-code-analysis), #71 (big-code-analysis-web, quick-win)
Wave 3 (sequential): #80 (cross-lang, depends on #42)
```

In branch mode, also note: "Agents in later waves see changes from earlier
waves (branch mode advantage)."

If all waves are single-issue, note: "All issues serialized (same crate,
cross-language, or unclassified)."

---

## Step 4: Process waves

For each wave, in order:

### 4a: Spawn agents

Use the issue data (title, body, comments) cached from Step 0a to populate
each agent's prompt.

Pass each agent the full agent prompt (see below) with `<ISSUE_NUMBER>`,
`<ISSUE_TITLE>`, and `<ISSUE_BODY>` substituted.

#### Worktree mode (`ISOLATION_MODE=worktree`)

**CRITICAL**: Every agent MUST be launched with `isolation: "worktree"`.
This is a required parameter on the Agent tool call, not optional. Agents
launched without worktree isolation will modify the main project directory,
corrupting the integration branch. Double-check that every Agent tool call
includes `isolation: "worktree"` before sending.

For a **single-issue wave**: launch one Agent with `isolation: "worktree"`
and `model: "opus"`.

For a **multi-issue wave**: launch ALL agents in a single message block
(parallel tool calls). Each agent gets `isolation: "worktree"` and
`model: "opus"`. Do NOT use `run_in_background` -- wait for all agents in
the wave to complete before proceeding.

**Known limitation**: Worktree agents fork from `INTEGRATION_BRANCH` at the
moment they are spawned. Within a single multi-issue wave, agents do not
see each other's in-flight work — they only see the integration-branch tip
that existed when the wave started. The merge in Step 4b reconciles their
results mechanically. For tightly coupled issues that must build on each
other, use `--sequential` or run them as a single `/fix-issue`.

#### Branch mode (`ISOLATION_MODE=branch`)

All agents in a wave MUST be processed **sequentially** (one at a time).
They share the working directory, so parallel execution is FORBIDDEN.

For each issue in the wave, in order:

1. Create a feature branch from the integration branch:

```bash
BRANCH="fix/issue-${ISSUE_NUMBER}"
if git rev-parse --verify "$BRANCH" >/dev/null 2>&1; then
  git branch -D "$BRANCH"  # stale local branch from prior run
fi
git checkout -b "$BRANCH" "$INTEGRATION_BRANCH"
```

2. Launch ONE Agent with `model: "opus"` (NO `isolation: "worktree"`).

3. On **SUCCESS**:

```bash
git checkout "$INTEGRATION_BRANCH"
git merge "fix/issue-${ISSUE_NUMBER}" --no-edit
git branch -d "fix/issue-${ISSUE_NUMBER}"
```

4. On **FAILED** or **SKIPPED**:

```bash
git checkout -- .
git reset HEAD
git checkout "$INTEGRATION_BRANCH"
git branch -D "fix/issue-${ISSUE_NUMBER}"
```

5. Verify clean state before the next issue. The orchestrator owns
   working-tree cleanup between agents in branch mode (agents are forbidden
   from running `git clean -fd` themselves):

```bash
DIRTY="$(git status --porcelain)"
if [[ -n "$DIRTY" ]]; then
  echo "WARNING: working tree dirty after agent cleanup, cleaning..."
  git checkout -- .
  git clean -fd
fi
```

**Branch mode advantage**: Each feature branch is created from the integration
branch AFTER prior merges, so later agents see earlier fixes. This is an
improvement over worktree mode where agents fork independently from `main`.

### 4b: Process results (after all agents in wave complete)

> **Branch mode**: Skip this step — results are processed inline in Step 4a.

For each agent result in the wave:

The worktree agent returns one of:

- **SUCCESS**: branch name, commit hash, files changed, summary, changelog entry
- **SKIPPED**: reason (issue is invalid, already fixed, or requires no code changes)
- **FAILED**: reason, what was attempted

**On SUCCESS**:

1. Ensure we are on the integration branch:

```bash
git checkout <INTEGRATION_BRANCH>
```

2. Merge the worktree branch:

```bash
git merge <worktree-branch> --no-edit
```

3. If merge conflict occurs, abort and log as FAILED:

```bash
git merge --abort
```

Log: "Issue #N: FAILED -- merge conflict with prior fixes on integration branch"

**On SKIPPED**:

Log the skip reason and continue to the next result. No merge needed.

**On FAILED**:

Log the failure reason and continue to the next result.

### 4c: Wave checkpoint

After merging all successful results from the wave, run a quick compile
check on the integration branch:

```bash
git checkout <INTEGRATION_BRANCH>
cargo check --workspace --all-targets
```

If `cargo check` fails after a multi-issue wave merge, the conflict is
between issues in this wave. Identify the culprit:

1. Record the list of merge commits from this wave (oldest to newest).
2. Reset the integration branch to the state before this wave's merges:

```bash
git reset --hard <pre-wave-commit>
```

3. Re-merge each wave branch one at a time, running `cargo check` after
   each merge. The first merge that causes `cargo check` to fail is the
   culprit.
4. Abort that merge (`git merge --abort`), log the issue as FAILED with
   reason "compilation conflict with parallel fix in same wave".
5. Continue re-merging the remaining (innocent) branches, skipping only
   the culprit.

Proceed to the next wave.

---

## Step 5: Consolidate CHANGELOG

After all waves are processed, collect CHANGELOG entries from all
successful agents and apply them in a single commit on the integration
branch. This avoids merge conflicts on `CHANGELOG.md`.

### 5a: Update CHANGELOG.md

Collect the `CHANGELOG:` entries from all successful agent results. Add them
to the `[Unreleased]` section of `CHANGELOG.md` under the appropriate
subsection (`### Fixed`, `### Added`, `### Changed`). Each entry should
reference the issue number (e.g., `- Fix incorrect Halstead operator
counting in Rust language module (#42)`).

### 5b: Commit changelog updates

```bash
git add CHANGELOG.md
git commit -m "$(cat <<'EOF'
docs(changelog): consolidate entries from batch fix

Update CHANGELOG.md with entries from all successfully merged issue fixes
in this batch.
EOF
)"
```

Skip this step if `CHANGELOG.md` did not change.

### 5c: Collect lessons

Gather every non-empty `LESSON:` value from successful agent results and
hold them for Step 7. Do not edit `docs/development/lessons_learned.md`
directly — the orchestrator only proposes; the user (or `/lessons-learned`)
decides which entries to keep.

---

## Step 6: Final validation

After all waves are processed, if any merges succeeded:

### 6a: Run validation gates on integration branch

```bash
git checkout <INTEGRATION_BRANCH>
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
# If pre-commit is installed:
pre-commit run --all-files
```

### 6b: Handle validation failure

If `cargo fmt --all -- --check` is the only failing gate, fix it in place
rather than bisecting — formatting drift is cheap to repair:

```bash
cargo fmt --all
git add -u
git commit -m "style: cargo fmt --all after batch fix"
```

Then re-run all gates from 6a. Only proceed to bisection below if a
non-fmt gate fails.

If clippy or test gates fail, identify and remove the bad merge:

1. List merge commits on the integration branch since `main`:

```bash
git log main..HEAD --merges --reverse --format="%H %s"
```

2. Test each merge point by checking it out (read-only, no destructive ops):

```bash
# For each merge commit hash, from oldest to newest:
git stash  # save any state
git checkout <merge-commit-hash>
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
```

3. The first commit where validation fails is the culprit. Return to the
   integration branch and reset to just before it:

```bash
git checkout <INTEGRATION_BRANCH>
git reset --hard <parent-of-bad-merge>
```

4. Re-apply subsequent good merges by cherry-picking or re-merging their
   source branches (skip the bad one).

5. Log the culprit issue as FAILED with reason "validation failure after
   merge with other fixes".

6. Re-run the gates to confirm the branch is clean.

If validation fails on the very first merge (no prior good state), reset to
`main`, log that issue as FAILED, and re-merge the remaining successful
branches.

If validation still fails after removing all suspect merges, something is
fundamentally wrong -- abort and report to the user.

**Recovery from interruption**: If the bisection is interrupted mid-sequence
(timeout, context exhaustion), return to the integration branch and restore
any stashed state:

```bash
git checkout <INTEGRATION_BRANCH>
git stash pop  # if stash was used
```

---

## Step 7: Summary

Print a summary table:

```
## Batch Fix Results
Branch: <INTEGRATION_BRANCH>
Isolation: <worktree | branch>
Mode: <parallel|sequential>

### Processing Plan
<wave plan from Step 3>

### Succeeded
| # | Issue | Title | Crate | Quick-win | Wave | Commit | Files Changed |
|---|-------|-------|-------|-----------|------|--------|---------------|

### Skipped
| # | Issue | Title | Reason |
|---|-------|-------|--------|

### Failed
| # | Issue | Title | Reason |
|---|-------|-------|--------|

### Statistics
- Issues attempted: N
- Succeeded: N
- Skipped: N
- Failed: N
- Waves executed: N (M parallel, K sequential)
- Total commits on integration branch: N

### Proposed lessons
| # | From issue | Lesson |
|---|------------|--------|
```

Populate **Proposed lessons** with every non-empty `LESSON:` value
collected in Step 5c. Omit the section if no lessons were proposed.

If any lessons were proposed, end with:
"Run `/lessons-learned` to review and add the proposed lessons to
`docs/development/lessons_learned.md`."

Remind the user: "Integration branch `<INTEGRATION_BRANCH>` is ready for your
review. Merge to main when satisfied, or push to open a PR."

---

## Agent Prompt

**BEGIN AGENT PROMPT**

You are fixing GitHub issue #<ISSUE_NUMBER>: <ISSUE_TITLE>

Issue body:

```
<ISSUE_BODY>
```

You must complete the full fix lifecycle: investigate, implement, simplify,
review, remediate, validate, commit. Do NOT close the GitHub issue -- only
annotate it. The `Fixes #N` commit trailer will close it on merge.

### Setup — Environment Verification (MANDATORY)

Determine your isolation mode:

```bash
PROJECT_ROOT="$(git rev-parse --show-toplevel)"
if [[ "$PROJECT_ROOT" == *".claude/worktrees/"* ]]; then
  ISOLATION_MODE="worktree"
else
  ISOLATION_MODE="branch"
fi
echo "ISOLATION_MODE=$ISOLATION_MODE PROJECT_ROOT=$PROJECT_ROOT"
```

Verify your branch:

```bash
AGENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
echo "AGENT_BRANCH=$AGENT_BRANCH"
```

**HARD GATE**: If `AGENT_BRANCH` is `main`, `master`, or `HEAD` (detached),
abort immediately — do NOT investigate, do NOT edit any files. Return:

```
STATUS: FAILED
REASON: Agent is on disallowed branch. AGENT_BRANCH=<branch>
ATTEMPTED: Setup verification only — no changes made.
```

Record `AGENT_BRANCH` — you will verify it again before committing.

**BRANCH SAFETY**: Do NOT switch branches. Do NOT run `git checkout`,
`git switch`, or `git checkout -b`. All commits must land on this branch.

In worktree mode: ALL file operations must be within `PROJECT_ROOT`.
**Worktree-safety bans apply**: never delete worktrees, never `cd` to the
main repo, never check out a different branch, never write to files outside
your worktree.

In branch mode: the orchestrator has verified a clean repo before launching you.

Try to activate Serena:

```
serena:activate_project project="big-code-analysis"
```

If Serena is unavailable, use text-based tools (Read, Edit, Grep, Glob) and
`rg` / `fd` via Bash. Never use legacy `grep` / `find`.

### Phase 1: Investigate and Fix

Follow the `/fix-issue` workflow:

1. Re-read project conventions: top-level `CLAUDE.md`, `AGENTS.md`, and any
   rule files under `.claude/rules/` if present.
2. If `docs/development/lessons_learned.md` exists, read it and identify
   any lessons relevant to this issue's domain.
3. Investigate the codebase to understand root cause. For tree-sitter
   grammar / language-specific behavior, examine the corresponding module
   under `src/languages/` and confirm whether the bug is in our wrapper or
   upstream in the grammar crate. If the bug is upstream, scope the fix
   accordingly (workaround locally, file an issue against the grammar repo,
   or both — do NOT silently paper over an upstream grammar bug).
4. **Check for the same bug pattern across sibling languages.** The
   `src/languages/` modules deliberately mirror each other; a bug in one
   language's metric implementation often exists in several. If the root
   cause is repeated, fix all instances. Similarly, if metric code under
   `src/metrics/` has the same anti-pattern in multiple metrics, fix all of
   them.
5. **Plan the fix using sequential thinking.** Use the
   `sequential-thinking:sequentialthinking` MCP tool to reason through the
   resolution step by step before writing any code. The sequential thinking
   process MUST:
   - **Start** with `thoughtNumber: 1`, an initial `totalThoughts` estimate
     (typically 5-8), and `nextThoughtNeeded: true`.
   - **Analyze** the root cause — not just the symptom. Trace the
     data/control flow that leads to the bug.
   - **Enumerate approaches** and evaluate trade-offs (simplicity,
     correctness, performance, scope). Remember `big-code-analysis` is
     published on crates.io — public API breaks affect downstream users.
   - **Identify edge cases** — empty inputs, boundary values, deeply nested
     ASTs, non-UTF-8 source, mixed line endings, language-specific quirks
     (preprocessor directives in C/C++, JSX in JavaScript, generics in
     Rust), concurrent access, error paths. Walk through each edge case
     and confirm the proposed fix handles it.
   - **Cross-check against project rules** — if your fix would introduce a
     silent `unwrap_or_default`, an `unwrap()` / `expect()` / `panic!()` in
     non-test code, a `to_string_lossy()` on an identifier path, `unsafe`
     code, or any other anti-pattern from `AGENTS.md`, redesign before
     proceeding.
   - **Verify completeness** — confirm the plan covers implementation,
     tests for **every affected language**, and documentation before
     concluding.
   - **Conclude** with `nextThoughtNeeded: false` and a final plan summary.
   - Adjust `totalThoughts` up or down as understanding evolves. Use
     `isRevision` if earlier reasoning needs correction.
6. **Implement the fix.** Execute the plan from step 5. Before changing any
   public API, run `find_referencing_symbols` (or workspace-wide `rg`) to
   enumerate every call site. If the implementation reveals issues the plan
   missed, revise via sequential thinking before proceeding.
7. Self-review the implementation:
   - Correctness: root cause addressed, not just symptom?
   - Performance: appropriate algorithms and data structures? No O(n²) on
     hot paths (whole-repo analysis, large source files, deep ASTs)?
   - Simplicity: simplest fix that solves the problem?
   - Completeness: edge cases handled? Sibling language modules updated?
     Similar patterns elsewhere fixed?
   - Test coverage: regression tests added for **every** affected language?
     Assertions specific?
   - Conventions: no `unwrap`/`expect`/`panic` outside tests, no `unsafe`,
     newtypes for domain invariants, narrow visibility, borrowing over
     cloning, no `to_string_lossy()` on identifier paths.
8. Fix any issues found in self-review. If fixes were non-trivial, re-review.
9. **Write tests.** Sufficient testing is mandatory before proceeding. At
   minimum:
   - **Unit tests**: for all new or changed public functions. Each edge
     case identified in step 5 should have a corresponding test.
   - **Integration tests**: for end-to-end behavior changes. Run
     `cargo build --workspace` before integration tests so they exercise
     the new binaries — never test against a stale binary.
   - **Per-language coverage**: if the fix touches metric computation, AST
     traversal, or any code under `src/languages/`, exercise **every**
     language affected.
   - **Snapshot tests** (`insta`): if existing snapshots changed, run
     `cargo insta test --review` and accept each diff individually rather
     than blindly accepting all.
   - **Regression check**: `cargo test --workspace --all-features` and
     `cargo clippy --workspace --all-targets -- -D warnings` must pass.
   - Tests must actually assert what they claim — no silent fallbacks
     (`unwrap_or_else` that swallows failures), no missing assertions, no
     coupling to incidental host/filesystem details.
10. If the change affects a compiled binary
    (`big-code-analysis-cli`, `big-code-analysis-web`), run integration
    tests against the NEW binary (`cargo build --workspace && cargo test
    --workspace`).
11. **Update agent-local documentation.** Review and update each of the
    following as applicable:
    - `README.md` — if user-facing behavior, install steps, or supported
      languages changed.
    - `big-code-analysis-book/` — if the change affects documented
      metrics, output formats, or CLI behavior.
    - Crate-level or module-level doc comments (`//!`) — if module intent
      or architecture changed.
    - Avoid hardcoding stale counts in any doc ("all tests passing", not
      "42 tests passing").
12. Do NOT update `CHANGELOG.md` — the orchestrator consolidates
    changelog entries after merging to avoid merge conflicts between
    parallel agents. Include the changelog entry text in your Phase 7
    result instead.

### Phase 2: Simplify

<!-- Adapted from /simplify-rust -- keep in sync -->

Review the diff (`git diff HEAD`) across three dimensions and apply fixes
directly:

**Reuse**:

- Repeated conversion code that should be a `From`/`TryFrom` impl
- Copy-pasted validation or formatting logic across functions
- Manual error mapping chains replaceable by a single `From` impl
- Identical match arms that can be consolidated
- Helper functions that duplicate standard library or crate functionality
- Duplicate logic across sibling language modules that should live in a
  shared helper, trait method, or macro (the project already uses
  `c_langs_macros/`, `src/macros.rs`, and `src/c_macro.rs` for shared
  structure)

**Clarity**:

- `unwrap()` / `expect()` / `panic!()` / `assert!()` in non-test code —
  must use `?` or `Result`/`Option`. `expect("reason")` is acceptable in
  tests; in production, only for provably-unreachable invariants with the
  invariant documented in the message.
- Complex nested `if`/`match` that can be flattened with early returns or
  `?`, or simplified using 2024-edition `let-else` / let-chains
- Boolean flags or stringly-typed APIs that should be enums or newtypes
- `pub` items that should be `pub(crate)` (not used outside the crate)
- Functions longer than ~40 lines that mix unrelated concerns
- `impl` blocks far from their struct/enum definition; methods not grouped
  in constructor → getter → mutation → domain → helper order
- Missing input validation on public functions (empty strings, zero-length
  slices, out-of-range indices)
- Unreachable code using fallback logic instead of `expect("invariant")`
- Redundant type annotations the compiler can infer
- `to_string_lossy()` on paths used as identifiers — use `to_str()` with
  explicit error handling instead. `path.display()` is fine for log output.
- Numeric literals missing underscore separators

**Efficiency**:

- `.clone()` where a borrow suffices
- `String` parameters where `&str` works
- Unnecessary `.collect()` into intermediate `Vec`
- Missing `with_capacity()` for collections built in loops

Do NOT: extract tiny helpers that obscure flow, simplify clear `for` loops
into unreadable iterator chains, or add lifetime annotations the compiler
infers.

Run `cargo check --workspace --all-targets` to verify changes compile after
simplification.

### Phase 3: Review and Remediate

<!-- Adapted from /review -- keep in sync -->

Review the cumulative diff (`git diff HEAD`) against this checklist. Read
each changed file in full for context.

**Correctness**:

- Off-by-one errors in ranges, indexes, slices
- Unreachable match arms or dead branches after the change
- Error cases silently swallowed
- Edge cases: empty input, single element, `None`, non-UTF-8 paths, deeply
  nested ASTs, language-specific quirks
- Changed behavior for existing callers — especially across `lib.rs`
  re-exports, since this is a published library
- Cross-language parity: was the fix mirrored across every sibling language
  module that needed it?

**Performance**:

- Unnecessary allocations in hot paths (whole-repo analysis, large source
  files, deep ASTs)
- O(n²) where O(n) is feasible
- Repeated work that should be hoisted out of loops

**Security**:

- Path traversal risk
- `to_string_lossy()` on identifier paths
- Injection risks (relevant for `big-code-analysis-web`)

**Tests**:

- Every new code path has a corresponding test
- Existing tests still cover their intended scenarios
- Test assertions are specific (not just `is_ok()`)
- Missing negative tests for error paths
- Cross-language fixes have a test in **every** affected language module

For each finding, classify severity and effort:

- **bug** or **security** -> fix immediately
- **performance** or **code-smell** (medium+ effort) -> fix if safe
- **trivial code-smell** or **test-gap** -> fix if trivial, note otherwise

Fix all actionable findings. If fixes were non-trivial, re-review the new
diff. Do NOT proceed with known bugs or security issues.

### Phase 4: Validate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
# If pre-commit is installed:
pre-commit run --all-files
```

If any check fails on code you changed, fix and retry (one attempt).
If it fails again or fails on code you did not change, report as FAILED.

### Phase 5: Commit

Before committing, verify you are still on your agent branch:

```bash
CURRENT_BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [ "$CURRENT_BRANCH" != "$AGENT_BRANCH" ]; then
  echo "ERROR: Branch drift detected. Expected $AGENT_BRANCH, on $CURRENT_BRANCH"
  git checkout -- .
  git reset HEAD
  # Report FAILED
fi
```

Verify what will be staged:

```bash
git status
git diff HEAD --stat
```

Stage only files you intentionally changed (do NOT use `git add -A`):

```bash
git add <file1> <file2> ...
```

Commit with a Conventional Commits message that references the issue for
auto-close on push. The `Fixes #N` trailer goes in the **body**, not the
subject:

```bash
git commit -m "$(cat <<'EOF'
<type>(<scope>): <subject>

<body explaining what and why, 72-char lines>

Fixes #<ISSUE_NUMBER>
EOF
)"
```

Do NOT add `Co-Authored-By` lines unless explicitly requested.

Record the branch name, commit hash, and what changed:

```bash
git rev-parse --abbrev-ref HEAD
git rev-parse --short HEAD
git show --stat HEAD
```

### Phase 6: Annotate GitHub Issue

Update the GitHub issue with research findings and fix details. Do NOT
close the issue -- the `Fixes #N` commit trailer handles closure on merge.

Update BOTH the issue body AND add a comment. Use `--body-file` with a
temp file to avoid quoting issues:

```bash
cat > /tmp/issue-comment-<ISSUE_NUMBER>.md <<'COMMENT_EOF'
## Fix Summary

**Root cause**: <what was wrong and why>

**Changes**:
- <file>: <what changed>
- ...

**Languages affected**: <list, or "n/a">

**Tests**: <what test coverage was added>

**Commit**: <hash> on branch <branch-name>

<any additional notes, follow-up items, or related issues>
COMMENT_EOF
gh issue comment <ISSUE_NUMBER> --body-file /tmp/issue-comment-<ISSUE_NUMBER>.md
```

Also update the issue body to reflect the fix status:

```bash
gh issue view <ISSUE_NUMBER> --json body --jq '.body' > /tmp/issue-body-<ISSUE_NUMBER>.md
cat >> /tmp/issue-body-<ISSUE_NUMBER>.md <<'BODY_EOF'

---

## Resolution

**Status**: Fixed (pending merge)
**Commit**: <hash> on branch <branch-name>
**Root cause**: <brief summary>
BODY_EOF
gh issue edit <ISSUE_NUMBER> --body-file /tmp/issue-body-<ISSUE_NUMBER>.md
```

### Phase 7: Report Result

Return EXACTLY one of:

**SUCCESS**:

```
STATUS: SUCCESS
BRANCH: <branch-name>
COMMIT: <short-hash>
FILES: <number of files changed>
SUMMARY: <one-line description of the fix>
CHANGELOG: <changelog entry text, e.g. "Fixed incorrect Halstead operator counting in Rust language module">
LESSON: <hard-won, globally reusable lesson if any, or "none" — the orchestrator gathers all non-empty values in Step 5c and prints them in Step 7 for the user to review via /lessons-learned>
```

**SKIPPED** (issue is invalid, already fixed, or requires no code changes):

```
STATUS: SKIPPED
REASON: <why no changes were needed>
```

**FAILED**:

```
STATUS: FAILED
REASON: <what went wrong>
ATTEMPTED: <what was tried before failure>
```

On FAILED, ensure no uncommitted changes remain in tracked files:

```bash
git checkout -- .
git reset HEAD
```

Do NOT run `git clean -fd` -- the worktree runtime manages untracked files.

**END AGENT PROMPT**

---

## Guardrails

- Do NOT merge the integration branch into `main` -- leave for the user
- Do NOT close GitHub issues -- the `Fixes #N` trailer handles this on merge
- Do NOT use `git push --force` or any destructive git operations
- Do NOT delete worktrees -- only the Claude Code runtime may do that
- Do NOT skip the review phase -- every fix must be reviewed before commit
- Do NOT bump tree-sitter grammar pins as part of an issue fix; grammar
  bumps are a deliberate, separate change driven by `recreate-grammars.sh`
  or `generate-grammars/`
- If a worktree agent cannot safely resolve an issue, it must report FAILED
- Parallel agents in the same wave MUST touch different crates -- same-crate
  issues, cross-language issues, and cross-crate issues are always
  serialized across waves
- Each worktree agent is fully self-contained -- it does not call
  /fix-issue, /simplify-rust, or /review as skills. The logic is embedded
  in the prompt.
