---
applies_to:
- server
authors:
- drganjoo
references: []
breaking: true
new_feature: false
bug_fix: true
---
Generated servers now apply request body read deadlines to non-streaming operations by default to mitigate slow request body attacks. The deadline is removed before the operation handler runs. If the request input is not fully received before the deadline expires, the server returns `408 Request Timeout` and closes HTTP/1.x connections with `Connection: close`.

Services can configure deadlines in `smithy-build.json` with `customizationConfig.protocols.common.requestBodyReadTimeouts`. `defaultNonPayload` defaults to `5m`, while `defaultPayload` defaults to `1h`. Per-operation overrides use `perOperation`. Positive durations require an explicit `ms`, `s`, `m`, or `h` unit, and `0` disables the applicable deadline.

Enforcing a deadline requires a tokio runtime with the time driver enabled (`enable_time` or `enable_all`). On runtimes without a time driver, requests whose input is ready on first poll still succeed; a request that has to wait for body bytes panics when the server arms the deadline. Disable the deadlines via the configuration above if your runtime has no time driver.
