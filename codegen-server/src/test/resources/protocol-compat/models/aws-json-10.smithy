$version: "2"

// Declares the AWS JSON 1.0 audit service over shared compatibility operations. The request
// corpus compares parsed handler inputs, success and error responses, and streaming observations
// from generated legacy and schema servers.

namespace compat.audit
use aws.protocols#awsJson1_0
@awsJson1_0
service Audit { version: "1", operations: [MediaResponse, Modeled, Bindings, MediaPrefix, Label, Greedy, Trailing, GreedyTrailing, NumberLabel, TimestampLabel, Body, StringPayload, BlobPayload, StructPayload, DocumentPayload, Response, Sensitive] }
