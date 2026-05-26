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

## Skeleton: GNU Make

The four recipes below are a self-contained drop-in. Adjust the
`BCA` variable to point at whatever invocation gives you the
checker (a pinned release binary, `cargo run --release`, an npm /
pip wrapper). Adjust `PATHS` and `EXCLUDE_FROM` to match your
layout.

<!-- markdownlint-disable MD010 -->

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

# `PY` lets Windows hosts override to `py -3` (the stock python.org
# installer ships `py.exe` and `python.exe` but no `python3` alias).
PY                ?= python3

# Common args, factored out so the four recipes stay in lockstep.
BCA_BASE_ARGS := --paths $(BCA_PATHS) --exclude-from $(BCA_EXCLUDE_FROM) \
                 --num-jobs $(shell nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)

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
# `BCA_THRESHOLDS` / `BCA_BASELINE` are exported because the helper
# reads them from the environment — see "Helper script" below.
self-scan-headroom: self-scan
	@echo "bca self-scan (soft gate, BCA_HEADROOM=$(BCA_HEADROOM))..."
	@BCA_HEADROOM=$(BCA_HEADROOM) \
	  BCA_THRESHOLDS=$(BCA_THRESHOLDS) \
	  BCA_BASELINE=$(BCA_BASELINE) \
	  $(PY) ./utils/bca-self-scan-headroom.py \
	  $(BCA) $(BCA_BASE_ARGS)

self-scan-write-baseline:
	@echo "Refreshing $(BCA_BASELINE) at hard thresholds..."
	@$(BCA) $(BCA_BASE_ARGS) check \
	  --config $(BCA_THRESHOLDS) \
	  --write-baseline $(BCA_BASELINE)

# Soft-tier baseline write. NOTE: this and `self-scan-write-baseline`
# both write `$(BCA_BASELINE)`; never compose them as parallel
# prerequisites of one umbrella target or invoke them with `make -j2`,
# or the two Python processes will race on the same file and the
# losing tier's offenders will silently vanish from the baseline.
# Run them sequentially (hard first, then soft) and commit the diff.
self-scan-write-baseline-headroom:
	@echo "Refreshing $(BCA_BASELINE) at soft thresholds (BCA_HEADROOM=$(BCA_HEADROOM))..."
	@BCA_HEADROOM=$(BCA_HEADROOM) \
	  BCA_THRESHOLDS=$(BCA_THRESHOLDS) \
	  BCA_BASELINE=$(BCA_BASELINE) \
	  BCA_HEADROOM_WRITE_BASELINE=$(BCA_BASELINE) \
	  $(PY) ./utils/bca-self-scan-headroom.py \
	  $(BCA) $(BCA_BASE_ARGS)
```

<!-- markdownlint-enable MD010 -->

The helper (`utils/bca-self-scan-headroom.py`) reads four env vars —
`BCA_HEADROOM` (default `0.95`), `BCA_THRESHOLDS` (default
`bca-thresholds.toml`), `BCA_BASELINE` (default `.bca-baseline.toml`),
and the optional `BCA_HEADROOM_WRITE_BASELINE` switch — multiplies
every value in the thresholds file by the headroom ratio, and
re-emits the limits as `--threshold name=value` flags so `bca check`
sees scaled limits without you having to maintain a second TOML
file. The Make skeleton above exports the first three so renaming
any of those paths in one place propagates to both tiers. See
[Helper script](#helper-script) below for a ready-to-paste
implementation.

The gate exit codes propagate verbatim from `bca check`: **`0`
clean, `2` on any threshold violation (hard or soft), `1` on tool
error**. The soft tier is a real gate — never wrap
`make self-scan-headroom` in `|| true` thinking it's advisory; the
non-zero exit is the whole point of the encroachment band.

> **Keep `--paths` identical across all four recipes.** Baseline
> entries are keyed by the exact path string `bca` emits at write
> time: `--paths .` records `./src/foo.rs`, `--paths src/` records
> `src/foo.rs`, and `--paths "$PWD"` records the absolute path.
> A subsequent `--baseline` invocation that uses a different
> `--paths` form silently mismatches every entry and the gate
> re-fails on every existing offender. The skeletons above all
> use `--paths .` deliberately — if you change it, change it in
> every recipe and refresh `.bca-baseline.toml` once. See
> [Baselines: path identity](baselines.md#limitations) for the
> full caveat.

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
py          := env_var_or_default("PY", "python3")

jobs        := `nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4`
base_args   := "--paths " + paths + " --exclude-from " + exclude + " --num-jobs " + jobs

self-scan:
    {{bca}} {{base_args}} \
        check --config {{thresholds}} --baseline {{baseline}}

self-scan-headroom: self-scan
    BCA_HEADROOM={{headroom}} \
        BCA_THRESHOLDS={{thresholds}} \
        BCA_BASELINE={{baseline}} \
        {{py}} ./utils/bca-self-scan-headroom.py {{bca}} {{base_args}}

self-scan-write-baseline:
    {{bca}} {{base_args}} \
        check --config {{thresholds}} --write-baseline {{baseline}}

# Like the Make skeleton, never compose this with `self-scan-write-baseline`
# in parallel — they race on the same {{baseline}} file.
self-scan-write-baseline-headroom:
    BCA_HEADROOM={{headroom}} \
        BCA_THRESHOLDS={{thresholds}} \
        BCA_BASELINE={{baseline}} \
        BCA_HEADROOM_WRITE_BASELINE={{baseline}} \
        {{py}} ./utils/bca-self-scan-headroom.py {{bca}} {{base_args}}
```

## Skeleton: `package.json` scripts

For JavaScript projects pulling in `bca` via `npx` or a pinned
binary. The `--num-jobs` flag is threaded through via the
`BCA_NUM_JOBS` env var (default in the wrapper script below) so the
npm tier runs the same shape of command as Make / `just` — per
Principle 1, all three skeletons should produce byte-identical
`bca check` invocations:

```json
{
  "scripts": {
    "self-scan": "bca --paths . --exclude-from .bcaignore --num-jobs ${BCA_NUM_JOBS:-4} check --config bca-thresholds.toml --baseline .bca-baseline.toml",
    "self-scan-headroom": "npm run self-scan && python3 ./utils/bca-self-scan-headroom.py bca --paths . --exclude-from .bcaignore --num-jobs ${BCA_NUM_JOBS:-4}",
    "self-scan-write-baseline": "bca --paths . --exclude-from .bcaignore --num-jobs ${BCA_NUM_JOBS:-4} check --config bca-thresholds.toml --write-baseline .bca-baseline.toml",
    "self-scan-write-baseline-headroom": "BCA_HEADROOM_WRITE_BASELINE=.bca-baseline.toml python3 ./utils/bca-self-scan-headroom.py bca --paths . --exclude-from .bcaignore --num-jobs ${BCA_NUM_JOBS:-4}"
  }
}
```

Three portability footnotes for the npm tier:

- **Env vars beat shell expansion.** The helper reads `BCA_HEADROOM`
  from the environment (default `0.95`), so overriding the band is
  `BCA_HEADROOM=0.90 npm run self-scan-headroom` on POSIX shells. On
  Windows `cmd.exe`, set the variable separately or use
  [`cross-env`](https://www.npmjs.com/package/cross-env):
  `cross-env BCA_HEADROOM=0.90 npm run self-scan-headroom`. Avoid
  `${VAR:-default}` *as a primary configuration mechanism* — `cmd.exe`
  passes it through literally. The `${BCA_NUM_JOBS:-4}` usage above
  is a reasonable default for POSIX hosts; Windows users either set
  `BCA_NUM_JOBS` explicitly or replace the literal with a fixed
  number in a per-platform script.
- **`python3` vs `python`.** The stock python.org Windows installer
  ships `python.exe` and `py.exe` but no `python3` alias. Replace
  the literal `python3` above with `py -3` (Windows launcher) or
  add a one-line `scripts/python3.cmd` shim that forwards to
  `py -3`. macOS / Linux / WSL hosts have `python3` on `PATH` by
  default.
- **Use `cross-env` (or `pnpm exec --shell`) if you need any env
  var to be portable across the package.json users' shells.** Mixing
  `bash`-isms into `scripts` is the most common source of "works on
  my Mac, broken on a Windows reviewer's machine" pings.

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

## Helper script

The headroom helper exists because `bca check`'s
`--threshold name=value` flag accepts overrides on the command
line. The helper reads the TOML, multiplies, and re-emits.

A ~40-line implementation suitable for any project. It is a
condensed restatement of `big-code-analysis`'s own
[`utils/bca-self-scan-headroom.py`](https://github.com/dekobon/big-code-analysis/blob/main/utils/bca-self-scan-headroom.py)
— same env-var contract, same defensive checks, same exit codes —
trimmed for in-line readability:

```python
#!/usr/bin/env python3
"""Scale every threshold by $BCA_HEADROOM and run bca check."""
from __future__ import annotations
import os, subprocess, sys
from pathlib import Path

try:
    import tomllib  # Python 3.11+
except ImportError:  # pragma: no cover
    import tomli as tomllib  # `pip install tomli` on 3.9/3.10

def main() -> int:
    if len(sys.argv) < 2:
        print("usage: bca-self-scan-headroom.py <bca-invocation...>", file=sys.stderr)
        return 64

    raw = os.environ.get("BCA_HEADROOM") or "0.95"  # treat '' as unset
    try:
        ratio = float(raw)
    except ValueError:
        print(f"BCA_HEADROOM must be a number; got {raw!r}", file=sys.stderr)
        return 64
    if not 0.0 < ratio <= 1.0:
        print(f"BCA_HEADROOM must be in (0, 1]; got {ratio}", file=sys.stderr)
        return 64

    thresholds_path = Path(os.environ.get("BCA_THRESHOLDS") or "bca-thresholds.toml")
    baseline_path = Path(os.environ.get("BCA_BASELINE") or ".bca-baseline.toml")
    if not thresholds_path.is_file():
        print(f"missing {thresholds_path}", file=sys.stderr)
        return 1
    cfg = tomllib.loads(thresholds_path.read_text(encoding="utf-8"))
    thresholds = cfg.get("thresholds", {})
    if not thresholds:
        print(f"no [thresholds] table in {thresholds_path}", file=sys.stderr)
        return 1

    flags: list[str] = []
    for name, limit in thresholds.items():
        # Float so a fractional scaled limit (e.g. 6.65 for nargs=7
        # at BCA_HEADROOM=0.95) survives — flooring to int silently
        # widens the band.
        flags += ["--threshold", f"{name}={limit * ratio:.6g}"]

    write_target = os.environ.get("BCA_HEADROOM_WRITE_BASELINE")
    if write_target:
        cmd = [*sys.argv[1:], "check", "--write-baseline", write_target, *flags]
    else:
        cmd = [*sys.argv[1:], "check", "--baseline", str(baseline_path), *flags]
    return subprocess.call(cmd)

if __name__ == "__main__":
    sys.exit(main())
```

Five implementation details that matter in practice:

- **Emit a float, not an int.** `bca check --threshold` parses
  every value as `f64`, and the offender test is
  `value > limit` (strict). At `BCA_HEADROOM=0.95`, `nargs=7`
  scales to `6.65`. Flooring to `6` would silently widen the band
  by an extra ratio step. The `{:.6g}` format truncates
  float-multiplication artefacts (`6.6499999999999995`) without
  losing precision on the largest thresholds in the file.
- **Validate the ratio.** The half-open interval `(0, 1]` is the
  only sensible range. `0` disables the gate; values above `1`
  would make the soft tier looser than the hard tier and fire
  *after* CI — useless. The `or "0.95"` idiom treats both unset
  and set-but-empty (`BCA_HEADROOM=` in a stripped CI env) as the
  default, so a misconfigured matrix variable does not exit 64
  with the confusing message `got ''`.
- **Same baseline as the hard tier.** The soft tier `--baseline`
  must point at the exact same file the hard tier writes; otherwise
  every hard-tier offender re-fires on the soft tier. The helper
  reads `BCA_BASELINE` from the env (default `.bca-baseline.toml`)
  so renaming the file in one place — the Make / `just` recipe —
  propagates to both tiers without editing the Python.
- **Read everything from the environment, not `argv`.** Env-var
  propagation works the same in `make`, `just`, and `npm` scripts
  on every platform; CLI parameter expansion (`${HEADROOM:-0.95}`)
  does not — Windows `cmd.exe` passes it through literally. Argv
  carries only the literal `bca` invocation prefix; the four
  configuration knobs (`BCA_HEADROOM`, `BCA_THRESHOLDS`,
  `BCA_BASELINE`, `BCA_HEADROOM_WRITE_BASELINE`) all come from
  `os.environ`.
- **Defensive diagnostics.** The argv-length, file-exists, and
  empty-`[thresholds]` checks all exit before constructing a `bca`
  command, with stderr messages that name the helper rather than
  the downstream tool. Without them, a missing config file
  produces a confusing "no thresholds defined" error from `bca`
  itself, and the user has to bisect whether the helper, the
  config, or `bca` is at fault. The fallback `import tomli as
  tomllib` keeps the script working on Python 3.9/3.10 hosts
  (RHEL 8, Ubuntu 20.04, Debian bullseye); on 3.11+ `tomllib` is
  stdlib and `tomli` is not needed.

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
