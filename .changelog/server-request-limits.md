---
applies_to:
  - server
authors:
  - drganjoo
references: []
breaking: true
new_feature: true
bug_fix: true
---

Servers now apply request-body read deadlines and yield while reading ready event-stream frames. The built-in `aws-smithy-http-server::serve` loop also limits concurrent connections and request-header reads. These defaults can change existing server behavior.

Service teams can override all the defaults below, using the configuration location supported by each setting:

- **Request reads:** Set request-body size limits, body-read deadlines, read poll budgets, and event-stream deadlines in the service's `smithy-build-template.json`, inside the `rust-server-codegen` plugin configuration at `customizationConfig.protocols.common.request.body`, then regenerate the SDK. Generated serde uses these settings for every application built from that SDK.
- **Application startup:** Set connection limits, HTTP/2 keep-alive settings, and graceful-shutdown timeouts in application Rust code on the `serve` builder. These changes do not require SDK regeneration.
- **Both locations:** Set `request.header.readTimeout` in that JSON configuration or call `.header_read_timeout(...)` on the generated `sdk::serve(...)` builder. An explicit Rust builder call takes precedence over the generated JSON setting.

Rust builder overrides take effect when the server is constructed; they do not provide live reconfiguration of a running server.

Both server runtimes use the same typed request-read configuration; header-deadline enforcement through the generated `serve` function requires the current runtime (`codegen.http-1x: true`).

Shared configuration types are exposed under `aws_smithy_http_server::config` and `aws_smithy_legacy_http_server::config`. Generated code uses these paths. Configuration lives separately from schema routing and serialization.

| Parameter relative to `customizationConfig.protocols.common` | Default         | Behavior                                                                                                                                                                                                                       |
| ------------------------------------------------------------ | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request.header.readTimeout`                                          | `"1m"`          | Request-head deadline automatically applied by generated `sdk::serve(listener, app.into_make_service())`; covers the first HTTP/1 or HTTP/2 request head and later HTTP/1 heads. Later HTTP/2 streams have no header deadline. |
| `request.body.maxBytes`                                        | `0` (unlimited) | Maximum bytes buffered for a non-streaming request body.                                                                                                                                                                       |
| `request.body.readTimeouts.defaultNonPayload`                  | `"5m"`          | Whole-body read deadline for non-streaming operations without an `@httpPayload` input member.                                                                                                                                  |
| `request.body.readTimeouts.defaultPayload`                     | `"1h"`          | Whole-body read deadline for non-streaming operations with an `@httpPayload` input member.                                                                                                                                     |
| `request.body.readTimeouts.perOperation`                       | `{}`            | Maps full operation shape IDs to duration overrides; omitted operations inherit the applicable body-read default. Streaming inputs do not support whole-body deadlines.                                                        |
| `request.body.readPollBudget`                                  | `32`            | Consecutive ready event-stream HTTP body frames read before yielding to the executor.                                                                                                                                          |
| `request.body.eventStreamTimeouts.initialMessage`                         | Disabled        | Bounds the entire first RPC frame before the handler runs, including waiting for its first bytes; this also covers the first ordinary event of a stream-only RPC input. Does not apply to REST streams. |
| `request.body.eventStreamTimeouts.messageCompletion`                      | Disabled        | Deadline from a frame's first bytes until the complete frame is read. Covers all REST frames and RPC frames after the first; never covers the first RPC frame, even if `initialMessage` is disabled. |
| `request.body.eventStreamTimeouts.idle`                                   | Disabled        | Deadline while waiting for a frame's first bytes. Covers the first REST frame and gaps between frames; never covers the first RPC frame, even if `initialMessage` is disabled. |
| `request.body.eventStreamTimeouts.perOperation`                           | `{}`            | Maps full event-stream input operation shape IDs to objects containing `initialMessage`, `messageCompletion`, and/or `idle`; omitted fields inherit the corresponding service setting.                                         |

Timeout values accept duration strings with `ms`, `s`, `m`, or `h` units, such as `"250ms"` or `"10s"`. Numeric `0` or a zero-duration string such as `"0s"` disables a timeout. Positive numeric values, negative durations, and fractional durations are rejected. Set `request.body.maxBytes` or `request.body.readPollBudget` to `0` to disable that limit. The nested `request.body.maxBytes` setting takes precedence over the deprecated `codegen.requestBodyMaxBytes` setting. The old key remains supported when the nested setting is absent; using it emits a warning pointing to `customizationConfig.protocols.common.request.body.maxBytes`. Omitted settings use runtime defaults, so a runtime update can change them without regenerating the service. Deadlines require a Tokio runtime with its time driver enabled.

RPC operations read their first frame before invoking the handler, whether it is
an `initial-request` message or the first ordinary event of a stream-only input.
Only `initialMessage` bounds this read, and its deadline does not reset as bytes
arrive. If it is disabled, enabling `messageCompletion` or `idle` leaves the first
RPC frame without an event-stream read deadline. Configure a nonzero
`initialMessage` to bound senders that withhold bytes or send an incomplete first
frame. Header deadlines do not cover body reads, and whole-body deadlines do not
apply to streaming inputs.

`request.body.maxBytes` accepts integer byte counts or quoted whole-number sizes such as `"100 KB"`, `"100MB"`, or `"1 MiB"`. `KB`, `MB`, `GB`, and `TB` use powers of 1000; `KiB`, `MiB`, `GiB`, and `TiB` use powers of 1024. `B` means bytes. Units are case-sensitive, and surrounding whitespace or whitespace between the amount and unit is optional. Numeric `0` or a zero-size string such as `"0 MB"` means unlimited. Negative values, fractions, unknown units, and amounts exceeding `9_223_372_036_854_775_807` bytes fail generation. Generated Rust stores byte counts as `Option<NonZeroUsize>`; the target must be able to represent the configured count.

For example, the following settings go inside the `rust-server-codegen` plugin configuration, alongside its `service`, `module`, and other required settings. `customizationConfig` is a sibling of `codegen`, not nested inside it:

```json
{
  "codegen": {
    "http-1x": true
  },
  "customizationConfig": {
    "protocols": {
      "common": {
        "request": {
          "header": {
            "readTimeout": "10s"
          },
          "body": {
            "maxBytes": "1 MiB",
            "readTimeouts": {
              "defaultNonPayload": "2m",
              "defaultPayload": "30m",
              "perOperation": {
                "example#Upload": "10m"
              }
            },
            "readPollBudget": 16,
            "eventStreamTimeouts": {
              "initialMessage": "10s",
              "messageCompletion": "30s",
              "idle": "5m",
              "perOperation": {
                "example#PublishEvents": {
                  "idle": 0
                }
              }
            }
          }
        }
      }
    }
  }
}
```

Replace the example operation IDs with full shape IDs from the service model. Body overrides replace the applicable body deadline; event-stream overrides replace only the specified fields and inherit the remaining service values. Set an individual timeout to `0` or `"0s"` to disable it. These JSON settings are selected during SDK generation; they are not read dynamically when the server starts.

Generated request-body allowances and event-stream deadlines are separate sections of the typed `ServiceConfig`, populated from JSON. Body configuration is immutable for each generated SDK.

The generated `sdk::serve(listener, app.into_make_service())` preserves the runtime function's arguments and `Serve` return type and applies this header timeout automatically. Calling `.header_read_timeout(...)` on the returned builder overrides the generated setting, including when enabling or disabling it. Calling the runtime's `aws_smithy_http_server::serve` directly uses its own default. Legacy servers (`codegen.http-1x: false`) have no generated `serve` function and manage their own Hyper accept loop; their typed configuration stores the setting but does not enforce it.

Configure the remaining transport limits and runtime overrides in the service application's Rust startup code through the `serve` builder; these do not require SDK regeneration:

| Builder option                                                   | Default                                                                   | Override                                                                                                                                                                                                                                                                                                         |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `.max_connections(max)`                                          | `16_384` concurrent connections                                           | `0` or `.disable_connection_limit()` disables the limit.                                                                                                                                                                                                                                                         |
| `.header_read_timeout(timeout)`                                  | Generated `request.header.readTimeout`, or 1m when omitted                         | Overrides the JSON setting. `None` or `Some(Duration::ZERO)` disables the deadline.                                                                                                                                                                                                                              |
| `.configure_hyper(callback)`                                     | HTTP/2 keep-alive ping interval and acknowledgment timeout are both `20s` | Set `.http2().timer(hyper_util::rt::TokioTimer::new()).keep_alive_interval(Some(duration)).keep_alive_timeout(duration)` on the custom builder; `keep_alive_interval(None)` disables pings. The callback replaces the default Hyper builder; `.header_read_timeout(...)` still controls HTTP/1 header deadlines. |
| `.with_graceful_shutdown(signal).with_shutdown_timeout(timeout)` | No shutdown timeout; waits indefinitely                                   | Sets how long graceful shutdown waits for connections to finish. Omit `.with_shutdown_timeout(...)` to keep the indefinite wait.                                                                                                                                                                                 |

For example, a service can override the generated header setting and connection limit at startup:

```rust,ignore
use std::time::Duration;

sdk::serve(listener, app.into_make_service())
    .max_connections(4096)
    .header_read_timeout(Some(Duration::from_secs(120)))
    .with_graceful_shutdown(shutdown_signal())
    .with_shutdown_timeout(Duration::from_secs(60))
    .await?;
```

Header deadlines cover the first request head for HTTP/1 and HTTP/2, and subsequent HTTP/1 request heads. The first deadline starts after connection service creation and includes protocol negotiation and header parsing; it does not reset as bytes arrive. It ends before Tower readiness, middleware, or body reads. Later HTTP/2 streams have no header deadline; HTTP/2 keep-alive pings do not provide one. With both protocols enabled, `serve` supports HTTP/1 upgrades; HTTP/1-only upgrades require a custom serve loop. Upgraded connections retain their connection permits.

A body-read or RPC initial-message timeout returns `408` before the handler runs, including when input becomes ready at or after expiry. Deadlines do not limit handler execution. Event-stream completion and idle timeouts surface from `recv()` with `event_stream_message_timeout::MessageTimeoutError` in the source chain. They are resumable: calling `recv()` again grants another timeout window without closing the connection. Bound retries to retain the protection.

Server runtime versions move to `0.69.0` (`aws-smithy-http-server`) and `0.68.0` (`aws-smithy-legacy-http-server`); server metrics and its macro move to `0.4.0`, and the Python server runtime moves to `0.70.0`.
