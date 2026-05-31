#!/usr/bin/env python3

"""check-grammar-crate
This script checks whether breaking changes could be introduced in
big-code-analysis code after the update of a tree-sitter-grammar crate.
To do so, it compares the differences between the metrics, computed on a
chosen repository, before and after a tree-sitter-grammar update.


To compute metrics:

./check-grammar-crate.py compute-metrics -u REPO_URL -p LOCAL_DIR -l TREE_SITTER_GRAMMAR

NOTE: The compute-metrics subcommand MUST be run on a clean master branch!

To compute metrics on a continuous integration system:

./check-grammar-crate.py compute-ci-metrics -p LOCAL_DIR -l TREE_SITTER_GRAMMAR

To compare metrics and retrieve per-metric differences:

./check-grammar-crate.py compare-metrics -g TREE_SITTER_GRAMMAR [-t MIN_CHANGE]

This buckets the per-file metric deltas by metric using the native
`bca diff` (issue #487), printing a summary to stdout and saving a
machine-readable `diff.json` into the compare directory. It replaces the
former external `json-minimal-tests` + `split-minimal-tests.py` chain.
"""

import argparse
import pathlib
import subprocess
import sys
import typing as T

# The /tmp directory will be used as workdir
WORKDIR = pathlib.Path("/tmp")
# Suffix for the directory containing the old metrics
OLD_SUFFIX = "-old"
# Suffix for the directory containing the new metrics
NEW_SUFFIX = "-new"

# Extensions parsed by each tree-sitter-grammar
EXTENSIONS = {
    "tree-sitter-tsx": ["*.tsx"],
    "tree-sitter-typescript": ["*.ts", "*.jsw", "*.jsmw"],
    "tree-sitter-java": ["*.java"],
    "tree-sitter-kotlin": ["*.kt", "*.kts"],
    "tree-sitter-rust": ["*.rs"],
    "tree-sitter-python": ["*.py"],
    "tree-sitter-mozjs": ["*.js", "*.js2", "*.jsm", "*.mjs", "*.jsx"],
    "tree-sitter-mozcpp": [
        "*.cpp",
        "*.cx",
        "*.cxx",
        "*.cc",
        "*.hxx",
        "*.hpp",
        "*.c",
        "*.h",
        "*.hh",
        "*.inc",
        "*.mm",
        "*.m",
    ],
}

# Run a subprocess.
def run_subprocess(cmd: str, *args: T.Union[str, pathlib.Path]) -> None:
    subprocess.run([cmd, *args])


# Run big-code-analysis on the chosen repository to compute metrics.
#
# Emits one JSON document per source file into `output_dir`, using the
# current subcommand CLI (`bca metrics -O json --output <dir>`). The pre-
# subcommand flags this script used to pass (`--metrics
# --output-format=json --pr`) no longer exist; `--pr` was dropped
# entirely in the subcommand refactor.
def run_rca(
    repo_dir: pathlib.Path,
    output_dir: pathlib.Path,
    manifest_path: T.Optional[pathlib.Path],
    include_grammars: T.List[str],
) -> None:
    run_subprocess(
        "cargo",
        "run",
        "--manifest-path",
        manifest_path / "Cargo.toml" if manifest_path else "Cargo.toml",
        "--release",
        "--package",
        "big-code-analysis-cli",
        "--",
        "metrics",
        "-O",
        "json",
        "-I",
        *include_grammars,
        "-p",
        repo_dir,
        "-o",
        output_dir,
    )


# Compute continuous integration metrics before and after a
# tree-sitter-grammar update.
def compute_ci_metrics(args: argparse.Namespace) -> None:

    if args.grammar != "tree-sitter" and args.grammar not in EXTENSIONS.keys():
        print(args.grammar, "is not a valid tree-sitter grammar")
        sys.exit(1)

    # Use C/C++ files to test if there are any changes in metrics when
    # the tree-sitter crate is updated
    if args.grammar == "tree-sitter":
        grammar = "tree-sitter-mozcpp"
    else:
        grammar = args.grammar

    # Repository passed as input
    repo_dir = pathlib.Path(args.path)

    # Create big-code-analysis repository path
    rca_path = WORKDIR / "big-code-analysis"

    # Old metrics directory
    old_dir = WORKDIR / (args.grammar + OLD_SUFFIX)
    # New metrics directory
    new_dir = WORKDIR / (args.grammar + NEW_SUFFIX)

    # Create output directories
    old_dir.mkdir(parents=True, exist_ok=True)
    new_dir.mkdir(parents=True, exist_ok=True)

    # Git clone big-code-analysis master branch repository
    print("Cloning big-code-analysis master branch into /tmp")
    run_subprocess(
        "git",
        "clone",
        "--depth=1",
        "-j8",
        "https://github.com/dekobon/big-code-analysis",
        rca_path,
    )

    # Compute old metrics
    print("\nComputing metrics before the update and saving them in", old_dir)
    run_rca(repo_dir, old_dir, rca_path, EXTENSIONS[grammar])

    # Compute new metrics
    print("\nComputing metrics after the update and saving them in", new_dir)
    run_rca(repo_dir, new_dir, None, EXTENSIONS[grammar])


# Compute metrics before and after a tree-sitter-grammar update.
def compute_metrics(args: argparse.Namespace) -> None:

    if args.grammar not in EXTENSIONS.keys():
        print(args.grammar, "is not a valid tree-sitter grammar")
        sys.exit(1)

    # Repository local directory
    repo_dir = WORKDIR / args.path
    # Old metrics directory
    old_dir = WORKDIR / (args.grammar + OLD_SUFFIX)
    # New metrics directory
    new_dir = WORKDIR / (args.grammar + NEW_SUFFIX)

    # Create output directories
    old_dir.mkdir(parents=True, exist_ok=True)
    new_dir.mkdir(parents=True, exist_ok=True)

    # Skip if only new metrics are requested
    if not args.only_new:

        # Git clone the chosen repository
        print(f"Cloning {args.url} into {repo_dir}")
        run_subprocess("git", "clone", "--depth=1", args.url, repo_dir)

        # Compute old metrics
        print("\nComputing metrics before the update and saving them in", old_dir)
        run_rca(repo_dir, old_dir, None, EXTENSIONS[args.grammar])

        # Create a new branch
        print("\nCreate a new branch called", args.grammar)
        run_subprocess("git", "checkout", "-B", args.grammar)

    # Compute new metrics
    print("\nComputing metrics after the update and saving them in", new_dir)
    run_rca(repo_dir, new_dir, None, EXTENSIONS[args.grammar])


# Compare metrics and dump the per-metric differences, if any.
#
# Uses the native `bca diff` (issue #487) to bucket the per-file deltas
# by metric, replacing the external `json-minimal-tests` binary plus
# `split-minimal-tests.py` that this step used to chain. The TTY form is
# printed for the CI log; the machine-readable JSON form is saved to the
# compare directory as a build artifact.
def compare_metrics(args: argparse.Namespace) -> None:
    # Old metrics directory
    old_dir = WORKDIR / (args.grammar + OLD_SUFFIX)
    # New metrics directory
    new_dir = WORKDIR / (args.grammar + NEW_SUFFIX)

    # Compare metrics directory
    compare_dir = WORKDIR / (args.grammar + "-compare")

    # Create compare directory
    compare_dir.mkdir(parents=True, exist_ok=True)

    # Human-readable summary to the CI log.
    print("\nPer-metric diff between", old_dir, "and", new_dir)
    run_subprocess(
        "cargo",
        "run",
        "--release",
        "--package",
        "big-code-analysis-cli",
        "--",
        "diff",
        str(old_dir),
        str(new_dir),
        "--min-change",
        str(args.min_change),
    )

    # Machine-readable artifact for download / inspection.
    json_path = compare_dir / "diff.json"
    print("Save JSON diff in", json_path)
    with open(json_path, "w") as out:
        subprocess.run(
            [
                "cargo",
                "run",
                "--release",
                "--package",
                "big-code-analysis-cli",
                "--",
                "diff",
                str(old_dir),
                str(new_dir),
                "--min-change",
                str(args.min_change),
                "--format",
                "json",
            ],
            stdout=out,
            check=True,
        )


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="check-grammar-crate",
        description="This tool computes the metrics of a chosen repository "
        "before and after a tree-sitter grammar update.",
        epilog="The source code of this program can be found on "
        "GitHub at https://github.com/dekobon/big-code-analysis",
    )

    # Subcommands parsers
    commands = parser.add_subparsers(help="Sub-command help")

    # Compute metrics command
    compute_metrics_cmd = commands.add_parser(
        "compute-metrics",
        help="Computes the metrics of a chosen repository before and after "
        "a tree-sitter grammar update.",
    )

    # Optional arguments
    compute_metrics_cmd.add_argument(
        "--only-new",
        "-n",
        action="store_true",
        help="Only compute the metrics after a tree-sitter grammar update",
    )

    # Arguments
    compute_metrics_cmd.add_argument(
        "-u",
        "--url",
        type=str,
        required=True,
        help="URL of the repository used to compute the metrics",
    )

    compute_metrics_cmd.add_argument(
        "-p",
        "--path",
        type=str,
        required=True,
        help="Path where the repository will be saved locally",
    )

    compute_metrics_cmd.add_argument(
        "-g",
        "--grammar",
        type=str,
        required=True,
        help="tree-sitter grammar to be updated",
    )
    compute_metrics_cmd.set_defaults(func=compute_metrics)

    # Compute continuous integration metrics command
    compute_ci_metrics_cmd = commands.add_parser(
        "compute-ci-metrics",
        help="Computes the metrics of a chosen repository before and after "
        "a tree-sitter grammar update on a continuous integration system.",
    )

    # Arguments
    compute_ci_metrics_cmd.add_argument(
        "-p",
        "--path",
        type=str,
        required=True,
        help="Path where the big-code-analysis repository is saved on the "
        "continuous integration system",
    )
    compute_ci_metrics_cmd.add_argument(
        "-g",
        "--grammar",
        type=str,
        required=True,
        help="tree-sitter grammar to be updated",
    )

    compute_ci_metrics_cmd.set_defaults(func=compute_ci_metrics)

    # Compare metrics command
    compare_metrics_cmd = commands.add_parser(
        "compare-metrics",
        help="Compares the metrics before and after "
        "a tree-sitter grammar update in order to discover whether "
        "there are differences.",
    )

    # Arguments
    compare_metrics_cmd.add_argument(
        "-g",
        "--grammar",
        type=str,
        required=True,
        help="tree-sitter grammar used to compare the metrics",
    )
    compare_metrics_cmd.add_argument(
        "-t",
        "--min-change",
        type=float,
        default=0.0,
        help="Minimum absolute per-file metric change to report "
        "(passed to `bca diff --min-change`; default %(default)s reports "
        "any change).",
    )
    compare_metrics_cmd.set_defaults(func=compare_metrics)

    # Parse arguments
    args = parser.parse_args()

    # Call the command
    args.func(args)


if __name__ == "__main__":
    main()
