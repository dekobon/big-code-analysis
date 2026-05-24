"""Produce SARIF 2.1.0 output ready for GitHub Code Scanning upload.

Issue #273 (phase 9/9) asks for a runnable example that emits a
SARIF document suitable for GitHub Code Scanning. This is the
"workflow-ready" complement to ``sarif_output.py``:

* ``sarif_output.py`` is the embedded snippet behind the book's
  ``python/sarif.md`` chapter — minimal, illustrative.
* This file is the artifact you would actually wire into a CI job:
  it walks an input set, applies threshold values that match the
  CLI's documented quality bars, writes the SARIF to a deterministic
  output path, and prints the ``gh code-scanning`` upload command
  the GitHub Actions runner can copy verbatim.

Recommended GitHub Actions wiring (place after a step that runs
this script):

.. code-block:: yaml

    - name: Generate SARIF
      run: |
        python big-code-analysis-py/examples/sarif_upload.py \\
          --output results.sarif \\
          src/
    - name: Upload SARIF
      uses: github/codeql-action/upload-sarif@v3
      with:
        sarif_file: results.sarif
        # Tag the run so Code Scanning groups findings under
        # "big-code-analysis" in the Security tab rather than
        # mixing with CodeQL output.
        category: big-code-analysis

Why thresholds matter for SARIF: ``bca.to_sarif`` emits findings
only for metrics that breach a configured threshold. Passing
``thresholds=None`` produces a valid-but-empty SARIF document
(`results: []`), which GitHub Code Scanning will happily ingest —
and which then silently masks every metric-quality regression
behind a "no new findings" banner. The default thresholds below
mirror the CLI's documented review bars; tune them per repo.

The output SARIF carries an ``automationDetails.id`` derived from
the script name so re-runs across PRs collapse into a single
Code Scanning "run" rather than accumulating duplicates. Override
with ``--category`` to match your Actions ``upload-sarif`` step.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Iterable, Mapping
from pathlib import Path

import big_code_analysis as bca

# Default threshold table. These mirror the bars surfaced in the
# CLI README's quality-report recipes; they are deliberate
# starting values, not project-of-record consensus. Override on
# the command line via ``--threshold name=value`` (repeatable) to
# track a stricter or looser policy per repo. The keys must match
# the CLI's `EXTRACTORS` table; an unknown name raises
# ``ValueError`` from inside :func:`bca.to_sarif` once the batch
# completes (the script does not pre-validate names against
# ``METRIC_NAMES``, so the failure surfaces after the directory
# walk — wrap in a dry-run check yourself if your runs are
# expensive).
#
# Values are float literals (``15.0`` not ``15``) so pyright sees
# the dict as ``dict[str, float]`` end-to-end; an ``int`` literal
# would type-narrow to ``int`` in the spread used by
# :func:`_merge_thresholds` below.
DEFAULT_THRESHOLDS: Mapping[str, float] = {
    "cyclomatic": 15.0,
    "cognitive": 15.0,
    "loc.lloc": 200.0,
    "halstead.difficulty": 30.0,
    "nargs": 7.0,
}


def _parse_threshold(spec: str) -> tuple[str, float]:
    """Parse a ``name=value`` CLI threshold spec.

    Raises :class:`argparse.ArgumentTypeError` (not a bare ``ValueError``)
    so argparse surfaces the failure as a usage error rather than a
    traceback — argparse's default ``type=`` error mapping unwraps
    that exception class specifically.
    """
    if "=" not in spec:
        msg = f"expected name=value, got {spec!r}"
        raise argparse.ArgumentTypeError(msg)
    name, _, raw = spec.partition("=")
    try:
        value = float(raw)
    except ValueError as exc:
        msg = f"could not parse threshold value {raw!r} as float"
        raise argparse.ArgumentTypeError(msg) from exc
    return name.strip(), value


def run(
    paths: Iterable[Path],
    output: Path,
    *,
    thresholds: Mapping[str, float] | None = None,
    category: str = "big-code-analysis",
) -> dict[str, object]:
    """Analyse ``paths``, render SARIF, write to ``output``.

    Returns a summary dict (``output``, ``results``, ``rules``,
    ``analyzed``, ``errors``) so callers / tests can assert on the
    document without re-reading the file. The summary deliberately
    surfaces both ``results`` (findings) and ``rules`` (distinct
    metric IDs that produced findings) since either may be zero
    independently — a regression that silently drops every finding
    would otherwise look identical to a healthy run.
    """
    materialised = [str(p) for p in paths]
    if not materialised:
        msg = "no input paths provided"
        raise SystemExit(msg)

    batch = bca.analyze_batch(materialised)
    analyzed = sum(1 for r in batch if not isinstance(r, bca.AnalysisError))
    errors = len(batch) - analyzed

    # `thresholds is None` (not truthiness) means "use the default
    # policy" — an explicitly-empty `{}` is a deliberate opt-out and
    # must be honoured. Falsy-check would silently swap `{}` for the
    # default table, masking the very "empty SARIF on no thresholds"
    # behaviour the module docstring at lines 38-43 calls out.
    effective: Mapping[str, float] = DEFAULT_THRESHOLDS if thresholds is None else thresholds
    sarif_text = bca.to_sarif(batch, thresholds=dict(effective))

    # Mutate the document in-memory BEFORE the single disk write so a
    # concurrent reader of `output` (a CI step that polls the file
    # while we are still serialising) never sees the un-tagged
    # intermediate document. Going through `json.loads`/`json.dumps`
    # would re-order keys vs. `bca.to_sarif`'s `serde_json` output —
    # acceptable for the example, but stay aware that the byte-for-
    # byte CLI parity contract is at the `to_sarif` layer, not at
    # this file's on-disk output.
    document = json.loads(sarif_text)
    run0 = document["runs"][0]
    rules = run0["tool"]["driver"].get("rules", [])
    results = run0.get("results", [])
    # Tag the run so multiple uploads (per workflow / per branch) do
    # not stack as separate Code Scanning analyses. GitHub keys
    # de-duplication on (tool.driver.name, category); we own only
    # the second half. The trailing slash is the documented
    # "category-only id" form (see
    # https://docs.github.com/en/code-security/code-scanning/integrating-with-code-scanning/sarif-support-for-code-scanning#runautomationdetails-object).
    # `setdefault` on the dict + an explicit check on `id` honours
    # any upstream-set id (today `bca.to_sarif` does not emit one,
    # but a future Rust-side bump might; an unconditional overwrite
    # would silently stomp it).
    automation = run0.setdefault("automationDetails", {})
    if not automation.get("id"):
        automation["id"] = f"{category}/"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(document, indent=2), encoding="utf-8")

    print(
        f"wrote {output} ({analyzed} analysed, {errors} errors, "
        f"{len(results)} findings across {len(rules)} rules)"
    )
    print(
        "to upload from a GitHub Actions step, use "
        f"`github/codeql-action/upload-sarif@v3` with sarif_file={output} "
        f"and category={category}"
    )
    return {
        "output": str(output),
        "results": len(results),
        "rules": len(rules),
        "analyzed": analyzed,
        "errors": errors,
    }


def _parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "paths",
        nargs="+",
        type=Path,
        help="Source files to analyse.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("results.sarif"),
        help="Where to write the SARIF document. Default: ./results.sarif",
    )
    parser.add_argument(
        "--threshold",
        action="append",
        type=_parse_threshold,
        default=None,
        metavar="NAME=VALUE",
        help=(
            "Override a single threshold (repeatable). Falls back to "
            "DEFAULT_THRESHOLDS for any unset metric. Accepts every "
            "name in the CLI's EXTRACTORS table — see bca.to_sarif's "
            "docstring for the full set."
        ),
    )
    parser.add_argument(
        "--category",
        default="big-code-analysis",
        help=(
            "Code Scanning category. Used as the SARIF "
            "automationDetails.id so re-runs collapse instead of "
            "stacking. Match this to the `category:` field of your "
            "Actions `upload-sarif` step."
        ),
    )
    return parser.parse_args(argv)


def _merge_thresholds(
    overrides: list[tuple[str, float]] | None,
) -> Mapping[str, float]:
    if not overrides:
        return DEFAULT_THRESHOLDS
    return {**DEFAULT_THRESHOLDS, **dict(overrides)}


if __name__ == "__main__":
    args = _parse_args(sys.argv[1:])
    run(
        args.paths,
        args.output,
        thresholds=_merge_thresholds(args.threshold),
        category=args.category,
    )
