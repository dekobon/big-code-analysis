#!/usr/bin/env bash
# Dogfood of the "Feeding metrics to an agent" recipe
# (big-code-analysis-book/src/recipes/agent-feedback.md).
#
# PostToolUse hook: after Claude edits a file, run `bca check` on just
# that file and, only when the per-function thresholds in the repo-root
# bca.toml are exceeded, feed the offender list + guidance back to the
# model. Stays silent (exit 0) on a clean file, an unsupported file
# type, a tool error, or a missing analyzer — it must never block an
# edit.
set -euo pipefail

root="${CLAUDE_PROJECT_DIR:-$PWD}"

# Resolve the analyzer: explicit override first, then this checkout's
# release build (so we dogfood THIS repo's bca, not whatever is on
# PATH), then a bca on PATH. No analyzer ⇒ silent no-op.
if [ -n "${BCA:-}" ]; then
	bca="$BCA"
elif [ -x "$root/target/release/bca" ]; then
	bca="$root/target/release/bca"
elif command -v bca >/dev/null 2>&1; then
	bca="bca"
else
	exit 0
fi

# PostToolUse delivers the tool call as JSON on stdin; the edited file's
# path is .tool_input.file_path for Edit/Write/MultiEdit.
file_path="$(jq -r '.tool_input.file_path // empty')"
[ -n "$file_path" ] || exit 0 # nothing to check.
[ -f "$file_path" ] || exit 0 # file gone (e.g. a delete).
case "$file_path" in          # only gate files inside this repo.
	"$root"/*) ;;
	*) exit 0 ;;
esac

# bca check exits 0 clean, 2 on offenders, 1 on tool error. Branch on 2
# so an unsupported file type or config error stays silent rather than
# being mislabelled as "complexity".
status=0
report="$("$bca" check "$file_path" --no-summary --no-remediation 2>&1)" || status=$?
case "$status" in
	0) exit 0 ;;
	2) ;;
	*) exit 0 ;;
esac

# Exit 2 makes Claude read stderr as context about the edit.
guidance="$root/.claude/hooks/bca-guidance.txt"
cat >&2 <<EOF
bca flagged complexity in the file you just edited:

$report

$([ -f "$guidance" ] && cat "$guidance")
EOF
exit 2
