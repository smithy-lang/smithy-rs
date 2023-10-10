$version: "1.0"

namespace com.amazonaws.testservice

use aws.api#service
use aws.protocols#restJson1

@restJson1
@title("Test Service")
@service(sdkId: "Test")
@aws.auth#sigv4(name: "test-service")
service RequiredValues {
    operations: [TestOperation]
}

@http(method: "GET", uri: "/")
operation TestOperation {
    errors: [Error]
}

@error("client")
structure Error {
    @required
    requestId: String

    @required
    message: String
}
