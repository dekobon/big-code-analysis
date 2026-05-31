#!/bin/bash

# Stop at the first error
set -e

# Get tree-sitter-grammar
TS_CRATE=$(grep "$1" Cargo.toml | tr -d ' ')

# Disable/Enable CI flag
RUN_CI="no"

# Temporary master branch Cargo.toml filename
MASTER_CARGO_TOML="master-cargo.toml"

# Download master branch Cargo.toml and save it in a temporary file
wget -LqO - https://raw.githubusercontent.com/dekobon/big-code-analysis/master/Cargo.toml | tr -d ' ' >"$MASTER_CARGO_TOML"

# Get the name of the current crate
TS_CRATE_NAME=$(echo "$TS_CRATE" | cut -f1 -d "=")

# Get the crate name from the master branch Cargo.toml
MASTER_TS_CRATE_NAME=$(grep "$TS_CRATE_NAME" "$MASTER_CARGO_TOML" | head -n 1 | cut -f1 -d "=")

# If the current crate name is not present in master branch, exit the script
if [ -z "$MASTER_TS_CRATE_NAME" ]; then
	exit 0
fi

# Get the same crate from the master branch Cargo.toml
MASTER_TS_CRATE=$(grep "$TS_CRATE" "$MASTER_CARGO_TOML" | head -n 1)

# If the current crate has been updated, save the crate name
if [ -z "$MASTER_TS_CRATE" ]; then
	# Enable CI flag
	RUN_CI="yes"
	# Name of tree-sitter crate
	TREE_SITTER_CRATE=$TS_CRATE_NAME
fi

# Remove temporary master branch Cargo.toml file
rm -rf "$MASTER_CARGO_TOML"

# If any crates have been updated, exit the script
if [ "$RUN_CI" = "no" ]; then
	exit 0
fi

# Download mozilla-central repository
MOZILLA_CENTRAL_REPO="https://github.com/mozilla/gecko-dev"
if [ ! -d "/cache/gecko-dev" ]; then
	git clone --quiet "$MOZILLA_CENTRAL_REPO" /cache/gecko-dev || true
fi
pushd /cache/gecko-dev && git pull origin master && popd

# Compute metrics
./check-grammar-crate.py compute-ci-metrics -p /cache/gecko-dev -g "$TREE_SITTER_CRATE"

# Count files in metrics directories
OLD=$(find "/tmp/$TREE_SITTER_CRATE-old" -mindepth 1 -maxdepth 1 | wc -l)
NEW=$(find "/tmp/$TREE_SITTER_CRATE-new" -mindepth 1 -maxdepth 1 | wc -l)

# Print number of files contained in metrics directories
echo "$TREE_SITTER_CRATE-old: $OLD"
echo "$TREE_SITTER_CRATE-new: $NEW"

# If metrics directories differ in number of files,
# print only the files that are in a directory but not in the other one
if [ "$OLD" != "$NEW" ]; then
	ONLY_FILES=$(diff -q "/tmp/$TREE_SITTER_CRATE-old" "/tmp/$TREE_SITTER_CRATE-new" | grep "Only in")
	echo "$ONLY_FILES"
fi

# Compare metrics: `bca diff` buckets the per-file deltas by metric and
# saves a machine-readable diff.json into the compare directory. This
# replaces the former json-minimal-tests + split-minimal-tests.py chain
# (issue #487).
#
# MIN_CHANGE is the minimum absolute per-file metric change to report; 0
# reports any change (the former MT_THRESHOLD capped the count of
# minimal-test files per metric, a different axis that bca diff's
# per-metric bucketing makes unnecessary).
MIN_CHANGE=0
./check-grammar-crate.py compare-metrics -g "$TREE_SITTER_CRATE" -t "$MIN_CHANGE"

# Create artifact to be uploaded (if there is any diff output)
COMPARE=/tmp/$TREE_SITTER_CRATE-compare
if [ "$(ls -A "$COMPARE")" ]; then
	# Grammar name (removes tree-sitter- prefix)
	GRAMMAR_NAME=$(echo "$TREE_SITTER_CRATE" | cut -c 13-)

	tar -czvf "/tmp/metric-diff-$GRAMMAR_NAME.tar.gz" "$COMPARE"
fi
