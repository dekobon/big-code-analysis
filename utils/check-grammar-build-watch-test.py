#!/usr/bin/env python3
"""Tests for check-grammar-build-watch.py.

Three kinds of test, matching the check-safety-doc-pin-test.py pattern:

* Unit tests over the comment stripper and the manifest reader, weighted
  toward the false-clean direction: a stripper that eats the block's own
  string literals, or a reader that returns an empty crate list, both
  make the gate report a clean tree.
* ``main()`` tests over a scratch repository root built from a minimal
  ``Cargo.toml`` and one ``build.rs`` per crate, driven through
  ``--root``. Every negative row is a shape a regeneration or a tidy-up
  could plausibly leave behind, and each asserts on the message rather
  than the exit code alone, so a gate that fails for the wrong reason is
  still caught.
* A smoke test running the real gate against the real repository.

Verified by perturbation (#1778): with ``main()`` patched to return 0
before it reads anything (an edit to the file on disk, so the
``RealRepository`` subprocess smoke sees it too), every ``Main`` row
that asserts a failure goes red, plus the one ``Main`` row and the one
``RealRepository`` row that assert the ``OK`` line — 13 of 27; the
``canonical_block_is_what_the_tcl_script_carries`` row never calls
``main`` and stays green; with the block deleted from the live
``tree-sitter-mozjs`` script the real gate names that crate, prints
``block absent`` and exits 1. Reverting the crate filter from a
``tree-sitter`` substring to a ``tree-sitter-`` prefix fails exactly
``test_published_name_is_in_scope`` (stage-D M1). The stripper's
literal rules each have a row that fails when the rule is removed: no
``\\`` skip inside a string, a greedy ``'…'`` char-literal regex, a raw
string lexed as an ordinary one, and the ``/* */`` arm disabled — the
last also takes down the three rows whose fixture or expectation
carries a block comment, so the commented-out-block row is the
discriminating one by name rather than by count.

Run with:
    python3 -B -m unittest -q utils/check-grammar-build-watch-test.py
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import pathlib
import subprocess
import sys
import tempfile
import unittest

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-grammar-build-watch.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_grammar_build_watch", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GATE = _load_module()

CRATES = ["tree-sitter-alpha", "tree-sitter-beta"]

MANIFEST = """\
[workspace]
members = ["big-code-analysis-cli"]
exclude = [
  "enums",
  "fuzz",
  "tree-sitter-alpha",
  "tree-sitter-beta",
]
"""

# The live block as #1773 wrote it, with its explanatory comment, in
# the tree-sitter-tcl shape (`std::path::Path::new`, no `use`).
PREAMBLE = """\
fn main() {
    let src_dir = std::path::Path::new("src");

    // The parser and scanner include the headers under `src/tree_sitter/`
    // (`parser.h`, and `alloc.h` / `array.h` where present). Watching only
    // the two translation units below left a header-only edit with a stale
    // object while the CLI's build-inputs digest (#1773) moved on — a
    // binary advertising a tree it did not compile.
"""

WATCH = GATE.CANONICAL_BLOCK

TAIL = """\

    let mut c_config = cc::Build::new();
    c_config.include(src_dir);
    let parser_path = src_dir.join("parser.c");
    c_config.file(&parser_path);
    println!("cargo:rerun-if-changed={}", parser_path.to_str().unwrap());
    let scanner_path = src_dir.join("scanner.c");
    c_config.file(&scanner_path);
    println!("cargo:rerun-if-changed={}", scanner_path.to_str().unwrap());
    c_config.compile("parser");
}
"""

GOOD = PREAMBLE + WATCH + TAIL


class StripComments(unittest.TestCase):
    """What the stripper must and must not remove."""

    def test_line_and_block_comments_go(self) -> None:
        source = "a // x\nb /* y */ c\n/* multi\nline */ d"
        self.assertEqual(GATE.tokens(source), ["a", "b", "c", "d"])

    def test_nested_block_comment_is_one_comment(self) -> None:
        self.assertEqual(GATE.strip_comments("a /* b /* c */ d */ e"), "a  e")

    def test_string_literals_survive(self) -> None:
        # The block's own payload: a `//`-free string, and one with `//`
        # and `/*` inside it, neither of which may open a comment.
        source = 'println!("cargo:rerun-if-changed={path}"); let s = "a // b /* c";'
        self.assertEqual(GATE.strip_comments(source), source)

    def test_escaped_quote_does_not_end_the_string(self) -> None:
        # One escaped quote, not a pair: with `\"` read as a terminator
        # a pair re-opens a string on the second one and the comment
        # that follows survives by accident (audit-tests, #1778).
        source = 'let s = "say \\" // still string"; // gone'
        self.assertEqual(GATE.strip_comments(source), 'let s = "say \\" // still string"; ')

    def test_char_literal_quote_does_not_open_a_string(self) -> None:
        # `'"'` read as an unpaired `"` would swallow the rest of the
        # file into a string and hide every later comment and token.
        source = "let q = '\"'; // gone\nlet r = '\\''; // gone"
        self.assertEqual(GATE.strip_comments(source), "let q = '\"'; \nlet r = '\\''; ")

    def test_raw_string_trailing_backslash_does_not_escape_its_quote(self) -> None:
        # Lexed as an ordinary string, `r"a\"` would read `\"` as an
        # escape, run on to the next `"`, and hide the comment between.
        source = 'let r = r"a\\"; // gone\nlet h = r#"x"y"#; // gone\nlet z = 1;'
        self.assertEqual(
            GATE.strip_comments(source), 'let r = r"a\\"; \nlet h = r#"x"y"#; \nlet z = 1;'
        )

    def test_escaped_newline_continues_the_string(self) -> None:
        # `"a\` + newline + `b"` is one Rust string. An escape pattern
        # that cannot cross a newline leaves it open at the line end,
        # pairs its closing quote with the next string's opening one,
        # and everything between reads as code or comment at random
        # (stage-E R1, #1778).
        source = 'let s = "a\\\n   b"; // gone\nlet t = "c"; // gone'
        self.assertEqual(GATE.strip_comments(source), 'let s = "a\\\n   b"; \nlet t = "c"; ')

    def test_lifetime_is_not_a_char_literal(self) -> None:
        # Two lifetimes with the comment between them: a greedy `'…'`
        # reading pairs `'a` with `'b` and the comment sits inside the
        # phantom literal. `fn f<'a>(x: &'a str)` cannot see that, since
        # its two `'a`s pair up before the comment (audit-tests, #1778).
        source = "let a: &'a str = x; // gone\nlet b: &'b str = y;"
        self.assertEqual(GATE.strip_comments(source), "let a: &'a str = x; \nlet b: &'b str = y;")


class VendoredCrates(unittest.TestCase):
    def test_filters_to_the_substring_in_manifest_order(self) -> None:
        self.assertEqual(GATE.vendored_crates(MANIFEST), CRATES)

    def test_published_name_is_in_scope(self) -> None:
        # The five crates publish as `bca-tree-sitter-*`; a sixth laid
        # out under that name, or a `dekobon-tree-sitter-*`, must be
        # checked rather than silently skipped (stage-D M1, #1778).
        manifest = (
            "[workspace]\n"
            'exclude = ["enums", "bca-tree-sitter-x", "fuzz", "dekobon-tree-sitter-y"]\n'
        )
        self.assertEqual(
            GATE.vendored_crates(manifest), ["bca-tree-sitter-x", "dekobon-tree-sitter-y"]
        )

    def test_missing_exclude_is_rejected(self) -> None:
        with self.assertRaises(GATE.ManifestError):
            GATE.vendored_crates('[workspace]\nmembers = ["x"]\n')

    def test_no_vendored_entry_is_rejected(self) -> None:
        # An empty list would pass the gate with nothing checked.
        with self.assertRaises(GATE.ManifestError):
            GATE.vendored_crates('[workspace]\nexclude = ["enums", "fuzz"]\n')

    def test_non_string_entry_is_rejected(self) -> None:
        with self.assertRaises(GATE.ManifestError):
            GATE.vendored_crates('[workspace]\nexclude = ["tree-sitter-a", 1]\n')


@contextlib.contextmanager
def _scratch_repo(  # type: ignore[no-untyped-def]
    scripts: dict[str, str | bytes | None], manifest: str | None = MANIFEST
):
    """A throwaway root with one crate per key.

    ``None`` for a crate means no build.rs; ``bytes`` are written raw.
    ``manifest=None`` writes no Cargo.toml at all.
    """
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        if manifest is not None:
            (root / "Cargo.toml").write_text(manifest, encoding="utf-8")
        for crate, source in scripts.items():
            script = root / crate / GATE.BUILD_SCRIPT
            script.parent.mkdir(parents=True)
            if isinstance(source, bytes):
                script.write_bytes(source)
            elif source is not None:
                script.write_text(source, encoding="utf-8")
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            yield lambda: (GATE.main(["--root", str(root)]), out.getvalue(), err.getvalue())


def _both(source: str | None) -> dict[str, str | None]:
    return dict.fromkeys(CRATES, source)


class Main(unittest.TestCase):
    def test_canonical_block_passes(self) -> None:
        with _scratch_repo(_both(GOOD)) as run:
            code, out, err = run()
        self.assertEqual(code, 0, err)
        self.assertIn("grammar-build-watch: OK (2 vendored crate(s)", out)
        self.assertEqual(err, "")

    def test_reflowed_block_passes(self) -> None:
        # Tokens, not text: a re-indented, re-commented copy is the same
        # block, and so is one rustfmt has wrapped at a bracket where the
        # canonical spelling has no whitespace at all (`println!(\n"…"`).
        reflowed = (
            "fn main() {\n"
            'let src_dir = std::path::Path::new("src");\n'
            '  let headers = src_dir.join("tree_sitter"); /* watch */\n'
            "  if let Ok(entries) =\n"
            "      std::fs::read_dir(&headers)\n"
            "  {\n"
            "      for entry in entries.flatten() { match entry.path().to_str() {\n"
            '          Some(path) => println!("cargo:rerun-if-changed={path}"),\n'
            "          None => println!(\n"
            '              "cargo:rerun-if-changed={}",\n'
            "              headers.display()\n"
            "          ),\n"
            "      } }\n"
            "  }\n" + TAIL
        )
        with _scratch_repo(_both(reflowed)) as run:
            code, _out, err = run()
        self.assertEqual(code, 0, err)

    def test_continued_string_before_the_block_passes(self) -> None:
        # End to end: a valid escaped-newline continuation ahead of the
        # watch must not make an intact block read as absent (R1).
        continued = PREAMBLE.replace(
            'let src_dir = std::path::Path::new("src");',
            'println!("cargo:warning=vendored \\\n    grammar");\n'
            '    let src_dir = std::path::Path::new("src");',
        )
        self.assertNotEqual(continued, PREAMBLE)
        with _scratch_repo(_both(continued + WATCH + TAIL)) as run:
            code, _out, err = run()
        self.assertEqual(code, 0, err)

    def test_block_deleted_but_comment_kept_fails_naming_the_crate(self) -> None:
        # The explanatory comment alone must not satisfy the gate — a
        # token check that keyed on `tree_sitter` would pass this.
        with _scratch_repo({CRATES[0]: GOOD, CRATES[1]: PREAMBLE + TAIL}) as run:
            code, _out, err = run()
        self.assertEqual(code, 1)
        self.assertIn("error: tree-sitter-beta: tree-sitter-beta/bindings/rust/build.rs", err)
        self.assertIn("block absent", err)
        self.assertNotIn("tree-sitter-alpha", err)

    def test_block_commented_out_fails(self) -> None:
        # The block's own tokens, present but dead. Each shape must read
        # as absent *because* its comment was stripped: with the
        # `/* */` arm disabled the wrapped copy passes the gate, and the
        # explanatory-comment row above cannot see that, since its
        # comment never carried the block's tokens (audit-tests, #1778).
        # The `/* */` wrapper is the shape the upstream template itself
        # uses for its disabled C++ scanner block.
        wrapped = PREAMBLE + "    /*\n" + WATCH + "    */\n" + TAIL
        lined = PREAMBLE + "".join(f"    //{line}\n" for line in WATCH.splitlines()) + TAIL
        for label, source in [("/* */", wrapped), ("//", lined)]:
            with self.subTest(label):
                with _scratch_repo(_both(source)) as run:
                    code, _out, err = run()
                self.assertEqual(code, 1)
                self.assertIn("error: tree-sitter-alpha:", err)
                self.assertIn("block absent", err)

    def test_read_dir_loop_deleted_fails(self) -> None:
        # `let headers = …` survives; nothing is watched.
        stub = PREAMBLE + '    let headers = src_dir.join("tree_sitter");\n' + TAIL
        with _scratch_repo({CRATES[0]: stub, CRATES[1]: GOOD}) as run:
            code, _out, err = run()
        self.assertEqual(code, 1)
        self.assertIn("error: tree-sitter-alpha:", err)
        self.assertIn("does not carry the src/tree_sitter/ header watch", err)

    def test_emission_replaced_fails_with_a_diff(self) -> None:
        # The loop runs but watches the wrong thing. The closest region
        # is diffed so the reader sees which token moved.
        wrong = GOOD.replace(
            'Some(path) => println!("cargo:rerun-if-changed={path}"),',
            'Some(_) => println!("cargo:rerun-if-changed=build.rs"),',
        )
        self.assertNotEqual(wrong, GOOD)
        with _scratch_repo(_both(wrong)) as run:
            code, _out, err = run()
        self.assertEqual(code, 1)
        self.assertIn("--- expected", err)
        self.assertIn('\n  -"cargo:rerun-if-changed={path}"\n', err)
        self.assertIn('\n  +"cargo:rerun-if-changed=build.rs"\n', err)
        # Whole tokens on both sides: a region sliced on character
        # offsets opened the diff mid-literal (`+rc");`).
        for line in err.splitlines():
            if line.startswith(("  +", "  -")) and not line.startswith(("  +++", "  ---")):
                self.assertEqual(GATE.tokens(line[3:]), [line[3:]], line)

    def test_only_parser_and_scanner_watches_fails(self) -> None:
        # The upstream template shape: `rerun-if-changed` is present for
        # the two translation units and nothing else.
        with _scratch_repo(_both("fn main() {\n" + TAIL)) as run:
            code, _out, err = run()
        self.assertEqual(code, 1)
        self.assertIn("error: tree-sitter-alpha:", err)
        self.assertIn("error: tree-sitter-beta:", err)

    def test_missing_build_rs_fails(self) -> None:
        with _scratch_repo({CRATES[0]: GOOD, CRATES[1]: None}) as run:
            code, _out, err = run()
        self.assertEqual(code, 1)
        self.assertIn("error: tree-sitter-beta: could not read", err)

    def test_excluded_crate_without_a_directory_fails(self) -> None:
        # Discovery is not hardcoded: a sixth entry is checked and, with
        # no directory behind it, fails by name.
        manifest = MANIFEST.replace(
            '  "tree-sitter-beta",\n', '  "tree-sitter-beta",\n  "tree-sitter-extra",\n'
        )
        self.assertNotEqual(manifest, MANIFEST)
        with _scratch_repo(_both(GOOD), manifest) as run:
            code, _out, err = run()
        self.assertEqual(code, 1)
        self.assertIn(
            "error: tree-sitter-extra: listed in [workspace].exclude but is not a directory",
            err,
        )

    def test_non_utf8_build_rs_fails_without_a_traceback(self) -> None:
        # `UnicodeDecodeError` is not an `OSError`; uncaught it is a
        # traceback and an exit of 1 by accident rather than a verdict.
        with _scratch_repo({CRATES[0]: GOOD, CRATES[1]: b"fn main() { \xff }\n"}) as run:
            code, _out, err = run()
        self.assertEqual(code, 1)
        self.assertIn("error: tree-sitter-beta: could not read", err)
        self.assertNotIn("Traceback", err)

    def test_malformed_exclude_exits_two(self) -> None:
        with _scratch_repo(_both(GOOD), manifest="[workspace]\nexclude = 7\n") as run:
            code, _out, err = run()
        self.assertEqual(code, 2)
        self.assertIn("could not derive the vendored crate list", err)
        self.assertIn("not a string array", err)

    def test_unparseable_manifest_exits_two(self) -> None:
        with _scratch_repo(_both(GOOD), manifest="[workspace\nexclude = [\n") as run:
            code, _out, err = run()
        self.assertEqual(code, 2)
        self.assertIn("could not derive the vendored crate list", err)

    def test_missing_manifest_exits_two(self) -> None:
        with _scratch_repo(_both(GOOD), manifest=None) as run:
            code, _out, err = run()
        self.assertEqual(code, 2)
        self.assertIn("could not derive the vendored crate list", err)
        self.assertIn("No such file", err)


class RealRepository(unittest.TestCase):
    def test_gate_passes_on_the_checked_in_tree(self) -> None:
        result = subprocess.run(
            [sys.executable, "-B", str(SCRIPT_SRC)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("grammar-build-watch: OK (5 vendored crate(s)", result.stdout)

    def test_canonical_block_is_what_the_tcl_script_carries(self) -> None:
        # The constant's provenance claim: it is lifted from tcl's
        # build.rs. Compared as tokens, exactly as the gate compares, so
        # a comment-only edit or a reindent of tcl's block changes
        # neither verdict; a change to the watch itself changes both.
        tcl = REPO_ROOT / "tree-sitter-tcl" / GATE.BUILD_SCRIPT
        self.assertTrue(
            GATE.contains_run(GATE.tokens(tcl.read_text(encoding="utf-8")), GATE.canonical())
        )


if __name__ == "__main__":
    unittest.main()
