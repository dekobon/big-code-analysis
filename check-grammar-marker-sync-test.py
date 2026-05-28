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
    default bare-version template. Pass `mozjs_version=None` to
    omit the marker line entirely. Returns the path to the
    staged script.
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
        (mozjs_dir / "Cargo.toml").write_text('[package]\nname = "stub"\n')

    if mozcpp_manifest is not None:
        (mozcpp_dir / "Cargo.toml").write_text(mozcpp_manifest)
    elif mozcpp_version is not None:
        (mozcpp_dir / "Cargo.toml").write_text(
            _MOZCPP_BARE.format(version=mozcpp_version)
        )
    else:
        (mozcpp_dir / "Cargo.toml").write_text('[package]\nname = "stub"\n')

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
        # rmtree with onexc that chmod's read-only entries back
        # to writable so a future test that exercises a read-only
        # fixture (a chmod'd Cargo.toml under a chmod'd dir) does
        # not mask the real assertion under a tearDown PermissionError.
        def _force_rm(func, path, exc):  # type: ignore[no-untyped-def]
            try:
                import os, stat
                os.chmod(path, stat.S_IRWXU)
                func(path)
            except OSError:
                pass

        # Python 3.12 renamed onerror -> onexc; tolerate both.
        try:
            shutil.rmtree(self.tmpdir, onexc=_force_rm)  # type: ignore[call-arg]
        except TypeError:
            shutil.rmtree(self.tmpdir, onerror=_force_rm)  # type: ignore[arg-type]

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
        self.assertIn("OK", result.stdout)

    def test_marker_inside_multiline_string_does_not_false_match(self) -> None:
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
        self.assertIn("OK", result.stdout)
        self.assertNotIn("99.99.99", result.stderr)

    def test_workspace_dependencies_marker_resolved(self) -> None:
        # Recursive dep scan must find the marker even when it
        # lives under a nested table like [workspace.dependencies].
        mozjs_manifest = (
            "[workspace.dependencies]\n"
            'tree-sitter-javascript = "0.25.0"\n'
        )
        script = _make_fixture(
            self.tmpdir,
            mozjs_manifest=mozjs_manifest,
            baseline=_BASELINE_MATCHING,
        )
        result = _run(script)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("OK", result.stdout)

    def test_target_conditional_dependencies_marker_resolved(self) -> None:
        mozjs_manifest = (
            '[target."cfg(unix)".build-dependencies]\n'
            'tree-sitter-javascript = "0.25.0"\n'
        )
        script = _make_fixture(
            self.tmpdir,
            mozjs_manifest=mozjs_manifest,
            baseline=_BASELINE_MATCHING,
        )
        result = _run(script)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("OK", result.stdout)

    def test_inline_table_without_version_returns_marker_not_found(self) -> None:
        # `{ workspace = true }` and similar forms have the marker
        # name but no explicit version pin. Document the contract
        # — we report "marker line not found" since the gate has
        # no version to compare against.
        mozjs_manifest = (
            "[build-dependencies]\n"
            "tree-sitter-javascript = { workspace = true }\n"
        )
        script = _make_fixture(
            self.tmpdir,
            mozjs_manifest=mozjs_manifest,
            baseline=_BASELINE_MATCHING,
        )
        result = _run(script)
        self.assertEqual(result.returncode, 2)
        self.assertIn("not found", result.stderr.lower())

    def test_malformed_cargo_toml_returns_structured_error(self) -> None:
        # The script must distinguish malformed Cargo.toml from
        # "marker absent": leaking the tomllib traceback is the
        # exact UX regression the script's policy bans.
        mozjs_manifest = (
            "[build-dependencies\n"  # unclosed bracket
            'tree-sitter-javascript = "0.25.0"\n'
        )
        script = _make_fixture(
            self.tmpdir,
            mozjs_manifest=mozjs_manifest,
            baseline=_BASELINE_MATCHING,
        )
        result = _run(script)
        self.assertEqual(result.returncode, 2)
        self.assertIn("not valid TOML", result.stderr)
        self.assertNotIn("Traceback", result.stderr)

    def test_directory_at_manifest_path_returns_structured_error(self) -> None:
        # IsADirectoryError must take the curated error path
        # (CargoTomlParseError → exit 2) and not leak a Python
        # traceback.
        script = _make_fixture(self.tmpdir, baseline=_BASELINE_MATCHING)
        manifest = self.tmpdir / "tree-sitter-mozjs" / "Cargo.toml"
        manifest.unlink()
        manifest.mkdir()
        result = _run(script)
        self.assertEqual(result.returncode, 2)
        self.assertIn("cannot read", result.stderr.lower())
        self.assertNotIn("Traceback", result.stderr)

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

    def test_malformed_baseline_toml_returns_2_without_traceback(self) -> None:
        script = _make_fixture(
            self.tmpdir,
            baseline="[mozjs\nthis is not toml",
        )
        result = _run(script)
        self.assertEqual(result.returncode, 2)
        self.assertIn("TOML", result.stderr)
        self.assertNotIn("Traceback", result.stderr)

    # --- baseline-side type handling ---

    def test_non_string_float_version_value_warns_and_shows_drift(self) -> None:
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
        self.assertIn("warning", result.stderr.lower())
        self.assertIn("0.25", result.stderr)
        # No misleading "baseline None" message.
        self.assertNotIn("baseline None", result.stderr)

    def test_toml_date_version_value_warns_and_shows_drift(self) -> None:
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            "version = 2026-05-28\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script)
        self.assertEqual(result.returncode, 1)
        self.assertIn("warning", result.stderr.lower())
        self.assertIn("2026-05-28", result.stderr)

    def test_toml_array_marker_value_warns_and_treated_as_missing(self) -> None:
        baseline = (
            "[mozjs]\n"
            'marker = ["tree-sitter-javascript"]\n'
            'version = "0.25.0"\n'
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script)
        self.assertEqual(result.returncode, 1)
        self.assertIn("warning", result.stderr.lower())
        self.assertIn("expected a quoted string", result.stderr)

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
        self.assertEqual(updated.count("[mozjs]"), 1)
        # Inserted line must be literally present (catches the case
        # where --update is a no-op but the post-gate check happens
        # to also be skipped by an unrelated regression).
        self.assertIn('version = "0.25.0"', updated)
        # The pending-verification comment must be preserved.
        self.assertIn("# version pending verification", updated)
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
        self.assertEqual(updated.count("[mozjs]"), 1)
        self.assertIn("the JS one", updated)
        self.assertIn('version = "0.25.0"', updated)

    def test_update_with_indented_section_header(self) -> None:
        # TOML accepts leading whitespace on section headers;
        # `_update_section` must locate them (regression #7).
        baseline = (
            "   [mozjs]\n"
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
        self.assertEqual(updated.count("[mozjs]"), 1)
        self.assertIn('version = "0.25.0"', updated)

    def test_update_with_literal_string_values(self) -> None:
        # TOML literal-string form is valid; `--update` must
        # rewrite both quote styles, not append duplicate keys.
        baseline = (
            "[mozjs]\n"
            "marker = 'tree-sitter-javascript'\n"
            "version = '0.24.0'\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        self.assertIn('version = "0.25.0"', updated)
        # Only one marker line per section — no duplicate-key TOML.
        post = _run(script)
        self.assertEqual(post.returncode, 0, post.stderr)

    def test_update_with_dotted_child_section(self) -> None:
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

    def test_update_with_array_of_tables_subsection(self) -> None:
        # `[[mozjs.audit]]` is a maintainer-maintained audit
        # trail; --update must NOT clobber the AoT's version field.
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            'version = "0.24.0"\n'
            "\n"
            "[[mozjs.audit]]\n"
            'note = "first regen"\n'
            'version = "0.24.0"\n'
            "\n"
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        # mozjs.version is updated; the AoT entry is NOT.
        self.assertIn("[[mozjs.audit]]", updated)
        self.assertIn('note = "first regen"', updated)
        # The audit subsection's `version = "0.24.0"` must survive.
        # Count: one updated 0.25.0 in [mozjs], one preserved 0.24.0
        # in the AoT entry.
        self.assertEqual(updated.count('version = "0.25.0"'), 1)
        self.assertEqual(updated.count('version = "0.24.0"'), 1)

    def test_update_with_last_body_line_lacking_newline(self) -> None:
        # Hand-edited or partially-merged baseline with a final
        # body line that lacks `\n` must not result in the
        # inserted field concatenating onto the prior text.
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            "# version pending"  # NO trailing newline
        )
        script = _make_fixture(
            self.tmpdir,
            mozcpp_version=None,  # only mozjs in scope
            baseline=baseline,
        )
        # mozcpp will report marker-not-found; that path exits 2
        # before --update is even attempted. Restage with mozcpp
        # in scope:
        (self.tmpdir / "tree-sitter-mozcpp" / "Cargo.toml").write_text(
            _MOZCPP_BARE.format(version="0.23.4")
        )
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 0, result.stderr)
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        # The new version line must be on its own line, NOT
        # concatenated to `# version pending`.
        self.assertNotIn("pendingversion", updated)
        self.assertIn('version = "0.25.0"', updated)
        post = _run(script)
        self.assertEqual(post.returncode, 0, post.stderr)

    def test_update_refuses_baseline_with_duplicate_version_key(self) -> None:
        # Two `version = "..."` lines under one [mozjs] section is
        # invalid TOML (duplicate key). The validate-before-update
        # guard must refuse with a structured message rather than
        # let `_update_section` rewrite the first line and ship
        # corruption.
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            'version = "0.24.0"\n'
            '# Historical: version = "0.23.1"\n'
            'version = "0.23.1"\n'
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 2)
        self.assertIn("not valid TOML", result.stderr)
        self.assertIn("Fix the baseline manually", result.stderr)

    def test_update_refuses_duplicate_section_baseline(self) -> None:
        # Merge-conflict artifact: two [mozjs] sections. The
        # validate-before-update guard must refuse with a clear
        # message rather than silently rewriting the first.
        baseline = (
            "[mozjs]\n"
            'marker = "tree-sitter-javascript"\n'
            'version = "0.24.0"\n'
            "[mozjs]\n"  # duplicate
            'marker = "tree-sitter-javascript"\n'
            'version = "0.25.0"\n'
            "[mozcpp]\n"
            'marker = "tree-sitter-cpp"\n'
            'version = "0.23.4"\n'
        )
        script = _make_fixture(self.tmpdir, baseline=baseline)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 2)
        self.assertIn("not valid TOML", result.stderr)

    def test_update_appends_missing_section_with_template(self) -> None:
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

    def test_update_refuses_missing_baseline(self) -> None:
        # The from-scratch regen path was removed; restore from
        # git is the documented recovery.
        script = _make_fixture(self.tmpdir, baseline=None)
        result = _run(script, "--update")
        self.assertEqual(result.returncode, 2)
        self.assertIn("missing", result.stderr.lower())
        self.assertIn("git checkout", result.stderr)

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
        updated = (self.tmpdir / ".grammar-marker-baseline.toml").read_text()
        self.assertIn("[mozold]", updated)


if __name__ == "__main__":
    unittest.main()
