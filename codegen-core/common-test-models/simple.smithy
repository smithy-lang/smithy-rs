$version: "2.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1

@restJson1
service SimpleService {
    operations: [
        Operation
    ]
}

@http(uri: "/operation", method: "POST")
operation Operation {
    input: OperationInputOutput
    output: OperationInputOutput
}

structure OperationInputOutput {
    message: String
}
