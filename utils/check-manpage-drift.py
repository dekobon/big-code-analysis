#!/usr/bin/env python3
"""check-manpage-drift

Assert the committed ``man/`` tree matches what ``cargo xtask`` just
regenerated, in **all three** directions: modified, deleted, and added.

This is the assertion half of the man-page drift gate. Callers run
``cargo xtask`` first (it rewrites ``man/*.1`` in place and sweeps
orphan pages), then run this script:

* ``make manpages-check`` — the local / ``make pre-commit`` flavour.
* the ``manpage`` job in ``.github/workflows/ci.yml``.

Both sites call this one script rather than each carrying their own
copy of the check, so the two cannot drift apart (the previous
arrangement was two hand-mirrored shell blocks kept in step by a
comment).

Why not ``git diff --exit-code -- man/`` alone (#1249)
------------------------------------------------------

``git diff`` compares tracked content only, so the page ``cargo xtask``
writes for a brand-new subcommand — untracked, never committed — is
invisible to it. The gate went green and the page silently never
shipped. Measured verdicts for every state that reaches this gate:

===========================  =========  ==================  ===========
state                        git diff   ls-files --others   porcelain
===========================  =========  ==================  ===========
clean                        pass       pass                pass
new page, untracked          **pass**   fail                fail
new page, staged             pass       pass                **fail**
tracked page modified        fail       pass                fail
tracked page deleted         fail       pass                fail
page removed from index      **pass**   fail                fail
gitignored new page          **pass**   see below            **pass**
===========================  =========  ==================  ===========

The last row is why this script does *not* pass ``--exclude-standard``;
see "Two scoping decisions" below. ``git status --porcelain`` cannot be
made to cover it at all.

``git diff`` plus ``git ls-files --others`` covers every row. Plain
``git status --porcelain`` also covers every row, but it fails the
"new page, staged" one *by design*, and that state is the remedy this
gate's own error message prescribes (``git add man/``, then commit):
the ``pre-commit`` framework runs its ``manpages`` hook against the
staged tree, so a porcelain check would reject every legitimate
man-page commit. ``git add -N`` is likewise rejected — it mutates the
contributor's index behind their back.

Note the "page removed from index" row: ``git rm --cached`` leaves the
file on disk and untracked, so ``ls-files --others`` reports it and
``git diff`` does not. That is also the cheapest faithful reproduction
of the added-page state for manual probing, because a fabricated
``man/zz-probe.1`` does *not* work — ``xtask``'s ``sweep_orphans``
deletes any unexpected ``.1`` before this script ever runs.

Two scoping decisions worth stating, because both are load-bearing:

* The untracked half runs **without** ``--exclude-standard``, and is
  scoped to ``man/*.1`` to afford that. An ignore rule that covered
  a page would otherwise reinstate #1249 exactly — measured: a global
  ``~/.config/git/ignore`` containing ``*.1`` makes
  ``ls-files --others --exclude-standard`` return nothing over an
  uncommitted page, and neither ``GIT_CONFIG_GLOBAL`` nor the repo's
  own ``.gitignore`` has any say in that. ``xtask`` writes nothing but
  ``.1`` files and ``sweep_orphans`` removes nothing else, so
  ``man/*.1`` is exactly the generated surface; narrowing to it means
  dropping the ignore rules cannot turn an editor dropping
  (``.swp``, ``~``, ``.DS_Store``) into a gate failure.
* The tracked half keeps the old ``man/`` scope, so a tracked non-page
  file living there is still covered. Anything tracked is there
  deliberately.

``--exit-code`` is likewise not decoration: a configured
``diff.external`` / ``GIT_EXTERNAL_DIFF`` can print nothing at all, so
treating empty stdout as "clean" reports a modified page as clean —
measured with ``GIT_EXTERNAL_DIFF=/bin/true``. The verdict is git's
exit status. ``--no-ext-diff`` then keeps the *printed* evidence git's
own, so a real failure still shows which lines moved.

See AGENTS.md "Validation gates" for the policy this enforces.
"""

from __future__ import annotations

import os
import pathlib
import subprocess
import sys

# `parents[1]`, not `parent`: these gates live in `utils/` but every
# path they read is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

# Everything tracked under `man/` is covered by the modified/deleted
# half; only the generated pages are covered by the added half. See the
# module docstring for why the two scopes differ.
MAN_PATHSPEC = "man/"
GENERATED_PAGE_PATHSPEC = "man/*.1"


class GitError(Exception):
    """A git invocation failed; carries the command and its stderr."""


def _git(
    args: list[str], root: pathlib.Path, allowed: tuple[int, ...] = (0,)
) -> subprocess.CompletedProcess[str]:
    """Run ``git <args>`` in *root* and return the completed process.

    Raises :class:`GitError` for any status outside *allowed*, rather
    than letting a failure read as an empty (== clean) result — a gate
    that reports "no drift" because git could not run is the failure
    mode this whole script exists to prevent.
    """
    proc = subprocess.run(
        ["git", *args],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode not in allowed:
        raise GitError(f"git {' '.join(args)}: {proc.stderr.strip()}")
    return proc


def tracked_drift(root: pathlib.Path) -> str:
    """Return the worktree-vs-index diff for ``man/`` ("" when clean).

    Covers the modified and deleted directions. Deliberately compares
    against the *index*, not ``HEAD``: staged-but-uncommitted pages are
    the state the gate's remedy produces and must read as clean.

    The verdict is ``--exit-code``'s status, not stdout emptiness: with
    ``diff.external`` configured, a diff driver that prints nothing
    would otherwise read as a clean tree. ``--no-ext-diff`` is the
    second half of that — it keeps the returned text git's own, so the
    failure report still shows which lines moved.
    """
    proc = _git(
        ["diff", "--no-ext-diff", "--exit-code", "--", MAN_PATHSPEC],
        root,
        allowed=(0, 1),
    )
    # `--exit-code` reports 1 for "differences found"; anything else is
    # a real git failure and already raised above. The fallback keeps
    # "differences found" from collapsing back into a clean verdict if
    # git ever reports 1 while printing nothing.
    if proc.returncode != 1:
        return ""
    return proc.stdout or "(git reports differences under man/)\n"


def untracked_pages(root: pathlib.Path) -> list[str]:
    """Return untracked ``man/*.1`` paths, ignore rules notwithstanding.

    Covers the added direction, which ``git diff`` cannot see (#1249).
    """
    out = _git(
        ["ls-files", "--others", "--", GENERATED_PAGE_PATHSPEC], root
    ).stdout
    return [line for line in out.splitlines() if line]


def _annotate(summary: str) -> None:
    """Emit a GitHub Actions ``::error::`` annotation when on a runner.

    Preserves the annotation the CI step used to spell inline, so a
    drift failure still surfaces in the workflow summary rather than
    only in the raw log. Annotations are read from stdout.
    """
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::error::{summary}")


def main() -> int:
    try:
        diff = tracked_drift(REPO_ROOT)
        untracked = untracked_pages(REPO_ROOT)
    except GitError as exc:
        sys.stderr.write(f"error: {exc}\n")
        return 2

    if not diff and not untracked:
        print("manpage-drift: OK (man/ matches the clap schema)")
        return 0

    if diff:
        # Same output the old `git diff --exit-code` printed, so a
        # reviewer can still see exactly which lines moved.
        sys.stdout.write(diff)
        sys.stderr.write(
            "error: committed man pages drift from the clap schema\n"
            "       The regenerated files are already in your working "
            "tree — run 'git add man/'\n"
            "       and commit them alongside the clap change.\n"
        )
    if untracked:
        sys.stderr.write(
            "error: newly generated man page(s) are not committed:\n"
        )
        for path in untracked:
            sys.stderr.write(f"  {path}\n")
        sys.stderr.write(
            "       'cargo xtask' writes one page per (sub)command, so a "
            "new subcommand's\n"
            "       page starts out untracked and 'git diff' cannot see "
            "it. Run 'git add man/'\n"
            "       and commit it alongside the clap change (or delete "
            "the file if it is\n"
            "       not a generated page).\n"
        )

    _annotate(
        "Committed man pages drift from the clap schema. Run 'cargo xtask' "
        "locally and commit the result (including any newly added page)."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
