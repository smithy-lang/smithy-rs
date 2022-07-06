$version: "1.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

@restJson1
@title("SimpleService")
@documentation("A simple service example, with a Service resource that can be registered and a readonly healthcheck")
service SimpleService {
    version: "2022-01-01",
    resources: [
        Service,
    ],
    operations: [
        Healthcheck,
        StoreServiceBlob,
    ],
}

@documentation("Id of the service that will be registered")
string ServiceId

@documentation("Name of the service that will be registered")
string ServiceName

@error("client")
@documentation(
    """
    Returned when a new resource cannot be created because one already exists.
    """
)
structure ResourceAlreadyExists {
    @required
    message: String
}

@documentation("A resource that can register services")
resource Service {
    identifiers: { id: ServiceId },
    put: RegisterService,
}

@idempotent
@http(method: "PUT", uri: "/service/{id}")
@documentation("Service register operation")
@httpRequestTests([
    {
        id: "RegisterServiceRequestTest",
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
        id: "RegisterServiceResponseTest",
        protocol: "aws.protocols#restJson1",
        params: { id: "1", name: "TestService" },
        body: "{\"id\":\"1\",\"name\":\"TestService\"}",
        code: 200,
        headers: {
            "Content-Length": "31"
        }
    }
])
operation RegisterService {
    input: RegisterServiceInputRequest,
    output: RegisterServiceOutputResponse,
    errors: [ResourceAlreadyExists]
}

@documentation("Service register input structure")
structure RegisterServiceInputRequest {
    @required
    @httpLabel
    id: ServiceId,
    name: ServiceName,
}

@documentation("Service register output structure")
structure RegisterServiceOutputResponse {
    @required
    id: ServiceId,
    name: ServiceName,
}

@readonly
@http(uri: "/healthcheck", method: "GET")
@documentation("Read-only healthcheck operation")
operation Healthcheck {
    input: HealthcheckInputRequest,
    output: HealthcheckOutputResponse
}

@documentation("Service healthcheck output structure")
structure HealthcheckInputRequest {

}

@documentation("Service healthcheck input structure")
structure HealthcheckOutputResponse {

}

@readonly
@http(method: "POST", uri: "/service/{id}/blob")
@documentation("Stores a blob for a service id")
operation StoreServiceBlob {
    input: StoreServiceBlobInput,
    output: StoreServiceBlobOutput
}

@documentation("Store a blob for a service id input structure")
structure StoreServiceBlobInput {
    @required
    @httpLabel
    id: ServiceId,
    @required
    @httpPayload
    content: Blob,
}

@documentation("Store a blob for a service id output structure")
structure StoreServiceBlobOutput {

}
