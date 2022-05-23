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

@http(uri: "/operation", method: "GET")
operation AnOperation {
    input: AnOperationInputOutput,
    output: AnOperationInputOutput,
}

structure AnOperationInputOutput {
    int: Integer
}
