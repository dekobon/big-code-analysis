#!/usr/bin/env python3
"""Self-tests for `utils/drift-compare.py` on synthetic captures: the
one-sided-view refusal and its `--only-shared-views` opt-in, the key
and byte comparisons, and the refusals an incomplete or mismatched
capture earns.

Run: ``python3 -B -m unittest -q utils/drift-compare-test.py``.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import pathlib
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


compare = _load("drift_compare", "drift-compare.py")

CLI = {"ops": ["ops"], "dump": ["dump"]}
METRICS = {"metrics": []}
KEYS = ["rust/fx/a.rs", "python/fx/b.py"]


def _capture(
    root: pathlib.Path,
    *,
    verbatim: bool,
    keys: list[str] = KEYS,
    content: str = "same",
    complete: bool = True,
    sha: str = "a" * 64,
    views_field: bool = True,
) -> pathlib.Path:
    root.mkdir(parents=True)
    views = [*CLI, *METRICS, *(["verbatim"] if verbatim else [])]
    manifest = {
        "complete": complete,
        "failed_executions": 0 if complete else 1,
        "bca": {"path": "bca", "sha256": sha},
        "verbatim": {"path": "v", "sha256": sha} if verbatim else None,
        "metrics_options": METRICS,
        "cli_cmds": CLI,
        "keys": keys,
        "inputs_sha256": {k: "0" * 64 for k in keys},
        "grammar_pins": {"tree-sitter-rust": "=0.24.0"},
    }
    if views_field:
        manifest["views"] = views
    (root / "manifest.json").write_text(json.dumps(manifest))
    for key in keys:
        dst = root / key
        dst.parent.mkdir(parents=True, exist_ok=True)
        for view in views:
            dst.with_name(dst.name + compare.suffix(view)).write_text(f"{view}:{content}\n")
    return root


class DriftCompareTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _run(self, base: pathlib.Path, head: pathlib.Path, *extra: str) -> tuple[int, str, dict]:
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            rc = compare.main([str(base), str(head), *extra])
        text = out.getvalue()
        # The JSON summary is the last line, and only of a run that
        # compared something.
        last = text.strip().splitlines()[-1]
        summary = json.loads(last) if last.startswith("{") else {}
        return rc, text, summary

    def test_identical_captures_compare_equal_over_every_view(self) -> None:
        base = _capture(self.root / "base", verbatim=True)
        head = _capture(self.root / "head", verbatim=True)
        rc, _, s = self._run(base, head)
        self.assertEqual(rc, 0)
        self.assertEqual(s["views_compared"], ["ops", "dump", "metrics", "verbatim"])
        self.assertEqual(s["views_uncompared"], [])
        self.assertEqual(s["outputs_compared"], len(KEYS) * 4)
        self.assertEqual(s["differing"], 0)

    def test_a_byte_difference_is_reported(self) -> None:
        base = _capture(self.root / "base", verbatim=True)
        head = _capture(self.root / "head", verbatim=True, content="moved")
        rc, text, s = self._run(base, head)
        self.assertEqual(rc, 1)
        self.assertEqual(s["differing"], len(KEYS) * 4)
        self.assertIn("DIFF rust/fx/a.rs.verbatim.jsonl", text)

    def test_a_view_on_one_side_only_is_refused(self) -> None:
        base = _capture(self.root / "base", verbatim=False)
        head = _capture(self.root / "head", verbatim=True)
        rc, text, s = self._run(base, head)
        self.assertEqual(rc, 2)
        self.assertIn("VIEW only in head: verbatim", text)
        self.assertIn("--only-shared-views", text)
        self.assertEqual(s, {}, "no summary: nothing was compared")

    def test_only_shared_views_compares_the_rest_and_names_the_omission(self) -> None:
        base = _capture(self.root / "base", verbatim=False)
        head = _capture(self.root / "head", verbatim=True)
        rc, text, s = self._run(base, head, "--only-shared-views")
        self.assertEqual(rc, 0)
        self.assertIn("VIEW only in head: verbatim", text)
        self.assertEqual(s["views_compared"], ["ops", "dump", "metrics"])
        self.assertEqual(s["views_uncompared"], ["verbatim"])
        self.assertEqual(s["outputs_compared"], len(KEYS) * 3)

    def test_a_legacy_manifest_without_views_carries_the_verbatim_view(self) -> None:
        base = _capture(self.root / "base", verbatim=True, views_field=False)
        head = _capture(self.root / "head", verbatim=True)
        rc, _, s = self._run(base, head)
        self.assertEqual(rc, 0)
        self.assertEqual(s["views_compared"], ["ops", "dump", "metrics", "verbatim"])

    def test_no_shared_view_is_refused(self) -> None:
        base = _capture(self.root / "base", verbatim=False)
        head = _capture(self.root / "head", verbatim=True)
        mh = json.loads((head / "manifest.json").read_text())
        mh["views"] = ["verbatim"]
        (head / "manifest.json").write_text(json.dumps(mh))
        rc, text, _ = self._run(base, head, "--only-shared-views")
        self.assertEqual(rc, 2)
        self.assertIn("no view on both sides", text)

    def test_a_key_on_one_side_is_a_problem_not_a_skip(self) -> None:
        base = _capture(self.root / "base", verbatim=True)
        head = _capture(self.root / "head", verbatim=True, keys=KEYS[:1])
        rc, text, s = self._run(base, head)
        self.assertEqual(rc, 1)
        self.assertIn("KEY only in base: python/fx/b.py", text)
        self.assertEqual(s["key_problems"], 1)
        self.assertEqual(s["outputs_compared"], 4)

    def test_a_missing_output_file_is_a_problem(self) -> None:
        base = _capture(self.root / "base", verbatim=True)
        head = _capture(self.root / "head", verbatim=True)
        (head / "rust" / "fx" / "a.rs.ops.txt").unlink()
        rc, text, s = self._run(base, head)
        self.assertEqual(rc, 1)
        self.assertIn("MISSING output: rust/fx/a.rs.ops.txt", text)
        self.assertEqual(s["key_problems"], 1)

    def test_incomplete_or_mismatched_captures_are_refused(self) -> None:
        good = _capture(self.root / "good", verbatim=True)
        incomplete = _capture(self.root / "incomplete", verbatim=True, complete=False)
        rc, text, _ = self._run(good, incomplete)
        self.assertEqual(rc, 2)
        self.assertIn("head capture is not complete", text)
        other_input = _capture(self.root / "input", verbatim=True)
        m = json.loads((other_input / "manifest.json").read_text())
        m["inputs_sha256"][KEYS[0]] = "1" * 64
        (other_input / "manifest.json").write_text(json.dumps(m))
        rc, text, _ = self._run(good, other_input)
        self.assertEqual(rc, 2)
        self.assertIn("INPUT changed between captures", text)
        other_pin = _capture(self.root / "pin", verbatim=True)
        m = json.loads((other_pin / "manifest.json").read_text())
        m["grammar_pins"] = {}
        (other_pin / "manifest.json").write_text(json.dumps(m))
        rc, text, _ = self._run(good, other_pin)
        self.assertEqual(rc, 2)
        self.assertIn("grammar pins differ", text)

    def test_lang_filter_narrows_the_populations(self) -> None:
        base = _capture(self.root / "base", verbatim=True)
        head = _capture(self.root / "head", verbatim=True)
        rc, _, s = self._run(base, head, "--lang", "rust")
        self.assertEqual(rc, 0)
        self.assertEqual(s["populations_compared"], ["rust"])
        self.assertEqual(s["outputs_compared"], 4)
        rc, text, _ = self._run(base, head, "--lang", "go")
        self.assertEqual(rc, 2)
        self.assertIn("no keys for ['go']", text)


if __name__ == "__main__":
    unittest.main()
