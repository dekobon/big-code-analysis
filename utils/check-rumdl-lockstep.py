#!/usr/bin/env python3
"""check-rumdl-lockstep

rumdl, actionlint and checkmake are installed from pinned, SHA256-verified
release artifacts both by CI and in the dev container, and only comments
kept the two sides' pins in step. Each row of ``PINS`` is one of them.

**rumdl** lints this repository's Markdown and is pinned in three files:

* ``mise.toml`` — ``[tools] rumdl = "X.Y.Z"``, what ``mise install``
  gives a contributor;
* ``.github/workflows/ci.yml`` — the ``lint`` job's ``RUMDL_VERSION`` /
  ``RUMDL_SHA256`` step env, what CI downloads and verifies;
* ``Dockerfile`` — ``ENV RUMDL_VERSION`` plus the per-arch SHA256
  ``case`` in the rumdl ``RUN`` block, what the dev container installs.

**actionlint** lints the workflows and is pinned in the last two only:
the same job's ``ACTIONLINT_VERSION`` / ``ACTIONLINT_SHA256``, and
``ENV ACTIONLINT_VERSION`` plus the ``case`` in its ``RUN`` block
(#1799). ``mise.toml`` declares no actionlint.

**checkmake** lints the Makefile and is pinned the same way as actionlint:
``CHECKMAKE_VERSION`` / ``CHECKMAKE_SHA256`` in the job, and
``ENV CHECKMAKE_VERSION`` plus the ``case`` in its ``RUN`` block (#1820).
Both arms install the same checkmake/checkmake release; only the amd64
hash is compared, as for the other two tools.

#1653 shows the cost. Its work list named ``mise.toml`` and ``ci.yml``
and left out the ``Dockerfile``, which the bump caught only by grepping.
Had it been missed, the container would lint on a different rumdl than
CI, and the only symptom would be "clean in the container, red in CI"
on the first document that tripped a rule the two versions disagree on.

Three invariants are checked for each tool.

**Its versions are equal.** There is no natural anchor — no file is
derived from another — so a disagreement names the file that differs
from the others, or every file when no two agree; for actionlint's and
checkmake's two sites that is both. Every value is printed either way,
so the reader sees the whole table rather than a verdict about which
side is "right".

**The Dockerfile's amd64 SHA256 equals ``ci.yml``'s.** Both verify the
same artifact — ``rumdl-v<version>-x86_64-unknown-linux-gnu.tar.gz``,
``actionlint_<version>_linux_amd64.tar.gz``,
``checkmake-<version>.linux.amd64`` — so they must match byte for byte.
This catches a version bump that pasted only one hash, offline. The
arm64 hashes are deliberately not compared: CI downloads and verifies
amd64 only, so they have no counterpart to compare against, and the
image build's ``sha256sum --check`` already covers them.

**A version bump moves the hash too.** The two checks above pass a bump
that changed every version and left both hashes alone, because the stale
hash still agrees with itself. Each tool's baseline
(``.rumdl-lockstep-baseline.txt``, ``.actionlint-lockstep-baseline.txt``,
``.checkmake-lockstep-baseline.txt``)
records the version and amd64 SHA256 the gate last accepted, and the
gate fails by name when the version has moved but the hash has not, or
the hash has moved but the version has not (a release's artifact does not
change). A bump that moves both fails too, until ``--update`` records
the new pair in the same commit: a record left at the old release would
check the *next* bump against the wrong hash, and a passing run's note
is read by no one. ``--update`` validates every tool before it writes
anything: it records nothing while any tool's pins are inconsistent,
rewrites only the baselines whose pair moved, and refuses when none did.

**Why these three, not every pin the Dockerfile header mirrors from
``ci.yml``.** taplo, shfmt and shellcheck reach CI through
``taiki-e/install-action``, which carries no SHA256 to compare.
checkmake joined once its block took the rumdl/actionlint shape — one
``<arch variable>="…"; SHA256="…"`` line per arch arm, here with the
whole ``URL`` as the variable — in place of the inline ``echo "<sha>  /tmp/checkmake" |
sha256sum --check`` the Dockerfile reader could not anchor on (#1820).

**Adding a tool.** Four edits and a bootstrap, in this order: the CI
step's ``<TOOL>_VERSION`` / ``<TOOL>_SHA256`` env keys; the Dockerfile's
``ENV <TOOL>_VERSION`` and a heredoc ``RUN`` block in the shape above,
whose download URL names ``${<TOOL>_VERSION}``; a ``Pin`` row in ``PINS``
with a comment above it saying what its ``amd64_anchor`` is; the new
baseline's file name in both ``files:`` regexes of
``.pre-commit-config.yaml``, and the tool's name in the Makefile help,
comment and echo. Then seed the baseline — ``--update`` refuses a
missing file on purpose, so that a typo in a baseline path reads as an
error rather than as a fresh start — with a stale record, ``0.0.1`` and
any 64 hex digits, and run ``--update``, which rewrites it from the
template; commit what it wrote. In the self-tests, ``CI_SAMPLE`` and
``DOCKER_SAMPLE`` need the tool's sites, ``_NEXT`` its next release and
``HAND_<TOOL>_BASELINE`` a hand-written record, and ``RepoCase._repo``
a ``<tool>_baseline`` keyword with its default and write (the
malformed-baseline test derives that name from every row); the tests
that assert a fixed tool count or one case per tool —
``test_an_aligned_tree_passes``, ``test_the_arm64_hash_is_not_compared``,
the ``*_only_bump`` tests, ``test_clean_tree_passes`` — grow a row. The
tests that iterate ``PINS`` then cover the rest.

``mise.toml`` is parsed with ``tomllib``. ``ci.yml`` and the
``Dockerfile`` are parsed by line, because PyYAML is not in the
standard library and nothing parses a Dockerfile: this gate runs under a
contributor's bare ``python3`` and as a ``language: system`` pre-commit
hook. Each read is anchored to a shape that occurs exactly once and
hard-errors on anything else — no match, two matches, a malformed hash —
because a version gate that silently passes on input it does not
recognise is the bug being fixed, not a lenient version of the fix. A
hard error on one tool's inputs still reports the other tools' drift.

Exit status: 0 when the pins agree (or ``--update`` recorded a new pair),
1 on drift, on a refused ``--update`` or on an input the gate cannot
read, 2 when ``tomllib`` is unavailable.

See `#1779`, `#1799` and `#1820`, and ``check-ruff-lockstep.py`` (#1230)
for the sibling gate this one is modelled on.
"""

from __future__ import annotations

import argparse
import collections
import pathlib
import re
import sys
import textwrap
from dataclasses import dataclass, replace

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


@dataclass(frozen=True)
class Pin:
    """One tool whose release ``ci.yml`` and the ``Dockerfile`` both pin."""

    tool: str
    # The CI step's env keys. The version key doubles as the Dockerfile
    # `ENV`, and its `${...}` expansion is what marks the tool's RUN block.
    version_var: str
    sha256_var: str
    # A substring that only the amd64 arm's line of the tool's Dockerfile
    # `case` carries, taken from what that arm feeds the download URL — a
    # target triple, an arch-variable assignment, or the asset suffix of
    # a whole URL — rather than the `amd64)` label, so an arm spelled
    # `x86_64)` or reordered still resolves to the hash of the artifact
    # CI verifies. The reader hard-errors unless exactly one line in the
    # block carries it beside a `SHA256=`.
    amd64_anchor: str
    # How messages name that artifact.
    artifact: str
    baseline: pathlib.Path
    # The issue that added the row, cited by its messages.
    issue: str
    # The `[tools]` key in mise.toml, for a tool mise pins too.
    mise_key: str | None = None


RUMDL = Pin(
    tool="rumdl",
    version_var="RUMDL_VERSION",
    sha256_var="RUMDL_SHA256",
    amd64_anchor="x86_64-unknown-linux-gnu",
    artifact="x86_64-unknown-linux-gnu",
    baseline=pathlib.Path(".rumdl-lockstep-baseline.txt"),
    issue="#1779",
    mise_key="rumdl",
)
# The arm sets the `linux_${ACTIONLINT_ARCH}` suffix of the download URL,
# so `amd64` here is CI's `linux_amd64`.
ACTIONLINT = Pin(
    tool="actionlint",
    version_var="ACTIONLINT_VERSION",
    sha256_var="ACTIONLINT_SHA256",
    amd64_anchor='ACTIONLINT_ARCH="amd64"',
    artifact="linux_amd64",
    baseline=pathlib.Path(".actionlint-lockstep-baseline.txt"),
    issue="#1799",
)
# Each arm sets its whole URL; `linux.amd64` is the asset suffix the amd64
# URL ends in, which is what CI's URL ends in too.
CHECKMAKE = Pin(
    tool="checkmake",
    version_var="CHECKMAKE_VERSION",
    sha256_var="CHECKMAKE_SHA256",
    amd64_anchor="linux.amd64",
    artifact="linux.amd64",
    baseline=pathlib.Path(".checkmake-lockstep-baseline.txt"),
    issue="#1820",
)
PINS = (RUMDL, ACTIONLINT, CHECKMAKE)

# One template for every baseline's header, a paragraph per entry and
# "" for a bare `#` line. It is filled rather than spelled per tool so the
# baselines cannot drift apart in wording; `_BASELINE_WIDTH` is the
# column the rumdl baseline was first wrapped at, which reproduces its
# committed bytes. After editing the template, rewrite every baseline by
# setting each record to a stale version and hash and running `--update`,
# which refuses a run where nothing moved.
_BASELINE_HEADER = (
    "{tool} lockstep baseline: the {tool} release and its x86_64 artifact "
    "SHA256 that check-rumdl-lockstep.py last accepted ({issue}).",
    "Maintained by `python3 utils/check-rumdl-lockstep.py --update`; see the "
    "script's docstring.",
    "",
    "The gate fails when the version pinned in {sites} has moved but the hash "
    "in ci.yml and the Dockerfile is still the one recorded here: the bump kept "
    "the old release's hash. A bump that pastes the new release's hash into both "
    "files fails too, until `--update` records the new pair; commit this file "
    "with that bump.",
    "",
    "One record: `<version> <sha256>`.",
)
_BASELINE_WIDTH = 73

# `RUN <<'EOF'` / `RUN <<EOF` / `RUN <<-"EOF"`, optionally after
# `--mount=…`-style flags: a heredoc RUN block. The second capture is
# the delimiter that closes it.
_DOCKER_HEREDOC_RE = re.compile(r"""^RUN\s+(?:--\S+\s+)*<<-?(['"]?)(\w+)\1\s*$""")
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


def read_mise_version(text: str, key: str) -> str:
    """Return ``[tools] <key>`` from ``mise.toml``."""
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        raise SystemExit(f"error: {MISE_TOML} is not valid TOML: {exc}") from exc
    tools = data.get("tools")
    version = tools.get(key) if isinstance(tools, dict) else None
    if not isinstance(version, str) or not version:
        raise SystemExit(
            f'error: expected `{key} = "X.Y.Z"` under [tools] in {MISE_TOML}.'
        )
    return version


def _one_ci_value(text: str, name: str) -> str:
    # A step-level `env:` key in the workflow. Anchored to leading
    # whitespace and a trailing `:` so the `${NAME}` expansions in the
    # `run:` script and the comment above the step cannot match.
    key_re = re.compile(rf"^\s*{re.escape(name)}:\s*(\S.*)$")
    values = [
        _scalar(match.group(1))
        for line in text.splitlines()
        if (match := key_re.match(line))
    ]
    if len(values) != 1:
        raise SystemExit(
            f"error: expected exactly one `{name}:` key in {CI_WORKFLOW},\n"
            f"       found {len(values)}."
        )
    return values[0]


def read_ci_pin(text: str, pin: Pin) -> tuple[str, str]:
    """Return the tool's ``(<VERSION>, <SHA256>)`` from the CI workflow.

    Exactly one of each is required. The lint job's sibling install
    steps carry their own ``*_VERSION`` / ``*_SHA256`` env maps, and a
    duplicated step would leave it ambiguous which one CI runs.
    """
    version = _one_ci_value(text, pin.version_var)
    digest = _one_ci_value(text, pin.sha256_var)
    return version, _sha256(digest, f"{CI_WORKFLOW} {pin.sha256_var}")


def read_docker_version(text: str, pin: Pin) -> str:
    """Return the Dockerfile's ``ENV <VERSION>``."""
    # `ENV RUMDL_VERSION=0.3.2`, or the legacy `ENV RUMDL_VERSION 0.3.2`.
    env_re = re.compile(rf"^ENV\s+{re.escape(pin.version_var)}(?:=|\s+)(\S+)\s*$")
    values = [
        match.group(1).strip("\"'")
        for line in text.splitlines()
        if (match := env_re.match(line))
    ]
    if len(values) != 1:
        raise SystemExit(
            f"error: expected exactly one `ENV {pin.version_var}=` line in "
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


def read_docker_x86_64_sha(text: str, pin: Pin) -> str:
    """Return the SHA256 the Dockerfile's block for ``pin`` pins for amd64.

    The block is the heredoc ``RUN`` whose body interpolates the tool's
    version variable (in its download URL): several blocks share the same
    ``case`` shape, so that — not the comment banner above it — is the
    anchor. Within it, the arm is the line naming ``pin.amd64_anchor``.
    Anything other than exactly one of each is a hard error.
    """
    version_ref = re.compile(rf"\$\{{?{re.escape(pin.version_var)}\b")
    tool_blocks = [
        block
        for block in _heredoc_blocks(text)
        if any(version_ref.search(line) for line in block)
    ]
    if len(tool_blocks) != 1:
        raise SystemExit(
            f"error: expected exactly one heredoc RUN block in {DOCKERFILE}\n"
            f"       whose download URL names ${{{pin.version_var}}}, "
            f"found {len(tool_blocks)}."
        )
    digests = [
        match.group(2)
        for line in tool_blocks[0]
        if pin.amd64_anchor in line and (match := _DOCKER_SHA_RE.search(line))
    ]
    if len(digests) != 1:
        raise SystemExit(
            f"error: expected exactly one `SHA256=` on a line naming "
            f"{pin.amd64_anchor}\n       in {DOCKERFILE}'s {pin.tool} block, "
            f"found {len(digests)}."
        )
    return _sha256(digests[0], f"{DOCKERFILE} {pin.artifact} SHA256")


def drifted(versions: dict[pathlib.Path, str]) -> list[pathlib.Path]:
    """Return the files whose version disagrees with the rest.

    That is the files outside the majority, or every file when no value
    is shared by more than one — with three sites the odd one out is
    named by itself, which is what #1653's forgotten Dockerfile needed;
    two sites that disagree have no majority, so both are named.
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


def read_baseline(text: str, pin: Pin) -> tuple[str, str]:
    """Return the ``(version, sha256)`` record of ``pin``'s baseline file."""
    records = [
        line.split()
        for line in text.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]
    if len(records) != 1 or len(records[0]) != 2:
        raise SystemExit(
            f"error: expected exactly one `<version> <sha256>` record in "
            f"{pin.baseline}, found {len(records)}."
        )
    version, digest = records[0]
    return version, _sha256(digest, f"{pin.baseline} record")


def format_baseline(pin: Pin, version: str, digest: str) -> str:
    """The baseline file recording ``version`` and ``digest`` for ``pin``."""
    sites = ("mise.toml, " if pin.mise_key else "") + "ci.yml and the Dockerfile"
    lines = (
        # Not at hyphens: `check-rumdl-lockstep.py` must stay whole.
        textwrap.fill(
            paragraph.format(tool=pin.tool, issue=pin.issue, sites=sites),
            width=_BASELINE_WIDTH,
            initial_indent="# ",
            subsequent_indent="# ",
            break_long_words=False,
            break_on_hyphens=False,
        )
        if paragraph
        else "#"
        for paragraph in _BASELINE_HEADER
    )
    return "".join(f"{line}\n" for line in lines) + f"{version} {digest}\n"


@dataclass(frozen=True)
class Verdict:
    """What one tool's live pins say, measured against its baseline."""

    pin: Pin
    version: str
    digest: str
    sites: int
    # The live pins contradict each other or the record, so no mode may
    # record them.
    invalid: str | None = None
    # The live pair is consistent but not the recorded one: a plain run's
    # failure, and what `--update` writes.
    unrecorded: str | None = None


def evaluate(pin: Pin, texts: dict[pathlib.Path, str]) -> Verdict:
    """Read ``pin``'s sites from ``texts`` and judge them against its baseline."""
    docker_text = texts[DOCKERFILE]
    ci_version, ci_sha = read_ci_pin(texts[CI_WORKFLOW], pin)
    docker_sha = read_docker_x86_64_sha(docker_text, pin)
    versions: dict[pathlib.Path, str] = {}
    if pin.mise_key is not None:
        versions[MISE_TOML] = read_mise_version(texts[MISE_TOML], pin.mise_key)
    versions[CI_WORKFLOW] = ci_version
    versions[DOCKERFILE] = read_docker_version(docker_text, pin)
    verdict = Verdict(pin, ci_version, ci_sha, len(versions))

    problems: list[str] = []

    odd = drifted(versions)
    if odd:
        table = "\n".join(f"    {path}: {pin.tool} {v}" for path, v in versions.items())
        problems.append(
            f"  {pin.tool} versions disagree; drifted: "
            + ", ".join(str(path) for path in odd)
            + f"\n{table}"
        )

    if docker_sha != ci_sha:
        problems.append(
            f"  {DOCKERFILE}: {pin.artifact} SHA256 {docker_sha}\n"
            f"    differs from {CI_WORKFLOW} {pin.sha256_var} {ci_sha}.\n"
            f"    Both verify the same artifact; one hash was not updated."
        )

    if problems:
        mise = f"{MISE_TOML}, " if pin.mise_key else ""
        return replace(
            verdict,
            invalid=f"error: {pin.tool} pin declarations disagree:\n"
            + "\n".join(problems)
            + f"\n\nBump {mise}the {CI_WORKFLOW} lint job's "
            f"{pin.version_var} / {pin.sha256_var},\nand the {DOCKERFILE}'s "
            f"{pin.version_var} plus per-arch SHA256 table together,\n"
            f"in the same commit. See {pin.issue}.",
        )

    base_version, base_sha = read_baseline(read_repo_file(pin.baseline), pin)
    bumped = ci_version != base_version
    rehashed = ci_sha != base_sha
    if bumped and not rehashed:
        return replace(
            verdict,
            invalid=f"error: {pin.tool} version bumped to {ci_version} but the "
            f"{pin.artifact} hash is still {base_version}'s;\n"
            f"  download the new release and paste its sha256 into "
            f"{CI_WORKFLOW} and the {DOCKERFILE}.\n"
            f"  ({pin.baseline} records {pin.tool} {base_version} SHA256 "
            f"{base_sha[:12]}...; see {pin.issue}.)",
        )
    if rehashed and not bumped:
        return replace(
            verdict,
            invalid=f"error: the {pin.artifact} hash changed but {pin.tool} is "
            f"still {ci_version};\n"
            f"  a release's artifact does not change, so either the version or "
            f"the hash in {CI_WORKFLOW} and the {DOCKERFILE} is wrong.\n"
            f"  ({pin.baseline} records SHA256 {base_sha[:12]}... for "
            f"{base_version}; see {pin.issue}.)",
        )
    if bumped:
        return replace(
            verdict,
            unrecorded=f"error: {pin.tool} bumped to {ci_version} (SHA256 "
            f"{ci_sha[:12]}...) but {pin.baseline} still records {base_version};\n"
            "  run `python3 utils/check-rumdl-lockstep.py --update` and commit "
            f"the result with this bump. See {pin.issue}.",
        )
    return verdict


def update(verdicts: list[Verdict]) -> int:
    """Record every moved pair, or nothing if any tool's pins are invalid."""
    invalid = [verdict.invalid for verdict in verdicts if verdict.invalid]
    if invalid:
        for message in invalid:
            print(message, file=sys.stderr)
        print(
            "note: --update recorded nothing; every tool's pins must agree first.",
            file=sys.stderr,
        )
        return 1
    moved = [verdict for verdict in verdicts if verdict.unrecorded]
    if not moved:
        print(
            "error: nothing to update:\n"
            + "\n".join(
                f"  {v.pin.baseline} already records {v.pin.tool} {v.version}"
                for v in verdicts
            ),
            file=sys.stderr,
        )
        return 1
    for verdict in moved:
        pin = verdict.pin
        (REPO_ROOT / pin.baseline).write_text(
            format_baseline(pin, verdict.version, verdict.digest), encoding="utf-8"
        )
        print(
            f"{pin.tool} lockstep: recorded {pin.tool} {verdict.version}, "
            f"{pin.artifact} SHA256 {verdict.digest[:12]}... in {pin.baseline}."
        )
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Check that mise.toml, ci.yml and the Dockerfile pin one rumdl, "
            "one actionlint and one checkmake release."
        )
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help=(
            "record each tool's live version and amd64 hash in its baseline "
            "after a bump that moved both"
        ),
    )
    args = parser.parse_args(argv)
    texts = {path: read_repo_file(path) for path in (MISE_TOML, CI_WORKFLOW, DOCKERFILE)}
    verdicts: list[Verdict] = []
    unreadable: list[str] = []
    for pin in PINS:
        # The tools share no values, so an input one of them cannot read
        # must not hide what the other tools' pins already say.
        try:
            verdicts.append(evaluate(pin, texts))
        except SystemExit as exc:
            unreadable.append(str(exc))
    if unreadable:
        reported = [
            message
            for verdict in verdicts
            # An unrecorded pair is what `--update` was asked to record.
            for message in (verdict.invalid, None if args.update else verdict.unrecorded)
            if message
        ]
        raise SystemExit("\n".join(reported + unreadable))

    if args.update:
        return update(verdicts)

    failures = [
        message
        for verdict in verdicts
        for message in (verdict.invalid, verdict.unrecorded)
        if message
    ]
    if failures:
        for message in failures:
            print(message, file=sys.stderr)
        return 1

    for verdict in verdicts:
        pin = verdict.pin
        print(
            f"{pin.tool} lockstep OK ({pin.tool} {verdict.version}, {pin.artifact} "
            f"SHA256 {verdict.digest[:12]}..., {verdict.sites} sites checked)."
        )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
