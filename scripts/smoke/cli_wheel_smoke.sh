#!/usr/bin/env bash
#
# CLI-wheel smoke for the `bca` command-line tool.
#
# Extracted from the inline `run:` block in
# `.github/workflows/python-cli-wheels.yml` so the load-bearing assertions
# are visible to reviewers, lintable (shellcheck), and runnable per-PR /
# locally rather than only on a `v*` tag push (#995). The
# `cyclomatic.sum == "3"` integer-metric assertion silently rotted from the
# pre-2.0 `"3.0"` (#530) precisely because it was buried in workflow YAML
# and only ran when the `v2.0.0` tag forced it.
#
# The binary under test is resolved from, in order:
#   1. $BCA           — an explicit path (used by the dev-build dry-run).
#   2. `bca` on PATH  — the console script installed from the wheel (the
#                       workflow's case).
#
# Optional env:
#   EXPECTED_TAG  — e.g. `v2.1.0`. When set, assert `bca --version` reports
#                   the tag version (minus the leading `v`). Empty on PR /
#                   dev runs, where there is no tag to compare against.
#
# Usage:
#   bca on PATH:        scripts/smoke/cli_wheel_smoke.sh
#   explicit binary:    BCA=target/release/bca scripts/smoke/cli_wheel_smoke.sh
set -euo pipefail

bca="${BCA:-bca}"

ver_out="$("$bca" --version)"
echo "$ver_out"

# On a tag build, prove the binary reports the lockstep release version
# (maturin reads `version.workspace = true`, so the wheel version must equal
# the tag minus its leading `v`).
if [[ -n "${EXPECTED_TAG:-}" ]]; then
	want="${EXPECTED_TAG#v}"
	case "$ver_out" in
		*"$want"*) : ;;
		*)
			echo "::error::bca --version '$ver_out' does not contain tag version $want"
			exit 1
			;;
	esac
fi

"$bca" list-metrics names >/dev/null

# Work in a scratch dir so the fixtures never collide with the caller's cwd.
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

# Parse two unrelated languages to prove the all-languages grammar set is
# compiled in, not just the host language.
printf 'def add(a, b):\n    if a > b:\n        return a\n    return b\n' >"$workdir/smoke.py"
printf 'fn main() { if true { println!("x"); } }\n' >"$workdir/smoke.rs"

# `python` is the wheel workflow's interpreter; fall back to `python3` for
# local shells where only the versioned name is on PATH.
py="python"
command -v "$py" >/dev/null 2>&1 || py="python3"

# `--no-config` keeps the smoke hermetic: the wheel job's sparse checkout
# (and a local `make smoke-cli` from the repo root) would otherwise let bca
# auto-discover the repo's own bca.toml and apply its self-scan config. The
# smoke asserts the binary's *default* behaviour, independent of repo state.
extract_cyclomatic_sum() {
	"$bca" metrics --no-config --paths "$1" -O json \
		| "$py" -c "import sys,json; print(json.load(sys.stdin)['metrics']['cyclomatic']['sum'])"
}

py_cc="$(extract_cyclomatic_sum "$workdir/smoke.py")"
rs_cc="$(extract_cyclomatic_sum "$workdir/smoke.rs")"

# 2.0 serializes integer-valued metrics as integers (#530), so `json.load`
# yields a Python int and `print` emits `3`, not the pre-2.0 `3.0`. A
# regression that flipped the wire field back to f64 would print `3.0` here
# (and red `cli_metrics_json_serializes_integer_metrics_as_integers`).
test "$py_cc" = "3" || {
	echo "::error::python cyclomatic.sum=$py_cc, expected 3"
	exit 1
}
test "$rs_cc" = "3" || {
	echo "::error::rust cyclomatic.sum=$rs_cc, expected 3"
	exit 1
}
echo "smoke OK"
