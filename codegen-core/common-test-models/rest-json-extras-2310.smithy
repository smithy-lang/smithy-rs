$version: "1.0"

namespace aws.protocoltests.restjson

use aws.protocols#restJson1
use smithy.test#httpMalformedRequestTests

@http(method: "POST", uri: "/MalformedContentTypeWithBody")
operation MalformedContentTypeWithBody2 {
    input: GreetingStruct
}

structure GreetingStruct {
    salutation: String,
}

apply MalformedContentTypeWithBody2 @httpMalformedRequestTests([
    {
        id: "RestJsonWithBodyExpectsApplicationJsonContentTypeNoHeaders",
        documentation: "When there is modeled input, the content type must be application/json",
        protocol: restJson1,
        request: {
            method: "POST",
            uri: "/MalformedContentTypeWithBody",
            body: "{}",
        },
        response: {
            code: 415,
            headers: {
                "x-amzn-errortype": "UnsupportedMediaTypeException"
            }
        },
        tags: [ "content-type" ]
    }
])
