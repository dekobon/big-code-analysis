---
name: lessons-learned
description: Review project activity and draft entries for lessons_learned.md. Use when asked to update or review lessons learned.
---

# Lessons Learned Workflow

Review recent project activity (issues, commits, changelog) to identify
hard-won lessons, evaluate them against a strict quality bar, and draft
entries for `docs/development/lessons_learned.md`.

If `docs/development/lessons_learned.md` does not yet exist, create the
directory and seed the file with a short header before the first entry —
do not begin appending into a missing file.

**Argument**: `$ARGUMENTS` — empty for the full workflow, or hint text to
narrow the search (e.g., `"tree-sitter parser"`, `"halstead metric"`).

---

## Step 1: Establish Boundary

Determine the time boundary for evidence gathering — everything since the
last update to the lessons file:

```bash
git log -1 --format=%aI -- docs/development/lessons_learned.md
```

If the file has never been modified beyond its initial creation (no
substantive history), fall back to the repository's first commit date:

```bash
git log --reverse --format=%aI | head -1
```

Record the boundary date as `$BOUNDARY`.

---

## Step 2: Read Current Coverage

Read `docs/development/lessons_learned.md` in full. Record:

1. **Highest lesson number** — new entries will start at N+1
2. **Each lesson's title and domain** — used for overlap detection in Step 5
3. **Issue numbers already cited** — avoid re-proposing lessons from known issues

This step is mandatory. Overlap detection in Steps 5 and 6 depends on it.

If the file is brand new and contains no lessons yet, record the highest
lesson number as 0 and proceed.

---

## Step 3: Gather Evidence

Collect evidence from four sources. When `$ARGUMENTS` contains hint text,
add the hint as an additional search keyword to narrow results.

### 3a: Closed issues since boundary

```bash
gh issue list --state closed --search "closed:>$BOUNDARY" --limit 100 \
  --json number,title,body,labels,closedAt
```

Triage: scan titles and bodies for hard-lesson signals:

- "root cause", "debugging", "turns out", "subtle", "silent"
- "security", "regression", "broke", "workaround", "misunderstood"

Deep-dive on comments only for candidates that show signals:

```bash
gh issue view <N> --json comments
```

### 3b: Git commits since boundary

```bash
git log --since="$BOUNDARY" --format="%H %s" -- src/ tests/ big-code-analysis-cli/ big-code-analysis-web/ docs/
```

Look for:

- Fix commits with substantial diffs (not trivial typos)
- Refactors that changed approach after initial implementation
- Multi-issue commits (suggest systemic pattern)
- Recurring tree-sitter / grammar version-bump fallout (these tend to
  produce reusable lessons about the upgrade process)

### 3c: CHANGELOG entries since boundary

If `CHANGELOG.md` exists, read it and identify entries added since the
boundary date. Focus on entries under "Fixed" and "Changed" sections — these
are most likely to contain lesson-worthy material.

### 3d: Documentation changes (skip when hint provided)

```bash
git log --since="$BOUNDARY" --name-only --format="" -- docs/ big-code-analysis-book/ README.md CLAUDE.md
```

Look for new or substantially updated documentation that may reflect
hard-won understanding.

---

## Step 4: Deep Investigation

For items from Step 3 showing hard-lesson signals:

1. Read full issue threads and linked PRs
2. Examine diffs: `git show <commit>`
3. Use Serena LSP tools or code reading for surrounding context
4. Look for pattern repetition — did the same mistake happen more than once
   (e.g., across multiple language modules)?

Record each potential lesson:

- **Source reference**: issue number(s), commit hash(es)
- **One-line summary**: what went wrong or what was learned
- **Evidence strength**: strong (cost real debugging time), moderate
  (non-obvious but caught quickly), weak (obvious in retrospect)

---

## Step 5: Candidate Evaluation

This is the core quality gate. The bar:

> **"Genuinely hard (cost real debugging time or caused real bugs) AND
> important (likely to recur)."**

Present candidates as a ranked batch:

```
### Candidate N: <summary>
- Source: #<issue>, <commit>
- Quality: QUALIFIES / DOES NOT QUALIFY
- Overlap: None / Related to lesson #N (explain distinction or overlap)
- Reasoning: <why it meets or fails the quality bar>
```

### Handling non-qualifying candidates

For each candidate that does not qualify, suggest an alternative home with
case-by-case reasoning:

| Signal | Alternative Home |
|--------|-----------------|
| One-off debugging trick | Code comment at the relevant site |
| Architectural decision | Module-level `//!` doc or design doc |
| Testing pattern | Test file comment, or a testing rule under `.claude/rules/` if present |
| Project convention | `CLAUDE.md`, `README.md`, or `.claude/rules/` |
| Already covered by existing lesson | Merge into existing lesson #N |
| Too specific to one issue | Issue comment or PR description |

Push-back language must be explicit. **"No candidates qualify" is a valid
success state.** Do not force entries to justify the workflow.

**Wait for the user to select which candidates to draft.** Do not proceed
to Step 6 without user confirmation.

---

## Step 6: Draft Entries

For each user-selected candidate, draft an entry matching the established
format in `docs/development/lessons_learned.md`:

1. `## N. <Pithy Principle Name>` — use the next sequential number
2. Opening paragraph: general lesson statement (not issue-specific)
3. Bold sub-examples with issue/commit references (e.g., `**Description
   of specific instance** (#42, abc1234).`)
4. Closing `**Lesson:**` paragraph summarizing the actionable takeaway
5. Horizontal rule (`---`) separator after the entry

### Overlap handling

- If a candidate overlaps with an existing lesson, propose one of:
  - **Merge**: add a new bold sub-example to the existing lesson
  - **Cross-reference**: add a note like "Related to lesson #N, which
    covers X; this lesson addresses the distinct Y aspect"
  - **Skip**: if the overlap is too close, recommend not adding it
- Do NOT modify existing lessons without explicit user approval

### Placement

- **Default**: append as the next numbered entry (after the current
  highest number)
- If the user requests insertion at a specific position, warn:
  > "Inserting here will change lesson numbers. Other skills (e.g.,
  > `fix-issue`, `audit-tests`) reference lessons by number. Consider
  > grepping `.claude/skills/` for affected references before
  > applying."

Show the complete draft in context (the markdown that would be appended).
**Wait for user approval before applying.**

---

## Step 7: Apply and Stage

After user approval:

1. Append approved entries to `docs/development/lessons_learned.md`
2. Run `markdownlint-cli2 docs/development/lessons_learned.md` to ensure
   the file passes lint
3. Stage the file: `git add docs/development/lessons_learned.md`
4. Do NOT commit — staging only

Post-completion notes to display:

- "Changes staged but not committed."
- "Other skills (`fix-issue`, `audit-tests`, `review`) may reference
  lessons by number. If new lessons changed the numbering of existing
  entries, grep `.claude/skills/` for stale references."

---

## Guardrails

- **Quality bar is non-negotiable**: do not draft entries that fail the
  "genuinely hard AND likely to recur" test. The file should stay small
  and actionable.
- **No automatic commits**: stage only. The user decides when and how to
  commit.
- **Preserve existing lessons**: no modifications to existing entries
  without explicit user approval. This includes rewording, renumbering,
  and reordering.
- **Append by default**: warn about renumbering risks if insertion is
  requested.
- **Complete evidence trail**: every drafted lesson must cite at least
  one issue number or commit hash. No lessons from vibes.
- **No forced lessons**: "no candidates qualify" is a valid and expected
  outcome. Do not lower the bar to produce output.
