$version: "2"

// Declares the REST JSON audit service using the shared bindings/body operations. Its corpus
// replays valid and malformed requests against legacy and schema servers, recording handler
// inputs, protocol rejections, and complete response observations.

namespace compat.audit
use aws.protocols#restJson1
@restJson1
service Audit { version: "1", operations: [MediaResponse, Modeled, Stream, StreamingBlob, Bindings, MediaPrefix, Label, Greedy, Trailing, GreedyTrailing, NumberLabel, TimestampLabel, Body, StringPayload, BlobPayload, StructPayload, DocumentPayload, Response, Sensitive] }
