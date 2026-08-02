#!/usr/bin/env bash
# Self-tests for utils/gate-status.sh.
#
# The assertion that matters most is `preserves a non-zero exit status`:
# a wrapper that reports `fail` but exits 0 would let a red branch look
# validated, which is strictly worse than the ambiguity #1172 set out to
# remove. The stage-extraction cases feed the script a canned GNU make
# transcript rather than a real gate run, so they stay fast and do not
# depend on which stage happens to be breakable today.
#
# These messages quote both verdict spellings. That is safe because the
# published contract is `^BCA_GATE:` and every message here is prefixed,
# so a failing run of this test cannot plant a second verdict in the
# gate's own log.

set -uo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
GATE_STATUS="$SCRIPT_DIR/gate-status.sh"

failures=0
out=$(mktemp)
err=$(mktemp)
trap 'rm -f -- "$out" "$err"' EXIT

fail() {
	printf 'FAIL: %s\n' "$1" >&2
	failures=$((failures + 1))
}

expect_eq() {
	# expect_eq <what> <expected> <actual>
	if [ "$2" != "$3" ]; then
		fail "$1: expected [$2], got [$3]"
	fi
}

run_gate() {
	# run_gate <gate-name> <command...>; leaves stdout in $out, stderr in
	# $err, and the wrapper's status in $rc.
	"$GATE_STATUS" "$@" >"$out" 2>"$err"
	rc=$?
}

# A canned transcript of what GNU make writes to stderr when stages fail
# under `-j`: the leaf target a stage delegates to, the stage itself, a
# second stage that was already running, a duplicate report, the
# aggregate this script is pointed at, and the outer epilogue.
#
# Held in a variable rather than inlined in the heredoc so the forwarded
# copy can be compared against it verbatim. A line *count* is not the
# `intact` the assertion below claims: a filter inserted into the
# forwarding path (`| sed …`, an awk annotator) rewrites content without
# changing the number of newlines, and that is exactly the edit the
# `tee`-not-awk choice in gate-status.sh exists to prevent.
read -r -d '' MAKE_TRANSCRIPT <<'TRANSCRIPT'
make[3]: *** [Makefile:200: fmt-check] Error 1
make[2]: *** [Makefile:1171: _pc-fmt] Error 2
make[2]: *** Waiting for unfinished jobs....
make[2]: *** [Makefile:1191: _pc-markdown-lint] Error 2
make[2]: *** [Makefile:1171: _pc-fmt] Error 2
make[1]: *** [Makefile:1093: _pc-all] Error 2
make: *** [Makefile:1092: pre-commit] Error 2
TRANSCRIPT
export MAKE_TRANSCRIPT

emit_make_transcript() {
	printf '%s\n' "$MAKE_TRANSCRIPT" >&2
	exit 2
}
export -f emit_make_transcript

# --- pass path -------------------------------------------------------
run_gate pre-commit bash -c 'echo stage output; echo stage warning >&2'
expect_eq 'pass exit status' 0 "$rc"
expect_eq 'pass verdict is the last stdout line' \
	'BCA_GATE: pass (gate=pre-commit)' "$(tail -n 1 "$out")"
expect_eq 'pass emits exactly one verdict' 1 "$(grep -c 'BCA_GATE:' "$out")"
expect_eq 'stdout is forwarded' 'stage output' "$(head -n 1 "$out")"
expect_eq 'stderr is forwarded, and stays on stderr' \
	'stage warning' "$(cat "$err")"
expect_eq 'the verdict does not leak onto stderr' \
	0 "$(grep -c 'BCA_GATE:' "$err")"

# --- failure path: the exit status must survive ----------------------
run_gate pre-commit bash -c 'exit 42'
expect_eq 'a non-zero exit status is preserved' 42 "$rc"
expect_eq 'fail verdict carries the real exit status' \
	'BCA_GATE: fail (gate=pre-commit, exit=42, stage=unknown)' \
	"$(tail -n 1 "$out")"

# --- failure path: stage extraction ----------------------------------
run_gate pre-commit bash -c emit_make_transcript
expect_eq 'transcript exit status is preserved' 2 "$rc"
expect_eq 'every failing stage is named once, in report order' \
	'BCA_GATE: fail (gate=pre-commit, exit=2, stage=_pc-fmt,_pc-markdown-lint)' \
	"$(tail -n 1 "$out")"
expect_eq 'fail emits exactly one verdict' 1 "$(grep -c 'BCA_GATE:' "$out")"
expect_eq 'the make transcript still reaches stderr intact' \
	"$MAKE_TRANSCRIPT" "$(cat "$err")"

# --- failure path: a broken `tee` is not a gate failure ---------------
# The case ${PIPESTATUS[0]} exists for, and the only one where it and a
# plain `$?` disagree. Pointing TMPDIR at a missing directory makes the
# `mktemp` fail, so `tee` gets an empty path and exits non-zero while the
# gate itself succeeds. Under `pipefail`, `$?` is then `tee`'s status and
# the wrapper reports a green run as failed. Without this case that
# substitution passes the whole suite.
TMPDIR=/nonexistent-bca-gate-status run_gate pre-commit bash -c 'echo stage output'
expect_eq 'a tee that cannot write is not a gate failure' 0 "$rc"
expect_eq 'the gate status, not the pipeline status, decides the verdict' \
	'BCA_GATE: pass (gate=pre-commit)' "$(tail -n 1 "$out")"

# --- usage -----------------------------------------------------------
run_gate onlyagatename
expect_eq 'a missing command is a usage error' 2 "$rc"
expect_eq 'a usage error emits no verdict' 0 "$(grep -c 'BCA_GATE:' "$out")"
# Which stream the usage line takes is the assertion, not merely that it
# exists: a verdict-free stdout is equally what dropping the `>&2` — or
# the whole printf — produces.
expect_eq 'the usage line goes to stderr' \
	"usage: $GATE_STATUS <gate-name> <command> [args...]" "$(cat "$err")"
expect_eq 'a usage error writes nothing to stdout' 0 "$(wc -c <"$out")"

if [ "$failures" -ne 0 ]; then
	printf '%d gate-status check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'gate-status: all checks passed\n'
