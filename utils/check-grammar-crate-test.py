#!/usr/bin/env python3
"""Tests for check-grammar-crate.py.

The headline test re-derives the grammar -> extension mapping from the
single source of truth (`src/langs.rs` `mk_langs!`) and asserts the
hand-maintained `EXTENSIONS` table matches it exactly. This is the
anti-drift guard for #869: the old table had gone stale across the
#507 / #720 / #721 / #724 refactors (wrong files, a wrong crate-name
key, phantom globs), so a grammar bump tested the wrong file set.

Run with:
    python3 -m unittest -q utils/check-grammar-crate-test.py
"""

from __future__ import annotations

import importlib.util
import pathlib
import re
import unittest

# The gate under test is a sibling in `utils/`; every path it reads
# or writes is anchored at the repository root one level above.
UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-grammar-crate.py"
LANGS_RS = REPO_ROOT / "src" / "langs.rs"

# The tree-sitter function token in `mk_langs!` maps 1:1 to the grammar
# crate name (underscores -> hyphens) for every variant EXCEPT the Tsx
# variant, which ships in the `tree-sitter-typescript` crate rather than
# a `tree-sitter-tsx` crate. A grammar bump names the crate, so the
# table is keyed by crate name; the alias records that one exception.
_FN_TO_CRATE_ALIAS = {"tree_sitter_tsx": "tree-sitter-typescript"}

# Grammar functions for internal C-family helpers that own no file
# extension and are never the subject of a standalone grammar bump in
# this script (they are pulled by the C-family feature). They have empty
# extension lists in `mk_langs!`, so excluding them keeps the derived
# mapping aligned with `EXTENSIONS`, which also omits them.
_HELPER_FNS = {"tree_sitter_ccomment", "tree_sitter_preproc"}


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("cgc", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cgc = _load_module()


def _strip_line_comment(line: str) -> str:
    """Drop a `//` line comment, ignoring `//` inside string literals."""
    in_string = False
    escaped = False
    for idx in range(len(line) - 1):
        ch = line[idx]
        if in_string:
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == '"':
                in_string = False
            continue
        if ch == '"':
            in_string = True
            continue
        if ch == "/" and line[idx + 1] == "/":
            return line[:idx]
    return line


def _mk_langs_tuples(block: str) -> list[str]:
    """Split the `mk_langs!( … )` body into top-level `( … )` tuples."""
    tuples: list[str] = []
    depth = 0
    current: list[str] = []
    in_string = False
    escaped = False
    for ch in block:
        if in_string:
            current.append(ch)
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == '"':
                in_string = False
            continue
        if ch == '"':
            in_string = True
            if depth >= 1:
                current.append(ch)
            continue
        if ch == "(":
            depth += 1
            if depth == 1:
                current = []
                continue
        if ch == ")":
            depth -= 1
            if depth == 0:
                tuples.append("".join(current))
                continue
        if depth >= 1:
            current.append(ch)
    return tuples


def _derive_extensions_from_langs_rs() -> dict[str, list[str]]:
    """Re-derive the grammar -> extension-glob table from src/langs.rs."""
    text = "\n".join(
        _strip_line_comment(line) for line in LANGS_RS.read_text().splitlines()
    )
    opener = re.search(r"mk_langs!\s*\(", text)
    assert opener is not None, "mk_langs! macro not found in src/langs.rs"
    start = opener.end() - 1
    depth = 0
    end = start
    for j in range(start, len(text)):
        if text[j] == "(":
            depth += 1
        elif text[j] == ")":
            depth -= 1
            if depth == 0:
                end = j
                break
    block = text[start + 1 : end]

    derived: dict[str, set[str]] = {}
    for tup in _mk_langs_tuples(block):
        ext_match = re.search(r"\[([^\]]*)\]", tup)
        if ext_match is None:
            continue
        exts = [e.strip() for e in ext_match.group(1).split(",") if e.strip()]
        # The tree-sitter function token is the last identifier before
        # the extension list.
        idents = re.findall(r"[A-Za-z_][A-Za-z0-9_]*", tup[: ext_match.start()])
        fn = idents[-1]
        if fn in _HELPER_FNS:
            continue
        crate = _FN_TO_CRATE_ALIAS.get(fn, fn.replace("_", "-"))
        derived.setdefault(crate, set()).update(f"*.{e}" for e in exts)

    return {crate: sorted(globs) for crate, globs in derived.items()}


class ExtensionsSyncTest(unittest.TestCase):
    def test_table_matches_langs_rs(self) -> None:
        derived = _derive_extensions_from_langs_rs()
        table = {crate: sorted(globs) for crate, globs in cgc.EXTENSIONS.items()}
        self.assertEqual(
            table,
            derived,
            "check-grammar-crate.py EXTENSIONS has drifted from "
            "src/langs.rs mk_langs!; update the table (see #869)",
        )

    def test_every_key_is_a_real_grammar_crate(self) -> None:
        # Each EXTENSIONS key must be a dependency in the root Cargo.toml
        # (the grammar crate names a bump would use). This catches the
        # `tree-sitter-kotlin` vs `tree-sitter-kotlin-ng` class of typo
        # that made a real bump get rejected as "not a valid grammar".
        cargo = (REPO_ROOT / "Cargo.toml").read_text()
        for crate in cgc.EXTENSIONS:
            self.assertRegex(
                cargo,
                rf"(?m)^{re.escape(crate)}\s*=",
                f"{crate} is not a dependency in the root Cargo.toml",
            )

    def test_no_phantom_globs(self) -> None:
        # `*.js2` (an emacs mode, not an extension) and `*.cx` (no such
        # extension) were phantom globs in the stale table. The derived
        # set never contains them, so the sync test already guards this;
        # assert it explicitly as a named regression.
        all_globs = {g for globs in cgc.EXTENSIONS.values() for g in globs}
        self.assertNotIn("*.js2", all_globs)
        self.assertNotIn("*.cx", all_globs)


class GrammarExtensionsHelperTest(unittest.TestCase):
    def test_mozcpp_resolves_to_cpp_set(self) -> None:
        # mozcpp owns no extension of its own; a CI bump must still have
        # files to diff over, so it borrows the upstream C++ set.
        self.assertEqual(
            cgc.grammar_extensions("tree-sitter-mozcpp"),
            cgc.EXTENSIONS["tree-sitter-cpp"],
        )
        # The table entry itself stays empty (it mirrors langs.rs).
        self.assertEqual(cgc.EXTENSIONS["tree-sitter-mozcpp"], [])

    def test_mozjs_globs_only_jsm(self) -> None:
        # Post-#507 the fork owns only `.jsm`; the standard JS extensions
        # belong to tree-sitter-javascript and must not be globbed for a
        # mozjs bump.
        self.assertEqual(cgc.grammar_extensions("tree-sitter-mozjs"), ["*.jsm"])
        self.assertNotIn("*.js", cgc.grammar_extensions("tree-sitter-mozjs"))

    def test_passthrough_for_normal_grammar(self) -> None:
        self.assertEqual(
            cgc.grammar_extensions("tree-sitter-rust"),
            cgc.EXTENSIONS["tree-sitter-rust"],
        )


if __name__ == "__main__":
    unittest.main()
