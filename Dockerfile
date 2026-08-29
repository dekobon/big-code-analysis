# Dev container for big-code-analysis.
#
# Ships every runtime and tool needed to (a) run Claude Code plus the
# project's MCP servers (Context7, codegraph, Serena) and (b) run the full
# `make pre-commit` / `make ci` validation gate against a mounted checkout:
# Rust (stable 1.94 + nightly for udeps), Python 3.12 (+ uv/maturin), Node
# (for the npx-launched MCP servers), and the pinned lint tools.
#
# Tool versions are pinned to the same releases CI uses (see
# .github/workflows/ci.yml and mise.toml) so behaviour matches CI. Every
# downloaded binary is SHA256-verified, mirroring the project's
# pinned-dependency discipline.
#
# The three MCP servers (Context7, codegraph, Serena) are registered into
# Claude Code's user scope at build time from docker/mcp.json, so `claude`
# launches them as stdio subprocesses with no extra flags — no
# docker-compose, no separate server containers.
#
# Build/run via the Makefile: `make dev-env-build`, `make dev-env-run`,
# `make dev-env-shell` (see docker/README.md). The Makefile passes the
# host UID/GID as build args so the mounted repo stays writable.

# Digest-pinned (OpenSSF Scorecard Pinned-Dependencies); Dependabot's
# `docker` entry in .github/dependabot.yml moves the digest.
FROM ubuntu:noble@sha256:33ceb71981b602c1a7443a53469e4dba065f7503eab3078a2d7a57a2ab987517

# UID/GID for the in-container `dev` user. Defaults match the maintainer's
# host (2424); `make dev-env-build` overrides them with the caller's
# `id -u` / `id -g` so bind-mounted files are owned correctly.
ARG USER_UID=2424
ARG USER_GID=2424

ENV DEBIAN_FRONTEND=noninteractive
ENV RUSTUP_HOME=/usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo
# uv installs its managed tools (ruff/mypy/maturin/serena) into a system
# location with shims on the system PATH, so they survive the /home/dev
# volume mount at runtime rather than being shadowed by it.
ENV UV_TOOL_DIR=/opt/uv/tools
ENV UV_TOOL_BIN_DIR=/usr/local/bin
ENV PATH=/usr/local/cargo/bin:/home/dev/.local/bin:/usr/local/bin:$PATH

# Toolchain / release pins. Keep these in lockstep with the sources noted:
#   RUST_VERSION       -> Cargo.toml workspace.package.rust-version
#   NODE_MAJOR         -> NodeSource line (>= 22.6 required by codegraph)
#   RUMDL/…/ACTIONLINT -> .github/workflows/ci.yml lint job
#   BINSTALL/UV        -> no CI counterpart; bump by hand together with
#                         the per-arch SHA256 tables in their RUN blocks
ENV RUST_VERSION=1.94.0
ENV NODE_MAJOR=24
ENV RUMDL_VERSION=0.2.2
ENV TAPLO_VERSION=0.10.0
ENV SHFMT_VERSION=3.12.0
ENV SHELLCHECK_VERSION=0.10.0
ENV CHECKMAKE_VERSION=0.2.2
ENV ACTIONLINT_VERSION=1.7.12
ENV BINSTALL_VERSION=1.22.0
ENV UV_VERSION=0.12.7

# ---------------------------------------------------------------------------
# APT repositories: GitHub CLI + NodeSource. Keyrings are fetched and
# dearmored explicitly rather than piped through apt-key (deprecated).
# ---------------------------------------------------------------------------
RUN apt-get update --quiet --quiet && \
    apt-get install --yes --no-install-recommends \
        ca-certificates curl gnupg wget && \
    rm -rf /var/lib/apt/lists/*

# Optional corporate/proxy root CAs. Drop PEM `.crt` files into docker/certs/
# to build behind a TLS-intercepting proxy (Netskope, Zscaler, corporate
# MITM, …); the directory ships empty so nothing environment-specific is
# committed. Certs are added to the system trust store here — before any
# HTTPS fetch below — and the env vars point every toolchain that keeps its
# own CA store (Node, Python, Cargo, git) at the same bundle, so an added CA
# is honored everywhere. With an empty docker/certs/ this is a no-op.
COPY docker/certs/ /usr/local/share/ca-certificates/corp-extra/
RUN update-ca-certificates
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    NODE_EXTRA_CA_CERTS=/etc/ssl/certs/ca-certificates.crt \
    REQUESTS_CA_BUNDLE=/etc/ssl/certs/ca-certificates.crt \
    CARGO_HTTP_CAINFO=/etc/ssl/certs/ca-certificates.crt \
    GIT_SSL_CAINFO=/etc/ssl/certs/ca-certificates.crt

RUN mkdir --parents --mode 0755 /etc/apt/keyrings && \
    wget --quiet --output-document=- https://cli.github.com/packages/githubcli-archive-keyring.gpg \
        > /etc/apt/keyrings/githubcli-archive-keyring.gpg && \
    chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg && \
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
        > /etc/apt/sources.list.d/github-cli.list && \
    wget --quiet --output-document=- https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
        | gpg --dearmor --output /usr/share/keyrings/nodesource.gpg && \
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/nodesource.gpg] https://deb.nodesource.com/node_${NODE_MAJOR}.x nodistro main" \
        > /etc/apt/sources.list.d/nodesource.list

# ---------------------------------------------------------------------------
# Base packages: build toolchain (Rust cc / tree-sitter grammars, node-gyp
# for codegraph's better-sqlite3), Python 3.12 headers (PyO3/maturin),
# C/C++ LSP + language servers' native deps, and the search/CLI tools the
# project's conventions require (ripgrep, fd-find).
# ---------------------------------------------------------------------------
RUN apt-get update --quiet --quiet && \
    apt-get upgrade --quiet --quiet --yes && \
    apt-get install --yes --no-install-recommends \
        gh \
        git \
        build-essential \
        pkg-config \
        libssl-dev \
        clang \
        clangd \
        nodejs \
        python3 \
        python3-dev \
        python3-venv \
        python3-pip \
        patchelf \
        ripgrep \
        fd-find \
        jq \
        less \
        vim \
        tree \
        unzip \
        procps \
        sudo \
        xz-utils && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/* /tmp/* /var/tmp/*

# ---------------------------------------------------------------------------
# Rust: stable (pinned) + nightly (for `cargo +nightly udeps`), with the
# components the gate needs. rust-analyzer doubles as the Rust LSP.
# ---------------------------------------------------------------------------
RUN set -eux; \
    wget --https-only --output-document=/tmp/rustup-init https://sh.rustup.rs; \
    chmod +x /tmp/rustup-init; \
    /tmp/rustup-init -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION"; \
    rm /tmp/rustup-init; \
    rustup component add rustfmt clippy rust-analyzer llvm-tools-preview; \
    rustup toolchain install nightly --profile minimal; \
    rustup default "$RUST_VERSION"

# ---------------------------------------------------------------------------
# cargo-binstall — SHA256-verified release tarball rather than the
# upstream `curl | bash` installer, which resolves "latest" at build
# time and cannot be pinned. Upstream publishes no `.sha256` asset, so
# to bump: download both tarballs and `sha256sum` them.
# ---------------------------------------------------------------------------
RUN <<'EOF'
set -eux
ARCH=$(uname -m)
case "$ARCH" in
    x86_64)  SHA256="df2921254924066627685ee09f6c8b9acd7acef68a338668f879d2b033c09458" ;;
    aarch64) SHA256="2fdf0ad20ffc7c7f94f32c19566f417d227484ff0335664d5df429e459d1354b" ;;
    *)       echo "Unsupported architecture: $ARCH" && exit 1 ;;
esac
wget --quiet -O /tmp/cargo-binstall.tgz "https://github.com/cargo-bins/cargo-binstall/releases/download/v${BINSTALL_VERSION}/cargo-binstall-${ARCH}-unknown-linux-musl.tgz"
echo "${SHA256}  /tmp/cargo-binstall.tgz" | sha256sum --check
tar -xzf /tmp/cargo-binstall.tgz -C /tmp cargo-binstall
install -m 0755 /tmp/cargo-binstall "$CARGO_HOME/bin/cargo-binstall"
rm -f /tmp/cargo-binstall.tgz /tmp/cargo-binstall
cargo binstall --version
EOF

# Cargo dev/CI tools via cargo-binstall (prebuilt binaries — no long
# compiles). cargo-nextest / cargo-llvm-cov mirror CI; cargo-about /
# cargo-deny back `make release-check`; mdbook backs `make book`.
#
# The `cargo about --version` line is a guard, not a version print.
# binstall's last-resort strategy is a plain `cargo install`, and
# cargo-about keeps its binary behind a non-default `cli` feature — so
# that fallback would install no binary, warn, and exit 0, baking a
# release image that cannot run `make release-check` (#1226).
#
# Every tool is version-pinned so a rebuild is reproducible. cargo-about
# tracks CARGO_ABOUT_VERSION in release.yml / python-cli-wheels.yml and
# the literal `cargo-about@…` in ci.yml's release-check job; mdbook
# tracks MDBOOK_VERSION in pages.yml (0.4.x is required by the
# mdbook-i18n-helpers pairing). No gate compares these copies, so a
# bump is a grep for the old version. The rest have no CI pin to
# mirror (ci.yml installs cargo-nextest / cargo-llvm-cov unpinned).
RUN set -eux; \
    cargo binstall --no-confirm \
        cargo-udeps@0.1.61 \
        cargo-insta@1.48.0 \
        cargo-nextest@0.9.143 \
        cargo-llvm-cov@0.9.0 \
        cargo-about@0.8.4 \
        cargo-deny@0.20.2 \
        mdbook@0.4.40; \
    cargo about --version; \
    rm -rf "$CARGO_HOME/registry" "$CARGO_HOME/git" /tmp/*

# ---------------------------------------------------------------------------
# taplo (TOML formatter/linter + LSP) — SHA256-verified release binary.
# ---------------------------------------------------------------------------
RUN <<'EOF'
set -eux
ARCH=$(uname -m)
case "$ARCH" in
    x86_64)  SHA256="8fe196b894ccf9072f98d4e1013a180306e17d244830b03986ee5e8eabeb6156" ;;
    aarch64) SHA256="033681d01eec8376c3fd38fa3703c79316f5e14bb013d859943b60a07bccdcc3" ;;
    *)       echo "Unsupported architecture: $ARCH" && exit 1 ;;
esac
wget --quiet -O /tmp/taplo.gz "https://github.com/tamasfe/taplo/releases/download/${TAPLO_VERSION}/taplo-linux-${ARCH}.gz"
echo "${SHA256}  /tmp/taplo.gz" | sha256sum --check
gunzip /tmp/taplo.gz
install -m 0755 /tmp/taplo /usr/local/bin/taplo
rm -f /tmp/taplo
taplo --version
EOF

# ---------------------------------------------------------------------------
# shfmt (shell formatter) — SHA256-verified release binary.
# ---------------------------------------------------------------------------
RUN <<'EOF'
set -eux
case $(dpkg --print-architecture) in
    amd64) SHFMT_ARCH="amd64"; SHA256="d9fbb2a9c33d13f47e7618cf362a914d029d02a6df124064fff04fd688a745ea" ;;
    arm64) SHFMT_ARCH="arm64"; SHA256="5f3fe3fa6a9f766e6a182ba79a94bef8afedafc57db0b1ad32b0f67fae971ba4" ;;
    *)     echo "Unsupported architecture: $(dpkg --print-architecture)" && exit 1 ;;
esac
wget --quiet -O /tmp/shfmt "https://github.com/mvdan/sh/releases/download/v${SHFMT_VERSION}/shfmt_v${SHFMT_VERSION}_linux_${SHFMT_ARCH}"
echo "${SHA256}  /tmp/shfmt" | sha256sum --check
install -m 0755 /tmp/shfmt /usr/local/bin/shfmt
rm -f /tmp/shfmt
shfmt --version
EOF

# ---------------------------------------------------------------------------
# shellcheck (shell linter) — SHA256-verified release tarball.
# ---------------------------------------------------------------------------
RUN <<'EOF'
set -eux
ARCH=$(uname -m)
case "$ARCH" in
    x86_64)  SHA256="6c881ab0698e4e6ea235245f22832860544f17ba386442fe7e9d629f8cbedf87" ;;
    aarch64) SHA256="324a7e89de8fa2aed0d0c28f3dab59cf84c6d74264022c00c22af665ed1a09bb" ;;
    *)       echo "Unsupported architecture: $ARCH" && exit 1 ;;
esac
wget --quiet -O /tmp/shellcheck.tar.xz "https://github.com/koalaman/shellcheck/releases/download/v${SHELLCHECK_VERSION}/shellcheck-v${SHELLCHECK_VERSION}.linux.${ARCH}.tar.xz"
echo "${SHA256}  /tmp/shellcheck.tar.xz" | sha256sum --check
tar -xJf /tmp/shellcheck.tar.xz -C /tmp
install -m 0755 "/tmp/shellcheck-v${SHELLCHECK_VERSION}/shellcheck" /usr/local/bin/shellcheck
rm -rf /tmp/shellcheck.tar.xz "/tmp/shellcheck-v${SHELLCHECK_VERSION}"
shellcheck --version
EOF

# ---------------------------------------------------------------------------
# rumdl (Markdown linter/formatter) — SHA256-verified release tarball.
# ---------------------------------------------------------------------------
RUN <<'EOF'
set -eux
case $(dpkg --print-architecture) in
    amd64) TRIPLE="x86_64-unknown-linux-gnu";  SHA256="d38ad81c51221990d5e0204b4746f8c980a77de235aa875c2f30f02f9a19bb1a" ;;
    arm64) TRIPLE="aarch64-unknown-linux-gnu"; SHA256="f64e74228bfd25ef83a7b8f7da44911d5f3c4b69ecfa6482c7f06de1e210255d" ;;
    *)     echo "Unsupported architecture: $(dpkg --print-architecture)" && exit 1 ;;
esac
wget --quiet -O /tmp/rumdl.tgz "https://github.com/rvben/rumdl/releases/download/v${RUMDL_VERSION}/rumdl-v${RUMDL_VERSION}-${TRIPLE}.tar.gz"
echo "${SHA256}  /tmp/rumdl.tgz" | sha256sum --check
tar -xzf /tmp/rumdl.tgz -C /tmp
install -m 0755 /tmp/rumdl /usr/local/bin/rumdl
rm -f /tmp/rumdl.tgz /tmp/rumdl
rumdl --version
EOF

# ---------------------------------------------------------------------------
# actionlint (GitHub Actions linter) — SHA256-verified release tarball.
# ---------------------------------------------------------------------------
RUN <<'EOF'
set -eux
case $(dpkg --print-architecture) in
    amd64) ACTIONLINT_ARCH="amd64"; SHA256="8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8" ;;
    arm64) ACTIONLINT_ARCH="arm64"; SHA256="325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6" ;;
    *)     echo "Unsupported architecture: $(dpkg --print-architecture)" && exit 1 ;;
esac
wget --quiet -O /tmp/actionlint.tgz "https://github.com/rhysd/actionlint/releases/download/v${ACTIONLINT_VERSION}/actionlint_${ACTIONLINT_VERSION}_linux_${ACTIONLINT_ARCH}.tar.gz"
echo "${SHA256}  /tmp/actionlint.tgz" | sha256sum --check
tar -xzf /tmp/actionlint.tgz -C /tmp actionlint
install -m 0755 /tmp/actionlint /usr/local/bin/actionlint
rm -f /tmp/actionlint.tgz /tmp/actionlint
actionlint -version
EOF

# ---------------------------------------------------------------------------
# checkmake (Makefile linter) — SHA256-verified release binary. amd64 uses
# the exact CI pin (mrtazz/checkmake 0.2.2); arm64 uses the checkmake/
# checkmake fork (see CHECKMAKE_FORK_VERSION note above). Any other arch
# continues without it (`make makefile-check` becomes the one missing gate).
# ---------------------------------------------------------------------------
RUN <<'EOF'
set -eux
# mrtazz/checkmake 0.2.2 (the CI pin) ships an amd64 Linux binary only. On
# arm64, fall back to the maintained checkmake/checkmake fork (v0.3.2), which
# publishes arm64 builds and uses the same rules + .checkmake.ini format.
CHECKMAKE_FORK_VERSION="0.3.2"
case $(dpkg --print-architecture) in
    amd64)
        wget --quiet -O /tmp/checkmake "https://github.com/mrtazz/checkmake/releases/download/${CHECKMAKE_VERSION}/checkmake-${CHECKMAKE_VERSION}.linux.amd64"
        echo "bedd033b06f2563809855ec2a9950c7a81acea6cd82937fd2f124e2c1c5fc3d5  /tmp/checkmake" | sha256sum --check
        ;;
    arm64)
        wget --quiet -O /tmp/checkmake "https://github.com/checkmake/checkmake/releases/download/v${CHECKMAKE_FORK_VERSION}/checkmake-v${CHECKMAKE_FORK_VERSION}.linux.arm64"
        echo "409167c4abb99407bd232c3bbd351b8a39df57997feafde5a08bddffb0f2dcb4  /tmp/checkmake" | sha256sum --check
        ;;
    *)
        echo "WARNING: no checkmake build for $(dpkg --print-architecture); skipping (make makefile-check will be unavailable)"
        ;;
esac
if [ -f /tmp/checkmake ]; then
    install -m 0755 /tmp/checkmake /usr/local/bin/checkmake
    rm -f /tmp/checkmake
    checkmake --version
fi
EOF

# ---------------------------------------------------------------------------
# Python tooling. uv is the project's canonical bootstrap (`make
# py-bootstrap` => `uv sync --locked`). ruff/mypy/maturin install as uv
# tools into the system location (see UV_TOOL_* above); pyright ships as a
# Node package so it uses the system Node instead of downloading its own.
#
# uv itself is pinned (UV_VERSION + SHA256, from upstream's `.sha256`
# release assets) because it is a downloaded binary. The tools it
# installs — ruff, mypy, maturin — are deliberately unversioned, ruff
# included, even though this repository holds the ruff version in
# lockstep across four files (#1230). The image is built without the
# repository checked out, so it cannot read uv.lock; a literal version
# here would be a fifth copy to bump on every release, in a file no
# gate reads. `make py-fmt` / `py-lint` / `py-typecheck` prefer the
# binaries under `big-code-analysis-py/.venv/bin/`, so once a
# contributor runs `make py-bootstrap` inside the container the locked
# ruff / mypy / pyright win over the ones installed here (the npm
# pyright below is for editors and MCP servers, not the gate).
# ---------------------------------------------------------------------------
RUN <<'EOF'
set -eux
ARCH=$(uname -m)
case "$ARCH" in
    x86_64)  SHA256="788f18abea7c5f55d6216e4f5613fd89d4d59b631efeec117b2b07fe72f1da21" ;;
    aarch64) SHA256="66393193038dd7eb108abd7a218d9cec04ac70ab98242b0720fa94de19223b7c" ;;
    *)       echo "Unsupported architecture: $ARCH" && exit 1 ;;
esac
wget --quiet -O /tmp/uv.tgz "https://github.com/astral-sh/uv/releases/download/${UV_VERSION}/uv-${ARCH}-unknown-linux-gnu.tar.gz"
echo "${SHA256}  /tmp/uv.tgz" | sha256sum --check
tar -xzf /tmp/uv.tgz -C /tmp
install -m 0755 "/tmp/uv-${ARCH}-unknown-linux-gnu/uv" "/tmp/uv-${ARCH}-unknown-linux-gnu/uvx" /usr/local/bin/
rm -rf /tmp/uv.tgz "/tmp/uv-${ARCH}-unknown-linux-gnu"
uv --version
EOF

RUN set -eux; \
    mkdir -p "$UV_TOOL_DIR"; \
    uv tool install --python 3.12 ruff; \
    uv tool install --python 3.12 mypy; \
    uv tool install --python 3.12 maturin; \
    rm -rf /root/.cache

# ---------------------------------------------------------------------------
# Node global packages, installed to the SYSTEM prefix (/usr) so they
# survive the /home/dev volume: Claude Code, the two npx-launched MCP
# servers (codegraph, Context7), plus the Python/Bash language servers.
# codegraph builds better-sqlite3 natively here (build-essential + python3).
#
# Versions are pinned exactly so this layer is reproducible (the image
# as a whole is not: nightly rustup, the uv-installed Python tools, and
# the git-sourced Serena below float by design). A global install has
# no lockfile, so this is as far as npm pinning goes (Scorecard's rule
# wants `npm ci`, which cannot apply here).
# ---------------------------------------------------------------------------
RUN set -eux; \
    npm install -g \
        @anthropic-ai/claude-code@2.1.251 \
        @optave/codegraph@3.17.0 \
        @upstash/context7-mcp@4.0.4 \
        pyright@1.1.413 \
        bash-language-server@5.6.0; \
    npm cache clean --force; \
    rm -rf /root/.npm /tmp/*

# Serena MCP server (LSP-backed code intelligence), pre-installed as a uv
# tool so it launches offline. Exposes the `serena` console script.
RUN set -eux; \
    uv tool install --python 3.12 git+https://github.com/oraios/serena; \
    rm -rf /root/.cache

# ---------------------------------------------------------------------------
# Non-root dev user. UID/GID come from build args so bind-mounted repo
# files are owned by the caller. ubuntu:noble ships a default `ubuntu`
# user/group at UID/GID 1000, so remove it first — otherwise a host UID/GID
# of 1000 (the common Linux default) collides and useradd fails. dev owns
# the Rust dirs so it can add toolchains/components/crates at runtime.
# ---------------------------------------------------------------------------
RUN set -eux; \
    userdel --remove ubuntu 2>/dev/null || true; \
    groupadd --gid "$USER_GID" dev; \
    useradd --uid "$USER_UID" --gid "$USER_GID" --shell /bin/bash --create-home dev; \
    usermod --append --groups sudo dev; \
    echo 'dev ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/dev; \
    chmod 0440 /etc/sudoers.d/dev; \
    chown --recursive dev:dev "$RUSTUP_HOME" "$CARGO_HOME"

USER dev
WORKDIR /home/dev/source/big-code-analysis

# Runtime npm globals (as dev) land under the home dir, out of /usr.
RUN mkdir -p /home/dev/.local/bin && npm config set prefix /home/dev/.local

# Register the MCP servers into Claude Code's user scope, sourced from the
# single committed definition so there is no drift. `claude` then launches
# Context7 / codegraph / Serena as stdio subprocesses in every session.
COPY --chown=dev:dev docker/mcp.json /home/dev/.config/bca/mcp.json
RUN set -eux; \
    for name in $(jq -r '.mcpServers | keys[]' /home/dev/.config/bca/mcp.json); do \
        cfg=$(jq -c ".mcpServers[\"${name}\"]" /home/dev/.config/bca/mcp.json); \
        claude mcp add-json --scope user "${name}" "${cfg}"; \
    done

CMD ["bash"]
