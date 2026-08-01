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
