---
applies_to: ["server"]
authors: ["drganjoo"]
references: []
breaking: false
new_feature: true
bug_fix: false
---

Add `aws_smithy_http_server::schema`, the runtime half of schema-driven server serialization.
`ServerProtocol` is implemented on the existing `RestJson1`, `RestXml`, `AwsJson1_0`, `AwsJson1_1`
and `RpcV2Cbor` markers and turns a `ServerRequest` — the canonical, transport-independent view of
a collected request — into a `ShapeDeserializer` for the operation input (running the protocol's
`Accept` and `Content-Type` gates and resolving `@httpLabel`, `@httpQuery`, `@httpHeader`,
`@httpPrefixHeaders` and `@httpPayload` bindings on the REST protocols), and an operation output
or modeled error into an HTTP response with each protocol's existing error framing.
`DynServerProtocol` is its erased view. `DeserializableShape`, `ModeledError` and
`HttpModeledError` are the traits generated code will implement, and every request-deserialization
failure travels in the protocol-independent `DeserializeError`, which each protocol renders in
`serialize_rejection` byte-for-byte as the generated servers answer today — including awsJson and
rpcv2Cbor collapsing `Accept` and `Content-Type` failures into a plain 400, and restXml dropping
the validation body. The existing rejection and runtime error enums are unchanged, and the
protocol markers now derive `Default`, `Clone` and `Copy`. Nothing changes for generated servers
that do not use the schema path.
