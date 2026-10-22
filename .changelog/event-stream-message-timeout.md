---
applies_to:
- server
authors:
- drganjoo
references: []
breaking: false
new_feature: true
bug_fix: false
---
Generated servers can now apply independent RPC initial-message, event-completion, and idle deadlines to operations with event stream inputs. The initial deadline bounds the existing pre-handler first-frame read on RPC streams, including inputs with only an event stream; expiry rejects the request with `408 Request Timeout` before the handler runs. The event deadline starts when each later event's first bytes arrive, or when the first REST event starts arriving. A separate idle deadline bounds waiting for the next event's first bytes after a complete frame, or for the first event on REST streams. Event and idle expiry make the handler's `recv()` return an error whose source chain contains `MessageTimeoutError`; `is_idle()` identifies idle expiry. Handlers can retry `recv()` for another window or stop processing the request.

Initial, completion, and idle deadlines are all disabled by default. Configure `customizationConfig.protocols.common.eventStreamTimeouts.initialMessage`, `messageCompletion`, and `idle` in `smithy-build.json`, using duration strings with `ms`, `s`, `m`, or `h` units, or `0` to disable. `eventStreamTimeouts.perOperation` entries can override each value.

Generated servers also now force a cooperative yield after 32 consecutive ready event-stream body frames by default, preventing a fast sender from starving other tasks on the same runtime worker. Configure the threshold via `customizationConfig.protocols.common.eventStreamReadPollBudget`; `0` disables forced yielding.
