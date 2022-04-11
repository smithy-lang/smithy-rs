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
    // input: RecursiveShapesInputOutput,
    // output: RecursiveShapesInputOutput,
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

    optConB: ConB,

    conBList: ConBList,

    conBMap: ConBMap
}

structure ConB {
    @required
    nice: String,
    @required
    int: Integer,

    optNice: String,
    optInt: Integer
}

// structure RecursiveShapesInputOutput {
//     nested: RecursiveShapesInputOutputNested1
// }
//
// structure RecursiveShapesInputOutputNested1 {
//     foo: String,
//     nested: RecursiveShapesInputOutputNested2
// }
//
// structure RecursiveShapesInputOutputNested2 {
//     bar: StringList,
//     recursiveMember: RecursiveShapesInputOutputNested1,
// }
//

list ConBList {
    member: AnotherList
}

list AnotherList {
    member: ConB
}

map ConBMap {
    key: String,
    value: AnotherMap
}

map AnotherMap {
    key: String,
    value: ConBList
}
