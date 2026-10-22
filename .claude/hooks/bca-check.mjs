import { realpath, stat } from "node:fs/promises"
import { isAbsolute, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { advisory, analyzer, checkout, checkContext, checkArgs, guidance, inside, run, unchanged } from "./bca-common.mjs"

const MAX_INPUT = 8 * 1024 * 1024
const EDIT_TOOLS = new Set(["Edit", "Write", "MultiEdit"])

// One invocation per successful tool call. No persistent dedup state can outlive a clean edit.
export async function checkHook(input, signal) {
  if (input?.hook_event_name !== "PostToolUse" || !EDIT_TOOLS.has(input.tool_name)) return
  const health = { hook: "bca-check", time: new Date().toISOString(), outcome: "unavailable" }
  if (typeof input.session_id === "string") health.session = input.session_id
  if (typeof input.agent_id === "string") health.agent = input.agent_id
  try {
    const directory = input.cwd
    const path = input.tool_input?.file_path
    if (typeof directory !== "string" || !isAbsolute(directory) || typeof path !== "string" || !isAbsolute(path)) {
      throw new Error("PostToolUse requires absolute cwd and tool_input.file_path")
    }
    const root = await checkout(directory)
    health.root = root
    const lexical = resolve(path)
    let file
    let before
    try {
      file = await realpath(lexical)
      before = await stat(file, { bigint: true })
    } catch (error) {
      if (error.code !== "ENOENT") throw error
      health.outcome = "missing"
      return
    }
    if (!inside(root, file) || !before.isFile()) {
      health.outcome = "out-of-scope"
      return
    }
    const context = await checkContext(directory, root, lexical, file)
    const { named, cwd } = context
    health.root = context.root
    health.cwd = cwd
    const bca = await analyzer(context.root)
    health.analyzer = bca
    if (signal?.aborted) {
      health.outcome = "cancelled"
      return
    }
    const result = await run(bca, checkArgs(named), cwd, signal)
    health.exit = result.code ?? "signal"
    health.diagnostic = result.stderr.trim().slice(0, 1_000)
    if (signal?.aborted) {
      health.outcome = "cancelled"
      return
    }
    if (result.failed || !Number.isInteger(result.code) || result.code < 0 || result.code > 5 || result.code === 1) {
      health.reason = "analyzer failed or could not run"
      return
    }
    if (result.code === 0) {
      health.outcome = "clean-or-excluded"
      return
    }
    const rows = result.stdout.trim()
    if (!rows) {
      health.reason = "analyzer returned an offender exit without stdout findings"
      return
    }
    const reminder = await guidance(context.root, signal)
    if (!await unchanged(lexical, file, before)) {
      health.outcome = "superseded"
      return
    }
    if (signal?.aborted) {
      health.outcome = "cancelled"
      return
    }
    const { text } = advisory([{ named, rows }], 0, reminder)
    health.outcome = "findings"
    return { hookSpecificOutput: { hookEventName: "PostToolUse", additionalContext: text } }
  } catch (error) {
    health.reason = error.code ?? error.message
  } finally {
    // Claude keeps exit-0 stderr in its debug log without injecting it into model context.
    console.error(`[bca-check] ${JSON.stringify(health)}`)
  }
}

async function main() {
  const controller = new AbortController()
  const cancel = () => {
    controller.abort()
    process.stdin.destroy()
  }
  process.once("SIGTERM", cancel)
  process.once("SIGINT", cancel)
  try {
    const chunks = []
    let size = 0
    for await (const chunk of process.stdin) {
      size += chunk.length
      if (size > MAX_INPUT) throw new Error("hook input exceeds 8 MiB")
      chunks.push(chunk)
    }
    const input = JSON.parse(Buffer.concat(chunks).toString("utf8"))
    const output = await checkHook(input, controller.signal)
    if (output) process.stdout.write(`${JSON.stringify(output)}\n`)
  } catch (error) {
    console.error(`[bca-check] ${JSON.stringify({ outcome: "unavailable", reason: error.message })}`)
  } finally {
    process.removeListener("SIGTERM", cancel)
    process.removeListener("SIGINT", cancel)
  }
}

if (process.argv[1] && await realpath(process.argv[1]).catch(() => undefined) === fileURLToPath(import.meta.url)) await main()
