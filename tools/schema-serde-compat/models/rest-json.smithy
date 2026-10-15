$version: "2"
namespace compat.audit
use aws.protocols#restJson1
@restJson1
service Audit { version: "1", operations: [MediaResponse, Modeled, Stream, StreamingBlob, Bindings, MediaPrefix, Label, Greedy, Trailing, GreedyTrailing, NumberLabel, TimestampLabel, Body, StringPayload, BlobPayload, StructPayload, DocumentPayload, Response, Sensitive] }
