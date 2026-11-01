#!/usr/bin/env python3
"""Self-tests for `utils/drift_common.py`: the extension tables are
derived from the enum and every extension-less variant is accounted
for, the grammar pins come from `tomllib`, a capture is bytes, the
walk recurses and logs symlinks, and the manifest carries its version.

Run: ``python3 -B -m unittest -q utils/drift_common-test.py``.
"""

from __future__ import annotations

import importlib.util
import os
import pathlib
import stat
import sys
import tempfile
import unittest

UTILS_DIR = pathlib.Path(__file__).resolve().parent


def _load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, UTILS_DIR / filename)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


dc = _load("drift_common", "drift_common.py")
rt = dc.role_tables


def _lang(variant: str, exts: tuple[str, ...] = ()) -> object:
    return rt.Language(variant, variant.lower(), f"tree_sitter_{variant.lower()}", exts)


class PopulationTests(unittest.TestCase):
    def test_every_enum_variant_is_a_population_with_dotted_extensions(self) -> None:
        pops = dc.populations()
        langs = rt.languages()
        self.assertEqual(list(pops), [lang.stem for lang in langs])
        for lang in langs:
            self.assertTrue(pops[lang.stem], lang.stem)
            self.assertTrue(all(e.startswith(".") for e in pops[lang.stem]), pops[lang.stem])
            own = tuple(f".{e}" for e in lang.extensions)
            self.assertEqual(pops[lang.stem][: len(own)], own, "the row's own extensions come first")
        # The forks and helpers borrow or declare, never drop out: Mozcpp
        # owns nothing and reads Cpp's; Mozjs owns `.jsm` and reads JS's
        # as well.
        self.assertEqual(pops["mozcpp"], pops["cpp"])
        self.assertEqual(pops["mozjs"], (".jsm", *pops["javascript"]))
        self.assertEqual(pops["ccomment"], (".c", ".cpp", ".h"))
        # The gaps the hand-copied tables had (D5 on #1744).
        for ext in (".kts", ".gradle", ".t", ".rake", ".tk", ".phtml", ".cjs", ".hxx", ".mm", ".jsm"):
            self.assertIn(ext, dc.langs_by_extension(pops), ext)

    def _enum(self, *extra: object) -> list:
        # The real forks and helpers, so the override tables resolve.
        return [
            _lang("Cpp", ("cpp",)), _lang("Mozcpp"), _lang("Javascript", ("js",)), _lang("Mozjs", ("jsm",)),
            _lang("Ccomment"), _lang("Preproc"), *extra,
        ]

    def test_an_extensionless_variant_without_an_override_is_an_error(self) -> None:
        with self.assertRaises(rt.RoleTableError) as cm:
            dc.populations(self._enum(_lang("Newlang")))
        self.assertIn("LANG::Newlang", str(cm.exception))
        self.assertEqual(dc.populations(self._enum())["mozjs"], (".jsm", ".js"))

    def test_a_stale_override_is_an_error(self) -> None:
        # Every BORROWS / DECLARED key must still be a variant; an enum
        # lacking one trips the check rather than carrying a dead row.
        with self.assertRaises(rt.RoleTableError) as cm:
            dc.populations(self._enum()[:-1])
        self.assertIn("names no LANG variant: ['preproc']", str(cm.exception))

    def test_langs_by_extension_inverts_in_population_order(self) -> None:
        by_ext = dc.langs_by_extension({"cpp": (".cpp", ".h"), "mozcpp": (".cpp", ".h"), "c": (".c",)})
        self.assertEqual(by_ext, {".cpp": ["cpp", "mozcpp"], ".h": ["cpp", "mozcpp"], ".c": ["c"]})


class GrammarPinTests(unittest.TestCase):
    def test_pins_are_read_with_tomllib_from_both_spellings(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            toml = pathlib.Path(d) / "Cargo.toml"
            toml.write_text(
                "[workspace.dependencies]\n"
                'serde = "1"\n'
                'tree-sitter-rust = "=0.24.2"\n'
                "# tree-sitter-fake = \"=9.9.9\"\n"
                'tree-sitter-mozcpp = { package = "bca-tree-sitter-mozcpp", path = "./x", version = "=2.4.0" }\n'
                "[dependencies]\n"
                'tree-sitter-go = "=0.25.0"\n'
            )
            self.assertEqual(dc.grammar_pins(toml), {"tree-sitter-rust": "=0.24.2", "tree-sitter-mozcpp": "=2.4.0"})
            toml.write_text('[workspace.dependencies]\nserde = "1"\n')
            with self.assertRaises(ValueError):
                dc.grammar_pins(toml)

    def test_the_checkout_pins_agree_with_role_tables(self) -> None:
        pins = dc.grammar_pins()
        self.assertEqual({k: v.lstrip("=") for k, v in pins.items()}, rt.grammar_pins())


class CaptureTests(unittest.TestCase):
    def test_distinct_invalid_bytes_and_line_endings_stay_distinct(self) -> None:
        # Text capture with errors="replace" folds b"\xff\r\n" and b"\xfe\n"
        # to the same "\ufffd\n"; a byte capture keeps them apart.
        with tempfile.TemporaryDirectory() as d:
            stubs = []
            for i, payload in enumerate((b"\xff\r\n", b"\xfe\n")):
                stub = pathlib.Path(d) / f"stub{i}"
                stub.write_text(f"#!{sys.executable}\nimport sys\nsys.stdout.buffer.write({payload!r})\n")
                stub.chmod(stub.stat().st_mode | stat.S_IXUSR)
                stubs.append(dc.capture([str(stub)]))
            self.assertEqual(stubs[0].stdout, b"\xff\r\n")
            self.assertNotEqual(stubs[0].rendered(), stubs[1].rendered())
            self.assertIsNone(stubs[0].how)
            self.assertEqual(stubs[0].rendered(), b"\xff\r\n")

    def test_a_failed_run_is_rendered_behind_its_status(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            stub = pathlib.Path(d) / "stub"
            stub.write_text(f"#!{sys.executable}\nimport sys\nsys.stdout.write('out')\nsys.stderr.write('err')\nsys.exit(3)\n")
            stub.chmod(stub.stat().st_mode | stat.S_IXUSR)
            c = dc.capture([str(stub)])
            self.assertEqual(c.how, "exit 3")
            self.assertEqual(c.rendered(), b"EXIT 3\nout\nSTDERR\nerr")
            self.assertEqual(c.preview(), "err")
            self.assertEqual(dc.Capture(("x",), -9, b"", b"").how, "signal 9")


class WalkTests(unittest.TestCase):
    def test_walk_recurses_filters_by_suffix_and_logs_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            root = pathlib.Path(d)
            (root / "control_flow" / "rust").mkdir(parents=True)
            (root / "control_flow" / "rust" / "r07.rs").write_text("")
            (root / "control_flow" / "notes.txt").write_text("")
            (root / ".git").mkdir()
            (root / ".git" / "x.rs").write_text("")
            os.symlink(root / "control_flow" / "rust" / "r07.rs", root / "link.rs")
            links: list[str] = []
            found = dc.walk(root, (".rs",), (".git",), links)
            self.assertEqual(found, [root / "control_flow" / "rust" / "r07.rs"])
            self.assertEqual(links, [str(root / "link.rs")])


class ManifestTests(unittest.TestCase):
    def test_build_manifest_carries_the_version_and_derives_outputs_per_file(self) -> None:
        m = dc.build_manifest(
            complete=True,
            failed_executions=0,
            bca={"path": "b", "sha256": "0" * 64},
            verbatim=None,
            views=["ops", "metrics"],
            roots={"fx": "/fx"},
            metrics_options={"metrics": []},
            cli_cmds={"ops": ["ops"]},
            populations={"rust": 1},
            keys=["rust/fx/b.rs", "rust/fx/a.rs"],
            inputs_sha256={},
            grammar_pins={"tree-sitter-rust": "=0.24.2"},
            skipped_symlinks=[],
        )
        self.assertEqual(m["version"], dc.MANIFEST_VERSION)
        self.assertEqual(m["outputs_per_file"], 2)
        self.assertEqual(m["keys"], ["rust/fx/a.rs", "rust/fx/b.rs"])


if __name__ == "__main__":
    unittest.main()
