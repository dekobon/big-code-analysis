#!/usr/bin/env python3
"""Self-tests for check-role-coverage.py on synthetic enums, tables and
pins, pinning both directions: the accounting, the identity rules
(id not spelling; named and anonymous apart), the ratchet, and the
observation labels.

Run: ``python3 -m unittest -q utils/check-role-coverage-test.py``.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import os
import pathlib
import re
import stat
import subprocess
import sys
import tempfile
import unittest
import unittest.mock

UTILS_DIR = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(UTILS_DIR))

import role_tables as rt  # noqa: E402


def _load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, UTILS_DIR / filename)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


cov = _load("check_role_coverage", "check-role-coverage.py")

MOD_RS = """\
macro_rules! role_table {
    (@named Call { $($b:tt)* }) => { pub(crate) mod call { $($b)* } };
    (@named Branch { $($b:tt)* }) => { pub(crate) mod branch { $($b)* } };
    (@named Container { $($b:tt)* }) => { pub(crate) mod container { $($b)* } };
}
"""

LANGS_RS = """\
mk_langs!(
    (
        "java",
        Java,
        "The Java language",
        "java",
        JavaCode,
        JavaParser,
        tree_sitter_java,
        [java],
    ),
);
"""

# Two languages sharing one pinned crate, as Tsx / Typescript do.
LANGS_RS_SHARED = LANGS_RS.replace(
    ");\n",
    """    (
        "typescript",
        Tsx,
        "TSX (shares the TypeScript crate)",
        "tsx",
        TsxCode,
        TsxParser,
        tree_sitter_tsx,
        [tsx],
    ),
    (
        "typescript",
        Typescript,
        "TypeScript",
        "typescript",
        TypescriptCode,
        TypescriptParser,
        tree_sitter_typescript,
        [ts],
    ),
);
""",
)

CARGO = 'tree-sitter-java = "=0.23.5"\n'

JAVA = rt.Language("Java", "java", "tree_sitter_java", ("java",))
JAVA_EXTS = {"Java": ("java",)}


# Keeps the suite's git away from the host's configuration as
# `tests/common/vcs_fixture.rs` does (#1771): a global `core.hooksPath`,
# `init.templateDir` or ignore-everything `core.excludesFile` would
# otherwise decide whether a fixture can be built at all.
GIT_ISOLATION = {
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_CONFIG_COUNT": "2",
    "GIT_CONFIG_KEY_0": "core.excludesFile",
    "GIT_CONFIG_VALUE_0": os.devnull,
    "GIT_CONFIG_KEY_1": "core.attributesFile",
    "GIT_CONFIG_VALUE_1": os.devnull,
}


def setUpModule() -> None:
    """Every git the suite runs — the fixtures, `Tree`'s scratch
    repository and the gate's own reads under test — sees `GIT_ISOLATION`
    and no inherited `GIT_*` variable, as `check-deny-manifests-test.py`'s
    `_git_env` drops them. Git exports `GIT_DIR` and `GIT_INDEX_FILE` to
    hooks, and `GIT_DIR` to `git rebase --exec` in a linked worktree;
    both outrank `git -C`, so inherited they turn a scratch repository's
    `init`, `add` and `commit` into writes to the checkout running the
    suite. Module-wide rather than per subprocess, as the sibling suites
    do it, because the gate's own `corpus_git` reads `os.environ`."""
    env = unittest.mock.patch.dict(os.environ)
    env.start()
    unittest.addModuleCleanup(env.stop)
    for key in [k for k in os.environ if k.startswith("GIT_")]:
        del os.environ[key]
    os.environ.update(GIT_ISOLATION)


def git(*args: str) -> str:
    """One fixture git command, with a throwaway identity for the commits a
    fixture makes. A failure carries git's own message, which
    `check=True` would drop."""
    run = subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@x", *args], capture_output=True, text=True, check=False)
    if run.returncode != 0:
        raise AssertionError(f"git {' '.join(args)}: {run.stderr.strip()}")
    return run.stdout


def tracked_corpus(path: pathlib.Path, files: dict[str, str]) -> pathlib.Path:
    """A corpus at `path` that is a repository of its own, with `files`
    (relative path → content) written and added to its index — the only
    thing the enumerator reads."""
    path.mkdir(parents=True, exist_ok=True)
    git("init", "-q", str(path))
    for rel, content in files.items():
        (path / rel).parent.mkdir(parents=True, exist_ok=True)
        (path / rel).write_text(content)
    git("-C", str(path), "add", "--", *files)
    return path


def enum_rs(kinds: list[tuple[str, str, bool, bool]]) -> str:
    """``(Variant, "kind", named, visible)`` per runtime id in order; the
    ERROR sentinel is appended as the generator does."""
    lines = ["pub enum Java {"]
    for i, (v, _, _, _) in enumerate(kinds):
        lines.append(f"    {v} = {i},")
    lines.append(f"    Error = {len(kinds)},")
    lines.append("}")
    lines.append("pub const JAVA_KIND_FLAGS: &[(u16, bool, bool)] = &[")
    for i, (_, _, n, vis) in enumerate(kinds):
        lines.append(f"    ({i}, {str(n).lower()}, {str(vis).lower()}),")
    lines.append("];")
    lines.append("impl From<Java> for &'static str {\n    fn from(tok: Java) -> Self {\n        match tok {")
    for v, k, _, _ in kinds:
        lines.append(f'            Java::{v} => "{k}",')
    lines.append('            Java::Error => "ERROR",')
    lines.append("        }\n    }\n}")
    return "\n".join(lines) + "\n"


# id 0 end; 1 identifier (named); 2 `<` anon; 3 `<` anon again (alias,
# a second id with the same spelling); 4 `_hidden` named hidden;
# 5 method_invocation named; 6 `__volatile__` anon
KINDS = [
    ("End", "end", False, False),
    ("Identifier", "identifier", True, True),
    ("LT", "<", False, True),
    ("LT2", "<", False, True),
    ("Hidden", "_hidden", True, False),
    ("MethodInvocation", "method_invocation", True, True),
    ("Volatile", "__volatile__", False, True),
]


class Tree:
    def __init__(self, kinds=KINDS) -> None:
        self.dir = pathlib.Path(tempfile.mkdtemp(prefix="role-cov-"))
        self.roles = self.dir / "roles"
        self.roles.mkdir()
        (self.roles / "mod.rs").write_text(MOD_RS)
        self.langs = self.dir / "langs.rs"
        self.langs.write_text(LANGS_RS)
        self.languages = self.dir / "languages"
        self.languages.mkdir()
        (self.languages / "language_java.rs").write_text(enum_rs(kinds))
        self.cargo = self.dir / "Cargo.toml"
        self.cargo.write_text(CARGO)
        self.baseline = self.dir / "baseline.txt"
        self.observed = self.dir / "observed.txt"
        # Corpus provenance is read from the gate's repository root; point
        # it at an empty scratch repo so no test reads the host checkout.
        self.repo = self.dir / "repo"
        self.repo.mkdir()
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        (self.repo / "tests" / "repositories").mkdir(parents=True)

    def table(self, body: str) -> None:
        (self.roles / "java.rs").write_text(f"role_table! {{\n    lang: Java;\n{body}}}\n")

    def run(self, *extra: str) -> tuple[int, str]:
        out = io.StringIO()
        saved = (cov.REPO_ROOT, cov.CORPUS_DIR)
        cov.REPO_ROOT, cov.CORPUS_DIR = self.repo, self.repo / "tests" / "repositories"
        try:
            with contextlib.redirect_stdout(out):
                code = cov.main([
                    "--roles-dir", str(self.roles), "--langs-rs", str(self.langs),
                    "--languages-dir", str(self.languages), "--cargo-toml", str(self.cargo),
                    "--baseline", str(self.baseline), "--observed", str(self.observed), *extra,
                ])
        finally:
            cov.REPO_ROOT, cov.CORPUS_DIR = saved
        return code, out.getvalue()

    def residue_ids(self) -> list[int]:
        _, entries = cov.load_baseline(self.baseline)
        return sorted(entries["Java"])


FULL = "    Call { owns [MethodInvocation] }\n    Branch { pending R07 }\n    Container { absent \"none\" }\n"


class CoverageTests(unittest.TestCase):
    def setUp(self) -> None:
        self.t = Tree()

    def test_observer_header_keeps_one_line_and_round_trips_through_the_reader(self) -> None:
        # Since #1773 `bca --version` is a multi-line provenance block; the
        # writer reads `-V` and keeps its one line, so the file it writes is
        # one its own reader accepts. Verified by perturbation: the old
        # `.strip()` of the whole block makes `load_observed` raise on the
        # `build inputs:` line.
        self.assertEqual(cov.observer_line("bca 2.4.0\n"), "bca 2.4.0")
        self.assertEqual(
            cov.observer_line("bca 2.4.0\nbuild inputs: abc\ngit: def\n"), "bca 2.4.0"
        )
        with self.assertRaises(rt.RoleTableError):
            cov.observer_line("build inputs: abc\n")
        with self.assertRaises(rt.RoleTableError):
            cov.observer_line("")
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp, "observed.txt")
            path.write_text(
                f"{cov.OBS_HEADER}observer {cov.observer_line('bca 2.4.0\nbuild inputs: abc\n')}\n",
                encoding="utf-8",
            )
            cov.load_observed(path)
            path.write_text(
                f"{cov.OBS_HEADER}observer bca 2.4.0\nbuild inputs: abc\n", encoding="utf-8"
            )
            with self.assertRaises(rt.RoleTableError):
                cov.load_observed(path)

    def test_claimed_kinds_leave_the_residue_and_end_is_synthetic(self) -> None:
        self.t.table(FULL)
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 0, out)
        # 0 (end) and 7 (ERROR) are never entries; 5 is claimed
        self.assertEqual(self.t.residue_ids(), [1, 2, 3, 4, 6])
        text = self.t.baseline.read_text()
        self.assertIn("[Java tree-sitter-java 0.23.5]", text)
        self.assertIn("4 named hidden '_hidden'", text)
        self.assertIn("6 anon visible '__volatile__'", text)
        self.assertIn("role-coverage: every runtime kind id is claimed", out)

    def test_two_ids_one_spelling_are_two_entries(self) -> None:
        self.t.table("    Call { owns [MethodInvocation] pays [LT] }\n    Branch { pending R07 }\n    Container { absent \"none\" }\n")
        self.t.run("--bootstrap")
        self.assertIn(3, self.t.residue_ids(), "claiming LT (id 2) must leave LT2 (id 3)")
        self.assertNotIn(2, self.t.residue_ids())

    def test_excluded_kind_leaves_the_residue_and_is_reported_under_its_role(self) -> None:
        self.t.table("    Call { owns [MethodInvocation] }\n    Branch { owns [Identifier] excludes [Volatile \"type syntax\"] }\n    Container { absent \"none\" }\n")
        self.t.run("--bootstrap")
        self.assertNotIn(6, self.t.residue_ids())
        code, out = self.t.run("--report")
        self.assertEqual(code, 0)
        self.assertIn("excluded 6 under Branch: type syntax", out)

    def test_cross_role_claim_removes_from_residue_once(self) -> None:
        self.t.table("    Call { owns [MethodInvocation] }\n    Branch { pending R07 }\n    Container { owns [MethodInvocation] }\n")
        self.t.run("--bootstrap")
        self.assertNotIn(5, self.t.residue_ids())

    def test_hidden_and_anonymous_kinds_are_debt_not_forgiven(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        self.assertIn(4, self.t.residue_ids())
        self.assertIn(6, self.t.residue_ids())

    def test_new_unclaimed_kind_fails_and_update_refuses(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        # a grammar bump adds an anonymous terminal at the same pin
        kinds = KINDS + [("CoReturn", "co_return", False, True)]
        (self.t.languages / "language_java.rs").write_text(enum_rs(kinds))
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("kind 7 `co_return` (anon, visible, corpus: unknown) is claimed by no role", out)
        code, out = self.t.run("--update")
        self.assertEqual(code, 1, "update must not accept new debt")
        self.assertNotIn(7, self.t.residue_ids())

    def test_update_only_removes(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        self.t.table("    Call { owns [MethodInvocation, Identifier] }\n    Branch { pending R07 }\n    Container { absent \"none\" }\n")
        code, out = self.t.run()
        self.assertEqual(code, 0)
        self.assertIn("no longer in the residue; run --update", out)
        code, out = self.t.run("--update")
        self.assertEqual(code, 0)
        self.assertEqual(self.t.residue_ids(), [2, 3, 4, 6])

    def test_row_naming_an_unknown_kind_is_an_input_error(self) -> None:
        self.t.table("    Call { owns [Nope] }\n    Branch { pending R07 }\n    Container { absent \"none\" }\n")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 1)
        self.assertIn("`Call` names `Nope`, which is not a `Java` kind", out)

    def test_missing_pin_fails(self) -> None:
        self.t.table(FULL)
        self.t.cargo.write_text("")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 2)
        self.assertIn("no `tree-sitter*` pins", out)

    def test_missing_sidecar_fails(self) -> None:
        self.t.table(FULL)
        (self.t.languages / "language_java.rs").write_text(enum_rs(KINDS).replace("JAVA_KIND_FLAGS", "NOPE"))
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 1)
        self.assertIn("no `JAVA_KIND_FLAGS` sidecar", out)

    def test_moved_pin_fails_until_migrated(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        self.t.cargo.write_text('tree-sitter-java = "=0.24.0"\n')
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("grammar pin moved from 0.23.5 to 0.24.0", out)
        code, out = self.t.run("--migrate-pin", "Java")
        self.assertEqual(code, 0, out)
        self.assertIn("[Java tree-sitter-java 0.24.0]", self.t.baseline.read_text())

    def test_two_languages_on_one_pin_migrate_together(self) -> None:
        self.t.langs.write_text(LANGS_RS_SHARED)
        for v in ("Tsx", "Typescript"):
            (self.t.languages / f"language_{v.lower()}.rs").write_text(enum_rs(KINDS).replace("Java", v).replace("JAVA_", v.upper() + "_"))
            (self.t.roles / f"{v.lower()}.rs").write_text(f"role_table! {{\n    lang: {v};\n" + FULL + "}\n")
        self.t.cargo.write_text('tree-sitter-typescript = "=0.23.2"\n' + CARGO)
        self.t.table(FULL)
        self.t.run("--bootstrap")
        self.t.cargo.write_text('tree-sitter-typescript = "=0.24.0"\n' + CARGO)
        code, out = self.t.run("--migrate-pin", "Tsx")
        self.assertEqual(code, 1, "the Typescript crate is shared; naming only Tsx cannot complete")
        self.assertIn("Typescript: grammar pin moved", out)
        self.assertNotIn("[Tsx tree-sitter-typescript 0.24.0]", self.t.baseline.read_text())
        code, out = self.t.run("--migrate-pin", "Tsx", "--migrate-pin", "Typescript")
        self.assertEqual(code, 0, out)
        text = self.t.baseline.read_text()
        self.assertIn("[Tsx tree-sitter-typescript 0.24.0]", text)
        self.assertIn("[Typescript tree-sitter-typescript 0.24.0]", text)
        self.assertIn("[Java tree-sitter-java 0.23.5]", text)
        # `all` is the spelling the release bump uses
        self.t.cargo.write_text('tree-sitter-typescript = "=0.25.0"\n' + CARGO)
        code, out = self.t.run("--migrate-pin", "all")
        self.assertEqual(code, 0, out)

    def test_a_new_language_names_the_deliberate_remedy(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        self.t.langs.write_text(LANGS_RS_SHARED)
        for v in ("Tsx", "Typescript"):
            (self.t.languages / f"language_{v.lower()}.rs").write_text(enum_rs(KINDS).replace("Java", v).replace("JAVA_", v.upper() + "_"))
            (self.t.roles / f"{v.lower()}.rs").write_text(f"role_table! {{\n    lang: {v};\n" + FULL + "}\n")
        self.t.cargo.write_text('tree-sitter-typescript = "=0.23.2"\n' + CARGO)
        for flags in ((), ("--update",), ("--migrate-pin", "Tsx")):
            code, out = self.t.run(*flags)
            self.assertEqual(code, 1, flags)
            self.assertIn("add a `[Tsx tree-sitter-typescript 0.23.2]` section", out)
        # The remedy the message names must produce the entries: `--report`
        # prints counts, not rows, and the per-entry errors need a section
        # first (#1729). A scratch bootstrap writes every language's block.
        self.assertIn("`--bootstrap --baseline <new-path>` — a path that does not exist yet", out)
        scratch = self.t.dir / "scratch" / "baseline.txt"
        scratch.parent.mkdir()
        code, out = self.t.run("--bootstrap", "--baseline", str(scratch))
        self.assertEqual(code, 0, out)
        text = scratch.read_text()
        start = text.index("[Tsx tree-sitter-typescript 0.23.2]")
        # Tsx is not the last section in a sorted bootstrap today; the
        # fallback keeps the slice honest if a language sorts after it.
        end = text.find("\n[", start)
        block = text[start : end if end != -1 else len(text)]
        self.assertIn("\n1 named visible 'identifier'\n", block)
        self.t.baseline.write_text(self.t.baseline.read_text() + "\n" + block)
        code, out = self.t.run()
        self.assertEqual(code, 1, "Typescript still has no section")
        self.assertNotIn("Tsx: no baseline section", out)
        self.assertNotIn("Tsx: kind", out)

    def test_extensions_come_from_the_one_reader_and_route_mozcpp(self) -> None:
        exts = cov.extensions_from_langs(self.t.langs)
        self.assertEqual(exts, {"Java": ("java",)})
        self.t.langs.write_text(LANGS_RS.replace("tree_sitter_java,\n        [java],", "tree_sitter_cpp,\n        [cpp, hpp],").replace("Java,", "Cpp,"))
        self.assertEqual(cov.extensions_from_langs(self.t.langs), {"Cpp": ("cpp", "hpp"), "Mozcpp": ("cpp", "hpp")})
        self.t.langs.write_text(LANGS_RS.replace("tree_sitter_java,\n        [java],", "tree_sitter_javascript,\n        [js, mjs],").replace("Java,", "Javascript,"))
        self.assertEqual(cov.extensions_from_langs(self.t.langs), {"Javascript": ("js", "mjs")}, "no Mozjs row, no override")
        two = LANGS_RS.replace("tree_sitter_java,\n        [java],", "tree_sitter_javascript,\n        [js, mjs],").replace("Java,", "Javascript,")
        two = two.replace("    ),\n);", '    ),\n    (\n        "mozjs",\n        Mozjs,\n        "Mozilla JS",\n        "mozjs",\n        MozjsCode,\n        MozjsParser,\n        tree_sitter_mozjs,\n        [jsm],\n    ),\n);')
        self.t.langs.write_text(two)
        self.assertEqual(cov.extensions_from_langs(self.t.langs), {"Javascript": ("js", "mjs"), "Mozjs": ("jsm", "js", "mjs")}, "Mozjs keeps its own extension and reads JavaScript's files too (#1700)")
        self.t.langs.write_text(LANGS_RS.replace("        [java],\n", ""))
        with self.assertRaises(rt.RoleTableError):
            cov.extensions_from_langs(self.t.langs)

    def corpora(self, corpora: tuple[tuple[str, str | None], ...] = (("serde", None),)) -> contextlib.AbstractContextManager[object]:
        """The gate pointed at the scratch repo's corpora; both globals are
        restored however the block exits."""
        return unittest.mock.patch.multiple(cov, CORPUS_DIR=self.t.repo / "tests" / "repositories", CORPORA=corpora)

    def corpus_files(self, corpora: tuple[tuple[str, str | None], ...] = (("serde", None),)) -> list[pathlib.Path]:
        """`corpus_listing` routed to Java against the scratch repo's
        corpora: the enumeration, every refusal it makes, and the suffix
        filter in one call."""
        with self.corpora(corpora):
            return cov.corpus_files(JAVA, JAVA_EXTS, cov.corpus_listing())

    def test_corpus_files_match_extensions_under_the_corpus_dir(self) -> None:
        tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"a/x.java": "", "a/y.rs": ""})
        self.assertEqual([f.name for f in self.corpus_files()], ["x.java"])

    def test_a_corpus_with_a_subpath_is_sampled_under_it_only(self) -> None:
        # The output submodule holds snapshot output and per-language
        # corpora beside `control_flow/`; only the subpath is sampled, so
        # a `.java` elsewhere in it is not an observation (#1700). Both
        # files are tracked, so the pathspec is what excludes the stray.
        sub = self.t.repo / "tests" / "repositories" / "big-code-analysis-output"
        tracked_corpus(sub, {"control_flow/java/r07.java": "", "csharp/stray.java": ""})
        files = self.corpus_files((("big-code-analysis-output", "control_flow"),))
        self.assertEqual([f.name for f in files], ["r07.java"])

    def test_a_nested_submodule_is_one_gitlink_not_its_files(self) -> None:
        # A recursive checkout populates the corpus's own submodules,
        # whose files belong to commits the provenance never records; the
        # observation must not change with checkout depth (#1836). The
        # nested repository's file and the untracked one are on disk
        # beside the tracked ones, and only the index decides. A tracked
        # symlink is a `120000` entry and is dropped, as the derived
        # parity sweep's walker drops it, so the two censuses agree.
        corpus = tracked_corpus(
            self.t.repo / "tests" / "repositories" / "serde",
            {"x.java": "", "run.java": "", "sub dir/café.java": ""},
        )
        (corpus / "run.java").chmod(0o755)
        (corpus / "link.java").symlink_to("x.java")
        git("-C", str(corpus), "add", "run.java", "link.java")
        # Added as a registered, active submodule rather than written as a
        # bare gitlink: `ls-files --recurse-submodules` descends only into
        # an active one, so only this shape notices that flag coming back.
        # Its path carries a routed suffix, so the mode filter, not the
        # extension filter, is what drops the gitlink.
        source = tracked_corpus(self.t.dir / "nested-source", {"inner.java": ""})
        git("-C", str(source), "commit", "-q", "-m", "nested")
        git("-C", str(corpus), "-c", "protocol.file.allow=always", "submodule", "add", "-q", str(source), "nested.java")
        (corpus / "stray.java").write_text("")
        modes = {line.split()[0] for line in git("-C", str(corpus), "ls-files", "-s").splitlines()}
        self.assertEqual(modes, {"100644", "100755", "120000", "160000"}, "the fixture spells a regular file, an executable, a symlink and a gitlink")
        files = self.corpus_files()
        self.assertEqual([f.relative_to(corpus).as_posix() for f in files], ["run.java", "sub dir/café.java", "x.java"])

    def test_a_corpus_that_is_not_its_own_repository_is_refused(self) -> None:
        # An uninitialised submodule is a directory of the parent
        # checkout, and git answers for the parent there. The file is in
        # the *parent's* index, so a missing guard samples it rather than
        # finding nothing.
        corpus = self.t.repo / "tests" / "repositories" / "serde"
        corpus.mkdir(parents=True)
        (corpus / "x.java").write_text("")
        git("-C", str(self.t.repo), "add", "--", str(corpus / "x.java"))
        with self.assertRaises(rt.RoleTableError) as ctx:
            self.corpus_files()
        self.assertIn("not a checked-out repository of its own; run `make worktree-setup`", str(ctx.exception))

    def test_a_corpus_git_cannot_open_is_refused_with_gits_reason(self) -> None:
        # A gitfile naming a module directory that is gone (or a corpus
        # owned by another user) is not what `make worktree-setup`
        # repairs, so git's own message reaches the user and that remedy
        # is not named.
        corpus = self.t.repo / "tests" / "repositories" / "serde"
        corpus.mkdir(parents=True)
        (corpus / ".git").write_text(f"gitdir: {self.t.dir / 'gone'}\n")
        with self.assertRaises(cov.CorpusUnopenable) as ctx:
            self.corpus_files()
        # The wording is git's and varies by version; its `fatal:` does not.
        self.assertIn(f"corpus {corpus}: git cannot open it: fatal: ", str(ctx.exception))
        self.assertNotIn("worktree-setup", str(ctx.exception))

    def test_observation_status_tolerates_a_corpus_git_cannot_open(self) -> None:
        # The gate judges staleness from the tree's gitlink; a corpus git
        # refuses outright has no HEAD to compare, and refusing the gate
        # for it would make `--require-observations` fail on a checkout
        # whose corpora were, say, chowned by an earlier `sudo` run. The
        # observer is what refuses such a corpus.
        repo = self.t.repo
        corpus = repo / "tests" / "repositories" / "serde"
        corpus.mkdir(parents=True)
        (corpus / ".git").write_text(f"gitdir: {self.t.dir / 'gone'}\n")
        git("-C", str(repo), "commit", "-q", "--allow-empty", "-m", "parent")
        sha = "1" * 40
        git("-C", str(repo), "update-index", "--add", "--cacheinfo", f"160000,{sha},tests/repositories/serde")
        obs = cov.Observation({"serde": sha}, {}, {}, {}, {})
        with unittest.mock.patch.object(cov, "REPO_ROOT", repo), self.corpora():
            self.assertEqual(cov.observation_status(obs, []), ("ok", []))

    def test_an_inherited_object_directory_does_not_reach_a_corpus_read(self) -> None:
        # `GIT_OBJECT_DIRECTORY`, which a `receive-pack` quarantine hook
        # exports, passes the toplevel check — git takes the `-C`
        # directory's own `.git` — and the index needs no object, so
        # `ls-files` lists the corpus; the dirt check's read of HEAD then
        # fails with `bad object`, an uncaught `CalledProcessError`
        # instead of an observation. The other repository holds a
        # different commit, so its store cannot answer for this one.
        corpus = tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"x.java": ""})
        git("-C", str(corpus), "commit", "-q", "-m", "corpus")
        other = tracked_corpus(self.t.dir / "other", {"o.java": ""})
        git("-C", str(other), "commit", "-q", "-m", "other")
        inherited = {"GIT_OBJECT_DIRECTORY": str(other / ".git" / "objects")}
        with unittest.mock.patch.dict(os.environ, inherited), self.corpora():
            shas = cov.submodule_shas(allow_dirty=False)
            files = cov.corpus_files(JAVA, JAVA_EXTS, cov.corpus_listing())
        self.assertEqual(shas, {"serde": git("-C", str(corpus), "rev-parse", "HEAD").strip()})
        self.assertEqual([f.relative_to(corpus).as_posix() for f in files], ["x.java"])

    def test_a_tracked_file_missing_from_disk_is_refused(self) -> None:
        # Sparse and skip-worktree checkouts leave `git status` clean with
        # tracked files absent, and an interrupted one (#1171) leaves them
        # deleted; sampling around them would write a partial observation
        # as a complete one.
        corpus = tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"x.java": "", "gone.java": ""})
        (corpus / "gone.java").unlink()
        with self.assertRaises(rt.RoleTableError) as ctx:
            self.corpus_files()
        self.assertIn("tracked file gone.java is not on disk", str(ctx.exception))
        # `make worktree-setup` alone cannot repair a sparse corpus, and
        # forcing it restores a file deleted on purpose.
        self.assertIn("git sparse-checkout disable", str(ctx.exception))
        self.assertIn("a file deleted on purpose `git rm`", str(ctx.exception))

    def test_a_tracked_file_whose_directory_is_now_a_file_is_missing_not_unreadable(self) -> None:
        # Stat answers ENOTDIR, not ENOENT, for `a/x.java` once `a` is a
        # regular file; nothing is there to read, so it takes the
        # missing-file remedy rather than the permission one.
        corpus = tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"a/x.java": ""})
        (corpus / "a" / "x.java").unlink()
        (corpus / "a").rmdir()
        (corpus / "a").write_text("")
        with self.assertRaises(rt.RoleTableError) as ctx:
            self.corpus_files()
        self.assertIn("tracked file a/x.java is not on disk", str(ctx.exception))

    def test_a_tracked_file_under_an_unsearchable_directory_is_refused_not_a_traceback(self) -> None:
        # A directory the user cannot search (left root-owned by an earlier
        # sudo run) is a permission problem, not a missing file, and `main`
        # reports a `RoleTableError`, never a traceback. `Path.is_file`
        # raises there through Python 3.13 and answers False from 3.14,
        # where it became `os.path.isfile`. CI's lint job runs `3.x` and
        # the venv 3.12, so each would see only its own; both behaviours
        # are installed in turn, and a gate that asks `is_file` fails one.
        corpus = tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"locked/x.java": ""})
        locked = corpus / "locked"
        locked.chmod(0)
        self.addCleanup(locked.chmod, 0o755)
        if os.access(locked, os.X_OK):
            self.skipTest("a privileged user searches through mode 000")

        def raises_on_eacces(path: pathlib.Path, **_: object) -> bool:
            return stat.S_ISREG(os.stat(path).st_mode)

        def answers_false(path: pathlib.Path, **_: object) -> bool:
            return os.path.isfile(path)

        for mode, is_file in (("through 3.13", raises_on_eacces), ("from 3.14", answers_false)):
            with self.subTest(mode), unittest.mock.patch.object(pathlib.Path, "is_file", is_file):
                with self.assertRaises(rt.RoleTableError) as ctx:
                    self.corpus_files()
                self.assertIn("tracked file locked/x.java cannot be read", str(ctx.exception))

    def test_observe_refuses_a_broken_corpus_before_building(self) -> None:
        # The release build and the dump loop take minutes, and a refusal
        # reached inside that loop wastes them. `--allow-dirty` lets a
        # deleted file past the dirt check, so the missing-file refusal is
        # the one left to fire, and it must fire before `cargo` runs: this
        # `REPO_ROOT` has no manifest, so a build that started would fail
        # with exit 2 rather than raise.
        corpus = tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"x.java": "", "gone.java": ""})
        git("-C", str(corpus), "commit", "-q", "-m", "corpus")
        (corpus / "gone.java").unlink()
        out = io.StringIO()
        with unittest.mock.patch.object(cov, "REPO_ROOT", self.t.repo), self.corpora(), contextlib.redirect_stdout(out):
            with self.assertRaises(rt.RoleTableError) as ctx:
                cov.observe(self.t.observed, allow_dirty=True, langs_rs=self.t.langs, cargo_toml=self.t.cargo)
        self.assertIn("tracked file gone.java is not on disk", str(ctx.exception))
        self.assertNotIn("building the observer", out.getvalue())

    def test_a_corpus_whose_index_lists_nothing_is_refused(self) -> None:
        # The interrupted checkout of #1171 can leave HEAD at the recorded
        # commit with no index and no files. Git reads the missing index
        # as an empty one, so no file is tracked to be missing, and the
        # corpus would contribute nothing instead of failing.
        corpus = tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"x.java": ""})
        git("-C", str(corpus), "commit", "-q", "-m", "corpus")
        (corpus / ".git" / "index").unlink()
        (corpus / "x.java").unlink()
        with self.assertRaises(rt.RoleTableError) as ctx:
            self.corpus_files()
        self.assertIn("corpus serde: its index lists no file; run `make worktree-setup`", str(ctx.exception))

    def test_a_corpus_whose_index_lists_nothing_under_the_subpath_names_it(self) -> None:
        # The output submodule is sampled under `control_flow/` only; one
        # that tracks files elsewhere but none there would also contribute
        # nothing, and the message has to say where it looked.
        sub = self.t.repo / "tests" / "repositories" / "big-code-analysis-output"
        tracked_corpus(sub, {"csharp/stray.java": ""})
        (sub / "control_flow").mkdir()
        with self.assertRaises(rt.RoleTableError) as ctx:
            self.corpus_files((("big-code-analysis-output", "control_flow"),))
        self.assertIn("corpus big-code-analysis-output: its index lists no file under `control_flow/`; run `make worktree-setup`", str(ctx.exception))

    def test_a_missing_tracked_file_is_refused_whatever_its_extension(self) -> None:
        # The derived parity sweep's `files_under` refuses any tracked file
        # the disk lacks, routed or not; the observer must refuse the same
        # checkout, or `make role-corpus-refresh` writes an observation and
        # then fails the sweep on a corpus the observer called complete.
        corpus = tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"x.java": "", "README.md": ""})
        (corpus / "README.md").unlink()
        with self.assertRaises(rt.RoleTableError) as ctx:
            self.corpus_files()
        self.assertIn("tracked file README.md is not on disk", str(ctx.exception))

    def test_a_malformed_ls_files_record_is_refused_not_dropped(self) -> None:
        # A record without its tab has no path to sample; dropping it
        # would list a corpus smaller than its index, as the sweep's
        # walker refuses to.
        corpus = tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"x.java": ""})
        listing = subprocess.CompletedProcess([], 0, stdout=b"100644 0000000000000000000000000000000000000000 0\0", stderr=b"")
        with unittest.mock.patch.object(cov, "corpus_git", return_value=listing), self.assertRaises(rt.RoleTableError) as ctx:
            cov.tracked_files(corpus, None)
        self.assertIn("malformed `git ls-files` record", str(ctx.exception))

    def test_an_inherited_repository_does_not_reach_a_corpus_read(self) -> None:
        # Git exports GIT_DIR and GIT_INDEX_FILE to hooks, and GIT_DIR to
        # `git rebase --exec` in a linked worktree. Inherited, the toplevel
        # check passes — git takes the `-C` directory for the work tree —
        # while HEAD and the index are the other repository's.
        corpus = tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"x.java": ""})
        git("-C", str(corpus), "commit", "-q", "-m", "corpus")
        other = tracked_corpus(self.t.dir / "other", {"o.java": ""})
        git("-C", str(other), "commit", "-q", "-m", "other")
        inherited = {"GIT_DIR": str(other / ".git"), "GIT_INDEX_FILE": str(other / ".git" / "index")}
        with unittest.mock.patch.dict(os.environ, inherited), self.corpora():
            files = cov.corpus_files(JAVA, JAVA_EXTS, cov.corpus_listing())
            shas = cov.submodule_shas(allow_dirty=False)
        self.assertEqual([f.relative_to(corpus).as_posix() for f in files], ["x.java"])
        self.assertEqual(shas, {"serde": git("-C", str(corpus), "rev-parse", "HEAD").strip()})

    def test_nested_submodule_state_is_not_a_local_change_of_the_corpus(self) -> None:
        # A nested submodule is one gitlink and never its files (#1836), so
        # build output inside one must not refuse the corpus as dirty. The
        # corpus's own untracked file still does: the check is narrowed to
        # what is sampled, not dropped.
        source = tracked_corpus(self.t.dir / "nested-source", {"n.java": ""})
        git("-C", str(source), "commit", "-q", "-m", "nested")
        corpus = tracked_corpus(self.t.repo / "tests" / "repositories" / "serde", {"x.java": ""})
        git("-C", str(corpus), "-c", "protocol.file.allow=always", "submodule", "add", "-q", str(source), "nested")
        git("-C", str(corpus), "commit", "-q", "-m", "corpus")
        (corpus / "nested" / "build.out").write_text("")
        with self.corpora():
            shas = cov.submodule_shas(allow_dirty=False)
            (corpus / "stray.java").write_text("")
            with self.assertRaises(rt.RoleTableError) as ctx:
                cov.submodule_shas(allow_dirty=False)
        self.assertEqual(shas, {"serde": git("-C", str(corpus), "rev-parse", "HEAD").strip()})
        self.assertIn("corpus serde has local changes", str(ctx.exception))

    def test_observation_status_reads_head_only_from_a_corpus_repository(self) -> None:
        # A populated corpus directory that is not a repository of its own
        # — files copied in, or strays in an uninitialised submodule's
        # directory — answers `rev-parse HEAD` with the parent's commit,
        # which the gate would report as the corpus checkout.
        repo = self.t.repo
        corpus = repo / "tests" / "repositories" / "serde"
        corpus.mkdir(parents=True)
        (corpus / "stray.txt").write_text("")
        git("-C", str(repo), "commit", "-q", "--allow-empty", "-m", "parent")
        sha = "1" * 40
        git("-C", str(repo), "update-index", "--add", "--cacheinfo", f"160000,{sha},tests/repositories/serde")
        obs = cov.Observation({"serde": sha}, {}, {}, {}, {})
        with unittest.mock.patch.object(cov, "REPO_ROOT", repo), self.corpora():
            status, problems = cov.observation_status(obs, [])
        self.assertEqual((status, problems), ("stale", [f"corpus {corpus} is not a checked-out repository of its own; run `make worktree-setup`"]))

    def test_submodule_shas_refuses_an_uninitialised_corpus(self) -> None:
        # Without the guard `rev-parse HEAD` in the empty directory
        # returns the parent's HEAD, and the observation records it as
        # the corpus commit.
        (self.t.repo / "f").write_text("")
        git("-C", str(self.t.repo), "add", "f")
        git("-C", str(self.t.repo), "commit", "-q", "-m", "parent")
        (self.t.repo / "tests" / "repositories" / "serde").mkdir(parents=True)
        with self.corpora(), self.assertRaises(rt.RoleTableError) as ctx:
            cov.submodule_shas(allow_dirty=True)
        self.assertIn("not a checked-out repository of its own", str(ctx.exception))

    def test_observe_refuses_a_corpus_missing_its_subpath(self) -> None:
        # A checkout whose submodule predates `control_flow/` would
        # otherwise observe nothing for it and write `not-sampled`, which
        # reads as "no file in this language" rather than as a stale
        # submodule (#1700).
        sub = self.t.repo / "tests" / "repositories" / "big-code-analysis-output"
        sub.mkdir()
        subprocess.run(["git", "init", "-q", str(sub)], check=True)
        saved = (cov.CORPUS_DIR, cov.CORPORA)
        cov.CORPUS_DIR, cov.CORPORA = self.t.repo / "tests" / "repositories", (("big-code-analysis-output", "control_flow"),)
        try:
            with self.assertRaises(rt.RoleTableError) as ctx:
                cov.submodule_shas(allow_dirty=True)
        finally:
            cov.CORPUS_DIR, cov.CORPORA = saved
        self.assertIn("has no `control_flow/`", str(ctx.exception))

    def test_the_production_corpora_name_the_output_submodule_under_control_flow(self) -> None:
        self.assertIn(("big-code-analysis-output", "control_flow"), cov.CORPORA)
        self.assertIn(("serde", None), cov.CORPORA)

    def test_claimed_report_labels_every_claim_by_the_corpus(self) -> None:
        # `--claimed` is the no-occurrence census of #1700: every declared
        # (language, role, kind) with the same label a residue entry gets,
        # under every observation state. `Call` claims id 5 and `Branch`
        # id 1; the fixture observation sees only id 5, so the `Call`
        # claim is observed and the `Branch` one unobserved; without a
        # sampled section every claim is unknown.
        self.t.table("    Call { owns [MethodInvocation] }\n    Branch { owns [Identifier] }\n    Container { absent \"none\" }\n")
        self.t.run("--bootstrap")
        self.t.observed.write_text("[Java tree-sitter-java 0.23.5 sampled attempted=1 parsed=1 failed=0]\n5\n")
        saved = cov.CORPORA
        try:
            cov.CORPORA = ()
            code, out = self.t.run("--claimed")
        finally:
            cov.CORPORA = saved
        self.assertEqual(code, 0, out)
        lines = out.splitlines()
        self.assertIn("Java: 2 claimed kinds \u2014 1 observed, 1 unobserved, 0 unknown (role, kind) claims", lines)
        self.assertIn("  Java Call 5 observed", lines)
        self.assertIn("  Java Branch 1 unobserved", lines)
        self.t.observed.write_text("[Java tree-sitter-java 0.23.5 not-sampled attempted=0 parsed=0 failed=0]\n")
        try:
            cov.CORPORA = ()
            _, out = self.t.run("--claimed")
        finally:
            cov.CORPORA = saved
        self.assertIn("  Java Call 5 unknown", out.splitlines())
        self.assertIn("Java: 2 claimed kinds \u2014 0 observed, 0 unobserved, 2 unknown (role, kind) claims", out.splitlines())

    def test_recorded_corpus_shas_read_the_index_before_head(self) -> None:
        # A corpus bump staged with its re-observation must pass: the
        # index gitlink is what the commit will record, HEAD is the past.
        repo = self.t.repo
        corpus = repo / "tests" / "repositories" / "serde"
        corpus.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "init", "-q", str(corpus)], check=True)
        (corpus / "f").write_text("1")
        subprocess.run(["git", "-C", str(corpus), "add", "."], check=True)
        subprocess.run(["git", "-C", str(corpus), "-c", "user.name=t", "-c", "user.email=t@x", "commit", "-q", "-m", "c1"], check=True)
        subprocess.run(["git", "-C", str(repo), "-c", "protocol.file.allow=always", "submodule", "add", "-q", str(corpus), "tests/repositories/serde"], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@x", "commit", "-q", "-m", "add corpus"], check=True)
        head_sha = subprocess.run(["git", "-C", str(corpus), "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
        # move the corpus and stage the new gitlink, do not commit
        sub = repo / "tests" / "repositories" / "serde"
        (sub / "g").write_text("2")
        subprocess.run(["git", "-C", str(sub), "add", "."], check=True)
        subprocess.run(["git", "-C", str(sub), "-c", "user.name=t", "-c", "user.email=t@x", "commit", "-q", "-m", "c2"], check=True)
        new_sha = subprocess.run(["git", "-C", str(sub), "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
        subprocess.run(["git", "-C", str(repo), "add", "tests/repositories/serde"], check=True)
        saved = (cov.REPO_ROOT, cov.CORPUS_DIR, cov.CORPORA)
        cov.REPO_ROOT, cov.CORPUS_DIR, cov.CORPORA = repo, repo / "tests" / "repositories", (("serde", None),)
        try:
            shas = cov.recorded_corpus_shas()
        finally:
            cov.REPO_ROOT, cov.CORPUS_DIR, cov.CORPORA = saved
        self.assertNotEqual(head_sha, new_sha)
        self.assertEqual(shas, {"serde": new_sha}, "the staged gitlink, not HEAD's")

    def test_a_malformed_corpus_line_is_reported_not_a_traceback(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        self.t.observed.write_text("corpus DeepSpeech\n")
        code, out = self.t.run()
        self.assertEqual(code, 2)
        self.assertIn("expected `corpus <name> <sha>`", out)

    def test_migration_refuses_a_renumbered_id(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        self.t.cargo.write_text('tree-sitter-java = "=0.24.0"\n')
        # id 4 now spells something else
        kinds = list(KINDS)
        kinds[4] = ("Other", "other_rule", True, True)
        (self.t.languages / "language_java.rs").write_text(enum_rs(kinds))
        code, out = self.t.run("--migrate-pin", "Java")
        self.assertEqual(code, 1)
        self.assertIn("baselined id 4 was `_hidden`, now `other_rule`", out)

    def test_missing_baseline_is_fatal_and_bootstrap_refuses_existing(self) -> None:
        self.t.table(FULL)
        code, out = self.t.run()
        self.assertEqual(code, 2)
        self.t.run("--bootstrap")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 2)

    def test_malformed_baseline_is_fatal(self) -> None:
        self.t.table(FULL)
        self.t.baseline.write_text("[Java tree-sitter-java 0.23.5]\n1 identifier named visible\n")
        code, out = self.t.run()
        self.assertEqual(code, 2)
        self.assertIn("expected `[<lang> <crate> <version>]` or", out)

    def test_kind_string_with_spaces_round_trips(self) -> None:
        kinds = KINDS + [("Notin", "not in", False, True), ("Quote", "it's", False, True)]
        t = Tree(kinds)
        t.table(FULL)
        t.run("--bootstrap")
        code, out = t.run()
        self.assertEqual(code, 0, out)
        _, entries = cov.load_baseline(t.baseline)
        self.assertEqual(entries["Java"][7].kind, "not in")
        self.assertEqual(entries["Java"][8].kind, "it's")

    def test_observation_labels_and_require_flag(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        code, out = self.t.run("--require-observations")
        self.assertEqual(code, 1)
        self.assertIn("observation missing", out)
        # a stale observation (wrong pin) is reported and fails under --require
        self.t.observed.write_text("corpus DeepSpeech x\ncorpus pdf.js y\ncorpus serde z\n[Java tree-sitter-java 0.22.0 sampled attempted=1 parsed=1 failed=0]\n1\n")
        code, out = self.t.run("--require-observations")
        self.assertEqual(code, 1)
        self.assertIn("observed under ('tree-sitter-java', '0.22.0'), pinned", out)
        code, out = self.t.run()
        self.assertEqual(code, 0, "stale observation is informational without --require")
        self.assertIn("observation stale", out)

    def test_not_sampled_is_reported_not_failed(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        saved = cov.CORPORA
        try:
            cov.CORPORA = ()
            self.t.observed.write_text("[Java tree-sitter-java 0.23.5 not-sampled attempted=0 parsed=0 failed=0]\n")
            code, out = self.t.run("--require-observations")
        finally:
            cov.CORPORA = saved
        self.assertEqual(code, 0, out)
        self.assertIn("not sampled by the corpus (1 languages): Java", out)

    def _new_kind_label(self, observed: str) -> str:
        """The corpus label a fresh residue entry gets under an `ok`
        observation file reading ``observed`` (its Java section at the
        pinned version, so only the sampling decides the label).

        The file's `ok` status is proved by a `--require-observations`
        run *before* the new kind exists: once a residue entry fails
        the gate, `main` returns ahead of the `observation:` lines, so
        a stale-guard on the labelled run could never fire."""
        self.t.table(FULL)
        self.t.run("--bootstrap")
        saved = cov.CORPORA
        try:
            cov.CORPORA = ()
            self.t.observed.write_text(observed)
            code, out = self.t.run("--require-observations")
            self.assertEqual(code, 0, out)
            self.assertNotIn("observation stale", out)
            (self.t.languages / "language_java.rs").write_text(enum_rs(KINDS + [("CoReturn", "co_return", False, True)]))
            code, out = self.t.run()
        finally:
            cov.CORPORA = saved
        self.assertEqual(code, 1)
        line = next(ln for ln in out.splitlines() if "kind 7 `co_return`" in ln)
        return line.split("corpus: ")[1].split(")")[0]

    def test_a_sampled_language_labels_an_unseen_kind_unobserved(self) -> None:
        label = self._new_kind_label("[Java tree-sitter-java 0.23.5 sampled attempted=1 parsed=1 failed=0]\n1\n")
        self.assertEqual(label, "unobserved")

    def test_an_unsampled_language_labels_a_new_kind_unknown(self) -> None:
        # The corpus never looked, so it did not fail to see the kind:
        # `unknown`, as the module docstring says, not `unobserved` (#1729).
        label = self._new_kind_label("[Java tree-sitter-java 0.23.5 not-sampled attempted=0 parsed=0 failed=0]\n")
        self.assertEqual(label, "unknown")

    def test_node_header_regex_reads_headers_only_and_punctuation_kinds(self) -> None:
        self.assertEqual(cov.NODE_HEADER_RE.match("╰─ {class_declaration:233} from (1, 1) to (1, 42) : class A { {x:9} }").group(1), "233")
        self.assertEqual(cov.NODE_HEADER_RE.match("{program:138} from (1, 1) to (2, 1) ").group(1), "138")
        # anonymous terminals spelled with braces or colons are headers too
        self.assertEqual(cov.NODE_HEADER_RE.match("      ├─ {{:72} from (1, 9) to (1, 10) : { ").group(1), "72")
        self.assertEqual(cov.NODE_HEADER_RE.match("      ╰─ {}:17} from (1, 86) to (1, 87) : } ").group(1), "17")
        self.assertEqual(cov.NODE_HEADER_RE.match("   ├─ {:::40} from (1, 1) to (1, 3) : :: ").group(1), "40")
        self.assertEqual(cov.NODE_HEADER_RE.match("   ├─ {::41} from (1, 1) to (1, 2) : : ").group(1), "41")
        self.assertIsNone(cov.NODE_HEADER_RE.match("   │  ╰─ text : {kind:9} from ("))

    def test_a_sampled_language_with_nothing_parsed_is_not_evidence(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        saved = cov.CORPORA
        try:
            cov.CORPORA = ()
            self.t.observed.write_text("[Java tree-sitter-java 0.23.5 sampled attempted=9 parsed=0 failed=9]\n")
            code, out = self.t.run("--require-observations")
        finally:
            cov.CORPORA = saved
        self.assertEqual(code, 1)
        self.assertIn("attempted 9 corpus files, parsed 0, saw 0 kinds — a broken observation", out)

    def test_a_headerless_run_relabelled_not_sampled_is_still_rejected(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        saved = cov.CORPORA
        try:
            cov.CORPORA = ()
            self.t.observed.write_text("[Java tree-sitter-java 0.23.5 not-sampled attempted=9 parsed=9 failed=0]\n")
            code, out = self.t.run("--require-observations")
        finally:
            cov.CORPORA = saved
        self.assertEqual(code, 1)
        self.assertIn("attempted 9 corpus files, parsed 9, saw 0 kinds — a broken observation", out)

    def test_require_observations_is_enforced_on_update_too(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        code, out = self.t.run("--update", "--require-observations")
        self.assertEqual(code, 1)
        self.assertIn("observation missing", out)

    def test_a_baselined_id_that_now_spells_another_kind_fails_without_a_pin_move(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        kinds = list(KINDS)
        kinds[4] = ("Other", "new_rule", True, True)
        (self.t.languages / "language_java.rs").write_text(enum_rs(kinds))
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("baselined id 4 was `_hidden`, now `new_rule`", out)
        code, _ = self.t.run("--update")
        self.assertEqual(code, 1)

    def test_output_is_deterministic_across_row_order(self) -> None:
        self.t.table(FULL)
        self.t.run("--bootstrap")
        first = self.t.baseline.read_text()
        self.t.baseline.unlink()
        # same claims, rows in another order
        self.t.table("    Container { absent \"none\" }\n    Branch { pending R07 }\n    Call { owns [MethodInvocation] }\n")
        self.t.run("--bootstrap")
        self.assertEqual(first, self.t.baseline.read_text())
        self.assertIn("[Java tree-sitter-java 0.23.5]", first)

    def test_the_canonical_dump_names_each_paying_mode(self) -> None:
        # `--dump` is the Rust differential test's oracle, and the lists
        # it prints are the same for a `pays []` row and a plain
        # `owns [..]` row, so the mode is its own field (#1745): one row
        # per shape, pinned here so a reader that collapsed two of them
        # would disagree with `canonical()` rather than match it.
        self.t.table(
            "    Call { owns [MethodInvocation] pays [] }\n"
            "    Branch { owns [Identifier] pays [LT] }\n"
            "    Container { owns [Volatile] pays never }\n"
        )
        table = rt.parse_table(self.t.roles / "java.rs")
        self.assertEqual(
            rt.dump_table(table, self.t.languages / "language_java.rs"),
            "lang Java\n"
            "Call rows owners=[5] payers=[] wraps=[] excluded=[] mode=split text_gated=false chain_gated=false\n"
            "Branch rows owners=[1] payers=[2] wraps=[] excluded=[] mode=split text_gated=false chain_gated=false\n"
            "Container rows owners=[6] payers=[] wraps=[] excluded=[] mode=never text_gated=false chain_gated=false\n",
        )
        self.t.table("    Call { owns [MethodInvocation] }\n    Branch { pending R07 }\n    Container { absent \"none\" }\n")
        table = rt.parse_table(self.t.roles / "java.rs")
        self.assertEqual(
            rt.dump_table(table, self.t.languages / "language_java.rs"),
            "lang Java\n"
            "Call rows owners=[5] payers=[] wraps=[] excluded=[] mode=owners text_gated=false chain_gated=false\n"
            "Branch pending R07\n"
            "Container absent none\n",
        )

    def test_the_canonical_dump_names_each_context_flag(self) -> None:
        # The two flags after the mode (#1793): `text_gated` from the
        # keyword, `chain_gated` from the absence of `no_chain` on a
        # gate or `opens_by` — each fn marked on its own, so a two-fn
        # owner reads by its unmarked fn whichever is written first.
        # One row per text × chain combination, then the two-fn pairs.
        self.t.table(
            "    Call { owns [MethodInvocation(text_gated_by f no_chain)] }\n"
            "    Branch { owns [Identifier(text_gated_by f)] }\n"
            "    Container { owns [Volatile(gated_by g no_chain opens_by h no_chain)] }\n"
            "    Loop { owns [LT(gated_by g)] }\n"
            "    FuncOpener { owns [LT2(opens_by h gated_by g no_chain)] }\n"
            "    Exit { owns [Hidden(gated_by g opens_by h no_chain)] }\n"
            "    Catch { owns [End(holds_payer gated_by g no_chain)] pays [LT] }\n"
        )
        table = rt.parse_table(self.t.roles / "java.rs")
        self.assertEqual(
            rt.dump_table(table, self.t.languages / "language_java.rs"),
            "lang Java\n"
            "Call rows owners=[5] payers=[] wraps=[] excluded=[] mode=owners text_gated=true chain_gated=false\n"
            "Branch rows owners=[1] payers=[] wraps=[] excluded=[] mode=owners text_gated=true chain_gated=true\n"
            "Container rows owners=[6] payers=[] wraps=[] excluded=[] mode=owners text_gated=false chain_gated=false\n"
            "Loop rows owners=[2] payers=[] wraps=[] excluded=[] mode=owners text_gated=false chain_gated=true\n"
            "FuncOpener rows owners=[3] payers=[] wraps=[] excluded=[] mode=owners text_gated=false chain_gated=true\n"
            "Exit rows owners=[4] payers=[] wraps=[] excluded=[] mode=owners text_gated=false chain_gated=true\n"
            "Catch rows owners=[0] payers=[2] wraps=[] excluded=[] mode=split text_gated=false chain_gated=false\n",
        )


if __name__ == "__main__":
    unittest.main()
