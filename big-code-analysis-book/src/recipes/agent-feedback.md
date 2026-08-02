# Feeding metrics to an agentic coding tool

An agentic coding tool — Claude Code, opencode, and the like — is a
fast-growing consumer of maintainability feedback, and it wants that
feedback in a different shape than a human in an editor does. A human
keystrokes, so the editor loop reaches for a language server: parse on
`didChange`, render complexity in the margin, update on every
character. An agent does not keystroke. It writes a whole edit through
a tool call and yields the turn. The right feedback for that consumer
is **batch, after-edit, structured** — exactly what
[`bca check`](../commands/check.md) already emits.

So this recipe ships no new binary surface. `bca check` already gives
an agent everything it needs:

- A machine-parseable offender list (per-violation rows on stdout, or
  `--report-format sarif | code-climate | clang-warning | msvc-warning
  | checkstyle` for a structured document).
- A tiered exit code — `2` when offenders are present, `0` clean, `1`
  on tool error — so a hook can branch on "did this edit make the code
  too complex?" without parsing anything.
- Baseline filtering, in-source suppression markers, and
  `[check.exclude]` globs, so the signal an agent sees is the same
  ratcheted signal a human sees — provided the project's exclusions
  live under [`[check]`](#put-the-hooks-exclusions-under-check) rather
  than in a walker deny-set.

What was missing is the wiring. This page is that wiring: a
copy-pasteable feedback loop per tool, plus the agent-facing guidance
that keeps the loop from backfiring.

## After-edit, not keystroke-time

This recipe is the deliberate counterpart to the proposed
[`bca lsp` server (#384)](https://github.com/dekobon/big-code-analysis/issues/384).
The two serve different consumers and do not depend on each other:

| | `bca lsp` (#384) | This recipe |
| --- | --- | --- |
| Consumer | Human in an editor | Agent in a tool loop |
| Trigger | Keystroke (`didChange`) | Tool call completes (an edit lands) |
| Reparse | Incremental | Whole-file, once per edit |
| Surface | Margin diagnostics | Text fed back into the model |
| Status | Proposed | Works today with `bca check` |

Reach for the LSP when a person is typing; reach for this when an
agent is editing. Wiring an agent through the LSP's incremental
`didChange` path would pay for machinery the agent never uses.

The command every per-tool section below invokes is the same one you
would run by hand:

```bash
# Exit 2 ⇒ this file has at least one offender. Thresholds come from
# the repo-root bca.toml (discovered automatically); override ad hoc
# with one or more --threshold flags.
bca check path/to/edited_file.rs --threshold cognitive=10
```

An agent loop wants tighter limits than a blocking CI gate: a false
positive costs one wasted refactor rather than a blocked merge, and the
signal arrives while the code is still being written. See the
[agent feedback profile](thresholds.md#profile-agent) for the rest of
the table, and [Choosing thresholds](thresholds.md) for where the
shipped defaults come from.

With a `bca.toml` at the repo root (see
[Local threshold gates](local-gates.md#zero-config-the-bcatoml-manifest))
the `--threshold` flags are unnecessary — a bare
`bca check <file>` reads the committed limits, baseline, and excludes,
so the agent loop gates on exactly what CI gates on.

### Put the hook's exclusions under `[check]` {#put-the-hooks-exclusions-under-check}

One thing does *not* carry over from the CI invocation, and it will
produce false positives if you skip it. A hook names one file per run,
and [an explicitly named path overrides every walker
exclude](../commands/README.md#explicit-paths-bypass-the-filter) — the
`rg` convention that a path you named is a direct request. So a file
your project keeps out of scope with `-X`, `--exclude-from`, a
`.bcaignore`, or a manifest `exclude` list is nonetheless analyzed the
moment the hook passes it by name, and reported as an offender under an
`exit 2` that frames it as a problem to address.

Walker excludes shape what gets **analyzed**. Check excludes shape what
gets **gated**. A per-file hook invocation only respects the second:

```toml
# bca.toml — survives an explicit path, so the hook sees what CI sees.
[check]
exclude = ["./utils/**", "./benches/**"]
```

`bca` warns on stderr whenever an explicitly named path overrides a
walker exclude, naming the glob, so the miswiring is visible rather than
silent:

```text
bca: warning: utils/gate.py matches an exclude pattern (./utils/**) but was named explicitly; analyzing anyway
```

Treat that line as a to-do: the entry it names wants moving to
`[check] exclude`. This repository moved its own dev-tooling globs there
for exactly this reason.

One constraint on where the hook runs: while
[#1164](https://github.com/dekobon/big-code-analysis/issues/1164) is
open, a `[check] exclude` glob resolves against the *working directory*
rather than the manifest root when the path is named explicitly. Both
hooks below inherit the agent's working directory, which is the project
root, so they are unaffected — but a hook that `cd`s into a subdirectory
first would see its exemptions stop matching.

## Claude Code

**Mechanism:** a
[`PostToolUse` hook](https://code.claude.com/docs/en/hooks) in
`.claude/settings.json`, with the `matcher` scoped to the
file-editing tools.

**Feedback channel:** this is the strongest fit of any agentic tool.
A `PostToolUse` hook fires the instant an edit lands, and it can inject
text straight back into the model — either by **exiting `2` with the
message on stderr** (Claude reads stderr as context about what
happened) or by emitting JSON with
`hookSpecificOutput.additionalContext`. Feedback arrives at the exact
edit boundary and costs zero tokens until there is something to say.

Wire the hook in `.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/bca-check.sh"
          }
        ]
      }
    ]
  }
}
```

The wrapper script reads the edited file's path from the hook's stdin
JSON, runs `bca check` on just that file, and — only when the gate
trips — prints the offender list followed by the agent-guidance block
on stderr and exits `2`:

```bash
#!/usr/bin/env bash
# .claude/hooks/bca-check.sh — gate a single edited file after the edit.
set -euo pipefail

# PostToolUse delivers the tool call as JSON on stdin; the edited
# file's path is .tool_input.file_path for Edit/Write/MultiEdit.
file_path="$(jq -r '.tool_input.file_path // empty')"
[ -n "$file_path" ] || exit 0          # nothing to check; stay silent.
[ -f "$file_path" ] || exit 0          # file gone (e.g. a delete).

# Thresholds, baseline, and excludes come from the repo-root bca.toml.
# --no-summary / --no-remediation keep the feedback to the offender
# rows themselves; the guidance below tells the agent what to do. The
# rows are on stdout and any diagnostic on stderr, so `2>&1` captures
# whichever the run produced.
status=0
report="$(bca check "$file_path" --no-summary --no-remediation 2>&1)" || status=$?

# bca check exits 0 clean, 2 on offenders, 1 on tool error. Branch on 2
# specifically so a config/IO error is not mislabelled as "complexity".
case "$status" in
  0) exit 0 ;;                         # clean ⇒ say nothing.
  2) ;;                                # offenders ⇒ report them below.
  *) printf 'bca check could not run (exit %s):\n%s\n' "$status" "$report" >&2
     exit 0 ;;
esac

# Exit 2 makes Claude read stderr as context about the edit. Default
# CLAUDE_PROJECT_DIR and test the guidance file: under `set -u` an unset
# variable aborts the hook, and a missing file would emit a bare `cat`
# error where the guidance should be.
guidance="${CLAUDE_PROJECT_DIR:-$PWD}/.claude/hooks/bca-guidance.txt"
cat >&2 <<EOF
bca flagged complexity in the file you just edited:

$report

$([ -f "$guidance" ] && cat "$guidance")
EOF
exit 2
```

Store the [agent-guidance block](#agent-guidance-ship-this-verbatim)
in `.claude/hooks/bca-guidance.txt` so the hook feedback and your
`CLAUDE.md` cite identical wording. (`jq` is the only dependency; it
ships with most agent images and is a one-line install otherwise.)

If you would rather inject advisory context without the `exit 2`
signal, emit JSON on stdout instead of writing to stderr:

```bash
jq -n --arg ctx "$offenders" '{
  hookSpecificOutput: {
    hookEventName: "PostToolUse",
    additionalContext: $ctx
  }
}'
exit 0
```

Use `additionalContext` when you want the offender list in Claude's
view as a note; use `exit 2` when you want it framed as a problem to
address before moving on. Both leave the edit in place — `PostToolUse`
runs *after* the tool, so neither can undo it.

## opencode

**Mechanism:** a [plugin](https://opencode.ai/docs/plugins/) — a
JavaScript or TypeScript module exporting an async function that
returns a hooks object — using the **`tool.execute.after`** hook,
which fires after a tool runs (including the `write` and `edit`
file-modification tools).

**Feedback channel:** the after-hook surfaces a problem to the agent by
**throwing**. opencode's public plugins page documents the throw-to-signal
pattern but not an advisory return value for the after-hook, so this
recipe defaults to throwing — a thrown `Error` carries its message back
to the agent as the tool's failure.

**Mind the argument shape — it differs from the `before` hook.** The
published `@opencode-ai/plugin` types give `tool.execute.after` the
signature `(input, output)` where the tool name is `input.tool` and the
**tool arguments are on `input.args`** (the file path is
`input.args.filePath`). This is the trap: the docs' only worked example
is for `tool.execute.before`, where args live on `output.args` (mutable,
pre-execution). Copy that into an *after* hook and `output.args` is
`undefined`, the guard below always trips, and the plugin silently never
runs — a no-op that looks installed. Read `input.args.filePath` in the
after hook.

Drop this file in `.opencode/plugins/` (project-level; auto-loaded —
no `opencode.json` entry needed, that key is for npm-published
plugins). The plugin below is plain JavaScript; for a TypeScript plugin,
`import type { Plugin } from "@opencode-ai/plugin"` and annotate the
export, and declare any extra deps in `.opencode/package.json` (opencode
installs it with Bun):

```javascript
// .opencode/plugins/bca-check.js
const GUIDANCE = `
Responding to bca metric feedback: make the code genuinely simpler,
not the number smaller. Do not extract a meaningless helper or split a
cohesive function to dodge the count — a spurious helper often raises
file-level nom/nargs and helps nothing. If the complexity is essential
and the function is clearest left whole, add a suppression marker with
a one-line reason instead of contorting the code. Keep the fix in the
function that was flagged rather than widening it into a module
rewrite.
`.trim()

export const BcaCheck = async ({ $ }) => {
  return {
    // Note: the after-hook's args are on `input.args`, NOT `output.args`
    // (output carries the tool's result: title/output/metadata).
    "tool.execute.after": async (input, _output) => {
      // React only to the file-writing tools. (Patch-style edit tools
      // carry no single filePath and are intentionally not covered.)
      if (input.tool !== "write" && input.tool !== "edit") return
      const filePath = input.args?.filePath
      if (!filePath) return

      // `bca check` exits 0 clean, 2 on offenders, 1 on tool error.
      // Bun's $ throws on non-zero by default; capture instead so we
      // can branch on the exact code.
      const res = await $`bca check ${filePath} --no-summary --no-remediation`
        .quiet()
        .nothrow()
      // 0 clean, 1 tool error: not a complexity issue. `< 2` rather than
      // `=== 2` so the tiered exit codes (3-5, from `--exit-codes=tiered`
      // / `exit_codes = "tiered"`) still report.
      if (res.exitCode < 2) return

      // Surface the offenders to the agent by throwing. The rows are on
      // stdout; stderr is the fallback for a run whose only output is a
      // diagnostic.
      const offenders = res.stdout.toString().trim() || res.stderr.toString().trim()
      throw new Error(`bca flagged complexity in ${filePath}:\n\n${offenders}\n\n${GUIDANCE}`)
    },
  }
}
```

Keep the `GUIDANCE` string in sync with the verbatim block below (or
read it from a shared file). Because the channel is a thrown error,
opencode reports it as a failed post-edit step — which is the intended
"address this before continuing" framing. The edit itself still lands:
`tool.execute.after` runs once the `write` or `edit` tool has already
written the file, so the throw frames the next step without undoing the
change.

**Restart opencode after adding the plugin.** Plugins are loaded once at
startup and are not hot-reloaded. A freshly dropped
`.opencode/plugins/bca-check.js` does nothing in the running session;
quit and relaunch opencode, then confirm by editing a file you know is
over threshold and checking that the `edit`/`write` tool reports the
`bca` failure. An installed-but-inert plugin is the most likely symptom,
and a stale session is the most likely cause.

For a hardened reference, this repository ships its own copy at
[`.opencode/plugins/bca-check.js`](https://github.com/dekobon/big-code-analysis/blob/main/.opencode/plugins/bca-check.js).
It adds three guards the minimal example omits, each worth porting for a
real project:

- **Repo-scope guard.** Resolve the path and skip anything outside the
  project root, so the hook never runs `bca` on a file the agent edits
  elsewhere on disk.
- **Local-build resolution.** Prefer `$BCA`, then a
  `target/release/bca` in the checkout, then `bca` on `PATH`. A project
  that builds `bca` itself then gates against its own analyzer instead
  of whatever is installed globally.
- **Shared guidance.** Read the guidance text from one file that both
  this plugin and the Claude Code hook cite, so the two never drift.

## Agent guidance (ship this verbatim) {#agent-guidance-ship-this-verbatim}

The feedback channel is only half the recipe. A bare "cognitive 26 >
25" reliably triggers the gaming move — the agent extracts a
semantically empty helper to shave the per-function number, *lowering*
per-function complexity while *raising* file-level `nom`/`nargs` and
leaving the code worse. The mitigation is telling the agent what a
violation means and what to do about it. Paste this block into your
agent rules file (`CLAUDE.md`, opencode `AGENTS.md`) **and** into the
hook feedback text, so the instruction is present both as standing
policy and at the moment of the violation:

```markdown
**Responding to `bca` metric feedback.** A threshold violation
(cognitive, cyclomatic, ABC, …) means *this function is hard for a
human to follow*. The number is a proxy for that, not the goal. Your
job is to make the code genuinely simpler — not to make the number go
down.

- **Do not game the metric.** Do not extract a helper that exists only
  to move complexity off one function, split a cohesive function at an
  arbitrary line, collapse readable branches into a dense expression,
  or inline/obfuscate logic to dodge the count. These lower the
  per-function score while making the code worse — and a spurious
  helper often *raises* file-level `nom`/`nargs`, so you have not even
  helped the file.
- **Refactor only when it truly clarifies.** A good split has a name
  that means something and a boundary a reader would have drawn anyway.
  If you cannot name the extracted piece without inventing a
  `foo_part2`, the split is gaming — stop.
- **When the complexity is essential, suppress with a reason.** Some
  functions are irreducibly complex *and clearest left whole* — a
  dispatch `match`, a hand-rolled parser table, an exhaustive state
  machine. For these, do not contort the code: add a suppression marker
  with the rationale on the same line —
  `// bca: suppress(cognitive) — exhaustive opcode dispatch` — and move
  on. A clear function with an honest marker is better than a
  "compliant" tangle.
- **Keep the fix where the violation is.** The flag is scoped to the
  function you just edited. Fix it there, mention anything larger you
  noticed, and do not widen the change into a module rewrite to bring
  the number down.
```

## Honest suppression (exact syntax) {#honest-suppression-exact-syntax}

The guidance above tells the agent that suppression is a legitimate
move. For that to work the agent has to spell the marker correctly —
and the marker syntax is a frequent source of silent no-ops. Teach it
precisely (full reference:
[Suppression markers](../commands/suppression.md)):

- **Per function** — place the marker in a comment inside the function
  body, naming the metric(s):
  `// bca: suppress(cyclomatic, abc) — hand-rolled parser table`. A
  bare `// bca: suppress` (no list) silences *every* metric for that
  function.
- **Whole file** — `// bca: suppress-file(halstead, nargs, nexits)`
  anywhere in the file; the bare `// bca: suppress-file` form silences
  every metric file-wide.
- **Put the rationale on the marker line, after a metric list.**
  Anything after the list is free text and does not need a separator, so
  the reason lives where the next reader of the flagged function will
  see it. (A *bare* verb takes no trailing text at all — nothing there
  distinguishes a rationale from prose about the marker, so naming the
  metrics is what buys you a reason.) Write it every time:
  a reviewer — human or agent — needs to tell an honest exemption from a
  dodge, and `bca exemptions` lists every marker in the tree for exactly
  that audit.
- **Use canonical metric names.** The accepted identifiers are `abc`,
  `cognitive`, `cyclomatic`, `halstead`, `loc`, `mi`, `nargs`,
  `nexits`, `nom`, `npa`, `npm`, `wmc`. It is **`nexits`, not `exit`**
  (the legacy `exit` alias was retired). An unknown identifier warns and
  is skipped; the recognized names beside it still suppress, so
  `suppress(cognitive, exit)` silences `cognitive` and complains about
  `exit`. `tokens` is deliberately *not* suppressible; treat it as a
  hard resource cap.

## Gate at the task boundary, not per edit

The per-edit hooks above are an early-warning convenience, not the
gate. The gate is the same check a human runs before declaring a task
done: the two-tier
[`make self-scan` / pre-commit pattern](local-gates.md) (hard tier
mirroring CI, soft tier as a 95%-of-limit headroom band). Point the
agent at that as its "before I'm finished" step.

Going finer-grained than the task boundary is low- or negative-value
for an agent. A complexity threshold is a proxy, not a correctness
gate (see the caveats below), so re-running it after *every* micro-edit
mid-refactor just produces transient violations that resolve
themselves a few edits later — noise the agent then wastes a turn
"fixing". Let the agent finish a coherent change, then gate once.

One instruction to leave out of the rules file: a standing "verify your
fix, then re-check the metrics" step layered on top of the hook.
Current models re-read and re-check their own edits without being asked,
and an explicit instruction compounds with that — the loop spends turns
re-running a gate whose input has not changed. The hook already fires on
its own when an edit lands. Let that be the signal.

## Caveats

Three caveats the recipe depends on; ignore them and the loop does more
harm than good.

- **Goodhart's law / metric-gaming.** "Make this number smaller" is not
  the same instruction as "make this code simpler", and an LLM told the
  former will satisfy it the cheapest way it can — usually by shuffling
  complexity across a function boundary rather than removing it. The
  [agent-guidance block](#agent-guidance-ship-this-verbatim) and the
  [honest-suppression section](#honest-suppression-exact-syntax) are
  the mitigation; they are load-bearing, not optional polish. Ship them
  with the hook or expect the gaming move.
- **Thresholds are proxies, not correctness gates.** A failing
  compile or a red test is unambiguous — a tight agent loop on those
  converges. A complexity threshold is softer: crossing it means
  "a human should look at whether this is still readable", which is a
  judgement call, not a defect. Set expectations accordingly — wire
  complexity feedback as advice the agent weighs, not a pass/fail it
  must drive to zero at any cost.
- **Complexity feedback invites scope creep.** "This function is hard to
  follow" reads to an agent as an invitation to restructure, and the
  restructuring does not reliably stop at the function the hook named —
  a two-line fix becomes a module rewrite nobody asked to review. Bound
  it in the rules file: the violation is scoped to the edit that
  triggered it. Fix it where it was flagged, mention anything larger you
  noticed, and do not widen the change to chase the number.
