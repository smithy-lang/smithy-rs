$version: "2.0"

namespace smithy.protocoltests.rpcv2Cbor

use smithy.framework#ValidationException
use smithy.protocols#rpcv2Cbor
use smithy.test#httpResponseTests
use smithy.test#httpMalformedRequestTests

@rpcv2Cbor
service RpcV2CborService {
    operations: [
        SimpleStructOperation
        ErrorSerializationOperation
        ComplexStructOperation
        EmptyStructOperation
        SingleMemberStructOperation
        RecursiveUnionOperation,
        StreamingOperation
        StreamingOperationWithInitialData
        StreamingOperationWithInitialResponse
    ]
}

operation StreamingOperation {
    input: StreamingOperationInput,
    output: StreamingOperationOutput,
    errors: [ValidationException]
}

structure StreamingOperationOutput {
    events: Events
}

structure StreamingOperationInput {
    events: Events
}

operation StreamingOperationWithInitialData {
    input: StreamingOperationWithInitialDataInput,
    output: StreamingOperationWithInitialDataOutput,
    errors: [ValidationException]
}

structure StreamingOperationWithInitialDataInput {
    @required
    initialData: String
    events: Events
}

structure StreamingOperationWithInitialDataOutput {
    events: Events
}

operation StreamingOperationWithInitialResponse {
    input: StreamingOperationWithInitialResponseInput,
    output: StreamingOperationWithInitialResponseOutput,
    errors: [ValidationException]
}

structure StreamingOperationWithInitialResponseInput {
    events: Events
}

structure StreamingOperationWithInitialResponseOutput {
    @required
    responseData: String
    events: Events
}


@streaming
union Events {
    A: Event,
    B: Event,
    C: Event,
}

structure Event {
}


// TODO(https://github.com/smithy-lang/smithy/issues/2326): Smithy should not
// allow HTTP binding traits in this protocol.
@http(uri: "/simple-struct-operation", method: "POST")
operation SimpleStructOperation {
    input: SimpleStruct
    output: SimpleStruct
    errors: [ValidationException]
}

operation ErrorSerializationOperation {
    input: SimpleStruct
    output: ErrorSerializationOperationOutput
    errors: [ValidationException]
}

operation ComplexStructOperation {
    input: ComplexStruct
    output: ComplexStruct
    errors: [ValidationException]
}

operation EmptyStructOperation {
    input: EmptyStruct
    output: EmptyStruct
}

operation SingleMemberStructOperation {
    input: SingleMemberStruct
    output: SingleMemberStruct
}

operation RecursiveUnionOperation {
    input: RecursiveOperationInputOutput
    output: RecursiveOperationInputOutput
}

apply EmptyStructOperation @httpMalformedRequestTests([
    {
        id: "AdditionalTokensEmptyStruct",
        documentation: """
        When additional tokens are found past where we expect the end of the body,
        the request should be rejected with a serialization exception.""",
        protocol: rpcv2Cbor,
        request: {
            method: "POST",
            uri: "/service/RpcV2CborService/operation/EmptyStructOperation",
            headers: {
                "smithy-protocol": "rpc-v2-cbor",
                "Accept": "application/cbor",
                "Content-Type": "application/cbor"
            }
            // Two empty variable-length encoded CBOR maps back to back.
            body: "v/+//w=="
        },
        response: {
            code: 400,
            body: {
                mediaType: "application/cbor",
                assertion: {
                    // An empty CBOR map.
                    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3716): we're not serializing `__type` because `SerializationException` is not modeled.
                    contents: "oA=="
                }
            }
        }
    }
])

apply SingleMemberStructOperation @httpMalformedRequestTests([
    {
        id: "AdditionalTokensSingleMemberStruct",
        documentation: """
        When additional tokens are found past where we expect the end of the body,
        the request should be rejected with a serialization exception.""",
        protocol: rpcv2Cbor,
        request: {
            method: "POST",
            uri: "/service/RpcV2CborService/operation/SingleMemberStructOperation",
            headers: {
                "smithy-protocol": "rpc-v2-cbor",
                "Accept": "application/cbor",
                "Content-Type": "application/cbor"
            }
            // Two empty variable-length encoded CBOR maps back to back.
            body: "v/+//w=="
        },
        response: {
            code: 400,
            body: {
                mediaType: "application/cbor",
                assertion: {
                    // An empty CBOR map.
                    // TODO(https://github.com/smithy-lang/smithy-rs/issues/3716): we're not serializing `__type` because `SerializationException` is not modeled.
                    contents: "oA=="
                }
            }
        }
    }
])

apply ErrorSerializationOperation @httpMalformedRequestTests([
    {
        id: "ErrorSerializationIncludesTypeField",
        documentation: """
        When invalid input is provided the request should be rejected with
        a validation exception, and a `__type` field should be included""",
        protocol: rpcv2Cbor,
        request: {
            method: "POST",
            uri: "/service/RpcV2CborService/operation/ErrorSerializationOperation",
            headers: {
                "smithy-protocol": "rpc-v2-cbor",
                "Accept": "application/cbor",
                "Content-Type": "application/cbor"
            }
            // An empty CBOR map. We're missing a lot of `@required` members!
            body: "oA==",
            bodyMediaType: "application/cbor"
        },
        response: {
            code: 400,
            body: {
                mediaType: "application/cbor",
                assertion: {
                    contents: "v2ZfX3R5cGV4JHNtaXRoeS5mcmFtZXdvcmsjVmFsaWRhdGlvbkV4Y2VwdGlvbmdtZXNzYWdleGsxIHZhbGlkYXRpb24gZXJyb3IgZGV0ZWN0ZWQuIFZhbHVlIGF0ICcvcmVxdWlyZWRCbG9iJyBmYWlsZWQgdG8gc2F0aXNmeSBjb25zdHJhaW50OiBNZW1iZXIgbXVzdCBub3QgYmUgbnVsbGlmaWVsZExpc3SBv2RwYXRobS9yZXF1aXJlZEJsb2JnbWVzc2FnZXhOVmFsdWUgYXQgJy9yZXF1aXJlZEJsb2InIGZhaWxlZCB0byBzYXRpc2Z5IGNvbnN0cmFpbnQ6IE1lbWJlciBtdXN0IG5vdCBiZSBudWxs//8="
                }
            }
        }
    }
])

apply ErrorSerializationOperation @httpResponseTests([
    {
        id: "OperationOutputSerializationQuestionablyIncludesTypeField",
        documentation: """
        Despite the operation output being a structure shape with the `@error` trait,
        `__type` field should, in a strict interpretation of the spec, not be included,
        because we're not serializing a server error response. However, we do, because
        there shouldn't™️ be any harm in doing so, and it greatly simplifies the
        code generator. This test just pins this behavior in case we ever modify it.""",
        protocol: rpcv2Cbor,
        code: 200,
        params: {
            errorShape: {
                message: "ValidationException message field"
            }
        }
        bodyMediaType: "application/cbor"
        body: "v2plcnJvclNoYXBlv2ZfX3R5cGV4JHNtaXRoeS5mcmFtZXdvcmsjVmFsaWRhdGlvbkV4Y2VwdGlvbmdtZXNzYWdleCFWYWxpZGF0aW9uRXhjZXB0aW9uIG1lc3NhZ2UgZmllbGT//w=="
    }
])

apply SimpleStructOperation @httpResponseTests([
    {
        id: "SimpleStruct",
        protocol: rpcv2Cbor,
        code: 200, // Not used.
        body: "v2RibG9iS2Jsb2JieSBibG9iZ2Jvb2xlYW70ZnN0cmluZ3hwVGhlcmUgYXJlIHRocmVlIHRoaW5ncyBhbGwgd2lzZSBtZW4gZmVhcjogdGhlIHNlYSBpbiBzdG9ybSwgYSBuaWdodCB3aXRoIG5vIG1vb24sIGFuZCB0aGUgYW5nZXIgb2YgYSBnZW50bGUgbWFuLmRieXRlGEVlc2hvcnQYRmdpbnRlZ2VyGEdkbG9uZxhIZWZsb2F0+j8wo9dmZG91Ymxl+z/mTQE6kqMFaXRpbWVzdGFtcMH7QdcKq2AAAABkZW51bWdESUFNT05EbHJlcXVpcmVkQmxvYktibG9iYnkgYmxvYm9yZXF1aXJlZEJvb2xlYW70bnJlcXVpcmVkU3RyaW5neHBUaGVyZSBhcmUgdGhyZWUgdGhpbmdzIGFsbCB3aXNlIG1lbiBmZWFyOiB0aGUgc2VhIGluIHN0b3JtLCBhIG5pZ2h0IHdpdGggbm8gbW9vbiwgYW5kIHRoZSBhbmdlciBvZiBhIGdlbnRsZSBtYW4ubHJlcXVpcmVkQnl0ZRhFbXJlcXVpcmVkU2hvcnQYRm9yZXF1aXJlZEludGVnZXIYR2xyZXF1aXJlZExvbmcYSG1yZXF1aXJlZEZsb2F0+j8wo9ducmVxdWlyZWREb3VibGX7P+ZNATqSowVxcmVxdWlyZWRUaW1lc3RhbXDB+0HXCqtgAAAAbHJlcXVpcmVkRW51bWdESUFNT05E/w==",
        bodyMediaType: "application/cbor",
        headers: {
            "smithy-protocol": "rpc-v2-cbor",
            "Content-Type": "application/cbor"
        },
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
            requiredEnum: "DIAMOND"
        }
    },
    // Same test, but leave optional types empty
    {
        id: "SimpleStructWithOptionsSetToNone",
        protocol: rpcv2Cbor,
        code: 200, // Not used.
        body: "v2xyZXF1aXJlZEJsb2JLYmxvYmJ5IGJsb2JvcmVxdWlyZWRCb29sZWFu9G5yZXF1aXJlZFN0cmluZ3hwVGhlcmUgYXJlIHRocmVlIHRoaW5ncyBhbGwgd2lzZSBtZW4gZmVhcjogdGhlIHNlYSBpbiBzdG9ybSwgYSBuaWdodCB3aXRoIG5vIG1vb24sIGFuZCB0aGUgYW5nZXIgb2YgYSBnZW50bGUgbWFuLmxyZXF1aXJlZEJ5dGUYRW1yZXF1aXJlZFNob3J0GEZvcmVxdWlyZWRJbnRlZ2VyGEdscmVxdWlyZWRMb25nGEhtcmVxdWlyZWRGbG9hdPo/MKPXbnJlcXVpcmVkRG91Ymxl+z/mTQE6kqMFcXJlcXVpcmVkVGltZXN0YW1wwftB1wqrYAAAAGxyZXF1aXJlZEVudW1nRElBTU9ORP8=",
        bodyMediaType: "application/cbor",
        headers: {
            "smithy-protocol": "rpc-v2-cbor",
            "Content-Type": "application/cbor"
        },
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
            requiredEnum: "DIAMOND"
        }
    }
])

apply RecursiveUnionOperation @httpResponseTests([
    {
        id: "RpcV2CborRecursiveShapesWithUnion",
        documentation: "Serializes recursive structures with union",
        protocol: rpcv2Cbor,
        code: 200,
        body: "v2ZuZXN0ZWS/Y2Zvb2RGb28xZ3ZhcmlhbnS/Z2Nob2ljZTFrT3V0ZXJDaG9pY2X/Zm5lc3RlZL9jYmFyZEJhcjFvcmVjdXJzaXZlTWVtYmVyv2Nmb29kRm9vMmd2YXJpYW50v2djaG9pY2Uyv2Nmb29kRm9vM2ZuZXN0ZWS/Y2JhcmRCYXIzb3JlY3Vyc2l2ZU1lbWJlcr9jZm9vZEZvbzRndmFyaWFudL9nY2hvaWNlMWtJbm5lckNob2ljZf//////Zm5lc3RlZL9jYmFyZEJhcjL//////w==",
        bodyMediaType: "application/cbor",
        headers: {
            "smithy-protocol": "rpc-v2-cbor",
            "Content-Type": "application/cbor"
        },
        params: {
            nested: {
                foo: "Foo1",
                variant: {
                    choice1: "OuterChoice"
                },
                nested: {
                    bar: "Bar1",
                    recursiveMember: {
                        foo: "Foo2",
                        variant: {
                            choice2: {
                                foo: "Foo3",
                                nested: {
                                    bar: "Bar3",
                                    recursiveMember: {
                                        foo: "Foo4",
                                        variant: {
                                            choice1: "InnerChoice"
                                        }
                                    }
                                }
                            }
                        },
                        nested: {
                            bar: "Bar2"
                        }
                    }
                }
            }
        }
    }
])

structure ErrorSerializationOperationOutput {
    errorShape: ValidationException
}

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
    emptyStructure: EmptyStruct
    list: SimpleList
    map: SimpleMap
    union: SimpleUnion
    unitUnion: UnitUnion
    structureList: StructList
    // `@required` for good measure here.
    @required complexList: ComplexList
    @required complexMap: ComplexMap
    @required complexUnion: ComplexUnion
}

structure EmptyStruct {}

structure SingleMemberStruct {
    message: String
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

// TODO(https://github.com/smithy-lang/smithy/issues/2325): Upstream protocol
// test suite doesn't cover unions. While the generated SDK compiles, we're not
// exercising the (de)serializers with actual values.
union SimpleUnion {
    blob: Blob
    boolean: Boolean
    string: String
    unit: Unit
}

union UnitUnion {
    unitA: Unit
    unitB: Unit
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

structure RecursiveOperationInputOutput {
    nested: RecursiveOperationInputOutputNested1
}

structure RecursiveOperationInputOutputNested1 {
    foo: String,
    nested: RecursiveOperationInputOutputNested2,
    variant: FooChoice,
}

structure RecursiveOperationInputOutputNested2 {
    bar: String,
    recursiveMember: RecursiveOperationInputOutputNested1,
}

union FooChoice {
    choice1: String
    choice2: RecursiveOperationInputOutputNested1
}
