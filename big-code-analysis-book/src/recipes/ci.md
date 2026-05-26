# CI integration

Recipes for wiring `bca` into a build pipeline. The
[`bca check`](../commands/check.md) command already ships every output
shape a modern CI needs (Checkstyle, SARIF, GitLab Code Climate JSON,
clang/GCC warning lines, MSVC warning lines), plus
[`bca report markdown`](../commands/report.md)
for humans. This page is a consolidated map from the user's *goal* to
the right combination of subcommand, flags, and platform glue.

## Picking outputs

The matrix below maps each common goal to the `bca` invocation that
feeds the corresponding CI surface. Linked sections below have the
runnable example.

| Goal                                            | Command + flags                                                                                              |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Hard gate on threshold regressions              | `bca check --config bca-thresholds.toml`                                                                     |
| Ratchet thresholds on an existing codebase      | `bca check --config bca-thresholds.toml --baseline .bca-baseline.toml` *(‡)*                                 |
| Inline PR annotations (GitHub)                  | `bca check … --output-format clang-warning --no-fail` + GCC problem matcher                                  |
| Code Scanning alerts (GitHub)                   | `bca check … --output-format sarif --no-fail` + `github/codeql-action/upload-sarif`                          |
| Merge-request widget (GitLab Code Quality)      | `bca check … --output-format code-climate --no-fail`                                                         |
| Jenkins / SonarQube ingestion                   | `bca check … --output-format checkstyle`                                                                     |
| Human-readable PR/MR comment or downloadable    | `bca report markdown --top 20 --strip-prefix "$PWD/"`                                                        |
| Machine-readable artifact for dashboards        | `bca metrics --output-format json --output ./out`                                                            |

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

### Live worked example

`big-code-analysis` runs the recipes below against its own source on
every push and PR. The workflow source —
[`.github/workflows/pages.yml`](https://github.com/dekobon/big-code-analysis/blob/main/.github/workflows/pages.yml) —
exercises the GitHub-Releases install path, the cache, the
baseline-ratcheted gate, and both report formats. The output sits on
GitHub Pages alongside this book:

- HTML hotspot report:
  <https://dekobon.github.io/big-code-analysis/reports/index.html>
- Markdown PR/MR comment:
  <https://dekobon.github.io/big-code-analysis/reports/report.md>

Copy snippets below straight into your own workflow; the `bca` version
quoted is the latest published release at the time of writing.

### Threshold gate, SARIF, and clang-warning matcher

The three pre-existing recipes — hard threshold gate, SARIF upload to
Code Scanning, and `clang-warning` + GCC problem matcher for inline PR
annotations — live in the
[Check command page](../commands/check.md#ci-example-github-actions).
Use the link rather than re-implementing them here.

### Installing `bca` from a GitHub Release (recommended)

The fastest, most reproducible install path is the prebuilt tarball
from this repository's [GitHub Releases](https://github.com/dekobon/big-code-analysis/releases).
It is a single `curl | sha256sum | tar`, requires no Rust toolchain,
and produces byte-identical binaries across runs. Pair it with
[`actions/cache`](https://github.com/actions/cache) keyed by version
so a green-path rerun skips the download entirely:

```yaml
env:
  BCA_VERSION: "1.1.0"
  BCA_TARGET:  "x86_64-unknown-linux-gnu"
  # sha256 of big-code-analysis-${BCA_VERSION}-${BCA_TARGET}.tar.gz from the
  # release's SHA256SUMS file. Bump together with BCA_VERSION.
  BCA_SHA256:  "f11c324fd80787e1a9edf99d3c1763980e035e51abb5479527b14b1e2f83e919"

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

### Alternative: `cargo install` via prebuilt-aware actions

When you cannot reach `github.com` from a runner (air-gapped, custom
mirror) but can reach crates.io, the following two actions fall back
transparently to `cargo install` when no prebuilt is published — at
the cost of compile time on the cold path. Both pin to the same
crates.io release as the GitHub Releases assets:

```yaml
# Option 1: taiki-e/install-action
- name: Install bca
  uses: taiki-e/install-action@v2
  with:
    tool: big-code-analysis-cli@1.1.0
```

```yaml
# Option 2: cargo-binstall
- name: Install cargo-binstall
  uses: cargo-bins/cargo-binstall@main
- name: Install bca
  run: cargo binstall --no-confirm big-code-analysis-cli --version 1.1.0
```

If either action falls back to compilation, cache the cargo registry +
the installed binary so the second run is fast:

```yaml
- name: Cache cargo registry and bca binary
  uses: actions/cache@v5
  with:
    path: |
      ~/.cargo/registry
      ~/.cargo/git
      ~/.cargo/bin/bca
    # crates.io publishes immutable releases, so a `<version>` key is
    # sufficient here — there is no sha256 to rotate. (The GitHub
    # Releases install path above is different: republished release
    # assets share a version, so its cache key must include the sha256.)
    key: bca-${{ runner.os }}-1.1.0
```

Pin to a specific version (matching a published
`big-code-analysis-cli` release on crates.io) so reports stay
reproducible across runs. A floating install surfaces
metric-counting changes as "mysterious CI flakes" on Mondays.

### Posting the Markdown report as a PR comment

`bca report markdown` is purpose-built for PR/MR comments: a stable
header structure, one row per hot spot, and short paths once you pass
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
          tool: big-code-analysis-cli@1.1.0
      - name: Generate report
        run: |
          bca \
            --paths "$PWD" \
            --num-jobs "$(nproc)" \
            report markdown \
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
(`actions/upload-artifact@v7`) if you want it downloadable from the
workflow run page in addition to the PR comment.

### Baseline / ratchet pattern

`bca check --baseline` is the native ratchet: record today's offenders
in a committed TOML file, fail only on regressions and new offenders,
and shrink the file over time. Bootstrap once, commit, then point CI
at it:

```bash
# Once, on a developer machine. Commit both files.
bca --paths src/ check \
    --config bca-thresholds.toml \
    --write-baseline .bca-baseline.toml
git add bca-thresholds.toml .bca-baseline.toml
```

> **Path-style stickiness.** Baseline entries are keyed by the exact
> path string bca emits at write time. `--paths src/` records
> `src/foo.rs`, `--paths .` records `./src/foo.rs`, and
> `--paths "$PWD"` records the absolute path. The subsequent
> `bca check --baseline` MUST use the same `--paths` form, or every
> entry mismatches and the gate fails on every existing offender.
> Pick one form and apply it consistently in CI and in the bootstrap
> command.

This snippet bootstraps from `src/` only — appropriate for a
single-crate library. For a multi-crate workspace, see the
[live worked example](#live-worked-example): its `.github/workflows/pages.yml`
scans the entire repo with `--exclude-from .bcaignore`, a checked-in
deny-set covering vendored grammars, generated trees, and tests.

> **Share the exclude list across workflow, recipe, and bootstrap.**
> Put the deny-set in a single file at the repo root (a `.bcaignore`
> by convention, mirroring `.gitignore` / `.dockerignore`) and point
> every `bca` invocation at it with `--exclude-from .bcaignore`.
> Patterns from `--exclude-from` are unioned with any inline
> `--exclude <GLOB>` flags into one deny-set — keep `--exclude` for
> one-off ad-hoc excludes. Blank lines and `#`-prefixed comment lines
> in the file are skipped. Patterns follow the same `./`-prefix
> convention as `--exclude` arguments (the walker's emitted form).
> Pair edits to `.bcaignore` with a `--write-baseline` refresh — the
> baseline keys are sensitive to which files the walker visits.

```yaml
- name: Threshold check with baseline
  run: |
    bca --paths src/ check \
        --config bca-thresholds.toml \
        --baseline .bca-baseline.toml
```

A regressed function (`current value > baseline value`) still fails.
A new offender not in the baseline still fails. An improved function
passes silently and stays in the baseline until the next
`--write-baseline` refresh.

Each surviving violation in the stderr stream is prefixed with a tag
so a developer can tell at a glance whether they are looking at a
brand-new offender or a known one that has worsened:

- `[new]` — no baseline entry for this function / metric.
- `[regr +N%]` — current value exceeds the recorded baseline by `N`
  percent. Special forms: `[regr from 0]` when the baseline value
  was zero, `[regr +>9999%]` when the regression exceeds 100× the
  baseline, `[regr NaN]` when the current value is NaN.

After the per-violation lines the stderr stream emits a per-file
rollup footer with the format `<path>: <count> violations (worst:
<metric> = <value> vs limit <limit> at L<start>)`, sorted by
violation count descending. This is intended to be the first thing a
reader looks at: which file has the most problems, and which metric
is the loudest in that file. Pass `--no-summary` to suppress the
footer for downstream tooling that grep-pipes the stderr stream.

Refresh after focused refactors:

```bash
bca --paths src/ check \
    --config bca-thresholds.toml \
    --write-baseline .bca-baseline.toml
git diff .bca-baseline.toml   # expect a shrinking file
```

Two `--write-baseline` runs over an unchanged tree produce
byte-identical output, so spurious diffs only appear when offenders
actually changed. See the [Baselines recipe](baselines.md) for the
full adoption flow, PR-review heuristics, and the suppression
composition rules.

#### Offender-count delta against merge base (stopgap)

For teams who cannot commit a baseline file (e.g. policy reasons), a
coarser approximation counts `<error>` elements in two Checkstyle
documents — one on the merge base, one on the PR head — and fails
when the count grows:

```yaml
- name: Compute offender deltas vs. merge base
  run: |
    set -euo pipefail
    BASE="$(git merge-base origin/main HEAD)"
    git worktree add /tmp/base "$BASE"

    bca --paths /tmp/base check \
        --config bca-thresholds.toml \
        --output-format checkstyle \
        --output /tmp/base.xml \
        --no-fail
    BASE_COUNT=$(grep -c "<error" /tmp/base.xml || true)

    bca --paths "$PWD" check \
        --config bca-thresholds.toml \
        --output-format checkstyle \
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

#### Self-scan threshold gate (local mirror of the CI gate)

CI's threshold gate fires only after push, which is too late if a
refactor silently nudged a metric past its limit. The
`big-code-analysis` repo's
[`Makefile`](https://github.com/dekobon/big-code-analysis/blob/main/Makefile)
exposes four targets that mirror the CI gate (the
[`Threshold gate` step in `.github/workflows/pages.yml`](https://github.com/dekobon/big-code-analysis/blob/main/.github/workflows/pages.yml))
locally and add a second tier at 95% of every limit so encroachment
is caught a commit or two before the hard gate trips:

```bash
make self-scan                            # hard gate, 100% of bca-thresholds.toml
make self-scan-headroom                   # soft gate, default 95% (BCA_HEADROOM)
make self-scan-write-baseline             # refresh baseline at hard thresholds
make self-scan-write-baseline-headroom    # refresh baseline at soft thresholds
```

The hard tier is exactly what CI runs; expanded, it is:

```bash
cargo run --quiet --release -p big-code-analysis-cli -- \
    --paths . --exclude-from .bcaignore \
    check \
    --config bca-thresholds.toml \
    --baseline .bca-baseline.toml
```

Both tiers consume the same `bca-thresholds.toml` and the same
`.bca-baseline.toml`; the soft tier just runs the hard recipe
with every threshold value multiplied by `BCA_HEADROOM`. Both
exit `0` clean, `2` on any threshold violation, `1` on tool
error — the soft tier is a real gate, not advisory, so do not
wrap `make self-scan-headroom` in `|| true`. All four targets
are wired into `make pre-commit`, `make ci`, and
`.pre-commit-config.yaml`, with `self-scan-headroom: self-scan`
as a Make prerequisite so the hard tier always reports a true
regression before the soft tier reports near-limit headroom.

`BCA_HEADROOM=0.90 make self-scan-headroom` widens the band;
`BCA_HEADROOM=0.99` tightens it to the last 1%. When the soft
tier fires, absorb the offender into the baseline with
`make self-scan-write-baseline-headroom` (which records every
offender at the scaled thresholds — strictly a superset of the
hard-tier offenders).

The pattern (hard tier mirroring CI + soft tier as early-warning
band, both ratcheted by the same baseline) is project-agnostic —
the [Local threshold gates recipe](local-gates.md) documents the
underlying principles, drop-in Makefile / `just` / `package.json`
skeletons, and the helper script that scales thresholds, so you
can adopt the same workflow in your own repo. The generic recipe
uses the same `BCA_*` env-var names as the Makefile above, so
overrides like `BCA_HEADROOM=0.90` work identically across both.

## GitLab CI

### Full `.gitlab-ci.yml` example

The job below installs `bca`, runs the threshold check producing
Code Climate JSON (for the MR Code Quality widget), Checkstyle XML,
and a Markdown report, then uploads them as artifacts:

```yaml
stages:
  - quality

variables:
  BCA_VERSION: "1.1.0"  # pin a published big-code-analysis-cli release
  BCA_TARGET:  "x86_64-unknown-linux-gnu"
  # sha256 of big-code-analysis-${BCA_VERSION}-${BCA_TARGET}.tar.gz from
  # the release's SHA256SUMS file. Bump together with BCA_VERSION.
  BCA_SHA256:  "f11c324fd80787e1a9edf99d3c1763980e035e51abb5479527b14b1e2f83e919"

bca-quality:
  stage: quality
  image: debian:stable-slim
  cache:
    # Same key shape as the GitHub Actions snippet — bumping
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
        --paths "$PWD"
        --num-jobs "$(nproc)"
        check
        --config bca-thresholds.toml
        --output-format code-climate
        --output gl-code-quality-report.json
        --no-fail
    - bca
        --paths "$PWD"
        --num-jobs "$(nproc)"
        check
        --config bca-thresholds.toml
        --output-format checkstyle
        --output bca-checkstyle.xml
        --no-fail
    - bca
        --paths "$PWD"
        --num-jobs "$(nproc)"
        report markdown
        --top 20
        --strip-prefix "$PWD/"
        --output bca-report.md
    # The threshold gate runs separately so the artifacts above still
    # publish on failure. Exit 2 = at least one threshold exceeded.
    - bca --paths "$PWD" check --config bca-thresholds.toml
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
- `artifacts:when: always` ensures every artifact is downloadable
  even on a red pipeline — which is exactly when you want them
  most.
- `artifacts:reports:codequality` wires the Code Climate JSON
  directly into GitLab's MR Code Quality widget — see the
  [Code Quality widget section below](#gitlab-code-quality-widget)
  for the field-by-field semantics.

### GitLab Code Quality widget

GitLab's first-class Code Quality experience (inline complaints on
the MR diff, summary on the MR overview page) consumes
[Code Climate JSON](https://docs.gitlab.com/ci/testing/code_quality/).
`bca check` emits this natively via `--output-format code-climate`,
so the integration is a one-liner:

```yaml
code_quality:
  stage: quality
  script:
    - bca --paths "$CI_PROJECT_DIR" check
          --config bca-thresholds.toml
          --output-format code-climate
          --output gl-code-quality-report.json
          --no-fail
  artifacts:
    when: always
    reports:
      codequality: gl-code-quality-report.json
    paths:
      - gl-code-quality-report.json
```

Severity bands are derived from how far each metric exceeds its
configured threshold (`value / limit` ratio, inverted for the
maintainability-index family where lower is worse): `≤ 1.5×` →
`minor`, `≤ 2×` → `major`, `≤ 4×` → `critical`, `> 4×` →
`blocker`. The widget deduplicates findings by `fingerprint`; `bca`
hashes `path \0 function \0 metric` (no line, no value) so a
violation surviving an upstream line-drift edit still collapses
into the same widget entry across pipeline runs.

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

Both Jenkins (via the *Warnings Next Generation* plugin) and
SonarQube (via its *Generic Issue* importer) consume Checkstyle 4.3
XML directly. The same invocation feeds both:

```bash
bca --paths src/ check \
    --config bca-thresholds.toml \
    --output-format checkstyle \
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
  surfaces metric-counting changes as "mysterious CI flakes" on
  Mondays.
- **Use `--num-jobs "$(nproc)"`.** The walker is CPU-bound on
  modern hardware; `--num-jobs 1` is a debugging knob, not a
  default.
- **Always pass `--strip-prefix "$PWD/"` to `bca report markdown`**
  so the path column is identical across runners with different
  workspace paths. Without it the diff between two reports is
  dominated by `/home/runner/work/...` vs.
  `/builds/group/project/...` noise.
- **Store `bca-thresholds.toml` at the repo root**, alongside
  `Cargo.toml` / `pyproject.toml` / `package.json`. Treat it as
  source: review threshold relaxations in code review.
- **Exit-code contract.** `bca check` exits `0` clean, `2` on any
  threshold violation, `1` on tool error (bad config, unknown
  metric, unreadable path). Reserving `1` for tool errors lets CI
  distinguish "a function got too complex" from "the analyzer
  crashed".
- **Honor in-source suppression markers, audit with
  `--no-suppress`.** The default `bca check` honors
  [`bca: suppress` / `bca: suppress-file` markers](../commands/suppression.md);
  passing `--no-suppress` ignores them so auditors see the raw
  offender list.
