---
applies_to: ["server"]
authors: ["fahadzub"]
references: []
breaking: false
new_feature: false
bug_fix: true
---

Schema-serde RPC v2 CBOR servers now handle blobs marked `@streaming` as ordinary
CBOR document members with the same decoded values as the legacy generated server. These operations
no longer return 404. Responses use `application/cbor` and stream an indefinite-length CBOR byte string
without collecting the `ByteStream` or setting Content-Length; event-stream-only Accept headers are rejected as in
the legacy server.

REST blob payloads continue to stream raw bytes, including when the same service
also serves RPC v2 CBOR. Event-stream unions retain their framed serialization.
Regenerate schema servers with the updated code generator and runtime to use
the document-bound blob input and output glue.
