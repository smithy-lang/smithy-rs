$version: "1.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1

@restJson1
@title("SimpleService")
service SimpleService {
    operations: [
        AnOperation,
    ],
}

@http(uri: "/foo/{greedy+}/suffix", method: "GET")
operation AnOperation {
    input: AnOperationInput,
    output: AnOperationOutput,
    errors: [MyError]
}

structure AnOperationInput {
    @httpResponseCode
    responseCode: Integer,

    @required
    @httpLabel
    greedy: String
}

structure AnOperationOutput {
    @httpResponseCode
    responseCode: Integer
}

@error("client")
@httpError(404)
structure MyError {
    @httpResponseCode
    responseCode: Integer
}
