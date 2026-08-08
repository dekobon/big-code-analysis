#!/usr/bin/env bash
# Check for required and optional tools used by the Makefile.
#
# `--release-only` probes just the two tools `make release-check`
# invokes and exits non-zero if either is absent; the gate calls it that
# way so a missing tool is named up front instead of surfacing as
# cargo's generic `no such command` partway through (#1226).

set -euo pipefail

usage() {
	echo "usage: check-tools.sh [--release-only]" >&2
	exit 2
}

release_only=0
if [ "$#" -gt 1 ]; then
	usage
elif [ "$#" -eq 1 ]; then
	[ "$1" = "--release-only" ] || usage
	release_only=1
fi

# The exact install commands for the two tools `make release-check`
# cannot run without. cargo-about 0.9.0 moved its binary behind a
# non-default `cli` feature, so a bare `cargo install cargo-about`
# compiles the library, installs no binary, reports the miss as a
# *warning*, and exits 0 — after which `cargo about` still answers "no
# such command" (#1226). Both readers of these strings — the section
# further down and `make release-check` through `--release-only` — take
# them from here, so the `--features cli` spelling cannot go missing
# from one of them.
CARGO_DENY_INSTALL="cargo install --locked cargo-deny"
CARGO_ABOUT_INSTALL="cargo install --locked cargo-about --features cli"

# Probes the release tooling, printing one status line each and setting
# deny_missing / about_missing / release_missing.
#
# `cargo <sub> --version` rather than `command -v cargo-about`: it is
# what the gate itself does, so a binary that is on PATH but which cargo
# cannot dispatch to still reads as missing here.
check_release_tools() {
	deny_missing=0
	if cargo deny --version >/dev/null 2>&1; then
		deny_version=$(cargo deny --version 2>/dev/null | awk 'NR==1{print $2; exit}' || true)
		echo "  ✓ cargo-deny (version: ${deny_version:-unknown})"
	else
		echo "  ✗ cargo-deny (not found)"
		deny_missing=1
	fi

	about_missing=0
	if cargo about --version >/dev/null 2>&1; then
		about_version=$(cargo about --version 2>/dev/null | awk 'NR==1{print $2; exit}' || true)
		echo "  ✓ cargo-about (version: ${about_version:-unknown})"
	else
		echo "  ✗ cargo-about (not found)"
		about_missing=1
	fi

	release_missing=$((deny_missing + about_missing))
}

print_release_hints() {
	if [ "$deny_missing" -eq 1 ]; then
		echo "  - cargo-deny: Install with: $CARGO_DENY_INSTALL"
	fi
	if [ "$about_missing" -eq 1 ]; then
		echo "  - cargo-about: Install with: $CARGO_ABOUT_INSTALL"
		echo "                 (--features cli is not optional: the binary is behind a"
		echo "                  non-default feature, so a bare 'cargo install cargo-about'"
		echo "                  installs nothing and still exits 0)"
	fi
}

if [ "$release_only" -eq 1 ]; then
	echo "Checking release tooling ('make release-check')..."
	check_release_tools
	if [ "$release_missing" -gt 0 ]; then
		echo ""
		echo "Missing release tools:"
		print_release_hints
		echo ""
		echo "'make release-check' invokes 'cargo deny' and 'cargo about' directly"
		echo "and cannot proceed without them."
		exit 1
	fi
	exit 0
fi

echo "Checking required tools..."
echo ""
echo "Core Tools:"

cargo_missing=0
if command -v cargo >/dev/null 2>&1; then
	echo "  ✓ cargo (version: $(cargo --version | cut -d' ' -f2))"
else
	echo "  ✗ cargo (not found)"
	cargo_missing=1
fi

nightly_missing=0
if cargo +nightly --version >/dev/null 2>&1; then
	echo "  ✓ rust nightly (version: $(cargo +nightly --version | cut -d' ' -f2))"
else
	echo "  ✗ rust nightly (not found)"
	nightly_missing=1
fi

udeps_missing=0
if cargo +nightly udeps --version >/dev/null 2>&1; then
	udeps_version=$(cargo +nightly udeps --version 2>/dev/null | awk 'NR==1{print $2; exit}' || true)
	udeps_version=${udeps_version:-unknown}
	echo "  ✓ cargo-udeps (version: $udeps_version)"
else
	echo "  ✗ cargo-udeps (not found)"
	udeps_missing=1
fi

insta_missing=0
if cargo insta --version >/dev/null 2>&1; then
	insta_version=$(cargo insta --version 2>/dev/null | awk 'NR==1{print $2; exit}' || true)
	insta_version=${insta_version:-unknown}
	echo "  ✓ cargo-insta (version: $insta_version)"
else
	echo "  ✗ cargo-insta (not found)"
	insta_missing=1
fi

checkmake_missing=0
if command -v checkmake >/dev/null 2>&1; then
	# checkmake --version: "checkmake v0.3.2 built at ..." when ldflags are
	# applied, or "checkmake  built at ..." when they are not. Scan for the
	# first token that looks like a version rather than assuming position.
	checkmake_version=$(checkmake --version 2>/dev/null \
		| awk '{ for (i = 1; i <= NF; i++) if ($i ~ /^v?[0-9]+\.[0-9]+/) { print $i; exit } }')
	checkmake_version=${checkmake_version:-unknown}
	echo "  ✓ checkmake (version: $checkmake_version)"
else
	echo "  ✗ checkmake (not found)"
	checkmake_missing=1
fi

echo ""
echo "Optional Tools (Markdown linting):"

rumdl_missing=0
if command -v rumdl >/dev/null 2>&1; then
	rumdl_version=$(rumdl --version 2>/dev/null | awk 'NR==1{print $NF; exit}' || true)
	rumdl_version=${rumdl_version:-unknown}
	echo "  ✓ rumdl (version: $rumdl_version)"
else
	echo "  ✗ rumdl (not found)"
	rumdl_missing=1
fi

echo ""
echo "Optional Tools (File search):"

fd_missing=0
if command -v fd >/dev/null 2>&1; then
	echo "  ✓ fd (version: $(fd --version 2>/dev/null | head -1))"
elif command -v fdfind >/dev/null 2>&1; then
	echo "  ✓ fdfind (version: $(fdfind --version 2>/dev/null | head -1))"
else
	echo "  ✗ fd/fdfind (not found)"
	fd_missing=1
fi

echo ""
echo "Optional Tools (TOML formatting/linting):"

taplo_missing=0
if command -v taplo >/dev/null 2>&1; then
	echo "  ✓ taplo (version: $(taplo --version 2>/dev/null | head -1))"
else
	echo "  ✗ taplo (not found)"
	taplo_missing=1
fi

echo ""
echo "Optional Tools (Bash linting/formatting):"

shellcheck_missing=0
if command -v shellcheck >/dev/null 2>&1; then
	echo "  ✓ shellcheck (version: $(shellcheck --version 2>/dev/null | awk '/^version:/{print $2; exit}'))"
else
	echo "  ✗ shellcheck (not found)"
	shellcheck_missing=1
fi

shfmt_missing=0
if command -v shfmt >/dev/null 2>&1; then
	echo "  ✓ shfmt (version: $(shfmt --version 2>/dev/null | head -1))"
else
	echo "  ✗ shfmt (not found)"
	shfmt_missing=1
fi

echo ""
echo "Optional Tools (Test runner):"
echo "  (optional because 'make test' falls back to 'cargo test', which runs"
echo "   the same tests one binary at a time instead of in one global pool)"

nextest_missing=0
if command -v cargo-nextest >/dev/null 2>&1; then
	nextest_version=$(cargo-nextest nextest --version 2>/dev/null | awk 'NR==1{print $2; exit}' || true)
	nextest_version=${nextest_version:-unknown}
	echo "  ✓ cargo-nextest (version: $nextest_version)"
else
	echo "  ✗ cargo-nextest (not found)"
	nextest_missing=1
fi

echo ""
echo "Optional Tools (GitHub Actions linting):"

actionlint_missing=0
if command -v actionlint >/dev/null 2>&1; then
	actionlint_version=$(actionlint -version 2>/dev/null | awk 'NR==1{print $1; exit}' || true)
	actionlint_version=${actionlint_version:-unknown}
	echo "  ✓ actionlint (version: $actionlint_version)"
else
	echo "  ✗ actionlint (not found)"
	actionlint_missing=1
fi

echo ""
echo "Optional Tools (Documentation):"

mdbook_missing=0
if command -v mdbook >/dev/null 2>&1; then
	echo "  ✓ mdbook (version: $(mdbook --version 2>/dev/null | head -1))"
else
	echo "  ✗ mdbook (not found)"
	mdbook_missing=1
fi

echo ""
echo "Optional Tools (Release engineering — 'make release-check'):"
echo "  (optional day to day; 'make release-check' hard-fails without both,"
echo "   so a release cannot be cut until they are installed)"

check_release_tools

echo ""
echo "Optional Tools (Python tooling — big-code-analysis-py):"
echo "  (any work on big-code-analysis-py needs uv + the four py-* tools;"
echo "   skip this section entirely if you only touch Rust)"

uv_missing=0
if command -v uv >/dev/null 2>&1; then
	uv_version=$(uv --version 2>/dev/null | awk '{print $2; exit}' || true)
	uv_version=${uv_version:-unknown}
	echo "  ✓ uv (version: $uv_version)"
else
	echo "  ✗ uv (not found)"
	uv_missing=1
fi

ruff_missing=0
if command -v ruff >/dev/null 2>&1; then
	echo "  ✓ ruff (version: $(ruff --version 2>/dev/null | awk '{print $2; exit}'))"
else
	echo "  ✗ ruff (not found)"
	ruff_missing=1
fi

mypy_missing=0
if command -v mypy >/dev/null 2>&1; then
	echo "  ✓ mypy (version: $(mypy --version 2>/dev/null | awk '{print $2; exit}'))"
else
	echo "  ✗ mypy (not found)"
	mypy_missing=1
fi

pyright_missing=0
if command -v pyright >/dev/null 2>&1; then
	echo "  ✓ pyright (version: $(pyright --version 2>/dev/null | awk '{print $2; exit}'))"
else
	echo "  ✗ pyright (not found)"
	pyright_missing=1
fi

maturin_py_missing=0
if command -v maturin >/dev/null 2>&1; then
	echo "  ✓ maturin (version: $(maturin --version 2>/dev/null | awk '{print $2; exit}'))"
else
	echo "  ✗ maturin (not found)"
	maturin_py_missing=1
fi

echo ""

core_missing=$((cargo_missing + nightly_missing + udeps_missing + insta_missing + checkmake_missing))
optional_missing=$((rumdl_missing + fd_missing + taplo_missing + shellcheck_missing + shfmt_missing + nextest_missing + actionlint_missing + mdbook_missing + release_missing + uv_missing + ruff_missing + mypy_missing + pyright_missing + maturin_py_missing))

if [ "$core_missing" -gt 0 ]; then
	echo "Missing core tools:"
	if [ "$cargo_missing" -eq 1 ]; then
		echo "  - cargo: Install from https://rustup.rs/"
	fi
	if [ "$nightly_missing" -eq 1 ]; then
		echo "  - rust nightly: Install with: rustup toolchain install nightly"
	fi
	if [ "$udeps_missing" -eq 1 ]; then
		echo "  - cargo-udeps: Install with: cargo install --locked cargo-udeps"
	fi
	if [ "$insta_missing" -eq 1 ]; then
		echo "  - cargo-insta: Install with: cargo install --locked cargo-insta"
	fi
	if [ "$checkmake_missing" -eq 1 ]; then
		echo "  - checkmake: Download from https://github.com/checkmake/checkmake/releases"
	fi
	echo ""
	echo "Error: Required core tools are missing. Please install them before continuing."
	exit 1
fi

if [ "$optional_missing" -gt 0 ]; then
	echo "Missing optional tools:"
	if [ "$rumdl_missing" -eq 1 ]; then
		echo "  - rumdl: Install with: mise install rumdl (or 'cargo install rumdl')"
	fi
	if [ "$fd_missing" -eq 1 ]; then
		echo "  - fd: Install with: apt install fd-find (Debian/Ubuntu) or cargo install fd-find"
	fi
	if [ "$taplo_missing" -eq 1 ]; then
		echo "  - taplo: Install with: cargo install taplo-cli --locked --features lsp"
	fi
	if [ "$shellcheck_missing" -eq 1 ]; then
		echo "  - shellcheck: Install with: apt install shellcheck (Debian/Ubuntu) or brew install shellcheck (macOS)"
	fi
	if [ "$shfmt_missing" -eq 1 ]; then
		echo "  - shfmt: Install from https://github.com/mvdan/sh/releases"
	fi
	if [ "$nextest_missing" -eq 1 ]; then
		echo "  - cargo-nextest: Install with: cargo install --locked cargo-nextest"
		echo "                   ('make test' still works without it, just slower)"
	fi
	if [ "$actionlint_missing" -eq 1 ]; then
		echo "  - actionlint: Download from https://github.com/rhysd/actionlint/releases or run 'go install github.com/rhysd/actionlint/cmd/actionlint@latest'"
	fi
	if [ "$mdbook_missing" -eq 1 ]; then
		echo "  - mdbook: Install with: cargo install --locked mdbook (needed for 'make book')"
	fi
	print_release_hints
	if [ "$uv_missing" -eq 1 ]; then
		echo "  - uv: needed for 'make py-bootstrap' (creates .venv from uv.lock)."
		echo "        Install with: curl -LsSf https://astral.sh/uv/install.sh | sh"
		echo "        (alternatives: brew install uv | pipx install uv)"
	fi
	if [ "$ruff_missing" -eq 1 ]; then
		echo "  - ruff: Install with 'make py-bootstrap' (preferred — pinned via uv.lock)"
		echo "          or standalone via 'pipx install ruff'"
	fi
	if [ "$mypy_missing" -eq 1 ]; then
		echo "  - mypy: Install with 'make py-bootstrap' (uses project's strict config + .venv)"
	fi
	if [ "$pyright_missing" -eq 1 ]; then
		echo "  - pyright: Install with 'make py-bootstrap' (uses pyrightconfig.json + .venv)"
	fi
	if [ "$maturin_py_missing" -eq 1 ]; then
		echo "  - maturin: Install with 'make py-bootstrap' (builds the PyO3 extension into .venv)"
	fi
	echo ""
	echo "Warning: Optional tools are missing. Some targets will fail."
	echo "All core tools are available - you can still run most targets."
else
	echo "All tools available!"
fi
