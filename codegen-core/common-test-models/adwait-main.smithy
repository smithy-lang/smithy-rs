$version: "2.0"

namespace aws.protocoltests.rpcv2
use aws.api#service
use smithy.protocols#rpcv2
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@service(sdkId: "Sample RpcV2 Protocol")
@rpcv2(format: ["cbor"])
@title("RpcV2 Protocol Service")
service RpcV2Protocol {
    version: "2020-07-14",
    operations: [
        //Basic input/output tests
        NoInputOutput,
        EmptyInputOutput,
        OptionalInputOutput,

        SimpleScalarProperties,
    ]
}

structure EmptyStructure {

}

structure SimpleStructure {
    value: String,
}