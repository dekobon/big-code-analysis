# Local threshold gates

CI is the last line of defence, not the first. By the time
`bca check --config bca-thresholds.toml --baseline .bca-baseline.toml`
fires red on a pull request, the offending change has already been
pushed, the author has context-switched, and someone has to revisit
the diff to nudge a metric back under its limit. A local threshold
gate moves that feedback to the moment of `git commit` — the same
moment `cargo fmt --check` and `cargo clippy -- -D warnings` already
fire — so the regression never makes it past the developer's
keyboard.

This recipe captures the pattern `big-code-analysis` uses on its own
source ([`Makefile`'s `self-scan*` targets](https://github.com/dekobon/big-code-analysis/blob/main/Makefile))
and distils it into something you can drop into your own repo's
`Makefile`, `justfile`, `package.json` script, or `pre-commit`
config. The underlying idea is provider-neutral: any threshold
checker (`bca`, ESLint, clippy, SonarLint, Qodana) can be wired the
same way.

## Principles

Three principles drive the design. They are not specific to `bca`;
they are the same conclusions Sonar reached when it pivoted its
default Quality Gate to focus on
[new code](https://docs.sonarsource.com/sonarqube-server/quality-standards-administration/managing-quality-gates/introduction-to-quality-gates)
and that the broader ratchet pattern formalises.

1. **Gate locally, mirror CI exactly.** The local gate must run the
   same binary with the same arguments and the same threshold /
   baseline / exclude files as CI. If the local gate is "almost
   what CI runs", it stops catching regressions the moment one
   diverges from the other. The cost of running the gate once before
   pushing is cheap; the cost of a red PR-bot ping is not.
2. **Ratchet, don't reset.** When you introduce thresholds on an
   existing codebase, *every* reasonable limit fires on dozens of
   pre-existing functions. The realistic adoption path is "absorb
   today's offenders into a baseline file, fail only on new or
   worsening ones, shrink the baseline over time". This is the same
   strategy that lets a multi-year codebase introduce strict TypeScript
   or strict clippy lints without a months-long boil-the-ocean pass.
   See the [Baselines recipe](baselines.md) for the bootstrap → CI
   → refresh → retire flow.
3. **Warn before you fail.** A hard 100% gate fails *at* the limit
   and gives no signal as a function creeps from 80% to 95% to 99%
   of its threshold. A second, looser tier that fires at e.g. 95%
   of every limit gives a one-or-two-commit early warning. The
   author still has the file open, the test cases in their head,
   and the freedom to refactor before the offender hardens into
   "well, it's in main now". Sonar's "new code" Quality Gate, the
   GCC `-Wall` / `-Werror` split, and clippy's `warn` vs. `deny`
   lint levels all encode the same insight: a tier between *clean*
   and *broken* is where teams actually catch drift.

## The two tiers

The pattern is two recipes wrapping the same checker, plus two
recipes for refreshing the baseline at each tier.

| Target                                | Tier | Thresholds            | Baseline-filtered | Use case                                            |
| ------------------------------------- | ---- | --------------------- | ----------------- | --------------------------------------------------- |
| `self-scan`                           | hard | 100% of config        | yes               | Mirror of CI. Must stay green on every commit.      |
| `self-scan-headroom`                  | soft | config × `HEADROOM`   | yes               | Early-warning band. Fires before the hard tier.     |
| `self-scan-write-baseline`            | hard | 100% of config        | (write)           | Absorb today's hard-tier offenders.                 |
| `self-scan-write-baseline-headroom`   | soft | config × `HEADROOM`   | (write)           | Absorb soft-tier offenders when launching or widening the band. |

The hard tier and the soft tier consume the **same**
`bca-thresholds.toml` and the **same** `.bca-baseline.toml`. The
only difference between them is a scalar multiplier applied to
every threshold value before `bca check` sees it.

This matters: it means a contributor who wants the soft tier to be
stricter (catch encroachment further out) bumps a single
environment variable rather than maintaining a parallel
`bca-thresholds-soft.toml` that will drift out of sync with the
hard config the first time anyone forgets to update both files.

## Zero-config: the `bca.toml` manifest

Rather than thread `--paths`, `--exclude-from`, `--num-jobs`,
`--config`, `--baseline`, and `--headroom` through every recipe,
drop a `bca.toml` at the repo root and let `bca check` discover it:

```toml
# bca.toml — discovered automatically at (or above) the working dir.
paths        = ["."]
exclude_from = ".bcaignore"
num_jobs     = "auto"          # or an integer
baseline     = ".bca-baseline.toml"

[thresholds]
cognitive    = 25
cyclomatic   = 15
"halstead.effort" = 50000
nom          = 30
nargs        = 7
nexits       = 5
abc          = 50
wmc          = 60
```

> Leave `headroom` **out** of the manifest. A top-level `headroom`
> applies to *every* `bca check` run, which would silently scale the
> hard tier too. Keep the soft-tier scale on the soft recipe's
> `--headroom` flag (below) so `bca check` stays the exact CI mirror.
> (Once per-metric `[thresholds.soft]` and `--tier=soft` land, the
> manifest gains a tier-scoped home for soft limits.)

With that file in place the four recipes collapse to one flag each:

<!-- rumdl-disable MD010 -->

```make
.PHONY: self-scan self-scan-headroom \
        self-scan-write-baseline self-scan-write-baseline-headroom

self-scan:                          # hard tier (CI mirror)
	bca check
self-scan-headroom:                 # soft tier (early warning)
	bca check --headroom 0.95
self-scan-write-baseline:           # absorb hard-tier offenders
	bca check --write-baseline .bca-baseline.toml
self-scan-write-baseline-headroom:  # absorb soft-tier offenders
	bca check --headroom 0.95 --write-baseline .bca-baseline.toml
```

<!-- rumdl-enable MD010 -->

### Discovery and precedence

- `bca` climbs from the working directory to the repo root (the
  directory containing `.git`) looking for `bca.toml`; the first
  match wins. Relative paths inside the manifest resolve against the
  manifest's own directory, so a `bca.toml` above the current
  directory still points at the right files.
- **CLI flags always win.** Any explicit `--paths`, `--baseline`,
  `--headroom`, etc. overrides the corresponding manifest key.
  `--config <file>` *merges* on top of the manifest `[thresholds]`
  table (config keys win on collision), and repeated
  `--threshold name=value` flags apply last as absolute limits. The
  full resolution order — `[thresholds]` → `--config` → `--headroom`
  scaling → `--threshold` overrides — is shared across all of
  `--config` / `--headroom` / the manifest.
- `--no-config` skips discovery entirely, for reproducible
  fully-explicit invocations that must not pick up repo-level config.
  `bca init` also ignores any existing manifest — it scaffolds config
  rather than consuming it.
- The top-level `include` / `exclude` keys are the global file-filter
  globs (the `--include` / `--exclude` flags) that decide which files
  are *analysed at all*. They are distinct from the forthcoming
  `[check] exclude` table (analysed-but-ungated paths, tracked under a
  separate issue).
- Unrecognized keys (forthcoming options such as `[thresholds.soft]`
  and `[check]`) are ignored with a one-line warning, so you can
  pre-adopt schema additions without breaking older `bca` builds.
- `bca check --print-effective-config` prints the resolved view,
  including a `manifest` provenance line, so you can see exactly what
  the merge produced.

> The explicit-flag skeletons below remain fully supported — the
> manifest is sugar over the same flags, not a replacement. Reach for
> them when you can't drop a file at the repo root, or when one CI job
> needs a different layout than the committed manifest (pair the flags
> with `--no-config`).

## Skeleton: GNU Make (explicit flags)

The four recipes below are a self-contained drop-in that thread every
flag explicitly — the long form of the manifest recipe above. Adjust
the `BCA` variable to point at whatever invocation gives you the
checker (a pinned release binary, `cargo run --release`, an npm /
pip wrapper). Adjust `PATHS` and `EXCLUDE_FROM` to match your
layout.

<!-- rumdl-disable MD010 -->

```make
# --- bca local threshold gates ------------------------------------------
# HARD tier mirrors CI exactly. Both tiers consume the same
# bca-thresholds.toml + .bca-baseline.toml; the soft tier scales every
# threshold by $(BCA_HEADROOM) (default 0.95).
#
# Knobs are namespaced with `BCA_` so they don't collide with anything
# else in your environment. The big-code-analysis repo's own Makefile
# uses the same names — this skeleton is drop-in for that project too.
BCA               := bca
BCA_PATHS         := .
BCA_EXCLUDE_FROM  := .bcaignore
BCA_THRESHOLDS    := bca-thresholds.toml
BCA_BASELINE      := .bca-baseline.toml
BCA_HEADROOM      ?= 0.95

# Common args, factored out so the four recipes stay in lockstep.
# `--num-jobs` defaults to the OS-reported effective CPU count
# (cgroup-/cpuset-aware on Linux), so no `$(nproc)` plumbing is
# needed. Override with `--num-jobs N` (or `--num-jobs 1` to force
# serial mode for debugging).
BCA_BASE_ARGS := --paths $(BCA_PATHS) --exclude-from $(BCA_EXCLUDE_FROM)

.PHONY: self-scan self-scan-headroom \
        self-scan-write-baseline self-scan-write-baseline-headroom

self-scan:
	@echo "bca self-scan (hard gate)..."
	@$(BCA) $(BCA_BASE_ARGS) check \
	  --config $(BCA_THRESHOLDS) \
	  --baseline $(BCA_BASELINE)

# `self-scan-headroom: self-scan` is intentional: under `make -j` Make
# would otherwise run both gates in parallel and the soft tier's scaled
# error message could land before the true regression on the hard tier.
# `--headroom $(BCA_HEADROOM)` scales every config limit before the
# offender comparison — no helper script, no second TOML file.
self-scan-headroom: self-scan
	@echo "bca self-scan (soft gate, BCA_HEADROOM=$(BCA_HEADROOM))..."
	@$(BCA) $(BCA_BASE_ARGS) check \
	  --config $(BCA_THRESHOLDS) \
	  --headroom $(BCA_HEADROOM) \
	  --baseline $(BCA_BASELINE)

self-scan-write-baseline:
	@echo "Refreshing $(BCA_BASELINE) at hard thresholds..."
	@$(BCA) $(BCA_BASE_ARGS) check \
	  --config $(BCA_THRESHOLDS) \
	  --write-baseline $(BCA_BASELINE)

# Soft-tier baseline write. NOTE: this and `self-scan-write-baseline`
# both write `$(BCA_BASELINE)`; never compose them as parallel
# prerequisites of one umbrella target or invoke them with `make -j2`,
# or the two `bca` processes will race on the same file and the
# losing tier's offenders will silently vanish from the baseline.
# Run them sequentially (hard first, then soft) and commit the diff.
self-scan-write-baseline-headroom:
	@echo "Refreshing $(BCA_BASELINE) at soft thresholds (BCA_HEADROOM=$(BCA_HEADROOM))..."
	@$(BCA) $(BCA_BASE_ARGS) check \
	  --config $(BCA_THRESHOLDS) \
	  --headroom $(BCA_HEADROOM) \
	  --write-baseline $(BCA_BASELINE)
```

<!-- rumdl-enable MD010 -->

`bca check --headroom <ratio>` scales every limit from `--config`
by the ratio (default `0.95`) before the offender comparison, then
filters against the same `.bca-baseline.toml` the hard tier writes.
Explicit `--threshold name=value` overrides are absolute and are
not rescaled. There is no separate helper script or second TOML
file to maintain — the soft tier is the hard-tier invocation plus
one flag.

The gate exit codes propagate verbatim from `bca check`: **`0`
clean, `2` on any threshold violation (hard or soft), `1` on tool
error**. The soft tier is a real gate — never wrap
`make self-scan-headroom` in `|| true` thinking it's advisory; the
non-zero exit is the whole point of the encroachment band.

### Wiring into pre-commit and CI

Add the soft gate to whatever umbrella target your developers
already run before pushing. The hard gate runs as its prerequisite
(see the `self-scan-headroom: self-scan` edge above), so listing
only the soft target is enough — and crucially survives
`make -j`, which would otherwise schedule both leaves in parallel
and interleave their output:

```make
.PHONY: pre-commit
pre-commit: fmt-check clippy test self-scan-headroom
```

Ordering matters: the hard tier names a true regression with the
100% limit, not the scaled one. The prerequisite edge enforces
that order even under parallel Make.

In CI, run **only** the hard tier:

```yaml
- name: Threshold gate
  run: make self-scan
```

The soft tier is a developer feedback knob, not a release gate.
Running it in CI either duplicates the hard tier (when nothing has
encroached) or fires noisily on a baseline-absorbed offender that
crept upward without crossing 100% — neither buys you anything CI
doesn't already cover.

## The headroom knob

`BCA_HEADROOM` is a single scalar in `(0, 1]`. The interesting band
is narrow:

| `BCA_HEADROOM` | Fires when a function reaches… | Use case                                            |
| -------------- | ------------------------------ | --------------------------------------------------- |
| `0.99`         | 99% of any limit               | Tightest possible warning, fires on the last commit before the hard gate would. |
| `0.95`         | 95% of any limit (default)     | One-or-two-commit lead time. Good default.          |
| `0.90`         | 90% of any limit               | Wider band — useful immediately after raising a limit, while the new ceiling settles. |
| `1.00`         | 100% (parity with hard gate)   | Sanity check that the two tiers agree.              |

Values below ~0.80 turn the soft tier into a second hard tier with
arbitrary numbers and stop being useful: every threshold has *some*
function near 80% of it on a real codebase, and the soft tier
becomes a permanent baseline-management chore rather than an
early-warning signal.

### When the soft tier fires

A failed soft gate is a decision point, not a bug report. There
are exactly three legitimate resolutions:

1. **Refactor.** Same workflow as any other complexity regression —
   extract a helper, collapse a dispatch arm, split the function.
   This is the common case, and the soft tier exists to give you
   the time to do it on the same branch.
2. **Raise the limit.** Edit `bca-thresholds.toml`, leave a
   why-comment explaining what changed (a new language module, a
   genuine algorithmic floor, a re-classified macro). Re-run
   `make self-scan-headroom` to confirm the new value covers the
   offender with room to spare.
3. **Absorb into the baseline.** Run
   `make self-scan-write-baseline` (hard tier) or
   `make self-scan-write-baseline-headroom` (soft tier) when the
   value is legitimate forever — a parser dispatch arm whose width
   matches the grammar it covers, a stable state machine, generated
   code. Commit the diff in `.bca-baseline.toml` in the same PR
   as the code that produced it.

Don't pick "raise the limit" silently to make the gate go away.
The committed why-comment is the only audit trail the next reader
has; without it the bumped limit looks indistinguishable from
neglect.

## Skeleton: `justfile`

For projects that prefer [`just`](https://github.com/casey/just):

```just
# bca local threshold gates. Hard tier mirrors CI; soft tier (headroom)
# is local-only early warning.
bca         := "bca"
paths       := "."
exclude     := ".bcaignore"
thresholds  := "bca-thresholds.toml"
baseline    := ".bca-baseline.toml"
headroom    := env_var_or_default("BCA_HEADROOM", "0.95")

# `--num-jobs` defaults to the effective CPU count, so the skeleton
# no longer threads `$(nproc)` through `just` (issue #383). Override
# inline if needed: `just self-scan --num-jobs 1`.
base_args   := "--paths " + paths + " --exclude-from " + exclude

self-scan:
    {{bca}} {{base_args}} \
        check --config {{thresholds}} --baseline {{baseline}}

self-scan-headroom: self-scan
    {{bca}} {{base_args}} \
        check --config {{thresholds}} --headroom {{headroom}} --baseline {{baseline}}

self-scan-write-baseline:
    {{bca}} {{base_args}} \
        check --config {{thresholds}} --write-baseline {{baseline}}

# Like the Make skeleton, never compose this with `self-scan-write-baseline`
# in parallel — they race on the same {{baseline}} file.
self-scan-write-baseline-headroom:
    {{bca}} {{base_args}} \
        check --config {{thresholds}} --headroom {{headroom}} --write-baseline {{baseline}}
```

## Skeleton: `package.json` scripts

For JavaScript projects pulling in `bca` via `npx` or a pinned
binary. `--num-jobs` defaults to the effective CPU count
(cgroup-/cpuset-aware on Linux), so the npm tier no longer needs a
`BCA_NUM_JOBS` env var to produce byte-identical `bca check`
invocations as Make / `just`. Pass `--num-jobs 1` explicitly only
when debugging:

```json
{
  "scripts": {
    "self-scan": "bca --paths . --exclude-from .bcaignore check --config bca-thresholds.toml --baseline .bca-baseline.toml",
    "self-scan-headroom": "bca --paths . --exclude-from .bcaignore check --config bca-thresholds.toml --headroom 0.95 --baseline .bca-baseline.toml",
    "self-scan-write-baseline": "bca --paths . --exclude-from .bcaignore check --config bca-thresholds.toml --write-baseline .bca-baseline.toml",
    "self-scan-write-baseline-headroom": "bca --paths . --exclude-from .bcaignore check --config bca-thresholds.toml --headroom 0.95 --write-baseline .bca-baseline.toml"
  }
}
```

Because the soft tier is now a plain `bca check` invocation, the npm
scripts are byte-identical across shells — no helper script, no
`python3`-vs-`py` alias to paper over, no env-var-vs-shell-expansion
portability traps. To widen the band, edit the literal `0.95` in the
script (or wire it through your task runner of choice); the flag
parses the same on every platform.

Pair with [`husky`](https://typicode.github.io/husky/) or
[`pre-commit`](https://pre-commit.com/) so the same scripts run on
`git commit`.

## Skeleton: `pre-commit` hook

If you use the [`pre-commit`](https://pre-commit.com/) framework
(**version 3.2.0 or newer** — see the version note below), both
tiers are local hooks that shell out to `make`:

```yaml
- repo: local
  hooks:
    - id: bca-self-scan
      name: bca self-scan (hard gate)
      entry: make self-scan
      language: system
      pass_filenames: false
      stages: [pre-commit]
    - id: bca-self-scan-headroom
      name: bca self-scan-headroom (soft gate)
      entry: make self-scan-headroom
      language: system
      pass_filenames: false
      stages: [pre-commit]
```

`pass_filenames: false` is deliberate — `bca` discovers its own
inputs from `--paths` plus the baseline. Letting `pre-commit`
pass the changed files in would shrink the scan to just those
files and miss the cross-file effect of a baseline refresh.

> **Minimum `pre-commit` version 3.2.0.** The `stages:` vocabulary
> was renamed in
> [pre-commit 3.2.0](https://github.com/pre-commit/pre-commit/releases/tag/v3.2.0)
> (March 2024) — `commit` → `pre-commit`, `push` → `pre-push`, etc.
> Older installs (notably RHEL 8 EPEL, Ubuntu 20.04 default
> packages, and any `.pre-commit-config.yaml` pinned to the legacy
> vocabulary) reject `stages: [pre-commit]` as an unknown stage
> name and the hook never registers. If you must support older
> installations, substitute `stages: [commit]`; in mixed fleets,
> pin the framework with `pre-commit --version` ≥ 3.2.0 in the
> dev-tooling docs so this contradiction does not surface
> silently.

## Composition with the broader baseline workflow

The four `self-scan*` targets above are not a replacement for the
documented [Baselines recipe](baselines.md) — they *are* that
recipe, mechanised into developer-machine commands. The same
ordering still applies:

1. **Bootstrap once.** Write the initial thresholds, write the
   initial baseline, commit both.
2. **Gate on every commit.** Hard tier fails on regression; soft
   tier fails on encroachment.
3. **Refresh during focused refactors.** When a function
   legitimately moved (someone *did* pay down debt), regenerate
   the baseline and review the diff.
4. **Retire when empty.** When `.bca-baseline.toml` shrinks to
   just `version = 2`, drop the `--baseline` flag and delete the
   file. The thresholds now stand on their own.

The local tiers shorten the feedback loop on steps 2 and 3 from
"red CI on a pull request" to "red Make recipe before
`git commit` returns". That is the whole pitch.

## Related industry patterns

The hard / soft tier split is one instance of a broader pattern.
If you have used any of the following, the mental model carries
over:

- **Sonar's
  [Quality Gates focused on new code](https://docs.sonarsource.com/sonarqube-server/quality-standards-administration/managing-quality-gates/introduction-to-quality-gates).**
  Old code is held at its current state; *changes* must not make
  things worse. The baseline file is `bca`'s native form of the
  "new code" / "leak period" idea.
- **clippy's `warn`-vs-`deny` lint levels.** A `warn` lint surfaces
  in local builds; the same lint denied with `-D warnings` fails
  CI. The two-tier configuration gives you a place to land
  experimental tighter rules.
- **The
  [ratchet pattern](https://github.com/stewartjarod/baseline)** in
  general migration tooling: record today's count, fail on
  increase, lower the ceiling as the count drops. `bca check`
  ratchets per-function rather than per-pattern, but the
  monotonicity guarantee is the same.
- **`-Wall` + `-Werror` in C/C++.** A first pass with `-Wall`
  reveals the noise; promoting to `-Werror` after the baseline
  reaches zero is the same retirement step as deleting
  `.bca-baseline.toml` once it's empty.
