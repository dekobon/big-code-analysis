#!/usr/bin/env python3
"""Tests for check-feature-gates.py.

Three kinds of test, matching the sibling gates' pattern:

* Unit tests for the subject scanner over fixture Rust text — the
  single-line and multi-line ``#[cfg(any(feature = …))]`` spellings, the
  ``all(test, any(…))`` nesting, and the shapes that must *not* be
  recorded (a gate quoted inside a fixture string, a commented-out one,
  an inverted ``not(any(…))``).
* Unit tests for cargo flag parsing and the feature closure, which is
  what stops ``default = ["all-languages"]`` from reading as "one
  feature enabled".
* ``main()`` tests pinning both directions with ``list_tests`` stubbed,
  so the self-test needs no cargo and does not rot when the real
  subjects change: a correctly-gated subject passes, and a subject whose
  union gate is defeated — so the build still contains it — fails.

Fixture text throughout, never the live tree, with one exception: a
smoke test asserting the scanner still finds subjects in ``src/``. That
one is the gate's own non-vacuity guard, and a gate that silently stops
finding anything is the defect this whole file exists to prevent.

Run with:
    python3 -m unittest -q utils/check-feature-gates-test.py
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import pathlib
import sys
import types
import unittest

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-feature-gates.py"


def _load_module() -> types.ModuleType:
    spec = importlib.util.spec_from_file_location("check_feature_gates", SCRIPT_SRC)
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


# A correctly-gated module: the union names both features its two tests
# use, so a build enabling neither drops it. This is the shape #1220 and
# PR #1221 established and the one the gate must leave alone.
WELL_GATED = """\
#[cfg(all(test, any(feature = "php", feature = "groovy")))]
mod hidden_literal_supertypes {
    #[test]
    #[cfg(feature = "php")]
    fn php_hidden_string_supertype_is_unreachable() {}
}
"""

# The same module with the union gate defeated by a non-feature
# disjunct. `cfg(test)` is true in every test build, so the `any(…)`
# gates nothing and the module is present however few languages are
# compiled in — while its `checked > 0` guard still has no rows to
# count. Nothing in `cargo clippy --all-targets -- -D warnings` says a
# word about this; it is the runtime half of #1472.
DEFEATED_UNION = """\
#[cfg(all(test, any(test, feature = "php", feature = "groovy")))]
mod hidden_literal_supertypes {
    #[test]
    fn php_hidden_string_supertype_is_unreachable() {}
}
"""

# The `hidden_literal_supertypes` bug as it actually shipped: the module
# carries no union at all. Pinned as a *known blind spot* — removing the
# gate removes the subject, so a scan of declared gates cannot see it.
# The compile half (unused imports under `-D warnings`) is what caught
# it, and that is clippy's job, not this gate's.
UNGATED_MODULE = """\
#[cfg(test)]
mod hidden_literal_supertypes {
    #[test]
    #[cfg(feature = "php")]
    fn php_hidden_string_supertype_is_unreachable() {}
}
"""


class ScannerTest(unittest.TestCase):
    def test_single_line_union_on_a_test_fn(self) -> None:
        source = """\
#[test]
#[cfg(any(feature = "perl", feature = "ruby"))]
fn the_baseline_values_are_one_and_four() {}
"""
        (subject,) = gate.scan_source(source, "x.rs")
        self.assertEqual(subject.kind, "fn")
        self.assertEqual(subject.name, "the_baseline_values_are_one_and_four")
        self.assertEqual(subject.features, frozenset({"perl", "ruby"}))
        self.assertTrue(subject.is_test_fn)
        self.assertTrue(subject.carries_tests)

    def test_multi_line_union_on_a_module(self) -> None:
        """The spelling most subjects actually use.

        A single-line `rg` over the tree finds 20 gates; the tree holds
        39, because the wide unions wrap. A scanner that only read one
        line would silently check half the subjects.
        """
        source = """\
#[cfg(test)]
#[cfg(any(
    feature = "java",
    feature = "javascript",
    feature = "kotlin"
))]
mod nameless_construct_boundaries {
}
"""
        (subject,) = gate.scan_source(source, "x.rs")
        self.assertEqual(subject.kind, "mod")
        self.assertEqual(
            subject.features, frozenset({"java", "javascript", "kotlin"})
        )
        self.assertTrue(subject.carries_tests)

    def test_all_test_any_nesting_is_recognised(self) -> None:
        (subject,) = gate.scan_source(WELL_GATED, "x.rs")
        self.assertEqual(subject.features, frozenset({"php", "groovy"}))
        self.assertTrue(subject.is_cfg_test)

    def test_intervening_attributes_and_comments_do_not_break_the_run(self) -> None:
        source = """\
#[cfg(any(feature = "bash", feature = "php"))]
// A comment between the gate and the item is legal Rust.
#[allow(clippy::needless_pass_by_value)]
fn assert_four_code_rows() {}
"""
        (subject,) = gate.scan_source(source, "x.rs")
        self.assertEqual(subject.name, "assert_four_code_rows")
        self.assertEqual(subject.features, frozenset({"bash", "php"}))

    def test_a_helper_fn_is_scanned_but_not_checked(self) -> None:
        """No `#[test]`, so no test name exists for nextest to report.

        Its absence under a disjoint feature set is a compile-time
        property the leg's clippy run already covers.
        """
        source = """\
#[cfg(any(feature = "mozjs", feature = "typescript"))]
fn assert_class_static_block_space(lang: LANG) {}
"""
        (subject,) = gate.scan_source(source, "x.rs")
        self.assertFalse(subject.carries_tests)

    def test_a_gate_inside_a_raw_string_fixture_is_not_a_subject(self) -> None:
        """Rust fixtures in this tree are raw strings of real Rust.

        `src/spaces_tests.rs` embeds a `cfg(any(feature = …))` as test
        input; recorded as a subject it would be permanently
        unsatisfiable, since no build can make a string literal absent.
        """
        source = '''\
const SOURCE: &str = r#"
#[cfg(any(feature = "kotlin", feature = "java"))]
mod embedded_example {}
"#;
'''
        self.assertEqual(gate.scan_source(source, "x.rs"), [])

    def test_a_gate_inside_a_block_comment_does_not_gate_the_code_below(self) -> None:
        """The comment closes on the attribute's own line.

        That is what makes this shape discriminating: the item below is
        live code outside every span, so only checking the *item* for a
        comment span is not enough — the attribute has to be checked
        too, or a commented-out gate is attributed to a real function
        that carries none.
        """
        source = """\
/* stale gate, kept for reference:
#[cfg(any(feature = "kotlin", feature = "java"))] */
fn real_fn() {}
"""
        self.assertEqual(gate.scan_source(source, "x.rs"), [])

    def test_a_commented_out_gate_is_not_a_subject(self) -> None:
        source = """\
// #[cfg(any(feature = "a", feature = "b"))]
// fn documented_example() {}
fn real_fn() {}
"""
        self.assertEqual(gate.scan_source(source, "x.rs"), [])

    def test_an_inverted_union_is_not_a_subject(self) -> None:
        """`not(any(…))` is present precisely when none is enabled.

        That is the opposite claim, so recording it would turn a
        deliberate fallback into a permanent failure.
        """
        source = """\
#[test]
#[cfg(not(any(feature = "perl", feature = "ruby")))]
fn the_fallback_path() {}
"""
        self.assertEqual(gate.scan_source(source, "x.rs"), [])

    def test_an_ungated_module_is_a_known_blind_spot(self) -> None:
        """The shipped `hidden_literal_supertypes` shape, pinned as absent.

        Deleting a gate deletes the subject, so a scan of declared gates
        has nothing to find. Asserted rather than left implicit so the
        next reader does not assume coverage this gate does not have.
        """
        self.assertEqual(gate.scan_source(UNGATED_MODULE, "x.rs"), [])


class FlagParsingTest(unittest.TestCase):
    def test_feature_spellings(self) -> None:
        for argv, expected in [
            (["--features", "a,b"], {"a", "b"}),
            (["--features=a,b"], {"a", "b"}),
            (["-F", "a b"], {"a", "b"}),
            (["--features", "a", "--features", "b"], {"a", "b"}),
        ]:
            with self.subTest(argv=argv):
                self.assertEqual(set(gate.parse_build_flags(argv).requested), expected)

    def test_package_and_default_suppression(self) -> None:
        flags = gate.parse_build_flags(
            ["--no-default-features", "--features", "go", "-p", "big-code-analysis"]
        )
        self.assertEqual(flags.package, "big-code-analysis")
        self.assertTrue(flags.no_default_features)

    def test_default_features_resolve_transitively(self) -> None:
        """`default = ["all-languages"]` must expand to the languages.

        Without the closure every subject would read as disjoint under
        the default leg and the gate would fail the whole matrix.
        """
        table = {
            "default": ["all-languages"],
            "all-languages": ["go", "rust", "dep:gix", "other-crate/feat"],
            "go": [],
            "rust": [],
        }
        enabled = gate.resolve_features(table, gate.parse_build_flags([]))
        self.assertIn("go", enabled)
        self.assertIn("rust", enabled)
        # `dep:` and `crate/feature` entries name nothing this crate's
        # own `cfg(feature = …)` can test.
        self.assertNotIn("dep:gix", enabled)
        self.assertNotIn("other-crate/feat", enabled)

        none = gate.resolve_features(
            table, gate.parse_build_flags(["--no-default-features"])
        )
        self.assertEqual(none, set())

    def test_all_features_enables_every_declared_feature(self) -> None:
        table = {"default": [], "go": [], "rust": []}
        enabled = gate.resolve_features(table, gate.parse_build_flags(["--all-features"]))
        self.assertEqual(enabled, {"default", "go", "rust"})


class SubjectMatcherTest(unittest.TestCase):
    def test_the_match_is_on_whole_path_segments(self) -> None:
        """A prefix must not match, or a rename silently widens the check."""
        subject = gate.scan_source(
            '#[test]\n#[cfg(any(feature = "go", feature = "rust"))]\nfn a_type() {}\n',
            "x.rs",
        )[0]
        matcher = gate.subject_matcher(subject)
        self.assertTrue(matcher.search("metrics::container_scope_tests::a_type"))
        self.assertTrue(matcher.search("a_type::inner"))
        self.assertFalse(matcher.search("metrics::a_type_declared_inside"))

    def test_a_nested_subject_matches_on_its_whole_path(self) -> None:
        """A bare `tests` would match every `::tests::` path in the crate."""
        subject = _nested_mod_subject()
        matcher = gate.subject_matcher(subject)
        self.assertEqual(subject.qualified_name, "outer::tests")
        self.assertTrue(matcher.search("metrics::outer::tests::a_case"))
        self.assertFalse(matcher.search("metrics::halstead::tests::a_case"))


NESTED_IN_TEST_SCOPE = """
#[cfg(test)]
mod outer {
    #[cfg(any(feature = "php", feature = "groovy"))]
    mod tests {
        #[test]
        fn a_case() {}
    }
}
"""


def _nested_mod_subject():
    subjects = gate.scan_source(NESTED_IN_TEST_SCOPE, "x.rs")
    return next(s for s in subjects if s.kind == "mod")


class NestedSubjectTest(unittest.TestCase):
    """#1472 item 3: a union-gated `mod` under a `#[cfg(test)]` parent.

    It carries no `test` predicate of its own, so reading only its own
    attribute classified it "compile-time only" and never compared it
    against the build — a false pass in exactly the #1220 shape this
    gate exists to catch.
    """

    def test_it_is_checkable_despite_carrying_no_test_predicate(self) -> None:
        subject = _nested_mod_subject()
        self.assertFalse(subject.is_cfg_test, "it has no `test` of its own")
        self.assertTrue(subject.in_cfg_test_scope)
        self.assertTrue(subject.carries_tests)

    def test_a_mod_whose_brace_is_on_the_next_line_still_opens_a_scope(
        self,
    ) -> None:
        """Requiring the brace on the header line re-creates the bug.

        rustfmt keeps it there today, so nothing in this tree reaches
        the other spelling — which is exactly why it needs a fixture:
        the failure is silent, and it is the same false pass this whole
        change removes.
        """
        subject = _nested_mod_subject_in(
            """
#[cfg(test)]
mod outer
{
    #[cfg(any(feature = "php", feature = "groovy"))]
    mod tests { }
}
"""
        )
        self.assertTrue(subject.in_cfg_test_scope)
        self.assertTrue(subject.carries_tests)
        self.assertEqual(subject.qualified_name, "outer::tests")

    def test_a_mod_declaration_opens_no_scope(self) -> None:
        """`mod outer;` has no body, so it must not swallow what follows."""
        subjects = gate.scan_source(
            """
#[cfg(test)]
mod outer;

#[cfg(any(feature = "php", feature = "groovy"))]
fn helper() {}
""",
            "y.rs",
        )
        (helper,) = subjects
        self.assertEqual(helper.qualified_name, "helper")
        self.assertFalse(helper.in_cfg_test_scope)

    def test_a_mod_declaration_is_not_opened_by_a_later_impl_block(
        self,
    ) -> None:
        """The clearing `elif` needs an item or a `;` to fire.

        `impl` and `struct` are neither, so a pending `mod foo;` waited
        for their `{` and claimed the whole block. Every subject inside
        then reported `foo::…`, which `subject_matcher` cannot match
        against any nextest name — a silent pass.
        """
        subjects = gate.scan_source(
            """
#[cfg(test)]
mod outer;

impl Holder {
    #[test]
    #[cfg(any(feature = "php", feature = "groovy"))]
    fn a_union_gated_test() {}
}
""",
            "y.rs",
        )
        (test_fn,) = subjects
        self.assertEqual(test_fn.qualified_name, "a_union_gated_test")

    def test_an_exotic_line_separator_does_not_desynchronise_the_scan(
        self,
    ) -> None:
        """`splitlines()` breaks on more characters than `\n`.

        Masking turns a form feed inside a string into a space, so a
        scanner splitting the masked text with `splitlines()` gets fewer
        lines than the raw one and indexes off the end — an `IndexError`
        traceback rather than a finding.
        """
        subjects = gate.scan_source(
            '#[cfg(any(feature = "php"))]\n'
            'const F: &str = "a\x0cb";\n'
            "#[test]\n"
            '#[cfg(any(feature = "php", feature = "groovy"))]\n'
            "fn after_the_form_feed() {}\n",
            "z.rs",
        )
        self.assertIn(
            "after_the_form_feed", [subject.name for subject in subjects]
        )

    def test_a_non_test_scope_mod_is_still_compile_time_only(self) -> None:
        # Without a test scope there is no test name for nextest to
        # report, so the clippy run owns it as it always did.
        subject = _nested_mod_subject_in(
            """
mod outer {
    #[cfg(any(feature = "php", feature = "groovy"))]
    mod inner {}
}
"""
        )
        self.assertFalse(subject.carries_tests)


def _nested_mod_subject_in(source: str):
    return next(s for s in gate.scan_source(source, "x.rs") if s.kind == "mod")


class MainTest(unittest.TestCase):
    """Both directions, with the build stubbed out.

    `list_tests` is the only part that shells out to cargo; replacing it
    lets the self-test pin the decision rather than the toolchain.
    """

    def _run(
        self, argv: list[str], source: str, listed: list[str]
    ) -> tuple[int, str, str]:
        subjects = gate.scan_source(source, "fixture.rs")
        out, err = io.StringIO(), io.StringIO()
        original_scan, original_list = gate.scan_tree, gate.list_tests
        gate.scan_tree = lambda _root: subjects
        gate.list_tests = lambda _flags: listed
        try:
            with (
                contextlib.redirect_stdout(out),
                contextlib.redirect_stderr(err),
            ):
                code = gate.main(argv)
        finally:
            gate.scan_tree, gate.list_tests = original_scan, original_list
        return code, out.getvalue(), err.getvalue()

    LEG = ["--no-default-features", "--features", "go", "-p", "big-code-analysis"]

    def test_a_correctly_gated_subject_passes(self) -> None:
        code, out, err = self._run(self.LEG, WELL_GATED, [])
        self.assertEqual(code, 0, err)
        self.assertIn("feature-gates: OK", out)

    def test_a_defeated_union_gate_fails(self) -> None:
        code, _, err = self._run(
            self.LEG,
            DEFEATED_UNION,
            ["metrics::abc::hidden_literal_supertypes::php_hidden_string_supertype"],
        )
        self.assertEqual(code, 1)
        self.assertIn("hidden_literal_supertypes", err)
        self.assertIn("none enabled here", err)

    def test_an_enabled_row_means_nothing_to_verify(self) -> None:
        """A leg enabling one of the union's features skips the subject.

        Pinned because the subject *is* present there, and treating
        presence as the failure would red-X every default leg.
        """
        leg = ["--no-default-features", "--features", "php", "-p", "big-code-analysis"]
        code, out, err = self._run(leg, WELL_GATED, ["x::hidden_literal_supertypes::y"])
        self.assertEqual(code, 0, err)
        self.assertIn("nothing to verify", out)

    def test_a_scan_that_finds_nothing_is_a_failure(self) -> None:
        """The gate's own non-vacuity guard.

        A scanner broken by a formatting change would otherwise pass
        every leg while checking nothing — the exact shape of the defect
        it exists to prevent.
        """
        code, _, err = self._run(self.LEG, "fn plain() {}\n", [])
        self.assertEqual(code, 2)
        self.assertIn("no `#[cfg(any(feature", err)

    def test_another_package_is_skipped_rather_than_passed_silently(self) -> None:
        code, out, err = self._run(
            ["--no-default-features", "--features", "go", "-p", "big-code-analysis-ast"],
            WELL_GATED,
            ["x::hidden_literal_supertypes::y"],
        )
        self.assertEqual(code, 0, err)
        self.assertIn("skipped", out)


class RepositoryTest(unittest.TestCase):
    def test_the_scanner_finds_subjects_in_the_real_tree(self) -> None:
        subjects = gate.scan_tree(gate.SRC_DIR)
        self.assertTrue(subjects, "no union-gated subjects found under src/")
        self.assertTrue(
            [s for s in subjects if s.carries_tests],
            "no subject carries tests, so no leg can check anything",
        )

    def test_the_default_leg_enables_every_subject(self) -> None:
        """A full build must leave the gate nothing to verify.

        If it did not, the `features (default (lib))` leg would fail on
        a subject no build can ever satisfy.
        """
        table = gate.manifest_features(REPO_ROOT / "Cargo.toml")
        enabled = gate.resolve_features(table, gate.parse_build_flags([]))
        disjoint = [
            s
            for s in gate.scan_tree(gate.SRC_DIR)
            if s.carries_tests and not (s.features & enabled)
        ]
        self.assertEqual(
            [s.describe() for s in disjoint],
            [],
            "a subject names no feature the default build enables",
        )


if __name__ == "__main__":
    unittest.main()
