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

    # --- script failure-propagation path (exit 2) ---

    def _run_with_cargo_stub(
        self, stub_body: str
    ) -> subprocess.CompletedProcess[str]:
        """Run the drift script with a fake `cargo` shadowing PATH.

        Mirrors `test_independent_of_fd`'s stub-on-PATH technique:
        a `cargo` stub is prepended to PATH so the script's
        `cargo build` / `cargo run` invocations hit it instead of
        the real toolchain, letting us drive the two `exit 2`
        guards deterministically. The stub still needs the real
        `rustfmt`/`diff`/etc. on PATH, so we prepend rather than
        replace.
        """
        stub_dir = self.tmpdir / "cargo-stub"
        stub_dir.mkdir()
        stub = stub_dir / "cargo"
        stub.write_text(stub_body, encoding="utf-8")
        stub.chmod(0o755)
        env = os.environ.copy()
        env["PATH"] = f"{stub_dir}{os.pathsep}{env['PATH']}"
        return subprocess.run(
            ["bash", str(self.tmpdir / SCRIPT_SRC.name)],
            capture_output=True,
            text=True,
            check=False,
            env=env,
        )

    def test_script_exits_2_when_cargo_build_fails(self) -> None:
        # Drive the FIRST `exit 2` guard (the `if ! cargo build`
        # branch) through the script itself. A `cargo` stub that
        # fails on `build` must make the script exit 2 and emit its
        # own diagnostic — proving it propagates the build failure
        # rather than swallowing it or mislabelling it as drift.
        result = self._run_with_cargo_stub(
            '#!/bin/sh\nif [ "$1" = "build" ]; then exit 2; fi\nexit 0\n'
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("enums crate failed to build", result.stderr)

    def test_script_exits_2_when_codegen_fails(self) -> None:
        # Drive the SECOND `exit 2` guard (the `if ! cargo run`
        # codegen branch): `build` succeeds, `run` fails. The script
        # must exit 2 and name the failing codegen mode, rather than
        # proceeding to diff an empty output dir and reporting drift
        # (exit 1) or success (exit 0).
        result = self._run_with_cargo_stub(
            '#!/bin/sh\nif [ "$1" = "run" ]; then exit 2; fi\nexit 0\n'
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("enums codegen", result.stderr)
        self.assertIn("failed", result.stderr)

    def test_enums_binary_exits_2_on_io_error(self) -> None:
        # Contract check on the codegen binary itself (NOT the
        # script): an io::Error during output must surface as the
        # binary's `ExitCode::from(2)`. The script's two exit-2
        # guards (covered above) rely on this non-zero propagation,
        # so pin it here too. Renamed from the former
        # `test_invalid_output_path_exits_2`, whose name implied it
        # covered the script's failure path (it does not — it
        # invokes the binary directly).
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

    def test_independent_of_fd(self) -> None:
        # The script must not depend on `fd`/`fdfind`: it globs the
        # flat codegen output dirs and rustfmt's them directly.
        # Prepend a dir whose `fd`/`fdfind` stubs exit non-zero
        # (rather than pruning PATH, which would also hide cargo and
        # rustfmt). A clean tree must still pass (exit 0): if the
        # script ever calls fd again, the broken stub trips `set -e`
        # and this test fails. Guards the regression where the gate
        # hard-required fd and broke CI's minimal lint image.
        stub_dir = self.tmpdir / "fd-blocker"
        stub_dir.mkdir()
        for name in ("fd", "fdfind"):
            stub = stub_dir / name
            stub.write_text("#!/bin/sh\nexit 127\n", encoding="utf-8")
            stub.chmod(0o755)
        env = os.environ.copy()
        env["PATH"] = f"{stub_dir}{os.pathsep}{env['PATH']}"
        result = subprocess.run(
            ["bash", str(self.tmpdir / SCRIPT_SRC.name)],
            capture_output=True,
            text=True,
            check=False,
            env=env,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

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


class MacroGeneratorPruneTest(unittest.TestCase):
    """#892: the `enums/data/mac.py` generator must prune stale entries.

    The generator's output is a pure function of its `macs` template:
    re-running it rewrites `c_macros.txt` to exactly `sorted(macros)`,
    so an entry no longer produced by the template is removed on the
    next run. The previous append-only contract could only grow the
    file, letting it drift away from its own generator.

    Each test copies the generator script into an isolated tempdir so
    the run never touches the live, git-tracked `enums/data/*.txt`.
    """

    GENERATOR = REPO_ROOT / "enums" / "data" / "mac.py"
    DATA_FILE_NAME = "c_macros.txt"

    def setUp(self) -> None:
        self.tmpdir = pathlib.Path(tempfile.mkdtemp(prefix="bca-macro-prune-"))
        # The generator resolves its data file as a sibling of the
        # script, so copying the script alone makes the run fully
        # hermetic — it reads/writes only the tempdir copy.
        self.script = self.tmpdir / self.GENERATOR.name
        shutil.copy(self.GENERATOR, self.script)
        self.data_file = self.tmpdir / self.DATA_FILE_NAME

    def tearDown(self) -> None:
        shutil.rmtree(self.tmpdir, ignore_errors=True)

    def _run_generator(self) -> None:
        result = subprocess.run(
            [sys.executable, str(self.script)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def _emitted_names(self) -> list[str]:
        return [
            line.strip()
            for line in self.data_file.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]

    def test_stale_entry_is_pruned(self) -> None:
        # Seed the data file with a bogus name the template never
        # produces; the generator must drop it on the next run.
        bogus = "BOGUS_MACRO_THAT_THE_TEMPLATE_NEVER_EMITS"
        self.data_file.write_text(f"{bogus}\n", encoding="utf-8")
        self._run_generator()
        self.assertNotIn(bogus, self._emitted_names())
        # A genuine template entry must still be present, proving the
        # prune did not simply empty the file.
        self.assertIn("INT16_C", self._emitted_names())

    def test_output_is_independent_of_prior_contents(self) -> None:
        # The same data file is produced whether the prior file was
        # empty or polluted — the output is a pure function of the
        # template, not a union with whatever was on disk.
        self.data_file.write_text("", encoding="utf-8")
        self._run_generator()
        from_empty = self._emitted_names()

        self.data_file.write_text(
            "EXTRA_ONE\nEXTRA_TWO\n", encoding="utf-8"
        )
        self._run_generator()
        from_polluted = self._emitted_names()

        self.assertEqual(from_empty, from_polluted)
        self.assertNotIn("EXTRA_ONE", from_polluted)
        # The file is written sorted, the contract the downstream
        # `binary_search` lookup relies on.
        self.assertEqual(from_polluted, sorted(from_polluted))


if __name__ == "__main__":
    unittest.main()
