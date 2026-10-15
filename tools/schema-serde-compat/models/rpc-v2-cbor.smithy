$version: "2"
namespace compat.audit
use smithy.protocols#rpcv2Cbor
@rpcv2Cbor
service Audit { version: "1", operations: [MediaResponse, Modeled, Stream, StreamingBlob, Bindings, MediaPrefix, Label, Greedy, Trailing, GreedyTrailing, NumberLabel, TimestampLabel, Body, StringPayload, BlobPayload, StructPayload, Response, Sensitive] }
