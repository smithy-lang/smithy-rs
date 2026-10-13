---
applies_to:
- server
authors:
- fahadzub
references: []
breaking: true
new_feature: true
bug_fix: false
---
Generated servers can now apply independent RPC initial-message, event-completion, and idle deadlines to operations with event stream inputs. The initial deadline includes waiting for the first byte and receiving the complete initial frame; expiry rejects the request before the handler runs. The event deadline starts when each event's first bytes arrive. A separate idle deadline defaults to one hour and bounds waiting for the next event's first bytes after a complete frame, or for the first event on REST streams. Event and idle expiry make the handler's `recv()` return an error whose source chain contains `MessageTimeoutError`; `is_idle()` identifies idle expiry. Handlers can retry `recv()` for another window or stop processing the request.

Initial and completion deadlines are disabled by default; idle deadlines are enabled by default. Configure `customizationConfig.protocols.global.eventStreamTimeouts.initialMessage`, `messageCompletion`, and `idle` in `smithy-build.json`, using duration strings with `ms`, `s`, `m`, or `h` units, or `0` to disable. `eventStreamTimeouts.perOperation` entries can override each value.
