#!/usr/bin/env python3
"""mutation-report

The two file-to-file halves of the sharded mutation-testing workflow
(`.github/workflows/mutation-test.yml`, #1726), kept out of the YAML so
they are unit-tested on every pull request rather than once a quarter.

``summarize`` runs inside a shard job, after cargo-mutants returns. It
joins the exit status the shell captured to the result files under
``mutants.out/`` and writes one small ``shard-summary-<k>.json``. The
join happens here, where the two are guaranteed to describe the same
run: a shard that is cancelled, times out, or crashes never reaches this
step and so writes no summary, and the one contradiction that would
mean the status was masked (exit 0 beside recorded escapes) is refused
on the spot.

``report`` runs once, after the matrix. It reads whichever summaries
arrived, names every shard that did not produce a well-formed one,
sums the escapes across the shards that did, and renders the issue
body. Its exit status is the workflow's verdict: zero only when every
shard completed and none recorded an escape.

Nothing here talks to GitHub; the workflow's own shell step publishes
the rendered body.
"""

from __future__ import annotations

import argparse
import collections
import json
import pathlib
import sys

SUMMARY_PREFIX = "shard-summary-"
EXIT_SUCCESS = 0
EXIT_FOUND_PROBLEMS = 2
EXIT_TIMEOUT = 3
EXIT_BASELINE_FAILED = 4
COMPLETE_EXIT_CODES = frozenset({EXIT_SUCCESS, EXIT_FOUND_PROBLEMS, EXIT_TIMEOUT})
MISSED_SAMPLE_LINES = 50
TOP_FILES = 10


def _read_lines(path: pathlib.Path) -> list[str]:
    if not path.is_file():
        return []
    # cargo-mutants writes these from Rust `String`s, so they are UTF-8;
    # `replace` keeps a four-hour shard's results over one odd byte.
    text = path.read_text(encoding="utf-8", errors="replace")
    return [line for line in text.splitlines() if line.strip()]


def summarize(
    mutants_out: pathlib.Path,
    *,
    exit_code: int,
    shard: int,
    shard_count: int,
    run_id: str,
    run_attempt: str,
    tool_version: str,
) -> dict:
    missed = _read_lines(mutants_out / "missed.txt")
    timeout = _read_lines(mutants_out / "timeout.txt")
    caught = len(_read_lines(mutants_out / "caught.txt"))
    unviable = len(_read_lines(mutants_out / "unviable.txt"))
    # Each refusal is a combination cargo-mutants 27.1.0 cannot produce
    # (`LabOutcome::exit_code`: baseline failure 4, else timeout 3, else
    # missed 2, else 0), so seeing one means the status or the files
    # were not this run's. The last is the #1710 shape: a selection
    # that matches nothing exits 0 with four empty files.
    if exit_code == EXIT_SUCCESS and (missed or timeout):
        raise SystemExit(
            f"mutation-report: cargo-mutants exited 0 but recorded "
            f"{len(missed)} missed and {len(timeout)} timed-out mutants; "
            "the exit status was masked somewhere upstream"
        )
    if exit_code == EXIT_FOUND_PROBLEMS and not missed:
        raise SystemExit(
            "mutation-report: cargo-mutants exited 2 (missed) but missed.txt "
            "is empty or absent"
        )
    if exit_code == EXIT_FOUND_PROBLEMS and timeout:
        raise SystemExit(
            f"mutation-report: cargo-mutants exited 2 (missed) but recorded "
            f"{len(timeout)} timed-out mutants, which it reports as 3"
        )
    if exit_code == EXIT_TIMEOUT and not timeout:
        raise SystemExit(
            "mutation-report: cargo-mutants exited 3 (timeout) but timeout.txt "
            "is empty or absent"
        )
    if exit_code in COMPLETE_EXIT_CODES and not (caught or unviable or missed or timeout):
        raise SystemExit(
            f"mutation-report: cargo-mutants exited {exit_code} having tested no "
            "mutants; the selection matched nothing"
        )
    return {
        "shard": shard,
        "shard_count": shard_count,
        "run_id": run_id,
        "run_attempt": run_attempt,
        "tool_version": tool_version,
        "exit_code": exit_code,
        "missed": missed,
        "timeout": timeout,
        "caught": caught,
        "unviable": unviable,
    }


REQUIRED_KEYS = {
    "shard": int,
    "shard_count": int,
    "exit_code": int,
    "missed": list,
    "timeout": list,
    "caught": int,
    "unviable": int,
    "run_attempt": str,
}


def load_summary(path: pathlib.Path, *, shard: int, shard_count: int) -> dict | str:
    """Return the parsed summary, or a one-line reason it cannot be used."""
    if not path.is_file():
        return "no summary uploaded: the shard was cancelled, timed out or never ran"
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as err:
        return f"summary unreadable: {err}"
    if not isinstance(data, dict):
        return "summary is not a JSON object"
    # Exact types: `isinstance(True, int)` holds, and a list of anything
    # would reach `str.split` below.
    for key, kind in REQUIRED_KEYS.items():
        if type(data.get(key)) is not kind:
            return f"summary field {key!r} missing or not {kind.__name__}"
    for key in ("missed", "timeout"):
        if not all(type(line) is str for line in data[key]):
            return f"summary field {key!r} holds a non-string entry"
    if data["shard"] != shard or data["shard_count"] != shard_count:
        return (
            f"summary is for shard {data['shard']}/{data['shard_count']}, "
            f"expected {shard}/{shard_count}"
        )
    if data["exit_code"] == EXIT_BASELINE_FAILED:
        return "baseline build or test failed; its mutants were not judged"
    if data["exit_code"] not in COMPLETE_EXIT_CODES:
        return f"cargo-mutants exited {data['exit_code']} before testing mutants"
    return data


def report(
    summaries_dir: pathlib.Path,
    *,
    shard_count: int,
    run_url: str,
    matrix_result: str,
) -> tuple[str, int]:
    """Render the issue body; return it with the exit status for the run."""
    complete: dict[int, dict] = {}
    incomplete: dict[int, str] = {}
    for k in range(shard_count):
        loaded = load_summary(
            summaries_dir / f"{SUMMARY_PREFIX}{k}.json",
            shard=k,
            shard_count=shard_count,
        )
        if isinstance(loaded, dict):
            complete[k] = loaded
        else:
            incomplete[k] = loaded

    missed = [line for k in sorted(complete) for line in complete[k]["missed"]]
    timeout = [line for k in sorted(complete) for line in complete[k]["timeout"]]
    by_file = collections.Counter(line.split(":", 1)[0] for line in missed)

    lines = [
        "The scheduled `mutation-test.yml` run reported on its shards.",
        "",
        f"- Run: {run_url}",
        f"- Shards complete: {len(complete)} of {shard_count}",
        f"- Missed mutants: {len(missed)}",
        f"- Timed-out mutants: {len(timeout)}",
    ]
    # A shard that records an escape exits 2 or 3, so the matrix result
    # is `failure` on every ordinary run with escapes, and that failure
    # needs no comment. Two things do. A non-success result that no
    # summary accounts for (all clean, or cancelled) means a shard's
    # result is not this run's. And a mix of attempts means some shards
    # were re-run: a re-run that failed before uploading leaves its
    # earlier artifact standing, and nothing here can tell that from a
    # shard that was simply not re-run, so the reader is pointed at it.
    escapes_explain = matrix_result == "failure" and bool(missed or timeout)
    if matrix_result != "success" and not incomplete and not escapes_explain:
        lines.append(
            f"- The matrix job finished `{matrix_result}` although every shard "
            "has a usable summary, so a shard's result here may be an earlier "
            "attempt's artifact standing in for one that failed on re-run. "
            "Check each shard's attempt below."
        )
    attempts = {s["run_attempt"] for s in complete.values()}
    if len(attempts) > 1:
        lines.append(
            f"- Summaries come from run attempts {', '.join(sorted(attempts))}. A "
            "shard whose re-run failed before uploading keeps its earlier "
            "attempt's summary; compare the attempt column with the matrix."
        )
    lines += ["", "| Shard | Attempt | Exit | Caught | Missed | Timed out | Unviable | Note |"]
    lines.append("|---|---|---|---|---|---|---|---|")
    for k in range(shard_count):
        if k in complete:
            s = complete[k]
            lines.append(
                f"| {k} | {s['run_attempt']} | {s['exit_code']} | {s['caught']} | "
                f"{len(s['missed'])} | {len(s['timeout'])} | {s['unviable']} | |"
            )
        else:
            lines.append(f"| {k} | | | | | | | {incomplete[k]} |")
    if incomplete:
        lines += [
            "",
            "## Shards that did not complete",
            "",
        ]
        lines += [f"- shard {k}: {why}" for k, why in sorted(incomplete.items())]
        lines.append("")
        lines.append(
            "Their mutants are not counted above. Re-run the failed jobs; the "
            "completed shards' artifacts persist and the report re-aggregates."
        )
    if by_file:
        lines += ["", f"## Files with the most missed mutants (top {TOP_FILES})", ""]
        lines += [
            f"- `{path}`: {count}" for path, count in by_file.most_common(TOP_FILES)
        ]
    if missed:
        lines += [
            "",
            f"## First {min(MISSED_SAMPLE_LINES, len(missed))} missed mutants",
            "",
            "```",
            *missed[:MISSED_SAMPLE_LINES],
            "```",
        ]
    lines += [
        "",
        "Download the `cargo-mutants-report-<shard>` artifacts for each shard's "
        "full `mutants.out/`. See `docs/development/mutation_testing.md` for "
        "triage guidance.",
    ]
    body = "\n".join(lines) + "\n"
    failed = bool(incomplete) or bool(missed) or bool(timeout) or matrix_result != "success"
    return body, (1 if failed else 0)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="command", required=True)

    s = sub.add_parser("summarize", help="write one shard's summary JSON")
    s.add_argument("--mutants-out", type=pathlib.Path, required=True)
    s.add_argument("--exit-code", type=int, required=True)
    s.add_argument("--shard", type=int, required=True)
    s.add_argument("--shard-count", type=int, required=True)
    s.add_argument("--run-id", default="")
    s.add_argument("--run-attempt", default="")
    s.add_argument("--tool-version", default="")
    s.add_argument("--output", type=pathlib.Path, required=True)

    r = sub.add_parser("report", help="aggregate the summaries into an issue body")
    r.add_argument("--summaries", type=pathlib.Path, required=True)
    r.add_argument("--shard-count", type=int, required=True)
    r.add_argument("--run-url", default="")
    r.add_argument("--matrix-result", default="success")
    r.add_argument("--output", type=pathlib.Path, required=True)

    args = ap.parse_args(argv)
    if args.command == "summarize":
        summary = summarize(
            args.mutants_out,
            exit_code=args.exit_code,
            shard=args.shard,
            shard_count=args.shard_count,
            run_id=args.run_id,
            run_attempt=args.run_attempt,
            tool_version=args.tool_version,
        )
        args.output.write_text(json.dumps(summary, indent=1) + "\n", encoding="utf-8")
        print(
            f"mutation-report: shard {args.shard}/{args.shard_count} exit "
            f"{args.exit_code}: {len(summary['missed'])} missed, "
            f"{len(summary['timeout'])} timed out, {summary['caught']} caught"
        )
        return 0

    body, status = report(
        args.summaries,
        shard_count=args.shard_count,
        run_url=args.run_url,
        matrix_result=args.matrix_result,
    )
    args.output.write_text(body, encoding="utf-8")
    print(body, end="")
    return status


if __name__ == "__main__":
    sys.exit(main())
