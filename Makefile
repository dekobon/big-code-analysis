MAKE_MAJOR_VER    := $(shell echo $(MAKE_VERSION) | cut -d'.' -f1)

ifneq ($(shell test $(MAKE_MAJOR_VER) -gt 3; echo $$?),0)
$(error Make version $(MAKE_VERSION) is not supported, please install GNU Make 4.x)
endif

# Strict shell and Make settings for robust recipes
SHELL          := bash
.SHELLFLAGS    := -eu -o pipefail -c
.DELETE_ON_ERROR:
MAKEFLAGS      += --warn-undefined-variables
MAKEFLAGS      += --no-builtin-rules
.DEFAULT_GOAL  := help

# Directory path of Makefile
BASE_DIR       := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

# Directories excluded from linting and file-search operations.
# `tests/repositories` holds vendored fixtures (incl. the
# big-code-analysis-output submodule); `tree-sitter-*` are vendored
# grammar crates that follow upstream conventions; `enums/` is excluded
# from the workspace and owns its own files. None of these may be
# reformatted by this project's tooling. Glob entries (e.g. `tree-sitter-*`)
# are quoted at the use site below — keep this list plain.
EXCLUDE_DIRS   := .claude .git target tests/repositories \
                  big-code-analysis-book/book \
                  tree-sitter-* enums

# File finder: prefer fd/fdfind (fast, .gitignore-aware), fall back to find
FD             := $(shell command -v fdfind 2>/dev/null || command -v fd 2>/dev/null)

# Precomputed exclusion flags for fd and find. Single-quote each entry so
# the recipe shell does not glob-expand patterns like `tree-sitter-*` into
# absolute paths before fd/find see them (see #160). `find -path` uses its
# own glob engine and accepts unquoted patterns the same way fd does.
FD_EXCLUDE     := $(foreach dir,$(EXCLUDE_DIRS),--exclude '$(dir)')
FIND_EXCLUDE   := $(foreach dir,$(EXCLUDE_DIRS),! -path './$(dir)/*')

# Find files by extension with fd (preferred) or find (fallback).
# Usage: $(call find-by-ext,EXTENSION,EXTRA_FD_ARGS). Always pass the
# second arg (empty if unused) to avoid --warn-undefined-variables
# warnings on `$(2)`, e.g. $(call find-by-ext,md,).
find-by-ext = $(if $(FD),$(FD) --extension $(1) $(FD_EXCLUDE) $(2),find . -name "*.$(1)" -type f $(FIND_EXCLUDE))

.PHONY: help check-tools build build-release check test test-doc fmt fmt-check markdown-fmt markdown-lint shellcheck sh-fmt sh-fmt-check toml-fmt toml-fmt-check toml-lint makefile-check snapshot-anchors enums-check lint clippy udeps insta-review insta-accept clean install install-cli install-web doc doc-open book book-serve all pre-commit ci _check-find _pc-fmt _pc-clippy _pc-test _pc-udeps _pc-shellcheck _pc-markdown-lint _pc-toml-lint _pc-makefile-check _pc-snapshot-anchors _pc-enums-check _ci-fmt-check _ci-clippy _ci-test _ci-build _ci-udeps _ci-shellcheck _ci-markdown-lint _ci-toml-lint _ci-makefile-check _ci-snapshot-anchors _ci-enums-check _ci-cargo-pipeline

# Default target
help:
	@echo "Build and test commands for big-code-analysis"
	@echo ""
	@echo "Usage: make <target>"
	@echo ""
	@echo "Prerequisites:"
	@echo "  check-tools                          Verify required tools are present"
	@echo ""
	@echo "Build targets:"
	@echo "  build                                Build debug binaries"
	@echo "  build-release                        Build optimized release binaries"
	@echo "  check                                Run cargo check"
	@echo ""
	@echo "Test targets:"
	@echo "  test                                 Run unit and integration tests"
	@echo "  test-doc                             Run cargo doc tests"
	@echo "  insta-review                         Review pending insta snapshot diffs"
	@echo "  insta-accept                         Accept all pending insta snapshots"
	@echo ""
	@echo "Code quality:"
	@echo "  fmt                                  Format Rust + Markdown + TOML + Bash"
	@echo "  fmt-check                            Verify formatting without modifying files"
	@echo "  clippy                               Run clippy with -D warnings"
	@echo "  udeps                                Detect unused deps (requires nightly)"
	@echo "  markdown-fmt                         Auto-fix Markdown with markdownlint-cli2"
	@echo "  markdown-lint                        Lint Markdown with markdownlint-cli2"
	@echo "  shellcheck                           Lint bash scripts"
	@echo "  sh-fmt                               Format bash scripts with shfmt"
	@echo "  sh-fmt-check                         Check bash formatting without modifying"
	@echo "  toml-fmt                             Format TOML files with taplo"
	@echo "  toml-fmt-check                       Check TOML formatting without modifying"
	@echo "  toml-lint                            Lint TOML files with taplo"
	@echo "  makefile-check                       Lint Makefile with checkmake"
	@echo "  snapshot-anchors                     Block new bare insta snapshots"
	@echo "  enums-check                          cargo check on workspace-excluded enums crate"
	@echo "  lint                                 Run all linters"
	@echo ""
	@echo "Maintenance:"
	@echo "  clean                                Remove build artifacts"
	@echo "  install                              Install both CLI and web binaries"
	@echo "  install-cli                          Install bca"
	@echo "  install-web                          Install bca-web"
	@echo ""
	@echo "Documentation:"
	@echo "  doc                                  Generate rustdoc"
	@echo "  doc-open                             Generate and open rustdoc"
	@echo "  book                                 Build the mdBook"
	@echo "  book-serve                           Serve the mdBook with live reload"
	@echo ""
	@echo "Combined targets:"
	@echo "  all                                  Check, test, build release"
	@echo "  pre-commit                           Verify formatting, lint, test (recommended before commit)"
	@echo "  ci                                   Validate formatting, lint, test (no auto-fix)"

# ---------------------------------------------------------------------------
# Prerequisites
# ---------------------------------------------------------------------------
check-tools:
	@bash $(BASE_DIR)utils/check-tools.sh

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
build:
	cargo build --workspace --all-targets

build-release:
	cargo build --workspace --release

check:
	cargo check --workspace --all-targets

# ---------------------------------------------------------------------------
# Test
# ---------------------------------------------------------------------------
test:
	cargo test --workspace --all-features --lib --bins --tests

test-doc:
	cargo test --workspace --all-features --doc

insta-review:
	cargo insta test --review

insta-accept:
	cargo insta test --accept

# ---------------------------------------------------------------------------
# Formatting
# ---------------------------------------------------------------------------
fmt:
	@echo "Formatting Rust code..."
	@cargo fmt --all
	@echo "Formatting Markdown files..."
	@$(MAKE) --no-print-directory markdown-fmt
	@echo "Formatting bash scripts..."
	@$(MAKE) --no-print-directory sh-fmt
	@echo "Formatting TOML files..."
	@$(MAKE) --no-print-directory toml-fmt

fmt-check:
	@echo "Checking Rust code formatting..."
	@cargo fmt --all --check || { echo "Rust code is not formatted (run 'make fmt')"; exit 1; }
	@echo "Checking Markdown formatting..."
	@$(MAKE) --no-print-directory markdown-lint
	@echo "Checking bash script formatting..."
	@$(MAKE) --no-print-directory sh-fmt-check
	@echo "Checking TOML formatting..."
	@$(MAKE) --no-print-directory toml-fmt-check
	@echo "All formatting checks passed"

# Sanity guard for the find-by-ext helper. If EXCLUDE_DIRS over-matches
# (as it did in #160 when `tree-sitter-*` was unquoted and the recipe
# shell expanded the glob into absolute paths), every lint that pipes
# through `xargs -r` silently no-ops. Run as a prerequisite of every
# recipe that consumes find-by-ext.
_check-find:
	@N=$$($(call find-by-ext,md,) | wc -l); \
	  [ "$$N" -ge 5 ] || { echo "ERROR: find-by-ext returned $$N .md files (expected >=5); EXCLUDE_DIRS is over-matching — see #160"; exit 1; }

markdown-fmt: _check-find
	@echo "Auto-fixing Markdown files..."
	@$(call find-by-ext,md,) | xargs -r markdownlint-cli2 --fix || { echo "markdownlint-cli2 could not auto-fix all issues"; exit 1; }

markdown-lint: _check-find
	@echo "Linting Markdown files..."
	@$(call find-by-ext,md,) | xargs -r markdownlint-cli2 || { echo "markdownlint-cli2 found issues"; exit 1; }

sh-fmt: _check-find
	@$(call find-by-ext,sh,) | xargs -r shfmt -w -i 0 -ci -bn

sh-fmt-check: _check-find
	@$(call find-by-ext,sh,) | xargs -r shfmt -d -i 0 -ci -bn || { echo "Bash scripts are not formatted (run 'make sh-fmt')"; exit 1; }

shellcheck: _check-find
	@echo "Linting bash scripts with shellcheck..."
	@$(call find-by-ext,sh,) | xargs -r shellcheck || { echo "Shellcheck found issues"; exit 1; }

toml-fmt:
	@taplo fmt

toml-fmt-check:
	@taplo fmt --check || { echo "TOML files are not formatted (run 'make toml-fmt')"; exit 1; }

toml-lint:
	@echo "Linting TOML files..."
	@taplo lint || { echo "TOML lint found issues"; exit 1; }

makefile-check:
	@echo "Linting Makefile with checkmake..."
	@checkmake --config $(BASE_DIR).checkmake.ini $(BASE_DIR)Makefile || { echo "checkmake found issues"; exit 1; }

snapshot-anchors:
	@echo "Checking insta snapshot anchors..."
	@python3 $(BASE_DIR)check-snapshot-anchors.py

# The `enums/` crate is listed in `[workspace].exclude` (it ships a
# non-published codegen binary used only by `recreate-grammars.sh`), so
# it is invisible to `cargo {check,clippy,test} --workspace` and to the
# `lint` / `test` CI jobs. Without an explicit gate, lint regressions in
# `enums/src/*.rs` drift silently — the `unused_imports` warning fixed
# in #162 went unnoticed for that exact reason (see #164).
#
# RUSTFLAGS is set per-recipe (defensive): CI exports the same value at
# workflow scope, so the recipe-local export is redundant in CI but
# necessary locally to keep `make enums-check` behave identically
# everywhere.
#
# This is intentionally `cargo check`, not `cargo clippy`: clippy
# requires fixing 3 pre-existing `manual_is_ascii_check` sites in
# `enums/src/common.rs` first (tracked as #166). `cargo check` already
# catches the rustc-level warning class that motivated #164 (e.g.,
# `unused_imports`, `dead_code`), so the gate closes that drift today.
enums-check:
	@echo "Checking workspace-excluded enums crate..."
	@RUSTFLAGS="-D warnings" cargo check \
	  --manifest-path $(BASE_DIR)enums/Cargo.toml \
	  --all-targets --locked

# ---------------------------------------------------------------------------
# Lint aggregate
# ---------------------------------------------------------------------------
clippy:
	@echo "Running Rust lints..."
	@cargo clippy --workspace --all-targets -- -D warnings
	@cargo clippy --workspace --all-targets --all-features -- -D warnings

udeps:
	@echo "Detecting unused dependencies..."
	@cargo +nightly udeps --workspace --all-targets

# Reuse the _ci-* family so `make lint` runs the same set of gates as
# `make ci`'s non-cargo-pipeline branch, in parallel. _ci-clippy holds
# the workspace `target/` lock; the other lints don't use cargo at all
# (or use a separate `target/`, in _ci-enums-check's case), so they
# fan out safely.
lint:
	$(MAKE) -j --output-sync=target \
	  _ci-clippy \
	  _ci-shellcheck _ci-markdown-lint _ci-toml-lint _ci-makefile-check \
	  _ci-snapshot-anchors _ci-enums-check

# ---------------------------------------------------------------------------
# Maintenance
# ---------------------------------------------------------------------------
clean:
	cargo clean

install: install-cli install-web

install-cli:
	RUSTFLAGS="-C target-cpu=native" cargo install --path big-code-analysis-cli

install-web:
	RUSTFLAGS="-C target-cpu=native" cargo install --path big-code-analysis-web

# ---------------------------------------------------------------------------
# Documentation
# ---------------------------------------------------------------------------
doc:
	cargo doc --no-deps --workspace --all-features

doc-open:
	cargo doc --no-deps --workspace --all-features --open

book:
	mdbook build big-code-analysis-book

book-serve:
	mdbook serve big-code-analysis-book

# ---------------------------------------------------------------------------
# Combined workflows
# ---------------------------------------------------------------------------
all: check test build-release

pre-commit:
	$(MAKE) -j --output-sync=target \
	  _pc-test \
	  _pc-shellcheck _pc-markdown-lint _pc-toml-lint _pc-makefile-check \
	  _pc-snapshot-anchors _pc-enums-check
	@echo "Pre-commit checks passed"

ci:
	$(MAKE) _ci-fmt-check
	$(MAKE) -j --output-sync=target \
	  _ci-cargo-pipeline \
	  _ci-shellcheck _ci-markdown-lint _ci-toml-lint _ci-makefile-check \
	  _ci-snapshot-anchors _ci-enums-check
	@echo "CI checks passed"

# ---------------------------------------------------------------------------
# Parallel pre-commit DAG
#
# These _pc-* targets express the dependency graph so `make -j` runs
# independent stages concurrently. The `pre-commit` target invokes them
# with `-j --output-sync=target`.
#
# All cargo invocations (clippy, test, udeps) share the package cache and
# target/ lock, so they are serialized into one chain. Non-cargo checks
# run in parallel with the cargo pipeline, gated only on _pc-fmt.
#
# Dependency graph:
#
#   _pc-fmt
#    ├── _pc-clippy → _pc-test → _pc-udeps
#    ├── _pc-shellcheck
#    ├── _pc-markdown-lint
#    ├── _pc-toml-lint
#    ├── _pc-makefile-check
#    ├── _pc-snapshot-anchors
#    └── _pc-enums-check
#
# _pc-enums-check runs cargo on `enums/Cargo.toml`, which has its own
# `target/` (the crate is workspace-excluded), so it does NOT share the
# `target/` lock with the workspace cargo chain and is safe to run in
# parallel with _pc-clippy/_pc-test/_pc-udeps.
#
# Do not invoke _pc-* targets directly; use `make pre-commit`.
# ---------------------------------------------------------------------------
_pc-fmt:
	$(MAKE) fmt-check

_pc-clippy: _pc-fmt
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy --workspace --all-targets --all-features -- -D warnings

_pc-test: _pc-clippy
	cargo test --workspace --all-features --lib --bins --tests
	cargo test --workspace --all-features --doc

_pc-udeps: _pc-test
	cargo +nightly udeps --workspace --all-targets

_pc-shellcheck: _pc-fmt
	$(MAKE) shellcheck

_pc-markdown-lint: _pc-fmt
	$(MAKE) markdown-lint

_pc-toml-lint: _pc-fmt
	$(MAKE) toml-lint

_pc-makefile-check: _pc-fmt
	$(MAKE) makefile-check

_pc-snapshot-anchors: _pc-fmt
	$(MAKE) snapshot-anchors

_pc-enums-check: _pc-fmt
	$(MAKE) enums-check

# ---------------------------------------------------------------------------
# CI validation targets (no auto-formatting)
#
# These _ci-* targets have NO prerequisites — they can be invoked
# individually from GitHub Actions workflow steps or composed via `make ci`
# for local use.
#
# Execution order (enforced by `ci` target + _ci-cargo-pipeline):
#   1. _ci-fmt-check (sequential, must pass before anything else)
#   2. parallel:
#      _ci-cargo-pipeline: clippy → test → build → udeps
#      _ci-shellcheck, _ci-markdown-lint, _ci-toml-lint, _ci-makefile-check,
#      _ci-snapshot-anchors, _ci-enums-check
#
# _ci-enums-check runs on `enums/Cargo.toml`, which has its own `target/`
# (workspace-excluded), so it does NOT share the workspace cargo lock and
# is safe to run alongside _ci-cargo-pipeline.
# ---------------------------------------------------------------------------
_ci-fmt-check:
	$(MAKE) fmt-check

_ci-clippy:
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy --workspace --all-targets --all-features -- -D warnings

_ci-test:
	cargo test --workspace --all-features --lib --bins --tests
	cargo test --workspace --all-features --doc

_ci-build:
	cargo build --workspace --all-targets

_ci-udeps:
	cargo +nightly udeps --workspace --all-targets

_ci-shellcheck:
	$(MAKE) shellcheck

_ci-markdown-lint:
	$(MAKE) markdown-lint

_ci-toml-lint:
	$(MAKE) toml-lint

_ci-makefile-check:
	$(MAKE) makefile-check

_ci-snapshot-anchors:
	$(MAKE) snapshot-anchors

_ci-enums-check:
	$(MAKE) enums-check

# Sequential cargo pipeline for local `make ci`. udeps shares the cargo
# target/ lock with the rest of the pipeline, so it is serialized here
# rather than running in parallel.
_ci-cargo-pipeline:
	$(MAKE) _ci-clippy
	$(MAKE) _ci-test
	$(MAKE) _ci-build
	$(MAKE) _ci-udeps
