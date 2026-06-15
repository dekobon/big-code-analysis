"""Unit tests for the binary-resolution helpers in ``conftest.py``.

These helpers locate the ``bca`` CLI that gates the parity suite, so a
regression in them silently undermines every parity test. The case
pinned here is the bug fixed by #920 (stale-release shadowing).

Run via::

    python -m pytest big-code-analysis-py/tests/test_conftest_helpers.py
"""

from __future__ import annotations

import os
from pathlib import Path

import conftest
import pytest

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
    debug = tmp_path / "debug" / "bca"
    release = tmp_path / "release" / "bca"
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
    release = tmp_path / "release" / "bca"
    release.parent.mkdir(parents=True)
    release.write_text("release binary")

    assert conftest._locate_workspace_binary() == str(release)


def test_locator_returns_none_when_no_binary(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path))
    assert conftest._locate_workspace_binary() is None
