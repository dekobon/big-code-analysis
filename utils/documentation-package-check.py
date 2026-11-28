#!/usr/bin/env python3
"""Compile AST test-support without checkout-only documentation fixtures.

Fallback when unpublished grammar dependencies prevent cargo package verification.
Copies tracked sources (plus current edits) and workspace manifests to a temporary
directory, retaining local grammar dependencies but omitting documentation fixtures.
Runs all-features check and rustdoc. --baseline REV substitutes only the old
documentation helper, proving that its external include fails across this boundary.
This does not verify registry resolution or Cargo's normalized package manifest.
"""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    files = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).split(b"\0")
    with tempfile.TemporaryDirectory(prefix="bca-documentation-package-") as directory:
        isolated = Path(directory)
        for raw in files:
            if not raw:
                continue
            relative = Path(os.fsdecode(raw))
            if relative.is_relative_to("tests/fixtures/documentation"):
                continue
            source = root / relative
            if not source.is_file():
                continue
            target = isolated / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
        assert not (isolated / "tests/fixtures/documentation").exists()
        if args.baseline:
            helper = "big-code-analysis-ast/src/test_support/documentation.rs"
            old = subprocess.check_output(["git", "show", f"{args.baseline}:{helper}"], cwd=root)
            (isolated / helper).write_bytes(old)
        env = {**os.environ, "CARGO_TARGET_DIR": str(root / "target/documentation-package-check"),
               "RUSTDOCFLAGS": "-D warnings"}
        for action in ("check", "doc"):
            command = ["cargo", action, "--offline", "--locked", "-p", "big-code-analysis-ast", "--all-features"]
            command += ["--lib"] if action == "check" else ["--no-deps"]
            print("+", " ".join(command), flush=True)
            result = subprocess.run(command, cwd=isolated, env=env)
            if result.returncode:
                return result.returncode
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
