## Description:

Add configurable timeouts for:

- **Body collection**
  - Defaults to **5 minutes** for ordinary inputs and **1 hour** for non-streaming `@httpPayload` inputs.
  - Applies while constructing the operation input and ends before the handler runs.
  - Expiry returns `408 Request Timeout`, with `Connection: close` for HTTP/1.x.

- **Event streams**
  - **a. Initial message:** Covers waiting for and completing an initial frame, where the RPC operation has initial-message fields. Expiry rejects the request before the handler runs. **Disabled by default.**
  - **b. Message completion:** Covers completing an event frame after its first bytes arrive. Slow-drip bytes do not reset the deadline. **Disabled by default.**
  - **c. Idle:** Covers waiting for the first event on streams without an initial read, and waiting between events. Defaults to **1 hour**.
  - Completion and idle expiry return `MessageTimeoutError` through the handler's `recv()`. The handler can retry or end processing.

Also default `serve` to **16,384 concurrent TCP connections**, configurable through `.max_connections(...)`.

## Configuration:

Configure the `rust-server-codegen` plugin in `smithy-build.json`:

```json
{
  "customizationConfig": {
    "protocols": {
      "common": {
        "requestBodyReadTimeouts": {
          "defaultNonPayload": "5m",
          "defaultPayload": "1h"
        },
        "eventStreamTimeouts": {
          "initialMessage": "10s",
          "messageCompletion": "30s",
          "idle": "1h"
        }
      }
    }
  }
}
```

This example explicitly enables initial and completion timeouts. Both otherwise default to `0`. Timeout values accept `ms`, `s`, `m`, or `h`; `0` disables a deadline. Both timeout objects support `perOperation` overrides.

These settings do not impose a handler-execution or total event-stream lifetime timeout.
