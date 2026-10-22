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
Servers now bound slow and oversized clients by default: request body reads time out with `408` (`5m` for non-payload operations, `1h` for `@httpPayload`), `serve` caps concurrent connections at 16,384 and HTTP version detection at 30s, and event stream reads yield after 32 consecutive ready frames (`requestBodyReadPollBudget`). Event stream `initialMessage`, `messageCompletion`, and `idle` deadlines are opt-in. All settings live under `customizationConfig.protocols.common`, including `requestBodyMaxBytes`, which takes precedence over the legacy `codegen.requestBodyMaxBytes`. Body read deadlines require a tokio runtime with the time driver enabled.

Deadlines bound reads from the client, not handler execution. A body read or RPC `initialMessage` timeout rejects the request with `408` before the handler runs. When an event stream message starts arriving but does not complete within `messageCompletion`, or no message arrives within `idle`, the receiver's `recv()` returns an error whose source chain contains `event_stream_message_timeout::MessageTimeoutError` (`is_idle()` identifies idle expiry). The error is resumable and does not close the connection: call `recv()` again to grant one more timeout window, or propagate it to end the request. Retry only a bounded number of times; retrying indefinitely removes the protection.
