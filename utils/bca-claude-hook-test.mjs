import assert from "node:assert/strict"
import { execFileSync, spawn, spawnSync } from "node:child_process"
import { once } from "node:events"
import { mkdtemp, mkdir, readFile, realpath, rm, symlink, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join, resolve, sep } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"
import test from "node:test"

const REPO = resolve(fileURLToPath(new URL("../", import.meta.url)))
const HOOK = join(REPO, ".claude/hooks/bca-check.mjs")
const ROW = '[new] ./sample.rs:1-3: "quoted\\name": cognitive = 20 (limit 15)'
const GUIDANCE = "Keep the change focused."
const baseEnv = () => Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith("GIT_") && key !== "BCA"))

async function fixture(t) {
  const temp = await realpath(await mkdtemp(join(tmpdir(), "bca claude test ")))
  t.after(() => rm(temp, { recursive: true, force: true }))
  const root = join(temp, "checkout")
  const config = join(temp, "gitconfig")
  await writeFile(config, "")
  const env = { ...baseEnv(), GIT_CONFIG_GLOBAL: config, GIT_CONFIG_NOSYSTEM: "1", CLAUDE_PROJECT_DIR: join(temp, "original-checkout") }
  const git = (args) => execFileSync("git", args, { env, stdio: "ignore" })
  await mkdir(join(root, ".claude/hooks"), { recursive: true })
  git(["init", "-q", root])
  await mkdir(join(temp, "empty-hooks"))
  git(["-C", root, "config", "core.hooksPath", join(temp, "empty-hooks")])
  await writeFile(join(root, ".claude/hooks/bca-guidance.txt"), GUIDANCE)
  const stub = join(temp, "analyzer.mjs")
  const calls = join(temp, "calls.jsonl")
  await writeFile(stub, `
import { readFileSync, writeFileSync, appendFileSync, unlinkSync } from "node:fs";
const args = process.argv.slice(2);
appendFileSync(${JSON.stringify(calls)}, JSON.stringify({ cwd: process.cwd(), args }) + "\\n");
const data = JSON.parse(readFileSync(args[1], "utf8"));
if (data.replace) writeFileSync(args[1], "changed while analyzing");
if (data.sameSize) writeFileSync(args[1], "x".repeat(readFileSync(args[1]).length));
if (data.remove) unlinkSync(args[1]);
if (data.hang) { process.on("SIGTERM", () => {}); setInterval(() => {}, 1000); }
else {
  process.stdout.write(data.rows ?? ${JSON.stringify(ROW)});
  process.stderr.write(data.stderr ?? "");
  process.exitCode = data.code ?? 2;
}
`)
  const preload = join(temp, "preload.mjs")
  await writeFile(preload, `
import cp from "node:child_process";
import { syncBuiltinESMExports } from "node:module";
const original = cp.spawn;
cp.spawn = (file, args, options) => file === ${JSON.stringify(stub)} || file.endsWith("/target/release/bca") || file.endsWith("\\\\target\\\\release\\\\bca.exe")
  ? original(process.execPath, [file, ...args], options)
  : original(file, args, options);
syncBuiltinESMExports();
`)
  const path = join(root, "sample.rs")
  await writeFile(path, "{}")
  return {
    temp, root, path, env, stub, preload, git,
    file: (data) => writeFile(path, JSON.stringify(data)),
    input: (overrides = {}) => ({ hook_event_name: "PostToolUse", tool_name: "Edit", cwd: root,
      session_id: "session-a", tool_input: { file_path: path }, ...overrides }),
    run(input, overrides = {}, script = HOOK) {
      return spawnSync(process.execPath, ["--import", pathToFileURL(preload).href, script], {
        env: { ...env, BCA: stub, ...overrides }, cwd: temp,
        input: typeof input === "string" ? input : JSON.stringify(input), encoding: "utf8", timeout: 15_000, maxBuffer: 512 * 1024,
      })
    },
    async calls() {
      try { return (await readFile(calls, "utf8")).trim().split("\n").map(JSON.parse) }
      catch (error) { if (error.code === "ENOENT") return []; throw error }
    },
  }
}

function report(result) {
  assert.equal(result.status, 0, result.stderr)
  const output = JSON.parse(result.stdout)
  assert.deepEqual(Object.keys(output), ["hookSpecificOutput"])
  assert.deepEqual(Object.keys(output.hookSpecificOutput), ["hookEventName", "additionalContext"])
  assert.equal(output.hookSpecificOutput.hookEventName, "PostToolUse")
  return output.hookSpecificOutput.additionalContext
}

function quiet(result, outcome) {
  assert.equal(result.status, 0, result.stderr)
  assert.equal(result.stdout, "")
  if (outcome) assert.equal(JSON.parse(result.stderr.slice("[bca-check] ".length)).outcome, outcome)
}

test("configured exec hook sends escaped advisory JSON for every file-writing tool", async (t) => {
  const f = await fixture(t)
  const settings = JSON.parse(await readFile(join(REPO, ".claude/settings.json"), "utf8"))
  assert.deepEqual(Object.keys(settings), ["hooks"])
  const [group] = settings.hooks.PostToolUse
  const [handler] = group.hooks
  assert.equal(handler.type, "command")
  assert.equal(handler.command, "node")
  assert.equal(handler.timeout, 10)
  const script = handler.args[0].replace("${CLAUDE_PROJECT_DIR}", REPO)
  assert.equal(resolve(script), HOOK)
  for (const tool of ["Edit", "Write", "MultiEdit"]) {
    assert.ok(group.matcher.split("|").includes(tool))
    const result = f.run(f.input({ tool_name: tool, agent_id: "child-a" }), {}, script)
    const text = report(result)
    assert.ok(text.includes(ROW))
    assert.ok(text.endsWith(GUIDANCE))
    assert.match(text, /not necessarily caused by this edit/)
    const health = JSON.parse(result.stderr.slice("[bca-check] ".length))
    assert.equal(health.outcome, "findings")
    assert.equal(health.agent, "child-a")
    assert.equal(health.root, f.root)
    assert.ok(!result.stderr.includes(ROW))
  }
  assert.deepEqual((await f.calls())[0], { cwd: f.root, args: ["check", `.${sep}sample.rs`, "--no-summary", "--no-remediation", "--summary-file", "never", "--github-annotations=never"] })
})

test("only exits 2–5 with stdout rows produce context; diagnostics stay out", async (t) => {
  const f = await fixture(t)
  for (const code of [0, 1, 2, 3, 4, 5, 7, 101]) {
    await f.file({ code, stderr: "analyzer diagnostic" })
    const result = f.run(f.input())
    if (code >= 2 && code <= 5) {
      assert.ok(report(result).includes(ROW))
      assert.ok(!report(result).includes("analyzer diagnostic"))
    } else quiet(result, code === 0 ? "clean-or-excluded" : "unavailable")
  }
  await f.file({ rows: "", code: 2, stderr: "not a finding" })
  quiet(f.run(f.input()), "unavailable")
})

test("failed/unrelated events never run the analyzer on an existing offender", async (t) => {
  const f = await fixture(t)
  for (const input of [f.input({ hook_event_name: "PostToolUseFailure" }), f.input({ tool_name: "Read" }), f.input({ tool_name: "Bash" }), null]) {
    const result = f.run(input)
    quiet(result)
    assert.equal(result.stderr, "")
  }
  assert.deepEqual(await f.calls(), [])
})

test("malformed input and unsupported paths degrade without injecting context", async (t) => {
  const f = await fixture(t)
  for (const input of ["{bad", f.input({ cwd: null }), f.input({ tool_input: {} }), f.input({ tool_input: { file_path: "relative.rs" } })]) quiet(f.run(input), "unavailable")
  await rm(f.path)
  quiet(f.run(f.input()), "missing")
  const outside = join(f.temp, "outside.rs")
  await writeFile(outside, "{}")
  await symlink(outside, f.path)
  quiet(f.run(f.input()), "out-of-scope")
  quiet(f.run(f.input({ tool_input: { file_path: f.root } })), "out-of-scope")
  assert.deepEqual(await f.calls(), [])
})

test("event cwd follows nested directories and worktrees instead of CLAUDE_PROJECT_DIR", async (t) => {
  const f = await fixture(t)
  await mkdir(join(f.root, "nested"))
  report(f.run(f.input({ cwd: join(f.root, "nested") })))
  f.git(["-C", f.root, "-c", "commit.gpgsign=false", "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture", "--allow-empty"])
  const worktree = join(f.temp, "worktree")
  f.git(["-C", f.root, "worktree", "add", "--detach", worktree])
  const target = join(worktree, "space ' file.rs")
  await writeFile(target, "{}")
  report(f.run(f.input({ cwd: worktree, tool_input: { file_path: target } })))
  assert.deepEqual((await f.calls()).map((call) => call.cwd), [f.root, worktree])
})

test("a symlinked project entrypoint still runs the hook", async (t) => {
  const f = await fixture(t)
  const alias = join(f.temp, "project-alias")
  await symlink(REPO, alias, "dir")
  assert.ok(report(f.run(f.input(), {}, join(alias, ".claude/hooks/bca-check.mjs"))).includes(ROW))
})

test("cancelling the command while reading stdin terminates without advisory context", {
  skip: process.platform === "win32" && "Windows terminates SIGTERM targets without invoking a JavaScript handler",
}, async (t) => {
  const f = await fixture(t)
  const ready = join(f.temp, "ready.mjs")
  await writeFile(ready, 'process.on("newListener", (name) => { if (name === "SIGINT") queueMicrotask(() => process.stderr.write("ready\\n")); });')
  const child = spawn(process.execPath, ["--import", pathToFileURL(ready).href, HOOK], { env: f.env, cwd: f.temp })
  t.after(() => child.kill("SIGKILL"))
  let stdout = ""
  child.stdout.on("data", (data) => { stdout += data })
  const closed = once(child, "close")
  // Sending input without EOF holds the reader open until the process is cancelled.
  child.stdin.write('{"hook_event_name":')
  await once(child.stderr, "data")
  child.kill("SIGTERM")
  const [code] = await closed
  assert.equal(code, 0)
  assert.equal(stdout, "")
  assert.deepEqual(await f.calls(), [])
})

test("missing binary, timeout and output overflow remain model-silent", async (t) => {
  const f = await fixture(t)
  quiet(f.run(f.input(), { BCA: join(f.temp, "absent") }), "unavailable")
  assert.deepEqual(await f.calls(), [])
  for (const data of [{ hang: true }, { rows: "x".repeat(100_000) }]) {
    await f.file(data)
    quiet(f.run(f.input()), "unavailable")
  }
})

test("explicit analyzer wins over the checkout build, which wins over PATH", async (t) => {
  const f = await fixture(t)
  const bin = join(f.temp, "bin")
  await mkdir(bin)
  const pathBinary = join(bin, process.platform === "win32" ? "bca.exe" : "bca")
  await writeFile(pathBinary, 'console.log("PATH-only finding"); process.exitCode = 2;', { mode: 0o755 })
  await writeFile(f.preload, (await readFile(f.preload, "utf8")) + `
const previous = cp.spawn;
cp.spawn = (file, args, options) => file === "bca" ? original(process.execPath, [${JSON.stringify(pathBinary)}, ...args], options) : previous(file, args, options);
syncBuiltinESMExports();
`)
  const pathEnv = { BCA: "", PATH: `${bin}${process.platform === "win32" ? ";" : ":"}${f.env.PATH}` }
  assert.match(report(f.run(f.input(), pathEnv)), /PATH-only finding/)
  const local = join(f.root, "target/release", process.platform === "win32" ? "bca.exe" : "bca")
  await mkdir(join(f.root, "target/release"), { recursive: true })
  await writeFile(local, 'console.log("local finding"); process.exitCode = 3;', { mode: 0o755 })
  assert.ok(report(f.run(f.input())).includes(ROW))
  assert.match(report(f.run(f.input(), pathEnv)), /local finding/)
  quiet(f.run(f.input(), { BCA: join(f.temp, "not-installed") }), "unavailable")
})

test("retargeting an edited symlink discards the old target's findings", async (t) => {
  const f = await fixture(t)
  const old = join(f.root, "old.rs")
  const replacement = join(f.root, "new.rs")
  await writeFile(old, "{}"); await writeFile(replacement, "{}")
  await rm(f.path); await symlink(old, f.path)
  await writeFile(f.stub, `
import { unlinkSync, symlinkSync } from "node:fs";
unlinkSync(process.argv[3]); symlinkSync(${JSON.stringify(replacement)}, process.argv[3]);
console.log(${JSON.stringify(ROW)}); process.exitCode = 2;
`)
  quiet(f.run(f.input()), "superseded")
})

test("SIGTERM and SIGINT abort an active analyzer without later context", { skip: process.platform === "win32" }, async (t) => {
  const f = await fixture(t)
  await writeFile(f.stub, 'process.on("SIGTERM", () => {}); console.log("ready"); setInterval(() => {}, 1000);')
  await writeFile(f.preload, (await readFile(f.preload, "utf8")) + `
const previous = cp.spawn;
cp.spawn = (...args) => {
  const child = previous(...args);
  if (args[0] === ${JSON.stringify(f.stub)}) child.stdout.once("data", () => process.stderr.write("READY " + child.pid + "\\n"));
  return child;
};
syncBuiltinESMExports();
`)
  for (const signal of ["SIGTERM", "SIGINT"]) {
    const child = spawn(process.execPath, ["--import", pathToFileURL(f.preload).href, HOOK], { env: { ...f.env, BCA: f.stub }, cwd: f.temp })
    t.after(() => child.kill("SIGKILL"))
    let output = ""
    child.stdout.on("data", (data) => { output += data })
    const ready = once(child.stderr, "data")
    const closed = once(child, "close")
    child.stdin.end(JSON.stringify(f.input()))
    const [data] = await ready
    const pid = Number(data.toString().trim().split(" ")[1])
    assert.ok(Number.isInteger(pid) && pid > 0)
    child.kill(signal)
    const [code] = await closed
    assert.equal(code, 0)
    assert.equal(output, "")
    assert.throws(() => process.kill(pid, 0), { code: "ESRCH" })
  }
})

test("oversized hook input is rejected without analyzer execution", async (t) => {
  const f = await fixture(t)
  quiet(f.run(JSON.stringify(f.input({ tool_input: { file_path: f.path, content: "x".repeat(8 * 1024 * 1024) } }))), "unavailable")
  assert.deepEqual(await f.calls(), [])
})

test("compatibility shell wrapper preserves the Node adapter's JSON and zero exit", { skip: spawnSync("bash", ["--version"]).status !== 0 }, async (t) => {
  const f = await fixture(t)
  const expected = f.run(f.input())
  const result = spawnSync("bash", [join(REPO, ".claude/hooks/bca-check.sh")], {
    input: JSON.stringify(f.input()), encoding: "utf8",
    env: { ...f.env, BCA: f.stub, NODE_OPTIONS: `--import=${pathToFileURL(f.preload).href}` },
    cwd: f.temp, timeout: 10_000,
  })
  assert.equal(report(result), report(expected))
  assert.equal(result.stdout, expected.stdout)
})

test("a file modified or removed while analyzing never receives stale findings", async (t) => {
  const f = await fixture(t)
  for (const data of [{ replace: true }, { sameSize: true }, { remove: true }]) {
    await f.file(data)
    quiet(f.run(f.input()), "superseded")
  }
  await f.file({})
  assert.ok(report(f.run(f.input())).includes(ROW), "recreating the file must not inherit stale dedup state")
})

test("fallback guidance and report truncation preserve valid bounded JSON", async (t) => {
  const f = await fixture(t)
  await rm(join(f.root, ".claude/hooks/bca-guidance.txt"))
  await f.file({ rows: "x".repeat(8_000) })
  const text = report(f.run(f.input()))
  assert.ok(text.length <= 4_000)
  assert.match(text, /Report truncated/)
  assert.match(text, /Keep the task-boundary gate/)
})

const binary = process.env.BCA_TEST_BINARY || "bca"
const available = spawnSync(binary, ["--version"], { encoding: "utf8" }).status === 0
test("real CLI honors exclusions, alias identities, baselines and clean-to-regressed transitions", { skip: !process.env.BCA_TEST_BINARY && !available }, async (t) => {
  assert.ok(available, "explicit test analyzer must exist")
  const f = await fixture(t)
  const source = "fn f(x: bool) { if x { while x { if x {} } } }\n"
  await writeFile(f.path, source)
  await writeFile(join(f.root, "bca.toml"), '[thresholds]\ncognitive = 1\n[check]\nexclude = ["./excluded.rs"]\n')
  assert.match(report(f.run(f.input(), { BCA: binary })), /cognitive/)
  const excluded = join(f.root, "excluded.rs")
  await symlink(f.path, excluded)
  quiet(f.run(f.input({ tool_input: { file_path: excluded } }), { BCA: binary }), "clean-or-excluded")
  execFileSync(binary, ["check", "./sample.rs", "--write-baseline", ".baseline.toml"], { cwd: f.root, env: f.env, stdio: "ignore" })
  await writeFile(join(f.root, "bca.toml"), '[thresholds]\ncognitive = 1\n[check]\nbaseline = ".baseline.toml"\n')
  quiet(f.run(f.input(), { BCA: binary }), "clean-or-excluded")
  await writeFile(f.path, source.replace("if x {}", "if x { if x {} }"))
  assert.match(report(f.run(f.input(), { BCA: binary })), /\[regr[^\]]*\]/)
})

test("real CLI preserves nearer manifests, nested checkout policy and actionable diagnostics", { skip: !process.env.BCA_TEST_BINARY && !available }, async (t) => {
  const f = await fixture(t)
  const source = "fn f(x: bool) { if x { while x { if x {} } } }\n"
  const packageDir = join(f.root, "package")
  await mkdir(packageDir)
  await writeFile(join(f.root, "bca.toml"), '[thresholds]\ncognitive = 100\n')
  await writeFile(join(packageDir, "bca.toml"), '[thresholds]\ncognitive = 1\n')
  const path = join(packageDir, "sample.rs")
  await writeFile(path, source)
  assert.match(report(f.run(f.input({ cwd: packageDir, tool_input: { file_path: path } }), { BCA: binary })), /cognitive = 6 \(limit 1\)/)
  const nested = join(f.root, "child")
  f.git(["init", "-q", nested])
  await writeFile(join(nested, "bca.toml"), '[thresholds]\ncognitive = 100\n')
  await writeFile(join(nested, "sample.rs"), source)
  await writeFile(join(f.root, "bca.toml"), '[thresholds]\ncognitive = 1\n')
  quiet(f.run(f.input({ tool_input: { file_path: join(nested, "sample.rs") } }), { BCA: binary }), "clean-or-excluded")
  await writeFile(join(f.root, "bca.toml"), "not valid TOML")
  const broken = f.run(f.input(), { BCA: binary })
  quiet(broken, "unavailable")
  assert.match(JSON.parse(broken.stderr.slice("[bca-check] ".length)).diagnostic, /parse bca.toml/)
})

test("sibling-package checks keep the session policy and nested gitfiles require their own thresholds", {
  skip: !process.env.BCA_TEST_BINARY && !available,
}, async (t) => {
  const f = await fixture(t)
  const packageA = join(f.root, "package-a")
  const packageB = join(f.root, "package-b")
  await mkdir(packageA)
  await mkdir(packageB)
  await writeFile(join(packageA, "bca.toml"), '[thresholds]\ncognitive = 1\n')
  await writeFile(join(packageB, "bca.toml"), '[thresholds]\ncognitive = 100\n')
  const path = join(packageB, "sample.rs")
  const source = "fn f(x: bool) { if x { while x { if x {} } } }\n"
  await writeFile(path, source)
  const input = f.input({ cwd: packageA, tool_input: { file_path: path } })
  assert.match(report(f.run(input, { BCA: binary })), /cognitive = 6 \(limit 1\)/)
  quiet(f.run({ ...input, cwd: packageB }, { BCA: binary }), "clean-or-excluded")

  const nested = join(f.root, "submodule-shaped")
  f.git(["init", "-q", "--separate-git-dir", join(f.temp, "nested.git"), nested])
  await writeFile(join(f.root, "bca.toml"), '[thresholds]\ncognitive = 100\n')
  await writeFile(join(nested, "sample.rs"), source)
  const result = f.run(f.input({ tool_input: { file_path: join(nested, "sample.rs") } }), { BCA: binary })
  quiet(result, "unavailable")
  const health = JSON.parse(result.stderr.slice("[bca-check] ".length))
  assert.equal(health.cwd, nested)
  assert.match(health.diagnostic, /no thresholds configured/)
})
