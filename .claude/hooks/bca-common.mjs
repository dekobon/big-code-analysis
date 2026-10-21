import { execFile, spawn } from "node:child_process"
import { constants } from "node:fs"
import { access, open, realpath, stat } from "node:fs/promises"
import { dirname, isAbsolute, relative, resolve, sep } from "node:path"

const TIMEOUT = 3_000
const MAX_REPORT = 4_000
const FALLBACK = "Review findings relevant to your change; do not move complexity just to lower a score.\n" +
  "These are current file findings, not proof of an edit-caused regression. Keep the task-boundary gate."

export function run(file, args, cwd, signal, env = process.env) {
  return new Promise((done) => {
    let error
    let stopped
    let treeKilled = Promise.resolve()
    const stdout = []
    const stderr = []
    const child = spawn(file, args, {
      cwd,
      env,
      stdio: ["ignore", "pipe", "pipe"],
      detached: process.platform !== "win32",
      windowsHide: true,
    })
    const stop = (reason) => {
      if (stopped) return
      stopped = reason
      if (child.pid && process.platform === "win32") {
        treeKilled = new Promise((finished) => {
          execFile("taskkill.exe", ["/PID", String(child.pid), "/T", "/F"], { timeout: TIMEOUT, windowsHide: true }, () => {
            child.kill("SIGKILL")
            finished()
          })
        })
      } else if (child.pid) {
        try { process.kill(-child.pid, "SIGKILL") } catch (error) {
          if (error.code !== "ESRCH") child.kill("SIGKILL")
        }
      } else child.kill("SIGKILL")
      child.stdout?.destroy()
      child.stderr?.destroy()
    }
    for (const [stream, chunks] of [[child.stdout, stdout], [child.stderr, stderr]]) {
      let bytes = 0
      stream?.on("data", (chunk) => {
        if (stopped) return
        const buffer = Buffer.from(chunk)
        chunks.push(buffer.subarray(0, Math.max(0, 64 * 1024 - bytes)))
        bytes += buffer.length
        if (bytes > 64 * 1024) stop("OUTPUT_LIMIT")
      })
    }
    child.on("error", (cause) => { error = cause.code ?? "SPAWN_ERROR" })
    const abort = () => stop("ABORT_ERR")
    const timer = setTimeout(() => stop("TIMEOUT"), TIMEOUT)
    signal?.addEventListener("abort", abort, { once: true })
    child.once("close", async (code, exitSignal) => {
      clearTimeout(timer)
      signal?.removeEventListener("abort", abort)
      await treeKilled
      done({ code: stopped ?? error ?? code, failed: Boolean(stopped || error || exitSignal),
        stdout: Buffer.concat(stdout).toString("utf8"), stderr: Buffer.concat(stderr).toString("utf8") })
    })
    if (signal?.aborted) abort()
  })
}

export function inside(root, file) {
  const path = relative(root, file)
  return path !== ".." && !path.startsWith(`..${sep}`) && !isAbsolute(path)
}

export async function analyzer(root) {
  if (process.env.BCA) return process.env.BCA
  const local = resolve(root, "target/release", process.platform === "win32" ? "bca.exe" : "bca")
  try {
    await access(local, constants.X_OK)
    if ((await stat(local)).isFile()) return local
  } catch {
    // This checkout has no executable release build; use the installed analyzer.
  }
  return "bca"
}

export async function checkout(directory) {
  const scoped = /^(GIT_(DIR|WORK_TREE|INDEX_FILE|PREFIX|COMMON_DIR|OBJECT_DIRECTORY|ALTERNATE_OBJECT_DIRECTORIES|CEILING_DIRECTORIES|DISCOVERY_ACROSS_FILESYSTEM))$/i
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !scoped.test(key)))
  const git = await run("git", ["rev-parse", "--show-toplevel"], directory, undefined, env)
  return realpath(git.code === 0 ? git.stdout.replace(/\r?\n$/, "") : directory)
}

async function exists(path) {
  try { return await stat(path) } catch (error) {
    if (error.code === "ENOENT" || error.code === "ENOTDIR") return undefined
    throw error
  }
}

export async function checkContext(directory, root, lexical, file) {
  let targetRoot = root
  for (let dir = dirname(file); dir !== root && inside(root, dir); dir = dirname(dir)) {
    if (await exists(resolve(dir, ".git"))) {
      targetRoot = dir
      break
    }
  }
  const start = targetRoot === root ? await realpath(directory) : dirname(file)
  let cwd = targetRoot
  for (let dir = start; inside(targetRoot, dir); dir = dirname(dir)) {
    if ((await exists(resolve(dir, "bca.toml")))?.isFile()) {
      cwd = dir
      break
    }
    if (dir === targetRoot || dirname(dir) === dir) break
  }
  const named = `.${sep}${relative(cwd, inside(targetRoot, lexical) ? lexical : file)}`
  return { root: targetRoot, cwd, named }
}

export function checkArgs(named) {
  return ["check", named, "--no-summary", "--no-remediation", "--summary-file", "never", "--github-annotations=never"]
}

export async function unchanged(lexical, file, before) {
  try {
    if (file !== await realpath(lexical)) return false
    const after = await stat(file, { bigint: true })
    return ["dev", "ino", "size", "mtimeNs", "ctimeNs"].every((key) => before[key] === after[key])
  } catch (error) {
    if (error.code === "ENOENT" || error.code === "ENOTDIR") return false
    throw error
  }
}

export async function guidance(root, signal) {
  let handle
  try {
    if (signal?.aborted) return FALLBACK
    handle = await open(resolve(root, ".claude/hooks/bca-guidance.txt"), constants.O_RDONLY | (constants.O_NONBLOCK ?? 0))
    if (signal?.aborted || !(await handle.stat()).isFile()) return FALLBACK
    const buffer = Buffer.alloc(8 * 1024)
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0)
    return signal?.aborted ? FALLBACK : buffer.toString("utf8", 0, bytesRead).trim() || FALLBACK
  } catch {
    return FALLBACK
  } finally {
    await handle?.close()
  }
}

export function advisory(findings, omitted, reminder, maxFiles = 8) {
  const header = "bca advisory — findings are not necessarily caused by this edit.\n\n"
  const clipped = "\n[Report truncated; further findings omitted. Run bca check for the full report.]"
  const guidance = reminder.length > 1_200 ? `${reminder.slice(0, 1_150)}\n[Guidance truncated.]` : reminder
  const footer = `${omitted ? `\n${omitted} further files not checked (limit ${maxFiles} targets per tool call).\n` : ""}\n${guidance}`
  let remaining = MAX_REPORT - header.length - footer.length - clipped.length
  const complete = []
  const parts = []
  let emitted = 0
  for (const finding of findings) {
    const text = `Current threshold findings in ${finding.named}:\n${finding.rows}\n\n`
    parts.push(text.slice(0, remaining))
    if (remaining > 0) emitted++
    if (text.length > remaining) {
      parts.push(clipped)
      break
    }
    complete.push(finding)
    remaining -= text.length
  }
  return { text: header + parts.join("").trimEnd() + footer, complete, emitted }
}
