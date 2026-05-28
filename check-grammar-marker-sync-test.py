#!/usr/bin/env python3
"""Tests for check-grammar-marker-sync.py.

Each test stages a synthetic mini-repo in a tempdir (script + the
two vendored Cargo.toml stubs + optional baseline), runs the
script as a subprocess from there, and asserts on exit code and
stderr/stdout. Subprocess invocation is the most representative
shape — the script's behaviour is keyed off
`pathlib.Path(__file__).resolve().parent` so co-locating with the
fixture is enough to redirect every path.

Run with: python3 -m unittest check-grammar-marker-sync-test.py
Or:       python3 check-grammar-marker-sync-test.py
"""

from __future__ import annotations

import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parent
SCRIPT_SRC = REPO_ROOT / "check-grammar-marker-sync.py"


def _make_fixture(
    tmpdir: pathlib.Path,
    *,
    mozjs_version: str | None = "0.25.0",
    mozcpp_version: str | None = "0.23.4",
    baseline: str | None = None,
) -> pathlib.Path:
    """Stage script + Cargo.toml stubs (+ optional baseline) in tmpdir.

    Returns the path to the staged script.
    """
    script_path = tmpdir / SCRIPT_SRC.name
    shutil.copy(SCRIPT_SRC, script_path)

    (tmpdir / "tree-sitter-mozjs").mkdir()
    (tmpdir / "tree-sitter-mozcpp").mkdir()

    if mozjs_version is not None:
        (tmpdir / "tree-sitter-mozjs" / "Cargo.toml").write_text(
            "[build-dependencies]\n"
            f'tree-sitter-javascript = "{mozjs_version}"\n'
            "\n"
            "[package.metadata.cargo-udeps.ignore]\n"
            'build = ["tree-sitter-javascript"]\n'
        )
    else:
        (tmpdir / "tree-sitter-mozjs" / "Cargo.toml").write_text(
            '[package]\nname = "stub"\n'
        )

    if mozcpp_version is not None:
        (tmpdir / "tree-sitter-mozcpp" / "Cargo.toml").write_text(
            "[build-dependencies]\n"
            f'tree-sitter-cpp = "{mozcpp_version}"\n'
        )
    else:
        (tmpdir / "tree-sitter-mozcpp" / "Cargo.toml").write_text(
            '[package]\nname = "stub"\n'
        )

    if baseline is not None:
        (tmpdir / ".grammar-marker-baseline.toml").write_text(baseline)

    return script_path


def _run(
    script: pathlib.Path, *args: str
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(script), *args],
        capture_output=True,
        text=True,
        check=False,
    )


_BASELINE_MATCHING = (
    "# Top-of-file header survives.\n"
    "\n"
    "[mozjs]\n"
    'marker = "tree-sitter-javascript"\n'
    'version = "0.25.0"\n'
    "\n"
    "[mozcpp]\n"
    'marker = "tree-sitter-cpp"\n'
    'version = "0.23.4"\n'
)


class GrammarMarkerSyncTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmpdir = pathlib.Path(tempfile.mkdtemp(prefix="bca-gms-"))

    def tearDown(self) -> None:
        shutil.rmtree(self.tmpdir, ignore_errors=True)

    def test_matching_baseline_passes(self) -> None:
        script = _make_fixture(self.tmpdir, baseline=_BASELINE_MATCHING)
        result = _run(script)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("OK", result.stdout)

    def test_marker_bumped_without_regen_trips(self) -> None:
        script = _make_fixture(
            self.tmpdir,
            mozjs_version="0.26.0",
            baseline=_BASELINE_MATCHING,
        )
        result = _run(script)
        self.assertEqual(result.returncode, 1)
        self.assertIn("mozjs", result.stderr)
        self.assertIn("0.25.0", result.stderr)
        self.assertIn("0.26.0", result.stderr)

    def test_baseline_ahead_of_cargo_trips(self) -> None:
        baseline = _BASELINE_MATCHING.replace(
            'version = "0.25.0"', 'version = "0.27.0"'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script)
        self.assertEqual(result.returncode, 1)
        self.assertIn("0.27.0", result.stderr)

    def test_missing_marker_line_returns_2(self) -> None:
        script = _make_fixture(
            self.tmpdir,
            mozjs_version=None,
            baseline=_BASELINE_MATCHING,
        )
        result = _run(script)
        self.assertEqual(result.returncode, 2)
        self.assertIn("not found", result.stderr.lower())
        self.assertIn("tree-sitter-javascript", result.stderr)

    def test_missing_baseline_returns_2_with_hint(self) -> None:
        script = _make_fixture(self.tmpdir, baseline=None)
        result = _run(script)
        self.assertEqual(result.returncode, 2)
        self.assertIn("missing", result.stderr.lower())
        self.assertIn("--update", result.stderr)

    def test_malformed_toml_returns_2_without_traceback(self) -> None:
        script = _make_fixture(
            self.tmpdir,
            baseline="[mozjs\nthis is not toml",
        )
        result = _run(script)
        self.assertEqual(result.returncode, 2)
        self.assertIn("TOML", result.stderr)
        self.assertNotIn("Traceback", result.stderr)

    def test_update_preserves_per_section_comments(self) -> None:
        baseline = (
            "# File-level header.\n"
            "\n"
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            'version = "0.25.0"\n'
            "# Audit note: pinned per #400 verification on 2026-05-28.\n"
            "\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
            "# Audit note: pinned pending #406 regen review.\n"
        )
        script = _make_fixture(
            self.tmpdir,
            mozjs_version="0.26.0",
            mozcpp_version="0.23.4",
            baseline=baseline,
        )
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        self.assertIn("File-level header.", updated)
        self.assertIn("pinned per #400 verification on 2026-05-28.", updated)
        self.assertIn("pinned pending #406 regen review.", updated)
        self.assertIn('version = "0.26.0"', updated)
        # Mozcpp version is unchanged but the line should still be
        # present in its original location.
        self.assertIn('version = "0.23.4"', updated)
        # Re-running the gate must now pass.
        post = _run(script)
        self.assertEqual(post.returncode, 0, post.stderr)
        self.assertIn("OK", post.stdout)

    def test_update_appends_missing_section_with_template(self) -> None:
        # Baseline knows about mozjs but not mozcpp; --update must
        # add the mozcpp section without touching the existing one.
        baseline = (
            "# Header.\n"
            "\n"
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            'version = "0.25.0"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        self.assertIn("[mozjs]", updated)
        self.assertIn("[mozcpp]", updated)
        self.assertIn('version = "0.23.4"', updated)
        post = _run(script)
        self.assertEqual(post.returncode, 0, post.stderr)

    def test_update_creates_baseline_from_scratch(self) -> None:
        script = _make_fixture(self.tmpdir, baseline=None)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        self.assertIn("Grammar-marker-sync baseline", updated)
        self.assertIn("[mozjs]", updated)
        self.assertIn("[mozcpp]", updated)
        post = _run(script)
        self.assertEqual(post.returncode, 0, post.stderr)

    def test_marker_name_drift_caught(self) -> None:
        # Baseline records the wrong marker name for mozjs.
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-mozjs-grammar"\n'
            'version = "0.25.0"\n'
            "\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script)
        self.assertEqual(result.returncode, 1)
        self.assertIn("marker name drift", result.stderr)


if __name__ == "__main__":
    unittest.main()
