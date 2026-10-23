$version: "2"
namespace compat.audit
use aws.protocols#awsJson1_0
@awsJson1_0
service Audit { version: "1", operations: [MediaResponse, Modeled, Bindings, MediaPrefix, Label, Greedy, Trailing, GreedyTrailing, NumberLabel, TimestampLabel, Body, StringPayload, BlobPayload, StructPayload, DocumentPayload, Response, Sensitive] }
