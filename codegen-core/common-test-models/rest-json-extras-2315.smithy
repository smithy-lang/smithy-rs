$version: "2.0"

namespace aws.protocoltests.restjson

use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use smithy.test#httpMalformedRequestTests
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
        body: "rawstring",
        bodyMediaType: "text/plain",
        headers: {
            "Content-Type": "text/plain",
        },
        requireHeaders: [
            "Content-Length"
        ],
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
        bodyMediaType: "text/plain",
        params: { payload: "rawstring" },
        protocol: "aws.protocols#restJson1",
        code: 200
    }
])
@httpMalformedRequestTests([
    {
        id: "RestJsonStringPayloadNoContentType2",
        documentation: "Serializes a string in the HTTP payload without a content-type header",
        protocol: "aws.protocols#restJson1",
        request: {
            method: "POST",
            uri: "/StringPayload2",
            body: "rawstring",
            // We expect a `Content-Type` header but none was provided.
        },
        response: {
            code: 415,
            headers: {
                "x-amzn-errortype": "UnsupportedMediaTypeException"
            }
        },
        tags: [ "content-type" ]
    },
    {
        id: "RestJsonStringPayloadWrongContentType2",
        documentation: "Serializes a string in the HTTP payload without the expected content-type header",
        protocol: "aws.protocols#restJson1",
        request: {
            method: "POST",
            uri: "/StringPayload2",
            body: "rawstring",
            headers: {
                // We expect `text/plain`.
                "Content-Type": "application/json",
            },
        },
        response: {
            code: 415,
            headers: {
                "x-amzn-errortype": "UnsupportedMediaTypeException"
            }
        },
        tags: [ "content-type" ]
    },
    {
        id: "RestJsonStringPayloadUnsatisfiableAccept2",
        documentation: "Serializes a string in the HTTP payload with an unstatisfiable accept header",
        protocol: "aws.protocols#restJson1",
        request: {
            method: "POST",
            uri: "/StringPayload2",
            body: "rawstring",
            headers: {
                "Content-Type": "text/plain",
                // We can't satisfy this requirement; the server will return `text/plain`.
                "Accept": "application/json",
            },
        },
        response: {
            code: 406,
            headers: {
                "x-amzn-errortype": "NotAcceptableException"
            }
        },
        tags: [ "accept" ]
    },
])
operation HttpStringPayload2 {
    input: StringPayloadInput,
    output: StringPayloadInput
}
