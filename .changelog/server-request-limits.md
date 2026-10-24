---
applies_to:
- server
authors:
- drganjoo
- fahadzub
references: []
breaking: true
new_feature: true
bug_fix: true
---
Servers now bound slow and oversized clients by default: request body reads time out with `408` (`5m` for non-payload operations, `1h` for `@httpPayload`), `serve` caps concurrent connections at 16,384 and request headers at 30s, and event stream reads yield after 32 consecutive ready frames (`requestBodyReadPollBudget`). Event stream `initialMessage`, `messageCompletion`, and `idle` deadlines are opt-in. All settings live under `customizationConfig.protocols.common`, including `requestBodyMaxBytes`, which takes precedence over the legacy `codegen.requestBodyMaxBytes`. Body read deadlines require a tokio runtime with the time driver enabled.

Deadlines bound reads from the client, not handler execution. A body read or RPC `initialMessage` timeout rejects the request with `408` before the handler runs. When an event stream message starts arriving but does not complete within `messageCompletion`, or no message arrives within `idle`, the receiver's `recv()` returns an error whose source chain contains `event_stream_message_timeout::MessageTimeoutError` (`is_idle()` identifies idle expiry). The error is resumable and does not close the connection: call `recv()` again to grant one more timeout window, or propagate it to end the request. Retry only a bounded number of times; retrying indefinitely removes the protection.

`Serve::header_read_timeout` configures the first-request header deadline for either HTTP version and Hyper's subsequent HTTP/1 header reads. The first deadline starts when the connection task begins polling Hyper, after service creation, and ends before Tower readiness or request middleware. There is no separate protocol-detection deadline or preface inspection. Later HTTP/2 streams are not covered by the first-request deadline. Passing `None` or `Some(Duration::ZERO)` disables both header deadlines.

`serve` exposes one Hyper automatic builder through `configure_hyper`. With both HTTP/1 and HTTP/2 enabled, it uses `serve_connection_with_upgrades`; with `http1_only()` or `http2_only()`, it uses `serve_connection` and rejects the other protocol before dispatch. HTTP/1-only upgrades require a custom serve loop. Upgraded connections retain their connection permits, and graceful shutdown drains requests on both serving paths.

Request-read defaults are defined in the `settings` module of each server runtime: body deadlines are `5m`/`1h`, the event-stream poll budget is 32, event-stream deadlines are disabled, and the body size limit is unlimited. Codegen selects payload and streaming behavior and passes explicit overrides through. Omitted settings use the runtime defaults; explicit zero disables the corresponding limit. Updating the runtime can therefore update omitted defaults without regenerating the service.

Companion crates receive breaking version bumps alongside their runtime dependencies: server metrics and its macro move to `0.4.0` for server runtime `0.69`, and the Python server runtime moves to `0.70.0` for legacy server runtime `0.68`.

Both server runtimes enforce an armed body-read deadline before accepting completed input. Input that becomes ready at or after expiry returns `408` without invoking the handler; handler execution remains outside the body-read deadline.

Kotlin normalizes shared request settings through `protocolSettings()`, folding the legacy body-size setting into the common section. Generated SDKs initialize one typed `ServiceConfig` containing `ServiceRequestBodyConfig`, `RequestBodyCollectionConfig`, and event-stream deadlines. These use the same configuration types and operation selection API as decoupled routing, so its runtime can consume the emitted settings without replacing the readers or generated configuration representation.
