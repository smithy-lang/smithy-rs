# Request limits integration tests

`tests/request_limits.rs` starts a generated REST JSON server in a Tokio task
and sends requests over HTTP/1.1 TCP. Each test binds an available loopback port
and aborts its server task when the test finishes. The server setup, handlers,
and assertions live in this one Rust file.

`model.smithy` defines the three operations. `settings.json` supplies
`customizationConfig.protocols.common.request` during SDK generation:

| Operation | Route | Body-read deadline | Maximum body |
| --- | --- | --- | --- |
| `Echo` | `POST /echo` | 5s default | 1024 bytes |
| `EchoWithOverride` | `POST /echo-override` | 3s override | 1024 bytes |
| `Upload` | `POST /upload` | 10s payload default | 1024 bytes |

The three tests verify:

- Incomplete headers close the connection after the JSON-configured 10s header deadline.
- Incomplete bodies return HTTP 408 at each operation's configured deadline.
- Bodies of 1024 bytes succeed and bodies of 1025 bytes return HTTP 400,
  with both Content-Length and chunked framing.

Run from the repository root:

```sh
./gradlew :codegen-server-test:assemble --quiet
cargo test --quiet --manifest-path codegen-server-test/integration-tests/Cargo.toml -p request-limits
```

The Gradle command generates the SDKs required by the integration-test workspace.
After generation, only the Cargo command is needed to rerun these tests.

The normal server integration-test task includes this package. Regenerate the
SDK after changing the Smithy model or JSON settings. Event-stream deadlines
have separate tests; this package covers non-streaming bodies and header reads.
