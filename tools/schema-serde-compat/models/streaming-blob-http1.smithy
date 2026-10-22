$version: "2"
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
