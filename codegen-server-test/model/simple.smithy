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
    setA: SetA,

    setC: SetC,

    setD: SetD,

    setE: SetE
}

structure AnOperationOutput {
    set: SetA
}

set SetA {
    member: SetB
}

set SetB {
    member: String
}

set SetC {
    member: MapA
}

set SetD {
    member: ListA
}

set SetE {
    member: StructureA
}

list ListA {
    member: SetA
}

map MapA {
    key: String,
    value: String
}

structure StructureA {
    string: String
}
