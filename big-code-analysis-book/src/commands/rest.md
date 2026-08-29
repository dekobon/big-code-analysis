# Rest API

**bca-web** is a web server that allows users to analyze source code
through a REST API. This service is useful for anyone looking to
perform code analysis over HTTP.

The server can be run on any host and port, and supports the following main functionalities:

- Remove Comments from source code.
- Retrieve Function Spans for given code.
- Retrieve the AST (abstract syntax tree) for given code.
- Compute Metrics for the provided source code.

## Running the Server

To run the server, you can use the following command:

```sh
bca-web --host 127.0.0.1 --port 9090
```

- `--host` specifies the IP address where the server should run (default is 127.0.0.1).
- `--port` specifies the port to be used (default is 8080).
- `-j` specifies the number of parallel jobs (optional).
- `--cors` enables [CORS](#cors) for browser-based tooling (off by default).

For the full flag set, environment variables, resource limits, and the
trust boundaries to respect before exposing the daemon, see
[Operating bca-web](web-server.md).

## CORS {#cors}

By default **bca-web emits no CORS headers**: a browser script served from
a different origin cannot read the API's responses. This keeps a local
`bca-web` (the default `127.0.0.1` bind) from exposing its repository paths
and metrics to any website the operator happens to be visiting.

Pass `--cors` to opt in. The argument is an explicit, comma-separated
allow-list of [origins](https://developer.mozilla.org/en-US/docs/Glossary/Origin);
only those origins receive an
[`Access-Control-Allow-Origin`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Origin)
header, and the matched origin is echoed back verbatim (a request from any
other origin gets no header and is blocked by the browser):

```sh
bca-web --cors https://app.example,https://tools.example
```

To answer **every** origin with `Access-Control-Allow-Origin: *`, pass a
literal `*`:

```sh
bca-web --cors '*'
```

A wide-open `*` exposes the server's metrics and repository paths to any
origin, so use it only on trusted networks.

When CORS is enabled, a preflight
[`OPTIONS`](https://developer.mozilla.org/en-US/docs/Web/HTTP/Methods/OPTIONS)
request is answered `204 No Content` with `Access-Control-Allow-Origin`,
`Access-Control-Allow-Methods` (the resource's own accepted methods, the same
set the `Allow` header advertises), and `Access-Control-Allow-Headers`
(echoing the request's `Access-Control-Request-Headers`, or `Content-Type,
Accept` for a bare probe). The API has no authentication or cookies, so
`Access-Control-Allow-Credentials` is **never** sent.

## API Versioning {#api-versioning}

All endpoints are mounted under a `/v1` prefix (for example
`/v1/metrics`). The full route set is `/v1/ping`, `/v1/version`,
`/v1/languages`, `/v1/ast`, `/v1/comment`, `/v1/function`, `/v1/metrics`,
`/v1/vcs`, `/v1/vcs/trend`, `/v1/vcs/jit`, and the route index `/v1`. To
discover them programmatically, `GET /v1` (see
[Route index](#8-route-index)).

The unprefixed paths (`/metrics`, `/comment`, `/ast`, `/`, …) that
earlier `1.x` releases served as deprecated aliases were **removed in
2.0**; requesting one now returns `404`. Use the `/v1` form everywhere.

## Error responses

Errors are reported with an HTTP status code, not inside a `200` body.
**Every** error — on the JSON endpoints, the raw/octet-stream
endpoints, and the `415`/`405`/`404` fallbacks alike — returns one
uniform machine-readable JSON body so clients parse a single error
shape regardless of the success content-type:

```json
{
  "error": "human-readable message",
  "error_kind": "stable_machine_token",
  "id": "echoed-request-id"
}
```

`error` is the specific human-readable cause; `error_kind` is a stable
`snake_case` machine token (e.g. `unknown_field`, `unsupported_language`,
`bad_request`, `parse_timeout`) so clients branch on the cause without
string-matching the prose (issue #631). The token vocabulary is closed
and governed by [`STABILITY.md`](https://github.com/dekobon/big-code-analysis/blob/main/STABILITY.md).

The `id` key is **always present**. It carries the client-supplied
correlation id when the request had one (the JSON endpoints), and an
empty string otherwise (the octet-stream / query endpoints carry no
id, and the content-type / method / not-found fallbacks — and any
request whose body failed to parse before the id was read — have no
parsed id to echo).

Status codes:

- `400 Bad Request` — a malformed body or query parameter: invalid
  JSON, a missing required field, an unrecognised key (the strict
  `deny_unknown_fields` parse, including the removed `unit` flag — see
  *Compute Metrics* below), or a `scope` value that is not `full` /
  `file`.
- `422 Unprocessable Entity` — the `file_name` extension (and content
  sniffing) maps to no supported language. The route matched and the
  body parsed; only the submitted entity cannot be processed. The
  response carries the stable machine token `"error":
  "unsupported_language"`; query `GET /v1/languages` for the supported
  set. (Before 2.0 this was a `404`, indistinguishable from an unknown
  URL — see issue #634.)
- `404 Not Found` — the URL matches no endpoint.
- `415 Unsupported Media Type` — a known `POST` endpoint received a
  `Content-Type` that is neither `application/json` nor
  `application/octet-stream` (a `charset` parameter is allowed).
- `405 Method Not Allowed` — a known endpoint was called with the wrong
  HTTP method (the analysis endpoints are `POST`-only; `/ping`,
  `/version`, and `/languages` are `GET`-only).
- `406 Not Acceptable` — the request `Accept` header named only media
  types the server cannot produce. The structured analysis endpoints
  serve `application/json`, `application/yaml`, and `application/cbor`;
  any other concrete type (e.g. `application/xml`) is a `406` carrying the
  `not_acceptable` token. See [Content negotiation](#content-negotiation).
- `413 Payload Too Large` — the request body exceeded the server limit.
- `500 Internal Server Error` — metric computation or AST construction
  failed for an otherwise-valid request, or a `/vcs` history walk failed
  on the server side. This also covers a response too deeply nested to
  serialize (`serialize_failed`) — see
  [Nesting limits](#nesting-limits) below.
- `503 Service Unavailable` — the parse pool is saturated by orphaned
  (timed-out) tasks; retry later.
- `504 Gateway Timeout` — the parse (or history walk) exceeded the
  server's configured deadline.

## Nesting limits {#nesting-limits}

Responses are trees, and no serializer can emit a tree without one
native stack frame per level. Rather than let a deeply nested response
overflow the stack — which aborts the whole process, taking every
in-flight request with it — the server refuses to serialize past a
fixed depth and answers `500` with the `serialize_failed` token:

| Response | Limit | What counts as a level |
|---|---|---|
| `/metrics` | 128 | one nested function space (a function inside a function inside a …) |
| `/ast` | 512 | one AST node |

Both limits sit far above anything real source reaches. Across the
14 450-file corpus this project tests against (TensorFlow, DeepSpeech,
serde, …) the deepest AST is 188 nodes and the deepest function-space
nesting is 10. The
`/metrics` limit is also more generous than the read side — a JSON
document of nested spaces cannot be parsed back past ~61 levels,
because `serde_json`'s own 128-level recursion limit charges two levels
per space.

## Content negotiation {#content-negotiation}

The structured analysis endpoints — `/v1/ast`, `/v1/comment` (JSON
variant), `/v1/function`, `/v1/metrics`, `/v1/vcs`, `/v1/vcs/trend`, and
`/v1/vcs/jit` — choose their response serialization from the request
`Accept` header, mirroring the CLI's `-O json|yaml|cbor` outputs. The
same value serializes byte-for-byte identically whether it comes from
the CLI or the server.

| `Accept` value                                   | Response `Content-Type` |
| ------------------------------------------------ | ----------------------- |
| absent, `*/*`, `application/*`, `application/json`| `application/json`      |
| `application/yaml` (or `text/yaml`, `application/x-yaml`) | `application/yaml` |
| `application/cbor`                                | `application/cbor`      |
| any other concrete type                          | **`406 Not Acceptable`**|

Rules:

- **JSON is the default.** A request with no `Accept` header, or one that
  includes `*/*` / `application/*` / `application/json`, gets JSON — the
  same body and `Content-Type` earlier releases always returned, so
  existing clients need no change.
- **`q` weights are honored.** Among the supported types the highest
  `q`-weighted entry wins (`Accept: application/json;q=0.5,
  application/yaml;q=0.9` returns YAML); `q=0` refuses a type. Ties keep
  the first-listed entry.
- **Unsupported types are a `406`,** not a silent JSON fallback. The body
  is the uniform `{error, error_kind, id}` envelope with
  `error_kind: "not_acceptable"`, and the message lists the supported
  media types.
- **Only structured serializations are offered.** TOML and CSV are
  excluded: TOML is awkward for the deeply nested space tree and CSV is
  flat/tabular. The error-envelope body and the `/v1/comment`
  octet-stream variant (raw byte-in / byte-out) are always JSON / raw
  bytes respectively and do not negotiate. The introspection routes
  (`/v1`, `/v1/version`, `/v1/languages`) return JSON metadata only.

```sh
# YAML metrics
curl --silent \
  --header 'Content-Type: application/json' \
  --header 'Accept: application/yaml' \
  --data '{"file_name": "foo.py", "code": "def f():\n    pass\n"}' \
  http://127.0.0.1:8080/v1/metrics

# CBOR metrics (binary; pipe to a decoder)
curl --silent \
  --header 'Content-Type: application/json' \
  --header 'Accept: application/cbor' \
  --data '{"file_name": "foo.py", "code": "def f():\n    pass\n"}' \
  http://127.0.0.1:8080/v1/metrics --output metrics.cbor
```

## Endpoints

### 1. Ping the Server

Use this endpoint to check if the server is running.

**Request:**

```http
GET http://127.0.0.1:8080/v1/ping
```

**Response:**

- Status Code: `200 OK`
- Body: empty.

Use `curl -sf http://127.0.0.1:8080/v1/ping && echo ok` to script a
liveness check — `-f` makes curl exit non-zero on any HTTP error.

### 2. Remove Comments

This endpoint removes comments from the provided source code. It
accepts two `Content-Type` variants. Use `application/octet-stream`
for raw byte-in / byte-out, and `application/json` for a JSON
envelope.

**Request:**

```http
POST http://127.0.0.1:8080/v1/comment
```

**Payload:**

```json
{
  "id": "unique-id",
  "file_name": "filename.ext",
  "code": "source code with comments"
}
```

- `id`: A unique identifier for the request. Optional (issue #645);
  omitting it defaults to an empty string (treated as "no correlation
  id").
- `file_name`: The name of the file being analyzed.
- `code`: The source code with comments.

**Response (JSON variant):**

```json
{
  "id": "unique-id",
  "language": "cpp",
  "code": "print"
}
```

The response envelope reports `id`, the detected `language` (the
canonical lowercase slug — see *Compute Metrics* below), and the
`code` result key. The `code` field is a **string** holding the
stripped source: the request `code` arrived as a JSON string, so the
stripped output is guaranteed valid UTF-8 and is handed back as a
string, matching the request and every other JSON endpoint. The
`application/octet-stream` variant returns the stripped source as the
raw response body (no envelope), which is the correct home for
binary-faithful round-trips and simpler for shell pipelines; its
*errors* still use the uniform JSON error body above.

When the source contains no removable comments, both variants signal
the empty result with a `200` status and an empty payload: the JSON
variant returns `"code": ""` (an empty string) and the
octet-stream variant returns an empty body. The status code and
envelope shape are therefore identical regardless of the requested
`Content-Type`; the octet-stream variant returns an empty `200` body
rather than `204 No Content`.

### 3. Retrieve Function Spans

This endpoint retrieves the spans of functions in the provided source code.

**Request:**

```http
POST http://127.0.0.1:8080/v1/function
```

**Payload:**

```json
{
  "id": "unique-id",
  "file_name": "filename.ext",
  "code": "source code with functions"
}
```

- `id`: A unique identifier for the request. Optional (issue #645);
  omitting it defaults to an empty string (treated as "no correlation
  id").
- `file_name`: The name of the file being analyzed.
- `code`: The source code with functions.

**Response:**

```json
{
  "id": "unique-id",
  "language": "cpp",
  "spans": [
    {
      "name": "function_name",
      "start_line": 1,
      "end_line": 10
    }
  ]
}
```

The envelope reports `id`, the detected `language` slug, and the
`spans` result key. `name` is `null` when the parser could not resolve the function's
name from the AST (e.g. an anonymous or malformed definition). A
`null` `name` is the malformed-span signal.

### 4. Retrieve the AST

This endpoint returns the full tree-sitter abstract syntax tree (AST)
for the provided source code as a recursive JSON node tree.

**Request:**

```http
POST http://127.0.0.1:8080/v1/ast
```

**Payload:**

```json
{
  "id": "unique-id",
  "file_name": "filename.ext",
  "code": "source code to parse",
  "comment": false,
  "span": true
}
```

- `id`: A unique identifier for the request. Optional; omitting it
  defaults to an empty string (treated as "no correlation id").
- `file_name`: The name of the file being analyzed.
- `code`: The source code to parse.
- `comment`: When `true`, comment nodes are omitted from the tree.
  Optional; defaults to `false`.
- `span`: When `true`, each node carries its source `span`; when
  `false`, `span` is `null`. Optional; defaults to `false`.

`id`, `comment`, and `span` are optional and default as noted above
(issue #645); `file_name` and `code` are required. Unknown keys are
rejected with `400` (issue #633).

**Response:**

```json
{
  "id": "unique-id",
  "language": "rust",
  "root": {
    "type": "source_file",
    "value": "",
    "span": { "start_line": 1, "start_col": 1, "end_line": 2, "end_col": 1 },
    "field_name": null,
    "children": [
      {
        "type": "function_item",
        "value": "",
        "span": { "start_line": 1, "start_col": 1, "end_line": 1, "end_col": 13 },
        "field_name": null,
        "children": [
          {
            "type": "identifier",
            "value": "main",
            "span": { "start_line": 1, "start_col": 4, "end_line": 1, "end_col": 8 },
            "field_name": "name",
            "children": []
          }
        ]
      }
    ]
  }
}
```

The envelope reports `id`, the detected `language` slug, and `root` —
the root AST node. Each node carries:

- `type`: the tree-sitter grammar node kind (grammar-specific; the
  `language` slug tells you which grammar produced it).
- `value`: the source text for leaf/named tokens (empty for interior
  nodes).
- `span`: a `{ start_line, start_col, end_line, end_col }` object (all
  1-based), or `null` when `span` was `false`. These span keys use the
  `*_line` vocabulary shared with `/function` and `/metrics` (issue
  #638 renamed the former `*_row` keys).
- `field_name`: the tree-sitter grammar field through which the parent
  reaches this node (e.g. `name`, `left`, `body`), or `null` for the
  root, anonymous tokens, and unfielded children.
- `children`: the node's child nodes, recursively.

Unlike the metric and function endpoints, the AST endpoint reports node
coordinates over the **exact bytes** the client submitted — the source
is not EOL-normalised, so spans line up with the client's own copy
(issue #640).

### 5. Compute Metrics

This endpoint computes various metrics for the provided source code.

**Request:**

```http
POST http://127.0.0.1:8080/v1/metrics
```

**Payload:**

```json
{
  "id": "unique-id",
  "file_name": "filename.ext",
  "code": "source code for metrics",
  "scope": "full"
}
```

- `id`: Unique identifier for the request. Optional (issue #645);
  omitting it defaults to an empty string (treated as "no correlation
  id").
- `file_name`: The filename of the source code file.
- `code`: The source code to analyze.
- `scope`: How much of the space tree to return. `full` (the default)
  returns the complete nested space tree — the file-level root plus a
  recursive `spaces` list for every function, class, and other unit.
  `file` returns only the file-level root with its `spaces` children
  cleared. This field replaces the pre-2.0 boolean `unit` flag (issue

  #638); sending the old `unit` key now fails with `400`.

The payload is validated strictly: an unrecognised key (a typo, or the
removed `unit`) is rejected with `400` and the uniform JSON error body,
naming the offending field (issue #633).

On the `application/octet-stream` variant, the source is the raw
request body and `scope` is supplied as a **query parameter**
(`?file_name=…&scope=full`). When the parameter is omitted it defaults
to `full`; an unrecognised value is rejected with `400`.

**Response:**

```json
{
  "id": "unique-id",
  "language": "rust",
  "root": {
    "name": "sample.rs",
    "start_line": 1,
    "end_line": 7,
    "kind": "unit",
    "spaces": [
      {
        "name": "double",
        "start_line": 1,
        "end_line": 7,
        "kind": "function",
        "spaces": [],
        "metrics": {
          "cyclomatic": { "sum": 2, "average": 2.0, "min": 2, "max": 2 },
          "loc": { "sloc": 7, "ploc": 7, "lloc": 1, "cloc": 0, "blank": 0 },
          "nom": { "functions": 1, "closures": 0, "total": 1 }
        }
      }
    ],
    "metrics": { "...": "the same metric block, aggregated over the file" }
  }
}
```

The response envelope reports `id`, the detected `language` slug, and
`root` — the **single** file-level space object (issue #638 renamed
this key from the misleading plural `spaces`). `root` carries `name`
(the request `file_name`), the `start_line` / `end_line` span, a `kind`
discriminator (`unit`, `function`, `class`, …), its `metrics` block,
and a recursive `spaces` list of child units. The example above is
**trimmed**: each real `metrics` block contains every metric family —
`cyclomatic`, `cognitive`, `halstead`, `loc`, `nom`, `nargs`, `nexits`,
`tokens`, `mi`, `abc`, and `wmc` — with the same nested fields the `bca`
CLI emits. When `scope` is `file`, `root.spaces` is an empty list.

The `language` value is the **canonical lowercase slug** (e.g. `rust`,
`cpp`, `csharp`, `tsx`) — the same token the language vocabulary
accepts — not a human-pretty display name. Every analysis endpoint
(`/ast`, `/comment`, `/function`, `/metrics`) reports this `language`
field so clients can confirm which grammar was selected.

### 6. Server and Library Version

Reports the running server version and the version of the
`big-code-analysis` library it was built against.

**Request:**

```http
GET http://127.0.0.1:8080/v1/version
```

**Response:**

```json
{
  "server": "2.2.0",
  "library": "2.2.0"
}
```

### 7. Supported Languages

Lists the supported languages and their registered file extensions.
The names are the canonical lowercase slugs; the list and extensions
are sourced from the library's language table, never hardcoded.

**Request:**

```http
GET http://127.0.0.1:8080/v1/languages
```

**Response:**

```json
{
  "languages": [
    { "name": "cpp", "extensions": ["cpp", "cc", "hpp", "..."] },
    { "name": "rust", "extensions": ["rs"] }
  ]
}
```

Like every endpoint, `/version` and `/languages` are served only under
the `/v1` prefix; the unprefixed `1.x` aliases were removed in 2.0 (see
[API Versioning](#api-versioning)).

### 8. Route index {#8-route-index}

Returns a machine-readable index of every registered route — its path,
the HTTP methods it accepts, and a one-line description — so clients can
discover the API surface without scraping this chapter. The index is
generated from the same route table the server registers, so it cannot
drift from the live routing.

**Request:**

```http
GET http://127.0.0.1:8080/v1
```

**Response:**

```json
{
  "service": "bca-web",
  "version": "2.2.0",
  "routes": [
    { "path": "/v1", "methods": ["GET", "HEAD"], "description": "This route index." },
    { "path": "/v1/metrics", "methods": ["POST"], "description": "Compute maintainability metrics for the source." }
  ]
}
```

`service` is always `bca-web`; `version` matches the `server` field of
`GET /v1/version`. The unprefixed root `/` that earlier releases served
as this endpoint's alias was removed in 2.0.

## Change-history (VCS) metrics {#change-history-vcs-metrics}

Three endpoints expose the change-history (version-control) metrics —
the same numbers `bca vcs` computes from the CLI. Unlike every other
endpoint, these analyse a **git repository already present on the
server's filesystem** rather than source code carried in the request
body: VCS metrics derive from commit history, which has no in-request
representation.

> **Operator warning — `repo_path` is a trust boundary.** The
> `repo_path` field is a server-side filesystem path. These endpoints
> make the server walk *any* git repository it can read and return that
> repository's relative file paths, churn, and author signals. This is
> materially different from the source-in-body endpoints, which only
> ever see code the client sends. The optional `cache_dir` field is a
> second caller-supplied server-side path that grants a *write*
> capability: with caching enabled (the default), the server creates
> directories and writes JSON cache files under it
> (`<cache_dir>/<repo>/<head_sha>.json`), so a caller controlling
> `cache_dir` can direct the server to write cache files at any path the
> server process can write to. (`cache_dir` is accepted only by `/vcs`;
> `/vcs/trend` and `/vcs/jit` do not cache.) The endpoint's filesystem
> reach is
> therefore an arbitrary read of any readable git repository *and* an
> arbitrary write of cache files under any writable path. **Do not expose
> `/vcs`, `/vcs/trend`, or `/vcs/jit` to untrusted clients without an
> authorization layer in front of `bca-web`.** The default `127.0.0.1`
> bind keeps them local. Each walk runs under the same parse-timeout and
> blocking-pool guard as the analysis endpoints.

All three endpoints are `POST`-only, accept `application/json`, echo the
request `id`, and report errors with the uniform `{error, error_kind,
id}` body (its `error_kind` tokens are the `vcs_*` family — e.g.
`vcs_not_a_repository`, `vcs_invalid_window`). A
client mistake — `repo_path` does not exist or is not a git working tree
(both carry `vcs_not_a_repository`), an unresolvable
`ref`/`commit`, a malformed or non-diff `diff`, an unusable
`author_hash_key` (`vcs_invalid_author_hash_key`), or a malformed window /
timestamp / formula / file-type / threshold / trend parameter — is a
`400`; a failure of the history walk itself is a `500`. A nonexistent
`repo_path` is a typo — the most common client error here — so it answers
`400` like a path that exists but is not a repository, not a `500`
(issue 653).

### 9. Rank files by risk — `/vcs`

Walks the repository's history once and returns its files ranked by a
composite risk score (issue #328).

**Request:**

```http
POST http://127.0.0.1:8080/v1/vcs
```

**Payload:**

```json
{
  "id": "unique-id",
  "repo_path": "/srv/repos/my-project"
}
```

`repo_path` is required; every other field is optional and defaults to
the `bca vcs` default. `id` is optional too (issue #645) — omitting it
defaults to an empty string, echoed back unchanged. The optional fields
are:

- `long_window` / `recent_window`: window specs (e.g. `12mo`, `90d`).
  Defaults `12mo` / `90d`.
- `top`: keep only the top *N* files by risk. Absent defaults to `50`
  (the `bca vcs --top` default); an explicit `0` returns all files.
- `ref`: revision to analyse (default `HEAD`).
- `risk_formula`: `weighted` (default) or `percentile`.
- `file_types`: `metrics` (default — only files bca has metrics for),
  `all` (every tracked text file), or a comma-separated extension
  allow-list (`rs,py`).
- `full_history`: walk the full DAG rather than first-parent only.
- `include_merges`: include merge commits.
- `follow_renames`: follow renames (default `true`).
- `exclude_bots`: exclude bot identities (default `true`).
- `bot_pattern`: override the bot-author exclusion regex.
- `as_of`: reference "now" (RFC 3339 / `@unix` / git date) for
  snapshots.
- `emit_author_details`: emit SHA-256-hashed author identities.
- `author_hash_key`: secret key that hardens `emit_author_details` into a
  keyed HMAC-SHA256 (requires `emit_author_details`; an empty key or one
  without the flag is a `400`).
- `include_deleted`: include files deleted at the target ref.
- `bus_factor_threshold`: bus-factor coverage threshold in `(0, 1)`
  (default `0.5`).
- `no_cache`: disable the persistent change-history cache for this
  request (default `false`).
- `cache_dir`: override the server-side cache directory.

**Response:**

```json
{
  "id": "unique-id",
  "vcs_schema_version": 2,
  "risk_score_version": 2,
  "long_window_days": 365,
  "recent_window_days": 90,
  "truncated_shallow_clone": false,
  "vcs_aggregate": { "...": "directory- / repo-level bus factor" },
  "files": [
    {
      "path": "src/main.rs",
      "vcs": {
        "commits_long": 12,
        "commits_recent": 3,
        "churn_long": 540,
        "churn_recent": 80,
        "authors_long": 4,
        "authors_recent": 2,
        "risk_score": 1.42
      }
    }
  ]
}
```

`files` is ordered by descending `vcs.risk_score`. Each entry carries the
repository-relative `path` plus a nested `vcs` metric block (the same
shape `bca vcs` emits, issue #684): commit and churn counts over the long
and recent windows, author counts, ownership share, burst, bug-fix /
security-fix / revert counts, age, change and co-change entropy, and the
composite `risk_score`. `hotspot_score` and the hashed `author_ids`
appear inside that block only when computable / requested. The four
constant stamps `vcs_schema_version`, `risk_score_version`,
`long_window_days`, and `recent_window_days` sit once at the top level,
never per row (issue #635). `vcs_aggregate` carries the directory- and
repo-level bus factor (issue #332).

### 10. Historical trend — `/vcs/trend`

Samples the change-history metrics at several evenly-spaced points in
time and returns the per-file time series (issue #333). Its response is a
series, not a ranked snapshot, so it is a distinct route from `/vcs`.

**Request:**

```http
POST http://127.0.0.1:8080/v1/vcs/trend
```

**Payload:** every `/vcs` field above **except the cache controls**
(`no_cache` / `cache_dir`) — trend does not use the persistent cache, so
sending either field is a `400` (issue #961) rather than a silent no-op —
plus:

- `points`: number of evenly-spaced sample points (`>= 2`). Defaults to
  `12` (the `bca vcs trend --points` default) when omitted.
- `span`: total look-back the points cover (default `12mo`).
- `top_deltas`: top *N* files per improving / regressing list. Absent
  defaults to `10`; an explicit `0` returns all.

**Response:**

```json
{
  "id": "unique-id",
  "trend_schema_version": 1,
  "vcs_schema_version": 2,
  "risk_score_version": 2,
  "long_window_days": 365,
  "recent_window_days": 90,
  "truncated_shallow_clone": false,
  "as_of_points": [1704067200, 1711929600],
  "files": {
    "src/main.rs": [ { "as_of": 1704067200, "vcs": { "risk_score": 1.1 } }, null ]
  },
  "deltas": { "improved": [], "regressed": [] }
}
```

`as_of_points` lists the sample timestamps oldest-first. Each file's
array in `files` aligns to it 1:1, with a `null` element at a point where
the file did not yet exist; each present element is
`{ "as_of": ..., "vcs": { ... } }`, with that file's VCS block nested
under `vcs` at that moment (issue #684). The four constant stamps sit
once at the top level, never per point (issue #635). `deltas` ranks the
most-improved and most-regressed files by their risk-score movement
across the series.

### 11. Just-in-time risk — `/vcs/jit`

Scores the just-in-time risk of a **single change** — either one commit
on a server-side repository, or an arbitrary unified diff carried in the
request body (issues #331 / #580). The two modes are mutually exclusive.

**Commit mode** scores a commit on `repo_path`:

```json
{
  "id": "unique-id",
  "repo_path": "/srv/repos/my-project",
  "commit": "HEAD"
}
```

Commit mode also accepts the experience-window knobs `long_window`,
`recent_window`, `full_history`, `include_merges`, `follow_renames`, and
`as_of`. The response is a full report whose `source` is `"commit"` and
whose `risk_score` folds in all five feature groups (size, diffusion,
history, experience, purpose):

```json
{
  "id": "unique-id",
  "jit_schema_version": 3,
  "jit_score_version": 1,
  "source": "commit",
  "long_window_days": 365,
  "recent_window_days": 90,
  "risk_score": 0.87,
  "commit": { "id": "…", "parent_count": 1, "is_merge": false, "purpose": {} },
  "features": { "size": {}, "diffusion": {}, "history": {}, "experience": {} },
  "contributions": { "size": 0.4, "diffusion": 0.2, "history": 0.1, "purpose": 0.0, "experience": -0.1 }
}
```

**Diff mode** scores an arbitrary unified diff with no repository:

```json
{
  "id": "unique-id",
  "diff": "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-old\n+new\n"
}
```

A bare diff carries no author, parent, or history, so only the *size* and
*diffusion* groups are computable. The diff report's `source` is
`"diff"` and it reports `partial_risk_score` — **not** `risk_score` —
because the missing groups are *absent from the body entirely*, never
present as zero:

```json
{
  "id": "unique-id",
  "jit_schema_version": 3,
  "jit_score_version": 1,
  "source": "diff",
  "partial_risk_score": 0.6,
  "size": {},
  "diffusion": {},
  "contributions": { "size": 0.4, "diffusion": 0.2 }
}
```

Branch on the `source` discriminator (`"commit"` vs `"diff"`) to read the
right score field. `partial_risk_score` is always lower than a commit's
`risk_score` for the same change and lives on a different scale: rank
diffs against other diffs, never against commit scores.

**Mode conflict.** Supplying `diff` together with **any** commit-mode
field (`repo_path`, `commit`, a window, history, rename, or `as_of` knob)
is rejected with a `400` rather than silently honouring the diff and
dropping the rest — the two modes answer different, non-comparable
questions, so the combination is treated as a client mistake (issue
number 632).

**Non-diff `diff`.** A `diff` value that is not a git unified diff —
the wrong field, an accidentally-mangled string, arbitrary text — is
rejected with a `400` (`vcs_invalid_diff`) rather than scored as a
confident `partial_risk_score` of `0.0`. On a risk-*gating* endpoint a
spurious "zero risk" is the most dangerous failure mode, so non-diff
input is a hard error (issue 652). An **empty** or whitespace-only `diff`
is the one exception: it legitimately means "no changes", so it still
returns a valid `0.0` — a CI step that computed an empty diff gets the
zero-risk answer it expects.
