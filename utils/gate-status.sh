#!/usr/bin/env bash
# Run a validation gate and end its output with one machine-readable
# verdict line.
#
#   utils/gate-status.sh <gate-name> <command> [args...]
#
# The command's stdout and stderr are forwarded unchanged, on their own
# descriptors, and the command's exit status is this script's exit
# status. After it finishes, exactly one line is printed to stdout:
#
#   BCA_GATE: pass (gate=pre-commit)
#   BCA_GATE: fail (gate=pre-commit, exit=2, stage=_pc-fmt)
#
# Why this exists (#1172): `make pre-commit` runs a parallel DAG, so
# GNU make reports the first failing stage as
# `make[1]: *** [Makefile:1234: _pc-fmt] Error 2` and then keeps going
# until the stages already running finish. That line is therefore not a
# terminal verdict — it is routinely followed by several more stages'
# successful output — and reading an outcome out of the log by eye or
# by grepping `Error N` has repeatedly produced the wrong answer.
#
# The contract a consumer greps is `^BCA_GATE:` — the token at the start
# of a line. Only this script emits that, and only once per run. The
# anchor matters: gate-status-test.sh quotes both spellings in its own
# failure messages, and those are prefixed, so they cannot match.
#
# Three states, not two. No `BCA_GATE:` line at all means the run never
# finished: it crashed, was killed, or was interrupted. That must not be
# read as either pass or fail.
#
# On the failure path GNU make appends its own
# `make: *** [Makefile:NNNN: pre-commit] Error 2` epilogue after this
# line, because this script exits non-zero and make says so. That is
# unavoidable without swallowing the exit status, which would be a far
# worse defect than the one being fixed. The `BCA_GATE:` line is still
# the last thing the gate itself writes, and it is still unique.
#
# Unlike the other utils/ scripts this one needs no repository root: it
# runs whatever command it is handed, from wherever it is invoked.

set -uo pipefail

if [ "$#" -lt 2 ]; then
	printf 'usage: %s <gate-name> <command> [args...]\n' "$0" >&2
	exit 2
fi

gate=$1
shift

captured_stderr=$(mktemp "${TMPDIR:-/tmp}/bca-gate-status.XXXXXX")
trap 'rm -f -- "$captured_stderr"' EXIT

# Copy stderr aside — make names a failing target only there — while
# keeping the two streams separate, since a caller may redirect them
# independently. `tee` rather than a filter written in awk: awk block-
# buffers its output, which would reorder stderr against stdout in a
# `> log 2>&1` capture.
#
# ${PIPESTATUS[0]} is the gate's status. `$?` is not a substitute even
# with pipefail set: pipefail yields the *last* non-zero status, so a
# `tee` that failed to write the temp file would be reported as a gate
# failure on an otherwise green run.
{ "$@" 2>&1 1>&3 | tee -- "$captured_stderr" >&2; } 3>&1
status=${PIPESTATUS[0]}

if [ "$status" -eq 0 ]; then
	printf 'BCA_GATE: pass (gate=%s)\n' "$gate"
	exit 0
fi

# Pull the DAG stages out of `make[N]: *** [Makefile:123: _pc-fmt] Error 2`.
# Every stage make named is listed, in the order it named them: under
# `-j` the first failure only stops *scheduling*, so stages already
# running can fail too, and naming just one would hide the rest.
#
# Only `_`-prefixed targets are DAG stages. That drops both the leaf
# targets a stage delegates to (`fmt-check`, `test`) and the outer
# `make: *** [... pre-commit]` epilogue, neither of which tells a reader
# anything the stage name does not. The `_pc-all` / `_ci-all` aggregate
# this script is pointed at is dropped for the same reason.
stages=$(
	sed -n 's/^make\[[0-9]*\]: \*\*\* \[[^]]*: \(_[A-Za-z][A-Za-z0-9_-]*\)\] Error .*/\1/p' \
		"$captured_stderr" \
		| grep -Ev '^_(pc|ci)-all$' \
		| awk '!seen[$0]++' \
		| paste -sd, -
)

printf 'BCA_GATE: fail (gate=%s, exit=%d, stage=%s)\n' \
	"$gate" "$status" "${stages:-unknown}"
exit "$status"
