$version: "2.0"

namespace com.amazonaws.simple

use smithy.framework#ValidationException
use smithy.protocols#rpcv2

@rpcv2(format: ["cbor"])
service RpcV2Service {
    version: "SomeVersion",
    operations: [RpcV2Operation],
}

@http(uri: "/operation/{message}", method: "GET")
operation RpcV2Operation {
    input: OperationInput
    output: OperationOutput
    errors: [ValidationException]
}

structure OperationInput {
    @required
    @httpLabel
    message: String
}

structure OperationOutput {
    message: String
}
