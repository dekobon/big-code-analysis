---
name: doc-author
description: Author or revise a human-consumed prose document (README, STABILITY, docs under docs/ and the mdBook under big-code-analysis-book/src/): clarify audience and purpose, outline, draft against docs/conventions/documentation.md, then revise in multiple focused passes (structure, completeness, line edit, proofread) that hunt the three doc failure modes (missing external links, insufficient background, hollow sounds-smart sentences), get an independent fresh-context review, and verify markdown lint. Includes a review mode that critiques an existing doc without editing it. Use when asked to write, revise, tighten, or de-slop a documentation or Markdown prose file. Not for code, diffs, doc comments, changelog entries, or lessons-learned (those have their own workflows).
---

# Doc-author skill

A workflow for writing or revising a human-consumed prose document
(`README.md`, `STABILITY.md`, anything under `docs/`, and the mdBook
source under `big-code-analysis-book/src/`).

`docs/conventions/documentation.md` is the source of truth; this skill
is an editor helper that applies those rules. If the skill and the
convention disagree, the convention wins. For lessons-learned entries
use the `lessons-learned` skill; for changelog entries follow the
`## Changelog` section of `docs/conventions/documentation.md` (plus the
stability-contract rules in `AGENTS.md`). This skill does neither.

## Arguments

A target doc path and an optional mode hint: `write`, `revise`, or
`review`. If no mode is given, infer it from whether the path exists
and from the verb the user used (a missing path means `write`;
"review" or "critique" means `review`; otherwise `revise`).

## Workflow

### Step 1: Load the rules

Read the `## Prose documents` block of
`docs/conventions/documentation.md`, plus its Markdown, No stale
counts, and Audience separation sections. Then read `.rumdl.toml`. The
convention is self-contained, so no runtime fetch is needed; the skill
works offline.

### Step 2: Clarify audience and purpose

State, in one or two sentences, who the reader is, the single question
the doc answers, and the background it assumes.

For a new doc, apply the "Whether a doc should exist" table first. If
the topic already has a home, extend that doc rather than adding a
file. If the target is a point-in-time artifact (a review rebuttal, a
status snapshot, an email draft), recommend a commit message or
archive home and stop.

If the target is generated (it carries a "generated, do not edit"
banner or a generator reference, like the man pages rendered by
`cargo xtask`), stop and report instead of editing. Fix the generator,
not the output.

### Step 3: Outline

Draft a heading outline: an orientation opening (what this is, who it
is for, where it fits), then content ordered by decreasing reader
relevance. Drop any heading that would be empty or that restates the
title. Show the outline before drafting anything long.

### Step 4: Draft

Write present-tense, active-voice prose. As you write: link every
external tool, standard, or paper on first mention; state assumed
background before relying on it. Use approximate language for scale,
never invented counts.

### Step 5: Revise in passes (self-edit)

Drafting and editing are separate acts; do not polish while drafting.
This is where most of the quality comes from. Revise in discrete
passes, one objective each, and **re-read the saved file from disk at
the start of every pass** rather than editing from memory: your
context is biased toward what you meant to write, and the file on disk
is what the reader gets.

In each pass, **first list the problems you find with line numbers,
then fix them.** Separating detection from fixing finds more than
editing inline as you read.

1. **Structure pass.** Does the opening orient the reader? Is content
   ordered by reader relevance? Cut or merge empty and title-restating
   sections.
2. **Completeness pass.** Every external tool, standard, or paper is
   linked on first mention; assumed background is stated before it is
   relied on.
3. **Line-edit pass.** Hunt the bans listed under "Plain, honest
   sentences" in the convention (em dashes, superlatives, marketing
   adjectives, AI sentence structures, hedging, noun stacks), plus
   non-parallel lists and wrong-term usage (update vs. upgrade). Prefer
   cutting over rephrasing. Length targets are suggestions, not
   failures.
4. **Proofread pass.** Run the self-review checklist in
   `docs/conventions/documentation.md`.

Repeat any pass whose fixes were substantial. **Stop when a full
re-read turns up no new findings**: convergence ends the loop, not a
fixed count. If the passes stop converging, report what remains rather
than looping, and never claim a pass you did not actually perform.

### Step 6: Independent review (fresh eyes)

Self-review in the same context rubber-stamps your own choices, so end
with a fresh-context critique, scaled to the size of the change:

- **Preferred:** dispatch a subagent (via the Agent tool, the pattern
  the `review` skill uses for code) to review the saved file with no
  knowledge of how it was written. Give it the `## Prose documents`
  rules and ask for a findings list (file and line, the rule broken, a
  concrete rewrite) and no edits.
- **Fallback** (no subagent available): run this skill again in
  `review` mode, in a fresh turn, against the saved file.

Apply the findings through the Step 5 fix discipline. Do not re-run the
reviewer to confirm your fixes landed — a second pass over prose you
just corrected finds nothing. Re-review only when the findings forced a
structural rewrite, because the rewritten sections are new prose nobody
has read yet.

### Step 7: Verify lint

Ensure `rumdl` is present (install it if `make markdown-lint` reports
the tool is missing), then `rm -rf .rumdl_cache && make
markdown-lint`. Fix any findings. Confirm internal links and anchors
resolve and external links are well-formed URLs.

### Step 8: Report and stop

Do NOT commit. Summarize the passes you ran, the independent-review
findings, the external links added, the sentences cut, and the
structural moves. The user decides commit timing.

## Review mode

When the mode is `review`, run steps 1, 2, and 5 as a critique only:
produce a findings list with file and line references and concrete
rewrites, and make no edits unless the user then asks. This mode is
also the fallback reviewer for Step 6 when no subagent is available. It
is the most effective path for de-slopping an existing doc.

## Guardrails

- **The convention wins.** `docs/conventions/documentation.md` is the
  source of truth. Propose convention edits rather than inventing
  skill-only rules.
- **No automatic commits.** Edit or leave the file; the user commits.
- **Passes are real, not theater.** Re-read the file from disk on each
  pass, list findings before fixing, and stop on convergence. Never
  announce a pass you did not perform or pad to hit a number.
- **Prose only.** Never edit code doc comments, source comments,
  released changelog history, or lessons-learned entries. Editing a
  code block inside a prose doc is fine; editing the source it came
  from is not. Do not edit generated docs (man pages, the rendered
  mdBook under `big-code-analysis-book/book/`). Stay within this repo.
- **No invented facts.** Never fabricate counts, benchmarks, versions,
  or citations. If a canonical URL is unknown, leave a `TODO(link)`
  marker rather than guessing.
- **Don't fight the linter.** Respect `.rumdl.toml` (the 120 ceiling,
  the allowed HTML set, the disabled rules). Keep prose wrapped near
  100.
- **De-slop cuts words, not facts.** If a hollow sentence carries a
  caveat, keep the caveat in plain words.
- **Lint from a clean cache.** Always `rm -rf .rumdl_cache` before
  trusting `make markdown-lint`.
