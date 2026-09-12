#!/usr/bin/env python3
"""Tests for check-versions.py.

Two kinds of test live here:

* Unit tests that import the module and exercise the pin-scanning
  patterns/helpers directly. These pin the two drift classes the
  hand-maintained regexes used to miss — the vendored `bca-tree-sitter-*`
  internal pins (#878) and the `recipes/ci.md` release pins (#879) —
  against synthetic inputs, including the exact strings the *old*
  patterns silently skipped.
* Tests for the excluded-crate lockfile check (#1234), which build a
  miniature workspace in a temp directory. The failure direction the
  issue reproduced — a bumped manifest against a lockfile
  `cargo update --workspace` never reached — cannot be staged in the
  checkout itself, because the gate must never mutate the tree.
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
import tempfile
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

    def test_sub_crate_consumer_pin_is_recognized(self) -> None:
        # #1376: the root pins `big-code-analysis-ast` at `=X.Y.Z` in
        # both `[dependencies]` and `[dev-dependencies]`. The pre-#1376
        # key alternative was a bare `big-code-analysis` under
        # `fullmatch`, so neither pin was seen and the gate still
        # reported every owned crate in lockstep.
        line = (
            'big-code-analysis-ast = { path = "big-code-analysis-ast", '
            'version = "=1.1.0", default-features = false }'
        )
        self.assertEqual(_scan_internal_pins(line), [("big-code-analysis-ast", "1.1.0")])

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
        # 10 vendored grammar pins, the 2 `big-code-analysis` consumer
        # pins, and the 2 `big-code-analysis-ast` pins the root carries
        # in `[dependencies]` and `[dev-dependencies]` since #1376
        # (14 total), all at the canonical workspace version.
        canonical = cv.workspace_version(REPO_ROOT)
        pins: list[tuple[str, str]] = []
        for manifest_path in cv.INTERNAL_PIN_MANIFESTS:
            pins += _scan_internal_pins(cv.read(REPO_ROOT / manifest_path))
        self.assertEqual(len(pins), 14, pins)
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


class ExcludedLockfileTest(unittest.TestCase):
    """#1234: an excluded crate's Cargo.lock must track its manifests.

    Fixtures are built in a temp tree rather than by perturbing the
    checkout: `check_excluded_lockfiles` takes its root as a parameter
    precisely so the failure direction can be exercised without a gate
    that mutates the working tree.
    """

    CANONICAL = "2.0.0"
    REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"

    @staticmethod
    def _lockfile(entries: list[tuple[str, str, str | None]]) -> str:
        """Render a Cargo.lock from `(name, version, source)` triples."""
        out = ["version = 4", ""]
        for name, version, source in entries:
            out += ["[[package]]", f'name = "{name}"', f'version = "{version}"']
            if source is not None:
                out.append(f'source = "{source}"')
            out.append("")
        return "\n".join(out)

    def _build(
        self,
        root: pathlib.Path,
        *,
        canonical: str,
        leaf_version: str,
        locked: list[tuple[str, str, str | None]] | None,
        lockless_leaf: bool = False,
    ) -> None:
        """Lay down a miniature workspace with one excluded leaf.

        `canonical` is what `[workspace.package]` declares (so the root
        package, which inherits it, moves with it); `leaf_version` is
        the excluded crate's own `[package].version`; `locked` is what
        its lockfile records, or None to omit the lockfile entirely.
        """
        excludes = ['"leaf"'] + (['"lockless"'] if lockless_leaf else [])
        (root / "Cargo.toml").write_text(
            "[workspace]\n"
            'members = ["member"]\n'
            f"exclude = [{', '.join(excludes)}, \".claude/worktrees\"]\n\n"
            f'[workspace.package]\nversion = "{canonical}"\n\n'
            '[package]\nname = "rootpkg"\nversion.workspace = true\n',
            encoding="utf-8",
        )
        (root / "member").mkdir()
        (root / "member" / "Cargo.toml").write_text(
            '[package]\nname = "memberpkg"\nversion.workspace = true\n',
            encoding="utf-8",
        )
        (root / "leaf").mkdir()
        (root / "leaf" / "Cargo.toml").write_text(
            # The package name deliberately differs from the directory
            # name, as `tree-sitter-tcl/` -> `bca-tree-sitter-tcl` does.
            f'[package]\nname = "leafpkg"\nversion = "{leaf_version}"\n',
            encoding="utf-8",
        )
        if locked is not None:
            (root / "leaf" / "Cargo.lock").write_text(
                self._lockfile(locked), encoding="utf-8"
            )
        if lockless_leaf:
            (root / "lockless").mkdir()
            (root / "lockless" / "Cargo.toml").write_text(
                '[package]\nname = "locklesspkg"\nversion = "1.0.0"\n',
                encoding="utf-8",
            )

    def _failures(
        self,
        *,
        leaf_version: str,
        locked: list[tuple[str, str, str | None]] | None,
        canonical: str = CANONICAL,
        lockless_leaf: bool = False,
    ) -> list[str]:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            self._build(
                root,
                canonical=canonical,
                leaf_version=leaf_version,
                locked=locked,
                lockless_leaf=lockless_leaf,
            )
            return cv.check_excluded_lockfiles(root, canonical)

    def test_consistent_tree_passes(self) -> None:
        self.assertEqual(
            self._failures(
                leaf_version="2.0.0",
                locked=[
                    ("leafpkg", "2.0.0", None),
                    ("rootpkg", "2.0.0", None),
                    ("serde", "1.0.219", self.REGISTRY),
                ],
            ),
            [],
        )

    def test_bumped_manifest_with_stale_lockfile_fails(self) -> None:
        # The reproducer from #1234: the bump moved both the workspace
        # version and the leaf's own, and `cargo update --workspace`
        # reached neither entry of the leaf's lockfile.
        failures = self._failures(
            canonical="2.1.0",
            leaf_version="2.1.0",
            locked=[("leafpkg", "2.0.0", None), ("rootpkg", "2.0.0", None)],
        )
        self.assertEqual(len(failures), 1, failures)
        message = failures[0]
        # The whole point is that the failure currently misattributes
        # itself, so the message must name the file, the packages, and
        # both versions.
        self.assertIn("leaf/Cargo.lock", message)
        self.assertIn("leafpkg locked at '2.0.0'", message)
        self.assertIn("leaf/Cargo.toml declares '2.1.0'", message)
        self.assertIn("rootpkg locked at '2.0.0'", message)
        self.assertIn("Cargo.toml declares '2.1.0'", message)
        self.assertIn("cargo update --manifest-path leaf/Cargo.toml", message)

    def test_registry_entry_at_another_version_is_ignored(self) -> None:
        # A `source`-carrying entry is a registry dependency; its
        # version has nothing to do with this repository's.
        self.assertEqual(
            self._failures(
                leaf_version="2.0.0",
                locked=[
                    ("leafpkg", "2.0.0", None),
                    # Same name as an owned crate, but resolved from the
                    # registry — the published copy of an older release.
                    ("rootpkg", "1.0.0", self.REGISTRY),
                ],
            ),
            [],
        )

    def test_excluded_crate_without_a_lockfile_is_skipped(self) -> None:
        # An excluded crate only grows a Cargo.lock once something
        # resolves it; absence is not staleness. A second, locked leaf
        # keeps the vacuity guard from firing for an unrelated reason.
        self.assertEqual(
            self._failures(
                leaf_version="2.0.0",
                locked=[("leafpkg", "2.0.0", None)],
                lockless_leaf=True,
            ),
            [],
        )

    def test_unowned_path_package_is_reported(self) -> None:
        # A source-less entry naming no manifest here cannot be
        # adjudicated. Staying quiet would leave a lockfile entry that
        # nothing checks, which is the shape of the original gap.
        failures = self._failures(
            leaf_version="2.0.0",
            locked=[("leafpkg", "2.0.0", None), ("stranger", "0.1.0", None)],
        )
        self.assertEqual(len(failures), 1, failures)
        self.assertIn("stranger at '0.1.0' resolves to no manifest here", failures[0])

    def test_versionless_entry_is_reported_not_dropped(self) -> None:
        # A half-written entry for an owned package cannot be compared,
        # and dropping it would silently check one entry fewer than the
        # lockfile holds.
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            self._build(
                root,
                canonical=self.CANONICAL,
                leaf_version="2.0.0",
                locked=[("leafpkg", "2.0.0", None)],
            )
            (root / "leaf" / "Cargo.lock").write_text(
                'version = 4\n\n[[package]]\nname = "leafpkg"\nversion = "2.0.0"\n\n'
                '[[package]]\nname = "rootpkg"\n',
                encoding="utf-8",
            )
            failures = cv.check_excluded_lockfiles(root, self.CANONICAL)
        self.assertEqual(len(failures), 1, failures)
        self.assertIn(f"rootpkg locked at {cv.NO_VERSION!r}", failures[0])

    def test_empty_lockfile_is_reported(self) -> None:
        # Every real Cargo.lock records at least its own root package.
        failures = self._failures(leaf_version="2.0.0", locked=[])
        self.assertEqual(len(failures), 1, failures)
        self.assertIn("no path packages recorded", failures[0])

    def test_no_candidates_is_not_a_pass(self) -> None:
        # The vacuity guard. If the `[workspace] exclude` derivation
        # ever returns nothing, the gate must say so rather than report
        # lockstep having examined no lockfile at all.
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "Cargo.toml").write_text(
                "[workspace]\nmembers = []\n"
                'exclude = [".claude/worktrees"]\n\n'
                '[workspace.package]\nversion = "2.0.0"\n',
                encoding="utf-8",
            )
            failures = cv.check_excluded_lockfiles(root, "2.0.0")
        self.assertEqual(len(failures), 1, failures)
        self.assertIn("derivation in check-versions.py has broken", failures[0])

    def test_fuzz_lockfile_is_in_scope(self) -> None:
        # The correction to #1234's own resolution plan: deriving the
        # candidate list from EXCLUDED_LEAF_DIRS would have skipped
        # `fuzz`, which is deliberately version 0.0.0 and therefore
        # absent from that tuple — yet `fuzz/Cargo.lock` is one of the
        # two lockfiles the issue exists for. The `[workspace] exclude`
        # derivation covers it.
        self.assertNotIn("fuzz", cv.EXCLUDED_LEAF_DIRS)
        crates = [crate for crate, _ in cv.excluded_lockfiles(REPO_ROOT)]
        self.assertIn("fuzz", crates)
        self.assertIn("enums", crates)

    def test_real_lockfiles_are_in_step(self) -> None:
        canonical = cv.workspace_version(REPO_ROOT)
        self.assertEqual(cv.check_excluded_lockfiles(REPO_ROOT, canonical), [])

    def test_real_lockfiles_would_flag_on_a_bump(self) -> None:
        # Proof the real files are actually in scope, not merely
        # parsed: at a hypothetical canonical version every lockfile
        # recording a workspace-inheriting crate goes stale. `fuzz`
        # must be among them — it records `big-code-analysis`.
        failures = cv.check_excluded_lockfiles(REPO_ROOT, "9.9.9")
        self.assertTrue(failures)
        self.assertTrue(
            any(f.startswith("fuzz/Cargo.lock:") for f in failures), failures
        )

    def test_package_name_is_read_from_the_manifest(self) -> None:
        # `tree-sitter-tcl/` publishes as `bca-tree-sitter-tcl`, so the
        # version map cannot key on the directory name.
        versions = cv.owned_package_versions(REPO_ROOT, "2.0.0")
        self.assertIn("bca-tree-sitter-tcl", versions)
        self.assertNotIn("tree-sitter-tcl", versions)
        # `fuzz` is 0.0.0 by design, and must be compared against its
        # own manifest rather than the canonical workspace version.
        self.assertEqual(versions["big-code-analysis-fuzz"][0], "0.0.0")
        # A member inheriting `version.workspace = true` resolves to the
        # canonical version passed in.
        self.assertEqual(versions["big-code-analysis-cli"][0], "2.0.0")


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
