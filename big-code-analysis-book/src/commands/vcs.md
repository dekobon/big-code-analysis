# Change-history (VCS) metrics

`bca vcs` ranks files by **change-history risk** — signals derived from
version-control history rather than the source AST. It is the project's
first language-agnostic, non-AST metric family (issue
[#328](https://github.com/dekobon/big-code-analysis/issues/328)). The
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
Change-history risk (long window 365d, recent 90d, formula v1)
 RANK      RISK  COMMITS r/l    CHURN r/l  AUTHORS  FILE
    1       7.2        68/68  11634/11634        1  src/metrics/cyclomatic.rs
    2       6.9        68/68    7299/7299        1  src/metrics/npa.rs
    ...
```

With no `--format`, a human-readable ranked table is printed. Pass
`--format json|yaml|toml|cbor|csv` for structured output (CBOR requires
`--output <dir>`). The global `--paths` / `--include` / `--exclude` /
`--no-ignore` filters are reused to pick which tracked files to report.

`bca vcs` errors clearly when run outside a git working tree.

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
| `risk_score` | f64 | Composite, formula-versioned (see below) — **ordinal, not cardinal** |
| `hotspot_score` | f64? | `complexity × churn_recent`; present only when AST metrics are computed alongside |
| `risk_score_version` / `vcs_schema_version` | u32 | Forward-compatibility version stamps |

Author identities are canonicalised through the repository `.mailmap`
and counted by lowercased email; `Co-authored-by:` trailers add
participants. Bot identities (`dependabot[bot]`, `renovate[bot]`,
`github-actions[bot]`, …) are excluded by default. Binary files and
symlinks are skipped; an untracked file has no record at all (distinct
from a tracked file with zero in-window activity).

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
new_file_bonus = 0.15 if age_days < recent_window_days else 0
dev_bonus      = 0.35 if authors_long >= 9 else 0.15 if authors_long >= 6 else 0

base = 0.30 * recency_churn
     + 0.25 * recency_count
     + 0.15 * long_count
     + 0.15 * author_factor * (1 + dilution)
     + 0.10 * fix_factor
     + 0.05 * long_churn
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
(Sentence-Level VFC studies; PySecDB). The full derivation lives in
`src/vcs/score.rs`.

The score is **ordinal**: only relative ranks have meaning. Any change
to the formula bumps `risk_score_version`.

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

## Dogfooding in this repo

This project runs `bca vcs` on its own source. `make vcs` prints the
ranked table (path selection and the `.bcaignore` deny-set come from the
repo-root `bca.toml` manifest, the same config `make self-scan` and
`make report` use; `BCA_VCS_TOP` overrides the row cap). The Pages CI job
also publishes a `vcs-report.json` / `vcs-report.txt` under the site's
`reports/` directory each run. A first-class rendered HTML/Markdown page
is tracked in issue
[#573](https://github.com/dekobon/big-code-analysis/issues/573).

## REST and Python

- **REST:** `POST /vcs` with a JSON body `{ "id": "...", "repo_path":
  "/path/to/repo", ... }` returns the ranked report. See
  [Driving the REST API](../recipes/rest-api.md).
- **Python:** `big_code_analysis.vcs_metrics(repo_path, …)` returns the
  report as a dict, and `analyze(path, vcs=True)` attaches a `vcs` block
  to a single file's metrics.

## Scope (v1)

v1 ships one backend (`vcs-git`, built on
[gitoxide](https://github.com/GitoxideLabs/gitoxide)) behind the
umbrella `vcs` Cargo feature. Out of scope and tracked as follow-ups:
per-function granularity ([#329](https://github.com/dekobon/big-code-analysis/issues/329)),
change entropy ([#330](https://github.com/dekobon/big-code-analysis/issues/330)),
just-in-time scoring ([#331](https://github.com/dekobon/big-code-analysis/issues/331)),
bus factor ([#332](https://github.com/dekobon/big-code-analysis/issues/332)),
historical trend ([#333](https://github.com/dekobon/big-code-analysis/issues/333)),
a persistent cache ([#334](https://github.com/dekobon/big-code-analysis/issues/334)),
and additional VCS backends ([#335](https://github.com/dekobon/big-code-analysis/issues/335)).
