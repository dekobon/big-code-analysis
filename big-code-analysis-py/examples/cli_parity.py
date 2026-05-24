"""CLI parity smoke test for ``big_code_analysis``.

Issue #273 (phase 9/9) asks the bindings to ship a runnable script
that demonstrates ``bca.analyze`` is byte-for-byte equal to
``bca metrics --output-format json`` at the ``FuncSpace`` serde
boundary. The smoke-test contract in ``tests/test_smoke.py``
already exercises this parity across multiple fixtures; this file
is the end-user copy-paste analogue — a single ``run()`` entry
point a downstream consumer can vendor verbatim to gate their own
"replace CLI with bindings" migration.

The strict equality is checked at two layers:

1. ``dict ==``. Structural; order-insensitive; treats ``1 == 1.0``.
2. ``json.dumps(..., sort_keys=False)``. Preserves CPython dict
   insertion order, so a regression that re-orders the
   ``FuncSpace`` fields (a re-introduction of the historical
   ``serde_json::to_value`` → ``BTreeMap`` alphabetisation path)
   surfaces as a string diff even when ``dict ==`` still holds.

The "documented set of fields" in the issue text is empty today:
the bindings and the CLI go through the same
``serde_json::to_string`` writer on the Rust side, then CPython's
``json.loads`` parses the CLI output back into a dict with
insertion order preserved (PEP 468 / CPython 3.7+). Any future
divergence is a bug, not a wart to be documented around.

The script accepts an optional ``--bca-binary`` flag (or
``$BCA_BINARY`` env var) so it can be run against a prebuilt
binary in CI without re-shelling ``cargo build`` from inside the
example. The fallback resolves the workspace ``target/{debug,release}/bca``
the same way ``tests/test_smoke.py`` does.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

import big_code_analysis as bca

# Match `tests/test_smoke.py::_locate_workspace_binary`. The script
# is two directories below the repo root (`examples/` under the
# bindings crate), so `.parents[2]` lands on the workspace root.
REPO_ROOT = Path(__file__).resolve().parents[2]


def _locate_bca_binary() -> str | None:
    """Locate a freshly-built ``bca`` under the workspace target dir."""
    env_dir = os.environ.get("CARGO_TARGET_DIR")
    target = Path(env_dir) if env_dir else REPO_ROOT / "target"
    candidates = [target / "debug" / "bca", target / "release" / "bca"]
    # Prefer the binary with the newer mtime so a stale build from a
    # previous branch checkout does not silently shadow a freshly-
    # built one. The fixture in `tests/conftest.py` rebuilds via
    # cargo to side-step this, but a script invoked directly does
    # not; pick newest-by-mtime as a defensive fallback.
    existing = [c for c in candidates if c.is_file()]
    if not existing:
        return None
    existing.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    return str(existing[0])


def _resolve_bca_binary(explicit: str | None, *, build_if_missing: bool = True) -> str:
    """Pick the ``bca`` binary, honouring explicit flag → env → target/.

    When ``build_if_missing`` is set (the default) and no candidate
    is found, invokes ``cargo build -p big-code-analysis-cli`` so
    the script's parity claim is always checked against the CURRENT
    source tree, not a stale build artifact. The conftest fixture
    does the same. Pass ``build_if_missing=False`` to fail fast if
    the binary is genuinely missing (e.g., from a release wheel).
    """
    if explicit:
        if not Path(explicit).is_file():
            msg = f"--bca-binary={explicit!r} is not a regular file"
            raise SystemExit(msg)
        return explicit
    env_path = os.environ.get("BCA_BINARY")
    if env_path:
        if not Path(env_path).is_file():
            msg = f"$BCA_BINARY={env_path!r} is not a regular file"
            raise SystemExit(msg)
        return env_path
    located = _locate_bca_binary()
    if located is not None:
        return located
    if not build_if_missing:
        msg = (
            "could not locate a `bca` binary under "
            f"{REPO_ROOT}/target/{{debug,release}}/. Build with "
            "`cargo build -p big-code-analysis-cli` or pass "
            "--bca-binary."
        )
        raise SystemExit(msg)
    cargo = shutil.which("cargo")
    if cargo is None:
        msg = "no `bca` binary found and `cargo` is not on PATH. Install Rust or pass --bca-binary."
        raise SystemExit(msg)
    print(f"building bca CLI via `{cargo} build -p big-code-analysis-cli`...")
    proc = subprocess.run(
        [cargo, "build", "-p", "big-code-analysis-cli", "--quiet"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        msg = (
            "`cargo build -p big-code-analysis-cli` failed; parity "
            f"check cannot run.\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
        raise SystemExit(msg)
    rebuilt = _locate_bca_binary()
    if rebuilt is None:
        msg = (
            "cargo build succeeded but no bca binary was found under "
            f"{REPO_ROOT}/target/{{debug,release}}/. If $CARGO_TARGET_DIR "
            "is set, ensure it matches between the build and this script."
        )
        raise SystemExit(msg)
    return rebuilt


def _cli_stdout(bca_path: str, path: Path) -> str:
    """Run ``bca metrics --output-format json --paths <path>`` and return stdout.

    Returns the raw stdout (no parsing) so the caller can compare
    the CLI's literal serde_json output byte-for-byte against
    ``json.dumps(bca.analyze(path))`` — the strongest parity check.
    Parsing through ``json.loads`` then re-serialising would
    normalise numeric types and mask int-vs-float drift between
    the two sides.
    """
    result = subprocess.run(
        [bca_path, "metrics", "--output-format", "json", "--paths", str(path)],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout


def run(path: Path, *, bca_binary: str | None = None) -> dict[str, Any]:
    """Compare ``bca.analyze(path)`` to ``bca metrics --output-format json``.

    Performs THREE complementary checks:

    1. ``dict ==`` (structural; order-insensitive; treats ``1 == 1.0``).
    2. Top-level key order (catches a re-introduction of the historical
       ``serde_json::to_value`` → ``BTreeMap`` alphabetisation path).
    3. **Byte-for-byte** comparison of the CLI's stdout against
       ``json.dumps(py_result)``. The CLI's output is *not* parsed
       through ``json.loads`` for this check — that would normalise
       ``1`` and ``1.0`` to the same Python ``int``, masking a real
       numeric-type drift. Trailing whitespace from the CLI is
       stripped before comparison (CLI emits one record per line
       plus a trailing newline; the bindings serialise without one).

    Returns a small report dict so the test harness can assert on
    each check independently.
    """
    binary = _resolve_bca_binary(bca_binary)

    py_result = bca.analyze(path)
    if py_result is None:
        msg = (
            f"{path} was skipped (looks generated); the parity check "
            "needs a non-generated input — see "
            "tests/fixtures/generated.rs for the skipped case."
        )
        raise SystemExit(msg)
    cli_stdout = _cli_stdout(binary, path)
    cli_result = json.loads(cli_stdout)

    if py_result != cli_result:
        msg = (
            "structural mismatch between bindings and CLI:\n"
            f"  python: {py_result!r}\n"
            f"  cli:    {cli_result!r}"
        )
        raise SystemExit(msg)

    # Insertion-order preservation. dict == ignores key order, so this
    # second layer catches the historical to_value/BTreeMap regression.
    py_keys = list(py_result.keys())
    cli_keys = list(cli_result.keys())
    key_order_matches = py_keys == cli_keys
    if not key_order_matches:
        msg = f"top-level key order diverged: py={py_keys} cli={cli_keys}"
        raise SystemExit(msg)

    # Strongest check: compare the CLI's literal stdout bytes against
    # the bindings' serialised dict. The CLI uses `serde_json::to_string`
    # (compact, no extra whitespace, no key sort) — and so does
    # `json.dumps(py_result)` by default for the field-name keys (CPython
    # preserves insertion order; serde matches struct definition order).
    # `cli_stdout.strip()` drops the CLI's trailing newline (per-record
    # writer convention); the bindings emit no trailing newline.
    # serde_json::to_string emits NO whitespace between tokens; match
    # that with `separators=(',', ':')`. Pass `ensure_ascii=False` so
    # Python emits non-ASCII codepoints as raw UTF-8 (matching serde),
    # not as `\uXXXX` escapes — without this, a fixture whose path or
    # an identifier contains non-ASCII characters would trip the byte
    # check with a misleading "numeric-type or nested-order regression"
    # message even though no real divergence exists.
    py_json = json.dumps(py_result, separators=(",", ":"), ensure_ascii=False)
    json_bytes_match = py_json == cli_stdout.strip()
    if not json_bytes_match:
        msg = (
            "JSON byte sequences diverged despite structural equality "
            "— this is the strongest parity signal and indicates a "
            "real numeric-type or nested-order regression. The CLI's "
            "literal stdout was:\n"
            f"  cli:    {cli_stdout.strip()!r}\n"
            f"  python: {py_json!r}\n"
            "See tests/test_smoke.py for the per-fixture parity "
            "contract."
        )
        raise SystemExit(msg)

    print(f"ok: {path} matches `bca metrics` byte-for-byte")
    return {
        "ok": True,
        "path": str(path),
        "key_order_matches": key_order_matches,
        "json_bytes_match": json_bytes_match,
    }


def _parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "path",
        type=Path,
        help="Source file to analyse with both the bindings and the CLI.",
    )
    parser.add_argument(
        "--bca-binary",
        default=None,
        help=(
            "Path to a prebuilt `bca` binary. Defaults to "
            "$BCA_BINARY, then the workspace target/{debug,release}/."
        ),
    )
    return parser.parse_args(argv)


if __name__ == "__main__":
    args = _parse_args(sys.argv[1:])
    run(args.path, bca_binary=args.bca_binary)
