# Rest API

**bca-web** is a web server that allows users to analyze source code
through a REST API. This service is useful for anyone looking to
perform code analysis over HTTP.

The server can be run on any host and port, and supports the following main functionalities:

- Remove Comments from source code.
- Retrieve Function Spans for given code.
- Compute Metrics for the provided source code.

## Running the Server

To run the server, you can use the following command:

```sh
bca-web --host 127.0.0.1 --port 9090
```

- `--host` specifies the IP address where the server should run (default is 127.0.0.1).
- `--port` specifies the port to be used (default is 8080).
- `-j` specifies the number of parallel jobs (optional).

## API Versioning

All endpoints are mounted under a `/v1` prefix (for example
`/v1/metrics`). The original unprefixed paths (`/metrics`, `/comment`,
`/function`, `/ast`, `/vcs`, `/vcs/trend`, `/vcs/jit`, `/ping`, the
introspection routes, and the route index `/`) remain available as
**deprecated aliases** until the 2.0 release and resolve to the same
handlers; new clients should use the `/v1` paths. To discover the full
route set programmatically, `GET /v1` (see [Route index](#7-route-index)).
The examples below use the versioned form.

Every alias response is stamped with deprecation-signalling headers so
clients and gateways can detect alias use before the routes are removed:

- `Deprecation: true` — the resource is deprecated
  ([Deprecation HTTP header draft](https://datatracker.ietf.org/doc/draft-ietf-httpapi-deprecation-header/)).
- `Sunset: <http-date>` — the planned-removal date
  ([RFC 8594](https://www.rfc-editor.org/rfc/rfc8594)); advisory, the
  authoritative removal trigger is the 2.0 release cut.
- `Link: </v1/...>; rel="successor-version"` — the canonical `/v1` twin
  to migrate to.

The `/v1` routes carry none of these headers. The aliases themselves are
removed at the 2.0 release.

## Error responses

Errors are reported with an HTTP status code, not inside a `200` body.
**Every** error — on the JSON endpoints, the raw/octet-stream
endpoints, and the `415`/`405`/`404` fallbacks alike — returns one
uniform machine-readable JSON body so clients parse a single error
shape regardless of the success content-type:

```json
{
  "error": "human-readable message",
  "id": "echoed-request-id"
}
```

The `id` key is **always present**. It carries the client-supplied
correlation id when the request had one (the JSON endpoints), and an
empty string otherwise (the octet-stream / query endpoints carry no
id, and the content-type / method / not-found fallbacks have not
parsed a body).

Status codes:

- `400 Bad Request` — a malformed query parameter (e.g. a `unit` flag
  that is not a recognised boolean — see *Compute Metrics* below).
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
- `413 Payload Too Large` — the request body exceeded the server limit.
- `500 Internal Server Error` — metric computation or AST construction
  failed for an otherwise-valid request, or a `/vcs` history walk failed
  on the server side.
- `503 Service Unavailable` — the parse pool is saturated by orphaned
  (timed-out) tasks; retry later.
- `504 Gateway Timeout` — the parse (or history walk) exceeded the
  server's configured deadline.

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

- `id`: A unique identifier for the request.
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

- `id`: A unique identifier for the request.
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

### 4. Compute Metrics

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
  "unit": false
}
```

- `id`: Unique identifier for the request.
- `file_name`: The filename of the source code file.
- `code`: The source code to analyze.
- `unit`: A boolean value. `true` to compute only top-level metrics,
  `false` for detailed metrics across all units (functions, classes,
  etc.).

In the JSON payload, `unit` is a JSON boolean (`true` / `false`). On
the `application/octet-stream` variant, the source is the raw request
body and `unit` is supplied as a **query parameter**
(`?file_name=…&unit=…`) accepting normal boolean forms: `true` /
`false` and `1` / `0`, case-insensitively. Any other value (including
the formerly-accepted `yes` / `on`) is rejected with `400` and the
uniform JSON error body. When the parameter is omitted it defaults to
`false`.

**Response:**

```json
{
  "id": "unique-id",
  "language": "rust",
  "spaces": {
    "metrics": {
      "cyclomatic_complexity": 5,
      "lines_of_code": 100,
      "function_count": 10
    }
  }
}
```

The `language` value is the **canonical lowercase slug** (e.g. `rust`,
`cpp`, `csharp`, `tsx`) — the same token the language vocabulary
accepts — not a human-pretty display name. Every analysis endpoint
(`/metrics`, `/comment`, `/function`) reports this `language` field so
clients can confirm which grammar was selected.

### 5. Server and Library Version

Reports the running server version and the version of the
`big-code-analysis` library it was built against.

**Request:**

```http
GET http://127.0.0.1:8080/v1/version
```

**Response:**

```json
{
  "server": "1.1.0",
  "library": "1.1.0"
}
```

### 6. Supported Languages

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

Like `/ping`, both `/version` and `/languages` are also exposed as
unprefixed aliases (`/version`, `/languages`) until the 2.0 release;
those alias responses carry the same deprecation headers described under
[API Versioning](#api-versioning).

### 7. Route index

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
  "version": "1.1.0",
  "routes": [
    { "path": "/v1", "methods": ["GET", "HEAD"], "description": "This route index." },
    { "path": "/v1/metrics", "methods": ["POST"], "description": "Compute maintainability metrics for the source." }
  ]
}
```

`service` is always `bca-web`; `version` matches the `server` field of
`GET /v1/version`. The unprefixed root `/` is the deprecated alias for
this endpoint and carries the same deprecation headers as the other
aliases.

## Change-history (VCS) metrics

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
> ever see code the client sends. **Do not expose `/vcs`, `/vcs/trend`,
> or `/vcs/jit` to untrusted clients without an authorization layer in
> front of `bca-web`.** The default `127.0.0.1` bind keeps them local.
> Each walk runs under the same parse-timeout and blocking-pool guard as
> the analysis endpoints.

All three endpoints are `POST`-only, accept `application/json`, echo the
request `id`, and report errors with the uniform `{error, id}` body. A
client mistake — `repo_path` is not a git working tree, an unresolvable
`ref`/`commit`, a malformed diff, or a malformed window / timestamp /
formula / file-type / threshold / trend parameter — is a `400`; a
failure of the history walk itself is a `500`.

### 8. Rank files by risk — `/vcs`

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

`id` and `repo_path` are required; every other field is optional and
defaults to the `bca vcs` default. The optional fields are:

- `long_window` / `recent_window`: window specs (e.g. `12mo`, `90d`).
  Defaults `12mo` / `90d`.
- `top`: keep only the top *N* files by risk (`0` / absent = all).
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
  "long_window_days": 365,
  "recent_window_days": 90,
  "truncated_shallow_clone": false,
  "vcs_aggregate": { "...": "directory- / repo-level bus factor" },
  "files": [
    {
      "path": "src/main.rs",
      "vcs_schema_version": 1,
      "risk_score_version": 1,
      "commits_long": 12,
      "commits_recent": 3,
      "churn_long": 540,
      "churn_recent": 80,
      "authors_long": 4,
      "authors_recent": 2,
      "risk_score": 1.42
    }
  ]
}
```

`files` is ordered by descending `risk_score`. Each entry carries the
repository-relative `path` plus the flat VCS metric block (the same shape
`bca vcs` emits): commit and churn counts over the long and recent
windows, author counts, ownership share, burst, bug-fix / security-fix /
revert counts, age, change and co-change entropy, and the composite
`risk_score`. `hotspot_score` and the hashed `author_ids` appear only
when computable / requested. `vcs_aggregate` carries the directory- and
repo-level bus factor (issue #332).

### 9. Historical trend — `/vcs/trend`

Samples the change-history metrics at several evenly-spaced points in
time and returns the per-file time series (issue #333). Its response is a
series, not a ranked snapshot, so it is a distinct route from `/vcs`.

**Request:**

```http
POST http://127.0.0.1:8080/v1/vcs/trend
```

**Payload:** every `/vcs` field above, plus:

- `points` (**required**): number of evenly-spaced sample points (`>= 2`).
- `span`: total look-back the points cover (default `12mo`).
- `top_deltas`: top *N* files per improving / regressing list (`0` /
  absent = all).

**Response:**

```json
{
  "id": "unique-id",
  "trend_schema_version": 1,
  "vcs_schema_version": 1,
  "risk_score_version": 1,
  "long_window_days": 365,
  "recent_window_days": 90,
  "truncated_shallow_clone": false,
  "as_of_points": [1704067200, 1711929600],
  "files": {
    "src/main.rs": [ { "as_of": 1704067200, "risk_score": 1.1 }, null ]
  },
  "deltas": { "improved": [], "regressed": [] }
}
```

`as_of_points` lists the sample timestamps oldest-first. Each file's
array in `files` aligns to it 1:1, with a `null` element at a point where
the file did not yet exist; each present element is that file's full VCS
block at that moment. `deltas` ranks the most-improved and most-regressed
files by their risk-score movement across the series.

### 10. Just-in-time risk — `/vcs/jit`

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
