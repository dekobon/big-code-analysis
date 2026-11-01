#!/usr/bin/env python3
"""Self-tests for `utils/drift-oracle.py` on a stub `bca` and a stub
`verbatim-oracle`: the manifest records which views the capture
carries, the verbatim view is optional, a failed execution makes the
capture incomplete, and a symlink is skipped and logged rather than
followed.

Run: ``python3 -B -m unittest -q utils/drift-oracle-test.py``.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
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


oracle = _load("drift_oracle", "drift-oracle.py")


def _stub(path: pathlib.Path, body: str) -> pathlib.Path:
    path.write_text(f"#!{sys.executable}\nimport sys\n{body}\n")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


class DriftOracleTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        self.bca = _stub(self.root / "bca", "sys.stdout.write(' '.join(sys.argv[1:]) + '\\n')")
        self.verbatim = _stub(self.root / "verbatim-oracle", "sys.stdout.write('{\"v\": ' + repr(sys.argv[1:]) + '}\\n')")
        self.fixtures = self.root / "fx"
        self.fixtures.mkdir()
        (self.fixtures / "a.rs").write_text("fn a() {}\n")
        (self.fixtures / "b.py").write_text("def b(): pass\n")
        self.out = self.root / "out"

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _run(self, *extra: str) -> tuple[int, str, str, dict]:
        argv = ["--bca", str(self.bca), "--out", str(self.out), "--root", f"fx={self.fixtures}", "--jobs", "2", *extra]
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            try:
                rc = oracle.main(argv)
            except SystemExit as e:
                rc = e.code
        manifest = json.loads((self.out / "manifest.json").read_text()) if (self.out / "manifest.json").is_file() else {}
        return rc, out.getvalue(), err.getvalue(), manifest

    def test_capture_with_verbatim_records_every_view(self) -> None:
        rc, _, _, m = self._run("--verbatim", str(self.verbatim), "--lang", "rust", "--lang", "python")
        self.assertEqual(rc, 0)
        self.assertTrue(m["complete"])
        self.assertEqual(m["views"], [*oracle.CLI_CMDS, *oracle.METRICS_OPTIONS, "verbatim"])
        self.assertEqual(m["outputs_per_file"], len(m["views"]))
        self.assertEqual(sorted(m["keys"]), ["python/fx/b.py", "rust/fx/a.rs"])
        self.assertEqual(m["verbatim"]["path"], str(self.verbatim))
        key = self.out / "rust" / "fx" / "a.rs"
        self.assertTrue(key.with_name("a.rs.verbatim.jsonl").is_file())
        self.assertTrue(key.with_name("a.rs.ops.txt").is_file())
        self.assertIn("--language rust", key.with_name("a.rs.metrics_exclude_tests.txt").read_text())

    def test_capture_without_verbatim_records_only_cli_views(self) -> None:
        rc, _, _, m = self._run("--lang", "rust")
        self.assertEqual(rc, 0)
        self.assertNotIn("verbatim", m["views"])
        self.assertIsNone(m["verbatim"])
        self.assertEqual(m["outputs_per_file"], len(oracle.CLI_CMDS) + len(oracle.METRICS_OPTIONS))
        self.assertFalse((self.out / "rust" / "fx" / "a.rs.verbatim.jsonl").exists())

    def test_a_failed_execution_makes_the_capture_incomplete(self) -> None:
        _stub(self.bca, "sys.exit(3)")
        rc, _, err, m = self._run("--lang", "rust")
        self.assertEqual(rc, 1)
        self.assertFalse(m["complete"])
        self.assertEqual(m["failed_executions"], len(oracle.CLI_CMDS) + len(oracle.METRICS_OPTIONS))
        self.assertIn("not a baseline", err)
        self.assertTrue((self.out / "rust" / "fx" / "a.rs.ops.txt").read_text().startswith("EXIT 3\n"))

    def test_a_missing_binary_or_root_fails_before_any_capture(self) -> None:
        rc, _, _, m = self._run("--verbatim", str(self.root / "none"), "--lang", "rust")
        self.assertEqual(rc, f"missing binary {self.root / 'none'}")
        self.assertEqual(m, {})
        rc, _, _, _ = self._run("--root", f"other={self.root / 'nope'}", "--lang", "rust")
        self.assertTrue(str(rc).startswith("missing fixture root other="), rc)

    def test_an_empty_population_fails_the_run(self) -> None:
        rc, _, _, m = self._run("--lang", "go")
        self.assertEqual(rc, "population go is empty")
        self.assertEqual(m, {})

    def test_symlinks_are_skipped_and_logged(self) -> None:
        os.symlink(self.fixtures / "a.rs", self.fixtures / "link.rs")
        rc, _, err, m = self._run("--lang", "rust")
        self.assertEqual(rc, 0)
        self.assertEqual(m["keys"], ["rust/fx/a.rs"])
        self.assertEqual(m["skipped_symlinks"], [str(self.fixtures / "link.rs")])
        self.assertIn("skipped 1 symlink", err)


if __name__ == "__main__":
    unittest.main()
