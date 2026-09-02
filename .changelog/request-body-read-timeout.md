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
Generated servers now apply a request body read timeout by default to mitigate slow request body attacks. If a request body is not fully received before the timeout expires, the server returns `408 Request Timeout` and closes HTTP/1.x connections with `Connection: close`.

The default timeout is 60 seconds. Services can configure the timeout in `smithy-build.json` with `customizationConfig.readTimeouts.defaultMillis`, configure per-operation overrides with `customizationConfig.readTimeouts.operationMillis`, or disable the timeout by setting a timeout value to `0`.
