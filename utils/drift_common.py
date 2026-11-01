"""Shared pieces of the role-series drift drivers (`drift-oracle.py`,
`drift-compare.py`, `recovery-oracle.py`; #1744).

- `populations()`: language slug -> extension tuple, derived from the
  `mk_langs!` table through `role_tables.languages()` so a corpus
  sweep cannot silently skip an extension the enum owns, plus the
  explicit rows for the fork and helper grammars that read another
  population's files.
- `grammar_pins()`: the `tree-sitter*` pins of the root `Cargo.toml`,
  read with `tomllib`.
- `capture()`: one subprocess run captured as bytes (never text:
  `errors="replace"` collapses distinct invalid sequences and universal
  newlines fold CRLF, so a real output change could compare equal).
- `walk()`: a recursive, symlink-logging directory walk keyed on the
  path relative to its root.
- `MANIFEST_VERSION` / `build_manifest()`: the one place a capture
  manifest's shape is spelled; `drift-compare.py` refuses any other
  version.

Nothing lints `utils/*.py` today (#1477); `drift_common-test.py`
(`make drift-common-test`) is the only gate on this module.
"""

from __future__ import annotations

import pathlib
import subprocess
import sys
import tomllib
from dataclasses import dataclass

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
DEFAULT_CARGO_TOML = REPO_ROOT / "Cargo.toml"

sys.path.insert(0, str(UTILS_DIR))

import role_tables  # noqa: E402

MANIFEST_VERSION = 1

# The variants that read another population's files. Mozcpp (no
# extension of its own) and Mozjs (only `.jsm`) are forks whose parity
# with Cpp / Javascript is what the sweep measures, so each runs over
# its sibling's population as well as its own. Ccomment / Preproc own
# none and run over C / C++ sources.
BORROWS: dict[str, str] = {"mozcpp": "cpp", "mozjs": "javascript"}
DECLARED: dict[str, tuple[str, ...]] = {
    "ccomment": (".c", ".cpp", ".h"),
    "preproc": (".c", ".cpp", ".h"),
}


def populations(langs: list[role_tables.Language] | None = None) -> dict[str, tuple[str, ...]]:
    """slug -> dotted extensions, in `mk_langs!` order. Every variant of
    the enum is a key: its own row's extensions, plus its `BORROWS`
    donor's, or its `DECLARED` tuple. A variant with none of the three
    is an error, never a population that silently drops out of a
    sweep; so is a `BORROWS` / `DECLARED` key naming no variant."""
    rows = role_tables.languages() if langs is None else langs
    by_stem = {lang.stem: lang for lang in rows}
    out: dict[str, tuple[str, ...]] = {}
    for lang in rows:
        exts = [f".{e}" for e in lang.extensions]
        donor = BORROWS.get(lang.stem)
        if donor is not None:
            if donor not in by_stem or not by_stem[donor].extensions:
                raise role_tables.RoleTableError(f"{lang.stem} borrows extensions from {donor!r}, which has none")
            exts += [f".{e}" for e in by_stem[donor].extensions if f".{e}" not in exts]
        exts += [e for e in DECLARED.get(lang.stem, ()) if e not in exts]
        if not exts:
            raise role_tables.RoleTableError(
                f"LANG::{lang.variant} lists no extension and drift_common names none for it"
            )
        out[lang.stem] = tuple(exts)
    stale = (set(BORROWS) | set(DECLARED)) - set(out)
    if stale:
        raise role_tables.RoleTableError(f"drift_common names no LANG variant: {sorted(stale)}")
    return out


def langs_by_extension(pops: dict[str, tuple[str, ...]] | None = None) -> dict[str, list[str]]:
    """dotted extension -> the slugs that read it, in population order."""
    out: dict[str, list[str]] = {}
    for slug, exts in (populations() if pops is None else pops).items():
        for ext in exts:
            out.setdefault(ext, []).append(slug)
    return out


def grammar_pins(cargo_toml: pathlib.Path = DEFAULT_CARGO_TOML) -> dict[str, str]:
    """dependency name -> version requirement for every `tree-sitter*`
    entry of `[workspace.dependencies]`, read with `tomllib`. A vendored
    grammar's requirement is the `version` of its inline table."""
    with cargo_toml.open("rb") as fh:
        doc = tomllib.load(fh)
    deps = doc.get("workspace", {}).get("dependencies", {})
    pins: dict[str, str] = {}
    for name, spec in deps.items():
        if "tree-sitter" not in name:
            continue
        version = spec if isinstance(spec, str) else spec.get("version")
        if version is None:
            raise ValueError(f"{cargo_toml}: {name} has no version requirement")
        pins[name] = version
    if not pins:
        raise ValueError(f"{cargo_toml}: no `tree-sitter*` pins under [workspace.dependencies]")
    return pins


@dataclass(frozen=True)
class Capture:
    """One subprocess run. `how` is None for a clean exit, else
    `"exit N"` / `"signal N"`."""

    argv: tuple[str, ...]
    returncode: int
    stdout: bytes
    stderr: bytes

    @property
    def how(self) -> str | None:
        if self.returncode < 0:
            return f"signal {-self.returncode}"
        if self.returncode != 0:
            return f"exit {self.returncode}"
        return None

    def preview(self) -> str:
        """The tail of stderr, decoded for a diagnostic only."""
        return self.stderr[-400:].decode("utf-8", errors="replace")

    def rendered(self) -> bytes:
        """The bytes a capture file holds: stdout, then stderr behind a
        marker when there is any, behind a status line when the run was
        not clean."""
        body = self.stdout + (b"\nSTDERR\n" + self.stderr if self.stderr else b"")
        if self.how is None:
            return body
        return self.how.upper().encode() + b"\n" + body


def capture(argv: list[str]) -> Capture:
    r = subprocess.run(argv, capture_output=True)
    return Capture(tuple(argv), r.returncode, r.stdout, r.stderr)


def walk(
    root: pathlib.Path,
    exts: tuple[str, ...],
    skip_parts: tuple[str, ...],
    skipped_links: list[str],
) -> list[pathlib.Path]:
    """Every regular file under `root` whose suffix is in `exts`, sorted.
    `rglob` does not descend a symlinked directory, so one is skipped
    silently; this walks by hand so every link — file or directory — is
    appended to `skipped_links` instead of followed, since a followed
    link would let a key escape its root. A path with any of
    `skip_parts` among its root-relative parts is skipped."""
    out: list[pathlib.Path] = []
    stack = [root]
    while stack:
        d = stack.pop()
        for p in sorted(d.iterdir()):
            if any(part in p.relative_to(root).parts for part in skip_parts):
                continue
            if p.is_symlink():
                skipped_links.append(str(p))
                continue
            if p.is_dir():
                stack.append(p)
            elif p.is_file() and p.suffix in exts:
                out.append(p)
    return sorted(out)


def build_manifest(
    *,
    complete: bool,
    failed_executions: int,
    bca: dict,
    verbatim: dict | None,
    views: list[str],
    roots: dict[str, str],
    metrics_options: dict[str, list[str]],
    cli_cmds: dict[str, list[str]],
    populations: dict[str, int],
    keys: list[str],
    inputs_sha256: dict[str, str],
    grammar_pins: dict[str, str],
    skipped_symlinks: list[str],
) -> dict:
    """A capture manifest. Keyword-only so a writer or a test cannot
    transpose two same-typed fields, and the one place the shape is
    spelled."""
    return {
        "version": MANIFEST_VERSION,
        "complete": complete,
        "failed_executions": failed_executions,
        "bca": bca,
        "verbatim": verbatim,
        "views": list(views),
        "roots": roots,
        "metrics_options": metrics_options,
        "cli_cmds": cli_cmds,
        "outputs_per_file": len(views),
        "populations": populations,
        "keys": sorted(keys),
        "inputs_sha256": inputs_sha256,
        "grammar_pins": grammar_pins,
        "skipped_symlinks": skipped_symlinks,
    }
