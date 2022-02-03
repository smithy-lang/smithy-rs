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
        // BlobPayloadOperation,
    ],
}

@http(uri: "/stringPayloadOperation", method: "GET")
operation StringPayloadOperation {
    input: NoInput,
    output: StringPayloadOperationOutput
}

@http(uri: "/structurePayloadOperation", method: "GET")
operation StructurePayloadOperation {
    input: NoInput,
    output: StructurePayloadOperationOutput
}

@http(uri: "/documentPayloadOperation", method: "GET")
operation DocumentPayloadOperation {
    input: NoInput,
    output: DocumentPayloadOperationOutput
}

// @http(uri: "/blobPayloadOperation", method: "GET")
// operation BlobPayloadOperation {
//     input: NoInput,
//     output: BlobPayloadOperationOutput
// }

structure NoInput {
}

// @streaming
// blob StreamingBlob

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

// structure BlobPayloadOperationOutput {
//     @httpPayload
//     blobPayload: StreamingBlob
// }
