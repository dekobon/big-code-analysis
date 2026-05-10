# Driving the REST API

`bca-web` exposes the same analysis primitives over
HTTP. Use it when the consumer is a long-running service (an editor
plugin, CI worker, or web app) that should not pay the cost of
spawning the CLI per file.

For the full endpoint reference, see [Rest API](../commands/rest.md).
The recipes below show practical end-to-end calls with `curl`.

## Start the server

```bash
bca-web --host 127.0.0.1 --port 8080 -j "$(nproc)"
```

Verify it's up:

```bash
curl -sf http://127.0.0.1:8080/ping && echo "ok"
# => ok
```

`/ping` returns `200 OK` with an empty body — `curl -sf` exits 0 on
success and non-zero on any HTTP error, which is what scripts want.

## Compute metrics for an inline snippet

```bash
curl -s http://127.0.0.1:8080/metrics \
    -H 'Content-Type: application/json' \
    -d '{
          "id": "snippet-1",
          "file_name": "demo.rs",
          "code": "fn add(a: i32, b: i32) -> i32 { a + b }",
          "unit": false
        }' \
  | jq '.spaces.metrics'
```

`unit: true` returns only top-level metrics; `false` walks every
function and class space inside the snippet. The server infers
language from `file_name`, so the extension matters.

## Compute metrics for a file from disk

`curl --data-binary` plus `jq` makes it easy to package a real file
into the JSON envelope the server expects:

```bash
jq -nc \
    --arg id "$(uuidgen)" \
    --arg file_name "src/lib.rs" \
    --rawfile code src/lib.rs \
    '{id: $id, file_name: $file_name, code: $code, unit: false}' \
  | curl -s http://127.0.0.1:8080/metrics \
      -H 'Content-Type: application/json' \
      --data-binary @- \
  | jq '.spaces.metrics.cyclomatic, .spaces.metrics.cognitive'
```

This pattern — `jq -n --rawfile` to build the request, `curl
--data-binary @-` to stream it — is the easiest way to avoid quoting
problems with multi-line source code.

## Strip comments through the API

The endpoint is `/comment` (singular). It has two variants selected
by `Content-Type`:

- `application/json` — wraps the request and response in JSON. The
  response `code` field is a **byte array**, not a string, because
  the underlying API is byte-oriented.
- `application/octet-stream` — accepts the source as the raw request
  body and returns the stripped source as the raw response body. This
  is by far the easiest variant to use from the shell.

Octet-stream form (recommended for one-off shell use):

```bash
curl -s "http://127.0.0.1:8080/comment?file_name=demo.py" \
    -H 'Content-Type: application/octet-stream' \
    --data-binary $'# leading comment\nprint("hi")  # trailing'
# => print("hi")
```

JSON form (use when your client speaks JSON natively). Decode the
byte array with `jq … | implode` for ASCII / UTF-8 source:

```bash
curl -s http://127.0.0.1:8080/comment \
    -H 'Content-Type: application/json' \
    -d '{
          "id": "strip-1",
          "file_name": "demo.py",
          "code": "# leading comment\nprint(\"hi\")  # trailing"
        }' \
  | jq -r '.code | implode'
```

The JSON response carries the same `id` you sent, so a client that
multiplexes many requests can correlate them.

## Extract function spans for an editor plugin

The endpoint is `/function` (singular):

```bash
curl -s http://127.0.0.1:8080/function \
    -H 'Content-Type: application/json' \
    -d '{
          "id": "spans-1",
          "file_name": "demo.rs",
          "code": "fn a() {}\nfn b() {}\n"
        }' \
  | jq '.spans'
```

Each entry has `name`, `start_line`, `end_line`, and an `error`
boolean (set when the parser flagged the function span as
malformed) — enough for an editor to draw a function navigator
without re-parsing the file locally.

## Calling the API from CI

The server starts in milliseconds, so for short-lived CI jobs it's
often simplest to start it as a background process inside the job and
tear it down at the end:

```bash
bca-web --port 8080 &
SERVER_PID=$!
trap 'kill "$SERVER_PID"' EXIT

# Wait for it to come up.
until curl -sf http://127.0.0.1:8080/ping >/dev/null; do sleep 0.1; done

# … run your analysis calls here …
```

For longer-lived workers, run the server as a systemd unit (or
container) and point your jobs at its host/port.
