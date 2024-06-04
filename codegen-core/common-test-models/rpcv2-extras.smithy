$version: "2.0"

namespace smithy.protocoltests.rpcv2Cbor

use smithy.framework#ValidationException
use smithy.protocols#rpcv2Cbor
use smithy.test#httpResponseTests


@rpcv2Cbor
service RpcV2Service {
    operations: [
        SimpleStructOperation,
        ComplexStructOperation
    ]
}

// TODO RpcV2 should not use the `@http` trait.
@http(uri: "/simple-struct-operation", method: "POST")
operation SimpleStructOperation {
    input: SimpleStruct
    output: SimpleStruct
    errors: [ValidationException]
}

// TODO RpcV2 should not use the `@http` trait.
@http(uri: "/complex-struct-operation", method: "POST")
operation ComplexStructOperation {
    input: ComplexStruct
    output: ComplexStruct
    errors: [ValidationException]
}

apply SimpleStructOperation @httpResponseTests([
    {
        id: "SimpleStruct",
        protocol: "smithy.protocols#rpcv2Cbor",
        code: 200, // Not used.
        params: {
            blob: "blobby blob",
            boolean: false,

            string: "There are three things all wise men fear: the sea in storm, a night with no moon, and the anger of a gentle man.",

            byte: 69,
            short: 70,
            integer: 71,
            long: 72,

            float: 0.69,
            double: 0.6969,

            timestamp: 1546300800,
            // document: {
            //     documentInteger: 69
            // }
            enum: "DIAMOND"

            // With `@required`.

            requiredBlob: "blobby blob",
            requiredBoolean: false,

            requiredString: "There are three things all wise men fear: the sea in storm, a night with no moon, and the anger of a gentle man.",

            requiredByte: 69,
            requiredShort: 70,
            requiredInteger: 71,
            requiredLong: 72,

            requiredFloat: 0.69,
            requiredDouble: 0.6969,

            requiredTimestamp: 1546300800,
            // document: {
            //     documentInteger: 69
            // }
            requiredEnum: "DIAMOND"
        }
    },
    // Same test, but leave optional types empty
    {
        id: "SimpleStructWithOptionsSetToNone",
        protocol: "smithy.protocols#rpcv2Cbor",
        code: 200, // Not used.
        params: {
            requiredBlob: "blobby blob",
            requiredBoolean: false,

            requiredString: "There are three things all wise men fear: the sea in storm, a night with no moon, and the anger of a gentle man.",

            requiredByte: 69,
            requiredShort: 70,
            requiredInteger: 71,
            requiredLong: 72,

            requiredFloat: 0.69,
            requiredDouble: 0.6969,

            requiredTimestamp: 1546300800,
            // document: {
            //     documentInteger: 69
            // }
            requiredEnum: "DIAMOND"
        }
    }
])

structure SimpleStruct {
    blob: Blob
    boolean: Boolean

    string: String

    byte: Byte
    short: Short
    integer: Integer
    long: Long

    float: Float
    double: Double

    timestamp: Timestamp
    // document: MyDocument
    enum: Suit

    // With `@required`.

    @required requiredBlob: Blob
    @required requiredBoolean: Boolean

    @required requiredString: String

    @required requiredByte: Byte
    @required requiredShort: Short
    @required requiredInteger: Integer
    @required requiredLong: Long

    @required requiredFloat: Float
    @required requiredDouble: Double

    @required requiredTimestamp: Timestamp
    // @required requiredDocument: MyDocument
    @required requiredEnum: Suit
}

structure ComplexStruct {
    structure: SimpleStruct
    list: SimpleList
    map: SimpleMap
    union: SimpleUnion

    structureList: StructList

    // `@required` for good measure here.
    @required complexList: ComplexList
    @required complexMap: ComplexMap
    @required complexUnion: ComplexUnion
}

list StructList {
    member: SimpleStruct
}

list SimpleList {
    member: String
}

map SimpleMap {
    key: String
    value: Integer
}

// TODO Cut ticket to Smithy: their protocol tests don't have unions
union SimpleUnion {
    blob: Blob
    boolean: Boolean
    string: String
}

list ComplexList {
    member: ComplexMap
}

map ComplexMap {
    key: String
    value: ComplexUnion
}

union ComplexUnion {
    // Recursive path here.
    complexStruct: ComplexStruct

    structure: SimpleStruct
    list: SimpleList
    map: SimpleMap
    union: SimpleUnion
}

enum Suit {
    DIAMOND
    CLUB
    HEART
    SPADE
}

// TODO Documents are not supported in RPC v2 CBOR.
// document MyDocument
