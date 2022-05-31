$version: "1.0"

namespace aws.protocoltests.restjson

use aws.protocols#restJson1
use aws.api#service
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use smithy.test#httpMalformedRequestTests

/// A REST JSON service that sends JSON requests and responses.
@service(sdkId: "Rest Json Protocol")
@restJson1
service RestJsonExtras {
    version: "2019-12-16",
    operations: [
        FixedMalformedAcceptWithGenericString,
        FixedRegisterService,
    ]
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
        tags: [ "accept" ]
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
