---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

RPC v2 CBOR servers can generate modeled errors containing `@httpHeader`
members. Those members serialize in the CBOR document, following the protocol's
body-only operation bindings. Previously the generated error response failed
to compile because header serialization used an incompatible rejection type.
