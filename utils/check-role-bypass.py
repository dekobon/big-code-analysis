#!/usr/bin/env python3
"""check-role-bypass

The role-membership bypass lint (#1692).

Inside the *consumer boundary* — the per-language dispatch files and
the dispatcher modules and macros that generate them — a kind that some
row of a language's role table names in ``owns``, ``pays`` or ``wraps``
must not be restated as classification code. A restatement is one of:

- ``Lang::Kind`` inside a ``matches!(…)``;
- ``Lang::Kind`` in a match arm's pattern (before its ``=>``);
- ``kind_id() == Lang::Kind`` (either operand order, ``as u16`` or not);
- ``kind() == "kind string"`` where the string spells a claimed kind;
- ``Lang::Kind`` in a ``const`` or ``static`` array.

Imports are read in lexical scope: a ``use Lang::*`` glob makes the
bare ``Kind`` spellings count inside the block holding it, a ``use …
as X`` alias resolves, and a plain ``use …::Lang;`` is what decides
which language a ``kind() == "…"`` string compare is checked against —
a string with no language in scope is reported as unresolved, never
guessed from its spelling. A match's arm patterns are read whole, over
as many lines and ``|`` alternatives as they span, with a guard
dropped. Macros are the part a text scan would miss, so three bounded
shapes are resolved from their ``macro_rules!`` headers: a macro whose
matcher takes a ``$lang`` / ``$language`` / ``$Lang:ident`` plus
identifier arguments (``impl_simple_is_string!(Java, StringLiteral,
…)``) attributes the identifier arguments to that language; a macro
whose body opens ``use $lang::*`` or spells ``$lang::Kind`` attributes
those kinds to every language it is invoked with, and a helper macro it
invokes inherits them; a ``kind_sets!``-style macro that expands to a
``|``-list of ``$crate::Lang::Kind`` is inlined where it is invoked.
Any other ``macro_rules!`` in the boundary whose body reads
``kind_id()``, ``matches!`` or ``kind()`` and that no language reaches
is reported as *unsupported*, never silently passed. Comments, string
and char literals and ``#[cfg(test)]`` modules are not scanned.

A site is exempt under ``// role-bypass: allow(<Role>) — <reason>`` on
the line above it, where ``<Role>`` is the role whose row claims the
kind; an exemption naming the wrong role is itself a finding. The
exemption covers the next statement or match arm only. Use it for a
site that is not membership — a naming rule, a traversal step — and
leave real duplication to the baseline.

``.role-bypass-baseline.txt`` records the exact set of findings as
``(file, enclosing item, language, kind, shape#n)`` — ``n`` numbering
the sites of one kind and shape within the item — so a removed site
cannot be replaced by another, nor a second added beside the first,
behind an unchanged count. The enclosing item is the innermost brace
block's ``fn`` / ``impl`` / ``macro_rules!``. A current finding not in
the baseline fails; ``--update`` only removes; ``--bootstrap`` writes
the initial file and refuses when one exists or is tracked at ``HEAD``
(a deletion already committed is not seen here — CI's base-branch
comparison is where that would show).

What this lint cannot see, printed with every run: a restatement
outside the boundary; a kind reached through a helper function in
``lang_helpers`` or any other call; a ``kind_id()`` stored in a local
and compared later (``let k = node.kind_id(); … k == Lang::X``), or a
``kind()`` compared to a variable rather than a literal; a macro shape
beyond the three above (reported as unsupported, not missed); and
whether a consumer that reads a row reads the right projection.

Reads and writes are anchored at the repository root resolved from this
file's location, so the gate runs correctly from anywhere.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import subprocess
import sys
from dataclasses import dataclass

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
sys.path.insert(0, str(UTILS_DIR))

import role_tables as rt  # noqa: E402

DEFAULT_BASELINE = REPO_ROOT / ".role-bypass-baseline.txt"

# The consumer boundary (#1692): per-language dispatch files, the
# dispatcher modules that hold their macros, and the shared kind-set
# macros. Paths relative to the repository root.
BOUNDARY = (
    "src/metrics/abc.rs",
    "src/metrics/abc/",
    "src/metrics/cyclomatic.rs",
    "src/metrics/cyclomatic/",
    "src/metrics/cognitive.rs",
    "src/metrics/cognitive/",
    "src/metrics/nexits.rs",
    "src/metrics/npm.rs",
    "src/metrics/npm/",
    "src/metrics/npa.rs",
    "src/metrics/npa/",
    "big-code-analysis-ast/src/checker.rs",
    "big-code-analysis-ast/src/checker/",
    "big-code-analysis-ast/src/getter.rs",
    "big-code-analysis-ast/src/getter/",
    "big-code-analysis-ast/src/macros/kind_sets.rs",
    "big-code-analysis-ast/src/c_langs_macros/",
)

SCOPE_NOTE = (
    "role-bypass: a kind a role row claims must not be restated as classification "
    "inside the consumer boundary. Not checked: files outside it and #[cfg(test)] "
    "modules; a kind reached through a helper function; a kind_id() held in a local "
    "and compared later; `kind()` against a variable; macro shapes beyond the three "
    "resolved ones (those are reported as unsupported); which projection a consumer reads."
)

HEADER = """\
# Role-bypass baseline: role-claimed kinds still restated as
# classification inside the consumer boundary (#1692).
# Maintained by check-role-bypass.py; see AGENTS.md "Validation gates".
#
# An entry is one site a migration package will replace with a call to
# the row. The gate fails on a site not listed here; `--update` only
# removes entries. Sites that are not membership — a naming rule, a
# traversal step — take a `// role-bypass: allow(<Role>) — <reason>`
# exemption instead of an entry.
#
# What the lint cannot see: a restatement outside the boundary or in a
# test module, a kind reached through a helper function, a `kind_id()`
# held in a local and compared later, a `kind()` compared to a
# variable, and which projection a consumer reads. A macro shape it
# does not resolve is reported as unsupported, not missed.
#
# One entry per line, tab-separated and sorted:
#   <file> <item> <language> <kind> <shape>#<n>
# where <n> numbers the sites of one kind and shape within the item.
"""

IDENT = r"[A-Za-z_][A-Za-z0-9_]*"
CAMEL = r"[A-Z][A-Za-z0-9]*"

SHAPES = ("matches", "arm", "kind_id", "kind_str", "const", "macro_arg", "macro_body")


@dataclass(frozen=True)
class Finding:
    file: str
    item: str
    lang: str
    kind: str
    shape: str
    line: int
    # The n-th such site in this item (1-based), so two restatements of
    # one kind in one function are two entries: a second cannot hide
    # behind the first. Assigned after the scan, in source order.
    nth: int = 1

    def key(self) -> tuple[str, str, str, str, str]:
        return (self.file, self.item, self.lang, self.kind, f"{self.shape}#{self.nth}")


@dataclass
class Claims:
    """``(lang, variant) → roles`` and ``(lang, kind string) → variant``."""

    by_variant: dict[tuple[str, str], list[str]]
    by_string: dict[tuple[str, str], str]
    langs: set[str]


def claims(
    roles_dir: pathlib.Path = rt.ROLES_DIR,
    langs_rs: pathlib.Path = rt.LANGS_RS,
    languages_dir: pathlib.Path = rt.LANGUAGES_DIR,
) -> Claims:
    by_variant: dict[tuple[str, str], list[str]] = {}
    by_string: dict[tuple[str, str], str] = {}
    langs = rt.languages(langs_rs)
    tables = rt.load_tables(roles_dir, langs)
    for lang in langs:
        table = tables.get(lang.variant)
        if table is None:
            continue
        strings: dict[str, str] = {}
        enum_path = languages_dir / f"language_{lang.stem}.rs"
        if enum_path.exists():
            _, kinds, _ = rt.parse_enum(enum_path)
            strings = {k.variant: k.kind for k in kinds}
        for role, row in table.rows.items():
            if row.status != "rows":
                continue
            for name in row.raw_kinds():
                by_variant.setdefault((lang.variant, name), []).append(role)
                if name in strings:
                    by_string[(lang.variant, strings[name])] = name
    return Claims(by_variant, by_string, {l.variant for l in langs})


# ---------------------------------------------------------------------------
# Source scanning
# ---------------------------------------------------------------------------


@dataclass
class Source:
    path: pathlib.Path
    rel: str
    text: str
    masked: str  # comments and literals blanked
    dead: list[tuple[int, int]]
    literals: list[tuple[int, int]]

    def line(self, idx: int) -> int:
        return self.text.count("\n", 0, idx) + 1

    def literal_at(self, idx: int) -> str | None:
        """The string literal's decoded value when ``idx`` is inside one
        (plain, raw or byte), or ``None`` for a comment or no literal."""
        for start, end in self.literals:
            if start <= idx < end:
                return rt.decode_literal(self.text[start:end])
        return None


def load_source(path: pathlib.Path) -> Source:
    text = path.read_text(encoding="utf-8")
    dead = rt.dead_spans(text)
    masked = rt.mask_dead(text, spans=dead)
    literals = rt.literal_spans(text, spans=dead)
    rel = path.relative_to(REPO_ROOT).as_posix() if path.is_relative_to(REPO_ROOT) else path.as_posix()
    return Source(path, rel, text, masked, dead, literals)


_OPEN = frozenset("{[(")
_CLOSE = frozenset("}])")


def _balanced(masked: str, open_idx: int) -> int:
    depth = 0
    for i in range(open_idx, len(masked)):
        ch = masked[i]
        if ch in _OPEN:
            depth += 1
        elif ch in _CLOSE:
            depth -= 1
            if depth == 0:
                return i + 1
    return len(masked)


def test_module_spans(src: Source) -> list[tuple[int, int]]:
    """``#[cfg(test)] mod … { … }`` extents, left unscanned."""
    spans = []
    # Other attributes may sit between `#[cfg(test)]` and the `mod`.
    for m in re.finditer(r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+" + IDENT + r"\s*\{", src.masked):
        spans.append((m.start(), _balanced(src.masked, m.end() - 1)))
    return spans


def _in_spans(idx: int, spans: list[tuple[int, int]]) -> bool:
    return any(s <= idx < e for s, e in spans)


ITEM_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:(?:const|unsafe|async)\s+)*(fn|impl|macro_rules!|const|static|mod)\s+(?:<[^>]*>\s*)?([A-Za-z_][A-Za-z0-9_]*)",
    re.M,
)


def item_spans(src: Source) -> list[tuple[int, int, str]]:
    """``(start, end, "fn name")`` for every item whose body is a brace
    block, innermost last, so the enclosing item of an index is the
    innermost span holding it — not merely the nearest header above."""
    out = []
    masked = src.masked
    for m in ITEM_RE.finditer(masked):
        brace = masked.find("{", m.end())
        semi = masked.find(";", m.end())
        if brace == -1 or (semi != -1 and semi < brace and m.group(1) in ("const", "static", "mod")):
            end = semi + 1 if semi != -1 else len(masked)
        else:
            end = _balanced(masked, brace)
        out.append((m.start(), end, f"{m.group(1)} {m.group(2)}"))
    return out


def enclosing_item(spans: list[tuple[int, int, str]], idx: int) -> str:
    best = None
    best_len = None
    for start, end, name in spans:
        if start <= idx < end and (best_len is None or end - start < best_len):
            best, best_len = name, end - start
    return best or "<file>"


GLOB_USE_RE = re.compile(rf"\buse\s+(?:crate::|super::|\$crate::)?(?:{IDENT}::)*({CAMEL})::\*\s*;")
ALIAS_USE_RE = re.compile(rf"\buse\s+(?:crate::|super::)?(?:{IDENT}::)*({CAMEL})\s+as\s+({CAMEL})\s*;")
NAMED_USE_RE = re.compile(rf"\buse\s+(?:crate::|super::)?(?:{IDENT}::)*({CAMEL})\s*;")
# `use crate::{Ancestors, Java, Node};` — the tree's usual spelling; each
# CamelCase entry of the group may be a language.
GROUP_USE_RE = re.compile(rf"\buse\s+(?:crate::|super::)?(?:{IDENT}::)*\{{([^}}]*)\}}\s*;")


@dataclass
class Scope:
    """A `use` and the brace block it is lexically in."""

    start: int
    end: int
    globs: set[str]
    aliases: dict[str, str]
    named: set[str]


def lang_scopes(src: Source, langs: set[str]) -> list[Scope]:
    """Every `use` that names a language, with the block it applies
    to: the innermost `{ … }` holding it, or the whole file for a
    top-level import. A glob in one `fn` does not leak into the next."""
    masked = src.masked
    blocks: list[tuple[int, int]] = []
    stack = []
    for i, ch in enumerate(masked):
        if ch == "{":
            stack.append(i)
        elif ch == "}" and stack:
            blocks.append((stack.pop(), i + 1))

    def block_of(idx: int) -> tuple[int, int]:
        inner = None
        for s_, e in blocks:
            if s_ <= idx < e and (inner is None or e - s_ < inner[1] - inner[0]):
                inner = (s_, e)
        return inner or (0, len(masked))

    scopes: list[Scope] = []
    for m in GROUP_USE_RE.finditer(masked):
        for entry in m.group(1).split(","):
            entry = entry.strip()
            am = re.fullmatch(rf"({CAMEL})\s+as\s+({CAMEL})", entry)
            start, end = block_of(m.start())
            if am and am.group(1) in langs:
                scopes.append(Scope(start, end, set(), {am.group(2): am.group(1)}, set()))
            elif entry in langs:
                scopes.append(Scope(start, end, set(), {}, {entry}))
    for rx, kind in ((GLOB_USE_RE, "glob"), (ALIAS_USE_RE, "alias"), (NAMED_USE_RE, "named")):
        for m in rx.finditer(masked):
            lang = m.group(1)
            if lang not in langs:
                continue
            start, end = block_of(m.start())
            sc = Scope(start, end, set(), {}, set())
            if kind == "glob":
                sc.globs.add(lang)
            elif kind == "alias":
                sc.aliases[m.group(2)] = lang
            else:
                sc.named.add(lang)
            scopes.append(sc)
    return scopes


def scope_at(scopes: list[Scope], idx: int) -> tuple[set[str], dict[str, str], set[str]]:
    """The imports in force at ``idx``. Scopes are applied outermost
    first, so an alias bound in an inner block shadows the same name
    bound outside it, whatever syntax either used."""
    globs: set[str] = set()
    aliases: dict[str, str] = {}
    named: set[str] = set()
    holding = [sc for sc in scopes if sc.start <= idx < sc.end]
    holding.sort(key=lambda sc: sc.end - sc.start, reverse=True)
    for sc in holding:
        globs |= sc.globs
        aliases.update(sc.aliases)
        named |= sc.named
    return globs, aliases, named


# `// role-bypass: allow(Role, Role) — reason`: the reason is required,
# since the marker is the one escape from the gate.
EXEMPT_RE = re.compile(r"//\s*role-bypass:\s*allow\(([A-Za-z, ]+)\)\s*(?:—|-|--)?\s*(\S.*)?$")
EXEMPT_HEAD_RE = re.compile(r"//\s*role-bypass:")


def exemptions(src: Source, problems: list[str]) -> list[tuple[int, int, set[str], int]]:
    """``(start, end, roles, comment_line)`` per exemption: the span it
    covers is the statement, arm or expression that begins on the line
    after the comment, to its closing `;`, `,`-at-depth-0, or the end of
    the bracket group it opens — so a multi-line `matches!` is covered
    whole, and the next statement is not. A comment with no code after
    it covers nothing."""
    out = []
    masked = src.masked
    for start, end in src.dead:
        if not src.text.startswith("//", start):
            continue
        comment = src.text[start:end].rstrip()
        if not EXEMPT_HEAD_RE.search(comment):
            continue
        m = EXEMPT_RE.search(comment)
        if not m or not (m.group(2) or "").strip():
            problems.append(f"{src.rel}:{src.line(start)}: a `role-bypass:` marker must read `allow(<Role>) — <reason>` with a non-empty reason")
            continue
        roles = {r.strip() for r in m.group(1).split(",") if r.strip()}
        # the exemption comment may run on over several `//` lines
        i = end
        while i < len(masked) and masked[i] in " \t\n":
            i += 1
        while src.text.startswith("//", i):
            nl = src.text.find("\n", i)
            i = len(masked) if nl == -1 else nl + 1
            while i < len(masked) and masked[i] in " \t\n":
                i += 1
        span_start = i
        depth = 0
        j = i
        while j < len(masked):
            ch = masked[j]
            if ch in "([{":
                depth += 1
            elif ch in ")]}":
                if depth == 0:
                    break
                depth -= 1
                if depth == 0 and ch == "}":
                    # a brace block closed at the statement's own level
                    # ends it: `if … { … }`, an arm's `=> { … }`
                    j += 1
                    break
            elif ch in ";," and depth == 0:
                j += 1
                break
            elif ch == "\n" and depth == 0 and "=>" in masked[span_start:j]:
                break
            j += 1
        out.append((span_start, j, roles, src.line(start)))
    return out


def _lang_of(path_prefix: str, aliases: dict[str, str], langs: set[str]) -> str | None:
    """Resolve a ``Lang::`` / alias path prefix to a language, else ``None``."""
    if path_prefix:
        name = path_prefix.rstrip(":")
        name = name.rsplit("::", 1)[-1]
        if name in langs:
            return name
        return aliases.get(name)
    return None


@dataclass
class Macro:
    name: str
    lang_arg: int | None  # index of the `$<lang>:ident` matcher argument
    body_globs_lang: bool  # body opens `use $<lang>::*`
    expands_to_kinds: list[str]  # `$crate::Lang::Kind` list for kind_sets-style
    touches_kinds: bool  # body reads kind_id()/matches!/kind()
    inner_calls: list[str]  # other macros the body invokes


MACRO_RE = re.compile(r"macro_rules!\s*(" + IDENT + r")\s*\{")
# The language parameter, under any of the names the tree uses.
LANG_PARAM_RE = re.compile(r"\$(lang|language|Lang)\s*:\s*ident")


def parse_macros(src: Source) -> dict[str, Macro]:
    out: dict[str, Macro] = {}
    for m in MACRO_RE.finditer(src.masked):
        name = m.group(1)
        end = _balanced(src.masked, m.end() - 1)
        body = src.masked[m.end() : end]
        # first matcher: `( … ) =>`
        mm = re.match(r"\s*\(", body)
        lang_arg = None
        if mm:
            matcher_end = _balanced(body, mm.end() - 1)
            matcher = body[mm.end() : matcher_end - 1]
            args = [a.strip() for a in matcher.split(",")]
            for i, a in enumerate(args):
                if LANG_PARAM_RE.match(a):
                    lang_arg = i
        # The body spells the language parameter: a glob, a qualified
        # kind (`$lang::Kind`) or a qualified metavariable
        # (`$lang::$first`). Any of these makes the body's kinds the
        # invoking languages'.
        globs_lang = bool(re.search(r"use\s+\$(?:lang|language|Lang)::\*", body)) or bool(
            re.search(r"\$(?:lang|language|Lang)::(?:" + CAMEL + r"|\$" + IDENT + ")", body)
        )
        expands = re.findall(r"\$crate::(" + CAMEL + r")::(" + CAMEL + r")", body)
        touches = bool(re.search(r"kind_id\(\)|matches!|\.kind\(\)", body))
        inner = sorted({c for c in re.findall(r"\b(" + IDENT + r")!\s*[\(\[{]", body) if c != name})
        out[name] = Macro(name, lang_arg, globs_lang, [f"{a}::{b}" for a, b in expands], touches, inner)
    return out


def macro_languages(macros: dict[str, Macro], invocations: dict[str, set[str]]) -> dict[str, set[str]]:
    """The languages each macro's bare kinds resolve to: its own
    invocations when it opens `use $lang::*`, else the languages of every
    macro that invokes it from inside such a body (transitively), so an
    inner helper like `is_js_func!` is attributed to the JS family."""
    # A macro that spells its language parameter is *resolved* even when
    # no invoking language declares rows yet: its languages are the set
    # it is invoked with, possibly empty. `reached` records that, so the
    # unsupported check tells "no language reaches it" from "its
    # languages have no rows".
    langs: dict[str, set[str]] = {n: set(invocations.get(n, set())) for n, m in macros.items() if m.body_globs_lang}
    changed = True
    while changed:
        changed = False
        for name, mac in macros.items():
            for inner in mac.inner_calls:
                if inner in macros and name in langs:
                    before = len(langs.setdefault(inner, set()))
                    langs[inner] |= langs[name]
                    changed |= len(langs[inner]) != before
    return langs


def scan(src: Source, cl: Claims, macros: dict[str, Macro], mlangs: dict[str, set[str]]) -> tuple[list[Finding], list[str]]:
    """Findings in one file, plus problems (bad exemptions, unsupported macros)."""
    findings: list[Finding] = []
    problems: list[str] = []
    masked = src.masked
    tests = test_module_spans(src)
    scopes = lang_scopes(src, cl.langs)
    items = item_spans(src)
    exempt = exemptions(src, problems)
    macro_spans = {m.start(): _balanced(masked, m.end() - 1) for m in MACRO_RE.finditer(masked)}

    def record(idx: int, lang: str, kind: str, shape: str, item: str | None = None) -> None:
        if _in_spans(idx, tests):
            return
        roles = cl.by_variant.get((lang, kind))
        if not roles:
            return
        line = src.line(idx)
        for span_start, span_end, allowed, comment_line in exempt:
            if span_start <= idx < span_end:
                if allowed & set(roles):
                    return
                problems.append(f"{src.rel}:{comment_line}: exemption names {sorted(allowed)} but `{lang}::{kind}` is claimed by {roles}")
                return
        findings.append(Finding(src.rel, item or enclosing_item(items, idx), lang, kind, shape, line))

    def in_macro_body(idx: int) -> bool:
        return any(start <= idx < end for start, end in macro_spans.items())

    def macro_body_lang(idx: int) -> set[str]:
        """Languages a bare token at ``idx`` inside a macro body resolves
        to (see `macro_languages`)."""
        for start, end in macro_spans.items():
            if start <= idx < end:
                name = MACRO_RE.match(masked, start).group(1)
                return mlangs.get(name, set())
        return set()

    def bare_langs(idx: int) -> set[str]:
        globs, _, _ = scope_at(scopes, idx)
        return globs | macro_body_lang(idx)

    qualified = re.compile(rf"(?:\$crate::|crate::)?((?:{IDENT}::)*)({CAMEL})::({CAMEL})\b")
    macro_qualified = re.compile(rf"\$(?:lang|language|Lang)::({CAMEL})\b")
    bare = re.compile(rf"(?<![:\w$])({CAMEL})\b(?!::|\s*\(|\s*!)")

    def expand_kind_sets(text: str, base: int) -> list[tuple[int, str, str]]:
        """``(idx, lang, kind)`` for every kind a `kind_sets!`-style
        macro invoked in ``text`` expands to, attributed at the
        invocation's index."""
        out = []
        for m in re.finditer(r"\b(" + IDENT + r")!\s*\(\s*\)", text):
            mac = macros.get(m.group(1))
            if mac and mac.expands_to_kinds:
                for lk in mac.expands_to_kinds:
                    lang, kind = lk.split("::")
                    if lang in cl.langs:
                        out.append((base + m.start(), lang, kind))
        return out

    def scan_pattern(text: str, base: int, shape: str) -> None:
        """Every kind a `matches!` body or an arm pattern names."""
        _, aliases, _ = scope_at(scopes, base)
        for q in qualified.finditer(text):
            lang = _lang_of(q.group(1) + q.group(2) + "::", aliases, cl.langs)
            if lang:
                record(base + q.start(), lang, q.group(3), shape)
        for q in macro_qualified.finditer(text):
            # `$lang::Kind` in a macro body: every language it is invoked with
            for lang in macro_body_lang(base + q.start()):
                record(base + q.start(), lang, q.group(1), shape)
        for b in bare.finditer(text):
            idx = base + b.start()
            for lang in bare_langs(idx):
                record(idx, lang, b.group(1), shape)
        for idx, lang, kind in expand_kind_sets(text, base):
            record(idx, lang, kind, shape)

    # 1. matches!( … )
    for m in re.finditer(r"\bmatches!\s*\(", masked):
        end = _balanced(masked, m.end() - 1)
        scan_pattern(masked[m.end() : end - 1], m.end(), "matches")

    # 2. match arms: every `match … { arm, arm }` body, each arm's full
    # pattern up to its top-level `=>` (multi-line alternatives and a
    # guard included — the guard's `if` is dropped before scanning).
    for m in re.finditer(r"\bmatch\b[^{;]*?\{", masked):
        body_end = _balanced(masked, m.end() - 1)
        body = masked[m.end() : body_end - 1]
        base = m.end()
        pos = 0
        depth = 0
        arm_start = 0
        i = 0
        while i < len(body):
            ch = body[i]
            if ch in "([{":
                depth += 1
            elif ch in ")]}":
                depth -= 1
            elif depth == 0 and body.startswith("=>", i):
                pattern = body[arm_start:i]
                guard = pattern.find(" if ")
                if guard != -1:
                    pattern = pattern[:guard]
                scan_pattern(pattern, base + arm_start, "arm")
                # skip the arm's value: a brace block, or to the next `,` at depth 0
                j = i + 2
                while j < len(body) and body[j] in " \t\n":
                    j += 1
                if j < len(body) and body[j] == "{":
                    j = _balanced(body, j)
                else:
                    d = 0
                    while j < len(body):
                        c = body[j]
                        if c in "([{":
                            d += 1
                        elif c in ")]}":
                            d -= 1
                        elif c == "," and d == 0:
                            break
                        j += 1
                while j < len(body) and body[j] in " \t\n,":
                    j += 1
                arm_start = j
                i = j
                continue
            i += 1

    # 3. kind_id() == Lang::Kind / Kind (bare under a glob), either order
    def kind_operand(prefix: str, head: str, tail: str | None, idx: int) -> None:
        """`Lang::Kind` (tail set), an alias path, or a bare `Kind` under a glob."""
        _, aliases, _ = scope_at(scopes, idx)
        if tail:
            lang = _lang_of(prefix + head + "::", aliases, cl.langs)
            if lang:
                record(idx, lang, tail, "kind_id")
        elif not prefix:
            for lang in bare_langs(idx):
                record(idx, lang, head, "kind_id")

    operand = rf"((?:\$crate::|crate::)?(?:{IDENT}::)*?)({CAMEL})(?:::({CAMEL}))?"
    macro_operand = rf"\$(?:lang|language|Lang)::({CAMEL})"
    for m in re.finditer(rf"kind_id\(\)\s*(?:as\s+u16\s*)?[!=]=\s*{macro_operand}\b", masked):
        for lang in macro_body_lang(m.start()):
            record(m.start(), lang, m.group(1), "kind_id")
    # The receiver may be a macro metavariable (`$n.kind_id()`).
    for m in re.finditer(rf"(?<![:\w]){macro_operand}\s*(?:as\s+u16\s*)?[!=]=\s*\$?\w+\.kind_id\(\)", masked):
        for lang in macro_body_lang(m.start()):
            record(m.start(), lang, m.group(1), "kind_id")
    for m in re.finditer(rf"kind_id\(\)\s*(?:as\s+u16\s*)?[!=]=\s*{operand}\b", masked):
        kind_operand(m.group(1), m.group(2), m.group(3), m.start())
    for m in re.finditer(rf"(?<![:\w]){operand}\s*(?:as\s+u16\s*)?[!=]=\s*\$?\w+\.kind_id\(\)", masked):
        kind_operand(m.group(1), m.group(2), m.group(3), m.start())

    # 4. kind() == "…": a string names no language, so the lexical scope
    # decides — a glob, a plain `use …::Lang;`, an alias, or the languages
    # a macro body is invoked with. With no language in scope the site is
    # reported as unresolved rather than guessed from the spelling.
    for m in re.finditer(r"\.kind\(\)\s*[!=]=", masked):
        j = m.end()
        while j < len(src.text) and src.text[j] in " \t":
            j += 1
        lit = src.literal_at(j)
        if lit is None or _in_spans(m.start(), tests):
            continue
        if in_macro_body(m.start()):
            # The macro's invoking languages are the scope — possibly
            # none that declares rows yet, which is an answer, not an
            # unresolved site.
            candidates = macro_body_lang(m.start())
        else:
            globs, aliases, named = scope_at(scopes, m.start())
            candidates = globs | named | set(aliases.values())
            if not candidates and any(s_ == lit for (_, s_) in cl.by_string):
                problems.append(f"{src.rel}:{src.line(m.start())}: `kind() == {lit!r}` spells a role-claimed kind but no language is in scope here; import the language or exempt the site")
                continue
        for lang in candidates:
            variant = cl.by_string.get((lang, lit))
            if variant:
                record(m.start(), lang, variant, "kind_str")

    # 5. const / static arrays
    for m in re.finditer(r"\b(?:const|static)\s+" + IDENT + r"\s*:[^=;]*=\s*&?\[", masked):
        end = _balanced(masked, m.end() - 1)
        scan_pattern(masked[m.end() : end - 1], m.end(), "const")

    # 6. macro invocations with a `$lang` argument carrying kind idents
    for m in re.finditer(r"\b(" + IDENT + r")!\s*\(", masked):
        name = m.group(1)
        mac = macros.get(name)
        if mac is None or mac.lang_arg is None:
            continue
        end = _balanced(masked, m.end() - 1)
        args = [a.strip() for a in masked[m.end() : end - 1].split(",")]
        if mac.lang_arg >= len(args):
            continue
        lang = args[mac.lang_arg]
        if lang not in cl.langs:
            continue
        for i, a in enumerate(args):
            if i == mac.lang_arg:
                continue
            for b in re.finditer(rf"(?<![:\w]){CAMEL}\b", a):
                record(m.start(), lang, b.group(0), "macro_arg", item=f"{name}!")
    return findings, problems


TEST_MOD_FILE_RE = re.compile(r"#\[cfg\((?:test|all\(\s*test\b[^)]*\))\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(" + IDENT + r")\s*;")


INLINE_MOD_RE = re.compile(r"(?:pub(?:\([^)]*\))?\s+)?\bmod\s+(" + IDENT + r")\s*\{")


def test_only_files(files: list[pathlib.Path]) -> set[pathlib.Path]:
    """Files loaded only by an out-of-line `#[cfg(test)] mod x;` — test
    code the boundary's directory globs would otherwise scan as
    production. A declaration at the file's top level resolves to `x.rs`
    or `x/mod.rs` beside a `mod.rs` / `lib.rs` / `main.rs`, or under the
    declaring file's own directory otherwise; one nested in inline
    modules `mod a { mod b { mod x; } }` resolves under `a/b/`, as rustc
    does. Only a declaration found on the top-level path is excluded; a
    production `x.rs` beside the file is never mistaken for a test
    module declared inside `mod tests { … }`."""
    out: set[pathlib.Path] = set()
    for f in files:
        masked = rt.mask_dead(f.read_text(encoding="utf-8"))
        # inline module extents, so a declaration's enclosing path is known
        inline = [(m.start(), _balanced(masked, m.end() - 1), m.group(1)) for m in INLINE_MOD_RE.finditer(masked)]
        root = f.parent if f.name in ("mod.rs", "lib.rs", "main.rs") else f.with_suffix("")
        for m in TEST_MOD_FILE_RE.finditer(masked):
            name = m.group(1)
            enclosing = sorted((s_, e, n) for s_, e, n in inline if s_ <= m.start() < e)
            base = root
            for _, _, n in enclosing:
                base = base / n
            for cand in (base / f"{name}.rs", base / name / "mod.rs"):
                if cand.exists():
                    out.add(cand.resolve())
    return out


def boundary_files() -> list[pathlib.Path]:
    files: list[pathlib.Path] = []
    for rel in BOUNDARY:
        p = REPO_ROOT / rel
        if p.is_dir():
            files.extend(sorted(p.rglob("*.rs")))
        elif p.is_file():
            files.append(p)
    skip = test_only_files(files)
    return [f for f in files if f.resolve() not in skip]


def analyse(files: list[pathlib.Path], cl: Claims) -> tuple[list[Finding], list[str]]:
    sources = [load_source(f) for f in files]
    macros: dict[str, Macro] = {}
    for s in sources:
        macros.update(parse_macros(s))
    # which languages each `use $lang::*` macro is invoked with
    invocations: dict[str, set[str]] = {}
    for s in sources:
        for m in re.finditer(r"\b(" + IDENT + r")!\s*\(", s.masked):
            mac = macros.get(m.group(1))
            if mac is None or mac.lang_arg is None:
                continue
            end = _balanced(s.masked, m.end() - 1)
            args = [a.strip() for a in s.masked[m.end() : end - 1].split(",")]
            if mac.lang_arg < len(args) and args[mac.lang_arg] in cl.langs:
                invocations.setdefault(mac.name, set()).add(args[mac.lang_arg])
    mlangs = macro_languages(macros, invocations)
    findings: list[Finding] = []
    problems: list[str] = []
    for s in sources:
        f, p = scan(s, cl, macros, mlangs)
        seen: dict[tuple[str, str, str, str, str], int] = {}
        for x in sorted(f, key=lambda x: x.line):
            k = (x.file, x.item, x.lang, x.kind, x.shape)
            seen[k] = seen.get(k, 0) + 1
            findings.append(Finding(x.file, x.item, x.lang, x.kind, x.shape, x.line, seen[k]))
        problems.extend(p)
        for mac in parse_macros(s).values():
            if mac.touches_kinds and mac.name not in mlangs and not mac.expands_to_kinds:
                how = "takes no `$lang:ident`" if mac.lang_arg is None else "takes a `$lang` it never spells in its body"
                problems.append(f"{s.rel}: `macro_rules! {mac.name}` reads kinds but no language reaches it — it {how}, opens no `use $lang::*`, and no macro that does invokes it; unsupported, exempt it with a reason or restructure")
    return findings, problems


# ---------------------------------------------------------------------------
# Baseline
# ---------------------------------------------------------------------------


def load_baseline(path: pathlib.Path) -> set[tuple[str, str, str, str, str]]:
    out = set()
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != 5 or not re.fullmatch("(" + "|".join(SHAPES) + r")#\d+", parts[4]):
            raise rt.RoleTableError(f"{path}:{lineno}: expected 5 tab-separated fields ending in `<shape>#<n>`, got `{line}`")
        out.add(tuple(parts))  # type: ignore[arg-type]
    return out


def write_baseline(path: pathlib.Path, keys: set[tuple[str, str, str, str, str]]) -> None:
    path.write_text(HEADER + "".join("\t".join(k) + "\n" for k in sorted(keys)), encoding="utf-8")


def existed_at_head(path: pathlib.Path) -> bool:
    rel = path.relative_to(REPO_ROOT) if path.is_relative_to(REPO_ROOT) else path
    run = subprocess.run(["git", "-C", str(REPO_ROOT), "ls-tree", "--name-only", "HEAD", "--", str(rel)], capture_output=True, text=True, check=False)
    return run.returncode == 0 and bool(run.stdout.strip())


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--baseline", type=pathlib.Path, default=DEFAULT_BASELINE)
    ap.add_argument("--roles-dir", type=pathlib.Path, default=rt.ROLES_DIR)
    ap.add_argument("--langs-rs", type=pathlib.Path, default=rt.LANGS_RS)
    ap.add_argument("--languages-dir", type=pathlib.Path, default=rt.LANGUAGES_DIR)
    ap.add_argument("--files", type=pathlib.Path, nargs="*", help="scan these files instead of the boundary (tests)")
    ap.add_argument("--update", action="store_true")
    ap.add_argument("--bootstrap", action="store_true")
    ap.add_argument("--show", action="store_true", help="print every finding with its line")
    args = ap.parse_args(argv)

    print(SCOPE_NOTE)
    try:
        cl = claims(args.roles_dir, args.langs_rs, args.languages_dir)
        files = args.files if args.files is not None else boundary_files()
        findings, problems = analyse(files, cl)
    except rt.RoleTableError as exc:
        print(f"error: {exc}")
        return 2
    if problems:
        for p in problems:
            print(f"error: {p}")
        print(f"role-bypass: FAIL — {len(problems)} problem(s)")
        return 1
    current = {f.key() for f in findings}
    if args.show:
        for f in sorted(findings, key=lambda f: (f.file, f.line)):
            print(f"  {f.file}:{f.line} [{f.item}] {f.lang}::{f.kind} via {f.shape}")

    if args.bootstrap:
        if args.baseline.exists() or existed_at_head(args.baseline):
            print(f"error: {args.baseline} exists or is tracked at HEAD; use --update, never re-bootstrap")
            return 2
        write_baseline(args.baseline, current)
        print(f"role-bypass: wrote {len(current)} finding(s) to {args.baseline}")
        return 0
    if not args.baseline.exists():
        print(f"error: {args.baseline} missing; a first run needs --bootstrap")
        return 2
    try:
        recorded = load_baseline(args.baseline)
    except rt.RoleTableError as exc:
        print(f"error: {exc}")
        return 2
    new = current - recorded
    if new:
        by_key = {f.key(): f for f in findings}
        for k in sorted(new):
            f = by_key[k]
            print(f"error: {f.file}:{f.line} [{f.item}] restates `{f.lang}::{f.kind}` ({f.shape}), which a role row claims — call the row, or exempt with `// role-bypass: allow(<Role>) — <reason>`")
        print(f"role-bypass: FAIL — {len(new)} new bypass site(s); `--update` will not accept them")
        return 1
    gone = recorded - current
    if args.update:
        # Always rewrite: the header is regenerated with the entries, so
        # a stale header cannot outlive the next update.
        write_baseline(args.baseline, current)
        if gone:
            print(f"role-bypass: removed {len(gone)} entr{'y' if len(gone) == 1 else 'ies'}")
        else:
            print("role-bypass: baseline rewritten; entries already current")
        return 0
    if gone:
        print(f"note: {len(gone)} baseline entr{'y' if len(gone) == 1 else 'ies'} no longer found; run --update")
    print(f"role-bypass: OK — {len(current)} baselined site(s) across {len(files)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
