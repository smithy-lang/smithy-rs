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
    // errors: [MyError]
}

structure AnOperationInput {
    @required
    conA: ConA,

    //  Only top-level members of an operation's input structure are considered
    //  when deserializing HTTP messages.

    // TODO Test with mediaType trait too.
    @httpHeader("X-Length")
    lengthString: LengthString,

    @httpHeader("X-Length-Set")
    lengthStringSet: LengthStringSet,

    @httpHeader("X-Length-List")
    lengthStringList: LengthStringList,

    @httpHeader("X-String")
    string: String,

    // @required
    @httpPrefixHeaders("X-Required-")
    requiredHeaderMap: HeaderMap,

    //@httpPrefixHeaders("X-Foo-")
    //headerMap: HeaderMap
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

    // conBMap: ConBMap

    // normalString: NormalString,

    // playerAction: PlayerAction,
    // myEnum: MyEnum

    // @length(min:4, max:6)
    // list: LengthList,

    //set: LengthStringSet,
}

map HeaderMap {
    key: LengthString,
    value: LengthString,
}

// @length(min:2, max:8)
// list LengthList {
//     member: LengthString
// }

@length(min:2, max:8)
string LengthString

string NormalString

union PlayerAction {
    /// Quit the game.
    quit: Unit,

    /// Move in a specific direction.
    move: DirectedAction,

    /// Jump in a specific direction.
    jump: DirectedAction
}

structure DirectedAction {
    @required
    direction: Integer
}

@enum([
    {
        value: "t2.nano",
        name: "T2_NANO",
    },
    {
        value: "t2.micro",
        name: "T2_MICRO",
    },
    {
        value: "m256.mega",
        name: "M256_MEGA",
    }
])
string MyEnum

// A set that is not directly constrained, but that has a member that is. There
// is no such example in any of the other test models!
set LengthStringSet {
    member: LengthString
}

list LengthStringList {
    member: LengthString
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

structure RecursiveShapesInputOutput {
    nested: RecursiveShapesInputOutputNested1
}

structure RecursiveShapesInputOutputNested1 {
    @required
    recursiveMember: RecursiveShapesInputOutputNested2
}

structure RecursiveShapesInputOutputNested2 {
    @required
    recursiveMember: RecursiveShapesInputOutputNested1,
}

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

// @length(min: 1, max: 69)
// map ConBMap {
//     key: String,
//     //value: AnotherMap
//     value: NiceString
// }

// @length(min: 1, max: 10)
// string NiceString

// @error("client")
// @retryable
// @httpError(429)
// structure MyError {
//     @required
//     message: NiceString
// }

//
// map AnotherMap {
//     key: String,
//     value: ConBList
//     //value: ConB
// }
