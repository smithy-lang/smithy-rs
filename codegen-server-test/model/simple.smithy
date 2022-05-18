$version: "1.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1

@restJson1
@title("SimpleService")
service SimpleService {
    operations: [
        //AnOperation,
        QueryParamsTargetingMapOfLengthString,
        QueryParamsTargetingMapOfListOfLengthString,
        QueryParamsTargetingMapOfSetOfLengthString,
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

@http(uri: "/query-params-targeting-map-of-length-string", method: "GET")
operation QueryParamsTargetingMapOfLengthString {
    input: QueryParamsTargetingMapOfLengthStringInputOutput,
    output: QueryParamsTargetingMapOfLengthStringInputOutput,
}

@http(uri: "/query-params-targeting-map-of-list-of-length-string", method: "GET")
operation QueryParamsTargetingMapOfListOfLengthString {
    input: QueryParamsTargetingMapOfListOfLengthStringInputOutput,
    output: QueryParamsTargetingMapOfListOfLengthStringInputOutput,
}

@http(uri: "/query-params-targeting-map-of-set-of-length-string", method: "GET")
operation QueryParamsTargetingMapOfSetOfLengthString {
    input: QueryParamsTargetingMapOfSetOfLengthStringInputOutput,
    output: QueryParamsTargetingMapOfSetOfLengthStringInputOutput,
}

structure QueryParamsTargetingMapOfLengthStringInputOutput {
    @httpQueryParams
    mapOfLengthString: MapOfLengthString
}

structure QueryParamsTargetingMapOfListOfLengthStringInputOutput {
    @httpQueryParams
    mapOfListOfLengthString: MapOfListOfLengthString
}

structure QueryParamsTargetingMapOfSetOfLengthStringInputOutput {
    @httpQueryParams
    mapOfSetOfLengthString: MapOfSetOfLengthString
}

structure AnOperationInput {
    @required
    conA: ConA,

    //  Only top-level members of an operation's input structure are considered
    //  when deserializing HTTP messages.

    // TODO Test with mediaType trait too.
    // @httpHeader("X-Length")
    // lengthStringHeader: LengthString,

    // @httpHeader("X-Length-Set")
    // lengthStringSetHeader: LengthStringSet,

    // @httpHeader("X-Length-List")
    // lengthStringListHeader: LengthStringList,

    // // TODO(https://github.com/awslabs/smithy-rs/issues/1394) `@required` not working
    // // @required
    // @httpPrefixHeaders("X-Prefix-Headers-")
    // lengthStringHeaderMap: LengthStringHeaderMap,

    @httpQuery("lengthString")
    lengthStringQuery: LengthString,

    @httpQuery("lengthStringList")
    lengthStringListQuery: LengthStringList,

    @httpQuery("lengthStringSet")
    lengthStringSetQuery: LengthStringSet,
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

// TODO Use MapOfLengthString
map LengthStringHeaderMap {
    key: LengthString,
    value: LengthString,
}

map MapOfLengthString {
    key: LengthString,
    value: LengthString,
}

map MapOfListOfLengthString {
    key: LengthString,
    value: LengthStringList,
}

map MapOfSetOfLengthString {
    key: LengthString,
    value: LengthStringSet,
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

// TODO Rename these to ListOfLengthString, SetOfLengthString
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
