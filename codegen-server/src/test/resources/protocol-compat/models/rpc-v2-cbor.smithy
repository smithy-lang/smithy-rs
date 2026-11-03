$version: "2"

// Declares the rpcv2Cbor audit service over the CBOR-specific shared shapes. The corpus checks
// RPC routing, decoded input values, error responses, payloads, and streaming observations
// under independently generated legacy and schema servers.

namespace compat.audit
use smithy.protocols#rpcv2Cbor
@rpcv2Cbor
service Audit { version: "1", operations: [MediaResponse, Modeled, Stream, StreamingBlob, Bindings, MediaPrefix, Label, Greedy, Trailing, GreedyTrailing, NumberLabel, TimestampLabel, Body, StringPayload, BlobPayload, StructPayload, Response, Sensitive] }
