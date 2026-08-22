# Dev container

A self-contained image for running [Claude Code](https://docs.claude.com/en/docs/claude-code)
and this project's MCP servers against a mounted checkout, plus the full
`make pre-commit` / `make ci` validation gate. It bundles every runtime and
tool the workspace needs:

- **Rust** — stable `1.94` (matching `Cargo.toml`) plus `nightly` for
  `cargo +nightly udeps`; components `rustfmt`, `clippy`, `rust-analyzer`
  (the Rust LSP), `llvm-tools-preview`; and the cargo tools the gate uses
  (`cargo-udeps`, `cargo-insta`, `cargo-nextest`, `cargo-llvm-cov`,
  `cargo-about`, `cargo-deny`, `mdbook`).
- **Python** — 3.12 with `uv` (the canonical `make py-bootstrap` path),
  `maturin`, `ruff`, `mypy`, and `pyright` (the Python LSP).
- **Node** — for the npx-launched MCP servers, plus `bash-language-server`.
- **Lint tooling** pinned to the CI versions and SHA256-verified: `rumdl`,
  `taplo`, `shellcheck`, `shfmt`, `checkmake` (amd64 only), `actionlint`.
- **MCP servers** — [Context7](https://github.com/upstash/context7),
  [codegraph](https://github.com/optave/ops-codegraph-tool), and
  [Serena](https://github.com/oraios/serena).

## MCP servers

The three servers are defined once in [`docker/mcp.json`](./mcp.json) and
registered into Claude Code's **user scope** at image-build time. Claude
launches each as a stdio subprocess automatically — there is no
docker-compose and no separate server container. Verify inside the
container with:

```bash
claude mcp list
```

To change which servers are wired in, edit `docker/mcp.json` and rebuild
(`make dev-env-build`).

**codegraph** needs a graph built once per checkout before its tools work:

```bash
cd /home/dev/source/big-code-analysis
codegraph build      # writes .codegraph/graph.db
```

## Usage

Drive everything from the repo root via the Makefile:

```bash
make dev-env-build    # build the image (passes your host UID/GID)
make dev-env-run      # start it detached, with the repo bind-mounted
make dev-env-shell    # open a shell inside it
# ... inside the container:
claude                # MCP servers are already registered
make pre-commit       # the usual validation gate works here too
make dev-env-rm       # stop and remove the container when done
```

The container mounts this repository at
`/home/dev/source/big-code-analysis`. The build passes your host UID/GID
(`id -u` / `id -g`) so files written in the mounted checkout keep the right
ownership on the host.

One directory inside that checkout is deliberately *not* shared:
`big-code-analysis-py/.venv`. `make dev-env-run` mounts
`~/.cache/big-code-analysis-dev/py-venv` over it (`$XDG_CACHE_HOME` wins
where it is set), so the container gets its own virtualenv. A venv records absolute interpreter paths in
`pyvenv.cfg` and in every console-script shebang, so a single directory
cannot serve both sides of the bind mount — whichever environment ran
`uv sync` last owns it, and the other gets

```text
bash: .venv/bin/mypy: /…/.venv/bin/python: bad interpreter
```

from `py-typecheck`, `py-test`, `py-stubtest` and friends. Because the
in-container path is still spelled `.venv`, no `py-*` recipe needs to
know about the split. Run `make py-bootstrap` once inside the container
to populate it; the host's own venv contents are untouched.

The cache directory outlives the container, but the container's home —
and with it the uv-managed interpreter the venv points at — does not. So
after a `make dev-env-rm` / `make dev-env-run` cycle the cached venv can
name an interpreter that no longer exists, and uv cannot repair it in
place because a mount point cannot be deleted. Reset it from the host:

```bash
rm -rf ~/.cache/big-code-analysis-dev/py-venv/*
```

then `make py-bootstrap` inside the container again. For the same reason
`make py-clean` empties `.venv` in place rather than removing it when it
is the mount point.

## Authentication

Claude Code state lives in the container's home directory, which persists
for the life of the container (until `make dev-env-rm`). Authenticate
either way:

- **Interactive**: run `claude` and log in once; the session persists
  across `make dev-env-shell` invocations.
- **API key**: `export ANTHROPIC_API_KEY=…` on the host before
  `make dev-env-run` — it is forwarded into the container.
  `CODEGRAPH_LLM_API_KEY` is forwarded the same way for codegraph's
  optional LLM features.

## Architecture notes

Images build on `linux/amd64` and `linux/arm64`. `checkmake` is the one
tool without a single cross-arch source: amd64 uses the exact CI pin
(`mrtazz/checkmake` 0.2.2, amd64-only upstream), and arm64 falls back to the
maintained `checkmake/checkmake` fork (v0.3.2, same rules and
`.checkmake.ini` format). On any other architecture the build continues
without it and `make makefile-check` is the single unavailable gate.

Developer tools install to system locations (`/usr/local`, `/opt`) so they
are never shadowed by a mounted home directory. Only user state (Claude
credentials, shell history, caches) lives under `/home/dev`.
