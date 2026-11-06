"""`python -m unittest`, recording its outcome as data for `perturb.py`.

`perturb.py`'s unittest runner replaces `-m unittest` in a sweep's
command with `-m perturb_unittest_recorder` (this module, on
`PYTHONPATH`), so the run is unittest's own `main` with its own
argument parsing, output and exit status, plus one thing: when it ends,
the outcome is written as JSON to the path in `PERTURB_UNITTEST_RECORD`.

The record is what the sweep reads, never unittest's printed report. A
test can print anything — a ruled `FAIL:` header, a whole nested
unittest report, a line shaped like a progress verdict — to either
stream, and `-b` copies it into the report; no reading of the text can
tell that from unittest's own. The record holds `TestCase.id()` for
each failure, as the result object received it:

  {"tests_run": N,
   "stopped": <bool>,              the run was stopped early (`-c` and
                                   SIGINT, or fail-fast)
   "failed": [<id>, ...],          failures and errors, subtests under
                                   the test that holds them
   "unexpected": [<id>, ...],      unexpected successes
   "loader": [<id>, ...],          import or discovery failures
   "fixtures": [<id>, ...]}        setUpClass / setUpModule and their
                                   teardowns, under the class or module

No record means the run did not return normally — an import that killed
the loader, an exception escaping a test such as `KeyboardInterrupt`, a
crash — and `perturb.py` reports that as a harness error, as it does a
run that was stopped early.

Self-tested through `utils/perturb-test.py`. A sweep of this file with
the unittest runner perturbs the runner's own recorder as well as the
subject, since both import it from here: sweep it from a `git archive`
copy, or read a mutant that stops the record being written as the
harness error it then is.
"""

from __future__ import annotations

import json
import os
import sys
import unittest
from unittest.suite import _ErrorHolder  # type: ignore[attr-defined]

RECORD_ENV = "PERTURB_UNITTEST_RECORD"
# An import or discovery failure is reported as a synthetic test named for
# the loader, never a test of the module.
LOADER_PREFIXES = ("unittest.loader._FailedTest", "unittest.loader.ModuleImportFailure")


class RecordingResult(unittest.TextTestResult):
    """unittest's own text result, recording each outcome by id."""

    def __init__(self, *args: object, **kwargs: object) -> None:
        super().__init__(*args, **kwargs)  # type: ignore[arg-type]
        self.record: dict[str, list[str]] = {"failed": [], "unexpected": [], "loader": [], "fixtures": []}
        self.seen: set[str] = set()

    def _failed(self, test: unittest.TestCase) -> None:
        test_id = test.id()
        if isinstance(test, _ErrorHolder):
            # `setUpClass (m.T)`: a class or module fixture, no one test.
            name = test.description.split(" ", 1)
            bucket = "fixtures"
            test_id = name[1].strip("()") if len(name) == 2 else test.description
        elif test_id.startswith(LOADER_PREFIXES):
            bucket = "loader"
        else:
            bucket = "failed"
        # Once per id: a test's subtests each report the test. A set rather
        # than a list scan, which was quadratic past ~10 000 failures.
        if test_id not in self.seen:
            self.seen.add(test_id)
            self.record[bucket].append(test_id)

    def addFailure(self, test, err):  # noqa: N802 — unittest's API
        super().addFailure(test, err)
        self._failed(test)

    def addError(self, test, err):  # noqa: N802
        super().addError(test, err)
        self._failed(test)

    def addSubTest(self, test, subtest, err):  # noqa: N802
        super().addSubTest(test, subtest, err)
        if err is not None:
            self._failed(test)

    def addUnexpectedSuccess(self, test):  # noqa: N802
        super().addUnexpectedSuccess(test)
        self.record["unexpected"].append(test.id())


class RecordingRunner(unittest.TextTestRunner):
    resultclass = RecordingResult

    def run(self, test):
        """unittest's run, then the record — only when the run returned.

        `TextTestRunner.run` calls `stopTestRun` in a `finally`, so a
        `KeyboardInterrupt` or `SystemExit` escaping a test still reaches
        it with the outcomes so far; writing there certified a partial
        run. Written here, an exception leaves no record, which the sweep
        reports as a harness error. `-c` turns SIGINT into a graceful
        stop that does return: `shouldStop` marks it, and the record says
        so for the sweep to refuse."""
        result = super().run(test)
        path = os.environ.get(RECORD_ENV)
        if path:
            with open(path, "w", encoding="utf-8") as fh:
                json.dump({"tests_run": result.testsRun, "stopped": bool(result.shouldStop), **result.record}, fh)
        return result


def main() -> None:
    # As `python -m unittest` names itself in usage and help.
    sys.argv[0] = os.path.basename(sys.executable) + " -m unittest"
    unittest.main(module=None, testRunner=RecordingRunner)


if __name__ == "__main__":
    main()
