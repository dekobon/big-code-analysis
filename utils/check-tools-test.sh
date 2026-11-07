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
# The probes run against a stub `cargo` and a stub `cargo-deny`, on a
# PATH holding nothing else but the few commands the scripts call, so
# the present/absent cases are both reachable on any machine and neither
# depends on what the developer happens to have installed.

set -uo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
CHECK_TOOLS="$SCRIPT_DIR/check-tools.sh"

failures=0
out=$(mktemp)
err=$(mktemp)
stubdir=$(mktemp -d)
# The release probes run with PATH set to the stubs plus this directory
# alone, which holds only the commands the two scripts call, so no
# cargo-deny the host has can answer for a stub — wherever it lives.
toolsdir=$(mktemp -d)
for tool in bash dirname awk; do
	ln -s "$(command -v "$tool")" "$toolsdir/$tool"
done
# An empty CARGO_HOME, so the "not on PATH" case cannot find the host's
# `~/.cargo/bin/cargo-deny` off PATH and report that instead.
cargohome=$(mktemp -d)
trap 'rm -f -- "$out" "$err"; rm -rf -- "$stubdir" "$toolsdir" "$cargohome"' EXIT

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

# Writes a `cargo-deny` stub that reports version "$1"; given `broken`,
# one that runs but fails (`exit 127`); given `missing`, none at all. The
# probe runs the binary through utils/check-deny-manifests.sh, not
# through cargo, so the `cargo` stub cannot answer for it.
write_deny_stub() {
	rm -f -- "$stubdir/cargo-deny"
	case "$1" in
		missing) return ;;
		broken)
			printf '#!/usr/bin/env bash\necho "cargo-deny: exec failed" >&2\nexit 127\n' \
				>"$stubdir/cargo-deny"
			;;
		*) printf '#!/usr/bin/env bash\necho "cargo-deny %s"\n' "$1" >"$stubdir/cargo-deny" ;;
	esac
	chmod +x "$stubdir/cargo-deny"
}

run_release_only() {
	PATH="$stubdir:$toolsdir" CARGO_HOME="$cargohome" \
		"$CHECK_TOOLS" --release-only >"$out" 2>"$err"
	rc=$?
}

# expect_count <what> <expected> <needle> <file>
expect_count() {
	expect_eq "$1" "$2" "$(grep -cF -- "$3" "$4")"
}

# --- both tools present ----------------------------------------------
# The `cargo` stub rejects `deny`: the probe must not need cargo's
# dispatch, because utils/check-deny-manifests.sh runs the binary.
write_cargo_stub about
write_deny_stub 0.20.2
run_release_only
expect_eq 'both tools present exits 0' 0 "$rc"
expect_contains 'cargo-deny is reported present' \
	'✓ cargo-deny (version: 0.20.2)' "$out"
expect_contains 'cargo-about is reported present' '✓ cargo-about' "$out"
expect_absent 'no hints are printed when nothing is missing' \
	'Missing release tools:' "$out"

# --- cargo-about missing: the #1226 case ------------------------------
write_cargo_stub
write_deny_stub 0.20.2
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
# `cargo deny` would answer here; the binary the audit runs is absent.
write_cargo_stub deny about
write_deny_stub missing
run_release_only
expect_eq 'a missing cargo-deny fails the probe' 1 "$rc"
expect_contains 'the missing tool is named' '✗ cargo-deny' "$out"
# The version is left to utils/check-deny-manifests-test.py, which holds
# every copy of the pin to CI's; this asserts only that one is named.
expect_contains 'the deny script says it is not on PATH, and the remedy' \
	'- cargo-deny not found on PATH; install the version CI pins: cargo install --locked cargo-deny@' \
	"$out"
expect_count 'the install command is printed once' 1 \
	'cargo install --locked cargo-deny@' "$out"
expect_absent 'no cargo-about hint when only cargo-deny is missing' \
	'cargo install --locked cargo-about' "$out"

# --- cargo-deny on PATH but not running -------------------------------
write_cargo_stub about
write_deny_stub broken
run_release_only
expect_eq 'a cargo-deny that does not run fails the probe' 1 "$rc"
expect_contains 'the broken binary is named' \
	"- cargo-deny at $stubdir/cargo-deny did not report a version" "$out"

# --- cargo-deny older than the deny script accepts (#1826) -------------
# 0.20 moved `--config`, so release-check's audit cannot run on 0.19;
# the probe must refuse it here rather than let the gate fail mid-run.
write_cargo_stub about
write_deny_stub 0.19.4
run_release_only
expect_eq 'a cargo-deny below the floor fails the probe' 1 "$rc"
expect_contains 'the old tool is named' '✗ cargo-deny' "$out"
expect_absent 'the old tool is not reported usable' '✓ cargo-deny' "$out"
expect_contains 'the reason is the version floor' \
	'- cargo-deny 0.19.4 is older than 0.20' "$out"
# An older copy earlier on PATH than a fresh install is the case where
# the install command alone would change nothing.
expect_contains 'the old binary is named by path' \
	"it is $stubdir/cargo-deny, the first cargo-deny on PATH" "$out"
expect_contains 'an old cargo-deny gets the pinned install hint' \
	"Install CI's pin ahead of it: cargo install --locked cargo-deny@" "$out"
expect_count 'the install command is printed once' 1 \
	'cargo install --locked cargo-deny@' "$out"

# --- both missing -----------------------------------------------------
write_cargo_stub
write_deny_stub missing
run_release_only
expect_eq 'both missing fails the probe' 1 "$rc"
expect_contains 'both hints are printed' \
	'cargo install --locked cargo-about --features cli' "$out"
expect_contains 'both hints are printed' \
	'- cargo-deny not found on PATH' "$out"

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
