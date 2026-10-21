import assert from "node:assert/strict"
import childProcess, { execFileSync, spawnSync } from "node:child_process"
import fsPromises, { mkdtemp, mkdir, readFile, realpath, rename, rm, symlink, writeFile } from "node:fs/promises"
import { EventEmitter } from "node:events"
import { syncBuiltinESMExports } from "node:module"
import { homedir, tmpdir } from "node:os"
import { join, relative, resolve, sep } from "node:path"
import { fileURLToPath } from "node:url"
import test from "node:test"
import plugin, { resolveInputPath } from "../.opencode/plugins/bca-check.js"
import { checkout, guidance } from "../.claude/hooks/bca-common.mjs"

const ROW = "[new] ./sample.rs:1-12: sample: cognitive = 20 (limit 15)"
const GUIDANCE = "Use a meaningful boundary; do not game the metric."
const named = (path) => `.${sep}${path.split("/").join(sep)}`
const EXE = process.platform === "win32" ? ".exe" : ""

function gitEnvironment(config) {
  return {
    ...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith("GIT_"))),
    GIT_CONFIG_GLOBAL: config,
    GIT_CONFIG_NOSYSTEM: "1",
  }
}

function redirectExecutables(t, scripts) {
  const original = childProcess.spawn
  const mock = t.mock.method(childProcess, "spawn", (file, args, options) => {
    return scripts.has(file) ? original(process.execPath, [file, ...args], options) : original(file, args, options)
  })
  syncBuiltinESMExports()
  t.after(() => {
    mock.mock.restore()
    syncBuiltinESMExports()
  })
}

async function fixture(t, { git = true, temporaryDirectory = tmpdir() } = {}) {
  const directory = await realpath(await mkdtemp(join(temporaryDirectory, "bca plugin test ")))
  const config = join(directory, "empty-gitconfig")
  await writeFile(config, "")
  const runGit = (args, options = {}) => execFileSync("git", args, { ...options, env: gitEnvironment(config) })
  const root = join(directory, "checkout")
  await mkdir(join(root, ".claude/hooks"), { recursive: true })
  await writeFile(join(root, ".claude/hooks/bca-guidance.txt"), GUIDANCE)
  if (git) runGit(["init", "-q", root])
  const hooks = join(directory, "empty-hooks")
  await mkdir(hooks)
  if (git) runGit(["-C", root, "config", "core.hooksPath", hooks])
  const stub = join(directory, "analyzer.mjs")
  await writeFile(stub, `#!${process.execPath}
import { appendFileSync, readFileSync } from "node:fs";
const args = process.argv.slice(2);
appendFileSync(${JSON.stringify(join(directory, "calls.jsonl"))}, JSON.stringify({ cwd: process.cwd(), args }) + "\\n");
const data = JSON.parse(readFileSync(args[1], "utf8"));
if (data.hang) { process.on("SIGTERM", () => {}); setInterval(() => {}, 1000); }
else {
  process.stdout.write(data.rows ?? ${JSON.stringify(ROW)});
  process.stderr.write(data.stderr ?? "");
  process.exitCode = data.code ?? 2;
}
`, { mode: 0o755 })
  const scripts = new Set([stub])
  redirectExecutables(t, scripts)
  const before = process.env.BCA
  process.env.BCA = stub
  const messages = []
  for (const method of ["info", "warn", "debug"]) {
    t.mock.method(console, method, (text) => messages.push([method, text]))
  }
  let hook
  let location = root
  const sessionIDs = []
  const records = []
  const ctx = {
    location: { directory: root },
    storage: { set: async (key, value) => { records.push([key, structuredClone(value)]) } },
    session: { get: async ({ sessionID }) => {
      sessionIDs.push(sessionID)
      return { location: { directory: location } }
    } },
    tool: { hook: async (name, callback) => {
      assert.equal(name, "execute.after")
      assert.equal(hook, undefined, "one registration per setup")
      hook = callback
    } },
  }
  const cleanup = await plugin.setup(ctx)
  t.after(async () => {
    cleanup()
    if (before === undefined) delete process.env.BCA
    else process.env.BCA = before
    await rm(directory, { recursive: true, force: true })
  })
  return {
    directory, root, stub, scripts, hooks, runGit, hook, cleanup, ctx, messages, sessionIDs, records,
    move: (next) => { location = next },
    async file(name = "sample.rs", data = {}) {
      const path = resolve(root, name)
      await mkdir(resolve(path, ".."), { recursive: true })
      await writeFile(path, JSON.stringify(data))
      return path
    },
    async calls() {
      try {
        return (await readFile(join(directory, "calls.jsonl"), "utf8")).trim().split("\n").map(JSON.parse)
      } catch (error) {
        if (error.code === "ENOENT") return []
        throw error
      }
    },
  }
}

function event(path, overrides = {}) {
  return {
    tool: "edit", status: "completed", sessionID: "session-a", id: "call-a",
    input: { path }, result: { content: "Edited successfully", output: { edited: true }, metadata: { kept: 7 } },
    ...overrides,
  }
}

function advice(e) {
  return Array.isArray(e.result.content) ? e.result.content.at(-1)?.text : undefined
}

function controlledAnalyzer(t, f) {
  const requests = []
  const waiters = []
  const original = childProcess.spawn
  const mock = t.mock.method(childProcess, "spawn", (file, args, options) => {
    if (file !== f.stub) return original(file, args, options)
    const child = new EventEmitter()
    child.stdout = new EventEmitter()
    child.stderr = new EventEmitter()
    child.stdout.destroy = child.stderr.destroy = () => {}
    let completed = false
    const request = { aborted: false, complete(code, rows = ROW) {
      if (completed) return
      completed = true
      options.signal?.removeEventListener("abort", abort)
      child.stdout.emit("data", Buffer.from(rows))
      queueMicrotask(() => child.emit("close", code))
    } }
    function abort() {
      request.aborted = true
      request.complete("ABORT_ERR", "")
    }
    child.kill = (signal) => {
      assert.equal(signal, "SIGKILL")
      abort()
      return true
    }
    options.signal?.addEventListener("abort", abort, { once: true })
    const waiter = waiters.shift()
    if (waiter) waiter(request)
    else requests.push(request)
    return child
  })
  syncBuiltinESMExports()
  t.after(() => {
    mock.mock.restore()
    syncBuiltinESMExports()
  })
  return () => requests.length ? Promise.resolve(requests.shift()) : new Promise((done) => waiters.push(done))
}

test("V2 default export registers a hook and preserves every original result field", async (t) => {
  assert.equal(plugin.id, "bca-check")
  const f = await fixture(t)
  const path = await f.file()
  for (const [index, content] of ["success", [{ type: "file", uri: "file:///artifact", mime: "text/plain" }], undefined].entries()) {
    const e = event(path, { sessionID: `content-${index}`, result: { content, output: { original: 3 }, metadata: { diagnostic: "kept" } } })
    const original = structuredClone(e.result)
    await f.hook(e)
    assert.deepEqual(e.result.output, original.output)
    assert.deepEqual(e.result.metadata, original.metadata)
    assert.deepEqual(e.result.content.slice(0, -1), typeof content === "string" ? [{ type: "text", text: content }] : content ?? [])
    assert.equal(advice(e), `bca advisory — findings are not necessarily caused by this edit.\n\nCurrent threshold findings in ${named("sample.rs")}:\n${ROW}\n${GUIDANCE}`)
    assert.equal(e.status, "completed")
  }
  assert.deepEqual((await f.calls())[0], {
    cwd: f.root,
    args: ["check", named("sample.rs"), "--no-summary", "--no-remediation", "--summary-file", "never", "--github-annotations=never"],
  })
})

test("edit/write input.path fallback and multi-file patch metadata select the actual files", async (t) => {
  const f = await fixture(t)
  const a = await f.file("space ' quote.rs")
  const b = await f.file("second.rs", { rows: "second finding" })
  for (const tool of ["edit", "write"]) {
    const e = event(a, { tool, sessionID: tool })
    await f.hook(e)
    assert.match(advice(e), /cognitive = 20/)
  }
  const e = event(undefined, { tool: "patch", input: { patchText: "not parsed" }, result: {
    content: "Patched", metadata: { files: [{ file: a }, { file: b }, { file: a }, { file: join(f.root, "deleted.rs") }] },
  } })
  await f.hook(e)
  assert.match(advice(e), /second finding/)
  assert.equal((advice(e).match(/Current threshold/g) ?? []).length, 2)
  assert.equal((await f.calls()).length, 4)
})

test("V2 edit metadata and write output select normalized paths without expanding tildes twice", async (t) => {
  const f = await fixture(t)
  const path = await f.file("~/sample.rs")
  const edit = event("a-raw-path-not-used.rs", { result: {
    content: "Edited successfully", metadata: { files: [{ file: "~/sample.rs", status: "modified" }] },
  } })
  await f.hook(edit)
  assert.match(advice(edit), /cognitive = 20/)
  const write = event("another-raw-path-not-used.rs", { tool: "write", sessionID: "writer", result: {
    content: "Written successfully", output: { operation: "write", target: path, resource: "~/sample.rs", existed: true },
  } })
  await f.hook(write)
  assert.match(advice(write), /cognitive = 20/)
  assert.deepEqual((await f.calls()).map((call) => call.args[1]), [named("~/sample.rs"), named("~/sample.rs")])
})

test("failed and unrelated calls never analyze an existing offender", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  for (const e of [event(path, { status: "error" }), event(path, { status: "error", tool: "write" }),
    event(path, { tool: "read" }),
    event(undefined, { status: "error", tool: "patch", result: { metadata: { files: [{ file: path }] } } })]) {
    const before = structuredClone(e)
    await f.hook(e)
    assert.deepEqual(e, before)
  }
  assert.deepEqual(await f.calls(), [])
})

test("malformed successful events stay model-silent", async (t) => {
  const f = await fixture(t)
  for (const e of [
    event(undefined), event("", { tool: "write" }),
    event(undefined, { tool: "patch", result: { content: "ok" } }),
    event(undefined, { tool: "patch", result: { metadata: { files: [null] } } }),
  ]) {
    const before = structuredClone(e)
    await f.hook(e)
    assert.deepEqual(e, before)
  }
  assert.deepEqual(await f.calls(), [])
  assert.ok(f.messages.some(([level, text]) => level === "warn" && text.includes("input.path")))
})

test("only numeric exits 2–5 with stdout findings become advisories", async (t) => {
  const f = await fixture(t)
  for (const code of [0, 1, 2, 3, 4, 5, 7, 101]) {
    const e = event(await f.file(`exit-${code}.rs`, { code, stderr: "diagnostic, not an offender" }))
    const before = structuredClone(e)
    await f.hook(e)
    if (code >= 2 && code <= 5) {
      assert.match(advice(e), /cognitive = 20/)
      assert.doesNotMatch(advice(e), /diagnostic, not an offender/)
    } else assert.deepEqual(e, before)
  }
  const empty = event(await f.file("empty.rs", { code: 2, rows: "", stderr: "warning" }))
  await f.hook(empty)
  assert.equal(empty.result.content, "Edited successfully")
})

test("explicit unavailable BCA, buffer overflow and timeout never erase tool success", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  process.env.BCA = join(f.directory, "missing")
  const missing = event(path)
  await f.hook(missing)
  assert.equal(missing.result.content, "Edited successfully")
  assert.deepEqual(await f.calls(), [])
  process.env.BCA = f.stub
  for (const data of [{ rows: "x".repeat(100_000) }, { hang: true }]) {
    const e = event(await f.file("bad.rs", data))
    await f.hook(e)
    assert.equal(e.result.content, "Edited successfully")
  }
  assert.ok(f.messages.some(([level, text]) => level === "warn" && text.includes("unavailable")))
})

test("containment rejects directories, sibling-prefix paths and escaping symlinks", async (t) => {
  const f = await fixture(t)
  const outside = await f.file("../checkout-sibling/outside.rs")
  const link = join(f.root, "escape.rs")
  await symlink(outside, link)
  for (const path of [outside, link, f.root, join(f.root, "absent.rs")]) {
    const e = event(path)
    await f.hook(e)
    assert.equal(e.result.content, "Edited successfully")
  }
  assert.deepEqual(await f.calls(), [])
})

test("same session moved between checkout, nested directory and Git worktree uses current location", async (t) => {
  const f = await fixture(t)
  f.runGit(["-C", f.root, "-c", "commit.gpgsign=false", "-c", `core.hooksPath=${f.hooks}`, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture", "--allow-empty"])
  const worktree = join(f.directory, "worktree")
  f.runGit(["-C", f.root, "-c", `core.hooksPath=${f.hooks}`, "worktree", "add", "--detach", worktree], { stdio: "ignore" })
  await f.file("sample.rs")
  await f.file("nested/sample.rs", { rows: "nested finding" })
  await writeFile(join(worktree, "sample.rs"), JSON.stringify({ rows: ROW }))
  for (const directory of [f.root, join(f.root, "nested"), worktree]) {
    f.move(directory)
    const e = event("sample.rs")
    await f.hook(e)
    assert.match(advice(e), /Current threshold findings/)
  }
  const calls = await f.calls()
  assert.deepEqual(calls.map((call) => call.cwd), [f.root, f.root, worktree])
  assert.deepEqual(calls.map((call) => call.args[1]), ["sample.rs", "nested/sample.rs", "sample.rs"].map(named))
  assert.deepEqual(f.sessionIDs, ["session-a", "session-a", "session-a"])
})

test("the worktree test works inside Git's inherited hook environment", () => {
  const env = { ...process.env }
  delete env.NODE_TEST_CONTEXT
  const result = spawnSync(process.execPath, [
    "--test", "--test-reporter=tap", "--test-name-pattern=^same session moved", fileURLToPath(import.meta.url),
  ], {
    encoding: "utf8",
    timeout: 30_000,
    env: { ...env, GIT_INDEX_FILE: ".git/index", GIT_DIR: "/nonexistent/outer-repo/.git",
      GIT_WORK_TREE: "/nonexistent/outer-repo", GIT_PREFIX: "nested/",
      GIT_CONFIG_COUNT: "2", GIT_CONFIG_KEY_0: "commit.gpgsign", GIT_CONFIG_VALUE_0: "true",
      GIT_CONFIG_KEY_1: "gpg.program", GIT_CONFIG_VALUE_1: "/nonexistent/gpg" },
  })
  assert.equal(result.status, 0, result.stdout + result.stderr)
  assert.match(result.stdout, /same session moved/)
  assert.match(result.stdout, /# pass 1\b/)
})

test("home-relative and symlinked checkout paths reach the same file", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const alias = join(f.directory, "alias")
  await symlink(f.root, alias, "dir")
  for (const [sessionID, directory, input] of [
    ["home", f.root, `~/${relative(homedir(), path)}`],
    ["alias", alias, join(alias, "sample.rs")],
    ["alias-relative", alias, "sample.rs"],
  ]) {
    f.move(directory)
    const e = event(input, { sessionID })
    await f.hook(e)
    assert.match(advice(e) ?? "", /cognitive = 20/)
  }
  assert.deepEqual((await f.calls()).map((c) => c.args[1]), Array(3).fill(named("sample.rs")))
})

test("overlapping completions append an identical report only once", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const events = Array.from({ length: 8 }, (_, id) => event(path, { id: `call-${id}` }))
  await Promise.all(events.map((e) => f.hook(e)))
  assert.equal(events.filter((e) => advice(e)?.includes("bca advisory")).length, 1)
})

test("a late offender cannot undo a newer clean check or silence a reintroduced violation", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const next = controlledAnalyzer(t, f)
  const old = event(path, { id: "older" })
  const older = f.hook(old)
  const oldCheck = await next()
  const clean = event(path, { id: "newer" })
  const newer = f.hook(clean)
  const cleanCheck = await next()
  cleanCheck.complete(0, "")
  await newer
  oldCheck.complete(2)
  await older

  const reintroduced = event(path)
  const again = f.hook(reintroduced)
  const reintroducedCheck = await next()
  reintroducedCheck.complete(2)
  await again
  assert.equal(old.result.content, "Edited successfully", "superseded findings must not be delivered")
  assert.equal(clean.result.content, "Edited successfully")
  assert.match(advice(reintroduced) ?? "", /cognitive = 20/)
})

test("a late clean check cannot clear a newer offender's dedup state", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const next = controlledAnalyzer(t, f)
  const older = f.hook(event(path))
  const oldCheck = await next()
  const finding = event(path)
  const newer = f.hook(finding)
  const newCheck = await next()
  newCheck.complete(2)
  await newer
  oldCheck.complete(0, "")
  await older
  const repeat = event(path)
  const again = f.hook(repeat)
  const repeatedCheck = await next()
  repeatedCheck.complete(2)
  await again
  assert.match(advice(finding) ?? "", /cognitive = 20/)
  assert.equal(repeat.result.content, "Edited successfully")
})

test("a newer edit supersedes findings waiting on the rest of a patch", async (t) => {
  const f = await fixture(t)
  const a = await f.file("a.rs")
  const b = await f.file("b.rs")
  const next = controlledAnalyzer(t, f)
  const patch = event(undefined, { tool: "patch", result: { content: "Patched", metadata: { files: [{ file: a }, { file: b }] } } })
  const older = f.hook(patch)
  const firstFile = await next()
  firstFile.complete(2)
  const secondFile = await next()
  const newer = f.hook(event(a))
  const newCheck = await next()
  newCheck.complete(0, "")
  await newer
  secondFile.complete(0, "")
  await older
  assert.equal(patch.result.content, "Patched")
})

test("external edits and a delayed session lookup cannot deliver an old file snapshot", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const next = controlledAnalyzer(t, f)
  const old = event(path)
  const running = f.hook(old)
  const analyzer = await next()
  await f.file("sample.rs", { code: 0, changed: true })
  analyzer.complete(2)
  await running
  assert.equal(old.result.content, "Edited successfully", "shell/editor changes must invalidate results too")

  const older = event(path)
  const pending = f.hook(older)
  const check = await next()
  let resolveSession
  f.ctx.session.get = () => new Promise((done) => { resolveSession = done })
  const latest = f.hook(event(path))
  assert.equal(check.aborted, true, "known absolute targets cancel before session lookup resolves")
  await pending
  resolveSession({ location: { directory: f.root } })
  const clean = await next()
  clean.complete(0, "")
  await latest
  assert.equal(older.result.content, "Edited successfully")
})

test("a missing target without a deleted marker clears delivery state", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const first = event(path)
  await f.hook(first)
  assert.match(advice(first), /cognitive = 20/)
  await rm(path)
  await f.hook(event(path))
  await f.file()
  const recreated = event(path)
  await f.hook(recreated)
  assert.match(advice(recreated), /cognitive = 20/)
})

test("checkout discovery ignores inherited Git repository selectors in production", async (t) => {
  const f = await fixture(t)
  const other = resolve(f.root, "../other")
  f.runGit(["init", "-q", other])
  const keys = ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR", "GIT_INDEX_FILE", "GIT_PREFIX"]
  const saved = Object.fromEntries(keys.map((key) => [key, process.env[key]]))
  try {
    Object.assign(process.env, { GIT_DIR: resolve(other, ".git"), GIT_WORK_TREE: other,
      GIT_COMMON_DIR: resolve(other, ".git"), GIT_INDEX_FILE: ".git/index", GIT_PREFIX: "foreign/" })
    assert.equal(await checkout(f.root), f.root)
  } finally {
    for (const key of keys) {
      if (saved[key] === undefined) delete process.env[key]
      else process.env[key] = saved[key]
    }
  }
})

test("guidance rejects FIFOs and reads only a bounded prefix of regular files", async (t) => {
  const f = await fixture(t)
  const path = resolve(f.root, ".claude/hooks/bca-guidance.txt")
  await writeFile(path, "a".repeat(100_000))
  assert.equal((await guidance(f.root)).length, 8_192)
  if (process.platform !== "win32") {
    await rm(path)
    execFileSync("mkfifo", [path])
    assert.match(await guidance(f.root), /Keep the task-boundary gate/)
  }
})

test("guidance rejects a nonregular source even when it has readable content", {
  skip: process.platform === "win32" && "POSIX character-device fixture",
}, async (t) => {
  const f = await fixture(t)
  const path = resolve(f.root, ".claude/hooks/bca-guidance.txt")
  await rm(path)
  await symlink("/dev/zero", path)
  const text = await guidance(f.root)
  assert.match(text, /Keep the task-boundary gate/)
  assert.doesNotMatch(text, /\0/)
})

test("a patch using two aliases releases its file state for cache eviction", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const alias = join(f.root, "alias.rs")
  await symlink(path, alias)
  const next = controlledAnalyzer(t, f)
  const first = event(undefined, { tool: "patch", result: {
    content: "Patched", metadata: { files: [{ file: path }, { file: alias }] },
  } })
  const initial = f.hook(first)
  for (let i = 0; i < 2; i++) (await next()).complete(2)
  await initial
  assert.equal((advice(first).match(/Current threshold findings/g) ?? []).length, 2)

  const repeated = event(path)
  const repeat = f.hook(repeated)
  const repeatCheck = await next()
  repeatCheck.complete(2)
  await repeat
  assert.equal(repeated.result.content, "Edited successfully", "delivery state exists before eviction")

  for (let i = 0; i < 256; i++) {
    const pending = f.hook(event(await f.file(`fill-${i}.rs`)))
    const check = await next()
    check.complete(0, "")
    await pending
  }
  const evicted = event(path)
  const afterEviction = f.hook(evicted)
  const afterEvictionCheck = await next()
  afterEvictionCheck.complete(2)
  await afterEviction
  assert.match(advice(evicted) ?? "", /cognitive = 20/, "the idle aliased state must be evictable")
})

test("fixtures canonicalize symlinked temporary directories", async (t) => {
  const parent = await realpath(await mkdtemp(join(tmpdir(), "bca tmp alias ")))
  t.after(() => rm(parent, { recursive: true, force: true }))
  const actual = join(parent, "actual")
  const alias = join(parent, "alias")
  await mkdir(actual)
  await symlink(actual, alias, "dir")
  const f = await fixture(t, { temporaryDirectory: alias })
  assert.equal(f.directory, await realpath(f.directory))
  const e = event(await f.file())
  await f.hook(e)
  assert.equal((await f.calls())[0].cwd, f.root)
  assert.equal(f.records.at(-1)[1].last.root, f.root)
})

test("non-Git location and missing/unreadable guidance use the concise fallback", async (t) => {
  const f = await fixture(t, { git: false })
  const path = await f.file()
  const guide = join(f.root, ".claude/hooks/bca-guidance.txt")
  await rm(guide)
  for (const sessionID of ["missing", "directory"]) {
    const e = event(path, { sessionID })
    await f.hook(e)
    assert.match(advice(e), /Keep the task-boundary gate/)
    if (sessionID === "missing") await mkdir(guide)
  }
})

test("dedup is session-scoped; unavailable preserves state and clean resets it", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const first = event(path)
  await f.hook(first)
  assert.match(advice(first), /cognitive = 20/)
  const repeat = event(path)
  await f.hook(repeat)
  assert.equal(repeat.result.content, "Edited successfully")
  await f.file("sample.rs", { code: 1 })
  await f.hook(event(path))
  await f.file()
  const afterFailure = event(path)
  await f.hook(afterFailure)
  assert.equal(afterFailure.result.content, "Edited successfully")
  const child = event(path, { sessionID: "child" })
  await f.hook(child)
  assert.match(advice(child), /cognitive = 20/)
  await f.file("sample.rs", { rows: "changed finding" })
  const changed = event(path)
  await f.hook(changed)
  assert.match(advice(changed), /changed finding/)
  await f.file("sample.rs", { code: 0 })
  await f.hook(event(path))
  await f.file("sample.rs", { rows: "changed finding" })
  const afterClean = event(path)
  await f.hook(afterClean)
  assert.match(advice(afterClean), /changed finding/)
})

test("health distinguishes clean, unavailable and repeated findings without recording source or rows", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  await f.hook(event(path))
  await f.hook(event(path))
  await f.file("sample.rs", { code: 0 })
  await f.hook(event(path))
  await f.file("sample.rs", { code: 1 })
  await f.hook(event(path))
  const [key, record] = f.records.at(-1)
  assert.equal(key, `health/${encodeURIComponent(f.root)}`)
  assert.equal(record.location, f.root)
  assert.match(record.loadID, /^[\da-f-]{36}$/)
  assert.ok(Number.isFinite(Date.parse(record.loadedAt)))
  assert.deepEqual(record.sinceLoad, { runs: 4, checked: 4, findings: 2, delivered: 1, unavailable: 1, omitted: 0 })
  assert.equal(record.last.root, f.root)
  assert.equal(record.last.analyzer, f.stub)
  assert.equal(record.last.sessionID, "session-a")
  assert.doesNotMatch(JSON.stringify(record), /cognitive = 20|sample.rs/)
  f.ctx.storage.set = async () => { throw new Error("storage failed") }
  await f.file("sample.rs", { rows: "fresh finding" })
  const e = event(path)
  await f.hook(e)
  assert.match(advice(e), /fresh finding/)
})

test("location instances sharing storage retain independent health and reload epochs", async (t) => {
  const f = await fixture(t)
  const path = await f.file("sample.rs", { code: 0 })
  await f.hook(event(path))
  await f.hook(event(path))
  const firstRecord = f.records.at(-1)[1]
  const other = await f.file("../worktree/sample.rs", { code: 0 })
  const otherLocation = resolve(other, "..")
  let otherHook
  const stopOther = await plugin.setup({ ...f.ctx,
    location: { directory: otherLocation },
    session: { get: async () => ({ location: { directory: otherLocation } }) },
    tool: { hook: async (_, callback) => { otherHook = callback } },
  })
  t.after(stopOther)
  await otherHook(event(other, { sessionID: "worktree-session" }))
  const stored = new Map(f.records)
  assert.equal(stored.size, 2, "one record per plugin location")
  const values = [...stored.values()]
  assert.deepEqual(values.map((record) => record.sinceLoad.runs), [2, 1])
  assert.deepEqual(values.map((record) => record.location), [f.root, otherLocation])
  assert.notEqual(values[0].loadID, values[1].loadID)

  f.cleanup()
  let reloadedHook
  const stopReloaded = await plugin.setup({ ...f.ctx,
    tool: { hook: async (_, callback) => { reloadedHook = callback } },
  })
  t.after(stopReloaded)
  await reloadedHook(event(path))
  const afterReload = new Map(f.records)
  assert.equal(afterReload.size, 2, "reload replaces only its own location record")
  const latest = [...afterReload.values()].find((record) => record.location === f.root)
  assert.notEqual(latest.loadID, firstRecord.loadID)
  assert.equal(latest.sinceLoad.runs, 1)
})

test("patch cap and report clipping are explicit; unseen findings are not deduplicated", async (t) => {
  const f = await fixture(t)
  const files = []
  for (let i = 0; i < 9; i++) files.push({ file: await f.file(`${i}.rs`, { rows: `finding ${i}` }) })
  const e = event(undefined, { tool: "patch", result: { metadata: { files } } })
  await f.hook(e)
  assert.equal((await f.calls()).length, 8)
  assert.match(advice(e), /1 further files not checked/)
  const ninth = event(files[8].file)
  await f.hook(ninth)
  assert.match(advice(ninth), /finding 8/)
  const large = await f.file("large.rs", { rows: "y".repeat(5_000) })
  for (let i = 0; i < 2; i++) {
    const clipped = event(large)
    await f.hook(clipped)
    assert.match(advice(clipped), /Report truncated/)
    assert.ok(advice(clipped).length < 4_500)
  }
})

test("combined patch advisory stays within one 4000-character budget", async (t) => {
  const f = await fixture(t)
  const files = []
  for (let i = 0; i < 8; i++) files.push({ file: await f.file(`${i}.rs`, { rows: `finding ${i}\n${"x".repeat(3_990)}` }) })
  const e = event(undefined, { tool: "patch", result: { metadata: { files } } })
  await f.hook(e)
  assert.ok(advice(e).length <= 4_000)
  assert.match(advice(e), /further findings omitted/)
  assert.equal(f.records.at(-1)[1].last.delivered, 1)
  const omitted = event(files[7].file)
  await f.hook(omitted)
  assert.match(advice(omitted), /finding 7/)
})

test("omitted patch targets incur no filesystem inspection", async (t) => {
  const f = await fixture(t)
  const files = []
  for (let i = 0; i < 8; i++) files.push({ file: await f.file(`${i}.rs`, { code: 0 }) })
  for (let i = 8; i < 1_024; i++) files.push({ file: join(f.root, `tail-${i}.rs`) })
  const inspected = []
  for (const name of ["realpath", "stat"]) {
    const original = fsPromises[name]
    const mocked = t.mock.method(fsPromises, name, async (path, ...args) => {
      if (String(path).includes("tail-")) inspected.push(path)
      return original(path, ...args)
    })
    t.after(() => { mocked.mock.restore(); syncBuiltinESMExports() })
  }
  syncBuiltinESMExports()
  const e = event(undefined, { tool: "patch", result: { metadata: { files } } })
  await f.hook(e)
  assert.deepEqual(inspected, [])
  assert.equal((await f.calls()).length, 8)
  assert.match(advice(e), /1016 further files not checked/)
})

function deletion(path) {
  return event(undefined, { tool: "patch", result: { content: "Deleted", metadata: { files: [{ file: path, status: "deleted" }] } } })
}

test("deleting a checked alias clears findings before recreation", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const alias = join(f.root, "alias.rs")
  await symlink(path, alias)
  const initial = event(alias)
  await f.hook(initial)
  assert.match(advice(initial), /cognitive = 20/)
  await rm(alias)
  await f.hook(deletion(alias))
  await symlink(path, alias)
  const recreated = event(alias)
  await f.hook(recreated)
  assert.match(advice(recreated), /cognitive = 20/)
})

test("deletion beyond the patch cap cancels pending checks before the first file runs", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const next = controlledAnalyzer(t, f)
  const e = event(path)
  const outstanding = f.hook(e)
  const running = await next()
  await rm(path)
  const files = []
  for (let i = 0; i < 8; i++) files.push({ file: join(f.root, `missing-${i}.rs`) })
  files.push({ file: path, status: "deleted" })
  await f.hook(event(undefined, { tool: "patch", result: { metadata: { files } } }))
  await outstanding
  assert.equal(running.aborted, true)
  assert.equal(e.result.content, "Edited successfully")
})

test("a child's clean check resets the parent's delivery history and cancels its stale check", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  await f.hook(event(path))
  await f.file("sample.rs", { code: 0 })
  await f.hook(event(path, { sessionID: "child" }))
  await f.file()
  const reintroduced = event(path)
  await f.hook(reintroduced)
  assert.match(advice(reintroduced), /cognitive = 20/)

  const next = controlledAnalyzer(t, f)
  const old = event(path)
  const outstanding = f.hook(old)
  const running = await next()
  const newer = f.hook(event(path, { sessionID: "child" }))
  const clean = await next()
  assert.equal(running.aborted, true)
  clean.complete(0, "")
  await Promise.all([newer, outstanding])
  assert.equal(old.result.content, "Edited successfully")
})

test("unload aborts an outstanding analyzer without delivery or a health write", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const next = controlledAnalyzer(t, f)
  const e = event(path)
  const outstanding = f.hook(e)
  const running = await next()
  f.cleanup()
  await outstanding
  assert.equal(running.aborted, true)
  assert.equal(e.result.content, "Edited successfully")
  assert.deepEqual(f.records, [])
})

test("supersession kills a real SIGTERM-resistant analyzer and lets its replacement finish", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  let ready
  const started = new Promise((done) => { ready = done })
  const original = childProcess.spawn
  const children = []
  const mock = t.mock.method(childProcess, "spawn", (file, args, options) => {
    if (file !== f.stub) return original(file, args, options)
    const code = children.length === 0 ?
      'process.on("SIGTERM", () => {}); console.log("ready"); setInterval(() => {}, 1000)' :
      'console.log("replacement finding"); process.exitCode = 2'
    const child = original(process.execPath, ["-e", code], options)
    children.push(child)
    child.stdout.once("data", ready)
    return child
  })
  syncBuiltinESMExports()
  let timer
  try {
    const olderEvent = event(path)
    const older = f.hook(olderEvent)
    await started
    const replacement = event(path)
    const newer = f.hook(replacement)
    await Promise.race([
      Promise.all([older, newer]),
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("cancelled analyzer did not close")), 5_000) }),
    ])
    assert.equal(children.length, 2)
    assert.ok(children[0].exitCode !== null || children[0].signalCode !== null)
    assert.equal(olderEvent.result.content, "Edited successfully")
    assert.match(advice(replacement), /replacement finding/)
  } finally {
    clearTimeout(timer)
    f.cleanup()
    for (const child of children) child.kill("SIGKILL")
    mock.mock.restore()
    syncBuiltinESMExports()
  }
})

function moveEvent(from, to) {
  return event(undefined, { tool: "patch",
    input: { patchText: `*** Begin Patch\n*** Update File: ${from}\n*** Move to: ${to}\n@@\n-old\n+new\n*** End Patch` },
    result: { content: "Moved", metadata: { files: [{ file: to, status: "modified" }] } },
  })
}

test("a wrapper's inherited output pipes cannot outlive the analyzer timeout", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  const original = childProcess.spawn
  let child
  let descendant
  let ready
  const started = new Promise((done) => { ready = done })
  const mock = t.mock.method(childProcess, "spawn", (file, args, options) => {
    if (file !== f.stub) return original(file, args, options)
    const code = 'const cp = require("node:child_process"); ' +
      'const c = cp.spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {stdio: ["ignore", 1, 2]}); ' +
      'console.log(c.pid); setInterval(() => {}, 1000)'
    child = original(process.execPath, ["-e", code], options)
    child.stdout.once("data", (data) => { descendant = Number(data.toString().trim()); ready() })
    return child
  })
  syncBuiltinESMExports()
  let timer
  try {
    const e = event(path)
    const running = f.hook(e)
    await started
    assert.ok(Number.isInteger(descendant) && descendant > 0)
    await Promise.race([
      running,
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("inherited pipes kept the hook open")), 5_000) }),
    ])
    assert.equal(e.result.content, "Edited successfully")
    assert.equal(f.records.at(-1)[1].last.unavailable, 1)
    if (process.platform !== "win32") {
      // Orphaned descendants may briefly remain as zombies until init reaps them.
      const status = spawnSync("ps", ["-o", "stat=", "-p", String(descendant)], { encoding: "utf8" }).stdout.trim()
      assert.ok(!status || status.startsWith("Z"), `descendant still running: ${status}`)
    } else {
      assert.throws(() => process.kill(descendant, 0), { code: "ESRCH" })
    }
  } finally {
    clearTimeout(timer)
    f.cleanup()
    child?.kill("SIGKILL")
    if (Number.isInteger(descendant) && descendant > 0) {
      try { process.kill(descendant, "SIGKILL") } catch (error) { if (error.code !== "ESRCH") throw error }
    }
    mock.mock.restore()
    syncBuiltinESMExports()
  }
})

test("patch moves invalidate the absent source before destination analysis and recreation", async (t) => {
  const f = await fixture(t)
  const from = await f.file()
  const to = join(f.root, "moved.rs")
  await f.hook(event(from))
  await rename(from, to)
  await f.hook(moveEvent(from, to))
  await f.file()
  const recreated = event(from)
  await f.hook(recreated)
  assert.match(advice(recreated), /cognitive = 20/)

  const next = controlledAnalyzer(t, f)
  const old = event(from)
  const outstanding = f.hook(old)
  const running = await next()
  await rm(to)
  await rename(from, to)
  const moving = f.hook(moveEvent(from, to))
  const destination = await next()
  assert.equal(running.aborted, true)
  destination.complete(0, "")
  await Promise.all([outstanding, moving])
  assert.equal(old.result.content, "Edited successfully")
})

test("move source invalidation accepts V2 header whitespace and intervening EOF markers", async (t) => {
  const f = await fixture(t)
  const from = join(f.root, "sample.rs")
  const to = join(f.root, "destination.rs")
  for (const header of [
    "  *** Update File: sample.rs   \n*** Move to: destination.rs   ",
    "*** Update File: sample.rs\n*** End of File\n*** End of File  \n*** Move to: destination.rs",
  ]) {
    await f.file()
    const initial = event(from)
    await f.hook(initial)
    await rm(to, { force: true })
    await rename(from, to)
    const moved = moveEvent(from, to)
    moved.input.patchText = `*** Begin Patch\n${header}\n@@\n-old\n+new\n*** End Patch`
    await f.hook(moved)
    await f.file()
    const recreated = event(from)
    await f.hook(recreated)
    assert.match(advice(recreated) ?? "", /cognitive = 20/, header)
  }
})

test("raw Windows shell paths normalize once while normalized patch resources preserve literal tildes", async (t) => {
  for (const path of ["/c/repo/sample.rs", "/c:/repo/sample.rs", "/cygdrive/c/repo/sample.rs", "/mnt/c/repo/sample.rs", "C:\\repo\\sample.rs"]) {
    assert.equal(resolveInputPath("C:\\repo", path, "win32", "C:\\Users\\dev"), "C:\\repo\\sample.rs")
  }
  assert.equal(resolveInputPath("C:\\repo", "~\\repo\\sample.rs", "win32", "C:\\Users\\dev"), "C:\\Users\\dev\\repo\\sample.rs")
  assert.equal(resolveInputPath("/repo", "/c/repo/sample.rs", "linux", "/home/dev"), "/c/repo/sample.rs")
  const f = await fixture(t)
  await f.file("~/sample.rs")
  const e = event(undefined, { tool: "patch", result: { metadata: { files: [{ file: "~/sample.rs" }] } } })
  await f.hook(e)
  assert.match(advice(e), /cognitive = 20/)
  assert.equal((await f.calls())[0].args[1], named("~/sample.rs"))
})

test("cleanup disables old registrations and explicit BCA takes precedence over local builds", async (t) => {
  const f = await fixture(t)
  const path = await f.file()
  await mkdir(join(f.root, "target/release"), { recursive: true })
  const local = join(f.root, `target/release/bca${EXE}`)
  await writeFile(local, `#!${process.execPath}\nconsole.log("local build"); process.exitCode = 2;`, { mode: 0o755 })
  f.scripts.add(local)
  const overridden = event(path)
  await f.hook(overridden)
  assert.match(advice(overridden), /cognitive = 20/)
  delete process.env.BCA
  const localEvent = event(path)
  await f.hook(localEvent)
  assert.match(advice(localEvent), /local build/)
  f.cleanup()
  const unloaded = event(path, { sessionID: "after-unload" })
  await f.hook(unloaded)
  assert.equal(unloaded.result.content, "Edited successfully")
})

const integrationBca = process.env.BCA_TEST_BINARY || "bca"
const realBca = spawnSync(integrationBca, ["--version"], { encoding: "utf8" })
test("real analyzer respects explicit-path exclusions and a baseline", { skip: !process.env.BCA_TEST_BINARY && realBca.status !== 0 && "bca not found; unit contract tests still run" }, async (t) => {
  assert.equal(realBca.status, 0, "explicit integration analyzer must be executable")
  const f = await fixture(t)
  process.env.BCA = integrationBca
  const source = "fn f(x: bool) { if x { while x { if x {} } } }\n"
  await writeFile(join(f.root, "sample.rs"), source)
  await mkdir(join(f.root, ".opencode"))
  await writeFile(join(f.root, ".opencode/excluded.rs"), source)
  await writeFile(join(f.root, "bca.toml"), '[thresholds]\ncognitive = 1\n[check]\nexclude = ["./.opencode/**"]\n')
  const newFinding = event(join(f.root, "sample.rs"))
  await f.hook(newFinding)
  assert.match(advice(newFinding), /cognitive/)
  const excluded = event(join(f.root, ".opencode/excluded.rs"))
  await f.hook(excluded)
  assert.equal(excluded.result.content, "Edited successfully")
  await symlink(join(f.root, "sample.rs"), join(f.root, ".opencode/alias.rs"))
  const excludedAlias = event(join(f.root, ".opencode/alias.rs"), { sessionID: "alias-session" })
  await f.hook(excludedAlias)
  assert.equal(excludedAlias.result.content, "Edited successfully")
  const paired = event(undefined, { tool: "patch", sessionID: "paired-alias", result: {
    metadata: { files: [{ file: join(f.root, ".opencode/alias.rs") }, { file: join(f.root, "sample.rs") }] },
  } })
  await f.hook(paired)
  assert.match(advice(paired) ?? "", /sample.rs.*cognitive/)
  execFileSync(integrationBca, ["check", "./sample.rs", "--write-baseline", ".baseline.toml"], { cwd: f.root, stdio: "ignore" })
  await writeFile(join(f.root, "bca.toml"), '[thresholds]\ncognitive = 1\n[check]\nbaseline = ".baseline.toml"\n')
  const covered = event(join(f.root, "sample.rs"), { sessionID: "covered" })
  await f.hook(covered)
  assert.equal(covered.result.content, "Edited successfully")
  await writeFile(join(f.root, "sample.rs"), source.replace("if x {}", "if x { if x {} }"))
  const regressed = event(join(f.root, "sample.rs"))
  await f.hook(regressed)
  assert.match(advice(regressed), /regr/)
})

test("real analyzer selects a nearer manifest and an explicitly targeted nested checkout", { skip: !process.env.BCA_TEST_BINARY && realBca.status !== 0 }, async (t) => {
  const f = await fixture(t)
  process.env.BCA = integrationBca
  const source = "fn f(x: bool) { if x { while x { if x {} } } }\n"
  await mkdir(join(f.root, "package"))
  await writeFile(join(f.root, "bca.toml"), '[thresholds]\ncognitive = 100\n')
  await writeFile(join(f.root, "package/bca.toml"), '[thresholds]\ncognitive = 1\n')
  await writeFile(join(f.root, "package/sample.rs"), source)
  f.move(join(f.root, "package"))
  const strict = event("sample.rs")
  await f.hook(strict)
  assert.match(advice(strict) ?? "", /cognitive = 6 \(limit 1\)/)
  const nested = join(f.root, "nested")
  f.runGit(["init", "-q", nested])
  await writeFile(join(nested, "bca.toml"), '[thresholds]\ncognitive = 100\n')
  await writeFile(join(nested, "sample.rs"), source)
  await writeFile(join(f.root, "bca.toml"), '[thresholds]\ncognitive = 1\n')
  f.move(f.root)
  const child = event(join(nested, "sample.rs"))
  await f.hook(child)
  assert.equal(child.result.content, "Edited successfully")
  assert.equal(f.records.at(-1)[1].last.cwd, nested)
})

test("same-checkout sibling packages keep the session manifest while gitfiles stop inheritance", {
  skip: !process.env.BCA_TEST_BINARY && realBca.status !== 0,
}, async (t) => {
  const f = await fixture(t)
  process.env.BCA = integrationBca
  const packageA = join(f.root, "package-a")
  const packageB = join(f.root, "package-b")
  await mkdir(packageA)
  await mkdir(packageB)
  await writeFile(join(packageA, "bca.toml"), '[thresholds]\ncognitive = 1\n')
  await writeFile(join(packageB, "bca.toml"), '[thresholds]\ncognitive = 100\n')
  const source = "fn f(x: bool) { if x { while x { if x {} } } }\n"
  const target = join(packageB, "sample.rs")
  await writeFile(target, source)
  f.move(packageA)
  const sibling = event(target)
  await f.hook(sibling)
  assert.match(advice(sibling) ?? "", /cognitive = 6 \(limit 1\)/)
  assert.equal(f.records.at(-1)[1].last.cwd, packageA)

  f.move(packageB)
  const ownPackage = event(target)
  await f.hook(ownPackage)
  assert.equal(ownPackage.result.content, "Edited successfully")
  assert.equal(f.records.at(-1)[1].last.cwd, packageB)

  const nested = join(f.root, "submodule-shaped")
  f.runGit(["init", "-q", "--separate-git-dir", join(f.directory, "nested.git"), nested])
  await writeFile(join(f.root, "bca.toml"), '[thresholds]\ncognitive = 100\n')
  await writeFile(join(nested, "sample.rs"), source)
  f.move(f.root)
  const noManifest = event(join(nested, "sample.rs"))
  await f.hook(noManifest)
  assert.equal(noManifest.result.content, "Edited successfully")
  const health = f.records.at(-1)[1].last
  assert.equal(health.cwd, nested)
  assert.equal(health.unavailable, 1)
  assert.match(health.diagnostic, /no thresholds configured/)
})
