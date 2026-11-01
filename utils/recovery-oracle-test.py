#!/usr/bin/env python3
"""Self-tests for `utils/recovery-oracle.py` on stub binaries: the cut
set reaches every token class lesson 102 names, equal sides are zero,
a drift is a difference, a crash on both sides is a failure rather than
parity, and the cut inputs are written under `TMPDIR`, never a fixed
path.

Run: ``python3 -B -m unittest -q utils/recovery-oracle-test.py``.
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
from unittest import mock

UTILS_DIR = pathlib.Path(__file__).resolve().parent


def _load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, UTILS_DIR / filename)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


ro = _load("recovery_oracle", "recovery-oracle.py")

FIXTURE = b"fn f(a: i32) -> i32 {\n if a > 1 { 1 } else { 0 }\n}\n"


def _stub(path: pathlib.Path, body: str) -> str:
    path.write_text(f"#!{sys.executable}\nimport sys\n{body}\n")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)
    return str(path)


# Echoes the argv and the input bytes, so output depends on the cut.
ECHO = "sys.stdout.write(' '.join(sys.argv[1:]) + '\\n' + open(sys.argv[-1], 'rb').read().decode())"
# Echoes the argv and the input, and for a `bca` invocation appends a
# marker to any input holding a stranded `else` at the end.
DRIFT = (
    "data = open(sys.argv[-1], 'rb').read()\n"
    "sys.stdout.write(' '.join(sys.argv[1:]) + '\\n' + data.decode())\n"
    "if data.rstrip().endswith(b'else'): sys.stdout.write('DRIFT\\n')"
)


class CutsTests(unittest.TestCase):
    def test_cuts_reach_every_token_class(self) -> None:
        cuts = ro.cuts_for(FIXTURE)
        self.assertEqual(cuts, sorted(set(cuts)))
        self.assertTrue(all(0 < c < len(FIXTURE) for c in cuts))
        text = FIXTURE.decode()
        # Every line end, every operator-plus-one, and the start / end /
        # end+1 of every word.
        for line_end in (text.index("\n"), text.index("\n", text.index("\n") + 1)):
            self.assertIn(line_end, cuts)
        self.assertIn(text.index(">") + 1, cuts)
        self.assertIn(text.index("else"), cuts)
        self.assertIn(text.index("else") + 4, cuts)
        self.assertIn(text.index("else") + 5, cuts)
        self.assertIn(text.index("fn") + 2, cuts)


class SweepTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        self.fixtures = self.root / "fx"
        self.fixtures.mkdir()
        (self.fixtures / "a.rs").write_bytes(FIXTURE)
        self.record = self.root / "record.json"

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _run(self, base_bca: str, head_bca: str, base_v: str, head_v: str, *extra: str) -> tuple[int, str]:
        argv = [
            "--base-bca", base_bca, "--head-bca", head_bca,
            "--base-verbatim", base_v, "--head-verbatim", head_v,
            "--root", str(self.fixtures), "--jobs", "2", "--record", str(self.record), *extra,
        ]
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            try:
                rc = ro.main(argv)
            except SystemExit as e:
                rc = e.code
        return rc, out.getvalue()

    def test_equal_sides_report_zero_and_record_the_cut_set(self) -> None:
        bca = _stub(self.root / "bca", ECHO)
        v = _stub(self.root / "verbatim", ECHO)
        rc, out = self._run(bca, bca, v, v)
        self.assertEqual(rc, 0)
        cuts = ro.cuts_for(FIXTURE)
        self.assertIn(f"recovery inputs x (lang, cmd): {len(cuts) * (len(ro.CLI) + 1)}; differing: 0; failed executions: 0", out)
        record = json.loads(self.record.read_text())
        self.assertEqual(record["cuts"], {"fx#0/a.rs": cuts})
        self.assertEqual(record["fixtures"], 1)

    def test_a_drift_on_one_side_is_a_difference(self) -> None:
        base = _stub(self.root / "bca-base", ECHO)
        head = _stub(self.root / "bca-head", DRIFT)
        v = _stub(self.root / "verbatim", ECHO)
        rc, out = self._run(base, head, v, v)
        self.assertEqual(rc, 1)
        text = FIXTURE.decode()
        # Two cuts strand `else` (its end and end+1, the trailing space);
        # every CLI command differs there and the verbatim view does not.
        self.assertIn(f"differing: {2 * len(ro.CLI)}; failed executions: 0", out)
        for cut in (text.index("else") + 4, text.index("else") + 5):
            self.assertIn(f"DIFF ('fx#0/a.rs', {cut}, 'rust', 'ops -O json')", out)
        self.assertNotIn("'verbatim')", out)

    def test_a_crash_on_both_sides_is_a_failure_not_parity(self) -> None:
        crash = _stub(self.root / "crash", "sys.exit(2)")
        v = _stub(self.root / "verbatim", ECHO)
        rc, out = self._run(crash, crash, v, v)
        self.assertEqual(rc, 1)
        n = len(ro.cuts_for(FIXTURE)) * len(ro.CLI) * 2
        self.assertIn(f"failed executions: {n}", out)
        self.assertIn("FAIL exit 2", out)
        self.assertEqual(len(json.loads(self.record.read_text())["failures"]), n)

    def test_cut_inputs_live_under_tmpdir_and_are_removed(self) -> None:
        bca = _stub(self.root / "bca", ECHO)
        scratch = self.root / "scratch"
        scratch.mkdir()
        with mock.patch.dict(os.environ, {"TMPDIR": str(scratch)}):
            tempfile.tempdir = None
            try:
                rc, _ = self._run(bca, bca, bca, bca)
            finally:
                tempfile.tempdir = None
        self.assertEqual(rc, 0)
        self.assertEqual(list(scratch.iterdir()), [], "the cut directory was removed")
        cuts_dir = self.root / "scratch-seen"
        cuts_dir.mkdir()
        # The stub records where its input lived; every cut file must be
        # under the TMPDIR the run was given.
        seen = _stub(self.root / "seen", f"open({str(cuts_dir / 'paths')!r}, 'a').write(sys.argv[-1] + '\\n')")
        with mock.patch.dict(os.environ, {"TMPDIR": str(scratch)}):
            tempfile.tempdir = None
            try:
                self._run(seen, seen, seen, seen)
            finally:
                tempfile.tempdir = None
        paths = (cuts_dir / "paths").read_text().split()
        self.assertTrue(paths)
        self.assertTrue(all(p.startswith(str(scratch) + "/bca-recovery-cuts.") for p in paths), paths[:3])

    def test_missing_binary_or_root_fails_before_the_sweep(self) -> None:
        bca = _stub(self.root / "bca", ECHO)
        rc, _ = self._run(bca, bca, bca, str(self.root / "none"))
        self.assertEqual(rc, f"missing binary {self.root / 'none'}")
        rc, _ = self._run(bca, bca, bca, bca, "--root", str(self.root / "nope"))
        self.assertEqual(rc, f"missing fixture root {self.root / 'nope'}")

    def test_an_unknown_extension_is_skipped_and_named(self) -> None:
        (self.fixtures / "a.rs").unlink()
        (self.fixtures / "a.txt").write_bytes(FIXTURE)
        bca = _stub(self.root / "bca", ECHO)
        rc, _ = self._run(bca, bca, bca, bca)
        self.assertEqual(rc, "no fixtures matched a known extension")


if __name__ == "__main__":
    unittest.main()
