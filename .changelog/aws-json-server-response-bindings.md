---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

AWS JSON 1.0 and 1.1 servers keep their historical response bindings (the operation's
`@http(code)` status, `@httpError` statuses, and `@httpHeader` members as headers) with or
without `codegen.schemaSerde`. With `schemaSerde` enabled, set
`customizationConfig.protocols.awsJson1_0.responseBindings` and/or
`customizationConfig.protocols.awsJson1_1.responseBindings` to `"protocol"` to follow the AWS JSON
specification instead: status 200 (errors use the `@error` class status) and every member in the
response document with no REST headers. The setting defaults to `"historical"` and applies
independently per protocol. REST JSON, REST XML and RPC v2 CBOR are unaffected.
