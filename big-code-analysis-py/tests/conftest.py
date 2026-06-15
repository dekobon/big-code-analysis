"""Shared pytest fixtures for the bindings test suite.

The ``bca_binary`` fixture is the single source of truth for the
``bca`` CLI binary used by ``test_smoke.py``, ``test_sarif.py``,
and ``test_book_examples.py``. Previously each file kept its own
copy; consolidating here means cargo build runs **once per pytest
session** instead of once per parity-aware test file, AND a future
tweak (sccache integration, release-profile fallback) lives in one
place.
"""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]


def _workspace_target_dir() -> Path:
    """Resolve the cargo target directory, honouring ``$CARGO_TARGET_DIR``.

    ``Path(env_dir).resolve()`` is critical when ``$CARGO_TARGET_DIR``
    is relative (e.g. ``./cache`` from a script that ``cd``-ed into
    the workspace before invoking cargo). A bare ``Path(env_dir)``
    would be relative to pytest's CWD, not cargo's — pytest invoked
    from outside the bindings dir would miss the binary even though
    cargo wrote it correctly.
    """
    env_dir = os.environ.get("CARGO_TARGET_DIR")
    if env_dir:
        return Path(env_dir).resolve()
    return REPO_ROOT / "target"


def _locate_workspace_binary(preferred_profile: str = "debug") -> str | None:
    """Look for a built ``bca`` under the workspace target dir.

    Returns ``preferred_profile``'s binary whenever it exists, falling
    back to the other profile only when the preferred one is absent.

    The caller below always builds the **debug** profile, so after a
    successful build the debug binary is authoritative — it is the one
    whose freshness cargo just guaranteed. An mtime-newest heuristic is
    *wrong* here: a successful ``cargo build`` is a no-op that does not
    touch the debug binary's mtime when the source is already up to
    date, so a stale ``release/bca`` left over from a prior branch (with
    a newer mtime) would shadow the binary the fixture just validated
    (#920). Pin to the profile actually built instead.
    """
    target = _workspace_target_dir()
    exe = ".exe" if os.name == "nt" else ""
    fallback_profile = "release" if preferred_profile == "debug" else "debug"
    for profile in (preferred_profile, fallback_profile):
        candidate = target / profile / f"bca{exe}"
        if candidate.is_file():
            return str(candidate)
    return None


@pytest.fixture(scope="session")
def bca_binary() -> str:
    """Session-scoped path to the ``bca`` CLI binary, built if needed.

    Honours ``$BCA_BINARY`` for callers that pre-built the CLI in a
    separate job (artifact upload/download in CI); otherwise invokes
    ``cargo build -p big-code-analysis-cli`` and resolves the
    resulting path under the effective target dir. Fails the fixture
    rather than skipping silently — the parity contract requires a
    matching CLI to run at all.
    """
    env_path = os.environ.get("BCA_BINARY")
    if env_path:
        if not Path(env_path).is_file():
            pytest.fail(
                f"$BCA_BINARY={env_path!r} does not point at a "
                "regular file; either fix the path or unset to let "
                "the fixture build the CLI."
            )
        return env_path
    cargo = shutil.which("cargo")
    if cargo is None:
        pytest.fail(
            "bca CLI cannot be built: `cargo` is not on PATH. Set "
            "$BCA_BINARY to a prebuilt binary, or install Rust and "
            "re-run."
        )
    result = subprocess.run(
        [cargo, "build", "-p", "big-code-analysis-cli", "--quiet"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        pytest.fail(
            "`cargo build -p big-code-analysis-cli` failed; parity "
            f"tests cannot run.\nstdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    built = _locate_workspace_binary()
    if built is None:
        target = _workspace_target_dir()
        pytest.fail(
            f"cargo build succeeded but no bca binary was found "
            f"under {target}/{{debug,release}}/. If you set "
            "$CARGO_TARGET_DIR, ensure the fixture sees the same "
            "value as the cargo invocation."
        )
    return built
