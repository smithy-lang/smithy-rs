$version: "1.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1

@restJson1
@title("SimpleService")
service SimpleService {
    version: "2022-01-01",
    operations: [
        StringPayloadOperation,
        StringEnumPayloadOperation,
        StructurePayloadOperation,
        DocumentPayloadOperation,
        StreamingBlobPayloadOperation,
        BlobPayloadOperation,
    ],
}

@http(uri: "/stringPayloadOperation", method: "GET")
operation StringPayloadOperation {
    input: NoInput,
    output: StringPayloadOperationOutput
}

@http(uri: "/stringEnumPayloadOperation", method: "GET")
operation StringEnumPayloadOperation {
    input: NoInput,
    output: StringEnumPayloadOperationOutput
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

@http(uri: "/streamingBlobPayloadOperation", method: "GET")
operation StreamingBlobPayloadOperation {
    input: NoInput,
    output: StreamingBlobPayloadOperationOutput
}

@http(uri: "/blobPayloadOperation", method: "GET")
operation BlobPayloadOperation {
    input: NoInput,
    output: BlobPayloadOperationOutput
}

structure NoInput {
}

@streaming
blob StreamingBlob

@enum([
    {
        value: "t2.nano",
        name: "T2_NANO",
    },
    {
        value: "t2.micro",
        name: "T2_MICRO",
    },
    {
        value: "m256.mega",
        name: "M256_MEGA",
        deprecated: true
    }
])
string StringEnum

structure MyStructure {
    aString: String,
    anInt: Integer
}

structure StringPayloadOperationOutput {
    @httpPayload
    stringPayload: String,
}

structure StringEnumPayloadOperationOutput {
    @httpPayload
    stringEnumPayload: StringEnum,
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
