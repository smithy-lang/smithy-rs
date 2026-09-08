---
applies_to:
- server
authors:
- fahadzub
references: []
breaking: true
new_feature: false
bug_fix: true
---
Generated servers now apply request body read deadlines to non-streaming operations by default to mitigate slow request body attacks. The deadline is removed before the operation handler runs. If the request input is not fully received before the deadline expires, the server returns `408 Request Timeout` and closes HTTP/1.x connections with `Connection: close`.

Services can configure deadlines in `smithy-build.json` with `customizationConfig.requestBodyReadTimeouts`. `defaultNonPayload` defaults to `1m`, while `defaultPayload` defaults to `10h`. Per-operation overrides use `perOperation`. Positive durations require an explicit `ms`, `s`, `m`, or `h` unit, and `0` disables the applicable deadline.
