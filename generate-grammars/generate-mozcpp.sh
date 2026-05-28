#!/bin/bash

# This script updates the mozcpp grammar automatically.
#
# Usage: ./generate-grammars/generate-mozcpp.sh
#
# Toolchain requirement: the tree-sitter CLI is pinned to 0.26.9 in
# tree-sitter-mozcpp/package.json (to match the workspace
# `tree-sitter = "=0.26.9"` runtime). The npm-distributed 0.26.9
# binary is built against GLIBC 2.39, so on older hosts (e.g.
# Ubuntu 22.04 / glibc 2.35) `tree-sitter generate` aborts with
# `version 'GLIBC_2.39' not found`. If you hit that, build the CLI
# from source and put it on PATH ahead of the npm one:
#     cargo install tree-sitter-cli --version 0.26.9 --locked
# (cargo compiles against the local glibc, sidestepping the issue).

# Fail loud rather than limp on with a half-regenerated, garbage
# parser: a failed download / npm install / fetch must abort the
# run, not silently fall through to `tree-sitter generate`.
set -euo pipefail

# Name of the tree-sitter-cpp crate
TS_CPP_CRATE="tree-sitter-cpp"

# Filename of the JSON file containing the sha1 of the commit associated to
# the current tree-sitter-cpp crate version
JSON_CRATE_FILENAME=".cargo_vcs_info.json"

# Get the current tree-sitter-cpp crate version from the tree-sitter-mozcpp grammar
TS_CPP_VERSION=$(grep -m 1 "$TS_CPP_CRATE" tree-sitter-mozcpp/Cargo.toml | cut -f2 -d "," | cut -f2 -d "=" | tr -d ' ' | tr -d '}' | tr -d \")

# Name assigned to the compressed binary crate downloaded from crates.io
CRATE_OUTPUT="$TS_CPP_CRATE-download.gz"

# Link of the current tree-sitter-cpp crate on crates.io
CRATES_IO_LINK="https://crates.io/api/v1/crates/$TS_CPP_CRATE/$TS_CPP_VERSION/download"

# Download the crate from crates.io and uncompress it.
# crates.io rejects requests without a User-Agent (HTTP 403), so one
# must be supplied explicitly — a bare `wget` no longer works.
wget --header="User-Agent: big-code-analysis grammar regen" \
	-O "$CRATE_OUTPUT" "$CRATES_IO_LINK" && tar -xf "$CRATE_OUTPUT"

# Uncompressed directory name
CRATE_DIR="$TS_CPP_CRATE-$TS_CPP_VERSION"

# Get the sha1 of the commit associated to the current tree-sitter-cpp crate version
TS_CPP_SHA1=$(grep "sha1" "$CRATE_DIR/$JSON_CRATE_FILENAME" | cut -f2 -d ":" | tr -d ' ' | tr -d \")

# Remove compressed binary file and the relative uncompressed directory
rm -rf "$CRATE_OUTPUT" "$CRATE_DIR"

# Enter the mozcpp directory
pushd tree-sitter-mozcpp || exit

# Create tree-sitter-cpp directory
mkdir -p "$TS_CPP_CRATE"

# Enter tree-sitter-cpp directory
pushd "$TS_CPP_CRATE" || exit

# Shallow clone tree-sitter-cpp to a specific revision
git init
git remote add origin https://github.com/tree-sitter/tree-sitter-cpp.git
git fetch --depth 1 origin "$TS_CPP_SHA1"
git checkout FETCH_HEAD

# Install tree-sitter-cpp dependencies
npm install -y

# Pin the tree-sitter-c base grammar that tree-sitter-cpp's grammar.js
# extends (`require('tree-sitter-c/grammar')`). tree-sitter-cpp declares
# it as a floating `^0.23.1`, so a bare install would silently float to
# the latest 0.23.x and change the generated parser — the second
# non-reproducible axis behind issue #406 (the first being the
# tree-sitter-cli version, pinned in tree-sitter-mozcpp/package.json).
# 0.23.1 is the version the committed grammar.json/node-types.json
# correspond to; pinning it keeps the regen byte-reproducible.
npm install --no-save tree-sitter-c@0.23.1

# Exit tree-sitter-cpp directory
popd || exit

# Copy tree-sitter-cpp `scanner.c` functions into the `src` directory
cp --verbose "$TS_CPP_CRATE/src/scanner.c" ./src/scanner.c

# Since the tree-sitter-mozcpp `scanner.c` file contains the very same functions
# present in the tree-sitter-cpp `scanner.c` file, to avoid having a
# multiple symbol definition error during the linking phase,
# those functions will be assigned a new prefix.
sed -i 's/tree_sitter_cpp/tree_sitter_mozcpp/g' ./src/scanner.c

# Exit tree-sitter-mozcpp directory
popd || exit

# Generate tree-sitter-mozcpp grammar
./generate-grammars/generate-grammar.sh tree-sitter-mozcpp

# Delete tree-sitter-mozcpp/tree-sitter-cpp directory
rm -rf "./tree-sitter-mozcpp/$TS_CPP_CRATE"
