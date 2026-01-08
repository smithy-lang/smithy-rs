$version: "1.0"

namespace aws.protocoltests.misc

use aws.protocols#restJson1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use smithy.framework#ValidationException

/// A service to test miscellaneous aspects of code generation where protocol
/// selection is not relevant. If you want to test something protocol-specific,
/// add it to a separate `[protocol]-extras.smithy`.
@restJson1
@title("MiscService")
service MiscService {
    operations: [
        TypeComplexityOperation,
        RequiredInnerShapeOperation,
        RequiredHeaderCollectionOperation,
        ResponseCodeRequiredOperation,
        ResponseCodeHttpFallbackOperation,
        ResponseCodeDefaultOperation,
    ],
}

/// An operation whose shapes generate complex Rust types.
/// See https://rust-lang.github.io/rust-clippy/master/index.html#type_complexity.
@http(uri: "/typeComplexityOperation", method: "POST")
operation TypeComplexityOperation {
    input: TypeComplexityOperationInputOutput,
    output: TypeComplexityOperationInputOutput,
}

structure TypeComplexityOperationInputOutput {
    list: ListA
}

list ListA {
    member: ListB
}

list ListB {
    member: ListC
}

list ListC {
    member: MapA
}

map MapA {
    key: String,
    value: EmptyStructure
}

/// This operation tests that (de)serializing required values from a nested
/// shape works correctly.
@http(uri: "/requiredInnerShapeOperation", method: "POST")
operation RequiredInnerShapeOperation {
    input: RequiredInnerShapeOperationInputOutput,
    output: RequiredInnerShapeOperationInputOutput,
    errors: [ValidationException],
}

structure RequiredInnerShapeOperationInputOutput {
    inner: InnerShape
}

structure InnerShape {
    @required
    requiredInnerMostShape: InnermostShape
}

structure InnermostShape {
    @required
    aString: String,

    @required
    aBoolean: Boolean,

    @required
    aByte: Byte,

    @required
    aShort: Short,

    @required
    anInt: Integer,

    @required
    aLong: Long,

    @required
    aFloat: Float,

    @required
    aDouble: Double,

     @required
     aBigInteger: BigInteger,

     @required
     aBigDecimal: BigDecimal,

    @required
    aTimestamp: Timestamp,

    @required
    aDocument: Timestamp,

    @required
    aStringList: AStringList,

    @required
    aStringMap: AMap,

    @required
    aStringSet: AStringSet,

    @required
    aBlob: Blob,

    @required
    aUnion: AUnion
}

list AStringList {
    member: String
}

list AStringSet {
    member: String
}

map AMap {
    key: String,
    value: Timestamp
}

union AUnion {
    i32: Integer,
    string: String,
    time: Timestamp,
}

/// This operation tests that the response code defaults to 200 when no other
/// code is set.
@httpResponseTests([
    {
        id: "ResponseCodeDefaultOperation",
        protocol: "aws.protocols#restJson1",
        code: 200,
    }
])
@http(method: "GET", uri: "/responseCodeDefaultOperation")
operation ResponseCodeDefaultOperation {
    input: EmptyStructure,
    output: EmptyStructure,
}

/// This operation tests that the response code defaults to `@http`'s code.
@httpResponseTests([
    {
        id: "ResponseCodeHttpFallbackOperation",
        protocol: "aws.protocols#restJson1",
        code: 201,
        headers: {
            "Content-Length": "2"
        }
    }
])
@http(method: "GET", uri: "/responseCodeHttpFallbackOperation", code: 201)
operation ResponseCodeHttpFallbackOperation {
    input: EmptyStructure,
    output: EmptyStructure,
}

structure EmptyStructure {}

/// This operation tests that `@httpResponseCode` is `@required`
/// and is used over `@http's` code.
@httpResponseTests([
    {
        id: "ResponseCodeRequiredOperation",
        protocol: "aws.protocols#restJson1",
        code: 201,
        params: {"responseCode": 201},
        headers: {
            "Content-Length": "2"
        }
    }
])
@http(method: "GET", uri: "/responseCodeRequiredOperation", code: 200)
operation ResponseCodeRequiredOperation {
    input: EmptyStructure,
    output: ResponseCodeRequiredOutput,
}

@output
structure ResponseCodeRequiredOutput {
    @required
    @httpResponseCode
    responseCode: Integer,
}

@http(uri: "/required-header-collection-operation", method: "GET")
operation RequiredHeaderCollectionOperation {
    input: RequiredHeaderCollectionOperationInputOutput,
    output: RequiredHeaderCollectionOperationInputOutput,
    errors: [ValidationException]
}

structure RequiredHeaderCollectionOperationInputOutput {
    @required
    @httpHeader("X-Required-List")
    requiredHeaderList: HeaderList,

    @required
    @httpHeader("X-Required-Set")
    requiredHeaderSet: HeaderSet,
}

list HeaderList {
    member: String
}

@uniqueItems
list HeaderSet {
    member: String
}
