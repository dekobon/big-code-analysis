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
`/function`, `/ast`, `/ping`) remain available as **deprecated aliases**
for one release cycle and resolve to the same handlers; new clients
should use the `/v1` paths. The examples below use the versioned form.

## Error responses

Errors are reported with an HTTP status code, not inside a `200` body:

- `404 Not Found` — the `file_name` extension maps to no supported
  language (JSON endpoints return an `{ "id", "error" }` body; the
  raw/octet-stream endpoints return a `text/plain` `error: …` body), or
  the URL matches no endpoint.
- `415 Unsupported Media Type` — a known `POST` endpoint received a
  `Content-Type` that is neither `application/json` nor
  `application/octet-stream` (a `charset` parameter is allowed).
- `405 Method Not Allowed` — a known endpoint was called with the wrong
  HTTP method (the analysis endpoints are `POST`-only; `/ping` is `GET`).
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
  "code": [10, 112, 114, 105, 110, 116]
}
```

The `code` field is a **byte array** of the stripped source, not a
string. Decode it with `jq -r '.code | implode'` (ASCII/UTF-8) or
the equivalent in your client. The `application/octet-stream`
variant returns the stripped source as the raw response body, which
is simpler for shell pipelines.

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
  "spans": [
    {
      "name": "function_name",
      "start_line": 1,
      "end_line": 10,
      "error": false
    }
  ]
}
```

`error` is `true` when the parser flagged the span as malformed
(e.g. unbalanced delimiters inside the function body).

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

**Response:**

```json
{
  "id": "unique-id",
  "language": "Rust",
  "spaces": {
    "metrics": {
      "cyclomatic_complexity": 5,
      "lines_of_code": 100,
      "function_count": 10
    }
  }
}
```
