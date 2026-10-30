#!/bin/bash
# check-xgettext-backend
#
# Probes the installed `mdbook-xgettext` on a four-line book and
# fails unless a two-line run of `#` comments in a bash fence is
# extracted as one msgid. That single shape is what separates the two
# regex-engine builds below; the probe does not cover other grammars
# or comment positions.
#
# mdbook-i18n-helpers 0.4.0 builds syntect on the pure-Rust
# `fancy-regex` engine by default (google/mdbook-i18n-helpers#267),
# and under that engine the shell grammar mis-walks consecutive
# comment lines: a two-line run keeps only its first line, a
# three-line run its first and third (google/mdbook-i18n-helpers#321).
# `cargo install mdbook-i18n-helpers --features syntect/regex-onig`
# restores the Oniguruma engine 0.3.6 used and extracts the block
# whole. The two builds report the same `--version`, so only a
# behavioural probe can tell them apart — and the wrong one is the
# dangerous one: `make book-po-update` against its pot marks every
# multi-line comment entry in po/ja.po obsolete and adds the
# fragments as new untranslated entries (#1724).
#
# Needs `mdbook` and `mdbook-xgettext` on PATH; both are reported by
# name when missing.

set -euo pipefail

for tool in mdbook mdbook-xgettext; do
	if ! command -v "$tool" >/dev/null 2>&1; then
		echo "check-xgettext-backend: $tool not found on PATH" >&2
		exit 2
	fi
done

WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/bca-xgettext-probe.XXXXXX")
trap 'rm -rf "$WORK_DIR"' EXIT INT TERM HUP

mkdir -p "$WORK_DIR/book/src"
printf '[book]\ntitle = "probe"\n' >"$WORK_DIR/book/book.toml"
printf -- '- [probe](probe.md)\n' >"$WORK_DIR/book/src/SUMMARY.md"
# The backticks are a Markdown fence, not a command substitution.
# shellcheck disable=SC2016
printf '# Probe\n\n```bash\n# first line\n# second line\n```\n' >"$WORK_DIR/book/src/probe.md"

if ! MDBOOK_OUTPUT='{"xgettext": {}}' mdbook build "$WORK_DIR/book" -d "$WORK_DIR/out" >"$WORK_DIR/build.log" 2>&1; then
	echo "check-xgettext-backend: mdbook build failed:" >&2
	cat "$WORK_DIR/build.log" >&2
	exit 2
fi

pot="$WORK_DIR/out/messages.pot"
if [ ! -f "$pot" ]; then
	echo "check-xgettext-backend: no messages.pot produced under $WORK_DIR/out" >&2
	exit 2
fi

# gettext writes a multi-line msgid as `msgid ""` followed by one
# quoted line per source line, so the whole-block entry is these
# three consecutive lines. Two single-line entries — which is what a
# split would look like — share no such run, and the fancy-regex
# build emits only the first line's entry. `grep` cannot express
# "these lines, adjacent, in this order" (a newline in a `-F` pattern
# is alternation), so walk the file.
if awk '
	$0 == "msgid \"\""            { state = 1; next }
	state == 1 && $0 == "\"# first line\\n\""  { state = 2; next }
	state == 2 && $0 == "\"# second line\\n\"" { found = 1; exit }
	                               { state = 0 }
	END                            { exit !found }
' "$pot"; then
	echo "check-xgettext-backend: ok — consecutive comment lines extract as one block"
	exit 0
fi

echo "check-xgettext-backend: mdbook-xgettext did not extract a two-line comment run as one msgid." >&2
echo "  The installed helper was built on fancy-regex. Reinstall with the" >&2
echo "  --features syntect/regex-onig command in docs/development/translations.md." >&2
echo "  Extracted entries:" >&2
grep -A3 '^#: src/probe.md' "$pot" >&2 || true
exit 1
