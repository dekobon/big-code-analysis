#!/usr/bin/env python3
"""Tests for check-deny-manifests.sh (#1823).

Each test stages a scratch git repository holding a copy of the script,
a `deny.toml`, and one fixture `Cargo.toml` / `Cargo.lock` pair per
crate directory the live tree tracks a lockfile for. A stub
`cargo-deny` first on PATH answers `--version` and records every other
invocation's argv, so nothing reaches the network or a real cargo-deny.
The script is run from an unrelated directory, so every test also pins
that its paths resolve from its own location rather than the cwd.

This and `utils/check-tools-test.sh`, which runs `--version-check`, are
the only verification `make pre-commit` runs: the real gate needs the
network and runs in the `deny` jobs of `ci.yml` and
`deny-advisories.yml`, and in `make release-check` (#1826).

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


def _git_env(home: pathlib.Path) -> dict[str, str]:
    """The caller's environment with its git state shut out.

    Every inherited `GIT_*` variable goes, not just the configuration:
    git exports `GIT_DIR` and `GIT_INDEX_FILE` to hooks and to `git rebase
    --exec` children, and both override `git -C`. Inherited, they sent
    this suite's `git init` and `git add` into the real repository — a
    `core.bare = true` in its config and an index holding only the
    fixtures. `HOME` and `XDG_CONFIG_HOME` point into the scratch tree so
    a user-level ignore file cannot hide a fixture lockfile either.
    """
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env["GIT_CONFIG_GLOBAL"] = os.devnull
    env["GIT_CONFIG_NOSYSTEM"] = "1"
    env["HOME"] = str(home)
    env["XDG_CONFIG_HOME"] = str(home / ".config")
    return env


def _manifest_rel(crate_dir: str) -> str:
    return f"{crate_dir}/Cargo.toml" if crate_dir else "Cargo.toml"


class DenyManifestsTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = pathlib.Path(tempfile.mkdtemp(prefix="bca-deny-manifests-"))
        # Registered before anything else in `setUp` can fail, which
        # `tearDown` would not be reached for.
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.repo = self.tmp / "repo"
        self.elsewhere = self.tmp / "elsewhere"
        self.elsewhere.mkdir()
        self.log = self.tmp / "invocations.jsonl"

        stub_dir = self.tmp / "stub-bin"
        stub_dir.mkdir()
        stub = stub_dir / "cargo-deny"
        stub.write_text(STUB_SOURCE.format(python=sys.executable), encoding="utf-8")
        stub.chmod(0o755)
        home = self.tmp / "home"
        home.mkdir()
        self.env = _git_env(home)
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
                # The root manifest is a package too; without this the
                # root graph is the library's alone (#1823 review).
                self.assertIn("--workspace", argv[: argv.index("check")])

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

    def test_a_lower_major_than_the_floor_is_refused(self) -> None:
        # The floor's major is 0 today, so no real version is below it;
        # a copy with the floor raised to 1.20 asks the comparison what
        # it will do the day it is, when a check that compared minors
        # only within the floor's major would accept every 0.x.
        text = self.script.read_text(encoding="utf-8")
        raised = text.replace("MIN_DENY_MAJOR=0\n", "MIN_DENY_MAJOR=1\n")
        self.assertNotEqual(raised, text, "the floor's major moved")
        self.script.write_text(raised, encoding="utf-8")
        result = self._run(*ALL_CHECKS, STUB_DENY_VERSION="0.25.0")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("cargo-deny 0.25.0 is older than 1.20", result.stderr)
        self.assertEqual(self._invocations(), [])

    def test_cargo_deny_with_a_newer_major_is_accepted(self) -> None:
        # Pins the comparison itself: a check on the minor alone would
        # refuse 1.0.
        result = self._run("advisories", STUB_DENY_VERSION="1.0.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self._invocations()), len(CRATE_DIRS))

    def test_cargo_deny_with_a_newer_minor_is_accepted(self) -> None:
        # The floor is a floor: CI's next pin is a 0.x minor above 20.
        # Every other test runs 0.20.2, the floor's own minor, so a
        # comparison that admitted that minor alone (`-ne` for `-lt`)
        # would pass them all and refuse the bump.
        result = self._run("advisories", STUB_DENY_VERSION="0.21.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self._invocations()), len(CRATE_DIRS))

    # --- --version-check (#1826) ---

    def _loose_copy(self) -> pathlib.Path:
        """A copy of the script in a directory no git repository holds."""
        loose = self.tmp / "loose" / "utils"
        loose.mkdir(parents=True)
        return pathlib.Path(shutil.copy(SCRIPT_SRC, loose / SCRIPT_SRC.name))

    def test_version_check_audits_nothing_and_needs_no_repository(self) -> None:
        # `check-tools.sh --release-only` asks this before any network
        # stage, on a host that may not have the tree's git state at all.
        loose = self._loose_copy()
        ceiling = {"GIT_CEILING_DIRECTORIES": str(self.tmp)}
        # The control: a real run from the same copy needs the repository,
        # so the pass below is the early exit and not a lucky `git`.
        control = self._run("advisories", script=loose, **ceiling)
        self.assertEqual(control.returncode, 2, control.stderr)
        self.assertIn("git ls-files failed", control.stderr)
        result = self._run("--version-check", script=loose, **ceiling)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "0.20.2\n")
        self.assertEqual(result.stderr, "")
        self.assertEqual(self._invocations(), [])

    def test_version_check_refuses_cargo_deny_below_the_floor(self) -> None:
        result = self._run("--version-check", STUB_DENY_VERSION="0.19.4")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("cargo-deny 0.19.4 is older than 0.20", result.stderr)
        self.assertIn("cargo install --locked cargo-deny@", result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(self._invocations(), [])

    def test_version_check_takes_no_other_arguments(self) -> None:
        # Dropping the extra argument would let `--version-check advisories`
        # report success for an audit that never ran.
        result = self._run("--version-check", "advisories")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("--version-check takes no other arguments", result.stderr)
        self.assertEqual(self._invocations(), [])

    def test_an_option_is_not_forwarded_as_a_check(self) -> None:
        # Forwarded, `--help` makes each of the eight `cargo-deny check`
        # runs print its help and exit 0, and the gate reports OK.
        result = self._run("advisories", "--help")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("unknown option '--help'", result.stderr)
        self.assertEqual(self._invocations(), [])

    # --- finding cargo-deny (#1826) ---

    def _path_without_cargo_deny(self) -> str:
        """A PATH holding only what the script runs before the audit.

        Built rather than filtered from the caller's, which may hold a
        real cargo-deny in any directory.
        """
        bare = self.tmp / "bare-bin"
        bare.mkdir()
        for tool in ("bash", "dirname"):
            found = shutil.which(tool)
            self.assertIsNotNone(found, tool)
            (bare / tool).symlink_to(str(found))
        return str(bare)

    def test_cargo_deny_absent_from_path_names_the_install(self) -> None:
        cargo_home = self.tmp / "cargo-home"
        cargo_home.mkdir()
        result = self._run(
            "--version-check",
            PATH=self._path_without_cargo_deny(),
            CARGO_HOME=str(cargo_home),
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("cargo-deny not found on PATH", result.stderr)
        self.assertIn("cargo install --locked cargo-deny@", result.stderr)

    def test_cargo_deny_off_path_is_named_not_reinstalled(self) -> None:
        # `cargo deny` finds a copy in `$CARGO_HOME/bin` off PATH; this
        # script does not, and reinstalling would put the new one beside
        # it, so the refusal names the directory instead of the install.
        bin_dir = self.tmp / "cargo-home" / "bin"
        bin_dir.mkdir(parents=True)
        shutil.copy(self.tmp / "stub-bin" / "cargo-deny", bin_dir / "cargo-deny")
        result = self._run(
            "--version-check",
            PATH=self._path_without_cargo_deny(),
            CARGO_HOME=str(bin_dir.parent),
        )
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn(f"but {bin_dir / 'cargo-deny'} exists", result.stderr)
        self.assertIn("put its directory on PATH", result.stderr)
        self.assertNotIn("cargo install", result.stderr)

    def test_an_old_cargo_deny_is_named_by_path(self) -> None:
        # An older copy earlier on PATH outranks a fresh install, so the
        # remedy must say which binary was judged.
        result = self._run("--version-check", STUB_DENY_VERSION="0.19.4")
        self.assertEqual(result.returncode, 2, result.stderr)
        stub = self.tmp / "stub-bin" / "cargo-deny"
        self.assertIn(f"it is {stub}, the first cargo-deny on PATH", result.stderr)

    def test_unreadable_cargo_deny_version_is_refused(self) -> None:
        # Without this refusal, `[ x -eq 0 ]` errors inside the version
        # `if`, which bash reads as false, and the gate runs a cargo-deny
        # whose version it never established.
        result = self._run(*ALL_CHECKS, STUB_DENY_VERSION="unknown")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertIn("cannot read a version from 'cargo-deny unknown'", result.stderr)
        self.assertEqual(self._invocations(), [])

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


class WiringTest(unittest.TestCase):
    """Every caller goes through the script, and every install names one pin."""

    # Each workflow and the checks its step must pass: the per-PR job all
    # four, the weekly one the only check that ages.
    WORKFLOWS = {
        "ci.yml": set(ALL_CHECKS),
        "deny-advisories.yml": {"advisories"},
    }
    # Every file that installs cargo-deny or tells a reader how to; each
    # must name the release CI runs. The workflows are WORKFLOWS', so a
    # deny workflow added there is pin-checked too.
    PIN_SITES = (
        *(f".github/workflows/{name}" for name in WORKFLOWS),
        "utils/check-deny-manifests.sh",
        "RELEASING.md",
        "Dockerfile",
    )
    # Any version a site names, exact or not (`@0.19`, `@^0.20`,
    # `@latest`), so a second one beside the pin is a second value rather
    # than no match.
    PIN = re.compile(r"cargo-deny@([\w.^~=<>*+-]+)")
    # An install of cargo-deny with no `@version`: a `cargo install` /
    # `cargo +toolchain install` / `cargo binstall` / `cargo-binstall`
    # line, an install-action `tool:` list, and an entry of a
    # backslash-continued install list such as the Dockerfile's. One
    # pinned mention satisfies PIN, so an unpinned line beside it is seen
    # only by this.
    UNPINNED = re.compile(
        r"(?:\bcargo(?:[ \t]+\+\S+)?[ \t]+b?install\b[^\n]*?\b"
        r"|\bcargo-binstall\b[^\n]*?\b"
        r"|\btool:[^\n]*?\b"
        r"|^[ \t]+(?=cargo-deny[ \t;]*\\?[ \t]*$))"
        r"cargo-deny(?![@\w-])",
        re.MULTILINE,
    )
    # A step's `run:` line, so a commented-out step or a mention in a
    # comment does not count as a call; the group is its arguments.
    RUN = re.compile(
        r"^\s*(?:-\s+)?run:\s*\./utils/check-deny-manifests\.sh\s+([^#\n]*)",
        re.MULTILINE,
    )
    # A recipe line that runs the script, by any path spelling and with or
    # without an explicit `bash`, but not an `echo` that names it. The
    # groups are make's prefix characters and the script's arguments.
    RECIPE_CALL = re.compile(
        r"^\t([@+-]*)(?:bash\s+)?\S*utils/check-deny-manifests\.sh\s+([^#\n]*)",
        re.MULTILINE,
    )
    # cargo-deny run anywhere on a recipe line, through cargo or not; read
    # with quoted strings removed, so an `echo` naming it does not count.
    RECIPE_BARE_DENY = re.compile(r"(?:\bcargo|\$[({]CARGO[)}])[ -]deny\b")
    QUOTED = re.compile(r"\"[^\"]*\"|'[^']*'")
    # Lines GNU make reads inside a recipe without ending it.
    RECIPE_TRANSPARENT = re.compile(
        r"^[ \t]*(?:$|#|(?:ifeq|ifneq|ifdef|ifndef|else|endif)\b)"
    )

    @classmethod
    def _recipe(cls, makefile: str, target: str) -> list[str]:
        """The recipe lines of `target`, up to the next line that ends it.

        GNU make keeps a recipe open across blank lines, comment-only
        lines (indented or not) and conditional directives, so stopping
        at the first of those would leave the recipe lines after it
        unread.
        """
        lines = makefile.splitlines()
        rule = re.compile(rf"{re.escape(target)}[ \t]*:(?!=)")
        start = next(i for i, line in enumerate(lines) if rule.match(line)) + 1
        recipe = []
        for line in lines[start:]:
            if line.startswith("\t"):
                recipe.append(line)
            elif not cls.RECIPE_TRANSPARENT.match(line):
                break
        return recipe

    def test_both_workflows_call_the_script(self) -> None:
        # A workflow left on `cargo-deny-action`, or one that stops calling
        # the script, audits the root lockfile alone again — the gap #1823
        # closed — and nothing else would notice.
        for name, checks in self.WORKFLOWS.items():
            text = (REPO_ROOT / ".github" / "workflows" / name).read_text(
                encoding="utf-8"
            )
            with self.subTest(workflow=name):
                calls = self.RUN.findall(text)
                self.assertEqual(len(calls), 1, calls)
                # A step trimmed to `advisories` would still call the
                # script and drop three checks from every PR.
                self.assertEqual(set(calls[0].split()), checks)
                self.assertNotIn("cargo-deny-action@", text)

    def test_release_check_audits_every_lockfile(self) -> None:
        # `make release-check` runs only before a tag, so a recipe that
        # drifts back to a root-only `cargo deny check` (#1826) would be
        # found by the next release, or never. A stubbed run of the target
        # would need cargo-about and five `cargo publish` dry-runs, so
        # this reads the recipe instead.
        recipe = self._recipe(
            (REPO_ROOT / "Makefile").read_text(encoding="utf-8"), "release-check"
        )
        # The selector found the real recipe, not an empty or stray one.
        self.assertIn("verify-changelog", "\n".join(recipe))
        calls = self.RECIPE_CALL.findall("\n".join(recipe))
        self.assertEqual(len(calls), 1, calls)
        prefix, args = calls[0]
        # make's `-` ignores a failing line: a red audit would print
        # "(ignored)" and the recipe would still end in "passed".
        self.assertNotIn("-", prefix)
        self.assertEqual(set(args.split()), set(ALL_CHECKS))
        bare = [
            line
            for line in recipe
            if self.RECIPE_BARE_DENY.search(self.QUOTED.sub("", line))
        ]
        self.assertEqual(bare, [])

    def test_every_install_names_cis_one_pin(self) -> None:
        # The script refuses a cargo-deny below 0.20, so an install line
        # that drifts from CI's pin hands the reader one it may refuse,
        # and an unpinned one installs a release CI has never run. A bump
        # edits every site; this names any the bump missed.
        pins: dict[str, set[str]] = {}
        for rel in self.PIN_SITES:
            text = (REPO_ROOT / rel).read_text(encoding="utf-8")
            pins[rel] = set(self.PIN.findall(text))
            with self.subTest(file=rel):
                self.assertEqual(len(pins[rel]), 1, pins[rel])
                self.assertEqual(self.UNPINNED.findall(text), [])
        self.assertEqual(len(set().union(*pins.values())), 1, pins)


if __name__ == "__main__":
    unittest.main()
