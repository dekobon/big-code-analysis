#!/usr/bin/env python3
"""check-ruff-lockstep

One ruff version is adopted by this repository, and it is declared in
four places that nothing previously compared.

``big-code-analysis-py/uv.lock`` is the anchor. It is what ``uv sync
--locked`` resolves, and every other declaration is either derived from
it (``requirements/dev.txt`` is a ``uv export`` of it) or is a
hand-maintained copy that must follow it.

Three invariants are checked.

**The ``ruff-pre-commit`` ``rev:`` must equal ``v`` + the locked
version.** Before #1222 that ``rev:`` was ``v0.15.14`` while ``uv.lock``
resolved ``0.15.22``, so ``pre-commit run --all-files`` ran a different
ruff than CI installs. Nothing failed, because neither version happened
to disagree on this tree — which is the failure mode worth gating: it is
silent until they do disagree, and then it presents as "works locally,
red in CI" (#1230). The comment above the ``rev:`` asking for a lockstep
bump was the entire previous mechanism.

**The ``requirements/dev.txt`` ``ruff==`` pin must equal the locked
version.** CI installs its ruff with ``pip install --require-hashes -r
requirements/dev.txt`` rather than through uv, so a stale export is a
ruff CI runs that the lockfile never resolved. AGENTS.md requires a
``uv.lock`` change and its regenerated exports to land in the same
commit; this is that rule as a check.

**``pyproject.toml``'s dev-extra ruff bound must equal the one recorded
in ``uv.lock``.** uv writes the requirement it resolved against into
``[package.metadata] requires-dist``, so comparing the two catches a
bound edited without a ``make py-relock``. That case would otherwise
leave CI installing, from the export, a version the manifest no longer
admits. Bounds are compared as *sets of whitespace-stripped clauses*, so
``>= 0.13, <0.17`` and ``>=0.13,<0.17`` agree and clause order does not
matter — uv normalises both, and a false failure here would be
indistinguishable from the drift the gate exists to report.

``uv.lock`` and ``pyproject.toml`` are parsed with ``tomllib``.
``.pre-commit-config.yaml`` is parsed by line, anchored to the
``ruff-pre-commit`` repo block, because PyYAML is not in the standard
library: this gate runs under a contributor's bare ``python3`` and as a
``language: system`` pre-commit hook, so a third-party import would make
it crash or be skipped exactly where it is needed. Every parse helper
hard-errors on a shape it does not recognise rather than returning a
default — a version gate that silently passes on malformed input is the
bug being fixed, not a lenient version of the fix.

See `#1230` and AGENTS.md "Validation gates".
"""

from __future__ import annotations

import pathlib
import re
import sys
from typing import Any

# tomllib landed in 3.11. On older Python, fall back to the external
# `tomli` package (same API), matching check-excluded-manifests.py.
try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib  # type: ignore[import-not-found,no-redef]
    except ImportError:
        sys.stderr.write(
            "error: check-ruff-lockstep.py requires Python 3.11+\n"
            "       (tomllib lives in the standard library starting at 3.11).\n"
            "       On older Python, install `tomli` and retry:\n"
            "           pip install tomli\n"
        )
        sys.exit(2)

# `parents[1]`, not `parent`: this gate lives in `utils/` but every path
# it reads is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

PRE_COMMIT_CONFIG = pathlib.Path(".pre-commit-config.yaml")
UV_LOCK = pathlib.Path("big-code-analysis-py/uv.lock")
REQUIREMENTS_DEV = pathlib.Path("big-code-analysis-py/requirements/dev.txt")
PYPROJECT = pathlib.Path("big-code-analysis-py/pyproject.toml")

# The distribution whose version is under lockstep, and the extra it is
# declared in.
RUFF = "ruff"
DEV_EXTRA = "dev"

# The `repo:` URL identifying the ruff hook block in
# `.pre-commit-config.yaml`. Matching the URL rather than the position
# is what keeps a reordered config from handing us a neighbouring
# repo's `rev:`.
RUFF_PRE_COMMIT_REPO = "https://github.com/astral-sh/ruff-pre-commit"

# A `- repo: <url>` line, in either the `-   repo:` or `- repo:`
# spelling this config mixes. The capture is the bare URL; any trailing
# comment is stripped by `_scalar`.
_REPO_LINE_RE = re.compile(r"^\s*-\s+repo:\s*(\S.*)$")
# A `rev:` key. Anchored to the start-of-line whitespace so a `rev:`
# appearing inside a hook's `args:` list cannot match.
_REV_LINE_RE = re.compile(r"^\s*rev:\s*(\S.*)$")
# `ruff==0.16.2 ; markers \` in a `uv export` requirements file. The
# version runs to the first whitespace or `;`.
_REQUIREMENT_PIN_RE = re.compile(rf"^{RUFF}==([^\s;]+)", re.MULTILINE)
# Splits `ruff>=0.13,<0.17` into name and specifier. PEP 508 allows an
# extras bracket and a marker; neither is used for ruff here, and both
# would fail the name comparison loudly rather than silently.
_REQUIREMENT_RE = re.compile(r"^\s*([A-Za-z0-9._-]+)\s*(.*)$")


def _scalar(raw: str) -> str:
    """Reduce a YAML scalar to its value: no comment, no quotes.

    Only the shapes a `rev:` can legally take are handled — a bare
    token, a quoted token, and either followed by a `#` comment. A
    ``rev`` value never contains whitespace or ``#``, so taking the
    first whitespace-delimited token is sufficient and cannot swallow
    one.
    """
    words = raw.split("#", 1)[0].split()
    return words[0].strip("\"'") if words else ""


def read_pre_commit_rev(text: str) -> str:
    """Return the ``rev:`` of the ``ruff-pre-commit`` repo block.

    The block runs from its ``- repo:`` line to the next one, so a
    ``rev:`` belonging to a neighbouring repo is never returned however
    the file is ordered. Anything other than exactly one ``rev:`` across
    all such blocks is a hard error: no block, no ``rev:``, a duplicated
    block, or two ``rev:`` keys in one. Picking the first of several
    would let a stale duplicate govern what pre-commit actually runs.
    """
    revs: list[str] = []
    in_ruff_block = False
    for line in text.splitlines():
        repo_match = _REPO_LINE_RE.match(line)
        if repo_match:
            in_ruff_block = _scalar(repo_match.group(1)) == RUFF_PRE_COMMIT_REPO
            continue
        rev_match = _REV_LINE_RE.match(line)
        if in_ruff_block and rev_match:
            revs.append(_scalar(rev_match.group(1)))
    if len(revs) != 1:
        raise SystemExit(
            f"error: expected exactly one `rev:` under a {RUFF_PRE_COMMIT_REPO}\n"
            f"       repo block in {PRE_COMMIT_CONFIG}, found {len(revs)}."
        )
    return revs[0]


def _packages(lock_text: str) -> list[dict[str, Any]]:
    """Return ``uv.lock``'s ``[[package]]`` entries."""
    try:
        data = tomllib.loads(lock_text)
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(f"error: {UV_LOCK} is not valid TOML: {exc}") from exc
    packages = data.get("package")
    if not isinstance(packages, list):
        raise SystemExit(f"error: {UV_LOCK} has no [[package]] entries.")
    return [entry for entry in packages if isinstance(entry, dict)]


def read_lock_version(lock_text: str) -> str:
    """Return the version ``uv.lock`` resolves for ruff."""
    versions = [
        entry["version"]
        for entry in _packages(lock_text)
        if entry.get("name") == RUFF and isinstance(entry.get("version"), str)
    ]
    if len(versions) != 1:
        raise SystemExit(
            f"error: expected exactly one versioned `{RUFF}` package in "
            f"{UV_LOCK},\n       found {len(versions)}."
        )
    return versions[0]


def read_lock_specifier(lock_text: str) -> str:
    """Return the ruff dev-extra bound ``uv.lock`` recorded resolving.

    uv writes each project requirement into ``[package.metadata]
    requires-dist``, so this is the ``pyproject.toml`` bound as it stood
    at the last ``uv lock``.
    """
    specifiers = [
        requirement["specifier"]
        for entry in _packages(lock_text)
        for requirement in _requires_dist(entry)
        if requirement.get("name") == RUFF
        and _names_extra(requirement.get("marker"), DEV_EXTRA)
        and isinstance(requirement.get("specifier"), str)
    ]
    if len(specifiers) != 1:
        raise SystemExit(
            f"error: expected exactly one `{RUFF}` `{DEV_EXTRA}`-extra entry in "
            f"{UV_LOCK}'s\n       requires-dist, found {len(specifiers)}."
        )
    return specifiers[0]


def _requires_dist(package: dict[str, Any]) -> list[dict[str, Any]]:
    """Return a ``[[package]]`` entry's ``metadata.requires-dist`` rows."""
    metadata = package.get("metadata")
    if not isinstance(metadata, dict):
        return []
    requirements = metadata.get("requires-dist")
    if not isinstance(requirements, list):
        return []
    return [row for row in requirements if isinstance(row, dict)]


def _names_extra(marker: Any, extra: str) -> bool:
    """True when a uv ``requires-dist`` marker selects ``extra``.

    uv writes the marker as ``extra == 'dev'``; quoting is compared
    loosely so a switch to double quotes does not read as "the dev extra
    disappeared".
    """
    if not isinstance(marker, str):
        return False
    pattern = rf"""extra\s*==\s*['"]{re.escape(extra)}['"]"""
    return re.search(pattern, marker) is not None


def read_requirements_version(requirements_text: str) -> str:
    """Return the ``ruff==`` version pinned in a ``uv export`` file."""
    versions = _REQUIREMENT_PIN_RE.findall(requirements_text)
    if len(versions) != 1:
        raise SystemExit(
            f"error: expected exactly one `{RUFF}==` line in {REQUIREMENTS_DEV},\n"
            f"       found {len(versions)}."
        )
    return versions[0]


def read_pyproject_specifier(pyproject_text: str) -> str:
    """Return the ruff bound declared in the ``dev`` optional extra."""
    try:
        data = tomllib.loads(pyproject_text)
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(f"error: {PYPROJECT} is not valid TOML: {exc}") from exc
    project = data.get("project")
    extras = project.get("optional-dependencies") if isinstance(project, dict) else None
    entries = extras.get(DEV_EXTRA) if isinstance(extras, dict) else None
    if not isinstance(entries, list):
        raise SystemExit(
            f"error: {PYPROJECT} has no "
            f"[project.optional-dependencies] {DEV_EXTRA} array."
        )
    specifiers = []
    for entry in entries:
        if not isinstance(entry, str):
            continue
        match = _REQUIREMENT_RE.match(entry)
        if match and match.group(1).lower().replace("_", "-") == RUFF:
            specifiers.append(match.group(2).strip())
    if len(specifiers) != 1:
        raise SystemExit(
            f"error: expected exactly one `{RUFF}` requirement in {PYPROJECT}'s\n"
            f"       {DEV_EXTRA} extra, found {len(specifiers)}."
        )
    return specifiers[0]


def clause_set(specifier: str) -> frozenset[str]:
    """Normalise a version bound for comparison.

    A PEP 508 specifier is a comma-separated, unordered set of clauses
    whose internal whitespace is insignificant, so compare it as one.
    Otherwise a purely cosmetic difference between what a human wrote in
    `pyproject.toml` and what uv wrote back into `uv.lock` would report
    as drift.
    """
    return frozenset(
        clause for clause in "".join(specifier.split()).split(",") if clause
    )


def read_repo_file(relative: pathlib.Path) -> str:
    """Read a repository file, hard-erroring when it is absent."""
    path = REPO_ROOT / relative
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise SystemExit(f"error: cannot read {relative}: {exc}") from exc


def main() -> int:
    # Read and parse the lockfile once: it answers both the resolved
    # version and the bound uv resolved against.
    lock_text = read_repo_file(UV_LOCK)
    locked = read_lock_version(lock_text)
    expected_rev = f"v{locked}"

    problems: list[str] = []

    rev = read_pre_commit_rev(read_repo_file(PRE_COMMIT_CONFIG))
    if rev != expected_rev:
        problems.append(
            f"  {PRE_COMMIT_CONFIG}: rev: {rev} (expected {expected_rev})\n"
            f"    `pre-commit run --all-files` would run ruff {rev.lstrip('v')} "
            f"while CI installs {locked}."
        )

    exported = read_requirements_version(read_repo_file(REQUIREMENTS_DEV))
    if exported != locked:
        problems.append(
            f"  {REQUIREMENTS_DEV}: {RUFF}=={exported} (expected {locked})\n"
            f"    The export is stale; re-run `make py-relock`."
        )

    declared = read_pyproject_specifier(read_repo_file(PYPROJECT))
    recorded = read_lock_specifier(lock_text)
    if clause_set(declared) != clause_set(recorded):
        problems.append(
            f"  {PYPROJECT}: {RUFF}{declared} but {UV_LOCK} resolved against "
            f"{recorded}\n    The lockfile predates the bound; re-run "
            f"`make py-relock`."
        )

    if problems:
        print(
            "error: ruff version declarations disagree:\n"
            + "\n".join(problems)
            + f"\n\n{UV_LOCK} is the anchor ({RUFF} {locked}). Bump the\n"
            f"`ruff-pre-commit` rev: to {expected_rev} and/or re-run\n"
            "`make py-relock`, in the same commit. See #1230.",
            file=sys.stderr,
        )
        return 1

    print(f"ruff lockstep OK ({RUFF} {locked}, bound {declared}, 3 sites checked).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
