#!/usr/bin/env python3
"""Zero-drift capture driver (#1697 R07, landed by #1744).

Per file and language, records the CLI's `ops`, `metrics` (under an
options matrix), `dump`, `find -t call|string|function` and `count`
output, and — when `--verbatim` names the `verbatim-oracle` binary
(`cargo build -p big-code-analysis-bench --bin verbatim-oracle`) — the
library-level verbatim record: metrics, ops, the Alterator-built AST
export, find/count, on the raw bytes, the only path that reaches a
node ending at EOF. Paired MI views run through both binaries, with
selected-metric dependency closure and Rust prune/try combinations.
The default verbatim record stays compatible with recovery captures;
additional metrics-only records use `--metrics-view` and spell their
effective options in the output. `metrics_options` maps each CLI or
verbatim metric view to its argv; additional views use the usual `.txt`
suffix even though their payload is JSON.

The verbatim view is optional per side because a base commit older
than #1744 has no `verbatim-oracle` to build: a base-vs-head run over
such a base compares the CLI views only, and the verbatim view is
compared head-vs-head from the next change on. The manifest records
which views the capture carries (`views`), and `drift-compare.py`
refuses to report a view only one side has as "0 differing".

Repairs over the R06 driver (plan §5 Phase 0b):
- fixture roots are arguments and every one is validated BEFORE any
  filtering, so a missing root fails the run instead of shrinking it;
- the extension table is derived from the `mk_langs!` rows
  (`drift_common.populations`), not hand-copied, so an extension the
  enum owns cannot be silently skipped;
- the output key is `<language>/<root-id>/<relpath>`, so two fixture
  roots holding the same basename cannot overwrite each other and a
  file several populations read (`.h` under cpp/mozcpp/ccomment/preproc)
  is one key per language;
- symlinks are never followed (skipped and logged): a followed link
  would let a key escape its root;
- outputs are captured and written as bytes, so CRLF and invalid UTF-8
  compare as what the binary emitted;
- a process killed by a signal (returncode < 0) is classified apart
  from a non-zero exit; both fail the run;
- every subprocess call is an argv list, never a shell string;
- `metrics` runs under an options matrix (default, --exclude-tests,
  --cyclomatic-count-try=false) so the Rust `count_try` path and the
  prune path are captured;
- a manifest records binaries (sha256), roots, option matrix, grammar
  pins and the full key set, so two captures can be compared on keys
  as well as bytes (`drift-compare.py`). An existing manifest in the
  output directory is rewritten as incomplete before the first output
  is written, so an interrupted recapture cannot leave a stale
  `complete: true` beside partially overwritten outputs.

usage: drift-oracle.py --bca BIN [--verbatim BIN] --out DIR --root ID=PATH ...
                       [--corpus PATH] [--lang L ...] [--jobs N]
                       [--repo-toml Cargo.toml]
                       [--selection keys.json] [--view NAME ...] [--relative-paths]

An explicit selection is a nonempty JSON array of language/root-id/relative-path
keys. Each selected input must be tracked by Git; selection bypasses extension
inference so dialects and helper inputs can be forced. --relative-paths passes
input paths relative to the current repository directory before execution. It
never rewrites returned output. Named views retain the existing option matrix.

Self-tested by `utils/drift-oracle-test.py` (`make drift-oracle-test`),
which stubs both binaries. Nothing lints `utils/*.py` today (#1477);
the self-test is the only gate.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

UTILS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(UTILS_DIR))

import drift_common as dc  # noqa: E402

# slug -> extensions, from the enum; see `drift_common.populations`.
POPULATIONS = dc.populations()

# The two helper grammars have no corpus of their own and would walk
# the whole C/C++ population (1k+ files) once more each; the fixtures
# are enough for them since every row kind is spelled there. Mozcpp
# walks the corpus: it is a distinct grammar whose parity with Cpp is
# what the sweep measures.
FIXTURE_ONLY = {"ccomment", "preproc"}

# Paired MI modes cover documentation, pruning and dependency closure.
# The last pair combines the Rust axes without multiplying every legacy
# selection by options that cannot affect it.
MI_METRICS_OPTIONS = {
    f"{name}_mi_charges_docs_{mode}": [*extra, f"--mi-charges-docs={mode}"]
    for name, extra in {
        "metrics": [],
        "metrics_exclude_tests": ["--exclude-tests"],
        "metrics_only_mi": ["--metrics", "mi"],
        "metrics_only_mi_exclude_tests": ["--metrics", "mi", "--exclude-tests"],
        "metrics_only_mi_exclude_tests_no_count_try": ["--metrics", "mi", "--exclude-tests", "--cyclomatic-count-try=false"],
    }.items()
    for mode in ("false", "true")
}

# `metrics` option matrix: name -> extra argv. Keep the original views
# so existing default captures retain their names and commands.
METRICS_OPTIONS = {
    "metrics": [],
    "metrics_exclude_tests": ["--exclude-tests"],
    "metrics_no_count_try": ["--cyclomatic-count-try=false"],
    "metrics_exclude_tests_no_count_try": ["--exclude-tests", "--cyclomatic-count-try=false"],
    "metrics_only_cyclomatic": ["--metrics", "cyclomatic"],
    "metrics_only_cognitive": ["--metrics", "cognitive"],
    "metrics_only_nexits": ["--metrics", "nexits"],
    **MI_METRICS_OPTIONS,
}

VERBATIM_OPTIONS = {
    f"verbatim_{name}": ["--metrics-view", *extra, "--"]
    for name, extra in MI_METRICS_OPTIONS.items()
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
    ap.add_argument("--selection", type=pathlib.Path, help="JSON array of exact language/root-id/relative-path keys; tracked files only")
    ap.add_argument("--view", action="append", default=[], help="capture only this named view; repeatable")
    ap.add_argument("--relative-paths", action="store_true", help="pass repository-relative input paths to binaries without rewriting output")
    ap.add_argument(
        "--repo-toml",
        type=pathlib.Path,
        default=dc.DEFAULT_CARGO_TOML,
        help="root Cargo.toml, for the grammar pins in the manifest (default: this checkout's)",
    )
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
FAILURES: list[tuple[list[str], str, str]] = []


def run(argv: list[str]) -> bytes:
    """The bytes one capture file holds for `argv`; a failed run is
    recorded in `FAILURES` and written behind its status line."""
    c = dc.capture(argv)
    if c.how is not None:
        FAILURES.append((argv, c.how, c.preview()))
    return c.rendered()


def views(verbatim: bool) -> list[str]:
    """The per-file output names a capture carries, in the order
    `drift-compare.py` walks them."""
    return [*CLI_CMDS, *METRICS_OPTIONS, *(["verbatim", *VERBATIM_OPTIONS] if verbatim else [])]


def selected_inputs(selection: pathlib.Path, roots: dict[str, pathlib.Path]) -> list[tuple[str, str, pathlib.Path, pathlib.Path]]:
    """Validate every explicit key before capture, including tracking and path containment."""
    keys = json.loads(selection.read_text())
    if not isinstance(keys, list) or not keys or any(not isinstance(k, str) for k in keys):
        sys.exit("selection must be a nonempty JSON array of keys")
    if len(set(keys)) != len(keys):
        sys.exit("duplicate selection key")
    jobs = []
    for key in keys:
        parts = key.split("/")
        if len(parts) < 3 or parts[0] not in POPULATIONS or parts[1] not in roots:
            sys.exit(f"unknown language/root in selection key {key!r}")
        if any(p in ("", ".", "..") or "\\" in p for p in parts):
            sys.exit(f"invalid selection key {key!r}")
        lang, rid, *relative = parts
        root = roots[rid]
        path = root.joinpath(*relative)
        if any(p.is_symlink() for p in [path, *path.parents] if p.is_relative_to(root)):
            sys.exit(f"symlink in selection key {key!r}")
        if not path.is_file():
            sys.exit(f"missing selected input {key!r}")
        tracked = subprocess.run(
            ["git", "-C", str(root), "ls-files", "--error-unmatch", "--", "/".join(relative)],
            capture_output=True,
        )
        if tracked.returncode:
            sys.exit(f"selected input is not tracked: {key!r}")
        jobs.append((lang, key, root, path))
    return jobs


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    SKIPPED_LINKS.clear()
    FAILURES.clear()
    for b in (args.bca, args.verbatim):
        if b is not None and not b.is_file():
            sys.exit(f"missing binary {b}")
    if not args.repo_toml.is_file():
        sys.exit(f"missing --repo-toml {args.repo_toml}")
    pins = dc.grammar_pins(args.repo_toml)
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
    available_views = views(args.verbatim is not None)
    if len(set(args.view)) != len(args.view) or set(args.view) - set(available_views):
        sys.exit("duplicate or unavailable --view")
    selected_views = args.view or available_views
    cli_cmds = {k: v for k, v in CLI_CMDS.items() if k in selected_views}
    metric_options = {k: v for k, v in METRICS_OPTIONS.items() if k in selected_views}
    verbatim_options = {k: v for k, v in VERBATIM_OPTIONS.items() if k in selected_views}

    # The corpus walk skips the snapshot submodule's own tree and git
    # metadata; a fixture root is walked whole.
    corpus_skip = ("big-code-analysis-output", ".git")
    jobs: list[tuple[str, str, pathlib.Path, pathlib.Path]] = []
    counts: dict[str, int] = {}
    keys: set[str] = set()
    inputs: dict[str, str] = {}
    out = args.out.resolve()
    destinations: set[pathlib.Path] = set()
    explicit = selected_inputs(args.selection, roots) if args.selection else None
    for lang, exts in POPULATIONS.items():
        if only and lang not in only:
            continue
        if explicit is not None and not only and not any(job[0] == lang for job in explicit):
            continue
        n = 0
        for rid, root in roots.items():
            if rid == "corpus" and lang in FIXTURE_ONLY:
                continue
            paths = (
                [p for l, k, _, p in explicit if l == lang and k.split("/")[1] == rid]
                if explicit is not None else
                dc.walk(root, exts, corpus_skip if rid == "corpus" else (".git",), SKIPPED_LINKS)
            )
            for p in paths:
                key = f"{lang}/{rid}/{p.relative_to(root)}"
                if key in keys:
                    sys.exit(f"duplicate key {key}")
                dst = (out / key).resolve()
                if not dst.is_relative_to(out) or dst in destinations:
                    sys.exit(f"output destination for {key} escapes or collides: {dst}")
                destinations.add(dst)
                keys.add(key)
                inputs[key] = sha256(p)
                if args.relative_paths:
                    try:
                        p = p.relative_to(pathlib.Path.cwd())
                    except ValueError:
                        sys.exit(f"selected input outside current directory: {p}")
                jobs.append((lang, key, root, p))
                n += 1
        if n == 0:
            sys.exit(f"population {lang} is empty")
        counts[lang] = n

    out.mkdir(parents=True, exist_ok=True)
    manifest_path = out / "manifest.json"

    def manifest(complete: bool) -> dict:
        return dc.build_manifest(
            complete=complete,
            failed_executions=len(FAILURES),
            bca={"path": str(args.bca), "sha256": sha256(args.bca)},
            verbatim=None if args.verbatim is None else {"path": str(args.verbatim), "sha256": sha256(args.verbatim)},
            views=selected_views,
            roots={k: str(v) for k, v in roots.items()},
            metrics_options={**metric_options, **verbatim_options},
            cli_cmds=cli_cmds,
            populations=counts,
            keys=sorted(keys),
            inputs_sha256=inputs,
            grammar_pins=pins,
            skipped_symlinks=SKIPPED_LINKS,
        )

    # Before the first output write: a recapture into a directory that
    # already holds a complete manifest must not leave it there if this
    # run dies part-way (D1 on #1744).
    manifest_path.write_text(json.dumps(manifest(complete=False), indent=1))

    def one(job: tuple[str, str, pathlib.Path, pathlib.Path]) -> None:
        lang, key, _root, p = job
        dst = out / key
        dst.parent.mkdir(parents=True, exist_ok=True)
        base = [str(args.bca)]
        tail = ["--no-config", "--language", lang, "-p", str(p)]
        for name, cmd in cli_cmds.items():
            dst.with_name(dst.name + f".{name}.txt").write_bytes(run([*base, *cmd, *tail]))
        for name, extra in metric_options.items():
            dst.with_name(dst.name + f".{name}.txt").write_bytes(run([*base, "metrics", "-O", "json", *extra, *tail]))
        if args.verbatim is not None:
            if "verbatim" in selected_views:
                dst.with_name(dst.name + ".verbatim.jsonl").write_bytes(run([str(args.verbatim), lang, str(p)]))
            for name, extra in verbatim_options.items():
                dst.with_name(dst.name + f".{name}.txt").write_bytes(run([str(args.verbatim), *extra, lang, str(p)]))

    with ThreadPoolExecutor(max_workers=args.jobs) as ex:
        list(ex.map(one, jobs))

    final = manifest(complete=not FAILURES)
    manifest_path.write_text(json.dumps(final, indent=1))
    print(json.dumps({"populations": counts, "files": len(keys), "outputs": len(keys) * final["outputs_per_file"]}))
    if SKIPPED_LINKS:
        sys.stderr.write(f"skipped {len(SKIPPED_LINKS)} symlink(s); see manifest\n")
    if FAILURES:
        for cmd, how, err in FAILURES[:20]:
            sys.stderr.write(f"FAIL {how}: {' '.join(cmd)}\n{err}\n")
        sys.stderr.write(f"{len(FAILURES)} execution(s) failed; the capture is not a baseline\n")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
