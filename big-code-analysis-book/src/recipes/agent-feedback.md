# Feeding metrics to an agentic coding tool

This recipe shows how to send maintainability findings to
[Claude Code](https://code.claude.com/docs/en/overview) or
[OpenCode](https://opencode.ai/) after a file-editing tool finishes.
It assumes you can run [`bca check`](../commands/check.md) in your project
and configure your coding agent. The findings are advice for the agent;
the project's final validation gate still decides when a change is ready.

The hooks use the existing `bca check` command, which provides:

- A machine-parseable offender list (per-violation rows on stdout, or
  `--report-format sarif | code-climate | clang-warning | msvc-warning
  | checkstyle` for a structured document).
- Exit codes that distinguish current findings (`2`, or `2–5` in
  tiered mode), a clean check (`0`), and tool errors (`1`). The
  findings cover the whole file, not only the latest edit.
- Filtering by the project's policy: baselines, in-source suppression
  markers, and `[check] exclude` globs. Put exclusions that must apply
  to explicitly named files under
  [`[check]`](#put-the-hooks-exclusions-under-check).

## After-edit, not keystroke-time

An editor integration can use the
[Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
to update diagnostics while a person types. An agent hook checks the
completed file edit. The proposed
[`bca lsp` server (#384)](https://github.com/dekobon/big-code-analysis/issues/384)
and these hooks serve those different uses:

| | `bca lsp` (#384) | This recipe |
| --- | --- | --- |
| Consumer | Human in an editor | Agent in a tool loop |
| Trigger | Keystroke (`didChange`) | Tool call completes (an edit lands) |
| Reparse | Incremental | Whole-file, once per edit |
| Surface | Margin diagnostics | Text fed back into the model |
| Status | Proposed | Works today with `bca check` |

Use an editor integration for feedback while typing and an after-tool
hook for feedback during an agent's edit loop.

The command every per-tool section below invokes is the same one you
would run by hand:

```bash
# Exit 2 ⇒ this file has at least one offender. Thresholds come from
# the repo-root bca.toml (discovered automatically); override ad hoc
# with one or more --threshold flags.
bca check path/to/edited_file.rs --threshold cognitive=10
```

The [agent feedback profile](thresholds.md#profile-agent) suggests tighter
limits than a blocking CI gate. Evaluate the extra findings before adopting
them: unnecessary refactoring can expand the task or introduce bugs.
[Choosing thresholds](thresholds.md) explains the defaults and other profiles.

With thresholds configured in `bca.toml` (see
[Local threshold gates](local-gates.md#zero-config-the-bcatoml-manifest)),
`bca check <file>` reads the committed limits, baseline, and exclusions.
Use the same configuration and tier as CI when you want the same threshold policy.

### Put the hook's exclusions under `[check]` {#put-the-hooks-exclusions-under-check}

One thing does *not* carry over from the CI invocation, and skipping
it produces false positives. A hook names one file per run, and
[an explicitly named path overrides every walker
exclude](../commands/index.html#explicit-paths-bypass-the-filter),
following the [`rg`](https://github.com/BurntSushi/ripgrep) convention
that a path you named is a direct request. So a file your project keeps out of scope with `-X`,
`--exclude-from`, a `.bcaignore`, or a manifest `exclude` list is
still analyzed the moment the hook passes it by name, and reported as
an offender under an `exit 2` that frames it as a problem to address.

Walker excludes shape what gets **analyzed**. Check excludes shape what
gets **gated**. A per-file hook invocation only respects the second:

```toml
# bca.toml: survives an explicit path, so the hook sees what CI sees.
[check]
exclude = ["./utils/**", "./benches/**"]
```

`bca` warns on stderr whenever an explicitly named path overrides a
walker exclude, naming the glob, so the miswiring is visible:

```text
warning: utils/gate.py matches an exclude pattern (./utils/**) but was named explicitly; analyzing anyway
```

Treat that line as a to-do: move the entry it names to
`[check] exclude`. This repository moved its own dev-tooling globs
there for this reason.

The hook uses the nearest `bca.toml` above the session's working
directory, stopping at the checkout boundary. The reference hooks
also recognize targets inside another nested checkout and apply that
checkout's policy.

## Claude Code {#claude-code}

Use a
[Claude Code `PostToolUse` hook](https://code.claude.com/docs/en/hooks#posttooluse)
to send current threshold findings to the model after a successful
file edit. The reference hook returns
`hookSpecificOutput.additionalContext` on stdout and exits 0. The
tool's original result remains intact.

Prerequisites:

- Claude Code **2.1.139 or later**; that release added the hook `args`
  field used below (see the
  [official changelog](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md#21139)).
- [Node.js](https://nodejs.org/) LTS and `bca`.
- A nonempty `[thresholds]` table in
  [`bca.toml`](local-gates.md#zero-config-the-bcatoml-manifest), from
  `bca init` or written by hand. Run `bca check path/to/a/source-file`
  directly to confirm the configuration loads.

Then copy these files into the same paths in your project:

- [`.claude/hooks/bca-check.mjs`](https://github.com/dekobon/big-code-analysis/blob/main/.claude/hooks/bca-check.mjs):
  reads the hook event and returns advisory JSON.
- [`.claude/hooks/bca-common.mjs`](https://github.com/dekobon/big-code-analysis/blob/main/.claude/hooks/bca-common.mjs):
  runs the analyzer and bounds its output; the OpenCode plugin uses
  this helper too.
- [`.claude/hooks/bca-guidance.txt`](https://github.com/dekobon/big-code-analysis/blob/main/.claude/hooks/bca-guidance.txt):
  supplies the short runtime reminder. Replace its final `make`
  commands with your project's validation commands.

Merge this configuration into `.claude/settings.json`, preserving
existing settings:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "node",
            "args": ["${CLAUDE_PROJECT_DIR}/.claude/hooks/bca-check.mjs"],
            "timeout": 15
          }
        ]
      }
    ]
  }
}
```

The [exec form](https://code.claude.com/docs/en/hooks#exec-form-and-shell-form)
passes the script path as one argument, including spaces and shell
metacharacters. The hook requires Node on `PATH`; its JSON handling
uses no [npm](https://www.npmjs.com/) packages or
[`jq`](https://jqlang.github.io/jq/). For an existing `bca-check.sh`
registration, replace the old script with the
[current wrapper](https://github.com/dekobon/big-code-analysis/blob/main/.claude/hooks/bca-check.sh)
and copy the `.mjs` files alongside it, or replace that registration
with the Node configuration above. Keep only one registration to avoid
duplicate checks.

How the hook finds its configuration and analyzer:

- The event's `cwd` selects the nearest manifest within the current
  checkout. For a target in a different nested checkout, the hook
  discovers configuration from that target's directory.
- `CLAUDE_PROJECT_DIR` locates the script, but stays at the original
  project when Claude enters a worktree. Using it to select the
  analyzer configuration would check the wrong tree.
- Select `$BCA` first, unprobed. Otherwise run `--version` on the
  checkout's `target/debug/bca`, `target/release/bca` (`bca.exe` on
  Windows) and `bca` on `PATH`, and take the first whose
  `build inputs:` digest equals the one `utils/self_scan_digest.py`
  computes over the checkout — the same matching criterion `make
  self-scan` applies, so a build that satisfies one satisfies the
  other (they can still pick different matching files, since the
  self-scan prefers a matching `PATH` binary). When none matches, take the
  newest local build by modification time, then `bca` on `PATH`. A
  copy of the hook without `utils/self_scan_digest.py` beside it
  skips the digest step and degrades to that order. An explicit but
  unavailable `$BCA` stays unavailable rather than falling back to
  another binary.
- Within one checkout, the session's manifest also applies when it
  edits a sibling package. Change the session directory to select
  that package's own manifest.
- Nested checkouts, including Git submodules, are separate
  configuration boundaries. If a nested checkout has no thresholds,
  the check is unavailable; it does not inherit the parent's policy.

The hook handles analyzer results as follows; the hook itself exits 0:

- **Exit 0:** no model context for a clean or excluded file.
- **Exits 2–5:** stdout findings become advisory context, capped at
  4,000 characters with a truncation notice.
- **Analyzer failure:** no model context for a missing binary, exit 1,
  abnormal exit, timeout, or output overflow. Each subprocess has a
  three-second deadline and a 64 KiB output limit per stream. Timeout,
  cancellation, and output overflow terminate the owned process group
  on POSIX or process tree on Windows.
- **Concurrent file change:** findings are discarded if the file's
  identity, size, or timestamps change during analysis.

The adapter keeps no persistent deduplication state, so an unchanged
offender can reappear on a later edit. It checks the whole file:
`[new]` and `[regr]` compare against the configured baseline, not the
preceding edit. Use the
[standing guidance](#agent-guidance-ship-this-verbatim) to keep any
response within the task's scope. This matcher covers Edit, Write and
MultiEdit; shell commands and external editors need separate
integration.

**Verify model delivery after installation.** Using Claude's Write
tool, create a disposable supported source file that breaches a
configured limit, and ask Claude to quote the finding it receives.
Then simplify the file through Edit and confirm the clean result adds
no advisory. In
[debug output](https://code.claude.com/docs/en/hooks#debug-hooks),
`[bca-check]` JSON records distinguish findings, clean/excluded files,
out-of-scope paths, superseded results, and unavailable checks. These
records go to stderr with exit 0, so Claude does not receive them as
additional context. They record the checkout, the analyzer and why it was chosen
(`analyzerReason`), the outcome, and up to 1,000 characters of
analyzer stderr for troubleshooting.
Offender rows remain in the advisory; CI annotations and summary-file
side effects are disabled. The reference suite runs with
`make plugin-test`, including the shared OpenCode helper regressions.

## OpenCode V2 {#opencode-v2}

Use the reference [OpenCode](https://opencode.ai/) plugin to append
current threshold findings to the result of a successful `edit`,
`write` or `patch` tool call. The edit stays successful; the advisory
is extra text in the tool result, not an error. This is the
integration the repository runs on itself, verified with OpenCode
2.1.3.

Prerequisites:

- OpenCode 2.x. The plugin uses the
  [V2 server plugin API](https://opencode.ai/v2/docs/build/plugins/server);
  for OpenCode 1.x see the [legacy section](#opencode-v1-legacy).
- `bca`, and a nonempty `[thresholds]` table in
  [`bca.toml`](local-gates.md#zero-config-the-bcatoml-manifest), as
  for Claude Code above.

Copy these files into the same paths in your project:

- [`.opencode/plugins/bca-check.js`](https://github.com/dekobon/big-code-analysis/blob/main/.opencode/plugins/bca-check.js):
  the plugin.
- [`.claude/hooks/bca-common.mjs`](https://github.com/dekobon/big-code-analysis/blob/main/.claude/hooks/bca-common.mjs)
  and
  [`.claude/hooks/bca-guidance.txt`](https://github.com/dekobon/big-code-analysis/blob/main/.claude/hooks/bca-guidance.txt):
  the shared helper and the short runtime reminder, the same two files
  the Claude Code hook uses. The plugin imports the helper by relative
  path, so keep this layout even if you do not use Claude Code.
  Replace the reminder's final `make` commands with your project's
  validation commands.

OpenCode loads direct `.js` files under `.opencode/plugins/`
automatically ([Plugins](https://opencode.ai/v2/docs/plugins)). Do
**not** add the file to `plugins` in `opencode.json(c)`: that key
takes published packages and local plugin *directories*, and a
single-file path there is skipped with a
`configured plugin path must be a directory` warning in the server log.

How it behaves:

- It reacts to completed `edit`, `write` and `patch` calls, and to
  nothing else; shell commands and external editors need separate
  integration.
- It finds the configuration and analyzer the same way the Claude Code
  hook does, including the `build inputs:` match against a local
  build. The session's directory, not the plugin's, selects the
  checkout, so a session moved into a Git worktree checks that
  worktree. A copy without `utils/self_scan_digest.py` beside it
  degrades to the newest local build, then `bca` on `PATH`.
- A `patch` touching several files checks at most 8 of them and says
  how many it skipped. One advisory is at most 4,000 characters,
  guidance included; longer reports are truncated with a notice.
- Findings already delivered for the same file in the same session are
  not repeated, so a second edit that leaves the offenders unchanged is
  silent. A clean check, deleting the file, or reloading the plugin
  resets that; truncated reports are not recorded and can reappear.
- Analyzer failure, exit 1, timeout and output overflow stay out of the
  tool result and are logged as `[bca-check]` warnings.

**Verify model delivery after installation.** Open a session in the
project and use the `write` tool to create a disposable source file
that breaches a configured limit; the tool result should end with a
`bca advisory` block that the model can quote back. Simplify the file
with `edit` and confirm the clean result adds nothing. The server log
records `[bca-check] analyzer=… reason=…` on the first check per
checkout, and the plugin keeps a per-location health record under its
storage (`health/<directory>`) with the last outcome and the analyzer
chosen. Configuration changes under `.opencode/` reload without a
restart; if a freshly copied plugin does not load, `opencode service
restart` is the fallback.

## OpenCode V1 (legacy) {#opencode-v1-legacy}

The example in this section uses the V1 plugin API and is an
alternative to the V2 plugin above, not an addition to it: both are
`.opencode/plugins/bca-check.js`. Use the
[migration guide](https://opencode.ai/v2/docs/build/plugins/migrate-v1)
when porting a V1 plugin.

**Mechanism:** implement a [plugin](https://opencode.ai/docs/plugins/):
a JavaScript or TypeScript module that exports an async function
returning a hooks object. Register **`tool.execute.after`**, which
runs after a tool completes, including the `write` and `edit` tools.

**Feedback channel:** the after-hook surfaces a problem to the agent by
**throwing**. opencode's public plugins page documents the
throw-to-signal pattern but not an advisory return value for the
after-hook, so this recipe defaults to throwing. A thrown `Error`
carries its message back to the agent as the tool's failure.

**Mind the argument shape; it differs from the `before` hook.** The
published `@opencode-ai/plugin` types give `tool.execute.after` the
signature `(input, output)` where the tool name is `input.tool` and the
**tool arguments are on `input.args`** (the file path is
`input.args.filePath`). This is the trap: the docs' only worked example
is for `tool.execute.before`, where args live on `output.args` (mutable,
pre-execution). Copy that into an *after* hook and `output.args` is
`undefined`, the guard below always trips, and the plugin silently never
runs: a no-op that looks installed. Read `input.args.filePath` in the
after hook.

Drop this file in `.opencode/plugins/`. It is project-level and
auto-loaded; no `opencode.json` entry is needed, and in V2 a
single-file entry under `plugins` is skipped with a warning (see
above). The plugin below is plain JavaScript. For a
TypeScript plugin, `import type { Plugin } from "@opencode-ai/plugin"`
and annotate the export, and declare any extra deps in
`.opencode/package.json` (opencode installs it with
[Bun](https://bun.sh/)):

```javascript
// .opencode/plugins/bca-check.js
// The same text as .claude/hooks/bca-guidance.txt.
const GUIDANCE = `
[new] and [regr] compare with the configured baseline, not this edit;
without a baseline, every offender is new. <file> rows are file-level
findings, not necessarily a problem in the function you changed.
Review findings relevant to your task: simplify where it genuinely
clarifies, or justify essential complexity with a narrow suppression.
For example, inside a Rust function: // bca: suppress(cognitive) -- reason.
Use the language's comment syntax; use nexits, never exit, and halstead
for halstead.* metrics. tokens is not suppressible. Leave unrelated code
alone and mention relevant remaining findings. Do not move complexity
around just to lower a number. Finish the coherent edit before reacting;
make self-scan and make pre-commit remain this repository's final gates.
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

`GUIDANCE` is the short runtime reminder from `bca-guidance.txt`, not
the standing policy block below; keep it in step with that file (the
repository's hook tests pin the two equal). Because the channel is a
thrown error,
opencode reports it as a failed post-edit step, which is the intended
"address this before continuing" framing. The edit itself still lands:
`tool.execute.after` runs once the `write` or `edit` tool has already
written the file, so the throw frames the next step without undoing the
change.

**Restart opencode after adding the plugin.** OpenCode 1.x loads
plugins once at startup and does not hot-reload them, so a freshly
dropped `.opencode/plugins/bca-check.js` does nothing in the running
session. Quit and relaunch opencode, then confirm by editing a file you
know is over threshold and checking that the `edit`/`write` tool
reports the `bca` failure. An installed-but-inert plugin is the most
likely symptom, and a stale session is the most likely cause.

The [V2 plugin](#opencode-v2) uses a different plugin API and adds
guards worth porting to a V1 setup:

- **Repo-scope guard.** Resolve the path and skip anything outside the
  project root, so the hook never runs `bca` on a file the agent edits
  elsewhere on disk.
- **Local-build resolution.** Prefer `$BCA`; then the candidate among
  the checkout's `target/debug/bca`, `target/release/bca` and `bca` on
  `PATH` whose `build inputs:` digest matches the checkout; then the
  newest local build; then `bca` on `PATH`. A project that builds
  `bca` itself then gates against a binary built from the tree it is
  editing whenever one exists, rather than a stale one or whatever is
  installed globally; with no match it falls back to the newest build
  it can find.
- **Shared guidance.** Read the guidance text from one file that both
  this plugin and the Claude Code hook cite, so the two never drift.
  The V1 example above inlines it instead, to stay a single-file
  install.

## Agent guidance (ship this verbatim) {#agent-guidance-ship-this-verbatim}

Bare metric feedback can encourage changes that lower scores without
improving readability, such as extracting helpers with no useful responsibility.
Tell the agent what to do with a finding. Paste this block into your
agent rules file (`CLAUDE.md` or `AGENTS.md`) as standing policy.
Use the short `bca-guidance.txt` reminder in runtime feedback; the reference
hooks bound that text separately from the findings.

The policy block below remains in English in translated editions; copy it as written.

```markdown
**Responding to `bca` metric feedback.** A threshold violation is a
reason to review readability, not proof of a defect. Improve the code
where that helps the current task; lowering a score is not itself a goal.

- **Do not game the metric.** Do not extract a helper that exists only
  to move complexity off one function, split a cohesive function at an
  arbitrary line, collapse readable branches into a dense expression,
  or inline/obfuscate logic to dodge the count. These lower the
  per-function score without necessarily improving readability.
  New helpers also add names and interfaces that readers must understand.
- **Refactor only when it truly clarifies.** A good split has a name
  that means something and a boundary a reader would have drawn anyway.
  If you cannot name the extracted piece without inventing a
  `foo_part2`, reconsider the split.
- **When the complexity is essential, suppress with a reason.** Some
  functions are clearest left whole, such as a
  dispatch `match`, a hand-rolled parser table, an exhaustive state
  machine. For these, do not contort the code: add a suppression marker
  with the rationale on the same line:
  `// bca: suppress(cognitive) -- exhaustive opcode dispatch`.
  A clear function with an honest marker is better than a
  "compliant" tangle.
- **Keep the fix within the task.** Review findings relevant to the
  current change. Findings may concern other functions or the whole
  file; mention unrelated findings without expanding the edit.
```

## Honest suppression (exact syntax) {#honest-suppression-exact-syntax}

Suppression requires the exact syntax described in
[Suppression markers](../commands/suppression.md):

- **Per function:** place the marker in a comment inside the function
  body, naming the metric(s):
  `// bca: suppress(cyclomatic, abc) -- hand-rolled parser table`.
  A bare `// bca: suppress` (no list) silences every suppressible metric
  for that function; `tokens` remains enforced.
- **Whole file:** `// bca: suppress-file(halstead, nargs, nexits)`
  anywhere in the file; the bare `// bca: suppress-file` form silences
  every suppressible metric file-wide. It does not suppress `tokens`.
- **Put the rationale on the marker line, after a metric list.**
  The text after the list is free-form and needs no separator.
  Without a metric list, trailing text is not accepted because it cannot
  be distinguished from prose about the marker. Include a reason so a
  reviewer can assess the exemption. `bca exemptions` lists the markers
  for that review.
- **Use canonical metric names.** The accepted identifiers are `abc`,
  `cognitive`, `cyclomatic`, `halstead`, `loc`, `mi`, `nargs`,
  `nexits`, `nom`, `npa`, `npm`, `wmc`. It is **`nexits`, not `exit`**
  (the legacy `exit` alias was retired). An unknown identifier warns and
  is skipped; the recognized names beside it still suppress, so
  `suppress(cognitive, exit)` silences `cognitive` and complains about
  `exit`. `tokens` is deliberately *not* suppressible; treat it as a
  hard resource cap.

## Gate at the task boundary, not per edit

Per-edit hooks provide early advice. Run the project's validation gate
after a coherent change, including its tests and linters. The
[`make self-scan` / pre-commit pattern](local-gates.md) uses the CI
limits for its hard gate. For higher-is-worse metrics, its default soft
threshold is 95% of the hard limit, giving an early warning near that limit.

Intermediate edits can produce temporary findings that disappear when
the refactoring is complete. Let the agent finish a coherent edit before
responding to those findings. A metric is an indirect indicator of
readability; it does not establish correctness.

Avoid requiring an extra metric check after every edit when the hook
already runs one. Keep the final validation requirement explicit and
rerun a check when its inputs change or its previous result is unavailable.

## Caveats

- **Metric-gaming:** optimizing a proxy can undermine the intended goal,
  a risk described by [Goodhart's law](https://en.wikipedia.org/wiki/Goodhart%27s_law).
  Asking for a lower number can encourage moving complexity across function
  boundaries without improving readability. Include the
  [agent guidance](#agent-guidance-ship-this-verbatim) and explain
  [justified suppression](#honest-suppression-exact-syntax).
- **Thresholds are indicators, not correctness checks:** a threshold
  crossing calls for a readability review. It does not prove the code
  is defective. Send findings as advice the agent considers, and verify
  behavior with tests and other checks appropriate to the change.
- **Scope creep:** a finding can invite a larger refactoring than the
  task requires. Keep fixes within the current task. Findings may concern
  unrelated functions or the whole file; report unrelated findings without
  expanding the edit.
