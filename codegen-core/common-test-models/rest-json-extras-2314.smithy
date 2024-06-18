$version: "1.0"

namespace aws.protocoltests.restjson

use aws.protocols#restJson1
use smithy.test#httpRequestTests

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
