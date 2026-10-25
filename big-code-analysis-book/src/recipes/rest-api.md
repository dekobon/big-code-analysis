# Driving the REST API

`bca-web` exposes the same analysis primitives as the `bca` CLI over
HTTP. Use it when the consumer is a long-running service (an editor
plugin, CI worker, or web app) that should not pay the cost of
spawning the CLI per file. This page is a set of end-to-end
[`curl`](https://curl.se/) calls, one per endpoint; the
[REST API reference](../commands/rest.md) documents every field.

Every endpoint is mounted under the `/v1` prefix; the old unprefixed
paths were removed at the 2.0 release (#637) and now return `404`.

## Start the server

```bash
bca-web --host 127.0.0.1 --port 8080 -j "$(nproc)"
```

Verify it is up:

```bash
curl -sf http://127.0.0.1:8080/v1/ping && echo "ok"
# => ok
```

`/ping` returns `200 OK` with an empty body. `curl -sf` exits 0 on
success and non-zero on any HTTP error, which is what scripts want.

For building, flag and environment-variable tuning, resource limits,
and the security posture of the daemon itself, see
[Operating bca-web](../commands/web-server.md).

## Compute metrics for an inline snippet

```bash
curl -s http://127.0.0.1:8080/v1/metrics \
    -H 'Content-Type: application/json' \
    -d '{
          "id": "snippet-1",
          "file_name": "demo.rs",
          "code": "fn add(a: i32, b: i32) -> i32 { a + b }",
          "scope": "full"
        }' \
  | jq '.root.metrics'
```

`scope: "file"` returns only top-level metrics; `"full"` (the default)
walks every function and class space inside the snippet. The server infers
language from `file_name`, so the extension matters.

## Compute metrics for a file from disk

`curl --data-binary` plus [`jq`](https://jqlang.github.io/jq/)
packages a real file into the JSON envelope the server expects:

```bash
jq -nc \
    --arg id "$(uuidgen)" \
    --arg file_name "src/lib.rs" \
    --rawfile code src/lib.rs \
    '{id: $id, file_name: $file_name, code: $code, scope: "full"}' \
  | curl -s http://127.0.0.1:8080/v1/metrics \
      -H 'Content-Type: application/json' \
      --data-binary @- \
  | jq '.root.metrics.cyclomatic, .root.metrics.cognitive'
```

This pattern (`jq -n --rawfile` to build the request, `curl
--data-binary @-` to stream it) avoids quoting problems with
multi-line source code.

## Strip comments through the API

The endpoint is `/comment` (singular). It has two variants selected
by `Content-Type`:

- `application/json` wraps the request and response in JSON. The
  response `code` field is a **string** holding the stripped source
  (the request `code` arrived as a JSON string, so the result is
  valid UTF-8; #629 replaced the earlier byte-array shape).
- `application/octet-stream` accepts the source as the raw request
  body and returns the stripped source as the raw response body. This
  is the simpler variant to use from the shell.

Octet-stream form (recommended for one-off shell use):

```bash
curl -s "http://127.0.0.1:8080/v1/comment?file_name=demo.py" \
    -H 'Content-Type: application/octet-stream' \
    --data-binary $'# leading comment\nprint("hi")  # trailing'
# => print("hi")
```

JSON form (use when your client speaks JSON natively):

```bash
curl -s http://127.0.0.1:8080/v1/comment \
    -H 'Content-Type: application/json' \
    -d '{
          "id": "strip-1",
          "file_name": "demo.py",
          "code": "# leading comment\nprint(\"hi\")  # trailing"
        }' \
  | jq -r '.code'
```

The JSON response carries the same `id` you sent, so a client that
multiplexes many requests can correlate them.

## Extract function spans for an editor plugin

The endpoint is `/function` (singular):

```bash
curl -s http://127.0.0.1:8080/v1/function \
    -H 'Content-Type: application/json' \
    -d '{
          "id": "spans-1",
          "file_name": "demo.rs",
          "code": "fn a() {}\nfn b() {}\n"
        }' \
  | jq '.spans'
```

Each entry has `name`, `start_line`, and `end_line`; `name` is `null`
when the parser could not resolve the function's name (an anonymous
or malformed definition). That is enough for an editor to draw a
function navigator without re-parsing the file locally.

## Rank a repository by change-history risk

The `/vcs` endpoint analyses a git working tree that already exists on
the **server's** filesystem (change history has no in-request
representation), and returns the files ranked by composite risk score.
See [Change-history (VCS) metrics](../commands/vcs.md) for the signal
and formula reference.

> **Security:** unlike the other endpoints, `/vcs` takes a server-side
> `repo_path` and will walk any git repository the server process can
> read, returning that repo's file paths and change signals. Do not
> expose `/vcs` to untrusted clients without an authorization layer; the
> default `127.0.0.1` bind keeps it local.

```bash
curl -s http://127.0.0.1:8080/v1/vcs \
    -H 'Content-Type: application/json' \
    -d '{
          "id": "risk-1",
          "repo_path": "/srv/checkouts/my-project",
          "top": 20
        }' \
  | jq '.files[] | {path, risk_score: .vcs.risk_score, churn_recent: .vcs.churn_recent}'
```

The body accepts the same knobs as `bca vcs` (`long_window`,
`recent_window`, `top`, `ref`, `risk_formula`, `file_types`,
`full_history`, `include_merges`, `follow_renames`, `exclude_bots`,
`bot_pattern`, `as_of`, `emit_author_details`, `author_hash_key`,
`include_deleted`, `bus_factor_threshold`, `no_cache`, `cache_dir`) as
optional fields; `top` defaults to `50` like the CLI, and `0` returns
every file. `file_types` scopes the ranking (`metrics`, the default,
or `all`, or a `rs,py`-style extension list), mirroring the CLI
`--file-types`. A `repo_path` that does not exist or is not a git
working tree, or a malformed window / timestamp / formula / scope,
returns `400` with the uniform JSON error body. Unknown fields are
rejected with `400` too.

### Trend over time

`POST /vcs/trend` samples the same metrics at several points in time and
returns a per-file time series (see [Historical
trend](../commands/vcs.md#historical-trend-over-time)). The body takes the
`/vcs` fields except the cache controls (`no_cache` / `cache_dir` are
rejected with `400`, since trend does not cache) plus `points` (>= 2,
default `12`), `span` (default `12mo`), and `top_deltas` (default
`10`; `0` returns all).

```bash
curl -s http://127.0.0.1:8080/v1/vcs/trend \
    -H 'Content-Type: application/json' \
    -d '{
          "id": "trend-1",
          "repo_path": "/srv/checkouts/my-project",
          "points": 12,
          "span": "24mo",
          "top": 20
        }' \
  | jq '.deltas.regressed[] | {path, delta}'
```

A point count below 2 (or above the supported maximum) returns `400` with
the uniform JSON error body, like the other bad-request cases.

### Just-in-time commit / diff scoring

`POST /vcs/jit` scores a single commit (see [Just-in-time
scoring](../commands/vcs.md#just-in-time-commit-level-scoring)). The body
takes `repo_path`, `commit` (default `HEAD`), and the `long_window` /
`recent_window` / `full_history` / `include_merges` / `follow_renames` /
`as_of` knobs; it returns the commit `JitReport` JSON with the echoed
`id`.

```bash
curl -s http://127.0.0.1:8080/v1/vcs/jit \
    -H 'Content-Type: application/json' \
    -d '{ "id": "jit-1", "repo_path": "/srv/checkouts/my-project",
          "commit": "HEAD" }' \
  | jq '{risk_score, purpose: .commit.purpose}'
```

To score an arbitrary diff instead, send a `diff` field carrying the
unified diff (no `repo_path` needed). The response is then the *partial*
report: `source: "diff"`, a `partial_risk_score`, and **no** history /
experience / purpose groups, which are absent rather than zero. The
partial score is **not comparable** to a commit score.

```bash
git diff | jq -Rs '{id: "jit-diff", diff: .}' \
  | curl -s http://127.0.0.1:8080/v1/vcs/jit \
      -H 'Content-Type: application/json' -d @- \
  | jq '{source, partial_risk_score}'
```

A malformed diff (or an unresolvable `commit`, or a `repo_path` that is
not a git working tree) returns `400` with the uniform JSON error body.

## Calling the API from CI

For a short-lived CI job, start the server as a background process
and stop it when the job finishes:

```bash
bca-web --port 8080 &
SERVER_PID=$!
trap 'kill "$SERVER_PID"' EXIT

# Wait for it to come up.
until curl -sf http://127.0.0.1:8080/v1/ping >/dev/null; do sleep 0.1; done

# … run your analysis calls here …
```

For longer-lived workers, run the server as a
[systemd](https://www.freedesktop.org/wiki/Software/systemd/) unit (or
a container) and point your jobs at its host and port.
