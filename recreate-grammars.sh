#!/bin/bash

# tree-sitter version pins live in two manifests that must move in
# lockstep: [workspace.dependencies] in ../Cargo.toml and the
# [dependencies] block in enums/Cargo.toml (the enums crate is
# excluded from the workspace and cannot inherit). Bump both, then
# run this script.

# Clean old grammars builds
cargo clean --manifest-path ./enums/Cargo.toml

# Recreate all grammars
cargo run --manifest-path ./enums/Cargo.toml -- -lrust -o ./src/languages

# Recreate C macros
cargo run --manifest-path ./enums/Cargo.toml -- -lc_macros -o ./src/c_langs_macros

# Format the code of the recreated grammars
cargo fmt
