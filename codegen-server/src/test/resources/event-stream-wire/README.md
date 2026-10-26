These fixtures test generated servers using real Rust Hyper clients over
HTTP/1.1 TCP. The client follows the channel-backed body pattern used by
`ManualEventStreamClient` in the existing event-stream integration tests.
Tests withhold or split request-body chunks after connecting; delaying the
connector alone would not exercise an incomplete message arriving at the server.

`EventStreamTimeoutWireTest` uses the ordinary `serverIntegrationTest`,
`rustCrate.testModule`, and `tokioTest` helpers. The generated Rust tests are
discoverable by Cargo. CI's existing `check-server-codegen-unit-tests` job
discovers the Kotlin `@Test` through `./gradlew codegen-server:test`.

Run the CI-discovered test from the repository root:

```sh
./gradlew :codegen-server:test --tests '*EventStreamTimeoutWireTest' --quiet
```

The wrapper prepares the three protocol crates using `cargo test --no-run`,
then executes their ordinary Rust test binaries concurrently. Each scenario has
its own **15-second watchdog**, including server startup and TCP cleanup. Each
protocol binary also has an independent five-minute process watchdog to catch
runtime hangs. These are hang guards, not elapsed-time assertions. Generation
and compilation happen before execution.
Results are saved to `codegen-server/build/event-stream-wire/results.txt`.
Scenario counting tolerates libtest printing its test-name prefix on the same
line as a scenario's `PASS` marker.

Prepared Rust test binaries can also be run directly, for example:

```sh
codegen-server/build/event-stream-wire/json_tests event_stream_timeouts_over_tcp --nocapture
```

The 38 scenarios cover REST JSON, AWS JSON 1.1, and RPC v2 CBOR. They check:

- Initial expiry before the handler is invoked.
- RPC inputs with only an event stream preserve the pre-handler first-frame read:
  missing or incomplete first frames use the initial deadline and reject before
  the handler, while a complete first ordinary event is buffered for the handler.
- Completion expiry while a frame is stalled inside its length prefix.
- Idle expiry before the first REST event, after the RPC first-frame read, and between complete events.
- Complete events decoded in order, with handler acknowledgements between sends.
- Inherited settings, operation overrides, and disabled idle deadlines.
- Handler retries after idle/completion expiry, preserving partial frame bytes.
- TCP closure and connection-permit reuse when the handler ends the request.

The Smithy event-stream frame writer supplies lengths and CRCs. Test handlers
decode the actual events, record the timeout kind and configured duration they
received, and choose whether to retry or finish. Successful event-stream
responses are decoded as JSON or CBOR and checked against both the expected
outcome and the handler's recorded result. The Hyper connection driver must
complete after each `Connection: close` response, verifying transport cleanup
as well as handler behavior.

Each scenario owns a fresh server, handler observations, and results. Handler
signals replace sleeps for event delivery and timeout/retry ordering. A retrying
handler waits until the test queues the remaining bytes before calling `recv`
again. The connection-slot test holds the first handler after timeout until the
test explicitly releases it, so checking the queued request cannot race its
completion.

Listeners bind only to `127.0.0.1` on ephemeral ports. Server and client tasks
have abort-on-drop guards, and the wrapper terminates unfinished test processes
on failure or deadline expiry.

Test configuration uses a 1 s RPC initial deadline, a 1 s completion
deadline, and a 5 s common idle deadline. `PublishEvents` and `RetryEvents`
override idle to 1 s; `NoIdleEvents` disables it. The inherited/disabled checks
wait 1.5 s, leaving 3.5 s of margin before the inherited deadline. All event
stream deadlines are disabled by default in production; these TCP scenarios
explicitly enable them and use real wall-clock time. Exact deadline boundaries, slow-drip
schedules, independent timer windows, and idle resets are covered separately
by `#[tokio::test(start_paused = true)]` tests in
`rust-runtime/inlineable/src/event_stream_message_timeout.rs`; the TCP fixtures
check propagation, serialization, and transport cleanup.

RPC v2 CBOR operations used for initial-read checks have an optional non-stream
input field. AWS JSON's server generator currently rejects combining non-stream
input fields with an event stream, so its checks use stream-only inputs.
`NoInitialEvents` has only the event stream and still reads its first frame before
the handler. Its 1 s idle override applies after that frame.
