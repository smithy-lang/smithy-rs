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

/// This operation tests that the response code defaults to @http's code
@http(method: "GET", uri: "/responseCodeDefault", code: 418)
@httpRequestTests([
    {
        id: "ResponseCodeDefaultTest",
        protocol: "aws.protocols#restJson1",
        uri: "/responseCodeDefault",
        method: "GET",
    }
])
@httpResponseTests([
    {
        id: "ResponseCodeDefaultTest",
        protocol: "aws.protocols#restJson1",
        code: 418,
    }
])
operation ResponseCodeDefault {
    input: ResponseCodeDefaultInput,
    output: ResponseCodeDefaultOutput,
}

@input
structure ResponseCodeDefaultInput {}

@output
structure ResponseCodeDefaultOutput {}

/// This operation tests that @httpResponseCode is @required
/// and is used over @http's code
@http(method: "GET", uri: "/responseCodeRequired", code: 200)
@httpRequestTests([
    {
        id: "ResponseCodeRequiredTest",
        protocol: "aws.protocols#restJson1",
        uri: "/responseCodeRequired",
        method: "GET",
    }
])
@httpResponseTests([
    {
        id: "ResponseCodeRequiredTest",
        protocol: "aws.protocols#restJson1",
        code: 418,
        params: {"responseCode": 418}
    }
])
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
    responseCode: HttpResponseCode,
}

integer HttpResponseCode
