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
`/function`, `/ast`, `/ping`, and the introspection routes) remain
available as **deprecated aliases** until the 2.0 release and resolve to
the same handlers; new clients should use the `/v1` paths. The examples
below use the versioned form.

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
  failed for an otherwise-valid request.

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
