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
