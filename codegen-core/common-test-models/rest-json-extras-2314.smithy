$version: "1.0"

namespace aws.protocoltests.restjson

use aws.protocols#restJson1
use smithy.test#httpMalformedRequestTests
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

/// This example serializes a blob shape in the payload.
///
/// In this example, no JSON document is synthesized because the payload is
/// not a structure or a union type.
@http(uri: "/HttpPayloadTraits", method: "POST")
operation HttpPayloadTraits2 {
    input: HttpPayloadTraitsInputOutput,
    output: HttpPayloadTraitsInputOutput
}

apply HttpPayloadTraits2 @httpRequestTests([
    {
        id: "RestJsonHttpPayloadTraitsWithBlobAcceptsNoContentType",
        documentation: """
            Servers must accept no content type for blob inputs
            without the media type trait.""",
        protocol: restJson1,
        method: "POST",
        uri: "/HttpPayloadTraits",
        body: "This is definitely a jpeg",
        bodyMediaType: "application/octet-stream",
        headers: {
            "X-Foo": "Foo",
        },
        params: {
            foo: "Foo",
            blob: "This is definitely a jpeg"
        },
        appliesTo: "server",
        tags: [ "content-type" ]
    }
])

/// This example serializes a string shape in the payload.
///
/// In this example, no JSON document is synthesized because the payload is
/// not a structure or a union type.
@http(uri: "/HttpPayloadTraitOnString", method: "POST")
operation HttpPayloadTraitOnString2 {
    input: HttpPayloadTraitOnStringInputOutput,
    output: HttpPayloadTraitOnStringInputOutput
}

structure HttpPayloadTraitOnStringInputOutput {
    @httpPayload
    foo: String,
}

apply HttpPayloadTraitOnString2 @httpRequestTests([
    {
        id: "RestJsonHttpPayloadTraitOnString",
        documentation: "Serializes a string in the HTTP payload",
        protocol: restJson1,
        method: "POST",
        uri: "/HttpPayloadTraitOnString",
        body: "Foo",
        bodyMediaType: "text/plain",
        headers: {
            "Content-Type": "text/plain",
        },
        requireHeaders: [
            "Content-Length"
        ],
        params: {
            foo: "Foo",
        }
    },
])

apply HttpPayloadTraitOnString2 @httpResponseTests([
    {
        id: "RestJsonHttpPayloadTraitOnString",
        documentation: "Serializes a string in the HTTP payload",
        protocol: restJson1,
        code: 200,
        body: "Foo",
        bodyMediaType: "text/plain",
        headers: {
            "Content-Type": "text/plain",
        },
        params: {
            foo: "Foo",
        }
    },
])

apply HttpPayloadTraitOnString2 @httpMalformedRequestTests([
    {
        id: "RestJsonHttpPayloadTraitOnStringNoContentType",
        documentation: "Serializes a string in the HTTP payload without a content-type header",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/HttpPayloadTraitOnString",
            body: "Foo",
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
        id: "RestJsonHttpPayloadTraitOnStringWrongContentType",
        documentation: "Serializes a string in the HTTP payload without the expected content-type header",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/HttpPayloadTraitOnString",
            body: "Foo",
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
        id: "RestJsonHttpPayloadTraitOnStringUnsatisfiableAccept",
        documentation: "Serializes a string in the HTTP payload with an unstatisfiable accept header",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/HttpPayloadTraitOnString",
            body: "Foo",
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
