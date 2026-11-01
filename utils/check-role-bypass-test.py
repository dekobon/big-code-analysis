#!/usr/bin/env python3
"""Self-tests for check-role-bypass.py on synthetic sources: every
supported signal, aliases and macro shapes, the things that must not
count (comments, strings, test modules, unrelated kinds, outside the
boundary), exemption scope and role check, the ratchet, and
determinism.

Run: ``python3 -m unittest -q utils/check-role-bypass-test.py``.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import pathlib
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


byp = _load("check_role_bypass", "check-role-bypass.py")

MOD_RS = """\
macro_rules! role_table {
    (@named Call { $($b:tt)* }) => { pub(crate) mod call { $($b)* } };
    (@named Exit { $($b:tt)* }) => { pub(crate) mod exit { $($b)* } };
    (@named Comparison { $($b:tt)* }) => { pub(crate) mod comparison { $($b)* } };
}
"""
LANGS_RS = """\
mk_langs!(
    ("java", Java, "Java", "java", JavaCode, JavaParser, tree_sitter_java, [java]),
    ("python", Python, "Python", "python", PythonCode, PythonParser, tree_sitter_python, [py]),
);
"""


def enum_rs(name: str, kinds: list[tuple[str, str]]) -> str:
    lines = [f"pub enum {name} {{"] + [f"    {v} = {i}," for i, (v, _) in enumerate(kinds)] + [f"    Error = {len(kinds)},", "}"]
    lines.append(f"pub const {name.upper()}_KIND_FLAGS: &[(u16, bool, bool)] = &[")
    lines += [f"    ({i}, true, true)," for i in range(len(kinds))] + ["];"]
    lines.append(f"impl From<{name}> for &'static str {{\n    fn from(tok: {name}) -> Self {{\n        match tok {{")
    lines += [f'            {name}::{v} => "{k}",' for v, k in kinds] + [f'            {name}::Error => "ERROR",', "        }\n    }\n}"]
    return "\n".join(lines) + "\n"


JAVA_KINDS = [("End", "end"), ("MethodInvocation", "method_invocation"), ("ReturnStatement", "return_statement"), ("LT", "<"), ("Other", "other")]
PY_KINDS = [("End", "end"), ("Call", "call"), ("ReturnStatement", "return_statement")]


class Tree:
    def __init__(self) -> None:
        self.dir = pathlib.Path(tempfile.mkdtemp(prefix="role-byp-"))
        self.roles = self.dir / "roles"
        self.roles.mkdir()
        (self.roles / "mod.rs").write_text(MOD_RS)
        self.langs = self.dir / "langs.rs"
        self.langs.write_text(LANGS_RS)
        self.languages = self.dir / "languages"
        self.languages.mkdir()
        (self.languages / "language_java.rs").write_text(enum_rs("Java", JAVA_KINDS))
        (self.languages / "language_python.rs").write_text(enum_rs("Python", PY_KINDS))
        (self.roles / "java.rs").write_text('role_table! {\n    lang: Java;\n    Call { owns [MethodInvocation] }\n    Exit { owns [ReturnStatement] }\n    Comparison { owns [] pays [LT] }\n}\n')
        (self.roles / "python.rs").write_text('role_table! {\n    lang: Python;\n    Call { owns [Call] }\n    Exit { owns [ReturnStatement] }\n    Comparison { absent "n" }\n}\n')
        self.baseline = self.dir / "baseline.txt"
        self.src_dir = self.dir / "src"
        self.src_dir.mkdir()
        self.files: list[pathlib.Path] = []

    def src(self, name: str, text: str) -> pathlib.Path:
        p = self.src_dir / name
        p.write_text(text)
        self.files.append(p)
        return p

    def run(self, *extra: str) -> tuple[int, str]:
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = byp.main([
                "--roles-dir", str(self.roles), "--langs-rs", str(self.langs),
                "--languages-dir", str(self.languages), "--baseline", str(self.baseline),
                "--files", *map(str, self.files), *extra,
            ])
        return code, out.getvalue()

    def findings(self) -> list[str]:
        """``lang::kind via shape`` per finding, from a bootstrap into a scratch baseline."""
        if self.baseline.exists():
            self.baseline.unlink()
        code, out = self.run("--bootstrap", "--show")
        assert code == 0, out
        return sorted(l.strip().split("] ", 1)[1] for l in out.splitlines() if l.startswith("  "))


class BypassTests(unittest.TestCase):
    def setUp(self) -> None:
        self.t = Tree()

    def test_every_signal_shape_is_found(self) -> None:
        self.t.src("a.rs", """
use crate::Java;
fn f(node: &Node) -> bool {
    if matches!(node.kind_id().into(), Java::MethodInvocation | Java::Other) { return true; }
    match node.kind_id().into() {
        Java::ReturnStatement => true,
        _ => false,
    }
}
fn g(node: &Node) -> bool { node.kind_id() == Java::MethodInvocation as u16 }
fn h(node: &Node) -> bool { Java::ReturnStatement == node.kind_id() }
fn s(node: &Node) -> bool { node.kind() == "method_invocation" }
const KINDS: &[u16] = &[Java::LT as u16, Java::Other as u16];
""")
        self.assertEqual(self.t.findings(), sorted([
            "Java::MethodInvocation via matches",
            "Java::ReturnStatement via arm",
            "Java::MethodInvocation via kind_id",
            "Java::ReturnStatement via kind_id",
            "Java::MethodInvocation via kind_str",
            "Java::LT via const",
        ]))

    def test_glob_import_resolves_bare_variants(self) -> None:
        self.t.src("a.rs", "use Java::*;\nfn f(n: &Node) -> bool { matches!(n.kind_id().into(), MethodInvocation | Other) }\n")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via matches"])

    def test_alias_import_resolves(self) -> None:
        self.t.src("a.rs", "use crate::Java as J;\nfn f(n: &Node) -> bool { n.kind_id() == J::MethodInvocation as u16 }\n")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via kind_id"])

    def test_unclaimed_kinds_and_other_languages_do_not_count(self) -> None:
        self.t.src("a.rs", "use crate::Java;\nfn f(n: &Node) -> bool { n.kind_id() == Java::Other as u16 }\nfn g(n: &Node) -> bool { n.kind() == \"other\" }\n")
        self.assertEqual(self.t.findings(), [])

    def test_comments_strings_and_tests_do_not_count(self) -> None:
        self.t.src("a.rs", """
use crate::Java;
// matches!(x, Java::MethodInvocation)
/* node.kind_id() == Java::ReturnStatement */
fn f() -> &'static str { "node.kind_id() == Java::MethodInvocation" }
fn fixture() -> &'static str { "x.kind() == \\"method_invocation\\"" }
#[cfg(test)]
mod tests {
    fn t(n: &Node) -> bool { n.kind_id() == Java::MethodInvocation as u16 }
}
""")
        self.assertEqual(self.t.findings(), [])

    def test_exemption_covers_its_statement_only_and_must_name_the_role(self) -> None:
        self.t.src("a.rs", """
use crate::Java;
fn name(n: &Node) -> bool {
    // role-bypass: allow(Call) — naming, not membership
    if n.kind_id() == Java::MethodInvocation as u16 { return true; }
    n.kind_id() == Java::ReturnStatement as u16
}
""")
        self.assertEqual(self.t.findings(), ["Java::ReturnStatement via kind_id"])
        self.t.files.clear()
        self.t.src("b.rs", "use crate::Java;\nfn f(n: &Node) -> bool {\n    // role-bypass: allow(Exit) — wrong role\n    n.kind_id() == Java::MethodInvocation as u16\n}\n")
        self.t.baseline.unlink()
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 1)
        self.assertIn("exemption names ['Exit'] but `Java::MethodInvocation` is claimed by ['Call']", out)

    def test_exemption_covers_a_multiline_matches(self) -> None:
        self.t.src("a.rs", """
use crate::Java;
fn f(n: &Node) -> bool {
    // role-bypass: allow(Call, Exit) — reason
    matches!(
        n.kind_id().into(),
        Java::MethodInvocation | Java::ReturnStatement
    )
}
""")
        self.assertEqual(self.t.findings(), [])

    def test_exemption_in_a_string_or_block_comment_is_not_honoured(self) -> None:
        self.t.src("a.rs", """
use crate::Java;
fn f(n: &Node) -> bool {
    let _ = "// role-bypass: allow(Call) — fake";
    n.kind_id() == Java::MethodInvocation as u16
}
""")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via kind_id"])

    def test_macro_with_lang_argument_attributes_its_kinds(self) -> None:
        self.t.src("a.rs", """
macro_rules! impl_simple_is_string {
    ($lang:ident, $first:ident $(, $rest:ident)*) => {
        fn is_string(node: &Node) -> bool { matches!(node.kind_id().into(), $lang::$first $(| $lang::$rest)*) }
    };
}
impl_simple_is_string!(Java, MethodInvocation, Other);
impl_simple_is_string!(Python, Call);
""")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via macro_arg", "Python::Call via macro_arg"])

    def test_macro_body_with_lang_glob_is_attributed_per_invocation(self) -> None:
        self.t.src("a.rs", """
macro_rules! impl_cyc {
    ($code:ty, $lang:ident) => {
        impl Cyc for $code {
            fn compute(node: &Node) -> bool {
                use $lang::*;
                match node.kind_id().into() {
                    ReturnStatement => true,
                    _ => false,
                }
            }
        }
    };
}
impl_cyc!(JavaCode, Java);
impl_cyc!(PythonCode, Python);
""")
        self.assertEqual(self.t.findings(), ["Java::ReturnStatement via arm", "Python::ReturnStatement via arm"])

    def test_inner_helper_macro_inherits_the_languages(self) -> None:
        self.t.src("a.rs", """
macro_rules! is_x {
    ($node:ident) => { matches!($node.kind_id().into(), ReturnStatement) };
}
macro_rules! outer {
    ($language:ident) => {
        fn f(node: &Node) -> bool { use $language::*; is_x!(node) }
    };
}
outer!(Java);
""")
        self.assertEqual(self.t.findings(), ["Java::ReturnStatement via matches"])

    def test_string_compare_in_a_macro_body_follows_the_invocations_not_the_spelling(self) -> None:
        # `kind() == "call"` inside a `$lang` macro invoked only with a
        # language that declares no rows must not be attributed to Python
        # merely because Python spells a claimed kind `call`.
        self.t.src("a.rs", """
macro_rules! target {
    ($lang:ident, $node:ident) => {{ use $lang::*; $node.kind() == "call" }};
}
fn f(n: &Node) -> bool { target!(Java, n) }
""")
        self.assertEqual(self.t.findings(), [])

    def test_unreachable_kind_reading_macro_is_a_problem_not_silence(self) -> None:
        self.t.src("a.rs", "macro_rules! lone {\n    ($node:ident) => { matches!($node.kind_id().into(), ReturnStatement) };\n}\n")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 1)
        self.assertIn("`macro_rules! lone` reads kinds but no language reaches it", out)

    def test_row_forwarding_macro_is_not_an_unsupported_problem(self) -> None:
        # `keyword_roles!` / `paying_roles!`: the body reads `kind_id()`
        # only to hand it to a row, so it classifies nothing itself.
        self.t.src("a.rs", """
macro_rules! paying_roles {
    ($roles:ident) => {
        fn get_paying_role(node: &Node, code: &[u8], a: Ancestors) -> bool {
            use crate::roles::$roles::comparison;
            let kind = node.kind_id();
            comparison::pays_at_leaf_kind(kind, node, code, a)
        }
    };
}
paying_roles!(java);
""")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 0, out)

    def test_macro_handed_its_rows_as_idents_is_a_forwarder_too(self) -> None:
        # `rows_own!` in `abc.rs`: the rows arrive as `$row:ident`
        # arguments rather than through `roles::$lang::`, and the body
        # reads `kind_id()` once to hand it to each — the same forward.
        self.t.src("a.rs", """
macro_rules! rows_own {
    ($expr:expr, $code:expr; $($row:ident),+ $(,)?) => {{
        let unknown = Ancestors::unknown();
        let kind = $expr.kind_id();
        false $(|| $row::owns_kind(kind, $expr, $code, unknown))+
    }};
}
fn f(node: &Node, code: &[u8]) -> bool { rows_own!(node, code; comparison, short_circuit) }
""")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 0, out)

    def test_macro_forwarding_one_kind_to_two_rows_is_a_forwarder(self) -> None:
        # The R09 `is_comment` shape (#1699): one cached kind handed to
        # two rows' `lists_kind`, the union being the whole answer. A
        # `const` assertion beside the forward reads no kind and must
        # not make the body a classifier.
        self.t.src("a.rs", """
macro_rules! impl_comment_from_rows {
    ($roles:ident) => {
        fn is_comment(node: &Node) -> bool {
            const _: () = assert!(!$crate::roles::$roles::call::TEXT_GATED, "no bytes here");
            let kind = node.kind_id();
            $crate::roles::$roles::call::lists_kind(kind)
                || $crate::roles::$roles::exit::lists_kind(kind)
        }
    };
}
impl_comment_from_rows!(java);
""")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 0, out)

    def test_macro_that_reads_a_row_and_still_matches_kinds_is_unsupported(self) -> None:
        # A half-migrated classifier: the row read beside a `match` on
        # the kind. The row mention must not launder the match.
        self.t.src("a.rs", """
macro_rules! mixed {
    ($roles:ident, $kind:ident) => {
        fn classify(node: &Node, code: &[u8], a: Ancestors) -> bool {
            use crate::roles::$roles::comparison;
            match node.kind_id().into() {
                $kind => true,
                _ => comparison::owns(node, code, a),
            }
        }
    };
}
mixed!(java, LT);
""")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 1)
        self.assertIn("`macro_rules! mixed` reads kinds but no language reaches it", out)

    def test_macro_that_reads_a_row_and_compares_the_cached_kind_is_unsupported(self) -> None:
        # The cached-kind comparison the lint cannot resolve, beside a
        # row read: still a classifier, still reported.
        self.t.src("a.rs", """
macro_rules! mixed {
    ($roles:ident, $kind:ident) => {
        fn classify(node: &Node, code: &[u8], a: Ancestors) -> bool {
            use crate::roles::$roles::comparison;
            let kind = node.kind_id();
            kind == $kind || comparison::owns_kind(kind, node, code, a)
        }
    };
}
mixed!(java, LT);
""")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 1)
        self.assertIn("`macro_rules! mixed` reads kinds but no language reaches it", out)

    def test_uninvoked_kind_set_macro_is_not_an_unsupported_problem(self) -> None:
        self.t.src("a.rs", "macro_rules! java_calls {\n    () => { $crate::Java::MethodInvocation | $crate::Java::Other };\n}\n")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 0, out)

    def test_ratchet_new_site_fails_update_refuses_and_swap_is_caught(self) -> None:
        a = self.t.src("a.rs", "use crate::Java;\nfn f(n: &Node) -> bool { n.kind_id() == Java::MethodInvocation as u16 }\n")
        self.t.run("--bootstrap")
        a.write_text("use crate::Java;\nfn f(n: &Node) -> bool { n.kind_id() == Java::ReturnStatement as u16 }\n")
        code, out = self.t.run()
        self.assertEqual(code, 1, "one site swapped for another must fail despite an equal count")
        self.assertIn("restates `Java::ReturnStatement` (kind_id)", out)
        code, out = self.t.run("--update")
        self.assertEqual(code, 1)
        self.assertNotIn("ReturnStatement", self.t.baseline.read_text())
        a.write_text("use crate::Java;\nfn f(n: &Node) -> bool { call::owns(n) }\n")
        code, out = self.t.run()
        self.assertEqual(code, 0)
        self.assertIn("no longer found; run --update", out)
        code, out = self.t.run("--update")
        self.assertEqual(code, 0)
        self.assertNotIn("MethodInvocation", self.t.baseline.read_text())

    def test_missing_and_malformed_baseline_are_fatal(self) -> None:
        self.t.src("a.rs", "fn f() {}\n")
        code, _ = self.t.run()
        self.assertEqual(code, 2)
        self.t.baseline.write_text("a.rs\tfn f\tJava\tX\n")
        code, out = self.t.run()
        self.assertEqual(code, 2)
        self.assertIn("expected 5 tab-separated fields", out)

    def test_output_is_deterministic(self) -> None:
        self.t.src("b.rs", "use crate::Java;\nfn f(n: &Node) -> bool { n.kind_id() == Java::MethodInvocation as u16 }\n")
        self.t.src("a.rs", "use crate::Java;\nfn g(n: &Node) -> bool { n.kind_id() == Java::ReturnStatement as u16 }\n")
        self.t.run("--bootstrap")
        first = self.t.baseline.read_text()
        self.t.baseline.unlink()
        self.t.files.reverse()
        self.t.run("--bootstrap")
        self.assertEqual(first, self.t.baseline.read_text())

    def test_multiline_arm_patterns_are_scanned_whole(self) -> None:
        # The live getter's shape: alternatives on their own lines, a
        # guard, and a block value holding a nested match.
        self.t.src("a.rs", """
use Java::*;
fn f(n: &Node) -> X {
    match n.kind_id().into() {
        MethodInvocation
        | Other => X::A,
        ReturnStatement if guard(n) => {
            match n.kind_id().into() { Other => X::B, _ => X::C }
        }
        LT => X::D,
        _ => X::E,
    }
}
""")
        self.assertEqual(self.t.findings(), ["Java::LT via arm", "Java::MethodInvocation via arm", "Java::ReturnStatement via arm"])

    def test_kind_set_macro_invoked_in_a_match_expands_to_its_kinds(self) -> None:
        self.t.src("a.rs", """
macro_rules! java_calls {
    () => { $crate::Java::MethodInvocation | $crate::Java::Other };
}
fn f(n: &Node) -> bool { matches!(n.kind_id().into(), java_calls!()) }
fn g(n: &Node) -> bool { match n.kind_id().into() { java_calls!() => true, _ => false } }
""")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via arm", "Java::MethodInvocation via matches"])

    def test_macro_naming_lang_kind_qualified_is_attributed_per_invocation(self) -> None:
        self.t.src("a.rs", """
macro_rules! calls {
    ($lang:ident, $n:ident) => { matches!($n.kind_id().into(), $lang::MethodInvocation) };
}
fn f(n: &Node) -> bool { calls!(Java, n) }
""")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via matches"])

    def test_second_site_of_the_same_kind_in_one_item_is_a_new_entry(self) -> None:
        a = self.t.src("a.rs", "use crate::Java;\nfn f(n: &Node, m: &Node) -> bool { n.kind_id() == Java::MethodInvocation as u16 }\n")
        self.t.run("--bootstrap")
        a.write_text("use crate::Java;\nfn f(n: &Node, m: &Node) -> bool { n.kind_id() == Java::MethodInvocation as u16 || m.kind_id() == Java::MethodInvocation as u16 }\n")
        code, out = self.t.run()
        self.assertEqual(code, 1, out)
        self.assertIn("restates `Java::MethodInvocation` (kind_id)", out)
        code, _ = self.t.run("--update")
        self.assertEqual(code, 1, "update must not accept the second site")
        self.assertNotIn("kind_id#2", self.t.baseline.read_text())

    def test_glob_import_is_lexically_scoped(self) -> None:
        self.t.src("a.rs", """
fn j(n: &Node) -> bool { use Java::*; matches!(n.kind_id().into(), ReturnStatement) }
fn p(n: &Node) -> bool { use Python::*; matches!(n.kind_id().into(), ReturnStatement) }
""")
        self.assertEqual(self.t.findings(), ["Java::ReturnStatement via matches", "Python::ReturnStatement via matches"])

    def test_bare_variants_under_a_glob_count_in_equality_and_const(self) -> None:
        self.t.src("a.rs", "use Java::*;\nfn f(n: &Node) -> bool { n.kind_id() == MethodInvocation as u16 }\nconst K: &[u16] = &[ReturnStatement as u16];\n")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via kind_id", "Java::ReturnStatement via const"])

    def test_string_compare_with_no_language_in_scope_is_reported_not_guessed(self) -> None:
        self.t.src("a.rs", "fn f(n: &Node) -> bool { n.kind() == \"call\" }\n")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 1)
        self.assertIn("spells a role-claimed kind but no language is in scope", out)

    def test_string_compare_with_no_language_in_scope_is_satisfied_by_an_exemption(self) -> None:
        self.t.src("a.rs", "fn f(n: &Node) -> bool {\n    // role-bypass: allow(Call) — a name-keyed read that serves every language\n    n.kind() == \"call\"\n}\n")
        self.assertEqual(self.t.findings(), [])

    def test_string_compare_with_no_language_in_scope_rejects_a_wrong_role_exemption(self) -> None:
        self.t.src("a.rs", "fn f(n: &Node) -> bool {\n    // role-bypass: allow(Exit) — wrong role\n    n.kind() == \"call\"\n}\n")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 1, out)
        self.assertIn("exemption names ['Exit']", out)

    def test_string_compare_follows_a_plain_use_of_the_language(self) -> None:
        self.t.src("a.rs", "use crate::Java;\nfn f(n: &Node) -> bool { n.kind() == \"call\" }\nfn g(n: &Node) -> bool { n.kind() == \"method_invocation\" }\n")
        # `call` is Python's spelling; with Java in scope it is not a finding
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via kind_str"])

    def test_exemption_without_a_reason_is_a_problem(self) -> None:
        self.t.src("a.rs", "use crate::Java;\nfn f(n: &Node) -> bool {\n    // role-bypass: allow(Call)\n    n.kind_id() == Java::MethodInvocation as u16\n}\n")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 1)
        self.assertIn("non-empty reason", out)

    def test_raw_string_kind_compare_is_found(self) -> None:
        self.t.src("a.rs", "use crate::Java;\nfn f(n: &Node) -> bool { n.kind() == r#\"method_invocation\"# }\n")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via kind_str"])

    def test_enclosing_item_is_the_innermost_brace_block(self) -> None:
        self.t.src("a.rs", """
use crate::Java;
fn outer(n: &Node) -> bool {
    fn inner(m: &Node) -> bool { m.kind_id() == Java::ReturnStatement as u16 }
    n.kind_id() == Java::MethodInvocation as u16
}
""")
        self.t.run("--bootstrap", "--show")
        text = self.t.baseline.read_text()
        self.assertIn("fn inner\tJava\tReturnStatement", text)
        self.assertIn("fn outer\tJava\tMethodInvocation", text)

    def test_macro_equality_and_string_forms_follow_the_invocations(self) -> None:
        self.t.src("a.rs", """
macro_rules! eq {
    ($lang:ident, $n:ident) => { $n.kind_id() == $lang::MethodInvocation as u16 };
}
macro_rules! rev {
    ($lang:ident, $n:ident) => { $lang::LT as u16 == $n.kind_id() };
}
macro_rules! st {
    ($lang:ident, $n:ident) => {{ use $lang::*; $n.kind() == "return_statement" }};
}
fn f(n: &Node) -> bool { eq!(Java, n) || st!(Java, n) || rev!(Java, n) }
""")
        self.assertEqual(self.t.findings(), ["Java::LT via kind_id", "Java::MethodInvocation via kind_id", "Java::ReturnStatement via kind_str"])

    def test_macro_with_a_lang_argument_it_never_uses_is_unsupported(self) -> None:
        self.t.src("a.rs", """
macro_rules! st {
    ($lang:ident, $n:ident) => { $n.kind() == "method_invocation" };
}
fn f(n: &Node) -> bool { st!(Java, n) }
""")
        code, out = self.t.run("--bootstrap")
        self.assertEqual(code, 1)
        self.assertIn("takes a `$lang` it never spells in its body", out)

    def test_raw_string_with_several_hashes_is_found(self) -> None:
        self.t.src("a.rs", "use crate::Java;\nfn f(n: &Node) -> bool { n.kind() == r##\"method_invocation\"## }\n")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via kind_str"])

    def test_brace_group_import_names_the_language(self) -> None:
        self.t.src("a.rs", "use crate::{Ancestors, Java as J, Node, Python};\nfn f(n: &Node) -> bool { n.kind() == \"call\" || n.kind_id() == J::MethodInvocation as u16 }\n")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via kind_id", "Python::Call via kind_str"])

    def test_inner_alias_shadows_an_outer_one_whatever_the_syntax(self) -> None:
        self.t.src("a.rs", """
use crate::Java as J;
fn f(n: &Node) -> bool {
    use crate::{Python as J};
    n.kind_id() == J::Call as u16
}
fn g(n: &Node) -> bool { n.kind_id() == J::MethodInvocation as u16 }
""")
        self.assertEqual(self.t.findings(), ["Java::MethodInvocation via kind_id", "Python::Call via kind_id"])

    def test_a_test_module_declared_inside_an_inline_module_resolves_under_it(self) -> None:
        parent = self.t.src("a.rs", "mod java;\n#[cfg(test)]\nmod tests {\n    #[cfg(test)]\n    mod java;\n}\n")
        (self.t.src_dir / "a" / "tests").mkdir(parents=True)
        prod = self.t.src_dir / "a" / "java.rs"
        prod.write_text("use crate::Java;\nfn p(n: &Node) -> bool { n.kind_id() == Java::LT as u16 }\n")
        test = self.t.src_dir / "a" / "tests" / "java.rs"
        test.write_text("use crate::Java;\nfn t(n: &Node) -> bool { n.kind_id() == Java::MethodInvocation as u16 }\n")
        skip = byp.test_only_files([parent, prod, test])
        self.assertEqual(skip, {test.resolve()}, "the production a/java.rs must not be excluded")

    def test_out_of_line_test_module_file_is_not_scanned(self) -> None:
        parent = self.t.src("abc.rs", "#[cfg(test)]\nmod slot_tests;\n#[cfg(all(test, feature = \"java\"))]\nmod other_tests;\nfn f() {}\n")
        (self.t.src_dir / "abc").mkdir()
        t1 = self.t.src_dir / "abc" / "slot_tests.rs"
        t1.write_text("use crate::Java;\nfn t(n: &Node) -> bool { n.kind_id() == Java::MethodInvocation as u16 }\n")
        t2 = self.t.src_dir / "abc" / "other_tests.rs"
        t2.write_text("use crate::Java;\nfn t(n: &Node) -> bool { n.kind_id() == Java::ReturnStatement as u16 }\n")
        prod = self.t.src_dir / "abc" / "java.rs"
        prod.write_text("use crate::Java;\nfn p(n: &Node) -> bool { n.kind_id() == Java::LT as u16 }\n")
        files = [parent, t1, t2, prod]
        skip = byp.test_only_files(files)
        self.assertEqual(skip, {t1.resolve(), t2.resolve()})
        # Through the gate's own boundary walk: only the production site
        # is a finding.
        saved = (byp.REPO_ROOT, byp.BOUNDARY)
        byp.REPO_ROOT, byp.BOUNDARY = self.t.src_dir, ("abc.rs", "abc/")
        try:
            self.t.files.clear()
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                code = byp.main(["--roles-dir", str(self.t.roles), "--langs-rs", str(self.t.langs), "--languages-dir", str(self.t.languages), "--baseline", str(self.t.baseline), "--bootstrap", "--show"])
        finally:
            byp.REPO_ROOT, byp.BOUNDARY = saved
        self.assertEqual(code, 0, out.getvalue())
        found = [l for l in out.getvalue().splitlines() if l.startswith("  ")]
        self.assertEqual(len(found), 1, found)
        self.assertIn("Java::LT via kind_id", found[0])

    def test_a_mod_declaration_does_not_swallow_the_next_item(self) -> None:
        # With `mod bash;` read as an item whose body is the next brace
        # block, `fn f` would be attributed to `mod bash`.
        self.t.src("a.rs", "mod bash;\nmod c;\nuse crate::Java;\nfn f(n: &Node) -> bool { n.kind_id() == Java::MethodInvocation as u16 }\n#[cfg(test)]\nmod tests { fn t() {} }\n")
        self.t.run("--bootstrap", "--show")
        text = self.t.baseline.read_text()
        self.assertIn("fn f\tJava\tMethodInvocation", text)
        self.assertNotIn("mod bash\t", text)
        src = byp.load_source(self.t.src_dir / "a.rs")
        spans = byp.item_spans(src)
        self.assertEqual([n for _, _, n in spans], ["mod bash", "mod c", "fn f", "mod tests"])
        bash = next(s_ for s_ in spans if s_[2] == "mod bash")
        self.assertLess(bash[1], src.masked.index("fn f"), "`mod bash;` ends at its semicolon")

    def test_update_rewrites_the_header(self) -> None:
        self.t.src("a.rs", "use crate::Java;\nfn f(n: &Node) -> bool { n.kind_id() == Java::MethodInvocation as u16 }\n")
        self.t.run("--bootstrap")
        stale = "# stale header\n" + "".join(l + "\n" for l in self.t.baseline.read_text().splitlines() if not l.startswith("#"))
        self.t.baseline.write_text(stale)
        code, out = self.t.run("--update")
        self.assertEqual(code, 0)
        self.assertNotIn("stale header", self.t.baseline.read_text())
        self.assertIn("Role-bypass baseline", self.t.baseline.read_text())

    def test_boundary_covers_the_dispatcher_modules(self) -> None:
        for must in ("src/metrics/cyclomatic.rs", "big-code-analysis-ast/src/checker.rs", "src/metrics/nexits.rs", "big-code-analysis-ast/src/macros/kind_sets.rs"):
            self.assertIn(must, byp.BOUNDARY)


if __name__ == "__main__":
    unittest.main()
