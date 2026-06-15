# Operating bca-web

`bca-web` is the HTTP daemon that wraps the `big-code-analysis`
library, exposing comment removal, function spans, AST dumps,
maintainability metrics, and change-history (VCS) metrics over a REST
API. This page is for the operator running the daemon: how to build
and start it, which flags and environment variables tune it, and the
trust boundaries to respect before exposing it.

It is the operations companion to two reference pages. The
[REST API reference](rest.md) documents every endpoint, its request
and response shapes, and the error contract. The
[Driving the REST API](../recipes/rest-api.md) recipe shows
end-to-end `curl` calls. This page covers the process itself and links
to those two rather than repeating them.

## Build and run

`bca-web` is the binary of the `big-code-analysis-web` crate. From a
checkout, run it through Cargo:

```sh
cargo run -p big-code-analysis-web -- --host 127.0.0.1 --port 8080
```

To install the binary on your `PATH`, build a release artifact and
copy it out, or install from the crate:

```sh
cargo install big-code-analysis-web   # installs the `bca-web` command
bca-web --host 127.0.0.1 --port 8080
```

`bca-web` binds the requested address, serves until interrupted, and
exits non-zero if it cannot bind the port or hits an I/O error, so a
supervisor ([systemd](https://www.freedesktop.org/wiki/Software/systemd/),
a container orchestrator, or a CI smoke check) sees the failure and
can restart or alert.

### Building with a subset of languages does not work

The shipped `bca-web` binary compiles every supported tree-sitter
grammar in. The `big-code-analysis-web` crate pins the library's
`all-languages` feature set explicitly, so passing
`--no-default-features` or a custom `--features` list to
`cargo build -p big-code-analysis-web` does **not** drop grammars from
the resulting binary. Dropping a grammar silently from a user-facing
daemon would surface as "language X stopped working" at request time
rather than as a build error, so the crate forbids it
([issue #252](https://github.com/dekobon/big-code-analysis/issues/252)).

If you need a reduced grammar set, embed the `big-code-analysis`
library in your own Rust code and select features in your own
`Cargo.toml`. The [per-language Cargo features](../library/cargo-features.md)
chapter lists every feature with a worked example.

## Command-line flags

The full flag set, with defaults:

| Flag | Default | Purpose |
|------|---------|---------|
| `-j`, `--num-jobs <N\|auto>` | `auto` | Worker-thread count. `auto` resolves to the OS-reported effective CPU count. |
| `--host <HOST>` | `127.0.0.1` | Address to bind. |
| `-p`, `--port <PORT>` | `8080` | TCP port. |
| `--parse-timeout-secs <SECS>` | `30` | Per-parse deadline. `0` disables it. |
| `--cors <ORIGINS>` | off | Enable CORS for a comma-separated origin allow-list. |
| `-h`, `--help` | | Print help and exit. |
| `-V`, `--version` | | Print version and exit. |

`--num-jobs auto` is
[cgroup](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html)-quota-
and `cpuset`-aware on Linux: in a container with a CPU quota it
resolves to the quota rather than the host's physical core count,
matching the `bca` CLI's `--num-jobs`. This count sizes the worker
pool and the parse-admission semaphore, so it caps how many parses run
concurrently. The minimum is `1`; `0` is rejected at parse time.

`--parse-timeout-secs` bounds how long a single parse may run before
the request returns `504 Gateway Timeout`. The default of `30` guards
against a pathological input wedging a worker indefinitely. Setting it
to `0` removes the deadline and, with it, the load-shedding described
below; use `0` only when an unbounded parse is acceptable. See the
[REST API reference](rest.md) for the response body the timeout
returns.

CORS is off by default. The [CORS](rest.md#cors) section of the
reference documents it in full, covering preflight handling, the
wildcard form, and the absence of credentials. The short version: pass
`--cors` with an explicit origin allow-list to let browser tooling
read responses; omit it to emit no `Access-Control-*` headers at all.

## Environment variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `BCA_MAX_ORPHANED_TASKS` | `max(num_jobs * 2, 4)` | Cap on orphaned (timed-out but still-running) parse tasks before new requests are shed with `503`. |
| `RUST_LOG` | `info` | Log filter for the [`tracing`](https://docs.rs/tracing/) subscriber. |

`RUST_LOG` uses the
[`EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html)
syntax (for example `RUST_LOG=big_code_analysis_web=debug`). The
daemon emits one access-log line per completed request, carrying the
method, route, status, and latency.

## Resource limits and back-pressure

Two limits protect the daemon from a single client exhausting it.

**Request body size.** Every endpoint rejects a request body larger
than 4 MiB with `413 Payload Too Large`. The limit applies uniformly
to the JSON and raw-octet-stream content types, so both reject
oversized bodies at the same threshold.

**Orphaned-task admission control.** When a parse exceeds
`--parse-timeout-secs`, the request returns `504`, but the blocking
thread keeps running on the pool until the parse finishes on its own,
because tree-sitter cannot be interrupted mid-parse. To stop sustained
pathological input from piling up unbounded background work, new
requests are rejected with `503 Service Unavailable` once the count of
orphaned tasks reaches a soft cap. The cap defaults to
`max(num_jobs * 2, 4)` and is overridable through
`BCA_MAX_ORPHANED_TASKS` (parsed as an unsigned integer; an invalid or
zero value falls back to the default). Setting `--parse-timeout-secs 0`
disables this mechanism entirely, since with no deadline no task is
ever orphaned.

## Security and trust boundaries

`bca-web` has no authentication, authorization, or rate limiting of
its own. The defaults are chosen for a local, single-operator
deployment; widen them deliberately.

**Default bind is loopback.** The server binds `127.0.0.1` unless
`--host` says otherwise. Keep it there, or put an authenticating proxy
in front, before exposing it to a network. Binding `0.0.0.0` makes
every capability below reachable by anyone who can route to the port.

**CORS is off by default.** With no `--cors` flag, a browser script
from another origin cannot read API responses, so a page the operator
happens to visit cannot quietly drive a loopback `bca-web`. The
wildcard form (`--cors '*'`) answers every origin and exposes the
server's metrics and repository paths to any site; use it only on
trusted networks. Full semantics are under [CORS](rest.md#cors).

**The VCS endpoints read server-side repositories.** Unlike the
source-in-body endpoints, `/v1/vcs`, `/v1/vcs/trend`, and `/v1/vcs/jit`
analyze a git repository already on the server's filesystem, named by
the request's `repo_path`. A caller who can reach these endpoints can
make the server walk any git repository it can read and learn that
repository's file paths, churn, and author signals. The
[VCS trust-boundary warning](rest.md#change-history-vcs-metrics) in the
reference covers this in full; do not expose these endpoints to
untrusted clients without an authorization layer.

**The VCS cache directory is a client-controlled write path.** The
VCS endpoints accept an optional `cache_dir` field that overrides where
the persistent change-history cache is written, defaulting to the
platform cache location (`$XDG_CACHE_HOME/big-code-analysis/vcs`). A
caller who can set `cache_dir` chooses a directory the server process
writes into, so an untrusted client could direct cache writes to an
attacker-chosen path. This is one more reason the VCS endpoints belong
behind an authorization layer, never open to untrusted input.
