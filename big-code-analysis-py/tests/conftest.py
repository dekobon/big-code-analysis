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


def _locate_workspace_binary() -> str | None:
    """Look for a freshly-built ``bca`` under the workspace target dir.

    Prefers the binary with the newer mtime so a stale build from a
    previous branch checkout does not silently shadow a freshly-
    built one. The cargo-build path in the fixture below already
    refreshes whichever binary cargo writes, but on a fresh check-
    out with both debug and release already populated from prior
    branches, mtime preference is the safer default.
    """
    target = _workspace_target_dir()
    candidates = [target / "debug" / "bca", target / "release" / "bca"]
    existing = [c for c in candidates if c.is_file()]
    if not existing:
        return None
    existing.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    return str(existing[0])


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
