#!/usr/bin/env python3
"""Recovery-tree drift sweep (lesson 102; #1697 R07, landed by #1744).

Each fixture is truncated at every line boundary, at 16 evenly spaced
byte offsets, one byte after every operator punctuation character, and
at the start, end and end+1 of every word — so every keyword (`if`,
`else`, `elif`, `for`, `switch`, `catch`, `return`, …), a call's `(`, a
string's opening delimiter and a stranded operator are each reached.
Runs the CLI (`ops`, `metrics`, `find -t call`, `find -t string`,
`dump`) AND the verbatim library oracle (the only path reaching a node
that ends at EOF) through both revisions and counts differing outputs.

Repairs over the R06 driver: fixture roots are arguments and are
validated; a run that fails on BOTH sides is a failure, never "equal"
(the old tuple compare read a shared crash as parity); a signal kill is
classified apart from a non-zero exit; the cut set is recorded so the
sweep is reproducible.

usage: recovery-oracle.py --base-bca B --head-bca B --base-verbatim V
                          --head-verbatim V --root DIR ... [--jobs N]
                          [--record FILE]

The cut inputs live in a `mkdtemp` directory under `${TMPDIR:-/tmp}`
for the run and are removed after it; `--record` keeps the cut set so
the sweep is reproducible. Self-tested by
`utils/recovery-oracle-test.py` (`make recovery-oracle-test`), which
stubs all four binaries. Nothing lints `utils/*.py` today (#1477); the
self-test is the only gate.
"""
from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

LANGS = {
    ".java": ["java"], ".py": ["python"], ".cpp": ["cpp", "mozcpp", "ccomment", "preproc"],
    ".cc": ["cpp", "mozcpp"], ".hpp": ["cpp", "mozcpp"], ".hh": ["cpp", "mozcpp"],
    ".h": ["cpp", "mozcpp", "c", "ccomment", "preproc"], ".mjs": ["javascript", "mozjs"],
    ".pm": ["perl"], ".exs": ["elixir"], ".bash": ["bash"],
    ".c": ["c"], ".m": ["objc"], ".cs": ["csharp"], ".ex": ["elixir"], ".go": ["go"],
    ".groovy": ["groovy"], ".irule": ["irules"], ".js": ["javascript", "mozjs"],
    ".jsx": ["javascript"], ".kt": ["kotlin"], ".lua": ["lua"], ".pl": ["perl"],
    ".php": ["php"], ".rb": ["ruby"], ".rs": ["rust"], ".sh": ["bash"], ".tcl": ["tcl"],
    ".ts": ["typescript"], ".tsx": ["tsx"],
}
CLI = [["ops", "-O", "json"], ["metrics", "-O", "json"], ["find", "-t", "call"],
       ["find", "-t", "string"], ["dump"]]

FAILURES: list[tuple[list[str], str, str]] = []


def checked(argv: list[str]) -> tuple[int, str, str]:
    r = subprocess.run(argv, capture_output=True, text=True, errors="replace")
    if r.returncode < 0:
        FAILURES.append((argv, f"signal {-r.returncode}", r.stderr[-400:]))
    elif r.returncode != 0:
        FAILURES.append((argv, f"exit {r.returncode}", r.stderr[-400:]))
    return (r.returncode, r.stdout, r.stderr)


def cuts_for(data: bytes) -> list[int]:
    cuts: set[int] = set()
    pos = 0
    for line in data.splitlines(keepends=True):
        pos += len(line)
        cuts.add(pos - 1)
    cuts |= {len(data) * i // 16 for i in range(1, 16)}
    for i, b in enumerate(data):
        if chr(b) in "?:<>=!&|~([{\"'`$@#%^*+-/\\,;.":
            cuts.add(i + 1)
    for m in re.finditer(rb"[A-Za-z_][A-Za-z_0-9]*", data):
        cuts.add(m.start())
        cuts.add(m.end())
        cuts.add(m.end() + 1)
    return sorted(c for c in cuts if 0 < c < len(data))


def main(argv: list[str] | None = None) -> int:
    FAILURES.clear()
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--base-bca", required=True)
    ap.add_argument("--head-bca", required=True)
    ap.add_argument("--base-verbatim", required=True)
    ap.add_argument("--head-verbatim", required=True)
    ap.add_argument("--root", action="append", default=[], type=pathlib.Path)
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--record", type=pathlib.Path, help="write the cut set here")
    args = ap.parse_args(argv)
    for b in (args.base_bca, args.head_bca, args.base_verbatim, args.head_verbatim):
        if not pathlib.Path(b).is_file():
            sys.exit(f"missing binary {b}")
    if not args.root:
        sys.exit("pass at least one --root")
    for r in args.root:
        if not r.is_dir():
            sys.exit(f"missing fixture root {r}")

    def one(job: tuple[str, int, str, pathlib.Path]) -> tuple[int, list[tuple[str, int, str, str]]]:
        fixture, cut, lang, p = job
        diffs = []
        for argv in CLI:
            a = checked([args.base_bca, *argv, "--no-config", "--language", lang, "-p", str(p)])
            b = checked([args.head_bca, *argv, "--no-config", "--language", lang, "-p", str(p)])
            if a != b:
                diffs.append((fixture, cut, lang, " ".join(argv)))
        a = checked([args.base_verbatim, lang, str(p)])
        b = checked([args.head_verbatim, lang, str(p)])
        if a != b:
            diffs.append((fixture, cut, lang, "verbatim"))
        return len(CLI) + 1, diffs

    total = 0
    diffs: list[tuple[str, int, str, str]] = []
    cut_record: dict[str, list[int]] = {}
    with tempfile.TemporaryDirectory(prefix="bca-recovery-cuts.") as d:
        jobs = []
        for ri, root in enumerate(args.root):
            for fixture in sorted(f for f in root.iterdir() if f.is_file() and not f.is_symlink()):
                langs = LANGS.get(fixture.suffix)
                if not langs:
                    sys.stderr.write(f"skipping {fixture}: no language for {fixture.suffix!r}\n")
                    continue
                ident = f"{root.name}#{ri}/{fixture.name}"
                if ident in cut_record:
                    sys.exit(f"duplicate fixture identity {ident}")
                data = fixture.read_bytes()
                cuts = cuts_for(data)
                cut_record[ident] = cuts
                for cut in cuts:
                    p = pathlib.Path(d) / f"r{ri}-{fixture.stem}-cut{cut}{fixture.suffix}"
                    p.write_bytes(data[:cut])
                    for lang in langs:
                        jobs.append((ident, cut, lang, p))
        if not jobs:
            sys.exit("no fixtures matched a known extension")
        with ThreadPoolExecutor(max_workers=args.jobs) as ex:
            for n, ds in ex.map(one, jobs):
                total += n
                diffs.extend(ds)
    if args.record:
        args.record.write_text(json.dumps({
            "binaries": {k: v for k, v in vars(args).items() if k.endswith(("bca", "verbatim"))},
            "fixtures": len(cut_record), "cuts": cut_record,
            "total": total, "differing": diffs, "failures": [(a, h, e) for a, h, e in FAILURES],
        }, indent=0))
    print(f"recovery inputs x (lang, cmd): {total}; differing: {len(diffs)}; failed executions: {len(FAILURES)}")
    for dd in diffs[:40]:
        print("DIFF", dd)
    for argv, how, err in FAILURES[:20]:
        print("FAIL", how, " ".join(str(a) for a in argv), err.strip()[-200:])
    return 1 if (diffs or FAILURES) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
