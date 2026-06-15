# Change-history (VCS) metrics

The `big_code_analysis.vcs` submodule ranks files and scores commits
by **change-history risk** — signals derived from
[version-control](https://git-scm.com/) history rather than the source
AST. It is the Python analogue of the [`bca vcs`](../commands/vcs.md)
CLI command, and the same Rust engine backs both, so the returned
dicts match the CLI's structured output field-for-field.

```python
from big_code_analysis import vcs

report = vcs.rank("path/to/repo", top=20)
trend = vcs.trend("path/to/repo", points=6)
commit = vcs.commit("path/to/repo", commit="HEAD")
diff = vcs.score_diff(unified_diff_text)
```

The four entry points mirror the `bca vcs` subcommands: `vcs.rank`
ranks files (`bca vcs`), `vcs.trend` samples that ranking over time
(`bca vcs trend`), `vcs.commit` scores one commit (`bca vcs commit`),
and `vcs.score_diff` scores a bare unified diff (`bca vcs commit
--diff`). For background on the signals, the composite risk score, and
the underlying defect-prediction literature, read the
[CLI chapter](../commands/vcs.md); this page covers the Python surface.

This is distinct from `analyze(..., vcs=True)`, which attaches a `vcs`
block to a **single file's** metrics. The `vcs` submodule walks history
**once** for a whole repository, so prefer it for ranking; reach for the
`analyze` kwarg only when you want change-history numbers alongside a
file's AST metrics. See [Batch processing](batch.md) for the
`analyze_batch(..., vcs=True)` path that amortises the walk across a
repository's files.

## Ranking files

`vcs.rank(repo_path, *, options=None, top=None, no_cache=False,
cache_dir=None)` ranks every in-scope file by descending risk and
returns a `VcsReportDict`. The keyword-only knobs that vary per call
live on `rank`; the history-walk knobs shared with `trend` and `commit`
live on a shared `Options` object (covered below).

```python
from big_code_analysis import vcs

report = vcs.rank("path/to/repo", top=20)

print(f"long window:  {report['long_window_days']} days")
print(f"recent window: {report['recent_window_days']} days")

for ranked in report["files"]:
    block = ranked["vcs"]
    print(f"{block['risk_score']:6.2f}  {ranked['path']}")
```

`top` caps how many files the ranking keeps; `0` or `None` keeps all.
The report carries the resolved window lengths and the
`risk_score_version` / `vcs_schema_version` stamps once at the top level
(not on each file's `vcs` block). The `files` list is ordered by
descending `vcs.risk_score`, and a `vcs_aggregate` key holds the
repository [bus-factor](https://en.wikipedia.org/wiki/Bus_factor)
summary when it was computed:

```python
aggregate = report.get("vcs_aggregate")
if aggregate is not None:
    bus = aggregate["bus_factor"]
    print(f"repo bus factor: {bus['repo']['bus_factor']}")
```

## Scoring a commit

`vcs.commit(repo_path, *, commit="HEAD", options=None)` scores a single
commit for just-in-time (commit-level) risk against its first parent,
returning a `JitCommitReportDict`. The `commit` argument is any git
revision spelling (`"HEAD"`, `"HEAD~3"`, a branch, a tag, a SHA).

```python
from big_code_analysis import vcs

report = vcs.commit("path/to/repo", commit="HEAD")

print(f"risk score: {report['risk_score']}")
print(f"is merge:   {report['commit']['is_merge']}")

size = report["features"]["size"]
print(f"+{size['lines_added']} -{size['lines_deleted']} "
      f"across {size['files_touched']} files")

# Each feature group's signed push on the ordinal score.
for group, value in report["contributions"].items():
    print(f"  {group:<11} {value}")
```

The score is **ordinal**: rank commits by it, or compare a commit
against the repository's own distribution, but do not read the
magnitude as a probability. The `contributions` block reports each
feature group's signed contribution so a consumer can see *why* a commit
ranked where it did.

## Scoring an arbitrary diff

`vcs.score_diff(diff)` scores a git-style unified diff that has not been
committed yet — the shape a pre-commit hook or code-review bot works
with. It returns a `JitDiffReportDict`.

```python
import subprocess
from big_code_analysis import vcs

staged = subprocess.run(
    ["git", "diff", "--cached"],
    capture_output=True, text=True, check=True,
).stdout

report = vcs.score_diff(staged)
print(f"partial risk: {report['partial_risk_score']}")
```

A bare diff carries no author, parent, or file history, so `source` is
the literal `"diff"`, only the `size` and `diffusion` groups are
computable, and `partial_risk_score` is **not comparable** to a commit's
`risk_score`. The history, experience, and purpose groups are absent, not
zero.

## Sampling a trend

`vcs.trend(repo_path, *, options=None, points=12, span=None, top=None,
top_deltas=None)` samples the file ranking at several points in time and
returns a `VcsTrendDict`. Each point re-anchors at that moment's mainline
tip, so the series is a sequence of true historical snapshots rather than
the current ranking re-projected backwards.

```python
from big_code_analysis import vcs

trend = vcs.trend("path/to/repo", points=6, span="6mo", top_deltas=10)

# The sample timestamps, oldest first (Unix seconds).
print("sampled at:", trend["as_of_points"])

# Files that regressed most over the window.
for delta in trend["deltas"]["regressed"]:
    print(f"  +{delta['delta']:.2f}  {delta['path']}")
```

`points` (at least 2) samples span `span` (default `12mo`), ending at
`options.as_of`. The `files` map aligns each file's series 1:1 with
`as_of_points`, with a `None` element where the file did not yet exist.
The `deltas` summary splits files into `improved` and `regressed` lists;
`top_deltas` trims each list and `top` caps how many files are kept.

## Shared options

The three repository-walking entry points — `rank`, `trend`, and
`commit` — accept the same `vcs.Options` object, so one configuration
can drive a rank plus a trend pass without restating the common knobs.
Every field is keyword-only and optional, and the defaults reproduce the
`bca vcs` CLI defaults, so `Options()` matches the default ranking.

```python
from datetime import datetime, timezone
from big_code_analysis import vcs

options = vcs.Options(
    long_window="2y",
    recent_window="60d",
    risk_formula="percentile",
    file_types=["rs", "py"],
    as_of=datetime(2026, 1, 1, tzinfo=timezone.utc),
)

report = vcs.rank("path/to/repo", options=options, top=20)
```

The widened option kwargs (issue #619) each accept more than a bare
string:

* `file_types` selects which files to rank: `"metrics"` (the default —
  only files bca computes metrics for), `"all"` (every tracked text
  file), a comma-separated extension allow-list (`"rs,py"`), or a
  `Sequence[str]` of extensions (`["rs", "py"]`).
* `as_of` pins the reference "now" for reproducible snapshots, as either
  a `datetime` or a string ([RFC 3339](https://www.rfc-editor.org/rfc/rfc3339),
  `@unix`, or a git date). Pinning `as_of` makes a run reproducible: the
  ranking is computed as it stood at that moment, not against the wall
  clock.
* `cache_dir` (on `rank`, not `Options`) accepts a `str` or any
  `os.PathLike` — a `pathlib.Path` passes straight through.

The history-walk toggles mirror the CLI flags: `full_history`,
`include_merges`, `follow_renames` (default `True`), `exclude_bots`
(default `True`), and `bot_pattern` to override the bot-author regular
expression. `bus_factor_threshold` sets the coverage fraction for the
bus-factor flag (default `0.5`), and `emit_author_details` includes
SHA-256-hashed canonical author identities.

## Caching

`vcs.rank` keeps a [persistent cache](../commands/vcs.md#caching) of each
history walk, on by default. A cache hit is bit-identical to a fresh
walk, and the time windows are recomputed against the current moment on
every run, so a cached result is never stale.

```python
from big_code_analysis import vcs

# First call primes the cache; the second replays it.
vcs.rank("path/to/repo")
vcs.rank("path/to/repo")  # reuses the prior walk

vcs.rank("path/to/repo", no_cache=True)            # ignore the cache
vcs.rank("path/to/repo", cache_dir="/tmp/bca")     # override the directory
```

By default the cache lives under the platform cache directory. Author
identities are stored only as their SHA-256 digests, never plaintext.
Note that hashing is pseudonymization, not anonymization: the digests are
recoverable against a candidate email set — see
[Author-detail privacy](../commands/vcs.md#author-detail-privacy).

## Releasing the GIL

The repository-walking calls (`vcs.rank`, `vcs.trend`, and the commit
score in `vcs.commit`) release the [GIL](https://docs.python.org/3/glossary.html#term-global-interpreter-lock)
across the history walk (issue #620), so a `ThreadPoolExecutor` can rank
several repositories in parallel without serialising on the interpreter
lock:

```python
from concurrent.futures import ThreadPoolExecutor
from big_code_analysis import vcs

repos = ["service-a", "service-b", "service-c"]

with ThreadPoolExecutor() as pool:
    reports = list(pool.map(lambda r: vcs.rank(r, top=20), repos))
```

This is the same pattern the [Async patterns](async.md) page applies to
the per-file `analyze` calls.

## Errors

The `vcs` functions raise a typed exception hierarchy rooted at
`bca.VcsError` (itself a `ValueError`). See
[Error handling](errors.md#change-history-vcs-exceptions) for the full
taxonomy and which call raises which type.

## See also

* [Change-history (VCS) metrics](../commands/vcs.md) — the CLI chapter,
  with the signal definitions, the composite risk-score formula, and the
  defect-prediction literature behind them.
* [Batch processing](batch.md) — `analyze_batch(..., vcs=True)` attaches
  a per-file `vcs` block while sharing one history index per repository.
* [Error handling](errors.md) — the VCS exception taxonomy.
