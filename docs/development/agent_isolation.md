# Isolating agent work in worktrees

How a skill that fans work out to child agents keeps each one in its own
checkout, verifies it got there, and cleans up. It is for the
orchestrating skills under `.claude/skills/` (`batch-fix`,
`improve-crate`, `cleanup-crate`, the audits) and for anyone running
them from OpenCode or Claude Code. The shared rules in
[`AGENTS.md`](../../AGENTS.md) ("Worktrees") apply to every agent; this
document is the protocol those skills follow to honour them.

## Three modes

- **Worktree mode**: each mutating child gets its own
  [git worktree](https://git-scm.com/docs/git-worktree) and branch, so
  children can run in parallel. Choose it when the runtime can start a
  child in a directory the orchestrator names or allocates.
- **Branch mode**: mutating children share the orchestrator's working
  directory and therefore run strictly one at a time, each on a
  feature branch. Choose it when a child cannot be started elsewhere.
- **Shared read-only mode**: children that write nothing (the audit and
  scan skills) may run in parallel in one checkout. No child switches
  branches, cleans or resets while a reader is running, and the
  orchestrator verifies `git status --porcelain` is empty before and
  after the fan-out.

The orchestrator decides the mode and tells each child which one it is
in. A child never infers it from its path.

## Worktree mode protocol

1. **Allocate.** Either the orchestrator creates the worktree and
   branch at the revision the child should start from
   (`git worktree add -b <branch> <path> <base>`, or OpenCode's
   worktree facility) and records the absolute path, branch and base
   commit; or the runtime allocates one at launch (in Claude Code,
   `isolation: worktree` on the Agent call, or a subagent definition
   carrying it). Never both: a preallocated path handed to a launch
   that allocates its own produces two worktrees and a child that
   cannot match its prompt.
2. **Hand over.** With orchestrator allocation, put the absolute path,
   the branch and the base commit in the child's prompt and start the
   child there (move its session into the worktree, or require absolute
   paths on every file tool). With runtime allocation, the prompt names
   the base commit and the branch pattern only; the child reports its
   `git rev-parse --show-toplevel`, `--abbrev-ref HEAD` and
   `rev-parse HEAD` before writing anything, and proceeds only if the
   orchestrator's instructions already cover what it reports (the base
   commit matches, the branch is not `main` / `master` / detached, the
   root is not the orchestrator's directory).
3. **Verify before writing.** Whichever allocation was used, the child
   aborts without editing anything unless its root and branch are the
   ones it was told or was told to expect. `main`, `master` and a
   detached `HEAD` are always disallowed.
4. **Stay put.** The child does not switch branches, `cd` to another
   checkout, or write outside its worktree. Every file-tool call takes
   an absolute path under the verified root: a relative path resolves
   against the session's directory, which may be the orchestrator's.
5. **Bootstrap once.** A fresh worktree has neither the integration
   corpora nor the Python venv; `make worktree-setup` is idempotent and
   runs before the first `make pre-commit`.
6. **See earlier work.** A worktree created from the base commit cannot
   see what the orchestrator has merged since. When later waves depend
   on earlier ones, the child merges the integration branch first
   (`git merge <integration-branch> --no-edit`); git permits merging
   from a branch another worktree has checked out, only not checking it
   out.
7. **Clean up by the owner, run by the user.** A child never removes or
   prunes worktrees. The orchestrator that created a worktree with
   `git worktree add` removes it after merging or discarding its branch;
   `git worktree remove` is denied to agents by this repository's
   permission rules (a deny, not an ask, so no per-run approval exists),
   so the orchestrator lists the worktrees it created, with path, branch
   and merged / unmerged / dirty state, in its final summary and the
   user removes them. A runtime-allocated worktree is the runtime's to
   remove; report any it leaves behind. A worktree the
   orchestrator did not create is left alone.

Keep at most six worktree children in flight: each is a full checkout
with a dozen crate manifests, and unbounded fan-out has filled the disk
before.

## Branch mode protocol

1. The orchestrator verifies a clean tree (`git status --porcelain` is
   empty) before starting each child, and owns cleanup between children.
2. It creates the child's feature branch from the integration branch's
   current tip and names it in the prompt.
3. The child verifies its branch exactly as in worktree mode, and never
   runs `git checkout`, `git switch`, `git clean` or `git reset` itself.
4. Mutating children run one at a time. Launching two against one
   directory corrupts both.

## Merging a child's branch

Merge only after every child in a wave has returned and only into a
clean integration checkout. `git checkout <integration-branch>` refuses
while another worktree holds that branch, which means a child is still
running: wait, do not repair. A successful `git merge` is not evidence
the tree was idle; it refuses only when it would overwrite an
uncommitted change to a file it touches.
