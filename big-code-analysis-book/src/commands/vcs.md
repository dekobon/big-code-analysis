# Change-history (VCS) metrics

`bca vcs` ranks files by **change-history risk** — signals derived from
version-control history rather than the source AST. It is the project's
first language-agnostic, non-AST metric family. The
goal is to surface the files most likely to harbour bugs or
vulnerabilities, using the signals the empirical defect- and
vulnerability-prediction literature most consistently backs.

A single history walk runs once per invocation (never per file) and
produces per-file signals over two configurable windows — a **long**
window (default `12mo` ≈ 365 days) and a **recent** window (default
`90d`).

## Quick start

```console
$ bca vcs --paths src --top 20
Change-history risk (long window 365d, recent 90d, formula v2)
 RANK      RISK  COMMITS r/l    CHURN r/l  AUTHORS  FILE
    1       7.2        68/68  11634/11634        1  src/metrics/cyclomatic.rs
    2       6.9        68/68    7299/7299        1  src/metrics/npa.rs
    ...
```

With no `--format`, a human-readable ranked table is printed. Pass
`--format markdown|html` for a rendered report page, or
`--format json|yaml|toml|cbor|csv` for structured output. Unlike
`bca metrics` / `bca ops` (whose `--output` is a *directory* of per-file
emissions), a change-history report is a single whole-repo document, so
`bca vcs --output <file>` writes **one file** (CBOR, being binary,
requires `--output`). The global `--paths` / `--include` / `--exclude` /
`--no-ignore` filters are reused to pick which tracked files to report.

`bca vcs` errors clearly when run outside a git working tree.

### Rendered report page

```bash
bca vcs --top 50 --format html --output vcs.html
bca vcs --top 50 --format markdown --output vcs.md
```

`--format html` produces a self-contained, sortable page styled exactly
like `bca report html` (click any column header to re-sort); `--format
markdown` produces the same ranked table as GitHub-Flavored Markdown.
Both render every signal column (the complete, sortable view of the same
data the structured formats carry). The column set is defined once and
shared by both renderers, so they cannot drift.

To fold the ranking into the aggregated quality report instead of a
standalone page, pass [`bca report --vcs`](report.md), which appends a
"Change-history risk" section to `report markdown` / `report html`.

## Signals

| Field | Type | Description |
|---|---|---|
| `commits_long` / `commits_recent` | u32 | Distinct commits touching the file in each window |
| `churn_long` / `churn_recent` | u64 | Σ(added + deleted) lines in each window |
| `authors_long` / `authors_recent` | u32 | Distinct canonical author identities in each window |
| `ownership_top_share` | f64 ∈ [0,1] | Share of edits attributable to the top author (lower = more diluted) |
| `burst` | f64 ∈ [0,1] | `commits_recent / commits_long` |
| `bug_fix_commits` | u32 | Long-window commits whose message matches a bug-fix keyword |
| `security_fix_commits` | u32 | Long-window commits matching security keywords (`CVE-####`, `security`, `vuln`, `exploit`, `sanitize`, …) |
| `revert_commits` | u32 | Long-window commits whose subject is a revert / rollback |
| `age_days` | u32 | Days since the file's first in-window commit (capped at the long window) |
| `last_modified_days` | u32 | Days since the file's most recent in-window commit |
| `change_entropy_long` / `change_entropy_recent` | f64 | Change entropy in bits per window (see below) |
| `cochange_entropy_long` / `cochange_entropy_recent` | f64 | Co-change graph entropy in bits per window (see below) |
| `risk_score` | f64 | Composite, formula-versioned (see below) — **ordinal, not cardinal** |
| `hotspot_score` | f64? | `complexity × churn_recent`; present only when AST metrics are computed alongside |
| `risk_score_version` / `vcs_schema_version` | u32 | Forward-compatibility version stamps |

Author identities are canonicalised through the repository `.mailmap`
and counted by lowercased email; `Co-authored-by:` trailers add
participants. Bot identities (`dependabot[bot]`, `renovate[bot]`,
`github-actions[bot]`, …) are excluded by default. Binary files and
symlinks are skipped; an untracked file has no record at all (distinct
from a tracked file with zero in-window activity).

## Change & co-change entropy

Two process-entropy signals (added in `risk_score_version` 2) capture
*how* a file changes, not just how much:

- **Change entropy** (Hassan, 2009 — _Predicting Faults Using the
  Complexity of Code Changes_). For each commit, the Shannon entropy (in
  bits) of its churn distribution across the files it touched measures
  how *scattered* that change was: a one-file commit is 0; a commit
  spreading churn evenly across *n* files approaches log₂(*n*). Each file
  is then credited its churn share `pᵢ·H` of every commit it took part in
  (Hassan's History Complexity Metric). Higher = the file is repeatedly
  caught up in diffuse, cross-cutting changes. File-level change entropy
  reaches Pearson 0.54 with defects on Apache projects.
- **Co-change graph entropy** (arXiv 2504.18511, 2025). Files that change
  in the same commit are joined by a weighted edge (weight = number of
  shared commits). A file's co-change entropy is the Shannon entropy of
  its edge-weight distribution: low when it always co-changes with the
  same partner, high when its changes ripple across many different files.
  Combined with change entropy it improved AUROC in 82.5% of cases over
  the v1 signal set on eight Apache projects.

Both are reported per window. A `0.0` is **computed**, not missing: the
file only ever changed alone (no co-change neighbours, or single-file
commits with zero change entropy). Bulk-import commits touching more than
1000 files are excluded from the co-change graph — its edge count grows
O(width²) — but still contribute their O(width) change entropy.

## Composite risk score

The default **weighted** formula is a log-scaled weighted sum with
categorical multiplicative bumps:

```text
recency_churn  = ln(1 + churn_recent)
recency_count  = ln(1 + commits_recent)
long_count     = ln(1 + commits_long)
long_churn     = ln(1 + churn_long)
author_factor  = ln(1 + authors_long)
dilution       = (1 - ownership_top_share).clamp(0, 1)
fix_factor     = ln(1 + bug_fix_commits + 2 * security_fix_commits)
size_factor    = ln(1 + sloc)^2 / 100              // tiny tie-breaker
entropy_factor = 0.10 * change_entropy_recent + 0.05 * cochange_entropy_recent
new_file_bonus = 0.15 if age_days < recent_window_days else 0
dev_bonus      = 0.35 if authors_long >= 9 else 0.15 if authors_long >= 6 else 0

base = 0.30 * recency_churn
     + 0.25 * recency_count
     + 0.15 * long_count
     + 0.15 * author_factor * (1 + dilution)
     + 0.10 * fix_factor
     + 0.05 * long_churn
     + entropy_factor
     + size_factor

risk_score = base * (1 + dev_bonus + new_file_bonus)
```

The term weights are grounded in the literature: recent churn and
commit frequency carry the highest weight (Nagappan & Ball relative
churn; just-in-time defect prediction; Firefox `NumChanges` PD 86); the
author factor is scaled by ownership dilution (Avelino DoA /
truck-factor; Bird et al.); the categorical developer-count bumps encode
the RHEL4 finding that files touched by ≥9 developers were ~16× more
likely to harbour a vulnerability; security fixes are double-weighted
(Sentence-Level VFC studies; PySecDB); and the recent-window change- and
co-change-entropy terms enter additively (Hassan 2009; arXiv 2504.18511).
The full derivation lives in `src/vcs/score.rs`.

The score is **ordinal**: only relative ranks have meaning. Any change
to the formula bumps `risk_score_version` (now `2`); the recent entropy
pair also joins the `--risk-formula percentile` blend.

`--risk-formula percentile` is an alternative: each signal is re-ranked
to its percentile within the analyzed set, then averaged — the
literature recommends relative triggers over hard thresholds for
cross-project robustness.

## Flags

| Flag | Default | Meaning |
|---|---|---|
| `--long-window <DUR>` | `12mo` | Long window (`12mo`, `2y`, `8w`, `365d`, ISO 8601 `P1Y`) |
| `--recent-window <DUR>` | `90d` | Recent window |
| `--top <N>` | `50` | Show only the top N (`0` = all) |
| `--ref <REF>` | `HEAD` | Revision to analyse |
| `--full-history` | off | Walk the full DAG (default: first-parent only) |
| `--include-merges` | off | Include merge commits |
| `--no-follow-renames` | off | Stop following renames (default: follow) |
| `--no-exclude-bots` / `--bot-pattern <RE>` | exclude | Bot-author filtering |
| `--as-of <WHEN>` | wall clock | Reference "now" (RFC 3339 / `@unix` / git date) for reproducible snapshots |
| `--risk-formula {weighted\|percentile}` | `weighted` | Composite formula |
| `--emit-author-details` | off | Emit SHA-256-hashed canonical author IDs |
| `--include-deleted` | off | Also rank files deleted at the target ref |

## In `bca metrics`

Pass `bca metrics --vcs` to attach a `vcs` block (plus a `hotspot_score`
computed from the file's cyclomatic sum) to each file's `metrics`:

```console
$ bca metrics --vcs --paths src/parser.rs --format json
{ "name": "src/parser.rs",
  "metrics": { "cyclomatic": { ... },
    "vcs": { "commits_long": 15, "churn_recent": 211,
             "risk_score": 3.7, "hotspot_score": 7596.0, ... } } }
```

`bca metrics --vcs` uses the default windows and weighted formula; for
window / formula tuning use `bca vcs`.

### Per-function attribution

`bca metrics --vcs-per-function` (which implies `--vcs`) additionally
attaches a `vcs` block to every nested function, method, and class space.
It blames each file once with `git blame` and buckets the surviving lines
into the AST function spans, so you can rank the risky *function* inside a
risky file:

```console
$ bca metrics --vcs-per-function --paths src/parser.rs --format json
{ "name": "src/parser.rs",
  "metrics": { "vcs": { "risk_score": 3.7, ... } },   // file-level block
  "spaces": [
    { "name": "parse", "kind": "function",
      "metrics": { "vcs": { "commits_long": 4, "churn_recent": 12,
                            "risk_score": 2.1, "hotspot_score": 144.0 } } } ] }
```

The per-function block is a **current-blame snapshot** and is *not*
directly comparable to the file-level block: its `churn` counts surviving
lines whose last touch falls inside the window (not historical
added+deleted churn), and ownership is credited per touching commit. A
function nobody has changed within the window reports zero counts. Lines
whose last touch predates the long window contribute to the function's
size but to none of the windowed counts.

**Limitations.** Blame follows file renames (so edits under a former path
still attribute), but attributes a line *moved between functions* to its
current position only. A function split into two has no record of its
pre-split identity, and a deleted-then-recreated function attributes to
the recreating commits. If a file cannot be blamed — untracked, or the
rare `gix-blame` failure on pathologically repetitive content — its
per-function blocks are simply omitted while the file-level block (and the
AST metrics) still emit.

## Just-in-time (commit-level) scoring

Where everything above ranks *files* at a ref, `bca vcs jit <commit>`
scores a single *commit* for defect-induction risk — the unit a CI gate
reviews at check-in. It is a static, rule-based scorer (no trained model,
so nothing drifts as the project ages), with the feature groups and signs
taken from the just-in-time defect-prediction literature: Kamei et al.,
[*A Large-Scale Empirical Study of Just-in-Time Quality
Assurance*](https://doi.org/10.1109/TSE.2013.2386), IEEE TSE 2013, with
the open replications [Commit
Guru](https://doi.org/10.1145/2786805.2803183) (FSE 2015) and McIntosh &
Kamei, [*Are Fix-Inducing Changes a Moving
Target?*](https://doi.org/10.1109/TSE.2017.2693980) (IEEE TSE 2018).

```console
$ bca vcs jit HEAD --pretty
{
  "jit_schema_version": 1,
  "jit_score_version": 1,
  "score": 4.40,
  "commit": { "id": "5176d3e…", "parent_count": 1, "is_merge": false,
              "purpose": { "is_fix": true, "is_security_fix": false,
                           "is_revert": false } },
  "features": {
    "size":       { "lines_added": 942, "lines_deleted": 60,
                    "files_touched": 19, "hunks": 78 },
    "diffusion":  { "subsystems": 5, "directories": 8, "entropy": 3.48 },
    "history":    { "prior_changes": 275, "prior_distinct_authors": 1,
                    "prior_bug_fix_commits": 237,
                    "prior_security_fix_commits": 21,
                    "file_risk_max": 10.97, "file_risk_mean": 3.87,
                    "new_files": 2 },
    "experience": { "author_prior_commits": 962,
                    "author_recent_commits": 962 }
  },
  "contributions": { "size": 2.74, "diffusion": 0.97, "history": 1.57,
                     "purpose": 0.15, "experience": -1.03 }
}
```

The five feature groups, and how each moves the score:

| Group | Features | Direction |
|---|---|---|
| **Size** | lines added / deleted, files touched, diff hunks | larger ⇒ riskier |
| **Diffusion** | distinct subsystems & directories, within-commit change entropy | more scattered ⇒ riskier |
| **History** | the touched files' priors — prior changes, distinct authors, bug- and security-fix counts, and the composite [`risk_score`](#composite-risk-score) — measured from history *before* the commit | turbulent file history ⇒ riskier |
| **Experience** | the author's prior commit count (long & recent) | more experience ⇒ **less** risky (this group subtracts) |
| **Purpose** | fix / security-fix / revert classification of the message | fixes add, reverts dampen |

The `contributions` block reports each group's signed contribution to the
ordinal `score`, so a consumer can see *why* a commit ranked where it did.
Like the file-level `risk_score`, the score is **ordinal**: rank commits
by it, or compare a commit against the repository's own distribution, but
do not read the magnitude as a probability. Any formula change bumps
`jit_score_version` (separate from the file-level `risk_score_version`).

The commit is scored against its **first parent**. A **merge** commit is
flagged (`is_merge`, `parent_count ≥ 2`) and scored against that first
parent. A **root** commit and any **new files** carry zero priors by
construction — the score then leans on size and author experience, exactly
as the literature prescribes for changes with no file history.

The window / `--ref` / bot / merge / rename flags are shared with the
parent `bca vcs` command; the jit-only flags are the positional `<commit>`
(default `HEAD`), `--format json|yaml|toml|cbor` (default `json`),
`--output`, `--pretty`, and:

```bash
# CI gate: exit 2 when the commit scores at or above the threshold.
bca vcs jit HEAD --fail-over 6.0
```

`--fail-over` uses exit code `2` (the same "metric gate" convention as
`bca check`; exit `1` stays reserved for tool errors). Because the score
is ordinal, calibrate the threshold against your repository's own
commit-score distribution rather than treating it as an absolute.

> Scoring an arbitrary `--diff <file>` (which has no author, parent, or
> file history, so only size and diffusion would be computable) and
> REST / Python parity are deferred follow-ups; ML-based JIT and
> server-side hooks are out of scope.

## Dogfooding in this repo

This project runs `bca vcs` on its own source. `make vcs` prints the
ranked table (path selection and the `.bcaignore` deny-set come from the
repo-root `bca.toml` manifest, the same config `make self-scan` and
`make report` use; `BCA_VCS_TOP` overrides the row cap). On every push to
`main` the Pages CI job folds the rendered ranking into the flagship
report — `bca report html --vcs` / `report markdown --vcs` — so the
published [`reports/index.html`](https://dekobon.github.io/big-code-analysis/reports/index.html)
shows the change-history risk section side-by-side with the AST hotspots,
and additionally publishes the full top-100 ranking as
[`reports/vcs-report.json`](https://dekobon.github.io/big-code-analysis/reports/vcs-report.json)
for tooling.

## REST and Python

- **REST:** `POST /vcs` with a JSON body `{ "id": "...", "repo_path":
  "/path/to/repo", ... }` returns the ranked report. See
  [Driving the REST API](../recipes/rest-api.md).
- **Python:** `big_code_analysis.vcs_metrics(repo_path, …)` returns the
  report as a dict, and `analyze(path, vcs=True)` attaches a `vcs` block
  to a single file's metrics.
