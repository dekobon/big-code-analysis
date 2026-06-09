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

### File-type scope

By default `bca vcs` ranks **only the files bca computes metrics for** —
the same set `bca metrics` would analyse. High-churn non-source files
(`CHANGELOG.md`, `Cargo.lock`, generated config) carry no maintainability
meaning yet maximise the churn / commit / author signals, so ranking them
beside source code is noise; scoping to files-with-metrics also keeps the
standalone ranking aligned with the AST hotspot tables in
[`bca report --vcs`](report.md).

`--file-types <SCOPE>` selects the scope:

| Value | Meaning |
|---|---|
| `metrics` *(default)* | Only files bca has a language/metrics for, by extension |
| `all` | Every tracked, non-binary, non-symlink text file |
| `rs,py,toml,…` | A comma-separated extension allow-list (leading dots optional, case-insensitive) |

```bash
bca vcs                          # rank source files only (default)
bca vcs --file-types all         # rank every tracked text file
bca vcs --file-types rs,py       # rank only Rust and Python files
```

The check is **extension-only** (no file content is read) and ANDs with
the `--paths` / `--include` / `--exclude` / `--no-ignore` filters — a
file must pass both to be ranked. Extension-less files (`Makefile`,
`Dockerfile`, `LICENSE`) and unknown extensions are out of the `metrics`
scope; a custom list is a literal extension filter, so it can include a
non-metrics type like `toml`. An empty or all-blank custom list is a
clear error rather than a scope that silently ranks nothing.

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
| `--file-types <SCOPE>` | `metrics` | Files to rank: `metrics`, `all`, or an extension list (`rs,py`) |
| `--ref <REF>` | `HEAD` | Revision to analyse |
| `--full-history` | off | Walk the full DAG (default: first-parent only) |
| `--include-merges` | off | Include merge commits |
| `--no-follow-renames` | off | Stop following renames (default: follow) |
| `--no-exclude-bots` / `--bot-pattern <RE>` | exclude | Bot-author filtering |
| `--as-of <WHEN>` | wall clock | Reference "now" (RFC 3339 / `@unix` / git date) for reproducible snapshots |
| `--risk-formula {weighted\|percentile}` | `weighted` | Composite formula |
| `--emit-author-details` | off | Emit SHA-256-hashed canonical author IDs |
| `--include-deleted` | off | Also rank files deleted at the target ref |
| `--no-cache` | off | Skip the persistent history cache (always walk fresh) |
| `--clear-cache` | off | Wipe this repo's cached history before running |
| `--cache-dir <DIR>` | platform cache | Override the cache directory |

## Caching

Ranking re-walks only the part of history inside the long window, but on a
large, active repository that is still the dominant cost — and in CI the
interesting deltas between runs are just the commits pushed since the last
one. `bca vcs` therefore keeps a **persistent cache** of each walk, keyed
by the resolved `HEAD` SHA and the repository's identity:

- On an **unchanged tree** the prior result is replayed, no history walk.
- When **`HEAD` has advanced** the walk visits only the new commits and
  splices them onto the cached history.
- A **force-push** (the cached head is no longer an ancestor of the new
  one) falls back to a full walk.

The cache is a pure optimization: a hit is **bit-identical** to a fresh
walk, and the time windows are recomputed against the *current* moment on
every run, so a cached result is never stale. An entry is ignored — and
the history recomputed — whenever the schema, the score-formula version,
or the *walk-affecting* options differ; in particular **changing a window
forces a fresh walk**. (Finalization-only knobs such as `--risk-formula`,
`--emit-author-details`, and `--include-deleted` are applied on replay, so
they reuse the same cached walk.)

By default the cache lives under
`$XDG_CACHE_HOME/big-code-analysis/vcs` (`%LOCALAPPDATA%` on Windows,
`~/.cache` otherwise). Author identities are stored only as their
irreversible SHA-256 digests — never plaintext — so the cache is not a
side channel for raw author emails. The same cache transparently
accelerates `bca metrics --vcs` and `bca report --vcs`.

```bash
# First run primes the cache; the second replays it.
bca vcs --paths .
bca vcs --paths .                 # reuses prior work

bca vcs --no-cache --paths .      # ignore the cache for this run
bca vcs --clear-cache --paths .   # rebuild from scratch
bca vcs --cache-dir /tmp/bca-cache --paths .
```

The REST (`POST /vcs`) and Python (`vcs_metrics`) surfaces expose the same
behaviour through optional `no_cache` / `cache_dir` parameters.

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

### Scoring an arbitrary diff (`--diff`)

`bca vcs jit --diff <file>` scores a `git diff` instead of a commit
(use `--diff -` to read the diff from stdin). This is handy in a
pre-commit hook or a code-review bot, where the change exists only as a
diff and has not been committed yet.

```bash
git diff --cached | bca vcs jit --diff - --pretty
```

The input must be a **git-style** unified diff carrying `diff --git`
file headers, as produced by `git diff` or `git format-patch`. Plain
`diff -u` / `diff -ru` output (which has `---`/`+++` header lines but no
`diff --git` header) parses to zero files, and combined / merge diffs
(`git diff --cc`, with `@@@` hunk headers) are rejected as a malformed
diff — pipe a regular two-way `git diff` instead.

A bare diff carries **no author, parent, or file history**, so only the
**size** and **diffusion** groups are computable. The output is therefore
a deliberately *partial* report — a distinct shape from a commit report:

```console
$ git diff | bca vcs jit --diff - --pretty
{
  "jit_schema_version": 1,
  "jit_score_version": 1,
  "source": "diff",
  "partial_score": 1.83,
  "size":      { "lines_added": 42, "lines_deleted": 8,
                 "files_touched": 3, "hunks": 6 },
  "diffusion": { "subsystems": 2, "directories": 3, "entropy": 1.46 },
  "contributions": { "size": 1.18, "diffusion": 0.65 }
}
```

The `source` field is a permanent `"diff"` marker, and the history /
experience / purpose groups are **absent from the report entirely** —
*not* present as zero. Zero is a real value (a commit genuinely with no
prior history scores those groups at zero); an absent group means
"unavailable", so a consumer can never mistake an unscored group for
"low risk". For the same reason the score field is named `partial_score`,
not `score`.

> **A diff-only score is not comparable to a commit score.** The partial
> score sums only size + diffusion, so it is always lower than the full
> commit score for the same change (which also folds in history,
> experience, and purpose). Rank diffs against *other diffs*, never
> against commit scores. `--diff` and the positional `<commit>` are
> mutually exclusive; `--fail-over` works in both modes (calibrate the
> diff-mode threshold against your own diff-score distribution).

### REST and Python parity

The JIT score is also available off the CLI:

- **REST:** `POST /vcs/jit` with `{ "id", "repo_path", "commit" }` returns
  the commit `JitReport` JSON, or `{ "id", "diff" }` returns the partial
  diff report. See [Driving the REST API](../recipes/rest-api.md).
- **Python:** `vcs_jit(repo_path, commit=...)` returns the commit report
  as a `dict`, or `vcs_jit(diff=...)` the partial diff report. See the
  [Python bindings](../python/README.md).

ML-based JIT models and server-side hook integration remain out of scope.

## Historical trend (over time)

A single `bca vcs` run answers *"what is risky now."* `bca vcs trend`
answers *"is it getting better or worse"* — the actionable question for a
technical-debt programme — by sampling the metrics at several points in
time and emitting a per-file time series.

```console
$ bca vcs --top 20 trend --points 12 --span 24mo --pretty
{
  "trend_schema_version": 1,
  "vcs_schema_version": 2,
  "risk_score_version": 2,
  "long_window_days": 365,
  "recent_window_days": 90,
  "truncated_shallow_clone": false,
  "as_of_points": [ 1700000000, 1705259520, ... ],
  "files": {
    "src/parser.rs": [
      null,                       // did not exist at the oldest point
      { "as_of": 1705259520, "risk_score": 4.1, ... },
      { "as_of": 1710519040, "risk_score": 6.8, ... }
    ]
  },
  "deltas": {
    "improved":  [ { "path": "src/old.rs",    "delta": -3.2, ... } ],
    "regressed": [ { "path": "src/parser.rs", "delta":  2.7, ... } ]
  }
}
```

`--points N` evenly-spaced samples (inclusive of both endpoints) cover
`--span DURATION`, ending at `--as-of` (or wall-clock now). `as_of_points`
lists the sample timestamps oldest-first; every file's array aligns to it
1:1, with a `null` element marking a point where the file did not exist
yet. `deltas` ranks the files whose `risk_score` fell the most
(`improved`) and rose the most (`regressed`) between each file's earliest
and latest present points; `--top-deltas` trims each list.

Crucially, each point **re-anchors at the mainline tip that existed at or
before that moment** — it does not just re-window today's `HEAD` tree.
That is what makes a file born later show as `null` at older points
(rather than leaking its present-day metrics backwards). Files kept in the
series are the `--top` highest-risk by their most-recent sample.

Flags reused from the parent `bca vcs` command: the window (`--long-window`
/ `--recent-window`), `--ref`, `--file-types`, bot / merge / rename
toggles, `--as-of` (the most-recent anchor), and `--top`. `-O` accepts `json` (default),
`yaml`, or `cbor`; TOML is excluded because an absent point serializes as
`null`, which TOML cannot represent. The point count is bounded (2–120) to
keep the per-point history walks tractable on deep histories.

> **Rename caveat.** Renames are followed *within* each sample's walk, but
> a file renamed *between* two samples appears as two separate path series
> (its old name, then its new name) rather than one continuous line.
> Cross-sample rename stitching is a deferred follow-up.

## Bus factor (directory & repo level)

Where the per-file `ownership_top_share` measures concentration *within* a
file, the **bus factor** (a.k.a. truck factor) measures it *across a set of
files*: the minimum number of developers whose departure would leave more
than half of a directory's files without a knowledgeable maintainer. `bca
vcs` emits it as a top-level `vcs_aggregate` object alongside the ranked
`files`:

```jsonc
{
  "vcs_aggregate": {
    "bus_factor": {
      "schema_version": 1,
      "coverage_threshold": 0.5,
      "doa_threshold": 0.75,
      "repo": { "bus_factor": 3, "files": 412, "authors": 11 },
      "by_directory": [
        { "directory": "src", "bus_factor": 2, "files": 180, "authors": 7 },
        { "directory": "src/vcs", "bus_factor": 1, "files": 24, "authors": 3 }
      ]
    }
  }
}
```

Each developer's authorship of each file is scored with the Avelino
*Degree-of-Authorship* heuristic (Avelino, Passos, Hora & Valente, *A Novel
Approach for Estimating Truck Factors*, ICPC 2016):

```text
DoA(d, f) = 3.293 + 1.098·FA + 0.164·DL − 0.321·ln(1 + AC)
```

where `FA` is first authorship (`1` if `d` created `f`), `DL` is `d`'s
deliveries (changes) to `f`, and `AC` is the changes made by *other*
developers. A developer is an **author** of `f` when their DoA, normalised
by the file's maximum, clears `0.75` (the paper's threshold). The truck
factor is then a greedy removal: drop the developer who authors the most
still-covered files, repeat until more than `--bus-factor-threshold`
(default `0.5`, per Avelino) of the files are orphaned, and report how many
were removed. `by_directory` covers each top-level directory and each of
its immediate subdirectories, computed over every file recursively beneath
it.

Caveats, by construction:

- A repository (or directory) of mostly single-author files reports a bus
  factor of `1` — losing that one author orphans each file. This is the
  heuristic working as intended, not a bug; treat the number as a planning
  signal, not a guarantee.
- Bot identities are filtered (like the per-file signals), and files with
  no in-window activity carry no authorship and are excluded from the
  denominator.
- "First authorship" means the earliest commit *observed within the long
  window*, not necessarily a file's true creation.

The aggregate reflects the **whole repository within the file-type
scope** (one history walk covers every in-scope file — by default the
files-with-metrics set, see [File-type scope](#file-type-scope) — so
`--file-types all` widens the bus factor to every tracked file).
`--paths` / `--include` / `--exclude` scope only the ranked per-file
list, *not* the bus factor. To focus on a subsystem, read its entry in
`by_directory` rather than filtering the walk.

`--emit-author-details` adds a `key_author_ids` list to each group — the
SHA-256-hashed identities of the removed key developers, in removal order
(plaintext identities never leave the process). The aggregate is computed
only for the dedicated `bca vcs` / `bca report --vcs` reports and the REST
/ Python endpoints; the per-file `bca metrics --vcs` injection path does
not pay for it.

## Dogfooding in this repo

This project runs `bca vcs` on its own source. `make vcs` prints the
ranked table (path selection and the `.bcaignore` deny-set come from the
repo-root `bca.toml` manifest, the same config `make self-scan` and
`make report` use; `BCA_VCS_TOP` overrides the row cap). The manifest's
`[vcs] file_types` key sets the default scope (the `--file-types` CLI
flag replaces it when given). On every push to
`main` the Pages CI job folds the rendered ranking into the flagship
report — `bca report html --vcs` / `report markdown --vcs` — so the
published [`reports/index.html`](https://dekobon.github.io/big-code-analysis/reports/index.html)
shows the change-history risk section side-by-side with the AST hotspots,
and additionally publishes the full top-100 ranking as
[`reports/vcs-report.json`](https://dekobon.github.io/big-code-analysis/reports/vcs-report.json)
for tooling.

## REST and Python

- **REST:** `POST /vcs` with a JSON body `{ "id": "...", "repo_path":
  "/path/to/repo", ... }` returns the ranked report, and `POST /vcs/trend`
  (same fields plus `points` / `span` / `top_deltas`) returns the
  historical time series. See [Driving the REST API](../recipes/rest-api.md).
- **Python:** `big_code_analysis.vcs_metrics(repo_path, …)` returns the
  ranked report as a dict, `vcs_trend(repo_path, points=…, span=…, …)`
  returns the time series, and `analyze(path, vcs=True)` attaches a `vcs`
  block to a single file's metrics.

Both `POST /vcs` and `vcs_metrics()` accept an optional `file_types`
(`"metrics"` / `"all"` / `"rs,py"`) to scope which files are ranked,
mirroring the CLI `--file-types`.

Both `POST /vcs` and `vcs_metrics()` include the `vcs_aggregate` bus
factor in the result and accept a `bus_factor_threshold` (in `(0, 1)`) to
tune the coverage fraction.
