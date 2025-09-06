$version: "2"

namespace com.test

use smithy.protocols#rpcv2Cbor
use smithy.framework#ValidationException

@title("Sample Service")
@rpcv2Cbor
service SampleService {
    version: "2024-03-18"
    operations: [
        SampleOperation
    ]
}

operation SampleOperation {
    input:= {
        @required
        inputValue: String
    }
    output:= {
        @required
        result: String
    }
    errors: [ValidationException]
}
