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
//     @required
//     recursiveMember: RecursiveShapesInputOutputNested2
// }
//
// structure RecursiveShapesInputOutputNested2 {
//     @required
//     recursiveMember: RecursiveShapesInputOutputNested1,
// }

// list ValidList {
//     member: RecursiveShapesInputOutput
// }
//
// structure RecursiveShapesInputOutput {
//     @required
//     foo: ValidList
// }

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
