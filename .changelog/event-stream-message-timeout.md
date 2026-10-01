---
applies_to:
- server
authors:
- fahadzub
references: []
breaking: false
new_feature: true
bug_fix: false
---
Generated servers can now apply a per-message completion deadline to operations with event stream inputs to mitigate slow-drip request attacks. Once the first bytes of an event stream message arrive, the complete message frame must arrive before the deadline expires; otherwise the operation handler's `recv()` call returns a timeout error and the stream is terminated. Time spent idle between messages is not subject to the deadline.

The deadline is disabled by default. Services can configure it in `smithy-build.json` via `customizationConfig.requestBodyReadTimeouts.defaultEventStreamMessage` (a duration string with an explicit `ms`, `s`, `m`, or `h` unit, or `0` to disable). `perOperation` entries for event stream operations override the default. The deadline is enforced by a body wrapper generated into the server crate itself; no runtime crates are modified.
