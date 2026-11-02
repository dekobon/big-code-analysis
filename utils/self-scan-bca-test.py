#!/usr/bin/env python3
"""Self-tests for `utils/self-scan-bca.py` and `utils/self_scan_digest.py` (#1773).

Run with `python3 -B -m unittest -q utils/self-scan-bca-test.py`.

Two subjects. The digest module is held to the Rust implementation by
one shared fixture and one pinned hex, repeated byte-for-byte from
`big-code-analysis-cli/src/build_info/build_info_tests.rs`; change the
walker on either side and exactly that pair fails. The resolver is
exercised two ways: in dry-run for the *selection* (which command, and
why), and launched for real — with a fake `bca` on a controlled PATH that
records its argv and pid — for the claims dry-run cannot make: that the
user's arguments reach the binary, that its exit status is the wrapper's,
and that on POSIX the wrapper `exec`s rather than forks.

Every selection case was verified by perturbation while being written
(the driver is recorded on #1773): a resolver that skips the digest
comparison, or trusts any digest, fails `..._falls_back_on_a_mismatch`;
one that reads the variables in the other order fails
`..._from_source_wins_over_an_explicit_binary`; one that forks instead
of `exec`ing fails the pid assertion in `..._launched_binary_...`; and
one that drops the user's arguments fails eight of eleven.
"""

from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

UTILS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(UTILS_DIR))

import self_scan_digest as digest  # noqa: E402

WRAPPER = UTILS_DIR / "self-scan-bca.py"

# Byte-for-byte the fixture `build_info_tests.rs::shared_fixture` writes,
# and the hex it pins as `SHARED_FIXTURE_DIGEST`.
SHARED_FIXTURE = {
    "Cargo.toml": b"[workspace]\n",
    "Cargo.lock": b"version = 4\n",
    "src/lib.rs": b"fn a() {}\n",
    "src/nested/b.rs": b"fn b() {\0}\n",
    "src/empty.rs": b"",
    "src/snapshots/x.snap": b"ignored\n",
    "src/build/generated.rs": b"// nested build dir is hashed\n",
    "big-code-analysis-ast/Cargo.toml": b"[package]\n",
    "big-code-analysis-cli/Cargo.toml": b"[package]\n",
    "big-code-analysis-cli/build.rs": b"fn main() {}\n",
    "big-code-analysis-cli/tests/smoke.rs": b"// integration test, not compiled in\n",
    "big-code-analysis-cli/pyproject.toml": b"[tool]\n",
    "src/tests/mod.rs": b"// a module named tests under src/ is source\n",
    "src/.rs": b"// a pure dotfile has no extension\n",
    "tree-sitter-ccomment/src/parser.c": b"int p;\n",
    "tree-sitter-ccomment/target/junk.rs": b"skipped\n",
    "tree-sitter-mozcpp/src/tree_sitter/parser.h": b"#define X\n",
    "tree-sitter-mozjs/src/scanner.c": b"int s;\n",
    "tree-sitter-preproc/src/node-types.json": b"[]\n",
    "tree-sitter-tcl/src/grammar.json": b"{}\n",
    "tree-sitter-tcl/examples/x.c": b"int e;\n",
}
SHARED_FIXTURE_DIGEST = "2d6dfcc20156bfc6bcd2bc38ab2cdff68cc864b1a4b6951b891cffc40508a17e"
SHARED_FIXTURE_FILES = [
    "Cargo.lock",
    "Cargo.toml",
    "big-code-analysis-ast/Cargo.toml",
    "big-code-analysis-cli/Cargo.toml",
    "big-code-analysis-cli/build.rs",
    "big-code-analysis-cli/pyproject.toml",
    "src/build/generated.rs",
    "src/empty.rs",
    "src/lib.rs",
    "src/nested/b.rs",
    "src/tests/mod.rs",
    "tree-sitter-ccomment/src/parser.c",
    "tree-sitter-mozcpp/src/tree_sitter/parser.h",
    "tree-sitter-mozjs/src/scanner.c",
]


def write_fixture(root: pathlib.Path, files: dict[str, bytes] = SHARED_FIXTURE) -> None:
    for rel, content in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)


class DigestTests(unittest.TestCase):
    def test_shared_fixture_hashes_to_the_pinned_digest(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            write_fixture(root)
            self.assertEqual(digest.input_files(root), SHARED_FIXTURE_FILES)
            self.assertEqual(digest.digest(root), SHARED_FIXTURE_DIGEST)

    def test_digest_moves_on_content_add_and_rename_but_not_on_snapshots_or_artifacts(self) -> None:
        def with_extra(rel: str, content: bytes) -> str:
            with tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                write_fixture(root)
                write_fixture(root, {rel: content})
                return digest.digest(root)

        self.assertNotEqual(with_extra("src/lib.rs", b"fn a() { }\n"), SHARED_FIXTURE_DIGEST)
        self.assertNotEqual(with_extra("src/added.rs", b""), SHARED_FIXTURE_DIGEST)
        self.assertNotEqual(with_extra("src/lib2.rs", b"fn a() {}\n"), SHARED_FIXTURE_DIGEST)
        self.assertEqual(with_extra("src/snapshots/y.snap", b"more"), SHARED_FIXTURE_DIGEST)
        self.assertEqual(with_extra("tree-sitter-tcl/target/out.rs", b"x"), SHARED_FIXTURE_DIGEST)
        self.assertEqual(
            with_extra("big-code-analysis-cli/tests/o.rs", b"x"), SHARED_FIXTURE_DIGEST
        )
        self.assertEqual(
            with_extra("tree-sitter-tcl/src/node-types.json", b"1"), SHARED_FIXTURE_DIGEST
        )
        self.assertNotEqual(with_extra("src/tests/other.rs", b"x"), SHARED_FIXTURE_DIGEST)

    def test_a_missing_root_or_a_symlink_refuses_the_tree(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            write_fixture(root)
            (root / "src/link.rs").symlink_to("lib.rs")
            with self.assertRaises(digest.DigestError):
                digest.digest(root)
            (root / "src/link.rs").unlink()
            shutil.rmtree(root / "tree-sitter-tcl")
            with self.assertRaises(digest.DigestError):
                digest.digest(root)


class ResolverHarness(unittest.TestCase):
    """A scratch workspace, a controlled PATH, and a fake `bca`."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.workspace = self.root / "ws"
        self.workspace.mkdir()
        write_fixture(self.workspace)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.record = self.root / "record.txt"

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def fake_bca(self, inputs: str | None, exit_code: int = 0, name: str = "bca") -> pathlib.Path:
        """A `bca` whose `--version` reports `inputs` and which records its argv and pid."""
        version = (
            "bca 9.9.9\n" + ("" if inputs is None else f"build inputs: {inputs}\n") + "git: x\n"
        )
        script = (
            "#!/bin/sh\n"
            'if [ "$1" = "--version" ]; then\n'
            f"  printf '%s' '{version}'\n"
            "  exit 0\n"
            "fi\n"
            f'printf "%s\\n" "$$" "$@" > "{self.record}"\n'
            f"exit {exit_code}\n"
        )
        path = self.bin / name
        path.write_text(script)
        path.chmod(0o755)
        return path

    def run_wrapper(
        self, *args: str, env: dict[str, str] | None = None, dry_run: bool = True
    ) -> subprocess.CompletedProcess[str]:
        full_env = {
            "PATH": str(self.bin),
            "HOME": self.tmp.name,
            "BCA_SELF_SCAN_WORKSPACE": str(self.workspace),
        }
        if dry_run:
            full_env["BCA_SELF_SCAN_DRY_RUN"] = "1"
        full_env.update(env or {})
        return subprocess.run(
            [sys.executable, "-B", str(WRAPPER), *args],
            capture_output=True,
            text=True,
            env=full_env,
            check=False,
            cwd=self.root,
        )

    def chosen(self, result: subprocess.CompletedProcess[str]) -> list[str]:
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.splitlines()


@unittest.skipIf(sys.platform == "win32", "the fake bca is a POSIX shell script")
class ResolverTests(ResolverHarness):
    CARGO_DEV = ["cargo", "run", "--quiet", "-p", "big-code-analysis-cli", "--"]
    CARGO_RELEASE = ["cargo", "run", "--quiet", "--release", "-p", "big-code-analysis-cli", "--"]

    def test_no_bca_on_path_builds_from_source_under_the_dev_profile(self) -> None:
        result = self.run_wrapper("check", "--tier=soft=0.95")
        self.assertEqual(self.chosen(result), [*self.CARGO_DEV, "check", "--tier=soft=0.95"])
        self.assertIn("no bca on PATH", result.stderr)

    def test_release_profile_adds_the_flag_and_a_typo_is_an_error(self) -> None:
        result = self.run_wrapper("check", env={"BCA_SELF_SCAN_PROFILE": "release"})
        self.assertEqual(self.chosen(result), [*self.CARGO_RELEASE, "check"])
        bad = self.run_wrapper("check", env={"BCA_SELF_SCAN_PROFILE": "relese"})
        self.assertNotEqual(bad.returncode, 0)
        self.assertIn("BCA_SELF_SCAN_PROFILE", bad.stderr)
        # An empty value reads as unset, not as a bad profile.
        empty = self.run_wrapper("check", env={"BCA_SELF_SCAN_PROFILE": ""})
        self.assertEqual(self.chosen(empty), [*self.CARGO_DEV, "check"])

    def test_path_bca_is_used_when_its_digest_matches(self) -> None:
        bca = self.fake_bca(SHARED_FIXTURE_DIGEST)
        result = self.run_wrapper("check")
        self.assertEqual(self.chosen(result), [str(bca), "check"])
        self.assertIn("build inputs match this tree", result.stderr)

    def test_path_bca_falls_back_on_a_mismatch_a_missing_line_or_a_registry_build(self) -> None:
        self.fake_bca("0" * 64)
        result = self.run_wrapper("check")
        self.assertEqual(self.chosen(result), [*self.CARGO_DEV, "check"])
        self.assertIn("built from other inputs", result.stderr)

        self.fake_bca(None)
        result = self.run_wrapper("check")
        self.assertEqual(self.chosen(result), [*self.CARGO_DEV, "check"])
        self.assertIn("reports no build-inputs digest", result.stderr)

        self.fake_bca("unavailable")
        result = self.run_wrapper("check")
        self.assertEqual(self.chosen(result), [*self.CARGO_DEV, "check"])

        # A matching binary stops matching the moment an input moves.
        self.fake_bca(SHARED_FIXTURE_DIGEST)
        (self.workspace / "src/lib.rs").write_bytes(b"fn a() { }\n")
        result = self.run_wrapper("check")
        self.assertEqual(self.chosen(result), [*self.CARGO_DEV, "check"])

    def test_explicit_binary_is_trusted_and_a_relative_path_is_anchored_to_the_caller(self) -> None:
        other = self.fake_bca("0" * 64, name="other-bca")
        result = self.run_wrapper("check", env={"BCA_SELF_SCAN_BINARY": str(other)})
        self.assertEqual(self.chosen(result), [str(other), "check"])
        relative = self.run_wrapper("check", env={"BCA_SELF_SCAN_BINARY": "bin/other-bca"})
        self.assertEqual(self.chosen(relative), [str(other.resolve()), "check"])
        # `./x` has a separator but `Path("./x").parent == Path()`; the
        # anchoring keys on the separator, so this resolves too. The
        # fake sits in the invoking directory for exactly this case.
        here = self.fake_bca("0" * 64, name="here-bca")
        here = here.rename(self.root / "here-bca")
        dot = self.run_wrapper("check", env={"BCA_SELF_SCAN_BINARY": "./here-bca"})
        self.assertEqual(self.chosen(dot), [str(here.resolve()), "check"])
        bare = self.run_wrapper("check", env={"BCA_SELF_SCAN_BINARY": "other-bca"})
        self.assertEqual(self.chosen(bare), ["other-bca", "check"])
        empty = self.run_wrapper("check", env={"BCA_SELF_SCAN_BINARY": ""})
        self.assertEqual(self.chosen(empty), [*self.CARGO_DEV, "check"])

    def test_from_source_wins_over_an_explicit_binary_and_a_matching_path_bca(self) -> None:
        bca = self.fake_bca(SHARED_FIXTURE_DIGEST)
        result = self.run_wrapper(
            "check", env={"BCA_SELF_SCAN_FROM_SOURCE": "1", "BCA_SELF_SCAN_BINARY": str(bca)}
        )
        self.assertEqual(self.chosen(result), [*self.CARGO_DEV, "check"])
        self.assertIn("BCA_SELF_SCAN_FROM_SOURCE", result.stderr)

    def test_a_tree_that_cannot_be_digested_falls_back(self) -> None:
        self.fake_bca(SHARED_FIXTURE_DIGEST)
        (self.workspace / "src/link.rs").symlink_to("lib.rs")
        result = self.run_wrapper("check")
        self.assertEqual(self.chosen(result), [*self.CARGO_DEV, "check"])
        self.assertIn("cannot be digested", result.stderr)

    def test_relative_paths_still_resolve_after_the_wrapper_changes_directory(self) -> None:
        # Both `which` on a relative PATH entry and an explicit `./bca`
        # name the binary relative to the *invoking* directory, and
        # `main` chdirs to the workspace before exec. Launched for real:
        # a dry run cannot see the chdir.
        self.fake_bca(SHARED_FIXTURE_DIGEST, exit_code=7)
        shutil.copy(self.bin / "bca", self.root / "bca")
        for env in (
            {"PATH": "bin"},
            {"PATH": str(self.bin), "BCA_SELF_SCAN_BINARY": "./bca"},
        ):
            result = self.run_wrapper("check", env=env, dry_run=False)
            self.assertEqual(result.returncode, 7, result.stderr)
            self.assertEqual(self.record.read_text().splitlines()[1:], ["check"])

    def test_a_lockfile_only_change_rejects_the_installed_binary(self) -> None:
        # The resolver compares digests instead of letting cargo
        # fingerprint, so a dependency bump with no source edit must
        # still move the digest: ranged dependencies decide which files
        # a walk sees.
        self.fake_bca(SHARED_FIXTURE_DIGEST)
        self.assertEqual(self.chosen(self.run_wrapper("check"))[0], str(self.bin / "bca"))
        (self.workspace / "Cargo.lock").write_bytes(b"version = 4\n[[package]]\n")
        result = self.run_wrapper("check")
        self.assertEqual(self.chosen(result), [*self.CARGO_DEV, "check"])
        self.assertIn("built from other inputs", result.stderr)

    @unittest.skipIf(getattr(os, "geteuid", lambda: -1)() == 0, "root reads unreadable files")
    def test_an_unreadable_input_falls_back_instead_of_crashing(self) -> None:
        self.fake_bca(SHARED_FIXTURE_DIGEST)
        locked = self.workspace / "src/lib.rs"
        locked.chmod(0o000)
        try:
            result = self.run_wrapper("check")
        finally:
            locked.chmod(0o644)
        self.assertEqual(self.chosen(result), [*self.CARGO_DEV, "check"])
        self.assertIn("cannot be digested", result.stderr)

    def test_launched_binary_receives_the_arguments_and_its_status_and_pid_are_the_wrappers(
        self,
    ) -> None:
        self.fake_bca(SHARED_FIXTURE_DIGEST, exit_code=3)
        proc = subprocess.Popen(
            [
                sys.executable,
                "-B",
                str(WRAPPER),
                "check",
                "--tier=soft=0.95",
                "--write-baseline",
                "x y.toml",
            ],
            env={
                "PATH": str(self.bin),
                "HOME": self.tmp.name,
                "BCA_SELF_SCAN_WORKSPACE": str(self.workspace),
            },
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            cwd=self.root,
        )
        _, stderr = proc.communicate(timeout=60)
        self.assertEqual(proc.returncode, 3, stderr)
        recorded = self.record.read_text().splitlines()
        # `exec` keeps the pid: the fake's `$$` is the process we spawned.
        self.assertEqual(int(recorded[0]), proc.pid)
        self.assertEqual(
            recorded[1:], ["check", "--tier=soft=0.95", "--write-baseline", "x y.toml"]
        )


if __name__ == "__main__":
    unittest.main()
