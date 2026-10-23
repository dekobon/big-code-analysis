#!/usr/bin/env python3
"""role_tables — the one reader of ``role_table!`` declarations.

The three role gates (``check-role-census.py``, ``check-role-coverage.py``,
``check-role-bypass.py``) all need the same two things from the tree:
which languages exist, and what each language's ``roles/<lang>.rs``
declares for each role. This module is the single implementation of
both, so the gates cannot disagree about what a row says.

It reads the declaration *text*, not the compiled crate: a lint gate
must not require a Rust build. That makes it a second implementation of
the DSL's meaning, which is kept honest two ways — ``--dump`` renders a
table in a canonical text form that a Rust test in ``roles/mod.rs``
compares against the compiled ``OWNERS`` / ``PAYERS`` / ``WRAPS`` /
``EXCLUDED`` / ``ABSENT`` / ``PENDING`` constants, and the reader fails
closed: any row shape it does not recognise is an error, never a row it
silently reads as empty.

Comments and literals are classified with the Rust lexer in
``check-test-lang-gates.py`` (``dead_spans``), then the structural scan
runs over the masked text while reasons and field names are recovered
from the original by span, so a ``}`` inside an ``absent`` reason cannot
end a row.

Reads are anchored at the repository root resolved from this file's
location, so every caller runs correctly from anywhere.
"""

from __future__ import annotations

import argparse
import importlib.util
import pathlib
import re
import sys
from dataclasses import dataclass, field

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
ROLES_DIR = REPO_ROOT / "big-code-analysis-ast" / "src" / "roles"
LANGS_RS = REPO_ROOT / "big-code-analysis-ast" / "src" / "langs.rs"
LANGUAGES_DIR = REPO_ROOT / "big-code-analysis-ast" / "src" / "languages"


def _load_lexer():
    """The Rust comment/literal lexer from the test-lang-gates gate."""
    src = UTILS_DIR / "check-test-lang-gates.py"
    name = "check_test_lang_gates"
    if name in sys.modules:
        return sys.modules[name]
    spec = importlib.util.spec_from_file_location(name, src)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    # Registered before execution so the module's own dataclasses can
    # resolve their annotations through `sys.modules`.
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


_LEXER = None


def dead_spans(source: str) -> list[tuple[int, int]]:
    """Index ranges of comments and string / char literals in ``source``."""
    global _LEXER
    if _LEXER is None:
        _LEXER = _load_lexer()
    return _LEXER.dead_spans(source)


def literal_spans(source: str, *, spans: list[tuple[int, int]] | None = None) -> list[tuple[int, int]]:
    """The string-literal spans among ``dead_spans``: ``"…"``, ``r"…"``,
    ``r#"…"#`` and the byte forms — never a comment, however many quotes
    it holds. Pass ``spans`` to reuse an already-lexed result."""
    out = []
    for start, end in (spans if spans is not None else dead_spans(source)):
        # A string span opens with `"` after an optional `b`, `r`, `br`
        # and any number of `#`; a comment span opens with `/`.
        if re.match(r'b?r?#*"', source[start:end]):
            out.append((start, end))
    return out


def decode_literal(lit: str) -> str:
    """The value of one Rust string literal (plain or raw), decoding the
    escapes the role tables use. An escape this does not decode is an
    error rather than a silently different string."""
    m = re.fullmatch(r'b?r(#*)"(.*)"\1', lit, re.S)
    if m:
        return m.group(2)
    m = re.fullmatch(r'b?"(.*)"', lit, re.S)
    if not m:
        raise RoleTableError(f"unreadable string literal {lit[:30]!r}")
    body = m.group(1)
    out = []
    i = 0
    while i < len(body):
        ch = body[i]
        if ch != "\\":
            out.append(ch)
            i += 1
            continue
        nxt = body[i + 1] if i + 1 < len(body) else ""
        simple = {'"': '"', "\\": "\\", "n": "\n", "t": "\t", "r": "\r", "0": "\0", "'": "'"}
        if nxt in simple:
            out.append(simple[nxt])
            i += 2
        elif nxt == "\n":
            # line continuation: skip the newline and leading whitespace
            i += 2
            while i < len(body) and body[i] in " \t":
                i += 1
        else:
            raise RoleTableError(f"unsupported escape `\\{nxt}` in a role-table string")
    return "".join(out)


def mask_dead(
    source: str, *, comments_only: bool = False, spans: list[tuple[int, int]] | None = None
) -> str:
    """``source`` with comments and literals blanked, offsets preserved.
    With ``comments_only`` the literals stay, for a scan that reads
    them. Pass ``spans`` to reuse an already-lexed result."""
    chars = list(source)
    for start, end in (spans if spans is not None else dead_spans(source)):
        if comments_only and not source.startswith(("//", "/*"), start):
            continue
        for i in range(start, min(end, len(chars))):
            if chars[i] != "\n":
                chars[i] = " "
    return "".join(chars)


class RoleTableError(Exception):
    """A declaration the reader refuses to interpret. The message names
    the file, line and the shape it found."""


# ---------------------------------------------------------------------------
# The fixed role set and the language table
# ---------------------------------------------------------------------------

NAMED_RE = re.compile(r"\(@named\s+([A-Z][A-Za-z]*)\s*\{")


def role_set(mod_rs: pathlib.Path = ROLES_DIR / "mod.rs") -> list[str]:
    """The roles ``role_table!`` accepts, in the order its ``@named``
    mapping lists them. The mapping is what fixes the set: a role not
    listed there is a compile error."""
    roles = NAMED_RE.findall(mask_dead(mod_rs.read_text(encoding="utf-8")))
    if not roles:
        raise RoleTableError(f"{mod_rs}: no `@named` role mapping found")
    if len(set(roles)) != len(roles):
        raise RoleTableError(f"{mod_rs}: a role is mapped twice in `@named`")
    return roles


# One `mk_langs!` row, read from its masked text with the feature
# literal recovered from the source: the row's first two fields, then
# the grammar crate's Rust path somewhere after them. Tsx and Typescript
# both name `tree_sitter_typescript`'s crate under different `_tsx` /
# `_typescript` module paths; the crate is what the pin names.
LANG_ROW_HEAD_RE = re.compile(r"\s*,\s*(?P<variant>[A-Z][A-Za-z0-9]*)\s*,")
LANG_ROW_GRAMMAR_RE = re.compile(r"\b(?P<grammar>(?:dekobon_)?tree_sitter_[a-z_]+)\b")


@dataclass(frozen=True)
class Language:
    """One ``LANG`` variant: its CamelCase name, Cargo feature, the
    lowercase stem its ``roles/<stem>.rs`` and ``language_<stem>.rs``
    use, and the grammar crate it parses with (as a Rust path, e.g.
    ``tree_sitter_java``; ``tree_sitter_tsx`` is the TypeScript crate)."""

    variant: str
    feature: str
    grammar: str
    # The file extensions the row lists, in order; empty for a variant
    # that owns none (Mozcpp, Mozjs, the helper grammars).
    extensions: tuple[str, ...] = ()

    @property
    def stem(self) -> str:
        return self.variant.lower()

    @property
    def crate(self) -> str:
        """The Cargo package name the pin in ``Cargo.toml`` uses."""
        name = self.grammar.replace("_", "-")
        if name == "tree-sitter-tsx":
            name = "tree-sitter-typescript"
        if name == "tree-sitter-c-sharp" or name == "tree-sitter-c_sharp":
            name = "tree-sitter-c-sharp"
        return name


def languages(langs_rs: pathlib.Path = LANGS_RS) -> list[Language]:
    """Every ``LANG`` variant from the ``mk_langs!`` table, in table
    order. The invocation is read as balanced bracket groups, one row
    per top-level parenthesis, and every row must be readable: a row the
    reader cannot interpret is an error, never a language that silently
    drops out of all three gates."""
    source = langs_rs.read_text(encoding="utf-8")
    masked = mask_dead(source)
    start = masked.find("mk_langs!")
    if start == -1:
        raise RoleTableError(f"{langs_rs}: no `mk_langs!` invocation found")
    open_idx = masked.index("(", start)
    close_idx = _balanced(masked, open_idx)
    body_start, body_end = open_idx + 1, close_idx - 1
    rows: list[Language] = []
    all_literals = literal_spans(source)
    pos = body_start
    while True:
        nxt = masked.find("(", pos, body_end)
        if nxt == -1:
            if masked[pos:body_end].replace(",", "").strip():
                raise RoleTableError(f"{langs_rs}:{_line_of(source, pos)}: unreadable text in `mk_langs!`")
            break
        if masked[pos:nxt].replace(",", "").strip():
            raise RoleTableError(f"{langs_rs}:{_line_of(source, pos)}: unreadable text before a `mk_langs!` row")
        row_end = _balanced(masked, nxt)
        row_masked = masked[nxt + 1 : row_end - 1]
        line = _line_of(source, nxt)
        # field 1: the feature literal, blanked in the mask
        lits = [(a, b) for a, b in all_literals if nxt < a < row_end]
        if not lits:
            raise RoleTableError(f"{langs_rs}:{line}: `mk_langs!` row has no feature literal")
        feature = decode_literal(source[lits[0][0] : lits[0][1]])
        after_feature = lits[0][1] - (nxt + 1)
        hm = LANG_ROW_HEAD_RE.match(row_masked, after_feature)
        if not hm:
            raise RoleTableError(f"{langs_rs}:{line}: `mk_langs!` row has no variant after its feature")
        gm = LANG_ROW_GRAMMAR_RE.search(row_masked, hm.end())
        if not gm:
            raise RoleTableError(f"{langs_rs}:{line}: `mk_langs!` row names no `tree_sitter_*` grammar")
        # The extension list is the first `[ … ]` after the grammar.
        ext_open = row_masked.find("[", gm.end())
        if ext_open == -1:
            raise RoleTableError(f"{langs_rs}:{line}: `mk_langs!` row has no `[extensions]` list")
        ext_close = _balanced(row_masked, ext_open)
        exts = tuple(e.strip() for e in row_masked[ext_open + 1 : ext_close - 1].split(",") if e.strip())
        rows.append(Language(hm.group("variant"), feature, gm.group("grammar"), exts))
        pos = row_end
    if not rows:
        raise RoleTableError(f"{langs_rs}: `mk_langs!` lists no languages")
    if len({r.variant for r in rows}) != len(rows):
        raise RoleTableError(f"{langs_rs}: a `LANG` variant is listed twice")
    return rows


# ---------------------------------------------------------------------------
# Rows
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Owner:
    kind: str
    gate: str | None = None
    text_gated: bool = False
    pays_itself: bool = False
    opens: str | None = None


@dataclass(frozen=True)
class Payer:
    kind: str
    gate: str | None = None


@dataclass
class Row:
    """One role's declaration in one table."""

    role: str
    line: int
    status: str  # "rows" | "absent" | "pending"
    reason: str | None = None  # absent
    package: str | None = None  # pending
    owners: list[Owner] = field(default_factory=list)
    pays_mode: str = "owners"  # "owners" | "split" | "never"
    payers: list[Payer] = field(default_factory=list)
    project: str | None = None
    wraps: list[str] = field(default_factory=list)
    excludes: list[tuple[str, str]] = field(default_factory=list)

    def raw_kinds(self) -> list[str]:
        """Every kind the row names as owning, paying or wrapping — the
        set the coverage ratchet subtracts (not the excluded ones)."""
        return (
            [o.kind for o in self.owners]
            + [p.kind for p in self.payers]
            + list(self.wraps)
        )


@dataclass
class Table:
    """One ``roles/<lang>.rs``."""

    path: pathlib.Path
    lang: str
    rows: dict[str, Row]


def _line_of(source: str, idx: int) -> int:
    return source.count("\n", 0, idx) + 1


_PAIRS = {"{": "}", "[": "]", "(": ")"}
_OPEN = frozenset(_PAIRS)
_CLOSE = frozenset(_PAIRS.values())


def _balanced(masked: str, open_idx: int) -> int:
    """Index just past the bracket group opening at ``open_idx``."""
    close = _PAIRS[masked[open_idx]]
    depth = 0
    for i in range(open_idx, len(masked)):
        ch = masked[i]
        if ch in _OPEN:
            depth += 1
        elif ch in _CLOSE:
            depth -= 1
            if depth == 0:
                if ch != close:
                    raise RoleTableError(f"mismatched bracket at index {i}")
                return i + 1
    raise RoleTableError(f"unclosed `{masked[open_idx]}` at index {open_idx}")


def _literals_between(source: str, spans: list[tuple[int, int]], start: int, end: int) -> list[str]:
    """The decoded string literals whose spans lie inside
    ``[start, end)`` — from the classified spans, so a quoted word in a
    comment there is not one."""
    return [decode_literal(source[a:b]) for a, b in spans if start <= a and b <= end]


def _string_at(source: str, spans: list[tuple[int, int]], start: int, end: int) -> str:
    """The one string literal between ``start`` and ``end``."""
    found = _literals_between(source, spans, start, end)
    if len(found) != 1:
        raise RoleTableError(
            f"expected exactly one string literal, found {len(found)} at line {_line_of(source, start)}"
        )
    return found[0]


IDENT = r"[A-Za-z_][A-Za-z0-9_]*"
ROW_HEAD_RE = re.compile(rf"\b([A-Z][A-Za-z]*)\s*\{{")
LANG_RE = re.compile(rf"lang\s*:\s*({IDENT})\s*;")
OWNER_RE = re.compile(rf"({IDENT})\s*(\(([^()]*)\))?")
PAYER_RE = re.compile(rf"({IDENT})(?:\s+gated_by\s+({IDENT}))?\s*$")


def _parse_owner(entry: str, line: int, path: pathlib.Path) -> Owner:
    m = OWNER_RE.fullmatch(entry.strip())
    if not m:
        raise RoleTableError(f"{path}:{line}: unreadable owner `{entry.strip()}`")
    kind = m.group(1)
    gate = None
    text_gated = False
    pays_itself = False
    opens = None
    attrs = (m.group(3) or "").split()
    i = 0
    while i < len(attrs):
        a = attrs[i]
        if a in ("gated_by", "text_gated_by"):
            if gate is not None or i + 1 >= len(attrs):
                raise RoleTableError(f"{path}:{line}: `{kind}` has a malformed or repeated gate")
            gate = attrs[i + 1]
            text_gated = a == "text_gated_by"
            i += 2
        elif a == "pays_itself":
            if pays_itself:
                raise RoleTableError(f"{path}:{line}: `{kind}` repeats `pays_itself`")
            pays_itself = True
            i += 1
        elif a == "opens":
            if opens is not None or i + 1 >= len(attrs):
                raise RoleTableError(f"{path}:{line}: `{kind}` has a malformed or repeated `opens`")
            opens = attrs[i + 1]
            i += 2
        else:
            raise RoleTableError(f"{path}:{line}: unknown owner attribute `{a}` on `{kind}`")
    return Owner(kind, gate, text_gated, pays_itself, opens)


def _split_list(text: str) -> list[str]:
    """Comma-separated entries of a bracket list body, respecting
    parentheses, with a trailing comma allowed."""
    out: list[str] = []
    depth = 0
    cur = []
    for ch in text:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch == "," and depth == 0:
            out.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    tail = "".join(cur)
    if tail.strip():
        out.append(tail)
    return [e for e in out if e.strip()]


FRAGMENT_RE = re.compile(r"macro_rules!\s*(" + IDENT + r")\s*\{")
# A fragment's one arm, carrying its kind list in the accumulator append.
FRAGMENT_ARM_RE = re.compile(
    r"\(@splice\s+\$cont:tt\s+\[\$\(\$acc:tt\)\*\]\s+\$\(\$rest:tt\)\*\)\s*=>\s*\{\s*"
    r"\$crate::role_table!\(\s*@owners\s+\$cont\s+\[\s*\$\(\$acc\)\*\s*(.*?)\]",
    re.S,
)
# Rust admits whitespace inside the attribute (`#[cfg ( test )]`); the
# predicate must be exactly `test`, so `cfg(any(test, …))` is not one.
TEST_MOD_RE = re.compile(
    r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+"
    + IDENT
    + r"\s*\{"
)

# ``name → (entries, file, line)`` for every shared kind fragment.
Fragments = dict[str, tuple[list[str], pathlib.Path, int]]


def _test_module_spans(masked: str) -> list[tuple[int, int]]:
    """``#[cfg(test)] mod … { … }`` extents: what is defined there does
    not exist for a production row."""
    return [(m.start(), _balanced(masked, m.end() - 1)) for m in TEST_MOD_RE.finditer(masked)]


def fragments(paths: list[pathlib.Path]) -> Fragments:
    """Every shared kind fragment in ``paths``: a `macro_rules!` with a
    `@splice` arm, read from that arm's accumulator append — the one
    place a fragment spells its kinds. One inside a ``#[cfg(test)]``
    module is skipped: rustc would reject a production row that spliced
    it, and the reader must not accept what the compiler does not."""
    out: Fragments = {}
    for path in paths:
        source = path.read_text(encoding="utf-8")
        masked = mask_dead(source)
        test_spans = _test_module_spans(masked)
        for m in FRAGMENT_RE.finditer(masked):
            if any(s <= m.start() < e for s, e in test_spans):
                continue
            body = masked[m.end() : _balanced(masked, m.end() - 1)]
            am = FRAGMENT_ARM_RE.search(body)
            if not am:
                continue
            name = m.group(1)
            if name in out:
                raise RoleTableError(
                    f"{path}: fragment `{name}` is defined twice (first in {out[name][1]})"
                )
            out[name] = (_split_list(am.group(1)), path, _line_of(source, m.start()))
    return out


def _expand_fragments(entries: list[str], frags: Fragments, line: int, path: pathlib.Path) -> list[str]:
    """``..name!()`` entries replaced by the fragment's own entries."""
    out: list[str] = []
    for e in entries:
        fm = re.fullmatch(r"\.\.(" + IDENT + r")!\(\)", e.strip())
        if not fm:
            out.append(e)
            continue
        name = fm.group(1)
        if name not in frags:
            raise RoleTableError(
                f"{path}:{line}: `..{name}!()` names no fragment the reader knows; fragments are "
                "`macro_rules!` with an `@splice` arm in the roles directory"
            )
        out.extend(frags[name][0])
    return out


def _parse_row_body(
    role: str,
    source: str,
    masked: str,
    spans: list[tuple[int, int]],
    body_start: int,
    body_end: int,
    path: pathlib.Path,
    frags: Fragments,
) -> Row:
    line = _line_of(source, body_start)
    body = masked[body_start:body_end]
    stripped = body.strip()
    row = Row(role=role, line=line, status="rows")

    # absent / pending: the whole body is one keyword and its argument
    # (the string reason is blanked in `stripped`, so only the keyword
    # survives the mask).
    if stripped.startswith("absent"):
        if not re.fullmatch(r"absent\s+", stripped) and stripped != "absent":
            raise RoleTableError(f"{path}:{line}: `{role}`: `absent` takes one string reason")
        row.status = "absent"
        row.reason = _string_at(source, spans, body_start, body_end)
        return row
    m = re.fullmatch(rf"pending\s+({IDENT})", stripped)
    if stripped.startswith("pending"):
        if not m:
            raise RoleTableError(f"{path}:{line}: `{role}`: `pending` takes one package identifier")
        row.status = "pending"
        row.package = m.group(1)
        return row

    # rows: owns [..] (pays never | pays [..])? (project "..")? (wraps [..])? (excludes [..])?
    pos = 0
    def expect_list(keyword: str) -> tuple[str, int, int] | None:
        nonlocal pos
        m = re.match(rf"\s*{keyword}\s*\[", body[pos:])
        if not m:
            return None
        open_idx = pos + m.end() - 1
        close_idx = _balanced(body, open_idx)
        inner = body[open_idx + 1 : close_idx - 1]
        abs_open, abs_close = body_start + open_idx + 1, body_start + close_idx - 1
        pos = close_idx
        return inner, abs_open, abs_close

    owns = expect_list("owns")
    if owns is None:
        raise RoleTableError(
            f"{path}:{line}: `{role}`: expected `owns [..]`, `absent \"..\"` or `pending Rnn`"
        )
    row.owners = [
        _parse_owner(e, line, path)
        for e in _expand_fragments(_split_list(owns[0]), frags, line, path)
    ]
    if re.match(r"\s*pays\s+never\b", body[pos:]):
        pos += re.match(r"\s*pays\s+never\b", body[pos:]).end()
        row.pays_mode = "never"
    else:
        pays = expect_list("pays")
        if pays is not None:
            row.pays_mode = "split"
            for e in _split_list(pays[0]):
                pm = PAYER_RE.fullmatch(e.strip())
                if not pm:
                    raise RoleTableError(f"{path}:{line}: `{role}`: unreadable payer `{e.strip()}`")
                row.payers.append(Payer(pm.group(1), pm.group(2)))
    pm = re.match(r"\s*project\s+", body[pos:])
    if pm:
        # The field name is a string literal the mask blanked; it runs
        # from the keyword to the next attribute keyword or the row's end.
        lit_start = body_start + pos + pm.start()
        nm = re.compile(r"\b(wraps|excludes)\b").search(masked, lit_start + pm.end() - pm.start(), body_end)
        lit_end = nm.start() if nm else body_end
        row.project = _string_at(source, spans, lit_start, lit_end)
        pos = lit_end - body_start
    wraps = expect_list("wraps")
    if wraps is not None:
        row.wraps = [e.strip() for e in _split_list(wraps[0])]
        for w in row.wraps:
            if not re.fullmatch(IDENT, w):
                raise RoleTableError(f"{path}:{line}: `{role}`: unreadable wraps entry `{w}`")
    excludes = expect_list("excludes")
    if excludes is not None:
        inner, abs_open, abs_close = excludes
        # Each entry is `Kind "reason"`; the reason is blanked in `inner`,
        # so pair kinds (from the mask) with string literals (from source)
        # in order.
        kinds = re.findall(IDENT, inner)
        reasons = _literals_between(source, spans, abs_open, abs_close)
        if len(kinds) != len(reasons):
            raise RoleTableError(
                f"{path}:{line}: `{role}`: every excluded kind needs one string reason"
            )
        row.excludes = list(zip(kinds, reasons))
    if body[pos:].strip():
        raise RoleTableError(
            f"{path}:{line}: `{role}`: unexpected `{body[pos:].strip()[:40]}` after the row's attributes"
        )
    return row


def parse_table(path: pathlib.Path, frags: Fragments | None = None) -> Table:
    """The one ``role_table!`` invocation in ``path``. ``frags`` are the
    shared kind fragments its rows may splice; by default every
    fragment defined in the file's own directory."""
    if frags is None:
        frags = fragments(sorted(path.parent.glob("*.rs")))
    source = path.read_text(encoding="utf-8")
    masked = mask_dead(source)
    spans = literal_spans(source)
    inv = masked.find("role_table!")
    if inv == -1:
        raise RoleTableError(f"{path}: no `role_table!` invocation")
    if masked.find("role_table!", inv + 1) != -1:
        raise RoleTableError(f"{path}: more than one `role_table!` invocation; one table per language")
    open_idx = masked.index("{", inv)
    close_idx = _balanced(masked, open_idx)
    inner_start, inner_end = open_idx + 1, close_idx - 1
    lm = LANG_RE.search(masked, inner_start, inner_end)
    if not lm:
        raise RoleTableError(f"{path}: table does not open with `lang: X;`")
    lang = lm.group(1)
    rows: dict[str, Row] = {}
    pos = lm.end()
    while True:
        hm = ROW_HEAD_RE.search(masked, pos, inner_end)
        if not hm:
            if masked[pos:inner_end].strip():
                raise RoleTableError(
                    f"{path}:{_line_of(source, pos)}: unreadable text `{masked[pos:inner_end].strip()[:40]}`"
                )
            break
        if masked[pos : hm.start()].strip():
            raise RoleTableError(
                f"{path}:{_line_of(source, pos)}: unreadable text `{masked[pos:hm.start()].strip()[:40]}` before `{hm.group(1)}`"
            )
        role = hm.group(1)
        body_open = hm.end() - 1
        body_close = _balanced(masked, body_open)
        row = _parse_row_body(role, source, masked, spans, body_open + 1, body_close - 1, path, frags)
        if role in rows:
            raise RoleTableError(
                f"{path}:{row.line}: `{role}` declared twice (first at line {rows[role].line})"
            )
        rows[role] = row
        pos = body_close
    return Table(path=path, lang=lang, rows=rows)


def load_tables(roles_dir: pathlib.Path = ROLES_DIR, langs: list[Language] | None = None) -> dict[str, Table | None]:
    """Every language's table keyed by the file's language; ``None``
    where the file is missing. Whether the table's ``lang:`` agrees with
    its file name is the census's check, reported with the rest of its
    diagnostics rather than raised here."""
    out: dict[str, Table | None] = {}
    frags = fragments(sorted(roles_dir.glob("*.rs")))
    for lang in langs if langs is not None else languages():
        path = roles_dir / f"{lang.stem}.rs"
        out[lang.variant] = parse_table(path, frags) if path.exists() else None
    return out


# ---------------------------------------------------------------------------
# Grammar pins: which symbol table the ids belong to
# ---------------------------------------------------------------------------

PIN_RE = re.compile(r'^(?P<name>[A-Za-z0-9_-]+)\s*=\s*(?:"=(?P<ver>[^"]+)"|\{[^}]*?version\s*=\s*"=(?P<ver2>[^"]+)"[^}]*\})', re.M)


def grammar_pins(cargo_toml: pathlib.Path = REPO_ROOT / "Cargo.toml") -> dict[str, str]:
    """``crate name → pinned version`` for every ``tree-sitter*`` entry
    in the workspace manifest. A vendored grammar is pinned through its
    `{ package = …, path = …, version = "=X" }` table; the key is the
    dependency name the manifest uses (``tree-sitter-mozcpp``), which is
    what ``mk_langs!`` names too."""
    text = cargo_toml.read_text(encoding="utf-8")
    pins: dict[str, str] = {}
    for m in PIN_RE.finditer(text):
        name = m.group("name")
        if "tree-sitter" not in name:
            continue
        pins[name] = m.group("ver") or m.group("ver2")
    if not pins:
        raise RoleTableError(f"{cargo_toml}: no `tree-sitter*` pins found")
    return pins


# ---------------------------------------------------------------------------
# Enums: the id table
# ---------------------------------------------------------------------------

VARIANT_RE = re.compile(rf"^\s+({IDENT})\s*=\s*(\d+),\s*$", re.M)
# rustfmt wraps a long arm as `=> {\n "…"\n }`, so the block is optional.
KIND_STR_RE = re.compile(rf"({IDENT})::({IDENT})\s*=>\s*\{{?\s*(\"(?:[^\"\\]|\\.)*\")")
FLAGS_RE = re.compile(r"\((\d+),\s*(true|false),\s*(true|false)\)")


@dataclass(frozen=True)
class Kind:
    id: int
    variant: str
    kind: str
    named: bool
    visible: bool


def parse_enum(path: pathlib.Path) -> tuple[str, list[Kind], int]:
    """``(EnumName, kinds in id order, error_sentinel_id)`` from a
    generated ``language_<lang>.rs``. The sentinel is the generator's
    appended ``ERROR`` variant, which is not a runtime kind and has no
    flags entry."""
    source = path.read_text(encoding="utf-8")
    masked = mask_dead(source)
    em = re.search(rf"pub enum ({IDENT}) \{{", masked)
    if not em:
        raise RoleTableError(f"{path}: no generated enum")
    name = em.group(1)
    ids = {int(v): n for n, v in VARIANT_RE.findall(masked[em.end() : _balanced(masked, em.end() - 1)])}
    strings = {}
    for enum, variant, lit in KIND_STR_RE.findall(source):
        if enum == name:
            strings[variant] = decode_literal(lit)
    fm = re.search(rf"pub const {name.upper()}_KIND_FLAGS: &\[\(u16, bool, bool\)\] = &\[", masked)
    if not fm:
        raise RoleTableError(f"{path}: no `{name.upper()}_KIND_FLAGS` sidecar; regenerate with `enums/`")
    flags_end = _balanced(masked, fm.end() - 1)
    flags = {int(i): (n == "true", v == "true") for i, n, v in FLAGS_RE.findall(masked[fm.end() : flags_end])}
    sentinel = max(ids)
    if strings.get(ids[sentinel]) != "ERROR":
        raise RoleTableError(f"{path}: last variant is not the ERROR sentinel")
    if sentinel in flags:
        raise RoleTableError(f"{path}: the ERROR sentinel has a flags entry; it is not a runtime kind")
    kinds = []
    for i in sorted(ids):
        if i == sentinel:
            continue
        if i not in flags:
            raise RoleTableError(f"{path}: kind id {i} has no flags entry; regenerate with `enums/`")
        kinds.append(Kind(i, ids[i], strings[ids[i]], flags[i][0], flags[i][1]))
    return name, kinds, sentinel


# ---------------------------------------------------------------------------
# Canonical dump (the Rust differential test's oracle)
# ---------------------------------------------------------------------------


def dump_table(table: Table, enum_path: pathlib.Path) -> str:
    """One line per declared row, in declaration order, kinds rendered
    as ids so the Rust side can compare against its ``u16`` constants
    without naming variants."""
    _, kinds, _ = parse_enum(enum_path)
    by_variant = {k.variant: k.id for k in kinds}
    lines = [f"lang {table.lang}"]
    for role, row in table.rows.items():
        if row.status == "absent":
            lines.append(f"{role} absent {row.reason}")
        elif row.status == "pending":
            lines.append(f"{role} pending {row.package}")
        else:
            def ids(names: list[str]) -> str:
                return ",".join(str(by_variant[n]) for n in names)
            owners = ids([o.kind for o in row.owners])
            payers = ids([p.kind for p in row.payers])
            wraps = ids(row.wraps)
            excluded = ";".join(f"{by_variant[k]}={r}" for k, r in row.excludes)
            never = "true" if row.pays_mode == "never" else "false"
            lines.append(
                f"{role} rows owners=[{owners}] payers=[{payers}] wraps=[{wraps}] "
                f"excluded=[{excluded}] pays_never={never}"
            )
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--dump", type=pathlib.Path, metavar="ROLES_RS", help="print a table canonically")
    args = ap.parse_args(argv)
    if args.dump:
        table = parse_table(args.dump)
        enum_path = LANGUAGES_DIR / f"language_{table.lang.lower()}.rs"
        # Bytes, UTF-8, LF: the Rust differential test compares this
        # against `canonical()` byte for byte, and a text stream would
        # write CRLF on Windows.
        sys.stdout.buffer.write(dump_table(table, enum_path).encode("utf-8"))
        return 0
    ap.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
