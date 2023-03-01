$version: "2.0"

namespace com.amazonaws.simple

use smithy.protocols#rpcv2

@rpcv2(format: ["cbor"])
service RpcV2Service {
    version: "SomeVersion",
    operations: [RpcV2Operation],
}

@http(uri: "/operation", method: "POST")
operation RpcV2Operation {
    input: OperationInputOutput
    output: OperationInputOutput
}

structure OperationInputOutput {
    message: String
}
