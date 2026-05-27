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

# Path to the Python bindings crate. Absolute path so the recipes
# below resolve correctly even when Make is invoked from a nested
# directory. BASE_DIR ends in a trailing slash (from $(dir ...))
# which absorbs into the concatenation; BCA_PY_DIR has no trailing
# slash and that's intended (matches how cargo / maturin invocations
# read it).
BCA_PY_DIR     := $(BASE_DIR)big-code-analysis-py

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

.PHONY: help check-tools build build-release check test test-doc fmt fmt-check markdown-fmt markdown-lint shellcheck sh-fmt sh-fmt-check toml-fmt toml-fmt-check toml-lint makefile-check actionlint snapshot-anchors check-versions enums-check self-scan self-scan-headroom self-scan-write-baseline self-scan-write-baseline-headroom lint clippy udeps insta-review insta-accept clean install install-cli install-web doc doc-open doc-check book book-serve book-deploy all pre-commit ci release-check verify-changelog pkg-deb-local pkg-rpm-local py-fmt py-fmt-check py-lint py-typecheck py-test _check-find _pc-fmt _pc-clippy _pc-test _pc-doc-check _pc-udeps _pc-shellcheck _pc-markdown-lint _pc-toml-lint _pc-makefile-check _pc-actionlint _pc-snapshot-anchors _pc-check-versions _pc-enums-check _pc-self-scan _pc-self-scan-headroom _pc-py-fmt _pc-py-typecheck _pc-py-test _ci-fmt-check _ci-clippy _ci-test _ci-doc-check _ci-build _ci-udeps _ci-shellcheck _ci-markdown-lint _ci-toml-lint _ci-makefile-check _ci-actionlint _ci-snapshot-anchors _ci-check-versions _ci-enums-check _ci-self-scan _ci-self-scan-headroom _ci-cargo-pipeline _ci-py-fmt-check _ci-py-lint _ci-py-typecheck _ci-py-test

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
	@echo "  actionlint                           Lint GitHub Actions workflows with actionlint"
	@echo "  snapshot-anchors                     Block new bare insta snapshots"
	@echo "  check-versions                       Enforce lockstep version invariant across owned crates"
	@echo "  enums-check                          cargo clippy + cargo test on workspace-excluded enums crate"
	@echo "  self-scan                            bca threshold gate against this repo (hard: 100%)"
	@echo "  self-scan-headroom                   bca threshold gate (soft: BCA_HEADROOM, default 0.95)"
	@echo "  self-scan-write-baseline             Refresh .bca-baseline.toml at the hard thresholds"
	@echo "  self-scan-write-baseline-headroom    Refresh .bca-baseline.toml at the soft thresholds"
	@echo "  lint                                 Run all linters"
	@echo ""
	@echo "Python bindings (big-code-analysis-py):"
	@echo "  py-fmt                               Format Python sources with ruff"
	@echo "  py-fmt-check                         Verify Python formatting"
	@echo "  py-lint                              Lint Python sources with ruff"
	@echo "  py-typecheck                         Type-check with mypy --strict + pyright"
	@echo "  py-test                              maturin develop + pytest (needs active venv)"
	@echo "  (install: 'mise install' or 'pipx install ruff/mypy/pyright/maturin')"
	@echo ""
	@echo "Maintenance:"
	@echo "  clean                                Remove build artifacts"
	@echo "  install                              Install both CLI and web binaries"
	@echo "  install-cli                          Install bca"
	@echo "  install-web                          Install bca-web"
	@echo ""
	@echo "Documentation:"
	@echo "  doc                                  Generate rustdoc (warning-tolerant viewer)"
	@echo "  doc-open                             Generate and open rustdoc (warning-tolerant viewer)"
	@echo "  doc-check                            Strict rustdoc gate (RUSTDOCFLAGS appends -D warnings)"
	@echo "  book                                 Build the mdBook"
	@echo "  book-serve                           Serve the mdBook with live reload"
	@echo "  book-deploy                          Publish the mdBook to the gh-pages branch (manual fallback)"
	@echo ""
	@echo "Combined targets:"
	@echo "  all                                  Check, test, build release"
	@echo "  pre-commit                           Verify formatting, lint, test (recommended before commit)"
	@echo "  ci                                   Validate formatting, lint, test (no auto-fix)"
	@echo ""
	@echo "Release engineering:"
	@echo "  release-check                        Pre-tag gate: deny + about + CHANGELOG (VERSION=x.y.z)"
	@echo "  verify-changelog                     Verify CHANGELOG.md has section for VERSION=x.y.z"
	@echo "  pkg-deb-local                        Build .deb locally (host target, no CI matrix)"
	@echo "  pkg-rpm-local                        Build .rpm locally (host target, no CI matrix)"

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

# actionlint scans every workflow under .github/workflows/ and shells
# out to shellcheck (when present on PATH) for the `run:` blocks. It
# takes no file arguments here: invoked at the repo root it discovers
# .github/workflows/ automatically, which matches the canonical
# upstream invocation and keeps the recipe robust against new
# workflows being added.
actionlint:
	@echo "Linting GitHub Actions workflows with actionlint..."
	@(cd $(BASE_DIR) && actionlint -no-color) || { echo "actionlint found issues"; exit 1; }

snapshot-anchors:
	@echo "Checking insta snapshot anchors..."
	@python3 $(BASE_DIR)check-snapshot-anchors.py

# Regenerate the man pages under `man/` from the live clap schema.
# Auto-fix flavour: `cargo xtask` rewrites every `.1` file so a
# subsequent `git diff --exit-code -- man/` is clean. Used by
# `_pc-manpages` so contributors can stage the regenerated output.
manpages:
	@echo "Regenerating man pages from clap schema..."
	@cargo xtask

# Regenerate-then-assert-clean drift gate. Like the CI `manpage`
# job in `.github/workflows/ci.yml`, this recipe runs `cargo xtask`
# (which rewrites `man/*.1` in place — the same side effect CI
# accepts on its ephemeral runners) and then fails if the resulting
# tree differs from the index. Used by `_ci-manpages` and
# `_pc-manpages`. Locally, contributors with hand-edited `.1` files
# will see those edits overwritten; man pages are generated
# artifacts and should not be hand-edited.
manpages-check:
	@echo "Checking man pages match clap schema..."
	@cargo xtask
	@if ! git diff --exit-code -- man/; then \
	  echo "ERROR: man pages drift from the clap schema. The regenerated files are already in your working tree — run 'git add man/' and commit alongside the clap change."; \
	  exit 1; \
	fi

# Lockstep-version invariant: every owned crate and every internal
# `=<v>` dep pin must equal `[workspace.package].version`. See
# `RELEASING.md` "Lockstep version policy" and `check-versions.py`.
check-versions:
	@echo "Checking lockstep version invariant..."
	@python3 $(BASE_DIR)check-versions.py

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
# Uses `cargo clippy` (not `cargo check`) so the gate enforces the same
# lint floor as the workspace `clippy` job: rustc-level warnings plus the
# clippy default group. The three `manual_is_ascii_check` sites that
# previously blocked this (tracked as #166) have been fixed.
#
# Also runs `cargo test` on the same manifest because the workspace
# `test` job (`cargo test --workspace`) skips this crate by exclusion,
# leaving `enums/tests/dispatch.rs` and any other integration tests
# unexecuted in CI / pre-commit. The dispatch test pins each `Lang`
# variant to its expected backing grammar crate (issue #350); without
# this runtime gate, an arm in `mk_get_language!` pointing at the wrong
# grammar would compile cleanly and only fail when a developer
# manually invoked `cargo test` against the enums manifest.
enums-check:
	@echo "Linting workspace-excluded enums crate..."
	@RUSTFLAGS="-D warnings" cargo clippy \
	  --manifest-path $(BASE_DIR)enums/Cargo.toml \
	  --all-targets --locked -- -D warnings
	@echo "Running tests for workspace-excluded enums crate..."
	@cargo test \
	  --manifest-path $(BASE_DIR)enums/Cargo.toml \
	  --locked

# ---------------------------------------------------------------------------
# bca self-scan threshold gate
#
# Re-runs the CI threshold gate against the in-tree bca binary.
# Mirrors the `Threshold gate (baseline-ratcheted)` step in
# .github/workflows/pages.yml. We build from source on purpose:
# any in-progress change to metric computation is reflected in the
# values we gate on. When pages.yml bumps the pinned release
# binary, the two invocations re-sync.
#
# Two tiers:
#
#   self-scan            hard gate (limits as configured in
#                        bca-thresholds.toml; absorbed by
#                        .bca-baseline.toml). Mirrors CI.
#
#   self-scan-headroom   soft gate. Scales every limit by
#                        BCA_HEADROOM (default 0.95) and runs the
#                        same baseline-ratcheted check, so new
#                        functions encroaching into the 95-100%
#                        band fail before they would trip the hard
#                        gate. Set BCA_HEADROOM=0.90 to widen the
#                        band, 0.99 to tighten it.
#
# Refresh the baseline (after an intentional regression or after
# raising a limit):
#
#   self-scan-write-baseline   refreshes .bca-baseline.toml in place.
#
# NOTE: `--paths .` is conventional but no longer load-bearing.
# Since v3 (issue #376), baseline keys are recorded relative to the
# baseline file's own directory (the anchor) so `--paths .`,
# `--paths $(BASE_DIR)`, and `--paths "$$PWD"` all produce
# byte-identical baselines and match each other on read.
# ---------------------------------------------------------------------------
SELF_SCAN_BCA := cargo run --quiet --release -p big-code-analysis-cli --
SELF_SCAN_BASE_ARGS := --paths . --exclude-from .bcaignore \
  --num-jobs $(shell nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)

self-scan:
	@echo "bca self-scan (hard gate)..."
	@$(SELF_SCAN_BCA) $(SELF_SCAN_BASE_ARGS) \
	  check \
	  --config bca-thresholds.toml \
	  --baseline .bca-baseline.toml

self-scan-headroom:
	@echo "bca self-scan (soft gate, BCA_HEADROOM=$${BCA_HEADROOM:-0.95})..."
	@python3 $(BASE_DIR)utils/bca-self-scan-headroom.py \
	  $(SELF_SCAN_BCA) $(SELF_SCAN_BASE_ARGS)

self-scan-write-baseline:
	@echo "Refreshing .bca-baseline.toml from current offenders..."
	@$(SELF_SCAN_BCA) $(SELF_SCAN_BASE_ARGS) \
	  check \
	  --config bca-thresholds.toml \
	  --write-baseline .bca-baseline.toml

# Refresh `.bca-baseline.toml` against the SOFT thresholds
# (`bca-thresholds.toml` scaled by BCA_HEADROOM, default 0.95).
# Records every current offender at the soft tier — strictly a
# superset of the hard-tier offenders. Use this when launching the
# soft gate or after raising BCA_HEADROOM so the baseline absorbs
# the new headroom band rather than firing on every commit.
self-scan-write-baseline-headroom:
	@echo "Refreshing .bca-baseline.toml from current soft-tier offenders (BCA_HEADROOM=$${BCA_HEADROOM:-0.95})..."
	@BCA_HEADROOM_WRITE_BASELINE=.bca-baseline.toml \
	  python3 $(BASE_DIR)utils/bca-self-scan-headroom.py \
	  $(SELF_SCAN_BCA) $(SELF_SCAN_BASE_ARGS)

# ---------------------------------------------------------------------------
# Python tooling (big-code-analysis-py)
#
# Targets gracefully no-op when the corresponding tool is absent —
# matching how the markdown / TOML lint families behave on a
# barebones host. CI installs all tools, so the skip path never fires
# there.
#
# Tools used:
#   ruff     — lint + format
#   mypy     — type check (strict mode, invoked from the bindings dir)
#   pyright  — type check (strict mode, second opinion)
#   maturin  — build the compiled extension into the active venv
#
# `py-test` requires an active venv that maturin can write the .so
# into. The recipe does NOT create one — if `VIRTUAL_ENV` is not set,
# maturin will fail with a clear error. CI explicitly creates one per
# matrix leg (see `.github/workflows/ci.yml`). Locally,
# `cd big-code-analysis-py && python -m venv .venv && source .venv/bin/activate`
# once.
# ---------------------------------------------------------------------------
py-fmt:
	@if command -v ruff >/dev/null 2>&1; then \
	  echo "Formatting Python sources..."; \
	  ruff format $(BCA_PY_DIR); \
	else echo "ruff not found; skipping py-fmt"; fi

py-fmt-check:
	@if command -v ruff >/dev/null 2>&1; then \
	  echo "Checking Python formatting..."; \
	  ruff format --check $(BCA_PY_DIR) || \
	    { echo "Python files not formatted (run 'make py-fmt')"; exit 1; }; \
	else echo "ruff not found; skipping py-fmt-check"; fi

py-lint:
	@if command -v ruff >/dev/null 2>&1; then \
	  echo "Linting Python sources..."; \
	  ruff check $(BCA_PY_DIR) || { echo "ruff lint found issues"; exit 1; }; \
	else echo "ruff not found; skipping py-lint"; fi

py-typecheck:
	@# Prefer the bindings dir's `.venv/bin/{mypy,pyright}` when
	@# present so the type checker resolves dev-dependencies
	@# (pytest, etc.) declared in `big-code-analysis-py/pyproject.toml`
	@# from the project's documented venv layout. Fall back to the
	@# host's PATH when the venv hasn't been provisioned (CI sets
	@# `VIRTUAL_ENV` and uses PATH-resolved binaries). A pipx-isolated
	@# system `mypy` can't see the bindings dir's pytest stubs.
	@if [ -x $(BCA_PY_DIR)/.venv/bin/mypy ]; then \
	  echo "Type-checking with mypy --strict (venv)..."; \
	  (cd $(BCA_PY_DIR) && .venv/bin/mypy --strict python tests examples) || \
	    { echo "mypy --strict found issues"; exit 1; }; \
	elif command -v mypy >/dev/null 2>&1; then \
	  echo "Type-checking with mypy --strict..."; \
	  (cd $(BCA_PY_DIR) && mypy --strict python tests examples) || \
	    { echo "mypy --strict found issues"; exit 1; }; \
	else echo "mypy not found; skipping mypy stage of py-typecheck"; fi
	@if [ -x $(BCA_PY_DIR)/.venv/bin/pyright ]; then \
	  echo "Type-checking with pyright (strict, venv)..."; \
	  (cd $(BCA_PY_DIR) && .venv/bin/pyright) || \
	    { echo "pyright found issues"; exit 1; }; \
	elif command -v pyright >/dev/null 2>&1; then \
	  echo "Type-checking with pyright (strict)..."; \
	  (cd $(BCA_PY_DIR) && pyright) || \
	    { echo "pyright found issues"; exit 1; }; \
	else echo "pyright not found; skipping pyright stage of py-typecheck"; fi

# Why the pre-build cleanup: maturin 1.13's `develop` plus cargo's
# incremental cache reliably emit a 0-byte .so on the second
# back-to-back invocation when neither sources nor deps changed (the
# wheel-build step truncates target/maturin/libbig_code_analysis_py.so
# before cargo decides "no rebuild needed" and skips the relink). The
# defensive `find ... -delete` forces cargo to relink each time. This
# is roughly free (~50ms) and prevents the failure mode entirely. CI
# does NOT need this guard because each CI job starts from a fresh
# checkout (target/ is restored from cache but the .so is rebuilt on
# every job invocation, not repeated within a single job).
py-test:
	@# Prefer the bindings dir's `.venv/bin/{maturin,python}` over the
	@# host's PATH for the same reason `py-typecheck` does: the venv
	@# has pytest (declared as a dev-dependency in
	@# `big-code-analysis-py/pyproject.toml`), the host's bare Python
	@# typically does not. CI activates the venv explicitly via
	@# `VIRTUAL_ENV` and uses PATH-resolved binaries — both paths
	@# reach the same wheel because `maturin develop` installs into
	@# whichever venv it finds.
	@if [ -x $(BCA_PY_DIR)/.venv/bin/maturin ] && [ -x $(BCA_PY_DIR)/.venv/bin/python ]; then \
	  echo "Building extension + running pytest (venv)..."; \
	  find $(BASE_DIR)target -name 'libbig_code_analysis_py*' -delete 2>/dev/null || true; \
	  (cd $(BCA_PY_DIR) && .venv/bin/maturin develop --quiet && .venv/bin/python -m pytest) || \
	    { echo "py-test failed"; exit 1; }; \
	elif command -v maturin >/dev/null 2>&1; then \
	  echo "Building extension + running pytest..."; \
	  find $(BASE_DIR)target -name 'libbig_code_analysis_py*' -delete 2>/dev/null || true; \
	  (cd $(BCA_PY_DIR) && maturin develop --quiet && python -m pytest) || \
	    { echo "py-test failed"; exit 1; }; \
	else echo "maturin not found; skipping py-test"; fi

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
	  _ci-actionlint _ci-snapshot-anchors _ci-check-versions _ci-enums-check

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
#
# `doc` and `doc-open` are warning-tolerant interactive viewers — they build
# whatever they can so a developer mid-refactor can still scroll the rendered
# output even when an unrelated doc-comment has drifted. `doc-check` is the
# strict gate invoked by the pre-commit and CI pipelines (`_pc-doc-check` /
# `_ci-doc-check`); it appends `-D warnings` to any caller-set `RUSTDOCFLAGS`
# so docs.rs-style invocations (e.g. `RUSTDOCFLAGS="--cfg docsrs"`) still
# compose correctly instead of being clobbered.
# ---------------------------------------------------------------------------
doc:
	cargo doc --no-deps --workspace --all-features

doc-open:
	cargo doc --no-deps --workspace --all-features --open

doc-check:
	@echo "Building rustdoc with -D warnings..."
	@RUSTDOCFLAGS="$${RUSTDOCFLAGS:-} -D warnings" \
	  cargo doc --no-deps --workspace --all-features

book:
	mdbook build big-code-analysis-book

book-serve:
	mdbook serve big-code-analysis-book

# Manual fallback for publishing the book to GitHub Pages. The
# canonical publish path is .github/workflows/book.yml, which fires
# on every push to main; this target exists so contributors can
# republish from a checkout when CI is unavailable.
book-deploy:
	./utils/deploy-book-to-gh-pages.sh

# ---------------------------------------------------------------------------
# Combined workflows
# ---------------------------------------------------------------------------
all: check test build-release

pre-commit:
	$(MAKE) -j --output-sync=target \
	  _pc-test \
	  _pc-shellcheck _pc-markdown-lint _pc-toml-lint _pc-makefile-check \
	  _pc-actionlint _pc-snapshot-anchors _pc-check-versions _pc-enums-check \
	  _pc-manpages \
	  _pc-self-scan _pc-self-scan-headroom \
	  _pc-py-fmt _pc-py-typecheck _pc-py-test
	@echo "Pre-commit checks passed"

ci:
	$(MAKE) _ci-fmt-check
	$(MAKE) -j --output-sync=target \
	  _ci-cargo-pipeline \
	  _ci-shellcheck _ci-markdown-lint _ci-toml-lint _ci-makefile-check \
	  _ci-actionlint _ci-snapshot-anchors _ci-check-versions _ci-enums-check \
	  _ci-py-fmt-check _ci-py-lint _ci-py-typecheck _ci-py-test
	@echo "CI checks passed"

# ---------------------------------------------------------------------------
# Parallel pre-commit DAG
#
# These _pc-* targets express the dependency graph so `make -j` runs
# independent stages concurrently. The `pre-commit` target invokes them
# with `-j --output-sync=target`.
#
# All cargo invocations against the workspace `target/` (clippy, test,
# udeps, py-test's maturin develop) share the package cache and the
# target/.cargo-lock mutex, so they are serialized into one chain.
# Non-cargo checks (lint families, py-fmt's ruff, py-typecheck's
# mypy + pyright) run in parallel with the cargo pipeline, gated only
# on _pc-fmt.
#
# Dependency graph:
#
#   _pc-fmt
#    ├── _pc-clippy → _pc-test → _pc-doc-check → _pc-udeps → _pc-manpages
#    │                → _pc-self-scan → _pc-self-scan-headroom → _pc-py-test
#    ├── _pc-shellcheck
#    ├── _pc-markdown-lint
#    ├── _pc-toml-lint
#    ├── _pc-makefile-check
#    ├── _pc-actionlint
#    ├── _pc-snapshot-anchors
#    ├── _pc-check-versions
#    ├── _pc-enums-check
#    ├── _pc-py-fmt
#    └── _pc-py-typecheck
#
# _pc-self-scan and _pc-self-scan-headroom both invoke
# `cargo run --release -p big-code-analysis-cli`, which holds the
# workspace `target/` lock for the release-profile build, so they
# serialize into the cargo chain after _pc-manpages rather than
# fanning out in parallel. The hard tier runs first so a regression
# is named before the soft tier reports near-limit headroom.
#
# _pc-enums-check runs cargo on `enums/Cargo.toml`, which has its own
# `target/` (the crate is workspace-excluded), so it does NOT share the
# `target/` lock with the workspace cargo chain and is safe to run in
# parallel with _pc-clippy/_pc-test/_pc-udeps.
#
# _pc-py-fmt and _pc-py-typecheck do NOT touch cargo — they invoke
# ruff and mypy/pyright respectively against pre-built sources/stubs
# — so they run in parallel with the cargo pipeline.
#
# _pc-py-test runs `maturin develop` against the workspace target/, so
# it MUST chain after the tail of the cargo lock-holding pipeline
# (currently _pc-self-scan-headroom) rather than fanning out in
# parallel. Fanning out caused implicit serialization via cargo's
# lock anyway and obscured the true wall-clock cost. When a new
# cargo-lock-holding stage is added, extend this chain at the tail
# (and update the dependency graph comment above) — do not parallelise.
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

_pc-doc-check: _pc-test
	$(MAKE) doc-check

_pc-udeps: _pc-doc-check
	cargo +nightly udeps --workspace --all-targets

_pc-shellcheck: _pc-fmt
	$(MAKE) shellcheck

_pc-markdown-lint: _pc-fmt
	$(MAKE) markdown-lint

_pc-toml-lint: _pc-fmt
	$(MAKE) toml-lint

_pc-makefile-check: _pc-fmt
	$(MAKE) makefile-check

_pc-actionlint: _pc-fmt
	$(MAKE) actionlint

_pc-snapshot-anchors: _pc-fmt
	$(MAKE) snapshot-anchors

_pc-check-versions: _pc-fmt
	$(MAKE) check-versions

_pc-enums-check: _pc-fmt
	$(MAKE) enums-check

# Man-page drift gate. Uses the verify flavour (`manpages-check`,
# not `manpages`) so `make pre-commit` exits non-zero when man
# pages drift — matching CI semantics. Chains after `_pc-udeps`
# rather than `_pc-fmt` because `cargo xtask` shares the workspace
# `target/` lock with the rest of the cargo pipeline; explicit
# serialization is clearer (and faster) than letting cargo's lock
# implicitly serialize parallel arms.
_pc-manpages: _pc-udeps
	$(MAKE) manpages-check

# bca self-scan tiers. Both build with `cargo run --release` against
# the workspace target/, so they chain after _pc-manpages (the
# tail of the workspace cargo chain) rather than fanning out in
# parallel. The hard gate runs before the soft gate so a regression
# beyond the configured limit surfaces first.
_pc-self-scan: _pc-manpages
	$(MAKE) self-scan

_pc-self-scan-headroom: _pc-self-scan
	$(MAKE) self-scan-headroom

# Python pre-commit stages. _pc-py-fmt auto-fixes (ruff format +
# ruff check --fix); the typecheck and test stages are check-only
# (they cannot reasonably auto-fix). _pc-py-fmt and _pc-py-typecheck
# gate on _pc-fmt — they do not touch cargo so they parallelise
# safely with the clippy/test chain. _pc-py-test runs `maturin
# develop` against the workspace target/, so it must chain after
# _pc-udeps to avoid lock contention with the cargo pipeline.
_pc-py-fmt: _pc-fmt
	@if command -v ruff >/dev/null 2>&1; then \
	  ruff format $(BCA_PY_DIR) && ruff check --fix $(BCA_PY_DIR); \
	else echo "ruff not found; skipping _pc-py-fmt"; fi

_pc-py-typecheck: _pc-fmt
	$(MAKE) py-typecheck

_pc-py-test: _pc-self-scan-headroom
	$(MAKE) py-test

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
#      _ci-cargo-pipeline: clippy → test → build → doc-check → udeps
#                          → manpages → self-scan → self-scan-headroom
#      _ci-shellcheck, _ci-markdown-lint, _ci-toml-lint, _ci-makefile-check,
#      _ci-actionlint, _ci-snapshot-anchors, _ci-check-versions,
#      _ci-enums-check
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

_ci-doc-check:
	$(MAKE) doc-check

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

_ci-actionlint:
	$(MAKE) actionlint

_ci-snapshot-anchors:
	$(MAKE) snapshot-anchors

_ci-check-versions:
	$(MAKE) check-versions

_ci-enums-check:
	$(MAKE) enums-check

# Check-only man-page drift gate. Mirrors `.github/workflows/ci.yml`'s
# `manpage` job so `make ci` produces the same verdict as the CI
# workflow on the same tree state.
_ci-manpages:
	$(MAKE) manpages-check

# bca self-scan tiers under `make ci`. Same shape as the _pc-
# counterparts; chained into _ci-cargo-pipeline (not parallelised)
# because both run `cargo run --release` against the shared
# workspace target/. Mirrors `.github/workflows/pages.yml`'s
# `Threshold gate` step.
_ci-self-scan:
	$(MAKE) self-scan

_ci-self-scan-headroom:
	$(MAKE) self-scan-headroom

# Python CI stages — check-only versions of the py-* targets.
# Mirror the _pc-py-* shape but without the auto-fix path.
_ci-py-fmt-check:
	$(MAKE) py-fmt-check

_ci-py-lint:
	$(MAKE) py-lint

_ci-py-typecheck:
	$(MAKE) py-typecheck

_ci-py-test:
	$(MAKE) py-test

# Sequential cargo pipeline for local `make ci`. Every step here
# touches the workspace `target/` lock, so they are serialized in
# this single chain rather than fanned out in parallel.
# _ci-manpages (`cargo xtask`), _ci-self-scan, and
# _ci-self-scan-headroom all share the same target/, so they are
# chained at the tail of this pipeline rather than fanned out as
# parallel arms of `ci:` — parallel scheduling would just block on
# cargo's lock anyway. When adding a new cargo-target-touching
# step, extend the tail here and also extend the
# `_pc-self-scan-headroom → _pc-py-test` chain on the pre-commit
# side.
_ci-cargo-pipeline:
	$(MAKE) _ci-clippy
	$(MAKE) _ci-test
	$(MAKE) _ci-build
	$(MAKE) _ci-doc-check
	$(MAKE) _ci-udeps
	$(MAKE) _ci-manpages
	$(MAKE) _ci-self-scan
	$(MAKE) _ci-self-scan-headroom

# ---------------------------------------------------------------------------
# Release engineering
#
# These targets mirror the gates `.github/workflows/release.yml` runs at
# tag time. Run `make release-check VERSION=x.y.z` before pushing a tag —
# it surfaces deny/about/CHANGELOG drift locally instead of letting CI
# fail mid-release. None of the targets here actually publishes or
# uploads anything; the release workflow is the only mutator.
# ---------------------------------------------------------------------------

# verify-changelog: confirm CHANGELOG.md has a `## [VERSION]` section.
# Fixed-string match so dots in the version aren't treated as regex
# wildcards. Mirrors the preflight check in release.yml.
verify-changelog:
	@if [ -z "$(VERSION)" ]; then \
	  echo "ERROR: VERSION not set. Usage: make verify-changelog VERSION=0.1.0"; \
	  exit 1; \
	fi
	@if ! grep -Fq "## [$(VERSION)]" CHANGELOG.md; then \
	  echo "ERROR: CHANGELOG.md has no section for [$(VERSION)]"; \
	  exit 1; \
	fi
	@echo "CHANGELOG.md contains section for [$(VERSION)]"

# release-check: full pre-tag gate. cargo-deny enforces the license /
# advisory / source allowlists; cargo-about's dry-run renders the
# per-binary THIRD-PARTY-LICENSES files the release archives ship;
# the publish dry-runs catch metadata regressions (missing
# description/license/readme, deny.toml violations, version drift)
# before any external publish fires. The five vendored grammar
# leaves dry-run unconditionally. The parent dry-run mirrors CI's
# preflight bootstrap probe (see `.github/workflows/release.yml`):
# we query the sparse index for `bca-tree-sitter-ccomment` at the
# workspace-pinned version and only run the parent dry-run if that
# leaf is already on crates.io. On the very first release the probe
# returns "not on registry", the parent dry-run is skipped with an
# explanatory note, and CI handles the bootstrap end-to-end.
release-check:
	@if [ -z "$(VERSION)" ]; then \
	  echo "ERROR: VERSION not set. Usage: make release-check VERSION=0.1.0"; \
	  exit 1; \
	fi
	@echo "Running cargo deny check..."
	@cargo deny check
	@echo "Generating THIRD-PARTY-LICENSES-bca.md (dry-run)..."
	@cargo about generate --locked \
	  --config about.toml \
	  --manifest-path big-code-analysis-cli/Cargo.toml \
	  about.hbs > /dev/null
	@echo "Generating THIRD-PARTY-LICENSES-bca-web.md (dry-run)..."
	@cargo about generate --locked \
	  --config about.toml \
	  --manifest-path big-code-analysis-web/Cargo.toml \
	  about.hbs > /dev/null
	@echo "Dry-running cargo publish for the five vendored grammar leaves..."
	@for d in tree-sitter-ccomment tree-sitter-mozcpp tree-sitter-mozjs tree-sitter-preproc tree-sitter-tcl; do \
	  cargo publish --dry-run --locked --manifest-path "$$d/Cargo.toml" || exit 1; \
	done
	@LEAF_VERSION=$$(awk -F'"' \
	  '/^\[package\]/{f=1; next} /^\[/{f=0} f && /^version *=/ {print $$2; exit}' \
	  tree-sitter-ccomment/Cargo.toml); \
	BODY=$$(curl -sfL "https://index.crates.io/bc/a-/bca-tree-sitter-ccomment" 2>/dev/null || true); \
	if [ -n "$$BODY" ] && echo "$$BODY" | grep -q "\"vers\":\"$$LEAF_VERSION\""; then \
	  echo "Dry-running cargo publish for big-code-analysis..."; \
	  cargo publish -p big-code-analysis --dry-run --locked; \
	else \
	  echo "Skipping big-code-analysis dry-run: bca-tree-sitter-ccomment $$LEAF_VERSION not yet on crates.io"; \
	  echo "(bootstrap state — CI will publish the leaves before the parent on the next release tag)."; \
	fi
	@$(MAKE) verify-changelog VERSION=$(VERSION)
	@echo "release-check passed for $(VERSION)"

# Local cargo-deb invocation. Builds the binary first because the
# release workflow's `--no-build` path requires the cross/runner layout
# to be staged — this target is just for smoke-testing the metadata
# block locally on the host triple.
pkg-deb-local:
	cargo build --release -p big-code-analysis-cli -p big-code-analysis-web
	mkdir -p out
	cargo deb -p big-code-analysis-cli --no-build --output out/
	cargo deb -p big-code-analysis-web --no-build --output out/
	@ls -lh out/*.deb

pkg-rpm-local:
	cargo build --release -p big-code-analysis-cli -p big-code-analysis-web
	mkdir -p out
	cargo generate-rpm -p big-code-analysis-cli --payload-compress zstd --output out/
	cargo generate-rpm -p big-code-analysis-web --payload-compress zstd --output out/
	@ls -lh out/*.rpm
