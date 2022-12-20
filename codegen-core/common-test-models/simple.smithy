$version: "2.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use smithy.framework#ValidationException

@restJson1
@title("SimpleService")
@documentation("A simple service example, with a Service resource that can be registered and a readonly healthcheck")
service SimpleService {
    version: "2022-01-01",
    operations: [
        HealthCheck,
    ],
}

@http(uri: "/", method: "POST")
operation HealthCheck {
    input: Input,
    errors: [ValidationException]
}

@input
structure Input {
    @default(15)
    int:ConstrainedInteger
}

@default(15)
@range(min: 10, max: 29)
integer ConstrainedInteger
