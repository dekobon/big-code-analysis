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

# `ROOT` is the directory containing the script (which is also
# the directory containing `enums/`, `src/`, etc.). Using
# `BASH_SOURCE` dirname rather than `git rev-parse
# --show-toplevel` keeps the gate hermetic: it doesn't matter
# what the caller's cwd is (e.g., a different git repo or
# `/tmp` in test fixtures), the script always operates on its
# own sibling directories. This also works under release
# tarballs / packaging-script invocations that have no `.git`.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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
# target subdir under `src/`. Parallel arrays (rather than a
# `:`-separated single array) keep this safe if a future mode
# name ever contains `:` and stay bash-3 compatible (no
# associative-array dependency for macOS contributors).
MODES=("rust" "c_macros")
SUBDIRS=("languages" "c_langs_macros")

for i in "${!MODES[@]}"; do
	mode="${MODES[$i]}"
	subdir="${SUBDIRS[$i]}"
	if ! cargo run --manifest-path "$MANIFEST" --quiet -- \
		-l"$mode" -o "$WORK_DIR/$subdir"; then
		echo "error: enums codegen (-l$mode) failed" >&2
		exit 2
	fi
done

# Format generated files so the diff against the rustfmt'd
# checked-in files isn't tripped by whitespace. The two codegen
# output subdirs are flat and known, so glob them directly rather
# than depend on `fd` being installed — this gate runs under
# `make lint`, whose CI image is intentionally minimal and ships
# no fd/fdfind. `shopt -s nullglob` (set above) makes an empty
# codegen output a no-op: the array stays empty and rustfmt is
# skipped. `rustfmt` is part of the Rust toolchain, always present
# wherever the codegen above could build.
#
# `--edition 2024` must track the workspace + enums Cargo.toml
# `edition = "2024"` setting; a workspace edition migration
# requires updating this flag in lockstep with both manifests.
generated_files=("$WORK_DIR"/languages/*.rs "$WORK_DIR"/c_langs_macros/*.rs)
if [ "${#generated_files[@]}" -gt 0 ]; then
	rustfmt --edition 2024 "${generated_files[@]}"
fi

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
			# Capture the full diff once so we can both show
			# the truncated head AND report how much was
			# hidden. The `|| true` lets diff's non-zero exit
			# (it's expected here) flow into the script
			# without tripping `set -e + pipefail`.
			local full_diff
			full_diff="$(diff -u "$checked_in" "$f" 2>/dev/null || true)"
			local total_lines
			total_lines=$(printf '%s\n' "$full_diff" | wc -l)
			printf '%s\n' "$full_diff" | head -40 >&2
			if [ "$total_lines" -gt 40 ]; then
				echo "  ... ($((total_lines - 40)) more diff lines hidden;" \
					"run the regen locally for the full output)" >&2
			fi
			fail=1
		fi
	done

	# Reverse: checked-in → codegen output. Skip `mod.rs`
	# (hand-maintained module index, not generated). If a
	# future hand-maintained file is added to either target
	# subdir (e.g., a `src/languages/shared.rs`), extend the
	# skip list — flagged orphans would otherwise look like
	# real drift to a confused reviewer.
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
