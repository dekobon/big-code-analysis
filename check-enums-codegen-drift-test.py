#!/usr/bin/env python3
"""Tests for check-enums-codegen-drift.sh.

Each test stages a synthetic mini-repo in a tempdir: the
`enums/` crate is symlinked to the live repo (path-dep
resolution depends on a real sibling layout, and the data files
are read verbatim), but `src/c_langs_macros/` and
`src/languages/` are deep-copied so per-test mutations stay
isolated. The drift script is then invoked from the tempdir.

The shared cargo target cache (`enums/target/`) is warmed once
in `setUpClass`, so per-test invocations hit a hot build.

Run with:
    python3 -m unittest -q check-enums-codegen-drift-test.py
"""

from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parent
SCRIPT_SRC = REPO_ROOT / "check-enums-codegen-drift.sh"


def _run(
    tmpdir: pathlib.Path, *args: str
) -> subprocess.CompletedProcess[str]:
    """Run the drift script from `tmpdir` (its $ROOT)."""
    return subprocess.run(
        ["bash", str(tmpdir / SCRIPT_SRC.name), *args],
        capture_output=True,
        text=True,
        check=False,
    )


class DriftGateTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        # Warm the enums binary build so per-test invocations
        # hit a hot cache. Without this every test pays a 5-15s
        # cargo build penalty.
        manifest = REPO_ROOT / "enums" / "Cargo.toml"
        result = subprocess.run(
            ["cargo", "build", "--manifest-path", str(manifest), "--quiet"],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"warm-up cargo build failed (rc={result.returncode}):\n"
                f"{result.stderr}"
            )

    def setUp(self) -> None:
        self.tmpdir = pathlib.Path(tempfile.mkdtemp(prefix="bca-drift-test-"))
        # Symlink the enums crate. Cargo resolves
        # `../tree-sitter-*` relative to the manifest path
        # AS GIVEN, not its symlink target — so the sibling
        # vendored grammar crates must also be symlinked
        # into the tempdir at the matching paths. Data files
        # (`enums/data/*.txt`) are read verbatim through the
        # enums symlink.
        (self.tmpdir / "enums").symlink_to(REPO_ROOT / "enums")
        for ts_crate in (
            "tree-sitter-tcl",
            "tree-sitter-preproc",
            "tree-sitter-ccomment",
            "tree-sitter-mozcpp",
            "tree-sitter-mozjs",
        ):
            (self.tmpdir / ts_crate).symlink_to(REPO_ROOT / ts_crate)
        # Copy the mutable artifact dirs; per-test mutations
        # land here and are torn down with the tempdir.
        (self.tmpdir / "src").mkdir()
        for sub in ("c_langs_macros", "languages"):
            shutil.copytree(
                REPO_ROOT / "src" / sub,
                self.tmpdir / "src" / sub,
            )
        # Copy the script itself so `$BASH_SOURCE` and
        # `dirname "$BASH_SOURCE"` resolve to the tempdir.
        # (`git rev-parse --show-toplevel` fails here — not a
        # git tree — and the script falls back to BASH_SOURCE
        # dirname, which is exactly what we want.)
        shutil.copy(SCRIPT_SRC, self.tmpdir / SCRIPT_SRC.name)

    def tearDown(self) -> None:
        # Symlink under self.tmpdir is removed by rmtree without
        # following it; the live enums/ is untouched.
        shutil.rmtree(self.tmpdir, ignore_errors=True)

    # --- happy path ---

    def test_baseline_passes(self) -> None:
        result = _run(self.tmpdir)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("OK", result.stdout)

    # --- drift detection ---

    def test_mutated_c_macros_fails_with_drift_message(self) -> None:
        # Mutate the checked-in c_macros.rs so codegen output
        # diverges. The script must report drift, the specific
        # filename, AND the remediation block (defended by the
        # pipefail-safe diff pipeline).
        target = self.tmpdir / "src" / "c_langs_macros" / "c_macros.rs"
        text = target.read_text(encoding="utf-8")
        target.write_text(
            text.replace('"INT16_C",', '"FAKE_INT16_C",', 1),
            encoding="utf-8",
        )
        result = _run(self.tmpdir)
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "drift: src/c_langs_macros/c_macros.rs", result.stderr
        )
        # Remediation block must print despite the diff
        # truncation pipeline — this is the regression test
        # for the `diff | head -40` pipefail abort that the
        # hardening commit fixed.
        self.assertIn("Codegen drift detected", result.stderr)
        self.assertIn("Regenerate the checked-in files", result.stderr)

    def test_mutated_language_file_fails_with_drift_message(self) -> None:
        # Same as above but on the src/languages side, to
        # exercise both diff_dir invocations.
        target = self.tmpdir / "src" / "languages" / "language_rust.rs"
        text = target.read_text(encoding="utf-8")
        target.write_text(
            text.replace("pub enum Rust", "pub enum RustFake", 1),
            encoding="utf-8",
        )
        result = _run(self.tmpdir)
        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "drift: src/languages/language_rust.rs", result.stderr
        )
        self.assertIn("Codegen drift detected", result.stderr)

    # --- orphan detection ---

    def test_orphan_language_file_fails_with_stale_message(self) -> None:
        # A `language_zombie.rs` that the codegen doesn't emit
        # must trip the reverse-direction (orphan) check.
        orphan = self.tmpdir / "src" / "languages" / "language_zombie.rs"
        orphan.write_text(
            "// orphan generated file no codegen produces\n",
            encoding="utf-8",
        )
        result = _run(self.tmpdir)
        self.assertEqual(result.returncode, 1)
        self.assertIn("language_zombie.rs", result.stderr)
        self.assertIn(
            "in repo but not produced by codegen (stale)",
            result.stderr,
        )

    def test_orphan_c_langs_macros_file_fails(self) -> None:
        orphan = self.tmpdir / "src" / "c_langs_macros" / "c_extra.rs"
        orphan.write_text("// orphan\n", encoding="utf-8")
        result = _run(self.tmpdir)
        self.assertEqual(result.returncode, 1)
        self.assertIn("c_extra.rs", result.stderr)
        self.assertIn("stale", result.stderr)

    def test_mod_rs_is_skipped_in_orphan_check(self) -> None:
        # mod.rs is hand-maintained; the gate must not flag it
        # as an orphan even though the codegen doesn't produce it.
        # The fixture already contains mod.rs from the copy; the
        # baseline run is the implicit assertion. Make it
        # explicit by re-running and checking no false alarm.
        result = _run(self.tmpdir)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("mod.rs", result.stderr)

    # --- large diff handling ---

    def test_large_diff_prints_truncation_footer(self) -> None:
        # Insert 50 fake entries to force the per-file diff to
        # exceed the 40-line head cap. The footer must report
        # how many lines were hidden so the reviewer knows the
        # output is incomplete.
        target = self.tmpdir / "src" / "c_langs_macros" / "c_macros.rs"
        text = target.read_text(encoding="utf-8")
        fake_block = "\n".join(
            f'    "FAKE_ENTRY_{i:03d}",' for i in range(50)
        )
        target.write_text(
            text.replace('"INT16_C",', f"{fake_block}\n    \"INT16_C\",", 1),
            encoding="utf-8",
        )
        result = _run(self.tmpdir)
        self.assertEqual(result.returncode, 1)
        self.assertIn("more diff lines hidden", result.stderr)
        # And the remediation block STILL prints — confirming
        # the pipefail-safe pipeline doesn't abort the script
        # mid-loop even when `head -40` truncates a large diff.
        self.assertIn("Codegen drift detected", result.stderr)

    # --- codegen failure path ---

    def test_invalid_output_path_exits_2(self) -> None:
        # Provoke a real codegen io::Error by making the work
        # path unwritable. The script's `mkdir -p $WORK_DIR/...`
        # would still succeed (it creates under $TMPDIR), so
        # we exercise the path via the enums binary directly:
        # the test is then a contract check that the codegen
        # binary exits non-zero on io errors. The drift script
        # invokes that binary; if the binary exits non-zero the
        # script propagates with `exit 2`.
        result = subprocess.run(
            [
                "cargo",
                "run",
                "--manifest-path",
                str(REPO_ROOT / "enums" / "Cargo.toml"),
                "--quiet",
                "--",
                "-lc_macros",
                "-o",
                "/nonexistent/path/that/cannot/be/created",
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("enums:", result.stderr)

    def test_neither_fd_nor_fdfind_exits_2(self) -> None:
        # The script requires `fd` or `fdfind`. With PATH
        # pruned to a directory containing neither, the script
        # must exit 2 with an actionable error.
        env = os.environ.copy()
        empty_path = self.tmpdir / "empty-path"
        empty_path.mkdir()
        # Keep just /bin and /usr/bin for `bash`, `cargo`, etc.,
        # but symlink them under a private dir without fd. We
        # achieve this by setting PATH to a stripped value that
        # finds cargo but not fd.
        # Simpler: replace fd/fdfind in PATH by prefixing a dir
        # containing fake "not-installed" stubs. But on systems
        # where neither was ever in PATH, this is moot. We
        # skip the test rather than fake it if fd IS available
        # — the test only meaningfully runs when fd is absent,
        # which is rare in dev environments. Document and skip.
        if shutil.which("fd") or shutil.which("fdfind"):
            self.skipTest("fd or fdfind present; cannot exercise absent-tool path")
        # Otherwise (no fd available), run normally and expect
        # exit 2 with the actionable error.
        env["PATH"] = "/usr/bin:/bin"
        result = subprocess.run(
            ["bash", str(self.tmpdir / SCRIPT_SRC.name)],
            capture_output=True,
            text=True,
            check=False,
            env=env,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("fd", result.stderr.lower())

    # --- clean-state assertions defending the diff_dir loop ---

    def test_script_re_passes_after_drift_revert(self) -> None:
        # Mutate, run (expect failure), revert, re-run (expect OK).
        # Pins that the script doesn't leave state behind that
        # would make subsequent invocations fail.
        target = self.tmpdir / "src" / "c_langs_macros" / "c_macros.rs"
        original = target.read_text(encoding="utf-8")
        target.write_text(
            original.replace('"INT16_C",', '"FAKE",', 1),
            encoding="utf-8",
        )
        first = _run(self.tmpdir)
        self.assertEqual(first.returncode, 1)
        target.write_text(original, encoding="utf-8")
        second = _run(self.tmpdir)
        self.assertEqual(second.returncode, 0, second.stderr)


if __name__ == "__main__":
    unittest.main()
