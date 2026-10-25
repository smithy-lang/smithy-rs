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
Servers now apply request-body read deadlines and yield while reading ready event-stream frames. The built-in `aws-smithy-http-server::serve` loop also limits concurrent connections and request-header reads. These defaults can change existing server behavior.

Configure generated servers in `smithy-build.json` under `customizationConfig.protocols.common`. These settings apply to both server runtimes:

| Parameter relative to `customizationConfig.protocols.common` | Default | Behavior |
| --- | --- | --- |
| `requestBodyMaxBytes` | `0` (unlimited) | Maximum bytes buffered for a non-streaming request body. |
| `requestBodyReadTimeouts.defaultNonPayload` | `"5m"` | Whole-body read deadline for non-streaming operations without an `@httpPayload` input member. |
| `requestBodyReadTimeouts.defaultPayload` | `"1h"` | Whole-body read deadline for non-streaming operations with an `@httpPayload` input member. |
| `requestBodyReadTimeouts.perOperation` | `{}` | Maps full operation shape IDs to duration overrides; omitted operations inherit the applicable body-read default. Streaming inputs do not support whole-body deadlines. |
| `requestBodyReadPollBudget` | `32` | Consecutive ready event-stream HTTP body frames read before yielding to the executor. |
| `eventStreamTimeouts.initialMessage` | Disabled | RPC event-stream initial-frame deadline, including the wait for its first bytes; REST streams have no initial read. |
| `eventStreamTimeouts.messageCompletion` | Disabled | Deadline from an event frame's first bytes until the complete frame is read. |
| `eventStreamTimeouts.idle` | Disabled | Deadline while waiting for the next event frame's first bytes. |
| `eventStreamTimeouts.perOperation` | `{}` | Maps full event-stream input operation shape IDs to objects containing `initialMessage`, `messageCompletion`, and/or `idle`; omitted fields inherit the corresponding service setting. |

Timeout values accept duration strings with `ms`, `s`, `m`, or `h` units, such as `"250ms"` or `"10s"`. Numeric `0` disables a timeout. Set `requestBodyMaxBytes` or `requestBodyReadPollBudget` to `0` to disable that limit. The common `requestBodyMaxBytes` setting takes precedence over the supported legacy `codegen.requestBodyMaxBytes` setting. Omitted settings use runtime defaults, so a runtime update can change them without regenerating the service. Body and event-stream deadlines require a Tokio runtime with its time driver enabled.

Configure transport limits through the Rust `serve` builder, separately from the JSON settings:

| Builder option | Default | Override |
| --- | --- | --- |
| `.max_connections(max)` | `16_384` concurrent connections | `0` or `.disable_connection_limit()` disables the limit. |
| `.header_read_timeout(timeout)` | `Some(Duration::from_secs(30))` | `None` or `Some(Duration::ZERO)` disables the deadline. |
| `.configure_hyper(callback)` | HTTP/2 keep-alive ping interval and acknowledgment timeout are both `20s` | Supplies a custom Hyper builder, replacing its defaults; `.header_read_timeout(...)` still controls HTTP/1 header deadlines. |
| `.with_graceful_shutdown(signal).with_shutdown_timeout(timeout)` | No shutdown timeout; waits indefinitely | Sets how long graceful shutdown waits for connections to finish. |

Header deadlines cover the first request head for HTTP/1 and HTTP/2, and subsequent HTTP/1 request heads. Later HTTP/2 streams have no header deadline. With both protocols enabled, `serve` supports HTTP/1 upgrades; HTTP/1-only upgrades require a custom serve loop. Upgraded connections retain their connection permits.

A body-read or RPC initial-message timeout returns `408` before the handler runs, including when input becomes ready at or after expiry. Deadlines do not limit handler execution. Event-stream completion and idle timeouts surface from `recv()` with `event_stream_message_timeout::MessageTimeoutError` in the source chain. They are resumable: calling `recv()` again grants another timeout window without closing the connection. Bound retries to retain the protection.

Server runtime versions move to `0.69.0` (`aws-smithy-http-server`) and `0.68.0` (`aws-smithy-legacy-http-server`); server metrics and its macro move to `0.4.0`, and the Python server runtime moves to `0.70.0`.
