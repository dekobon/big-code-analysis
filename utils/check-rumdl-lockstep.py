#!/usr/bin/env python3
"""check-rumdl-lockstep

One rumdl release lints this repository's Markdown, and it is pinned in
three files that only comments kept in step:

* ``mise.toml`` — ``[tools] rumdl = "X.Y.Z"``, what ``mise install``
  gives a contributor;
* ``.github/workflows/ci.yml`` — the ``lint`` job's ``RUMDL_VERSION`` /
  ``RUMDL_SHA256`` step env, what CI downloads and verifies;
* ``Dockerfile`` — ``ENV RUMDL_VERSION`` plus the per-arch SHA256
  ``case`` in the rumdl ``RUN`` block, what the dev container installs.

#1653 shows the cost. Its work list named ``mise.toml`` and ``ci.yml``
and left out the ``Dockerfile``, which the bump caught only by grepping.
Had it been missed, the container would lint on a different rumdl than
CI, and the only symptom would be "clean in the container, red in CI"
on the first document that tripped a rule the two versions disagree on.

Two invariants are checked.

**The three versions are equal.** There is no natural anchor — no file
is derived from another — so a disagreement names the file that differs
from the other two, or all three when no two agree. Every value is
printed either way, so the reader sees the whole table rather than a
verdict about which side is "right".

**The Dockerfile's x86_64 SHA256 equals ``ci.yml``'s ``RUMDL_SHA256``.**
Both verify ``rumdl-v<version>-x86_64-unknown-linux-gnu.tar.gz``, so
they must match byte for byte. This catches a version bump that pasted
only one hash, offline. The arm64 hash has no CI counterpart to compare
against; the image build's ``sha256sum --check`` already covers it.

**A version bump moves the hash too.** The two checks above pass a bump
that changed all three versions and left both hashes alone, because the
stale hash still agrees with itself. ``.rumdl-lockstep-baseline.txt``
records the version and x86_64 SHA256 the gate last accepted, and the
gate fails by name when the version has moved but the hash has not, or
the hash has moved but the version has not (a release's tarball does not
change). A bump that moves both fails too, until ``--update`` records
the new pair in the same commit: a record left at the old release would
check the *next* bump against the wrong hash, and a passing run's note
is read by no one. ``--update`` refuses a pair that moved only one of
the two, and refuses when nothing moved.

**Why rumdl alone, not every pin the Dockerfile header mirrors from
``ci.yml``.** rumdl is the only tool declared in all three files. taplo,
shfmt and shellcheck reach CI through ``taiki-e/install-action``, which
carries no SHA256 to compare. checkmake's arm64 container path installs
a different fork at a different version from CI's, so "the container
matches CI" is false for it by design. actionlint does have the same
``ci.yml`` / ``Dockerfile`` version-and-hash pair; it is two files, not
three, and is left for a follow-up rather than generalising this gate
on spec.

``mise.toml`` is parsed with ``tomllib``. ``ci.yml`` and the
``Dockerfile`` are parsed by line, because PyYAML is not in the
standard library and nothing parses a Dockerfile: this gate runs under a
contributor's bare ``python3`` and as a ``language: system`` pre-commit
hook. Each read is anchored to a shape that occurs exactly once and
hard-errors on anything else — no match, two matches, a malformed hash —
because a version gate that silently passes on input it does not
recognise is the bug being fixed, not a lenient version of the fix.

Exit status: 0 when the pins agree (or ``--update`` recorded a new pair),
1 on drift, on a refused ``--update`` or on an input the gate cannot
read, 2 when ``tomllib`` is unavailable.

See `#1779`, and ``check-ruff-lockstep.py`` (#1230) for the sibling
gate this one is modelled on.
"""

from __future__ import annotations

import argparse
import collections
import pathlib
import re
import sys

# tomllib landed in 3.11. On older Python, fall back to the external
# `tomli` package (same API), matching check-ruff-lockstep.py.
try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib  # type: ignore[import-not-found,no-redef]
    except ImportError:
        sys.stderr.write(
            "error: check-rumdl-lockstep.py requires Python 3.11+\n"
            "       (tomllib lives in the standard library starting at 3.11).\n"
            "       On older Python, install `tomli` and retry:\n"
            "           pip install tomli\n"
        )
        sys.exit(2)

# `parents[1]`, not `parent`: this gate lives in `utils/` but every path
# it reads is anchored at the repository root.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

MISE_TOML = pathlib.Path("mise.toml")
CI_WORKFLOW = pathlib.Path(".github/workflows/ci.yml")
DOCKERFILE = pathlib.Path("Dockerfile")
BASELINE = pathlib.Path(".rumdl-lockstep-baseline.txt")

BASELINE_HEADER = """\
# rumdl lockstep baseline: the rumdl release and its x86_64 tarball
# SHA256 that check-rumdl-lockstep.py last accepted (#1779).
# Maintained by `python3 utils/check-rumdl-lockstep.py --update`; see the
# script's docstring.
#
# The gate fails when the version pinned in mise.toml, ci.yml and the
# Dockerfile has moved but the hash in ci.yml and the Dockerfile is still
# the one recorded here: the bump kept the old release's hash. A bump
# that pastes the new release's hash into both files fails too, until
# `--update` records the new pair; commit this file with that bump.
#
# One record: `<version> <sha256>`.
"""

RUMDL = "rumdl"
VERSION_VAR = "RUMDL_VERSION"
SHA256_VAR = "RUMDL_SHA256"

# The release artifact both CI and the container's amd64 arm download.
# Keying the Dockerfile arm on the triple rather than on `amd64)` is
# what makes the comparison mean "the same tarball": an arm spelled
# `x86_64)` or reordered still resolves to the hash of this file.
X86_64_TRIPLE = "x86_64-unknown-linux-gnu"

# A step-level `env:` key in the workflow. Anchored to leading
# whitespace and a trailing `:` so the `${RUMDL_VERSION}` expansions in
# the `run:` script and the comment above the step cannot match.
_CI_KEY_RE = {
    name: re.compile(rf"^\s*{name}:\s*(\S.*)$") for name in (VERSION_VAR, SHA256_VAR)
}
# `ENV RUMDL_VERSION=0.3.2`, or the legacy `ENV RUMDL_VERSION 0.3.2`.
_DOCKER_ENV_RE = re.compile(rf"^ENV\s+{VERSION_VAR}(?:=|\s+)(\S+)\s*$")
# `RUN <<'EOF'` / `RUN <<EOF` / `RUN <<-"EOF"`, optionally after
# `--mount=…`-style flags: a heredoc RUN block. The second capture is
# the delimiter that closes it.
_DOCKER_HEREDOC_RE = re.compile(r"""^RUN\s+(?:--\S+\s+)*<<-?(['"]?)(\w+)\1\s*$""")
# The rumdl block is the one that interpolates the version variable
# (in its download URL). Six blocks share the same `case` shape, so
# this — not the comment banner above it — is the anchor.
_DOCKER_VERSION_REF_RE = re.compile(rf"\$\{{?{VERSION_VAR}\b")
_DOCKER_SHA_RE = re.compile(r"""\bSHA256=(['"]?)([^'"\s;]+)\1""")
_SHA256_HEX_RE = re.compile(r"^[0-9a-f]{64}$")


def _scalar(raw: str) -> str:
    """Reduce a YAML scalar to its value: no comment, no quotes.

    A version or a hex digest never contains whitespace or ``#``, so the
    first whitespace-delimited token before any comment is the value.
    """
    words = raw.split("#", 1)[0].split()
    return words[0].strip("\"'") if words else ""


def _sha256(value: str, where: str) -> str:
    """Return ``value`` if it is a lowercase hex SHA256, else hard-error.

    Equality alone would bless two identically truncated pastes, which
    ``sha256sum --check`` would then reject at install time instead.
    """
    if not _SHA256_HEX_RE.match(value):
        raise SystemExit(
            f"error: {where} is not a 64-digit lowercase hex SHA256: {value!r}"
        )
    return value


def read_mise_version(text: str) -> str:
    """Return ``[tools] rumdl`` from ``mise.toml``."""
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(f"error: {MISE_TOML} is not valid TOML: {exc}") from exc
    tools = data.get("tools")
    version = tools.get(RUMDL) if isinstance(tools, dict) else None
    if not isinstance(version, str) or not version:
        raise SystemExit(
            f'error: expected `{RUMDL} = "X.Y.Z"` under [tools] in {MISE_TOML}.'
        )
    return version


def _one_ci_value(text: str, name: str) -> str:
    values = [
        _scalar(match.group(1))
        for line in text.splitlines()
        if (match := _CI_KEY_RE[name].match(line))
    ]
    if len(values) != 1:
        raise SystemExit(
            f"error: expected exactly one `{name}:` key in {CI_WORKFLOW},\n"
            f"       found {len(values)}."
        )
    return values[0]


def read_ci_pin(text: str) -> tuple[str, str]:
    """Return ``(RUMDL_VERSION, RUMDL_SHA256)`` from the CI workflow.

    Exactly one of each is required. The lint job's sibling install
    steps carry their own ``*_VERSION`` / ``*_SHA256`` env maps, and a
    duplicated rumdl step would leave it ambiguous which one CI runs.
    """
    version = _one_ci_value(text, VERSION_VAR)
    digest = _one_ci_value(text, SHA256_VAR)
    return version, _sha256(digest, f"{CI_WORKFLOW} {SHA256_VAR}")


def read_docker_version(text: str) -> str:
    """Return the Dockerfile's ``ENV RUMDL_VERSION``."""
    values = [
        match.group(1).strip("\"'")
        for line in text.splitlines()
        if (match := _DOCKER_ENV_RE.match(line))
    ]
    if len(values) != 1:
        raise SystemExit(
            f"error: expected exactly one `ENV {VERSION_VAR}=` line in "
            f"{DOCKERFILE},\n       found {len(values)}."
        )
    return values[0]


def _heredoc_blocks(text: str) -> list[list[str]]:
    """Split a Dockerfile into the bodies of its heredoc ``RUN`` blocks."""
    blocks: list[list[str]] = []
    body: list[str] | None = None
    delimiter = ""
    for line in text.splitlines():
        if body is None:
            match = _DOCKER_HEREDOC_RE.match(line)
            if match:
                body, delimiter = [], match.group(2)
        elif line.strip() == delimiter:
            blocks.append(body)
            body = None
        else:
            body.append(line)
    return blocks


def read_docker_x86_64_sha(text: str) -> str:
    """Return the SHA256 the Dockerfile's rumdl block pins for x86_64.

    The block is the heredoc ``RUN`` whose download URL interpolates
    ``RUMDL_VERSION``; within it, the arm is the one naming the x86_64
    triple. Anything other than exactly one of each is a hard error.
    """
    rumdl_blocks = [
        block
        for block in _heredoc_blocks(text)
        if any(_DOCKER_VERSION_REF_RE.search(line) for line in block)
    ]
    if len(rumdl_blocks) != 1:
        raise SystemExit(
            f"error: expected exactly one heredoc RUN block in {DOCKERFILE}\n"
            f"       whose download URL names ${{{VERSION_VAR}}}, "
            f"found {len(rumdl_blocks)}."
        )
    digests = [
        match.group(2)
        for line in rumdl_blocks[0]
        if X86_64_TRIPLE in line and (match := _DOCKER_SHA_RE.search(line))
    ]
    if len(digests) != 1:
        raise SystemExit(
            f"error: expected exactly one `SHA256=` on a line naming "
            f"{X86_64_TRIPLE}\n       in {DOCKERFILE}'s rumdl block, "
            f"found {len(digests)}."
        )
    return _sha256(digests[0], f"{DOCKERFILE} {X86_64_TRIPLE} SHA256")


def drifted(versions: dict[pathlib.Path, str]) -> list[pathlib.Path]:
    """Return the files whose version disagrees with the rest.

    That is the files outside the majority, or every file when no value
    is shared by more than one — with three sites the odd one out is
    named by itself, which is what #1653's forgotten Dockerfile needed.
    """
    (common, count), *_ = collections.Counter(versions.values()).most_common(1)
    if count == len(versions):
        return []
    return [
        path for path, version in versions.items() if count == 1 or version != common
    ]


def read_repo_file(relative: pathlib.Path) -> str:
    """Read a repository file, hard-erroring when it is absent."""
    path = REPO_ROOT / relative
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        raise SystemExit(f"error: cannot read {relative}: {exc}") from exc


def read_baseline(text: str) -> tuple[str, str]:
    """Return the ``(version, sha256)`` record of the baseline file."""
    records = [
        line.split()
        for line in text.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]
    if len(records) != 1 or len(records[0]) != 2:
        raise SystemExit(
            f"error: expected exactly one `<version> <sha256>` record in "
            f"{BASELINE}, found {len(records)}."
        )
    version, digest = records[0]
    return version, _sha256(digest, f"{BASELINE} record")


def format_baseline(version: str, digest: str) -> str:
    """The baseline file recording ``version`` and ``digest``."""
    return f"{BASELINE_HEADER}{version} {digest}\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Check that mise.toml, ci.yml and the Dockerfile pin one rumdl release."
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help=f"record the live version and x86_64 hash in {BASELINE} after a bump that moved both",
    )
    args = parser.parse_args(argv)
    docker_text = read_repo_file(DOCKERFILE)
    ci_version, ci_sha = read_ci_pin(read_repo_file(CI_WORKFLOW))
    docker_sha = read_docker_x86_64_sha(docker_text)
    versions = {
        MISE_TOML: read_mise_version(read_repo_file(MISE_TOML)),
        CI_WORKFLOW: ci_version,
        DOCKERFILE: read_docker_version(docker_text),
    }

    problems: list[str] = []

    odd = drifted(versions)
    if odd:
        table = "\n".join(f"    {path}: {RUMDL} {v}" for path, v in versions.items())
        problems.append(
            f"  {RUMDL} versions disagree; drifted: "
            + ", ".join(str(path) for path in odd)
            + f"\n{table}"
        )

    if docker_sha != ci_sha:
        problems.append(
            f"  {DOCKERFILE}: {X86_64_TRIPLE} SHA256 {docker_sha}\n"
            f"    differs from {CI_WORKFLOW} {SHA256_VAR} {ci_sha}.\n"
            f"    Both verify the same tarball; one hash was not updated."
        )

    if problems:
        print(
            "error: rumdl pin declarations disagree:\n"
            + "\n".join(problems)
            + f"\n\nBump {MISE_TOML}, the {CI_WORKFLOW} lint job's "
            f"{VERSION_VAR} / {SHA256_VAR},\nand the {DOCKERFILE}'s "
            f"{VERSION_VAR} plus per-arch SHA256 table together,\n"
            "in the same commit. See #1779.",
            file=sys.stderr,
        )
        return 1

    base_version, base_sha = read_baseline(read_repo_file(BASELINE))
    bumped = ci_version != base_version
    rehashed = ci_sha != base_sha
    if bumped and not rehashed:
        print(
            f"error: {RUMDL} version bumped to {ci_version} but the "
            f"{X86_64_TRIPLE} hash is still {base_version}'s;\n"
            f"  download the new release and paste its sha256 into "
            f"{CI_WORKFLOW} and the {DOCKERFILE}.\n"
            f"  ({BASELINE} records {RUMDL} {base_version} SHA256 "
            f"{base_sha[:12]}...; see #1779.)",
            file=sys.stderr,
        )
        return 1
    if rehashed and not bumped:
        print(
            f"error: the {X86_64_TRIPLE} hash changed but {RUMDL} is still "
            f"{ci_version};\n"
            f"  a release's tarball does not change, so either the version or "
            f"the hash in {CI_WORKFLOW} and the {DOCKERFILE} is wrong.\n"
            f"  ({BASELINE} records SHA256 {base_sha[:12]}... for "
            f"{base_version}; see #1779.)",
            file=sys.stderr,
        )
        return 1

    if args.update:
        if not bumped:
            print(
                f"error: {BASELINE} already records {RUMDL} {ci_version}; "
                "nothing to update.",
                file=sys.stderr,
            )
            return 1
        (REPO_ROOT / BASELINE).write_text(format_baseline(ci_version, ci_sha), encoding="utf-8")
        print(
            f"rumdl lockstep: recorded {RUMDL} {ci_version}, {X86_64_TRIPLE} "
            f"SHA256 {ci_sha[:12]}... in {BASELINE}."
        )
        return 0

    if bumped:
        print(
            f"error: {RUMDL} bumped to {ci_version} (SHA256 {ci_sha[:12]}...) "
            f"but {BASELINE} still records {base_version};\n"
            "  run `python3 utils/check-rumdl-lockstep.py --update` and commit "
            "the result with this bump. See #1779.",
            file=sys.stderr,
        )
        return 1

    print(
        f"rumdl lockstep OK ({RUMDL} {ci_version}, {X86_64_TRIPLE} "
        f"SHA256 {ci_sha[:12]}..., 3 sites checked)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
