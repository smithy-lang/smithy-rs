$version: "2"

// Declares the AWS JSON 1.1 audit service over shared compatibility operations. The request
// corpus exercises bindings, structured bodies, payloads, and errors with the same model and
// requests under legacy and schema serde.

namespace compat.audit
use aws.protocols#awsJson1_1
@awsJson1_1
service Audit { version: "1", operations: [MediaResponse, Modeled, Bindings, MediaPrefix, Label, Greedy, Trailing, GreedyTrailing, NumberLabel, TimestampLabel, Body, StringPayload, BlobPayload, StructPayload, DocumentPayload, Response, Sensitive] }
