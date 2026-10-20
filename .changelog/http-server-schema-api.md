---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: true
new_feature: false
bug_fix: false
---

`aws-smithy-http-server` 0.68 introduces schema-based server protocol APIs.
Protocol types such as `RestJson1` are no longer unit structs: construct them
with `Default::default()` instead of using the type name as a value, and replace
unit-struct patterns. The legacy server
runtime retains its unit protocol markers.

Modeled errors implementing `schema::HttpModeledError` receive a blanket
`IntoResponse<P>` implementation. Remove overlapping custom implementations
of `IntoResponse<P>` for those types; customize modeled-error serialization
through the schema protocol APIs. `std::convert::Infallible` remains supported
through this blanket implementation. For modeled errors, use
`into_response_with_protocol` to retain the selected protocol's headers and body;
the context-free conversion supplies only the status and error extension.

Public schema APIs expose `aws-smithy-schema` types. Applications and custom
protocol implementations must use a compatible version of that crate with
their server runtime and generated code.

The new `RouteClaim`, `BodyRouteClaim`, `SharedProtocolRouter`, and
`ProtocolOrder` enums are non-exhaustive. Downstream matches must include a
fallback arm so that future variants can be added.
