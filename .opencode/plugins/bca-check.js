import { randomUUID } from "node:crypto"
import { realpath, stat } from "node:fs/promises"
import { homedir } from "node:os"
import { isAbsolute, resolve, win32, posix } from "node:path"
import { advisory, analyzer, checkout as resolveCheckout, checkContext, checkArgs, guidance, inside, run, unchanged } from "../../.claude/hooks/bca-common.mjs"

const MAX_FILES = 8
// Match OpenCode v2.0.32 FileAccess.resolvePath / FSUtil.windowsPath for raw tool inputs only.
export function resolveInputPath(directory, input, platform = process.platform, home = homedir()) {
  const path = platform === "win32" ? win32 : posix
  const normalized = platform === "win32" ? input
    .replace(/^\/([a-zA-Z]):(?:[\\/]|$)/, (_, drive) => `${drive.toUpperCase()}:/`)
    .replace(/^\/([a-zA-Z])(?:\/|$)/, (_, drive) => `${drive.toUpperCase()}:/`)
    .replace(/^\/cygdrive\/([a-zA-Z])(?:\/|$)/, (_, drive) => `${drive.toUpperCase()}:/`)
    .replace(/^\/mnt\/([a-zA-Z])(?:\/|$)/, (_, drive) => `${drive.toUpperCase()}:/`) : input
  const expanded = normalized === "~" ? home :
    normalized.startsWith("~/") || (platform === "win32" && normalized.startsWith("~\\")) ?
      path.join(home, normalized.slice(2)) : normalized
  return path.resolve(directory, expanded)
}

function targets(event) {
  if (event.tool === "write" && typeof event.result.output?.target === "string") {
    return [{ path: event.result.output.target, normalized: true }]
  }
  if (event.tool !== "patch" && !event.result.metadata?.files) {
    if (typeof event.input?.path !== "string" || !event.input.path) {
      throw new Error(`${event.tool} completed without input.path`)
    }
    return [{ path: event.input.path, normalized: false }]
  }
  const files = event.result.metadata?.files
  if (!Array.isArray(files) || files.some((item) => typeof item?.file !== "string" || !item.file)) {
    throw new Error(`${event.tool} completed without metadata.files[].file`)
  }
  const result = files.map((item) => ({ path: item.file, normalized: true, deleted: item.status === "deleted" }))
  if (event.tool === "patch" && typeof event.input?.patchText === "string") {
    const lines = event.input.patchText.split(/\r?\n/)
    for (let i = 0; i < lines.length; i++) {
      const header = lines[i].trim()
      if (!header.startsWith("*** Update File: ")) continue
      const source = header.slice("*** Update File: ".length).trim()
      while (lines[i + 1]?.trimEnd() === "*** End of File") i++
      if (lines[i + 1]?.trimEnd().startsWith("*** Move to: ")) {
        result.push({ path: source, normalized: false, deleted: true, invalidationOnly: true })
      }
    }
  }
  return result
}

export default {
  id: "bca-check",
  async setup(ctx) {
    const roots = new Map()
    const files = new Map()
    const aliases = new Map()
    let revision = 0
    const warnings = new Set()
    const binaries = new Set()
    const totals = { runs: 0, checked: 0, findings: 0, delivered: 0, unavailable: 0, omitted: 0 }
    const location = ctx.location.directory
    const healthKey = `health/${encodeURIComponent(location)}`
    const loadID = randomUUID()
    const loadedAt = new Date().toISOString()
    let active = true
    const lifetime = new AbortController()

    function invalidate(state, version, deleted = false) {
      if (version <= state.version) return
      state.version = version
      state.running?.controller.abort()
      if (deleted) state.delivered.clear()
    }

    function fileState(file, lexical, version) {
      let state = files.get(file)
      if (!state) {
        state = { version: 0, delivered: new Map(), aliases: new Set(), users: 0 }
        files.set(file, state)
      }
      if (state.aliases.size < 32) {
        state.aliases.add(lexical)
        aliases.set(lexical, file)
      }
      invalidate(state, version)
      return state
    }

    function prune() {
      for (const [file, state] of files) {
        if (files.size <= 256) break
        if (state.users || state.running) continue
        files.delete(file)
        for (const alias of state.aliases) aliases.delete(alias)
      }
    }

    function warn(message) {
      if (!active || warnings.has(message)) return
      if (warnings.size >= 256) warnings.clear()
      warnings.add(message)
      console.warn(`[bca-check] ${message}`)
    }

    async function checkout(directory) {
      if (roots.has(directory)) return roots.get(directory)
      const root = await resolveCheckout(directory)
      if (roots.size >= 256) roots.clear()
      roots.set(directory, root)
      return root
    }

    console.info("[bca-check] active: advisory checks after edit, write and patch")
    await ctx.tool.hook("execute.after", async (event) => {
      if (!active || event.status !== "completed" || !["edit", "write", "patch"].includes(event.tool)) return
      const version = ++revision
      const audit = { time: new Date().toISOString(), sessionID: event.sessionID, tool: event.tool,
        checked: 0, findings: 0, delivered: 0, unavailable: 0, omitted: 0 }
      const held = new Set()
      try {
        const targetsToCheck = targets(event)
        for (const target of targetsToCheck) {
          if (!isAbsolute(target.path)) continue
          const known = files.get(aliases.get(target.path) ?? target.path)
          if (known) invalidate(known, version, target.deleted)
        }
        const session = await ctx.session.get({ sessionID: event.sessionID })
        const directory = session.location.directory
        const paths = new Map()
        for (const target of targetsToCheck) {
          const lexical = target.normalized ? resolve(directory, target.path) : resolveInputPath(directory, target.path)
          if (!target.invalidationOnly) paths.set(lexical, target)
          const known = files.get(aliases.get(lexical) ?? lexical)
          if (known) invalidate(known, version, target.deleted)
        }
        const root = await checkout(directory)
        audit.root = root

        const pending = []
        const selected = [...paths].slice(0, MAX_FILES)
        const omitted = paths.size - selected.length
        for (const [lexical, target] of selected) {
          if (!active) return
          let file
          let before
          try {
            file = await realpath(lexical)
            before = await stat(file, { bigint: true })
            if (!inside(root, file) || !before.isFile()) continue
          } catch (error) {
            const known = files.get(aliases.get(lexical) ?? lexical)
            if (error.code === "ENOENT" && known?.version === version) known.delivered.clear()
            else if (error.code !== "ENOENT") warn(`cannot inspect ${lexical}: ${error.message}`)
            continue
          }
          const context = await checkContext(directory, root, lexical, file)
          const { named, cwd } = context
          const bca = await analyzer(context.root)
          audit.analyzer = bca
          audit.cwd = cwd
          const binaryKey = JSON.stringify([context.root, bca])
          if (!binaries.has(binaryKey)) {
            if (binaries.size >= 256) binaries.clear()
            binaries.add(binaryKey)
            console.info(`[bca-check] analyzer=${bca} cwd=${cwd}`)
          }
          const state = fileState(file, lexical, version)
          if (!held.has(state)) state.users++
          held.add(state)
          if (state.running) await state.running.promise
          if (!active) return
          if (state.version !== version) continue
          audit.checked++
          const controller = new AbortController()
          const promise = run(bca, checkArgs(named), cwd, controller.signal)
          const running = { controller, promise }
          state.running = running
          const result = await promise
          if (state.running === running) state.running = undefined
          if (!active) return
          if (state.version !== version) continue
          const diagnostic = result.stderr.trim().slice(0, 1_000)
          if (diagnostic) {
            audit.diagnostic = diagnostic
            warn(`analyzer diagnostic: ${diagnostic}`)
          }
          if (result.failed || !Number.isInteger(result.code) || result.code < 0 || result.code > 5 || result.code === 1) {
            audit.unavailable++
            warn(`analyzer unavailable for ${named} (exit ${result.code ?? "signal"})`)
            continue
          }
          if (result.code === 0) {
            state.delivered.clear()
            console.debug(`[bca-check] clean or excluded: ${named}`)
            continue
          }
          const rows = result.stdout.trim()
          if (!rows) {
            audit.unavailable++
            warn(`analyzer exited ${result.code} without offender rows for ${named}`)
            continue
          }
          audit.findings++
          pending.push({ state, rows, named, lexical, file, before, deliveryKey: JSON.stringify([event.sessionID, cwd, named]) })
        }
        audit.omitted = omitted
        if (!pending.length && !omitted) return
        const reminder = await guidance(root, lifetime.signal)
        if (!active) return
        const current = []
        for (const finding of pending) {
          if (await unchanged(finding.lexical, finding.file, finding.before)) current.push(finding)
        }
        if (!active) return
        // No await between the final duplicate check and delivery/cache update: callbacks overlap.
        const fresh = current.filter(({ state, rows, deliveryKey }) =>
          state.version === version && state.delivered.get(deliveryKey) !== rows)
        if (!fresh.length && !omitted) {
          console.debug(`[bca-check] unchanged findings session=${event.sessionID}`)
          return
        }
        const { text, complete, emitted } = advisory(fresh, omitted, reminder, MAX_FILES)
        const original = event.result.content
        event.result = {
          ...event.result,
          content: [
            ...(typeof original === "string" ? [{ type: "text", text: original }] : original ?? []),
            { type: "text", text },
          ],
        }
        for (const { state, rows, deliveryKey } of complete) {
          if (state.delivered.size >= 256) state.delivered.clear()
          state.delivered.set(deliveryKey, rows)
        }
        audit.delivered = emitted
        console.info(`[bca-check] advisory delivered session=${event.sessionID} checked=${audit.checked} omitted=${omitted}`)
      } catch (error) {
        audit.unavailable++
        warn(`check unavailable: ${error.message}`)
      } finally {
        for (const state of held) state.users--
        prune()
        if (active) {
          totals.runs++
          for (const key of ["checked", "findings", "delivered", "unavailable", "omitted"]) totals[key] += audit[key]
          try {
            // Console output may be discarded by the shared service; keep a bounded health record.
            await ctx.storage.set(healthKey, { location, loadID, loadedAt, sinceLoad: { ...totals }, last: audit })
          } catch (error) {
            warn(`health record unavailable: ${error.message}`)
          }
        }
      }
    })
    return () => {
      active = false
      lifetime.abort()
      for (const state of files.values()) state.running?.controller.abort()
    }
  },
}
