#!/usr/bin/env python3
"""Tests for check-rumdl-lockstep.py.

Three kinds of test, matching the check-ruff-lockstep-test.py pattern:

* Unit tests against synthetic file contents, weighted toward the shapes
  a line-oriented read can get wrong — a sibling tool's identically
  shaped pin, a ``${RUMDL_VERSION}`` expansion that is not a key, the
  arm64 hash sitting beside the x86_64 one — and toward the malformed
  inputs on which a version gate must fail loudly rather than pass.
* ``main()`` tests over a synthetic repository root, covering each drift
  branch and the file it names.
* A smoke test running the real gate against the real repository,
  asserting a clean tree reports OK.

Run with:
    python3 -m unittest -q utils/check-rumdl-lockstep-test.py
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-rumdl-lockstep.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_rumdl_lockstep", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GATE = _load_module()

X86_SHA = "a" * 64
ARM_SHA = "b" * 64
# A sibling tool's hash, deliberately equal in shape: the gate must not
# reach it however the files are ordered.
OTHER_SHA = "c" * 64

MISE_SAMPLE = """\
[tools]
node = "25.9.0"
# Keep in lockstep with ci.yml and the Dockerfile.
rumdl = "0.3.2"
python = "3.12"
"pipx:ruff" = "latest"
"""

# The rumdl step sits between two siblings with identically shaped env
# maps, and the comment above it expands `${RUMDL_VERSION}` the way the
# real one does and spells the key itself after a `#`.
CI_SAMPLE = f"""\
jobs:
  lint:
    steps:
      - name: Install actionlint
        env:
          ACTIONLINT_VERSION: "1.7.12"
          ACTIONLINT_SHA256: "{OTHER_SHA}"
        run: echo
      # RUMDL_VERSION: bump with mise.toml and the Dockerfile.
      # Regenerate with:
      #   curl -fsSL https://example.invalid/v${{RUMDL_VERSION}}/rumdl.tar.gz.sha256
      - name: Install rumdl
        env:
          RUMDL_VERSION: "0.3.2"
          RUMDL_SHA256: "{X86_SHA}"
        run: |
          url="https://example.invalid/v${{RUMDL_VERSION}}/rumdl-v${{RUMDL_VERSION}}-x86_64-unknown-linux-gnu.tar.gz"
          echo "${{RUMDL_SHA256}}  /tmp/rumdl.tgz" | sha256sum -c -
      - name: Install checkmake
        env:
          CHECKMAKE_VERSION: "0.2.2"
          CHECKMAKE_SHA256: "{OTHER_SHA}"
        run: echo
"""

# Two heredoc RUN blocks of the same `case` shape, rumdl second, so the
# block selection — not position — decides which hash is read.
DOCKER_SAMPLE = f"""\
FROM debian:trixie
# Toolchain / release pins.
#   RUMDL/…/ACTIONLINT -> .github/workflows/ci.yml lint job
ENV RUMDL_VERSION=0.3.2
ENV ACTIONLINT_VERSION=1.7.12

RUN <<'EOF'
set -eux
case $(dpkg --print-architecture) in
    amd64) ACTIONLINT_ARCH="amd64"; SHA256="{OTHER_SHA}" ;;
    arm64) ACTIONLINT_ARCH="arm64"; SHA256="{OTHER_SHA}" ;;
esac
wget -O /tmp/actionlint.tgz "https://example.invalid/v${{ACTIONLINT_VERSION}}/actionlint.tar.gz"
EOF

# rumdl (Markdown linter/formatter)
RUN <<'EOF'
set -eux
case $(dpkg --print-architecture) in
    amd64) TRIPLE="x86_64-unknown-linux-gnu";  SHA256="{X86_SHA}" ;;
    arm64) TRIPLE="aarch64-unknown-linux-gnu"; SHA256="{ARM_SHA}" ;;
    *)     echo "Unsupported architecture" && exit 1 ;;
esac
wget -O /tmp/rumdl.tgz "https://example.invalid/v${{RUMDL_VERSION}}/rumdl-v${{RUMDL_VERSION}}-${{TRIPLE}}.tar.gz"
echo "${{SHA256}}  /tmp/rumdl.tgz" | sha256sum --check
EOF
"""


class MiseVersionTest(unittest.TestCase):
    def test_the_tools_entry_is_returned(self) -> None:
        self.assertEqual(GATE.read_mise_version(MISE_SAMPLE), "0.3.2")

    def test_a_missing_entry_is_a_hard_error(self) -> None:
        text = MISE_SAMPLE.replace('rumdl = "0.3.2"\n', "")
        with self.assertRaises(SystemExit) as caught:
            GATE.read_mise_version(text)
        self.assertIn("under [tools]", str(caught.exception))

    def test_a_non_string_entry_is_a_hard_error(self) -> None:
        # mise accepts a table form too; this gate reads only the string
        # form, and must say so rather than compare a dict to a version.
        text = MISE_SAMPLE.replace('rumdl = "0.3.2"', 'rumdl = { version = "0.3.2" }')
        with self.assertRaises(SystemExit):
            GATE.read_mise_version(text)

    def test_an_entry_outside_tools_is_not_read(self) -> None:
        text = '[env]\nrumdl = "0.3.2"\n'
        with self.assertRaises(SystemExit):
            GATE.read_mise_version(text)

    def test_invalid_toml_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.read_mise_version("[tools\n")
        self.assertIn("not valid TOML", str(caught.exception))


class CiPinTest(unittest.TestCase):
    def test_the_rumdl_step_env_is_returned(self) -> None:
        # The sample has `${RUMDL_VERSION}` in a comment and in `run:`;
        # only the `env:` keys count, so exactly one of each is found.
        self.assertEqual(GATE.read_ci_pin(CI_SAMPLE), ("0.3.2", X86_SHA))

    def test_an_unquoted_or_commented_value_is_read(self) -> None:
        text = CI_SAMPLE.replace(
            'RUMDL_VERSION: "0.3.2"', "RUMDL_VERSION: 0.3.2  # pin"
        )
        self.assertEqual(GATE.read_ci_pin(text)[0], "0.3.2")

    def test_crlf_line_endings_are_handled(self) -> None:
        self.assertEqual(
            GATE.read_ci_pin(CI_SAMPLE.replace("\n", "\r\n")), ("0.3.2", X86_SHA)
        )

    def test_a_missing_version_key_is_a_hard_error(self) -> None:
        text = CI_SAMPLE.replace('          RUMDL_VERSION: "0.3.2"\n', "")
        with self.assertRaises(SystemExit) as caught:
            GATE.read_ci_pin(text)
        self.assertIn("RUMDL_VERSION", str(caught.exception))
        self.assertIn("found 0", str(caught.exception))

    def test_a_duplicated_step_is_a_hard_error(self) -> None:
        # Ambiguous rather than wrong: picking either would let a stale
        # duplicate decide what the gate compares.
        text = CI_SAMPLE + '      - env:\n          RUMDL_SHA256: "' + X86_SHA + '"\n'
        with self.assertRaises(SystemExit) as caught:
            GATE.read_ci_pin(text)
        self.assertIn("RUMDL_SHA256", str(caught.exception))
        self.assertIn("found 2", str(caught.exception))

    def test_a_truncated_hash_is_a_hard_error(self) -> None:
        text = CI_SAMPLE.replace(X86_SHA, X86_SHA[:40])
        with self.assertRaises(SystemExit) as caught:
            GATE.read_ci_pin(text)
        self.assertIn("not a 64-digit", str(caught.exception))


class DockerVersionTest(unittest.TestCase):
    def test_the_env_line_is_returned(self) -> None:
        self.assertEqual(GATE.read_docker_version(DOCKER_SAMPLE), "0.3.2")

    def test_the_legacy_space_form_is_read(self) -> None:
        text = DOCKER_SAMPLE.replace(
            "ENV RUMDL_VERSION=0.3.2", "ENV RUMDL_VERSION 0.3.2"
        )
        self.assertEqual(GATE.read_docker_version(text), "0.3.2")

    def test_a_quoted_value_is_unquoted(self) -> None:
        text = DOCKER_SAMPLE.replace("RUMDL_VERSION=0.3.2", 'RUMDL_VERSION="0.3.2"')
        self.assertEqual(GATE.read_docker_version(text), "0.3.2")

    def test_a_missing_env_line_is_a_hard_error(self) -> None:
        text = DOCKER_SAMPLE.replace("ENV RUMDL_VERSION=0.3.2\n", "")
        with self.assertRaises(SystemExit) as caught:
            GATE.read_docker_version(text)
        self.assertIn("found 0", str(caught.exception))

    def test_two_env_lines_are_a_hard_error(self) -> None:
        text = DOCKER_SAMPLE + "ENV RUMDL_VERSION=0.3.1\n"
        with self.assertRaises(SystemExit) as caught:
            GATE.read_docker_version(text)
        self.assertIn("found 2", str(caught.exception))


class DockerShaTest(unittest.TestCase):
    def test_the_x86_64_arm_of_the_rumdl_block_is_returned(self) -> None:
        # The first `amd64)` arm in the file is actionlint's, and the
        # rumdl block's other arm is arm64's: a position-based read would
        # return one of them instead.
        self.assertEqual(GATE.read_docker_x86_64_sha(DOCKER_SAMPLE), X86_SHA)

    def test_reordered_arms_still_resolve_to_the_triple(self) -> None:
        amd = f'    amd64) TRIPLE="x86_64-unknown-linux-gnu";  SHA256="{X86_SHA}" ;;\n'
        arm = f'    arm64) TRIPLE="aarch64-unknown-linux-gnu"; SHA256="{ARM_SHA}" ;;\n'
        text = DOCKER_SAMPLE.replace(amd + arm, arm + amd)
        self.assertNotEqual(text, DOCKER_SAMPLE)
        self.assertEqual(GATE.read_docker_x86_64_sha(text), X86_SHA)

    def test_an_unquoted_delimiter_is_recognised(self) -> None:
        text = DOCKER_SAMPLE.replace("RUN <<'EOF'", "RUN <<EOF")
        self.assertEqual(GATE.read_docker_x86_64_sha(text), X86_SHA)

    def test_run_flags_before_the_heredoc_are_accepted(self) -> None:
        text = DOCKER_SAMPLE.replace(
            "RUN <<'EOF'", "RUN --mount=type=cache,target=/tmp/c <<'EOF'"
        )
        self.assertEqual(GATE.read_docker_x86_64_sha(text), X86_SHA)

    def test_no_rumdl_block_is_a_hard_error(self) -> None:
        text = DOCKER_SAMPLE.replace("${RUMDL_VERSION}", "0.3.2")
        with self.assertRaises(SystemExit) as caught:
            GATE.read_docker_x86_64_sha(text)
        self.assertIn("found 0", str(caught.exception))

    def test_a_block_without_an_x86_64_arm_is_a_hard_error(self) -> None:
        text = DOCKER_SAMPLE.replace('TRIPLE="x86_64-unknown-linux-gnu"', 'TRIPLE="x"')
        with self.assertRaises(SystemExit) as caught:
            GATE.read_docker_x86_64_sha(text)
        # The arm's own error, not the block-count one, which also ends
        # in "found 0" and is what a fixture that lost the block raises.
        self.assertIn("SHA256=", str(caught.exception))
        self.assertIn(GATE.X86_64_TRIPLE, str(caught.exception))
        self.assertIn("found 0", str(caught.exception))

    def test_an_unterminated_heredoc_yields_no_block(self) -> None:
        # The closing `EOF` is the only thing that delimits a block; a
        # body that never closes must not be read as one.
        text = DOCKER_SAMPLE.rsplit("EOF", 1)[0]
        with self.assertRaises(SystemExit) as caught:
            GATE.read_docker_x86_64_sha(text)
        self.assertIn("found 0", str(caught.exception))

    def test_a_malformed_hash_is_a_hard_error(self) -> None:
        text = DOCKER_SAMPLE.replace(X86_SHA, X86_SHA.upper())
        with self.assertRaises(SystemExit) as caught:
            GATE.read_docker_x86_64_sha(text)
        self.assertIn("not a 64-digit", str(caught.exception))


class DriftedTest(unittest.TestCase):
    A, B, C = (pathlib.Path(name) for name in ("a", "b", "c"))

    def test_agreement_names_nothing(self) -> None:
        self.assertEqual(GATE.drifted({self.A: "1", self.B: "1", self.C: "1"}), [])

    def test_the_odd_one_out_is_named_alone(self) -> None:
        # Each position in turn, so the answer cannot come from order.
        for odd in (self.A, self.B, self.C):
            versions = {path: "1" for path in (self.A, self.B, self.C)}
            versions[odd] = "2"
            with self.subTest(odd=odd):
                self.assertEqual(GATE.drifted(versions), [odd])

    def test_three_different_values_name_every_file(self) -> None:
        self.assertEqual(
            GATE.drifted({self.A: "1", self.B: "2", self.C: "3"}),
            [self.A, self.B, self.C],
        )


class RepoCase(unittest.TestCase):
    """A synthetic repository root for `main()` to read."""

    def _repo(
        self,
        mise: str = MISE_SAMPLE,
        ci: str = CI_SAMPLE,
        docker: str = DOCKER_SAMPLE,
        baseline: str | None = None,
    ) -> None:
        root = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        (root / GATE.CI_WORKFLOW).parent.mkdir(parents=True)
        (root / GATE.MISE_TOML).write_text(mise, encoding="utf-8")
        (root / GATE.CI_WORKFLOW).write_text(ci, encoding="utf-8")
        (root / GATE.DOCKERFILE).write_text(docker, encoding="utf-8")
        if baseline is None:
            baseline = GATE.format_baseline("0.3.2", X86_SHA)
        (root / GATE.BASELINE).write_text(baseline, encoding="utf-8")
        self.enterContext(mock.patch.object(GATE, "REPO_ROOT", root))

    def _run(self, *argv: str) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = GATE.main(list(argv))
        return code, out.getvalue(), err.getvalue()


class MainTest(RepoCase):
    """`main()` over a synthetic repository root."""

    def _drifted_line(self, err: str) -> str:
        lines = [line for line in err.splitlines() if "drifted:" in line]
        self.assertEqual(len(lines), 1, err)
        return lines[0]

    def test_an_aligned_tree_passes(self) -> None:
        self._repo()
        code, out, _ = self._run()
        self.assertEqual(code, 0)
        self.assertIn("rumdl lockstep OK", out)
        # The version, not just the word OK: a pass that names the wrong
        # version is the drift this gate exists to report.
        self.assertIn("rumdl 0.3.2", out)

    def test_a_forgotten_dockerfile_is_named(self) -> None:
        # #1653's near miss: mise.toml and ci.yml bumped, Dockerfile not.
        self._repo(
            mise=MISE_SAMPLE.replace("0.3.2", "0.3.3"),
            ci=CI_SAMPLE.replace('"0.3.2"', '"0.3.3"'),
        )
        code, _, err = self._run()
        self.assertEqual(code, 1)
        line = self._drifted_line(err)
        self.assertIn("Dockerfile", line)
        self.assertNotIn("mise.toml", line)
        self.assertNotIn("ci.yml", line)

    def test_each_drifted_file_is_named_alone(self) -> None:
        cases = {
            "mise.toml": {"mise": MISE_SAMPLE.replace("0.3.2", "0.3.1")},
            "ci.yml": {"ci": CI_SAMPLE.replace('"0.3.2"', '"0.3.1"')},
            "Dockerfile": {"docker": DOCKER_SAMPLE.replace("=0.3.2", "=0.3.1")},
        }
        for name, files in cases.items():
            with self.subTest(drifted=name):
                self._repo(**files)
                code, _, err = self._run()
                self.assertEqual(code, 1)
                line = self._drifted_line(err)
                for other in cases:
                    if other == name:
                        self.assertIn(other, line)
                    else:
                        self.assertNotIn(other, line)
                # The whole table is printed, not only the verdict.
                self.assertIn("rumdl 0.3.1", err)
                self.assertIn("rumdl 0.3.2", err)

    def test_a_hash_pasted_into_one_file_only_fails(self) -> None:
        # Same version everywhere, but the Dockerfile kept the old hash.
        self._repo(docker=DOCKER_SAMPLE.replace(X86_SHA, "d" * 64))
        code, _, err = self._run()
        self.assertEqual(code, 1)
        self.assertNotIn("drifted:", err)
        # The digest comparison's own sentence; a bare "SHA256" is also in
        # the footer every failure prints.
        self.assertIn(f"differs from {GATE.CI_WORKFLOW} {GATE.SHA256_VAR}", err)
        self.assertIn("d" * 64, err)
        self.assertIn(X86_SHA, err)

    def test_the_arm64_hash_is_not_compared(self) -> None:
        # No offline counterpart exists; the image build verifies it.
        self._repo(docker=DOCKER_SAMPLE.replace(ARM_SHA, "e" * 64))
        code, _, _ = self._run()
        self.assertEqual(code, 0)

    def test_every_problem_is_reported_in_one_run(self) -> None:
        self._repo(
            mise=MISE_SAMPLE.replace("0.3.2", "0.3.1"),
            ci=CI_SAMPLE.replace(X86_SHA, "d" * 64),
        )
        code, _, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn("mise.toml", self._drifted_line(err))
        self.assertIn("d" * 64, err)

    def test_an_absent_input_file_is_a_hard_error(self) -> None:
        self._repo()
        (GATE.REPO_ROOT / GATE.DOCKERFILE).unlink()
        with self.assertRaises(SystemExit) as caught:
            self._run()
        self.assertIn("cannot read Dockerfile", str(caught.exception))


class BaselineTest(RepoCase):
    """The recorded version/hash pair (#1779 review): a bump must move both."""

    NEW_SHA = "d" * 64

    def _bumped(self, *, rehash: bool) -> None:
        # All three versions moved to 0.3.3; the x86_64 hash in ci.yml and
        # the Dockerfile moved with them only when `rehash`.
        ci, docker = CI_SAMPLE.replace("0.3.2", "0.3.3"), DOCKER_SAMPLE.replace("0.3.2", "0.3.3")
        if rehash:
            ci, docker = ci.replace(X86_SHA, self.NEW_SHA), docker.replace(X86_SHA, self.NEW_SHA)
        self._repo(mise=MISE_SAMPLE.replace("0.3.2", "0.3.3"), ci=ci, docker=docker)

    def _recorded(self) -> tuple[str, str]:
        return GATE.read_baseline((GATE.REPO_ROOT / GATE.BASELINE).read_text(encoding="utf-8"))

    def test_a_version_bump_with_stale_hashes_fails_by_name(self) -> None:
        self._bumped(rehash=False)
        code, out, err = self._run()
        self.assertEqual(code, 1)
        self.assertNotIn("lockstep OK", out)
        self.assertIn(
            "version bumped to 0.3.3 but the x86_64-unknown-linux-gnu hash is still 0.3.2's",
            err,
        )
        self.assertIn("paste its sha256 into", err)

    def test_an_honest_bump_fails_until_recorded(self) -> None:
        # Both moved, but the record is still 0.3.2's: left that way, the
        # next bump would be checked against the wrong release's hash.
        self._bumped(rehash=True)
        code, out, err = self._run()
        self.assertEqual(code, 1)
        self.assertNotIn("lockstep OK", out)
        self.assertIn("still records 0.3.2", err)
        self.assertIn("--update", err)
        self.assertEqual(self._recorded(), ("0.3.2", X86_SHA), "a plain run writes nothing")

    def test_update_records_an_honest_bump(self) -> None:
        self._bumped(rehash=True)
        code, out, err = self._run("--update")
        self.assertEqual(code, 0, err)
        self.assertIn("recorded rumdl 0.3.3", out)
        self.assertEqual(self._recorded(), ("0.3.3", self.NEW_SHA))
        # The next run checks against the new pair and has nothing to say.
        code, out, err = self._run()
        self.assertEqual((code, err), (0, ""))

    def test_update_refuses_a_stale_bump(self) -> None:
        self._bumped(rehash=False)
        code, _, err = self._run("--update")
        self.assertEqual(code, 1)
        self.assertIn("hash is still 0.3.2's", err)
        self.assertEqual(self._recorded(), ("0.3.2", X86_SHA), "a stale pair is never recorded")

    def test_update_with_nothing_moved_is_refused(self) -> None:
        self._repo()
        code, _, err = self._run("--update")
        self.assertEqual(code, 1)
        self.assertIn("already records rumdl 0.3.2", err)

    def test_a_new_hash_without_a_bump_fails(self) -> None:
        self._repo(
            ci=CI_SAMPLE.replace(X86_SHA, self.NEW_SHA),
            docker=DOCKER_SAMPLE.replace(X86_SHA, self.NEW_SHA),
        )
        code, _, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn("hash changed but rumdl is still 0.3.2", err)

    def test_a_malformed_baseline_is_a_hard_error(self) -> None:
        self._repo(baseline="# header only\n")
        with self.assertRaises(SystemExit) as caught:
            self._run()
        self.assertIn("found 0", str(caught.exception))
        self.assertIn(str(GATE.BASELINE), str(caught.exception))

    def test_the_committed_baseline_is_what_update_writes(self) -> None:
        text = (REPO_ROOT / GATE.BASELINE).read_text(encoding="utf-8")
        self.assertEqual(text, GATE.format_baseline(*GATE.read_baseline(text)))


class RealRepositoryTest(unittest.TestCase):
    def test_clean_tree_passes(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPT_SRC)],
            capture_output=True,
            check=False,
            text=True,
            cwd=REPO_ROOT,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("rumdl lockstep OK", result.stdout)
        self.assertIn("3 sites checked", result.stdout)

    def test_it_runs_from_an_unrelated_cwd(self) -> None:
        # `REPO_ROOT` is derived from the script's own location, so the
        # gate must not depend on being invoked from the repository root
        # (AGENTS.md, "Project layout").
        with tempfile.TemporaryDirectory() as elsewhere:
            result = subprocess.run(
                [sys.executable, str(SCRIPT_SRC)],
                capture_output=True,
                check=False,
                text=True,
                cwd=elsewhere,
            )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
