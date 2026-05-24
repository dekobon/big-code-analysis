# Installation

The bindings are distributed as a pure-wheel Python package. The
recommended install is via `pip` (or your preferred lockfile
manager — `uv`, `poetry`, `pdm`).

```bash
pip install big-code-analysis
```

Python `>=3.12` is required. The compiled extension uses CPython's
stable [abi3](https://docs.python.org/3/c-api/stable.html) surface
(`abi3-py312`), so one wheel covers `3.12`, `3.13`, and every
future minor release without a per-version wheel build.

## Wheel matrix

CI publishes wheels for the following targets today. If your
platform is not listed, [build from source](#building-from-source).

| Platform | Architectures |
|----------|---------------|
| Linux (`manylinux_2_28`) | `x86_64`, `aarch64` |

The wheel matrix is defined in
[`.github/workflows/python-wheels.yml`](https://github.com/dekobon/big-code-analysis/blob/main/.github/workflows/python-wheels.yml).
[Phase 7](https://github.com/dekobon/big-code-analysis/issues/271)
of the bindings work lit up the `manylinux_2_28` Linux legs.
`manylinux_2_28` requires glibc `>= 2.28` (RHEL 8 / Debian 10 /
Ubuntu 18.10 and newer); older distributions (RHEL 7 / CentOS 7,
glibc 2.17) need to build from source. macOS and Windows wheel
publication is tracked under [#103](https://github.com/dekobon/big-code-analysis/issues/103)
and not yet shipped — `pip install` on those platforms falls back
to a source build today.

## Verifying the install

```bash
python -c "import big_code_analysis as bca; print(bca.__version__)"
```

The version printed equals
[`[workspace.package].version`](https://github.com/dekobon/big-code-analysis/blob/main/Cargo.toml)
from the Rust workspace's `Cargo.toml` — the bindings and the Rust
library version in lockstep.

## Building from source

If no wheel matches your platform, or you want to bind against an
unreleased Rust commit, build with
[maturin](https://www.maturin.rs/):

```bash
git clone https://github.com/dekobon/big-code-analysis.git
cd big-code-analysis/big-code-analysis-py
python -m venv .venv && source .venv/bin/activate
pip install --upgrade pip
pip install "maturin>=1.7,<2.0"
maturin develop --release   # editable install of big_code_analysis
python -c "import big_code_analysis as bca; print(bca.__version__)"
```

`maturin develop` builds the Rust extension in-place and installs it
into the active venv so `import big_code_analysis` resolves
locally — no separate `pip install -e .` step is required. The
`--release` flag turns on the optimiser; omit it during development
for faster rebuilds.

You will also need:

* A stable Rust toolchain (MSRV: `1.94`). Install via
  [rustup](https://rustup.rs/).
* A C compiler (used by the tree-sitter grammar crates).
* CPython development headers (`python3-dev` on Debian / Ubuntu).

## Next

Walk through the [quick-start](quick-start.md) to compute your
first metric, or skip ahead to [batch processing](batch.md) if
you're wiring this into a pipeline over many files.
