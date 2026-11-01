#!/usr/bin/env python3
"""Zero-drift capture driver (#1697 R07, landed by #1744).

Per file and language, records the CLI's `ops`, `metrics` (under an
options matrix), `dump`, `find -t call|string|function` and `count`
output, and — when `--verbatim` names the `verbatim-oracle` binary
(`cargo build -p big-code-analysis-bench --bin verbatim-oracle`) — the
library-level verbatim record: metrics, ops, the Alterator-built AST
export, find/count, on the raw bytes, the only path that reaches a
node ending at EOF.

The verbatim view is optional per side because a base commit older
than #1744 has no `verbatim-oracle` to build: a base-vs-head run over
such a base compares the five CLI views only, and the verbatim view is
compared head-vs-head from the next change on. The manifest records
which views the capture carries (`views`), and `drift-compare.py`
refuses to report a view only one side has as "0 differing".

Repairs over the R06 driver (plan §5 Phase 0b):
- fixture roots are arguments and every one is validated BEFORE any
  filtering, so a missing root fails the run instead of shrinking it;
- the output key is `<language>/<root-id>/<relpath>`, so two fixture
  roots holding the same basename cannot overwrite each other and a
  file several populations read (`.h` under cpp/mozcpp/ccomment/preproc)
  is one key per language;
- symlinks are never followed (skipped and logged): a followed link
  would let a key escape its root;
- a process killed by a signal (returncode < 0) is classified apart
  from a non-zero exit; both fail the run;
- every subprocess call is an argv list, never a shell string;
- `metrics` runs under an options matrix (default, --exclude-tests,
  --cyclomatic-count-try=false) so the Rust `count_try` path and the
  prune path are captured;
- a manifest records binaries (sha256), roots, option matrix, grammar
  pins and the full key set, so two captures can be compared on keys
  as well as bytes (`compare.py`).

usage: drift-oracle.py --bca BIN [--verbatim BIN] --out DIR --root ID=PATH ...
                       [--corpus PATH] [--lang L ...] [--jobs N]
                       [--repo-toml Cargo.toml]

Self-tested by `utils/drift-oracle-test.py` (`make drift-oracle-test`),
which stubs both binaries. Nothing lints `utils/*.py` today (#1477);
the self-test is the only gate.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

# slug -> extensions. Mozcpp / Mozjs own none; they run over the Cpp /
# JS populations explicitly. Ccomment / Preproc own none either and run
# over C / C++ sources.
POPULATIONS = {
    "bash": (".sh", ".bash"),
    "c": (".c",),
    "cpp": (".cpp", ".cc", ".cxx", ".hpp", ".hh", ".h"),
    "mozcpp": (".cpp", ".cc", ".cxx", ".hpp", ".hh", ".h"),
    "ccomment": (".c", ".cpp", ".h"),
    "preproc": (".c", ".cpp", ".h"),
    "csharp": (".cs",),
    "elixir": (".ex", ".exs"),
    "go": (".go",),
    "groovy": (".groovy",),
    "irules": (".irule", ".irules"),
    "java": (".java",),
    "javascript": (".js", ".jsx", ".mjs"),
    "mozjs": (".js", ".jsx", ".mjs"),
    "kotlin": (".kt",),
    "lua": (".lua",),
    "objc": (".m",),
    "perl": (".pl", ".pm"),
    "php": (".php",),
    "python": (".py",),
    "ruby": (".rb",),
    "rust": (".rs",),
    "tcl": (".tcl",),
    "typescript": (".ts",),
    "tsx": (".tsx",),
}
assert len(POPULATIONS) == 25, len(POPULATIONS)

# The two helper grammars and Mozcpp have no corpus of their own and
# would walk the whole C/C++ population (1k+ files, three times); the
# fixtures are enough for them since every row kind is spelled there.
FIXTURE_ONLY = {"ccomment", "preproc"}

# `metrics` option matrix: name -> extra argv. The other commands take
# no metric-affecting options.
METRICS_OPTIONS = {
    "metrics": [],
    "metrics_exclude_tests": ["--exclude-tests"],
    "metrics_no_count_try": ["--cyclomatic-count-try=false"],
    "metrics_exclude_tests_no_count_try": ["--exclude-tests", "--cyclomatic-count-try=false"],
    "metrics_only_cyclomatic": ["--metrics", "cyclomatic"],
    "metrics_only_cognitive": ["--metrics", "cognitive"],
    "metrics_only_nexits": ["--metrics", "nexits"],
}

CLI_CMDS = {
    "ops": ["ops", "-O", "json"],
    "dump": ["dump"],
    "find_call": ["find", "-t", "call"],
    "find_string": ["find", "-t", "string"],
    "find_function": ["find", "-t", "function"],
    "count": ["count", "-t", "call", "-t", "string", "-t", "function"],
}


def sha256(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    h.update(path.read_bytes())
    return h.hexdigest()


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bca", required=True, type=pathlib.Path)
    ap.add_argument("--verbatim", type=pathlib.Path, help="verbatim-oracle binary; omit on a side that has none")
    ap.add_argument("--out", required=True, type=pathlib.Path)
    ap.add_argument("--root", action="append", default=[], metavar="ID=PATH", help="a fixture root; repeatable")
    ap.add_argument("--corpus", type=pathlib.Path, help="tests/repositories; omit for fixtures only")
    ap.add_argument("--lang", action="append", default=[], help="restrict to these populations")
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--repo-toml", help="root Cargo.toml, for the grammar pins in the manifest")
    return ap.parse_args(argv)


def resolve_roots(specs: list[str]) -> dict[str, pathlib.Path]:
    roots: dict[str, pathlib.Path] = {}
    for spec in specs:
        if "=" not in spec:
            sys.exit(f"--root takes ID=PATH, got {spec!r}")
        rid, path = spec.split("=", 1)
        if rid in roots:
            sys.exit(f"duplicate root id {rid!r}")
        if "/" in rid or rid in ("", ".", "..", "corpus"):
            sys.exit(f"root id {rid!r} must be a bare name and not '.', '..' or 'corpus'")
        p = pathlib.Path(path).resolve()
        if not p.is_dir():
            sys.exit(f"missing fixture root {rid}={p}")
        roots[rid] = p
    return roots


SKIPPED_LINKS: list[str] = []


def walk(root: pathlib.Path, exts: tuple[str, ...], skip_parts: tuple[str, ...]) -> list[pathlib.Path]:
    out: list[pathlib.Path] = []
    # `rglob` does not descend a symlinked directory, so one is skipped
    # silently; walk by hand so every link — file or directory — is logged.
    stack = [root]
    while stack:
        d = stack.pop()
        for p in sorted(d.iterdir()):
            # Relative to the root: a fixture root may itself live under
            # the snapshot submodule the corpus walk skips.
            if any(part in p.relative_to(root).parts for part in skip_parts):
                continue
            if p.is_symlink():
                SKIPPED_LINKS.append(str(p))
                continue
            if p.is_dir():
                stack.append(p)
            elif p.is_file() and p.suffix in exts:
                out.append(p)
    return sorted(out)


FAILURES: list[tuple[list[str], str, str]] = []


def run(argv: list[str]) -> str:
    r = subprocess.run(argv, capture_output=True, text=True, errors="replace")
    if r.returncode < 0:
        FAILURES.append((argv, f"signal {-r.returncode}", r.stderr[-400:]))
        return f"SIGNAL {-r.returncode}\n{r.stdout}\nSTDERR\n{r.stderr}"
    if r.returncode != 0:
        FAILURES.append((argv, f"exit {r.returncode}", r.stderr[-400:]))
        return f"EXIT {r.returncode}\n{r.stdout}\nSTDERR\n{r.stderr}"
    return r.stdout + ("\nSTDERR\n" + r.stderr if r.stderr else "")


def views(verbatim: bool) -> list[str]:
    """The per-file output names a capture carries, in the order
    `drift-compare.py` walks them."""
    return [*CLI_CMDS, *METRICS_OPTIONS, *(["verbatim"] if verbatim else [])]


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    SKIPPED_LINKS.clear()
    FAILURES.clear()
    for b in (args.bca, args.verbatim):
        if b is not None and not b.is_file():
            sys.exit(f"missing binary {b}")
    roots = resolve_roots(args.root)
    if args.corpus is not None:
        corpus = args.corpus.resolve()
        if not corpus.is_dir():
            sys.exit(f"missing corpus {corpus}")
        roots = {"corpus": corpus, **roots}
    if not roots:
        sys.exit("no roots: pass --corpus and/or --root")
    only = set(args.lang)
    unknown = only - POPULATIONS.keys()
    if unknown:
        sys.exit(f"unknown language(s) {sorted(unknown)}")

    # The corpus walk skips the snapshot submodule's own tree and git
    # metadata; a fixture root is walked whole.
    corpus_skip = ("big-code-analysis-output", ".git")
    jobs: list[tuple[str, str, pathlib.Path, pathlib.Path]] = []
    counts: dict[str, int] = {}
    keys: set[str] = set()
    inputs: dict[str, str] = {}
    out = args.out.resolve()
    destinations: set[pathlib.Path] = set()
    for lang, exts in POPULATIONS.items():
        if only and lang not in only:
            continue
        n = 0
        for rid, root in roots.items():
            if rid == "corpus" and lang in FIXTURE_ONLY:
                continue
            for p in walk(root, exts, corpus_skip if rid == "corpus" else (".git",)):
                key = f"{lang}/{rid}/{p.relative_to(root)}"
                if key in keys:
                    sys.exit(f"duplicate key {key}")
                dst = (out / key).resolve()
                if not dst.is_relative_to(out) or dst in destinations:
                    sys.exit(f"output destination for {key} escapes or collides: {dst}")
                destinations.add(dst)
                keys.add(key)
                inputs[key] = sha256(p)
                jobs.append((lang, key, root, p))
                n += 1
        if n == 0:
            sys.exit(f"population {lang} is empty")
        counts[lang] = n

    out.mkdir(parents=True, exist_ok=True)

    def one(job: tuple[str, str, pathlib.Path, pathlib.Path]) -> None:
        lang, key, _root, p = job
        dst = out / key
        dst.parent.mkdir(parents=True, exist_ok=True)
        base = [str(args.bca)]
        tail = ["--no-config", "--language", lang, "-p", str(p)]
        for name, argv in CLI_CMDS.items():
            dst.with_name(dst.name + f".{name}.txt").write_text(run([*base, *argv, *tail]))
        for name, extra in METRICS_OPTIONS.items():
            dst.with_name(dst.name + f".{name}.txt").write_text(
                run([*base, "metrics", "-O", "json", *extra, *tail])
            )
        if args.verbatim is not None:
            dst.with_name(dst.name + ".verbatim.jsonl").write_text(run([str(args.verbatim), lang, str(p)]))

    with ThreadPoolExecutor(max_workers=args.jobs) as ex:
        list(ex.map(one, jobs))

    pins = {}
    cargo = pathlib.Path(args.repo_toml) if args.repo_toml else None
    if cargo and cargo.is_file():
        for line in cargo.read_text().splitlines():
            m = re.match(r'\s*((?:[a-z0-9-]*tree-sitter[a-z0-9-]*)|tree-sitter)\s*=.*?"(=?[0-9][^"]*)"', line)
            if m:
                pins[m.group(1)] = m.group(2)
    manifest = {
        "complete": not FAILURES,
        "failed_executions": len(FAILURES),
        "bca": {"path": str(args.bca), "sha256": sha256(args.bca)},
        "verbatim": None
        if args.verbatim is None
        else {"path": str(args.verbatim), "sha256": sha256(args.verbatim)},
        "views": views(args.verbatim is not None),
        "roots": {k: str(v) for k, v in roots.items()},
        "metrics_options": METRICS_OPTIONS,
        "cli_cmds": CLI_CMDS,
        "outputs_per_file": len(views(args.verbatim is not None)),
        "populations": counts,
        "keys": sorted(keys),
        "inputs_sha256": inputs,
        "grammar_pins": pins,
        "skipped_symlinks": SKIPPED_LINKS,
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1))
    print(json.dumps({"populations": counts, "files": len(keys), "outputs": len(keys) * manifest["outputs_per_file"]}))
    if SKIPPED_LINKS:
        sys.stderr.write(f"skipped {len(SKIPPED_LINKS)} symlink(s); see manifest\n")
    if FAILURES:
        for argv, how, err in FAILURES[:20]:
            sys.stderr.write(f"FAIL {how}: {' '.join(argv)}\n{err}\n")
        sys.stderr.write(f"{len(FAILURES)} execution(s) failed; the capture is not a baseline\n")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
