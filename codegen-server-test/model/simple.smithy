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
    // conA: ConA,

    // conCList: ConCList,
}

structure ConA {
    // @required
    // conB: ConB,

    // optConB: ConB,

    // conBList: ConBList,
    // conBList2: ConBList2,

    // conBSet: ConBSet,

    conBMap: ConBMap
}

// structure ConB {
//     @required
//     nice: String,
//     @required
//     int: Integer,
//
//     optNice: String,
//     optInt: Integer
// }

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

// list ConBList {
//     member: AnotherList
// }

// list ConBList2 {
//     member: ConB
// }

// list ConCList {
//     member: AnotherList
// }
//
// list AnotherList {
//     member: ConB
// }
//
// set ConBSet {
//     member: AnotherSet
// }
//
// set AnotherSet {
//     member: String
// }
//

@length(min: 1, max: 69)
map ConBMap {
    key: String,
    //value: AnotherMap
    value: NiceString
}

@length(min: 1, max: 10)
string NiceString

//
// map AnotherMap {
//     key: String,
//     value: ConBList
//     //value: ConB
// }
