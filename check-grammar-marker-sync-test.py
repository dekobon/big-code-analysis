#!/usr/bin/env python3
"""Tests for check-grammar-marker-sync.py.

Each test stages a synthetic mini-repo in a tempdir (the script
plus the two vendored Cargo.toml stubs plus an optional baseline)
and runs the script as a subprocess. Subprocess invocation is the
most representative shape — the script's behaviour is keyed off
`pathlib.Path(__file__).resolve().parent`, so co-locating with
the fixture is enough to redirect every path.

Run with:
    python3 -m unittest -q check-grammar-marker-sync-test.py
Or:
    python3 check-grammar-marker-sync-test.py
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
LIVE_BASELINE = REPO_ROOT / ".grammar-marker-baseline.toml"


_MOZJS_BARE = (
    "[build-dependencies]\n"
    'tree-sitter-javascript = "{version}"\n'
    "\n"
    "[package.metadata.cargo-udeps.ignore]\n"
    'build = ["tree-sitter-javascript"]\n'
)
_MOZCPP_BARE = (
    "[build-dependencies]\n"
    'tree-sitter-cpp = "{version}"\n'
    "\n"
    "[package.metadata.cargo-udeps.ignore]\n"
    'build = ["tree-sitter-cpp"]\n'
)


def _make_fixture(
    tmpdir: pathlib.Path,
    *,
    mozjs_manifest: str | None = None,
    mozcpp_manifest: str | None = None,
    mozjs_version: str | None = "0.25.0",
    mozcpp_version: str | None = "0.23.4",
    baseline: str | None = None,
) -> pathlib.Path:
    """Stage script + Cargo.toml stubs (+ optional baseline) in tmpdir.

    Pass `mozjs_manifest` / `mozcpp_manifest` to override the
    default bare-version template (e.g. to exercise the
    inline-table form or a missing-file scenario). Pass
    `mozjs_version=None` to omit the marker line entirely (real
    Cargo.toml without the marker — must still trip the gate).
    Returns the path to the staged script.
    """
    script_path = tmpdir / SCRIPT_SRC.name
    shutil.copy(SCRIPT_SRC, script_path)

    mozjs_dir = tmpdir / "tree-sitter-mozjs"
    mozcpp_dir = tmpdir / "tree-sitter-mozcpp"
    mozjs_dir.mkdir()
    mozcpp_dir.mkdir()

    if mozjs_manifest is not None:
        (mozjs_dir / "Cargo.toml").write_text(mozjs_manifest)
    elif mozjs_version is not None:
        (mozjs_dir / "Cargo.toml").write_text(
            _MOZJS_BARE.format(version=mozjs_version)
        )
    else:
        (mozjs_dir / "Cargo.toml").write_text(
            '[package]\nname = "stub"\n'
        )

    if mozcpp_manifest is not None:
        (mozcpp_dir / "Cargo.toml").write_text(mozcpp_manifest)
    elif mozcpp_version is not None:
        (mozcpp_dir / "Cargo.toml").write_text(
            _MOZCPP_BARE.format(version=mozcpp_version)
        )
    else:
        (mozcpp_dir / "Cargo.toml").write_text(
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
        # Track the tmpdir so a stray file written outside it fails
        # the test rather than being silently swallowed in tearDown.
        self._tmpdir_str = str(self.tmpdir)

    def tearDown(self) -> None:
        # Fail loudly if the script wrote files outside the tmpdir
        # (catches REPO_ROOT-computation regressions that the prior
        # `ignore_errors=True` rmtree would have hidden).
        shutil.rmtree(self.tmpdir)

    # --- happy path & basic drift ---

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

    # --- Cargo.toml-side parsing (tomllib path) ---

    def test_inline_table_marker_form_supported(self) -> None:
        # `tree-sitter-javascript = { version = "0.25.0", features = [...] }`
        # is a legitimate Cargo idiom and must not blind the gate.
        mozjs_manifest = (
            "[build-dependencies]\n"
            'tree-sitter-javascript = { version = "0.25.0", '
            'features = ["foo"] }\n'
        )
        script = _make_fixture(
            self.tmpdir,
            mozjs_manifest=mozjs_manifest,
            baseline=_BASELINE_MATCHING,
        )
        result = _run(script)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_marker_inside_multiline_string_does_not_false_match(self) -> None:
        # A docstring that mentions the marker MUST NOT shadow the
        # real `[build-dependencies]` entry.
        mozjs_manifest = (
            "[package]\n"
            'description = """\n'
            '  tree-sitter-javascript = "99.99.99" is the marker.\n'
            '"""\n'
            "[build-dependencies]\n"
            'tree-sitter-javascript = "0.25.0"\n'
        )
        script = _make_fixture(
            self.tmpdir,
            mozjs_manifest=mozjs_manifest,
            baseline=_BASELINE_MATCHING,
        )
        result = _run(script)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("99.99.99", result.stderr)

    def test_missing_manifest_returns_2_without_traceback(self) -> None:
        script = _make_fixture(
            self.tmpdir,
            mozjs_version="0.25.0",
            baseline=_BASELINE_MATCHING,
        )
        (self.tmpdir / "tree-sitter-mozjs" / "Cargo.toml").unlink()
        result = _run(script)
        self.assertEqual(result.returncode, 2)
        self.assertIn("not found", result.stderr.lower())
        self.assertNotIn("Traceback", result.stderr)

    def test_missing_marker_line_returns_2(self) -> None:
        script = _make_fixture(
            self.tmpdir,
            mozjs_version=None,
            baseline=_BASELINE_MATCHING,
        )
        result = _run(script)
        self.assertEqual(result.returncode, 2)
        self.assertIn("not found", result.stderr.lower())

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

    # --- baseline-side parsing & non-string values ---

    def test_non_string_version_value_caught(self) -> None:
        # TOML accepts bare floats, but the gate must surface the
        # actual baseline value (not "None") so the user sees what
        # they typed.
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            "version = 0.25\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script)
        self.assertEqual(result.returncode, 1)
        self.assertIn("0.25", result.stderr)
        self.assertNotIn("baseline None", result.stderr)

    def test_marker_name_drift_caught(self) -> None:
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript-TYPO"\n'
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

    # --- --update behaviour ---

    def test_update_preserves_per_section_comments(self) -> None:
        baseline = (
            "# File-level header.\n"
            "\n"
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            'version = "0.25.0"\n'
            "# Audit note for mozjs.\n"
            "\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
            "# Audit note for mozcpp.\n"
        )
        script = _make_fixture(
            self.tmpdir,
            mozjs_version="0.26.0",
            baseline=baseline,
        )
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        self.assertIn("File-level header.", updated)
        self.assertIn("Audit note for mozjs.", updated)
        self.assertIn("Audit note for mozcpp.", updated)
        self.assertIn('version = "0.26.0"', updated)
        post = _run(script)
        self.assertEqual(post.returncode, 0, post.stderr)

    def test_update_heals_marker_name_drift(self) -> None:
        # The earlier review caught that --update only touched the
        # `version` line, so a marker-name typo could not be
        # recovered via the documented workflow.
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript-TYPO"\n'
            'version = "0.25.0"\n'
            "\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        post = _run(script)
        self.assertEqual(post.returncode, 0, post.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        self.assertNotIn("TYPO", updated)

    def test_update_inserts_missing_version_into_existing_section(self) -> None:
        # Earlier review caught that a section header with no
        # version line caused --update to append a duplicate header
        # that then broke `tomllib.loads`.
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            "# version pending verification\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        # No duplicate [mozjs] section.
        self.assertEqual(updated.count("[mozjs]"), 1)
        # Re-running the gate must succeed (file is still valid TOML).
        post = _run(script)
        self.assertEqual(post.returncode, 0, post.stderr)

    def test_update_inserts_missing_marker_into_existing_section(self) -> None:
        baseline = (
            "[mozjs]\n"
            "# marker pending\n"
            'version = "0.25.0"\n'
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        self.assertEqual(updated.count("[mozjs]"), 1)
        self.assertIn('marker = "tree-sitter-javascript"', updated)

    def test_update_with_section_header_inline_comment(self) -> None:
        # `[mozjs] # the JS one` is valid TOML; the gate must
        # locate it as the existing section, not append a duplicate.
        baseline = (
            "[mozjs] # the JS one\n"
            'marker = "tree-sitter-javascript"\n'
            'version = "0.24.0"\n'
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        # The inline comment survives, and there is no duplicate
        # section header.
        self.assertEqual(updated.count("[mozjs]"), 1)
        self.assertIn("the JS one", updated)
        self.assertIn('version = "0.25.0"', updated)

    def test_update_with_dotted_child_section(self) -> None:
        # A dotted-child table `[mozjs.notes]` belongs to mozjs;
        # its body must not be treated as `[mozjs]`'s body during
        # version replacement, and the next-section boundary must
        # respect the child.
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            'version = "0.24.0"\n'
            "\n"
            "[mozjs.notes]\n"
            'audit = "documented"\n'
            "\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        self.assertIn("[mozjs.notes]", updated)
        self.assertIn('audit = "documented"', updated)
        self.assertIn('version = "0.25.0"', updated)

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

    # --- orphan section handling ---

    def test_orphan_section_warns_but_passes_when_known_match(self) -> None:
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            'version = "0.25.0"\n'
            "\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
            "\n"
            "[mozold]\n"
            'marker = "tree-sitter-old"\n'
            'version = "0.1.0"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("warning", result.stderr.lower())
        self.assertIn("mozold", result.stderr)

    def test_orphan_section_warning_on_update_path(self) -> None:
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            'version = "0.25.0"\n'
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
            "[mozold]\n"
            'marker = "tree-sitter-old"\n'
            'version = "0.1.0"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("warning", result.stderr.lower())
        self.assertIn("mozold", result.stderr)
        # Orphan must still be present (we don't auto-delete).
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        self.assertIn("[mozold]", updated)

    # --- header sync invariant (defends against script vs on-disk drift) ---

    def test_live_baseline_starts_with_embedded_header(self) -> None:
        # If the on-disk baseline doesn't begin with the
        # script-embedded header, a future --update on a deleted
        # baseline will regenerate a file with a different header
        # than the canonical one — exactly the drift the review
        # called out. The embedded header is intentionally minimal
        # so this invariant is cheap to maintain.
        if not LIVE_BASELINE.exists():
            self.skipTest("live baseline not present (running outside repo)")
        live_text = LIVE_BASELINE.read_text(encoding="utf-8")
        # Import the script as a module to grab _BASELINE_HEADER.
        import importlib.util

        spec = importlib.util.spec_from_file_location(
            "_check_grammar_marker_sync", SCRIPT_SRC
        )
        assert spec is not None and spec.loader is not None
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        self.assertTrue(
            live_text.startswith(mod._BASELINE_HEADER),
            "On-disk .grammar-marker-baseline.toml does not begin with the "
            "script's _BASELINE_HEADER constant — update one or the other "
            "so that --update on a deleted baseline regenerates the same "
            "header users see on disk.",
        )


if __name__ == "__main__":
    unittest.main()
