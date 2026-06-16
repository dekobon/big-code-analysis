"""Unit tests for the binary-resolution helpers in ``conftest.py``.

These helpers locate the ``bca`` CLI that gates the parity suite, so a
regression in them silently undermines every parity test. The two cases
pinned here are the bugs fixed by #920 (stale-release shadowing) and
#922 (relative ``$CARGO_TARGET_DIR`` anchored to the wrong base).

Run via::

    python -m pytest big-code-analysis-py/tests/test_conftest_helpers.py
"""

# This suite deliberately unit-tests the underscore-prefixed conftest
# helpers (`_locate_workspace_binary`, `_workspace_target_dir`); there is
# no public surface for the binary-resolution logic they encode, so the
# private access is intentional here only.
# pyright: reportPrivateUsage=false

from __future__ import annotations

import os
from pathlib import Path

import conftest
import pytest

# The locator appends the platform executable suffix (`.exe` on Windows),
# so the fixture files these tests create must use the same name or the
# locator finds nothing on Windows (mirrors `conftest._locate_workspace_binary`).
EXE = ".exe" if os.name == "nt" else ""

# ── #920: post-build locator must pick the profile cargo built ──────


def test_locator_prefers_built_profile_over_newer_mtime(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A stale ``release/bca`` with a *newer* mtime must not shadow the
    freshly-built ``debug/bca`` (#920).

    The fixture builds the debug profile, then resolves the path. A
    successful no-op ``cargo build`` does not touch the debug binary's
    mtime, so a leftover release binary from a prior branch can be
    newer. The post-build locator must pin to the profile actually
    built (debug), not the newest-by-mtime candidate.
    """
    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path))
    debug = tmp_path / "debug" / f"bca{EXE}"
    release = tmp_path / "release" / f"bca{EXE}"
    debug.parent.mkdir(parents=True)
    release.parent.mkdir(parents=True)
    debug.write_text("debug binary")
    release.write_text("release binary")
    # Make release the newer of the two — the exact trap the old
    # mtime-newest heuristic fell into.
    os.utime(debug, (1_000, 1_000))
    os.utime(release, (2_000, 2_000))

    located = conftest._locate_workspace_binary()
    assert located == str(debug), (
        "post-build locator must return the debug binary it built, not the "
        "newer-mtime release leftover"
    )


def test_locator_falls_back_to_release_when_debug_absent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """When the preferred profile is absent, fall back to the other one
    (e.g. a CI job that only produced a release artifact)."""
    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path))
    release = tmp_path / "release" / f"bca{EXE}"
    release.parent.mkdir(parents=True)
    release.write_text("release binary")

    assert conftest._locate_workspace_binary() == str(release)


def test_locator_returns_none_when_no_binary(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path))
    assert conftest._locate_workspace_binary() is None


# ── #922: relative CARGO_TARGET_DIR anchors to cargo's CWD ──────────


def test_relative_target_dir_anchored_to_repo_root_not_pytest_cwd(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A relative ``$CARGO_TARGET_DIR`` resolves against ``REPO_ROOT``
    (cargo's CWD), not pytest's CWD (#922).

    Cargo writes a relative target dir under its own CWD, which the
    fixture pins to ``REPO_ROOT``. Changing pytest's CWD must not move
    the resolved path — the old ``Path(env_dir).resolve()`` anchored to
    ``os.getcwd()`` and broke exactly this.
    """
    monkeypatch.setenv("CARGO_TARGET_DIR", "relsub")
    monkeypatch.chdir(tmp_path)  # a dir other than REPO_ROOT
    assert conftest._workspace_target_dir() == (conftest.REPO_ROOT / "relsub").resolve()


def test_absolute_target_dir_is_returned_unchanged(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """An absolute ``$CARGO_TARGET_DIR`` is unaffected by the REPO_ROOT
    join (``Path("/a") / "/b"`` yields ``/b``)."""
    abs_dir = tmp_path / "abscache"
    monkeypatch.setenv("CARGO_TARGET_DIR", str(abs_dir))
    assert conftest._workspace_target_dir() == abs_dir.resolve()


def test_unset_target_dir_defaults_to_repo_root_target(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv("CARGO_TARGET_DIR", raising=False)
    assert conftest._workspace_target_dir() == conftest.REPO_ROOT / "target"
