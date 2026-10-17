# History Rewrite Rule

`git rebase -i` resolves each todo line's hash as any commit-ish. A
scripted todo edit that names a *superseded* hash — the pre-amend or
pre-fixup copy of a commit, which still exists in the object store —
replays that old commit in place of the current one. Nothing errors: the
rebase completes, later commits apply on top, and the tree silently
loses whatever the old copy lacked. On PR #1609 a reword script naming
the pre-fixup hash of a commit dropped a fix, a fixture and a scaling
probe, and `make pre-commit` then passed on the stale tree.

## How to apply

- Before a scripted rewrite, keep the tip: `git branch
  backup/<name>-before-<step> HEAD`.
- Build todo edits from hashes in the *current* `git log`, never from a
  hash noted earlier in the session.
- After the rewrite, compare trees, not test results. A message-only
  rewrite must leave `git diff --stat <backup> HEAD` empty. For a content
  rewrite, list `git log --format='%T %s'` for both ranges side by side;
  every tree should match its old counterpart except where you meant it
  to change.
- A gate that passes after a rewrite proves the tree passes, not that it
  is the tree you meant.
