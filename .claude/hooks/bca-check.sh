#!/usr/bin/env bash
# Compatibility entrypoint for existing shell-form hook configurations.
set -euo pipefail
if ! command -v node >/dev/null 2>&1; then
	printf '%s\n' '[bca-check] Node.js unavailable; advisory check skipped' >&2
	exit 0
fi
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec node "$script_dir/bca-check.mjs"
