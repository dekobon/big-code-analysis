# Quality reports

Recipes for producing aggregated, human-readable Markdown reports.

> **Wiring reports into CI?** See the
> [CI integration recipe](ci.md) for runnable GitHub Actions and
> GitLab CI examples that post the Markdown report as a PR/MR comment
> and surface threshold violations through the platform's native code
> quality widgets.

## Generate a project-wide quality report

Run from the project root and write the report to a file:

```bash
bca \
    --paths "$PWD" \
    --num-jobs "$(nproc)" \
    report markdown \
    --top 20 \
    --strip-prefix "$PWD/" \
    --output report.md
```

- `--strip-prefix` keeps the file paths short and stable across
  machines — without it every row carries the absolute path of the
  current checkout.
- `--top` controls how many rows appear in each hotspot table. 20 is
  a good default for a PR comment; drop to 5 for a dashboard tile.
- `--num-jobs` controls parallelism. The walker is CPU-bound on most
  modern hardware.

## Limit the report to specific languages

`bca` infers language from extension, so the
include/exclude globs do the filtering:

```bash
bca \
    --include "*.rs" "*.py" \
    --paths "$PWD" \
    report markdown --output report.md
```

To exclude vendored or generated trees, layer in `--exclude`:

```bash
bca \
    --include "*.rs" \
    --exclude "**/target/**" "**/vendor/**" \
    --paths "$PWD" \
    report markdown
```

> **Flag ordering.** `--include` and `--exclude` accept multiple values
> and stop only when the next flag begins. Put them **before**
> `--paths` (or any single-value flag) so the subcommand name isn't
> swallowed as a glob. Equivalent single-value forms with `=` also
> work: `--include="*.rs" --exclude="**/target/**"`.

## Show only the worst offenders

For a quick triage view that highlights the top three problems per
section:

```bash
bca -p src/ report markdown --top 3
```

The report still includes every section, but each table is short
enough to scan at a glance.

## Compare two revisions

Aggregate reports do not diff revisions on their own. Run the report
on each side and diff the Markdown:

```bash
git worktree add /tmp/before main
bca -p /tmp/before report markdown \
    --strip-prefix /tmp/before/ --output /tmp/before.md

bca -p "$PWD" report markdown \
    --strip-prefix "$PWD/" --output /tmp/after.md

diff -u /tmp/before.md /tmp/after.md | less
```

Because both reports use the same `--strip-prefix` shape, the path
columns line up and the diff is dominated by metric changes rather
than path noise.

## C/C++ preprocessor-aware reports

Macro-heavy C/C++ codebases benefit from feeding preprocessor data
into the analyzer so that conditional compilation is interpreted the
way the compiler sees it. The workflow is two steps:

```bash
# 1. Build a preprocessor-data JSON from the headers and sources.
bca \
    --paths src/ include/ \
    preproc \
    --output /tmp/preproc.json

# 2. Run the report (or any other command) with that data attached.
bca \
    --paths src/ \
    --preproc-data /tmp/preproc.json \
    report markdown --output report.md
```

`--preproc-data` is a global flag, so it works with `metrics`, `ops`,
`functions`, and the other subcommands as well — anywhere accurate
C/C++ analysis matters.

## Analyze only files changed in a PR

Pipe a list of changed files into `--paths-from -` to score just the
diff, not the whole tree:

```bash
git diff --name-only --diff-filter=AM origin/main...HEAD \
    | bca --paths-from - metrics -O json -o ./out
```

- `--diff-filter=AM` keeps Added and Modified files and drops
  Deletions — you cannot analyze a file that no longer exists.
- `--paths-from -` reads newline-separated paths from stdin. A file
  argument works the same way: `--paths-from changed.txt`.
- Paths fed in this way are treated as **explicit**, so they bypass
  any `.gitignore` rule that would have hidden them in a directory
  walk. Combine with `-I '*.py' '*.rs'` to filter by language.

For a PR-scoped Markdown summary, swap `metrics` for the report
pipeline:

```bash
git diff --name-only --diff-filter=AM origin/main...HEAD \
    | bca --paths-from - report markdown \
        --top 10 --output pr-report.md
```

`.gitignore` is honored automatically when walking a directory, so
recipes earlier in this page no longer need an explicit
`-X "**/target/**" "**/node_modules/**"` if those paths are already
covered by your project's `.gitignore`. Add `--no-ignore` if you do
need to analyze gitignored trees.
