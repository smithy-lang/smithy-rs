# Request limits integration server

This permanent REST JSON fixture exposes different request-body deadlines together,
with a 1 KiB buffered body limit. Automated tests launch the same executable on an
available loopback port. The Pokémon example uses its original model and build commands.

`settings.json` supplies the configuration under `protocols.common.request`.
The body limit is the quoted size `"1 KiB"` at `request.body.maxBytes`;
generation converts it to 1024 bytes. Integer byte counts are also accepted.
`KB` and `MB` are decimal units; `KiB` and `MiB` are binary units.

From this directory:

```sh
make test
make run                         # 127.0.0.1:13734; Ctrl-C stops the server
make run ARGS="--port 13735"      # choose another loopback port
```

Enable tracing with `RUST_LOG` (logs go to stderr):

```sh
RUST_LOG=trace make run
RUST_LOG=info,aws_smithy_http_server=trace,request_limits_server_sdk=trace make run
```

The second command focuses on the server runtime and generated SDK. For timeout
logs, start the server with this filter and send an incomplete body as shown below.

After the first code generation, Cargo can be used directly from `integration-tests/`:

```sh
cargo run -p request-limits --bin request-limits-server
cargo test -p request-limits
```

The service is registered permanently in `codegen-server-test/build.gradle.kts`.
The normal server integration test task includes this package. `make codegen`
generates its SDK and the SDKs required to load the integration-test workspace.
No feature flags or alternate Pokémon SDK are required. Rerun code generation after changing `model.smithy` or `settings.json`.

| Operation | Route | Body | Read deadline | Maximum body |
| --- | --- | --- | --- | --- |
| `Echo` | `POST /echo` | `{"note":"hello"}` | 5s default | 1024 bytes |
| `EchoWithOverride` | `POST /echo-override` | `{"note":"hello"}` | 3s override | 1024 bytes |
| `Upload` | `POST /upload` | Raw bytes | 10s payload default | 1024 bytes |

A stalled body returns HTTP `408` before calling the handler. The deadline covers
input reading and parsing, beginning when that future first waits; handler execution
is outside it. Exceeding the size limit returns HTTP `400 SerializationException` through the
current REST JSON body-buffering rejection, including chunked requests
without a Content-Length. For JSON, the limit counts the complete encoded body.
The byte limit is a service-wide setting, so these operations test both limits and
timeouts together. Event-stream deadlines have separate existing integration tests;
this fixture exercises non-streaming bodies.

The 10s header-read timeout is in `settings.json` under
`protocols.common.request.header.readTimeout`, passed to codegen as `customizationConfig`.
The generated `serve(listener, app.into_make_service())` applies it automatically;
an explicit `.header_read_timeout(...)` call overrides it. Omitting the JSON setting
keeps 1m, and numeric `0` or `"0s"` disables it.
Body limits and deadlines are selected during SDK generation. Update
`request.body` in `settings.json` and regenerate the SDK to change them.
`src/lib.rs` configures HTTP/2 keep-alive pings every 10s with a 5s response deadline. The server supports HTTP/1
and HTTP/2 and binds only to loopback. Header-read timeout protection for every
request currently supports HTTP/1 only. For HTTP/2, the deadline covers only the
first request head on the connection; later streams have no header-read deadline.
Keep-alive pings do not enforce a header-read deadline.

For a successful request:

```sh
curl -i -H 'Content-Type: application/json' -d '{"note":"hello"}' http://127.0.0.1:13734/echo
```

For an oversized payload:

```sh
python3 -c 'import sys; sys.stdout.buffer.write(b"x" * 1025)' | \
  curl -i -H 'Content-Type: application/octet-stream' --data-binary @- http://127.0.0.1:13734/upload
```

To compare the two JSON deadlines on the same running server, run this once with
`/echo-override` (roughly 3s), then `/echo` (roughly 5s), or `/upload` (roughly 10s):

```python
import socket
import time

path = "/echo-override"
with socket.create_connection(("127.0.0.1", 13734), timeout=2) as sock:
    sock.settimeout(15)
    started = time.monotonic()
    headers = (
        f"POST {path} HTTP/1.1\r\n"
        "Host: localhost\r\n"
        "Content-Type: application/json\r\n"
        "Content-Length: 100\r\n"
        "Connection: close\r\n\r\n"
    )
    sock.sendall(headers.encode() + b"{")
    response = bytearray()
    while chunk := sock.recv(4096):
        response.extend(chunk)
print(bytes(response).split(b"\r\n", 1)[0].decode(), time.monotonic() - started)
```

`tests/request_limits.rs` checks the JSON-configured header deadline, all three body deadlines concurrently against one
server, successful requests at the byte limit, and `400` size rejections above the limit
with both Content-Length and chunked framing. Each test cleans up its server process.
