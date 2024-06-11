$version: "2.0"

namespace aws.protocoltests.restjson

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use smithy.framework#ValidationException

@http(uri: "/EnumPayload2", method: "POST")
@httpRequestTests([
    {
        id: "RestJsonEnumPayloadRequest2",
        uri: "/EnumPayload2",
        headers: { "Content-Type": "text/plain" },
        body: "enumvalue",
        params: { payload: "enumvalue" },
        method: "POST",
        protocol: "aws.protocols#restJson1"
    }
])
@httpResponseTests([
    {
        id: "RestJsonEnumPayloadResponse2",
        headers: { "Content-Type": "text/plain" },
        body: "enumvalue",
        params: { payload: "enumvalue" },
        protocol: "aws.protocols#restJson1",
        code: 200
    }
])
operation HttpEnumPayload2 {
    input: EnumPayloadInput,
    output: EnumPayloadInput
    errors: [ValidationException]
}

@http(uri: "/StringPayload2", method: "POST")
@httpRequestTests([
    {
        id: "RestJsonStringPayloadRequest2",
        uri: "/StringPayload2",
        headers: { "Content-Type": "text/plain" },
        body: "rawstring",
        params: { payload: "rawstring" },
        method: "POST",
        protocol: "aws.protocols#restJson1"
    }
])
@httpResponseTests([
    {
        id: "RestJsonStringPayloadResponse2",
        headers: { "Content-Type": "text/plain" },
        body: "rawstring",
        params: { payload: "rawstring" },
        protocol: "aws.protocols#restJson1",
        code: 200
    }
])
operation HttpStringPayload2 {
    input: StringPayloadInput,
    output: StringPayloadInput
}
