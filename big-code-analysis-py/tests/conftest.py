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

    A relative ``$CARGO_TARGET_DIR`` (e.g. ``./cache``) must be anchored
    to the directory cargo actually runs in, which the build fixture
    pins to ``REPO_ROOT`` via ``cwd=REPO_ROOT``. ``Path(env_dir).resolve()``
    alone would anchor a relative value to ``os.getcwd()`` — pytest's CWD,
    not cargo's — so the locator would look in the wrong place when pytest
    runs from outside the workspace root (#922). Joining ``REPO_ROOT / env_dir``
    first matches cargo's base; an absolute ``env_dir`` is unaffected by the
    join (``Path("/a") / "/b"`` yields ``/b``), so this is correct for both
    relative and absolute values.
    """
    env_dir = os.environ.get("CARGO_TARGET_DIR")
    if env_dir:
        return (REPO_ROOT / env_dir).resolve()
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
