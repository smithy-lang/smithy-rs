$version: "1.0"

namespace aws.protocoltests.misc

use aws.protocols#restJson1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

/// A service to test miscellaneous aspects of code generation where protocol
/// selection is not relevant. If you want to test something protocol-specific,
/// add it to a separate `<protocol>-extras.smithy`.
@restJson1
@title("MiscService")
service MiscService {
    operations: [
        OperationWithInnerRequiredShape,
        ResponseCodeRequired,
        ResponseCodeHttpFallback,
        ResponseCodeDefault,
    ],
}

/// This operation tests that (de)serializing required values from a nested
/// shape works correctly.
@http(uri: "/operation", method: "GET")
operation OperationWithInnerRequiredShape {
    input: OperationWithInnerRequiredShapeInput,
    output: OperationWithInnerRequiredShapeOutput,
}

structure OperationWithInnerRequiredShapeInput {
    inner: InnerShape
}

structure OperationWithInnerRequiredShapeOutput {
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

    // TODO(https://github.com/awslabs/smithy-rs/issues/312)
    // @required
    // aBigInteger: BigInteger,

    // @required
    // aBigDecimal: BigDecimal,

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

/// This operation tests that the response code defaults to 200 when no other code is set
@httpResponseTests([
    {
        id: "ResponseCodeDefault",
        protocol: "aws.protocols#restJson1",
        code: 200,
    }
])
@http(method: "GET", uri: "/responseCodeDefault")
operation ResponseCodeDefault {
    input: ResponseCodeDefaultInput,
    output: ResponseCodeDefaultOutput,
}

@input
structure ResponseCodeDefaultInput {}

@output
structure ResponseCodeDefaultOutput {}

/// This operation tests that the response code defaults to @http's code
@httpResponseTests([
    {
        id: "ResponseCodeHttpFallback",
        protocol: "aws.protocols#restJson1",
        code: 418,
        headers: {
            "Content-Length": "2"
        }
    }
])
@http(method: "GET", uri: "/responseCodeHttpFallback", code: 418)
operation ResponseCodeHttpFallback {
    input: ResponseCodeHttpFallbackInput,
    output: ResponseCodeHttpFallbackOutput,
}

@input
structure ResponseCodeHttpFallbackInput {}

@output
structure ResponseCodeHttpFallbackOutput {}

/// This operation tests that @httpResponseCode is @required
/// and is used over @http's code
@httpResponseTests([
    {
        id: "ResponseCodeRequired",
        protocol: "aws.protocols#restJson1",
        code: 201,
        params: {"responseCode": 201},
        headers: {
            "Content-Length": "2"
        }
    }
])
@http(method: "GET", uri: "/responseCodeRequired", code: 200)
operation ResponseCodeRequired {
    input: ResponseCodeRequiredInput,
    output: ResponseCodeRequiredOutput,
}

@input
structure ResponseCodeRequiredInput {}

@output
structure ResponseCodeRequiredOutput {
    @required
    @httpResponseCode
    responseCode: Integer,
}
