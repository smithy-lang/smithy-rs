---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: true
new_feature: true
bug_fix: false
---

All `HttpModeledError` types now implement `IntoResponse<P>` through a runtime blanket
implementation. Middleware can serialize generated operation errors through
`into_response_with_protocol`, using the protocol selected by schema routing, without
additional generated response implementations. Direct conversion without a selected protocol
returns an empty response with the modeled status and a `ModeledErrorExtension`.

Types implementing both `HttpModeledError` and an explicit `IntoResponse<P>` must remove
the explicit response implementation or use a separate custom-response wrapper.

`InternalFailureException` supports `IntoResponse<P>` for protocol tuples of two through ten
elements. Its context-aware conversion uses the selected protocol; direct tuple conversion
returns an empty 500 with a `RuntimeErrorExtension`. Existing concrete-protocol direct
conversions retain their established status, headers, and body.
