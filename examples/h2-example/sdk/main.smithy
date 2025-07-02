$version: "2"

namespace com.test

use aws.protocols#restJson1
use smithy.framework#ValidationException

@title("Sample Service")
@restJson1
service SampleService {
    version: "2024-03-18"
    operations: [
        SampleOperation
    ]
}

@http(uri: "/sample", method: "POST")
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
