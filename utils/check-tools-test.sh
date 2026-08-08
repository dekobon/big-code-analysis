#!/usr/bin/env bash
# Self-tests for utils/check-tools.sh.
#
# The assertion that matters most is `cargo-about hint spells --features
# cli`. That string is the artifact #1226 is about, not a proxy for it:
# cargo-about's binary sits behind a non-default `cli` feature, so a
# hint that drops the flag sends the reader back into the exact trap the
# gate exists to close — `cargo install cargo-about` compiles the
# library, installs no binary, and exits 0.
#
# The probes run against a stub `cargo` on PATH rather than the real
# one, so the present/absent cases are both reachable on any machine and
# neither depends on what the developer happens to have installed.

set -uo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
CHECK_TOOLS="$SCRIPT_DIR/check-tools.sh"

failures=0
out=$(mktemp)
err=$(mktemp)
stubdir=$(mktemp -d)
trap 'rm -f -- "$out" "$err"; rm -rf -- "$stubdir"' EXIT

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

expect_contains() {
	# expect_contains <what> <needle> <file>
	if ! grep -qF -- "$2" "$3"; then
		fail "$1: [$2] not found in $(basename "$3")"
	fi
}

expect_absent() {
	# expect_absent <what> <needle> <file>
	if grep -qF -- "$2" "$3"; then
		fail "$1: [$2] unexpectedly present in $(basename "$3")"
	fi
}

# Writes a `cargo` stub that answers `--version` for each subcommand
# named in "$@" and reports every other subcommand the way cargo does.
# Bare `cargo --version` always succeeds so the script's own core-tool
# probe is unaffected.
write_cargo_stub() {
	local supported=" $* "
	cat >"$stubdir/cargo" <<STUB
#!/usr/bin/env bash
if [ "\$#" -eq 1 ] && [ "\$1" = "--version" ]; then
	echo "cargo 1.0.0-stub"
	exit 0
fi
case "$supported" in
	*" \$1 "*)
		echo "cargo-\$1 9.9.9-stub"
		exit 0
		;;
esac
echo "error: no such command: \\\`\$1\\\`" >&2
exit 101
STUB
	chmod +x "$stubdir/cargo"
}

run_release_only() {
	PATH="$stubdir:$PATH" "$CHECK_TOOLS" --release-only >"$out" 2>"$err"
	rc=$?
}

# --- both tools present ----------------------------------------------
write_cargo_stub deny about
run_release_only
expect_eq 'both tools present exits 0' 0 "$rc"
expect_contains 'cargo-deny is reported present' '✓ cargo-deny' "$out"
expect_contains 'cargo-about is reported present' '✓ cargo-about' "$out"
expect_absent 'no hints are printed when nothing is missing' \
	'Missing release tools:' "$out"

# --- cargo-about missing: the #1226 case ------------------------------
write_cargo_stub deny
run_release_only
expect_eq 'a missing cargo-about fails the probe' 1 "$rc"
expect_contains 'the missing tool is named' '✗ cargo-about' "$out"
expect_contains 'the present tool is not' '✓ cargo-deny' "$out"
# The whole point of the issue: an install line without `--features cli`
# reproduces the silent no-op it is supposed to prevent.
expect_contains 'cargo-about hint spells --features cli' \
	'cargo install --locked cargo-about --features cli' "$out"
expect_contains 'the hint says why the flag is needed' \
	'installs nothing and still exits 0' "$out"

# --- cargo-deny missing ----------------------------------------------
write_cargo_stub about
run_release_only
expect_eq 'a missing cargo-deny fails the probe' 1 "$rc"
expect_contains 'the missing tool is named' '✗ cargo-deny' "$out"
expect_contains 'cargo-deny hint is the plain install' \
	'cargo install --locked cargo-deny' "$out"
expect_absent 'no cargo-about hint when only cargo-deny is missing' \
	'cargo install --locked cargo-about' "$out"

# --- both missing -----------------------------------------------------
write_cargo_stub
run_release_only
expect_eq 'both missing fails the probe' 1 "$rc"
expect_contains 'both hints are printed' \
	'cargo install --locked cargo-about --features cli' "$out"
expect_contains 'both hints are printed' \
	'- cargo-deny: Install with:' "$out"

# --- usage ------------------------------------------------------------
PATH="$stubdir:$PATH" "$CHECK_TOOLS" --bogus >"$out" 2>"$err"
expect_eq 'an unknown flag is a usage error' 2 "$?"
expect_eq 'the usage line goes to stderr' \
	'usage: check-tools.sh [--release-only]' "$(cat "$err")"
expect_eq 'a usage error writes nothing to stdout' 0 "$(wc -c <"$out")"

PATH="$stubdir:$PATH" "$CHECK_TOOLS" --release-only extra >"$out" 2>"$err"
expect_eq 'a surplus argument is a usage error' 2 "$?"

if [ "$failures" -ne 0 ]; then
	printf '%d check-tools check(s) failed\n' "$failures" >&2
	exit 1
fi
printf 'check-tools: all checks passed\n'
