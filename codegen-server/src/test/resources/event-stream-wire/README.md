These fixtures test generated servers using real Rust Hyper clients over
HTTP/1.1 TCP. The client follows the channel-backed body pattern used by
`ManualEventStreamClient` in the existing event-stream integration tests.
Tests delay or split request-body chunks after connecting; delaying the connector
alone would not exercise an incomplete message arriving at the server.

`EventStreamTimeoutWireTest` uses the ordinary `serverIntegrationTest`,
`rustCrate.testModule`, and `tokioTest` helpers. The generated Rust tests are
discoverable by Cargo. CI's existing `check-server-codegen-unit-tests` job
discovers the Kotlin `@Test` through `./gradlew codegen-server:test`.

Run the CI-discovered test from the repository root:

```sh
./gradlew :codegen-server:test --tests '*EventStreamTimeoutWireTest' --quiet
```

The wrapper prepares the three protocol crates using `cargo test --no-run`,
then executes their ordinary Rust test binaries concurrently under a shared
**10-second wall-clock limit**. Each generated Tokio test also has its own
ten-second deadline. Generation and compilation happen before the timed run.
Results are saved to `codegen-server/build/event-stream-wire/results.txt`.

Prepared Rust test binaries can also be run directly, for example:

```sh
codegen-server/build/event-stream-wire/json_tests event_stream_timeouts_over_tcp --nocapture
```

The 34 scenarios cover REST JSON, AWS JSON 1.1, and RPC v2 CBOR. They check:

- Initial expiry before the handler is invoked.
- Completion expiry during slow-drip frames, including a split length prefix.
- Idle expiry before the first event and between complete events.
- Timely events across multiple windows without conflicting deadlines.
- Inherited settings, operation overrides, and disabled idle deadlines.
- Handler retries after idle/completion expiry, preserving partial frame bytes.
- TCP closure and connection-permit reuse when the handler ends the request.

The Smithy event-stream frame writer supplies lengths and CRCs. Test handlers
decode the actual events, record the timeout they received, and choose whether
to retry or finish. The Hyper connection driver must complete after each
`Connection: close` response, verifying transport cleanup as well as handler
behavior.

Listeners bind only to `127.0.0.1` on ephemeral ports. Server and client tasks
have abort-on-drop guards, and the wrapper terminates unfinished test processes
on failure or deadline expiry.

Test configuration uses a 300 ms RPC initial deadline, a 200 ms completion
deadline, and a 1 s global idle deadline. `PublishEvents` and `RetryEvents`
override idle to 400 ms; `NoIdleEvents` disables it. Production defaults are
unchanged, and the TCP scenarios use real wall-clock time.
