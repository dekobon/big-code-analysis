# Shell Rule

The `Bash` tool runs **zsh**, not bash (`echo $0` → `/usr/bin/zsh`).

## zsh does not field-split an unquoted parameter expansion

In POSIX sh and bash, `$var` unquoted is split on `IFS`. **In zsh it is
not.** The value arrives as a single word, however many spaces or
newlines it contains.

This is the one zsh/bash divergence that reliably produces a *wrong
answer rather than an error*, because the command still runs, still
exits 0, and still prints a plausible result.

Measured in this repository's shell:

| expression | zsh | bash |
| --- | --- | --- |
| `FLAGS="-p a -p b"; cmd $FLAGS` | **1 argument** | 4 arguments |
| `FILES=$(cat two-lines); for f in $FILES` | **1 iteration** | 2 iterations |
| `for f in $(cat two-lines)` | 2 iterations | 2 iterations |
| `cmd "${ARR[@]}"` where `ARR=(-p a -p b)` | 4 arguments | 4 arguments |

Note the third row. **Command substitution *is* split in zsh** — only
*parameter expansion* is not. So `for f in $(rg -l pattern)` behaves as
you expect, and the trap is specifically the intermediate variable:
assign the output first and the loop silently collapses to one
iteration. Do not "fix" the working form while chasing this.

## What it cost here

Two measurement loops during the #1090-#1151 batch, both of which
produced confident, uniform, entirely fabricated numbers.

Re-measuring #1143's threshold offenders:

```zsh
SCOPE="-p src -p big-code-analysis-cli/src -p big-code-analysis-web/src"
for spec in nargs=7 nargs=6 abc=50 cognitive=15; do
  bca check --no-config --exclude-tests $SCOPE --threshold "$spec" …
done
```

`$SCOPE` reached `bca` as the single argument
`-p src -p big-code-analysis-cli/src -p big-code-analysis-web/src`,
which matched no path, so every row reported **0 offenders**. Seven
rows of zeros is a coherent-looking result — "the repo is already
compliant everywhere" — and it is the answer that would have shipped
had the number not been implausible enough to re-check. The array form
reported 19, 73 and 127.

## How to apply

- **Build argument lists as arrays, expand them quoted:**

  ```zsh
  SCOPE=(-p src -p big-code-analysis-cli/src)
  bca check "${SCOPE[@]}" --threshold cognitive=15
  ```

  `"${ARR[@]}"` expands to one word per element in both shells. This is
  the only spelling that is correct in zsh *and* bash, so prefer it even
  in a script you think only zsh will run.

- **Iterate lines with `while IFS= read -r`, never through a variable:**

  ```zsh
  while IFS= read -r f; do …; done < list.txt
  rg -l pattern | while IFS= read -r f; do …; done
  ```

  This also survives paths containing spaces, which the split forms do
  not.

- **When a loop must reuse a captured list, capture into an array:**
  `FILES=("${(@f)$(cat list.txt)}")` splits on newlines only, or just
  re-run the command inside the `for`.

- **Sanity-check any measurement loop against a single hand-run case
  before believing the table.** One `bca check -p src …` typed out in
  full would have caught this immediately. A loop that emits a tidy
  column of zeros deserves that check specifically, because zero is
  what every one of these failure modes produces.

## Two siblings worth knowing

Both bit the same measurement in the same session, and both also yield
a plausible number rather than an error:

- **`$?` after a pipeline is the *last* stage's status.**
  `cmd | head` reports `head`'s success even when `cmd` failed. zsh
  spells the per-stage array `$pipestatus` (1-indexed); `PIPESTATUS`
  is bash-only and expands to nothing here.
- **`bca check` writes offenders to stderr.** `2>/dev/null` on a check
  invocation discards the entire result and leaves an empty stdout that
  reads as "no offenders".

## A `pgrep -f` wait loop matches itself and never exits

`pgrep -f` matches against the **full command line**. When a wait loop
is passed as text — `zsh -c "until ! pgrep -f 'make pre-commit' …"`,
which is exactly how the `Bash` tool runs every command — that text
*is* the shell's argv. The loop therefore finds itself, the condition
never goes false, and it runs until the machine reboots:

```zsh
# Never exits: this shell's own argv contains "make pre-commit".
zsh -c "until ! pgrep -f 'make pre-commit' >/dev/null; do sleep 30; done"
```

**The `-c` part is the precondition, not incidental.** The identical
loop inside a script file has argv `zsh /path/to/waiter.sh`, does not
match itself, and exits normally — verified by probe. So the bug is
specific to the way commands are issued here, and a reader who fails to
reproduce it from a `.sh` file has not disproved it.

The failure is silent in the way the rest of this file describes: no
error, no output, nothing in the log. It reads as "the job is still
running", which is indistinguishable from the truth right up until you
notice the job finished half a day ago.

Ten of these accumulated in one session here — seven waiting on a
`make pre-commit` that had long since written its `BCA_GATE: pass`,
three on a finished `collect.sh`. Each spun a `sleep` every 15-30s, the
oldest for twelve hours. They also **match each other**, so killing one
at a time does not help: six siblings keep the seventh's condition
true. `pgrep -af <pattern>` is the diagnosis — if every PID it prints
is a waiter, that is the whole bug.

### How to apply

- **Prefer a condition that is not a process at all.** The artifact the
  job produces cannot match the watcher — and bound the wait, because
  an unbounded loop on a job that dies is the same hang by another
  route:

  ```zsh
  log=$(mktemp /tmp/bca-pre-commit.XXXXXX.log)
  make pre-commit >"$log" 2>&1 &

  for _ in {1..90}; do                    # ceiling: 90 x 20s = 30 min
    grep -qs '^BCA_GATE:' "$log" && break
    sleep 20
  done

  grep -s '^BCA_GATE:' "$log" ||
    { echo "no BCA_GATE line after 30 min — crashed, killed, or still running" >&2; exit 1; }
  ```

  Three details earn their place. The `for` ceiling is what makes a
  dead job a bounded failure instead of a hang. `-s` on both greps
  suppresses `No such file or directory` before the log exists —
  without it the loop emits one stderr line per tick, forever. And the
  trailing `grep ||` makes the no-verdict case exit non-zero, so
  "silence" cannot read as success; that is the third state
  [`AGENTS.md`](../../AGENTS.md) names under "Reading the verdict".

  Exit 0 here means *a verdict appeared*, not that it said `pass` —
  read the line, as that section requires.

  Verified against four cases: verdict already present (breaks early),
  verdict arriving mid-wait (picked up), log present with no verdict,
  and log never created (both exit 1, no stderr noise).

- **When it really must be a process, break the self-match** with a
  bracket class, the standard `ps | grep` trick — `[m]ake` matches the
  string `make` but the pattern itself does not contain it:

  ```zsh
  until ! pgrep -f '[m]ake pre-commit' >/dev/null; do sleep 30; done
  ```

  Verified both halves by probe: the bracket watcher does not appear in
  its own `pgrep` output (so the loop exits), and the pattern still
  matches a real process whose argv contains `make pre-commit`. The
  naive form in the same probe matched itself and hung.

  It protects the watcher from *itself* only. Any other process quoting
  the plain string still matches — a sibling watcher written the naive
  way, a `ps | grep` someone left running, an editor holding the
  command in a buffer. That is why the file-artifact form above is the
  first recommendation and this one the fallback.

- **Do not reach for `$$` to exclude yourself. It does not work.**
  The obvious repair —

  ```zsh
  # BROKEN. Hangs exactly like the naive form.
  until ! (pgrep -f 'make pre-commit' | grep -qv "^$$\$"); do sleep 30; done
  ```

  — fails because the watcher is not the only process carrying that
  argv. The command substitution and the `(…)` subshell both fork from
  it and inherit it, so `pgrep` returns several PIDs where `$$` is only
  one:

  ```text
  watcher $$ = 1347315 ; pgrep sees: 1347301 1347315 1347316
  ```

  Filtering one PID can never empty that list, the pipeline stays true,
  and the loop never ends — measured, in both quoting styles. (Under
  `zsh -c "…"` there is a second, independent defect: a double-quoted
  `$$` is expanded by the *parent* before the child ever sees it, so
  the watcher excludes someone else's PID.) The bracket class above
  avoids the whole class, because it forks no subshell and `pgrep`'s
  own argv carries `[m]ake`, not `make`.

- **Do not poll for harness-tracked work at all.** `Bash` with
  `run_in_background` re-invokes on exit, and `Monitor` streams events.
  A hand-rolled waiter is only for something neither can see.

- **Check for leaks before ending a long session**: `pgrep -af 'do
  sleep'`. A waiter costs almost nothing, but it outlives the session
  and the next one inherits a process list nobody can account for.

Every snippet in this section was run before it was written down. The
first draft was not: the bracket form was probed and the other two were
reasoned about, and both of the reasoned ones were wrong — one hung,
one claimed a timeout it did not have. In a file about shell that
returns a plausible answer instead of an error, an unrun example is the
defect it documents.
