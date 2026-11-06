#!/bin/bash
# check-deny-manifests
#
# Runs `cargo-deny check <checks>` once per tracked `Cargo.lock`, against
# the root `deny.toml`. Usage:
#
#     utils/check-deny-manifests.sh advisories bans licenses sources
#
# The repository tracks eight lockfiles: the root workspace's and one per
# `[workspace].exclude` crate (`enums`, `fuzz`, the five `tree-sitter-*`),
# each of which roots its own workspace and resolves its own graph. A
# cargo-deny run from the root reads only the first. Until #1823 the other
# seven were read by the Scorecard job alone, on `main`, after merge:
# RUSTSEC-2026-0374 sat in `enums/Cargo.lock` that way and passed every
# PR's `deny` job (fixed by #1822).
#
# Both `deny` jobs call this script (`.github/workflows/ci.yml` and
# `.github/workflows/deny-advisories.yml`), so the per-PR and the weekly
# run cannot disagree on which graphs they cover. `make pre-commit` runs
# only its self-test, `utils/check-deny-manifests-test.py`, which drives
# it against a stubbed cargo-deny: the real run needs the network.

set -euo pipefail

# The parent of `utils/`, from this script's own location, so the gate
# reads the tree it was invoked from whatever the caller's cwd.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# The root lockfile plus the seven excluded crates'. A lockfile that
# appears or disappears fails the gate until this number is changed by
# hand, so a ninth excluded crate cannot enter the tree unaudited — the
# reasoning `utils/check-excluded-manifests.py` applies to the manifests
# themselves (#1151, #1228).
EXPECTED_LOCKFILES=8

# cargo-deny 0.20 moved `--config` from the `check` subcommand to the top
# level, and each side rejects the other's spelling. CI pins a 0.20
# release through `taiki-e/install-action`; an older local copy fails
# here, with the remedy, rather than with clap's tip to move the flag
# back. The install hints below name CI's exact pin, and
# `utils/check-deny-manifests-test.py` holds them to it.
MIN_DENY_MAJOR=0
MIN_DENY_MINOR=20

die() {
	echo "check-deny-manifests: $*" >&2
	exit 2
}

if [ "$#" -eq 0 ]; then
	die "name the checks to run, e.g. 'advisories bans licenses sources'"
fi

# Called directly rather than as `cargo deny`: cargo resolves a
# subcommand from `$CARGO_HOME/bin` before PATH, which would put an
# installed copy ahead of the one PATH names (and of the self-test's stub).
if ! version_line=$(cargo-deny --version 2>/dev/null); then
	die "cargo-deny not found; install the version CI pins:" \
		"cargo install --locked cargo-deny@0.20.2"
fi
version=${version_line##* }
major=${version%%.*}
minor=${version#*.}
minor=${minor%%.*}
if ! [[ "$major" =~ ^[0-9]+$ && "$minor" =~ ^[0-9]+$ ]]; then
	die "cannot read a version from '$version_line'"
fi
if [ "$major" -eq "$MIN_DENY_MAJOR" ] && [ "$minor" -lt "$MIN_DENY_MINOR" ]; then
	die "cargo-deny $version is older than $MIN_DENY_MAJOR.$MIN_DENY_MINOR," \
		"which this script's '--config' placement needs; install CI's pin:" \
		"cargo install --locked cargo-deny@0.20.2"
fi

# The tracked set, not the filesystem: corpora, `target/` and scratch
# checkouts carry lockfiles nobody ships. `**/` also matches the root,
# and the glob pathspec keeps `NotCargo.lock` out, which `*Cargo.lock`
# would not.
if ! listing=$(git -C "$ROOT" ls-files -- ':(glob)**/Cargo.lock'); then
	die "git ls-files failed in $ROOT; this gate reads the tracked lockfiles"
fi
locks=()
while IFS= read -r lock; do
	if [ -n "$lock" ]; then
		locks+=("$lock")
	fi
done <<<"$listing"

if [ "${#locks[@]}" -ne "$EXPECTED_LOCKFILES" ]; then
	{
		echo "check-deny-manifests: expected $EXPECTED_LOCKFILES tracked Cargo.lock files, found ${#locks[@]}:"
		# The `+` form: an empty array is unbound under `set -u` before bash 4.4.
		printf '  %s\n' ${locks[@]+"${locks[@]}"}
		echo "An excluded crate was added or removed. Run this script with every"
		echo "check against the new set, then change EXPECTED_LOCKFILES in"
		echo "utils/check-deny-manifests.sh to match."
	} >&2
	exit 1
fi

failed=()
for lock in "${locks[@]}"; do
	manifest="${lock%Cargo.lock}Cargo.toml"
	echo "==> cargo-deny check $*: $manifest"
	# `--locked` audits the committed lockfile rather than a graph cargo
	# re-resolved on the way, and refuses to rewrite a tracked lockfile
	# when the script runs in a working tree.
	if ! cargo-deny --locked \
		--manifest-path "$ROOT/$manifest" \
		--config "$ROOT/deny.toml" \
		check "$@"; then
		failed+=("$manifest")
	fi
done

if [ "${#failed[@]}" -gt 0 ]; then
	{
		echo "check-deny-manifests: cargo-deny failed for ${#failed[@]} of ${#locks[@]} manifests:"
		printf '  %s\n' "${failed[@]}"
	} >&2
	exit 1
fi

echo "check-deny-manifests: OK (${#locks[@]} manifests: $*)"
