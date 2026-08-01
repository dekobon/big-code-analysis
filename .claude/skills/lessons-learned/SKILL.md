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

- **Source reference**: issue number(s) and the PR that landed the
  change — never a commit hash (see
  [Cite issues and PRs](#cite-issues-and-prs-never-commit-hashes)).
  Resolve a commit to its PR with:

  ```bash
  gh api "repos/{owner}/{repo}/commits/<sha>/pulls" \
    --jq '.[] | "PR #\(.number)"'
  ```
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
- Source: #<issue>, PR #<pr>
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
| Testing pattern | [`.claude/rules/testing.md`](../../rules/testing.md) |
| Per-language dispatch / grammar shape | [`.claude/rules/grammar-dispatch.md`](../../rules/grammar-dispatch.md) |
| Formatting or gate quirk | [`.claude/rules/formatting.md`](../../rules/formatting.md) |
| Project convention or standing policy | `AGENTS.md` |
| Already covered by existing lesson | Merge into existing lesson #N |
| Too specific to one issue | Issue comment or PR description |

**A rule beats a lesson whenever the takeaway is a standing
obligation.** A lesson is evidence that a bug class is real; a rule is
the instruction you follow without re-reading the evidence. If the
`**Lesson:**` paragraph you are about to write is an imperative that
applies every time someone touches a given surface — not a warning
about one recurring trap — it belongs in `.claude/rules/` or
`AGENTS.md`, and the lessons entry should shrink to the incident plus a
pointer. Do not restate a policy that already lives in `AGENTS.md`:
duplicated policy drifts, and the copy in the lessons file is the one
nothing enforces.

Push-back language must be explicit. **"No candidates qualify" is a valid
success state.** Do not force entries to justify the workflow.

**Wait for the user to select which candidates to draft.** Do not proceed
to Step 6 without user confirmation.

---

## Step 6: Draft Entries

For each user-selected candidate, draft an entry matching the established
format in `docs/development/lessons_learned.md`:

1. `## N. <Pithy Principle Name>` — use the next sequential number
2. `**Lesson:**` paragraph **first**: the actionable takeaway, stated as
   an instruction. This is what a reader who greps the file needs, so it
   leads; the narrative that justifies it follows.
3. One paragraph on the mechanism — why the failure is invisible, what
   makes it recur. Not issue-specific.
4. Bold sub-examples citing the issue and the PR that landed the fix
   (e.g., `**Description of specific instance** (#42, PR #1085).`).
   Never a commit hash — see below.
5. Horizontal rule (`---`) separator after the entry
6. Add the one-line summary to the index at the top of the file.

**Budget the length by mechanism, not by a flat cap.** A single-mechanism
entry fits in **45 lines**: the `**Lesson:**` paragraph, the mechanism,
one sub-example. Each *additional distinct* mechanism buys about 15 more
— so a two-mechanism entry runs to ~60 and a merged entry carrying two
lessons' worth (15, 24, 59) legitimately reaches ~55. Past **75 lines**,
stop: that is either two lessons, or one lesson plus a design doc, and
`docs/development/` is the home for the latter (see
`output_name_normalization_design.md`).

The clause that does the work is **one sub-example per distinct
mechanism**. A second instance of a mechanism the entry already covers —
the same bug in another language, the same trap in another crate — is
one clause naming the issue, not a paragraph. Most over-long entries are
long for that reason rather than because they document too much.

Check with:

```bash
awk '/^## [0-9]+\./{if(n)print len" "n; n=$0; len=0; next} {if(n)len++}
     END{if(n)print len" "n}' docs/development/lessons_learned.md | sort -rn | head
```

The file is reference material read under time pressure, and
`CLAUDE.md`'s length rule applies to it: no filler, no summary that
restates the section above it.

### Cite issues and PRs, never commit hashes

Issue and PR numbers are assigned once and never change. A commit hash
is not stable until the change reaches the default branch, and this
repository rebase-merges, so every hash on a branch is rewritten when
the PR lands. An entry drafted alongside the change it describes — the
normal case, because that is when the lesson is fresh — therefore cites
hashes that are orphaned the moment it merges. Nothing detects this: the
text still renders, and the hash still looks like a hash.

It has already happened twice, both times caught by chance rather than
by a gate, during PR #1085:

- Lesson 81 cited `3fd01c70`, an orphaned pre-amend copy of the #1056
  commit.
- Lesson 80's sub-example, written in that same PR, cited two branch
  hashes that the PR's own rebase merge rewrote.

Existing entries that cite hashes are left alone; do not retrofit them,
and do not treat this as licence to edit entries you are not otherwise
touching. If you are already editing an entry and its hash turns out to
be unreachable, check with `git merge-base --is-ancestor <sha> main` and
replace it with the PR number then.

A hash is acceptable in one case: quoting a commit that is already on
the default branch and has no PR (a direct push). Verify reachability
before citing it.

### Overlap handling

Default to **merge**. A new entry is justified only when it names a
failure *mechanism* no existing entry names — not merely a new language,
crate, or subsystem exhibiting a mechanism already documented.

- **Merge** (preferred): add a bold sub-example to the existing lesson,
  and extend its `**Lesson:**` paragraph if the new instance sharpens
  the instruction. Most candidates land here.
- **New entry**: only for a genuinely new mechanism. Then it stands on
  its own — do **not** append a paragraph explaining how it differs from
  lesson #N. Those paragraphs are throat-clearing that costs every
  future reader, and needing one is a signal the candidate should have
  been a merge. A bare `(cf. lesson #N)` is the most cross-referencing
  an entry should carry.
- **Skip**: if the overlap is too close to justify either.
- Do NOT modify existing lessons without explicit user approval.

### De-duplication pass

Before drafting, check whether the file has drifted. Run:

```bash
awk '/^## [0-9]+\./{if(n)print len" "n; n=$0; len=0; next} {if(n)len++}
     END{if(n)print len" "n}' docs/development/lessons_learned.md | sort -rn | head
rg -c 'Related to lesson|Distinct from lesson|cousin of lesson|Refines lesson' \
   docs/development/lessons_learned.md
```

Entries over the length budget in Step 6 — especially any running past
75 lines — and any surviving "how I differ from lesson #N" paragraph
are candidates to trim or merge. Report them
alongside the new candidates in Step 5 and let the user decide — do not
edit them unprompted.

**Renumbering is a breaking change and is never in scope.** Roughly
sixty files cite lessons by number, including production source
(`src/checker/*.rs`, `src/macros/kind_sets.rs`), the cross-language
parity tests, `AGENTS.md`, `CONTRIBUTING.md`, the book, and the
`Makefile`; #2, #11, #19, #4 and #6 carry the most references. Verify
before assuming a number is free:

```bash
rg -n -i 'lessons?[^.\n]{0,80}' --glob '!docs/development/lessons_learned.md' \
   --glob '!*.po' --glob '!CHANGELOG.md'
```

When two entries merge, the surviving text goes under the **lower**
number and the higher number stays in place as a one-line redirect, so
every existing citation still resolves.

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
2. Run `rumdl check docs/development/lessons_learned.md` to ensure
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
- **Prefer a rule to a lesson** when the takeaway is a standing
  obligation, and prefer a merge to a new entry when the mechanism
  already has a home. The file reached 87 entries and 4,700 lines partly
  because neither preference was written down.
- **Respect the length budget** (Step 6) and update the index in the
  same edit.
- **No automatic commits**: stage only. The user decides when and how to
  commit.
- **Preserve existing lessons**: no modifications to existing entries
  without explicit user approval. This includes rewording, renumbering,
  and reordering.
- **Append by default**: warn about renumbering risks if insertion is
  requested.
- **Complete evidence trail**: every drafted lesson must cite at least
  one issue or PR number. No lessons from vibes, and no commit hashes —
  a rebase merge invalidates them.
- **No forced lessons**: "no candidates qualify" is a valid and expected
  outcome. Do not lower the bar to produce output.
