# CLAUDE.md

## Shared Project Instructions

@AGENTS.md

## Claude Code-Specific Configuration

### Worktree safety (ABSOLUTE PRIORITY)

If you are running inside a worktree (check:
`git rev-parse --show-toplevel` returns a path under
`.claude/worktrees/`), the following are hard bans — violating them
destroys other agents' in-progress work:

- Never run `git worktree remove`, `git worktree prune`, or `rm -rf`
  on any worktree directory.
- Never `cd` to the main repository, check out `main` (this repo's
  default branch), or write files outside your worktree.
- Never use `/clean_gone` or any command that removes worktrees.
- The only entity that may remove a worktree is the Claude Code
  runtime that created it (automatic cleanup on session end).
- If you see stale worktrees, leave them alone — another agent may
  be using them, or the user will clean them up manually.

**Run `make worktree-setup` once per worktree before `make
pre-commit`.** A fresh worktree inherits neither the integration
corpora nor a Python venv, and neither absence names itself:

- Without the corpora under `tests/repositories/`, 24 tests fail —
  the 5 corpus tests, plus 19 CLI tests that analyse a real source
  file from the `DeepSpeech` corpus. Since #1171 each names the cause
  and the remedy; before it they read as bugs in whatever you were
  changing.
- Without `big-code-analysis-py/.venv`, `py-typecheck` reports ~33
  mypy errors (`pytest` untyped without its stubs) and `py-test` dies
  with "Couldn't find a virtualenv".

Both are bootstrap artifacts, not regressions. The cargo stages
themselves work in a worktree — that was #1145, fixed by the
`[workspace]` tables on the six excluded crates.

`make worktree-setup` is idempotent and a ~100 ms no-op once the tree
is set up, so re-running it is the cheapest way to rule the
environment out. Run it again in particular if a corpus checkout was
interrupted: that leaves the submodule with its files deleted but its
HEAD already at the recorded SHA, so **a plain `git submodule update
--init` is a silent no-op** and only `--force` repairs it.
`worktree-setup` detects that state and escalates on its own; by hand
it is `git submodule update --init --force -- <path>`. It refuses to
force a submodule that also has local modifications — accepted `.snap`
files in `big-code-analysis-output` are exactly what that protects —
and prints the command for you to run once they are safe.

### Tool choice

- **Text search**: built-in `Grep`, or `rg` via Bash. Never `grep`.
- **File search**: built-in `Glob`, or `fd` (or `fdfind` on
  Debian/Ubuntu) via Bash. Never `find`.
- **Code intelligence**: when an LSP-based tool such as Serena is
  available, use it as the default for read / search / edit /
  refactor.
- **External docs**: prefer Context7 / `cargo doc` over web search
  for library / crate documentation.

### Editing

- For code files: prefer Serena symbol-level editing
  (`replace_symbol_body`, `insert_before/after_symbol`) over
  line-based `Edit` tool calls when available.
- For non-code files: use targeted `Edit` tool calls with scoped
  `old_string` / `new_string` pairs.

### Communication during a task

- Say in one sentence what you are about to do before the first tool
  call. After that, speak up when you find something that changes the
  plan or the conclusion — not once per file read.
- Lead the closing message with the outcome: what happened, what
  broke, or what you found. Supporting detail goes underneath it, for
  the reader who wants it.
- Keep answers short and caveats shorter. When asked to explain a
  design decision or a metric, give the high-level answer and expand
  only if asked.

### Length of written deliverables

Documents produced here — audit and triage reports, GitHub issue
bodies, changelog entries, lessons-learned drafts, book pages — should
be as long as their substance requires and no longer. No filler
sections, no summary that restates the section above it, no
boilerplate heading carrying nothing. A skill's report template is a
maximum shape, not a quota: drop a section with no findings rather
than padding it. `doc-author` and
[`docs/conventions/documentation.md`](docs/conventions/documentation.md)
own the prose rules; this length rule applies to every other written
artifact too.

### Delegating to subagents

Skills that fan work out set their own agent counts — follow them.
Outside those, the default is to do the work yourself:

- Delegate only for work that is genuinely independent and larger than
  a handful of tool calls: a wide multi-file investigation, one fix
  per crate. Reading three files and editing one is not worth an
  agent.
- One agent that can carry the whole task beats three that split it.
- Do not spawn an agent to check work you just did. The independent
  reviewers in `simplify-rust`, `review`, and `doc-author` exist
  because a fresh context catches what an authoring context cannot;
  that is a different thing from re-verifying your own output, which
  costs tokens and finds nothing.
- Keep concurrent `isolation: "worktree"` agents to six or fewer.
  Each clones the workspace, and this workspace has twelve crate
  manifests, so unbounded fan-out has filled the disk with scratch
  clones. Queue the remainder and start each as a slot frees. The
  fan-out skills encode this same six: `batch-fix` caps wave size,
  `improve-crate` and `cleanup-crate` cap concurrent area agents.
- Match the model to the stage. The Agent tool takes `model`, so a
  mechanical pass (collecting a diff, running a lint, filling in a
  template) can run on a smaller one while analysis keeps the default.
  There is no reasoning-effort knob on the Agent tool — only
  `Workflow`'s `agent()` accepts one, and no skill here uses
  `Workflow`.

### Skills available under `.claude/skills/`

| Skill | Use when… |
|-------|-----------|
| `add-lang` | Adding a new tree-sitter language end-to-end (grammar crate, enum, Checker/Getter/Alterator, metrics, tests, docs) |
| `batch-fix` | Fixing several GitHub issues at once on an integration branch (parallel worktrees per crate) |
| `simplify-rust` | Reviewing a diff for reuse / clarity / efficiency, applying fixes inline |
| `rust-optimize` | Reducing verbosity / modernizing Rust syntax with pedantic-clippy triage |
| `review` | Read-only review of a diff, branch, PR, or commit range |
| `audit-tests` | Finding tests that pass for the wrong reason |
| `audit-crate` | Read-only crate-level audit that files GitHub issues for findings |
| `audit-file` | Read-only single-file audit that files GitHub issues for findings |
| `audit-naming` | Read-only crate-level audit of naming quality |
| `scan-project` | Scanning the core library + CLI + web crates for logic errors, security issues, and metrics calculation bugs (6 parallel agents, 50-question checklist) |
| `cleanup-crate` | Removing dead code, unused imports, and unreachable paths from one crate |
| `improve-crate` | Safe code-improvement workflow for one crate (clarity / reuse / efficiency) |
| `issue-plan` | Reading an issue, building a sequential-thinking plan, rating it, applying `low-priority` |
| `issue-triage` | Producing a read-only triage report (quick wins + groupings) over open issues |
| `fix-issue` | End-to-end workflow for fixing a GitHub issue |
| `lessons-learned` | Drafting entries for `docs/development/lessons_learned.md` |
| `doc-author` | Writing / revising / de-slopping a prose doc against `docs/conventions/documentation.md` |

The `audit-crate`, `audit-file`, `audit-naming`, `scan-project`, `issue-triage`, and
`review` skills are read-only and must not modify the working tree; all
other skills may edit code as part of their workflow.

### Hooks

`.claude/hooks/bca-check.sh` (the per-edit `bca check` feedback loop) is
opt-in and **not registered by default** — to enable it, copy the
`PostToolUse` registration snippet from
[`recipes/agent-feedback.md`](big-code-analysis-book/src/recipes/agent-feedback.md)
into your `.claude/settings.local.json`. It never blocks an edit and
no-ops silently when no `bca` binary is available.
