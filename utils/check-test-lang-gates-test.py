#!/usr/bin/env python3
"""Tests for check-test-lang-gates.py.

Three kinds of test, matching the other `…-test.py` gates:

* **lexer and predicate units** — the pieces a scanning gate goes
  quietly wrong in. A gate that stops matching reports a clean tree, so
  these are the only thing standing between a broken regex and a check
  that has silently switched itself off.
* **derivation units** — the role rules (test / sweep / helper), the
  dispatcher and inner-`cfg` exclusions, and the `not(feature = …)`
  case. Each is a shape that has already produced a wrong gate during
  #1472; the fixtures are cut down from the real ones.
* **repository smoke tests** — the live tree must be clean, and the
  derivation must still reproduce every gate a human wrote by hand.
  That second one is the load-bearing check: it is what earned the
  derivation the right to generate the other ~2,950.

Run with: python3 -m unittest -q utils/check-test-lang-gates-test.py
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
import types
import unittest

SCRIPT_SRC = pathlib.Path(__file__).resolve().parent / "check-test-lang-gates.py"
REPO_ROOT = SCRIPT_SRC.parent.parent


def _load_module() -> types.ModuleType:
    spec = importlib.util.spec_from_file_location("check_test_lang_gates", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    # Registered before execution because `@dataclass` resolves a
    # field's forward reference through `sys.modules[cls.__module__]`;
    # an unregistered module makes that lookup `None` and the decorator
    # raises at import time.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


gate = _load_module()

# A cut-down `mk_langs!`. It carries the rows whose feature name does not
# match their variant, plus `cpp` — which is what makes the
# `c-family-helpers` implication testable, since disabling the helper
# feature is only meaningful when something that enables it is present.
LANGS_FIXTURE = """
mk_langs!(
    // 1) Cargo feature name
    (
        "python",
        Python,
        "The `Python` language",
        "python",
        PythonCode,
        PythonParser,
        tree_sitter_python,
        [py],
        [],
        "0.25.0"
    ),
    (
        "typescript",
        Tsx,
        "The `Tsx` language",
        "tsx",
        TsxCode,
        TsxParser,
        tree_sitter_tsx,
        [tsx],
        [],
        "0.23.2"
    ),
    (
        "cpp",
        Cpp,
        "The `C++` language",
        "cpp",
        CppCode,
        CppParser,
        tree_sitter_cpp,
        [cpp],
        [],
        "0.23.4"
    ),
    (
        "c-family-helpers",
        Preproc,
        "The `PreProc` language",
        "preproc",
        PreprocCode,
        PreprocParser,
        tree_sitter_preproc,
        [],
        [],
        "2.2.1"
    ),
    (
        "rust",
        Rust,
        "The `Rust` language",
        "rust",
        RustCode,
        RustParser,
        tree_sitter_rust,
        [rs],
        [],
        "0.24.0"
    )
)
"""


class LanguageTableTest(unittest.TestCase):
    def setUp(self) -> None:
        self.table = gate.language_table(LANGS_FIXTURE)

    def test_keys_are_qualified_never_the_bare_variant(self) -> None:
        # `C`, `Go`, `Java` and `Rust` are ordinary identifiers in these
        # files; a table keyed on the bare variant derives gates for
        # languages the item never touches.
        self.assertIn("LANG::Rust", self.table)
        self.assertIn("RustParser", self.table)
        self.assertNotIn("Rust", self.table)

    def test_the_three_rows_whose_feature_is_not_their_slug(self) -> None:
        self.assertEqual(self.table["TsxParser"], "typescript")
        self.assertEqual(self.table["LANG::Tsx"], "typescript")
        self.assertEqual(self.table["PreprocParser"], "c-family-helpers")
        self.assertEqual(self.table["TsxCode"], "typescript")
        self.assertEqual(self.table["LANG::Python"], "python")

    def test_an_empty_table_is_refused_rather_than_passing(self) -> None:
        with self.assertRaises(gate.ScanError):
            gate.language_table("fn main() {}")

    def test_a_row_missing_a_field_is_refused_rather_than_bleeding(self) -> None:
        """`.*?` spans the descriptions, so a short row eats the next one.

        Without the check it silently files the *following* row's
        `*Code` / `*Parser` under this row's feature — a wrong language
        for two spellings, with no error anywhere.
        """
        short = LANGS_FIXTURE.replace("        PythonCode,\n", "")
        with self.assertRaises(gate.ScanError) as caught:
            gate.language_table(short)
        self.assertIn("run into the next one", str(caught.exception))

    def test_the_pattern_does_not_match_a_longer_identifier(self) -> None:
        pattern = gate.symbol_pattern(self.table)
        self.assertEqual(
            [
                m.group(0)
                for m in pattern.finditer("RustParser RustParserMock LANG::Rust")
            ],
            ["RustParser", "LANG::Rust"],
        )


class PredicateTest(unittest.TestCase):
    def test_any_all_not_and_nesting(self) -> None:
        off = frozenset({"python"})
        self.assertFalse(gate.evaluate_predicate('feature = "python"', off))
        self.assertTrue(gate.evaluate_predicate('feature = "rust"', off))
        self.assertTrue(
            gate.evaluate_predicate('any(feature = "python", feature = "rust")', off)
        )
        self.assertFalse(
            gate.evaluate_predicate('all(feature = "python", feature = "rust")', off)
        )
        self.assertTrue(gate.evaluate_predicate('not(feature = "python")', off))
        self.assertTrue(
            gate.evaluate_predicate('all(test, any(feature = "rust"))', off)
        )

    def test_non_feature_atoms_are_true_so_a_gate_only_ever_narrows(self) -> None:
        # `test`, `unix`, `debug_assertions` and a non-language feature
        # must not excuse a missing language gate.
        self.assertTrue(gate.evaluate_predicate("test", frozenset({"python"})))
        self.assertTrue(
            gate.evaluate_predicate('feature = "vcs-git"', frozenset({"python"}))
        )

    def test_disabling_the_helper_feature_disables_its_enablers(self) -> None:
        # `c`, `cpp` and `mozcpp` each enable `c-family-helpers`, so a
        # build without the helper grammars has none of the three.
        self.assertEqual(
            gate.disabled_closure("c-family-helpers"),
            frozenset({"c-family-helpers", "c", "cpp", "mozcpp"}),
        )
        self.assertEqual(gate.disabled_closure("python"), frozenset({"python"}))
        self.assertFalse(gate.gate_admits('feature = "cpp"', "c-family-helpers"))

    def test_an_unbalanced_predicate_is_refused(self) -> None:
        with self.assertRaises(gate.ScanError):
            gate.evaluate_predicate('any(feature = "rust"', frozenset())

    def test_gate_excludes_spots_a_deliberate_disabled_path_test(self) -> None:
        self.assertTrue(gate.gate_excludes('not(feature = "python")', "python"))
        self.assertFalse(gate.gate_excludes('feature = "python"', "python"))
        self.assertFalse(gate.gate_excludes(None, "python"))


def _scan(source: str, path: str = "src/fixture.rs") -> list:
    return gate.scan_source(source, path, gate.language_table(LANGS_FIXTURE))


def _named(items: list, name: str):
    return next(item for item in items if item.name == name)


class DerivationTest(unittest.TestCase):
    def test_a_plain_test_needs_the_language_it_names(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[test]
    fn python_thing() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(
            needs[_named(items, "python_thing").index], frozenset({"python"})
        )
        self.assertEqual(
            [i.name for i, _ in gate.offenders(items, needs)], ["python_thing"]
        )

    def test_production_code_is_left_alone(self) -> None:
        # The same reference outside a test scope is the library doing
        # its job, and gating it would be a bug.
        items = _scan(
            """
fn build() {
    let _ = PythonParser::new(b"".to_vec());
}
"""
        )
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])

    def test_a_correct_gate_is_accepted_and_a_wrong_one_is_not(self) -> None:
        source = """
#[cfg(test)]
mod tests {
    #[cfg(feature = "%s")]
    #[test]
    fn python_thing() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        right = _scan(source % "python")
        self.assertEqual(gate.offenders(right, gate.resolve_needs(right)), [])
        wrong = _scan(source % "rust")
        self.assertEqual(
            [i.name for i, _ in gate.offenders(wrong, gate.resolve_needs(wrong))],
            ["python_thing"],
        )

    def test_a_multi_language_test_needs_all_of_them(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[test]
    fn parity() {
        check::<PythonParser>("a", "f.py");
        check::<RustParser>("b", "f.rs");
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        item = _named(items, "parity")
        self.assertEqual(
            gate.required_marker(items, item, needs),
            'all(feature = "python", feature = "rust")',
        )
        # `any(...)` is not enough: a Rust-only build would compile it
        # and then panic on the Python half.
        satisfied = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(any(feature = "python", feature = "rust"))]
    #[test]
    fn parity() {
        check::<PythonParser>("a", "f.py");
        check::<RustParser>("b", "f.rs");
    }
}
"""
        )
        self.assertEqual(
            [
                i.name
                for i, _ in gate.offenders(satisfied, gate.resolve_needs(satisfied))
            ],
            ["parity"],
        )

    def test_a_reference_behind_an_inner_cfg_is_already_conditional(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[test]
    fn either() {
        #[cfg(feature = "python")]
        {
            check::<PythonParser>("a", "f.py");
        }
    }
}
"""
        )
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])

    def test_a_language_used_both_ways_is_still_needed(self) -> None:
        """Exclusion is positional, not a feature-wide subtraction.

        Subtracting the whole feature made a language named
        unconditionally *and* inside its own `#[cfg]` vanish from the
        needs entirely, so the test got no gate at all and panicked
        without the grammar — the unsafe direction.
        """
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[test]
    fn both_ways() {
        check::<PythonParser>("a", "f.py");
        #[cfg(feature = "python")]
        {
            check::<PythonParser>("b", "f2.py");
        }
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(needs[_named(items, "both_ways").index], frozenset({"python"}))

    def test_a_cfg_gated_tuple_row_is_conditional_too(self) -> None:
        # The rows of a fixture array are parenthesised, not braced;
        # counting only `{}` ends the extent on the opening paren and
        # leaves every `LANG::` inside the row looking unconditional.
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[test]
    fn table() {
        let cases = &[
            #[cfg(feature = "python")]
            (
                LANG::Python,
                "f.py",
            ),
        ];
    }
}
"""
        )
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])

    def test_a_helper_that_hardcodes_a_parser_reaches_its_callers(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    fn spells(src: &str) {
        assert_fixture_spells::<PythonParser>(src, "f.py");
    }

    #[test]
    fn uses_the_helper() {
        spells("a = 1");
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(
            needs[_named(items, "uses_the_helper").index], frozenset({"python"})
        )

    def test_a_lang_parameterised_helper_does_not(self) -> None:
        # Its body names languages as dispatch arms; propagating them
        # marks every caller as needing every language in the match.
        items = _scan(
            """
#[cfg(test)]
mod tests {
    fn conditions(lang: LANG, src: &str) -> u64 {
        metrics_verbatim(lang, src.as_bytes())
    }

    #[test]
    fn rust_only() {
        conditions(LANG::Rust, "fn f() {}");
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(needs[_named(items, "rust_only").index], frozenset({"rust"}))

    def test_a_compared_lang_is_an_identity_test_not_a_use(self) -> None:
        """#1478. `LANG` variants exist without their grammars.

        Asking which one a value *is* parses nothing, so counting the
        comparison as a requirement conjoins a feature onto the gate and
        drops the test from every build without it —
        `container_scope_tests.rs` lost the positive half of the #1197
        contract to exactly this.
        """
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[test]
    fn python_only() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
        assert_eq!(space.lang == LANG::Rust, false);
        assert!(matches!(space.lang, LANG::Cpp));
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(
            needs[_named(items, "python_only").index], frozenset({"python"})
        )

    def test_a_dispatcher_keyed_on_a_string_does_not_either(self) -> None:
        # `takes_lang_param` cannot see this one: the parameter is a
        # path. Without the match-arm rule all of its callers look as
        # though they need every language it can return.
        items = _scan(
            """
#[cfg(test)]
mod tests {
    fn analyze_lang(path: &str) -> FuncSpace {
        let lang = match ext {
            "py" => LANG::Python,
            "rs" => LANG::Rust,
            other => panic!("{other}"),
        };
        analyze(lang)
    }

    #[test]
    fn a_python_case() {
        analyze_lang("f.py");
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(needs[_named(items, "a_python_case").index], frozenset())

    def test_a_helper_is_resolved_in_its_own_module_not_by_bare_name(self) -> None:
        # `spaces_tests.rs` declares three `analyse` helpers in sibling
        # modules, one hardcoding Rust. A flat by-name map makes every
        # test in the other two look as though it needs Rust.
        items = _scan(
            """
#[cfg(test)]
mod a {
    fn analyse(src: &str) { check::<RustParser>(src, "f.rs"); }

    #[test]
    fn rust_case() { analyse("fn f() {}"); }
}

#[cfg(test)]
mod b {
    fn analyse(lang: LANG, src: &str) { metrics_verbatim(lang, src); }

    #[test]
    fn python_case() { analyse(LANG::Python, "a = 1"); }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(needs[_named(items, "rust_case").index], frozenset({"rust"}))
        self.assertEqual(
            needs[_named(items, "python_case").index], frozenset({"python"})
        )

    def test_a_disabled_path_test_is_not_asked_to_enable_the_language(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(not(feature = "python"))]
    #[test]
    fn disabled_language_reports_language_disabled() {
        assert!(matches!(parse(LANG::Python), Err(LanguageDisabled(_))));
    }
}
"""
        )
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])

    def test_a_sweep_needs_any_row_not_every_row(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(any(feature = "python", feature = "rust"))]
    #[test]
    fn sweep() {
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            let _ = (LANG::Python, LANG::Rust, lang);
        }
    }
}
"""
        )
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])

    def test_a_reference_in_a_string_or_comment_is_not_a_reference(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[test]
    fn only_looks_like_one() {
        // check_metrics::<PythonParser>(…)
        let src = r#"PythonParser and LANG::Python"#;
        assert_eq!(src.len(), 30);
    }
}
"""
        )
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])

    def test_a_macro_rules_body_is_a_template_not_an_item(self) -> None:
        """The transcriber is a token tree, and `--fix` must not write in it.

        A fixture using only `fn $lang()` cannot fail: `$` is not
        matched by `ITEM_RE`, so nothing leaks whether the skip works or
        not. The literal `fn inner_probe()` is what makes this
        discriminating, and the positive assertion on the macro item is
        what pins that it is recorded at all.
        """
        items = _scan(
            """
#[cfg(test)]
mod tests {
    macro_rules! roundtrip_tests {
        () => {
            fn inner_probe() { check::<PythonParser>("a", "f.py"); }
        };
    }
}
"""
        )
        kinds = {(item.kind, item.name) for item in items}
        self.assertIn(("macro", "roundtrip_tests"), kinds)
        self.assertNotIn("inner_probe", {name for _, name in kinds})
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])

    def test_an_attributed_macro_is_recognised_and_marked_in_place(self) -> None:
        """Reading `attrs` before binding them broke this three ways.

        The macro went unrecognised, so its transcriber was walked as
        ordinary source and a marker could be written inside the
        template; and the item it did record inherited the *previous*
        line's `attr_line`, putting any marker on the item above.
        """
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[allow(unused_macros)]
    macro_rules! gen {
        () => {
            fn inner_probe() { check::<PythonParser>("a", "f.py"); }
        };
    }
}
"""
        )
        macro = _named(items, "gen")
        self.assertEqual(macro.kind, "macro")
        # The `#[allow]`, not the `mod tests {` line above it.
        self.assertEqual(macro.attr_line, macro.line - 1)
        self.assertNotIn("inner_probe", {item.name for item in items})

    def test_a_leading_macro_rules_does_not_crash_the_scan(self) -> None:
        """A file whose first scanned line is a macro raised UnboundLocalError.

        It escaped the `main()` try as a traceback, so the exit status
        was 1 — which a caller reads as "offenders found".
        """
        items = _scan("macro_rules! foo {\n}\n", path="tests/api/x.rs")
        self.assertEqual([(i.kind, i.name) for i in items], [("macro", "foo")])
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])

    def test_an_integration_file_is_a_test_scope_without_cfg_test(self) -> None:
        items = _scan(
            """
#[test]
fn integration() {
    check::<PythonParser>("a = 1", "f.py");
}
""",
            path="tests/api/thing.rs",
        )
        self.assertEqual(
            [i.name for i, _ in gate.offenders(items, gate.resolve_needs(items))],
            ["integration"],
        )


class FeatureAtomTest(unittest.TestCase):
    """`feature_atom` is invisible to every other check in this file.

    Deleting the root-crate spelling leaves the repository scan green,
    because `gate_admits` answers the same either way — the difference
    only shows up in a real `cargo test --features c`. Its own docstring
    calls getting this wrong silent, and 33 root-crate markers depend on
    it, so it needs asserting directly.
    """

    def test_an_ordinary_feature_is_spelled_the_same_in_both_crates(self) -> None:
        for path in ("src/metrics/abc.rs", "big-code-analysis-ast/src/checker.rs"):
            self.assertEqual(gate.feature_atom("python", path), 'feature = "python"')

    def test_the_ast_crate_can_name_the_helper_feature_directly(self) -> None:
        # There `cpp`/`c`/`mozcpp` each list `c-family-helpers`, so the
        # bare atom is true whenever the helper grammars are compiled.
        self.assertEqual(
            gate.feature_atom(
                "c-family-helpers", "big-code-analysis-ast/src/checker.rs"
            ),
            'feature = "c-family-helpers"',
        )

    def test_the_root_crate_must_name_the_enablers_instead(self) -> None:
        # The root's `cpp` forwards to `big-code-analysis-ast/cpp` and
        # leaves its own `c-family-helpers` off, and `all-languages` does
        # not list it — so the bare atom is false in a default build and
        # the item disappears from it.
        self.assertEqual(
            gate.feature_atom("c-family-helpers", "src/metrics/loc.rs"),
            'any(feature = "c", feature = "c-family-helpers", '
            'feature = "cpp", feature = "mozcpp")',
        )

    def test_c_family_helper_enablers_match_the_manifest(self) -> None:
        """The one table in this gate that is hand-copied, not derived."""
        import tomllib

        manifest = tomllib.loads(
            (REPO_ROOT / "big-code-analysis-ast" / "Cargo.toml").read_text()
        )
        enablers = {
            name
            for name, enables in manifest["features"].items()
            if gate.C_FAMILY_HELPER_FEATURE in enables
        }
        self.assertEqual(enablers, set(gate.IMPLIES_C_FAMILY_HELPERS))


class SweepMarkerTest(unittest.TestCase):
    def test_a_sweep_is_marked_any_not_all(self) -> None:
        """`all(...)` here gates the 23-language parity suites out of
        every build but the full one, and takes their helpers dead with
        them. The existing sweep test only asserts `offenders == []`,
        which exercises the other branch."""
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[test]
    fn sweep() {
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            let _ = (LANG::Python, LANG::Rust, lang);
        }
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(
            gate.required_marker(items, _named(items, "sweep"), needs),
            'any(feature = "python", feature = "rust")',
        )


class HelperInheritanceTest(unittest.TestCase):
    def test_a_helper_inherits_the_gate_of_the_tests_that_call_it(self) -> None:
        """The caller-to-callee widening fixpoint, which had no coverage.

        Deleting it left the whole suite green.
        """
        items = _scan(
            """
#[cfg(test)]
mod tests {
    fn shape(space: &FuncSpace) -> String { String::new() }

    #[test]
    fn python_case() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| { shape(&m); });
    }

    #[test]
    fn rust_case() {
        check_metrics::<RustParser>("fn f() {}", "f.rs", |m| { shape(&m); });
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(
            needs[_named(items, "shape").index], frozenset({"python", "rust"})
        )
        self.assertEqual(
            gate.required_marker(items, _named(items, "shape"), needs),
            'any(feature = "python", feature = "rust")',
        )


class OverGatedTest(unittest.TestCase):
    """#1478. The direction no other check can see.

    A gate too *wide* panics on the leg that lacks the grammar. A gate
    too *narrow* just drops the test, and the leg still looks green — so
    the only thing that can notice is the derivation itself.
    """

    def test_a_gate_naming_a_language_the_body_never_uses_is_reported(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(all(feature = "python", feature = "rust"))]
    #[test]
    fn only_needs_python() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(
            [(i.name, sorted(w)) for i, w in gate.over_gated(items, needs)],
            [("only_needs_python", ["rust"])],
        )

    def test_an_exact_gate_is_not_reported(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(feature = "python")]
    #[test]
    fn only_needs_python() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        )
        self.assertEqual(gate.over_gated(items, gate.resolve_needs(items)), [])

    def test_a_marker_accepts_the_features_it_names(self) -> None:
        source = """
#[cfg(test)]
mod tests {
    // test-lang-gates: hand-written(%s) — the language comes from a
    //     glob, which no scanner can read.
    #[cfg(all(feature = "python", feature = "rust"))]
    #[test]
    fn only_needs_python() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        accepted = _scan(source % "rust")
        self.assertEqual(gate.over_gated(accepted, gate.resolve_needs(accepted)), [])
        # Naming the wrong feature does not silence the real one.
        wrong = _scan(source % "typescript")
        self.assertEqual(
            [sorted(w) for _, w in gate.over_gated(wrong, gate.resolve_needs(wrong))],
            [["rust"]],
        )

    def test_a_disabled_path_gate_is_not_over_gating(self) -> None:
        # `cfg(not(feature = "python"))` is false with everything on, so
        # every feature would read as "required" and the whole tree would
        # report. It names the language precisely because it is absent.
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(not(feature = "python"))]
    #[test]
    fn disabled_language_reports_language_disabled() {
        assert!(matches!(parse(LANG::Python), Err(LanguageDisabled(_))));
    }
}
"""
        )
        self.assertEqual(gate.over_gated(items, gate.resolve_needs(items)), [])

    def test_a_feature_that_would_disable_a_needed_one_is_not_over_gating(
        self,
    ) -> None:
        # A `cpp` test is excluded from a build without
        # `c-family-helpers`, but only because that build has no `cpp`
        # either. There is no configuration it could have run in.
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(feature = "cpp")]
    #[test]
    fn a_cpp_test() {
        check_metrics::<CppParser>("int f(){}", "f.cpp", |m| {});
    }
}
""",
        )
        self.assertEqual(gate.over_gated(items, gate.resolve_needs(items)), [])

    def test_the_reference_build_turns_off_only_what_the_gate_wants_off(
        self,
    ) -> None:
        """A negated conjunct must not exempt the whole item.

        `all(not(python), rust, go)` is false with everything enabled,
        so measuring from an all-on baseline reads every feature as
        required and the item escapes entirely. Measuring from a build
        that satisfies the negation finds the `go` that is genuinely
        over-declared.
        """
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(all(not(feature = "python"), feature = "rust", feature = "typescript"))]
    #[test]
    fn rust_without_python() {
        check_metrics::<RustParser>("fn f() {}", "f.rs", |m| {});
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(
            [(i.name, sorted(w)) for i, w in gate.over_gated(items, needs)],
            [("rust_without_python", ["typescript"])],
        )

    def test_a_feature_the_gate_never_spells_is_not_reported(self) -> None:
        """The closure runs one way only.

        A gate reading `feature = "cpp"` does exclude every build
        without `c-family-helpers` — such a build has no `cpp` — but the
        gate never mentions the helper and narrowing it would change
        nothing. Reporting it made every C-family gate in the tree carry
        a marker for a feature its author never wrote.
        """
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(feature = "cpp")]
    #[test]
    fn walks_a_corpus() {
        for file in glob("corpus/**/*") {
            let _ = analyze(file);
        }
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(
            [(i.name, sorted(w)) for i, w in gate.over_gated(items, needs)],
            [("walks_a_corpus", ["cpp"])],
        )

    def test_a_pub_item_is_not_reported(self) -> None:
        # Its callers are in files this scanner never sees, so `needs`
        # is only the visible subset and a gate placed for one of the
        # others reads as over-declared. `offenders` skips them too.
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(feature = "python")]
    pub(crate) fn assert_python_fixture_spells(source: &str) {
        assert!(!source.is_empty());
    }
}
"""
        )
        self.assertEqual(gate.over_gated(items, gate.resolve_needs(items)), [])

    def test_a_sweeps_union_gate_never_reports(self) -> None:
        # `any(...)` requires no feature on its own.
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(any(feature = "python", feature = "rust"))]
    #[test]
    fn sweep() {
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            let _ = (LANG::Python, LANG::Rust, lang);
        }
    }
}
"""
        )
        self.assertEqual(gate.over_gated(items, gate.resolve_needs(items)), [])


class StaleMarkerTest(unittest.TestCase):
    """An accepted gate that stopped needing accepting.

    A stale marker silences nothing, so it can never cause a wrong
    verdict on its own. It still tells the next reader the gate is wider
    than the body for a reason that no longer applies, and a list of
    accepted gates only stays readable as a census while every one of
    them is load-bearing.
    """

    def test_a_marker_for_a_feature_that_is_not_over_declared_is_reported(
        self,
    ) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    // test-lang-gates: hand-written(rust) — no longer true.
    #[cfg(feature = "python")]
    #[test]
    fn only_needs_python() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(
            [(i.name, sorted(w)) for i, w in gate.stale_markers(items, needs)],
            [("only_needs_python", ["rust"])],
        )

    def test_a_load_bearing_marker_is_not_reported(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    // test-lang-gates: hand-written(rust) — the language comes from a
    //     glob, which no scanner can read.
    #[cfg(all(feature = "python", feature = "rust"))]
    #[test]
    fn only_needs_python() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(gate.stale_markers(items, needs), [])
        self.assertEqual(gate.over_gated(items, needs), [])

    def test_a_doc_comment_is_not_a_marker(self) -> None:
        # `///` renders into the crate documentation. A directive read
        # out of prose is one a reader has no reason to think is live.
        items = _scan(
            """
#[cfg(test)]
mod tests {
    /// test-lang-gates: hand-written(rust) — in a doc comment.
    #[cfg(all(feature = "python", feature = "rust"))]
    #[test]
    fn only_needs_python() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        self.assertEqual(_named(items, "only_needs_python").hand_written, frozenset())
        self.assertEqual(
            [sorted(w) for _, w in gate.over_gated(items, needs)], [["rust"]]
        )


class ComparisonSpellingTest(unittest.TestCase):
    """Every spelling of "which variant is this" the tree uses.

    One of these escaping the rule conjoins a feature onto the gate and
    drops the test from every build without it, silently — the #1197
    shape. They are listed separately because each is a separate regex
    and a passing sibling proves nothing about the others.
    """

    def _needs(self, body: str) -> frozenset:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[test]
    fn python_only() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
        %s
    }
}
"""
            % body
        )
        return gate.resolve_needs(items)[_named(items, "python_only").index]

    def test_the_variant_may_come_first(self) -> None:
        self.assertEqual(self._needs("assert!(LANG::Tsx == space.lang);"), {"python"})

    def test_a_crate_qualified_path_is_still_a_comparison(self) -> None:
        self.assertEqual(
            self._needs("assert!(space.lang == crate::LANG::Tsx);"), {"python"}
        )
        self.assertEqual(
            self._needs("assert!(matches!(space.lang, crate::LANG::Tsx));"), {"python"}
        )

    def test_an_assert_eq_is_a_comparison(self) -> None:
        self.assertEqual(self._needs("assert_eq!(space.lang, LANG::Tsx);"), {"python"})
        self.assertEqual(self._needs("assert_ne!(space.lang, LANG::Tsx);"), {"python"})

    def test_a_real_use_is_still_a_use(self) -> None:
        # The guard on all of the above, and on the fixture: the rule
        # must not swallow a construction, and a variant the table does
        # not carry would make every assertion here vacuously true.
        self.assertEqual(
            self._needs("let _ = analyze(LANG::Tsx, source);"),
            {"python", "typescript"},
        )


class SweepHardcodesTest(unittest.TestCase):
    def test_a_sweep_still_needs_the_parser_it_hardcodes(self) -> None:
        """#1478. The exemption was a way round the whole gate.

        A sweep skips disabled languages at run time, so its rows need
        only `any`. A parser it picks through a *type parameter* cannot
        be skipped by any runtime filter, so that one is required — and
        without this the union gate admits the test into a build that
        panics in `Tree::new`.
        """
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(any(feature = "python", feature = "rust"))]
    #[test]
    fn sweep_that_also_hardcodes() {
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            let _ = (LANG::Python, lang);
        }
        check_metrics::<RustParser>("fn f() {}", "f.rs", |m| {});
    }
}
"""
        )
        needs = gate.resolve_needs(items)
        item = _named(items, "sweep_that_also_hardcodes")
        self.assertTrue(item.is_sweep)
        self.assertEqual(item.hardcoded, frozenset({"rust"}))
        self.assertEqual(
            [(i.name, sorted(w)) for i, w in gate.offenders(items, needs)],
            [("sweep_that_also_hardcodes", ["rust"])],
        )

    def test_a_sweep_naming_only_lang_values_is_still_exempt(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(any(feature = "python", feature = "rust"))]
    #[test]
    fn sweep() {
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            let _ = (LANG::Python, LANG::Rust, lang);
        }
    }
}
"""
        )
        items_by_name = _named(items, "sweep")
        self.assertEqual(items_by_name.hardcoded, frozenset())
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])

    def test_a_pin_one_call_away_still_pins(self) -> None:
        """#1478's own example. `hardcoded` has to propagate.

        `needs` already travels through helpers; without the same
        propagation here a sweep whose only fixed-parser call sits one
        hop away reads as pinning nothing, and the union gate admits it
        into a build that panics in `Tree::new`.
        """
        items = _scan(
            """
#[cfg(test)]
mod tests {
    fn parse_rust(source: &str) {
        check_metrics::<RustParser>(source, "f.rs", |m| {});
    }

    #[cfg(any(feature = "python", feature = "rust"))]
    #[test]
    fn sweep_whose_pin_is_one_call_away() {
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            let _ = (LANG::Python, lang);
        }
        parse_rust("fn f() {}");
    }
}
"""
        )
        item = _named(items, "sweep_whose_pin_is_one_call_away")
        # The item itself names no parser -- the pin is the helper's.
        self.assertEqual(item.hardcoded, frozenset())
        self.assertEqual(gate.hardcoded_closure(items, item), frozenset({"rust"}))
        reported = {
            i.name: sorted(w)
            for i, w in gate.offenders(items, gate.resolve_needs(items))
        }
        self.assertEqual(reported["sweep_whose_pin_is_one_call_away"], ["rust"])

    def test_a_helper_that_hardcodes_is_not_pinned(self) -> None:
        """A helper is live when a caller is, and simply not called
        otherwise — `halstead.rs`'s `assert_js_family_counts` binds three
        parsers and is correctly gated `any` of them."""
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(any(feature = "python", feature = "rust"))]
    fn assert_both(source: &str) {
        check::<PythonParser>(source, "f.py");
        check::<RustParser>(source, "f.rs");
    }

    #[cfg(all(feature = "python", feature = "rust"))]
    #[test]
    fn uses_both() { assert_both("x"); }
}
"""
        )
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])


class MarkerTest(unittest.TestCase):
    def test_a_short_marker_stays_on_one_line(self) -> None:
        self.assertEqual(
            gate.wrap_marker('feature = "python"', "    "),
            ['    #[cfg(feature = "python")]'],
        )

    def test_a_long_marker_wraps_the_way_rustfmt_would(self) -> None:
        predicate = "any(" + ", ".join(f'feature = "lang{i}"' for i in range(12)) + ")"
        lines = gate.wrap_marker(predicate, "")
        self.assertEqual(lines[0], "#[cfg(any(")
        self.assertEqual(lines[-1], "))]")
        self.assertEqual(len(lines), 14)
        self.assertTrue(all(len(line) <= gate.MAX_WIDTH for line in lines))


class FixTest(unittest.TestCase):
    def test_fix_inserts_above_the_attribute_run_and_the_gate_then_passes(self) -> None:
        source = """#[cfg(test)]
mod tests {
    /// Doc comment.
    #[test]
    fn python_thing() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "src").mkdir()
            (root / "big-code-analysis-ast" / "src").mkdir(parents=True)
            (root / "big-code-analysis-ast" / "src" / "langs.rs").write_text(
                LANGS_FIXTURE
            )
            target = root / "src" / "fixture.rs"
            target.write_text(source)

            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                self.assertEqual(gate.main(["--root", str(root)]), 1)
                self.assertEqual(gate.main(["--root", str(root), "--fix"]), 0)

            # The marker joins the attribute stack, under the doc
            # comment and above `#[test]` -- not between `#[test]` and
            # the signature, and not above the documentation.
            self.assertIn(
                '    /// Doc comment.\n    #[cfg(feature = "python")]\n    #[test]\n',
                target.read_text(),
            )
            # Idempotent, and the tree is clean afterwards.
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(gate.main(["--root", str(root)]), 0)

    def test_fix_converges_on_a_sweep_that_pins_a_parser(self) -> None:
        """The marker `--fix` writes must satisfy the check that asked.

        A sweep needs `any` of the rows it iterates and `all` of what it
        pins by type. Writing only the `any` half leaves the pin
        unguarded, so the next pass asks for the same marker again --
        `--fix` stacked one `#[cfg]` per iteration and then exited 2,
        having made the tree worse than it found it.
        """
        source = """#[cfg(test)]
mod tests {
    #[test]
    fn sweep_that_also_pins() {
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            let _ = (LANG::Python, LANG::Tsx, lang);
        }
        check_metrics::<RustParser>("fn f() {}", "f.rs", |m| {});
    }
}
"""
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "src").mkdir()
            (root / "big-code-analysis-ast" / "src").mkdir(parents=True)
            (root / "big-code-analysis-ast" / "src" / "langs.rs").write_text(
                LANGS_FIXTURE
            )
            target = root / "src" / "fixture.rs"
            target.write_text(source)

            with (
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(gate.main(["--root", str(root), "--fix"]), 0)
                # The pass that stacked: clean in one, not two.
                self.assertEqual(gate.main(["--root", str(root)]), 0)

            written = target.read_text()
            self.assertIn(
                '    #[cfg(all(feature = "rust", '
                'any(feature = "python", feature = "typescript")))]\n',
                written,
            )
            self.assertEqual(written.count("#[cfg("), 2)

    def test_both_directions_are_reported_in_one_run(self) -> None:
        """They are independent defects in independent items.

        Returning on the first report hid every under-gated item behind
        any single over-gated one, so a `--fix`-able failure could be
        invisible until an unrelated marker was written by hand.
        """
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "src").mkdir()
            (root / "big-code-analysis-ast" / "src").mkdir(parents=True)
            (root / "big-code-analysis-ast" / "src" / "langs.rs").write_text(
                LANGS_FIXTURE
            )
            (root / "src" / "wide.rs").write_text(
                """#[cfg(test)]
mod tests {
    #[cfg(all(feature = "python", feature = "rust"))]
    #[test]
    fn gated_too_wide() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
            )
            (root / "src" / "narrow.rs").write_text(
                """#[cfg(test)]
mod tests {
    #[test]
    fn gated_not_at_all() {
        check_metrics::<RustParser>("fn f() {}", "f.rs", |m| {});
    }
}
"""
            )
            err = io.StringIO()
            with (
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(err),
            ):
                self.assertEqual(gate.main(["--root", str(root)]), 1)
            report = err.getvalue()
            self.assertIn("gated_too_wide", report)
            self.assertIn("gated_not_at_all", report)

    def test_a_tree_of_production_code_only_is_not_a_pass(self) -> None:
        """The guard counts tests, not items.

        `scan_source` returns production items too, so testing whether
        *anything* was found let a tree with no tests at all report OK
        having checked nothing — the silent switch-off these self-tests
        exist to prevent.
        """
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "src").mkdir()
            (root / "big-code-analysis-ast" / "src").mkdir(parents=True)
            (root / "big-code-analysis-ast" / "src" / "langs.rs").write_text(
                LANGS_FIXTURE
            )
            (root / "src" / "lib.rs").write_text(
                "pub fn parse() { let _ = PythonParser::new(vec![]); }\n"
            )
            err = io.StringIO()
            with (
                contextlib.redirect_stderr(err),
                contextlib.redirect_stdout(io.StringIO()),
            ):
                self.assertEqual(gate.main(["--root", str(root)]), 2)
            self.assertIn("no tests found", err.getvalue())

    def test_an_empty_tree_fails_rather_than_passing_vacuously(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "src").mkdir()
            (root / "big-code-analysis-ast" / "src").mkdir(parents=True)
            (root / "big-code-analysis-ast" / "src" / "langs.rs").write_text(
                LANGS_FIXTURE
            )
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                self.assertEqual(gate.main(["--root", str(root)]), 2)
            self.assertIn("no tests found", err.getvalue())


class AssertionOperandTest(unittest.TestCase):
    """A variant an assertion *compares* is not a variant it parses.

    The flat `[^()]*?` this shipped with stopped at the first `(` in the
    argument list and never reached the variant, so 36 tests over
    `get_from_ext` / `FromStr` / `name()` — none of which is `cfg`-gated
    — were gated on grammars they never touch, several down to one build
    in twenty-five. Invisible to `over_gated`, whose marker mirrored the
    derivation, and to `--compare`, which the narrowing label skipped.
    """

    def _needs(self, body: str) -> frozenset:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        %s
    }
}
"""
            % body
        )
        return gate.resolve_needs(items)[_named(items, "t").index]

    def test_a_variant_behind_a_call_in_the_expected_value_is_a_comparison(
        self,
    ) -> None:
        self.assertEqual(
            self._needs('assert_eq!(get_from_ext("rs"), Some(LANG::Rust));'),
            frozenset(),
        )
        self.assertEqual(
            self._needs('assert_eq!(pick(buf, "x"), (Some(LANG::Tsx), "tsx"));'),
            frozenset(),
        )
        self.assertEqual(
            self._needs("assert_eq!(collected, vec![LANG::Rust, LANG::Python]);"),
            frozenset(),
        )

    def test_a_variant_handed_to_the_call_under_test_is_still_a_use(self) -> None:
        # The guard on the rule above: excluding the whole argument list
        # would exempt the language this test actually parses.
        self.assertEqual(
            self._needs("assert_eq!(conditions(LANG::Rust, src), 3);"),
            frozenset({"rust"}),
        )
        self.assertEqual(
            self._needs('assert_eq!(check::<RustParser>(src, "f.rs"), 3);'),
            frozenset({"rust"}),
        )

    def test_a_variant_in_receiver_position_reads_metadata(self) -> None:
        # `extensions()` / `name()` read the table `mk_langs!` generates
        # unconditionally, so they need no grammar.
        self.assertEqual(
            self._needs("assert!(LANG::Rust.extensions().is_empty());"), frozenset()
        )

    def test_except_the_two_that_hand_back_the_grammar(self) -> None:
        """`tree_sitter_language()` returns `None` with the feature off.

        Every caller here follows it with `.expect(…)`, so reading the
        receiver as metadata gates the test out of nothing and lets it
        panic under a build without that language — the same failure as
        `Tree::new`, one call earlier.
        """
        self.assertEqual(
            self._needs("let _ = LANG::Rust.tree_sitter_language().unwrap();"),
            frozenset({"rust"}),
        )


class NegatedGateTest(unittest.TestCase):
    def test_a_conjunction_of_negations_finds_its_reference_build(self) -> None:
        """Probing one feature at a time cannot.

        `all(not(a), not(b))` is false with either alone disabled, so no
        single probe ever finds the build that satisfies it, and every
        feature in the tree then reads as over-declared. Reading the
        negated names out of the text instead is exact.
        """
        self.assertEqual(
            gate.negated_features(
                'all(not(feature = "python"), not(feature = "rust"))'
            ),
            frozenset({"python", "rust"}),
        )
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(all(not(feature = "python"), not(feature = "rust")))]
    #[test]
    fn disabled_both() {
        check_metrics::<CppParser>("int f(){}", "f.cpp", |m| {});
    }
}
"""
        )
        self.assertEqual(gate.over_gated(items, gate.resolve_needs(items)), [])

    def test_a_negated_test_cfg_is_not_a_test_scope(self) -> None:
        # `cfg(not(test))` marks code that exists *outside* the test
        # build; reading it as a test scope pulls production items into
        # `--show` and into the helper-inheritance walk.
        self.assertFalse(gate.predicate_requires_test("not(test)"))
        self.assertTrue(gate.predicate_requires_test("test"))
        self.assertTrue(gate.predicate_requires_test('all(test, feature = "rust")'))


class InnerAttributeTest(unittest.TestCase):
    def test_a_module_gated_by_an_inner_attribute_is_seen(self) -> None:
        """`#![cfg(...)]` inside the braces, not `#[cfg(...)]` above them.

        The branch that reads one was unreachable: every caller tested
        `startswith("#[")` first, which an inner attribute fails.
        """
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #![cfg(feature = "python")]

    #[test]
    fn t() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        )
        self.assertEqual(
            gate.effective_predicate(items, _named(items, "t")),
            'all(feature = "python", test)',
        )
        self.assertEqual(gate.offenders(items, gate.resolve_needs(items)), [])


class DispatchArmTest(unittest.TestCase):
    def test_a_wrapped_arm_is_still_a_dispatch_arm(self) -> None:
        # `=> Some(LANG::Python)` is how a fallible lookup spells it.
        # Read as a use, it hands every language the helper can return
        # to each of its callers.
        items = _scan(
            """
#[cfg(test)]
mod tests {
    fn pick(ext: &str) -> Option<LANG> {
        match ext {
            "py" => Some(LANG::Python),
            "rs" => Some(LANG::Rust),
            _ => None,
        }
    }

    #[test]
    fn t() {
        let _ = pick("py");
    }
}
"""
        )
        self.assertEqual(
            gate.resolve_needs(items)[_named(items, "t").index], frozenset()
        )


class FullEnumSweepTest(unittest.TestCase):
    """A sweep over the whole enum, and what it may not derive from.

    Both rules here were found by `--compare` on its first CI run, and
    neither `offenders` nor `over_gated` can see the defect: the gate was
    an `any(...)`, which requires no single feature, and it faithfully
    mirrored a derivation that was itself wrong.
    """

    SWEEP = """
#[cfg(test)]
mod tests {
    fn fixture(lang: LANG) -> (&'static str, &'static str) {
        match lang {
            LANG::Python => ("a = 1", "py"),
            LANG::Rust => ("fn f() {}", "rs"),
            _ => ("", ""),
        }
    }

    #[test]
    fn every_language_holds_the_property() {
        let mut checked = 0;
        for lang in LANG::into_enum_iter() {
            if !lang.is_enabled() {
                continue;
            }
            checked += 1;
            let (source, ext) = fixture(lang);
            let _ = analyze(Source::new(lang, source.as_bytes()), ext);
            %s
        }
        %s
    }
}
"""
    EXCLUSION = "if !matches!(lang, LANG::Ccomment | LANG::Preproc) { let _ = 1; }"
    GUARD = 'assert!(checked > 0, "no language enabled");'

    def test_a_guarded_full_enum_sweep_needs_every_language(self) -> None:
        """It *fails* rather than skips with no language enabled.

        So it has to be absent then — the #1220 class. Deriving anything
        narrower gates it out of builds it would have run in, and the
        narrower set cannot be read off the body at all when the
        fixtures come from a `LANG`-parameterised helper.
        """
        items = _scan(self.SWEEP % ("", self.GUARD))
        needs = gate.resolve_needs(items)
        self.assertEqual(
            needs[_named(items, "every_language_holds_the_property").index],
            frozenset({"python", "typescript", "cpp", "c-family-helpers", "rust"}),
        )

    def test_an_unguarded_full_enum_sweep_is_left_alone(self) -> None:
        # `Display`, `FromStr` and slug round-trips walk the same enum
        # and parse nothing; the variants exist without their grammars.
        # Gating those stops them running on the
        # `--no-default-features` leg, which is where they belong.
        items = _scan(self.SWEEP % ("", ""))
        needs = gate.resolve_needs(items)
        self.assertNotIn(
            "typescript",
            needs[_named(items, "every_language_holds_the_property").index],
        )

    def test_every_alternative_of_a_matches_is_an_identity_test(self) -> None:
        """Not just the first one.

        The leading alternative was already excluded; the rest were not,
        so a `matches!(lang, LANG::Ccomment | LANG::Preproc)` written to
        *skip* two languages read as a requirement for one of them. That
        single leak was the entire gate on two `every_*_in_every_language`
        parity sweeps, which ran in four builds instead of twenty-three.
        """
        guarded = _scan(self.SWEEP % (self.EXCLUSION, self.GUARD))
        needs = gate.resolve_needs(guarded)
        item = _named(guarded, "every_language_holds_the_property")
        # Correct because of the sweep rule, whichever way the `matches!`
        # is read -- so assert the narrow case too, where the leak is the
        # only thing that could contribute.
        self.assertEqual(len(needs[item.index]), 5)

        bare = _scan(self.SWEEP % (self.EXCLUSION, ""))
        leaked = gate.resolve_needs(bare)[
            _named(bare, "every_language_holds_the_property").index
        ]
        self.assertNotIn("c-family-helpers", leaked)


class CompareTest(unittest.TestCase):
    """#1478's residue: the bug in the derivation itself.

    Every static rule here compares a marker against the derivation.
    When the two agree and both are wrong there is nothing left to
    compare against except the previous revision, and membership per
    build is computable from source — no cargo, no fifteen builds.
    """

    def test_a_qualified_name_carries_the_module_path(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    mod inner {
        #[test]
        fn a_case() {}
    }
}
""",
            path="src/metrics/nom.rs",
        )
        self.assertEqual(
            gate.qualified_name(items, _named(items, "a_case")),
            "src/metrics/nom.rs::tests::inner::a_case",
        )

    def test_the_probe_set_comes_from_the_language_table(self) -> None:
        # Not from the CI matrix: a new language then extends the probes
        # for free and there is nothing to keep in step.
        probes = gate.probe_builds(gate.language_table(LANGS_FIXTURE))
        self.assertEqual(probes["--no-default-features"], frozenset())
        self.assertEqual(
            probes["--all-features"],
            frozenset({"python", "typescript", "cpp", "c-family-helpers", "rust"}),
        )
        # `cpp` enables the helper grammars, so its probe carries both.
        self.assertEqual(
            probes["--features cpp"], frozenset({"cpp", "c-family-helpers"})
        )
        self.assertEqual(probes["--features rust"], frozenset({"rust"}))

    def test_membership_follows_the_gate(self) -> None:
        items = _scan(
            """
#[cfg(test)]
mod tests {
    #[cfg(feature = "python")]
    #[test]
    fn python_case() {
        check_metrics::<PythonParser>("a = 1", "f.py", |m| {});
    }
}
"""
        )
        probes = gate.probe_builds(gate.language_table(LANGS_FIXTURE))
        built = gate.membership({"src/fixture.rs": items}, probes)
        self.assertEqual(
            built["src/fixture.rs::tests::python_case"],
            frozenset({"--all-features", "--features python"}),
        )

    def test_a_gate_that_narrowed_since_the_reference_is_reported(self) -> None:
        wide = """#[cfg(test)]
mod tests {
    #[cfg(any(feature = "python", feature = "rust"))]
    #[test]
    fn a_case() {
        for lang in LANG::into_enum_iter().filter(LANG::is_enabled) {
            let _ = (LANG::Python, LANG::Rust, lang);
        }
    }
}
"""
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "src").mkdir()
            (root / "big-code-analysis-ast" / "src").mkdir(parents=True)
            (root / "big-code-analysis-ast" / "src" / "langs.rs").write_text(
                LANGS_FIXTURE
            )
            target = root / "src" / "fixture.rs"
            target.write_text(wide)

            def git(*args: str) -> None:
                subprocess.run(
                    ["git", *args],
                    cwd=root,
                    check=True,
                    capture_output=True,
                    env={
                        "GIT_AUTHOR_NAME": "t",
                        "GIT_AUTHOR_EMAIL": "t@t",
                        "GIT_COMMITTER_NAME": "t",
                        "GIT_COMMITTER_EMAIL": "t@t",
                        "PATH": os.environ.get("PATH", ""),
                        "HOME": str(root),
                    },
                )

            git("init", "-q")
            git("add", "-A")
            git("commit", "-q", "-m", "base")

            table = gate.language_table(LANGS_FIXTURE)
            # Unchanged: nothing lost.
            self.assertEqual(gate.compare_revisions("HEAD", root, table), [])

            # Narrowed to Python alone. The test still exists, still
            # passes under `--all-features`, and has silently stopped
            # running on the Rust leg -- the one thing no marker check
            # can see.
            target.write_text(
                wide.replace(
                    '#[cfg(any(feature = "python", feature = "rust"))]',
                    '#[cfg(feature = "python")]',
                )
            )
            self.assertEqual(
                [
                    (name, sorted(lost))
                    for name, lost in gate.compare_revisions("HEAD", root, table)
                ],
                [("src/fixture.rs::tests::a_case", ["--features rust"])],
            )

            # A deletion is a deliberate change, not a narrowed gate.
            target.write_text("#[cfg(test)]\nmod tests {}\n")
            self.assertEqual(gate.compare_revisions("HEAD", root, table), [])


class RepositoryTest(unittest.TestCase):
    def setUp(self) -> None:
        self.table = gate.language_table(
            (REPO_ROOT / "big-code-analysis-ast" / "src" / "langs.rs").read_text()
        )
        self.per_file = gate.scan_tree(REPO_ROOT, self.table)

    def test_the_real_language_table_has_every_row(self) -> None:
        features = set(self.table.values())
        self.assertIn("c-family-helpers", features)
        self.assertEqual(self.table["TsxParser"], "typescript")
        # 25 variants + 25 parser aliases + 25 `*Code` tags.
        self.assertEqual(len(self.table), 75)

    def test_the_scan_finds_the_metric_modules(self) -> None:
        # A scanner that silently stops matching reports a clean tree.
        self.assertIn("src/metrics/abc.rs", self.per_file)
        self.assertGreater(len(self.per_file), 50)

    def test_the_repository_is_clean(self) -> None:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = gate.main([])
        self.assertEqual(code, 0, err.getvalue())

    def test_the_derivation_reproduces_every_hand_written_gate(self) -> None:
        """No item a human gated may be reported as under-gated.

        This is what licenses the mechanical pass: the derivation agrees
        with all 167 markers written by hand over twelve months, so the
        ~2,950 it generated are trustworthy for the same reason. A
        change that makes the derivation stricter shows up here first.
        """
        checked = 0
        for items in self.per_file.values():
            needs = gate.resolve_needs(items)
            reported = {item.index for item, _ in gate.offenders(items, needs)}
            for item in items:
                if not item.in_test_scope:
                    continue
                if not any("feature" in p for p in item.own_predicates):
                    continue
                checked += 1
                self.assertNotIn(item.index, reported, item.describe())
        self.assertGreater(checked, 150, "the hand-gated population vanished")


if __name__ == "__main__":
    unittest.main()
