# Quality reports

Recipes for producing aggregated, human-readable Markdown reports.

> **Wiring reports into CI?** See the
> [CI integration recipe](ci.md) for runnable
> [GitHub Actions](https://docs.github.com/en/actions) and
> [GitLab CI](https://docs.gitlab.com/ci/) examples that post the
> Markdown report as a PR/MR comment
> and surface threshold violations through the platform's native code
> quality widgets.

## Live example reports

`big-code-analysis` publishes the output of `bca report -O markdown
--vcs` and `bca report -O html --vcs` against its own source tree on
every push to `main`. Open either to see what the recipes on this page
produce on a multi-language Rust + Python codebase:

- HTML hotspot report (sortable tables, per-language sections, plus a
  "Change-history risk" section from `--vcs`):
  <https://dekobon.github.io/big-code-analysis/reports/index.html>
- Markdown PR/MR comment (paste-into-issue ready):
  <https://dekobon.github.io/big-code-analysis/reports/report.md>
- Change-history risk, full top-100 ranking as machine-readable JSON
  (see [`bca vcs`](../commands/vcs.md)):
  <https://dekobon.github.io/big-code-analysis/reports/vcs-report.json>

The wiring that produces them lives in
[`.github/workflows/pages.yml`](https://github.com/dekobon/big-code-analysis/blob/main/.github/workflows/pages.yml).
The same workflow runs the threshold gate; see
[CI integration](ci.md#live-worked-example) for the full pipeline
shape.

## Generate a project-wide quality report

Run from the project root and write the report to a file:

```bash
bca report \
    --paths "$PWD" \
    -O markdown \
    --top 20 \
    --strip-prefix "$PWD/" \
    --output report.md
```

- `--strip-prefix` keeps the file paths short and stable across
  machines. Without it every row carries the absolute path of the
  current checkout.
- `--top` controls how many rows appear in each hotspot table. 20 is
  a reasonable default for a PR comment; drop to 5 for a dashboard
  tile, or pass `0` to list every row.
- `--jobs` defaults to the effective CPU count
  (cgroup-/cpuset-aware on Linux); pass `--jobs 1` only to force
  serial mode for debugging.

## Limit the report to specific languages

`bca` infers language from extension, so the
include/exclude globs do the filtering:

```bash
bca report \
    --include "*.rs" --include "*.py" \
    --paths "$PWD" \
    -O markdown --output report.md
```

To exclude vendored or generated trees, layer in `--exclude`:

```bash
bca report \
    --include "*.rs" \
    --exclude "**/target/**" --exclude "**/vendor/**" \
    --paths "$PWD" \
    -O markdown
```

> **Flag arity.** `--include` and `--exclude` take exactly one glob per
> occurrence; repeat the flag for additional patterns. The `=` form
> works the same way: `--include="*.rs" --exclude="**/target/**"`.
>
> **Leading `./` is optional.** A bare-relative pattern and its
> `./`-prefixed spelling are equivalent: `--exclude "vendor/**"` matches
> exactly what `--exclude "./vendor/**"` does. This holds for every glob
> surface: `--include`, `--exclude`, `--exclude-from`, `.bcaignore`, and
> the `[check.exclude]` gate-exemption set.

For a stable repo-wide deny-set, keep the patterns in a file at the
repo root (a `.bcaignore` by convention) and load it with
`--exclude-from`. Patterns are unioned with any inline `--exclude`
values; blank lines and `#`-prefixed comments are skipped:

```bash
bca report \
    --paths . \
    --exclude-from .bcaignore \
    -O markdown --output report.md
```

## Show only the worst offenders

For a quick triage view that highlights the top three problems per
section:

```bash
bca report -p src/ -O markdown --top 3
```

The report still includes every section, but each table is short
enough to scan at a glance.

## Compare two revisions

Aggregate reports do not diff revisions on their own. Run the report
on each side and diff the Markdown:

```bash
work=$(mktemp -d "${TMPDIR:-/tmp}/bca-report-diff.XXXXXX")
git worktree add "$work/before" main
bca report -p "$work/before" -O markdown \
    --strip-prefix "$work/before/" --output "$work/before.md"

bca report -p "$PWD" -O markdown \
    --strip-prefix "$PWD/" --output "$work/after.md"

diff -u "$work/before.md" "$work/after.md" | less
```

Because both reports use the same `--strip-prefix` shape, the path
columns line up and the diff shows metric changes rather than path
noise.

## C/C++ preprocessor-aware reports

For macro-heavy C/C++ codebases, feed preprocessor data into the
analyzer so conditional compilation is interpreted the way the
compiler sees it. The workflow is two steps:

```bash
# 1. Build a preprocessor-data JSON from the headers and sources.
bca preproc \
    --paths src/ include/ \
    --output /tmp/preproc.json

# 2. Run the report (or any other command) with that data attached.
bca report \
    --paths src/ \
    --preproc-data /tmp/preproc.json \
    -O markdown --output report.md
```

Every metric-computing walking subcommand (`metrics`, `ops`,
`functions`, `report`, `check`, …) accepts `--preproc-data`.
Subcommands that do not consume it (`vcs`, `preproc`, `list-metrics`,
`diff-baseline`) reject it as a usage error.

Step 1's document is deterministic. The `files` map is written in
sorted path order, and each entry's `direct_includes`,
`indirect_includes`, and `macros` arrays in sorted name order, so two
`bca preproc` runs over an unchanged tree produce byte-identical
output, on stdout as well as through `--output`. Check
`/tmp/preproc.json` into a build cache or diff it between branches
without spurious churn, as you can a `metrics --output` or
`ops --output` document.

You need the sorting rules only when reproducing the output order. The
`files` keys sort by path component, so `a/x.h` precedes `a-b/x.h`.
Include and macro arrays sort by bytes, so `a-b/x.h` precedes `a/x.h`.
You do not need to reproduce either sort when reading the document;
[`jq`](https://jqlang.github.io/jq/) preserves the stored order.

## Analyze only files changed in a PR

Pipe a list of changed files into `--paths-from -` to score just the
diff, not the whole tree:

```bash
git diff --name-only --diff-filter=AM origin/main...HEAD \
    | bca metrics --paths-from - -O json --output-dir ./out
```

- `--diff-filter=AM` keeps Added and Modified files and drops
  Deletions, since a file that no longer exists cannot be analyzed.
- `--paths-from -` reads newline-separated paths from stdin. A file
  argument works the same way: `--paths-from changed.txt`.
- Paths fed in this way are treated as **explicit**, so they bypass
  any `.gitignore` rule that would have hidden them in a directory
  walk. Combine with `-I '*.py' -I '*.rs'` to filter by language
  (repeat the flag once per glob).

For a PR-scoped Markdown summary, swap `metrics` for the report
pipeline:

```bash
git diff --name-only --diff-filter=AM origin/main...HEAD \
    | bca report --paths-from - -O markdown \
        --top 10 --output pr-report.md
```

A directory walk honors `.gitignore` automatically, so the recipes
earlier on this page need no explicit
`-X "**/target/**" -X "**/node_modules/**"` when your project's
`.gitignore` already covers those paths. Add `--no-ignore` to analyze
gitignored trees.
