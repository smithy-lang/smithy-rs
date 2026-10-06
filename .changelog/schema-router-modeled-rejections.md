---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: true
new_feature: false
bug_fix: false
---

Schema-mode protocol routers report rejections as typed, schema-backed errors.
`ProtocolRouter` and `BodyProtocolRouter` expose `type Error: HttpModeledError`;
`route` returns that error and `RouteClaim` is generic over it. Extension protocols
can frame routing errors through their own `serialize_routing_error` implementation.

Built-in AWS JSON, restJson1, and restXml protocols preserve their legacy routing
responses for unknown operations and method mismatches. RPC v2 CBOR retains
protocol-framed modeled routing errors.
