#!/bin/bash

# This script generates a Mozilla-defined grammar automatically.
#
# Usage: ./generate-grammars/generate-grammar.sh GRAMMAR_NAME

# Fail loud. This script runs the actual `tree-sitter generate`; if
# that aborts (e.g. a too-old glibc for the pinned CLI, or a grammar
# error) the run must stop rather than fall through to
# recreate-grammars + `cargo test` against the *unchanged* parser,
# which would report success and leave the caller believing a regen
# happened when nothing changed. Callers (generate-mozcpp.sh,
# generate-mozjs.sh) are separate processes, so their own `set -e`
# does not cover this child — it needs its own.
set -euo pipefail

# Enter grammar directory
pushd "$1" || exit

# Install dependencies
npm install --include=dev

# Generate grammar
./node_modules/.bin/tree-sitter generate

# Delete node_modules
rm -rf node_modules

# Exit grammar directory
popd || exit

# Recreate grammars
./recreate-grammars.sh

# Run rust code-analysis to verify if everything works correctly and to
# update the Cargo.lock
cargo clean && cargo test --workspace
