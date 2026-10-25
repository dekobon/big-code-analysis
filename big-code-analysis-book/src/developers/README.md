# Developers Guide

How to build, test, lint and run `big-code-analysis` from a checkout.
It is for contributors; the repository-level rules (stability
contract, validation gates, review conventions) live in `AGENTS.md`
and `CONTRIBUTING.md` at the repository root.

You need a current stable
<a href="https://www.rust-lang.org/tools/install" target="_blank">Rust toolchain</a>;
the minimum supported version is declared in the root `Cargo.toml`.

## Clone Repository

Clone through **HTTPS**

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
targets. Run `make check-tools` to list the optional tools and check
which are installed.

The two composite targets you will use most:

- `make pre-commit`: the recommended local gate before committing.
  It runs formatting, both clippy configurations, tests,
  documentation checks, dependency checks, and the lint families. It
  also runs repository consistency checks, both self-scan tiers, and
  the Python validation stages, in one parallel pass. The membership
  comparison (`make check-test-lang-gates-compare`) requires
  `COMPARE_REF` and runs separately in CI. See `_pc-all` in the
  `Makefile` for the complete stage list. The gate ends with a single
  `BCA_GATE: pass` or `BCA_GATE: fail (…)` line; read the verdict
  from that line and nothing else.
- `make ci`: the same checks in the order CI runs them, with no
  auto-fixing. Use this to reproduce a failing CI run locally.

If GNU Make 4 or any of the optional tools are unavailable, fall back
to the raw cargo commands shown below; they are equivalent to the
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
cargo build -p big-code-analysis-ast     # parse + classification layer only
cargo build -p big-code-analysis-cli     # CLI only
cargo build -p big-code-analysis-web     # web server only
```

`make check` runs `cargo check --workspace --all-targets` for fast
type-checking during iteration.

## Testing

To verify that all tests pass:

```console
make test       # cargo nextest run --workspace --all-features
make test-doc   # cargo test --workspace --all-features --doc
```

`make test` prefers [cargo-nextest](https://nexte.st/), which is also
what CI runs. Plain `cargo test` finishes each test binary before
starting the next, so parallelism exists only within a binary; nextest
schedules every binary's tests into one global pool. Install it with
`cargo install --locked cargo-nextest`.

Without it, `make test` falls back to `cargo test --workspace
--all-features --lib --bins --tests`, which runs the same set of tests.
The two runners are not quite interchangeable, though: nextest runs each
test in its own process, so anything sharing state across tests within a
binary (a `OnceLock` cache, an environment variable, the working
directory) is isolated under nextest and shared under `cargo test`.
Preferring nextest locally means local runs match CI on that axis.

Set `NEXTEST=` to force the fallback without uninstalling anything
(`make test NEXTEST=`), or point it at a specific binary
(`make test NEXTEST=/path/to/cargo-nextest`).

Doctests are a separate target because nextest cannot run them.
`make pre-commit` and `make ci` run both.

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

Format before opening a pull request. The `make fmt` target runs every
formatter in the project (Rust, Markdown, TOML, Bash) in one shot;
`make fmt-check` verifies formatting without modifying files.

```console
make fmt         # cargo fmt + rumdl check --fix + shfmt -w + taplo fmt
make fmt-check   # the equivalent --check variants
```

### Rustfmt

[rustfmt](https://github.com/rust-lang/rustfmt) formats Rust code to
the project style.

To install:

```console
rustup component add rustfmt
```

To format the code (handled automatically by `make fmt`):

```console
cargo fmt
```

### Clippy

[Clippy](https://doc.rust-lang.org/clippy/) catches common mistakes.
The warnings it reports **must** be fixed before making a pull
request.

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
and [`cargo-udeps`](https://crates.io/crates/cargo-udeps).

## Code Documentation

```console
make doc        # cargo doc --no-deps --workspace --all-features  (warning-tolerant)
make doc-open   # same, then open in a browser
make doc-check  # strict gate: appends -D warnings to RUSTDOCFLAGS, fails on any rustdoc warning
```

`make doc` and `make doc-open` are the interactive viewers: they
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

The book you are reading is an [mdBook](https://rust-lang.github.io/mdBook/)
under `big-code-analysis-book/`:

```console
make book        # mdbook build
make book-serve  # mdbook serve with live reload
```

## Run your code

Run `bca`:

```console
cargo run -p big-code-analysis-cli -- [bca-parameters]
```

List its parameters:

```console
cargo run -p big-code-analysis-cli -- --help
```

Run `bca-web`:

```console
cargo run -p big-code-analysis-web -- [bca-web-parameters]
```

List its parameters:

```console
cargo run -p big-code-analysis-web -- --help
```

`make install`, `make install-cli`, and `make install-web` invoke
`cargo install --path` for the respective binary crates.

## Practical advice

- Add at least one unit or integration test with every new feature.
- Document the public API.
- Do not add dead code.
- Comment intricate code so others can follow it.
- Run `make pre-commit` before pushing; it is the same gate CI runs.
