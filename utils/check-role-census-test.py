#!/usr/bin/env python3
"""Self-tests for check-role-census.py: both directions, on synthetic
tables, so a regression in the census or the shared reader fails here
rather than reading as a clean tree.

Run: ``python3 -m unittest -q utils/check-role-census-test.py``.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

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


census = _load("check_role_census", "check-role-census.py")

MOD_RS = """\
macro_rules! role_table {
    (@named Call { $($b:tt)* }) => { pub(crate) mod call { $($b)* } };
    (@named Modifier { $($b:tt)* }) => { pub(crate) mod modifier { $($b)* } };
    (@named Exit { $($b:tt)* }) => { pub(crate) mod exit { $($b)* } };
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
    (
        "python",
        Python,
        "Python",
        "python",
        PythonCode,
        PythonParser,
        tree_sitter_python,
        [py],
    ),
);
"""


class Tree:
    """A scratch roles dir with a mod.rs and langs.rs; ``write`` adds a
    table."""

    def __init__(self) -> None:
        self.dir = pathlib.Path(tempfile.mkdtemp(prefix="role-census-"))
        self.roles = self.dir / "roles"
        self.roles.mkdir()
        (self.roles / "mod.rs").write_text(MOD_RS)
        self.langs = self.dir / "langs.rs"
        self.langs.write_text(LANGS_RS)
        self.baseline = self.dir / "baseline.txt"

    def write(self, stem: str, body: str) -> None:
        (self.roles / f"{stem}.rs").write_text(f"role_table! {{\n{body}}}\n")

    def run(self, *extra: str) -> tuple[int, str]:
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = census.main([
                "--roles-dir", str(self.roles), "--langs-rs", str(self.langs),
                "--baseline", str(self.baseline), *extra,
            ])
        return code, out.getvalue()


FRAGMENT_RS = """\
macro_rules! jvm_calls {
    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {
        $crate::role_table!(@owners $cont [$($acc)* MethodInvocation, ExplicitConstructorInvocation(gated_by in_body),] $($rest)*);
    };
}
"""

JAVA_FULL = "    lang: Java;\n    Call { owns [MethodInvocation] }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n"
PY_FULL = "    lang: Python;\n    Call { owns [Call] }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n"


class CensusTests(unittest.TestCase):
    def setUp(self) -> None:
        self.t = Tree()

    def test_complete_tables_pass_and_count(self) -> None:
        self.t.write("java", JAVA_FULL)
        self.t.write("python", PY_FULL)
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 0, out)
        code, out = self.t.run()
        self.assertEqual(code, 0, out)
        self.assertIn("2 languages × 3 roles, 2 pending, 4 declared or absent", out)
        self.assertIn("role-census: counts declarations", out)

    def test_missing_cell_fails_naming_language_and_role(self) -> None:
        self.t.write("java", "    lang: Java;\n    Call { owns [MethodInvocation] }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("Java Modifier: no row", out)
        self.assertIn("HOLE", out)

    def test_missing_file_fails_not_skips(self) -> None:
        self.t.write("java", JAVA_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("Python: no table", out)

    def test_duplicate_role_is_a_reader_error(self) -> None:
        self.t.write("java", JAVA_FULL + "    Exit { absent \"twice\" }\n")
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("`Exit` declared twice", out)

    def test_unknown_role_is_rejected(self) -> None:
        self.t.write("java", JAVA_FULL + "    Widget { pending R07 }\n")
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("`Widget` is not a role", out)

    def test_conflicting_status_in_one_row_is_rejected(self) -> None:
        self.t.write("java", "    lang: Java;\n    Call { owns [MethodInvocation] absent \"x\" }\n    Modifier { absent \"n\" }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("unexpected `absent", out)

    def test_wrong_lang_in_file_is_rejected(self) -> None:
        self.t.write("java", PY_FULL)
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("declares `lang: Python` but is `Java`'s file", out)

    def test_brace_inside_absent_reason_does_not_end_the_row(self) -> None:
        self.t.write("java", "    lang: Java;\n    Call { owns [MethodInvocation] }\n    Modifier { absent \"a `{` and a } inside\" }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        self.t.run("--bootstrap")
        code, out = self.t.run()
        self.assertEqual(code, 0, out)

    def test_new_pending_cell_fails_and_update_refuses(self) -> None:
        self.t.write("java", JAVA_FULL)
        self.t.write("python", PY_FULL)
        self.t.run("--bootstrap")
        # a declared row regresses to pending
        self.t.write("java", "    lang: Java;\n    Call { pending R08 }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n")
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("Java Call is pending(R08) but not in the baseline", out)
        code, out = self.t.run("--update")
        self.assertEqual(code, 1, "update must not launder a new pending cell")
        self.assertIn("`--update` will not accept them", out)
        self.assertNotIn("Java Call R08", self.t.baseline.read_text())

    def test_swapped_pending_cell_is_not_hidden_by_an_equal_count(self) -> None:
        self.t.write("java", JAVA_FULL)
        self.t.write("python", PY_FULL)
        self.t.run("--bootstrap")
        # Exit declared, Call made pending: count unchanged, set changed
        self.t.write("java", "    lang: Java;\n    Call { pending R08 }\n    Modifier { absent \"none\" }\n    Exit { owns [ReturnStatement] }\n")
        code, out = self.t.run()
        self.assertEqual(code, 1, out)

    def test_update_only_removes(self) -> None:
        self.t.write("java", JAVA_FULL)
        self.t.write("python", PY_FULL)
        self.t.run("--bootstrap")
        self.t.write("java", "    lang: Java;\n    Call { owns [MethodInvocation] }\n    Modifier { absent \"none\" }\n    Exit { owns [ReturnStatement] }\n")
        code, out = self.t.run()
        self.assertEqual(code, 0)
        self.assertIn("no longer pending; run --update", out)
        code, out = self.t.run("--update")
        self.assertEqual(code, 0)
        self.assertIn("removed 1 entry", out)
        self.assertEqual(self.t.baseline.read_text().count("Java Exit"), 0)
        self.assertIn("Python Exit R07", self.t.baseline.read_text())

    def test_missing_baseline_is_fatal(self) -> None:
        self.t.write("java", JAVA_FULL)
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 2)
        self.assertIn("needs --bootstrap", out)
        code, out = self.t.run("--update")
        self.assertEqual(code, 2)

    def test_bootstrap_refuses_an_existing_file(self) -> None:
        self.t.write("java", JAVA_FULL)
        self.t.write("python", PY_FULL)
        self.t.run("--bootstrap")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 2)
        self.assertIn("never re-bootstrap", out)

    def test_bootstrap_refuses_a_deleted_tracked_file(self) -> None:
        # Simulate: baseline tracked at HEAD in a scratch repo, then deleted.
        repo = pathlib.Path(tempfile.mkdtemp(prefix="role-census-git-"))
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        bl = repo / ".role-census-baseline.txt"
        bl.write_text("# x\n")
        subprocess.run(["git", "-C", str(repo), "add", "."], check=True)
        subprocess.run(
            ["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@x", "commit", "-q", "-m", "b"],
            check=True,
        )
        bl.unlink()
        saved = census.REPO_ROOT
        try:
            census.REPO_ROOT = repo
            self.t.write("java", JAVA_FULL)
            self.t.write("python", PY_FULL)
            code, out = self.t.run("--bootstrap", "--baseline", str(bl))
        finally:
            census.REPO_ROOT = saved
        self.assertEqual(code, 2)
        self.assertIn("tracked at HEAD", out)

    def test_malformed_baseline_line_is_fatal(self) -> None:
        self.t.write("java", JAVA_FULL)
        self.t.write("python", PY_FULL)
        self.t.baseline.write_text("# hdr\nJava Exit\n")
        code, out = self.t.run()
        self.assertEqual(code, 2)
        self.assertIn("expected `<language> <role> <package>`", out)

    def test_quoted_text_in_a_comment_beside_a_reason_is_not_a_literal(self) -> None:
        self.t.write("java", "    lang: Java;\n    Call { owns [MethodInvocation] }\n    Modifier { absent \"none\" /* \"quoted rationale\" */ }\n    // \"another\"\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        self.t.run("--bootstrap")
        code, out = self.t.run()
        self.assertEqual(code, 0, out)

    def test_escapes_in_a_reason_decode_as_rust_does(self) -> None:
        self.t.write("java", "    lang: Java;\n    Call { owns [MethodInvocation] }\n    Modifier { absent \"a \\\"quoted\\\" word\\nsecond line\" }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        table = rt.parse_table(self.t.roles / "java.rs")
        self.assertEqual(table.rows["Modifier"].reason, 'a "quoted" word\nsecond line')

    def test_an_unsupported_escape_is_an_error_not_a_different_string(self) -> None:
        self.t.write("java", "    lang: Java;\n    Call { owns [MethodInvocation] }\n    Modifier { absent \"tab\\u{9}\" }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("unsupported escape", out)

    def test_a_language_row_with_nested_parentheses_in_its_description_is_still_read(self) -> None:
        self.t.langs.write_text(LANGS_RS.replace('"The Java language"', '"The Java language ((example) and (another))"'))
        self.t.write("java", JAVA_FULL)
        self.t.write("python", PY_FULL)
        self.assertEqual([l.variant for l in rt.languages(self.t.langs)], ["Java", "Python"])
        self.t.run("--bootstrap")
        code, out = self.t.run()
        self.assertEqual(code, 0, out)

    def test_an_unreadable_language_row_fails_rather_than_vanishing(self) -> None:
        self.t.langs.write_text(LANGS_RS.replace("        tree_sitter_java,\n", ""))
        with self.assertRaises(rt.RoleTableError) as ctx:
            rt.languages(self.t.langs)
        self.assertIn("names no `tree_sitter_*` grammar", str(ctx.exception))
        self.t.write("java", JAVA_FULL)
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 2)

    def test_a_fragment_splices_its_kinds_in_place(self) -> None:
        # The fragment's kinds land where `..name!()` sits, in list
        # order, and the row's own entries keep their attributes.
        (self.t.roles / "jvm.rs").write_text(FRAGMENT_RS)
        self.t.write("java", "    lang: Java;\n    Call { owns [..jvm_calls!(), ObjectCreation(opens Function)] }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        table = rt.parse_table(self.t.roles / "java.rs")
        owners = table.rows["Call"].owners
        self.assertEqual([o.kind for o in owners], ["MethodInvocation", "ExplicitConstructorInvocation", "ObjectCreation"])
        self.assertEqual(owners[1].gate, "in_body")
        self.assertEqual(owners[2].opens, "Function")
        self.t.run("--bootstrap")
        code, out = self.t.run()
        self.assertEqual(code, 0, out)

    def test_a_fragment_splices_payers_with_their_inline_gates(self) -> None:
        # A `pays [..]` list walks the same splice (#1694): the fragment's
        # payers land in order where `..name!()` sits, each keeping the
        # gate it spells inline, and the row's own gated payer after it
        # is read the same way.
        (self.t.roles / "tokens.rs").write_text(
            "macro_rules! equality_tokens {\n"
            "    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {\n"
            "        $crate::role_table!(@owners $cont [$($acc)* EQEQ, BANGEQ gated_by applied,] $($rest)*);\n"
            "    };\n"
            "}\n"
        )
        self.t.write(
            "java",
            "    lang: Java;\n"
            "    Call { owns [MethodInvocation] pays [..equality_tokens!(), LT gated_by under_binary] }\n"
            "    Modifier { absent \"none\" }\n    Exit { pending R07 }\n",
        )
        self.t.write("python", PY_FULL)
        row = rt.parse_table(self.t.roles / "java.rs").rows["Call"]
        self.assertEqual(row.pays_mode, "split")
        self.assertEqual([(p.kind, p.gate) for p in row.payers], [("EQEQ", None), ("BANGEQ", "applied"), ("LT", "under_binary")])

    def test_holds_payer_and_under_are_read(self) -> None:
        # `holds_payer` on an owner and `under P` / `under [P, Q]` on a
        # payer (#1695) — the second inside a fragment, whose `]` must not
        # end the fragment's list early.
        (self.t.roles / "jvm.rs").write_text(
            "macro_rules! ordering_tokens {\n"
            "    (@splice $cont:tt [$($acc:tt)*] $($rest:tt)*) => {\n"
            "        $crate::role_table!(@owners $cont [$($acc)* LT under [BinaryExpression, Fold], GT under Fold,] $($rest)*);\n"
            "    };\n"
            "}\n"
        )
        self.t.write(
            "java",
            "    lang: Java;\n"
            "    Call { owns [MethodInvocation(holds_payer), Lambda(holds_payer gated_by applied)] "
            "pays [EQEQ, ..ordering_tokens!(), Instanceof under Call] project \"operator\" }\n"
            "    Modifier { absent \"none\" }\n    Exit { pending R07 }\n",
        )
        self.t.write("python", PY_FULL)
        row = rt.parse_table(self.t.roles / "java.rs").rows["Call"]
        self.assertEqual([(o.kind, o.holds_payer, o.gate) for o in row.owners], [("MethodInvocation", True, None), ("Lambda", True, "applied")])
        self.assertEqual(
            [(p.kind, p.gate, p.under) for p in row.payers],
            [("EQEQ", None, ()), ("LT", None, ("BinaryExpression", "Fold")), ("GT", None, ("Fold",)), ("Instanceof", None, ("Call",))],
        )

    def test_a_fragment_is_shared_across_tables(self) -> None:
        (self.t.roles / "jvm.rs").write_text(FRAGMENT_RS)
        self.t.write("java", "    lang: Java;\n    Call { owns [..jvm_calls!()] }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n")
        self.t.write("python", "    lang: Python;\n    Call { owns [Call, ..jvm_calls!()] }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n")
        tables = rt.load_tables(self.t.roles, rt.languages(self.t.langs))
        self.assertEqual([o.kind for o in tables["Java"].rows["Call"].owners], ["MethodInvocation", "ExplicitConstructorInvocation"])
        self.assertEqual([o.kind for o in tables["Python"].rows["Call"].owners], ["Call", "MethodInvocation", "ExplicitConstructorInvocation"])

    def test_an_unknown_fragment_is_an_error_not_an_empty_splice(self) -> None:
        self.t.write("java", "    lang: Java;\n    Call { owns [..no_such!()] }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("`..no_such!()` names no fragment", out)

    def test_a_fragment_defined_twice_is_an_error(self) -> None:
        (self.t.roles / "jvm.rs").write_text(FRAGMENT_RS)
        (self.t.roles / "jvm2.rs").write_text(FRAGMENT_RS)
        self.t.write("java", JAVA_FULL)
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("fragment `jvm_calls` is defined twice", out)

    def test_a_fragment_inside_a_test_module_is_not_visible_to_a_row(self) -> None:
        # rustc rejects a production row splicing a `#[cfg(test)]`
        # fragment; the reader must reject it too, or the differential
        # test — which compares only rows that compile — cannot see the
        # disagreement.
        (self.t.roles / "jvm.rs").write_text("#[cfg(test)]\nmod tests {\n" + FRAGMENT_RS + "}\n")
        self.t.write("java", "    lang: Java;\n    Call { owns [..jvm_calls!()] }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("`..jvm_calls!()` names no fragment", out)

    def test_a_spaced_cfg_test_module_is_still_a_test_module(self) -> None:
        (self.t.roles / "jvm.rs").write_text("#[cfg ( test )]\nmod tests {\n" + FRAGMENT_RS + "}\n")
        self.t.write("java", "    lang: Java;\n    Call { owns [..jvm_calls!()] }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("`..jvm_calls!()` names no fragment", out)

    def test_a_cfg_that_is_not_exactly_test_does_not_hide_a_fragment(self) -> None:
        # `cfg(any(test, feature = "x"))` compiles into a production
        # build with the feature on, so its fragment is a real one.
        (self.t.roles / "jvm.rs").write_text('#[cfg(any(test, feature = "x"))]\nmod maybe {\n' + FRAGMENT_RS + "}\n")
        self.t.write("java", "    lang: Java;\n    Call { owns [..jvm_calls!()] }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        self.t.run("--bootstrap")
        code, out = self.t.run()
        self.assertEqual(code, 0, out)

    def test_a_macro_without_a_splice_arm_is_not_a_fragment(self) -> None:
        (self.t.roles / "helper.rs").write_text("macro_rules! jvm_calls {\n    ($x:ident) => { $x };\n}\n")
        self.t.write("java", "    lang: Java;\n    Call { owns [..jvm_calls!()] }\n    Modifier { absent \"none\" }\n    Exit { pending R07 }\n")
        self.t.write("python", PY_FULL)
        code, out = self.t.run()
        self.assertEqual(code, 1)
        self.assertIn("`..jvm_calls!()` names no fragment", out)

    def test_output_is_deterministic_across_row_order(self) -> None:
        # The same cells declared in a different row order, and the
        # tables written in a different file order, must give the same
        # baseline bytes and the same matrix: the baseline is sorted and
        # the matrix follows `mk_langs!`, not the files.
        # Python listed first in `mk_langs!`, so the pending set is built
        # Python-first and only sorting puts Java first in the file.
        py_first = LANGS_RS.replace(LANGS_RS[LANGS_RS.index("    (\n        \"java\""):LANGS_RS.index("    (\n        \"python\"")], "")
        py_first = py_first.replace(");\n", LANGS_RS[LANGS_RS.index("    (\n        \"java\""):LANGS_RS.index("    (\n        \"python\"")] + ");\n")
        self.t.langs.write_text(py_first)
        self.t.write("python", PY_FULL)
        self.t.write("java", JAVA_FULL)
        self.t.run("--bootstrap")
        first = self.t.baseline.read_text()
        _, out1 = self.t.run("--matrix")
        self.t.baseline.unlink()
        reordered = "    lang: Java;\n    Exit { pending R07 }\n    Modifier { absent \"none\" }\n    Call { owns [MethodInvocation] }\n"
        self.t.write("java", reordered)
        self.t.write("python", PY_FULL)
        self.t.run("--bootstrap")
        self.assertEqual(first, self.t.baseline.read_text())
        _, out2 = self.t.run("--matrix")
        self.assertEqual(out1, out2)
        # The entries are sorted, not in the order the matrix met them:
        # `Java` is listed before `Python` whatever the file order.
        entries = [l for l in first.splitlines() if l and not l.startswith("#")]
        self.assertEqual(entries, ["Java Exit R07", "Python Exit R07"])


if __name__ == "__main__":
    unittest.main()
