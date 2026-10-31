# Temp Files and Evidence Rule

Every file an agent writes outside the tree is in one of three tiers,
and the tier decides the directory. Decide it *before* the first
write: a file parked in the wrong tier is the one that is gone when
you need it, and nothing warns you at the time.

## The three tiers

Work down the list; the first match wins.

### 1. Cannot be regenerated

A hand-written oracle fixture, a perturbation driver, a comparison
harness, a captured output whose producer no longer exists. If losing
it would leave a claim nobody can re-measure, it does not belong in a
temp directory at all.

- A **fixture** that pins a behaviour becomes a regression test under
  `src/` or `tests/`, where it is run and gated.
- A **driver or harness** that is reusable across issues goes under
  `utils/`, where it is linted and self-tested like the gates there.
- Anything that fits neither and is ≤ 25 MB compressed is **attached
  to the issue or PR** that cites it, so the claim and its inputs travel
  together.
- **Otherwise, stop and ask the user.** That covers a file too large to
  attach, one with sensitive or third-party content, one with no issue
  to attach it to, and a per-issue driver that is not obviously
  worth a permanent home (the #1710 workflow harness was kept out of
  the tree deliberately). Keep the only copy where it is until you have
  an answer — "do not park it in `/tmp`" never means delete it.

### 2. Regenerable, but expensive or needed across sessions

A base-vs-head corpus dump, a coverage profile, a built binary for a
superseded commit, a benchmark log you will compare against next
week. Everything here can be rebuilt from the tree and a command, so
the risk is cost, not loss — which is what `/var/tmp` is for.

```zsh
issue_dir=/var/tmp/bca/<issue>
mkdir -p "$issue_dir"
run_dir=$(mktemp -d -p "$issue_dir" '<purpose>.XXXXXX')
```

Three things that shape earns:

- **`/var/tmp/bca/<issue>/` is deterministic** so the next session can
  find it without being told; `mktemp -d` *inside* it
  keeps two concurrent runs — two agents on the same issue, or a
  re-run beside its predecessor — from overwriting each other. Name
  the run directory after what it holds (`corpus-base.XXXXXX`,
  `coverage.XXXXXX`, `bench.XXXXXX`), not after the tool.
- **Check the root before trusting it.** `/var/tmp` is sticky and
  world-writable, and `mkdir -p` succeeds on a directory someone else
  owns. On a shared host, `[ -O /var/tmp/bca ] && [ ! -L /var/tmp/bca ]`
  must hold; if it does not, stop rather than write into it.
- **Every run directory carries a `PROVENANCE.md`** naming the commit,
  the command that produced each file, and what to run to regenerate
  it. A cache without that is a pile of numbers; with it, the cache is
  optional.

**A claim never cites this directory.** `AGENTS.md`'s "Support for a
claim goes where the claim is" governs what an issue or PR comment
points at — the numbers, the revisions, the exact command, a fixture
in the tree — and a path on one host is a citation to nothing. This
tier is the working cache that lets *you* avoid re-running the command
tomorrow; the `PROVENANCE.md` is what makes the cache disposable once
the comment carries the command. `/var/tmp` survives a reboot but is
not archival: systemd's upstream `tmpfiles.d/tmp.conf` ages it at 30
days (`q /var/tmp 1777 root root 30d`); Ubuntu ships that line
commented out, and other distributions may not. Check the host's
`tmpfiles.d` when a run needs to outlive that, and ask the user when
nothing on the host can keep it long enough.

### 3. Easily regenerated, truly temporary

A pre-commit log, an issue body on its way to `gh --body-file`, a
one-shot `bca` dump, a `git archive` scratch tree. These go in `/tmp`
and are allowed to disappear on the next boot.

```zsh
log=$(mktemp "${TMPDIR:-/tmp}/bca-pre-commit.XXXXXX.log")
body=$(mktemp "${TMPDIR:-/tmp}/bca-issue-body.XXXXXX.md")
scratch=$(mktemp -d "${TMPDIR:-/tmp}/bca-scan.XXXXXX")
```

- **Always `mktemp`, never a fixed name.** `/tmp/pc.log`,
  `/tmp/issue-body.md` and `/tmp/scan-metrics.json` are shared by every
  checkout, worktree and agent on the host; one agent has already
  diagnosed another's `fmt-check` failure from a shared log
  (`CONTRIBUTING.md`, "Reading the verdict").
- **The template names the source.** `bca-<purpose>.XXXXXX[.ext]` reads
  in `ls /tmp` as what it is and who wrote it; GNU `mktemp` accepts the
  suffix after the `X`s, so the extension can stay for tools that key
  on it. Quote the whole template: `${TMPDIR:-/tmp}` is a parameter
  expansion, and unquoted it is subject to word-splitting and globbing
  in bash.
- **A harness-preferred scratch directory is still tier 3.**
  `/tmp/opencode/`, `$TMPDIR`, and plain `/tmp` are interchangeable
  here — honour `TMPDIR` when it is set, and do not keep anything in
  any of them that the next session needs.
- **Hand a random path forward explicitly.** A skill that allocates a
  directory in step 2 and reads from it in step 8 must print the
  resolved absolute path and pass it to every later step and
  subagent; a variable does not survive a tool-call boundary.

## Typical placements

The tier is a property of the *need*, not the file type: a pre-commit
log you must compare against next week is tier 2, and a review diff
holding edits that exist nowhere else is tier 1. The table is where
each usually lands.

| Artifact | Usual tier | Where |
| --- | --- | --- |
| `make pre-commit` log | 3 | `mktemp "${TMPDIR:-/tmp}/bca-pre-commit.XXXXXX.log"` |
| `gh` issue / PR body, comment | 3 | `mktemp "${TMPDIR:-/tmp}/bca-issue-body.XXXXXX.md"` |
| `bca` corpus dumps for a base/head diff | 2 | `/var/tmp/bca/<issue>/corpus-<side>.XXXXXX/` |
| `llvm-cov` JSON, patch-coverage report | 2 | `/var/tmp/bca/<issue>/coverage.XXXXXX/` |
| base / head `bca` binaries, worktree checkouts | 2 | `/var/tmp/bca/<issue>/bin.XXXXXX/` |
| bench-scaling logs kept for comparison | 2 | `/var/tmp/bca/<issue>/bench.XXXXXX/` |
| perturbation driver, comparison harness | 1 | `utils/`, or attached to the issue |
| hand-written oracle fixture | 1 | a regression test; interim copies may sit beside the tier-2 run that used them |
| review-packet diff of *committed* work | 3 | `mktemp "${TMPDIR:-/tmp}/bca-review.XXXXXX.diff"` |

## What it cost here

Two fixture files of the #1696 oracle lived only in `/tmp/opencode/`
and were lost to a host restart mid-review. They were rebuilt from the
recorded verbatim exports and dumps — possible only because the
outputs had been kept, and the outputs happened to encode the inputs.
The #1710 workflow harness sat under the earlier `~/bca-evidence/`
convention beside the captures it produced, a check that would never
run again; it is `utils/mutation-workflow-test.py` now, under `make
mutation-workflow-test` — the tier-1 move this rule asks for, made
after the fact.

`~/bca-evidence/<issue>/` was the convention before this rule. It was
one directory for every tier at once, which is why 196 GB of
regenerable corpus output sat beside a handful of irreplaceable
drivers with nothing to tell them apart. `AGENTS.md` retired it as a
*citation* target; this rule retires it as a *location*. Its contents
moved to `/var/tmp/bca/<issue>/` when this rule landed, and the tier-1
pieces still there are listed as follow-ups on the PR that moved them.
