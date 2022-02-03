$version: "1.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1

@restJson1
@title("SimpleService")
service SimpleService {
    version: "2022-01-01",
    operations: [
        StringPayloadOperation,
        IntPayloadOperation,
        BlobPayloadOperation,
    ],
}

@http(uri: "/stringPayloadOperation", method: "GET")
operation StringPayloadOperation {
    input: NoInput,
    output: StringPayloadOperationOutput
}

@http(uri: "/intPayloadOperation", method: "GET")
operation IntPayloadOperation {
    input: NoInput,
    output: IntPayloadOperationOutput
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

structure StringPayloadOperationOutput {
    @httpPayload
    stringPayload: String,
}

structure IntPayloadOperationOutput {
    @httpPayload
    intPayload: Integer,
}

structure BlobPayloadOperationOutput {
    @httpPayload
    blobPayload: StreamingBlob
}
