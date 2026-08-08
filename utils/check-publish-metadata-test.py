#!/usr/bin/env python3
"""Tests for check-publish-metadata.py.

Three kinds of test, matching the check-ruff-lockstep-test.py pattern:

* Unit tests against synthetic manifests and `cargo metadata`-shaped
  dicts, one case per regression the gate claims to catch and one per
  malformed input it must fail loudly on.
* ``audit()`` / ``main()`` tests over a synthetic workspace with the two
  cargo subprocess calls stubbed, covering each finding branch and the
  remediation text it prints.
* A smoke test running the real gate against the real repository,
  asserting a clean tree reports OK.

The size ceiling is exercised by patching ``MAX_PACKAGED_BYTES`` down
rather than by writing 32 MiB of fixtures: the branch under test is the
comparison, and both sides of the boundary matter.

Run with:
    python3 -m unittest -q utils/check-publish-metadata-test.py
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
from typing import Any
from unittest import mock

UTILS_DIR = pathlib.Path(__file__).resolve().parent
REPO_ROOT = UTILS_DIR.parent
SCRIPT_SRC = UTILS_DIR / "check-publish-metadata.py"


def _load_module():  # type: ignore[no-untyped-def]
    spec = importlib.util.spec_from_file_location("check_publish_metadata", SCRIPT_SRC)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GATE = _load_module()

# A package entry shaped like `cargo metadata --no-deps` emits one, with
# every field the gate requires populated. Values are deliberately
# distinct from each other and from the empty string so a test that
# mutates one field cannot pass because another happened to match.
GOOD_PACKAGE: dict[str, Any] = {
    "name": "demo",
    "description": "A demonstration crate",
    "readme": "README.md",
    "repository": "https://example.invalid/demo",
    "license": "MPL-2.0",
    "license_file": None,
    "publish": None,
    "manifest_path": "/ws/Cargo.toml",
}


def package(**overrides: Any) -> dict[str, Any]:
    """A `GOOD_PACKAGE` with fields replaced or removed.

    An override of ``None`` replaces the value; to drop a key entirely,
    pass it through ``drop``.
    """
    entry = dict(GOOD_PACKAGE)
    entry.update(overrides)
    return entry


def drop(entry: dict[str, Any], *keys: str) -> dict[str, Any]:
    """A copy of `entry` with `keys` absent rather than empty."""
    return {key: value for key, value in entry.items() if key not in keys}


class BlankTest(unittest.TestCase):
    def test_recognises_text(self) -> None:
        self.assertFalse(GATE._blank("x"))

    def test_absent_empty_and_whitespace_are_blank(self) -> None:
        for value in (None, "", "   ", "\n\t", 17, [], {"workspace": True}):
            with self.subTest(value=value):
                self.assertTrue(GATE._blank(value))


class MetadataFieldsTest(unittest.TestCase):
    def test_complete_package_has_no_findings(self) -> None:
        self.assertEqual(GATE.check_metadata_fields(GOOD_PACKAGE), [])

    def test_each_required_field_is_reported_when_absent(self) -> None:
        for field in ("description", "readme", "repository"):
            with self.subTest(field=field):
                problems = GATE.check_metadata_fields(drop(GOOD_PACKAGE, field))
                self.assertEqual(len(problems), 1, problems)
                self.assertIn(field, problems[0])

    def test_each_required_field_is_reported_when_empty(self) -> None:
        # Present-but-empty is the case a `in package` test would miss.
        for field in ("description", "readme", "repository"):
            with self.subTest(field=field):
                problems = GATE.check_metadata_fields(package(**{field: "   "}))
                self.assertEqual(len(problems), 1, problems)
                self.assertIn(field, problems[0])

    def test_missing_both_license_forms_is_reported(self) -> None:
        problems = GATE.check_metadata_fields(package(license=None, license_file=None))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("license", problems[0])

    def test_empty_license_string_is_reported(self) -> None:
        problems = GATE.check_metadata_fields(package(license="", license_file=None))
        self.assertEqual(len(problems), 1, problems)

    def test_license_file_alone_satisfies_the_licence_requirement(self) -> None:
        # crates.io accepts either spelling; requiring `license` would
        # reject a legitimate manifest.
        self.assertEqual(
            GATE.check_metadata_fields(package(license=None, license_file="LICENSE")),
            [],
        )

    def test_several_missing_fields_are_all_reported(self) -> None:
        problems = GATE.check_metadata_fields(
            drop(GOOD_PACKAGE, "description", "repository")
        )
        self.assertEqual(len(problems), 2, problems)


class ResolveIncludeTest(unittest.TestCase):
    def test_literal_list_is_returned(self) -> None:
        manifest = {"package": {"include": ["/src/**/*"]}}
        self.assertEqual(GATE.resolve_include(manifest, {}), ["/src/**/*"])

    def test_absent_include_resolves_to_none(self) -> None:
        self.assertIsNone(GATE.resolve_include({"package": {}}, {}))

    def test_workspace_inheritance_is_resolved(self) -> None:
        # The trap: `include.workspace = true` is a legitimate spelling
        # that a plain `.get("include")` reads as a dict, not a list.
        manifest = {"package": {"include": {"workspace": True}}}
        workspace = {"workspace": {"package": {"include": ["/src/**/*", "/README.md"]}}}
        self.assertEqual(
            GATE.resolve_include(manifest, workspace), ["/src/**/*", "/README.md"]
        )

    def test_inheritance_from_a_workspace_without_the_key_is_none(self) -> None:
        manifest = {"package": {"include": {"workspace": True}}}
        self.assertIsNone(GATE.resolve_include(manifest, {"workspace": {"package": {}}}))
        self.assertIsNone(GATE.resolve_include(manifest, {}))

    def test_unrecognised_include_table_is_a_hard_error(self) -> None:
        for value in ({"workspace": False}, {"path": "x"}, {}):
            with self.subTest(value=value):
                with self.assertRaises(SystemExit) as caught:
                    GATE.resolve_include({"package": {"include": value}}, {})
                self.assertIn("unrecognised", str(caught.exception))

    def test_manifest_without_a_package_table_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.resolve_include({"workspace": {}}, {})
        self.assertIn("[package]", str(caught.exception))


class CheckIncludeTest(unittest.TestCase):
    def test_populated_include_passes(self) -> None:
        manifest = {"package": {"include": ["/src/**/*", "/Cargo.toml"]}}
        self.assertEqual(GATE.check_include(manifest, {}), [])

    def test_absent_include_is_reported(self) -> None:
        problems = GATE.check_include({"package": {}}, {})
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("absent", problems[0])

    def test_empty_include_is_reported(self) -> None:
        problems = GATE.check_include({"package": {"include": []}}, {})
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("empty", problems[0])

    def test_include_of_only_blank_entries_is_reported(self) -> None:
        problems = GATE.check_include({"package": {"include": ["", "  "]}}, {})
        self.assertEqual(len(problems), 1, problems)

    def test_non_list_include_is_reported(self) -> None:
        problems = GATE.check_include({"package": {"include": "/src/**/*"}}, {})
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("malformed", problems[0])

    def test_inherited_include_passes(self) -> None:
        manifest = {"package": {"include": {"workspace": True}}}
        workspace = {"workspace": {"package": {"include": ["/src/**/*"]}}}
        self.assertEqual(GATE.check_include(manifest, workspace), [])

    def test_inherited_but_undeclared_include_is_reported(self) -> None:
        manifest = {"package": {"include": {"workspace": True}}}
        problems = GATE.check_include(manifest, {"workspace": {"package": {}}})
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("absent", problems[0])


class LoadTomlTest(unittest.TestCase):
    def test_parses_a_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path = pathlib.Path(raw) / "Cargo.toml"
            path.write_text('[package]\nname = "demo"\n', encoding="utf-8")
            self.assertEqual(GATE.load_toml(path)["package"]["name"], "demo")

    def test_missing_file_is_a_hard_error(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            with self.assertRaises(SystemExit) as caught:
                GATE.load_toml(pathlib.Path(raw) / "absent.toml")
            self.assertIn("cannot read", str(caught.exception))

    def test_malformed_toml_is_a_hard_error(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            path = pathlib.Path(raw) / "Cargo.toml"
            path.write_text("[package\nname =", encoding="utf-8")
            with self.assertRaises(SystemExit) as caught:
                GATE.load_toml(path)
            self.assertIn("not valid TOML", str(caught.exception))


class MeasureListingTest(unittest.TestCase):
    def _tree(self, root: pathlib.Path) -> None:
        (root / "src").mkdir()
        # Distinct sizes so a total can only be right for one reason.
        (root / "src" / "lib.rs").write_bytes(b"a" * 100)
        (root / "README.md").write_bytes(b"b" * 23)

    def test_sums_only_files_present_on_disk(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            self._tree(root)
            listing = ["src/lib.rs", "README.md", "Cargo.toml.orig", ".cargo_vcs_info.json"]
            self.assertEqual(GATE.measure_listing(listing, root), 123)

    def test_every_generated_entry_is_skipped(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            self.assertEqual(GATE.measure_listing(sorted(GATE.GENERATED_ENTRIES), root), 0)

    def test_an_unexpected_absent_entry_is_a_hard_error(self) -> None:
        # Without this the wrong-base-directory case totals zero bytes
        # and passes the ceiling, which is the silent pass being fixed.
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            self._tree(root)
            with self.assertRaises(SystemExit) as caught:
                GATE.measure_listing(["src/lib.rs", "src/gone.rs"], root)
            self.assertIn("gone.rs", str(caught.exception))


class PackagedSizeTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        (self.root / "src").mkdir()
        # 1_000 + 24 + 40 = 1_064, so the ceiling cases below can sit
        # exactly on the boundary and one byte past it.
        (self.root / "src" / "lib.rs").write_bytes(b"a" * 1_000)
        (self.root / "README.md").write_bytes(b"b" * 24)
        (self.root / "LICENSE").write_bytes(b"c" * 40)
        self.addCleanup(self._tmp.cleanup)

    def test_a_listing_under_the_ceiling_passes(self) -> None:
        listing = ["src/lib.rs", "README.md", "LICENSE"]
        self.assertEqual(GATE.check_packaged_size(listing, self.root), [])

    def test_a_listing_exactly_at_the_ceiling_passes(self) -> None:
        # The passing side of the boundary: a `>=` comparison reports
        # this one, so the bound cannot silently tighten.
        listing = ["src/lib.rs", "README.md", "LICENSE"]
        with mock.patch.object(GATE, "MAX_PACKAGED_BYTES", 1_064):
            self.assertEqual(GATE.check_packaged_size(listing, self.root), [])

    def test_a_listing_one_byte_over_the_ceiling_is_reported(self) -> None:
        listing = ["src/lib.rs", "README.md", "LICENSE"]
        with mock.patch.object(GATE, "MAX_PACKAGED_BYTES", 1_063):
            problems = GATE.check_packaged_size(listing, self.root)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("ceiling", problems[0])
        self.assertIn("include", problems[0])


class PublishablePackagesTest(unittest.TestCase):
    def test_publish_false_is_excluded(self) -> None:
        metadata = {
            "packages": [
                package(name="shipped", publish=None),
                package(name="private", publish=[]),
                package(name="registry-scoped", publish=["crates-io"]),
            ]
        }
        names = [entry["name"] for entry in GATE.publishable_packages(metadata)]
        self.assertEqual(names, ["shipped", "registry-scoped"])

    def test_missing_packages_array_is_a_hard_error(self) -> None:
        with self.assertRaises(SystemExit) as caught:
            GATE.publishable_packages({})
        self.assertIn("packages", str(caught.exception))


class RunCargoTest(unittest.TestCase):
    def test_stdout_is_returned_on_success(self) -> None:
        completed = subprocess.CompletedProcess(["cargo"], 0, stdout="out", stderr="")
        with mock.patch.object(subprocess, "run", return_value=completed):
            self.assertEqual(GATE.run_cargo(["metadata"], REPO_ROOT), "out")

    def test_non_zero_exit_is_a_hard_error_carrying_stderr(self) -> None:
        completed = subprocess.CompletedProcess(["cargo"], 101, stdout="", stderr="boom")
        with (
            mock.patch.object(subprocess, "run", return_value=completed),
            self.assertRaises(SystemExit) as caught,
        ):
            GATE.run_cargo(["metadata"], REPO_ROOT)
        self.assertIn("boom", str(caught.exception))

    def test_a_missing_cargo_binary_is_a_hard_error(self) -> None:
        with (
            mock.patch.object(subprocess, "run", side_effect=OSError("no cargo")),
            self.assertRaises(SystemExit) as caught,
        ):
            GATE.run_cargo(["metadata"], REPO_ROOT)
        self.assertIn("no cargo", str(caught.exception))


class CargoMetadataTest(unittest.TestCase):
    def test_json_is_parsed(self) -> None:
        with mock.patch.object(GATE, "run_cargo", return_value='{"packages": []}'):
            self.assertEqual(GATE.cargo_metadata(REPO_ROOT), {"packages": []})

    def test_non_json_output_is_a_hard_error(self) -> None:
        with (
            mock.patch.object(GATE, "run_cargo", return_value="not json"),
            self.assertRaises(SystemExit) as caught,
        ):
            GATE.cargo_metadata(REPO_ROOT)
        self.assertIn("did not emit JSON", str(caught.exception))

    def test_non_object_json_is_a_hard_error(self) -> None:
        with (
            mock.patch.object(GATE, "run_cargo", return_value="[1, 2]"),
            self.assertRaises(SystemExit) as caught,
        ):
            GATE.cargo_metadata(REPO_ROOT)
        self.assertIn("JSON object", str(caught.exception))


class AuditTest(unittest.TestCase):
    """`audit()` over a synthetic workspace, with `--list` stubbed."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)
        (self.root / "src").mkdir()
        (self.root / "src" / "lib.rs").write_bytes(b"a" * 1_000)
        (self.root / "README.md").write_bytes(b"b" * 24)
        self.member = self.root / "member"
        (self.member / "src").mkdir(parents=True)
        (self.member / "src" / "main.rs").write_bytes(b"c" * 500)
        (self.member / "README.md").write_bytes(b"d" * 12)
        self.write_manifest(include=["/src/**/*", "/README.md"])
        (self.member / "Cargo.toml").write_text(
            '[package]\nname = "member"\n', encoding="utf-8"
        )
        self.listings = {
            "root": ["src/lib.rs", "README.md", "Cargo.toml.orig"],
            "member": ["src/main.rs", "README.md"],
        }

    def write_manifest(self, include: Any) -> None:
        """Write the workspace-root manifest, `include` set as given."""
        body = '[workspace]\nmembers = ["member"]\n\n[package]\nname = "root"\n'
        if include is not None:
            body += f"include = {json.dumps(include)}\n"
        (self.root / "Cargo.toml").write_text(body, encoding="utf-8")

    def metadata(self, **root_overrides: Any) -> dict[str, Any]:
        return {
            "workspace_root": str(self.root),
            "packages": [
                package(
                    name="root",
                    manifest_path=str(self.root / "Cargo.toml"),
                    **root_overrides,
                ),
                package(name="member", manifest_path=str(self.member / "Cargo.toml")),
            ],
        }

    @contextlib.contextmanager
    def stubbed_listings(self):  # type: ignore[no-untyped-def]
        def listing(name: str, package_root: pathlib.Path) -> list[str]:
            return self.listings[name]

        with mock.patch.object(GATE, "package_listing", side_effect=listing):
            yield

    def test_a_compliant_workspace_reports_nothing(self) -> None:
        with self.stubbed_listings():
            self.assertEqual(GATE.audit(self.metadata()), [])

    def test_findings_name_the_offending_crate(self) -> None:
        with self.stubbed_listings():
            problems = GATE.audit(self.metadata(description=""))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("root:", problems[0])
        self.assertIn("description", problems[0])

    def test_include_is_required_only_at_the_workspace_root(self) -> None:
        # `member/Cargo.toml` carries no `include` and must not be
        # faulted for it: a package in a subdirectory already packages
        # only its own tree.
        with self.stubbed_listings():
            problems = GATE.audit(self.metadata())
        self.assertEqual(problems, [])

    def test_a_dropped_include_at_the_workspace_root_is_reported(self) -> None:
        self.write_manifest(include=None)
        with self.stubbed_listings():
            problems = GATE.audit(self.metadata())
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("root:", problems[0])
        self.assertIn("include", problems[0])

    def test_an_oversized_crate_is_reported(self) -> None:
        with self.stubbed_listings(), mock.patch.object(GATE, "MAX_PACKAGED_BYTES", 600):
            problems = GATE.audit(self.metadata())
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("root:", problems[0])
        self.assertIn("ceiling", problems[0])

    def test_a_workspace_with_no_publishable_member_is_a_hard_error(self) -> None:
        metadata = self.metadata()
        for entry in metadata["packages"]:
            entry["publish"] = []
        with self.assertRaises(SystemExit) as caught:
            GATE.audit(metadata)
        self.assertIn("would have checked nothing", str(caught.exception))

    def test_missing_workspace_root_is_a_hard_error(self) -> None:
        metadata = self.metadata()
        del metadata["workspace_root"]
        with self.assertRaises(SystemExit) as caught:
            GATE.audit(metadata)
        self.assertIn("workspace_root", str(caught.exception))

    def test_a_package_entry_without_a_manifest_path_is_a_hard_error(self) -> None:
        metadata = self.metadata()
        del metadata["packages"][0]["manifest_path"]
        with self.assertRaises(SystemExit) as caught:
            GATE.audit(metadata)
        self.assertIn("malformed", str(caught.exception))


class MainTest(unittest.TestCase):
    def _run(self, problems: list[str]) -> tuple[int, str, str]:
        metadata = {"packages": [package(publish=None)]}
        out, err = io.StringIO(), io.StringIO()
        with (
            mock.patch.object(GATE, "cargo_metadata", return_value=metadata),
            mock.patch.object(GATE, "audit", return_value=problems),
            contextlib.redirect_stdout(out),
            contextlib.redirect_stderr(err),
        ):
            code = GATE.main()
        return code, out.getvalue(), err.getvalue()

    def test_a_clean_workspace_exits_zero_and_reports_the_count(self) -> None:
        code, out, err = self._run([])
        self.assertEqual(code, 0)
        self.assertIn("1 publishable crates checked", out)
        self.assertEqual(err, "")

    def test_findings_exit_one_and_reach_stderr_with_remediation(self) -> None:
        code, out, err = self._run(["  demo: [package].description is missing or empty"])
        self.assertEqual(code, 1)
        self.assertEqual(out, "")
        self.assertIn("demo: [package].description", err)
        self.assertIn("RELEASING.md", err)
        self.assertIn("#1224", err)


class RealRepositoryTest(unittest.TestCase):
    """The gate against the tree it ships in."""

    def test_the_repository_passes_its_own_gate(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(SCRIPT_SRC)],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(
            completed.returncode, 0, f"{completed.stdout}\n{completed.stderr}"
        )
        self.assertIn("publish metadata OK", completed.stdout)

    def test_cargo_packages_the_declared_readme_without_being_asked(self) -> None:
        # The gate does not verify that `readme` reaches the archive,
        # because cargo already does: measured against cargo 1.95, it
        # fails `--list` outright on a readme that does not exist, and
        # packages one that `include` does not cover. Pinned here so a
        # future cargo that stopped doing either is a red test rather
        # than a silent loss of coverage.
        metadata = GATE.cargo_metadata(REPO_ROOT)
        parent = next(
            entry
            for entry in GATE.publishable_packages(metadata)
            if entry["name"] == "big-code-analysis"
        )
        self.assertIn(
            parent["readme"], GATE.package_listing("big-code-analysis", REPO_ROOT)
        )

    def test_the_three_top_level_crates_are_the_ones_checked(self) -> None:
        # Pins the discovery rule, not just that discovery found
        # something: `publish = false` on the bench and Python crates is
        # what keeps them out, and a change there must be deliberate.
        names = sorted(
            entry["name"] for entry in GATE.publishable_packages(GATE.cargo_metadata(REPO_ROOT))
        )
        self.assertEqual(
            names,
            ["big-code-analysis", "big-code-analysis-cli", "big-code-analysis-web"],
        )


if __name__ == "__main__":
    unittest.main()
