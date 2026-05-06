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
# reformatted by this project's tooling.
EXCLUDE_DIRS   := .claude .git target tests/repositories \
                  big-code-analysis-book/book \
                  tree-sitter-* enums

# File finder: prefer fd/fdfind (fast, .gitignore-aware), fall back to find
FD             := $(shell command -v fdfind 2>/dev/null || command -v fd 2>/dev/null)

# Precomputed exclusion flags for fd and find
FD_EXCLUDE     := $(foreach dir,$(EXCLUDE_DIRS),--exclude $(dir))
FIND_EXCLUDE   := $(foreach dir,$(EXCLUDE_DIRS),! -path "./$(dir)/*")

# Find files by extension with fd (preferred) or find (fallback).
# Usage: $(call find-by-ext,EXTENSION[,EXTRA_FD_ARGS[,EXTRA_FIND_ARGS]])
# Always pass all three args (use empty for unused) to avoid
# --warn-undefined-variables warnings, e.g. $(call find-by-ext,md,,).
find-by-ext = $(if $(FD),$(FD) --extension $(1) $(FD_EXCLUDE) $(2),find . -name "*.$(1)" -type f $(FIND_EXCLUDE) $(3))

.PHONY: help check-tools build build-release check test test-doc fmt fmt-check markdown-fmt markdown-lint shellcheck sh-fmt sh-fmt-check toml-fmt toml-fmt-check toml-lint makefile-check lint clippy udeps insta-review insta-accept clean install install-cli install-web doc doc-open book book-serve all pre-commit ci _pc-fmt _pc-clippy _pc-test _pc-udeps _pc-shellcheck _pc-markdown-lint _pc-toml-lint _pc-makefile-check _ci-fmt-check _ci-clippy _ci-test _ci-build _ci-udeps _ci-shellcheck _ci-markdown-lint _ci-toml-lint _ci-makefile-check _ci-cargo-pipeline

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
	@echo "  lint                                 Run all linters"
	@echo ""
	@echo "Maintenance:"
	@echo "  clean                                Remove build artifacts"
	@echo "  install                              Install both CLI and web binaries"
	@echo "  install-cli                          Install big-code-analysis-cli"
	@echo "  install-web                          Install big-code-analysis-web"
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

markdown-fmt:
	@echo "Auto-fixing Markdown files..."
	@$(call find-by-ext,md,,) | xargs -r markdownlint-cli2 --fix || { echo "markdownlint-cli2 could not auto-fix all issues"; exit 1; }

markdown-lint:
	@echo "Linting Markdown files..."
	@$(call find-by-ext,md,,) | xargs -r markdownlint-cli2 || { echo "markdownlint-cli2 found issues"; exit 1; }

sh-fmt:
	@$(call find-by-ext,sh,,) | xargs -r shfmt -w -i 0 -ci -bn

sh-fmt-check:
	@$(call find-by-ext,sh,,) | xargs -r shfmt -d -i 0 -ci -bn || { echo "Bash scripts are not formatted (run 'make sh-fmt')"; exit 1; }

shellcheck:
	@echo "Linting bash scripts with shellcheck..."
	@$(call find-by-ext,sh,,) | xargs -r shellcheck || { echo "Shellcheck found issues"; exit 1; }

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

lint:
	@$(MAKE) --no-print-directory clippy
	@echo "Running bash script lints..."
	@$(MAKE) --no-print-directory shellcheck
	@echo "Running Markdown lints..."
	@$(MAKE) --no-print-directory markdown-lint
	@echo "Running TOML lints..."
	@$(MAKE) --no-print-directory toml-lint
	@echo "Running Makefile lints..."
	@$(MAKE) --no-print-directory makefile-check

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
	  _pc-shellcheck _pc-markdown-lint _pc-toml-lint _pc-makefile-check
	@echo "Pre-commit checks passed"

ci:
	$(MAKE) _ci-fmt-check
	$(MAKE) -j --output-sync=target \
	  _ci-cargo-pipeline \
	  _ci-shellcheck _ci-markdown-lint _ci-toml-lint _ci-makefile-check
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
#    └── _pc-makefile-check
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
#      _ci-shellcheck, _ci-markdown-lint, _ci-toml-lint, _ci-makefile-check
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

# Sequential cargo pipeline for local `make ci`. udeps shares the cargo
# target/ lock with the rest of the pipeline, so it is serialized here
# rather than running in parallel.
_ci-cargo-pipeline:
	$(MAKE) _ci-clippy
	$(MAKE) _ci-test
	$(MAKE) _ci-build
	$(MAKE) _ci-udeps
