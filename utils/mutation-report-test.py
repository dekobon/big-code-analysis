#!/usr/bin/env python3
"""Self-tests for mutation-report.py: the summarize-side contradictions
and every report-side shard state, on synthetic result trees, so a
regression here fails on a pull request rather than once a quarter.

Run: ``python3 -m unittest -q utils/mutation-report-test.py``.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import pathlib
import sys
import tempfile
import unittest

UTILS_DIR = pathlib.Path(__file__).resolve().parent


def _load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, UTILS_DIR / filename)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


mr = _load("mutation_report", "mutation-report.py")

N = 4


def _mutants_out(root: pathlib.Path, **files: list[str]) -> pathlib.Path:
    out = root / "mutants.out"
    out.mkdir(parents=True, exist_ok=True)
    for name, lines in files.items():
        (out / f"{name}.txt").write_text("".join(f"{line}\n" for line in lines))
    return out


def _summary(
    shard: int,
    *,
    exit_code: int = 0,
    missed: list[str] | None = None,
    timeout: list[str] | None = None,
    attempt: str = "1",
    shard_count: int = N,
) -> dict:
    return {
        "shard": shard,
        "shard_count": shard_count,
        "run_id": "1",
        "run_attempt": attempt,
        "tool_version": "cargo-mutants 27.1.0",
        "exit_code": exit_code,
        "missed": missed or [],
        "timeout": timeout or [],
        "caught": 7,
        "unviable": 1,
    }


class SummarizeTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _run(self, exit_code: int, **files: list[str]) -> dict:
        out = _mutants_out(self.root, **files)
        return mr.summarize(
            out,
            exit_code=exit_code,
            shard=2,
            shard_count=N,
            run_id="99",
            run_attempt="1",
            tool_version="cargo-mutants 27.1.0",
        )

    def test_clean_shard_records_counts(self) -> None:
        s = self._run(0, caught=["a:1:1: x", "b:2:2: y"], unviable=["c:3:3: z"])
        self.assertEqual((s["exit_code"], s["caught"], s["unviable"]), (0, 2, 1))
        self.assertEqual((s["missed"], s["timeout"]), ([], []))
        self.assertEqual((s["shard"], s["shard_count"], s["run_id"]), (2, N, "99"))

    def test_missed_and_timeout_lines_are_carried_verbatim(self) -> None:
        s = self._run(3, missed=["m:1:1: p"], timeout=["t:2:2: q"], caught=["c:3:3: r"])
        self.assertEqual(s["missed"], ["m:1:1: p"])
        self.assertEqual(s["timeout"], ["t:2:2: q"])

    def test_blank_lines_in_result_files_are_not_mutants(self) -> None:
        s = self._run(2, missed=["m:1:1: p", "", "  "])
        self.assertEqual(s["missed"], ["m:1:1: p"])

    def test_missing_mutants_out_is_an_empty_infra_summary(self) -> None:
        s = mr.summarize(
            self.root / "never-created",
            exit_code=1,
            shard=0,
            shard_count=N,
            run_id="",
            run_attempt="",
            tool_version="",
        )
        self.assertEqual(s["exit_code"], 1)
        self.assertEqual((s["missed"], s["caught"]), ([], 0))

    def test_exit_zero_with_recorded_escapes_is_refused(self) -> None:
        with self.assertRaises(SystemExit) as cm:
            self._run(0, missed=["m:1:1: p"])
        self.assertIn("masked", str(cm.exception))

    def test_exit_two_without_missed_is_refused(self) -> None:
        with self.assertRaises(SystemExit) as cm:
            self._run(2, caught=["c:1:1: r"])
        self.assertIn("missed.txt", str(cm.exception))

    def test_exit_three_without_timeout_is_refused(self) -> None:
        with self.assertRaises(SystemExit) as cm:
            self._run(3, missed=["m:1:1: p"])
        self.assertIn("timeout.txt", str(cm.exception))

    def test_exit_two_beside_timeouts_is_refused(self) -> None:
        # cargo-mutants reports any timeout as 3, so 2 + timeouts is not its.
        with self.assertRaises(SystemExit) as cm:
            self._run(2, missed=["m:1:1: p"], timeout=["t:2:2: q"])
        self.assertIn("reports as 3", str(cm.exception))

    def test_exit_three_with_both_missed_and_timeouts_is_accepted(self) -> None:
        s = self._run(3, missed=["m:1:1: p"], timeout=["t:2:2: q"])
        self.assertEqual((len(s["missed"]), len(s["timeout"])), (1, 1))

    def test_a_shard_that_tested_no_mutants_is_refused(self) -> None:
        # The #1710 shape: a selection matching nothing exits 0 with four
        # empty files, which must not read as a clean shard.
        with self.assertRaises(SystemExit) as cm:
            self._run(0, caught=[], missed=[], timeout=[], unviable=[])
        self.assertIn("tested no mutants", str(cm.exception))

    def test_a_shard_with_only_unviable_mutants_is_not_empty(self) -> None:
        s = self._run(0, unviable=["u:1:1: v"])
        self.assertEqual((s["caught"], s["unviable"]), (0, 1))

    def test_baseline_failure_is_recorded_not_refused(self) -> None:
        # Exit 4 with nothing recorded is the honest baseline-failed shape,
        # not the empty-selection one.
        s = self._run(4)
        self.assertEqual(s["exit_code"], 4)

    def test_non_utf8_bytes_in_a_result_line_do_not_lose_the_shard(self) -> None:
        out = _mutants_out(self.root, caught=["c:1:1: r"])
        (out / "missed.txt").write_bytes(b"src/x.rs:1:1: replace \xe9 with 0\n")
        s = mr.summarize(
            out,
            exit_code=2,
            shard=0,
            shard_count=N,
            run_id="",
            run_attempt="",
            tool_version="",
        )
        self.assertEqual(len(s["missed"]), 1)
        self.assertTrue(s["missed"][0].startswith("src/x.rs:1:1: replace "))


class ReportTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def _write(self, summaries: dict[int, dict | str]) -> None:
        for k, s in summaries.items():
            path = self.dir / f"{mr.SUMMARY_PREFIX}{k}.json"
            path.write_text(s if isinstance(s, str) else json.dumps(s))

    def _report(self, matrix_result: str = "success") -> tuple[str, int]:
        return mr.report(
            self.dir,
            shard_count=N,
            run_url="https://example/run/1",
            matrix_result=matrix_result,
        )

    def _all_clean(self) -> dict[int, dict]:
        return {k: _summary(k) for k in range(N)}

    def test_all_shards_complete_and_clean_passes(self) -> None:
        self._write(self._all_clean())
        body, status = self._report()
        self.assertEqual(status, 0)
        self.assertIn(f"Shards complete: {N} of {N}", body)
        self.assertIn("Missed mutants: 0", body)
        self.assertIn(f"Caught: {7 * N}; unviable: {1 * N}", body)
        self.assertNotIn("did not complete", body)

    def test_body_says_what_a_missed_mutant_is_and_how_to_close(self) -> None:
        # The report is the only thing a triager reads; a data dump with
        # no framing is what the first real run filed (#1736).
        s = self._all_clean()
        s[1] = _summary(1, exit_code=2, missed=["src/abc/c.rs:1:1: x", "src/abc/c.rs:2:2: y", "src/wmc.rs:3:3: z"])
        self._write(s)
        body, _ = self._report()
        self.assertIn("no test's expected value depends on that line as written", body)
        self.assertIn("**To resolve this issue**", body)
        self.assertIn("`exclude_re` entry in `.cargo/mutants.toml`", body)
        # The close condition must not promise a comment a clean run never
        # posts: the workflow's issue step is gated on a non-zero status.
        self.assertIn("a clean run files nothing", body)
        self.assertIn("every shard completed reports zero missed and zero timed out", body)
        self.assertIn("Unviable mutants did not compile and are not work", body)
        # Equivalence is behavioural on every input, not "the default
        # matched this fixture"; that case is the one that needs a test.
        self.assertIn("identically on every reachable input, side effects and panics included", body)
        self.assertNotIn("happens to equal", body)
        self.assertNotIn("compiler proves unused", body)
        # The `+=` remedy names the operand that discriminates.
        self.assertIn("non-zero amount being merged in", body)
        # The repro command names the worst file, so the first step is one line.
        self.assertIn("cargo mutants --in-place -f src/abc/c.rs", body)

    def test_repro_command_names_the_package_for_a_file_outside_the_root_crate(self) -> None:
        s = self._all_clean()
        s[0] = _summary(0, exit_code=2, missed=["big-code-analysis-ast/src/checker.rs:1:1: x"])
        self._write(s)
        body, _ = self._report()
        self.assertIn(
            "cargo mutants --in-place --package big-code-analysis-ast -f big-code-analysis-ast/src/checker.rs",
            body,
        )
        self.assertNotIn("outside the crates this report knows", body)

    def test_repro_command_does_not_guess_a_package_for_an_unknown_path(self) -> None:
        # A directory name is not a crate name; a guessed `--package` the
        # command rejects is worse than none plus a note.
        s = self._all_clean()
        s[0] = _summary(0, exit_code=2, missed=["tools/gen/thing.rs:1:1: x"])
        self._write(s)
        body, _ = self._report()
        self.assertIn("cargo mutants --in-place -f tools/gen/thing.rs", body)
        self.assertNotIn("--package tools", body)
        self.assertIn("`tools/gen/thing.rs` is outside the crates this report knows", body)

    def test_top_files_break_ties_by_path_not_shard_order(self) -> None:
        s = self._all_clean()
        # shard 0 lists z.rs first, shard 1 lists a.rs first; counts tie.
        s[0] = _summary(0, exit_code=2, missed=["src/z.rs:1:1: m", "src/a.rs:1:1: m"])
        s[1] = _summary(1, exit_code=2, missed=["src/a.rs:2:2: m", "src/z.rs:2:2: m"])
        self._write(s)
        body, _ = self._report()
        a = body.index("- `src/a.rs`: 2")
        z = body.index("- `src/z.rs`: 2")
        self.assertLess(a, z)
        self.assertIn("cargo mutants --in-place -f src/a.rs", body)
        self.assertIn("Read a line as `file:line:col:", body)
        self.assertIn("mutants.out/missed.txt", body)

    def test_escapes_are_summed_across_shards_and_grouped_by_file(self) -> None:
        s = self._all_clean()
        s[1] = _summary(1, exit_code=2, missed=["src/a.rs:1:1: x", "src/a.rs:2:2: y"])
        s[3] = _summary(3, exit_code=3, missed=["src/b.rs:3:3: z"], timeout=["src/a.rs:4:4: w"])
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn("Missed mutants: 3", body)
        self.assertIn("Timed-out mutants: 1", body)
        self.assertIn("| 1 | 1 | 2 | 7 | 2 | 0 | 1 |", body)
        self.assertIn("| 3 | 1 | 3 | 7 | 1 | 1 | 1 |", body)
        self.assertIn("- `src/a.rs`: 2", body)
        self.assertIn("- `src/b.rs`: 1", body)
        self.assertIn("## First 3 missed mutants", body)
        self.assertIn("src/b.rs:3:3: z", body)

    def test_missed_sample_is_capped(self) -> None:
        s = self._all_clean()
        s[0] = _summary(
            0,
            exit_code=2,
            missed=[f"src/f{i}.rs:1:1: m" for i in range(mr.MISSED_SAMPLE_LINES + 20)],
        )
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn(f"## First {mr.MISSED_SAMPLE_LINES} missed mutants", body)
        # Two fenced blocks now: the repro command under the top-files
        # section, then the sample. Select the sample by its heading.
        after_heading = body.split(f"## First {mr.MISSED_SAMPLE_LINES} missed mutants", 1)[1]
        fence = after_heading.split("```")
        self.assertGreaterEqual(len(fence), 3, "a fenced sample block follows the heading")
        sample = fence[1].strip().splitlines()
        self.assertEqual(len(sample), mr.MISSED_SAMPLE_LINES)
        self.assertEqual(sample[0], "src/f0.rs:1:1: m")
        self.assertEqual(sample[-1], f"src/f{mr.MISSED_SAMPLE_LINES - 1}.rs:1:1: m")
        # Only the top files are listed, whatever the file count.
        self.assertEqual(body.count("- `src/f"), mr.TOP_FILES)

    def test_missed_only_run_fails(self) -> None:
        s = self._all_clean()
        s[2] = _summary(2, exit_code=2, missed=["src/a.rs:1:1: x"])
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn("Missed mutants: 1", body)
        self.assertIn("Timed-out mutants: 0", body)

    def test_timeout_only_run_fails(self) -> None:
        s = self._all_clean()
        s[2] = _summary(2, exit_code=3, timeout=["src/a.rs:1:1: x"])
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn("Missed mutants: 0", body)
        self.assertIn("Timed-out mutants: 1", body)
        self.assertNotIn("missed mutants\n", body)
        # A timeout has its own route to resolution. The missed-mutant
        # route (a discriminating assertion, or equivalence) cannot kill a
        # mutant that never terminates, so the body must not fold the two.
        self.assertIn("A timed-out mutant is different", body)
        self.assertIn("a test that bounds the loop", body)
        self.assertIn("`non-terminating under mutation`", body)
        self.assertIn("**To resolve this issue**, every missed mutant needs", body)
        self.assertNotIn("every missed and timed-out mutant", body)

    def test_absent_shard_is_named_and_fails_even_when_others_are_clean(self) -> None:
        s = self._all_clean()
        del s[2]
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn(f"Shards complete: {N - 1} of {N}", body)
        self.assertIn("- shard 2: no summary uploaded", body)
        self.assertIn("Missed mutants: 0", body)

    def test_malformed_summary_is_named_and_fails(self) -> None:
        s: dict[int, dict | str] = dict(self._all_clean())
        s[1] = "{not json"
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn("- shard 1: summary unreadable", body)

    def test_summary_missing_a_field_is_named(self) -> None:
        s = self._all_clean()
        del s[3]["missed"]
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn("- shard 3: summary field 'missed' missing", body)

    def test_summary_for_the_wrong_partition_is_named(self) -> None:
        s = self._all_clean()
        s[0] = _summary(0, shard_count=N + 2)
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn(f"- shard 0: summary is for shard 0/{N + 2}, expected 0/{N}", body)

    def test_summary_for_another_shard_index_is_named(self) -> None:
        s = self._all_clean()
        s[1] = _summary(3)
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn(f"- shard 1: summary is for shard 3/{N}, expected 1/{N}", body)

    def test_summary_that_is_not_an_object_is_named(self) -> None:
        for literal in ("[]", '"x"', "1", "null"):
            with self.subTest(literal=literal):
                s: dict[int, dict | str] = dict(self._all_clean())
                s[2] = literal
                self._write(s)
                body, status = self._report()
                self.assertEqual(status, 1)
                self.assertIn("- shard 2: summary is not a JSON object", body)

    def test_booleans_do_not_pass_the_integer_fields(self) -> None:
        s = self._all_clean()
        s[0]["exit_code"] = False
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn("- shard 0: summary field 'exit_code' missing or not int", body)

    def test_non_string_result_entries_are_named_not_crashed(self) -> None:
        s = self._all_clean()
        s[1] = _summary(1, exit_code=2, missed=["src/a.rs:1:1: x"])
        s[1]["missed"] = [7]
        s[2] = _summary(2, exit_code=2, missed=["src/b.rs:1:1: y"])
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn("- shard 1: summary field 'missed' holds a non-string entry", body)
        # The malformed shard is excluded; the valid one still counts.
        self.assertIn("Missed mutants: 1", body)
        self.assertIn("src/b.rs:1:1: y", body)

    def test_baseline_failed_shard_is_excluded_and_named(self) -> None:
        s = self._all_clean()
        s[2] = _summary(2, exit_code=4, missed=["src/x.rs:1:1: should-not-count"])
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn("- shard 2: baseline build or test failed", body)
        self.assertIn("Missed mutants: 0", body)
        self.assertNotIn("should-not-count", body)

    def test_infra_exit_code_is_named(self) -> None:
        s = self._all_clean()
        s[1] = _summary(1, exit_code=70)
        self._write(s)
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn("- shard 1: cargo-mutants exited 70 before testing mutants", body)

    def test_no_shard_completed(self) -> None:
        body, status = self._report()
        self.assertEqual(status, 1)
        self.assertIn(f"Shards complete: 0 of {N}", body)
        for k in range(N):
            self.assertIn(f"- shard {k}: no summary uploaded", body)

    def test_matrix_failure_with_clean_summaries_fails_and_warns(self) -> None:
        self._write(self._all_clean())
        body, status = self._report(matrix_result="failure")
        self.assertEqual(status, 1)
        self.assertIn("finished `failure`", body)
        self.assertIn("earlier attempt's artifact", body)

    def test_matrix_failure_explained_by_escapes_does_not_warn(self) -> None:
        # The steady state: a shard with an escape exits 2, so the matrix
        # result is `failure` on every ordinary run. Not a stale artifact.
        s = self._all_clean()
        s[3] = _summary(3, exit_code=2, missed=["src/a.rs:1:1: x"])
        self._write(s)
        body, status = self._report(matrix_result="failure")
        self.assertEqual(status, 1)
        self.assertNotIn("earlier attempt's artifact", body)

    def test_matrix_cancelled_with_clean_summaries_still_warns(self) -> None:
        self._write(self._all_clean())
        body, status = self._report(matrix_result="cancelled")
        self.assertEqual(status, 1)
        self.assertIn("finished `cancelled`", body)

    def test_matrix_cancelled_with_escapes_does_not_call_them_clean(self) -> None:
        s = self._all_clean()
        s[0] = _summary(0, exit_code=2, missed=["src/a.rs:1:1: x"])
        self._write(s)
        body, _ = self._report(matrix_result="cancelled")
        self.assertIn("finished `cancelled`", body)
        self.assertNotIn("clean summary", body)

    def test_mixed_attempts_are_called_out(self) -> None:
        # Attempt 1 left shard 0 with escapes; attempt 2 re-ran it and
        # failed before uploading, so the old summary stands. Escapes
        # explain the `failure`, so the stale warning cannot fire; the
        # attempt mix is the only visible trace, and it must be named.
        s = self._all_clean()
        s[0] = _summary(0, exit_code=2, missed=["src/a.rs:1:1: x"], attempt="1")
        for k in range(1, N):
            s[k] = _summary(k, attempt="2")
        self._write(s)
        body, status = self._report(matrix_result="failure")
        self.assertEqual(status, 1)
        self.assertIn("run attempts 1, 2", body)
        self.assertIn("keeps its earlier attempt's summary", body)
        self.assertNotIn("standing in for one that failed", body)

    def test_single_attempt_has_no_attempt_note(self) -> None:
        self._write(self._all_clean())
        body, _ = self._report()
        self.assertNotIn("run attempts", body)

    def test_incomplete_shard_reason_sits_in_the_note_column(self) -> None:
        s = self._all_clean()
        del s[1]
        self._write(s)
        body, _ = self._report()
        self.assertIn("| 1 | | | | | | | no summary uploaded", body)

    def test_attempt_number_is_shown_per_shard(self) -> None:
        s = self._all_clean()
        s[3] = _summary(3, attempt="2")
        self._write(s)
        body, _ = self._report()
        self.assertIn("| 3 | 2 | 0 | 7 | 0 | 0 | 1 |", body)


class CliTests(unittest.TestCase):
    def test_round_trip_through_main(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            summaries = root / "summaries"
            summaries.mkdir()
            for k in range(N):
                out = _mutants_out(root / f"shard{k}", caught=["c:1:1: r"])
                with contextlib.redirect_stdout(io.StringIO()) as printed:
                    code = mr.main(
                        [
                            "summarize",
                            "--mutants-out", str(out),
                            "--exit-code", "0",
                            "--shard", str(k),
                            "--shard-count", str(N),
                            "--run-attempt", "1",
                            "--output", str(summaries / f"{mr.SUMMARY_PREFIX}{k}.json"),
                        ]
                    )
                self.assertEqual(code, 0)
                self.assertIn(f"shard {k}/{N} exit 0: 0 missed", printed.getvalue())
            body = root / "body.md"
            printed = io.StringIO()
            with contextlib.redirect_stdout(printed):
                code = mr.main(
                    [
                        "report",
                        "--summaries", str(summaries),
                        "--shard-count", str(N),
                        "--output", str(body),
                    ]
                )
            self.assertEqual(code, 0)
            self.assertIn(f"Shards complete: {N} of {N}", body.read_text())
            self.assertEqual(printed.getvalue(), body.read_text())

    def test_report_failure_propagates_through_main(self) -> None:
        # The workflow's verdict is this exit code and nothing else.
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            summaries = root / "summaries"
            summaries.mkdir()
            for k in range(N):
                s = _summary(k, exit_code=2 if k == 1 else 0, missed=["src/a.rs:1:1: x"] if k == 1 else None)
                (summaries / f"{mr.SUMMARY_PREFIX}{k}.json").write_text(json.dumps(s))
            with contextlib.redirect_stdout(io.StringIO()):
                code = mr.main(
                    [
                        "report",
                        "--summaries", str(summaries),
                        "--shard-count", str(N),
                        "--output", str(root / "body.md"),
                    ]
                )
            self.assertEqual(code, 1)
            self.assertIn("Missed mutants: 1", (root / "body.md").read_text())

    def test_summarize_refusal_exits_nonzero_through_main(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            out = _mutants_out(root, missed=["m:1:1: p"])
            with self.assertRaises(SystemExit) as cm:
                mr.main(
                    [
                        "summarize",
                        "--mutants-out", str(out),
                        "--exit-code", "0",
                        "--shard", "0",
                        "--shard-count", "1",
                        "--output", str(root / "s.json"),
                    ]
                )
            self.assertNotEqual(cm.exception.code, 0)


if __name__ == "__main__":
    unittest.main()
