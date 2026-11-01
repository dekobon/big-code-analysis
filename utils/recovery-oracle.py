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
validated; every root is walked recursively (the fixture store this
landed, `control_flow/<lang>/`, nests one directory per language) with
symlinks skipped and logged; the extension table is derived from the
`mk_langs!` rows (`drift_common.langs_by_extension`), so a fixture
spelled with an extension the enum owns cannot be skipped; outputs are
compared as bytes; a run that fails on BOTH sides is a failure, never
"equal" (the old tuple compare read a shared crash as parity); a
signal kill is classified apart from a non-zero exit; the cut set is
recorded so the sweep is reproducible.

The sweep is quadratic in the fixture: about 0.48 cuts per byte, each
a near-full copy of the input, so a 100 KB fixture would mean ~48k
cuts and 2.4 GB of cut files. Each cut is therefore written inside its
job and deleted after it, and `--max-bytes` (default 16384) refuses a
larger fixture up front, naming the cost, rather than filling the
disk part-way through.

usage: recovery-oracle.py --base-bca B --head-bca B --base-verbatim V
                          --head-verbatim V --root DIR ... [--jobs N]
                          [--record FILE] [--max-bytes N]

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
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

UTILS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(UTILS_DIR))

import drift_common as dc  # noqa: E402

# dotted extension -> the slugs that read it, from the enum.
LANGS = dc.langs_by_extension()
CLI = [["ops", "-O", "json"], ["metrics", "-O", "json"], ["find", "-t", "call"],
       ["find", "-t", "string"], ["dump"]]
DEFAULT_MAX_BYTES = 16384

FAILURES: list[tuple[list[str], str, str]] = []
SKIPPED_LINKS: list[str] = []


def checked(argv: list[str]) -> tuple[int, bytes, bytes]:
    c = dc.capture(argv)
    if c.how is not None:
        FAILURES.append((argv, c.how, c.preview()))
    return (c.returncode, c.stdout, c.stderr)


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
    SKIPPED_LINKS.clear()
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--base-bca", required=True)
    ap.add_argument("--head-bca", required=True)
    ap.add_argument("--base-verbatim", required=True)
    ap.add_argument("--head-verbatim", required=True)
    ap.add_argument("--root", action="append", default=[], type=pathlib.Path)
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--record", type=pathlib.Path, help="write the cut set here")
    ap.add_argument(
        "--max-bytes",
        type=int,
        default=DEFAULT_MAX_BYTES,
        help="refuse a fixture larger than this: the cut count, and so the work, is quadratic in its size",
    )
    args = ap.parse_args(argv)
    for b in (args.base_bca, args.head_bca, args.base_verbatim, args.head_verbatim):
        if not pathlib.Path(b).is_file():
            sys.exit(f"missing binary {b}")
    if not args.root:
        sys.exit("pass at least one --root")
    for r in args.root:
        if not r.is_dir():
            sys.exit(f"missing fixture root {r}")

    def one(job: tuple[str, int, str, pathlib.Path, bytes, str]) -> tuple[int, list[tuple[str, int, str, str]]]:
        fixture, cut, lang, cut_path, data, suffix = job
        # One cut file per job, removed after it: with every cut written
        # up front the scratch directory is O(n²) in the fixture.
        p = cut_path.with_name(f"{cut_path.name}-{lang}{suffix}")
        p.write_bytes(data[:cut])
        diffs = []
        try:
            for cmd in CLI:
                a = checked([args.base_bca, *cmd, "--no-config", "--language", lang, "-p", str(p)])
                b = checked([args.head_bca, *cmd, "--no-config", "--language", lang, "-p", str(p)])
                if a != b:
                    diffs.append((fixture, cut, lang, " ".join(cmd)))
            a = checked([args.base_verbatim, lang, str(p)])
            b = checked([args.head_verbatim, lang, str(p)])
            if a != b:
                diffs.append((fixture, cut, lang, "verbatim"))
        finally:
            p.unlink()
        return len(CLI) + 1, diffs

    total = 0
    diffs: list[tuple[str, int, str, str]] = []
    cut_record: dict[str, list[int]] = {}
    with tempfile.TemporaryDirectory(prefix="bca-recovery-cuts.") as d:
        jobs = []
        for ri, root in enumerate(args.root):
            root = root.resolve()
            for fi, fixture in enumerate(dc.walk(root, tuple(LANGS), (".git",), SKIPPED_LINKS)):
                rel = fixture.relative_to(root)
                ident = f"{root.name}#{ri}/{rel}"
                if ident in cut_record:
                    sys.exit(f"duplicate fixture identity {ident}")
                size = fixture.stat().st_size
                if size > args.max_bytes:
                    sys.exit(
                        f"{fixture} is {size} bytes, over --max-bytes {args.max_bytes}: the sweep is "
                        f"quadratic (about {int(size * 0.48)} cuts of up to {size} bytes each, "
                        f"~{size * size * 0.48 / 2 / 1e6:.0f} MB of cut input); split the fixture or raise the limit"
                    )
                data = fixture.read_bytes()
                cuts = cuts_for(data)
                cut_record[ident] = cuts
                for cut in cuts:
                    # Numbered per fixture, not spelled from the relative
                    # path: `a/b.rs` and `a__b.rs` would otherwise share a
                    # scratch name, and two concurrent jobs would read and
                    # unlink each other's cut.
                    cut_path = pathlib.Path(d) / f"r{ri}-f{fi}-cut{cut}"
                    for lang in LANGS[fixture.suffix]:
                        jobs.append((ident, cut, lang, cut_path, data, fixture.suffix))
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
            "skipped_symlinks": SKIPPED_LINKS,
        }, indent=0))
    print(f"recovery inputs x (lang, cmd): {total}; differing: {len(diffs)}; failed executions: {len(FAILURES)}")
    if SKIPPED_LINKS:
        sys.stderr.write(f"skipped {len(SKIPPED_LINKS)} symlink(s)\n")
    for dd in diffs[:40]:
        print("DIFF", dd)
    for cmd, how, err in FAILURES[:20]:
        print("FAIL", how, " ".join(str(a) for a in cmd), err.strip()[-200:])
    return 1 if (diffs or FAILURES) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
