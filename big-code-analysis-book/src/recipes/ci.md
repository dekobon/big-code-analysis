# CI integration

Recipes for wiring `bca` into a build pipeline. The
[`bca check`](../commands/check.md) command ships every output shape a
CI system consumes
([Checkstyle](https://checkstyle.org/),
[SARIF](https://sarifweb.azurewebsites.net/), GitLab
[Code Climate JSON](https://docs.gitlab.com/ci/testing/code_quality/),
clang/GCC warning lines, MSVC warning lines), plus
[`bca report markdown`](../commands/report.md) for humans. This page
maps each *goal* to the subcommand, flags, and platform glue that
serve it. Copy-paste snippets come first; the design notes behind them
([live worked example](#live-worked-example),
[self-scan gate](#self-scan-threshold-gate-local-mirror-of-the-ci-gate),
[trust boundary](#gate-trust-boundary)) come after.

## Picking outputs

The matrix below maps each common goal to the `bca` invocation that
feeds the corresponding CI surface. The linked sections have the
runnable example.

| Goal                                            | Command + flags                                                                                              |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Hard gate on threshold regressions              | `bca check` *(thresholds from the auto-discovered `bca.toml`)*                                               |
| Ratchet thresholds on an existing codebase      | `bca check --baseline .bca-baseline.toml` *(‡)*                                                              |
| Inline PR annotations (GitHub)                  | `bca check … --report-format clang-warning --no-fail` + GCC problem matcher |
| Code Scanning alerts (GitHub)                   | `bca check … --report-format sarif --no-fail` + `github/codeql-action/upload-sarif` |
| Merge-request widget (GitLab Code Quality)      | `bca check … --report-format code-climate --no-fail` |
| Jenkins / SonarQube ingestion                   | `bca check … --report-format checkstyle` |
| Human-readable PR/MR comment or downloadable    | `bca report -O markdown --top 20 --strip-prefix "$PWD/"`                                                        |
| Machine-readable artifact for dashboards        | `bca metrics --format json --output-dir ./out`                                                            |

*(‡) Recommended adoption path when introducing thresholds on a
codebase with existing offenders. See the
[Baselines recipe](baselines.md) for the bootstrap-refresh-retire
workflow.*

The full reference for `bca check`'s output formats, exit codes
(`0` clean, `2` violation, `1` tool error), and threshold config lives
in the [Check command page](../commands/check.md). For the Markdown
report shape, see the [Report command page](../commands/report.md) and
the [Quality reports recipe](quality-reports.md).

## GitHub Actions

### Installing `bca` from a GitHub Release (recommended) {#installing-bca-from-a-github-release-recommended}

For a reproducible install without a Rust toolchain, use the prebuilt
tarball from this repository's
[GitHub Releases](https://github.com/dekobon/big-code-analysis/releases).
It is a single [`curl`](https://curl.se/) `| sha256sum | tar` and
produces byte-identical binaries across runs. Pair it with
[`actions/cache`](https://github.com/actions/cache) keyed by version
so a green-path rerun skips the download entirely:

> **CLI-artifact schema compatibility.** The `BCA_VERSION` you pin
> here must support the schema version of every CLI artifact your
> repo commits, above all `.bca-baseline.toml` (which carries its own
> `version` field) and the `bca.toml` manifest. A baseline file
> written by a newer `bca` (carrying a newer schema version) is
> **not loadable** by an older `bca`, and the gate fails with
> `baseline version N is not supported by this bca`. When tracking
> `main` or regenerating baselines locally with a newer `bca`,
> either re-pin to a release that covers the new schema or switch
> to a `cargo install --git` build of `bca` pointed at the same
> commit your baseline was written from (see the
> [`cargo install` alternative](#alternative-cargo-install-via-prebuilt-aware-actions)
> below).
> The compatibility contract is recorded in
> [STABILITY.md](https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#cli-artifact-formats).

```yaml
env:
  BCA_VERSION: "2.4.0"
  BCA_TARGET:  "x86_64-unknown-linux-gnu"
  # sha256 of big-code-analysis-${BCA_VERSION}-${BCA_TARGET}.tar.gz from the
  # release's SHA256SUMS file. Bump together with BCA_VERSION.
  BCA_SHA256:  "d132c0ef559b4917b7679e17fe3d8f69f5bc14a67692f5d3596d0ec21954d20e"

steps:
  # Cache key MUST include BCA_SHA256 (and BCA_TARGET). Without the
  # sha256 in the key, rotating the published checksum without bumping
  # the version returns a stale binary on cache hit and silently
  # bypasses the `sha256sum --check` in the install step (which is
  # gated on cache miss). Including BCA_TARGET matters when the same
  # workflow runs against multiple `runs-on`.
  - name: Cache bca binary
    id: bca-cache
    uses: actions/cache@v5
    with:
      path: ~/.local/bin/bca
      key: bca-${{ runner.os }}-${{ env.BCA_TARGET }}-${{ env.BCA_VERSION }}-${{ env.BCA_SHA256 }}

  - name: Install bca from GitHub Releases
    if: steps.bca-cache.outputs.cache-hit != 'true'
    run: |
      set -euo pipefail
      stage="big-code-analysis-${BCA_VERSION}-${BCA_TARGET}"
      tarball="${stage}.tar.gz"
      url="https://github.com/dekobon/big-code-analysis/releases/download/v${BCA_VERSION}/${tarball}"
      mkdir -p "$HOME/.local/bin"
      curl -fsSL --proto '=https' --tlsv1.2 -o "/tmp/${tarball}" "$url"
      echo "${BCA_SHA256}  /tmp/${tarball}" | sha256sum --check --strict -
      tar -xzf "/tmp/${tarball}" -C /tmp
      install -m 0755 "/tmp/${stage}/bca" "$HOME/.local/bin/bca"
      rm -rf "/tmp/${tarball}" "/tmp/${stage}"

  - name: Prepend ~/.local/bin to PATH
    run: echo "$HOME/.local/bin" >> "$GITHUB_PATH"
```

Available `BCA_TARGET` values (pick the one that matches `runs-on`):
`x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-gnu`, `aarch64-unknown-linux-musl`,
`aarch64-apple-darwin`, `x86_64-pc-windows-msvc`,
`aarch64-pc-windows-msvc`. Windows assets use `.zip` instead of
`.tar.gz`; the `bca-web` binary ships alongside `bca` in the same
archive.

### Alternative: `cargo install` via prebuilt-aware actions {#alternative-cargo-install-via-prebuilt-aware-actions}

When a runner cannot reach `github.com` (air-gapped, custom mirror)
but can reach [crates.io](https://crates.io/), the two actions below
fall back to `cargo install` when no prebuilt is published, at the
cost of compile time on the cold path. Both pin to the same crates.io
release as the GitHub Releases assets, so the
[CLI-artifact schema compatibility](#installing-bca-from-a-github-release-recommended)
warning applies here unchanged.

If you need a `bca` ahead of the latest crates.io release (for
example, your `.bca-baseline.toml` is committed at a newer schema
than any published `bca` understands), swap the
`tool: big-code-analysis-cli@<version>` or `--version` form for
`cargo install --git https://github.com/dekobon/big-code-analysis
--rev <SHA> --locked big-code-analysis-cli` against the exact commit
the baseline was generated from. This is what the in-tree
[`pages.yml`](https://github.com/dekobon/big-code-analysis/blob/main/.github/workflows/pages.yml)
workflow does (against the local checkout via `--path`). It is a
deliberate workaround for `bca`'s own repo, not a recommended default
for downstream adopters.

Use [taiki-e/install-action](https://github.com/taiki-e/install-action)
or [cargo-binstall](https://github.com/cargo-bins/cargo-binstall).

```yaml
# Option 1: taiki-e/install-action
- name: Install bca
  uses: taiki-e/install-action@v2
  with:
    tool: big-code-analysis-cli@2.4.0
```

```yaml
# Option 2: cargo-binstall
- name: Install cargo-binstall
  uses: cargo-bins/cargo-binstall@main
- name: Install bca
  run: cargo binstall --no-confirm big-code-analysis-cli --version 2.4.0
```

If either action falls back to compilation, cache the cargo registry
and the installed binary so the second run is fast:

```yaml
- name: Cache cargo registry and bca binary
  uses: actions/cache@v5
  with:
    path: |
      ~/.cargo/registry
      ~/.cargo/git
      ~/.cargo/bin/bca
    # crates.io publishes immutable releases, so a `<version>` key is
    # sufficient here; there is no sha256 to rotate. (The GitHub
    # Releases install path above is different: republished release
    # assets share a version, so its cache key must include the sha256.)
    key: bca-${{ runner.os }}-2.4.0
```

Pin to a specific version (matching a published
`big-code-analysis-cli` release on crates.io) so reports stay
reproducible across runs. A floating install surfaces
metric-counting changes as unexplained CI flakes.

### Threshold gate, SARIF, and clang-warning matcher

Three recipes live on the
[Check command page](../commands/check.md#ci-example-github-actions)
rather than here: the hard threshold gate, the SARIF upload to
[Code Scanning](https://docs.github.com/en/code-security/code-scanning),
and the `clang-warning` output paired with the GCC
[problem matcher](https://github.com/actions/toolkit/blob/main/docs/problem-matchers.md)
for inline PR annotations.

### Posting the Markdown report as a PR comment

`bca report markdown` is built for PR/MR comments: a stable header
structure, one row per hot spot, and short paths once you pass
`--strip-prefix`. Pair it with
[`marocchino/sticky-pull-request-comment`](https://github.com/marocchino/sticky-pull-request-comment)
so each push updates a single comment instead of stacking new ones:

```yaml
name: bca-pr-report
on:
  pull_request:
    branches: [main]
jobs:
  report:
    runs-on: ubuntu-latest
    permissions:
      pull-requests: write
    steps:
      - uses: actions/checkout@v4
      - name: Install bca
        uses: taiki-e/install-action@v2
        with:
          tool: big-code-analysis-cli@2.4.0
      - name: Generate report
        run: |
          bca \
            report -O markdown \
            --paths "$PWD" \
            --top 20 \
            --strip-prefix "$PWD/" \
            --output report.md
      - name: Post or update PR comment
        uses: marocchino/sticky-pull-request-comment@v2
        with:
          path: report.md
          header: bca-quality-report
```

The same Markdown file is suitable for upload as a build artifact
([`actions/upload-artifact@v7`](https://github.com/actions/upload-artifact))
if you want it downloadable from the workflow run page in addition to
the PR comment.

### Baseline / ratchet pattern {#baseline--ratchet-pattern}

`bca check --baseline` is the native ratchet: record today's offenders
in a committed TOML file, fail only on regressions and new offenders,
and shrink the file over time. Bootstrap once, commit, then point CI
at it:

```bash
# Once, on a developer machine. Commit both files.
bca check --paths src/ \
    --write-baseline .bca-baseline.toml
git add bca.toml .bca-baseline.toml
```

This snippet bootstraps from `src/` only, which suits a single-crate
library. For a multi-crate workspace, see the
[live worked example](#live-worked-example): its
`.github/workflows/pages.yml` scans the entire repo with
`--exclude-from .bcaignore`, a checked-in deny-set covering vendored
grammars, generated trees, and tests.

> **Share the exclude list across workflow, recipe, and bootstrap.**
> Put the deny-set in a single file at the repo root (a `.bcaignore`
> by convention, mirroring `.gitignore` / `.dockerignore`) and point
> every `bca` invocation at it with `--exclude-from .bcaignore`.
> Patterns from `--exclude-from` are unioned with any inline
> `--exclude <GLOB>` flags into one deny-set; keep `--exclude` for
> one-off excludes. Blank lines and `#`-prefixed comment lines in the
> file are skipped. Patterns follow the same `./`-prefix convention as
> `--exclude` arguments (the walker's emitted form). Pair edits to
> `.bcaignore` with a `--write-baseline` refresh, because the baseline
> keys depend on which files the walker visits.

```yaml
- name: Threshold check with baseline
  run: |
    bca check --paths src/ \
        --baseline .bca-baseline.toml
```

A regressed function (worse than its baseline value in the metric's own
direction: higher, or lower for the `mi.*` family) still fails.
A new offender not in the baseline still fails. An improved function
passes silently and stays in the baseline until the next
`--write-baseline` refresh.

Each surviving violation row starts with a tag that distinguishes a
new offender from a known one that has worsened:

- `[new]`: no baseline entry for this function / metric.
- `[regr …]`: the current value is worse than the recorded baseline,
  measured in the metric's own direction (for the lower-is-worse
  `mi.*` family that is a drop, still rendered `+`). The amount comes
  in two shapes: `[regr +N%]`, a percentage of the record, for counts,
  and `[regr +N.N pts]`, index points, for `mi.*`, whose arbitrary zero
  makes a percentage meaningless. Both start `[regr `, so
  `rg '\[regr '` matches every regressed row. Special forms:
  `[regr from 0]` when a count's baseline value was zero,
  `[regr +>9999%]` when a count's regression exceeds 100× the baseline,
  `[regr NaN]` when the current value is NaN. See
  [Reading the gate output](./baselines.md#reading-the-gate-output)
  for the details.

The violation rows go to stdout; everything below goes to stderr.
See [Which stream](../commands/check.md#which-stream) for the full
split.

After the per-violation rows, stderr carries a per-file rollup footer
with the format `<path>: <count> violations (worst: <metric> =
<value> vs limit <limit> at L<start>)`, sorted by violation count
descending. It is meant to be the first thing a reader looks at:
which file has the most problems, and which metric is the loudest in
that file. Pass `--no-summary` to suppress the footer for tooling that
reads the merged streams.

### Actionable failure output {#actionable-failure-output}

The four sub-sections below turn `bca check`'s failure output from a
wall of offender lines into CI-aware presentations: which files in
this PR tripped a threshold (`--since` / `--changed-only`), inline
file-diff annotations (`--github-annotations`), a rendered
step-summary digest (`$GITHUB_STEP_SUMMARY`), and a copy-paste-safe
remediation block. Each is independent; mix and match per CI surface.
A combined worked example is at the end of this group.

### Diff-aware mode (`--since` / `--changed-only`)

On a PR or push, the developer's first question is usually *which of
my files in this change* tripped a threshold, not the whole-tree
offender list. Two flags answer that:

- `--since <ref>` partitions the per-file footer into a
  "Files in this range:" section (offenders in files touched between
  `<ref>` and `HEAD`) followed by "Other offenders:" (everything
  else). Per-violation lines are unchanged so existing grep-anchored
  tooling keeps working.
- `--changed-only` drops violations from files outside the touched
  set entirely. Use it for PR gates that should be terse.

```yaml
- uses: actions/checkout@v4
  with:
    # `--since origin/<base>` resolves a merge-base. The default
    # `fetch-depth: 1` checkout makes that ref unreachable; `0`
    # pulls the full history so the diff base resolves.
    fetch-depth: 0
- name: Threshold check with diff-aware footer
  run: |
    bca check --paths . --exclude-from .bcaignore \
        --baseline .bca-baseline.toml \
        --since "origin/${{ github.base_ref }}"
```

When `--since` is omitted, `bca` auto-detects the diff base from the
environment in this precedence:

1. `BCA_DIFF_BASE`: the explicit override. Use this from a local
   shell or non-GHA CI runner to mimic the auto-detection.
2. `GITHUB_BASE_REF`: set by GitHub Actions on `pull_request`
   events. Expanded to `origin/<value>`; the runner is responsible
   for the corresponding `git fetch`.
3. `GITHUB_EVENT_BEFORE`: set by GitHub Actions on `push` events to
   the SHA at HEAD before the push. The all-zeroes SHA (force push,
   brand-new branch) is treated as no signal.

A failed auto-detection (`git` missing, ref unresolvable, not a git
checkout) is non-fatal without `--changed-only`: `bca` prints a
warning and falls back to the whole-tree footer. With
`--changed-only`, the same failure is fatal, so a misconfigured CI
cannot pass by suppressing every violation.

The "Files in this range:" banner names the resolved base and the
signal that produced it, so a CI-log reader can verify the gate
latched onto the expected ref:

```text
Files in this range (diff base: origin/main via GITHUB_BASE_REF):
./src/a.rs: 1 violation (worst: cyclomatic = 11 vs limit 2 at L1)

Other offenders:
./src/b.rs: 1 violation (worst: cyclomatic = 11 vs limit 2 at L1)
```

This is distinct from `bca diff-baseline`, which
diffs **baseline files** between two on-disk paths and reports
added / removed / worsened / improved entries. `--since` diffs
**source files** between two git refs.

### GitHub Actions inline annotations (`--github-annotations`)

The GHA UI renders `::error file=…,line=…,title=…::msg`
[workflow commands](https://docs.github.com/en/actions/reference/workflow-commands-for-github-actions)
as inline annotations on the file-diff view, which are easier to find
than lines in the raw job log. `bca check` emits one per violation
according to the tri-state `--github-annotations <auto|always|never>`:
`auto` (the default) enables them when `$GITHUB_ACTIONS == "true"`
(set by every GHA workflow step), `always` forces them on, and
`never` suppresses them even inside a step (useful when a workflow
runs `bca check` twice and wants annotations from only one run). A
bare `--github-annotations` means `always`.

The annotations are emitted alongside the existing per-violation
human stream, not instead of it. To avoid exhausting GitHub's
10-error-per-step UI quota, annotations are capped at 10 per metric;
overflow rolls up to a single `::error::N more <metric> violations
not shown` line so the count stays visible.

```yaml
- name: Threshold check with inline annotations
  run: |
    bca check --paths . --exclude-from .bcaignore \
        --baseline .bca-baseline.toml
  # No `--github-annotations` flag needed: auto-enabled in GHA.
```

Pair this with `--since` (above) so the annotations point at the
files in the PR, not the entire offender list.

### Step-summary markdown digest (`$GITHUB_STEP_SUMMARY`)

GitHub Actions exposes
[`$GITHUB_STEP_SUMMARY`](https://docs.github.com/en/actions/reference/workflow-commands-for-github-actions#adding-a-job-summary),
a path to a markdown file that, when populated, renders as the step's
summary view in the job UI. Whenever that env var is set, or when
`--summary-file <path>` is passed explicitly, `bca check` appends a
digest containing the per-file rollup table, a per-metric count
breakdown, and the top-10 offenders by severity (ranked the same way
as the footer's worst metric). `--summary-file never` suppresses the
digest even when `$GITHUB_STEP_SUMMARY` is set.

The digest is bracketed by HTML-comment markers
(`<!-- bca-step-summary-begin -->` / `<!-- bca-step-summary-end -->`)
so a retried step replaces the previous block rather than stacking a
new one; three retries converge to exactly one up-to-date digest.
Content outside the markers (for example, summaries written by other
tools earlier in the same step) is preserved.

```yaml
- name: Threshold check with step-summary digest
  run: |
    bca check --paths . --exclude-from .bcaignore \
        --baseline .bca-baseline.toml
  # No flag needed: `$GITHUB_STEP_SUMMARY` is set automatically in GHA.
```

Local users can pipe the digest into any markdown file with
`--summary-file <path>`. Empty input (clean run) still writes a
"✓ No threshold violations." block so the step summary positively
confirms the gate ran.

### Remediation footer (always on)

When the gate finds violations, `bca check` emits a trailing
`--- next steps ---` block on stderr (and inside the step-summary
digest from above):

```text
--- next steps ---
* Detailed reports: bca-reports artifact at https://github.com/<owner>/<repo>/actions/runs/<run-id>
* To refresh baseline: bca check --paths . --exclude-from .bcaignore --write-baseline .bca-baseline.toml
* Adoption guide: https://dekobon.github.io/big-code-analysis/recipes/baselines.html
```

The refresh invocation mirrors every flag the gate ran with that
decides what a baseline records, so a first-time reader of a failing
CI log can copy-paste it verbatim. That covers the walk scope
(`--paths`, `--include`, `--exclude`, `--exclude-from`,
`--paths-from`), the walker tuning (`--language`, `--exclude-tests`,
`--cyclomatic-count-try`, `--no-ignore`, `--no-skip-generated`,
`--no-config`), and the gate's own inputs (`--config`, `--threshold`,
`--tier`, `--no-suppress`, `--check-exclude`, `--check-exclude-from`,
`--baseline-fuzzy-match`, `--baseline`). Flags that change only
reporting or the exit code (`--jobs`, `--report-format`, `--output`,
`--no-fail`, `--exit-codes`) are left off, since none can move an
entry into or out of the file. The artifact URL is derived from
`$GITHUB_REPOSITORY` and `$GITHUB_RUN_ID` when both are present
(always true in GHA). Local runs have no upload to point at, so they
suggest running `bca report` for the detailed view instead.

Suppress the block with `--no-remediation` for tooling that reads
the merged streams; a plain `bca check | ...` pipeline never sees it.

Refresh after focused refactors:

```bash
bca check --paths src/ \
    --write-baseline .bca-baseline.toml
git diff .bca-baseline.toml   # expect a shrinking file
```

Two `--write-baseline` runs over an unchanged tree produce
byte-identical output, so a diff appears only when offenders changed.
See the [Baselines recipe](baselines.md) for the full adoption flow,
PR-review heuristics, and the suppression composition rules.

### Putting it all together

The four flags above compose. For a PR-gate workflow, the
recommended invocation is:

```yaml
- uses: actions/checkout@v4
  with:
    # `--since origin/<base>` resolves a merge-base. Default
    # `fetch-depth: 1` makes that ref unreachable; `0` pulls the
    # full history so the diff resolves.
    fetch-depth: 0

- name: Threshold gate (diff-aware + GHA UX)
  run: |
    bca check --paths . --exclude-from .bcaignore \
        --baseline .bca-baseline.toml \
        --since "origin/${{ github.base_ref }}"
  # No `--github-annotations` or `--summary-file` flag needed:
  # both auto-enable from `$GITHUB_ACTIONS == "true"` and
  # `$GITHUB_STEP_SUMMARY`. The trailing remediation block is also
  # auto-emitted.
```

What this gives you on a failing PR:

1. **Per-violation rows on stdout**, in the same shape as the legacy
   gate, so existing grep tooling keeps working.
2. **Per-file rollup footer** with `Files in this range:`
   (touched in the PR) listed before `Other offenders:`, so the
   developer sees their own contributions first.
3. **Inline GHA annotations** on the file-diff view, capped at 10
   per metric with an overflow rollup.
4. **Step-summary panel** with a rendered markdown digest (per-file
   rollup, per-metric breakdown, top-10 offenders by severity).
5. **Trailing remediation block** naming the artifact, printing the
   exact `--write-baseline` refresh invocation, and linking to the
   Baselines recipe.

Knobs:

| Flag                        | Effect                                                      | Default                                                          |
| --------------------------- | ----------------------------------------------------------- | ---------------------------------------------------------------- |
| `--since <ref>`             | Partition footer; auto-detect from env if omitted           | Off, auto-detect via `BCA_DIFF_BASE` / `GITHUB_BASE_REF` / `GITHUB_EVENT_BEFORE` |
| `--changed-only`            | Drop violations outside the diff entirely                   | Off                                                              |
| `--github-annotations <auto\|always\|never>` | Emit `::error file=…::msg` workflow commands (bare flag = `always`) | `auto` enables when `$GITHUB_ACTIONS == "true"`; `never` opts out |
| `--summary-file <path\|auto\|never>` | Append markdown digest; `never` opts out                   | `auto` detects `$GITHUB_STEP_SUMMARY`                           |
| `--no-remediation`          | Suppress the trailing `--- next steps ---` block            | Block emitted on failure                                         |

Local users running `bca check` outside GHA see no change in
default behaviour: none of the four features auto-enable without
an env signal. To preview the GHA experience locally:

```bash
summary=$(mktemp "${TMPDIR:-/tmp}/bca-summary.XXXXXX.md")
GITHUB_ACTIONS=true GITHUB_STEP_SUMMARY="$summary" \
  BCA_DIFF_BASE=main \
  bca check --paths . --exclude-from .bcaignore \
      --baseline .bca-baseline.toml
cat "$summary"
```

For a non-GHA CI (GitLab, Buildkite, Jenkins), set the env vars
your runner exposes (or pass the flags explicitly) and the same
output paths fire.

#### Offender-count delta against merge base (stopgap)

For teams who cannot commit a baseline file (for policy reasons, say),
a coarser approximation counts `<error>` elements in two Checkstyle
documents, one on the merge base and one on the PR head, and fails
when the count grows:

```yaml
- name: Compute offender deltas vs. merge base
  run: |
    set -euo pipefail
    BASE="$(git merge-base origin/main HEAD)"
    git worktree add /tmp/base "$BASE"

    bca check --paths /tmp/base \
        --report-format checkstyle \
        --output /tmp/base.xml \
        --no-fail
    BASE_COUNT=$(grep -c "<error" /tmp/base.xml || true)

    bca check --paths "$PWD" \
        --report-format checkstyle \
        --output /tmp/head.xml \
        --no-fail
    HEAD_COUNT=$(grep -c "<error" /tmp/head.xml || true)

    echo "Offenders: base=$BASE_COUNT head=$HEAD_COUNT"
    if [ "$HEAD_COUNT" -gt "$BASE_COUNT" ]; then
      echo "::error::Offender count grew from $BASE_COUNT to $HEAD_COUNT"
      exit 1
    fi
```

This counts violations, not their identity: renaming an offender does
not register as a regression, and improving one offender while
regressing another nets to zero. The native baseline flow above is
strictly more precise and is the recommended approach.

### Live worked example {#live-worked-example}

`big-code-analysis` runs the recipes above against its own source on
every push and PR. The workflow source,
[`.github/workflows/pages.yml`](https://github.com/dekobon/big-code-analysis/blob/main/.github/workflows/pages.yml),
exercises the threshold gate, the baseline ratchet, both report
formats, and a SARIF upload to GitHub Code Scanning end-to-end against
the workspace itself. (The SARIF upload runs on same-repo pushes and
PRs only; fork PRs skip it because the upload needs a write-scoped
token, exactly as the clippy SARIF job does.) The output sits on
GitHub Pages alongside this book:

- HTML hotspot report:
  <https://dekobon.github.io/big-code-analysis/reports/index.html>
- Markdown PR/MR comment:
  <https://dekobon.github.io/big-code-analysis/reports/report.md>

The `bca` version quoted in the snippets on this page is the latest
published release at the time of writing.

> The in-tree workflow installs `bca` by building it from the current
> checkout rather than downloading a pinned release. This avoids the
> CLI-artifact schema-skew failure mode described under [Installing
> `bca` from a GitHub Release](#installing-bca-from-a-github-release-recommended)
> for repos whose `.bca-baseline.toml` is always written by the same
> `bca` that gates it. Downstream adopters tracking a stable release
> line should stick with the pinned-tarball pattern; only switch to
> "build from checkout" if you, too, are mutating CLI artifact schemas
> in lockstep with the binary.

### Self-scan threshold gate (local mirror of the CI gate) {#self-scan-threshold-gate-local-mirror-of-the-ci-gate}

CI's threshold gate fires only after push, which is too late if a
refactor silently nudged a metric past its limit. The
`big-code-analysis` repo's
[`Makefile`](https://github.com/dekobon/big-code-analysis/blob/main/Makefile)
exposes four targets that mirror the CI gate (the
[`Threshold gate` step in `.github/workflows/pages.yml`](https://github.com/dekobon/big-code-analysis/blob/main/.github/workflows/pages.yml))
locally and add a second tier at 95% of every limit, so encroachment
is caught a commit or two before the hard gate trips:

```bash
make self-scan                            # hard gate, 100% of bca.toml thresholds
make self-scan-headroom                   # soft gate, default 95% (BCA_HEADROOM)
make self-scan-write-baseline             # refresh baseline at hard thresholds
make self-scan-write-baseline-headroom    # refresh baseline at soft thresholds
```

Path selection, the `.bcaignore` deny-set, the per-function
thresholds, the cyclomatic `?` policy, and the baseline file all
live in the repo-root `bca.toml` manifest, which `bca` discovers
automatically. The hard tier is exactly what CI runs; expanded, it
is a bare check with no path, threshold, or baseline flags, because
the manifest supplies them:

```bash
cargo run --quiet --release -p big-code-analysis-cli -- check
```

Both tiers consume the same `bca.toml` thresholds and the same
`.bca-baseline.toml`; the soft tier runs the hard recipe with every
threshold value multiplied by `BCA_HEADROOM`. Both exit `0` clean,
`2` on any threshold violation, and `1` on tool error. The soft tier
is a real gate, not advisory, so do not wrap `make
self-scan-headroom` in `|| true`. The two gate targets (`self-scan`,
`self-scan-headroom`) are wired into `make pre-commit`, `make ci`,
and `.pre-commit-config.yaml`; those chains run the hard tier before
the soft tier, so a true regression always reports before near-limit
headroom. The two `write-baseline` targets are side-effecting and
deliberately not wired in.

`BCA_HEADROOM=0.90 make self-scan-headroom` widens the band;
`BCA_HEADROOM=0.99` tightens it to the last 1%. When the soft tier
fires, absorb the offender into the baseline with
`make self-scan-write-baseline-headroom`, which records every
offender at the scaled thresholds (a strict superset of the hard-tier
offenders).

The pattern (hard tier mirroring CI, soft tier as an early-warning
band, both ratcheted by the same baseline) is project-agnostic. The
[Local threshold gates recipe](local-gates.md) documents the
underlying principles, drop-in Makefile / `just` / `package.json`
skeletons, and the helper script that scales thresholds, so you can
adopt the same workflow in your own repo. The generic recipe uses the
same `BCA_*` env-var names as the Makefile above, so overrides like
`BCA_HEADROOM=0.90` work identically across both.

## GitLab CI

### Full `.gitlab-ci.yml` example

The job below installs `bca`, runs the threshold check producing
Code Climate JSON (for the MR Code Quality widget), Checkstyle XML,
and a Markdown report, then uploads them as artifacts.

> The same [CLI-artifact schema-compatibility
> note](#installing-bca-from-a-github-release-recommended) from the
> GitHub Actions section applies here: the `BCA_VERSION` pin must
> cover the schema version of every CLI artifact you commit.

```yaml
stages:
  - quality

variables:
  BCA_VERSION: "2.4.0"  # pin a published big-code-analysis-cli release
  BCA_TARGET:  "x86_64-unknown-linux-gnu"
  # sha256 of big-code-analysis-${BCA_VERSION}-${BCA_TARGET}.tar.gz from
  # the release's SHA256SUMS file. Bump together with BCA_VERSION.
  BCA_SHA256:  "d132c0ef559b4917b7679e17fe3d8f69f5bc14a67692f5d3596d0ec21954d20e"

bca-quality:
  stage: quality
  image: debian:stable-slim
  cache:
    # Same key shape as the GitHub Actions snippet: bumping
    # BCA_VERSION invalidates the cache automatically.
    key: "bca-$BCA_VERSION"
    paths:
      - .cache/bca/
  before_script:
    - apt-get update -qq && apt-get install -y --no-install-recommends ca-certificates curl tar
    - |
      set -euo pipefail
      install -d "$CI_PROJECT_DIR/.cache/bca" "$HOME/.local/bin"
      if [ ! -x "$CI_PROJECT_DIR/.cache/bca/bca" ]; then
        stage="big-code-analysis-${BCA_VERSION}-${BCA_TARGET}"
        tarball="${stage}.tar.gz"
        url="https://github.com/dekobon/big-code-analysis/releases/download/v${BCA_VERSION}/${tarball}"
        curl -fsSL --proto '=https' --tlsv1.2 -o "/tmp/${tarball}" "$url"
        echo "${BCA_SHA256}  /tmp/${tarball}" | sha256sum --check --strict -
        tar -xzf "/tmp/${tarball}" -C /tmp
        install -m 0755 "/tmp/${stage}/bca" "$CI_PROJECT_DIR/.cache/bca/bca"
        rm -rf "/tmp/${tarball}" "/tmp/${stage}"
      fi
      install -m 0755 "$CI_PROJECT_DIR/.cache/bca/bca" "$HOME/.local/bin/bca"
      export PATH="$HOME/.local/bin:$PATH"
  script:
    - bca
        check
        --paths "$PWD"
        --report-format code-climate
        --output gl-code-quality-report.json
        --no-fail
    - bca
        check
        --paths "$PWD"
        --report-format checkstyle
        --output bca-checkstyle.xml
        --no-fail
    - bca
        report -O markdown
        --paths "$PWD"
        --top 20
        --strip-prefix "$PWD/"
        --output bca-report.md
    # The threshold gate runs separately so the artifacts above still
    # publish on failure. Exit 2 = at least one threshold exceeded.
    - bca check --paths "$PWD"
  artifacts:
    when: always
    reports:
      codequality: gl-code-quality-report.json
    paths:
      - gl-code-quality-report.json
      - bca-checkstyle.xml
      - bca-report.md
```

A few notes about the example:

- The first two `bca check … --no-fail` invocations collect
  offenders for the artifacts; the final `bca check` (no
  `--no-fail`) is the pass/fail gate. All three runs use the same
  threshold config so the artifacts always match the gate decision.
- `artifacts:when: always` keeps every artifact downloadable even on
  a red pipeline, which is when you need them most.
- `artifacts:reports:codequality` wires the Code Climate JSON
  directly into GitLab's MR Code Quality widget; see the
  [Code Quality widget section below](#gitlab-code-quality-widget)
  for the field-by-field semantics.

### GitLab Code Quality widget {#gitlab-code-quality-widget}

GitLab's Code Quality view (inline complaints on the MR diff, summary
on the MR overview page) consumes
[Code Climate JSON](https://docs.gitlab.com/ci/testing/code_quality/).
`bca check` emits this via `--report-format code-climate`, so the
integration is a one-liner:

```yaml
code_quality:
  stage: quality
  script:
    - bca check --paths "$CI_PROJECT_DIR"
          --report-format code-climate
          --output gl-code-quality-report.json
          --no-fail
  artifacts:
    when: always
    reports:
      codequality: gl-code-quality-report.json
    paths:
      - gl-code-quality-report.json
```

Severity bands are derived from how far each metric breaches its
configured threshold: `value / limit` for a count, and for the
maintainability-index family, where lower is worse and the index goes
negative, one step per maintainability band below the floor. `≤ 1.5`
→ `minor`, `≤ 2` → `major`, `≤ 4` → `critical`, `> 4` → `blocker`;
`bca check`'s summary ranks offenders by the same key. The widget
deduplicates findings by `fingerprint`; `bca` hashes
`path \0 function \0 metric` (no line, no value) so a violation that
survives an upstream line-drift edit still collapses into the same
widget entry across pipeline runs.

Sanity-check a generated report locally:

```bash
jq 'all(.[]; has("description") and has("check_name")
     and has("fingerprint") and has("severity")
     and has("location"))' gl-code-quality-report.json
# → true
jq '[.[] | .severity] | unique' gl-code-quality-report.json
# → a subset of ["info","minor","major","critical","blocker"]
```

### MR-only comment with the Markdown report

To attach the Markdown report as an MR note (the GitLab analogue of
the GitHub PR comment recipe), use the project access token and the
[Notes API](https://docs.gitlab.com/ee/api/notes.html):

```yaml
bca-mr-comment:
  stage: quality
  image: alpine:3
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
  needs: ["bca-quality"]
  before_script:
    - apk add --no-cache curl jq
  script:
    - |
      BODY=$(jq -Rs '.' < bca-report.md)
      curl --fail --silent --show-error \
        --request POST \
        --header "PRIVATE-TOKEN: $CI_BCA_BOT_TOKEN" \
        --header "Content-Type: application/json" \
        --data "{\"body\": $BODY}" \
        "$CI_API_V4_URL/projects/$CI_PROJECT_ID/merge_requests/$CI_MERGE_REQUEST_IID/notes"
```

`CI_BCA_BOT_TOKEN` is a project access token with `api` scope. The
job depends on `bca-quality` so the Markdown artifact is in place
before it runs.

## Jenkins / SonarQube

Both [Jenkins](https://www.jenkins.io/) (via the
[Warnings Next Generation](https://plugins.jenkins.io/warnings-ng/)
plugin) and [SonarQube](https://www.sonarsource.com/products/sonarqube/)
(via its
[Generic Issue](https://docs.sonarsource.com/sonarqube-server/latest/analyzing-source-code/importing-external-issues/generic-issue-import-format/)
importer) consume Checkstyle 4.3 XML directly. The same invocation
feeds both:

```bash
bca check --paths src/ \
    --report-format checkstyle \
    --output report.checkstyle.xml
```

Wire `report.checkstyle.xml` into your existing Jenkins
*Record Issues* / SonarQube *External Issues* step. The Checkstyle
writer emits an empty (well-formed) document when there are no
offenders, so neither tool needs special-casing for a clean run. See
the [Check command page](../commands/check.md#checkstyle-ci-integration)
for the writer's schema details.

## Generic CI guidance

Applies regardless of provider:

- **Pin `bca` to a specific version.** Both `cargo install
  --version` and `cargo binstall --version` accept the published
  crate version of `big-code-analysis-cli`. A floating install
  surfaces metric-counting changes as unexplained CI flakes. Pin to a
  version whose CLI-artifact schemas
  ([baseline](https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md#cli-artifact-formats),
  thresholds) match the files your repo commits; see the
  [schema-compatibility note](#installing-bca-from-a-github-release-recommended)
  in the install section.
- **Use the default CPU-based parallelism unless memory requires
  fewer workers.** `--jobs` defaults to the effective CPU count and
  honors
  [`available_parallelism()`](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html)
  (cgroup-, cpuset- and quota-aware on Linux, OS CPU count on
  macOS/Windows), so CI runners need not thread `--jobs "$(nproc)"`
  through every recipe. `--jobs 1` remains a debugging knob, not a
  default. It is *not* memory-aware. Each worker holds one whole file
  at a time and a parse-only run measures 25–70× the source size, so
  the *in-flight* term is roughly `jobs × 70 × the largest source
  file` in the tree, and `--jobs` bounds that term only. The
  destination decides the rest: `--output-dir` writes each document
  as it is produced and `bca check` reduces each file to its
  violations, so both stay at the in-flight bound. `metrics --output
  <FILE>` collects every file's result and serializes it after the
  walk, and structured stdout holds finished documents while an
  earlier file is still being analyzed, which is worst when one large
  file sorts early. A memory-limited container with many vCPUs should
  pass an explicit `--jobs`, and prefer `--output-dir` to
  `--output <FILE>` on a large tree.
- **Always pass `--strip-prefix "$PWD/"` to `bca report markdown`**
  so the path column is identical across runners with different
  workspace paths. Without it the diff between two reports is
  dominated by `/home/runner/work/...` vs.
  `/builds/group/project/...` noise.
- **Store `bca.toml` at the repo root**, alongside
  `Cargo.toml` / `pyproject.toml` / `package.json`. `bca` discovers
  it automatically, so a bare `bca check` reads the committed
  thresholds, paths, and baseline. Treat it as source: review
  threshold relaxations in code review.
- **Handle the exit-code contract.** `bca check` exits `0` clean, `2` on any
  threshold violation, `1` on tool error (bad config, unknown
  metric, unreadable path). Reserving `1` for tool errors lets CI
  distinguish "a function got too complex" from "the analyzer
  crashed". Pass `--exit-codes=tiered` (or set `[check] exit_codes
  = "tiered"` in `bca.toml`) to split the violation case by
  severity: `2` new offenders only, `3` regressions only, `4` both,
  `5` a `--tier=soft` violation that also breaches the hard limit.
  The tiered codes are opt-in; the default stays `0`/`1`/`2`. Every
  fail-state remains non-zero, so `exit != 0 → fail` wrappers keep
  working; only tooling that tests `$? -eq 2` explicitly needs to
  widen to `2`-`5`.
- **Honor in-source suppression markers, audit with
  `--no-suppress`.** The default `bca check` honors
  [`bca: suppress` / `bca: suppress-file` markers](../commands/suppression.md);
  passing `--no-suppress` ignores them so auditors see the raw
  offender list.

## The gate's trust boundary {#gate-trust-boundary}

A pull-request gate analyzes code its author controls, and two of the
inputs that decide *which files the gate looks at* live in that same
branch: file contents, which the
[generated-code detector](../commands/index.html#skipping-generated-code)
sniffs, and the
[`.gitignore` files](../commands/index.html#respecting-gitignore) the
walker honors. Both defaults are right for analyzing a trusted
checkout. In a PR gate they are levers the change under review can
pull. `bca check` reports both by default (the summary below) and
turns both off under [`--strict`](../commands/check.md#strict-mode).

### A generated-code marker removes a file from the gate

Take a C file whose `f()` exceeds `cyclomatic=2`, checked with
`bca check --no-config --paths . --threshold cyclomatic=2`:

| state | exit |
|---|---|
| baseline | `2` (violation reported) |
| prepend `// @generated` as line 1 | `0` |
| same file, `--no-skip-generated` | `2` |
| marker removed again | `2` |

One comment line takes the file out of the gate. Every recognized
marker phrase works (`@generated`, `DO NOT EDIT`, `GENERATED CODE`,
case-insensitive), and the detector scans the first ~50 lines / 5 KiB,
not just line 1. Two forms worth knowing when reviewing a diff:

- `// DO NOT EDIT, see docs/schema.md before changing` reads as
  ordinary housekeeping and passes review without comment.
- `@generated` buried on line 4 of a five-line license header still
  matches; a reviewer scanning for a suspicious first line will not
  see it.

### A committed `.gitignore` removes a file from the walk

Same gate, with the violator accompanied by one benign sibling file:

| state | exit |
|---|---|
| violator + benign sibling | `2` |
| + `.gitignore` containing `violator.c` | `0` |
| same, `--no-ignore` | `2` |

The walker honors ignore files found inside the walked tree, so a
`.gitignore` committed in the branch under test shrinks the checked
set. One nuance: when the ignored file is the *only* analyzable input,
the run exits `1` ("no input files matched") rather than `0`, so the
clean bypass needs at least one other file to remain.

### The gate says what it skipped

`bca check` prints a one-line stderr summary whenever either mechanism
dropped files, so neither bypass is silent:

```text
bca: 2 files not checked (1 generated, 1 ignored) — pass --report-skipped to list them
```

Clean runs print nothing. `--report-skipped` adds a
`note: skipped (generated): <path>` / `note: skipped (ignored): <path>`
line per file. The summary changes no exit code: a bypassed run still
exits `0`, it just says so.

The ignored count covers analyzable files at the directories the walk
entered. A whole directory dropped by an ignore rule (a build tree, or
an attacker ignoring a source directory outright) is never entered and
appears only under `--report-skipped`, as its own clause
(`3 ignored directories not walked`), because ignored build trees are
present in nearly every checkout and would otherwise make the default
summary permanent noise. A PR gate that must see whole-directory drops
should pass `--report-skipped` or, better, `--strict`. See
[the check page](../commands/check.md#strict-mode) for the exact rules.

### `--strict` turns both mechanisms off

For a gate over untrusted input, turn both mechanisms off rather than
auditing stderr:

```yaml
- name: Threshold gate (PR)
  run: bca check --strict --paths src/
```

[`--strict`](../commands/check.md#strict-mode) is equivalent to
`--no-skip-generated --no-ignore`; prefer the single flag, since a
recipe that remembers one of the two constituents still has one open
bypass. Projects opt in once with `strict = true` under `[check]` in
`bca.toml` instead of editing every workflow. Genuinely generated
trees then belong in an explicit deny-set the *reviewers* own (the
committed [`.bcaignore`](#baseline--ratchet-pattern) pattern), not in
content sniffing the *submitter* controls.
