#!/usr/bin/env python3
"""check-role-coverage

The runtime-kind coverage ratchet of the role tables (#1692).

For every language, every runtime kind id in the generated
``language_<lang>.rs`` enum — named rules and anonymous terminals, the
visible and the hidden, every alias that shares a spelling — is either
*claimed* (some row of some role names it in ``owns``, ``pays`` or
``wraps``), *excluded* (a row's ``excludes [Kind "why"]`` names it, and
the report says which role rejected it and why), or *debt* recorded in
``.role-coverage-baseline.txt``. A kind in none of the three fails,
naming the language, id, spelling and flags.

Identity is the ``(language, id)`` pair; a spelling identifies nothing.
Two ids that share a kind string are two entries, and claiming one
leaves the other in the residue (``grammar-dispatch.md`` §1). The ids
belong to one symbol table, so each language's baseline section
records the grammar crate and the version ``Cargo.toml`` pins; when the
pin moves the gate fails until ``--migrate-pin <lang>`` confirms every
baselined id still spells the same kind, listing those that do not for
hand removal. Named / visible flags are tree-sitter's own, from the
``<LANG>_KIND_FLAGS`` sidecar the ``enums/`` generator emits; they
label an entry and forgive nothing — a hidden ``_rule`` or an auxiliary
``_repeat1`` symbol is debt like any other until a row claims or
excludes it. The generator's ``ERROR`` sentinel and the ``end`` kind
(id 0) are the two synthetic entries left out, by name.

Debt only ratchets down. A residue entry not in the baseline fails;
``--update`` removes entries no longer in the residue and refuses,
non-zero, when the residue holds an entry the baseline does not.
``--bootstrap`` writes the initial file and refuses when one exists or
ever existed at ``HEAD``.

Observation. ``.role-coverage-observed.txt``, written by ``--observe``,
records which ``(language, id)`` pairs the integration corpus emitted at
this pin, with provenance: the corpus submodules' HEAD SHAs, the
``bca`` that observed them (built by the command itself), the grammar
pins, and per language the files attempted, parsed and failed. The
gate labels each residue entry ``observed`` or ``unobserved`` from it,
``unknown`` when the file is missing or stale or the language has no
corpus file at all; it never says "unreachable" — a kind no corpus
file emits may be one line of valid input away. ``--require-observations``
(CI) fails on a missing or stale file; languages the corpus does not
sample are named in every run and are a limit of the corpus, not of
the gate.

What this gate cannot see, printed with every run: whether a kind is
claimed by the *right* role, whether a row is complete, or whether the
consumer that should read a row does. It catches a kind newly unclaimed
after a grammar bump or a new language; it does not replace #1458's
measured sibling sweeps.

Reads and writes are anchored at the repository root resolved from this
file's location, so the gate runs correctly from anywhere.
"""

from __future__ import annotations

import argparse
import ast
import pathlib
import re
import subprocess
import sys
from dataclasses import dataclass

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
sys.path.insert(0, str(UTILS_DIR))

import role_tables as rt  # noqa: E402

DEFAULT_BASELINE = REPO_ROOT / ".role-coverage-baseline.txt"
DEFAULT_OBSERVED = REPO_ROOT / ".role-coverage-observed.txt"
CORPUS_DIR = REPO_ROOT / "tests" / "repositories"
CORPORA = ("DeepSpeech", "pdf.js", "serde")

SCOPE_NOTE = (
    "role-coverage: every runtime kind id is claimed, excluded with a reason, or "
    "baselined debt. It catches a kind newly unclaimed after a grammar bump or a new "
    "language; it does not judge which role claims a kind, whether a row is complete, "
    "or whether a consumer reads it, and it does not replace #1458's measured sweeps."
)

HEADER = """\
# Role-coverage baseline: runtime kinds no role declares (#1692).
# Maintained by check-role-coverage.py; see AGENTS.md "Validation gates".
#
# An entry is one (language, kind id) that no row of any role names in
# `owns`, `pays` or `wraps` and no row excludes with a reason. Identity
# is the id within the grammar version each section names; the spelling
# and the named / visible flags are labels. The gate fails on an entry
# not listed here; `--update` only removes entries. A moved grammar pin
# fails the gate until `--migrate-pin <lang>` confirms the listed ids
# still spell the same kinds.
#
# What the ratchet cannot see: a kind claimed by the wrong role, an
# incomplete row, a dead consumer path, or a consumer that reads nothing.
# It is not a measured sweep (#1458); it is the accounting.
#
# Sections: `[<language> <crate> <version>]` then one entry per line:
# <id> <named|anon> <visible|hidden> '<kind string>'
"""


@dataclass(frozen=True)
class Entry:
    lang: str
    id: int
    kind: str
    named: bool
    visible: bool

    def line(self) -> str:
        # The kind string is last and quoted: it can hold spaces (`not in`)
        # and any character but a newline.
        return f"{self.id} {'named' if self.named else 'anon'} {'visible' if self.visible else 'hidden'} {self.kind!r}"


@dataclass
class LangReport:
    lang: str
    crate: str
    version: str
    total: int
    claimed: dict[int, list[str]]  # id -> roles
    excluded: dict[int, list[tuple[str, str]]]  # id -> (role, reason)
    residue: list[Entry]


def analyse(
    roles_dir: pathlib.Path = rt.ROLES_DIR,
    langs_rs: pathlib.Path = rt.LANGS_RS,
    languages_dir: pathlib.Path = rt.LANGUAGES_DIR,
    cargo_toml: pathlib.Path = REPO_ROOT / "Cargo.toml",
) -> tuple[list[LangReport], list[str]]:
    """Per-language accounting, plus actionable errors (missing tables,
    unknown kinds named by a row, missing pins)."""
    errors: list[str] = []
    pins = rt.grammar_pins(cargo_toml)
    langs = rt.languages(langs_rs)
    tables = rt.load_tables(roles_dir, langs)
    reports: list[LangReport] = []
    for lang in langs:
        enum_path = languages_dir / f"language_{lang.stem}.rs"
        try:
            _, kinds, _ = rt.parse_enum(enum_path)
        except (rt.RoleTableError, FileNotFoundError) as exc:
            errors.append(f"{lang.variant}: cannot read the id table: {exc}")
            continue
        version = pins.get(lang.crate)
        if version is None:
            errors.append(f"{lang.variant}: no `{lang.crate}` pin in {cargo_toml.name}")
            continue
        table = tables.get(lang.variant)
        if table is None:
            errors.append(f"{lang.variant}: no roles/{lang.stem}.rs; the census reports this too")
            continue
        by_variant = {k.variant: k for k in kinds}
        claimed: dict[int, list[str]] = {}
        excluded: dict[int, list[tuple[str, str]]] = {}
        for role, row in table.rows.items():
            if row.status != "rows":
                continue
            for name in row.raw_kinds():
                k = by_variant.get(name)
                if k is None:
                    errors.append(f"{table.path.name}:{row.line}: `{role}` names `{name}`, which is not a `{lang.variant}` kind")
                    continue
                claimed.setdefault(k.id, []).append(role)
            for name, why in row.excludes:
                k = by_variant.get(name)
                if k is None:
                    errors.append(f"{table.path.name}:{row.line}: `{role}` excludes `{name}`, which is not a `{lang.variant}` kind")
                    continue
                excluded.setdefault(k.id, []).append((role, why))
        residue = [
            Entry(lang.variant, k.id, k.kind, k.named, k.visible)
            for k in kinds
            if k.id != 0 and k.id not in claimed and k.id not in excluded
        ]
        reports.append(LangReport(lang.variant, lang.crate, version, len(kinds), claimed, excluded, residue))
    return reports, errors


# ---------------------------------------------------------------------------
# Baseline
# ---------------------------------------------------------------------------

SECTION_RE = re.compile(r"^\[(\S+) (\S+) (\S+)\]$")
ENTRY_RE = re.compile(r"^(\d+) (named|anon) (visible|hidden) ('(?:[^'\\]|\\.)*'|\"(?:[^\"\\]|\\.)*\")$")


def load_baseline(path: pathlib.Path) -> tuple[dict[str, tuple[str, str]], dict[str, dict[int, Entry]]]:
    """``(pins_by_lang, entries_by_lang)``."""
    pins: dict[str, tuple[str, str]] = {}
    entries: dict[str, dict[int, Entry]] = {}
    lang = None
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.startswith("#"):
            continue
        sm = SECTION_RE.match(line)
        if sm:
            lang = sm.group(1)
            if lang in pins:
                raise rt.RoleTableError(f"{path}:{lineno}: section `{lang}` repeated")
            pins[lang] = (sm.group(2), sm.group(3))
            entries[lang] = {}
            continue
        em = ENTRY_RE.match(line)
        if not em or lang is None:
            raise rt.RoleTableError(f"{path}:{lineno}: expected `[<lang> <crate> <version>]` or `<id> <named|anon> <visible|hidden> '<kind>'`, got `{line}`")
        kid = int(em.group(1))
        if kid in entries[lang]:
            raise rt.RoleTableError(f"{path}:{lineno}: id {kid} repeated in `{lang}`")
        entries[lang][kid] = Entry(lang, kid, ast.literal_eval(em.group(4)), em.group(2) == "named", em.group(3) == "visible")
    return pins, entries


def write_baseline(path: pathlib.Path, reports: list[LangReport]) -> None:
    out = [HEADER]
    for r in sorted(reports, key=lambda r: r.lang):
        out.append(f"\n[{r.lang} {r.crate} {r.version}]\n")
        out.extend(e.line() + "\n" for e in sorted(r.residue, key=lambda e: e.id))
    path.write_text("".join(out), encoding="utf-8")


def existed_at_head(path: pathlib.Path) -> bool:
    rel = path.relative_to(REPO_ROOT) if path.is_relative_to(REPO_ROOT) else path
    run = subprocess.run(
        ["git", "-C", str(REPO_ROOT), "ls-tree", "--name-only", "HEAD", "--", str(rel)],
        capture_output=True, text=True, check=False,
    )
    return run.returncode == 0 and bool(run.stdout.strip())


# ---------------------------------------------------------------------------
# Observation
# ---------------------------------------------------------------------------

OBS_HEADER = """\
# Role-coverage observation: which runtime kinds the integration corpus
# emitted, with provenance (#1692). Written by
# `check-role-coverage.py --observe`; informational for the ratchet.
# A kind absent here is "unobserved", never "unreachable".
"""

# A node header in `bca dump`'s tree rendering: `{kind:id} from (r, c)`
# directly after the box-drawing prefix at the start of a line. The
# kind string may itself be `{`, `}` or `::`, so the id is read as the
# digits that close the header before ` from (`; the source snippet
# follows a ` : ` later on the line and is never read.
NODE_HEADER_RE = re.compile(r"^[\s│├╰─]*\{.*?:(\d+)\} from \(")

EXTENSIONS: dict[str, tuple[str, ...]] = {}


def corpus_files(lang: rt.Language, exts: dict[str, tuple[str, ...]]) -> list[pathlib.Path]:
    want = exts.get(lang.variant, ())
    files: list[pathlib.Path] = []
    if not want:
        return files
    for corpus in CORPORA:
        root = CORPUS_DIR / corpus
        if not root.is_dir():
            continue
        for p in root.rglob("*"):
            if p.is_file() and p.suffix.lstrip(".") in want:
                files.append(p)
    return sorted(files)


def extensions_from_langs(langs_rs: pathlib.Path = rt.LANGS_RS) -> dict[str, tuple[str, ...]]:
    """The extension lists `mk_langs!` gives each variant, through the
    one reader (fail-closed: a row it cannot read is an error, not a
    language silently `not-sampled`). Mozcpp owns no extension and is
    routed through Cpp's files by `--language`."""
    out = {l.variant: l.extensions for l in rt.languages(langs_rs)}
    if "Cpp" in out:
        out["Mozcpp"] = out["Cpp"]
    return out


def submodule_shas(allow_dirty: bool) -> dict[str, str]:
    shas: dict[str, str] = {}
    for corpus in CORPORA:
        path = CORPUS_DIR / corpus
        if not path.is_dir():
            raise rt.RoleTableError(f"corpus {path} is not checked out; run `make worktree-setup`")
        sha = subprocess.run(["git", "-C", str(path), "rev-parse", "HEAD"], check=True, **RUN).stdout.strip()
        dirty = subprocess.run(["git", "-C", str(path), "status", "--porcelain"], check=True, **RUN).stdout.strip()
        if dirty and not allow_dirty:
            raise rt.RoleTableError(f"corpus {corpus} has local changes; the observation would not describe {sha}")
        shas[corpus] = sha
    return shas


# Everything a subprocess prints is read as UTF-8 with replacement:
# `bca dump` echoes source snippets, and a locale of `C` would otherwise
# turn one non-ASCII byte into a traceback mid-walk.
RUN = dict(capture_output=True, text=True, encoding="utf-8", errors="replace")


def observe(
    out_path: pathlib.Path,
    allow_dirty: bool,
    langs_rs: pathlib.Path = rt.LANGS_RS,
    cargo_toml: pathlib.Path = REPO_ROOT / "Cargo.toml",
) -> int:
    shas = submodule_shas(allow_dirty)
    print("role-coverage: building the observer (`cargo build --release -p big-code-analysis-cli --locked`)")
    build = subprocess.run(["cargo", "build", "--release", "-p", "big-code-analysis-cli", "--locked"], cwd=REPO_ROOT, check=False, **RUN)
    if build.returncode != 0:
        print(build.stderr[-2000:])
        print("error: the observer did not build")
        return 2
    bca = REPO_ROOT / "target" / "release" / "bca"
    # `bca --version` prints `bca X.Y.Z`; keep the whole line.
    version = subprocess.run([str(bca), "--version"], check=True, **RUN).stdout.strip()
    pins = rt.grammar_pins(cargo_toml)
    exts = extensions_from_langs(langs_rs)
    # Provenance is the inputs — observer version, corpus commits, pins —
    # not the parent tree: a HEAD recorded here would name a commit that
    # a later squash or rebase leaves dangling.
    lines = [OBS_HEADER, f"observer {version}\n"]
    for corpus, sha in shas.items():
        lines.append(f"corpus {corpus} {sha}\n")
    for lang in rt.languages(langs_rs):
        files = corpus_files(lang, exts)
        seen: set[int] = set()
        attempted = parsed = failed = 0
        for f in files:
            attempted += 1
            run = subprocess.run([str(bca), "dump", "--language", lang.variant.lower(), "-p", str(f)], check=False, **RUN)
            if run.returncode != 0:
                failed += 1
                continue
            parsed += 1
            for line in run.stdout.splitlines():
                m = NODE_HEADER_RE.match(line)
                if m:
                    seen.add(int(m.group(1)))
        # `not-sampled` means the corpus has no file in this language.
        # Files attempted that all failed, or parsed with no header read,
        # are written as `sampled` with their counts and the gate rejects
        # the record — a broken observation is not evidence of absence.
        status = "not-sampled" if attempted == 0 else "sampled"
        lines.append(f"\n[{lang.variant} {lang.crate} {pins[lang.crate]} {status} attempted={attempted} parsed={parsed} failed={failed}]\n")
        lines.extend(f"{i}\n" for i in sorted(seen))
        print(f"  {lang.variant}: {status}, {parsed}/{attempted} parsed, {len(seen)} kinds")
    out_path.write_text("".join(lines), encoding="utf-8")
    print(f"role-coverage: wrote {out_path}")
    return 0


OBS_SECTION_RE = re.compile(r"^\[(\S+) (\S+) (\S+) (sampled|not-sampled) attempted=(\d+) parsed=(\d+) failed=(\d+)\]$")


@dataclass
class Observation:
    corpus_shas: dict[str, str]
    pins: dict[str, tuple[str, str]]
    sampled: dict[str, bool]
    counts: dict[str, tuple[int, int, int]]  # attempted, parsed, failed
    seen: dict[str, set[int]]


def load_observed(path: pathlib.Path) -> Observation:
    obs = Observation({}, {}, {}, {}, {})
    lang = None
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.startswith("#") or line.startswith("observer "):
            continue
        if line.startswith("corpus "):
            parts = line.split()
            if len(parts) != 3:
                raise rt.RoleTableError(f"{path}:{lineno}: expected `corpus <name> <sha>`, got `{line}`")
            obs.corpus_shas[parts[1]] = parts[2]
            continue
        sm = OBS_SECTION_RE.match(line)
        if sm:
            lang = sm.group(1)
            obs.pins[lang] = (sm.group(2), sm.group(3))
            obs.sampled[lang] = sm.group(4) == "sampled"
            obs.counts[lang] = (int(sm.group(5)), int(sm.group(6)), int(sm.group(7)))
            obs.seen[lang] = set()
            continue
        if lang is None or not line.strip().isdigit():
            raise rt.RoleTableError(f"{path}:{lineno}: unreadable observation line `{line}`")
        obs.seen[lang].add(int(line))
    return obs


def recorded_corpus_shas() -> dict[str, str]:
    """The corpus commits the tree records as gitlinks — what a clean
    checkout would populate. Read from the index, which is what the
    next commit records, so a corpus bump staged together with its
    re-observation passes the pre-commit gate; `HEAD`'s tree is the
    fallback for a checkout with no index entry. Either way a shallow
    checkout without the submodules (the CI lint job) still knows which
    corpus the observation must describe."""
    shas: dict[str, str] = {}
    for c in CORPORA:
        rel = (CORPUS_DIR / c).relative_to(REPO_ROOT).as_posix()
        run = subprocess.run(["git", "-C", str(REPO_ROOT), "ls-files", "-s", "--", rel], check=False, **RUN)
        parts = run.stdout.split()
        if run.returncode == 0 and len(parts) >= 4 and parts[0] == "160000":
            shas[c] = parts[1]
            continue
        run = subprocess.run(["git", "-C", str(REPO_ROOT), "ls-tree", "HEAD", "--", rel], check=False, **RUN)
        parts = run.stdout.split()
        if run.returncode == 0 and len(parts) >= 3 and parts[1] == "commit":
            shas[c] = parts[2]
    return shas


def observation_status(obs: Observation | None, reports: list[LangReport]) -> tuple[str, list[str]]:
    """``("ok" | "stale" | "missing", problems)``. Staleness is judged
    against the corpus commits the tree records; a populated submodule
    that has been moved off that commit locally is reported too."""
    if obs is None:
        return "missing", ["no .role-coverage-observed.txt; run `make role-coverage-observe`"]
    problems: list[str] = []
    recorded = recorded_corpus_shas()
    for c in CORPORA:
        want = recorded.get(c)
        if want is None:
            problems.append(f"corpus {c}: not a submodule of this tree")
            continue
        if obs.corpus_shas.get(c) != want:
            problems.append(f"corpus {c}: observed at {obs.corpus_shas.get(c, '?')[:12]}, tree records {want[:12]}")
        path = CORPUS_DIR / c
        if (path / ".git").exists() or (path.is_dir() and any(path.iterdir())):
            run = subprocess.run(["git", "-C", str(path), "rev-parse", "HEAD"], check=False, **RUN)
            if run.returncode == 0 and run.stdout.strip() != want:
                problems.append(f"corpus {c}: checked out at {run.stdout.strip()[:12]}, tree records {want[:12]}")
    for r in reports:
        if obs.pins.get(r.lang) != (r.crate, r.version):
            problems.append(f"{r.lang}: observed under {obs.pins.get(r.lang)}, pinned ({r.crate}, {r.version})")
            continue
        attempted, parsed, failed = obs.counts.get(r.lang, (0, 0, 0))
        seen = len(obs.seen.get(r.lang, ()))
        # `not-sampled` is only ever "the corpus has no file in this
        # language". Files that were attempted and yielded nothing are
        # a broken observation, whatever the record calls itself.
        if attempted and (parsed == 0 or seen == 0):
            problems.append(f"{r.lang}: attempted {attempted} corpus files, parsed {parsed}, saw {seen} kinds — a broken observation, not evidence; re-run --observe")
        if not attempted and obs.sampled.get(r.lang):
            problems.append(f"{r.lang}: marked sampled with no files attempted")
    return ("stale" if problems else "ok"), problems


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--baseline", type=pathlib.Path, default=DEFAULT_BASELINE)
    ap.add_argument("--observed", type=pathlib.Path, default=DEFAULT_OBSERVED)
    ap.add_argument("--roles-dir", type=pathlib.Path, default=rt.ROLES_DIR)
    ap.add_argument("--langs-rs", type=pathlib.Path, default=rt.LANGS_RS)
    ap.add_argument("--languages-dir", type=pathlib.Path, default=rt.LANGUAGES_DIR)
    ap.add_argument("--cargo-toml", type=pathlib.Path, default=REPO_ROOT / "Cargo.toml")
    ap.add_argument("--update", action="store_true", help="drop baseline entries no longer in the residue")
    ap.add_argument("--bootstrap", action="store_true", help="write the initial baseline")
    ap.add_argument("--migrate-pin", metavar="LANG", action="append", default=[], help="accept a moved grammar pin for LANG (repeatable; `all` for every language) when every baselined id still spells the same kind")
    ap.add_argument("--observe", action="store_true", help="rebuild the observer and record what the corpus emits")
    ap.add_argument("--allow-dirty", action="store_true", help="observe a corpus with local changes")
    ap.add_argument("--require-observations", action="store_true", help="fail when the observation file is missing or stale (CI)")
    ap.add_argument("--report", action="store_true", help="print the per-language accounting and the exclusions")
    args = ap.parse_args(argv)

    print(SCOPE_NOTE)
    if args.observe:
        try:
            return observe(args.observed, args.allow_dirty, args.langs_rs, args.cargo_toml)
        except rt.RoleTableError as exc:
            print(f"error: {exc}")
            return 2

    try:
        reports, errors = analyse(args.roles_dir, args.langs_rs, args.languages_dir, args.cargo_toml)
    except rt.RoleTableError as exc:
        print(f"error: {exc}")
        return 2
    if errors:
        for e in errors:
            print(f"error: {e}")
        print(f"role-coverage: FAIL — {len(errors)} input problem(s)")
        return 1

    if args.bootstrap:
        if args.baseline.exists() or existed_at_head(args.baseline):
            print(f"error: {args.baseline} exists or is tracked at HEAD; use --update, never re-bootstrap")
            return 2
        write_baseline(args.baseline, reports)
        print(f"role-coverage: wrote {sum(len(r.residue) for r in reports)} residue entr(ies) to {args.baseline}")
        return 0
    if not args.baseline.exists():
        print(f"error: {args.baseline} missing; a first run needs --bootstrap")
        return 2
    try:
        pins, recorded = load_baseline(args.baseline)
    except rt.RoleTableError as exc:
        print(f"error: {exc}")
        return 2

    obs = None
    obs_status, obs_problems = "missing", []
    if args.observed.exists():
        try:
            obs = load_observed(args.observed)
        except rt.RoleTableError as exc:
            print(f"error: {exc}")
            return 2
    obs_status, obs_problems = observation_status(obs, reports)

    failed = False
    for r in sorted(reports, key=lambda r: r.lang):
        if args.report:
            print(f"{r.lang}: {r.total} kinds, {len(r.claimed)} claimed, {len(r.excluded)} excluded, {len(r.residue)} residue")
            for kid, rejections in sorted(r.excluded.items()):
                for role, why in rejections:
                    print(f"  excluded {kid} under {role}: {why}")
        # pin transition
        section = pins.get(r.lang)
        if section is None:
            print(f"error: {r.lang}: no baseline section — a new language's residue is recorded deliberately: add a `[{r.lang} {r.crate} {r.version}]` section listing every residue entry below (`--report` prints them), then run the gate")
            failed = True
            continue
        if section != (r.crate, r.version):
            if r.lang in args.migrate_pin or "all" in args.migrate_pin:
                current_by_id = {e.id: e for e in r.residue}
                moved = [e for e in recorded[r.lang].values() if e.id in current_by_id and current_by_id[e.id].kind != e.kind]
                if moved:
                    for e in moved:
                        print(f"error: {r.lang}: baselined id {e.id} was `{e.kind}`, now `{current_by_id[e.id].kind}` — remove the entry by hand")
                    failed = True
                    continue
                recorded[r.lang] = {e.id: e for e in r.residue if e.id in recorded[r.lang]}
                pins[r.lang] = (r.crate, r.version)
                print(f"{r.lang}: pin migrated to {r.crate} {r.version}")
            else:
                print(f"error: {r.lang}: grammar pin moved from {section[1]} to {r.version}; ids may have renumbered — review and run --migrate-pin {r.lang} (several languages can share one pin: repeat the flag, or `--migrate-pin all`), then `make role-coverage-observe`")
                failed = True
                continue
        # A baselined id must still spell the kind it was recorded as —
        # on every run, not only across a pin move. A vendored grammar's
        # version does not change when its sources are regenerated, so
        # the id table can move under an unchanged pin.
        for e in r.residue:
            rec = recorded[r.lang].get(e.id)
            if rec is not None and rec.kind != e.kind:
                print(f"error: {r.lang}: baselined id {e.id} was `{rec.kind}`, now `{e.kind}` — the symbol table moved; remove the entry and record the new kind deliberately")
                failed = True
        new = [e for e in r.residue if e.id not in recorded[r.lang]]
        for e in new:
            label = "unknown" if obs is None or obs_status != "ok" else ("observed" if e.id in obs.seen.get(r.lang, set()) else "unobserved")
            print(f"error: {r.lang}: kind {e.id} `{e.kind}` ({'named' if e.named else 'anon'}, {'visible' if e.visible else 'hidden'}, corpus: {label}) is claimed by no role, excluded by none, and not baselined")
            failed = True
    if failed:
        print("role-coverage: FAIL")
        return 1

    if args.require_observations and obs_status != "ok":
        for p in obs_problems:
            print(f"observation: {p}")
        print(f"role-coverage: FAIL — observation {obs_status}")
        return 1
    if args.update or args.migrate_pin:
        updated = []
        for r in reports:
            current_ids = {e.id for e in r.residue}
            before = len(recorded[r.lang])
            recorded[r.lang] = {i: e for i, e in recorded[r.lang].items() if i in current_ids}
            if len(recorded[r.lang]) != before:
                updated.append((r.lang, before - len(recorded[r.lang])))
        out = [HEADER]
        for lang in sorted(recorded):
            crate, version = pins[lang]
            out.append(f"\n[{lang} {crate} {version}]\n")
            out.extend(e.line() + "\n" for e in sorted(recorded[lang].values(), key=lambda e: e.id))
        args.baseline.write_text("".join(out), encoding="utf-8")
        print(f"role-coverage: baseline updated — removed {sum(n for _, n in updated)} entr(ies)" if updated else "role-coverage: baseline already current")
        return 0

    stale = sum(1 for r in reports for i in recorded[r.lang] if i not in {e.id for e in r.residue})
    if stale:
        print(f"note: {stale} baseline entr(ies) no longer in the residue; run --update")
    if obs_status != "ok":
        for p in obs_problems:
            print(f"observation: {p}")
    unsampled = sorted(r.lang for r in reports if obs is not None and obs_status == "ok" and not obs.sampled.get(r.lang, False))
    if unsampled:
        # Reported, never failed: the corpus has no file in these languages,
        # so every residue entry there is `unknown` — not `unobserved`, and
        # never `unreachable`. Adding a corpus is the only way to change it.
        print(f"observation: not sampled by the corpus ({len(unsampled)} languages): {', '.join(unsampled)}")
    residue = sum(len(r.residue) for r in reports)
    observed = sum(1 for r in reports for e in r.residue if obs is not None and obs_status == "ok" and e.id in obs.seen.get(r.lang, set()))
    print(f"role-coverage: OK — {len(reports)} languages, {residue} baselined residue entries (observation {obs_status}: {observed} observed in the corpus)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
