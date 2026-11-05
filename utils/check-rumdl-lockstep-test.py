#!/usr/bin/env python3
"""Tests for check-rumdl-lockstep.py.

Three kinds of test, matching the check-ruff-lockstep-test.py pattern:

* Unit tests against synthetic file contents, weighted toward the shapes
  a line-oriented read can get wrong — the other rows' identically
  shaped pins (rumdl, actionlint and checkmake are each the others'
  decoys), a ``${RUMDL_VERSION}`` expansion that is not a key, the arm64
  hash sitting beside the amd64 one — and toward the malformed inputs on
  which a version gate must fail loudly rather than pass.
* ``main()`` tests over a synthetic repository root, covering each drift
  branch and the file it names, and ``--update``'s all-or-nothing write
  across the three baselines.
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
from typing import Any
from unittest import mock

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-rumdl-lockstep.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_rumdl_lockstep", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    # Registered before execution because `@dataclass` resolves a
    # field's forward reference through `sys.modules[cls.__module__]`;
    # an unregistered module makes that lookup `None` and the decorator
    # raises at import time.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


GATE = _load_module()

X86_SHA = "a" * 64
ARM_SHA = "b" * 64
# An unchecked tool's hash (the fictitious `foolint` step), deliberately
# equal in shape: the gate must not reach it however the files are
# ordered.
OTHER_SHA = "c" * 64
# actionlint's and checkmake's, distinct from rumdl's and each other's so
# a read that crossed blocks returns a value the assertion can tell apart.
AL_SHA = "1" * 64
AL_ARM_SHA = "2" * 64
CM_SHA = "4" * 64
CM_ARM_SHA = "5" * 64

MISE_SAMPLE = """\
[tools]
node = "25.9.0"
# Keep in lockstep with ci.yml and the Dockerfile.
rumdl = "0.3.2"
python = "3.12"
"pipx:ruff" = "latest"
"""

# The rumdl step sits between two siblings with identically shaped env
# maps, and the comment above each checked step spells its key after a
# `#`; rumdl's also expands `${RUMDL_VERSION}` the way the real one does.
CI_SAMPLE = f"""\
jobs:
  lint:
    steps:
      # ACTIONLINT_VERSION: bump with the Dockerfile.
      - name: Install actionlint
        env:
          ACTIONLINT_VERSION: "1.7.12"
          ACTIONLINT_SHA256: "{AL_SHA}"
        run: |
          url="https://example.invalid/v${{ACTIONLINT_VERSION}}/actionlint_${{ACTIONLINT_VERSION}}_linux_amd64.tar.gz"
          echo "${{ACTIONLINT_SHA256}}  /tmp/actionlint.tgz" | sha256sum -c -
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
          CHECKMAKE_SHA256: "{CM_SHA}"
        run: |
          url="https://example.invalid/${{CHECKMAKE_VERSION}}/checkmake-${{CHECKMAKE_VERSION}}.linux.amd64"
          echo "${{CHECKMAKE_SHA256}}  /tmp/checkmake" | sha256sum -c -
      - name: Install foolint
        env:
          FOOLINT_VERSION: "9.9.9"
          FOOLINT_SHA256: "{OTHER_SHA}"
        run: echo
"""

# Three heredoc RUN blocks of the same `case` shape — actionlint, rumdl,
# then checkmake — so the block selection, not position, decides which
# hash each row reads. checkmake's arms set whole URLs, and its arm64 arm
# names a fork version (`0.4.0`, deliberately not rumdl's `0.3.2`, which
# `_bumped` rewrites wherever it appears) rather than `${CHECKMAKE_VERSION}`.
DOCKER_SAMPLE = f"""\
FROM debian:trixie
# Toolchain / release pins.
#   RUMDL/…/ACTIONLINT -> .github/workflows/ci.yml lint job
ENV RUMDL_VERSION=0.3.2
ENV CHECKMAKE_VERSION=0.2.2
ENV ACTIONLINT_VERSION=1.7.12

RUN <<'EOF'
set -eux
case $(dpkg --print-architecture) in
    amd64) ACTIONLINT_ARCH="amd64"; SHA256="{AL_SHA}" ;;
    arm64) ACTIONLINT_ARCH="arm64"; SHA256="{AL_ARM_SHA}" ;;
esac
wget -O /tmp/actionlint.tgz "https://example.invalid/v${{ACTIONLINT_VERSION}}/actionlint_${{ACTIONLINT_VERSION}}_linux_${{ACTIONLINT_ARCH}}.tar.gz"
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

# checkmake (Makefile linter)
RUN <<'EOF'
set -eux
CHECKMAKE_FORK_VERSION="0.4.0"
case $(dpkg --print-architecture) in
    amd64) URL="https://example.invalid/${{CHECKMAKE_VERSION}}/checkmake-${{CHECKMAKE_VERSION}}.linux.amd64"; SHA256="{CM_SHA}" ;;
    arm64) URL="https://example.invalid/fork/v${{CHECKMAKE_FORK_VERSION}}/checkmake-v${{CHECKMAKE_FORK_VERSION}}.linux.arm64"; SHA256="{CM_ARM_SHA}" ;;
    *)     echo "WARNING: no checkmake build; skipping"; exit 0 ;;
esac
wget -O /tmp/checkmake "${{URL}}"
echo "${{SHA256}}  /tmp/checkmake" | sha256sum --check
EOF
"""


class MiseVersionTest(unittest.TestCase):
    def test_the_tools_entry_is_returned(self) -> None:
        self.assertEqual(GATE.read_mise_version(MISE_SAMPLE, "rumdl"), "0.3.2")

    def test_a_missing_entry_is_a_hard_error(self) -> None:
        text = MISE_SAMPLE.replace('rumdl = "0.3.2"\n', "")
        with self.assertRaises(SystemExit) as caught:
            GATE.read_mise_version(text, "rumdl")
        self.assertIn("under [tools]", str(caught.exception))

    def test_a_non_string_entry_is_a_hard_error(self) -> None:
        # mise accepts a table form too; this gate reads only the string
        # form, and must say so rather than compare a dict to a version.
        text = MISE_SAMPLE.replace('rumdl = "0.3.2"', 'rumdl = { version = "0.3.2" }')
        with self.assertRaises(SystemExit):
            GATE.read_mise_version(text, "rumdl")

    def test_an_entry_outside_tools_is_not_read(self) -> None:
        text = '[env]\nrumdl = "0.3.2"\n'
        with self.assertRaises(SystemExit):
            GATE.read_mise_version(text, "rumdl")

    def test_invalid_toml_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.read_mise_version("[tools\n", "rumdl")
        self.assertIn("not valid TOML", str(caught.exception))


class CiPinTest(unittest.TestCase):
    def test_the_rumdl_step_env_is_returned(self) -> None:
        # The sample has `${RUMDL_VERSION}` in a comment and in `run:`;
        # only the `env:` keys count, so exactly one of each is found.
        self.assertEqual(GATE.read_ci_pin(CI_SAMPLE, GATE.RUMDL), ("0.3.2", X86_SHA))

    def test_the_actionlint_step_env_is_returned(self) -> None:
        # The step before rumdl's, with its key spelled in a comment too.
        self.assertEqual(
            GATE.read_ci_pin(CI_SAMPLE, GATE.ACTIONLINT), ("1.7.12", AL_SHA)
        )

    def test_the_checkmake_step_env_is_returned(self) -> None:
        # The step after rumdl's, directly above the unchecked decoy.
        self.assertEqual(
            GATE.read_ci_pin(CI_SAMPLE, GATE.CHECKMAKE), ("0.2.2", CM_SHA)
        )

    def test_an_unquoted_or_commented_value_is_read(self) -> None:
        text = CI_SAMPLE.replace(
            'RUMDL_VERSION: "0.3.2"', "RUMDL_VERSION: 0.3.2  # pin"
        )
        self.assertEqual(GATE.read_ci_pin(text, GATE.RUMDL)[0], "0.3.2")

    def test_crlf_line_endings_are_handled(self) -> None:
        self.assertEqual(
            GATE.read_ci_pin(CI_SAMPLE.replace("\n", "\r\n"), GATE.RUMDL),
            ("0.3.2", X86_SHA),
        )

    def test_a_missing_version_key_is_a_hard_error(self) -> None:
        text = CI_SAMPLE.replace('          RUMDL_VERSION: "0.3.2"\n', "")
        with self.assertRaises(SystemExit) as caught:
            GATE.read_ci_pin(text, GATE.RUMDL)
        self.assertIn("RUMDL_VERSION", str(caught.exception))
        self.assertIn("found 0", str(caught.exception))

    def test_a_duplicated_step_is_a_hard_error(self) -> None:
        # Ambiguous rather than wrong: picking either would let a stale
        # duplicate decide what the gate compares.
        text = CI_SAMPLE + '      - env:\n          RUMDL_SHA256: "' + X86_SHA + '"\n'
        with self.assertRaises(SystemExit) as caught:
            GATE.read_ci_pin(text, GATE.RUMDL)
        self.assertIn("RUMDL_SHA256", str(caught.exception))
        self.assertIn("found 2", str(caught.exception))

    def test_a_truncated_hash_is_a_hard_error(self) -> None:
        text = CI_SAMPLE.replace(X86_SHA, X86_SHA[:40])
        with self.assertRaises(SystemExit) as caught:
            GATE.read_ci_pin(text, GATE.RUMDL)
        self.assertIn("not a 64-digit", str(caught.exception))


class DockerVersionTest(unittest.TestCase):
    def test_the_env_line_is_returned(self) -> None:
        self.assertEqual(GATE.read_docker_version(DOCKER_SAMPLE, GATE.RUMDL), "0.3.2")

    def test_the_actionlint_env_line_is_returned(self) -> None:
        self.assertEqual(
            GATE.read_docker_version(DOCKER_SAMPLE, GATE.ACTIONLINT), "1.7.12"
        )

    def test_the_checkmake_env_line_is_returned(self) -> None:
        self.assertEqual(
            GATE.read_docker_version(DOCKER_SAMPLE, GATE.CHECKMAKE), "0.2.2"
        )

    def test_the_legacy_space_form_is_read(self) -> None:
        text = DOCKER_SAMPLE.replace(
            "ENV RUMDL_VERSION=0.3.2", "ENV RUMDL_VERSION 0.3.2"
        )
        self.assertEqual(GATE.read_docker_version(text, GATE.RUMDL), "0.3.2")

    def test_a_quoted_value_is_unquoted(self) -> None:
        text = DOCKER_SAMPLE.replace("RUMDL_VERSION=0.3.2", 'RUMDL_VERSION="0.3.2"')
        self.assertEqual(GATE.read_docker_version(text, GATE.RUMDL), "0.3.2")

    def test_a_missing_env_line_is_a_hard_error(self) -> None:
        text = DOCKER_SAMPLE.replace("ENV RUMDL_VERSION=0.3.2\n", "")
        with self.assertRaises(SystemExit) as caught:
            GATE.read_docker_version(text, GATE.RUMDL)
        self.assertIn("found 0", str(caught.exception))

    def test_two_env_lines_are_a_hard_error(self) -> None:
        text = DOCKER_SAMPLE + "ENV RUMDL_VERSION=0.3.1\n"
        with self.assertRaises(SystemExit) as caught:
            GATE.read_docker_version(text, GATE.RUMDL)
        self.assertIn("found 2", str(caught.exception))


class DockerShaTest(unittest.TestCase):
    def test_the_x86_64_arm_of_the_rumdl_block_is_returned(self) -> None:
        # The first `amd64)` arm in the file is actionlint's, and the
        # rumdl block's other arm is arm64's: a position-based read would
        # return one of them instead.
        self.assertEqual(GATE.read_docker_x86_64_sha(DOCKER_SAMPLE, GATE.RUMDL), X86_SHA)

    def test_the_amd64_arm_of_the_actionlint_block_is_returned(self) -> None:
        # The mirror case: the rumdl block after it is the decoy, and its
        # own arm64 arm sits beside the one read.
        self.assertEqual(
            GATE.read_docker_x86_64_sha(DOCKER_SAMPLE, GATE.ACTIONLINT), AL_SHA
        )

    def test_the_amd64_arm_of_the_checkmake_block_is_returned(self) -> None:
        # The last block, whose arm64 arm interpolates a *different*
        # variable (`${CHECKMAKE_FORK_VERSION}`): only the amd64 arm names
        # `linux.amd64`, so the fork's hash beside it is not read.
        self.assertEqual(
            GATE.read_docker_x86_64_sha(DOCKER_SAMPLE, GATE.CHECKMAKE), CM_SHA
        )
        # A second heredoc interpolating a variable whose name *extends*
        # `CHECKMAKE_VERSION` is not a checkmake block either. Only the
        # `\b` in the reader's version reference says so: without it
        # `${CHECKMAKE_VERSION_FORK}` matches, two blocks are found and
        # the reader hard-errors. (The sample's own fork variable,
        # `${CHECKMAKE_FORK_VERSION}`, is a different prefix and needs
        # no `\b`; `test_no_tool_block_is_a_hard_error` pins that one.)
        extended_name = (
            "\nRUN <<'EOF'\n"
            'CHECKMAKE_VERSION_FORK="0.4.0"\n'
            'echo "fork v${CHECKMAKE_VERSION_FORK} linux.amd64"; '
            f'SHA256="{OTHER_SHA}"\n'
            "EOF\n"
        )
        self.assertEqual(
            GATE.read_docker_x86_64_sha(DOCKER_SAMPLE + extended_name, GATE.CHECKMAKE),
            CM_SHA,
        )

    def test_reordered_arms_still_resolve_to_the_triple(self) -> None:
        amd = f'    amd64) TRIPLE="x86_64-unknown-linux-gnu";  SHA256="{X86_SHA}" ;;\n'
        arm = f'    arm64) TRIPLE="aarch64-unknown-linux-gnu"; SHA256="{ARM_SHA}" ;;\n'
        text = DOCKER_SAMPLE.replace(amd + arm, arm + amd)
        self.assertNotEqual(text, DOCKER_SAMPLE)
        self.assertEqual(GATE.read_docker_x86_64_sha(text, GATE.RUMDL), X86_SHA)

    def test_an_unquoted_delimiter_is_recognised(self) -> None:
        text = DOCKER_SAMPLE.replace("RUN <<'EOF'", "RUN <<EOF")
        self.assertEqual(GATE.read_docker_x86_64_sha(text, GATE.RUMDL), X86_SHA)

    def test_run_flags_before_the_heredoc_are_accepted(self) -> None:
        text = DOCKER_SAMPLE.replace(
            "RUN <<'EOF'", "RUN --mount=type=cache,target=/tmp/c <<'EOF'"
        )
        self.assertEqual(GATE.read_docker_x86_64_sha(text, GATE.RUMDL), X86_SHA)

    def test_no_tool_block_is_a_hard_error(self) -> None:
        cases = {
            GATE.RUMDL: DOCKER_SAMPLE.replace("${RUMDL_VERSION}", "0.3.2"),
            GATE.ACTIONLINT: DOCKER_SAMPLE.replace("${ACTIONLINT_VERSION}", "1.7.12"),
            # The arm64 arm's `${CHECKMAKE_FORK_VERSION}` stays and must
            # not count as the block's version reference.
            GATE.CHECKMAKE: DOCKER_SAMPLE.replace("${CHECKMAKE_VERSION}", "0.2.2"),
        }
        for pin, text in cases.items():
            with self.subTest(tool=pin.tool):
                self.assertNotEqual(text, DOCKER_SAMPLE)
                with self.assertRaises(SystemExit) as caught:
                    GATE.read_docker_x86_64_sha(text, pin)
                self.assertIn(f"names ${{{pin.version_var}}}", str(caught.exception))
                self.assertIn("found 0", str(caught.exception))

    def test_a_block_without_an_amd64_arm_is_a_hard_error(self) -> None:
        cases = {
            GATE.RUMDL: DOCKER_SAMPLE.replace(
                'TRIPLE="x86_64-unknown-linux-gnu"', 'TRIPLE="x"'
            ),
            GATE.ACTIONLINT: DOCKER_SAMPLE.replace(
                'ACTIONLINT_ARCH="amd64"', 'ACTIONLINT_ARCH="x"'
            ),
            GATE.CHECKMAKE: DOCKER_SAMPLE.replace("linux.amd64", "linux.x"),
        }
        for pin, text in cases.items():
            with self.subTest(tool=pin.tool):
                self.assertNotEqual(text, DOCKER_SAMPLE)
                with self.assertRaises(SystemExit) as caught:
                    GATE.read_docker_x86_64_sha(text, pin)
                # The arm's own error, not the block-count one, which also
                # ends in "found 0" and is what a fixture that lost the
                # block raises.
                self.assertIn("SHA256=", str(caught.exception))
                self.assertIn(pin.amd64_anchor, str(caught.exception))
                self.assertIn(f"{pin.tool} block", str(caught.exception))
                self.assertIn("found 0", str(caught.exception))

    def test_an_unterminated_heredoc_yields_no_block(self) -> None:
        # The closing `EOF` is the only thing that delimits a block; a
        # body that never closes must not be read as one. The last block
        # in the sample, whose `EOF` this drops, is checkmake's.
        text = DOCKER_SAMPLE.rsplit("EOF", 1)[0]
        with self.assertRaises(SystemExit) as caught:
            GATE.read_docker_x86_64_sha(text, GATE.CHECKMAKE)
        self.assertIn("found 0", str(caught.exception))

    def test_a_malformed_hash_is_a_hard_error(self) -> None:
        text = DOCKER_SAMPLE.replace(X86_SHA, X86_SHA.upper())
        with self.assertRaises(SystemExit) as caught:
            GATE.read_docker_x86_64_sha(text, GATE.RUMDL)
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

    def test_two_sites_that_disagree_are_both_named(self) -> None:
        # actionlint's shape: with no majority, neither side is "right".
        self.assertEqual(GATE.drifted({self.A: "1", self.B: "1"}), [])
        self.assertEqual(GATE.drifted({self.A: "1", self.B: "2"}), [self.A, self.B])


# Each tool's next release in the samples: its versions, and the amd64
# hash in ci.yml and the Dockerfile.
NEW_SHA = "d" * 64
NEW_AL_SHA = "3" * 64
NEW_CM_SHA = "6" * 64
_NEXT = {
    "rumdl": ("0.3.2", "0.3.3", X86_SHA, NEW_SHA),
    "actionlint": ("1.7.12", "1.7.13", AL_SHA, NEW_AL_SHA),
    "checkmake": ("0.2.2", "0.2.3", CM_SHA, NEW_CM_SHA),
}
HONEST, STALE = "honest", "stale"


def _bumped(**kinds: str) -> dict[str, str]:
    """The sample files with each named tool moved to its next release.

    ``HONEST`` moves the tool's versions and its amd64 hash; ``STALE``
    moves only the versions, keeping the old release's hash.
    """
    files = {"mise": MISE_SAMPLE, "ci": CI_SAMPLE, "docker": DOCKER_SAMPLE}
    for tool, kind in kinds.items():
        old, new, old_sha, new_sha = _NEXT[tool]
        files = {name: text.replace(old, new) for name, text in files.items()}
        if kind == HONEST:
            files = {name: text.replace(old_sha, new_sha) for name, text in files.items()}
    return files


# Valid records under a header the template does not write, so a baseline
# `--update` rewrote no longer reads byte for byte as one it left alone.
HAND_RUMDL_BASELINE = f"# hand-written header\n0.3.2 {X86_SHA}\n"
HAND_AL_BASELINE = f"# hand-written header\n1.7.12 {AL_SHA}\n"
HAND_CM_BASELINE = f"# hand-written header\n0.2.2 {CM_SHA}\n"


class RepoCase(unittest.TestCase):
    """A synthetic repository root for `main()` to read."""

    def _repo(
        self,
        mise: str = MISE_SAMPLE,
        ci: str = CI_SAMPLE,
        docker: str = DOCKER_SAMPLE,
        rumdl_baseline: str | None = None,
        actionlint_baseline: str | None = None,
        checkmake_baseline: str | None = None,
    ) -> None:
        root = pathlib.Path(self.enterContext(tempfile.TemporaryDirectory()))
        (root / GATE.CI_WORKFLOW).parent.mkdir(parents=True)
        (root / GATE.MISE_TOML).write_text(mise, encoding="utf-8")
        (root / GATE.CI_WORKFLOW).write_text(ci, encoding="utf-8")
        (root / GATE.DOCKERFILE).write_text(docker, encoding="utf-8")
        if rumdl_baseline is None:
            rumdl_baseline = GATE.format_baseline(GATE.RUMDL, "0.3.2", X86_SHA)
        if actionlint_baseline is None:
            actionlint_baseline = GATE.format_baseline(GATE.ACTIONLINT, "1.7.12", AL_SHA)
        if checkmake_baseline is None:
            checkmake_baseline = GATE.format_baseline(GATE.CHECKMAKE, "0.2.2", CM_SHA)
        (root / GATE.RUMDL.baseline).write_text(rumdl_baseline, encoding="utf-8")
        (root / GATE.ACTIONLINT.baseline).write_text(actionlint_baseline, encoding="utf-8")
        (root / GATE.CHECKMAKE.baseline).write_text(checkmake_baseline, encoding="utf-8")
        self.enterContext(mock.patch.object(GATE, "REPO_ROOT", root))

    def _run(self, *argv: str) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = GATE.main(list(argv))
        return code, out.getvalue(), err.getvalue()

    def _baselines(self) -> dict[str, bytes]:
        """Every baseline file's bytes, keyed by tool."""
        return {
            pin.tool: (GATE.REPO_ROOT / pin.baseline).read_bytes() for pin in GATE.PINS
        }

    def _recorded(self, pin: Any) -> tuple[str, str]:
        text = (GATE.REPO_ROOT / pin.baseline).read_text(encoding="utf-8")
        recorded: tuple[str, str] = GATE.read_baseline(text, pin)
        return recorded


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
        # The version and site count per row, not just the word OK: a
        # pass that names the wrong version, or reads a mise.toml site
        # actionlint does not have, is the drift this gate exists to
        # report.
        lines = out.splitlines()
        self.assertEqual(len(lines), 3, out)
        self.assertTrue(lines[0].startswith("rumdl lockstep OK (rumdl 0.3.2,"), out)
        self.assertIn("3 sites checked", lines[0])
        self.assertTrue(
            lines[1].startswith("actionlint lockstep OK (actionlint 1.7.12,"), out
        )
        self.assertIn(f"linux_amd64 SHA256 {AL_SHA[:12]}...", lines[1])
        self.assertIn("2 sites checked", lines[1])
        self.assertTrue(
            lines[2].startswith("checkmake lockstep OK (checkmake 0.2.2,"), out
        )
        self.assertIn(f"linux.amd64 SHA256 {CM_SHA[:12]}...", lines[2])
        self.assertIn("2 sites checked", lines[2])

    def test_an_actionlint_only_drift_names_the_dockerfile(self) -> None:
        cases = {
            "version": DOCKER_SAMPLE.replace(
                "ENV ACTIONLINT_VERSION=1.7.12", "ENV ACTIONLINT_VERSION=1.7.13"
            ),
            "hash": DOCKER_SAMPLE.replace(AL_SHA, NEW_AL_SHA),
        }
        for drift, docker in cases.items():
            with self.subTest(drift=drift):
                self.assertNotEqual(docker, DOCKER_SAMPLE)
                self._repo(docker=docker)
                code, out, err = self._run()
                self.assertEqual(code, 1)
                self.assertEqual(out, "")
                self.assertIn("error: actionlint pin declarations disagree:", err)
                self.assertNotIn("rumdl", err)
                # actionlint has no mise.toml site, in the table or the
                # footer's list of files to bump.
                self.assertNotIn("mise.toml", err)
                self.assertIn(
                    f"Bump the {GATE.CI_WORKFLOW} lint job's "
                    "ACTIONLINT_VERSION / ACTIONLINT_SHA256",
                    err,
                )
                if drift == "version":
                    # Two sites have no majority, so both are named; the
                    # table says which one moved.
                    self.assertIn(
                        f"drifted: {GATE.CI_WORKFLOW}, {GATE.DOCKERFILE}",
                        self._drifted_line(err),
                    )
                    self.assertIn("    Dockerfile: actionlint 1.7.13", err)
                    self.assertIn(f"    {GATE.CI_WORKFLOW}: actionlint 1.7.12", err)
                else:
                    self.assertNotIn("drifted:", err)
                    self.assertIn(f"  Dockerfile: linux_amd64 SHA256 {NEW_AL_SHA}", err)
                    self.assertIn(
                        f"differs from {GATE.CI_WORKFLOW} ACTIONLINT_SHA256 {AL_SHA}",
                        err,
                    )

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
        self.assertIn(f"differs from {GATE.CI_WORKFLOW} {GATE.RUMDL.sha256_var}", err)
        self.assertIn("d" * 64, err)
        self.assertIn(X86_SHA, err)

    def test_the_arm64_hash_is_not_compared(self) -> None:
        # CI verifies amd64 only, so no offline counterpart exists; the
        # image build verifies it.
        for arm_sha in (ARM_SHA, AL_ARM_SHA, CM_ARM_SHA):
            with self.subTest(arm_sha=arm_sha[:1]):
                docker = DOCKER_SAMPLE.replace(arm_sha, "e" * 64)
                # A hash the sample no longer carries replaces nothing,
                # and an unchanged fixture passes for no reason.
                self.assertNotEqual(docker, DOCKER_SAMPLE)
                self._repo(docker=docker)
                code, _, err = self._run()
                self.assertEqual(code, 0, err)

    def test_every_problem_is_reported_in_one_run(self) -> None:
        self._repo(
            mise=MISE_SAMPLE.replace("0.3.2", "0.3.1"),
            ci=CI_SAMPLE.replace(X86_SHA, "d" * 64),
        )
        code, _, err = self._run()
        self.assertEqual(code, 1)
        self.assertIn("mise.toml", self._drifted_line(err))
        self.assertIn("d" * 64, err)

    def test_every_tool_is_reported_in_one_run(self) -> None:
        # One tool's failure must not hide the others'.
        self._repo(
            mise=MISE_SAMPLE.replace("0.3.2", "0.3.1"),
            ci=CI_SAMPLE.replace(AL_SHA, NEW_AL_SHA),
            docker=DOCKER_SAMPLE.replace(CM_SHA, NEW_CM_SHA),
        )
        code, out, err = self._run()
        self.assertEqual(code, 1)
        self.assertEqual(out, "", "a failing run prints no OK line for any tool")
        self.assertIn("error: rumdl pin declarations disagree:", err)
        self.assertIn("error: actionlint pin declarations disagree:", err)
        self.assertIn("error: checkmake pin declarations disagree:", err)

    def test_an_unreadable_input_does_not_hide_the_other_tools_drift(self) -> None:
        # A hard error on one tool's inputs is still a hard error, but it
        # carries another tool's drift with it, whichever comes first.
        cases = {
            "actionlint unreadable": (
                {"mise": MISE_SAMPLE.replace("0.3.2", "0.3.1"), "actionlint_baseline": "# x\n"},
                "error: rumdl pin declarations disagree:",
                GATE.ACTIONLINT,
            ),
            "rumdl unreadable": (
                {"ci": CI_SAMPLE.replace(AL_SHA, NEW_AL_SHA), "rumdl_baseline": "# x\n"},
                "error: actionlint pin declarations disagree:",
                GATE.RUMDL,
            ),
            "checkmake unreadable": (
                {"ci": CI_SAMPLE.replace(AL_SHA, NEW_AL_SHA), "checkmake_baseline": "# x\n"},
                "error: actionlint pin declarations disagree:",
                GATE.CHECKMAKE,
            ),
            "rumdl unreadable, checkmake drifted": (
                {"docker": DOCKER_SAMPLE.replace(CM_SHA, NEW_CM_SHA), "rumdl_baseline": "# x\n"},
                "error: checkmake pin declarations disagree:",
                GATE.RUMDL,
            ),
        }
        for name, (files, drift, unreadable) in cases.items():
            with self.subTest(name):
                self._repo(**files)
                with self.assertRaises(SystemExit) as caught:
                    self._run()
                self.assertIn(drift, str(caught.exception))
                self.assertIn(
                    f"record in {unreadable.baseline}, found 0", str(caught.exception)
                )

    def test_an_unreadable_input_does_not_hide_an_unrecorded_bump(self) -> None:
        # The other drift kind: rumdl's honest bump is a plain run's
        # failure, so the hard error carries it; under `--update` it is
        # the pair being asked for, so it is left out, and nothing is
        # written either way.
        self._repo(**_bumped(rumdl=HONEST), actionlint_baseline="# x\n")
        before = self._baselines()
        for argv, reported in (((), True), (("--update",), False)):
            with self.subTest(argv=argv):
                with self.assertRaises(SystemExit) as caught:
                    self._run(*argv)
                message = str(caught.exception)
                self.assertIn(f"record in {GATE.ACTIONLINT.baseline}, found 0", message)
                unrecorded = f"error: rumdl bumped to 0.3.3 (SHA256 {NEW_SHA[:12]}...)"
                if reported:
                    self.assertIn(unrecorded, message)
                else:
                    self.assertNotIn(unrecorded, message)
                self.assertEqual(self._baselines(), before)

    def test_an_absent_input_file_is_a_hard_error(self) -> None:
        self._repo()
        (GATE.REPO_ROOT / GATE.DOCKERFILE).unlink()
        with self.assertRaises(SystemExit) as caught:
            self._run()
        self.assertIn("cannot read Dockerfile", str(caught.exception))


class BaselineTest(RepoCase):
    """The recorded version/hash pair (#1779 review): a bump must move both."""

    def test_a_version_bump_with_stale_hashes_fails_by_name(self) -> None:
        self._repo(**_bumped(rumdl=STALE))
        code, out, err = self._run()
        self.assertEqual(code, 1)
        self.assertNotIn("lockstep OK", out)
        self.assertIn(
            "version bumped to 0.3.3 but the x86_64-unknown-linux-gnu hash is still 0.3.2's",
            err,
        )
        self.assertIn("paste its sha256 into", err)

    def test_a_stale_actionlint_bump_fails_by_name(self) -> None:
        self._repo(**_bumped(actionlint=STALE))
        code, out, err = self._run()
        self.assertEqual(code, 1)
        self.assertEqual(out, "")
        self.assertIn(
            "actionlint version bumped to 1.7.13 but the linux_amd64 hash is still 1.7.12's",
            err,
        )
        self.assertIn(f"({GATE.ACTIONLINT.baseline} records actionlint 1.7.12", err)

    def test_a_stale_checkmake_bump_fails_by_name(self) -> None:
        self._repo(**_bumped(checkmake=STALE))
        code, out, err = self._run()
        self.assertEqual(code, 1)
        self.assertEqual(out, "")
        self.assertIn(
            "checkmake version bumped to 0.2.3 but the linux.amd64 hash is still 0.2.2's",
            err,
        )
        self.assertIn(f"({GATE.CHECKMAKE.baseline} records checkmake 0.2.2", err)

    def test_an_honest_bump_fails_until_recorded(self) -> None:
        # Both moved, but the record is still 0.3.2's: left that way, the
        # next bump would be checked against the wrong release's hash.
        self._repo(**_bumped(rumdl=HONEST))
        code, out, err = self._run()
        self.assertEqual(code, 1)
        self.assertNotIn("lockstep OK", out)
        self.assertIn("still records 0.3.2", err)
        self.assertIn("--update", err)
        self.assertEqual(
            self._recorded(GATE.RUMDL), ("0.3.2", X86_SHA), "a plain run writes nothing"
        )

    def test_update_records_an_honest_bump(self) -> None:
        # rumdl only: the other tools' unchanged pairs are not an error, and
        # their baselines are left byte for byte as they were. They start
        # hand-written, since the template's bytes are also what a rewrite
        # would leave.
        self._repo(
            **_bumped(rumdl=HONEST),
            actionlint_baseline=HAND_AL_BASELINE,
            checkmake_baseline=HAND_CM_BASELINE,
        )
        before = self._baselines()
        code, out, err = self._run("--update")
        self.assertEqual(code, 0, err)
        self.assertEqual(
            out,
            f"rumdl lockstep: recorded rumdl 0.3.3, x86_64-unknown-linux-gnu SHA256 "
            f"{NEW_SHA[:12]}... in {GATE.RUMDL.baseline}.\n",
        )
        self.assertEqual(self._recorded(GATE.RUMDL), ("0.3.3", NEW_SHA))
        self.assertEqual(self._baselines()["actionlint"], before["actionlint"])
        self.assertEqual(self._baselines()["checkmake"], before["checkmake"])
        # The next run checks against the new pair and has nothing to say.
        code, out, err = self._run()
        self.assertEqual((code, err), (0, ""))

    def test_update_records_an_actionlint_only_bump(self) -> None:
        self._repo(
            **_bumped(actionlint=HONEST),
            rumdl_baseline=HAND_RUMDL_BASELINE,
            checkmake_baseline=HAND_CM_BASELINE,
        )
        before = self._baselines()
        code, out, err = self._run("--update")
        self.assertEqual(code, 0, err)
        self.assertEqual(
            out,
            f"actionlint lockstep: recorded actionlint 1.7.13, linux_amd64 SHA256 "
            f"{NEW_AL_SHA[:12]}... in {GATE.ACTIONLINT.baseline}.\n",
        )
        self.assertEqual(self._recorded(GATE.ACTIONLINT), ("1.7.13", NEW_AL_SHA))
        self.assertEqual(self._baselines()["rumdl"], before["rumdl"])
        self.assertEqual(self._baselines()["checkmake"], before["checkmake"])
        code, out, err = self._run()
        self.assertEqual((code, err), (0, ""))

    def test_update_records_a_checkmake_only_bump(self) -> None:
        self._repo(
            **_bumped(checkmake=HONEST),
            rumdl_baseline=HAND_RUMDL_BASELINE,
            actionlint_baseline=HAND_AL_BASELINE,
        )
        before = self._baselines()
        code, out, err = self._run("--update")
        self.assertEqual(code, 0, err)
        self.assertEqual(
            out,
            f"checkmake lockstep: recorded checkmake 0.2.3, linux.amd64 SHA256 "
            f"{NEW_CM_SHA[:12]}... in {GATE.CHECKMAKE.baseline}.\n",
        )
        self.assertEqual(self._recorded(GATE.CHECKMAKE), ("0.2.3", NEW_CM_SHA))
        self.assertEqual(self._baselines()["rumdl"], before["rumdl"])
        self.assertEqual(self._baselines()["actionlint"], before["actionlint"])
        code, out, err = self._run()
        self.assertEqual((code, err), (0, ""))

    def test_update_records_every_bump(self) -> None:
        self._repo(**_bumped(rumdl=HONEST, actionlint=HONEST, checkmake=HONEST))
        code, out, err = self._run("--update")
        self.assertEqual(code, 0, err)
        self.assertIn("recorded rumdl 0.3.3", out)
        self.assertIn("recorded actionlint 1.7.13", out)
        self.assertIn("recorded checkmake 0.2.3", out)
        self.assertEqual(self._recorded(GATE.RUMDL), ("0.3.3", NEW_SHA))
        self.assertEqual(self._recorded(GATE.ACTIONLINT), ("1.7.13", NEW_AL_SHA))
        self.assertEqual(self._recorded(GATE.CHECKMAKE), ("0.2.3", NEW_CM_SHA))
        code, out, err = self._run()
        self.assertEqual((code, err), (0, ""))

    def test_update_refuses_a_stale_bump(self) -> None:
        self._repo(**_bumped(rumdl=STALE))
        code, _, err = self._run("--update")
        self.assertEqual(code, 1)
        self.assertIn("hash is still 0.3.2's", err)
        self.assertEqual(
            self._recorded(GATE.RUMDL), ("0.3.2", X86_SHA), "a stale pair is never recorded"
        )

    def test_update_writes_nothing_while_any_tool_is_invalid(self) -> None:
        # In each case another tool moved honestly, so a gate that wrote
        # the valid part would change its baseline: the bytes of all three
        # must survive. The halves fail differently — a stale hash, and
        # versions or hashes that disagree across files — so each invalid
        # path is covered.
        honest_rumdl = _bumped(rumdl=HONEST)
        cases = {
            "rumdl invalid": (_bumped(rumdl=STALE, actionlint=HONEST), "rumdl", "actionlint"),
            "actionlint invalid": (
                {**honest_rumdl, "ci": _bumped(rumdl=HONEST, actionlint=HONEST)["ci"]},
                "actionlint",
                "rumdl",
            ),
            "checkmake invalid": (
                {**honest_rumdl, "docker": honest_rumdl["docker"].replace(CM_SHA, NEW_CM_SHA)},
                "checkmake",
                "rumdl",
            ),
        }
        for name, (files, invalid, valid) in cases.items():
            with self.subTest(name):
                self._repo(**files)
                before = self._baselines()
                code, out, err = self._run("--update")
                self.assertEqual(code, 1)
                self.assertEqual(out, "", "nothing is reported as recorded")
                self.assertIn(f"error: {invalid} ", err)
                self.assertNotIn(f"error: {valid} ", err)
                self.assertIn("--update recorded nothing", err)
                self.assertEqual(self._baselines(), before)

    def test_update_with_nothing_moved_is_refused(self) -> None:
        self._repo()
        before = self._baselines()
        code, _, err = self._run("--update")
        self.assertEqual(code, 1)
        self.assertIn("nothing to update", err)
        self.assertIn(f"{GATE.RUMDL.baseline} already records rumdl 0.3.2", err)
        self.assertIn(f"{GATE.ACTIONLINT.baseline} already records actionlint 1.7.12", err)
        self.assertIn(f"{GATE.CHECKMAKE.baseline} already records checkmake 0.2.2", err)
        self.assertEqual(self._baselines(), before)

    def test_a_new_hash_without_a_bump_fails(self) -> None:
        cases = {
            "rumdl": (
                X86_SHA,
                NEW_SHA,
                "the x86_64-unknown-linux-gnu hash changed but rumdl is still 0.3.2",
            ),
            "actionlint": (
                AL_SHA,
                NEW_AL_SHA,
                "the linux_amd64 hash changed but actionlint is still 1.7.12",
            ),
            "checkmake": (
                CM_SHA,
                NEW_CM_SHA,
                "the linux.amd64 hash changed but checkmake is still 0.2.2",
            ),
        }
        for tool, (old, new, message) in cases.items():
            with self.subTest(tool=tool):
                self._repo(
                    ci=CI_SAMPLE.replace(old, new), docker=DOCKER_SAMPLE.replace(old, new)
                )
                code, _, err = self._run()
                self.assertEqual(code, 1)
                self.assertIn(message, err)

    def test_a_malformed_baseline_is_a_hard_error(self) -> None:
        for pin in GATE.PINS:
            with self.subTest(tool=pin.tool):
                self._repo(**{f"{pin.tool}_baseline": "# header only\n"})
                with self.assertRaises(SystemExit) as caught:
                    self._run()
                self.assertIn("found 0", str(caught.exception))
                self.assertIn(str(pin.baseline), str(caught.exception))

    def test_the_committed_baseline_is_what_update_writes(self) -> None:
        for pin in GATE.PINS:
            with self.subTest(tool=pin.tool):
                text = (REPO_ROOT / pin.baseline).read_text(encoding="utf-8")
                self.assertEqual(
                    text, GATE.format_baseline(pin, *GATE.read_baseline(text, pin))
                )

    def test_update_from_older_records_writes_the_committed_files(self) -> None:
        # The real mise.toml, ci.yml and Dockerfile, with each baseline one
        # (fictitious) release behind: `--update` must then write exactly
        # the committed bytes, header included, for every tool.
        real = {
            name: (REPO_ROOT / path).read_text(encoding="utf-8")
            for name, path in (
                ("mise", GATE.MISE_TOML),
                ("ci", GATE.CI_WORKFLOW),
                ("docker", GATE.DOCKERFILE),
            )
        }
        self._repo(
            **real,
            rumdl_baseline=GATE.format_baseline(GATE.RUMDL, "0.0.1", "9" * 64),
            actionlint_baseline=GATE.format_baseline(GATE.ACTIONLINT, "0.0.1", "8" * 64),
            checkmake_baseline=GATE.format_baseline(GATE.CHECKMAKE, "0.0.1", "7" * 64),
        )
        code, out, err = self._run("--update")
        self.assertEqual(code, 0, err)
        self.assertEqual(len(out.splitlines()), 3, out)
        for pin in GATE.PINS:
            with self.subTest(tool=pin.tool):
                self.assertEqual(
                    (GATE.REPO_ROOT / pin.baseline).read_bytes(),
                    (REPO_ROOT / pin.baseline).read_bytes(),
                )


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
        rumdl, actionlint, checkmake = result.stdout.splitlines()
        self.assertTrue(rumdl.startswith("rumdl lockstep OK"), result.stdout)
        self.assertIn("3 sites checked", rumdl)
        self.assertTrue(actionlint.startswith("actionlint lockstep OK"), result.stdout)
        self.assertIn("2 sites checked", actionlint)
        self.assertTrue(checkmake.startswith("checkmake lockstep OK"), result.stdout)
        self.assertIn("2 sites checked", checkmake)

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
