---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Schema-serde RPC v2 CBOR servers support streaming blob outputs as CBOR document
members. Responses use `application/cbor` and stream an indefinite-length CBOR byte
string without collecting the `ByteStream` or setting Content-Length; event-stream-only
Accept headers are rejected as in the legacy server.

RPC v2 CBOR operations with streaming blob inputs temporarily return an empty 501
before input decoding, body reads, or handler invocation. Input support is deferred
until the completion and validation API is settled. This intentionally differs from
the legacy generated server, which collects these inputs.

REST blob inputs and outputs continue to stream raw bytes, including when the same
service also serves RPC v2 CBOR. Handlers accept their ordinary modeled input with
a `ByteStream` payload; HTTP-bound metadata is available and validated before
handler invocation. Event-stream unions retain their framed serialization.

Operations with event streams on protocols that do not support event streaming
also return an empty 501 before body reads or handler invocation.
