---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: true
new_feature: false
bug_fix: true
---

AWS JSON 1.0 and 1.1 servers now ignore REST response bindings.
Successful responses use HTTP 200 and modeled client/server errors use HTTP
400/500. Members annotated with `@httpHeader`, `@httpPrefixHeaders`, or
`@httpResponseCode` remain ordinary body members.

The same behavior applies when `codegen.schemaSerde` is enabled.
There is no historical response-binding switch.

RPC v2 CBOR no longer emits modeled REST headers on errors, fixing compilation
of generated error serializers with header-bound members. Its support for
`@httpError` remains unchanged, as do REST JSON and REST XML response bindings.
