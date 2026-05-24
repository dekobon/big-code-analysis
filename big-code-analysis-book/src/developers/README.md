# Developers Guide

If you want to contribute to the development of `big-code-analysis` we have
summarized here a series of guidelines that are supposed to help you in your
building process.

As prerequisite, you need to install the last available version of `Rust`.
You can learn how to do that
<a href="https://www.rust-lang.org/tools/install" target="_blank">here</a>.

## Clone Repository

First of all, you need to clone the repository.
You can do that:

through **HTTPS**

```console
git clone -j8 https://github.com/dekobon/big-code-analysis.git
```

or through **SSH**

```console
git clone -j8 git@github.com:dekobon/big-code-analysis.git
```

## Make is the canonical entry point

The repository ships a `Makefile` that wraps every common build, test,
lint, format, and docs task. Run `make help` to see the full list of
targets, and `make check-tools` to verify which optional tools
(`taplo`, `markdownlint-cli2`, `shellcheck`, `shfmt`, `checkmake`,
`mdbook`, `cargo-insta`, `cargo-udeps`) are present on your machine.

The two composite targets you will use most:

- `make pre-commit` — the recommended local gate before committing.
  Runs `cargo fmt --check`, both clippy invocations
  (default-features and `--all-features`), `cargo test --workspace
  --all-features` (lib + bin + integration + doc), `cargo +nightly
  udeps`, and the markdown / TOML / shell / Makefile lint families
  in one parallel pass.
- `make ci` — the same checks in the order CI runs them, with no
  auto-fixing. Use this to reproduce a failing CI run locally.

If GNU Make 4 or any of the optional tools are unavailable, fall back
to the raw cargo commands shown below — they are equivalent to the
corresponding Make targets.

## Building

To build the `big-code-analysis` library, the CLI, and the web
server in one shot:

```console
make build           # cargo build --workspace --all-targets
make build-release   # cargo build --workspace --release
```

For an individual crate, invoke `cargo` directly:

```console
cargo build                              # library only
cargo build -p big-code-analysis-cli     # CLI only
cargo build -p big-code-analysis-web     # web server only
```

`make check` runs `cargo check --workspace --all-targets` for fast
type-checking during iteration.

## Testing

To verify that all tests pass:

```console
make test       # cargo test --workspace --all-features --lib --bins --tests
make test-doc   # cargo test --workspace --all-features --doc
```

If you only want to run the cargo command yourself:

```console
cargo test --workspace --all-features --verbose
```

### Updating insta tests

We use [insta](https://insta.rs); install
[cargo insta](https://crates.io/crates/cargo-insta) to manage
snapshots. The Makefile wraps the two operations you need:

```console
make insta-review   # cargo insta test --review (interactive)
make insta-accept   # cargo insta test --accept (use with care)
```

`make insta-review` runs the tests, generates the new snapshot
references, and lets you review each diff. Reach for `make
insta-accept` only for bulk metric-value-only refreshes (grammar
bumps, Halstead operator reclassification) where you have already
verified the diff pattern is uniform.

## Code Formatting

If all previous steps went well, and you want to make a pull request
to integrate your invaluable help in the codebase, the last step left
is code formatting. The `make fmt` target runs every formatter in the
project (Rust, Markdown, TOML, Bash) in one shot; `make fmt-check`
verifies formatting without modifying files.

```console
make fmt         # cargo fmt + markdownlint-cli2 --fix + shfmt -w + taplo fmt
make fmt-check   # the equivalent --check variants
```

### Rustfmt

This tool formats your code according to Rust style guidelines.

To install:

```console
rustup component add rustfmt
```

To format the code (handled automatically by `make fmt`):

```console
cargo fmt
```

### Clippy

This tool helps developers to write better code catching automatically lots of
common mistakes for them. It detects in your code a series of errors and
warnings that **must** be fixed before making a pull request.

`make clippy` runs both clippy invocations the project enforces
(default-features and `--all-features`); `make lint` additionally
runs the markdown, shell, TOML, and Makefile linters.

To install:

```console
rustup component add clippy
```

To detect errors and warnings:

```console
make clippy
# or, manually:
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### Unused dependencies

`make udeps` runs `cargo +nightly udeps --workspace --all-targets` to
catch dependencies declared in `Cargo.toml` but never referenced.
Requires the nightly toolchain (`rustup toolchain install nightly`)
and `cargo-udeps`.

## Code Documentation

```console
make doc        # cargo doc --no-deps --workspace --all-features  (warning-tolerant)
make doc-open   # same, then open in a browser
make doc-check  # strict gate: appends -D warnings to RUSTDOCFLAGS, fails on any rustdoc warning
```

`make doc` and `make doc-open` are the interactive viewers — they
build whatever they can so you can still inspect rendered output
mid-refactor. `make doc-check` is the strict gate that runs as part
of `make pre-commit` and CI (`cargo doc --no-deps --workspace
--all-features` with `RUSTDOCFLAGS` extended by `-D warnings`); it
catches broken intra-doc links, links into private items, and other
rustdoc regressions.

Remove the `--no-deps` option from the underlying cargo invocation if
you also want to build the documentation of each dependency used by
**big-code-analysis**.

### Building this book

The book you are reading lives under `big-code-analysis-book/`:

```console
make book        # mdbook build
make book-serve  # mdbook serve with live reload
```

## Run your code

You can run **bca** using:

```console
cargo run -p big-code-analysis-cli -- [bca-parameters]
```

To know the list of **bca** parameters, run:

```console
cargo run -p big-code-analysis-cli -- --help
```

You can run **bca-web** using:

```console
cargo run -p big-code-analysis-web -- [bca-web-parameters]
```

To know the list of **bca-web** parameters, run:

```console
cargo run -p big-code-analysis-web -- --help
```

`make install`, `make install-cli`, and `make install-web` invoke
`cargo install --path` for the respective binary crates.

## Practical advice

- When you add a new feature, add at least one unit or integration test to
  verify that everything works correctly
- Document public API
- Do not add dead code
- Comment intricate code such that others can comprehend what you have
  accomplished
- Run `make pre-commit` before pushing — it is the same gate CI runs
