# CI integration

Recipes for wiring `bca` into a build pipeline. The
[`bca check`](../commands/check.md) command already ships every output
shape a modern CI needs (Checkstyle, SARIF, clang/GCC warning lines,
MSVC warning lines), plus [`bca report markdown`](../commands/report.md)
for humans. This page is a consolidated map from the user's *goal* to
the right combination of subcommand, flags, and platform glue.

## Picking outputs

The matrix below maps each common goal to the `bca` invocation that
feeds the corresponding CI surface. Linked sections below have the
runnable example.

| Goal                                            | Command + flags                                                                                              |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Hard gate on threshold regressions              | `bca check --config bca-thresholds.toml`                                                                     |
| Inline PR annotations (GitHub)                  | `bca check … --output-format clang-warning --no-fail` + GCC problem matcher                                  |
| Code Scanning alerts (GitHub)                   | `bca check … --output-format sarif --no-fail` + `github/codeql-action/upload-sarif`                          |
| Merge-request widget (GitLab Code Quality)      | `bca check … --output-format checkstyle --no-fail` + Checkstyle-to-Code-Climate-JSON converter *(†)*         |
| Jenkins / SonarQube ingestion                   | `bca check … --output-format checkstyle`                                                                     |
| Human-readable PR/MR comment or downloadable    | `bca report markdown --top 20 --strip-prefix "$PWD/"`                                                        |
| Machine-readable artifact for dashboards        | `bca metrics --output-format json --output ./out`                                                            |

*(†) GitLab's native Code Quality widget consumes **Code Climate JSON**,
not Checkstyle. `bca check` does not emit Code Climate JSON yet; see
[GitLab Code Quality widget](#gitlab-code-quality-widget) below for the
converter recipe and current gap.*

The full reference for `bca check`'s output formats, exit codes
(`0` clean, `2` violation, `1` tool error), and threshold config lives
in the [Check command page](../commands/check.md). For the Markdown
report shape, see the [Report command page](../commands/report.md) and
the [Quality reports recipe](quality-reports.md).

## GitHub Actions

### Threshold gate, SARIF, and clang-warning matcher

The three pre-existing recipes — hard threshold gate, SARIF upload to
Code Scanning, and `clang-warning` + GCC problem matcher for inline PR
annotations — live in the
[Check command page](../commands/check.md#ci-example-github-actions).
Use the link rather than re-implementing them here.

### Caching the `cargo install` of `bca`

Compiling `bca` from source on every push wastes minutes. Two options
keep the install path fast and reproducible:

```yaml
# Option 1: prebuilt binaries via taiki-e/install-action
- name: Install bca
  uses: taiki-e/install-action@v2
  with:
    tool: big-code-analysis-cli@<VERSION>
```

```yaml
# Option 2: cargo-binstall, which falls back to source build if no
# prebuilt artifact is published for the requested version
- name: Install cargo-binstall
  uses: cargo-bins/cargo-binstall@main
- name: Install bca
  run: cargo binstall --no-confirm big-code-analysis-cli --version <VERSION>
```

Pin to a specific `<VERSION>` (the published `big-code-analysis-cli`
crate version on crates.io) so reports stay reproducible across
runs. If a prebuilt binary is not yet published for your platform,
both actions transparently fall back to `cargo install`, which is
where the cargo registry cache earns its keep:

```yaml
- name: Cache cargo registry and bca binary
  uses: actions/cache@v4
  with:
    path: |
      ~/.cargo/registry
      ~/.cargo/git
      ~/.cargo/bin/bca
    key: bca-${{ runner.os }}-<VERSION>
```

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
          tool: big-code-analysis-cli@<VERSION>
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
(`actions/upload-artifact@v4`) if you want it downloadable from the
workflow run page in addition to the PR comment.

### Baseline / ratchet pattern

`bca check` does not natively diff offender counts between two refs.
The ratchet pattern below runs `check --output-format checkstyle
--no-fail` on the merge base and on the PR head, counts `<error>`
elements in each Checkstyle document, and fails only when the count
grows:

```yaml
- name: Compute offender deltas vs. merge base
  run: |
    set -euo pipefail
    BASE="$(git merge-base origin/main HEAD)"
    git worktree add /tmp/base "$BASE"

    # Baseline: count offenders on the merge base.
    bca --paths /tmp/base check \
        --config bca-thresholds.toml \
        --output-format checkstyle \
        --output /tmp/base.xml \
        --no-fail
    BASE_COUNT=$(grep -c "<error" /tmp/base.xml || true)

    # Head: count offenders on this PR.
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

The ratchet is intentionally coarse: it counts violations, not
their identity, so renaming an offender does not register as a
regression. For a per-offender diff, drop both Checkstyle XML files
into the same artifact and let humans review.

The shape above is the documented path until `bca check` gains a
native `--baseline` flag (filed as a follow-up).

## GitLab CI

### Full `.gitlab-ci.yml` example

The job below installs `bca`, runs the threshold check producing
both Checkstyle XML and a Markdown report, uploads them as artifacts,
and exposes the Checkstyle XML through GitLab's Code Quality report
slot:

```yaml
stages:
  - quality

variables:
  BCA_VERSION: "<VERSION>"  # pin a published big-code-analysis-cli release

bca-quality:
  stage: quality
  image: rust:1-slim
  before_script:
    - cargo install big-code-analysis-cli --version "$BCA_VERSION" --locked
  script:
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
    paths:
      - bca-checkstyle.xml
      - bca-report.md
```

A few notes about the example:

- The first `bca check … --no-fail` invocation collects offenders
  for the artifacts; the final `bca check` (no `--no-fail`) is the
  pass/fail gate. Both runs use the same threshold config so the
  artifacts always match the gate decision.
- `artifacts:when: always` ensures the Markdown report and
  Checkstyle XML are downloadable even on a red pipeline — which is
  exactly when you want them most.
- `artifacts:reports:codequality` is intentionally omitted: that slot
  expects Code Climate JSON, not Checkstyle XML — see the
  [Code Quality widget section below](#gitlab-code-quality-widget)
  for the converter recipe that lights up the MR widget.

### GitLab Code Quality widget

GitLab's first-class Code Quality experience (inline complaints on
the MR diff, summary on the MR overview page) requires
[Code Climate JSON](https://docs.gitlab.com/ee/ci/testing/code_quality.html),
**not** Checkstyle XML. `bca check` does not currently emit Code
Climate JSON; a follow-up issue tracks adding it as a fifth
`--output-format` value.

Until then there are two paths:

**1. Artifact-only (works today, no widget integration).** Publish
the Checkstyle XML as a generic artifact (the example above already
does this — no `reports:codequality` slot) and let developers
download it from the pipeline page. The Markdown report fills the
human-readable role.

**2. Convert Checkstyle to Code Climate JSON.** Pipe the XML
through a short converter so the Code Quality widget lights up. A
minimal jq-style converter:

```bash
bca --paths "$PWD" check \
    --config bca-thresholds.toml \
    --output-format checkstyle \
    --output bca-checkstyle.xml \
    --no-fail

# Convert Checkstyle XML to Code Climate JSON. Adjust to taste; the
# point is the shape required by GitLab.
python3 - <<'PY' > gl-code-quality-report.json
import hashlib, json, xml.etree.ElementTree as ET
issues = []
for file_el in ET.parse("bca-checkstyle.xml").getroot().findall("file"):
    path = file_el.get("name", "")
    for err in file_el.findall("error"):
        msg = err.get("message", "")
        line = int(err.get("line", "1"))
        fp = hashlib.sha1(f"{path}:{line}:{msg}".encode()).hexdigest()
        issues.append({
            "description": msg,
            "check_name": err.get("source", "bca"),
            "fingerprint": fp,
            "severity": "minor",
            "location": {"path": path, "lines": {"begin": line}},
        })
print(json.dumps(issues))
PY
```

Then reference `gl-code-quality-report.json` under
`artifacts:reports:codequality` instead of the Checkstyle XML. The
converter is intentionally small (≈15 lines) so it can live in the
repo next to `bca-thresholds.toml`.

This documents the gap rather than papering over it. When `bca`
gains a `codeclimate-json` format, the converter step will be
deletable.

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
