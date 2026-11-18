#!/usr/bin/env python3
"""Self-tests for `utils/drift-oracle.py` on a stub `bca` and a stub
`verbatim-oracle`: the manifest records which views the capture
carries and the grammar pins of the checkout, the verbatim view is
optional and its argv contract pinned, a failed execution makes the
capture incomplete — and so does an interrupted recapture, which may
not leave the previous complete manifest behind — outputs are bytes,
and a symlink is skipped and logged rather than followed.

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
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

UTILS_DIR = pathlib.Path(__file__).resolve().parent


def _load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, UTILS_DIR / filename)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


oracle = _load("drift_oracle", "drift-oracle.py")
dc = oracle.dc

EXPECTED_MI_OPTIONS = {
    "metrics_mi_charges_docs_false": ["--mi-charges-docs=false"],
    "metrics_mi_charges_docs_true": ["--mi-charges-docs=true"],
    "metrics_exclude_tests_mi_charges_docs_false": ["--exclude-tests", "--mi-charges-docs=false"],
    "metrics_exclude_tests_mi_charges_docs_true": ["--exclude-tests", "--mi-charges-docs=true"],
    "metrics_only_mi_mi_charges_docs_false": ["--metrics", "mi", "--mi-charges-docs=false"],
    "metrics_only_mi_mi_charges_docs_true": ["--metrics", "mi", "--mi-charges-docs=true"],
    "metrics_only_mi_exclude_tests_mi_charges_docs_false": ["--metrics", "mi", "--exclude-tests", "--mi-charges-docs=false"],
    "metrics_only_mi_exclude_tests_mi_charges_docs_true": ["--metrics", "mi", "--exclude-tests", "--mi-charges-docs=true"],
    "metrics_only_mi_exclude_tests_no_count_try_mi_charges_docs_false": ["--metrics", "mi", "--exclude-tests", "--cyclomatic-count-try=false", "--mi-charges-docs=false"],
    "metrics_only_mi_exclude_tests_no_count_try_mi_charges_docs_true": ["--metrics", "mi", "--exclude-tests", "--cyclomatic-count-try=false", "--mi-charges-docs=true"],
}

EXPECTED_METRICS_OPTIONS = {
    "metrics": [],
    "metrics_exclude_tests": ["--exclude-tests"],
    "metrics_no_count_try": ["--cyclomatic-count-try=false"],
    "metrics_exclude_tests_no_count_try": ["--exclude-tests", "--cyclomatic-count-try=false"],
    "metrics_only_cyclomatic": ["--metrics", "cyclomatic"],
    "metrics_only_cognitive": ["--metrics", "cognitive"],
    "metrics_only_nexits": ["--metrics", "nexits"],
    **EXPECTED_MI_OPTIONS,
}


def _stub(path: pathlib.Path, body: str) -> pathlib.Path:
    path.write_text(f"#!{sys.executable}\nimport sys\n{body}\n")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return path


class DriftOracleTests(unittest.TestCase):
    def setUp(self) -> None:
        env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
        env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull)
        self._git_env = patch.dict(os.environ, env, clear=True)
        self._git_env.start()
        self.addCleanup(self._git_env.stop)
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        self.bca = _stub(self.root / "bca", "sys.stdout.write(' '.join(sys.argv[1:]) + '\\n')")
        self.verbatim = _stub(self.root / "verbatim-oracle", "import json; sys.stdout.write(json.dumps({'v': sys.argv[1:]}) + '\\n')")
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
        self.assertEqual(m["views"], [*oracle.CLI_CMDS, *EXPECTED_METRICS_OPTIONS, "verbatim", *oracle.VERBATIM_OPTIONS])
        self.assertEqual(m["outputs_per_file"], 34)
        self.assertEqual(sorted(m["keys"]), ["python/fx/b.py", "rust/fx/a.rs"])
        self.assertEqual(m["verbatim"]["path"], str(self.verbatim))
        self.assertEqual(m["version"], dc.MANIFEST_VERSION)
        # The pins default to this checkout's Cargo.toml, never `{}`.
        self.assertEqual(m["grammar_pins"], dc.grammar_pins())
        self.assertIn("tree-sitter-rust", m["grammar_pins"])
        key = self.out / "rust" / "fx" / "a.rs"
        # The driver <-> bin contract: `verbatim-oracle <slug> <path>`.
        self.assertEqual(
            json.loads(key.with_name("a.rs.verbatim.jsonl").read_text()),
            {"v": ["rust", str(self.fixtures / "a.rs")]},
        )
        self.assertTrue(key.with_name("a.rs.ops.txt").is_file())
        self.assertIn("--language rust", key.with_name("a.rs.metrics_exclude_tests.txt").read_text())

    def test_mi_matrix_and_every_argv_are_symmetric_and_explicit(self) -> None:
        _stub(self.bca, "import json; print(json.dumps(sys.argv[1:]))")
        rc, out, _, m = self._run("--verbatim", str(self.verbatim), "--lang", "rust")
        self.assertEqual(rc, 0)
        expected_verbatim = {
            f"verbatim_{name}": ["--metrics-view", *extra, "--"]
            for name, extra in EXPECTED_MI_OPTIONS.items()
        }
        self.assertEqual(oracle.METRICS_OPTIONS, EXPECTED_METRICS_OPTIONS)
        self.assertEqual(oracle.VERBATIM_OPTIONS, expected_verbatim)
        self.assertEqual(m["metrics_options"], {**EXPECTED_METRICS_OPTIONS, **expected_verbatim})
        self.assertEqual(m["views"], [*oracle.CLI_CMDS, *EXPECTED_METRICS_OPTIONS, "verbatim", *expected_verbatim])
        self.assertEqual(json.loads(out)["outputs"], 34)
        path = str(self.fixtures / "a.rs")
        key = self.out / "rust" / "fx" / "a.rs"
        for name, extra in EXPECTED_METRICS_OPTIONS.items():
            with self.subTest(view=name):
                self.assertEqual(
                    json.loads(key.with_name(f"a.rs.{name}.txt").read_text()),
                    ["metrics", "-O", "json", *extra, "--no-config", "--language", "rust", "-p", path],
                )
        for name, extra in expected_verbatim.items():
            with self.subTest(view=name):
                self.assertEqual(
                    json.loads(key.with_name(f"a.rs.{name}.txt").read_text()),
                    {"v": [*extra, "rust", path]},
                )

    def test_a_failed_mi_verbatim_view_is_not_a_baseline(self) -> None:
        _stub(self.verbatim, "sys.exit(4 if '--mi-charges-docs=true' in sys.argv else 0)")
        rc, _, err, m = self._run("--verbatim", str(self.verbatim), "--lang", "rust")
        self.assertEqual(rc, 1)
        self.assertFalse(m["complete"])
        self.assertEqual(m["failed_executions"], 5)
        self.assertIn("not a baseline", err)

    def test_outputs_are_captured_as_bytes(self) -> None:
        # A text capture would fold b"\xff\r\n" and b"\xfe\n" to the same
        # "\ufffd\n", so a real change in the binary's output would
        # compare equal (D2 on #1744).
        _stub(self.bca, "sys.stdout.buffer.write(b'\\xff\\r\\n' if 'ops' in sys.argv else b'\\xfe\\n')")
        rc, _, _, _ = self._run("--lang", "rust")
        self.assertEqual(rc, 0)
        key = self.out / "rust" / "fx" / "a.rs"
        self.assertEqual(key.with_name("a.rs.ops.txt").read_bytes(), b"\xff\r\n")
        self.assertEqual(key.with_name("a.rs.dump.txt").read_bytes(), b"\xfe\n")

    def test_an_interrupted_recapture_does_not_leave_a_complete_manifest(self) -> None:
        rc, _, _, m = self._run("--lang", "rust")
        self.assertEqual((rc, m["complete"]), (0, True))
        # The second run into the same directory dies mid-capture.
        real_capture = dc.capture

        def dying(argv: list[str]):
            if "dump" in argv:
                raise KeyboardInterrupt
            return real_capture(argv)

        dc.capture = dying
        try:
            with self.assertRaises(KeyboardInterrupt):
                self._run("--lang", "rust")
        finally:
            dc.capture = real_capture
        m = json.loads((self.out / "manifest.json").read_text())
        self.assertFalse(m["complete"], "the stale complete manifest was rewritten before the first output")
        self.assertEqual(m["version"], dc.MANIFEST_VERSION)

    def test_capture_without_verbatim_records_only_cli_views(self) -> None:
        rc, _, _, m = self._run("--lang", "rust")
        self.assertEqual(rc, 0)
        self.assertNotIn("verbatim", m["views"])
        self.assertIsNone(m["verbatim"])
        self.assertEqual(m["metrics_options"], EXPECTED_METRICS_OPTIONS)
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
        rc, _, _, m = self._run("--repo-toml", str(self.root / "no.toml"), "--lang", "rust")
        self.assertEqual(rc, f"missing --repo-toml {self.root / 'no.toml'}")
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

    def test_explicit_selection_is_tracked_exact_and_can_force_a_dialect(self) -> None:
        subprocess.run(["git", "init", "-q", str(self.fixtures)], check=True)
        subprocess.run(["git", "-C", str(self.fixtures), "add", "a.rs"], check=True)
        selection = self.root / "selection.json"
        selection.write_text(json.dumps(["mozcpp/fx/a.rs"]))
        view = "verbatim_metrics_only_mi_mi_charges_docs_true"
        rc, out, _, m = self._run("--selection", str(selection), "--verbatim", str(self.verbatim), "--view", view)
        self.assertEqual(rc, 0)
        self.assertEqual(m["keys"], ["mozcpp/fx/a.rs"])
        self.assertEqual(m["populations"], {"mozcpp": 1})
        self.assertEqual(m["views"], [view])
        self.assertEqual(m["cli_cmds"], {})
        self.assertEqual(m["outputs_per_file"], 1)
        self.assertEqual(json.loads(out)["outputs"], 1)
        self.assertEqual(m["metrics_options"], {view: ["--metrics-view", "--metrics", "mi", "--mi-charges-docs=true", "--"]})
        self.assertEqual(json.loads((self.out / f"mozcpp/fx/a.rs.{view}.txt").read_text()),
                         {"v": ["--metrics-view", "--metrics", "mi", "--mi-charges-docs=true", "--", "mozcpp", str(self.fixtures / "a.rs")]})

    def test_selection_rejects_empty_duplicate_missing_untracked_and_escape_keys(self) -> None:
        subprocess.run(["git", "init", "-q", str(self.fixtures)], check=True)
        subprocess.run(["git", "-C", str(self.fixtures), "add", "a.rs"], check=True)
        selection = self.root / "selection.json"
        for keys in [[], {}, [1], ["rust/fx/a.rs"] * 2, ["rust/fx/missing.rs"],
                     ["python/fx/b.py"], ["rust/fx/../a.rs"], ["rust/fx//a.rs"],
                     ["unknown/fx/a.rs"], ["rust/other/a.rs"]]:
            with self.subTest(keys=keys):
                selection.write_text(json.dumps(keys))
                rc, _, _, m = self._run("--selection", str(selection))
                self.assertNotEqual(rc, 0)
                self.assertEqual(m, {})
        os.symlink(self.fixtures / "a.rs", self.fixtures / "link.rs")
        selection.write_text('["rust/fx/link.rs"]')
        rc, _, _, _ = self._run("--selection", str(selection))
        self.assertIn("symlink", str(rc))

    def test_view_validation_and_requested_empty_population_fail(self) -> None:
        for options in [("--view", "missing"), ("--view", "verbatim"),
                        ("--view", "metrics", "--view", "metrics")]:
            rc, _, _, m = self._run("--lang", "rust", *options)
            self.assertEqual(rc, "duplicate or unavailable --view")
            self.assertEqual(m, {})
        subprocess.run(["git", "init", "-q", str(self.fixtures)], check=True)
        subprocess.run(["git", "-C", str(self.fixtures), "add", "a.rs"], check=True)
        selection = self.root / "selection.json"
        selection.write_text('["rust/fx/a.rs"]')
        rc, _, _, m = self._run("--selection", str(selection), "--lang", "python")
        self.assertEqual(rc, "population python is empty")
        self.assertEqual(m, {})

    def test_relative_paths_reach_binary_without_output_rewriting(self) -> None:
        old = pathlib.Path.cwd()
        try:
            os.chdir(self.root)
            rc, _, _, m = self._run("--lang", "rust", "--view", "metrics", "--relative-paths")
        finally:
            os.chdir(old)
        self.assertEqual(rc, 0)
        self.assertEqual(m["keys"], ["rust/fx/a.rs"])
        self.assertEqual((self.out / "rust/fx/a.rs.metrics.txt").read_bytes(),
                         b"metrics -O json --no-config --language rust -p fx/a.rs\n")

    def test_selected_filename_is_a_literal_git_pathspec(self) -> None:
        subprocess.run(["git", "init", "-q", str(self.fixtures)], check=True)
        subprocess.run(["git", "-C", str(self.fixtures), "add", "a.rs"], check=True)
        (self.fixtures / "*.rs").write_text("fn untracked() {}")
        selection = self.root / "selection.json"
        selection.write_text('["rust/fx/*.rs"]')
        rc, _, _, m = self._run("--selection", str(selection))
        self.assertEqual(rc, "selected input is not tracked: 'rust/fx/*.rs'")
        self.assertEqual(m, {})

    def test_selection_rejects_directory_prefix_impostor(self) -> None:
        directory = self.fixtures / "dir"
        directory.mkdir()
        child = directory / "child.rs"
        child.write_text("fn child() {}")
        subprocess.run(["git", "init", "-q", str(self.fixtures)], check=True)
        subprocess.run(["git", "-C", str(self.fixtures), "add", "dir/child.rs"], check=True)
        child.unlink()
        directory.rmdir()
        directory.write_text("fn impostor() {}")
        selection = self.root / "selection.json"
        selection.write_text('["rust/fx/dir"]')
        rc, _, _, m = self._run("--selection", str(selection), "--view", "metrics")
        self.assertEqual(rc, "selected input is not tracked: 'rust/fx/dir'")
        self.assertEqual(m, {})

    def test_selection_accepts_exact_odd_paths_in_nested_root(self) -> None:
        _stub(self.bca, "import json; print(json.dumps(sys.argv[1:]))")
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        names = ["*.rs", ":(glob)*.rs", "quote\"\tline\n.rs", "café.rs"]
        if os.name == "posix":
            names.append(os.fsdecode(b"nonutf8-\xff.rs"))
        for name in names:
            (self.fixtures / name).write_text("fn exact() {}")
        subprocess.run(["git", "-C", str(self.root), "add", "fx"], check=True)
        selection = self.root / "selection.json"
        keys = [f"rust/fx/{name}" for name in names]
        selection.write_text(json.dumps(keys))
        rc, _, err, m = self._run("--selection", str(selection), "--view", "metrics")
        self.assertEqual(rc, 0, err)
        self.assertEqual(m["keys"], sorted(keys))
        for name in names:
            args = json.loads((self.out / f"rust/fx/{name}.metrics.txt").read_text())
            self.assertEqual(os.fsencode(args[-1]), os.fsencode(self.fixtures / name))

    def test_leading_hyphen_relative_path_is_disambiguated_for_every_binary(self) -> None:
        _stub(self.bca, "import json; print(json.dumps(sys.argv[1:]))")
        (self.fixtures / "--help").write_text("fn f() {}")
        subprocess.run(["git", "init", "-q", str(self.fixtures)], check=True)
        subprocess.run(["git", "-C", str(self.fixtures), "add", "--", "--help"], check=True)
        selection = self.root / "selection.json"
        selection.write_text('["rust/fx/--help"]')
        with contextlib.chdir(self.fixtures):
            rc, _, _, m = self._run("--selection", str(selection), "--relative-paths",
                                     "--verbatim", str(self.verbatim))
        self.assertEqual(rc, 0)
        self.assertTrue(m["complete"])
        for view in m["views"]:
            suffix = ".verbatim.jsonl" if view == "verbatim" else f".{view}.txt"
            args = json.loads((self.out / f"rust/fx/--help{suffix}").read_text())
            if view.startswith("verbatim"):
                args = args["v"]
                expected = [*oracle.VERBATIM_OPTIONS.get(view, []), "rust", "./--help"]
            else:
                head = oracle.CLI_CMDS.get(view, ["metrics", "-O", "json", *EXPECTED_METRICS_OPTIONS.get(view, [])])
                expected = [*head, "--no-config", "--language", "rust", "-p", "./--help"]
            self.assertEqual(args, expected, view)

    def test_explicit_selection_keeps_root_identity_and_selected_helpers(self) -> None:
        subprocess.run(["git", "init", "-q", str(self.fixtures)], check=True)
        subprocess.run(["git", "-C", str(self.fixtures), "add", "a.rs"], check=True)
        selection = self.root / "selection.json"
        selection.write_text('["rust/alias/a.rs", "ccomment/corpus/a.rs"]')
        rc, _, _, m = self._run("--selection", str(selection), "--root", f"alias={self.fixtures}",
                                "--corpus", str(self.fixtures), "--view", "metrics")
        self.assertEqual(rc, 0)
        self.assertEqual(m["keys"], ["ccomment/corpus/a.rs", "rust/alias/a.rs"])
        self.assertEqual(m["populations"], {"rust": 1, "ccomment": 1})


if __name__ == "__main__":
    unittest.main()
