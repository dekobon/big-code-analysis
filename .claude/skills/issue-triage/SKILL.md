---
name: issue-triage
description: Fetch open GitHub issues and generate a triage report with quick wins and recommended groupings. Use when asked to triage issues.
---

# Issue Triage

Generate a triage report for open GitHub issues. Read-only — no issue
creation, modification, or closing.

**Argument**: `$ARGUMENTS`

- Empty → all qualifying open issues
- `<crate-name>` → filter to issues whose title starts with that crate name
  (one of `rust-code-analysis`, `rust-code-analysis-cli`,
  `rust-code-analysis-web`)
- `<language>` → filter to issues whose title or body references a
  specific language module (e.g., `python`, `rust`, `java`)

---

## Step 1: Fetch issues

### 1a: Ensure label vocabulary exists

`gh issue list --label <missing>` returns empty rather than erroring, so
querying a non-existent label silently filters it out. Provision the
labels this skill expects up front:

```bash
ensure_label() {
  local name="$1" color="$2" desc="$3"
  if ! gh label list --limit 200 --json name --jq '.[].name' | grep -qx "$name"; then
    gh label create "$name" --color "$color" --description "$desc"
  fi
}
ensure_label refactor         "fbca04" "Refactor without behavior change"
ensure_label security         "ee0701" "Security-relevant finding"
ensure_label upstream-grammar "fbca04" "Cannot be fixed locally; needs upstream tree-sitter grammar change"
```

`bug`, `enhancement`, and `documentation` already exist on most GitHub
repos; `low-priority` is provisioned by `issue-plan`.

### 1b: Query and merge

The `--label` flag only supports AND, so query each label separately and
merge with `jq`:

```bash
(
  gh issue list --state open --label bug           --limit 200 --json number,title,labels,body
  gh issue list --state open --label enhancement   --limit 200 --json number,title,labels,body
  gh issue list --state open --label refactor      --limit 200 --json number,title,labels,body
  gh issue list --state open --label documentation --limit 200 --json number,title,labels,body
  gh issue list --state open --label security      --limit 200 --json number,title,labels,body
  gh issue list --state open --label upstream-grammar --limit 200 --json number,title,labels,body
) | jq -s '
  add
  | unique_by(.number)
  | [ .[] | select([ .labels[].name ] | any(. == "low-priority") | not) ]
'
```

If `$ARGUMENTS` is a crate name, further filter to titles matching
`<crate-name>:` or `<crate-name>(` (case-insensitive prefix). If
`$ARGUMENTS` is a language name, filter to issues whose title or body
mentions that language.

Save the filtered list. Record the count — this is "Issues analyzed: N".

---

## Step 2: Read every issue body

Titles can mislead. For each issue, ensure you have the full body text
(already fetched in Step 1 via `--json ... body`).

Skim each body for:

- Scope (which crate, which language module, which metric)
- Complexity signals (cross-language work, public-API impact,
  tree-sitter grammar involvement)
- Dependencies on other issues
- Root cause hints

---

## Step 3: Classify quick wins

For each issue, evaluate against these criteria.

### Positive indicators (need 3+ to qualify)

1. Touches a single crate only
2. Narrow scope — one function, one module, or one `language_*.rs`
3. Obvious fix implied by the issue description
4. Likely < 50 lines changed
5. No new abstractions needed (no new traits, modules, or public APIs)
6. No public API change (does not touch `lib.rs` re-exports, public
   traits, or shapes of `Metrics` / `FuncSpace` / language enums)
7. No tree-sitter grammar version bump required

### Disqualifiers (any one eliminates)

1. Requires a new module, trait, or public type
2. Touches multiple crates
3. Modifies the public API of the published library
4. Requires bumping a tree-sitter grammar version pin (`=X.Y.Z`)
5. Requires the same change in many `language_*.rs` files (cross-language
   sweep) — this is mechanical but voluminous, not a quick win
6. Requires regenerating snapshots (`*.snap`) across many languages
7. Bundles multiple distinct problems
8. Has the `upstream-grammar` label (cannot be fixed locally without
   coordination with an upstream tree-sitter project)

**Err on the side of NOT classifying as a quick win.** When in doubt,
put it in Remaining Issues.

---

## Step 4: Identify groupings

Two-pass approach.

### Pass 1 — By crate

Group mechanically by the crate name prefix in the title (e.g.,
`rust-code-analysis:`, `rust-code-analysis-cli:`). A group requires 2+
issues.

### Pass 2 — By theme

Look for cross-cutting connections:

- **Same metric**: multiple issues touching `halstead`, `cognitive`,
  `cyclomatic`, etc. across different language modules
- **Same language**: multiple issues touching one `language_*.rs`
- **Same upstream grammar**: issues blocked on the same tree-sitter
  crate version
- **Shared traversal/AST plumbing**: issues that all touch
  `parser.rs`, `node.rs`, `spaces.rs`, `checker.rs`, or `getter.rs`
- **Cross-language parity**: independent reports of the same defect in
  different languages — fixing one is a template for fixing all
- **Risk coupling**: changing code for A could break the area B touches

A theme group requires 2+ issues. Do NOT force groupings — if issues
are genuinely independent, leave them in Remaining Issues.

---

## Step 5: Produce the report

Every fetched issue MUST appear in exactly one section: Quick Wins, a
Group row, or Remaining Issues.

```markdown
## Issue Triage Report

**Scope**: all issues | <crate-name> | <language>
**Issues analyzed**: N
**Date**: YYYY-MM-DD

### Quick Wins

| # | Title | Why it's a quick win |
|---|-------|----------------------|
| #XX | ... | ... |

### Recommended Groupings

| Group | Issues | Rationale |
|-------|--------|-----------|
| ... | #XX, #YY | ... |

### Remaining Issues

| # | Title | Notes |
|---|-------|-------|
| #ZZ | ... | ... |

---

**Quick wins**: #X, #Y, #Z
**Grouped**: [#A, #B], [#C, #D], #E
```

The last two lines are copyable summary lines for sprint planning.

If a section has no entries, keep the header and write "None" in the
table body.

---

## Guardrails

- **Read-only**: Do NOT create, modify, close, or comment on any issues.
- **Must read body**: Never classify based on title alone.
- **No forced groupings**: Ungrouped issues go in Remaining Issues.
- **Complete coverage**: Every fetched issue appears in exactly one
  section.
- **Excluded issues**: Issues with the `low-priority` label never
  appear.
