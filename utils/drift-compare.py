#!/usr/bin/env python3
"""Compare two `drift-oracle.py` captures on key sets AND bytes.

A byte diff alone cannot see a file that one capture skipped; the
manifest's key set is compared first, and a key missing on either side
is a failure before any content is read. Exit 1 on any difference,
2 when the captures cannot be compared at all.

The view set is compared the same way. A capture made without
`--verbatim` (a base older than #1744) carries five CLI views and no
`verbatim` one; a view present on only one side is **refused**, not
reported as "0 differing" — a zero over an absent view is the silent
pass this driver exists to prevent. `--only-shared-views` opts into
comparing the intersection, and the summary then names every view it
left out as `views_uncompared`.

usage: drift-compare.py <base-out-dir> <head-out-dir> [--show N]
                        [--lang L ...] [--only-shared-views]

Self-tested by `utils/drift-compare-test.py` (`make drift-compare-test`).
Nothing lints `utils/*.py` today (#1477); the self-test is the only
gate.
"""

from __future__ import annotations

import argparse
import filecmp
import json
import pathlib
import sys


def m_inputs(m: dict) -> dict[str, str]:
    return m.get("inputs_sha256", {})


def m_views(m: dict) -> list[str]:
    """The views a capture carries. A manifest written before the
    `views` field existed carried every CLI view and always the
    verbatim one."""
    if "views" in m:
        return list(m["views"])
    return [*m["cli_cmds"], *m["metrics_options"], "verbatim"]


def suffix(view: str) -> str:
    return ".verbatim.jsonl" if view == "verbatim" else f".{view}.txt"


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("base", type=pathlib.Path)
    ap.add_argument("head", type=pathlib.Path)
    ap.add_argument("--show", type=int, default=20, help="list up to N differing outputs")
    ap.add_argument("--lang", action="append", default=[], help="compare only these populations (a slice capture)")
    ap.add_argument(
        "--only-shared-views",
        action="store_true",
        help="compare the views both sides carry and name the rest, instead of refusing",
    )
    return ap.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    mb = json.loads((args.base / "manifest.json").read_text())
    mh = json.loads((args.head / "manifest.json").read_text())
    kb, kh = set(mb["keys"]), set(mh["keys"])
    if args.lang:
        langs = set(args.lang)
        kb = {k for k in kb if k.split("/", 1)[0] in langs}
        kh = {k for k in kh if k.split("/", 1)[0] in langs}
        if not kb or not kh:
            print(f"no keys for {sorted(langs)} on one side; nothing compared")
            return 2
    problems = 0
    for side, m in (("base", mb), ("head", mh)):
        if not m.get("complete", False):
            print(f"{side} capture is not complete ({m.get('failed_executions', '?')} failed executions); refusing to compare")
            return 2
    ib, ih = m_inputs(mb), m_inputs(mh)
    changed = sorted(k for k in ib.keys() & ih.keys() if ib[k] != ih[k])
    if changed:
        for k in changed[:10]:
            print(f"INPUT changed between captures: {k}")
        print(f"{len(changed)} input file(s) differ by content; the captures are not of the same inputs")
        return 2
    if mb.get("grammar_pins") != mh.get("grammar_pins"):
        print("grammar pins differ between captures")
        return 2
    vb, vh = m_views(mb), m_views(mh)
    one_sided = sorted(set(vb) ^ set(vh))
    shared = [v for v in vb if v in vh]
    if not shared:
        print(f"no view on both sides (base={vb}, head={vh}); nothing to compare")
        return 2
    if one_sided:
        for v in one_sided:
            print(f"VIEW only in {'base' if v in vb else 'head'}: {v}")
        if not args.only_shared_views:
            print(
                f"{len(one_sided)} view(s) exist on one side only; a zero over them would mean nothing. "
                "Pass --only-shared-views to compare the rest and have the omission named in the summary."
            )
            return 2
    if kb != kh:
        for k in sorted(kb - kh):
            print(f"KEY only in base: {k}")
        for k in sorted(kh - kb):
            print(f"KEY only in head: {k}")
        problems += len(kb ^ kh)
    if mb["metrics_options"] != mh["metrics_options"] or mb["cli_cmds"] != mh["cli_cmds"]:
        print("option matrix or command set differs between captures")
        problems += 1
    compared = 0
    differing: list[str] = []
    for key in sorted(kb & kh):
        for view in shared:
            a = args.base / (key + suffix(view))
            b = args.head / (key + suffix(view))
            if not a.is_file() or not b.is_file():
                print(f"MISSING output: {key}{suffix(view)} (base={a.is_file()} head={b.is_file()})")
                problems += 1
                continue
            compared += 1
            if not filecmp.cmp(a, b, shallow=False):
                differing.append(key + suffix(view))
    for d in differing[: args.show]:
        print(f"DIFF {d}")
    print(
        json.dumps(
            {
                "populations_compared": sorted({k.split("/", 1)[0] for k in kb & kh}),
                "views_compared": shared,
                "views_uncompared": one_sided,
                "keys_base": len(kb),
                "keys_head": len(kh),
                "outputs_compared": compared,
                "differing": len(differing),
                "key_problems": problems,
                "base_bca": mb["bca"]["sha256"][:12],
                "head_bca": mh["bca"]["sha256"][:12],
            }
        )
    )
    return 1 if (differing or problems) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
