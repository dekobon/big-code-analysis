#!/usr/bin/env python3
"""check-role-census

The per-(language, role) census of the role tables (#1692).

For every ``LANG`` variant and every role ``role_table!`` accepts, the
language's ``roles/<lang>.rs`` must carry exactly one of: declared rows,
``absent "reason"``, or ``pending Rnn`` naming the work package that
will declare it. A missing file, a missing cell, or a cell the reader
cannot interpret fails with the file, language and role to fix.

The ``pending`` cells are a ratchet. ``.role-census-baseline.txt``
records the exact set of ``(language, role, package)`` cells that are
pending; a current pending cell not in that set fails, so a declared
row cannot quietly go back to pending and a new language cannot add
pending cells without the baseline recording them. ``--update`` only
removes entries: it refuses, non-zero, when the current set holds a
cell the baseline does not, printing which. ``--bootstrap`` writes the
initial file and refuses when one exists or ever existed at ``HEAD``.
The set must be empty when #1701 closes the umbrella.

What this gate cannot see, printed with every run: whether a declared
row's membership is right, whether an ``absent`` reason is true of the
language, or whether a pending cell is owned by the package that will
in fact declare it. It counts declarations; the parity suites and
review judge them.

Reads and writes are anchored at the repository root resolved from this
file's location, so the gate runs correctly from anywhere.
"""

from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
sys.path.insert(0, str(UTILS_DIR))

import role_tables as rt  # noqa: E402

DEFAULT_BASELINE = REPO_ROOT / ".role-census-baseline.txt"

SCOPE_NOTE = (
    "role-census: counts declarations per (language, role). It does not judge "
    "a row's membership, an `absent` reason, or a `pending` cell's owner."
)

HEADER = """\
# Role-census baseline: the (language, role, package) cells still
# `pending` in big-code-analysis-ast/src/roles/<lang>.rs (#1692).
# Maintained by check-role-census.py; see AGENTS.md "Validation gates".
#
# An entry is one cell a later work package will declare. The gate
# fails when a current pending cell is not listed here, so a row cannot
# go back to pending and a new language cannot add pending cells
# unrecorded. `--update` only removes entries. The file must be empty
# when #1701 closes the umbrella.
#
# What the census cannot see: whether a declared row's membership is
# right, whether an `absent` reason is true of the language, or whether
# a pending cell names the package that will in fact declare it.
#
# One entry per line: <language> <role> <package>, sorted.
"""


def census(
    roles_dir: pathlib.Path = rt.ROLES_DIR,
    langs_rs: pathlib.Path = rt.LANGS_RS,
    mod_rs: pathlib.Path | None = None,
) -> tuple[dict[str, dict[str, rt.Row | None]], list[str], list[str]]:
    """``(matrix, roles, errors)``: ``matrix[lang][role]`` is the row or
    ``None`` for a hole; ``errors`` lists every hole and reader failure
    as an actionable line."""
    roles = rt.role_set(mod_rs) if mod_rs else rt.role_set(roles_dir / "mod.rs")
    langs = rt.languages(langs_rs)
    errors: list[str] = []
    matrix: dict[str, dict[str, rt.Row | None]] = {}
    for lang in langs:
        path = roles_dir / f"{lang.stem}.rs"
        cells: dict[str, rt.Row | None] = {r: None for r in roles}
        if not path.exists():
            errors.append(
                f"{lang.variant}: no table — add {path.relative_to(REPO_ROOT) if path.is_relative_to(REPO_ROOT) else path} "
                f"with a `role_table! {{ lang: {lang.variant}; … }}` naming every role"
            )
            matrix[lang.variant] = cells
            continue
        try:
            table = rt.parse_table(path)
        except rt.RoleTableError as exc:
            errors.append(f"{lang.variant}: unreadable table: {exc}")
            matrix[lang.variant] = cells
            continue
        if table.lang != lang.variant:
            errors.append(f"{path}: declares `lang: {table.lang}` but is `{lang.variant}`'s file")
        for role, row in table.rows.items():
            if role not in cells:
                errors.append(f"{path}:{row.line}: `{role}` is not a role; the set is fixed in roles/mod.rs")
                continue
            cells[role] = row
        for role in roles:
            if cells[role] is None:
                errors.append(
                    f"{lang.variant} {role}: no row — add `{role} {{ pending Rnn }}`, "
                    f"`{role} {{ absent \"why\" }}` or declared rows to {path.name}"
                )
        matrix[lang.variant] = cells
    return matrix, roles, errors


def pending_set(matrix: dict[str, dict[str, rt.Row | None]]) -> set[tuple[str, str, str]]:
    return {
        (lang, role, row.package or "?")
        for lang, cells in matrix.items()
        for role, row in cells.items()
        if row is not None and row.status == "pending"
    }


def render_matrix(matrix: dict[str, dict[str, rt.Row | None]], roles: list[str]) -> str:
    """The language × role matrix: `rows`, `absent`, `pend:Rnn` or `HOLE`."""
    width = max(len(r) for r in roles)
    lang_w = max(len(l) for l in matrix)
    lines = [" " * lang_w + "  " + "  ".join(r.ljust(width) for r in roles)]
    for lang, cells in matrix.items():
        marks = []
        for role in roles:
            row = cells[role]
            if row is None:
                mark = "HOLE"
            elif row.status == "rows":
                mark = "rows"
            elif row.status == "absent":
                mark = "absent"
            else:
                mark = f"pend:{row.package}"
            marks.append(mark.ljust(width))
        lines.append(lang.ljust(lang_w) + "  " + "  ".join(marks))
    return "\n".join(lines)


def load_baseline(path: pathlib.Path) -> set[tuple[str, str, str]]:
    entries: set[tuple[str, str, str]] = set()
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) != 3:
            raise rt.RoleTableError(f"{path}:{lineno}: expected `<language> <role> <package>`, got `{line}`")
        entries.add((parts[0], parts[1], parts[2]))
    return entries


def write_baseline(path: pathlib.Path, entries: set[tuple[str, str, str]]) -> None:
    body = "".join(f"{l} {r} {p}\n" for l, r, p in sorted(entries))
    path.write_text(HEADER + body, encoding="utf-8")


def existed_at_head(path: pathlib.Path) -> bool:
    """Whether ``path`` is tracked at ``HEAD`` — a bootstrap must not
    replace a baseline someone deleted."""
    rel = path.relative_to(REPO_ROOT) if path.is_relative_to(REPO_ROOT) else path
    run = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "ls-tree", "--name-only", "HEAD", "--", str(rel)],
        capture_output=True,
        text=True,
        check=False,
    )
    return run.returncode == 0 and bool(run.stdout.strip())


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--baseline", type=pathlib.Path, default=DEFAULT_BASELINE)
    ap.add_argument("--roles-dir", type=pathlib.Path, default=rt.ROLES_DIR)
    ap.add_argument("--langs-rs", type=pathlib.Path, default=rt.LANGS_RS)
    ap.add_argument("--update", action="store_true", help="drop baseline entries no longer pending")
    ap.add_argument("--bootstrap", action="store_true", help="write the initial baseline")
    ap.add_argument("--matrix", action="store_true", help="print the language × role matrix")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args(argv)

    print(SCOPE_NOTE)
    try:
        matrix, roles, errors = census(args.roles_dir, args.langs_rs)
    except rt.RoleTableError as exc:
        print(f"error: {exc}")
        return 2
    if args.matrix or errors:
        print(render_matrix(matrix, roles))
    if errors:
        for e in errors:
            print(f"error: {e}")
        print(f"role-census: FAIL — {len(errors)} problem(s)")
        return 1
    current = pending_set(matrix)

    if args.bootstrap:
        if args.baseline.exists() or existed_at_head(args.baseline):
            print(f"error: {args.baseline} exists or is tracked at HEAD; use --update, never re-bootstrap")
            return 2
        write_baseline(args.baseline, current)
        print(f"role-census: wrote {len(current)} pending cell(s) to {args.baseline}")
        return 0
    if not args.baseline.exists():
        print(f"error: {args.baseline} missing; a first run needs --bootstrap")
        return 2
    try:
        recorded = load_baseline(args.baseline)
    except rt.RoleTableError as exc:
        print(f"error: {exc}")
        return 2

    new = sorted(current - recorded)
    gone = sorted(recorded - current)
    if new:
        for lang, role, pkg in new:
            print(f"error: {lang} {role} is pending({pkg}) but not in the baseline — declare it, or record it deliberately")
        print(f"role-census: FAIL — {len(new)} new pending cell(s); `--update` will not accept them")
        return 1
    if args.update:
        if gone:
            write_baseline(args.baseline, current)
            print(f"role-census: removed {len(gone)} entr{'y' if len(gone) == 1 else 'ies'} from {args.baseline}")
        else:
            print("role-census: baseline already current")
        return 0
    if gone and not args.quiet:
        print(f"note: {len(gone)} baseline entr{'y' if len(gone) == 1 else 'ies'} no longer pending; run --update")
    total = sum(len(c) for c in matrix.values())
    print(f"role-census: OK — {len(matrix)} languages × {len(roles)} roles, {len(current)} pending, {total - len(current)} declared or absent")
    return 0


if __name__ == "__main__":
    sys.exit(main())
