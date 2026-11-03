$version: "2"

// Defines REST JSON and CBOR blob echo services for HTTP 1.x generation tests. The handlers
// check input bytes and response payloads, protecting legacy generated serde after changes
// to the body types used by HTTP 1.x.

namespace compat.streamingblob

@aws.protocols#restJson1
service RestBlob { version: "1", operations: [BlobEcho] }

@smithy.protocols#rpcv2Cbor
service CborBlob { version: "1", operations: [BlobEcho] }

@http(method: "POST", uri: "/blob")
operation BlobEcho {
    input := { @required @httpPayload data: StreamingBlob }
    output := { @required @httpPayload data: StreamingBlob }
    errors: [smithy.framework#ValidationException]
}

@streaming
blob StreamingBlob
