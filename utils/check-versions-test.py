#!/usr/bin/env python3
"""Tests for check-versions.py.

Two kinds of test live here:

* Unit tests that import the module and exercise the pin-scanning
  patterns/helpers directly. These pin the two drift classes the
  hand-maintained regexes used to miss — the vendored `bca-tree-sitter-*`
  internal pins (#878) and the `recipes/ci.md` release pins (#879) —
  against synthetic inputs, including the exact strings the *old*
  patterns silently skipped.
* A smoke test that runs the real script against the real repo and
  asserts a clean tree reports lockstep.

Run with:
    python3 -m unittest -q utils/check-versions-test.py
"""

from __future__ import annotations

import importlib.util
import pathlib
import subprocess
import sys
import unittest

# The gate under test is a sibling in `utils/`; every path it reads
# or writes is anchored at the repository root one level above.
UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-versions.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_versions", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


cv = _load_module()


def _scan_internal_pins(text: str) -> list[tuple[str, str]]:
    """Mirror main()'s internal-pin scan over a manifest snippet."""
    out: list[tuple[str, str]] = []
    for table in cv.INTERNAL_TABLE_RE.finditer(text):
        body = table.group("body")
        if not cv._is_internal_table(table.group("key"), body):
            continue
        pin = cv.INTERNAL_VERSION_PIN_RE.search(body)
        if pin is not None:
            out.append((table.group("key"), pin.group(1)))
    return out


class InternalVendoredPinTest(unittest.TestCase):
    """#878: vendored `bca-tree-sitter-*` pins must be validated."""

    VENDORED = (
        'tree-sitter-mozcpp = { package = "bca-tree-sitter-mozcpp", '
        'path = "./tree-sitter-mozcpp", version = "=1.1.0" }'
    )

    def test_vendored_pin_is_recognized(self) -> None:
        # The key is the upstream alias; the bca-* name is only in
        # `package = "..."`. main()'s scan must still see the pin.
        self.assertEqual(
            _scan_internal_pins(self.VENDORED),
            [("tree-sitter-mozcpp", "1.1.0")],
        )

    def test_vendored_pin_recognized_regardless_of_field_order(self) -> None:
        # `version` may precede `package`; the span scan is order-free.
        line = (
            'tree-sitter-x = { version = "=2.2.2", '
            'package = "bca-tree-sitter-x", path = "./x" }'
        )
        self.assertEqual(_scan_internal_pins(line), [("tree-sitter-x", "2.2.2")])

    def test_consumer_pin_still_recognized(self) -> None:
        # The `big-code-analysis = { … }` consumer form (the only
        # internal pins the old regex caught) must keep working.
        line = 'big-code-analysis = { path = "..", version = "=1.1.0" }'
        self.assertEqual(_scan_internal_pins(line), [("big-code-analysis", "1.1.0")])

    def test_external_grammar_table_not_treated_as_internal(self) -> None:
        # A non-vendored grammar declared as an inline table (no
        # bca-* package alias) must NOT be swept into the internal pin
        # set — it is enforced separately, against the upstream pin.
        line = 'tree-sitter-bash = { version = "=0.25.1" }'
        self.assertEqual(_scan_internal_pins(line), [])

    def test_old_key_only_pattern_would_have_missed_vendored(self) -> None:
        # Regression marker: the pre-#878 key-anchored regex returned
        # nothing for the vendored form, which is exactly the silent
        # skip this fix closes. If the scan were ever reverted to a
        # key-only match, test_vendored_pin_is_recognized fails; this
        # test documents *why*.
        import re

        old = re.compile(
            r"(?:bca-tree-sitter-\w+|big-code-analysis)\s*=\s*\{"
            r"[^}]*?\bversion\s*=\s*\"=([^\"]+)\""
        )
        self.assertEqual(old.findall(self.VENDORED), [])

    def test_real_manifests_expose_all_vendored_pins(self) -> None:
        # Across the real INTERNAL_PIN_MANIFESTS the scan must find the
        # 10 vendored grammar pins plus the 2 consumer pins (12 total),
        # all at the canonical workspace version.
        canonical = cv.workspace_version(REPO_ROOT)
        pins: list[tuple[str, str]] = []
        for manifest_path in cv.INTERNAL_PIN_MANIFESTS:
            pins += _scan_internal_pins(cv.read(REPO_ROOT / manifest_path))
        self.assertEqual(len(pins), 12, pins)
        self.assertTrue(all(ver == canonical for _, ver in pins), pins)


class CiRecipePinTest(unittest.TestCase):
    """#879: recipes/ci.md release pins must be checked in lockstep."""

    def _stale_lines(self, text: str, canonical: str) -> list[str]:
        out: list[str] = []
        for m in cv.CI_PIN_RE.finditer(text):
            cited = next(g for g in m.groups() if g is not None)
            if cv.normalize(cited, canonical) != canonical:
                out.append(cited)
        return out

    def test_all_three_install_forms_match(self) -> None:
        text = (
            'BCA_VERSION: "1.1.0"\n'
            "tool: big-code-analysis-cli@1.1.0\n"
            "cargo binstall big-code-analysis-cli --version 1.1.0\n"
        )
        # All three forms cite 1.1.0; on a bump to 1.2.0 all three are
        # stale.
        self.assertEqual(self._stale_lines(text, "1.2.0"), ["1.1.0"] * 3)

    def test_prerelease_install_forms_match(self) -> None:
        # A pre-release cut (e.g. 2.0.0-rc1) pins the CI recipe at the
        # full pre-release version; CI_PIN_RE must capture the `-rc1`
        # suffix whole so the pin matches canonical instead of matching
        # only the bare `2.0.0` core and tripping on the suffix.
        text = (
            'BCA_VERSION: "2.0.0-rc1"\n'
            "tool: big-code-analysis-cli@2.0.0-rc1\n"
            "cargo binstall big-code-analysis-cli --version 2.0.0-rc1\n"
        )
        self.assertEqual(self._stale_lines(text, "2.0.0-rc1"), [])
        # The bare X.Y.Z core is the wrong pin while canonical is a
        # pre-release, and is still flagged stale.
        self.assertEqual(
            self._stale_lines(text.replace("-rc1", ""), "2.0.0-rc1"),
            ["2.0.0"] * 3,
        )

    def test_cache_key_form_is_not_matched(self) -> None:
        # The `key: bca-…-X.Y.Z` GitHub Actions cache key is
        # deliberately exempt — a stale cache key is only a cache miss.
        text = "key: bca-${{ runner.os }}-1.1.0\n"
        self.assertEqual(self._stale_lines(text, "1.2.0"), [])

    def test_runtime_version_output_is_not_matched(self) -> None:
        # A `bca --version` example printing runtime output is install
        # documentation noise, not a pin; the patterns key on install
        # invocations, so a bare "big-code-analysis-cli 1.1.0" line is
        # not over-matched.
        text = "$ bca --version\nbig-code-analysis-cli 1.1.0\n"
        self.assertEqual(self._stale_lines(text, "1.2.0"), [])

    def test_real_ci_md_pins_are_at_a_published_release(self) -> None:
        # ci.md pins track the published releases (latest or the one
        # before), never the workspace version — they can only move
        # once the release's SHA256SUMS exists.
        ci_allowed = cv.released_versions(REPO_ROOT)[:2]
        for ci_path in cv.CI_RECIPE_FILES:
            text = cv.read(REPO_ROOT / ci_path)
            stale = [
                cited
                for m in cv.CI_PIN_RE.finditer(text)
                for cited in [next(g for g in m.groups() if g is not None)]
                if not cv.matches_any(cited, ci_allowed)
            ]
            self.assertEqual(stale, [], ci_path)

    def test_real_ci_md_would_flag_on_bump(self) -> None:
        # On a hypothetical bump, the real ci.md pins go stale — proving
        # the file is actually in scope (the gap #879 closed).
        for ci_path in cv.CI_RECIPE_FILES:
            text = cv.read(REPO_ROOT / ci_path)
            stale = self._stale_lines(text, "9.9.9")
            self.assertTrue(stale, f"{ci_path} should expose stale pins on a bump")


class ChangelogReleaseTest(unittest.TestCase):
    """Doc pins are checked against CHANGELOG's released sections."""

    SYNTHETIC = (
        "# Changelog\n\n"
        "## [Unreleased]\n\n### Added\n- something\n\n"
        "## [2.1.0] - 2026-08-01\n\n### Added\n- thing\n\n"
        "## [2.0.0] - 2026-06-29\n\n### Changed\n- other\n\n"
        "## [2.0.0-rc1] - 2026-06-19\n\n- rc\n"
    )

    def test_release_headers_parse_newest_first(self) -> None:
        found = cv.CHANGELOG_RELEASE_RE.findall(self.SYNTHETIC)
        self.assertEqual(found, ["2.1.0", "2.0.0", "2.0.0-rc1"])

    def test_unreleased_section_is_not_a_release(self) -> None:
        found = cv.CHANGELOG_RELEASE_RE.findall("## [Unreleased]\n")
        self.assertEqual(found, [])

    def test_real_changelog_has_a_released_section(self) -> None:
        released = cv.released_versions(REPO_ROOT)
        self.assertTrue(released, "CHANGELOG.md must have a released section")
        self.assertRegex(released[0], r"^\d+\.\d+\.\d+")

    def test_ci_pins_may_lag_one_release_but_not_two(self) -> None:
        allowed = ["2.1.0", "2.0.0"]
        self.assertTrue(cv.matches_any("2.1.0", allowed))
        self.assertTrue(cv.matches_any("2.0.0", allowed))  # lag of one
        self.assertFalse(cv.matches_any("1.1.0", allowed))  # lag of two

    def test_doc_pin_prefix_still_normalizes(self) -> None:
        # The README's major-line form ("2") must satisfy the latest
        # published release via prefix normalization.
        self.assertEqual(cv.normalize("2", "2.1.0"), "2.1.0")


class SmokeTest(unittest.TestCase):
    def test_clean_repo_reports_lockstep(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPT_SRC)],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("versions OK", result.stdout)


if __name__ == "__main__":
    unittest.main()
