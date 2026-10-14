---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: true
new_feature: false
bug_fix: true
---

AWS JSON 1.0 and 1.1 servers now ignore REST response bindings by default.
Successful responses use HTTP 200 and modeled client/server errors use HTTP
400/500. Members annotated with `@httpHeader`, `@httpPrefixHeaders`, or
`@httpResponseCode` remain ordinary body members.

To retain historical success statuses, modeled error status overrides, and
response headers, set `customizationConfig.protocols.awsJson1_0.legacyResponseBindings`
and/or `customizationConfig.protocols.awsJson1_1.legacyResponseBindings` to `true`.
Each flag defaults to `false` and applies independently.

RPC v2 CBOR no longer emits modeled REST headers on errors, fixing compilation
of generated error serializers with header-bound members. Its support for
`@httpError` remains unchanged, as do REST JSON and REST XML response bindings.
