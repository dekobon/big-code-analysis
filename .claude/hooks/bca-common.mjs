import { execFile, spawn } from "node:child_process"
import { constants } from "node:fs"
import { access, open, realpath, stat } from "node:fs/promises"
import { delimiter, dirname, isAbsolute, join, relative, resolve, sep } from "node:path"
import { fileURLToPath } from "node:url"

const TIMEOUT = 3_000
const DISCOVERY_BUDGET = 4_000
const MAX_REPORT = 4_000
const BUILD_INPUTS = "build inputs: "
const DIGEST_SCRIPT = fileURLToPath(new URL("../../utils/self_scan_digest.py", import.meta.url))
const FALLBACK = "Review findings relevant to your change; do not move complexity just to lower a score.\n" +
  "These are current file findings, not proof of an edit-caused regression. Keep the task-boundary gate."

// `BCA_HOOK_TIMEOUT_MS` is a test-oriented override of the budget the
// analyzer run arms (#1857), like `BCA_DIGEST_SCRIPT`: a Node cold start of a
// stub analyzer on a loaded Windows runner can cross 3 s, so the plugin tests
// raise it and the timeout tests set their own. Unset, the budget is TIMEOUT,
// unchanged. It is read per call, from the environment the run is given, so
// a fixture can change it between runs — and so a shell that exports it
// changes a real hook's budget too; `analyzer()`'s arithmetic says how far
// that can go. Only the analyzer run moves: `checkout()`'s git call, the
// `--version` probes and the `taskkill` budget (which bounds the kill, not
// the analyzer) stay at TIMEOUT, which is why the budget is an argument the
// one analysing call site passes rather than something `run()` reads. The
// value is decimal digits only, in Node's timer range: `Number()` would also
// read `1e3`, hex and padded strings, and a delay above 2^31 - 1 is clamped
// to 1 ms with a warning, so anything else falls back to the default rather
// than arming a budget nobody wrote.
const MAX_TIMER_MS = 2_147_483_647
export function runTimeout(env = process.env) {
  const override = env.BCA_HOOK_TIMEOUT_MS ?? ""
  const ms = /^\d+$/.test(override) ? Number(override) : 0
  return ms >= 1 && ms <= MAX_TIMER_MS ? ms : TIMEOUT
}

export function run(file, args, cwd, signal, env = process.env, budget = TIMEOUT) {
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
    const timer = setTimeout(() => stop("TIMEOUT"), budget)
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

async function executableFile(path) {
  try {
    await access(path, constants.X_OK)
    const info = await stat(path)
    return info.isFile() ? { path, mtimeMs: info.mtimeMs, identity: await realpath(path) } : undefined
  } catch {
    return undefined
  }
}

async function localBuilds(root) {
  const name = process.platform === "win32" ? "bca.exe" : "bca"
  const builds = await Promise.all(["debug", "release"].map((profile) => executableFile(resolve(root, "target", profile, name))))
  return builds.filter(Boolean)
}

// The executable a bare `name` would spawn with `cwd` as the working
// directory: PATH entries are resolved against `cwd` — a relative entry,
// or an empty one, which `spawn` reads as the child's cwd — so the file
// probed here is the file the check later runs. PATHEXT spells its
// entries in upper case (`.COM;.EXE;…`) while Windows matches them
// case-insensitively, so the spelling here is the hook's to choose:
// lower case names the file as cargo writes it and as `localBuilds`
// spells it, rather than reporting (and spawning) a `bca.EXE` that
// exists on no disk.
async function installed(name, cwd) {
  const extensions = process.platform === "win32" ? (process.env.PATHEXT ?? ".EXE;.CMD;.BAT;.COM").toLowerCase().split(";") : [""]
  for (const directory of (process.env.PATH ?? "").split(delimiter)) {
    for (const extension of extensions) {
      const found = await executableFile(resolve(cwd, directory, name + extension))
      if (found) return found
    }
  }
  return undefined
}

async function probe(file, args, cwd, signal) {
  const result = await run(file, args, cwd, signal)
  if (!result.failed && result.code === 0) return { stdout: result.stdout }
  const detail = result.stderr.split(/\r?\n/).map((line) => line.trim()).find(Boolean)?.slice(0, 200)
  const status = Number.isInteger(result.code) ? `exited ${result.code}` : result.code ?? "signal"
  return { failure: detail ? `${file} ${status}: ${detail}` : `${file} ${status}` }
}

// Every failure of the digest step is a discovery failure, never a thrown
// error, so a check can lose at most the match step — never itself.
async function treeDigest(script, root, signal) {
  let present
  try { present = (await stat(script)).isFile() } catch (error) { return { failure: `digest script ${error.code ?? "unreadable"}` } }
  if (!present) return { failure: "no digest script" }
  const { stdout, failure } = await probe("python3", ["-B", script, root], root, signal)
    .catch((error) => ({ failure: `python3 ${error.code ?? error.message}` }))
  if (failure) return { failure }
  const digest = stdout.split(/\r?\n/).map((line) => line.trim()).find((line) => /^[0-9a-f]{64}$/.test(line))
  return digest ? { digest } : { failure: "python3 printed no digest" }
}

function reportedInputs(stdout) {
  return stdout.split(/\r?\n/).find((line) => line.startsWith(BUILD_INPUTS))?.slice(BUILD_INPUTS.length).trim()
}

// Order: `$BCA` unprobed; else the first of `target/debug/bca`, `target/release/bca`
// and PATH `bca` whose `--version` reports a `build inputs:` digest equal to the
// one `utils/self_scan_digest.py` computes over `root` — the same test
// `utils/self-scan-bca.py` applies, so the hook agrees with `make self-scan`
// (#1776); else the newest local build by mtime; else PATH `bca`.
// Probing executes each candidate, which is the trust level the old order already
// granted `target/release/bca` by running it unconditionally. The script is
// resolved relative to this module, never to `root`: `checkContext` can hand over
// a nested checkout, and that checkout must not supply the Python the parent's
// hook runs. `BCA_DIGEST_SCRIPT` is the tests' seam for that path, like
// `BCA_SELF_SCAN_WORKSPACE` in self-scan-bca.py. PATH `bca` is probed and
// returned as the resolved file, so the binary that answered `--version` is
// the one the check runs and the one the health line names. Discovery shares
// one budget so the check itself keeps room: the digest plus three probes at
// TIMEOUT each could otherwise spend more. The `checkout()` git call before
// this is not under the budget; its 3 s TIMEOUT plus this 4 s plus the
// check's 3 s is 10 s, which is why `.claude/settings.json` registers 15.
// `BCA_HOOK_TIMEOUT_MS` moves the check's term alone, so a shell exporting
// more than 8 000 ms makes a real hook outrun the 15 s Claude Code allows it
// and lose its report; the probes here keep TIMEOUT whatever is exported.
export async function analyzer(root, options = {}) {
  if (process.env.BCA) return { path: process.env.BCA, reason: "BCA environment variable" }
  const budget = new AbortController()
  const expire = () => budget.abort()
  const timer = setTimeout(expire, DISCOVERY_BUDGET)
  options.signal?.addEventListener("abort", expire, { once: true })
  if (options.signal?.aborted) expire()
  try {
    const builds = await localBuilds(root)
    const script = process.env.BCA_DIGEST_SCRIPT || DIGEST_SCRIPT
    let { digest, failure } = await treeDigest(script, root, budget.signal)
    if (digest) {
      const candidates = builds.map((build) => build.path)
      const onPath = await installed("bca", root)
      if (onPath && !builds.some((build) => build.identity === onPath.identity)) candidates.push(onPath.path)
      for (const candidate of candidates) {
        if (budget.signal.aborted) break
        const { stdout } = await probe(candidate, ["--version"], root, budget.signal)
        if (stdout !== undefined && reportedInputs(stdout) === digest) {
          return { path: candidate, reason: "build inputs match this tree" }
        }
      }
      failure = budget.signal.aborted ? "discovery budget exhausted" : "no candidate reports this tree's build inputs"
    }
    const [newest] = builds.sort((a, b) => b.mtimeMs - a.mtimeMs)
    if (newest) return { path: newest.path, reason: `newest local build (${failure})` }
    return { path: "bca", reason: `PATH bca (no local build; ${failure})` }
  } finally {
    clearTimeout(timer)
    options.signal?.removeEventListener("abort", expire)
  }
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
