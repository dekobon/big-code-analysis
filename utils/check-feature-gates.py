#!/usr/bin/env python3
"""check-feature-gates

Verify that every *union-gated* test subject in ``src/`` is **absent**,
not merely non-vacuous, under a feature set that enables none of the
features its rows name.

``.claude/rules/testing.md`` ("Gate a feature-gated fixture table on the
union of its rows") requires two halves on any test whose case list is
built from ``#[cfg(feature = …)]`` rows:

* ``#[cfg(any(feature = "a", feature = "b", …))]`` on the **item**, so
  a build enabling none of them drops the subject entirely, and
* a non-vacuity guard inside it (``assert_fixtures_present``, a
  ``checked > 0`` counter) covering the residual case where a runtime
  ``is_enabled()`` check stops agreeing with the feature it compiled
  under.

Omitting the first half turns the second into a spurious *failure* on
any feature subset that enables none of the rows. That has shipped three
times — #1220, PR #1221, and ``hidden_literal_supertypes`` (a module
gated on ``test`` alone while both its tests were gated on
``php``/``groovy``). ``cargo clippy --all-targets`` over a partial
feature set catches the compile half of this class; nothing catches the
runtime half, because a module that compiles but has no enabled rows is
a perfectly valid *build*.

This gate closes that. Given the cargo feature flags of a build, it:

1. scans ``src/**/*.rs`` for ``#[cfg(… any(feature = …) …)]`` on a
   ``mod`` or a ``#[test] fn``;
2. computes which of those subjects are **disjoint** from the enabled
   feature set;
3. asks ``cargo nextest list`` what the build actually contains, and
   fails if any disjoint subject is still there.

Run it with the same flags as the build under test, e.g.::

    ./utils/check-feature-gates.py --no-default-features --features go \\
        -p big-code-analysis

It is wired into the ``feature-matrix`` job in
``.github/workflows/ci.yml`` and deliberately *not* into
``make pre-commit``: it needs a test-binary build per feature set, which
would dominate the local gate.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass

# `parents[1]`, not `parent`: these gates live in `utils/` but every path
# they read is anchored at the repository root, so the script works from
# any cwd.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SRC_DIR = REPO_ROOT / "src"

# The package whose `src/` this gate scans. A leg targeting any other
# member never builds these subjects, so the check has nothing to say
# about it.
SUBJECT_PACKAGE = "big-code-analysis"

# `pub`, `pub(crate)`, and the `async`/`const`/`unsafe`/`extern "C"`
# qualifiers may all precede the keyword; the capture groups are the
# item kind and its name.
ITEM_RE = re.compile(
    r"^(?:pub\s*(?:\([^)]*\)\s*)?)?"
    r"(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?"
    r'(?:extern\s+"[^"]*"\s+)?'
    r"(mod|fn)\s+([A-Za-z_][A-Za-z0-9_]*)"
)
# `#[test]`, plus the namespaced spellings (`#[tokio::test]`).
TEST_ATTR_RE = re.compile(r"#\[\s*(?:[A-Za-z_][A-Za-z0-9_]*::)*test\s*\]")
FEATURE_RE = re.compile(r'feature\s*=\s*"([^"]+)"')
# A `feature = "x"` predicate, used to blank the feature names out before
# looking for a bare `test` predicate — otherwise a language named
# `test` would be indistinguishable from `cfg(test)`.
FEATURE_PREDICATE_RE = re.compile(r'feature\s*=\s*"[^"]*"')
BARE_TEST_RE = re.compile(r"\btest\b")


class ScanError(Exception):
    """A malformed input the scanner refuses to guess about."""


# ---------------------------------------------------------------------------
# Rust literal / comment lexing
#
# Ported from `check-snapshot-anchors.py`, which needs it for the same
# reason: a `#[cfg(any(feature = …))]` quoted inside a fixture string or
# sitting in a `//` comment is not a live attribute, and counting one
# produces a spurious failure. The gate scripts are hyphen-named and so
# not importable; every one of them carries its own copy. Fix a lexing
# bug here and check the siblings.
# ---------------------------------------------------------------------------


def char_literal_end(source: str, i: int) -> int | None:
    """End index (exclusive) of the char literal at ``i``, else ``None``.

    Rust spells lifetimes (``'a``), anonymous lifetimes (``'_``) and loop
    labels (``'outer:``) with the same leading quote and no terminator,
    so returning ``None`` for those is what keeps a lifetime from opening
    a span that swallows the rest of the file.
    """
    n = len(source)
    j = i + 1
    if j >= n:
        return None
    if source[j] == "\\":
        j += 1
        if j >= n:
            return None
        if source[j] == "u":
            close = source.find("}", j)
            if close == -1:
                return None
            j = close + 1
        elif source[j] == "x":
            j += 3
        else:
            j += 1
    else:
        j += 1
    return j + 1 if j < n and source[j] == "'" else None


def raw_string_end(source: str, i: int) -> int | None:
    """End index (exclusive) of the raw string at ``i``, else ``None``.

    Covers ``r"…"``, ``r#"…"#`` and the byte-string spellings. A plain
    ``b"…"`` needs no special case: the ``b`` is an ordinary character
    and the ``"`` opens a regular literal.
    """
    n = len(source)
    j = i
    if source[j] == "b" and j + 1 < n and source[j + 1] == "r":
        j += 1
    if j >= n or source[j] != "r":
        return None
    j += 1
    hashes = 0
    while j < n and source[j] == "#":
        hashes += 1
        j += 1
    if j >= n or source[j] != '"':
        return None
    close = '"' + ("#" * hashes)
    end = source.find(close, j + 1)
    return n if end == -1 else end + len(close)


def regular_string_end(source: str, i: int) -> int:
    """End index (exclusive) of the ``"``-delimited literal at ``i``."""
    n = len(source)
    j = i + 1
    while j < n:
        if source[j] == "\\" and j + 1 < n:
            j += 2
            continue
        if source[j] == '"':
            break
        j += 1
    return j + 1


def dead_spans(source: str) -> list[tuple[int, int]]:
    """Index ranges holding comments and string / char literals.

    One walk, string literals consumed before comment openers are tested,
    so a ``//`` inside a string and a ``"`` inside a comment are both
    read correctly.
    """
    spans: list[tuple[int, int]] = []
    i = 0
    n = len(source)
    while i < n:
        ch = source[i]
        if ch == "/" and i + 1 < n and source[i + 1] == "/":
            nl = source.find("\n", i)
            end = n if nl == -1 else nl
            spans.append((i, end))
            i = end
            continue
        if ch == "/" and i + 1 < n and source[i + 1] == "*":
            start = i
            depth = 1
            i += 2
            while i < n and depth > 0:
                if source[i] == "/" and i + 1 < n and source[i + 1] == "*":
                    depth += 1
                    i += 2
                    continue
                if source[i] == "*" and i + 1 < n and source[i + 1] == "/":
                    depth -= 1
                    i += 2
                    continue
                i += 1
            spans.append((start, i))
            continue
        if ch in "rb":
            stop = raw_string_end(source, i)
            if stop is not None:
                spans.append((i, stop))
                i = stop
                continue
        if ch == "'":
            stop = char_literal_end(source, i)
            if stop is not None:
                spans.append((i, stop))
                i = stop
                continue
            i += 1
            continue
        if ch == '"':
            stop = regular_string_end(source, i)
            spans.append((i, stop))
            i = stop
            continue
        i += 1
    return spans


def _in_any_span(idx: int, spans: list[tuple[int, int]]) -> bool:
    return any(start <= idx < end for start, end in spans)


# ---------------------------------------------------------------------------
# Subject discovery
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Subject:
    """A ``mod`` or ``fn`` carrying a ``cfg(any(feature = …))`` gate."""

    path: str
    line: int
    kind: str
    name: str
    features: frozenset[str]
    #: ``#[test]`` (or a namespaced spelling) sits on the same item.
    is_test_fn: bool
    #: Some ``cfg`` in the run also requires the bare ``test`` predicate.
    is_cfg_test: bool

    @property
    def carries_tests(self) -> bool:
        """Whether a build can be asked whether this subject is present.

        A ``#[cfg(test)] mod`` is a container of tests and a ``#[test]
        fn`` is one. A bare helper ``fn`` gated on a union carries no
        test name, so ``nextest`` has nothing to report about it — its
        absence under a disjoint feature set is a pure compile-time
        property, which the leg's ``cargo clippy --all-targets`` already
        covers (an unused helper is ``dead_code``, a missing one is
        ``E0425``).
        """
        return (self.kind == "mod" and self.is_cfg_test) or (
            self.kind == "fn" and self.is_test_fn
        )

    def describe(self) -> str:
        return f"{self.path}:{self.line}  {self.kind} {self.name}"


def _balanced_group(text: str, open_idx: int) -> str | None:
    """Contents of the parenthesised group whose ``(`` is at ``open_idx``."""
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return text[open_idx + 1 : i]
    return None


def union_features(attr: str) -> frozenset[str]:
    """Features named inside an ``any(...)`` group of a ``cfg`` attribute.

    Returns the empty set when the attribute is not a ``cfg``, holds no
    ``any(...)``, or inverts one with ``not(any(...))`` — an inverted
    gate makes the subject present precisely when none of the features is
    enabled, which is the opposite claim and not this gate's business.
    """
    if "cfg" not in attr:
        return frozenset()
    features: set[str] = set()
    for match in re.finditer(r"\bany\s*\(", attr):
        before = attr[: match.start()].rstrip()
        if before.endswith("not("):
            continue
        group = _balanced_group(attr, match.end() - 1)
        if group is None:
            continue
        features.update(FEATURE_RE.findall(group))
    return frozenset(features)


def _cfg_requires_test(attr: str) -> bool:
    """Whether ``attr`` is a ``cfg`` with a bare ``test`` predicate."""
    if "cfg" not in attr:
        return False
    return bool(BARE_TEST_RE.search(FEATURE_PREDICATE_RE.sub("", attr)))


def scan_source(source: str, path: str) -> list[Subject]:
    """Union-gated subjects declared in one Rust source file.

    Attributes accumulate until a code line is reached; that line must be
    the item they attach to. Blank lines and comments between an
    attribute and its item are legal Rust and do not break the run.
    """
    spans = dead_spans(source)
    lines = source.splitlines()
    # Start-of-line character offsets, so a `#[` inside a fixture string
    # can be told from a real attribute.
    offsets: list[int] = []
    pos = 0
    for line in lines:
        offsets.append(pos)
        pos += len(line) + 1

    subjects: list[Subject] = []
    attrs: list[str] = []
    pending: list[str] = []  # partial multi-line attribute
    index = 0
    while index < len(lines):
        raw = lines[index]
        stripped = raw.strip()
        line_no = index + 1
        index += 1

        if pending:
            pending.append(stripped)
            joined = " ".join(pending)
            if joined.count("[") <= joined.count("]"):
                attrs.append(joined)
                pending = []
            continue

        if not stripped or stripped.startswith("//"):
            continue

        if stripped.startswith("#[") or stripped.startswith("#!["):
            hash_idx = offsets[line_no - 1] + (len(raw) - len(raw.lstrip()))
            if _in_any_span(hash_idx, spans):
                continue
            if stripped.count("[") > stripped.count("]"):
                pending = [stripped]
            else:
                attrs.append(stripped)
            continue

        if attrs:
            item = ITEM_RE.match(stripped)
            code_idx = offsets[line_no - 1] + (len(raw) - len(raw.lstrip()))
            if item is not None and not _in_any_span(code_idx, spans):
                features: set[str] = set()
                for attr in attrs:
                    features.update(union_features(attr))
                if features:
                    subjects.append(
                        Subject(
                            path=path,
                            line=line_no,
                            kind=item.group(1),
                            name=item.group(2),
                            features=frozenset(features),
                            is_test_fn=any(
                                TEST_ATTR_RE.search(a) for a in attrs
                            ),
                            is_cfg_test=any(_cfg_requires_test(a) for a in attrs),
                        )
                    )
            attrs = []
    return subjects


def scan_tree(root: pathlib.Path) -> list[Subject]:
    subjects: list[Subject] = []
    for path in sorted(root.rglob("*.rs")):
        if not path.is_file():
            continue
        rel = path.relative_to(REPO_ROOT).as_posix()
        subjects.extend(scan_source(path.read_text(encoding="utf-8"), rel))
    return subjects


# ---------------------------------------------------------------------------
# Cargo feature resolution
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class BuildFlags:
    package: str | None
    no_default_features: bool
    all_features: bool
    requested: frozenset[str]


def parse_build_flags(argv: list[str]) -> BuildFlags:
    """Read the cargo feature selection out of a leg's flag list."""
    package: str | None = None
    no_default = False
    all_features = False
    requested: set[str] = set()

    index = 0
    while index < len(argv):
        arg = argv[index]
        index += 1
        if arg == "--no-default-features":
            no_default = True
        elif arg == "--all-features":
            all_features = True
        elif arg in ("--features", "-F"):
            if index >= len(argv):
                raise ScanError(f"{arg} needs a value")
            requested.update(re.split(r"[,\s]+", argv[index].strip()))
            index += 1
        elif arg.startswith("--features="):
            requested.update(re.split(r"[,\s]+", arg.partition("=")[2].strip()))
        elif arg in ("-p", "--package"):
            if index >= len(argv):
                raise ScanError(f"{arg} needs a value")
            package = argv[index]
            index += 1
        elif arg.startswith("--package="):
            package = arg.partition("=")[2]
    requested.discard("")
    return BuildFlags(package, no_default, all_features, frozenset(requested))


def manifest_features(manifest: pathlib.Path) -> dict[str, list[str]]:
    with manifest.open("rb") as handle:
        data = tomllib.load(handle)
    table = data.get("features", {})
    return {name: list(values) for name, values in table.items()}


def resolve_features(table: dict[str, list[str]], flags: BuildFlags) -> set[str]:
    """The transitive feature closure a build with ``flags`` enables.

    ``default = ["all-languages"]`` and ``all-languages = [22 languages]``
    means the answer is never the literal flag list; without the closure
    every subject would read as disjoint under the default leg.
    """
    if flags.all_features:
        return set(table)
    seeds = set(flags.requested)
    if not flags.no_default_features:
        seeds.update(table.get("default", []))

    enabled: set[str] = set()
    queue = list(seeds)
    while queue:
        name = queue.pop()
        # `dep:foo` activates an optional dependency and `crate/feat`
        # forwards to another package; neither names a feature of this
        # one, so neither can satisfy a `cfg(feature = …)` here.
        if name.startswith("dep:") or "/" in name or name in enabled:
            continue
        enabled.add(name)
        queue.extend(table.get(name, []))
    return enabled


# ---------------------------------------------------------------------------
# What the build actually contains
# ---------------------------------------------------------------------------


def list_tests(flags: list[str]) -> list[str]:
    """Every test name ``cargo nextest`` reports for this feature set.

    One listing rather than one ``-E test(/subject/)`` invocation per
    subject: the build is shared and the filterset would only be
    re-applying, in nextest, a name match this script can do directly.
    ``-T oneline`` prints ``<binary-id> <test-name>`` per line.
    """
    command = ["cargo", "nextest", "list", "-T", "oneline", *flags]
    try:
        result = subprocess.run(
            command,
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
    except FileNotFoundError as exc:  # pragma: no cover - environment probe
        raise ScanError(
            "cargo-nextest not found; install it with "
            "`cargo install cargo-nextest --locked`"
        ) from exc
    if result.returncode != 0:
        raise ScanError(
            f"`{' '.join(command)}` failed with exit {result.returncode}:\n"
            f"{result.stderr.strip()}"
        )
    names: list[str] = []
    for line in result.stdout.splitlines():
        _, _, name = line.strip().partition(" ")
        if name:
            names.append(name.strip())
    return names


def subject_matcher(subject: Subject) -> re.Pattern[str]:
    """Match a nextest test name belonging to ``subject``.

    Test names are ``::``-separated module paths, so the subject is a
    whole path segment — anchoring on the separators keeps
    ``a_type_declared_inside_a_function`` from matching
    ``a_type_declared_inside_a_function_reaches_the_root_rollup``.
    """
    return re.compile(rf"(?:^|::){re.escape(subject.name)}(?:::|$)")


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def _print_subjects(subjects: list[Subject]) -> None:
    for subject in subjects:
        kind = "checked" if subject.carries_tests else "compile-time only"
        features = ", ".join(sorted(subject.features))
        print(f"{subject.describe()}  [{kind}]  any({features})")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__.splitlines()[0],
        epilog=(
            "Any argument not listed above is passed through to cargo, so "
            "this takes a CI leg's flags verbatim."
        ),
    )
    parser.add_argument(
        "--show",
        action="store_true",
        help="Print the discovered union-gated subjects and exit 0.",
    )
    args, cargo_flags = parser.parse_known_args(argv)

    subjects = scan_tree(SRC_DIR)
    if args.show:
        _print_subjects(subjects)
        return 0

    # Non-vacuity, first half: a scan that finds nothing has either lost
    # its parser to a formatting change or been pointed at the wrong
    # tree. Either way it would pass every leg while asserting nothing,
    # which is the exact defect this gate exists to prevent.
    if not subjects:
        sys.stderr.write(
            f"error: no `#[cfg(any(feature = …))]` subjects found under {SRC_DIR}\n"
            "       the scanner is broken or the tree is wrong; this gate\n"
            "       cannot pass by finding nothing to check.\n"
        )
        return 2

    try:
        flags = parse_build_flags(cargo_flags)
    except ScanError as exc:
        sys.stderr.write(f"error: {exc}\n")
        return 2

    if flags.package is not None and flags.package != SUBJECT_PACKAGE:
        print(
            f"feature-gates: skipped — subjects live in `{SUBJECT_PACKAGE}`, "
            f"this build targets `{flags.package}`."
        )
        return 0

    try:
        enabled = resolve_features(manifest_features(REPO_ROOT / "Cargo.toml"), flags)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        sys.stderr.write(f"error: reading root Cargo.toml: {exc}\n")
        return 2

    checkable = [s for s in subjects if s.carries_tests]
    disjoint = [s for s in checkable if not (s.features & enabled)]

    if not disjoint:
        print(
            f"feature-gates: OK — {len(checkable)} test subject(s), every one "
            "with an enabled row under this feature set (nothing to verify)."
        )
        return 0

    try:
        names = list_tests(cargo_flags)
    except ScanError as exc:
        sys.stderr.write(f"error: {exc}\n")
        return 2

    offenders: list[tuple[Subject, list[str]]] = []
    for subject in disjoint:
        matcher = subject_matcher(subject)
        hits = [name for name in names if matcher.search(name)]
        if hits:
            offenders.append((subject, hits))

    if offenders:
        sys.stderr.write(
            "error: a union-gated test subject is present under a feature set\n"
            "       that enables none of the features its rows name\n\n"
        )
        for subject, hits in offenders:
            sys.stderr.write(f"  {subject.describe()}\n")
            sys.stderr.write(
                f"      gated on any({', '.join(sorted(subject.features))}) "
                "— none enabled here\n"
            )
            for hit in hits[:5]:
                sys.stderr.write(f"      still listed: {hit}\n")
            if len(hits) > 5:
                sys.stderr.write(f"      … and {len(hits) - 5} more\n")
        sys.stderr.write(
            "\nPut `#[cfg(any(feature = \"a\", feature = \"b\", …))]` on the item\n"
            "itself, naming the union of the features its rows use, so the\n"
            "subject is *absent* rather than tripping its own non-vacuity\n"
            "guard. See `.claude/rules/testing.md`, \"Gate a feature-gated\n"
            "fixture table on the union of its rows\" (#1220, #1472).\n"
        )
        return 1

    print(
        f"feature-gates: OK — {len(disjoint)} of {len(checkable)} test subject(s) "
        f"enable no row here, and none is present in the build."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
