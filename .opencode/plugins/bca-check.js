// .opencode/plugins/bca-check.js
//
// opencode port of the "Feeding metrics to an agent" recipe
// (big-code-analysis-book/src/recipes/agent-feedback.md, "## opencode").
//
// `tool.execute.after` hook: after the agent writes/edits a file, run
// `bca check` on just that file and, only when the per-function
// thresholds in the repo-root bca.toml are exceeded (exit 2), surface
// the offender list + guidance back to the agent by THROWING (the
// book's sanctioned after-hook feedback channel — there is no
// documented advisory return value). Stays silent on a clean file, an
// unsupported file type, a tool error, a file outside the repo, or a
// missing analyzer — it must never block an edit.
//
// Mirrors .claude/hooks/bca-check.sh: (a) dogfoods THIS checkout's
// release build before any `bca` on PATH, and (b) reads the shared
// guidance text from .claude/hooks/bca-guidance.txt so the two hooks
// never drift. Both behaviours go beyond the book's bare-`bca` +
// inlined-string example (see the note at the bottom of this file).

import { existsSync, readFileSync, accessSync, constants } from "node:fs"
import { resolve, sep } from "node:path"

// Fallback used only if the shared guidance file is missing.
const GUIDANCE_FALLBACK = `
Responding to bca metric feedback: make the code genuinely simpler,
not the number smaller. Do not extract a meaningless helper or split a
cohesive function to dodge the count — a spurious helper often raises
file-level nom/nargs and helps nothing. If the complexity is essential
and the function is clearest left whole, add a suppression marker with
a one-line reason instead of contorting the code. Keep the fix in the
function that was flagged rather than widening it into a module
rewrite.
`.trim()

// Resolve the analyzer with the same precedence as the shell hook:
// explicit $BCA override, then this checkout's release build (dogfood
// THIS repo's bca, not whatever is on PATH), then a bca on PATH.
// Returns null when no analyzer is available — a silent no-op.
const resolveBca = (root) => {
  const isExec = (p) => {
    try {
      accessSync(p, constants.X_OK)
      return true
    } catch {
      return false
    }
  }
  if (process.env.BCA && isExec(process.env.BCA)) return process.env.BCA
  const release = resolve(root, "target/release/bca")
  if (isExec(release)) return release
  return "bca" // fall back to PATH; if absent, the spawn below no-ops.
}

const loadGuidance = (root) => {
  const shared = resolve(root, ".claude/hooks/bca-guidance.txt")
  if (existsSync(shared)) {
    const text = readFileSync(shared, "utf8").trim()
    if (text) return text
  }
  return GUIDANCE_FALLBACK
}

export const BcaCheck = async ({ $, worktree, directory }) => {
  const root = worktree || directory || process.cwd()
  return {
    // Note: the after-hook's args are on `input.args`, NOT `output.args`
    // (output carries the tool's result: title/output/metadata).
    "tool.execute.after": async (input, _output) => {
      // React only to the file-writing tools. (Patch-style edit tools
      // carry no single filePath and are intentionally not covered.)
      if (input.tool !== "write" && input.tool !== "edit") return
      const filePath = input.args?.filePath
      if (!filePath) return

      // Only gate files inside this repo, and only if they still exist.
      const abs = resolve(root, filePath)
      if (abs !== root && !abs.startsWith(root + sep)) return
      if (!existsSync(abs)) return

      const bca = resolveBca(root)

      // `bca check` exits 0 clean, 2 on offenders, 1 on tool error.
      // Bun's $ throws on non-zero by default; capture instead so we
      // can branch on the exact code. A missing analyzer also no-ops.
      let res
      try {
        res = await $`${bca} check ${abs} --no-summary --no-remediation`
          .quiet()
          .nothrow()
      } catch {
        return // analyzer not found / failed to spawn: stay silent.
      }
      // 0 clean, 1 tool error: not a complexity issue. Use `< 2` rather
      // than `=== 2` so the tiered exit codes (3-5, from
      // `--strict-exit-codes` / `exit_codes = "tiered"`) still report.
      if (res.exitCode < 2) return

      // Surface the offenders to the agent by throwing.
      const offenders = res.stderr.toString().trim() || res.stdout.toString().trim()
      throw new Error(
        `bca flagged complexity in ${filePath}:\n\n${offenders}\n\n${loadGuidance(root)}`,
      )
    },
  }
}
