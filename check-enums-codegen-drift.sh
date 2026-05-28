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

ROOT=$(git rev-parse --show-toplevel)
TMPDIR=$(mktemp -d -t bca-enums-drift-XXXXXX)
trap 'rm -rf "$TMPDIR"' EXIT

mkdir -p "$TMPDIR/languages" "$TMPDIR/c_langs_macros"

if ! (cd "$ROOT" && cargo run --manifest-path ./enums/Cargo.toml --quiet -- \
	-lrust -o "$TMPDIR/languages"); then
	echo "error: enums codegen (-lrust) failed" >&2
	exit 2
fi
if ! (cd "$ROOT" && cargo run --manifest-path ./enums/Cargo.toml --quiet -- \
	-lc_macros -o "$TMPDIR/c_langs_macros"); then
	echo "error: enums codegen (-lc_macros) failed" >&2
	exit 2
fi

# Format the generated files so the diff against the rustfmt'd
# checked-in files doesn't trip on whitespace.
find "$TMPDIR" -name "*.rs" -print0 | xargs -0 rustfmt --edition 2024

fail=0
diff_dir() {
	local source_dir="$1"
	local target_subdir="$2"
	for f in "$source_dir"/*.rs; do
		local base
		base=$(basename "$f")
		local checked_in="$ROOT/$target_subdir/$base"
		if [ ! -f "$checked_in" ]; then
			echo "drift: $target_subdir/$base exists in codegen output but not in repo" >&2
			fail=1
			continue
		fi
		if ! diff -q "$checked_in" "$f" >/dev/null 2>&1; then
			echo "drift: $target_subdir/$base" >&2
			diff -u "$checked_in" "$f" | head -40 >&2
			fail=1
		fi
	done
}

diff_dir "$TMPDIR/languages" "src/languages"
diff_dir "$TMPDIR/c_langs_macros" "src/c_langs_macros"

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
		echo "See #405 for context."
	} >&2
	exit 1
fi

echo "enums-codegen-drift: OK"
