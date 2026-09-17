#!/usr/bin/env python3
"""check-test-lang-gates

Verify that every test item naming a concrete per-language parser carries
a ``cfg`` marker implying that language's Cargo feature.

``mk_langs!`` generates each ``*Parser`` alias and each ``LANG`` variant
unconditionally -- only ``get_ts_language``'s arms are cfg'd. So
``check_metrics::<PythonParser>(…)`` *compiles* with ``python`` disabled
and then panics inside ``Tree::new``::

    invariant: the caller checked LANG::is_enabled, or reached this
    through AnyParser

``ParserTrait::new``'s own doc says as much and directs callers to check
``LANG::is_enabled`` first. Before #1472 almost none did, and
``--no-default-features --features rust`` failed ~2,600 tests, which
makes a partial-feature build useless for verifying anything -- the
misleading-red class #1171 fixed for the corpora.

This gate is the companion to ``check-feature-gates.py``. That one
catches a union gate that is present but *defeated* (weakened by a
non-feature disjunct). It structurally cannot catch a *missing* one,
because deleting the marker deletes the subject it scans for. This one
derives what the marker must be from the item's own contents and
compares.

The derivation, for every item inside a ``cfg(test)`` scope:

1. **Needed set** -- the languages the item reaches unconditionally:
   concrete ``*Parser`` aliases and ``LANG::<Variant>`` literals in its
   body, ignoring anything already behind a nested
   ``#[cfg(feature = …)]``, plus transitively the needed set of
   same-module helper ``fn``s it calls.
2. **Declared gate** -- the item's own ``cfg`` conjoined with every
   enclosing ``mod``'s.
3. **Verdict** -- for each needed feature, evaluate the declared gate
   with that feature (and everything implying it) off and every other
   language feature on. If the gate is still satisfied, the item would
   be compiled into a build that cannot run it: that is an offender.

Evaluating the real predicate under a one-feature-off assignment is what
makes this sound without symbolic implication, and it handles ``any`` /
``all`` / ``not`` / nesting for free.

A ``#[test] fn`` needs *every* language it names, so its marker is an
``all(…)``. A helper is live when *any* caller is, so its marker is an
``any(…)`` over its callers. An item that filters at runtime on
``LANG::is_enabled`` / ``into_enum_iter`` skips disabled languages by
itself; those are checked only for naming at least one enabled language,
and the ``checked > 0`` non-vacuity discipline (#1286) plus
``check-feature-gates.py`` own the rest.

Run it over the whole tree::

    ./utils/check-test-lang-gates.py

``--show`` lists every item and its derived marker; ``--fix`` writes the
missing markers in place. It is wired into ``make pre-commit`` and
``make ci``: unlike ``check-feature-gates.py`` this is a pure source
scan with no cargo invocation, so it is cheap enough for the local gate.
"""

from __future__ import annotations

import argparse
import os
import shutil
import pathlib
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass, replace

# `parents[1]`, not `parent`: these gates live in `utils/` but every path
# they read is anchored at the repository root, so the script works from
# any cwd.
REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]

# The `mk_langs!` invocation is the single source of truth for the
# variant / feature / parser triples. Parsing it rather than copying the
# table is what keeps this gate from drifting the next time a language
# is added -- `Tsx` -> `typescript` and `Ccomment` / `Preproc` ->
# `c-family-helpers` are exactly the rows a hand-maintained copy gets
# wrong.
LANGS_RS = REPO_ROOT / "big-code-analysis-ast" / "src" / "langs.rs"

# Roots holding this workspace's own tests.
#
# `tests/repositories/` is deliberately absent: it is the vendored corpus
# submodule, and serde's own several-hundred-test suite lives under it.
SCAN_ROOTS = (
    pathlib.Path("src"),
    pathlib.Path("big-code-analysis-ast") / "src",
    pathlib.Path("tests"),
)
EXCLUDED = (pathlib.Path("tests") / "repositories",)

# rustfmt's default `max_width`, so a generated marker that fits on one
# line is already the line rustfmt would have produced.
MAX_WIDTH = 100

# How many times `--fix` re-scans before giving up. Three is ample: the
# observed need was two.
FIX_PASSES = 3

# Enabling any of these enables `c-family-helpers`
# (`big-code-analysis-ast/Cargo.toml`), so a build that disables the
# helper feature necessarily disables them too. Every other language
# feature stands alone.
#
# Hand-maintained, unlike the language table: a fourth enabler added to
# that manifest has to be added here too, and nothing in the gate would
# say so. `c_family_helper_enablers_match_the_manifest` in the
# self-tests is what notices.
C_FAMILY_HELPER_FEATURE = "c-family-helpers"
IMPLIES_C_FAMILY_HELPERS = frozenset({"c", "cpp", "mozcpp"})

# `#[test]`, plus the namespaced spellings (`#[tokio::test]`).
TEST_ATTR_RE = re.compile(r"#\[\s*(?:[A-Za-z_][A-Za-z0-9_]*::)*test\s*\]")
# A runtime filter over `LANG`, which makes the item skip disabled
# languages on its own rather than needing them all.
SWEEP_RE = re.compile(r"\b(?:is_enabled|into_enum_iter)\b")
# The three marks of a sweep that is live exactly when *some* language
# is: it walks the whole enum, skips the disabled rows, and then refuses
# to pass having done nothing. The last one is what makes the gate
# mandatory rather than merely tidy — without it the sweep would simply
# run zero iterations and report green.
ALL_LANGS_SWEEP_RE = re.compile(r"\binto_enum_iter\b")
ENABLED_FILTER_RE = re.compile(r"\bis_enabled\b")
NON_VACUITY_RE = re.compile(r"assert_fixtures_present|assert!\s*\(\s*\w+\s*>\s*0")
ITEM_RE = re.compile(
    r"^(?P<indent>[ \t]*)"
    r"(?P<visibility>pub\s*(?:\([^)]*\)\s*)?)?"
    r"(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?"
    r'(?:extern\s+"[^"]*"\s+)?'
    r"(?P<kind>mod|fn|const|static|type)\s+(?:mut\s+)?"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
)
# Every identifier in a body. Needed for a `const` or `type`, which are
# referenced bare.
REFERENCE_RE = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\b")
# The call-shaped subset: `helper(`, `helper::<T>(`. A `fn` referent is
# matched against this rather than the broad set, because the broad set
# links a local named `score` or `shape` to the helper of that name --
# and a false link there marks the helper unconditional, leaving it
# ungated and unused on every single-language leg.
CALL_RE = re.compile(r"\b([a-z_][A-Za-z0-9_]*)\s*(?:::\s*<[^;{}()]*>\s*)?\(")
# A parameter of type `LANG`, which makes a helper language-generic.
#
# The lookbehind is load-bearing: without it the `::` of a path like
# `&[(crate::LANG, &[u8])]` reads as a parameter colon, so a `const`
# fixture table declaring its own element type was taken for a generic
# helper and its languages never reached the tests driving it (70
# failures on the `--features go` leg).
LANG_PARAM_RE = re.compile(r"(?<!:):\s*(?:&\s*)?(?:crate::)?LANG\b")
# The opt-out for a gate the derivation cannot justify, spelled like the
# `// bca: suppress(<metrics>) — <why>` markers the analyser itself reads.
# Anything after the feature list is free text.
HAND_WRITTEN_RE = re.compile(r"//\s*test-lang-gates:\s*hand-written\s*\(([^)]*)\)")
# A macro invocation: `check_metrics_only_shim!(…)`. An imported macro
# is used from item position, where this scanner models nothing, so a
# name invoked this way anywhere in the file is treated as always used.
# Filled in from the language table once it is read.
KNOWN_FEATURES: frozenset[str] = frozenset()
# A `LANG` in match-arm *value* position (`"py" => LANG::Python,`),
# including the wrapped spellings a fallible lookup returns
# (`=> Some(LANG::Python)`, `=> Ok(LANG::Python)`). Without the wrappers
# a `fn pick(ext: &str) -> Option<LANG>` hands every language it can
# return to each of its callers.
DISPATCH_ARM_RE = re.compile(
    r"=>\s*(?:(?:Some|Ok|Err)\s*\(\s*)?(?:crate::)?(LANG::[A-Za-z][A-Za-z0-9]*)"
)
# A `LANG` being *compared*, not used: `lang == LANG::Go`,
# `matches!(lang, LANG::Cpp | LANG::Mozcpp)`. Asking which variant a value
# is needs no grammar — the enum is generated unconditionally — so
# counting one as a requirement gates the test out of builds it could run
# in. `container_scope_tests.rs` lost the positive half of the #1197
# contract to exactly this (#1478).
COMPARISON_RES = (
    # `lang == LANG::Go`, `lang != crate::LANG::Go`
    re.compile(r"[=!]=\s*(?:crate::)?(LANG::[A-Za-z][A-Za-z0-9]*)"),
    # `LANG::Go == lang`
    re.compile(r"(LANG::[A-Za-z][A-Za-z0-9]*)\s*[=!]="),
    # `matches!(lang, LANG::Cpp …)` — the first alternative. The rest of
    # the alternation is handled by `MATCHES_CALL_RE` below, which needs
    # balanced parens rather than a regex.
    re.compile(r"matches!\s*\([^()]*?(?:crate::)?(LANG::[A-Za-z][A-Za-z0-9]*)"),
    # `assert_eq!` / `assert_ne!` are *not* here: `[^()]*?` cannot cross a
    # parenthesis, so it stops at the first call in the argument list and
    # never reaches the variant. `ASSERT_CALL_RE` below reads the whole
    # argument list instead.
)
# Deliberately not the later arms of an or-pattern. `| LANG::X` is also
# how a per-language *dispatch table* groups its arms
# (`LANG::C | LANG::Cpp => ("…", "c")`), and excluding those strips the
# languages a sweep derives from the table it drives — eleven items lost
# their whole C-family union to that.
#
# A `matches!` is the exception, handled by `MATCHES_CALL_RE` above: it
# has no arms and drives no table, so every alternative in one is an
# identity test. Keeping them cost two `every_*_in_every_language`
# parity sweeps their whole gate — the single `c-family-helpers` that
# leaked out of a `Ccomment | Preproc` *exclusion* became the entire
# derivation, and they ran in four builds instead of twenty-three.
#
# Nothing here reports that on its own. `over_gated` compares a gate
# against the derivation, so a gate faithfully mirroring a wrong
# derivation is by construction never flagged, and an `any(...)`
# requires no single feature for it to object to. `--compare` found it,
# on its first run in CI, which is the argument for that check existing.


# A whole `matches!` call. *Every* alternative inside one is an identity
# test, not just the first: `matches!(lang, LANG::Ccomment | LANG::Preproc)`
# asks which variant a value is and parses nothing, so the later arms are
# no more a use than the leading one.
#
# This is not the or-pattern caveat below, which is about `match` *arms*
# — `LANG::C | LANG::Cpp => ("…", "c")` groups a dispatch table, and a
# sweep does derive its languages from the table it drives. A `matches!`
# has no arms and no table. Reading its later alternatives as uses is
# what gated two `every_*_in_every_language` parity sweeps down to the
# single `c-family-helpers` that leaked out of their `Ccomment |
# Preproc` exclusion, which `--compare` caught on its first CI run.
MATCHES_CALL_RE = re.compile(r"\bmatches!\s*\(")
# `assert_eq!(get_from_ext("rb"), Some(LANG::Ruby))`. The expected side of
# an equality assertion is a *value*, and a `LANG` variant exists without
# its grammar, so naming one there parses nothing.
#
# Reading it with a flat `[^()]*?` — which is how this shipped — stops at
# the first `(` in the argument list and never sees the variant, so 37
# tests over `get_from_ext` / `get_from_emacs_mode` / `FromStr` / `name()`
# were gated on grammars they never touch, several down to one build in
# twenty-five. None of those functions is `cfg`-gated; only `is_enabled`,
# `get_ts_language` and the `AnyParser` entry points are.
ASSERT_CALL_RE = re.compile(r"\bassert_(?:ne|eq)!\s*\(")
# A variant in *receiver* position: `LANG::Mozcpp.extensions()`. Most
# methods on `LANG` read the table `mk_langs!` generates
# unconditionally — extensions, slug, display name — and `is_enabled`
# answers a question that is meaningful precisely when the grammar is
# absent. None of those needs a feature.
#
# The two that hand back the grammar itself do. `tree_sitter_language()`
# returns `None` with the feature off, and every caller here follows it
# with `.expect(…)`, so treating the receiver as metadata gates the test
# out of nothing and lets it panic under a build without that language —
# the same failure as `Tree::new`, one call earlier.
LANG_RECEIVER_RE = re.compile(
    r"(?:crate::)?(LANG::[A-Za-z][A-Za-z0-9]*)\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)"
)
GRAMMAR_BEARING_METHODS = frozenset({"tree_sitter_language", "get_ts_language"})
LANG_PATH_RE = re.compile(r"(?:crate::)?(LANG::[A-Za-z][A-Za-z0-9]*)")


def _is_value_position(args: str, offset: int) -> bool:
    """Whether the variant at ``offset`` is compared rather than used.

    ``args`` is an equality assertion's argument list. A variant wrapped
    only in constructors — `Some(…)`, `Ok(…)`, a tuple, a `vec![…]` — is
    the expected value and needs no grammar. One passed to a *function*
    is an argument to the thing under test, which may well parse it:
    `assert_eq!(conditions(LANG::Go, src), 3)` genuinely needs Go.

    The two are told apart by the identifier before each enclosing `(`,
    on Rust's own naming convention: lower-case means a call, upper-case
    or nothing means a constructor or a tuple.
    """
    stack: list[int] = []
    for index, char in enumerate(args[:offset]):
        if char == "(":
            stack.append(index)
        elif char == ")" and stack:
            stack.pop()
    for open_index in stack:
        end = open_index
        while end > 0 and args[end - 1].isspace():
            end -= 1
        start = end
        while start > 0 and (args[start - 1].isalnum() or args[start - 1] == "_"):
            start -= 1
        name = args[start:end]
        if name and (name[0].islower() or name[0] == "_"):
            return False
    return True


class ScanError(Exception):
    """A malformed input the scanner refuses to guess about."""


# ---------------------------------------------------------------------------
# Rust literal / comment lexing
#
# Ported from `check-feature-gates.py`, itself ported from
# `check-snapshot-anchors.py`, and needed here for the same reason: a
# `PythonParser` spelled inside a fixture string or a `//` comment is not
# a live reference, and counting one derives a gate the item does not
# need. The gate scripts are hyphen-named and so not importable; every
# one of them carries its own copy. Fix a lexing bug here and check the
# siblings.
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


def mask_dead(source: str) -> str:
    """``source`` with every comment and literal blanked to spaces.

    Blanked rather than deleted so every index and line number in the
    masked text still addresses the original.
    """
    chars = list(source)
    for start, end in dead_spans(source):
        for i in range(start, min(end, len(chars))):
            if chars[i] != "\n":
                chars[i] = " "
    return "".join(chars)


# ---------------------------------------------------------------------------
# The language table
# ---------------------------------------------------------------------------

# One `mk_langs!` row: feature literal, CamelCase variant, then (six
# fields along) the `*Parser` alias. Matching the alias by name rather
# than by counting commas keeps this robust against the description
# strings, which contain both commas and escaped newlines.
LANG_ROW_RE = re.compile(
    r'\(\s*"(?P<feature>[a-z0-9-]+)"\s*,\s*'
    r"(?P<variant>[A-Za-z][A-Za-z0-9]*)\s*,.*?"
    r"(?P<code>[A-Za-z][A-Za-z0-9]*Code)\s*,\s*"
    r"(?P<parser>[A-Za-z][A-Za-z0-9]*Parser)\s*,",
    re.DOTALL,
)


def enabled_closure(feature: str) -> frozenset[str]:
    """``feature`` plus the language features enabling it turns on.

    The forward direction of `disabled_closure`: `cpp` lists
    `c-family-helpers`, so a build asking for `cpp` gets the helper
    grammars too.
    """
    if feature in IMPLIES_C_FAMILY_HELPERS:
        return frozenset({feature, C_FAMILY_HELPER_FEATURE})
    return frozenset({feature})


def probe_builds(table: dict[str, str]) -> dict[str, frozenset[str]]:
    """The feature sets membership is compared under.

    Derived from the language table rather than from the CI matrix, so
    adding a language extends the probe set for free and nothing has to
    be kept in step. Each single-language build is the configuration a
    per-language gate is most likely to get wrong, and the two ends
    catch a gate that moved without naming a language at all.
    """
    languages = frozenset(table.values())
    probes = {
        "--no-default-features": frozenset(),
        "--all-features": languages,
    }
    for language in sorted(languages):
        probes[f"--features {language}"] = enabled_closure(language)
    return probes


def language_table(langs_rs: str) -> dict[str, str]:
    """Map every language spelling a test body can use to its feature.

    Keys are the *qualified* spellings -- ``LANG::Python`` and
    ``PythonParser`` -- never the bare variant. Half the variant names
    (``C``, ``Go``, ``Java``, ``Rust``) are ordinary identifiers that
    occur as generic parameters and type names all over these files, so
    a table keyed on the bare form derives gates for languages the item
    never touches.
    """
    table: dict[str, str] = {}
    for row in LANG_ROW_RE.finditer(mask_comments_only(langs_rs)):
        feature = row.group("feature")
        # `.*?` spans the description fields, so a row missing one would
        # silently match the *next* row's `*Code` / `*Parser` and file
        # them under the wrong feature. Every real row names them after
        # its variant, so requiring that turns the bleed into an error.
        variant = row.group("variant")
        for suffix, found in (
            ("Code", row.group("code")),
            ("Parser", row.group("parser")),
        ):
            if found != variant + suffix:
                raise ScanError(
                    f"`mk_langs!` row for `{variant}` names `{found}` where "
                    f"`{variant}{suffix}` was expected — a field is missing "
                    "and the row has run into the next one"
                )
        table[f"LANG::{row.group('variant')}"] = feature
        table[row.group("parser")] = feature
        # The `*Code` tag is the third spelling, and the one a generic
        # test reaches the grammar through: `check::<CppCode>(…)` inside
        # `for_each_node_with_chain::<L>` parses exactly as
        # `CppParser::new` does. Omitting it left 79 tests ungated and
        # panicking on the `--features go` leg.
        table[row.group("code")] = feature
    global KNOWN_FEATURES
    KNOWN_FEATURES = frozenset(table.values())
    if not table:
        raise ScanError(
            f"no `mk_langs!` rows found in {LANGS_RS}; the language table "
            "cannot be derived and this gate has nothing to check against"
        )
    return table


def symbol_pattern(table: dict[str, str]) -> re.Pattern[str]:
    """Match any language spelling in ``table``, longest alternative first.

    ``LANG::Typescript`` must be tried before ``LANG::Tsx`` only in the
    sense that Python's alternation is first-match: sorting by length
    descending keeps a shorter key from claiming a prefix of a longer
    one.
    """
    alternatives = "|".join(
        re.escape(key) for key in sorted(table, key=len, reverse=True)
    )
    return re.compile(rf"\b(?:{alternatives})\b")


def mask_comments_only(source: str) -> str:
    """``source`` with comments blanked but string literals kept.

    The `mk_langs!` rows are *made of* string literals, so the full mask
    would erase the feature names this table is read from. Comments still
    have to go: `langs.rs` documents the tuple layout in a comment that
    contains a parenthesised example.
    """
    chars = list(source)
    for start, end in dead_spans(source):
        if source[start] not in "/":
            continue
        for i in range(start, min(end, len(chars))):
            if chars[i] != "\n":
                chars[i] = " "
    return "".join(chars)


# ---------------------------------------------------------------------------
# cfg predicate parsing and evaluation
# ---------------------------------------------------------------------------


def balanced_group(text: str, open_idx: int) -> tuple[str, int] | None:
    """Contents of the ``(``-delimited group at ``open_idx``, and its end.

    ``None`` when the parentheses do not balance before the text runs
    out, which the caller treats as a malformed attribute rather than
    guessing at its shape.
    """
    if open_idx >= len(text) or text[open_idx] != "(":
        return None
    depth = 0
    for i in range(open_idx, len(text)):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return text[open_idx + 1 : i], i
    return None


def split_top_level(text: str) -> list[str]:
    """Split on commas that are not nested inside parentheses."""
    parts: list[str] = []
    depth = 0
    current: list[str] = []
    for ch in text:
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append("".join(current))
            current = []
            continue
        current.append(ch)
    tail = "".join(current)
    if tail.strip():
        parts.append(tail)
    return [p.strip() for p in parts if p.strip()]


FEATURE_ATOM_RE = re.compile(r'^feature\s*=\s*"([^"]*)"$')


def evaluate_predicate(predicate: str, disabled: frozenset[str]) -> bool:
    """Whether ``predicate`` holds with ``disabled`` features turned off.

    Every feature not named in ``disabled`` is treated as enabled, and
    every non-``feature`` atom (``test``, ``unix``, ``debug_assertions``)
    as true. Both defaults point the same way: assume the item *is*
    compiled unless a feature predicate says otherwise, so a gate only
    clears an item when it genuinely excludes it.
    """
    predicate = predicate.strip()
    # `startswith` alone treats `cfg(anyhow)` and `cfg(nothing)` as
    # malformed combinators and raises; the `(` is what distinguishes a
    # combinator from an ordinary atom that happens to share a prefix.
    for combinator, reduce_fn in (("all", all), ("any", any)):
        if predicate.startswith(combinator) and predicate[
            len(combinator) :
        ].lstrip().startswith("("):
            rest = predicate[len(combinator) :].lstrip()
            group = balanced_group(rest, 0)
            if group is None:
                raise ScanError(f"unbalanced `{combinator}(` in cfg: {predicate!r}")
            return reduce_fn(
                evaluate_predicate(part, disabled) for part in split_top_level(group[0])
            )
    if predicate.startswith("not") and predicate[len("not") :].lstrip().startswith("("):
        rest = predicate[len("not") :].lstrip()
        group = balanced_group(rest, 0)
        if group is None:
            raise ScanError(f"unbalanced `not(` in cfg: {predicate!r}")
        return not evaluate_predicate(group[0], disabled)
    match = FEATURE_ATOM_RE.match(predicate)
    if match:
        return match.group(1) not in disabled
    return True


def disabled_closure(feature: str) -> frozenset[str]:
    """``feature`` plus every feature that would re-enable it.

    Only `c-family-helpers` has any: `c`, `cpp` and `mozcpp` each list it
    (`big-code-analysis-ast/Cargo.toml`), so a build without the helper
    grammars is necessarily a build without those three.
    """
    if feature == C_FAMILY_HELPER_FEATURE:
        return frozenset({feature}) | IMPLIES_C_FAMILY_HELPERS
    return frozenset({feature})


def gate_excludes(predicate: str | None, feature: str) -> bool:
    """Whether ``predicate`` compiles the item *only* when ``feature`` is off.

    ``#[cfg(not(feature = "javascript"))]`` marks a test of the
    disabled-language path: it names `JavascriptParser` precisely
    because the grammar is absent, and demanding a `javascript` gate on
    it would invert the test. Three such tests exist
    (`langs.rs::disabled_language_dispatch_returns_language_disabled`
    and two in `tests/api/ast_seam_test.rs`).
    """
    if predicate is None:
        return False
    return not evaluate_predicate(predicate, frozenset()) and evaluate_predicate(
        predicate, disabled_closure(feature)
    )


def negated_features(predicate: str) -> frozenset[str]:
    """Features ``predicate`` requires to be *off*, read syntactically.

    The `not(feature = …)` case. Such a gate is false with everything
    enabled, so measuring over-declaration from an all-on baseline reads
    every other feature as required; this names the features to turn off
    first, so the measurement starts from a build the item compiles in.

    Read out of the text rather than probed one feature at a time.
    Probing cannot see a conjunction of negations — `all(not(a),
    not(b))` is false with either alone disabled, so no single probe
    ever finds the build that satisfies it, and every feature in the
    tree then reads as over-declared. No such gate exists here yet; the
    first one would have produced a wall of false positives.
    """
    found: set[str] = set()
    for match in re.finditer(r"\bnot\s*\(", predicate):
        group = balanced_group(predicate, match.end() - 1)
        if group is None:
            continue
        found.update(hit.group(1) for hit in FEATURE_NAME_RE.finditer(group[0]))
    return frozenset(found & KNOWN_FEATURES)


def gate_admits(predicate: str | None, feature: str) -> bool:
    """Whether an item under ``predicate`` compiles with ``feature`` off.

    ``True`` is the offending answer: the item would be built into a
    configuration whose grammar it needs but does not have.
    """
    if predicate is None:
        return True
    return evaluate_predicate(predicate, disabled_closure(feature))


# ---------------------------------------------------------------------------
# Item discovery
# ---------------------------------------------------------------------------

CFG_ATTR_RE = re.compile(r"#!?\[\s*cfg\s*\(")
BARE_TEST_RE = re.compile(r"\btest\b")
FEATURE_PREDICATE_RE = re.compile(r'feature\s*=\s*"[^"]*"')
# The same atom, unanchored and capturing, for reading names out of a
# sub-expression. `FEATURE_ATOM_RE` is anchored and matches one whole
# predicate, so `finditer` over a group's contents finds nothing at all.
FEATURE_NAME_RE = re.compile(r'feature\s*=\s*"([^"]*)"')
MACRO_RULES_RE = re.compile(r"\bmacro_rules!\s*([A-Za-z_][A-Za-z0-9_]*)")


def cfg_predicate(attr: str) -> str | None:
    """The predicate inside ``#[cfg(...)]``, or ``None`` if not a cfg."""
    match = CFG_ATTR_RE.search(attr)
    if match is None:
        return None
    group = balanced_group(attr, match.end() - 1)
    if group is None:
        raise ScanError(f"unbalanced `cfg(` in attribute: {attr!r}")
    return group[0].strip()


def predicate_requires_test(predicate: str) -> bool:
    """Whether ``predicate`` carries a bare ``test``.

    Feature names are blanked first, so a language literally called
    ``test`` could not be mistaken for ``cfg(test)``. Negated groups are
    blanked too: `cfg(not(test))` marks code that exists *outside* the
    test build, and reading it as a test scope pulls production items
    into `--show` and into the helper-inheritance walk.
    """
    return bool(
        BARE_TEST_RE.search(_without_negations(FEATURE_PREDICATE_RE.sub("", predicate)))
    )


def _without_negations(predicate: str) -> str:
    """``predicate`` with every ``not(...)`` group blanked out."""
    out = predicate
    while True:
        match = re.search(r"\bnot\s*\(", out)
        if match is None:
            return out
        group = balanced_group(out, match.end() - 1)
        if group is None:
            return out
        out = (
            out[: match.start()]
            + " " * (group[1] - match.start() + 1)
            + out[group[1] + 1 :]
        )


@dataclass
class Item:
    """A ``mod`` or ``fn`` in a test scope, with its derived needs."""

    path: str
    #: 1-based line of the item header.
    line: int
    #: 1-based first line of the item's attribute run. `--fix` inserts
    #: here, so a generated marker lands above `#[test]` rather than
    #: between it and the signature.
    attr_line: int
    indent: str
    kind: str
    name: str
    #: `cfg(...)` predicate texts written on this item.
    own_predicates: tuple[str, ...]
    is_test_fn: bool
    is_sweep: bool
    #: Iterates `LANG::into_enum_iter()` behind an `is_enabled()` filter
    #: *and* asserts non-vacuity, so it fails rather than skips when no
    #: language is enabled.
    needs_a_language_enabled: bool
    #: Visible outside this file, so its users are not all in view.
    is_public: bool
    #: The signature takes a runtime `LANG`, so the languages its body
    #: names are dispatch arms rather than the caller's requirements.
    takes_lang_param: bool
    #: Language features named directly in the body, outside any inner
    #: `cfg` that already excludes them.
    direct: frozenset[str]
    #: The subset of `direct` reached through a concrete `*Parser` /
    #: `*Code` type rather than a `LANG::` value. A type parameter cannot
    #: be chosen at run time, so these are requirements even for a sweep.
    hardcoded: frozenset[str]
    #: Features a `// test-lang-gates: hand-written(…)` comment accepts as
    #: gated for a reason the derivation cannot see.
    hand_written: frozenset[str]
    #: Call-shaped references in the body, for resolving `fn`s.
    calls: frozenset[str]
    #: Every identifier in the body, for resolving `const` / `type`.
    references: frozenset[str]
    parent_index: int | None
    #: Index into the file's item list; the parent link is by index so
    #: the dataclass stays comparable and printable.
    index: int
    in_test_scope: bool

    def describe(self) -> str:
        return f"{self.path}:{self.line}  {self.kind} {self.name}"


def qualified_name(items: list[Item], item: Item) -> str:
    """``path::mod::mod::name`` — stable across revisions.

    What makes a test the *same* test in two trees. A rename or a move
    reads as one name gone and another arrived, which is what keeps a
    deliberate deletion from reporting as a regression.
    """
    parts = [item.name]
    node = None if item.parent_index is None else items[item.parent_index]
    while node is not None:
        if node.kind == "mod":
            parts.append(node.name)
        node = None if node.parent_index is None else items[node.parent_index]
    return item.path + "::" + "::".join(reversed(parts))


def membership(
    per_file: dict[str, list[Item]], probes: dict[str, frozenset[str]]
) -> dict[str, frozenset[str]]:
    """Which probe builds compile each test, keyed by qualified name."""
    languages = frozenset().union(*probes.values()) if probes else frozenset()
    result: dict[str, frozenset[str]] = {}
    for items in per_file.values():
        for item in items:
            if not (item.is_test_fn and item.in_test_scope):
                continue
            predicate = effective_predicate(items, item)
            if predicate is None:
                result[qualified_name(items, item)] = frozenset(probes)
                continue
            result[qualified_name(items, item)] = frozenset(
                name
                for name, enabled in probes.items()
                if evaluate_predicate(predicate, languages - enabled)
            )
    return result


def effective_predicate(items: list[Item], item: Item) -> str | None:
    """``item``'s ``cfg`` conjoined with every enclosing item's."""
    parts: list[str] = []
    node: Item | None = item
    while node is not None:
        parts.extend(node.own_predicates)
        node = None if node.parent_index is None else items[node.parent_index]
    if not parts:
        return None
    if len(parts) == 1:
        return parts[0]
    return "all(" + ", ".join(parts) + ")"


def _join_attributes(
    lines: list[str], raw: list[str], start: int
) -> tuple[list[str], int]:
    """Attributes starting at ``start``; returns them and the next index.

    ``lines`` is the masked text and decides *where* the attributes are
    -- a ``#[cfg(…)]`` quoted inside a fixture string is blanked there
    and correctly reads as ordinary text. The attribute text itself
    comes from ``raw``, because masking blanks string literals and the
    feature names this gate exists to read are string literals. Reading
    the predicate off the masked line yields ``feature = ""`` for every
    gate in the tree, which no feature set can ever disable, so every
    correctly gated item reports as an offender.

    Multi-line attributes are joined onto one line. Blank lines (which
    is also what a masked comment looks like) do not end a run, because
    these modules routinely put a rationale comment between two
    attributes.
    """
    attrs: list[str] = []
    index = start
    while index < len(lines):
        stripped = lines[index].strip()
        if not stripped:
            index += 1
            continue
        if not stripped.startswith("#"):
            break
        buffer = [raw[index].strip()]
        opened = stripped.count("[") - stripped.count("]")
        while opened > 0:
            index += 1
            if index >= len(lines):
                raise ScanError(f"unterminated attribute at line {start + 1}")
            opened += lines[index].count("[") - lines[index].count("]")
            buffer.append(raw[index].strip())
        attrs.append(" ".join(buffer))
        index += 1
    return attrs, index


def _hand_written_features(raw: list[str], attr_line: int) -> frozenset[str]:
    """Features accepted by the marker in the comment run above an item.

    Read from the unmasked text, and from *above* the attributes rather
    than among them, because that is where every such rationale already
    sits in this tree.
    """
    named: set[str] = set()
    index = attr_line - 1
    while index >= 0:
        stripped = raw[index].strip()
        # `//`, but not `///` or `//!`. A machine-read directive has no
        # business hiding in rendered documentation, and a `//!` at the
        # top of a file would attach to that file's first item.
        if not stripped.startswith("//") or stripped.startswith(("///", "//!")):
            break
        match = HAND_WRITTEN_RE.search(stripped)
        if match:
            named.update(
                part.strip() for part in match.group(1).split(",") if part.strip()
            )
        index -= 1
    return frozenset(named)


def _bracket_delta(line: str) -> int:
    """Net nesting change across ``line`` over ``{}``, ``()`` and ``[]``."""
    return sum(line.count(c) for c in "{([") - sum(line.count(c) for c in "})]")


def _inner_cfg_extents(
    lines: list[str], raw: list[str], body_start: int, body_end: int
) -> list[tuple[int, int, str]]:
    """``(first_line, last_line, predicate)`` for cfgs inside a body.

    A symbol sitting in one of these is already conditional, so it must
    not be counted towards the enclosing item's unconditional needs. The
    extent runs to the close of whatever bracket the following line
    opens, and is that one line when it opens none.

    All three bracket kinds count, not just braces: the commonest cfg'd
    element in this tree is a *tuple* in a fixture array, whose rows are
    parenthesised rather than braced (`ops.rs`'s
    `ops_classifies_space_kind_once_per_space_1110`). Counting only
    `{}` ends the extent on the opening `(` and leaves every `LANG::`
    inside the row looking unconditional.
    """
    extents: list[tuple[int, int, str]] = []
    index = body_start
    while index <= body_end and index < len(lines):
        stripped = lines[index].strip()
        if not stripped.startswith(("#[", "#![")):
            index += 1
            continue
        attrs, after = _join_attributes(lines, raw, index)
        predicates = [p for p in (cfg_predicate(a) for a in attrs) if p is not None]
        feature_predicates = [p for p in predicates if "feature" in p]
        if not feature_predicates or after > body_end:
            index = max(after, index + 1)
            continue
        depth = _bracket_delta(lines[after])
        end = after
        while depth > 0 and end < body_end:
            end += 1
            depth += _bracket_delta(lines[end])
        for predicate in feature_predicates:
            extents.append((after, end, predicate))
        index = max(after, index + 1)
    return extents


def scan_source(
    source: str,
    path: str,
    table: dict[str, str],
    *,
    whole_file_is_test: bool = False,
) -> list[Item]:
    """Every ``mod`` / ``fn`` in ``source`` that sits in a test scope."""
    masked = mask_dead(source)
    lines = masked.split("\n")
    raw = source.split("\n")
    symbol_re = symbol_pattern(table)

    items: list[Item] = []
    stack: list[tuple[int, int]] = []  # (item index, brace depth on entry)
    depth = 0
    index = 0
    # A `macro_rules!` body is a token tree, not items: the `#[test] fn`
    # inside `roundtrip_tests!` is a template, and its `$lang`
    # metavariable names no language at all.
    macro_depth: int | None = None
    is_integration_test = path.startswith("tests/") or whole_file_is_test

    while index < len(lines):
        line = lines[index]
        stripped = line.strip()
        entry_depth = depth

        if macro_depth is not None:
            depth += line.count("{") - line.count("}")
            if depth <= macro_depth:
                macro_depth = None
            index += 1
            continue
        attrs: list[str] = []
        attr_line = index
        if stripped.startswith(("#[", "#![")):
            attrs, next_index = _join_attributes(lines, raw, index)
            if next_index >= len(lines):
                break
            index = next_index
            line = lines[index]
            stripped = line.strip()
            entry_depth = depth

        # After the attribute run, not before it. Reading `attrs` and
        # `attr_line` above their assignment raised `UnboundLocalError`
        # on any file whose first scanned line is a `macro_rules!`, left
        # an *attributed* macro unrecognised — so `--fix` wrote markers
        # into its transcriber body — and gave every recorded macro the
        # previous line's `attr_line`, putting its marker on the item
        # above it.
        declared_macro = MACRO_RULES_RE.search(line)
        if declared_macro:
            # The body is a token tree, not items -- the `#[test] fn`
            # inside `roundtrip_tests!` is a template whose `$lang`
            # metavariable names no language. The *definition* is still
            # an item that goes dead when every caller is gated away
            # (`unused macro definition: assert_no_string_matches`), so
            # it is recorded with an empty body and takes `any(users)`
            # from the reverse propagation like any other helper.
            if stack or is_integration_test:
                macro_end = index
                macro_running = line.count("{") - line.count("}")
                while macro_running > 0 and macro_end + 1 < len(lines):
                    macro_end += 1
                    macro_running += lines[macro_end].count("{") - lines[
                        macro_end
                    ].count("}")
                macro_body = "\n".join(lines[index : macro_end + 1])
                items.append(
                    Item(
                        path=path,
                        line=index + 1,
                        attr_line=attr_line + 1,
                        indent=line[: len(line) - len(line.lstrip())],
                        kind="macro",
                        name=declared_macro.group(1),
                        own_predicates=tuple(
                            p
                            for p in (cfg_predicate(a) for a in attrs)
                            if p is not None
                        ),
                        is_test_fn=False,
                        is_sweep=False,
                        needs_a_language_enabled=False,
                        is_public=False,
                        takes_lang_param=False,
                        # No `direct`: the languages a macro body names
                        # are `$lang` metavariables, not requirements.
                        # Its *references* are real, though — they are
                        # the only record that `assert_variants_is_string!`
                        # expands to `assert_variant_is_string`, without
                        # which that helper looks to have no callers at
                        # all and stays ungated while the helper *it*
                        # calls does not.
                        direct=frozenset(),
                        hardcoded=frozenset(),
                        hand_written=_hand_written_features(raw, attr_line),
                        calls=frozenset(
                            m.group(1) for m in CALL_RE.finditer(macro_body)
                        ),
                        references=frozenset(
                            m.group(1) for m in REFERENCE_RE.finditer(macro_body)
                        ),
                        parent_index=stack[-1][0] if stack else None,
                        index=len(items),
                        in_test_scope=(
                            items[stack[-1][0]].in_test_scope
                            if stack
                            else is_integration_test
                        ),
                    )
                )
            macro_depth = depth
            depth += line.count("{") - line.count("}")
            index += 1
            continue

        match = ITEM_RE.match(line)
        if match is None:
            # An inner `#![cfg(...)]` applies to the enclosing item, which
            # is already on the stack.
            if attrs and stack and any(a.startswith("#![") for a in attrs):
                owner = items[stack[-1][0]]
                extra = tuple(
                    p
                    for p in (cfg_predicate(a) for a in attrs if a.startswith("#!["))
                    if p is not None
                )
                if extra:
                    items[owner.index] = replace(
                        owner, own_predicates=owner.own_predicates + extra
                    )
            depth += line.count("{") - line.count("}")
            while stack and depth <= stack[-1][1]:
                stack.pop()
            index += 1
            continue

        predicates = tuple(
            p for p in (cfg_predicate(a) for a in attrs) if p is not None
        )
        parent_index = stack[-1][0] if stack else None
        in_test_scope = (
            is_integration_test
            or any(predicate_requires_test(p) for p in predicates)
            or (parent_index is not None and items[parent_index].in_test_scope)
        )

        # Body extent: find the opening brace, then brace-match to its
        # partner. The brace is not always on the header line -- a
        # wrapped signature puts it three or four lines down -- and
        # treating the header as the whole body makes the item look as
        # though it names no language and calls nothing. That silently
        # under-gates every such helper and, worse, gates the helpers
        # *it* calls more narrowly than itself (`E0425 cannot find
        # function assert_members_score`). A declaration (`mod bash;`,
        # a trait signature) reaches its `;` first and has no body.
        body_end = index
        opened = False
        parens = 0
        for probe in range(index, len(lines)):
            for char in lines[probe]:
                if char in "([":
                    parens += 1
                elif char in ")]":
                    parens -= 1
                elif char == "{" and parens <= 0:
                    opened = True
                    break
                elif char == ";" and parens <= 0:
                    break
            else:
                continue
            body_end = probe
            break
        if opened:
            running = 0
            for probe in range(body_end, len(lines)):
                running += lines[probe].count("{") - lines[probe].count("}")
                if running <= 0:
                    body_end = probe
                    break
                body_end = probe

        # The header through its closing paren: enough to see whether a
        # helper is parameterised on the language.
        signature = line
        paren = line.count("(") - line.count(")")
        probe = index
        while paren > 0 and probe + 1 <= body_end:
            probe += 1
            signature += lines[probe]
            paren += lines[probe].count("(") - lines[probe].count(")")

        body = "\n".join(lines[index : body_end + 1])
        is_test_fn = any(TEST_ATTR_RE.search(a) for a in attrs)
        # Guarded *positions*, not guarded features. Subtracting the
        # whole feature made a language named both unconditionally and
        # inside its own `#[cfg]` disappear from the needs entirely, so
        # the test got no gate and panicked without the grammar — the
        # unsafe direction.
        guarded: dict[int, list[str]] = {}
        for first, last, predicate in _inner_cfg_extents(lines, raw, index, body_end):
            for guarded_line in range(first, last + 1):
                guarded.setdefault(guarded_line, []).append(predicate)
        # A helper that maps one of its arguments onto a `LANG` is a
        # dispatcher, and its arms are options rather than requirements.
        # `tests/api/suppression_test.rs`'s `analyze_lang` picks the
        # language from a file extension, so a body-literal reading makes
        # all twenty of its tests look as though each needs C++,
        # JavaScript, Python and Rust at once. `takes_lang_param` cannot
        # see this one: the parameter is a `&str` path.
        dispatched = set()
        if not is_test_fn and "(" in signature:
            dispatched = {hit.start(1) for hit in DISPATCH_ARM_RE.finditer(body)}
        # Comparisons are skipped for every item, test or helper: the
        # enum variant exists without its grammar, so asking whether a
        # value *is* it never parses anything.
        for pattern in COMPARISON_RES:
            for hit in pattern.finditer(body):
                dispatched.add(hit.start(1))
        for hit in LANG_RECEIVER_RE.finditer(body):
            if hit.group(2) not in GRAMMAR_BEARING_METHODS:
                dispatched.add(hit.start(1))
        # And every remaining alternative of each `matches!`.
        for call in MATCHES_CALL_RE.finditer(body):
            group = balanced_group(body, call.end() - 1)
            if group is None:
                continue
            for variant in LANG_PATH_RE.finditer(group[0]):
                dispatched.add(call.end() + variant.start(1))
        # And each variant an equality assertion *compares* rather than
        # hands to the call under test.
        for call in ASSERT_CALL_RE.finditer(body):
            group = balanced_group(body, call.end() - 1)
            if group is None:
                continue
            args = group[0]
            for variant in LANG_PATH_RE.finditer(args):
                if _is_value_position(args, variant.start(1)):
                    dispatched.add(call.end() + variant.start(1))
        direct: set[str] = set()
        hardcoded: set[str] = set()
        body_offset = 0
        for line_number in range(index, body_end + 1):
            # Not `predicates`: that name already holds the item's own
            # `cfg`, and shadowing it here silently emptied
            # `own_predicates` for every item in the tree.
            inner_gates = guarded.get(line_number, ())
            for hit in symbol_re.finditer(lines[line_number]):
                if body_offset + hit.start() in dispatched:
                    continue
                feature = table[hit.group(0)]
                if any(not gate_admits(g, feature) for g in inner_gates):
                    continue
                direct.add(feature)
                if not hit.group(0).startswith("LANG::"):
                    hardcoded.add(feature)
            body_offset += len(lines[line_number]) + 1

        item = Item(
            path=path,
            line=index + 1,
            attr_line=attr_line + 1,
            indent=match.group("indent"),
            kind=match.group("kind"),
            name=match.group("name"),
            own_predicates=predicates,
            is_test_fn=is_test_fn,
            is_public=bool(match.group("visibility")),
            is_sweep=bool(SWEEP_RE.search(body)),
            needs_a_language_enabled=bool(
                ALL_LANGS_SWEEP_RE.search(body)
                and ENABLED_FILTER_RE.search(body)
                and NON_VACUITY_RE.search(body)
            ),
            takes_lang_param=(
                match.group("kind") == "fn" and bool(LANG_PARAM_RE.search(signature))
            ),
            direct=frozenset(direct),
            hardcoded=frozenset(hardcoded),
            hand_written=_hand_written_features(raw, attr_line),
            calls=frozenset(m.group(1) for m in CALL_RE.finditer(body)),
            references=frozenset(m.group(1) for m in REFERENCE_RE.finditer(body)),
            parent_index=parent_index,
            index=len(items),
            in_test_scope=in_test_scope,
        )
        items.append(item)
        depth += line.count("{") - line.count("}")
        if depth > entry_depth:
            stack.append((item.index, entry_depth))
        while stack and depth <= stack[-1][1]:
            stack.pop()
        index += 1

    # Every item is returned, not just the in-scope ones: `parent_index`
    # and `index` address this list, so filtering here would make a
    # parent link point at the wrong item -- or at the item itself, which
    # spins `effective_predicate` forever. Callers filter on
    # `in_test_scope` when reporting.
    return items


def rust_sources(root: pathlib.Path) -> list[pathlib.Path]:
    """Repo-relative ``.rs`` paths under the scan roots.

    The excluded directories are pruned during the walk rather than
    filtered afterwards: `tests/repositories/` holds whole checked-out
    repositories (DeepSpeech, pdf.js, serde), so a `rglob` that descends
    into them before discarding them takes minutes.
    """
    found: list[pathlib.Path] = []
    for scan_root in SCAN_ROOTS:
        base = root / scan_root
        if not base.is_dir():
            continue
        for directory, subdirectories, filenames in os.walk(base):
            here = pathlib.Path(directory).relative_to(root)
            subdirectories[:] = sorted(
                name
                for name in subdirectories
                if not any((here / name).is_relative_to(e) for e in EXCLUDED)
            )
            found.extend(
                here / name for name in sorted(filenames) if name.endswith(".rs")
            )
    return found


MOD_DECL_RE = re.compile(r"^[ \t]*(?:pub\s*(?:\([^)]*\)\s*)?)?mod\s+(\w+)\s*;")
PATH_ATTR_RE = re.compile(r'#\[\s*path\s*=\s*"([^"]+)"\s*\]')


def test_scope_files(root: pathlib.Path, sources: list[pathlib.Path]) -> set[str]:
    """Files that are wholly test modules by their declaration.

    `src/spaces_tests.rs` and its siblings hold nothing but tests, yet
    carry no `#[cfg(test)]` of their own: the marker is on the
    `#[cfg(test)] mod spaces_tests;` line in `lib.rs`. Reading that
    declaration is what puts their 94 top-level tests in scope --
    without it they are taken for production code and left ungated,
    which is 79 panics on a single-language leg.
    """
    scoped: set[str] = set()
    for relative in sources:
        text = (root / relative).read_text(encoding="utf-8")
        lines = mask_dead(text).split("\n")
        raw = text.split("\n")
        index = 0
        while index < len(lines):
            if not lines[index].strip().startswith(("#[", "#![")):
                index += 1
                continue
            attrs, after = _join_attributes(lines, raw, index)
            index = max(after, index + 1)
            if after >= len(lines):
                break
            declared = MOD_DECL_RE.match(lines[after])
            if declared is None:
                continue
            predicates = [cfg_predicate(a) for a in attrs]
            if not any(p and predicate_requires_test(p) for p in predicates):
                continue
            # `#[path = "…"]` renames the file the module comes from:
            # `src/spaces.rs` declares `#[path = "spaces_tests.rs"] mod
            # tests;`, so the name says `tests` and the file does not.
            override = PATH_ATTR_RE.search(" ".join(attrs))
            name = declared.group(1)
            candidates = (
                [relative.parent / override.group(1)]
                if override
                else [
                    relative.parent / f"{name}.rs",
                    relative.parent / name / "mod.rs",
                ]
            )
            for candidate in candidates:
                if (root / candidate).is_file():
                    scoped.add(candidate.as_posix())
    return scoped


def scan_tree(root: pathlib.Path, table: dict[str, str]) -> dict[str, list[Item]]:
    """Every in-scope item under the scan roots, keyed by file."""
    sources = rust_sources(root)
    scoped = test_scope_files(root, sources)
    found: dict[str, list[Item]] = {}
    for relative in sources:
        items = scan_source(
            (root / relative).read_text(encoding="utf-8"),
            relative.as_posix(),
            table,
            whole_file_is_test=relative.as_posix() in scoped,
        )
        if items:
            found[relative.as_posix()] = items
    return found


# ---------------------------------------------------------------------------
# Needed-set resolution
# ---------------------------------------------------------------------------


def hardcoded_closure(items: list[Item], item: Item) -> frozenset[str]:
    """Parsers ``item`` pins by type, its own and its helpers'.

    `needs` propagates through helpers and this has to as well, or the
    shape #1478 describes walks straight through: a sweep whose only
    hardcoded parser sits one call away reads as pinning nothing.
    """
    scopes = _helper_scopes(items)
    collected = set(item.hardcoded)
    seen = {item.index}
    queue = [item]
    while queue:
        current = queue.pop()
        for name in current.references:
            index = _resolve_call(items, scopes, current, name)
            if index is None or index in seen:
                continue
            callee = items[index]
            if not _uses(current, callee, strict=False):
                continue
            seen.add(index)
            collected |= callee.hardcoded
            queue.append(callee)
    return frozenset(collected)


def _helper_scopes(items: list[Item]) -> dict[int | None, dict[str, int]]:
    """Referenceable non-test items, by enclosing scope then by name.

    ``const`` and ``static`` are in here beside ``fn``: a fixture
    ``const SRC: &str`` used only by gated tests goes dead in a narrow
    build exactly like a helper does.
    """
    scopes: dict[int | None, dict[str, int]] = {}
    for item in items:
        if item.kind != "mod" and not item.is_test_fn:
            scopes.setdefault(item.parent_index, {}).setdefault(item.name, item.index)
    return scopes


def _uses(item: Item, referent: Item, *, strict: bool) -> bool:
    """Whether ``item``'s body references ``referent``.

    The two propagation directions want different answers, because they
    fail in opposite directions:

    * Widening a gate (a helper inheriting its callers' needs) is safe
      when over-linked, and *missing* a link leaves a helper ungated and
      dead on a narrow leg. So it takes the broad reading -- a `fn`
      handed over as a function pointer (``is_some_and(is_synthesised_
      name)``) never shows a ``(``.
    * Marking something unconditional is unsafe when over-linked: a
      local named ``score`` or ``find`` would keep the helper of that
      name ungated and unused on every single-language leg. So that one
      is ``strict`` and wants the call shape.
    """
    if strict and referent.kind == "fn":
        return referent.name in item.calls
    return referent.name in item.references


def _resolve_call(
    items: list[Item], scopes: dict[int | None, dict[str, int]], caller: Item, name: str
) -> int | None:
    """The helper ``name`` refers to from inside ``caller``.

    Resolution walks outwards from the calling scope, the way Rust's
    does. A file-wide lookup by bare name is not good enough here:
    `spaces_tests.rs` declares three different `analyse` helpers in three
    sibling modules, one of which hardcodes `RustParser`, so a flat map
    makes every Kotlin, Java, Ruby and Groovy test in the file look as
    though it needs Rust. Same shape for `conditions` in `abc.rs`.
    """
    # Start at the caller's *own* scope, not its parent's: a `fn` can
    # declare helpers inside itself, and `spaces_tests.rs` does exactly
    # that (`find` nested in `child`). Starting a level out never
    # resolves those, which leaves them with no callers, no inherited
    # needs, and an ungated look that then vetoes the gate on every
    # import they touch.
    scope: int | None = caller.index
    seen: set[int | None] = set()
    while scope not in seen:
        seen.add(scope)
        found = scopes.get(scope, {}).get(name)
        if found is not None and found != caller.index:
            return found
        if scope is None:
            return None
        scope = items[scope].parent_index
    return None


def resolve_needs(items: list[Item]) -> dict[int, frozenset[str]]:
    """The languages each item needs, by role.

    The three roles want three different answers, and conflating them
    produces gates that are confidently wrong in both directions:

    * A **test** needs the languages its own body names, plus those of
      any helper it calls that *hardcodes* a parser
      (``assert_csharp_fixture_spells``). A helper taking ``lang: LANG``
      is generic -- its body may mention `LANG::Rust` in one arm of a
      dispatch ``match`` -- and propagating that to callers marks every
      Kotlin test in `spaces_tests.rs` as needing Rust.
    * A **sweep** drives a per-language table that usually lives in a
      helper, so it does follow generic calls; the result is only ever
      used for the "enables at least one row" union check.
    * A **helper** is live as soon as any caller is, so it needs the
      union of its callers' needs -- the reverse direction.

    A ``mod`` then takes the union of everything inside it, which is
    what keeps its imports from going unused in a build that drops every
    test it holds.
    """
    scopes = _helper_scopes(items)

    def reachable(item: Item, generic_too: bool) -> set[str]:
        """Direct needs of ``item`` and of the helpers it calls."""
        collected = set(item.direct)
        seen = {item.index}
        queue: list[Item] = [item]
        while queue:
            current = queue.pop()
            for name in current.references:
                callee_index = _resolve_call(items, scopes, current, name)
                if callee_index is None or callee_index in seen:
                    continue
                callee = items[callee_index]
                # `strict`: a test's marker is an `all(...)`, so an
                # over-link here gates it more narrowly than it is used
                # and it silently stops running. A local, field or type
                # sharing a helper's name is enough to cause one.
                if not _uses(current, callee, strict=True):
                    continue
                seen.add(callee_index)
                if callee.takes_lang_param and not generic_too:
                    continue
                collected |= callee.direct
                queue.append(callee)
        return collected

    needs: dict[int, set[str]] = {item.index: set() for item in items}
    for item in items:
        if item.kind == "fn" and (item.is_test_fn or not item.takes_lang_param):
            needs[item.index] = reachable(item, generic_too=item.is_sweep)
            # A sweep over the whole enum hands every *enabled* variant
            # to whatever parses it, so it is live exactly when some
            # language is — which is also what its own `checked > 0`
            # guard asserts. Its fixtures come from a
            # `LANG`-parameterised helper, whose arms are deliberately
            # not attributed to callers, so the body names almost
            # nothing and whatever leaks through becomes the whole gate:
            # two `every_*_in_every_language` parity sweeps were gated
            # down to one `c-family-helpers`, and `--compare` was the
            # only check that saw it.
            #
            # Conditioned on the sweep carrying *both halves* of the
            # rule in `.claude/rules/testing.md`: an `is_enabled()` row
            # filter and a non-vacuity assertion. Together those mean it
            # *fails* rather than skips when no language is enabled, so
            # it must be absent then — the #1220 class. A full-enum
            # sweep with no such guard asserts over variants that exist
            # without their grammars (`Display`, `FromStr`, slug
            # round-trips, `is_enabled` itself); gating those would stop
            # them running on the `--no-default-features` leg that is
            # precisely where they belong.
            if item.is_test_fn and item.needs_a_language_enabled:
                needs[item.index] |= set(KNOWN_FEATURES)

    # Drop what an item's own gate already excludes, before any of it
    # propagates. `ast_seam_test.rs`'s
    # `ast_parse_returns_language_disabled_for_off_feature` is
    # `cfg(not(feature = "javascript"))` and names `LANG::Javascript`
    # precisely because the grammar is absent; leaving `javascript` in
    # its set gates the import it makes on the one feature under which
    # the test never exists (`E0433: cannot find type LANG`).
    for item in items:
        predicate = effective_predicate(items, item)
        if predicate is None:
            continue
        needs[item.index] = {
            feature
            for feature in needs[item.index]
            if not gate_excludes(predicate, feature)
        }

    # A referent with even one unconditional caller must stay
    # unconditional itself, whatever its other callers need. Without
    # this the reverse propagation below gates a helper on the union of
    # only its *language* callers and the language-agnostic test that
    # also calls it loses the definition -- seven `E0425 cannot find
    # function` errors on the `--features go` leg, in `args`,
    # `assert_members_score`, `render`, `on_stack` and friends.
    # Helpers inherit from their callers. Iterated to a fixpoint because
    # a helper's callers include other helpers.
    changed = True
    while changed:
        changed = False
        for item in items:
            for name in item.references:
                callee_index = _resolve_call(items, scopes, item, name)
                if callee_index is None or items[callee_index].is_test_fn:
                    continue
                if not _uses(item, items[callee_index], strict=False):
                    continue
                addition = needs[item.index] - needs[callee_index]
                if addition:
                    needs[callee_index] |= addition
                    changed = True

    # Only now, once every item knows what it needs, can "always
    # compiled" be read off: an item with an empty set is one `offenders`
    # will not gate, so everything it uses must stay ungated too.
    # Computing this *before* the propagation above misses the case that
    # matters — `checker.rs`'s `assert_variant_is_string` is reached only
    # from a macro body, so it inherits nothing and stays ungated while
    # the `count_string_matches_for_kind` it calls gets gated from its
    # other callers (`E0425` on the zero-language leg).
    #
    # `pub` joins the seed for the same reason: `offenders` never gates
    # one, because its users are in files this scanner cannot see.
    unconditional: set[int] = set()
    changed = True
    while changed:
        changed = False
        for item in items:
            # A container is not a user. A `mod`'s body spans every item
            # inside it, so letting one propagate marks everything it
            # holds as always-compiled — which is how `checker.rs`'s two
            # `assert_*_is_string` macros lost the 17-language gate their
            # actual callers give them and went dead on every leg.
            if not item.in_test_scope or item.kind == "mod":
                continue
            if item.index not in unconditional:
                if needs[item.index] and not item.is_public:
                    continue
                unconditional.add(item.index)
                changed = True
            for name in item.references:
                index = _resolve_call(items, scopes, item, name)
                if index is None or index in unconditional:
                    continue
                if not _uses(item, items[index], strict=True):
                    continue
                unconditional.add(index)
                needs[index] = set()
                changed = True

    for index in unconditional:
        needs[index] = set()

    # Containers last, innermost first, so a `mod` sees its children's
    # resolved sets rather than their raw ones.
    for item in reversed(items):
        if item.parent_index is not None:
            needs[item.parent_index] |= needs[item.index]
    return {index: frozenset(value) for index, value in needs.items()}


def _combine_gates(gates: list[str]) -> str:
    """``any(...)`` over each user's gate, deduped, flattened when it can be."""
    unique = list(dict.fromkeys(gates))
    if len(unique) == 1:
        return unique[0]
    # A disjunction of bare `feature = "x"` atoms is just their `any`,
    # which is what most of these are and much the easier read.
    atoms = [g for g in unique if g.startswith("feature")]
    if len(atoms) == len(unique):
        return "any(" + ", ".join(sorted(atoms)) + ")"
    return "any(" + ", ".join(unique) + ")"


def needed_features(
    items: list[Item], item: Item, needs: dict[int, frozenset[str]]
) -> frozenset[str]:
    """``item``'s needs, minus any its own gate deliberately excludes."""
    predicate = effective_predicate(items, item)
    return frozenset(f for f in needs[item.index] if not gate_excludes(predicate, f))


AST_CRATE_PREFIX = "big-code-analysis-ast/"


def feature_atom(feature: str, path: str) -> str:
    """The `cfg` atom asserting ``feature``'s grammar, as ``path`` sees it.

    `c-family-helpers` is spelled differently in the two crates, and
    getting it wrong is silent. In `big-code-analysis-ast` the
    `c` / `cpp` / `mozcpp` features each list `c-family-helpers`
    directly, so the bare atom is true whenever the helper grammars are
    compiled. In the root crate they list
    `big-code-analysis-ast/cpp` instead, which enables the *sub-crate's*
    copy and leaves the root's own feature off — and `all-languages`
    does not list it either. So a root-crate item gated on the bare atom
    is absent from a default build, taking its `Ccomment` / `Preproc`
    coverage with it, and `--all-features` hides that by enabling the
    feature explicitly.
    """
    if feature != C_FAMILY_HELPER_FEATURE or path.startswith(AST_CRATE_PREFIX):
        return f'feature = "{feature}"'
    enablers = sorted(IMPLIES_C_FAMILY_HELPERS | {feature})
    return "any(" + ", ".join(f'feature = "{f}"' for f in enablers) + ")"


def _render(features: frozenset[str], path: str, combinator: str) -> str:
    atoms = [feature_atom(f, path) for f in sorted(features)]
    if combinator == "any":
        # `feature_atom` returns an `any(...)` of its own for the root
        # crate's `c-family-helpers`. Nesting that inside an `any` is a
        # disjunction of a disjunction — the same predicate, spelled with
        # `c`, `cpp` and `mozcpp` listed twice.
        flattened: list[str] = []
        for atom in atoms:
            group = (
                balanced_group(atom, atom.index("("))
                if atom.startswith("any(")
                else None
            )
            flattened.extend(split_top_level(group[0]) if group else [atom])
        atoms = sorted(dict.fromkeys(flattened))
    if len(atoms) == 1:
        return atoms[0]
    return f"{combinator}(" + ", ".join(atoms) + ")"


def required_marker(
    items: list[Item], item: Item, needs: dict[int, frozenset[str]]
) -> str | None:
    """The ``cfg`` predicate ``item`` should carry, or ``None`` if any.

    A ``#[test] fn`` uses every language it names, so it needs them all.
    Everything else -- a helper, a containing ``mod`` -- is live as soon
    as one caller is, so it needs any.
    """
    required = needed_features(items, item, needs)
    if not required:
        return None
    # Only a plain test needs *every* language it names. A sweep skips
    # the disabled ones at run time, so it needs any one of its rows —
    # `all(...)` there gates the 23-language parity suites out of every
    # build but the full one, and takes their helpers dead with them.
    if item.is_test_fn and not item.is_sweep:
        return _render(required, item.path, "all")

    # A sweep that also pins a parser by type needs both halves: `all` of
    # what it pins, `any` of what it iterates. Without this the marker
    # `--fix` writes can never satisfy the check that asked for it, and
    # the fixpoint loop stacks one copy per pass before giving up.
    if item.is_test_fn and item.is_sweep:
        pinned = hardcoded_closure(items, item) & required
        if pinned:
            rest = required - pinned
            parts = [feature_atom(f, item.path) for f in sorted(pinned)]
            if rest:
                parts.append(_render(rest, item.path, "any"))
            return parts[0] if len(parts) == 1 else "all(" + ", ".join(parts) + ")"

    # A `macro_rules!` takes the disjunction of its callers' *gates*
    # rather than of the features inside them, because flattening loses
    # a conjunction: `checker.rs`'s `assert_variants_is_string!` has one
    # caller, a test needing all seventeen languages at once, and
    # `any(…seventeen…)` leaves the macro defined on a build with one of
    # them and nothing to invoke it (`unused macro definition`).
    #
    # Only macros. Applied to ordinary helpers the same rule gates them
    # more narrowly than they are used, because this sees only the
    # callers it can resolve by name and a helper reached another way
    # then loses its definition (`E0425: cannot find function analyse`).
    # An over-wide `any(features)` on a helper is merely an unused
    # warning, which the crate-level `cfg_attr` already covers.
    caller_gates: list[str] = []
    if item.kind == "macro":
        # Hoisted: rebuilding the scope index per candidate caller is
        # `O(users x n)`, and it only looked cheap because `and`
        # short-circuits before it for everything that is not a user.
        scopes = _helper_scopes(items)
        caller_gates = [
            gate
            for gate in (
                _caller_marker(items, other, needs)
                for other in items
                if other.index != item.index
                and other.in_test_scope
                and other.kind != "mod"
                and _uses(other, item, strict=False)
                and _resolve_call(items, scopes, other, item.name) == item.index
            )
            if gate
        ]
    if caller_gates:
        return _combine_gates(caller_gates)
    return _render(required, item.path, "any")


def _caller_marker(
    items: list[Item], item: Item, needs: dict[int, frozenset[str]]
) -> str | None:
    """``item``'s gate as a caller: declared, else derived for a test.

    Only tests are derived here. Recursing into another helper would
    need cycle handling for no gain: a helper reached only through other
    helpers still bottoms out at the tests that drive them, and those
    already carry their own markers by the time this is asked.
    """
    own = [
        p
        for p in item.own_predicates
        if any(not gate_admits(p, f) for f in KNOWN_FEATURES)
    ]
    if own:
        return own[0] if len(own) == 1 else "all(" + ", ".join(own) + ")"
    if item.is_test_fn:
        required = needed_features(items, item, needs)
        if not required:
            return None
        return _render(required, item.path, "all")
    return None


def over_gated(
    items: list[Item], needs: dict[int, frozenset[str]]
) -> list[tuple[Item, frozenset[str]]]:
    """Items whose own `cfg` requires a feature they never use.

    The mirror of `offenders`, and the direction nothing else can see: a
    gate that is too *wide* panics on the leg that lacks the grammar, but
    one that is too *narrow* just drops the test, and a leg running two
    hundred fewer tests looks exactly like a green run (#1478).

    A feature is over-declared when turning *it alone* off is what
    excludes the item, and the item needs neither it nor anything that
    enables it. Both halves matter:

    * Off alone, not off with its closure. A `cpp`-gated test is also
      excluded from a build without `c-family-helpers` — but only
      because such a build has no `cpp` either, and reading that as a
      requirement makes every C-family gate report a helper feature it
      never spells.
    * Nothing the item needs may enable it. In the ast crate `cpp`
      lists `c-family-helpers`, so a test needing `cpp` and gated on the
      helper is excluded from no build it could have run in.

    The reference build is all features on *except* the ones the gate
    requires to be off, so a `cfg(not(feature = "javascript"))`
    disabled-path test is measured from a build it actually compiles in.
    Testing against a bare all-on baseline instead exempted the whole
    item, and `all(not(python), rust, go)` then hid its `go`.

    An `any(…)` requires no feature on its own, so a sweep's union gate
    never reports here.
    """
    found: list[tuple[Item, frozenset[str]]] = []
    for item in items:
        unused = over_declared(item, needs) - item.hand_written
        if unused:
            found.append((item, unused))
    return found


def over_declared(item: Item, needs: dict[int, frozenset[str]]) -> frozenset[str]:
    """The features ``item``'s own gate requires but its body never uses.

    Before the marker subtraction, so `stale_markers` can ask the same
    question of the same answer rather than keeping a second copy of the
    rule in step with this one.
    """
    if not item.in_test_scope:
        return frozenset()
    # A `pub` item's users are in files this single-file scanner never
    # sees, so `needs` is only the subset visible here and every gate
    # placed for an out-of-file caller reads as over-declared.
    # `offenders` skips them for the mirror-image reason.
    if item.is_public:
        return frozenset()
    own = [p for p in item.own_predicates if "feature" in p]
    if not own:
        return frozenset()
    conjunction = own[0] if len(own) == 1 else "all(" + ", ".join(own) + ")"
    required = needs[item.index]
    implied = frozenset().union(*(enabled_closure(f) for f in required), frozenset())
    reference = negated_features(conjunction)
    return frozenset(
        feature
        for feature in KNOWN_FEATURES
        if feature not in implied
        and feature not in reference
        and not evaluate_predicate(conjunction, reference | {feature})
    )


def stale_markers(
    items: list[Item], needs: dict[int, frozenset[str]]
) -> list[tuple[Item, frozenset[str]]]:
    """Markers naming a feature that is no longer over-declared.

    A stale marker silences nothing — there is nothing left to silence —
    so it cannot cause a wrong verdict on its own. It is still a lie
    about why the gate is the shape it is, and the next reader has no
    way to tell a load-bearing entry from a leftover. Making it fail is
    what keeps the accepted gates readable as a census rather than
    accumulating into one.
    """
    return [
        (item, stale)
        for item in items
        if (stale := item.hand_written - over_declared(item, needs))
    ]


def offenders(
    items: list[Item], needs: dict[int, frozenset[str]]
) -> list[tuple[Item, frozenset[str]]]:
    """Items admitted into a build lacking a language they need.

    A sweep is exempt from the per-feature test: it skips disabled
    languages at runtime. It still has to be excluded from a build
    enabling *none* of them, which is the union check in the else branch
    and the half `check-feature-gates.py` verifies against a real build.
    """
    found: list[tuple[Item, frozenset[str]]] = []
    for item in items:
        # Production code names these types unconditionally and must
        # stay ungated: only the test scope is this gate's business.
        if not item.in_test_scope:
            continue
        # Containers are deliberately not gated. Gating `mod tests` on
        # the union of its contents would be the cheap way to drop its
        # imports and shims in a build with no grammars at all, but it
        # takes 134 language-agnostic tests down with them -- 34 in
        # `suppression.rs` alone, which never parses anything. Those
        # tests are exactly what the `--no-default-features` leg is for,
        # so the module stays open and its helpers, shims and
        # language-bound imports carry their own markers instead.
        if item.kind == "mod":
            continue
        # A `pub` / `pub(crate)` item is reachable from files this
        # single-file scanner never sees. `test_support.rs` is the whole
        # story: its `pub(crate) use` re-export feeds 45 call sites in
        # seven modules, and gating it on the users visible *here* drops
        # the name from every build the other six need it in
        # (`E0432 unresolved import`). Gate those by hand, the way
        # `assert_perl_fixture_spells` already is.
        if item.is_public:
            continue
        required = needed_features(items, item, needs)
        if not required:
            continue
        predicate = effective_predicate(items, item)
        if item.is_sweep or not item.is_test_fn:
            # A sweep skips disabled languages at run time, so the rows it
            # iterates need only `any`. A parser it *hardcodes* is a
            # different thing: `check_metrics::<PythonParser>` picks its
            # grammar through a type parameter, which no runtime filter
            # can skip, so that subset is required outright even here.
            # Without this the exemption is a way round the whole gate.
            # Sweeps only. A *helper* that hardcodes a parser is still
            # correctly gated `any(callers)`: it exists in a build where
            # one caller does, and is simply never called there. Demanding
            # `all` of it gates the helper out from under callers that do
            # need it.
            pinned = frozenset(
                f
                for f in hardcoded_closure(items, item) & required
                if item.is_test_fn and gate_admits(predicate, f)
            )
            if pinned:
                found.append((item, pinned))
                continue
            disabled = frozenset().union(*(disabled_closure(f) for f in required))
            if predicate is None or evaluate_predicate(predicate, disabled):
                found.append((item, required))
            continue
        missing = frozenset(f for f in required if gate_admits(predicate, f))
        if missing:
            found.append((item, missing))
    return found


# ---------------------------------------------------------------------------
# Rewriting
# ---------------------------------------------------------------------------


def wrap_marker(predicate: str, indent: str) -> list[str]:
    """``#[cfg(...)]`` lines for ``predicate``, wrapped like rustfmt would.

    Emitted above the rest of the attribute run so the marker sits on the
    function's attribute stack rather than between ``#[test]`` and the
    signature, and so the whole run reads gate-then-test in every file.
    """
    single = f"{indent}#[cfg({predicate})]"
    if len(single) <= MAX_WIDTH:
        return [single]
    open_paren = predicate.find("(")
    group = None if open_paren == -1 else balanced_group(predicate, open_paren)
    if group is None:
        return [single]
    head = predicate[:open_paren]
    lines = [f"{indent}#[cfg({head}("]
    lines.extend(f"{indent}    {part}," for part in split_top_level(group[0]))
    lines.append(f"{indent}))]")
    return lines


def apply_fixes(
    path: pathlib.Path,
    items: list[Item],
    needs: dict[int, frozenset[str]],
    missing: list[Item],
) -> int:
    """Insert the derived marker above each item in ``missing``.

    Rewrites bottom-up so an earlier insertion cannot shift a later
    item's recorded line.
    """
    lines = path.read_text(encoding="utf-8").split("\n")
    inserted = 0
    for item in sorted(missing, key=lambda i: -i.attr_line):
        predicate = required_marker(items, item, needs)
        if predicate is None:
            continue
        lines[item.attr_line - 1 : item.attr_line - 1] = wrap_marker(
            predicate, item.indent
        )
        inserted += 1
    if inserted:
        path.write_text("\n".join(lines), encoding="utf-8")
    return inserted


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def scan_revision(ref: str, root: pathlib.Path) -> dict[str, list[Item]]:
    """Scan the tree as of ``ref``, extracted to a scratch directory.

    `git archive` rather than a worktree: it touches nothing in the
    checkout, needs no lock, and costs about two tenths of a second.
    """
    checkout = pathlib.Path(tempfile.mkdtemp(prefix="check-test-lang-gates-"))
    try:
        archive = subprocess.run(
            ["git", "archive", ref],
            cwd=root,
            capture_output=True,
            check=False,
        )
        if archive.returncode != 0:
            raise ScanError(
                f"`git archive {ref}` failed: {archive.stderr.decode().strip()}"
            )
        subprocess.run(
            ["tar", "-x", "-C", str(checkout)],
            input=archive.stdout,
            check=True,
        )
        table = language_table(
            (checkout / LANGS_RS.relative_to(REPO_ROOT)).read_text(encoding="utf-8")
        )
        return scan_tree(checkout, table)
    finally:
        shutil.rmtree(checkout, ignore_errors=True)


def compare_revisions(
    ref: str, root: pathlib.Path, table: dict[str, str]
) -> list[tuple[str, frozenset[str]]]:
    """Tests that still exist but stopped compiling somewhere.

    The one direction no static check can reach on its own. `over_gated`
    compares a marker against the derivation; when the two agree and are
    both wrong, only the previous revision says so.
    """
    probes = probe_builds(table)
    reference_tree = scan_revision(ref, root)
    # The same guard `main` applies to the working tree. A reference with
    # no tests in it makes every name miss the `after` side, so nothing
    # is ever reported and the check prints OK having compared nothing —
    # an `export-ignore` on a scan root, or a ref predating one, is all
    # it takes.
    if not any(
        item.is_test_fn and item.in_test_scope
        for items in reference_tree.values()
        for item in items
    ):
        raise ScanError(
            f"the tree at {ref} has no tests in it — nothing to compare "
            "against. A scan root is missing from that revision, or is "
            "excluded from `git archive` by `.gitattributes`."
        )
    before = membership(reference_tree, probes)
    after = membership(scan_tree(root, table), probes)
    lost: list[tuple[str, frozenset[str]]] = []
    for name, was in sorted(before.items()):
        # Only a test that is still here. One that was deleted or renamed
        # is a deliberate change, not a gate that narrowed under it.
        if name not in after:
            continue
        dropped = was - after[name]
        if dropped:
            lost.append((name, dropped))
    return lost


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="check-test-lang-gates",
        description=__doc__.splitlines()[0],
        epilog=(
            "Every test naming a concrete parser must be absent from a build "
            "without that language's grammar, not present and panicking."
        ),
    )
    parser.add_argument(
        "--show",
        action="store_true",
        help="Print every in-scope item with its derived marker and exit 0.",
    )
    parser.add_argument(
        "--fix",
        action="store_true",
        help="Write the missing markers in place and exit 0.",
    )
    parser.add_argument(
        "--compare",
        metavar="REF",
        help=(
            "Fail if a test that still exists stopped compiling under any "
            "single-language build since REF."
        ),
    )
    parser.add_argument(
        "--root",
        type=pathlib.Path,
        default=REPO_ROOT,
        help=argparse.SUPPRESS,
    )
    args = parser.parse_args(argv)

    try:
        table = language_table(
            (args.root / LANGS_RS.relative_to(REPO_ROOT)).read_text(encoding="utf-8")
        )
        per_file = scan_tree(args.root, table)
    except (ScanError, OSError, UnicodeDecodeError) as exc:
        sys.stderr.write(f"error: {exc}\n")
        return 2

    # Production items are scanned too, so a bare `if not per_file` would
    # be satisfied by a tree of pure library code and let the gate report
    # OK having checked nothing. Count the thing it exists to find.
    if not any(
        item.is_test_fn and item.in_test_scope
        for items in per_file.values()
        for item in items
    ):
        sys.stderr.write(
            "error: no tests found under "
            f"{', '.join(str(r) for r in SCAN_ROOTS)}\n"
            "       the scanner is broken or the tree is wrong; this gate\n"
            "       cannot pass by finding nothing to check.\n"
        )
        return 2

    if args.compare:
        try:
            lost = compare_revisions(args.compare, args.root, table)
        except (
            ScanError,
            OSError,
            UnicodeDecodeError,
            subprocess.CalledProcessError,
        ) as exc:
            sys.stderr.write(f"error: {exc}\n")
            return 2
        if lost:
            sys.stderr.write(
                f"error: {len(lost)} test(s) stopped compiling somewhere they "
                f"used to, since {args.compare}\n\n"
            )
            for name, dropped in lost:
                sys.stderr.write(f"  {name}\n")
                for build in sorted(dropped):
                    sys.stderr.write(f"      no longer built by: {build}\n")
            sys.stderr.write(
                "\nEach of these still exists and still passes under "
                "`--all-features`, so\nnothing else reports it: a gate too "
                "*wide* panics on the leg that lacks\nthe grammar, but one "
                "too *narrow* just drops the test and the leg stays\ngreen. "
                "Widen the gate back to what it was.\n\nIf the narrowing is "
                "deliberate — the gate really was too wide and the test\ndoes "
                "not need that grammar — say so on the pull request with the\n"
                "`gate-narrowing-intended` label. Not with an in-source "
                "marker: this\ncompares against the merge base, so a marker "
                "would be stale the moment\nthe branch lands and would sit in "
                "the tree as a hole nothing detects.\n\n"
                "See `.claude/rules/testing.md` (#1478).\n"
            )
            return 1
        print(f"test-lang-gates: OK — no test lost a build since {args.compare}")
        return 0

    if args.show:
        # Only the items a marker was derived for. Printing every scanned
        # item buries those under thousands of `needs -` lines, most of
        # them production code the gate never looks at.
        for items in per_file.values():
            needs = resolve_needs(items)
            for item in items:
                if not item.in_test_scope:
                    continue
                marker = required_marker(items, item, needs)
                if marker is not None:
                    print(f"{item.describe()}  needs {marker}")
        return 0

    if args.fix:
        # To a fixpoint. Gating one item changes what the items around
        # it need -- an import takes the union of its users, and a user
        # that just acquired a marker contributes differently -- so a
        # single pass can leave a marker one feature short and the next
        # run stacks a second `#[cfg]` on top of the first rather than
        # widening it.
        fixed = 0
        for _ in range(FIX_PASSES):
            pass_fixed = 0
            try:
                per_pass = scan_tree(args.root, table).items()
                for relative, items in per_pass:
                    needs = resolve_needs(items)
                    found = offenders(items, needs)
                    if found:
                        pass_fixed += apply_fixes(
                            args.root / relative, items, needs, [i for i, _ in found]
                        )
            except ScanError as exc:
                sys.stderr.write(f"error: {exc}\n")
                return 2
            fixed += pass_fixed
            if pass_fixed == 0:
                break
        else:
            sys.stderr.write(
                f"error: still not clean after {FIX_PASSES} passes; the "
                "derivation is not converging\n"
            )
            return 2
        print(f"test-lang-gates: inserted {fixed} marker(s)")
        return 0

    checked = 0
    failures: list[tuple[str, Item, frozenset[str]]] = []
    wider: list[tuple[str, Item, frozenset[str]]] = []
    stale: list[tuple[str, Item, frozenset[str]]] = []
    try:
        for relative, items in per_file.items():
            needs = resolve_needs(items)
            checked += sum(
                1
                for item in items
                if item.in_test_scope and needed_features(items, item, needs)
            )
            failures.extend(
                (relative, item, why) for item, why in offenders(items, needs)
            )
            wider.extend(
                (relative, item, why) for item, why in over_gated(items, needs)
            )
            stale.extend(
                (relative, item, why) for item, why in stale_markers(items, needs)
            )
    except ScanError as exc:
        sys.stderr.write(f"error: {exc}\n")
        return 2

    # Both directions, always. They are independent defects in
    # independent items, and short-circuiting on the first hid the other
    # until the tree happened to be clean of it.
    if wider:
        sys.stderr.write(
            f"error: {len(wider)} test item(s) require a feature they never use\n\n"
        )
        for relative, item, why in wider:
            sys.stderr.write(
                f"  {item.describe()}\n      gated on, but never uses: "
                f"{', '.join(sorted(why))}\n"
            )
        sys.stderr.write(
            "\nA gate wider than the item needs keeps it out of builds it "
            "could run in,\nand nothing else notices: too *wide* panics on "
            "the leg that lacks the\ngrammar, too *narrow* just drops the "
            "test and the leg still looks green.\n\n"
            "Either narrow the gate, or — when the reason is one the "
            "derivation cannot\nsee, such as a language chosen from a glob "
            "or a path — say so above it:\n\n"
            "    // test-lang-gates: hand-written(cpp) — the corpus walk "
            "picks the\n    //     language per file from its extension\n\n"
            "See `.claude/rules/testing.md` (#1478).\n"
        )
        if failures:
            sys.stderr.write("\n")

    if failures:
        sys.stderr.write(
            f"error: {len(failures)} test item(s) name a language their `cfg` "
            "does not require\n\n"
        )
        for relative, item, why in failures:
            names = ", ".join(sorted(why))
            sys.stderr.write(f"  {item.describe()}\n      unguarded: {names}\n")
        sys.stderr.write(
            "\nEach of these compiles into a build without the grammar it "
            "names and\npanics in `Tree::new`. Add the marker the derivation "
            "reports:\n\n"
            "    ./utils/check-test-lang-gates.py --show\n"
            "    ./utils/check-test-lang-gates.py --fix\n\n"
            'See `.claude/rules/testing.md`, "Gate a feature-gated fixture '
            'table on the\nunion of its rows" (#1472, #1413).\n'
        )

    if stale:
        sys.stderr.write(
            f"error: {len(stale)} hand-written marker(s) name a feature the "
            "gate no longer\nover-declares\n\n"
        )
        for relative, item, why in stale:
            sys.stderr.write(
                f"  {item.describe()}\n      no longer over-declared: "
                f"{', '.join(sorted(why))}\n"
            )
        sys.stderr.write(
            "\nThe marker silences nothing — there is nothing left to "
            "silence — but the\nnext reader cannot tell a load-bearing "
            "entry from a leftover. Drop the\nfeature from the marker, or "
            "the whole marker if it names nothing else.\n"
        )

    if wider or failures or stale:
        return 1

    print(f"test-lang-gates: OK — {checked} gated test item(s) checked")
    return 0


if __name__ == "__main__":
    sys.exit(main())
