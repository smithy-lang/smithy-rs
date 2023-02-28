$version: "2.0"

namespace com.amazonaws.simple

use smithy.protocols#rpcv2

@rpcv2(format: ["cbor"])
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
