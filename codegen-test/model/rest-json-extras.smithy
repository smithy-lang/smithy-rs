$version: "1.0"

namespace aws.protocoltests.restjson

use aws.protocols#restJson1
use aws.api#service
use smithy.test#httpMalformedRequestTests
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

apply QueryPrecedence @httpRequestTests([
    {
        id: "UrlParamsKeyEncoding",
        documentation: "Keys and values must be url encoded",
        protocol: restJson1,
        method: "POST",
        uri: "/Precedence",
        body: "",
        queryParams: ["bar=%26%F0%9F%90%B1", "hello%20there=how%27s%20your%20encoding%3F", "a%20%26%20b%20%26%20c=better%20encode%20%3D%20this"],
        params: {
            foo: "&🐱",
            baz: {
                "hello there": "how's your encoding?",
                "a & b & c": "better encode = this"
            }
        },
        appliesTo: "client",
    },
    {
        id: "RestJsonQueryPrecedenceForbid",
        documentation: "Prefer named query parameters when serializing",
        protocol: restJson1,
        method: "POST",
        uri: "/Precedence",
        body: "",
        queryParams: [
            "bar=named",
            "qux=alsoFromMap"
        ],
        forbidQueryParams: ["bar=fromMap"],
        params: {
            foo: "named",
            baz: {
                bar: "fromMap",
                qux: "alsoFromMap"
            }
        },
        appliesTo: "client",
    }]
)

/// A REST JSON service that sends JSON requests and responses.
@service(sdkId: "Rest Json Protocol")
@restJson1
service RestJsonExtras {
    version: "2019-12-16",
    operations: [
        StringPayload,
        PrimitiveIntHeader,
        EnumQuery,
        StatusResponse,
        MapWithEnumKeyOp,
        PrimitiveIntOp,
        EscapedStringValues,
        NullInNonSparse,
        CaseInsensitiveErrorOperation,
        EmptyStructWithContentOnWireOp,
        // TODO(https://github.com/awslabs/smithy/pull/1042): Remove this once the test case in Smithy is fixed
        PostPlayerAction,
        FixedMalformedAcceptWithGenericString,
        FixedRegisterService,
    ],
    errors: [ExtraError]
}

@httpResponseTests([
    {
        documentation: "Upper case error modeled lower case",
        id: "ServiceLevelError",
        protocol: "aws.protocols#restJson1",
        code: 500,
        body: "",
        headers: { "X-Amzn-Errortype": "ExtraError" },
        params: {},
        appliesTo: "client",
    }
])
@error("server")
@error("server")
structure ExtraError {}

@http(uri: "/StringPayload", method: "POST")
@httpRequestTests([
    {
        id: "StringPayload",
        uri: "/StringPayload",
        body: "rawstring",
        params: { payload: "rawstring" },
        method: "POST",
        protocol: "aws.protocols#restJson1",
        appliesTo: "client",
    }
])
operation StringPayload {
    input: StringPayloadInput,
    output: StringPayloadInput
}

structure StringPayloadInput {
    @httpPayload
    payload: String
}

@httpRequestTests([{
    id: "SerPrimitiveInt",
    protocol: "aws.protocols#restJson1",
    documentation: "Primitive ints should not be serialized when they are unset",
    uri: "/primitive-document",
    method: "POST",
    body: "{}",
    params: {},
    appliesTo: "client",
}])
@http(uri: "/primitive-document", method: "POST")
operation PrimitiveIntOp {
    input: PrimitiveIntDocument,
    output: PrimitiveIntDocument
}

structure PrimitiveIntDocument {
    value: PrimitiveInt
}

@httpResponseTests([
    {
        id: "DeserPrimitiveHeader",
        protocol: "aws.protocols#restJson1",
        code: 200,
        headers: { "x-field": "123" },
        params: { field: 123 },
        appliesTo: "client",
    },
    {
        id: "DeserPrimitiveHeaderMissing",
        protocol: "aws.protocols#restJson1",
        code: 200,
        headers: { },
        params: { field: 0 },
        appliesTo: "client",
    }
])
@http(uri: "/primitive", method: "POST")
operation PrimitiveIntHeader {
    output: PrimitiveIntHeaderInput
}

integer PrimitiveInt

structure PrimitiveIntHeaderInput {
    @httpHeader("x-field")
    @required
    field: PrimitiveInt
}

@http(uri: "/foo/{enum}", method: "GET")
@httpRequestTests([
    {
        id: "EnumQueryRequest",
        uri: "/foo/enumvalue",
        params: { enum: "enumvalue" },
        method: "GET",
        protocol: "aws.protocols#restJson1",
        appliesTo: "client",
    }
])
operation EnumQuery {
    input: EnumQueryInput
}

structure EnumQueryInput {
    @httpLabel
    @required
    enum: StringEnum
}

@http(uri: "/", method: "POST")
operation StatusResponse {
    output: StatusOutput
}

structure StatusOutput {
    @httpResponseCode
    field: PrimitiveInt
}

map MapWithEnumKey {
    key: StringEnum,
    value: String,
}

structure MapWithEnumKeyInputOutput {
    map: MapWithEnumKey,
}

@http(uri: "/map-with-enum-key", method: "POST")
@httpRequestTests([
    {
        id: "MapWithEnumKeyRequest",
        uri: "/map-with-enum-key",
        method: "POST",
        protocol: "aws.protocols#restJson1",
        body: "{\"map\":{\"enumvalue\":\"something\"}}",
        params: { map: { "enumvalue": "something" } },
        appliesTo: "client",
    },
])
@httpResponseTests([
    {
        id: "MapWithEnumKeyResponse",
        protocol: "aws.protocols#restJson1",
        code: 200,
        body: "{\"map\":{\"enumvalue\":\"something\"}}",
        params: { map: { "enumvalue": "something" } },
        appliesTo: "client",
    },
])
operation MapWithEnumKeyOp {
    input: MapWithEnumKeyInputOutput,
    output: MapWithEnumKeyInputOutput,
}


@enum([
    { value: "has\"quotes", name: "HAS_QUOTES", documentation: "this needs#tobe escaped" },
    { value: "normal", name: "NORMAL" },
])
string EnumWithEscapedChars

structure EscapedStringValuesInputOutput {
    enum: EnumWithEscapedChars,
    @jsonName("also\"has\"quotes")
    someString: String,
}

@http(uri: "/escaped-string-values", method: "POST")
@httpRequestTests([
    {
        id: "EscapedStringValuesRequest",
        uri: "/escaped-string-values",
        method: "POST",
        protocol: "aws.protocols#restJson1",
        body: "{\"enum\":\"has\\\"quotes\",\"also\\\"has\\\"quotes\":\"test\"}",
        params: { enum: "has\"quotes", someString: "test" },
        appliesTo: "client",
    }
])
@httpResponseTests([
    {
        id: "EscapedStringValuesResponse",
        protocol: "aws.protocols#restJson1",
        code: 200,
        body: "{\"enum\":\"has\\\"quotes\",\"also\\\"has\\\"quotes\":\"test\"}",
        params: { enum: "has\"quotes", someString: "test" },
        appliesTo: "client",
    }
])
operation EscapedStringValues {
    input: EscapedStringValuesInputOutput,
    output: EscapedStringValuesInputOutput,
}

list NonSparseList {
    member: String,
}

map NonSparseMap {
    key: String,
    value: String,
}

union SingleElementUnion {
    a: String
}

structure NullInNonSparseOutput {
    list: NonSparseList,
    map: NonSparseMap,
    union: SingleElementUnion
}

@http(uri: "/null-in-non-sparse", method: "POST")
@httpResponseTests([
    {
        id: "NullInNonSparse",
        protocol: "aws.protocols#restJson1",
        code: 200,
        body: "{\"list\":[null,\"one\",null,\"two\",null],\"map\":{\"zero\":null,\"one\":\"1\"}}",
        params: { list: ["one", "two"], map: { "one": "1" } },
        appliesTo: "client",
    }
])
operation NullInNonSparse {
    output: NullInNonSparseOutput,
}

@http(uri: "/error-sensitive", method: "POST")
operation CaseInsensitiveErrorOperation {
    errors: [CaseInsensitiveError]
}

@httpResponseTests([
    {
        documentation: "Upper case error modeled lower case",
        id: "UpperErrorModeledLower",
        protocol: "aws.protocols#restJson1",
        code: 500,
        body: "{\"Message\": \"hello\"}",
        headers: { "X-Amzn-Errortype": "CaseInsensitiveError" },
        params: { message: "hello" },
        appliesTo: "client",
    }
])
@error("server")
structure CaseInsensitiveError {
    message: String
}

structure EmptyStruct {}
structure EmptyStructWithContentOnWireOpOutput {
    empty: EmptyStruct,
}

@http(uri: "/empty-struct-with-content-on-wire-op", method: "GET")
@httpResponseTests([
    {
        id: "EmptyStructWithContentOnWire",
        protocol: "aws.protocols#restJson1",
        code: 200,
        body: "{\"empty\": {\"value\":\"not actually empty\"}}",
        params: { empty: {} },
        appliesTo: "client",
    }
])
operation EmptyStructWithContentOnWireOp {
    output: EmptyStructWithContentOnWireOpOutput,
}

apply FixedMalformedAcceptWithGenericString @httpMalformedRequestTests([
    {
        id: "RestJsonWithPayloadExpectsImpliedAcceptFixed",
        documentation: """
        When there is a payload without a mediaType trait, the accept must match the
        implied content type of the shape.""",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/FixedMalformedAcceptWithGenericString",
            headers: {
                // this should be text/plain
                "accept": "application/json"
            }
        },
        response: {
            code: 406,
            headers: {
                "x-amzn-errortype": "NotAcceptableException"
            }
        },
        tags: [ "accept" ],
        appliesTo: "server",
    }
])

@suppress(["UnstableTrait"])
@http(method: "POST", uri: "/FixedMalformedAcceptWithGenericString")
operation FixedMalformedAcceptWithGenericString {
    input: FixedMalformedAcceptWithGenericStringInput
}

structure FixedMalformedAcceptWithGenericStringInput {
    @httpPayload
    payload: String
}

@idempotent
@http(method: "PUT", uri: "/service/{id}")
@documentation("Service register operation")
@httpRequestTests([
    {
        id: "FixedRegisterServiceRequestTest",
        protocol: "aws.protocols#restJson1",
        uri: "/service/1",
        headers: {
            "Content-Type": "application/json",
        },
        params: { id: "1", name: "TestService" },
        body: "{\"name\":\"TestService\"}",
        method: "PUT",
        appliesTo: "server",
    }
])
@httpResponseTests([
    {
        id: "FixedRegisterServiceResponseTest",
        protocol: "aws.protocols#restJson1",
        params: { id: "1", name: "TestService" },
        headers: {
            "Content-Type": "TestService",
        },
        body: "{\"id\":\"1\"}",
        code: 200,
        appliesTo: "server",
    }
])
operation FixedRegisterService {
    input: FixedRegisterServiceInputRequest,
    output: FixedRegisterServiceOutputResponse,
    errors: [FixedResourceAlreadyExists]
}

@documentation("Service register input structure")
structure FixedRegisterServiceInputRequest {
    @required
    @httpLabel
    id: ServiceId,
    name: ServiceName,
}

@documentation("Service register output structure")
structure FixedRegisterServiceOutputResponse {
    @required
    id: ServiceId,

    @required
    @httpHeader("Content-Type")
    name: ServiceName,
}

@error("client")
@documentation(
    """
    Returned when a new resource cannot be created because one already exists.
    """
)
structure FixedResourceAlreadyExists {
    @required
    message: String
}

@documentation("Id of the service that will be registered")
string ServiceId

@documentation("Name of the service that will be registered")
string ServiceName
