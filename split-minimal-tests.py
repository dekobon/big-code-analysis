#!/usr/bin/env python3

"""split-minimal-tests
This script splits HTML minimal-tests, produced by a software called
`json-minimal-tests`, into distinct directories depending on metric differences.

Usage:

./split-minimal-tests.py -i INPUT_DIR -o OUTPUT_DIR [-t MT_THRESHOLD]

NOTE: OUTPUT_DIR is the path to the output directory to be created.
This directory could contain either a series of directories, called as
the metrics that presents differences, or be empty if no metric differences
are found.
MT_THRESHOLD determines the maximum number of considered minimal tests
for a metric.
"""

import argparse
import pathlib
import re
import shutil
import subprocess
import typing as T

# Subset of metric names emitted by `big-code-analysis-cli --metrics` that
# this script splits HTML minimal tests by. The full set is discovered at
# runtime from `big-code-analysis-cli --list-metrics` so the two stay in
# sync. This list filters out object-oriented / class-only metrics (wmc,
# npm, npa, mi, abc) and the lone-letter `mi` summary, which the HTML
# minimal-test fixtures do not currently exercise.
RELEVANT_METRICS = frozenset(
    {
        "cognitive",
        "sloc",
        "ploc",
        "lloc",
        "cloc",
        "blank",
        "cyclomatic",
        "halstead",
        "nom",
        "nexits",
        "nargs",
    }
)


def _parse_list_metrics(stdout: str) -> T.List[str]:
    """Parse `big-code-analysis-cli --list-metrics` stdout into the subset
    of metric names this script knows how to split by.

    Blank lines and surrounding whitespace are tolerated; names absent
    from `RELEVANT_METRICS` are filtered out so future CLI additions
    (e.g. new class-only metrics) do not break the script.

    >>> sorted(_parse_list_metrics("cognitive\\nsloc\\nwmc\\n"))
    ['cognitive', 'sloc']
    >>> _parse_list_metrics("")
    []
    >>> sorted(_parse_list_metrics("  cognitive  \\n\\nploc\\n"))
    ['cognitive', 'ploc']
    >>> _parse_list_metrics("totally_unknown_metric\\n")
    []
    """
    available = {line.strip() for line in stdout.splitlines() if line.strip()}
    return [m for m in available if m in RELEVANT_METRICS]


def discover_metrics(cli: str) -> T.List[str]:
    """Run `big-code-analysis-cli --list-metrics` and return the metric
    names this script knows how to split by. Names absent from the CLI
    output are dropped (the binary is the source of truth); names absent
    from `RELEVANT_METRICS` are filtered out."""
    result = subprocess.run(
        [cli, "--list-metrics"],
        capture_output=True,
        text=True,
        check=True,
    )
    return _parse_list_metrics(result.stdout)


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="split-minimal-tests",
        description="This tool splits HTML minimal-tests, produced by "
        "a software called `json-minimal-tests`, into distinct directories "
        "depending on metric differences.",
        epilog="The source code of this program can be found on "
        "GitHub at https://github.com/dekobon/big-code-analysis",
    )

    # Arguments
    parser.add_argument(
        "--input",
        "-i",
        type=lambda value: pathlib.Path(value),
        required=True,
        help="Input directory containing HTML minimal tests.",
    )

    parser.add_argument(
        "--output",
        "-o",
        type=lambda value: pathlib.Path(value),
        required=True,
        help="Path to the output directory.",
    )

    # Optional arguments
    parser.add_argument(
        "--threshold",
        "-t",
        type=int,
        help="Maximum number of considered minimal tests for a metric.",
    )
    parser.add_argument(
        "--cli",
        default="big-code-analysis-cli",
        help="Path to the big-code-analysis-cli binary (default: %(default)s).",
    )

    # Parse arguments
    args = parser.parse_args()

    # Create output directory
    args.output.mkdir(parents=True, exist_ok=True)

    # Save files associated to each metric
    metrics_saver: T.Dict[str, T.List] = {
        metric_name: [] for metric_name in discover_metrics(args.cli)
    }

    # Iterate over the files contained in the input directory
    for path in args.input.glob("*.html"):
        # Open a file
        with open(path) as f:
            # Read a file
            file_str = f.read()

            # Remove all code inside <pre></pre> tags
            file_no_pre = re.sub(r"<pre>(.|\n)*?<\/pre>", "", file_str)

            # Iterate over metrics
            for metric_name, metric_files in metrics_saver.items():
                # Check if there is a metric difference in a file
                m = re.search(rf"(\.{metric_name})", file_no_pre)

                # If some errors occurred, skip to the next metric
                if m is None:
                    continue

                # Save path if there is a metric difference in a file
                if m.group(1):
                    metric_files.append(path)

    # Iterate over metrics to print them
    for metric_name, metric_files in metrics_saver.items():
        # Create path for metric directory
        metric_path = args.output / metric_name

        if metric_files:
            # Create metric directory
            metric_path.mkdir(parents=True, exist_ok=True)

            # Save the number of files specified in the threshold
            output_paths = (
                metric_files[: args.threshold] if args.threshold else metric_files
            )

            for path in output_paths:
                # Copy files in the directory
                shutil.copy(path, metric_path)


if __name__ == "__main__":
    main()
