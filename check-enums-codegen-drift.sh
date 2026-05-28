#!/bin/bash
# check-enums-codegen-drift
#
# Runs the `enums/` codegen into a tempdir, formats the output,
# and diffs against the checked-in `src/c_langs_macros/*.rs` and
# `src/languages/language_*.rs` files. Any divergence fails.
#
# Closes the failure mode from #405: running any grammar regen
# silently regenerated `c_macros.rs` / `c_specials.rs` to a
# pre-optimization form (linear `.contains()` lookup + missing
# sorted-invariant tests). With this gate in place, the codegen
# template and the checked-in files must stay in sync, and the
# next contributor to run `recreate-grammars.sh` either picks up
# the hand-improved form or trips the gate.

set -euo pipefail

# `shopt -s nullglob` so an empty codegen output (silent failure)
# surfaces as "codegen produced no files" rather than iterating
# the loop body once with `f=$source_dir/*.rs` as a literal
# string and reporting drift on a non-existent path.
shopt -s nullglob

# `ROOT` derivation. `git rev-parse --show-toplevel` fails outside
# a git work tree; fall back to the script's own directory so
# release-tarball / packaging-script invocations still work
# rather than dying with an opaque `fatal: not a git repository`.
if ! ROOT=$(git rev-parse --show-toplevel 2>/dev/null); then
	ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fi

# Use a script-local variable name (NOT `TMPDIR`) so the caller's
# `$TMPDIR` env var is preserved for cargo / rustfmt / etc. The
# EXIT/INT/TERM trap then cleans only OUR working directory.
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bca-enums-drift.XXXXXX")
trap 'rm -rf "$WORK_DIR"' EXIT INT TERM HUP

mkdir -p "$WORK_DIR/languages" "$WORK_DIR/c_langs_macros"

MANIFEST="$ROOT/enums/Cargo.toml"
# Build the binary once so the two `cargo run` invocations below
# don't both pay the lock/check cost. After this, both invocations
# hit a warm artifact cache.
if ! cargo build --manifest-path "$MANIFEST" --quiet; then
	echo "error: enums crate failed to build" >&2
	exit 2
fi

# Each codegen mode pairs an `enums -l<mode>` flag with the
# target subdir under `src/`. Looping keeps the cargo invocations
# and the diff loop below in sync — adding a third codegen
# language is now a single-line append.
declare -a MODES=(
	"rust:languages"
	"c_macros:c_langs_macros"
)

for mode_pair in "${MODES[@]}"; do
	mode="${mode_pair%%:*}"
	subdir="${mode_pair##*:}"
	if ! cargo run --manifest-path "$MANIFEST" --quiet -- \
		-l"$mode" -o "$WORK_DIR/$subdir"; then
		echo "error: enums codegen (-l$mode) failed" >&2
		exit 2
	fi
done

# Format generated files so the diff against the rustfmt'd
# checked-in files isn't tripped by whitespace. `fd` per
# CLAUDE.md tool-choice rules (never `find`). The empty-tree
# case is a no-op: `fd -X` skips invocation when no matches.
if ! command -v fd >/dev/null 2>&1; then
	# Some Debian/Ubuntu images ship `fdfind` rather than `fd`.
	if command -v fdfind >/dev/null 2>&1; then
		fd_bin=fdfind
	else
		echo "error: fd (or fdfind) is required to format codegen output" >&2
		exit 2
	fi
else
	fd_bin=fd
fi
"$fd_bin" -e rs . "$WORK_DIR" -X rustfmt --edition 2024

# Detect drift in both directions:
#   1. Every codegen output must match a checked-in file.
#   2. Every checked-in file in the target subdir (excluding
#      hand-maintained mod.rs / mod files) must have a counterpart
#      in the codegen output. Otherwise a stale generated file
#      that the codegen no longer emits (e.g., a removed
#      `Lang::Foo` leaving behind `language_foo.rs`) lingers
#      undetected.
fail=0
diff_dir() {
	local source_dir="$1"
	local target_subdir="$2"
	local checked_in_dir="$ROOT/$target_subdir"

	# Codegen output → checked-in.
	for f in "$source_dir"/*.rs; do
		local base
		base=$(basename "$f")
		local checked_in="$checked_in_dir/$base"
		if [ ! -f "$checked_in" ]; then
			echo "drift: $target_subdir/$base produced by codegen but missing from repo" >&2
			fail=1
			continue
		fi
		if ! diff -q "$checked_in" "$f" >/dev/null 2>&1; then
			echo "drift: $target_subdir/$base" >&2
			# Wrap in `|| true` so SIGPIPE from `head` closing
			# the pipe early (diff > 40 lines) doesn't trip
			# `set -e` + `pipefail` and abort before subsequent
			# files are checked or the remediation message is
			# printed.
			{ diff -u "$checked_in" "$f" 2>/dev/null || true; } \
				| head -40 >&2 || true
			fail=1
		fi
	done

	# Reverse: checked-in → codegen output. Skip `mod.rs`
	# (hand-maintained module index, not generated).
	for f in "$checked_in_dir"/*.rs; do
		local base
		base=$(basename "$f")
		if [ "$base" = "mod.rs" ]; then
			continue
		fi
		if [ ! -f "$source_dir/$base" ]; then
			echo "drift: $target_subdir/$base in repo but not produced by codegen (stale)" >&2
			fail=1
		fi
	done
}

diff_dir "$WORK_DIR/languages" "src/languages"
diff_dir "$WORK_DIR/c_langs_macros" "src/c_langs_macros"

if [ "$fail" -ne 0 ]; then
	{
		echo ""
		echo "Codegen drift detected. Either:"
		echo "  - Regenerate the checked-in files:"
		echo "      cargo run --manifest-path ./enums/Cargo.toml -- \\"
		echo "          -lrust -o ./src/languages"
		echo "      cargo run --manifest-path ./enums/Cargo.toml -- \\"
		echo "          -lc_macros -o ./src/c_langs_macros"
		echo "      cargo fmt"
		echo "  - Or update enums/templates/ to match the checked-in form."
		echo "  - Or, for stale generated files in repo but not produced"
		echo "    by codegen, delete them and update enums/src/languages.rs."
		echo "See #405 for context."
	} >&2
	exit 1
fi

echo "enums-codegen-drift: OK"
