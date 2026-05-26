#!/usr/bin/env python3
"""Re-run ``bca check`` with thresholds scaled to ``BCA_HEADROOM``.

This is the soft tier of the local self-scan gate. The hard gate
(``make self-scan``) fails when a function exceeds 100% of any
threshold in ``bca-thresholds.toml``. This helper takes every
threshold, multiplies it by ``BCA_HEADROOM`` (default ``0.95``),
and invokes ``bca check`` with those scaled limits plus the same
``.bca-baseline.toml`` filter the hard gate uses.

The baseline filter is what keeps the soft gate quiet: anything
already over 100% is recorded in the baseline and suppressed
here; only new or worsening functions encroaching into the
95-100% band trip this gate.

Exit code matches ``bca`` itself: 0 pass, 2 on threshold
encroachment, 1 on tool error.

Usage (driven by ``make self-scan-headroom``)::

    python3 utils/bca-self-scan-headroom.py \\
      cargo run --quiet --release -p big-code-analysis-cli -- \\
      --paths . --exclude-from .bcaignore --num-jobs N

Knobs:

    BCA_HEADROOM=0.90   widen the band (catch what's further out)
    BCA_HEADROOM=0.99   tighten to the last 1% before the hard limit
"""

from __future__ import annotations

import os
import subprocess
import sys
import tomllib
from pathlib import Path

THRESHOLDS_PATH = Path("bca-thresholds.toml")
BASELINE_PATH = Path(".bca-baseline.toml")


def main() -> int:
    if len(sys.argv) < 2:
        print(
            "usage: bca-self-scan-headroom.py <bca-invocation...>",
            file=sys.stderr,
        )
        return 64

    raw = os.environ.get("BCA_HEADROOM", "0.95")
    try:
        ratio = float(raw)
    except ValueError:
        print(f"BCA_HEADROOM must be a number; got {raw!r}", file=sys.stderr)
        return 64
    # Half-open interval (0, 1]. The upper bound is inclusive so
    # `BCA_HEADROOM=1.0` is a valid no-op-equivalent of the hard
    # gate (useful for parity / sanity checks).
    if not 0.0 < ratio <= 1.0:
        print(
            f"BCA_HEADROOM must be in (0, 1]; got {ratio}",
            file=sys.stderr,
        )
        return 64

    if not THRESHOLDS_PATH.is_file():
        print(f"missing {THRESHOLDS_PATH}", file=sys.stderr)
        return 1
    cfg = tomllib.loads(THRESHOLDS_PATH.read_text(encoding="utf-8"))
    thresholds = cfg.get("thresholds", {})
    if not thresholds:
        print(f"no [thresholds] table in {THRESHOLDS_PATH}", file=sys.stderr)
        return 1

    flags: list[str] = []
    for name, limit in thresholds.items():
        # Emit the scaled limit as a float regardless of the source
        # type. `bca check --threshold` parses the value as f64 for
        # every metric, and the offender check is `value > limit`
        # (strict). Passing the raw float keeps the BCA_HEADROOM knob
        # smooth: flooring to int would collapse adjacent ratios
        # (0.95 and 0.99 of `nargs=7` both floor to 6) and silently
        # widen the soft band well past the advertised ratio.
        warn = limit * ratio
        # `.6g` truncates float-multiplication artifacts (e.g.
        # `0.95 * 7 == 6.6499999999999995`) to a readable
        # `6.65` while preserving enough precision for all
        # thresholds in `bca-thresholds.toml` (max ~50000).
        flags.extend(["--threshold", f"{name}={warn:.6g}"])

    # `BCA_HEADROOM_WRITE_BASELINE=<path>` switches the helper from
    # "check against baseline" to "write baseline". Used by
    # `make self-scan-write-baseline-headroom` to capture today's
    # soft-tier offenders into the baseline, ratcheting future
    # encroachment from there.
    write_target = os.environ.get("BCA_HEADROOM_WRITE_BASELINE")
    if write_target:
        cmd = [
            *sys.argv[1:],
            "check",
            "--write-baseline",
            write_target,
            *flags,
        ]
        mode = f"write-baseline -> {write_target}"
    else:
        cmd = [
            *sys.argv[1:],
            "check",
            "--baseline",
            str(BASELINE_PATH),
            *flags,
        ]
        mode = f"baseline <- {BASELINE_PATH}"
    print(
        f"bca-self-scan-headroom: BCA_HEADROOM={ratio} "
        f"(scaled {len(thresholds)} thresholds, {mode})",
        file=sys.stderr,
    )
    return subprocess.call(cmd)


if __name__ == "__main__":
    sys.exit(main())
