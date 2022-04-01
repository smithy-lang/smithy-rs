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
    input: AnOperationInput,
    output: AnOperationOutput,
}

structure AnOperationInput {
    @required
    conA: ConA
}

structure AnOperationOutput {
    conA: ConA
}

structure ConA {
    @required
    conB: ConB,

    optConB: ConB
}

structure ConB {
    @required
    nice: String,
    @required
    int: Integer,

    optNice: String,
    optInt: Integer
}
