$version: "1.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1

@restJson1
@title("SimpleService")
service SimpleService {
    version: "2022-01-01",
    operations: [
        StringPayloadOperation,
        StructurePayloadOperation,
        DocumentPayloadOperation,
        StreamingBlobPayloadOperation,
        BlobPayloadOperation,
    ],
}

@http(uri: "/stringPayloadOperation", method: "GET")
operation StringPayloadOperation {
    input: StringPayloadOperationInput,
    output: StringPayloadOperationOutput
}

@http(uri: "/structurePayloadOperation", method: "GET")
operation StructurePayloadOperation {
    input: StructurePayloadOperationInput,
    output: StructurePayloadOperationOutput
}

@http(uri: "/documentPayloadOperation", method: "GET")
operation DocumentPayloadOperation {
    input: DocumentPayloadOperationInput,
    output: DocumentPayloadOperationOutput
}

@http(uri: "/streamingBlobPayloadOperation", method: "GET")
operation StreamingBlobPayloadOperation {
    input: StreamingBlobPayloadOperationInput,
    output: StreamingBlobPayloadOperationOutput
}

@http(uri: "/blobPayloadOperation", method: "GET")
operation BlobPayloadOperation {
    input: BlobPayloadOperationInput,
    output: BlobPayloadOperationOutput
}

@streaming
blob StreamingBlob

structure MyStructure {
    aString: String,
    anInt: Integer
}

structure StringPayloadOperationOutput {
    @httpPayload
    stringPayload: String,
}

structure StructurePayloadOperationOutput {
    @httpPayload
    structPayload: MyStructure,
}

structure DocumentPayloadOperationOutput {
    @httpPayload
    documentPayload: Document,
}

structure StreamingBlobPayloadOperationOutput {
    @httpPayload
    streamingBlobPayload: StreamingBlob
}

structure BlobPayloadOperationOutput {
    @httpPayload
    BlobPayload: Blob
}

structure StringPayloadOperationInput {
    @httpPayload
    stringPayload: String,
}

structure StructurePayloadOperationInput {
    @httpPayload
    structPayload: MyStructure,
}

structure DocumentPayloadOperationInput {
    @httpPayload
    documentPayload: Document,
}

structure StreamingBlobPayloadOperationInput {
    @httpPayload
    streamingBlobPayload: StreamingBlob
}

structure BlobPayloadOperationInput {
    @httpPayload
    BlobPayload: Blob
}
