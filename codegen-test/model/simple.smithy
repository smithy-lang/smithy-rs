$version: "1.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1

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
    ],
}

@documentation("Id of the service that will be registered")
string ServiceId

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
operation RegisterService {
    input: RegisterServiceInput,
    output: RegisterServiceOutput,
    errors: [ResourceAlreadyExists]
}

@documentation("Service register input structure")
structure RegisterServiceInput {
    @required
    @httpLabel
    id: ServiceId,
}

@documentation("Service register output structure")
structure RegisterServiceOutput {
    @required
    id: ServiceId
}

@readonly
@http(uri: "/healthcheck", method: "GET")
@documentation("Read-only healthcheck operation")
operation Healthcheck {
    input: HealthcheckInput,
    output: HealthcheckOutput
}

@documentation("Service healthcheck output structure")
structure HealthcheckInput {

}

@documentation("Service healthcheck input structure")
structure HealthcheckOutput {

}
