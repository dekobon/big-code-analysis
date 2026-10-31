#!/usr/bin/env python3
"""Self-tests for the shell in `.github/workflows/mutation-test.yml`.

`utils/mutation-report-test.py` covers the Python half of the sharded
mutation-testing workflow. This covers the other half: the four `run:`
bodies, which are the only place the workflow's exit status, the
`| tee` capture, the summary hand-off and the issue dedupe are wired
together, and which run once a quarter. #1710 was a `| tee` without
`pipefail` in exactly such a body, green for a quarter.

Each body is extracted verbatim from the workflow and executed with
`bash -e` — GitHub's default shell for a `run:` step, with no inherited
`pipefail` — in a scratch directory, with stub `cargo` and `gh` first on
PATH and an allowlisted environment. The stubs never reach the network.
The pipefail check is built in: the mutants body with `set -euo
pipefail` weakened to `set -eu` must make the summarizer refuse a masked
exit status, so the test fails if the workflow ever loses the flag.

What this does not cover, so nobody reads it as covering: anything
GitHub evaluates rather than bash. The `if:` expressions between steps,
the `${{ }}` plumbing into each step's `env:` (the harness supplies
those variables itself, so a renamed `env:` key is invisible here),
`fromJSON` into the matrix, and the artifact upload and download that
fan the summaries in. actionlint checks the expressions' syntax; the
`GITHUB_OUTPUT` assertions below are the only check that `shards` is an
array of the declared length, since actionlint does not type `fromJSON`.
The sequencing between `aggregate` and the issue step is replicated by
the test, not proven. Four cases need `jq` and skip without it unless
`MUTATION_WORKFLOW_REQUIRE_JQ=1`, which the explicit CI step sets.

Run: ``python3 -m unittest -q utils/mutation-workflow-test.py``.
"""

from __future__ import annotations

import json
import os
import pathlib
import shlex
import shutil
import stat
import subprocess
import sys
import tempfile
import textwrap
import unittest

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "mutation-test.yml"
REPORT_SCRIPT = UTILS_DIR / "mutation-report.py"
HAVE_JQ = shutil.which("jq") is not None
HAVE_BASH = shutil.which("bash") is not None
# The explicit CI step sets this so a runner image that dropped jq fails
# rather than skipping the four cases that need it.
REQUIRE_JQ = os.environ.get("MUTATION_WORKFLOW_REQUIRE_JQ") == "1"
TIMEOUT = 60


def needs_jq(test):
    """Skip without jq, unless the environment says that must be a failure."""
    if HAVE_JQ:
        return test
    if REQUIRE_JQ:

        def fail(self, *_args, **_kwargs):
            self.fail("MUTATION_WORKFLOW_REQUIRE_JQ=1 but jq is not on PATH")

        fail.__name__ = test.__name__
        return fail
    return unittest.skip("jq is required by this body")(test)


class WorkflowShapeError(Exception):
    """The workflow is not in the shape this reader understands."""


def run_body(workflow_text: str, step: str) -> str:
    """Return the `run: |` body of the step whose `id:` or `name:` is `step`.

    Accepts exactly the shape the workflow uses: a `- id:` or `- name:`
    list item, a `run: |` key at the item's indentation plus two, and a
    literal block whose lines are indented deeper than the key. Blank
    lines inside the block are kept; the block ends at the first non-blank
    line indented at or shallower than the `run:` key. Any other block
    indicator (`|-`, `>`, a plain scalar) is refused rather than guessed
    at, and so is a step name that matches more than once.
    """
    lines = workflow_text.splitlines()
    # A step starts at the indentation of a job's `steps:` list items. A
    # `steps:` deeper than the jobs' own key depth — inside a run: body
    # or a heredoc — is text, not structure, so it contributes nothing.
    jobs_at = [i for i, line in enumerate(lines) if line.rstrip() == "jobs:"]
    if len(jobs_at) != 1:
        raise WorkflowShapeError(f"expected one top-level `jobs:`, found {len(jobs_at)}")
    job_indent = next(
        (len(l) - len(l.lstrip()) for l in lines[jobs_at[0] + 1 :] if l.strip() and not l.lstrip().startswith("#")),
        None,
    )
    if job_indent is None:
        raise WorkflowShapeError("`jobs:` has no entries")
    steps_indent = job_indent * 2
    item_indents: set[int] = set()
    for i, line in enumerate(lines):
        if line.strip() != "steps:" or len(line) - len(line.lstrip()) != steps_indent:
            continue
        for nxt in lines[i + 1 :]:
            if nxt.strip() and not nxt.lstrip().startswith("#"):
                if nxt.lstrip().startswith("- "):
                    item_indents.add(len(nxt) - len(nxt.lstrip()))
                break
    starts = [
        i
        for i, line in enumerate(lines)
        if line.strip() in (f"- id: {step}", f"- name: {step}")
        and len(line) - len(line.lstrip()) in item_indents
    ]
    if len(starts) != 1:
        raise WorkflowShapeError(f"step {step!r} matched {len(starts)} times, expected 1")
    start = starts[0]
    item_indent = len(lines[start]) - len(lines[start].lstrip())
    key_indent = item_indent + 2
    run_at = None
    for i in range(start + 1, len(lines)):
        line = lines[i]
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip())
        if indent <= item_indent:
            break
        if indent == key_indent and line.strip().startswith("run:"):
            run_at = i
            break
    if run_at is None:
        raise WorkflowShapeError(f"step {step!r} has no run: key at indent {key_indent}")
    if lines[run_at].strip() != "run: |":
        raise WorkflowShapeError(
            f"step {step!r}: only `run: |` is supported, found {lines[run_at].strip()!r}"
        )
    body: list[str] = []
    content_indent = None
    for line in lines[run_at + 1 :]:
        if not line.strip():
            body.append("")
            continue
        indent = len(line) - len(line.lstrip())
        if indent <= key_indent:
            break
        if content_indent is None:
            content_indent = indent
        if indent < content_indent:
            raise WorkflowShapeError(
                f"step {step!r}: a body line is indented shallower than the block's first line"
            )
        body.append(line[content_indent:])
    text = "\n".join(body).rstrip("\n") + "\n"
    if not text.strip():
        raise WorkflowShapeError(f"step {step!r} has an empty run: body")
    return text


STUB_CARGO = r"""#!/usr/bin/env bash
# Stub cargo for the workflow harness. Dispatches on the subcommand the
# body uses and never builds anything. Argv is logged one argument per
# line between markers, so quoting mistakes in the body are visible.
set -eu
{ echo "<call>"; printf '%s\n' "$@"; echo "</call>"; } >> "$STUB_LOG"
if [ "$1" = mutants ] && [ "${2:-}" = --version ]; then
  if [ "${STUB_VERSION_EXIT:-0}" -ne 0 ]; then
    echo "stub: version command failed" >&2
    exit "$STUB_VERSION_EXIT"
  fi
  echo "cargo-mutants 27.1.0"
  exit 0
fi
if [ "$1" = mutants ]; then
  mkdir -p target/mutants/mutants.out
  for f in caught missed timeout unviable; do : > "target/mutants/mutants.out/$f.txt"; done
  [ -n "${STUB_CAUGHT:-}" ] && printf '%s\n' "$STUB_CAUGHT" > target/mutants/mutants.out/caught.txt
  [ -n "${STUB_MISSED:-}" ] && printf '%s\n' "$STUB_MISSED" > target/mutants/mutants.out/missed.txt
  [ -n "${STUB_TIMEOUT:-}" ] && printf '%s\n' "$STUB_TIMEOUT" > target/mutants/mutants.out/timeout.txt
  echo "stub cargo-mutants ran"
  exit "${STUB_CARGO_EXIT:-0}"
fi
echo "stub cargo: unsupported invocation: $*" >&2
exit 97
"""

STUB_GH = r"""#!/usr/bin/env bash
# Stub gh for the workflow harness. Records every call losslessly;
# `issue list` evaluates the body's own --jq filter with real jq over a
# fixture so the dedupe filter is exercised, not bypassed. Strict mode:
# a jq error or a missing body file fails the call, as real gh would.
set -euo pipefail
{ echo "<call>"; printf '%s\n' "$@"; echo "</call>"; } >> "$STUB_LOG"
if [ "$1" = issue ] && [ "$2" = list ]; then
  filter=""
  while [ $# -gt 0 ]; do
    if [ "$1" = --jq ]; then filter=$2; shift; fi
    shift
  done
  [ -n "$filter" ] || { echo "stub gh: no --jq" >&2; exit 98; }
  jq -r "$filter" "$STUB_ISSUES_JSON"
  exit 0
fi
if [ "$1" = issue ] && { [ "$2" = comment ] || [ "$2" = create ]; }; then
  body=""
  while [ $# -gt 0 ]; do
    if [ "$1" = --body-file ]; then body=$2; shift; fi
    shift
  done
  [ -n "$body" ] || { echo "stub gh: no --body-file" >&2; exit 98; }
  cp "$body" "$STUB_LOG.body"
  exit 0
fi
echo "stub gh: unsupported invocation: $*" >&2
exit 98
"""


class Harness:
    """One scratch directory with stubs on PATH and an allowlisted env."""

    def __init__(self, root: pathlib.Path) -> None:
        self.root = root
        self.bin = root / "bin"
        self.bin.mkdir()
        # The body invokes `python3` by name; pin it to the interpreter
        # running this test, so a version-manager shim that reads config
        # from the scratch HOME cannot pick a different one.
        launcher = f'#!/usr/bin/env bash\nexec {shlex.quote(sys.executable)} "$@"\n'
        for name, text in (("cargo", STUB_CARGO), ("gh", STUB_GH), ("python3", launcher)):
            path = self.bin / name
            path.write_text(text)
            path.chmod(path.stat().st_mode | stat.S_IXUSR)
        (root / "utils").mkdir()
        shutil.copy(REPORT_SCRIPT, root / "utils" / "mutation-report.py")
        self.log = root / "stub.log"
        self.github_output = root / "github_output"
        self.runner_temp = root / "runner_temp"
        self.runner_temp.mkdir()
        self.issues_json = root / "issues.json"
        self.issues_json.write_text("[]")

    def env(self, **extra: str) -> dict[str, str]:
        base = {
            "PATH": f"{self.bin}{os.pathsep}{os.environ.get('PATH') or os.defpath}",
            "HOME": str(self.root),
            "LANG": "C.UTF-8",
            "STUB_LOG": str(self.log),
            "STUB_ISSUES_JSON": str(self.issues_json),
            "GITHUB_OUTPUT": str(self.github_output),
            "RUNNER_TEMP": str(self.runner_temp),
        }
        base.update(extra)
        return base

    def run(self, body: str, **extra: str) -> subprocess.CompletedProcess[str]:
        script = (
            "[ \"$(command -v cargo)\" = \"$STUB_BIN/cargo\" ] || { echo 'harness: cargo is not the stub' >&2; exit 96; }\n"
            "[ \"$(command -v gh)\" = \"$STUB_BIN/gh\" ] || { echo 'harness: gh is not the stub' >&2; exit 96; }\n"
            "[ \"$(command -v python3)\" = \"$STUB_BIN/python3\" ] || { echo 'harness: python3 is not the launcher' >&2; exit 96; }\n"
            + body
        )
        return subprocess.run(
            ["bash", "-e", "-c", script],
            cwd=self.root,
            env=self.env(STUB_BIN=str(self.bin), **extra),
            capture_output=True,
            text=True,
            timeout=TIMEOUT,
            check=False,
        )

    def outputs(self) -> dict[str, str]:
        if not self.github_output.is_file():
            return {}
        pairs = (line.split("=", 1) for line in self.github_output.read_text().splitlines() if "=" in line)
        return dict(pairs)

    def calls(self) -> list[list[str]]:
        """Every stub invocation as its exact argv."""
        if not self.log.is_file():
            return []
        calls: list[list[str]] = []
        current: list[str] | None = None
        for line in self.log.read_text().splitlines():
            if line == "<call>":
                current = []
            elif line == "</call>":
                assert current is not None
                calls.append(current)
                current = None
            elif current is not None:
                current.append(line)
        return calls


def _bodies() -> dict[str, str]:
    text = WORKFLOW.read_text()
    return {
        "prepare": run_body(text, "define"),
        "mutants": run_body(text, "Run cargo-mutants"),
        "aggregate": run_body(text, "Aggregate shard results"),
        "file": run_body(text, "File or update the escaped-mutants issue"),
    }


MUTANTS_ENV = {"SHARD_K": "3", "SHARD_N": "8", "RUN_ID": "4242", "RUN_ATTEMPT": "1"}


class ExtractorTests(unittest.TestCase):
    SAMPLE = textwrap.dedent(
        """\
        jobs:
          a:
            steps:
              # a comment before the first item
              - id: one
                run: |
                  first line

                  after a blank
                env:
                  X: y
              - name: two
                env:
                  Y: z
                run: |
                  cat <<'EOF'
                  steps:
                    - id: phantom
                      run: |
                        echo FAKE
                  run: |
                  - name: two
                  EOF
              - name: deeper
                run: |
                      four in, not two
                      cat <<'EOF'
                      EOF
              - name: ragged
                run: |
                    first at four
                  second at two
              - name: folded
                run: >
                  nope
              - name: dup
                run: |
                  a
              - name: dup
                run: |
                  b
        """
    )

    def test_body_keeps_inner_blank_lines_and_stops_at_sibling_key(self) -> None:
        self.assertEqual(run_body(self.SAMPLE, "one"), "first line\n\nafter a blank\n")

    def test_body_after_other_keys_and_containing_yaml_lookalikes(self) -> None:
        self.assertEqual(
            run_body(self.SAMPLE, "two"),
            "cat <<'EOF'\nsteps:\n  - id: phantom\n    run: |\n      echo FAKE\nrun: |\n- name: two\nEOF\n",
        )

    def test_a_step_inside_a_heredoc_is_text_not_a_step(self) -> None:
        with self.assertRaises(WorkflowShapeError):
            run_body(self.SAMPLE, "phantom")

    def test_block_indent_is_taken_from_its_first_line(self) -> None:
        # YAML takes the content indent from the first line; a heredoc
        # terminator would break if the extractor assumed key + 2.
        self.assertEqual(run_body(self.SAMPLE, "deeper"), "four in, not two\ncat <<'EOF'\nEOF\n")

    def test_a_line_shallower_than_the_block_is_refused(self) -> None:
        with self.assertRaises(WorkflowShapeError):
            run_body(self.SAMPLE, "ragged")

    def test_other_block_indicators_are_refused(self) -> None:
        with self.assertRaises(WorkflowShapeError):
            run_body(self.SAMPLE, "folded")

    def test_duplicate_step_names_are_refused(self) -> None:
        with self.assertRaises(WorkflowShapeError):
            run_body(self.SAMPLE, "dup")

    def test_unknown_step_is_refused(self) -> None:
        with self.assertRaises(WorkflowShapeError):
            run_body(self.SAMPLE, "absent")

    def test_the_real_workflow_yields_the_four_bodies(self) -> None:
        bodies = _bodies()
        self.assertIn("jq -n", bodies["prepare"])
        self.assertIn("cargo mutants", bodies["mutants"])
        self.assertIn("mutation-report.py summarize", bodies["mutants"])
        self.assertIn("mutation-report.py report", bodies["aggregate"])
        self.assertIn("gh issue list", bodies["file"])
        for name, body in bodies.items():
            self.assertNotIn("${{", body, f"{name}: an Actions expression inside a run: body")
            self.assertIn("set -euo pipefail", body, f"{name}: missing the strict-mode line")


@unittest.skipUnless(HAVE_BASH, "the bodies run under bash")
class HarnessCase(unittest.TestCase):
    bodies: dict[str, str]

    @classmethod
    def setUpClass(cls) -> None:
        cls.bodies = _bodies()

    def setUp(self) -> None:
        self.fresh()

    def fresh(self) -> None:
        """A new scratch directory; the previous one is released first."""
        if hasattr(self, "_tmp"):
            self._tmp.cleanup()
        self._tmp = tempfile.TemporaryDirectory(prefix="mutation-workflow-")
        self.addCleanup(self._tmp.cleanup)
        self.h = Harness(pathlib.Path(self._tmp.name))

    def summary_path(self, k: int = 3) -> pathlib.Path:
        return self.h.root / "target" / "mutants" / f"shard-summary-{k}.json"


class PrepareTests(HarnessCase):
    @needs_jq
    def test_shard_list_and_count_reach_github_output(self) -> None:
        r = self.h.run(self.bodies["prepare"])
        self.assertEqual(r.returncode, 0, r.stderr)
        out = self.h.outputs()
        self.assertEqual(json.loads(out["shards"]), [0, 1, 2, 3, 4, 5, 6, 7])
        self.assertEqual(out["count"], str(len(json.loads(out["shards"]))))


class MutantsStepTests(HarnessCase):
    def run_mutants(self, body: str | None = None, **stub: str) -> subprocess.CompletedProcess[str]:
        return self.h.run(body or self.bodies["mutants"], **MUTANTS_ENV, **stub)

    def test_step_exit_follows_cargo_mutants_and_summary_is_complete(self) -> None:
        cases = {
            0: dict(STUB_CAUGHT="src/a.rs:1:1: x"),
            2: dict(STUB_CAUGHT="src/a.rs:1:1: x", STUB_MISSED="src/b.rs:2:2: y"),
            3: dict(STUB_CAUGHT="src/a.rs:1:1: x", STUB_TIMEOUT="src/c.rs:3:3: z"),
            4: dict(),
            1: dict(),
        }
        for code, files in cases.items():
            with self.subTest(cargo_exit=code):
                self.fresh()
                r = self.run_mutants(STUB_CARGO_EXIT=str(code), **files)
                self.assertEqual(r.returncode, code, r.stderr)
                summary = json.loads(self.summary_path().read_text())
                self.assertEqual(summary["exit_code"], code)
                self.assertEqual((summary["shard"], summary["shard_count"]), (3, 8))
                self.assertEqual((summary["run_id"], summary["run_attempt"]), ("4242", "1"))
                self.assertEqual(summary["tool_version"], "cargo-mutants 27.1.0")
                self.assertEqual(summary["caught"], 1 if "STUB_CAUGHT" in files else 0)
                self.assertEqual(len(summary["missed"]), 1 if "STUB_MISSED" in files else 0)
                self.assertEqual(len(summary["timeout"]), 1 if "STUB_TIMEOUT" in files else 0)

    def test_cargo_mutants_receives_the_shard_and_strategy(self) -> None:
        self.run_mutants(STUB_CAUGHT="src/a.rs:1:1: x")
        main_calls = [c for c in self.h.calls() if c[:2] == ["mutants", "--package"]]
        self.assertEqual(len(main_calls), 1, self.h.calls())
        argv = main_calls[0]
        self.assertEqual(argv.count("--package"), 2)
        self.assertIn("big-code-analysis", argv)
        self.assertIn("big-code-analysis-ast", argv)
        self.assertIn("--in-place", argv)
        # The #1710 glob: whole-path matched, so it must reach the
        # subdirectories, and must arrive as one unexpanded argument.
        self.assertIn("src/metrics/**/*.rs", argv)
        self.assertIn("big-code-analysis-ast/src/checker.rs", argv)
        self.assertIn("big-code-analysis-ast/src/getter.rs", argv)
        self.assertEqual(argv[argv.index("--shard") + 1], "3/8")
        self.assertEqual(argv[argv.index("--sharding") + 1], "round-robin")
        self.assertNotIn("-j", argv)
        self.assertNotIn("--jobs", argv)

    def test_a_failing_version_command_stops_before_the_run(self) -> None:
        r = self.run_mutants(STUB_VERSION_EXIT="1", STUB_CAUGHT="src/a.rs:1:1: x")
        self.assertNotEqual(r.returncode, 0)
        self.assertFalse(self.summary_path().exists())
        self.assertEqual(
            [c for c in self.h.calls() if c[:2] == ["mutants", "--package"]],
            [],
            "the long run must not start when the version read failed",
        )

    def test_without_pipefail_the_masked_status_is_refused(self) -> None:
        # The #1710 defect. Weaken only pipefail: tee's 0 then stands for
        # the pipeline, `|| status=$?` never fires, and summarize is
        # handed exit 0 beside a recorded escape.
        original = self.bodies["mutants"]
        anchor = "set -euo pipefail\n"
        self.assertEqual(original.count(anchor), 1)
        perturbed = original.replace(anchor, "set -eu\n")
        self.assertNotEqual(original, perturbed)

        r = self.run_mutants(STUB_CARGO_EXIT="2", STUB_CAUGHT="src/a.rs:1:1: x", STUB_MISSED="src/b.rs:2:2: y")
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertTrue(self.summary_path().exists())

        self.fresh()
        r = self.run_mutants(perturbed, STUB_CARGO_EXIT="2", STUB_CAUGHT="src/a.rs:1:1: x", STUB_MISSED="src/b.rs:2:2: y")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("exit status was masked", r.stderr)
        self.assertFalse(self.summary_path().exists())


class ReportJobTests(HarnessCase):
    N = 4
    REPORT_ENV = {
        "SHARD_N": "4",
        "RUN_URL": "https://example/run/4242",
        "MATRIX_RESULT": "success",
        "GH_TOKEN": "stub-token",
        "GH_REPO": "o/r",
        "ISSUE_TITLE": "Mutation testing: escaped mutants",
    }

    def write_summaries(self, escapes_in: set[int] = frozenset(), absent: set[int] = frozenset()) -> None:
        d = self.h.root / "summaries"
        d.mkdir(exist_ok=True)
        for k in range(self.N):
            if k in absent:
                continue
            missed = [f"src/f{k}.rs:1:1: m"] if k in escapes_in else []
            (d / f"shard-summary-{k}.json").write_text(
                json.dumps(
                    {
                        "shard": k,
                        "shard_count": self.N,
                        "run_id": "4242",
                        "run_attempt": "1",
                        "tool_version": "cargo-mutants 27.1.0",
                        "exit_code": 2 if missed else 0,
                        "missed": missed,
                        "timeout": [],
                        "caught": 5,
                        "unviable": 0,
                    }
                )
            )

    def aggregate(self, **extra: str) -> subprocess.CompletedProcess[str]:
        return self.h.run(self.bodies["aggregate"], **self.REPORT_ENV, **extra)

    def file_step(self, issues: list[dict]) -> subprocess.CompletedProcess[str]:
        self.h.issues_json.write_text(json.dumps(issues))
        return self.h.run(self.bodies["file"], **self.REPORT_ENV)

    def test_clean_run_reports_status_zero_and_writes_the_body(self) -> None:
        self.write_summaries()
        r = self.aggregate()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.h.outputs()["status"], "0")
        body = (self.h.runner_temp / "issue-body.md").read_text()
        self.assertIn(f"Shards complete: {self.N} of {self.N}", body)

    def test_escapes_fail_the_step_with_the_status_in_outputs(self) -> None:
        self.write_summaries(escapes_in={1, 2})
        r = self.aggregate()
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertEqual(self.h.outputs()["status"], "1")
        self.assertIn("Missed mutants: 2", (self.h.runner_temp / "issue-body.md").read_text())

    def test_an_absent_shard_fails_the_step(self) -> None:
        self.write_summaries(absent={3})
        r = self.aggregate()
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("- shard 3: no summary uploaded", (self.h.runner_temp / "issue-body.md").read_text())

    def test_no_summaries_directory_at_all_still_reports(self) -> None:
        r = self.aggregate()
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn(f"Shards complete: 0 of {self.N}", (self.h.runner_temp / "issue-body.md").read_text())

    @needs_jq
    def test_an_open_issue_by_title_gets_a_comment(self) -> None:
        (self.h.runner_temp / "issue-body.md").write_text("report body\n")
        r = self.file_step(
            [
                {"number": 7, "title": "Mutation testing: something else"},
                {"number": 9, "title": "Mutation testing: escaped mutants"},
                {"number": 11, "title": "Mutation testing: escaped mutants"},
            ]
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        calls = self.h.calls()
        self.assertEqual(len(calls), 2, calls)
        # The whole lookup as argv, not a prefix of a flattened string:
        # `--limit 100` is the one argument that exists for a tracker
        # with more open issues than gh's default page, and the title
        # must reach gh as one argument.
        self.assertEqual(
            calls[0],
            [
                "issue", "list", "--state", "open", "--label", "mutation-testing",
                "--limit", "100", "--json", "number,title",
                "--jq", "first(.[] | select(.title == env.ISSUE_TITLE) | .number) // empty",
            ],
        )
        self.assertEqual(calls[1][:3], ["issue", "comment", "9"])
        self.assertIn("--body-file", calls[1])
        self.assertEqual((self.h.log.with_suffix(".log.body")).read_text(), "report body\n")

    @needs_jq
    def test_a_missing_body_file_fails_the_issue_step(self) -> None:
        # Nothing wrote `issue-body.md`; the stub gh refuses as real gh
        # would, and the step must not read as having filed something.
        # The lookup must have succeeded first, so the failure is the
        # create call's and not an earlier one's.
        r = self.file_step([])
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("issue-body.md", r.stderr, "the create call's cp must name the missing file")
        calls = self.h.calls()
        self.assertEqual(len(calls), 2, calls)
        self.assertEqual(calls[0][:2], ["issue", "list"])
        self.assertEqual(calls[1][:2], ["issue", "create"])
        self.assertIn("--body-file", calls[1])
        self.assertFalse(self.h.log.with_suffix(".log.body").exists())

    @needs_jq
    def test_no_open_issue_by_title_creates_one(self) -> None:
        (self.h.runner_temp / "issue-body.md").write_text("report body\n")
        r = self.file_step([{"number": 7, "title": "Mutation testing: something else"}])
        self.assertEqual(r.returncode, 0, r.stderr)
        calls = self.h.calls()
        self.assertEqual(len(calls), 2, calls)
        create = calls[1]
        self.assertEqual(create[:2], ["issue", "create"])
        self.assertEqual(create[create.index("--title") + 1], "Mutation testing: escaped mutants")
        self.assertEqual(create.count("--label"), 2)
        self.assertIn("mutation-testing", create)
        self.assertIn("--body-file", create)
        self.assertEqual((self.h.log.with_suffix(".log.body")).read_text(), "report body\n")


if __name__ == "__main__":
    unittest.main()
