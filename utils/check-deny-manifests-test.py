#!/usr/bin/env python3
"""Tests for check-deny-manifests.sh (#1823).

Each test stages a scratch git repository holding a copy of the script,
a `deny.toml`, and one fixture `Cargo.toml` / `Cargo.lock` pair per
crate directory the live tree tracks a lockfile for. A stub
`cargo-deny` first on PATH answers `--version` and records every other
invocation's argv, so nothing reaches the network or a real cargo-deny.
The script is run from an unrelated directory, so every test also pins
that its paths resolve from its own location rather than the cwd.

This is the only verification `make pre-commit` runs: the real gate
needs the network and runs in the `deny` jobs of `ci.yml` and
`deny-advisories.yml`.

Run with:
    python3 -m unittest -q utils/check-deny-manifests-test.py
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-deny-manifests.sh"

# The directories whose `Cargo.lock` the live tree tracks; "" is the root.
# `test_live_tree_tracks_exactly_these_lockfiles` holds this list to the
# tree, so the fixture cannot drift from what the gate really audits.
CRATE_DIRS = (
    "",
    "enums",
    "fuzz",
    "tree-sitter-ccomment",
    "tree-sitter-mozcpp",
    "tree-sitter-mozjs",
    "tree-sitter-preproc",
    "tree-sitter-tcl",
)
ALL_CHECKS = ("advisories", "bans", "licenses", "sources")

# Exits 4 for the manifest `STUB_DENY_FAIL` names, the bit cargo-deny
# sets for a failed licences check; any non-zero status is a failure to
# the script, so the value only has to differ from 0.
STUB_SOURCE = """\
#!{python}
import json, os, sys
args = sys.argv[1:]
if args == ["--version"]:
    print("cargo-deny " + os.environ.get("STUB_DENY_VERSION", "0.20.2"))
    sys.exit(0)
with open(os.environ["STUB_DENY_LOG"], "a", encoding="utf-8") as log:
    log.write(json.dumps(args) + "\\n")
manifest = args[args.index("--manifest-path") + 1]
sys.exit(4 if manifest == os.environ.get("STUB_DENY_FAIL") else 0)
"""


def _git_env() -> dict[str, str]:
    """The caller's environment with its git configuration shut out."""
    env = os.environ.copy()
    env["GIT_CONFIG_GLOBAL"] = os.devnull
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    return env


def _manifest_rel(crate_dir: str) -> str:
    return f"{crate_dir}/Cargo.toml" if crate_dir else "Cargo.toml"


class DenyManifestsTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = pathlib.Path(tempfile.mkdtemp(prefix="bca-deny-manifests-"))
        self.repo = self.tmp / "repo"
        self.elsewhere = self.tmp / "elsewhere"
        self.elsewhere.mkdir()
        self.log = self.tmp / "invocations.jsonl"

        stub_dir = self.tmp / "stub-bin"
        stub_dir.mkdir()
        stub = stub_dir / "cargo-deny"
        stub.write_text(STUB_SOURCE.format(python=sys.executable), encoding="utf-8")
        stub.chmod(0o755)
        self.env = _git_env()
        self.env["PATH"] = f"{stub_dir}{os.pathsep}{self.env['PATH']}"
        self.env["STUB_DENY_LOG"] = str(self.log)

        # The script derives its root as the parent of its own directory,
        # so the copy sits in `repo/utils/` for `repo` to stand in for it.
        (self.repo / "utils").mkdir(parents=True)
        self.script = self.repo / "utils" / SCRIPT_SRC.name
        shutil.copy(SCRIPT_SRC, self.script)
        (self.repo / "deny.toml").write_text("[graph]\n", encoding="utf-8")
        for crate_dir in CRATE_DIRS:
            self._add_crate(crate_dir)
        self._git("init", "-q")
        self._git("add", "--", ".")

    def tearDown(self) -> None:
        shutil.rmtree(self.tmp, ignore_errors=True)

    # --- helpers ---

    def _add_crate(self, crate_dir: str) -> None:
        directory = self.repo / crate_dir
        directory.mkdir(parents=True, exist_ok=True)
        name = crate_dir or "root"
        (directory / "Cargo.toml").write_text(
            f'[package]\nname = "{name}"\n', encoding="utf-8"
        )
        (directory / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")

    def _git(self, *args: str) -> None:
        subprocess.run(
            ["git", "-C", str(self.repo), *args],
            env=self.env,
            check=True,
            capture_output=True,
        )

    def _run(
        self, *checks: str, script: pathlib.Path | None = None, **env: str
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(script or self.script), *checks],
            cwd=self.elsewhere,
            env={**self.env, **env},
            capture_output=True,
            text=True,
            check=False,
        )

    def _invocations(self) -> list[list[str]]:
        if not self.log.exists():
            return []
        lines = self.log.read_text(encoding="utf-8").splitlines()
        return [json.loads(line) for line in lines]

    @staticmethod
    def _manifest_of(argv: list[str]) -> str:
        return argv[argv.index("--manifest-path") + 1]

    def _expected_manifests(self, root: pathlib.Path) -> list[str]:
        return sorted(str(root / _manifest_rel(d)) for d in CRATE_DIRS)

    # --- coverage: every manifest, every check ---

    def test_runs_every_check_against_every_manifest(self) -> None:
        result = self._run(*ALL_CHECKS)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("OK (8 manifests", result.stdout)
        calls = self._invocations()
        self.assertEqual(
            sorted(self._manifest_of(argv) for argv in calls),
            self._expected_manifests(self.repo),
        )
        deny_toml = str(self.repo / "deny.toml")
        for argv in calls:
            with self.subTest(manifest=self._manifest_of(argv)):
                self.assertEqual(argv[-5:], ["check", *ALL_CHECKS])
                # One shared config, by absolute path, ahead of `check`:
                # cargo-deny 0.20 rejects `check --config`.
                config_at = argv.index("--config")
                self.assertEqual(argv[config_at + 1], deny_toml)
                self.assertLess(config_at, argv.index("check"))
                self.assertIn("--locked", argv[: argv.index("check")])

    def test_checks_are_forwarded_not_hard_coded(self) -> None:
        # The weekly job asks for advisories alone. A script that ignored
        # its arguments and always ran all four would pass the test above.
        result = self._run("advisories")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self._invocations()
        self.assertEqual(len(calls), len(CRATE_DIRS))
        for argv in calls:
            self.assertEqual(argv[-2:], ["check", "advisories"])

    # --- the lockfile count ---

    def test_ninth_lockfile_fails_before_any_check_runs(self) -> None:
        self._add_crate("tree-sitter-new")
        self._git("add", "--", "tree-sitter-new")
        result = self._run(*ALL_CHECKS)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("expected 8 tracked Cargo.lock files, found 9", result.stderr)
        self.assertIn("tree-sitter-new/Cargo.lock", result.stderr)
        self.assertIn("EXPECTED_LOCKFILES", result.stderr)
        self.assertEqual(self._invocations(), [])

    def test_dropped_lockfile_fails(self) -> None:
        self._git("rm", "-q", "--cached", "--", "tree-sitter-tcl/Cargo.lock")
        result = self._run(*ALL_CHECKS)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("expected 8 tracked Cargo.lock files, found 7", result.stderr)
        self.assertEqual(self._invocations(), [])

    def test_untracked_lockfile_is_not_counted(self) -> None:
        # A build tree or a corpus checkout carries lockfiles nobody ships;
        # the count is of what git tracks, not of what is on disk.
        stray = self.repo / "target" / "package" / "Cargo.lock"
        stray.parent.mkdir(parents=True)
        stray.write_text("version = 4\n", encoding="utf-8")
        result = self._run("advisories")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self._invocations()), len(CRATE_DIRS))

    def test_lookalike_file_name_is_not_counted(self) -> None:
        # `*Cargo.lock` as a pathspec would match this; the gate must not.
        lookalike = self.repo / "docs" / "NotCargo.lock"
        lookalike.parent.mkdir()
        lookalike.write_text("", encoding="utf-8")
        self._git("add", "--", "docs/NotCargo.lock")
        result = self._run("advisories")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self._invocations()), len(CRATE_DIRS))

    # --- failure propagation ---

    def test_one_failing_manifest_fails_the_gate_and_the_rest_still_run(self) -> None:
        failing = str(self.repo / "fuzz" / "Cargo.toml")
        result = self._run(*ALL_CHECKS, STUB_DENY_FAIL=failing)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("failed for 1 of 8 manifests", result.stderr)
        self.assertIn("  fuzz/Cargo.toml", result.stderr)
        self.assertNotIn("check-deny-manifests: OK", result.stdout)
        # Every graph is still audited, so one red run reports them all.
        self.assertEqual(len(self._invocations()), len(CRATE_DIRS))

    # --- preconditions ---

    def test_no_checks_is_a_usage_error(self) -> None:
        result = self._run()
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("name the checks to run", result.stderr)
        self.assertEqual(self._invocations(), [])

    def test_cargo_deny_older_than_0_20_is_refused(self) -> None:
        result = self._run(*ALL_CHECKS, STUB_DENY_VERSION="0.19.4")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("cargo-deny 0.19.4 is older than 0.20", result.stderr)
        self.assertEqual(self._invocations(), [])

    def test_cargo_deny_with_a_newer_major_is_accepted(self) -> None:
        # Pins the comparison itself: a check on the minor alone would
        # refuse 1.0.
        result = self._run("advisories", STUB_DENY_VERSION="1.0.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self._invocations()), len(CRATE_DIRS))

    # --- the live tree ---

    def test_live_tree_tracks_exactly_these_lockfiles(self) -> None:
        # The live script against the live tree, stub still first on PATH.
        # A ninth excluded crate fails here, under `make pre-commit`, and
        # not first in CI's `deny` job.
        result = self._run("advisories", script=SCRIPT_SRC)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            sorted(self._manifest_of(argv) for argv in self._invocations()),
            self._expected_manifests(REPO_ROOT),
        )


class WorkflowWiringTest(unittest.TestCase):
    """Both `deny` jobs go through the script, on one cargo-deny."""

    WORKFLOWS = ("ci.yml", "deny-advisories.yml")
    PIN = re.compile(r"cargo-deny@(\d+\.\d+\.\d+)")

    def test_both_workflows_call_the_script_on_one_pinned_cargo_deny(self) -> None:
        # A workflow left on `cargo-deny-action`, or one that stops calling
        # the script, audits the root lockfile alone again — the gap #1823
        # closed — and nothing else would notice.
        pins: dict[str, set[str]] = {}
        for name in self.WORKFLOWS:
            text = (REPO_ROOT / ".github" / "workflows" / name).read_text(
                encoding="utf-8"
            )
            with self.subTest(workflow=name):
                self.assertIn("./utils/check-deny-manifests.sh ", text)
                self.assertNotIn("cargo-deny-action@", text)
                pins[name] = set(self.PIN.findall(text))
                self.assertEqual(len(pins[name]), 1, pins[name])
        self.assertEqual(pins["ci.yml"], pins["deny-advisories.yml"])
        # The script's install hint names the same release.
        hinted = set(self.PIN.findall(SCRIPT_SRC.read_text(encoding="utf-8")))
        self.assertEqual(hinted, pins["ci.yml"])


if __name__ == "__main__":
    unittest.main()
