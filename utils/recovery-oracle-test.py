#!/usr/bin/env python3
"""Self-tests for `utils/recovery-oracle.py` on stub binaries: the cut
set reaches every token class lesson 102 names, equal sides are zero,
a drift is a difference, outputs compare as bytes, a crash on both
sides is a failure rather than parity, nested fixture roots are walked,
an oversized fixture is refused up front, and the cut inputs are
written one at a time under `TMPDIR`, never a fixed path.

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
ECHO = "sys.stdout.buffer.write((' '.join(sys.argv[1:]) + '\\n').encode() + open(sys.argv[-1], 'rb').read())"
# Echoes the argv and the input, and for a `bca` invocation appends a
# marker to any input holding a stranded `else` at the end.
DRIFT = (
    "data = open(sys.argv[-1], 'rb').read()\n"
    "sys.stdout.buffer.write((' '.join(sys.argv[1:]) + '\\n').encode() + data)\n"
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

    def test_outputs_compare_as_bytes(self) -> None:
        # Text capture with errors="replace" folds b"\xff\r\n" and
        # b"\xfe\n" to the same "\ufffd\n": a real drift reads as parity.
        base = _stub(self.root / "bca-base", "sys.stdout.buffer.write(b'\\xff\\r\\n')")
        head = _stub(self.root / "bca-head", "sys.stdout.buffer.write(b'\\xfe\\n')")
        v = _stub(self.root / "verbatim", ECHO)
        rc, out = self._run(base, head, v, v)
        self.assertEqual(rc, 1)
        self.assertIn(f"differing: {len(ro.cuts_for(FIXTURE)) * len(ro.CLI)}; failed executions: 0", out)

    def test_verbatim_argv_is_slug_then_path(self) -> None:
        # The driver <-> bin contract: `verbatim-oracle <slug> <path>`.
        log = self.root / "argv.log"
        bca = _stub(self.root / "bca", ECHO)
        v = _stub(self.root / "verbatim", f"open({str(log)!r}, 'a').write(repr(sys.argv[1:]) + '\\n')")
        rc, _ = self._run(bca, bca, v, v)
        self.assertEqual(rc, 0)
        lines = log.read_text().splitlines()
        self.assertEqual(len(lines), 2 * len(ro.cuts_for(FIXTURE)))
        for line in lines:
            slug, path = eval(line)  # noqa: S307 — our own repr
            self.assertEqual(slug, "rust")
            self.assertTrue(path.endswith("-rust.rs"), path)
            self.assertIn("bca-recovery-cuts.", path)

    def test_nested_fixture_roots_are_walked_and_a_symlink_is_skipped(self) -> None:
        # The fixture store this landed is `control_flow/<lang>/`; a flat
        # iterdir found nothing there (D6 on #1744).
        (self.fixtures / "a.rs").unlink()
        (self.fixtures / "rust").mkdir()
        (self.fixtures / "rust" / "r07.rs").write_bytes(FIXTURE)
        (self.fixtures / "python").mkdir()
        (self.fixtures / "python" / "r07.py").write_bytes(b"if a:\n  pass\n")
        os.symlink(self.fixtures / "rust" / "r07.rs", self.fixtures / "link.rs")
        bca = _stub(self.root / "bca", ECHO)
        rc, out = self._run(bca, bca, bca, bca)
        self.assertEqual(rc, 0)
        record = json.loads(self.record.read_text())
        self.assertEqual(sorted(record["cuts"]), ["fx#0/python/r07.py", "fx#0/rust/r07.rs"])
        self.assertEqual(record["skipped_symlinks"], [str(self.fixtures / "link.rs")])
        self.assertIn("differing: 0", out)

    def test_fixtures_whose_paths_collide_under_a_separator_rewrite_get_distinct_cuts(self) -> None:
        # `a/b.rs` and `a__b.rs` spelled their scratch cut name the same
        # way, so two concurrent jobs could read and unlink each other's
        # cut (N1 on #1744). The race is too narrow to provoke reliably
        # with two small fixtures, so this pins the property the fix
        # rests on instead: every (fixture, cut, lang) job is handed a
        # distinct scratch path. The stub records the path it was given.
        (self.fixtures / "a.rs").unlink()
        (self.fixtures / "a").mkdir()
        (self.fixtures / "a" / "b.rs").write_bytes(FIXTURE)
        (self.fixtures / "a__b.rs").write_bytes(FIXTURE)
        log = self.root / "paths.log"
        seen = _stub(self.root / "seen", f"open({str(log)!r}, 'a').write(sys.argv[-1] + '\\n')")
        rc, out = self._run(seen, seen, seen, seen, "--jobs", "1")
        self.assertEqual(rc, 0, out)
        paths = log.read_text().split()
        per_cut = 2 * (len(ro.CLI) + 1)
        expected_jobs = 2 * len(ro.cuts_for(FIXTURE))
        self.assertEqual(len(paths), expected_jobs * per_cut)
        # Each job's path is seen exactly `per_cut` times, by that job
        # alone: a shared name would appear twice as often.
        self.assertEqual(len(set(paths)), expected_jobs)
        record = json.loads(self.record.read_text())
        self.assertEqual(sorted(record["cuts"]), ["fx#0/a/b.rs", "fx#0/a__b.rs"])

    def test_an_oversized_fixture_is_refused_before_any_cut(self) -> None:
        (self.fixtures / "big.rs").write_bytes(b"x" * 100)
        bca = _stub(self.root / "bca", ECHO)
        rc, _ = self._run(bca, bca, bca, bca, "--max-bytes", "64")
        self.assertTrue(str(rc).startswith(f"{self.fixtures / 'big.rs'} is 100 bytes, over --max-bytes 64"), rc)
        self.assertIn("quadratic", str(rc))
        self.assertFalse(self.record.exists())
        rc, _ = self._run(bca, bca, bca, bca, "--max-bytes", "100")
        self.assertEqual(rc, 0)

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

    def test_each_cut_is_written_for_its_job_and_removed_after(self) -> None:
        # Writing every cut up front is O(n²) disk; the stub counts the
        # cut files beside its input, which must never exceed the job
        # width.
        counts = self.root / "counts"
        stub = _stub(
            self.root / "count",
            "import os\n"
            f"open({str(counts)!r}, 'a').write(str(len(os.listdir(os.path.dirname(sys.argv[-1])))) + '\\n')",
        )
        rc, _ = self._run(stub, stub, stub, stub, "--jobs", "2")
        self.assertEqual(rc, 0)
        seen = [int(x) for x in counts.read_text().split()]
        self.assertTrue(seen)
        self.assertLessEqual(max(seen), 2, "at most one cut file per concurrent job")

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
